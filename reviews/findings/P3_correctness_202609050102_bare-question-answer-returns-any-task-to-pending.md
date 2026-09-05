---
id: PR153-FOLD-ANSWER-RETURNS-TO-PENDING
severity: P3
disposition: deferred
category: correctness
pr: 153
reviewed_sha: 807b6057bcfa5c6772b3969edd82881c47232277
location: src/topology/fold/apply.rs:499
provenance: pre_existing
first_bad:
guard: `src/topology/fold/apply.rs`, queue row 29 of `standards/SWEEP.md`; whoever takes that row inherits this, and the remedy is `OpenQuestion` recording the state a bare question parked its task from
---

## Failure sequence

A bare `question_raised` is accepted against any non-terminal task with no open generation, and
its answer returns the task to `Pending` whatever state it was parked from. `apply_answer` derives
the return state from the question's origin alone — `VerificationPark` returns to `AwaitingMerge`,
everything else to `Pending` — and a bare question has no origin of its own.

Measured at `807b605` with a probe test built on the fold's own fixtures (not committed):

```
run_started
… ALPHA merged (generation 0, sequence 0)
task_dispatched(MID) → attempt_started → candidate_prepared → task_candidate_created
merge_verification_started(MID, sequence 1)
merge_rejected(MID; repair TaskKey(3), lineage rooted at MID)     # MID = AwaitingRepair
question_raised(MID)                                               # accepted; MID = AwaitingInput
question_answered(MID, Answered { option 0 })                      # MID = Pending
```

After the answer: `task_state(MID) = Pending` while its repair (task 3) is `Pending` and `ready`;
`ready(MID) = false`, because the lineage lease rooted at MID overlaps its predicted region; but
`plan_transition(task_dispatched(MID, generation 1))` is **accepted** — `check_dispatched` asks the
state and the generation, not the leases. So a conforming driver never re-dispatches the rejected
original (it asks `ready` first), but the fold's accepted-log set contains a log that dispatches
the original alongside its repair, and between the answer and the repair's merge the task reads
`Pending` in every reader when the design says a rejected task awaits its repair. The same round
trip from `AwaitingMerge` leaves a `Pending` task with a queued candidate, and from `Deferred`
ends the backoff early.

No run wedges and no lease leaks: the repair's `task_merged` satisfies the original and moves it
to `Merged`, and `structurally_admissible` never selects the original because `ready` is
lease-blocked. That is why this is P3 and not the P1/P2 shape #153 fixed, where the answer's
return state was applied to a task holding an open generation.

## What #153 closes, and what it does not

#153 refuses a bare `question_raised` against a task with an open generation and against a
terminal task. **It does not refuse the sequence above**: `AwaitingRepair`, `AwaitingMerge` and
`Deferred` are non-terminal states with no open generation, and the refusal admits them on
purpose — `engine/topology/select/tests.rs` parks a queued candidate's task with exactly this
event. So this row is a second consequence of the same unguarded input, and it is live after
#153, not closed by it. It fires only from a bare `question_raised`, which no schema-4 emitter
constructs today; the other three paths that open a question return their task to the state
they took it from.

## What the change that takes this up should do

Give `OpenQuestion` the state its task was parked from, for a bare question, and have
`apply_answer` return the task there; `Derived::Answer(QuestionOrigin)` already carries the two
returns the fold knows, and the bare question is the one that has neither. That is an `apply.rs`
change and belongs to the stream that holds the file. The `check_end.rs` alternative — refusing
the bare event against every state but `Pending` — would refuse the shape
`engine/topology/select/tests.rs` uses to park a queued candidate's task and was not taken here.

If a later pass labels this P2, the remedy above is in reach and needs no design sentence: it
restores the state the log already recorded rather than choosing a new one.
