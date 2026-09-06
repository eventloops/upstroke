---
id: SWEEP-WORKTREE-007
severity: P3
disposition: deferred
category: correctness
pr: 
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/engine/topology/run.rs:1338
provenance: pre_existing
first_bad: —
guard: deferred to the engine, which is not on this queue and outside this sweep's bound: run.rs should carry the failure's text into the…
---

## Failure sequence

`VerifyFailure`'s `Display` is rendered by no production caller -> `RetryOutcome::Close { closed, .. }` drops the failure at this site and `Reuse::Recreated` carries one that only the dispatch tests read -> an operator sees a retained generation close as `WorktreeMissing`, or a worktree rebuilt, and is never told which of the seven observations it was: the message this file writes for a caller to act on reaches no one

## What the change that takes this up should do

deferred to the engine, which is not on this queue and outside this sweep's bound: `run.rs` should carry the failure's text into the `generation_closed` record's detail or the run's log at the site that drops it, and the dispatch path should do the same for `Recreated`. The `VerifyFailure` doc states the contract the engine owes: the variant and its `Display` are what an operator is told afterwards

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
