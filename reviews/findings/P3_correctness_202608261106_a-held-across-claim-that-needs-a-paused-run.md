---
id: PR7-PIPELINE-014
severity: P3
disposition: deferred
category: correctness
pr: 7
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: the slice that can pause a run
---

## Failure sequence

A catalogue entry whose claim is that something is *held across* a span. No test in the tree
can pause a run at the point the claim is about, so the mutation survives the whole suite and the
claim is unwitnessed. It shares its shape with `PR5-R2-WORKTREE-LOCK-RETENTION`: existing tests
exercise acquisition, not retention.

## What the change that takes this up should do

Give the suite a coordinator seam that can pause a run mid-span, then witness the retention
rather than the acquisition. The two rows close together or not at all — inventing a one-off pause
for this entry is the orchestration a repair round should not be inventing.

Recorded in `reviews/FINDINGS.md` §20 as unchanged in disposition and evidence. Severity is this migration's judgement.
