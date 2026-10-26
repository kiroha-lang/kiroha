# Handoff: Kiroha 0.7.0 / Web1

## Database adapters (0.7, published)

PostgreSQL 16, MySQL 8.4 and MariaDB 11.4 share `db`/`sql`/`tx`, normalized
`SqlType`/`SqlParam`/`RawRows`, pool leases and the transaction state machine.
Host pool `backend` defaults to `postgresql`; `mysql` and `mariadb` select
MySQL-protocol connections and their statement validator. See the
[database guide](docs/guides/databases.md), spec §11.6 and
`tests/database_backends.rs`. Plugin 0.3 includes the same guide.

The implementation is merged through PR6 at `8f52d53`, also immutable tag
`v0.7.0`. PR7 (`96bfe21`) repairs the release workflow without changing the
language source: macOS 15 ARM, Xcode 16.2 for both Mac architectures, and an
optional `release_tag` dispatch input that checks out that tag in every job.
Release 40344545672 verifies and publishes the original tagged source.

104 release tests pass with PostgreSQL 16.4, MySQL 8.4.4 and MariaDB 11.4.5,
without skips; 253 conformance cases pass. Ten backend tests include exact
values, JSON differences, cleanup, poisoned transactions, canceled handshakes,
session reset, lost COMMIT acknowledgment, mixed aliases and verified TLS
with rejection of bad chains/identities. Clippy, both TLC models, installer,
docs/examples and the Codex 0.164.0 plugin installation/discovery checks pass.
Four native release targets pass; public latest/pinned installations both
pass all cases and three-backend transactions. See
[publication evidence](docs/completion.md#public-070-distribution-2026-10-27).

No database implementation/publication work remains. The task-owned native
servers from `HANDOFF-codex.md` were stopped after verification. That file is
an archived pre-commit handoff; the current source and evidence supersede it.
Local Docker Desktop WSL integration was unavailable; CI provisions all three
Docker services with nonempty MySQL/MariaDB passwords and a private TLS CA.
Do not run `git worktree prune` while the moved linked-worktree pointer remains
unrepaired. The primary checkout is `/home/mizushima/dev/kiri` (main); the
linked checkout used for this work is `/home/mizushima/dev/kiroha`.

## Current revision and contribution workflow

Use `fun greet(name:str) ... end fun greet` with no header `then` or required
separator after its end. Keep `if ... then`. Collections use receiver methods
and `.size`; see [the 0.5 migration](docs/migration-0.5.md). The receiver registry
and common bound-call path prepare future extensions without exposing an
extension declaration syntax. Current inventory: 31 globals, 19 methods,
one size property and five schema atoms; 253 embedded cases.

**Do not push directly to main.** Make changes on a working branch, open a
pull request, wait for checks, then merge. Documentation examples are checked
against the runtime built from the same revision, so breaking syntax changes
can be reviewed before a release exists. The sections below preserve the
previous publication and independent-audit evidence.

## Lightweight type checking (0.6)

Use explicit `type Name = ...;` declarations, named parameter/callback annotations,
and result annotations on recursion. Inference is local to bindings, contextual
lambdas, nonrecursive results and generic calls. No implicit `any` or automatic
generalization. See [types](docs/types.md) and [migration](docs/migration-0.6.md).
Concrete data aliases reify as shared validation schemas; generic parameters are
erased, so generic validators accept `schema<T>` explicitly. HTTP/parse return
unknown `json`, and SQL columns need validation before shape-specific use.

Local release verification: 93 tests with PostgreSQL, 253 conformance cases,
warning-free Clippy, 8 installer tests, both TLC models, exact measurements,
documentation examples and plugin 0.2 package/install/discovery checks pass.
Historical sections below retain their original versions and run evidence.

## Codex plugin

The skills-only plugin in `plugins/kiroha` provides `kiroha:write-kiroha` for
Kiroha 0.6.0; plugin version is independently 0.2.0. Register the repository's
`.agents/plugins/marketplace.json` using `codex plugin marketplace add kiroha-lang/kiroha`,
then `codex plugin add kiroha@kiroha-lang`. See [the guide](docs/codex-plugin.md).

The canonical generation card/API/host/migration references are synchronized by
`scripts/plugin.py sync`; CI rejects drift, checks all eight source examples,
executes pure/HTTP/pagination/file/service fixtures, and inspects a reproducible
ZIP. The DB example is checked statically only. `scripts/check_plugin_codex.py`
verified marketplace install, all installed bytes and actual `skills/list`
discovery on Codex CLI 0.163.0 in temporary state without model calls. No MCP
server or account connection is needed.

## Documentation website

The friendly English documentation is published at
https://kiroha-lang.github.io/kiroha/ (29 generated HTML pages). It includes a guided
first program, functions, data, a local HTTP walkthrough, troubleshooting,
the complete reference and the specification. Search, copy buttons, dark mode
and mobile navigation were verified on the public site at 1440×900 and 390×844.

The [Pages workflow](https://github.com/kiroha-lang/kiroha/actions/runs/40274604867)
passed the strict build, internal-link checks and 20 executable beginner
examples, then deployed source `26fb7b5`. The language verification also
passed. Sources live in `docs/`; `spec.md` and `examples/README.md` are included
at build time without duplicate copies. See [documentation contribution](docs/contributing-docs.md)
for preview and validation commands. Build dependencies are pinned in
`requirements-docs.txt`; generated `site/` output is ignored.

## Historical 0.4 syntax revision

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

Requires Rust 1.89+ (tested 1.96), a C compiler/libclang, Java 25 for TLC,
and Python with exactly `tiktoken==0.14.0` for measurement. Full integration
verification requires PostgreSQL 16, MySQL 8.4 and MariaDB 11.4. The Verify
workflow provisions these as Docker services and generates a private MySQL
TLS CA/certificate with localhost/127.0.0.1 identities.

For local verification, first provision those fixtures and export all four
variables: `KIROHA_TEST_PG_DSN`, `KIROHA_TEST_MYSQL_DSN`,
`KIROHA_TEST_MARIADB_DSN`, and `KIROHA_TEST_MYSQL_CA` (the private fixture CA
PEM path). The network fault-injection tests expect loopback TCP endpoints.
Then run:

```sh
cargo build --release --locked
cargo test --release --locked
timeout 60 target/release/kiroha conformance spec.md --json
cargo clippy --release --all-targets --locked -- -D warnings
timeout 60 ./formal/check.sh
python scripts/measure.py --check
```

Unset integration variables explicitly print SKIP and return; that is
insufficient acceptance. The task-owned local servers have been stopped.
Their native binary/data paths are archived in `HANDOFF-codex.md`; check
current process state before using them. `LIBCLANG_PATH=/usr/lib/llvm-15/lib`
works on this machine. `/tmp/kiroha-maint-env` contains the pinned documentation
and measurement dependencies used for this release.

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
