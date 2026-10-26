//! HTTP/1.1 client adapter (spec §10). One connection per request: no hidden
//! cookies, auth or session reuse; no redirects or retries. The message is
//! framed by this module (not a general HTTP library) so that raw field-section
//! octets are counted exactly and chunked framing, trailers and Content-Length
//! are validated strictly (RFC 9112). TLS verifies the
//! certificate chain and hostname against the host trust store
//! (`SSL_CERT_FILE` PEM bundle when set, otherwise the system store).

use super::http1::{self, ChunkErr};
use crate::host::{EffOutcome, EffValue, HttpRequest, RawResponse};
use crate::url::Url;
use crate::value::{err, Value};
use std::cell::Cell;
use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Client {
    tls: Arc<rustls::ClientConfig>,
}

fn net(reason: &str, outcome: &str) -> EffOutcome {
    EffOutcome::Err(crate::builtins::effects::network_error(reason, outcome))
}

fn tls_err(reason: &str) -> EffOutcome {
    EffOutcome::Err(err::make(
        "tls",
        format!("http: TLS failure: {reason}"),
        vec![("operation", Value::str("http")), ("reason", Value::str(reason)), ("outcome", Value::str("not_sent"))],
    ))
}

pub fn root_store() -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    if let Ok(path) = std::env::var("SSL_CERT_FILE") {
        if let Ok(pem) = std::fs::read(&path) {
            for c in rustls_pemfile::certs(&mut &pem[..]).flatten() {
                let _ = roots.add(c);
            }
        }
        return roots;
    }
    let native = rustls_native_certs::load_native_certs();
    for c in native.certs {
        let _ = roots.add(c);
    }
    if roots.is_empty() {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    roots
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

enum Stream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl Client {
    pub fn new() -> Client {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cfg = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("TLS protocol versions")
            .with_root_certificates(root_store())
            .with_no_client_auth();
        Client { tls: Arc::new(cfg) }
    }

    async fn connect(&self, u: &Url) -> Result<Stream, EffOutcome> {
        let host = u.host_bare().to_string();
        let port = u.port_number();
        let tcp = TcpStream::connect((host.as_str(), port)).await.map_err(|e| {
            let reason = if e.kind() == std::io::ErrorKind::ConnectionRefused { "connection_refused" } else { "connect" };
            net(reason, "not_sent")
        })?;
        let _ = tcp.set_nodelay(true);
        if u.scheme == "http" {
            return Ok(Stream::Plain(tcp));
        }
        let name = rustls::pki_types::ServerName::try_from(host.clone()).map_err(|_| tls_err("server_name"))?;
        let connector = tokio_rustls::TlsConnector::from(self.tls.clone());
        match connector.connect(name, tcp).await {
            Ok(s) => Ok(Stream::Tls(Box::new(s))),
            Err(e) => {
                let msg = e.to_string().to_ascii_lowercase();
                let reason = if msg.contains("certificate") || msg.contains("unknownissuer") || msg.contains("notvalidforname") {
                    "certificate"
                } else {
                    "handshake"
                };
                Err(tls_err(reason))
            }
        }
    }

    /// `sent` is set just before request bytes are written, so a cancellation
    /// can report `not_sent` precisely.
    pub async fn request(&self, req: HttpRequest, sent: Rc<Cell<bool>>) -> EffOutcome {
        let u = match Url::parse(&req.url) {
            Ok(u) => u,
            Err(_) => return net("invalid_url", "not_sent"),
        };
        let stream = match self.connect(&u).await {
            Ok(s) => s,
            Err(e) => return e,
        };
        match stream {
            Stream::Plain(s) => exchange(s, &u, req, sent).await,
            Stream::Tls(s) => exchange(*s, &u, req, sent).await,
        }
    }
}

/// Longest accepted status line.
const STATUS_LINE_MAX: usize = 8192;
/// Interim (1xx) responses tolerated before the final one.
const MAX_INTERIM: usize = 16;

enum Mode {
    None,
    Length(u64),
    Chunked,
    /// Delimited by the end of the connection.
    Close,
}

async fn read_more<T: AsyncRead + Unpin>(io: &mut T, buf: &mut Vec<u8>) -> bool {
    let mut chunk = [0u8; 16384];
    match io.read(&mut chunk).await {
        Ok(0) | Err(_) => false,
        Ok(k) => {
            buf.extend_from_slice(&chunk[..k]);
            true
        }
    }
}

/// Collects the (content-encoded) body while enforcing the decoded-size limit
/// incrementally (§10.4).
struct Sink {
    body: Vec<u8>,
    decoder: Option<flate2::write::MultiGzDecoder<Counter>>,
    max_bytes: u64,
    /// Guard on raw gzip input (streams of empty members).
    raw_cap: u64,
}

impl Sink {
    fn push(&mut self, data: &[u8]) -> Result<(), EffOutcome> {
        self.body.extend_from_slice(data);
        match self.decoder.as_mut() {
            Some(d) => {
                if d.write_all(data).is_err() {
                    return Err(net("decode", "unknown"));
                }
                if d.get_ref().0 > self.max_bytes {
                    return Err(EffOutcome::Err(crate::builtins::effects::limit_error(self.max_bytes)));
                }
                if self.body.len() as u64 > self.raw_cap {
                    return Err(EffOutcome::Resource("transport".into()));
                }
            }
            None => {
                if self.body.len() as u64 > self.max_bytes {
                    return Err(EffOutcome::Err(crate::builtins::effects::limit_error(self.max_bytes)));
                }
            }
        }
        Ok(())
    }
}

async fn exchange<T>(mut io: T, u: &Url, req: HttpRequest, sent: Rc<Cell<bool>>) -> EffOutcome
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // ---- request -----------------------------------------------------------
    let mut out = Vec::with_capacity(512 + req.body.as_ref().map_or(0, |b| b.len()));
    out.extend_from_slice(format!("{} {} HTTP/1.1\r\nhost: {}\r\n", req.method, u.target(), u.authority()).as_bytes());
    for (n, v) in &req.headers {
        out.extend_from_slice(format!("{n}: {v}\r\n").as_bytes());
    }
    match &req.body {
        Some(b) => out.extend_from_slice(format!("content-length: {}\r\n", b.len()).as_bytes()),
        None if matches!(req.method.as_str(), "POST" | "PUT" | "PATCH") => out.extend_from_slice(b"content-length: 0\r\n"),
        None => {}
    }
    out.extend_from_slice(b"connection: close\r\n\r\n");
    if let Some(b) = &req.body {
        out.extend_from_slice(b);
    }
    // From here application bytes may have been sent.
    sent.set(true);
    if io.write_all(&out).await.is_err() || io.flush().await.is_err() {
        return net("exchange", "unknown");
    }

    // ---- response head -----------------------------------------------------
    let limit = req.header_limit;
    let mut buf: Vec<u8> = Vec::new();
    let mut interim = 0;
    let mut header_bytes = 0u64;
    let (status, headers) = loop {
        let head_end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            match buf.windows(2).position(|w| w == b"\r\n") {
                None if buf.len() > STATUS_LINE_MAX => return net("framing", "unknown"),
                Some(le) if (buf.len() - (le + 2)) as u64 > limit.saturating_add(4) => return EffOutcome::Resource("header_bytes".into()),
                _ => {}
            }
            if !read_more(&mut io, &mut buf).await {
                return net("framing", "unknown");
            }
        };
        // header_bytes counts the received field-section octets exactly
        // (unlike a parsed, whitespace-trimmed view).
        header_bytes = header_bytes.saturating_add(http1::field_section_len(&buf, head_end) as u64);
        if header_bytes > limit {
            return EffOutcome::Resource("header_bytes".into());
        }
        let parsed = {
            let mut slots = vec![httparse::EMPTY_HEADER; (limit as usize / 4 + 1).min(1 << 16)];
            let mut resp = httparse::Response::new(&mut slots);
            match resp.parse(&buf[..head_end]) {
                Ok(httparse::Status::Complete(n)) if n == head_end => {}
                Err(httparse::Error::TooManyHeaders) => return EffOutcome::Resource("header_bytes".into()),
                _ => return net("framing", "unknown"),
            }
            let hs: Vec<(String, Vec<u8>)> = resp.headers.iter().map(|h| (h.name.to_string(), h.value.to_vec())).collect();
            (resp.code.unwrap_or(0), hs)
        };
        buf.drain(..head_end);
        let (code, hs) = parsed;
        if (100..200).contains(&code) {
            if code == 101 {
                return net("invalid_status", "unknown");
            }
            interim += 1;
            if interim > MAX_INTERIM {
                return net("framing", "unknown");
            }
            continue;
        }
        if !(200..=599).contains(&code) {
            return net("invalid_status", "unknown");
        }
        break (code, hs);
    };

    // ---- body framing (RFC 9112 §6) ----------------------------------------
    let field = |name: &'static str| headers.iter().filter(move |(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_slice());
    let tokens = |name: &'static str| -> Vec<String> {
        field(name)
            .flat_map(|v| String::from_utf8_lossy(v).split(',').map(|t| t.trim().to_ascii_lowercase()).collect::<Vec<_>>())
            .filter(|t| !t.is_empty())
            .collect()
    };
    let bodyless = req.method == "HEAD" || matches!(status, 204 | 304);
    let te = tokens("transfer-encoding");
    let mode = if bodyless {
        Mode::None
    } else if field("transfer-encoding").next().is_some() {
        // Only exactly `chunked` can be removed (no TE header is ever sent);
        // Transfer-Encoding together with Content-Length is a smuggling signal.
        if field("content-length").next().is_some() || te != ["chunked"] {
            return net("framing", "unknown");
        }
        Mode::Chunked
    } else {
        match http1::content_length(field("content-length")) {
            Err(()) => return net("framing", "unknown"),
            Ok(Some(n)) => Mode::Length(n),
            Ok(None) => Mode::Close,
        }
    };
    // Content coding: absent/identity or a single gzip (§10.4).
    let codings = tokens("content-encoding");
    let gzip = codings == ["gzip"];
    if !bodyless && !codings.is_empty() && !gzip && codings != ["identity"] {
        return net("unsupported_encoding", "unknown");
    }

    // ---- body --------------------------------------------------------------
    let raw_cap = if gzip { req.max_bytes.saturating_mul(16).saturating_add(16 << 20) } else { req.max_bytes };
    let mut sink = Sink {
        body: Vec::new(),
        decoder: if gzip { Some(flate2::write::MultiGzDecoder::new(Counter::default())) } else { None },
        max_bytes: req.max_bytes,
        raw_cap,
    };
    let too_large = |gzip: bool| {
        if gzip {
            EffOutcome::Resource("transport".into())
        } else {
            EffOutcome::Err(crate::builtins::effects::limit_error(req.max_bytes))
        }
    };
    match mode {
        Mode::None => {}
        Mode::Length(n) => {
            if n > raw_cap {
                return too_large(gzip);
            }
            let mut left = n;
            if !buf.is_empty() && left > 0 {
                let k = (buf.len() as u64).min(left) as usize;
                if let Err(e) = sink.push(&buf[..k]) {
                    return e;
                }
                left -= k as u64;
            }
            while left > 0 {
                let mut piece = Vec::new();
                if !read_more(&mut io, &mut piece).await {
                    return net("framing", "unknown");
                }
                let k = (piece.len() as u64).min(left) as usize;
                if let Err(e) = sink.push(&piece[..k]) {
                    return e;
                }
                left -= k as u64;
            }
        }
        Mode::Chunked => {
            // Head fields (including interim responses) and trailers share
            // one received-field budget (§14.1).
            match http1::read_chunked(&mut io, std::mem::take(&mut buf), raw_cap, limit - header_bytes, |d| sink.push(d)).await {
                Ok(()) => {}
                Err(ChunkErr::Framing) => return net("framing", "unknown"),
                Err(ChunkErr::TooLarge) => return too_large(gzip),
                // header_bytes overflow on a client is a host resource abort (§14.1)
                Err(ChunkErr::TrailerTooLarge) => return EffOutcome::Resource("header_bytes".into()),
                Err(ChunkErr::Sink(e)) => return e,
            }
        }
        Mode::Close => {
            if !buf.is_empty() {
                if let Err(e) = sink.push(&buf) {
                    return e;
                }
            }
            loop {
                let mut piece = Vec::new();
                let mut chunk = [0u8; 16384];
                match io.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(k) => piece.extend_from_slice(&chunk[..k]),
                    Err(_) => return net("framing", "unknown"),
                }
                if let Err(e) = sink.push(&piece) {
                    return e;
                }
            }
        }
    }
    EffOutcome::Ok(EffValue::Http(RawResponse { status, headers, body: sink.body }))
}

/// Counts decoded bytes without storing them.
#[derive(Default)]
struct Counter(u64);

impl std::io::Write for Counter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
