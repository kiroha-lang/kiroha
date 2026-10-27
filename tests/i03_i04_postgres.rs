//! I03/I04: PostgreSQL 16 parameters/results and transactions.
//! Requires `KIROHA_TEST_PG_DSN` (e.g. `host=127.0.0.1 port=55432 user=kiroha dbname=postgres`).
//! Without it the tests print an explicit SKIP line.

mod common;
use common::*;

fn dsn() -> Option<String> {
    std::env::var("KIROHA_TEST_PG_DSN").ok()
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
    r#"{"format":"kiroha-host/1","capabilities":{"databases":["main"]},"database_pools":{"main":{"dsn_env":"KIROHA_PG_MAIN","max_connections":2}}}"#.into()
}

fn pg(src: &str) -> Run {
    let d = dsn().unwrap();
    run_env(src, Some(&host()), &[("KIROHA_PG_MAIN", &d)])
}

macro_rules! need_pg {
    () => {
        if dsn().is_none() {
            eprintln!("SKIP: KIROHA_TEST_PG_DSN not set");
            return;
        }
    };
}

#[test]
fn cancelled_connection_setup_returns_pool_slot() {
    need_pg!();
    let d = dsn().unwrap();
    let cfg: tokio_postgres::Config = d.parse().unwrap();
    let pg_port = cfg.get_ports().first().copied().unwrap_or(5432);
    let (ready, port) = std::sync::mpsc::channel();
    // Stall just the first startup handshake. The second connection reaches
    // the real database, proving cancellation did not exhaust the single slot.
    let proxy = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            ready.send(listener.local_addr().unwrap().port()).unwrap();
            let mut tasks = Vec::new();
            for index in 0..2 {
                let (mut client, _) = tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
                tasks.push(tokio::spawn(async move {
                    if index == 0 {
                        tokio::io::copy(&mut client, &mut tokio::io::sink()).await.ok();
                    } else {
                        let mut server = tokio::net::TcpStream::connect(("127.0.0.1", pg_port)).await.unwrap();
                        tokio::io::copy_bidirectional(&mut client, &mut server).await.ok();
                    }
                }));
            }
            for task in tasks {
                tokio::time::timeout(std::time::Duration::from_secs(5), task).await.unwrap().unwrap();
            }
        });
    });
    let port = port.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    // DSNs in the integration environment use a single localhost endpoint.
    let proxied = format!("host=127.0.0.1 port={port} user={} dbname={} sslmode=disable", cfg.get_user().unwrap(), cfg.get_dbname().unwrap_or("postgres"));
    let h = host().replace("\"max_connections\":2", "\"max_connections\":1");
    let r = run_env(
        r#"d=db("main"); [(try sql(d,q`select 1 as x`,{timeout:100}) catch {e=>e.code}), sql(d,q`select 2 as x`,{timeout:2000}).rows.one().x]"#,
        Some(&h), &[("KIROHA_PG_MAIN", &proxied)],
    );
    proxy.join().unwrap();
    assert_eq!(result(&r), r#"["timeout",2]"#);
}

