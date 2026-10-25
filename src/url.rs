//! Accepted URL form and the `url` constructor (spec §10.2).

use crate::value::{err, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    /// Lowercased DNS name, dotted IPv4, or bracketed IPv6.
    pub host: String,
    /// Port text exactly as supplied (validated), if any.
    pub port: Option<String>,
    /// Raw path, `/` when absent.
    pub path: String,
    /// Raw query text after `?` (may be empty) if a `?` was present.
    pub query: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

fn bad(reason: &str) -> String {
    reason.to_string()
}

fn valid_percent(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if !(b.get(i + 1).is_some_and(|c| c.is_ascii_hexdigit()) && b.get(i + 2).is_some_and(|c| c.is_ascii_hexdigit())) {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    true
}

fn strict_ipv4(h: &str) -> bool {
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| {
        !p.is_empty()
            && p.len() <= 3
            && p.bytes().all(|c| c.is_ascii_digit())
            && !(p.len() > 1 && p.starts_with('0'))
            && p.parse::<u16>().map(|v| v <= 255).unwrap_or(false)
    })
}

fn looks_numeric_label(l: &str) -> bool {
    !l.is_empty()
        && (l.bytes().all(|c| c.is_ascii_digit())
            || ((l.starts_with("0x") || l.starts_with("0X")) && l[2..].bytes().all(|c| c.is_ascii_hexdigit())))
}

fn parse_host(h: &str) -> Result<String, String> {
    if h.is_empty() {
        return Err(bad("empty_host"));
    }
    if h.starts_with('[') {
        if !h.ends_with(']') || h.contains('%') {
            return Err(bad("invalid_host"));
        }
        let inner = &h[1..h.len() - 1];
        if inner.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(bad("invalid_host"));
        }
        return Ok(h.to_ascii_lowercase());
    }
    let lower = h.to_ascii_lowercase();
    let trimmed = lower.strip_suffix('.').unwrap_or(&lower);
    let labels: Vec<&str> = trimmed.split('.').collect();
    if labels.iter().any(|l| l.is_empty()) {
        return Err(bad("invalid_host"));
    }
    if !lower.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.' || c == b'_') {
        return Err(bad("invalid_host"));
    }
    if looks_numeric_label(labels.last().unwrap()) && !strict_ipv4(&lower) {
        return Err(bad("invalid_host"));
    }
    Ok(lower)
}

impl Url {
    /// Parse and validate an input URL. Error is a short reason string.
    pub fn parse(s: &str) -> Result<Url, String> {
        if !s.is_ascii() {
            return Err(bad("non_ascii"));
        }
        if s.bytes().any(|c| c <= 0x20 || c == 0x7f || c == b'\\') {
            return Err(bad("invalid_character"));
        }
        if s.contains('#') {
            return Err(bad("fragment"));
        }
        let colon = s.find("://").ok_or_else(|| bad("not_absolute"))?;
        let scheme = s[..colon].to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(bad("scheme"));
        }
        let rest = &s[colon + 3..];
        let auth_end = rest.find(['/', '?']).unwrap_or(rest.len());
        let authority = &rest[..auth_end];
        let after = &rest[auth_end..];
        if authority.is_empty() {
            return Err(bad("empty_authority"));
        }
        if authority.contains('@') {
            return Err(bad("userinfo"));
        }
        let (host_part, port) = if authority.starts_with('[') {
            let close = authority.find(']').ok_or_else(|| bad("invalid_host"))?;
            let h = &authority[..=close];
            let r = &authority[close + 1..];
            if r.is_empty() {
                (h, None)
            } else if let Some(p) = r.strip_prefix(':') {
                (h, Some(p))
            } else {
                return Err(bad("invalid_host"));
            }
        } else {
            match authority.rfind(':') {
                Some(i) => (&authority[..i], Some(&authority[i + 1..])),
                None => (authority, None),
            }
        };
        let host = parse_host(host_part)?;
        let port = match port {
            None => None,
            Some(p) => {
                if p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(bad("invalid_port"));
                }
                let v: u64 = p.parse().unwrap_or(u64::MAX);
                if !(1..=65535).contains(&v) {
                    return Err(bad("invalid_port"));
                }
                Some(p.to_string())
            }
        };
        let (path, query) = match after.find('?') {
            Some(q) => (&after[..q], Some(after[q + 1..].to_string())),
            None => (after, None),
        };
        if !valid_percent(path) || !valid_percent(query.as_deref().unwrap_or("")) {
            return Err(bad("invalid_percent_escape"));
        }
        let path = if path.is_empty() { "/".to_string() } else { path.to_string() };
        Ok(Url { scheme, host, port, path, query })
    }

    pub fn port_number(&self) -> u16 {
        match &self.port {
            Some(p) => p.parse().unwrap(),
            None => {
                if self.scheme == "https" {
                    443
                } else {
                    80
                }
            }
        }
    }

    pub fn origin(&self) -> Origin {
        Origin { scheme: self.scheme.clone(), host: self.host.clone(), port: self.port_number() }
    }

    /// Host name without IPv6 brackets.
    pub fn host_bare(&self) -> &str {
        self.host.trim_start_matches('[').trim_end_matches(']')
    }

    pub fn authority(&self) -> String {
        match &self.port {
            Some(p) => format!("{}:{}", self.host, p),
            None => self.host.clone(),
        }
    }

    /// Origin-form request target.
    pub fn target(&self) -> String {
        match &self.query {
            Some(q) => format!("{}?{}", self.path, q),
            None => self.path.clone(),
        }
    }

    pub fn to_string_url(&self) -> String {
        format!("{}://{}{}", self.scheme, self.authority(), self.target())
    }
}

