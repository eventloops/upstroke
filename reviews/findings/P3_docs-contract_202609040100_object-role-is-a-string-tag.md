---
id: PR126-OBJECT-ROLE-IS-A-STRING-TAG
severity: P3
disposition: deferred
category: docs-contract
pr: 126
reviewed_sha: 809130d540a20ad01faa1c9e94d7acc2ab3f0359
location: src/workspace_manager/object.rs:44
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to the parent's sweep of src/workspace_manager.rs, the queue's last row of this family, where the variant's field lives; until then the…
---

## Failure sequence

`role: &'static str` on the refusal and on the parent's `MalformedObjectId` field is a string tag with two values where §5 asks for an enum -> a third call site can pass any text -> the refusal message names a side that does not exist; today nothing branches on the tag and the two literals are passed by the file's own two wrappers

## What the change that takes this up should do

deferred to the parent's sweep of `src/workspace_manager.rs`, the queue's last row of this family, where the variant's field lives; until then the two literals are passed only by `refuse_expected_old` and `refuse_new`, and `an_expected_old_is_a_well_formed_non_null_id_at_either_hash_length` was witnessed failing under the expected-old wrapper passing the role new

Recorded by the PR #126 `src/workspace_manager/object.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
