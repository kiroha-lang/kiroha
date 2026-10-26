//! Closed host configuration `kiroha-host/1` (spec §14.1).

use crate::url::{Origin, Url};
use crate::value::{Record, Value};

#[derive(Clone, Debug)]
pub struct Limits {
    pub source_bytes: u64,
    pub memory_bytes: u64,
    pub eval_steps: u64,
    pub max_tasks: u64,
    pub max_file_bytes: u64,
    pub header_bytes: u64,
    pub deadline_ms: Option<crate::decimal::Decimal>,
    pub cleanup_ms: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            source_bytes: 1_048_576,
            memory_bytes: 268_435_456,
            eval_steps: 10_000_000,
            max_tasks: 1024,
            max_file_bytes: 16_777_216,
            header_bytes: 32_768,
            deadline_ms: None,
            cleanup_ms: 1000,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub http: Vec<Origin>,
    pub listen: Vec<(String, u16)>,
    pub databases: Vec<String>,
    pub env: Vec<String>,
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub clock: bool,
    pub random: bool,
    pub log: bool,
}

#[derive(Clone, Debug)]
pub struct PoolConfig {
    pub dsn_env: String,
    pub max_connections: u32,
}

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub filesystem_root: String,
    pub capabilities: Capabilities,
    pub environment: Vec<(String, String)>,
    pub database_pools: Vec<(String, PoolConfig)>,
    pub limits: Limits,
}

impl HostConfig {
    pub fn default_with_root(root: String) -> HostConfig {
        HostConfig {
            filesystem_root: root,
            capabilities: Capabilities::default(),
            environment: Vec::new(),
            database_pools: Vec::new(),
            limits: Limits::default(),
        }
    }

    pub fn env_value(&self, name: &str) -> Option<&str> {
        self.environment.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

type CResult<T> = Result<T, String>;

fn closed(r: &Record, allowed: &[&str], what: &str) -> CResult<()> {
    for k in r.keys() {
        if !allowed.contains(&&**k) {
            return Err(format!("unknown key {k:?} in {what}"));
        }
    }
    Ok(())
}

fn rec<'a>(v: &'a Value, what: &str) -> CResult<&'a Record> {
    v.as_record().ok_or_else(|| format!("{what} must be a record"))
}

fn string_array(v: &Value, what: &str) -> CResult<Vec<String>> {
    let a = v.as_array().ok_or_else(|| format!("{what} must be an array"))?;
    let mut out: Vec<String> = Vec::new();
    for x in a.iter() {
        let s = x.as_str().ok_or_else(|| format!("{what} entries must be strings"))?.to_string();
        if out.contains(&s) {
            return Err(format!("duplicate entry {s:?} in {what}"));
        }
        out.push(s);
    }
    Ok(out)
}

fn boolean(v: &Value, what: &str) -> CResult<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(format!("{what} must be a boolean")),
    }
}

fn int_in(v: &Value, what: &str, min: u64, max: u64) -> CResult<u64> {
    let d = v.as_num().ok_or_else(|| format!("{what} must be an integer"))?;
    if !d.is_integer() || d.is_negative() {
        return Err(format!("{what} must be a nonnegative integer"));
    }
    let i = d.to_u64().unwrap_or(u64::MAX);
    if i < min || i > max {
        return Err(format!("{what} out of range"));
    }
    Ok(i)
}

fn absolute_dir(s: &str, what: &str) -> CResult<String> {
    if !s.starts_with('/') || s.contains('\0') || s.split('/').any(|c| c == "..") {
        return Err(format!("{what} entries must be absolute paths without `..`"));
    }
    Ok(crate::host::paths::normalize_components(s))
}

/// Directory roots, normalized by components; duplicates after
/// normalization (e.g. `/a` and `/a/`) are configuration errors.
fn dir_list(v: &Value, what: &str) -> CResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for s in string_array(v, what)? {
        let d = absolute_dir(&s, what)?;
        if out.contains(&d) {
            return Err(format!("duplicate entry {s:?} in {what}"));
        }
        out.push(d);
    }
    Ok(out)
}

pub fn parse_origin(s: &str) -> CResult<Origin> {
    let u = Url::parse(s).map_err(|e| format!("invalid http origin {s:?}: {e}"))?;
    let rest = &s[s.find("://").unwrap() + 3..];
    if rest.contains('/') || rest.contains('?') {
        return Err(format!("http origin {s:?} must not contain a path or query"));
    }
    Ok(u.origin())
}

