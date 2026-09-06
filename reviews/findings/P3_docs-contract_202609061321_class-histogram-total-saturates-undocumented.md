---
id: SWEEP-EFFECTS-REGISTRY-002
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/registry.rs:166 (as of the reviewed sha)
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects/registry.rs` (queue row 23)
---

## Failure sequence

`ClassHistogram::total` is documented as "how many samples the histogram
accounts for" and returns `u32` by three `saturating_add`s. At the boundary
that sentence is false: `{ none: u32::MAX, internal: 1, after: 0 }` accounts
for `u32::MAX + 1` samples and `total()` answers `u32::MAX`.

The saturation is deliberate — §5 requires the arithmetic to be chosen — but
the doc comment does not say so, and the consequence is already recorded
elsewhere in the same family as a defect that shipped:
`bijection::check_evidence` sums the three fields itself in `u64` and its
comment explains that it does so because "a saturating sum agrees with an
`n` of `u32::MAX` whatever the histogram holds", the reproduction the
`the_bijection_fails_on_every_missing_link` direction "a histogram whose
saturating total equals n" carries. So the one in-tree caller that has to be
right about the count cannot use this function, and the contract that would
tell the next caller why is not written down.

The remaining caller, `src/workspace_manager/tests.rs`, compares
`record.histogram.total()` with a sample count of its own.

## What the change that takes this up should do

Either say what the function does at the boundary — a `# Panics`-shaped
sentence for saturation, naming `check_evidence`'s `u64` sum as the reader
that needs the exact answer — or return the exact one (`u64`, or
`Option<u32>` from `checked_add`) and let `check_evidence` use it, which
would leave the family with one authority on the count rather than two.
`the_sampling_histogram_totals_its_three_counts_and_saturates` in
`src/topology/effects/tests.rs` pins today's behaviour and must move with the
decision.
