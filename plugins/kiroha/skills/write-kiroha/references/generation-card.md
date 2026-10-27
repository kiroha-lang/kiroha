Kiroha 0.7.0 / Web1
Write only this language/API. No imports, host escape, invented methods or implicit variables. Task URLs, credentials, DB schemas and requirements are separate input.

CORE
Case-sensitive ASCII identifiers; JSON null/true/false, JSON-number syntax, double-quoted JSON strings, arrays [e,...], records {key:e,...}. Identifier keys need no quotes; {x}={x:x}. Duplicate decoded keys error. // line comments. Newlines are whitespace; bindings and expressions need ; between items; a function definition ends itself. No trailing commas, spread, assignment or coercion.
Immutable values: JSON, functions, Bytes, schemas, Query, DB/transaction handles, Reply. Records retain insertion order. Equality accepts only recursively JSON values, ignoring record-key order; opaque/function comparison errors. Strings use Unicode scalars.
Numbers: exact finite decimals; parse,+,-,*,comparison exact; / rounds to 34 significant digits ties-even; % uses exact rational quotient truncated toward zero. Zero divisor errors. int=integer-valued number. + also joins two strings or arrays. No NaN/Infinity.
Sequences: top level, do {...}, named-function and lambda bodies. x=e binds; fun f(x:num,y:num):num sequence end fun f defines a self-recursive function. Opening/closing names must match exactly; both fun and end are reserved. No then in a function header and no required ; after its named end. if still requires then. Old f(x)=e syntax is invalid. Last expression returns; empty/end-in-definition returns null; trailing ; preserves value. Duplicate same-scope names error; inner scopes shadow. All names resolve statically including dead branches. Definitions see earlier bindings/self, not later names. Closures capture lexical symbols. Proper tail calls use bounded continuation space.
Lambdas: {x=>sequence}, {(x,y)=>sequence}, {()=>sequence}; no bare arrow, .field lambda, or {(x)=>...}. Braces with lambda header are lambdas, otherwise records; use do for other sequences.
fun f(x:num,y:num){body:(num)=>num}:num sequence end fun f declares positional parameters and one separate REQUIRED callback slot. Calls: f(a,b) or f(a,b){x=>sequence}. Ordinary calls require parentheses, including (). A dot member with zero ordinary arguments may take its trailing callback directly: xs.map{x=>x*2}. Other methods use xs.sum(), xs.take(3). x.size is a property, not a call. One trailing lambda supplies the slot. f(a,{x=>x}), f(a){existing}, missing/extra slots invalid. Reuse g with f(a){x=>g(x)}. Grouped lambda also forbidden as entire ordinary argument. Functions in variables/records/arrays are ordinary data. Aliases retain signatures. User arity exact; no user defaults. A full {IDENT} after a definition parameter list declares its callback slot; a first-body shorthand record must use {x:x} or do{{x}}.
Evaluate callee, ordinary args left-to-right, create trailing closure, check arity, invoke. Builtins check callbacks before effects/empty shortcuts. Bodies run only when invoked. Defaults only when omitted, never null; unknown options error.
e | f(a,b){x=>body} evaluates e once first, inserts it as FIRST ordinary argument; callback stays separate. Stage callee identifier/dotted path, always (). Pipes left-associate; no implicit map/unwrap/flatten.
Control: if bool then e else e; do {sequence}; try e catch {err=>sequence}. if/try start an expression; group as operands. Else consumes full expression. Group whole control results before pipe/access: (try...catch {...}) | f(). Catch ordinary runtime errors only.
Dot members bind registered methods by receiver kind; a matching method/property wins over record data (use r["size"] or r["keys"] for collisions). A bound method keeps its receiver, evaluates it once and retains its signature. Other dot names read record fields. Strict r.k/r["k"]/a[i]: missing/type/bounds errors. Indices nonnegative integers. r?.k/v?[k] return null for null receiver/absent key/out-of-range valid index; wrong types/negative index error. Null receiver skips dynamic key evaluation. No implicit projection/string indexing.
Precedence: call/access; unary ! -; * / %; + -; comparison; &&; ||; ??; |. Max one unparenthesized comparison (< <= > >= == !=). Binary left-associative except ?? right. &&/|| short-circuit bool only. ?? replaces only null. Ordering two numbers or two strings, scalar lexicographic.
q`SQL ${expr} SQL` creates Query; holes evaluate left-to-right. Query text allows newlines, escapes \\, \` and \${. SQL holes bind values only, outside quotes/comments. Ordinary strings never interpolate.

TYPES
Declare type Point={x:num,y:num}; type Box<T>={value:T}; bind p:Point={x:1,y:2}. Named params and callback slots require types; recursive functions require result :T. Nonrecursive result/bindings infer locally. Lambda parameters infer from a known expected function type or need annotations, e.g. {x:num=>x+1}. No whole-program inference, auto-generalization or implicit any; ambiguous=>annotation required. any must be explicit and retains runtime checks. json is unknown JSON, not any: validate before projection.
Types: primitives str/num/int/bool/null/json, opaque bytes/query/database/transaction/reply, [T], {k:T}, maybe(T), optional(T) fields, open(record), (T,U)=>R, (T){(U)=>V}=>R, schema<T>, any/never. Aliases structural, record extras allowed statically; arrays covariant. fun id<T>(x:T):T x end fun id; id(1) or id<num>(1). Simple rank-one generics only, erased; T is not a runtime value. Pass schema<T> for generic validation. Type aliases lexical, no forward/recursive aliases. Concrete aliases (Box<num>) are schemas.

SCHEMAS
Atoms str,num,int,bool,json. Prefer named type declarations; schema<T> describes a validator. [S]=homogeneous array of any length; {k:S}=closed required-field record. maybe(S) adds null. optional(S) permits field absence, not null; only immediate field schema. open(recordSchema) allows/preserves extra JSON fields. No literal scalar schemas/unions.
Static width compatibility allows extras; runtime check of a closed record rejects them. v.as(S) fully validates S then v, returns unchanged v; v.as[T] validates against type T; no defaults/coercion/stripping. Malformed schema=>schema; mismatch=>validation data={path,expected,actual}, no payload. optional(maybe(str)) accepts absent/null/string; maybe(optional(str)) invalid.

DATA (callbacks sequential unless parallel)
x.size: array/record/string-scalars/Bytes length.
range(start,stop,step=1): integers, exclusive stop, nonzero step.
xs.map{f}, xs.filter{f}, xs.each{f}: f(item); filter bool; each=>null.
xs.fold(initial){f}: f(acc,item), left fold.
xs.flat(): concatenate array members one level; non-array errors.
xs.take(n): first n; nonnegative integer.
xs.sort{f}: stable ascending numeric OR string keys, once/item.
xs.group{f}: [{key,items}], first-seen JSON-equal keys.
xs.unique{f}: first item per JSON-equal key.
ONLY sort/unique allow omitted callback, identity.
xs.sum(): exact numeric sum, []=>0.
r.keys(), r.has(k), r.pick(keys), a.merge(b), xs.one().
pick requires present unique string keys, preserves requested order.
merge shallow, b wins, old positions stay; one requires length1.
s.split(sep), strings.join(sep), s.replace(old,new): literal; split/replace search nonempty; retain split empties.
text(x): string or canonical number/bool/null spelling.
number(s): complete signed JSON-number string, no whitespace.
parse(s): strict one JSON value, exact decimals; duplicate keys/surrogates/BOM invalid.
dump(v): compact JSON, ordered keys, exact numbers; non-JSON errors.
encode(s): UTF8 Bytes; decode(b): strict UTF8, preserves valid BOM scalar.
Validation uses `.as[T]` or `.as(schema)`; maybe, optional, open as above.

RUNTIME
xs.parallel(n){f}: positive n, at most n active f(item), input-order results. First observed failure stops starts, cancels/joins siblings, rethrows. Effects remain. Catch inside f for per-item failures.
sleep(ms): nonnegative integer, cancellable, null.
retry({attempts,when,delay}){f}: attempts>=1 counts first; f takes0 args; when is function data e.g. {e=>e.code=="timeout"}, returns bool; delay>=0 ms. Failed attempt i with attempts remaining: if when true wait delay*2^(i-1), retry. No predicate after final attempt. No hidden jitter/idempotency.
pages(seed,o={}){step}: JSON seed, step(cursor)=>exact {items:[...],next:JSON}; first call even null seed. Only next=null ends; concatenate items. o={max_pages:100,max_items:10000}; page>=1,item>=0. Repeated non-null cursor/excess=>pagination; terminal exactly-at-limit succeeds; no partial success.
fail({code,message,data}): raise exact error; code/message strings, data JSON record, no extra fields.
I/O suspends current task. No automatic retries. Effects obey capabilities/deadlines/cancellation. Integer ms durations. Operation timeout/explicit byte-row limits catchable; ancestor/request/host cancellation and host resource termination uncatchable even in pure loops. Cleanup bounded; catch never undoes effects.
Codes: type,arity,argument,missing,bounds,number,json,utf8,schema,validation,cardinality,network,tls,timeout,limit,http,pagination,db,input,media,env,io,permission. Error={code,message,data}; messages diagnostic.

HTTP
http(method,u,o={}): uppercase method except CONNECT; strict absolute ASCII http/https URL without credentials/fragment. Returns {status,headers,body:Bytes} for final200..599 including failures. Headers lowercase keys,string arrays. TLS verified; no redirects,retries,cookies,ambient auth.
Options: headers:{string:string},query:{string:scalar-or-array},body:Bytes,timeout:10000,max_bytes:8388608. GET/HEAD reject body. Header case duplicates/controls/framing headers error. Query string/number/bool emitted,null omitted,arrays repeat,UTF8 percent encoded. Timeout pool-through-body; max_bytes decoded. identity/gzip supported; HEAD/204/205/304 bodyless.
get(u,o={}): GET, require2xx else http data.status, strict UTF8+JSON. Empty/204/205=>json error. No body option.
post(u,v,o={}): JSON POST then same status/parse contract. No body option; application/json; no JSON compression.
url(base,segments,query={}): append encoded string segments, preserve origin/query; "."/".." segments forbidden. Segment slash encoded; query spaces %20.
Network/timeout outcome not_sent only when proven, otherwise unknown. Failed request does not imply rollback.

DATABASE
db(name): configured borrowed database pool,reused,no close. Host selects postgresql(default)/mysql/mariadb.
sql(d,Q,o={}): one parameterized SELECT/INSERT/UPDATE/DELETE (PG16 also MERGE, WITH allowed); backend SQL syntax, no SELECT INTO/DDL/session/transaction control. Query only; no literal $n(PG)/?(MySQL/MariaDB) parameters. o={timeout:10000,max_rows:10000}. Returns {rows:[record],count:int}, count server command count. Outside tx autocommit.
Unreleased: sql[T](d,o?){SQL ${expr}} validates all rows as T and returns [T]; sql(d,o?){SQL ${expr}} returns {rows,count}. Holes bind values as in q literals; validation happens after SQL execution.
Bind null/bool/string/exact-number/Bytes/JSON records-arrays, not SQL arrays. PG: numeric/jsonb/bytea/bool map naturally. MySQL/MariaDB: large/fractional numbers bind exact text; CAST(... AS DECIMAL(p,s)) for numeric semantics. BOOL=>int, binary=>Bytes, MySQL JSON=>json, MariaDB JSON=>str then parse/check. Results integer/decimal exact, text str, NULL null. Float/date/time/unsupported types need SQL casts. Duplicate columns/mapping/limit errors; autocommit may have committed.
tx(d,o={timeout:10000}){f}: f(t), pinned read-committed transaction. Normal unpoisoned result commits; otherwise rollback attempt. Every sql(t) failure after live owned t accepted poisons with first failure even if caught. t task-confined/expires on callback return; parallel children cannot use it even n1. No nested tx same task. Only explicit sql(t) joins; sql(d),HTTP/files independent.
tx timeout aborts callback and is catchable only outside tx. Ancestor abort wins, then tx timeout, then first SQL error, then other callback error. Lost COMMIT acknowledgment=>db outcome unknown; no replay.

SERVER
serve(routes,o={}): {"METHOD /path/:param":{r=>...}}, runs until cancelled.
o={host:"127.0.0.1",port:8080,timeout:30000,max_body:1048576,max_inflight:128}.
Split before one path decode. Same-method match with most literal segments wins; overlapping equal-specificity routes/repeated param names error. No implicit HEAD/OPTIONS/CORS; wrong method405, absent path404.
r={method,path,params,query,headers,body}; raw encoded path,params strings,query/headers string arrays,lowercase headers,Bytes body. Query +=>space,path + literal. Request compression unsupported.
input(r,S=json): normalize schema,validate request shape,require application/json optional charset=utf-8,strict parse/check. Bad payload input400,media415; malformed schema/internal check500.
Handler JSON=>200; reply(status,v,headers={}) nominal Reply. Status200..599; JSON=>application/json,Bytes raw defaultapplication/octet-stream.204/205/304 require null. Plain {status:404}=>JSON200. Explicit HEAD validates then suppresses body.
Other uncaught errors generic500+log; deadline504,bodyexcess413,inflight503. Disconnect/host cancellation stops/joins handlers; failure after headers closes transport.

LOCAL
env(name): explicitly exposed string; absent=>env.
read(path): Bytes; write(path,Bytes): atomic replacement/null,preserve0777,new0600; no parent creation/symlinks/.. components. Host path/byte permissions.
now(): UTC YYYY-MM-DDTHH:mm:ss.sssZ; uuid(): randomUUIDv4; log(v): dump(v)+LF stderr/null. Explicit secrets are not redacted.
These are the 30 global functions, 19 methods and size property. Use task-provided bindings/schemas; do not infer absent APIs.
