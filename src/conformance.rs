//! Embedded conformance protocol: extraction, manifest validation and the
//! deterministic case runner (spec §16).

use crate::host::config::{parse_case_host, HostConfig};
use crate::host::virtual_host::{Fixture, FixtureResult, VRequest, VServer, VirtualHost};
use crate::runtime::{RootOutcome, Runtime};
use crate::value::{json_equal, Kind, Record, Value};
use base64::Engine;
use std::collections::HashSet;

/// Extract a marked fenced block (whole-line markers, exact fence).
pub fn extract_block(text: &str, name: &str, lang: &str) -> Result<String, String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let begin = format!("<!-- KIROHA_{name}_BEGIN -->");
    let end = format!("<!-- KIROHA_{name}_END -->");
    let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| **l == begin).map(|(i, _)| i).collect();
    let stops: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| **l == end).map(|(i, _)| i).collect();
    if starts.len() != 1 || stops.len() != 1 {
        return Err(format!("expected exactly one {name} begin and end marker"));
    }
    let (a, b) = (starts[0], stops[0]);
    if a + 2 > b || lines[a + 1] != format!("```{lang}") || lines[b - 1] != "```" {
        return Err(format!("{name} markers must enclose exactly one ```{lang} fence"));
    }
    let inner = &lines[a + 2..b - 1];
    if inner.iter().any(|l| l.starts_with("```")) {
        return Err(format!("{name} block contains an extra fence"));
    }
    let mut payload = inner.join("\n");
    payload.push('\n');
    Ok(payload)
}

pub struct Case {
    pub id: String,
    pub source: String,
    pub expect: Record,
    pub host: HostConfig,
    pub fixtures: Vec<Fixture>,
    pub server: Option<VServer>,
    pub abort_at_ms: Option<u64>,
    pub abort_after_steps: Option<u64>,
}

type H<T> = Result<T, String>;

fn closed(r: &Record, allowed: &[&str], what: &str) -> H<()> {
    for k in r.keys() {
        if !allowed.contains(&&**k) {
            return Err(format!("unknown key {k:?} in {what}"));
        }
    }
    Ok(())
}

fn req_str<'a>(r: &'a Record, k: &str, what: &str) -> H<&'a str> {
    r.get(k).and_then(|v| v.as_str()).ok_or_else(|| format!("{what}.{k} must be a string"))
}

fn nonneg(v: &Value, what: &str) -> H<u64> {
    v.as_num().and_then(|d| d.to_u64()).ok_or_else(|| format!("{what} must be a nonnegative integer"))
}

