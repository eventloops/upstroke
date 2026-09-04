# `src/engine/topology/emit.rs`

Extended notes for [`src/engine/topology/emit.rs`](../../../../src/engine/topology/emit.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The emit path: the one place a schema-4 event is written, and what happens
when the write returns an error.

`decisions.coordinator_integration.emit` is six steps and this module is
those six steps and nothing else:

> "build event → serialize → round-trip → `plan_transition` → append the
> exact bytes through the Event funnel (written, then synced; the newline is
> the commit marker) → `apply_delta` only after the funnel returned `Ok`; a
> `FoldError` aborts before any write; an `Err` returned by the funnel after
> the append was entered runs the `append_error_protocol`."

Almost every mechanism it needs already exists and is tested:
[`TopologyLine::round_trip`] *is* the round-trip,
[`EventLog::append_topology_hooked`] *is* the funnel, and
[`establish_stable_prefix`] *is* the barrier. What is new here is the
**order** over them and the protocol that runs when the append fails —
and this project's own measurement says orderings are where its defects
live.

### The append-error protocol, and why the legacy engine is the wrong template

`Run::drain_and_report` in [`crate::engine::coordinator`] handles a returned
append error by catching the propagated `Err`, building a partial report
**from in-memory state**, writing it, and re-returning. That is correct for
schema 1..3 and is forbidden here, clause by clause
(`coordinator_integration.append_error_protocol`):

1. `apply_delta` is not run and **the in-memory fold is marked poisoned**.
   [`TopologyFold::poison`] is called explicitly, by [`protocol`], because
   the two poisonings are of two different objects: [`EventLog`] poisons its
   own *handle*, and the fold is a separate value that does not learn
   anything from that. Without the explicit call `plan_transition` keeps
   succeeding and the next emit writes a transition derived from a state
   this process cannot vouch for.
2. Provisional reservations are cancelled ([`Reservations::cancel_any`]) —
   `permits`: "cancellation on any pre-append failure, run end, shutdown, or
   a poisoned fold".
3. In-flight invocations are cancelled, and **both halves are the
   caller's**. The Runner side always was ("in-flight invocations are
   cancelled through the Runner"); the ledger side moved out of this module
   in `bcc5c2f`, which deleted `EmitState`'s `invocations` field and made
   [`AppendError`] carry the obligation to its constructor instead — see
   [`UncancelledAppend`]'s own note, which has said so since. This sentence
   still claimed the ledger side "is this module's". Frontier review of
   `75da796`, finding 5.
4. **No retry, no cleanup, and no report, status or question payload derived
   from the poisoned fold.** There is no code here that does any of them,
   which is the only way to state that clause.
5. The log is reopened through `Event.OpenLog` (torn-tail normalization) and
   the **stable-prefix barrier** is established before anything is reported;
   the command then ends naming the run id, the event kind, and whether the
   proven prefix contains the line — **present**, **absent**, or, when the
   barrier itself did not hold, **undetermined**, asserting neither. All
   three paths perform no effect.

`Event.AppendFirst` has a fourth shape on top of those three, because the
event whose outcome is unknown is the run's own commitment boundary: "for
`Event.AppendFirst` the creator additionally never deletes either half (the
commit record already exists) and reports the run as committed, as a
retained possibly committed husk, or as undetermined and retained". That is
[`FirstAppendDisposition`], derived from the outcome rather than stored
beside it.

### What this module does not do

It does not continue. "A write command never continues past a returned
append error **even when the proven prefix shows the line present**
(deferred: continuation after a recovered append error)." So [`protocol`]
reports `Present` and still ends: the barrier's own fold is dropped with the
rest, and the next resume rebuilds it from (a0).

## `pub struct RunIdentity {`

---------------------------------------------------------------------------
What one emit borrows
---------------------------------------------------------------------------

## `pub struct RunIdentity {`

The facts about the run that do not change between emits, and that the
append-error protocol needs in order to reopen and report.

`inputs` and `committed_first_line_sha256` are here rather than passed to
the protocol because the barrier the protocol establishes is the *same*
barrier recovery step (a1) establishes, over the same two inputs — a
protocol that took its own copies could establish a barrier against a
different plan than the run was folded from and prove nothing.

## `pub struct RunIdentity` › `pub run_id: String,`

The run id every refusal names.

## `pub struct RunIdentity` › `pub inputs: FrozenInputs,`

The frozen plan and its digest, which the checked replay is derived
against.

## `pub struct RunIdentity` › `pub committed_first_line_sha256: Option<String>,`

`committed.json`'s `run_started_sha256`, once the run has a commit
record. `None` before P5b, when there is no committed first line to
prove anything about.

## `pub struct EmitState<'a> {`

The mutable state one emit touches, borrowed for the call.

**Four** borrows rather than one `&mut TopologyRun` because this module is
deliberately not the run: `emit` is called from creation, from recovery, and
from the loop, and each of those holds its own surrounding state. What every
one of them must hand over is exactly this.

**It said five, and said the protocol's obligations were each a statement
about one of them.** Both halves stopped being true when `bcc5c2f` moved
obligation (3) to the caller and deleted the `invocations` field: the count
is four, and obligation (3) is now *not* a statement about a field here —
[`AppendError`]'s own protocol note says so in as many words. `9b6fef1`
removed that field's stranded doc lines from `warnings` and left this
sentence seven lines above them unread. `R5-SEAMS-005`.

## `pub struct EmitState<'a>` › `pub fold: &'a mut TopologyFold,`

The derived state. Poisoned by the protocol, never mutated by it.

## `pub struct EmitState<'a>` › `pub log: &'a mut EventLog,`

The append handle the stable-prefix barrier entitled this command to.

## `pub struct EmitState<'a>` › `pub reservations: &'a mut Reservations,`

The provisional-reservation ledger. Cancelled by the protocol.

## `pub struct EmitState<'a>` › `pub warnings: &'a mut Vec<String>,`

Where a torn-tail normalization at the protocol's reopen is reported.

**Two lines that are no longer here used to be**: "The invocation
ledger. Every still-running entry is cancelled by the protocol;
cancelling the *processes* is the caller's." `bcc5c2f` deleted the
`invocations: &'a mut InvocationLedger` field they documented and left
them stranded on this one, so rustdoc rendered a warnings sink as a
ledger. `PR7-R3-EMIT-003`; confirmed against the tree and removed
2026-08-26. `EmitState` has four fields and each now documents itself.
(The first draft of this note opened "the two lines **above this one**",
which resolves only against the version it replaced — S5 round 5's
`seams` lens, filed to the standards work-list.)

## `pub enum AppendOutcome {`

---------------------------------------------------------------------------
The outcome of an append whose result was unknown
---------------------------------------------------------------------------

## `pub enum AppendOutcome {`

What the reopened, proven prefix says about the line whose append failed.

Three values, not two, and the third is not an error case dressed up: "when
the barrier's sync fails, the reread is unstable, or the replay refuses, it
ends the command **without asserting either**". A protocol that folded that
into `Absent` would report a durable previous prefix on the strength of a
prefix nothing proved.

## `pub enum AppendOutcome` › `Present,`

The proven prefix contains the line: the transition is committed and
durable.

## `pub enum AppendOutcome` › `Absent,`

It does not: the previous prefix stands and is durable.

## `pub enum AppendOutcome` › `Undetermined {`

The barrier did not hold. Neither is asserted, and the next resume
establishes the barrier before acting.

## `pub enum AppendOutcome` › `step: BarrierStep,`

Which step of the barrier refused.

## `pub enum AppendOutcome` › `detail: String,`

What that step found.

## `impl AppendOutcome` › `pub fn describe(&self) -> String {`

The sentence the infrastructure error carries.

## `pub enum FirstAppendDisposition {`

What the creator reports about a run whose `run_started` append failed.

Only `Event.AppendFirst` has one. Every later append's outcome is a
statement about a transition; this one is a statement about whether the run
exists at all, and the commit record has already been published either way
(P5b precedes P6), so **neither half is ever deleted from here on**.

## `pub enum FirstAppendDisposition` › `Committed,`

The proven prefix holds the `run_started`: the run is committed.

## `pub enum FirstAppendDisposition` › `RetainedPossiblyCommitted,`

It does not. The commit record exists, so the directory is retained and
reported as a **possibly committed** husk rather than removed.

## `pub enum FirstAppendDisposition` › `UndeterminedAndRetained,`

The barrier did not hold: retained, and nothing asserted about it.

## `pub struct AppendError {`

An append that was entered and returned an error, after **all five**
obligations ran.

Reaching this type is proof that obligation (3) ran, because
[`UncancelledAppend::cancelling`] is its only constructor and it takes the
ledger. Everything the protocol established before (3) is on the report;
what this adds is the count only the discharge could know.

## `pub struct AppendError` › `pub report: UncancelledAppend,`

What obligations (1), (2), (4) and (5) established.

## `pub struct AppendError` › `pub cancelled_invocations: usize,`

How many still-running invocations the ledger cancelled.

## `pub struct AppendError` › `_cancelled: Cancelled,`

Proof that obligation (3) ran, and the reason this type has no
struct-literal construction outside this module.

`Cancelled`'s own field is private, so nothing else in this crate can
build one — the `PrivateHalfProof` device applied to an obligation
instead of a directory.

## `struct Cancelled(());`

Proof that in-flight invocations were cancelled.

## `pub struct UncancelledAppend {`

The append-error report with obligation (3) still outstanding.

Obligations (1), (2), (4) and (5) have run — the fold is poisoned, the
provisional reservation is cancelled, nothing was retried or rebuilt from
memory, and the stable-prefix barrier is established. What has not run is
the ledger half of "in-flight invocations are cancelled", because the ledger
belongs to the caller: it is the same object [`crate::engine::topology::AttemptContext`]
registers every Runner process in, and an emitter that held it for its whole
life could not lend it to the attempt that is running.

That is the same reason `hooks` is a per-call parameter of
[`crate::engine::topology::EventEmitter::emit`] and not a field: "the caller
holds the same bundle for its own effects and cannot lend it for an
emitter's whole lifetime". This is that sentence applied to the ledger.

The obligation is discharged by [`Self::cancelling`], which is the only
constructor of [`AppendError`].

**Two production call sites, not one.** This sentence read "`cancel_all_running`
has one call site, and this is it"; the other is
`AttemptContext::cancel_in_flight` (`attempt.rs`), the `T-ATTEMPT`
halt-cancellation path, and it is in this slice.

**No raw hit count here, deliberately.** The first draft quoted one, and a
count over the tree changes whenever anything — including a doc comment
naming the function — is added: it read three, then four, then five across
three commits, each time correctly and each time differently.
`PR7-R6-ATT-004`. The claim that carries the obligation is the one above and
it is stable: `cancelling` is the only constructor of [`AppendError`], so the
obligation cannot be discharged by forgetting it.
`PR7-R3-EMIT-005`; corrected 2026-08-26. The claim that matters is unchanged
and is the one above: `cancelling` is the only constructor of
[`AppendError`], so the obligation cannot be discharged by forgetting it.

## `pub struct UncancelledAppend` › `pub run_id: String,`

The run the operator is told about.

## `pub struct UncancelledAppend` › `pub kind: &'static str,`

The event kind whose outcome is unknown.

## `pub struct UncancelledAppend` › `pub site: EventSite,`

The site it was filed at.

## `pub struct UncancelledAppend` › `pub cause: UpstrokeError,`

What the funnel returned. Kept because the funnel names the point that
poisoned the handle, and that is what says *where* the outcome became
unknown.

## `pub struct UncancelledAppend` › `pub outcome: AppendOutcome,`

What the reopened, proven prefix says.

## `pub struct UncancelledAppend` › `pub cancelled_reservation: bool,`

Whether a provisional reservation was held and cancelled.

## `impl UncancelledAppend` › `pub fn creator_disposition(&self) -> Option<FirstAppendDisposition> {`

The creator's report, for `Event.AppendFirst` and for nothing else.

`None` rather than a fourth `AppendOutcome` variant: the three shapes
are a projection of the outcome onto the run's commitment boundary, and
a run has exactly one of those. Deriving it here rather than storing it
makes "the disposition disagrees with the outcome" unrepresentable.

## `impl UncancelledAppend` › `pub const fn resumable(&self) -> bool {`

Whether the run is still resumable. Always: "the run is NoRunFinished
and resumable and the next resume follows the fault row of the surviving
prefix (T-APPEND) only after its own barrier".

A method rather than a comment because the three outcomes look like
three severities and are not: an undetermined outcome is *no less*
resumable than an absent one, and a caller reading the outcome to decide
would eventually decide otherwise.

## `impl UncancelledAppend` › `pub fn cancelling(self, invocations: &mut InvocationLedger) -> AppendError {`

Discharge obligation (3) and mint the report.

"In-flight invocations are cancelled through the Runner"; this is the
ledger half. The Runner half — cancelling the pipelines and discarding
the completions — is the caller's too, and always was.

## `pub enum EmitError {`

Why an emit did not apply its transition.

The first three all mean **nothing was written**, and they are kept apart
because they fail at three different steps of `emit`'s six and an operator
asked to act on one of them is being asked to act on a different thing.
Only [`Self::AppendFailed`] carries an outcome-unknown append.

## `pub enum EmitError` › `Unserializable(UpstrokeError),`

The value does not survive its own wire format. Serialization's
business, a step before the fold's — and an append that never happened
rather than one whose outcome is unknown.

## `pub enum EmitError` › `Refused(FoldError),`

The checked fold refused the transition. `emit`: "a `FoldError` aborts
**before any write**".

## `pub enum EmitError` › `NotEntered(UpstrokeError),`

The funnel refused *before* the append was entered: a poisoned handle, a
legacy handle, or a site that is not this line's. Nothing was written,
so the append-error protocol does not apply and did not run.

## `pub enum EmitError` › `AppendFailed(Box<UncancelledAppend>),`

The append was entered and returned an error. The protocol ran, and this
is its report.

## `impl EmitError` › `pub const fn wrote_nothing(&self) -> bool {`

Whether this refusal left the log exactly as it found it.

True for the three pre-append refusals and false for
[`Self::AppendFailed`], where the whole point is that the process cannot
tell. INV-02's "an invalid transition is never appended" is this
predicate over the first two.

## `impl EmitError` › `pub fn append_error(&self) -> Option<&UncancelledAppend> {`

The outcome-unknown append this refusal carries, if it carries one.

## `fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` › `Self::AppendFailed(error) => write!(`

Same reason as `EmitFailure`: an outstanding obligation is not
a report and must not read like one.

## `impl EmitError` › `pub fn discharging(self, invocations: &mut InvocationLedger) -> UpstrokeError {`

This refusal as the error a caller propagates, discharging obligation
(3) on the way if one is outstanding.

**There is deliberately no `From<EmitError> for UpstrokeError`.** The
conversion reads the report — `append.to_string()` — and reaching the
report is exactly what must require the ledger. A blanket `From` would
let every `?` in the tree turn an outstanding obligation into a string
and drop it, which is the "remembered, not enforced" failure this
design exists to make impossible.

## `impl EmitError` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `pub fn emit(`

---------------------------------------------------------------------------
emit
---------------------------------------------------------------------------

## `pub fn emit(`

Build, check, append, and only then apply.

The six steps of `coordinator_integration.emit`, in order, with the two
aborts the sentence specifies. Two of them are worth stating rather than
leaving to the reader:

* `plan_transition` is fed the **round-tripped** event, never the one just
  constructed. Those are the same value only when the wire format is
  lossless for it, and the whole reason the round-trip exists is that it is
  not always. Checking the original would check a transition the log can
  never reproduce.
* `apply_delta` runs only after the funnel returned `Ok`. The delta is a
  [`crate::topology::fold::TopologyDelta`], which nothing outside the fold
  can construct, so "the only path into the state runs through
  `plan_transition`" is a type property; "and only after the append" is this
  function's, and it is the one the protocol below exists to hold.

### Errors

[`EmitError`]. The first three variants mean nothing was written; the fourth
means the append was entered, its outcome is unknown, and the append-error
protocol has already run.

## `let event = TopologyEvent {`

build → serialize → round-trip.

## `let delta = state`

plan_transition, on the checked event. A FoldError aborts before any
write — including the `Poisoned` refusal a previous append-error protocol
installed.

## `let site = site_for(&checked.body);`

append the exact bytes through the Event funnel.

## `let poisoned_before = state.log.poisoned_at().is_some();`

Whether the append was *entered* is the funnel's own answer, not a guess
from the error value: every refusal before entry (wrong site, wrong
scope, already-poisoned handle) leaves `poisoned_at` where it was, and
every failure after entry sets it. Reading it on both sides of the call
is what makes "an Err returned by the funnel **after the append was
entered**" a decidable condition rather than a description.

## `Ok(()) => {`

apply_delta only after the funnel returned Ok.

## `Err(EmitError::NotEntered(cause))`

Nothing was written. The delta is dropped unapplied and the fold
is left usable, because this is not an outcome-unknown append.

## `fn protocol(`

`coordinator_integration.append_error_protocol`, in the order it specifies.

Five obligations, and each one is a line here rather than a rule a call site
is asked to remember. What is *not* here is as much of the contract as what
is: no retry of the append, no removal of anything, and no report, status or
question payload built from `state.fold` — which by then holds a transition
that may or may not be durable and can vouch for neither.

## `state.fold.poison();`

(1) The fold is poisoned here, explicitly, and this is the only caller
    that does it. `EventLog` poisoned its own handle inside the funnel;
    that is a different object, and a fold left unpoisoned goes on
    accepting `plan_transition` for a state whose last transition may or
    may not exist on disk.

## `let cancelled_reservation = state.reservations.cancel_any();`

(2) The provisional reservation, if one is held. Cancelled without being
    named: the coordinator is ending and asserting *which* reservation it
    holds would be one more thing derived from a state it cannot vouch
    for.

## `let path = state.log.path().to_path_buf();`

(3) Not here. The ledger half of "in-flight invocations are cancelled
    through the Runner" is the caller's, beside the Runner half that
    always was — see `UncancelledAppend`, which is what this returns and
    which cannot become a report without it.

## `let path = state.log.path().to_path_buf();`

(4) No retry. No cleanup. No report from memory. Stated by absence,
    which is the only way it can be stated.

## `let path = state.log.path().to_path_buf();`

(5) Reopen through `Event.OpenLog` (which normalizes a torn tail) and
    establish the stable-prefix barrier before anything is reported. The
    poisoned handle in `state.log` is left exactly as it is: it refuses
    every later append, which is what "never retried" means, and reopening
    *through it* is not a thing the funnel offers.

## `Ok(prefix) => {`

"whether the proven prefix contains the line". The line is the last
thing this process attempted to append and the log is append-only, so
the question is exactly whether the proven prefix *ends* with those
bytes — a `contains` would answer yes for an identical earlier line
that this append had nothing to do with.

## (end of file)

`prefix` is dropped here, fold and all. "A write command never
continues past a returned append error even when the proven
prefix shows the line present."

## `pub enum EmitFailure {`

A failure on a path that emits, with obligation (3) still outstanding if the
append was entered.

**The error type of the emit seam, and the reason it is not
[`UpstrokeError`].** An ordering module — `dispatch.rs`, `candidate.rs` —
emits without holding the run's ledger, so it cannot discharge obligation
(3) and must not be able to pretend it did. Carrying the obligation in the
error is what makes it travel to the one place that can: the driver, which
owns the ledger because [`crate::engine::topology::AttemptContext`] registers
every Runner process in it.

`From<UpstrokeError>` exists so a function that fails for ordinary reasons
*and* emits still returns one error type and still uses `?` for both.

## `pub enum EmitFailure` › `Clean(UpstrokeError),`

Something other than an entered append: an ordinary refusal, or one of
the three pre-append aborts. No obligation.

## `pub enum EmitFailure` › `Undischarged(Box<UncancelledAppend>),`

The append was entered and failed. Obligation (3) is outstanding.

## `impl EmitFailure` › `pub fn discharging(self, invocations: &mut InvocationLedger) -> UpstrokeError {`

The error a caller propagates, discharging obligation (3) on the way if
one is outstanding.

## `impl EmitFailure` › `pub const fn wrote_nothing(&self) -> bool {`

Whether this failure left the log exactly as it found it.

## `impl EmitFailure` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` › `Self::Undischarged(append) => write!(`

**Deliberately not the report.** Rendering it here would be a
second way to read an `AppendError`'s content without the ledger,
which is the whole of what `cancelling` is supposed to gate — and
`to_string()` is the easiest possible bypass. What a caller may
know before discharging is that an entered append failed and at
which site; the outcome, the cause and the creator disposition
arrive with the report.
