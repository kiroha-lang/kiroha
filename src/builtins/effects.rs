//! Effectful builtins: sleep, env, files, clock, randomness, log, HTTP client
//! and db pool lookup (spec §9.2, §10, §11.1, §13).

use super::args::*;
use super::headers;
use super::pure::{json_fail, utf8_fail};
use super::Builtin;
use crate::host::paths;
use crate::host::{EffOutcome, EffValue, Effect, Host, HttpRequest, RawResponse};
use crate::json;
use crate::runtime::frames::Frame;
use crate::runtime::{AbortReason, BResult, Ctl, DispatchOpts, Fail, Runtime, Task};
use crate::url::{self, Url};
use crate::value::{err, DbHandle, Value};
use std::io::Read;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HttpKind {
    Raw,
    Get,
    Post,
}

pub enum OpFrame {
    Sleep,
    Env,
    Read,
    Write,
    Now,
    Uuid,
    Log,
    Http { kind: HttpKind, head: bool, max_bytes: u64 },
    Pool { alias: String },
}

fn io_error(op: &str, reason: &str, outcome: &str) -> Value {
    err::make(
        "io",
        format!("{op}: {reason}"),
        vec![("operation", Value::str(op)), ("reason", Value::str(reason)), ("outcome", Value::str(outcome))],
    )
}

pub fn network_error(reason: &str, outcome: &str) -> Value {
    err::make(
        "network",
        format!("http: {reason}"),
        vec![("operation", Value::str("http")), ("reason", Value::str(reason)), ("outcome", Value::str(outcome))],
    )
}

impl<H: Host> Runtime<H> {
    pub fn start_effect(&mut self, t: &mut Task, b: Builtin, args: Vec<Value>) -> Ctl {
        match self.prepare_effect(t, b, &args) {
            Ok(Prepared::Dispatch(frame, eff, opts)) => {
                t.stack.push(Frame::Op(Box::new(frame)));
                Ctl::Dispatch(eff, opts)
            }
            Ok(Prepared::Value(v)) => Ctl::Ret(v),
            Err(f) => self.fail_ctl(t, f),
        }
    }

