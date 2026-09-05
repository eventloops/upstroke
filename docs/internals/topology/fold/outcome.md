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

## `impl RunState` › `pub(super) fn dispatch_lease_check(&self, key: TaskKey, entry: &TaskEntry) -> bool {`

A repair dispatch is never lease-blocked; an ordinary one is blocked by
any overlapping active lease of another owner.

The predicted region is not in the log until the dispatch that takes it,
so the check the *fold* can make is over the run's own leases: a task
with a repo-wide prediction is admissible exactly when nothing is held.

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

## `impl RunState` › `pub(super) fn blocked_tasks(&self) -> BTreeSet<usize> {`

Every task that can never run because a failure sits in its transitive
dependency closure.

## `pub(super) fn blocked_tasks(&self) -> BTreeSet<usize>` › `loop {`

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
