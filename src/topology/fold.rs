//! The checked fold: one transition function for a live run and for a replay.
//!
//! **INV-02 — an invalid transition is never appended, and never applied.**
//! [`TopologyFold::plan_transition`] decides whether an event may be applied
//! and returns a [`TopologyDelta`] when it may; [`TopologyFold::apply_delta`]
//! is the only thing that changes the state, and a `TopologyDelta` is the only
//! thing it accepts. The delta has no public constructor, so there is no way to
//! reach the state except through the check — which is what makes "the live run
//! and the replay use one transition" a property of the types rather than a
//! convention two call sites are expected to keep.
//!
//! A live emission is `plan_transition` → append the exact bytes → `apply_delta`
//! only after the append returned `Ok`. A replay is
//! [`TopologyFold::replay`], which is those same two calls per event with the
//! append taken out. Nothing else exists.
//!
//! # What the fold refuses
//!
//! Everything in `decisions.schema_compatibility.refusals`, less the four the
//! header probe answers before a fold exists ([`crate::topology::schema`]).
//! The refusals are not a validation pass bolted onto a fold: they *are* the
//! fold, because a transition this module cannot state the effect of is a
//! transition it must not pretend to have applied.
//!
//! Three of them are worth naming here because they are relations rather than
//! shapes, and a reader looking for them in one event will not find them:
//!
//! * **The publication relations** (INV-09). A `merge_prepared` is checked
//!   against the candidate's own record, the pinned proposal, and the head the
//!   verification read — three records elsewhere in the log.
//! * **The derived outcome** (INV-15). `run_finished` carries an outcome, and
//!   the fold accepts it only when it equals [`TopologyFold::derived_outcome`],
//!   which is computed from durable state alone and never consults spend,
//!   capacity, or runner availability.
//! * **Queue order** (`decisions.coordinator_integration.queue`). An
//!   integration may only start for the first *eligible* candidate, which is
//!   not the same as the first queued one.
//!
//! # What it does not do
//!
//! No production path writes or reads a schema-4 log yet, and nothing here
//! performs an effect: no ref moves, no worktree is created, no report is
//! written. The fold decides what a log *means*; the effects that log
//! authorizes, and the typed sites they run through, arrive in later slices.

