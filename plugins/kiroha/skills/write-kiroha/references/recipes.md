# Starting programs

Sources live in [assets/examples](../assets/examples/) relative to this skill.
They are small complete programs; adapt their inputs and permissions to the
task. `// =>` records the expected represented result, not the CLI envelope.
The packaged examples are checked by the repository's plugin verifier.

## JSON and collection work

- [summarize.kiroha](../assets/examples/summarize.kiroha): exact totals, named
  function, grouping, method chains, `.size` and ordered JSON output.
- [callbacks.kiroha](../assets/examples/callbacks.kiroha): required user callback
  slot, recursion, saved bound method and `.fold`.
- [validation.kiroha](../assets/examples/validation.kiroha): closed/open schemas,
  optional and nullable fields, optional access, validation errors and a
  record field colliding with `.size`.

Run these without a host file; they are pure.

## HTTP client, pages and concurrency

[fetch.kiroha](../assets/examples/fetch.kiroha) expects a local fixture at
`http://127.0.0.1:8765/items` returning
`[{"id":1,"price":1},{"id":2,"price":3},{"id":3,"price":4}]`.
Use [client.host.json](../assets/examples/client.host.json).

[pagination.kiroha](../assets/examples/pagination.kiroha) expects the same
origin. `/page/1` returns `{"items":[{"id":1}],"next":"http://127.0.0.1:8765/page/2"}`;
`/page/2` returns `{"items":[{"id":2}],"next":null}`. `/detail/1` and `/detail/2`
return `{"id":1,"name":"Tea"}` and `{"id":2,"name":"Cake"}`. Only `next:null`
ends pagination. Retry predicates are ordinary function data in the options
record; the operation itself is a trailing zero-argument callback.

For actual services, change the URL and matching host origin together; use
schemas from their real response contracts. Retry only failures appropriate
to the task's side effects; a timeout may have an unknown outcome.

## HTTP service

[service.kiroha](../assets/examples/service.kiroha) and
[service.host.json](../assets/examples/service.host.json) provide `GET /health`
and `POST /total` at `127.0.0.1:8080`. A JSON body such as
`[{"price":2.5,"quantity":2},{"price":4.2,"quantity":1}]` returns `{"total":9.2}`.
`serve` runs until cancelled; stop a local test process after exercising it.
`input` handles JSON decoding and schema validation. Route handlers return
JSON directly or a nominal `reply(status,value)`; a plain `{status:...}` record
is just response data.

## Databases

PostgreSQL, MySQL and MariaDB use the same `db`/`sql`/`tx` API. See the
[database guide](databases.md) before choosing backend SQL or result types.
The recipe below uses PostgreSQL-specific `RETURNING` syntax.

[database.kiroha](../assets/examples/database.kiroha) shows parameterized
insertion within `tx`, using [database.host.json](../assets/examples/database.host.json).
It expects a `notes` table with integer identity `id` and text `body`. Match
this to the real task's schema. Do not run the example against an arbitrary DB;
it inserts a row. Put the DSN in the launch process's `KIROHA_PG_MAIN` variable.
A query hole `${body}` binds a value, not SQL text or an identifier. The `tx`
callback receives a task-bound handle: use it sequentially in its owner task.

## Local JSON files

[files.kiroha](../assets/examples/files.kiroha) reads `input.json` and writes
`summary.json` under the configured filesystem root. Input fixture:
`[{"price":2.5,"quantity":2},{"price":4.2,"quantity":1}]`. Grant absolute read and
write directory roots for that actual task directory. `read` returns Bytes;
use `decode` then `parse`, and `dump` then `encode` before `write`.
See [host configuration](host-config.md).
