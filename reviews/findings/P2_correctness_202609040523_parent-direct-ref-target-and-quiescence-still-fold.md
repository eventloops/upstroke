---
id: PR128-PARENT-DIRECT-REF-TARGET-AND-QUIESCENCE-STILL-FOLD
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: dfc238c63fd7db4c9f9d8ab5f41113ad8ad56617
location: src/workspace_manager.rs:2019
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to the sweep of src/workspace_manager.rs, queue row 11: the two helpers and the design sentence belong to the same change, and the…
---

## Failure sequence

`direct_ref_target` maps every unsuccessful `git show-ref --verify` to `None`, including a repository failure, and candidate recovery decides on that answer; `quiescence` maps a `common_git_dir` failure to `VerifyFailure::Missing` -> two inspection paths outside this file's reach fold failure into an answer -> a design sentence that says every repository read propagates its failure is false while they do, which is why this pull request's design paragraph is reverted rather than narrowed

## What the change that takes this up should do

deferred to the sweep of `src/workspace_manager.rs`, queue row 11: the two helpers and the design sentence belong to the same change, and the sentence follows the code. Until then no living authority claims more than the code does

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
