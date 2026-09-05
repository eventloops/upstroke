---
id: SWEEP-FOLD-APPLY-ORIGIN-SUPERSEDED
severity: P3
disposition: deferred
category: correctness
pr: 152
reviewed_sha: 943ae61dc61c579a3b03744c8994a1ce81a9acf8
location: src/topology/fold/start.rs:157
provenance: introduced_by_feature
first_bad:
guard: src/topology/fold/check_end.rs (row 32) and src/topology/fold/start.rs (row 38), whose sweeps remove the superseded type
---

## Failure sequence

Not a failure — a superseded type left in place to respect a bound. #152 gave `OpenQuestion` a
derived `parked_from: TaskState` and made `apply_answer` restore it, so an answered question returns
its task to the state it was parked from. That is the whole of what `QuestionOrigin` decided: its
only role was the answer-return (`VerificationPark -> AwaitingMerge`, `Admission -> Pending`), and
`parked_from` subsumes it exactly -> so `QuestionOrigin`, `OpenQuestion::origin`, and the
`Derived::Answer(QuestionOrigin)` payload are now dead for the return -> `apply_answer` keeps the
wire-carried `origin` only as a `debug_assert` cross-check (a verification park is parked from
awaiting merge) rather than discarding it, because removing the type reaches two files this sweep's
own-file bound does not cover.

## What the change that takes this up should do

Remove `QuestionOrigin` and its threading, in the sweeps of the two rows it lives in:

- `src/topology/fold/check_end.rs` (row 32, **open in #153** `fix/declined-halt-wedge` at `7a6b23b`
  — coordinate with that stream): `check_question_answered` returns `Result<QuestionOrigin, ..>`
  (line 18) and ends `Ok(open.origin)` (line 130). It becomes `Result<(), ..>`.
- `src/topology/fold/start.rs` (row 38): the dispatch builds `Derived::Answer(origin)` (line 157).
  `Derived::Answer` loses its `QuestionOrigin` field.
- `src/topology/fold.rs`: drop `QuestionOrigin` (and `OpenQuestion::origin`), and `apply_answer`'s
  `origin` parameter and its cross-check go with it — `parked_from` is the whole rule.

The rule survives the removal because `design/12` states it as *an answered question returns the
task to the state it was parked from*, not as an origin table.
