---
id: PR128-REVIEW-ADD-STATE-DOES-NOT-ESTABLISH-A-GIT-DIRECTORY
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: b5c12e042b01798d7f0dc8bec15543da9d807a33
location: src/workspace_manager/residue.rs:451
provenance: introduced_by_feature
first_bad: a17b8c5
guard: the repair is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, with the whole…
---

## Failure sequence

`git_dir_of` returned `Some(PathBuf::new())` for a pointer reading `gitdir:` with an empty path and `Some` for any target without reading it -> `add_state` answered `Populated` for a registered worktree with no git directory behind its pointer, the state the pull request claimed to fix, and every later name was joined onto `""` -> the add sites classified `After`; git 2.43 refuses the empty pointer as "invalid gitfile format"

## What the change that takes this up should do

the repair is **reverted** at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555 on the coordinator's final instruction of 2026-09-04 15:35Z, with the whole parent-helper programme, after pass 3 found the same defect classes in the machinery each round added: the parent helper is master's again and this row is deferred to the sweep of `src/workspace_manager.rs`, the queue's last row of this family, with the reproduction it carries. Making an inspection's absence trustworthy means reading a repository the way Git reads it, which is that file's work; a design sentence follows that code rather than preceding it

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
