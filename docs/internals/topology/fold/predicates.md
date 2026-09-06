# `src/topology/fold/predicates.rs`

Extended notes for [`src/topology/fold/predicates.rs`](../../../../src/topology/fold/predicates.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

What a caller may ask the fold, and nothing it may ask twice.

The readers of [`TopologyFold`] and the selection predicates the run loop
asks instead of re-deriving admission for itself.

## `impl TopologyFold` › `pub fn poison(&mut self) {`

Mark this process's fold unusable after an append whose outcome is
unknown.

Not a state transition and not reversible. The command has already
ended; what remains is to refuse everything that would derive an effect
from a state this process can no longer vouch for.

## `impl TopologyFold` › `pub fn state(&self) -> Option<&RunState> {`

This run's folded state, or `None` before its `run_started`.

The value two folds are compared as: a live fold and a replay of the
bytes it appended hold the same `RunState` or INV-02 does not hold.

## `impl TopologyFold` › `pub fn ready(&self, key: TaskKey) -> bool {`

-----------------------------------------------------------------------
Selection predicates

`decisions.admission_and_leases` defines `ready` and `ready_retry` as
"structural over fold state only", and INV-22 makes entitlements
fold-enforced. The loop that drives a run therefore has to ask the fold
these questions rather than answer them itself: a second implementation
of "which generation classes hold the pipeline entitlement" is two rules
that can disagree, and `wrong_internal_assumption` is the largest
measured root cause in this project by a factor of three.

What stays with the caller is the packet's actual division of labour:
the loop decides *which* eligible item to take and checks the budget
ceiling (`sequential_substrate.loop`; a breach appends `budget_exceeded`
before any effect), and the fold decides *whether* an item is
structurally eligible. These accessors are that second half and nothing
more — each delegates to the private predicate it names and adds no
logic of its own.

An unstarted run offers no work: readiness predicates return false,
and the continuation reader returns no generation. Such a run holds no
entitlement either.

**A poisoned fold authorises nothing.** `plan_transition` refuses with
`FoldError::Poisoned` once an append has returned an error, and INV-20
says "no completion is applied after the fold is poisoned by a returned
append error". A predicate that kept answering `true` would let the
coordinator select work from a state this process can no longer vouch
for, and the append-error protocol's "no report, cleanup, or question
payload is derived from the poisoned fold" would hold in the emit path
and leak here. So every predicate below is false once poisoned.

The exceptions are the four that state what the run *is* rather than
what it may do: `pipeline_held`, `run_is_ending`, `backoff_pending` and
`questions_open`. They are accounting, not authorisation. A poisoned
fold whose `halted_at` is set is still a halted run, and answering `0`
or `false` there would be a false statement about durable state rather
than a refusal. Their callers must not derive a report from them after a
poisoned append either, but that is a rule about reports and it lives in
the emit path — and nothing selects on them from a poisoned fold in any
case, because selection refuses at the top on `is_poisoned`.
-----------------------------------------------------------------------

## `impl TopologyFold` › `pub fn ready(&self, key: TaskKey) -> bool {`

Whether `key` may be dispatched into a fresh generation.

`decisions.admission_and_leases.ready`.

## `impl TopologyFold` › `pub fn ready_retry(&self, key: TaskKey) -> bool {`

Whether `key` may take its next attempt in the generation it retained.

`decisions.admission_and_leases.ready_retry`. False in any incarnation
but the retaining one — `retained_incarnation == state.resumes` is part
of the predicate, which is why a caller must not re-derive it.

## `impl TopologyFold` › `pub fn pipeline_held(&self) -> usize {`

The pipeline entitlement currently held, derived from the fold.

Generations in `OpenNoAttempt`, `InFlight` and `Promoting` hold one
each; `RetainedIdle` and `Closed` hold none; an unresolved integration
transaction holds one. `decisions.admission_and_leases.permits.pipeline`.

## `impl TopologyFold` › `pub fn pipeline_reservable(&self) -> bool {`

Whether a further pipeline entitlement is within `max_parallel`.

## `impl TopologyFold` › `pub fn structurally_admissible(&self) -> bool {`

Whether some task could be dispatched, retried, or integrated from this
state alone.

Budget, capacity and runner availability are not consulted — this is
what "structurally admissible" means, and it is the predicate the
ceiling check is applied *to*, not a substitute for it.

## `impl TopologyFold` › `pub fn integration_admissible(&self) -> bool {`

Whether an integration transaction could start from this state.

## `impl TopologyFold` › `pub fn run_is_ending(&self) -> bool {`

Whether this run has already ended in the sense that forbids further
work: `halted_at` is set, or a `budget_stop` of **this** epoch is.

The epoch is the load-bearing half. A `budget_stop` recorded in an
earlier incarnation was cleared by the resume that raised the ceiling,
and a caller that read the field without the epoch would refuse a run
the operator has already unblocked. It is exposed for the same reason
`ready` is: `refusals[18]` refuses `defer_wait_elapsed` under either
condition, so a selector that decided the backoff branch from its own
copy of this rule would offer the loop an append the fold is about to
refuse — and the two copies would be free to disagree.

## `impl TopologyFold` › `pub fn backoff_pending(&self) -> bool {`

Whether anything is waiting on a wait: a task in
[`TaskState::Deferred`], a task whose execution backoff is hidden by
an open question, or a queue entry whose verification was deferred.

This is the *pending work* half of the backoff branch and not the
branch itself — [`Self::run_is_ending`] is the other half, and
`derived_outcome` consults this one alone. Both halves are here so that
neither is re-derived: the fold walks its own tasks, and a caller
walking the registry's keys instead is walking a different sequence the
moment a repair is registered.

## `impl TopologyFold` › `pub fn frozen_rung_binding(&self, key: TaskKey, rung: u32) -> Option<RungBinding> {`

The binding rung `rung` of `key` is frozen as.

**The eleventh reader, and it is deliberately only half of the fold's
rule.** `check_attempt_started` accepts a binding that matches the
human override when one is recorded, and the frozen rung otherwise.
This returns the frozen rung's, and nothing else, for two reasons.

First, no override is constructible in a run this crate currently
drives: a `BindingOverride` arrives from an `Answered` event, and the
loop's answer-ingest branch is not implemented, so the override arm has
no reachable input. Second, and more important, **the fold's override
check is partial**: `matches_override` compares agent, model and effort
and says nothing about `tier` or `pinned`. A caller that built an
override binding would be choosing those two fields unchallenged, and
this reader is not the place to invent a rule the packet states
somewhere the author of this method has not read.

So a caller holding an override must not use this. The intended shape,
when the answers branch lands, is that this method grows the second arm
together with the passage that decides those two fields — not that a
caller composes one from [`Self::binding_override`] and this.

`None` when the run has no registry, the task is not registered, or the
ladder has no such rung.

## `impl TopologyFold` › `pub fn open_no_attempt(&self, key: TaskKey) -> Option<GenerationId> {`

The generation `key` has open with no attempt started, if it has one.

**`T-DISPATCH`'s "continue attempt (no spend repeats)", made askable.**
A run killed between `task_dispatched` and `attempt_started` leaves the
generation `OpenNoAttempt`; recovery step (g) verifies or recreates its
worktree, and then the loop is supposed to start the attempt in it.

[`Self::ready`] cannot answer this and must not: it requires
`task.open().is_none()`, which is correct — a task with an open
generation is not *ready to be dispatched*. The continuation is a
different question about the same task, and asking it of `ready` would
make one predicate mean two things.

A lookup over the generation [`Self::task`] holds open, deciding nothing:
the class is what `apply` recorded and the id is the generation's. It
reaches that generation through the same `TaskFold::open` the
continuation eligibility reader uses, so the two cannot name different
generations; a task whose open generation is of another class, and a
task holding no open generation at all, are both absence. Poisoning is not
consulted for the same reason the other statement accessors do not — a
poisoned fold of a run with an open generation still has one, and `None`
here would be a false statement rather than a refusal.
A lineage question likewise leaves the generation visible to recovery;
selection uses the separate continuation eligibility reader.

## `impl TopologyFold` › `pub fn predicted_region(&self, key: TaskKey) -> Option<PathSet> {`

The region an ordinary dispatch of `key` predicts.

**The tenth reader, and it exists because the alternative was a second
authority.** `dispatch_lease_check` decides whether a task is `ready` at
all by computing this region and asking the lease table what it
overlaps. A caller that then recorded a *different* region in
`task_dispatched` would have the fold admitting on one answer and the
log holding another — and the log's is the one the lease table keeps.

That is not hypothetical. It was written: a driver that took the plan's
hints literally recorded `src/auth/*.rs` as a **prefix**, which overlaps
nothing, while the fold had admitted the dispatch on `src/auth`. At
`max_parallel = 1` nothing can collide and the disagreement is
invisible; at the first width above one it is two tasks editing the same
files.

**A convention until the region became derivation-checked.** This reader
existing did not oblige anyone to read it: `check_dispatched` matched a
`task_dispatched` lease's *shape* only and `apply_dispatched` granted the
region the event carried. `check_dispatched` now refuses an ordinary
dispatch whose recorded region is not this answer, so the disagreement is
inexpressible rather than merely undocumented, and this reader is the
convenient way to obtain what the fold will accept rather than the only
way to avoid a refusal.

`None` when the run has no registry yet, which is before `run_started`.

## `impl TopologyFold` › `pub fn questions_open(&self) -> bool {`

Whether any question is open.

The ids themselves are [`Self::open_questions`]; this is the predicate
`derived_outcome` decides `Parked` with, exposed so that the hard-block
branch and the derived outcome cannot disagree about what "open" means.

## `pub(crate) fn eligible_continuation(&self, key: TaskKey) -> Option<GenerationId> {`

The open generation whose first attempt is structurally eligible.

Unlike `open_no_attempt`, this authorizes selection. It returns `None`
for an unstarted, poisoned or ending run, an absent task or generation,
a generation that already started, or a lineage with an open question.
The generation already holds its pipeline entitlement, so continuation
does not require another free slot. Event-specific binding and
materialization facts are still checked when the attempt is recorded.

## `pub(crate) fn eligible_integration_candidate(&self) -> Option<&CandidateRef> {`

The same eligible candidate that makes integration admissible. The
engine uses this borrowed result so its choice includes lineage questions.
