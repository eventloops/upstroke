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

INV-09: the exact-base decision is made before any staging effect, so
a candidate whose base *is* the head is published fast and is never
cherry-picked or re-verified.

## `impl RunState` › `pub(super) fn prepared_candidate(`

What `candidate_prepared` recorded for this candidate.

## `impl RunState` › `pub(super) fn check_verification_unavailable(`

--- merge_verification_unavailable ------------------------------------

## `impl RunState` › `let max = self.started.limits.max_defers;`

The boundary is the same number read from both sides: the deferral
this outage *would* be. `coordinator_integration.dispositions` gives
Infrastructure `Deferred{defers}` while `defers < max_defers` and
`Parked{question}` at `max_defers`, so the two arms partition on
`next` and neither may take the other's cell.

## `impl RunState` › `if *defers != next {`

refusals[17]: consecutive, and within the frozen allowance.

## `impl RunState` › `if *defers >= max {`

refusals[16]: "Deferred at max_defers" is refused. The
allowance is the number of deferrals the run may *take*, so
the last one it may take is `max_defers - 1` and the outage
that would be the `max_defers`th parks instead.

## `impl RunState` › `if matches!(unavailable.cause, UnavailableCause::Infrastructure { .. })`

refusals[16], the other half: `HumanRequired` always parks,
whatever the count, and an Infrastructure outage parks
exactly at the boundary — one earlier would consume an
allowance the run still has.

## `impl RunState` › `pub(super) fn check_verification_interrupted(`

--- merge_verification_interrupted ------------------------------------

## `impl RunState` › `pub(super) fn check_merge_prepared(&self, prepared: &MergePrepared) -> Result<(), FoldErr…`

--- merge_prepared ----------------------------------------------------

## `pub(super) fn check_merge_prepared(&self, prepared: &MergeP…` › `prepared`

A1's intra-event relations first: a record that disagrees with itself
is refused before it is compared with anything else.

## `pub(super) fn check_merge_prepared(&self, prepared: &MergeP…` › `self.check_transaction_start(KIND, prepared.sequence, &prepared.candidate())?;`

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

## `impl RunState` › `pub(super) fn check_merge_rejected(&self, rejected: &MergeRejected) -> Result<(), FoldErr…`

--- merge_rejected ----------------------------------------------------

## `pub(super) fn check_merge_rejected(&self, rejected: &MergeR…` › `self.check_transaction_start(KIND, rejected.sequence, &rejected.candidate)?;`

A conflict is decided at the cherry-pick, before any
verification starts: it opens and closes its own transaction.

## `pub(super) fn check_merge_rejected(&self, rejected: &MergeR…` › `let entry = self.entry(KIND, rejected.candidate.key)?;`

The lease effect and the repair are one decision: a non-lineage
candidate's lease becomes the new lineage's, and a lineage member's
rejection widens the lineage it already belongs to.

## `impl RunState` › `pub(super) fn lineage_members(&self, root: TaskKey) -> u32 {`

How many repairs lineage `root` already holds.

## `impl RunState` › `pub(super) fn check_task_merged(&self, merged: &TaskMerged) -> Result<(), FoldError> {`

--- task_merged -------------------------------------------------------

## `pub(super) fn check_task_merged(&self, merged: &TaskMerged)…` › `if merged.satisfies != *satisfies {`

"copied exactly from the authorization", not re-derived here.
