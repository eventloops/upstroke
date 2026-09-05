---
id: SWEEP-WORKTREE-012
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/workspace_manager/tests.rs:6777
provenance: fix_regression
first_bad: PR127-RECORD-FOR-COLLAPSES-A-GIT-FAILURE-INTO-ABSENCE
guard: deferred, not this file's: the residue classifier (residue.rs, row 6, PR #128 open) or the parent's record_for (row 11) decides that a worktree…
---

## Failure sequence

two runs of `sampled_git_child_kills_every_residue_classified_and_recovered` on the box out of twenty-six, both under a mutation of this module the classifier cannot reach on Git's own output, failed `every sample is accounted for by exactly one class` with 7 of 8, both with the `Worktree.Add` site reading `n=8 none=1 internal=6 after=0 unclassified=1` -> an unclassified sample is `classify_object_residue` returning `Err` (`tests.rs:7216`, `.ok()`), and since PR #127's `record_for` repair (`8cf2d90`) a non-zero `git worktree list` is an `Err` where it was `Ok(None)` -> measured on the box (Git 2.43.0) over hand-built registrations: an admin directory with only `gitdir`, or with `gitdir` and `commondir` and no `HEAD`, lists fine (`HEAD 0000…`, `detached`, `prunable gitdir file points to non-existent location`, inside the grammar), and a zero-length `commondir` alone exits 128 with `fatal: failed to read .git/worktrees/x/commondir: Success` -> a `git worktree add` killed in the window between creating `commondir` and writing it leaves the one registration the classifier now answers with `Err`, the sampler counts it unclassified, and the test's own reading of that count is "durable state no tabled action recovers"

## What the change that takes this up should do

deferred, not this file's: the residue classifier (residue.rs, row 6, PR #128 open) or the parent's `record_for` (row 11) decides that a `worktree list` failing to read a registration's `commondir` is Git's own interrupted-add residue and classifies it (`RegisteredUnpopulatedWorktree`, `Internal`) instead of erroring, and the sampler then accounts for it; until then the fingerprint is the `Worktree.Add` histogram with `unclassified=1` at the histogram-total assertion, 2 in 26 on the box, not yet seen in CI (three green runs of this pull request and #127's runs)

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
