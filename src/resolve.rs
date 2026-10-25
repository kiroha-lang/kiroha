//! Static resolution and lexical scope (spec §4), with closure conversion.

use crate::ast::{Body, Node, NodeId, Program, Var};
use crate::diag::{Diagnostic, Span};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy)]
enum Sym {
    Local(u32),
    SelfFn,
}

struct Scope {
    names: HashMap<Rc<str>, Sym>,
}

/// One function activation context (program top level, lambda, or definition).
struct Ctx {
    scopes: Vec<Scope>,
    next_slot: u32,
    captures: Vec<Var>,
    /// key: (outer var) → capture index
    capture_index: HashMap<VarKey, u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum VarKey {
    Local(u32),
    Capture(u32),
    SelfFn,
}

impl Ctx {
    fn new() -> Ctx {
        Ctx { scopes: vec![Scope { names: HashMap::new() }], next_slot: 0, captures: Vec::new(), capture_index: HashMap::new() }
    }

    fn alloc(&mut self) -> u32 {
        let s = self.next_slot;
        self.next_slot += 1;
        s
    }
}

struct Resolver<'a> {
    prog: &'a mut Program,
    ctxs: Vec<Ctx>,
    prelude: &'a dyn Fn(&str) -> Option<u32>,
}

fn name_error(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new("check", "name", format!("unresolved name `{name}`"), span)
        .with("name", crate::value::Value::str(name))
}

fn dup_error(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new("check", "duplicate_binding", format!("`{name}` is already declared in this scope"), span)
        .with("name", crate::value::Value::str(name))
}

/// Resolve all names in place. `prelude` maps prelude names to table indices.
pub fn resolve(prog: &mut Program, prelude: &dyn Fn(&str) -> Option<u32>) -> Result<(), Diagnostic> {
    let items = prog.items.clone();
    let mut r = Resolver { prog, ctxs: vec![Ctx::new()], prelude };
    r.sequence(&items)?;
    let ctx = r.ctxs.pop().unwrap();
    r.prog.nlocals = ctx.next_slot;
    Ok(())
}

impl<'a> Resolver<'a> {
    fn ctx(&mut self) -> &mut Ctx {
        self.ctxs.last_mut().unwrap()
    }

    fn declare(&mut self, name: &Rc<str>, sym: Sym, span: Span) -> Result<(), Diagnostic> {
        let scope = self.ctx().scopes.last_mut().unwrap();
        if scope.names.contains_key(name) {
            return Err(dup_error(name, span));
        }
        scope.names.insert(name.clone(), sym);
        Ok(())
    }

    fn push_scope(&mut self) {
        self.ctx().scopes.push(Scope { names: HashMap::new() });
    }

    fn pop_scope(&mut self) {
        self.ctx().scopes.pop();
    }

    /// Find `name` as seen from context `depth`.
    fn lookup(&mut self, depth: usize, name: &str) -> Option<Var> {
        let found = self.ctxs[depth].scopes.iter().rev().find_map(|s| s.names.get(name).copied());
        if let Some(sym) = found {
            return Some(match sym {
                Sym::Local(s) => Var::Local(s),
                Sym::SelfFn => Var::SelfFn,
            });
        }
        if depth == 0 {
            return (self.prelude)(name).map(Var::Prelude);
        }
        let outer = self.lookup(depth - 1, name)?;
        let key = match outer {
            Var::Prelude(_) => return Some(outer),
            Var::Local(s) => VarKey::Local(s),
            Var::Capture(c) => VarKey::Capture(c),
            Var::SelfFn => VarKey::SelfFn,
            Var::Unresolved => unreachable!(),
        };
        let ctx = &mut self.ctxs[depth];
        let idx = match ctx.capture_index.get(&key) {
            Some(i) => *i,
            None => {
                let i = ctx.captures.len() as u32;
                ctx.captures.push(outer);
                ctx.capture_index.insert(key, i);
                i
            }
        };
        Some(Var::Capture(idx))
    }

