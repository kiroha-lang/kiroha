//! HTTP/1.1 server adapter (spec §12). Raw request targets and header pairs
//! reach the runtime unnormalized; framing is decoded here, while admission,
//! routing and responses are runtime decisions. One request per connection.

use super::http1::{self, ChunkErr};
use crate::host::{BodyFault, EffOutcome, EffValue, HostEvent, IncomingRequest};
use crate::value::{err, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const HEADER_TIMEOUT_MS: u64 = 30_000;

#[derive(Clone, Copy, Debug)]
enum Framing {
    Empty,
    Length(u128),
    Chunked,
    Invalid,
}

struct ConnState {
    read: Option<OwnedReadHalf>,
    write: Option<OwnedWriteHalf>,
    buf: Vec<u8>,
    framing: Framing,
    watcher: Option<CancellationToken>,
    header_limit: u64,
}

#[derive(Default)]
struct Inner {
    listeners: HashMap<String, CancellationToken>,
    conns: HashMap<u64, ConnState>,
    next_req: u64,
    next_listener: u64,
}

pub struct Servers {
    tx: mpsc::UnboundedSender<HostEvent>,
    inner: Rc<RefCell<Inner>>,
    pub memory: Option<http1::MemoryBudget>,
}

fn io_err(reason: &str, outcome: &str) -> EffOutcome {
    EffOutcome::Err(err::make(
        "io",
        format!("serve: {reason}"),
        vec![("operation", Value::str("serve")), ("reason", Value::str(reason)), ("outcome", Value::str(outcome))],
    ))
}

fn framing_of(headers: &[(String, Vec<u8>)]) -> Framing {
    let field = |name: &'static str| headers.iter().filter(move |(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_slice());
    let te: Vec<String> = field("transfer-encoding")
        .flat_map(|v| String::from_utf8_lossy(v).split(',').map(|t| t.trim().to_ascii_lowercase()).collect::<Vec<_>>())
        .filter(|t| !t.is_empty())
        .collect();
    let cl_present = field("content-length").next().is_some();
    if field("transfer-encoding").next().is_some() {
        // Transfer-Encoding overrides Content-Length and indicates smuggling
        // when both are present (RFC 9112 §6.3): reject.
        if cl_present || te != ["chunked"] {
            return Framing::Invalid;
        }
        return Framing::Chunked;
    }
    match http1::content_length(field("content-length")) {
        Err(()) => Framing::Invalid,
        Ok(None) => Framing::Empty,
        Ok(Some(n)) => Framing::Length(n),
    }
}

impl Servers {
    pub fn new(tx: mpsc::UnboundedSender<HostEvent>) -> Servers {
        Servers { tx, inner: Rc::new(RefCell::new(Inner::default())), memory: None }
    }

    pub fn listen(&mut self, host: &str, port: u16, header_limit: u64) -> EffOutcome {
        let addr = match host.parse::<std::net::IpAddr>() {
            Ok(ip) => std::net::SocketAddr::new(ip, port),
            Err(_) => return io_err("bind", "not_applied"),
        };
        let std_listener = match std::net::TcpListener::bind(addr) {
            Ok(l) => l,
            Err(_) => return io_err("bind", "not_applied"),
        };
        if std_listener.set_nonblocking(true).is_err() {
            return io_err("bind", "not_applied");
        }
        let listener = match TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(_) => return io_err("bind", "not_applied"),
        };
        let token = CancellationToken::new();
        let id = {
            let mut i = self.inner.borrow_mut();
            i.next_listener += 1;
            let id = format!("listener-{}", i.next_listener);
            i.listeners.insert(id.clone(), token.clone());
            id
        };
        let inner = self.inner.clone();
        let tx = self.tx.clone();
        let lid = id.clone();
        let memory = self.memory;
        tokio::task::spawn_local(async move {
            loop {
                let accepted = tokio::select! {
                    _ = token.cancelled() => break,
                    a = listener.accept() => a,
                };
                if let Ok((stream, _)) = accepted {
                    let inner = inner.clone();
                    let tx = tx.clone();
                    let lid = lid.clone();
                    let live = token.clone();
                    tokio::task::spawn_local(async move {
                        handle_conn(stream, inner, tx, lid, header_limit, live, memory).await;
                    });
                }
            }
        });
        EffOutcome::Ok(EffValue::Str(id))
    }

    pub fn read_body(&mut self, request: u64, max_body: u64, max_body_wide: bool) -> impl Future<Output = EffOutcome> {
        let inner = self.inner.clone();
        let tx = self.tx.clone();
        let memory = self.memory;
        async move {
            let (mut rd, mut buf, framing, header_limit) = {
                let mut i = inner.borrow_mut();
                match i.conns.get_mut(&request) {
                    Some(c) => match c.read.take() {
                        Some(r) => (r, std::mem::take(&mut c.buf), c.framing, c.header_limit),
                        None => return EffOutcome::Ok(EffValue::Body(Err(BodyFault::BadFraming))),
                    },
                    None => return EffOutcome::Ok(EffValue::Body(Err(BodyFault::BadFraming))),
                }
            };
            let result = match framing {
                Framing::Empty => Ok(Vec::new()),
                Framing::Invalid => Err(BodyFault::BadFraming),
                Framing::Length(n) => {
                    if n > max_body as u128 {
                        if max_body_wide { return EffOutcome::Resource("transport".into()); }
                        Err(BodyFault::TooLarge)
                    } else {
                        let mut ok = true;
                        while (buf.len() as u128) < n {
                            let mut chunk = vec![0u8; 16384];
                            match rd.read(&mut chunk).await {
                                Ok(0) | Err(_) => {
                                    ok = false;
                                    break;
                                }
                                Ok(k) => buf.extend_from_slice(&chunk[..k]),
                            }
                        }
                        if ok {
                            buf.truncate(n as usize);
                            Ok(buf)
                        } else {
                            Err(BodyFault::BadFraming)
                        }
                    }
                }
                Framing::Chunked => {
                    let mut out: Vec<u8> = Vec::new();
                    match http1::read_chunked_bounded::<_, ()>(&mut rd, buf, max_body, header_limit, memory, |d| {
                        out.extend_from_slice(d);
                        Ok(())
                    })
                    .await
                    {
                        Ok(()) => Ok(out),
                        Err(ChunkErr::TooLarge) if max_body_wide => return EffOutcome::Resource("transport".into()),
                        Err(ChunkErr::TooLarge) => Err(BodyFault::TooLarge),
                        Err(ChunkErr::Memory) => return EffOutcome::Resource("memory".into()),
                        Err(ChunkErr::TrailerTooLarge) => return EffOutcome::Resource("header_bytes".into()),
                        Err(_) => Err(BodyFault::BadFraming),
                    }
                }
            };
            // Watch for client disconnect while the handler runs.
            let token = CancellationToken::new();
            {
                let mut i = inner.borrow_mut();
                if let Some(c) = i.conns.get_mut(&request) {
                    c.watcher = Some(token.clone());
                }
            }
            tokio::task::spawn_local(async move {
                let mut probe = [0u8; 1024];
                loop {
                    tokio::select! {
                        _ = token.cancelled() => break,
                        r = rd.read(&mut probe) => match r {
                            // TCP cannot tell a half-close from a full close
                            // before writing; like Go's net/http, EOF while the
                            // handler runs is treated as a client disconnect.
                            Ok(0) | Err(_) => {
                                let _ = tx.send(HostEvent::Disconnect(request));
                                break;
                            }
                            Ok(_) => {}
                        }
                    }
                }
            });
            EffOutcome::Ok(EffValue::Body(result))
        }
    }

    pub fn send(
        &mut self,
        request: u64,
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        suppress_body: bool,
    ) -> impl Future<Output = EffOutcome> {
        let inner = self.inner.clone();
        async move {
            let state = inner.borrow_mut().conns.remove(&request);
            let mut st = match state {
                Some(s) => s,
                None => return io_err("disconnected", "unknown"),
            };
            if let Some(w) = st.watcher.take() {
                w.cancel();
            }
            let mut wr = match st.write.take() {
                Some(w) => w,
                None => return io_err("disconnected", "unknown"),
            };
            let reason = hyper::StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()).unwrap_or("");
            let mut out = format!("HTTP/1.1 {status} {reason}\r\n").into_bytes();
            for (n, v) in &headers {
                out.extend_from_slice(format!("{n}: {v}\r\n").as_bytes());
            }
            out.extend_from_slice(b"connection: close\r\n\r\n");
            if !suppress_body {
                out.extend_from_slice(&body);
            }
            let res = async {
                wr.write_all(&out).await?;
                wr.flush().await?;
                wr.shutdown().await
            }
            .await;
            match res {
                Ok(()) => EffOutcome::Ok(EffValue::Null),
                Err(_) => io_err("write", "unknown"),
            }
        }
    }

    pub fn stop_accepting(&mut self, listener: &str) {
        if let Some(t) = self.inner.borrow().listeners.get(listener) {
            t.cancel();
        }
    }

    pub fn close_request(&mut self, request: u64) {
        if let Some(mut c) = self.inner.borrow_mut().conns.remove(&request) {
            if let Some(w) = c.watcher.take() {
                w.cancel();
            }
        }
    }

    pub fn close_listener(&mut self, listener: &str) {
        let mut i = self.inner.borrow_mut();
        if let Some(t) = i.listeners.remove(listener) {
            t.cancel();
        }
        for (_, c) in i.conns.iter_mut() {
            if let Some(w) = c.watcher.take() {
                w.cancel();
            }
        }
        i.conns.clear();
    }
}

