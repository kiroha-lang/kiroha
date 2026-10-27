//! Structural static checking and rank-one generic inference. `any` is an
//! explicit escape hatch, never a fallback for unknown or ambiguous types.
mod builtins;
mod solver;
use crate::ast::{BinOp, Body, Node, NodeId, Program, UnOp, Var};
use crate::builtins::Builtin;
use crate::diag::{Diagnostic, Span};
use crate::types::{join, ty, Field, Function, Ty, Type, TypeExpr, TypeExprKind, TypeRef};
use crate::value::{Kind, Value};
use builtins::{array, dict, function, record};
use solver::{substitute, Solver};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

struct Context {
    locals: Vec<Ty>,
    captures: Vec<Ty>,
    self_fn: Ty,
    needs_result_annotation: bool,
}
struct Checker<'a> {
    program: &'a mut Program,
    solver: Solver,
    contexts: Vec<Context>,
    prelude: Vec<Ty>,
    next_param: u32,
    alias_cache: HashMap<u32, Vec<(Vec<Ty>, Ty)>>,
    norm_cache: HashMap<usize, (Ty, Rc<crate::schema::Norm>)>,
}
pub fn check(program: &mut Program) -> Result<(), Diagnostic> {
    let nlocals = program.nlocals as usize;
    let next_param = program.next_type_param;
    let mut checker = Checker {
        program,
        solver: Solver::default(),
        contexts: vec![Context {
            locals: vec![ty(Type::Never); nlocals],
            captures: Vec::new(),
            self_fn: ty(Type::Never),
            needs_result_annotation: false,
        }],
        prelude: Vec::new(),
        next_param,
        alias_cache: HashMap::new(),
        norm_cache: HashMap::new(),
    };
    checker.program.inferred_types = vec![None; checker.program.nodes.len()];
    for b in crate::builtins::GLOBALS {
        let t = checker.builtin_type(*b, None, Span::default())?;
        checker.prelude.push(t);
    }
    for name in crate::builtins::ATOMS {
        checker
            .prelude
            .push(ty(Type::Schema(crate::types::builtin(name).unwrap())));
    }
    for index in 0..checker.program.aliases.len() {
        let body = checker.program.aliases[index].body.clone();
        let t = checker.annotation(&body)?;
        checker.program.aliases[index].ty = Some(t);
    }
    let items = checker.program.items.clone();
    let result = checker.sequence(&items)?;
    let mut free = HashSet::new();
    checker.solver.free(&result, &mut free);
    if !free.is_empty() {
        return Err(checker.ambiguous(
            checker
                .program
                .items
                .last()
                .map(|id| checker.program.span(*id))
                .unwrap_or_default(),
        ));
    }
    for t in checker.program.inferred_types.iter_mut().flatten() {
        *t = checker.solver.deep(t);
    }
    Ok(())
}

fn parameters_in(t: &Ty, out: &mut HashSet<u32>) {
    fn walk(t: &Ty, out: &mut HashSet<u32>, seen: &mut HashSet<usize>) {
        if !seen.insert(Rc::as_ptr(t) as usize) {
            return;
        }
        match &**t {
            Type::Param { id, .. } => {
                out.insert(*id);
            }
            Type::Array(e) | Type::Schema(e) | Type::OptionalSchema(e) => walk(e, out, seen),
            Type::Record { fields, rest } => {
                for f in fields {
                    walk(&f.ty, out, seen);
                }
                if let Some(r) = rest {
                    walk(r, out, seen);
                }
            }
            Type::Function(f) => {
                for p in &f.params {
                    walk(p, out, seen);
                }
                if let Some(c) = &f.callback {
                    walk(c, out, seen);
                }
                walk(&f.result, out, seen);
            }
            Type::Union(xs) => {
                for x in xs {
                    walk(x, out, seen);
                }
            }
            _ => (),
        }
    }
    walk(t, out, &mut HashSet::new());
}

