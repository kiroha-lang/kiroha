//! `sql` and `tx` (spec §11.2–11.5).

use super::args::*;
use crate::host::{EffOutcome, EffValue, Effect, Host, RawRows, SqlParam};
use crate::runtime::frames::Frame;
use crate::runtime::{timeout_error, AbortReason, AbortSig, Ctl, DispatchOpts, Fail, Runtime, ScopeId, Task, TxState};
use crate::sqlcheck;
use crate::value::{err, Record, TxHandle, Value};
use base64::Engine;
use std::rc::Rc;

pub fn db_error(phase: &str, outcome: &str, extra: Vec<(&str, Value)>) -> Value {
    let mut data = vec![("phase", Value::str(phase)), ("outcome", Value::str(outcome))];
    data.extend(extra);
    err::make("db", format!("database {phase} failure ({outcome})"), data)
}

#[derive(Debug)]
pub enum SqlPhase {
    Acquiring,
    Executing,
    Releasing,
}

pub struct SqlFrame {
    pub phase: SqlPhase,
    pub tx: Option<u64>,
    pub conn: Option<String>,
    pub sql: String,
    pub params: Vec<SqlParam>,
    pub max_rows: u64,
    pub deadline: u64,
    pub timeout: u64,
    pub primary: Option<Result<Value, Value>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TxPhase {
    Acquiring,
    Beginning,
    Callback,
    Committing,
    RollingBack,
    Releasing,
}

pub enum Primary {
    Value(Value),
    Error(Value),
    Timeout,
    Abort(AbortSig),
}

pub struct TxFrame {
    pub phase: TxPhase,
    pub pool: String,
    pub timeout: u64,
    pub scope: ScopeId,
    pub prev_scope: ScopeId,
    pub f: Value,
    pub conn: Option<String>,
    pub tx: Option<u64>,
    pub primary: Option<Primary>,
    /// Outcome reported with a tx-scope timeout (§11.4).
    pub timeout_outcome: &'static str,
}

/// Bind a Kiri value as a PostgreSQL parameter (§11.3).
pub fn bind_param(v: &Value) -> Result<SqlParam, Value> {
    let bind_err = || db_error("bind", "not_sent", vec![]);
    Ok(match v {
        Value::Null => SqlParam { oid: 0, value: None },
        Value::Bool(b) => SqlParam { oid: 16, value: Some(b.to_string()) },
        Value::Str(s) => {
            if s.contains('\0') {
                return Err(bind_err());
            }
            SqlParam { oid: 25, value: Some(s.to_string()) }
        }
        Value::Num(d) => SqlParam { oid: 1700, value: Some(d.canonical()) },
        Value::Bytes(b) => SqlParam { oid: 17, value: Some(base64::engine::general_purpose::STANDARD.encode(&**b)) },
        Value::Array(_) | Value::Record(_) => {
            if !v.is_json() {
                return Err(bind_err());
            }
            SqlParam { oid: 3802, value: Some(crate::json::dump(v)) }
        }
        _ => return Err(bind_err()),
    })
}

const SUPPORTED_OIDS: &[u32] = &[21, 23, 20, 1700, 25, 1043, 1042, 2950, 16, 17, 114, 3802];

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let h = s.strip_prefix("\\x")?;
    if h.len() % 2 != 0 {
        return None;
    }
    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok()).collect()
}

