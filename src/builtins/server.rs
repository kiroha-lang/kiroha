//! `serve`, `input`, `reply` and request lifecycle (spec §12).

use super::args::*;
use super::headers;
use super::pure::json_fail;
use crate::host::{BodyFault, EffOutcome, EffValue, Effect, Host, IncomingRequest};
use crate::runtime::frames::Frame;
use crate::runtime::{
    AbortReason, AbortSig, BResult, Ctl, DispatchOpts, DoneTo, Fail, Runtime, ScopeId, Task, TaskId, TaskOutcome,
};
use crate::schema;
use crate::value::{err, Record, Reply, Value};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum Seg {
    Lit(String),
    Param(String),
}

#[derive(Clone, Debug)]
pub struct Route {
    pub method: String,
    pub segs: Vec<Seg>,
    pub handler: Value,
}

impl Route {
    fn literal_count(&self) -> usize {
        self.segs.iter().filter(|s| matches!(s, Seg::Lit(_))).count()
    }

    fn matches(&self, path: &[String]) -> Option<Record> {
        if self.segs.len() != path.len() {
            return None;
        }
        let mut params = Record::new();
        for (s, p) in self.segs.iter().zip(path) {
            match s {
                Seg::Lit(l) => {
                    if l != p {
                        return None;
                    }
                }
                Seg::Param(n) => params.push(n, Value::str(p)),
            }
        }
        Some(params)
    }
}

pub struct ServeOpts {
    pub host: String,
    pub port: u16,
    pub timeout: u64,
    pub max_body: u64,
    pub max_inflight: u64,
}

pub struct ReqState {
    pub task: Option<TaskId>,
    pub scope: Option<ScopeId>,
    pub counted: bool,
}

