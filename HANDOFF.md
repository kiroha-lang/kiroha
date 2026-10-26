# Handoff: Kiri 0.3.0 / Web1 implementation

Written for the next agent (Codex) to continue this work. Read this file first,
then [`README.md`](README.md) and [`docs/architecture.md`](docs/architecture.md).

- Repository: `/home/mizushima/dev/kiri` (git, branch `master`, 24 commits, clean tree at `de280ab`)
- Goal that is still active: **complete implementation of the language in
  [`spec.md`](spec.md) (Kiri 0.3.0 / Web1) plus comprehensive documentation.**
  The spec's own completion rule (§15.3) includes "all normative prose edge
  cases are implemented", so the work has been driven by repeated independent
  audits (below).
- Language/tools: Rust 1.96 (needs ≥ 1.88), Cargo, Java 25 (for TLC),
  PostgreSQL 16 (for the I03/I04 tests), libclang (for `pg_query`).

## 1. State at handoff

| Item | State |
| --- | --- |
| Embedded conformance corpus (`kiri conformance spec.md --json`) | 145 / 145 pass, exit 0 |
| `cargo test --release` | 57 pass, 0 fail (with PostgreSQL DSN, see §3) |
| `cargo clippy --release --all-targets` | 0 warnings |
| TLA+ models (`./formal/check.sh`) | Parallel (1,212 states) and Tx (90 states): no violation |
| Property tests (`tests/properties.rs`) | 13 properties pass; all mutation-checked |
| Documentation | README, language guide, builtins/values/errors reference, CLI, host config, architecture, formal verification, conformance report, 6 examples |
| Independent audits | 15 run so far. Findings per audit: 9, 5, 9, 6, 7, 5, 8, 2, 2, 13, 2, 1, 3, 5, then **8 open (audit 15)** |

**Audit 15 found 8 violations that are NOT fixed yet** (§5). Nothing else is
known to be open. Audits 1–14 are fully fixed with regression tests, recorded
row by row in [`docs/conformance-report.md`](docs/conformance-report.md)
("Independent audits" table).

## 2. Code map

```
src/lexer.rs parser.rs resolve.rs ast.rs     frontend (spec §2, §4)
src/decimal.rs json.rs value.rs schema.rs    exact decimals, strict JSON, values, schemas
src/url.rs sqlcheck.rs wire.rs               URL rules, SQL statement/hole validation (libpg_query), CLI wire values
src/interrupt.rs alloc.rs                    abort checkpoints inside builtin work; counting allocator (memory_bytes)
src/runtime/{mod,machine,frames}.rs          CEK machine, tasks, scopes, timers, parallel groups
src/builtins/{pure,callbacks,effects,db,server,headers,args}.rs   the 51 functions
src/host/mod.rs config.rs paths.rs           Host trait, kiri-host/1 config, filesystem path rules
src/host/virtual_host.rs                     deterministic fixture host (spec §16)
src/host/real/{mod,http,pg,server,files}.rs  production adapters (tokio, rustls, tokio-postgres, httparse, openat)
src/conformance.rs cli.rs                    §16 runner, §15 CLI
tests/                                       i01..i08 integration scenarios, prose_regressions, properties
formal/                                      TLA+ models + check.sh
```

Key design facts (details in `docs/architecture.md`):

- Evaluator is an explicit-continuation (CEK) machine; callbacks of builtins are
  continuation frames (never Rust recursion), so tasks can suspend on effects.
- `Host` trait: `dispatch(id, Effect)` / `cancel(id)` / `wait(until)`. The
  runtime owns semantics; adapters do I/O. `VirtualHost` is used by the
  conformance runner, `RealHost` by `kiri run`.
- Aborts (`Ctl::Abort`) are separate from errors and bypass `try`.
- **Interruptibility (§8.3)** is the most fragile area: any new loop over user
  data must call `interrupt::poll()` (or `poll_big(len)` for large units) and
  any large allocation must call `interrupt::reserve(bytes)` first. Results of
  an interrupted builtin are discarded by the runtime (`Fail::Interrupted`).
- Number quota: coefficients ≤ 100,000 significant digits, exponents ≤ ~±2.3e18
  → `resource` abort (documented host quota, spec §3.2).

## 3. How to build and verify

```sh
cargo build --release
./target/release/kiri conformance spec.md --json        # must say "failed":0
cargo clippy --release --all-targets                    # must be warning-free
./formal/check.sh                                       # needs Java; downloads tla2tools.jar into formal/.tools
```

PostgreSQL 16 for the I03/I04 tests (a cluster was running during this work at
`/tmp/claude-1000/pg16`, port 55432; scratch space, may be gone):

```sh
B=/home/linuxbrew/.linuxbrew/opt/postgresql@16/bin   # brew install postgresql@16 if missing
D=/tmp/kiri-pg16
LANG=C LC_ALL=C $B/initdb -D $D -U kiri --auth=trust -E UTF8 --locale=C
LANG=C LC_ALL=C $B/pg_ctl -D $D -o "-p 55432 -k /tmp -c listen_addresses=127.0.0.1" -l $D/log start
export KIRI_TEST_PG_DSN="host=127.0.0.1 port=55432 user=kiri dbname=postgres"
cargo test --release            # 57 tests; without the DSN, I03/I04 print SKIP and pass vacuously
```

