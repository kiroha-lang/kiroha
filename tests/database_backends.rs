//! Shared APIs exercised against real MySQL and MariaDB servers.
mod common;
use common::*;
use kiroha::database::Backend;
use kiroha::value::Value;
use mysql_async::prelude::Queryable;
use std::sync::atomic::{AtomicUsize, Ordering};

fn backends() -> Vec<(Backend, String)> {
    let mut found = Vec::new();
    for (backend, name) in [
        (Backend::MySql, "KIROHA_TEST_MYSQL_DSN"),
        (Backend::MariaDb, "KIROHA_TEST_MARIADB_DSN"),
    ] {
        if let Ok(dsn) = std::env::var(name) {
            found.push((backend, dsn));
        } else {
            eprintln!("SKIP: {name} not set");
        }
    }
    found
}
fn host(backend: Backend) -> String {
    kiroha::json::dump(&Value::rec(vec![
        ("format", Value::str("kiroha-host/1")),
        (
            "capabilities",
            Value::rec(vec![("databases", Value::array(vec![Value::str("main")]))]),
        ),
        (
            "database_pools",
            Value::rec(vec![(
                "main",
                Value::rec(vec![
                    ("backend", Value::str(backend.name())),
                    ("dsn_env", Value::str("KIROHA_BACKEND_TEST_DSN")),
                    ("max_connections", Value::int(1)),
                ]),
            )]),
        ),
    ]))
}
fn program(backend: Backend, dsn: &str, source: &str) -> Run {
    run_env(
        source,
        Some(host(backend).as_str()),
        &[("KIROHA_BACKEND_TEST_DSN", dsn)],
    )
}
fn admin(dsn: &str, statements: &[&str]) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let dsn = dsn
            .strip_prefix("mariadb://")
            .map(|s| format!("mysql://{s}"))
            .unwrap_or_else(|| dsn.into());
        let mut conn = mysql_async::Conn::new(mysql_async::Opts::from_url(&dsn).unwrap())
            .await
            .unwrap();
        for sql in statements {
            conn.query_drop(*sql).await.unwrap();
        }
        conn.disconnect().await.unwrap();
    });
}
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn table() -> String {
    format!(
        "kiroha_backend_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
fn expect(backend: Backend, dsn: &str, source: &str, want: &str) {
    let r = program(backend, dsn, source);
    assert_eq!(r.code, 0, "{}: {} {}", backend.name(), r.stdout, r.stderr);
    assert_eq!(result(&r), want, "{}: {source}", backend.name());
}
#[test]
fn backend_configuration_is_explicit_and_defaults_to_postgresql() {
    for (source, expected) in [
        (
            r#"{"format":"kiroha-host/1","database_pools":{"a":{"dsn_env":"DSN"}}}"#,
            Backend::PostgreSql,
        ),
        (
            r#"{"format":"kiroha-host/1","database_pools":{"a":{"backend":"mysql","dsn_env":"DSN"}}}"#,
            Backend::MySql,
        ),
        (
            r#"{"format":"kiroha-host/1","database_pools":{"a":{"backend":"mariadb","dsn_env":"DSN"}}}"#,
            Backend::MariaDb,
        ),
    ] {
        let v = kiroha::json::parse(source).unwrap();
        let c = kiroha::host::config::parse_host_file(&v, "/").unwrap();
        assert_eq!(c.database_pools[0].1.backend, expected);
    }
}
#[test]
fn mysql_and_mariadb_parameter_and_result_types() {
    for (backend, dsn) in backends() {
        expect(backend,&dsn,r#"type Row={z:null,b:int,s:str,n:num,raw:bytes,huge:int};
            r=check(sql(db("main"),q`select ${null} as z,cast(${true} as signed) as b,
                cast(${"a\u0000é'"} as char) as s,
                cast(${123456789012345678901234567890.12345678901234567890} as decimal(65,20)) as n,
                cast(${encode("é")} as binary) as raw,cast(${18446744073709551615} as unsigned) as huge`).rows.one(),Row);
            [r.z,r.b,r.s,r.n,decode(r.raw),r.huge]"#,
            "[null,1,\"a\\u0000é'\",1.234567890123456789012345678901234567890123456789e29,\"é\",18446744073709551615]");
        expect(
            backend,
            &dsn,
            r#"r=sql(db("main"),q`select 1 as n union all select 2`);[r.count,r.rows.size]"#,
            "[2,2]",
        );
    }
}

#[test]
fn mysql_and_mariadb_sql_blocks_bind_and_validate() {
    for (backend, dsn) in backends() {
        expect(backend, &dsn, r#"type Name={name:str}; d=db("main"); name="x'); drop table t; --";
            names=sql[Name](d){select cast(${name} as char) as name};
            count=sql(d){select 1 as n union all select 2}.count;
            bad=try sql[Name](d){select 1 as name} catch {e=>e.code};
            [names.one().name,count,bad]"#,
            r#"["x'); drop table t; --",2,"validation"]"#);
    }
}
#[test]
fn mysql_and_mariadb_commit_rollback_and_poison() {
    for (backend, dsn) in backends() {
        let table = table();
        admin(
            &dsn,
            &[&format!(
                "create table {table}(id integer primary key,v varchar(100)) engine=InnoDB"
            )],
        );
        let source=r#"d=db("main");tx(d){t=>sql(t,q`insert into TABLE_NAME(id,v) values (1,${"first"})`);sql(t,q`insert into TABLE_NAME(id,v) values (2,${"second"})`)};
            stopped=try tx(d){t=>sql(t,q`insert into TABLE_NAME(id,v) values (3,'rollback')`);fail({code:"stop",message:"",data:{}})} catch {e=>e.code};
            poison=try tx(d){t=>try sql(t,q`insert into TABLE_NAME(id,v) values (1,'duplicate')`) catch {e=>null};sql(t,q`insert into TABLE_NAME(id,v) values (4,'poisoned')`)} catch {e=>e.data.sqlstate};
            [stopped,poison,sql(d,q`select count(*) as n from TABLE_NAME`).rows.one().n,sql(d,q`update TABLE_NAME set v=v where id=1`).count,sql(d,q`update TABLE_NAME set v='changed' where id=1`).count]"#.replace("TABLE_NAME",&table);
        expect(backend, &dsn, &source, "[\"stop\",\"23000\",2,0,1]");
        admin(&dsn, &[&format!("drop table {table}")]);
    }
}
#[test]
fn mysql_and_mariadb_cancel_sql_and_transaction_then_reuse_pool() {
    for (backend, dsn) in backends() {
        let table = table();
        admin(
            &dsn,
            &[&format!(
                "create table {table}(id integer primary key) engine=InnoDB"
            )],
        );
        let source=r#"d=db("main");
            first=try sql(d,q`select sleep(5)`,{timeout:150}) catch {e=>e.code};
            second=try tx(d,{timeout:150}){t=>sql(t,q`insert into TABLE_NAME values (1)`);sql(t,q`select sleep(5)`)} catch {e=>e.code};
            before=sql(d,q`select count(*) as n from TABLE_NAME`).rows.one().n;
            sql(d,q`insert into TABLE_NAME values (1)`);
            [first,second,before,sql(d,q`select count(*) as n from TABLE_NAME`).rows.one().n]"#.replace("TABLE_NAME",&table);
        let began = std::time::Instant::now();
        expect(backend, &dsn, &source, "[\"timeout\",\"timeout\",0,1]");
        assert!(
            began.elapsed().as_secs() < 4,
            "server cancellation did not stop sleep"
        );
        admin(&dsn, &[&format!("drop table {table}")]);
    }
}
#[test]
fn mysql_and_mariadb_validate_sql_and_result_boundaries() {
    for (backend, dsn) in backends() {
        expect(backend,&dsn,r#"d=db("main");[
            (try sql(d,q`create table must_not_exist(x int)`) catch {e=>[e.data.phase,e.data.reason]}),
            (try sql(d,q`select ?`) catch {e=>e.data.reason}),
            (try sql(d,q`/*! select 1 */`) catch {e=>e.data.reason}),
            (try sql(d,q`select 1e0 as unsupported_float`) catch {e=>e.data.phase}),
            (try sql(d,q`select 1 as x,2 as x`) catch {e=>e.data.phase}),
            (try sql(d,q`select 1e0 as x where false`) catch {e=>e.data.phase}),
            (try sql(d,q`select if(1,null,1e0) as x`) catch {e=>e.data.phase}),
            (try sql(d,q`select 1 union all select 2`,{max_rows:1}) catch {e=>e.code}),
            sql(d,q`select 7 as n`).rows.one().n
        ]"#,"[[\"prepare\",\"statement\"],\"literal_parameter\",\"statement\",\"decode\",\"decode\",\"decode\",\"decode\",\"limit\",7]");
    }
}

#[test]
fn mysql_and_mariadb_json_and_session_reset() {
    for (backend, dsn) in backends() {
        let table = table();
        admin(&dsn,&[&format!("create table {table}(id integer primary key, doc JSON, flag BOOLEAN) engine=InnoDB")]);
        let read = if backend == Backend::MariaDb {
            "parse(check(r.doc,str))"
        } else {
            "r.doc"
        };
        let source = format!(
            r#"type Doc={{name:str,values:[num]}};
            d=db("main");sql(d,q`insert into {table} values (1,${{{{name:"é",values:[1,2,3]}}}},${{true}})`);
            r=sql(d,q`select doc,flag from {table}`).rows.one();
            doc=check({read},Doc);
            sql(d,q`select @kiroha_private := 73 as n`);
            [doc.name,doc.values.sum(),r.flag,sql(d,q`select @kiroha_private as n`).rows.one().n]"#
        );
        expect(backend, &dsn, &source, r#"["é",6,1,null]"#);
        admin(&dsn, &[&format!("drop table {table}")]);
    }
}

fn options(dsn: &str) -> mysql_async::Opts {
    mysql_async::Opts::from_url(&dsn.replacen("mariadb://", "mysql://", 1)).unwrap()
}
fn proxied_dsn(dsn: &str, port: u16) -> String {
    let old = options(dsn);
    // Test DSNs use a single TCP loopback endpoint. Preserve URL escaping,
    // credentials and driver settings, overriding only that endpoint.
    dsn.replace(
        &format!("{}:{}", old.ip_or_hostname(), old.tcp_port()),
        &format!("127.0.0.1:{port}"),
    )
}

#[test]
fn mysql_and_mariadb_cancelled_handshake_returns_single_pool_slot() {
    for (backend, dsn) in backends() {
        let target = options(&dsn).tcp_port();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let proxy = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let mut tasks = Vec::new();
                for index in 0..2 {
                    let (mut client, _) =
                        tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                            .await
                            .unwrap()
                            .unwrap();
                    tasks.push(tokio::spawn(async move {
                        if index == 0 {
                            tokio::io::copy(&mut client, &mut tokio::io::sink())
                                .await
                                .ok();
                        } else {
                            let mut server = tokio::net::TcpStream::connect(("127.0.0.1", target))
                                .await
                                .unwrap();
                            tokio::io::copy_bidirectional(&mut client, &mut server)
                                .await
                                .ok();
                        }
                    }));
                }
                for task in tasks {
                    tokio::time::timeout(std::time::Duration::from_secs(5), task)
                        .await
                        .unwrap()
                        .unwrap();
                }
            });
        });
        expect(
            backend,
            &proxied_dsn(&dsn, port),
            r#"d=db("main");[(try sql(d,q`select 1 as n`,{timeout:100}) catch {e=>e.code}),sql(d,q`select 2 as n`,{timeout:2000}).rows.one().n]"#,
            r#"["timeout",2]"#,
        );
        proxy.join().unwrap();
    }
}

#[test]
fn mysql_and_mariadb_lost_commit_ack_is_unknown_without_replay() {
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::sync::{atomic::AtomicBool, Arc};
    fn packet(s: &mut std::net::TcpStream) -> std::io::Result<Vec<u8>> {
        let mut head = [0u8; 4];
        s.read_exact(&mut head)?;
        let size = head[0] as usize + ((head[1] as usize) << 8) + ((head[2] as usize) << 16);
        assert!(size < 1024 * 1024, "unexpected test protocol packet size");
        let mut p = vec![0; 4 + size];
        p[..4].copy_from_slice(&head);
        s.read_exact(&mut p[4..])?;
        Ok(p)
    }
    for (backend, dsn) in backends() {
        let table = table();
        admin(
            &dsn,
            &[&format!(
                "create table {table}(id integer primary key) engine=InnoDB"
            )],
        );
        let target = options(&dsn).tcp_port();
        assert!(
            options(&dsn).ssl_opts().is_none(),
            "fault-injection proxy requires plaintext test DSN"
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let dropped = Arc::new(AtomicBool::new(false));
        let observed = dropped.clone();
        let proxy = std::thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut server = std::net::TcpStream::connect(("127.0.0.1", target)).unwrap();
            for stream in [&client, &server] {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
            }
            let mut client_out = client.try_clone().unwrap();
            let mut server_out = server.try_clone().unwrap();
            let commit = Arc::new(AtomicBool::new(false));
            let sent = commit.clone();
            let upstream = std::thread::spawn(move || {
                while let Ok(p) = packet(&mut client) {
                    if p.get(4) == Some(&3) && p.ends_with(b"COMMIT") {
                        sent.store(true, Ordering::SeqCst);
                    }
                    if server_out.write_all(&p).is_err() {
                        break;
                    }
                }
                server_out.shutdown(Shutdown::Both).ok();
            });
            while let Ok(p) = packet(&mut server) {
                if commit.load(Ordering::SeqCst) {
                    assert_eq!(
                        p.get(4),
                        Some(&0),
                        "server must confirm COMMIT before acknowledgment is dropped"
                    );
                    dropped.store(true, Ordering::SeqCst);
                    break;
                }
                if client_out.write_all(&p).is_err() {
                    break;
                }
            }
            client_out.shutdown(Shutdown::Both).ok();
            server.shutdown(Shutdown::Both).ok();
            upstream.join().unwrap();
        });
        let source = format!(
            r#"try tx(db("main")){{t=>sql(t,q`insert into {table} values (1)`);17}} catch {{e=>[e.code,e.data.phase,e.data.outcome]}}"#
        );
        expect(
            backend,
            &proxied_dsn(&dsn, port),
            &source,
            r#"["db","commit","unknown"]"#,
        );
        proxy.join().unwrap();
        assert!(observed.load(Ordering::SeqCst));
        expect(
            backend,
            &dsn,
            &format!(r#"sql(db("main"),q`select count(*) as n from {table}`).rows.one().n"#),
            "1",
        );
        admin(&dsn, &[&format!("drop table {table}")]);
    }
}

#[test]
fn different_backends_share_one_source_api_and_independent_pools() {
    let mut configs = Vec::new();
    let mut env = Vec::new();
    let mut dsns = backends();
    if let Ok(dsn) = std::env::var("KIROHA_TEST_PG_DSN") {
        dsns.push((Backend::PostgreSql, dsn));
    }
    for (backend, dsn) in &dsns {
        configs.push((
            backend.name(),
            Value::rec(vec![
                ("backend", Value::str(backend.name())),
                ("dsn_env", Value::str(backend.name())),
                ("max_connections", Value::int(1)),
            ]),
        ));
        env.push((backend.name(), dsn.as_str()));
    }
    let config = kiroha::json::dump(&Value::rec(vec![
        ("format", Value::str("kiroha-host/1")),
        (
            "capabilities",
            Value::rec(vec![(
                "databases",
                Value::array(dsns.iter().map(|(b, _)| Value::str(b.name())).collect()),
            )]),
        ),
        ("database_pools", Value::rec(configs)),
    ]));
    let names = kiroha::json::dump(&Value::array(
        dsns.iter().map(|(b, _)| Value::str(b.name())).collect(),
    ));
    let r = run_env(
        &format!(
            r#"{names}.parallel(3){{name:str=>tx(db(name)){{t=>sql(t,q`select ${{7}} as n`).rows.one().n}}}}"#
        ),
        Some(&config),
        &env,
    );
    assert_eq!(r.code, 0, "{} {}", r.stdout, r.stderr);
    assert_eq!(result(&r), format!("[{}]", vec!["7"; dsns.len()].join(",")));
}

#[test]
fn mysql_tls_verifies_certificate_and_hostname() {
    let (Ok(dsn), Ok(ca)) = (
        std::env::var("KIROHA_TEST_MYSQL_DSN"),
        std::env::var("KIROHA_TEST_MYSQL_CA"),
    ) else {
        eprintln!("SKIP: KIROHA_TEST_MYSQL_DSN and KIROHA_TEST_MYSQL_CA required for verified TLS");
        return;
    };
    let secure = format!(
        "{dsn}{}require_ssl=true",
        if dsn.contains('?') { '&' } else { '?' }
    );
    let source = r#"sql(db("main"),q`select 42 as n`).rows.one().n"#;
    let success = run_env(
        source,
        Some(&host(Backend::MySql)),
        &[("KIROHA_BACKEND_TEST_DSN", &secure), ("SSL_CERT_FILE", &ca)],
    );
    assert_eq!(success.code, 0, "{} {}", success.stdout, success.stderr);
    assert_eq!(result(&success), "42");
    // A trusted CA is insufficient when the IP identity differs from the SAN.
    let listener = std::net::TcpListener::bind("127.0.0.2:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let target = options(&dsn).tcp_port();
    let proxy = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (mut client, _) =
                tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
            let mut server = tokio::net::TcpStream::connect(("127.0.0.1", target))
                .await
                .unwrap();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::io::copy_bidirectional(&mut client, &mut server),
            )
            .await
            .ok();
        });
    });
    let mismatch = proxied_dsn(&secure, port).replace("127.0.0.1:", "127.0.0.2:");
    let rejected = run_env(
        r#"try sql(db("main"),q`select 1`) catch {e=>[e.code,e.data.phase,e.data.outcome]}"#,
        Some(&host(Backend::MySql)),
        &[
            ("KIROHA_BACKEND_TEST_DSN", &mismatch),
            ("SSL_CERT_FILE", &ca),
        ],
    );
    proxy.join().unwrap();
    assert_eq!(result(&rejected), r#"["db","pool","not_sent"]"#);
    // Same server, without its private CA: verification must reject the chain.
    let untrusted = run_env(
        r#"try sql(db("main"),q`select 1`) catch {e=>[e.code,e.data.phase,e.data.outcome]}"#,
        Some(&host(Backend::MySql)),
        &[
            ("KIROHA_BACKEND_TEST_DSN", &secure),
            ("SSL_CERT_FILE", "/kiroha-test-missing-ca.pem"),
            ("SSL_CERT_DIR", "/kiroha-test-missing-ca-dir"),
        ],
    );
    assert_eq!(result(&untrusted), r#"["db","pool","not_sent"]"#);
}
