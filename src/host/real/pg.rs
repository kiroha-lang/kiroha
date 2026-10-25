//! PostgreSQL adapter (placeholder until step 6).
use crate::host::config::HostConfig;
use crate::host::{EffOutcome, SqlParam};
use std::rc::Rc;
fn e() -> EffOutcome { EffOutcome::Err(crate::builtins::db::db_error("pool", "not_sent", vec![])) }
pub struct Pools { _c: Rc<HostConfig> }
impl Pools {
    pub fn new(c: Rc<HostConfig>) -> Pools { Pools { _c: c } }
    pub fn pool(&mut self, _a: &str) -> EffOutcome { e() }
    pub fn acquire(&mut self, _p: String) -> impl std::future::Future<Output = EffOutcome> { async { e() } }
    pub fn simple(&mut self, _c: String, _s: &str, _p: &str) -> impl std::future::Future<Output = EffOutcome> { async { e() } }
    pub fn commit(&mut self, _c: String) -> impl std::future::Future<Output = EffOutcome> { async { e() } }
    pub fn release(&mut self, _c: String, _d: bool) -> impl std::future::Future<Output = EffOutcome> { async { e() } }
    pub fn execute(&mut self, _c: String, _s: String, _p: Vec<SqlParam>) -> impl std::future::Future<Output = EffOutcome> { async { e() } }
}
