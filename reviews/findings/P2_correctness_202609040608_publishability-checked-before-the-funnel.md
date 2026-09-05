---
id: SWEEP-WORKTREE-016
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha: 9dd3a791f19dcee490ae4da006c39ed84a16f304
location: src/workspace_manager.rs:2179
provenance: pre_existing
first_bad: SWEEP-WORKTREE-015
guard: deferred to the parent's sweep (queue row 11), whose funnel this is; the call site names it
---

## Failure sequence

`compare_and_swap_ref` calls `assert_publishable` before it enters the effect funnel, and nothing re-asks after the Before hook or before `git update-ref` -> the reviewer reproduced it on Git 2.43.0: the check observed a linked worktree detached at A, that worktree then checked out the run ref at A, and `update-ref <ref> B A` succeeded -> afterwards the ref and the worktree's HEAD were both B while its index and file still held A, with `git status --short` reporting `M  payload` -> publication into a checked-out ref, which the design forbids, through a check that ran too early rather than a check that was missing

## What the change that takes this up should do

deferred to the parent's sweep (queue row 11), whose funnel this is; the call site names it. The proposal: re-assert publishability inside the funnel closure, where `revalidate_acted_through` already runs, after the Before hook. That narrows the window rather than closing it -- `git update-ref` and a checkout are not one atomic act -- so row 11 owes a statement of what the funnel guarantees as well as the code, which is why this round does not change it

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
