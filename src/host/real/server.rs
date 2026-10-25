//! HTTP/1.1 server adapter (placeholder until step 6).
use crate::host::{EffOutcome, HostEvent};
use tokio::sync::mpsc;
pub struct Servers { _tx: mpsc::UnboundedSender<HostEvent> }
fn e() -> EffOutcome { EffOutcome::Err(crate::value::err::make("io", "unimplemented", vec![])) }
impl Servers {
    pub fn new(tx: mpsc::UnboundedSender<HostEvent>) -> Servers { Servers { _tx: tx } }
    pub fn listen(&mut self, _h: &str, _p: u16, _l: u64) -> EffOutcome { e() }
    pub fn read_body(&mut self, _r: u64, _m: u64) -> impl std::future::Future<Output = EffOutcome> { async { e() } }
    pub fn send(&mut self, _r: u64, _s: u16, _h: Vec<(String, String)>, _b: Vec<u8>, _x: bool) -> impl std::future::Future<Output = EffOutcome> { async { e() } }
    pub fn close_request(&mut self, _r: u64) {}
    pub fn close_listener(&mut self, _l: &str) {}
}
