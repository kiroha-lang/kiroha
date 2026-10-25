# Conformance report

Implementation report required by spec §15.3. Token measurements
(`measurement.json`) are a separate artifact and are not part of this report.

## Implementation and environment

| Item | Value |
| --- | --- |
| Implementation | `kiri-rs/0.3.0` (this repository) |
| Language / profile | Kiri 0.3.0 / Web1, CLI `kiri-cli/1`, conformance `kiri-conformance/1` |
| Compiler | rustc 1.96.0 (2026-05-25), release profile |
| OS | Linux 6.18.54.1-microsoft-standard-WSL2 (x86_64) |
| PostgreSQL server | PostgreSQL 16.15 (Homebrew build), local, C locale, UTF8 |
| Key dependencies | tokio 1.54.1, hyper 1.12.0, rustls 0.23.46 (ring), tokio-postgres 0.7.18, tokio-postgres-rustls 0.13.0, pg_query 5.1.1 (libpg_query, PostgreSQL 16 grammar), num-bigint 0.4.8, flate2 1.1.10, httparse 1.10.1 |

## Embedded conformance corpus (§16)

Command: `kiri conformance spec.md --json`

| Total | Passed | Failed | Skipped |
| --- | --- | --- | --- |
| 145 | 145 | 0 | 0 |

Exit status 0. All 145 cases (lex, parse, check, value, runtime, abort, and
all `host-*` fixture, trace, timing, parallelism and virtual-server cases)
run; there are no skips. Per-case results are in the JSON report.

## Integration scenarios (§15.3)

Command:

```sh
KIRI_TEST_PG_DSN="host=127.0.0.1 port=55432 user=kiri dbname=postgres" cargo test --release
```

| ID | Test | Status | What is exercised |
| --- | --- | --- | --- |
| I01 | `tests/i01_http.rs` (5 tests) | pass | Local raw HTTP fixture: GET/DELETE/POST, 4xx/5xx as data, repeated headers and `set-cookie`, 204/205/304, explicit HEAD with unsupported coding, gzip and decoded `max_bytes`, chunked framing, invalid chunked framing, invalid UTF-8/JSON, 302 not followed, one request per call (no retries), no cookies sent, query/header encoding, connection refused → `not_sent` |
| I02 | `tests/i02_tls.rs` (2 tests) | pass | rcgen CA + rustls server: valid chain/hostname succeeds; wrong hostname and untrusted chain fail with `tls`/`not_sent`; server that drops after receiving the request → `network`/`unknown` |
| I03 | `tests/i03_i04_postgres.rs::i03_*` | pass | PostgreSQL 16: every parameter kind, all supported result OIDs (int2/4/8, numeric with 39 digits, text, varchar, bpchar padding, uuid, bool, bytea, json, jsonb with `1e30`), NULL, hostile text as data, duplicate column names, unsupported types (date, float8, array, NaN numeric), DDL/multi-statement rejection, server SQLSTATE, U+0000 bind rejection, explicit conversions |
| I04 | `tests/i03_i04_postgres.rs::i04_*` | pass | Commit with verified table state, callback-failure rollback, caught poison stays primary (23505) with rollback, tx timeout interrupting a pure loop, `sql` timeout on `pg_sleep`, task confinement and nested-tx ownership, independent autocommit inside a rolled-back tx, `DISCARD ALL` reset before pooled reuse |
| I05 | `tests/i05_server.rs` | pass | Raw-socket requests: literal-over-parameter precedence, encoded slash, plus/query decoding, explicit HEAD (no body) and OPTIONS, implicit HEAD → 405 + Allow, JSON input, 415, 413 by Content-Length, chunked request body, 504, 500 for bare bytes, absolute-form target 400, 503 at `max_inflight`, client disconnect freeing the slot, SIGINT shutdown (exit 3) and closed listener |
| I06 | `tests/i06_i07_i08.rs::i06_*` | pass | Mode preservation (0640 kept), new files 0600, no parent creation, symlink target and symlinked directory refused, outside file untouched, `..` rejected, missing file `not_found`, no temp files left, component-based permission, `max_file_bytes` abort |
| I07 | `tests/i06_i07_i08.rs::i07_*` | pass | Pure infinite loop stopped by `deadline_ms` within the deadline (try handler not run), bounded parallelism timing, first failure cancelling a sleeping sibling, `eval_steps` exhaustion, 1,000,000-iteration tail loop |
| I08 | `tests/i06_i07_i08.rs::i08_*` | pass | Tagged numbers/bytes/opaque containers, stderr isolation of `log`, exit codes 0/1/2/3, configuration and `source_bytes` limits, `check` without effects, unreadable and invalid-UTF-8 sources, duplicate/unknown options, stdin source, `spec --compact`, `--version`, conformance run |

Additional prose-rule regressions (`tests/prose_regressions.rs`) cover rules
the corpus does not: holes as ParamRefs only, valid WITH … SEARCH/CYCLE and
sibling data-modifying CTEs, stacked content codings and 205 coding checks,
JSON content type on pre-admission errors, strict route escapes, `fail`
raising its record unchanged, and left-to-right argument validation.

Without `KIRI_TEST_PG_DSN`, I03/I04 print `SKIP: KIRI_TEST_PG_DSN not set`
and do not assert; the results above were obtained **with** PostgreSQL 16.15.

## Manual checks performed

- Real HTTPS: `example.com` succeeds; `wrong.host.badssl.com` and
  `self-signed.badssl.com` fail with `tls`/`certificate`/`not_sent`.
- `examples/fetch.kiri` against the GitHub API, `examples/database.kiri`
  against PostgreSQL 16, `examples/server.kiri`-style server with `curl`.
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
| Lost COMMIT acknowledgment against a real server | `i04_lost_commit_acknowledgment_is_unknown_without_replay`: a TCP proxy forwards `COMMIT` to PostgreSQL 16 and drops the acknowledgment; Kiri reports `db`/`commit`/`unknown`, discards the connection and does not replay (≤ 1 row). |
| HTTP timeout outcome precision | `timeout_outcome_reflects_whether_bytes_were_sent`: a timeout during an unfinished TLS handshake reports `not_sent`; a timeout after the request was written reports `unknown`. |
| Statement grammar | Validation uses libpg_query built from PostgreSQL 16 (`PG_MAJORVERSION 16`), the reference dialect. |
| Fresh audit after fixes | See "Independent audits" below. |

## Independent audits

Eleven independent read-and-execute audits of sections 2–15 were run against the
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
| Client half-close | Kiri 0.3.0 does not define half-close; §12.2 requires that a "client disconnect aborts its task". TCP offers no way to tell a half-close from a full close before a response is written (both arrive as FIN/EOF), so honouring §12.2 requires treating EOF during handler execution as a disconnect — the same rule Go's net/http applies. A client that shuts down its sending side before the response therefore receives none. This is the conforming reading of §12.2, not a deviation. |
| Number size | Coefficients (significant digits, excluding a spelling's leading/trailing zeros) are limited to 100,000 decimal digits (operands and results), and exponents to about ±2.3×10^18; exceeding it is a `resource` abort with `{resource:"number"}`. §3.2 allows coefficient limits only as host resource termination; the quota bounds the cost of a single uninterruptible arbitrary-precision operation to tens of milliseconds, so deadlines and other tasks are not starved. |
| Memory quota | Process live-heap measurement at checkpoints (published strategy above); a single huge allocation may complete before it is observed, which §14.1 leaves implementation-dependent. |
| Build requirement | `pg_query` needs a C compiler and libclang at build time. |

None of these change a value, error code or ordering asserted by the
specification.