`initdb` fails with "invalid locale settings" unless `LANG=C LC_ALL=C` is set.
`pg_query` needs libclang: if the build cannot find it, set
`LIBCLANG_PATH` (here: `/usr/lib/llvm-15/lib`).

## 4. The audit loop (how this project converges)

Because §15.3 demands all prose edge cases, the loop was:

1. Spawn a fresh read-and-execute audit agent: "walk every normative rule in
   §2–§15, try to break it, report only CONFIRMED violations with file:line,
   reproducer, actual vs expected; list per-subsection coverage". Give it the
   documented deliberate choices (§6) so it does not re-report them.
2. Fix every confirmed finding. Add a regression test for each (fixture-host
   manifest in `tests/prose_regressions.rs`, real-wire tests in
   `tests/i0x_*.rs`, unit tests next to the code).
3. Run: tests, conformance, clippy, `./formal/check.sh`.
4. Add a row to the audit table in `docs/conformance-report.md`; commit.
5. Repeat until an audit reports no confirmed violation.

Practical rules learned the hard way:

- Always wrap shell commands that run kiri/servers/clients in `timeout 60`;
  an audit once stalled for 80 minutes on an un-timed command.
- Measure durations from outside the process (wall clock in WSL2 drifts).
- Test processes: kill what you start (the I05 test uses a kill guard).
- The audit agents must not edit the repo; run them against a copy of the binary
  so concurrent rebuilds do not disturb them.
- When a property/model is added, mutate the implementation once to prove the
  check can fail (this exposed weak checks twice).

## 5. OPEN: audit-15 findings (fix these first)

Severity order. Re-run each reproducer before and after.

### 5.1 Cancelled `write` still replaces the target after the abort (§13.2, §9.1, §14.2)

- `src/host/real/files.rs` runs `replace_regular` in `spawn_blocking`;
  `renameat` (~line 156) has no abort check, and `RealHost::cancel`
  (`src/host/real/mod.rs`) only drops the future, so the blocking thread keeps
  going and the rename happens after the abort; the parallel join also does not
  wait for it.
