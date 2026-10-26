//! SQL blocks share query safety and execution with the original SQL API.
use kiroha::conformance::{parse_manifest, run_all};

#[test]
fn typed_and_untyped_blocks_execute_and_validate() {
    let cases = parse_manifest(include_str!("fixtures/sql_blocks.json")).unwrap();
    let (report, all) = run_all(&cases);
    assert!(all, "{}", kiroha::json::dump(&report));
}

#[test]
fn syntax_and_type_errors_are_reported_before_execution() {
    for src in [
        "type Name={name:str}; sql[Name](db(\"main\")){select ${} as name}",
        "sql[Missing](db(\"main\")){select 1}",
        "type F=(num)=>num; sql[F](db(\"main\")){select 1}",
        "type Name={name:str}; sql[Name](db(\"main\")){select 1}.one().missing",
        "type Name={name:str}; xs:[num]=sql[Name](db(\"main\")){select 1}; xs",
        "sql(db(\"main\")){select 1",
        "sql(){select 1}",
    ] {
        assert!(kiroha::compile(src).is_err(), "accepted {src}");
    }
    for src in [
        "type Name={name:str}; sql[Name](db(\"main\")){select ${\"}\"} as name}.one().name",
        "type Box<T>={value:T}; sql[Box<str>](db(\"main\")){select 1}.one().value",
        "sql(db(\"main\")){select ${sql(db(\"main\")){select 1}.count} as name}",
        "sql(db(\"main\")){select '}' as name /* } */}",
        "sql(db(\"main\")){select $tag$}$tag$ as name}",
        "sql(db(\"main\")){select $日本$}$日本$ as name}",
        "sql(db(\"main\")){select foo$tag$ as name}",
        "sql(db(\"main\")){select 1 -- }\n}",
        "sql(db(\"main\")){select '{\"a\":\"}\"}' as name}",
        "sql(db(\"main\")){select 1 /* outer /* } */ inner */}",
        "sql(db(\"main\")){select '{}'::jsonb #> '{a}' as name}",
        "fun sql(d:num){f:(num)=>num}:num f(d) end fun sql sql(1){x=>x+1}",
        "fun sql(d:num){f:(num)=>num}:num f(d) end fun sql sql(1){x\n=>x+1}",
        "fun sql(d:num){f:({x:num})=>num}:num f({x:d}) end fun sql sql(1){r:{x:num}=>r.x}",
        "sql=[{n:1}]; sql[0].n",
    ] {
        assert!(kiroha::compile(src).is_ok(), "rejected {src}: {:?}", kiroha::compile(src).err());
    }
}

#[test]
fn block_delimiters_preserve_sql_and_escape_literal_interpolation() {
    use kiroha::lexer::Tok;
    let source = r#"sql(db("main")){select '} \${literal}' as name, ${1} as n /* } */}"#;
    let tokens = kiroha::lexer::tokenize(source).unwrap();
    let block = tokens.iter().find_map(|t| match &t.tok { Tok::SqlBlock(q) => Some(q), _ => None }).unwrap();
    assert_eq!(block.holes.len(), 1);
    assert_eq!(block.segments, ["select '} ${literal}' as name, ", " as n /* } */"]);
}
