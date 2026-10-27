//! The explicit-continuation evaluator for the core language (spec §5).

use super::frames::Frame;
use super::*;
use crate::ast::{BinOp, Body, Node, UnOp, Var};
use crate::value::{err, json_equal, CallbackPolicy, Closure, Kind, Record};

impl<H: Host> Runtime<H> {
    /// Raise a fresh runtime error at the task's current node.
    pub fn raise(&self, t: &mut Task, e: Value) -> Ctl {
        t.throw_span = Some(self.prog.span(t.cur_node));
        Ctl::Throw(e)
    }

    pub fn fail_ctl(&mut self, t: &mut Task, f: Fail) -> Ctl {
        if let Fail::Abort(_) = &f {
            if let Some(sig) = self.abort_of(t.scope) {
                return Ctl::Abort(sig, None);
            }
        }
        match f {
            Fail::Interrupted => Ctl::Ret(Value::Null),
            Fail::Err(e) => self.raise(t, e),
            Fail::Abort(reason) => {
                let scope = self.budgets.get(&t.budget).map(|b| b.scope).unwrap_or(self.root_scope);
                self.abort_scope(scope, reason);
                match self.abort_of(t.scope) {
                    Some(sig) => Ctl::Abort(sig, None),
                    None => Ctl::Ret(Value::Null),
                }
            }
        }
    }

    /// Abort observed for the task, if any (cached by epoch).
    pub fn task_abort(&mut self, t: &mut Task) -> Option<AbortSig> {
        if t.seen_epoch == self.abort_epoch {
            return None;
        }
        match self.abort_of(t.scope) {
            None => {
                t.seen_epoch = self.abort_epoch;
                None
            }
            Some(sig) => Some(sig),
        }
    }

    fn checkpoint(&mut self, t: &mut Task) -> Option<Ctl> {
        self.task_abort(t).map(|sig| Ctl::Abort(sig, None))
    }

    /// Charge one semantic step to the task's budget.
    pub fn charge(&mut self, t: &mut Task) {
        let virt = self.host.is_virtual();
        let b = match self.budgets.get_mut(&t.budget) {
            Some(b) => b,
            None => return,
        };
        b.steps += 1;
        let steps = b.steps;
        let scope = b.scope;
        if steps > b.limit {
            self.abort_scope(scope, AbortReason::Resource("eval_steps"));
            return;
        }
        if b.abort_after == Some(steps) {
            let root = self.root_scope;
            self.abort_scope(root, AbortReason::Cancelled);
        }
        if steps.is_multiple_of(512) {
            if self.memory_exceeded() {
                self.abort_scope(scope, AbortReason::Resource("memory"));
            }
            if !virt && steps.is_multiple_of(1024) {
                let now = self.host.now_ms();
                self.fire_due_timers(now);
                if crate::interrupt::cancel_requested() {
                    let root = self.root_scope;
                    self.abort_scope(root, AbortReason::Cancelled);
                }
            }
        }
    }

