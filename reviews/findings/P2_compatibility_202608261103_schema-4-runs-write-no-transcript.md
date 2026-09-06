---
id: PR7-R3-ATTEMPT-004-NO-TRANSCRIPT-NO-GATE-LOG
severity: P2
disposition: deferred
category: compatibility
pr: 7
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: the project owner, for the G2 erratum list
---

## Failure sequence

Nothing on the schema-4 path writes `transcripts/<stem>-<attempt>.json`, so the
operator-facing evidence the legacy engine used to write is absent on schema-4 runs. §11.1's
feedback mechanism itself is intact — `judge` builds the gate tail and retries are told — so what
is missing is the artifact an operator reads after the fact, not the feedback the next attempt
gets.

## What the change that takes this up should do

Decide whether the schema-4 path owes the transcript artifact, and record the answer on the
G2 erratum list. This is a real capability gap rather than a defect in what exists, so the repair
is a decision followed by a writer, not a fix to a broken writer.

Recorded in `reviews/FINDINGS.md` §20. Its stated pair, `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4`, is fixed; this one is not. Severity is this migration's judgement.
