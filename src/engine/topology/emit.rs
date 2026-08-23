//! The emit path: the one place a schema-4 event is written, and what happens
//! when the write returns an error.
//!
//! `decisions.coordinator_integration.emit` is six steps and this module is
//! those six steps and nothing else:
//!
//! > "build event → serialize → round-trip → `plan_transition` → append the
//! > exact bytes through the Event funnel (written, then synced; the newline is
//! > the commit marker) → `apply_delta` only after the funnel returned `Ok`; a
//! > `FoldError` aborts before any write; an `Err` returned by the funnel after
//! > the append was entered runs the `append_error_protocol`."
//!
//! Almost every mechanism it needs already exists and is tested:
//! [`TopologyLine::round_trip`] *is* the round-trip,
//! [`EventLog::append_topology_hooked`] *is* the funnel, and
//! [`establish_stable_prefix`] *is* the barrier. What is new here is the
//! **order** over them and the protocol that runs when the append fails —
//! and this project's own measurement says orderings are where its defects
//! live.
//!
//! # The append-error protocol, and why the legacy engine is the wrong template
//!
//! `Run::drain_and_report` in [`crate::engine::coordinator`] handles a returned
//! append error by catching the propagated `Err`, building a partial report
//! **from in-memory state**, writing it, and re-returning. That is correct for
//! schema 1..3 and is forbidden here, clause by clause
//! (`coordinator_integration.append_error_protocol`):
//!
//! 1. `apply_delta` is not run and **the in-memory fold is marked poisoned**.
//!    [`TopologyFold::poison`] is called explicitly, by [`protocol`], because
//!    the two poisonings are of two different objects: [`EventLog`] poisons its
//!    own *handle*, and the fold is a separate value that does not learn
//!    anything from that. Without the explicit call `plan_transition` keeps
//!    succeeding and the next emit writes a transition derived from a state
//!    this process cannot vouch for.
//! 2. Provisional reservations are cancelled ([`Reservations::cancel_any`]) —
//!    `permits`: "cancellation on any pre-append failure, run end, shutdown, or
//!    a poisoned fold".
//! 3. In-flight invocations are cancelled. The Runner side of that is the
//!    caller's ("in-flight invocations are cancelled through the Runner");
//!    [`InvocationLedger::cancel_all_running`] is the ledger side and is this
//!    module's.
//! 4. **No retry, no cleanup, and no report, status or question payload derived
//!    from the poisoned fold.** There is no code here that does any of them,
//!    which is the only way to state that clause.
//! 5. The log is reopened through `Event.OpenLog` (torn-tail normalization) and
//!    the **stable-prefix barrier** is established before anything is reported;
//!    the command then ends naming the run id, the event kind, and whether the
//!    proven prefix contains the line — **present**, **absent**, or, when the
//!    barrier itself did not hold, **undetermined**, asserting neither. All
//!    three paths perform no effect.
//!
//! `Event.AppendFirst` has a fourth shape on top of those three, because the
//! event whose outcome is unknown is the run's own commitment boundary: "for
//! `Event.AppendFirst` the creator additionally never deletes either half (the
//! commit record already exists) and reports the run as committed, as a
//! retained possibly committed husk, or as undetermined and retained". That is
//! [`FirstAppendDisposition`], derived from the outcome rather than stored
//! beside it.
//!
//! # What this module does not do
//!
//! It does not continue. "A write command never continues past a returned
//! append error **even when the proven prefix shows the line present**
//! (deferred: continuation after a recovered append error)." So [`protocol`]
//! reports `Present` and still ends: the barrier's own fold is dropped with the
//! rest, and the next resume rebuilds it from (a0).

use std::fmt;

use crate::error::UpstrokeError;
use crate::events::log::{BarrierStep, EventLog, TopologyLine, establish_stable_prefix, site_for};
use crate::topology::effects::EventSite;
use crate::topology::events::{TopologyEvent, TopologyEventBody};
use crate::topology::fold::{FoldError, FrozenInputs, TopologyFold};

