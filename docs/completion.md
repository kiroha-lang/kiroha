# Web1 acceptance record

## 0.8.0 validation and SQL syntax

Kiroha 0.8.0 replaces the global `check` function with `.as[Type]` and
`.as(schema)`, and adds typed and untyped SQL blocks. The current conformance
corpus contains 263 cases, including validation, SQL binding and compatibility
checks. The [migration guide](migration-0.8.md) explains the breaking change;
the database guide describes row validation and transaction behavior.

The dated 0.7.0 and earlier records below remain historical evidence.

## 0.7.0 shared database adapters (2026-10-26)

PostgreSQL 16, MySQL 8.4 and MariaDB 11.4 now use one pool/lease abstraction,
logical parameter/result types and `db`/`sql`/`tx` state machine. Host pool
`backend` defaults to `postgresql`. See the [database guide](guides/databases.md)
and spec §11.6 for SQL grammar and native type differences. PostgreSQL's
existing OID mapping and deterministic fixture format remain compatible.

Local release-mode verification: **104 Rust tests passed, 0 failed, 0 skipped**
against PostgreSQL **16.4**, MySQL **8.4.4** and MariaDB **11.4.5** on Linux
x86_64 / Rust **1.96.0**. Ten backend tests cover exact numbers/binary/Unicode,
JSON differences, real commit/rollback/poison, affected-row counts, timeouts,
post-cancel reuse, session reset, zero-row/NULL unsupported types, a stalled
handshake with one pool slot, mixed-profile parallel transactions, and a proxy
that drops a confirmed COMMIT acknowledgment (unknown outcome, exactly one
persisted row, no replay). MySQL TLS succeeds with a private trusted CA and
rejects both an untrusted chain and a mismatched IP identity.

The suite uses `KIROHA_TEST_PG_DSN`, `KIROHA_TEST_MYSQL_DSN`,
`KIROHA_TEST_MARIADB_DSN` and `KIROHA_TEST_MYSQL_CA`; unset variables explicitly
skip integration coverage and are insufficient acceptance. Local Docker Desktop
WSL integration was unavailable, so task-owned native server binaries were used.
Hosted CI provisions all three Docker services and a hostname-valid MySQL TLS
fixture. The test databases use nonempty passwords for MySQL/MariaDB.

Additional gates: **253/253** conformance cases, warning-free Clippy, **8**
installer tests, both TLC models (1212/90 distinct states), strict docs/internal
links, **29** executed beginner examples and exact specification measurements.
The compact card is **3156/3132** tokens. Plugin **0.3.0** packages **23**
inspected files; Codex **0.164.0** installs identical bytes and discovers the
skill without errors in temporary state. The plugin's PostgreSQL-specific DB
recipe is statically checked; real backend behavior is verified by runtime tests.
The historical audits below describe earlier revisions, not a new independent
audit of 0.7. Public distribution evidence follows.


### Public 0.7.0 distribution (2026-10-27)

