---
id: PR47-PUBLIC-PROCESS-API-REMOVED
severity: P3
disposition: deferred
category: compatibility
pr: 47
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: the project owner — a later compatibility-owned slice
---

## Failure sequence

A public process API was removed rather than deprecated. Nothing in the tree depends on it,
so no gate can see the removal; a downstream crate that named it does not compile against the next
release.

## What the change that takes this up should do

Take it up in a slice that owns compatibility, and decide there whether the crate's public
surface carries a deprecation obligation at all. Recorded as an accepted residual and preserved as
residue rather than as a gate blocker — the point of the row is that the removal is known, not that
it is waiting on a fix.

Recorded in `reviews/FINDINGS.md` §25. Severity is this migration's judgement.
