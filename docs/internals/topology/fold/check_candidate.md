# `src/topology/fold/check_candidate.rs`

Extended notes for [`src/topology/fold/check_candidate.rs`](../../../../src/topology/fold/check_candidate.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The candidate checks: the settlement that prepares one, and its creation.

## `impl RunState` › `pub(super) fn check_candidate_prepared(`

--- candidate_prepared ------------------------------------------------

## `impl RunState` › `if !matches!(generation.class, GenerationClass::InFlight { .. }) {`

**The generation is still in flight, because this event is what
settles it.** It used to require `Promoting`, which only an
`attempt_finished{Succeeded}` could produce — so the fold *required*
the dual pattern the 2026-08-12 record forbids. With the settlement
moved here, a `Promoting` generation means that record was appended
anyway, and the arm above already refuses it; this refuses the other
half of the same shape, so neither order can produce two settlements.

## `impl RunState` › `if generation.candidate.is_some() {`

INV-06: "at most one candidate per generation", enforced_by "fold
refuses a second candidate for a generation". Refused here, before
any lease or candidate-state mutation could be planned: a second
record would replace the first and hand a later
`task_candidate_created` a candidate the queue never saw prepared.

## `impl RunState` › `if !prepared.attempt.is_successful() {`

**And the attempt it names must have succeeded.** This event is the
sole successful settlement for a candidate-producing attempt, so a
record carrying a failure is a settlement contradicting itself: the
candidate's own authoritative evidence would say a gate failed while
the fold promoted the generation and carried it to
`task_candidate_created`, queueing it as a success.

Missing until 2026-08-27. The Class B change made this the successful
settlement and did not make the fold require success — the semantic
condition that motivated the change was the one condition not
enforced, and the round-4 review of `09f9a99` walked the five steps.
It also gives `TopologyRun`'s `Brief::replay` the property it already
assumed: a `candidate_prepared` record never carries feedback,
because it never carries a failure.

`InconsistentRecord` rather than a new variant: the refusal inventory
is packet-enumerated, and "the event disagrees with the record it
cites" is exactly this kind.

## `impl RunState` › `let obliged: Vec<&str> = entry`

**And it must have run the passes the run froze for this task.**
`is_successful` above asks `all` over *the passes the record happens
to carry*, which is a predicate the record's own author chooses the
domain of: a `candidate_prepared` carrying a lone passed
`second-opinion` — or an empty list — satisfies it, and the fold
charges the rung, enters `Promoting`, and permits
`task_candidate_created` for a tree the configured primary reviewer
never read. Round 6 of the `cfa1be8` review found it as its first P1;
that round fixed the *outcome* half — a pass recorded `Failed` or
`Unavailable` is refused — and this is the *presence* half.

**Fold-side, and taking `(record, frozen)`.** The predicate needs the
plan and `AttemptRecord` does not carry it, so it cannot be a method
on the record; the entry is already in hand here for the lease and
lineage relations below.

The comparison is the ordered list of pass names, so it refuses in
one place every way a record can disagree with its obligation: a
configured pass omitted, a pass duplicated, a pass nobody configured,
and the configured passes in another order. §11.3's own reason for
the order is that "a later pass only exists because every earlier one
approved" — a record whose second opinion precedes its acceptance
pass describes a review that did not happen.

`FrozenReviews::obliged_lenses` is `review::passes_for`'s answer
rather than a second reading of §11.2/§11.3, and it is the same
reader the plan assembler dispatches from. That is the whole of why
this is safe to enforce: the obligation the fold requires and the
passes the driver runs are one derivation.

## `impl RunState` › `if prepared.attempt.attempt != generation.attempts {`

ST-06: a candidate is prepared *by the attempt that succeeded*, so
the embedded record names the generation's current attempt. Without
this the record is inert data and a candidate can be published
attributed to an attempt that did not produce it.

## `impl RunState` › `if !prepared.parent_is_base() {`

INV-09 depends on this: the exact-base decision compares the
integration head against `base_sha` and then publishes `commit_sha`,
so a commit parented anywhere else would fast-forward the integration
ref onto history nobody judged.

## `impl RunState` › `pub(super) fn check_candidate_created(`

--- task_candidate_created --------------------------------------------

## `impl RunState` › `let Some(prepared) = promoting_candidate(generation) else {`

ST-06: a mismatched task_candidate_created.

## `fn check_lease_effect(`

The relation between the record's lease effect and the lineage its
task belongs to, moved out of `check_candidate_prepared` so it can be
exercised without a `RunState`. Its four arms, their order and their
refusal text are what the inline `match` had; the only change of shape
is the parameter, which is the lineage's `root` rather than the whole
`Lineage`, because the root is all the relation reads.

The two checks inside the widening arm had no test anywhere in the
crate: at the head review pass 1 of PR #186 read, disabling either one
left the whole suite green (2,134 passed, 0 failed), which is what
that pass raised as `SWEEP-CHECK_CANDIDATE-003`. They are reachable —
a repair's `candidate_prepared` is where they bite — and both are now
pinned in this module's own test block.

The kind is the module's `CANDIDATE_PREPARED` rather than a parameter.
Passing it in was tried first and measured: with the kind an argument,
a call site handing over the wrong one survived the whole suite, so
the argument was a seam nothing held. One caller and one kind means
the constant belongs to the module, and `check_candidate_prepared`
spells the same name.

## `fn promoting_candidate(`

Which prepared candidate, if any, a generation may promote: the one it
prepared, and only while it is promoting.

**The class conjunct is redundant against the fold's own apply path,
and is kept deliberately.** `generation.candidate` is written in
exactly one place, `apply_candidate_prepared`, which sets it and
`class = Promoting` in the same statement; a generation is created
with `candidate: None`; `class` leaves `Promoting` only through
`close_generation`, and `TaskFold::open` does not return a closed
generation. `check_attempt_started` and `in_flight` both refuse a
promoting generation, so no checked event moves it to `InFlight` or
`RetainedIdle` with a candidate still attached. Every generation
`open_generation` can return carrying a candidate is therefore
promoting already.

Review pass 1 of PR #186 raised the surviving mutation here as
`SWEEP-CHECK_CANDIDATE-004`, on the reading that a closed generation
keeps its candidate and could promote it. That reading is wrong in one
step: `open_generation` refuses a closed generation before this line
is reached. The mutation survived because the conjunct is redundant,
not because a reachable path was untested — and it is redundant only
while `apply.rs` (queue row 29, still open) keeps writing the two
fields together, which is why the conjunct stays and this module's
test block pins it directly rather than through a state the fold can
reach.

## `#[cfg(test)] mod tests`

Tests for the two functions above, in this file rather than in
`src/topology/fold/tests.rs` where the rest of the fold's tests live.
Both checkers take `&RunState`, whose construction needs the
`RunStarted4`, plan, chain and registry-digest fixture that sibling
file builds; that file is queue row 39 and the sweep that added these
tests could not edit it. The two relations were extracted so they
could be reached with values instead.

Each test asserts the refusal's `kind` and its whole `detail`, not the
`FoldError` variant: all three refusals of `check_lease_effect` are
`InconsistentRecord`, so a variant-only assertion would pass on the
wrong arm.
