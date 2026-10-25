//! I06 filesystem, I07 task engine, I08 CLI.

mod common;
use common::*;
use std::os::unix::fs::PermissionsExt;
use std::time::Instant;

fn fs_host(root: &std::path::Path) -> String {
    let r = root.to_str().unwrap();
    format!(r#"{{"format":"kiri-host/1","filesystem_root":"{r}","capabilities":{{"read":["{r}"],"write":["{r}"]}}}}"#)
}

#[test]
fn i06_modes_symlinks_parents_and_atomic_replace() {
    let root = tempdir("fs");
    let existing = root.join("keep.txt");
    std::fs::write(&existing, "old").unwrap();
    std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o640)).unwrap();
    let outside = tempdir("outside").join("secret");
    std::fs::write(&outside, "s").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    std::os::unix::fs::symlink(outside.parent().unwrap(), root.join("dirlink")).unwrap();
    let src = r#"[write("keep.txt",encode("new")), write("new.txt",encode("n")),
      (try write("missing/x.txt",encode("x")) catch {e=>[e.code,e.data.outcome]}),
      (try write("link",encode("x")) catch {e=>[e.code,e.data.reason,e.data.outcome]}),
      (try read("link") catch {e=>e.code}), (try read("dirlink/secret") catch {e=>e.code}),
      (try read("nope") catch {e=>[e.code,e.data.reason]}), (try read("a/../b") catch {e=>e.code}),
      decode(read("keep.txt"))]"#;
    let r = run(src, Some(&fs_host(&root)));
    assert_eq!(
        result(&r),
        r#"[null,null,["io","not_applied"],["io","symlink","not_applied"],"io","io",["io","not_found"],"io","new"]"#
    );
    assert_eq!(std::fs::metadata(&existing).unwrap().permissions().mode() & 0o777, 0o640);
    assert_eq!(std::fs::metadata(root.join("new.txt")).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "s");
    assert!(!root.join("missing").exists());
    // no leftover temporary files
    assert!(std::fs::read_dir(&root).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains(".tmp")));
}

