# `src/topology/fold/check_integration.rs`

Extended notes for [`src/topology/fold/check_integration.rs`](../../../../src/topology/fold/check_integration.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The integration checks: opening a transaction, the verification records,
and the publication relations a merge is judged against (INV-09).

## `impl RunState` › `pub(super) fn check_transaction_start(`

--- integration: starting a transaction --------------------------------

## `impl RunState` › `pub(super) fn check_transaction_start(`

The checks every first append of an integration transaction shares:
nothing else is open, the sequence is the next dense one, and the
candidate is the first *eligible* entry in the queue.

## `impl RunState` › `if let Some(open) = &self.transaction {`

refusals[7]: one integration transaction at a time.

## `impl RunState` › `if sequence.0 != self.next_sequence {`

refusals[6] / refusals[10]: sequences are dense from 0 across the run.

## `impl RunState` › `let first = self`

refusals[8]: the first eligible entry is integrated, and the fold
refuses an integration start for any other candidate.

## `impl RunState` › `pub(super) fn open_transaction(`

The open transaction this event must belong to (refusals[6]).

## `impl RunState` › `pub(super) fn check_verification_started(`

--- merge_verification_started ----------------------------------------

## `impl RunState` › `if started.expected_head == prepared.base_sha {`

The exact-base decision is made before any staging effect, so a
candidate whose base *is* the head is published fast and is never
cherry-picked or re-verified: `design/26_design_merge_queue_protocol.md`
§1, "The fast path publishes that exact commit when its parent is still
the integration head", and the effect-site section's own restatement,
"an integration whose base is still the head publishes the exact
candidate: no staging worktree is added, nothing is cherry-picked, and
no prepared pin is taken". `INV-09` is the retired packet's label for
the same rule and is kept only because other notes in this family cite
it; no living design section defines any `INV-` id.

## `impl RunState` › `pub(super) fn prepared_candidate(`

What `candidate_prepared` recorded for this candidate.

## `impl RunState` › `pub(super) fn check_verification_unavailable(`

--- merge_verification_unavailable ------------------------------------

## `fn check_defer_allowance(`

The boundary is the same number read from both sides: the deferral
this outage *would* be. `coordinator_integration.dispositions` gives
Infrastructure `Deferred{defers}` while `defers < max_defers` and
`Parked{question}` at `max_defers`, so the two arms partition on
`next` and neither may take the other's cell.

It is a free function rather than a method because it decides on four
values and nothing else, and can therefore be pinned directly. The
`RunState` a method would need is built by the `RunStarted4`, plan,
chain and registry-digest fixture in `src/topology/fold/tests.rs`,
which is queue row 39 and outside this sweep's scope.

The caller supplies `taken` from the candidate's own queue entry and
`max` from the frozen limits, in that order. That seam is measured, not
assumed: swapping the two arguments fails seven tests of
`topology::fold`.

## `fn check_defer_allowance(` › `if *defers != next {`

refusals[17]: consecutive, and within the frozen allowance.

## `fn check_defer_allowance(` › `if *defers >= max {`

refusals[16]: "Deferred at max_defers" is refused. The
allowance is the number of deferrals the run may *take*, so
the last one it may take is `max_defers - 1` and the outage
that would be the `max_defers`th parks instead.

This reading of `max_defers` is one lower than
`ladder::next_step`'s, which defers while `state.defers <
max_defers` and so takes `max_defers` of them. One frozen
number, two allowances; recorded as
`SWEEP-CHECK_INTEGRATION-003` rather than settled here,
because nothing produces a `merge_verification_unavailable`
yet and the two readings have to be reconciled in `design/26`
rather than in either call site.

## `fn check_defer_allowance(` › `if matches!(cause, UnavailableCause::Infrastructure { .. }) && next < max {`

refusals[16], the other half: `HumanRequired` always parks,
whatever the count, and an Infrastructure outage parks
exactly at the boundary — one earlier would consume an
allowance the run still has.

`next < max` and not `next != max`. `taken` is only ever set
from an accepted `Deferred { defers }`, whose own check
refuses `defers >= max`, so `taken <= max - 1` and `next <=
max`: for every allowance of one or more the two predicates
are the same. They differ only at `max_defers == 0`, where
`next` is 1 and `next != max` refused the park while the
`Deferred` arm refused the deferral — an empty partition, so
no infrastructure outage could be folded at all. Zero is
`ladder::next_step`'s "park the first outage" and is
reachable: `RunOptions.max_defers` is a public field with no
validator.

## `impl RunState` › `pub(super) fn check_verification_interrupted(`

--- merge_verification_interrupted ------------------------------------

## `impl RunState` › `pub(super) fn check_merge_prepared(&self, prepared: &MergePrepared) -> Result<(), FoldErr…`

--- merge_prepared ----------------------------------------------------

## `pub(super) fn check_merge_prepared(&self, prepared: &MergeP…` › `prepared`

A1's intra-event relations first: a record that disagrees with itself
is refused before it is compared with anything else.

## `pub(super) fn check_merge_prepared(&self, prepared: &MergeP…` › `self.check_transaction_start(`

A fast publication opens and closes its own transaction: no
verification ran, so there is nothing already open.

## `pub(super) fn check_merge_prepared(&self, prepared: &MergeP…` › `if prepared.expected_head != candidate_record.base_sha {`

refusals[9]: expected_head == the candidate's recorded base,
proposed_sha == the candidate's recorded commit.

## `pub(super) fn check_merge_prepared(&self, prepared: &MergeP…` › `if prepared.expected_head != *expected_head {`

refusals[22], fold half: the head the CAS expects is the head
the transaction read.

## `pub(super) fn check_merge_prepared(&self, prepared: &MergeP…` › `if prepared.proposed_sha != *proposed_sha {`

refusals[9]: the proposal is the one that was verified — the
pinned proposal for a stale publication, the head itself for
an already-present one.

## `pub(super) fn check_merge_prepared(&self, prepared: &MergeP…` › `let derived = self.satisfies_closure(prepared.key);`

refusals[10]: the closure this publication settles is derived, not
asserted.

## `impl RunState` › `pub(super) fn satisfies_closure(&self, key: TaskKey) -> Vec<TaskKey> {`

Every task one publication settles: the candidate's own task and, for a
repair, every entry back up its lineage to the root.

A repair carries the work of everything it descends from — that is what
it was materialized from — so publishing it settles the whole chain.
Ascending key order, because the value is derived and two readers must
derive the same list.

The walk is a path and not a tree: `check_spawn` refuses a lineage whose parent
is not a backwards key, and `check_dispatched` refuses a task that is not
`Pending`, so a rejected task — which the rejection leaves `AwaitingRepair` —
never opens a second generation and never acquires a second child. Two
consequences, both dispositioned rather than repaired. The `lineage.parent >=
current` break is unreachable for any entry `check_spawn` admitted, and if it
ever did fire it would shorten the derived closure and refuse the publication as
`InvalidSatisfies` rather than settle the wrong set. The `dedup` after the sort
can never remove an element, because each pushed parent is strictly smaller than
the key it was reached from; it is kept as the second half of the sort idiom,
not as a live guard.

## `impl RunState` › `pub(super) fn check_merge_rejected(&self, rejected: &MergeRejected) -> Result<(), FoldErr…`

--- merge_rejected ----------------------------------------------------

## `pub(super) fn check_merge_rejected(&self, rejected: &MergeR…` › `self.check_transaction_start(`

A conflict is decided at the cherry-pick, before any
verification starts: it opens and closes its own transaction.

## `pub(super) fn check_merge_rejected(&self, rejected: &MergeR…` › `let entry = self.entry(MERGE_REJECTED, rejected.candidate.key)?;`

The lease effect and the repair are one decision: a non-lineage
candidate's lease becomes the new lineage's, and a lineage member's
rejection widens the lineage it already belongs to.

## `impl RunState` › `pub(super) fn lineage_members(&self, root: TaskKey) -> u32 {`

How many repairs lineage `root` already holds.

The count is `usize` and the recorded index is `u32`, so the conversion is
checked and saturates rather than panicking. It cannot saturate in a run this
crate can build: a key is its own dense index into the registry
(`check_spawn` refuses `spawn.key.index() != self.registry.len()`) and a key is
a `u32`, so the registry cannot hold `u32::MAX` entries, let alone that many
members of one lineage. Saturating rather than wrapping is the deliberate
choice §5 asks for: a wrapped count would answer a small index and accept a
repair numbered wrongly, where `u32::MAX` matches no index any log can record.

## `impl RunState` › `pub(super) fn check_task_merged(&self, merged: &TaskMerged) -> Result<(), FoldError> {`

--- task_merged -------------------------------------------------------

## `pub(super) fn check_task_merged(&self, merged: &TaskMerged)…` › `if merged.satisfies != *satisfies {`

"copied exactly from the authorization", not re-derived here.

## `check_merge_prepared` › `if self.lineage_has_question(prepared.key) {`

A sibling attempt can settle with an embedded question after this
verification started. Recheck before authorizing the ref move.

## `fn check_proposal_pin(`

The proposal pin, over the whole of `VerificationBasis` rather than its
stale-clean half. A stale-clean publication pins the proposal the cherry-pick
produced and must name the ref its verification pinned (refusals[9], the pin
half). An already-present publication has nothing to pin: `design/26` §1 —
"the queue does not manufacture an empty commit" — so no proposal object
exists, and both producers record `None` for it (`src/topology/census.rs`'s
already-present candidates, and `tests.rs`'s fixture). A record that carries a
pin there names an object no run created, and is refused.

Written as a `match` over both variants rather than the `if let` it replaced,
so a third basis is a compile error rather than an unconstrained pin. The
caller passes `prepared.prepared_ref.as_ref()`, and that seam is measured:
passing `None` unconditionally fails six tests of `topology::fold`, because
the suite accepts well-formed stale-clean publications that do carry their pin.

## `fn rejection_lineage_root(`

The lease effect and the rejected task's lineage are one decision, and this
names all four of their combinations. Two are the protocol: an ordinary
candidate's rejection creates the lineage it roots, and a member's widens the
lineage it already belongs to. Two are the record disagreeing with itself, and
they share one refusal.

They are named rather than left to a wildcard so that a third
`RejectionLeaseEffect` variant fails to compile here instead of falling into a
generic message. All four cells are pinned by this file's own suite, which is
where they have to be: the fixture that would build a lineage member under
integration lives in queue row 39's file.

One of this function's two call-site seams is measured and does **not** hold.
Substituting the caller's `entry.lineage` with a constant `None` leaves the
whole `topology::fold` suite green, because no test in the tree rejects a
candidate that is itself a lineage member: the rejection suite's accepted case
creates a lineage, and its widening case names a lineage the rejected task does
not belong to and is refused either way by an `is_err()` assertion. So the
successful `WidensLineage` path of `check_merge_rejected` has never been
exercised end to end. That is a gap in row 39's suite that this extraction
measured rather than made — the same cell of the inline match it replaced was
unexercised — and it is `SWEEP-CHECK_INTEGRATION-004`. The other seam does
hold: passing the repair's key instead of the rejected candidate's fails twenty
tests.

## `fn check_lease_release(`

The same shape one event later: a publication releases the candidate's own
lease, or the lineage lease when it settles that lineage's root, and the
crossing of `MergeLeaseRelease` with "is the published task a lineage member"
has four cells, all named.

The settled root comes from `self.entry(TASK_MERGED, ..)?`, not from
`self.registry.get(..)`: an unknown key is `UnknownKey` rather than an absence
that reads as "not a lineage member" and silently demands a candidate release
(§7 — only an actual not-found becomes absence). `check_merge_rejected` already
made the same lookup that way. The key cannot be unknown on this path — the
transaction's candidate was checked by `prepared_candidate` before the
transaction opened — so this is the honest shape rather than a repair, and it is
recorded as one: restoring the `registry.get(..)` form leaves the whole
`topology::fold` suite green, which is what an unreachable branch should do.

The other seam does hold: forcing the settled root to `Some(..)` for every
publication fails twenty-seven tests.
