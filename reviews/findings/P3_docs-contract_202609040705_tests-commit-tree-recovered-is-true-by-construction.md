---
id: SWEEP-TESTS-COMMIT-TREE-RECOVERED-IS-TRUE-BY-CONSTRUCTION
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/tests.rs:7153
provenance: pre_existing
first_bad: —
guard: deferred to this file: the honest shape is for SyntheticRecord's recovered field to carry "nothing to recover" as a third state rather than as…
---

## Failure sequence

`construct_and_recover` initialises `recovered` to `true` and only assigns it for a site with a slot -> the two commit-tree sites have none, by design, since their tabled action is to delete nothing -> `every_registered_residue_element_is_constructed_and_recovers` asserts `record.recovered` for those two sites against a value nothing computed, and the real claim for them is the fsck comparison beside it

## What the change that takes this up should do

deferred to this file: the honest shape is for `SyntheticRecord`'s `recovered` field to carry "nothing to recover" as a third state rather than as `true`, and `SyntheticRecord` is `src/topology/effects.rs`'s frozen packet type, whose sweep is queue row 28. The fsck assertion that does carry the claim runs for every site

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