/// Convert driver rows to `{rows,count}` (§11.3–11.4).
/// Errors are ordinary `db`/`limit` errors, except values beyond the host
/// number quota, which are resource aborts (§3.2), and interrupt trips.
pub fn convert_rows(raw: &RawRows, max_rows: u64) -> Result<Value, Fail> {
    let outcome = raw.outcome.as_str();
    let decode = || Fail::Err(db_error("decode", outcome, vec![]));
    let mut names: Vec<&str> = Vec::new();
    for (n, oid) in &raw.columns {
        if names.contains(&n.as_str()) {
            return Err(decode());
        }
        names.push(n);
        if !SUPPORTED_OIDS.contains(oid) {
            return Err(decode());
        }
    }
    if raw.count.is_empty() || !raw.count.bytes().all(|c| c.is_ascii_digit()) {
        return Err(decode());
    }
    let count = crate::decimal::Decimal::parse_json(raw.count.trim_start_matches('0'))
        .ok()
        .flatten()
        .unwrap_or_else(crate::decimal::Decimal::zero);
    if raw.rows.len() as u64 > max_rows {
        return Err(Fail::Err(err::make(
            "limit",
            "result rows exceed max_rows",
            vec![
                ("operation", Value::str("sql")),
                ("limit", Value::str("max_rows")),
                ("maximum", Value::int(max_rows as i64)),
                ("outcome", Value::str(outcome)),
            ],
        )));
    }
    let mut rows = Vec::with_capacity(raw.rows.len());
    for row in &raw.rows {
        if crate::interrupt::poll() {
            return Err(Fail::Interrupted);
        }
        if row.len() != raw.columns.len() {
            return Err(decode());
        }
        let mut r = Record::with_capacity(row.len());
        for ((name, oid), cell) in raw.columns.iter().zip(row) {
            let v = match cell {
                None => Value::Null,
                Some(text) => match oid {
                    21 | 23 | 20 | 1700 => match crate::decimal::Decimal::parse_json(text) {
                        Ok(Some(d)) => Value::num(d),
                        Err(_) => return Err(Fail::Abort(AbortReason::Resource("number"))),
                        Ok(None) => return Err(decode()),
                    },
                    25 | 1043 | 1042 | 2950 => {
                        if crate::interrupt::reserve(text.len()) {
                            return Err(Fail::Interrupted);
                        }
                        Value::str(text)
                    }
                    16 => match text.as_str() {
                        "t" => Value::Bool(true),
                        "f" => Value::Bool(false),
                        _ => return Err(decode()),
                    },
                    17 => Value::bytes(hex_bytes(text).ok_or_else(decode)?),
                    114 | 3802 => match crate::json::parse(text) {
                        Ok(v) => v,
                        Err(crate::json::JsonError::Overflow) => return Err(Fail::Abort(AbortReason::Resource("number"))),
                        Err(crate::json::JsonError::Interrupted) => return Err(Fail::Interrupted),
                        Err(_) => return Err(decode()),
                    },
                    _ => return Err(decode()),
                },
            };
            r.push(name, v);
        }
        rows.push(Value::record(r));
    }
    Ok(Value::rec(vec![("rows", Value::array(rows)), ("count", Value::num(count))]))
}

impl<H: Host> Runtime<H> {
    fn latch(&mut self, tx: Option<u64>, e: &Value) {
        if let Some(id) = tx {
            if let Some(st) = self.txs.get_mut(&id) {
                if st.latched.is_none() {
                    st.latched = Some(e.clone());
                }
            }
        }
    }

