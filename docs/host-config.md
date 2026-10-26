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

Each alias has required `dsn_env`, optional `backend` (`postgresql`, `mysql`
or `mariadb`, default `postgresql`), and optional `max_connections` (default
4, positive integer). The launch process environment variable named by
`dsn_env` holds the trusted DSN, read only by the connector. Credentials and
raw connection errors are never exposed to Kiroha source or diagnostics.

```json
"database_pools": {
  "main": {"backend":"mysql", "dsn_env":"APP_DATABASE_URL", "max_connections":4},
  "archive": {"backend":"mariadb", "dsn_env":"ARCHIVE_DATABASE_URL"},
  "legacy": {"dsn_env":"KIROHA_PG_MAIN"}
}
```

For PostgreSQL, use a libpq-style connection string. TLS follows `sslmode`
(`disable`, `prefer` — the default — or `require`); TLS certificates and
hostnames are verified. Connections use UTF-8, `standard_conforming_strings`
on and read committed, and reset with `DISCARD ALL` plus the baseline.

For MySQL/MariaDB, use `mysql://user:password@host:port/database` (MariaDB
also accepts `mariadb://`). TCP addresses are honored. Add `?require_ssl=true`
for TLS with certificate and hostname verification. Trust roots include the
system and process `SSL_CERT_FILE`/`SSL_CERT_DIR`; trusted driver URL options
own any additional TLS policy. Sessions use `utf8mb4_bin`, strict SQL mode,
read committed and autocommit; reset clears session state and reapplies the
baseline, or discards the connection. Cancellation uses an authenticated
control connection to stop the query, then discards its interrupted lease.
Use InnoDB tables for rollback. The configured account must be able to
cancel its own queries.

See [the database guide](guides/databases.md) for a complete host file,
portable queries and backend type differences.

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

Durations and absolute monotonic deadlines beyond the implementation's u64
millisecond capacity abort evaluation with `resource` / `clock`; even a very
large root deadline is retained exactly until that check. Waits are never
shortened to fit the clock.

## TLS trust store

HTTPS verification uses the system trust store. Setting the process
environment variable `SSL_CERT_FILE` to a PEM bundle replaces it (useful for
private CAs and tests). HTTPS has no option to disable verification. Database TLS is controlled by the trusted DSN as described above.
