//! I01: local HTTP fixture — methods/status, repeated headers, bodyless
//! statuses, HEAD, gzip and decoded limits, invalid framing/UTF-8/JSON, and
//! no redirects/retries/cookies.

mod common;
use common::*;

fn raw(status: &str, headers: &[&str], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n").into_bytes();
    for h in headers {
        out.extend_from_slice(h.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

#[test]
fn maximum_header_quota_accepts_fragmented_response() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = socket.write_all(b"\r\n");
    });
    let h = format!(r#"{{"format":"kiroha-host/1","capabilities":{{"http":["http://127.0.0.1:{port}"]}},"limits":{{"header_bytes":18446744073709551615}}}}"#);
    let r = run(&format!(r#"http("GET","http://127.0.0.1:{port}/").status"#), Some(&h));
    server.join().unwrap();
    assert_eq!(result(&r), "200");
}

fn server() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    fixture_server(|path, _head| {
        let gz = gzip(br#"{"x":1}"#);
        match path {
            "/json" => raw("200 OK", &["Content-Length: 12", "X-A: 1", "x-a: 2", "Set-Cookie: a=1", "Set-Cookie: b=2"], b"{\"ok\":[1.0]}"),
            "/teapot" => raw("418 I'm a teapot", &["Content-Length: 1"], b"x"),
            "/204" => raw("204 No Content", &[], b""),
            "/205" => raw("205 Reset Content", &["Content-Length: 0"], b""),
            "/205bad" => raw("205 Reset Content", &["Content-Length: 3"], b"abc"),
            "/304" => raw("304 Not Modified", &[], b""),
            "/head" => raw("200 OK", &["Content-Length: 100", "Content-Encoding: br"], b""),
            "/gzip" => {
                let mut h = [format!("Content-Length: {}", gz.len()), "Content-Encoding: gzip".into()];
                let hs: Vec<&str> = h.iter_mut().map(|s| s.as_str()).collect();
                raw("200 OK", &hs, &gz)
            }
            "/chunked" => raw("200 OK", &["Transfer-Encoding: chunked"], b"3\r\n{\"a\r\n4\r\n\":1}\r\n0\r\n\r\n"),
            "/badchunk" => raw("200 OK", &["Transfer-Encoding: chunked"], b"zz\r\nabc\r\n"),
            "/badutf8" => raw("200 OK", &["Content-Length: 2"], &[0xc3, 0x28]),
            "/badjson" => raw("200 OK", &["Content-Length: 3"], b"{x}"),
            "/redirect" => raw("302 Found", &["Location: /json", "Content-Length: 0"], b""),
            _ => raw("404 Not Found", &["Content-Length: 0"], b""),
        }
    })
}

fn host(port: u16) -> String {
    format!(r#"{{"format":"kiroha-host/1","capabilities":{{"http":["http://127.0.0.1:{port}"]}}}}"#)
}

#[test]
fn statuses_headers_and_bodyless() {
    let (port, _) = server();
    let b = format!("http://127.0.0.1:{port}");
    let src = format!(
        r#"r=http("GET","{b}/json"); t=http("DELETE","{b}/teapot");
        [r.status,r.headers["x-a"],r.headers["set-cookie"],get("{b}/json"),t.status,len(t.body),
         http("GET","{b}/204").status, len(http("GET","{b}/205").body), http("GET","{b}/304").status,
         len(http("HEAD","{b}/head",{{max_bytes:0}}).body),
         (try http("GET","{b}/205bad") catch {{e=>[e.code,e.data.reason]}}),
         http("GET","{b}/redirect").status, (try get("{b}/redirect") catch {{e=>[e.code,e.data.status]}})]"#
    );
    let r = run(&src, Some(&host(port)));
    assert_eq!(
        result(&r),
        r#"[200,["1","2"],["a=1","b=2"],{"ok":[1]},418,1,204,0,304,0,["network","protocol"],302,["http",302]]"#
    );
}

#[test]
fn gzip_limits_framing_and_decoding_errors() {
    let (port, log) = server();
    let b = format!("http://127.0.0.1:{port}");
    let src = format!(
        r#"[get("{b}/gzip",{{max_bytes:7}}), (try get("{b}/gzip",{{max_bytes:6}}) catch {{e=>e.code}}),
         get("{b}/chunked"), (try get("{b}/badchunk") catch {{e=>e.code}}),
         (try get("{b}/badutf8") catch {{e=>e.code}}), (try get("{b}/badjson") catch {{e=>e.code}}),
         (try get("{b}/json",{{max_bytes:11}}) catch {{e=>[e.code,e.data.maximum]}})]"#
    );
    let r = run(&src, Some(&host(port)));
    assert_eq!(result(&r), r#"[{"x":1},"limit",{"a":1},"network","utf8","json",["limit",11]]"#);
    let heads = log.lock().unwrap();
    assert!(heads.iter().all(|h| h.to_ascii_lowercase().contains("accept-encoding: gzip")));
    // one request per call: no retries, no redirect following
    assert_eq!(heads.len(), 7);
    assert!(!heads.iter().any(|h| h.to_ascii_lowercase().contains("cookie")));
}

#[test]
fn request_shape_body_and_query() {
    let (port, log) = server();
    let b = format!("http://127.0.0.1:{port}");
    let src = format!(
        r#"[http("POST","{b}/x",{{body:encode("hi"),headers:{{"X-T":" v "}},query:{{q:["a b",null,1e21]}}}}).status,
          (try post("{b}/x",{{a:1}}) catch {{e=>[e.code,e.data.status]}})]"#
    );
    let r = run(&src, Some(&host(port)));
    assert_eq!(result(&r), r#"[404,["http",404]]"#);
    let heads = log.lock().unwrap();
    assert!(heads[0].starts_with("POST /x?q=a%20b&q=1e21 HTTP/1.1"), "{}", heads[0]);
    assert!(heads[0].to_ascii_lowercase().contains("x-t: v\r\n"));
    assert!(heads[0].to_ascii_lowercase().contains("content-length: 2"));
    assert!(heads[1].to_ascii_lowercase().contains("content-type: application/json"));
}

#[test]
fn denied_origin_and_connection_refused() {
    let r = run(r#"try get("http://127.0.0.1:9/") catch {e=>[e.code,e.data.capability]}"#, None);
    assert_eq!(result(&r), r#"["permission","http"]"#);
    let host = r#"{"format":"kiroha-host/1","capabilities":{"http":["http://127.0.0.1:9"]}}"#;
    let r = run(r#"try get("http://127.0.0.1:9/") catch {e=>[e.code,e.data.outcome]}"#, Some(host));
    assert_eq!(result(&r), r#"["network","not_sent"]"#);
}

/// A TCP server that accepts and reads but never answers.
fn silent_server() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for s in l.incoming().flatten() {
            held.push(s); // keep the connection open, never respond
        }
    });
    port
}

#[test]
fn timeout_outcome_reflects_whether_bytes_were_sent() {
    let port = silent_server();
    let host = format!(
        r#"{{"format":"kiroha-host/1","capabilities":{{"http":["http://127.0.0.1:{port}","https://127.0.0.1:{port}"]}}}}"#
    );
    // Plain HTTP: the request is written, then no response -> unknown.
    // HTTPS: the TLS handshake never completes, no request bytes -> not_sent.
    let src = format!(
        r#"[(try http("GET","http://127.0.0.1:{port}/",{{timeout:300}}) catch {{e=>[e.code,e.data.outcome]}}),
          (try http("GET","https://127.0.0.1:{port}/",{{timeout:300}}) catch {{e=>[e.code,e.data.outcome]}})]"#
    );
    let r = run(&src, Some(&host));
    assert_eq!(result(&r), r#"[["timeout","unknown"],["timeout","not_sent"]]"#);
}

#[test]
fn gzip_over_decoded_limit_is_a_catchable_limit_error() {
    let mut rnd = vec![0u8; 2_000_000];
    let mut x: u32 = 12345;
    for b in rnd.iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = x as u8;
    }
    let gz = gzip(&rnd);
    let (port, _) = fixture_server(move |_, _| {
        let mut out = format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", gz.len()).into_bytes();
        out.extend_from_slice(&gz);
        out
    });
    let r = run(
        &format!(r#"try http("GET","http://127.0.0.1:{port}/",{{max_bytes:10}}) catch {{e=>[e.code,e.data.limit,e.data.maximum]}}"#),
        Some(&host(port)),
    );
    assert_eq!(result(&r), r#"["limit","max_bytes",10]"#);
}

#[test]
fn many_response_header_fields_within_header_bytes() {
    let (port, _) = fixture_server(|_, _| {
        let mut out = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n".to_vec();
        for i in 0..150 {
            out.extend_from_slice(format!("X-{i}: a\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out
    });
    let r = run(&format!(r#"len(keys(http("GET","http://127.0.0.1:{port}/").headers))"#), Some(&host(port)));
    assert_eq!(result(&r), "151");
}

#[test]
fn unremovable_transfer_coding_is_a_framing_failure() {
    let gz = gzip(b"[7]");
    let (port, _) = fixture_server(move |_, _| {
        let mut out = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n".to_vec();
        out.extend_from_slice(format!("{:x}\r\n", gz.len()).as_bytes());
        out.extend_from_slice(&gz);
        out.extend_from_slice(b"\r\n0\r\n\r\n");
        out
    });
    let r = run(
        &format!(r#"try http("GET","http://127.0.0.1:{port}/") catch {{e=>[e.code,e.data.reason,e.data.outcome]}}"#),
        Some(&host(port)),
    );
    assert_eq!(result(&r), r#"["network","framing","unknown"]"#);
}

#[test]
fn gzip_bomb_within_max_bytes_honours_memory_quota() {
    let zeros = vec![0u8; 400_000_000];
    let gz = gzip(&zeros);
    drop(zeros);
    let (port, _) = fixture_server(move |_, _| {
        let mut out = format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", gz.len()).into_bytes();
        out.extend_from_slice(&gz);
        out
    });
    let host = format!(
        r#"{{"format":"kiroha-host/1","capabilities":{{"http":["http://127.0.0.1:{port}"]}},"limits":{{"memory_bytes":100000000}}}}"#
    );
    let r = run(&format!(r#"len(http("GET","http://127.0.0.1:{port}/",{{max_bytes:2000000000}}).body)"#), Some(&host));
    assert_eq!(r.code, 3, "{}", r.stdout);
    assert!(r.stdout.contains("\"resource\":\"memory\""), "{}", r.stdout);
}

/// Strict client framing (audit 15): bodyless statuses ignore transfer
/// codings, header_bytes counts raw octets, invalid chunked framing is a
/// transport failure.
#[test]
fn strict_client_framing() {
    let big_pad = format!("X-Big:{}a\r\n", " ".repeat(5000));
    let big_trailer = format!("X-T: {}\r\n", "a".repeat(5000));
    let (port, _) = fixture_server(move |path, _| match path {
        "/head-te" => raw("200 OK", &["Transfer-Encoding: gzip, chunked"], b""),
        "/304-te" => raw("304 Not Modified", &["Transfer-Encoding: gzip, chunked"], b""),
        "/pad" => format!("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n{big_pad}\r\n").into_bytes(),
        "/trailer" => format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n0\r\n{big_trailer}\r\n").into_bytes(),
        "/ok-chunked" => raw("200 OK", &["Transfer-Encoding: chunked"], b"2;a=\"q\"\r\n[7\r\n1\r\n]\r\n0\r\nX-T: v\r\n\r\n"),
        "/c1" => raw("200 OK", &["Transfer-Encoding: chunked"], b"2;\x01\r\nab\r\n0\r\n\r\n"),
        "/c2" => raw("200 OK", &["Transfer-Encoding: chunked"], b"2;a=\"\x01\"\r\nab\r\n0\r\n\r\n"),
        "/c3" => raw("200 OK", &["Transfer-Encoding: chunked"], b"2;=b\r\nab\r\n0\r\n\r\n"),
        "/c4" => raw("200 OK", &["Transfer-Encoding: chunked"], b"2 \r\nab\r\n0\r\n\r\n"),
        "/c5" => raw("200 OK", &["Transfer-Encoding: chunked, chunked"], b"2\r\nab\r\n0\r\n\r\n"),
        "/c6" => raw("200 OK", &["Transfer-Encoding: chunked", "Content-Length: 2"], b"2\r\nab\r\n0\r\n\r\n"),
        "/c7" => raw("200 OK", &["Content-Length: 2", "Content-Length: 3"], b"abc"),
        "/c8" => raw("200 OK", &["Transfer-Encoding: chunked"], b"2\r\nab\r\n0\r\nBad Trailer\r\n\r\n"),
        "/c9" => raw("200 OK", &["Transfer-Encoding:", "Content-Length: 2"], b"ab"),
        "/c10" => raw("200 OK", &["Transfer-Encoding: ,"], b"ab"),
        "/combined" => raw("200 OK", &["Transfer-Encoding: chunked", "X: 1234567890"], b"0\r\nY: 12345678901234567890\r\n\r\n"),
        "/interim" => b"HTTP/1.1 103 Early Hints\r\nX: 12345678901234567890\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 0\r\nX: 12345678901234567890\r\n\r\n".to_vec(),
        _ => raw("404 Not Found", &["Content-Length: 0"], b""),
    });
    let b = format!("http://127.0.0.1:{port}");
    let r = run(
        &format!(
            r#"[http("HEAD","{b}/head-te").status, len(http("HEAD","{b}/head-te").body), http("GET","{b}/304-te").status,
               get("{b}/ok-chunked"),
               map(["c1","c2","c3","c4","c5","c6","c7","c8","c9","c10"]){{p=>try http("GET","{b}/"+p) catch {{e=>[e.code,e.data.reason]}}}}]"#
        ),
        Some(&host(port)),
    );
    let framing = r#"["network","framing"]"#;
    assert_eq!(result(&r), format!("[200,0,304,[7],[{}]]", [framing; 10].join(",")));
    // header_bytes counts raw field-section octets, including padding and trailers.
    let small = format!(
        r#"{{"format":"kiroha-host/1","capabilities":{{"http":["http://127.0.0.1:{port}"]}},"limits":{{"header_bytes":1000}}}}"#
    );
    for p in ["pad", "trailer"] {
        let r = run(&format!(r#"try http("GET","{b}/{p}") catch {{e=>e.code}}"#), Some(&small));
        assert_eq!(r.code, 3, "{p}: {}", r.stdout);
        assert!(r.stdout.contains("\"resource\":\"header_bytes\""), "{p}: {}", r.stdout);
    }
    // Neither section alone exceeds 64 bytes; their total does. Interim
    // response fields also consume the same per-operation budget (§14.1).
    for p in ["combined", "interim"] {
        let r = run(&format!(r#"http("GET","{b}/{p}")"#), Some(&small.replace("1000", "64")));
        assert_eq!(r.code, 3, "{p}: {}", r.stdout);
        assert!(r.stdout.contains("\"resource\":\"header_bytes\""), "{p}: {}", r.stdout);
    }
    // Exactly 68 field octets (43 head + 25 trailer) are allowed.
    let r = run(&format!(r#"len(http("GET","{b}/combined").body)"#), Some(&small.replace("1000", "68")));
    assert_eq!(result(&r), "0");
}
