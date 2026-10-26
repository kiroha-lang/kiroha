# Conformance report

## 0.4.0 named-function update — 2026-10-26

Local release tests pass **83/83** with PostgreSQL enabled and no ignored
tests. Embedded conformance is **175/175**, adding 30 cases for named
delimiters, rejected legacy definitions, empty/sequential/nested bodies,
parameter/slot scope, capture and 100,000-call tail recursion. A parser unit
test checks malformed delimiters and the mismatched closing name's byte span.
The generation card measures 2638 tokens in o200k_base and 2618 in cl100k_base.
See [migration](migration-0.4.md) and [acceptance](completion.md).

The [v0.4.0 release CI](https://github.com/kiroha-lang/kiroha/actions/runs/40271474229)
also passed these gates and the four native builds on source `b8314f3`.
Public latest/pinned installations both passed 175/175 cases and executed
the new named-end factorial definition. See the [publication evidence](completion.md#040-named-function-revision-2026-10-26).

The following environment, independent audit and original publication details
are the dated **0.3.0 baseline**, not a fresh independent audit of 0.4.

Implementation report required by spec §15.3. Token measurements
(`measurement.json`) are a separate artifact and are not part of this report.

## Implementation and environment

| Item | Value |
| --- | --- |
| Implementation | `kiroha-rs/0.3.0` (this repository) |
| Language / profile | Kiroha 0.3.0 / Web1, CLI `kiroha-cli/1`, conformance `kiroha-conformance/1` |
| Compiler | rustc 1.96.0 (2026-05-25), release profile |
| OS | Linux 6.18.54.1-microsoft-standard-WSL2 (x86_64) |
| PostgreSQL server | PostgreSQL 16.15 (Homebrew build), local, C locale, UTF8 |
| Key dependencies | tokio 1.54.1, httparse 1.10.1, rustls 0.23.46 (ring), tokio-postgres 0.7.18, tokio-postgres-rustls 0.13.0, pg_query 5.1.1 (libpg_query, PostgreSQL 16 grammar), num-bigint 0.4.8, flate2 1.1.10 |

Final §15.3 acceptance is recorded requirement by requirement in
[the completion record](completion.md): 82 tests pass with PostgreSQL enabled,
145 corpus cases pass, and the final independent audit reports no confirmed
violation. Clippy, TLC and measurement checks pass.

## Embedded conformance corpus (§16)

Command: `kiroha conformance spec.md --json`

| Total | Passed | Failed | Skipped |
| --- | --- | --- | --- |
| 145 | 145 | 0 | 0 |

Exit status 0. All 145 cases (lex, parse, check, value, runtime, abort, and
all `host-*` fixture, trace, timing, parallelism and virtual-server cases)
run; there are no skips. Per-case results are in the JSON report.

## Integration scenarios (§15.3)

Command:

```sh
KIROHA_TEST_PG_DSN="host=127.0.0.1 port=55432 user=kiri dbname=postgres" cargo test --release --locked
```

| ID | Test | Status | What is exercised |
| --- | --- | --- | --- |
| I01 | `tests/i01_http.rs` (13 tests) | pass | Local raw HTTP fixture: GET/DELETE/POST, 4xx/5xx as data, repeated headers and `set-cookie`, 204/205/304, explicit HEAD with unsupported coding, gzip and decoded `max_bytes`, chunked framing, invalid chunked framing, invalid UTF-8/JSON, 302 not followed, one request per call (no retries), no cookies sent, query/header encoding, connection refused → `not_sent` |
| I02 | `tests/i02_tls.rs` (2 tests) | pass | rcgen CA + rustls server: valid chain/hostname succeeds; wrong hostname and untrusted chain fail with `tls`/`not_sent`; server that drops after receiving the request → `network`/`unknown` |
| I03 | `tests/i03_i04_postgres.rs::i03_*`, `cancelled_connection_setup_returns_pool_slot` | pass | PostgreSQL 16: every parameter kind, all supported result OIDs (int2/4/8, numeric with 39 digits, text, varchar, bpchar padding, uuid, bool, bytea, json, jsonb with `1e30`), NULL, hostile text as data, duplicate column names, unsupported types (date, float8, array, NaN numeric), DDL/multi-statement rejection, server SQLSTATE, U+0000 bind rejection, explicit conversions |
| I04 | `tests/i03_i04_postgres.rs::i04_*` | pass | Commit with verified table state, callback-failure rollback, caught poison stays primary (23505) with rollback, tx timeout interrupting a pure loop, `sql` timeout on `pg_sleep`, task confinement and nested-tx ownership, independent autocommit inside a rolled-back tx, `DISCARD ALL` reset before pooled reuse |
| I05 | `tests/i05_server.rs` (4 tests) | pass | Raw-socket requests: literal-over-parameter precedence, encoded slash, plus/query decoding, explicit HEAD (no body) and OPTIONS, implicit HEAD → 405 + Allow, JSON input, 415, 413 by Content-Length, chunked request body, 504, 500 for bare bytes, absolute-form target 400, 503 at `max_inflight`, client disconnect freeing the slot, SIGINT shutdown (exit 3) and closed listener |
| I06 | `tests/i06_i07_i08.rs::i06_*` | pass | Mode preservation (0640 kept), new files 0600, no parent creation, symlink target and symlinked directory refused, outside file untouched, `..` rejected, missing file `not_found`, no temp files left, component-based permission, `max_file_bytes` abort |
| I07 | `tests/i06_i07_i08.rs::i07_*` | pass | Pure infinite loop stopped by `deadline_ms` within the deadline (try handler not run), bounded parallelism timing, first failure cancelling a sleeping sibling, `eval_steps` exhaustion, 1,000,000-iteration tail loop |
| I08 | `tests/i06_i07_i08.rs::i08_*` | pass | Tagged numbers/bytes/opaque containers, stderr isolation of `log`, exit codes 0/1/2/3, configuration and `source_bytes` limits, `check` without effects, unreadable and invalid-UTF-8 sources, duplicate/unknown options, stdin source, `spec --compact`, `--version`, conformance run |

Additional prose-rule regressions (`tests/prose_regressions.rs`) cover rules
the corpus does not: holes as ParamRefs only, valid WITH … SEARCH/CYCLE and
sibling data-modifying CTEs, stacked content codings and 205 coding checks,
JSON content type on pre-admission errors, strict route escapes, unsigned duration/clock metadata and closed fixture formats, one enclosing cleanup deadline across joins/rollback/release, `fail`
raising its record unchanged, and left-to-right argument validation.

Without `KIROHA_TEST_PG_DSN`, I03/I04 print `SKIP: KIROHA_TEST_PG_DSN not set`
and do not assert; the results above were obtained **with** PostgreSQL 16.15.

## Formal verification

TLA+ models of `parallel` (§9.1) and `tx` (§11.5) are model-checked with TLC
(`./formal/check.sh`: 1,212 and 90 distinct states, no violation), and decimal
and JSON properties are checked against independent oracles
(`tests/properties.rs`, 13 properties). Every property was shown to catch an
injected defect. See [formal verification](formal-verification.md).

## Manual checks performed

- Real HTTPS: `example.com` succeeds; `wrong.host.badssl.com` and
  `self-signed.badssl.com` fail with `tls`/`certificate`/`not_sent`.
- `examples/fetch.kiroha` against the GitHub API, `examples/database.kiroha`
  against a temporary PostgreSQL 16 database (demo and audit state verified), and `examples/server.kiroha` with an isolated notes directory/port (greeting, save/read note, file content and SIGINT shutdown). All six examples pass `check`; the three pure examples also pass `run`.
- PostgreSQL `sslmode=require` against a non-TLS server fails at `db`/`pool`.

## Memory accounting strategy

Published per §14.1: live process heap bytes from a counting global
allocator, measured every 512 semantic steps and every 1024
interrupt polls inside builtin work; an excess over the baseline aborts the running
root/request scope with `resource`. See
[architecture](architecture.md#memory-accounting).

## Previously open items, now covered

| Item | Resolution |
| --- | --- |
| Lost COMMIT acknowledgment against a real server | `i04_lost_commit_acknowledgment_is_unknown_without_replay`: a TCP proxy forwards `COMMIT` to PostgreSQL 16 and drops the acknowledgment; Kiroha reports `db`/`commit`/`unknown`, discards the connection and does not replay (≤ 1 row). |
| HTTP timeout outcome precision | `timeout_outcome_reflects_whether_bytes_were_sent`: a timeout during an unfinished TLS handshake reports `not_sent`; a timeout after the request was written reports `unknown`. |
| Statement grammar | Validation uses libpg_query built from PostgreSQL 16 (`PG_MAJORVERSION 16`), the reference dialect. |
| Fresh audit after fixes | See "Independent audits" below. |

## Independent audits

Twenty-three independent read-and-execute audits of sections 2–15 were run against the
code (each walked the normative rules section by section and confirmed every
finding by running programs or custom fixture manifests).

| Audit | Confirmed findings | Status |
| --- | --- | --- |
| 1 | 9: SQL holes not checked as ParamRefs; SEARCH/CYCLE and sibling-CTE false rejections; malformed statements sent; pre-admission responses without JSON content type; lenient route escapes; argument validation order in `replace`; `fail` reordering fields; `url` query error position; stacked content codings / 205 coding | All fixed; regression cases in `tests/prose_regressions.rs` |
| 2 (after fixes) | 5: ParamRef counting missed VALUES/LIMIT/RETURNING/MERGE/window/array positions; deadlines not observed inside long builtin work; harness hang on an unmatched effect while a timer was pending; `WITH … TABLE` accepted; fixture host skipped Upgrade/header-field transport checks | All fixed; regression cases in `tests/prose_regressions.rs`, `tests/i06_i07_i08.rs` (deadline inside builtin work) and `tests/i03_i04_postgres.rs` (holes in VALUES/RETURNING/LIMIT on PostgreSQL 16) |

| 3 (fixes + docs) | 9 code, 5 docs: SELECT INTO in a UNION's leftmost branch accepted; real-server request leaked when its deadline passed inside builtin work; deadlines not re-checked before builtin calls / final result, non-polling builtins; malformed real-server header sections closed silently instead of 400; multi-column SET counted shared ParamRefs twice; Debug-text counting fooled by literals; parenthesized `WITH … (TABLE a)` accepted and `… UNION ALL TABLE a` rejected; repeated trailing semicolons accepted; doc errors (minimum Rust version, server example setup, `source_bytes` wording, test count, an escape example) | All fixed; regressions in `src/sqlcheck.rs` unit tests, `tests/i03_i04_postgres.rs` (SELECT INTO creates no table; multi-column SET), `tests/i05_server.rs` (504 after CPU-bound handler, 400 for malformed heads), `tests/i06_i07_i08.rs` (`flat` and string concatenation honour deadlines) |

| 4 (focused re-check) | 6 + 2 minor: TABLE accepted after a SEARCH/CYCLE CTE followed by another CTE; half-closed clients treated as disconnected; running SQL not cancelled on timeout/abort (rollback waited for cleanup grace); listener kept accepting during serve cancellation; late request heads after serve ended left open, per-read (not total) header timeout; `replace` without checkpoints; non-ASCII whitespace and `UESCAPE` in the SQL lexer | Fixed, except half-close (see design choices). Regressions: SQL unit tests, `tests/i03_i04_postgres.rs` (tx timeout during `pg_sleep` finishes < 900 ms with server-side cancel), `tests/i06_i07_i08.rs` (`replace` under deadline); stop-accepting verified manually (new connection refused after cancellation) |

| 5 (full, §2–§15) | 7: `memory_bytes` not enforced for `+`/`dump`/`encode` (process could abort on a huge allocation); long arbitrary-precision work not interruptible; tx timeout without `outcome`; `select x${1}` (libpg_query's `typename $n` extension) not rejected; record field keys not charged as steps; serve handler type error position; duplicate directory roots differing by a trailing slash | All fixed. Regressions: `tests/i06_i07_i08.rs` (memory quota for `+`, `dump`, `encode`; number quota under a deadline; duplicate roots), `tests/prose_regressions.rs` (tx timeout outcomes `rolled_back`/`unknown`, hole after a name, handler position), SQL unit tests. Audit 5 reported "verified, no violation" for every other subsection of §2–§15 |

| 6 (full, §2–§15) | 5, none critical: gzip body over a small `max_bytes` aborted instead of raising `limit`; integers beyond the number quota (e.g. `1e100001`) rejected as non-integers in counts/options/limits; URLs with `" < > { } \| ^` backtick or `[ ]` in path/query accepted; missing/duplicate Host not rejected by the real server (RFC 9110 §7.2); field names instead of argument indices in some `type` positions (`reply` headers, `fail`) | All fixed. Regressions: `tests/i01_http.rs` (incremental gzip limit), `tests/i05_server.rs` (Host rules), `tests/prose_regressions.rs` (huge integers, URL characters, positions), `tests/i06_i07_i08.rs` (huge `eval_steps`). Audit 6 reported "verified, no violation" for every other subsection of §2–§15 |

| 7 (full, §2–§15) | 4 + 4 minor: overflowing chunk size panicked the server adapter (slot leaked, shutdown hung); >100 response header fields misreported as a `header_bytes` abort; PostgreSQL rows buffered entirely before `max_rows`; `retry` aborting after 400 attempts with zero delay; >256 request header fields rejected; `+1` chunk size accepted; number quota counting non-coefficient digits (trailing zeros, `%` alignment); JSON usage envelopes outside JSON-mode commands | All fixed; adapter futures are additionally panic-isolated (a panic becomes a host resource outcome, never a hang). Regressions: `tests/i05_server.rs` (chunk sizes, 300 headers), `tests/i01_http.rs` (150 response headers), `tests/i03_i04_postgres.rs` (`max_rows` under a 30 MB quota on a 5M-row query), `tests/prose_regressions.rs` (retry, number quota), `tests/i06_i07_i08.rs` (CLI modes), decimal unit tests |

| 8 (full, §2–§15) | 2: a response with a non-`chunked` transfer coding (e.g. `gzip, chunked`) returned the still-encoded body; division/remainder aborted on internal scaling although operands and result were within the quota (`1/(1e99998+1)`, `1e100001 % 7`, `1e60000 % 1e-60000`) | Fixed: unremovable transfer codings are `network`/`framing`; division scaling is bounded by operand sizes and `%` uses modular exponentiation. Regressions: `tests/i01_http.rs`, decimal unit tests. Every other subsection of §2–§15 was reported "verified, no violation" |

| 9 (full, §2–§15) | 2: `json` schema checks and JSON key hashing (`group`/`unique`/`pages` cursors) had no interrupt checkpoint; SIGINT/SIGTERM not observed while builtin work held the evaluator | Fixed: checkpoints added to both traversals; a signal handler sets a flag that every checkpoint honours (cancelled abort). Regressions in `tests/i06_i07_i08.rs`. Every other subsection of §2–§15 was reported "verified, no violation" |

| 10 (full, §8.3 focus) | 13, mostly interruptibility on pathological inputs: handler-result serialization and CLI result conversion not under checkpoints; schema normalization over shared structures; `sort` comparisons; O(n·m) closed-record extras and O(n²) `pick` duplicates; SQL row conversion and adapter row buffering vs memory; gzip decoding vs memory/deadline; PostgreSQL numeric/JSON beyond the number quota reported as catchable `db`/`decode`; JSON escaping and URL encoding loops; unbounded exponent-digit parsing; huge shared scalars postponing checks | All fixed: length-weighted checkpoints (`poll_big`), pointer-equality shortcuts, hash sets for duplicate/extra detection, exact pre-allocation reservations, checkpoints in normalization/sort/escaping/encoding/row conversion, adapter-side memory check while buffering rows, request serialization trips mapped to 504/503, CLI conversion under the root deadline/memory/SIGINT. Regressions in `tests/i06_i07_i08.rs`, `tests/i05_server.rs`, `tests/i01_http.rs`, `tests/i03_i04_postgres.rs` |

| 11 (full, §2–§15) | 2 (§8.3): `sql` result decoding not bounded by the operation timeout (and the adapter converted huge numeric cells without yielding); checkpoints too sparse when JSON/CLI output contains many huge numbers | Fixed: decoding is armed with min(operation, scope deadlines) and expires as an ordinary `sql` timeout; the adapter yields after large cells; number checkpoints are weighted by coefficient size in `dump`, `parse` and wire output. Regressions in `tests/i03_i04_postgres.rs` and `tests/i06_i07_i08.rs`. All normal results re-verified correct; every other subsection "verified, no violation" |

| 12 (full, §2–§15) | 1 (low): `reply` accepted a syntactically invalid custom Content-Type parameter for Bytes bodies | Fixed: media types validated per RFC 9110 §8.3.1 (`token "=" (token / quoted-string)` parameters). Regressions in `src/builtins/headers.rs` unit tests and `tests/prose_regressions.rs`. Every other subsection of §2–§15 "verified, no violation", including normal results, interruptibility, memory quotas, deep nesting, scheduler step counts and PostgreSQL 16 behaviour |

| 13 (full, §2–§15, incl. RFC syntax) | 3 (low–medium): empty media-type parameters (`application/json;`) rejected; http/get/post header and body rules checked after `timeout`/`max_bytes`; malformed chunked trailer fields accepted by the server | Fixed: RFC 9110 §5.6.6 empty parameters accepted; option rules validated in table order; trailer fields validated (RFC 9112 §7.1.2). Regressions in `tests/prose_regressions.rs`, `tests/i05_server.rs`, header unit tests. Every other subsection "verified, no violation" |

| 14 (full, §2–§15, incl. RFC syntax) | 5: chunked trailer section not bounded by `header_bytes` (memory growth); lenient chunk-size lines (whitespace, malformed extensions); bare `"` inside quoted media-type parameters accepted; empty Accept-Encoding / empty list elements rejected; quoted-pairs in `charset` not unescaped | Fixed: trailer section counted against `header_bytes` with buffer draining; RFC 9112 §7.1.1 chunk-size/extension parser; RFC 9110 media-type parser with quoted-string unescaping; empty Accept-Encoding elements accepted. Regressions in `tests/i05_server.rs`, `tests/prose_regressions.rs`, header unit tests. Every other subsection "verified, no violation" |

| 15 (full, §2–§15) | 8: a cancelled `write` still replaced the target after the abort and the join did not wait; HEAD/304 with `Transfer-Encoding: gzip, chunked` failed; client `header_bytes` undercounted (trimmed values, trailers); client accepted invalid chunked framing (extensions, trailing blanks, duplicate chunked); server numerals (Content-Length, chunk size) overflowed to 400 instead of 413 and leading zeros were rejected; server `header_bytes` counted the request line; framing errors lost to the Content-Encoding check (415 instead of 400); `Content-Encoding: identity,` / empty value → 415 | All fixed. The HTTP client no longer uses hyper: a purpose-built strict HTTP/1.1 client counts raw octets and shares a strict framing module (`src/host/real/http1.rs`) with the server; file operations are cooperatively cancellable; in-flight slots are released when the response is written. Regressions in `tests/i01_http.rs`, `tests/i05_server.rs`, `tests/i06_i07_i08.rs` and `src/host/real/http1.rs` unit tests |

| 16 (full, §2–§15) | 6: HTTP header budget reset across head/trailers and interim responses; empty Transfer-Encoding treated as absent; admitted trailer quota mapped to 400; max_file_bytes=u64::MAX wrapped the read bound; cancelled pool acquisition leaked capacity; cleanup_ms restarted for each cleanup effect | Fixed: shared field budgets and framing presence checks; request quota abort maps to 503; saturating file bound; RAII pool slots across connection setup/reset cancellation; enclosing cleanup deadlines cover cancellation joins, rollback and release. Regressions in I01/I03/I05/I06 and `tests/fixtures/cleanup.json`; all four cleanup timing cases fail on the audit-16 baseline. |

| 17 (full, §2–§16) | 4: unsigned timeout/clock-report values narrowed to negative i64; u64::MAX header quota overflow on fragmented client/server heads; fixture clock/OID/status numeric narrowing; closed fixture argument/result records accepted unknown fields | Fixed: exact unsigned values; saturating head bounds with fragmented-wire regressions; checked fixture metadata and wide clock values; pre-execution validation of closed normalized fixture schemas. Regressions in I01/I05 and `tests/prose_regressions.rs` / `tests/fixtures/unsigned.json`. |

| 18 (full, §2–§16) | 3: temporal values/absolute deadlines silently saturated and real sleep capped at 30 years; expected response status/header and virtual request octet metadata incompletely validated; file temporary-name RNG errors ignored | Fixed: checked duration/clock capacity with resource aborts and no shortened sleep; complete pre-execution expectation/request validation; RNG failure returns io/random_source/not_applied before file creation. Regressions: 13 virtual clock cases, simulated 31-year real-host sleep, malformed manifests and Linux fault-injected file entropy failure. |

| 19 (full, §2–§16) | 2: valid informational responses rejected after 16; long chunk/trailer lines depended on packet fragmentation; zero-step cancellation never fired | Fixed: unlimited internal informational responses, streaming extension validation, trailer bounds from the remaining field budget, shared transport heap accounting and start-line buffers independent of fragmentation; abort_after_steps=0 cancels before any expression/effect. Long fragmented metadata, memory-budget and zero-step regressions added. |

| 20 (full source walk, executed corpus/semantic/HTTP/TLS/server checks) | 2: external body declarations above u64::MAX became indistinguishable from the maximum configured body limit; failed temporary-file unlink was not diagnosed | Fixed: retain an overflow sentinel for framing comparisons, preserve whether a configured byte bound exceeds native capacity, return 413/limit or explicit transport resource termination without waiting for impossible bodies; failed unlink emits a best-effort sanitized cleanup diagnostic without replacing the primary outcome. Added exact/overflow/wide-bound wire controls and a deterministic OS unlink-failure regression. |

| 21 (full, §2–§16) | 2: max_connections imposed an undocumented u32 configuration bound; oversized command-count coefficients silently became zero | Fixed: widened positive pool counts without eager allocation; explicit resource/number for command-count quota exhaustion. CLI large-count and numeric quota/exactness controls added. |

| 22 (full, §2–§16, independent overlapping review) | 1: independently confirmed the same command-count zero substitution against its pre-fix snapshot | Fixed by the command-count correction recorded in audit 21; the fresh post-fix review is audit 23. |

| 23 (final full, §2–§16) | **0 confirmed violations** | Completed independent review of the fixed implementation (`7b79e8e`), with 360 passing cases/control records: corpus 145, fresh semantics 65, prior semantics 53, HTTP 19, PG16 13, CLI 17, real server 14, virtual server 4, TLS/files/server 26, latest count controls 4. [Full coverage and evidence](audits/audit-23.txt). |

Audit 2 recorded "no new violations" for §2, §3, §4, §5, §6, §7, §8, §9,
§10 (apart from the fixture header checks), §11 (apart from the two SQL
items), §12, §13, §14.1 and §15, and confirmed 8 of the 9 audit-1 fixes
(the ninth, ParamRef counting, was the item it re-opened and is now fixed).
Audit 3 confirmed the audit-2 fixes for the fixture server, HTTP pseudo-header
handling, the harness hang and ~60 valid hole positions, and reported the
remaining items in the table above, all now fixed. It also verified ~150
documentation snippets and claims as accurate.

## Design choices (within the specification)

| Area | Choice |
| --- | --- |
| HTTP client connections | One connection per request; no pooling, so no hidden session state (§10.1 permits reuse but does not require it). |
| Server connections | One request per connection with `connection: close`, a valid HTTP/1.1 behavior; HTTP/2 is not offered (optional in §10.1). |
| Client half-close | Kiroha 0.3.0 does not define half-close; §12.2 requires that a "client disconnect aborts its task". TCP offers no way to tell a half-close from a full close before a response is written (both arrive as FIN/EOF), so honouring §12.2 requires treating EOF during handler execution as a disconnect. A client that shuts down its sending side before the response therefore receives none. This is the conforming reading of §12.2, not a deviation. |
| Number size | Coefficients (significant digits, excluding a spelling's leading/trailing zeros) are limited to 100,000 decimal digits (operands and results), and exponents to about ±2.3×10^18; exceeding it is a `resource` abort with `{resource:"number"}`. §3.2 allows coefficient limits only as host resource termination; the quota bounds the cost of a single uninterruptible arbitrary-precision operation to tens of milliseconds, so deadlines and other tasks are not starved. |
| Memory quota | Process live-heap measurement at checkpoints (published strategy above); a single huge allocation may complete before it is observed, which §14.1 leaves implementation-dependent. |
| Native body capacity | Decoded body counters/buffers use native u64 capacity. A wider configured limit still accepts ordinary bodies; a declared body outside that capacity terminates with `resource` / `transport`, without substituting a smaller successful body. Wire numerals retain an overflow sentinel for comparisons with representable limits. |
| Clock capacity | Durations and absolute monotonic timestamps must fit u64 milliseconds; inability to represent a time aborts with `resource` / `{resource:"clock"}` rather than shortening a wait or substituting a timeout. Cleanup may abandon unresponsive work within its bounded grace. |
| Build requirement | `pg_query` needs a C compiler and libclang at build time. |

None of these change a value, error code or ordering asserted by the
specification.