    pub fn start_sql(&mut self, t: &mut Task, a: Vec<Value>) -> Ctl {
        let op = "sql";
        // 1. handle kind and ownership
        let (pool, tx, conn) = match &a[0] {
            Value::Database(d) => (Some(d.pool.clone()), None, None),
            Value::Transaction(h) => {
                let ok = match self.txs.get(&h.id) {
                    Some(st) => st.active && st.owner == t.id,
                    None => false,
                };
                if !ok {
                    let e = db_error("ownership", "not_sent", vec![]);
                    return self.raise(t, e);
                }
                let st = self.txs.get(&h.id).unwrap();
                if let Some(l) = &st.latched {
                    let l = l.clone();
                    return self.raise(t, l);
                }
                (None, Some(h.id), Some(st.connection.clone()))
            }
            other => {
                let e = err::type_error(op, err::Pos::Index(1), "database or transaction", other.kind());
                return self.raise(t, e);
            }
        };
        let prepared: Result<(String, Vec<SqlParam>, u64, u64), Fail> = (|| {
            let q = match &a[1] {
                Value::Query(q) => q.clone(),
                other => return Err(type_err(op, 2, "query", other)),
            };
            let o = options(op, 3, a.get(2), &["timeout", "max_rows"])?;
            let timeout = match field(o, "timeout") {
                Some(v) => opt_int(op, "timeout", v, 1)?,
                None => 10000,
            };
            let max_rows = match field(o, "max_rows") {
                Some(v) => opt_int(op, "max_rows", v, 0)?,
                None => 10000,
            };
            reject_unknown(op, o, &["timeout", "max_rows"])?;
            let sql = sqlcheck::translate(&q.segments).map_err(|r| {
                Fail::Err(db_error("prepare", "not_sent", vec![("reason", Value::str(r.reason()))]))
            })?;
            let mut params = Vec::with_capacity(q.values.len());
            for v in &q.values {
                params.push(bind_param(v).map_err(Fail::Err)?);
            }
            Ok((sql, params, timeout, max_rows))
        })();
        let (sql, params, timeout, max_rows) = match prepared {
            Ok(x) => x,
            Err(Fail::Err(e)) => {
                self.latch(tx, &e);
                return self.raise(t, e);
            }
            Err(f) => return self.fail_ctl(t, f),
        };
        let now = self.now_ms();
        let mut frame = SqlFrame {
            phase: SqlPhase::Acquiring,
            tx,
            conn,
            sql,
            params,
            max_rows,
            deadline: now.saturating_add(timeout),
            timeout,
            primary: None,
        };
        let opts = DispatchOpts { cleanup: false, timeout: Some(("sql", timeout)) };
        if let Some(p) = pool {
            t.stack.push(Frame::Sql(Box::new(frame)));
            Ctl::Dispatch(Effect::Acquire(p), opts)
        } else {
            frame.phase = SqlPhase::Executing;
            let eff = Effect::Execute {
                connection: frame.conn.clone().unwrap(),
                sql: frame.sql.clone(),
                params: frame.params.clone(),
                max_rows: frame.max_rows,
            };
            t.stack.push(Frame::Sql(Box::new(frame)));
            Ctl::Dispatch(eff, opts)
        }
    }

    fn sql_release(&mut self, t: &mut Task, mut f: SqlFrame, discard: bool) -> Ctl {
        let conn = f.conn.take().unwrap();
        f.phase = SqlPhase::Releasing;
        t.stack.push(Frame::Sql(Box::new(f)));
        Ctl::Dispatch(Effect::Release { connection: conn, discard }, DispatchOpts { cleanup: true, timeout: None })
    }

    fn sql_deliver(&mut self, t: &mut Task, tx: Option<u64>, res: Result<Value, Value>) -> Ctl {
        match res {
            Ok(v) => Ctl::Ret(v),
            Err(e) => {
                self.latch(tx, &e);
                self.raise(t, e)
            }
        }
    }

