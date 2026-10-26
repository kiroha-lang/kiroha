# Web1 acceptance record

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
| New language name | **Kiroha (きろは)**; crate, executable, protocols, markers, extensions and examples renamed. `docs/naming.md` records the rationale and dated `kiroha-lang` account lookup. No organization or remote was created. |

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
Its syntax passed actionlint v1.7.7 locally. No hosted CI run is claimed:
the repository has no configured remote. Optional fuzzing, sanitizers, Kani
and mechanically linked proofs remain future work, outside §15.3 acceptance.