pub struct ServerState {
    pub task: TaskId,
    pub scope: ScopeId,
    pub listener: Option<String>,
    pub routes: Vec<Route>,
    pub opts: ServeOpts,
    pub inflight: u64,
    pub requests: HashMap<u64, ReqState>,
    pub closing: bool,
    pub woke: bool,
    pub shutting: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ReqPhase {
    Body,
    Handler,
    Sending,
}

pub struct RequestFrame {
    pub server: u32,
    pub request: u64,
    pub phase: ReqPhase,
    pub head: bool,
    pub method: String,
    pub raw_path: String,
    pub segments: Vec<String>,
    pub query: Record,
    pub headers: Record,
}

fn error_body(kind: &str) -> Vec<u8> {
    format!("{{\"error\":\"{kind}\"}}").into_bytes()
}

/// Strict single percent-decoding to UTF-8; `plus_space` decodes `+`.
pub fn percent_decode(s: &str, plus_space: bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = b.get(i + 1..i + 3)?;
                if !h.iter().all(|c| c.is_ascii_hexdigit()) {
                    return None;
                }
                out.push(u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?);
                i += 3;
            }
            b'+' if plus_space => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn split_path(path: &str) -> Vec<&str> {
    if path == "/" {
        return Vec::new();
    }
    path[1..].split('/').collect()
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(x) if x.is_ascii_alphabetic() || x == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

fn parse_route(key: &str) -> Result<(String, Vec<Seg>), String> {
    let (method, path) = key.split_once(' ').ok_or("route must be `METHOD /path`")?;
    if !headers::valid_method(method) {
        return Err("invalid route method".into());
    }
    if !path.starts_with('/') || path.contains(['?', '#', ' ']) || !path.is_ascii() || path.bytes().any(|c| c < 0x21 || c == 0x7f) {
        return Err("invalid route path".into());
    }
    let mut segs = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for raw in split_path(path) {
        if let Some(name) = raw.strip_prefix(':') {
            if !is_ident(name) {
                return Err("invalid route parameter".into());
            }
            if names.iter().any(|n| n == name) {
                return Err("repeated route parameter name".into());
            }
            names.push(name.to_string());
            segs.push(Seg::Param(name.to_string()));
        } else {
            let lit = percent_decode(raw, false).ok_or("malformed route encoding")?;
            segs.push(Seg::Lit(lit));
        }
    }
    Ok((method.to_string(), segs))
}

fn ambiguous(a: &Route, b: &Route) -> bool {
    a.method == b.method
        && a.segs.len() == b.segs.len()
        && a.literal_count() == b.literal_count()
        && a.segs.iter().zip(&b.segs).all(|(x, y)| match (x, y) {
            (Seg::Lit(p), Seg::Lit(q)) => p == q,
            _ => true,
        })
}

/// Parse a request query string (§12.2).
fn parse_query(q: &str) -> Option<Record> {
    let mut acc: Vec<(String, Vec<Value>)> = Vec::new();
    for comp in q.split('&') {
        if comp.is_empty() {
            continue;
        }
        let (k, v) = match comp.split_once('=') {
            Some((k, v)) => (k, v),
            None => (comp, ""),
        };
        let k = percent_decode(k, true)?;
        let v = percent_decode(v, true)?;
        match acc.iter_mut().find(|(n, _)| *n == k) {
            Some((_, vals)) => vals.push(Value::str(&v)),
            None => acc.push((k, vec![Value::str(&v)])),
        }
    }
    let mut r = Record::new();
    for (k, vals) in acc {
        r.push(&k, Value::array(vals));
    }
    Some(r)
}

fn valid_target(t: &str) -> bool {
    t.starts_with('/')
        && t.is_ascii()
        && !t.contains('#')
        && !t.bytes().any(|c| c < 0x21 || c == 0x7f)
        && {
            let b = t.as_bytes();
            let mut ok = true;
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'%' {
                    if !(b.get(i + 1).is_some_and(|c| c.is_ascii_hexdigit()) && b.get(i + 2).is_some_and(|c| c.is_ascii_hexdigit())) {
                        ok = false;
                        break;
                    }
                    i += 3;
                } else {
                    i += 1;
                }
            }
            ok
        }
}

impl<H: Host> Runtime<H> {
    pub fn start_serve(&mut self, t: &mut Task, a: Vec<Value>) -> Ctl {
        let op = "serve";
        let res: Result<(Vec<Route>, ServeOpts), Fail> = (|| {
            let routes_v = record(op, 1, &a[0])?;
            let mut routes: Vec<Route> = Vec::new();
            for (k, h) in routes_v.iter() {
                let (method, segs) = parse_route(k).map_err(|r| arg_err(op, &r))?;
                callback(op, h, 1)?;
                routes.push(Route { method, segs, handler: h.clone() });
            }
            for i in 0..routes.len() {
                for j in i + 1..routes.len() {
                    if ambiguous(&routes[i], &routes[j]) {
                        return Err(arg_err(op, "ambiguous routes"));
                    }
                }
            }
            let o = options(op, 2, a.get(1), &["host", "port", "timeout", "max_body", "max_inflight"])?;
            let host = match field(o, "host") {
                Some(Value::Str(s)) => {
                    if s.parse::<std::net::IpAddr>().is_err() {
                        return Err(arg_err(op, "host must be a numeric IP address"));
                    }
                    s.to_string()
                }
                Some(other) => return Err(opt_type_err(op, "host", "string", other)),
                None => "127.0.0.1".to_string(),
            };
            let port = match field(o, "port") {
                Some(v) => {
                    let p = opt_int(op, "port", v, 1)?;
                    if p > 65535 {
                        return Err(arg_err(op, "port out of range"));
                    }
                    p as u16
                }
                None => 8080,
            };
            let timeout = match field(o, "timeout") {
                Some(v) => opt_int(op, "timeout", v, 1)?,
                None => 30000,
            };
            let max_body = match field(o, "max_body") {
                Some(v) => opt_int(op, "max_body", v, 0)?,
                None => 1_048_576,
            };
            let max_inflight = match field(o, "max_inflight") {
                Some(v) => opt_int(op, "max_inflight", v, 1)?,
                None => 128,
            };
            reject_unknown(op, o, &["host", "port", "timeout", "max_body", "max_inflight"])?;
            Ok((routes, ServeOpts { host, port, timeout, max_body, max_inflight }))
        })();
        let (routes, opts) = match res {
            Ok(x) => x,
            Err(e) => return self.fail_ctl(t, e),
        };
        if !self.config.capabilities.listen.iter().any(|(h, p)| *h == opts.host && *p == opts.port) {
            let e = err::permission(op, "listen");
            return self.raise(t, e);
        }
        let sid = self.next_server;
        self.next_server += 1;
        let eff = Effect::Listen { host: opts.host.clone(), port: opts.port };
        self.servers.insert(
            sid,
            ServerState {
                task: t.id,
                scope: t.scope,
                listener: None,
                routes,
                opts,
                inflight: 0,
                requests: HashMap::new(),
                closing: false,
                woke: false,
                shutting: false,
            },
        );
        t.stack.push(Frame::Serve(sid));
        Ctl::Dispatch(eff, DispatchOpts::default())
    }

