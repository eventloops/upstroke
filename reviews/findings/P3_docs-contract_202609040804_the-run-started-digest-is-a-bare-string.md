---
id: SWEEP-CLASSIFY-011
severity: P3
disposition: deferred
category: docs-contract
pr: 137
reviewed_sha: 5f661fa7f8d5c45471cc33746a70df1cd192c61e
location: src/rundir/classify.rs:278
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to src/events/log.rs and src/rundir.rs, with the proposal named: one definition of the spelling, with first_line_digest delegating to it,…
---

## Failure sequence

`run_started_sha256` returns a `String` and `CommitRecord.run_started_sha256` holds one -> a digest and any other string are the same type, so a ref name, a truncated digest or an empty value is assignable and comparable where a digest is meant, and two functions in two modules each build the `sha256:` spelling with their own `format!` -> nothing refuses a malformed value at the boundary the way `workspace_manager::object` refuses an object id, and the two producers agree only by inspection

## What the change that takes this up should do

deferred to `src/events/log.rs` and `src/rundir.rs`, with the proposal named: one definition of the spelling, with `first_line_digest` delegating to it, and a validating predicate applied where the record is read, which is the shape `is_object_id` takes rather than a newtype through a serde field. `the_two_spellings_of_the_first_line_digest_agree` pins the agreement in the meantime

Recorded by the PR #137 pass over `src/rundir/classify.rs`; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