    pub fn step(&mut self, t: &mut Task) -> StepResult {
        let ctl = std::mem::replace(&mut t.ctl, Ctl::Waiting);
        if matches!(ctl, Ctl::Eval(..) | Ctl::Seq(..) | Ctl::Apply(..)) {
            t.cleanup_deadline = None;
        }
        let next = match ctl {
            Ctl::Eval(id, env) => match self.checkpoint(t) {
                Some(c) => c,
                None => self.eval(t, id, env),
            },
            Ctl::Seq(owner, idx, env) => match self.checkpoint(t) {
                Some(c) => c,
                None => self.seq(t, owner, idx, env),
            },
            Ctl::Apply(f, args, cb) => match self.checkpoint(t) {
                Some(c) => c,
                None => self.apply(t, f, args, cb),
            },
            Ctl::Ret(v) => match t.stack.pop() {
                None => {
                    if let Some(c) = self.deadline_due(t) {
                        t.ctl = c;
                        return StepResult::Continue;
                    }
                    return StepResult::Done(TaskOutcome::Value(v));
                }
                Some(fr @ (Frame::BinRight { .. } | Frame::Collect(_) | Frame::Pages(_) | Frame::Request(_))) => {
                    if let Some(c) = self.deadline_due(t) {
                        t.ctl = c;
                        return StepResult::Continue;
                    }
                    self.arm_interrupts(t);
                    let ctl = self.frame_ret(t, fr, v);
                    self.disarm_interrupts(t).unwrap_or(ctl)
                }
                Some(fr) => self.frame_ret(t, fr, v),
            },
            Ctl::Throw(e) => match t.stack.pop() {
                None => {
                    if let Some(c) = self.deadline_due(t) {
                        t.ctl = c;
                        return StepResult::Continue;
                    }
                    return StepResult::Done(TaskOutcome::Error(e, t.throw_span));
                }
                Some(fr) => self.frame_throw(t, fr, e),
            },
            Ctl::Effect(o) => {
                let cleanup = t.in_cleanup;
                t.in_cleanup = false;
                if !cleanup {
                    if let Some(sig) = self.task_abort(t) {
                        t.ctl = Ctl::Abort(sig, Some(o));
                        return StepResult::Continue;
                    }
                }
                match t.stack.pop() {
                    Some(fr) => {
                        self.arm_interrupts(t);
                        let ctl = self.frame_effect(t, fr, o);
                        self.disarm_interrupts(t).unwrap_or(ctl)
                    }
                    None => {
                        self.harness_failure = Some("effect completion without a waiting frame".into());
                        return StepResult::Suspended;
                    }
                }
            }
            Ctl::Abort(sig, carry) => {
                // An ancestor abort observed later dominates.
                let sig = match self.abort_of(t.scope) {
                    Some(s) if s.scope != sig.scope && self.is_ancestor_or_self(s.scope, sig.scope) => s,
                    _ => sig,
                };
                t.unwinding = Some(sig.clone());
                match t.stack.pop() {
                    None => return StepResult::Done(TaskOutcome::Abort(sig)),
                    Some(fr) => self.frame_abort(t, fr, sig, carry),
                }
            }
            Ctl::Dispatch(eff, opts) => {
                if !opts.cleanup {
                    if let Some(sig) = self.task_abort(t) {
                        t.ctl = Ctl::Abort(sig, None);
                        return StepResult::Continue;
                    }
                }
                if let Some(ctl) = self.dispatch(t, eff, opts) {
                    t.ctl = ctl;
                    return StepResult::Continue;
                }
                t.ctl = Ctl::Waiting;
                return StepResult::Suspended;
            }
            Ctl::WaitGroup(_) | Ctl::Waiting => {
                t.ctl = Ctl::Waiting;
                return StepResult::Suspended;
            }
        };
        t.ctl = next;
        StepResult::Continue
    }

    pub fn read_var(&self, var: Var, env: &Env) -> Value {
        match var {
            Var::Local(s) => env.locals.borrow()[s as usize].clone(),
            Var::Capture(i) => match env.func.as_deref() {
                Some(Function::Closure(c)) => c.captures[i as usize].clone(),
                _ => unreachable!("capture outside closure"),
            },
            Var::SelfFn => Value::Func(env.func.clone().expect("self outside function")),
            Var::Prelude(i) => self.prelude[i as usize].clone(),
            Var::TypeAlias(i) => self.prog.aliases[i as usize].runtime.clone().expect("type value checked before evaluation"),
            Var::Unresolved => unreachable!("unresolved name at runtime"),
        }
    }

    pub fn make_closure(&self, node: NodeId, env: &Env) -> Value {
        let info = self.prog.fn_info(node);
        let captures = info.captures.iter().map(|v| self.read_var(*v, env)).collect();
        Value::Func(Rc::new(Function::Closure(Closure {
            node,
            captures,
            nparams: info.params.len(),
            has_slot: info.slot_name.is_some(),
            named: info.name.is_some(),
        })))
    }

    fn seq_items(prog: &Program, owner: SeqOwner) -> &[NodeId] {
        match owner {
            SeqOwner::Program => &prog.items,
            SeqOwner::Node(id) => match prog.node(id) {
                Node::Do { items } | Node::Try { items, .. } => items,
                Node::Lambda(f) | Node::Define(f) => match &f.body {
                    Body::Seq(items) => items,
                    Body::Expr(_) => unreachable!(),
                },
                _ => unreachable!(),
            },
        }
    }

