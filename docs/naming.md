# Kiroha (きろは)

The language is named **Kiroha**, combining its original working name Kiri
with *iroha*, the basics of a language. It keeps the short Japanese sound
while giving the language its own name.

The official GitHub organization is [kiroha-lang](https://github.com/kiroha-lang),
created by the language's owner on 2026-10-26 after the naming decision.
The source repository is [kiroha-lang/kiroha](https://github.com/kiroha-lang/kiroha).
The earlier account lookup returned HTTP 404 before the owner created it.
Public searches for `Kiroha programming language` found
no same-named language. Kiroha is also used outside language development,
including an unrelated [GitHub user](https://github.com/Kiroha).

## Moving from the working name

| Surface | Working name | Current name |
| --- | --- | --- |
| Language | Kiri | Kiroha |
| Cargo package, library, executable | `kiri` | `kiroha` |
| Source extension | `.kiri` | `.kiroha` |
| CLI protocol | `kiri-cli/1` | `kiroha-cli/1` |
| Host format | `kiri-host/1` | `kiroha-host/1` |
| Conformance format | `kiri-conformance/1` | `kiroha-conformance/1` |
| Specification markers | `KIRI_*` | `KIROHA_*` |
| Integration-test DSN variable | `KIRI_TEST_PG_DSN` | `KIROHA_TEST_PG_DSN` |

Update format strings in host files and fixture manifests, Rust imports and
scripts invoking the executable. Example files and documentation already
use the current names. Configured `dsn_env` names are arbitrary; rename the
environment variable and its configuration together if desired. The syntax,
51-function API, language version 0.3.0 and Web1 semantics are preserved.
Source filenames are accepted regardless of extension. There is no legacy
executable or old-format alias in this prepublication rename.

The local checkout may still be named `kiri`; directory names are not part
of the language or CLI contract.