    pub fn sql_effect(&mut self, t: &mut Task, mut f: SqlFrame, o: EffOutcome) -> Ctl {
        match f.phase {
            SqlPhase::Acquiring => match o {
                EffOutcome::Ok(EffValue::Str(conn)) => {
                    f.conn = Some(conn.clone());
                    f.phase = SqlPhase::Executing;
                    let now = self.now_ms();
                    let remaining = f.deadline.saturating_sub(now).max(1);
                    let eff =
                        Effect::Execute { connection: conn, sql: f.sql.clone(), params: f.params.clone(), max_rows: f.max_rows };
                    t.stack.push(Frame::Sql(Box::new(f)));
                    Ctl::Dispatch(eff, DispatchOpts { cleanup: false, timeout: Some(("sql", remaining)) })
                }
                EffOutcome::Err(e) => {
                    let e = if err::code_of(&e) == "timeout" { timeout_error("sql", f.timeout, Some("not_sent")) } else { e };
                    self.sql_deliver(t, f.tx, Err(e))
                }
                EffOutcome::Resource(r) => self.fail_ctl(t, Fail::Abort(AbortReason::Resource(super::callbacks::leak(r)))),
                _ => self.joined_without_abort(t),
            },
            SqlPhase::Executing => {
                let (res, discard) = match o {
                    EffOutcome::Ok(EffValue::Rows(raw)) => match convert_rows(&raw, f.max_rows) {
                        Ok(v) => (Ok(v), false),
                        Err(Fail::Err(e)) => (Err(e), false),
                        Err(other) => {
                            // Abort (quota/interrupt): record it, but still return
                            // a pooled connection (discarded) before unwinding.
                            let ctl = match other {
                                Fail::Interrupted => self.disarm_interrupts(t).unwrap_or(Ctl::Ret(Value::Null)),
                                o => self.fail_ctl(t, o),
                            };
                            if f.tx.is_none() {
                                f.primary = Some(Ok(Value::Null));
                                return self.sql_release(t, f, true);
                            }
                            return ctl;
                        }
                    },
                    EffOutcome::Err(e) => {
                        let e = if err::code_of(&e) == "timeout" { timeout_error("sql", f.timeout, Some("unknown")) } else { e };
                        let discard = matches!(err::code_of(&e), "timeout" | "network");
                        (Err(e), discard)
                    }
                    EffOutcome::Resource(r) => {
                        return self.fail_ctl(t, Fail::Abort(AbortReason::Resource(super::callbacks::leak(r))))
                    }
                    _ => return self.joined_without_abort(t),
                };
                if f.tx.is_some() {
                    return self.sql_deliver(t, f.tx, res);
                }
                f.primary = Some(res);
                self.sql_release(t, f, discard)
            }
            SqlPhase::Releasing => {
                let res = f.primary.take().unwrap();
                if let Some(sig) = self.abort_of(t.scope) {
                    return Ctl::Abort(sig, None);
                }
                self.sql_deliver(t, f.tx, res)
            }
        }
    }

    pub fn sql_abort(&mut self, t: &mut Task, mut f: SqlFrame, sig: AbortSig, carry: Option<EffOutcome>) -> Ctl {
        if f.tx.is_none() {
            if let (SqlPhase::Acquiring, Some(EffOutcome::Ok(EffValue::Str(c)))) = (&f.phase, &carry) {
                f.conn = Some(c.clone());
            }
            if f.conn.is_some() && !matches!(f.phase, SqlPhase::Releasing) {
                f.primary = Some(Ok(Value::Null));
                return self.sql_release(t, f, true);
            }
        }
        Ctl::Abort(sig, None)
    }

    // ------------------------------------------------------------------ tx

    pub fn start_tx(&mut self, t: &mut Task, a: Vec<Value>, f: Value) -> Ctl {
        let op = "tx";
        let res: Result<(String, u64), Fail> = (|| {
            let pool = match &a[0] {
                Value::Database(d) => d.pool.clone(),
                other => return Err(type_err(op, 1, "database", other)),
            };
            let o = options(op, 2, a.get(1), &["timeout"])?;
            let timeout = match field(o, "timeout") {
                Some(v) => opt_int(op, "timeout", v, 1)?,
                None => 10000,
            };
            reject_unknown(op, o, &["timeout"])?;
            callback(op, &f, 1)?;
            Ok((pool, timeout))
        })();
        let (pool, timeout) = match res {
            Ok(x) => x,
            Err(e) => return self.fail_ctl(t, e),
        };
        if t.tx.is_some() {
            let e = db_error("ownership", "not_sent", vec![]);
            return self.raise(t, e);
        }
        let scope = self.new_scope(Some(t.scope));
        let at = self.now_ms().saturating_add(timeout);
        self.add_scope_deadline(scope, at, Some(timeout));
        let frame = TxFrame {
            phase: TxPhase::Acquiring,
            pool: pool.clone(),
            timeout,
            scope,
            prev_scope: t.scope,
            f,
            conn: None,
            tx: None,
            primary: None,
            timeout_outcome: "not_sent",
        };
        t.scope = scope;
        t.seen_epoch = u64::MAX;
        t.stack.push(Frame::Tx(Box::new(frame)));
        Ctl::Dispatch(Effect::Acquire(pool), DispatchOpts::default())
    }

