---
id: PR172-SAMPLER-REFUSED-A-TORN-WORKTREE-LIST-RECORD
severity: P3
disposition: deferred
category: correctness
pr: 172
reviewed_sha: a328b6fe92a715c3b9339db58a014cd175e83761
location: src/workspace_manager/tests.rs:8479
provenance: undetermined
first_bad: —
guard: deferred: one red on tactusbox in a module PR #172 does not touch, passing alone at the same head and on CI's three platforms; a rate is owed before anything is called a flake (§12), and the owner is the change that next opens the workspace sampler
---

## Failure sequence

`sampled_git_child_kills_every_residue_classified_and_recovered` on tactusbox at `a328b6f`, in the
full suite on 32 threads -> at the `Worktree.Add` sample point the classifier refused one of eight
samples with "git error: worktree list record 1 names a HEAD but neither a branch nor a detached
checkout" -> the test's own guard fired: an inspection that failed is not a residue in no class.
Passes alone at the same head. PR #172 changes nothing under `src/workspace_manager/`; its only
source change is `src/agent/proc.rs`'s helper handshake, which a `git worktree list` parse does not
reach.

## What the change that takes this up should do

Count before concluding. If it recurs, keep the `git worktree list --porcelain` output the classifier
refused and decide whether a record with `HEAD` and neither `branch` nor `detached` is a torn read of a
worktree mid-`add` (which the sampler kills on purpose) that the classifier should read as
`Interrupted` rather than refuse.
