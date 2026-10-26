//! Callback-driven builtins: collection functions, parallel, retry, pages
//! (spec §7.3, §9). Callbacks are invoked through the machine, never by host
//! recursion, so they may suspend on effects.

use super::args::*;
use super::pure::JsonIndex;
use super::Builtin;
use crate::host::{EffOutcome, Effect, Host};
use crate::runtime::frames::Frame;
use crate::runtime::{AbortReason, Ctl, DispatchOpts, Fail, Runtime, Task};
use crate::value::{err, Kind, Record, Value};
use num_bigint::BigInt;
use std::rc::Rc;

pub struct Collect {
    pub kind: Builtin,
    pub f: Option<Value>,
    pub items: Rc<Vec<Value>>,
    pub idx: usize,
    pub out: Vec<Value>,
    pub keys: Vec<Value>,
    pub acc: Value,
    pub groups: Vec<Vec<Value>>,
    pub index: JsonIndex,
    pub key_kind: Option<Kind>,
}

pub enum RetryPhase {
    Attempt,
    When(Value),
    Backoff,
}

pub struct RetryFrame {
    pub f: Value,
    pub when: Value,
    pub delay: crate::decimal::Decimal,
    pub attempts: crate::decimal::Decimal,
    pub i: u64,
    pub phase: RetryPhase,
}

pub struct PagesFrame {
    pub step: Value,
    pub seen: Vec<Value>,
    pub seen_index: JsonIndex,
    pub result: Vec<Value>,
    pub pages: u64,
    pub max_pages: u64,
    pub max_items: u64,
}

impl<H: Host> Runtime<H> {
    pub fn start_collect(&mut self, t: &mut Task, b: Builtin, args: Vec<Value>, cb: Option<Value>) -> Ctl {
        match self.start_collect_inner(t, b, args, cb) {
            Ok(c) => c,
            Err(f) => self.fail_ctl(t, f),
        }
    }

    fn start_collect_inner(&mut self, t: &mut Task, b: Builtin, args: Vec<Value>, cb: Option<Value>) -> Result<Ctl, Fail> {
        let op = b.name();
        let items = array(op, 1, &args[0])?.clone();
        let acc = if b == Builtin::Fold { args[1].clone() } else { Value::Null };
        let arity = if b == Builtin::Fold { 2 } else { 1 };
        if let Some(f) = &cb {
            callback(op, f, arity)?;
        }
        let mut c = Collect {
            kind: b,
            f: cb,
            items,
            idx: 0,
            out: Vec::new(),
            keys: Vec::new(),
            acc,
            groups: Vec::new(),
            index: JsonIndex::new(),
            key_kind: None,
        };
        if c.items.is_empty() {
            return self.collect_finish(c);
        }
        if c.f.is_none() {
            // sort/unique identity: keys are the items themselves.
            let items = c.items.clone();
            for v in items.iter() {
                if crate::interrupt::poll() {
                    return Err(Fail::Interrupted);
                }
                self.collect_accept(&mut c, v.clone())?;
                c.idx += 1;
            }
            return self.collect_finish(c);
        }
        Ok(self.collect_invoke(t, c))
    }

    fn collect_invoke(&mut self, t: &mut Task, c: Collect) -> Ctl {
        let item = c.items[c.idx].clone();
        let f = c.f.clone().unwrap();
        let args = if c.kind == Builtin::Fold { vec![c.acc.clone(), item] } else { vec![item] };
        t.stack.push(Frame::Collect(Box::new(c)));
        Ctl::Apply(f, args, None)
    }

