//! PostgreSQL adapter (spec §11) on tokio-postgres.
//!
//! Parameters are sent in text format with the exact OIDs chosen by the
//! runtime (OID 0 for NULL leaves the type to server inference). Results are
//! received in binary and converted to PostgreSQL text-format strings without
//! any floating-point step (numeric is decoded digit by digit).

use crate::builtins::db::db_error;
use crate::decimal::Decimal;
use crate::host::config::HostConfig;
use crate::host::{EffOutcome, EffValue, RawRows, SqlParam};
use base64::Engine;
use bytes::BytesMut;
use num_bigint::BigInt;
use postgres_types::{Format, IsNull, Kind, ToSql, Type};
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::rc::Rc;
use tokio::sync::Notify;

const BASELINE: &str =
    "SET client_encoding = 'UTF8'; SET standard_conforming_strings = on; SET default_transaction_isolation = 'read committed'";

struct Conn {
    client: Rc<tokio_postgres::Client>,
    pool: String,
    in_tx: bool,
}

struct PoolState {
    dsn: String,
    max: u32,
    open: u32,
    idle: Vec<String>,
    notify: Rc<Notify>,
}

#[derive(Default)]
struct Inner {
    pools: HashMap<String, PoolState>,
    conns: HashMap<String, Conn>,
    next: u64,
}

pub struct Pools {
    config: Rc<HostConfig>,
    inner: Rc<RefCell<Inner>>,
    /// Live-heap baseline for counting outstanding decoded row buffers
    /// against `memory_bytes` (§14.1).
    baseline: usize,
}

fn err_out(phase: &str, outcome: &str, sqlstate: Option<String>) -> EffOutcome {
    let extra = match sqlstate {
        Some(s) => vec![("sqlstate", crate::value::Value::str(&s))],
        None => vec![],
    };
    EffOutcome::Err(db_error(phase, outcome, extra))
}

fn map_err(e: &tokio_postgres::Error, phase: &str, in_tx: bool) -> EffOutcome {
    match e.as_db_error() {
        Some(db) => err_out(phase, if in_tx { "unknown" } else { "rolled_back" }, Some(db.code().code().to_string())),
        None => err_out(phase, "unknown", None),
    }
}

#[derive(Debug)]
struct TextParam {
    value: Option<Vec<u8>>,
}

