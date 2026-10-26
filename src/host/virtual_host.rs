//! Deterministic fixture host for the embedded conformance protocol (§16).

use super::{EffOutcome, EffValue, Effect, Host, HostEvent, IncomingRequest, RawResponse, RawRows, SqlParam};
use crate::value::{json_equal, Record, Value};
use base64::Engine;
use std::collections::{BTreeMap, HashMap};

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

#[derive(Clone, Debug)]
pub enum FixtureResult {
    Ok(Value),
    Error(Value),
    Hold,
}

#[derive(Clone, Debug)]
pub struct Fixture {
    pub id: String,
    pub op: String,
    pub matcher: Value,
    pub result: FixtureResult,
    pub after_ms: u64,
    pub cancel_ack_ms: u64,
    pub consumed: bool,
}

#[derive(Clone, Debug)]
pub struct VRequest {
    pub id: String,
    pub at_ms: u64,
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct VServer {
    pub requests: Vec<VRequest>,
    pub cancel_at_ms: u64,
}

#[derive(Clone, Debug)]
pub struct VResponse {
    pub id: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

enum Ev {
    Complete { eff: u64, outcome: EffOutcome, label: Option<String>, kind: &'static str },
    Joined { eff: u64, label: Option<String> },
    Arrive(usize),
    CancelRoot,
}

struct InFlight {
    label: Option<String>,
    event_key: Option<(u64, u8, u64)>,
    cancel_ack_ms: u64,
}

pub struct VirtualHost {
    pub now: u64,
    pub fixtures: Vec<Fixture>,
    pub trace: Vec<String>,
    pub starts: BTreeMap<String, u64>,
    pub responses: Vec<VResponse>,
    pub diagnostics: Vec<String>,
    pub failure: Option<String>,
    pub server: Option<VServer>,
    events: BTreeMap<(u64, u8, u64), Ev>,
    seq: u64,
    inflight: HashMap<u64, InFlight>,
    sleep_counter: u64,
    listener_started: Option<u64>,
    bodies: HashMap<u64, Vec<u8>>,
    labels: HashMap<u64, String>,
}

impl VirtualHost {
    pub fn new(fixtures: Vec<Fixture>, server: Option<VServer>) -> VirtualHost {
        VirtualHost {
            now: 0,
            fixtures,
            trace: Vec::new(),
            starts: BTreeMap::new(),
            responses: Vec::new(),
            diagnostics: Vec::new(),
            failure: None,
            server,
            events: BTreeMap::new(),
            seq: 0,
            inflight: HashMap::new(),
            sleep_counter: 0,
            listener_started: None,
            bodies: HashMap::new(),
            labels: HashMap::new(),
        }
    }

    fn push(&mut self, at: u64, prio: u8, ev: Ev) -> (u64, u8, u64) {
        self.seq += 1;
        let key = (at, prio, self.seq);
        self.events.insert(key, ev);
        key
    }

    fn complete_now(&mut self, id: u64, outcome: EffOutcome) {
        let key = self.push(self.now, 1, Ev::Complete { eff: id, outcome, label: None, kind: "" });
        self.inflight.insert(id, InFlight { label: None, event_key: Some(key), cancel_ack_ms: 0 });
    }

    fn fail(&mut self, msg: String) {
        if self.failure.is_none() {
            self.failure = Some(msg);
        }
    }