    fn seq(&mut self, t: &mut Task, owner: SeqOwner, idx: usize, env: Rc<Env>) -> Ctl {
        let prog = self.prog.clone();
        let items = Self::seq_items(&prog, owner);
        if idx >= items.len() {
            return Ctl::Ret(Value::Null);
        }
        let item = items[idx];
        match prog.node(item) {
            Node::Bind { slot, e, .. } => {
                t.cur_node = item;
                self.charge(t);
                if let Some(c) = self.checkpoint(t) {
                    return c;
                }
                t.stack.push(Frame::SeqBind { owner, idx, slot: *slot, env: env.clone() });
                Ctl::Eval(*e, env)
            }
            Node::TypeDecl { .. } => {
                t.cur_node = item;
                self.charge(t);
                if let Some(c) = self.checkpoint(t) { return c; }
                Ctl::Seq(owner, idx + 1, env)
            }
            Node::Define(info) => {
                t.cur_node = item;
                self.charge(t);
                if let Some(c) = self.checkpoint(t) {
                    return c;
                }
                let f = self.make_closure(item, &env);
                env.set(info.def_slot, f);
                Ctl::Seq(owner, idx + 1, env)
            }
            _ => {
                if idx + 1 == items.len() {
                    Ctl::Eval(item, env)
                } else {
                    t.stack.push(Frame::SeqExpr { owner, idx, env: env.clone() });
                    Ctl::Eval(item, env)
                }
            }
        }
    }

    fn eval(&mut self, t: &mut Task, id: NodeId, env: Rc<Env>) -> Ctl {
        let prog = self.prog.clone();
        let node = prog.node(id);
        if let Node::Group(inner) = node {
            return Ctl::Eval(*inner, env);
        }
        t.cur_node = id;
        self.charge(t);
        if let Some(c) = self.checkpoint(t) {
            return c;
        }
        match node {
            Node::Lit(v) => Ctl::Ret(v.clone()),
            Node::Intrinsic(builtin) => Ctl::Ret(Value::Func(Rc::new(Function::Builtin(*builtin)))),
            Node::Instantiate { callee, schema, .. } => match schema {
                Some(value) => Ctl::Ret(value.clone()), None => Ctl::Eval(*callee, env),
            },
            Node::TypeSchema { schema, .. } => Ctl::Ret(schema.as_ref().expect("type-checked validation schema").clone()),
            Node::NumOverflow => self.fail_ctl(t, Fail::Abort(AbortReason::Resource("number"))),
            Node::Name { var, .. } => Ctl::Ret(self.read_var(*var, &env)),
            Node::Array(elems) => {
                if elems.is_empty() {
                    return Ctl::Ret(Value::array(Vec::new()));
                }
                let first = elems[0];
                t.stack.push(Frame::Array { node: id, acc: Vec::with_capacity(elems.len()), env: env.clone() });
                Ctl::Eval(first, env)
            }
            Node::Record(fields) => {
                if fields.is_empty() {
                    return Ctl::Ret(Value::record(Record::new()));
                }
                let first = fields[0].1;
                self.charge(t); // field key as a Literal (§14.1)
                t.stack.push(Frame::Record { node: id, acc: Record::with_capacity(fields.len()), env: env.clone() });
                Ctl::Eval(first, env)
            }
            Node::Lambda(_) => Ctl::Ret(self.make_closure(id, &env)),
            Node::Call { callee, .. } => {
                t.stack.push(Frame::CallCallee { node: id, env: env.clone() });
                Ctl::Eval(*callee, env)
            }
            Node::Pipe { left, .. } => {
                t.stack.push(Frame::PipeLeft { node: id, env: env.clone() });
                Ctl::Eval(*left, env)
            }
            Node::Get { recv, .. } => {
                t.stack.push(Frame::GetRecv { node: id, env: env.clone() });
                Ctl::Eval(*recv, env)
            }
            Node::Member { recv, .. } => {
                t.stack.push(Frame::Member { node: id });
                Ctl::Eval(*recv, env)
            }
            Node::Unary { e, .. } => {
                t.stack.push(Frame::Unary { node: id });
                Ctl::Eval(*e, env)
            }
            Node::Binary { l, .. } => {
                t.stack.push(Frame::BinLeft { node: id, env: env.clone() });
                Ctl::Eval(*l, env)
            }
            Node::If { c, .. } => {
                t.stack.push(Frame::If { node: id, env: env.clone() });
                Ctl::Eval(*c, env)
            }
            Node::Do { .. } => Ctl::Seq(SeqOwner::Node(id), 0, env),
            Node::Try { e, .. } => {
                t.stack.push(Frame::Try { node: id, env: env.clone() });
                Ctl::Eval(*e, env)
            }
            Node::Query { segments, holes } => {
                if holes.is_empty() {
                    return Ctl::Ret(Value::Query(Rc::new(crate::value::Query {
                        segments: (**segments).clone(),
                        values: Vec::new(),
                    })));
                }
                let first = holes[0];
                t.stack.push(Frame::Query { node: id, vals: Vec::new(), env: env.clone() });
                Ctl::Eval(first, env)
            }
            Node::Group(_) => unreachable!(),
            Node::Bind { .. } | Node::Define(_) | Node::TypeDecl { .. } => unreachable!("items are evaluated by seq"),
        }
    }

