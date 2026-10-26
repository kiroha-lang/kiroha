//! I02: TLS fixture — valid chain/hostname succeeds; wrong name and untrusted
//! chain fail without disabling verification; a dropped response after the
//! request was sent stays `unknown`.

mod common;
use common::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Pki {
    ca_pem: String,
    cert: rustls::pki_types::CertificateDer<'static>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
}

fn pki(names: &[&str]) -> Pki {
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let leaf_key = rcgen::KeyPair::generate().unwrap();
    let leaf = rcgen::CertificateParams::new(names.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .unwrap()
        .signed_by(&leaf_key, &ca, &ca_key)
        .unwrap();
    Pki {
        ca_pem: ca.pem(),
        cert: leaf.der().clone(),
        key: rustls::pki_types::PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into()),
    }
}

/// TLS server; `drop_after_request` closes without responding.
fn tls_server(p: &Pki, drop_after_request: bool) -> u16 {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![p.cert.clone()], p.key.clone_key())
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
    let std_l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = std_l.local_addr().unwrap().port();
    std_l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let l = tokio::net::TcpListener::from_std(std_l).unwrap();
            loop {
                let (s, _) = l.accept().await.unwrap();
                let acc = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(mut tls) = acc.accept(s).await {
                        let mut buf = vec![0u8; 4096];
                        let _ = tls.read(&mut buf).await;
                        if !drop_after_request {
                            let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\n{\"t\":1}").await;
                            let _ = tls.shutdown().await;
                        }
                    }
                });
            }
        });
    });
    port
}

fn host(port: u16) -> String {
    format!(r#"{{"format":"kiroha-host/1","capabilities":{{"http":["https://localhost:{port}","https://127.0.0.1:{port}"]}}}}"#)
}

#[test]
fn verified_chain_hostname_and_failures() {
    let p = pki(&["localhost"]);
    let dir = tempdir("tls");
    let ca = dir.join("ca.pem");
    std::fs::write(&ca, &p.ca_pem).unwrap();
    let port = tls_server(&p, false);
    let ca_s = ca.to_str().unwrap();
    // valid chain + hostname
    let r = run_env(&format!(r#"get("https://localhost:{port}/")"#), Some(&host(port)), &[("SSL_CERT_FILE", ca_s)]);
    assert_eq!(result(&r), r#"{"t":1}"#);
    // wrong name (certificate is for localhost only)
    let r = run_env(
        &format!(r#"try get("https://127.0.0.1:{port}/") catch {{e=>[e.code,e.data.outcome]}}"#),
        Some(&host(port)),
        &[("SSL_CERT_FILE", ca_s)],
    );
    assert_eq!(result(&r), r#"["tls","not_sent"]"#);
    // untrusted chain (another CA in the trust store)
    let other = pki(&["localhost"]);
    let oca = dir.join("other.pem");
    std::fs::write(&oca, &other.ca_pem).unwrap();
    let r = run_env(
        &format!(r#"try get("https://localhost:{port}/") catch {{e=>[e.code,e.data.outcome]}}"#),
        Some(&host(port)),
        &[("SSL_CERT_FILE", oca.to_str().unwrap())],
    );
    assert_eq!(result(&r), r#"["tls","not_sent"]"#);
}

#[test]
fn dropped_acknowledgment_is_unknown() {
    let p = pki(&["localhost"]);
    let dir = tempdir("tls2");
    let ca = dir.join("ca.pem");
    std::fs::write(&ca, &p.ca_pem).unwrap();
    let port = tls_server(&p, true);
    let r = run_env(
        &format!(r#"try post("https://localhost:{port}/",{{a:1}}) catch {{e=>[e.code,e.data.outcome]}}"#),
        Some(&host(port)),
        &[("SSL_CERT_FILE", ca.to_str().unwrap())],
    );
    assert_eq!(result(&r), r#"["network","unknown"]"#);
}
