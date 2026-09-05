---
id: SWEEP-WORKTREE-015
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha: 9dd3a791f19dcee490ae4da006c39ed84a16f304
location: src/workspace_manager/worktree.rs:357
provenance: pre_existing
first_bad: SWEEP-PARSERS-008
guard: deferred to the parent (queue row 11) or the ref layer, which have the repository and can ask its backend; at…
---

## Failure sequence

`has_checked_out` compares the `branch` bytes with the queried refname exactly, and this pull request certified that comparison as ref identity when it closed `SWEEP-PARSERS-008` -> with the files ref backend on a case-insensitive filesystem (Git documents Windows and macOS) `refs/heads/x` and `refs/heads/X` can be one loose ref file while comparing unequal, and Git prints whichever spelling the worktree's symbolic HEAD carries -> the reviewer's sequence: create the lowercase run ref at A, make a linked worktree's HEAD symbolic to the uppercase spelling, porcelain reports uppercase, `assert_publishable` sees no match, `update-ref <ref> B A` succeeds, and the worktree's HEAD follows to B while its index and files stay at A -> the ref the design says is never checked out is published into a checkout

## What the change that takes this up should do

deferred to the parent (queue row 11) or the ref layer, which have the repository and can ask its backend; at 0e510735c5324d8d8c5e9194cf80f2858e4e1878 the certification is withdrawn instead of guessed at: `has_checked_out` documents that `false` is not proof of a different ref and that no case folding is done here, and `assert_publishable`'s site says the same. `SWEEP-PARSERS-008` is closed only for the half it measured, the whitespace and byte-set spelling of a branch; ref-storage equivalence is this row and stays open. The proposal for row 11: ask the guard a fail-closed question (byte identity, or a spelling the store cannot tell from it) rather than an identity one. Over-refusal can in principle refuse a legitimate publication where two distinct refs differ only by case; that is acceptable, because a refusal fails loudly where the alternative corrupts a checkout silently, and because this engine's run refs are generated rather than user-chosen

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