const OPS: &[&str] = &[
    "http",
    "environment",
    "read_regular",
    "replace_regular",
    "log_line",
    "utc_now",
    "random_bytes",
    "pool",
    "acquire",
    "begin",
    "execute",
    "commit",
    "rollback",
    "release",
];

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn validate_expect(e: &Record) -> H<()> {
    let phase = req_str(e, "phase", "expect")?;
    let common = ["phase", "trace", "starts", "max_parallel_active", "responses"];
    let mut allowed: Vec<&str> = common.to_vec();
    match phase {
        "value" => {
            allowed.extend(["json", "kind", "bytes_base64"]);
            let n = ["json", "kind", "bytes_base64"].iter().filter(|k| e.contains(k)).count();
            if n != 1 {
                return Err("value expectation needs exactly one of json, kind, bytes_base64".into());
            }
            if let Some(v) = e.get("json") {
                v.as_str().ok_or("json must be a string")?;
            }
            if let Some(v) = e.get("kind") {
                Kind::from_name(v.as_str().ok_or("kind must be a string")?).ok_or("unknown kind")?;
            }
            if let Some(v) = e.get("bytes_base64") {
                b64().decode(v.as_str().ok_or("bytes_base64 must be a string")?).map_err(|_| "invalid base64")?;
            }
        }
        "lex" | "parse" | "check" | "runtime" => {
            allowed.extend(["code", "data"]);
            req_str(e, "code", "expect")?;
        }
        "abort" => {
            allowed.extend(["reason", "data"]);
            let r = req_str(e, "reason", "expect")?;
            if !["cancelled", "deadline", "resource"].contains(&r) {
                return Err("abort reason must be cancelled, deadline or resource".into());
            }
        }
        _ => return Err(format!("unknown expectation phase {phase:?}")),
    }
    closed(e, &allowed, "expect")?;
    if let Some(d) = e.get("data") {
        d.as_record().ok_or("expect.data must be a record")?;
    }
    if let Some(t) = e.get("trace") {
        let a = t.as_array().ok_or("trace must be an array")?;
        if a.iter().any(|x| x.as_str().is_none()) {
            return Err("trace entries must be strings".into());
        }
    }
    if let Some(s) = e.get("starts") {
        for (_, v) in s.as_record().ok_or("starts must be a record")?.iter() {
            nonneg(v, "starts entry")?;
        }
    }
    if let Some(m) = e.get("max_parallel_active") {
        nonneg(m, "max_parallel_active")?;
    }
    if let Some(rs) = e.get("responses") {
        for r in rs.as_array().ok_or("responses must be an array")?.iter() {
            let rr = r.as_record().ok_or("response must be a record")?;
            closed(rr, &["id", "status", "body_base64", "headers"], "response")?;
            req_str(rr, "id", "response")?;
            nonneg(rr.get("status").ok_or("response.status required")?, "response.status")?;
            b64().decode(req_str(rr, "body_base64", "response")?).map_err(|_| "invalid base64")?;
            if let Some(h) = rr.get("headers") {
                h.as_record().ok_or("response.headers must be a record")?;
            }
        }
    }
    Ok(())
}

fn parse_fixtures(v: &Value) -> H<Vec<Fixture>> {
    let mut out: Vec<Fixture> = Vec::new();
    for f in v.as_array().ok_or("fixtures must be an array")?.iter() {
        let r = f.as_record().ok_or("fixture must be a record")?;
        closed(r, &["id", "op", "match", "ok", "error", "hold", "after_ms", "cancel_ack_ms"], "fixture")?;
        let id = req_str(r, "id", "fixture")?.to_string();
        if id.is_empty() || out.iter().any(|x| x.id == id) {
            return Err(format!("fixture id {id:?} is empty or duplicated"));
        }
        let op = req_str(r, "op", "fixture")?.to_string();
        if !OPS.contains(&op.as_str()) {
            return Err(format!("unknown fixture op {op:?}"));
        }
        let matcher = r.get("match").ok_or("fixture.match required")?.clone();
        matcher.as_record().ok_or("fixture.match must be a record")?;
        VirtualHost::validate_match(&op, &matcher)?;
        let n = ["ok", "error", "hold"].iter().filter(|k| r.contains(k)).count();
        if n != 1 {
            return Err("fixture needs exactly one of ok, error, hold".into());
        }
        let result = if let Some(v) = r.get("ok") {
            VirtualHost::convert_ok(&op, v)?;
            FixtureResult::Ok(v.clone())
        } else if let Some(e) = r.get("error") {
            crate::builtins::pure::validate_error_record("fixture", e).map_err(|_| "fixture error must be {code,message,data}")?;
            FixtureResult::Error(e.clone())
        } else {
            match r.get("hold") {
                Some(Value::Bool(true)) => FixtureResult::Hold,
                _ => return Err("hold must be true".into()),
            }
        };
        let after_ms = match r.get("after_ms") {
            Some(v) => nonneg(v, "after_ms")?,
            None => 0,
        };
        let cancel_ack_ms = match r.get("cancel_ack_ms") {
            Some(v) => nonneg(v, "cancel_ack_ms")?,
            None => 0,
        };
        out.push(Fixture { id, op, matcher, result, after_ms, cancel_ack_ms, consumed: false });
    }
    Ok(out)
}

