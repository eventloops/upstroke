---
id: PR174-REVIEW4-2-TERMINAL-GUARD-MUTATION-ATTRIBUTED-TO-WRONG-ASSERTION
severity: P3
disposition: deferred
category: docs-contract
pr: 174
reviewed_sha: 9a49f1a50424e4d726dd5b6065dbe74123dbdb43
location: docs/internals/runner/container/tests.md:2042
provenance: introduced_by_feature
first_bad: 9a49f1a50424e4d726dd5b6065dbe74123dbdb43
guard: "PR #174's follow-up, or whichever sweep next opens docs/internals/runner/container/tests.md"
---

## Failure sequence

Pass 4 of PR #174 (gpt-6-astra at max, on the delta `ce2965be..9a49f1a5`):
the `note_racing_performed` note says all four native mutations fail at
`assert_every_pause_was_performed_as_decided` -> with the schedule's terminal
`Done` arm removed, failure 64 decides `Sleep`, the performer requests that
sleep, and the performed-vs-decided equality passes -> that mutation is caught
by the independent schedule assertions (`the_racing_pause_is_sixteen_yields_
then_forty_seven_sleeps_and_nothing_after_the_last`, and the held-past-budget
test's schedule equality), which the archived witness
`witness-no-terminal-guard-ff5c892e...log` shows failing at
`src/runner/container/tests.rs:4058`. The combined coverage works; the sentence
attributes it to the wrong assertion.

## What the change that takes this up should do

Say that three of the four mutations fail at
`assert_every_pause_was_performed_as_decided` and the terminal-guard mutation
at the schedule assertions. Recorded rather than repaired in #174 on the
owner's instruction that P3 findings from its review passes are ledgered, not
fixed in the same change.
