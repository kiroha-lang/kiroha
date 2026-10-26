# Running the examples

Build with `cargo build --release --locked`; the commands below assume
`target/release` is on `PATH` (or replace `kiroha` with
`./target/release/kiroha`). Run from the repository root.

## Pure programs

```sh
kiroha run examples/data.kiroha --json
kiroha run examples/recursion.kiroha --json
kiroha run examples/errors.kiroha --json
```

`data` groups and sums orders, `recursion` computes Fibonacci/Collatz results
with exact decimals, and `errors` returns both successes and caught failures.

## HTTP client

```sh
kiroha run examples/fetch.kiroha --json --host examples/fetch.host.json
```

The host permits only `https://api.github.com`. The program reads public
repository metadata, so results and availability depend on GitHub's API.

## PostgreSQL 16

Use a scratch database and set its DSN. Create the tables using `psql`:
DDL is an administrator action outside the language's SQL subset.

```sh
export KIROHA_PG_MAIN='host=127.0.0.1 port=55432 user=kiroha dbname=postgres'
psql "$KIROHA_PG_MAIN" <<'SQL'
CREATE TABLE IF NOT EXISTS kiroha_demo (
  id serial PRIMARY KEY,
  name text NOT NULL,
  meta jsonb NOT NULL
);
CREATE TABLE IF NOT EXISTS kiroha_demo_audit (
  demo_id integer NOT NULL REFERENCES kiroha_demo(id)
);
SQL
kiroha run examples/database.kiroha --json --host examples/database.host.json
```

Each execution inserts a demo row and its audit row in one transaction,
then reads the rows. Change the DSN's user/port/database for your server.

## HTTP server and files

```sh
mkdir -p /tmp/kiroha-notes/notes
kiroha run examples/server.kiroha --json --host examples/server.host.json
```

From another terminal:

```sh
curl http://127.0.0.1:8080/hello/ada
curl -X POST -H 'Content-Type: application/json' \
  -d '{"text":"hi"}' http://127.0.0.1:8080/notes
# Use the returned id:
curl http://127.0.0.1:8080/notes/RETURNED_ID
```

The host file grants listening on port 8080, clock/random/log capabilities,
and file access below `/tmp/kiroha-notes`. Stop with Ctrl-C; orderly shutdown
prints a cancelled-abort envelope and exits with status 3.
