//! `TopologyRun` — the driver `decisions.sequential_substrate` names twice.
//!
//! `engine`: "`src/engine/topology.rs` TopologyRun drives schema 4 at
//! max_parallel = 1 synchronously; every path exists here before Tokio".
//! `selection`: "schemas 1-3 always run the legacy engine; **schema 4 always
//! runs TopologyRun**".
//!
//! # Why this module was missing, and what that cost
//!
//! For the whole of PR7's implementation and two review rounds there was no
//! such type. `create_run`, `run_recovery_order`, `select`, `dispatch` and
//! `close_at_run_end` were each reachable **only from their own tests**, and
//! nothing outside `select.rs` so much as matched on a [`Step`]. Every
//! component of the run was built and tested; no caller sequenced them.
//!
//! Nothing in the project could see it. All 117 named tests passed, every gate
//! was green on three platforms, and a per-lane review reads the lanes that
//! exist. A mutation catalogue measures whether existing code is pinned, and
//! **omission has nothing to mutate.** It was found by asking *which command
//! runs this?* rather than *does this pass?*
//!
//! Measured afterwards: of 265 withheld-catalogue entries authored from the
//! packet alone by readers forbidden to open this directory, **93 — 35% — are
//! written against this module**, naming methods like `run_fresh` and
//! `initialize_slots`. Six independent readers all assumed the driver existed,
//! because the specification describes one.
//!
//! # So the loop's branches are a type
//!
//! `loop` is an ordered enumeration exactly like `recovery_order`, and it fails
//! the same way. [`LoopBranch`] transcribes it, [`LoopBranch::disposition`]
//! says of each branch whether this build performs it, refuses it, or has **not
//! yet implemented** it — and that third answer is deliberately in the type
//! rather than in a comment. A branch nobody has written and a branch nobody
//! has *named* are the same thing to every instrument here; naming it is the
//! whole difference between debt and an omission.

use crate::error::UpstrokeError;
use crate::topology::events::TopologyEventBody;

use crate::interaction::Sleeper;
use crate::topology::events::GenerationId;
use crate::topology::fold::{FrozenInputs, TopologyFold};
use crate::topology::paths::{GitPath, PathSet};
use crate::topology::registry::TaskKey;
use crate::workspace_manager::WorkspaceManager;

use super::dispatch::{DispatchKind, DispatchRequest, Dispatched, EventEmitter, dispatch};
use super::emit::{EmitState, RunIdentity, emit};
use super::identity::{InvocationLedger, ReservationKind, Reservations};
use super::recover::RunHandle;
use super::seams::{TimeSource, TopologyHooks};
use super::select::{Admitted, Ceiling, Spend, Step, checkpoint, select};
use super::settle::Deferral;

// ---------------------------------------------------------------------------
// The production emitter
// ---------------------------------------------------------------------------

/// The one emitter a run's appends go through.
///
/// **Before this, `EventEmitter` had a single implementation in the whole tree
/// and it was `#[cfg(test)]`.** Same root cause as the missing driver: the seam
/// was written for a caller nobody built. And that test emitter re-implements
/// the append — round-trip, `plan_transition`, append, `apply_delta` — rather
/// than calling [`emit`], so it **does not run the append-error protocol's five
/// obligations**: no explicit poison, no reservation cancellation, no
/// in-flight invocation cancellation, no reopen, and no
/// present/absent/undetermined report. Every dispatch, attempt, settle and
/// candidate test drives through it, which is why the protocol's coverage over
/// the pipeline is thinner than the suite's size suggests.
///
/// This type exists so the production path does not inherit that. It is a
/// forwarder and deliberately nothing else — there is **one** implementation of
/// the protocol and it is [`emit`]. A slice whose dominant finding class is
/// duplication does not get a second one.
pub struct RunEmitter<'a> {
    /// What every refusal names, and what the checked replay is derived
    /// against.
    pub identity: &'a RunIdentity,
    /// The fold, the append handle, the two ledgers, and the warnings — the
    /// five things one append touches.
    pub state: EmitState<'a>,
    /// Where the event's timestamp comes from.
    pub clock: &'a dyn TimeSource,
}