    pub fn serve_effect(&mut self, t: &mut Task, sid: u32, o: EffOutcome) -> Ctl {
        let st = match self.servers.get_mut(&sid) {
            Some(s) => s,
            None => return Ctl::Ret(Value::Null),
        };
        if st.shutting {
            // listener closed after abort
            let sig = st_sig(self, t);
            self.servers.remove(&sid);
            return match sig {
                Some(s) => Ctl::Abort(s, None),
                None => {
                    let e = err::make(
                        "io",
                        "serve: listener failed",
                        vec![
                            ("operation", Value::str("serve")),
                            ("reason", Value::str("listener")),
                            ("outcome", Value::str("applied")),
                        ],
                    );
                    self.raise(t, e)
                }
            };
        }
        match o {
            EffOutcome::Ok(EffValue::Str(listener)) => {
                st.listener = Some(listener);
                t.stack.push(Frame::Serve(sid));
                Ctl::Waiting
            }
            EffOutcome::Err(e) => {
                self.servers.remove(&sid);
                self.raise(t, e)
            }
            _ => {
                self.servers.remove(&sid);
                self.joined_without_abort(t)
            }
        }
    }

    pub fn serve_abort(&mut self, t: &mut Task, sid: u32, sig: AbortSig, _carry: Option<EffOutcome>) -> Ctl {
        let listener = match self.servers.get_mut(&sid) {
            Some(st) => {
                st.shutting = true;
                st.listener.take()
            }
            None => None,
        };
        match listener {
            Some(l) => {
                t.stack.push(Frame::Serve(sid));
                Ctl::Dispatch(Effect::CloseListener { listener: l }, DispatchOpts { cleanup: true, timeout: None })
            }
            None => {
                self.servers.remove(&sid);
                Ctl::Abort(sig, None)
            }
        }
    }

    /// Called when any scope aborts: a server whose scope is aborted stops
    /// accepting, and its serve task wakes once requests are joined.
    pub fn server_scope_aborted(&mut self, sid: u32) {
        let (scope, task, idle, woke) = match self.servers.get(&sid) {
            Some(s) => (s.scope, s.task, s.requests.is_empty(), s.woke),
            None => return,
        };
        if self.abort_of(scope).is_none() {
            return;
        }
        if let Some(s) = self.servers.get_mut(&sid) {
            s.closing = true;
        }
        if idle && !woke {
            self.wake_serve(sid, task);
        }
        let _ = scope;
    }

    fn wake_serve(&mut self, sid: u32, task: TaskId) {
        let scope = match self.servers.get_mut(&sid) {
            Some(s) => {
                if s.woke {
                    return;
                }
                s.woke = true;
                s.closing = false;
                s.scope
            }
            None => return,
        };
        let sig = self.abort_of(scope).unwrap_or(AbortSig { scope, reason: AbortReason::Cancelled });
        if let Some(t) = self.tasks.get_mut(&task) {
            if matches!(t.ctl, Ctl::Waiting) && t.waiting_eff.is_none() {
                t.ctl = Ctl::Abort(sig, None);
                self.ready.push_back(task);
            }
        }
    }

    fn respond_task(&mut self, sid: u32, req: &IncomingRequest, status: u16, kind: &str, counted: bool) {
        let head = req.method == "HEAD";
        let scope = match self.servers.get(&sid) {
            Some(s) => s.scope,
            None => return,
        };
        let frame = RequestFrame {
            server: sid,
            request: req.id,
            phase: ReqPhase::Sending,
            head,
            method: req.method.clone(),
            raw_path: String::new(),
            segments: Vec::new(),
            query: Record::new(),
            headers: Record::new(),
        };
        let budget = self.new_budget(scope, None);
        let (hdrs, body) = response_parts(status, vec![("content-type".into(), "application/json".into())], error_body(kind), head);
        let ctl = Ctl::Dispatch(
            Effect::SendResponse { request: req.id, status, headers: hdrs, body, suppress_body: head },
            DispatchOpts { cleanup: true, timeout: None },
        );
        let tid = self.spawn(scope, budget, ctl, DoneTo::Request { server: sid, request: req.id });
        if let Some(t) = self.tasks.get_mut(&tid) {
            t.stack.push(Frame::Request(Box::new(frame)));
        }
        if let Some(s) = self.servers.get_mut(&sid) {
            s.requests.insert(req.id, ReqState { task: Some(tid), scope: None, counted });
        }
    }

