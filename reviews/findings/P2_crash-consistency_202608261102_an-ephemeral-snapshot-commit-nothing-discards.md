---
id: PR7-R3-ATTEMPT-003-RESIDUE-DISCARD-UNREACHED
severity: P2
disposition: deferred
category: crash-consistency
pr: 7
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: the project owner — this needs a packet decision, not an implementer's repair
---

## Failure sequence

The snapshot worktree's ephemeral commit is reachable after a coordinator death mid-attempt,
and nothing discards it. The residue sits inside the run's own private root, so it is contained,
but it is not reclaimed.

## What the change that takes this up should do

Rule on the classification first. §38 reclassifies this from deferrable to **blocked**: the
packet and the tests classify the commit as Git-owned R27 and require recovery to leave it in
place, so deleting it would contradict an explicit contract. An implementer cannot repair around
that — what is needed is an owner packet decision that reclassifies the commit, and only then a
reclaim path.

Recorded in `reviews/FINDINGS.md` §20 and reclassified by §38. Severity is this migration's judgement.