/// Unreserved-only percent encoding with uppercase hex.
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Append query pairs from a Kiri record per §10.2. Returns type errors.
pub fn query_pairs(op: &str, q: &Value, position: err::Pos) -> Result<Vec<(String, String)>, Value> {
    let r = match q {
        Value::Record(r) => r,
        other => return Err(err::type_error(op, position, "record", other.kind())),
    };
    let mut pairs = Vec::new();
    let scalar = |v: &Value| -> Result<Option<String>, Value> {
        Ok(match v {
            Value::Null => None,
            Value::Str(s) => Some(s.to_string()),
            Value::Num(d) => Some(d.canonical()),
            Value::Bool(b) => Some(b.to_string()),
            other => {
                return Err(err::type_error(op, err::Pos::Option("query"), "string, number, bool or null", other.kind()))
            }
        })
    };
    for (k, v) in r.iter() {
        match v {
            Value::Array(items) => {
                for it in items.iter() {
                    if let Some(s) = scalar(it)? {
                        pairs.push((k.to_string(), s));
                    }
                }
            }
            other => {
                if let Some(s) = scalar(other)? {
                    pairs.push((k.to_string(), s));
                }
            }
        }
    }
    Ok(pairs)
}

/// Apply encoded query pairs to an existing query.
pub fn append_query(url: &mut Url, pairs: &[(String, String)]) {
    if pairs.is_empty() {
        return;
    }
    let encoded: Vec<String> =
        pairs.iter().map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v))).collect();
    let joined = encoded.join("&");
    url.query = Some(match &url.query {
        Some(existing) if !existing.is_empty() => format!("{existing}&{joined}"),
        _ => joined,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rules() {
        let u = Url::parse("HTTPS://EXAMPLE.TEST/a//?old=%2f").unwrap();
        assert_eq!(u.to_string_url(), "https://example.test/a//?old=%2f");
        assert_eq!(Url::parse("https://example.test").unwrap().path, "/");
        assert!(Url::parse("https://user@example.test").is_err());
        assert!(Url::parse("https://example.test/#frag").is_err());
        assert!(Url::parse("https://example.test:/").is_err());
        assert!(Url::parse("https://example.test:70000/").is_err());
        assert!(Url::parse("http://0x7f.1/").is_err());
        assert!(Url::parse("http://127.1/").is_err());
        assert!(Url::parse("http://127.0.0.1:8080/x").is_ok());
        assert!(Url::parse("http://[::1]:80/").is_ok());
        assert!(Url::parse("http://a/%zz").is_err());
        assert!(Url::parse("http://a/b c").is_err());
        assert!(Url::parse("ftp://a/").is_err());
    }

    #[test]
    fn encoding() {
        assert_eq!(encode_component("a/b"), "a%2Fb");
        assert_eq!(encode_component("x y+%"), "x%20y%2B%25");
        assert_eq!(encode_component("é"), "%C3%A9");
    }
}