    pub fn server_request(&mut self, req: IncomingRequest) {
        let sid = match self.servers.iter().find(|(_, s)| s.listener.as_deref() == Some(req.listener.as_str())) {
            Some((id, _)) => *id,
            None => return,
        };
        {
            let st = self.servers.get(&sid).unwrap();
            if st.closing {
                return;
            }
        }
        // Stage 1: transport/target/method.
        let has_expect = req.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("expect"));
        if !headers::is_token(&req.method) || req.method == "CONNECT" || req.upgrade || has_expect || !valid_target(&req.target) {
            self.respond_task(sid, &req, 400, "bad_request", false);
            return;
        }
        let (raw_path, query_text) = match req.target.split_once('?') {
            Some((p, q)) => (p.to_string(), Some(q.to_string())),
            None => (req.target.clone(), None),
        };
        let segments: Option<Vec<String>> = split_path(&raw_path).into_iter().map(|s| percent_decode(s, false)).collect();
        let query = match &query_text {
            Some(q) => parse_query(q),
            None => Some(Record::new()),
        };
        let (segments, query) = match (segments, query) {
            (Some(s), Some(q)) => (s, q),
            _ => {
                self.respond_task(sid, &req, 400, "bad_request", false);
                return;
            }
        };
        // Stage 2: admission.
        let (inflight, max, timeout, max_body, scope) = {
            let st = self.servers.get(&sid).unwrap();
            (st.inflight, st.opts.max_inflight, st.opts.timeout, st.opts.max_body, st.scope)
        };
        if inflight >= max || self.live_tasks + 1 > self.config.limits.max_tasks {
            self.respond_task(sid, &req, 503, "unavailable", false);
            return;
        }
        // Stage 3: request deadline and task.
        let rscope = self.new_scope(Some(scope));
        let at = self.now_ms() + timeout;
        self.add_scope_deadline(rscope, at, None);
        let budget = self.new_budget(rscope, None);
        let hdr_record = headers::incoming(&req.headers);
        let coding_ok = req
            .headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("content-encoding"))
            .all(|(_, v)| headers::identity_coding(Some(&headers::latin1(v))));
        let frame = RequestFrame {
            server: sid,
            request: req.id,
            phase: ReqPhase::Body,
            head: req.method == "HEAD",
            method: req.method.clone(),
            raw_path,
            segments,
            query,
            headers: hdr_record,
        };
        let ctl = if coding_ok {
            Ctl::Dispatch(Effect::ReadBody { request: req.id, max_body }, DispatchOpts::default())
        } else {
            Ctl::Effect(EffOutcome::Err(err::make("media", "content coding", vec![("reason", Value::str("content_encoding"))])))
        };
        let tid = self.spawn(rscope, budget, ctl, DoneTo::Request { server: sid, request: req.id });
        if let Some(t) = self.tasks.get_mut(&tid) {
            t.stack.push(Frame::Request(Box::new(frame)));
        }
        let st = self.servers.get_mut(&sid).unwrap();
        st.inflight += 1;
        st.requests.insert(req.id, ReqState { task: Some(tid), scope: Some(rscope), counted: true });
    }

    pub fn server_disconnect(&mut self, request: u64) {
        let found = self.servers.iter().find_map(|(sid, s)| s.requests.get(&request).map(|r| (*sid, r.scope)));
        if let Some((_, Some(scope))) = found {
            self.abort_scope(scope, AbortReason::Cancelled);
        }
    }

    pub fn request_done(&mut self, sid: u32, request: u64, _outcome: TaskOutcome) {
        let (task, idle_closing) = {
            let st = match self.servers.get_mut(&sid) {
                Some(s) => s,
                None => return,
            };
            if let Some(r) = st.requests.remove(&request) {
                if r.counted {
                    st.inflight -= 1;
                }
                if let Some(s) = r.scope {
                    self.scopes.remove(&s);
                }
            }
            let st = self.servers.get(&sid).unwrap();
            (st.task, st.requests.is_empty() && self.abort_of(st.scope).is_some())
        };
        if idle_closing {
            self.wake_serve(sid, task);
        }
    }

    fn send(&mut self, t: &mut Task, mut f: RequestFrame, status: u16, hdrs: Vec<(String, String)>, body: Vec<u8>, cleanup: bool) -> Ctl {
        let head = f.head;
        let (hdrs, body) = response_parts(status, hdrs, body, head);
        f.phase = ReqPhase::Sending;
        let request = f.request;
        t.stack.push(Frame::Request(Box::new(f)));
        Ctl::Dispatch(
            Effect::SendResponse { request, status, headers: hdrs, body, suppress_body: head },
            DispatchOpts { cleanup, timeout: None },
        )
    }

    fn send_error(&mut self, t: &mut Task, f: RequestFrame, status: u16, kind: &str, cleanup: bool) -> Ctl {
        self.send(t, f, status, vec![("content-type".into(), "application/json".into())], error_body(kind), cleanup)
    }

    pub fn request_effect(&mut self, t: &mut Task, f: RequestFrame, o: EffOutcome) -> Ctl {
        match f.phase {
            ReqPhase::Body => match o {
                EffOutcome::Ok(EffValue::Body(Ok(body))) => self.route(t, f, body),
                EffOutcome::Ok(EffValue::Body(Err(BodyFault::TooLarge))) => self.send_error(t, f, 413, "payload_too_large", false),
                EffOutcome::Err(e) if err::code_of(&e) == "media" => self.send_error(t, f, 415, "unsupported_media_type", false),
                _ => self.send_error(t, f, 400, "bad_request", false),
            },
            ReqPhase::Sending => Ctl::Ret(Value::Null),
            ReqPhase::Handler => {
                self.harness_failure = Some("effect delivered to a request handler frame".into());
                Ctl::Waiting
            }
        }
    }

    fn route(&mut self, t: &mut Task, mut f: RequestFrame, body: Vec<u8>) -> Ctl {
        let st = self.servers.get(&f.server).unwrap();
        let mut best: Option<(usize, Record, Value)> = None;
        let mut allow: Vec<String> = Vec::new();
        for r in &st.routes {
            if let Some(params) = r.matches(&f.segments) {
                if r.method == f.method {
                    let lc = r.literal_count();
                    if best.as_ref().map(|(b, _, _)| lc > *b).unwrap_or(true) {
                        best = Some((lc, params, r.handler.clone()));
                    }
                } else if !allow.contains(&r.method) {
                    allow.push(r.method.clone());
                }
            }
        }
        match best {
            Some((_, params, handler)) => {
                let req = Value::rec(vec![
                    ("method", Value::str(&f.method)),
                    ("path", Value::str(&f.raw_path)),
                    ("params", Value::record(params)),
                    ("query", Value::record(std::mem::take(&mut f.query))),
                    ("headers", Value::record(std::mem::take(&mut f.headers))),
                    ("body", Value::bytes(body)),
                ]);
                f.phase = ReqPhase::Handler;
                t.stack.push(Frame::Request(Box::new(f)));
                Ctl::Apply(handler, vec![req], None)
            }
            None if !allow.is_empty() => {
                allow.sort();
                let hdrs = vec![("content-type".into(), "application/json".into()), ("allow".into(), allow.join(", "))];
                self.send(t, f, 405, hdrs, error_body("method_not_allowed"), false)
            }
            None => self.send_error(t, f, 404, "not_found", false),
        }
    }

    pub fn request_ret(&mut self, t: &mut Task, f: RequestFrame, v: Value) -> Ctl {
        match v {
            Value::Reply(r) => {
                let hdrs = r.headers.clone();
                self.send(t, f, r.status, hdrs, r.body.clone(), false)
            }
            ref other if other.is_json() => {
                let body = crate::json::dump(other).into_bytes();
                self.send(t, f, 200, vec![("content-type".into(), "application/json".into())], body, false)
            }
            other => {
                self.log_request_error("type", &f.raw_path);
                let _ = other;
                self.send_error(t, f, 500, "internal", false)
            }
        }
    }

    pub fn request_throw(&mut self, t: &mut Task, f: RequestFrame, e: Value) -> Ctl {
        if f.phase != ReqPhase::Handler {
            return Ctl::Throw(e);
        }
        match err::code_of(&e) {
            "input" => self.send_error(t, f, 400, "bad_request", false),
            "media" => self.send_error(t, f, 415, "unsupported_media_type", false),
            code => {
                let code = code.to_string();
                self.log_request_error(&code, &f.raw_path);
                self.send_error(t, f, 500, "internal", false)
            }
        }
    }

    pub fn request_abort(&mut self, t: &mut Task, f: RequestFrame, sig: AbortSig, _carry: Option<EffOutcome>) -> Ctl {
        let own = self.servers.get(&f.server).and_then(|s| s.requests.get(&f.request)).and_then(|r| r.scope);
        let request_scope_only = own == Some(sig.scope);
        if f.phase == ReqPhase::Sending || !request_scope_only {
            return Ctl::Abort(sig, None);
        }
        match sig.reason {
            AbortReason::Deadline => {
                t.unwinding = None;
                self.send_error(t, f, 504, "timeout", true)
            }
            AbortReason::Resource(_) => {
                t.unwinding = None;
                self.send_error(t, f, 503, "unavailable", true)
            }
            _ => Ctl::Abort(sig, None),
        }
    }

    fn log_request_error(&mut self, code: &str, path: &str) {
        let line = crate::json::dump(&Value::rec(vec![
            ("event", Value::str("request_error")),
            ("code", Value::str(code)),
            ("path", Value::str(path)),
        ]));
        self.host.diagnostic(&line);
    }
}

