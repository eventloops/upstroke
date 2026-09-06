---
id: SWEEP-CHECKEND-003
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/check_attempt.rs:149
provenance: pre_existing
first_bad:
guard: the sweep of src/topology/fold/check_attempt.rs, or the change that lands the emitter of a `HumanBinding` spawn
---

## Failure sequence

`check_admission` compares a `HumanBinding` spawn's authorized agent list with the same list on
its frozen entry's ladder, and `check_new_question` checks that the embedded question has an
identity, a context and at least one option -> nothing compares the two lists, so a spawn whose
question offers three options and whose admission authorizes two bindings is registered ->
option 2 is then unanswerable in both directions, and this is exercised by the fixture that is
already in the tree: `clip_to_human_binding` in src/topology/fold/tests.rs pairs the
three-option `question()` fixture with a two-agent list. An answer choosing option 2 with no
override is refused by `check_question_answered` for naming no binding; the same answer with an
override is refused because the authority has no entry at that option
("authorized 2 binding(s) and this chose 2"). The answer side is right and is now pinned by
`an_answer_may_not_take_an_option_its_admission_authorized_no_binding_for`; what is missing is
the refusal at the event that creates the trap. A task parked on that question can be answered
only by choosing an option below the authorized count, and the log records nothing saying so.

## What the change that takes this up should do

Refuse the spawn, not the answer: in `check_admission`'s `HumanBinding` arm, where the event's
options and the entry's are already compared, also require that the embedded question offers
exactly as many options as the admission authorizes bindings, and refuse with
`UnanswerableQuestion` -- the variant that already exists for a question a task cannot continue
from -- naming both counts. `check_ladder` already refuses a `HumanBinding` ladder that offers
no agent at all, so this is the same proposition one step further: every option a person can
choose has a binding behind it. Pin it with a spawn whose question has one more option than the
admission authorizes.
