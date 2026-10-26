//! Receiver-based collection members. Syntax is independent of this registry.
//!
//! A bound member keeps an ordinary callable target and an immutable receiver;
//! extension methods can use the same dispatch path without parser changes.
use crate::builtins::Builtin;
use crate::value::{Kind, Signature};

#[derive(Clone, Copy, Debug)]
pub struct Method {
    pub name: &'static str,
    pub builtin: Builtin,
    pub receivers: &'static [Kind],
    pub property: bool,
}

impl Method {
    pub fn signature(self) -> Signature {
        let mut sig = self.builtin.signature();
        sig.min -= 1;
        sig.max -= 1;
        sig
    }
    pub fn expected(self) -> &'static str {
        match self.receivers {
            [Kind::Array] => "array",
            [Kind::Record] => "record",
            [Kind::String] => "string",
            _ => "array, record, string or bytes",
        }
    }
}

macro_rules! member {
    ($name:literal, $builtin:ident, [$($kind:ident),+], $property:literal) => {
        Method { name: $name, builtin: Builtin::$builtin, receivers: &[$(Kind::$kind),+], property: $property }
    };
}

pub const ALL: &[Method] = &[
    member!("size", Len, [Array, Record, String, Bytes], true),
    member!("map", Map, [Array], false),
    member!("filter", Filter, [Array], false),
    member!("fold", Fold, [Array], false),
    member!("each", Each, [Array], false),
    member!("flat", Flat, [Array], false),
    member!("take", Take, [Array], false),
    member!("sort", Sort, [Array], false),
    member!("group", Group, [Array], false),
    member!("unique", Unique, [Array], false),
    member!("sum", Sum, [Array], false),
    member!("keys", Keys, [Record], false),
    member!("has", Has, [Record], false),
    member!("pick", Pick, [Record], false),
    member!("merge", Merge, [Record], false),
    member!("one", One, [Array], false),
    member!("split", Split, [String], false),
    member!("join", Join, [Array], false),
    member!("replace", Replace, [String], false),
    member!("parallel", Parallel, [Array], false),
];

pub fn named(name: &str) -> Option<&'static Method> {
    ALL.iter().find(|m| m.name == name)
}
pub fn lookup(kind: Kind, name: &str) -> Option<&'static Method> {
    ALL.iter().find(|m| m.name == name && m.receivers.contains(&kind))
}
pub fn is_method(builtin: Builtin) -> bool {
    ALL.iter().any(|m| m.builtin == builtin)
}
