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
guard: PR #152 owns answer return and durable backoff repair; keep this finding until integrated checked-log and replay tests establish the correct queued and deferred return states
---

## Failure sequence

A bare `question_raised` is accepted against a `Pending`, `Deferred` or queued `AwaitingMerge`
task outside a repair lineage, with no open generation and no transaction open on it. Its answer
returns the task to `Pending` whatever state it was parked from. `apply_answer` derives the return state from the
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

#153 refuses a bare `question_raised` against `Merged`, `Failed`, `AwaitingInput`,
`AwaitingRepair`, any repair-lineage member, any task with an open generation, or the candidate
under integration. It also refuses a dispatch of a task whose candidate is queued.
It does not change what an answer returns
a bare question's task to. PR #152 owns the application repair, including answer return and
durable backoff. A previous revision prescribed `OpenQuestion.parked_from` and spoke of a future
fix as settled. That mechanism is not evidence that this candidate is repaired. Keep this file
until the integrated candidate passes regressions for queued and deferred returns, including a
backoff elapsed while its task is parked. Cite the actual integrated commit when closing it.

## What the change that takes this up should do

Derive the resumed state from the work still pending and the remaining questions. Keep queued
work `AwaitingMerge`, preserve a pending backoff, and record a wake that occurs while questions
hide `Deferred`. Distinct questions must keep the affected work parked until their answers permit
it to resume. Exercise both answer orders and compare the live result with replay.

The reproduced contract deviation remains a dependency under the owner's witness rule despite
its historical P3 label. The `deferred` field records that #152 is repairing it; it does not waive
the repair for merge.