fn parse_server(v: &Value) -> H<VServer> {
    let r = v.as_record().ok_or("server must be a record")?;
    closed(r, &["requests", "cancel_at_ms"], "server")?;
    let cancel_at_ms = nonneg(r.get("cancel_at_ms").ok_or("server.cancel_at_ms required")?, "cancel_at_ms")?;
    let mut requests: Vec<VRequest> = Vec::new();
    for q in r.get("requests").and_then(|x| x.as_array()).ok_or("server.requests must be an array")?.iter() {
        let qr = q.as_record().ok_or("request must be a record")?;
        closed(qr, &["id", "at_ms", "method", "target", "headers", "body_base64"], "request")?;
        for k in ["id", "at_ms", "method", "target", "headers", "body_base64"] {
            if !qr.contains(k) {
                return Err(format!("request.{k} required"));
            }
        }
        let id = req_str(qr, "id", "request")?.to_string();
        if requests.iter().any(|x| x.id == id) {
            return Err("duplicate request id".into());
        }
        let mut headers = Vec::new();
        for h in qr.get("headers").unwrap().as_array().ok_or("request.headers must be an array")?.iter() {
            let p = h.as_array().ok_or("header pair")?;
            if p.len() != 2 {
                return Err("header pair must have two strings".into());
            }
            headers.push((
                p[0].as_str().ok_or("header name")?.to_string(),
                p[1].as_str().ok_or("header value")?.to_string(),
            ));
        }
        requests.push(VRequest {
            id,
            at_ms: nonneg(qr.get("at_ms").unwrap(), "at_ms")?,
            method: req_str(qr, "method", "request")?.to_string(),
            target: req_str(qr, "target", "request")?.to_string(),
            headers,
            body: b64().decode(req_str(qr, "body_base64", "request")?).map_err(|_| "invalid base64")?,
        });
    }
    Ok(VServer { requests, cancel_at_ms })
}

/// Validate the complete manifest before running any case.
pub fn parse_manifest(payload: &str) -> H<Vec<Case>> {
    let root = crate::json::parse(payload).map_err(|e| format!("manifest is not strict JSON: {e:?}"))?;
    let r = root.as_record().ok_or("manifest root must be a record")?;
    if r.len() != 4 {
        return Err("manifest root keys must be exactly format, language, scheduler, cases".into());
    }
    closed(r, &["format", "language", "scheduler", "cases"], "manifest")?;
    if req_str(r, "format", "manifest")? != "kiroha-conformance/1"
        || req_str(r, "language", "manifest")? != "0.3.0/Web1"
        || req_str(r, "scheduler", "manifest")? != "virtual-fifo/1"
    {
        return Err("manifest format/language/scheduler mismatch".into());
    }
    let cases_v = r.get("cases").and_then(|c| c.as_array()).ok_or("cases must be an array")?;
    if cases_v.is_empty() {
        return Err("cases must be nonempty".into());
    }
    let mut ids = HashSet::new();
    let mut cases = Vec::new();
    for c in cases_v.iter() {
        let cr = c.as_record().ok_or("case must be a record")?;
        closed(cr, &["id", "source", "expect", "host", "fixtures", "server", "abort_at_ms", "abort_after_steps"], "case")?;
        let id = req_str(cr, "id", "case")?.to_string();
        if id.is_empty() || !ids.insert(id.clone()) {
            return Err(format!("case id {id:?} is empty or duplicated"));
        }
        let source = req_str(cr, "source", "case")?.to_string();
        let expect = cr.get("expect").and_then(|e| e.as_record()).ok_or("case.expect must be a record")?.clone();
        validate_expect(&expect).map_err(|e| format!("case {id}: {e}"))?;
        let host = match cr.get("host") {
            Some(h) => parse_case_host(h).map_err(|e| format!("case {id}: {e}"))?,
            None => HostConfig::default_with_root("/".into()),
        };
        let fixtures = match cr.get("fixtures") {
            Some(f) => parse_fixtures(f).map_err(|e| format!("case {id}: {e}"))?,
            None => Vec::new(),
        };
        for f in &fixtures {
            if f.op != "environment" {
                continue;
            }
            let name = f.matcher.as_record().and_then(|m| m.get("name")).and_then(|n| n.as_str());
            let name = name.ok_or_else(|| format!("case {id}: environment fixture must match a specific name"))?;
            let consistent = match (host.env_value(name), &f.result) {
                (Some(v), FixtureResult::Ok(Value::Str(s))) => **s == *v,
                (None, FixtureResult::Error(e)) => {
                    crate::value::err::code_of(e) == "env"
                        && crate::value::err::data_of(e).and_then(|d| d.get("name")).and_then(|n| n.as_str()) == Some(name)
                }
                _ => false,
            };
            if !consistent {
                return Err(format!("case {id}: environment fixture {} disagrees with host.environment", f.id));
            }
        }
        let server = match cr.get("server") {
            Some(s) => Some(parse_server(s).map_err(|e| format!("case {id}: {e}"))?),
            None => None,
        };
        let abort_at_ms = cr.get("abort_at_ms").map(|v| nonneg(v, "abort_at_ms")).transpose()?;
        let abort_after_steps = cr.get("abort_after_steps").map(|v| nonneg(v, "abort_after_steps")).transpose()?;
        cases.push(Case { id, source, expect, host, fixtures, server, abort_at_ms, abort_after_steps });
    }
    Ok(cases)
}

