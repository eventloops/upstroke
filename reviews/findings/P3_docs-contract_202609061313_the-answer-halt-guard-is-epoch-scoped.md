---
id: SWEEP-CHECKEND-001
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/check_end.rs:73
provenance: pre_existing
first_bad:
guard: the sweep of src/topology/fold.rs (queue row 40) and src/topology/fold/apply.rs (queue row 29), which own the field and the notes this would move
---

## Failure sequence

`check_question_answered` refuses an answer while `halted_epoch == Some(self.epoch)`, so the
halt's refusal lapses at the next epoch -> a run halts (`halted_at` set, no `run_finished` yet),
the coordinator dies, an operator resumes it -> measured on `sweep/topology-fold-check-end` with
a probe test at this reviewed sha: after the resume `halted_at` is still `Some(TaskKey(1))`,
`derived_outcome()` is `Ending(Halted)`, and `plan_transition` on a `question_answered` returns
`Ok(())`; applying it moves the parked task from `AwaitingInput` to `Pending` inside a run whose
only legal continuation is `run_finished(Halted)`. Two things disagree with that. The sibling
refusal for the same condition, `check_defer_wait_elapsed` (src/topology/fold/check_attempt.rs:719),
tests `self.halted_at.is_some()` and so refuses permanently; and
docs/internals/topology/fold/tests.md states of the test that covers this line that "a
budget-stopped run ingests the answer after its resume, and a halted one never does, because
`halted_at` is never cleared", which the measurement above contradicts. The same file's next
section, and docs/internals/topology/fold.md's note on `halted_epoch`, both describe the
epoch-scoped behaviour the code has, so the notes contradict each other and one of them is
false. No sentence in design/ settles which is the contract: the budget half of the same
refusal is genuinely epoch-scoped, because a resume clears `budget_stop` and raising the ceiling
is the documented response to it, while a resume clears nothing about a halt. The run's outcome,
its admissibility and its replay determinism are unaffected either way, which is why this is P3
rather than P2; **if a later pass finds a state where the ingested answer changes an outcome or a
task's terminal state in a way replay does not reproduce, it is a P2 and is fixed at once.**

## What the change that takes this up should do

Settle the contract first, then make the code and both notes say the same thing. If the refusal
is permanent, the predicate becomes `self.halted_at.is_some()`, matching `check_defer_wait_elapsed`
word for word, and `halted_epoch` then has no reader at all: it is written in
src/topology/fold/apply.rs:180 and read only at src/topology/fold/check_end.rs:73, so leaving it
in place is a `dead_code` failure under `-D warnings` and it has to be removed from
`RunState` in src/topology/fold.rs, from `record_halt` in src/topology/fold/apply.rs, and from
the field's note in docs/internals/topology/fold.md. That is three files with three separate
sweep sessions live on them at this sha, which is why this sweep did not take it. If instead the
refusal is deliberately epoch-scoped, the fix is one sentence: correct the claim in
docs/internals/topology/fold/tests.md, and say there why a halt that is permanent for the
outcome is temporary for ingestion while the sibling refusal is not.