mod apply;
mod check_attempt;
mod check_candidate;
mod check_end;
mod check_integration;
mod outcome;
mod parse;
mod predicates;
mod region;
mod start;

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::events::RunOutcome;
use crate::ir::{Plan, QuestionId, Tier};
use crate::topology::events::{
    Answer4, AttemptFinished4, AttemptInterrupted4, AttemptNumber, AttemptSettlement,
    AttemptStarted4, BindingOverride, BudgetExceeded4, BudgetStop, CandidateLeaseEffect,
    CandidatePrepared, CandidateRef, CommitSha, DerivedOutcome, Epoch, FrozenQuestion, FrozenSpawn,
    GenerationClosed, GenerationId, GitRef, IncarnationId, LeaseDisposition, LeaseGrant,
    MergeLeaseRelease, MergePrepared, MergeRejected, MergeVerificationInterrupted,
    MergeVerificationStarted, MergeVerificationUnavailable, PreparedDisposition, QuestionAnswered4,
    RejectionDisposition, RejectionLeaseEffect, RunFinished4, RunResumed4, RunStarted4,
    RungBinding, SequenceId, SessionId, SettlementTransition, SpawnAdmission, TaskCandidateCreated,
    TaskDispatched, TaskMerged, TopologyEvent, TopologyEventBody, UnavailableCause,
    UnavailableOutcome, VerificationBasis, VerificationSource, VerificationVerdict,
};
use crate::topology::leases::{GenerationLease, LeaseOwner, LeaseTable};
use crate::topology::paths::{GitPath, PathSet};
use crate::topology::queue::{CandidateQueue, Ineligible, QueueEntry};
use crate::topology::registry::{Admission, FrozenLadder, TaskEntry, TaskKey, TaskRegistry};

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Why a transition was refused.
///
/// Every message names the record it refused and the value it disagreed with,
/// because a fold error reaches an operator as "your log is invalid" unless it
/// says which line and which field.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FoldError {
    #[error(
        "a `{kind}` arrived before this log's `run_started`; the first event of a topology log \
         records the registry, the runner and the limits every later event is checked against"
    )]
    NotStarted { kind: &'static str },

    #[error(
        "a second `run_started` in one log; a run begins once, and a second beginning would \
         replace the registry every event so far was folded against"
    )]
    AlreadyStarted,

    #[error(
        "this `run_started` records event schema {schema}, not the topology schema; a log that \
         does not say it is a topology run cannot be folded as one"
    )]
    NotTopologySchema { schema: u32 },

    #[error("the run's runner record is unusable: {defect}")]
    IncompleteRunner { defect: String },

    #[error(
        "this incarnation established a different runner from the one the run started with: the \
         {field} differs. A run's confinement boundary and image are fixed for its life."
    )]
    RunnerMoved { field: String },

    #[error(
        "the recorded {what} digest `{recorded}` does not match `{actual}`, derived here from the \
         frozen inputs; the plan or the run record moved underneath the log"
    )]
    DigestMismatch {
        what: &'static str,
        recorded: String,
        actual: String,
    },

    #[error("the registry could not be rebuilt from the frozen plan and this record: {detail}")]
    RegistryUnbuildable { detail: String },

    #[error(
        "task {key}'s frozen ladder is malformed: {defect}. A ladder is frozen at registration \
         and every later attempt is checked against it, so one that cannot be escalated through \
         is refused before it is stored rather than when it is climbed."
    )]
    MalformedLadder { key: u32, defect: String },

    #[error("`{kind}` names task {key}, which this run has no entry for")]
    UnknownKey { kind: &'static str, key: u32 },

    #[error(
        "`{kind}` registers key {key}, but the registry holds {len} entries; a key is the next \
         dense index at the event that registers it"
    )]
    NonDenseKey {
        kind: &'static str,
        key: u32,
        len: usize,
    },

    #[error("`{kind}` for task {key} is inconsistent with what a registered entry is: {detail}")]
    MalformedEntry {
        kind: &'static str,
        key: u32,
        detail: String,
    },

    #[error(
        "task {key} is `{state}`, and `{kind}` applies to a task that is `{expected}`; the fold \
         holds one state per task and this event would apply to another run's"
    )]
    WrongTaskState {
        kind: &'static str,
        key: u32,
        state: &'static str,
        expected: &'static str,
    },

    #[error(
        "`{kind}` names generation {generation} of task {key}, which is not the open one \
         ({detail}); a completion applies only while its identity is the current open one"
    )]
    NotTheOpenGeneration {
        kind: &'static str,
        key: u32,
        generation: u32,
        detail: String,
    },

    #[error(
        "`{kind}` names attempt {attempt} of task {key} generation {generation}, and the open \
         attempt is {expected}"
    )]
    WrongAttempt {
        kind: &'static str,
        key: u32,
        generation: u32,
        attempt: u32,
        expected: String,
    },

    #[error(
        "`{kind}` parks task {key} while its generation {generation} is {class}; a question is \
         raised against a task with no open generation, because its answer returns the task to \
         pending and a decline fails it, and neither settles an attempt — a generation left open \
         under a failed task could never close, and the run could never end. A question that \
         arises from an attempt is carried by that attempt's settlement."
    )]
    GenerationOpen {
        kind: &'static str,
        key: u32,
        generation: u32,
        class: &'static str,
    },

    #[error(
        "attempt {attempt} of task {key} resumes a session this incarnation may not resume: \
         {detail}. A session belongs to the process that retained it."
    )]
    StaleIncarnation {
        key: u32,
        attempt: u32,
        detail: String,
    },

    #[error(
        "attempt {attempt} of task {key} runs a binding the run never froze for it: {detail}. \
         Run-start exact bindings are execution identity."
    )]
    BindingMismatch {
        key: u32,
        attempt: u32,
        detail: String,
    },

    #[error(
        "`{kind}` for task {key} records the lease disposition `{recorded}`, and a {owner} \
         generation that {fate} records `{expected}`"
    )]
    InvalidLeaseDisposition {
        kind: &'static str,
        key: u32,
        recorded: String,
        owner: &'static str,
        fate: &'static str,
        expected: String,
    },

    #[error(
        "`{kind}` opens integration sequence {sequence}, and this run has consumed {next}; \
         sequences are dense from 0 across the run"
    )]
    NonDenseSequence {
        kind: &'static str,
        sequence: u32,
        next: u32,
    },

    #[error(
        "`{kind}` names integration sequence {sequence}, and the open transaction is {open}; an \
         event applies to the transaction it belongs to or to none"
    )]
    WrongSequence {
        kind: &'static str,
        sequence: u32,
        open: String,
    },

    #[error(
        "`{kind}` opens integration sequence {sequence} while sequence {open} is unresolved; one \
         integration transaction runs at a time"
    )]
    TransactionAlreadyOpen {
        kind: &'static str,
        sequence: u32,
        open: u32,
    },

    #[error(
        "`{kind}` starts an integration for task {key} generation {generation}, which is not the \
         first eligible candidate in the queue ({detail})"
    )]
    NotFirstEligible {
        kind: &'static str,
        key: u32,
        generation: u32,
        detail: String,
    },

    #[error("`{kind}` disagrees with the record it cites: {detail}")]
    InconsistentRecord { kind: &'static str, detail: String },

    #[error(
        "`{kind}` settles {recorded:?}, and the fold derives {derived:?} as this publication's \
         closure"
    )]
    InvalidSatisfies {
        kind: &'static str,
        recorded: Vec<u32>,
        derived: Vec<u32>,
    },

    #[error(
        "a verification outage records {defers} deferral(s) for this candidate, and {detail}; an \
         outage that has waited its allowance parks for a human instead"
    )]
    InvalidDefers { defers: u32, detail: String },

    #[error("`{kind}` carries a question that cannot be answered: {detail}")]
    UnanswerableQuestion { kind: &'static str, detail: String },

    #[error("`{kind}` names question `{question}`, which {detail}")]
    WrongQuestion {
        kind: &'static str,
        question: String,
        detail: String,
    },

    #[error(
        "`{kind}` arrived after {what} in this epoch; the run has stopped admitting work and an \
         answer ingested now would restart a run that already ended"
    )]
    RunEnding {
        kind: &'static str,
        what: &'static str,
    },

    #[error(
        "`{kind}` continues a run that finished `{outcome}`; a {outcome} run is terminal — it is \
         finalized and then refused, never continued"
    )]
    RunIsOver {
        kind: &'static str,
        outcome: &'static str,
    },

    #[error(
        "`run_finished` records `{recorded}`, and the outcome derived from durable state is \
         {derived}; a run ends at the outcome its state implies or not at all"
    )]
    OutcomeMismatch {
        recorded: &'static str,
        derived: String,
    },

    #[error(
        "the fold is poisoned by an append whose outcome is unknown; this process appends nothing \
         further and derives nothing further from memory — the state is re-derived only by reopen \
         and the stable-prefix barrier"
    )]
    Poisoned,

    #[error(
        "line {line} of the log is newline-terminated and is not a valid event ({detail}). This \
         is not a torn tail — the line was committed, so the log has been rewritten, and state \
         derived from what is left would be confidently wrong."
    )]
    RewrittenLog { line: usize, detail: String },
}

