---
id: PR207-PROSE-REVIEW-BASE-UNRECORDED
severity: P3
disposition: deferred
category: correctness
pr: 207
reviewed_sha: 628784013580477bd51ca39ea9329161cb2d5c25
location: scripts/pr-ready-audit.sh:393
provenance: introduced_by_feature
first_bad: PR207-WORKFLOW-REVIEW-WITHOUT-BASE
guard: deferred: the frontier review comment format carries a head and no base; recording the base is a change to the owner's review driver on the box, not to this repository
---

## Failure sequence

the owner's frontier review comment carries `head=<sha>` and no base -> the audit can bind that review to the base only by the pull request's timeline (no base change after the comment) -> a pull request whose base was changed before the review, or one opened against the integration branch, has a review the audit cannot tie to a base commit and sends to MANUAL

## What the change that takes this up should do

teach `review-post.sh` on the box to write `base=<sha>` into the marker comment beside `head=`, and teach the prose parser here to read it and apply the same base checks the workflow form gets; the workflow form already records `base_sha` and is checked

Recorded by PR #207 from review pass 12.