impl EventEmitter for RunEmitter<'_> {
    /// [`emit`], and nothing before or after it.
    ///
    /// The event it returns is discarded because no caller of this trait has
    /// ever wanted it: what a caller needs to know is whether the effect that
    /// follows the append may run, and that is the `Result`. `emit` itself
    /// hands the round-tripped event to `plan_transition`, so the value has
    /// already done its work by the time it reaches here.
    ///
    /// # Errors
    ///
    /// Whatever [`emit`] returns, converted at the boundary. Every variant
    /// means the same thing to a caller — the following effect must not run —
    /// and they differ only in whether the log was touched, which
    /// `EmitError::wrote_nothing` answers for a reader that cares.
    fn emit(
        &mut self,
        body: TopologyEventBody,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(), UpstrokeError> {
        emit(self.identity, &mut self.state, self.clock, body, hooks)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The loop's branches, as the packet names them
// ---------------------------------------------------------------------------

/// One branch of `decisions.sequential_substrate.loop`, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoopBranch {
    /// "after recovery: ingest answers (never after `budget_exceeded` or a
    /// halting settlement in this epoch)". Before selection, not a [`Step`].
    IngestAnswers,
    /// "if an eligible integration exists: check the ceiling … then take a
    /// provisional integration reservation and integrate exactly one".
    Integration,
    /// "else if a `ready_retry` task exists: ceiling check, provisional
    /// {pipeline} reservation, next attempt in the retained generation".
    ReadyRetry,
    /// "else if a ready task exists: ceiling check, provisional dispatch
    /// reservation, dispatch, run one attempt through the Runner and settle".
    ReadyDispatch,
    /// "else if any Deferred task or verification-deferred candidate exists …
    /// sleep the defer backoff and append `defer_wait_elapsed`".
    DeferBackoff,
    /// "else apply the hard-block rules (attached-terminal prompt or
    /// `wait_on_block` for open questions)".
    HardBlock,
    /// "else run-end closure, `derived_outcome`, `run_finished`, terminal
    /// finalization per `run_end_policy`".
    Closure,
}

/// What this build does with a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Implemented and performed.
    Performed,
    /// Refused before any append, by a live clause naming this build.
    RefusedByCheckpoint,
    /// **Not written yet.** Carried in the type so it cannot become the kind of
    /// omission this module exists because of.
    NotYetImplemented,
    /// Partly written, with both halves named **in the branch's own words**.
    ///
    /// `loop` states each branch as a sequence of clauses, and a branch can be
    /// honestly half-built: the ready-dispatch branch's first three clauses are
    /// a reservation and a dispatch, its last is an entire attempt through the
    /// Runner. Collapsing that into `Performed` would claim work nobody did,
    /// and into `NotYetImplemented` would hide a production append that
    /// genuinely happens. Neither is true, so the type says both.
    PartlyImplemented {
        /// The clauses this build performs.
        performs: &'static str,
        /// The clauses it refuses by name, having performed the ones above.
        owes: &'static str,
    },
}

impl LoopBranch {
    /// The seven branches, in the packet's order.
    ///
    /// Transcribed from `decisions.sequential_substrate.loop`. The order of
    /// this array **is** the claim; a test compares behaviour against it rather
    /// than against a second list written from the implementation.
    pub const ALL: [Self; 7] = [
        Self::IngestAnswers,
        Self::Integration,
        Self::ReadyRetry,
        Self::ReadyDispatch,
        Self::DeferBackoff,
        Self::HardBlock,
        Self::Closure,
    ];

