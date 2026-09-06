# `src/topology/fold/check_end.rs`

Extended notes for [`src/topology/fold/check_end.rs`](../../../../src/topology/fold/check_end.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The question checks, the budget stop, and the end of a run.

## `impl RunState` › `pub(super) fn check_question_raised(&self, question: &FrozenQuestion) -> Result<(), FoldE…`

--- questions ---------------------------------------------------------

## `impl RunState` › `if self.halted_epoch == Some(self.epoch) {`

refusals[20]: answers are not ingested in an epoch after a halting
settlement or a budget stop.

## `impl RunState` › `answered`

refusals[13], A1's half: the answer must agree with itself.

## `impl RunState` › `match (binding_override, &open.binding) {`

refusals[12] / `task_registry.binding_override`: an override is
validated "against the frozen options of that task's open
HumanBinding question". A1's `self_consistency` has already
proved the override names this answer's task, question and
option; what is left — and what no other check makes — is that
there *is* such an authority and that the agent it names is the
one that authority froze at that index.

## `impl RunState` › `pub(super) fn check_budget_exceeded(`

--- budget_exceeded ---------------------------------------------------

## `impl RunState` › `pub(super) fn check_run_finished(&self, finished: &RunFinished4) -> Result<(), FoldError>…`

--- run_finished ------------------------------------------------------

## `pub(super) fn check_run_finished(&self, finished: &RunFinis…` › `let derived = self.derived_outcome();`

refusals[19] / INV-15: the recorded outcome is the derived one, and
the derived one is not NotEnding.

## `pub(super) fn check_question_can_park_lineage(`

A question cannot suspend an unresolved process or an already
authorized publication. Standalone admission questions use this check;
a settlement's embedded question accounts for its own closing work.
