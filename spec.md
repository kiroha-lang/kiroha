# Kiroha 0.5.0 — Web1 Language and Runtime Specification

**Revision:** 0.5.0 · **Profile:** Web1 · **Date:** 2026-10-26
**Deliverables:** this self-contained implementation specification and `measurement.json`.

Kiroha is a small, strict, immutable expression language for LLM-generated programs that manipulate JSON, call web services, serve APIs and use PostgreSQL. Its single canonical callback syntax is a mandatory trailing lambda, with a matching declaration-side callback slot:

```kiroha
fun apply(x){f} f(x) end fun apply
apply(10){v=>v+1};

urls=["https://a.example/data","https://b.example/data"];
urls.parallel(4){u=>get(u)}
```

The declaration and call both have an ordinary argument channel and a separate callback channel. A positional-callback alternative is not part of the language.

## Contents

- [0. Authority, scope and implementation handoff](#0-authority-scope-and-implementation-handoff)
- [1. Compact generation card](#1-compact-generation-card)
- [2. Source text, tokens, and grammar](#2-source-text-tokens-and-grammar)
- [3. Values, decimal arithmetic, and serialization](#3-values-decimal-arithmetic-and-serialization)
- [4. Static resolution and lexical scope](#4-static-resolution-and-lexical-scope)
- [5. Evaluation and calls](#5-evaluation-and-calls)
- [6. Schemas](#6-schemas)
- [7. Standard data library](#7-standard-data-library)
- [8. Failures, diagnostics, budgets, and task scopes](#8-failures-diagnostics-budgets-and-task-scopes)
- [9. Concurrency, retries, pagination](#9-concurrency-retries-pagination)
- [10. HTTP client and URL contract](#10-http-client-and-url-contract)
- [11. PostgreSQL](#11-postgresql)
- [12. HTTP server](#12-http-server)
- [13. Environment, files, clock, randomness and logs](#13-environment-files-clock-randomness-and-logs)
- [14. Host configuration and adapter](#14-host-configuration-and-adapter)
- [15. Command-line and implementation completion contract](#15-command-line-and-implementation-completion-contract)
- [16. Embedded conformance protocol](#16-embedded-conformance-protocol)
- [17. Measurements and reproducibility](#17-measurements-and-reproducibility)
- [18. External standards and dependency boundaries](#18-external-standards-and-dependency-boundaries)

## 0. Authority, scope and implementation handoff

### 0.1 What this document defines

Sections 2–16 are the normative implementation contract. MUST/MUST NOT express conformance requirements. SHOULD is used only for explicitly optional engineering advice. Examples and test expectations must agree with the normative rules; an implementation may not silently choose a different behavior to match its host language.

This file replaces the Kiroha 0.4.0 specification as the implementation authority for 0.5.0. It defines lexical/syntactic rules, static resolution, values, evaluation, 31 global functions, 19 collection methods and the size property, external effects, host configuration, a CLI and an embedded conformance corpus. No prior chat, hidden design document, skill, existing implementation or unstated API is needed to determine Kiroha semantics.

This revision has a Rust reference implementation in this repository. Embedded cases remain normative acceptance requirements; recorded execution results are in `docs/conformance-report.md`. Token measurements report document/source length only. Finite review and tests do not prove that every future implementation will be bug-free.

### 0.2 Two distinct sizes

The full Markdown is the implementation handoff. Section 1 is a compact **generation card** for an agent writing Kiroha against an already-conforming runtime. The card deliberately omits parser/adapter/test engineering detail; it does not replace the full implementation contract. Both sizes are measured separately in measurement.json.

The generation surface is small because common work has explicit runtime/library support. This specification does not claim that its large implementation contract itself fits a tiny system prompt. The target for the extracted generation card is at most 4096 tokens in each of the two named measurement encodings; measured counts, not a promise about an arbitrary model tokenizer, determine whether that target is met.

### 0.3 Implementation choices already settled

| Decision | Contract |
| --- | --- |
| Callbacks | `f(args){x=>...}` only; definition `fun f(params){slot} expr end fun f` |
| Data | Ordered immutable records, heterogeneous arrays, exact decimal numbers |
| Evaluation | Strict left-to-right; sequential effects unless parallel is explicit |
| Binding | Static lexical symbols; no hoisting; named self recursion; proper tail calls |
| Missing/null | Distinct; optional access and ?? have narrowly specified behavior |
| Error model | Catchable error records; separate uncatchable abort channel |
| Networking | Buffered HTTP/JSON; explicit limits; no hidden redirects/retries/cookies |
| Database | Parameterized PostgreSQL; explicit transaction handles and poison state |
| Portability | Host-independent values; explicit I/O and configuration boundaries |
| Tests | Embedded machine-readable cases plus required real integration scenarios |

The 0.3.0 baseline fixed details not settled by the prior short contract: grammar disambiguation, comparison nonchaining, scalar-based strings, canonical decimal/JSON spelling, static error timing, callback preflight, complete error data, timeout/cleanup scopes, URL/header/framing policies, SQL parsing/typing, filesystem replacement, CLI/wire formats and conformance fixtures. It also treats HTTP 205 as bodyless. The 0.4.0 revision introduced explicit `fun` and matching named ends. Version 0.5.0 removes function-header `then`, makes a separator after a definition optional, and moves collection operations to receiver methods with `.size` for length. Conditional `then` remains mandatory. The native operation inventory remains 51 entries; its public surface is 31 globals, 19 methods and one property. Token counts below measure length, not generation success.

### 0.4 Agent build instruction

Read the entire document before choosing implementation architecture. Use Section 15's dependency order and Section 16's acceptance protocol. Reuse mature HTTP/TLS/PostgreSQL/decimal primitives only where they satisfy the defined contract; add adapters where their defaults differ. Do not translate Kiroha into unchecked host-language eval. Do not replace exact numbers with host floats, callbacks with positional syntax, or cancellation with catchable ordinary errors.

The implementation language, parser algorithm, VM/AST/backend design, native libraries and package layout are engineering choices. Language-visible behavior is already fixed. External protocol/dialect references at the end define dependency boundaries, not missing Kiroha syntax.

## 1. Compact generation card

The exact text between the text fence below is extracted by `kiroha spec --compact` and measured independently. The marker lines and fence delimiters are excluded from the card's token count.

<!-- KIROHA_GENERATION_CARD_BEGIN -->
```text
Kiroha 0.5.0 / Web1
Write only this language/API. No imports, host escape, invented methods or implicit variables. Task URLs, credentials, DB schemas and requirements are separate input.

CORE
Case-sensitive ASCII identifiers; JSON null/true/false, JSON-number syntax, double-quoted JSON strings, arrays [e,...], records {key:e,...}. Identifier keys need no quotes; {x}={x:x}. Duplicate decoded keys error. // line comments. Newlines are whitespace; bindings and expressions need ; between items; a function definition ends itself. No trailing commas, spread, assignment or coercion.
Immutable values: JSON, functions, Bytes, schemas, Query, DB/transaction handles, Reply. Records retain insertion order. Equality accepts only recursively JSON values, ignoring record-key order; opaque/function comparison errors. Strings use Unicode scalars.
Numbers: exact finite decimals; parse,+,-,*,comparison exact; / rounds to 34 significant digits ties-even; % uses exact rational quotient truncated toward zero. Zero divisor errors. int=integer-valued number. + also joins two strings or arrays. No NaN/Infinity.
Sequences: top level, do {...}, named-function and lambda bodies. x=e binds; fun f(x,y) sequence end fun f defines a self-recursive function. Opening/closing names must match exactly; both fun and end are reserved. No then in a function header and no required ; after its named end. if still requires then. Old f(x)=e syntax is invalid. Last expression returns; empty/end-in-definition returns null; trailing ; preserves value. Duplicate same-scope names error; inner scopes shadow. All names resolve statically including dead branches. Definitions see earlier bindings/self, not later names. Closures capture lexical symbols. Proper tail calls use bounded continuation space.
Lambdas: {x=>sequence}, {(x,y)=>sequence}, {()=>sequence}; no bare arrow, .field lambda, or {(x)=>...}. Braces with lambda header are lambdas, otherwise records; use do for other sequences.
fun f(x,y){body} sequence end fun f declares positional parameters and one separate REQUIRED callback slot. Calls: f(a,b) or f(a,b){x=>sequence}. Ordinary calls require parentheses, including (). A dot member with zero ordinary arguments may take its trailing callback directly: xs.map{x=>x*2}. Other methods use xs.sum(), xs.take(3). x.size is a property, not a call. One trailing lambda supplies the slot. f(a,{x=>x}), f(a){existing}, missing/extra slots invalid. Reuse g with f(a){x=>g(x)}. Grouped lambda also forbidden as entire ordinary argument. Functions in variables/records/arrays are ordinary data. Aliases retain signatures. User arity exact; no user defaults. A full {IDENT} after a definition parameter list declares its callback slot; a first-body shorthand record must use {x:x} or do{{x}}.
Evaluate callee, ordinary args left-to-right, create trailing closure, check arity, invoke. Builtins check callbacks before effects/empty shortcuts. Bodies run only when invoked. Defaults only when omitted, never null; unknown options error.
e | f(a,b){x=>body} evaluates e once first, inserts it as FIRST ordinary argument; callback stays separate. Stage callee identifier/dotted path, always (). Pipes left-associate; no implicit map/unwrap/flatten.
Control: if bool then e else e; do {sequence}; try e catch {err=>sequence}. if/try start an expression; group as operands. Else consumes full expression. Group whole control results before pipe/access: (try...catch {...}) | f(). Catch ordinary runtime errors only.
Dot members bind registered methods by receiver kind; a matching method/property wins over record data (use r["size"] or r["keys"] for collisions). A bound method keeps its receiver, evaluates it once and retains its signature. Other dot names read record fields. Strict r.k/r["k"]/a[i]: missing/type/bounds errors. Indices nonnegative integers. r?.k/v?[k] return null for null receiver/absent key/out-of-range valid index; wrong types/negative index error. Null receiver skips dynamic key evaluation. No implicit projection/string indexing.
Precedence: call/access; unary ! -; * / %; + -; comparison; &&; ||; ??; |. Max one unparenthesized comparison (< <= > >= == !=). Binary left-associative except ?? right. &&/|| short-circuit bool only. ?? replaces only null. Ordering two numbers or two strings, scalar lexicographic.
q`SQL ${expr} SQL` creates Query; holes evaluate left-to-right. Query text allows newlines, escapes \\, \` and \${. SQL holes bind values only, outside quotes/comments. Ordinary strings never interpolate.

SCHEMAS
Atoms str,num,int,bool,json. [S]=homogeneous array of any length; {k:S}=closed required-field record. maybe(S) adds null. optional(S) permits field absence, not null; only immediate field schema. open(recordSchema) allows/preserves extra JSON fields. No literal scalar schemas/unions.
check(v,S) fully validates S then v, returns unchanged v; no defaults/coercion/stripping. Malformed schema=>schema; mismatch=>validation data={path,expected,actual}, no payload. optional(maybe(str)) accepts absent/null/string; maybe(optional(str)) invalid.

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
check,maybe,optional,open as above.

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
db(name): configured borrowed PostgreSQL pool,reused,no close.
sql(d,Q,o={}): one parameterized PostgreSQL16 SELECT/INSERT/UPDATE/DELETE/MERGE (WITH allowed); no SELECT INTO/DDL/session/transaction control. Query only; no literal $n parameters. o={timeout:10000,max_rows:10000}. Returns {rows:[record],count:int}, count server command count. Outside tx autocommit.
Bind null/bool/string/exact-number/Bytes; JSON records/arrays=>JSONB, not SQL arrays. Results: integer/numeric exact; text/varchar/char/uuid string; bool,bytea,JSON/JSONB,NULL map naturally. Other OIDs (arrays,date/time,float,etc.) require explicit SQL conversion. Duplicate columns/mapping/limit errors; autocommit may already have committed.
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
These are the 31 global functions, 19 methods and size property. Use task-provided bindings/schemas; do not infer absent APIs.
```
<!-- KIROHA_GENERATION_CARD_END -->

## 2. Source text, tokens, and grammar

### 2.1 Source representation

A source file is strict UTF-8 without a byte-order mark. A string is a sequence of Unicode scalar values: U+0000..U+D7FF and U+E000..U+10FFFF. Ill-formed UTF-8 and unpaired surrogate escapes are errors; replacement characters MUST NOT be substituted. No Unicode normalization is performed. Source offsets are zero-based UTF-8 byte offsets; line and column numbers are one-based, with columns counting scalar values. CRLF is one line ending; lone CR and LF also end a line.

Outside strings and query text, whitespace is exactly space, horizontal tab, CR, and LF. `//` begins a comment through the next CR/LF or end of file. There are no block comments or automatic semicolon insertion. Newlines do not terminate expressions. A leading shebang is not accepted.

An identifier matches `[A-Za-z_][A-Za-z0-9_]*` and is case-sensitive. Reserved words are exactly `null true false if then else do try catch fun end`. An identifier token excluding these words is called `IDENT`. `WORD` means an identifier-shaped token including reserved words. WORD is permitted as an unquoted record key or after `.`/`?.`. A shorthand field must be an IDENT, because it refers to a binding.

Prelude names, including `map`, `str` and `json`, are ordinary bindings and can be shadowed. `q` alone is an ordinary identifier; the adjacent character sequence `q` followed by a backtick starts a query literal regardless of any binding named q. Whitespace between q and the backtick is invalid.

Use maximal munch for identifiers and the operators `=> == != <= >= && || ?? ?. ?[`. Other punctuation is `( ) [ ] { } , ; : . + - * / % ! < > = |` and the query delimiters. There are no other operators. `? [` is not `?[`. Comments are recognized before a single division token.

Source number tokens are unsigned JSON numbers:

```ebnf
NUMBER = ("0" | NONZERO, { DIGIT }),
         [ ".", DIGIT, { DIGIT } ],
         [ ("e" | "E"), [ "+" | "-" ], DIGIT, { DIGIT } ];
DIGIT = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9";
NONZERO = "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9";
```

A source minus is a unary/binary operator, not part of a NUMBER token. A number immediately followed by an identifier character, an extra decimal point, or a digit after a leading zero is a lexical error: `01`, `1abc`, `1.2.3`, `.5` and `1.` are invalid. `1-2` and `1e-2` are valid. Field access on a numeric expression requires grouping, for example `(1).x`; it then fails by type at runtime.

Strings use double quotes and JSON escapes: `\" \\ \/ \b \f \n \r \t \uHHHH`. Raw U+0000..U+001F, raw line endings, unknown escapes, and unpaired surrogates are invalid. A high-surrogate escape must be followed immediately by a low-surrogate escape and is decoded to one scalar. There is no string interpolation. Strings may contain scalar U+0000 via an escape; some external APIs reject it explicitly.

### 2.2 Grammar

The following EBNF is normative. `{ X }` means repetition, `[ X ]` means optional, and quoted text is a terminal. The notation is unrelated to source record/lambda braces. Parentheses in the grammar group grammar terms. A source string token is `STRING`. A `QUERY` is lexed as specified in Section 2.4.

```ebnf
Program     = Sequence, EOF;
Sequence    = { FunctionDef, [ ";" ] | SimpleItem, ";" }, [ SimpleItem ];
Item        = FunctionDef | SimpleItem;
SimpleItem  = Binding | Expr;
Binding     = IDENT, "=", Expr;
FunctionDef = "fun", IDENT, "(", [ Params ], ")",
              [ "{", IDENT, "}" ], Sequence, "end", "fun", IDENT;
Params      = IDENT, { ",", IDENT };

Expr        = IfExpr | TryExpr | PipeExpr;
IfExpr      = "if", Expr, "then", Expr, "else", Expr;
TryExpr     = "try", Expr, "catch", Handler;
Handler     = "{", IDENT, "=>", Sequence, "}";

PipeExpr    = Coalesce, { "|", Stage };
Stage       = Path, Arguments, [ Lambda ];
Path        = IDENT, { ".", WORD };
Coalesce    = OrExpr, [ "??", Coalesce ];
OrExpr      = AndExpr, { "||", AndExpr };
AndExpr     = Equality, { "&&", Equality };
Equality    = Ordering, [ ("==" | "!="), Ordering ];
Ordering    = Additive, [ ("<" | "<=" | ">" | ">="), Additive ];
Additive    = Product, { ("+" | "-"), Product };
Product     = Unary, { ("*" | "/" | "%"), Unary };
Unary       = ("!" | "-"), Unary | Postfix;
Postfix     = Primary, { Suffix };
Suffix      = Arguments, [ Lambda ]
            | ("." | "?."), WORD, [ Lambda ]
            | "[", Expr, "]" | "?[", Expr, "]";
Arguments   = "(", [ Expr, { ",", Expr } ], ")";

Primary     = "null" | "true" | "false" | NUMBER | STRING | IDENT
            | Array | Record | Lambda | QUERY
            | "(", Expr, ")" | "do", "{", Sequence, "}";
Array       = "[", [ Expr, { ",", Expr } ], "]";
Record      = "{", [ Field, { ",", Field } ], "}";
Field       = (WORD | STRING), ":", Expr | IDENT;
Lambda      = "{", LambdaHead, "=>", Sequence, "}";
LambdaHead  = IDENT | "(", ")" | "(", IDENT, ",", Params, ")";
```

Additional mandatory parsing constraints:

1. No trailing commas are permitted. Empty arrays, records, argument lists, parameter lists, and sequences are valid. Leading or repeated sequence semicolons are invalid.
2. At the start of an Item, `fun` begins a function definition. There is no header `then`; its body is a Sequence ending at `end fun IDENT`. A complete `{ IDENT }` immediately after the parameter list is a callback-slot declaration; other brace forms begin the body. To return a shorthand record as the first body expression, write `{x:x}` or `do{{x}}` instead of `{x}`. The closing IDENT MUST exactly match the opening function name, including case. All three closing tokens are required. Named functions may nest; each closing name belongs to the innermost open definition. Empty bodies and a trailing body semicolon are permitted. A definition may be followed by the next item without a semicolon; one optional semicolon is accepted. Bindings and expressions still require a semicolon before another item. The old `f(x)=e` spelling is invalid. Recognize a binding only for IDENT followed by `=`. Otherwise parse an expression. No expression may contain a binding or definition except through a sequence.
3. At an expression-position `{`, a syntactically valid LambdaHead followed by `=>` selects Lambda; otherwise parse Record. There is no general brace block. `do {…}` is the expression form for a sequence outside a lambda.
4. The only zero/one/multiple-parameter lambda heads are `()=>`, `x=>`, and `(x,y,...)=>`. In particular `{(x)=>x}` is invalid.
5. For each ordinary call argument, strip all grouping nodes; if the resulting root node is Lambda, reject with a parse diagnostic. This includes pipe-stage arguments. The rule does not recursively reject lambdas stored in records/arrays, or expressions that compute function values.
6. A trailing callback must be a Lambda immediately attached to an Arguments suffix or directly after a dot/optional-dot member name (zero ordinary arguments). There is exactly one slot per call. `f(){x=>x}{y=>y}` and `f(){existing}` are invalid. `f(){x=>x}(3)` is a call on the first call's result and is syntactically valid.
7. Within one unparenthesized comparison expression, at most one operator from `< <= > >= == !=` is allowed. Consequently `a<b<c`, `a==b==c` and `a<b==true` are invalid; `(a<b)==true` is valid. This additional restriction closes the cross-level case allowed by the EBNF.
8. Record literal keys are decoded strings. A repeated decoded key is a parse error, including `{a:1,"a":2}` and escaped equivalents.
9. `if` and `try` occur at the beginning of Expr, not as unparenthesized arithmetic/postfix operands. Write `1+(if c then 2 else 3)`. The else expression consumes a full Expr. Write `(if c then a else b) | f()` to pipe the complete conditional.
10. A try expression ends after its handler. To pipe/access/call its result, group it: `(try a catch {e=>b}) | f()`. The protected expression may itself contain pipes. Catch is a one-parameter handler syntax, not a general callable expression.
11. Static names, duplicate parameters, and same-scope definitions are checked after parsing, before any program effect. Arity is a runtime check, including where a compiler could infer it.
12. Whitespace may occur between tokens, including between `)` and an attached Lambda. Only the adjacency of q/backtick and multi-character tokens is significant.

These constraints are part of syntax, not optional lint rules. There is no automatic currying, user extension declaration, destructuring, spread, assignment, import, user operator, macro, async/await construct, return statement, or implicit variable. A name such as `await` can still be an ordinary user-defined IDENT.

### 2.3 AST contract

Implementations may use other representations, but MUST preserve these distinctions:

```text
Program(items)
Bind(name, expr)
Define(name, positionalNames, callbackNameOrNone, bodySequence)
Literal(value)
Array(elements)
Record(ordered decoded-key/expression pairs)
Name(symbol)
Lambda(positionalNames, bodySequence)             // no callback slot
Call(callee, ordinaryArguments, callbackOrNone)
Pipe(left, calleePath, ordinaryArguments, callbackOrNone)
Member(receiver, name, optional)                 // type-dispatched dot selection
Get(receiver, keyExpression, optional)            // bracket lookup
Unary(operator, expression)
Binary(operator, left, right)
If(condition, thenExpression, elseExpression)
Do(sequence)
Try(protectedExpression, handlerName, handlerSequence)
Query(literalSegments, holeExpressions)
Group(expression)                               // may be erased after restrictions
```

Both `Define` and `Call` have a distinct callback field. A compiler may lower this representation internally after validating the source contract. It MUST NOT accept a positional-callback spelling or print one as canonical Kiroha.

### 2.4 Query lexing

`q` followed by an opening backtick begins literal SQL text. An unescaped backtick ends it. Unescaped `${` starts a Kiroha Expr, ending at the matching top-level `}`. Query holes use the full Kiroha lexer/parser, including strings, records, comments, nested queries, and balanced delimiters. An empty hole is invalid. SQL quotes do not suppress Kiroha interpolation.

Inside literal query text, the only escapes are `\\` for a backslash, `\` followed by a backtick for a literal backtick, and `\${` for the literal two-character sequence `${`. Every other backslash escape is a lexical error. Newlines are preserved as written. SQL comments are literal SQL text, not Kiroha comments.

For example:

```kiroha
q`select ${user.id}::numeric, ${user.name}::text`
q`select ${do { x=2; x+1 }}`
```

Evaluation produces an opaque Query containing exactly `holes+1` decoded literal segments and a sequence of hole values. Holes evaluate once, in source order. Construction does not contact a database, serialize values, or validate SQL. Section 11 defines when a hole is legal as a SQL parameter.

## 3. Values, decimal arithmetic, and serialization

### 3.1 Value domain

The runtime has the following value kinds, with these exact names in diagnostics:

| Kind | Meaning |
| --- | --- |
| `null` | The sole null value |
| `bool` | true or false |
| `number` | A finite arbitrary-precision decimal |
| `string` | An immutable scalar-value sequence |
| `array` | An immutable, ordered finite sequence of values |
| `record` | An immutable ordered mapping from distinct strings to values |
| `function` | A builtin, named function, or closure, with a call signature |
| `bytes` | An immutable finite sequence of octets |
| `schema` | A nominal schema atom or wrapper |
| `query` | An opaque parameterized SQL query |
| `database` | A host-owned pool handle |
| `transaction` | A task-bound, expiring transaction handle |
| `reply` | A nominal HTTP response wrapper |

There is no undefined, missing value, symbol, date, promise, iterator, mutable reference, prototype, or object identity comparison. An absent record key is not a stored value. Keys such as `"__proto__"` and `"constructor"` are ordinary keys with no host-language behavior.

Arrays and records may contain any value kinds. A **JSON value** recursively contains only null/bool/number/string/array/record. Structural sharing is allowed; externally observable mutation and cyclic containers are not. Closures may recursively reference themselves, but cannot become JSON by serialization.

Records retain insertion order. Replacing an existing key with `merge` retains its original position; adding a new key appends it. Runtime values have no public identity or mutation API.

### 3.2 Decimal representation and operations

A nonzero decimal has the mathematical form `c × 10^e`, with signed integer c, integer exponent e, and c not divisible by 10. Zero has c=0,e=0 and no sign. This normalized pair is a semantic model, not a mandated memory layout. Literal parsing and `number`/`parse` do not round.

The operations `+ - *`, unary minus, comparison, and summation are mathematically exact. Implementers MUST NOT pass these values through a binary floating-point number, including when reading JSON or a database driver result. Exponent and coefficient limits may arise only through the explicit host resource termination rules, never silent numeric approximation.

For `a/b`:

1. If b=0, raise `number` with `data.reason="division_by_zero"`.
2. Compute the exact rational quotient conceptually. If it is zero, return 0.
3. Round to 34 significant decimal digits, nearest, ties to even. If the quotient is exactly representable with at most 34 significant digits, preserve it exactly.
4. Normalize the decimal result. Exponents are not clamped to an IEEE decimal128 exponent range.

An implementation can compute rounding with integer division. For positive rational N/D, determine the integer k such that `10^k <= N/D < 10^(k+1)`. Scale N/D by `10^(33-k)`, compute integer quotient q and remainder r, increment q when `2r>D` or `2r==D and q is odd`, restore the power of ten and sign, then normalize. When scaling is negative, multiply D instead of N. This also handles a carry to the next decade.

`a%b` is defined by `a-trunc(a/b)*b` where trunc is applied to the **exact rational** a/b, toward zero, without the 34-digit division rounding. Zero divisor has the same number error. The nonzero remainder has the sign of a.

`int` means a number with no fractional mathematical part. Integers are not a separate runtime kind. Positive/nonnegative integer constraints always check mathematical value before converting to a host index. Host integer overflow is not a permitted wraparound.

### 3.3 Canonical decimal spelling

`text(number)`, `dump`, diagnostic numeric serialization, and conformance expectations use one canonical representation.

For nonzero normalized c,e, let digits be the decimal digits of |c|, n their count, and a=e+n-1.

- If `-6 <= a < 21`, use plain decimal notation, placing the decimal point at position n+e; pad leading/trailing zeroes as necessary.
- Otherwise use scientific notation: first digit, optionally `.` plus remaining digits, then lowercase `e` and a in signed decimal with no plus sign and no leading zeroes.
- Emit a leading minus only for negative values. Never emit a trailing fractional zero, an unnecessary decimal point, or negative zero.
- Zero is exactly `0`.

Examples: `1.2300=>1.23`; `1e20=>100000000000000000000`; `1e21=>1e21`; `1e-6=>0.000001`; `1e-7=>1e-7`; `-0.0=>0`. This choice is a Kiroha formatting rule, not a promise that other JSON tools use the same spelling.

### 3.4 Strings, Bytes, and JSON

String length, sorting, and comparison operate on Unicode scalar values, not bytes, UTF-16 code units, grapheme clusters, locale, or normalized text. `"😀".size` is 1. Bytes has length and encode/decode/I/O operations; it does not support array indexing, arithmetic, or equality.

A strict JSON parse accepts exactly one JSON value with surrounding space/tab/CR/LF. It rejects trailing material, comments, duplicate decoded keys, BOM, nonfinite numbers, unpaired surrogates and malformed numbers. Parsing preserves object key order and exact decimals.

Canonical JSON serialization emits no insignificant whitespace. Object keys are emitted in insertion order. Strings use `\"` and `\\`, the short escapes for backspace/form-feed/newline/CR/tab, and lowercase `\u00hh` for other U+0000..U+001F characters. All other scalar values, including slash, U+2028/U+2029 and non-ASCII characters, are emitted literally in UTF-8. There is no ASCII-only mode.

`dump` recursively rejects any non-JSON kind with a type error before returning a partial string. `parse(dump(v))==v` for every JSON v within host quotas. Serialization does not preserve the original numeric spelling. RFC 8259 supplies the interchange baseline; Kiroha deliberately tightens duplicate-key and scalar-value handling.[J]

### 3.5 Equality and ordering

`==` and `!=` first verify that **both entire operands are JSON values**, even if an early tag/key mismatch could decide the result. A function or Bytes nested anywhere therefore raises type. For valid JSON operands:

- Distinct kinds are unequal, except all numbers share one kind.
- Numbers compare by exact mathematical value; strings by identical scalar sequences.
- Arrays compare recursively by length and index.
- Records compare recursively by key/value sets, ignoring insertion order.
- null equals null; booleans compare by value.

There is no implicit conversion: `1=="1"` is false and `1==true` is false. Two empty records with different construction histories are equal.

Ordering accepts two numbers or two strings of the same kind. String order is lexicographic by scalar code point, with a proper prefix ordered first. Other pairs raise type. Grouping and uniqueness use the same JSON equality, including record-order insensitivity.

## 4. Static resolution and lexical scope

Before evaluation, tokenize, parse, and resolve **the whole program**, including unused function bodies and unreachable branches. Run phases in that order; do not proceed to a later phase after failure. Any error prevents all program effects. Implementations may collect several diagnostics within a phase; the CLI's primary diagnostic is from the first failing phase and has the earliest source-start offset among its detected diagnostics. Error recovery beyond the primary failure is optional.

Each top-level sequence, do sequence, lambda body, named function body, and catch-handler body establishes a lexical scope. Function parameters and a callback-slot name occupy the function body's scope. The function body is a sequence in that parameter scope; local bindings cannot redeclare parameters or the callback slot. An additional explicit do creates a child scope.

Resolution processes sequence items in order:

1. Resolve a binding initializer in the current environment, then introduce its symbol for subsequent items. `x=x` sees an earlier/outer x, never the x being created; without an outer x it is a name error.
2. For a function definition, introduce its function symbol and resolve its body with a fresh parameter scope that also sees that symbol. This is the only implicit recursive binding.
3. Resolve an expression against the symbols currently visible.
4. A closure captures these resolved symbols, not a mutable map whose later additions could change lookup.

No name may be declared twice in one scope, including parameters and callback slots. Inner scopes may shadow outer names, including a function's recursive name. The prelude is an outer scope, so top-level shadowing is permitted. Record keys do not declare bindings. Named definitions cannot refer to a later sibling definition. Mutual recursion needs explicit ordinary function data/combinators; there is no hidden hoisting mechanism.

Examples:

```kiroha
x=1; fun f() x end fun f; do { x=2; f() }       // 1
x=7; do { y=x; x=9; y }          // 7
fun fac(n) if n<=1 then 1 else n*fac(n-1) end fun fac; fac(5)
fun apply(x){f} f(x) end fun apply; apply(10){v=>v+1}
```

`fun f() g() end fun f fun g() 1 end fun g f()` is a check-phase name error even though g would be bound before the call. `if false then unknown else 1` is also a name error. A named function's self-reference is implemented with an immutable recursive cell or equivalent; no general user-visible mutable cell is introduced.

## 5. Evaluation and calls

### 5.1 Evaluation order and sequence values

Evaluation is strict, left-to-right, and sequential within a task. Array elements and record field values evaluate in textual order. A field shorthand reads the corresponding lexical binding. A later failure does not undo prior effects.

A sequence evaluates items in order. Its value is the last expression's value if the final item is an expression. An empty sequence, or a sequence whose final item is a binding/function definition, returns null. A trailing semicolon does not change this rule. There is no implicit return statement or early return syntax.

For example `do { 1; }` is 1, `do { x=1; }` is null, and `{x=>a=x+1; a*2}` is a function whose result is twice x+1.

### 5.2 Call signatures and the callback slot

A function signature consists of:

- Minimum and maximum ordinary positional arity.
- A callback-slot policy: absent, required, or optional.
- For user named functions, names for all ordinary parameters and the optional declared slot.

User-defined ordinary arity is exact, and their callback slot is either absent or required. Only builtin sort and unique have an optional callback slot. Lambda values never themselves declare a callback slot. A function alias preserves its signature.

A call performs:

1. Evaluate the callee.
2. Evaluate ordinary arguments left-to-right.
3. Create, but do not execute, the attached callback closure if supplied.
4. Check that the callee is a function. Check ordinary positional arity first; a failure has arity reason positional. Only if that succeeds, check callback presence (missing_callback or unexpected_callback).
5. Enter the function or builtin.

These ordering rules apply even if the callee is not callable. An argument's error can therefore precede the callee type/arity error. Compile-time optimization MUST preserve this observable behavior.

`fun apply(x){f} f(x) end fun apply` declares one ordinary parameter and one independent required callback parameter. `apply(10){v=>v+1}` supplies those two channels. Inside the body f is an ordinary function value. The slot name carries no invocation count or timing magic: the function body controls whether/how often it calls f.

The following distinctions are normative:

```kiroha
fun apply(x){f} f(x) end fun apply
g={v=>v+1};
apply(10){v=>g(v)}       // valid; 11
```

```text
apply(10)                       invalid at runtime: missing callback, arity
apply(10,{v=>v+1})              invalid syntax: positional lambda literal
apply(10,g)                     runtime arity mismatch; g is positional data
apply(10){g}                    invalid syntax: not a Lambda
apply(10){v=>v+1}{w=>w}        invalid syntax: second callback
```

A user function can intentionally accept function-valued ordinary data: `fun invoke(f,x) f(x) end fun invoke g={x=>x+1}; invoke(g,2)` is valid. Records and arrays may also carry functions. This does not fill, erase, or reinterpret a declared callback slot.

Before a builtin performs an effect, invokes a callback, or takes an empty-input shortcut, it validates all explicit argument types/options and the callback's callable signature. “Accepts n” means that a call with n ordinary arguments and no callback is legal for that function. A supplied value that is not a function raises type; an incompatible signature raises arity. This also applies to retry.when and every serve handler. Return types are checked when results are obtained.

### 5.3 Pipe

A pipe stage MUST be a dotted lexical path followed by parentheses and at most one trailing Lambda. The grammar does not accept a bare name, arbitrary callee expression, implicit projection, or placeholder.

`lhs | f(a,b){x=>body}`:

1. Evaluate lhs once.
2. Resolve/evaluate the stage's callee path.
3. Evaluate a,b left-to-right.
4. Create the callback closure.
5. Invoke f with ordinary arguments [lhs,a,b] and the separate callback.

A dotted path uses the same member selection as ordinary dot access, including bound collection methods. The pipe value remains the first explicit argument; it does not replace a bound receiver. A sequence of pipes associates left. The callback attaches to the immediately preceding call before pipe composition. No pipe automatically unwraps error/result records, flattens arrays, retries calls, or creates concurrency.

### 5.4 Operators and access

| Form | Required operands and result |
| --- | --- |
| `!x` | bool; logical negation |
| `-x` | number; exact negation |
| `a+b` | two numbers (sum), two strings (concatenation), or two arrays (concatenation) |
| `a-b a*b a/b a%b` | two numbers; Section 3 |
| `a<b a<=b a>b a>=b` | two numbers or two strings; bool |
| `a==b a!=b` | whole-operand JSON validation followed by structural equality |
| `a&&b` | evaluate a, require bool; false returns false; otherwise evaluate b and require bool |
| `a\|\|b` | evaluate a, require bool; true returns true; otherwise evaluate b and require bool |
| `a??b` | return a unless it is null; otherwise evaluate and return b, without type restriction |
| `if c then a else b` | evaluate c, require bool; evaluate exactly one branch |

The grammar gives precedence and associativity. There is no truthiness. `false`, 0 and empty string are preserved by `??`.

Bracket access evaluates receiver then key. Dot access first selects registered members as specified in §7.1.1; otherwise it performs ordinary record field access. A record requires a string key; an array requires a nonnegative integer number. Wrong receiver/key kind raises type; a fractional or negative array index raises argument; an out-of-range index raises bounds; an absent record key raises missing. Record access treats every string literally, including numeric-looking strings.

Optional access first evaluates its receiver. If null, return null without evaluating a dynamic key. Otherwise evaluate the key and apply the same type/domain rules; only an absent key or an out-of-range **valid** index produces null. `[1]?[-1]` errors; `[1]?[4]` returns null. Bracket access on strings, Bytes, functions, schemas and handles always raises type. Dot selection may access their registered members; unsupported access raises type. There is no string/Bytes indexing or prototype lookup.

### 5.5 Try, errors and tail calls

`try e catch {err=>sequence}` evaluates e. On a catchable runtime error, pop that try handler, bind the error record as err in a new scope, and evaluate the handler sequence. The handler does not catch its own failures. Static errors, ancestor cancellation and host termination bypass all Kiroha catch handlers.

Proper tail calls MUST use bounded continuation space, excluding space retained by program values. Tail positions are: a function/lambda result; the final expression of a tail-position sequence/do; either selected branch of a tail-position if; the right operand of a tail-position `??`; the final invocation of a tail-position pipe; and the result of the catch handler of a tail-position try after its handled frame is popped.

Calls in a protected try expression are not tail calls because a handler remains pending. Callees, arguments, accesses, arithmetic/comparison operands, and `&&`/`||` operands are not tail positions. A function that recursively installs a pending catch each time is not promised constant space. Recursion in an explicitly tail-recursive loop must not overflow the host stack for a million iterations under a sufficient budget.

### 5.6 Effects and implementation freedom

Observable effects include network/DB/file operations, log, time/random reads, waits, server lifecycle, cancellation, and order of callback execution. An implementation may fuse pure work or choose an interpreter, VM, compiler, JIT, or host-language backend only if values, errors, permitted event order, deadlines, and capability checks remain conformant.

There is no promise of transactional effect rollback, exactly-once delivery to a remote service, deterministic real-world scheduling, or replay. Kiroha's sequential call syntax suspends the current task during asynchronous host operations; it MUST NOT block unrelated live runtime tasks merely because the source omits async/await.

## 6. Schemas

### 6.1 Schema language and normalization

The prelude provides nominal schema atoms `str num int bool json`. They are values, not quoted strings or type annotations. Other values become schema descriptions only when passed to a schema API; a record literal containing atoms is still an ordinary record value.

Valid descriptions are defined recursively:

| Description | Accepted values |
| --- | --- |
| `str` | string |
| `num` | number |
| `int` | integer-valued number |
| `bool` | bool |
| `json` | any recursively valid JSON value |
| `[S]` | an array of arbitrary length whose every item matches S |
| `{k:S,...}` | a closed record with exactly the listed fields |
| `maybe(S)` | null or a value matching S |
| `optional(S)` | a field wrapper permitting absence; only as an immediate record-field description |
| `open(S)` | a record schema that permits additional JSON-valued fields |

An array description must contain exactly one element. `[]` and `[str,num]` are malformed schemas. An empty record schema describes exactly an empty record. Literal strings/numbers/bools/null are not schemas. Schema descriptions cannot contain functions, Bytes or handles.

`maybe(S)` validates/normalizes S immediately and rejects an optional field wrapper. Repeated maybe wrappers normalize to one. `optional(S)` validates S immediately, rejects an optional wrapper operand, and returns a field-only wrapper; `optional(maybe(str))` permits an absent field or present null/string. Creating a wrapper value is legal outside a record; using that wrapper as a root or array-element schema is a schema error. `maybe(optional(str))` is invalid.

`open(S)` accepts a plain record description or an already-open record schema, validates all fields, and returns a nominal open record schema. It does not accept maybe, scalar, array, or optional descriptions. Reapplying open is idempotent. Fields that the schema knows are still checked; extras must recursively be JSON and are preserved.

No schema API coerces, strips, inserts default fields, mutates values, or accepts aliases beyond the descriptions listed here. Record descriptions retain field order for diagnostic traversal. Compiling/caching a schema is allowed only without changing when malformed schemas are reported.

### 6.2 Validation algorithm and diagnostics

`check(v,S)` first fully normalizes S, then validates v, and returns v unchanged. A malformed schema wins over a mismatch in v. Root/field wrapper legality is checked even for empty input arrays or missing fields.

Visit record schema fields in description insertion order and array elements in index order, depth-first. For a closed record, check known fields first, then detect the first additional field in the value's insertion order. An open record validates extras as JSON in their insertion order. Report only the first failing path.

A mismatch is `validation` with exactly the required fields:

```json
{"path":["users",2,"id"],"expected":"int","actual":"string"}
```

The path contains decoded string keys or nonnegative integer indices. `actual` is a Section 3 kind, or `"missing"` for an absent required field. `expected` is one of `str num int bool json array record absent`. “absent” denotes an extra field of a closed record. maybe delegates to its inner expectation for non-null values. No input value or unexpected key value is copied into the error; the path itself necessarily includes a field name.

Malformed schema errors use code `schema` and data `{path,reason}`. Reasons are `invalid_description`, `array_arity`, `optional_position`, `nested_optional`, or `record_required`, with a path relative to the schema description. Constructor errors use path `[]` unless a nested description caused the failure.

`input` uses the same validation algorithm but translates data mismatch/JSON/UTF-8 failures to input errors; malformed schema remains schema (Section 12).

## 7. Standard data library

### 7.1 Closed API inventory

Web1 has exactly **31 global functions, 19 collection methods and one size property**, plus five schema atoms. The 51 native operations below are exhaustive. Globals are ordinary lexical bindings and may be shadowed; methods are selected by receiver kind and are not prelude names. The following inventory is exhaustive. `=` in signatures documents a builtin default; default-argument syntax is not legal in user definitions. Optional callback notation in this table is descriptive only.

| # | Signature | Normal result / exact special rules |
| --- | --- | --- |
| 1 | `x.size` | Integer length of array, record, string scalars, or Bytes octets; other kinds type error |
| 2 | `range(start,stop,step=1)` | All integer start+k*step before exclusive stop; step nonzero; wrong direction gives [] |
| 3 | `xs.map{f}` | f(item), once per item, in order; output array |
| 4 | `xs.filter{f}` | f(item) must be bool; preserve items whose result is true |
| 5 | `xs.fold(initial){f}` | Left fold f(acc,item); empty xs returns initial |
| 6 | `xs.each{f}` | f(item) in order; discard each result; return null |
| 7 | `xs.flat()` | Every item must be an array; concatenate exactly one level |
| 8 | `xs.take(n)` | First min(n,xs.size) elements; n nonnegative integer |
| 9 | `xs.sort{f}` | Stable ascending key sort; omitted callback means identity |
| 10 | `xs.group{f}` | First-seen groups `[{key,items}]` using JSON key equality |
| 11 | `xs.unique{f}` | Keep first item for each JSON key; omitted callback means identity |
| 12 | `xs.sum()` | Exact numeric sum; empty is 0 |
| 13 | `r.keys()` | String array in record insertion order |
| 14 | `r.has(k)` | Record own-key presence; k must be string |
| 15 | `r.pick(ks)` | New record in ks order; every key present; duplicates in ks error |
| 16 | `a.merge(b)` | Shallow record merge; b values win, existing positions stay |
| 17 | `xs.one()` | Sole item; cardinality error for any other length |
| 18 | `s.split(sep)` | Literal nonempty separator, retain empty leading/trailing parts |
| 19 | `xs.join(sep)` | Join array of strings with string separator |
| 20 | `s.replace(old, new)` | Replace all left-to-right nonoverlapping literal occurrences; old nonempty |
| 21 | `text(x)` | String unchanged; number/bool/null canonical JSON spelling; other kinds type error |
| 22 | `number(s)` | Complete signed JSON-number string, no surrounding whitespace |
| 23 | `parse(s)` | Strict JSON parse; exact decimals and ordered records |
| 24 | `dump(v)` | Canonical compact JSON string, or type error |
| 25 | `encode(s)` | UTF-8 Bytes, no BOM |
| 26 | `decode(b)` | Strict UTF-8 string; preserve a valid U+FEFF scalar |
| 27 | `check(v,S)` | Validate with Section 6; return unchanged v |
| 28 | `maybe(S)` | Nullable schema wrapper |
| 29 | `optional(S)` | Optional record-field wrapper |
| 30 | `open(S)` | Open record schema |
| 31 | `xs.parallel(n){f}` | Bounded concurrent f(item), ordered results |
| 32 | `sleep(ms)` | Cancellable wait; null |
| 33 | `retry(o){f}` | Explicit error-based retries; f takes zero ordinary arguments |
| 34 | `pages(seed,o={}){step}` | Cursor pagination; flattened result array |
| 35 | `fail(err)` | Raise a validated ordinary error |
| 36 | `http(method,u,o={})` | `{status,headers,body:Bytes}` for a final HTTP response |
| 37 | `get(u,o={})` | Require HTTP 2xx; decode/parse JSON |
| 38 | `post(u,v,o={})` | Send JSON; require 2xx; decode/parse JSON |
| 39 | `url(base,segments,query={})` | Strict URL construction with encoded segments/query |
| 40 | `db(name)` | Borrow configured PostgreSQL pool handle |
| 41 | `sql(d,Q,o={})` | `{rows:[record],count:int}` |
| 42 | `tx(d,o={}){f}` | Transaction callback f(t); commit result or rollback/fail |
| 43 | `serve(routes,o={})` | Serve until termination; no normal return |
| 44 | `input(r,S=json)` | Parse and validate an incoming JSON request |
| 45 | `reply(status,v,headers={})` | Nominal Reply |
| 46 | `env(name)` | Configured environment string |
| 47 | `read(path)` | File Bytes |
| 48 | `write(path,b)` | Atomic byte replacement; null |
| 49 | `now()` | UTC timestamp string with milliseconds |
| 50 | `uuid()` | Lowercase random UUIDv4 string |
| 51 | `log(v)` | One JSON line on stderr; null |

The `decode` row's exact policy is: strict UTF-8 decoding **preserves** a leading U+FEFF as a normal string scalar. Source and JSON parsing subsequently reject a leading BOM. Bytes `EF BB BF` alone decode to `"\uFEFF"`; they are not invalid UTF-8. A decoder MUST NOT silently strip them.

### 7.1.1 Receiver members and bound calls

`.size` is a property on arrays, records, strings and Bytes. Array methods are map, filter, fold, each, flat, take, sort, group, unique, sum, one, join and parallel. Record methods are keys, has, pick and merge. String methods are split and replace. These names, and len, are not global prelude functions.

A dot selection evaluates its receiver exactly once. A registered member for that receiver kind takes precedence over a record field; otherwise an existing record field is returned. Brackets always access data, so `r["size"]` and `r["keys"]` escape collisions. A known member on an unsupported non-record receiver raises type, with the receiver at position 0. On a record without a matching field, an inapplicable known member also raises type. Other missing names use ordinary access errors. Optional selection returns null for a null receiver or an absent ordinary field; it does not suppress a wrong-kind error. Optional selection does not make a subsequent call optional.

Method selection returns a bound function retaining its immutable receiver and target callable. Its public arity excludes the receiver. It can be saved and called normally: `m=[1,2].map; m(){x=>x+1}`. Direct zero-argument callback calls also accept `[1,2].map{x=>x+1}`. A method with no callback uses parentheses, such as `xs.sum()`. Calling `xs.size()` attempts to call a number and raises type. Method lookup is independent of lexical names such as a local `map` binding.

For invocation, resolve the member first, then evaluate explicit arguments left to right, construct the trailing closure, and perform arity/callback preflight before running the target. Native method diagnostics use position 0 for the receiver and 1 onward for explicit arguments. Callback-result diagnostics retain position 0. Collection methods retain the native validation, resource, effect and cancellation rules below. The grammar and bound-call mechanism do not depend on builtin member names; this revision does not expose user extension declarations.

### 7.2 General builtin validation

Array functions require array xs; record functions require records. Builtin defaults apply only to omitted arguments/option fields. Explicit null is a value and never requests a default. Unknown options are argument errors. A malformed options value is a type error.

Validate explicit top-level arguments left-to-right, then option fields in the published table order, then a supplied callback signature, before doing work. Validate unknown option names after known fields in input insertion order. For collections whose element types are constrained, validate all input elements in input order before invoking a callback. This does not validate unknown future callback results. `sort`/`group`/`unique` evaluate a callback result, validate it, then move to the next item; a failure prevents remaining callbacks.

Type errors identify the function/operator, argument position, expected type and actual kind. Domain errors for a correct kind use argument, unless a more specific code is listed. Examples: `range(0,4,0)`, `xs.take(-1)`, `s.split("")`, and duplicate pick keys are argument errors. Missing pick keys raise missing. One raises cardinality with the actual length. User callback failures propagate unchanged.

Callback arity is one for map/filter/each/sort/group/unique/parallel/pages/tx, two for fold, and zero for retry's trailing callback. The identity default for sort/unique is a builtin identity operation, not source syntax. All ordinary collection callbacks are sequential, including when called inside one parallel child.

### 7.3 Detailed collection behavior

`range` checks all three arguments are integers. Positive step emits x while x<stop; negative step emits x while x>stop. The sequence is computed exactly and may terminate by host allocation quota. Equal endpoints return [] regardless of nonzero step.

`flat` does not flatten recursively and does not retain non-array members: `[[1],[2,[3]]].flat()` is `[1,2,[3]]`; `[[1],2].flat()` raises type. `take` never mutates or lazily traverses xs.

`sort` obtains each key once, in input order, and stores it alongside the original index/item. Every key must be a number or every key a string; mixed kinds, booleans/null, records and arrays are type errors. Empty xs returns []; one item still has its key evaluated/validated. Ties retain input order. Sorting itself must not invoke the callback again.

`group` and `unique` permit any JSON key, including arrays/records. They validate the entire key as JSON. The first observed key value is retained as the group's key, including that record's insertion order. A hash implementation MUST make numerically equal decimals and record-order-insensitive values hash compatibly, or use an equality fallback. Items within a group retain order; groups retain first appearance order.

`pick` first validates every requested key's type and duplicate status, then copies fields in requested order, raising missing for the first absent key. It never inserts null. `merge` accepts two records only and performs no recursive merge. `has` never tests a prototype and never treats null as absent.

### 7.4 Text and JSON edge cases

All text operations use literal scalar sequences, with no regex or locale behavior. Empty separators are allowed by join, but forbidden by split/replace. `"".split(",")` is `[""]`. `"aaaa".replace("aa", "b")` is `"bb"`. Replacement text is not rescanned. The input to number must be a string and must match signed JSON-number grammar exactly; invalid spelling raises number with reason `invalid_syntax`. No trimming, hexadecimal, underscores, Infinity or NaN.

`parse` uses json errors for malformed content and type for a non-string argument. `decode` uses utf8 errors for malformed encoding and type for a non-Bytes argument. `encode(decode(b))` equals b byte-for-byte for every valid UTF-8 sequence, including a BOM; the language has no Bytes equality operator, so tests observe bytes through the test protocol.

## 8. Failures, diagnostics, budgets, and task scopes

### 8.1 Three distinct outcomes

Each evaluation has one of these outcomes:

| Outcome | Catchable by Kiroha try? | Representation |
| --- | --- | --- |
| Normal result | Not applicable | A Kiroha value |
| Ordinary failure | Yes | JSON error record `{code,message,data}` |
| Abort | No | Host/runtime control outcome with a reason |

Lex/parse/check diagnostics occur before evaluation and are not values or catchable failures. An abort is not converted to an ordinary error by arbitrary try handlers.

Every ordinary error has exactly three outer fields: nonempty string code, string message, record data containing only JSON values. Messages may vary by implementation and are never control-flow keys. The runtime does not add stack traces or source spans to the caught error; diagnostics can carry those separately. User errors may use any nonempty code.

`fail(err)` validates this exact shape; extra/missing fields, wrong types, or non-JSON data raise type/argument as applicable. A valid err is raised unchanged. Thus raising an error in a catch handler is explicit and preserves its data. Failure does not roll back unrelated effects.

### 8.2 Stable runtime error contracts

Unless stated otherwise, data has the required fields below and MAY add diagnostic JSON fields. Tests use required-field matching, not unspecified messages. Added fields MUST NOT include credentials, request bodies, SQL parameter values, or whole validation input values by default.

| code | Required data / meaning |
| --- | --- |
| `type` | `{operation,position,expected,actual}`; position is a 1-based argument index, 0 for receiver/callee/result, or an option-field string |
| `arity` | `{operation,min,max,got,callback,reason}`; callback is expected policy `"absent"`, `"required"` or `"optional"`; reason is `"positional"`, `"missing_callback"`, `"unexpected_callback"` or `"callback_signature"` |
| `argument` | `{operation,reason}`; a value has the right kind but violates a domain/configuration rule |
| `missing` | `{key}` for absent record key |
| `bounds` | `{index,length}` for valid nonnegative index outside an array |
| `number` | `{reason}`: `division_by_zero` or `invalid_syntax` |
| `json` | `{reason,offset}`; reason `syntax`, `duplicate_key`, `surrogate`, `bom` or `trailing`; offset is UTF-8 byte position |
| `utf8` | `{offset}`; index of the first invalid octet sequence |
| `schema` | `{path,reason}` as Section 6 |
| `validation` | `{path,expected,actual}` as Section 6 |
| `cardinality` | `{expected:1,actual}` |
| `network` | `{operation,reason,outcome}`; outcome `not_sent` or `unknown` |
| `tls` | `{operation,reason,outcome:"not_sent"}` for failed authentication before application bytes |
| `timeout` | `{operation,timeout}`; add outcome for remote/file effects as defined below |
| `limit` | `{operation,limit,maximum}`; explicit operation byte/row limit exceeded |
| `http` | `{status}`; valid final status outside 200..299 |
| `pagination` | `{reason,pages,items}`; reason `repeated_cursor`, `max_pages` or `max_items` |
| `db` | `{phase,outcome}`; optional sqlstate when supplied by the server |
| `input` | `{reason}`, plus validation path/expected/actual if applicable |
| `media` | `{reason}` |
| `env` | `{name}`; configured name absent |
| `io` | `{operation,reason,outcome}`; outcome `not_applied`, `applied` or `unknown` |
| `permission` | `{operation,capability}` |

Codes are not nominal types. A user can deliberately raise an input error and obtain the corresponding server error response. Host capabilities, transaction abort, and cancellation MUST NOT depend on trusting a code string.

Malformed byte/source input produces lex diagnostics; malformed grammar produces parse diagnostics; unresolved/duplicate names produce check diagnostics. Primary diagnostic codes are `lexical`, `syntax`, `duplicate_key`, `name` and `duplicate_binding` as appropriate. A syntax error cannot be hidden in an unexecuted branch.

When two constraints could fail, lexical/parser/static checks precede all execution; expression evaluation follows Section 5; builtin validation follows Section 7; then each operation's stated order applies. An abort already requested at a mandatory checkpoint takes precedence over a normal result or ordinary failure not yet delivered. Among concurrent ordinary failures use Section 9, not wall-clock guessing.

### 8.3 Time and scopes

All durations exposed by Web1 are nonnegative integer milliseconds. Timeouts must be positive integers; sleep and retry delay may be zero. The runtime uses a monotonic clock for durations and a separate UTC wall clock for now. An operation's deadline is the earlier of its own start+timeout and every applicable parent deadline. Argument evaluation occurs before operation entry and is not included in that operation's timeout; a parent deadline still applies.

The runtime distinguishes:

- **Root/request/task abort:** ancestor cancellation, request deadline, process shutdown, host memory/work quota, or host deadline. This bypasses every Kiroha try in that scope and descendants.
- **Operation timeout:** a live caller's http/sql operation expires. It returns an ordinary timeout after cleanup.
- **Transaction scope timeout:** a tx deadline aborts its entire callback scope, including pure computation and nested catch handlers. Only the tx boundary converts this abort to an ordinary timeout if its enclosing task is still live.
- **Cleanup scope:** a runtime-only, non-user-code scope with a fresh bounded cancellation/deadline budget.

The tx callback retains the caller's task identity, even though it receives a nested abort scope. This is essential for transaction ownership. Parallel children always have distinct task identities.

An evaluator MUST check abort state before every function/builtin invocation, at every tail-call backedge, before starting/delivering an effect, and at least every 1024 work steps within long work. A work step for this checkpoint bound includes a node evaluation or one loop iteration over a container/digit/byte sequence; the separate eval_steps budget counts AST entries as Section 14 specifies. Long bigint, encoding, sorting, parsing or transport work MUST be interruptible or isolated so it can be abandoned without invoking further user callbacks or initiating another effect after abort is observed. Already-dispatched external operations may still complete or commit; their cleanup and outcome reporting follow the applicable effect contract. A host library that monopolizes the event loop indefinitely does not satisfy this contract.

Ordinary max_bytes/max_rows limits are catchable while the task is live. Host resource exhaustion is an abort; it must not be turned into an approximate number, truncated collection, successful partial response, or catchable retry loop.

### 8.4 Cleanup and error precedence

User work stops when its scope aborts. Cleanup never invokes user lambdas, retries a user operation, or resumes the program. Cleanup gets a separate monotonic deadline, configured by the host as cleanup_ms (default 1000). It may outlive the expired operation deadline only by this bounded grace.

Cleanup joins/cancels tasks, releases or discards leases, closes transports and attempts rollback where applicable. If rollback/reset cannot be confirmed, discard the connection. An unresponsive host adapter is forcibly detached/terminated at the boundary; it MUST NOT continue user callbacks or reuse a dirty connection.

For an existing primary failure, cleanup failures are recorded by the host and MUST NOT replace it. If successful user work cannot safely finalize, finalization failure becomes the operation failure. Root/ancestor abort always dominates ordinary failures. Section 11 gives the additional first-SQL-failure rule.

Logging an internal cleanup failure is best-effort; logging failure must not recursively prevent cleanup. Completed external effects can remain after any failure or abort. “Cancelled” never means “remote action did not happen.”

## 9. Concurrency, retries, pagination

### 9.1 xs.parallel(n){f}

Validate xs, positive integer n and f's one-argument signature before creating children. An empty array returns [] and creates no task, but still validates f. Each child receives one item, the same immutable lexical captures/capabilities, a distinct task identity and inherited abort/deadline state.

Starts are assigned to ascending input indices. Initially at most min(n,xs.size) children start. Each completion frees one slot; if no failure/abort has been observed, start the next unstarted index. No input starts twice. At most n callbacks are active, including suspended callbacks. Nested parallel calls each impose their own limit; the host's overall task quota still applies. n is not a global network rate limit.

Collect results at their original indices. Normal return waits for all children and returns the complete ordered array. A callback may itself return any value kind; there is no implicit JSON conversion.

On the first **observed** ordinary child failure:

1. Latch that exact error and stop scheduling unstarted indices.
2. Request abort of every still-running sibling.
3. Join all started children, including their cleanup, within the cleanup rules.
4. Raise the latched error in the parent.

The scheduler defines observation order; production runtimes need not make simultaneous external completions deterministic. Later ordinary sibling failures cannot replace the first observed error. A root/ancestor abort dominates the latched ordinary failure. A descendant host-resource abort propagates as an abort and cancels the scope; it is never an ordinary error. Cancellation try handlers in siblings do not execute. Completed external actions are not reversed.

To collect ordinary per-item failures, catch inside f and return data. This does not make cancellation catchable. Shared immutable arrays/records need no locks; pool handles may be shared, transaction handles may not.

### 9.2 sleep(ms)

ms is a nonnegative integer. A successful sleep completes no earlier than its requested monotonic interval and returns null; scheduler delays may make it later. sleep(0) is an abort checkpoint and yields to other runnable tasks before resuming. Sleeping is not a wall-clock busy wait.

### 9.3 retry(o){f}

Options are required and have no defaults:

| Field | Type / domain |
| --- | --- |
| `attempts` | Integer >=1, counting the initial attempt |
| `when` | Function callable with one error and no callback |
| `delay` | Integer milliseconds >=0 |

Unknown keys error. Validate the complete options and f's zero-argument signature before attempting f.

For attempt i=1..attempts, invoke f in the current task. On success return its value. On ordinary failure e:

- If i==attempts, raise e **without calling when**.
- Otherwise call when(e) once and require bool.
- If false, raise e unchanged.
- If true, wait `delay*2^(i-1)` milliseconds, then attempt again.

A when failure or non-bool result replaces e and ends retry. A delay calculation remains exact; exceeding host limits aborts. Abort during f/when/backoff bypasses retry. There is no retry for validation errors occurring before f is entered, no jitter, status classification, idempotency inference, or deadline reset. Every attempt may cause a new external effect; the caller explicitly owns that policy.

### 9.4 pages(seed,o={}){step}

seed must be JSON; step accepts one argument and returns an exact record with fields items and next, no extras. items must be an array of arbitrary values; next must be JSON. A malformed step result raises type/argument (not pagination). Options, in validation order:

| Field | Default | Domain |
| --- | --- | --- |
| `max_pages` | 100 | Integer >=1 |
| `max_items` | 10000 | Integer >=0 |

Initialize cursor=seed, pages=0, result=[]. Remember seed in a seen set if it is non-null, using JSON equality. Invoke step once even if seed is null. For each returned page:

1. Validate the entire result shape, items and next.
2. Increment pages and calculate prospective item count.
3. If prospective count exceeds max_items, raise pagination(max_items) with pages and that prospective count.
4. Append items in order.
5. If next is null, return result, including when exactly on either limit.
6. If next is already in seen, raise pagination(repeated_cursor), without invoking another step.
7. If pages>=max_pages, raise pagination(max_pages).
8. Add non-null next to seen, assign it to cursor, and repeat.

Only null is terminal. 0, false, empty string, [], {} and an empty items array do not end pagination. The first test in this order wins if several conditions coincide. Failed pagination returns no partial array; effects already performed by step remain. There is no speculative next-page request.

## 10. HTTP client and URL contract

### 10.1 HTTP transport profile

Web1 requires HTTP/1.1 client and server transport; implementations may also negotiate HTTP/2/3 only with the same Kiroha-visible contract. HTTP framing/TLS implementations may be dependencies. Kiroha-visible rules here take precedence over framework conveniences. HTTP semantic definitions follow RFC 9110.[H]

http accepts an uppercase ASCII HTTP method token: token characters are letters, digits and `!#$%&'*+-.^_` plus backtick, pipe and tilde; every alphabetic character must be uppercase. CONNECT is rejected with argument because Web1 has no tunnel mode. There is no automatic HEAD/OPTIONS substitution. Informational responses are consumed internally; 101/upgrade and a response without a valid final 200..599 status are network errors. No automatic redirects, authentication negotiation, retries, cookies, proxy discovery or response cache.

https MUST verify the server certificate chain and hostname using the configured host trust store. No source option disables verification. URLs contain no ambient credentials. Connection pooling/reuse is permitted only without hidden cookie/auth/session state. A failed stale connection is not automatically retried once an operation has been dispatched.

### 10.2 Accepted URLs and the url constructor

An input URL is an ASCII absolute URI with scheme http or https (case-insensitive), nonempty authority and no fragment or userinfo. Reject controls, spaces, backslashes, invalid percent escapes, an empty port, and ports outside 1..65535. A host is a DNS name in ASCII, strict dotted-decimal IPv4, or bracketed IPv6 without a zone identifier. Raw Unicode host/path/query input is rejected; pre-encoded ASCII/punycode hostnames and percent-encoded UTF-8 components are allowed. DNS host spelling is case-insensitive; nonstandard integer/octal/hex IPv4 spellings are rejected.

The runtime MUST preserve the supplied path and preexisting query bytes and must not resolve dot segments, collapse repeated slashes, decode encoded slashes, or reinterpret percent escapes. An absent path is normalized to `/`. Scheme and DNS host are lowercased; a supplied valid port and existing percent-escape case are retained. Empty query marker `?` is permitted and preserved until query pairs are appended. Host policy compares origins after lowercasing the host/scheme and replacing an omitted port with 80/443; it does not infer authorization from a textual prefix.

`url(base,segments,query={})` validates base with these rules, validates every segment as string, and rejects any segment whose complete value is "." or "..". Each segment is UTF-8 percent-encoded, leaving only ASCII `A-Z a-z 0-9 - . _ ~` unescaped; use uppercase hex digits. A slash inside a segment becomes `%2F`, a percent sign `%25`, space `%20`, and plus `%2B`. No percent-decoding is performed before encoding. Empty segments are legal.

If segments is empty, preserve the base path unchanged. Otherwise remove at most one trailing slash from the base path, then append `"/"+encodedSegment` for every segment in order. Thus `/a/` plus `["","b"]` gives `/a//b`; existing additional trailing slashes remain. An absent base path has already been normalized to `/`, so appending "x" gives `/x`.

Query construction uses input record insertion order. Values are strings/numbers/bools/null or one-level arrays of those scalars. null omits an occurrence; an empty array emits nothing. Strings are unchanged before encoding; other emitted scalars use text. Encode names and values using the same unreserved-only encoding, including always encoding space as %20. Join as `name=value` pairs with `&`. Append pairs after existing query text with one & when nonempty; an empty existing query uses only ?. No new pairs means the existing query is unchanged. Nested arrays/records raise type.

```kiroha
url("https://api.example/base?old=1",["a/b","x y"],{q:["a b",null,true],n:0})
// "https://api.example/base/a%2Fb/x%20y?old=1&q=a%20b&q=true&n=0"
```

This constructor never changes origin. URI component and percent-encoding concepts follow RFC 3986; the joining/preservation policy above is the stricter Kiroha contract.[U]

### 10.3 Headers

Outbound headers are records of string values. Field names are nonempty ASCII HTTP tokens, compared case-insensitively; duplicate lowercase names are argument errors. Values may contain horizontal tab, space and visible ASCII U+0021..U+007E, but not CR/LF, other controls, DEL or non-ASCII. Leading/trailing spaces/tabs are removed when sending.

The runtime owns Host, Content-Length, Transfer-Encoding, Connection, Keep-Alive, Proxy-Connection, Upgrade, TE and Trailer; callers supplying any of these names get argument. It also rejects Expect (no 100-continue API). It sets framing, SNI and connection management internally. A user Authorization header is allowed and is never automatically forwarded to another origin because redirects are disabled.

Incoming headers become a record of lowercase names to arrays of values in wire occurrence order, with leading/trailing optional whitespace trimmed. Do not split commas or semicolons, and do not coalesce Set-Cookie. Header record key order is first occurrence order. HTTP/2 pseudoheaders are transport metadata and do not enter this record. For HTTP/1 field-value octets 0x80..0xFF, map octet-for-octet to U+0080..U+00FF; invalid controls/framing are transport failures. This mapping is not UTF-8 decoding.

### 10.4 http and its options

`http(method,u,o={})` validates method, URL, options and capabilities, then sends one request. It reads the entire final response within the deadlines/limits before returning:

```text
{status: integer, headers: record<string,array<string>>, body: Bytes}
```

| Option | Default | Type / meaning |
| --- | --- | --- |
| `headers` | {} | Outbound headers per Section 10.3 |
| `query` | {} | Query record appended by Section 10.2 |
| `body` | absent | Bytes; absent sends no body, present empty Bytes sends an empty body |
| `timeout` | 10000 | Positive integer ms, from operation entry through pool/DNS/TLS/send/read/decode |
| `max_bytes` | 8388608 | Nonnegative integer limit on decoded response-body octets |

GET and HEAD reject a supplied body. Other permitted methods may carry Bytes; request compression is absent. A missing content type for Bytes is left unspecified by http. If absent, the runtime sends `Accept-Encoding: gzip`; a supplied Accept-Encoding may request only gzip/identity. It never advertises unsupported content codings.

Support response content-coding absent/identity or a single gzip coding, case-insensitive. Reject other or stacked codings as network(reason=unsupported_encoding). Invalid gzip is network(reason=decode). Remove HTTP transfer framing, then apply content decoding, enforcing max_bytes incrementally on the decoded output. Validate a gzip stream's checksum/trailer; concatenate valid gzip members; reject trailing non-member bytes. Host transport/memory quotas may additionally abort excessive encoded input.

Response headers retain their normalized **wire** metadata, so Content-Encoding and Content-Length may describe the compressed wire representation rather than returned Bytes. They are not silently rewritten. A body exceeding max_bytes raises limit; no truncated successful body is returned. Equality at the byte limit succeeds.

A HEAD response, or status 204/205/304, has an empty returned body under this profile. HEAD/204/304 are treated as bodyless by framing and skip representation content-coding support checks as well as decoding; a HEAD response with an unsupported Content-Encoding still returns empty Bytes. A 205 follows ordinary message framing: consume valid zero-content framing completely, including a terminal zero chunk and permitted trailers when chunked, before reuse or success. Any nonzero 205 content is a network protocol error. Receiving 205 headers alone does not establish message completion. 1xx are consumed internally and never returned. Every valid final 200..599 status otherwise returns normally, including 4xx/5xx.

### 10.5 get and post

`get(u,o={})` accepts the http options except body and performs GET. `post(u,v,o={})` accepts the same reduced options, verifies v is JSON, serializes it with dump+UTF-8 and performs POST. An explicit post Content-Type is permitted only if it denotes application/json with at most charset=utf-8; names/value tokens compare case-insensitively. Otherwise post supplies application/json. Conflicting content types raise argument before network dispatch. JSON serialization performs no content compression; a supplied Content-Encoding must be identity, case-insensitive, or absent, otherwise argument before dispatch.

Both wrappers follow this exact order:

1. Validate call/options and, for post, serialize v.
2. Complete the underlying http operation including content decoding and byte limits.
3. Require status 200..299, else raise http with status. Do not attempt JSON decoding on a rejected status.
4. Strictly decode UTF-8, then parse exactly one JSON value.
5. Return that value, discarding response headers.

Response Content-Type does not suppress JSON parsing. Empty body, including successful 204/205, is a json error, not null. A 404 with malformed JSON yields http; a 404 with an invalid gzip stream fails earlier as network. There is no special handling of 429/503 or Retry-After.

### 10.6 Outcome reporting

For network/TLS/timeout/limit failures associated with a request, report outcome `not_sent` only when the adapter can establish that no application request bytes were sent. Otherwise report `unknown`. Receiving an HTTP response does not make replay automatically safe; a subsequent JSON/UTF-8 error does not undo the request. TLS certificate failure before application bytes is tls/not_sent; later TLS stream failure is network/unknown.

No ordinary error is proof that a remote mutation was rolled back. This is an observable API property, not a hidden retry policy.

## 11. PostgreSQL

### 11.1 Connection and dialect boundary

Web1's reference database dialect is **PostgreSQL 16**. A conforming adapter must pass the required integration cases against PostgreSQL 16; newer servers may be used only while preserving this profile. Kiroha is not a SQL implementation. A PostgreSQL driver and a PostgreSQL-aware statement parser are permitted dependencies. The external server defines SQL results, SQL errors, privileges, triggers and read-committed isolation.[P][I]

`db(name)` accepts a string alias in the host configuration. Check permission before lookup; an authorized but unavailable configured pool raises db with phase pool. It returns a reusable borrowed database handle. It neither creates a user-owned pool per call nor reserves a connection. Repeated db(name) uses the same configured pool. There is no source-level close.

The host owns pool limits, credentials, lifetime and shutdown. Pool acquisition observes the operation deadline. A lease is released only after protocol completion and session reset, or discarded. Reset must restore the configured baseline (including search_path, roles, timezone and prepared/session state), or the connection is discarded. The normalized release disposition is reset for a healthy connection in a known ended state; this asks the adapter to reset, with physical discard permitted if reset cannot be confirmed. Disposition discard is mandatory for uncertain protocol/COMMIT state and never returns that connection to reuse. Startup baseline is UTF-8 client encoding, standard_conforming_strings=on, and read-committed transactions. Server-side privileges remain the authority for allowed database actions.

### 11.2 Query translation and statement validation

`sql(d,Q,o={})` requires a database or live owned transaction handle and a Query value. An ordinary string never becomes SQL implicitly. Query values are immutable segments plus values (Section 2.4).

Translate holes to distinct PostgreSQL positional value parameters in encounter order, inserting a space before and after each generated `$n` token. A PostgreSQL-aware lexer/parser MUST establish that:

- Each hole occurs in ordinary SQL lexical state, outside strings, quoted identifiers, comments and dollar-quoted bodies.
- Each generated marker parses exactly once as a ParamRef expression. Holes cannot supply identifiers, keywords, operators or fragments.
- No user-written positional parameter token occurs in literal text outside quotes/comments. Thus source `$1` cannot alias a generated hole.
- There is exactly one nonempty statement; one optional trailing semicolon and trailing SQL whitespace/comments are allowed.
- The top-level command is SELECT, INSERT, UPDATE, DELETE or MERGE, optionally introduced by WITH. A top-level VALUES/TABLE shorthand is excluded; equivalent subqueries within SELECT are allowed.
- SELECT INTO, transaction/session control, DDL, COPY, CALL, DO, EXPLAIN, prepared-statement control, and utility statements are excluded. CTE statements must themselves be allowed SELECT/DML forms.

Reject violations before dispatch with db, phase `prepare`, outcome `not_sent`, and a reason identifying `statement`, `multiple_statements`, `hole_context` or `literal_parameter`. On a pool handle, all Query/statement/options/parameter validation completes before acquiring a connection; an earlier db(name) pool lookup is a separate call. Do not implement statement validation with a semicolon split or keyword regex. A semicolon in a SQL string/comment/dollar-quoted literal is not a second statement.

A Query whose literal text places a Kiroha hole inside SQL quotes is invalid when sql consumes it, even though query construction succeeded. Unknown/unsupported parameter values fail at binding, not by interpolating their text. This statement filter does not prevent side effects through SQL functions or triggers; the configured database role controls those privileges.

### 11.3 Parameters and results

The adapter preserves exact values at the driver boundary:

| Kiroha parameter | Wire binding |
| --- | --- |
| null | SQL NULL, unspecified type OID 0 for contextual inference |
| bool | PostgreSQL boolean, OID 16 |
| string | PostgreSQL text, OID 25, UTF-8 |
| number | PostgreSQL numeric, OID 1700, exact canonical decimal text or exact binary encoding |
| Bytes | PostgreSQL bytea, OID 17 |
| JSON record/array | PostgreSQL jsonb, OID 3802, exact dump text |
| Other / nested non-JSON | db, phase bind, outcome not_sent |

No parameter is a native SQL array. Explicit SQL casts can request another compatible SQL type. Drivers MUST NOT turn decimal/JSON numbers into host binary64 first. A string containing U+0000 cannot be bound as PostgreSQL text. JSONB has server numeric/Unicode restrictions; values outside the server representation fail as db/bind or db/execute, preserving whether dispatch occurred.[P]

Result metadata is an ordered column sequence. Supported **reported base type OIDs** are:

| PostgreSQL types / OIDs | Kiroha result |
| --- | --- |
| int2 21, int4 23, int8 20, numeric 1700 | Exact number; reject NaN/Infinity numeric |
| text 25, varchar 1043, bpchar 1042, uuid 2950 | String; do not trim bpchar beyond the server's transmitted representation |
| bool 16 | bool |
| bytea 17 | Bytes |
| json 114, jsonb 3802 | Strict parsed JSON, exact numbers |
| SQL NULL in a supported column | null |

Validate all column names for decoded-string uniqueness and all reported type OIDs before exposing any rows, including a zero-row result. Unsupported OIDs, native SQL arrays, enums, domains reported under distinct OIDs, date/time/timestamp, float4/float8 and other types raise db/phase decode. Use an explicit SQL conversion such as `to_jsonb(array_column)`, `date_column::text` or a deliberate numeric cast when needed. A server JSON value still must satisfy Kiroha's duplicate-key, finite-decimal and Unicode rules.

A row is a record in server column order; columns are never renamed or overwritten. The adapter retains raw exact representations until conversion succeeds.

### 11.4 sql options and outcome

| Option | Default | Domain |
| --- | --- | --- |
| `timeout` | 10000 | Positive integer milliseconds |
| `max_rows` | 10000 | Nonnegative integer rows in the returned result |

sql returns exactly `{rows:[record],count:int}`. rows=[] for no returned rows. count is the server command tag's affected/returned row count, not necessarily rows.size: DML without RETURNING can have count>0 and no rows. For SELECT, it is the returned row count. A missing/invalid command count is db/decode.

Outside a transaction, each sql executes with autocommit. max_rows bounds result rows, not affected rows; exceeding it raises limit, never truncates success. A result decoding/size failure can happen after the server committed an autocommit statement. Such failure MUST NOT claim rollback. HTTP/file effects of a SQL function likewise are outside Kiroha rollback guarantees.

All database-related remote errors add outcome:

- `not_sent`: the statement or COMMIT was definitely not dispatched.
- `rolled_back`: the relevant transaction is confirmed rolled back.
- `committed`: an autocommit command or COMMIT acknowledgment establishes commit.
- `unknown`: the adapter cannot establish the transaction outcome.

Outcome is a snapshot of evidence available when the error is created. For a dispatched statement inside an active transaction whose COMMIT has not completed, use unknown: the enclosing transaction's final outcome is unresolved. not_sent describes the failing statement or COMMIT attempt, not earlier statements. A later rollback does not mutate an already-created or latched error record. A confirmed COMMIT remains committed even if finalization prevents returning the callback result; cleanup cannot describe it as rolled back. These values do not describe external effects of triggers/functions. Raw server error messages may be logged under host policy but ordinary data must not echo parameter values. An available five-character SQLSTATE is included as `sqlstate`; user code must not infer retry safety merely from db.

### 11.5 tx lifecycle and poisoning

`tx(d,o={}){f}` accepts a database handle (not a transaction handle), positive timeout default 10000, and f callable with one ordinary argument. Validate arguments and no-active-tx-in-this-task before pool acquisition. A nested tx call in the same task is db/phase ownership/outcome not_sent even when it names another pool. A parallel child is a distinct task and may start its own transaction, but it cannot use its parent's transaction handle.

The transaction state machine is:

```text
acquiring -> beginning -> active -> committing -> committed -> released
                                \-> rolling_back -> rolled_back -> released
any uncertain protocol/cleanup state -> discarded
```

This is a state enumeration; no implementation-specific lock layout is required.

A tx scope:

1. Starts a monotonic operation deadline at entry, covering acquisition, BEGIN, callback, COMMIT and ordinary processing.
2. Acquires one connection and sends BEGIN at read committed.
3. Creates t with owner task identity, generation/lifetime marker, active state and no latched failure.
4. Invokes f(t) **in the same task**, under the tx abort scope.
5. Invalidates t as soon as the callback finishes or aborts.
6. If callback returned normally, no failure was latched and the scope is live, sends COMMIT.
7. After a confirmed commit, returns the callback value and releases/resets the connection.
8. On failure/abort, attempts bounded rollback and releases/resets or discards, then delivers the primary outcome.

A handle may be returned/stored/captured as an opaque value, but every later use fails because it has expired. No hidden connection escapes with it. A closure capturing t is also unable to revive it.

Entering `sql(t,...)` checks kind, owner task and active lifetime first. A wrong-task/expired-handle rejection does not poison a different owner or completed transaction. Once a valid active owned t is accepted, **every ordinary failure within that sql call** latches the first failure on t: Query/options/parameter validation, permission, dispatch, timeout, server error, decoding or row limit. Argument-expression errors happen before sql is entered and do not themselves poison t. An already-poisoned sql call raises the latched error without dispatching SQL.

A caught SQL error does not clear poison. On leaving f, first priority is any ancestor/host abort; second is tx-scope timeout; third is the first latched SQL failure; fourth is any other uncaught callback failure. This means a latched SQL error remains primary even if the callback later raises another ordinary error. Rollback/cleanup errors cannot replace it. A live caller can catch a tx-scope timeout after the entire callback scope has been stopped and cleaned up.

Only `sql(t,...)` joins the transaction. `sql(d,...)` with the pool handle remains an independent autocommit operation, even inside f. It may wait for another connection and be limited by the parent tx deadline. HTTP/file/log effects in f are independent and remain after rollback.

A COMMIT failure without a definite server rejection/rollback acknowledgment is db/phase commit/outcome unknown. Never replay the callback. An unknown COMMIT outcome, including timeout/abort after COMMIT dispatch, is an explicit exception to the rollback step: issue no further SQL, discard the connection and preserve the primary outcome. A confirmed COMMIT is never rolled back. A definite server-confirmed rollback releases/resets the connection; a failure before COMMIT dispatch still attempts rollback. Real cancellation/rollback uses the cleanup scope from Section 8, not the already-expired callback token.

## 12. HTTP server

### 12.1 serve configuration and route compilation

`serve(routes,o={})` takes a record of route strings to functions callable with one argument. It validates all routes, all handlers and all options before attempting to listen. Configuration/startup errors are ordinary errors in its caller. Successful serve stays active until its scope aborts; it has no normal return value and does not return a background handle.

| Option | Default | Domain |
| --- | --- | --- |
| `host` | `"127.0.0.1"` | Numeric IPv4 or IPv6 bind address string; no DNS resolution |
| `port` | 8080 | Integer 1..65535 |
| `timeout` | 30000 | Positive integer ms per request |
| `max_body` | 1048576 | Nonnegative integer request-body bytes |
| `max_inflight` | 128 | Positive integer concurrent accepted request tasks |

Host permissions authorize the exact bind endpoint. Listening is plain HTTP; TLS termination is an external deployment concern. No source option silently exposes all interfaces. Bind refusal/conflict raises io/operation serve.

A route key has exactly `METHOD SP /path`, with one ASCII space and a method accepted by Section 10. Paths have no query/fragment. Split the pattern by literal slash before decoding; `/` is zero segments, a trailing slash yields an empty final segment except for root. Repeated slashes create empty segments. A segment that starts with a raw colon must be `:IDENT` and is a parameter; repeated parameter names in a route are argument errors. A percent-encoded colon is a literal.

Literal route segments are percent-decoded once as strict UTF-8; "+" stays "+". Malformed encodings or raw non-ASCII route paths are configuration errors. Literal "." and ".." segments are allowed and have no normalization effect. No glob, regex, optional segment or catch-all exists.

Reject an ambiguous pair at configuration time if all are true:

1. Same method.
2. Same segment count.
3. Same number of literal segments.
4. At every index, either one segment is a parameter or both literals are equal.

Otherwise, for an incoming method/path, select a matching route of the same segment count with the highest literal-segment count. A parameter matches one decoded segment, including an empty segment. Ties cannot exist after validation.

### 12.2 Request target, headers and lifecycle

The host supplies the raw origin-form request target (path plus optional query), without framework normalization. Reject absolute-form, authority-form, asterisk-form, fragments, raw non-ASCII, controls and malformed percent sequences with 400. Split path into segments **before** decoding exactly once as UTF-8. Thus `/a/x%2Fy` has two segments and a parameter can be `"x/y"`; never split the decoded slash. No dot-segment removal or duplicate-slash collapse.

The request record is exactly:

```text
{
  method: string,                // original case-sensitive method token
  path: string,                  // raw encoded path, excluding query
  params: record<string,string>,
  query: record<string,array<string>>,
  headers: record<string,array<string>>,
  body: Bytes
}
```

Server methods containing lowercase ASCII are syntactically HTTP tokens but do not equal uppercase route methods. Invalid method tokens are 400. CONNECT, attempted protocol upgrade and any Expect header are rejected with 400 and `{"error":"bad_request"}` before admission; close the connection without invoking a handler or waiting for a request body. Request query is split on &, then each nonempty component on its first =; missing = means empty value. Decode percent escapes once as strict UTF-8 and decode + as space. Empty components between separators are ignored. An empty key is allowed. Accumulate repeated names in first-key/occurrence order. Headers follow Section 10.3.

Request transfer framing is removed. Request content-coding is identity/absent only; a non-identity Content-Encoding is 415. The runtime reads the complete body subject to max_body and timeout before running the handler. Header/framing limits are host quotas, never silent truncation.

Admission order is: validate transport/request target and method; enforce max_inflight; start request deadline and task; read/validate body framing/content-coding/size; perform route lookup; run handler. Once admission is rejected, do not run a handler. Unknown routes still obey body/time/overload limits. When several ingress faults coexist, the earliest encountered stage determines the response.

No method+path match yields 405 if another configured method matches the decoded path, otherwise 404. 405 includes an Allow header listing the matching methods in ASCII lexicographic order separated by comma+space. There is no implicit HEAD, OPTIONS or CORS handler.

Each accepted request has a distinct task and deadline covering body read, handler, serialization and response send. It shares immutable closures/pool handles, not another task's transaction ownership. Client disconnect aborts its task, joins cleanup, and suppresses a new error response. Host serve cancellation stops accepts, cancels/joins requests and closes the listener.

### 12.3 input and response construction

`input(r,S=json)` first normalizes S, then validates the full exact six-field request shape (method/path strings, params string values, query/headers string-array values with lowercase header names, body Bytes), then checks content coding and Content-Type, then decodes/parses/validates the body. This explicitly overrides general argument-validation order: malformed S precedes request-shape, media and body errors. Caller-constructed request records with that shape are permitted; structural misuse is type/argument. An absent/identity Content-Encoding is accepted; another coding is media, including on constructed records. Server-originated records have already passed ingress coding checks. Accept exactly one Content-Type field value identifying application/json, optionally one charset=utf-8 parameter; ASCII case-insensitive tokens, optional surrounding whitespace and a quoted utf-8 parameter are accepted. Reject other/duplicate parameters or duplicate field occurrences with media. An absent Content-Type is media.

Strictly decode body and parse JSON; translate UTF-8/JSON errors to input with reason utf8/json. Validate using S; translate a data mismatch to input with reason validation and its path/expected/actual. Invalid S remains schema. Return the validated unchanged value. Calling check on unrelated data does not mark it as HTTP input.

`reply(status,v,headers={})` validates status integer 200..599, outbound headers, and v. v must be JSON or Bytes. It creates a nominal Reply with eagerly serialized immutable body Bytes and normalized explicit headers. A value of another kind raises type immediately. A plain record with fields status/body is never interpreted as Reply.

- JSON v uses canonical dump+UTF-8 and defaults Content-Type to application/json. A supplied Content-Type must satisfy the same JSON media rule as post. It performs no compression; supplied Content-Encoding must be identity or absent, otherwise argument.
- Bytes is sent unchanged, defaulting Content-Type to application/octet-stream; a syntactically valid custom content type is allowed.
- Status 204, 205 or 304 requires v=null and produces no body. A user Content-Type is rejected for these bodyless replies; no Content-Type is generated.
- Header/framing restrictions from Section 10.3 apply. Users cannot supply Content-Length or transfer framing.

A handler's normal JSON result is as if reply(200,result) were used. A normal Reply result uses its stored status/headers/body. A bare Bytes result is not an ordinary JSON result and therefore raises type; use reply for raw bytes. Other non-JSON results yield the generic 500 path.

For an explicit HEAD route, invoke the handler normally and validate/serialize its result, then suppress all response body bytes. The runtime MUST never send a HEAD body. It omits Content-Length on 204 and 304 and sends Content-Length:0 on 205. For other HEAD responses it may send the encoded would-be body length. A HEAD request with no HEAD route never invokes GET. Server response compression is off; raw Bytes may represent a caller-supplied encoded body with an appropriate explicit Content-Encoding.

### 12.4 Error responses and finalization

Before response headers are committed, map uncaught request outcomes as follows:

| Outcome | Status | Exact JSON body |
| --- | --- | --- |
| input | 400 | `{"error":"bad_request"}` |
| media | 415 | `{"error":"unsupported_media_type"}` |
| Invalid request target/framing | 400 | `{"error":"bad_request"}` |
| No path | 404 | `{"error":"not_found"}` |
| Wrong method | 405 | `{"error":"method_not_allowed"}` |
| Request body too large | 413 | `{"error":"payload_too_large"}` |
| Admission limit | 503 | `{"error":"unavailable"}` |
| Request deadline | 504 | `{"error":"timeout"}` |
| Other ordinary handler/serialization error | 500 | `{"error":"internal"}` |
| Request work/memory quota abort | 503 | `{"error":"unavailable"}` |

Use application/json and suppress the body for HEAD even on these responses. Messages, exception names, SQL, request data, credentials and stack traces never appear in these client error bodies. Ordinary startup errors are not mapped to imaginary HTTP requests.

Internal ordinary errors mapped to 500 trigger a best-effort structured server log `{event:"request_error",code:<error code>,path:<raw path>}`. Input/media/admission/deadline responses do not require this log. Do not copy query/header/body payload into it. Automatic runtime diagnostics use a separate host diagnostic sink, independent of the source log capability; failure of that sink cannot replace the response/primary outcome. The host may attach a request identifier. User log(v) is explicit, follows its capability and may contain user data.

A request deadline response uses a cleanup-only send budget because the handler deadline has expired; it never resumes user code. If headers/body have already been partially sent, a later error/abort closes the connection/stream and cannot send a second response. Response write failure aborts that request, runs cleanup and does not crash unrelated requests. An ancestor/host cancellation of serve remains an uncatchable abort. A fatal listener failure after startup stops accepts, cancels/joins accepted tasks, closes the listener, then raises ordinary io in the live caller with `{operation:"serve",reason:"listener",outcome:"applied"}`. An ancestor abort during cleanup takes precedence.

## 13. Environment, files, clock, randomness and logs

### 13.1 Local functions

`env(name)` accepts a string and reads only the explicitly exposed host environment map. A disallowed name raises permission before lookup; an allowed but absent name raises env. It does not enumerate or implicitly expose the process environment. Empty configured string values are valid.

`now()` returns a UTC wall-clock timestamp exactly `YYYY-MM-DDTHH:mm:ss.sssZ`, years 0000..9999, zero-padded fields, seconds 00..59. Fractional time is truncated to milliseconds, not rounded. An out-of-range wall-clock value is io. The host clock must supply a representation without leap-second 60; any leap handling is a property of that clock. now is not monotonic.

`uuid()` obtains 16 octets from cryptographically secure host randomness, sets byte 6 to `(byte6 & 15) | 64` and byte 8 to `(byte8 & 63) | 128` (zero-based indices), then formats the 16 bytes in order as lowercase hexadecimal in 8-4-4-4-12 form. These are UUID version 4 and the RFC variant bits.[V] It does not promise collision-free identifiers or reproducible output. Lack of a suitable random source is io; it never substitutes a predictable PRNG silently.

`log(v)` first serializes all of v with dump, then emits the UTF-8 text and one LF as an indivisible line relative to other Kiroha log calls. It returns null. A non-JSON value fails before any log bytes are emitted. Output goes to stderr for the CLI, never its result channel. Explicitly logged secrets are not redacted. Sink failure raises io; automatic internal diagnostics remain best-effort.

### 13.2 Filesystem profile

Web1 paths use POSIX-style slash-separated strings. A nonempty relative path is resolved against the configured filesystem_root; an absolute path remains absolute. Reject NUL and any component exactly `..`. Ignore `.` components and repeated separators for filesystem resolution; this does not change HTTP path rules. A trailing slash requests a directory and is not a regular-file read/write target.

All path components, including the final component, MUST be checked without following symlinks. Symlinks, directories as file targets, devices, FIFOs, sockets and other nonregular targets are io errors. Capabilities apply to canonical component-based paths, not string prefixes. A race-resistant implementation uses directory-relative handles/no-follow operations or an equivalent mechanism. It MUST NOT pass a check and later follow a substituted symlink outside the allowed root.

`read(path)` returns the entire regular file as Bytes. It observes byte/memory/deadline quotas, performs no text decoding and provides no snapshot guarantee against concurrent external modification. A missing file is io(reason=not_found). Exceeding max_file_bytes is a host resource abort, because read has no ordinary byte-limit option.

`write(path,b)` requires Bytes and an existing parent directory. Validate path, body quota and permission before creating anything. It MUST:

1. Inspect the permitted existing regular target, if any, without following symlinks.
2. Create an exclusive unpredictable temporary regular file in the same directory.
3. Write all bytes, set the mode, and close the temporary file.
4. Perform one supported atomic replacement rename.
5. Return null only after confirmed replacement.

Preserve the existing ordinary permission bits masked by 0777 as sampled during inspection. A newly created target uses mode 0600 regardless of process umask; ownership, ACLs, extended attributes and special mode bits are not preserved by this profile. No parent directory is created. If atomic replacement is unsupported, raise io; never fall back to truncate-and-write.

Atomic rename is the externally visible replacement point. Abort observed before dispatch of that replacement must leave the old target unchanged; a replacement already in flight may complete and must use accurate outcome reporting. After replacement, the new file may remain even when the caller is aborted. Remove temporary files on pre-replacement failure/abort within cleanup grace. An unsuccessful cleanup may leave an unreferenced temporary file and must be logged; it is not successful rollback of an uncertain replacement.

Concurrent writers are ordered at replacement points; the last replacement wins. Atomicity does not promise fsync durability across power loss. Successful write returns null; io errors include outcome not_applied, applied or unknown, describing this replacement attempt. Read errors use not_applied because they do not mutate the target.

## 14. Host configuration and adapter

### 14.1 Closed host configuration

The CLI host file is UTF-8 JSON using the strict Kiroha JSON rules. It is host configuration, not a Kiroha value injected into lexical scope. Its root fields are closed:

```json
{
  "format": "kiroha-host/1",
  "filesystem_root": "/absolute/task/root",
  "capabilities": {
    "http": ["https://api.example"],
    "listen": [{"host":"127.0.0.1","port":8080}],
    "databases": ["main"],
    "env": ["TOKEN"],
    "read": ["/absolute/task/root/input"],
    "write": ["/absolute/task/root/output"],
    "clock": true,
    "random": true,
    "log": true
  },
  "environment": {"TOKEN":"example-only"},
  "database_pools": {
    "main": {"dsn_env":"KIROHA_PG_MAIN","max_connections":4}
  },
  "limits": {
    "source_bytes":1048576,
    "memory_bytes":268435456,
    "eval_steps":10000000,
    "max_tasks":1024,
    "max_file_bytes":16777216,
    "header_bytes":32768,
    "deadline_ms":null,
    "cleanup_ms":1000
  }
}
```

The example's values are illustrative inputs, not automatically enabled permissions. format is required and must match exactly. Other top-level fields may be omitted: filesystem_root defaults to the launch working directory; capabilities/environment/database_pools default to empty records; limits default to the table below. All unknown keys are configuration errors, including nested keys.

Capability arrays default to []; booleans default to false. http entries are exact normalized http/https origins, with no wildcard/path/query/userinfo. listen entries are exact host/port pairs. database and env entries are exact case-sensitive aliases/names. read/write entries are absolute permitted **directory** roots, including descendants by path components. Duplicate capability entries are configuration errors.

Environment values are strings; allowing an env name does not require it to exist, so absent names can raise env. Database-pool entries have exactly dsn_env and optional max_connections (default 4, positive integer). dsn_env names a launch-process environment variable read only by the host connector; its value is not automatically exposed to Kiroha env. Unknown/missing pool configuration is reported when db is called. Database credentials, connection TLS policy and native driver connection parameters are encoded in that trusted DSN; they are outside the source language and must not appear in errors.

| Limit | Default | Domain / semantics |
| --- | --- | --- |
| source_bytes | 1048576 | Positive integer; input source bytes |
| memory_bytes | 268435456 | Positive integer; host-managed runtime allocation budget |
| eval_steps | 10000000 | Positive integer; semantic expression/item evaluation budget |
| max_tasks | 1024 | Positive integer; simultaneously live task objects, including parent/request tasks |
| max_file_bytes | 16777216 | Nonnegative integer; each complete read/write payload |
| header_bytes | 32768 | Positive integer; total received HTTP field-section octets |
| deadline_ms | null | null or positive integer; root duration from evaluation start |
| cleanup_ms | 1000 | Integer 1..10000; maximum final cleanup grace per enclosing cleanup scope |

A semantic evaluation step is one entry into a non-Group expression AST node or a Bind/Define item; sequence containers are not extra steps. Use the Section 2.3 semantic AST: erase Group, expand a record shorthand to Name, represent a field key as a Literal string, and evaluate a pipe callee path as Name followed by Get/Literal pairs. Creating an attached Lambda closure charges its Lambda entry. A catch-handler binding/sequence container charges no synthetic Lambda node. Evaluating a function body again charges its nodes again. A compiler/fused engine must preserve this abstract count. Literal/closure allocation and builtin internal loops additionally obey memory/abort checkpoints; they need not pretend to be AST nodes. Source-byte and configuration checks happen before evaluation; their failures are CLI/configuration diagnostics.

The root evaluation and each admitted server request have a fresh eval_steps budget of the configured size. Parallel descendants share their enclosing root/request budget; calls, tx, retries and catches never reset it. Exhaustion aborts that evaluation scope and all its descendants. Request exhaustion maps to 503 while the server and unrelated requests continue; root exhaustion aborts the root. The server's internal accept loop does not evaluate source AST nodes or consume request budgets.

memory_bytes is an aggregate runtime allocation budget. An allocation that would cross it aborts the root/request evaluation scope requesting that allocation, including its descendants; it does not automatically abort unrelated requests. Shared live allocations are counted once under the published accounting strategy. Internal caches must evict/discard rather than exceed the budget. max_tasks is global: failed request admission due to it returns 503, while inability to create a parallel child aborts its enclosing root/request scope as resource. Cleanup retains a bounded emergency reserve and cannot allocate new user values. Exceeding a live quota is an abort, never a catchable ordinary error.

Memory accounting includes runtime-owned values and outstanding decoded buffers; the implementation publishes its allocation-accounting strategy in its conformance report. Exact byte-level allocation overhead is explicitly implementation-dependent. Shared immutable storage, GC timing and backend overhead can affect when a memory quota terminates a large program; within quotas they cannot change values. Conformance tests use ample memory and never treat a host's particular allocation count as a language result.

header_bytes overflow on an HTTP client is a host resource abort. Before server admission, excessive/incomplete header input is rejected by closing the request transport without a handler; there is no promised JSON response to an unreadable header section. Other server application/request limits are as Section 12.

No configured capability grants another one: database permission does not grant HTTP, and a read directory does not grant write. Internal monotonic-clock checks, cleanup and sleep need no external capability. Cleanup may release only already-acquired resources; it cannot use permissions to initiate new user work. Default configuration therefore runs pure programs and sleep but denies external I/O, wall time and randomness.

### 14.2 Adapter interface

The interface below is normative in **behavior**, not source-language ABI. It is implementable with futures, coroutines, threads, callbacks or a VM. No host API name is exposed as an extra Kiroha function.

```text
Context = {
  task_id, parent_task_id, abort_scope,
  monotonic_deadline, capabilities, budgets
}
Outcome<T> = Ok(T) | Error({code,message,data}) | Abort(reason)

Host.monotonic_now() -> monotonic integer nanoseconds
Host.utc_now(ctx) -> wall-clock instant
Host.random_bytes(count,ctx) -> Outcome<Bytes>
Host.environment(name,ctx) -> Outcome<string>
Host.sleep(ms,ctx) -> Outcome<null>
Host.read_regular(path,ctx) -> Outcome<Bytes>
Host.replace_regular(path,Bytes,ctx) -> Outcome<null>
Host.log_line(Bytes,ctx) -> Outcome<null>

Host.http(normalized_request,ctx) -> Outcome<raw_response>
Host.pool(alias,ctx) -> Outcome<pool_identity>
Host.acquire(pool,ctx) -> Outcome<connection_identity>
Host.begin(connection,read_committed,ctx) -> Outcome<null>
Host.execute(connection,sql_text,typed_parameters,ctx) -> Outcome<typed_rows>
Host.commit(connection,ctx) -> Outcome<null>
Host.rollback(connection,cleanup_ctx) -> Outcome<null>
Host.release(connection,reset_or_discard,cleanup_ctx) -> Outcome<null>

Host.listen(endpoint,ctx) -> Outcome<listener_identity>
Host.accept_request(listener,ctx) -> Outcome<raw_request>
Host.send_response(request_id,encoded_response,ctx) -> Outcome<null>
Host.close_request(request_id,cleanup_ctx) -> Outcome<null>
Host.close_listener(listener,cleanup_ctx) -> Outcome<null>
```

The runtime owns parsing/resolution, values, exact decimals, schema logic, preflight/defaults, callback invocation, error translation, route matching, transaction state/ownership, scopes and joins. The host supplies I/O, byte streams, native protocol primitives, pools, clocks and randomness, and enforces permissions/deadlines at dispatch. Either layer may implement buffering/decoding as long as the boundary retains enough raw data to enforce all rules.

HTTP input retains status, ordered raw header pairs, encoded body bytes and transfer/content-coding metadata; a framework must not silently discard duplicate headers or pre-normalize the request target. Database input retains ordered column names, reported OIDs, SQLSTATE, command count and raw exact cell values. An adapter that has already rounded numeric/JSON data is invalid.

Every operation is cancellable/joinable, or the host must isolate it so the runtime can terminate its worker and discard affected resources within cleanup grace. Abort is a disjoint control outcome from Error. An adapter MUST report conservative unknown outcomes for dispatched mutations whose acknowledgment was lost. Parent and operation deadlines are combined, never reset by nesting.

Task/pool/connection/request identities are opaque internal tokens. They cannot be forged by source records. Context snapshots preserve capabilities across closures/children; the user cannot create a privileged context by constructing a record or throwing an error with a special code.

### 14.3 Permitted variation and mandatory invariants

The following are deliberately external observations: network/database results and latency, UTC time, random values, external file contents, DNS/TLS trust configuration, database scheduling, production completion races and allocation overhead. The host configuration and conformance report identify the relevant environment.

No freedom is granted for grammar, lexical lookup, decimal math, JSON representation, callback shape, callback invocation order in sequential functions, option defaults, error phase/code, route selection, transaction ownership/poisoning or cleanup obligations. A host backend's native behavior must be adapted to these rules.

## 15. Command-line and implementation completion contract

### 15.1 Required commands

A complete Web1 implementation ships an executable named kiroha with these commands. SOURCE is a file path; `-` means stdin. The commands accept the options in any order, reject duplicate/unknown options, and use exit 2 for usage/configuration/source-read failure. CLI reading of source/config is a launcher action, not a Kiroha read capability.

| Command | Behavior |
| --- | --- |
| `kiroha --version` | One compact JSON line with implementation, language, profile and protocol versions |
| `kiroha check SOURCE --json` | Lex, parse and resolve only; no program evaluation or effect |
| `kiroha run SOURCE --json [--host HOST.json]` | Evaluate with explicit/default host; one result/error/abort envelope |
| `kiroha spec --compact` | Print exactly the embedded generation-card payload, followed by its existing final LF |
| `kiroha conformance spec.md --json` | Extract and run the embedded conformance manifest with the deterministic fixture host |

The --json flag is mandatory for check/run/conformance in this version; a human-oriented mode is an optional separate extension and cannot change this protocol. `fmt`, a REPL, debugger, package manager and language server are optional future tools, not required or advertised as implemented by this specification.

--version has fields `{"implementation":"<name/version>","language":"0.5.0","profile":"Web1","cli":"kiroha-cli/1","conformance":"kiroha-conformance/1"}`. check success is `{"protocol":"kiroha-cli/1","ok":true,"phase":"check"}`.

run success is `{"protocol":"kiroha-cli/1","ok":true,"result":WireValue}`. An ordinary/runtime or frontend failure is `{"protocol":"kiroha-cli/1","ok":false,"phase":Phase,"error":Error,"span":SpanOrNull}`. Frontend phases are lex/parse/check; runtime is runtime. A source span has byte offsets `{start,end,line,column}` with end exclusive. Span can be null for host-originated runtime failures, not for frontend diagnostics. Static error records use the same outer code/message/data shape.

An abort is `{"protocol":"kiroha-cli/1","ok":false,"phase":"abort","reason":"cancelled|deadline|resource","data":{...}}`, with the reason field holding one listed string, not the literal pipe-separated text. data identifies the exhausted resource or cancellation scope without secrets. Process signals that allow cleanup (SIGINT/SIGTERM) request a cancelled abort; forced OS termination cannot promise an envelope.

Exit statuses: 0 for success/all conformance cases passed; 1 for frontend/runtime failure or a failed conformance case; 2 for CLI/config/file/harness-format failure; 3 for root abort. In JSON modes, an exit-2 error emits the same failure envelope with phase cli, span null and code usage, configuration, source_io, source_limit or harness_format as applicable. Its data is a JSON record; no stack trace/secret is exposed. check uses the default source_bytes limit; run reads/validates host configuration first, then applies its source_bytes limit. Invalid source UTF-8 is a lex failure (exit1), while inability to read the source is source_io (exit2). Outside a recognized JSON mode, usage errors go to stderr and stdout stays empty.

Logs/diagnostics go to stderr. No extra progress text goes to stdout in JSON modes. A run of serve normally stays running until abort; it does not falsely report successful completion immediately after binding a port.

### 15.2 Lossless wire values

The result protocol uses tagged objects so a host JSON parser never loses exact decimals, Bytes, record order or opaque kinds:

```text
null       {"t":"null"}
bool       {"t":"bool","v":true}
number     {"t":"number","v":"0.3"}       // canonical decimal string
string     {"t":"string","v":"hello"}
bytes      {"t":"bytes","base64":"AAE="} // standard padded base64
array      {"t":"array","v":[WireValue,...]}
record     {"t":"record","v":[["key",WireValue],...]}
opaque     {"t":"opaque","kind":"function|schema|query|database|transaction|reply"}
```

For opaque, kind is one actual kind string. The descriptor reveals no closure source, credentials, SQL values or host IDs. Container encoding is recursive, so a record containing a function can be returned through the CLI without becoming a JSON value **inside Kiroha**. Base64 uses the standard alphabet and mandatory padding, with no whitespace.

Error.data is guaranteed JSON. To preserve arbitrary exact numbers there, CLI JSON is emitted by the Kiroha serializer, not a lossy host serializer. Consumers wanting lossless error data must parse exact decimals too.

### 15.3 Implementation stages and acceptance

An implementation agent given only this Markdown should build in this dependency order:

1. Extract the generation card, conformance corpus, measurement corpus and measurement script using their exact marker contracts.
2. Implement tokens/source spans, nested query lexer, parser, syntax restrictions and static symbol resolution. Run frontend cases through check.
3. Implement immutable values, exact decimals, closures, sequences, operators, proper tail calls, errors, schema normalization and all data functions.
4. Implement the task engine, abort scopes, deadlines and the deterministic fixture host. Exercise parallel/retry/pages and event traces.
5. Implement production HTTP, files, environment/time/random/log and PostgreSQL adapters with exact encodings, lifecycle and outcome reporting.
6. Implement server transport, routing, input/reply, ingress limits and shutdown.
7. Package the required CLI; run embedded cases plus the mandatory real integration scenarios below; record versions and outcomes.

No step is permission to invent a new syntax or substitute a stub silently. A partial implementation may report its subset, but must not label itself a complete Web1 implementation. A function that merely raises “not implemented” does not satisfy an API contract.

Completion requires: every embedded case passes; all normative prose edge cases are implemented; integration scenarios pass on the stated environment; the conformance report records no hidden skips; and the generation card/API inventory match the runtime. Finite tests support conformance; they do not prove absence of every implementation bug.

Required integration scenarios, beyond the deterministic corpus:

| ID | Real integration requirement |
| --- | --- |
| I01 | Local HTTP fixture: all methods/status cases, repeated headers, 204/205/304, explicit HEAD, gzip and decoded limits, invalid framing/UTF-8/JSON; no redirects/retries/cookies |
| I02 | TLS fixture: valid chain/hostname succeeds, wrong name/untrusted chain fails without disabling verification; dropped request acknowledgment stays unknown |
| I03 | PostgreSQL 16: every parameter/result OID, huge exact numeric/JSON, NULL, hostile text as data, duplicate names, unsupported types, statement/parser restrictions |
| I04 | PostgreSQL transaction: commit, rollback, caught poison, timeout/cancel, lost COMMIT acknowledgment, task confinement, no dirty pooled reuse; verify actual database state |
| I05 | Server raw HTTP requests: route overlap/precedence, encoded slash, plus/query rules, explicit HEAD/OPTIONS, content type, 413/503/504, disconnect and listener shutdown |
| I06 | File system: mode preservation/new 0600, symlink substitution defense, no parent creation, atomic replacement, failed write/cancel cleanup and unknown rename outcome |
| I07 | Task engine: CPU-bound cancellation, bounded parallelism, joins and cleanup deadlines; no callback/effect initiation after abort observation |
| I08 | CLI: exact tagged numbers/Bytes/opaque containers, stderr isolation, all exit codes, static checking without effects, source/config limits |

The implementation report identifies runtime/compiler/OS/dependency versions, PostgreSQL server version, test command, case IDs, pass/fail/skip states and the reason for each skip. Spec/token measurements are separate from this execution report.

## 16. Embedded conformance protocol

### 16.1 Status and extraction

The corpus below is normative test input, with **no execution result asserted by this document**. A runner validates its complete structure before executing cases. Each case runs in a fresh runtime, lexical prelude, fixture host, virtual clock and resource budget. No case has access to the real network, process environment, filesystem, database or random source.

The manifest is the single JSON fence between standalone lines `<!-- KIROHA_CONFORMANCE_BEGIN -->` and `<!-- KIROHA_CONFORMANCE_END -->`. Match marker **whole lines**, not occurrences inside prose or the embedded extraction script. The generation card, measurement corpus and measurement script use the same extraction rule with their respective marker names. Require exactly one begin and end, correct order, exactly one fence with the declared language and no extra content inside the markers. Preserve the payload's final LF.

Manifest root keys are exactly `format`, `language`, `scheduler`, `cases`. Values are `kiroha-conformance/1`, `0.5.0/Web1`, `virtual-fifo/1`, and a nonempty case array. IDs must be unique.

### 16.2 Case and expectation schema

Each case has required id, source, expect and optional host, fixtures, server, abort_at_ms, abort_after_steps. Unknown keys are harness-format errors. id is a nonempty string; source is exact Kiroha source text. host is an optional partial host configuration without format/filesystem_root/database_pools; permitted fields are capabilities, environment, limits with the Section 14 shapes/defaults. The fixture host supplies virtual pool identities through explicit fixtures, not real DSNs.

expect has phase, plus the corresponding fields:

| phase | Required/allowed expectation |
| --- | --- |
| `value` | Exactly one of `json`, `kind`, `bytes_base64` |
| `lex parse check` | `code`, optional `data` subset |
| `runtime` | `code`, optional `data` subset |
| `abort` | `reason`: cancelled/deadline/resource; optional data subset |

A phase cell listing several words denotes separate permitted enum strings. json is a string containing the **canonical JSON serialization** expected from the result. Compare it byte-for-byte to dump(actual), so numeric exactness and record order are tested without relying on a host JSON-number type. kind is one Section 3 value-kind name and is used when only an opaque category is relevant. bytes_base64 compares exact Bytes using standard padded base64. A non-JSON result cannot satisfy json.

All expectation phases may additionally specify:

- `trace`: exact ordered array of normalized event strings.
- `starts`: map from fixture ID to expected integer virtual start time in ms.
- `max_parallel_active`: expected maximum active callbacks for a tested parallel invocation; cases using it contain one such invocation.
- `responses`: exact server response count/order and per-response id/status/body_base64, with optional headers subset as a lowercase-key record of string arrays, using the incoming-header normalization from Section 10.3.

An absent trace assertion does not permit unexpected effects. Every fixture and effect still follows the rules below. Error data subsets match recursively: object keys are required subsets; arrays/scalars must match exactly, numbers by exact value. Message wording and implementation-specific stack traces are never compared.

A runner report has protocol kiroha-conformance/1, total/passed/failed counts, and an ordered cases array of `{id,status,observed}`. status is pass/fail. observed contains phase/result-or-error/trace and relevant timing/response data. A harness-format error aborts the command with exit 2; a mismatch marks that case failed and continues the other cases. The command exits 0 only if all cases passed.

### 16.3 Deterministic scheduler

Virtual time starts at 0 ms. Ready tasks execute FIFO; initial parallel children enqueue in input order. Each callback/task runs until it suspends, finishes, or exhausts a 1024-semantic-AST-step cooperative quantum, then rejoins the ready queue. Only the Section 14 semantic count advances this fixture quantum; backend digit/container loop checkpoints remain abort checks and do not introduce backend-dependent scheduling. When no task is ready, jump virtual time to the next timer/fixture/request event. At equal timestamps, ancestor abort/deadline events run before ordinary completions; other ties use event registration order. Resume awakened tasks FIFO.

Production runtimes are not required to replicate virtual tie-breaking. This harness removes race ambiguity so a specific implementation can test the invariants.

sleep creates an automatic virtual timer and needs no fixture. Its event IDs are sleep#1, sleep#2, etc., in invocation order; sleep(0) still yields. Retry backoff waits use this same timer/counter/trace, including zero delay. It emits `sleep#n:start` and `sleep#n:ok`, or cancel/joined events. Internal deadline timers do not appear as effect events.

abort_at_ms schedules root cancellation at absolute virtual ms. abort_after_steps requests root cancellation immediately after that many semantic AST evaluation steps; it can test a pure loop without waiting for virtual time to advance. These options are test controls, not source APIs. A root host limits.deadline_ms produces deadline, and eval_steps exhaustion produces resource. CPU-bound real-clock timeout behavior is separately required by I07.

### 16.4 Effect fixtures

A fixture has required `id`, `op`, `match` and exactly one of `ok`, `error`, `hold:true`. It may have after_ms (default0, nonnegative integer) and cancel_ack_ms (default0). A hold fixture has no normal completion. Unknown fields/duplicate IDs are harness-format errors.

At an effect dispatch, permissions and language validation have already occurred. Select the first unconsumed fixture in manifest order with the same op and a recursively matching match subset. Consume it once and record `id:start` and its virtual start time. A normal completion at start+after_ms records `id:ok` or `id:error` and delivers the supplied result/error. Errors are valid ordinary `{code,message,data}` records conforming to the operation contract.

If the scope is aborted before completion, remove the ordinary completion, record `id:cancel`, then after cancel_ack_ms record `id:joined` and finish the aborted effect. A cancellation acknowledgment does not undo a previously simulated external mutation. Cleanup grace can cut short a nonresponding acknowledgment; I07 additionally checks real adapter isolation.

An unmatched dispatched effect, a forbidden real I/O attempt, or an unconsumed fixture at case end is a **harness failure outside Kiroha's catch channel**. Every listed fixture is required to start. This makes “no third request after parallel failure” testable by omitting a third fixture. A cancelled fixture counts as consumed. Host validation errors that prevent dispatch consume no fixture.

For environment lookups, host.environment is the value source of truth and fixtures assert/trace each authorized lookup. Every environment fixture must match a specific name. If the name is present in the map, its fixture must return the identical string; if absent, its fixture must return env with data.name equal to that name. A disagreement is a harness-format error before execution. A forbidden name still fails permission before consuming a fixture. Thus fixtures cannot inject an environment value absent from the map.

The following normalized operation argument/result formats are closed. Metadata numbers here (status/OID/duration) are small exact integers; arbitrary program numbers are text.

| op | matchable arguments | ok value |
| --- | --- | --- |
| http | method, url, headers (lowercase ordered pairs), body_base64 (null if absent) | {status,headers:[name/value pairs],body_base64}; body is transfer-decoded but content-encoded bytes |
| environment | name | string |
| read_regular | path | {bytes_base64} |
| replace_regular | path, bytes_base64 | null |
| log_line | line (full UTF-8 string including LF) | null |
| utc_now | {} | {unix_ms:integer} |
| random_bytes | count | {bytes_base64} of requested length |
| pool | alias | nonempty opaque identity string |
| acquire | pool | nonempty opaque connection identity string |
| begin | connection, isolation:"read_committed" | null |
| execute | connection, sql, parameters | {columns:[{name,oid}],rows:[[cell,...]],count:"integer",outcome} |
| commit | connection | null (confirmed commit) |
| rollback | connection | null (confirmed rollback) |
| release | connection, disposition:"reset" or "discard" | null |

An execute parameter is exactly `{oid,value}`. value is null for SQL NULL, canonical decimal text for numeric, "true"/"false" for bool, decoded string for text, canonical padded base64 for bytea, and canonical JSON text for JSONB. The sql field is the translated SQL text with spaces around generated parameters as Section 11 specifies. An execute result cell is null or PostgreSQL text-format content: decimal text, t/f, raw text/UUID, bytea `\x` hexadecimal, or complete JSON text. count is a nonnegative integer string. outcome is committed for confirmed autocommit or unknown for an active transaction/unresolved result. Duplicate names/unsupported OIDs must be handled by the runtime, not hidden by the fixture host.

An HTTP fixture's bytes already have valid transport framing removed. It can exercise gzip/content/JSON limits; corrupt wire framing belongs in I01. Fixture supplied headers still undergo Kiroha normalization. It never performs real DNS/TLS. Protocol guarantees that require real wire behavior are not established by this mock result. Automatic runtime diagnostic logs are captured separately in the runner's diagnostic report and do not consume log_line fixtures; those fixtures represent source log(v) only.

Pool/acquire/begin/execute/commit/rollback/release are all explicit fixtures, so omitted cleanup/extra COMMIT/automatic replay cannot pass. A production adapter may fuse these internals, but its fixture adapter must expose this normalized lifecycle or an equivalent trace. All fixture identity strings are internal, never Kiroha strings supplied as a handle.

### 16.5 Virtual server requests

A case with server uses one virtual listener. It has exactly requests and cancel_at_ms. Source serve still validates routes/options/capability; on successful listen, the fixture host records `listener:start`. Its accept/send/close operations are automatic for this server object and do not consume ordinary fixtures.

Each request has exactly id, at_ms, method, target, headers and body_base64. Times are nonnegative and relative to listener startup; IDs are unique. These are already-framed transport envelopes, not literal HTTP/1 request bytes: do not invent extra Host/framing fields in the declared header list or require those fields in the mock envelope. At that time deliver its headers/body, record `id:receive`, then follow normal target/admission/routing/task rules. Equal-time requests arrive in array order. A response records `id:response:<status>` with normalized status/header/body data. A response after a disconnect or duplicate response is a harness failure.

cancel_at_ms is relative to listener startup and requests root cancellation; after joins/cleanup, listener closure records `listener:close`. A normally running serve case therefore expects an abort/cancelled result plus its completed responses. If cancellation occurs while a request is pending, its ordinary response must not be invented. Handler effects use ordinary fixtures; sleep uses automatic timers. This virtual protocol tests runtime/router behavior; I05 tests actual HTTP framing and socket behavior.

### 16.6 Acceptance corpus

All following cases are independent. A reference runtime is not included. Extract this JSON verbatim; do not reinterpret examples as optional suggestions.

<!-- KIROHA_CONFORMANCE_BEGIN -->
```json
{
  "format": "kiroha-conformance/1",
  "language": "0.5.0/Web1",
  "scheduler": "virtual-fifo/1",
  "cases": [
    {"id":"lex-leading-zero","source":"01","expect":{"phase":"lex","code":"lexical"}},
    {"id":"lex-leading-dot","source":".5","expect":{"phase":"lex","code":"lexical"}},
    {"id":"lex-trailing-dot","source":"1.","expect":{"phase":"lex","code":"lexical"}},
    {"id":"lex-number-identifier","source":"1abc","expect":{"phase":"lex","code":"lexical"}},
    {"id":"lex-unknown-string-escape","source":"\"\\x41\"","expect":{"phase":"lex","code":"lexical"}},
    {"id":"lex-unpaired-surrogate","source":"\"\\uD800\"","expect":{"phase":"lex","code":"lexical"}},
    {"id":"lex-source-bom","source":"﻿1","expect":{"phase":"lex","code":"lexical"}},
    {"id":"lex-raw-newline-in-string","source":"\"a\nb\"","expect":{"phase":"lex","code":"lexical"}},
    {"id":"parse-trailing-comma","source":"[1,]","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-repeated-semicolon","source":"1;;2","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-parenthesized-single-lambda","source":"{(x)=>x}","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-positional-lambda","source":"[1].map({x=>x})","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-grouped-positional-lambda","source":"[1].map((({x=>x})))","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-trailing-function-name","source":"g={x=>x};[1].map{g}","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-two-callbacks","source":"[1].map{x=>x}{y=>y}","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-comparison-chain","source":"1<2==true","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-bare-pipe-target","source":"[1] | map","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-computed-pipe-target","source":"[1] | (map)(){x=>x}","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-empty-query-hole","source":"q`select ${}`","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-ungrouped-if-operand","source":"1+if true then 2 else 3","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-user-default-parameter","source":"fun f(x=1) x end fun f;f()","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-decoded-duplicate-key","source":"{a:1,\"\\u0061\":2}","expect":{"phase":"parse","code":"duplicate_key"}},
    {"id":"check-unreachable-name","source":"if false then unknown else 1","expect":{"phase":"check","code":"name"}},
    {"id":"check-forward-definition","source":"fun f() g() end fun f fun g() 1 end fun g f()","expect":{"phase":"check","code":"name"}},
    {"id":"check-duplicate-binding","source":"x=1;x=2","expect":{"phase":"check","code":"duplicate_binding"}},
    {"id":"check-duplicate-parameters","source":"fun f(x,x) x end fun f f(1,2)","expect":{"phase":"check","code":"duplicate_binding"}},
    {"id":"check-slot-parameter-collision","source":"fun f(x){x} x end fun f 1","expect":{"phase":"check","code":"duplicate_binding"}},
    {"id":"check-lambda-rebind-parameter","source":"{x=>x=2;x}","expect":{"phase":"check","code":"duplicate_binding"}},
    {"id":"check-self-initializer","source":"x=x","expect":{"phase":"check","code":"name"}},
    {"id":"call-slot-alias-and-reuse","source":"fun apply(x){f} f(x) end fun apply alias=apply;g={y=>y+2};[alias(10){v=>v+1},apply(10){v=>g(v)}]","expect":{"phase":"value","json":"[11,12]"}},
    {"id":"call-required-callback","source":"fun apply(x){f} f(x) end fun apply apply(1)","expect":{"phase":"runtime","code":"arity","data":{"reason":"missing_callback"}}},
    {"id":"call-unexpected-callback","source":"fun f(x) x end fun f f(1){v=>v}","expect":{"phase":"runtime","code":"arity","data":{"reason":"unexpected_callback"}}},
    {"id":"call-positional-function-does-not-fill-slot","source":"fun apply(x){f} f(x) end fun apply g={x=>x};apply(1,g)","expect":{"phase":"runtime","code":"arity","data":{"reason":"positional"}}},
    {"id":"call-empty-map-validates-callback","source":"[].map{(x,y)=>x}","expect":{"phase":"runtime","code":"arity","data":{"reason":"callback_signature"}}},
    {"id":"call-empty-fold-validates-callback","source":"[].fold(0){x=>x}","expect":{"phase":"runtime","code":"arity","data":{"reason":"callback_signature"}}},
    {"id":"call-argument-before-callee-type","source":"1(fail({code:\"from_argument\",message:\"\",data:{}}))","expect":{"phase":"runtime","code":"from_argument"}},
    {"id":"call-function-valued-ordinary-data","source":"fun invoke(f,x) f(x) end fun invoke g={x=>x+1};[invoke(g,2),invoke([{x=>x+2}][0],3),invoke((if true then {x=>x+3} else {x=>x}),4)]","expect":{"phase":"value","json":"[3,5,7]"}},
    {"id":"call-zero-multiple-and-returned-function","source":"z={() =>7};a={(x,y)=>x+y};fun factory(){f} f end fun factory [z(),a(2,3),factory(){x=>x+1}(4)]","expect":{"phase":"value","json":"[7,5,5]"}},
    {"id":"call-callback-body-is-lazy","source":"fun ignore(x){f} x end fun ignore ignore(7){x=>fail({code:\"unused\",message:\"\",data:{}})}","expect":{"phase":"value","json":"7"}},
    {"id":"value-function-kind","source":"{x=>x}","expect":{"phase":"value","kind":"function"}},
    {"id":"scope-lexical-capture-shadow-and-prelude","source":"x=1;fun f() x end fun f [do{x=2;f()},do{y=x;x=9;y},do{fun text(x) x+2 end fun text text(3)},do{x=x+1;x}]","expect":{"phase":"value","json":"[1,1,5,2]"}},
    {"id":"sequence-empty-binding-definition-and-semicolon","source":"[do{},do{1;},do{x=1;},do{fun f() 1 end fun f }]","expect":{"phase":"value","json":"[null,1,null,null]"}},
    {"id":"operators-precedence-and-associativity","source":"[1+2*3,20/5/2,10-3-2,!false&&true||false,null??null??7,1-2,1e-2,(1<2)==true]","expect":{"phase":"value","json":"[7,2,5,true,7,-1,0.01,true]"}},
    {"id":"pipe-dotted-path-and-callback","source":"fun map_items(xs){body} xs.map{x=>body(x)} end fun map_items ns={map:map_items};([1,2,3] | ns.map(){x=>x+1}).sum()","expect":{"phase":"value","json":"9"}},
    {"id":"pipe-left-before-callee-access","source":"ns={};fail({code:\"left_first\",message:\"\",data:{}}) | ns.absent()","expect":{"phase":"runtime","code":"left_first"}},
    {"id":"control-lazy-branches-and-no-truthiness","source":"fun bad() fail({code:\"unused\",message:\"\",data:{}}) end fun bad [if true then 1 else bad(),false&&bad(),true||bad(),false??7,0??7,\"\"??7,(try if 0 then 1 else 2 catch {e=>e.code}),(try 0||true catch {e=>e.code})]","expect":{"phase":"value","json":"[1,false,true,false,0,\"\",\"type\",\"type\"]"}},
    {"id":"catch-handler-does-not-catch-itself","source":"try fail({code:\"first\",message:\"\",data:{}}) catch {e=>fail({code:\"second\",message:\"\",data:{}})}","expect":{"phase":"runtime","code":"second"}},
    {"id":"fail-error-shape-preserved-and-rethrow","source":"try (try fail({code:\"custom\",message:\"detail\",data:{x:3}}) catch {e=>fail(e)}) catch {e=>e}","expect":{"phase":"value","json":"{\"code\":\"custom\",\"message\":\"detail\",\"data\":{\"x\":3}}"}},
    {"id":"tail-recursive-loop","source":"fun go(n,a) if n==0 then a else go(n-1,a+1) end fun go go(100000,0)","expect":{"phase":"value","json":"100000"}},
    {"id":"named-self-recursion","source":"fun fac(n) if n<=1 then 1 else n*fac(n-1) end fun fac fac(10)","expect":{"phase":"value","json":"3628800"}},
    {"id":"evaluation-order-callee-arguments-and-containers","source":"fun bad(c) fail({code:c,message:\"\",data:{}}) end fun bad fun f(a,b) 0 end fun f [(try (bad(\"callee\"))(bad(\"arg\")) catch {e=>e.code}),(try f(bad(\"arg1\"),bad(\"arg2\")) catch {e=>e.code}),(try [bad(\"array1\"),bad(\"array2\")] catch {e=>e.code}),(try {a:bad(\"record1\"),b:bad(\"record2\")} catch {e=>e.code}),(try 1?[bad(\"key\")] catch {e=>e.code})]","expect":{"phase":"value","json":"[\"callee\",\"arg1\",\"array1\",\"record1\",\"key\"]"}},
    {"id":"operators-reject-coercion-and-wrong-kinds","source":"[(try 1+\"x\" catch {e=>e.code}),(try [1]+2 catch {e=>e.code}),(try \"x\"*2 catch {e=>e.code}),(try -true catch {e=>e.code}),(try !0 catch {e=>e.code}),(try []<[] catch {e=>e.code}),(try str==num catch {e=>e.code})]","expect":{"phase":"value","json":"[\"type\",\"type\",\"type\",\"type\",\"type\",\"type\",\"type\"]"}},
    {"id":"decimal-exact-add-multiply-and-zero","source":"[0.1+0.2,9007199254740993+1,1e100*1e-100,-0,1.2300]","expect":{"phase":"value","json":"[0.3,9007199254740994,1,0,1.23]"}},
    {"id":"decimal-canonical-exponent-boundaries","source":"[1e20,1e21,1e-6,1e-7,-1e21]","expect":{"phase":"value","json":"[100000000000000000000,1e21,0.000001,1e-7,-1e21]"}},
    {"id":"decimal-division-34-significant-digits","source":"[1/3,2/3,-1/3,1/8]","expect":{"phase":"value","json":"[0.3333333333333333333333333333333333,0.6666666666666666666666666666666667,-0.3333333333333333333333333333333333,0.125]"}},
    {"id":"decimal-division-ties-even-both-signs","source":"[12345678901234567890123456789012345/1,12345678901234567890123456789012355/1,-12345678901234567890123456789012345/1,-12345678901234567890123456789012355/1]","expect":{"phase":"value","json":"[1.234567890123456789012345678901234e34,1.234567890123456789012345678901236e34,-1.234567890123456789012345678901234e34,-1.234567890123456789012345678901236e34]"}},
    {"id":"decimal-remainder-exact-quotient-and-sign","source":"[5%2,-5%2,5%-2,-5%-2,29999999999999999999999999999999999%3]","expect":{"phase":"value","json":"[1,-1,1,-1,2]"}},
    {"id":"decimal-zero-divisors","source":"[(try 1/0 catch {e=>[e.code,e.data.reason]}),(try 1%0 catch {e=>[e.code,e.data.reason]})]","expect":{"phase":"value","json":"[[\"number\",\"division_by_zero\"],[\"number\",\"division_by_zero\"]]"}},
    {"id":"record-keys-keywords-shorthand-and-no-prototypes","source":"x=9;r={if:1,\"0\":2,\"__proto__\":3,constructor:4,x};[r.if,r[\"0\"],r.__proto__,r.constructor,r.x]","expect":{"phase":"value","json":"[1,2,3,4,9]"}},
    {"id":"optional-access-only-suppresses-absence","source":"[null?[fail({code:\"unused\",message:\"\",data:{}})]??7,{}?.missing,[1]?[4],{x:null}?.x,{x:null}.has(\"x\")]","expect":{"phase":"value","json":"[7,null,null,null,true]"}},
    {"id":"access-missing-key","source":"{x:1}.y","expect":{"phase":"runtime","code":"missing","data":{"key":"y"}}},
    {"id":"access-array-bounds","source":"[1][2]","expect":{"phase":"runtime","code":"bounds","data":{"index":2,"length":1}}},
    {"id":"access-invalid-key-and-container-types","source":"[(try [1]?[-1] catch {e=>e.code}),(try [1][0.5] catch {e=>e.code}),(try [1][\"0\"] catch {e=>e.code}),(try {x:1}[0] catch {e=>e.code}),(try \"a\"[0] catch {e=>e.code}),(try encode(\"a\")[0] catch {e=>e.code})]","expect":{"phase":"value","json":"[\"argument\",\"argument\",\"type\",\"type\",\"type\",\"type\"]"}},
    {"id":"equality-structural-number-and-kind","source":"[{a:1,b:[2]}=={b:[2.0],a:1.0},1==\"1\",1==true,[1,2]==[2,1],null==null,[]!={},\"😀\">\"\",\"a\"<\"aa\"]","expect":{"phase":"value","json":"[true,false,false,false,true,true,true,true]"}},
    {"id":"equality-scans-deep-opaque-after-length-mismatch","source":"[0,{f:{x=>x}}]==[1]","expect":{"phase":"runtime","code":"type"}},
    {"id":"inequality-scans-deep-bytes-after-key-mismatch","source":"{a:encode(\"x\")}!={b:0}","expect":{"phase":"runtime","code":"type"}},
    {"id":"equality-query-versus-number-is-error","source":"q`select 1`==0","expect":{"phase":"runtime","code":"type"}},
    {"id":"query-construction-and-nested-hole-no-effect","source":"q=1;q`select ${do{x=2;{n:x}}}, ${q`nested`} \\${literal}`","expect":{"phase":"value","kind":"query"}},
    {"id":"len-arrays-records-scalars-and-bytes","source":"[[].size,{a:1,b:2}.size,\"😀\".size,\"é\".size,\"é\".size,encode(\"é😀\").size]","expect":{"phase":"value","json":"[0,2,1,2,1,6]"}},
    {"id":"range-directions-exclusive-stop-and-default","source":"[range(0,5,2),range(5,0,-2),range(0,3,-1),range(2,2),range(1,4),(try range(0,3,0) catch {e=>e.code}),(try range(0,3,null) catch {e=>e.code})]","expect":{"phase":"value","json":"[[0,2,4],[5,3,1],[],[],[1,2,3],\"argument\",\"type\"]"}},
    {"id":"map-filter-fold-and-array-concatenation","source":"xs=[1,2,3];[xs.map{x=>{v:x*2}},xs.filter{x=>x>1},xs.fold(10){(a,x)=>a-x},[].fold(7){(a,x)=>a+x},[1]+[2,3]]","expect":{"phase":"value","json":"[[{\"v\":2},{\"v\":4},{\"v\":6}],[2,3],4,7,[1,2,3]]"}},
    {"id":"filter-requires-bool-result","source":"[1].filter{x=>x}","expect":{"phase":"runtime","code":"type"}},
    {"id":"each-empty-and-nonempty-return-null","source":"[[].each{x=>x},[1,2].each{x=>x*2}]","expect":{"phase":"value","json":"[null,null]"}},
    {"id":"each-propagates-first-callback-failure","source":"[1,2].each{x=>fail({code:text(x),message:\"\",data:{}})}","expect":{"phase":"runtime","code":"1"}},
    {"id":"flat-one-level-and-take-clamps","source":"[[[1],[2,[3]]].flat(),[1,2,3].take(2),[1,2].take(5),[1].take(0),(try [1].take(-1) catch {e=>e.code})]","expect":{"phase":"value","json":"[[1,2,[3]],[1,2],[1,2],[],\"argument\"]"}},
    {"id":"flat-rejects-nonarray-member","source":"[[1],2].flat()","expect":{"phase":"runtime","code":"type"}},
    {"id":"sort-stable-callback-and-identity","source":"[[{n:2,id:\"a\"},{n:1,id:\"b\"},{n:2,id:\"c\"}].sort{x=>x.n},[3,1,2].sort(),[\"😀\",\"\",\"a\"].sort(),[].sort()]","expect":{"phase":"value","json":"[[{\"n\":1,\"id\":\"b\"},{\"n\":2,\"id\":\"a\"},{\"n\":2,\"id\":\"c\"}],[1,2,3],[\"a\",\"\",\"😀\"],[]]"}},
    {"id":"sort-rejects-mixed-and-singleton-invalid-keys","source":"[(try [1,\"2\"].sort() catch {e=>e.code}),(try [1].sort{x=>true} catch {e=>e.code})]","expect":{"phase":"value","json":"[\"type\",\"type\"]"}},
    {"id":"group-json-equality-and-first-key-order","source":"[{k:{a:1,b:2},v:\"a\"},{k:{b:2,a:1.0},v:\"b\"},{k:null,v:\"c\"}].group{x=>x.k}","expect":{"phase":"value","json":"[{\"key\":{\"a\":1,\"b\":2},\"items\":[{\"k\":{\"a\":1,\"b\":2},\"v\":\"a\"},{\"k\":{\"b\":2,\"a\":1},\"v\":\"b\"}]},{\"key\":null,\"items\":[{\"k\":null,\"v\":\"c\"}]}]"}},
    {"id":"unique-identity-and-callback-retain-first","source":"[[1,1.00,{a:1,b:2},{b:2,a:1},null,null].unique(),[{k:1,v:\"a\"},{k:1,v:\"b\"},{k:2,v:\"c\"}].unique{x=>x.k}]","expect":{"phase":"value","json":"[[1,{\"a\":1,\"b\":2},null],[{\"k\":1,\"v\":\"a\"},{\"k\":2,\"v\":\"c\"}]]"}},
    {"id":"unique-rejects-nested-opaque-key","source":"[{x:encode(\"a\")}].unique()","expect":{"phase":"runtime","code":"type"}},
    {"id":"collection-callback-failures-follow-input-order","source":"fun bad(x) fail({code:text(x),message:\"\",data:{}}) end fun bad [(try [2,1].map{x=>bad(x)} catch {e=>e.code}),(try [2,1].sort{x=>bad(x)} catch {e=>e.code}),(try [2,1].group{x=>bad(x)} catch {e=>e.code}),(try [2,1].unique{x=>bad(x)} catch {e=>e.code})]","expect":{"phase":"value","json":"[\"2\",\"2\",\"2\",\"2\"]"}},
    {"id":"sum-empty-exact-and-type-validation","source":"[[].sum(),[0.1,0.2,9007199254740993].sum(),(try [1,\"2\"].sum() catch {e=>e.code})]","expect":{"phase":"value","json":"[0,9007199254740993.3,\"type\"]"}},
    {"id":"keys-has-pick-and-shallow-merge-order","source":"r={a:{x:1},b:2}.merge({a:{y:3},c:null});[r.keys(),r.has(\"c\"),r.has(\"z\"),r.pick([\"c\",\"a\"]),r]","expect":{"phase":"value","json":"[[\"a\",\"b\",\"c\"],true,false,{\"c\":null,\"a\":{\"y\":3}},{\"a\":{\"y\":3},\"b\":2,\"c\":null}]"}},
    {"id":"pick-validates-duplicates-before-missing-copy","source":"{a:1}.pick([\"missing\",\"a\",\"a\"])","expect":{"phase":"runtime","code":"argument"}},
    {"id":"pick-missing-is-not-null","source":"{a:null}.pick([\"b\"])","expect":{"phase":"runtime","code":"missing","data":{"key":"b"}}},
    {"id":"one-exact-cardinality","source":"[[4].one(),(try [].one() catch {e=>[e.code,e.data.actual]}),(try [1,2].one() catch {e=>[e.code,e.data.actual]})]","expect":{"phase":"value","json":"[4,[\"cardinality\",0],[\"cardinality\",2]]"}},
    {"id":"split-join-replace-literal-and-empty-parts","source":"[\",a,,\".split(\",\"),\"\".split(\",\"),[\"a\",\"b\"].join(\"\"),[].join(\",\"),\"aaaa\".replace(\"aa\", \"b\"),\"a\".replace(\"a\", \"aa\"),\"$1.$1\".replace(\"$1\", \"x\")]","expect":{"phase":"value","json":"[[\"\",\"a\",\"\",\"\"],[\"\"],\"ab\",\"\",\"bb\",\"aa\",\"x.x\"]"}},
    {"id":"text-operations-domain-and-element-types","source":"[(try \"a\".split(\"\") catch {e=>e.code}),(try \"a\".replace(\"\", \"b\") catch {e=>e.code}),(try [\"a\",1].join(\",\") catch {e=>e.code}),(try text([]) catch {e=>e.code})]","expect":{"phase":"value","json":"[\"argument\",\"argument\",\"type\",\"type\"]"}},
    {"id":"text-scalars-and-string-concatenation","source":"[text(\"x\"),text(1.2300),text(1e21),text(false),text(null),\"a\"+\"b\"]","expect":{"phase":"value","json":"[\"x\",\"1.23\",\"1e21\",\"false\",\"null\",\"ab\"]"}},
    {"id":"number-exact-complete-json-spelling","source":"[number(\"-0\"),number(\"9007199254740993\"),number(\"1.25e2\"),(try number(\" 1\") catch {e=>e.code}),(try number(\"+1\") catch {e=>e.code}),(try number(\"01\") catch {e=>e.code}),(try number(\"NaN\") catch {e=>e.code}),(try number(1) catch {e=>e.code})]","expect":{"phase":"value","json":"[0,9007199254740993,125,\"number\",\"number\",\"number\",\"number\",\"type\"]"}},
    {"id":"parse-exact-json-order-and-supplementary-scalar","source":"parse(\" \\n{\\\"b\\\":9007199254740993,\\\"a\\\":\\\"\\uD83D\\uDE00\\\"}\\t\")","expect":{"phase":"value","json":"{\"b\":9007199254740993,\"a\":\"😀\"}"}},
    {"id":"parse-rejects-duplicates-trailing-bom-and-surrogate","source":"[(try parse(\"{\\\"a\\\":1,\\\"\\\\u0061\\\":2}\") catch {e=>e.data.reason}),(try parse(\"1 2\") catch {e=>e.data.reason}),(try parse(\"\\uFEFF1\") catch {e=>e.data.reason}),(try parse(\"\\\"\\\\uD800\\\"\") catch {e=>e.data.reason})]","expect":{"phase":"value","json":"[\"duplicate_key\",\"trailing\",\"bom\",\"surrogate\"]"}},
    {"id":"parse-rejects-comments-nonfinite-trailing-comma-and-control","source":"[(try parse(\"/*x*/null\") catch {e=>e.code}),(try parse(\"NaN\") catch {e=>e.code}),(try parse(\"[1,]\") catch {e=>e.code}),(try parse(\"\\\"a\\nb\\\"\") catch {e=>e.code}),(try parse(1) catch {e=>e.code})]","expect":{"phase":"value","json":"[\"json\",\"json\",\"json\",\"json\",\"type\"]"}},
    {"id":"dump-canonical-escaping-and-roundtrip","source":"v={z:\"/é\\u2028\\u2029\\u0000\\b\\f\\n\\r\\t\\\"\\\\\",n:1.2300};[dump(v),parse(dump(v))==v]","expect":{"phase":"value","json":"[\"{\\\"z\\\":\\\"/é  \\\\u0000\\\\b\\\\f\\\\n\\\\r\\\\t\\\\\\\"\\\\\\\\\\\",\\\"n\\\":1.23}\",true]"}},
    {"id":"dump-rejects-deep-function-value","source":"dump({a:[1,{f:{x=>x}}]})","expect":{"phase":"runtime","code":"type"}},
    {"id":"encode-exact-utf8-octets","source":"encode(\"é😀\")","expect":{"phase":"value","bytes_base64":"w6nwn5iA"}},
    {"id":"decode-roundtrip-preserves-bom","source":"decode(encode(\"﻿é😀\"))","expect":{"phase":"value","json":"\"﻿é😀\""}},
    {"id":"decode-rejects-nonbytes","source":"decode([65])","expect":{"phase":"runtime","code":"type"}},
    {"id":"schema-all-five-atoms","source":"[check(\"s\",str),check(1.25,num),check(1.0,int),check(false,bool),check([null,{x:1}],json)]","expect":{"phase":"value","json":"[\"s\",1.25,1,false,[null,{\"x\":1}]]"}},
    {"id":"schema-optional-nullable-open-preserves-data-order","source":"S=open({id:int,label:optional(maybe(str))});[check({extra:2,id:1},S),check({id:2,label:null},S),check([], [str]),check({}, {})]","expect":{"phase":"value","json":"[{\"extra\":2,\"id\":1},{\"id\":2,\"label\":null},[],{}]"}},
    {"id":"schema-nested-validation-path","source":"check({users:[{id:1},{id:\"x\"}]},{users:[{id:int}]})","expect":{"phase":"runtime","code":"validation","data":{"path":["users",1,"id"],"expected":"int","actual":"string"}}},
    {"id":"schema-known-required-before-extra","source":"check({extra:1,b:\"bad\"},{a:int,b:int})","expect":{"phase":"runtime","code":"validation","data":{"path":["a"],"expected":"int","actual":"missing"}}},
    {"id":"schema-closed-extra-field","source":"check({a:1,b:2},{a:int})","expect":{"phase":"runtime","code":"validation","data":{"path":["b"],"expected":"absent","actual":"number"}}},
    {"id":"schema-optional-is-not-nullable","source":"check({x:null},{x:optional(str)})","expect":{"phase":"runtime","code":"validation","data":{"path":["x"],"expected":"str","actual":"null"}}},
    {"id":"schema-invalid-description-before-data-mismatch","source":"check(null,{a:[]})","expect":{"phase":"runtime","code":"schema","data":{"path":["a"],"reason":"array_arity"}}},
    {"id":"schema-malformed-descriptions-and-wrapper-placement","source":"[(try check(null,[]) catch {e=>e.code}),(try check(null,[str,num]) catch {e=>e.code}),(try check(null,true) catch {e=>e.code}),(try check(null,optional(str)) catch {e=>e.code}),(try check([], [optional(str)]) catch {e=>e.code}),(try maybe(optional(str)) catch {e=>e.code}),(try optional(optional(str)) catch {e=>e.code}),(try open(int) catch {e=>e.code})]","expect":{"phase":"value","json":"[\"schema\",\"schema\",\"schema\",\"schema\",\"schema\",\"schema\",\"schema\",\"schema\"]"}},
    {"id":"schema-wrapper-idempotence-and-nullable-result","source":"[check(null,maybe(maybe(str))),check({x:1,y:2},open(open({x:int}))),(try check(1.5,int) catch {e=>[e.code,e.data.expected,e.data.actual]})]","expect":{"phase":"value","json":"[null,{\"x\":1,\"y\":2},[\"validation\",\"int\",\"number\"]]"}},
    {"id":"schema-open-extra-must-still-be-json","source":"check({a:1,extra:encode(\"x\")},open({a:int}))","expect":{"phase":"runtime","code":"validation"}},
    {"id":"value-schema-wrapper-kind","source":"optional(str)","expect":{"phase":"value","kind":"schema"}},
    {"id":"host-parallel-input-order","source":"[1,2,3].parallel(2){i=>get(url(\"https://fixture.invalid\",[text(i)]))}","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[{"id":"p1","op":"http","match":{"method":"GET","url":"https://fixture.invalid/1"},"ok":{"status":200,"headers":[],"body_base64":"MQ=="},"after_ms":30},{"id":"p2","op":"http","match":{"method":"GET","url":"https://fixture.invalid/2"},"ok":{"status":200,"headers":[],"body_base64":"Mg=="},"after_ms":10},{"id":"p3","op":"http","match":{"method":"GET","url":"https://fixture.invalid/3"},"ok":{"status":200,"headers":[],"body_base64":"Mw=="},"after_ms":5}],"expect":{"phase":"value","json":"[1,2,3]","trace":["p1:start","p2:start","p2:ok","p3:start","p3:ok","p1:ok"],"starts":{"p1":0,"p2":0,"p3":10},"max_parallel_active":2}},
    {"id":"host-parallel-failure-cancels-joins-no-third-start","source":"[1,2,3].parallel(2){i=>try get(url(\"https://fixture.invalid\",[text(i)])) catch {e=>if i==1 then fail({code:\"caught_abort\",message:\"\",data:{}}) else fail(e)}}","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[{"id":"p1","op":"http","match":{"method":"GET","url":"https://fixture.invalid/1"},"hold":true,"cancel_ack_ms":2},{"id":"p2","op":"http","match":{"method":"GET","url":"https://fixture.invalid/2"},"error":{"code":"network","message":"fixture diagnostic","data":{"operation":"http","reason":"second_failed","outcome":"unknown"}},"after_ms":10}],"expect":{"phase":"runtime","code":"network","trace":["p1:start","p2:start","p2:error","p1:cancel","p1:joined"],"starts":{"p1":0,"p2":0},"max_parallel_active":2,"data":{"reason":"second_failed","outcome":"unknown"}}},
    {"id":"host-retry-backoff-and-no-final-predicate","source":"retry({attempts:3,when:{e=>if e.data.attempt==3 then fail({code:\"predicate_after_final\",message:\"\",data:{}}) else true},delay:10}){()=>get(\"https://fixture.invalid/retry\")}","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[{"id":"a1","op":"http","match":{"method":"GET","url":"https://fixture.invalid/retry"},"error":{"code":"network","message":"fixture diagnostic","data":{"operation":"http","reason":"fixture","outcome":"unknown","attempt":1}}},{"id":"a2","op":"http","match":{"method":"GET","url":"https://fixture.invalid/retry"},"error":{"code":"network","message":"fixture diagnostic","data":{"operation":"http","reason":"fixture","outcome":"unknown","attempt":2}}},{"id":"a3","op":"http","match":{"method":"GET","url":"https://fixture.invalid/retry"},"error":{"code":"network","message":"fixture diagnostic","data":{"operation":"http","reason":"fixture","outcome":"unknown","attempt":3}}}],"expect":{"phase":"runtime","code":"network","trace":["a1:start","a1:error","sleep#1:start","sleep#1:ok","a2:start","a2:error","sleep#2:start","sleep#2:ok","a3:start","a3:error"],"starts":{"a1":0,"a2":10,"a3":30},"data":{"attempt":3,"outcome":"unknown"}}},
    {"id":"host-retry-success-zero-backoff","source":"retry({attempts:2,when:{e=>e.code==\"network\"},delay:0}){()=>get(\"https://fixture.invalid/retry\")}","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[{"id":"a1","op":"http","match":{"method":"GET","url":"https://fixture.invalid/retry"},"error":{"code":"network","message":"fixture diagnostic","data":{"operation":"http","reason":"retry_once","outcome":"unknown"}}},{"id":"a2","op":"http","match":{"method":"GET","url":"https://fixture.invalid/retry"},"ok":{"status":200,"headers":[],"body_base64":"eyJvayI6dHJ1ZX0="}}],"expect":{"phase":"value","json":"{\"ok\":true}","trace":["a1:start","a1:error","sleep#1:start","sleep#1:ok","a2:start","a2:ok"],"starts":{"a1":0,"a2":0}}},
    {"id":"host-pages-false-zero-empty-and-terminal-exact-limits","source":"pages(null,{max_pages:3,max_items:2}){c=>if c==null then {items:[1],next:0} else if c==0 then {items:[],next:false} else {items:[2],next:null}}","expect":{"phase":"value","json":"[1,2]","trace":[]}},
    {"id":"host-pages-cycle-and-limit-priority","source":"[(try pages({a:1,b:2},{max_pages:1}){c=>{items:[],next:{b:2,a:1}}} catch {e=>[e.code,e.data.reason,e.data.pages,e.data.items]}),(try pages(null,{max_pages:1}){c=>{items:[1],next:0}} catch {e=>[e.code,e.data.reason,e.data.pages,e.data.items]}),(try pages(\"x\",{max_pages:1,max_items:0}){c=>{items:[1],next:\"x\"}} catch {e=>[e.code,e.data.reason,e.data.pages,e.data.items]})]","expect":{"phase":"value","json":"[[\"pagination\",\"repeated_cursor\",1,0],[\"pagination\",\"max_pages\",1,1],[\"pagination\",\"max_items\",1,1]]","trace":[]}},
    {"id":"host-http-status-before-json-and-empty-success","source":"r=http(\"GET\",\"https://fixture.invalid/raw\");[r.status,r.body.size,(try get(\"https://fixture.invalid/rejected\") catch {e=>[e.code,e.data.status]}),(try get(\"https://fixture.invalid/empty\") catch {e=>e.code}),post(\"https://fixture.invalid/create\",{x:1})]","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[{"id":"raw","op":"http","match":{"method":"GET","url":"https://fixture.invalid/raw"},"ok":{"status":404,"headers":[],"body_base64":"eA=="}},{"id":"rejected","op":"http","match":{"method":"GET","url":"https://fixture.invalid/rejected"},"ok":{"status":404,"headers":[],"body_base64":"bm90IGpzb24="}},{"id":"empty","op":"http","match":{"method":"GET","url":"https://fixture.invalid/empty"},"ok":{"status":204,"headers":[["Content-Encoding","gzip"]],"body_base64":""}},{"id":"created","op":"http","match":{"method":"POST","url":"https://fixture.invalid/create","body_base64":"eyJ4IjoxfQ=="},"ok":{"status":201,"headers":[],"body_base64":"eyJpZCI6Mn0="}}],"expect":{"phase":"value","json":"[404,1,[\"http\",404],\"json\",{\"id\":2}]","trace":["raw:start","raw:ok","rejected:start","rejected:ok","empty:start","empty:ok","created:start","created:ok"]}},
    {"id":"host-http-gzip-wire-headers-and-decoded-byte-limit","source":"r=http(\"GET\",\"https://fixture.invalid/gzip\",{max_bytes:7});[{status:r.status,headers:r.headers,value:parse(decode(r.body))},(try get(\"https://fixture.invalid/over\",{max_bytes:6}) catch {e=>e.code}),(try get(\"https://fixture.invalid/bad-gzip\") catch {e=>[e.code,e.data.reason]})]","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[{"id":"gzip","op":"http","match":{"method":"GET","url":"https://fixture.invalid/gzip"},"ok":{"status":200,"headers":[["Content-Encoding","GZip"],["X-V"," a "],["x-v","b"],["Content-Length","27"]],"body_base64":"H4sIAAAAAAACA6tWqlCyMqwFAFxZ6zsHAAAA"}},{"id":"over","op":"http","match":{"method":"GET","url":"https://fixture.invalid/over"},"ok":{"status":200,"headers":[["Content-Encoding","gzip"]],"body_base64":"H4sIAAAAAAACA6tWqlCyMqwFAFxZ6zsHAAAA"}},{"id":"badgzip","op":"http","match":{"method":"GET","url":"https://fixture.invalid/bad-gzip"},"ok":{"status":404,"headers":[["Content-Encoding","gzip"]],"body_base64":"bm90IGd6aXA="}}],"expect":{"phase":"value","json":"[{\"status\":200,\"headers\":{\"content-encoding\":[\"GZip\"],\"x-v\":[\"a\",\"b\"],\"content-length\":[\"27\"]},\"value\":{\"x\":1}},\"limit\",[\"network\",\"decode\"]]","trace":["gzip:start","gzip:ok","over:start","over:ok","badgzip:start","badgzip:ok"]}},
    {"id":"host-http-head-and-204-skip-content-coding","source":"a=http(\"HEAD\",\"https://fixture.invalid/head\",{max_bytes:0});b=http(\"GET\",\"https://fixture.invalid/bodyless\",{max_bytes:0});[a.status,a.body.size,b.status,b.body.size]","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[{"id":"head","op":"http","match":{"method":"HEAD","url":"https://fixture.invalid/head"},"ok":{"status":200,"headers":[["Content-Encoding","br"],["Content-Length","999999"]],"body_base64":""}},{"id":"empty","op":"http","match":{"method":"GET","url":"https://fixture.invalid/bodyless"},"ok":{"status":204,"headers":[["Content-Encoding","unsupported"]],"body_base64":""}}],"expect":{"phase":"value","json":"[200,0,204,0]","trace":["head:start","head:ok","empty:start","empty:ok"]}},
    {"id":"host-http-invalid-preflight-never-dispatches","source":"[(try http(\"GET\",\"https://fixture.invalid/\",{headers:{\"X-A\":\"1\",\"x-a\":\"2\"}}) catch {e=>e.code}),(try http(\"GET\",\"https://fixture.invalid/\",{headers:{X:\"a\\r\\nb\"}}) catch {e=>e.code}),(try http(\"GET\",\"https://fixture.invalid/\",{headers:{Host:\"elsewhere\"}}) catch {e=>e.code}),(try http(\"GET\",\"https://fixture.invalid/\",{body:encode(\"\")}) catch {e=>e.code}),(try post(\"https://fixture.invalid/\",null,{headers:{\"Content-Type\":\"text/plain\"}}) catch {e=>e.code}),(try post(\"https://fixture.invalid/\",null,{headers:{\"Content-Encoding\":\"gzip\"}}) catch {e=>e.code}),(try get(\"https://fixture.invalid/\",{unknown:1}) catch {e=>e.code})]","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[],"expect":{"phase":"value","json":"[\"argument\",\"argument\",\"argument\",\"argument\",\"argument\",\"argument\",\"argument\"]","trace":[]}},
    {"id":"host-url-component-encoding-and-origin-preservation","source":" [url(\"HTTPS://EXAMPLE.TEST/a//?old=%2f\",[\"\",\"a/b\",\"+\",\"%\"],{q:[\"a b\",null,false],n:1e21}),url(\"https://example.test\",[\"é\"]),(try url(\"https://example.test\",[\"..\"]) catch {e=>e.code}),(try url(\"https://user@example.test\",[]) catch {e=>e.code}),(try url(\"https://example.test/#frag\",[]) catch {e=>e.code})]","expect":{"phase":"value","json":"[\"https://example.test/a///a%2Fb/%2B/%25?old=%2f&q=a%20b&q=false&n=1e21\",\"https://example.test/%C3%A9\",\"argument\",\"argument\",\"argument\"]","trace":[]}},
    {"id":"host-read-malformed-utf8-and-input-translation","source":"b=read(\"/fixtures/input\");[(try decode(b) catch {e=>[e.code,e.data.offset]}),(try input({method:\"POST\",path:\"/\",params:{},query:{},headers:{\"content-type\":[\"application/json\"]},body:b}) catch {e=>[e.code,e.data.reason]})]","host":{"capabilities":{"read":["/fixtures"]}},"fixtures":[{"id":"read","op":"read_regular","match":{"path":"/fixtures/input"},"ok":{"bytes_base64":"wyg="}}],"expect":{"phase":"value","json":"[[\"utf8\",0],[\"input\",\"utf8\"]]","trace":["read:start","read:ok"]}},
    {"id":"host-environment-clock-random-and-atomic-log-line","source":"v={token:env(\"TOKEN\"),at:now(),id:uuid()};log(v);v.merge({missing:try env(\"MISSING\") catch {e=>e.code}})","host":{"capabilities":{"env":["TOKEN","MISSING"],"clock":true,"random":true,"log":true},"environment":{"TOKEN":"demo"}},"fixtures":[{"id":"env","op":"environment","match":{"name":"TOKEN"},"ok":"demo"},{"id":"clock","op":"utc_now","match":{},"ok":{"unix_ms":123}},{"id":"random","op":"random_bytes","match":{"count":16},"ok":{"bytes_base64":"AAAAAAAAAAAAAAAAAAAAAA=="}},{"id":"log","op":"log_line","match":{"line":"{\"token\":\"demo\",\"at\":\"1970-01-01T00:00:00.123Z\",\"id\":\"00000000-0000-4000-8000-000000000000\"}\n"},"ok":null},{"id":"missing","op":"environment","match":{"name":"MISSING"},"error":{"code":"env","message":"fixture diagnostic","data":{"name":"MISSING"}}}],"expect":{"phase":"value","json":"{\"token\":\"demo\",\"at\":\"1970-01-01T00:00:00.123Z\",\"id\":\"00000000-0000-4000-8000-000000000000\",\"missing\":\"env\"}","trace":["env:start","env:ok","clock:start","clock:ok","random:start","random:ok","log:start","log:ok","missing:start","missing:error"]}},
    {"id":"host-file-replacement-and-normalized-read-path","source":"write(\"/fixtures/./out\",encode(\"é\"));decode(read(\"/fixtures//out\"))","host":{"capabilities":{"write":["/fixtures"],"read":["/fixtures"]}},"fixtures":[{"id":"write","op":"replace_regular","match":{"path":"/fixtures/out","bytes_base64":"w6k="},"ok":null},{"id":"read","op":"read_regular","match":{"path":"/fixtures/out"},"ok":{"bytes_base64":"w6k="}}],"expect":{"phase":"value","json":"\"é\"","trace":["write:start","write:ok","read:start","read:ok"]}},
    {"id":"host-default-denies-every-external-capability","source":"[(try env(\"TOKEN\") catch {e=>e.code}),(try read(\"/fixtures/x\") catch {e=>e.code}),(try write(\"/fixtures/x\",encode(\"x\")) catch {e=>e.code}),(try now() catch {e=>e.code}),(try uuid() catch {e=>e.code}),(try log(null) catch {e=>e.code}),(try db(\"main\") catch {e=>e.code}),(try http(\"GET\",\"https://fixture.invalid/\") catch {e=>e.code}),(try serve({}) catch {e=>e.code})]","fixtures":[],"expect":{"phase":"value","json":"[\"permission\",\"permission\",\"permission\",\"permission\",\"permission\",\"permission\",\"permission\",\"permission\",\"permission\"]","trace":[]}},
    {"id":"host-sql-all-parameter-kinds-and-exact-result-mapping","source":"d=db(\"main\");r=sql(d,q`select ${null}::text as z, ${true}::boolean as b, ${\"x'); drop table t; --\"}::text as s, ${9007199254740993.25}::numeric as n, ${encode(\"é\")}::bytea as raw, ${{a:1}}::jsonb as obj, ${[1,2]}::jsonb as arr`);x=r.rows.one();{count:r.count,row:{z:x.z,b:x.b,s:x.s,n:x.n,raw:decode(x.raw),obj:x.obj,arr:x.arr}}","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"},{"id":"acquire","op":"acquire","match":{"pool":"pool-main"},"ok":"conn-1"},{"id":"query","op":"execute","match":{"connection":"conn-1","sql":"select  $1 ::text as z,  $2 ::boolean as b,  $3 ::text as s,  $4 ::numeric as n,  $5 ::bytea as raw,  $6 ::jsonb as obj,  $7 ::jsonb as arr","parameters":[{"oid":0,"value":null},{"oid":16,"value":"true"},{"oid":25,"value":"x'); drop table t; --"},{"oid":1700,"value":"9007199254740993.25"},{"oid":17,"value":"w6k="},{"oid":3802,"value":"{\"a\":1}"},{"oid":3802,"value":"[1,2]"}]},"ok":{"columns":[{"name":"z","oid":25},{"name":"b","oid":16},{"name":"s","oid":25},{"name":"n","oid":1700},{"name":"raw","oid":17},{"name":"obj","oid":3802},{"name":"arr","oid":3802}],"rows":[[null,"t","x'); drop table t; --","9007199254740993.25","\\xc3a9","{\"a\":1}","[1,2]"]],"count":"1","outcome":"committed"}},{"id":"release","op":"release","match":{"connection":"conn-1","disposition":"reset"},"ok":null}],"expect":{"phase":"value","json":"{\"count\":1,\"row\":{\"z\":null,\"b\":true,\"s\":\"x'); drop table t; --\",\"n\":9007199254740993.25,\"raw\":\"é\",\"obj\":{\"a\":1},\"arr\":[1,2]}}","trace":["pool:start","pool:ok","acquire:start","acquire:ok","query:start","query:ok","release:start","release:ok"]}},
    {"id":"host-sql-preflight-before-acquire","source":"d=db(\"main\");[(try sql(d,\"select 1\") catch {e=>e.code}),(try sql(d,q`select 1;select 2`) catch {e=>[e.code,e.data.phase,e.data.reason]}),(try sql(d,q`select $1`) catch {e=>[e.code,e.data.phase,e.data.reason]}),(try sql(d,q`select '${\"x\"}'`) catch {e=>[e.code,e.data.phase,e.data.reason]}),(try sql(d,q`select ${{f:{x=>x}}}`) catch {e=>[e.code,e.data.phase,e.data.outcome]})]","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"}],"expect":{"phase":"value","json":"[\"type\",[\"db\",\"prepare\",\"multiple_statements\"],[\"db\",\"prepare\",\"literal_parameter\"],[\"db\",\"prepare\",\"hole_context\"],[\"db\",\"bind\",\"not_sent\"]]","trace":["pool:start","pool:ok"]}},
    {"id":"host-sql-zero-row-unsupported-metadata-still-fails","source":"sql(db(\"main\"),q`select current_date where false`)","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"},{"id":"acquire","op":"acquire","match":{"pool":"pool-main"},"ok":"conn-1"},{"id":"query","op":"execute","match":{"connection":"conn-1","sql":"select current_date where false","parameters":[]},"ok":{"columns":[{"name":"current_date","oid":1082}],"rows":[],"count":"0","outcome":"committed"}},{"id":"release","op":"release","match":{"connection":"conn-1","disposition":"reset"},"ok":null}],"expect":{"phase":"runtime","code":"db","trace":["pool:start","pool:ok","acquire:start","acquire:ok","query:start","query:ok","release:start","release:ok"],"data":{"phase":"decode","outcome":"committed"}}},
    {"id":"host-sql-row-limit-after-autocommit-is-not-rollback","source":"sql(db(\"main\"),q`insert into t(v) values (1) returning id`,{max_rows:0})","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"},{"id":"acquire","op":"acquire","match":{"pool":"pool-main"},"ok":"conn-1"},{"id":"query","op":"execute","match":{"connection":"conn-1","sql":"insert into t(v) values (1) returning id","parameters":[]},"ok":{"columns":[{"name":"id","oid":23}],"rows":[["7"]],"count":"1","outcome":"committed"}},{"id":"release","op":"release","match":{"connection":"conn-1","disposition":"reset"},"ok":null}],"expect":{"phase":"runtime","code":"limit","trace":["pool:start","pool:ok","acquire:start","acquire:ok","query:start","query:ok","release:start","release:ok"],"data":{"operation":"sql","limit":"max_rows","maximum":0,"outcome":"committed"}}},
    {"id":"host-tx-caught-poison-stays-primary-and-rolls-back","source":"d=db(\"main\");tx(d){t=>try sql(t,q`select 1`) catch {e=>null};try sql(t,q`select 2`) catch {e=>null};fail({code:\"later_callback\",message:\"\",data:{}})}","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"},{"id":"acquire","op":"acquire","match":{"pool":"pool-main"},"ok":"conn-1"},{"id":"begin","op":"begin","match":{"connection":"conn-1","isolation":"read_committed"},"ok":null},{"id":"query","op":"execute","match":{"connection":"conn-1","sql":"select 1","parameters":[]},"error":{"code":"db","message":"fixture diagnostic","data":{"phase":"execute","outcome":"unknown","sqlstate":"23505"}}},{"id":"rollback","op":"rollback","match":{"connection":"conn-1"},"ok":null},{"id":"release","op":"release","match":{"connection":"conn-1","disposition":"reset"},"ok":null}],"expect":{"phase":"runtime","code":"db","trace":["pool:start","pool:ok","acquire:start","acquire:ok","begin:start","begin:ok","query:start","query:error","rollback:start","rollback:ok","release:start","release:ok"],"data":{"phase":"execute","outcome":"unknown","sqlstate":"23505"}}},
    {"id":"host-tx-unknown-commit-discards-without-rollback-or-replay","source":"tx(db(\"main\")){t=>7}","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"},{"id":"acquire","op":"acquire","match":{"pool":"pool-main"},"ok":"conn-1"},{"id":"begin","op":"begin","match":{"connection":"conn-1","isolation":"read_committed"},"ok":null},{"id":"commit","op":"commit","match":{"connection":"conn-1"},"error":{"code":"db","message":"fixture diagnostic","data":{"phase":"commit","outcome":"unknown"}}},{"id":"release","op":"release","match":{"connection":"conn-1","disposition":"discard"},"ok":null}],"expect":{"phase":"runtime","code":"db","trace":["pool:start","pool:ok","acquire:start","acquire:ok","begin:start","begin:ok","commit:start","commit:error","release:start","release:ok"],"data":{"phase":"commit","outcome":"unknown"}}},
    {"id":"host-tx-child-task-rejection-does-not-poison-owner","source":"tx(db(\"main\")){t=>[0].parallel(1){x=>try sql(t,q`select 1`) catch {e=>[e.code,e.data.phase,e.data.outcome]}}}","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"},{"id":"acquire","op":"acquire","match":{"pool":"pool-main"},"ok":"conn-1"},{"id":"begin","op":"begin","match":{"connection":"conn-1","isolation":"read_committed"},"ok":null},{"id":"commit","op":"commit","match":{"connection":"conn-1"},"ok":null},{"id":"release","op":"release","match":{"connection":"conn-1","disposition":"reset"},"ok":null}],"expect":{"phase":"value","json":"[[\"db\",\"ownership\",\"not_sent\"]]","trace":["pool:start","pool:ok","acquire:start","acquire:ok","begin:start","begin:ok","commit:start","commit:ok","release:start","release:ok"],"max_parallel_active":1}},
    {"id":"host-tx-returned-handle-has-expired","source":"t=tx(db(\"main\")){t=>t};sql(t,q`select 1`)","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"},{"id":"acquire","op":"acquire","match":{"pool":"pool-main"},"ok":"conn-1"},{"id":"begin","op":"begin","match":{"connection":"conn-1","isolation":"read_committed"},"ok":null},{"id":"commit","op":"commit","match":{"connection":"conn-1"},"ok":null},{"id":"release","op":"release","match":{"connection":"conn-1","disposition":"reset"},"ok":null}],"expect":{"phase":"runtime","code":"db","trace":["pool:start","pool:ok","acquire:start","acquire:ok","begin:start","begin:ok","commit:start","commit:ok","release:start","release:ok"],"data":{"phase":"ownership","outcome":"not_sent"}}},
    {"id":"host-tx-timeout-stops-callback-and-cleans-up","source":"try tx(db(\"main\"),{timeout:5}){t=>sleep(100);7} catch {e=>[e.code,e.data.operation,e.data.timeout]}","host":{"capabilities":{"databases":["main"]}},"fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"pool-main"},{"id":"acquire","op":"acquire","match":{"pool":"pool-main"},"ok":"conn-1"},{"id":"begin","op":"begin","match":{"connection":"conn-1","isolation":"read_committed"},"ok":null},{"id":"rollback","op":"rollback","match":{"connection":"conn-1"},"ok":null},{"id":"release","op":"release","match":{"connection":"conn-1","disposition":"reset"},"ok":null}],"expect":{"phase":"value","json":"[\"timeout\",\"tx\",5]","trace":["pool:start","pool:ok","acquire:start","acquire:ok","begin:start","begin:ok","sleep#1:start","sleep#1:cancel","sleep#1:joined","rollback:start","rollback:ok","release:start","release:ok"],"starts":{"pool":0,"acquire":0,"begin":0,"rollback":5,"release":5}}},
    {"id":"host-serve-ambiguity-rejected-before-listen","source":"serve({\"GET /a/:x\":{r=>1},\"GET /:y/b\":{r=>2}})","host":{"capabilities":{"listen":[{"host":"127.0.0.1","port":8080}]}},"fixtures":[],"expect":{"phase":"runtime","code":"argument","trace":[],"data":{"operation":"serve"}}},
    {"id":"host-input-constructed-record-and-error-precedence","source":"fun r(s,h) {method:\"POST\",path:\"/\",params:{},query:{},headers:h,body:encode(s)} end fun r [input(r(\"{\\\"id\\\":1}\",{\"content-type\":[\"Application/JSON; charset=\\\"utf-8\\\"\"]}),{id:int}),(try input(r(\"x\",{\"content-type\":[\"application/json\"]})) catch {e=>[e.code,e.data.reason]}),(try input(r(\"{\\\"id\\\":\\\"bad\\\"}\",{\"content-type\":[\"application/json\"]}),{id:int}) catch {e=>[e.code,e.data.reason,e.data.path]}),(try input(r(\"null\",{})) catch {e=>e.code}),(try input(r(\"null\",{\"content-type\":[\"application/json\",\"application/json\"]})) catch {e=>e.code}),(try input(r(\"null\",{\"content-type\":[\"application/json\"],\"content-encoding\":[\"gzip\"]})) catch {e=>e.code}),(try input({},[]) catch {e=>e.code})]","expect":{"phase":"value","json":"[{\"id\":1},[\"input\",\"json\"],[\"input\",\"validation\",[\"id\"]],\"media\",\"media\",\"media\",\"schema\"]","trace":[]}},
    {"id":"host-reply-eager-validation-and-bodyless-rules","source":"[(try reply(199,null) catch {e=>e.code}),(try reply(204,{x:1}) catch {e=>e.code}),(try reply(205,null,{\"Content-Type\":\"application/json\"}) catch {e=>e.code}),(try reply(200,{f:{x=>x}}) catch {e=>e.code}),(try reply(200,null,{\"Content-Type\":\"text/plain\"}) catch {e=>e.code}),(try reply(200,null,{\"Content-Encoding\":\"gzip\"}) catch {e=>e.code}),(try reply(200,null,{Connection:\"close\"}) catch {e=>e.code})]","expect":{"phase":"value","json":"[\"argument\",\"argument\",\"argument\",\"type\",\"argument\",\"argument\",\"argument\"]","trace":[]}},
    {"id":"host-reply-is-nominal-opaque-value","source":"reply(205,null)","expect":{"phase":"value","kind":"reply","trace":[]}},
    {"id":"host-server-routing-query-headers-and-head","source":"serve({\"GET /a/fixed\":{r=>{kind:\"literal\"}},\"GET /a/:x\":{r=>{p:r.params.x,path:r.path,q:r.query,h:r.headers}},\"POST /a/:x\":{r=>null},\"HEAD /head\":{r=>reply(200,{body:\"suppressed\"})},\"GET /only\":{r=>fail({code:\"unexpected_get\",message:\"\",data:{}})}})","host":{"capabilities":{"listen":[{"host":"127.0.0.1","port":8080}]}},"fixtures":[],"server":{"requests":[{"id":"literal","at_ms":0,"method":"GET","target":"/a/fixed","headers":[],"body_base64":""},{"id":"encoded","at_ms":2,"method":"GET","target":"/a/x%2Fy?tag=a+b&tag=c%2Bd&&flag&=empty","headers":[["X-Test"," first "],["x-test","second"]],"body_base64":""},{"id":"wrong","at_ms":4,"method":"PUT","target":"/a/fixed","headers":[],"body_base64":""},{"id":"missing","at_ms":6,"method":"GET","target":"/missing","headers":[],"body_base64":""},{"id":"head","at_ms":8,"method":"HEAD","target":"/head","headers":[],"body_base64":""},{"id":"nohead","at_ms":10,"method":"HEAD","target":"/only","headers":[],"body_base64":""},{"id":"invalid","at_ms":12,"method":"GET","target":"/a/%ZZ","headers":[],"body_base64":""}],"cancel_at_ms":20},"expect":{"phase":"abort","reason":"cancelled","trace":["listener:start","literal:receive","literal:response:200","encoded:receive","encoded:response:200","wrong:receive","wrong:response:405","missing:receive","missing:response:404","head:receive","head:response:200","nohead:receive","nohead:response:405","invalid:receive","invalid:response:400","listener:close"],"responses":[{"id":"literal","status":200,"body_base64":"eyJraW5kIjoibGl0ZXJhbCJ9"},{"id":"encoded","status":200,"body_base64":"eyJwIjoieC95IiwicGF0aCI6Ii9hL3glMkZ5IiwicSI6eyJ0YWciOlsiYSBiIiwiYytkIl0sImZsYWciOlsiIl0sIiI6WyJlbXB0eSJdfSwiaCI6eyJ4LXRlc3QiOlsiZmlyc3QiLCJzZWNvbmQiXX19"},{"id":"wrong","status":405,"body_base64":"eyJlcnJvciI6Im1ldGhvZF9ub3RfYWxsb3dlZCJ9","headers":{"allow":["GET, POST"]}},{"id":"missing","status":404,"body_base64":"eyJlcnJvciI6Im5vdF9mb3VuZCJ9"},{"id":"head","status":200,"body_base64":""},{"id":"nohead","status":405,"body_base64":"","headers":{"allow":["GET"]}},{"id":"invalid","status":400,"body_base64":"eyJlcnJvciI6ImJhZF9yZXF1ZXN0In0="}]}},
    {"id":"host-server-admission-body-coding-and-timeout-order","source":"serve({\"GET /slow\":{r=>sleep(20);{ok:true}}},{timeout:5,max_body:2,max_inflight:1})","host":{"capabilities":{"listen":[{"host":"127.0.0.1","port":8080}]}},"fixtures":[],"server":{"requests":[{"id":"slow","at_ms":0,"method":"GET","target":"/slow","headers":[],"body_base64":""},{"id":"badtarget","at_ms":1,"method":"GET","target":"/%ZZ","headers":[],"body_base64":""},{"id":"busy","at_ms":2,"method":"GET","target":"/unknown","headers":[],"body_base64":"YWJj"},{"id":"large","at_ms":7,"method":"GET","target":"/unknown","headers":[],"body_base64":"YWJj"},{"id":"coding","at_ms":9,"method":"GET","target":"/unknown","headers":[["Content-Encoding","gzip"]],"body_base64":""},{"id":"expect","at_ms":11,"method":"GET","target":"/slow","headers":[["Expect","100-continue"]],"body_base64":"b3ZlcnNpemU="}],"cancel_at_ms":15},"expect":{"phase":"abort","reason":"cancelled","trace":["listener:start","slow:receive","sleep#1:start","badtarget:receive","badtarget:response:400","busy:receive","busy:response:503","sleep#1:cancel","sleep#1:joined","slow:response:504","large:receive","large:response:413","coding:receive","coding:response:415","expect:receive","expect:response:400","listener:close"],"responses":[{"id":"badtarget","status":400,"body_base64":"eyJlcnJvciI6ImJhZF9yZXF1ZXN0In0="},{"id":"busy","status":503,"body_base64":"eyJlcnJvciI6InVuYXZhaWxhYmxlIn0="},{"id":"slow","status":504,"body_base64":"eyJlcnJvciI6InRpbWVvdXQifQ=="},{"id":"large","status":413,"body_base64":"eyJlcnJvciI6InBheWxvYWRfdG9vX2xhcmdlIn0="},{"id":"coding","status":415,"body_base64":"eyJlcnJvciI6InVuc3VwcG9ydGVkX21lZGlhX3R5cGUifQ=="},{"id":"expect","status":400,"body_base64":"eyJlcnJvciI6ImJhZF9yZXF1ZXN0In0="}]}},
    {"id":"host-server-root-cancellation-joins-pending-handler","source":"serve({\"GET /\":{r=>get(\"https://fixture.invalid/held\")}})","host":{"capabilities":{"listen":[{"host":"127.0.0.1","port":8080}],"http":["https://fixture.invalid"]}},"fixtures":[{"id":"held","op":"http","match":{"method":"GET","url":"https://fixture.invalid/held"},"hold":true,"cancel_ack_ms":3}],"server":{"requests":[{"id":"pending","at_ms":0,"method":"GET","target":"/","headers":[],"body_base64":""}],"cancel_at_ms":2},"expect":{"phase":"abort","reason":"cancelled","trace":["listener:start","pending:receive","held:start","held:cancel","held:joined","listener:close"],"starts":{"held":0},"responses":[]}},
    {"id":"host-server-input-and-response-nominal-behavior","source":"serve({\"GET /plain\":{r=>{status:404}},\"POST /created\":{r=>reply(201,input(r,{x:int}))},\"GET /raw\":{r=>reply(200,encode(\"ok\"),{\"Content-Type\":\"text/plain\"})},\"GET /empty\":{r=>reply(205,null)}})","host":{"capabilities":{"listen":[{"host":"127.0.0.1","port":8080}]}},"fixtures":[],"server":{"requests":[{"id":"plain","at_ms":0,"method":"GET","target":"/plain","headers":[],"body_base64":""},{"id":"created","at_ms":2,"method":"POST","target":"/created","headers":[["Content-Type","application/json"]],"body_base64":"eyJ4IjoxfQ=="},{"id":"invalid","at_ms":4,"method":"POST","target":"/created","headers":[["Content-Type","application/json"]],"body_base64":"eyJ4IjoiYmFkIn0="},{"id":"media","at_ms":6,"method":"POST","target":"/created","headers":[["Content-Type","text/plain"]],"body_base64":"eyJ4IjoxfQ=="},{"id":"raw","at_ms":8,"method":"GET","target":"/raw","headers":[],"body_base64":""},{"id":"empty","at_ms":10,"method":"GET","target":"/empty","headers":[],"body_base64":""}],"cancel_at_ms":15},"expect":{"phase":"abort","reason":"cancelled","trace":["listener:start","plain:receive","plain:response:200","created:receive","created:response:201","invalid:receive","invalid:response:400","media:receive","media:response:415","raw:receive","raw:response:200","empty:receive","empty:response:205","listener:close"],"responses":[{"id":"plain","status":200,"body_base64":"eyJzdGF0dXMiOjQwNH0="},{"id":"created","status":201,"body_base64":"eyJ4IjoxfQ=="},{"id":"invalid","status":400,"body_base64":"eyJlcnJvciI6ImJhZF9yZXF1ZXN0In0="},{"id":"media","status":415,"body_base64":"eyJlcnJvciI6InVuc3VwcG9ydGVkX21lZGlhX3R5cGUifQ=="},{"id":"raw","status":200,"body_base64":"b2s=","headers":{"content-type":["text/plain"]}},{"id":"empty","status":205,"body_base64":"","headers":{"content-length":["0"]}}]}},
    {"id":"host-cancellation-interrupts-pure-tail-loop","source":"fun go() go() end fun go try go() catch {e=>\"caught\"}","abort_after_steps":1000,"expect":{"phase":"abort","reason":"cancelled","trace":[]}},
    {"id":"host-root-deadline-bypasses-catch-and-joins","source":"try get(\"https://fixture.invalid/held\") catch {e=>\"caught\"}","host":{"capabilities":{"http":["https://fixture.invalid"]},"limits":{"deadline_ms":3}},"fixtures":[{"id":"held","op":"http","match":{"method":"GET","url":"https://fixture.invalid/held"},"hold":true,"cancel_ack_ms":2}],"expect":{"phase":"abort","reason":"deadline","trace":["held:start","held:cancel","held:joined"],"starts":{"held":0}}},
    {"id":"host-operation-timeout-remains-catchable-after-join","source":"try http(\"GET\",\"https://fixture.invalid/held\",{timeout:5}) catch {e=>[e.code,e.data.timeout,e.data.outcome]}","host":{"capabilities":{"http":["https://fixture.invalid"]}},"fixtures":[{"id":"held","op":"http","match":{"method":"GET","url":"https://fixture.invalid/held"},"hold":true,"cancel_ack_ms":2}],"expect":{"phase":"value","json":"[\"timeout\",5,\"unknown\"]","trace":["held:start","held:cancel","held:joined"],"starts":{"held":0}}},
    {"id":"fun-factorial-named-end","source":"fun fac(n) if n<=1 then 1 else n*fac(n-1) end fun fac fac(5)","expect":{"phase":"value","json":"120"}},
    {"id":"fun-body-sequence","source":"fun add(x,y) z=x+y;z*2 end fun add add(2,3)","expect":{"phase":"value","json":"10"}},
    {"id":"fun-empty-body","source":"fun empty() end fun empty empty()","expect":{"phase":"value","json":"null"}},
    {"id":"fun-trailing-semicolon","source":"fun f() 7;end fun f f()","expect":{"phase":"value","json":"7"}},
    {"id":"fun-body-ending-in-binding","source":"fun f() x=7 end fun f f()","expect":{"phase":"value","json":"null"}},
    {"id":"fun-body-ending-in-definition","source":"fun f() fun g() 1 end fun g end fun f f()","expect":{"phase":"value","json":"null"}},
    {"id":"fun-nested-same-name","source":"fun f(x) fun f(y) x+y end fun f f(3) end fun f f(4)","expect":{"phase":"value","json":"7"}},
    {"id":"fun-body-closure-capture","source":"fun make(x) y=x+1;fun add(z) y+z end fun add add end fun make f=make(3);f(5)","expect":{"phase":"value","json":"9"}},
    {"id":"fun-callback-body-sequence","source":"fun twice(x){body} y=body(x);body(y) end fun twice twice(2){x=>x*3}","expect":{"phase":"value","json":"18"}},
    {"id":"fun-reserved-record-keys","source":"r={fun:3,end:4};[r.fun,r?.end]","expect":{"phase":"value","json":"[3,4]"}},
    {"id":"fun-sequence-tail-recursion","source":"fun count(n,a) next=a+1;if n==0 then a else count(n-1,next) end fun count count(100000,0)","expect":{"phase":"value","json":"100000"}},
    {"id":"fun-commented-end","source":"fun f() 1 end // marker\nfun // name\nf f()","expect":{"phase":"value","json":"1"}},
    {"id":"parse-fun-old-definition","source":"f(x)=x;f(1)","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-missing-prefix","source":"f(x) then x end fun f","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-old-equals","source":"fun f(x)=x end fun f","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-legacy-then","source":"fun f(x) then x end fun f","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-unclosed","source":"fun f() 1","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-missing-end-name","source":"fun f() 1 end fun","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-missing-end-keyword","source":"fun f() 1 end f","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-wrong-end-name","source":"fun f() 1 end fun g","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-end-case-sensitive","source":"fun f() 1 end fun F","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-crossed-end-names","source":"fun f() fun g() 1 end fun f end fun g","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-no-expression-definition","source":"x=fun f() 1 end fun f","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-reserved-binding","source":"fun=1","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-end-reserved-binding","source":"end=1","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-fun-reserved-parameter","source":"fun f(end) 1 end fun f","expect":{"phase":"parse","code":"syntax"}},
    {"id":"check-fun-parameter-body-scope","source":"fun f(x) x=2;x end fun f f(1)","expect":{"phase":"check","code":"duplicate_binding"}},
    {"id":"check-fun-slot-body-scope","source":"fun f(){body} body=1 end fun f","expect":{"phase":"check","code":"duplicate_binding"}},
    {"id":"check-fun-body-no-hoisting","source":"fun f() g();fun g() 1 end fun g end fun f f()","expect":{"phase":"check","code":"name"}},
    {"id":"check-fun-body-local-no-leak","source":"fun f() x=2;x end fun f x","expect":{"phase":"check","code":"name"}},
    {"id":"compact-definitions-adjacent","source":"fun f() 1 end fun f fun g() f()+1 end fun g g()","expect":{"phase":"value","json":"2"}},
    {"id":"compact-body-record-and-lambda","source":"fun r(x) {x:x} end fun r fun f() {x=>x+1} end fun f [r(2),f()(4)]","expect":{"phase":"value","json":"[{\"x\":2},5]"}},
    {"id":"compact-callback-slot","source":"fun f(x){body} body(x) end fun f f(2){x=>x+3}","expect":{"phase":"value","json":"5"}},
    {"id":"compact-nested-and-empty","source":"fun outer() fun inner() 7 end fun inner inner() end fun outer fun empty() end fun empty [outer(),empty()]","expect":{"phase":"value","json":"[7,null]"}},
    {"id":"member-size-kinds","source":"[[1,2].size,{a:1,b:2}.size,\"a😀é\".size,encode(\"é\").size]","expect":{"phase":"value","json":"[2,2,4,2]"}},
    {"id":"member-bound-alias","source":"m=[1,2].map;[m(){x=>x+1},m(){x=>x*2}]","expect":{"phase":"value","json":"[[2,3],[2,4]]"}},
    {"id":"member-record-collision","source":"r={size:99,keys:42,map:{x=>x+1}};[r.size,r[\"size\"],r[\"keys\"],r.map(2)]","expect":{"phase":"value","json":"[3,99,42,3]"}},
    {"id":"member-independent-of-lexical-name","source":"map=99;[1].map{x=>x+map}","expect":{"phase":"value","json":"[100]"}},
    {"id":"member-public-type-positions","source":"[(try (1).size catch {e=>e.data.position}),(try [1].take(\"x\") catch {e=>e.data.position})]","expect":{"phase":"value","json":"[0,1]"}},
    {"id":"member-optional-call-is-ordinary","source":"[null?.size,(try null?.map{x=>x} catch {e=>e.code}),(try [1].size() catch {e=>e.code})]","expect":{"phase":"value","json":"[null,\"type\",\"type\"]"}},
    {"id":"member-selection-before-arguments","source":"try (1).take(fail({code:\"argument_ran\",message:\"\",data:{}})) catch {e=>e.code}","expect":{"phase":"value","json":"\"type\""}},
    {"id":"member-explicit-arguments-left-to-right","source":"try \"a\".replace(fail({code:\"first\",message:\"\",data:{}}),fail({code:\"second\",message:\"\",data:{}})) catch {e=>e.code}","expect":{"phase":"value","json":"\"first\""}},
    {"id":"member-chain","source":"range(1,6).filter{n=>n>2}.map{n=>n*10}.sum()","expect":{"phase":"value","json":"120"}},
    {"id":"parse-if-needs-then","source":"if true 1 else 2","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-binding-needs-separator","source":"x=1 x","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-expression-needs-separator","source":"1 2","expect":{"phase":"parse","code":"syntax"}},
    {"id":"parse-function-double-separator","source":"fun f() 1 end fun f;;f()","expect":{"phase":"parse","code":"syntax"}},
    {"id":"check-no-global-len","source":"len","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-map","source":"map","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-filter","source":"filter","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-fold","source":"fold","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-each","source":"each","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-flat","source":"flat","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-take","source":"take","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-sort","source":"sort","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-group","source":"group","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-unique","source":"unique","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-sum","source":"sum","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-keys","source":"keys","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-has","source":"has","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-pick","source":"pick","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-merge","source":"merge","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-one","source":"one","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-split","source":"split","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-join","source":"join","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-replace","source":"replace","expect":{"phase":"check","code":"name"}},
    {"id":"check-no-global-parallel","source":"parallel","expect":{"phase":"check","code":"name"}},
    {"id":"member-receiver-once-and-evaluation-order","source":"fun source() log(\"receiver\");[1,2] end fun source fun initial() log(\"argument\");0 end fun initial source().fold(initial()){(a,x)=>log(text(x));a+x}","host":{"capabilities":{"log":true}},"fixtures":[{"id":"recv","op":"log_line","match":{"line":"\"receiver\"\n"},"ok":null},{"id":"arg","op":"log_line","match":{"line":"\"argument\"\n"},"ok":null},{"id":"one","op":"log_line","match":{"line":"\"1\"\n"},"ok":null},{"id":"two","op":"log_line","match":{"line":"\"2\"\n"},"ok":null}],"expect":{"phase":"value","json":"3","trace":["recv:start","recv:ok","arg:start","arg:ok","one:start","one:ok","two:start","two:ok"]}}
  ]
}
```
<!-- KIROHA_CONFORMANCE_END -->

## 17. Measurements and reproducibility

### 17.1 What is measured

measurement.json is generated from this exact Markdown file. It contains:

- SHA-256, UTF-8 bytes, Unicode code points and token counts for the whole file.
- Separate counts/hashes for the generation card, conformance corpus, measurement corpus and reproduction script.
- The exact handwritten Kiroha and proposed same-SDK TypeScript snippets, per-source hashes and counts.
- Per-case deltas and **weighted aggregate** savings (sum token counts before calculating a percentage).
- Tokenizer package version and fingerprints of its pattern/rank/special-token tables.
- The extracted API count, conformance case count and expected phase distribution.

The named tokenizers are o200k_base and cl100k_base, using tiktoken 0.14.0's encode_ordinary. This is not a claim about any particular current model's tokenizer. Special-token-looking source text is counted as ordinary text. Chat framing, task prompts, tool results, repair attempts and hidden/model-internal tokens are outside this measurement.[T]

The three comparison snippets are a small handwritten convenience sample. Both sides assume a proposed common SDK and exclude import/async harness setup symmetrically. The TypeScript binding has not been implemented or executed. Its exact-decimal/JSON values would need a compatible representation; native host floats do not automatically satisfy Kiroha's semantics. These measurements do not establish runtime equivalence, generation reliability, total cost savings or general superiority over TypeScript.

The generation-card target is checked mechanically. The full implementation file is intentionally larger and is never reported as if it were the generation prompt. No generated hash/count is copied back into this Markdown, which would create self-reference.

### 17.2 Reproduce with only this file

This specification artifact uses UTF-8 without BOM and LF line endings. The following POSIX-shell commands create a temporary measurement environment, extract the script from this file and regenerate measurement.json. No Kiroha runtime is needed.

```bash
python3 -m venv .kiroha-measure-env
.kiroha-measure-env/bin/python -m pip install tiktoken==0.14.0
.kiroha-measure-env/bin/python - <<'PY'
from pathlib import Path
import subprocess, sys, tempfile

spec = Path("spec.md")
lines = spec.read_bytes().decode("utf-8").split("\n")
begin = "<!-- KIROHA_" + "MEASUREMENT_SCRIPT_BEGIN -->"
end = "<!-- KIROHA_" + "MEASUREMENT_SCRIPT_END -->"
starts = [i for i, line in enumerate(lines) if line == begin]
stops = [i for i, line in enumerate(lines) if line == end]
assert len(starts) == len(stops) == 1, "ambiguous markers"
a, b = starts[0], stops[0]
assert a < b and lines[a+1] == "```python" and lines[b-1] == "```"
payload = "\n".join(lines[a+2:b-1]) + "\n"
with tempfile.TemporaryDirectory(prefix="kiroha-measure-") as tmp:
    script = Path(tmp) / "measure.py"
    script.write_bytes(payload.encode("utf-8"))
    subprocess.run([sys.executable, str(script), str(spec),
                    "measurement.json"], check=True)
PY
```

The exact toolchain is recorded by the generated output. If the pinned package is unavailable or extraction validation fails, stop with an error; never replace unmeasured numbers with estimates. Different Python versions are recorded and may change metadata, while the tokenizer version/fingerprints and exact input determine token counts.

### 17.3 Handwritten measurement corpus

The next block is source data, not executed conformance evidence.

<!-- KIROHA_MEASUREMENT_CORPUS_BEGIN -->
```json
{
  "format": "kiroha-measurement-corpus/1",
  "language": "0.5.0/Web1",
  "status": "handwritten_sources_not_executed",
  "baseline": "TypeScript snippets using a proposed SDK with the same contracts; SDK/binding not implemented. Native array helpers are used only in the first case. An async expression harness supplies context. Imports and harness wrappers are excluded symmetrically. No executed-equivalence claim.",
  "cases": [
    {
      "id": "example-1",
      "task": "HTTP JSON validation, filtering and projection",
      "kiroha": "User=open({id:int,name:str,active:bool});\n((get(\"https://api.example/users\") | check([User])).filter{u=>u.active}).map{u=>{id:u.id,name:u.name}}",
      "typescript_same_sdk": "const User=open({id:int,name:str,active:bool});\ncheck(await get(\"https://api.example/users\"),[User]).filter(u=>u.active).map(u=>({id:u.id,name:u.name}))"
    },
    {
      "id": "example-2",
      "task": "Cursor pagination with page validation",
      "kiroha": "Page=open({items:[json],next:maybe(str)});\npages(null){c=>\n  p=get(\"https://api.example/items\",{query:{cursor:c}}) | check(Page);\n  {items:p.items,next:p.next}\n}",
      "typescript_same_sdk": "const Page=open({items:[json],next:maybe(str)});\nawait pages(null,async c=>{\n  const p=check(await get(\"https://api.example/items\",{query:{cursor:c}}),Page);\n  return {items:p.items,next:p.next}\n})"
    },
    {
      "id": "example-3",
      "task": "JSON API with two-write transaction",
      "kiroha": "d=db(\"main\");\nserve({\"POST /users\":{r=>\n  u=input(r,{name:str,email:str});\n  tx(d){t=>\n    row=sql(t,q`insert into users(name,email) values (${u.name},${u.email}) returning id`).rows.one();\n    sql(t,q`insert into audit(user_id) values (${row.id})`);\n    reply(201,row)\n  }\n}})",
      "typescript_same_sdk": "const d=db(\"main\");\nserve({\"POST /users\":async r=>{\n  const u=input(r,{name:str,email:str});\n  return tx(d,async t=>{\n    const row=one((await sql(t,q`insert into users(name,email) values (${u.name},${u.email}) returning id`)).rows);\n    await sql(t,q`insert into audit(user_id) values (${row.id})`);\n    return reply(201,row)\n  })\n}})"
    }
  ],
  "canonical_callback_examples": [
    "urls.parallel(4){u=>get(u)}",
    "users.map{u=>u.name}",
    "fun apply(x){f} f(x) end fun apply apply(10){v=>v+1}"
  ]
}
```
<!-- KIROHA_MEASUREMENT_CORPUS_END -->

### 17.4 Reproduction script

<!-- KIROHA_MEASUREMENT_SCRIPT_BEGIN -->
```python
#!/usr/bin/env python3
"""Reproduce measurement.json from the one-file Kiroha specification.
Usage: python reproduce_measurements.py spec.md measurement.json
Requires exactly tiktoken==0.14.0. This script does not execute Kiroha or TypeScript.
"""
import hashlib
import json
import platform
import re
import sys
from pathlib import Path
from collections import Counter
import tiktoken

PIN = "0.14.0"
ENCODINGS = ("o200k_base", "cl100k_base")
LANGUAGE = "0.5.0/Web1"

def fail(message):
    raise SystemExit(message)

def extract(document, name, language):
    # Split only LF: Unicode scalar separators may occur inside a JSON string.
    lines = document.split("\n")
    begin = "<!-- KIROHA_" + name + "_BEGIN -->"
    end = "<!-- KIROHA_" + name + "_END -->"
    starts = [i for i, line in enumerate(lines) if line == begin]
    stops = [i for i, line in enumerate(lines) if line == end]
    if len(starts) != 1 or len(stops) != 1:
        fail("Expected unique standalone markers for " + name)
    a, b = starts[0], stops[0]
    if b <= a + 3 or lines[a+1] != "```" + language or lines[b-1] != "```":
        fail("Invalid block/fence structure for " + name)
    payload = "\n".join(lines[a+2:b-1]) + "\n"
    if "\n```\n" in payload:
        fail("Unexpected internal closing fence for " + name)
    return payload

def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            fail("Duplicate JSON key: " + key)
        result[key] = value
    return result

def strict_json(text):
    return json.loads(text, object_pairs_hook=unique_object,
                      parse_constant=lambda s: fail("Nonfinite JSON: " + s))

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def encoding_fingerprint(enc):
    # Version-pinned tiktoken internals; no token tables are written to output.
    h = hashlib.sha256()
    pattern = enc._pat_str.encode("utf-8")
    h.update(len(pattern).to_bytes(8, "big")); h.update(pattern)
    for token, rank in sorted(enc._mergeable_ranks.items(), key=lambda p: p[1]):
        h.update(rank.to_bytes(8, "big"))
        h.update(len(token).to_bytes(8, "big")); h.update(token)
    for spelling, rank in sorted(enc._special_tokens.items()):
        raw = spelling.encode("utf-8")
        h.update(len(raw).to_bytes(8, "big")); h.update(raw)
        h.update(rank.to_bytes(8, "big"))
    return h.hexdigest()

def main():
    if len(sys.argv) != 3:
        fail(__doc__)
    if tiktoken.__version__ != PIN:
        fail("Expected tiktoken==" + PIN + ", found " + tiktoken.__version__)
    source, destination = map(Path, sys.argv[1:])
    raw = source.read_bytes()
    if raw.startswith(b"\xef\xbb\xbf") or b"\r" in raw:
        fail("Specification artifact must be UTF-8 without BOM, with LF line endings")
    document = raw.decode("utf-8", errors="strict")
    payloads = {
        "generation_card": extract(document, "GENERATION_CARD", "text"),
        "conformance": extract(document, "CONFORMANCE", "json"),
        "measurement_corpus": extract(document, "MEASUREMENT_CORPUS", "json"),
        "measurement_script": extract(document, "MEASUREMENT_SCRIPT", "python"),
    }
    corpus = strict_json(payloads["measurement_corpus"])
    manifest = strict_json(payloads["conformance"])
    if corpus.get("format") != "kiroha-measurement-corpus/1" or corpus.get("language") != LANGUAGE:
        fail("Wrong measurement corpus revision")
    if manifest.get("format") != "kiroha-conformance/1" or manifest.get("language") != LANGUAGE:
        fail("Wrong conformance manifest revision")
    ids = [case["id"] for case in manifest["cases"]]
    if len(ids) != len(set(ids)):
        fail("Duplicate conformance case id")
    api = re.findall(r"^\| ([0-9]+) \| `([^`]+)` \|", document, re.MULTILINE)
    if [int(n) for n, _ in api] != list(range(1, 52)):
        fail("API inventory must have consecutive rows 1..51")
    members = [signature.split("(", 1)[0].split("{", 1)[0] for _, signature in api]
    functions = [name.split(".")[-1] for name in members]
    globals_ = [name for name in members if "." not in name]
    methods = [name.split(".")[-1] for name in members if "." in name and not name.endswith(".size")]
    if len(set(functions)) != 51:
        fail("Duplicate callable name in inventory")

    encoders = {name: tiktoken.get_encoding(name) for name in ENCODINGS}

    def measure(text):
        data = text.encode("utf-8")
        return {
            "sha256": sha(data),
            "unicode_code_points": len(text),
            "utf8_bytes": len(data),
            "tokens": {name: len(enc.encode_ordinary(text))
                       for name, enc in encoders.items()},
        }

    cases = []
    for case in corpus["cases"]:
        kiroha = measure(case["kiroha"])
        baseline = measure(case["typescript_same_sdk"])
        comparisons = {}
        for name in ENCODINGS:
            k, t = kiroha["tokens"][name], baseline["tokens"][name]
            comparisons[name] = {
                "kiroha_minus_baseline_tokens": k-t,
                "savings_percent": round(100*(t-k)/t, 2),
            }
        cases.append({
            "id": case["id"], "task": case["task"],
            "sources": {"kiroha": case["kiroha"],
                        "typescript_same_sdk": case["typescript_same_sdk"]},
            "measurements": {"kiroha": kiroha, "typescript_same_sdk": baseline},
            "comparison": comparisons,
        })
    aggregate = {}
    for name in ENCODINGS:
        k = sum(c["measurements"]["kiroha"]["tokens"][name] for c in cases)
        t = sum(c["measurements"]["typescript_same_sdk"]["tokens"][name] for c in cases)
        aggregate[name] = {
            "kiroha_tokens": k, "baseline_tokens": t,
            "kiroha_minus_baseline_tokens": k-t,
            "weighted_savings_percent": round(100*(t-k)/t, 2),
        }
    card = measure(payloads["generation_card"])
    result = {
        "format": "kiroha-measurement/2",
        "language": LANGUAGE,
        "status": "tokenization_only",
        "runtime_execution_evidence": None,
        "generation_evaluation_evidence": None,
        "toolchain": {
            "python": platform.python_version(),
            "tiktoken": tiktoken.__version__,
            "encodings": {
                name: {"fingerprint_sha256": encoding_fingerprint(enc)}
                for name, enc in encoders.items()
            },
        },
        "method": {
            "input": "Exact raw UTF-8 text; LF; no normalization or chat framing",
            "encoding": "encode_ordinary; special-token-looking strings count as ordinary text",
            "extraction": "Unique whole-line markers, matching fence, payload including final LF",
            "percentage": "100*(baseline-kiroha)/baseline, rounded to 2 decimals; aggregate sums tokens first",
            "model_claim": "Named tokenizers do not identify any current model tokenizer",
        },
        "documents": {
            "full_markdown": {"file": source.name, **measure(document)},
            "generation_card": {
                **card, "target_tokens_each_encoding": 4096,
                "within_target": all(n <= 4096 for n in card["tokens"].values()),
            },
        },
        "embedded_blocks": {
            name: measure(payload) for name, payload in payloads.items()
        },
        "specified_surface": {
            "native_operation_count": len(functions), "native_operation_names": functions,
            "global_function_count": len(globals_), "global_function_names": globals_,
            "method_count": len(methods), "method_names": methods, "properties": ["size"],
            "schema_atoms": ["str", "num", "int", "bool", "json"],
            "conformance_case_count": len(ids),
            "expected_case_phases": dict(sorted(Counter(
                c["expect"]["phase"] for c in manifest["cases"]).items())),
            "conformance_execution": "not run by this script",
        },
        "corpus": {
            "status": corpus["status"], "baseline_assumptions": corpus["baseline"],
            "cases": cases, "aggregate": aggregate,
        },
        "canonical_callback_examples": [
            {"source": text, **measure(text)}
            for text in corpus["canonical_callback_examples"]
        ],
        "limitations": [
            "Handwritten convenience sample; no compiler/runtime/model generation was executed.",
            "Same-SDK TypeScript binding is proposed, not an implemented measured dependency.",
            "Source tokens exclude task prompts, imports/harness, diagnostics, repairs and model hidden tokens.",
            "Full implementation document and generation card serve different purposes.",
            "Document length and sample source savings do not establish LLM reliability or total task cost savings.",
        ],
    }
    destination.write_bytes((json.dumps(result, ensure_ascii=False, indent=2)+"\n").encode("utf-8"))
    print(json.dumps({
        "output": destination.name,
        "full_tokens": result["documents"]["full_markdown"]["tokens"],
        "generation_card_tokens": card["tokens"],
        "generation_card_within_target": result["documents"]["generation_card"]["within_target"],
        "conformance_cases": len(ids), "aggregate": aggregate,
    }, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()

```
<!-- KIROHA_MEASUREMENT_SCRIPT_END -->

## 18. External standards and dependency boundaries

The Kiroha-visible policies in Sections 2–16 are complete within the Web1 profile. Mature implementations of the following external formats/protocols may be reused. The protocol/dialect themselves remain external dependencies; implementing all of TLS or PostgreSQL inside the Kiroha evaluator is not a requirement.

| Reference | Boundary |
| --- | --- |
| [RFC 8259 — JSON][J] | JSON interchange baseline; Kiroha additionally rejects duplicate keys and non-scalar strings and preserves exact decimals |
| [RFC 3986 — URI syntax][U] | URI components/percent encoding; Section 10 supplies Kiroha's restricted accepted form and joining rules |
| [RFC 9110 — HTTP semantics][H] | HTTP methods/status/field semantics |
| [RFC 9112 — HTTP/1.1][F] | Message framing and connection handling; Web1 additionally fixes buffering/coding/limit behavior |
| [RFC 9562 — UUID][V] | UUIDv4 bits and conventional hexadecimal form |
| [PostgreSQL 16 JSON types][P] | Server JSON/JSONB representation boundary |
| [PostgreSQL 16 transaction isolation][I] | Read-committed database behavior |
| [PostgreSQL 16 message flow][D] | Driver protocol completion and transaction state |
| [tiktoken repository][T] | Reproducible tokenizer implementation; version pinned in the embedded script |

[J]: https://www.rfc-editor.org/rfc/rfc8259.html
[U]: https://www.rfc-editor.org/rfc/rfc3986.html
[H]: https://www.rfc-editor.org/rfc/rfc9110.html
[F]: https://www.rfc-editor.org/rfc/rfc9112.html
[V]: https://www.rfc-editor.org/rfc/rfc9562.html
[P]: https://www.postgresql.org/docs/16/datatype-json.html
[I]: https://www.postgresql.org/docs/16/transaction-iso.html
[D]: https://www.postgresql.org/docs/16/protocol-flow.html
[T]: https://github.com/openai/tiktoken
