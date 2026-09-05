# `src/topology/fold/check_attempt.rs`

Extended notes for [`src/topology/fold/check_attempt.rs`](../../../../src/topology/fold/check_attempt.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The attempt lifecycle checks: resume, spawn, dispatch, and a generation
from its first attempt to its close.

## `impl RunState` › `pub(super) fn check_run_resumed(&self, resumed: &RunResumed4) -> Result<(), FoldError> {`

--- run_resumed -------------------------------------------------------

## `pub(super) fn check_run_resumed(&self, resumed: &RunResumed…` › `if let Some(field) = self.started.runner.difference(&resumed.runner) {`

refusals[5], second half: exact equality, field for field (INV-23).

## `impl RunState` › `pub(super) fn check_spawn(`

--- task_spawned ------------------------------------------------------

## `impl RunState` › `if spawn.key.index() != self.registry.len() {`

refusals[10]: a dynamic task's key is the registry's length at the
event that registers it.

## `impl RunState` › `if entry.allowed_agents != self.started.probed_agents {`

The allow-list is the run's, not the registering event's: an entry
that widened it would admit an agent pre-flight never probed.

## `impl RunState` › `if entry.deps.len() != entry.display_deps.len() {`

Dependencies: every one exists, refers backwards, and the display
list is the same list.

## `impl RunState` › `if self.task(kind, *dep)?.state != TaskState::Merged {`

A repair rebases work that was already integrated; a dependency
that is not merged has nothing for it to build on.

## `impl RunState` › `pub(super) fn check_admission<F>(`

The registered entry's admission and the event's must be the same
statement about the same task.

## `impl RunState` › `pub(super) fn check_dispatched(&self, dispatched: &TaskDispatched) -> Result<(), FoldErro…`

--- task_dispatched ---------------------------------------------------

## `pub(super) fn check_dispatched(&self, dispatched: &TaskDisp…` › `if usize::try_from(dispatched.generation.0).unwrap_or(usize::MAX) != task.generations.len…`

refusals[10]: generations are dense per task.

## `pub(super) fn check_dispatched(&self, dispatched: &TaskDisp…` › `(LeaseGrant::Predicted { paths }, None) => {`

**The recorded region is derivation-checked, exactly as the
recorded binding is.** One event over, `check_attempt_started`
refuses a binding the fold did not derive
(`FoldError::BindingMismatch`); this arm used to match the
lease's *shape* alone and let `apply_dispatched` grant whatever
region the event carried — so the fold could admit a dispatch on
one region while the lease table held another, and the lease
table's is the one every later overlap check consults.

That was not hypothetical. A driver that took the plan's hints
literally recorded `src/auth/*.rs` as a **prefix**, which
overlaps nothing, while the fold had admitted the dispatch on
`src/auth`. `84a3978` made the driver read
[`TopologyFold::predicted_region`] instead of deriving its own,
which fixed that instance; nothing stopped the next caller — or
a later slice's second writer — from constructing a
`task_dispatched` the fold would accept and the lease table would
honour. This is the class fix, and it is why the reader and this
validator call the **same** free function rather than two copies
of one rule.

**Exact equality, and deliberately not a policy-aware one.** The
run's frozen `PathPolicy` decides whether two regions *overlap*,
case-folding component by component; it does not decide whether
two regions are the same region. A recorded `SRC/Auth` that folds
onto a derived `src/auth` is still a different literal, and the
lease table stores literals — so an equality that folded here
would admit a component set the derivation never produced and
hand it to `apply_dispatched` unchanged. Order counts for the
same reason: the derivation emits one prefix per hint in the
frozen order, so a reordered list is a list this run's frozen
hints do not derive.

Live at the first width above `max_parallel = 1`, where two tasks
holding non-overlapping-by-construction regions edit the same
files; invisible below it.

## `impl RunState` › `pub(super) fn check_attempt_started(&self, started: &AttemptStarted4) -> Result<(), FoldE…`

--- attempt_started ---------------------------------------------------

## `pub(super) fn check_attempt_started(&self, started: &Attemp…` › `match (&generation.class, &started.resume_session) {`

ST-06: a retry names the generation it is retrying, and a fresh
attempt names one nothing has run in yet.

## `pub(super) fn check_attempt_started(&self, started: &Attemp…` › `if session != resumed {`

refusals[12]: a session belongs to the incarnation that
retained it, and only that incarnation may resume it.

## `pub(super) fn check_attempt_started(&self, started: &Attemp…` › `if started.attempt.0 != generation.attempts + 1 {`

ST-06: attempts are dense from 1 within a generation.

## `pub(super) fn check_attempt_started(&self, started: &Attemp…` › `let mismatch = |detail: String| FoldError::BindingMismatch {`

refusals[11] / INV-19: the binding is the override when one was
recorded, and the frozen rung binding otherwise.

## `impl RunState` › `pub(super) fn open_generation<'a>(`

The open generation this event must be naming (ST-06).

## `impl RunState` › `pub(super) fn in_flight<'a>(`

The open generation, additionally required to be running `attempt`.

## `impl RunState` › `pub(super) fn check_attempt_finished(`

--- attempt_finished --------------------------------------------------

## `impl RunState` › `if finished.record.attempt != finished.attempt.0 {`

**The envelope and the record name one attempt, on this arm
too.** This arm checked the epoch and stopped, so a
current-epoch retained settlement could carry a ledger line
belonging to a different attempt of the same generation —
the same disagreement the `Closed` arm has refused since
round 6, one arm over. Every one of that round's four new
refusal witnesses constructs `Closed`, which is why this arm
was undriven; the `cfa1be8` review found it as its second P1.

**A door is not fixed until every arm through it asks the
same question.**

## `impl RunState` › `if finished.record.is_successful() {`

**And the record does not claim the attempt succeeded.**
`candidate_prepared` is the sole successful settlement
(INV-07,
`decisions/2026-08-12-merge-queue-execution-topology.md`),
and the `Closed` arm has enforced that against the record
since round 6. This arm did not, so the invariant held on one
path through the door and not the other: a current-epoch
retained settlement could carry a record with no failure and
every configured pass green — a record
`check_candidate_prepared` would itself accept — while the
fold held the generation open for a retry. The ledger line an
operator reads would say the work passed.

**This is not a terminal-failure requirement, and the
difference is the whole of the earlier hesitation.**
`settle::settle_failed` is the only producer of a `Retained`
settlement and it is reached on the failure path, for a
same-rung retry that has a session to resume — so a retained
attempt has not succeeded, by construction. Asking
`!is_successful()` is the record saying that much and no
more: `Retained` carries no transition, so nothing here makes
the generation terminal, and the arm goes on to leave it open
with its lease held.

One predicate, both arms, as the candidate door and the
closed settlement already share it — a door is not fixed
until every arm through it asks the same question.

## `impl RunState` › `if finished.record.session_id.as_deref() != Some(retained_session.0.as_str()) {`

**And the record names the conversation the settlement
keeps.** A `Retained` settlement exists to hold a session for
a same-session retry, and `check_attempt_started` will make
the retry name the *generation's* session — the one this
event puts there. If the ledger line names another session,
or none, then the two halves of one event disagree about
which conversation was left open, and the half a person reads
is not the half the fold enforces.

## `impl RunState` › `if matches!(transition, SettlementTransition::Succeeded) {`

**`attempt_finished` does not settle a success.** INV-07 and
`decisions/2026-08-12-merge-queue-execution-topology.md` say
it outright — `candidate_prepared` is "the **sole**
successful settlement for an attempt that produces a
candidate … `attempt_finished` is not also emitted for that
attempt" — and this build appended both, so one attempt
carried its record on two lines.

Refused here rather than tolerated downstream. The 2026-08-27
ruling is CONFORM, not supersession, and a reader that
*coped* with the dual pattern would be a second reading of
the same sentence: `Spend::replay` grew per-attempt
deduplication to survive it, which is evidence of the
duplicate rather than permission for it. Schema 4 has no
external writers — `src/engine/mod.rs` is `pub(crate) mod
topology` — so no log this build did not write can carry the
shape, and refusing it costs no compatibility.

## `impl RunState` › `if finished.record.is_successful() {`

**The record must say the attempt failed, and must be this
attempt's.** This door refused `Succeeded` and asked nothing
else, so a settlement could fail a task and halt a run while
carrying a record whose failure field is empty and whose
reviews all passed — a ledger line reporting success attached
to a terminal failure. `AttemptRecord::is_successful` is the
one definition, shared with `check_candidate_prepared`, so
the two doors cannot drift apart again.

## `impl RunState` › `if finished.record.attempt != finished.attempt.0 {`

The envelope and the record name one attempt. Without this the
ledger line a settlement carries can belong to a different
attempt of the same generation.

## `impl RunState` › `pub(super) fn check_attempt_interrupted(`

--- attempt_interrupted -----------------------------------------------

## `impl RunState` › `check_lease_disposition(KIND, interrupted.key, generation.lease, interrupted.lease)`

The generation does *not* survive an interruption.
`transaction_fault_matrix[T-ATTEMPT].resume_action` is explicit:
"append attempt_interrupted (unknown spend, allowance refunded,
generation Closed, lease by kind) ... task returns Pending; later
dispatch new generation". Nothing was judged and the spend is
unknown, so the worktree is scrubbed with force rather than reused —
which is why an ordinary generation releases its predicted region
here and a lineage member goes on holding its root's.

## `impl RunState` › `pub(super) fn check_generation_closed(`

--- generation_closed -------------------------------------------------

## `impl RunState` › `match generation.class {`

refusals[15]: an open generation with no attempt in flight. A
promoting generation is not closed — it is promoted.

## `impl RunState` › `pub(super) fn check_defer_wait_elapsed(&self) -> Result<(), FoldError> {`

--- defer_wait_elapsed ------------------------------------------------

## `pub(super) fn check_defer_wait_elapsed(&self) -> Result<(),…` › `if self.halted_at.is_some() {`

refusals[18]: halt and budget outrank backoff, so no wait elapses
under either.
