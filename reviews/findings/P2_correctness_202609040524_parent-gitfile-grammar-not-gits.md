---
id: PR128-PARENT-GITFILE-GRAMMAR-NOT-GITS
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: dfc238c63fd7db4c9f9d8ab5f41113ad8ad56617
location: src/workspace_manager.rs:2872
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to the sweep of src/workspace_manager.rs, queue row 11, with the reproduction: rewrite a valid checkout's .git to gitdir:/path/to/admin,…
---

## Failure sequence

`git_dir_of` accepts `gitdir:` without the space Git requires, trims leading and trailing blanks Git does not, and takes any directory as a git directory -> a checkout whose `.git` reads `gitdir:/path/to/admin`, or points at an ordinary empty directory, is rejected by Git and accepted here -> `add_state` answers `Populated` and the classifier `After` for a worktree Git will not open, which is the opposite of the interrupted-add residue the site registers

## What the change that takes this up should do

deferred to the sweep of `src/workspace_manager.rs`, queue row 11, with the reproduction: rewrite a valid checkout's `.git` to `gitdir:/path/to/admin`, and separately point a well-formed line at an empty directory. Reading the gitfile as Git reads it (`setup.c`, git 2.43) and validating the target as a git directory is that file's work; `add_state` reads whatever it answers

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
