//! I05: server with raw HTTP requests — route precedence, encoded slash,
//! plus/query rules, explicit HEAD/OPTIONS, content types, 413/503/504,
//! disconnect and listener shutdown.

mod common;
use common::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn send(port: u16, raw: &[u8]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).to_string()
}

fn status(resp: &str) -> &str {
    resp.split_whitespace().nth(1).unwrap_or("")
}

fn body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

#[test]
fn routing_framing_limits_and_shutdown() {
    let port = free_port();
    let dir = tempdir("srv");
    let src = format!(
        r#"serve({{"GET /a/fixed":{{r=>"literal"}},"GET /a/:x":{{r=>{{p:r.params.x,q:r.query,path:r.path}}}},
        "HEAD /h":{{r=>{{a:1}}}},"OPTIONS /o":{{r=>reply(204,null)}},"POST /in":{{r=>input(r,{{n:int}})}},
        "GET /slow":{{r=>sleep(2000);1}},"GET /block":{{r=>sleep(800);1}},"GET /bytes":{{r=>encode("x")}},
        "GET /cpu":{{r=>xs=range(0,1000000);len(flat([xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs]))}}}},
        {{port:{port},timeout:300,max_body:8,max_inflight:1}})"#
    );
    let sp = dir.join("s.kiri");
    std::fs::write(&sp, src).unwrap();
    let hp = dir.join("h.json");
    std::fs::write(
        &hp,
        format!(r#"{{"format":"kiri-host/1","capabilities":{{"listen":[{{"host":"127.0.0.1","port":{port}}}]}},"limits":{{"memory_bytes":4000000000}}}}"#),
    )
    .unwrap();
    let child = kiri().arg("run").arg(&sp).arg("--json").arg("--host").arg(&hp).stdout(std::process::Stdio::piped()).spawn().unwrap();
    // Kill the server if an assertion fails before the orderly shutdown.
    struct Guard(Option<std::process::Child>);
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(c) = self.0.as_mut() {
                let _ = c.kill();
            }
        }
    }
    let mut guard = Guard(Some(child));
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let r = send(port, b"GET /a/fixed HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!((status(&r), body(&r)), ("200", "\"literal\""));
    let r = send(port, b"GET /a/x%2Fy?t=a+b&t=c%2Bd HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(body(&r), r#"{"p":"x/y","q":{"t":["a b","c+d"]},"path":"/a/x%2Fy"}"#);
    let r = send(port, b"HEAD /h HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!((status(&r), body(&r)), ("200", ""));
    let r = send(port, b"OPTIONS /o HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status(&r), "204");
    let r = send(port, b"HEAD /a/fixed HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status(&r), "405");
    assert!(r.to_ascii_lowercase().contains("allow: get"));
    let r = send(port, b"POST /in HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"n\":1}");
    assert_eq!(body(&r), r#"{"n":1}"#);
    let r = send(port, b"POST /in HTTP/1.1\r\nHost: x\r\nContent-Type: text/plain\r\nContent-Length: 7\r\n\r\n{\"n\":1}");
    assert_eq!(status(&r), "415");
    let r = send(port, b"POST /in HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 20\r\n\r\n{\"n\":1}");
    assert_eq!(status(&r), "413");
    let r = send(port, b"POST /in HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"n\":2}\r\n0\r\n\r\n");
    assert_eq!(body(&r), r#"{"n":2}"#);
    let r = send(port, b"GET /slow HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!((status(&r), body(&r)), ("504", r#"{"error":"timeout"}"#));
    // A deadline that passes inside builtin work still yields 504.
    let t = std::time::Instant::now();
    let r = send(port, b"GET /cpu HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status(&r), "504");
    assert!(t.elapsed().as_millis() < 3000);
    // Malformed header sections get 400 rather than a silent close.
    for raw in [
        &b"GET /a/fixed HTTP/1.1\r\n\r\n"[..],
        b"GET /a/fixed HTTP/1.1\r\nHost: x\r\nHost: y\r\n\r\n",
        &b"GET /a/fixed HTTP/1.1\r\nBad Name: v\r\n\r\n"[..],
        b"GET /a/fixed HTTP/1.1\r\nX-A: a\x01b\r\n\r\n",
        b"GET /o\x01k HTTP/1.1\r\nHost: x\r\n\r\n",
        b"GET /a/fixed HTTP/1.0\r\nHost: x\r\n\r\n",
        b"GET /a/fixed HTTP/1.1\nHost: x\n\n",
    ] {
        let r = send(port, raw);
        assert_eq!((status(&r), body(&r)), ("400", r#"{"error":"bad_request"}"#), "{:?}", String::from_utf8_lossy(raw));
    }
    let r = send(port, b"GET /bytes HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status(&r), "500");
    let r = send(port, b"GET http://x/a/fixed HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status(&r), "400");
    // 503 while the single inflight slot is busy (server timeout is 300 ms,
    // so use a disconnecting client to hold the slot briefly)
    let mut busy = TcpStream::connect(("127.0.0.1", port)).unwrap();
    busy.write_all(b"GET /block HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let r = send(port, b"GET /a/fixed HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status(&r), "503");
    drop(busy); // disconnect aborts the pending handler
    std::thread::sleep(Duration::from_millis(100));
    let r = send(port, b"GET /a/fixed HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status(&r), "200");
    // shutdown via SIGINT -> cancelled abort, listener closed
    unsafe {
        libc::kill(guard.0.as_ref().unwrap().id() as i32, libc::SIGINT);
    }
    let out = guard.0.take().unwrap().wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"reason\":\"cancelled\""));
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}