use super::identity::{InvocationLedger, Reservations};
use super::seams::{TimeSource, TopologyHooks};

// ---------------------------------------------------------------------------
// What one emit borrows
// ---------------------------------------------------------------------------

/// The facts about the run that do not change between emits, and that the
/// append-error protocol needs in order to reopen and report.
///
/// `inputs` and `committed_first_line_sha256` are here rather than passed to
/// the protocol because the barrier the protocol establishes is the *same*
/// barrier recovery step (a1) establishes, over the same two inputs — a
/// protocol that took its own copies could establish a barrier against a
/// different plan than the run was folded from and prove nothing.
#[derive(Debug, Clone)]
pub struct RunIdentity {
    /// The run id every refusal names.
    pub run_id: String,
    /// The frozen plan and its digest, which the checked replay is derived
    /// against.
    pub inputs: FrozenInputs,
    /// `committed.json`'s `run_started_sha256`, once the run has a commit
    /// record. `None` before P5b, when there is no committed first line to
    /// prove anything about.
    pub committed_first_line_sha256: Option<String>,
}

/// The mutable state one emit touches, borrowed for the call.
///
/// Five borrows rather than one `&mut TopologyRun` because this module is
/// deliberately not the run: `emit` is called from creation, from recovery, and
/// from the loop, and each of those holds its own surrounding state. What every
/// one of them must hand over is exactly this — and the append-error protocol's
/// obligations are each a statement about one of these five.
pub struct EmitState<'a> {
    /// The derived state. Poisoned by the protocol, never mutated by it.
    pub fold: &'a mut TopologyFold,
    /// The append handle the stable-prefix barrier entitled this command to.
    pub log: &'a mut EventLog,
    /// The provisional-reservation ledger. Cancelled by the protocol.
    pub reservations: &'a mut Reservations,
    /// The invocation ledger. Every still-running entry is cancelled by the
    /// protocol; cancelling the *processes* is the caller's.
    pub invocations: &'a mut InvocationLedger,
    /// Where a torn-tail normalization at the protocol's reopen is reported.
    pub warnings: &'a mut Vec<String>,
}

// ---------------------------------------------------------------------------
// The outcome of an append whose result was unknown
// ---------------------------------------------------------------------------

/// What the reopened, proven prefix says about the line whose append failed.
///
/// Three values, not two, and the third is not an error case dressed up: "when
/// the barrier's sync fails, the reread is unstable, or the replay refuses, it
/// ends the command **without asserting either**". A protocol that folded that
/// into `Absent` would report a durable previous prefix on the strength of a
/// prefix nothing proved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendOutcome {
    /// The proven prefix contains the line: the transition is committed and
    /// durable.
    Present,
    /// It does not: the previous prefix stands and is durable.
    Absent,
    /// The barrier did not hold. Neither is asserted, and the next resume
    /// establishes the barrier before acting.
    Undetermined {
        /// Which step of the barrier refused.
        step: BarrierStep,
        /// What that step found.
        detail: String,
    },
}

impl AppendOutcome {
    /// The sentence the infrastructure error carries.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Present => {
                "the proven prefix contains the line: the transition is committed and durable"
                    .to_owned()
            }
            Self::Absent => {
                "the proven prefix does not contain the line: the previous prefix stands and is \
                 durable"
                    .to_owned()
            }
            Self::Undetermined { step, detail } => format!(
                "the outcome is undetermined — the stable-prefix barrier did not hold at {step} \
                 ({detail}), so neither the line's presence nor its absence is asserted"
            ),
        }
    }
}

