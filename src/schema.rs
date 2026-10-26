//! Schemas: normalization and validation (spec §6).

use crate::value::{err, Kind, Record, Value};
use std::rc::Rc;

/// A normalized schema.
#[derive(Debug, Clone)]
pub enum Norm {
    Str,
    Num,
    Int,
    Bool,
    Json,
    Any,
    Never,
    Null,
    Kind(Kind),
    Ref(Rc<Norm>),
    Array(Box<Norm>),
    Record { fields: Vec<Field>, open: bool },
    Maybe(Box<Norm>),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub key: Rc<str>,
    pub schema: Norm,
    pub optional: bool,
}

/// Nominal schema values: the five atoms and the three wrappers.
#[derive(Debug)]
pub enum SchemaValue {
    Atom(Norm),
    Type(Rc<Norm>),
    Maybe(Norm),
    /// Field-only wrapper; the inner schema is already normalized.
    Optional(Norm),
    /// Open record schema (inner is `Norm::Record { open: true }`).
    Open(Norm),
}

pub fn atom(name: &str) -> Value {
    let n = match name {
        "str" => Norm::Str,
        "num" => Norm::Num,
        "int" => Norm::Int,
        "bool" => Norm::Bool,
        "json" => Norm::Json,
        _ => unreachable!(),
    };
    Value::Schema(Rc::new(SchemaValue::Atom(n)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Root,
    Field,
    Element,
    MaybeOperand,
    OptionalOperand,
}

#[derive(Clone, Debug)]
pub enum PathItem {
    Key(Rc<str>),
    Index(usize),
}

fn path_value(path: &[PathItem]) -> Value {
    Value::array(
        path.iter()
            .map(|p| match p {
                PathItem::Key(k) => Value::Str(k.clone()),
                PathItem::Index(i) => Value::int(*i as i64),
            })
            .collect(),
    )
}

fn schema_error(path: &[PathItem], reason: &str) -> Value {
    err::make(
        "schema",
        format!("malformed schema: {reason}"),
        vec![("path", path_value(path)), ("reason", Value::str(reason))],
    )
}

/// Normalized description plus whether it is an optional-field wrapper.
struct Desc {
    norm: Norm,
    optional: bool,
}

fn normalize_at(v: &Value, pos: Position, path: &mut Vec<PathItem>) -> Result<Desc, Value> {
    if crate::interrupt::poll() {
        return Err(Value::Null); // discarded: the runtime turns the trip into an abort
    }
    match v {
        Value::Schema(s) => match &**s {
            SchemaValue::Type(n) => Ok(Desc { norm: Norm::Ref(n.clone()), optional: false }),
            SchemaValue::Atom(n) => Ok(Desc { norm: n.clone(), optional: false }),
            SchemaValue::Maybe(n) => Ok(Desc { norm: n.clone(), optional: false }),
            SchemaValue::Open(n) => Ok(Desc { norm: n.clone(), optional: false }),
            SchemaValue::Optional(n) => match pos {
                Position::Field => Ok(Desc { norm: n.clone(), optional: true }),
                Position::OptionalOperand => Err(schema_error(path, "nested_optional")),
                _ => Err(schema_error(path, "optional_position")),
            },
        },
        Value::Array(items) => {
            if items.len() != 1 {
                return Err(schema_error(path, "array_arity"));
            }
            path.push(PathItem::Index(0));
            let inner = normalize_at(&items[0], Position::Element, path)?;
            path.pop();
            Ok(Desc { norm: Norm::Array(Box::new(inner.norm)), optional: false })
        }
        Value::Record(r) => {
            let fields = normalize_fields(r, path)?;
            Ok(Desc { norm: Norm::Record { fields, open: false }, optional: false })
        }
        _ => Err(schema_error(path, "invalid_description")),
    }
}

fn normalize_fields(r: &Record, path: &mut Vec<PathItem>) -> Result<Vec<Field>, Value> {
    let mut fields = Vec::with_capacity(r.len());
    for (k, fv) in r.iter() {
        path.push(PathItem::Key(k.clone()));
        let d = normalize_at(fv, Position::Field, path)?;
        path.pop();
        fields.push(Field { key: k.clone(), schema: d.norm, optional: d.optional });
    }
    Ok(fields)
}

/// Normalize a root schema description.
pub fn normalize(v: &Value) -> Result<Norm, Value> {
    Ok(normalize_at(v, Position::Root, &mut Vec::new())?.norm)
}

/// `maybe(S)`.
pub fn make_maybe(v: &Value) -> Result<Value, Value> {
    let d = normalize_at(v, Position::MaybeOperand, &mut Vec::new())?;
    let inner = match unshared(&d.norm) {
        Norm::Maybe(i) => (**i).clone(),
        _ => d.norm,
    };
    Ok(Value::Schema(Rc::new(SchemaValue::Maybe(Norm::Maybe(Box::new(inner))))))
}

/// `optional(S)`.
pub fn make_optional(v: &Value) -> Result<Value, Value> {
    let d = normalize_at(v, Position::OptionalOperand, &mut Vec::new())?;
    Ok(Value::Schema(Rc::new(SchemaValue::Optional(d.norm))))
}

/// `open(S)`.
pub fn make_open(v: &Value) -> Result<Value, Value> {
    let mut path = Vec::new();
    let fields = match v {
        Value::Record(r) => normalize_fields(r, &mut path)?,
        Value::Schema(s) => match &**s {
            SchemaValue::Open(Norm::Record { fields, .. }) => fields.clone(),
            SchemaValue::Type(n) => match unshared(n) {
                Norm::Record { fields, .. } => fields.clone(),
                _ => return Err(schema_error(&path, "record_required")),
            },
            _ => return Err(schema_error(&path, "record_required")),
        },
        _ => return Err(schema_error(&path, "record_required")),
    };
    Ok(Value::Schema(Rc::new(SchemaValue::Open(Norm::Record { fields, open: true }))))
}

/// A validation mismatch.
pub struct Mismatch {
    pub path: Vec<PathItem>,
    pub expected: &'static str,
    pub actual: &'static str,
}

impl Mismatch {
    pub fn path_value(&self) -> Value {
        path_value(&self.path)
    }

    pub fn to_error(&self) -> Value {
        err::make(
            "validation",
            format!("expected {}, got {}", self.expected, self.actual),
            vec![
                ("path", self.path_value()),
                ("expected", Value::str(self.expected)),
                ("actual", Value::str(self.actual)),
            ],
        )
    }
}

fn mismatch(path: &[PathItem], expected: &'static str, actual: Kind) -> Mismatch {
    Mismatch { path: path.to_vec(), expected, actual: actual.name() }
}

/// First non-JSON location inside v, as a mismatch with expected "json".
fn json_mismatch(v: &Value, path: &mut Vec<PathItem>) -> Option<Mismatch> {
    if crate::interrupt::poll() {
        return None; // result is discarded by the runtime
    }
    match v {
        Value::Null | Value::Bool(_) | Value::Num(_) | Value::Str(_) => None,
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                path.push(PathItem::Index(i));
                if let Some(m) = json_mismatch(x, path) {
                    return Some(m);
                }
                path.pop();
            }
            None
        }
        Value::Record(r) => {
            for (k, x) in r.iter() {
                path.push(PathItem::Key(k.clone()));
                if let Some(m) = json_mismatch(x, path) {
                    return Some(m);
                }
                path.pop();
            }
            None
        }
        other => Some(mismatch(path, "json", other.kind())),
    }
}

