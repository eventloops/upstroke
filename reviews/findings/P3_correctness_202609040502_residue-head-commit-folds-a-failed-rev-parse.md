---
id: PR128-RESIDUE-HEAD-COMMIT-FOLDS-A-FAILED-REV-PARSE
severity: P3
disposition: deferred
category: correctness
pr: 128
reviewed_sha: 80843302a8367e607e54f181ef592c02ca5a089f
location: src/workspace_manager.rs:3008
provenance: pre_existing
first_bad: 7a83e69
guard: the repair is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found…
---

## Failure sequence

`head_commit` answers `None` for any non-zero `rev-parse --verify --quiet HEAD` -> `ProposalCherryPick`'s after phase reads a Git failure as no HEAD -> the site classifies `None` or `Internal` for a worktree whose state is unknown; no production caller reaches the site today

## What the change that takes this up should do

the repair is **reverted** at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found the same defect classes in the machinery each round added: the parent helper is master's again and this row is deferred to the sweep of `src/workspace_manager.rs`, the queue's last row of this family, with the reproduction it carries. Making an inspection's absence trustworthy means reading a repository the way Git reads it, which is that file's work; a design sentence follows that code rather than preceding it

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
