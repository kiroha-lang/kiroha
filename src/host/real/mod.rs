//! Production host: tokio timers, local effects, HTTP, PostgreSQL, server.

pub mod files;
pub mod http;
pub mod http1;
pub mod pg;
pub mod mysql;
pub mod database;
pub mod server;

use super::config::HostConfig;
use super::{EffOutcome, EffValue, Effect, Host, HostEvent};
use crate::value::{err, Value};
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct RealHost {
    start: Instant,
    pub config: Rc<HostConfig>,
    tx: mpsc::UnboundedSender<HostEvent>,
    rx: mpsc::UnboundedReceiver<HostEvent>,
    tokens: HashMap<u64, CancellationToken>,
    pub databases: database::Pools,
    pub servers: server::Servers,
    http: Rc<http::Client>,
}

impl RealHost {
    pub fn new(config: Rc<HostConfig>) -> RealHost {
        let (tx, rx) = mpsc::unbounded_channel();
        let databases = database::Pools::new(config.clone());
        RealHost {
            start: Instant::now(),
            config,
            tx: tx.clone(),
            rx,
            tokens: HashMap::new(),
            databases,
            servers: server::Servers::new(tx),
            http: Rc::new(http::Client::new()),
        }
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<HostEvent> {
        self.tx.clone()
    }

    /// Spawn an operation future; it reports `Done` or `Joined` on cancel.
    fn spawn<F>(&mut self, id: u64, fut: F)
    where
        F: std::future::Future<Output = EffOutcome> + 'static,
    {
        let token = CancellationToken::new();
        self.tokens.insert(id, token.clone());
        let tx = self.tx.clone();
        let fut = guarded(fut);
        tokio::task::spawn_local(async move {
            let out = tokio::select! {
                biased;
                _ = token.cancelled() => EffOutcome::Joined,
                o = fut => o,
            };
            let _ = tx.send(HostEvent::Done(id, out));
        });
    }

    /// Blocking file work with cooperative cancellation: on cancel the flag is
    /// set but the operation is still awaited, so the effect is reported done
    /// only after the blocking thread stopped (nothing applies after an abort).
    fn spawn_file_op<F, Fut>(&mut self, id: u64, make: F)
    where
        F: FnOnce(files::Cancel) -> Fut,
        Fut: std::future::Future<Output = EffOutcome> + 'static,
    {
        let flag = files::Cancel::default();
        let token = CancellationToken::new();
        self.tokens.insert(id, token.clone());
        let tx = self.tx.clone();
        let work = guarded(make(flag.clone()));
        tokio::task::spawn_local(async move {
            let mut work = Box::pin(work);
            let out = tokio::select! {
                biased;
                _ = token.cancelled() => {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    // An already-applied replacement keeps its real outcome.
                    work.await
                }
                o = &mut work => o,
            };
            let _ = tx.send(HostEvent::Done(id, out));
        });
    }

    fn done_now(&mut self, id: u64, out: EffOutcome) {
        let _ = self.tx.send(HostEvent::Done(id, out));
    }
}

/// Isolate an adapter future: a panic inside it becomes a host resource
/// outcome instead of an operation that never completes (§14.2).
fn guarded<F>(fut: F) -> impl std::future::Future<Output = EffOutcome>
where
    F: std::future::Future<Output = EffOutcome>,
{
    use futures_util::FutureExt;
    async move {
        match std::panic::AssertUnwindSafe(fut).catch_unwind().await {
            Ok(o) => o,
            Err(_) => EffOutcome::Resource("host".into()),
        }
    }
}

/// Bound a scheduler wake interval; callers re-check the absolute deadline.
fn clamp_ms(ms: u64) -> std::time::Duration {
    std::time::Duration::from_millis(ms.min(30 * 365 * 24 * 3_600_000))
}


fn io_err(op: &str, reason: &str, outcome: &str) -> Value {
    err::make(
        "io",
        format!("{op}: {reason}"),
        vec![("operation", Value::str(op)), ("reason", Value::str(reason)), ("outcome", Value::str(outcome))],
    )
}

impl Host for RealHost {
    fn set_memory_budget(&mut self, baseline: usize, limit: u64) {
        let memory = http1::MemoryBudget { baseline, limit };
        self.http.memory.set(Some(memory));
        self.servers.memory = Some(memory);
    }
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn dispatch(&mut self, id: u64, eff: Effect) {
        match eff {
            Effect::Sleep(ms) => self.spawn(id, async move {
                let Some(at) = tokio::time::Instant::now().checked_add(std::time::Duration::from_millis(ms)) else {
                    return EffOutcome::Resource("clock".into());
                };
                tokio::time::sleep_until(at).await;
                EffOutcome::Ok(EffValue::Null)
            }),
            Effect::Environment(name) => {
                let out = match self.config.env_value(&name) {
                    Some(v) => EffOutcome::Ok(EffValue::Str(v.to_string())),
                    None => EffOutcome::Err(err::make(
                        "env",
                        format!("environment variable {name} is not configured"),
                        vec![("name", Value::str(&name))],
                    )),
                };
                self.done_now(id, out);
            }
            Effect::UtcNow => {
                let out = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => EffOutcome::Ok(EffValue::UnixMs(d.as_millis() as i128)),
                    Err(_) => EffOutcome::Err(io_err("now", "clock_out_of_range", "not_applied")),
                };
                self.done_now(id, out);
            }
            Effect::RandomBytes(n) => {
                let mut buf = vec![0u8; n];
                let out = match getrandom::getrandom(&mut buf) {
                    Ok(()) => EffOutcome::Ok(EffValue::Bytes(buf)),
                    Err(_) => EffOutcome::Err(io_err("uuid", "random_source", "not_applied")),
                };
                self.done_now(id, out);
            }
            Effect::LogLine(line) => {
                let mut e = std::io::stderr().lock();
                let out = match e.write_all(line.as_bytes()).and_then(|_| e.flush()) {
                    Ok(()) => EffOutcome::Ok(EffValue::Null),
                    Err(_) => EffOutcome::Err(io_err("log", "sink", "unknown")),
                };
                self.done_now(id, out);
            }
            Effect::ReadRegular { path, max_bytes } => {
                self.spawn_file_op(id, move |c| async move { files::read_regular(path, max_bytes, c).await });
            }
            Effect::ReplaceRegular { path, bytes } => {
                self.spawn_file_op(id, move |c| async move { files::replace_regular(path, bytes, c).await });
            }
            Effect::Http(req) => {
                let client = self.http.clone();
                let sent = Rc::new(std::cell::Cell::new(false));
                let flag = sent.clone();
                let request = guarded(async move { client.request(req, sent).await });
                let token = CancellationToken::new();
                self.tokens.insert(id, token.clone());
                let tx = self.tx.clone();
                tokio::task::spawn_local(async move {
                    let out = tokio::select! {
                        biased;
                        _ = token.cancelled() => if flag.get() { EffOutcome::Joined } else { EffOutcome::JoinedNotSent },
                        o = request => o,
                    };
                    let _ = tx.send(HostEvent::Done(id, out));
                });
            }
            Effect::Pool(alias) => {
                let out = self.databases.pool(&alias);
                self.done_now(id, out);
            }
            Effect::Acquire(pool) => {
                let fut = self.databases.acquire(pool);
                self.spawn(id, fut);
            }
            Effect::Begin(c) => {
                let fut = self.databases.transaction(c, database::TransactionOp::Begin);
                self.spawn(id, fut);
            }
            Effect::Commit(c) => {
                let fut = self.databases.transaction(c, database::TransactionOp::Commit);
                self.spawn(id, fut);
            }
            Effect::Rollback(c) => {
                let fut = self.databases.transaction(c, database::TransactionOp::Rollback);
                self.spawn(id, fut);
            }
            Effect::Release { connection, discard } => {
                let fut = self.databases.release(connection, discard);
                self.spawn(id, fut);
            }
            Effect::Execute { connection, sql, params, max_rows } => {
                // Each driver cancels server-side work before cleanup continues.
                let cancel = self.databases.cancel(&connection);
                let fut = guarded(self.databases.execute(connection, sql, params, max_rows));
                let token = CancellationToken::new();
                self.tokens.insert(id, token.clone());
                let tx = self.tx.clone();
                tokio::task::spawn_local(async move {
                    let out = tokio::select! {
                        biased;
                        _ = token.cancelled() => {
                            cancel.await;
                            EffOutcome::Joined
                        }
                        o = fut => o,
                    };
                    let _ = tx.send(HostEvent::Done(id, out));
                });
            }
            Effect::Listen { host, port } => {
                let header_limit = self.config.limits.header_bytes;
                let out = self.servers.listen(&host, port, header_limit);
                self.done_now(id, out);
            }
            Effect::ReadBody { request, max_body, max_body_wide } => {
                let fut = self.servers.read_body(request, max_body, max_body_wide);
                self.spawn(id, fut);
            }
            Effect::SendResponse { request, status, headers, body, suppress_body } => {
                let fut = self.servers.send(request, status, headers, body, suppress_body);
                self.spawn(id, fut);
            }
            Effect::CloseRequest { request } => {
                self.servers.close_request(request);
                self.done_now(id, EffOutcome::Ok(EffValue::Null));
            }
            Effect::CloseListener { listener } => {
                self.servers.close_listener(&listener);
                self.done_now(id, EffOutcome::Ok(EffValue::Null));
            }
        }
    }

