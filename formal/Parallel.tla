---------------------------- MODULE Parallel ----------------------------
(***************************************************************************)
(* Model of `parallel(xs, n){f}` (spec §9.1) as implemented by             *)
(* src/runtime/mod.rs (create_group / start_child / child_done).           *)
(*                                                                         *)
(* Children are started in ascending index order, at most Lim active      *)
(* (running or being cancelled).  Each child may succeed, fail, or hold    *)
(* forever.  The first observed failure is latched, no further index is    *)
(* started, running siblings are cancelled; an ancestor abort cancels all  *)
(* running children.  The parent resumes only after every started child   *)
(* has finished (joined).                                                  *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS N,     \* number of items
          Lim    \* the bound n (positive)

ASSUME N \in Nat /\ Lim \in Nat \ {0}

Idx == 1..N

VARIABLES next,          \* next index to start
          st,            \* st[i]: idle | running | cancelling | ok | fail | aborted
          latched,       \* 0 or the index of the first observed failure
          parentAborted, \* ancestor abort requested
          result,        \* none | value | error | abort
          maxActive,     \* history: largest number of active children seen
          lateStart      \* history: a child started after a failure/abort

vars == <<next, st, latched, parentAborted, result, maxActive, lateStart>>

Active == {i \in Idx : st[i] \in {"running", "cancelling"}}
Started == {i \in Idx : st[i] # "idle"}

Init ==
  /\ next = 1
  /\ st = [i \in Idx |-> "idle"]
  /\ latched = 0
  /\ parentAborted = FALSE
  /\ result = "none"
  /\ maxActive = 0
  /\ lateStart = FALSE

\* A slot is free and nothing stops scheduling (start_child guard).
Start ==
  /\ result = "none"
  /\ next <= N
  /\ latched = 0
  /\ ~parentAborted
  /\ Cardinality(Active) < Lim
  /\ st' = [st EXCEPT ![next] = "running"]
  /\ next' = next + 1
  /\ maxActive' = IF Cardinality(Active) + 1 > maxActive
                    THEN Cardinality(Active) + 1 ELSE maxActive
  /\ lateStart' = (lateStart \/ latched # 0 \/ parentAborted)
  /\ UNCHANGED <<latched, parentAborted, result>>

ChildOk(i) ==
  /\ st[i] = "running"
  /\ st' = [st EXCEPT ![i] = "ok"]
  /\ UNCHANGED <<next, latched, parentAborted, result, maxActive, lateStart>>

\* First observed failure latches and cancels the running siblings.
ChildFail(i) ==
  /\ st[i] = "running"
  /\ IF latched = 0
       THEN /\ latched' = i
            /\ st' = [j \in Idx |->
                        IF j = i THEN "fail"
                        ELSE IF st[j] = "running" THEN "cancelling" ELSE st[j]]
       ELSE /\ latched' = latched
            /\ st' = [st EXCEPT ![i] = "fail"]
  /\ UNCHANGED <<next, parentAborted, result, maxActive, lateStart>>

\* A cancelled child acknowledges and is joined (its try handlers do not run).
Joined(i) ==
  /\ st[i] = "cancelling"
  /\ st' = [st EXCEPT ![i] = "aborted"]
  /\ UNCHANGED <<next, latched, parentAborted, result, maxActive, lateStart>>

AncestorAbort ==
  /\ ~parentAborted
  /\ result = "none"
  /\ parentAborted' = TRUE
  /\ st' = [j \in Idx |-> IF st[j] = "running" THEN "cancelling" ELSE st[j]]
  /\ UNCHANGED <<next, latched, result, maxActive, lateStart>>

\* The parent resumes when no child is active and no more will start.
Finish ==
  /\ result = "none"
  /\ Active = {}
  /\ (latched # 0 \/ parentAborted \/ next > N)
  /\ result' = IF parentAborted THEN "abort"
               ELSE IF latched # 0 THEN "error" ELSE "value"
  /\ UNCHANGED <<next, st, latched, parentAborted, maxActive, lateStart>>

Next ==
  \/ Start
  \/ \E i \in Idx : ChildOk(i) \/ ChildFail(i) \/ Joined(i)
  \/ AncestorAbort
  \/ Finish

\* Children may hold forever, so only the runtime's own steps are fair.
Spec == Init /\ [][Next]_vars /\ WF_vars(Finish) /\ WF_vars(Start)
             /\ \A i \in Idx : WF_vars(Joined(i))

---------------------------------------------------------------------------
TypeOK ==
  /\ next \in 1..(N + 1)
  /\ st \in [Idx -> {"idle", "running", "cancelling", "ok", "fail", "aborted"}]
  /\ latched \in 0..N
  /\ result \in {"none", "value", "error", "abort"}

\* At most n callbacks are active, including ones being cancelled.
BoundedActive == Cardinality(Active) <= Lim /\ maxActive <= Lim

\* Starts are assigned to ascending indices; no index starts twice.
OrderedStarts == \A i \in Idx : (st[i] # "idle") <=> (i < next)

\* The parent resumes only after every started child has been joined.
JoinBeforeResume == result # "none" => Active = {}

\* A normal result contains every item, all succeeded.
CompleteValue == result = "value" => \A i \in Idx : st[i] = "ok"

\* The latched (first observed) failure is the one raised; an ancestor
\* abort dominates it.
ErrorIsLatched ==
  /\ result = "error" => (latched # 0 /\ st[latched] = "fail" /\ ~parentAborted)
  /\ result = "abort" => parentAborted

\* After a failure is latched or an abort requested, nothing new starts.
NoStartAfterStop == ~lateStart

\* Liveness: if no child holds forever the parent eventually resumes.
EventuallyResumes ==
  (<>[](\A i \in Idx : st[i] # "running")) => <>(result # "none")
=============================================================================