/// What happened when running a case.
pub struct Observed {
    pub phase: String,
    pub value: Option<Value>,
    pub error: Option<Value>,
    pub abort: Option<(String, Value)>,
    pub trace: Vec<String>,
    pub starts: Vec<(String, u64)>,
    pub max_parallel_active: usize,
    pub responses: Vec<crate::host::virtual_host::VResponse>,
    pub harness: Option<String>,
}

pub fn run_case(case: &Case) -> Observed {
    let mut obs = Observed {
        phase: String::new(),
        value: None,
        error: None,
        abort: None,
        trace: Vec::new(),
        starts: Vec::new(),
        max_parallel_active: 0,
        responses: Vec::new(),
        harness: None,
    };
    let prog = match crate::compile(&case.source) {
        Ok(p) => p,
        Err(d) => {
            obs.phase = d.phase.to_string();
            obs.error = Some(d.error_value());
            return obs;
        }
    };
    let host = VirtualHost::new(case.fixtures.clone(), case.server.clone());
    let mut rt = Runtime::new(prog, host, case.host.clone());
    rt.start_root(case.abort_after_steps);
    if let Some(at) = case.abort_at_ms {
        rt.schedule_root_cancel(at);
    }
    let tokio_rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
    let outcome = tokio_rt.block_on(rt.run());
    let host = &rt.host;
    obs.trace = host.trace.clone();
    obs.starts = host.starts.iter().map(|(k, v)| (k.clone(), *v)).collect();
    obs.max_parallel_active = rt.stats.max_parallel_active;
    obs.responses = host.responses.clone();
    if let Some(f) = &host.failure {
        obs.harness = Some(f.clone());
    }
    match outcome {
        RootOutcome::Value(v) => {
            obs.phase = "value".into();
            obs.value = Some(v);
        }
        RootOutcome::Error { error, .. } => {
            obs.phase = "runtime".into();
            obs.error = Some(error);
        }
        RootOutcome::Abort { reason, data } => {
            obs.phase = "abort".into();
            obs.abort = Some((reason.to_string(), data));
        }
        RootOutcome::Harness(m) => {
            obs.phase = "harness".into();
            if obs.harness.is_none() {
                obs.harness = Some(m);
            }
        }
    }
    let unconsumed = rt.host.unconsumed();
    if obs.harness.is_none() && !unconsumed.is_empty() {
        obs.harness = Some(format!("unconsumed fixtures: {}", unconsumed.join(", ")));
    }
    obs
}

