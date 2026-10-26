# Handoff: Kiroha 0.3.0 / Web1

The active objective is to complete the implementation of `spec.md`, its
comprehensive documentation, and choose a new language name because
`kiri-lang` was taken. The name is now **Kiroha (きろは)**; see
[docs/naming.md](docs/naming.md) for the migration and dated account lookup.
The checkout remains `/home/mizushima/dev/kiri`.

## Current state

- Audits 1–17 are fixed. Audits 16 and 17 found six and four boundary violations; fixes
  are recorded in [the conformance report](docs/conformance-report.md).
- A fresh full audit 18 and final verification of the renamed tree are next.
  Do not claim completion until they pass.
- The normative contract is [spec.md](spec.md), especially §15.3. Optional
  fuzzing, Miri/sanitizers and a mechanically linked proof are future work,
  not acceptance requirements.
- The Rust package, library, executable, source extension, CLI/host/conformance
  formats and specification markers use `kiroha`/`Kiroha`/`KIROHA` consistently.
  There is no legacy format alias. No remote or GitHub organization was created.
- CI is defined in `.github/workflows/verify.yml`, including PostgreSQL 16,
  Clippy, conformance, TLC and reproducible token measurement. A local pass is
  not evidence of a hosted CI run.

## Code and design

Read [the architecture](docs/architecture.md) for the complete map. Key areas:

- `src/{lexer,parser,resolve,ast}.rs`: source frontend and static resolution.
- `src/{decimal,json,value,schema}.rs`: exact values and schemas.
- `src/runtime/`: explicit-continuation evaluator, task/scopes and timers.
- `src/builtins/`: the 51 callable functions and callback state machines.
- `src/host/virtual_host.rs`: deterministic §16 fixture host.
- `src/host/real/`: production HTTP/TLS, PostgreSQL, files and server.
- `src/{cli,conformance,wire}.rs`: CLI and conformance protocols.
- `tests/`: I01–I08, prose regressions, independent-oracle properties and API
  inventory checks. `tests/fixtures/cleanup.json` asserts shared deadlines.
- `formal/`: bounded TLA+ abstractions of parallel/transaction transitions.

Long user-data loops must call `interrupt::poll()` / `poll_big(len)` and
large allocations must reserve memory first. Aborts bypass `try`. HTTP raw
field budgets include interim/final heads plus trailers. Pool slots are RAII
protected during setup and reset, including unpolled reset cancellation.
Final cleanup shares one enclosing deadline, including cancellation joins,
rollback and release; it cannot renew its grace for each effect.

## Verify

Requires Rust 1.88+ (tested 1.96), C/compiler libclang, Java 25 for TLC,
PostgreSQL 16 and Python with exactly `tiktoken==0.14.0` for measurement.

```sh
cargo build --release --locked
KIROHA_TEST_PG_DSN="host=127.0.0.1 port=55432 user=kiri dbname=postgres" cargo test --release --locked
timeout 60 target/release/kiroha conformance spec.md --json
cargo clippy --release --all-targets --locked -- -D warnings
timeout 60 ./formal/check.sh
python scripts/measure.py --check
```

Without `KIROHA_TEST_PG_DSN`, database integration tests explicitly print SKIP
and return; that is insufficient for acceptance. The existing local PG16
cluster is `/tmp/claude-1000/pg16`, port 55432, user `kiri` (check that it is
actually live). `LIBCLANG_PATH=/usr/lib/llvm-15/lib` works on this machine.
An isolated measurement venv is `/tmp/kiri-measure-env`.

To create another local PG16 cluster:

```sh
B=/home/linuxbrew/.linuxbrew/opt/postgresql@16/bin
D=/tmp/kiroha-pg16
LANG=C LC_ALL=C $B/initdb -D $D -U kiroha --auth=trust -E UTF8 --locale=C
LANG=C LC_ALL=C $B/pg_ctl -D $D -o "-p 55432 -k /tmp -c listen_addresses=127.0.0.1" -l $D/log start
```

## Audit workflow

The original handoff explicitly requests fresh read-and-execute audit agents:
walk all normative §2–§15 subsections, report only confirmed violations with
reproducers and subsection coverage, fix with regressions, rerun required
checks, repeat until no confirmed violation. Audit agents must not edit the
repo. Give them a copied binary; wrap CLI/server/client commands in
`timeout 60`; clean up processes. Record each audit in the conformance report.

Documented engineering choices are not bugs: one HTTP/1.1 connection per
request; EOF during a handler counts as client disconnect; live-heap memory
accounting at checkpoints; 100,000-digit coefficient quota and bounded i64
exponents abort as `resource`; adapter panics become host resource aborts;
listen capability strings match exactly; libpg_query uses PostgreSQL 16
syntax with the `typename $n` extension explicitly rejected.

Tests/model checks support conformance, not a universal proof of bug freedom.
TLA+ models remain hand-aligned abstractions; cleanup timing is exercised by
virtual-clock regressions, not modeled as a clock in the transaction model.
