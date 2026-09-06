# `src/topology/fold/tests/questions.rs`

Extended notes for [`src/topology/fold/tests/questions.rs`](../../../../../src/topology/fold/tests/questions.rs).

## `replay` › `let replay =`

Replay owns an independent copy of the same frozen inputs.

## `a_new_bare_or_standalone_admission_question_cannot_enter_an_active_lineage_transaction` › `trace.record(settle(`

Settle the sibling without a question so transaction ownership is the
only reason the later questions must be refused.

## `bare_questions_and_active_generations_exclude_each_other_across_a_lineage` › `trace.record(raised("quiet", queried));`

Question first: the affected lineage cannot acquire a generation.

## `bare_questions_and_active_generations_exclude_each_other_across_a_lineage` › `trace.record(dispatched);`

Generation first: bare or standalone questions cannot enter it.
