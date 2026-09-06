---
id: PR128-REVIEW2-PATHS-READ-AS-UTF8
severity: P2
disposition: deferred
category: portability
pr: 128
reviewed_sha: f161c9e8555c88474f09f89f095737346070d334
location: src/workspace_manager.rs:2868
provenance: introduced_by_feature
first_bad: a17b8c5
guard: the repair is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found…
---

## Failure sequence

`git_dir_of` read the `.git` pointer with `read_to_string`, and `object_directory` and `common_git_dir` converted git's printed path with `String::from_utf8` -> in a valid repository whose path contains byte `0xff`, git writes and accepts that byte in a linked worktree's pointer and prints it back -> the add classifier answered `Io` where it had answered `After`, and every residue element read through the objects directory failed, against §8 in code this pull request rewrote

## What the change that takes this up should do

the repair is **reverted** at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, after pass 3 found the same defect classes in the machinery each round added: the parent helper is master's again and this row is deferred to the sweep of `src/workspace_manager.rs`, the queue's last row of this family, with the reproduction it carries. Making an inspection's absence trustworthy means reading a repository the way Git reads it, which is that file's work; a design sentence follows that code rather than preceding it

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
