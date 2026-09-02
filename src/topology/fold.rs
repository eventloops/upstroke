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

    /// Every committed line of a topology log, in order.
    ///
    /// The newline is the commit marker: an unterminated final line is a torn
    /// tail and is dropped, exactly as [`crate::events`] drops it. A
    /// newline-terminated line that will not parse is the opposite situation —
    /// the line was committed and is not an event, which means the log was
    /// rewritten rather than appended to, and no amount of reading further
    /// recovers it.
    ///
    /// # Errors
    ///
    /// [`FoldError::RewrittenLog`] naming the first committed line that is not
    /// a valid event.
    pub fn parse_log(bytes: &[u8]) -> Result<Vec<TopologyEvent>, FoldError> {
        let committed_end = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |position| position + 1);
        let committed = std::str::from_utf8(&bytes[..committed_end]).map_err(|error| {
            FoldError::RewrittenLog {
                line: bytes[..error.valid_up_to()]
                    .iter()
                    .filter(|byte| **byte == b'\n')
                    .count()
                    + 1,
                detail: error.to_string(),
            }
        })?;

        let mut events = Vec::new();
        for (position, line) in committed.lines().enumerate() {
            // Every committed line is one event, including a blank or
            // whitespace-only one. refusals[23] is about the *commit marker*,
            // not about what the bytes look like: a newline-terminated line
            // that is not a valid event means the log was rewritten, and a line
            // that is empty is not a valid event. Skipping it would fold a log
            // whose physical shape nobody can account for.
            events.push(
                serde_json::from_str::<TopologyEvent>(line).map_err(|error| {
                    FoldError::RewrittenLog {
                        line: position + 1,
                        detail: error.to_string(),
                    }
                })?,
            );
        }
        Ok(events)
    }

    /// Mark this process's fold unusable after an append whose outcome is
    /// unknown.
    ///
    /// Not a state transition and not reversible. The command has already
    /// ended; what remains is to refuse everything that would derive an effect
    /// from a state this process can no longer vouch for.
    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub fn started(&self) -> Option<&RunStarted4> {
        self.run.as_ref().map(|run| &*run.started)
    }

    pub fn registry(&self) -> Option<&TaskRegistry> {
        self.run.as_ref().map(|run| &run.registry)
    }

    pub fn task(&self, key: TaskKey) -> Option<&TaskFold> {
        self.run.as_ref()?.tasks.get(key.index())
    }

    pub fn task_state(&self, key: TaskKey) -> Option<TaskState> {
        self.task(key).map(|task| task.state)
    }

    pub fn queue(&self) -> Option<&CandidateQueue> {
        self.run.as_ref().map(|run| &run.queue)
    }

    pub fn leases(&self) -> Option<&LeaseTable> {
        self.run.as_ref().map(|run| &run.leases)
    }

    pub fn transaction(&self) -> Option<&Transaction> {
        self.run.as_ref()?.transaction.as_ref()
    }

    pub fn epoch(&self) -> Option<Epoch> {
        self.run.as_ref().map(|run| run.epoch)
    }

    pub fn halted_at(&self) -> Option<TaskKey> {
        self.run.as_ref()?.halted_at
    }

    pub fn budget_stop(&self) -> Option<BudgetStop> {
        self.run.as_ref()?.budget_stop
    }

    pub fn finished(&self) -> Option<&RunOutcome> {
        self.run.as_ref()?.finished.as_ref()
    }

    /// This run's folded state, or `None` before its `run_started`.
    ///
    /// The value two folds are compared as: a live fold and a replay of the
    /// bytes it appended hold the same `RunState` or INV-02 does not hold.
    pub fn state(&self) -> Option<&RunState> {
        self.run.as_ref()
    }

    pub fn open_questions(&self) -> Option<&BTreeMap<QuestionId, OpenQuestion>> {
        self.run.as_ref().map(|run| &run.questions)
    }

    pub fn binding_override(&self, key: TaskKey) -> Option<&BindingOverride> {
        self.run.as_ref()?.overrides.get(&key)
    }

    // -----------------------------------------------------------------------
    // Selection predicates
    //
    // `decisions.admission_and_leases` defines `ready` and `ready_retry` as
    // "structural over fold state only", and INV-22 makes entitlements
    // fold-enforced. The loop that drives a run therefore has to ask the fold
    // these questions rather than answer them itself: a second implementation
    // of "which generation classes hold the pipeline entitlement" is two rules
    // that can disagree, and `wrong_internal_assumption` is the largest
    // measured root cause in this project by a factor of three.
    //
    // What stays with the caller is the packet's actual division of labour:
    // the loop decides *which* eligible item to take and checks the budget
    // ceiling (`sequential_substrate.loop`; a breach appends `budget_exceeded`
    // before any effect), and the fold decides *whether* an item is
    // structurally eligible. These accessors are that second half and nothing
    // more — each delegates to the private predicate it names and adds no
    // logic of its own.
    //
    // Each returns the value that is true of a run which has not recorded its
    // `run_started` yet, rather than an `Option`: no task of an unstarted run
    // is ready, and such a run holds no entitlement. Those are statements, not
    // defaults.
    //
    // **A poisoned fold authorises nothing.** `plan_transition` refuses with
    // `FoldError::Poisoned` once an append has returned an error, and INV-20
    // says "no completion is applied after the fold is poisoned by a returned
    // append error". A predicate that kept answering `true` would let the
    // coordinator select work from a state this process can no longer vouch
    // for, and the append-error protocol's "no report, cleanup, or question
    // payload is derived from the poisoned fold" would hold in the emit path
    // and leak here. So every predicate below is false once poisoned.
    //
    // The exceptions are the four that state what the run *is* rather than
    // what it may do: `pipeline_held`, `run_is_ending`, `backoff_pending` and
    // `questions_open`. They are accounting, not authorisation. A poisoned
    // fold whose `halted_at` is set is still a halted run, and answering `0`
    // or `false` there would be a false statement about durable state rather
    // than a refusal. Their callers must not derive a report from them after a
    // poisoned append either, but that is a rule about reports and it lives in
    // the emit path — and nothing selects on them from a poisoned fold in any
    // case, because selection refuses at the top on `is_poisoned`.
    // -----------------------------------------------------------------------

    /// Whether `key` may be dispatched into a fresh generation.
    ///
    /// `decisions.admission_and_leases.ready`.
    #[must_use]
    pub fn ready(&self, key: TaskKey) -> bool {
        !self.poisoned && self.run.as_ref().is_some_and(|run| run.ready(key))
    }

    /// Whether `key` may take its next attempt in the generation it retained.
    ///
    /// `decisions.admission_and_leases.ready_retry`. False in any incarnation
    /// but the retaining one — `retained_incarnation == state.resumes` is part
    /// of the predicate, which is why a caller must not re-derive it.
    #[must_use]
    pub fn ready_retry(&self, key: TaskKey) -> bool {
        !self.poisoned && self.run.as_ref().is_some_and(|run| run.ready_retry(key))
    }

    /// The pipeline entitlement currently held, derived from the fold.
    ///
    /// Generations in `OpenNoAttempt`, `InFlight` and `Promoting` hold one
    /// each; `RetainedIdle` and `Closed` hold none; an unresolved integration
    /// transaction holds one. `decisions.admission_and_leases.permits.pipeline`.
    #[must_use]
    pub fn pipeline_held(&self) -> usize {
        self.run.as_ref().map_or(0, RunState::pipeline_held)
    }

    /// Whether a further pipeline entitlement is within `max_parallel`.
    #[must_use]
    pub fn pipeline_reservable(&self) -> bool {
        !self.poisoned && self.run.as_ref().is_some_and(RunState::pipeline_reservable)
    }

    /// Whether some task could be dispatched, retried, or integrated from this
    /// state alone.
    ///
    /// Budget, capacity and runner availability are not consulted — this is
    /// what "structurally admissible" means, and it is the predicate the
    /// ceiling check is applied *to*, not a substitute for it.
    #[must_use]
    pub fn structurally_admissible(&self) -> bool {
        !self.poisoned
            && self
                .run
                .as_ref()
                .is_some_and(RunState::structurally_admissible)
    }

    /// Whether an integration transaction could start from this state.
    #[must_use]
    pub fn integration_admissible(&self) -> bool {
        !self.poisoned
            && self
                .run
                .as_ref()
                .is_some_and(RunState::integration_admissible)
    }

    /// Whether this run has already ended in the sense that forbids further
    /// work: `halted_at` is set, or a `budget_stop` of **this** epoch is.
    ///
    /// The epoch is the load-bearing half. A `budget_stop` recorded in an
    /// earlier incarnation was cleared by the resume that raised the ceiling,
    /// and a caller that read the field without the epoch would refuse a run
    /// the operator has already unblocked. It is exposed for the same reason
    /// `ready` is: `refusals[18]` refuses `defer_wait_elapsed` under either
    /// condition, so a selector that decided the backoff branch from its own
    /// copy of this rule would offer the loop an append the fold is about to
    /// refuse — and the two copies would be free to disagree.
    #[must_use]
    pub fn run_is_ending(&self) -> bool {
        self.run.as_ref().is_some_and(RunState::run_is_ending)
    }

    /// Whether anything is waiting on a wait: a task in
    /// [`TaskState::Deferred`], or a queue entry whose verification was
    /// deferred by an outage.
    ///
    /// This is the *pending work* half of the backoff branch and not the
    /// branch itself — [`Self::run_is_ending`] is the other half, and
    /// `derived_outcome` consults this one alone. Both halves are here so that
    /// neither is re-derived: the fold walks its own tasks, and a caller
    /// walking the registry's keys instead is walking a different sequence the
    /// moment a repair is registered.
    #[must_use]
    pub fn backoff_pending(&self) -> bool {
        self.run.as_ref().is_some_and(RunState::backoff_pending)
    }

    /// The binding rung `rung` of `key` is frozen as.
    ///
    /// **The eleventh reader, and it is deliberately only half of the fold's
    /// rule.** `check_attempt_started` accepts a binding that matches the
    /// human override when one is recorded, and the frozen rung otherwise.
    /// This returns the frozen rung's, and nothing else, for two reasons.
    ///
    /// First, no override is constructible in a run this crate currently
    /// drives: a `BindingOverride` arrives from an `Answered` event, and the
    /// loop's answer-ingest branch is not implemented, so the override arm has
    /// no reachable input. Second, and more important, **the fold's override
    /// check is partial**: `matches_override` compares agent, model and effort
    /// and says nothing about `tier` or `pinned`. A caller that built an
    /// override binding would be choosing those two fields unchallenged, and
    /// this reader is not the place to invent a rule the packet states
    /// somewhere the author of this method has not read.
    ///
    /// So a caller holding an override must not use this. The intended shape,
    /// when the answers branch lands, is that this method grows the second arm
    /// together with the passage that decides those two fields — not that a
    /// caller composes one from [`Self::binding_override`] and this.
    ///
    /// `None` when the run has no registry, the task is not registered, or the
    /// ladder has no such rung.
    #[must_use]
    pub fn frozen_rung_binding(&self, key: TaskKey, rung: u32) -> Option<RungBinding> {
        let entry = self.registry()?.get(key)?;
        let frozen = entry.ladder.rungs.get(usize::try_from(rung).ok()?)?;
        Some(RungBinding::from_frozen(
            frozen,
            entry.ladder.effort.implementation_for(frozen.tier),
        ))
    }

    /// The generation `key` has open with no attempt started, if it has one.
    ///
    /// **`T-DISPATCH`'s "continue attempt (no spend repeats)", made askable.**
    /// A run killed between `task_dispatched` and `attempt_started` leaves the
    /// generation `OpenNoAttempt`; recovery step (g) verifies or recreates its
    /// worktree, and then the loop is supposed to start the attempt in it.
    ///
    /// [`Self::ready`] cannot answer this and must not: it requires
    /// `task.open().is_none()`, which is correct — a task with an open
    /// generation is not *ready to be dispatched*. The continuation is a
    /// different question about the same task, and asking it of `ready` would
    /// make one predicate mean two things.
    ///
    /// A lookup over [`Self::task`]'s own state, deciding nothing: the class is
    /// what `apply` recorded and the id is the generation's. Poisoning is not
    /// consulted for the same reason the other statement accessors do not — a
    /// poisoned fold of a run with an open generation still has one, and `None`
    /// here would be a false statement rather than a refusal.
    #[must_use]
    pub fn open_no_attempt(&self, key: TaskKey) -> Option<GenerationId> {
        self.task(key)?
            .generations
            .iter()
            .find(|generation| generation.class == GenerationClass::OpenNoAttempt)
            .map(|generation| generation.id)
    }

    /// The region an ordinary dispatch of `key` predicts.
    ///
    /// **The tenth reader, and it exists because the alternative was a second
    /// authority.** `dispatch_lease_check` decides whether a task is `ready` at
    /// all by computing this region and asking the lease table what it
    /// overlaps. A caller that then recorded a *different* region in
    /// `task_dispatched` would have the fold admitting on one answer and the
    /// log holding another — and the log's is the one the lease table keeps.
    ///
    /// That is not hypothetical. It was written: a driver that took the plan's
    /// hints literally recorded `src/auth/*.rs` as a **prefix**, which overlaps
    /// nothing, while the fold had admitted the dispatch on `src/auth`. At
    /// `max_parallel = 1` nothing can collide and the disagreement is
    /// invisible; at the first width above one it is two tasks editing the same
    /// files.
    ///
    /// **A convention until the region became derivation-checked.** This reader
    /// existing did not oblige anyone to read it: `check_dispatched` matched a
    /// `task_dispatched` lease's *shape* only and `apply_dispatched` granted the
    /// region the event carried. `check_dispatched` now refuses an ordinary
    /// dispatch whose recorded region is not this answer, so the disagreement is
    /// inexpressible rather than merely undocumented, and this reader is the
    /// convenient way to obtain what the fold will accept rather than the only
    /// way to avoid a refusal.
    ///
    /// `None` when the run has no registry yet, which is before `run_started`.
    #[must_use]
    pub fn predicted_region(&self, key: TaskKey) -> Option<PathSet> {
        self.registry()
            .and_then(|registry| registry.get(key))
            .map(predicted_region)
    }

    /// Whether any question is open.
    ///
    /// The ids themselves are [`Self::open_questions`]; this is the predicate
    /// `derived_outcome` decides `Parked` with, exposed so that the hard-block
    /// branch and the derived outcome cannot disagree about what "open" means.
    #[must_use]
    pub fn questions_open(&self) -> bool {
        self.run.as_ref().is_some_and(RunState::questions_open)
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

    // -----------------------------------------------------------------------
    // run_started
    // -----------------------------------------------------------------------

    fn check_run_started(&self, started: &RunStarted4) -> Result<TaskRegistry, FoldError> {
        if self.run.is_some() {
            return Err(FoldError::AlreadyStarted);
        }
        if !started.is_topology_schema() {
            return Err(FoldError::NotTopologySchema {
                schema: started.schema,
            });
        }
        // refusals[5], first half: the record must name everything needed to
        // re-establish the runner. The digest is not required — it is the
        // manifest digest when the runtime reported one (INV-23).
        started
            .runner
            .completeness()
            .map_err(|defect| FoldError::IncompleteRunner {
                defect: defect.to_string(),
            })?;

        // refusals[4]: both digests, against the bytes this reader was handed.
        if started.normalized_plan_digest != self.inputs.normalized_plan_digest {
            return Err(FoldError::DigestMismatch {
                what: "normalized plan",
                recorded: started.normalized_plan_digest.clone(),
                actual: self.inputs.normalized_plan_digest.clone(),
            });
        }
        let registry = TaskRegistry::originals_with_agents(
            &self.inputs.plan,
            &started.registry_record(),
            &started.probed_agents,
        )
        .map_err(|error| FoldError::RegistryUnbuildable {
            detail: error.to_string(),
        })?;
        let actual = registry.digest();
        if actual != started.registry_digest {
            return Err(FoldError::DigestMismatch {
                what: "registry",
                recorded: started.registry_digest.clone(),
                actual,
            });
        }

        // Ladder validation at the fold boundary: a malformed ladder is refused
        // before it is stored, not when something tries to climb it.
        for entry in registry.entries() {
            check_ladder(entry.key, &entry.ladder)?;
        }
        Ok(registry)
    }

    // -----------------------------------------------------------------------
    // Everything after run_started
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn check_started_run(
        &self,
        run: &RunState,
        event: &TopologyEvent,
        kind: &'static str,
    ) -> Result<TopologyDelta, FoldError> {
        // refusals[21]: a Complete or Halted run is finalized and then refused,
        // never continued. A Parked or BudgetExceeded run continues, and the
        // only event that continues it is the resume that opens the next epoch.
        if let Some(outcome) = run.finished.clone() {
            match outcome {
                RunOutcome::Complete | RunOutcome::Halted => {
                    return Err(FoldError::RunIsOver {
                        kind,
                        outcome: outcome_name(&outcome),
                    });
                }
                RunOutcome::Parked | RunOutcome::BudgetExceeded => {
                    if !matches!(event.body, TopologyEventBody::RunResumed { .. }) {
                        return Err(FoldError::RunIsOver {
                            kind,
                            outcome: outcome_name(&outcome),
                        });
                    }
                }
            }
        }

        match &event.body {
            TopologyEventBody::RunStarted { .. } => Err(FoldError::AlreadyStarted),
            TopologyEventBody::RunResumed { data } => run
                .check_run_resumed(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::TaskSpawned { data } => run
                .check_spawn(&data.spawn, kind)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::TaskDispatched { data } => run
                .check_dispatched(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::AttemptStarted { data } => run
                .check_attempt_started(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::AttemptFinished { data } => run
                .check_attempt_finished(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::AttemptInterrupted { data } => run
                .check_attempt_interrupted(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::GenerationClosed { data } => run
                .check_generation_closed(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::DeferWaitElapsed { .. } => run
                .check_defer_wait_elapsed()
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::CandidatePrepared { data } => run
                .check_candidate_prepared(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::TaskCandidateCreated { data } => run
                .check_candidate_created(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergeVerificationStarted { data } => run
                .check_verification_started(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergeVerificationUnavailable { data } => run
                .check_verification_unavailable(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergeVerificationInterrupted { data } => run
                .check_verification_interrupted(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergePrepared { data } => run
                .check_merge_prepared(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergeRejected { data } => run
                .check_merge_rejected(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::TaskMerged { data } => run
                .check_task_merged(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::QuestionRaised { data } => run
                .check_question_raised(&data.question)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::QuestionAnswered { data } => run
                .check_question_answered(data)
                .map(|origin| self.delta(event, Derived::Answer(origin))),
            TopologyEventBody::BudgetExceeded { data } => run
                .check_budget_exceeded(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::RunFinished { data } => run
                .check_run_finished(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::CapacitySnapshot { .. }
            | TopologyEventBody::PoolExhausted { .. }
            | TopologyEventBody::DesignDefect { .. } => Ok(self.delta(event, Derived::None)),
        }
    }

    // -----------------------------------------------------------------------
    // The derived outcome
    // -----------------------------------------------------------------------

    /// The total outcome function (`decisions.run_end_policy.derived_outcome`).
    ///
    /// Computed from durable state alone: no spend, no capacity, no runner
    /// availability, no clock. The legacy precedence is preserved —
    /// halt > budget > parked > complete — and pending backoff makes `Parked`
    /// and `Complete` [`DerivedOutcome::NotEnding`] without ever blocking
    /// `Halted` or `BudgetExceeded`.
    ///
    /// A run that has not started is [`DerivedOutcome::NotEnding`]: nothing has
    /// been recorded, so nothing has ended.
    pub fn derived_outcome(&self) -> DerivedOutcome {
        self.run
            .as_ref()
            .map_or(DerivedOutcome::NotEnding, RunState::derived_outcome)
    }
}

fn outcome_name(outcome: &RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Complete => "complete",
        RunOutcome::Parked => "parked",
        RunOutcome::Halted => "halted",
        RunOutcome::BudgetExceeded => "budget exceeded",
    }
}

/// Whether a frozen ladder is one an attempt could actually climb.
///
/// Fold-boundary work rather than registry work: the registry derives a ladder
/// from whatever the run recorded, and this decides whether that ladder may
/// enter a fold's state. Both malformations it names are invisible to the
/// registry — a floor above its ceiling clips to nothing on the first
/// escalation, and a tier list that does not ascend makes "the next rung" mean
/// two different things depending on whether it is read by position or by tier.
fn check_ladder(key: TaskKey, ladder: &FrozenLadder) -> Result<(), FoldError> {
    let malformed = |defect: String| FoldError::MalformedLadder { key: key.0, defect };

    if let (Some(floor), Some(ceiling)) = (ladder.floor, ladder.ceiling) {
        if floor > ceiling {
            return Err(malformed(format!(
                "its floor is `{floor}` and its ceiling is `{ceiling}`, so no tier satisfies both"
            )));
        }
    }
    if ladder.attempts_per == 0 {
        return Err(malformed(
            "it allows 0 attempts per rung, so no attempt is ever permitted".to_owned(),
        ));
    }
    let mut previous: Option<Tier> = None;
    for tier in &ladder.tiers {
        if let Some(previous) = previous {
            if *tier <= previous {
                return Err(malformed(format!(
                    "its tiers are recorded as `{}`, which does not escalate: `{tier}` does not \
                     outrank `{previous}`",
                    ladder
                        .tiers
                        .iter()
                        .map(Tier::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
        previous = Some(*tier);
    }
    if ladder.ceiling != ladder.tiers.iter().copied().max() {
        return Err(malformed(format!(
            "its recorded ceiling is {:?} and its highest rung is {:?}",
            ladder.ceiling.map(|tier| tier.to_string()),
            ladder
                .tiers
                .iter()
                .copied()
                .max()
                .map(|tier| tier.to_string())
        )));
    }
    match &ladder.admission {
        Admission::Runnable => {
            if ladder.rungs.is_empty() {
                return Err(malformed(
                    "it is admitted as runnable and has no rungs, so there is no binding to run"
                        .to_owned(),
                ));
            }
        }
        Admission::HumanBinding { options } => {
            if !ladder.rungs.is_empty() {
                return Err(malformed(
                    "it waits for a human binding and already has rungs, so two authorities name \
                     what runs"
                        .to_owned(),
                ));
            }
            if options.is_empty() {
                return Err(malformed(
                    "it waits for a human binding and offers no agent to choose from".to_owned(),
                ));
            }
        }
    }
    if !ladder.rungs.is_empty() && ladder.rungs.len() != ladder.tiers.len() {
        return Err(malformed(format!(
            "it has {} rung binding(s) for {} tier(s)",
            ladder.rungs.len(),
            ladder.tiers.len()
        )));
    }
    for (rung, tier) in ladder.rungs.iter().zip(&ladder.tiers) {
        if rung.tier != *tier {
            return Err(malformed(format!(
                "its `{tier}` rung is bound at `{}`",
                rung.tier
            )));
        }
    }
    Ok(())
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

    // --- run_resumed -------------------------------------------------------

    fn check_run_resumed(&self, resumed: &RunResumed4) -> Result<(), FoldError> {
        // refusals[5], second half: exact equality, field for field (INV-23).
        if let Some(field) = self.started.runner.difference(&resumed.runner) {
            return Err(FoldError::RunnerMoved {
                field: field.to_string(),
            });
        }
        Ok(())
    }

    // --- task_spawned ------------------------------------------------------

    fn check_spawn(&self, spawn: &FrozenSpawn, kind: &'static str) -> Result<(), FoldError> {
        let malformed = |detail: String| FoldError::MalformedEntry {
            kind,
            key: spawn.key.0,
            detail,
        };
        // refusals[10]: a dynamic task's key is the registry's length at the
        // event that registers it.
        if spawn.key.index() != self.registry.len() {
            return Err(FoldError::NonDenseKey {
                kind,
                key: spawn.key.0,
                len: self.registry.len(),
            });
        }
        let entry = &spawn.entry;
        if entry.key != spawn.key {
            return Err(malformed(format!(
                "the embedded entry calls itself {} and the event registers {}",
                entry.key, spawn.key
            )));
        }
        if self.registry.key_of(entry.display_id.as_str()).is_some() {
            return Err(malformed(format!(
                "the display id `{}` already names another task",
                entry.display_id
            )));
        }
        let Some(lineage) = entry.lineage else {
            return Err(malformed(
                "a registered task descends from the rejection that produced it, and this one \
                 records no lineage"
                    .to_owned(),
            ));
        };
        if lineage.root >= spawn.key || lineage.parent >= spawn.key {
            return Err(malformed(format!(
                "its lineage names root {} and parent {}, and a key may only refer backwards from \
                 {}",
                lineage.root, lineage.parent, spawn.key
            )));
        }
        // The allow-list is the run's, not the registering event's: an entry
        // that widened it would admit an agent pre-flight never probed.
        if entry.allowed_agents != self.started.probed_agents {
            return Err(malformed(format!(
                "it allows {:?} and this run probed {:?}",
                entry.allowed_agents, self.started.probed_agents
            )));
        }
        // Dependencies: every one exists, refers backwards, and the display
        // list is the same list.
        if entry.deps.len() != entry.display_deps.len() {
            return Err(malformed(format!(
                "it records {} dependency key(s) and {} display dependency(ies)",
                entry.deps.len(),
                entry.display_deps.len()
            )));
        }
        for (dep, display) in entry.deps.iter().zip(&entry.display_deps) {
            if *dep >= spawn.key {
                return Err(malformed(format!(
                    "it depends on {dep}, which is not registered before it"
                )));
            }
            let known = self.entry(kind, *dep)?;
            if known.display_id != *display {
                return Err(malformed(format!(
                    "it names dependency {dep} as `{display}`, and that key is `{}`",
                    known.display_id
                )));
            }
            // A repair rebases work that was already integrated; a dependency
            // that is not merged has nothing for it to build on.
            if self.task(kind, *dep)?.state != TaskState::Merged {
                return Err(malformed(format!(
                    "it depends on {dep}, which is `{}` — a repair's dependencies are merged \
                     before it is registered",
                    self.task(kind, *dep)?.state.name()
                )));
            }
        }
        check_ladder(spawn.key, &entry.ladder)?;
        self.check_admission(spawn, &malformed)?;
        Ok(())
    }

    /// The registered entry's admission and the event's must be the same
    /// statement about the same task.
    fn check_admission<F>(&self, spawn: &FrozenSpawn, malformed: &F) -> Result<(), FoldError>
    where
        F: Fn(String) -> FoldError,
    {
        match (&spawn.admission, &spawn.entry.ladder.admission) {
            (SpawnAdmission::Runnable, Admission::Runnable) => {}
            (SpawnAdmission::HumanRequired { limit, .. }, Admission::Runnable) => {
                if *limit != self.started.limits.max_merge_repairs {
                    return Err(malformed(format!(
                        "it reports the automatic-repair limit as {limit} and this run froze {}",
                        self.started.limits.max_merge_repairs
                    )));
                }
            }
            (
                SpawnAdmission::HumanBinding { options, .. },
                Admission::HumanBinding {
                    options: frozen, ..
                },
            ) => {
                if options != frozen {
                    return Err(malformed(
                        "the event and the entry offer different bindings to choose from"
                            .to_owned(),
                    ));
                }
            }
            (event, _) => {
                return Err(malformed(format!(
                    "its admission is `{}` and its entry's is `{}`",
                    spawn_admission_name(event),
                    admission_name(&spawn.entry.ladder.admission)
                )));
            }
        }
        if let Some(question) = spawn.admission.question() {
            self.check_new_question("task_spawned", question, spawn.key)?;
        }
        Ok(())
    }

    fn check_new_question(
        &self,
        kind: &'static str,
        question: &FrozenQuestion,
        key: TaskKey,
    ) -> Result<(), FoldError> {
        if !question.is_complete() {
            return Err(FoldError::UnanswerableQuestion {
                kind,
                detail: format!(
                    "`{}` has no identity, no context, or no options, so the task it parks has no \
                     way to continue",
                    question.id
                ),
            });
        }
        if question.key != key {
            return Err(FoldError::UnanswerableQuestion {
                kind,
                detail: format!(
                    "`{}` is keyed to task {} and this event parks task {key}",
                    question.id, question.key
                ),
            });
        }
        if self.seen_questions.contains(&question.id) {
            return Err(FoldError::WrongQuestion {
                kind,
                question: question.id.to_string(),
                detail: "this log has already used that identity; a question is asked once"
                    .to_owned(),
            });
        }
        Ok(())
    }

    // --- task_dispatched ---------------------------------------------------

    fn check_dispatched(&self, dispatched: &TaskDispatched) -> Result<(), FoldError> {
        const KIND: &str = "task_dispatched";
        let entry = self.entry(KIND, dispatched.key)?;
        let task = self.task(KIND, dispatched.key)?;

        if task.state != TaskState::Pending {
            return Err(FoldError::WrongTaskState {
                kind: KIND,
                key: dispatched.key.0,
                state: task.state.name(),
                expected: "pending",
            });
        }
        if let Some(open) = task.open() {
            return Err(FoldError::NotTheOpenGeneration {
                kind: KIND,
                key: dispatched.key.0,
                generation: dispatched.generation.0,
                detail: format!("generation {} is still {}", open.id.0, open.class.name()),
            });
        }
        // refusals[10]: generations are dense per task.
        if usize::try_from(dispatched.generation.0).unwrap_or(usize::MAX) != task.generations.len()
        {
            return Err(FoldError::NonDenseKey {
                kind: KIND,
                key: dispatched.generation.0,
                len: task.generations.len(),
            });
        }

        let is_repair = entry.lineage.is_some();
        match (&dispatched.lease, entry.lineage) {
            // **The recorded region is derivation-checked, exactly as the
            // recorded binding is.** One event over, `check_attempt_started`
            // refuses a binding the fold did not derive
            // (`FoldError::BindingMismatch`); this arm used to match the
            // lease's *shape* alone and let `apply_dispatched` grant whatever
            // region the event carried — so the fold could admit a dispatch on
            // one region while the lease table held another, and the lease
            // table's is the one every later overlap check consults.
            //
            // That was not hypothetical. A driver that took the plan's hints
            // literally recorded `src/auth/*.rs` as a **prefix**, which
            // overlaps nothing, while the fold had admitted the dispatch on
            // `src/auth`. `84a3978` made the driver read
            // [`TopologyFold::predicted_region`] instead of deriving its own,
            // which fixed that instance; nothing stopped the next caller — or
            // a later slice's second writer — from constructing a
            // `task_dispatched` the fold would accept and the lease table would
            // honour. This is the class fix, and it is why the reader and this
            // validator call the **same** free function rather than two copies
            // of one rule.
            //
            // **Exact equality, and deliberately not a policy-aware one.** The
            // run's frozen `PathPolicy` decides whether two regions *overlap*,
            // case-folding component by component; it does not decide whether
            // two regions are the same region. A recorded `SRC/Auth` that folds
            // onto a derived `src/auth` is still a different literal, and the
            // lease table stores literals — so an equality that folded here
            // would admit a component set the derivation never produced and
            // hand it to `apply_dispatched` unchanged. Order counts for the
            // same reason: the derivation emits one prefix per hint in the
            // frozen order, so a reordered list is a list this run's frozen
            // hints do not derive.
            //
            // Live at the first width above `max_parallel = 1`, where two tasks
            // holding non-overlapping-by-construction regions edit the same
            // files; invisible below it.
            (LeaseGrant::Predicted { paths }, None) => {
                let derived = predicted_region(entry);
                if *paths != derived {
                    return Err(FoldError::MalformedEntry {
                        kind: KIND,
                        key: dispatched.key.0,
                        detail: format!(
                            "it takes the predicted region {} and this entry's frozen path \
                             hints derive {}; an ordinary dispatch takes the region the fold \
                             admitted it on",
                            describe_region(paths),
                            describe_region(&derived)
                        ),
                    });
                }
            }
            (LeaseGrant::InheritedLineage { root }, Some(lineage)) => {
                if *root != lineage.root {
                    return Err(FoldError::MalformedEntry {
                        kind: KIND,
                        key: dispatched.key.0,
                        detail: format!(
                            "it executes inside lineage {root} and its entry descends from {}",
                            lineage.root
                        ),
                    });
                }
            }
            (LeaseGrant::Predicted { .. }, Some(_)) => {
                return Err(FoldError::MalformedEntry {
                    kind: KIND,
                    key: dispatched.key.0,
                    detail: "a repair takes no lease of its own; it executes inside the lineage \
                             lease its root already holds"
                        .to_owned(),
                });
            }
            (LeaseGrant::InheritedLineage { .. }, None) => {
                return Err(FoldError::MalformedEntry {
                    kind: KIND,
                    key: dispatched.key.0,
                    detail: "an ordinary task belongs to no lineage and cannot inherit one's lease"
                        .to_owned(),
                });
            }
        }
        if is_repair != dispatched.source_candidate.is_some() {
            return Err(FoldError::MalformedEntry {
                kind: KIND,
                key: dispatched.key.0,
                detail: if is_repair {
                    "a repair is materialized from the candidate its lineage rejected, and this \
                     dispatch names none"
                        .to_owned()
                } else {
                    "an ordinary dispatch materializes nothing and this one names a source \
                     candidate"
                        .to_owned()
                },
            });
        }
        Ok(())
    }

    // --- attempt_started ---------------------------------------------------

    fn check_attempt_started(&self, started: &AttemptStarted4) -> Result<(), FoldError> {
        const KIND: &str = "attempt_started";
        let entry = self.entry(KIND, started.key)?;
        let task = self.task(KIND, started.key)?;
        let generation = self.open_generation(KIND, task, started.key, started.generation)?;

        // ST-06: a retry names the generation it is retrying, and a fresh
        // attempt names one nothing has run in yet.
        match (&generation.class, &started.resume_session) {
            (GenerationClass::OpenNoAttempt, None) => {}
            (
                GenerationClass::RetainedIdle {
                    session,
                    incarnation,
                },
                Some(resumed),
            ) => {
                // refusals[12]: a session belongs to the incarnation that
                // retained it, and only that incarnation may resume it.
                if session != resumed {
                    return Err(FoldError::StaleIncarnation {
                        key: started.key.0,
                        attempt: started.attempt.0,
                        detail: format!(
                            "it resumes session `{resumed}` and the generation retained `{session}`"
                        ),
                    });
                }
                if *incarnation != self.epoch {
                    return Err(FoldError::StaleIncarnation {
                        key: started.key.0,
                        attempt: started.attempt.0,
                        detail: format!(
                            "the session was retained by incarnation {} and this run has resumed \
                             {} time(s)",
                            incarnation.0, self.epoch.0
                        ),
                    });
                }
            }
            (class, resumed) => {
                return Err(FoldError::NotTheOpenGeneration {
                    kind: KIND,
                    key: started.key.0,
                    generation: started.generation.0,
                    detail: if resumed.is_some() {
                        format!(
                            "it resumes a session and the generation is {}, not retained idle",
                            class.name()
                        )
                    } else {
                        format!(
                            "the generation is {} and a fresh attempt starts in one nothing has \
                             run in",
                            class.name()
                        )
                    },
                });
            }
        }

        // ST-06: attempts are dense from 1 within a generation.
        if started.attempt.0 != generation.attempts + 1 {
            return Err(FoldError::WrongAttempt {
                kind: KIND,
                key: started.key.0,
                generation: started.generation.0,
                attempt: started.attempt.0,
                expected: (generation.attempts + 1).to_string(),
            });
        }

        // refusals[11] / INV-19: the binding is the override when one was
        // recorded, and the frozen rung binding otherwise.
        let mismatch = |detail: String| FoldError::BindingMismatch {
            key: started.key.0,
            attempt: started.attempt.0,
            detail,
        };
        if let Some(binding) = self.overrides.get(&started.key) {
            if !started.binding.matches_override(binding) {
                return Err(mismatch(format!(
                    "a human named `{}`/`{}` at effort `{}` for this task and it ran `{}`/`{}` at \
                     effort `{}`",
                    binding.agent,
                    binding.model,
                    binding.effort,
                    started.binding.agent,
                    started.binding.model,
                    started.binding.effort
                )));
            }
        } else {
            let rung = usize::try_from(started.rung).unwrap_or(usize::MAX);
            let frozen = entry.ladder.rungs.get(rung).ok_or_else(|| {
                mismatch(format!(
                    "it climbs rung {rung} of a ladder with {} rung(s)",
                    entry.ladder.rungs.len()
                ))
            })?;
            let effort = entry.ladder.effort.implementation_for(frozen.tier);
            if !started.binding.matches_frozen(frozen, effort) {
                return Err(mismatch(format!(
                    "rung {rung} is frozen as `{}`/`{}` at tier `{}` effort `{}` and it ran \
                     `{}`/`{}` at tier `{}` effort `{}`",
                    frozen.agent,
                    frozen.model,
                    frozen.tier,
                    effort,
                    started.binding.agent,
                    started.binding.model,
                    started.binding.tier,
                    started.binding.effort
                )));
            }
        }

        if entry.lineage.is_some() != started.materialization_observed.is_some() {
            return Err(FoldError::MalformedEntry {
                kind: KIND,
                key: started.key.0,
                detail: if entry.lineage.is_some() {
                    "a repair's attempt records what its worktree was materialized from".to_owned()
                } else {
                    "an ordinary attempt materializes nothing".to_owned()
                },
            });
        }
        Ok(())
    }

    /// The open generation this event must be naming (ST-06).
    fn open_generation<'a>(
        &self,
        kind: &'static str,
        task: &'a TaskFold,
        key: TaskKey,
        generation: GenerationId,
    ) -> Result<&'a GenerationFold, FoldError> {
        let open = task.open().ok_or_else(|| FoldError::NotTheOpenGeneration {
            kind,
            key: key.0,
            generation: generation.0,
            detail: "no generation of this task is open".to_owned(),
        })?;
        if open.id != generation {
            return Err(FoldError::NotTheOpenGeneration {
                kind,
                key: key.0,
                generation: generation.0,
                detail: format!("generation {} is the open one", open.id.0),
            });
        }
        Ok(open)
    }

    /// The open generation, additionally required to be running `attempt`.
    fn in_flight<'a>(
        &self,
        kind: &'static str,
        task: &'a TaskFold,
        key: TaskKey,
        generation: GenerationId,
        attempt: AttemptNumber,
    ) -> Result<&'a GenerationFold, FoldError> {
        let open = self.open_generation(kind, task, key, generation)?;
        let GenerationClass::InFlight { attempt: running } = &open.class else {
            return Err(FoldError::NotTheOpenGeneration {
                kind,
                key: key.0,
                generation: generation.0,
                detail: format!(
                    "the generation is {}, and no attempt is running",
                    open.class.name()
                ),
            });
        };
        if *running != attempt {
            return Err(FoldError::WrongAttempt {
                kind,
                key: key.0,
                generation: generation.0,
                attempt: attempt.0,
                expected: running.0.to_string(),
            });
        }
        Ok(open)
    }

    // --- attempt_finished --------------------------------------------------

    fn check_attempt_finished(&self, finished: &AttemptFinished4) -> Result<(), FoldError> {
        const KIND: &str = "attempt_finished";
        let task = self.task(KIND, finished.key)?;
        let generation = self.in_flight(
            KIND,
            task,
            finished.key,
            finished.generation,
            finished.attempt,
        )?;

        match &finished.settlement {
            AttemptSettlement::Retained {
                retained_session,
                retained_incarnation,
            } => {
                if *retained_incarnation != self.epoch {
                    return Err(FoldError::StaleIncarnation {
                        key: finished.key.0,
                        attempt: finished.attempt.0,
                        detail: format!(
                            "it retains the session for incarnation {} and this run has resumed \
                             {} time(s)",
                            retained_incarnation.0, self.epoch.0
                        ),
                    });
                }
                // **The envelope and the record name one attempt, on this arm
                // too.** This arm checked the epoch and stopped, so a
                // current-epoch retained settlement could carry a ledger line
                // belonging to a different attempt of the same generation —
                // the same disagreement the `Closed` arm has refused since
                // round 6, one arm over. Every one of that round's four new
                // refusal witnesses constructs `Closed`, which is why this arm
                // was undriven; the `cfa1be8` review found it as its second P1.
                //
                // **A door is not fixed until every arm through it asks the
                // same question.**
                if finished.record.attempt != finished.attempt.0 {
                    return Err(FoldError::WrongAttempt {
                        kind: KIND,
                        key: finished.key.0,
                        generation: finished.generation.0,
                        attempt: finished.record.attempt,
                        expected: finished.attempt.0.to_string(),
                    });
                }
                // **And the record does not claim the attempt succeeded.**
                // `candidate_prepared` is the sole successful settlement
                // (INV-07,
                // `decisions/2026-08-12-merge-queue-execution-topology.md`),
                // and the `Closed` arm has enforced that against the record
                // since round 6. This arm did not, so the invariant held on one
                // path through the door and not the other: a current-epoch
                // retained settlement could carry a record with no failure and
                // every configured pass green — a record
                // `check_candidate_prepared` would itself accept — while the
                // fold held the generation open for a retry. The ledger line an
                // operator reads would say the work passed.
                //
                // **This is not a terminal-failure requirement, and the
                // difference is the whole of the earlier hesitation.**
                // `settle::settle_failed` is the only producer of a `Retained`
                // settlement and it is reached on the failure path, for a
                // same-rung retry that has a session to resume — so a retained
                // attempt has not succeeded, by construction. Asking
                // `!is_successful()` is the record saying that much and no
                // more: `Retained` carries no transition, so nothing here makes
                // the generation terminal, and the arm goes on to leave it open
                // with its lease held.
                //
                // One predicate, both arms, as the candidate door and the
                // closed settlement already share it — a door is not fixed
                // until every arm through it asks the same question.
                if finished.record.is_successful() {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "attempt {} of generation {} retains its session for a further \
                             attempt and its record says the attempt succeeded — failure {:?}, \
                             review outcomes {:?} — and `candidate_prepared` is the settlement \
                             of an attempt that succeeded",
                            finished.attempt.0,
                            finished.generation.0,
                            finished.record.failure.as_ref().map(|failure| failure.kind),
                            finished
                                .record
                                .reviews
                                .iter()
                                .map(|pass| pass.outcome)
                                .collect::<Vec<_>>()
                        ),
                    });
                }
                // **And the record names the conversation the settlement
                // keeps.** A `Retained` settlement exists to hold a session for
                // a same-session retry, and `check_attempt_started` will make
                // the retry name the *generation's* session — the one this
                // event puts there. If the ledger line names another session,
                // or none, then the two halves of one event disagree about
                // which conversation was left open, and the half a person reads
                // is not the half the fold enforces.
                if finished.record.session_id.as_deref() != Some(retained_session.0.as_str()) {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "attempt {} of generation {} retains session `{retained_session}` \
                             and its record names {}; a retained settlement holds the session \
                             its own ledger line reports",
                            finished.attempt.0,
                            finished.generation.0,
                            match finished.record.session_id.as_deref() {
                                Some(other) => format!("`{other}`"),
                                None => "no session at all".to_owned(),
                            }
                        ),
                    });
                }
            }
            AttemptSettlement::Closed { transition, lease } => {
                // **`attempt_finished` does not settle a success.** INV-07 and
                // `decisions/2026-08-12-merge-queue-execution-topology.md` say
                // it outright — `candidate_prepared` is "the **sole**
                // successful settlement for an attempt that produces a
                // candidate … `attempt_finished` is not also emitted for that
                // attempt" — and this build appended both, so one attempt
                // carried its record on two lines.
                //
                // Refused here rather than tolerated downstream. The 2026-08-27
                // ruling is CONFORM, not supersession, and a reader that
                // *coped* with the dual pattern would be a second reading of
                // the same sentence: `Spend::replay` grew per-attempt
                // deduplication to survive it, which is evidence of the
                // duplicate rather than permission for it. Schema 4 has no
                // external writers — `src/engine/mod.rs` is `pub(crate) mod
                // topology` — so no log this build did not write can carry the
                // shape, and refusing it costs no compatibility.
                if matches!(transition, SettlementTransition::Succeeded) {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "attempt {} of generation {} settles `succeeded`, and \
                             `candidate_prepared` is the sole successful settlement for a \
                             candidate-producing attempt",
                            finished.attempt.0, finished.generation.0
                        ),
                    });
                }
                // **The record must say the attempt failed, and must be this
                // attempt's.** This door refused `Succeeded` and asked nothing
                // else, so a settlement could fail a task and halt a run while
                // carrying a record whose failure field is empty and whose
                // reviews all passed — a ledger line reporting success attached
                // to a terminal failure. `AttemptRecord::is_successful` is the
                // one definition, shared with `check_candidate_prepared`, so
                // the two doors cannot drift apart again.
                if finished.record.is_successful() {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "attempt {} of generation {} settles as a failure and its record \
                             says the attempt succeeded — `candidate_prepared` is the \
                             settlement of a successful attempt",
                            finished.attempt.0, finished.generation.0
                        ),
                    });
                }
                // The envelope and the record name one attempt. Without this the
                // ledger line a settlement carries can belong to a different
                // attempt of the same generation.
                if finished.record.attempt != finished.attempt.0 {
                    return Err(FoldError::WrongAttempt {
                        kind: KIND,
                        key: finished.key.0,
                        generation: finished.generation.0,
                        attempt: finished.record.attempt,
                        expected: finished.attempt.0.to_string(),
                    });
                }
                check_lease_disposition(KIND, finished.key, generation.lease, *lease)?;
                if let SettlementTransition::Parked { question } = transition {
                    self.check_new_question(KIND, question, finished.key)?;
                }
            }
        }
        Ok(())
    }

    // --- attempt_interrupted -----------------------------------------------

    fn check_attempt_interrupted(
        &self,
        interrupted: &AttemptInterrupted4,
    ) -> Result<(), FoldError> {
        const KIND: &str = "attempt_interrupted";
        let task = self.task(KIND, interrupted.key)?;
        let generation = self.in_flight(
            KIND,
            task,
            interrupted.key,
            interrupted.generation,
            interrupted.attempt,
        )?;
        // The generation does *not* survive an interruption.
        // `transaction_fault_matrix[T-ATTEMPT].resume_action` is explicit:
        // "append attempt_interrupted (unknown spend, allowance refunded,
        // generation Closed, lease by kind) ... task returns Pending; later
        // dispatch new generation". Nothing was judged and the spend is
        // unknown, so the worktree is scrubbed with force rather than reused —
        // which is why an ordinary generation releases its predicted region
        // here and a lineage member goes on holding its root's.
        check_lease_disposition(KIND, interrupted.key, generation.lease, interrupted.lease)
    }

    // --- generation_closed -------------------------------------------------

    fn check_generation_closed(&self, closed: &GenerationClosed) -> Result<(), FoldError> {
        const KIND: &str = "generation_closed";
        let task = self.task(KIND, closed.key)?;
        let generation = self.open_generation(KIND, task, closed.key, closed.generation)?;
        // refusals[15]: an open generation with no attempt in flight. A
        // promoting generation is not closed — it is promoted.
        match generation.class {
            GenerationClass::OpenNoAttempt | GenerationClass::RetainedIdle { .. } => {}
            ref class => {
                return Err(FoldError::NotTheOpenGeneration {
                    kind: KIND,
                    key: closed.key.0,
                    generation: closed.generation.0,
                    detail: format!(
                        "it is {}, and a generation is closed only from open-with-no-attempt or \
                         retained-idle",
                        class.name()
                    ),
                });
            }
        }
        check_lease_disposition(KIND, closed.key, generation.lease, closed.lease)
    }

    // --- defer_wait_elapsed ------------------------------------------------

    fn check_defer_wait_elapsed(&self) -> Result<(), FoldError> {
        // refusals[18]: halt and budget outrank backoff, so no wait elapses
        // under either.
        if self.halted_at.is_some() {
            return Err(FoldError::RunEnding {
                kind: "defer_wait_elapsed",
                what: "a halting settlement",
            });
        }
        if self.budget_stop_is_current() {
            return Err(FoldError::RunEnding {
                kind: "defer_wait_elapsed",
                what: "the budget stop",
            });
        }
        Ok(())
    }

    // --- candidate_prepared ------------------------------------------------

    fn check_candidate_prepared(&self, prepared: &CandidatePrepared) -> Result<(), FoldError> {
        const KIND: &str = "candidate_prepared";
        let entry = self.entry(KIND, prepared.key)?;
        let task = self.task(KIND, prepared.key)?;
        let generation = self.open_generation(KIND, task, prepared.key, prepared.generation)?;
        // **The generation is still in flight, because this event is what
        // settles it.** It used to require `Promoting`, which only an
        // `attempt_finished{Succeeded}` could produce — so the fold *required*
        // the dual pattern the 2026-08-12 record forbids. With the settlement
        // moved here, a `Promoting` generation means that record was appended
        // anyway, and the arm above already refuses it; this refuses the other
        // half of the same shape, so neither order can produce two settlements.
        if !matches!(generation.class, GenerationClass::InFlight { .. }) {
            return Err(FoldError::NotTheOpenGeneration {
                kind: KIND,
                key: prepared.key.0,
                generation: prepared.generation.0,
                detail: format!(
                    "the generation is {}, and a candidate is prepared by a generation whose \
                     attempt is still in flight — this event is its settlement",
                    generation.class.name()
                ),
            });
        }
        // INV-06: "at most one candidate per generation", enforced_by "fold
        // refuses a second candidate for a generation". Refused here, before
        // any lease or candidate-state mutation could be planned: a second
        // record would replace the first and hand a later
        // `task_candidate_created` a candidate the queue never saw prepared.
        if generation.candidate.is_some() {
            return Err(FoldError::NotTheOpenGeneration {
                kind: KIND,
                key: prepared.key.0,
                generation: prepared.generation.0,
                detail: "the generation has already prepared a candidate, and one generation \
                         prepares at most one"
                    .to_owned(),
            });
        }
        // **And the attempt it names must have succeeded.** This event is the
        // sole successful settlement for a candidate-producing attempt, so a
        // record carrying a failure is a settlement contradicting itself: the
        // candidate's own authoritative evidence would say a gate failed while
        // the fold promoted the generation and carried it to
        // `task_candidate_created`, queueing it as a success.
        //
        // Missing until 2026-08-27. The Class B change made this the successful
        // settlement and did not make the fold require success — the semantic
        // condition that motivated the change was the one condition not
        // enforced, and the round-4 review of `09f9a99` walked the five steps.
        // It also gives `TopologyRun`'s `Brief::replay` the property it already
        // assumed: a `candidate_prepared` record never carries feedback,
        // because it never carries a failure.
        //
        // `InconsistentRecord` rather than a new variant: the refusal inventory
        // is packet-enumerated, and "the event disagrees with the record it
        // cites" is exactly this kind.
        if !prepared.attempt.is_successful() {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "attempt {} of generation {} does not record a successful attempt — \
                     failure {:?}, review outcomes {:?} — and `candidate_prepared` is the \
                     settlement of an attempt that succeeded",
                    prepared.attempt.attempt,
                    prepared.generation.0,
                    prepared.attempt.failure.as_ref().map(|f| f.kind),
                    prepared
                        .attempt
                        .reviews
                        .iter()
                        .map(|pass| pass.outcome)
                        .collect::<Vec<_>>()
                ),
            });
        }
        // **And it must have run the passes the run froze for this task.**
        // `is_successful` above asks `all` over *the passes the record happens
        // to carry*, which is a predicate the record's own author chooses the
        // domain of: a `candidate_prepared` carrying a lone passed
        // `second-opinion` — or an empty list — satisfies it, and the fold
        // charges the rung, enters `Promoting`, and permits
        // `task_candidate_created` for a tree the configured primary reviewer
        // never read. Round 6 of the `cfa1be8` review found it as its first P1;
        // that round fixed the *outcome* half — a pass recorded `Failed` or
        // `Unavailable` is refused — and this is the *presence* half.
        //
        // **Fold-side, and taking `(record, frozen)`.** The predicate needs the
        // plan and `AttemptRecord` does not carry it, so it cannot be a method
        // on the record; the entry is already in hand here for the lease and
        // lineage relations below.
        //
        // The comparison is the ordered list of pass names, so it refuses in
        // one place every way a record can disagree with its obligation: a
        // configured pass omitted, a pass duplicated, a pass nobody configured,
        // and the configured passes in another order. §11.3's own reason for
        // the order is that "a later pass only exists because every earlier one
        // approved" — a record whose second opinion precedes its acceptance
        // pass describes a review that did not happen.
        //
        // `FrozenReviews::obliged_lenses` is `review::passes_for`'s answer
        // rather than a second reading of §11.2/§11.3, and it is the same
        // reader the plan assembler dispatches from. That is the whole of why
        // this is safe to enforce: the obligation the fold requires and the
        // passes the driver runs are one derivation.
        let obliged: Vec<&str> = entry
            .reviews
            .obliged_lenses()
            .iter()
            .map(|lens| lens.name())
            .collect();
        let recorded: Vec<&str> = prepared
            .attempt
            .reviews
            .iter()
            .map(|pass| pass.pass.as_str())
            .collect();
        if recorded != obliged {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "attempt {} of generation {} records the review pass(es) {:?} and this task \
                     is frozen to require {:?}, in that order — every configured pass runs and \
                     passes, and a record does not choose which ones it is judged on",
                    prepared.attempt.attempt, prepared.generation.0, recorded, obliged
                ),
            });
        }
        // ST-06: a candidate is prepared *by the attempt that succeeded*, so
        // the embedded record names the generation's current attempt. Without
        // this the record is inert data and a candidate can be published
        // attributed to an attempt that did not produce it.
        if prepared.attempt.attempt != generation.attempts {
            return Err(FoldError::WrongAttempt {
                kind: KIND,
                key: prepared.key.0,
                generation: prepared.generation.0,
                attempt: prepared.attempt.attempt,
                expected: generation.attempts.to_string(),
            });
        }
        // INV-09 depends on this: the exact-base decision compares the
        // integration head against `base_sha` and then publishes `commit_sha`,
        // so a commit parented anywhere else would fast-forward the integration
        // ref onto history nobody judged.
        if !prepared.parent_is_base() {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "the candidate is parented on {} and the work started from {}",
                    prepared.parent_sha, prepared.base_sha
                ),
            });
        }
        if prepared.base_sha != generation.base_sha {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it records base {} and generation {} was dispatched at {}",
                    prepared.base_sha, prepared.generation.0, generation.base_sha
                ),
            });
        }
        match (&prepared.lease_effect, entry.lineage) {
            (CandidateLeaseEffect::ReplacesPredicted { paths }, None) => {
                if *paths != prepared.actual_paths {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: "the region it takes is not the region its diff touched".to_owned(),
                    });
                }
            }
            (CandidateLeaseEffect::WidensLineage { root, paths }, Some(lineage)) => {
                if *root != lineage.root {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "it widens lineage {root} and its task descends from {}",
                            lineage.root
                        ),
                    });
                }
                if *paths != prepared.actual_paths {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: "the region it widens by is not the region its diff touched"
                            .to_owned(),
                    });
                }
            }
            _ => {
                return Err(FoldError::InconsistentRecord {
                    kind: KIND,
                    detail: "a lineage member widens its lineage and an ordinary candidate \
                             replaces its predicted region; this does the other one"
                        .to_owned(),
                });
            }
        }
        Ok(())
    }

    // --- task_candidate_created --------------------------------------------

    fn check_candidate_created(&self, created: &TaskCandidateCreated) -> Result<(), FoldError> {
        const KIND: &str = "task_candidate_created";
        let candidate = &created.candidate;
        let task = self.task(KIND, candidate.key)?;
        let generation = self.open_generation(KIND, task, candidate.key, candidate.generation)?;
        // ST-06: a mismatched task_candidate_created.
        let prepared = match &generation.candidate {
            Some(prepared) if generation.class == GenerationClass::Promoting => prepared,
            _ => {
                return Err(FoldError::NotTheOpenGeneration {
                    kind: KIND,
                    key: candidate.key.0,
                    generation: candidate.generation.0,
                    detail: format!(
                        "the generation is {} and has prepared no candidate",
                        generation.class.name()
                    ),
                });
            }
        };
        if prepared.candidate != *candidate {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it promotes commit {} at `{}` and the prepared candidate is {} at `{}`",
                    candidate.commit_sha,
                    candidate.candidate_ref,
                    prepared.candidate.commit_sha,
                    prepared.candidate.candidate_ref
                ),
            });
        }
        Ok(())
    }

    // --- integration: starting a transaction --------------------------------

    /// The checks every first append of an integration transaction shares:
    /// nothing else is open, the sequence is the next dense one, and the
    /// candidate is the first *eligible* entry in the queue.
    fn check_transaction_start(
        &self,
        kind: &'static str,
        sequence: SequenceId,
        candidate: &CandidateRef,
    ) -> Result<&QueueEntry, FoldError> {
        // refusals[7]: one integration transaction at a time.
        if let Some(open) = &self.transaction {
            return Err(FoldError::TransactionAlreadyOpen {
                kind,
                sequence: sequence.0,
                open: open.sequence.0,
            });
        }
        // refusals[6] / refusals[10]: sequences are dense from 0 across the run.
        if sequence.0 != self.next_sequence {
            return Err(FoldError::NonDenseSequence {
                kind,
                sequence: sequence.0,
                next: self.next_sequence,
            });
        }
        // refusals[8]: the first eligible entry is integrated, and the fold
        // refuses an integration start for any other candidate.
        let first = self
            .queue
            .first_eligible(
                |key| self.task_is_awaiting_input(key),
                &self.leases,
                &self.started.path_policy,
            )
            .ok_or_else(|| FoldError::NotFirstEligible {
                kind,
                key: candidate.key.0,
                generation: candidate.generation.0,
                detail: "no queued candidate is eligible".to_owned(),
            })?;
        if first.candidate != *candidate {
            let detail = self
                .queue
                .get(candidate.key, candidate.generation)
                .map_or_else(
                    || "it holds no queue position at all".to_owned(),
                    |entry| {
                        CandidateQueue::ineligible(
                            entry,
                            &|key| self.task_is_awaiting_input(key),
                            &self.leases,
                            &self.started.path_policy,
                        )
                        .map_or_else(
                            || {
                                format!(
                                    "task {} generation {} is queued ahead of it and eligible",
                                    first.key().0,
                                    first.generation().0
                                )
                            },
                            |why| format!("it is not eligible: {}", ineligible_detail(why)),
                        )
                    },
                );
            return Err(FoldError::NotFirstEligible {
                kind,
                key: candidate.key.0,
                generation: candidate.generation.0,
                detail,
            });
        }
        Ok(first)
    }

    fn task_is_awaiting_input(&self, key: TaskKey) -> bool {
        self.tasks
            .get(key.index())
            .is_some_and(|task| task.state == TaskState::AwaitingInput)
    }

    /// The open transaction this event must belong to (refusals[6]).
    fn open_transaction(
        &self,
        kind: &'static str,
        sequence: SequenceId,
    ) -> Result<&Transaction, FoldError> {
        let open = self
            .transaction
            .as_ref()
            .ok_or_else(|| FoldError::WrongSequence {
                kind,
                sequence: sequence.0,
                open: "none".to_owned(),
            })?;
        if open.sequence != sequence {
            return Err(FoldError::WrongSequence {
                kind,
                sequence: sequence.0,
                open: open.sequence.0.to_string(),
            });
        }
        Ok(open)
    }

    // --- merge_verification_started ----------------------------------------

    fn check_verification_started(
        &self,
        started: &MergeVerificationStarted,
    ) -> Result<(), FoldError> {
        const KIND: &str = "merge_verification_started";
        let queued = self.check_transaction_start(KIND, started.sequence, &started.candidate)?;
        let prepared = self.prepared_candidate(KIND, &started.candidate)?;

        // INV-09: the exact-base decision is made before any staging effect, so
        // a candidate whose base *is* the head is published fast and is never
        // cherry-picked or re-verified.
        if started.expected_head == prepared.base_sha {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "the head is {} and the candidate's base is the same commit, which is the \
                     exact-base case and publishes the candidate itself",
                    started.expected_head
                ),
            });
        }
        let _ = queued;
        match &started.basis {
            VerificationBasis::AlreadyPresent => {
                if started.proposed_sha != started.expected_head {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "an already-present verification judges the head itself, and this one \
                             judges {} against head {}",
                            started.proposed_sha, started.expected_head
                        ),
                    });
                }
            }
            VerificationBasis::StaleClean { .. } => {
                if started.proposed_sha == started.expected_head {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: "a stale-clean verification judges the proposal the cherry-pick \
                                 produced, and this one judges the head"
                            .to_owned(),
                    });
                }
            }
        }
        Ok(())
    }

    /// What `candidate_prepared` recorded for this candidate.
    fn prepared_candidate(
        &self,
        kind: &'static str,
        candidate: &CandidateRef,
    ) -> Result<&PreparedCandidate, FoldError> {
        let task = self.task(kind, candidate.key)?;
        task.generations
            .iter()
            .filter_map(|generation| generation.candidate.as_ref())
            .find(|prepared| prepared.candidate.generation == candidate.generation)
            .filter(|prepared| prepared.candidate == *candidate)
            .ok_or_else(|| FoldError::InconsistentRecord {
                kind,
                detail: format!(
                    "no `candidate_prepared` in this log records task {} generation {} as commit \
                     {}",
                    candidate.key.0, candidate.generation.0, candidate.commit_sha
                ),
            })
    }

    // --- merge_verification_unavailable ------------------------------------

    fn check_verification_unavailable(
        &self,
        unavailable: &MergeVerificationUnavailable,
    ) -> Result<(), FoldError> {
        const KIND: &str = "merge_verification_unavailable";
        let transaction = self.open_transaction(KIND, unavailable.sequence)?;
        if !matches!(
            transaction.class,
            TransactionClass::VerificationStarted { .. }
        ) {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: "the transaction is already authorized to publish; an outage refuses a \
                         verification that is still running"
                    .to_owned(),
            });
        }
        unavailable
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: KIND,
                detail: defect.to_string(),
            })?;

        let queued = self
            .queue
            .get(transaction.candidate.key, transaction.candidate.generation)
            .ok_or_else(|| FoldError::InconsistentRecord {
                kind: KIND,
                detail: "the candidate under verification holds no queue position".to_owned(),
            })?;
        // The boundary is the same number read from both sides: the deferral
        // this outage *would* be. `coordinator_integration.dispositions` gives
        // Infrastructure `Deferred{defers}` while `defers < max_defers` and
        // `Parked{question}` at `max_defers`, so the two arms partition on
        // `next` and neither may take the other's cell.
        let max = self.started.limits.max_defers;
        let next = queued.defers.saturating_add(1);
        match &unavailable.outcome {
            UnavailableOutcome::Deferred { defers } => {
                // refusals[17]: consecutive, and within the frozen allowance.
                if *defers != next {
                    return Err(FoldError::InvalidDefers {
                        defers: *defers,
                        detail: format!(
                            "this candidate has been deferred {} time(s), so the next deferral is \
                             {next}",
                            queued.defers,
                        ),
                    });
                }
                // refusals[16]: "Deferred at max_defers" is refused. The
                // allowance is the number of deferrals the run may *take*, so
                // the last one it may take is `max_defers - 1` and the outage
                // that would be the `max_defers`th parks instead.
                if *defers >= max {
                    return Err(FoldError::InvalidDefers {
                        defers: *defers,
                        detail: format!(
                            "this run allows {max}, and the {max}th outage parks rather than \
                             defers"
                        ),
                    });
                }
            }
            UnavailableOutcome::Parked { question } => {
                self.check_new_question(KIND, question, transaction.candidate.key)?;
                // refusals[16], the other half: `HumanRequired` always parks,
                // whatever the count, and an Infrastructure outage parks
                // exactly at the boundary — one earlier would consume an
                // allowance the run still has.
                if matches!(unavailable.cause, UnavailableCause::Infrastructure { .. })
                    && next != max
                {
                    return Err(FoldError::InvalidDefers {
                        defers: next,
                        detail: format!(
                            "an infrastructure outage parks at {max} deferral(s) and this \
                             candidate has been deferred {} time(s), so this one defers",
                            queued.defers
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    // --- merge_verification_interrupted ------------------------------------

    fn check_verification_interrupted(
        &self,
        interrupted: &MergeVerificationInterrupted,
    ) -> Result<(), FoldError> {
        const KIND: &str = "merge_verification_interrupted";
        let transaction = self.open_transaction(KIND, interrupted.sequence)?;
        if !matches!(
            transaction.class,
            TransactionClass::VerificationStarted { .. }
        ) {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: "the transaction is already authorized to publish; an authorized \
                         publication is completed, never abandoned"
                    .to_owned(),
            });
        }
        Ok(())
    }

    // --- merge_prepared ----------------------------------------------------

    fn check_merge_prepared(&self, prepared: &MergePrepared) -> Result<(), FoldError> {
        const KIND: &str = "merge_prepared";
        // A1's intra-event relations first: a record that disagrees with itself
        // is refused before it is compared with anything else.
        prepared
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: KIND,
                detail: defect.to_string(),
            })?;

        let candidate_record = self.prepared_candidate(KIND, &prepared.candidate())?;
        let inconsistent = |detail: String| FoldError::InconsistentRecord { kind: KIND, detail };

        match prepared.disposition {
            PreparedDisposition::Fast => {
                // A fast publication opens and closes its own transaction: no
                // verification ran, so there is nothing already open.
                self.check_transaction_start(KIND, prepared.sequence, &prepared.candidate())?;
                // refusals[9]: expected_head == the candidate's recorded base,
                // proposed_sha == the candidate's recorded commit.
                if prepared.expected_head != candidate_record.base_sha {
                    return Err(inconsistent(format!(
                        "a fast publication expects the head to be the candidate's base {} and \
                         this one expects {}",
                        candidate_record.base_sha, prepared.expected_head
                    )));
                }
                if prepared.proposed_sha != candidate_record.candidate.commit_sha {
                    return Err(inconsistent(format!(
                        "it publishes {} and the candidate's recorded commit is {}",
                        prepared.proposed_sha, candidate_record.candidate.commit_sha
                    )));
                }
                match &prepared.verification_source {
                    VerificationSource::CandidatePrepared { key, generation } => {
                        if *key != prepared.key || *generation != prepared.generation {
                            return Err(inconsistent(format!(
                                "it cites the record of task {} generation {} and publishes task \
                                 {} generation {}",
                                key.0, generation.0, prepared.key.0, prepared.generation.0
                            )));
                        }
                    }
                    VerificationSource::Verification { .. } => {
                        return Err(inconsistent(
                            "a fast publication cites the candidate's own record".to_owned(),
                        ));
                    }
                }
            }
            PreparedDisposition::StaleClean | PreparedDisposition::AlreadyPresent => {
                let transaction = self.open_transaction(KIND, prepared.sequence)?;
                let TransactionClass::VerificationStarted {
                    basis,
                    expected_head,
                    proposed_sha,
                } = &transaction.class
                else {
                    return Err(inconsistent(
                        "the transaction is already authorized to publish".to_owned(),
                    ));
                };
                if transaction.candidate != prepared.candidate() {
                    return Err(inconsistent(format!(
                        "it publishes task {} generation {} and the open transaction is verifying \
                         task {} generation {}",
                        prepared.key.0,
                        prepared.generation.0,
                        transaction.candidate.key.0,
                        transaction.candidate.generation.0
                    )));
                }
                let stale = prepared.disposition == PreparedDisposition::StaleClean;
                if stale != matches!(basis, VerificationBasis::StaleClean { .. }) {
                    return Err(inconsistent(
                        "the disposition it publishes under is not the basis its verification ran \
                         on"
                        .to_owned(),
                    ));
                }
                // refusals[22], fold half: the head the CAS expects is the head
                // the transaction read.
                if prepared.expected_head != *expected_head {
                    return Err(inconsistent(format!(
                        "it expects head {} and the verification recorded head {expected_head}",
                        prepared.expected_head
                    )));
                }
                // refusals[9]: the proposal is the one that was verified — the
                // pinned proposal for a stale publication, the head itself for
                // an already-present one.
                if prepared.proposed_sha != *proposed_sha {
                    return Err(inconsistent(format!(
                        "it publishes {} and the verification judged {proposed_sha}",
                        prepared.proposed_sha
                    )));
                }
                if let VerificationBasis::StaleClean { prepared_ref } = basis {
                    if prepared.prepared_ref.as_ref() != Some(prepared_ref) {
                        return Err(inconsistent(format!(
                            "it pins the proposal at {:?} and the verification pinned it at `{}`",
                            prepared.prepared_ref.as_ref().map(GitRefName::name),
                            prepared_ref
                        )));
                    }
                }
                match &prepared.verification_source {
                    VerificationSource::Verification { sequence } => {
                        if *sequence != prepared.sequence {
                            return Err(inconsistent(format!(
                                "it cites verification {} and belongs to transaction {}",
                                sequence.0, prepared.sequence.0
                            )));
                        }
                    }
                    VerificationSource::CandidatePrepared { .. } => {
                        return Err(inconsistent(
                            "a verified publication cites the verification that judged what it \
                             publishes"
                                .to_owned(),
                        ));
                    }
                }
            }
        }

        // refusals[10]: the closure this publication settles is derived, not
        // asserted.
        let derived = self.satisfies_closure(prepared.key);
        if prepared.satisfies != derived {
            return Err(FoldError::InvalidSatisfies {
                kind: KIND,
                recorded: prepared.satisfies.iter().map(|key| key.0).collect(),
                derived: derived.iter().map(|key| key.0).collect(),
            });
        }
        Ok(())
    }

    /// Every task one publication settles: the candidate's own task and, for a
    /// repair, every entry back up its lineage to the root.
    ///
    /// A repair carries the work of everything it descends from — that is what
    /// it was materialized from — so publishing it settles the whole chain.
    /// Ascending key order, because the value is derived and two readers must
    /// derive the same list.
    fn satisfies_closure(&self, key: TaskKey) -> Vec<TaskKey> {
        let mut chain = vec![key];
        let mut current = key;
        while let Some(lineage) = self.registry.get(current).and_then(|entry| entry.lineage) {
            if lineage.parent >= current {
                break;
            }
            chain.push(lineage.parent);
            current = lineage.parent;
        }
        chain.sort_unstable();
        chain.dedup();
        chain
    }

    // --- merge_rejected ----------------------------------------------------

    fn check_merge_rejected(&self, rejected: &MergeRejected) -> Result<(), FoldError> {
        const KIND: &str = "merge_rejected";
        let inconsistent = |detail: String| FoldError::InconsistentRecord { kind: KIND, detail };
        match &rejected.disposition {
            RejectionDisposition::Conflict { .. } => {
                // A conflict is decided at the cherry-pick, before any
                // verification starts: it opens and closes its own transaction.
                self.check_transaction_start(KIND, rejected.sequence, &rejected.candidate)?;
            }
            RejectionDisposition::CodeRejected { verification } => {
                let transaction = self.open_transaction(KIND, rejected.sequence)?;
                let TransactionClass::VerificationStarted { expected_head, .. } =
                    &transaction.class
                else {
                    return Err(inconsistent(
                        "the transaction is already authorized to publish".to_owned(),
                    ));
                };
                if transaction.candidate != rejected.candidate {
                    return Err(inconsistent(format!(
                        "it rejects task {} generation {} and the open transaction is verifying \
                         task {} generation {}",
                        rejected.candidate.key.0,
                        rejected.candidate.generation.0,
                        transaction.candidate.key.0,
                        transaction.candidate.generation.0
                    )));
                }
                if rejected.rejecting_head != *expected_head {
                    return Err(inconsistent(format!(
                        "it was judged against head {} and the verification recorded head \
                         {expected_head}",
                        rejected.rejecting_head
                    )));
                }
                if verification.verdict == VerificationVerdict::Passed {
                    return Err(inconsistent(
                        "a code rejection carries the verification that rejected it, and this one \
                         passed"
                            .to_owned(),
                    ));
                }
            }
        }

        // The lease effect and the repair are one decision: a non-lineage
        // candidate's lease becomes the new lineage's, and a lineage member's
        // rejection widens the lineage it already belongs to.
        let entry = self.entry(KIND, rejected.candidate.key)?;
        let root = match (&rejected.lease_effect, entry.lineage) {
            (RejectionLeaseEffect::CreatesLineage { root, .. }, None) => {
                if *root != rejected.candidate.key {
                    return Err(inconsistent(format!(
                        "it creates lineage {root} from the rejection of task {}",
                        rejected.candidate.key.0
                    )));
                }
                *root
            }
            (RejectionLeaseEffect::WidensLineage { root, .. }, Some(lineage)) => {
                if *root != lineage.root {
                    return Err(inconsistent(format!(
                        "it widens lineage {root} and the rejected task descends from {}",
                        lineage.root
                    )));
                }
                *root
            }
            _ => {
                return Err(inconsistent(
                    "a rejection creates a lineage from an ordinary candidate and widens the \
                     lineage of a member; this does the other one"
                        .to_owned(),
                ));
            }
        };

        self.check_spawn(&rejected.repair, KIND)?;
        let lineage =
            rejected.repair.entry.lineage.ok_or_else(|| {
                inconsistent("the repair it registers records no lineage".to_owned())
            })?;
        if lineage.root != root {
            return Err(inconsistent(format!(
                "the repair descends from lineage {} and the rejection widens {root}",
                lineage.root
            )));
        }
        if lineage.parent != rejected.candidate.key {
            return Err(inconsistent(format!(
                "the repair's parent is {} and the rejected candidate is task {}",
                lineage.parent, rejected.candidate.key.0
            )));
        }
        let index = self.lineage_members(root);
        if lineage.index != index {
            return Err(inconsistent(format!(
                "the repair is the {} member of lineage {root} and records index {}",
                ordinal(index),
                lineage.index
            )));
        }
        Ok(())
    }

    /// How many repairs lineage `root` already holds.
    fn lineage_members(&self, root: TaskKey) -> u32 {
        u32::try_from(
            self.registry
                .entries()
                .iter()
                .filter(|entry| entry.lineage.is_some_and(|lineage| lineage.root == root))
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    // --- task_merged -------------------------------------------------------

    fn check_task_merged(&self, merged: &TaskMerged) -> Result<(), FoldError> {
        const KIND: &str = "task_merged";
        let transaction = self.open_transaction(KIND, merged.sequence)?;
        let TransactionClass::Prepared {
            proposed_sha,
            satisfies,
        } = &transaction.class
        else {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: "the integration ref moves only after `merge_prepared`, and this \
                         transaction has not authorized a publication"
                    .to_owned(),
            });
        };
        if merged.merged_sha != *proposed_sha {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "the ref now points at {} and the authorization proposed {proposed_sha}",
                    merged.merged_sha
                ),
            });
        }
        // "copied exactly from the authorization", not re-derived here.
        if merged.satisfies != *satisfies {
            return Err(FoldError::InvalidSatisfies {
                kind: KIND,
                recorded: merged.satisfies.iter().map(|key| key.0).collect(),
                derived: satisfies.iter().map(|key| key.0).collect(),
            });
        }
        let root_settled = self
            .registry
            .get(transaction.candidate.key)
            .and_then(|entry| entry.lineage)
            .map(|lineage| lineage.root);
        match (&merged.lease_release, root_settled) {
            (MergeLeaseRelease::Candidate { key, generation }, None) => {
                if *key != transaction.candidate.key
                    || *generation != transaction.candidate.generation
                {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "it releases the lease of task {} generation {} and publishes task {} \
                             generation {}",
                            key.0,
                            generation.0,
                            transaction.candidate.key.0,
                            transaction.candidate.generation.0
                        ),
                    });
                }
            }
            (MergeLeaseRelease::Lineage { root }, Some(settled)) => {
                if *root != settled {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!("it releases lineage {root} and settles lineage {settled}"),
                    });
                }
            }
            _ => {
                return Err(FoldError::InconsistentRecord {
                    kind: KIND,
                    detail: "a publication releases the candidate's lease, or the lineage lease \
                             when it settles that lineage's root; this releases the other one"
                        .to_owned(),
                });
            }
        }
        Ok(())
    }

    // --- questions ---------------------------------------------------------

    fn check_question_raised(&self, question: &FrozenQuestion) -> Result<(), FoldError> {
        const KIND: &str = "question_raised";
        self.entry(KIND, question.key)?;
        self.check_new_question(KIND, question, question.key)
    }

    fn check_question_answered(
        &self,
        answered: &QuestionAnswered4,
    ) -> Result<QuestionOrigin, FoldError> {
        const KIND: &str = "question_answered";
        // refusals[20]: answers are not ingested in an epoch after a halting
        // settlement or a budget stop.
        if self.halted_epoch == Some(self.epoch) {
            return Err(FoldError::RunEnding {
                kind: KIND,
                what: "a halting settlement",
            });
        }
        if self.budget_stop_is_current() {
            return Err(FoldError::RunEnding {
                kind: KIND,
                what: "the budget stop",
            });
        }
        // refusals[13], A1's half: the answer must agree with itself.
        answered
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: KIND,
                detail: defect.to_string(),
            })?;

        let open =
            self.questions
                .get(&answered.question)
                .ok_or_else(|| FoldError::WrongQuestion {
                    kind: KIND,
                    question: answered.question.to_string(),
                    detail: if self.seen_questions.contains(&answered.question) {
                        "has already been answered; a question is answered once".to_owned()
                    } else {
                        "this log never asked".to_owned()
                    },
                })?;
        if open.question.key != answered.key {
            return Err(FoldError::WrongQuestion {
                kind: KIND,
                question: answered.question.to_string(),
                detail: format!(
                    "was asked about task {} and this answers it for task {}",
                    open.question.key, answered.key
                ),
            });
        }
        if let Answer4::Answered {
            option_index,
            binding_override,
        } = &answered.answer
        {
            let options = open.question.options.len();
            let chosen = usize::try_from(*option_index).unwrap_or(usize::MAX);
            if chosen >= options {
                return Err(FoldError::WrongQuestion {
                    kind: KIND,
                    question: answered.question.to_string(),
                    detail: format!("offered {options} option(s) and this chose {option_index}"),
                });
            }
            // refusals[12] / `task_registry.binding_override`: an override is
            // validated "against the frozen options of that task's open
            // HumanBinding question". A1's `self_consistency` has already
            // proved the override names this answer's task, question and
            // option; what is left — and what no other check makes — is that
            // there *is* such an authority and that the agent it names is the
            // one that authority froze at that index.
            match (binding_override, &open.binding) {
                (Some(_), None) => {
                    return Err(FoldError::WrongQuestion {
                        kind: KIND,
                        question: answered.question.to_string(),
                        detail: "carries a binding override and did not ask for a binding; only a \
                                 HumanBinding admission authorizes one"
                            .to_owned(),
                    });
                }
                (None, Some(_)) => {
                    return Err(FoldError::WrongQuestion {
                        kind: KIND,
                        question: answered.question.to_string(),
                        detail: "asked for a binding and this answer names none, so its task has \
                                 no binding to run"
                            .to_owned(),
                    });
                }
                (Some(binding), Some(authorized)) => {
                    let Some(agent) = authorized.get(chosen) else {
                        return Err(FoldError::WrongQuestion {
                            kind: KIND,
                            question: answered.question.to_string(),
                            detail: format!(
                                "authorized {} binding(s) and this chose {option_index}",
                                authorized.len()
                            ),
                        });
                    };
                    if binding.agent != *agent {
                        return Err(FoldError::WrongQuestion {
                            kind: KIND,
                            question: answered.question.to_string(),
                            detail: format!(
                                "authorized `{agent}` at option {option_index} and the override \
                                 names `{}`",
                                binding.agent
                            ),
                        });
                    }
                }
                (None, None) => {}
            }
        }
        Ok(open.origin)
    }

    // --- budget_exceeded ---------------------------------------------------

    fn check_budget_exceeded(&self, exceeded: &BudgetExceeded4) -> Result<(), FoldError> {
        const KIND: &str = "budget_exceeded";
        if let Some(key) = exceeded.key {
            self.entry(KIND, key)?;
        }
        if exceeded.epoch != self.epoch {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it belongs to epoch {} and this run is in epoch {}",
                    exceeded.epoch.0, self.epoch.0
                ),
            });
        }
        Ok(())
    }

    // --- run_finished ------------------------------------------------------

    fn check_run_finished(&self, finished: &RunFinished4) -> Result<(), FoldError> {
        // refusals[19] / INV-15: the recorded outcome is the derived one, and
        // the derived one is not NotEnding.
        let derived = self.derived_outcome();
        let matches = match &derived {
            DerivedOutcome::Ending(outcome) => *outcome == finished.outcome,
            DerivedOutcome::NotEnding | DerivedOutcome::FoldError => false,
        };
        if !matches {
            return Err(FoldError::OutcomeMismatch {
                recorded: outcome_name(&finished.outcome),
                derived: match &derived {
                    DerivedOutcome::NotEnding => "not ending".to_owned(),
                    DerivedOutcome::Ending(outcome) => outcome_name(outcome).to_owned(),
                    DerivedOutcome::FoldError => "unreachable".to_owned(),
                },
            });
        }
        if finished.halted_at != self.halted_at {
            return Err(FoldError::InconsistentRecord {
                kind: "run_finished",
                detail: format!(
                    "it attributes the halt to {:?} and the fold recorded {:?}",
                    finished.halted_at.map(|key| key.0),
                    self.halted_at.map(|key| key.0)
                ),
            });
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // derived_outcome
    // -----------------------------------------------------------------------

    fn derived_outcome(&self) -> DerivedOutcome {
        if !self.common() {
            return DerivedOutcome::NotEnding;
        }
        if self.halted_at.is_some() {
            return DerivedOutcome::Ending(RunOutcome::Halted);
        }
        if self.budget_stop_is_current() {
            return DerivedOutcome::Ending(RunOutcome::BudgetExceeded);
        }
        if self.structurally_admissible() || self.backoff_pending() {
            return DerivedOutcome::NotEnding;
        }
        if self.questions_open() {
            return DerivedOutcome::Ending(RunOutcome::Parked);
        }
        if self.complete_shape() {
            return DerivedOutcome::Ending(RunOutcome::Complete);
        }
        DerivedOutcome::FoldError
    }

    /// No generation is open and no integration transaction is unresolved.
    fn common(&self) -> bool {
        self.tasks.iter().all(|task| {
            task.open()
                .is_none_or(|generation| !generation.class.blocks_run_end())
        }) && self.transaction.is_none()
    }

    /// Some task could be dispatched, retried, or integrated from this state
    /// alone. Budget, capacity and runner availability are not consulted.
    fn structurally_admissible(&self) -> bool {
        (0..self.tasks.len())
            .map(|index| TaskKey(u32::try_from(index).unwrap_or(u32::MAX)))
            .any(|key| self.ready(key) || self.ready_retry(key))
            || self.integration_admissible()
    }

    fn ready(&self, key: TaskKey) -> bool {
        let (Some(task), Some(entry)) = (self.tasks.get(key.index()), self.registry.get(key))
        else {
            return false;
        };
        task.state == TaskState::Pending
            && task.open().is_none()
            && entry.deps.iter().all(|dep| {
                self.tasks
                    .get(dep.index())
                    .is_some_and(|dep| dep.state == TaskState::Merged)
            })
            && self.open_question_for(key).is_none()
            && !self.queue.holds_task(key)
            && self
                .transaction
                .as_ref()
                .is_none_or(|open| open.candidate.key != key)
            && self.dispatch_lease_check(key, entry)
            && self.pipeline_reservable()
            && !self.run_is_ending()
    }

    /// A repair dispatch is never lease-blocked; an ordinary one is blocked by
    /// any overlapping active lease of another owner.
    ///
    /// The predicted region is not in the log until the dispatch that takes it,
    /// so the check the *fold* can make is over the run's own leases: a task
    /// with a repo-wide prediction is admissible exactly when nothing is held.
    fn dispatch_lease_check(&self, key: TaskKey, entry: &TaskEntry) -> bool {
        if entry.lineage.is_some() {
            return true;
        }
        let predicted = predicted_region(entry);
        !self.leases.overlaps_another(
            LeaseOwner::Generation {
                key,
                generation: GenerationId(
                    u32::try_from(
                        self.tasks
                            .get(key.index())
                            .map_or(0, |task| task.generations.len()),
                    )
                    .unwrap_or(u32::MAX),
                ),
            },
            &predicted,
            &self.started.path_policy,
        )
    }

    fn ready_retry(&self, key: TaskKey) -> bool {
        let Some(task) = self.tasks.get(key.index()) else {
            return false;
        };
        let retained = task.open().is_some_and(|generation| {
            matches!(
                &generation.class,
                GenerationClass::RetainedIdle { incarnation, .. } if *incarnation == self.epoch
            )
        });
        task.state == TaskState::Pending
            && retained
            && self.open_question_for(key).is_none()
            && self
                .transaction
                .as_ref()
                .is_none_or(|open| open.candidate.key != key)
            && self.pipeline_reservable()
            && !self.run_is_ending()
    }

    fn pipeline_reservable(&self) -> bool {
        self.pipeline_held()
            < usize::try_from(self.started.limits.max_parallel).unwrap_or(usize::MAX)
    }

    /// `permits.provisional_reservations` gives integration selection the
    /// `{pipeline, merge}` pair, and `deadlock_freedom` takes a reservation
    /// "only when the derived count permits" — so the entitlement is a clause
    /// of admissibility here for the same reason it is one in [`Self::ready`]
    /// and [`Self::ready_retry`], and not a check the caller is trusted to
    /// remember. `permits.pipeline` counts an unresolved integration
    /// transaction among the held, which is the other half of the same
    /// statement: a selector that admitted an integration while the count was
    /// at `max_parallel` would open the entitlement that is already held.
    fn integration_admissible(&self) -> bool {
        self.transaction.is_none()
            && self.pipeline_reservable()
            && !self.run_is_ending()
            && self
                .queue
                .first_eligible(
                    |key| self.task_is_awaiting_input(key),
                    &self.leases,
                    &self.started.path_policy,
                )
                .is_some()
    }

    fn backoff_pending(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.state == TaskState::Deferred)
            || self
                .queue
                .entries()
                .iter()
                .any(|entry| entry.verification_deferred)
    }

    fn questions_open(&self) -> bool {
        !self.questions.is_empty()
    }

    fn complete_shape(&self) -> bool {
        let blocked = self.blocked_tasks();
        self.tasks.iter().enumerate().all(|(index, task)| {
            task.state.is_terminal()
                || (task.state == TaskState::Pending && blocked.contains(&index))
        }) && self.queue.is_empty()
            && !self.leases.any_candidate_or_lineage()
    }

    /// Every task that can never run because a failure sits in its transitive
    /// dependency closure.
    fn blocked_tasks(&self) -> BTreeSet<usize> {
        let mut blocked = BTreeSet::new();
        // To a fixed point, not in one pass. A *repair*'s dependencies refer
        // only backwards, but an original's keys are assigned in plan order
        // (`keys_by_display_id`) and plan order is not topological order, so
        // an ordinary plan can have a task depend on a later key. One forward
        // pass would then decide that task before it had decided what the task
        // waits on, and a failure two hops away would go unseen — which is the
        // difference between "directly failed dependency" and the transitive
        // closure the packet asks for.
        //
        // Each round adds at least one member or stops, and membership only
        // grows, so this runs at most `tasks.len()` rounds.
        loop {
            let mut grew = false;
            for (index, task) in self.tasks.iter().enumerate() {
                if task.state != TaskState::Pending || blocked.contains(&index) {
                    continue;
                }
                let Some(entry) = self
                    .registry
                    .get(TaskKey(u32::try_from(index).unwrap_or(u32::MAX)))
                else {
                    continue;
                };
                let poisoned = entry.deps.iter().any(|dep| {
                    blocked.contains(&dep.index())
                        || self
                            .tasks
                            .get(dep.index())
                            .is_some_and(|dep| dep.state == TaskState::Failed)
                });
                if poisoned {
                    blocked.insert(index);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        blocked
    }
}

// ---------------------------------------------------------------------------
// RunState: the application
// ---------------------------------------------------------------------------

impl RunState {
    /// Apply a transition the check accepted.
    ///
    /// Total by construction: every lookup here was proved to succeed by the
    /// check that produced the delta, and each one is written so that a miss
    /// leaves the state alone rather than panicking. Nothing in this function
    /// decides anything — a decision made here would be a decision the live
    /// path and the replay path could reach differently, which is the one thing
    /// INV-02 forbids.
    #[allow(clippy::too_many_lines)]
    fn apply(&mut self, body: &TopologyEventBody, derived: &Derived) {
        match body {
            TopologyEventBody::RunStarted { .. } => {}
            TopologyEventBody::RunResumed { data } => self.apply_resumed(data),
            TopologyEventBody::TaskSpawned { data } => self.register(&data.spawn),
            TopologyEventBody::TaskDispatched { data } => self.apply_dispatched(data),
            TopologyEventBody::AttemptStarted { data } => {
                if let Some(generation) = self.open_generation_mut(data.key) {
                    generation.class = GenerationClass::InFlight {
                        attempt: data.attempt,
                    };
                    generation.attempts = data.attempt.0;
                }
                // **Not counted here.** An attempt that has *started* has not
                // yet spent anything: `ladder::spends_allowance` is total over
                // `FailureKind` and its line is "the worker ran and produced
                // work to judge", which `attempt_started` cannot know. Counting
                // here made this fold a second authority for a rule that has one
                // production implementation, and made every interruption, park
                // and outage burn a rung the packet says they do not —
                // `transaction_fault_matrix[T-ATTEMPT]`'s "unknown spend,
                // **allowance refunded**". The count is taken at the settlement,
                // in `apply_settlement`, from the record the settlement carries.
            }
            TopologyEventBody::AttemptFinished { data } => self.apply_settlement(data),
            TopologyEventBody::AttemptInterrupted { data } => {
                // T-ATTEMPT: generation Closed, task Pending, later dispatch a
                // new generation. The close releases the ordinary generation's
                // own region exactly as every other closing settlement does.
                self.close_generation(data.key);
                self.set_state(data.key, TaskState::Pending);
            }
            TopologyEventBody::GenerationClosed { data } => {
                self.close_generation(data.key);
            }
            TopologyEventBody::DeferWaitElapsed { .. } => self.wake_backoff(),
            TopologyEventBody::CandidatePrepared { data } => self.apply_candidate_prepared(data),
            TopologyEventBody::TaskCandidateCreated { data } => {
                self.apply_candidate_created(&data.candidate);
            }
            TopologyEventBody::MergeVerificationStarted { data } => {
                self.apply_verification_started(data);
            }
            TopologyEventBody::MergeVerificationUnavailable { data } => {
                self.apply_verification_unavailable(data);
            }
            TopologyEventBody::MergeVerificationInterrupted { .. } => {
                self.release_transaction();
            }
            TopologyEventBody::MergePrepared { data } => self.apply_merge_prepared(data),
            TopologyEventBody::MergeRejected { data } => self.apply_merge_rejected(data),
            TopologyEventBody::TaskMerged { data } => self.apply_task_merged(data),
            TopologyEventBody::QuestionRaised { data } => {
                // A bare `question_raised` carries no admission and so
                // authorizes no binding.
                self.open_question(&data.question, QuestionOrigin::Admission, None);
                self.set_state(data.question.key, TaskState::AwaitingInput);
            }
            TopologyEventBody::QuestionAnswered { data } => self.apply_answer(data, derived),
            TopologyEventBody::BudgetExceeded { data } => {
                if !self.budget_stop_is_current() {
                    self.budget_stop = Some(data.stop());
                }
            }
            TopologyEventBody::RunFinished { data } => {
                self.finished = Some(data.outcome.clone());
            }
            TopologyEventBody::CapacitySnapshot { .. }
            | TopologyEventBody::PoolExhausted { .. }
            | TopologyEventBody::DesignDefect { .. } => {}
        }
    }

    fn apply_resumed(&mut self, resumed: &RunResumed4) {
        self.epoch = Epoch(self.epoch.0.saturating_add(1));
        self.incarnation = resumed.incarnation.clone();
        // The stop belongs to the epoch that hit the old ceiling; the next
        // epoch starts without one, which is what makes "raise the budget and
        // resume" the response to it.
        self.budget_stop = None;
        self.finished = None;
        // Deferred items are woken by a resume exactly as they are by an
        // elapsed wait.
        self.wake_backoff();
    }

    fn wake_backoff(&mut self) {
        self.queue.wake_deferred();
        for task in &mut self.tasks {
            if task.state == TaskState::Deferred {
                task.state = TaskState::Pending;
            }
        }
    }

    fn register(&mut self, spawn: &FrozenSpawn) {
        self.registry.register(spawn.entry.clone());
        self.tasks.push(TaskFold::new());
        match &spawn.admission {
            SpawnAdmission::Runnable => {}
            SpawnAdmission::HumanRequired { question, .. } => {
                self.open_question(question, QuestionOrigin::Admission, None);
                self.set_state(spawn.key, TaskState::AwaitingInput);
            }
            SpawnAdmission::HumanBinding { options, question } => {
                // The one admission that authorizes an override, and the one
                // place its option list is frozen.
                self.open_question(question, QuestionOrigin::Admission, Some(options.clone()));
                self.set_state(spawn.key, TaskState::AwaitingInput);
            }
        }
    }

    fn apply_dispatched(&mut self, dispatched: &TaskDispatched) {
        // The recorded region and `predicted_region(entry)` are one value by the
        // time this runs: `check_dispatched` refuses an ordinary dispatch whose
        // `Predicted { paths }` is anything else. Granting the event's copy is
        // therefore granting the derivation, and it stays the event's copy so
        // that the region in the lease table is demonstrably the region the log
        // holds rather than a second derivation of it.
        let (lease, region) = match &dispatched.lease {
            LeaseGrant::Predicted { paths } => (GenerationLease::Own, Some(paths.clone())),
            LeaseGrant::InheritedLineage { root } => {
                (GenerationLease::InheritedLineage { root: *root }, None)
            }
        };
        if let Some(paths) = region {
            self.leases.grant(
                LeaseOwner::Generation {
                    key: dispatched.key,
                    generation: dispatched.generation,
                },
                paths,
            );
        }
        if let Some(task) = self.tasks.get_mut(dispatched.key.index()) {
            task.generations.push(GenerationFold {
                id: dispatched.generation,
                class: GenerationClass::OpenNoAttempt,
                base_sha: dispatched.base_sha.clone(),
                lease,
                attempts: 0,
                candidate: None,
            });
        }
    }

    fn apply_settlement(&mut self, finished: &AttemptFinished4) {
        // **The allowance, decided once, by the one function that decides it.**
        //
        // `ladder::spends_allowance` is documented as "the single production
        // implementation of the allowance rule" and is total over `FailureKind`
        // so a new variant stops the build rather than taking a default. This
        // fold consumes it; it does not re-derive it. `FailureRecord::shape`
        // exists for exactly this call — "a settlement holds a record rather
        // than the live failure, and the allowance decision is the same decision
        // either way".
        //
        // **Taken at the settlement, which is what makes the refund free.**
        // T-ATTEMPT refunds an interrupted attempt's allowance. An attempt that
        // never settled never counted, so there is nothing to give back and no
        // second rule to keep in step with the first — the refund is the absence
        // of a charge rather than a subtraction that could be forgotten.
        //
        // Before the `Escalated` arm below, which resets the count: an attempt
        // that escalates spent its allowance on the rung it is leaving, and the
        // rung it climbs onto starts again at zero.
        //
        // Nested rather than a `let`-chain: `if cond && let Some(x) = ..` is
        // unstable on **1.85**, which this crate's MSRV pins, and stable rustc
        // accepts it — so the local gates pass and only the MSRV leg refuses.
        self.charge_allowance(finished.key, &finished.record);

        match &finished.settlement {
            AttemptSettlement::Retained {
                retained_session,
                retained_incarnation,
            } => {
                if let Some(generation) = self.open_generation_mut(finished.key) {
                    generation.class = GenerationClass::RetainedIdle {
                        session: retained_session.clone(),
                        incarnation: *retained_incarnation,
                    };
                }
            }
            AttemptSettlement::Closed { transition, .. } => match transition {
                // Unreachable: `check_attempt_finished` refuses this
                // transition before `apply` is called, because
                // `candidate_prepared` is the sole successful settlement. The
                // arm stays so the match is total over the wire vocabulary —
                // the variant is still a legal *shape*, it is simply not a
                // settlement this fold accepts — and it does nothing, so a
                // check that stopped refusing would produce a generation stuck
                // in flight rather than a silently-promoted one.
                SettlementTransition::Succeeded => {}
                SettlementTransition::Retry => {
                    self.close_generation(finished.key);
                }
                SettlementTransition::Escalated { rung } => {
                    self.close_generation(finished.key);
                    // The settlement's own number: the packet defines it as the
                    // rung the escalation climbs *onto*. The allowance is per
                    // rung, so it starts again here.
                    if let Some(task) = self.tasks.get_mut(finished.key.index()) {
                        task.rung = *rung;
                        task.attempts_on_rung = 0;
                    }
                }
                SettlementTransition::Deferred { defers, .. } => {
                    self.close_generation(finished.key);
                    self.set_state(finished.key, TaskState::Deferred);
                    // The settlement's own number, not this fold's plus one.
                    // `settle_failed` computed it as `defers.saturating_add(1)`
                    // and appended it; recomputing here would be a second
                    // derivation of a value the log already holds, and a replay
                    // of the same log would then disagree with the process that
                    // wrote it.
                    self.set_defers(finished.key, *defers);
                }
                SettlementTransition::Parked { question } => {
                    self.close_generation(finished.key);
                    self.open_question(question, QuestionOrigin::Admission, None);
                    self.set_state(finished.key, TaskState::AwaitingInput);
                }
                SettlementTransition::Failed { halts_run, .. } => {
                    self.close_generation(finished.key);
                    self.set_state(finished.key, TaskState::Failed);
                    if *halts_run {
                        self.record_halt(finished.key);
                    }
                }
            },
        }
    }

    /// `halted_at` is first in wins, and is never cleared.
    fn record_halt(&mut self, key: TaskKey) {
        if self.halted_at.is_none() {
            self.halted_at = Some(key);
            self.halted_epoch = Some(self.epoch);
        }
    }

    /// One settled attempt against its rung's allowance.
    ///
    /// **The single write, and both settlements reach it through here.** The
    /// increment used to live inline in [`Self::apply_settlement`], which was
    /// fine while `attempt_finished` was the only settlement — and stopped being
    /// fine on 2026-08-27, when `candidate_prepared` became the sole successful
    /// one. The settlement moved and the counting did not, so **a successful
    /// attempt stopped spending anything**: a first-attempt success left
    /// `attempts_on_rung` at zero, replay reproduced the undercount, and a later
    /// allowance reader could grant an extra attempt on a rung already paid for.
    /// The round-4 review of `09f9a99` found it, and the Class B approval this
    /// change was made under says the thing that did not happen — *"settlement
    /// counting moves to the sole event"*.
    ///
    /// A shared core rather than a second increment, because two increments are
    /// two rules: `the_rungs_allowance_is_counted_in_one_production_place` exists
    /// to forbid exactly that, and it counts **calls to this** so a settlement
    /// that stops charging is a failing census rather than a silent undercount.
    ///
    /// It consults `spends_allowance` and answers nothing itself. A successful
    /// record carries no failure, and `spends_allowance(None)` is `true`: the
    /// worker ran and produced work that was judged and accepted.
    fn charge_allowance(&mut self, key: TaskKey, record: &crate::events::AttemptRecord) {
        if crate::ladder::spends_allowance(
            record
                .failure
                .as_ref()
                .map(crate::events::FailureRecord::shape),
        ) {
            if let Some(task) = self.tasks.get_mut(key.index()) {
                task.attempts_on_rung = task.attempts_on_rung.saturating_add(1);
            }
        }
    }

    fn apply_candidate_prepared(&mut self, prepared: &CandidatePrepared) {
        let record = PreparedCandidate {
            candidate: prepared.candidate(),
            base_sha: prepared.base_sha.clone(),
            tree_sha: prepared.tree_sha.clone(),
            paths: prepared.actual_paths.clone(),
        };
        if let Some(generation) = self.open_generation_mut(prepared.key) {
            generation.candidate = Some(record);
            // **The settlement, which used to arrive on its own event.** A
            // candidate-producing attempt has exactly one successful
            // settlement and this is it, so the class transition belongs here
            // rather than to an `attempt_finished` the 2026-08-12 record says
            // is not emitted.
            generation.class = GenerationClass::Promoting;
        }
        // **The settlement's accounting, which moved with the settlement.**
        // Same core as the failure path, so there is one increment in this
        // build and both settlements reach it.
        self.charge_allowance(prepared.key, &prepared.attempt);
        match &prepared.lease_effect {
            CandidateLeaseEffect::ReplacesPredicted { paths } => {
                self.leases.release(LeaseOwner::Generation {
                    key: prepared.key,
                    generation: prepared.generation,
                });
                self.leases.grant(
                    LeaseOwner::Candidate {
                        key: prepared.key,
                        generation: prepared.generation,
                    },
                    paths.clone(),
                );
            }
            CandidateLeaseEffect::WidensLineage { root, paths } => {
                self.leases.widen_lineage(*root, paths);
            }
        }
        self.set_state(prepared.key, TaskState::AwaitingMerge);
    }

    fn apply_candidate_created(&mut self, candidate: &CandidateRef) {
        let paths = self
            .tasks
            .get(candidate.key.index())
            .and_then(TaskFold::open)
            .and_then(|generation| generation.candidate.as_ref())
            .map_or(PathSet::RepoWide, |prepared| prepared.paths.clone());
        let lineage_root = self
            .registry
            .get(candidate.key)
            .and_then(|entry| entry.lineage)
            .map(|lineage| lineage.root);
        self.close_generation(candidate.key);
        self.queue.push(QueueEntry {
            candidate: candidate.clone(),
            paths,
            lineage_root,
            verification_deferred: false,
            defers: 0,
            sequence: None,
        });
    }

    fn apply_verification_started(&mut self, started: &MergeVerificationStarted) {
        self.next_sequence = self.next_sequence.saturating_add(1);
        if let Some(entry) = self
            .queue
            .get_mut(started.candidate.key, started.candidate.generation)
        {
            entry.sequence = Some(started.sequence);
        }
        self.transaction = Some(Transaction {
            sequence: started.sequence,
            candidate: started.candidate.clone(),
            class: TransactionClass::VerificationStarted {
                basis: started.basis.clone(),
                expected_head: started.expected_head.clone(),
                proposed_sha: started.proposed_sha.clone(),
            },
        });
    }

    fn apply_verification_unavailable(&mut self, unavailable: &MergeVerificationUnavailable) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        let candidate = transaction.candidate;
        if let Some(entry) = self.queue.get_mut(candidate.key, candidate.generation) {
            entry.sequence = None;
            if let UnavailableOutcome::Deferred { defers } = &unavailable.outcome {
                entry.verification_deferred = true;
                entry.defers = *defers;
            }
        }
        if let UnavailableOutcome::Parked { question } = &unavailable.outcome {
            self.open_question(question, QuestionOrigin::VerificationPark, None);
            self.set_state(candidate.key, TaskState::AwaitingInput);
        }
    }

    fn release_transaction(&mut self) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        if let Some(entry) = self
            .queue
            .get_mut(transaction.candidate.key, transaction.candidate.generation)
        {
            entry.sequence = None;
        }
    }

    fn apply_merge_prepared(&mut self, prepared: &MergePrepared) {
        if prepared.disposition == PreparedDisposition::Fast {
            self.next_sequence = self.next_sequence.saturating_add(1);
        }
        self.transaction = Some(Transaction {
            sequence: prepared.sequence,
            candidate: prepared.candidate(),
            class: TransactionClass::Prepared {
                proposed_sha: prepared.proposed_sha.clone(),
                satisfies: prepared.satisfies.clone(),
            },
        });
    }

    fn apply_merge_rejected(&mut self, rejected: &MergeRejected) {
        if matches!(rejected.disposition, RejectionDisposition::Conflict { .. }) {
            self.next_sequence = self.next_sequence.saturating_add(1);
        }
        self.transaction = None;
        let candidate = &rejected.candidate;
        self.queue.remove(candidate.key, candidate.generation);
        match &rejected.lease_effect {
            RejectionLeaseEffect::CreatesLineage { root, paths } => {
                // The rejected candidate's own holding becomes the lineage's,
                // widened by the region the conflict named.
                let held = self
                    .tasks
                    .get(candidate.key.index())
                    .and_then(|task| {
                        task.generations
                            .iter()
                            .find(|generation| generation.id == candidate.generation)
                    })
                    .and_then(|generation| generation.candidate.as_ref())
                    .map(|prepared| prepared.paths.clone());
                if let Some(held) = held {
                    self.leases.widen_lineage(*root, &held);
                }
                self.leases.widen_lineage(*root, paths);
                self.leases.release(LeaseOwner::Candidate {
                    key: candidate.key,
                    generation: candidate.generation,
                });
            }
            RejectionLeaseEffect::WidensLineage { root, paths } => {
                self.leases.widen_lineage(*root, paths);
            }
        }
        self.set_state(candidate.key, TaskState::AwaitingRepair);
        self.register(&rejected.repair);
    }

    fn apply_task_merged(&mut self, merged: &TaskMerged) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        let candidate = transaction.candidate;
        self.queue.remove(candidate.key, candidate.generation);
        for key in &merged.satisfies {
            self.set_state(*key, TaskState::Merged);
        }
        match &merged.lease_release {
            MergeLeaseRelease::Candidate { key, generation } => {
                self.leases.release(LeaseOwner::Candidate {
                    key: *key,
                    generation: *generation,
                });
            }
            MergeLeaseRelease::Lineage { root } => {
                self.leases.release(LeaseOwner::Lineage { root: *root });
            }
        }
    }

    fn apply_answer(&mut self, answered: &QuestionAnswered4, derived: &Derived) {
        self.questions.remove(&answered.question);
        match &answered.answer {
            Answer4::Answered {
                binding_override, ..
            } => {
                if let Some(binding) = binding_override {
                    self.overrides.insert(answered.key, binding.clone());
                }
                let state = match derived {
                    Derived::Answer(QuestionOrigin::VerificationPark) => TaskState::AwaitingMerge,
                    _ => TaskState::Pending,
                };
                self.set_state(answered.key, state);
            }
            Answer4::Declined { decline_halts_run } => {
                self.set_state(answered.key, TaskState::Failed);
                self.release_holdings_of(answered.key);
                if *decline_halts_run {
                    self.record_halt(answered.key);
                }
            }
        }
    }

    /// A declined question consumes the task's queue position and releases what
    /// it held: its candidate lease, or the lineage lease when the task belongs
    /// to a lineage — a declined lineage fails as a whole.
    fn release_holdings_of(&mut self, key: TaskKey) {
        let generations: Vec<GenerationId> = self
            .tasks
            .get(key.index())
            .map(|task| {
                task.generations
                    .iter()
                    .map(|generation| generation.id)
                    .collect()
            })
            .unwrap_or_default();
        for generation in generations {
            self.queue.remove(key, generation);
            self.leases
                .release(LeaseOwner::Candidate { key, generation });
        }
        let root = self
            .registry
            .get(key)
            .and_then(|entry| entry.lineage)
            .map(|lineage| lineage.root);
        if let Some(root) = root {
            self.leases.release(LeaseOwner::Lineage { root });
        } else if self.leases.holds(LeaseOwner::Lineage { root: key }) {
            self.leases.release(LeaseOwner::Lineage { root: key });
        }
    }

    /// Open a question, carrying the binding authority it was asked under.
    ///
    /// `binding` is `Some` for a `HumanBinding` admission and `None` for every
    /// other question this run can ask — a `HumanRequired` admission, a parked
    /// settlement, a verification park, a bare `question_raised`. That is the
    /// whole of what an override may be validated against.
    fn open_question(
        &mut self,
        question: &FrozenQuestion,
        origin: QuestionOrigin,
        binding: Option<Vec<String>>,
    ) {
        self.seen_questions.insert(question.id.clone());
        self.questions.insert(
            question.id.clone(),
            OpenQuestion {
                question: question.clone(),
                origin,
                binding,
            },
        );
    }

    fn set_state(&mut self, key: TaskKey, state: TaskState) {
        if let Some(task) = self.tasks.get_mut(key.index()) {
            task.state = state;
        }
    }

    /// Record the deferral count a `Deferred` settlement carried.
    ///
    /// Assignment rather than increment: the number is the settlement's, which
    /// is what makes a replay of the same log reach the same count as the
    /// process that wrote it.
    fn set_defers(&mut self, key: TaskKey, defers: u32) {
        if let Some(task) = self.tasks.get_mut(key.index()) {
            task.defers = defers;
        }
    }

    fn open_generation_mut(&mut self, key: TaskKey) -> Option<&mut GenerationFold> {
        self.tasks.get_mut(key.index())?.open_mut()
    }

    /// Close the open generation, releasing the region it held on its own.
    fn close_generation(&mut self, key: TaskKey) {
        let Some(generation) = self.open_generation_mut(key) else {
            return;
        };
        let id = generation.id;
        let own = generation.lease == GenerationLease::Own;
        generation.class = GenerationClass::Closed;
        if own {
            self.leases.release(LeaseOwner::Generation {
                key,
                generation: id,
            });
        }
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

/// A region as a refusal names it.
///
/// The empty prefix list is spelled out rather than printed as `[]`: an empty
/// region and an unread one are different answers — [`PathSet::prefixes`] says
/// so — and a refusal that rendered the first as an empty pair of brackets
/// would read like a formatting accident next to `the whole repository`.
fn describe_region(paths: &PathSet) -> String {
    match paths.prefixes() {
        None => "the whole repository".to_owned(),
        Some([]) => "no path at all".to_owned(),
        Some(prefixes) => prefixes
            .iter()
            .map(|path| format!("`{}`", path.as_str()))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// A ref name, for a diagnostic that has to print an `Option<GitRef>`.
trait GitRefName {
    fn name(&self) -> &str;
}

impl GitRefName for GitRef {
    fn name(&self) -> &str {
        self.as_str()
    }
}

fn ineligible_detail(why: Ineligible) -> String {
    match why {
        Ineligible::AwaitingInput => "its task is parked on a question".to_owned(),
        Ineligible::VerificationDeferred => {
            "its verification is deferred until the backoff elapses".to_owned()
        }
        Ineligible::InsideLineage { root } => {
            format!("it overlaps the region lineage {root} holds")
        }
        Ineligible::BehindOlderLineage { root } => {
            format!("it overlaps the region the older lineage {root} holds")
        }
    }
}

fn spawn_admission_name(admission: &SpawnAdmission) -> &'static str {
    match admission {
        SpawnAdmission::Runnable => "runnable",
        SpawnAdmission::HumanRequired { .. } => "human-required",
        SpawnAdmission::HumanBinding { .. } => "human-binding",
    }
}

fn admission_name(admission: &Admission) -> &'static str {
    match admission {
        Admission::Runnable => "runnable",
        Admission::HumanBinding { .. } => "human-binding",
    }
}

fn ordinal(index: u32) -> String {
    format!("#{index}")
}

/// refusals[14]: the disposition an event records must be the one this
/// generation's holding admits.
/// The recorded disposition against the one the holding implies.
///
/// **Every caller passes a closing generation, and since 2026-08-27 there is no
/// other kind.** This took a `survives: bool`, and exactly one caller ever
/// passed `true`: `attempt_finished{Succeeded}`, the settlement that left a
/// generation open to hand its region to a candidate. That event is no longer a
/// settlement this fold accepts — `candidate_prepared` is the sole successful
/// one — so the parameter had a single reachable value and a second value that
/// documented a rule nothing could exercise.
///
/// **The surviving case did not disappear, it moved.** A generation that keeps
/// its region hands it over through `CandidatePrepared::lease_effect`, which
/// [`TopologyFold::check_candidate_prepared`] matches against the entry's
/// lineage — the same decision, on the event that now makes it.
/// [`GenerationLease::expected`] keeps both arms and its own table test,
/// because it is the statement of the rule rather than a caller of it.
fn check_lease_disposition(
    kind: &'static str,
    key: TaskKey,
    lease: GenerationLease,
    recorded: LeaseDisposition,
) -> Result<(), FoldError> {
    let expected = lease.expected(false);
    if recorded == expected {
        return Ok(());
    }
    Err(FoldError::InvalidLeaseDisposition {
        kind,
        key: key.0,
        recorded: format!("{recorded:?}"),
        owner: match lease {
            GenerationLease::Own => "leaseholding",
            GenerationLease::InheritedLineage { .. } => "lineage",
        },
        fate: "closes",
        expected: format!("{expected:?}"),
    })
}

#[cfg(test)]
mod tests;
