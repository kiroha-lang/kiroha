# Work with databases

Kiroha uses the same three functions for PostgreSQL, MySQL and MariaDB:
`db` selects a configured pool, `sql` runs a parameterized query, and `tx`
commits or rolls back a callback. Choose the database in the host file.
Your program does not import a driver or manage connection lifetimes.

## Choose a pool

This host file allows a pool named `main`, backed by MySQL:

```json
{
  "format": "kiroha-host/1",
  "capabilities": {"databases": ["main"]},
  "database_pools": {
    "main": {
      "backend": "mysql",
      "dsn_env": "APP_DATABASE_URL",
      "max_connections": 4
    }
  }
}
```

Use `"mariadb"` for MariaDB or `"postgresql"` for PostgreSQL. Omitting
`backend` keeps PostgreSQL as the default. Different aliases can use different
backends in the same host. The reference profiles are PostgreSQL 16,
MySQL 8.4 and MariaDB 11.4.

Set the connection details in the launch process, then run with the host file:

```sh
export APP_DATABASE_URL='mysql://app:password@127.0.0.1:3306/app'
kiroha run query.kiroha --json --host database.host.json
```

MariaDB accepts `mysql://` and `mariadb://` URLs. PostgreSQL uses its native
connection string, for example `host=127.0.0.1 user=app dbname=app`.
Use URL encoding for special characters in URL credentials. The DSN is read
by the host connector; it is not automatically exposed through Kiroha `env`.
See [host configuration](../host-config.md#database-pools) for TLS policy.

## Start with a value

Save this as `query.kiroha`. It works on all three backends:

```kiroha
type Greeting = {message:str};
d = db("main");
row = check(sql(d, q`select ${"Hello, Kiroha!"} as message`).rows.one(), Greeting);
row.message
```

Holes bind values. They never paste SQL text, so names containing quotes stay
data. Leave holes outside SQL quotes and comments. Table names, column names
and SQL keywords must be literal SQL; they cannot come from a hole.

The result is `{rows, count}`. Each row is a record. Use `check` to establish
the shape of columns before doing shape-specific work. Duplicate column names
are errors: give joined or calculated columns distinct aliases.

## Keep related writes together

Suppose an administrator has created an `audit(message)` table. Use InnoDB
for MySQL/MariaDB tables that need rollback. This example uses portable SQL:

```kiroha
d = db("main");
tx(d){t =>
  sql(t, q`insert into audit(message) values (${"started"})`);
  sql(t, q`insert into audit(message) values (${"finished"})`);
  "saved"
}
```

A successful callback commits. A callback failure rolls back transactional
writes. A failed `sql(t, ...)` poisons the transaction even if the callback
catches the error. Only queries using `t` join the transaction; queries using
`d`, HTTP calls and file writes remain independent.

A timeout cancels outstanding work and cleans up the lease. If a COMMIT was
sent but its acknowledgment was lost, the outcome is `unknown`. Kiroha does
not replay the callback. Check actual application state before deciding what
to do next; `db` alone never means a retry is safe.

## Know the small dialect differences

The connection API is shared; SQL and server types follow the selected
backend. PostgreSQL supports `MERGE`, `RETURNING`, `$n` driver markers and
`::type` casts. MySQL/MariaDB use `?` driver markers and `CAST(value AS type)`;
write Kiroha holes in every case, rather than supplying markers yourself.
Only one SELECT/INSERT/UPDATE/DELETE is allowed (PostgreSQL also MERGE).
DDL, REPLACE, session/transaction commands and SELECT INTO are rejected.

MySQL/MariaDB syntax must be accepted by sqlparser 0.63's MySQL dialect and
by the server. Some server extensions are outside this subset: for example,
use `CAST(value AS CHAR)` rather than `CHAR CHARACTER SET ...` in a cast.
Unsupported syntax fails before dispatch with `db`, phase `prepare`.

| Data | PostgreSQL | MySQL / MariaDB |
| --- | --- | --- |
| Integer | Exact number | Exact number, including unsigned 64-bit values |
| Decimal | `numeric` → exact number | `DECIMAL` → exact number |
| Boolean | `boolean` → bool | `BOOLEAN` / `TINYINT(1)` → integer 0/1 |
| Binary | `bytea` → Bytes | Binary strings/blobs and BIT → Bytes |
| JSON | `json/jsonb` → parsed JSON | MySQL native JSON → parsed JSON; MariaDB JSON → string |
| Date/time and float | Explicit conversion required | Explicit conversion required |

The adapter never converts exact numbers through a host float. On
MySQL/MariaDB, fractional or very large parameters bind as exact decimal
**text**. Use `CAST(${amount} AS DECIMAL(p,s))` when numeric semantics are
required. Server precision, rounding and JSON normalization still apply.
To preserve an original exact JSON representation, store `dump(value)` in a
text column. To read JSON the same way on MySQL and MariaDB, select
`CAST(doc AS CHAR)`, then `parse(check(row.doc, str))` and validate its shape.

Unsupported result types, including NULL-only or zero-row columns, raise
`db` with phase `decode`. Cast dates to text or choose an explicit decimal
conversion for a float. SQL capabilities do not replace server permissions;
the configured role remains responsible for access to tables and functions.

See the [database API](../reference/builtins.md#database) for options and
errors, and [specification §11](../../spec.md#11-databases) for the full contract.
