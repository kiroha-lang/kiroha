//! Runtime values (spec §3.1).

use crate::decimal::Decimal;
use crate::schema::SchemaValue;
use std::collections::HashMap;
use std::rc::Rc;

/// The thirteen value kinds with their diagnostic names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Record,
    Function,
    Bytes,
    Schema,
    Query,
    Database,
    Transaction,
    Reply,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Null => "null",
            Kind::Bool => "bool",
            Kind::Number => "number",
            Kind::String => "string",
            Kind::Array => "array",
            Kind::Record => "record",
            Kind::Function => "function",
            Kind::Bytes => "bytes",
            Kind::Schema => "schema",
            Kind::Query => "query",
            Kind::Database => "database",
            Kind::Transaction => "transaction",
            Kind::Reply => "reply",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Some(match s {
            "null" => Kind::Null,
            "bool" => Kind::Bool,
            "number" => Kind::Number,
            "string" => Kind::String,
            "array" => Kind::Array,
            "record" => Kind::Record,
            "function" => Kind::Function,
            "bytes" => Kind::Bytes,
            "schema" => Kind::Schema,
            "query" => Kind::Query,
            "database" => Kind::Database,
            "transaction" => Kind::Transaction,
            "reply" => Kind::Reply,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Num(Rc<Decimal>),
    Str(Rc<str>),
    Array(Rc<Vec<Value>>),
    Record(Rc<Record>),
    Func(Rc<Function>),
    Bytes(Rc<Vec<u8>>),
    Schema(Rc<SchemaValue>),
    Query(Rc<Query>),
    Database(Rc<DbHandle>),
    Transaction(Rc<TxHandle>),
    Reply(Rc<Reply>),
}

/// An ordered record with distinct string keys.
#[derive(Clone, Debug, Default)]
pub struct Record {
    entries: Vec<(Rc<str>, Value)>,
    index: Option<HashMap<Rc<str>, usize>>,
}

const INDEX_THRESHOLD: usize = 12;

impl Record {
    pub fn new() -> Record {
        Record::default()
    }

    pub fn with_capacity(n: usize) -> Record {
        Record { entries: Vec::with_capacity(n), index: None }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn position(&self, key: &str) -> Option<usize> {
        if let Some(ix) = &self.index {
            return ix.get(key).copied();
        }
        self.entries.iter().position(|(k, _)| &**k == key)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.position(key).map(|i| &self.entries[i].1)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    /// Insert or replace; replacing retains the original position.
    pub fn insert(&mut self, key: Rc<str>, value: Value) {
        if let Some(i) = self.position(&key) {
            self.entries[i].1 = value;
            return;
        }
        if let Some(ix) = &mut self.index {
            ix.insert(key.clone(), self.entries.len());
        }
        self.entries.push((key, value));
        if self.index.is_none() && self.entries.len() > INDEX_THRESHOLD {
            let mut ix = HashMap::with_capacity(self.entries.len() * 2);
            for (i, (k, _)) in self.entries.iter().enumerate() {
                ix.insert(k.clone(), i);
            }
            self.index = Some(ix);
        }
    }

    /// Push a key known to be absent (caller verified).
    pub fn push(&mut self, key: &str, value: Value) {
        self.insert(Rc::from(key), value);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Rc<str>, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &Rc<str>> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn entries(&self) -> &[(Rc<str>, Value)] {
        &self.entries
    }
}

/// Callable values.
#[derive(Debug)]
pub enum Function {
    Builtin(crate::builtins::Builtin),
    Closure(Closure),
}

#[derive(Debug)]
pub struct Closure {
    /// Code node: a `Define` or `Lambda` node in the program arena.
    pub node: crate::ast::NodeId,
    pub captures: Vec<Value>,
    pub nparams: usize,
    pub has_slot: bool,
    pub named: bool,
}

/// Call signature (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature {
    pub min: usize,
    pub max: usize,
    pub callback: CallbackPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackPolicy {
    Absent,
    Required,
    Optional,
}

impl CallbackPolicy {
    pub fn name(self) -> &'static str {
        match self {
            CallbackPolicy::Absent => "absent",
            CallbackPolicy::Required => "required",
            CallbackPolicy::Optional => "optional",
        }
    }
}

impl Function {
    pub fn signature(&self) -> Signature {
        match self {
            Function::Builtin(b) => b.signature(),
            Function::Closure(c) => Signature {
                min: c.nparams,
                max: c.nparams,
                callback: if c.has_slot { CallbackPolicy::Required } else { CallbackPolicy::Absent },
            },
        }
    }
}

impl Signature {
    /// "Accepts n": legal with n ordinary arguments and no callback.
    pub fn accepts(&self, n: usize) -> bool {
        self.min <= n && n <= self.max && self.callback != CallbackPolicy::Required
    }
}

#[derive(Debug)]
pub struct Query {
    pub segments: Vec<String>,
    pub values: Vec<Value>,
}

#[derive(Debug)]
pub struct DbHandle {
    pub alias: String,
    pub pool: String,
}

#[derive(Debug)]
pub struct TxHandle {
    pub id: u64,
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Value {
    pub fn kind(&self) -> Kind {
        match self {
            Value::Null => Kind::Null,
            Value::Bool(_) => Kind::Bool,
            Value::Num(_) => Kind::Number,
            Value::Str(_) => Kind::String,
            Value::Array(_) => Kind::Array,
            Value::Record(_) => Kind::Record,
            Value::Func(_) => Kind::Function,
            Value::Bytes(_) => Kind::Bytes,
            Value::Schema(_) => Kind::Schema,
            Value::Query(_) => Kind::Query,
            Value::Database(_) => Kind::Database,
            Value::Transaction(_) => Kind::Transaction,
            Value::Reply(_) => Kind::Reply,
        }
    }

    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    pub fn num(d: Decimal) -> Value {
        Value::Num(Rc::new(d))
    }

    pub fn int(i: i64) -> Value {
        Value::num(Decimal::from_i64(i))
    }

    pub fn array(v: Vec<Value>) -> Value {
        Value::Array(Rc::new(v))
    }

    pub fn record(r: Record) -> Value {
        Value::Record(Rc::new(r))
    }

    pub fn bytes(b: Vec<u8>) -> Value {
        Value::Bytes(Rc::new(b))
    }

    /// Build a record from static keys.
    pub fn rec(fields: Vec<(&str, Value)>) -> Value {
        let mut r = Record::with_capacity(fields.len());
        for (k, v) in fields {
            r.push(k, v);
        }
        Value::record(r)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_num(&self) -> Option<&Decimal> {
        match self {
            Value::Num(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_record(&self) -> Option<&Record> {
        match self {
            Value::Record(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Recursively contains only JSON kinds. Returns the first offending kind.
    pub fn first_non_json(&self) -> Option<Kind> {
        if crate::interrupt::poll() {
            return None; // result is discarded by the runtime
        }
        match self {
            Value::Null | Value::Bool(_) | Value::Num(_) | Value::Str(_) => None,
            Value::Array(a) => a.iter().find_map(|v| v.first_non_json()),
            Value::Record(r) => r.iter().find_map(|(_, v)| v.first_non_json()),
            other => Some(other.kind()),
        }
    }

    pub fn is_json(&self) -> bool {
        self.first_non_json().is_none()
    }

    /// Approximate retained size for memory accounting.
    pub fn approx_bytes(&self) -> usize {
        match self {
            Value::Num(d) => d.approx_bytes(),
            Value::Str(s) => 24 + s.len(),
            Value::Array(a) => 24 + a.len() * 16,
            Value::Record(r) => 48 + r.len() * 40,
            Value::Bytes(b) => 24 + b.len(),
            _ => 16,
        }
    }
}

/// JSON structural equality (§3.5). Caller must have verified both are JSON.
pub fn json_equal(a: &Value, b: &Value) -> bool {
    if crate::interrupt::poll() {
        return false; // result is discarded by the runtime
    }
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| json_equal(p, q))
        }
        (Value::Record(x), Value::Record(y)) => {
            x.len() == y.len()
                && x.iter().all(|(k, v)| match y.get(k) {
                    Some(w) => json_equal(v, w),
                    None => false,
                })
        }
        _ => false,
    }
}

/// Hash compatible with `json_equal` (numbers normalized, records order-free).
pub fn json_hash(v: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    fn go(v: &Value, h: &mut std::collections::hash_map::DefaultHasher) {
        if crate::interrupt::poll() {
            return; // result is discarded by the runtime
        }
        match v {
            Value::Null => 0u8.hash(h),
            Value::Bool(b) => {
                1u8.hash(h);
                b.hash(h)
            }
            Value::Num(d) => {
                2u8.hash(h);
                d.hash(h)
            }
            Value::Str(s) => {
                3u8.hash(h);
                s.hash(h)
            }
            Value::Array(a) => {
                4u8.hash(h);
                a.len().hash(h);
                for x in a.iter() {
                    go(x, h);
                }
            }
            Value::Record(r) => {
                5u8.hash(h);
                r.len().hash(h);
                let mut acc: u64 = 0;
                for (k, x) in r.iter() {
                    let mut eh = std::collections::hash_map::DefaultHasher::new();
                    k.hash(&mut eh);
                    go(x, &mut eh);
                    acc = acc.wrapping_add(eh.finish());
                }
                acc.hash(h);
            }
            _ => 6u8.hash(h),
        }
    }
    go(v, &mut h);
    h.finish()
}

/// Error record helpers (§8.1–8.2).
pub mod err {
    use super::*;

    pub fn make(code: &str, message: impl Into<String>, data: Vec<(&str, Value)>) -> Value {
        let msg: String = message.into();
        Value::rec(vec![
            ("code", Value::str(code)),
            ("message", Value::str(&msg)),
            ("data", Value::rec(data)),
        ])
    }

    pub fn code_of(e: &Value) -> &str {
        e.as_record().and_then(|r| r.get("code")).and_then(|c| c.as_str()).unwrap_or("")
    }

    pub fn data_of(e: &Value) -> Option<&Record> {
        e.as_record().and_then(|r| r.get("data")).and_then(|d| d.as_record())
    }

    /// Argument position used in type errors.
    pub enum Pos<'a> {
        Index(usize),
        Option(&'a str),
    }

    pub fn type_error(op: &str, pos: Pos, expected: &str, actual: Kind) -> Value {
        let p = match pos {
            Pos::Index(i) => Value::int(i as i64),
            Pos::Option(s) => Value::str(s),
        };
        make(
            "type",
            format!("{op}: expected {expected}, got {}", actual.name()),
            vec![
                ("operation", Value::str(op)),
                ("position", p),
                ("expected", Value::str(expected)),
                ("actual", Value::str(actual.name())),
            ],
        )
    }

    pub fn argument(op: &str, reason: &str) -> Value {
        make(
            "argument",
            format!("{op}: {reason}"),
            vec![("operation", Value::str(op)), ("reason", Value::str(reason))],
        )
    }

    pub fn arity(op: &str, sig: Signature, got: usize, reason: &str) -> Value {
        make(
            "arity",
            format!("{op}: arity mismatch ({reason})"),
            vec![
                ("operation", Value::str(op)),
                ("min", Value::int(sig.min as i64)),
                ("max", Value::int(sig.max as i64)),
                ("got", Value::int(got as i64)),
                ("callback", Value::str(sig.callback.name())),
                ("reason", Value::str(reason)),
            ],
        )
    }

    pub fn permission(op: &str, capability: &str) -> Value {
        make(
            "permission",
            format!("{op}: capability {capability} not granted"),
            vec![("operation", Value::str(op)), ("capability", Value::str(capability))],
        )
    }

    pub fn number(reason: &str) -> Value {
        make("number", reason.to_string(), vec![("reason", Value::str(reason))])
    }
}
