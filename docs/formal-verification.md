# Formal verification

Tests show behavior on the inputs they try. Two parts of Kiri are verified
more strongly: the concurrency and transaction state machines are
**model-checked** in TLA+ over every interleaving of a bounded model, and the
exact-decimal and JSON cores are checked with **property-based tests against
independent oracles**. Each verified property was also shown to catch a
deliberately injected bug ("mutation check"), so none is vacuously true.

| Area | Technique | Where | How to run |
| --- | --- | --- | --- |
| `parallel` scheduling (§9.1) | TLA+ / TLC model checking | [`formal/Parallel.tla`](../formal/Parallel.tla) | `./formal/check.sh` |
| `tx` lifecycle (§11.5) | TLA+ / TLC model checking | [`formal/Tx.tla`](../formal/Tx.tla) | `./formal/check.sh` |
| Decimals (§3.2–3.3), JSON (§3.4–3.5) | proptest with oracles | [`tests/properties.rs`](../tests/properties.rs) | `cargo test --release --test properties` |

## TLA+ models

The models are abstractions written to mirror the implementation's transition
code; the correspondence is maintained by hand (comments in each module name
the Rust functions it models). They verify the *design* of those state
machines for every interleaving; they do not mechanically prove the Rust code.

### `Parallel.tla` — `parallel(xs, n){f}`

Mirrors `create_group` / `start_child` / `child_done` in `src/runtime/mod.rs`.
Each child may succeed, fail, or hold forever; an ancestor abort may arrive at
any time.

| Property | Meaning |
| --- | --- |
| `BoundedActive` | At most `n` callbacks are active, counting ones being cancelled |
| `OrderedStarts` | Starts go to ascending indices; no index starts twice |
| `NoStartAfterStop` | Nothing starts after a failure is latched or an abort is requested |
| `JoinBeforeResume` | The parent resumes only after every started child is joined |
| `CompleteValue` | A normal result means every item succeeded |
| `ErrorIsLatched` | The raised error is the first observed failure; an ancestor abort dominates |
| `EventuallyResumes` (liveness) | If no child holds forever, the parent eventually resumes |

Checked with `N = 5` items and `n = 3`: 1,212 distinct states, no violation.

### `Tx.tla` — `tx(d){f}`

Mirrors `TxFrame` (`tx_effect`, `tx_ret`, `tx_throw`, `tx_abort`, `tx_finish`)
in `src/builtins/db.rs`. The callback may run failing statements (which latch
even if caught), return, or throw; a tx timeout or ancestor abort may arrive in
any non-cleanup phase; COMMIT may be confirmed, rejected or lose its
acknowledgment; ROLLBACK may succeed or fail.

| Property | Meaning |
| --- | --- |
| `NeverCommitPoisoned` | A transaction with a failed `sql(t)` is never committed |
| `NoRollbackAfterCommit` | ROLLBACK is never sent once COMMIT was dispatched |
| `NoReplay` | The callback runs at most once |
| `ConnectionAccounted` | Every acquired connection ends reset or discarded |
| `UncertainDiscards` | A lost or unconfirmed COMMIT discards the connection |
| `NeverReuseDirty` | A connection is reset for reuse only in a known ended state |
| `Precedence` | Ancestor abort > tx timeout > first SQL failure > other callback failure |
| `ValueMeansCommitted` | A value is returned only after a confirmed COMMIT |
| `HonestTimeoutOutcome` | A tx timeout's outcome never claims more than is known (`unknown` once COMMIT was sent; `rolled_back` only after a confirmed rollback) |
| `Completes` (liveness) | Once the callback has finished, the transaction completes |

90 distinct states, no violation.

### Mutation checks

Each injected defect is caught by the property named:

| Model | Injected defect | Caught by |
| --- | --- | --- |
| Parallel | Keep starting children after a failure | `NoStartAfterStop` |
| Parallel | Resume the parent before joining children | `JoinBeforeResume` |
| Tx | Commit even when poisoned | `NeverCommitPoisoned` |
| Tx | Roll back after COMMIT was dispatched | `NoRollbackAfterCommit` |
| Tx | Reuse a connection after a lost COMMIT | `UncertainDiscards` |
| Tx | Report `rolled_back` for a timeout during COMMIT | `HonestTimeoutOutcome` |
| Tx | Let a SQL failure outrank a tx timeout | `Precedence` |
| Tx | Re-run the callback after a lost COMMIT | `NoReplay` |

Two of these initially went undetected, which exposed observability gaps in
the first version of `Tx.tla` (a history variable recorded at the wrong time,
and `rollbackSent` tied to the previous phase); the model was corrected until
every mutant was caught.

## Property-based tests

`tests/properties.rs` generates random decimals with up to 60 significant
digits and exponents in ±40, and random JSON documents (nested arrays and
records, control characters, quotes, backslashes, non-ASCII and astral
characters in keys and strings). Each property runs 1,500–2,000 cases.

| Property | Oracle |
| --- | --- |
| Normalized form (coefficient not divisible by 10, zero is `0e0`) | Definition (§3.2) |
| Canonical spelling parses back to the same value; no `+`, no `-0`, no trailing fractional zero | `parse_json` round trip (§3.3) |
| `+`, `-`, `*` exact; `(a+b)-b = a` | Integer arithmetic at a common exponent |
| Comparison exact and antisymmetric | Integer comparison at a common exponent |
| `/` within half a unit of the 34th digit; ties to even | Cross-multiplied integer error bound |
| Ties constructed deliberately round to even | Expected 34-digit prefix computed directly |
| Quotients that fit in 34 digits are exact | `(q·b)/b = q` |
| `%` equals `a - trunc(a/b)·b`, `|r| < |b|`, sign of `a` | Integer truncating division |
| Zero divisors raise `division_by_zero` | Definition |
| `parse(dump(v)) = v`; `dump` is idempotent; no raw control characters | Round trip (§3.4) |
| Equality ignores record order, is symmetric; equal values hash equally | Record-order reversal (§3.5) |
| Every truncation of a valid document is rejected (except numeric prefixes) | Strict parser contract |

Mutation checks: breaking ties-to-even is caught by the constructed-ties
property (random inputs alone almost never hit an exact tie — that is why the
constructed generator exists); taking the absolute value of remainders is
caught by the remainder property.

## Why not Kani or a proof assistant

Kani (bounded model checking of the Rust code itself) was tried first for the
decimal module. It cannot analyse `num-bigint`, whose arithmetic uses inline
assembly (`_addcarry_u64`), which Kani does not support; verifying a separate
fixed-width re-implementation instead would not say anything about the real
code. Mechanised proofs (Coq/Lean) of the whole semantics would cost far more
than the rest of the project and still leave the code–proof correspondence
open. The combination above targets the two areas where independent audits
found the most defects.