/// Parse the capabilities/environment/limits fields shared by the CLI host
/// file and conformance case `host` objects.
fn parse_common(cfg: &mut HostConfig, r: &Record) -> CResult<()> {
    if let Some(c) = r.get("capabilities") {
        let c = rec(c, "capabilities")?;
        closed(c, &["http", "listen", "databases", "env", "read", "write", "clock", "random", "log"], "capabilities")?;
        let caps = &mut cfg.capabilities;
        if let Some(v) = c.get("http") {
            let mut origins: Vec<Origin> = Vec::new();
            for s in string_array(v, "capabilities.http")? {
                let o = parse_origin(&s)?;
                if origins.contains(&o) {
                    return Err(format!("duplicate http origin {s:?}"));
                }
                origins.push(o);
            }
            caps.http = origins;
        }
        if let Some(v) = c.get("listen") {
            let a = v.as_array().ok_or("capabilities.listen must be an array")?;
            for e in a.iter() {
                let er = rec(e, "listen entry")?;
                closed(er, &["host", "port"], "listen entry")?;
                let host = er.get("host").and_then(|h| h.as_str()).ok_or("listen entry needs a string host")?;
                let port = int_in(er.get("port").ok_or("listen entry needs a port")?, "listen port", 1, 65535)?;
                let entry = (host.to_string(), port as u16);
                if caps.listen.contains(&entry) {
                    return Err("duplicate listen entry".into());
                }
                caps.listen.push(entry);
            }
        }
        if let Some(v) = c.get("databases") {
            caps.databases = string_array(v, "capabilities.databases")?;
        }
        if let Some(v) = c.get("env") {
            caps.env = string_array(v, "capabilities.env")?;
        }
        if let Some(v) = c.get("read") {
            caps.read = dir_list(v, "capabilities.read")?;
        }
        if let Some(v) = c.get("write") {
            caps.write = dir_list(v, "capabilities.write")?;
        }
        if let Some(v) = c.get("clock") {
            caps.clock = boolean(v, "capabilities.clock")?;
        }
        if let Some(v) = c.get("random") {
            caps.random = boolean(v, "capabilities.random")?;
        }
        if let Some(v) = c.get("log") {
            caps.log = boolean(v, "capabilities.log")?;
        }
    }
    if let Some(e) = r.get("environment") {
        let e = rec(e, "environment")?;
        for (k, v) in e.iter() {
            let s = v.as_str().ok_or("environment values must be strings")?;
            cfg.environment.push((k.to_string(), s.to_string()));
        }
    }
    if let Some(l) = r.get("limits") {
        let l = rec(l, "limits")?;
        closed(
            l,
            &["source_bytes", "memory_bytes", "eval_steps", "max_tasks", "max_file_bytes", "header_bytes", "deadline_ms", "cleanup_ms"],
            "limits",
        )?;
        let lim = &mut cfg.limits;
        let big = u64::MAX;
        if let Some(v) = l.get("source_bytes") {
            lim.source_bytes = int_in(v, "source_bytes", 1, big)?;
        }
        if let Some(v) = l.get("memory_bytes") {
            lim.memory_bytes = int_in(v, "memory_bytes", 1, big)?;
        }
        if let Some(v) = l.get("eval_steps") {
            lim.eval_steps = int_in(v, "eval_steps", 1, big)?;
        }
        if let Some(v) = l.get("max_tasks") {
            lim.max_tasks = int_in(v, "max_tasks", 1, big)?;
        }
        if let Some(v) = l.get("max_file_bytes") {
            lim.max_file_bytes = int_in(v, "max_file_bytes", 0, big)?;
        }
        if let Some(v) = l.get("header_bytes") {
            lim.header_bytes = int_in(v, "header_bytes", 1, big)?;
        }
        if let Some(v) = l.get("deadline_ms") {
            lim.deadline_ms = match v {
                Value::Null => None,
                other => {
                    int_in(other, "deadline_ms", 1, big)?;
                    Some(other.as_num().unwrap().clone())
                }
            };
        }
        if let Some(v) = l.get("cleanup_ms") {
            lim.cleanup_ms = int_in(v, "cleanup_ms", 1, 10_000)?;
        }
    }
    Ok(())
}

/// Parse a complete CLI host file value.
pub fn parse_host_file(v: &Value, launch_dir: &str) -> CResult<HostConfig> {
    let r = rec(v, "host configuration")?;
    closed(r, &["format", "filesystem_root", "capabilities", "environment", "database_pools", "limits"], "host configuration")?;
    match r.get("format") {
        Some(Value::Str(s)) if &**s == "kiroha-host/1" => {}
        _ => return Err("format must be exactly \"kiroha-host/1\"".into()),
    }
    let mut cfg = HostConfig::default_with_root(launch_dir.to_string());
    if let Some(root) = r.get("filesystem_root") {
        let s = root.as_str().ok_or("filesystem_root must be a string")?;
        cfg.filesystem_root = absolute_dir(s, "filesystem_root")?;
    }
    parse_common(&mut cfg, r)?;
    if let Some(p) = r.get("database_pools") {
        let p = rec(p, "database_pools")?;
        for (alias, pv) in p.iter() {
            let pr = rec(pv, "database pool")?;
            closed(pr, &["dsn_env", "max_connections"], "database pool")?;
            let dsn_env = pr.get("dsn_env").and_then(|d| d.as_str()).ok_or("database pool needs a string dsn_env")?;
            let max_connections = match pr.get("max_connections") {
                Some(v) => int_in(v, "max_connections", 1, u32::MAX as u64)? as u32,
                None => 4,
            };
            cfg.database_pools.push((alias.to_string(), PoolConfig { dsn_env: dsn_env.to_string(), max_connections }));
        }
    }
    Ok(cfg)
}

/// Parse a conformance case's partial host object.
pub fn parse_case_host(v: &Value) -> CResult<HostConfig> {
    let r = rec(v, "case host")?;
    closed(r, &["capabilities", "environment", "limits"], "case host")?;
    let mut cfg = HostConfig::default_with_root("/".to_string());
    parse_common(&mut cfg, r)?;
    Ok(cfg)
}
