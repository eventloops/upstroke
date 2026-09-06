# `src/engine/topology/run/tests.rs`

Repository source for these notes: [`src/engine/topology/run/tests.rs`](../../../../../src/engine/topology/run/tests.rs).
[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/engine/topology/run/tests.rs).
The relative link works in a checkout or on GitHub; the GitHub link also works from the published site.

The code is the authority for what it does. The explanatory prose is preserved below.
Each backticked part of a section heading is an exact source excerpt. Search for the final
excerpt within the preceding item when a heading names both an item and a line inside it.

## Module

The loop's branches, checked against the packet's list rather than against
the implementation.

## `fn the_transcribed_loop_branches_are_the_packets_seven() {`

The transcribed list is the packet's list — seven branches, these labels, in
this order.

`decisions.sequential_substrate.loop` names them in one sentence, split on
`->`. A branch dropped from [`LoopBranch::ALL`] would make every other test
in this file pass by asking for less, which is exactly how step (g) survived
two review rounds in `recover.rs`.

## `fn every_branch_states_what_this_build_does_with_it() {`

Every branch this build does not perform says which, and why, in the type.

**The point of this test is the third disposition.** `RefusedByCheckpoint`
is a decision the packet licenses; `NotYetImplemented` is debt. A build that
conflated them would be indistinguishable from one that had quietly dropped
a branch — and "quietly dropped a branch" is the defect this whole module
exists because of.

## `fn every_branch_states_what_this_build_does_with_it()` › `let owed: Vec<&str> = LoopBranch::ALL`

And the debt, named rather than implied. This assertion is expected to
shrink as branches land; it must never grow.

## `fn every_branch_states_what_this_build_does_with_it()` › `let elsewhere: Vec<&str> = LoopBranch::ALL`

**What is another slice's, cited rather than owed.** `ingest answers` is
not debt and is not a checkpoint refusal — the packet authorises exactly
two of those — so it carries the contract passage that assigns it.

## `fn every_branch_states_what_this_build_does_with_it()` › `assert_eq!(`

The half-built one, and both halves in the branch's own words. A branch
that performs a durable append and reports `NotYetImplemented` would be
claiming the log is untouched when it is not; one that reported
`Performed` would be claiming an attempt ran.

## `fn every_step_belongs_to_one_branch_or_to_none_for_a_reason() {`

Every `Step` a selection can produce maps to exactly one branch, or to none
for a stated reason.

The mapping is total by construction — `LoopBranch::of` matches on `Step`
exhaustively, so a new variant does not compile until someone decides which
branch it belongs to. What this test adds is the *two `None` arms*, which a
compiler cannot check: they are the claim that neither is a branch of the
loop, and each is wrong in a different and specific way if the claim slips.

## `fn a_refusal_names_the_branch_and_says_whether_anything_happened() {`

A refusal says which branch, and — this is the part that matters — whether
anything happened.

**The two messages must not be interchangeable.** A branch that performed
nothing says so, and an operator reading it knows the log is untouched. A
branch that appended and then stopped says what it did, because an operator
told "not implemented" after a durable `task_dispatched` would go looking
for a run directory that does not match the message.

## `fn a_refusal_names_the_branch_and_says_whether_anything_happened() {` › `assert!(`

**No branch is `PartlyImplemented` today**, and that is a statement about
this build rather than about the type. `ReadyRetry` was the last one and
became `Performed` when its second half landed. The variant stays because
the next branch built in halves will need it, and this assertion is what
says so out loud the moment one appears — a half-built branch is the one
shape whose refusal has to say what it already did, because by then
`attempt_started` or `task_dispatched` is durable.

## `fn a_refusal_names_the_branch_and_says_whether_anything_happened() {` › `for branch in LoopBranch::ALL {`

Every refusal names its own branch, whatever its disposition. A message
that named the wrong one would send an operator to the wrong lane.

## `fn every_driver_append_propagates_its_error() {`

**Every append the driver makes propagates its error.**

The append-error protocol is five obligations, and all five begin with the
error *reaching* the protocol. A `let _ = self.emit(..)` reaches none of
them: the fold is not poisoned, no reservation or invocation is cancelled,
and the command reports success for a run whose log does not contain the
line it just claimed to write.