    fn tx_invalidate(&mut self, t: &mut Task, x: &mut TxFrame) {
        if let Some(id) = x.tx.take() {
            if let Some(st) = self.txs.get_mut(&id) {
                st.active = false;
                if x.primary.is_none() {
                    if let Some(l) = st.latched.clone() {
                        x.primary = Some(Primary::Error(l));
                    }
                }
            }
            self.txs.remove(&id);
            t.tx = None;
            x.tx = Some(id); // keep id for diagnostics only
            x.tx = None;
        }
    }

    fn tx_cleanup(&mut self, t: &mut Task, mut x: TxFrame, eff: Effect, phase: TxPhase) -> Ctl {
        x.phase = phase;
        t.stack.push(Frame::Tx(Box::new(x)));
        Ctl::Dispatch(eff, DispatchOpts { cleanup: true, timeout: None })
    }

    fn tx_rollback(&mut self, t: &mut Task, x: TxFrame) -> Ctl {
        let conn = x.conn.clone().unwrap();
        self.tx_cleanup(t, x, Effect::Rollback(conn), TxPhase::RollingBack)
    }

    fn tx_release(&mut self, t: &mut Task, x: TxFrame, discard: bool) -> Ctl {
        let conn = x.conn.clone().unwrap();
        self.tx_cleanup(t, x, Effect::Release { connection: conn, discard }, TxPhase::Releasing)
    }

    /// Final delivery after cleanup.
    fn tx_finish(&mut self, t: &mut Task, x: TxFrame) -> Ctl {
        self.drop_scope(x.scope);
        t.scope = x.prev_scope;
        t.seen_epoch = u64::MAX;
        t.tx = None;
        if let Some(sig) = self.abort_of(t.scope) {
            return Ctl::Abort(sig, None);
        }
        t.unwinding = None;
        match x.primary.unwrap_or(Primary::Value(Value::Null)) {
            Primary::Value(v) => Ctl::Ret(v),
            Primary::Error(e) => {
                if t.throw_span.is_none() {
                    t.throw_span = Some(self.prog.span(t.cur_node));
                }
                Ctl::Throw(e)
            }
            Primary::Timeout => {
                let e = timeout_error("tx", x.timeout, Some(x.timeout_outcome));
                self.raise(t, e)
            }
            Primary::Abort(sig) => Ctl::Abort(sig, None),
        }
    }

    pub fn tx_ret(&mut self, t: &mut Task, mut x: TxFrame, v: Value) -> Ctl {
        // callback returned normally
        let latched = x.tx.and_then(|id| self.txs.get(&id)).and_then(|st| st.latched.clone());
        self.tx_invalidate(t, &mut x);
        if let Some(e) = latched {
            x.primary = Some(Primary::Error(e));
            return self.tx_rollback(t, x);
        }
        x.primary = Some(Primary::Value(v));
        x.phase = TxPhase::Committing;
        let conn = x.conn.clone().unwrap();
        t.stack.push(Frame::Tx(Box::new(x)));
        Ctl::Dispatch(Effect::Commit(conn), DispatchOpts::default())
    }

    pub fn tx_throw(&mut self, t: &mut Task, mut x: TxFrame, e: Value) -> Ctl {
        if x.phase != TxPhase::Callback {
            return Ctl::Throw(e);
        }
        let latched = x.tx.and_then(|id| self.txs.get(&id)).and_then(|st| st.latched.clone());
        self.tx_invalidate(t, &mut x);
        x.primary = Some(Primary::Error(latched.unwrap_or(e)));
        self.tx_rollback(t, x)
    }