/// What the creator reports about a run whose `run_started` append failed.
///
/// Only `Event.AppendFirst` has one. Every later append's outcome is a
/// statement about a transition; this one is a statement about whether the run
/// exists at all, and the commit record has already been published either way
/// (P5b precedes P6), so **neither half is ever deleted from here on**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstAppendDisposition {
    /// The proven prefix holds the `run_started`: the run is committed.
    Committed,
    /// It does not. The commit record exists, so the directory is retained and
    /// reported as a **possibly committed** husk rather than removed.
    RetainedPossiblyCommitted,
    /// The barrier did not hold: retained, and nothing asserted about it.
    UndeterminedAndRetained,
}

impl fmt::Display for FirstAppendDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Committed => "committed",
            Self::RetainedPossiblyCommitted => "retained, possibly committed",
            Self::UndeterminedAndRetained => "undetermined and retained",
        })
    }
}

/// An append that was entered and returned an error, after the protocol ran.
///
/// Everything on it is a *report*. Nothing here was derived from the poisoned
/// fold: `run_id`, `kind` and `site` were known before the append, `outcome`
/// comes from the reopened prefix, and the two cancellation counts come from
/// the process-local ledgers.
#[derive(Debug)]
pub struct AppendError {
    /// The run the operator is told about.
    pub run_id: String,
    /// The event kind whose outcome is unknown.
    pub kind: &'static str,
    /// The site it was filed at.
    pub site: EventSite,
    /// What the funnel returned. Kept because the funnel names the point that
    /// poisoned the handle, and that is what says *where* the outcome became
    /// unknown.
    pub cause: UpstrokeError,
    /// What the reopened, proven prefix says.
    pub outcome: AppendOutcome,
    /// Whether a provisional reservation was held and cancelled.
    pub cancelled_reservation: bool,
    /// How many still-running invocations the ledger cancelled.
    pub cancelled_invocations: usize,
}

impl AppendError {
    /// The creator's report, for `Event.AppendFirst` and for nothing else.
    ///
    /// `None` rather than a fourth `AppendOutcome` variant: the three shapes
    /// are a projection of the outcome onto the run's commitment boundary, and
    /// a run has exactly one of those. Deriving it here rather than storing it
    /// makes "the disposition disagrees with the outcome" unrepresentable.
    #[must_use]
    pub fn creator_disposition(&self) -> Option<FirstAppendDisposition> {
        if self.site != EventSite::AppendFirst {
            return None;
        }
        Some(match self.outcome {
            AppendOutcome::Present => FirstAppendDisposition::Committed,
            AppendOutcome::Absent => FirstAppendDisposition::RetainedPossiblyCommitted,
            AppendOutcome::Undetermined { .. } => FirstAppendDisposition::UndeterminedAndRetained,
        })
    }

    /// Whether the run is still resumable. Always: "the run is NoRunFinished
    /// and resumable and the next resume follows the fault row of the surviving
    /// prefix (T-APPEND) only after its own barrier".
    ///
    /// A method rather than a comment because the three outcomes look like
    /// three severities and are not: an undetermined outcome is *no less*
    /// resumable than an absent one, and a caller reading the outcome to decide
    /// would eventually decide otherwise.
    #[must_use]
    pub const fn resumable(&self) -> bool {
        true
    }
}

impl fmt::Display for AppendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "run `{}`: the `{}` append at `Event.{}` was entered and returned an error ({}), so \
             its outcome is unknown. {}. Nothing was retried, no state was derived from this \
             process's fold, and the run is resumable.",
            self.run_id,
            self.kind,
            self.site.name(),
            self.cause,
            self.outcome.describe()
        )?;
        if let Some(disposition) = self.creator_disposition() {
            write!(
                f,
                " The run directory is reported as {disposition}; neither half is deleted."
            )?;
        }
        Ok(())
    }
}

