---
id: SWEEP-CHECK_INTEGRATION-002
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/tests.rs:4901
provenance: pre_existing
first_bad:
guard: queue row 39, the sweep of `src/topology/fold/tests.rs`
---

## Failure sequence

Three of the integration suites drive a grid of broken records through
`plan_transition` and assert only that *something* was refused:

- `the_publication_relations_hold_over_the_crossed_disposition_grid`, seven
  stale-clean cases and three already-present ones, each
  `stale.plan_transition(..).is_err()`;
- `a_rejection_creates_or_widens_exactly_one_lineage_and_registers_its_repair`,
  six cases, each `ready.plan_transition(..).is_err()`.

`FoldError` has more than twenty variants and the checkers in
`src/topology/fold/check_integration.rs` run their relations in a fixed order,
so an assertion of this shape is green whichever guard fired, including one
that fired for the wrong reason. Measured on this grid: the case labelled "no
proposal pin" sets `prepared_ref = None` on a stale-clean publication, and
`check_merge_prepared` calls `MergePrepared::self_consistency` before any of its
own relations, so that record is refused as `PreparedDefect::StaleWithoutPreparedRef`
and never reaches the pin comparison in
`src/topology/fold/check_integration.rs`. The case's label names a relation of
that file; the case exercises a different one, and `is_err()` cannot tell them
apart.

The neighbouring suites in the same file show the stronger form: the deferral
suite asserts `FoldError::InvalidDefers { .. }` and the sequence suite asserts
whole error values with `assert_eq!`.

## What the change that takes this up should do

Give each grid case the refusal it is about: at least the `FoldError` variant,
and for the cases that share `InconsistentRecord` the whole `detail`, since a
variant-only assertion passes on the wrong arm. The refusal texts are stable
strings in `src/topology/fold/check_integration.rs` and each names its own
relation. Then re-run each grid's own mutation — deleting the guard the label
names — and see it fail.
