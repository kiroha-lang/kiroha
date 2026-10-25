//! HTTP/1.1 server adapter (spec §12). Raw request targets and header pairs
//! reach the runtime unnormalized; framing is decoded here, while admission,
//! routing and responses are runtime decisions. One request per connection.

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
    Length(u64),
    Chunked,
    Invalid,
}

struct ConnState {
    read: Option<OwnedReadHalf>,
    write: Option<OwnedWriteHalf>,
    buf: Vec<u8>,
    framing: Framing,
    watcher: Option<CancellationToken>,
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
}

fn io_err(reason: &str, outcome: &str) -> EffOutcome {
    EffOutcome::Err(err::make(
        "io",
        format!("serve: {reason}"),
        vec![("operation", Value::str("serve")), ("reason", Value::str(reason)), ("outcome", Value::str(outcome))],
    ))
}

fn framing_of(headers: &[(String, Vec<u8>)]) -> Framing {
    let te: Vec<&Vec<u8>> = headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case("transfer-encoding")).map(|(_, v)| v).collect();
    let cl: Vec<&Vec<u8>> = headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case("content-length")).map(|(_, v)| v).collect();
    if !te.is_empty() {
        if !cl.is_empty() || te.len() != 1 || !te[0].trim_ascii().eq_ignore_ascii_case(b"chunked") {
            return Framing::Invalid;
        }
        return Framing::Chunked;
    }
    let mut len: Option<u64> = None;
    for v in cl {
        let s = match std::str::from_utf8(v.trim_ascii()) {
            Ok(s) if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) => s,
            _ => return Framing::Invalid,
        };
        let n: u64 = match s.parse() {
            Ok(n) => n,
            Err(_) => return Framing::Invalid,
        };
        if len.is_some_and(|l| l != n) {
            return Framing::Invalid;
        }
        len = Some(n);
    }
    match len {
        Some(n) => Framing::Length(n),
        None => Framing::Empty,
    }
}

impl Servers {
    pub fn new(tx: mpsc::UnboundedSender<HostEvent>) -> Servers {
        Servers { tx, inner: Rc::new(RefCell::new(Inner::default())) }
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
                        handle_conn(stream, inner, tx, lid, header_limit, live).await;
                    });
                }
            }
        });
        EffOutcome::Ok(EffValue::Str(id))
    }

    pub fn read_body(&mut self, request: u64, max_body: u64) -> impl Future<Output = EffOutcome> {
        let inner = self.inner.clone();
        let tx = self.tx.clone();
        async move {
            let (mut rd, mut buf, framing) = {
                let mut i = inner.borrow_mut();
                match i.conns.get_mut(&request) {
                    Some(c) => match c.read.take() {
                        Some(r) => (r, std::mem::take(&mut c.buf), c.framing),
                        None => return EffOutcome::Ok(EffValue::Body(Err(BodyFault::BadFraming))),
                    },
                    None => return EffOutcome::Ok(EffValue::Body(Err(BodyFault::BadFraming))),
                }
            };
            let result = match framing {
                Framing::Empty => Ok(Vec::new()),
                Framing::Invalid => Err(BodyFault::BadFraming),
                Framing::Length(n) => {
                    if n > max_body {
                        Err(BodyFault::TooLarge)
                    } else {
                        let mut ok = true;
                        while (buf.len() as u64) < n {
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
                Framing::Chunked => read_chunked(&mut rd, buf, max_body).await,
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

async fn read_chunked(rd: &mut OwnedReadHalf, mut buf: Vec<u8>, max_body: u64) -> Result<Vec<u8>, BodyFault> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    async fn need(rd: &mut OwnedReadHalf, buf: &mut Vec<u8>, upto: usize) -> Result<(), BodyFault> {
        while buf.len() < upto {
            let mut chunk = vec![0u8; 16384];
            match rd.read(&mut chunk).await {
                Ok(0) | Err(_) => return Err(BodyFault::BadFraming),
                Ok(k) => buf.extend_from_slice(&chunk[..k]),
            }
        }
        Ok(())
    }
    async fn line(rd: &mut OwnedReadHalf, buf: &mut Vec<u8>, pos: &mut usize) -> Result<Vec<u8>, BodyFault> {
        loop {
            if let Some(i) = buf[*pos..].windows(2).position(|w| w == b"\r\n") {
                let l = buf[*pos..*pos + i].to_vec();
                *pos += i + 2;
                return Ok(l);
            }
            if buf.len() - *pos > 8192 {
                return Err(BodyFault::BadFraming);
            }
            let mut chunk = vec![0u8; 16384];
            match rd.read(&mut chunk).await {
                Ok(0) | Err(_) => return Err(BodyFault::BadFraming),
                Ok(k) => buf.extend_from_slice(&chunk[..k]),
            }
        }
    }
    loop {
        let l = line(rd, &mut buf, &mut pos).await?;
        let size_text = std::str::from_utf8(&l).map_err(|_| BodyFault::BadFraming)?;
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = u64::from_str_radix(size_text, 16).map_err(|_| BodyFault::BadFraming)?;
        if size == 0 {
            // trailers until empty line
            loop {
                let t = line(rd, &mut buf, &mut pos).await?;
                if t.is_empty() {
                    return Ok(out);
                }
            }
        }
        if out.len() as u64 + size > max_body {
            return Err(BodyFault::TooLarge);
        }
        need(rd, &mut buf, pos + size as usize + 2).await?;
        out.extend_from_slice(&buf[pos..pos + size as usize]);
        if &buf[pos + size as usize..pos + size as usize + 2] != b"\r\n" {
            return Err(BodyFault::BadFraming);
        }
        pos += size as usize + 2;
    }
}

async fn handle_conn(
    stream: TcpStream,
    inner: Rc<RefCell<Inner>>,
    tx: mpsc::UnboundedSender<HostEvent>,
    listener: String,
    header_limit: u64,
    live: CancellationToken,
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
        if buf.len() as u64 > header_limit {
            return; // unreadable header section: close without a response
        }
        let mut chunk = vec![0u8; 8192];
        let read = tokio::select! {
            _ = live.cancelled() => return,
            r = tokio::time::timeout_at(head_deadline, rd.read(&mut chunk)) => r,
        };
        match read {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return,
            Ok(Ok(k)) => buf.extend_from_slice(&chunk[..k]),
        }
    };
    if head_end as u64 > header_limit + 4 {
        return;
    }
    let mut hdrs = vec![httparse::EMPTY_HEADER; 256];
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
        i.conns.insert(id, ConnState { read: Some(rd), write: Some(wr), buf: rest, framing, watcher: None });
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
