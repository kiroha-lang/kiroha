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
| Key dependencies | tokio 1.54.1, hyper 1.12.0, rustls 0.23.46 (ring), tokio-postgres 0.7.18, tokio-postgres-rustls 0.13.0, pg_query 6.2.1 (libpg_query), num-bigint 0.4.8, flate2 1.1.10, httparse 1.10.1 |

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
| I01 | `tests/i01_http.rs` (4 tests) | pass | Local raw HTTP fixture: GET/DELETE/POST, 4xx/5xx as data, repeated headers and `set-cookie`, 204/205/304, explicit HEAD with unsupported coding, gzip and decoded `max_bytes`, chunked framing, invalid chunked framing, invalid UTF-8/JSON, 302 not followed, one request per call (no retries), no cookies sent, query/header encoding, connection refused → `not_sent` |
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
allocator, measured every 512 semantic steps and every 4096 elements inside
long builtin loops; an excess over the baseline aborts the running
root/request scope with `resource`. See
[architecture](architecture.md#memory-accounting).

## Known deviations and limitations

| Area | Note |
| --- | --- |
| Lost COMMIT acknowledgment (I04) | Exercised by the fixture-host case `host-tx-unknown-commit-discards-without-rollback-or-replay`. Against the real server it is not reproduced by an automated test (it requires dropping the connection after COMMIT is sent); the adapter maps any non-server COMMIT failure to `db`/`commit`/`unknown` and discards the connection. |
| HTTP/TLS outcome precision | Timeouts of dispatched requests always report `outcome:"unknown"`, including timeouts that happen before any byte was sent (DNS/connect). This is the conservative choice the spec permits. |
| HTTP connection reuse | The client opens one connection per request (permitted; no hidden state). |
| Server keep-alive | One request per connection (`connection: close`); HTTP/2 is not offered. |
| Statement parser | libpg_query (PostgreSQL 17 grammar) is used for statement validation; PostgreSQL 16 remains the execution dialect. Statements valid only in 17 would pass validation and fail at the server as `db`/`execute`. |
| Build requirement | `pg_query` needs a C compiler and libclang at build time. |
| Memory quota | Measured for the whole process at checkpoints; a single very large allocation can exceed the quota before it is observed (§14.1 explicitly makes allocation overhead implementation-dependent). |

These do not change any value, error code or ordering asserted by the
specification's tests.
