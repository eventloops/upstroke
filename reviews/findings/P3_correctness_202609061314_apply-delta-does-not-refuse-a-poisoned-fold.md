---
id: SWEEP-FOLD-003
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold.rs:536
provenance: pre_existing
first_bad:
guard: the change that makes TopologyFold::apply_delta fallible
---

## Failure sequence

`TopologyFold::plan_transition` opens with `if self.poisoned { return
Err(FoldError::Poisoned) }` — refusals[24], "a process whose fold is poisoned by a
returned append error attempts no further transition". `TopologyFold::apply_delta`,
the other half of the same public protocol, does not read `poisoned` at all. A delta
planned before the poisoning and applied after it mutates the run state of a fold whose
own contract says it "derives nothing further from memory".

Measured, not assumed: the poisoning protocol has one production site,
`src/engine/topology/create.rs`, which calls `fold.poison()` on the append arm whose
outcome is unknown and does not call `apply_delta` on that arm; the same shape is in
`src/engine/topology/emit.rs`. `grep -rn 'apply_delta' src/` finds no site that
applies after poisoning. So this is an unchecked precondition, documented as one in
`docs/internals/topology/fold.md` ("These are caller preconditions. This method does
not validate freshness, reject duplicate application or verify that an append
occurred"), and not a live defect.

## What the change that takes this up should do

Not the one-line guard. Returning early when `poisoned` trades one silent outcome for
another: the caller still cannot tell whether its delta was applied, which is the same
hole `Poisoned` exists to close on the planning side.

The shape that closes it is a fallible `apply_delta(&mut self, delta) -> Result<(),
FoldError>` refusing `FoldError::Poisoned`, which is a public API change with call
sites in `src/engine/topology/{create,emit,scaffold}.rs` and `src/topology/census.rs`
as well as `TopologyFold::replay`. Weigh it against the sibling preconditions the same
method leaves unchecked — freshness and single use — since a signature that refuses one
of the three and stays silent on the other two is a worse contract than one that refuses
none.
