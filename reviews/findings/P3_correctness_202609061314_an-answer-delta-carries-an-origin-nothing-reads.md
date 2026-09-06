---
id: SWEEP-FOLD-002
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold.rs:491
provenance: pre_existing
first_bad:
guard: the sweep of src/topology/fold/check_end.rs (queue row 32) or src/topology/fold/apply.rs (row 29)
---

## Failure sequence

`Derived::Answer(QuestionOrigin)` is the only `Derived` variant carrying a payload
that the application does not use. `check_question_answered` returns the open
question's `origin`, `check_started_run` puts it in the delta, and
`RunState::apply`'s single reader is

    Derived::Answer(QuestionOrigin::VerificationPark | QuestionOrigin::Admission) => {
        self.apply_answer(data);
    }

Both spellings do the same thing, and `apply_answer` derives the task's next state
from current fold facts through `refresh_task_state` rather than from the origin.
`grep -rn 'Derived::Answer' src/` returns exactly those two sites, so the payload is
carried from the check to the application and read by nothing.

This is not wrong behaviour today; it is a value that looks load-bearing and is not.
A reader — or a later change — can reasonably conclude that the origin decides where
an answered question returns its task, which is what this file's notes said until the
sweep that filed this finding corrected them, and what PR #152's repair had already
stopped being true.

## What the change that takes this up should do

Either make the payload load-bearing again, or take it off: `Derived::Answer` becomes
a unit variant and `check_question_answered` returns `Result<(), FoldError>` like its
sibling checkers. That is a signature change in `src/topology/fold/check_end.rs` with
its application site in `src/topology/fold/apply.rs`, neither of which is inside the
bound of the `src/topology/fold.rs` sweep, and both of which are in the queue.

Whichever way it goes, keep the reader spelling both variants rather than matching
`Derived::Answer(_)`: a third `QuestionOrigin` should force a decision at that site.
