mod common;
use common::*;
use kiroha::types::{Ty, Type};
use std::collections::HashSet;
use std::rc::Rc;

fn rejected(source: &str, code: &str) {
    let error = kiroha::compile(source).expect_err(source);
    assert_eq!(
        (error.phase, error.code),
        ("check", code),
        "{source}: {error:?}"
    );
}
fn inspect(t: &Ty, seen: &mut HashSet<usize>, allow_any: bool) {
    if !seen.insert(Rc::as_ptr(t) as usize) {
        return;
    }
    match &**t {
        Type::Any => assert!(allow_any, "an implicit any escaped the checker"),
        Type::Hole(_) => panic!("unresolved local inference variable"),
        Type::Array(e) | Type::Schema(e) | Type::OptionalSchema(e) => inspect(e, seen, allow_any),
        Type::Record { fields, rest } => {
            for f in fields {
                inspect(&f.ty, seen, allow_any);
            }
            if let Some(r) = rest {
                inspect(r, seen, allow_any);
            }
        }
        Type::Function(f) => {
            for p in &f.params {
                inspect(p, seen, allow_any);
            }
            if let Some(c) = &f.callback {
                inspect(c, seen, allow_any);
            }
            inspect(&f.result, seen, allow_any);
        }
        Type::Union(xs) => {
            for x in xs {
                inspect(x, seen, allow_any);
            }
        }
        _ => (),
    }
}
#[test]
fn declarations_and_structural_compatibility() {
    for (source, expected) in [
        ("type Point={x:num,y:num};p:Point={x:1,y:2};p.x+p.y", "3"),
        ("type A={x:num};type B={x:num};fun f(p:A):num p.x end fun f p:B={x:2};f(p)", "2"),
        ("type Point={x:num,y:num};fun f(p:Point):num p.x+p.y end fun f f({x:1,y:2,label:\"ok\"})", "3"),
        ("type Point={x:num,y:num};check({x:1,y:2},Point)", "{\"x\":1,\"y\":2}"),
        ("type Point={x:num};try check({x:1,y:2},Point) catch {e=>e.code}", "\"validation\""),
        ("type Point={x:num};check({x:1,y:2},open(Point))", "{\"x\":1,\"y\":2}"),
        ("type P={name:str,nick:optional(maybe(str))};p:P={name:\"Ada\"};p?.nick??p.name", "\"Ada\""),
        ("type P={x:num};fun make(p:P):(num)=>num {n=>p.x+n} end fun make make({x:2})(3)", "5"),
        ("type=1", "parse"),
    ] {
        if expected == "parse" { assert_eq!(error_code(&run(source,None)),"syntax"); }
        else { assert_eq!(result(&run(source,None)),expected,"{source}"); }
    }
    rejected("type Point={x:num};p:Point={x:\"wrong\"};p", "type");
    rejected(
        "type Point={x:num};fun f(p:Point):num p.missing end fun f",
        "type",
    );
    rejected("Point={x:num};p:Point={x:1};p", "type_name");
    rejected("type P={x:Missing};0", "type_name");
    rejected("type P={x:P};0", "type_name");
    rejected("type P=num;type P=str;0", "duplicate_binding");
}
#[test]
fn generic_aliases_and_function_calls() {
    for (source, expected) in [
        ("type Box<T>={value:T};x:Box<num>={value:3};x.value", "3"),
        ("type Box<T>={value:T};check({value:3},Box<num>)", "{\"value\":3}"),
        ("fun identity<T>(value:T):T value end fun identity [identity(3),identity<str>(\"x\")]", "[3,\"x\"]"),
        ("type Box<T>={value:T};fun identity<T>(v:T):T v end fun identity identity<Box<num>>({value:3}).value", "3"),
        ("fun first<T>(xs:[T]):T xs.one() end fun first first([4])", "4"),
        ("fun transform<T,U>(x:T){step:(T)=>U}:U step(x) end fun transform transform(2){x=>text(x)}", "\"2\""),
        ("fun identity<T>(v:T):T v end fun identity f=identity<num>;f(2)", "2"),
        ("type Point={x:num};fun validate<T>(v:json,s:schema<T>):T check(v,s) end fun validate validate({x:1},Point).x", "1"),
        ("type Pair<A,B>={left:A,right:B};p:Pair<str,[int]>={left:\"x\",right:[2]};p.right.one()", "2"),
        ("type Option<T>=maybe(T);x:Option<int>=null;if x==null then 0 else x+1", "0"),
        ("[0.1,0.2].fold<num>(0){(acc,n)=>acc+n}", "0.3"),
    ] { assert_eq!(result(&run(source,None)),expected,"{source}"); }
    rejected("type Box<T>={value:T};x:Box={value:1};x", "type_arity");
    rejected("type Box<T>={value:T};Box", "type_arity");
    rejected("type Box<T,T>={value:T};0", "duplicate_type_parameter");
    rejected("fun id<T>(v:T):T v end fun id id<str>(1)", "type");
    rejected("fun id<T>(v:T):T v end fun id id<num,str>(1)", "type_arity");
    rejected("fun bad<T>(v:T):num v+1 end fun bad", "type");
    rejected("fun bad<T>(v:T):T check(v,T) end fun bad", "name");
}
#[test]
fn lightweight_inference_requires_annotations_instead_of_any() {
    for source in [
        "x=1;y=x+2;y",
        "f:(num)=>num={x=>x+1};f(2)",
        "f={x:int=>x+1};f(2)",
        "[1,2].map{x=>x+1}.fold(0){(a,x)=>a+x}",
        "fun greet(name:str) \"Hi \"+name end fun greet greet(\"Ada\")",
        "fun fac(n:int):int if n<=1 then 1 else n*fac(n-1) end fun fac fac(5)",
        "type P={x:num};type Box<T>={value:T};b:Box<P>={value:{x:2}};b.value.x",
    ] {
        let program = kiroha::compile(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
        let mut seen = HashSet::new();
        for t in program.inferred_types.iter().flatten() {
            inspect(t, &mut seen, false);
        }
    }
    rejected("fun f(x) x end fun f", "type_annotation_required");
    rejected("f={x=>x};f(1)", "type_annotation_required");
    rejected(
        "fun f(x:int){step} step(x) end fun f",
        "type_annotation_required",
    );
    rejected(
        "fun loop(n:int) loop(n-1) end fun loop",
        "type_annotation_required",
    );
    rejected(
        "fun empty<T>():[T] [] end fun empty empty()",
        "type_annotation_required",
    );
    rejected("serve({\"GET /\":{r=>encode(\"x\")}})", "type");
    rejected("get(\"https://fixture.invalid\").x", "type");
    rejected(
        "type P={x:num};fun id<T>(v:T):T v end fun id p:P=id(parse(\"{}\"));p",
        "type",
    );
}
#[test]
fn explicit_any_keeps_dynamic_checks_available() {
    for (source, expected) in [
        ("x:any=1;try x+\"s\" catch {e=>e.code}", "\"type\""),
        (
            "f:any={x:num=>x+1};try f(\"wrong\") catch {e=>e.code}",
            "\"type\"",
        ),
        ("type Dynamic=any;x:Dynamic={f:{()=>3}};x.f()", "3"),
        (
            "fun legacy(x:any) x.missing() end fun legacy try legacy(1) catch {e=>e.code}",
            "\"type\"",
        ),
    ] {
        let program = kiroha::compile(source).unwrap();
        let mut seen = HashSet::new();
        for t in program.inferred_types.iter().flatten() {
            inspect(t, &mut seen, true);
        }
        assert_eq!(result(&run(source, None)), expected, "{source}");
    }
    rejected("x:any=1+\"wrong\";x", "type");
}

#[test]
fn schema_bridge_scope_and_callback_variance() {
    for (source, expected) in [
        ("type Nothing=never;try check(1,Nothing) catch {e=>e.code}", "\"validation\""),
        ("type Dynamic=any;check({x:1},Dynamic).x", "1"),
        ("type Empty=null;check(null,Empty)", "null"),
        ("type P={x:num};fun f(p:P):num fun read():num p.x end fun read read() end fun f do {type P=str;[f({x:2}),check(\"s\",P)]}", "[2,\"s\"]"),
        ("type Mapper=(num)=>num;fun f(x:num){step:Mapper}:num step(x) end fun f f(2){x=>x+1}", "3"),
        ("f:(int)=>num={x:num=>x+1};f(2)", "3"),
        ("type Request={method:str,path:str,params:{},query:{},headers:{},body:bytes};f:(Request)=>json=input;f({method:\"POST\",path:\"/\",params:{},query:{},headers:{\"content-type\":[\"application/json\"]},body:encode(\"1\")})", "1"),
    ] { assert_eq!(result(&run(source,None)),expected,"{source}"); }
    rejected("type P={x:num};f:({method:str,path:str,params:{},query:{},headers:{},body:bytes})=>P=input;0", "type");
    rejected(
        "input<int>({method:\"POST\",path:\"/\",params:{},query:{},headers:{},body:encode(\"1\")})",
        "arity",
    );
    rejected("f:(num)=>num={x:int=>x};0", "type");
    rejected(
        "f=if false then {a:1}.merge else {b:2}.merge;f({}).a",
        "type",
    );
    rejected("[1].map{()=>1}", "arity");
    rejected(
        "type Function=(num)=>num;check(1,Function)",
        "type_reification",
    );
    rejected("do {type Hidden=num;0};x:Hidden=1", "type_name");
    rejected("type Box<T>={value:T};x:T=1", "type_name");
}

#[test]
fn shared_generic_aliases_remain_compact() {
    let mut source = "type Pair<T>={left:T,right:T};type A0=int;".to_string();
    for level in 1..=30 {
        source.push_str(&format!("type A{level}=Pair<A{}>;", level - 1));
    }
    source.push_str("fun id<T>(x:T):T x end fun id fun up(x:A30):json x end fun up A30");
    let p = kiroha::compile(&source).unwrap();
    let t = p.aliases.last().unwrap().ty.as_ref().unwrap();
    let mut seen = HashSet::new();
    inspect(t, &mut seen, false);
    assert!(
        seen.len() < 100,
        "generic expansion duplicated a shared alias DAG: {}",
        seen.len()
    );
    assert!(
        kiroha::types::display(t).len() < 5000,
        "diagnostic should abbreviate large types"
    );
}

#[test]
fn public_native_signatures_match_runtime_arities() {
    use kiroha::builtins::Builtin;
    use kiroha::value::CallbackPolicy;
    let mut sources = kiroha::builtins::GLOBALS
        .iter()
        .map(|b| (b.name().to_string(), *b, false))
        .collect::<Vec<_>>();
    for m in kiroha::methods::ALL.iter().filter(|m| !m.property) {
        let recv = match m.builtin {
            Builtin::Flat => "[[1]]",
            Builtin::Join => "[\"a\"]",
            Builtin::Split | Builtin::Replace => "\"a\"",
            _ if m.receivers.contains(&kiroha::value::Kind::Record) => "{}",
            _ => "[1]",
        };
        sources.push((format!("{recv}.{}", m.name), m.builtin, true));
    }
    for (source, b, member) in sources {
        let p = kiroha::compile(&source).unwrap();
        let Type::Function(f) = &**p.inferred_types[*p.items.last().unwrap() as usize]
            .as_ref()
            .unwrap()
        else {
            panic!("{source}")
        };
        let native = b.signature();
        let offset = usize::from(member);
        assert_eq!(
            (f.min, f.params.len()),
            (native.min - offset, native.max - offset),
            "{source}"
        );
        assert_eq!(
            f.callback.is_some(),
            native.callback != CallbackPolicy::Absent,
            "{source}"
        );
        assert_eq!(
            f.optional_callback,
            native.callback == CallbackPolicy::Optional,
            "{source}"
        );
    }
}
