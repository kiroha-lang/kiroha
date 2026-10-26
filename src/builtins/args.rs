//! Shared argument validation helpers (spec §7.2).

use crate::decimal::Decimal;
use crate::runtime::{AbortReason, Fail};
use crate::value::{err, Record, Value};
use num_bigint::BigInt;
use std::rc::Rc;

pub type VResult<T> = Result<T, Fail>;

pub fn type_err(op: &str, pos: usize, expected: &str, v: &Value) -> Fail {
    Fail::Err(err::type_error(op, err::Pos::Index(pos), expected, v.kind()))
}

pub fn opt_type_err(op: &str, field: &str, expected: &str, v: &Value) -> Fail {
    Fail::Err(err::type_error(op, err::Pos::Option(field), expected, v.kind()))
}

pub fn arg_err(op: &str, reason: &str) -> Fail {
    Fail::Err(err::argument(op, reason))
}

pub fn array<'a>(op: &str, pos: usize, v: &'a Value) -> VResult<&'a Rc<Vec<Value>>> {
    match v {
        Value::Array(a) => Ok(a),
        _ => Err(type_err(op, pos, "array", v)),
    }
}

pub fn record<'a>(op: &str, pos: usize, v: &'a Value) -> VResult<&'a Rc<Record>> {
    match v {
        Value::Record(r) => Ok(r),
        _ => Err(type_err(op, pos, "record", v)),
    }
}

pub fn string<'a>(op: &str, pos: usize, v: &'a Value) -> VResult<&'a Rc<str>> {
    match v {
        Value::Str(s) => Ok(s),
        _ => Err(type_err(op, pos, "string", v)),
    }
}

pub fn bytes<'a>(op: &str, pos: usize, v: &'a Value) -> VResult<&'a Rc<Vec<u8>>> {
    match v {
        Value::Bytes(b) => Ok(b),
        _ => Err(type_err(op, pos, "bytes", v)),
    }
}

pub fn number<'a>(op: &str, pos: usize, v: &'a Value) -> VResult<&'a Decimal> {
    match v {
        Value::Num(d) => Ok(d),
        _ => Err(type_err(op, pos, "number", v)),
    }
}

/// Integer-valued number (type error for non-number, argument for fractional).
pub fn integer(op: &str, pos: usize, v: &Value) -> VResult<BigInt> {
    let d = number(op, pos, v)?;
    d.to_bigint().ok_or_else(|| arg_err(op, "integer required"))
}

/// Integer check on the mathematical value (no host conversion), then a
/// saturating conversion: integers beyond u64 become u64::MAX (§3.2).
fn int_value<'a>(op: &str, pos: usize, v: &'a Value) -> VResult<&'a Decimal> {
    let d = number(op, pos, v)?;
    if !d.is_integer() {
        return Err(arg_err(op, "integer required"));
    }
    Ok(d)
}

pub fn nonneg_u64(op: &str, pos: usize, v: &Value) -> VResult<u64> {
    let d = int_value(op, pos, v)?;
    if d.is_negative() {
        return Err(arg_err(op, "nonnegative integer required"));
    }
    Ok(d.to_u64().unwrap_or(u64::MAX))
}

pub fn positive_u64(op: &str, pos: usize, v: &Value) -> VResult<u64> {
    let d = int_value(op, pos, v)?;
    if d.is_negative() || d.is_zero() {
        return Err(arg_err(op, "positive integer required"));
    }
    Ok(d.to_u64().unwrap_or(u64::MAX))
}

/// An option field validated as an integer within bounds.
pub fn opt_int(op: &str, field: &str, v: &Value, min: u64) -> VResult<u64> {
    let d = match v {
        Value::Num(d) => d,
        _ => return Err(opt_type_err(op, field, "number", v)),
    };
    if !d.is_integer() {
        return Err(arg_err(op, &format!("{field} must be an integer")));
    }
    let i = if d.is_negative() { 0 } else { d.to_u64().unwrap_or(u64::MAX) };
    if d.is_negative() || i < min {
        return Err(arg_err(op, &format!("{field} out of range")));
    }
    Ok(i)
}

/// Counts can saturate when no collection can reach the bound. Durations
/// cannot: shortening a wait changes its meaning (§8.3).
pub fn opt_duration(op: &str, field: &str, v: &Value, min: u64) -> VResult<u64> {
    opt_int(op, field, v, min)?;
    v.as_num().and_then(|d| d.to_u64()).ok_or(Fail::Abort(AbortReason::Resource("clock")))
}

pub fn duration(op: &str, pos: usize, v: &Value) -> VResult<u64> {
    nonneg_u64(op, pos, v)?;
    v.as_num().and_then(|d| d.to_u64()).ok_or(Fail::Abort(AbortReason::Resource("clock")))
}

/// Validate an options record against known fields in table order.
/// Returns the record (or an empty one when omitted).
pub fn options<'a>(op: &str, pos: usize, v: Option<&'a Value>, known: &[&str]) -> VResult<Option<&'a Rc<Record>>> {
    let r = match v {
        None => return Ok(None),
        Some(v) => record(op, pos, v)?,
    };
    let _ = known; // unknown names are checked by `reject_unknown` after known fields
    Ok(Some(r))
}

pub fn reject_unknown(op: &str, r: Option<&Rc<Record>>, known: &[&str]) -> VResult<()> {
    if let Some(r) = r {
        for k in r.keys() {
            if !known.contains(&&**k) {
                return Err(arg_err(op, &format!("unknown option {k:?}")));
            }
        }
    }
    Ok(())
}

pub fn field<'a>(r: Option<&'a Rc<Record>>, name: &str) -> Option<&'a Value> {
    r.and_then(|r| r.get(name))
}

/// Check that a callback value accepts `n` ordinary arguments and no callback.
pub fn callback(op: &str, f: &Value, n: usize) -> VResult<()> {
    match f {
        Value::Func(func) => {
            let sig = func.signature();
            if sig.accepts(n) {
                Ok(())
            } else {
                Err(Fail::Err(err::arity(op, sig, n, "callback_signature")))
            }
        }
        other => Err(type_err(op, 0, "function", other)),
    }
}

/// Function-valued option field (e.g. retry.when).
pub fn function_option(op: &str, field: &str, f: &Value, n: usize) -> VResult<()> {
    match f {
        Value::Func(func) => {
            let sig = func.signature();
            if sig.accepts(n) {
                Ok(())
            } else {
                Err(Fail::Err(err::arity(op, sig, n, "callback_signature")))
            }
        }
        other => Err(opt_type_err(op, field, "function", other)),
    }
}