    fn prepare_effect(&mut self, _t: &mut Task, b: Builtin, a: &[Value]) -> Result<Prepared, Fail> {
        let op = b.name();
        let caps = self.config.capabilities.clone();
        let plain = DispatchOpts::default();
        match b {
            Builtin::Sleep => {
                let ms = nonneg_u64(op, 1, &a[0])?;
                Ok(Prepared::Dispatch(OpFrame::Sleep, Effect::Sleep(ms), plain))
            }
            Builtin::Env => {
                let name = string(op, 1, &a[0])?;
                if !caps.env.iter().any(|n| **n == **name) {
                    return Err(Fail::Err(err::permission(op, "env")));
                }
                Ok(Prepared::Dispatch(OpFrame::Env, Effect::Environment(name.to_string()), plain))
            }
            Builtin::Read => {
                let p = string(op, 1, &a[0])?;
                let path = paths::resolve(&self.config.filesystem_root, p)
                    .map_err(|paths::PathError::Invalid(r)| Fail::Err(io_error(op, r, "not_applied")))?;
                if !caps.read.iter().any(|root| paths::within(&path, root)) {
                    return Err(Fail::Err(err::permission(op, "read")));
                }
                let max = self.config.limits.max_file_bytes;
                Ok(Prepared::Dispatch(OpFrame::Read, Effect::ReadRegular { path, max_bytes: max }, plain))
            }
            Builtin::Write => {
                let p = string(op, 1, &a[0])?;
                let body = bytes(op, 2, &a[1])?;
                let path = paths::resolve(&self.config.filesystem_root, p)
                    .map_err(|paths::PathError::Invalid(r)| Fail::Err(io_error(op, r, "not_applied")))?;
                if body.len() as u64 > self.config.limits.max_file_bytes {
                    return Err(Fail::Abort(AbortReason::Resource("max_file_bytes")));
                }
                if !caps.write.iter().any(|root| paths::within(&path, root)) {
                    return Err(Fail::Err(err::permission(op, "write")));
                }
                Ok(Prepared::Dispatch(OpFrame::Write, Effect::ReplaceRegular { path, bytes: body.to_vec() }, plain))
            }
            Builtin::Now => {
                if !caps.clock {
                    return Err(Fail::Err(err::permission(op, "clock")));
                }
                Ok(Prepared::Dispatch(OpFrame::Now, Effect::UtcNow, plain))
            }
            Builtin::Uuid => {
                if !caps.random {
                    return Err(Fail::Err(err::permission(op, "random")));
                }
                Ok(Prepared::Dispatch(OpFrame::Uuid, Effect::RandomBytes(16), plain))
            }
            Builtin::Log => {
                if let Some(k) = a[0].first_non_json() {
                    return Err(Fail::Err(err::type_error(op, err::Pos::Index(1), "json", k)));
                }
                if !caps.log {
                    return Err(Fail::Err(err::permission(op, "log")));
                }
                let mut line = json::dump(&a[0]);
                line.push('\n');
                Ok(Prepared::Dispatch(OpFrame::Log, Effect::LogLine(line), plain))
            }
            Builtin::Db => {
                let name = string(op, 1, &a[0])?;
                if !caps.databases.iter().any(|n| **n == **name) {
                    return Err(Fail::Err(err::permission(op, "databases")));
                }
                if let Some(pool) = self.pools.get(&**name) {
                    return Ok(Prepared::Value(Value::Database(Rc::new(DbHandle {
                        alias: name.to_string(),
                        pool: pool.clone(),
                    }))));
                }
                Ok(Prepared::Dispatch(OpFrame::Pool { alias: name.to_string() }, Effect::Pool(name.to_string()), plain))
            }
            Builtin::Http | Builtin::Get | Builtin::Post => self.prepare_http(b, a),
            _ => unreachable!(),
        }
    }