#[test]
fn i03_parameters_results_and_restrictions() {
    need_pg!();
    let r = pg(r#"fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value d=db("main");x=sql(d,q`select ${null}::text as z, ${true}::boolean as b, ${"x'); drop table t; --"}::text as s,
        ${123456789012345678901234567890.123456789}::numeric as n, ${encode("é")}::bytea as raw, ${{a:1.50,b:[1e30]}}::jsonb as obj,
        ${[1,2]}::json as arr, 7::int2 as i2, 8::int4 as i4, 9007199254740993::int8 as i8, 'ab'::varchar as vc, 'ab'::char(4) as bp,
        '00000000-0000-4000-8000-000000000000'::uuid as u`).rows.one();
        [x.z,x.b,x.s,x.n,decode(kiroha_runtime_value(x).raw),x.obj,x.arr,x.i2,x.i4,x.i8,x.vc,x.bp,x.u]"#);
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
fn sql_blocks_use_postgresql_parameters_and_validate_rows() {
    need_pg!();
    let r = pg(r#"type Name={name:str}; d=db("main"); name="x'); drop table t; --";
        names=sql[Name](d){select ${name}::text as name};
        quoted=sql[Name](d){select $日本$}$日本$ as name};
        count=sql(d){select 1 as n union all select 2}.count;
        bad=try sql[Name](d){select 1 as name} catch {e=>e.code};
        [names.one().name,quoted.one().name,count,bad]"#);
    assert_eq!(r.code, 0, "{} {}", r.stdout, r.stderr);
    assert_eq!(result(&r), r#"["x'); drop table t; --","}",2,"validation"]"#);
}

#[test]
fn sql_block_validation_respects_transaction_boundaries() {
    need_pg!();
    let table = format!("kiroha_sql_block_{}", std::process::id());
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) { admin(&format!("drop table if exists {}", self.0)); }
    }
    admin(&format!("create table {table}(id integer primary key)"));
    let _cleanup = Cleanup(table.clone());
    let source = r#"type Row={name:str}; d=db("main");
        caught=tx(d){t=>try sql[Row](t){insert into TABLE_NAME values (1) returning id} catch {e=>e.code}};
        uncaught=try tx(d){t=>sql[Row](t){insert into TABLE_NAME values (2) returning id}} catch {e=>e.code};
        [caught,uncaught,sql(d){select count(*) as n from TABLE_NAME}.rows.one().n]"#.replace("TABLE_NAME", &table);
    let r = pg(&source);
    assert_eq!(r.code, 0, "{} {}", r.stdout, r.stderr);
    assert_eq!(result(&r), r#"["validation","validation",1]"#);
}

#[test]
fn i04_transactions_and_pool_hygiene() {
    need_pg!();
    admin("drop table if exists kiroha_t; create table kiroha_t(id serial primary key, v int unique)");
    // commit, then counts for DML without RETURNING
    let r = pg(r#"d=db("main");[tx(d){t=>sql(t,q`insert into kiroha_t(v) values (1),(2)`).count}, sql(d,q`update kiroha_t set v=v+10`).count]"#);
    assert_eq!(result(&r), "[2,2]");
    assert_eq!(admin("select count(*) from kiroha_t"), vec![vec![Some("2".into())]]);
    // callback failure rolls back
    let r = pg(r#"d=db("main");try tx(d){t=>sql(t,q`insert into kiroha_t(v) values (100)`);fail({code:"boom",message:"",data:{}})} catch {e=>e.code}"#);
    assert_eq!(result(&r), r#""boom""#);
    assert_eq!(admin("select count(*) from kiroha_t where v=100"), vec![vec![Some("0".into())]]);
    // caught poison stays primary and rolls back
    let r = pg(r#"d=db("main");try tx(d){t=>sql(t,q`insert into kiroha_t(v) values (200)`);
        try sql(t,q`insert into kiroha_t(v) values (11)`) catch {e=>null}; 7} catch {e=>[e.code,e.data.sqlstate]}"#);
    assert_eq!(result(&r), r#"["db","23505"]"#);
    assert_eq!(admin("select count(*) from kiroha_t where v=200"), vec![vec![Some("0".into())]]);
    // tx timeout aborts a pure loop and pending work, then rolls back
    let r = pg(r#"d=db("main");fun spin():any spin() end fun spin try tx(d,{timeout:200}){t=>sql(t,q`insert into kiroha_t(v) values (300)`);spin()} catch {e=>[e.code,e.data.operation]}"#);
    assert_eq!(result(&r), r#"["timeout","tx"]"#);
    assert_eq!(admin("select count(*) from kiroha_t where v=300"), vec![vec![Some("0".into())]]);
    // tx timeout during a running statement cancels it server-side, so the
    // rollback is not queued behind it (well under the 1000 ms cleanup grace)
    let t = std::time::Instant::now();
    let r = pg(r#"try tx(db("main"),{timeout:100}){t=>sql(t,q`select 1 as x from pg_sleep(3)`)} catch {e=>[e.code,e.data.operation]}"#);
    assert_eq!(result(&r), r#"["timeout","tx"]"#);
    assert!(t.elapsed().as_millis() < 900, "{:?}", t.elapsed());
    assert!(!r.stderr.contains("detached"), "{}", r.stderr);
    // max_rows stops buffering early (memory quota far below the full result)
    let d = dsn().unwrap();
    let host_small = r#"{"format":"kiroha-host/1","capabilities":{"databases":["main"]},"database_pools":{"main":{"dsn_env":"KIROHA_PG_MAIN"}},"limits":{"memory_bytes":30000000}}"#;
    let r = run_env(
        r#"try sql(db("main"),q`select generate_series(1,5000000) x, repeat(${"a"},100) y`,{max_rows:10}) catch {e=>[e.code,e.data.limit]}"#,
        Some(host_small),
        &[("KIROHA_PG_MAIN", &d)],
    );
    assert_eq!(result(&r), r#"["limit","max_rows"]"#);
    // values beyond the host number quota are resource aborts, not db errors
    let r = pg(r#"try sql(db("main"),q`select repeat('9',110000)::numeric as n`) catch {e=>e.code}"#);
    assert_eq!(r.code, 3, "{}", r.stdout);
    assert!(r.stdout.contains("\"resource\":\"number\""), "{}", r.stdout);
    // decoded row buffers count against memory_bytes
    let host_mem = r#"{"format":"kiroha-host/1","capabilities":{"databases":["main"]},"database_pools":{"main":{"dsn_env":"KIROHA_PG_MAIN"}},"limits":{"memory_bytes":100000000}}"#;
    let r = run_env(
        r#"sql(db("main"),q`select generate_series(1,1500000) x, repeat(${"a"},500) y`,{max_rows:5000000}).rows.size"#,
        Some(host_mem),
        &[("KIROHA_PG_MAIN", &dsn().unwrap())],
    );
    assert_eq!(r.code, 3, "{}", r.stdout);
    assert!(r.stdout.contains("\"resource\":\"memory\""), "{}", r.stdout);
    // result decoding is bounded by the sql timeout
    let host_big = r#"{"format":"kiroha-host/1","capabilities":{"databases":["main"]},"database_pools":{"main":{"dsn_env":"KIROHA_PG_MAIN"}},"limits":{"memory_bytes":4000000000}}"#;
    let t = std::time::Instant::now();
    let r = run_env(
        r#"try sql(db("main"),q`select COLS from (select repeat('7',99000)::numeric x) t`,{timeout:1000}) catch {e=>[e.code,e.data.operation]}"#.replace("COLS", "x as a0,x as a1,x as a2,x as a3,x as a4,x as a5,x as a6,x as a7,x as a8,x as a9,x as a10,x as a11,x as a12,x as a13,x as a14,x as a15,x as a16,x as a17,x as a18,x as a19,x as a20,x as a21,x as a22,x as a23,x as a24,x as a25,x as a26,x as a27,x as a28,x as a29,x as a30,x as a31,x as a32,x as a33,x as a34,x as a35,x as a36,x as a37,x as a38,x as a39,x as a40,x as a41,x as a42,x as a43,x as a44,x as a45,x as a46,x as a47,x as a48,x as a49,x as a50,x as a51,x as a52,x as a53,x as a54,x as a55,x as a56,x as a57,x as a58,x as a59,x as a60,x as a61,x as a62,x as a63,x as a64,x as a65,x as a66,x as a67,x as a68,x as a69,x as a70,x as a71,x as a72,x as a73,x as a74,x as a75,x as a76,x as a77,x as a78,x as a79,x as a80,x as a81,x as a82,x as a83,x as a84,x as a85,x as a86,x as a87,x as a88,x as a89,x as a90,x as a91,x as a92,x as a93,x as a94,x as a95,x as a96,x as a97,x as a98,x as a99,x as a100,x as a101,x as a102,x as a103,x as a104,x as a105,x as a106,x as a107,x as a108,x as a109,x as a110,x as a111,x as a112,x as a113,x as a114,x as a115,x as a116,x as a117,x as a118,x as a119,x as a120,x as a121,x as a122,x as a123,x as a124,x as a125,x as a126,x as a127,x as a128,x as a129,x as a130,x as a131,x as a132,x as a133,x as a134,x as a135,x as a136,x as a137,x as a138,x as a139,x as a140,x as a141,x as a142,x as a143,x as a144,x as a145,x as a146,x as a147,x as a148,x as a149,x as a150,x as a151,x as a152,x as a153,x as a154,x as a155,x as a156,x as a157,x as a158,x as a159,x as a160,x as a161,x as a162,x as a163,x as a164,x as a165,x as a166,x as a167,x as a168,x as a169,x as a170,x as a171,x as a172,x as a173,x as a174,x as a175,x as a176,x as a177,x as a178,x as a179,x as a180,x as a181,x as a182,x as a183,x as a184,x as a185,x as a186,x as a187,x as a188,x as a189,x as a190,x as a191,x as a192,x as a193,x as a194,x as a195,x as a196,x as a197,x as a198,x as a199,x as a200,x as a201,x as a202,x as a203,x as a204,x as a205,x as a206,x as a207,x as a208,x as a209,x as a210,x as a211,x as a212,x as a213,x as a214,x as a215,x as a216,x as a217,x as a218,x as a219,x as a220,x as a221,x as a222,x as a223,x as a224,x as a225,x as a226,x as a227,x as a228,x as a229,x as a230,x as a231,x as a232,x as a233,x as a234,x as a235,x as a236,x as a237,x as a238,x as a239,x as a240,x as a241,x as a242,x as a243,x as a244,x as a245,x as a246,x as a247,x as a248,x as a249,x as a250,x as a251,x as a252,x as a253,x as a254,x as a255,x as a256,x as a257,x as a258,x as a259,x as a260,x as a261,x as a262,x as a263,x as a264,x as a265,x as a266,x as a267,x as a268,x as a269,x as a270,x as a271,x as a272,x as a273,x as a274,x as a275,x as a276,x as a277,x as a278,x as a279,x as a280,x as a281,x as a282,x as a283,x as a284,x as a285,x as a286,x as a287,x as a288,x as a289,x as a290,x as a291,x as a292,x as a293,x as a294,x as a295,x as a296,x as a297,x as a298,x as a299,x as a300,x as a301,x as a302,x as a303,x as a304,x as a305,x as a306,x as a307,x as a308,x as a309,x as a310,x as a311,x as a312,x as a313,x as a314,x as a315,x as a316,x as a317,x as a318,x as a319,x as a320,x as a321,x as a322,x as a323,x as a324,x as a325,x as a326,x as a327,x as a328,x as a329,x as a330,x as a331,x as a332,x as a333,x as a334,x as a335,x as a336,x as a337,x as a338,x as a339,x as a340,x as a341,x as a342,x as a343,x as a344,x as a345,x as a346,x as a347,x as a348,x as a349,x as a350,x as a351,x as a352,x as a353,x as a354,x as a355,x as a356,x as a357,x as a358,x as a359,x as a360,x as a361,x as a362,x as a363,x as a364,x as a365,x as a366,x as a367,x as a368,x as a369,x as a370,x as a371,x as a372,x as a373,x as a374,x as a375,x as a376,x as a377,x as a378,x as a379,x as a380,x as a381,x as a382,x as a383,x as a384,x as a385,x as a386,x as a387,x as a388,x as a389,x as a390,x as a391,x as a392,x as a393,x as a394,x as a395,x as a396,x as a397,x as a398,x as a399").as_str(),
        Some(host_big),
        &[("KIROHA_PG_MAIN", &dsn().unwrap())],
    );
    assert_eq!(result(&r), r#"["timeout","sql"]"#);
    assert!(t.elapsed().as_millis() < 4000, "{:?}", t.elapsed());
    // sql timeout on a slow statement
    let r = pg(r#"try sql(db("main"),q`select pg_sleep(5)`,{timeout:200}) catch {e=>[e.code,e.data.outcome]}"#);
    assert_eq!(result(&r), r#"["timeout","unknown"]"#);
    // task confinement and nested tx
    let r = pg(r#"d=db("main");tx(d){t=>[[1].parallel(1){x=>try sql(t,q`select 1`) catch {e=>e.data.phase}},
        (try tx(d){u=>1} catch {e=>e.data.phase})]}"#);
    assert_eq!(result(&r), r#"[["ownership"],"ownership"]"#);
    // independent autocommit inside tx survives rollback
    let r = pg(r#"d=db("main");try tx(d){t=>sql(d,q`insert into kiroha_t(v) values (400)`);fail({code:"x",message:"",data:{}})} catch {e=>e.code}"#);
    assert_eq!(result(&r), r#""x""#);
    assert_eq!(admin("select count(*) from kiroha_t where v=400"), vec![vec![Some("1".into())]]);
    // holes in VALUES, RETURNING and LIMIT positions are parameters
    let r = pg(r#"d=db("main");x=sql(d,q`insert into kiroha_t(v) values (${500}) returning v + ${1} as w`);
        [x.rows.one().w, sql(d,q`select v from kiroha_t order by v limit ${2}`).rows.size]"#);
    assert_eq!(result(&r), "[501,2]");
    // SELECT INTO in a set operation's leftmost branch is rejected before dispatch
    admin("drop table if exists kiroha_into");
    let r = pg(r#"try sql(db("main"),q`select 1 as x into kiroha_into union select 2`) catch {e=>[e.data.phase,e.data.reason]}"#);
    assert_eq!(result(&r), r#"["prepare","statement"]"#);
    assert_eq!(admin("select to_regclass('kiroha_into') is null"), vec![vec![Some("t".into())]]);
    // multi-column SET with holes is valid
    let r = pg(r#"sql(db("main"),q`update kiroha_t set (v, id) = (${600}, id) where v = ${500}`).count"#);
    assert_eq!(result(&r), "1");
    // no dirty pooled reuse: session settings are reset before reuse
    let r = pg(r#"d=db("main");a=sql(d,q`select set_config('search_path','nowhere',false) as s`);
        b=sql(d,q`select current_setting('search_path') as s`);[b.rows.one().s]"#);
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
    admin("drop table if exists kiroha_lost; create table kiroha_lost(v int)");
    let proxy = commit_dropping_proxy(pg_port);
    let proxied = d.replace(&format!("port={pg_port}"), &format!("port={proxy}")) + " sslmode=disable";
    let src = r#"try tx(db("main")){t=>sql(t,q`insert into kiroha_lost(v) values (1)`);7} catch {e=>[e.code,e.data.phase,e.data.outcome]}"#;
    let r = run_env(src, Some(&host()), &[("KIROHA_PG_MAIN", &proxied)]);
    assert_eq!(result(&r), r#"["db","commit","unknown"]"#);
    // The callback was not replayed: at most one row exists (the server may
    // or may not have committed — that is exactly why the outcome is unknown).
    let rows = admin("select count(*) from kiroha_lost");
    let n: u32 = rows[0][0].as_deref().unwrap().parse().unwrap();
    assert!(n <= 1, "callback replayed: {n} rows");
}
