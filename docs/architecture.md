# Architecture

This document describes how this implementation realizes the Kiri 0.3.0 /
Web1 contract. Language-visible behavior is fixed by [`spec.md`](../spec.md);
everything here is an engineering choice.

```text
source bytes ─▶ lexer ─▶ parser ─▶ resolver ─▶ Program (arena, resolved)
                                                  │
                                         runtime::Runtime<H: Host>
                                  ┌───────────────┴────────────────┐
                           VirtualHost (conformance)        RealHost (CLI run)
                           virtual clock, fixtures,         tokio LocalSet, hyper+rustls,
                           traces, virtual server           tokio-postgres, httparse server,
                                                            openat/renameat files
```

## Source layout

| Path | Responsibility |
| --- | --- |
| `src/lexer.rs` | Strict UTF-8, tokens, numbers, JSON-escape strings, nested query literals |
| `src/parser.rs` | Recursive descent for the §2.2 grammar and its 12 constraints |
| `src/resolve.rs` | Static scopes, `name`/`duplicate_binding`, closure conversion |
| `src/ast.rs` | Arena AST with resolved variable references |
| `src/decimal.rs` | Exact decimals (BigInt coefficient, i64 exponent) |
| `src/json.rs` | Strict JSON parser and canonical serializer |
| `src/value.rs` | Value kinds, records, functions, error constructors, JSON equality/hash |
| `src/schema.rs` | Schema normalization and validation |
| `src/url.rs` | URL acceptance rules and the `url` constructor |
| `src/sqlcheck.rs` | Query translation, hole-context lexing and statement validation with libpg_query (PostgreSQL 16 grammar) |
| `src/runtime/` | Task engine (`mod.rs`), evaluator (`machine.rs`), frames |
| `src/builtins/` | The 51 functions: `pure`, `callbacks`, `effects`, `db`, `server`, helpers |
| `src/host/` | `Host` trait, config, path rules, virtual and real hosts |
| `src/conformance.rs`, `src/cli.rs`, `src/wire.rs` | Embedded test protocol, CLI, wire values |

## Frontend

The lexer produces tokens with byte spans. A query literal `q\`...\`` is
lexed into text segments plus, for each `${...}` hole, the full token list of
the hole (found by brace depth over tokens, so strings, records, comments and
nested queries inside holes work). The parser parses each hole as a complete
`Expr`.

The parser is hand-written recursive descent following the EBNF levels. The
non-context-free rules are implemented directly: definition/binding detection
by token lookahead, lambda-vs-record by checking for a lambda head followed by
`=>`, the "at most one comparison" rule by tracking whether each operand of
`==`/`!=` already contains an ungrouped comparison, the positional-lambda ban
by stripping `Group` nodes from each argument, and duplicate decoded record
keys.

The resolver performs **closure conversion**: each function context (top
level, lambda, definition) gets local slots; a reference to an outer variable
becomes a capture index, and the creating environment copies exactly those
values into the closure. A definition's own name resolves to `SelfFn` (the
running function), so recursion needs no mutable cell and creates no
reference cycle.

## Evaluator: an explicit-continuation (CEK) machine

Each task holds a control value (`Ctl`) and a heap-allocated continuation
stack (`Vec<Frame>`). One `step` either evaluates a node (pushing a frame for
pending work), delivers a value/error/effect result to the top frame, or
unwinds an abort. Consequences:

- **Proper tail calls.** Tail positions (§5.5) evaluate without pushing a
  frame, so loops run in constant continuation space; deep non-tail recursion
  only grows the heap.
- **Preemption.** The scheduler can stop a task after any step: the 1024-step
  cooperative quantum, `abort_after_steps`, and CPU-bound cancellation all
  fall out naturally.
- **Suspension inside callbacks.** Callback-taking builtins (`map`, `fold`,
  `sort`, `retry`, `pages`, `tx`, request handlers, ...) are frames that
  re-enter the machine, never Rust recursion — so `get` inside `map` simply
  suspends the task.

Step accounting follows §14: one step per entry into a non-`Group` node or a
`Bind`/`Define` item, two per pipe-path field, one per attached-lambda
creation. Steps are charged to a budget shared by a root (or request)
evaluation and all its `parallel` descendants.

Errors are `Ctl::Throw(record)` and unwind frames until a `Try` frame (or a
builtin frame that intercepts them, such as `retry` and `tx`). Aborts are a
separate `Ctl::Abort` that no `Try` frame handles.

## Task engine

- **Tasks** — the root, every `parallel` child and every server request.
  `tx` callbacks stay in the caller's task (required for ownership).
- **Scopes** — a tree of abort scopes: root → (tx | request | parallel child)
  → .... Aborting a scope marks it; tasks observe the outermost aborted
  ancestor at checkpoints (cached by a global abort epoch, so the check is
  cheap). Waiting effects inside an aborted scope are cancelled immediately.
