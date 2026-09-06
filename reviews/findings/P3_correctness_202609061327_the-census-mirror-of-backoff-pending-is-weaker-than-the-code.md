---
id: SWEEP-FOLD-OUTCOME-CENSUS-BACKOFF-MIRROR
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/census.rs:1131
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/census.rs`, which owns the mirror; the census family is not on the `standards/SWEEP.md` queue yet, so this is owed to whichever change next touches that oracle
---

## Failure sequence

`RunState::backoff_pending` (`src/topology/fold/outcome.rs`) reads `self.deferred_tasks`, the set
`set_state` maintains. That set deliberately survives a task's move to `AwaitingInput`:
`set_state`'s `AwaitingInput` arm removes nothing, and `refresh_task_state` restores `Deferred`
from it once the question is answered. This implements DESIGN §26 in as many words — "Execution
backoff remains pending beneath a question. An elapsed wait or resume consumes it once, including
hidden waits, without closing questions."

The census's mirror of the same predicate (`src/topology/census.rs:1131`) reads
`fold.task_state(*key) == Some(TaskState::Deferred)` instead. For a task that was deferred and is
now parked the two disagree: the code says backing off, the mirror says not.

Measured at `ee5dc81f`. One task deferred by a real settlement, then a `question_raised` on it —
which `check_question_raised` explicitly admits for a `Deferred` task — and the rest of the plan
merged:

```
state=AwaitingInput backoff_pending=true questions_open=true outcome=NotEnding
run_finished(parked) refused: the outcome derived from durable state is not ending
```

The code is right and the mirror is wrong, so nothing is miscomputed today. What is lost is the
oracle. In that state the census's assertion chain reaches neither the `backoff_pending` arm nor
`complete_shape`, and the outcome-side `Parked` arm — which asserts `!backoff_pending(fold)` — is
not reached either, because the code answers `NotEnding`. So a mutation that deletes
`!self.deferred_tasks.is_empty()` from `RunState::backoff_pending` turns this state into
`Ending(Parked)` and the census then asserts the mirror's `!backoff_pending`, which is already
false — and passes. The oracle agrees with the mutant it exists to catch.

`topology::fold::tests::pending_backoff_blocks_parked_and_complete_and_never_blocks_halted_or_budget`
does catch that mutation, through `grid_state`, which reaches `Backoff::DeferredTask` by
`run.set_state(MID, TaskState::Deferred)` — the state where the two definitions agree. So the
class is covered and only the census's independence is not.

## What the change that takes this up should do

Make the mirror say what §26 says: derive it from the fold's own deferred set rather than from the
task state, or, if the census is to keep reading only the public accessors, from
`task_state == Deferred` *or* an `AwaitingInput` task that has an unelapsed wait — and then add a
census state that reaches the disagreement, since `CensusBounds` did not explore one.

A mirror that is a paraphrase rather than a second derivation is worth naming as a class while
there: the census's value is that it computes the answer a second way, and a second way that is
narrower than the first is a control that passes for the wrong reason.
