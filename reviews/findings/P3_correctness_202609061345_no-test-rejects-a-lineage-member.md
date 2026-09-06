---
id: SWEEP-CHECK_INTEGRATION-004
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/tests.rs:5891
provenance: pre_existing
first_bad:
guard: queue row 39, the sweep of `src/topology/fold/tests.rs`
---

## Failure sequence

No test in the tree rejects a candidate that is itself a lineage member, so
`check_merge_rejected`'s widening path has never been exercised end to end.

`a_rejection_creates_or_widens_exactly_one_lineage_and_registers_its_repair`
rejects `MID`, an ordinary task, and its accepted case is
`RejectionLeaseEffect::CreatesLineage`. Its one widening case names
`WidensLineage { root: MID }` on that same non-member and is refused — labelled
"a widening of a lineage that does not exist" and asserted with `is_err()`, so
it exercises the mismatched-pairing refusal, not the widening. Nothing drives
the second rejection of a lineage: the repair `TaskKey(3)` the suite registers
is never dispatched, never prepares a candidate, and is never rejected in turn.

Measured, at the head of the row-33 sweep: substituting the `entry.lineage`
argument at `check_merge_rejected`'s call of `rejection_lineage_root` with a
constant `None` leaves all 136 `topology::fold` tests green. The same cell was
unexercised before that function was extracted — the substitution is what makes
it visible, not what causes it. Every other seam of the four extractions in
`src/topology/fold/check_integration.rs` is killed by that suite: the pin
argument by six tests, the rejected key by twenty, the settled root by
twenty-seven, the allowance arguments by seven.

Unexercised with it: `lineage_members` returning a nonzero index,
`apply_merge_rejected`'s `WidensLineage` arm, and the `check_spawn` relation
that a second repair's `index` is the count of the lineage's existing members.

## What the change that takes this up should do

Extend the rejection suite with a second round on the same lineage: dispatch
the repair the first rejection registered, prepare and queue its candidate,
start a verification, and reject it with `WidensLineage { root }` and a repair
whose `Lineage` records `index: 1` and the first repair as its parent. Assert
the accepted case and, one broken field at a time, the refusals — a widening
rooted elsewhere, a repair numbered `0` again, a `CreatesLineage` on a member —
each against its own `FoldError` variant and `detail` rather than `is_err()`
(see `SWEEP-CHECK_INTEGRATION-002`). Then re-run the substitution above and see
it fail.
