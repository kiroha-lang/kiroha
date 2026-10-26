//! PostgreSQL adapter (spec §11) on tokio-postgres.
//!
//! Parameters are sent in text format with the exact OIDs chosen by the
//! runtime (OID 0 for NULL leaves the type to server inference). Results are
//! received in binary and converted to PostgreSQL text-format strings without
//! any floating-point step (numeric is decoded digit by digit).

use super::database::{Connection, RowBudget, TransactionOp};
use crate::builtins::db::db_error;
use crate::database::SqlType;
use crate::decimal::Decimal;
use crate::host::{EffOutcome, EffValue, RawRows, SqlParam};
use base64::Engine;
use bytes::BytesMut;
use futures_util::future::LocalBoxFuture;
use num_bigint::BigInt;
use postgres_types::{Format, IsNull, Kind, ToSql, Type};
use std::error::Error;
use std::rc::Rc;

pub(crate) fn column_kind(oid: u32) -> SqlType {
    match oid {
        16 => SqlType::Bool,
        20 | 21 | 23 => SqlType::Int,
        1700 => SqlType::Num,
        25 | 1043 | 1042 | 2950 => SqlType::Str,
        17 => SqlType::Bytes,
        114 | 3802 => SqlType::Json,
        _ => SqlType::Unsupported,
    }
}
pub(crate) fn param_oid(kind: SqlType) -> u32 {
    match kind {
        SqlType::Null => 0,
        SqlType::Bool => 16,
        SqlType::Int | SqlType::Num => 1700,
        SqlType::Str => 25,
        SqlType::Bytes => 17,
        SqlType::Json => 3802,
        SqlType::Unsupported => 0,
    }
}

const BASELINE: &str =
    "SET client_encoding = 'UTF8'; SET standard_conforming_strings = on; SET default_transaction_isolation = 'read committed'";

fn err_out(phase: &str, outcome: &str, sqlstate: Option<String>) -> EffOutcome {
    let extra = match sqlstate {
        Some(s) => vec![("sqlstate", crate::value::Value::str(&s))],
        None => vec![],
    };
    EffOutcome::Err(db_error(phase, outcome, extra))
}

fn map_err(e: &tokio_postgres::Error, phase: &str, in_tx: bool) -> EffOutcome {
    match e.as_db_error() {
        Some(db) => err_out(
            phase,
            if in_tx { "unknown" } else { "rolled_back" },
            Some(db.code().code().to_string()),
        ),
        None => err_out(phase, "unknown", None),
    }
}

#[derive(Debug)]
struct TextParam {
    value: Option<Vec<u8>>,
}

impl ToSql for TextParam {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match &self.value {
            None => Ok(IsNull::Yes),
            Some(v) => {
                out.extend_from_slice(v);
                Ok(IsNull::No)
            }
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    fn encode_format(&self, _ty: &Type) -> Format {
        Format::Text
    }