- **Timers** — scope deadlines (root `deadline_ms`, request timeout, tx
  timeout), operation timeouts (`http`, `sql`), cleanup grace and harness
  cancellations live in one heap; at equal timestamps they run before host
  completions.
- **Effects** — a builtin returns `Ctl::Dispatch(effect)`; the scheduler
  checks for aborts, assigns an id and hands it to the host. The completion
  comes back as `Ctl::Effect(outcome)` to the waiting frame. Cleanup effects
  (rollback, release, close listener, timeout responses) are flagged so the
  abort being cleaned up does not cancel them, but they are bounded by
  `cleanup_ms`.
- **Parallel groups** start children in index order up to `n`, start the next
  index on each completion, latch the first observed failure, abort sibling
  scopes, and resume the parent only after every started child has finished.

### Transactions

`tx` is a frame-driven state machine: acquire → begin → callback (the task's
scope is switched to a fresh tx scope with the tx deadline) → commit or
rollback → release (reset or discard). `sql(t)` checks owner task and
liveness, then latches the first failure in the transaction state. The final
outcome follows the spec precedence; a tx-scope timeout is converted to an
ordinary `timeout` only if the enclosing scope is still live. An unconfirmed
COMMIT discards the connection without rollback.

### Server

`serve` validates and compiles routes, dispatches `Listen`, then parks the
root task. Each incoming request head is admitted in the §12.2 order
(target/method → inflight limit → request scope + deadline → body framing,
coding and size → route lookup → handler) as its own task with a fresh step
budget. Results, errors and request-scope aborts are mapped to responses in a
`Request` frame; a deadline sends `504` through a cleanup send. Cancelling the
root aborts all request scopes, waits for them, then closes the listener.

## Hosts

`Host` is a small trait: `dispatch(id, effect)`, `cancel(id)`, and
`wait(until)` returning the next event (effect completion, incoming request,
disconnect, root cancellation).

### VirtualHost (conformance)

Implements §16.3–16.5 exactly: virtual time starts at 0, completions are
events at `start + after_ms`, cancellation removes the ordinary completion,
records `id:cancel` and schedules `id:joined` after `cancel_ack_ms`. Fixtures
are matched by operation and recursive subset on the normalized arguments.
`sleep` uses automatic `sleep#n` timers. The virtual server delivers requests
at their times, records `receive`/`response` events and the response bodies.
Unmatched effects, deadlocks and unconsumed fixtures are harness failures.

### RealHost (production)

Runs on a tokio current-thread runtime inside a `LocalSet`; every operation is
a spawned future racing a `CancellationToken`, reporting `Joined` when
cancelled.

- **HTTP client** — one hyper HTTP/1.1 connection per request (no pooling,
  hence no hidden session state); a flag set just before request bytes are
  written lets a cancelled/timed-out request report `not_sent` precisely; rustls with the system or `SSL_CERT_FILE`
  trust store; no redirect/retry/cookie logic exists. Raw identity bodies are
  bounded by `max_bytes + 1`; gzip bodies by a host quota; decoding and
  `max_bytes` are enforced by the runtime on the decoded stream.
- **PostgreSQL** — tokio-postgres; parameters are sent in text format with the
  runtime's OIDs (OID 0 for NULL), results are received in binary and
  converted to exact text (numeric is decoded from base-10000 digits, never
  through floats). Pools are per alias with `max_connections`; release runs
  `DISCARD ALL` plus the baseline settings, otherwise the connection is
  dropped.
- **Server** — a tokio listener and an httparse request-head parser that keeps
  the raw target and header pairs; body framing (Content-Length/chunked) is
  decoded on request of the runtime with the `max_body` limit; a watcher
  reports client disconnects. One request per connection.
- **Files** — every path component is opened with `O_NOFOLLOW` relative to
  its parent directory handle; writes go to an exclusive random temp file in
  the target directory, get their mode set, and are installed with `renameat`.

## Memory accounting

`memory_bytes` uses a counting global allocator (`src/alloc.rs`) that tracks
live heap bytes of the process. The runtime records a baseline when it is
created and checks `live - baseline` every 512 semantic steps and every 4096
elements inside long builtin loops (e.g. `range`, `flat`, `sum`). An excess
aborts the root/request scope that is running when it is observed, with
`resource` / `{resource:"memory"}`. Shared immutable values (`Rc`) are
naturally counted once. Allocation overhead is host-dependent; within quota it
never affects values.

## Performance notes

Values are reference-counted and immutable; records switch to a hash index
above 12 fields; decimals normalize trailing zeros in 16-digit chunks.
Measured on the test machine (release build, WSL2): an infinite tail loop
consumed a 50,000,000-step budget in 4.6–9.5 s across six runs taken while
another build was running, i.e. roughly 5–11 million semantic steps per
second.