    /// Normalized matchable arguments for an effect.
    fn args_of(eff: &Effect) -> Value {
        let params = |ps: &[SqlParam]| {
            Value::array(
                ps.iter()
                    .map(|p| {
                        Value::rec(vec![
                            ("oid", Value::int(super::real::pg::param_oid(p.kind) as i64)),
                            ("value", p.value.as_deref().map(Value::str).unwrap_or(Value::Null)),
                        ])
                    })
                    .collect(),
            )
        };
        match eff {
            Effect::Http(r) => Value::rec(vec![
                ("method", Value::str(&r.method)),
                ("url", Value::str(&r.url)),
                (
                    "headers",
                    Value::array(r.headers.iter().map(|(n, v)| Value::array(vec![Value::str(n), Value::str(v)])).collect()),
                ),
                ("body_base64", r.body.as_ref().map(|b| Value::str(&b64().encode(b))).unwrap_or(Value::Null)),
            ]),
            Effect::Environment(n) => Value::rec(vec![("name", Value::str(n))]),
            Effect::ReadRegular { path, .. } => Value::rec(vec![("path", Value::str(path))]),
            Effect::ReplaceRegular { path, bytes } => {
                Value::rec(vec![("path", Value::str(path)), ("bytes_base64", Value::str(&b64().encode(bytes)))])
            }
            Effect::LogLine(l) => Value::rec(vec![("line", Value::str(l))]),
            Effect::UtcNow => Value::rec(vec![]),
            Effect::RandomBytes(n) => Value::rec(vec![("count", Value::int(*n as i64))]),
            Effect::Pool(a) => Value::rec(vec![("alias", Value::str(a))]),
            Effect::Acquire(p) => Value::rec(vec![("pool", Value::str(p))]),
            Effect::Begin(c) => Value::rec(vec![("connection", Value::str(c)), ("isolation", Value::str("read_committed"))]),
            Effect::Execute { connection, sql, params: ps, .. } => Value::rec(vec![
                ("connection", Value::str(connection)),
                ("sql", Value::str(sql)),
                ("parameters", params(ps)),
            ]),
            Effect::Commit(c) | Effect::Rollback(c) => Value::rec(vec![("connection", Value::str(c))]),
            Effect::Release { connection, discard } => Value::rec(vec![
                ("connection", Value::str(connection)),
                ("disposition", Value::str(if *discard { "discard" } else { "reset" })),
            ]),
            _ => Value::rec(vec![]),
        }
    }

