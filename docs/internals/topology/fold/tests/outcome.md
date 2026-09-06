# `src/topology/fold/tests/outcome.rs`

Extended notes for [`src/topology/fold/tests/outcome.rs`](../../../../../src/topology/fold/tests/outcome.rs).

## Module

The selection accessors of [`super::super::outcome`], at the clauses the
rest of the suite reaches only through something that short-circuits
first.

## `fn four_offers() -> TopologyFold {`

One state holding all four offers at once: `alpha` dispatchable, `zeta`
retryable in this incarnation, `beta` continuable on an open generation
with no attempt, and `mid`'s candidate queued and eligible. It is the
fixture `a_poisoned_fold_authorises_nothing_while_still_reporting_what_it_holds`
uses, for the same reason: four independent authorisations, one state, so
a clause that withdraws all four is distinguishable from four that each
withdraw one.

## `fn a_run_that_is_ending_offers_no_dispatch_retry_continuation_or_integration() {`

The `!run_is_ending()` clause of each accessor. Nothing else in the suite
pins it: `derived_outcome` answers `Halted` or `BudgetExceeded` before it
consults `structurally_admissible`, and `select` returns
`Step::Closure` before it asks for a dispatch, a retry or an
integration — so in both callers the clause is unreachable, and deleting
all four leaves every other test green.

The budget half is the real `budget_exceeded` event, which changes
nothing but `budget_stop`, so the four offers are the same four the
assertion above just saw. The halt half sets `halted_at` directly, as
`halt_and_budget_outrank_every_structural_source_that_can_coexist_with_them`
does: a halting settlement would have to close a generation to record
itself, and that would withdraw the continuation for the wrong reason.

## `fn a_continuation_is_offered_at_the_parallel_ceiling_and_a_dispatch_is_not() {`

A continuation is deliberately *not* clauses-for-clauses with
[`RunState::ready`]: it takes no `pipeline_reservable` check, because the
generation it continues is `OpenNoAttempt` and already holds a pipeline
entitlement (`GenerationClass::holds_pipeline`). Adding the check would
wedge a run at its ceiling — every generation it holds would be one it
may not attempt. This is the test that fails when someone adds it for
symmetry.

## `fn the_integration_offered_is_the_first_eligible_candidate_and_not_merely_an_eligible_one() {`

`integration_admissible` is a boolean, so every other test of the
integration door pins *whether* a candidate is offered and none pins
*which*. `check_transaction_start` derives the first eligible entry
independently, so an accessor that offered a later one would be selected
and then refused by the fold that admitted it —
`FoldError::NotFirstEligible` at a candidate the selector chose.

The second half is the part a `first`-shaped mutation survives: with the
head of the queue parked, first *queued* and first *eligible* differ, and
only then does the queue's own eligibility filter show in the answer.