Catalogue entry `PR7-SELECT-026` did exactly that to the
`Admitted::BudgetExceeded` arm and the whole suite stayed green, because the
arms whose append failure *is* armed by a fixture are not that one.

A **census rather than a fixture per arm**, for the reason the other four
single-authority censuses exist: a per-arm test proves the arm it names and
says nothing about the arm added next week. This proves the property over
every append site the driver has, including the ones not yet written.

The region is [`crate::effects::production_code`], which blanks comments and
strings — a `let _ = self.emit(` quoted in a doc comment must not fail this,
and a truncating region would let a site below the cut through, which is
`PR4-CENSUS-COMMENT-ORACLE` and is how the barrier census scanned 4.7% of
this very file.

## `fn every_driver_append_propagates_its_error()` › `let mut depth = 0_i32;`

Walk to the matching close paren, then check what follows it.

## `fn the_loop_selects_through_one_function() {`

**The loop chooses its branch through one selector.**

`decisions.sequential_substrate.loop` gives seven branches in one order, and
`select` is where that order lives. Catalogue entry `PR7-SELECT-015` added a
**second** selector — `select_rescan`, ordered Dispatch/Retry/Integrate
instead of Integrate/Retry/Dispatch — pointed `TopologyRun::step` at it, and
left canonical `select` untouched with every one of its tests still passing.
The whole suite was green.

That is the seams category in its purest form: `select.rs` is coherent,
`run.rs` is coherent, and the branch order the packet specifies is not the
one the run takes. No per-function test can see it, because each function is
right about itself.

The fifth single-authority census this slice owns, and the cheapest: the
driver reaches its branch order through exactly one call, and `checkpoint`
guards exactly that call's result. A second selector makes this count zero,
not two — which is why the assertion is on the **canonical** name rather than
on a total.

## `fn the_loop_selects_through_one_function()` › `let calls = |needle: &str| {`

Calls, not definitions — neither is defined here, but the filter is the
one the barrier census learned to use and costs nothing.

## `fn the_frozen_pool_table_is_read_through_one_seam() {`

**The frozen pool table is read through one seam.**

`AttemptPlans::pool_for` exists so that the plan builder, the reviewer
profile and the driver's `RetryRequest` reach one answer. `79cd9c8` said it
gave the rule "one production implementation" and it did not: `assembly.rs`
called `crate::capacity::pool_for` from three places, two of them
character-for-character copies of the seam's body, and the seam's only caller
was `run.rs`. `reviews/FINDINGS.md` §19, claim (4).

**The needle is a free call to `pool_for`**, through the shared
[`crate::effects::census_domain::production_calls`]. It was the literal
`capacity::pool_for(`, which reasons about one direction only — a longer
identifier colliding with it — and not about the other: `use
crate::capacity::pool_for;` followed by a bare `pool_for(...)` is the
ordinary way to write a second implementation and that literal does not
match it. Both spellings are already live in this tree. `R5-SEAMS-002`.

**What it still cannot see, stated rather than left to be found**: a second
resolution that never names the function. `capacity::pool_for` is
`pools.iter().find(…)`, and a caller walking `self.pools` inline is a second
implementation of the rule with no `pool_for` in it. A name census cannot
reach that, so what this asserts is **one named resolution**, not one
resolution.

**The count is one and not zero.** Zero would mean the seam had been rewritten
to resolve pools some other way, which is the same defect from the other
side, so the assertion is an equality.

## `fn the_frozen_pool_table_is_read_through_one_seam()` › `use crate::effects::census_domain::{Call, production_calls};`

**Free calls to `pool_for`, not the qualified spelling.** The needle was
the literal `capacity::pool_for(`, which does not match the ordinary way
to write a second implementation — `use crate::capacity::pool_for;` and
then a bare `pool_for(...)`. Both idioms are live in this tree
(`config.rs` writes the qualified form, `capacity.rs` the bare one), so
it is not a hypothetical spelling. `R5-SEAMS-002`, `PR7-R5-ATT-002`.

`Call::Free` is what separates a second implementation from the seam's
own callers: the plan builder and the reviewer profile ask
`self.pool_for(...)`, a method call, and the trait method's definition is
filtered as a definition.

## `fn the_frozen_pool_table_is_read_through_one_seam()` › `assert_eq!(`