    /// Call protocol (§5.2 steps 4–5).
    pub fn apply(&mut self, t: &mut Task, f: Value, args: Vec<Value>, cb: Option<Value>) -> Ctl {
        let func = match &f {
            Value::Func(rc) => rc.clone(),
            other => {
                let e = err::type_error("call", err::Pos::Index(0), "function", other.kind());
                return self.raise(t, e);
            }
        };
        let sig = func.signature();
        let opname: String = match &*func {
            Function::Builtin(b) => b.name().to_string(),
            Function::BoundMethod { name, .. } => name.to_string(),
            Function::Closure(c) => {
                self.prog.fn_info(c.node).name.as_deref().map(|s| s.to_string()).unwrap_or_else(|| "function".into())
            }
        };
        if args.len() < sig.min || args.len() > sig.max {
            let e = err::arity(&opname, sig, args.len(), "positional");
            return self.raise(t, e);
        }
        match (sig.callback, cb.is_some()) {
            (CallbackPolicy::Required, false) => {
                let e = err::arity(&opname, sig, args.len(), "missing_callback");
                return self.raise(t, e);
            }
            (CallbackPolicy::Absent, true) => {
                let e = err::arity(&opname, sig, args.len(), "unexpected_callback");
                return self.raise(t, e);
            }
            _ => {}
        }
        match &*func {
            Function::BoundMethod { receiver, target, .. } => {
                let mut full_args = Vec::with_capacity(args.len() + 1);
                full_args.push(receiver.clone());
                full_args.extend(args);
                Ctl::Apply(Value::Func(target.clone()), full_args, cb)
            }
            Function::Closure(c) => {
                let info = self.prog.fn_info(c.node);
                let mut locals = vec![Value::Null; info.nlocals as usize];
                let n = args.len();
                for (i, a) in args.into_iter().enumerate() {
                    locals[i] = a;
                }
                if let Some(cbv) = cb {
                    locals[n] = cbv;
                }
                let env = Rc::new(Env { locals: RefCell::new(locals), func: Some(func.clone()) });
                match &info.body {
                    Body::Expr(e) => Ctl::Eval(*e, env),
                    Body::Seq(_) => Ctl::Seq(SeqOwner::Node(c.node), 0, env),
                }
            }
            Function::Builtin(b) => {
                if let Some(c) = self.deadline_due(t) {
                    return c;
                }
                self.arm_interrupts(t);
                let ctl = self.call_builtin(t, *b, args, cb);
                self.disarm_interrupts(t).unwrap_or(ctl)
            }
        }
    }

    fn finish_call(&mut self, t: &mut Task, node: NodeId, f: Value, args: Vec<Value>, env: &Rc<Env>) -> Ctl {
        let prog = self.prog.clone();
        let cbnode = match prog.node(node) {
            Node::Call { cb, .. } | Node::Pipe { cb, .. } => *cb,
            _ => unreachable!(),
        };
        let cb = cbnode.map(|c| {
            t.cur_node = c;
            self.charge(t);
            self.make_closure(c, env)
        });
        t.cur_node = node;
        Ctl::Apply(f, args, cb)
    }

