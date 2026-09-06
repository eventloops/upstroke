---
id: PR207-AUDIT-WRITE-WINDOWS-OPEN
severity: P2
disposition: deferred
category: security-trust
pr: 207
reviewed_sha: 628784013580477bd51ca39ea9329161cb2d5c25
location: scripts/pr-ready-audit.sh:528
provenance: introduced_by_feature
first_bad: PR207-READY-LABEL-ON-MOVED-HEAD
guard: deferred: the windows are between an API read and a write, which a shell script can shrink and not close; the enqueue is bound to the head by --match-head-commit and the label is documented as advisory
---

## Failure sequence

the audit reads head H and base B and judges them READY -> a push or a retarget lands between that read and the label write, or between the last re-read and the enqueue call -> the label names a head or base the audit never judged, and an enqueue bound to H by `--match-head-commit` but only re-checked for the base can queue a diff the review did not see

## What the change that takes this up should do

close the windows where GitHub lets them be closed: bind the enqueue to the base as well as the head (a merge-queue API that takes both, or a check that the queue entry's ref names the base the review used), and stop using a label as any kind of signal to other tooling; until then nothing may treat `ready-to-merge` as permission, and the residual after-enqueue retarget is read off the pull request's timeline by the next audit

Recorded by PR #207 from review passes 6, 7 and 12; the label re-check, the head-bound enqueue and the base re-read were the repairs, and this file is what they leave open.
