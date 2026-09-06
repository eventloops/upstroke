---
id: PR7-R3-SETTLE-LADDER-POSITION-RUNG-HALF
severity: P3
disposition: deferred
category: correctness
pr: 7
reviewed_sha: 
location: src/topology/fold.rs
provenance: pre_existing
first_bad: PR7-FOLD-LADDER-POSITION
guard: PR8
---

## Failure sequence

`ladder_position` accumulates two values and only one of them is witnessed on the read side.
The `attempts_on_rung` half has a driver-side witness; the `rung` half does not, so replacing the
driver's read of `rung` with a constant leaves the suite green. This is §4's recurring class —
"an accumulator's witness proves the accumulation and not the read" — and it is that class's fourth
occurrence, found inside the repair the class was filed from.

## What the change that takes this up should do

Write the second witness: replace the driver's `rung` reader with a constant and require a
**named** test to fail. A fixture that cannot make the value observable has not tested the read at
all, so the fixture needs a task that has actually escalated — the ladder position needs a spent
allowance the same way the deferral count needed a prior deferral in the log.

Recorded in `reviews/FINDINGS.md` §20 and named again in §4's recurrence table. Severity is this migration's judgement: a test-sufficiency gap in escalation bookkeeping, with no reproduction of a wrong rung in production.