    fn prepare_http(&mut self, b: Builtin, a: &[Value]) -> Result<Prepared, Fail> {
        let op = b.name();
        let (kind, method, url_v, opts_v, opt_pos) = match b {
            Builtin::Http => {
                let m = string(op, 1, &a[0])?;
                if !headers::is_token(m) || m.bytes().any(|c| c.is_ascii_lowercase()) {
                    return Err(arg_err(op, "invalid method"));
                }
                if &**m == "CONNECT" {
                    return Err(arg_err(op, "CONNECT is not supported"));
                }
                (HttpKind::Raw, m.to_string(), &a[1], a.get(2), 3)
            }
            Builtin::Get => (HttpKind::Get, "GET".to_string(), &a[0], a.get(1), 2),
            _ => (HttpKind::Post, "POST".to_string(), &a[0], a.get(2), 3),
        };
        let upos = if b == Builtin::Http { 2 } else { 1 };
        let u = string(op, upos, url_v)?;
        let mut url = Url::parse(u).map_err(|r| arg_err(op, &format!("invalid url: {r}")))?;
        let known: &[&str] = if kind == HttpKind::Raw {
            &["headers", "query", "body", "timeout", "max_bytes"]
        } else {
            &["headers", "query", "timeout", "max_bytes"]
        };
        let post_body = if kind == HttpKind::Post {
            if let Some(k) = a[1].first_non_json() {
                return Err(Fail::Err(err::type_error(op, err::Pos::Index(2), "json", k)));
            }
            Some(json::dump(&a[1]).into_bytes())
        } else {
            None
        };
        let o = options(op, opt_pos, opts_v, known)?;
        let mut hdrs = match field(o, "headers") {
            Some(h) => headers::outbound(op, h, None)?,
            None => Vec::new(),
        };
        // Header-field rules belong to the `headers` option (validated first).
        if let Some(ae) = headers::get(&hdrs, "accept-encoding") {
            // Empty value / empty list elements are legal (RFC 9110 §12.5.3, §5.6.1).
            let ok = ae.split(',').all(|tok| {
                let coding = tok.split(';').next().unwrap_or("").trim_matches(|c| c == ' ' || c == '\t');
                coding.is_empty() || coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("identity")
            });
            if !ok {
                return Err(arg_err(op, "Accept-Encoding may request only gzip or identity"));
            }
        }
        if kind == HttpKind::Post {
            match headers::get(&hdrs, "content-type") {
                Some(ct) if !headers::is_json_media(ct) => {
                    return Err(arg_err(op, "post Content-Type must be application/json"))
                }
                Some(_) => {}
                None => hdrs.push(("content-type".into(), "application/json".into())),
            }
            if !headers::identity_coding(headers::get(&hdrs, "content-encoding")) {
                return Err(arg_err(op, "post Content-Encoding must be identity"));
            }
        }
        if let Some(q) = field(o, "query") {
            let pairs = url::query_pairs(op, q, None)?;
            url::append_query(&mut url, &pairs);
        }
        let mut body = None;
        if kind == HttpKind::Raw {
            if let Some(bv) = field(o, "body") {
                match bv {
                    Value::Bytes(bb) => body = Some(bb.to_vec()),
                    other => return Err(opt_type_err(op, "body", "bytes", other)),
                }
            }
        }
        if (method == "GET" || method == "HEAD") && body.is_some() {
            return Err(arg_err(op, "GET and HEAD requests cannot carry a body"));
        }
        let timeout = match field(o, "timeout") {
            Some(v) => opt_int(op, "timeout", v, 1)?,
            None => 10000,
        };
        let max_bytes = match field(o, "max_bytes") {
            Some(v) => opt_int(op, "max_bytes", v, 0)?,
            None => 8_388_608,
        };
        reject_unknown(op, o, known)?;
        if kind == HttpKind::Post {
            body = post_body;
        }
        if !self.config.capabilities.http.contains(&url.origin()) {
            return Err(Fail::Err(err::permission(op, "http")));
        }
        if headers::get(&hdrs, "accept-encoding").is_none() {
            hdrs.push(("accept-encoding".into(), "gzip".into()));
        }
        let req = HttpRequest {
            method: method.clone(),
            url: url.to_string_url(),
            headers: hdrs,
            body,
            header_limit: self.config.limits.header_bytes,
            max_bytes,
        };
        Ok(Prepared::Dispatch(
            OpFrame::Http { kind, head: method == "HEAD", max_bytes },
            Effect::Http(req),
            DispatchOpts { cleanup: false, timeout: Some(("http", timeout)) },
        ))
    }

