use crate::diag::{Diagnostic, Span};
use crate::types::{display, ty, Field, Function, Ty, Type};
use crate::value::Value;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Solver {
    pub bindings: Vec<Option<Ty>>,
}
impl Solver {
    pub fn fresh(&mut self) -> Ty {
        let id = self.bindings.len() as u32;
        self.bindings.push(None);
        ty(Type::Hole(id))
    }
    pub fn resolve(&self, t: &Ty) -> Ty {
        match &**t {
            Type::Hole(id) => self.bindings[*id as usize]
                .as_ref()
                .map(|v| self.resolve(v))
                .unwrap_or_else(|| t.clone()),
            _ => t.clone(),
        }
    }
    pub fn deep(&self, t: &Ty) -> Ty {
        fn walk(s: &Solver, t: &Ty, memo: &mut HashMap<usize, Ty>) -> Ty {
            let t = s.resolve(t);
            let key = std::rc::Rc::as_ptr(&t) as usize;
            if let Some(found) = memo.get(&key) {
                return found.clone();
            }
            let result = match &*t {
                Type::Array(e) => ty(Type::Array(walk(s, e, memo))),
                Type::Schema(e) => ty(Type::Schema(walk(s, e, memo))),
                Type::OptionalSchema(e) => ty(Type::OptionalSchema(walk(s, e, memo))),
                Type::Record { fields, rest } => ty(Type::Record {
                    fields: fields
                        .iter()
                        .map(|f| Field {
                            name: f.name.clone(),
                            ty: walk(s, &f.ty, memo),
                            optional: f.optional,
                        })
                        .collect(),
                    rest: rest.as_ref().map(|r| walk(s, r, memo)),
                }),
                Type::Function(f) => ty(Type::Function(Function {
                    params: f.params.iter().map(|p| walk(s, p, memo)).collect(),
                    result: walk(s, &f.result, memo),
                    callback: f.callback.as_ref().map(|c| walk(s, c, memo)),
                    ..f.clone()
                })),
                Type::Union(xs) => xs
                    .iter()
                    .map(|x| walk(s, x, memo))
                    .fold(ty(Type::Never), crate::types::join),
                _ => t,
            };
            memo.insert(key, result.clone());
            result
        }
        walk(self, t, &mut HashMap::new())
    }
    fn occurs(&self, id: u32, t: &Ty) -> bool {
        fn walk(s: &Solver, id: u32, t: &Ty, seen: &mut HashSet<usize>) -> bool {
            let t = s.resolve(t);
            if !seen.insert(std::rc::Rc::as_ptr(&t) as usize) {
                return false;
            }
            match &*t {
                Type::Hole(i) => *i == id,
                Type::Array(e) | Type::Schema(e) | Type::OptionalSchema(e) => walk(s, id, e, seen),
                Type::Record { fields, rest } => {
                    fields.iter().any(|f| walk(s, id, &f.ty, seen))
                        || rest.as_ref().is_some_and(|r| walk(s, id, r, seen))
                }
                Type::Function(f) => {
                    f.params.iter().any(|p| walk(s, id, p, seen))
                        || walk(s, id, &f.result, seen)
                        || f.callback.as_ref().is_some_and(|c| walk(s, id, c, seen))
                }
                Type::Union(xs) => xs.iter().any(|x| walk(s, id, x, seen)),
                _ => false,
            }
        }
        walk(self, id, t, &mut HashSet::new())
    }
    pub fn bind(&mut self, id: u32, value: Ty, span: Span) -> Result<(), Diagnostic> {
        if matches!(&*value, Type::Hole(i) if *i == id) {
            return Ok(());
        }
        if self.occurs(id, &value) {
            return Err(Diagnostic::new(
                "check",
                "type",
                "infinite type; add a finite annotation",
                span,
            ));
        }
        self.bindings[id as usize] = Some(value);
        Ok(())
    }
    pub fn require(&mut self, actual: &Ty, expected: &Ty, span: Span) -> Result<(), Diagnostic> {
        self.require_inner(actual, expected, span, &mut HashSet::new())
    }
    fn require_inner(
        &mut self,
        actual: &Ty,
        expected: &Ty,
        span: Span,
        seen: &mut HashSet<(usize, usize)>,
    ) -> Result<(), Diagnostic> {
        let actual = self.resolve(actual);
        let expected = self.resolve(expected);
        if crate::types::same(&actual, &expected) {
            return Ok(());
        }
        if let Type::Hole(id) = &*expected {
            return self.bind(*id, actual, span);
        }
        if let Type::Hole(id) = &*actual {
            return self.bind(*id, expected, span);
        }
        if matches!(&*actual, Type::Any | Type::Never) || matches!(&*expected, Type::Any) {
            return Ok(());
        }
        let key = (
            std::rc::Rc::as_ptr(&actual) as usize,
            std::rc::Rc::as_ptr(&expected) as usize,
        );
        if seen.contains(&key) {
            return Ok(());
        }
        let ok = match (&*actual, &*expected) {
            (Type::Int, Type::Num) => true,
            (Type::Union(xs), _) => {
                for x in xs {
                    self.require_inner(x, &expected, span, seen)?;
                }
                true
            }
            (_, Type::Union(xs)) => {
                for x in xs {
                    let saved = self.bindings.clone();
                    let saved_seen = seen.clone();
                    if self.require_inner(&actual, x, span, seen).is_ok() {
                        return Ok(());
                    }
                    self.bindings = saved;
                    *seen = saved_seen;
                }
                false
            }
            (_, Type::Json) => self.json(&actual, span)?,
            (Type::Array(a), Type::Array(e)) | (Type::Schema(a), Type::Schema(e)) => {
                self.require_inner(a, e, span, seen)?;
                true
            }
            (
                Type::Record {
                    fields: a,
                    rest: ar,
                },
                Type::Record {
                    fields: e,
                    rest: er,
                },
            ) => {
                for field in e {
                    match a.iter().find(|f| f.name == field.name) {
                        Some(found) if field.optional || !found.optional => {
                            self.require_inner(&found.ty, &field.ty, span, seen)?
                        }
                        None if field.optional => (),
                        _ => return Err(self.error(&actual, &expected, span)),
                    }
                }
                if let Some(rest) = er {
                    for f in a.iter().filter(|f| !e.iter().any(|g| g.name == f.name)) {
                        self.require_inner(&f.ty, rest, span, seen)?;
                    }
                    if let Some(r) = ar {
                        self.require_inner(r, rest, span, seen)?;
                    }
                }
                true
            }
            (Type::Function(a), Type::Function(e))
                if a.min <= e.min && a.params.len() >= e.params.len() =>
            {
                for (want, accept) in e.params.iter().zip(&a.params) {
                    self.require_inner(want, accept, span, seen)?;
                }
                match (&a.callback, &e.callback) {
                    (Some(_), Some(_)) if e.optional_callback && !a.optional_callback => {
                        return Err(self.error(&actual, &expected, span))
                    }
                    (Some(ac), Some(ec)) => self.require_inner(ec, ac, span, seen)?,
                    (Some(_), None) if !a.optional_callback => {
                        return Err(self.error(&actual, &expected, span))
                    }
                    (None, Some(_)) => return Err(self.error(&actual, &expected, span)),
                    _ => (),
                }
                // input(request) defaults to json; only the two-argument form
                // is generic in its schema. Preserve that distinction when a
                // builtin is assigned to a one-argument function type.
                let result =
                    if a.builtin == Some(crate::builtins::Builtin::Input) && e.params.len() == 1 {
                        ty(Type::Json)
                    } else {
                        a.result.clone()
                    };
                self.require_inner(&result, &e.result, span, seen)?;
                true
            }
            _ => false,
        };
        if ok {
            seen.insert(key);
            Ok(())
        } else {
            Err(self.error(&actual, &expected, span))
        }
    }
    fn json(&mut self, t: &Ty, span: Span) -> Result<bool, Diagnostic> {
        self.json_inner(t, span, &mut HashSet::new())
    }
    fn json_inner(
        &mut self,
        t: &Ty,
        span: Span,
        seen: &mut HashSet<usize>,
    ) -> Result<bool, Diagnostic> {
        let t = self.resolve(t);
        if !seen.insert(std::rc::Rc::as_ptr(&t) as usize) {
            return Ok(true);
        }
        match &*t {
            Type::Null
            | Type::Bool
            | Type::Int
            | Type::Num
            | Type::Str
            | Type::Json
            | Type::Never
            | Type::Any => Ok(true),
            Type::Hole(id) => {
                self.bind(*id, ty(Type::Json), span)?;
                Ok(true)
            }
            Type::Array(e) => self.json_inner(e, span, seen),
            Type::Record { fields, rest } => {
                for f in fields {
                    if !self.json_inner(&f.ty, span, seen)? {
                        return Ok(false);
                    }
                }
                if let Some(r) = rest {
                    return self.json_inner(r, span, seen);
                }
                Ok(true)
            }
            Type::Union(xs) => {
                for x in xs {
                    if !self.json_inner(x, span, seen)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
    pub fn error(&self, actual: &Ty, expected: &Ty, span: Span) -> Diagnostic {
        let actual = display(&self.deep(actual));
        let expected = display(&self.deep(expected));
        Diagnostic::new(
            "check",
            "type",
            format!("expected {expected}, got {actual}"),
            span,
        )
        .with("expected", Value::str(&expected))
        .with("actual", Value::str(&actual))
    }
    pub fn free(&self, t: &Ty, out: &mut HashSet<u32>) {
        fn walk(s: &Solver, t: &Ty, out: &mut HashSet<u32>, seen: &mut HashSet<usize>) {
            let t = s.resolve(t);
            if !seen.insert(std::rc::Rc::as_ptr(&t) as usize) {
                return;
            }
            match &*t {
                Type::Hole(id) => {
                    out.insert(*id);
                }
                Type::Array(e) | Type::Schema(e) | Type::OptionalSchema(e) => walk(s, e, out, seen),
                Type::Record { fields, rest } => {
                    for f in fields {
                        walk(s, &f.ty, out, seen);
                    }
                    if let Some(r) = rest {
                        walk(s, r, out, seen);
                    }
                }
                Type::Function(f) => {
                    for p in &f.params {
                        walk(s, p, out, seen);
                    }
                    if let Some(c) = &f.callback {
                        walk(s, c, out, seen);
                    }
                    walk(s, &f.result, out, seen);
                }
                Type::Union(xs) => {
                    for x in xs {
                        walk(s, x, out, seen);
                    }
                }
                _ => (),
            }
        }
        walk(self, t, out, &mut HashSet::new());
    }
}

pub(super) fn substitute(t: &Ty, params: &HashMap<u32, Ty>, holes: &HashMap<u32, Ty>) -> Ty {
    fn walk(
        t: &Ty,
        params: &HashMap<u32, Ty>,
        holes: &HashMap<u32, Ty>,
        memo: &mut HashMap<usize, Ty>,
    ) -> Ty {
        let key = std::rc::Rc::as_ptr(t) as usize;
        if let Some(found) = memo.get(&key) {
            return found.clone();
        }
        let result = match &**t {
            Type::Param { id, .. } => params.get(id).cloned().unwrap_or_else(|| t.clone()),
            Type::Hole(id) => holes.get(id).cloned().unwrap_or_else(|| t.clone()),
            Type::Array(e) => ty(Type::Array(walk(e, params, holes, memo))),
            Type::Schema(e) => ty(Type::Schema(walk(e, params, holes, memo))),
            Type::OptionalSchema(e) => ty(Type::OptionalSchema(walk(e, params, holes, memo))),
            Type::Record { fields, rest } => ty(Type::Record {
                fields: fields
                    .iter()
                    .map(|f| Field {
                        name: f.name.clone(),
                        ty: walk(&f.ty, params, holes, memo),
                        optional: f.optional,
                    })
                    .collect(),
                rest: rest.as_ref().map(|r| walk(r, params, holes, memo)),
            }),
            Type::Function(f) => ty(Type::Function(Function {
                params: f
                    .params
                    .iter()
                    .map(|p| walk(p, params, holes, memo))
                    .collect(),
                callback: f.callback.as_ref().map(|c| walk(c, params, holes, memo)),
                result: walk(&f.result, params, holes, memo),
                ..f.clone()
            })),
            Type::Union(xs) => xs
                .iter()
                .map(|x| walk(x, params, holes, memo))
                .fold(ty(Type::Never), crate::types::join),
            _ => t.clone(),
        };
        memo.insert(key, result.clone());
        result
    }
    walk(t, params, holes, &mut HashMap::new())
}
