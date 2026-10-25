//! Pure standard data functions (spec §7) and builtin dispatch.

use super::args::*;
use super::Builtin;
use crate::decimal::Decimal;
use crate::host::Host;
use crate::json::{self, JsonError};
use crate::runtime::{AbortReason, BResult, Ctl, Fail as RFail, Runtime, Task};
use crate::schema;
use crate::value::{err, json_equal, Record, Value};
use num_bigint::BigInt;
use num_traits::Zero;
use std::rc::Rc;

impl<H: Host> Runtime<H> {
    /// Enter a builtin after the generic call protocol checks.
    pub fn call_builtin(&mut self, t: &mut Task, b: Builtin, args: Vec<Value>, cb: Option<Value>) -> Ctl {
        use Builtin::*;
        let res = match b {
            Map | Filter | Each | Fold | Sort | Group | Unique => return self.start_collect(t, b, args, cb),
            Parallel => return self.start_parallel(t, args, cb.unwrap()),
            Retry => return self.start_retry(t, args, cb.unwrap()),
            Pages => return self.start_pages(t, args, cb.unwrap()),
            Sleep | Env | Read | Write | Now | Uuid | Log | Http | Get | Post | Db => {
                return self.start_effect(t, b, args)
            }
            Sql => return self.start_sql(t, args),
            Tx => return self.start_tx(t, args, cb.unwrap()),
            Serve => return self.start_serve(t, args),
            Input => crate::builtins::server::input(&args),
            Reply => crate::builtins::server::reply(&args),
            Url => crate::builtins::effects::url_builtin(&args),
            _ => {
                pure(b, &args, &mut |_n: usize| {
                    if crate::interrupt::poll() {
                        Err(RFail::Interrupted)
                    } else {
                        Ok(())
                    }
                })
            }
        };
        match res {
            Ok(v) => Ctl::Ret(v),
            Err(f) => self.fail_ctl(t, f),
        }
    }
}

type Tick<'a> = &'a mut dyn FnMut(usize) -> Result<(), RFail>;

pub fn json_fail(e: JsonError) -> RFail {
    match e {
        JsonError::Invalid { reason, offset } => RFail::Err(err::make(
            "json",
            format!("invalid JSON: {reason} at byte {offset}"),
            vec![("reason", Value::str(reason)), ("offset", Value::int(offset as i64))],
        )),
        JsonError::Overflow => RFail::Abort(AbortReason::Resource("number")),
        JsonError::Interrupted => RFail::Interrupted,
    }
}

pub fn utf8_fail(offset: usize) -> RFail {
    RFail::Err(err::make("utf8", format!("invalid UTF-8 at byte {offset}"), vec![("offset", Value::int(offset as i64))]))
}

fn num_overflow() -> RFail {
    RFail::Abort(AbortReason::Resource("number"))
}