    pub fn frame_ret(&mut self, t: &mut Task, fr: Frame, v: Value) -> Ctl {
        let prog = self.prog.clone();
        match fr {
            Frame::SeqBind { owner, idx, slot, env } => {
                env.set(slot, v);
                Ctl::Seq(owner, idx + 1, env)
            }
            Frame::SeqExpr { owner, idx, env } => Ctl::Seq(owner, idx + 1, env),
            Frame::Array { node, mut acc, env } => {
                acc.push(v);
                let elems = match prog.node(node) {
                    Node::Array(e) => e,
                    _ => unreachable!(),
                };
                if acc.len() < elems.len() {
                    let next = elems[acc.len()];
                    t.stack.push(Frame::Array { node, acc, env: env.clone() });
                    Ctl::Eval(next, env)
                } else {
                    Ctl::Ret(Value::array(acc))
                }
            }
            Frame::Record { node, mut acc, env } => {
                let fields = match prog.node(node) {
                    Node::Record(f) => f,
                    _ => unreachable!(),
                };
                let k = fields[acc.len()].0.clone();
                acc.insert(k, v);
                if acc.len() < fields.len() {
                    let next = fields[acc.len()].1;
                    self.charge(t); // field key as a Literal (§14.1)
                    t.stack.push(Frame::Record { node, acc, env: env.clone() });
                    Ctl::Eval(next, env)
                } else {
                    Ctl::Ret(Value::record(acc))
                }
            }
            Frame::CallCallee { node, env } => {
                let args = match prog.node(node) {
                    Node::Call { args, .. } => args,
                    _ => unreachable!(),
                };
                if args.is_empty() {
                    self.finish_call(t, node, v, Vec::new(), &env)
                } else {
                    let first = args[0];
                    t.stack.push(Frame::CallArgs { node, f: v, args: Vec::with_capacity(args.len()), env: env.clone() });
                    Ctl::Eval(first, env)
                }
            }
            Frame::CallArgs { node, f, mut args, env } => {
                args.push(v);
                let anodes = match prog.node(node) {
                    Node::Call { args, .. } => args,
                    _ => unreachable!(),
                };
                if args.len() < anodes.len() {
                    let next = anodes[args.len()];
                    t.stack.push(Frame::CallArgs { node, f, args, env: env.clone() });
                    Ctl::Eval(next, env)
                } else {
                    self.finish_call(t, node, f, args, &env)
                }
            }
            Frame::PipeLeft { node, env } => {
                let head = match prog.node(node) {
                    Node::Pipe { head, .. } => *head,
                    _ => unreachable!(),
                };
                t.stack.push(Frame::PipeHead { node, lhs: v, env: env.clone() });
                Ctl::Eval(head, env)
            }
            Frame::PipeHead { node, lhs, env } => {
                let (fields, anodes) = match prog.node(node) {
                    Node::Pipe { fields, args, .. } => (fields, args),
                    _ => unreachable!(),
                };
                let mut f = v;
                for field in fields.iter() {
                    t.cur_node = node;
                    self.charge(t);
                    self.charge(t);
                    let next = match &f {
                        Value::Record(r) => match r.get(field) {
                            Some(x) => x.clone(),
                            None => {
                                let e = err::make(
                                    "missing",
                                    format!("missing key {field:?}"),
                                    vec![("key", Value::Str(field.clone()))],
                                );
                                return self.raise(t, e);
                            }
                        },
                        other => {
                            let e = err::type_error("access", err::Pos::Index(0), "record", other.kind());
                            return self.raise(t, e);
                        }
                    };
                    f = next;
                }
                let mut args = Vec::with_capacity(anodes.len() + 1);
                args.push(lhs);
                if anodes.is_empty() {
                    self.finish_call(t, node, f, args, &env)
                } else {
                    let first = anodes[0];
                    t.stack.push(Frame::PipeArgs { node, f, args, env: env.clone() });
                    Ctl::Eval(first, env)
                }
            }
            Frame::PipeArgs { node, f, mut args, env } => {
                args.push(v);
                let anodes = match prog.node(node) {
                    Node::Pipe { args, .. } => args,
                    _ => unreachable!(),
                };
                if args.len() - 1 < anodes.len() {
                    let next = anodes[args.len() - 1];
                    t.stack.push(Frame::PipeArgs { node, f, args, env: env.clone() });
                    Ctl::Eval(next, env)
                } else {
                    self.finish_call(t, node, f, args, &env)
                }
            }
            Frame::GetRecv { node, env } => {
                let (key, optional) = match prog.node(node) {
                    Node::Get { key, optional, .. } => (*key, *optional),
                    _ => unreachable!(),
                };
                if optional && matches!(v, Value::Null) {
                    return Ctl::Ret(Value::Null);
                }
                t.stack.push(Frame::GetKey { node, recv: v });
                Ctl::Eval(key, env)
            }
            Frame::Member { node } => {
                let (name, optional) = match prog.node(node) {
                    Node::Member { name, optional, .. } => (name, *optional),
                    _ => unreachable!(),
                };
                t.cur_node = node;
                if optional && matches!(v, Value::Null) {
                    return Ctl::Ret(Value::Null);
                }
                if let Some(method) = crate::methods::lookup(v.kind(), name) {
                    let target = Rc::new(Function::Builtin(method.builtin));
                    if method.property {
                        self.charge(t); // property invocation, as distinct from selection
                        if let Some(stop) = self.checkpoint(t) { return stop; }
                        return Ctl::Apply(Value::Func(target), vec![v], None);
                    }
                    return Ctl::Ret(Value::Func(Rc::new(Function::BoundMethod {
                        name: name.clone(), receiver: v, target,
                    })));
                }
                if let Value::Record(r) = &v {
                    if let Some(field) = r.get(name) {
                        self.charge(t); // field-name literal, as for bracket access
                        if let Some(stop) = self.checkpoint(t) { return stop; }
                        return Ctl::Ret(field.clone());
                    }
                }
                if let Some(method) = crate::methods::named(name) {
                    return self.raise(t, err::type_error(name, err::Pos::Index(0), method.expected(), v.kind()));
                }
                self.charge(t); // ordinary field-name literal
                if let Some(stop) = self.checkpoint(t) { return stop; }
                match access(&v, &Value::Str(name.clone()), optional) {
                    Ok(value) => Ctl::Ret(value),
                    Err(error) => self.raise(t, error),
                }
            }
            Frame::GetKey { node, recv } => {
                let optional = matches!(prog.node(node), Node::Get { optional: true, .. });
                t.cur_node = node;
                match access(&recv, &v, optional) {
                    Ok(x) => Ctl::Ret(x),
                    Err(e) => self.raise(t, e),
                }
            }
            Frame::Unary { node } => {
                t.cur_node = node;
                let op = match prog.node(node) {
                    Node::Unary { op, .. } => *op,
                    _ => unreachable!(),
                };
                match (op, &v) {
                    (UnOp::Not, Value::Bool(b)) => Ctl::Ret(Value::Bool(!b)),
                    (UnOp::Neg, Value::Num(d)) => Ctl::Ret(Value::num(d.neg())),
                    (UnOp::Not, other) => {
                        let e = err::type_error("!", err::Pos::Index(1), "bool", other.kind());
                        self.raise(t, e)
                    }
                    (UnOp::Neg, other) => {
                        let e = err::type_error("-", err::Pos::Index(1), "number", other.kind());
                        self.raise(t, e)
                    }
                }
            }
            Frame::BinLeft { node, env } => {
                let (op, r) = match prog.node(node) {
                    Node::Binary { op, r, .. } => (*op, *r),
                    _ => unreachable!(),
                };
                match op {
                    BinOp::And | BinOp::Or => {
                        let b = match v {
                            Value::Bool(b) => b,
                            other => {
                                t.cur_node = node;
                                let e = err::type_error(op.symbol(), err::Pos::Index(1), "bool", other.kind());
                                return self.raise(t, e);
                            }
                        };
                        if (op == BinOp::And && !b) || (op == BinOp::Or && b) {
                            return Ctl::Ret(Value::Bool(b));
                        }
                        t.stack.push(Frame::BoolRight { node });
                        Ctl::Eval(r, env)
                    }
                    BinOp::Coalesce => {
                        if matches!(v, Value::Null) {
                            Ctl::Eval(r, env)
                        } else {
                            Ctl::Ret(v)
                        }
                    }
                    _ => {
                        t.stack.push(Frame::BinRight { node, left: v });
                        Ctl::Eval(r, env)
                    }
                }
            }
            Frame::BoolRight { node } => match v {
                Value::Bool(_) => Ctl::Ret(v),
                other => {
                    t.cur_node = node;
                    let op = match prog.node(node) {
                        Node::Binary { op, .. } => *op,
                        _ => unreachable!(),
                    };
                    let e = err::type_error(op.symbol(), err::Pos::Index(2), "bool", other.kind());
                    self.raise(t, e)
                }
            },
            Frame::BinRight { node, left } => {
                t.cur_node = node;
                let op = match prog.node(node) {
                    Node::Binary { op, .. } => *op,
                    _ => unreachable!(),
                };
                match binary(op, &left, &v) {
                    Ok(x) => Ctl::Ret(x),
                    Err(f) => self.fail_ctl(t, f),
                }
            }
            Frame::If { node, env } => {
                let (tb, eb) = match prog.node(node) {
                    Node::If { t, e, .. } => (*t, *e),
                    _ => unreachable!(),
                };
                match v {
                    Value::Bool(true) => Ctl::Eval(tb, env),
                    Value::Bool(false) => Ctl::Eval(eb, env),
                    other => {
                        t.cur_node = node;
                        let e = err::type_error("if", err::Pos::Index(1), "bool", other.kind());
                        self.raise(t, e)
                    }
                }
            }
            Frame::Try { .. } => Ctl::Ret(v),
            Frame::Query { node, mut vals, env } => {
                vals.push(v);
                let (segments, holes) = match prog.node(node) {
                    Node::Query { segments, holes } => (segments, holes),
                    _ => unreachable!(),
                };
                if vals.len() < holes.len() {
                    let next = holes[vals.len()];
                    t.stack.push(Frame::Query { node, vals, env: env.clone() });
                    Ctl::Eval(next, env)
                } else {
                    Ctl::Ret(Value::Query(Rc::new(crate::value::Query { segments: (**segments).clone(), values: vals })))
                }
            }
            Frame::Collect(c) => self.collect_ret(t, *c, v),
            Frame::Retry(r) => self.retry_ret(t, *r, v),
            Frame::Pages(p) => self.pages_ret(t, *p, v),
            Frame::Tx(x) => self.tx_ret(t, *x, v),
            Frame::Request(f) => self.request_ret(t, *f, v),
            Frame::Op(_) | Frame::Sql(_) | Frame::Serve(_) => {
                self.harness_failure = Some("value delivered to an effect frame".into());
                Ctl::Waiting
            }
        }
    }