/// Why an emit did not apply its transition.
///
/// The first three all mean **nothing was written**, and they are kept apart
/// because they fail at three different steps of `emit`'s six and an operator
/// asked to act on one of them is being asked to act on a different thing.
/// Only [`Self::AppendFailed`] carries an outcome-unknown append.
#[derive(Debug)]
pub enum EmitError {
    /// The value does not survive its own wire format. Serialization's
    /// business, a step before the fold's — and an append that never happened
    /// rather than one whose outcome is unknown.
    Unserializable(UpstrokeError),
    /// The checked fold refused the transition. `emit`: "a `FoldError` aborts
    /// **before any write**".
    Refused(FoldError),
    /// The funnel refused *before* the append was entered: a poisoned handle, a
    /// legacy handle, or a site that is not this line's. Nothing was written,
    /// so the append-error protocol does not apply and did not run.
    NotEntered(UpstrokeError),
    /// The append was entered and returned an error. The protocol ran, and this
    /// is its report.
    AppendFailed(Box<AppendError>),
}

impl EmitError {
    /// Whether this refusal left the log exactly as it found it.
    ///
    /// True for the three pre-append refusals and false for
    /// [`Self::AppendFailed`], where the whole point is that the process cannot
    /// tell. INV-02's "an invalid transition is never appended" is this
    /// predicate over the first two.
    #[must_use]
    pub const fn wrote_nothing(&self) -> bool {
        !matches!(self, Self::AppendFailed(_))
    }

