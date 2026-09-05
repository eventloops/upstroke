---
id: PR120-TABLE-OMITS-GIT-DISCOVERY-PATHS
severity: P1
disposition: deferred
category: security-trust
pr: 120
reviewed_sha: 62816dd4f3263e8dd252b009d3b0a6c999e5e9cc
location: src/workspace_manager.rs:487
provenance: pre_existing
first_bad: 7a83e69 (the parent's funnel ran Git through those paths before this pull request); ceec50f's table did not name them
guard: deferred to the parent's sweep of src/workspace_manager.rs (standards/SWEEP.md queue row 11): the table is nine roles and says so in…
---

## Failure sequence

the ref primitives' set is `HooksPath` alone and `base` being a directory does not inspect `base/.git` -> repositories A and B hold the same commit, `create_ref_zero_old` prechecks against A, its `Before` hook renames `A/.git` and plants `A/.git -> B/.git` -> the walk passes, A still a real directory and `hooks-none` unchanged -> `git update-ref` follows the link, creates the ref in B and returns success with A unchanged; the checkout's `.git` pointer, `admin/commondir` and the two commit-tree funnels are omitted the same way, and a test generated from the table cannot see any omission

## What the change that takes this up should do

deferred to the parent's sweep of `src/workspace_manager.rs` (`standards/SWEEP.md` queue row 11): the table is nine roles and says so in dcb0bddee749d12f1fa7d029cff93513ffbb0f78, its docs naming what it does not cover — the `.git` file or link of the checkout and of the base, and `commondir`, `objects`, `refs`, `packed-refs`, `index` and `config` behind them, and the two commit-tree funnels without a variant — and the durable fix is directory-handle-relative operations or a stated trust boundary for what may write inside the execution root, a design question for the owner (`DESIGN.md` §4, `CODING_STANDARDS.md` §14); `every_path_a_primitive_acts_through_refuses_a_link_planted_at_the_before_hook` pins the table's own size, a regression pin and not a proof

Recorded by the PR #120 `src/workspace_manager/containment.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