#[test]
fn i06_quota_and_permission_by_component() {
    let root = tempdir("fsq");
    let r = root.to_str().unwrap();
    let host = format!(
        r#"{{"format":"kiri-host/1","filesystem_root":"{r}","capabilities":{{"write":["{r}/out"]}},"limits":{{"max_file_bytes":2}}}}"#
    );
    std::fs::create_dir_all(root.join("out")).unwrap();
    std::fs::create_dir_all(root.join("outx")).unwrap();
    let out = run(r#"try write("outx/f",encode("a")) catch {e=>e.code}"#, Some(&host));
    assert_eq!(result(&out), r#""permission""#);
    let out = run(r#"write("out/f",encode("abc"))"#, Some(&host));
    assert_eq!(out.code, 3);
}

#[test]
fn i07_cpu_bound_cancellation_and_bounded_parallelism() {
    // A pure infinite tail loop is stopped by the root deadline.
    let t = Instant::now();
    let r = run(
        r#"go()=go(); try go() catch {e=>"caught"}"#,
        Some(r#"{"format":"kiri-host/1","limits":{"deadline_ms":200,"eval_steps":1000000000000}}"#),
    );
    assert_eq!(r.code, 3);
    assert!(r.stdout.contains("\"reason\":\"deadline\""));
    assert!(t.elapsed().as_millis() < 3000);
    // Parallel children sleep concurrently with bound 2: 4 x 200ms ≈ 400ms.
    let t = Instant::now();
    let r = run(r#"parallel([1,2,3,4],2){x=>sleep(200);x*10}"#, None);
    assert_eq!(result(&r), "[10,20,30,40]");
    let ms = t.elapsed().as_millis();
    assert!((350..1500).contains(&ms), "{ms}");
    // First failure cancels the sibling and no effect starts after it.
    let r = run(r#"parallel([1,2],2){x=>if x==1 then do{sleep(5000);1} else fail({code:"x",message:"",data:{}})}"#, None);
    assert_eq!(error_code(&r), "x");
    // Step budget exhaustion is a resource abort.
    let r = run("go(n)=go(n+1);go(0)", Some(r#"{"format":"kiri-host/1","limits":{"eval_steps":1000}}"#));
    assert!(r.stdout.contains("\"reason\":\"resource\""));
    // A million-iteration tail loop runs in bounded space under a sufficient budget.
    let r = run(
        "go(n,a)=if n==0 then a else go(n-1,a+1);go(1000000,0)",
        Some(r#"{"format":"kiri-host/1","limits":{"eval_steps":100000000}}"#),
    );
    assert_eq!(result(&r), "1000000");
}

#[test]
fn i08_cli_protocol() {
    let r = run(r#"{n:0.1+0.2,b:encode("é"),f:{x=>x},a:[db]}"#, None);
    assert_eq!(result(&r), r#"{"n":0.3,"b":b64:w6k=,"f":<function>,"a":[<function>]}"#);
    let r = run(r#"log(1)"#, Some(r#"{"format":"kiri-host/1","capabilities":{"log":true}}"#));
    assert_eq!(r.stderr, "1\n");
    assert!(!r.stdout.contains("\n1\n"));
    // exit codes
    assert_eq!(run("1+", None).code, 1);
    assert_eq!(run("fail({code:\"c\",message:\"\",data:{}})", None).code, 1);
    assert_eq!(run("x", Some("{\"format\":\"wrong\"}")).code, 2);
    assert_eq!(run("1", Some(r#"{"format":"kiri-host/1","limits":{"source_bytes":0}}"#)).code, 2);
    let r = run("1", Some(r#"{"format":"kiri-host/1","limits":{"source_bytes":1}}"#));
    assert_eq!(r.code, 0);
    let r = run("12", Some(r#"{"format":"kiri-host/1","limits":{"source_bytes":1}}"#));
    assert_eq!(r.code, 2);
    assert!(r.stdout.contains("source_limit"));
    // check performs no effects (log would write to stderr)
    let dir = tempdir("cli");
    let p = dir.join("x.kiri");
    std::fs::write(&p, "log(1); if false then 1 else 2").unwrap();
    let out = finish(kiri().arg("check").arg(&p).arg("--json").output().unwrap());
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    let out = finish(kiri().arg("check").arg(dir.join("missing.kiri")).arg("--json").output().unwrap());
    assert_eq!(out.code, 2);
    assert!(out.stdout.contains("source_io"));
    std::fs::write(&p, [0xffu8, 0xfe]).unwrap();
    let out = finish(kiri().arg("check").arg(&p).arg("--json").output().unwrap());
    assert_eq!(out.code, 1);
    assert!(out.stdout.contains("\"phase\":\"lex\""));
    // usage errors
    let out = finish(kiri().arg("run").arg(&p).arg("--json").arg("--json").output().unwrap());
    assert_eq!(out.code, 2);
    let out = finish(kiri().arg("frobnicate").output().unwrap());
    assert_eq!((out.code, out.stdout.as_str()), (2, ""));
    // stdin source
    let mut c = kiri().args(["run", "-", "--json"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    use std::io::Write;
    c.stdin.take().unwrap().write_all(b"[1,\"a\"]").unwrap();
    let out = finish(c.wait_with_output().unwrap());
    assert_eq!(result(&out), r#"[1,"a"]"#);
    // spec card and version
    let out = finish(kiri().args(["spec", "--compact"]).output().unwrap());
    assert!(out.stdout.starts_with("Kiri 0.3.0 / Web1\n") && out.stdout.ends_with("do not infer absent APIs.\n"));
    let out = finish(kiri().arg("--version").output().unwrap());
    assert!(out.stdout.contains("\"cli\":\"kiri-cli/1\""));
    // conformance corpus passes
    let spec = concat!(env!("CARGO_MANIFEST_DIR"), "/spec.md");
    let out = finish(kiri().args(["conformance", spec, "--json"]).output().unwrap());
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("\"failed\":0"));
}
