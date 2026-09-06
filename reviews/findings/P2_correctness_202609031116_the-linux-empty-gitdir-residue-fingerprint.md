---
id: PR107-LINUX-WORKSPACE-RESIDUE-EMPTY-GITDIR-FINGERPRINT
severity: P2
disposition: deferred
category: correctness
pr: 107
reviewed_sha:
location: 
provenance: pre_existing
first_bad:
guard: project owner / the slice that next opens the workspace residue sampler
---

## Failure sequence

`workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered` panicked at `src/workspace_manager/tests.rs:5691:10` with `forced removal converges: Git { message: "worktree registration …/.git/worktrees/kalpha-g1 has an empty gitdir" }`, `test result: FAILED. 1806 passed; 1 failed; 35 ignored`. Run `33787330192`, attempt 1, job `100755588011`, `test (ubuntu-latest)`, at `9963fb0` on PR #107

## What the change that takes this up should do

Owner, as the ledger records it: project owner / the slice that next opens the workspace residue sampler.

**Open as one unexplained observation, not classified as a flake or regression.** **A third platform**, and its own ID rather than folded into either Windows row: different platform, different subsystem, different assertion. Nondeterministic — the same commit produced two green runs of this leg in the same hour. Cause unknown: `remove_worktree` handles an empty `commondir` deliberately (`src/workspace_manager.rs:1249-1258`), so whether the sampler is racing that arm or reaching a different empty-gitdir path is the open question and is not answered here. **This is the cheapest of the class's four members to chase**, recorded so the choice is not re-derived: it is the only one on the Linux leg, which this project's build box reproduces directly — no guest and no hosted macOS runner. **Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.** Full evidence: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