impl ToSql for TextParam {
    fn to_sql(&self, _ty: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
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
    Type::from_oid(oid).unwrap_or_else(|| Type::new("unspecified".into(), oid, Kind::Pseudo, "pg_catalog".into()))
}

fn text_param(p: &SqlParam) -> TextParam {
    let value = p.value.as_ref().map(|v| {
        if p.oid == 17 {
            let raw = base64::engine::general_purpose::STANDARD.decode(v).unwrap_or_default();
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
        16 => (if raw.first() == Some(&1) { "t" } else { "f" }).to_string(),
        17 => {
            let mut s = String::from("\\x");
            for b in raw {
                s.push_str(&format!("{b:02x}"));
            }
            s
        }
        2950 if raw.len() == 16 => crate::builtins::effects::format_uuid_raw(raw),
        3802 => match raw.split_first() {
            Some((1, rest)) => String::from_utf8(rest.to_vec()).unwrap_or_else(|_| "\u{0}invalid".into()),
            _ => "\u{0}invalid".into(),
        },
        25 | 1043 | 1042 | 114 => String::from_utf8(raw.to_vec()).unwrap_or_else(|_| "\u{0}invalid".into()),
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

impl Pools {
    pub fn new(config: Rc<HostConfig>) -> Pools {
        Pools { config, inner: Rc::new(RefCell::new(Inner::default())), baseline: crate::alloc::live_bytes() }
    }

    pub fn pool(&mut self, alias: &str) -> EffOutcome {
        let cfg = match self.config.database_pools.iter().find(|(a, _)| a == alias) {
            Some((_, c)) => c.clone(),
            None => return err_out("pool", "not_sent", None),
        };
        let dsn = match std::env::var(&cfg.dsn_env) {
            Ok(d) => d,
            Err(_) => return err_out("pool", "not_sent", None),
        };
        let mut inner = self.inner.borrow_mut();
        inner.pools.entry(alias.to_string()).or_insert_with(|| PoolState {
            dsn,
            max: cfg.max_connections,
            open: 0,
            idle: Vec::new(),
            notify: Rc::new(Notify::new()),
        });
        EffOutcome::Ok(EffValue::Str(alias.to_string()))
    }

    pub fn acquire(&mut self, pool: String) -> impl Future<Output = EffOutcome> {
        let inner = self.inner.clone();
        async move {
            loop {
                let (dsn, notify) = {
                    let mut i = inner.borrow_mut();
                    let p = match i.pools.get_mut(&pool) {
                        Some(p) => p,
                        None => return err_out("pool", "not_sent", None),
                    };
                    if let Some(c) = p.idle.pop() {
                        return EffOutcome::Ok(EffValue::Str(c));
                    }
                    if p.open < p.max {
                        p.open += 1;
                        (Some(p.dsn.clone()), p.notify.clone())
                    } else {
                        (None, p.notify.clone())
                    }
                };
                let dsn = match dsn {
                    Some(d) => d,
                    None => {
                        notify.notified().await;
                        continue;
                    }
                };
                let connected = async {
                    let (client, connection) = tokio_postgres::connect(&dsn, tls_connector()).await?;
                    tokio::task::spawn_local(async move {
                        let _ = connection.await;
                    });
                    client.batch_execute(BASELINE).await?;
                    Ok::<_, tokio_postgres::Error>(client)
                }
                .await;
                let mut i = inner.borrow_mut();
                match connected {
                    Ok(client) => {
                        i.next += 1;
                        let id = format!("{pool}#{}", i.next);
                        i.conns.insert(id.clone(), Conn { client: Rc::new(client), pool: pool.clone(), in_tx: false });
                        return EffOutcome::Ok(EffValue::Str(id));
                    }
                    Err(_) => {
                        if let Some(p) = i.pools.get_mut(&pool) {
                            p.open -= 1;
                            p.notify.notify_one();
                        }
                        return err_out("pool", "not_sent", None);
                    }
                }
            }
        }
    }

    pub fn cancel_token(&self, conn: &str) -> Option<tokio_postgres::CancelToken> {
        self.inner.borrow().conns.get(conn).map(|c| c.client.cancel_token())
    }

    fn client(inner: &Rc<RefCell<Inner>>, conn: &str) -> Option<(Rc<tokio_postgres::Client>, bool)> {
        inner.borrow().conns.get(conn).map(|c| (c.client.clone(), c.in_tx))
    }

    pub fn simple(&mut self, conn: String, sql: &'static str, phase: &'static str) -> impl Future<Output = EffOutcome> {
        let inner = self.inner.clone();
        async move {
            let (client, in_tx) = match Self::client(&inner, &conn) {
                Some(c) => c,
                None => return err_out(phase, "not_sent", None),
            };
            match client.batch_execute(sql).await {
                Ok(()) => {
                    if let Some(c) = inner.borrow_mut().conns.get_mut(&conn) {
                        c.in_tx = phase == "begin";
                    }
                    EffOutcome::Ok(EffValue::Null)
                }
                Err(e) => map_err(&e, phase, in_tx || phase == "rollback"),
            }
        }
    }

    pub fn commit(&mut self, conn: String) -> impl Future<Output = EffOutcome> {
        let inner = self.inner.clone();
        async move {
            let (client, _) = match Self::client(&inner, &conn) {
                Some(c) => c,
                None => return err_out("commit", "not_sent", None),
            };
            match client.batch_execute("COMMIT").await {
                Ok(()) => {
                    if let Some(c) = inner.borrow_mut().conns.get_mut(&conn) {
                        c.in_tx = false;
                    }
                    EffOutcome::Ok(EffValue::Null)
                }
                Err(e) => match e.as_db_error() {
                    // A server rejection of COMMIT ends the transaction rolled back.
                    Some(db) => err_out("commit", "rolled_back", Some(db.code().code().to_string())),
                    None => err_out("commit", "unknown", None),
                },
            }
        }
    }

    pub fn release(&mut self, conn: String, discard: bool) -> impl Future<Output = EffOutcome> {
        let inner = self.inner.clone();
        async move {
            let (client, pool) = match inner.borrow().conns.get(&conn) {
                Some(c) => (c.client.clone(), c.pool.clone()),
                None => return EffOutcome::Ok(EffValue::Null),
            };
            let healthy = if discard {
                false
            } else {
                client.batch_execute("DISCARD ALL").await.is_ok() && client.batch_execute(BASELINE).await.is_ok()
            };
            let mut i = inner.borrow_mut();
            if healthy {
                if let Some(p) = i.pools.get_mut(&pool) {
                    p.idle.push(conn);
                    p.notify.notify_one();
                }
            } else {
                i.conns.remove(&conn);
                if let Some(p) = i.pools.get_mut(&pool) {
                    p.open = p.open.saturating_sub(1);
                    p.notify.notify_one();
                }
            }
            EffOutcome::Ok(EffValue::Null)
        }
    }

    pub fn execute(&mut self, conn: String, sql: String, params: Vec<SqlParam>, max_rows: u64) -> impl Future<Output = EffOutcome> {
        let inner = self.inner.clone();
        let (baseline, limit) = (self.baseline, self.config.limits.memory_bytes);
        async move {
            let (client, in_tx) = match Self::client(&inner, &conn) {
                Some(c) => c,
                None => return err_out("execute", "not_sent", None),
            };
            let types: Vec<Type> = params.iter().map(|p| param_type(p.oid)).collect();
            let stmt = match client.prepare_typed(&sql, &types).await {
                Ok(s) => s,
                Err(e) => return map_err(&e, "execute", in_tx),
            };
            let tp: Vec<TextParam> = params.iter().map(text_param).collect();
            let refs: Vec<&(dyn ToSql + Sync)> = tp.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
            let columns: Vec<(String, u32)> = stmt.columns().iter().map(|c| (c.name().to_string(), c.type_().oid())).collect();
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
                for (i, (_, oid)) in columns.iter().enumerate() {
                    let cell: Option<RawCell> = row.try_get(i).unwrap_or(None);
                    cells.push(cell.map(|c| cell_text(*oid, &c.0)));
                }
                rows.push(cells);
            }
            if truncated {
                let _ = client.cancel_token().cancel_query(tls_connector()).await;
                // The statement was cancelled mid-result: commit state is unknown.
                let outcome = "unknown";
                let count = rows.len().to_string();
                return EffOutcome::Ok(EffValue::Rows(RawRows { columns, rows, count, outcome: outcome.into() }));
            }
            let count = stream.rows_affected().map(|n| n.to_string()).unwrap_or_default();
            let outcome = if in_tx { "unknown" } else { "committed" };
            EffOutcome::Ok(EffValue::Rows(RawRows { columns, rows, count, outcome: outcome.into() }))
        }
    }
}
