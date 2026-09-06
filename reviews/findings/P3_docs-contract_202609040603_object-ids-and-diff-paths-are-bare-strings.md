---
id: SWEEP-WORKTREE-009
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/workspace_manager/worktree.rs:130
provenance: pre_existing
first_bad: —
guard: deferred: a validated object-id type belongs to object.rs, which owns is_object_id (the engine's CommitSha is the unvalidated spelling of the same…
---

## Failure sequence

`Quiescence::AtBase`, `HoldsTree`, `HeadMismatch` and `TreeMismatch` carry object ids as bare `String`, and `TreeMismatch::difference` carries the differing paths as one lossy `String` the parent's `index_differs_from` builds -> §5 asks for a dedicated identifier type where a mix-up is possible (a base and a tree are both hex) and §8 for paths as `Path` values -> a public API change reaching the engine's `dispatch.rs`, `settle.rs` and their tests, outside this sweep's bound

## What the change that takes this up should do

deferred: a validated object-id type belongs to `object.rs`, which owns `is_object_id` (the engine's `CommitSha` is the unvalidated spelling of the same thing), and `difference` as a typed list of paths or a reason belongs to the parent's `index_differs_from` (row 11); the field's doc comment on `difference` states that the string is a diagnostic

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
