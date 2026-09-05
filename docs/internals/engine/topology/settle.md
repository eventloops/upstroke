# `src/engine/topology/settle.rs`

Repository source for these notes: [`src/engine/topology/settle.rs`](../../../../src/engine/topology/settle.rs).
[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/engine/topology/settle.rs).
The relative link works in a checkout or on GitHub; the GitHub link also works from the published site.

The code is the authority for what it does. The explanatory prose is preserved below.
Each backticked part of a section heading is an exact source excerpt. Search for the final
excerpt within the preceding item when a heading names both an item and a line inside it.

## Module

Settlements: what one finished attempt records, and what happens next.

`T-FAILED`'s durable state is "transition/parking/defer/generation/
lease-disposition/allowance decision", and its `resume_action` ends
"**never re-decide**". Those two sentences are the whole shape of this
module: every consequence of a settlement is decided **once**, at the
settlement, and written into the event; every later reader — the retry, the
resume, the closure — reads the decision back rather than making it again.

That is why [`rematerialize_question`] takes an `attempt_finished` and
returns the question it *recorded*, rather than re-deriving a question from
the failure. A resume that re-derived one would produce a second question
id for the same park, and the answer the operator already wrote would
answer neither.

### The three rows

* `T-FAILED` — [`settle_failed`], [`rematerialize_question`], [`Deferral`].
* `T-RETAINED` — [`close_generation`] and [`close_retained`]: a retained
  generation is closed by a fresh process at recovery, by a failed
  verification, and at run end, and by nothing else.
* `T-RETRY` — [`retry`]: ceiling, provisional `{pipeline}` reservation,
  `Worktree.Verify`, then `attempt_started(retry)`.

### `retained_incarnation` is an `Epoch`

`AttemptSettlement::Retained.retained_incarnation` is typed
[`Epoch`](crate::topology::events::Epoch) and is compared against the
fold's epoch — `state.resumes`, the number of times this run has resumed.
`RunStarted4::incarnation` and `RunResumed4::incarnation` are
[`IncarnationId`](crate::topology::events::IncarnationId), a per-process
ULID that identifies a coordinator process and is never compared to an
epoch. The same English word names two different types; this module wires
the field from [`TopologyFold::epoch`] and never from an `IncarnationId`.

## `pub struct FinishedAttempt {`

---------------------------------------------------------------------------
T-FAILED — the settlement
---------------------------------------------------------------------------

## `pub struct FinishedAttempt {`

One finished attempt, as the caller observed it.

The ladder decides `next`; the attempt decides `record` and `session`; the
run's `run_end_policy` decides `halts_run`. This type is what those three
answers look like arriving together, and [`settle_failed`] is the one place
they are turned into a durable decision.

## `pub struct FinishedAttempt` › `pub key: TaskKey,`

The task.

## `pub struct FinishedAttempt` › `pub generation: GenerationId,`

Its open, in-flight generation.

## `pub struct FinishedAttempt` › `pub attempt: AttemptNumber,`

The attempt that ran.

## `pub struct FinishedAttempt` › `pub record: AttemptRecord,`

The ledger line.

## `pub struct FinishedAttempt` › `pub next: Next,`

What the ladder decided.

## `pub struct FinishedAttempt` › `pub session: Option<SessionId>,`

The session the worker returned, when the adapter can resume it.

`Next::RetrySameRung { resume: true }` is the ladder's *permission* to
resume; this is whether there is anything to resume. Both are required
for a `Retained` settlement, and the ladder's own `resumable` already
folds in "the attempt actually returned a session id" — carried
separately here because the settlement records the id, not the flag.

## `pub struct FinishedAttempt` › `pub question: Option<FrozenQuestion>,`

The question a park raises, frozen by whoever built it.

`Next::AskHuman` names a [`crate::ir::QuestionKind`] and nothing else;
the id, context and options are the caller's. A settlement that
invented them here would be deciding, at the settlement, something the
resume would then have to re-decide identically.

## `pub struct FinishedAttempt` › `pub halts_run: bool,`

