//! HTTP header and media-type rules (spec §10.3, §10.5, §12.3).

use super::args::{arg_err, opt_type_err, VResult};
use crate::value::Value;

pub fn is_token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)
}

pub fn is_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(is_token_char)
}

/// HTTP method accepted by Web1: uppercase token, not CONNECT.
pub fn valid_method(m: &str) -> bool {
    is_token(m) && !m.bytes().any(|c| c.is_ascii_lowercase()) && m != "CONNECT"
}

const RUNTIME_OWNED: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "proxy-connection",
    "upgrade",
    "te",
    "trailer",
    "expect",
];

/// Validate an outbound header record; returns lowercase-name/trimmed-value
/// pairs preserving the original name spelling for sending.
/// `arg_index`: Some(i) for a positional headers argument (`reply`), None
/// for the `headers` option of http/get/post.
pub fn outbound(op: &str, v: &Value, arg_index: Option<usize>) -> VResult<Vec<(String, String)>> {
    let bad = |expected: &str, got: &Value| match arg_index {
        Some(i) => super::args::type_err(op, i, expected, got),
        None => opt_type_err(op, "headers", expected, got),
    };
    let r = match v {
        Value::Record(r) => r,
        other => return Err(bad("record", other)),
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for (name, value) in r.iter() {
        let val = match value {
            Value::Str(s) => s,
            other => return Err(bad("string", other)),
        };
        if !is_token(name) {
            return Err(arg_err(op, "invalid header name"));
        }
        let lower = name.to_ascii_lowercase();
        if out.iter().any(|(n, _)| *n == lower) {
            return Err(arg_err(op, "duplicate header name"));
        }
        if RUNTIME_OWNED.contains(&lower.as_str()) {
            return Err(arg_err(op, "header is owned by the runtime"));
        }
        if !val.bytes().all(|c| c == b'\t' || c == b' ' || (0x21..=0x7e).contains(&c)) {
            return Err(arg_err(op, "invalid header value"));
        }
        out.push((lower, val.trim_matches(|c| c == ' ' || c == '\t').to_string()));
    }
    Ok(out)
}

pub fn get<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// application/json with at most one charset=utf-8 parameter (quoted allowed).
pub fn is_json_media(v: &str) -> bool {
    let mut parts = v.split(';');
    let mt = parts.next().unwrap_or("").trim_matches(|c| c == ' ' || c == '\t');
    if !mt.eq_ignore_ascii_case("application/json") {
        return false;
    }
    let params: Vec<&str> = parts.collect();
    match params.len() {
        0 => true,
        1 => {
            let p = params[0].trim_matches(|c| c == ' ' || c == '\t');
            let (k, val) = match p.split_once('=') {
                Some(x) => x,
                None => return false,
            };
            let k = k.trim_matches(|c| c == ' ' || c == '\t');
            let val = val.trim_matches(|c| c == ' ' || c == '\t');
            let val = if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') { &val[1..val.len() - 1] } else { val };
            k.eq_ignore_ascii_case("charset") && val.eq_ignore_ascii_case("utf-8")
        }
        _ => false,
    }
}

/// Syntactically valid `type/subtype *(;param)` media type.
pub fn is_media_type(v: &str) -> bool {
    let mut parts = v.split(';');
    let mt = parts.next().unwrap_or("").trim();
    match mt.split_once('/') {
        Some((a, b)) => is_token(a) && is_token(b),
        None => false,
    }
}

/// Content-Encoding must be absent or identity.
pub fn identity_coding(v: Option<&str>) -> bool {
    match v {
        None => true,
        Some(s) => s.trim().eq_ignore_ascii_case("identity"),
    }
}

/// Map raw HTTP/1 field-value octets to a string, octet-for-octet (Latin-1).
pub fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// A received field: token name, value without controls other than HTAB
/// (obs-text 0x80..0xFF allowed). Invalid fields are transport failures.
pub fn valid_field(name: &str, value: &[u8]) -> bool {
    is_token(name) && value.iter().all(|&c| c == b'\t' || (0x20..0x7f).contains(&c) || c >= 0x80)
}

/// Normalize incoming headers: lowercase names, OWS-trimmed values, record
/// keyed by first occurrence with values in wire order.
pub fn incoming(raw: &[(String, Vec<u8>)]) -> crate::value::Record {
    let mut order: Vec<(String, Vec<Value>)> = Vec::new();
    for (n, v) in raw {
        let name = n.to_ascii_lowercase();
        let s = latin1(v);
        let s = s.trim_matches(|c| c == ' ' || c == '\t').to_string();
        match order.iter_mut().find(|(k, _)| *k == name) {
            Some((_, vals)) => vals.push(Value::str(&s)),
            None => order.push((name, vec![Value::str(&s)])),
        }
    }
    let mut r = crate::value::Record::with_capacity(order.len());
    for (k, vals) in order {
        r.push(&k, Value::array(vals));
    }
    r
}