    pub fn frame_throw(&mut self, t: &mut Task, fr: Frame, e: Value) -> Ctl {
        match fr {
            Frame::Try { node, env } => {
                let slot = match self.prog.node(node) {
                    Node::Try { slot, .. } => *slot,
                    _ => unreachable!(),
                };
                t.throw_span = None;
                env.set(slot, e);
                Ctl::Seq(SeqOwner::Node(node), 0, env)
            }
            Frame::Retry(r) => self.retry_throw(t, *r, e),
            Frame::Tx(x) => self.tx_throw(t, *x, e),
            Frame::Request(f) => self.request_throw(t, *f, e),
            _ => Ctl::Throw(e),
        }
    }

    pub fn frame_effect(&mut self, t: &mut Task, fr: Frame, o: EffOutcome) -> Ctl {
        match fr {
            Frame::Op(op) => self.op_effect(t, *op, o),
            Frame::Retry(r) => self.retry_effect(t, *r, o),
            Frame::Tx(x) => self.tx_effect(t, *x, o),
            Frame::Sql(s) => self.sql_effect(t, *s, o),
            Frame::Serve(s) => self.serve_effect(t, s, o),
            Frame::Request(f) => self.request_effect(t, *f, o),
            _ => {
                self.harness_failure = Some("effect completion delivered to a non-effect frame".into());
                Ctl::Waiting
            }
        }
    }