    pub fn op_effect(&mut self, t: &mut Task, op: OpFrame, o: EffOutcome) -> Ctl {
        let v = match o {
            EffOutcome::Ok(v) => v,
            EffOutcome::Err(e) => return self.raise(t, e),
            EffOutcome::Resource(r) => {
                return self.fail_ctl(t, Fail::Abort(AbortReason::Resource(super::callbacks::leak(r))))
            }
            EffOutcome::Joined | EffOutcome::JoinedNotSent => return self.joined_without_abort(t),
        };
        let res: BResult = match (op, v) {
            (OpFrame::Sleep, _) => Ok(Value::Null),
            (OpFrame::Env, EffValue::Str(s)) => Ok(Value::str(&s)),
            (OpFrame::Read, EffValue::Bytes(b)) => {
                if b.len() as u64 > self.config.limits.max_file_bytes {
                    Err(Fail::Abort(AbortReason::Resource("max_file_bytes")))
                } else {
                    Ok(Value::bytes(b))
                }
            }
            (OpFrame::Write, _) => Ok(Value::Null),
            (OpFrame::Log, _) => Ok(Value::Null),
            (OpFrame::Now, EffValue::UnixMs(ms)) => format_utc(ms).map(|s| Value::str(&s)).ok_or_else(|| {
                Fail::Err(io_error("now", "clock_out_of_range", "not_applied"))
            }),
            (OpFrame::Uuid, EffValue::Bytes(b)) => {
                if b.len() != 16 {
                    Err(Fail::Err(io_error("uuid", "random_source", "not_applied")))
                } else {
                    Ok(Value::str(&format_uuid(&b)))
                }
            }
            (OpFrame::Pool { alias }, EffValue::Str(id)) => {
                self.pools.insert(alias.clone(), id.clone());
                Ok(Value::Database(Rc::new(DbHandle { alias, pool: id })))
            }
            (OpFrame::Http { kind, head, max_bytes }, EffValue::Http(resp)) => http_result(kind, head, max_bytes, resp),
            _ => {
                self.harness_failure = Some("effect returned a value of the wrong shape".into());
                return Ctl::Waiting;
            }
        };
        match res {
            Ok(v) => Ctl::Ret(v),
            Err(f) => self.fail_ctl(t, f),
        }
    }
}

pub enum Prepared {
    Dispatch(OpFrame, Effect, DispatchOpts),
    Value(Value),
}

/// Convert a raw response per §10.4–10.5.
pub fn http_result(kind: HttpKind, head: bool, max_bytes: u64, resp: RawResponse) -> BResult {
    if !(200..=599).contains(&resp.status) {
        return Err(Fail::Err(network_error("invalid_status", "unknown")));
    }
    // HTTP/2 pseudo-headers are transport metadata, not header fields.
    let fields: Vec<(String, Vec<u8>)> = resp.headers.iter().filter(|(n, _)| !n.starts_with(':')).cloned().collect();
    if fields.iter().any(|(n, v)| !headers::valid_field(n, v)) {
        return Err(Fail::Err(network_error("framing", "unknown")));
    }
    let hdr_record = headers::incoming(&fields);
    let bodyless = head || matches!(resp.status, 204 | 304);
    let body: Vec<u8> = if bodyless {
        Vec::new()
    } else if resp.status == 205 {
        if !decode_content(&resp, max_bytes)?.is_empty() || !resp.body.is_empty() {
            return Err(Fail::Err(network_error("protocol", "unknown")));
        }
        Vec::new()
    } else {
        decode_content(&resp, max_bytes)?
    };
    match kind {
        HttpKind::Raw => Ok(Value::rec(vec![
            ("status", Value::int(resp.status as i64)),
            ("headers", Value::record(hdr_record)),
            ("body", Value::bytes(body)),
        ])),
        HttpKind::Get | HttpKind::Post => {
            if !(200..=299).contains(&resp.status) {
                return Err(Fail::Err(err::make(
                    "http",
                    format!("unexpected status {}", resp.status),
                    vec![("status", Value::int(resp.status as i64))],
                )));
            }
            let text = std::str::from_utf8(&body).map_err(|e| utf8_fail(e.valid_up_to()))?;
            json::parse(text).map_err(json_fail)
        }
    }
}

