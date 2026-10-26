//! Host adapter boundary (spec §14.2).
//!
//! The runtime owns language semantics; a [`Host`] performs I/O. Effects are
//! dispatched with an id and complete later as [`HostEvent::Done`]. Every
//! effect is cancellable: `cancel` eventually produces `EffOutcome::Joined`
//! unless the ordinary completion was already delivered.

pub mod config;
pub mod paths;
pub mod real;
pub mod virtual_host;

use crate::value::Value;
use std::future::Future;

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    /// Lowercase names, trimmed values, in order.
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub header_limit: u64,
    /// Decoded-body limit; the adapter bounds raw identity bodies by it.
    pub max_bytes: u64,
    pub max_bytes_wide: bool,
}

#[derive(Clone, Debug)]
pub struct RawResponse {
    pub status: u16,
    /// Header names as received; values as raw octets.
    pub headers: Vec<(String, Vec<u8>)>,
    /// Transfer-decoded, content-encoded body octets.
    pub body: Vec<u8>,
}

pub use crate::database::{RawRows, SqlParam};

#[derive(Clone, Debug)]
pub enum Effect {
    Http(HttpRequest),
    Environment(String),
    ReadRegular { path: String, max_bytes: u64 },
    ReplaceRegular { path: String, bytes: Vec<u8> },
    LogLine(String),
    UtcNow,
    RandomBytes(usize),
    Pool(String),
    Acquire(String),
    Begin(String),
    /// `max_rows`: the adapter may stop reading after max_rows + 1 rows.
    Execute { connection: String, sql: String, params: Vec<SqlParam>, max_rows: u64 },
    Commit(String),
    Rollback(String),
    Release { connection: String, discard: bool },
    Sleep(u64),
    Listen { host: String, port: u16 },
    ReadBody { request: u64, max_body: u64, max_body_wide: bool },
    SendResponse { request: u64, status: u16, headers: Vec<(String, String)>, body: Vec<u8>, suppress_body: bool },
    CloseRequest { request: u64 },
    CloseListener { listener: String },
}

impl Effect {
    pub fn op_name(&self) -> &'static str {
        match self {
            Effect::Http(_) => "http",
            Effect::Environment(_) => "environment",
            Effect::ReadRegular { .. } => "read_regular",
            Effect::ReplaceRegular { .. } => "replace_regular",
            Effect::LogLine(_) => "log_line",
            Effect::UtcNow => "utc_now",
            Effect::RandomBytes(_) => "random_bytes",
            Effect::Pool(_) => "pool",
            Effect::Acquire(_) => "acquire",
            Effect::Begin(_) => "begin",
            Effect::Execute { .. } => "execute",
            Effect::Commit(_) => "commit",
            Effect::Rollback(_) => "rollback",
            Effect::Release { .. } => "release",
            Effect::Sleep(_) => "sleep",
            Effect::Listen { .. } => "listen",
            Effect::ReadBody { .. } => "read_body",
            Effect::SendResponse { .. } => "send_response",
            Effect::CloseRequest { .. } => "close_request",
            Effect::CloseListener { .. } => "close_listener",
        }
    }
}

#[derive(Clone, Debug)]
pub enum EffValue {
    Null,
    Str(String),
    Bytes(Vec<u8>),
    UnixMs(i128),
    Http(RawResponse),
    Rows(RawRows),
    /// Request body read result: Ok(bytes) or Err(reason) for an ingress fault.
    Body(Result<Vec<u8>, BodyFault>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyFault {
    TooLarge,
    BadFraming,
}

#[derive(Clone, Debug)]
pub enum EffOutcome {
    Ok(EffValue),
    /// Ordinary error record `{code,message,data}`.
    Err(Value),
    /// Cancellation acknowledged; no ordinary result.
    Joined,
    /// Cancellation acknowledged and the adapter proved no application bytes
    /// were sent (normalized to `Joined` except for operation timeouts).
    JoinedNotSent,
    /// The host terminated the operation for exceeding a host quota.
    Resource(String),
}

/// An incoming server request head (already framed).
#[derive(Clone, Debug)]
pub struct IncomingRequest {
    pub listener: String,
    pub id: u64,
    /// Trace label (virtual) or numeric id.
    pub label: String,
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, Vec<u8>)>,
    /// Host-detected transport fault (e.g. upgrade attempt), if any.
    pub upgrade: bool,
    /// Header-level body framing is invalid (conflicting Content-Length,
    /// Transfer-Encoding with Content-Length, unsupported coding, ...).
    pub framing_invalid: bool,
}

#[derive(Debug)]
pub enum HostEvent {
    Done(u64, EffOutcome),
    Request(IncomingRequest),
    Disconnect(u64),
    /// SIGINT/SIGTERM or harness cancellation of the root.
    CancelRoot,
}

pub trait Host {
    /// Share the evaluator's allocation baseline with buffered transports.
    fn set_memory_budget(&mut self, _baseline: usize, _limit: u64) {}
    /// Monotonic milliseconds since runtime start.
    fn now_ms(&self) -> u64;
    fn dispatch(&mut self, id: u64, eff: Effect);
    fn cancel(&mut self, id: u64);
    /// Wait for the next host event, or until the absolute time `until`.
    /// Returns `None` when `until` was reached first (or, for a host that
    /// cannot make progress, when nothing can ever happen).
    fn wait(&mut self, until: Option<u64>) -> impl Future<Output = Option<HostEvent>>;
    /// True when an indefinite wait with nothing pending is a deadlock.
    fn is_virtual(&self) -> bool;
    /// Diagnostic sink (stderr for the CLI); best effort.
    fn diagnostic(&mut self, line: &str);
    /// Stop accepting new connections on a listener (serve cancellation);
    /// accepted requests continue until joined.
    fn stop_accepting(&mut self, _listener: &str) {}
    /// Close a request transport that will never be admitted (no task).
    fn drop_request(&mut self, _request: u64) {}
    /// A harness failure that must stop the run (fixture host only).
    fn failed(&self) -> bool {
        false
    }
}