    fn sequence(&mut self, items: &[NodeId]) -> Result<(), Diagnostic> {
        for &id in items {
            match self.prog.node(id).clone() {
                Node::Bind { name, e, .. } => {
                    self.expr(e)?;
                    let slot = self.ctx().alloc();
                    let span = self.prog.span(id);
                    self.declare(&name, Sym::Local(slot), span)?;
                    if let Node::Bind { slot: s, .. } = &mut self.prog.nodes[id as usize] {
                        *s = slot;
                    }
                }
                Node::Define(info) => {
                    let name = info.name.clone().unwrap();
                    let slot = self.ctx().alloc();
                    let span = self.prog.span(id);
                    self.declare(&name, Sym::Local(slot), span)?;
                    self.function(id, Some(name), slot)?;
                }
                _ => self.expr(id)?,
            }
        }
        Ok(())
    }

    /// Resolve a lambda or definition body in a fresh context.
    fn function(&mut self, id: NodeId, self_name: Option<Rc<str>>, def_slot: u32) -> Result<(), Diagnostic> {
        let info = self.prog.fn_info(id).clone();
        let span = self.prog.span(id);
        self.ctxs.push(Ctx::new());
        if let Some(n) = &self_name {
            self.declare(n, Sym::SelfFn, span)?;
            self.push_scope();
        }
        for p in &info.params {
            let slot = self.ctx().alloc();
            self.declare(p, Sym::Local(slot), span)?;
        }
        if let Some(s) = &info.slot_name {
            let slot = self.ctx().alloc();
            self.declare(s, Sym::Local(slot), span)?;
        }
        let res = match &info.body {
            Body::Expr(e) => self.expr(*e),
            Body::Seq(items) => self.sequence(items),
        };
        let ctx = self.ctxs.pop().unwrap();
        res?;
        match &mut self.prog.nodes[id as usize] {
            Node::Lambda(f) | Node::Define(f) => {
                f.nlocals = ctx.next_slot;
                f.captures = ctx.captures;
                f.def_slot = def_slot;
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn expr(&mut self, id: NodeId) -> Result<(), Diagnostic> {
        let node = self.prog.node(id).clone();
        match node {
            Node::Lit(_) | Node::NumOverflow => Ok(()),
            Node::Array(es) => {
                for e in es {
                    self.expr(e)?;
                }
                Ok(())
            }
            Node::Record(fs) => {
                for (_, e) in fs {
                    self.expr(e)?;
                }
                Ok(())
            }
            Node::Name { name, .. } => {
                let depth = self.ctxs.len() - 1;
                match self.lookup(depth, &name) {
                    Some(v) => {
                        if let Node::Name { var, .. } = &mut self.prog.nodes[id as usize] {
                            *var = v;
                        }
                        Ok(())
                    }
                    None => Err(name_error(&name, self.prog.span(id))),
                }
            }
            Node::Lambda(_) => self.function(id, None, 0),
            Node::Call { callee, args, cb } => {
                self.expr(callee)?;
                for a in args {
                    self.expr(a)?;
                }
                if let Some(c) = cb {
                    self.expr(c)?;
                }
                Ok(())
            }
            Node::Pipe { left, head, args, cb, .. } => {
                self.expr(left)?;
                self.expr(head)?;
                for a in args {
                    self.expr(a)?;
                }
                if let Some(c) = cb {
                    self.expr(c)?;
                }
                Ok(())
            }
            Node::Get { recv, key, .. } => {
                self.expr(recv)?;
                self.expr(key)
            }
            Node::Unary { e, .. } => self.expr(e),
            Node::Binary { l, r, .. } => {
                self.expr(l)?;
                self.expr(r)
            }
            Node::If { c, t, e } => {
                self.expr(c)?;
                self.expr(t)?;
                self.expr(e)
            }
            Node::Do { items } => {
                self.push_scope();
                let r = self.sequence(&items);
                self.pop_scope();
                r
            }
            Node::Try { e, name, items, .. } => {
                self.expr(e)?;
                self.push_scope();
                let slot = self.ctx().alloc();
                let span = self.prog.span(id);
                let r = self.declare(&name, Sym::Local(slot), span).and_then(|_| self.sequence(&items));
                self.pop_scope();
                if let Node::Try { slot: s, .. } = &mut self.prog.nodes[id as usize] {
                    *s = slot;
                }
                r
            }
            Node::Query { holes, .. } => {
                for h in holes {
                    self.expr(h)?;
                }
                Ok(())
            }
            Node::Group(e) => self.expr(e),
            Node::Bind { .. } | Node::Define(_) => unreachable!("items only appear in sequences"),
        }
    }
}
