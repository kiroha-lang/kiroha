-------------------------------- MODULE Tx --------------------------------
(***************************************************************************)
(* Model of `tx(d){f}` (spec §11.5) as implemented by src/builtins/db.rs   *)
(* (TxFrame: tx_effect / tx_ret / tx_throw / tx_abort / tx_finish).        *)
(*                                                                         *)
(* The callback may run any number of sql(t) statements that succeed or   *)
(* fail (failures latch the transaction even if caught), then return or   *)
(* throw.  A tx-scope timeout or an ancestor abort may arrive in any       *)
(* non-cleanup phase.  COMMIT may be confirmed, rejected by the server, or *)
(* lose its acknowledgment; ROLLBACK may succeed or fail.                 *)
(***************************************************************************)
EXTENDS Naturals

VARIABLES
  phase,          \* acquiring | beginning | callback | committing
                  \* | rollingback | releasing | done
  conn,           \* none | held | reset | discarded
  latched,        \* a failed sql(t) poisoned the transaction
  threw,          \* the callback raised an ordinary error
  timeout,        \* the tx scope deadline aborted the scope
  ancestor,       \* an ancestor abort arrived
  runs,           \* times the callback was invoked
  commitSent, rollbackSent,
  commitResult,   \* none | ok | rejected | lost
  server,         \* open | committed | rolledback | maybe  (database state)
  latchedAtCommit,
  timeoutOutcome, \* outcome reported with a tx timeout
  result          \* none | value | sqlerr | cberr | commiterr | acqerr
                  \* | beginerr | timeout | abort

vars == <<phase, conn, latched, threw, timeout, ancestor, runs, commitSent,
          rollbackSent, commitResult, server, latchedAtCommit, timeoutOutcome,
          result>>

Init ==
  /\ phase = "acquiring" /\ conn = "none"
  /\ latched = FALSE /\ threw = FALSE /\ timeout = FALSE /\ ancestor = FALSE
  /\ runs = 0 /\ commitSent = FALSE /\ rollbackSent = FALSE
  /\ commitResult = "none" /\ server = "open" /\ latchedAtCommit = FALSE
  /\ timeoutOutcome = "none" /\ result = "none"

U(v) == UNCHANGED v

\* ---- acquisition and BEGIN ------------------------------------------------
AcquireOk ==
  /\ phase = "acquiring"
  /\ phase' = ("beginning")
  /\ conn' = ("held")
  /\ UNCHANGED <<latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, commitResult, server, latchedAtCommit, timeoutOutcome, result>>

AcquireErr ==
  /\ phase = "acquiring"
  /\ phase' = ("done")
  /\ result' = ("acqerr")
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, commitResult, server, latchedAtCommit, timeoutOutcome>>

BeginOk ==
  /\ phase = "beginning"
  /\ phase' = ("callback")
  /\ runs' = (runs + 1)
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, commitSent, rollbackSent, commitResult, server, latchedAtCommit, timeoutOutcome, result>>

\* A failed BEGIN leaves the session state uncertain: discard.
BeginErr ==
  /\ phase = "beginning"
  /\ phase' = ("releasing")
  /\ result' = ("beginerr")
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, commitResult, server, latchedAtCommit, timeoutOutcome>>

\* ---- the callback ---------------------------------------------------------
SqlFail ==
  /\ phase = "callback"
  /\ latched' = (TRUE)
  /\ UNCHANGED <<phase, conn, threw, timeout, ancestor, runs, commitSent, rollbackSent, commitResult, server, latchedAtCommit, timeoutOutcome, result>>

\* Normal return: commit unless poisoned (tx_ret).
CbReturn ==
  /\ phase = "callback"
  /\ phase' = (IF latched THEN "rollingback" ELSE "committing")
  /\ rollbackSent' = (rollbackSent \/ latched)
  /\ commitSent' = (commitSent \/ ~latched)
  /\ latchedAtCommit' = (IF latched THEN latchedAtCommit ELSE latched)
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, runs, commitResult, server, timeoutOutcome, result>>

