---
id: PR128-RESIDUE-QUIESCENCE-READS-A-COMMON-DIR-FAILURE-AS-MISSING
severity: P3
disposition: deferred
category: correctness
pr: 128
reviewed_sha: 80843302a8367e607e54f181ef592c02ca5a089f
location: src/workspace_manager.rs:1613
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to the parent's sweep of src/workspace_manager.rs, the queue's last row of this family;…
---

## Failure sequence

`quiescence` matches `common_git_dir`'s error as `VerifyFailure::Missing` -> a linked worktree whose git dir the process cannot search is reported missing, and `administrative_residue_at`'s error is never reached through the public method -> the child's new error is observable only by driving the helper directly, which its witness does; a caller acting on `Missing` treats a worktree it could not read as one that is not there

## What the change that takes this up should do

deferred to the parent's sweep of `src/workspace_manager.rs`, the queue's last row of this family; `a_git_dir_that_cannot_be_searched_is_an_error_and_not_an_absent_residue` says in its doc why it drives `administrative_residue_at` directly

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
