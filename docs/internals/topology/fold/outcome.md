# `src/topology/fold/outcome.rs`

Extended notes for [`src/topology/fold/outcome.rs`](../../../../src/topology/fold/outcome.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The derived outcome (INV-15) and the structural predicates it and the
selection accessors are both answered from.

## `impl RunState` › `pub(super) fn derived_outcome(&self) -> DerivedOutcome {`

-----------------------------------------------------------------------
derived_outcome
-----------------------------------------------------------------------

## `impl RunState` › `pub(super) fn common(&self) -> bool {`

No generation is open and no integration transaction is unresolved.

## `impl RunState` › `pub(super) fn structurally_admissible(&self) -> bool {`

Some task could be dispatched, retried, or integrated from this state
alone. Budget, capacity and runner availability are not consulted.

The key range is derived once, by converting the length: a `TaskKey` is
a `u32`, so a length past `u32::MAX` narrows the domain scanned rather
than mapping every index past the boundary onto key `u32::MAX`, which
is a different task and would answer this predicate about it.

## `impl RunState` › `pub(super) fn dispatch_lease_check(`

A repair dispatch is never lease-blocked; an ordinary one is blocked by
any overlapping active lease of another owner.

The predicted region is not in the log until the dispatch that takes it,
so the check the *fold* can make is over the run's own leases: a task
with a repo-wide prediction is admissible exactly when nothing is held.

The owner is the generation the dispatch *would* take, and
`check_dispatched` refuses any other: a dispatch names generation
`generations.len()` or it is not dense. The caller has already proved
the task exists — [`Self::ready`] reads it out before it asks — so the
`TaskFold` is a parameter rather than a second lookup with a fallback,
which is what the earlier form was: an absent task fabricated
generation 0 and a count past `u32::MAX` aliased onto `u32::MAX`, both
of them an owner the run could hold a lease under. A count that does not
fit refuses instead, and refusing is the true answer — a task with more
generations than a `GenerationId` can name has no dispatch left to
admit.

## `impl RunState` › `pub(super) fn integration_admissible(&self) -> bool {`

`permits.provisional_reservations` gives integration selection the
`{pipeline, merge}` pair, and `deadlock_freedom` takes a reservation
"only when the derived count permits" — so the entitlement is a clause
of admissibility here for the same reason it is one in [`Self::ready`]
and [`Self::ready_retry`], and not a check the caller is trusted to
remember. `permits.pipeline` counts an unresolved integration
transaction among the held, which is the other half of the same
statement: a selector that admitted an integration while the count was
at `max_parallel` would open the entitlement that is already held.

## `impl RunState` › `pub(super) fn blocked_tasks(&self) -> BTreeSet<TaskKey> {`

Every task that can never run because a failure sits in its transitive
dependency closure.

The set is keyed by [`TaskKey`], which is what a dependency is: the
membership test against `entry.deps` used to convert every key to an
index to ask, and the set it was asked of was a set of indices that
nothing else in the fold is keyed by. An index that no key can name is
neither terminal nor blocked, so [`Self::complete_shape`] answers `false`
for it and the run is not `Complete` — the conservative half, and the
one that cannot invent a completion.

## `pub(super) fn blocked_tasks(&self) -> BTreeSet<TaskKey>` › `loop {`

To a fixed point, not in one pass. A *repair*'s dependencies refer
only backwards, but an original's keys are assigned in plan order
(`keys_by_display_id`) and plan order is not topological order, so
an ordinary plan can have a task depend on a later key. One forward
pass would then decide that task before it had decided what the task
waits on, and a failure two hops away would go unseen — which is the
difference between "directly failed dependency" and the transitive
closure the packet asks for.

Each round adds at least one member or stops, and membership only
grows, so this runs at most `tasks.len()` rounds.

## `pub(super) fn eligible_continuation(&self, key: TaskKey) -> Option<GenerationId> {`

No eligible continuation when the run is ending, its lineage is
waiting on a question, or the task has no generation awaiting attempt 1.

It asks for no pipeline entitlement, and that is the one clause it does
not share with [`Self::ready`] and [`Self::ready_retry`]. An
`OpenNoAttempt` generation already holds one
(`GenerationClass::holds_pipeline`), so the attempt this admits takes no
second; adding the check for symmetry would wedge a run at its ceiling,
every generation it holds being one it may not attempt.

## `pub(super) fn pipeline_reservable(&self) -> bool {`

The conversion saturates deliberately. `max_parallel` is a `u32` and the
held count a `usize`: a limit that does not fit is a limit no reachable
count can meet, and `usize::MAX` is the narrowest bound that is still
true of it. Saturating a *limit* upwards admits nothing the real limit
would refuse, which is why this one is not a refusal the way
[`Self::dispatch_lease_check`]'s is.
