---
id: PR174-REVIEW4-1-PERFORMED-LOG-CLONE-UNJUSTIFIED
severity: P3
disposition: deferred
category: performance
pr: 174
reviewed_sha: 9a49f1a50424e4d726dd5b6065dbe74123dbdb43
location: src/runner/container/tests.rs:116
provenance: introduced_by_feature
first_bad: 9a49f1a50424e4d726dd5b6065dbe74123dbdb43
guard: "PR #174's follow-up, or whichever sweep next opens src/runner/container/tests.rs; the same change as PR174-REVIEW3-2-SCHEDULE-SNAPSHOT-CLONE-UNJUSTIFIED"
---

## Failure sequence

Pass 4 of PR #174 (gpt-6-astra at max, on the delta `ce2965be..9a49f1a5`):
`RacingObservation::assert_every_pause_was_performed_as_decided` clones the
whole thread-local performed log solely to compare it outside the
`RACING_PERFORMED.with` closure -> nothing requires an owned snapshot -> the
same §6 deviation as `PR174-REVIEW3-2-SCHEDULE-SNAPSHOT-CLONE-UNJUSTIFIED`, a
second occurrence. Test code, one small vector per observed loop; no behaviour
is affected.

## What the change that takes this up should do

Compare over a shared borrow inside `RACING_PERFORMED.with`, together with the
schedule vector's clone in `assert_every_sleep_was_slept`. Recorded rather than
repaired in #174 on the owner's instruction that P3 findings from its review
passes are ledgered, not fixed in the same change.