// ---------------------------------------------------------------------------
// Fold state
// ---------------------------------------------------------------------------

/// What a task is doing, as the log says.
///
/// The topology's own states, not [`crate::events::TaskState`]: a task with an
/// open generation is `Pending` here and is kept out of admission by the
/// generation rather than by a state of its own, because the thing that has to
/// be closed before the run may end is the generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Runnable once its dependencies are merged and nothing else holds it.
    Pending,
    /// A candidate exists and is queued for integration.
    AwaitingMerge,
    /// Its candidate was rejected and a repair carries it.
    AwaitingRepair,
    /// Parked on a question.
    AwaitingInput,
    /// Backing off after an outage, until `defer_wait_elapsed` or a resume.
    Deferred,
    /// Its work is in the integration ref.
    Merged,
    /// Terminal.
    Failed,
}

impl TaskState {
    fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::AwaitingMerge => "awaiting merge",
            Self::AwaitingRepair => "awaiting repair",
            Self::AwaitingInput => "awaiting input",
            Self::Deferred => "deferred",
            Self::Merged => "merged",
            Self::Failed => "failed",
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Merged | Self::Failed)
    }
}

/// Where one generation of one task is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationClass {
    /// Dispatched; no attempt has started.
    OpenNoAttempt,
    /// An attempt is running.
    InFlight { attempt: AttemptNumber },
    /// Settled holding a session, for a same-session retry by the incarnation
    /// that retained it.
    RetainedIdle {
        session: SessionId,
        incarnation: Epoch,
    },
    /// An attempt succeeded; the candidate is being promoted to its
    /// authoritative ref.
    Promoting,
    /// Over.
    Closed,
}