    /// The outcome-unknown append this refusal carries, if it carries one.
    #[must_use]
    pub fn append_error(&self) -> Option<&AppendError> {
        match self {
            Self::AppendFailed(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for EmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unserializable(error) | Self::NotEntered(error) => write!(f, "{error}"),
            Self::Refused(error) => write!(f, "{error}"),
            Self::AppendFailed(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EmitError {}

impl From<EmitError> for UpstrokeError {
    fn from(error: EmitError) -> Self {
        match error {
            EmitError::Unserializable(error) | EmitError::NotEntered(error) => error,
            EmitError::Refused(refusal) => Self::Refused {
                message: refusal.to_string(),
            },
            EmitError::AppendFailed(append) => Self::Refused {
                message: append.to_string(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// emit
// ---------------------------------------------------------------------------

/// Build, check, append, and only then apply.
///
/// The six steps of `coordinator_integration.emit`, in order, with the two
/// aborts the sentence specifies. Two of them are worth stating rather than
/// leaving to the reader:
///
/// * `plan_transition` is fed the **round-tripped** event, never the one just
///   constructed. Those are the same value only when the wire format is
///   lossless for it, and the whole reason the round-trip exists is that it is
///   not always. Checking the original would check a transition the log can
///   never reproduce.
/// * `apply_delta` runs only after the funnel returned `Ok`. The delta is a
///   [`crate::topology::fold::TopologyDelta`], which nothing outside the fold
///   can construct, so "the only path into the state runs through
///   `plan_transition`" is a type property; "and only after the append" is this
///   function's, and it is the one the protocol below exists to hold.
///
/// # Errors
///
/// [`EmitError`]. The first three variants mean nothing was written; the fourth
/// means the append was entered, its outcome is unknown, and the append-error
/// protocol has already run.
pub fn emit(
    identity: &RunIdentity,
    state: &mut EmitState<'_>,
    time: &dyn TimeSource,
    body: TopologyEventBody,
    hooks: &mut dyn TopologyHooks,
) -> Result<TopologyEvent, EmitError> {
    // build → serialize → round-trip.
    let event = TopologyEvent {
        ts: time.now_rfc3339(),
        body,
    };
    let (line, checked) = TopologyLine::round_trip(&event).map_err(EmitError::Unserializable)?;

    // plan_transition, on the checked event. A FoldError aborts before any
    // write — including the `Poisoned` refusal a previous append-error protocol
    // installed.
    let delta = state
        .fold
        .plan_transition(&checked)
        .map_err(EmitError::Refused)?;

    // append the exact bytes through the Event funnel.
    let site = site_for(&checked.body);
    // Whether the append was *entered* is the funnel's own answer, not a guess
    // from the error value: every refusal before entry (wrong site, wrong
    // scope, already-poisoned handle) leaves `poisoned_at` where it was, and
    // every failure after entry sets it. Reading it on both sides of the call
    // is what makes "an Err returned by the funnel **after the append was
    // entered**" a decidable condition rather than a description.
    let poisoned_before = state.log.poisoned_at().is_some();
    match state
        .log
        .append_topology_hooked(site, &line, hooks.events())
    {
        // apply_delta only after the funnel returned Ok.
        Ok(()) => {
            state.fold.apply_delta(delta);
            Ok(checked)
        }
        Err(cause) if poisoned_before || state.log.poisoned_at().is_none() => {
            // Nothing was written. The delta is dropped unapplied and the fold
            // is left usable, because this is not an outcome-unknown append.
            Err(EmitError::NotEntered(cause))
        }
        Err(cause) => Err(EmitError::AppendFailed(Box::new(protocol(
            identity, state, &line, site, cause, hooks,
        )))),
    }
}

/// `coordinator_integration.append_error_protocol`, in the order it specifies.
///
/// Five obligations, and each one is a line here rather than a rule a call site
/// is asked to remember. What is *not* here is as much of the contract as what
/// is: no retry of the append, no removal of anything, and no report, status or
/// question payload built from `state.fold` — which by then holds a transition
/// that may or may not be durable and can vouch for neither.
fn protocol(
    identity: &RunIdentity,
    state: &mut EmitState<'_>,
    line: &TopologyLine,
    site: EventSite,
    cause: UpstrokeError,
    hooks: &mut dyn TopologyHooks,
) -> AppendError {
    // (1) The fold is poisoned here, explicitly, and this is the only caller
    //     that does it. `EventLog` poisoned its own handle inside the funnel;
    //     that is a different object, and a fold left unpoisoned goes on
    //     accepting `plan_transition` for a state whose last transition may or
    //     may not exist on disk.
    state.fold.poison();

    // (2) The provisional reservation, if one is held. Cancelled without being
    //     named: the coordinator is ending and asserting *which* reservation it
    //     holds would be one more thing derived from a state it cannot vouch
    //     for.
    let cancelled_reservation = state.reservations.cancel_any();

    // (3) The ledger half of "in-flight invocations are cancelled through the
    //     Runner". The Runner half — cancelling the pipelines and discarding
    //     the completions — belongs to the caller, which is the only thing
    //     holding the Runner.
    let cancelled_invocations = state.invocations.cancel_all_running();

    // (4) No retry. No cleanup. No report from memory. Stated by absence,
    //     which is the only way it can be stated.

    // (5) Reopen through `Event.OpenLog` (which normalizes a torn tail) and
    //     establish the stable-prefix barrier before anything is reported. The
    //     poisoned handle in `state.log` is left exactly as it is: it refuses
    //     every later append, which is what "never retried" means, and reopening
    //     *through it* is not a thing the funnel offers.
    let path = state.log.path().to_path_buf();
    let outcome = match establish_stable_prefix(
        &path,
        identity.inputs.clone(),
        identity.committed_first_line_sha256.as_deref(),
        state.warnings,
        hooks.events(),
    ) {
        // "whether the proven prefix contains the line". The line is the last
        // thing this process attempted to append and the log is append-only, so
        // the question is exactly whether the proven prefix *ends* with those
        // bytes — a `contains` would answer yes for an identical earlier line
        // that this append had nothing to do with.
        Ok(prefix) => {
            if prefix.bytes().ends_with(line.committed_bytes()) {
                AppendOutcome::Present
            } else {
                AppendOutcome::Absent
            }
            // `prefix` is dropped here, fold and all. "A write command never
            // continues past a returned append error even when the proven
            // prefix shows the line present."
        }
        Err(error) => AppendOutcome::Undetermined {
            step: error.step,
            detail: error.detail,
        },
    };

    AppendError {
        run_id: identity.run_id.clone(),
        kind: line.kind(),
        site,
        cause,
        outcome,
        cancelled_reservation,
        cancelled_invocations,
    }
}

#[cfg(test)]
mod tests;
