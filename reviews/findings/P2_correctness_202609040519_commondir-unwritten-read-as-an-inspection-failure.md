---
id: PR128-COMMONDIR-UNWRITTEN-READ-AS-AN-INSPECTION-FAILURE
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: f161c9e8555c88474f09f89f095737346070d334
location: src/workspace_manager/residue.rs:464
provenance: fix_regression
first_bad: PR127-RECORD-FOR-COLLAPSES-A-GIT-FAILURE-INTO-ABSENCE
guard: the repair is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found…
---

## Failure sequence

`git worktree add` creates the administrative directory and writes `commondir` after, so a kill in between leaves a zero-length one -> git 2.43 then refuses to enumerate any worktree ("failed to read `…/commondir`", exit 128), which since PR #127's `record_for` repair is an error -> `add_state` propagated it and the classifier answered no class for durable state the site registers; the sampling harness counted it unclassified in 2 of 26 box runs, always `Worktree.Add: n=8 none=1 internal=6 after=0 unclassified=1`

## What the change that takes this up should do

the repair is **reverted** at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found the same defect classes in the machinery each round added: the parent helper is master's again and this row is deferred to the sweep of `src/workspace_manager.rs`, the queue's last row of this family, with the reproduction it carries. Making an inspection's absence trustworthy means reading a repository the way Git reads it, which is that file's work; a design sentence follows that code rather than preceding it

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
