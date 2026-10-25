//! Task engine: scheduler, abort scopes, deadlines, budgets and effect
//! plumbing (spec §8, §9, §14, §16.3).
//!
//! Every Kiri task is an explicit-continuation machine (`Task::stack` +
//! `Task::ctl`). The scheduler runs ready tasks FIFO for at most
//! [`QUANTUM`] semantic steps, then requeues them. When no task is ready it
//! waits for the next host event or runtime timer.

pub mod frames;
pub mod machine;

use crate::ast::{NodeId, Program};
use crate::builtins;
use crate::diag::Span;
use crate::host::config::HostConfig;
use crate::host::{EffOutcome, Effect, Host, HostEvent};
use crate::schema;
use crate::value::{err, Function, Value};
use frames::Frame;
use std::cell::RefCell;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::rc::Rc;

pub const QUANTUM: u64 = 1024;

pub type TaskId = u32;
pub type ScopeId = u32;
pub type EffId = u64;

/// Activation environment.
pub struct Env {
    pub locals: RefCell<Vec<Value>>,
    pub func: Option<Rc<Function>>,
}

impl Env {
    pub fn set(&self, slot: u32, v: Value) {
        self.locals.borrow_mut()[slot as usize] = v;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqOwner {
    Program,
    Node(NodeId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AbortReason {
    Cancelled,
    Deadline,
    Resource(&'static str),
    /// A transaction scope deadline (converted at the tx boundary).
    TxTimeout(u64),
}

impl AbortReason {
    pub fn name(&self) -> &'static str {
        match self {
            AbortReason::Cancelled => "cancelled",
            AbortReason::Deadline | AbortReason::TxTimeout(_) => "deadline",
            AbortReason::Resource(_) => "resource",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AbortSig {
    pub scope: ScopeId,
    pub reason: AbortReason,
}

/// Options for an effect dispatch.
#[derive(Clone, Debug, Default)]
pub struct DispatchOpts {
    /// Cleanup effects are not cancelled by the abort being cleaned up.
    pub cleanup: bool,
    /// Operation timeout: (operation name, ms).
    pub timeout: Option<(&'static str, u64)>,
}

/// Machine control state.
pub enum Ctl {
    Eval(NodeId, Rc<Env>),
    Seq(SeqOwner, usize, Rc<Env>),
    Apply(Value, Vec<Value>, Option<Value>),
    Ret(Value),
    Throw(Value),
    /// Effect completion delivered to the top frame.
    Effect(EffOutcome),
    /// Abort unwinding; carries an effect outcome that arrived meanwhile.
    Abort(AbortSig, Option<EffOutcome>),
    /// Request an effect dispatch, then wait.
    Dispatch(Effect, DispatchOpts),
    /// Wait for a parallel group.
    WaitGroup(u32),
    /// Suspended (waiting); no step to take.
    Waiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoneTo {
    Root,
    Child { group: u32, index: usize },
    Request { server: u32, request: u64 },
}

pub struct Task {
    pub id: TaskId,
    pub scope: ScopeId,
    pub budget: u32,
    pub stack: Vec<Frame>,
    pub ctl: Ctl,
    pub done_to: DoneTo,
    /// Active transaction owned by this task.
    pub tx: Option<u64>,
    pub throw_span: Option<Span>,
    pub cur_node: NodeId,
    pub seen_epoch: u64,
    /// Waiting on a cleanup effect whose result must be delivered.
    pub in_cleanup: bool,
    /// The abort currently being unwound, if any.
    pub unwinding: Option<AbortSig>,
    pub waiting_eff: Option<EffId>,
}

pub struct Scope {
    pub parent: Option<ScopeId>,
    pub abort: Option<AbortReason>,
    /// Absolute monotonic ms of this scope's own deadline, if any.
    pub deadline: Option<u64>,
}

pub struct Budget {
    pub steps: u64,
    pub limit: u64,
    pub abort_after: Option<u64>,
    pub scope: ScopeId,
}

pub struct Pending {
    pub task: TaskId,
    pub cleanup: bool,
    pub cancelled: bool,
    pub timeout: Option<(&'static str, u64)>,
    pub timed_out: bool,
    pub op: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimerKind {
    ScopeDeadline(ScopeId),
    EffectTimeout(EffId),
    CleanupGrace(EffId),
    RootCancel,
}

#[derive(Debug, PartialEq, Eq)]
struct Timer {
    at: u64,
    seq: u64,
    kind: TimerKind,
    reason_deadline: Option<u64>,
}

impl Ord for Timer {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (o.at, o.seq).cmp(&(self.at, self.seq))
    }
}

impl PartialOrd for Timer {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// Parallel group state (§9.1).
pub struct Group {
    pub parent: TaskId,
    pub items: Rc<Vec<Value>>,
    pub f: Value,
    pub n: usize,
    pub next: usize,
    pub active: usize,
    pub results: Vec<Option<Value>>,
    pub failure: Option<(Value, Option<Span>)>,
    pub children: HashMap<usize, (TaskId, ScopeId)>,
    pub parent_scope: ScopeId,
}

/// Transaction state (§11.5).
pub struct TxState {
    pub owner: TaskId,
    pub connection: String,
    pub active: bool,
    pub latched: Option<Value>,
}

/// Final outcome of the root evaluation.
#[derive(Debug)]
pub enum RootOutcome {
    Value(Value),
    Error { error: Value, span: Option<Span> },
    Abort { reason: &'static str, data: Value },
    /// A conformance-harness failure outside Kiri semantics.
    Harness(String),
}

pub struct Stats {
    pub max_parallel_active: usize,
}

pub struct Runtime<H: Host> {
    pub prog: Rc<Program>,
    pub prelude: Vec<Value>,
    pub host: H,
    pub config: Rc<HostConfig>,
    pub tasks: HashMap<TaskId, Task>,
    pub ready: VecDeque<TaskId>,
    pub scopes: HashMap<ScopeId, Scope>,
    pub budgets: HashMap<u32, Budget>,
    pub pending: HashMap<EffId, Pending>,
    timers: BinaryHeap<Timer>,
    pub groups: HashMap<u32, Group>,
    pub txs: HashMap<u64, TxState>,
    pub servers: HashMap<u32, crate::builtins::server::ServerState>,
    pub pools: HashMap<String, String>,
    pub stats: Stats,
    pub root_outcome: Option<RootOutcome>,
    pub harness_failure: Option<String>,
    pub abort_epoch: u64,
    next_task: TaskId,
    next_scope: ScopeId,
    next_eff: EffId,
    next_timer_seq: u64,
    next_group: u32,
    next_budget: u32,
    pub next_tx: u64,
    pub next_server: u32,
    pub root_scope: ScopeId,
    pub memory_baseline: usize,
    pub live_tasks: u64,
}

pub enum Fail {
    Err(Value),
    Abort(AbortReason),
    /// Long builtin work stopped at an interrupt checkpoint; the runtime
    /// replaces the result with the observed abort.
    Interrupted,
}

impl From<Value> for Fail {
    fn from(v: Value) -> Fail {
        Fail::Err(v)
    }
}

pub type BResult = Result<Value, Fail>;

impl<H: Host> Runtime<H> {
    pub fn new(prog: Program, host: H, config: HostConfig) -> Runtime<H> {
        let mut prelude: Vec<Value> =
            builtins::ALL.iter().map(|b| Value::Func(Rc::new(Function::Builtin(*b)))).collect();
        for a in builtins::ATOMS {
            prelude.push(schema::atom(a));
        }
        Runtime {
            prog: Rc::new(prog),
            prelude,
            host,
            config: Rc::new(config),
            tasks: HashMap::new(),
            ready: VecDeque::new(),
            scopes: HashMap::new(),
            budgets: HashMap::new(),
            pending: HashMap::new(),
            timers: BinaryHeap::new(),
            groups: HashMap::new(),
            txs: HashMap::new(),
            servers: HashMap::new(),
            pools: HashMap::new(),
            stats: Stats { max_parallel_active: 0 },
            root_outcome: None,
            harness_failure: None,
            abort_epoch: 0,
            next_task: 1,
            next_scope: 1,
            next_eff: 1,
            next_timer_seq: 0,
            next_group: 1,
            next_budget: 1,
            next_tx: 1,
            next_server: 1,
            root_scope: 0,
            memory_baseline: crate::alloc::live_bytes(),
            live_tasks: 0,
        }
    }

    // ---------------------------------------------------------------- scopes

    pub fn new_scope(&mut self, parent: Option<ScopeId>) -> ScopeId {
        let id = self.next_scope;
        self.next_scope += 1;
        self.scopes.insert(id, Scope { parent, abort: None, deadline: None });
        id
    }

    pub fn drop_scope(&mut self, id: ScopeId) {
        self.scopes.remove(&id);
    }

    /// The outermost aborted scope on the chain from `scope` to the root.
    pub fn abort_of(&self, scope: ScopeId) -> Option<AbortSig> {
        let mut cur = Some(scope);
        let mut found = None;
        while let Some(s) = cur {
            match self.scopes.get(&s) {
                Some(sc) => {
                    if let Some(r) = &sc.abort {
                        found = Some(AbortSig { scope: s, reason: r.clone() });
                    }
                    cur = sc.parent;
                }
                None => break,
            }
        }
        found
    }

    pub fn is_ancestor_or_self(&self, anc: ScopeId, mut s: ScopeId) -> bool {
        loop {
            if s == anc {
                return true;
            }
            match self.scopes.get(&s).and_then(|x| x.parent) {
                Some(p) => s = p,
                None => return false,
            }
        }
    }

    /// Abort a scope and every descendant; cancel affected waiting effects.
    pub fn abort_scope(&mut self, scope: ScopeId, reason: AbortReason) {
        match self.scopes.get_mut(&scope) {
            Some(s) if s.abort.is_none() => s.abort = Some(reason),
            _ => return,
        }
        self.abort_epoch += 1;
        let mut to_cancel = Vec::new();
        for (id, p) in self.pending.iter() {
            if p.cleanup || p.cancelled {
                continue;
            }
            let tscope = match self.tasks.get(&p.task) {
                Some(t) => t.scope,
                None => continue,
            };
            if self.is_ancestor_or_self(scope, tscope) {
                to_cancel.push(*id);
            }
        }
        to_cancel.sort();
        for id in to_cancel {
            if let Some(p) = self.pending.get_mut(&id) {
                p.cancelled = true;
            }
            self.host.cancel(id);
        }
        // Server listeners whose scope is aborted stop accepting.
        let servers: Vec<u32> = self.servers.keys().copied().collect();
        for sid in servers {
            self.server_scope_aborted(sid);
        }
    }

    // ---------------------------------------------------------------- timers

    pub fn add_timer(&mut self, at: u64, kind: TimerKind) {
        self.next_timer_seq += 1;
        self.timers.push(Timer { at, seq: self.next_timer_seq, kind, reason_deadline: None });
    }

    pub fn add_scope_deadline(&mut self, scope: ScopeId, at: u64, tx_timeout: Option<u64>) {
        if let Some(s) = self.scopes.get_mut(&scope) {
            s.deadline = Some(at);
        }
        self.next_timer_seq += 1;
        self.timers.push(Timer { at, seq: self.next_timer_seq, kind: TimerKind::ScopeDeadline(scope), reason_deadline: tx_timeout });
    }

    fn next_timer_at(&self) -> Option<u64> {
        self.timers.peek().map(|t| t.at)
    }

    pub fn fire_due_timers(&mut self, now: u64) {
        while let Some(t) = self.timers.peek() {
            if t.at > now {
                break;
            }
            let t = self.timers.pop().unwrap();
            match t.kind {
                TimerKind::ScopeDeadline(s) => {
                    if self.scopes.contains_key(&s) {
                        let reason = match t.reason_deadline {
                            Some(ms) => AbortReason::TxTimeout(ms),
                            None => AbortReason::Deadline,
                        };
                        self.abort_scope(s, reason);
                    }
                }
                TimerKind::RootCancel => {
                    let root = self.root_scope;
                    self.abort_scope(root, AbortReason::Cancelled);
                }
                TimerKind::EffectTimeout(id) => {
                    if let Some(p) = self.pending.get_mut(&id) {
                        if !p.cancelled {
                            p.cancelled = true;
                            p.timed_out = true;
                            self.host.cancel(id);
                        }
                    }
                }
                TimerKind::CleanupGrace(id) => {
                    if self.pending.contains_key(&id) {
                        // Detach an unresponsive cleanup operation.
                        let p = self.pending.remove(&id).unwrap();
                        self.host.diagnostic("cleanup grace expired; operation detached");
                        self.deliver(p.task, EffOutcome::Joined);
                    }
                }
            }
        }
    }

    // ----------------------------------------------------------------- tasks

    pub fn new_budget(&mut self, scope: ScopeId, abort_after: Option<u64>) -> u32 {
        let id = self.next_budget;
        self.next_budget += 1;
        let limit = self.config.limits.eval_steps;
        self.budgets.insert(id, Budget { steps: 0, limit, abort_after, scope });
        id
    }

    pub fn spawn(&mut self, scope: ScopeId, budget: u32, ctl: Ctl, done_to: DoneTo) -> TaskId {
        let id = self.next_task;
        self.next_task += 1;
        self.live_tasks += 1;
        let t = Task {
            id,
            scope,
            budget,
            stack: Vec::new(),
            ctl,
            done_to,
            tx: None,
            throw_span: None,
            cur_node: 0,
            seen_epoch: u64::MAX,
            in_cleanup: false,
            unwinding: None,
            waiting_eff: None,
        };
        self.tasks.insert(id, t);
        self.ready.push_back(id);
        id
    }

    /// Start the root evaluation of the program.
    pub fn start_root(&mut self, abort_after_steps: Option<u64>) {
        let scope = self.new_scope(None);
        self.root_scope = scope;
        let budget = self.new_budget(scope, abort_after_steps);
        let env = Rc::new(Env { locals: RefCell::new(vec![Value::Null; self.prog.nlocals as usize]), func: None });
        if let Some(ms) = self.config.limits.deadline_ms {
            let at = self.host.now_ms().saturating_add(ms);
            self.add_scope_deadline(scope, at, None);
        }
        self.spawn(scope, budget, Ctl::Seq(SeqOwner::Program, 0, env), DoneTo::Root);
    }

    pub fn schedule_root_cancel(&mut self, at: u64) {
        self.add_timer(at, TimerKind::RootCancel);
    }

    /// Deliver an effect outcome to a waiting task and make it ready.
    pub fn deliver(&mut self, tid: TaskId, outcome: EffOutcome) {
        if let Some(t) = self.tasks.get_mut(&tid) {
            t.waiting_eff = None;
            t.ctl = Ctl::Effect(outcome);
            self.ready.push_back(tid);
        }
    }

    fn handle_event(&mut self, ev: HostEvent) {
        match ev {
            HostEvent::Done(id, outcome) => {
                let p = match self.pending.remove(&id) {
                    Some(p) => p,
                    None => return,
                };
                let outcome = match (p.timed_out, outcome) {
                    (true, EffOutcome::Joined) => {
                        let (op, ms) = p.timeout.unwrap();
                        EffOutcome::Err(timeout_error(op, ms, Some("unknown")))
                    }
                    (true, EffOutcome::JoinedNotSent) => {
                        let (op, ms) = p.timeout.unwrap();
                        EffOutcome::Err(timeout_error(op, ms, Some("not_sent")))
                    }
                    (false, EffOutcome::JoinedNotSent) => EffOutcome::Joined,
                    (_, o) => o,
                };
                self.deliver(p.task, outcome);
            }
            HostEvent::Request(req) => self.server_request(req),
            HostEvent::Disconnect(id) => self.server_disconnect(id),
            HostEvent::CancelRoot => {
                let root = self.root_scope;
                self.abort_scope(root, AbortReason::Cancelled);
            }
        }
    }

    /// Run until the root task completes.
    pub async fn run(&mut self) -> RootOutcome {
        let virtual_mode = self.host.is_virtual();
        loop {
            while let Some(tid) = self.ready.pop_front() {
                self.run_task(tid);
                if let Some(f) = self.harness_failure.take() {
                    return RootOutcome::Harness(f);
                }
                if !virtual_mode {
                    let now = self.host.now_ms();
                    self.fire_due_timers(now);
                    while let Some(ev) = self.host.wait(Some(0)).await {
                        self.handle_event(ev);
                    }
                }
            }
            if let Some(out) = self.root_outcome.take() {
                return out;
            }
            if self.host.failed() {
                return RootOutcome::Harness("fixture host failure".into());
            }
            let next = self.next_timer_at();
            match self.host.wait(next).await {
                Some(ev) => self.handle_event(ev),
                None => match next {
                    Some(_) => {
                        let now = self.host.now_ms();
                        self.fire_due_timers(now);
                    }
                    None => {
                        if virtual_mode {
                            return RootOutcome::Harness("deadlock: no runnable task, timer or event".into());
                        }
                    }
                },
            }
            if let Some(f) = self.harness_failure.take() {
                return RootOutcome::Harness(f);
            }
        }
    }

    fn run_task(&mut self, tid: TaskId) {
        let mut t = match self.tasks.remove(&tid) {
            Some(t) => t,
            None => return,
        };
        let mut used = 0u64;
        loop {
            let before = self.budgets.get(&t.budget).map(|b| b.steps).unwrap_or(0);
            let next = self.step(&mut t);
            let after = self.budgets.get(&t.budget).map(|b| b.steps).unwrap_or(0);
            used += after.saturating_sub(before);
            match next {
                StepResult::Continue => {
                    if used >= QUANTUM {
                        self.tasks.insert(tid, t);
                        self.ready.push_back(tid);
                        return;
                    }
                }
                StepResult::Suspended => {
                    self.tasks.insert(tid, t);
                    return;
                }
                StepResult::Done(outcome) => {
                    self.live_tasks -= 1;
                    self.task_done(t, outcome);
                    return;
                }
            }
        }
    }

    /// Dispatch an effect for a task.
    pub fn dispatch(&mut self, t: &mut Task, eff: Effect, opts: DispatchOpts) {
        let id = self.next_eff;
        self.next_eff += 1;
        let op = eff.op_name();
        self.pending.insert(
            id,
            Pending { task: t.id, cleanup: opts.cleanup, cancelled: false, timeout: opts.timeout, timed_out: false, op },
        );
        t.waiting_eff = Some(id);
        t.in_cleanup = opts.cleanup;
        let now = self.host.now_ms();
        if let Some((_, ms)) = opts.timeout {
            self.add_timer(now.saturating_add(ms), TimerKind::EffectTimeout(id));
        }
        if opts.cleanup {
            let grace = self.config.limits.cleanup_ms;
            self.add_timer(now.saturating_add(grace), TimerKind::CleanupGrace(id));
        }
        self.host.dispatch(id, eff);
    }

    fn task_done(&mut self, t: Task, outcome: TaskOutcome) {
        match t.done_to {
            DoneTo::Root => {
                self.root_outcome = Some(match outcome {
                    TaskOutcome::Value(v) => RootOutcome::Value(v),
                    TaskOutcome::Error(e, span) => RootOutcome::Error { error: e, span },
                    TaskOutcome::Abort(sig) => RootOutcome::Abort { reason: sig.reason.name(), data: abort_data(&sig) },
                });
            }
            DoneTo::Child { group, index } => self.child_done(group, index, outcome),
            DoneTo::Request { server, request } => self.request_done(server, request, outcome),
        }
    }

    // ----------------------------------------------------- parallel groups

    pub fn create_group(&mut self, t: &mut Task, items: Rc<Vec<Value>>, n: usize, f: Value) -> Result<u32, AbortReason> {
        let gid = self.next_group;
        self.next_group += 1;
        let len = items.len();
        self.groups.insert(
            gid,
            Group {
                parent: t.id,
                items,
                f,
                n,
                next: 0,
                active: 0,
                results: vec![None; len],
                failure: None,
                children: HashMap::new(),
                parent_scope: t.scope,
            },
        );
        let initial = n.min(len);
        for _ in 0..initial {
            self.start_child(gid, t.budget)?;
        }
        Ok(gid)
    }

    fn start_child(&mut self, gid: u32, budget: u32) -> Result<(), AbortReason> {
        if self.live_tasks + 1 > self.config.limits.max_tasks {
            return Err(AbortReason::Resource("max_tasks"));
        }
        let (index, item, f, pscope) = {
            let g = self.groups.get_mut(&gid).unwrap();
            let i = g.next;
            g.next += 1;
            g.active += 1;
            (i, g.items[i].clone(), g.f.clone(), g.parent_scope)
        };
        let scope = self.new_scope(Some(pscope));
        let tid = self.spawn(scope, budget, Ctl::Apply(f, vec![item], None), DoneTo::Child { group: gid, index });
        let g = self.groups.get_mut(&gid).unwrap();
        g.children.insert(index, (tid, scope));
        if g.active > self.stats.max_parallel_active {
            self.stats.max_parallel_active = g.active;
        }
        Ok(())
    }

    fn child_done(&mut self, gid: u32, index: usize, outcome: TaskOutcome) {
        let budget;
        {
            let g = match self.groups.get_mut(&gid) {
                Some(g) => g,
                None => return,
            };
            g.active -= 1;
            if let Some((_, scope)) = g.children.remove(&index) {
                self.scopes.remove(&scope);
            }
            let g = self.groups.get_mut(&gid).unwrap();
            budget = self.tasks.get(&g.parent).map(|p| p.budget).unwrap_or(0);
            match outcome {
                TaskOutcome::Value(v) => g.results[index] = Some(v),
                TaskOutcome::Error(e, span) => {
                    if g.failure.is_none() {
                        g.failure = Some((e, span));
                        let siblings: Vec<ScopeId> = g.children.values().map(|(_, s)| *s).collect();
                        for s in siblings {
                            self.abort_scope(s, AbortReason::Cancelled);
                        }
                    }
                }
                TaskOutcome::Abort(_) => {}
            }
        }
        let parent_aborted = {
            let g = self.groups.get(&gid).unwrap();
            self.abort_of(g.parent_scope).is_some()
        };
        let can_start = {
            let g = self.groups.get(&gid).unwrap();
            g.failure.is_none() && !parent_aborted && g.next < g.items.len()
        };
        if can_start {
            if let Err(r) = self.start_child(gid, budget) {
                let scope = self.budgets.get(&budget).map(|b| b.scope).unwrap_or(self.root_scope);
                self.abort_scope(scope, r);
            }
        }
        let g = self.groups.get(&gid).unwrap();
        if g.active == 0 && (g.failure.is_some() || parent_aborted || g.next >= g.items.len()) {
            let g = self.groups.remove(&gid).unwrap();
            let ctl = if let Some(sig) = self.abort_of(g.parent_scope) {
                Ctl::Abort(sig, None)
            } else if let Some((e, span)) = g.failure {
                if let Some(p) = self.tasks.get_mut(&g.parent) {
                    p.throw_span = span;
                }
                Ctl::Throw(e)
            } else {
                Ctl::Ret(Value::array(g.results.into_iter().map(|v| v.unwrap()).collect()))
            };
            if let Some(p) = self.tasks.get_mut(&g.parent) {
                p.ctl = ctl;
                self.ready.push_back(g.parent);
            }
        }
    }

    /// Check memory quota; returns an abort reason when exceeded.
    pub fn memory_exceeded(&self) -> bool {
        let live = crate::alloc::live_bytes();
        live.saturating_sub(self.memory_baseline) as u64 > self.config.limits.memory_bytes
    }

    pub fn now_ms(&self) -> u64 {
        self.host.now_ms()
    }

    /// Real-time deadline check before heavy work and before delivering a
    /// final result: fire due timers and report an observed abort.
    pub fn deadline_due(&mut self, t: &mut Task) -> Option<Ctl> {
        if self.host.is_virtual() {
            return None;
        }
        let now = self.host.now_ms();
        if self.next_timer_at().is_some_and(|at| at <= now) {
            self.fire_due_timers(now);
            t.seen_epoch = u64::MAX;
        }
        self.task_abort(t).map(|s| Ctl::Abort(s, None))
    }

    /// Arm builtin-work checkpoints for a task (earliest deadline on its
    /// scope chain in real time; memory quota always).
    pub fn arm_interrupts(&self, t: &Task) {
        self.arm_interrupts_until(t, None);
    }

    /// As `arm_interrupts`, additionally bounded by an operation deadline
    /// (absolute monotonic ms), e.g. an `sql` timeout covering result decoding.
    pub fn arm_interrupts_until(&self, t: &Task, op_deadline: Option<u64>) {
        let mem = Some((self.memory_baseline, self.config.limits.memory_bytes));
        let at = if self.host.is_virtual() {
            None
        } else {
            let mut cur = Some(t.scope);
            let mut earliest: Option<u64> = op_deadline;
            while let Some(s) = cur {
                match self.scopes.get(&s) {
                    Some(sc) => {
                        if let Some(d) = sc.deadline {
                            earliest = Some(earliest.map_or(d, |e| e.min(d)));
                        }
                        cur = sc.parent;
                    }
                    None => break,
                }
            }
            earliest.map(|d| {
                let now = self.host.now_ms();
                std::time::Instant::now() + std::time::Duration::from_millis(d.saturating_sub(now))
            })
        };
        crate::interrupt::arm(at, mem);
    }

    /// Disarm; when a checkpoint tripped, return the abort that replaces the
    /// (discarded) result.
    pub fn disarm_interrupts(&mut self, t: &mut Task) -> Option<Ctl> {
        let trip = crate::interrupt::disarm()?;
        match trip {
            crate::interrupt::Trip::Deadline => {
                let now = self.host.now_ms();
                self.fire_due_timers(now);
            }
            crate::interrupt::Trip::Memory => {
                let scope = self.budgets.get(&t.budget).map(|b| b.scope).unwrap_or(self.root_scope);
                self.abort_scope(scope, AbortReason::Resource("memory"));
            }
            crate::interrupt::Trip::Cancelled => {
                let root = self.root_scope;
                self.abort_scope(root, AbortReason::Cancelled);
            }
        }
        t.seen_epoch = u64::MAX;
        self.abort_of(t.scope).map(|sig| Ctl::Abort(sig, None))
    }
}

pub enum StepResult {
    Continue,
    Suspended,
    Done(TaskOutcome),
}

pub enum TaskOutcome {
    Value(Value),
    Error(Value, Option<Span>),
    Abort(AbortSig),
}

pub fn abort_data(sig: &AbortSig) -> Value {
    match &sig.reason {
        AbortReason::Resource(r) => Value::rec(vec![("resource", Value::str(r))]),
        AbortReason::Cancelled => Value::rec(vec![("scope", Value::str("root"))]),
        AbortReason::Deadline | AbortReason::TxTimeout(_) => Value::rec(vec![("scope", Value::str("root"))]),
    }
}

pub fn timeout_error(op: &str, ms: u64, outcome: Option<&str>) -> Value {
    let mut data = vec![("operation", Value::str(op)), ("timeout", Value::int(ms as i64))];
    if let Some(o) = outcome {
        data.push(("outcome", Value::str(o)));
    }
    err::make("timeout", format!("{op} timed out after {ms} ms"), data)
}
