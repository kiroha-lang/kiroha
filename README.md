# Kiri

Kiri is a small, strict, immutable expression language for programs that
manipulate JSON, call web services, serve HTTP APIs and use PostgreSQL. It was
designed to be written by LLMs: the whole callable surface is 51 prelude
functions, numbers are exact decimals, every effect is explicit and
capability-checked, and there is exactly one way to pass a callback — a
mandatory trailing lambda.

```kiri
User = open({id:int, name:str, active:bool});

get("https://api.example/users")
  | check([User])
  | filter(){u => u.active}
  | map(){u => {id:u.id, name:u.name}}
```

This repository is a complete Rust implementation of the
**Kiri 0.3.0 / Web1** specification ([`spec.md`](spec.md)), including the
`kiri` command-line tool, a deterministic conformance runner and production
adapters for HTTP/1.1 (+ verified TLS), PostgreSQL 16, an HTTP server and the
local filesystem.

## Status

| Area | State |
| --- | --- |
| Embedded conformance corpus (spec §16) | **145 / 145 pass** (`kiri conformance spec.md --json`) |
| Integration scenarios I01–I08 (spec §15.3) | All pass (`cargo test`, PostgreSQL 16.15 for I03/I04) |
| Extra prose-rule regressions | Pass (`tests/prose_regressions.rs`) |
| Formal verification | TLA+ models of `parallel` and `tx` model-checked with TLC; property-based tests of decimals and JSON against oracles ([details](docs/formal-verification.md)) |
| 51 prelude functions + 5 schema atoms | Implemented; inventory matches the generation card |
| CLI protocol `kiri-cli/1` | Implemented |

Details, versions and known deviations are in
[docs/conformance-report.md](docs/conformance-report.md).

## Build and install

Requires Rust 1.88+ (tested with 1.96), a C compiler and libclang (for the
embedded PostgreSQL parser, `pg_query`; set `LIBCLANG_PATH` if it is not
found automatically).

```sh
cargo build --release
cp target/release/kiri ~/.local/bin/     # or anywhere on PATH
kiri --version
```

## Quick start

```sh
echo 'fac(n)=if n<=1 then 1 else n*fac(n-1); fac(20)' > fac.kiri
kiri check fac.kiri --json      # lex, parse and resolve only
kiri run fac.kiri --json        # evaluate
# {"protocol":"kiri-cli/1","ok":true,"result":{"t":"number","v":"2432902008176640000"}}
```

By default a program may compute and `sleep` but every external capability
(network, files, environment, clock, randomness, logging, database, listening)
is denied. Grant capabilities with a host file:

```json
{
  "format": "kiri-host/1",
  "capabilities": {
    "http": ["https://api.example"],
    "log": true
  }
}
```

```sh
kiri run program.kiri --json --host host.json
```

See [examples/](examples/) for runnable programs.

## Documentation

| Document | Contents |
| --- | --- |
| [Language guide](docs/language-guide.md) | Tutorial: syntax, callbacks, scope, pipes, errors, schemas, concurrency, HTTP, SQL, servers |
| [Built-in reference](docs/reference/builtins.md) | All 51 functions and 5 schema atoms: signatures, defaults, validation, errors |
| [Values and numbers](docs/reference/values-and-numbers.md) | Value kinds, exact decimals, canonical spelling, JSON, equality |
| [Errors and aborts](docs/reference/errors.md) | Error codes and data, aborts, precedence |
| [CLI](docs/cli.md) | Commands, envelopes, wire values, exit codes |
| [Host configuration](docs/host-config.md) | `kiri-host/1` capabilities, pools, limits |
| [Architecture](docs/architecture.md) | Implementation design: CEK machine, scheduler, adapters, memory accounting |
| [Conformance report](docs/conformance-report.md) | Test results, environment, deviations |
| [Formal verification](docs/formal-verification.md) | TLA+ models (scheduler, transactions) and property-based tests (decimals, JSON) |
| [Specification](spec.md) | The normative Kiri 0.3.0 / Web1 contract |
| [Handoff](HANDOFF.md) | Current state, open work, verification and audit workflow for the next maintainer |

## Development

```sh
cargo test                                  # unit + integration tests
KIRI_TEST_PG_DSN="host=127.0.0.1 port=55432 user=kiri dbname=postgres" cargo test   # include PostgreSQL I03/I04
cargo run --release -- conformance spec.md --json
./formal/check.sh                           # TLA+ model checking (needs Java)
```

Without `KIRI_TEST_PG_DSN` the PostgreSQL tests print `SKIP` and pass
vacuously; the conformance report states which environment was used.
