mod common;
use common::*;

#[test]
fn collection_members_and_bound_receivers() {
    for (source, expected) in [
        (r#"[[1,2].size,{a:1,b:2}.size,"a😀é".size,encode("é").size]"#, "[2,2,4,2]"),
        ("[1,2,3].map{x=>x*2}.filter{x=>x>2}.fold(0){(a,x)=>a+x}", "10"),
        ("[3,1,3].sort().unique().sum()", "4"),
        ("[[1],[2,3]].flat().take(1).one()", "1"),
        (r#"["a","b"].join(",").replace(",","-").split("-")"#, r#"["a","b"]"#),
        (r#"r={a:1}.merge({b:2});[r.keys(),r.has("b"),r.pick(["b"])]"#, r#"[["a","b"],true,{"b":2}]"#),
        ("[1,2].parallel(2){x=>x+1}", "[2,3]"),
        ("[1,2].each{x=>x+1}", "null"),
        ("m=[1,2].map;[m(){x=>x+1},m(){x=>x*2}]", "[[2,3],[2,4]]"),
        ("r={size:99,keys:42,map:{x:any=>x+1}};[r.size,r[\"size\"],r[\"keys\"],r.map(2)]", "[3,99,42,3]"),
        ("map=99;[1].map{x=>x+map}", "[100]"),
        ("[null?.size,{}?.missing,[1]?[5]]", "[null,null,null]"),
    ] {
        assert_eq!(result(&run(source,None)), expected, "{source}");
    }
}

#[test]
fn member_errors_and_callback_preflight() {
    for (source, expected) in [
        ("fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value (try kiroha_runtime_value((1)).size catch {e=>[e.code,e.data.position]})", r#"["type",0]"#),
        ("fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value (try kiroha_runtime_value([1].take)(\"x\") catch {e=>[e.code,e.data.position]})", r#"["type",1]"#),
        ("fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value try kiroha_runtime_value([].map{x=>x})(1) catch {e=>e.code}", r#""type""#),
        ("fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value try kiroha_runtime_value([].map)(){()=>fail({code:\"ran\",message:\"\",data:{}})} catch {e=>e.code}", r#""arity""#),
        ("fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value try kiroha_runtime_value([1].size)() catch {e=>e.code}", r#""type""#),
        ("fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value try kiroha_runtime_value(null?.map)(){x=>x} catch {e=>e.code}", r#""type""#),
        ("fun kiroha_runtime_value(value:any):any value end fun kiroha_runtime_value try kiroha_runtime_value({}).map catch {e=>e.code}", r#""type""#),
    ] { assert_eq!(result(&run(source,None)),expected,"{source}"); }
    for old in ["len([])","map([]){x=>x}","sort([])","keys({})"] {
        assert_eq!(error_code(&run(old,None)),"name","{old}");
    }
}

#[test]
fn compact_functions_delimit_themselves() {
    for (source, expected) in [
        ("fun f(x:any) x+1 end fun f f(2)","3"),
        ("fun f() 1 end fun f fun g() f()+1 end fun g g()","2"),
        ("do{fun f() 1 end fun f; f()}","1"),
        ("f={()=>fun g() 9 end fun g g()};f()","9"),
        ("fun f() fun g() 2 end fun g g() end fun f f()","2"),
        ("fun f() {} end fun f f()","{}"),
        ("fun f(x:any) {x:x} end fun f f(2)","{\"x\":2}"),
        ("fun f(x:any) do{{x}} end fun f f(2)","{\"x\":2}"),
        ("fun f() {x:any=>x+1} end fun f f()(2)","3"),
        ("fun f(x:any){body:any}:any body(x) end fun f f(2){x=>x+3}","5"),
        ("fun f() end fun f f()","null"),
        ("fun f(n:any):any if n==0 then 1 else n*f(n-1) end fun f f(5)","120"),
    ] { assert_eq!(result(&run(source,None)),expected,"{source}"); }
    for bad in ["fun f(x) then x end fun f", "if true 1 else 2", "x=1 x", "1 2", "fun f() 1 end fun f;;f()"] {
        assert_eq!(error_code(&run(bad,None)),"syntax","{bad}");
    }
}