    fn cancel(&mut self, id: u64) {
        if let Some(t) = self.tokens.remove(&id) {
            t.cancel();
        }
    }

    async fn wait(&mut self, until: Option<u64>) -> Option<HostEvent> {
        let ev = match until {
            Some(u) => {
                let now = self.now_ms();
                if u <= now {
                    match self.rx.try_recv() {
                        Ok(ev) => Some(ev),
                        Err(_) => {
                            tokio::task::yield_now().await;
                            self.rx.try_recv().ok()
                        }
                    }
                } else {
                    let d = clamp_ms(u - now);
                    tokio::time::timeout(d, self.rx.recv()).await.ok().flatten()
                }
            }
            None => self.rx.recv().await,
        };
        if let Some(HostEvent::Done(id, _)) = &ev {
            self.tokens.remove(id);
        }
        ev
    }

    fn is_virtual(&self) -> bool {
        false
    }

    fn stop_accepting(&mut self, listener: &str) {
        self.servers.stop_accepting(listener);
    }

    fn drop_request(&mut self, request: u64) {
        self.servers.close_request(request);
    }

    fn diagnostic(&mut self, line: &str) {
        let mut e = std::io::stderr().lock();
        let _ = writeln!(e, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn long_sleep_is_not_shortened_to_the_scheduler_wake_interval() {
        tokio::task::LocalSet::new().run_until(async {
            let mut host = RealHost::new(Rc::new(HostConfig::default_with_root("/".into())));
            let year_ms = 365 * 24 * 3_600_000u64;
            host.dispatch(1, Effect::Sleep(31 * year_ms));
            tokio::task::yield_now().await;
            tokio::time::advance(std::time::Duration::from_millis(30 * year_ms)).await;
            tokio::task::yield_now().await;
            assert!(matches!(host.rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
            tokio::time::advance(std::time::Duration::from_millis(year_ms)).await;
            tokio::task::yield_now().await;
            assert!(matches!(host.rx.try_recv(), Ok(HostEvent::Done(1, EffOutcome::Ok(EffValue::Null)))));
        }).await;
    }
}
