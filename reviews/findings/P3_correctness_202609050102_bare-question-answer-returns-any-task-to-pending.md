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

A bare `question_raised` is accepted against a `Pending`, `Deferred` or queued `AwaitingMerge`
task with no open generation and no transaction open on it, and its answer returns the task to
`Pending` whatever state it was parked from. `apply_answer` derives the return state from the
question's origin alone — `VerificationPark` returns to `AwaitingMerge`, everything else to
`Pending` — and a bare question has no origin of its own. The live case is `AwaitingMerge`:

```
run_started
task_dispatched(ALPHA, g0) → attempt_started → candidate_prepared → task_candidate_created
                                                   # ALPHA = AwaitingMerge, g0 queued
question_raised(ALPHA)                             # accepted; ALPHA = AwaitingInput
question_answered(ALPHA, Answered { option 0 })    # ALPHA = Pending; g0 still queued
```

After the answer the task reads `Pending` in every reader while `DESIGN.md` §14 says successful
work is `AwaitingMerge` until integration or repair. What the wrong label used to permit — the
pass-2 review of #153 at `784449e` drove it — was a second generation dispatched for the queued
task, the candidate merged under it, and `attempt_interrupted` returning the merged task to
`Pending`, from which the merged work ran again. #153 closes that consequence at
`check_dispatched`, which now refuses a dispatch of any task holding a queue position
(`a_task_whose_candidate_is_queued_is_not_dispatched_again`), so from this state the candidate
integrates (`merge_prepared` fast path; the task ends `Merged`) or is rejected (`AwaitingRepair`)
exactly as if it had never been parked. What remains is the label: `Pending` for a task that is
queued. The other live case, `Deferred`, ends the backoff early on the answer; `defers` is intact.

`AwaitingRepair` is **not** a live case: #153's exhaustive state door refuses a bare question
against it (its lineage's `task_merged` would move it under the question). An earlier revision of
this file said otherwise and was wrong.

## What #153 closes, and what it does not

#153 refuses a bare `question_raised` against a task that is not at rest — `Merged`, `Failed`,
`AwaitingInput`, `AwaitingRepair`, any open generation, or the candidate under integration — and
refuses a dispatch of a task whose candidate is queued. It does not change what an answer returns
a bare question's task to, which is `apply.rs` (queue row 29) and is being fixed in #152:
`apply_answer` restores a derived `OpenQuestion.parked_from`, and `design/12` gains the sentence
stating the return. When that fix is on master this file is deleted on the #153 branch, citing
#152's commit; if #152 does not land, this file stays and says so.

## What the change that takes this up should do

Give `OpenQuestion` the state its task was parked from, for a bare question, and have
`apply_answer` return the task there — which is #152's fix. The `check_end.rs` alternative,
refusing the bare event against every state but `Pending`, would refuse the shape
`engine/topology/select/tests.rs` uses to park a queued candidate's task, and `DESIGN.md` §12 has
questions raised eagerly while unrelated work proceeds, so it was not taken.