fn decode_content(resp: &RawResponse, max_bytes: u64) -> Result<Vec<u8>, Fail> {
    let mut codings: Vec<String> = Vec::new();
    for (n, v) in &resp.headers {
        if n.eq_ignore_ascii_case("content-encoding") {
            for tok in headers::latin1(v).split(',') {
                let tok = tok.trim();
                if !tok.is_empty() {
                    codings.push(tok.to_ascii_lowercase());
                }
            }
        }
    }
    if codings.len() == 1 && codings[0] == "identity" {
        codings.clear();
    }
    let limit_err = || Fail::Err(limit_error(max_bytes));
    match codings.as_slice() {
        [] => {
            if resp.body.len() as u64 > max_bytes {
                return Err(limit_err());
            }
            Ok(resp.body.clone())
        }
        [g] if g == "gzip" => {
            let mut dec = flate2::read::MultiGzDecoder::new(&resp.body[..]);
            let mut out = Vec::new();
            let mut buf = [0u8; 16384];
            loop {
                match dec.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if crate::interrupt::reserve(out.len() + n) || crate::interrupt::poll_big(n) {
                            return Err(Fail::Interrupted);
                        }
                        out.extend_from_slice(&buf[..n]);
                        if out.len() as u64 > max_bytes {
                            return Err(limit_err());
                        }
                    }
                    Err(_) => return Err(Fail::Err(network_error("decode", "unknown"))),
                }
            }
            Ok(out)
        }
        _ => Err(Fail::Err(network_error("unsupported_encoding", "unknown"))),
    }
}

/// The `limit` error for a decoded body over `max_bytes` (§10.4).
pub fn limit_error(max_bytes: u64) -> Value {
    err::make(
        "limit",
        "response body exceeds max_bytes",
        vec![
            ("operation", Value::str("http")),
            ("limit", Value::str("max_bytes")),
            ("maximum", Value::num(crate::decimal::Decimal::from_u64(max_bytes))),
            ("outcome", Value::str("unknown")),
        ],
    )
}

/// `url(base,segments,query={})` (§10.2).
pub fn url_builtin(a: &[Value]) -> BResult {
    let op = "url";
    let base = string(op, 1, &a[0])?;
    let mut u = Url::parse(base).map_err(|r| arg_err(op, &format!("invalid url: {r}")))?;
    let segs = array(op, 2, &a[1])?;
    let mut encoded = Vec::new();
    for s in segs.iter() {
        let s = match s {
            Value::Str(s) => s,
            other => return Err(type_err(op, 2, "array of strings", other)),
        };
        if &**s == "." || &**s == ".." {
            return Err(arg_err(op, "dot segments are forbidden"));
        }
        encoded.push(url::encode_component(s));
    }
    let pairs = match a.get(2) {
        Some(q) => url::query_pairs(op, q, Some(3))?,
        None => Vec::new(),
    };
    if !encoded.is_empty() {
        let mut path = u.path.clone();
        if path.ends_with('/') {
            path.pop();
        }
        for e in encoded {
            path.push('/');
            path.push_str(&e);
        }
        u.path = path;
    }
    url::append_query(&mut u, &pairs);
    Ok(Value::str(&u.to_string_url()))
}

/// Format unix milliseconds as `YYYY-MM-DDTHH:mm:ss.sssZ`.
pub fn format_utc(ms: i128) -> Option<String> {
    let ms = i64::try_from(ms).ok()?;
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    if !(0..=9999).contains(&y) {
        return None;
    }
    let h = rem / 3_600_000;
    let mi = (rem / 60_000) % 60;
    let s = (rem / 1000) % 60;
    let milli = rem % 1000;
    Some(format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{milli:03}Z"))
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn format_uuid(b: &[u8]) -> String {
    let mut x = b.to_vec();
    x[6] = (x[6] & 15) | 64;
    x[8] = (x[8] & 63) | 128;
    format_uuid_raw(&x)
}

/// Hyphenated lowercase hex of 16 bytes, without version/variant changes.
pub fn format_uuid_raw(x: &[u8]) -> String {
    let h: Vec<String> = x.iter().map(|c| format!("{c:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        h[0..4].concat(),
        h[4..6].concat(),
        h[6..8].concat(),
        h[8..10].concat(),
        h[10..16].concat()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc() {
        assert_eq!(format_utc(123).unwrap(), "1970-01-01T00:00:00.123Z");
        assert_eq!(format_utc(951_782_400_000).unwrap(), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn uuid_bits() {
        assert_eq!(format_uuid(&[0u8; 16]), "00000000-0000-4000-8000-000000000000");
    }
}
