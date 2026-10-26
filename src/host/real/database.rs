//! Shared pool leases, transaction operations and adapter interface.
use crate::builtins::db::db_error;
use crate::database::{Backend, SqlParam};
use crate::host::config::HostConfig;
use crate::host::{EffOutcome, EffValue};
use futures_util::future::LocalBoxFuture;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tokio::sync::Notify;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransactionOp {
    Begin,
    Commit,
    Rollback,
}
impl TransactionOp {
    pub fn phase(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Commit => "commit",
            Self::Rollback => "rollback",
        }
    }
}
#[derive(Clone, Copy)]
pub struct RowBudget {
    pub baseline: usize,
    pub limit: u64,
}

/// Connections own protocol details; the pool owns all lease accounting.
pub trait Connection {
    fn execute(
        &self,
        sql: String,
        params: Vec<SqlParam>,
        max_rows: u64,
        in_tx: bool,
        budget: RowBudget,
    ) -> LocalBoxFuture<'static, EffOutcome>;
    fn transaction(&self, op: TransactionOp, in_tx: bool) -> LocalBoxFuture<'static, EffOutcome>;
    fn reset(&self) -> LocalBoxFuture<'static, bool>;
    fn cancel(&self) -> LocalBoxFuture<'static, ()>;
}
struct Lease {
    connection: Rc<dyn Connection>,
    pool: String,
    in_tx: bool,
}
struct PoolState {
    backend: Backend,
    dsn: String,
    max: u64,
    open: u64,
    idle: Vec<String>,
    notify: Rc<Notify>,
}
#[derive(Default)]
struct Inner {
    pools: HashMap<String, PoolState>,
    leases: HashMap<String, Lease>,
    next: u64,
}
struct SlotGuard {
    inner: Rc<RefCell<Inner>>,
    pool: String,
    lease: Option<String>,
    keep: bool,
}
impl Drop for SlotGuard {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        let mut i = self.inner.borrow_mut();
        if let Some(id) = &self.lease {
            i.leases.remove(id);
        }
        if let Some(p) = i.pools.get_mut(&self.pool) {
            p.open = p.open.saturating_sub(1);
            p.notify.notify_one();
        }
    }
}
pub struct Pools {
    config: Rc<HostConfig>,
    inner: Rc<RefCell<Inner>>,
    budget: RowBudget,
}
fn error(phase: &str) -> EffOutcome {
    EffOutcome::Err(db_error(phase, "not_sent", vec![]))
}
async fn connect(backend: Backend, dsn: &str) -> Result<Rc<dyn Connection>, EffOutcome> {
    let connection: Rc<dyn Connection> = match backend {
        Backend::PostgreSql => Rc::new(super::pg::PgConnection::connect(dsn).await?),
        Backend::MySql | Backend::MariaDb => {
            Rc::new(super::mysql::MySqlConnection::connect(dsn).await?)
        }
    };
    Ok(connection)
}
impl Pools {
    pub fn new(config: Rc<HostConfig>) -> Self {
        let budget = RowBudget {
            baseline: crate::alloc::live_bytes(),
            limit: config.limits.memory_bytes,
        };
        Self {
            config,
            inner: Rc::new(RefCell::new(Inner::default())),
            budget,
        }
    }
    pub fn pool(&mut self, alias: &str) -> EffOutcome {
        let Some((_, cfg)) = self.config.database_pools.iter().find(|(a, _)| a == alias) else {
            return error("pool");
        };
        let Ok(dsn) = std::env::var(&cfg.dsn_env) else {
            return error("pool");
        };
        self.inner
            .borrow_mut()
            .pools
            .entry(alias.into())
            .or_insert_with(|| PoolState {
                backend: cfg.backend,
                dsn,
                max: cfg.max_connections,
                open: 0,
                idle: Vec::new(),
                notify: Rc::new(Notify::new()),
            });
        EffOutcome::Ok(EffValue::Str(alias.into()))
    }
    pub fn acquire(&mut self, pool: String) -> LocalBoxFuture<'static, EffOutcome> {
        let inner = self.inner.clone();
        Box::pin(async move {
            loop {
                let (target, notify) = {
                    let mut i = inner.borrow_mut();
                    let Some(p) = i.pools.get_mut(&pool) else {
                        return error("pool");
                    };
                    if let Some(id) = p.idle.pop() {
                        return EffOutcome::Ok(EffValue::Str(id));
                    }
                    let target = if p.open < p.max {
                        p.open += 1;
                        Some((p.backend, p.dsn.clone()))
                    } else {
                        None
                    };
                    (target, p.notify.clone())
                };
                let Some((backend, dsn)) = target else {
                    notify.notified().await;
                    continue;
                };
                let mut slot = SlotGuard {
                    inner: inner.clone(),
                    pool: pool.clone(),
                    lease: None,
                    keep: false,
                };
                let connection = match connect(backend, &dsn).await {
                    Ok(c) => c,
                    Err(e) => return e,
                };
                let mut i = inner.borrow_mut();
                i.next += 1;
                let id = format!("{pool}#{}", i.next);
                i.leases.insert(
                    id.clone(),
                    Lease {
                        connection,
                        pool: pool.clone(),
                        in_tx: false,
                    },
                );
                slot.keep = true;
                return EffOutcome::Ok(EffValue::Str(id));
            }
        })
    }
    fn lease(&self, id: &str) -> Option<(Rc<dyn Connection>, bool)> {
        self.inner
            .borrow()
            .leases
            .get(id)
            .map(|l| (l.connection.clone(), l.in_tx))
    }
    pub fn execute(
        &mut self,
        id: String,
        sql: String,
        params: Vec<SqlParam>,
        max_rows: u64,
    ) -> LocalBoxFuture<'static, EffOutcome> {
        match self.lease(&id) {
            Some((c, in_tx)) => c.execute(sql, params, max_rows, in_tx, self.budget),
            None => Box::pin(async { error("execute") }),
        }
    }
    pub fn transaction(
        &mut self,
        id: String,
        op: TransactionOp,
    ) -> LocalBoxFuture<'static, EffOutcome> {
        let inner = self.inner.clone();
        let lease = self.lease(&id);
        Box::pin(async move {
            let Some((c, in_tx)) = lease else {
                return error(op.phase());
            };
            let result = c.transaction(op, in_tx).await;
            if matches!(result, EffOutcome::Ok(_)) {
                if let Some(l) = inner.borrow_mut().leases.get_mut(&id) {
                    l.in_tx = op == TransactionOp::Begin;
                }
            }
            result
        })
    }
    pub fn release(&mut self, id: String, discard: bool) -> LocalBoxFuture<'static, EffOutcome> {
        let inner = self.inner.clone();
        let lease = inner
            .borrow()
            .leases
            .get(&id)
            .map(|l| (l.connection.clone(), l.pool.clone()));
        // Arm the lease before polling: cancellation can win immediately.
        let mut slot = lease.as_ref().map(|(_, pool)| SlotGuard {
            inner: inner.clone(),
            pool: pool.clone(),
            lease: Some(id.clone()),
            keep: false,
        });
        Box::pin(async move {
            if let Some((c, pool)) = lease {
                if !discard && c.reset().await {
                    let mut i = inner.borrow_mut();
                    if let Some(l) = i.leases.get_mut(&id) {
                        l.in_tx = false;
                    }
                    if let Some(p) = i.pools.get_mut(&pool) {
                        p.idle.push(id);
                        p.notify.notify_one();
                        slot.as_mut().unwrap().keep = true;
                    }
                }
            }
            EffOutcome::Ok(EffValue::Null)
        })
    }
    pub fn cancel(&self, id: &str) -> LocalBoxFuture<'static, ()> {
        self.lease(id).map_or_else(
            || Box::pin(async {}) as LocalBoxFuture<'static, ()>,
            |(c, _)| c.cancel(),
        )
    }
}
