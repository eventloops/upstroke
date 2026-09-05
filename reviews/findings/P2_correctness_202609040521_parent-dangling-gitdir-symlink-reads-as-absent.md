---
id: PR128-PARENT-DANGLING-GITDIR-SYMLINK-READS-AS-ABSENT
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: dfc238c63fd7db4c9f9d8ab5f41113ad8ad56617
location: src/workspace_manager.rs:3292
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to the sweep of src/workspace_manager.rs, queue row 11, with the reproduction: replace a linked worktree registration's gitdir with a…
---

## Failure sequence

a registration's `gitdir` replaced by a dangling symlink is `NotFound` when followed, though the name exists -> `git worktree list` succeeds and omits that entry, and any scan that treats `NotFound` as "no registration here" agrees with the omission -> `record_for` answers `None`, `add_state` `Unregistered` and the classifier `ObjectResidue::None` for a registration that is there; the same holds for a dangling `commondir`, which reads as an unfinished add rather than an inspection failure. Git's own omission follows its linked-worktree reader returning no worktree when `gitdir` cannot be read (git 2.43 `worktree.c`)

## What the change that takes this up should do

deferred to the sweep of `src/workspace_manager.rs`, queue row 11, with the reproduction: replace a linked worktree registration's `gitdir` with a dangling symlink, list, classify. A reader that distinguishes "the name is not there" from "the name is there and unreadable" needs `symlink_metadata` before the read and a decision for each kind, which is the repository-reading helpers' work, not a classifier's

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
