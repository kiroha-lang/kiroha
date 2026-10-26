# Handoff: Kiroha 0.4.0 / Web1

## Current 0.4 syntax revision

The function syntax is now `fun fac(n) then ... end fun fac`. Both `fun` and
`end` are reserved, the closing name must match exactly, and bodies are
sequences in the parameter scope. Old bare definitions are parse errors.
See [migration](docs/migration-0.4.md). The 51-function API and callback channel
are retained. Local verification passes 83 PostgreSQL-enabled tests and
175/175 embedded cases, including 30 new syntax/scope/tail-call cases.
The compact card measures 2638/2618 tokens. The dated acceptance/audit and
0.3.0 publication records below are baseline evidence, not a new independent
audit of 0.4.

[v0.4.0 is public](https://github.com/kiroha-lang/kiroha/releases/tag/v0.4.0),
source `b8314f3`. The [full release CI](https://github.com/kiroha-lang/kiroha/actions/runs/40271474229)
passed all gates and four native builds. Public latest/pinned installers were
executed; both installed 0.4.0 binaries passed 175/175 cases, factorial,
exact-decimal execution, and rejection of legacy/malformed definitions.

## Completed 0.3 baseline

The requested implementation of `spec.md`, comprehensive documentation and
new language name are complete as of 2026-10-26. The working name changed because
`kiri-lang` was taken. The name is now **Kiroha (きろは)**; see
[docs/naming.md](docs/naming.md) for the migration and dated account lookup.
The checkout remains `/home/mizushima/dev/kiri`.

## 0.3 baseline state

- Audits 1–22 are fixed. The recent boundary findings and fixes
  are recorded in [the conformance report](docs/conformance-report.md).
- Final full audit 23 reports **0 confirmed violations** across §2–§16, with
  360 independently passing cases/controls. [Full report](docs/audits/audit-23.txt).
- Final gates pass: **82 tests** with PostgreSQL enabled (no skips), **145/145**
  conformance cases, warning-free Clippy, TLC models (1212/90 states), exact
  measurement checks and six checked/executed examples.
- [Acceptance record](docs/completion.md) maps every §15.3 completion requirement
  to its evidence. No required implementation work remains; future changes
  should rerun the affected checks and keep the report current.
- The normative contract is [spec.md](spec.md), especially §15.3. Optional
  fuzzing, Miri/sanitizers and a mechanically linked proof are future work,
  not acceptance requirements.
- The Rust package, library, executable, source extension, CLI/host/conformance
  formats and specification markers use `kiroha`/`Kiroha`/`KIROHA` consistently.
  There is no legacy format alias. The owner created the `kiroha-lang` GitHub
  organization; the repository is https://github.com/kiroha-lang/kiroha,
  with `origin` configured and default branch `main`.
- CI is defined in `.github/workflows/verify.yml`, including PostgreSQL 16,
  Clippy, conformance, TLC and reproducible token measurement. The
  [hosted verification](https://github.com/kiroha-lang/kiroha/actions/runs/40269236156)
  and [release CI](https://github.com/kiroha-lang/kiroha/actions/runs/40269471769)
  succeeded on the released source `d992c16`.
- `install.sh` and `.github/workflows/release.yml` provide verified prebuilt
  releases for Linux/macOS x86_64/ARM64. [v0.3.0 is public](https://github.com/kiroha-lang/kiroha/releases/tag/v0.3.0).
  Latest and pinned public installers were executed successfully, including
  exact-decimal execution and all 145 conformance cases. See
  [installation](docs/install.md) and the [publication record](docs/completion.md#public-distribution).

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
Time values/absolute deadlines use checked capacity (resource/clock on overflow).
File temp-name RNG failure is io/not_applied before creation. Final cleanup
shares one enclosing deadline, including cancellation joins,
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
