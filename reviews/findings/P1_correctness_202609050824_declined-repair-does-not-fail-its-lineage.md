---
id: SWEEP-FOLD-APPLY-DECLINE-LINEAGE
severity: P1
disposition: deferred
category: correctness
pr: 152
reviewed_sha: b98c275a5d2084b1ee1eeee05f6e675683664908
location: src/topology/fold/apply.rs:598
provenance: pre_existing
first_bad:
guard: needs two homes — src/topology/fold/apply.rs (row 29, the failing and the surviving transaction) and src/topology/fold/check_end.rs (row 32, the admission and transaction ownership); take it once #152 and #153 have both landed
---

## Failure sequence

`design/26` states the rule — *"Declining fails the lineage."* — and `release_holdings_of`'s own doc
restates it: *"a declined lineage fails as a whole."* Neither holds today, and the fix cannot be
completed in `apply.rs` alone. #152 attempted the apply-half at `ed7f6cce`..`b98c275a` and it is
reverted here to master's behaviour, because the reviewer's pass-4 sequence shows the in-file fix
both incomplete and, in one branch, worse than the wedge.

**Base defect (master's behaviour, restored by the revert).** `apply_answer`'s `Declined` arm runs
only `self.set_state(answered.key, TaskState::Failed)` and `release_holdings_of(answered.key)`. When
the declined task is a repair, its lineage root is left `AwaitingRepair` with no queue entry, no open
question, no generation and no runnable repair, so `derived_outcome` has no ending for that shape
(`FoldError`) and refuses every `run_finished`: the run can never end.

**Why failing the whole lineage in `apply_answer` does not fix it.** `release_holdings_of` releases
the candidate and lineage leases but **never clears `self.transaction`**. A repair is declined
through a question, and a question can be raised on a task whose integration transaction is open (see
the admission below). So after the decline:

1. **The wedge is preserved, not cleared.** `common()` counts `usize::from(self.transaction.is_some())`,
   so a standing transaction keeps `common()` non-zero and `derived_outcome` cannot be `Ending` — the
   very wedge the lineage-failure was meant to remove.
2. **Worse — declined work can be published.** The surviving transaction is trusted by the
   integration checks: `check_merge_prepared` and `check_task_merged` validate against the open
   transaction, not against the tasks' states. `apply_task_merged` then does
   `let Some(transaction) = self.transaction.take()` and `set_state(*key, TaskState::Merged)` for the
   transaction's satisfied keys — turning the just-declined lineage back to `Merged`. A run that a
   human **declined** publishes its work. This half is new in pass 4 and is the part a successor most
   needs: it is worse than the wedge and less obvious, because the failing path looks complete when
   read on its own.

**Root cause — the admission, in another file.** `check_end.rs:9`, `check_question_raised`, admits a
question with only `self.entry(KIND, question.key)?` and `check_new_question(...)` — it checks task
existence, completeness, key and unique id, and **neither the task's state nor whether an integration
transaction is open for it**. That is what lets a question (and its decline) reach a task mid-merge.
`check_end.rs` is #153's file (row 32), and #153 is mid-repair on a reproduced P1 in it.

## What the change that takes this up should do

The fix needs both files at once, which is why it is not in #152:

- **`src/topology/fold/apply.rs` (row 29).** A decline of a lineage member fails the root and every
  task descending from it (`registry` lineage walk), and the release must clear any integration
  transaction the declined task owns, not only its leases — otherwise 1 and 2 above stand. Do it
  unconditionally on `decline_halts_run`; that flag decides only whether the run also halts, and
  reading it in the failing path hides the wedge behind the halt.
- **`src/topology/fold/check_end.rs` (row 32).** `check_question_raised` refuses a question raised on
  a task that is not in a state that can hold one — in particular one with a live transaction it does
  not own — so that a decline can never reach a task mid-integration in the first place. Decide there
  what transaction ownership means for a question, since the apply-half's transaction clearing depends
  on it.

Take it in a session that can hold both files, once #152 (this sweep of `apply.rs`) and #153 (the
`check_end.rs` stream) have both landed. Until then the run can wedge on a declined repair; it does
not, at master's behaviour, publish declined work, because master does not fail the lineage and so
never reaches the merge-the-declined-work branch — that branch is reachable only from the incomplete
in-file fix, which is why the revert is the safe state to hold.