    postgres_types::to_sql_checked!();
}

struct RawCell(Vec<u8>);

impl<'a> postgres_types::FromSql<'a> for RawCell {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Ok(RawCell(raw.to_vec()))
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

fn param_type(oid: u32) -> Type {
    Type::from_oid(oid)
        .unwrap_or_else(|| Type::new("unspecified".into(), oid, Kind::Pseudo, "pg_catalog".into()))
}

fn text_param(p: &SqlParam) -> TextParam {
    let value = p.value.as_ref().map(|v| {
        if p.kind == SqlType::Bytes {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(v)
                .unwrap_or_default();
            let mut s = String::from("\\x");
            for b in raw {
                s.push_str(&format!("{b:02x}"));
            }
            s.into_bytes()
        } else {
            v.clone().into_bytes()
        }
    });
    TextParam { value }
}

/// PostgreSQL binary numeric → exact decimal text (or NaN/Infinity).
fn numeric_text(raw: &[u8]) -> String {
    if raw.len() < 8 {
        return "invalid".into();
    }
    let rd = |i: usize| u16::from_be_bytes([raw[i], raw[i + 1]]);
    let ndigits = rd(0) as usize;
    let weight = rd(2) as i16 as i64;
    let sign = rd(4);
    match sign {
        0xC000 => return "NaN".into(),
        0xD000 => return "Infinity".into(),
        0xF000 => return "-Infinity".into(),
        _ => {}
    }
    if raw.len() < 8 + 2 * ndigits {
        return "invalid".into();
    }
    let mut coef = BigInt::from(0);
    for k in 0..ndigits {
        coef = coef * 10_000u32 + BigInt::from(rd(8 + 2 * k));
    }
    if sign == 0x4000 {
        coef = -coef;
    }
    let exp = 4 * (weight - ndigits as i64 + 1);
    Decimal::normalize(coef, exp).canonical()
}

fn cell_text(oid: u32, raw: &[u8]) -> String {
    match oid {
        21 if raw.len() == 2 => i16::from_be_bytes([raw[0], raw[1]]).to_string(),
        23 if raw.len() == 4 => i32::from_be_bytes(raw.try_into().unwrap()).to_string(),
        20 if raw.len() == 8 => i64::from_be_bytes(raw.try_into().unwrap()).to_string(),
        1700 => numeric_text(raw),
        16 => (if raw.first() == Some(&1) {
            "true"
        } else {
            "false"
        })
        .to_string(),
        17 => base64::engine::general_purpose::STANDARD.encode(raw),
        2950 if raw.len() == 16 => crate::builtins::effects::format_uuid_raw(raw),
        3802 => match raw.split_first() {
            Some((1, rest)) => {
                String::from_utf8(rest.to_vec()).unwrap_or_else(|_| "\u{0}invalid".into())
            }
            _ => "\u{0}invalid".into(),
        },
        25 | 1043 | 1042 | 114 => {
            String::from_utf8(raw.to_vec()).unwrap_or_else(|_| "\u{0}invalid".into())
        }
        _ => String::new(),
    }
}

/// TLS for PostgreSQL follows the DSN's `sslmode` (default `prefer`), with
/// certificates verified against the same trust store as HTTPS.
pub fn tls_connector() -> tokio_postgres_rustls::MakeRustlsConnect {
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("TLS protocol versions")
        .with_root_certificates(super::http::root_store())
        .with_no_client_auth();
    tokio_postgres_rustls::MakeRustlsConnect::new(cfg)
}

pub struct PgConnection {
    client: Rc<tokio_postgres::Client>,
}
impl PgConnection {
    pub async fn connect(dsn: &str) -> Result<Self, EffOutcome> {
        let (client, connection) = tokio_postgres::connect(dsn, tls_connector())
            .await
            .map_err(|_| err_out("pool", "not_sent", None))?;
        tokio::task::spawn_local(async move {
            let _ = connection.await;
        });
        client
            .batch_execute(BASELINE)
            .await
            .map_err(|_| err_out("pool", "not_sent", None))?;
        Ok(Self {
            client: Rc::new(client),
        })
    }
}
impl Connection for PgConnection {
    fn transaction(&self, op: TransactionOp, in_tx: bool) -> LocalBoxFuture<'static, EffOutcome> {
        let client = self.client.clone();
        Box::pin(async move {
            let sql = match op {
                TransactionOp::Begin => "BEGIN ISOLATION LEVEL READ COMMITTED",
                TransactionOp::Commit => "COMMIT",
                TransactionOp::Rollback => "ROLLBACK",
            };
            match client.batch_execute(sql).await {
                Ok(()) => EffOutcome::Ok(EffValue::Null),
                Err(e) if op == TransactionOp::Commit => match e.as_db_error() {
                    Some(db) => err_out("commit", "rolled_back", Some(db.code().code().into())),
                    None => err_out("commit", "unknown", None),
                },
                Err(e) => map_err(&e, op.phase(), in_tx || op == TransactionOp::Rollback),
            }
        })
    }
    fn reset(&self) -> LocalBoxFuture<'static, bool> {
        let client = self.client.clone();
        Box::pin(async move {
            client.batch_execute("DISCARD ALL").await.is_ok()
                && client.batch_execute(BASELINE).await.is_ok()
        })
    }
    fn cancel(&self) -> LocalBoxFuture<'static, ()> {
        let cancel = self.client.cancel_token();
        Box::pin(async move {
            let _ = cancel.cancel_query(tls_connector()).await;
        })
    }
    fn execute(
        &self,
        sql: String,
        params: Vec<SqlParam>,
        max_rows: u64,
        in_tx: bool,
        budget: RowBudget,
    ) -> LocalBoxFuture<'static, EffOutcome> {
        let client = self.client.clone();
        let (baseline, limit) = (budget.baseline, budget.limit);
        Box::pin(async move {
            let types: Vec<Type> = params
                .iter()
                .map(|p| param_type(param_oid(p.kind)))
                .collect();
            let stmt = match client.prepare_typed(&sql, &types).await {
                Ok(s) => s,
                Err(e) => return map_err(&e, "execute", in_tx),
            };
            let tp: Vec<TextParam> = params.iter().map(text_param).collect();
            let refs: Vec<&(dyn ToSql + Sync)> =
                tp.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
            let oids: Vec<u32> = stmt.columns().iter().map(|c| c.type_().oid()).collect();
            let columns: Vec<_> = stmt
                .columns()
                .iter()
                .map(|c| (c.name().to_string(), column_kind(c.type_().oid())))
                .collect();
            let stream = match client.query_raw(&stmt, refs).await {
                Ok(s) => s,
                Err(e) => return map_err(&e, "execute", in_tx),
            };
            use futures_util::StreamExt;
            let mut stream = Box::pin(stream);
            let mut rows = Vec::new();
            let mut truncated = false;
            while let Some(r) = stream.next().await {
                if rows.len() % 1024 == 1023
                    && crate::alloc::live_bytes().saturating_sub(baseline) as u64 > limit
                {
                    // Outstanding decoded buffers exceed the memory quota.
                    let _ = client.cancel_token().cancel_query(tls_connector()).await;
                    return EffOutcome::Resource("memory".into());
                }
                if rows.len() as u64 > max_rows {
                    // The runtime raises `limit`; stop buffering and cancel
                    // the rest of the statement on the server.
                    truncated = true;
                    break;
                }
                let row = match r {
                    Ok(r) => r,
                    Err(e) => return map_err(&e, "execute", in_tx),
                };
                let mut cells = Vec::with_capacity(columns.len());
                for (i, oid) in oids.iter().enumerate() {
                    let cell: Option<RawCell> = row.try_get(i).unwrap_or(None);
                    let large = cell.as_ref().is_some_and(|c| c.0.len() > 4096);
                    cells.push(cell.map(|c| cell_text(*oid, &c.0)));
                    if large {
                        // Converting a large cell is CPU work: yield so the
                        // runtime's timers (operation timeout, deadlines) and
                        // cancellation can run (§8.3).
                        tokio::task::yield_now().await;
                    }
                }
                rows.push(cells);
                if rows.len() % 256 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            if truncated {
                let _ = client.cancel_token().cancel_query(tls_connector()).await;
                // The statement was cancelled mid-result: commit state is unknown.
                let outcome = "unknown";
                let count = rows.len().to_string();
                return EffOutcome::Ok(EffValue::Rows(RawRows {
                    columns,
                    rows,
                    count,
                    outcome: outcome.into(),
                }));
            }
            let count = stream
                .rows_affected()
                .map(|n| n.to_string())
                .unwrap_or_default();
            let outcome = if in_tx { "unknown" } else { "committed" };
            EffOutcome::Ok(EffValue::Rows(RawRows {
                columns,
                rows,
                count,
                outcome: outcome.into(),
            }))
        })
    }
}