    pub(crate) fn validate_match(op: &str, v: &Value) -> Result<(), String> {
        let keys: &[&str] = match op {
            "http" => &["method", "url", "headers", "body_base64"],
            "environment" => &["name"],
            "read_regular" => &["path"],
            "replace_regular" => &["path", "bytes_base64"],
            "log_line" => &["line"],
            "utc_now" => &[],
            "random_bytes" => &["count"],
            "pool" => &["alias"],
            "acquire" => &["pool"],
            "begin" => &["connection", "isolation"],
            "execute" => &["connection", "sql", "parameters"],
            "commit" | "rollback" => &["connection"],
            "release" => &["connection", "disposition"],
            _ => return Err("unknown fixture operation".into()),
        };
        let r = v.as_record().ok_or("fixture match must be a record")?;
        closed_fields(r, keys)?;
        for (key, value) in r.iter() {
            match key.as_ref() {
                "headers" => {
                    for h in value.as_array().ok_or("match headers must be pairs")?.iter() {
                        let pair = h.as_array().ok_or("match header must be a pair")?;
                        if pair.len() != 2 || pair.iter().any(|v| v.as_str().is_none()) {
                            return Err("match header must be two strings".into());
                        }
                    }
                }
                "parameters" => {}
                "count" => {
                    value.as_num().and_then(|d| d.to_u64()).ok_or("match count must be a nonnegative integer")?;
                }
                "body_base64" if matches!(value, Value::Null) => {}
                "body_base64" | "bytes_base64" => {
                    b64().decode(value.as_str().ok_or("match bytes must be base64")?).map_err(|_| "invalid match base64")?;
                }
                "isolation" if value.as_str() == Some("read_committed") => {}
                "isolation" => return Err("match isolation must be read_committed".into()),
                "disposition" if matches!(value.as_str(), Some("reset" | "discard")) => {}
                "disposition" => return Err("match disposition must be reset or discard".into()),
                _ => { value.as_str().ok_or("match argument must be a string")?; }
            }
        }
        if op == "execute" {
            if let Some(parameters) = r.get("parameters") {
                for p in parameters.as_array().ok_or("parameters must be an array")?.iter() {
                    let p = p.as_record().ok_or("parameter must be a record")?;
                    closed_fields(p, &["oid", "value"])?;
                    if let Some(oid) = p.get("oid") {
                        oid.as_num().and_then(|d| d.to_u64()).and_then(|n| u32::try_from(n).ok()).ok_or("match parameter oid out of range")?;
                    }
                    if p.get("value").is_some_and(|v| !matches!(v, Value::Null | Value::Str(_))) {
                        return Err("match parameter value must be string or null".into());
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn convert_ok(op: &str, v: &Value) -> Result<EffValue, String> {
        let rec = |v: &Value, keys: &[&str]| {
            let r = v.as_record().cloned().ok_or_else(|| format!("{op} fixture ok must be a record"))?;
            closed_fields(&r, keys)?;
            Ok::<_, String>(r)
        };
        let b64field = |r: &Record, k: &str| -> Result<Vec<u8>, String> {
            let s = r.get(k).and_then(|x| x.as_str()).ok_or_else(|| format!("{op} fixture needs {k}"))?;
            b64().decode(s).map_err(|_| format!("{op} fixture {k} is not base64"))
        };
        Ok(match op {
            "http" => {
                let r = rec(v, &["status", "headers", "body_base64"])?;
                let status = r.get("status").and_then(|s| s.as_num()).and_then(|d| d.to_u64())
                    .and_then(|n| u16::try_from(n).ok()).ok_or("http status out of range")?;
                if !(200..=599).contains(&status) { return Err("http fixture status must be 200..599".into()); }
                let mut headers = Vec::new();
                for h in r.get("headers").and_then(|h| h.as_array()).ok_or("http headers")?.iter() {
                    let pair = h.as_array().ok_or("header pair")?;
                    if pair.len() != 2 { return Err("header pair must have two strings".into()); }
                    let n = pair.first().and_then(|x| x.as_str()).ok_or("header name")?;
                    let val = pair.get(1).and_then(|x| x.as_str()).ok_or("header value")?;
                    if val.chars().any(|c| c as u32 > 255) { return Err("header value must represent octets".into()); }
                    headers.push((n.to_string(), val.chars().map(|c| c as u32 as u8).collect()));
                }
                EffValue::Http(RawResponse { status, headers, body: b64field(&r, "body_base64")? })
            }
            "environment" | "pool" | "acquire" => {
                let s = v.as_str().ok_or("fixture ok must be a string")?;
                if op != "environment" && s.is_empty() { return Err("fixture identity must be nonempty".into()); }
                EffValue::Str(s.to_string())
            }
            "read_regular" | "random_bytes" => EffValue::Bytes(b64field(&rec(v, &["bytes_base64"])?, "bytes_base64")?),
            "utc_now" => {
                let r = rec(v, &["unix_ms"])?;
                let ms = r.get("unix_ms").and_then(|x| x.as_num()).and_then(|d| d.to_i128()).ok_or("unix_ms")?;
                EffValue::UnixMs(ms)
            }
            "execute" => {
                let r = rec(v, &["columns", "rows", "count", "outcome"])?;
                let mut columns = Vec::new();
                for c in r.get("columns").and_then(|c| c.as_array()).ok_or("columns")?.iter() {
                    let cr = c.as_record().ok_or("column")?;
                    closed_fields(cr, &["name", "oid"])?;
                    let name = cr.get("name").and_then(|x| x.as_str()).ok_or("column name")?;
                    let oid = cr.get("oid").and_then(|x| x.as_num()).and_then(|d| d.to_u64()).ok_or("column oid")?;
                    columns.push((name.to_string(), u32::try_from(oid).map_err(|_| "column oid out of range")?));
                }
                let mut rows = Vec::new();
                for row in r.get("rows").and_then(|x| x.as_array()).ok_or("rows")?.iter() {
                    let cells = row.as_array().ok_or("row")?;
                    if cells.len() != columns.len() { return Err("row width differs from columns".into()); }
                    rows.push(
                        cells
                            .iter()
                            .map(|c| match c {
                                Value::Null => Ok(None),
                                Value::Str(s) => Ok(Some(s.to_string())),
                                _ => Err("cell must be string or null".to_string()),
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                }
                let count = r.get("count").and_then(|x| x.as_str()).ok_or("count")?.to_string();
                let outcome = r.get("outcome").and_then(|x| x.as_str()).ok_or("outcome")?.to_string();
                if count.is_empty() || !count.bytes().all(|b| b.is_ascii_digit()) { return Err("count must be a nonnegative integer string".into()); }
                if !matches!(outcome.as_str(), "committed" | "unknown") { return Err("invalid execute outcome".into()); }
                for row in &mut rows {
                    for ((_,oid),cell) in columns.iter().zip(row) {
                        if let Some(text)=cell {
                            if *oid==16 { *text=match text.as_str(){"t"=>"true","f"=>"false",_=>"invalid"}.into(); }
                            if *oid==17 {
                                let bytes=text.strip_prefix("\\x").filter(|h| h.len()%2==0 && h.is_ascii()).and_then(|h|(0..h.len()).step_by(2).map(|i|u8::from_str_radix(&h[i..i+2],16).ok()).collect::<Option<Vec<_>>>());
                                *text=bytes.map(|b|base64::engine::general_purpose::STANDARD.encode(b)).unwrap_or_else(||"!".into());
                            }
                        }
                    }
                }
                let columns=columns.into_iter().map(|(name,oid)|(name,super::real::pg::column_kind(oid))).collect();
                EffValue::Rows(RawRows { columns, rows, count, outcome })
            }
            _ => {
                if !matches!(v, Value::Null) { return Err(format!("{op} fixture ok must be null")); }
                EffValue::Null
            }
        })
    }

    fn subset(m: &Value, a: &Value) -> bool {
        match (m, a) {
            (Value::Record(mr), Value::Record(ar)) => mr.iter().all(|(k, mv)| ar.get(k).is_some_and(|av| Self::subset(mv, av))),
            _ => json_equal(m, a),
        }
    }

    fn dispatch_fixture(&mut self, id: u64, eff: &Effect) {
        let op = eff.op_name();
        let args = Self::args_of(eff);
        let idx = self.fixtures.iter().position(|f| !f.consumed && f.op == op && Self::subset(&f.matcher, &args));
        let idx = match idx {
            Some(i) => i,
            None => {
                let shown = crate::json::dump(&args);
                self.fail(format!("unmatched {op} effect {shown}"));
                return;
            }
        };
        self.fixtures[idx].consumed = true;
        let f = self.fixtures[idx].clone();
        self.trace.push(format!("{}:start", f.id));
        self.starts.insert(f.id.clone(), self.now);
        let at = match self.now.checked_add(f.after_ms) {
            Some(at) => at,
            None if !matches!(f.result, FixtureResult::Hold) => {
                self.complete_now(id, EffOutcome::Resource("clock".into()));
                return;
            }
            None => self.now,
        };
        let key = match &f.result {
            FixtureResult::Hold => None,
            FixtureResult::Ok(v) => match Self::convert_ok(op, v) {
                Ok(ev) => Some(self.push(
                    at,
                    1,
                    Ev::Complete { eff: id, outcome: EffOutcome::Ok(ev), label: Some(f.id.clone()), kind: "ok" },
                )),
                Err(e) => {
                    self.fail(format!("fixture {}: {e}", f.id));
                    None
                }
            },
            FixtureResult::Error(e) => Some(self.push(
                at,
                1,
                Ev::Complete { eff: id, outcome: EffOutcome::Err(e.clone()), label: Some(f.id.clone()), kind: "error" },
            )),
        };
        self.inflight.insert(id, InFlight { label: Some(f.id), event_key: key, cancel_ack_ms: f.cancel_ack_ms });
    }

    pub fn unconsumed(&self) -> Vec<String> {
        self.fixtures.iter().filter(|f| !f.consumed).map(|f| f.id.clone()).collect()
    }
}

fn closed_fields(r: &Record, keys: &[&str]) -> Result<(), String> {
    for (key, _) in r.iter() {
        if !keys.contains(&key.as_ref()) { return Err(format!("unknown fixture field {key:?}")); }
    }
    Ok(())
}

impl Host for VirtualHost {
    fn now_ms(&self) -> u64 {
        self.now
    }

    fn dispatch(&mut self, id: u64, eff: Effect) {
        match &eff {
            Effect::Sleep(ms) => {
                self.sleep_counter += 1;
                let label = format!("sleep#{}", self.sleep_counter);
                self.trace.push(format!("{label}:start"));
                let Some(at) = self.now.checked_add(*ms) else {
                    self.complete_now(id, EffOutcome::Resource("clock".into()));
                    return;
                };
                let key = self.push(
                    at,
                    1,
                    Ev::Complete { eff: id, outcome: EffOutcome::Ok(EffValue::Null), label: Some(label.clone()), kind: "ok" },
                );
                self.inflight.insert(id, InFlight { label: Some(label), event_key: Some(key), cancel_ack_ms: 0 });
            }
            Effect::Listen { .. } => {
                let srv = match &self.server {
                    Some(s) => s.clone(),
                    None => {
                        self.fail("serve attempted to listen without a virtual server".into());
                        return;
                    }
                };
                if self.now.checked_add(srv.cancel_at_ms).is_none()
                    || srv.requests.iter().any(|r| self.now.checked_add(r.at_ms).is_none()) {
                    self.complete_now(id, EffOutcome::Resource("clock".into()));
                    return;
                }
                self.trace.push("listener:start".into());
                self.listener_started = Some(self.now);
                self.complete_now(id, EffOutcome::Ok(EffValue::Str("listener-1".into())));
                for (i, r) in srv.requests.iter().enumerate() {
                    self.push(self.now.checked_add(r.at_ms).unwrap(), 1, Ev::Arrive(i));
                }
                self.push(self.now.checked_add(srv.cancel_at_ms).unwrap(), 0, Ev::CancelRoot);
            }
            Effect::ReadBody { request, max_body, .. } => {
                let body = self.bodies.get(request).cloned().unwrap_or_default();
                let res = if body.len() as u64 > *max_body { Err(super::BodyFault::TooLarge) } else { Ok(body) };
                self.complete_now(id, EffOutcome::Ok(EffValue::Body(res)));
            }
            Effect::SendResponse { request, status, headers, body, suppress_body } => {
                let label = self.labels.get(request).cloned().unwrap_or_default();
                if self.responses.iter().any(|r| r.id == label) {
                    self.fail(format!("duplicate response for {label}"));
                }
                self.trace.push(format!("{label}:response:{status}"));
                self.responses.push(VResponse {
                    id: label,
                    status: *status,
                    headers: headers.clone(),
                    body: if *suppress_body { Vec::new() } else { body.clone() },
                });
                self.complete_now(id, EffOutcome::Ok(EffValue::Null));
            }
            Effect::CloseRequest { .. } => self.complete_now(id, EffOutcome::Ok(EffValue::Null)),
            Effect::CloseListener { .. } => {
                self.trace.push("listener:close".into());
                self.complete_now(id, EffOutcome::Ok(EffValue::Null));
            }
            _ => self.dispatch_fixture(id, &eff),
        }
    }

    fn cancel(&mut self, id: u64) {
        let fl = match self.inflight.remove(&id) {
            Some(f) => f,
            None => return,
        };
        if let Some(k) = fl.event_key {
            self.events.remove(&k);
        }
        if let Some(l) = &fl.label {
            self.trace.push(format!("{l}:cancel"));
        }
        self.push(self.now.saturating_add(fl.cancel_ack_ms), 1, Ev::Joined { eff: id, label: fl.label });
    }

    async fn wait(&mut self, until: Option<u64>) -> Option<HostEvent> {
        if self.failure.is_some() {
            return None;
        }
        let key = match self.events.keys().next() {
            Some(k) => *k,
            None => {
                if let Some(u) = until {
                    self.now = self.now.max(u);
                }
                return None;
            }
        };
        if let Some(u) = until {
            if u <= key.0 {
                self.now = self.now.max(u);
                return None;
            }
        }
        let ev = self.events.remove(&key).unwrap();
        self.now = self.now.max(key.0);
        match ev {
            Ev::Complete { eff, outcome, label, kind } => {
                self.inflight.remove(&eff);
                if let Some(l) = label {
                    self.trace.push(format!("{l}:{kind}"));
                }
                Some(HostEvent::Done(eff, outcome))
            }
            Ev::Joined { eff, label } => {
                if let Some(l) = label {
                    self.trace.push(format!("{l}:joined"));
                }
                Some(HostEvent::Done(eff, EffOutcome::Joined))
            }
            Ev::Arrive(i) => {
                let r = self.server.as_ref().unwrap().requests[i].clone();
                let rid = (i + 1) as u64;
                self.trace.push(format!("{}:receive", r.id));
                self.bodies.insert(rid, r.body.clone());
                self.labels.insert(rid, r.id.clone());
                Some(HostEvent::Request(IncomingRequest {
                    listener: "listener-1".into(),
                    id: rid,
                    label: r.id,
                    method: r.method,
                    target: r.target,
                    upgrade: r.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("upgrade")),
                    framing_invalid: false,
                    headers: r.headers.iter().map(|(n, v)| (n.clone(), v.chars().map(|c| c as u32 as u8).collect())).collect(),
                }))
            }
            Ev::CancelRoot => Some(HostEvent::CancelRoot),
        }
    }

    fn is_virtual(&self) -> bool {
        true
    }

    fn diagnostic(&mut self, line: &str) {
        self.diagnostics.push(line.to_string());
    }

    fn failed(&self) -> bool {
        self.failure.is_some()
    }
}
