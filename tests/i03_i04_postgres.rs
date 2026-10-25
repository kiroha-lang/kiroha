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
    // no dirty pooled reuse: session settings are reset before reuse
    let r = pg(r#"d=db("main");a=sql(d,q`select set_config('search_path','nowhere',false) as s`);
        b=sql(d,q`select current_setting('search_path') as s`);[one(b.rows).s]"#);
    assert_eq!(result(&r), r#"["\"$user\", public"]"#);
}