/// Recursive required-subset match for expectation data.
pub fn data_subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Record(e), Value::Record(a)) => e.iter().all(|(k, ev)| a.get(k).is_some_and(|av| data_subset(ev, av))),
        _ => expected.is_json() && actual.is_json() && json_equal(expected, actual),
    }
}

/// Compare an observation with the expectation; returns a mismatch reason.
pub fn compare(case: &Case, obs: &Observed) -> Option<String> {
    if let Some(h) = &obs.harness {
        return Some(format!("harness failure: {h}"));
    }
    let e = &case.expect;
    let phase = e.get("phase").and_then(|p| p.as_str()).unwrap();
    if obs.phase != phase {
        let detail = match (&obs.error, &obs.value) {
            (Some(err), _) => crate::wire::encode_json_lossless(err),
            (None, Some(v)) if v.is_json() => crate::json::dump(v),
            _ => String::new(),
        };
        return Some(format!("expected phase {phase}, observed {} {detail}", obs.phase));
    }
    match phase {
        "value" => {
            let v = obs.value.as_ref().unwrap();
            if let Some(j) = e.get("json").and_then(|x| x.as_str()) {
                if !v.is_json() {
                    return Some(format!("expected JSON {j}, observed non-JSON {}", v.kind().name()));
                }
                let got = crate::json::dump(v);
                if got != j {
                    return Some(format!("expected {j}, observed {got}"));
                }
            }
            if let Some(k) = e.get("kind").and_then(|x| x.as_str()) {
                if v.kind().name() != k {
                    return Some(format!("expected kind {k}, observed {}", v.kind().name()));
                }
            }
            if let Some(b) = e.get("bytes_base64").and_then(|x| x.as_str()) {
                match v {
                    Value::Bytes(bytes) if b64().encode(&**bytes) == b => {}
                    _ => return Some(format!("expected bytes {b}")),
                }
            }
        }
        "abort" => {
            let (reason, data) = obs.abort.as_ref().unwrap();
            if e.get("reason").and_then(|r| r.as_str()) != Some(reason.as_str()) {
                return Some(format!("expected abort reason {:?}, observed {reason}", e.get("reason")));
            }
            if let Some(d) = e.get("data") {
                if !data_subset(d, data) {
                    return Some("abort data mismatch".into());
                }
            }
        }
        _ => {
            let err = obs.error.as_ref().unwrap();
            let code = crate::value::err::code_of(err);
            if e.get("code").and_then(|c| c.as_str()) != Some(code) {
                return Some(format!("expected code {:?}, observed {}", e.get("code").map(crate::json::dump), crate::json::dump(err)));
            }
            if let Some(d) = e.get("data") {
                let actual = err.as_record().and_then(|r| r.get("data")).cloned().unwrap_or(Value::Null);
                if !data_subset(d, &actual) {
                    return Some(format!("error data mismatch: observed {}", crate::json::dump(&actual)));
                }
            }
        }
    }
    if let Some(t) = e.get("trace").and_then(|t| t.as_array()) {
        let want: Vec<&str> = t.iter().map(|x| x.as_str().unwrap()).collect();
        if want != obs.trace {
            return Some(format!("trace mismatch: expected {want:?}, observed {:?}", obs.trace));
        }
    }
    if let Some(s) = e.get("starts").and_then(|s| s.as_record()) {
        for (k, v) in s.iter() {
            let want = v.as_num().and_then(|d| d.to_u64()).unwrap();
            match obs.starts.iter().find(|(n, _)| **n == **k) {
                Some((_, got)) if *got == want => {}
                other => return Some(format!("start of {k}: expected {want}, observed {other:?}")),
            }
        }
    }
    if let Some(m) = e.get("max_parallel_active") {
        let want = m.as_num().and_then(|d| d.to_u64()).unwrap() as usize;
        if want != obs.max_parallel_active {
            return Some(format!("max_parallel_active: expected {want}, observed {}", obs.max_parallel_active));
        }
    }
    if let Some(rs) = e.get("responses").and_then(|r| r.as_array()) {
        if rs.len() != obs.responses.len() {
            return Some(format!("expected {} responses, observed {}", rs.len(), obs.responses.len()));
        }
        for (want, got) in rs.iter().zip(&obs.responses) {
            let w = want.as_record().unwrap();
            let id = w.get("id").and_then(|x| x.as_str()).unwrap();
            let status = w.get("status").and_then(|x| x.as_num()).and_then(|d| d.to_u64()).unwrap() as u16;
            let body = w.get("body_base64").and_then(|x| x.as_str()).unwrap();
            if got.id != id || got.status != status || b64().encode(&got.body) != body {
                return Some(format!(
                    "response mismatch: expected {id} {status} {body}, observed {} {} {}",
                    got.id,
                    got.status,
                    b64().encode(&got.body)
                ));
            }
            if let Some(h) = w.get("headers").and_then(|h| h.as_record()) {
                let raw: Vec<(String, Vec<u8>)> = got.headers.iter().map(|(n, v)| (n.clone(), v.as_bytes().to_vec())).collect();
                let norm = crate::builtins::headers::incoming(&raw);
                for (k, v) in h.iter() {
                    match norm.get(k) {
                        Some(a) if json_equal(a, v) => {}
                        _ => return Some(format!("response {id} header {k} mismatch")),
                    }
                }
            }
        }
    }
    None
}