    /// Validate and record one callback result (or identity key).
    fn collect_accept(&mut self, c: &mut Collect, v: Value) -> Result<(), Fail> {
        let op = c.kind.name();
        match c.kind {
            Builtin::Map => c.out.push(v),
            Builtin::Filter => match v {
                Value::Bool(true) => c.out.push(c.items[c.idx].clone()),
                Value::Bool(false) => {}
                other => return Err(type_err(op, 0, "bool", &other)),
            },
            Builtin::Each => {}
            Builtin::Fold => c.acc = v,
            Builtin::Sort => {
                let k = v.kind();
                match (c.key_kind, k) {
                    (None, Kind::Number | Kind::String) => c.key_kind = Some(k),
                    (Some(prev), _) if prev == k => {}
                    _ => return Err(type_err(op, 0, "number or string key of one kind", &v)),
                }
                c.keys.push(v);
            }
            Builtin::Group | Builtin::Unique => {
                if let Some(k) = v.first_non_json() {
                    return Err(Fail::Err(err::type_error(op, err::Pos::Index(0), "json", k)));
                }
                let pos = c.keys.len();
                match c.index.find_or_insert(&c.keys, &v, pos) {
                    Some(i) => {
                        if c.kind == Builtin::Group {
                            c.groups[i].push(c.items[c.idx].clone());
                        }
                    }
                    None => {
                        c.keys.push(v);
                        c.groups.push(vec![c.items[c.idx].clone()]);
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn collect_finish(&mut self, c: Collect) -> Result<Ctl, Fail> {
        let v = match c.kind {
            Builtin::Map | Builtin::Filter => Value::array(c.out),
            Builtin::Each => Value::Null,
            Builtin::Fold => c.acc,
            Builtin::Sort => {
                let mut idx: Vec<usize> = (0..c.keys.len()).collect();
                let keys = &c.keys;
                // Comparisons poll in proportion to their size; once tripped the
                // order no longer matters (the result is discarded).
                idx.sort_by(|&a, &b| match (&keys[a], &keys[b]) {
                    _ if crate::interrupt::tripped() => std::cmp::Ordering::Equal,
                    (Value::Num(x), Value::Num(y)) => {
                        crate::interrupt::poll();
                        x.cmp_exact(y)
                    }
                    (Value::Str(x), Value::Str(y)) => {
                        if Rc::ptr_eq(x, y) || crate::interrupt::poll_big(x.len().min(y.len())) {
                            std::cmp::Ordering::Equal
                        } else {
                            x.chars().cmp(y.chars())
                        }
                    }
                    _ => std::cmp::Ordering::Equal,
                });
                if crate::interrupt::tripped() {
                    return Err(Fail::Interrupted);
                }
                Value::array(idx.into_iter().map(|i| c.items[i].clone()).collect())
            }
            Builtin::Group => Value::array(
                c.keys
                    .into_iter()
                    .zip(c.groups)
                    .map(|(k, items)| Value::rec(vec![("key", k), ("items", Value::array(items))]))
                    .collect(),
            ),
            Builtin::Unique => Value::array(c.groups.into_iter().map(|g| g.into_iter().next().unwrap()).collect()),
            _ => unreachable!(),
        };
        Ok(Ctl::Ret(v))
    }

    pub fn collect_ret(&mut self, t: &mut Task, mut c: Collect, v: Value) -> Ctl {
        if let Err(f) = self.collect_accept(&mut c, v) {
            return self.fail_ctl(t, f);
        }
        c.idx += 1;
        if c.idx < c.items.len() {
            self.collect_invoke(t, c)
        } else {
            match self.collect_finish(c) {
                Ok(ctl) => ctl,
                Err(f) => self.fail_ctl(t, f),
            }
        }
    }

    // ------------------------------------------------------------ parallel

    pub fn start_parallel(&mut self, t: &mut Task, args: Vec<Value>, f: Value) -> Ctl {
        let op = "parallel";
        let res: Result<(Rc<Vec<Value>>, u64), Fail> = (|| {
            let xs = array(op, 1, &args[0])?.clone();
            let n = positive_u64(op, 2, &args[1])?;
            callback(op, &f, 1)?;
            Ok((xs, n))
        })();
        let (xs, n) = match res {
            Ok(x) => x,
            Err(e) => return self.fail_ctl(t, e),
        };
        if xs.is_empty() {
            return Ctl::Ret(Value::array(Vec::new()));
        }
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        match self.create_group(t, xs, n, f) {
            Ok(g) => Ctl::WaitGroup(g),
            Err(r) => self.fail_ctl(t, Fail::Abort(r)),
        }
    }

    // --------------------------------------------------------------- retry

    pub fn start_retry(&mut self, t: &mut Task, args: Vec<Value>, f: Value) -> Ctl {
        let op = "retry";
        let res: Result<RetryFrame, Fail> = (|| {
            let o = record(op, 1, &args[0])?;
            let attempts = o.get("attempts").ok_or_else(|| arg_err(op, "attempts is required"))?;
            let attempts = match attempts {
                Value::Num(d) if d.is_integer() => (**d).clone(),
                Value::Num(_) => return Err(arg_err(op, "attempts must be an integer")),
                other => return Err(opt_type_err(op, "attempts", "number", other)),
            };
            if attempts.is_negative() || attempts.is_zero() {
                return Err(arg_err(op, "attempts must be at least 1"));
            }
            let when = o.get("when").ok_or_else(|| arg_err(op, "when is required"))?.clone();
            function_option(op, "when", &when, 1)?;
            let delay = o.get("delay").ok_or_else(|| arg_err(op, "delay is required"))?;
            let delay = match delay {
                Value::Num(d) if d.is_integer() => (**d).clone(),
                Value::Num(_) => return Err(arg_err(op, "delay must be an integer")),
                other => return Err(opt_type_err(op, "delay", "number", other)),
            };
            if delay.is_negative() {
                return Err(arg_err(op, "delay must be nonnegative"));
            }
            reject_unknown(op, Some(o), &["attempts", "when", "delay"])?;
            callback(op, &f, 0)?;
            Ok(RetryFrame { f: f.clone(), when, delay, attempts, i: 1, phase: RetryPhase::Attempt })
        })();
        match res {
            Ok(r) => {
                let f = r.f.clone();
                t.stack.push(Frame::Retry(Box::new(r)));
                Ctl::Apply(f, Vec::new(), None)
            }
            Err(e) => self.fail_ctl(t, e),
        }
    }

    pub fn retry_ret(&mut self, t: &mut Task, mut r: RetryFrame, v: Value) -> Ctl {
        match std::mem::replace(&mut r.phase, RetryPhase::Attempt) {
            RetryPhase::Attempt => Ctl::Ret(v),
            RetryPhase::When(e) => match v {
                Value::Bool(true) => {
                    // Exact delay*2^(i-1); a wait beyond host limits aborts.
                    let factor = crate::decimal::Decimal::from_bigint(num_traits::pow(BigInt::from(2), ((r.i - 1).min(400)) as usize));
                    let ms = match r.delay.mul(&factor).ok().and_then(|d| d.to_u64()) {
                        _ if r.delay.is_zero() => 0,
                        Some(m) if r.i <= 400 && m < (1u64 << 52) => m,
                        _ => return self.fail_ctl(t, Fail::Abort(AbortReason::Resource("retry_delay"))),
                    };
                    r.phase = RetryPhase::Backoff;
                    t.stack.push(Frame::Retry(Box::new(r)));
                    Ctl::Dispatch(Effect::Sleep(ms), DispatchOpts::default())
                }
                Value::Bool(false) => Ctl::Throw(e),
                other => {
                    let e = err::type_error("retry", err::Pos::Option("when"), "bool", other.kind());
                    self.raise(t, e)
                }
            },
            RetryPhase::Backoff => unreachable!(),
        }
    }

    pub fn retry_throw(&mut self, _t: &mut Task, mut r: RetryFrame, e: Value) -> Ctl {
        match r.phase {
            RetryPhase::Attempt => {
                if crate::decimal::Decimal::from_u64(r.i).cmp_exact(&r.attempts) != std::cmp::Ordering::Less {
                    return Ctl::Throw(e);
                }
                let when = r.when.clone();
                r.phase = RetryPhase::When(e.clone());
                _t.stack.push(Frame::Retry(Box::new(r)));
                Ctl::Apply(when, vec![e], None)
            }
            // A failing predicate replaces the original error.
            RetryPhase::When(_) | RetryPhase::Backoff => Ctl::Throw(e),
        }
    }

    pub fn retry_effect(&mut self, t: &mut Task, mut r: RetryFrame, o: EffOutcome) -> Ctl {
        match o {
            EffOutcome::Ok(_) => {
                r.i += 1;
                r.phase = RetryPhase::Attempt;
                let f = r.f.clone();
                t.stack.push(Frame::Retry(Box::new(r)));
                Ctl::Apply(f, Vec::new(), None)
            }
            EffOutcome::Err(e) => self.raise(t, e),
            EffOutcome::Resource(r) => self.fail_ctl(t, Fail::Abort(AbortReason::Resource(leak(r)))),
            EffOutcome::Joined | EffOutcome::JoinedNotSent => self.joined_without_abort(t),
        }
    }

    pub fn joined_without_abort(&mut self, t: &mut Task) -> Ctl {
        match self.abort_of(t.scope) {
            Some(sig) => Ctl::Abort(sig, None),
            None => {
                self.harness_failure = Some("effect joined without an abort".into());
                Ctl::Waiting
            }
        }
    }

    // --------------------------------------------------------------- pages

    pub fn start_pages(&mut self, t: &mut Task, args: Vec<Value>, step: Value) -> Ctl {
        let op = "pages";
        let res: Result<(Value, PagesFrame), Fail> = (|| {
            let seed = args[0].clone();
            if let Some(k) = seed.first_non_json() {
                return Err(Fail::Err(err::type_error(op, err::Pos::Index(1), "json", k)));
            }
            let o = options(op, 2, args.get(1), &["max_pages", "max_items"])?;
            let max_pages = match field(o, "max_pages") {
                Some(v) => opt_int(op, "max_pages", v, 1)?,
                None => 100,
            };
            let max_items = match field(o, "max_items") {
                Some(v) => opt_int(op, "max_items", v, 0)?,
                None => 10000,
            };
            reject_unknown(op, o, &["max_pages", "max_items"])?;
            callback(op, &step, 1)?;
            let mut p = PagesFrame {
                step: step.clone(),
                seen: Vec::new(),
                seen_index: JsonIndex::new(),
                result: Vec::new(),
                pages: 0,
                max_pages,
                max_items,
            };
            if !matches!(seed, Value::Null) {
                p.seen_index.find_or_insert(&[], &seed, 0);
                p.seen.push(seed.clone());
            }
            Ok((seed, p))
        })();
        match res {
            Ok((seed, p)) => {
                let f = p.step.clone();
                t.stack.push(Frame::Pages(Box::new(p)));
                Ctl::Apply(f, vec![seed], None)
            }
            Err(e) => self.fail_ctl(t, e),
        }
    }

    pub fn pages_ret(&mut self, t: &mut Task, mut p: PagesFrame, v: Value) -> Ctl {
        let op = "pages";
        let res: Result<Option<Value>, Fail> = (|| {
            let r = match &v {
                Value::Record(r) => r,
                other => return Err(type_err(op, 0, "record {items,next}", other)),
            };
            if r.len() != 2 || !r.contains("items") || !r.contains("next") {
                return Err(arg_err(op, "step result must have exactly the fields items and next"));
            }
            let items = match r.get("items").unwrap() {
                Value::Array(a) => a.clone(),
                other => return Err(type_err(op, 0, "array", other)),
            };
            let next = r.get("next").unwrap().clone();
            if let Some(k) = next.first_non_json() {
                return Err(Fail::Err(err::type_error(op, err::Pos::Index(0), "json", k)));
            }
            p.pages += 1;
            let prospective = p.result.len() as u64 + items.len() as u64;
            let pag = |reason: &str, pages: u64, items: u64| {
                Fail::Err(err::make(
                    "pagination",
                    format!("pagination stopped: {reason}"),
                    vec![
                        ("reason", Value::str(reason)),
                        ("pages", Value::uint(pages)),
                        ("items", Value::uint(items)),
                    ],
                ))
            };
            if prospective > p.max_items {
                return Err(pag("max_items", p.pages, prospective));
            }
            p.result.extend(items.iter().cloned());
            if matches!(next, Value::Null) {
                return Ok(None);
            }
            let pos = p.seen.len();
            if p.seen_index.find_or_insert(&p.seen, &next, pos).is_some() {
                return Err(pag("repeated_cursor", p.pages, p.result.len() as u64));
            }
            if p.pages >= p.max_pages {
                return Err(pag("max_pages", p.pages, p.result.len() as u64));
            }
            p.seen.push(next.clone());
            Ok(Some(next))
        })();
        match res {
            Ok(None) => Ctl::Ret(Value::array(std::mem::take(&mut p.result))),
            Ok(Some(next)) => {
                let f = p.step.clone();
                t.stack.push(Frame::Pages(Box::new(p)));
                Ctl::Apply(f, vec![next], None)
            }
            Err(e) => self.fail_ctl(t, e),
        }
    }
}

/// Leak a resource name into a static string (bounded set of names).
pub fn leak(s: String) -> &'static str {
    match s.as_str() {
        "memory" => "memory",
        "max_file_bytes" => "max_file_bytes",
        "header_bytes" => "header_bytes",
        "transport" => "transport",
        "clock" => "clock",
        _ => "host",
    }
}

#[allow(dead_code)]
fn unused(_: Record) {}