impl GenerationClass {
    fn name(&self) -> &'static str {
        match self {
            Self::OpenNoAttempt => "open with no attempt",
            Self::InFlight { .. } => "in flight",
            Self::RetainedIdle { .. } => "retained idle",
            Self::Promoting => "promoting",
            Self::Closed => "closed",
        }
    }

    /// Whether this generation holds a pipeline entitlement.
    fn holds_pipeline(&self) -> bool {
        matches!(
            self,
            Self::OpenNoAttempt | Self::InFlight { .. } | Self::Promoting
        )
    }

    /// Whether the run may end while this generation is in this class.
    fn blocks_run_end(&self) -> bool {
        !matches!(self, Self::Closed)
    }
}

/// One generation of one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationFold {
    pub id: GenerationId,
    pub class: GenerationClass,
    /// The commit the worktree was created at.
    pub base_sha: CommitSha,
    pub lease: GenerationLease,
    /// The highest attempt number started in this generation.
    pub attempts: u32,
    /// The candidate this generation prepared, once it has.
    pub candidate: Option<PreparedCandidate>,
}

/// What `candidate_prepared` recorded, kept for the relations a publication is
/// checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCandidate {
    pub candidate: CandidateRef,
    /// The base the work started from, and the parent of the commit.
    pub base_sha: CommitSha,
    /// The tree the gates ran against and the reviewers judged.
    ///
    /// **Retained because adoption is about identity, not existence.**
    /// `DESIGN.md` §15 requires `candidate_prepared` to record "exactly one
    /// complete attempt/base/commit/tree identity ... so resume adopts only
    /// that exact shape". The tree was on the event and stopped here: recovery
    /// could check that the object exists and that its parent is the recorded
    /// base, and a commit with that parent and a **different tree** passed —
    /// so a resume could publish an object no gate ran against and no reviewer
    /// read. `candidate.rs`'s own comment recorded the gap rather than closing
    /// it, because closing it is this field.
    ///
    /// Per-instance **Class B** approval, granted 2026-08-26 against the
    /// frontier re-review of `c2c0294`, finding B; the ledger row is
    /// `reviews/FINDINGS.md` §3 and `PR7-CANDIDATE-TREE-UNVERIFIED` in §2.
    /// Nothing serde-visible moves — `CandidatePrepared::tree_sha` already
    /// exists on the wire and this is the fold keeping what it reads. It
    /// conforms to §15 rather than amending it.
    pub tree_sha: CommitSha,
    pub paths: PathSet,
}

