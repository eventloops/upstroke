---
id: SWEEP-TESTS-RECOVER-SAMPLE-COMPARES-TWO-DISCARDED-RESULTS
severity: P3
disposition: deferred
category: correctness
pr: 
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/tests.rs:8285
provenance: pre_existing
first_bad: —
guard: deferred to this file: the honest form is to compare only two Ok answers and say what an unresolvable record means to a recovery check, which is a…
---

## Failure sequence

`recover_sample` matches a worktree record by `canonical_prefix(record.path()).ok() == canonical_prefix(&path).ok()` -> two paths that both fail to resolve compare equal as `None` -> a record the process could not resolve is read as this slot's, so a sample is reported unrecovered for a reason the assertion does not name; the fold is loud rather than silent here, which is why it is P3

## What the change that takes this up should do

deferred to this file: the honest form is to compare only two `Ok` answers and say what an unresolvable record means to a recovery check, which is a decision about `canonical_prefix`'s contract that `src/workspace_manager/containment.rs` owns and queue row 11 composes

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
