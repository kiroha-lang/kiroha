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
                let mut h = vec![format!("Content-Length: {}", gz.len()), "Content-Encoding: gzip".into()];
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
    format!(r#"{{"format":"kiri-host/1","capabilities":{{"http":["http://127.0.0.1:{port}"]}}}}"#)
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
    let host = r#"{"format":"kiri-host/1","capabilities":{"http":["http://127.0.0.1:9"]}}"#;
    let r = run(r#"try get("http://127.0.0.1:9/") catch {e=>[e.code,e.data.outcome]}"#, Some(host));
    assert_eq!(result(&r), r#"["network","not_sent"]"#);
}