- Repro (host: read/write on `<R>/out`, `max_file_bytes:300000000`, `out/race` = `OLD\n`):
  ```
  s=fold(range(0,27),"x"){(a,i)=>a+a}; b=encode(s);
  r=try parallel([1,2],2){x=>if x==1 then write("out/race",b) else do {sleep(1); fail({code:"boom",message:"m",data:{}})}} catch {e=>e.code};
  [r,len(read("out/race"))]
  ```
  Actual `["boom",4]` and the file later becomes 134 MB. Expected: old target
  unchanged and temp file removed (spec: "Abort observed before dispatch of that
  replacement must leave the old target unchanged"), or the join waits and the
  outcome is reported accurately.
- Suggested fix: give `ReplaceRegular` an `Arc<AtomicBool>` cancel flag; on
  `cancel` set it but **keep awaiting the blocking task** (do not drop it);
  check the flag immediately before `renameat`, remove the temp file and return
  "cancelled / not applied" if set. Same treatment is worth giving `read`.

### 5.2 HEAD/304 response with `Transfer-Encoding: gzip, chunked` fails (§10.4)

`src/host/real/http.rs` rejects any non-chunked transfer coding before it
decides the response is bodyless. Expected: for HEAD/204/304 (no body) ignore
Transfer-Encoding. Repro: raw server replying `HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n`
to `http("HEAD",u).status` → should be 200 with empty Bytes.

### 5.3 Client `header_bytes` undercounts (§14.1)

The client counts `name+value+4` from hyper's already-trimmed values and never
counts trailers. With `header_bytes:1000`: `X-Big:` + 5000 spaces + `a` and a
5000-octet chunked trailer both succeed; expected `resource` abort (host
resource). Needs raw octet counting of the field section (excluding the status
line, excluding the terminal blank line) plus trailers.

### 5.4 Client accepts invalid chunked framing (§10.3/§10.4)

hyper is lenient. Each of these must be `network`/`framing`: chunk line `7;\x01`,
`2;a="\x01"`, `2;=b`, `7 ` (trailing space after size), header
`Transfer-Encoding: chunked, chunked`.

> **Recommended for 5.2–5.4 together:** replace hyper in the client
> (`Client::exchange` in `src/host/real/http.rs`) with a small strict
> HTTP/1.1 client built on `httparse::Response`:
> write the request (`host`, caller headers, `content-length` if body,
> `connection: close`); read the head, counting raw field-section octets
> (excluding status line and terminal CRLF) against `header_limit` (→
> `EffOutcome::Resource("header_bytes")`, map `httparse::Error::TooManyHeaders`
> to the same); skip 1xx (101 → `network invalid_status`); frame the body as
> none (HEAD/204/304), chunked (only exactly `chunked`), Content-Length
> (identical duplicates allowed, conflicting → framing), or read-until-close;
> TE together with Content-Length → framing error; decode chunked with the same
> strict parser the server uses (§5.5) and validate trailer fields
> (`headers::valid_field`) and count them against `header_limit`;
> keep the existing incremental gzip decode with decoded `max_bytes`
> (`Counter` writer, `limit_error`) and `raw_cap` (`Resource("transport")`);
> keep setting `sent` just before writing request bytes. Keep behavior that the
> existing tests in `tests/i01_http.rs` and `tests/i02_tls.rs` rely on.

### 5.5 Server: large numerals in Content-Length and chunk-size (§12.4)

`src/host/real/server.rs`: `Content-Length: 99999999999999999999999` → currently
400, expected 413 when above `max_body` (RFC 9110 §8.6: parse as saturating
integer). Chunk size `ffffffffffffffffff` → 413; `00000000000000005` (valid
5-byte chunk with leading zeros) → currently 400, expected 200. Fix: strip
leading zeros, treat > 16 significant hex digits as `u64::MAX`; make
`framing_of` saturate instead of `Framing::Invalid` on overflow.
Refactor suggestion: move the chunked reader into a generic module
(`AsyncRead + Unpin`, per-piece sink callback, `max_total`, `header_limit`)
shared by server and the new client (it already contains `parse_chunk_size` and
bounded trailer handling).

### 5.6 Server `header_bytes` counts the request line (§14.1)

`handle_conn` measures the buffer starting at the request line. Reproduce with
`header_bytes:300`, a 230-char path and a 100-octet field section → connection
closed; expected: admitted (field section ≤ limit). Count only field lines
(after the first CRLF, excluding the terminal blank line); cap the request line
separately (e.g. 16 KiB, close if exceeded).

### 5.7 Framing error vs Content-Encoding precedence (§12.2)

POST with `Transfer-Encoding: chunked`, `Content-Length: 3`, `Content-Encoding: gzip`
→ currently 415; expected 400 (framing is validated before coding in the same
admission stage). Fix idea: add `framing_invalid: bool` to
`host::IncomingRequest` (set where `Framing::Invalid` is computed; `false` in
`virtual_host.rs`), and in `Runtime::server_request`
(`src/builtins/server.rs`) check it before `coding_ok`, using the same
`Ctl::Effect(Err(...))` pattern (non-`media` error → 400).

### 5.8 `Content-Encoding: identity,` or empty value → 415 (§12.2)

`headers::identity_coding` (`src/builtins/headers.rs`) compares the whole value.
Split on `,`, trim OWS, ignore empty elements (RFC 9110 §5.6.1.2); accept if
every remaining element is `identity` (or none). Used by the server admission
path, `input`, `post` and `reply`.

After fixing: add regression tests, update the audit table (row 15), commit,
then run audit 16 (and continue the loop).

## 6. Deliberate choices (documented; audits are told not to re-report them)

- One HTTP client connection per request; server: one request per connection, HTTP/1.1 only.
- EOF during handler execution is treated as client disconnect (TCP cannot
  distinguish a half-close; §12.2 requires disconnect to abort). Documented in
  the report's "Design choices".
- `memory_bytes` = process live heap (counting allocator), checked at
  checkpoints and via `interrupt::reserve` before large allocations.
- Number quota (100,000 significant digits; exponents ≈ ±2.3e18) is a
  resource abort.
- An adapter panic becomes a host `resource` abort (`guarded()` in `host/real/mod.rs`).
- `listen` capability entries match the configured host string exactly.
- A single native operation bounded by the memory or number quota is acceptable
  when the deadline is honored right before and after it.
- libpg_query (PostgreSQL 16 grammar, `pg_query` 5.1.1) validates SQL; the
  `typename $n` extension is rejected explicitly.

## 7. Conventions

- Commit messages: conventional style (`fix:`, `feat:`, `test:`, `docs:`,
  `verify:`), body listing the changes, and the attribution trailer your tooling
  requires.
- Every behavior fix needs a regression test; every doc claim should be
  checkable (examples in `docs/language-guide.md` were run and verified).
- Update `docs/conformance-report.md` (audit table, versions, design choices)
  when behavior or limits change; keep `docs/architecture.md` in step with the code.
- Code style: match surrounding code; comments explain *why* and cite spec
  sections; keep files focused.
- Environment quirks: the user's shell is zsh (no word splitting of `$var`; use
  `${=var}`); a pre-tool hook blocks some `grep`-heavy shell lines and inline
  Python containing certain patterns — retrying once overrides it.

## 8. Not done / ideas beyond the audits

- Kani could not verify `decimal.rs` (num-bigint uses inline assembly); a
  fixed-width re-implementation would not prove anything about the real code.
- The TLA+ models are hand-aligned abstractions of `create_group`/`child_done`
  and `TxFrame`, not mechanically linked to the Rust code. Keep them updated if
  those state machines change.
- Optional further hardening: fuzz the lexer/parser and the HTTP parsers
  (`cargo fuzz`), run the I01–I08 scenarios under `miri`/sanitizers, and add
  `measurement.json` regeneration (spec §17.2 script) to CI.
- No CI is configured yet; a workflow running the commands in §3 would be the
  natural next step.