Whether this task's terminal failure halts the run
(`decisions.run_end_policy`).

## `pub struct FinishedAttempt` › `pub defers: u32,`

How many times this task has already deferred, before this settlement.

## `pub struct FinishedAttempt` › `pub reason: String,`

Why it failed, for the durable reason string.

## `pub struct FinishedAttempt` › `pub rung: u32,`

The rung an escalation climbs onto.

## `pub struct Settled {`

What one settlement decided.

## `pub struct Settled` › `pub event: AttemptFinished4,`

The event to append.

## `pub struct Settled` › `pub spent_attempt: bool,`

The **allowance decision**: whether this settlement spent one of the
rung's `attempts_per`.

**Thirteen `FailureShape`s spend nothing, spanning seven `FailureKind`s.**
`ladder::spends_allowance` is the single authority and it takes a
[`crate::ladder::FailureShape`], which **is** a `(kind, origin)` pair. It
answers `false` for four kinds outright — **`NeedsHuman`, `NoChain`,
`Interrupted`, `Declined`** — and, before the match runs at all, for
anything `FailureShape::is_outage` accepts: **`RateLimited`** at any
origin, **`ReviewUnavailable`** at any origin, and **`Timeout` with
`FailureOrigin::Reviewer`**.

Six of those seven kinds contribute two shapes each and `Timeout`
contributes one, which is 13 — and `Timeout` is the single kind whose
answer depends on the origin at all.

The rule they share is the one §2's `PR3-ATTEMPT-SHAPE` ruling states:
*an attempt spends one of its rung's `attempts_per` iff the worker ran
and produced work to judge.* A declined task, an exhausted chain, an
interruption, a reviewer asking for a person, and a pool or a reviewer
that was simply not there are each a case where nothing was judged.