    pub fn tx_effect(&mut self, t: &mut Task, mut x: TxFrame, o: EffOutcome) -> Ctl {
        match x.phase {
            TxPhase::Acquiring => match o {
                EffOutcome::Ok(EffValue::Str(conn)) => {
                    x.conn = Some(conn.clone());
                    x.phase = TxPhase::Beginning;
                    t.stack.push(Frame::Tx(Box::new(x)));
                    Ctl::Dispatch(Effect::Begin(conn), DispatchOpts::default())
                }
                EffOutcome::Err(e) => {
                    x.primary = Some(Primary::Error(e));
                    self.tx_finish(t, x)
                }
                EffOutcome::Resource(r) => {
                    x.primary = None;
                    let ctl = self.tx_finish(t, x);
                    let _ = ctl;
                    self.fail_ctl(t, Fail::Abort(AbortReason::Resource(super::callbacks::leak(r))))
                }
                EffOutcome::Joined | EffOutcome::JoinedNotSent | EffOutcome::Ok(_) => self.joined_without_abort(t),
            },
            TxPhase::Beginning => match o {
                EffOutcome::Ok(_) => {
                    let id = self.next_tx;
                    self.next_tx += 1;
                    self.txs.insert(id, TxState { owner: t.id, connection: x.conn.clone().unwrap(), active: true, latched: None });
                    x.tx = Some(id);
                    t.tx = Some(id);
                    x.phase = TxPhase::Callback;
                    let f = x.f.clone();
                    t.stack.push(Frame::Tx(Box::new(x)));
                    Ctl::Apply(f, vec![Value::Transaction(Rc::new(TxHandle { id }))], None)
                }
                EffOutcome::Err(e) => {
                    x.primary = Some(Primary::Error(e));
                    self.tx_release(t, x, true)
                }
                _ => self.joined_without_abort(t),
            },
            TxPhase::Committing => match o {
                EffOutcome::Ok(_) => self.tx_release(t, x, false),
                EffOutcome::Err(e) => {
                    let definite = err::data_of(&e)
                        .and_then(|d| d.get("outcome"))
                        .and_then(|v| v.as_str())
                        .map(|s| s == "rolled_back")
                        .unwrap_or(false);
                    x.primary = Some(Primary::Error(e));
                    self.tx_release(t, x, !definite)
                }
                _ => self.joined_without_abort(t),
            },
            TxPhase::RollingBack => {
                let ok = matches!(o, EffOutcome::Ok(_));
                if !ok {
                    x.timeout_outcome = "unknown";
                }
                self.tx_release(t, x, !ok)
            }
            TxPhase::Releasing => self.tx_finish(t, x),
            TxPhase::Callback => {
                self.harness_failure = Some("effect delivered to a transaction callback frame".into());
                Ctl::Waiting
            }
        }
    }

    pub fn tx_abort(&mut self, t: &mut Task, mut x: TxFrame, sig: AbortSig, carry: Option<EffOutcome>) -> Ctl {
        let own_timeout = sig.scope == x.scope && matches!(sig.reason, AbortReason::TxTimeout(_));
        let primary = if own_timeout { Primary::Timeout } else { Primary::Abort(sig.clone()) };
        if own_timeout {
            x.timeout_outcome = match x.phase {
                TxPhase::Acquiring => "not_sent",
                TxPhase::Callback => "rolled_back", // downgraded if rollback fails
                _ => "unknown",
            };
        }
        match x.phase {
            TxPhase::Acquiring => {
                x.primary = Some(primary);
                if let Some(EffOutcome::Ok(EffValue::Str(c))) = carry {
                    x.conn = Some(c);
                    return self.tx_release(t, x, true);
                }
                self.tx_finish(t, x)
            }
            TxPhase::Beginning => {
                x.primary = Some(primary);
                self.tx_release(t, x, true)
            }
            TxPhase::Callback => {
                self.tx_invalidate(t, &mut x);
                x.primary = Some(primary);
                self.tx_rollback(t, x)
            }
            TxPhase::Committing => {
                // Unknown COMMIT outcome: no further SQL, discard.
                x.primary = Some(primary);
                self.tx_release(t, x, true)
            }
            TxPhase::RollingBack | TxPhase::Releasing => {
                // Cleanup in progress; an ancestor abort dominates the result.
                if let Primary::Abort(_) = primary {
                    x.primary = Some(primary);
                }
                self.tx_finish(t, x)
            }
        }
    }
}
