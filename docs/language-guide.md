# Kiroha language guide

This guide teaches Kiroha 0.3.0 / Web1 by example. The normative definition is
[`spec.md`](../spec.md); the [built-in reference](reference/builtins.md) lists
every function. Examples show the result after `//`.

- [1. A first program](#1-a-first-program)
- [2. Lexical basics](#2-lexical-basics)
- [3. Values](#3-values)
- [4. Sequences, bindings and functions](#4-sequences-bindings-and-functions)
- [5. Lambdas and the callback slot](#5-lambdas-and-the-callback-slot)
- [6. Pipes](#6-pipes)
- [7. Operators and control flow](#7-operators-and-control-flow)
- [8. Access: strict and optional](#8-access-strict-and-optional)
- [9. Errors](#9-errors)
- [10. Schemas](#10-schemas)
- [11. Working with data](#11-working-with-data)
- [12. Concurrency, retries and pagination](#12-concurrency-retries-and-pagination)
- [13. HTTP client](#13-http-client)
- [14. PostgreSQL](#14-postgresql)
- [15. HTTP server](#15-http-server)
- [16. Local effects](#16-local-effects)
- [17. Things Kiroha deliberately does not have](#17-things-kiroha-deliberately-does-not-have)

## 1. A first program

A Kiroha program is a *sequence* of items separated by `;`. The value of the last
expression is the result.

```kiroha
greeting = "hello";
names = ["ada", "lin"];
names | map(){n => greeting + ", " + n}
// ["hello, ada","hello, lin"]
```

Run it with `kiroha run hello.kiroha --json`. Every program is fully checked
(lexed, parsed and name-resolved) before anything runs, so a typo in a branch
that never executes is still an error.

## 2. Lexical basics

- Source is strict UTF-8 without a byte-order mark.
- Whitespace is space, tab, CR and LF; newlines are *not* statement
  terminators. Use `;` between sequence items.
- Comments: `// to end of line`. There are no block comments.
- Identifiers: `[A-Za-z_][A-Za-z0-9_]*`, case-sensitive.
- Reserved words: `null true false if then else do try catch`.
- Numbers use JSON syntax without a sign: `0`, `12`, `1.5`, `2e-3`.
  `01`, `.5`, `1.`, `1abc` are lexical errors. `-1` is unary minus applied to `1`.
- Strings use double quotes and JSON escapes (`\n`, `\"`, `\u00e9`, surrogate
  pairs). There is no interpolation.
- `q` immediately followed by a backtick starts a SQL query literal
  (see [§14](#14-postgresql)).

## 3. Values

| Kind | Examples | Notes |
| --- | --- | --- |
| `null` | `null` | |
| `bool` | `true`, `false` | no truthiness |
| `number` | `1`, `0.1`, `1e100` | exact decimals |
| `string` | `"héllo"` | Unicode scalar values |
| `array` | `[1, "a", [true]]` | heterogeneous, immutable |
| `record` | `{a:1, "b c":2}` | ordered, immutable |
| `function` | `{x => x+1}`, `len` | |
| `bytes` | `encode("x")` | octets |
| `schema` | `int`, `maybe(str)` | |
| `query` | ``q`select 1` `` | opaque SQL |
| `database`, `transaction`, `reply` | from `db`, `tx`, `reply` | opaque handles |

Numbers are exact: `0.1 + 0.2` is exactly `0.3`, and
`9007199254740993 + 1` is `9007199254740994`. Only division rounds, to 34
significant digits: `1/3` is `0.3333333333333333333333333333333333`.

Records keep insertion order. Keys that are identifiers need no quotes, and
`{x}` is shorthand for `{x:x}`:

```kiroha
x = 9; {x, if:1, "0":2}     // {"x":9,"if":1,"0":2}
```

Duplicate keys (including escaped equivalents such as `{a:1,"a":2}`) are
a parse error. See [values and numbers](reference/values-and-numbers.md).

## 4. Sequences, bindings and functions

Three places hold sequences: the top level, `do { ... }`, and lambda bodies.

```kiroha
total = 1 + 2;              // binding
double(n) = n * 2;          // function definition
double(total)               // 6
```

Rules worth remembering:

- **No hoisting.** A name is visible only after its binding. `f()=g(); g()=1`
  is a check error.
- **Named recursion only.** A definition can call itself:
  `fac(n) = if n<=1 then 1 else n*fac(n-1)`.
- **No rebinding in one scope.** `x=1; x=2` is a `duplicate_binding` error, but
  inner scopes may shadow: `x=1; do { x=2; x }` is `2`, and the outer `x` is
  still `1`.
- **Closures capture lexically.** `x=1; f()=x; do { x=2; f() }` is `1`.
- **Sequence value.** The final expression is the value; an empty sequence or
  one ending in a binding/definition is `null`. A trailing `;` changes nothing:
  `do { 1; }` is `1`, `do { x=1; }` is `null`.
- **Proper tail calls.** Tail-recursive loops run in constant space:

```kiroha
count(n, acc) = if n == 0 then acc else count(n - 1, acc + 1);
count(100000, 0)            // 100000
```

## 5. Lambdas and the callback slot

Lambdas are written in braces with an arrow:

```kiroha
inc = {x => x + 1};
add = {(a, b) => a + b};
seven = {() => 7};
[inc(1), add(2, 3), seven()]    // [2,5,7]
```

`{x => ...}`, `{() => ...}` and `{(a, b, ...) => ...}` are the only heads;
`{(x) => x}` is invalid. Braces without a lambda head are records.

### The trailing callback

Every call has two channels: ordinary arguments in parentheses and at most one
**trailing lambda**. Builtins such as `map` take their callback this way:

```kiroha
map([1, 2, 3]){x => x * 10}     // [10,20,30]
fold([1, 2, 3], 0){(acc, x) => acc + x}   // 6
```

A user function declares the callback slot after its parameters:

```kiroha
apply(x){f} = f(x);
apply(10){v => v + 1}           // 11
```

The slot is a separate, **required** channel. These are all errors:

```text
apply(10)                  runtime arity error: missing callback
apply(10, {v => v + 1})    parse error: a lambda literal is never an ordinary argument
g = {v => v + 1}; apply(10, g)   runtime arity error: g fills an ordinary position
apply(10){g}               parse error: the trailing callback must be a lambda literal
```

To pass an existing function, wrap it: `apply(10){v => g(v)}`. Functions can
still be ordinary data — stored in records, arrays, or passed to parameters
that expect them: `invoke(f, x) = f(x); invoke(g, 2)`.

Parentheses are always required, even for no arguments: `now()`, `f(){x=>x}`.

## 6. Pipes

`lhs | f(a, b)` evaluates `lhs` once and passes it as the *first* ordinary
argument: it means `f(lhs, a, b)`. The callback stays separate:

```kiroha
[3, 1, 2] | sort() | map(){x => x * 2} | sum()     // 12
```

A pipe stage is always a name or dotted path followed by `(...)`:
`[1] | map` and `[1] | (map)()` are parse errors. Dotted paths look up record
fields: `ns = {m: map}; [1] | ns.m(){x => x}`.

To pipe the result of `if` or `try`, parenthesize it:
`(try risky() catch {e => null}) | f()`.

## 7. Operators and control flow

Precedence, highest first:

| Level | Operators | Associativity |
| --- | --- | --- |
| postfix | call `f()`, `.k`, `?.k`, `[i]`, `?[i]` | left |
| unary | `!`, `-` | |
| multiplicative | `*`, `/`, `%` | left |
| additive | `+`, `-` | left |
| comparison | `<`, `<=`, `>`, `>=`, `==`, `!=` | **at most one** per expression |
| and | `&&` | left |
| or | <code>&#124;&#124;</code> | left |
| coalesce | `??` | right |
| pipe | <code>&#124;</code> | left |

- `+` adds two numbers, concatenates two strings, or concatenates two arrays.
  Anything else is a `type` error — there is no coercion (`1 + "x"` fails).
- `%` is the remainder of the exact quotient truncated toward zero; the sign
  follows the left operand: `-5 % 2` is `-1`.
- Comparisons do not chain: `a < b < c` and `a < b == true` are parse errors;
  write `(a < b) == true`.
- `==`/`!=` compare JSON values structurally (`{a:1,b:2} == {b:2,a:1.0}` is
  `true`); comparing functions, bytes or handles is a `type` error.
- `&&`, `||` and `if` require booleans. There is no truthiness: `if 0 then ...`
  is a `type` error.
- `??` replaces only `null`: `0 ?? 7` is `0`, `null ?? 7` is `7`.

```kiroha
grade(n) = if n >= 90 then "A" else if n >= 80 then "B" else "C";
grade(85)                       // "B"
```

`if` and `try` must start an expression; use parentheses inside arithmetic:
`1 + (if c then 2 else 3)`. `do { ... }` is the block expression.

## 8. Access: strict and optional

Strict access fails loudly:

```kiroha
{x:1}.y      // runtime missing error, data {key:"y"}
[1][2]       // runtime bounds error, data {index:2,length:1}
```

Optional access returns `null` only for absence:

```kiroha
[{}?.missing, [1]?[4], null?.anything, {x:null}?.x]   // [null,null,null,null]
```

Wrong types are still errors (`[1]?[-1]` is an `argument` error; `"a"[0]` is a
`type` error). There is no string indexing.

## 9. Errors

Ordinary runtime failures are records `{code, message, data}` and can be
caught:

```kiroha
safe_div(a, b) = try a / b catch {e => e.code};
[safe_div(1, 4), safe_div(1, 0)]       // [0.25,"number"]
```

Raise your own with `fail`:

```kiroha
check_age(n) = if n < 0 then fail({code:"bad_age", message:"negative", data:{n}}) else n;
try check_age(-1) catch {e => e.data}  // {"n":-1}
```

- Branch on `e.code`, never on `e.message` (messages are diagnostic text).
- A handler does not catch its own failures.
- **Aborts** — cancellation, deadlines, resource exhaustion — are *not*
  errors and bypass every `try`. See [errors and aborts](reference/errors.md).

## 10. Schemas

Schemas validate data without coercion. The atoms are `str num int bool json`;
`[S]` is an array of `S`; `{k: S}` is a closed record.

```kiroha
User = {id:int, name:str, email:maybe(str), nick:optional(str)};
check({id:1, name:"ada", email:null}, User)
// {"id":1,"name":"ada","email":null}
```

- `maybe(S)` allows `null`.
- `optional(S)` allows the field to be *absent* (but not `null`); combine as
  `optional(maybe(str))`.
- `open(R)` allows and preserves extra JSON fields.
- `check` returns its input unchanged or raises `validation` with
  `{path, expected, actual}` for the first failure:

```kiroha
try check({users:[{id:1},{id:"x"}]}, {users:[{id:int}]}) catch {e => e.data}
// {"path":["users",1,"id"],"expected":"int","actual":"string"}
```

A malformed schema (`[]`, `[str,num]`, `true`, `optional(str)` at the root)
raises `schema` before any data is examined.

## 11. Working with data

Collections are immutable; functions return new values.

```kiroha
orders = [{c:"a", n:2}, {c:"b", n:5}, {c:"a", n:1}];
[
  orders | filter(){o => o.n > 1} | len(),
  orders | sort(){o => o.n} | map(){o => o.n},
  orders | group(){o => o.c} | map(){g => {c:g.key, total: g.items | map(){o => o.n} | sum()}},
  orders | unique(){o => o.c} | len()
]
// [2,[1,2,5],[{"c":"a","total":3},{"c":"b","total":5}],2]
```

Text and JSON:

```kiroha
[split("a,b,,c", ","), join(["x","y"], "-"), replace("aaaa", "aa", "b"),
 text(1.50), number("1.25e2"), parse("{\"a\":[1]}"), dump({a:[1, 2.0]})]
// [["a","b","","c"],"x-y","bb","1.5",125,{"a":[1]},"{\"a\":[1,2]}"]
```

Records: `keys`, `has`, `pick`, `merge` (shallow, right wins, positions kept),
`one` (exactly one element). Bytes: `encode`/`decode` (strict UTF-8).

## 12. Concurrency, retries and pagination

Everything is sequential unless you ask for concurrency with `parallel`:

```kiroha
urls | parallel(4){u => get(u)}     // at most 4 requests in flight, results in input order
```

The first failure cancels the remaining children and is re-raised. To collect
per-item failures, catch inside the callback:

```kiroha
urls | parallel(4){u => try {ok:get(u)} catch {e => {error:e.code}}}
```

`retry` re-runs a zero-argument callback with exponential backoff, only when
your predicate says so:

```kiroha
retry({attempts:3, when:{e => e.code == "timeout" || e.code == "network"}, delay:100}){() =>
  get("https://api.example/flaky")
}
```

`pages` follows cursors until `next` is `null`:

```kiroha
pages(null){cursor =>
  p = get("https://api.example/items", {query:{cursor}});
  {items:p.items, next:p.next}
}
```

`sleep(ms)` waits without blocking other tasks.

## 13. HTTP client

```kiroha
r = http("GET", "https://api.example/raw", {headers:{Accept:"text/plain"}, timeout:5000});
[r.status, r.headers["content-type"], decode(r.body)]

get("https://api.example/users", {query:{page:2}})       // parsed JSON, requires 2xx
post("https://api.example/users", {name:"ada"})            // JSON body, parsed JSON reply
url("https://api.example/v1", ["users", "a/b"], {q:"x y"})
// "https://api.example/v1/users/a%2Fb?q=x%20y"
```

- `http` returns every final status (including 4xx/5xx) as data;
  `get`/`post` raise `http` with `{status}` for non-2xx.
- No redirects, retries, cookies or ambient credentials. TLS is always
  verified.
- Response gzip is decoded; `max_bytes` limits the decoded body.
- The origin must be listed in the host's `capabilities.http`.

## 14. PostgreSQL

Queries are written as query literals; `${...}` holes become bound parameters,
never text:

```kiroha
d = db("main");
name = "x'); drop table users; --";
sql(d, q`select id, name from users where name = ${name}`).rows
```

- `sql` returns `{rows, count}`; `count` is the server's affected/returned
  count.
- Holes are only allowed in value positions outside quotes and comments; one
  statement per query; only `SELECT/INSERT/UPDATE/DELETE/MERGE` (optionally
  with `WITH`).
- Results map exactly: integers and `numeric` become exact numbers, text types
  strings, `json/jsonb` parsed JSON, `bytea` bytes. Other types (dates, floats,
  arrays) must be converted in SQL, e.g. `created_at::text`,
  `to_jsonb(tags)`.

Transactions:

```kiroha
tx(d){t =>
  row = sql(t, q`insert into users(name) values (${"ada"}) returning id`).rows | one();
  sql(t, q`insert into audit(user_id) values (${row.id})`);
  row
}
```

The callback's normal result commits; any failure rolls back. A failed
`sql(t, ...)` *poisons* the transaction even if you catch it — the transaction
will roll back and report that first SQL error. Only `sql(t, ...)` joins the
transaction; `sql(d, ...)`, HTTP and files inside the callback are independent.

## 15. HTTP server

```kiroha
d = db("main");
serve({
  "GET /health": {r => {ok:true}},
  "GET /users/:id": {r => sql(d, q`select id, name from users where id = ${number(r.params.id)}::int`).rows | one()},
  "POST /users": {r =>
    u = input(r, {name:str});
    reply(201, sql(d, q`insert into users(name) values (${u.name}) returning id`).rows | one())
  }
}, {port: 8080})
```

- Handlers receive `{method, path, params, query, headers, body}`.
- A JSON result is a `200 application/json` response; use `reply(status, v,
  headers)` for other statuses or raw bytes.
- `input(r, schema)` checks `Content-Type: application/json`, parses and
  validates; failures become `400`/`415`.
- Unknown paths are `404`, wrong methods `405` with `Allow`; uncaught errors
  are a generic `500`. Deadlines give `504`, overload `503`, large bodies `413`.
- `serve` runs until the process is cancelled (Ctrl-C), which the CLI reports
  as a `cancelled` abort.

## 16. Local effects

```kiroha
token = env("API_TOKEN");          // only names listed in the host file
write("out/report.json", encode(dump({ok:true})));   // atomic replacement
data = decode(read("in/data.txt"));
log({event:"done", at:now(), id:uuid()});           // one JSON line on stderr
```

Each needs its capability (`env`, `write`, `read`, `clock`, `random`, `log`).

## 17. Things Kiroha deliberately does not have

No assignment or mutation, no loops (use recursion and collection functions),
no `return`, no imports, no classes or methods, no default parameters in user
functions, no truthiness or implicit conversion, no string interpolation, no
positional callbacks, no async/await syntax (effects suspend implicitly), and
no hidden retries, redirects or cookies.
