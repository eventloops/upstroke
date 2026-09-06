---
id: PR174-REVIEW3-2-SCHEDULE-SNAPSHOT-CLONE-UNJUSTIFIED
severity: P3
disposition: deferred
category: performance
pr: 174
reviewed_sha: ce2965be96799b80a785bc4bc2d7eb3f240efb0e
location: src/runner/container/tests.rs:92
provenance: introduced_by_feature
first_bad: ce2965be96799b80a785bc4bc2d7eb3f240efb0e
guard: "PR #174's follow-up, or whichever sweep next opens src/runner/container/tests.rs"
---

## Failure sequence

Pass 3 of PR #174 (gpt-6-astra at max, on the delta `b3a346b1..ce2965be`):
`RacingObservation::assert_every_sleep_was_slept` clones the whole thread-local
schedule `Vec` solely to iterate over it outside the `RACING_SCHEDULE.with`
closure -> the assertion consumes no owned result and nothing states a snapshot
requirement -> a nontrivial clone with no reason, which standards §6 requires.
Test code, one small vector per observed loop; no behaviour is affected.

## What the change that takes this up should do

Iterate over a shared borrow inside `RACING_SCHEDULE.with` and drop the clone,
or state why a snapshot is wanted. Recorded rather than repaired in #174 on the
owner's instruction that P3 findings from its review passes are ledgered, not
fixed in the same change.
