# Errors and aborts

Normative text: spec §8.

## Three outcomes

| Outcome | Catchable by `try`? | Representation |
| --- | --- | --- |
| Normal result | – | A value |
| Ordinary failure | yes | Record `{code, message, data}` |
| Abort | **no** | Control outcome with reason `cancelled`, `deadline` or `resource` |

Lex/parse/check diagnostics happen before evaluation and are never values.
Messages are diagnostic text and may change; branch on `code` and `data`.

## Runtime error codes

| code | Required `data` | Raised by |
| --- | --- | --- |
| `type` | `operation, position, expected, actual` (`position`: 1-based argument, 0 for receiver/callee/result, or option name) | wrong value kind anywhere |
| `arity` | `operation, min, max, got, callback, reason` — `reason`: `positional`, `missing_callback`, `unexpected_callback`, `callback_signature` | calls |
| `argument` | `operation, reason` | right kind, bad value; unknown options |
| `missing` | `key` | strict record access, `pick` |
| `bounds` | `index, length` | strict array access |
| `number` | `reason`: `division_by_zero`, `invalid_syntax` | `/`, `%`, `number` |
| `json` | `reason, offset` | `parse`, `get`, `post` |
| `utf8` | `offset` | `decode`, `get`, `post` |
| `schema` | `path, reason` | schema construction/normalization |
| `validation` | `path, expected, actual` | `check` |
| `cardinality` | `expected: 1, actual` | `one` |
| `network` | `operation, reason, outcome` (`not_sent` / `unknown`) | HTTP |
| `tls` | `operation, reason, outcome: "not_sent"` | HTTPS handshake/certificate |
| `timeout` | `operation, timeout` (+ `outcome` for remote effects) | `http`, `sql`, `tx` |
| `limit` | `operation, limit, maximum` | `max_bytes`, `max_rows` |
| `http` | `status` | `get`, `post` non-2xx |
| `pagination` | `reason, pages, items` | `pages` |
| `db` | `phase, outcome` (+ `sqlstate`, `reason`) | `db`, `sql`, `tx` |
| `input` | `reason` (+ `path, expected, actual`) | `input` |
| `media` | `reason` | `input` |
| `env` | `name` | `env` |
| `io` | `operation, reason, outcome` (`not_applied` / `applied` / `unknown`) | files, `serve` bind |
| `permission` | `operation, capability` | any effect without its capability |

User code may raise any nonempty code with `fail`. Codes are not types: a
handler that raises `input` produces a 400 response, but nothing else trusts
a code string.

`db.phase` values: `pool`, `prepare`, `bind`, `execute`, `decode`, `begin`,
`commit`, `rollback`, `ownership`. `db.outcome`: `not_sent`, `rolled_back`,
`committed`, `unknown` — a snapshot of the evidence when the error was
created.

## Frontend diagnostics

| Phase | Codes |
| --- | --- |
| `lex` | `lexical` |
| `parse` | `syntax`, `duplicate_key` |
| `check` | `name`, `duplicate_binding` |

The CLI reports the first failing phase with a source span
`{start, end, line, column}` (byte offsets, end exclusive; columns count
scalars).

## Aborts

| Reason | Causes |
| --- | --- |
| `cancelled` | SIGINT/SIGTERM, harness cancellation, a client disconnect (request scope), sibling failure in `parallel` (child scope) |
| `deadline` | `limits.deadline_ms` (root), server request timeout (request scope → 504) |
| `resource` | `eval_steps`, `memory_bytes`, `max_tasks`, `max_file_bytes`, `header_bytes` |

An abort stops all user code in the aborted scope and its descendants —
including pure infinite loops — and bypasses every `try`. Cleanup (joining
tasks, rolling back transactions, releasing connections, removing temp files)
still runs within `cleanup_ms`. Completed external effects are not undone.

Two scope timeouts are *ordinary* errors at their boundary: an operation
timeout (`http`, `sql`) after its cleanup, and a `tx` timeout once the whole
callback scope has been stopped and rolled back — both catchable by a live
caller.

## Precedence

1. Frontend diagnostics precede any evaluation.
2. Evaluation order is strict left to right: callee, then arguments, then the
   callback closure, then the callee type and arity checks.
3. Builtins validate arguments, options, callback signature, then elements,
   before any work.
4. An abort observed at a checkpoint dominates a pending result or error.
5. In `parallel` the first *observed* child failure wins.
6. In `tx`: ancestor abort > tx timeout > first latched SQL failure > other
   callback failure. Cleanup failures never replace a primary failure.
