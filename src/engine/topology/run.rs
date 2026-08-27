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

use std::collections::BTreeMap;

use crate::error::UpstrokeError;
use crate::ir::{Answer, Question, QuestionId};
use crate::review;
use crate::topology::events::TopologyEventBody;

use crate::events::AttemptRecord;
use crate::interaction::Sleeper;
use crate::topology::events::{
    AttemptNumber, CandidateLeaseEffect, CommitSha, FrozenQuestion, GenerationId, SessionId,
    TopologyEvent,
};
use crate::topology::fold::{FrozenInputs, TopologyFold};
use crate::topology::registry::TaskKey;
use crate::workspace_manager::WorkspaceManager;

use super::attempt::{
    Assessment, AttemptContext, AttemptPlan, AttemptPlans, AttemptSite, Capture, InputsRequest,
    Judgement, Judging, PlanRequest, ReviewInputPolicy, ReviewPasses,
};
use super::candidate::{
    CandidateJournal, JudgedTree, append_candidate_created, append_candidate_prepared,
    create_candidates_ref, pin_candidate, reclaim_after_creation, write_candidate_commit,
};
use super::dispatch::{
    DispatchKind, DispatchRequest, Dispatched, EventEmitter, OpenGeneration, dispatch,
    resume_open_no_attempt, task_slot,
};
use super::emit::{EmitFailure, EmitState, RunIdentity, emit};
use super::identity::{InvocationLedger, ReservationKind, Reservations, SlotAssertion};
use super::recover::RunHandle;
use super::seams::{IdSource, TimeSource, TopologyHooks};
use super::select::{Admitted, Ceiling, Spend, Step, checkpoint, select};
use super::settle::{
    Deferral, FinishedAttempt, ManagedWorktrees, RetryOutcome, RetryRequest, retry, settle_failed,
};

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
    ) -> Result<(), EmitFailure> {
        emit(self.identity, &mut self.state, self.clock, body, hooks)?;
        Ok(())
    }
}

/// The run's [`CandidateJournal`], for the candidate sequence's own appends.
///
/// The sequence takes a journal rather than an [`EventEmitter`] because
/// `candidate.rs` needs to *read* the fold between its appends — the typestate
/// carries a candidate from commit to pin to prepared, and each step checks the
/// generation class it is transitioning from. So the journal is an emitter and
/// a fold reader together.
///
/// **Before this, `CandidateJournal` had one implementation and it was
/// `#[cfg(test)]`** — and that one re-implements the append rather than calling
/// [`emit`], so it runs none of the append-error protocol's five obligations.
/// The whole candidate sequence was therefore tested against an emitter no
/// production path would use, which is the same hole `RunEmitter` was written
/// to close for dispatch and attempt.
struct RunJournal<'a, 'h> {
    emitter: RunEmitter<'a>,
    hooks: &'h mut dyn TopologyHooks,
    invocations: &'h mut InvocationLedger,
}

impl CandidateJournal for RunJournal<'_, '_> {
    fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError> {
        // Three disjoint field borrows, and the discharge of obligation (3)
        // happens here because this struct is what holds the ledger.
        self.emitter
            .emit(body, self.hooks)
            .map_err(|failure| failure.discharging(self.invocations))
    }

