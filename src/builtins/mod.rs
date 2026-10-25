//! The closed 51-function prelude (spec §7.1) plus the five schema atoms.

pub mod args;
pub mod callbacks;
pub mod db;
pub mod effects;
pub mod headers;
pub mod pure;
pub mod server;

use crate::value::{CallbackPolicy, Signature};

macro_rules! builtins {
    ($( $variant:ident = $name:literal, $min:literal, $max:literal, $cb:ident; )*) => {
        /// Every callable prelude function.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Builtin { $( $variant, )* }

        /// Prelude functions in inventory order.
        pub const ALL: &[Builtin] = &[ $( Builtin::$variant, )* ];

        impl Builtin {
            pub fn name(self) -> &'static str {
                match self { $( Builtin::$variant => $name, )* }
            }

            pub fn signature(self) -> Signature {
                match self {
                    $( Builtin::$variant => Signature { min: $min, max: $max, callback: CallbackPolicy::$cb }, )*
                }
            }
        }
    };
}

builtins! {
    Len = "len", 1, 1, Absent;
    Range = "range", 2, 3, Absent;
    Map = "map", 1, 1, Required;
    Filter = "filter", 1, 1, Required;
    Fold = "fold", 2, 2, Required;
    Each = "each", 1, 1, Required;
    Flat = "flat", 1, 1, Absent;
    Take = "take", 2, 2, Absent;
    Sort = "sort", 1, 1, Optional;
    Group = "group", 1, 1, Required;
    Unique = "unique", 1, 1, Optional;
    Sum = "sum", 1, 1, Absent;
    Keys = "keys", 1, 1, Absent;
    Has = "has", 2, 2, Absent;
    Pick = "pick", 2, 2, Absent;
    Merge = "merge", 2, 2, Absent;
    One = "one", 1, 1, Absent;
    Split = "split", 2, 2, Absent;
    Join = "join", 2, 2, Absent;
    Replace = "replace", 3, 3, Absent;
    Text = "text", 1, 1, Absent;
    Number = "number", 1, 1, Absent;
    Parse = "parse", 1, 1, Absent;
    Dump = "dump", 1, 1, Absent;
    Encode = "encode", 1, 1, Absent;
    Decode = "decode", 1, 1, Absent;
    Check = "check", 2, 2, Absent;
    Maybe = "maybe", 1, 1, Absent;
    Optional = "optional", 1, 1, Absent;
    Open = "open", 1, 1, Absent;
    Parallel = "parallel", 2, 2, Required;
    Sleep = "sleep", 1, 1, Absent;
    Retry = "retry", 1, 1, Required;
    Pages = "pages", 1, 2, Required;
    Fail = "fail", 1, 1, Absent;
    Http = "http", 2, 3, Absent;
    Get = "get", 1, 2, Absent;
    Post = "post", 2, 3, Absent;
    Url = "url", 2, 3, Absent;
    Db = "db", 1, 1, Absent;
    Sql = "sql", 2, 3, Absent;
    Tx = "tx", 1, 2, Required;
    Serve = "serve", 1, 2, Absent;
    Input = "input", 1, 2, Absent;
    Reply = "reply", 2, 3, Absent;
    Env = "env", 1, 1, Absent;
    Read = "read", 1, 1, Absent;
    Write = "write", 2, 2, Absent;
    Now = "now", 0, 0, Absent;
    Uuid = "uuid", 0, 0, Absent;
    Log = "log", 1, 1, Absent;
}

/// Schema atom names, after the functions in the prelude table.
pub const ATOMS: &[&str] = &["str", "num", "int", "bool", "json"];

/// Prelude table index for a name.
pub fn prelude_index(name: &str) -> Option<u32> {
    if let Some(i) = ALL.iter().position(|b| b.name() == name) {
        return Some(i as u32);
    }
    ATOMS.iter().position(|a| *a == name).map(|i| (ALL.len() + i) as u32)
}
