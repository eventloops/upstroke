---
id: SWEEP-TESTS-SUBSTITUTION-CASE-READS-METADATA-WITH-OK
severity: P3
disposition: deferred
category: correctness
pr: 
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/tests.rs:1736
provenance: pre_existing
first_bad: —
guard: deferred to this file: distinguishing absence from a failed inspection here means deciding what an unreadable acted-through path means to the…
---

## Failure sequence

`every_path_a_primitive_acts_through_refuses_a_link_planted_at_the_before_hook` decides whether a target is a file with `fs::symlink_metadata(&target).ok()` -> a permission failure reads as absence, so the case plants a directory link where it meant a file link -> the case still runs and still asserts a refusal, but against a substitution of the wrong kind, and the skipped-on-Windows count it feeds is then wrong too

## What the change that takes this up should do

deferred to this file: distinguishing absence from a failed inspection here means deciding what an unreadable acted-through path means to the generator, and the same question is open one level down in `src/workspace_manager/residue.rs` §51's deferred rows. The count assertion makes a wrong classification visible as a number that stops matching

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
