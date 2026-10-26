# Built-in reference

Kiroha 0.5.0 / Web1 has **31 global functions, 19 collection methods, one `.size` property and 5 schema
atoms**. Globals are ordinary bindings and may be shadowed. Members are selected
by receiver kind; they are not global names. Normative text:
[`spec.md`](../../spec.md) §7–§13.

**Notation.** `f(a, b=default){cb}`: `=` marks a builtin default (applied only
when the argument or option is *omitted* — explicit `null` never requests a
default). `{cb}` is a required trailing callback, `[{cb}]` an optional one.
"Accepts n" means callable with n ordinary arguments and no callback.

Collection members use a receiver: `xs.map{f}`, `xs.fold(0){f}`, `xs.sum()`.
Zero ordinary arguments may omit `()` before a trailing callback. `.size`
returns a number directly. A saved method retains its receiver:
`m=[1,2].map; m(){x=>x+1}`. Registered record members take precedence over
fields; use `r["size"]` or `r["keys"]` to read colliding data. Other fields keep
ordinary dot access. A member type error reports the receiver at position 0,
then explicit arguments starting at 1.

**General validation order (§7.2).** Explicit arguments left to right, then
option fields in the order listed in each table, then unknown option names
(`argument`), then the callback's signature, then — for constrained
collections — every element, all before any callback runs or effect starts.
A wrong kind is `type`; a right kind with a bad value is `argument`. A
non-record options value is `type`. Callback signature mismatches are `arity`
with `reason:"callback_signature"`. User callback failures propagate
unchanged.