impl Checker<'_> {
    fn ambiguous(&self, span: Span) -> Diagnostic {
        Diagnostic::new(
            "check",
            "type_annotation_required",
            "type is ambiguous; add an annotation (or write any explicitly for dynamic typing)",
            span,
        )
    }
    fn error(&self, message: impl Into<String>, span: Span) -> Diagnostic {
        Diagnostic::new("check", "type", message, span)
    }
    fn arity(&self, expected: usize, got: usize, span: Span) -> Diagnostic {
        Diagnostic::new(
            "check",
            "type_arity",
            format!("expected {expected} type arguments, got {got}"),
            span,
        )
        .with("expected", Value::int(expected as i64))
        .with("got", Value::int(got as i64))
    }
    fn instantiate_alias(&mut self, id: u32, args: Vec<Ty>, span: Span) -> Result<Ty, Diagnostic> {
        let alias = &self.program.aliases[id as usize];
        if alias.params.len() != args.len() {
            return Err(self.arity(alias.params.len(), args.len(), span));
        }
        let body = alias
            .ty
            .clone()
            .expect("aliases resolve in declaration order");
        if args.is_empty() {
            return Ok(body);
        }
        if let Some((_, result)) = self.alias_cache.get(&id).and_then(|instances| {
            instances.iter().find(|(previous, _)| {
                previous.len() == args.len()
                    && previous
                        .iter()
                        .zip(&args)
                        .all(|(a, b)| crate::types::same(a, b))
            })
        }) {
            return Ok(result.clone());
        }
        let params = alias
            .params
            .iter()
            .zip(&args)
            .map(|(p, t)| (p.id, t.clone()))
            .collect();
        let result = substitute(&body, &params, &HashMap::new());
        self.alias_cache
            .entry(id)
            .or_default()
            .push((args, result.clone()));
        Ok(result)
    }
    fn annotation(&mut self, expr: &TypeExpr) -> Result<Ty, Diagnostic> {
        let span = expr.span;
        Ok(match &expr.kind {
            TypeExprKind::Name { name, args, target } => {
                let args = args
                    .iter()
                    .map(|e| self.annotation(e))
                    .collect::<Result<Vec<_>, _>>()?;
                match target {
                    TypeRef::Builtin if &**name == "schema" => {
                        if args.len() != 1 {
                            return Err(self.arity(1, args.len(), span));
                        }
                        ty(Type::Schema(args[0].clone()))
                    }
                    TypeRef::Builtin => {
                        if !args.is_empty() {
                            return Err(self.arity(0, args.len(), span));
                        }
                        crate::types::builtin(name).unwrap()
                    }
                    TypeRef::Param(id) => {
                        if !args.is_empty() {
                            return Err(self.arity(0, args.len(), span));
                        }
                        ty(Type::Param {
                            id: *id,
                            name: name.clone(),
                        })
                    }
                    TypeRef::Alias(id) => self.instantiate_alias(*id, args, span)?,
                    TypeRef::Unresolved => unreachable!(),
                }
            }
            TypeExprKind::Array(e) => array(self.annotation(e)?),
            TypeExprKind::Record(fields) => {
                let mut result = Vec::new();
                for (name, annotation) in fields {
                    let (optional, inner) = match &annotation.kind {
                        TypeExprKind::Wrap { name, inner } if &**name == "optional" => {
                            (true, &**inner)
                        }
                        _ => (false, annotation),
                    };
                    result.push(Field {
                        name: name.clone(),
                        ty: self.annotation(inner)?,
                        optional,
                    });
                }
                ty(Type::Record {
                    fields: result,
                    rest: None,
                })
            }
            TypeExprKind::Function {
                params,
                callback,
                result,
            } => {
                let params = params
                    .iter()
                    .map(|p| self.annotation(p))
                    .collect::<Result<Vec<_>, _>>()?;
                let cb = callback.as_ref().map(|c| self.annotation(c)).transpose()?;
                let result = self.annotation(result)?;
                let mut f = if let Type::Function(f) = &*function(params, result) {
                    f.clone()
                } else {
                    unreachable!()
                };
                if let Some(c) = &cb {
                    if !matches!(&**c, Type::Function(_) | Type::Any | Type::Param { .. }) {
                        return Err(self.error("callback type must be a function", span));
                    }
                }
                f.callback = cb;
                ty(Type::Function(f))
            }
            TypeExprKind::Wrap { name, inner } => {
                let inner = self.annotation(inner)?;
                match &**name {
                    "maybe" => join(inner, ty(Type::Null)),
                    "open" => match &*inner {
                        Type::Record { fields, .. } => ty(Type::Record {
                            fields: fields.clone(),
                            rest: Some(ty(Type::Json)),
                        }),
                        Type::Any => inner,
                        _ => return Err(self.error("open requires a record type", span)),
                    },
                    "optional" => {
                        return Err(
                            self.error("optional is only allowed directly on a record field", span)
                        )
                    }
                    _ => unreachable!(),
                }
            }
        })
    }
    fn norm(&mut self, t: &Ty, span: Span) -> Result<Rc<crate::schema::Norm>, Diagnostic> {
        use crate::schema::Norm;
        let t = self.solver.resolve(t);
        let key = Rc::as_ptr(&t) as usize;
        if let Some((_, n)) = self.norm_cache.get(&key) {
            return Ok(n.clone());
        }
        let n = match &*t {
            Type::Never => Norm::Never,
            Type::Any => Norm::Any,
            Type::Null => Norm::Null,
            Type::Bool => Norm::Bool,
            Type::Int => Norm::Int,
            Type::Num => Norm::Num,
            Type::Str => Norm::Str,
            Type::Json => Norm::Json,
            Type::Bytes => Norm::Kind(Kind::Bytes),
            Type::Query => Norm::Kind(Kind::Query),
            Type::Database => Norm::Kind(Kind::Database),
            Type::Transaction => Norm::Kind(Kind::Transaction),
            Type::Reply => Norm::Kind(Kind::Reply),
            Type::Array(e) => Norm::Array(Box::new(Norm::Ref(self.norm(e, span)?))),
            Type::Record { fields, rest } => {
                let mut fs = Vec::new();
                for field in fields {
                    fs.push(crate::schema::Field {
                        key: field.name.clone(),
                        schema: Norm::Ref(self.norm(&field.ty, span)?),
                        optional: field.optional,
                    });
                }
                Norm::Record {
                    fields: fs,
                    open: rest.is_some(),
                }
            }
            Type::Union(xs) if xs.iter().any(|x| matches!(&**x, Type::Null)) => {
                let other = xs
                    .iter()
                    .filter(|x| !matches!(&***x, Type::Null))
                    .cloned()
                    .collect::<Vec<_>>();
                if other.len() != 1 {
                    return Err(self.error("this type is not a runtime validation schema", span));
                }
                Norm::Maybe(Box::new(Norm::Ref(self.norm(&other[0], span)?)))
            }
            _ => {
                return Err(Diagnostic::new(
                    "check",
                    "type_reification",
                    "function types and erased type parameters are not runtime validation schemas",
                    span,
                ))
            }
        };
        let n = Rc::new(n);
        self.norm_cache.insert(key, (t, n.clone()));
        Ok(n)
    }
    fn schema_value(&mut self, t: &Ty, span: Span) -> Result<Value, Diagnostic> {
        Ok(Value::Schema(Rc::new(crate::schema::SchemaValue::Type(
            self.norm(t, span)?,
        ))))
    }
    fn read(&self, var: Var) -> Ty {
        let ctx = self.contexts.last().unwrap();
        match var {
            Var::Local(i) => ctx.locals[i as usize].clone(),
            Var::Capture(i) => ctx.captures[i as usize].clone(),
            Var::SelfFn => ctx.self_fn.clone(),
            Var::Prelude(i) => self.prelude[i as usize].clone(),
            Var::TypeAlias(i) => self.program.aliases[i as usize].ty.clone().unwrap(),
            Var::Unresolved => unreachable!(),
        }
    }
    fn sequence(&mut self, items: &[NodeId]) -> Result<Ty, Diagnostic> {
        self.sequence_context(items, None)
    }
    fn sequence_context(
        &mut self,
        items: &[NodeId],
        expected: Option<&Ty>,
    ) -> Result<Ty, Diagnostic> {
        let mut result = ty(Type::Null);
        for (index, &id) in items.iter().enumerate() {
            let span = self.program.span(id);
            result = match self.program.node(id).clone() {
                Node::TypeDecl { .. } => ty(Type::Null),
                Node::Bind {
                    slot,
                    e,
                    annotation,
                    ..
                } => {
                    let expected = annotation
                        .as_ref()
                        .map(|a| self.annotation(a))
                        .transpose()?;
                    let actual = self.expr(e, expected.as_ref())?;
                    let value = match expected {
                        Some(want) => {
                            let actual = self.instantiate_function(actual, None, span)?;
                            self.solver.require(&actual, &want, span)?;
                            want
                        }
                        None => actual,
                    };
                    self.contexts.last_mut().unwrap().locals[slot as usize] = value;
                    ty(Type::Null)
                }
                Node::Define(info) => {
                    let f = self.infer_function(id, None)?;
                    self.contexts.last_mut().unwrap().locals[info.def_slot as usize] = f;
                    ty(Type::Null)
                }
                _ => self.expr(
                    id,
                    if index + 1 == items.len() {
                        expected
                    } else {
                        None
                    },
                )?,
            };
            self.program.inferred_types[id as usize] = Some(result.clone());
        }
        Ok(result)
    }
    fn instantiate_function(
        &mut self,
        t: Ty,
        explicit: Option<Vec<Ty>>,
        span: Span,
    ) -> Result<Ty, Diagnostic> {
        let t = self.solver.resolve(&t);
        let Type::Function(f) = &*t else {
            if explicit.is_some() && !matches!(&*t, Type::Any) {
                return Err(self.error("type arguments require a generic function", span));
            }
            return Ok(t);
        };
        let explicitly_specialized = explicit.is_some();
        let args = match explicit {
            Some(xs) => {
                if xs.len() != f.generics.len() {
                    return Err(self.arity(f.generics.len(), xs.len(), span));
                }
                xs
            }
            None => f.generics.iter().map(|_| self.solver.fresh()).collect(),
        };
        if f.generics.is_empty() {
            return Ok(t);
        }
        let params = f.generics.iter().copied().zip(args).collect();
        let instance = substitute(&t, &params, &HashMap::new());
        let mut f = if let Type::Function(f) = &*instance {
            f.clone()
        } else {
            unreachable!()
        };
        f.generics.clear();
        if explicitly_specialized && f.builtin == Some(Builtin::Input) {
            f.min = 2;
        }
        Ok(ty(Type::Function(f)))
    }
    fn infer_function(&mut self, id: NodeId, contextual: Option<&Ty>) -> Result<Ty, Diagnostic> {
        let info = self.program.fn_info(id).clone();
        let span = self.program.span(id);
        let contextual = contextual.map(|t| self.solver.resolve(t));
        let expected = contextual.as_ref().and_then(|t| {
            if let Type::Function(f) = &**t {
                Some(f.clone())
            } else {
                None
            }
        });
        let mut params = Vec::new();
        for (index, annotation) in info.param_types.iter().enumerate() {
            let t = match annotation {
                Some(a) => self.annotation(a)?,
                None if info.name.is_none() => {
                    let t = expected
                        .as_ref()
                        .and_then(|f| f.params.get(index))
                        .cloned()
                        .or_else(|| {
                            contextual
                                .as_ref()
                                .filter(|t| matches!(&***t, Type::Any))
                                .cloned()
                        })
                        .ok_or_else(|| self.ambiguous(span))?;
                    if matches!(&*self.solver.resolve(&t), Type::Hole(_)) {
                        return Err(self.ambiguous(span));
                    }
                    t
                }
                None => {
                    return Err(Diagnostic::new(
                        "check",
                        "type_annotation_required",
                        format!("parameter `{}` needs a type annotation", info.params[index]),
                        span,
                    ))
                }
            };
            params.push(t);
        }
        let declared_result = info
            .result_type
            .as_ref()
            .map(|a| self.annotation(a))
            .transpose()?;
        let result_context = declared_result
            .clone()
            .or_else(|| expected.as_ref().map(|f| f.result.clone()));
        let provisional_result = result_context
            .clone()
            .unwrap_or_else(|| self.solver.fresh());
        let callback = if info.slot_name.is_some() {
            let Some(annotation) = &info.slot_type else {
                return Err(Diagnostic::new(
                    "check",
                    "type_annotation_required",
                    "a named callback slot needs a function type annotation",
                    span,
                ));
            };
            let t = self.annotation(annotation)?;
            if !matches!(&*t, Type::Function(_) | Type::Any) {
                return Err(self.error("callback slot requires a function type", span));
            }
            Some(t)
        } else {
            None
        };
        let signature = Function {
            params: params.clone(),
            min: params.len(),
            callback: callback.clone(),
            optional_callback: false,
            result: provisional_result,
            generics: info.type_params.iter().map(|p| p.id).collect(),
            builtin: None,
            receiver: None,
        };
        if info.captures.contains(&Var::SelfFn)
            && self.contexts.last().unwrap().needs_result_annotation
        {
            return Err(Diagnostic::new(
                "check",
                "type_annotation_required",
                "recursive functions need an explicit result type",
                span,
            ));
        }
        let captures = info.captures.iter().map(|v| self.read(*v)).collect();
        let mut locals = vec![ty(Type::Never); info.nlocals as usize];
        for (index, t) in params.iter().enumerate() {
            locals[index] = t.clone();
        }
        if let Some(cb) = callback {
            locals[params.len()] = cb;
        }
        self.contexts.push(Context {
            locals,
            captures,
            self_fn: ty(Type::Function(signature.clone())),
            needs_result_annotation: info.name.is_some() && declared_result.is_none(),
        });
        let actual = match &info.body {
            Body::Expr(e) => self.expr(*e, result_context.as_ref()),
            Body::Seq(items) => self.sequence_context(items, result_context.as_ref()),
        };
        self.contexts.pop();
        let actual = actual?;
        let result = if let Some(want) = result_context {
            self.solver.require(&actual, &want, span)?;
            if declared_result.is_some() {
                want
            } else {
                actual
            }
        } else {
            actual
        };
        let mut signature = signature;
        signature.result = result;
        let signature = self.solver.deep(&ty(Type::Function(signature)));
        let mut free = HashSet::new();
        self.solver.free(&signature, &mut free);
        if contextual.is_none() && !free.is_empty() {
            return Err(self.ambiguous(span));
        }
        Ok(signature)
    }
    fn expr(&mut self, id: NodeId, expected: Option<&Ty>) -> Result<Ty, Diagnostic> {
        let span = self.program.span(id);
        // An unsolved call result is not a concrete expected type. Infer each
        // branch/container first, then match the complete local result.
        let expected_type = expected
            .map(|t| self.solver.resolve(t))
            .filter(|t| !matches!(&**t, Type::Hole(_)));
        let expected = expected_type.as_ref();
        let t = match self.program.node(id).clone() {
            Node::Lit(v) => match v {
                Value::Null => ty(Type::Null),
                Value::Bool(_) => ty(Type::Bool),
                Value::Num(n) => ty(if n.is_integer() { Type::Int } else { Type::Num }),
                Value::Str(_) => ty(Type::Str),
                _ => unreachable!("source literals are scalar"),
            },
            Node::NumOverflow => ty(Type::Num),
            Node::Intrinsic(builtin) => self.builtin_type(builtin, None, span)?,
            Node::Name {
                var: Var::TypeAlias(index),
                ..
            } => {
                let alias = self.program.aliases[index as usize].clone();
                if !alias.params.is_empty() {
                    return Err(self.arity(alias.params.len(), 0, span));
                }
                let target = alias.ty.unwrap();
                let value = self.schema_value(&target, span)?;
                self.program.aliases[index as usize].runtime = Some(value);
                ty(Type::Schema(target))
            }
            Node::Name {
                var: Var::SelfFn, ..
            } if self.contexts.last().unwrap().needs_result_annotation => {
                return Err(Diagnostic::new(
                    "check",
                    "type_annotation_required",
                    "recursive functions need an explicit result type",
                    span,
                ))
            }
            Node::Name { var, .. } => self.read(var),
            Node::Instantiate { callee, args, .. } => {
                let args = args
                    .iter()
                    .map(|a| self.annotation(a))
                    .collect::<Result<Vec<_>, _>>()?;
                let root = self.program.strip_groups(callee);
                if let Node::Name {
                    var: Var::TypeAlias(index),
                    ..
                } = self.program.node(root)
                {
                    let target = self.instantiate_alias(*index, args, span)?;
                    let value = self.schema_value(&target, span)?;
                    if let Node::Instantiate { schema, .. } = &mut self.program.nodes[id as usize] {
                        *schema = Some(value);
                    }
                    ty(Type::Schema(target))
                } else {
                    let callee = self.expr(callee, None)?;
                    self.instantiate_function(callee, Some(args), span)?
                }
            }
            Node::TypeSchema { annotation, .. } => {
                let target = self.annotation(&annotation)?;
                let value = self.schema_value(&target, span)?;
                if let Node::TypeSchema { schema, .. } = &mut self.program.nodes[id as usize] {
                    *schema = Some(value);
                }
                ty(Type::Schema(target))
            }
            Node::Array(items) => {
                let expected_elem = expected.and_then(|t| match &*self.solver.resolve(t) {
                    Type::Array(e) => Some(e.clone()),
                    Type::Any => Some(t.clone()),
                    _ => None,
                });
                let mut elem = ty(Type::Never);
                for item in items {
                    elem = join(elem, self.expr(item, expected_elem.as_ref())?);
                }
                array(elem)
            }
            Node::Record(fields) => {
                let wanted = expected.map(|t| self.solver.resolve(t));
                let mut result = Vec::new();
                for (name, value) in fields {
                    let context = wanted.as_ref().and_then(|t| match &**t {
                        Type::Record { fields, rest } => fields
                            .iter()
                            .find(|f| f.name == name)
                            .map(|f| f.ty.clone())
                            .or_else(|| rest.clone()),
                        Type::Any => Some(t.clone()),
                        _ => None,
                    });
                    result.push(Field {
                        name,
                        ty: self.expr(value, context.as_ref())?,
                        optional: false,
                    });
                }
                ty(Type::Record {
                    fields: result,
                    rest: None,
                })
            }
            Node::Lambda(_) => self.infer_function(id, expected)?,
            Node::Call { callee, args, cb } => {
                let callee_type = self.expr(callee, None)?;
                self.call(callee_type, &args, cb, None, span, expected)?
            }
            Node::Pipe {
                left,
                head,
                args,
                cb,
                ..
            } => {
                let left = self.expr(left, None)?;
                let head = self.expr(head, None)?;
                self.call(head, &args, cb, Some(left), span, expected)?
            }
            Node::Member {
                recv,
                name,
                optional,
            } => {
                let recv = self.expr(recv, None)?;
                self.member(recv, &name, optional, span)?
            }
            Node::Get {
                recv,
                key,
                optional,
            } => {
                let recv = self.expr(recv, None)?;
                let key_type = self.expr(key, None)?;
                let key_type = self.solver.resolve(&key_type);
                if let Node::Lit(Value::Str(name)) =
                    self.program.node(self.program.strip_groups(key))
                {
                    self.field(recv, name.clone(), optional, span)?
                } else {
                    let recv = self.solver.resolve(&recv);
                    match &*recv {
                        Type::Any | Type::Never => recv.clone(),
                        Type::Null if optional => ty(Type::Null),
                        Type::Hole(i) if matches!(&*key_type, Type::Int | Type::Num) => {
                            let elem = self.solver.fresh();
                            self.solver.bind(*i, array(elem.clone()), span)?;
                            self.solver.require(&key_type, &ty(Type::Int), span)?;
                            if optional {
                                join(elem, ty(Type::Null))
                            } else {
                                elem
                            }
                        }
                        Type::Array(elem) => {
                            self.solver.require(&key_type, &ty(Type::Int), span)?;
                            if optional {
                                join(elem.clone(), ty(Type::Null))
                            } else {
                                elem.clone()
                            }
                        }
                        Type::Record { fields, rest } => {
                            self.solver.require(&key_type, &ty(Type::Str), span)?;
                            let t = fields
                                .iter()
                                .map(|f| f.ty.clone())
                                .chain(rest.clone())
                                .fold(ty(Type::Never), join);
                            if optional {
                                join(t, ty(Type::Null))
                            } else {
                                t
                            }
                        }
                        Type::Hole(_) => return Err(self.ambiguous(span)),
                        _ => {
                            return Err(self.error(
                                "indexing requires an array or record; check external JSON first",
                                span,
                            ))
                        }
                    }
                }
            }
            Node::Unary { op, e } => {
                let operand = self.expr(e, None)?;
                let concrete = self.solver.resolve(&operand);
                if matches!(&*concrete, Type::Any | Type::Never) {
                    concrete
                } else {
                    match op {
                        UnOp::Not => {
                            self.solver.require(&operand, &ty(Type::Bool), span)?;
                            ty(Type::Bool)
                        }
                        UnOp::Neg => {
                            self.solver.require(&operand, &ty(Type::Num), span)?;
                            self.solver.resolve(&operand)
                        }
                    }
                }
            }
            Node::Binary { op, l, r } => {
                let left = self.expr(l, None)?;
                let right = self.expr(r, None)?;
                self.binary(op, left, right, span)?
            }
            Node::If { c, t, e } => {
                let condition = self.expr(c, Some(&ty(Type::Bool)))?;
                self.solver
                    .require(&condition, &ty(Type::Bool), self.program.span(c))?;
                let saved_locals = self.contexts.last().unwrap().locals.clone();
                let saved_captures = self.contexts.last().unwrap().captures.clone();
                self.refine(c, true);
                let yes = self.expr(t, expected)?;
                self.contexts.last_mut().unwrap().locals = saved_locals.clone();
                self.contexts.last_mut().unwrap().captures = saved_captures.clone();
                self.refine(c, false);
                let no = self.expr(e, expected)?;
                self.contexts.last_mut().unwrap().locals = saved_locals;
                self.contexts.last_mut().unwrap().captures = saved_captures;
                join(yes, no)
            }
            Node::Do { items } => self.sequence_context(&items, expected)?,
            Node::Try { e, slot, items, .. } => {
                let protected = self.expr(e, expected)?;
                self.contexts.last_mut().unwrap().locals[slot as usize] = builtins::error();
                join(protected, self.sequence_context(&items, expected)?)
            }
            Node::Query { holes, .. } => {
                for h in holes {
                    self.expr(h, None)?;
                }
                ty(Type::Query)
            }
            Node::Group(e) => self.expr(e, expected)?,
            Node::Bind { .. } | Node::Define(_) | Node::TypeDecl { .. } => unreachable!(),
        };
        let t = if expected.is_some_and(|e| matches!(&*self.solver.resolve(e), Type::Function(_)))
            && matches!(&*t, Type::Function(f) if !f.generics.is_empty())
        {
            self.instantiate_function(t, None, span)?
        } else {
            t
        };
        self.program.inferred_types[id as usize] = Some(t.clone());
        Ok(t)
    }
    fn without_null(&self, t: &Ty) -> Ty {
        match &*self.solver.resolve(t) {
            Type::Null => ty(Type::Never),
            Type::Union(xs) => xs
                .iter()
                .filter(|t| !matches!(&***t, Type::Null))
                .cloned()
                .fold(ty(Type::Never), join),
            _ => t.clone(),
        }
    }
    fn refine(&mut self, condition: NodeId, truth: bool) {
        let Node::Binary {
            op: op @ (BinOp::Eq | BinOp::Ne),
            l,
            r,
        } = self.program.node(self.program.strip_groups(condition))
        else {
            return;
        };
        let (op, l, r) = (*op, *l, *r);
        let name = if matches!(
            self.program.node(self.program.strip_groups(r)),
            Node::Lit(Value::Null)
        ) {
            l
        } else if matches!(
            self.program.node(self.program.strip_groups(l)),
            Node::Lit(Value::Null)
        ) {
            r
        } else {
            return;
        };
        let Node::Name { var, .. } = self.program.node(self.program.strip_groups(name)) else {
            return;
        };
        let var = *var;
        let original = self.read(var);
        let refined = if truth == (op == BinOp::Eq) {
            ty(Type::Null)
        } else {
            self.without_null(&original)
        };
        let ctx = self.contexts.last_mut().unwrap();
        match var {
            Var::Local(i) => ctx.locals[i as usize] = refined,
            Var::Capture(i) => ctx.captures[i as usize] = refined,
            _ => (),
        }
    }
    fn field(
        &mut self,
        receiver: Ty,
        name: Rc<str>,
        optional: bool,
        span: Span,
    ) -> Result<Ty, Diagnostic> {
        let concrete = self.solver.resolve(&receiver);
        let result =
            match &*concrete {
                Type::Any | Type::Never => concrete.clone(),
                Type::Null if optional => return Ok(ty(Type::Null)),
                Type::Hole(_) => return Err(self.ambiguous(span)),
                Type::Record { fields, rest } => {
                    if let Some(field) = fields.iter().find(|f| f.name == name) {
                        field.ty.clone()
                    } else if let Some(rest) = rest {
                        rest.clone()
                    } else if optional {
                        return Ok(ty(Type::Null));
                    } else {
                        return Err(self.error(format!("record has no field `{name}`"), span));
                    }
                }
                Type::Union(xs) => {
                    let mut result = ty(Type::Never);
                    for x in xs {
                        result = join(result, self.field(x.clone(), name.clone(), optional, span)?);
                    }
                    return Ok(result);
                }
                _ => return Err(self.error(
                    "field access requires a known record type; validate JSON or add an annotation",
                    span,
                )),
            };
        Ok(if optional {
            join(result, ty(Type::Null))
        } else {
            result
        })
    }
    fn member(
        &mut self,
        receiver: Ty,
        name: &str,
        optional: bool,
        span: Span,
    ) -> Result<Ty, Diagnostic> {
        let concrete = self.solver.resolve(&receiver);
        match &*concrete {
            Type::Any | Type::Never => return Ok(concrete),
            Type::Null if optional => return Ok(ty(Type::Null)),
            Type::Union(xs) => {
                let mut result = ty(Type::Never);
                for x in xs {
                    result = join(result, self.member(x.clone(), name, optional, span)?);
                }
                return Ok(result);
            }
            _ => (),
        }
        if name == "size" {
            match &*concrete {
                Type::Array(_) | Type::Record { .. } | Type::Str | Type::Bytes => {
                    return Ok(ty(Type::Int))
                }
                Type::Hole(_) => return Err(self.ambiguous(span)),
                _ => {
                    return Err(self.error("size requires an array, record, string or bytes", span))
                }
            }
        }
        if let Some(method) = crate::methods::named(name) {
            let applicable = match &*concrete {
                Type::Array(_) => method.receivers.contains(&Kind::Array),
                Type::Record { .. } => method.receivers.contains(&Kind::Record),
                Type::Str => method.receivers.contains(&Kind::String),
                Type::Hole(_) => true,
                _ => false,
            };
            if applicable {
                return self.builtin_type(method.builtin, Some(receiver), span);
            }
        }
        self.field(receiver, Rc::from(name), optional, span)
    }
    fn binary(&mut self, op: BinOp, l: Ty, r: Ty, span: Span) -> Result<Ty, Diagnostic> {
        use BinOp::*;
        let a = self.solver.resolve(&l);
        let b = self.solver.resolve(&r);
        if op == Coalesce {
            return Ok(join(self.without_null(&a), b));
        }
        if matches!(&*a, Type::Never) || matches!(&*b, Type::Never) && !matches!(op, And | Or) {
            return Ok(ty(Type::Never));
        }
        if matches!(&*a, Type::Any) {
            return Ok(if matches!(op, Eq | Ne | Lt | Le | Gt | Ge | And | Or) {
                ty(Type::Bool)
            } else {
                a
            });
        }
        if matches!(&*b, Type::Any) {
            return Ok(if matches!(op, Eq | Ne | Lt | Le | Gt | Ge | And | Or) {
                ty(Type::Bool)
            } else {
                b
            });
        }
        match op {
            And | Or => {
                self.solver.require(&l, &ty(Type::Bool), span)?;
                self.solver.require(&r, &ty(Type::Bool), span)?;
                Ok(ty(Type::Bool))
            }
            Eq | Ne => {
                self.solver.require(&l, &ty(Type::Json), span)?;
                self.solver.require(&r, &ty(Type::Json), span)?;
                Ok(ty(Type::Bool))
            }
            Add if matches!(&*a, Type::Str) || matches!(&*b, Type::Str) => {
                self.solver.require(&l, &ty(Type::Str), span)?;
                self.solver.require(&r, &ty(Type::Str), span)?;
                Ok(ty(Type::Str))
            }
            Add if matches!(&*a, Type::Array(_)) || matches!(&*b, Type::Array(_)) => {
                let elem = self.solver.fresh();
                let arr = array(elem.clone());
                self.solver.require(&l, &arr, span)?;
                // Concatenation preserves heterogeneous element unions.
                let b = self.solver.resolve(&b);
                if let Type::Array(other) = &*b {
                    return Ok(array(join(self.solver.resolve(&elem), other.clone())));
                }
                self.solver.require(&r, &arr, span)?;
                Ok(arr)
            }
            Lt | Le | Gt | Ge if matches!(&*a, Type::Str) || matches!(&*b, Type::Str) => {
                self.solver.require(&l, &ty(Type::Str), span)?;
                self.solver.require(&r, &ty(Type::Str), span)?;
                Ok(ty(Type::Bool))
            }
            _ => {
                if matches!((&*a, &*b), (Type::Hole(_), Type::Hole(_)))
                    && matches!(op, Add | Lt | Le | Gt | Ge)
                {
                    return Err(self.ambiguous(span));
                }
                self.solver.require(&l, &ty(Type::Num), span)?;
                self.solver.require(&r, &ty(Type::Num), span)?;
                if matches!(op, Lt | Le | Gt | Ge) {
                    Ok(ty(Type::Bool))
                } else if op != Div
                    && matches!(
                        (&*self.solver.resolve(&a), &*self.solver.resolve(&b)),
                        (Type::Int, Type::Int)
                    )
                {
                    Ok(ty(Type::Int))
                } else {
                    Ok(ty(Type::Num))
                }
            }
        }
    }
    fn schema_description(
        &mut self,
        description: &Ty,
        allow_optional: bool,
        span: Span,
    ) -> Result<(Ty, bool), Diagnostic> {
        let description = self.solver.resolve(description);
        Ok(match &*description {
            Type::Schema(t) => (t.clone(), false),
            Type::OptionalSchema(t) if allow_optional => (t.clone(), true),
            Type::Any => (description.clone(), false),
            Type::Hole(id) => {
                let inner = self.solver.fresh();
                self.solver
                    .bind(*id, ty(Type::Schema(inner.clone())), span)?;
                (inner, false)
            }
            Type::Array(inner) => (array(self.schema_description(inner, false, span)?.0), false),
            Type::Record { fields, rest } => {
                let mut result = Vec::new();
                for field in fields {
                    let (target, optional) = self.schema_description(&field.ty, true, span)?;
                    result.push(Field {
                        name: field.name.clone(),
                        ty: target,
                        optional,
                    });
                }
                let rest = rest
                    .as_ref()
                    .map(|r| self.schema_description(r, false, span).map(|p| p.0))
                    .transpose()?;
                (
                    ty(Type::Record {
                        fields: result,
                        rest,
                    }),
                    false,
                )
            }
            Type::Union(xs) => {
                let mut target = ty(Type::Never);
                let mut optional = false;
                for x in xs {
                    let (t, o) = self.schema_description(x, allow_optional, span)?;
                    target = join(target, t);
                    optional |= o;
                }
                (target, optional)
            }
            _ => return Err(self.error(
                "expected a schema descriptor; optional is only valid directly on a record field",
                span,
            )),
        })
    }
    fn callback_context(&mut self, id: NodeId, dynamic: Option<&Ty>) -> Ty {
        let count = self.program.fn_info(id).params.len();
        let params = (0..count)
            .map(|_| dynamic.cloned().unwrap_or_else(|| self.solver.fresh()))
            .collect();
        let result = dynamic.cloned().unwrap_or_else(|| self.solver.fresh());
        function(params, result)
    }
    fn call(
        &mut self,
        callee: Ty,
        args: &[NodeId],
        callback: Option<NodeId>,
        prefix: Option<Ty>,
        span: Span,
        expected_result: Option<&Ty>,
    ) -> Result<Ty, Diagnostic> {
        let callee = self.solver.resolve(&callee);
        if let Type::Union(xs) = &*callee {
            let mut result = ty(Type::Never);
            for x in xs {
                result = join(
                    result,
                    self.call(
                        x.clone(),
                        args,
                        callback,
                        prefix.clone(),
                        span,
                        expected_result,
                    )?,
                );
            }
            return Ok(result);
        }
        if matches!(&*callee, Type::Never) {
            return Ok(callee);
        }
        if matches!(&*callee, Type::Any) {
            for &arg in args {
                self.expr(arg, Some(&callee))?;
            }
            if let Some(cb) = callback {
                let expected = self.callback_context(cb, Some(&callee));
                self.expr(cb, Some(&expected))?;
            }
            return Ok(callee);
        }
        if matches!(&*callee, Type::Hole(_)) {
            return Err(self.ambiguous(span));
        }
        let instance = self.instantiate_function(callee, None, span)?;
        let Type::Function(f) = &*instance else {
            return Err(self.error("value is not callable", span));
        };
        let f = f.clone();
        if let Some(expected) = expected_result {
            self.solver.require(&f.result, expected, span)?;
        }
        let count = args.len() + usize::from(prefix.is_some());
        let reason = if count < f.min || count > f.params.len() {
            Some("positional")
        } else if callback.is_none() && f.callback.is_some() && !f.optional_callback {
            Some("missing_callback")
        } else if callback.is_some() && f.callback.is_none() {
            Some("unexpected_callback")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(Diagnostic::new(
                "check",
                "arity",
                format!("call signature mismatch: {reason}"),
                span,
            )
            .with("reason", Value::str(reason))
            .with("min", Value::int(f.min as i64))
            .with("max", Value::int(f.params.len() as i64))
            .with("got", Value::int(count as i64)));
        }
        let mut actuals = Vec::new();
        let mut source_ids = Vec::new();
        if let Some(value) = prefix {
            actuals.push(value);
            source_ids.push(None);
        }
        for (index, &arg) in args.iter().enumerate() {
            let position = actuals.len();
            let want = &f.params[position];
            let actual = self.expr(arg, Some(want))?;
            let actual = self.instantiate_function(actual, None, self.program.span(arg))?;
            let processed = if matches!(
                f.builtin,
                Some(Builtin::Maybe | Builtin::Optional | Builtin::Open)
            ) || matches!(f.builtin, Some(Builtin::Validate | Builtin::Input))
                && position == 1
            {
                ty(Type::Schema(
                    self.schema_description(&actual, false, self.program.span(arg))?
                        .0,
                ))
            } else {
                actual.clone()
            };
            self.solver
                .require(&processed, want, self.program.span(arg))?;
            actuals.push(actual);
            source_ids.push(Some(args[index]));
        }
        // A pipe's injected argument is checked after its callee has been selected.
        if !source_ids.is_empty() && source_ids[0].is_none() {
            self.solver.require(&actuals[0], &f.params[0], span)?;
        }
        let cb_type = if let Some(cb) = callback {
            let want = f.callback.as_ref().unwrap();
            if let Type::Function(expected) = &*self.solver.resolve(want) {
                let count = self.program.fn_info(cb).params.len();
                if count < expected.min || count > expected.params.len() {
                    return Err(Diagnostic::new(
                        "check",
                        "arity",
                        "callback signature mismatch",
                        self.program.span(cb),
                    )
                    .with("reason", Value::str("callback_signature")));
                }
            }
            let actual = self.expr(cb, Some(want))?;
            let a = self.solver.resolve(&actual);
            let e = self.solver.resolve(want);
            if let (Type::Function(a), Type::Function(e)) = (&*a, &*e) {
                if a.min > e.min
                    || a.params.len() < e.params.len()
                    || a.callback.is_some() && !a.optional_callback
                {
                    return Err(Diagnostic::new(
                        "check",
                        "arity",
                        "callback signature mismatch",
                        self.program.span(cb),
                    )
                    .with("reason", Value::str("callback_signature")));
                }
            }
            self.solver.require(&actual, want, self.program.span(cb))?;
            Some(actual)
        } else {
            None
        };
        // Recheck earlier arguments after generic constraints have been solved.
        for (index, actual) in actuals.iter().enumerate() {
            let processed = if matches!(
                f.builtin,
                Some(Builtin::Maybe | Builtin::Optional | Builtin::Open)
            ) || matches!(f.builtin, Some(Builtin::Validate | Builtin::Input))
                && index == 1
            {
                ty(Type::Schema(
                    self.schema_description(actual, false, span)?.0,
                ))
            } else {
                actual.clone()
            };
            self.solver.require(&processed, &f.params[index], span)?;
        }
        let result = self.intrinsic_result(&f, &actuals, cb_type.as_ref(), span)?;
        let mut free = HashSet::new();
        self.solver.free(&result, &mut free);
        if !free.is_empty() {
            return Err(self.ambiguous(span));
        }
        Ok(self.solver.deep(&result))
    }
    fn intrinsic_result(
        &mut self,
        f: &Function,
        args: &[Ty],
        callback: Option<&Ty>,
        span: Span,
    ) -> Result<Ty, Diagnostic> {
        use Builtin::*;
        let receiver = f.receiver.as_ref().map(|r| self.solver.resolve(r));
        match f.builtin {
            Some(Validate) => return Ok(self.schema_description(&args[1], false, span)?.0),
            Some(Input) => {
                return Ok(if args.len() == 1 {
                    ty(Type::Json)
                } else {
                    self.schema_description(&args[1], false, span)?.0
                })
            }
            Some(Maybe | Optional | Open) => {
                let target = self.schema_description(&args[0], false, span)?.0;
                return Ok(match f.builtin.unwrap() {
                    Maybe => ty(Type::Schema(join(target, ty(Type::Null)))),
                    Optional => ty(Type::OptionalSchema(target)),
                    Open => {
                        let target = self.solver.resolve(&target);
                        match &*target {
                            Type::Record { fields, .. } => ty(Type::Schema(ty(Type::Record {
                                fields: fields.clone(),
                                rest: Some(ty(Type::Json)),
                            }))),
                            Type::Any => ty(Type::Schema(target)),
                            _ => return Err(self.error("open requires a record schema", span)),
                        }
                    }
                    _ => unreachable!(),
                });
            }
            Some(Sort | Unique) if callback.is_none() => {
                if let Some(receiver) = &receiver {
                    if let Type::Array(e) = &**receiver {
                        let expected = if f.builtin == Some(Sort) {
                            join(ty(Type::Num), ty(Type::Str))
                        } else {
                            ty(Type::Json)
                        };
                        self.solver.require(e, &expected, span)?;
                    }
                }
            }
            Some(Sum) => {
                if let Some(receiver) = &receiver {
                    if let Type::Array(e) = &**receiver {
                        if matches!(&*self.solver.resolve(e), Type::Int | Type::Never) {
                            return Ok(ty(Type::Int));
                        }
                    }
                }
            }
            Some(Group) => {
                if let (Some(receiver), Some(callback)) = (&receiver, callback) {
                    if let (Type::Array(e), Type::Function(cb)) =
                        (&**receiver, &*self.solver.resolve(callback))
                    {
                        return Ok(array(record(vec![
                            ("key", self.solver.resolve(&cb.result), false),
                            ("items", array(e.clone()), false),
                        ])));
                    }
                }
            }
            Some(Merge) => {
                if let (Some(receiver), Some(other)) = (&receiver, args.first()) {
                    let other = self.solver.resolve(other);
                    if let (
                        Type::Record {
                            fields: a,
                            rest: ar,
                        },
                        Type::Record {
                            fields: b,
                            rest: br,
                        },
                    ) = (&**receiver, &*other)
                    {
                        let mut fields = a.clone();
                        for field in b {
                            if let Some(position) = fields.iter().position(|f| f.name == field.name)
                            {
                                fields[position] = field.clone();
                            } else {
                                fields.push(field.clone());
                            }
                        }
                        let rest = match (ar, br) {
                            (Some(a), Some(b)) => Some(join(a.clone(), b.clone())),
                            (a, b) => a.clone().or_else(|| b.clone()),
                        };
                        return Ok(ty(Type::Record { fields, rest }));
                    }
                }
            }
            Some(Pick) => {
                if let Some(receiver) = &receiver {
                    if let Type::Record { fields, rest } = &**receiver {
                        let value = fields
                            .iter()
                            .map(|f| f.ty.clone())
                            .chain(rest.clone())
                            .fold(ty(Type::Never), join);
                        return Ok(dict(value));
                    }
                }
            }
            _ => (),
        }
        Ok(self.solver.resolve(&f.result))
    }
}
