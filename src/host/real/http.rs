//! HTTP/1.1 client adapter (placeholder until step 6).
use crate::host::{EffOutcome, HttpRequest};
pub struct Client;
impl Client {
    pub fn new() -> Client { Client }
    pub async fn request(&self, _req: HttpRequest) -> EffOutcome {
        EffOutcome::Err(crate::builtins::effects::network_error("unimplemented", "not_sent"))
    }
}