Controls on the needle itself, both directions, because a needle that has
stopped matching reads exactly like a clean file.

## `fn both_attempt_started_arms_take_their_pool_from_an_authority() {`

**Both arms of `attempt_started` get their pool from an authority.**

`attempt_started` is appended from two places and they reach it differently:
the dispatch arm builds its plan first and reads `plan.pool`; the retry arm
appends **before** its plan exists, because `settle::retry` produces the
event and the plan is built after. Sol's `R3-SEAMS-001` is what that
asymmetry produced — the retry passed `pool: None`, so a resumed run's ledger
recorded no pool while the plan it then built resolved one, and the two
disagreed about the same attempt.

The needle is the field's value in each production `AttemptStarted4` literal.
A hard-coded `None` fails; anything that names something does not, because
this census's claim is "not invented here", not "non-empty".

### Two corrections to what this test was said to be

**It is not the only witness available, and the claim that it was is false.**
`79cd9c8`'s message argued a source census was structurally necessary because
"a retry is only reachable *within* one process … and **no driver fixture can
reach the arm**". One does: the fixture is
`recover::tests::the_retaining_incarnation_retries_in_place`, and it exists —
**named, not cited by line**. The first draft of this block quoted
`recover/tests.rs:5488` as terminal output — correct **at `c01a844`** — and the
very next commit inserted nineteen lines above it. `PR7-R6-ATT-003`, and
the rule it gives: a doc comment names an item, because a line number is a
claim about a version of a file and decays silently. The doc-comment filter
(`| grep -v '///'`) is the other half — a needle quoted here would otherwise
match its own quotation, `reviews/FINDINGS.md` §4.

It drives `TopologyRun::step` twice in one process and the second iteration
**is** the retained-generation retry. It now asserts the pool on both
`attempt_started` appends, which is the behavioural witness this census was
offered in place of. `reviews/FINDINGS.md` §19, claim (3).

**And this census does not read the file the defect was in.** The two sites
below are `attempt.rs` and `settle.rs`; the literal `None` that
`R3-SEAMS-001` found was in **`run.rs`**, which fills `settle::retry`'s
`RetryRequest`, and `settle.rs`'s own literal reads `request.pool` and was
correct throughout. Measured at `5a08f19`: restoring `pool: None` in
`run.rs` leaves this census green **and the entire suite green** — 1698 + 8
passed, 0 failed. The behavioural assertion above is what kills it. §19,
claim (2).

So this census keeps a real and narrower job: the two *literals* name an
authority rather than inventing a value. It is not a witness that the value
arriving at them is right.

## `fn the_settled_notes_separate_the_successful_and_the_failed_settlement() {`

`PR160-NOTES-SUCCESS-SETTLEMENT`. The `Progress::Settled` section of
`docs/internals/engine/topology/run.md` summarised the whole ready-dispatch
branch as ending in `attempt_finished`. That is only the rejected half.
[`TopologyRun::settle`] opens with `let Some(failure) = judgement.failure ...
else`, so an attempt nothing rejected takes [`TopologyRun::promote_candidate`]
before any failure settlement is built, and that path appends
`candidate_prepared` then `task_candidate_created` and no `attempt_finished`
at all. The fold enforces the same rule — `check_attempt_finished` refuses
`SettlementTransition::Succeeded` outright — and so do
`design/15_design_event_log_resume_run_layout.md` and
`design/26_design_merge_queue_protocol.md` §26.

So a reader using that contract to reconstruct a successful attempt's durable
record was sent looking for an event that is never written. This pins the two
settlements separately, and refuses the retired sentence by name so the claim
cannot come back under a reflow.

**It is a text pin and only a text pin.** The behaviour it describes is held
elsewhere — `recover::tests::the_driver_carries_an_accepted_attempt_through_the_candidate_sequence`
for the successful durable sequence, and the fold's
`candidate_prepared_is_the_sole_successful_settlement` for the settlement
contract. This one asserts that the prose agrees with them; `src/export.rs` and
`agent/proc/tests.rs` pin the sentences they own the same way.

## `fn the_settled_notes_separate_the_successful_and_the_failed_settlement()` › `let settled = settled.split_whitespace().collect::<Vec<_>>().join(" ");`

Match on the prose, not on where its line breaks fall: a reflow must not break
the pin, only a changed claim.