/// One task's fold state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFold {
    pub state: TaskState,
    /// How many times an attempt on this task has settled `Deferred`.
    ///
    /// **The fold owns this count because only the fold survives a resume.**
    /// `ladder::next_step` reads it on exactly one branch — an outage defers
    /// while `defers < max_defers` and parks at it — and a driver keeping its
    /// own tally would restart at zero in the next process while the log still
    /// held the deferrals, so a run that had already exhausted its allowance
    /// would defer forever. The legacy engine keeps it in
    /// `state.progress[index].defers`, which is in-memory schema-3 state; a
    /// schema-4 run derives everything by replay, so this is derived by replay.
    ///
    /// Read through the existing [`TopologyFold::task`] reader. It is a field
    /// rather than a twelfth reader for that reason.
    ///
    /// `max_defers` is **not** here: the ceiling is policy and stays in
    /// `ladder::LadderPolicy`, read from `run_started(4).limits`. This is the
    /// count, and only the count.
    pub defers: u32,
    /// The rung this task's **next** attempt runs at.
    ///
    /// **The fold owns it because a task's ladder position survives a resume.**
    /// A settlement that escalates closes the generation and leaves the task
    /// `Pending`, so the ready-dispatch branch selects it again — at a rung the
    /// driver has no other way to know. A driver-side tally reads zero in the
    /// next process while the log holds the escalation, so the task is
    /// dispatched on rung 0 forever and never reaches the tier its chain
    /// escalated it to.
    ///
    /// `SettlementTransition::Escalated { rung }` is the durable answer — the
    /// packet defines it as the rung an escalation climbs *onto* — so this is
    /// assigned from it, never computed.
    pub rung: u32,
    /// Attempts already spent at [`Self::rung`].
    ///
    /// Not `GenerationFold::attempts`: that counts one generation, and attempts
    /// at one rung span generations — a same-rung retry that does not resume
    /// closes its generation and opens a fresh one at the same rung. Feeding
    /// `LadderState::attempts_on_rung` the per-generation count makes
    /// `next_step` see the first attempt of the allowance every time, so a task
    /// retries forever and never escalates.
    ///
    /// Reset by an escalation, because the allowance is per rung.
    pub attempts_on_rung: u32,
    pub generations: Vec<GenerationFold>,
}

impl TaskFold {
    fn new() -> Self {
        Self {
            state: TaskState::Pending,
            defers: 0,
            rung: 0,
            attempts_on_rung: 0,
            generations: Vec::new(),
        }
    }

    /// The generation that is not closed, if any. At most one exists: a new one
    /// is only opened when the previous closed.
    fn open(&self) -> Option<&GenerationFold> {
        self.generations
            .iter()
            .find(|generation| generation.class != GenerationClass::Closed)
    }

    fn open_mut(&mut self) -> Option<&mut GenerationFold> {
        self.generations
            .iter_mut()
            .find(|generation| generation.class != GenerationClass::Closed)
    }
}

/// Why a question is open, which is what decides where its answer returns the
/// task to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionOrigin {
    /// A verification could not be run. An answer returns the task to awaiting
    /// merge, to be re-verified under a new sequence.
    VerificationPark,
    /// An attempt parked, or a repair's admission is gated. An answer returns
    /// the task to pending.
    Admission,
}

/// An open question and what raised it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenQuestion {
    pub question: FrozenQuestion,
    pub origin: QuestionOrigin,
    /// The frozen binding options this question's admission authorized, for a
    /// `HumanBinding` admission and for nothing else.
    ///
    /// `decisions.task_registry.binding_override` validates an override
    /// "against the frozen options of that task's open `HumanBinding`
    /// question", so the authority has to survive from the `task_spawned` that
    /// froze it to the `question_answered` that draws on it. Kept here rather
    /// than re-read from the registry entry because it is the *question's*
    /// authority: two questions of one task are answered separately and only
    /// one of them ever authorized a binding.
    pub binding: Option<Vec<String>>,
}

/// Where an integration transaction is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionClass {
    /// A verification is running against a recorded head.
    VerificationStarted {
        basis: VerificationBasis,
        expected_head: CommitSha,
        proposed_sha: CommitSha,
    },
    /// The publication is authorized and the ref move is owed.
    Prepared {
        proposed_sha: CommitSha,
        satisfies: Vec<TaskKey>,
    },
}

/// The one unresolved integration transaction, if there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub sequence: SequenceId,
    pub candidate: CandidateRef,
    pub class: TransactionClass,
}