\* Uncaught error: roll back (tx_throw).
CbThrow ==
  /\ phase = "callback"
  /\ phase' = ("rollingback")
  /\ rollbackSent' = (TRUE)
  /\ threw' = (TRUE)
  /\ UNCHANGED <<conn, latched, timeout, ancestor, runs, commitSent, commitResult, server, latchedAtCommit, timeoutOutcome, result>>

\* ---- COMMIT ---------------------------------------------------------------
CommitOk ==
  /\ phase = "committing"
  /\ commitResult = "none"
  /\ commitResult' = ("ok")
  /\ server' = ("committed")
  /\ phase' = ("releasing")
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, latchedAtCommit, timeoutOutcome, result>>

CommitRejected ==
  /\ phase = "committing"
  /\ commitResult = "none"
  /\ commitResult' = ("rejected")
  /\ server' = ("rolledback")
  /\ phase' = ("releasing")
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, latchedAtCommit, timeoutOutcome, result>>

\* Lost acknowledgment: the server may or may not have committed.
CommitLost ==
  /\ phase = "committing"
  /\ commitResult = "none"
  /\ commitResult' = ("lost")
  /\ server' = ("maybe")
  /\ phase' = ("releasing")
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, latchedAtCommit, timeoutOutcome, result>>

\* ---- ROLLBACK (cleanup) ---------------------------------------------------
RollbackOk ==
  /\ phase = "rollingback"
  /\ server' = ("rolledback")
  /\ phase' = ("releasing")
  /\ timeoutOutcome' = (IF timeoutOutcome = "pending" THEN "rolled_back" ELSE timeoutOutcome)
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, commitResult, latchedAtCommit, result>>

\* Unconfirmed rollback: the connection is discarded (server ends the tx).
RollbackErr ==
  /\ phase = "rollingback"
  /\ server' = ("maybe")
  /\ phase' = ("releasing")
  /\ timeoutOutcome' = (IF timeoutOutcome = "pending" THEN "unknown" ELSE timeoutOutcome)
  /\ UNCHANGED <<conn, latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, commitResult, latchedAtCommit, result>>

\* ---- RELEASE and final outcome (tx_finish) --------------------------------
\* Reset only a connection in a known ended state; otherwise discard.
Known == \/ commitResult \in {"ok", "rejected"}
         \/ (rollbackSent /\ server = "rolledback" /\ ~commitSent)

Primary ==
  IF ancestor THEN "abort"
  ELSE IF timeout THEN "timeout"
  ELSE IF result # "none" THEN result
  ELSE IF latched THEN "sqlerr"
  ELSE IF threw THEN "cberr"
  ELSE IF commitResult = "ok" THEN "value"
  ELSE "commiterr"

Release ==
  /\ phase = "releasing"
  /\ conn' = (IF conn = "held" THEN (IF Known THEN "reset" ELSE "discarded") ELSE conn)
  /\ phase' = ("done")
  /\ result' = (Primary)
  /\ UNCHANGED <<latched, threw, timeout, ancestor, runs, commitSent, rollbackSent, commitResult, server, latchedAtCommit, timeoutOutcome>>

\* ---- scope aborts (tx_abort) ----------------------------------------------
\* Phase after the abort, per tx_abort: acquiring -> done (no connection);
\* beginning -> release (discard); callback -> rollback; committing -> release
\* (unknown COMMIT: no further SQL, discard).
AfterAbort ==
  CASE phase = "acquiring" -> "done"
    [] phase = "beginning" -> "releasing"
    [] phase = "callback" -> "rollingback"
    [] phase = "committing" -> "releasing"

TimeoutOutcomeFor ==
  CASE phase = "acquiring" -> "not_sent"
    [] phase = "beginning" -> "unknown"
    [] phase = "callback" -> "pending"
    [] phase = "committing" -> "unknown"

