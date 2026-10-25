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

fn root_store() -> rustls::RootCertStore {
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

    pub async fn request(&self, req: HttpRequest) -> EffOutcome {
        let u = match Url::parse(&req.url) {
            Ok(u) => u,
            Err(_) => return net("invalid_url", "not_sent"),
        };
        let stream = match self.connect(&u).await {
            Ok(s) => s,
            Err(e) => return e,
        };
        match stream {
            Stream::Plain(s) => self.exchange(TokioIo::new(s), &u, req).await,
            Stream::Tls(s) => self.exchange(TokioIo::new(*s), &u, req).await,
        }
    }

    async fn exchange<T>(&self, io: TokioIo<T>, u: &Url, req: HttpRequest) -> EffOutcome
    where
        T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    {
        let mut builder = hyper::client::conn::http1::Builder::new();
        builder.max_buf_size((req.header_limit as usize).max(8192) + 1024);
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
        let mut gzip = false;
        for (n, v) in resp.headers() {
            header_bytes += (n.as_str().len() + v.as_bytes().len() + 4) as u64;
            if n.as_str() == "content-encoding" && !v.as_bytes().eq_ignore_ascii_case(b"identity") {
                gzip = true;
            }
            headers.push((n.as_str().to_string(), v.as_bytes().to_vec()));
        }
        if header_bytes > req.header_limit {
            return EffOutcome::Resource("header_bytes".into());
        }
        let raw_cap: u64 = if gzip { req.max_bytes.saturating_mul(16).saturating_add(1 << 20) } else { req.max_bytes.saturating_add(1) };
        let mut body = Vec::new();
        let mut incoming = resp.into_body();
        if !head_method {
            while let Some(frame) = incoming.frame().await {
                match frame {
                    Ok(f) => {
                        if let Some(data) = f.data_ref() {
                            body.extend_from_slice(data);
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
