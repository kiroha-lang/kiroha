//! Shared helpers for the integration scenarios (spec §15.3 I01–I08).
#![allow(dead_code)]

use kiroha::value::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

pub fn kiroha() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiroha"))
}

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn tempdir(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("kiroha-it-{}-{}-{}", name, std::process::id(), n));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub struct Run {
    pub json: Value,
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

pub fn finish(out: Output) -> Run {
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let json = kiroha::json::parse(stdout.trim_end()).unwrap_or(Value::Null);
    Run { json, stdout, stderr, code: out.status.code().unwrap_or(-1) }
}

/// Run a Kiroha program with a host configuration JSON text.
pub fn run(src: &str, host: Option<&str>) -> Run {
    run_env(src, host, &[])
}

pub fn run_env(src: &str, host: Option<&str>, env: &[(&str, &str)]) -> Run {
    let dir = tempdir("run");
    let sp = dir.join("main.kiroha");
    std::fs::write(&sp, src).unwrap();
    let mut c = kiroha();
    c.arg("run").arg(&sp).arg("--json");
    if let Some(h) = host {
        let hp = dir.join("host.json");
        std::fs::write(&hp, h).unwrap();
        c.arg("--host").arg(hp);
    }
    for (k, v) in env {
        c.env(k, v);
    }
    finish(c.stdin(Stdio::null()).output().unwrap())
}

/// Decode a tagged wire value into a compact JSON-ish string for assertions.
pub fn plain(w: &Value) -> String {
    fn go(w: &Value) -> String {
        let r = w.as_record().unwrap();
        let t = r.get("t").unwrap().as_str().unwrap();
        match t {
            "null" => "null".into(),
            "bool" => kiroha::json::dump(r.get("v").unwrap()),
            "number" => r.get("v").unwrap().as_str().unwrap().to_string(),
            "string" => kiroha::json::dump(r.get("v").unwrap()),
            "bytes" => format!("b64:{}", r.get("base64").unwrap().as_str().unwrap()),
            "array" => format!("[{}]", r.get("v").unwrap().as_array().unwrap().iter().map(go).collect::<Vec<_>>().join(",")),
            "record" => format!(
                "{{{}}}",
                r.get("v")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        let p = p.as_array().unwrap();
                        format!("{}:{}", kiroha::json::dump(&p[0]), go(&p[1]))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            "opaque" => format!("<{}>", r.get("kind").unwrap().as_str().unwrap()),
            _ => panic!("bad tag"),
        }
    }
    go(w)
}

pub fn result(r: &Run) -> String {
    assert_eq!(r.code, 0, "run failed: {} {}", r.stdout, r.stderr);
    plain(r.json.as_record().unwrap().get("result").unwrap())
}

pub fn error_code(r: &Run) -> String {
    let e = r.json.as_record().unwrap().get("error").unwrap();
    e.as_record().unwrap().get("code").unwrap().as_str().unwrap().to_string()
}

/// A raw HTTP fixture server: `respond(path, head) -> raw response bytes`.
/// Returns the port; also records request heads.
pub fn fixture_server<F>(respond: F) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>)
where
    F: Fn(&str, &str) -> Vec<u8> + Send + 'static,
{
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log2 = log.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = match s {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                }
            }
            let head = String::from_utf8_lossy(&buf).to_string();
            let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
            log2.lock().unwrap().push(head.clone());
            let resp = respond(&path, &head);
            let _ = s.write_all(&resp);
            let _ = s.flush();
        }
    });
    (port, log)
}

pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}
