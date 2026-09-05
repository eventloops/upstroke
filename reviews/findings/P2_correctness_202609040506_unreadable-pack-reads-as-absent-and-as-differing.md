---
id: PR128-REVIEW-UNREADABLE-PACK-READS-AS-ABSENT-AND-AS-DIFFERING
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: b5c12e042b01798d7f0dc8bec15543da9d807a33
location: src/workspace_manager.rs:2935
provenance: introduced_by_feature
first_bad: 335bb27
guard: the repair is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, with the whole…
---

## Failure sequence

the lookup helper took exit 1 with empty stderr as absence and `index_differs_from_head` took exit 1 as a difference -> with a pack file at mode 000 and its `.idx` readable, git 2.43 answers `rev-parse --verify --quiet <id>^{}` with exit 1 in silence and `diff --cached --quiet` with exit 1, and `cat-file --batch-check` prints `missing` (measured) -> `WorkspaceManager::object_exists` answered `false` and `refuse_absent_source` refused a missing candidate for a store it could not read, and `RepairMaterialize` classified a clean but unreadable repository `After`; the pull request's claim that an unreadable store propagates an error held only for a store Git itself refuses to open

## What the change that takes this up should do

the repair is **reverted** at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, with the whole parent-helper programme, after pass 3 found the same defect classes in the machinery each round added: the parent helper is master's again and this row is deferred to the sweep of `src/workspace_manager.rs`, the queue's last row of this family, with the reproduction it carries. Making an inspection's absence trustworthy means reading a repository the way Git reads it, which is that file's work; a design sentence follows that code rather than preceding it

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