    pub fn frame_abort(&mut self, t: &mut Task, fr: Frame, sig: AbortSig, carry: Option<EffOutcome>) -> Ctl {
        match fr {
            Frame::Tx(x) => self.tx_abort(t, *x, sig, carry),
            Frame::Sql(s) => self.sql_abort(t, *s, sig, carry),
            Frame::Serve(s) => self.serve_abort(t, s, sig, carry),
            Frame::Request(f) => self.request_abort(t, *f, sig, carry),
            _ => Ctl::Abort(sig, None),
        }
    }
}

/// Strict/optional access (§5.4).
pub fn access(recv: &Value, key: &Value, optional: bool) -> Result<Value, Value> {
    match recv {
        Value::Record(r) => match key {
            Value::Str(k) => match r.get(k) {
                Some(v) => Ok(v.clone()),
                None if optional => Ok(Value::Null),
                None => Err(err::make("missing", format!("missing key {k:?}"), vec![("key", Value::Str(k.clone()))])),
            },
            other => Err(err::type_error("access", err::Pos::Index(1), "string", other.kind())),
        },
        Value::Array(a) => match key {
            Value::Num(d) => {
                if !d.is_integer() || d.is_negative() {
                    return Err(err::argument("access", "index must be a nonnegative integer"));
                }
                let len = a.len();
                match d.to_u64() {
                    Some(i) if (i as u128) < len as u128 => Ok(a[i as usize].clone()),
                    _ if optional => Ok(Value::Null),
                    _ => Err(err::make(
                        "bounds",
                        "index out of range",
                        vec![("index", Value::Num(Rc::new((**d).clone()))), ("length", Value::int(len as i64))],
                    )),
                }
            }
            other => Err(err::type_error("access", err::Pos::Index(1), "number", other.kind())),
        },
        other => Err(err::type_error("access", err::Pos::Index(0), "record or array", other.kind())),
    }
}