fn observed_value(obs: &Observed) -> Value {
    let mut fields: Vec<(&str, Value)> = vec![("phase", Value::str(&obs.phase))];
    if let Some(v) = &obs.value {
        fields.push(("result", crate::wire::to_wire(v)));
    }
    if let Some(e) = &obs.error {
        fields.push(("error", e.clone()));
    }
    if let Some((r, d)) = &obs.abort {
        fields.push(("reason", Value::str(r)));
        fields.push(("data", d.clone()));
    }
    fields.push(("trace", Value::array(obs.trace.iter().map(|s| Value::str(s)).collect())));
    if !obs.starts.is_empty() {
        let mut r = Record::new();
        for (k, v) in &obs.starts {
            r.push(k, Value::uint(*v));
        }
        fields.push(("starts", Value::record(r)));
    }
    if obs.max_parallel_active > 0 {
        fields.push(("max_parallel_active", Value::int(obs.max_parallel_active as i64)));
    }
    if !obs.responses.is_empty() {
        fields.push((
            "responses",
            Value::array(
                obs.responses
                    .iter()
                    .map(|r| {
                        Value::rec(vec![
                            ("id", Value::str(&r.id)),
                            ("status", Value::int(r.status as i64)),
                            ("body_base64", Value::str(&b64().encode(&r.body))),
                        ])
                    })
                    .collect(),
            ),
        ));
    }
    if let Some(h) = &obs.harness {
        fields.push(("harness", Value::str(h)));
    }
    Value::rec(fields)
}

/// Run every case; returns the report value and whether all passed.
pub fn run_all(cases: &[Case]) -> (Value, bool) {
    let mut passed = 0;
    let mut reports = Vec::new();
    for c in cases {
        let obs = run_case(c);
        let mismatch = compare(c, &obs);
        let ok = mismatch.is_none();
        if ok {
            passed += 1;
        }
        let mut fields = vec![
            ("id", Value::str(&c.id)),
            ("status", Value::str(if ok { "pass" } else { "fail" })),
            ("observed", observed_value(&obs)),
        ];
        if let Some(m) = mismatch {
            fields.push(("mismatch", Value::str(&m)));
        }
        reports.push(Value::rec(fields));
    }
    let total = cases.len();
    let report = Value::rec(vec![
        ("protocol", Value::str("kiroha-conformance/1")),
        ("total", Value::int(total as i64)),
        ("passed", Value::int(passed as i64)),
        ("failed", Value::int((total - passed) as i64)),
        ("cases", Value::array(reports)),
    ]);
    (report, passed == total)
}