- [Data](#data): size, range, map, filter, fold, each, flat, take, sort, group, unique, sum
- [Records](#records): keys, has, pick, merge, one
- [Text and JSON](#text-and-json): split, join, replace, text, number, parse, dump, encode, decode
- [Schemas](#schemas): str, num, int, bool, json, check, maybe, optional, open
- [Runtime](#runtime): parallel, sleep, retry, pages, fail
- [HTTP client](#http-client): http, get, post, url
- [Database](#database): db, sql, tx
- [Server](#server): serve, input, reply
- [Local](#local): env, read, write, now, uuid, log

## Data

| Function | Result | Errors / notes |
| --- | --- | --- |
| `x.size` | Integer length: array items, record fields, string **scalars**, bytes octets | `type` for other kinds. `"😀".size` is 1 |
| `range(start, stop, step=1)` | Integers `start + k*step` before the exclusive `stop` | All integers (`type` if not number, `argument` if fractional); `step=0` → `argument`. Wrong direction or equal endpoints → `[]` |
| `xs.map{f}` | `[f(x) ...]` in order | `f` accepts 1 |
| `xs.filter{f}` | Items where `f(x)` is `true` | `f` must return bool (`type`) |
| `xs.fold(init){f}` | Left fold `f(acc, x)`; `init` when empty | `f` accepts 2 |
| `xs.each{f}` | `null`; runs `f(x)` for effects | |
| `xs.flat()` | One-level concatenation | Every item must be an array (`type`), checked first |
| `xs.take(n)` | First `min(n, len)` items | `n` nonnegative integer |
| `xs.sort() / xs.sort{f}` | Stable ascending sort by key (identity when omitted) | Keys all numbers or all strings (`type` otherwise, including a single invalid key). Each key computed once, in order |
| `xs.group{f}` | `[{key, items}]` in first-seen order | Keys must be JSON (`type`); equality is JSON equality (`1 == 1.0`, record order ignored); first key value kept |
| `xs.unique() / xs.unique{f}` | First item per JSON-equal key | As `group` |
| `xs.sum()` | Exact sum; `[]` → 0 | All items numbers (`type`) |

```kiroha
[range(5, 0, -2), [{n:2},{n:1}].sort{x => x.n}, [1, 1.0, {a:1,b:2}, {b:2,a:1}].unique()]
// [[5,3,1],[{"n":1},{"n":2}],[1,{"a":1,"b":2}]]
```

## Records

| Function | Result | Errors / notes |
| --- | --- | --- |
| `r.keys()` | Keys in insertion order | |
| `r.has(k)` | Own-key presence (`null` values count as present) | `k` string |
| `r.pick(ks)` | New record with keys in `ks` order | All `ks` strings and unique (checked first, `argument` for duplicates); absent key → `missing {key}`; never inserts null |
| `a.merge(b)` | Shallow merge; `b` wins; existing keys keep position; new keys append | Both records |
| `xs.one()` | The only item | Other lengths → `cardinality {expected:1, actual}` |

## Text and JSON

All text operations are literal scalar sequences — no regex, no locale.

| Function | Result | Errors / notes |
| --- | --- | --- |
| `s.split(sep)` | Parts, keeping empty leading/trailing parts. `"".split(",")` → `[""]` | `sep` nonempty (`argument`) |
| `xs.join(sep)` | Strings joined; empty `sep` allowed | Items strings (`type`) |
| `s.replace(old, new)` | All non-overlapping occurrences left to right; replacement not rescanned | `old` nonempty |
| `text(x)` | String unchanged; number/bool/null canonical spelling | Other kinds `type` |
| `number(s)` | Exact number from a complete signed JSON number | Non-string `type`; bad spelling (spaces, `+1`, `01`, `NaN`) → `number {reason:"invalid_syntax"}` |
| `parse(s)` | Strict JSON with exact numbers and ordered records | `json {reason, offset}`; reasons `syntax`, `duplicate_key`, `surrogate`, `bom`, `trailing` |
| `dump(v)` | Canonical compact JSON (see [values](values-and-numbers.md#json)) | Any non-JSON value anywhere → `type` |
| `encode(s)` | UTF-8 bytes | |
| `decode(b)` | Strict UTF-8 string; a leading U+FEFF is kept | `utf8 {offset}`; non-bytes `type` |

## Schemas

| Name | Meaning |
| --- | --- |
| `str`, `num`, `int`, `bool`, `json` | Atoms: string, number, integer-valued number, bool, any JSON |
| `[S]` (array literal) | Array of any length whose items match `S`; must have exactly one element |
| `{k: S, ...}` (record literal) | Closed record with exactly these required fields |
| `maybe(S)` | `null` or `S`; repeated `maybe` collapses; rejects `optional` operand |
| `optional(S)` | Field may be absent (not null). Only valid directly as a record field |
| `open(R)` | Record schema allowing extra JSON fields (preserved). Idempotent; only records |
| `check(v, S)` | Normalizes `S` fully, then validates `v`; returns `v` unchanged |

Errors: malformed schema → `schema {path, reason}` with reasons
`invalid_description`, `array_arity`, `optional_position`, `nested_optional`,
`record_required` (path relative to the schema). Mismatch →
`validation {path, expected, actual}` where `expected` is one of
`str num int bool json array record absent` and `actual` a kind name or
`"missing"`. Record fields are checked in schema order, then extra fields in
value order. Only the first failure is reported; no input values are copied
into the error.

## Runtime

### `xs.parallel(n){f}`

Runs `f(item)` with at most `n` (positive integer) callbacks active, starting
in index order; returns results in input order. Empty `xs` returns `[]`
without starting anything (but `f` is still validated). On the first observed
failure no further items start, running siblings are cancelled and joined, and
that error is raised. Collect per-item failures with `try` inside `f`.

### `sleep(ms)`

Waits at least `ms` (nonnegative integer) milliseconds, returns `null`.
Cancellable; `sleep(0)` yields to other tasks.

### `retry({attempts, when, delay}){f}`

All three options are required: `attempts` integer ≥ 1 (counts the first
attempt), `when` a function accepting one error, `delay` integer ms ≥ 0. `f`
accepts 0. After a failed attempt `i` with attempts remaining, `when(e)` must
return a bool; `true` waits `delay * 2^(i-1)` ms and retries, `false` re-raises
`e`. The final failure is raised **without** calling `when`. A failing or
non-bool `when` replaces the error. Aborts are never retried.

### `pages(seed, o={}){step}`

| Option | Default | Domain |
| --- | --- | --- |
| `max_pages` | 100 | integer ≥ 1 |
| `max_items` | 10000 | integer ≥ 0 |

`seed` must be JSON. `step(cursor)` must return exactly `{items:[...], next:JSON}`
(`type`/`argument` otherwise). Items are concatenated; only `next == null`
ends (0, false, "", [] do not). Checks per page, first match wins: too many
items → `pagination max_items`; `next` null → return; repeated non-null cursor
(JSON equality, seed included) → `pagination repeated_cursor`; page count
reached → `pagination max_pages`. Error data `{reason, pages, items}`. No
partial result on failure.

### `fail(err)`

Raises `err` unchanged. `err` must be exactly `{code, message, data}`: `code`
a nonempty string, `message` a string, `data` a JSON record. Extra/missing
fields → `argument`; wrong field kinds → `type`.

## HTTP client

### `http(method, u, o={})`

`method` is an uppercase HTTP token (not `CONNECT`); `u` a strict absolute
`http`/`https` URL (no userinfo, fragment, spaces or bad escapes). Returns
`{status, headers, body}` for every final status 200–599. Response headers are
a record of lowercase names to arrays of values in wire order (not
comma-split; `set-cookie` not merged).

| Option | Default | Meaning |
| --- | --- | --- |
| `headers` | `{}` | Record of string values. Names are tokens, case-insensitively unique. Values: tab, space, visible ASCII. Runtime-owned names (`Host`, `Content-Length`, `Transfer-Encoding`, `Connection`, `Keep-Alive`, `Proxy-Connection`, `Upgrade`, `TE`, `Trailer`, `Expect`) → `argument`. `Accept-Encoding` may request only gzip/identity |
| `query` | `{}` | Record appended as encoded pairs; values string/number/bool/null or one-level arrays; null omitted |
| `body` | absent | Bytes; forbidden for GET and HEAD |
| `timeout` | 10000 | Positive ms covering connect, TLS, send, read and decode |
| `max_bytes` | 8388608 | Limit on *decoded* body octets → `limit` |

Responses with `Content-Encoding: gzip` are decoded (checksums verified); any
other coding → `network unsupported_encoding`; corrupt gzip →
`network decode`. HEAD, 204 and 304 return empty bodies; a 205 with content is
a `network` protocol error. No redirects, retries, cookies or proxy
discovery; HTTPS always verifies chain and hostname. Network failures report
`outcome: "not_sent"` only when no request bytes were sent, otherwise
`"unknown"`.

### `get(u, o={})` and `post(u, v, o={})`

Same options except `body`. `post` serializes JSON `v` with `dump` and sends
`application/json` (an explicit Content-Type must be `application/json`,
optionally `charset=utf-8`; Content-Encoding must be identity). Both require
status 2xx (else `http {status}`, without parsing), then strictly decode UTF-8
and parse one JSON value. An empty body is a `json` error.

### `url(base, segments, query={})`

Appends percent-encoded path segments (unreserved characters kept, `/` → `%2F`,
space → `%20`, `+` → `%2B`) after removing at most one trailing slash from the
base path, then appends query pairs. `"."`/`".."` segments → `argument`.
Origin and existing query are preserved.

```kiroha
url("https://api.example/base?old=1", ["a/b","x y"], {q:["a b",null,true], n:0})
// "https://api.example/base/a%2Fb/x%20y?old=1&q=a%20b&q=true&n=0"
```

## Database

### `db(name)`

Returns the configured pool handle for alias `name` (needs the
`databases` capability). Repeated calls reuse the pool; there is no close.

### `sql(d, Q, o={})`

`d` is a database handle or a live transaction handle owned by the current
task; `Q` must be a query literal (a string is `type`).

| Option | Default | Domain |
| --- | --- | --- |
| `timeout` | 10000 | positive ms |
| `max_rows` | 10000 | nonnegative; exceeding → `limit` (never truncates) |

Statement validation (before any connection is used) rejects with
`db {phase:"prepare", outcome:"not_sent", reason}`: `statement` (not a single
SELECT/INSERT/UPDATE/DELETE/MERGE, optionally with WITH; includes DDL, SELECT
INTO, transaction control), `multiple_statements`, `hole_context` (hole inside
quotes, comments or dollar-quoted text), `literal_parameter` (a written `$1`).

Parameters bind as: null → untyped NULL, bool → boolean, string → text (no
U+0000), number → numeric (exact), bytes → bytea, JSON record/array → jsonb.
Other kinds → `db {phase:"bind"}`. Result columns: int2/int4/int8/numeric →
exact numbers (NaN/Infinity rejected), text/varchar/bpchar/uuid → strings,
bool, bytea → bytes, json/jsonb → parsed JSON, NULL → null. Any other column
type or duplicate column name → `db {phase:"decode"}` even for zero rows.

Returns `{rows:[record], count:int}`. Database errors carry `phase`,
`outcome` (`not_sent`, `rolled_back`, `committed`, `unknown`) and `sqlstate`
when the server supplied one.

### `tx(d, o={timeout:10000}){f}`

Runs `f(t)` inside a read-committed transaction on one connection in the same
task. Normal return commits and returns the value; failure rolls back.
Precedence of outcomes: ancestor abort > transaction timeout (ordinary
`timeout {operation:"tx"}`, catchable outside) > first failed `sql(t)`
(latched even if caught) > other callback error. `t` expires when `f`
finishes; other tasks (including `parallel` children) cannot use it; nested
`tx` in the same task → `db {phase:"ownership"}`. An unconfirmed COMMIT is
`db {phase:"commit", outcome:"unknown"}` and is never retried.

## Server

### `serve(routes, o={})`

`routes` maps `"METHOD /path"` to one-argument handlers. Path segments starting
with `:` are parameters. All routes, handlers and options are validated
before listening; ambiguous routes (same method, same length, same number of
literals, compatible at every position) → `argument`.

| Option | Default | Domain |
| --- | --- | --- |
| `host` | `"127.0.0.1"` | numeric IP |
| `port` | 8080 | 1–65535 |
| `timeout` | 30000 | positive ms per request |
| `max_body` | 1048576 | nonnegative bytes |
| `max_inflight` | 128 | positive |

The endpoint must be in `capabilities.listen`. The most specific matching
route (most literal segments) wins. Request record:
`{method, path, params, query, headers, body}` — `path` is the raw encoded
path, params are decoded once (so `%2F` stays inside one segment), query
values are arrays with `+` decoded as space. Response mapping and error codes:
see the [language guide](../language-guide.md#15-http-server) and spec §12.4.

### `input(r, S=json)`

Normalizes `S` (malformed → `schema`), checks the six-field request shape,
requires identity content coding and exactly one
`Content-Type: application/json` (optional `charset=utf-8`) (else `media`),
then decodes, parses and validates; failures → `input {reason}` with reason
`utf8`, `json` or `validation` (plus `path/expected/actual`).

### `reply(status, v, headers={})`

Status 200–599. `v` is JSON (sent as `application/json`) or bytes (default
`application/octet-stream`). 204/205/304 require `null` and forbid a
Content-Type. Header rules as for `http`. Returns an opaque `reply` value; a
plain record like `{status:404}` is just JSON.

## Local

| Function | Capability | Result / errors |
| --- | --- | --- |
| `env(name)` | `env` lists `name` | The configured string; absent → `env {name}` |
| `read(path)` | `read` directory | Whole regular file as bytes; missing → `io not_found`; over `max_file_bytes` → resource abort |
| `write(path, b)` | `write` directory | Atomic replacement, returns `null`. Keeps existing permission bits, new files 0600, no parent creation, symlinks refused |
| `now()` | `clock` | UTC `YYYY-MM-DDTHH:mm:ss.sssZ` (truncated ms) |
| `uuid()` | `random` | Lowercase random UUIDv4 |
| `log(v)` | `log` | Writes `dump(v)` + LF to stderr; non-JSON → `type` before output |

Paths: relative paths resolve against `filesystem_root`; `..` components and
NUL are rejected; `.` and repeated slashes are ignored; a trailing slash is
not a file. Every component is opened without following symlinks.