/// Everything one topology run has recorded.
///
/// `PartialEq` and not `Eq`: the run record it holds carries the reported
/// spend of a budget stop, and a float has no total equality. Comparing two of
/// these is how a live fold and a replayed one are proved identical (INV-02).
#[derive(Debug, Clone, PartialEq)]
pub struct RunState {
    started: Box<RunStarted4>,
    registry: TaskRegistry,
    tasks: Vec<TaskFold>,
    epoch: Epoch,
    incarnation: IncarnationId,
    questions: BTreeMap<QuestionId, OpenQuestion>,
    /// Every question id this log has used, open or not: an id is never reused.
    seen_questions: BTreeSet<QuestionId>,
    overrides: BTreeMap<TaskKey, BindingOverride>,
    queue: CandidateQueue,
    leases: LeaseTable,
    transaction: Option<Transaction>,
    next_sequence: u32,
    halted_at: Option<TaskKey>,
    /// The epoch the halting settlement was recorded in. `halted_at` is never
    /// cleared, and the answer-ingestion refusal is epoch-scoped.
    halted_epoch: Option<Epoch>,
    budget_stop: Option<BudgetStop>,
    finished: Option<RunOutcome>,
}

/// The frozen inputs a fold is derived against.
///
/// Both are read before the first event: the plan the run normalized, and the
/// digest of the exact bytes it was normalized to. The fold rebuilds the
/// registry from the plan and refuses a `run_started` whose recorded digests do
/// not match, which is the whole of `refusals[4]` — a plan that moved
/// underneath a log is refused rather than folded on a guess.
#[derive(Debug, Clone)]
pub struct FrozenInputs {
    pub plan: Plan,
    /// Digest of the exact `plan.normalized.json` bytes, in the
    /// `sha256:<hex>` shape the registry digest uses.
    pub normalized_plan_digest: String,
}

/// One checked transition, ready to apply.
///
/// Deliberately opaque and deliberately unconstructible outside this module:
/// [`TopologyFold::apply_delta`] takes one of these and nothing else, so the
/// only path into the state runs through [`TopologyFold::plan_transition`].
/// That is INV-02 expressed as a type rather than as a rule two call sites are
/// asked to remember.
#[derive(Debug, Clone, PartialEq)]
pub struct TopologyDelta {
    event: TopologyEvent,
    derived: Derived,
}

impl TopologyDelta {
    /// The event this delta applies. Readable so a caller can append the exact
    /// bytes it checked.
    pub fn event(&self) -> &TopologyEvent {
        &self.event
    }
}

/// What the check derived and the application would otherwise have to look up
/// again.
#[derive(Debug, Clone, PartialEq)]
enum Derived {
    None,
    /// The registry rebuilt from the frozen plan and this record, already
    /// authenticated against the recorded digest.
    Registry(Box<TaskRegistry>),
    /// Where an answered question returns its task to.
    Answer(QuestionOrigin),
}

/// The state of one topology run, and the only way to change it.
#[derive(Debug, Clone)]
pub struct TopologyFold {
    inputs: FrozenInputs,
    run: Option<RunState>,
    poisoned: bool,
}

impl TopologyFold {
    /// A fold over a run that has recorded nothing yet.
    pub fn new(inputs: FrozenInputs) -> Self {
        Self {
            inputs,
            run: None,
            poisoned: false,
        }
    }

    /// Fold `events` from nothing, refusing the first transition that does not
    /// apply.
    ///
    /// This *is* the live path with the append removed: one `plan_transition`
    /// and one `apply_delta` per event, in order. There is no second reader.
    ///
    /// # Errors
    ///
    /// The [`FoldError`] of the first event that does not apply.
    pub fn replay(inputs: FrozenInputs, events: &[TopologyEvent]) -> Result<Self, FoldError> {
        let mut fold = Self::new(inputs);
        for event in events {
            let delta = fold.plan_transition(event)?;
            fold.apply_delta(delta);
        }
        Ok(fold)
    }

    // -----------------------------------------------------------------------
    // The transition
    // -----------------------------------------------------------------------

    /// Whether `event` may be applied to this state, and what applying it does.
    ///
    /// # Errors
    ///
    /// The [`FoldError`] naming what the event disagrees with. A refusal is a
    /// statement about the pair — this event against this state — and never a
    /// statement that the event is malformed in isolation, which is
    /// serialization's business.
    pub fn plan_transition(&self, event: &TopologyEvent) -> Result<TopologyDelta, FoldError> {
        // refusals[24]: a process whose fold is poisoned by a returned append
        // error attempts no further transition. The command has already ended.
        if self.poisoned {
            return Err(FoldError::Poisoned);
        }
        let kind = event.body.kind();
        match &event.body {
            TopologyEventBody::RunStarted { data } => {
                let registry = self.check_run_started(data)?;
                Ok(self.delta(event, Derived::Registry(Box::new(registry))))
            }
            _ => {
                let run = self.run.as_ref().ok_or(FoldError::NotStarted { kind })?;
                self.check_started_run(run, event, kind)
            }
        }
    }

