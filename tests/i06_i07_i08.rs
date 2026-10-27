//! I06 filesystem, I07 task engine, I08 CLI.

mod common;
use common::*;
use std::os::unix::fs::PermissionsExt;
use std::time::Instant;

fn fs_host(root: &std::path::Path) -> String {
    let r = root.to_str().unwrap();
    format!(r#"{{"format":"kiroha-host/1","filesystem_root":"{r}","capabilities":{{"read":["{r}"],"write":["{r}"]}}}}"#)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn i06_failed_random_source_never_creates_predictable_temp_file() {
    let tools = tempdir("rng-fault");
    let c = tools.join("deny.c");
    let so = tools.join("deny.so");
    std::fs::write(&c, include_str!("fixtures/deny_file_random.c")).unwrap();
    assert!(std::process::Command::new("cc").args(["-shared", "-fPIC"]).arg(&c).arg("-ldl").arg("-o").arg(&so).status().unwrap().success());
    let root = tempdir("rng-file");
    std::fs::write(root.join("out"), "old").unwrap();
    let r = run_env(r#"try write("out",encode("new")) catch {e=>[e.code,e.data.reason,e.data.outcome]}"#,
        Some(&fs_host(&root)), &[("LD_PRELOAD", so.to_str().unwrap())]);
    assert!(r.stderr.contains("audit-denied-getrandom8"), "fault injection was not reached: {}", r.stderr);
    assert_eq!(result(&r), r#"["io","random_source","not_applied"]"#);
    assert_eq!(std::fs::read_to_string(root.join("out")).unwrap(), "old");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1, "no temporary file was created");
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
        r#"{{"format":"kiroha-host/1","filesystem_root":"{r}","capabilities":{{"write":["{r}/out"]}},"limits":{{"max_file_bytes":2}}}}"#
    );
    std::fs::create_dir_all(root.join("out")).unwrap();
    std::fs::create_dir_all(root.join("outx")).unwrap();
    let out = run(r#"try write("outx/f",encode("a")) catch {e=>e.code}"#, Some(&host));
    assert_eq!(result(&out), r#""permission""#);
    let out = run(r#"write("out/f",encode("abc"))"#, Some(&host));
    assert_eq!(out.code, 3);
    // A valid maximum u64 quota must not wrap the read sentinel to zero.
    std::fs::write(root.join("small"), "hello").unwrap();
    let mut host = fs_host(&root);
    host.pop();
    host.push_str(r#", "limits":{"max_file_bytes":18446744073709551615}}"#);
    assert_eq!(result(&run(r#"decode(read("small"))"#, Some(&host))), r#""hello""#);
}

/// A cancelled `write` leaves the old target in place and no temp file (§13.2).
#[test]
fn i06_cancelled_write_does_not_replace_target() {
    let root = tempdir("fs-cancel");
    let out = root.join("out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("race"), "OLD\n").unwrap();
    let r = root.to_str().unwrap();
    let host = format!(
        r#"{{"format":"kiroha-host/1","filesystem_root":"{r}","capabilities":{{"read":["{r}/out"],"write":["{r}/out"]}},"limits":{{"memory_bytes":4000000000,"max_file_bytes":300000000}}}}"#
    );
    let src = r#"s=range(0,27).fold("x"){(a,i)=>a+a}; b=encode(s);
      r=try [1,2].parallel(2){x=>if x==1 then write("out/race",b) else do {sleep(1); fail({code:"boom",message:"m",data:{}})}} catch {e=>e.code};
      [r,read("out/race").size]"#;
    let res = run(src, Some(&host));
    assert_eq!(result(&res), r#"["boom",4]"#);
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert_eq!(std::fs::read_to_string(out.join("race")).unwrap(), "OLD\n");
    let names: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names, vec!["race".to_string()], "no temporary files remain");
}

#[test]
fn i07_cpu_bound_cancellation_and_bounded_parallelism() {
    // A pure infinite tail loop is stopped by the root deadline.
    let t = Instant::now();
    let r = run(
        r#"fun go():any go() end fun go try go() catch {e=>"caught"}"#,
        Some(r#"{"format":"kiroha-host/1","limits":{"deadline_ms":200,"eval_steps":1000000000000}}"#),
    );
    assert_eq!(r.code, 3);
    assert!(r.stdout.contains("\"reason\":\"deadline\""));
    assert!(t.elapsed().as_millis() < 3000);
    // A finite builtin may legitimately finish before a wall-clock deadline
    // on a fast host. Repeat the work so success cannot race the assertion.
    let repeat = |src: &str| format!("fun again():any {src};again() end fun again again()");
    // Long builtin work (JSON, equality, schema checks) observes the deadline.
    for src in [
        "fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value x=dump(range(0,3000000));kiroha_runtime_value(parse(x)).size",
        "range(0,2000000)==range(0,2000000)",
        "(range(0,3000000)).as([int])",
        "xs=range(0,3000000);[xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs,xs].flat().size",
        "s=dump(range(0,3000000));s2=s+s+s+s+s+s+s+s;s3=s2+s2+s2+s2;(s3+s3).size",
        "s=dump(range(0,3000000));(s+s+s+s).replace(\"1\", \"aaaaaaaaaaaaaaaa\").size",
    ] {
        let t = Instant::now();
        let r = run(&repeat(src), Some(r#"{"format":"kiroha-host/1","limits":{"deadline_ms":20,"eval_steps":100000000000,"memory_bytes":4000000000}}"#));
        assert!(r.stdout.contains("\"reason\":\"deadline\""), "{src}: {}", r.stdout);
        assert!(t.elapsed().as_millis() < 1500, "{src}");
    }
    // JSON schema checks and JSON key hashing over huge shared structures
    // observe the deadline.
    for src in [
        "fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value z=kiroha_runtime_value(range(0,60).fold)([1]){(a,i)=>[a,a]}; (z).as(json)",
        "fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value z=kiroha_runtime_value(range(0,60).fold)([1]){(a,i)=>[a,a]}; [z,z].unique()",
        "fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value z=kiroha_runtime_value(range(0,60).fold)([1]){(a,i)=>[a,a]}; pages(z){c=>{items:[],next:null}}",
    ] {
        let t = Instant::now();
        let r = run(src, Some(r#"{"format":"kiroha-host/1","limits":{"deadline_ms":300}}"#));
        assert!(r.stdout.contains("\"reason\":\"deadline\""), "{src}: {}", r.stdout);
        assert!(t.elapsed().as_millis() < 3000, "{src}");
    }
    // More long builtin work on huge or shared inputs observes the deadline
    // (audit 10): schema normalization, sorting, closed-record extras, pick,
    // JSON escaping, URL encoding, hashing/equality of huge shared strings.
    let big = "s=range(0,24).fold(\"x\"){(a,i)=>a+a};";
    for src in [
        "fun d(x:any,n:any):any if n==0 then x else d({a:x,b:x},n-1) end fun d (1).as(maybe(d(json,26)))".to_string(),
        format!("{big} range(0,3000).map{{i=>s}}.sort()"),
        "c=range(0,26).fold(\"\\u0001\"){(a,i)=>a+a}; dump(c).size".to_string(),
        format!("{big} url(\"https://a.example\",[s.replace(\"x\", \" \")]).size"),
        format!("{big} xs=range(0,3000).map{{i=>s+\"\"}}; xs.unique().size"),
        format!("{big} xs=range(0,3000).map{{i=>s+\"\"}}; xs==xs.map{{x=>x+\"\"}}"),
    ] {
        let t = Instant::now();
        let r = run(&repeat(&src), Some(r#"{"format":"kiroha-host/1","limits":{"deadline_ms":400,"eval_steps":100000000000,"memory_bytes":8000000000}}"#));
        assert!(r.stdout.contains("\"reason\":\"deadline\""), "{src}: {}", r.stdout);
        assert!(t.elapsed().as_millis() < 3500, "{src}: {:?}", t.elapsed());
    }
    // Former O(n^2) paths are linear: closed-record extras and pick duplicates.
    for (src, want) in [
        (
            "fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value v=parse(\"{\"+range(0,100000).map{i=>\"\\\"k\"+text(i)+\"\\\":{}\"}.join(\",\")+\"}\"); v.as(kiroha_runtime_value(v)).size",
            "100000",
        ),
        ("try {}.pick(range(0,150000).map{i=>text(i)}) catch {e=>e.code}", "\"missing\""),
    ] {
        let t = Instant::now();
        let r = run(src, Some(r#"{"format":"kiroha-host/1","limits":{"eval_steps":100000000000}}"#));
        assert_eq!(result(&r), want, "{src}");
        assert!(t.elapsed().as_millis() < 5000, "{src}: {:?}", t.elapsed());
    }
    // dump/parse/CLI output of many huge numbers check the deadline per number.
    for src in [
        "xs=range(0,30).map{i=>number(\"1e99990\")+i}; ys=range(0,6).fold(xs){(a,i)=>a+a}; dump(ys).size",
        "fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value p=range(0,16).fold(\"7\"){(a,i)=>a+a}; s=\"[\"+range(0,1900).map{i=>p}.join(\",\")+\"]\"; kiroha_runtime_value(parse(s)).size",
        "xs=range(0,30).map{i=>number(\"1e99990\")+i}; range(0,6).fold(xs){(a,i)=>a+a}",
    ] {
        let t = Instant::now();
        let r = run(&repeat(src), Some(r#"{"format":"kiroha-host/1","limits":{"deadline_ms":1500,"eval_steps":100000000000,"memory_bytes":8000000000}}"#));
        assert!(r.stdout.contains("\"reason\":\"deadline\""), "{src}: {}", r.stdout);
        assert!(t.elapsed().as_millis() < 2600, "{src}: {:?}", t.elapsed());
    }
    // Huge exponent spellings are rejected without quadratic parsing.
    let t = Instant::now();
    let r = run(
        "fun d(n:any,a:any):any if n==0 then a else d(n-1,a+a) end fun d number(\"1e\"+d(22,\"1\"))",
        Some(r#"{"format":"kiroha-host/1","limits":{"memory_bytes":4000000000}}"#),
    );
    assert!(r.stdout.contains("\"resource\":\"number\""), "{}", r.stdout);
    assert!(t.elapsed().as_millis() < 3000);
    // Converting a huge (shared) result for the CLI honours the memory quota.
    let r = run(
        "fun d(x:any,n:any):any if n==0 then x else d([x,x],n-1) end fun d d(1,22)",
        Some(r#"{"format":"kiroha-host/1","limits":{"memory_bytes":100000000}}"#),
    );
    assert_eq!(r.code, 3, "{}", r.stdout);
    assert!(r.stdout.contains("\"resource\":\"memory\""), "{}", r.stdout);
    // SIGINT during long builtin work is observed (cancelled abort, exit 3).
    {
        let dir = tempdir("sig");
        let p = dir.join("s.kiroha");
        std::fs::write(&p, "fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value z=kiroha_runtime_value(range(0,60).fold)([1]){(a,i)=>[a,a]}; dump(z)").unwrap();
        let child = kiroha().arg("run").arg(&p).arg("--json").stdout(std::process::Stdio::piped()).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        unsafe {
            libc::kill(child.id() as i32, libc::SIGINT);
        }
        let out = finish(child.wait_with_output().unwrap());
        assert_eq!(out.code, 3, "{}", out.stdout);
        assert!(out.stdout.contains("\"reason\":\"cancelled\""));
    }
    // memory_bytes is enforced before large allocations (+, dump, encode)
    for src in [
        "fun d(n:any,a:any):any if n==0 then a else d(n-1,a+a) end fun d d(40,[1]).size",
        "fun d(n:any,a:any):any if n==0 then a else d(n-1,a+a) end fun d d(40,\"x\").size",
        "fun d(n:any,a:any):any if n==0 then a else d(n-1,a+a) end fun d x=d(20,\"x\"); dump(range(0,500).map{i=>x}).size",
        "fun d(n:any,a:any):any if n==0 then a else d(n-1,a+a) end fun d x=d(26,\"x\"); encode(x).size+encode(x).size",
    ] {
        let r = run(src, Some(r#"{"format":"kiroha-host/1","limits":{"memory_bytes":50000000,"eval_steps":100000000000}}"#));
        assert_eq!(r.code, 3, "{src}: {}", r.stdout);
        assert!(r.stdout.contains("\"resource\":\"memory\""), "{src}: {}", r.stdout);
    }
    // Huge coefficients hit the host number quota promptly instead of
    // monopolizing the evaluator.
    let t = Instant::now();
    let r = run(
        "fun d(n:any,a:any):any if n==0 then a else d(n-1,a+a) end fun d number(d(21,\"1\"))",
        Some(r#"{"format":"kiroha-host/1","limits":{"memory_bytes":4000000000,"deadline_ms":300}}"#),
    );
    assert_eq!(r.code, 3, "{}", r.stdout);
    assert!(t.elapsed().as_millis() < 2000);
    let r = run("x=1e20000000+1; x*x", None);
    assert!(r.stdout.contains("\"resource\":\"number\""), "{}", r.stdout);
    // Parallel children sleep concurrently with bound 2: 4 x 200ms ≈ 400ms.
    let t = Instant::now();
    let r = run(r#"[1,2,3,4].parallel(2){x=>sleep(200);x*10}"#, None);
    assert_eq!(result(&r), "[10,20,30,40]");
    let ms = t.elapsed().as_millis();
    assert!((350..1500).contains(&ms), "{ms}");
    // First failure cancels the sibling and no effect starts after it.
    let r = run(r#"[1,2].parallel(2){x=>if x==1 then do{sleep(5000);1} else fail({code:"x",message:"",data:{}})}"#, None);
    assert_eq!(error_code(&r), "x");
    // Step budget exhaustion is a resource abort.
    let r = run("fun go(n:any):any go(n+1) end fun go go(0)", Some(r#"{"format":"kiroha-host/1","limits":{"eval_steps":1000}}"#));
    assert!(r.stdout.contains("\"reason\":\"resource\""));
    // A million-iteration tail loop runs in bounded space under a sufficient budget.
    let r = run(
        "fun go(n:any,a:any):any if n==0 then a else go(n-1,a+1) end fun go go(1000000,0)",
        Some(r#"{"format":"kiroha-host/1","limits":{"eval_steps":100000000}}"#),
    );
    assert_eq!(result(&r), "1000000");
}

#[test]
fn i08_cli_protocol() {
    let r = run(r#"{n:0.1+0.2,b:encode("é"),f:{x:any=>x},a:[db]}"#, None);
    assert_eq!(result(&r), r#"{"n":0.3,"b":b64:w6k=,"f":<function>,"a":[<function>]}"#);
    let r = run(r#"log(1)"#, Some(r#"{"format":"kiroha-host/1","capabilities":{"log":true}}"#));
    assert_eq!(r.stderr, "1\n");
    assert!(!r.stdout.contains("\n1\n"));
    // exit codes
    assert_eq!(run("1+", None).code, 1);
    assert_eq!(run("fail({code:\"c\",message:\"\",data:{}})", None).code, 1);
    assert_eq!(run("x", Some("{\"format\":\"wrong\"}")).code, 2);
    assert_eq!(run("1", Some(r#"{"format":"kiroha-host/1","capabilities":{"read":["/a","/a/"]}}"#)).code, 2);
    assert_eq!(run("1", Some(r#"{"format":"kiroha-host/1","limits":{"eval_steps":99999999999999999999999999}}"#)).code, 0);
    for count in ["4294967296", "1e100001"] {
        let h = format!(r#"{{"format":"kiroha-host/1","database_pools":{{"unused":{{"dsn_env":"KIROHA_UNUSED_DSN","max_connections":{count}}}}}}}"#);
        assert_eq!(result(&run("1", Some(&h))), "1", "a valid unused pool count does not allocate connections");
    }
    assert_eq!(run("1", Some(r#"{"format":"kiroha-host/1","limits":{"source_bytes":0}}"#)).code, 2);
    let r = run("1", Some(r#"{"format":"kiroha-host/1","limits":{"source_bytes":1}}"#));
    assert_eq!(r.code, 0);
    let r = run("12", Some(r#"{"format":"kiroha-host/1","limits":{"source_bytes":1}}"#));
    assert_eq!(r.code, 2);
    assert!(r.stdout.contains("source_limit"));
    // check performs no effects (log would write to stderr)
    let dir = tempdir("cli");
    let p = dir.join("x.kiroha");
    std::fs::write(&p, "log(1); if false then 1 else 2").unwrap();
    let out = finish(kiroha().arg("check").arg(&p).arg("--json").output().unwrap());
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
    let out = finish(kiroha().arg("check").arg(dir.join("missing.kiroha")).arg("--json").output().unwrap());
    assert_eq!(out.code, 2);
    assert!(out.stdout.contains("source_io"));
    std::fs::write(&p, [0xffu8, 0xfe]).unwrap();
    let out = finish(kiroha().arg("check").arg(&p).arg("--json").output().unwrap());
    assert_eq!(out.code, 1);
    assert!(out.stdout.contains("\"phase\":\"lex\""));
    // usage errors
    let out = finish(kiroha().arg("run").arg(&p).arg("--json").arg("--json").output().unwrap());
    assert_eq!(out.code, 2);
    for args in [&["spec", "--compact", "--json"][..], &["--version", "--json"], &["foo", "--json"]] {
        let out = finish(kiroha().args(args).output().unwrap());
        assert_eq!((out.code, out.stdout.as_str()), (2, ""), "{args:?}");
        assert!(!out.stderr.is_empty());
    }
    let out = finish(kiroha().arg("frobnicate").output().unwrap());
    assert_eq!((out.code, out.stdout.as_str()), (2, ""));
    // stdin source
    let mut c = kiroha().args(["run", "-", "--json"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    use std::io::Write;
    c.stdin.take().unwrap().write_all(b"[1,\"a\"]").unwrap();
    let out = finish(c.wait_with_output().unwrap());
    assert_eq!(result(&out), r#"[1,"a"]"#);
    // spec card and version
    let out = finish(kiroha().args(["spec", "--compact"]).output().unwrap());
    assert!(out.stdout.starts_with("Kiroha 0.7.0 / Web1\n") && out.stdout.ends_with("do not infer absent APIs.\n"));
    let out = finish(kiroha().arg("--version").output().unwrap());
    assert!(out.stdout.contains("\"cli\":\"kiroha-cli/1\""));
    // conformance corpus passes
    let spec = concat!(env!("CARGO_MANIFEST_DIR"), "/spec.md");
    let out = finish(kiroha().args(["conformance", spec, "--json"]).output().unwrap());
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("\"failed\":0"));
}
