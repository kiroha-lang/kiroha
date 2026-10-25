//! I03/I04: PostgreSQL 16 parameters/results and transactions.
//! Requires `KIRI_TEST_PG_DSN` (e.g. `host=127.0.0.1 port=55432 user=kiri dbname=postgres`).
//! Without it the tests print an explicit SKIP line.

mod common;
use common::*;

fn dsn() -> Option<String> {
    std::env::var("KIRI_TEST_PG_DSN").ok()
}

fn admin(sql: &str) -> Vec<Vec<Option<String>>> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let (c, conn) = tokio_postgres::connect(&dsn().unwrap(), tokio_postgres::NoTls).await.unwrap();
        tokio::spawn(conn);
        let mut out = Vec::new();
        for m in c.simple_query(sql).await.unwrap() {
            if let tokio_postgres::SimpleQueryMessage::Row(r) = m {
                out.push((0..r.len()).map(|i| r.get(i).map(|s| s.to_string())).collect());
            }
        }
        out
    })
}

fn host() -> String {
    r#"{"format":"kiri-host/1","capabilities":{"databases":["main"]},"database_pools":{"main":{"dsn_env":"KIRI_PG_MAIN","max_connections":2}}}"#.into()
}

fn pg(src: &str) -> Run {
    let d = dsn().unwrap();
    run_env(src, Some(&host()), &[("KIRI_PG_MAIN", &d)])
}

macro_rules! need_pg {
    () => {
        if dsn().is_none() {
            eprintln!("SKIP: KIRI_TEST_PG_DSN not set");
            return;
        }
    };
}