/// Validate v against a normalized schema; first failing path only.
pub fn validate(v: &Value, s: &Norm) -> Result<(), Mismatch> {
    let mut path = Vec::new();
    match check_at(v, s, &mut path) {
        None => Ok(()),
        Some(m) => Err(m),
    }
}

fn check_at(v: &Value, s: &Norm, path: &mut Vec<PathItem>) -> Option<Mismatch> {
    if crate::interrupt::poll() {
        return None; // result is discarded by the runtime
    }
    match s {
        Norm::Any => None,
        Norm::Never => Some(mismatch(path, "never", v.kind())),
        Norm::Null => (!matches!(v, Value::Null)).then(|| mismatch(path, "null", v.kind())),
        Norm::Kind(kind) => (v.kind() != *kind).then(|| mismatch(path, kind.name(), v.kind())),
        Norm::Ref(inner) => check_at(v, inner, path),
        Norm::Str => (!matches!(v, Value::Str(_))).then(|| mismatch(path, "str", v.kind())),
        Norm::Num => (!matches!(v, Value::Num(_))).then(|| mismatch(path, "num", v.kind())),
        Norm::Int => match v {
            Value::Num(d) if d.is_integer() => None,
            _ => Some(mismatch(path, "int", v.kind())),
        },
        Norm::Bool => (!matches!(v, Value::Bool(_))).then(|| mismatch(path, "bool", v.kind())),
        Norm::Json => json_mismatch(v, path),
        Norm::Maybe(inner) => {
            if matches!(v, Value::Null) {
                None
            } else {
                check_at(v, inner, path)
            }
        }
        Norm::Array(inner) => match v {
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    path.push(PathItem::Index(i));
                    if let Some(m) = check_at(x, inner, path) {
                        return Some(m);
                    }
                    path.pop();
                }
                None
            }
            _ => Some(mismatch(path, "array", v.kind())),
        },
        Norm::Record { fields, open } => {
            let r = match v {
                Value::Record(r) => r,
                _ => return Some(mismatch(path, "record", v.kind())),
            };
            for f in fields {
                path.push(PathItem::Key(f.key.clone()));
                match r.get(&f.key) {
                    None => {
                        if !f.optional {
                            let m = Mismatch {
                                path: path.clone(),
                                expected: expected_name(&f.schema),
                                actual: "missing",
                            };
                            return Some(m);
                        }
                    }
                    Some(x) => {
                        if let Some(m) = check_at(x, &f.schema, path) {
                            return Some(m);
                        }
                    }
                }
                path.pop();
            }
            let known: std::collections::HashSet<&str> = fields.iter().map(|f| &*f.key).collect();
            for (k, x) in r.iter() {
                if crate::interrupt::poll() {
                    return None;
                }
                if known.contains(&**k) {
                    continue;
                }
                path.push(PathItem::Key(k.clone()));
                if *open {
                    if let Some(m) = json_mismatch(x, path) {
                        return Some(m);
                    }
                } else {
                    return Some(mismatch(path, "absent", x.kind()));
                }
                path.pop();
            }
            None
        }
    }
}

fn expected_name(s: &Norm) -> &'static str {
    match s {
        Norm::Any => "any",
        Norm::Never => "never",
        Norm::Null => "null",
        Norm::Kind(k) => k.name(),
        Norm::Ref(inner) => expected_name(inner),
        Norm::Str => "str",
        Norm::Num => "num",
        Norm::Int => "int",
        Norm::Bool => "bool",
        Norm::Json => "json",
        Norm::Array(_) => "array",
        Norm::Record { .. } => "record",
        Norm::Maybe(i) => expected_name(i),
    }
}

fn unshared(mut n: &Norm) -> &Norm {
    while let Norm::Ref(inner) = n { n = inner; }
    n
}
