---
id: PR5-RD-002
severity: P1
disposition: deferred
category: crash-consistency
pr: 5
reviewed_sha: 327cce3
location: src/workspace_manager.rs:9530
provenance: pre_existing
first_bad: 
guard: the slice that next **changes** the worktree removal or residue-recovery path in `src/workspace_manager.rs` — touching the file is not the trigger
---

## Failure sequence

A kill inside `git worktree add`'s registration window leaves
`.git/worktrees/<slot>/commondir` **zero-length**. Git treats a zero-length read as failed, and
`git worktree list --porcelain -z` then fails with `fatal: … : Success` — glibc's `strerror(0)`
for an errno that was never set — taking down the whole enumeration. `remove_worktree` errors
rather than converging.

Measured **1 in 18** clean-tree runs locally on Linux, and observed on CI 2026-08-27 at `327cce3`,
`test (macos-latest)`:

```
workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered FAILED
panicked at src/workspace_manager.rs:9530: forced removal converges:
Git { message: "git worktree list --porcelain -z failed …: fatal: failed to read
.git/worktrees/kalpha-g0/commondir: Undefined error: 0" }
```

**The recognition trap is in the signature, not the string.** macOS renders errno 0 as
`Undefined error: 0`, not glibc's `Success`; a reader matching on the word `Success` will miss the
macOS occurrence of this exact row. The signature is *errno 0 on a read that returned no bytes*.
First occurrence in 41 concluded runs of that branch, each running a macOS leg; a re-run at the
same SHA came back green, and CI re-runs erase the failure from `gh run list`, which is why the
rate was written down at observation time.

## What the change that takes this up should do

Solve containment-authorization restoration and removal-widening **together**, and get a
macOS reproduction path first. Round 7 attempted a repair — `enumerated_worktree_paths` falling
back to a directory scan when enumeration fails — and it was reverted whole, because the fallback
silently skips unreadable, zero-length and non-UTF-8 entries, so containment checks would run
against a list shorter than Git's real one and an execution root inside an omitted worktree would
pass. That turns a fail-closed defect into a fail-open one capable of recursive deletion inside the
user's own checkout: a 1-in-18 test flake is not worth a path that can delete outside the
authorized root.

Manual recovery, for anyone who hits it: `rm -rf <common-git-dir>/worktrees/<slot>` and re-run.
`git worktree prune` alone may not clear it, because a `locked` file from the same interrupted
registration is what prune skips. Every production call site propagates with `?`
(`src/workspace_manager.rs:1433`, `:1816`) — nothing panics, nothing is deleted, and the failure is
a refusal rather than damage.

Recorded in `reviews/FINDINGS.md` §13, which is the record of the reverted round-7 repair. The row carried no P-label; **P1** here is this migration's judgement, from the section's own reasoning that the defect it fixed is safer than the fix.
