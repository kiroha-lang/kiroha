# Kiroha

**Documentation (GitHub Pages):** [kiroha-lang.github.io/kiroha](https://kiroha-lang.github.io/kiroha/)

**Kiroha (きろは)** combines the original working name Kiri with *iroha*, the basics of a language. The executable and source extension are `kiroha` and `.kiroha`. See [the naming and migration note](docs/naming.md).

Kiroha is a small, strict, immutable expression language for programs that
manipulate JSON, call web services, serve HTTP APIs and use PostgreSQL, MySQL or MariaDB. It was
designed to be written by LLMs: the standard surface is 31 global functions, 19 collection methods
and one `.size` property, numbers are exact decimals, every effect is explicit and
capability-checked, and there is exactly one way to pass a callback — a
mandatory trailing lambda.

```kiroha
type User = open({id:int, name:str, active:bool});

check(get("https://api.example/users"), [User])
  .filter{u => u.active}
  .map{u => {id:u.id, name:u.name}}
```

This repository is a complete Rust implementation of the
**Kiroha 0.7.0 / Web1** specification ([`spec.md`](spec.md)), including the
`kiroha` command-line tool, a deterministic conformance runner and production
adapters for HTTP/1.1 (+ verified TLS), PostgreSQL 16, MySQL 8.4, MariaDB 11.4, an HTTP server and the
local filesystem.

## Status

| Area | State |
| --- | --- |
| Embedded conformance corpus (spec §16) | **253 / 253 pass**, including explicit types, generics, compact definitions and receiver members (`kiroha conformance spec.md --json`) |
| Integration scenarios I01–I08 (spec §15.3) | All pass (`cargo test`, PostgreSQL 16.15 for I03/I04) |
| Extra prose-rule regressions | Pass (`tests/prose_regressions.rs`) |
| Formal verification | TLA+ models of `parallel` and `tx` model-checked with TLC; property-based tests of decimals and JSON against oracles ([details](docs/formal-verification.md)) |
| 31 globals + 19 methods + size + 5 schema atoms | Implemented; inventory matches the generation card |
| CLI protocol `kiroha-cli/1` | Implemented |
| Acceptance | 93 tests pass with PostgreSQL; compact syntax and members have dedicated regressions; the dated 0.3 baseline passed audit 23 ([record](docs/completion.md)) |

Details, versions and known deviations are in
[docs/conformance-report.md](docs/conformance-report.md).

## Install

```sh
curl -fsSL https://github.com/kiroha-lang/kiroha/releases/latest/download/install.sh | bash
export PATH="$HOME/.local/bin:$PATH"
kiroha --version
```

Prebuilt releases support Linux/WSL (glibc 2.35+) and macOS 13+, on x86_64 and
ARM64. No Rust build environment is needed. The installer verifies SHA-256
and defaults to `~/.local/bin`; see [installation options](docs/install.md)
for version pinning, alternate directories and source builds.

## Build from source

Requires Rust 1.88+ (tested with 1.96), a C compiler and libclang (for the
embedded PostgreSQL parser, `pg_query`; set `LIBCLANG_PATH` if it is not
found automatically).

```sh
cargo build --release
cp target/release/kiroha ~/.local/bin/     # or anywhere on PATH
kiroha --version
```

## Codex plugin

Give an agent Kiroha's versioned generation card, API guidance and runnable examples:

```sh
codex plugin marketplace add kiroha-lang/kiroha
codex plugin add kiroha@kiroha-lang
```

Use `$kiroha:write-kiroha` to write, review, debug or migrate Kiroha programs.
See [the plugin guide](docs/codex-plugin.md) for usage, local development and packaging.

## Quick start

```sh
echo 'fun fac(n:int):int if n<=1 then 1 else n*fac(n-1) end fun fac fac(20)' > fac.kiroha
kiroha check fac.kiroha --json      # lex, parse, resolve and type-check
kiroha run fac.kiroha --json        # evaluate
# {"protocol":"kiroha-cli/1","ok":true,"result":{"t":"number","v":"2432902008176640000"}}
```

By default a program may compute and `sleep` but every external capability
(network, files, environment, clock, randomness, logging, database, listening)
is denied. Grant capabilities with a host file:

```json
{
  "format": "kiroha-host/1",
  "capabilities": {
    "http": ["https://api.example"],
    "log": true
  }
}
```

```sh
kiroha run program.kiroha --json --host host.json
```

See [the examples and their setup instructions](examples/README.md) for runnable programs.

## Documentation

| Document | Contents |
| --- | --- |
| [Language guide](docs/language-guide.md) | Tutorial: syntax, callbacks, scope, pipes, errors, schemas, concurrency, HTTP, SQL, servers |
| [Types and generics](docs/types.md) | Local inference, annotations, explicit `any`, structural types and simple generics |
| [0.6 migration](docs/migration-0.6.md) | Type declarations and checked function signatures |
| [0.5 migration](docs/migration-0.5.md) | Compact definitions, collection methods and `.size` |
| [0.4 migration](docs/migration-0.4.md) | `fun name(...) ... end fun name`, body sequences and reserved words |
| [Built-in reference](docs/reference/builtins.md) | All globals, collection members and 5 schema atoms: signatures, defaults, validation, errors |
| [Values and numbers](docs/reference/values-and-numbers.md) | Value kinds, exact decimals, canonical spelling, JSON, equality |
| [Errors and aborts](docs/reference/errors.md) | Error codes and data, aborts, precedence |
| [CLI](docs/cli.md) | Commands, envelopes, wire values, exit codes |
| [Installation](docs/install.md) | One-command installer, versions, platforms and release verification |
| [Host configuration](docs/host-config.md) | `kiroha-host/1` capabilities, pools, limits |
| [Architecture](docs/architecture.md) | Implementation design: CEK machine, scheduler, adapters, memory accounting |
| [Conformance report](docs/conformance-report.md) | Test results, environment, deviations |
| [Formal verification](docs/formal-verification.md) | TLA+ models (scheduler, transactions) and property-based tests (decimals, JSON) |
| [Specification](spec.md) | The normative Kiroha 0.7.0 / Web1 contract |
| [Acceptance record](docs/completion.md) | Requirement-by-requirement completion evidence and final independent audit |
| [Handoff](HANDOFF.md) | Completed state, verification and maintenance workflow |

## Development

```sh
cargo test                                  # unit + integration tests
KIROHA_TEST_PG_DSN="host=127.0.0.1 port=55432 user=kiroha dbname=postgres" cargo test   # include PostgreSQL I03/I04
cargo run --release -- conformance spec.md --json
./formal/check.sh                           # TLA+ model checking (needs Java)
python scripts/measure.py --check            # requires tiktoken==0.14.0
```

Without `KIROHA_TEST_PG_DSN` the PostgreSQL tests print `SKIP` and pass
vacuously; the conformance report states which environment was used.

MySQL and MariaDB use the same database abstraction (`backend` in a host
`database_pools` entry: `postgresql` (default), `mysql` or `mariadb`). Their
real-server tests in `tests/database_backends.rs` are enabled by DSNs and skip
without them:

```sh
KIROHA_TEST_MYSQL_DSN='mysql://user:pass@127.0.0.1:3306/db' \
KIROHA_TEST_MARIADB_DSN='mariadb://user:pass@127.0.0.1:3307/db' cargo test --test database_backends
```

The [verification workflow](.github/workflows/verify.yml) provisions PostgreSQL
16 and runs the build, tests, conformance corpus, warning-free Clippy, TLC
models and measurement check. Regenerate measurements after editing `spec.md`
with `python scripts/measure.py`.