**Fourth statement of this sentence, and the first that names the thing it
counts.** It first said an outage deferral spends none "and every other
settlement spends one" — off by six. Round 6 corrected it to "five kinds",
which reads the outage arm as one kind when it covers three. The
`c2c0294` review corrected that to "seven shapes … not a `FailureKind`
count" — and *seven* is the kind count: there are 13 shapes. The
`bf927f3` review found that one, and the test beneath it was computing
the kind count while the doc quoted a shape count, so the two disagreed
with each other as well as with the authority. **Note the reader's trap it comes from**: the
match's last arm lists `Timeout | RateLimited | … | ReviewUnavailable =>
true`, and all three of those are unreachable there for the origins the
outage guard already took. Reading the arm alone gives the wrong
answer, which is how this sentence has been wrong twice.

**The number is no longer prose, and the enumeration is no longer a list
somebody keeps up to date.**
`ladder::tests::exactly_thirteen_failure_shapes_spend_no_allowance` reads the
variants out of the enum's own source, maps each through an exhaustive
`match`, and counts the shapes the authority exempts — so a new
`FailureKind` stops the crate building until it has a value here, and a
restatement that disagrees with the authority is a failing test rather
than a sentence nobody re-reads.

**And the fold derives it when applying `attempt_finished`, not
`attempt_started`** — `TopologyFold::apply_settlement` calls
`spends_allowance` on the settled record. Counting at the settlement is
what makes `T-ATTEMPT`'s refund the *absence of a charge* rather than a
correction, which is the whole contrast with the legacy tracker's seven
write sites.

It is returned rather than carried on the event because it is the input
the *next* ladder decision reads, and deriving it twice from two
different places is how the two disagree. Frontier review of `75da796`,
finding 5.

## `pub fn settle_failed(`

The settlement one finished attempt records.

Decides, once: the transition, the parking, the deferral count, whether the
generation survives, the lease disposition, and the allowance. The lease
disposition is **read from the fold** — `GenerationLease::expected` is the
whole of the rule and `check_lease_disposition` refuses any other answer —
rather than restated here, so a repair's `LineageHeld` and an ordinary
generation's `PredictedReleased` cannot drift apart.

### Errors

[`UpstrokeError::Refused`] when the fold holds no such open generation,
when a park carries no question, or when the run has not started.

## `if let Next::RetrySameRung { resume: true } = finished.next {`

`Retained` is the one settlement that leaves the generation open, and it
needs both halves: the ladder's permission to resume, and a session to
resume. Either alone closes the generation and retries from a fresh one.

## `retained_incarnation: epoch,`

The trap this module's header names: an `Epoch`,
taken from the fold, and never an `IncarnationId`.

## `let lease = generation.lease.expected(false);`

Every one of these closes the generation, so `survives` is `false`.

**And there is no success here at all.** This said the surviving case was
settle_succeeded's — a function this ruling deleted — "a separate
function because it is appended at a
different point of the sequence". That function is gone: since the
2026-08-27 CONFORM ruling `candidate_prepared` is the sole successful
settlement, and the region a surviving generation hands to its candidate
is decided by `CandidateLeaseEffect` on that event, which
`check_candidate_prepared` matches against the entry's lineage.

## `fn spent(finished: &FinishedAttempt) -> bool {`

The allowance decision, from the one authority.

`ladder::spends_allowance` is total over `FailureKind` and is the function
whose four-cell grid was measured against the legacy park paths. This module
used to answer the same question itself, as `!matches!(finished.next,
Next::Defer)` — derived from the ladder's *decision* rather than from the
failure the decision was made about.

The two disagree, and on the cell that matters most. `Next::AskHuman` from a
`NeedsHuman` failure is not a `Defer`, so the old form said the attempt
spent one; `spends_allowance` says it does not, because "the code was never
judged, so nothing is spent and nothing escalates" — `next_step`'s own words
about that exact branch. An operator would have lost a rung's attempt to a
worker that asked a question instead of working.

A record with no failure is a settlement of work that was judged and
accepted, which spends. That is `spends_allowance`'s `None` arm and not a
second rule here.

## `pub fn rematerialize_question(finished: &AttemptFinished4) -> Option<&FrozenQuestion> {`

**There is no successful settlement to build here, and that is the point.**

settle_succeeded used to live at this spot, and no longer exists: it built an
`attempt_finished{Succeeded}` that the driver appended between the pin and
`candidate_prepared`, and its doc argued that INV-07's *"candidate_prepared
is the sole successful attempt settlement"* was "about which event records
the candidate, not about which event settles the attempt".

That reading was wrong, and
`decisions/2026-08-12-merge-queue-execution-topology.md` had already
answered it in the same breath as the sentence it reinterpreted:
`candidate_prepared` "contains exactly one complete attempt record … ;
**`attempt_finished` is not also emitted for that attempt**". Ruled CONFORM
on 2026-08-27 — the record stands and the code changed.

The settlement now belongs to `candidate_prepared`:
[`crate::topology::fold::TopologyFold`] promotes the generation when it
applies that event, refuses an `attempt_finished` that settles `succeeded`
at all, and refuses a `candidate_prepared` whose generation is already
promoted — so neither ordering of the old pair can be written. The per-instance
Class B approval is `reviews/FINDINGS.md` §3.

## `pub fn rematerialize_question(finished: &AttemptFinished4) -> Option<&FrozenQuestion> {`

The question a settlement raised, read back from the event.

`T-FAILED.resume_action`: "rematerialize question from the event … never
re-decide". A process that died between the settlement's append and the
question payload's write comes back, replays, and finds the question here —
the same id, context and options the settlement froze, because they are the
bytes it appended.

## `pub struct Deferral {`

---------------------------------------------------------------------------
T-FAILED — the defer branch
---------------------------------------------------------------------------

## `pub struct Deferral {`

The backoff branch of `sequential_substrate.loop`.

"sleep the defer backoff and append `defer_wait_elapsed`" — in that order,
and only when neither `halted_at` nor the epoch's `budget_stop` is set,
which is `refusals[18]` and is why the selector never offers this branch
under either. A Deferred task therefore **never blocks a Halted or
BudgetExceeded closure**: it is not that the closure outranks the wait, it
is that no wait is entered at all.

The sleeper is [`crate::interaction::Sleeper`] and the schedule is
[`crate::interaction::defer_backoff`], both of which already exist and are
already threaded. A second sleep abstraction here would be the duplication
this design exists to avoid.

## `pub struct Deferral` › `base: Duration,`

The first wait, doubled per round and capped.

## `pub struct Deferral` › `round: u32,`

Consecutive waits where deferred work was the only runnable work.

## `impl Deferral` › `pub const fn new(base: Duration) -> Self {`

A backoff starting from `base`.

## `impl Deferral` › `pub const fn default_backoff() -> Self {`

The default backoff.

## `impl Deferral` › `pub fn wait(&mut self, sleeper: &dyn Sleeper) -> DeferWaitElapsed4 {`

Sleep this round's wait and return the event that records it.

The sleep happens first because the event is the *record of a wait that
elapsed*: appending it before sleeping would put a claim in the log
that a kill during the sleep would make false.

## `impl Deferral` › `pub fn progressed(&mut self) {`

Reset the doubling: work other than a wait made progress.

## `impl Deferral` › `pub const fn round(&self) -> u32 {`

Which wait this is.

## `pub fn close_generation(`

---------------------------------------------------------------------------
T-RETAINED — closing a generation
---------------------------------------------------------------------------

## `pub fn close_generation(`

`generation_closed` for one open generation.

The lease disposition is the fold's, exactly as in [`settle_failed`], and
`check_generation_closed` refuses anything else. The reason is the caller's
because the three reasons are three different callers: recovery step (e)
closes a retained session it did not retain, a failed `Worktree.Verify`
closes a generation whose tree is gone, and run-end closure closes
everything still open.

### Errors

[`UpstrokeError::Refused`] when `key` has no open generation, or when the
open one is in flight or promoting — `refusals[15]`: "a generation is
closed only from open-with-no-attempt or retained-idle".

## `pub fn close_retained(`

Every `RetainedIdle` generation this run holds, closed for `reason`.

`T-RETAINED.resume_action` names three occasions and this is all of them
except the per-generation one: recovery step (e) closes every retained
generation with `ResumeDiscardsRetainedSession` **before** `run_resumed`,
and run-end closure closes them with `RunEnding`. The order is ascending
key, so a replay and a live run append the same bytes in the same order.

### Errors

As [`close_generation`].

## `pub const fn run_ending(outcome: RunOutcome) -> GenerationCloseReason {`

The run-end reason for `outcome`.

Present so a caller cannot reach for `ResumeDiscardsRetainedSession` at run
end: the two are different claims about why a session was discarded, and
only the recovery one is true of a fresh process.

## `fn retained_keys(fold: &TopologyFold) -> Vec<TaskKey> {`

Every task holding a `RetainedIdle` generation, ascending.

## `pub trait WorktreeVerify {`

---------------------------------------------------------------------------
T-RETRY
---------------------------------------------------------------------------

## `pub trait WorktreeVerify {`

The `Worktree.Verify` a retry performs before it appends.

A seam, and the reason is a rule rather than a preference:
`src/engine/topology/**` may name neither `std::process::Command` nor raw
`std::fs`, in production **or in tests**, so a test in this module cannot
build the repository a real [`WorkspaceManager`] is derived from. The
production implementation is [`ManagedWorktrees`], which is one call over
`WorkspaceManager::verify_worktree` — so the funnel, its containment
checks, and the `Worktree.Verify` effect site stay exactly where they are
and this trait adds no second path to them.

## `pub trait WorktreeVerify` › `fn verify(`

`Worktree.Verify` over `slot`.

A worktree that is not quiescent is `Ok(Err(VerifyFailure))`, not an
error: that failure routes to a closure, which is a decision this
module makes.

### Errors

A containment refusal or a Git error.

## `pub struct ManagedWorktrees<'a>(&'a WorkspaceManager);`

Production: the run's own [`WorkspaceManager`].

## `impl<'a> ManagedWorktrees<'a>` › `pub const fn new(manager: &'a WorkspaceManager) -> Self {`

Verify through `manager`.

## `pub enum RetryOutcome {`

What the retaining incarnation does next.

## `pub enum RetryOutcome` › `Start(Box<AttemptStarted4>),`

The worktree verified. Append this, and convert the reservation at the
append.

## `pub enum RetryOutcome` › `Close {`

The worktree is missing, foreign, or carries administrative residue
from an interrupted command. The generation closes and the reservation
was cancelled.

## `pub enum RetryOutcome` › `closed: GenerationClosed,`

The `generation_closed{WorktreeMissing}` to append.

## `pub enum RetryOutcome` › `failure: VerifyFailure,`

What `Worktree.Verify` observed, for the operator-facing detail.

## `pub struct RetryRequest {`

What a retry needs beyond the fold.

## `pub struct RetryRequest` › `pub key: TaskKey,`

The task.

## `pub struct RetryRequest` › `pub slot: Slot,`

The slot holding the retained cumulative tree.

## `pub struct RetryRequest` › `pub retained_tree: String,`

The tree the retained worktree must still hold.

`Quiescence::HoldsTree`, not `AtBase`: a retained generation's whole
point is that the worktree carries the previous attempt's cumulative
work, so HEAD is still the base and the *index* is what differs. A
retry verified against the base would pass on a worktree that had been
reset and would then re-gate an empty tree as if it were the retained
one.

## `pub struct RetryRequest` › `pub binding: RungBinding,`

The frozen rung binding this attempt runs, or the validated override.

## `pub struct RetryRequest` › `pub rung: u32,`

Which rung of the frozen ladder that is.

## `pub struct RetryRequest` › `pub pool: Option<String>,`

The capacity pool the attempt draws on, where its agent names one.

## `pub struct RetryRequest` › `pub materialization: Option<Materialization>,`

What a repair's worktree was materialized from. `Some` for a repair,
`None` otherwise — `check_attempt_started` refuses the other two pairs.

## `pub fn retry(`

`T-RETRY`: the retaining incarnation takes its next attempt in place.

The order is the packet's, and every step of it is load-bearing:

1. **The ceiling** — checked by the selector, which is why this function is
   reached only through [`super::select::Admitted::Retry`].
2. **The provisional `{pipeline}` reservation**, taken before the verify
   because `permits.provisional_reservations` calls it "the bridge between
   a selection decision and its first append" and the verify is already
   past the decision.
3. **`Worktree.Verify`**, present, quiescent, holding the retained tree.
4. **`attempt_started(retry)`**, which the caller appends and at which it
   converts the reservation.

A failure at 3 cancels the reservation and closes the generation: a
retained worktree is **never recreated** (INV-06), because what it held was
a cumulative tree no base can be re-cut into.

A fresh process never reaches here. Recovery step (e) closes every retained
generation before `run_resumed`, and `ready_retry` requires
`retained_incarnation == state.resumes` — so after a resume the fold's
epoch has moved and `check_attempt_started` refuses the stale incarnation
even if a caller reached past the selector.

### Errors

A reservation the ledger refuses, a Git or containment error from the
verify, or [`UpstrokeError::Refused`] when the fold holds no retained
generation for `key`. Every error path cancels the reservation it took.

## `reservations.cancel(request.key, ReservationKind::Retry)?;`

"cancellation on any pre-append failure".

## `fn retained(`

The retained generation of `key`, its session, and the attempt a retry
starts.

The incarnation equality is the fold's — `ready_retry` is where it is
enforced for selection and `check_attempt_started` is where it is enforced
for the append. It is restated here as a *refusal* rather than a filter so
that a caller which reached past the selector is told why, rather than
being handed an event the fold will refuse three lines later.

## `fn open_generation(`

The open generation this settlement names, refusing anything else.

## `fn class_name(class: &GenerationClass) -> &'static str {`

How a refusal names a generation class.

`GenerationClass::name` is private to the fold, so this is a second
spelling of the same five words. It is a diagnostic string and nothing
reads it back — the one thing a duplicated *name* cannot get wrong is a
decision, because no decision is made from it.