    /// A short name for the branch, for a refusal message and a test.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::IngestAnswers => "ingest answers",
            Self::Integration => "integration",
            Self::ReadyRetry => "ready_retry",
            Self::ReadyDispatch => "ready dispatch",
            Self::DeferBackoff => "defer backoff",
            Self::HardBlock => "hard block",
            Self::Closure => "run-end closure",
        }
    }

    /// What this build does with it, and — for anything but `Performed` — the
    /// reason belongs in the arm, not in prose somewhere else.
    #[must_use]
    pub const fn disposition(self) -> Disposition {
        match self {
            // `checkpoint_refusals`: "an intermediate build refuses, before any
            // append, any operation whose terminals it does not implement
            // (PR7: integration and run end beyond refusal)". Both are made
            // unrepresentable rather than remembered — `Admitted` carries five
            // of `Step`'s seven variants, so no value reaching the acting half
            // can name either.
            Self::Integration | Self::Closure => Disposition::RefusedByCheckpoint,
            // `Deferral::wait` sleeps the backoff and returns the event;
            // `TopologyRun::step` appends it. In that order — the event records
            // a wait that *elapsed*, so appending it first would put a claim in
            // the log a kill during the sleep would make false.
            Self::DeferBackoff => Disposition::Performed,
            // The branch reads "ceiling check, provisional dispatch
            // reservation, dispatch, run one attempt through the Runner and
            // settle". The first three are here. The fourth is an attempt: a
            // ladder rung, an adapter-built worker command, a spawn, a capture,
            // gates, reviews and a settlement — and the state this build leaves
            // instead is `OpenNoAttempt`, which is a **tabled** state, not a
            // stuck one: recovery step (g) recreates its worktree at its base,
            // and `close_at_run_end` closes it. Stopping here leaves the run in
            // a shape the system already knows how to recover.
            Self::ReadyDispatch => Disposition::PartlyImplemented {
                performs: "ceiling check, provisional dispatch reservation, dispatch",
                owes: "run one attempt through the Runner and settle",
            },
            Self::IngestAnswers | Self::ReadyRetry | Self::HardBlock => {
                Disposition::NotYetImplemented
            }
        }
    }

    /// The branch a selected [`Step`] belongs to.
    ///
    /// One mapping, so "which branch is this" is never re-derived at a second
    /// site — the two-rules-that-can-disagree shape this slice has paid for
    /// repeatedly.
    #[must_use]
    pub const fn of(step: &Step) -> Option<Self> {
        match step {
            // Not a branch of the loop: the append-error protocol has already
            // ended the command. `select` returns it so that a poisoned fold
            // cannot be read as "no further transition, therefore end the run".
            Step::Poisoned => None,
            // A breach is recorded *by* the branch that asked, and every
            // asking branch is an admitting one; the ceiling is never consulted
            // outside one.
            Step::BudgetExceeded(_) => None,
            Step::Integrate { .. } => Some(Self::Integration),
            Step::Retry { .. } => Some(Self::ReadyRetry),
            Step::Dispatch { .. } => Some(Self::ReadyDispatch),
            Step::Backoff => Some(Self::DeferBackoff),
            Step::HardBlock { .. } => Some(Self::HardBlock),
            Step::Closure(_) => Some(Self::Closure),
        }
    }

    /// The refusal a not-yet-implemented branch returns.
    ///
    /// # Errors
    ///
    /// Always [`UpstrokeError::Refused`]; the value exists so the message is
    /// written once.
    pub fn unimplemented(self) -> UpstrokeError {
        match self.disposition() {
            Disposition::PartlyImplemented { performs, owes } => UpstrokeError::Refused {
                message: format!(
                    "the schema-4 run loop's `{}` branch performed {performs}, and this build \
                     does not {owes}",
                    self.label()
                ),
            },
            _ => UpstrokeError::Refused {
                message: format!(
                    "the schema-4 run loop selected its `{}` branch, which this build does not \
                     implement yet; no effect was performed and no event was appended",
                    self.label()
                ),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// The seams one iteration of the loop needs, and nothing it owns.
///
/// Separate from [`TopologyRun`] because these are the *caller's* — a clock, a
/// sleeper, the hook bundle — while the run owns the fold, the log, the
/// ledgers and the locks. A driver that owned its clock could not be driven
/// deterministically by a test, which is the whole of what "with seams from the
/// start" means here.
pub struct RunSeams<'a> {
    /// The execution root and its Git funnels. Every effect of the loop that is
    /// not an append goes through it.
    pub manager: &'a WorkspaceManager,
    /// Where a durable event's timestamp comes from.
    pub clock: &'a dyn TimeSource,
    /// What the defer backoff sleeps on.
    pub sleeper: &'a dyn Sleeper,
}

/// What one iteration of the loop did.
///
/// A value rather than `()` so a test asserts on the branch that ran rather
/// than on the absence of an error — and so `drive` can tell "made progress"
/// from "waited", which is the difference between a loop that is working and
/// one that is spinning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// The defer backoff elapsed and `defer_wait_elapsed` is durable.
    Waited {
        /// How long this round slept.
        waited_ms: u64,
        /// Which wait it was.
        round: u32,
    },
    /// A ceiling refused the next spawn and `budget_exceeded` is durable.
    ///
    /// `loop`: a breach "appends `budget_exceeded` before any effect and
    /// **proceeds to closure**" — and closure is one of the two terminals this
    /// build refuses, so the next iteration ends the command. The append and
    /// the refusal are deliberately two iterations: the record of the breach is
    /// durable either way, which is what makes the refusal diagnosable.
    BudgetExceeded,
}

/// `TopologyRun` — the schema-4 run, driven.
///
/// Owns exactly what a run owns: the fold, the append handle, the two locks
/// (inside [`RunHandle`]), the two provisional ledgers, and the ceiling it was
/// configured with. Everything else is a seam.
///
/// **It does not yet own the slot assertion (R3), and that is deliberate.** The
/// assertion belongs to the run and `SlotAssertion::balances()` has to be true
/// at process end — but nothing here acquires a slot until the dispatch and
/// retry branches exist, and a field nothing reads is a claim the code does not
/// back. It arrives with the branch that uses it.
pub struct TopologyRun {
    handle: RunHandle,
    identity: RunIdentity,
    reservations: Reservations,
    invocations: InvocationLedger,
    warnings: Vec<String>,
    ceiling: Ceiling,
    spend: Spend,
    deferral: Deferral,
}

impl TopologyRun {
    /// Take over a run a completed recovery handed back.
    ///
    /// `run_recovery_order` returns the handle beside its summary; this is the
    /// caller that consumes it. Before the handle existed, the order dropped
    /// the log, the fold and both locks, and there was nothing for this
    /// function to take.
    #[must_use]
    pub fn resumed(handle: RunHandle, inputs: FrozenInputs, ceiling: Ceiling) -> Self {
        let identity = RunIdentity {
            run_id: handle.started.run_id.clone(),
            inputs,
            committed_first_line_sha256: None,
        };
        Self {
            handle,
            identity,
            reservations: Reservations::new(),
            invocations: InvocationLedger::new(),
            warnings: Vec::new(),
            ceiling,
            spend: Spend::new(),
            deferral: Deferral::default_backoff(),
        }
    }

    /// The fold this run derives every decision from.
    #[must_use]
    pub fn fold(&self) -> &TopologyFold {
        &self.handle.fold
    }

    /// Warnings accumulated across the run, in order.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// How many pipeline entitlements this run currently holds.
    ///
    /// Exposed so a test can assert the provisional ledger is balanced after a
    /// branch that refused partway. At `max_parallel = 1` a single leaked
    /// entitlement is a full pipeline, and nothing is ever selected again.
    #[must_use]
    pub fn entitlements_held(&self) -> u32 {
        self.reservations.entitlements_held()
    }

    /// Which wait the defer backoff is on.
    #[must_use]
    pub fn defer_round(&self) -> u32 {
        self.deferral.round()
    }

    /// One iteration of `decisions.sequential_substrate.loop`.
    ///
    /// **Select, checkpoint, then act — and the order is the guarantee.**
    /// `select` appends nothing and performs nothing; `checkpoint` refuses on a
    /// value nothing has acted on, so `checkpoint_refusals`' "before any
    /// append" holds by construction rather than by this function remembering
    /// to check early enough.
    ///
    /// # Errors
    ///
    /// The checkpoint refusals — integration, run end, and a poisoned fold —
    /// each of which ends the command. Otherwise whatever the branch returns,
    /// or [`LoopBranch::unimplemented`] for a branch this build has not
    /// written, which performs nothing and appends nothing.
    pub fn step(
        &mut self,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<Progress, UpstrokeError> {
        let selected = select(&self.handle.fold, &self.ceiling, &self.spend);
        let admitted = checkpoint(selected)?;
        match admitted {
            // `loop`: "a breach appends `budget_exceeded` before any effect".
            // The append is the whole of this arm — there is no effect after it
            // to be before.
            Admitted::BudgetExceeded(exceeded) => {
                self.emit(
                    TopologyEventBody::BudgetExceeded { data: *exceeded },
                    seams,
                    hooks,
                )?;
                Ok(Progress::BudgetExceeded)
            }
            // "sleep the defer backoff and append `defer_wait_elapsed`" — in
            // that order, and `Deferral::wait` owns it. The sleep is first
            // because the event records a wait that *elapsed*: appending it
            // first would put a claim in the log that a kill during the sleep
            // would make false.
            Admitted::Backoff => {
                let elapsed = self.deferral.wait(seams.sleeper);
                let (waited_ms, round) = (elapsed.waited_ms, elapsed.round);
                self.emit(
                    TopologyEventBody::DeferWaitElapsed { data: elapsed },
                    seams,
                    hooks,
                )?;
                Ok(Progress::Waited { waited_ms, round })
            }
            Admitted::Retry { .. } => Err(LoopBranch::ReadyRetry.unimplemented()),
            Admitted::Dispatch { key, generation } => {
                self.dispatch_ready(key, generation, seams, hooks)?;
                Err(LoopBranch::ReadyDispatch.unimplemented())
            }
            Admitted::HardBlock { .. } => Err(LoopBranch::HardBlock.unimplemented()),
        }
    }

    /// The first three clauses of the ready-dispatch branch: reserve, dispatch,
    /// convert.
    ///
    /// **The reservation is provisional and it is converted at the append, not
    /// after it.** `permits.pipeline` counts unresolved transactions in the
    /// held count, and O24's "converted at `task_dispatched`" is what stops a
    /// dispatch that appended from still occupying an entitlement. A conversion
    /// placed after the worktree effects would hold the entitlement across two
    /// Git commands for no reason, and one placed before the append would
    /// release it for a dispatch that never happened.
    ///
    /// **And it is cancelled on every failure path**, which is
    /// `refusals`' "cancellation on any pre-append failure". A leaked
    /// reservation is not a hypothetical here: it is `PR7-O24-DOUBLE-VERIFICATION`,
    /// where two verifications of one worktree could leave a generation neither
    /// closed nor converted.
    ///
    /// # Errors
    ///
    /// Whatever [`dispatch`] refuses or fails at, with the reservation
    /// cancelled first. Or [`UpstrokeError::Refused`] when the task is not one
    /// the registry knows, which is a fold and a registry that disagree.
    fn dispatch_ready(
        &mut self,
        key: TaskKey,
        generation: GenerationId,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<Dispatched, UpstrokeError> {
        let request = self.dispatch_request(key, generation)?;

        // Provisional, before the effect it authorizes.
        self.reservations.take(key, ReservationKind::Dispatch)?;

        let dispatched = {
            let mut emitter = RunEmitter {
                identity: &self.identity,
                state: EmitState {
                    fold: &mut self.handle.fold,
                    log: &mut self.handle.log,
                    reservations: &mut self.reservations,
                    invocations: &mut self.invocations,
                    warnings: &mut self.warnings,
                },
                clock: seams.clock,
            };
            dispatch(seams.manager, hooks, &mut emitter, &request)
        };

        match dispatched {
            Ok(dispatched) => {
                self.reservations.convert(key, ReservationKind::Dispatch)?;
                self.deferral.progressed();
                Ok(dispatched)
            }
            Err(error) => {
                // Cancel before returning, and do not let a cancellation
                // failure hide the failure that caused it: the first error is
                // the one an operator needs.
                let _ = self.reservations.cancel(key, ReservationKind::Dispatch);
                Err(error)
            }
        }
    }

    /// What a first ordinary dispatch of `key` asks for.
    ///
    /// Every field is read from the run's own record or the frozen registry,
    /// never invented: the base is `run_started(4).base_sha`, and the predicted
    /// region is the task's `path_hints`. **An empty hint list is `RepoWide`,
    /// not an empty prefix set** — `PathSet::RepoWide` is documented as the
    /// classification for an absent answer, and a task with no hints has given
    /// one. An empty `Prefixes` would be a region that overlaps nothing, which
    /// would let every task run against every other.
    fn dispatch_request(
        &self,
        key: TaskKey,
        generation: GenerationId,
    ) -> Result<DispatchRequest, UpstrokeError> {
        let entry = self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.entries().get(key.0 as usize))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "the fold selected task {} for dispatch and the frozen registry has no such \
                     entry; the two disagree and nothing is dispatched",
                    key.0
                ),
            })?;
        Ok(DispatchRequest {
            key,
            generation,
            base: self.handle.started.base_sha.clone(),
            kind: DispatchKind::Ordinary {
                paths: predicted_region(&entry.spec.path_hints),
            },
        })
    }

    /// Append one event through the run's own emitter.
    ///
    /// Every append this type makes goes through [`RunEmitter`], which is
    /// [`emit`] and nothing else. There is no second append path in this file
    /// and there must not be one: three of this slice's findings were a second
    /// implementation of this protocol.
    fn emit(
        &mut self,
        body: TopologyEventBody,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(), UpstrokeError> {
        let mut emitter = RunEmitter {
            identity: &self.identity,
            state: EmitState {
                fold: &mut self.handle.fold,
                log: &mut self.handle.log,
                reservations: &mut self.reservations,
                invocations: &mut self.invocations,
                warnings: &mut self.warnings,
            },
            clock: seams.clock,
        };
        emitter.emit(body, hooks)
    }
}

/// The predicted region a task's path hints imply.
///
/// **An empty hint list is `RepoWide`, not an empty prefix set**, and the
/// difference is the whole reason this is a function rather than three lines at
/// the call site. `PathSet::RepoWide` is documented as "the classification for
/// an absent, unsafe, unparsable, or undecodable answer", and a task that gave
/// no hints has given an absent one. An empty `Prefixes` is the opposite: a
/// region that overlaps *nothing*, so `overlaps_another` is false against every
/// other task and the predicted lease this dispatch takes protects nothing.
///
/// At `max_parallel = 1` that is invisible — one generation runs at a time and
/// nothing can collide with it. It becomes a live defect at the first width
/// above one, which is PR11, by which time the dispatch that wrote it is many
/// slices old.
fn predicted_region(hints: &[String]) -> PathSet {
    if hints.is_empty() {
        PathSet::RepoWide
    } else {
        PathSet::Prefixes {
            paths: hints.iter().map(|hint| GitPath(hint.clone())).collect(),
        }
    }
}

#[cfg(test)]
mod tests;