fn st_sig<H: Host>(rt: &Runtime<H>, t: &Task) -> Option<AbortSig> {
    rt.abort_of(t.scope)
}

/// Final response headers (content-type/length rules, §12.3).
pub fn response_parts(status: u16, mut hdrs: Vec<(String, String)>, body: Vec<u8>, _head: bool) -> (Vec<(String, String)>, Vec<u8>) {
    let bodyless = matches!(status, 204 | 205 | 304);
    if bodyless {
        hdrs.retain(|(n, _)| n != "content-type");
        if status == 205 {
            hdrs.push(("content-length".into(), "0".into()));
        }
        return (hdrs, Vec::new());
    }
    hdrs.push(("content-length".into(), body.len().to_string()));
    (hdrs, body)
}

/// `input(r,S=json)` (§12.3).
pub fn input(a: &[Value]) -> BResult {
    let op = "input";
    let s = match a.get(1) {
        Some(sv) => schema::normalize(sv)?,
        None => schema::Norm::Json,
    };
    let r = record(op, 1, &a[0])?;
    let keys = ["method", "path", "params", "query", "headers", "body"];
    if r.len() != 6 || !keys.iter().all(|k| r.contains(k)) {
        return Err(arg_err(op, "request must have exactly method, path, params, query, headers and body"));
    }
    string(op, 1, r.get("method").unwrap())?;
    string(op, 1, r.get("path").unwrap())?;
    let params = record(op, 1, r.get("params").unwrap())?;
    for (_, v) in params.iter() {
        string(op, 1, v)?;
    }
    for name in ["query", "headers"] {
        let m = record(op, 1, r.get(name).unwrap())?;
        for (k, v) in m.iter() {
            let arr = array(op, 1, v)?;
            for x in arr.iter() {
                string(op, 1, x)?;
            }
            if name == "headers" && k.bytes().any(|c| c.is_ascii_uppercase()) {
                return Err(arg_err(op, "header names must be lowercase"));
            }
        }
    }
    let body = bytes(op, 1, r.get("body").unwrap())?;
    let hdrs = r.get("headers").unwrap().as_record().unwrap();
    let media = |reason: &str| Fail::Err(err::make("media", format!("unsupported media: {reason}"), vec![("reason", Value::str(reason))]));
    if let Some(ce) = hdrs.get("content-encoding") {
        for v in ce.as_array().unwrap().iter() {
            if !headers::identity_coding(v.as_str()) {
                return Err(media("content_encoding"));
            }
        }
    }
    match hdrs.get("content-type").and_then(|v| v.as_array()) {
        Some(vals) if vals.len() == 1 && headers::is_json_media(vals[0].as_str().unwrap()) => {}
        Some(vals) if vals.len() > 1 => return Err(media("content_type")),
        _ => return Err(media("content_type")),
    }
    let input_err = |reason: &str, extra: Vec<(&str, Value)>| {
        let mut data = vec![("reason", Value::str(reason))];
        data.extend(extra);
        Fail::Err(err::make("input", format!("bad request input: {reason}"), data))
    };
    let text = match std::str::from_utf8(body) {
        Ok(t) => t,
        Err(_) => return Err(input_err("utf8", vec![])),
    };
    let v = match crate::json::parse(text) {
        Ok(v) => v,
        Err(crate::json::JsonError::Overflow) => return Err(json_fail(crate::json::JsonError::Overflow)),
        Err(_) => return Err(input_err("json", vec![])),
    };
    match schema::validate(&v, &s) {
        Ok(()) => Ok(v),
        Err(m) => Err(input_err(
            "validation",
            vec![("path", m.path_value()), ("expected", Value::str(m.expected)), ("actual", Value::str(m.actual))],
        )),
    }
}

