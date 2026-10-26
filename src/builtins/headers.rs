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

/// Length in bytes of a quoted-string at the start of `b` (RFC 9110 §5.6.4):
/// `DQUOTE *( qdtext / quoted-pair ) DQUOTE`.
pub fn quoted_string_len(b: &[u8]) -> Option<usize> {
    if b.first() != Some(&b'"') {
        return None;
    }
    let mut k = 1;
    while k < b.len() {
        match b[k] {
            b'"' => return Some(k + 1),
            b'\\' => {
                let c = *b.get(k + 1)?;
                if !(c == b'\t' || c == b' ' || (0x21..=0x7e).contains(&c) || c >= 0x80) {
                    return None;
                }
                k += 2;
            }
            c if c == b'\t' || c == b' ' || c == 0x21 || (0x23..=0x5b).contains(&c) || (0x5d..=0x7e).contains(&c) || c >= 0x80 => {
                k += 1
            }
            _ => return None,
        }
    }
    None
}

/// Unescape a complete quoted-string (quoted-pairs stand for their octet).
fn unquote(b: &[u8]) -> String {
    let mut out = Vec::with_capacity(b.len());
    let mut k = 1;
    while k + 1 < b.len() {
        if b[k] == b'\\' {
            out.push(b[k + 1]);
            k += 2;
        } else {
            out.push(b[k]);
            k += 1;
        }
    }
    out.iter().map(|&c| c as char).collect()
}

/// A parsed media type: lowercase type, lowercase subtype, parameters.
pub type MediaType = (String, String, Vec<(String, String)>);

/// Parse `type "/" subtype *( OWS ";" OWS [ parameter ] )` with
/// `parameter = token "=" ( token / quoted-string )` (RFC 9110 §8.3.1,
/// §5.6.6). Returns lowercase type/subtype and (lowercase name, value).
pub fn parse_media_type(v: &str) -> Option<MediaType> {
    let b = v.as_bytes();
    let ows = |mut k: usize| {
        while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
            k += 1;
        }
        k
    };
    let token = |k: usize| {
        let mut e = k;
        while e < b.len() && is_token_char(b[e]) {
            e += 1;
        }
        e
    };
    let mut k = ows(0);
    let e = token(k);
    if e == k || b.get(e) != Some(&b'/') {
        return None;
    }
    let ty = v[k..e].to_ascii_lowercase();
    k = e + 1;
    let e = token(k);
    if e == k {
        return None;
    }
    let sub = v[k..e].to_ascii_lowercase();
    k = ows(e);
    let mut params = Vec::new();
    while k < b.len() {
        if b[k] != b';' {
            return None;
        }
        k = ows(k + 1);
        if k >= b.len() || b[k] == b';' {
            continue; // empty parameter
        }
        let e = token(k);
        if e == k || b.get(e) != Some(&b'=') {
            return None;
        }
        let name = v[k..e].to_ascii_lowercase();
        k = e + 1;
        let value = if b.get(k) == Some(&b'"') {
            let n = quoted_string_len(&b[k..])?;
            let val = unquote(&b[k..k + n]);
            k += n;
            val
        } else {
            let e = token(k);
            if e == k {
                return None;
            }
            let val = v[k..e].to_string();
            k = e;
            val
        };
        params.push((name, value));
        k = ows(k);
    }
    Some((ty, sub, params))
}

/// application/json with at most one charset=utf-8 parameter (quoted allowed).
pub fn is_json_media(v: &str) -> bool {
    match parse_media_type(v) {
        Some((t, s, params)) if t == "application" && s == "json" => match params.as_slice() {
            [] => true,
            [(name, value)] => name == "charset" && value.eq_ignore_ascii_case("utf-8"),
            _ => false,
        },
        _ => false,
    }
}

/// Syntactically valid media type (RFC 9110 §8.3.1).
pub fn is_media_type(v: &str) -> bool {
    parse_media_type(v).is_some()
}

/// Content-Encoding must be absent or identity.
pub fn identity_coding(v: Option<&str>) -> bool {
    match v {
        None => true,
        // A list of `identity` elements; empty elements are ignored
        // (RFC 9110 §5.6.1.2), so an empty value means no coding.
        Some(s) => s.split(',').map(|t| t.trim_matches(|c| c == ' ' || c == '\t')).filter(|t| !t.is_empty()).all(|t| t.eq_ignore_ascii_case("identity")),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_types() {
        assert!(is_media_type("text/plain"));
        assert!(is_media_type("text/plain; charset=utf-8"));
        assert!(is_media_type("text/plain;charset=\"utf-8; x\""));
        assert!(is_media_type("application/vnd.a+json ; a=b ; c=\"d\""));
        assert!(!is_media_type("text/plain; q"));
        assert!(!is_media_type("text/plain; @@@"));
        assert!(!is_media_type("text/plain; a="));
        assert!(!is_media_type("text/plain; a=\"open"));
        assert!(!is_media_type("text"));
        assert!(is_media_type("text/plain;"));
        assert!(is_media_type("text/plain; ; charset=x;"));
        assert!(is_json_media("application/json;"));
        assert!(is_json_media("application/json; charset=utf-8;"));
        assert!(!is_json_media("application/json; charset=utf-8; a=b"));
        assert!(!is_media_type("text/plain; a=\"x\"\"y\""));
        assert!(is_json_media("application/json; charset=\"utf\\-8\""));
        assert!(is_json_media("Application/JSON ; CHARSET=UTF-8"));
    }
}
