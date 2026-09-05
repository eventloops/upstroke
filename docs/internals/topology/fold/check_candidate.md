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

## `impl RunState` › `let prepared = match &generation.candidate {`

ST-06: a mismatched task_candidate_created.
