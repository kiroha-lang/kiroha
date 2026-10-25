//! Lossless tagged wire values for the CLI protocol (spec §15.2).

use crate::value::Value;
use base64::Engine;

/// Convert a runtime value to its tagged wire representation (a JSON value).
pub fn to_wire(v: &Value) -> Value {
    if crate::interrupt::poll() {
        return Value::Null; // discarded: the caller reports the trip as an abort
    }
    match v {
        Value::Null => Value::rec(vec![("t", Value::str("null"))]),
        Value::Bool(b) => Value::rec(vec![("t", Value::str("bool")), ("v", Value::Bool(*b))]),
        Value::Num(d) => {
            if crate::interrupt::poll_big(d.coef().bits() as usize / 3) {
                return Value::Null;
            }
            Value::rec(vec![("t", Value::str("number")), ("v", Value::str(&d.canonical()))])
        }
        Value::Str(s) => Value::rec(vec![("t", Value::str("string")), ("v", Value::Str(s.clone()))]),
        Value::Bytes(b) => Value::rec(vec![
            ("t", Value::str("bytes")),
            ("base64", Value::str(&base64::engine::general_purpose::STANDARD.encode(&**b))),
        ]),
        Value::Array(a) => Value::rec(vec![("t", Value::str("array")), ("v", Value::array(a.iter().map(to_wire).collect()))]),
        Value::Record(r) => Value::rec(vec![
            ("t", Value::str("record")),
            ("v", Value::array(r.iter().map(|(k, x)| Value::array(vec![Value::Str(k.clone()), to_wire(x)])).collect())),
        ]),
        other => Value::rec(vec![("t", Value::str("opaque")), ("kind", Value::str(other.kind().name()))]),
    }
}

/// Serialize a JSON value with the exact Kiri serializer, falling back to the
/// wire form for non-JSON values.
pub fn encode_json_lossless(v: &Value) -> String {
    if v.is_json() {
        crate::json::dump(v)
    } else {
        crate::json::dump(&to_wire(v))
    }
}
