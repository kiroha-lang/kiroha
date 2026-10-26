//! Type syntax and structural types. Runtime schema descriptors are a bridge,
//! not the representation used to type-check expressions or generic functions.
use crate::builtins::Builtin;
use crate::diag::Span;
use crate::value::Value;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum TypeExprKind {
    Name {
        name: Rc<str>,
        args: Vec<TypeExpr>,
        target: TypeRef,
    },
    Array(Box<TypeExpr>),
    Record(Vec<(Rc<str>, TypeExpr)>),
    Function {
        params: Vec<TypeExpr>,
        callback: Option<Box<TypeExpr>>,
        result: Box<TypeExpr>,
    },
    Wrap {
        name: Rc<str>,
        inner: Box<TypeExpr>,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum TypeRef {
    Unresolved,
    Builtin,
    Alias(u32),
    Param(u32),
}
#[derive(Clone, Debug)]
pub struct TypeParam {
    pub name: Rc<str>,
    pub id: u32,
}
#[derive(Clone, Debug)]
pub struct Alias {
    pub name: Rc<str>,
    pub params: Vec<TypeParam>,
    pub body: TypeExpr,
    pub ty: Option<Ty>,
    pub runtime: Option<Value>,
}

pub type Ty = Rc<Type>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: Rc<str>,
    pub ty: Ty,
    pub optional: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub params: Vec<Ty>,
    pub min: usize,
    pub callback: Option<Ty>,
    pub optional_callback: bool,
    pub result: Ty,
    pub generics: Vec<u32>,
    pub builtin: Option<Builtin>,
    pub receiver: Option<Ty>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Any,
    Never,
    Null,
    Bool,
    Int,
    Num,
    Str,
    Json,
    Bytes,
    Query,
    Database,
    Transaction,
    Reply,
    Array(Ty),
    Record {
        fields: Vec<Field>,
        rest: Option<Ty>,
    },
    Function(Function),
    Schema(Ty),
    OptionalSchema(Ty),
    Param {
        id: u32,
        name: Rc<str>,
    },
    Hole(u32),
    Union(Vec<Ty>),
}
pub fn ty(t: Type) -> Ty {
    Rc::new(t)
}
pub fn builtin(name: &str) -> Option<Ty> {
    Some(ty(match name {
        "any" => Type::Any,
        "never" => Type::Never,
        "null" => Type::Null,
        "bool" => Type::Bool,
        "int" => Type::Int,
        "num" => Type::Num,
        "str" => Type::Str,
        "json" => Type::Json,
        "bytes" => Type::Bytes,
        "query" => Type::Query,
        "database" => Type::Database,
        "transaction" => Type::Transaction,
        "reply" => Type::Reply,
        _ => return None,
    }))
}
pub fn display(t: &Ty) -> String {
    fn render(t: &Ty, remaining: &mut usize) -> String {
        if *remaining == 0 {
            return "…".into();
        }
        *remaining -= 1;
        match &**t {
            Type::Any => "any".into(),
            Type::Never => "never".into(),
            Type::Null => "null".into(),
            Type::Bool => "bool".into(),
            Type::Int => "int".into(),
            Type::Num => "num".into(),
            Type::Str => "str".into(),
            Type::Json => "json".into(),
            Type::Bytes => "bytes".into(),
            Type::Query => "query".into(),
            Type::Database => "database".into(),
            Type::Transaction => "transaction".into(),
            Type::Reply => "reply".into(),
            Type::Param { name, .. } => name.to_string(),
            Type::Hole(_) => "_".into(),
            Type::Array(e) => format!("[{}]", render(e, remaining)),
            Type::Schema(s) => format!("schema<{}>", render(s, remaining)),
            Type::OptionalSchema(s) => format!("optional-schema<{}>", render(s, remaining)),
            Type::Union(xs) => xs
                .iter()
                .map(|t| render(t, remaining))
                .collect::<Vec<_>>()
                .join(" | "),
            Type::Record { fields, rest } => {
                let fs = fields
                    .iter()
                    .map(|f| {
                        format!(
                            "{}{}:{}",
                            f.name,
                            if f.optional { "?" } else { "" },
                            render(&f.ty, remaining)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                if rest.is_some() {
                    format!("open({{{fs}}})")
                } else {
                    format!("{{{fs}}}")
                }
            }
            Type::Function(f) => {
                let args = f
                    .params
                    .iter()
                    .map(|t| render(t, remaining))
                    .collect::<Vec<_>>()
                    .join(", ");
                let cb = f
                    .callback
                    .as_ref()
                    .map(|c| format!("{{{}}}", render(c, remaining)))
                    .unwrap_or_default();
                format!("({args}){cb}=>{}", render(&f.result, remaining))
            }
        }
    }
    render(t, &mut 128)
}

pub fn join(a: Ty, b: Ty) -> Ty {
    if same(&a, &b) {
        return a;
    }
    if matches!(&*a, Type::Any) || matches!(&*b, Type::Any) {
        return ty(Type::Any);
    }
    if matches!(&*a, Type::Never) {
        return b;
    }
    if matches!(&*b, Type::Never) {
        return a;
    }
    if matches!((&*a, &*b), (Type::Num, Type::Int) | (Type::Int, Type::Num)) {
        return ty(Type::Num);
    }
    if let (Type::Array(a), Type::Array(b)) = (&*a, &*b) {
        return ty(Type::Array(join(a.clone(), b.clone())));
    }
    let mut choices = match &*a {
        Type::Union(xs) => xs.clone(),
        _ => vec![a],
    };
    let others = match &*b {
        Type::Union(xs) => xs.clone(),
        _ => vec![b],
    };
    for x in others {
        if !choices.iter().any(|old| same(old, &x)) {
            choices.push(x);
        }
    }
    ty(Type::Union(choices))
}

/// Structural equality of a shared type DAG, independent of record field order.
pub fn same(a: &Ty, b: &Ty) -> bool {
    fn go(a: &Ty, b: &Ty, memo: &mut std::collections::HashMap<(usize, usize), bool>) -> bool {
        if Rc::ptr_eq(a, b) {
            return true;
        }
        let key = (Rc::as_ptr(a) as usize, Rc::as_ptr(b) as usize);
        if let Some(result) = memo.get(&key) {
            return *result;
        }
        let result = match (&**a, &**b) {
            (Type::Array(a), Type::Array(b))
            | (Type::Schema(a), Type::Schema(b))
            | (Type::OptionalSchema(a), Type::OptionalSchema(b)) => go(a, b, memo),
            (Type::Param { id: a, .. }, Type::Param { id: b, .. })
            | (Type::Hole(a), Type::Hole(b)) => a == b,
            (
                Type::Record {
                    fields: a,
                    rest: ar,
                },
                Type::Record {
                    fields: b,
                    rest: br,
                },
            ) => {
                a.len() == b.len()
                    && a.iter().all(|af| {
                        b.iter().find(|bf| af.name == bf.name).is_some_and(|bf| {
                            af.optional == bf.optional && go(&af.ty, &bf.ty, memo)
                        })
                    })
                    && match (ar, br) {
                        (None, None) => true,
                        (Some(a), Some(b)) => go(a, b, memo),
                        _ => false,
                    }
            }
            (Type::Function(a), Type::Function(b)) => {
                (match (&a.receiver, &b.receiver) {
                    (None, None) => true,
                    (Some(a), Some(b)) => go(a, b, memo),
                    _ => false,
                }) && a.min == b.min
                    && a.optional_callback == b.optional_callback
                    && a.generics == b.generics
                    && a.builtin == b.builtin
                    && a.params.len() == b.params.len()
                    && a.params.iter().zip(&b.params).all(|(a, b)| go(a, b, memo))
                    && go(&a.result, &b.result, memo)
                    && match (&a.callback, &b.callback) {
                        (None, None) => true,
                        (Some(a), Some(b)) => go(a, b, memo),
                        _ => false,
                    }
            }
            (Type::Union(a), Type::Union(b)) => {
                a.len() == b.len() && a.iter().all(|a| b.iter().any(|b| go(a, b, memo)))
            }
            (
                Type::Array(_)
                | Type::Schema(_)
                | Type::OptionalSchema(_)
                | Type::Record { .. }
                | Type::Function(_)
                | Type::Param { .. }
                | Type::Hole(_)
                | Type::Union(_),
                _,
            ) => false,
            (
                _,
                Type::Array(_)
                | Type::Schema(_)
                | Type::OptionalSchema(_)
                | Type::Record { .. }
                | Type::Function(_)
                | Type::Param { .. }
                | Type::Hole(_)
                | Type::Union(_),
            ) => false,
            _ => std::mem::discriminant(&**a) == std::mem::discriminant(&**b),
        };
        memo.insert(key, result);
        result
    }
    go(a, b, &mut std::collections::HashMap::new())
}
