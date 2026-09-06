---
id: PR128-PARENT-UNBOUNDED-READS-OF-REPOSITORY-CONTROLLED-FILES
severity: P2
disposition: deferred
category: liveness
pr: 128
reviewed_sha: dfc238c63fd7db4c9f9d8ab5f41113ad8ad56617
location: src/workspace_manager.rs:3319
provenance: introduced_by_feature
first_bad: 87c29fc
guard: fixed at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 by reverting the scan, so no production path in this pull request reads a repository-controlled…
---

## Failure sequence

the registration scan reads `gitdir` and `commondir` with unrestricted `fs::read` -> a FIFO at `.git/worktrees/poison/gitdir` blocks the open for ever with no writer, and a symlink to `/dev/zero` reads without bound -> classifying an add site hangs or exhausts memory; bounding the number of registrations bounds neither bytes nor time, so the Risk claim that no inspection can block on a FIFO was false

## What the change that takes this up should do

fixed at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 by reverting the scan, so no production path in this pull request reads a repository-controlled file; deferred as a requirement to the sweep of `src/workspace_manager.rs`, queue row 11: every read of a file a repository controls needs a file-type check before the open and a bound on bytes and on time, and that requirement covers `revalidate_removal`'s existing `gitdir` reads as much as anything new

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
