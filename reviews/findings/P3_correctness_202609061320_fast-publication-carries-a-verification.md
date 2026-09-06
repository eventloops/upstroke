---
id: SWEEP-CHECK_INTEGRATION-001
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/events.rs:1094
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/events.rs`, or any change that adds a `PreparedDefect`
---

## Failure sequence

`MergePrepared::self_consistency`'s `Fast` arm refuses a stray proposal pin
(`PreparedDefect::FastWithPreparedRef`) and a proposal that is not the candidate
commit, but says nothing about the `verification` field. A fast publication is
the exact-base case: no integration verification runs, and both producers emit
`verification: None` for it — `src/topology/census.rs:778` selects `None` for a
`VerificationSource::CandidatePrepared` source, and the `fast_publication`
fixture in `src/topology/fold/tests.rs:763` records `None`.

A `merge_prepared` with `disposition: fast` and
`verification: Some(VerificationRecord { verdict: passed, .. })` is therefore
accepted by `MergePrepared::self_consistency` and by
`RunState::check_merge_prepared`, and is folded and replayed unchanged. The
field is inert in the fold, so no state diverges; what survives is a persisted
record claiming an integration verification passed on a publication where none
ran. Ground truth is the diff, but the event log is what ledger, status and
export read, and this one lies about its own provenance.

The symmetric refusal already exists on the other side: a verified publication
without a terminal record is `PreparedDefect::VerifiedWithoutRecord`.

## What the change that takes this up should do

Add a `PreparedDefect` for a fast publication carrying a verification record and
refuse it in `self_consistency`'s `Fast` arm, beside `FastWithPreparedRef`,
which is the same shape one field over. The refusal belongs there rather than in
`src/topology/fold/check_integration.rs`, because it is an intra-event relation:
the file's own notes record that A1's intra-event relations are settled before
the record is compared with anything else. It needs one test in
`src/topology/events.rs`'s own suite and one hostile-replay candidate in
`src/topology/census.rs`.
