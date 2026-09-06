---
id: SWEEP-CHECK_INTEGRATION-003
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/check_integration.rs:534
provenance: pre_existing
first_bad:
guard: the change that implements the integration outage producer, or the sweep of `src/ladder.rs`
---

## Failure sequence

`RunStarted4.limits.max_defers` is one frozen number and two paths read it as
two different allowances.

`ladder::next_step` (`src/ladder.rs:154`) answers `Next::Defer` for an outage
while `state.defers < policy.max_defers`, where `state.defers` counts the
deferrals already taken. A task therefore takes `max_defers` deferrals and the
next outage parks.

`check_defer_allowance` in `src/topology/fold/check_integration.rs` accepts
`UnavailableOutcome::Deferred { defers }` only when `defers == taken + 1` and
`defers < max`, so the accepted counts are `1 ..= max - 1`: a candidate takes
`max_defers - 1` deferrals and the `max_defers`th outage must park. The suite
states it as behaviour — `a_deferred_verification_is_consecutive_and_within_the_frozen_allowance`
asserts the fixture's `max_defers = 2` allows exactly one deferral — and the
file's notes state it as intent ("the last one it may take is `max_defers - 1`").

With `max_defers = 2` a task defers twice and a candidate defers once, from the
same recorded limit and under the same name.

Nothing wedges today: measured at this head, nothing outside the fold, its own
tests and `src/topology/census.rs` constructs a `MergeVerificationUnavailable`
(`grep -rn 'MergeVerificationUnavailable' --include=*.rs src/`), so the engine
has no integration-outage producer yet to disagree with the fold. The
disagreement becomes live the moment one is written against `ladder::next_step`,
which is the obvious way to write it: the producer would emit
`Deferred { defers: max }` for the last deferral its own policy allows, and the
fold would refuse its own emitter's event and poison the run.

## What the change that takes this up should do

Decide which reading `max_defers` has, in `design/26` rather than in either
call site, and make the two agree. If the ladder's reading wins, the fold's
`Deferred` arm accepts `defers <= max` and the infrastructure park boundary
becomes `next <= max`; if the fold's wins, `next_step`'s outage branch defers
while `state.defers + 1 < policy.max_defers`, and every ladder test that counts
deferrals moves with it. Either way the integration outage producer is written
against the settled reading and one test drives a candidate to the boundary
through the real emitter rather than through a hand-built event.