fn overflow() -> Fail {
    Fail::Abort(AbortReason::Resource("number"))
}

/// Strict binary operators (§5.4).
pub fn binary(op: BinOp, l: &Value, r: &Value) -> Result<Value, Fail> {
    use crate::decimal::DivError;
    let sym = op.symbol();
    let num_pair = || -> Result<(&crate::decimal::Decimal, &crate::decimal::Decimal), Fail> {
        match (l, r) {
            (Value::Num(a), Value::Num(b)) => Ok((a, b)),
            (Value::Num(_), other) => Err(err::type_error(sym, err::Pos::Index(2), "number", other.kind()).into()),
            (other, _) => Err(err::type_error(sym, err::Pos::Index(1), "number", other.kind()).into()),
        }
    };
    match op {
        BinOp::Add => match (l, r) {
            (Value::Num(a), Value::Num(b)) => Ok(Value::num(a.add(b).map_err(|_| overflow())?)),
            (Value::Str(a), Value::Str(b)) => {
                if crate::interrupt::reserve(a.len() + b.len()) {
                    return Err(Fail::Interrupted);
                }
                let mut s = String::with_capacity(a.len() + b.len());
                s.push_str(a);
                s.push_str(b);
                Ok(Value::str(&s))
            }
            (Value::Array(a), Value::Array(b)) => {
                if crate::interrupt::reserve((a.len() + b.len()) * std::mem::size_of::<Value>()) {
                    return Err(Fail::Interrupted);
                }
                let mut v = Vec::with_capacity(a.len() + b.len());
                v.extend(a.iter().cloned());
                v.extend(b.iter().cloned());
                Ok(Value::array(v))
            }
            (Value::Num(_), other) | (Value::Str(_), other) | (Value::Array(_), other) => {
                let expected = l.kind().name();
                Err(err::type_error(sym, err::Pos::Index(2), expected, other.kind()).into())
            }
            (other, _) => Err(err::type_error(sym, err::Pos::Index(1), "number, string or array", other.kind()).into()),
        },
        BinOp::Sub => {
            let (a, b) = num_pair()?;
            Ok(Value::num(a.sub(b).map_err(|_| overflow())?))
        }
        BinOp::Mul => {
            let (a, b) = num_pair()?;
            Ok(Value::num(a.mul(b).map_err(|_| overflow())?))
        }
        BinOp::Div | BinOp::Rem => {
            let (a, b) = num_pair()?;
            let res = if op == BinOp::Div { a.div(b) } else { a.rem(b) };
            match res {
                Ok(d) => Ok(Value::num(d)),
                Err(DivError::DivisionByZero) => Err(err::number("division_by_zero").into()),
                Err(DivError::Overflow) => Err(overflow()),
            }
        }
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let ord = match (l, r) {
                (Value::Num(a), Value::Num(b)) => a.cmp_exact(b),
                (Value::Str(a), Value::Str(b)) => a.chars().cmp(b.chars()),
                (Value::Num(_), other) | (Value::Str(_), other) => {
                    return Err(err::type_error(sym, err::Pos::Index(2), l.kind().name(), other.kind()).into())
                }
                (other, _) => return Err(err::type_error(sym, err::Pos::Index(1), "number or string", other.kind()).into()),
            };
            use std::cmp::Ordering::*;
            let b = match op {
                BinOp::Lt => ord == Less,
                BinOp::Le => ord != Greater,
                BinOp::Gt => ord == Greater,
                _ => ord != Less,
            };
            Ok(Value::Bool(b))
        }
        BinOp::Eq | BinOp::Ne => {
            if let Some(k) = l.first_non_json() {
                return Err(err::type_error(sym, err::Pos::Index(1), "json", k).into());
            }
            if let Some(k) = r.first_non_json() {
                return Err(err::type_error(sym, err::Pos::Index(2), "json", k).into());
            }
            let eq = json_equal(l, r);
            Ok(Value::Bool(if op == BinOp::Eq { eq } else { !eq }))
        }
        BinOp::And | BinOp::Or | BinOp::Coalesce => unreachable!(),
    }
}

#[allow(dead_code)]
fn kind_name(v: &Value) -> &'static str {
    let k: Kind = v.kind();
    k.name()
}
