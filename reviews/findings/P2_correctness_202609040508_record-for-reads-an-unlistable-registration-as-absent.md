---
id: PR128-REVIEW-RECORD-FOR-READS-AN-UNLISTABLE-REGISTRATION-AS-ABSENT
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: b5c12e042b01798d7f0dc8bec15543da9d807a33
location: src/workspace_manager.rs:3108
provenance: introduced_by_feature
first_bad: 335bb27
guard: the repair is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, with the whole…
---

## Failure sequence

`record_for` trusted a listing that exited 0 -> with a registration's administrative directory at mode 000, `git worktree list --porcelain -z` exits 0 with nothing on stderr and omits it (measured on git 2.43) -> `add_state` answered `Unregistered` and the add sites classified `None` for a registration the process could not read; exit status alone did not make the listing complete

## What the change that takes this up should do

the repair is **reverted** at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, with the whole parent-helper programme, after pass 3 found the same defect classes in the machinery each round added: the parent helper is master's again and this row is deferred to the sweep of `src/workspace_manager.rs`, the queue's last row of this family, with the reproduction it carries. Making an inspection's absence trustworthy means reading a repository the way Git reads it, which is that file's work; a design sentence follows that code rather than preceding it

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
