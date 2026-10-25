//! HTTP/1.1 client adapter (spec §10). One connection per request: no hidden
//! cookies, auth or session reuse; no redirects or retries. TLS verifies the
//! certificate chain and hostname against the host trust store
//! (`SSL_CERT_FILE` PEM bundle when set, otherwise the system store).

use crate::host::{EffOutcome, EffValue, HttpRequest, RawResponse};
use crate::url::Url;
use crate::value::{err, Value};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper_util::rt::TokioIo;
use std::sync::Arc;
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
    pub async fn request(&self, req: HttpRequest, sent: std::rc::Rc<std::cell::Cell<bool>>) -> EffOutcome {
        let u = match Url::parse(&req.url) {
            Ok(u) => u,
            Err(_) => return net("invalid_url", "not_sent"),
        };
        let stream = match self.connect(&u).await {
            Ok(s) => s,
            Err(e) => return e,
        };
        match stream {
            Stream::Plain(s) => self.exchange(TokioIo::new(s), &u, req, sent).await,
            Stream::Tls(s) => self.exchange(TokioIo::new(*s), &u, req, sent).await,
        }
    }

    async fn exchange<T>(&self, io: TokioIo<T>, u: &Url, req: HttpRequest, sent: std::rc::Rc<std::cell::Cell<bool>>) -> EffOutcome
    where
        T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    {
        let mut builder = hyper::client::conn::http1::Builder::new();
        builder.max_buf_size((req.header_limit as usize).max(8192) + 1024);
        // Field count is bounded by header_bytes (a field line is >= 4 octets),
        // not by hyper's default of 100.
        builder.max_headers((req.header_limit as usize / 4 + 1).max(100));
        let head_method = req.method == "HEAD";
        let mut b = hyper::Request::builder().method(req.method.as_str()).uri(u.target());
        b = b.header("host", u.authority());
        for (n, v) in &req.headers {
            b = b.header(n.as_str(), v.as_str());
        }
        let body: http_body_util::combinators::UnsyncBoxBody<Bytes, std::convert::Infallible> = match &req.body {
            Some(bytes) => {
                b = b.header("content-length", bytes.len().to_string());
                Full::new(Bytes::from(bytes.clone())).boxed_unsync()
            }
            None => Empty::<Bytes>::new().boxed_unsync(),
        };
        let request = match b.body(body) {
            Ok(r) => r,
            Err(_) => return net("invalid_request", "not_sent"),
        };
        let (mut sender, conn) = match builder.handshake(io).await {
            Ok(x) => x,
            Err(_) => return net("handshake", "not_sent"),
        };
        tokio::task::spawn_local(async move {
            let _ = conn.await;
        });
        // From here application bytes may have been sent.
        sent.set(true);
        let resp = match sender.send_request(request).await {
            Ok(r) => r,
            Err(e) => {
                if e.is_parse() && e.to_string().contains("too large") {
                    return EffOutcome::Resource("header_bytes".into());
                }
                return net("exchange", "unknown");
            }
        };
        let status = resp.status().as_u16();
        if status == 101 || !(200..=599).contains(&status) {
            return net("invalid_status", "unknown");
        }
        let mut headers = Vec::new();
        let mut header_bytes: u64 = 0;
        let mut codings: Vec<String> = Vec::new();
        for (n, v) in resp.headers() {
            header_bytes += (n.as_str().len() + v.as_bytes().len() + 4) as u64;
            if n.as_str() == "content-encoding" {
                for tok in String::from_utf8_lossy(v.as_bytes()).split(',') {
                    let tok = tok.trim().to_ascii_lowercase();
                    if !tok.is_empty() {
                        codings.push(tok);
                    }
                }
            }
            headers.push((n.as_str().to_string(), v.as_bytes().to_vec()));
        }
        let gzip = codings == ["gzip"];
        // hyper removes only a final `chunked` transfer coding. Any other
        // transfer coding (never advertised: no TE header is sent) cannot be
        // removed, so the response is a framing failure (§10.4).
        let te_other = resp.headers().get_all("transfer-encoding").iter().any(|v| {
            String::from_utf8_lossy(v.as_bytes()).split(',').map(|t| t.trim().to_ascii_lowercase()).any(|t| !t.is_empty() && t != "chunked")
        });
        if te_other {
            return net("framing", "unknown");
        }
        if header_bytes > req.header_limit {
            return EffOutcome::Resource("header_bytes".into());
        }
        // Identity bodies are bounded by max_bytes + 1 (the runtime then raises
        // `limit`). Gzip bodies are decoded incrementally here so max_bytes is
        // enforced on the decoded output as the stream arrives (§10.4); the raw
        // cap only guards streams of empty gzip members.
        let raw_cap: u64 =
            if gzip { req.max_bytes.saturating_mul(16).saturating_add(16 << 20) } else { req.max_bytes.saturating_add(1) };
        let mut decoder = if gzip { Some(flate2::write::MultiGzDecoder::new(Counter::default())) } else { None };
        let mut body = Vec::new();
        let mut incoming = resp.into_body();
        let bodyless = head_method || matches!(status, 204 | 304);
        if !bodyless {
            while let Some(frame) = incoming.frame().await {
                match frame {
                    Ok(f) => {
                        if let Some(data) = f.data_ref() {
                            body.extend_from_slice(data);
                            if let Some(d) = decoder.as_mut() {
                                use std::io::Write;
                                if d.write_all(data).is_err() {
                                    return net("decode", "unknown");
                                }
                                if d.get_ref().0 > req.max_bytes {
                                    return EffOutcome::Err(crate::builtins::effects::limit_error(req.max_bytes));
                                }
                            }
                            if body.len() as u64 > raw_cap {
                                if gzip {
                                    return EffOutcome::Resource("transport".into());
                                }
                                break;
                            }
                        }
                    }
                    Err(_) => return net("framing", "unknown"),
                }
            }
        }
        EffOutcome::Ok(EffValue::Http(RawResponse { status, headers, body }))
    }
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
