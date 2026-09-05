---
id: PR128-REVIEW-GIT-FAILURES-CARRY-NO-COMMAND-OR-STDOUT
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: b5c12e042b01798d7f0dc8bec15543da9d807a33
location: src/workspace_manager.rs:2891
provenance: introduced_by_feature
first_bad: 335bb27
guard: the repair is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found…
---

## Failure sequence

`read_only_git`'s spawn failure said only "failed to run git", and `read_only_git_failure` carried stderr alone -> a corrupt repository makes `fsck` exit 2 with `missing blob <id>` on stdout and nothing on stderr (measured on git 2.43), so `unreachable_objects` returned an error with a blank diagnosis, and a spawn failure named neither command nor directory -> the §7 operation-context claim the body made for every propagated `?` was false at the source

## What the change that takes this up should do

the repair is **reverted** at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found the same defect classes in the machinery each round added: the parent helper is master's again and this row is deferred to the sweep of `src/workspace_manager.rs`, the queue's last row of this family, with the reproduction it carries. Making an inspection's absence trustworthy means reading a repository the way Git reads it, which is that file's work; a design sentence follows that code rather than preceding it

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