#[test]
fn i03_parameters_results_and_restrictions() {
    need_pg!();
    let r = pg(r#"d=db("main");x=one(sql(d,q`select ${null}::text as z, ${true}::boolean as b, ${"x'); drop table t; --"}::text as s,
        ${123456789012345678901234567890.123456789}::numeric as n, ${encode("é")}::bytea as raw, ${{a:1.50,b:[1e30]}}::jsonb as obj,
        ${[1,2]}::json as arr, 7::int2 as i2, 8::int4 as i4, 9007199254740993::int8 as i8, 'ab'::varchar as vc, 'ab'::char(4) as bp,
        '00000000-0000-4000-8000-000000000000'::uuid as u`).rows);
        [x.z,x.b,x.s,x.n,decode(x.raw),x.obj,x.arr,x.i2,x.i4,x.i8,x.vc,x.bp,x.u]"#);
    assert_eq!(
        result(&r),
        r#"[null,true,"x'); drop table t; --",1.23456789012345678901234567890123456789e29,"é",{"a":1.5,"b":[1e30]},[1,2],7,8,9007199254740993,"ab","ab  ","00000000-0000-4000-8000-000000000000"]"#
    );
    let r = pg(r#"d=db("main");[
        (try sql(d,q`select 1 as a, 2 as a`) catch {e=>[e.code,e.data.phase]}),
        (try sql(d,q`select current_date`) catch {e=>[e.code,e.data.phase]}),
        (try sql(d,q`select 1.5::float8`) catch {e=>[e.code,e.data.phase]}),
        (try sql(d,q`select array[1,2]`) catch {e=>[e.code,e.data.phase]}),
        (try sql(d,q`select 'NaN'::numeric`) catch {e=>[e.code,e.data.phase]}),
        (try sql(d,q`create table x(y int)`) catch {e=>[e.code,e.data.phase,e.data.reason]}),
        (try sql(d,q`select 1; select 2`) catch {e=>e.data.reason}),
        (try sql(d,q`select nosuchcol`) catch {e=>[e.code,e.data.phase,e.data.sqlstate]}),
        (try sql(d,q`select ${"a\u0000"}::text`) catch {e=>[e.code,e.data.phase]}),
        sql(d,q`select to_jsonb(array[1,2]) as a, current_date::text <> '' as ok`).rows,
        sql(d,q`select 'a;b' as s -- trailing ; comment`).count]"#);
    assert_eq!(
        result(&r),
        r#"[["db","decode"],["db","decode"],["db","decode"],["db","decode"],["db","decode"],["db","prepare","statement"],"multiple_statements",["db","execute","42703"],["db","bind"],[{"a":[1,2],"ok":true}],1]"#
    );
}

#[test]
fn i04_transactions_and_pool_hygiene() {
    need_pg!();
    admin("drop table if exists kiri_t; create table kiri_t(id serial primary key, v int unique)");
    // commit, then counts for DML without RETURNING
    let r = pg(r#"d=db("main");[tx(d){t=>sql(t,q`insert into kiri_t(v) values (1),(2)`).count}, sql(d,q`update kiri_t set v=v+10`).count]"#);
    assert_eq!(result(&r), "[2,2]");
    assert_eq!(admin("select count(*) from kiri_t"), vec![vec![Some("2".into())]]);
    // callback failure rolls back
    let r = pg(r#"d=db("main");try tx(d){t=>sql(t,q`insert into kiri_t(v) values (100)`);fail({code:"boom",message:"",data:{}})} catch {e=>e.code}"#);
    assert_eq!(result(&r), r#""boom""#);
    assert_eq!(admin("select count(*) from kiri_t where v=100"), vec![vec![Some("0".into())]]);
    // caught poison stays primary and rolls back
    let r = pg(r#"d=db("main");try tx(d){t=>sql(t,q`insert into kiri_t(v) values (200)`);
        try sql(t,q`insert into kiri_t(v) values (11)`) catch {e=>null}; 7} catch {e=>[e.code,e.data.sqlstate]}"#);
    assert_eq!(result(&r), r#"["db","23505"]"#);
    assert_eq!(admin("select count(*) from kiri_t where v=200"), vec![vec![Some("0".into())]]);
    // tx timeout aborts a pure loop and pending work, then rolls back
    let r = pg(r#"d=db("main");spin()=spin();try tx(d,{timeout:200}){t=>sql(t,q`insert into kiri_t(v) values (300)`);spin()} catch {e=>[e.code,e.data.operation]}"#);
    assert_eq!(result(&r), r#"["timeout","tx"]"#);
    assert_eq!(admin("select count(*) from kiri_t where v=300"), vec![vec![Some("0".into())]]);
    // tx timeout during a running statement cancels it server-side, so the
    // rollback is not queued behind it (well under the 1000 ms cleanup grace)
    let t = std::time::Instant::now();
    let r = pg(r#"try tx(db("main"),{timeout:100}){t=>sql(t,q`select 1 as x from pg_sleep(3)`)} catch {e=>[e.code,e.data.operation]}"#);
    assert_eq!(result(&r), r#"["timeout","tx"]"#);
    assert!(t.elapsed().as_millis() < 900, "{:?}", t.elapsed());
    assert!(!r.stderr.contains("detached"), "{}", r.stderr);
    // max_rows stops buffering early (memory quota far below the full result)
    let d = dsn().unwrap();
    let host_small = r#"{"format":"kiri-host/1","capabilities":{"databases":["main"]},"database_pools":{"main":{"dsn_env":"KIRI_PG_MAIN"}},"limits":{"memory_bytes":30000000}}"#;
    let r = run_env(
        r#"try sql(db("main"),q`select generate_series(1,5000000) x, repeat(${"a"},100) y`,{max_rows:10}) catch {e=>[e.code,e.data.limit]}"#,
        Some(host_small),
        &[("KIRI_PG_MAIN", &d)],
    );
    assert_eq!(result(&r), r#"["limit","max_rows"]"#);
    // values beyond the host number quota are resource aborts, not db errors
    let r = pg(r#"try sql(db("main"),q`select repeat('9',110000)::numeric as n`) catch {e=>e.code}"#);
    assert_eq!(r.code, 3, "{}", r.stdout);
    assert!(r.stdout.contains("\"resource\":\"number\""), "{}", r.stdout);
    // decoded row buffers count against memory_bytes
    let host_mem = r#"{"format":"kiri-host/1","capabilities":{"databases":["main"]},"database_pools":{"main":{"dsn_env":"KIRI_PG_MAIN"}},"limits":{"memory_bytes":100000000}}"#;
    let r = run_env(
        r#"len(sql(db("main"),q`select generate_series(1,1500000) x, repeat(${"a"},500) y`,{max_rows:5000000}).rows)"#,
        Some(host_mem),
        &[("KIRI_PG_MAIN", &dsn().unwrap())],
    );
    assert_eq!(r.code, 3, "{}", r.stdout);
    assert!(r.stdout.contains("\"resource\":\"memory\""), "{}", r.stdout);
    // sql timeout on a slow statement
    let r = pg(r#"try sql(db("main"),q`select pg_sleep(5)`,{timeout:200}) catch {e=>[e.code,e.data.outcome]}"#);
    assert_eq!(result(&r), r#"["timeout","unknown"]"#);
    // task confinement and nested tx
    let r = pg(r#"d=db("main");tx(d){t=>[parallel([1],1){x=>try sql(t,q`select 1`) catch {e=>e.data.phase}},
        (try tx(d){u=>1} catch {e=>e.data.phase})]}"#);
    assert_eq!(result(&r), r#"[["ownership"],"ownership"]"#);
    // independent autocommit inside tx survives rollback
    let r = pg(r#"d=db("main");try tx(d){t=>sql(d,q`insert into kiri_t(v) values (400)`);fail({code:"x",message:"",data:{}})} catch {e=>e.code}"#);
    assert_eq!(result(&r), r#""x""#);
    assert_eq!(admin("select count(*) from kiri_t where v=400"), vec![vec![Some("1".into())]]);
    // holes in VALUES, RETURNING and LIMIT positions are parameters
    let r = pg(r#"d=db("main");x=sql(d,q`insert into kiri_t(v) values (${500}) returning v + ${1} as w`);
        [one(x.rows).w, len(sql(d,q`select v from kiri_t order by v limit ${2}`).rows)]"#);
    assert_eq!(result(&r), "[501,2]");
    // SELECT INTO in a set operation's leftmost branch is rejected before dispatch
    admin("drop table if exists kiri_into");
    let r = pg(r#"try sql(db("main"),q`select 1 as x into kiri_into union select 2`) catch {e=>[e.data.phase,e.data.reason]}"#);
    assert_eq!(result(&r), r#"["prepare","statement"]"#);
    assert_eq!(admin("select to_regclass('kiri_into') is null"), vec![vec![Some("t".into())]]);
    // multi-column SET with holes is valid
    let r = pg(r#"sql(db("main"),q`update kiri_t set (v, id) = (${600}, id) where v = ${500}`).count"#);
    assert_eq!(result(&r), "1");
    // no dirty pooled reuse: session settings are reset before reuse
    let r = pg(r#"d=db("main");a=sql(d,q`select set_config('search_path','nowhere',false) as s`);
        b=sql(d,q`select current_setting('search_path') as s`);[one(b.rows).s]"#);
    assert_eq!(result(&r), r#"["\"$user\", public"]"#);
}

/// Proxy to PostgreSQL that forwards traffic but, once the client sends a
/// simple-query COMMIT, forwards it and then drops both connections without
/// relaying the acknowledgment.
fn commit_dropping_proxy(pg_port: u16) -> u16 {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for client in l.incoming().flatten() {
            let server = std::net::TcpStream::connect(("127.0.0.1", pg_port)).unwrap();
            let (mut c_rd, mut s_wr) = (client.try_clone().unwrap(), server.try_clone().unwrap());
            let (mut s_rd, mut c_wr) = (server, client);
            let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let d1 = dropped.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 65536];
                loop {
                    let n = match c_rd.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let _ = s_wr.write_all(&buf[..n]);
                    if buf[..n].windows(12).any(|w| w == b"Q\0\0\0\x0bCOMMIT\0") {
                        d1.store(true, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        let _ = c_rd.shutdown(std::net::Shutdown::Both);
                        let _ = s_wr.shutdown(std::net::Shutdown::Both);
                        break;
                    }
                }
            });
            std::thread::spawn(move || {
                let mut buf = [0u8; 65536];
                loop {
                    let n = match s_rd.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if dropped.load(std::sync::atomic::Ordering::SeqCst) {
                        break; // swallow the COMMIT acknowledgment
                    }
                    let _ = c_wr.write_all(&buf[..n]);
                }
            });
        }
    });
    port
}

#[test]
fn i04_lost_commit_acknowledgment_is_unknown_without_replay() {
    need_pg!();
    let d = dsn().unwrap();
    let pg_port: u16 = d
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix("port="))
        .and_then(|p| p.parse().ok())
        .unwrap_or(5432);
    admin("drop table if exists kiri_lost; create table kiri_lost(v int)");
    let proxy = commit_dropping_proxy(pg_port);
    let proxied = d.replace(&format!("port={pg_port}"), &format!("port={proxy}")) + " sslmode=disable";
    let src = r#"try tx(db("main")){t=>sql(t,q`insert into kiri_lost(v) values (1)`);7} catch {e=>[e.code,e.data.phase,e.data.outcome]}"#;
    let r = run_env(src, Some(&host()), &[("KIRI_PG_MAIN", &proxied)]);
    assert_eq!(result(&r), r#"["db","commit","unknown"]"#);
    // The callback was not replayed: at most one row exists (the server may
    // or may not have committed — that is exactly why the outcome is unknown).
    let rows = admin("select count(*) from kiri_lost");
    let n: u32 = rows[0][0].as_deref().unwrap().parse().unwrap();
    assert!(n <= 1, "callback replayed: {n} rows");
}
