---
id: PR7-R3-ATTEMPT-002-REVIEWERS-TAKE-NO-SLOT
severity: P2
disposition: deferred
category: correctness
pr: 7
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: PR11
---

## Failure sequence

A review pass reaches the Runner through the `ReviewPasses` seam with a raw `&dyn Runner`, so
it takes no capacity slot. Nothing in the accounting knows the reviewer is running, so a reviewer
and a worker can be in flight against a budget that admits one.

## What the change that takes this up should do

Make the reviewer path take the same `SlotAssertion` the rest of the system uses. That is a
seam change, not a line-level fix. It is dormant at this width and not fixed: R3 is "assertion only"
at `max_parallel = 1` and this build ships that width, so over-subscription cannot currently occur —
it becomes live with PR11's parallelism work, and PR11 is where the repair belongs.

Recorded in `reviews/FINDINGS.md` §20. Severity is this migration's judgement: a real accounting bypass that is currently masked by the shipped width.