/// Canonical text of a scalar (§7.1 text).
pub fn text_of(op: &str, v: &Value) -> VResult<String> {
    Ok(match v {
        Value::Str(s) => s.to_string(),
        Value::Num(d) => d.canonical(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        other => return Err(type_err(op, 1, "string, number, bool or null", other)),
    })
}

fn pure(b: Builtin, a: &[Value], tick: Tick) -> BResult {
    use Builtin::*;
    let op = b.name();
    match b {
        Len => {
            let n = match &a[0] {
                Value::Array(x) => x.len(),
                Value::Record(r) => r.len(),
                Value::Str(s) => s.chars().count(),
                Value::Bytes(x) => x.len(),
                other => return Err(type_err(op, 1, "array, record, string or bytes", other)),
            };
            Ok(Value::int(n as i64))
        }
        Range => {
            let start = integer(op, 1, &a[0])?;
            let stop = integer(op, 2, &a[1])?;
            let step = match a.get(2) {
                Some(v) => integer(op, 3, v)?,
                None => BigInt::from(1),
            };
            if step.is_zero() {
                return Err(arg_err(op, "step must be nonzero"));
            }
            let mut out = Vec::new();
            let mut x = start;
            let positive = step > BigInt::zero();
            while (positive && x < stop) || (!positive && x > stop) {
                out.push(Value::num(Decimal::from_bigint(x.clone())));
                tick(out.len())?;
                x += &step;
            }
            Ok(Value::array(out))
        }
        Flat => {
            let xs = array(op, 1, &a[0])?;
            for x in xs.iter() {
                if !matches!(x, Value::Array(_)) {
                    return Err(type_err(op, 1, "array of arrays", x));
                }
            }
            let mut out = Vec::new();
            for x in xs.iter() {
                for item in x.as_array().unwrap().iter() {
                    out.push(item.clone());
                    tick(out.len())?;
                }
            }
            Ok(Value::array(out))
        }
        Take => {
            let xs = array(op, 1, &a[0])?;
            let n = nonneg_u64(op, 2, &a[1])?;
            let k = (n as u128).min(xs.len() as u128) as usize;
            Ok(Value::array(xs[..k].to_vec()))
        }
        Sum => {
            let xs = array(op, 1, &a[0])?;
            for x in xs.iter() {
                if !matches!(x, Value::Num(_)) {
                    return Err(type_err(op, 1, "array of numbers", x));
                }
            }
            let mut acc = Decimal::zero();
            for (i, x) in xs.iter().enumerate() {
                acc = acc.add(x.as_num().unwrap()).map_err(|_| num_overflow())?;
                tick(i + 1)?;
            }
            Ok(Value::num(acc))
        }
        Keys => {
            let r = record(op, 1, &a[0])?;
            Ok(Value::array(r.keys().map(|k| Value::Str(k.clone())).collect()))
        }
        Has => {
            let r = record(op, 1, &a[0])?;
            let k = string(op, 2, &a[1])?;
            Ok(Value::Bool(r.contains(k)))
        }
        Pick => {
            let r = record(op, 1, &a[0])?;
            let ks = array(op, 2, &a[1])?;
            let mut seen: Vec<&str> = Vec::new();
            for k in ks.iter() {
                let s = match k {
                    Value::Str(s) => s,
                    other => return Err(type_err(op, 2, "array of strings", other)),
                };
                if seen.contains(&&**s) {
                    return Err(arg_err(op, "duplicate key"));
                }
                seen.push(s);
            }
            let mut out = Record::with_capacity(ks.len());
            for k in seen {
                match r.get(k) {
                    Some(v) => out.push(k, v.clone()),
                    None => {
                        return Err(RFail::Err(err::make(
                            "missing",
                            format!("missing key {k:?}"),
                            vec![("key", Value::str(k))],
                        )))
                    }
                }
            }
            Ok(Value::record(out))
        }
        Merge => {
            let x = record(op, 1, &a[0])?;
            let y = record(op, 2, &a[1])?;
            let mut out = (**x).clone();
            for (k, v) in y.iter() {
                out.insert(k.clone(), v.clone());
            }
            Ok(Value::record(out))
        }
        One => {
            let xs = array(op, 1, &a[0])?;
            if xs.len() != 1 {
                return Err(RFail::Err(err::make(
                    "cardinality",
                    format!("expected exactly one item, got {}", xs.len()),
                    vec![("expected", Value::int(1)), ("actual", Value::int(xs.len() as i64))],
                )));
            }
            Ok(xs[0].clone())
        }
        Split => {
            let s = string(op, 1, &a[0])?;
            let sep = string(op, 2, &a[1])?;
            if sep.is_empty() {
                return Err(arg_err(op, "separator must be nonempty"));
            }
            let mut parts = Vec::new();
            for (i, p) in s.split(&**sep).enumerate() {
                parts.push(Value::str(p));
                tick(i)?;
            }
            Ok(Value::array(parts))
        }
        Join => {
            let xs = array(op, 1, &a[0])?;
            for x in xs.iter() {
                if !matches!(x, Value::Str(_)) {
                    return Err(type_err(op, 1, "array of strings", x));
                }
            }
            let sep = string(op, 2, &a[1])?;
            let parts: Vec<&str> = xs.iter().map(|x| x.as_str().unwrap()).collect();
            let total: usize = parts.iter().map(|p| p.len()).sum::<usize>() + sep.len() * parts.len();
            if crate::interrupt::reserve(total) {
                return Err(RFail::Interrupted);
            }
            Ok(Value::str(&parts.join(sep)))
        }
        Replace => {
            let s = string(op, 1, &a[0])?;
            let old = string(op, 2, &a[1])?;
            if old.is_empty() {
                return Err(arg_err(op, "search text must be nonempty"));
            }
            let new = string(op, 3, &a[2])?;
            let mut out = String::with_capacity(s.len());
            let mut last = 0;
            for (i, (pos, _)) in s.match_indices(&**old).enumerate() {
                if crate::interrupt::reserve(out.len() + new.len() + (pos - last)) {
                    return Err(RFail::Interrupted);
                }
                out.push_str(&s[last..pos]);
                out.push_str(new);
                last = pos + old.len();
                tick(i)?;
            }
            out.push_str(&s[last..]);
            Ok(Value::str(&out))
        }
        Text => Ok(Value::str(&text_of(op, &a[0])?)),
        Number => {
            let s = string(op, 1, &a[0])?;
            match Decimal::parse_json(s) {
                Ok(Some(d)) => Ok(Value::num(d)),
                Ok(None) => Err(RFail::Err(err::number("invalid_syntax"))),
                Err(_) => Err(num_overflow()),
            }
        }
        Parse => {
            let s = string(op, 1, &a[0])?;
            json::parse(s).map_err(json_fail)
        }
        Dump => {
            if let Some(k) = a[0].first_non_json() {
                return Err(RFail::Err(err::type_error(op, err::Pos::Index(1), "json", k)));
            }
            Ok(Value::str(&json::dump(&a[0])))
        }
        Encode => {
            let s = string(op, 1, &a[0])?;
            if crate::interrupt::reserve(s.len()) {
                return Err(RFail::Interrupted);
            }
            Ok(Value::bytes(s.as_bytes().to_vec()))
        }
        Decode => {
            let b = bytes(op, 1, &a[0])?;
            match std::str::from_utf8(b) {
                Ok(s) => Ok(Value::str(s)),
                Err(e) => Err(utf8_fail(e.valid_up_to())),
            }
        }
        Check => {
            let s = schema::normalize(&a[1])?;
            match schema::validate(&a[0], &s) {
                Ok(()) => Ok(a[0].clone()),
                Err(m) => Err(RFail::Err(m.to_error())),
            }
        }
        Maybe => Ok(schema::make_maybe(&a[0])?),
        Optional => Ok(schema::make_optional(&a[0])?),
        Open => Ok(schema::make_open(&a[0])?),
        Builtin::Fail => Err(RFail::Err(validate_error_record(op, &a[0])?)),
        _ => unreachable!("{op} is not a pure builtin"),
    }
}

/// `fail(err)` shape validation (§8.1).
pub fn validate_error_record(op: &str, v: &Value) -> VResult<Value> {
    let r = record(op, 1, v)?;
    for k in r.keys() {
        if !matches!(&**k, "code" | "message" | "data") {
            return Err(arg_err(op, &format!("unexpected error field {k:?}")));
        }
    }
    let code = r.get("code").ok_or_else(|| arg_err(op, "missing code"))?;
    let message = r.get("message").ok_or_else(|| arg_err(op, "missing message"))?;
    let data = r.get("data").ok_or_else(|| arg_err(op, "missing data"))?;
    match code {
        Value::Str(s) if s.is_empty() => return Err(arg_err(op, "code must be nonempty")),
        Value::Str(_) => {}
        other => return Err(opt_type_err(op, "code", "string", other)),
    }
    if !matches!(message, Value::Str(_)) {
        return Err(opt_type_err(op, "message", "string", message));
    }
    match data {
        Value::Record(_) => {
            if let Some(k) = data.first_non_json() {
                return Err(RFail::Err(err::type_error(op, err::Pos::Option("data"), "json", k)));
            }
        }
        other => return Err(opt_type_err(op, "data", "record", other)),
    }
    Ok(v.clone())
}

/// JSON-equality lookup helper for group/unique.
pub struct JsonIndex {
    buckets: std::collections::HashMap<u64, Vec<usize>>,
}

impl JsonIndex {
    pub fn new() -> JsonIndex {
        JsonIndex { buckets: std::collections::HashMap::new() }
    }

    /// Find an existing key position among `keys`, or insert `pos`.
    pub fn find_or_insert(&mut self, keys: &[Value], key: &Value, pos: usize) -> Option<usize> {
        let h = crate::value::json_hash(key);
        let bucket = self.buckets.entry(h).or_default();
        for &i in bucket.iter() {
            if json_equal(&keys[i], key) {
                return Some(i);
            }
        }
        bucket.push(pos);
        None
    }
}

impl Default for JsonIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
fn rc_str(s: &str) -> Rc<str> {
    Rc::from(s)
}