async fn handle_conn(
    stream: TcpStream,
    inner: Rc<RefCell<Inner>>,
    tx: mpsc::UnboundedSender<HostEvent>,
    listener: String,
    header_limit: u64,
    live: CancellationToken,
    memory: Option<http1::MemoryBudget>,
) {
    let _ = stream.set_nodelay(true);
    let (mut rd, mut wr) = stream.into_split();
    let mut buf: Vec<u8> = Vec::new();
    // The whole header section must arrive within HEADER_TIMEOUT_MS.
    let head_deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(HEADER_TIMEOUT_MS);
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        // A complete header section with bare LF line endings is malformed.
        if buf.windows(2).any(|w| w == b"\n\n") {
            bad_request(&mut wr).await;
            return;
        }
        // header_bytes counts the field section only; the request line is
        // bounded by memory. An unreadable head is closed without a response.
        match buf.windows(2).position(|w| w == b"\r\n") {
            Some(le) if (buf.len() - (le + 2)) as u64 > header_limit.saturating_add(4) => return,
            _ => {}
        }
        let mut chunk = vec![0u8; 8192];
        let read = tokio::select! {
            _ = live.cancelled() => return,
            r = tokio::time::timeout_at(head_deadline, rd.read(&mut chunk)) => r,
        };
        match read {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return,
            Ok(Ok(k)) => {
                let growth = if buf.len() + k > buf.capacity() { (buf.len() + k).max(buf.capacity().saturating_mul(2)).saturating_sub(buf.capacity()) } else { 0 };
                if memory.is_some_and(|m| !m.allows(growth)) { return; }
                buf.extend_from_slice(&chunk[..k]);
            },
        }
    };
    let header_bytes = http1::field_section_len(&buf, head_end) as u64;
    if header_bytes > header_limit {
        return;
    }
    // Each field line takes at least 4 octets, so header_bytes bounds the count.
    let count = header_bytes as usize / 4 + 1;
    if memory.is_some_and(|m| !m.allows(count * std::mem::size_of::<httparse::Header>())) { return; }
    let mut hdrs = vec![httparse::EMPTY_HEADER; count];
    let mut req = httparse::Request::new(&mut hdrs);
    // A complete but malformed header section (invalid field names or
    // octets, bad target, folding, non-HTTP/1.1) is an invalid request.
    match req.parse(&buf[..head_end]) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => {
            bad_request(&mut wr).await;
            return;
        }
    }
    if req.version != Some(1) {
        bad_request(&mut wr).await;
        return;
    }
    let method = req.method.unwrap_or("").to_string();
    let target = req.path.unwrap_or("").to_string();
    let headers: Vec<(String, Vec<u8>)> = req.headers.iter().map(|h| (h.name.to_string(), h.value.to_vec())).collect();
    // RFC 9110 §7.2: exactly one Host field line in an HTTP/1.1 request.
    if headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case("host")).count() != 1 {
        bad_request(&mut wr).await;
        return;
    }
    let upgrade = headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("upgrade"));
    let framing = framing_of(&headers);
    if live.is_cancelled() {
        return; // the listener stopped accepting while the head arrived
    }
    let rest = buf[head_end..].to_vec();
    let id = {
        let mut i = inner.borrow_mut();
        i.next_req += 1;
        let id = i.next_req;
        i.conns.insert(id, ConnState {
            read: Some(rd), write: Some(wr), buf: rest, framing, watcher: None,
            // The body reader receives only the budget left for trailers.
            header_limit: header_limit - header_bytes,
        });
        id
    };
    let _ = tx.send(HostEvent::Request(IncomingRequest {
        listener,
        id,
        label: id.to_string(),
        method,
        target,
        headers,
        upgrade,
        framing_invalid: matches!(framing, Framing::Invalid),
    }));
}

/// Pre-admission 400 for an unparseable request head; closes the connection.
async fn bad_request(wr: &mut OwnedWriteHalf) {
    let body = b"{\"error\":\"bad_request\"}";
    let head = format!(
        "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = wr.write_all(head.as_bytes()).await;
    let _ = wr.write_all(body).await;
    let _ = wr.shutdown().await;
}
