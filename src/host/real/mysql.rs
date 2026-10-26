//! MySQL/MariaDB protocol adapter. Exact decimals never pass through a float.
use super::database::{Connection, RowBudget, TransactionOp};
use crate::builtins::db::db_error;
use crate::database::SqlType;
use crate::host::{EffOutcome, EffValue, RawRows, SqlParam};
use crate::value::Value;
use base64::Engine;
use futures_util::future::LocalBoxFuture;
use mysql_async::consts::ColumnType;
use mysql_async::{prelude::Queryable, Column, Conn, Opts, OptsBuilder, Params};
use std::cell::Cell;
use std::rc::Rc;
use tokio::sync::Mutex;

const BASELINE: &[&str] = &[
    "SET NAMES utf8mb4 COLLATE utf8mb4_bin",
    "SET SESSION sql_mode = 'STRICT_TRANS_TABLES,NO_ENGINE_SUBSTITUTION'",
    "SET SESSION TRANSACTION ISOLATION LEVEL READ COMMITTED",
    "SET autocommit = 1",
];
pub struct MySqlConnection {
    client: Rc<Mutex<Conn>>,
    opts: Opts,
    id: u32,
    broken: Rc<Cell<bool>>,
}
fn error(phase: &str, outcome: &str) -> EffOutcome {
    EffOutcome::Err(db_error(phase, outcome, vec![]))
}
fn map_err(e: mysql_async::Error, phase: &str, in_tx: bool) -> EffOutcome {
    match e {
        mysql_async::Error::Server(e) => {
            let outcome = if phase == "commit" {
                if e.code == 1213 {
                    "rolled_back"
                } else {
                    "unknown"
                }
            } else if in_tx {
                "unknown"
            } else {
                "rolled_back"
            };
            EffOutcome::Err(db_error(
                phase,
                outcome,
                vec![
                    ("sqlstate", Value::str(&e.state)),
                    ("errno", Value::int(e.code as i64)),
                ],
            ))
        }
        _ => error(phase, "unknown"),
    }
}
async fn baseline(conn: &mut Conn) -> Result<(), mysql_async::Error> {
    for sql in BASELINE {
        conn.query_drop(*sql).await?;
    }
    Ok(())
}
async fn kill(opts: Opts, id: u32) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(2), async move {
        let mut control = Conn::new(opts).await?;
        control.query_drop(format!("KILL QUERY {id}")).await?;
        control.disconnect().await
    })
    .await
    .is_ok_and(|result: Result<(), mysql_async::Error>| result.is_ok())
}
fn bind(params: &[SqlParam]) -> Result<Params, EffOutcome> {
    use mysql_async::Value as M;
    let mut result = Vec::with_capacity(params.len());
    for p in params {
        let value = match (&p.kind, &p.value) {
            (_, None) => M::NULL,
            (SqlType::Bool, Some(s)) => match s.as_str() {
                "true" => M::Int(1),
                "false" => M::Int(0),
                _ => return Err(error("bind", "not_sent")),
            },
            (SqlType::Int | SqlType::Num, Some(s)) => {
                let d = crate::decimal::Decimal::parse_json(s)
                    .ok()
                    .flatten()
                    .ok_or_else(|| error("bind", "not_sent"))?;
                match d.to_i128() {
                    Some(n) if i64::try_from(n).is_ok() => M::Int(n as i64),
                    Some(n) if u64::try_from(n).is_ok() => M::UInt(n as u64),
                    _ => M::Bytes(s.as_bytes().to_vec()),
                }
            }
            (SqlType::Str | SqlType::Json, Some(s)) => M::Bytes(s.as_bytes().to_vec()),
            (SqlType::Bytes, Some(s)) => M::Bytes(
                base64::engine::general_purpose::STANDARD
                    .decode(s)
                    .map_err(|_| error("bind", "not_sent"))?,
            ),
            _ => return Err(error("bind", "not_sent")),
        };
        result.push(value);
    }
    Ok(Params::Positional(result))
}
fn column_kind(c: &Column) -> SqlType {
    use ColumnType::*;
    match c.column_type() {
        MYSQL_TYPE_NULL => SqlType::Null,
        MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_LONG | MYSQL_TYPE_LONGLONG
        | MYSQL_TYPE_INT24 | MYSQL_TYPE_YEAR => SqlType::Int,
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => SqlType::Num,
        MYSQL_TYPE_JSON => SqlType::Json,
        MYSQL_TYPE_BIT => SqlType::Bytes,
        MYSQL_TYPE_STRING
        | MYSQL_TYPE_VAR_STRING
        | MYSQL_TYPE_VARCHAR
        | MYSQL_TYPE_TINY_BLOB
        | MYSQL_TYPE_MEDIUM_BLOB
        | MYSQL_TYPE_LONG_BLOB
        | MYSQL_TYPE_BLOB
        | MYSQL_TYPE_ENUM
        | MYSQL_TYPE_SET => {
            if c.character_set() == 63 {
                SqlType::Bytes
            } else {
                SqlType::Str
            }
        }
        _ => SqlType::Unsupported,
    }
}
fn cell(value: mysql_async::Value, kind: SqlType) -> Result<Option<String>, ()> {
    use mysql_async::Value as M;
    Ok(Some(match value {
        M::NULL => return Ok(None),
        M::Int(n) => n.to_string(),
        M::UInt(n) => n.to_string(),
        M::Bytes(bytes) => {
            if kind == SqlType::Bytes {
                base64::engine::general_purpose::STANDARD.encode(bytes)
            } else {
                String::from_utf8(bytes).map_err(|_| ())?
            }
        }
        // Unsupported floating/date/time types are rejected by row conversion
        // even when the value is NULL. Never round them via a Rust float.
        _ if kind == SqlType::Unsupported => String::new(),
        _ => return Err(()),
    }))
}
impl MySqlConnection {
    pub async fn connect(dsn: &str) -> Result<Self, EffOutcome> {
        let normalized = dsn
            .strip_prefix("mariadb://")
            .map(|s| format!("mysql://{s}"));
        let opts = Opts::from_url(normalized.as_deref().unwrap_or(dsn))
            .map_err(|_| error("pool", "not_sent"))?;
        let ssl = opts.ssl_opts().cloned().map(|ssl| {
            if !ssl.disable_built_in_roots()
                || std::env::var_os("SSL_CERT_FILE").is_some()
                || std::env::var_os("SSL_CERT_DIR").is_some()
            {
                ssl.with_root_certs(
                    rustls_native_certs::load_native_certs()
                        .certs
                        .into_iter()
                        .map(|cert| cert.as_ref().to_vec().into())
                        .collect(),
                )
            } else {
                ssl
            }
        });
        let opts: Opts = OptsBuilder::from_opts(opts)
            .ssl_opts(ssl)
            .prefer_socket(false)
            .into();
        let mut conn = Conn::new(opts.clone())
            .await
            .map_err(|_| error("pool", "not_sent"))?;
        baseline(&mut conn)
            .await
            .map_err(|_| error("pool", "not_sent"))?;
        let id = conn.id();
        Ok(Self {
            client: Rc::new(Mutex::new(conn)),
            opts,
            id,
            broken: Rc::new(Cell::new(false)),
        })
    }
}
impl Connection for MySqlConnection {
    fn transaction(&self, op: TransactionOp, in_tx: bool) -> LocalBoxFuture<'static, EffOutcome> {
        let client = self.client.clone();
        Box::pin(async move {
            let mut conn = client.lock().await;
            let sql = match op {
                TransactionOp::Begin => "START TRANSACTION",
                TransactionOp::Commit => "COMMIT",
                TransactionOp::Rollback => "ROLLBACK",
            };
            match conn.query_drop(sql).await {
                Ok(()) => EffOutcome::Ok(EffValue::Null),
                Err(e) => map_err(e, op.phase(), in_tx || op == TransactionOp::Rollback),
            }
        })
    }
    fn reset(&self) -> LocalBoxFuture<'static, bool> {
        let client = self.client.clone();
        let broken = self.broken.clone();
        Box::pin(async move {
            if broken.get() {
                return false;
            }
            let mut conn = client.lock().await;
            matches!(conn.reset().await, Ok(true)) && baseline(&mut conn).await.is_ok()
        })
    }
    fn cancel(&self) -> LocalBoxFuture<'static, ()> {
        let opts = self.opts.clone();
        let id = self.id;
        let broken = self.broken.clone();
        Box::pin(async move {
            broken.set(true);
            let _ = kill(opts, id).await;
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
        let opts = self.opts.clone();
        let id = self.id;
        let broken = self.broken.clone();
        Box::pin(async move {
            let params = match bind(&params) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let mut conn = client.lock().await;
            let mut result = match conn.exec_iter(sql, params).await {
                Ok(r) => r,
                Err(e) => return map_err(e, "execute", in_tx),
            };
            let columns: Result<Vec<_>, _> = result
                .columns_ref()
                .iter()
                .map(|c| {
                    std::str::from_utf8(c.name_ref()).map(|name| (name.to_owned(), column_kind(c)))
                })
                .collect();
            let columns = match columns {
                Ok(columns) => columns,
                Err(_) => {
                    // No command-completion acknowledgment has been read yet.
                    drop(result);
                    broken.set(true);
                    let _ = kill(opts, id).await;
                    return error("decode", "unknown");
                }
            };
            let mut rows = Vec::new();
            let mut truncated = false;
            loop {
                let row = match result.next().await {
                    Ok(Some(r)) => r,
                    Ok(None) => break,
                    Err(e) => return map_err(e, "execute", in_tx),
                };
                let values = row.unwrap();
                let mut cells = Vec::with_capacity(values.len());
                for (value, (_, kind)) in values.into_iter().zip(&columns) {
                    match cell(value, *kind) {
                        Ok(v) => cells.push(v),
                        Err(()) => {
                            drop(result);
                            broken.set(true);
                            let _ = kill(opts, id).await;
                            return error("decode", "unknown");
                        }
                    }
                }
                rows.push(cells);
                if crate::alloc::live_bytes().saturating_sub(budget.baseline) as u64 > budget.limit
                {
                    drop(result);
                    broken.set(true);
                    let _ = kill(opts, id).await;
                    return EffOutcome::Resource("memory".into());
                }
                if rows.len() as u64 > max_rows {
                    truncated = true;
                    break;
                }
                tokio::task::yield_now().await;
            }
            let count = if columns.is_empty() {
                result.affected_rows().to_string()
            } else {
                rows.len().to_string()
            };
            drop(result);
            if truncated {
                broken.set(true);
                let _ = kill(opts, id).await;
            }
            let outcome = if in_tx || truncated {
                "unknown"
            } else {
                "committed"
            };
            EffOutcome::Ok(EffValue::Rows(RawRows {
                columns,
                rows,
                count,
                outcome: outcome.into(),
            }))
        })
    }
}
