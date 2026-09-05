---
id: SWEEP-FOLD-APPLY-DECLINE-LINEAGE
severity: P1
disposition: deferred
category: correctness
pr: 152
reviewed_sha: a00bebab3e9a597e361b18585eb1d9908a6fbc59
location: src/topology/fold/apply.rs:611
provenance: pre_existing
first_bad:
guard: beyond this pull request's reach — the fix requires editing src/topology/fold/check_end.rs (row 32, check_question_raised), owned by live pull request #153, so no head of #152 can carry it; this is beyond-reach, not a scope choice. Escalate if reclassified — if a later pass relabels this or a reader disputes the deferral, it escalates to the owner rather than remaining silently deferred. The coordinator briefs the successor once #152 and #153 land. Both homes — src/topology/fold/apply.rs (row 29, the failing and the surviving transaction) and src/topology/fold/check_end.rs (row 32, the admission).
---

## Failure sequence

`design/26` states the rule — *"Declining fails the lineage."* — and `release_holdings_of`'s own doc
restates it: *"a declined lineage fails as a whole."* Neither holds, and **this is live on master, not
something this pull request was about to introduce.** #152 attempted the apply-half fix (fail the
whole lineage on a decline) at `ed7f6cce`..`b98c275a` and reverted it, because the fix cannot complete
in `apply.rs` alone and the reviewer showed the publish path is reachable at master regardless.

**Base defect — the run wedges.** `apply_answer`'s `Declined` arm (master's, restored by the revert)
runs only `self.set_state(answered.key, TaskState::Failed)` and `release_holdings_of(answered.key)`.
When the declined task is a repair, its lineage root is left `AwaitingRepair` with no queue entry, no
open question, no generation and no runnable repair, so `derived_outcome` has no ending for that
shape (`FoldError`) and refuses every `run_finished`: the run can never end.

**Worse, and live on master — declined work is published.** `release_holdings_of` releases the
candidate and lineage leases but **never clears `self.transaction`**, which `common()` counts. A
repair is declined through a question, and a question can be raised on a task whose integration
transaction is open (see the admission below). So after a decline the transaction stands, and
**`check_task_merged` validates the transaction, not task state or lease existence** — so a
`task_merged` is still accepted, and `apply_task_merged` takes the transaction and marks its satisfied
keys (root and repair) `Merged`. A run a human **declined** publishes its work. This holds at master's
behaviour: the earlier draft of this finding claimed the revert made publication unreachable — that
was wrong, corrected here, and the finding is stronger for it, because the defect is pre-existing and
live rather than something the reverted fix would have introduced.

**Root cause — the admission, in another file.** `check_end.rs:9`, `check_question_raised`, admits a
question with only `self.entry(KIND, question.key)?` and `check_new_question(...)` — it checks task
existence, completeness, key and unique id, and **neither the task's state nor whether an integration
transaction is open for it**. That is what lets a question (and its decline) reach a task mid-merge.
`check_end.rs` is #153's file (row 32), and #153 is mid-repair on a reproduced P1 in it.

## What the change that takes this up should do

The fix needs both files at once, which is why it is **beyond this pull request's reach**, not merely
out of scope: no head of #152 can edit `check_end.rs`, so no head of #152 can carry it.

- **`src/topology/fold/apply.rs` (row 29).** A decline of a lineage member fails the root and every
  task descending from it, and the release must clear any integration transaction the declined task
  owns, not only its leases — otherwise the wedge and the publish both stand. Do it unconditionally on
  `decline_halts_run`; that flag decides only whether the run also halts, and reading it in the failing
  path hides the wedge behind the halt.
- **`src/topology/fold/check_end.rs` (row 32).** `check_question_raised` refuses a question raised on
  a task with a live transaction it does not own, so a decline can never reach a task mid-integration.
  Decide there what transaction ownership means for a question, since the apply-half's transaction
  clearing depends on it.

**Beyond reach, escalation, handoff.** The fix requires `check_end.rs`, owned by live #153, so it
cannot land on any head of #152. If a later pass relabels this finding or a reader disputes the
deferral, it escalates to the owner rather than remaining silently deferred. The coordinator briefs
the successor once #152 and #153 have both landed, so the handoff does not depend on memory of this
exchange.