    fn fold(&self) -> &TopologyFold {
        self.emitter.state.fold
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
    /// **Another slice's, by contract.** Distinct from
    /// [`Self::RefusedByCheckpoint`], and the distinction is not pedantry:
    /// `checkpoint_refusals` authorises this build to refuse exactly two things
    /// — "integration and run end beyond refusal" — and a third refusal wearing
    /// that name would be a build refusing something the packet never let it
    /// refuse. It is not [`Self::NotYetImplemented`] either, because that is
    /// debt this slice owes and this is not.
    NotThisSlice {
        /// Which slice owns it.
        slice: &'static str,
        /// The contract passage that says so.
        citation: &'static str,
    },
    /// Partly written, with both halves named **in the branch's own words**.
    ///
    /// `loop` states each branch as a sequence of clauses, and a branch can be
    /// honestly half-built: the ready-dispatch branch's first three clauses are
    /// a reservation and a dispatch, its last is an entire attempt through the
    /// Runner. Collapsing that into `Performed` would claim work nobody did,
    /// and into `NotYetImplemented` would hide a production append that
    /// genuinely happens. Neither is true, so the type says both.
    #[allow(dead_code)] // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
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
            // All four clauses, and every case of the last one: a success
            // through the candidate sequence, a retry, an escalation, an
            // outage deferral, a park, and a terminal failure. The last two
            // were refusals until `TaskFold::defers` and the question builder
            // existed, and both refusals went with their causes.
            Self::ReadyDispatch => Disposition::Performed,
            // "{pipeline} reservation, next attempt in the retained
            // generation" is here; running that attempt and settling it is the
            // half still owed, and it is the same machinery the ready-dispatch
            // branch already runs.
            // "{pipeline} reservation, next attempt in the retained
            // generation", whole: the reservation, `Worktree.Verify`, the
            // retry's `attempt_started`, the attempt itself and its
            // settlement. The attempt half is the ready-dispatch branch's
            // machinery, reached through the same `attempt` and `settle`.
            Self::ReadyRetry => Disposition::Performed,
            // `loop`'s "attached-terminal prompt or `wait_on_block` for open
            // questions". The channel decision is `interaction::answers_for`'s;
            // this branch asks whatever source it is handed. An answer that
            // arrives is refused, because ingesting one is PR9's.
            Self::HardBlock => Disposition::Performed,
            // **Refused by contract, not by omission.** `pr_sequence[8]` does
            // not contain the word "answer"; PR8 still refuses
            // "repair-admission answers before any append"; and PR9 owns
            // `question_answered`, `T-ANSWER`, and "AwaitingInput -> Pending
            // via validated answer". PR7's `replay_recovery` never names
            // `T-ANSWER`. Same shape as `Integration` and `Closure`, whose
            // terminals arrive in PR8 and PR10.
            Self::IngestAnswers => Disposition::NotThisSlice {
                slice: "PR9",
                citation: "`pr_sequence[8]` does not contain the word `answer`; PR8 still \
                           refuses `repair-admission answers before any append`; PR9 owns \
                           `question_answered`, `T-ANSWER`, and `AwaitingInput -> Pending via \
                           validated answer`. PR7's `replay_recovery` never names `T-ANSWER`",
            },
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

    /// The refusal a branch returns for one clause it does not implement.
    ///
    /// Distinct from [`Self::unimplemented`], which speaks for the whole
    /// branch. This one names a single clause, so a branch that performs most
    /// of itself can still refuse a case precisely — and the message says which
    /// case rather than which branch, because "ready dispatch is not
    /// implemented" would be false by the time this is reached.
    ///
    /// # Errors
    ///
    /// Always [`UpstrokeError::Refused`].
    #[allow(dead_code)] // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
    pub fn owes(self, clause: &str) -> UpstrokeError {
        UpstrokeError::Refused {
            message: format!(
                "the schema-4 run loop's `{}` branch reached a case this build does not \
                 implement: {clause}. Nothing was appended for it",
                self.label()
            ),
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
            Disposition::NotThisSlice { slice, citation } => UpstrokeError::Refused {
                message: format!(
                    "the schema-4 run loop's `{}` branch belongs to {slice}, not to this build: \
                     {citation}. No effect was performed and no event was appended",
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
    /// The boundary every process of this run crosses.
    pub runner: &'a dyn crate::runner::Runner,
    /// Where an agent name becomes an adapter.
    pub adapters: &'a dyn crate::agent::AdapterSource,
    /// The run's directories.
    pub paths: &'a crate::rundir::RunPaths,
    /// Where an attempt's plan and its review inputs are assembled. A seam
    /// because building the worker's command materializes the permissions file
    /// that defines the sandbox, and because a plan needs the run's config —
    /// neither of which is in the log the fold replays.
    pub plans: &'a dyn AttemptPlans,
    /// Where a review pass is executed. `review::run_review`, behind its seam.
    pub reviews: &'a dyn ReviewPasses,
    /// Whether the staged evidence is reviewable. `Workspace`'s policy, behind
    /// its seam.
    pub input_policy: &'a dyn ReviewInputPolicy,
    /// Where an answer comes from at a hard block: a terminal prompt or a
    /// `wait_on_block` wait, decided by `interaction::answers_for` and not
    /// here.
    pub answers: &'a dyn crate::interaction::AnswerSource,
    /// Where a question's id comes from. A seam because a ULID minted inline
    /// would append different bytes every run, and a park's whole point is
    /// that the id survives to the resume that rematerializes it.
    pub ids: &'a dyn IdSource,
    /// `decisions.run_end_policy`: whether a task's terminal failure halts the
    /// run. Run configuration rather than an injection point, and here because
    /// the settlement records it and the log is the only place it survives.
    pub halts_run: bool,
}

/// Which attempt of which rung this is, and whether it has already been
/// announced.
///
/// The difference between the two branches that run an attempt. A dispatch is
/// **attempt one of its generation**, at the rung the log records, carrying the
/// task's accumulated brief, with nothing to resume, and it appends
/// `attempt_started` itself; a retry is the generation's next attempt, may
/// resume a session, and was already announced by `settle::retry` after the
/// worktree verified.
///
/// **Attempt one of its generation, not of a *fresh* one**, and the distinction
/// is the third false half of this comment rather than pedantry. `step`'s
/// `Admitted::Dispatch` arm builds one `RunAs` for both of its paths —
/// `dispatch_ready` when `continuing` is false and `continue_open` when it is
/// true — and the continuation runs over the `OpenNoAttempt` generation a
/// **prior process** opened with `task_dispatched` and recovery step (g)
/// recreated the worktree for. `Step::Dispatch`'s own doc says so in as many
/// words: "It is the same branch reaching the same attempt over ground that
/// already exists."
///
/// `Self::FIRST_ATTEMPT` is still right for it, and the corrected sentence is
/// what leaves a reader a reason to check that: `OpenNoAttempt` means the
/// generation exists and **no attempt has started in it**, so the continuation's
/// attempt is its first. A reader who believed "fresh generation" would conclude
/// the continuation never reaches this constructor and stop looking.
/// `PR7-R4-LOOP-005`, S5 round 4.
///
/// **This sentence said "always attempt one on rung zero" with an empty brief,
/// and both halves were falsified by the repairs that made them false.** The
/// rung half by `6d3fc6f`, which made this arm read `ladder_position` and added
/// a test asserting it dispatches at rung 1; the brief half by `136fcdb`, whose
/// commit message **quotes this exact sentence as false** and then left it
/// standing. `PR7-R3-RUNAS-DOC-FALSIFIED-BY-ITS-OWN-REPAIR`, and a doc class
/// sub-shape no insertion detector can see, because nothing moved.
#[derive(Debug, Clone)]
struct RunAs {
    /// What the attempts before this one failed on, oldest first. Empty for a
    /// first dispatch, which has nothing to be told.
    feedback: Vec<crate::events::Feedback>,
    /// Which attempt of the generation.
    attempt: AttemptNumber,
    /// Which rung of the frozen ladder.
    rung: u32,
    /// The session this attempt resumes, if any.
    resume_session: Option<SessionId>,
    /// Whether `attempt_started` is already durable.
    announced: bool,
}

/// §11.4's accumulated brief, derived from the durable record.
///
/// "Failure feedback (gate log or `required_changes`) goes back to the *same
/// rung* … `attempts_per` exhausted → next rung, fresh session, **accumulated
/// feedback summary included**." The brief is what that sentence sends.
///
/// **Derived rather than accumulated, for the reason §15 states.** "One fold,
/// not two": the engine "appends an event and folds it back in through the same
/// function `resume` and `status` use … and it applies the event *as it will be
/// read back* rather than as constructed". [`Self::replay`] is that reader and
/// [`Self::record`] is the single step it is made of, so the live loop —
/// recording each settlement from the record it is appending, never from the
/// live `AttemptFailure` beside it — reaches the identical value a replay of
/// its own log does. The shape is [`super::select::Spend`]'s, and so is the
/// argument.
///
/// This was a `BTreeMap` the live path pushed to and `TopologyRun::resumed`
/// created empty. A crash after a gate failure therefore threw away the
/// diagnostic tail: the retry ran on attempt 1's prompt, spent a rung's
/// allowance to be told nothing, and could repeat the same defect. The
/// 2026-08-26 frontier review of `75da796` raised it as finding 2; the durable
/// field it needed is authorised by
/// `decisions/2026-08-26-durable-retry-feedback.md`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Brief {
    per_task: BTreeMap<TaskKey, Vec<crate::events::Feedback>>,
}

impl Brief {
    /// Nothing recorded yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What this task's earlier attempts failed on, oldest first.
    #[must_use]
    pub fn lines(&self, key: TaskKey) -> Vec<crate::events::Feedback> {
        self.per_task.get(&key).cloned().unwrap_or_default()
    }

    /// Add one settled attempt's line to its task's brief.
    ///
    /// **Every judged failure adds one, whatever settlement it produced.** The
    /// sentence is "failure feedback goes back to the same rung, **and an
    /// escalation carries the accumulated feedback with it**", and an escalation
    /// closes the generation — so the brief belongs to the *task*. Gating it on
    /// a retained generation is what made every escalation and every sessionless
    /// retry, which is every Copilot attempt (`DESIGN.md:452`), start from
    /// nothing.
    ///
    /// A record with no failure adds nothing: a successful attempt has no
    /// feedback, and `detail` is `None` on an attempt nothing judged.
    pub fn record(&mut self, key: TaskKey, record: &AttemptRecord) {
        let Some(failure) = record.failure.as_ref() else {
            return;
        };
        self.per_task
            .entry(key)
            .or_default()
            .push(crate::events::Feedback {
                attempt: record.attempt,
                tier: record.tier.clone(),
                summary: failure.reason.clone(),
                detail: failure.detail.clone(),
                human: false,
            });
    }

    /// Every task's brief over a whole log.
    ///
    /// **`attempt_finished` only, which is exactly the live loop's coverage.**
    /// Two events carry an [`AttemptRecord`] — `attempt_finished` and
    /// `candidate_prepared` — and the driver calls [`Self::record`] beside
    /// `Spend::record` at both of its own appends. `candidate_prepared` is the
    /// *successful* settlement (INV-07), so its record carries no failure and
    /// contributes nothing either way; walking it here would add a duplicate
    /// only on the replay side. `attempt_interrupted` is deliberately absent:
    /// nothing judged that attempt, the live path settles it in recovery rather
    /// than through a settlement, and a line for it would be a line replay
    /// invented.
    #[must_use]
    pub fn replay(events: &[TopologyEvent]) -> Self {
        let mut brief = Self::new();
        for event in events {
            if let TopologyEventBody::AttemptFinished { data } = &event.body {
                brief.record(data.key, &data.record);
            }
        }
        brief
    }
}

/// What the retaining incarnation kept from the attempt it settled.
///
/// The tree a retry re-gates, and what that attempt failed on. Both are
/// process-local on purpose: `T-RETAINED` is "the retaining incarnation
/// proceeds to T-RETRY; a fresh process closes it in recovery", so only the
/// process that settled the attempt may continue it, and the fold's
/// `RetainedIdle { incarnation }` refuses any other.
#[derive(Debug, Clone)]
struct Retained {
    /// `Quiescence::HoldsTree`'s subject: the cumulative work the retry re-gates.
    tree: String,
}

/// What one attempt produced, for the settlement that reads all three.
///
/// The counterpart to [`Judging`], one phase later: judging reads the
/// identities, the tree and the cheap rungs; settling reads the tree, the
/// worker's own report and the verdict. A bundle for the same reason — they
/// arrive together, are read together, and passing them singly put `settle` at
/// eight arguments.
#[derive(Debug, Clone, Copy)]
struct Produced<'a> {
    /// The exact tree the attempt captured.
    capture: &'a Capture,
    /// What the ladder's cheap rungs said, and the adapter's parse beside it.
    assessed: &'a Assessment,
    /// What the gates and reviewers said.
    judgement: &'a Judgement,
}

/// What one iteration of the loop did.
///
/// A value rather than `()` so a **test** asserts on the branch that ran rather
/// than on the absence of an error, and so that a driving loop can tell "made
/// progress" from "waited" — the difference between a loop that is working and
/// one that is spinning.
///
/// **The sentence used to name `drive` as that loop's caller and `drive` does
/// not exist**. `grep -rn 'fn drive' --include='*.rs' src/ | grep -v '///'`
/// finds a hook-harness helper in `topology/effects.rs` and two test helpers —
/// `drive_into_the_kill` and `drive_a_report` — and no loop that consumes
/// `Progress`. The first draft added "and nothing in `src/engine/`", which the
/// command's own output contradicts: two of the three *are* under
/// `src/engine/`. `PR7-R6-ATT-008`. The doc-comment filter is the other half —
/// a needle quoted here would otherwise match its own quotation,
/// `reviews/FINDINGS.md` §4. That is
/// verbatim the defect this module's own header exists because of — "six
/// independent readers all assumed the driver existed" — reproduced in the
/// justification for a type. `PR7-R3-CONTRACT-007`; confirmed against the tree
/// and corrected 2026-08-26. `step`'s callers in this slice are its tests; the
/// loop that consumes `Progress` in production is PR8's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// One attempt ran, was judged, and its settlement is **durable**.
    ///
    /// The whole of the ready-dispatch branch: ceiling check, reservation,
    /// dispatch, the attempt through the Runner, and `attempt_finished`.
    Settled {
        /// Whose attempt.
        key: TaskKey,
        /// Whether the verification ladder found nothing to reject.
        accepted: bool,
        /// Whether this settlement spent one of the rung's `attempts_per`, as
        /// `ladder::spends_allowance` prices it. The input the **next** ladder
        /// decision reads, which is why it is carried out of the branch rather
        /// than derived again there.
        spent_attempt: bool,
    },
    /// A retained generation's worktree did not verify, so the generation
    /// closed instead of retrying.
    ///
    /// Not a failure of the task: `generation_closed{WorktreeMissing}` leaves
    /// it dispatchable from a fresh generation, which is what the next
    /// iteration selects.
    GenerationClosed {
        /// Whose generation.
        key: TaskKey,
    },
    /// Nothing could run and the run is blocked on open questions.
    ///
    /// The hard-block rule applied and nobody answered. Not a terminal: an
    /// answer arriving later un-blocks the run, and that is PR9's to ingest.
    Blocked {
        /// How many questions are open.
        questions: usize,
    },
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
/// **It owns the slot assertion (R3)**, and the sentence here said it did not.
/// `slots: SlotAssertion` is a field of this struct, `TopologyRun::resumed`
/// initialises it with `SlotAssertion::new()`, and `fn attempt` threads
/// `slots: &mut self.slots` into the one production `AttemptContext`. Named
/// rather than cited by line: the correction that replaced this paragraph made
/// it thirteen lines longer, so the `run.rs:615` the first draft carried — correct
/// **at `9b6fef1`** — now points **into the correction itself**. `R5-SEAMS`/`PR7-R5-ATT-007`, and
/// CLAUDE.md's trap list already names stale line anchors as a repeat cost. The denial was written
/// while that was true — "a field nothing reads is a claim the code does not
/// back" — and stayed after the branch that uses it arrived.
/// `PR7-R3-CONTRACT-006`; confirmed against the tree and corrected 2026-08-26.
/// `SlotAssertion::balances()` has to be true at process end, and that is the
/// claim the field carries.
pub struct TopologyRun {
    handle: RunHandle,
    identity: RunIdentity,
    reservations: Reservations,
    invocations: InvocationLedger,
    warnings: Vec<String>,
    ceiling: Ceiling,
    spend: Spend,
    deferral: Deferral,
    slots: SlotAssertion,
    retained: BTreeMap<TaskKey, Retained>,
    /// §11.4's accumulated brief, per task, oldest first.
    ///
    /// **Separate from [`Retained`] because the brief outlives the retained
    /// generation.** §11.4 is "failure feedback goes back to the same rung, and
    /// an escalation carries the accumulated feedback with it" — so it belongs
    /// to the *task*, not to a generation. Holding it inside `Retained` meant it
    /// existed only for `AttemptSettlement::Retained`, which `settle::retry`
    /// produces only for a resumable same-rung retry **with** a session. Every
    /// escalation and every sessionless retry — which is every Copilot attempt,
    /// `DESIGN.md:452` — therefore ran on attempt 1's prompt.
    ///
    /// **Durable, and derived from the log.** This field said schema 4's wire
    /// could not carry it and that a resume therefore restarted the brief empty
    /// — true when written, and the defect the 2026-08-26 frontier review of
    /// `75da796` raised as finding 2. [`crate::events::FailureRecord::detail`]
    /// now carries what §11.4 sends, and [`Brief`] derives the whole map from
    /// the log by the same step the live loop applies.
    /// `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4` in `reviews/FINDINGS.md` §2 is
    /// closed by that field, authorised as Class C in
    /// `decisions/2026-08-26-durable-retry-feedback.md`.
    brief: Brief,
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
            // The digest recovery verified, not `None`. A loop that dropped it
            // would make its own appends unable to report a creator
            // disposition, while recovery's emitter — over the same run —
            // could.
            committed_first_line_sha256: Some(handle.committed_first_line_sha256.clone()),
        };
        // Read before the handle moves into the struct below it.
        let spend = Spend::replay(&handle.events);
        // **The log's brief, not an empty map.** Same reader, same reason as
        // `spend`: §11.4's feedback is on the durable record now, so a resumed
        // run tells the next worker what the attempts before the crash failed
        // on rather than handing it attempt 1's prompt.
        let brief = Brief::replay(&handle.events);
        Self {
            handle,
            identity,
            reservations: Reservations::new(),
            invocations: InvocationLedger::new(),
            warnings: Vec::new(),
            ceiling,
            // **The log's spend, not a fresh counter.** `Spend::replay` is
            // documented as the reader "a resumed process rebuilds this with",
            // and until this line it had **no production caller** — every
            // restart started the run's ceiling again at zero, so a run that had
            // spent its budget resumed with the whole of it back.
            //
            // Third occurrence of the §4 class, and the one that re-scoped it:
            // `C2` witnessed `Spend::replay`'s accumulation and its live-vs-
            // replay parity, and nothing witnessed the *read*. The class was
            // filed as "a fold field's witness proves the accumulation and not
            // the read" and `Spend` is not literally a fold field — it behaves
            // as one, and the narrow name is what let this through.
            spend,
            deferral: Deferral::default_backoff(),
            slots: SlotAssertion::new(),
            retained: BTreeMap::new(),
            brief,
        }
    }

    /// The digest this run's appends prove their committed first line against.
    ///
    /// `establish_stable_prefix` skips the check entirely when this is `None` —
    /// `if let Some(expected)` — so a loop that lost it would reopen after an
    /// append error and accept a first line the commit record does not name.
    /// That is `PR31-CONTRACT-006`, and the answer is `EMIT-002`'s: the handle
    /// carries the digest recovery verified and this is where it lands.
    pub fn commitment_digest(&self) -> Option<&str> {
        self.identity.committed_first_line_sha256.as_deref()
    }

    /// Whether this run is still holding a pipeline entitlement.
    ///
    /// The `Reservations` counterpart of [`Self::invocations_balance`], and it
    /// exists for the same reason: `permits.protocol` is "registered exactly
    /// once, settled exactly once", and a claim about a ledger is checkable
    /// only if something can ask the ledger.
    ///
    /// **A step that refused must leave nothing held.** Three catalogue entries
    /// — `PR7-PIPELINE-014`, `PR7-SELECT-024`, `PR7-SELECT-033` — take an
    /// entitlement before the step that can refuse and leak it on the refusing
    /// path, and all three were green because no test asked this question.
    #[must_use]
    pub fn holds_entitlement(&mut self) -> bool {
        // `cancel_any` reports whether there was one *and* releases it, which
        // is exactly right here: the run is being inspected after the step it
        // was supposed to have released on, and leaving it held would make the
        // next assertion in the same test read a stale ledger.
        self.reservations.cancel_any()
    }

    /// Whether every Runner process this run registered was also settled.
    ///
    /// R4's "registered and settled exactly once", as a question a test can
    /// ask. It is the ledger's own answer; nothing here re-derives it.
    pub fn invocations_balance(&self) -> bool {
        self.invocations.balances()
    }

    /// What this process believes the run has spent.
    ///
    /// Paired with `Spend::replay` over the same log, this is the parity a
    /// resumed run depends on.
    pub const fn spend(&self) -> &Spend {
        &self.spend
    }

    /// The fold this run derives every decision from.
    #[must_use]
    pub fn fold(&self) -> &TopologyFold {
        &self.handle.fold
    }

    /// Warnings accumulated across the run, in order.
    #[must_use]
    #[allow(dead_code)] // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
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
    #[allow(dead_code)] // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
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
            Admitted::Retry {
                key, generation, ..
            } => self.retry_ready(key, generation, seams, hooks),
            Admitted::Dispatch {
                key,
                generation,
                continuing,
            } => {
                let dispatched = if continuing {
                    self.continue_open(key, generation, seams, hooks)?
                } else {
                    self.dispatch_ready(key, generation, seams, hooks)?
                };
                let (plan, capture, assessed, judgement) = self.attempt(
                    dispatched.site(),
                    RunAs {
                        attempt: Self::FIRST_ATTEMPT,
                        rung: self.ladder_position(key)?.0,
                        resume_session: None,
                        // **Not empty.** A fresh generation is not a fresh
                        // task: `settle::failed` closes the generation and
                        // retries from a new one for every sessionless retry
                        // and every escalation, and §11.4 says the accumulated
                        // feedback goes with it. Empty here meant a retried
                        // worker got attempt 1's prompt verbatim — a rung's
                        // allowance spent to be told nothing.
                        feedback: self.brief.lines(key),
                        announced: false,
                    },
                    seams,
                    hooks,
                )?;
                let accepted = judgement.accepted();
                let spent_attempt = self.settle(
                    dispatched.site(),
                    &plan,
                    Produced {
                        capture: &capture,
                        assessed: &assessed,
                        judgement: &judgement,
                    },
                    seams,
                    hooks,
                )?;
                Ok(Progress::Settled {
                    key,
                    accepted,
                    spent_attempt,
                })
            }
            Admitted::HardBlock { questions } => self.hard_block(&questions, seams),
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
                Err(error.discharging(&mut self.invocations))
            }
        }
    }

    /// The hard-block branch: **apply the hard-block rules.**
    ///
    /// `loop`: "else apply the hard-block rules (attached-terminal prompt or
    /// `wait_on_block` for open questions)". Which of the two applies is
    /// **not** decided here — `interaction::answers_for` decides it, and its
    /// own doc says why: "`on_block` at an attached terminal means *prompt*,
    /// and the identical config detached means *wait for `upstroke answer`*.
    /// Deciding it here rather than in the engine keeps that distinction where
    /// the channels live." This branch asks the source it is handed and does
    /// not know which channel it got.
    ///
    /// # What it does with an answer, and why that is a refusal
    ///
    /// Nothing, yet. **Ingesting an answer is PR9's.** `pr_sequence[8]` does
    /// not contain the word *answer*; `pr_sequence[9]` (PR8) still lists
    /// "repair-admission answers refused before any append"; and
    /// `pr_sequence[10]` (PR9) owns `question_answered`, `T-ANSWER`, and
    /// "AwaitingInput (limit | human_binding) -> Pending via validated
    /// answer". PR7's `replay_recovery` does not name `T-ANSWER` at all.
    ///
    /// So an answer that arrives is refused **before any append**, which is
    /// `checkpoint_refusals`' own shape: "an intermediate build refuses, before
    /// any append, any operation whose terminals it does not implement".
    /// `Answer::Unanswered` is not an answer and is not refused — it is the
    /// detached case reporting that nobody was there, and the run stays
    /// blocked, which is exactly what the rule prescribes.
    fn hard_block(
        &mut self,
        questions: &[QuestionId],
        seams: &RunSeams<'_>,
    ) -> Result<Progress, UpstrokeError> {
        for id in questions {
            let question = self.open_question(id)?;
            match seams.answers.resolve(&question)? {
                Answer::Unanswered => {}
                Answer::Answered { .. } | Answer::Declined => {
                    return Err(UpstrokeError::Refused {
                        message: format!(
                            "question {} was answered, and ingesting an answer is PR9's: \
                             `question_answered` and `T-ANSWER` are that slice's and this one's \
                             contract does not name them. Refused before any append",
                            id.0
                        ),
                    });
                }
            }
        }
        Ok(Progress::Blocked {
            questions: questions.len(),
        })
    }

    /// One open question, in the shape [`crate::interaction::AnswerSource`]
    /// reads.
    ///
    /// The fold froze the id, kind, context and options when the settlement
    /// parked; `affected_tasks` is the display id the registry holds for the
    /// key it froze. Nothing is re-decided — `T-FAILED` is explicit that a
    /// question is rematerialized from the event and "never re-decided".
    fn open_question(&self, id: &QuestionId) -> Result<Question, UpstrokeError> {
        let open = self
            .handle
            .fold
            .open_questions()
            .and_then(|open| open.get(id))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("question {} is not open in this run's fold", id.0),
            })?;
        let frozen = &open.question;
        Ok(Question {
            id: frozen.id.clone(),
            kind: frozen.kind,
            affected_tasks: vec![crate::ir::TaskId(self.display_id(frozen.key)?)],
            context: frozen.context.clone(),
            options: frozen.options.clone(),
        })
    }

    /// **`T-DISPATCH`'s "continue attempt (no spend repeats)".**
    ///
    /// A run killed between `task_dispatched` and `attempt_started` leaves the
    /// generation `OpenNoAttempt`. Its `task_dispatched` is already durable and
    /// the fold refuses a second, so this reuses the ground instead of opening
    /// it: `Worktree.Verify` at the recorded base, or a forced recreate, and
    /// then the same attempt the fresh path runs. No spend repeats because
    /// nothing was spent — no attempt started.
    ///
    /// `resume_open_no_attempt` is what performs it, and this is the caller the
    /// design was waiting for: recovery step (g) recreated these worktrees and
    /// nothing then started an attempt in them, so the run stalled with the
    /// pipeline entitlement held by a generation no branch could select.
    fn continue_open(
        &mut self,
        key: TaskKey,
        generation: GenerationId,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<Dispatched, UpstrokeError> {
        let base = self
            .handle
            .fold
            .task(key)
            .and_then(|task| task.generations.iter().find(|held| held.id == generation))
            .map(|held| held.base_sha.clone())
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "task {} has no generation {} to continue",
                    key.index(),
                    generation.0
                ),
            })?;
        let slot = task_slot(key, generation);
        let open = OpenGeneration {
            key,
            generation,
            base: base.clone(),
            slot: slot.clone(),
            // Repairs are PR9's; a repair generation is refused by recovery
            // before the loop ever sees one.
            source: None,
        };
        resume_open_no_attempt(seams.manager, hooks, &open)?;
        self.deferral.progressed();
        Ok(Dispatched {
            key,
            generation,
            base,
            worktree: seams.manager.slot_path(&slot),
            slot,
            kind: DispatchKind::Ordinary {
                paths: self.handle.fold.predicted_region(key).ok_or_else(|| {
                    UpstrokeError::Refused {
                        message: format!("task {} has no predicted region", key.index()),
                    }
                })?,
            },
        })
    }

    /// The first half of the ready-retry branch: **reserve, verify, and start
    /// the next attempt in the retained generation.**
    ///
    /// `loop`: "if a retained generation exists: {pipeline} reservation, next
    /// attempt in the retained generation". `settle::retry` owns the order and
    /// every step of it — the reservation before the verify, because
    /// `permits.provisional_reservations` calls it "the bridge between a
    /// selection decision and its first append" and the verify is already past
    /// the decision; then `Worktree.Verify` for a worktree that is present,
    /// quiescent and still holding the retained tree.
    ///
    /// **`Quiescence::HoldsTree`, not `AtBase`.** A retained generation's whole
    /// point is that the worktree carries the previous attempt's cumulative
    /// work, so HEAD is still the base and the index is what differs. That tree
    /// comes from [`Self::retained`] — the note this process made when it
    /// settled the attempt — and it is process-local on purpose: `T-RETAINED`'s
    /// resume action is "the retaining incarnation proceeds to T-RETRY; a fresh
    /// process closes it in recovery", and the fold's
    /// `RetainedIdle { incarnation }` refuses one that tries otherwise.
    ///
    /// A verify failure is not an error: the generation closes, the reservation
    /// is cancelled, and `generation_closed{WorktreeMissing}` is appended.
    fn retry_ready(
        &mut self,
        key: TaskKey,
        generation: GenerationId,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<Progress, UpstrokeError> {
        let position = self.ladder_position(key)?;
        let held = self
            .retained
            .get(&key)
            .cloned()
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "task {} has a retained generation this process did not retain, so the tree \
                     its retry must re-gate is not known here. A fresh process closes a retained \
                     generation in recovery rather than continuing it",
                    key.index()
                ),
            })?;
        let binding = self
            .handle
            .fold
            .frozen_rung_binding(key, position.0)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "task {} has no rung {} in its frozen ladder",
                    key.index(),
                    position.0
                ),
            })?;
        let slot_for_run = task_slot(key, generation);
        let slot = slot_for_run.clone();
        // Read before `binding` moves into the request below it.
        let pool = seams.plans.pool_for(&binding.agent);

        let outcome = {
            let worktrees = ManagedWorktrees::new(seams.manager);
            retry(
                &self.handle.fold,
                &mut self.reservations,
                &worktrees,
                hooks.effects(),
                &RetryRequest {
                    key,
                    slot,
                    retained_tree: held.tree.clone(),
                    binding,
                    rung: position.0,
                    // Through the seam that owns the frozen pool table, not a
                    // second lookup here. The dispatch arm reads `plan.pool`;
                    // this arm appends before its plan exists, so it asks the
                    // same authority one step earlier (`R3-SEAMS-001`).
                    pool: pool.clone(),
                    materialization: None,
                },
            )?
        };

        match outcome {
            RetryOutcome::Start(started) => {
                let run_as = RunAs {
                    attempt: started.attempt,
                    rung: started.rung,
                    resume_session: started.resume_session.clone(),
                    // §11.4: what the attempts before this one failed on.
                    // From the task's brief, which every judged failure adds to
                    // — not from the retained generation, which only a resumable
                    // same-rung retry ever produces.
                    feedback: self.brief.lines(key),
                    // `settle::retry` built this event; appending it is what
                    // announces the attempt, and the fold refuses a second.
                    announced: true,
                };
                self.emit(
                    TopologyEventBody::AttemptStarted { data: *started },
                    seams,
                    hooks,
                )?;
                self.reservations.convert(key, ReservationKind::Retry)?;
                self.deferral.progressed();

                let base = self
                    .handle
                    .fold
                    .task(key)
                    .and_then(|task| task.generations.iter().find(|held| held.id == generation))
                    .map(|held| held.base_sha.clone())
                    .ok_or_else(|| UpstrokeError::Refused {
                        message: format!(
                            "generation {} of task {} left the fold mid-retry",
                            generation.0,
                            key.index()
                        ),
                    })?;
                let worktree = seams.manager.slot_path(&slot_for_run);
                let site = AttemptSite {
                    key,
                    generation,
                    base: &base,
                    slot: &slot_for_run,
                    worktree: &worktree,
                };
                let (plan, capture, assessed, judgement) =
                    self.attempt(site, run_as, seams, hooks)?;
                let accepted = judgement.accepted();
                let spent_attempt = self.settle(
                    site,
                    &plan,
                    Produced {
                        capture: &capture,
                        assessed: &assessed,
                        judgement: &judgement,
                    },
                    seams,
                    hooks,
                )?;
                return Ok(Progress::Settled {
                    key,
                    accepted,
                    spent_attempt,
                });
            }
            RetryOutcome::Close { closed, .. } => {
                // The reservation was already cancelled by `retry`.
                self.emit(
                    TopologyEventBody::GenerationClosed { data: closed },
                    seams,
                    hooks,
                )?;
                self.retained.remove(&key);
            }
        }
        Ok(Progress::GenerationClosed { key })
    }

    /// The attempt number a fresh generation's first attempt carries.
    const FIRST_ATTEMPT: crate::topology::events::AttemptNumber =
        crate::topology::events::AttemptNumber(1);

    /// The fourth clause of the ready-dispatch branch: **run one attempt
    /// through the Runner.**
    ///
    /// `attempt_started`, the worker, the exact-tree capture, the gate set on
    /// one shared snapshot, and each reviewer on a fresh one — in that order,
    /// which is [`AttemptContext`]'s, not a second one. This function's whole
    /// job is to assemble what that machinery reads and to hand it the seams
    /// its effects go through, and it is what makes the driver
    /// `review::run_review`'s **second production caller**.
    ///
    /// **The settle write is not here.** What comes back is a [`Judgement`] no
    /// event records yet; the caller appends it, immediately after this
    /// returns.
    ///
    /// # This block was re-targeted, and two of its sentences went false
    ///
    /// It was written for this function and an insertion moved it onto
    /// `FIRST_ATTEMPT`, where it stayed while the code beneath it changed —
    /// `PR7-R2-CONTRACT-005`, the doc-re-targeting class's second site. Both
    /// corrections are recorded rather than quietly applied, because a doc that
    /// has been wrong is worth more as a warning than as clean prose:
    ///
    /// 1. It said "the branch's `owes` says so". [`LoopBranch::owes`] has
    ///    **zero call sites** in the crate. The clause it named was never a
    ///    thing the code did.
    /// 2. It carried a section titled *"Attempt 1, rung 0, and why neither is
    ///    read from the fold"*, arguing they were properties of the branch. That
    ///    was true when written and is now the opposite of the code: both are
    ///    read from the fold, because assuming them was `C1` — a task resumed
    ///    mid-ladder dispatched at rung 0, attempt 1, forever. The argument the
    ///    section made against a fold reader is the argument this slice had to
    ///    abandon.
    fn attempt(
        &mut self,
        site: AttemptSite<'_>,
        run_as: RunAs,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(AttemptPlan, Capture, Assessment, Judgement), UpstrokeError> {
        let key = site.key;
        let binding = self
            .handle
            .fold
            .frozen_rung_binding(key, run_as.rung)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "task {} has no rung {} in its frozen ladder, so there is no binding to \
                     run it under",
                    key.index(),
                    run_as.rung
                ),
            })?;

        // **Cloned once, before the emitter takes the fold.** The plan is built
        // before the worker runs and the review inputs after it, and the
        // emitter holds `&mut fold` across both — so a reference taken here
        // would not survive to the second read. One registry entry is a few
        // strings; the alternative is the driver copying the five fields
        // `inputs` reads, which is assembly logic in the one place that must
        // not hold any.
        let entry = self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.get(key))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's frozen registry", key.index()),
            })?
            .clone();

        let plan = seams.plans.plan(&PlanRequest {
            key,
            entry: &entry,
            attempt: run_as.attempt,
            rung: run_as.rung,
            binding,
            workspace: site.worktree,
            resume_session: run_as.resume_session.clone(),
            feedback: run_as.feedback.clone(),
            materialization_observed: None,
        })?;

        let mut emitter = RunEmitter {
            identity: &self.identity,
            state: EmitState {
                fold: &mut self.handle.fold,
                log: &mut self.handle.log,
                reservations: &mut self.reservations,
                warnings: &mut self.warnings,
            },
            clock: seams.clock,
        };
        let mut cx = AttemptContext {
            manager: seams.manager,
            hooks,
            emitter: &mut emitter,
            runner: seams.runner,
            slots: &mut self.slots,
            ledger: &mut self.invocations,
            adapters: seams.adapters,
            paths: seams.paths,
            reviews: seams.reviews,
            input_policy: seams.input_policy,
        };

        // **`attempt_started` is the retry branch's, not this one's.** A retry
        // appends it inside `settle::retry`, after the worktree verified, so a
        // second append here would be refused by the fold — and rightly: the
        // verify is what makes the claim true.
        let run = if run_as.announced {
            cx.run_worker(site, &plan)?
        } else {
            cx.start(site, &plan)?
        };
        let capture = cx.capture(site)?;
        let diff = seams
            .manager
            .candidate_diff(site.slot, &capture.parent, &capture.tree)?;

        // The ladder's cheap rungs, before the expensive ones. `judge` starts
        // from this rather than from `None`, so a worker that died or produced
        // no diff never reaches a gate or a frontier reviewer.
        let assessed = cx.assess(site, &plan, &run, &capture, &diff, entry.spec.kind)?;

        let inputs = seams.plans.inputs(&InputsRequest {
            entry: &entry,
            diff,
        })?;

        let judgement = cx.judge(
            site,
            &plan,
            Judging {
                run: &run,
                capture: &capture,
                assessed: &assessed,
            },
            &inputs,
            &|pass| review::ReviewInvocations {
                pass: run.identities.review_pass(pass, 0),
                reask: run.identities.review_reask(pass, 0),
            },
        )?;
        Ok((plan, capture, assessed, judgement))
    }

    /// The branch's last clause: **settle the attempt.**
    ///
    /// `settle_failed` decides the transition, the parking, the deferral count,
    /// whether the generation survives, the lease disposition and the
    /// allowance — all of it, once. Nothing here re-decides any of them: the
    /// lease disposition comes from `GenerationLease::expected`, which is the
    /// whole of that rule, and the allowance from `ladder::spends_allowance`.
    /// This function's job is to hand that module the three answers only the
    /// run has — what the ladder decided, what the record says, and what the
    /// run's policy does with a terminal failure.
    ///
    /// # Every case of the branch, and nothing refused
    ///
    /// A success goes through the candidate sequence; a retry, an escalation
    /// and a terminal failure settle directly; an outage defers from
    /// `TaskFold::defers` (`PR7-FOLD-DEFERS-ACCUMULATOR`); and a park raises
    /// its question through [`Self::park_question`] before the settlement that
    /// records it. Two of those were refusals until the readers they needed
    /// existed, and both refusals are gone with their causes.
    fn settle(
        &mut self,
        site: AttemptSite<'_>,
        plan: &AttemptPlan,
        produced: Produced<'_>,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<bool, UpstrokeError> {
        let Produced {
            capture,
            assessed,
            judgement,
        } = produced;
        let record = crate::engine::classify::attempt_record(
            plan.attempt.0,
            crate::engine::classify::AttemptFacts {
                tier: plan.binding.tier,
                model: &plan.binding.model,
                pool: plan.pool.clone(),
                resumed: plan.resume_session.is_some(),
                outcome: &assessed.outcome,
                reviews: &judgement.reviews,
                failure: judgement.failure.as_ref(),
                // Schema 4 has no other carrier: no `SettlementTransition`
                // variant holds feedback, so this record is where §11.4's text
                // survives a crash or it does not survive one.
                feedback: crate::engine::classify::FeedbackCarrier::AttemptRecord,
            },
        );

        let Some(failure) = judgement.failure.as_ref() else {
            self.promote_candidate(site, plan, capture, record, seams, hooks)?;
            // Through the authority, not as a literal. `spends_allowance`'s
            // `None` arm is "the worker ran, and its work was judged and
            // accepted" — the same sentence, decided in the one place that is
            // total over `FailureKind`. A `true` here would be a second answer
            // to a question the ladder already owns, and the next cell someone
            // changes there would not reach this path.
            return Ok(crate::ladder::spends_allowance(None));
        };

        let policy = self.ladder_policy(site.key)?;

        let defers = self.deferrals_recorded(site.key)?;
        // Where the task stands on its ladder, from the log rather than from
        // this branch's assumptions.
        let position = self.ladder_position(site.key)?;

        // **The rung's allowance, including the attempt in hand.**
        //
        // The fold counts an attempt at its *settlement*, because that is when
        // `spends_allowance` can answer — its line is "the worker ran and
        // produced work to judge", which `attempt_started` cannot know.
        // `next_step` runs one step earlier: it *decides* the transition this
        // settlement will carry, so the append has not happened and the fold's
        // count does not yet include this attempt.
        //
        // So the driver asks the same question about the one attempt the fold
        // has not seen. That is one rule with one implementation consulted about
        // two subjects — the settled attempts and the live one — and not a
        // second implementation: change a cell in `spends_allowance` and both
        // move together, which is exactly what failed to happen while the fold
        // counted starts.
        //
        // Bound once and read twice, by the ladder and by the question a park
        // raises. A park that quoted a different number than the ladder decided
        // on would send an operator looking for a run that did not happen.
        let attempts_on_rung =
            position
                .1
                .saturating_add(u32::from(crate::ladder::spends_allowance(Some(
                    crate::ladder::FailureShape::of(failure),
                ))));

        let next = crate::ladder::next_step(
            failure,
            &crate::ladder::LadderState {
                rung: position.0 as usize,
                attempts_on_rung,
                defers,
                // Both halves, as `LadderState::resumable` documents: the
                // agent's CLI advertises `session_resume` and this attempt
                // actually returned a session to resume. Either alone closes
                // the generation and retries from a fresh one.
                resumable: plan.session_resume && assessed.outcome.session_id.is_some(),
            },
            &policy,
        );
        // **The question a park raises, built once here.** `settle_failed`
        // refuses a parking settlement that carries none, and
        // `rematerialize_question` reads it back out of the event on resume
        // rather than re-deciding it — so a question invented at the resume
        // would have to word itself identically, which is the duplication this
        // slice keeps paying for.
        let question = match next {
            crate::ladder::Next::AskHuman(kind) => {
                Some(self.park_question(site.key, attempts_on_rung, kind, failure, seams.ids)?)
            }
            _ => None,
        };

        let settled = settle_failed(
            &self.handle.fold,
            &FinishedAttempt {
                key: site.key,
                generation: site.generation,
                attempt: plan.attempt,
                record,
                next,
                session: assessed.outcome.session_id.clone().map(SessionId),
                question,
                halts_run: seams.halts_run,
                defers,
                reason: failure.reason.clone(),
                rung: position.0.saturating_add(1),
            },
        )?;

        // **The retaining incarnation's own note.** `T-RETAINED`'s resume action
        // is "the retaining incarnation proceeds to T-RETRY; a fresh process
        // closes it in recovery", so the tree a retry re-gates is legitimately
        // process-local: a different process may not use it, and the fold's
        // `RetainedIdle { incarnation }` is what refuses one that tries.
        //
        // The retained *tree* is per-generation where the brief above is
        // per-task: `Quiescence::HoldsTree` is a claim about a worktree this
        // process left in place, and only a `Retained` settlement leaves one.
        if matches!(
            settled.event.settlement,
            crate::topology::events::AttemptSettlement::Retained { .. }
        ) {
            self.retained.insert(
                site.key,
                Retained {
                    tree: capture.tree.clone(),
                },
            );
        }
        // The ceiling's ledger, before the append: `Spend::replay` rebuilds it
        // from the log on resume, so this keeps the in-process copy current for
        // the next iteration's ceiling check rather than being a second source
        // for it.
        self.spend.record(site.key, &settled.event.record);
        // **§11.4's brief, from the record being appended.** Not from the
        // `AttemptFailure` beside it: `Brief::replay` reads this event back off
        // the log, and applying the event as it will be read back is what makes
        // a live run and a replay of its own log one computation rather than two
        // that agree by inspection (§15, "one fold, not two").
        self.brief.record(site.key, &settled.event.record);
        self.emit(
            TopologyEventBody::AttemptFinished {
                data: Box::new(settled.event),
            },
            seams,
            hooks,
        )?;
        Ok(settled.spent_attempt)
    }

    /// The candidate sequence for an attempt nothing rejected.
    ///
    /// `side_effect_vs_event_ordering`: "commit object (R27) before pin
    /// (IdUnread between); pin before candidate_prepared; candidates ref after
    /// candidate_prepared and before task_candidate_created". The settlement
    /// lands **between the pin and `candidate_prepared`**, which is
    /// [`settle_succeeded`]'s own note: `T-CAND-OBJ`'s window covers the commit
    /// object and the pin with `authoritative_state: attempt unsettled`.
    ///
    /// INV-07's "candidate_prepared is the sole successful attempt settlement"
    /// is about which event records the *candidate*, not which settles the
    /// attempt: without `attempt_finished(Succeeded)` the generation never
    /// reaches `Promoting` and the fold refuses the `candidate_prepared` that
    /// follows.
    ///
    /// # Why `complete_promotion` was split to make this callable
    ///
    /// It took `hooks` **and** a journal. The driver's journal must hold
    /// `hooks` to emit through [`emit`], and one `&mut dyn TopologyHooks`
    /// cannot be both. The test journal sidesteps that by holding a *different*
    /// events bundle — the two-observation-surface divergence
    /// `reviews/FINDINGS.md` records — and production must not, because then
    /// the candidate sequence's appends would not be the ones the harness sees.
    ///
    /// So `complete_promotion` is now the composition of three halves that
    /// alternate between the two borrows, with its own typestate carrying the
    /// order. Its signature is unchanged and every existing caller still
    /// compiles.
    fn promote_candidate(
        &mut self,
        site: AttemptSite<'_>,
        plan: &AttemptPlan,
        capture: &Capture,
        record: AttemptRecord,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(), UpstrokeError> {
        let key = site.key;
        // **The region the diff actually touched**, read through the manager
        // rather than predicted: `lease_effect` is `ReplacesPredicted`, and the
        // whole point of that variant is that the prediction is replaced by the
        // measurement.
        let actual_paths = seams.manager.changed_paths(site.slot, &capture.parent)?;

        let judged = JudgedTree {
            key,
            generation: site.generation,
            attempt: Box::new(record.clone()),
            base_sha: site.base.clone(),
            tree_sha: CommitSha(capture.tree.clone()),
            message: format!(
                "upstroke: {} attempt {}",
                self.display_id(key)?,
                plan.attempt.0
            ),
            actual_paths,
            lease_effect: CandidateLeaseEffect::ReplacesPredicted {
                paths: seams.manager.changed_paths(site.slot, &capture.parent)?,
            },
        };

        let run_id = self.identity.run_id.clone();
        let unpinned = write_candidate_commit(seams.manager, hooks, &run_id, judged)?;
        let pinned = pin_candidate(seams.manager, hooks, unpinned)?;

        // Between the pin and `candidate_prepared`, and in that order.
        // **No `attempt_finished` here.** `candidate_prepared` is the sole
        // successful settlement for a candidate-producing attempt —
        // `decisions/2026-08-12-merge-queue-execution-topology.md`, which adds
        // "`attempt_finished` is not also emitted for that attempt" — and this
        // path appended both until the 2026-08-27 ruling. The fold now refuses
        // either half of that pair, so the omission is enforced rather than
        // remembered.
        //
        // The record still reaches the ledger: `append_candidate_prepared`
        // carries it on the event that settles the attempt, which is where the
        // record was always specified to live.
        self.spend.record(key, &record);
        // A succeeded record carries no failure and adds no brief line. Called
        // anyway so that `Brief::record`'s coverage is identical to
        // `Brief::replay`'s by construction rather than equal by argument —
        // replay walks every settlement, and this is one.
        self.brief.record(key, &record);

        // The three halves alternate between the hooks bundle and the journal,
        // and the journal must hold the bundle to emit through `emit`. So each
        // half runs with the borrow it needs and the typestate carries the
        // order between them: only `append_candidate_prepared` makes a
        // `PromotingCandidate`, only `create_candidates_ref` makes a
        // `ReferencedCandidate`, and only `append_candidate_created` makes a
        // `CreatedCandidate`.
        let promoting = self.with_journal(seams, hooks, |journal| {
            append_candidate_prepared(journal, pinned)
        })?;
        let referenced = create_candidates_ref(seams.manager, hooks, promoting)?;
        let created = self.with_journal(seams, hooks, |journal| {
            append_candidate_created(journal, referenced)
        })?;
        reclaim_after_creation(seams.manager, hooks, site.slot, created)?;
        Ok(())
    }

    /// Lend the run's state to the candidate sequence as a [`CandidateJournal`].
    ///
    /// A closure rather than a stored field because the journal borrows the
    /// fold, the log and the hooks, and the sequence's effect halves need the
    /// hooks back between appends.
    fn with_journal<T>(
        &mut self,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
        run: impl FnOnce(&mut RunJournal<'_, '_>) -> Result<T, UpstrokeError>,
    ) -> Result<T, UpstrokeError> {
        let mut journal = RunJournal {
            emitter: RunEmitter {
                identity: &self.identity,
                state: EmitState {
                    fold: &mut self.handle.fold,
                    log: &mut self.handle.log,
                    reservations: &mut self.reservations,
                    warnings: &mut self.warnings,
                },
                clock: seams.clock,
            },
            hooks,
            invocations: &mut self.invocations,
        };
        run(&mut journal)
    }

    /// The display id the frozen registry gave this task.
    fn display_id(&self, key: TaskKey) -> Result<String, UpstrokeError> {
        Ok(self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.get(key))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's frozen registry", key.index()),
            })?
            .display_id
            .as_str()
            .to_owned())
    }

    /// The question a parked attempt raises.
    ///
    /// Every word of it comes from the legacy authorities:
    /// `coordinator::question_context` for the prose the human reads and
    /// `coordinator::question_options` for what they can answer. The driver
    /// supplies only what the frozen registry knows — the display id, the
    /// title, the acceptance list — and what this branch knows about the
    /// attempt.
    ///
    /// **`attempts` is the task's spend on this rung, from the fold.** It was
    /// `plan.attempt` — this generation's attempt number — which restarts at
    /// one for a same-rung retry that did not resume, so a park after two
    /// attempts told the human "1 attempt(s)".
    ///
    /// **And `rungs_spent` was the literal `1`**, with a comment saying a
    /// multi-rung count was "owed with the escalation lane" — while this slice
    /// claimed that lane delivered. A two-rung chain at `attempts_per = 1` that
    /// exhausts told the operator *"1 attempt(s) across 1 rung(s) all failed"*
    /// when two attempts across two rungs had. The frontier review of
    /// `75da796`, finding 3.
    ///
    /// **Both numbers now come from the fold, and both match the legacy
    /// authority's rule.** `coordinator::ParkSubject::of` — the schema-3 path
    /// that ships today — counts `attempts` as the task's total across every
    /// record and `rungs_spent` as the number of *distinct tiers* those records
    /// name, floored at one. Here:
    ///
    /// * total attempts is the sum of every generation's `attempts`, because
    ///   `GenerationFold::attempts` is the highest attempt number started in
    ///   that generation and the number restarts at each new generation — so a
    ///   single generation's count is what the old code passed, and the sum is
    ///   the task's;
    /// * rungs spent is `task.rung + 1`, floored at one. Rungs are dense and
    ///   monotonic — `settle`'s escalation moves a task to `rung + 1` and
    ///   nothing moves it back — so the count of distinct **rungs** a task has
    ///   occupied *is* its current rung plus one.
    ///
    /// **It is not always the same number the legacy path computes, and this
    /// used to claim it was.** `ParkSubject::of` counts distinct *tiers* with a
    /// `BTreeSet`; a rung is an index into the chain, and `ChainSummary.tiers`
    /// is a `Vec<Tier>` that nothing deduplicates. For a chain naming the same
    /// tier twice — `chain = ["small", "small"]`, which config accepts — a task
    /// that exhausts both rungs is 2 here and 1 there. The 2026-08-26 re-review
    /// of `c2c0294` found the claim, finding C.
    ///
    /// **The two are answering different questions and this one is the right
    /// one for the sentence it builds.** The question is "how much of your
    /// ladder did this task spend before it asked for you", and a chain with a
    /// repeated tier really does have two rungs to spend: the task ran twice,
    /// at two allowances, and telling the operator "1 rung" would understate it
    /// in exactly the direction finding 3 was about. The legacy count is the
    /// one that is wrong for such a chain; it is not changed here because that
    /// is the shipped path and a question's wording is not worth a behaviour
    /// change to it.
    ///
    /// Held by `recover::tests::the_driver_escalates_onto_the_rung_above`,
    /// which asserts the sentence for a two-tier chain.
    fn park_question(
        &self,
        key: TaskKey,
        attempts_on_rung: u32,
        kind: crate::ir::QuestionKind,
        failure: &crate::ladder::AttemptFailure,
        ids: &dyn IdSource,
    ) -> Result<FrozenQuestion, UpstrokeError> {
        let entry = self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.get(key))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's frozen registry", key.index()),
            })?;
        let task = self
            .handle
            .fold
            .task(key)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's fold", key.index()),
            })?;
        let total_attempts: u32 = task
            .generations
            .iter()
            .map(|generation| generation.attempts)
            .sum();
        let rungs_spent = (task.rung as usize).saturating_add(1).max(1);
        // `attempts_on_rung` stays a parameter: it is what the caller has in
        // hand at the park, and dropping it would let a future caller pass a
        // number nothing reads. It is deliberately not what the human is told.
        let _ = attempts_on_rung;
        Ok(FrozenQuestion {
            id: ids.question_id(),
            key,
            kind,
            context: crate::engine::coordinator::question_context(
                crate::engine::coordinator::ParkSubject {
                    display_id: entry.display_id.as_str(),
                    title: &entry.spec.title,
                    acceptance: &entry.spec.acceptance,
                    // The task's **total** attempts and the rungs they
                    // spent, both derived from the fold, because that is what
                    // the sentence the operator reads says: "N attempt(s)
                    // across M rung(s) all failed". `attempts_on_rung` is this
                    // rung's spend and is what the ladder reads; it is not what
                    // the human is told.
                    attempts: total_attempts,
                    rungs_spent,
                },
                kind,
                failure,
            ),
            options: crate::engine::coordinator::question_options(kind),
        })
    }

    /// Where this task stands on its ladder: the rung its next attempt runs
    /// at, and the attempts already spent there.
    ///
    /// **Both from the fold, because a ladder position survives a resume.** A
    /// settlement that escalates closes the generation and leaves the task
    /// `Pending`, so this branch selects it again — and a driver that assumed
    /// rung 0 would dispatch it at rung 0 forever, never reaching the tier its
    /// chain escalated it to. A driver that assumed attempt 1 would hand
    /// `next_step` the first attempt of the allowance every time, so the task
    /// would retry forever and never escalate at all. Both were true of this
    /// driver until `PR7-FOLD-LADDER-POSITION`.
    fn ladder_position(&self, key: TaskKey) -> Result<(u32, u32), UpstrokeError> {
        let task = self
            .handle
            .fold
            .task(key)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's fold", key.index()),
            })?;
        Ok((task.rung, task.attempts_on_rung))
    }

    /// How many times this task has already settled `Deferred`.
    ///
    /// **The fold's count, not a tally this process kept.** `next_step` reads
    /// it on the outage branch alone, and a process-local number agrees with
    /// the log on every reading except the one after a resume — where it reads
    /// zero while the log holds the deferrals, and the run defers past its
    /// allowance forever.
    /// `fold::tests::a_deferral_count_is_derived_by_replay_and_not_by_a_process_local_tally`
    /// is the witness for that property.
    ///
    /// **Witnessed at this level too.** The value is load-bearing only on the
    /// outage branch — `next_step` ignores it otherwise, and so does
    /// `settle_failed`, which reads `FinishedAttempt::defers` only to build
    /// `SettlementTransition::Deferred` — so replacing this expression with a
    /// constant zero once left the whole suite green.
    /// `the_driver_settles_an_outage_from_the_folds_deferral_count` closes
    /// that: it seeds one deferral in the log before the run, so the settlement
    /// must record `defers: 2`, and the constant-zero mutation records `1`.
    /// A prior deferral is what makes the read observable at all.
    fn deferrals_recorded(&self, key: TaskKey) -> Result<u32, UpstrokeError> {
        Ok(self
            .handle
            .fold
            .task(key)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's fold", key.index()),
            })?
            .defers)
    }

    /// The frozen ladder's shape, as `next_step` reads it.
    ///
    /// Every field from a record the run already froze: the entry's own ladder
    /// for the allowance and the rung count, and `run_started(4).limits` for the
    /// deferral ceiling. None of it is re-derived.
    fn ladder_policy(&self, key: TaskKey) -> Result<crate::ladder::LadderPolicy, UpstrokeError> {
        let entry = self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.get(key))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's frozen registry", key.index()),
            })?;
        let limits = self
            .handle
            .fold
            .started()
            .ok_or_else(|| UpstrokeError::Refused {
                message: "the run has not started".to_owned(),
            })?
            .limits;
        Ok(crate::ladder::LadderPolicy {
            attempts_per: entry.ladder.attempts_per,
            rungs: entry.ladder.rungs.len(),
            max_defers: limits.max_defers,
        })
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
        // **The fold's answer, not a second one.** `dispatch_lease_check`
        // admitted this task by computing the region and asking the lease table
        // what it overlaps; recording a different region here would leave the
        // fold admitting on one answer and the log holding another, and the
        // log's is the one the lease table keeps. This module derived it
        // independently for exactly one commit, and took the plan's hints
        // literally where the fold strips globs to a literal prefix — so a hint
        // of `src/auth/*.rs` became a prefix that overlaps nothing while the
        // fold had admitted on `src/auth`.
        let paths = self.handle.fold.predicted_region(key).ok_or_else(|| {
            UpstrokeError::Refused {
                message: format!(
                    "the fold selected task {} for dispatch and the frozen registry has no such \
                     entry; the two disagree and nothing is dispatched",
                    key.0
                ),
            }
        })?;
        Ok(DispatchRequest {
            key,
            generation,
            base: self.handle.started.base_sha.clone(),
            kind: DispatchKind::Ordinary { paths },
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
                warnings: &mut self.warnings,
            },
            clock: seams.clock,
        };
        // The driver owns the ledger, so obligation (3) is discharged here
        // and the loop keeps one error type. `emitter` borrows the fold, the
        // log, the reservations and the warnings; `invocations` is a disjoint
        // field, which is the whole reason it is no longer inside `EmitState`.
        emitter
            .emit(body, hooks)
            .map_err(|failure| failure.discharging(&mut self.invocations))
    }
}

#[cfg(test)]
mod tests;