[PR6](https://github.com/kiroha-lang/kiroha/pull/6) merged the database adapters
at source `8f52d533fb16f4f8d9099de9e705b7d7174e899c`, the immutable
[v0.7.0 tag](https://github.com/kiroha-lang/kiroha/releases/tag/v0.7.0).
[PR7](https://github.com/kiroha-lang/kiroha/pull/7) moved ARM builds from the
retiring macOS 14 runner to macOS 15 and added tagged-source recovery dispatch.
The [release run](https://github.com/kiroha-lang/kiroha/actions/runs/40344545672)
checked out the same tag in verification, all four native builds and publishing.
It passed the 104-test three-container suite and all 253 cases on Linux/macOS
x86_64/ARM64 before publishing the verified archives and installer.

The public **latest** and **pinned** installers were both executed into isolated
directories. Each installed **0.7.0**, passed **253/253** cases and committed a
transaction through each of PostgreSQL, MySQL and MariaDB from one source
program. No user installation was overwritten. The published
[database guide](https://kiroha-lang.github.io/kiroha/guides/databases/), host
configuration and branding pages return HTTP 200; all five served branding
images match canonical bytes. Both guide programs pass static checking, and
the first executes unchanged on all three servers. The public main marketplace
also installs plugin 0.3.0 with identical files and discovers its skill on Codex
0.164.0 without errors.

The superseded macOS-14 release attempts were canceled after queueing; they
are infrastructure retries, not passing release evidence. The tag was never
moved. The task-owned local test servers were shut down after verification.


## 0.6.0 lightweight types and simple generics (2026-10-26)

`type Point={x:num,y:num};` declares a structural type. Named function
parameters/callbacks are annotated, recursive results are explicit, and inference
stays local to bindings, contextual lambdas, nonrecursive results and call type
arguments. Missing information never becomes `any`. Generic aliases/functions
use `<T>`; concrete aliases bridge to runtime validation, and generic validators
receive `schema<T>` explicitly. See [types](types.md) and [migration](migration-0.6.md).

The frontend runs type checking before effects. Existing runtime-failure probes
use explicit dynamic annotations so runtime checks remain covered. Seven new
regression suites cover structural compatibility, generic calls, inference
limits, explicit dynamic typing, validation/scopes/variance, shared alias DAGs,
and every public native signature's arity/callback policy. The embedded corpus
adds 40 typed acceptance cases.

Local verification passes **93 tests with PostgreSQL enabled**, **253/253
conformance cases**, warning-free release Clippy, 8 installer tests and both TLC
models. The compact card is **3095/3072 tokens** (o200k_base/cl100k_base).
Documentation and plugin examples are migrated. Plugin **0.2.0** packages 22
inspected files; Codex 0.163.0 installs the exact bundle and discovers the skill
in temporary state. DB plugin execution is not claimed; real PostgreSQL behavior
is covered by the runtime integration suite. The dated records below remain
historical evidence for their respective revisions.


## 0.5.0 compact functions and collection members (2026-10-26)

Function headers no longer use `then`; `end fun name` delimits the definition,
so its following semicolon is optional. Conditional `if ... then` retains
`then`. Collection operations are receiver methods, for example
`xs.map{f}`, `xs.fold(0){f}` and `xs.parallel(4){f}`. Length is `x.size`.
The public surface is 31 global functions, 19 methods, one property and five
schema atoms. Old global collection names are removed. See
[migration](migration-0.5.md) for the complete mapping and record collisions.

The parser represents members independently of builtin names. Bound methods
hold an immutable receiver and an ordinary callable target; invocation shares
the existing arity/callback path. This provides a base for future extensions,
without adding user extension declarations in this release.

Validation covers compact bodies and separators, retained conditional syntax,
all receiver kinds, method aliases, record-field collisions, diagnostic
positions, callback preflight and observable receiver/argument/callback order.
Local verification passes **86 tests with PostgreSQL enabled**, **213/213
conformance cases** (38 added), eight installer tests and warning-free Clippy.
TLC explores 1212/90 states. The compact card measures 2797/2778 tokens; exact
measurement reproduction passes. The strict documentation build, 20 executable
beginner programs, 23 content pages, internal links and desktop/mobile browser
flows pass (1440×900 and 390×844; no page/console errors).

The dated 0.3 independent audit and
0.4 release records below remain historical evidence for those versions.

## 0.4.0 named-function revision (2026-10-26)

Named functions now use `fun name(params) then sequence end fun name`.
The parser requires the exact closing name, rejects old bare definitions,
and executes named bodies through the common sequence path. Specification,
generation card, fixtures, examples and current guides use the new syntax.
See [migration](migration-0.4.md).

Local verification: **83 PostgreSQL-enabled tests**, **175/175 conformance
cases**, including 30 new function cases and a parser delimiter/span
regression. The compact card measures **2638/2618 tokens**, within 4096.
The dated 0.3 audit and publication evidence below remains historical.

[v0.4.0](https://github.com/kiroha-lang/kiroha/releases/tag/v0.4.0) was published
from `b8314f3c4f1bb2825a8d62fede3502d3fbc7c952` after the
[release CI](https://github.com/kiroha-lang/kiroha/actions/runs/40271474229)
passed the full PostgreSQL-backed 83 tests, 8 installer tests, 175 cases,
Clippy, TLC and exact measurement reproduction. Each of the four native
Linux/macOS × x86_64/ARM64 builds passed all 175 cases, exact arithmetic and
system-Bash installer tests. All five release checksums match GitHub's stored
asset digests.

Public latest and pinned `curl | bash` installs into paths containing spaces
both reported `kiroha-rs/0.4.0`, evaluated the new `fun fac(n) then ... end fun fac`
definition at 20 as exact `2432902008176640000`, passed 175/175 cases, and
rejected bare definitions, mismatched end names and missing end markers.
Both installed Linux x86_64 binaries have SHA-256
`b7e94d60315471f0d00afbb09d57bea76abf8b7e2249eb2587e76ea223e264e6`.

## 0.3 baseline acceptance

Date: 2026-10-26. Implementation source: `7b79e8e5316f2825af4629f182dc4b815d83ffbb`.
The local checkout is `/home/mizushima/dev/kiri`; the language is **Kiroha**.
Final independent audit: **audit 23 completed with 0 confirmed violations**.
The [full subsection coverage and execution record](audits/audit-23.txt)
contains 360 independently passing cases/control records.

## Specification §15.3 requirements

| Requirement | Current evidence |
| --- | --- |
| Exact extraction of generation card, conformance corpus, measurement corpus and reproduction script | `scripts/measure.py --check` validates the four marker/fence contracts and exact measured content. I08 checks the CLI's compact payload. |
| Lexer, spans, grammar constraints and whole-program static resolution | All embedded lex/parse/check cases pass; prose regressions and independent subsection reviews cover §2 and §4. |
| Immutable values, exact numbers, closures, tail calls, schemas and data API | Embedded runtime cases, 13 independent-oracle properties, I07/I08, prose regressions and `tests/inventory.rs` pass. The runtime, normative inventory and generation card agree on all 51 functions and 5 schema atoms. |
| Tasks, scopes, deadlines and deterministic fixture host | Embedded traces/timing cases and `tests/fixtures/{cleanup,durations,unsigned}.json` pass. Shared cleanup, exact clock-capacity outcomes and zero-step cancellation have regression coverage. |
| Production HTTP/TLS, PostgreSQL and local effects | I01/I02/I03/I04/I06 pass with real local transports, TLS fixtures, PostgreSQL 16.15 and files. Four PostgreSQL tests execute with the DSN set; none are skipped. |
| Server routing, input/reply, ingress limits and shutdown | Four I05 tests pass, including fragmented metadata, body-size overflow, admission/resource responses, disconnect and SIGINT shutdown. |
| Required CLI, tagged values and exit codes | I08 passes, including all required commands, exact values, opaque containers, stderr isolation, source/config limits and static checking without evaluation. |
| All normative prose rules | Audits 1–22 findings fixed; the final full §2–§16 audit 23 reports 0 confirmed violations. Subsection coverage and independent execution results are preserved in `docs/audits/audit-23.txt`. |
| Every embedded case passes | **145 total, 145 passed, 0 failed, 0 skipped**. |
| Report records environment and hidden skips are absent | `docs/conformance-report.md` records compiler/OS/dependencies, PostgreSQL version, real integration IDs, commands and explicit skip behavior. The final run has **82 passed, 0 failed, 0 ignored**. |
| Comprehensive documentation and runnable examples | README, language guide, built-in/value/error references, CLI/host configuration, architecture, verification and migration notes are present. All six examples pass static checking; pure, HTTP, database and server examples were executed with their required setup. |
| New language name | **Kiroha (きろは)**; crate, executable, protocols, markers, extensions and examples renamed. The owner subsequently created `kiroha-lang`; the source repository is [kiroha-lang/kiroha](https://github.com/kiroha-lang/kiroha). |

## Reproduction

```sh
cargo build --release --locked
KIROHA_TEST_PG_DSN="host=127.0.0.1 port=55432 user=kiri dbname=postgres" cargo test --release --locked
timeout 60 target/release/kiroha conformance spec.md --json
cargo clippy --release --all-targets --locked -- -D warnings
timeout 60 ./formal/check.sh
python scripts/measure.py --check
```

The last command requires exactly `tiktoken==0.14.0`. Both generation-card
counts meet the 4096-token target: **2600** (`o200k_base`) and **2580**
(`cl100k_base`). Clippy has no warnings. TLC finds no violation in the
bounded Parallel model (1212 distinct states) or Tx model (90 states).
These hand-maintained models are not mechanically linked proofs of the Rust
implementation; finite tests and audits support conformance.

The release binary reviewed by audit 23 has SHA-256:

```text
b21b3a8ae325fbcba95ac3d7744c1b9dc55e31998976281f2a3f3b1543e59047
```

The verification workflow provisions PostgreSQL 16 and runs these gates.
Its original syntax passed actionlint v1.7.7 locally. The subsequent publication
adds a native four-target release workflow and a checked `curl | bash` installer;
see [installation](install.md) for those distribution checks. Hosted workflow
results are available in [GitHub Actions](https://github.com/kiroha-lang/kiroha/actions).
Optional fuzzing, sanitizers, Kani
and mechanically linked proofs remain future work, outside §15.3 acceptance.

## Public distribution

Published on 2026-10-26: [Kiroha v0.3.0](https://github.com/kiroha-lang/kiroha/releases/tag/v0.3.0),
source `d992c16162236bbb0af27ae50d357f0f580b0e04`.
The [release workflow](https://github.com/kiroha-lang/kiroha/actions/runs/40269471769)
passed the PostgreSQL-backed 82 tests, 8 installer tests, Clippy, conformance,
TLC and token measurement. Each native Linux/macOS × x86_64/ARM64 build also
passed the 145 conformance cases, exact-decimal execution and system-Bash
installer tests before publication. The release contains four archives,
`install.sh` and `SHA256SUMS`; all five listed checksums match GitHub's stored
asset digests.

Both latest and pinned public `curl | bash` installations succeeded into paths
containing spaces. Each installed binary reported `kiroha-rs/0.3.0`, evaluated
`0.1 + 0.2` as exact `0.3`, and passed 145/145 conformance cases. The installed
Linux x86_64 binary has SHA-256
`b4d458b174c2e3baf49d0078d964b3da25f3695cd98187ce4925c5c2503e200b`.

Distribution work also removed CPU-speed assumptions from finite-work deadline
tests by repeating work until cancellation, with a direct expired-deadline
escaping regression. macOS builds use native protoc; Intel builds select the
macOS 15.2 SDK to retain the audited SQL parser and macOS 13 deployment target.
See [installation](install.md) for commands and source-build requirements.