TxTimeout ==
  /\ ~timeout
  /\ ~ancestor
  /\ phase \in {"acquiring", "beginning", "callback", "committing"}
  /\ timeout' = (TRUE)
  /\ phase' = (AfterAbort)
  /\ rollbackSent' = (rollbackSent \/ AfterAbort = "rollingback")
  /\ timeoutOutcome' = (TimeoutOutcomeFor)
  /\ result' = (IF phase = "acquiring" THEN "timeout" ELSE result)
  /\ UNCHANGED <<conn, latched, threw, ancestor, runs, commitSent, commitResult, server, latchedAtCommit>>

AncestorAbort ==
  /\ ~ancestor
  /\ phase \in {"acquiring", "beginning", "callback", "committing"}
  /\ ancestor' = (TRUE)
  /\ phase' = (AfterAbort)
  /\ rollbackSent' = (rollbackSent \/ AfterAbort = "rollingback")
  /\ result' = (IF phase = "acquiring" THEN "abort" ELSE result)
  /\ UNCHANGED <<conn, latched, threw, timeout, runs, commitSent, commitResult, server, latchedAtCommit, timeoutOutcome>>

\* An ancestor abort during cleanup only changes the reported outcome.
AncestorDuringCleanup ==
  /\ ~ancestor
  /\ phase \in {"rollingback", "releasing"}
  /\ ancestor' = (TRUE)
  /\ UNCHANGED <<phase, conn, latched, threw, timeout, runs, commitSent, rollbackSent, commitResult, server, latchedAtCommit, timeoutOutcome, result>>

Next ==
  \/ AcquireOk \/ AcquireErr \/ BeginOk \/ BeginErr
  \/ SqlFail \/ CbReturn \/ CbThrow
  \/ CommitOk \/ CommitRejected \/ CommitLost
  \/ RollbackOk \/ RollbackErr \/ Release
  \/ TxTimeout \/ AncestorAbort \/ AncestorDuringCleanup

\* The callback may keep running statements; runtime and cleanup steps are fair.
Spec == Init /\ [][Next]_vars
        /\ WF_vars(Release) /\ WF_vars(RollbackOk \/ RollbackErr)
        /\ WF_vars(CommitOk \/ CommitRejected \/ CommitLost)

---------------------------------------------------------------------------
\* A poisoned (or timed-out) transaction is never committed.
NeverCommitPoisoned == server = "committed" => ~latched

\* ROLLBACK is never sent after COMMIT was dispatched (no rollback of a
\* confirmed or unknown commit).
NoRollbackAfterCommit == ~(commitSent /\ rollbackSent)

\* The callback is never replayed.
NoReplay == runs <= 1

\* Every acquired connection ends reset or discarded; uncertain states discard.
ConnectionAccounted ==
  phase = "done" => conn \in {"none", "reset", "discarded"}
UncertainDiscards ==
  (phase = "done" /\ (commitResult = "lost" \/ (commitSent /\ commitResult = "none")))
    => conn = "discarded"
NeverReuseDirty == conn = "reset" => server \in {"committed", "rolledback"}

\* Outcome precedence: ancestor abort > tx timeout > first SQL failure >
\* other callback failure.
Precedence ==
  phase = "done" =>
    /\ (ancestor => result = "abort")
    /\ (~ancestor /\ timeout => result = "timeout")
    /\ (~ancestor /\ ~timeout /\ latched /\ result \notin {"acqerr", "beginerr"}
          => result = "sqlerr")

\* A value is returned only after a confirmed COMMIT.
ValueMeansCommitted == result = "value" => (server = "committed" /\ commitResult = "ok")

\* A timeout's outcome never claims more than is known.
HonestTimeoutOutcome ==
  (phase = "done" /\ result = "timeout") =>
    /\ timeoutOutcome \in {"not_sent", "rolled_back", "unknown"}
    /\ (commitSent => timeoutOutcome = "unknown")
    /\ (timeoutOutcome = "rolled_back" => server = "rolledback")

\* Liveness: once the callback has finished, the transaction completes.
Completes == (phase \in {"committing", "rollingback", "releasing"}) ~> (phase = "done")
=============================================================================
