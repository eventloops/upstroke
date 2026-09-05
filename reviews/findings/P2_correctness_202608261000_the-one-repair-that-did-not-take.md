---
id: PR5-EVENTS-051
severity: P2
disposition: deferred
category: correctness
pr: 5
reviewed_sha: 
location: src/events/log.rs
provenance: pre_existing
first_bad: 
guard: the project owner — the G2 adjudication sitting
---

## Failure sequence

A withheld-mutation-catalogue entry targeting the legacy `EventLog::append` flush step. It
`SURVIVED` at catalogue time, was then ruled repaired, and on final re-measurement against the
shipped tree it still `SURVIVED`. Of the original 38 repairs it is the one that did not take.

## What the change that takes this up should do

Re-derive the killing assertion for the flush step and prove it kills. This entry is the one
of §15's six that is qualitatively different: the other five are `KILLED` and need adjudication
only as to whether that is a real detection-power loss or an equivalent-mutant artifact of
re-expressing a prose-recorded mutation. This one is a flat regression candidate, still surviving,
and does not need adjudication to be believed — it needs a test.

Recorded in `reviews/FINDINGS.md` §15, “the catalogue re-measured against the shipped code”. Severity is this migration's judgement.
