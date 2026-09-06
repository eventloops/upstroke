# `src/topology/fold/start.rs`

Extended notes for [`src/topology/fold/start.rs`](../../../../src/topology/fold/start.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

`run_started`, and the dispatch of everything after it.

The two checks that bracket a run: the one that builds the registry a fold
is derived against, and the match that routes every later event to the
check that owns it.

## `impl TopologyFold` › `pub(super) fn check_run_started(`

-----------------------------------------------------------------------
run_started
-----------------------------------------------------------------------

## `impl TopologyFold` › `started`

refusals[5], first half: the record must name everything needed to
re-establish the runner. The digest is not required — it is the
manifest digest when the runtime reported one (INV-23).

## `impl TopologyFold` › `if started.limits.max_parallel == 0 {`

The entitlement this run freezes must admit work. `max_parallel = 0`
makes `pipeline_reservable` false for the life of the run, so nothing
is ever structurally admissible, no state the run reaches has an
outcome to derive, and every `run_finished` is refused — the log folds
to a state with no exit. DESIGN §26, "Frozen limits". The other two
limits have no such floor: each gates a branch that still answers at
zero.

## `impl TopologyFold` › `if started.normalized_plan_digest != self.inputs.normalized_plan_digest {`

refusals[4]: both digests, against the bytes this reader was handed.

## `impl TopologyFold` › `for entry in registry.entries() {`

Ladder validation at the fold boundary: a malformed ladder is refused
before it is stored, not when something tries to climb it.

## `impl TopologyFold` › `pub(super) fn check_started_run(`

-----------------------------------------------------------------------
Everything after run_started
-----------------------------------------------------------------------

## `impl TopologyFold` › `if let Some(outcome) = run.finished.as_ref() {`

refusals[21]: a Complete or Halted run is finalized and then refused,
never continued. A Parked or BudgetExceeded run continues, and the
only event that continues it is the resume that opens the next epoch.

## `impl TopologyFold` › `pub fn derived_outcome(&self) -> DerivedOutcome {`

-----------------------------------------------------------------------
The derived outcome
-----------------------------------------------------------------------

## `impl TopologyFold` › `pub fn derived_outcome(&self) -> DerivedOutcome {`

The total outcome function (`decisions.run_end_policy.derived_outcome`).

Computed from durable state alone: no spend, no capacity, no runner
availability, no clock. The legacy precedence is preserved —
halt > budget > parked > complete — and pending backoff makes `Parked`
and `Complete` [`DerivedOutcome::NotEnding`] without ever blocking
`Halted` or `BudgetExceeded`.

A run that has not started is [`DerivedOutcome::NotEnding`]: nothing has
been recorded, so nothing has ended.

## `pub(super) fn check_ladder(key: TaskKey, ladder: &FrozenLadder) -> Result<(), FoldError> {`

Whether a frozen ladder is one an attempt could actually climb.

Fold-boundary work rather than registry work: the registry derives a ladder
from whatever the run recorded, and this decides whether that ladder may
enter a fold's state. The three malformations of the floor and the tier list
are all invisible to the registry, which copies `task.min_tier` into `floor`
and the recorded tiers into `tiers` and compares neither with the other — a
floor above its ceiling clips to nothing on the first escalation; a floor
above the tier the chain starts at binds nothing at all, because the position
starts at the first rung and the attempt validated against that rung then runs
beneath the recorded floor (DESIGN §26, "Frozen ladders"); and a tier list
that does not ascend makes "the next rung" mean two different things depending
on whether it is read by position or by tier.
