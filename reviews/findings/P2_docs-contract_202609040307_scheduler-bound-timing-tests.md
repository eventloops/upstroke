---
id: PR125-CLOSE-SCHEDULER-BOUND-TIMING-TESTS
severity: P2
disposition: deferred
category: docs-contract
pr: 125
reviewed_sha: 33604e648aa06fdd0551526b3b8f95d3676df7ae
location: src/agent/proc.rs:5669
provenance: introduced_by_feature
first_bad: 6da790e
guard: deferred: tests of a bounded wait observe the mechanism of the bound (the number of looks taken, or a clock the test controls), not wall time, and…
---

## Failure sequence

the closed pull request's tests of the bounded end required wall-clock time under the budget (a killed child reaped inside 500 ms) or under the budget plus a second (a child left running), measured across thread creation, and discarded a worker's `JoinHandle` -> a test thread descheduled for that long fails correct code, and an earlier test of the same class had already failed on a loaded macOS runner at `ee0e914` (run 33861839254: 134.7 ms past a 100 ms bound)

## What the change that takes this up should do

deferred: tests of a bounded wait observe the mechanism of the bound (the number of looks taken, or a clock the test controls), not wall time, and join every worker they spawn; §12's rule that concurrency tests do not depend on scheduling luck

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