    /// Apply a checked transition. Total: every value it needs was decided by
    /// the check that produced the delta.
    pub fn apply_delta(&mut self, delta: TopologyDelta) {
        let TopologyDelta { event, derived } = delta;
        if let (TopologyEventBody::RunStarted { data }, Derived::Registry(registry)) =
            (&event.body, &derived)
        {
            self.run = Some(RunState::start(data.clone(), (**registry).clone()));
            return;
        }
        let Some(run) = self.run.as_mut() else {
            return;
        };
        run.apply(&event.body, &derived);
    }

    fn delta(&self, event: &TopologyEvent, derived: Derived) -> TopologyDelta {
        TopologyDelta {
            event: event.clone(),
            derived,
        }
    }
}

// ---------------------------------------------------------------------------
// RunState: the checks
// ---------------------------------------------------------------------------

impl RunState {
    fn start(started: Box<RunStarted4>, registry: TaskRegistry) -> Self {
        let tasks = (0..registry.len()).map(|_| TaskFold::new()).collect();
        let incarnation = started.incarnation.clone();
        Self {
            started,
            registry,
            tasks,
            epoch: Epoch(0),
            incarnation,
            questions: BTreeMap::new(),
            seen_questions: BTreeSet::new(),
            overrides: BTreeMap::new(),
            queue: CandidateQueue::new(),
            leases: LeaseTable::new(),
            transaction: None,
            next_sequence: 0,
            halted_at: None,
            halted_epoch: None,
            budget_stop: None,
            finished: None,
        }
    }

    fn entry(&self, kind: &'static str, key: TaskKey) -> Result<&TaskEntry, FoldError> {
        self.registry
            .get(key)
            .ok_or(FoldError::UnknownKey { kind, key: key.0 })
    }

    fn task(&self, kind: &'static str, key: TaskKey) -> Result<&TaskFold, FoldError> {
        self.tasks
            .get(key.index())
            .ok_or(FoldError::UnknownKey { kind, key: key.0 })
    }

    /// The pipeline entitlement this state holds.
    fn pipeline_held(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| {
                task.open()
                    .is_some_and(|generation| generation.class.holds_pipeline())
            })
            .count()
            + usize::from(self.transaction.is_some())
    }

    fn run_is_ending(&self) -> bool {
        self.halted_at.is_some() || self.budget_stop_is_current()
    }

    fn budget_stop_is_current(&self) -> bool {
        self.budget_stop
            .is_some_and(|stop| stop.epoch == self.epoch)
    }

    fn open_question_for(&self, key: TaskKey) -> Option<&OpenQuestion> {
        self.questions
            .values()
            .find(|open| open.question.key == key)
    }
}

/// The region an ordinary dispatch of this entry would predict.
///
/// The plan's path hints, taken literally: a hint with no glob metacharacter is
/// its own literal prefix. Anything else — an absent hint list, or a hint whose
/// literal prefix is empty — classifies repo-wide, which overlaps everything.
fn predicted_region(entry: &TaskEntry) -> PathSet {
    if entry.spec.path_hints.is_empty() {
        return PathSet::RepoWide;
    }
    let mut paths = Vec::with_capacity(entry.spec.path_hints.len());
    for hint in &entry.spec.path_hints {
        let literal: String = hint
            .replace('\\', "/")
            .chars()
            .take_while(|character| !matches!(character, '*' | '?' | '[' | '{'))
            .collect();
        let trimmed = literal.trim_end_matches('/');
        if trimmed.is_empty() {
            return PathSet::RepoWide;
        }
        paths.push(GitPath(trimmed.to_owned()));
    }
    PathSet::Prefixes { paths }
}

#[cfg(test)]
mod tests;
