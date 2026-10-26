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
                            ("oid", Value::int(p.oid as i64)),
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

    fn convert_ok(op: &str, v: &Value) -> Result<EffValue, String> {
        let rec = |v: &Value| v.as_record().cloned().ok_or_else(|| format!("{op} fixture ok must be a record"));
        let b64field = |r: &Record, k: &str| -> Result<Vec<u8>, String> {
            let s = r.get(k).and_then(|x| x.as_str()).ok_or_else(|| format!("{op} fixture needs {k}"))?;
            b64().decode(s).map_err(|_| format!("{op} fixture {k} is not base64"))
        };
        Ok(match op {
            "http" => {
                let r = rec(v)?;
                let status = r.get("status").and_then(|s| s.as_num()).and_then(|d| d.to_u64()).ok_or("http status")? as u16;
                let mut headers = Vec::new();
                for h in r.get("headers").and_then(|h| h.as_array()).ok_or("http headers")?.iter() {
                    let pair = h.as_array().ok_or("header pair")?;
                    let n = pair.first().and_then(|x| x.as_str()).ok_or("header name")?;
                    let val = pair.get(1).and_then(|x| x.as_str()).ok_or("header value")?;
                    headers.push((n.to_string(), val.chars().map(|c| c as u32 as u8).collect()));
                }
                EffValue::Http(RawResponse { status, headers, body: b64field(&r, "body_base64")? })
            }
            "environment" | "pool" | "acquire" => EffValue::Str(v.as_str().ok_or("fixture ok must be a string")?.to_string()),
            "read_regular" | "random_bytes" => EffValue::Bytes(b64field(&rec(v)?, "bytes_base64")?),
            "utc_now" => {
                let r = rec(v)?;
                let ms = r.get("unix_ms").and_then(|x| x.as_num()).and_then(|d| d.to_i128()).ok_or("unix_ms")?;
                EffValue::UnixMs(ms as i64)
            }
            "execute" => {
                let r = rec(v)?;
                let mut columns = Vec::new();
                for c in r.get("columns").and_then(|c| c.as_array()).ok_or("columns")?.iter() {
                    let cr = c.as_record().ok_or("column")?;
                    let name = cr.get("name").and_then(|x| x.as_str()).ok_or("column name")?;
                    let oid = cr.get("oid").and_then(|x| x.as_num()).and_then(|d| d.to_u64()).ok_or("column oid")?;
                    columns.push((name.to_string(), oid as u32));
                }
                let mut rows = Vec::new();
                for row in r.get("rows").and_then(|x| x.as_array()).ok_or("rows")?.iter() {
                    let cells = row.as_array().ok_or("row")?;
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
                EffValue::Rows(RawRows { columns, rows, count, outcome })
            }
            _ => EffValue::Null,
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
        let key = match &f.result {
            FixtureResult::Hold => None,
            FixtureResult::Ok(v) => match Self::convert_ok(op, v) {
                Ok(ev) => Some(self.push(
                    self.now.saturating_add(f.after_ms),
                    1,
                    Ev::Complete { eff: id, outcome: EffOutcome::Ok(ev), label: Some(f.id.clone()), kind: "ok" },
                )),
                Err(e) => {
                    self.fail(format!("fixture {}: {e}", f.id));
                    None
                }
            },
            FixtureResult::Error(e) => Some(self.push(
                self.now.saturating_add(f.after_ms),
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
                let key = self.push(
                    self.now.saturating_add(*ms),
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
                self.trace.push("listener:start".into());
                self.listener_started = Some(self.now);
                self.complete_now(id, EffOutcome::Ok(EffValue::Str("listener-1".into())));
                for (i, r) in srv.requests.iter().enumerate() {
                    self.push(self.now.saturating_add(r.at_ms), 1, Ev::Arrive(i));
                }
                self.push(self.now.saturating_add(srv.cancel_at_ms), 0, Ev::CancelRoot);
            }
            Effect::ReadBody { request, max_body } => {
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
