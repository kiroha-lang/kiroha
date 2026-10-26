# Host configuration (`kiroha-host/1`)

Normative text: spec §14.1. The host file is strict JSON (duplicate keys,
comments and BOM rejected). It configures the runtime; it is not visible to
Kiroha code. Every key — including nested ones — is closed: unknown keys are
configuration errors (exit 2).

```json
{
  "format": "kiroha-host/1",
  "filesystem_root": "/srv/task",
  "capabilities": {
    "http": ["https://api.example", "http://127.0.0.1:9000"],
    "listen": [{"host": "127.0.0.1", "port": 8080}],
    "databases": ["main"],
    "env": ["API_TOKEN"],
    "read": ["/srv/task/input"],
    "write": ["/srv/task/output"],
    "clock": true,
    "random": true,
    "log": true
  },
  "environment": {"API_TOKEN": "secret"},
  "database_pools": {
    "main": {"dsn_env": "KIROHA_PG_MAIN", "max_connections": 4}
  },
  "limits": {
    "source_bytes": 1048576,
    "memory_bytes": 268435456,
    "eval_steps": 10000000,
    "max_tasks": 1024,
    "max_file_bytes": 16777216,
    "header_bytes": 32768,
    "deadline_ms": null,
    "cleanup_ms": 1000
  }
}
```

Only `format` is required. Omitted fields default to: `filesystem_root` = the
launch directory, empty capabilities/environment/pools, and the limits shown.
**The default host denies every external capability** — pure computation and
`sleep` still work.

## Capabilities

| Key | Grants | Matching |
| --- | --- | --- |
| `http` | `http`, `get`, `post` | Exact origins `scheme://host[:port]` (no path/query/userinfo/wildcard); default ports normalized |
| `listen` | `serve` | Exact `{host, port}` pairs |
| `databases` | `db` | Exact pool aliases |
| `env` | `env` | Exact variable names |
| `read` / `write` | `read` / `write` | Absolute directory roots; descendants by path *components*, never string prefixes |
| `clock` | `now` | |
| `random` | `uuid` | |
| `log` | `log` | |

Duplicate entries are errors. No capability implies another.

## Environment

`environment` is the complete set of values `env` can return. Listing a name
in `capabilities.env` without a value makes `env(name)` raise `env`. The
process environment is never exposed implicitly.

## Database pools

Each alias has `dsn_env` (the *launch process* environment variable holding a
libpq-style DSN, read only by the connector) and optional `max_connections`
(default 4). Credentials and connection parameters stay in the DSN and never
appear in errors. Example:

```sh
export KIROHA_PG_MAIN="host=127.0.0.1 port=5432 user=app dbname=app password=..."
```

Connection TLS follows the DSN's `sslmode` (`disable`, `prefer` — the
default — or `require`); server certificates are verified against the same
trust store as HTTPS. Each
connection starts with UTF-8 client encoding, `standard_conforming_strings`
on and read-committed isolation, and is reset with `DISCARD ALL` before reuse
(or discarded if the reset fails).

## Limits

| Limit | Default | Meaning |
| --- | --- | --- |
| `source_bytes` | 1048576 | Maximum source size (checked before evaluation) |
| `memory_bytes` | 268435456 | Live heap budget for evaluation (see [architecture](architecture.md#memory-accounting)) |
| `eval_steps` | 10000000 | Semantic steps per root evaluation and per server request |
| `max_tasks` | 1024 | Simultaneously live tasks (root, parallel children, requests) |
| `max_file_bytes` | 16777216 | Per `read`/`write` payload |
| `header_bytes` | 32768 | Received HTTP header section size |
| `deadline_ms` | null | Root deadline from evaluation start |
| `cleanup_ms` | 1000 | Cleanup grace per scope (1–10000) |

Exceeding a runtime limit is a `resource` (or `deadline`) **abort**, never a
catchable error. `source_bytes` is checked by the CLI before evaluation and
fails with `phase:"cli"`, code `source_limit` (exit 2).

## TLS trust store

HTTPS verification uses the system trust store. Setting the process
environment variable `SSL_CERT_FILE` to a PEM bundle replaces it (useful for
private CAs and tests). There is no way to disable verification.