/// `reply(status,v,headers={})` (§12.3).
pub fn reply(a: &[Value]) -> BResult {
    let op = "reply";
    let status = integer(op, 1, &a[0])?;
    let status: u16 = match u16::try_from(&status) {
        Ok(s) if (200..=599).contains(&s) => s,
        _ => return Err(arg_err(op, "status must be 200..599")),
    };
    let v = &a[1];
    let hdrs = match a.get(2) {
        Some(h) => match h {
            Value::Record(_) => headers::outbound(op, h)?,
            other => return Err(type_err(op, 3, "record", other)),
        },
        None => Vec::new(),
    };
    let is_bytes = matches!(v, Value::Bytes(_));
    if !is_bytes {
        if let Some(k) = v.first_non_json() {
            return Err(Fail::Err(err::type_error(op, err::Pos::Index(2), "json or bytes", k)));
        }
    }
    let ct = headers::get(&hdrs, "content-type").map(|s| s.to_string());
    let ce = headers::get(&hdrs, "content-encoding").map(|s| s.to_string());
    let mut out = hdrs.clone();
    let body;
    if matches!(status, 204 | 205 | 304) {
        if !matches!(v, Value::Null) {
            return Err(arg_err(op, "bodyless status requires null"));
        }
        if ct.is_some() {
            return Err(arg_err(op, "bodyless reply cannot have Content-Type"));
        }
        body = Vec::new();
    } else if is_bytes {
        if let Some(c) = &ct {
            if !headers::is_media_type(c) {
                return Err(arg_err(op, "invalid Content-Type"));
            }
        } else {
            out.push(("content-type".into(), "application/octet-stream".into()));
        }
        body = match v {
            Value::Bytes(b) => b.to_vec(),
            _ => unreachable!(),
        };
    } else {
        match &ct {
            Some(c) if !headers::is_json_media(c) => return Err(arg_err(op, "JSON reply Content-Type must be application/json")),
            Some(_) => {}
            None => out.push(("content-type".into(), "application/json".into())),
        }
        if !headers::identity_coding(ce.as_deref()) {
            return Err(arg_err(op, "JSON reply Content-Encoding must be identity"));
        }
        body = crate::json::dump(v).into_bytes();
    }
    Ok(Value::Reply(Rc::new(Reply { status, headers: out, body })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        let (m, s) = parse_route("GET /a/:x").unwrap();
        assert_eq!(m, "GET");
        assert_eq!(s.len(), 2);
        assert!(parse_route("GET /:x/:x").is_err());
        assert!(parse_route("get /").is_err());
        assert!(parse_route("GET /").unwrap().1.is_empty());
        assert_eq!(parse_route("GET /a/").unwrap().1.len(), 2);
    }

    #[test]
    fn queries() {
        let q = parse_query("tag=a+b&tag=c%2Bd&&flag&=empty").unwrap();
        assert_eq!(crate::json::dump(&Value::record(q)), r#"{"tag":["a b","c+d"],"flag":[""],"":["empty"]}"#);
    }
}
