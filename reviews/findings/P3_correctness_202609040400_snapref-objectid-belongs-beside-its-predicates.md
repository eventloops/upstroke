---
id: PR130-SNAPREF-OBJECTID-BELONGS-BESIDE-ITS-PREDICATES
severity: P3
disposition: deferred
category: correctness
pr: 130
reviewed_sha: 80843302a8367e607e54f181ef592c02ca5a089f
location: src/workspace_manager/object.rs:62
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to the parent's sweep of src/workspace_manager.rs (standards/SWEEP.md queue row 11), where the primitives that would take the type live:…
---

## Failure sequence

`ObjectId` lives in `snapshot_ref.rs` because that module is its only consumer, while `is_object_id` and the ref refusals take `&str` and the engine's `CommitSha` validates nothing -> one crate has two spellings of "an object id", a type and a predicate -> a ref primitive still accepts, then refuses at its own site, a value a snapshot input cannot be built from, and the two refusals (`MalformedObjectId`, `NotAnObjectId`) describe one fact

## What the change that takes this up should do

deferred to the parent's sweep of `src/workspace_manager.rs` (`standards/SWEEP.md` queue row 11), where the primitives that would take the type live: move `ObjectId` beside its predicates in `object.rs`, take it on both sides of `create_ref_zero_old`, `compare_and_swap_ref` and `delete_ref_expected_old`, and let the engine's `CommitSha` carry one; until then `ObjectId::new` is the one validated entry and `a_malformed_value_is_refused_as_not_an_object_id_with_the_value_as_offered` pins it

Recorded by the PR #130 `src/workspace_manager/snapshot_ref.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
