//! Extended notes: `docs/internals/topology/fold.md`

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    AwaitingMerge,
    AwaitingRepair,
    AwaitingInput,
    Deferred,
    Merged,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationClass {
    OpenNoAttempt,
    InFlight {
        attempt: AttemptNumber,
    },
    RetainedIdle {
        session: SessionId,
        incarnation: Epoch,
    },
    Promoting,
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

    fn holds_pipeline(&self) -> bool {
        matches!(
            self,
            Self::OpenNoAttempt | Self::InFlight { .. } | Self::Promoting
        )
    }

    fn blocks_run_end(&self) -> bool {
        !matches!(self, Self::Closed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationFold {
    pub id: GenerationId,
    pub class: GenerationClass,
    pub base_sha: CommitSha,
    pub lease: GenerationLease,
    pub attempts: u32,
    pub candidate: Option<PreparedCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCandidate {
    pub candidate: CandidateRef,
    pub base_sha: CommitSha,
    pub tree_sha: CommitSha,
    pub paths: PathSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFold {
    pub state: TaskState,
    pub defers: u32,
    pub rung: u32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionOrigin {
    VerificationPark,
    Admission,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenQuestion {
    pub question: FrozenQuestion,
    pub origin: QuestionOrigin,
    pub binding: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionClass {
    VerificationStarted {
        basis: VerificationBasis,
        expected_head: CommitSha,
        proposed_sha: CommitSha,
    },
    Prepared {
        proposed_sha: CommitSha,
        satisfies: Vec<TaskKey>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub sequence: SequenceId,
    pub candidate: CandidateRef,
    pub class: TransactionClass,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunState {
    started: Box<RunStarted4>,
    registry: TaskRegistry,
    tasks: Vec<TaskFold>,
    epoch: Epoch,
    incarnation: IncarnationId,
    questions: BTreeMap<QuestionId, OpenQuestion>,
    seen_questions: BTreeSet<QuestionId>,
    deferred_tasks: BTreeSet<TaskKey>,
    overrides: BTreeMap<TaskKey, BindingOverride>,
    queue: CandidateQueue,
    leases: LeaseTable,
    transaction: Option<Transaction>,
    next_sequence: u32,
    halted_at: Option<TaskKey>,
    halted_epoch: Option<Epoch>,
    budget_stop: Option<BudgetStop>,
    finished: Option<RunOutcome>,
}

#[derive(Debug, Clone)]
pub struct FrozenInputs {
    pub plan: Plan,
    pub normalized_plan_digest: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopologyDelta {
    event: TopologyEvent,
    derived: Derived,
}

impl TopologyDelta {
    pub fn event(&self) -> &TopologyEvent {
        &self.event
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Derived {
    None,
    Registry(Box<TaskRegistry>),
    Answer(QuestionOrigin),
}

#[derive(Debug, Clone)]
pub struct TopologyFold {
    inputs: FrozenInputs,
    run: Option<RunState>,
    poisoned: bool,
}

impl TopologyFold {
    pub fn new(inputs: FrozenInputs) -> Self {
        Self {
            inputs,
            run: None,
            poisoned: false,
        }
    }

    pub fn replay(inputs: FrozenInputs, events: &[TopologyEvent]) -> Result<Self, FoldError> {
        let mut fold = Self::new(inputs);
        for event in events {
            let delta = fold.plan_transition(event)?;
            fold.apply_delta(delta);
        }
        Ok(fold)
    }

    pub fn plan_transition(&self, event: &TopologyEvent) -> Result<TopologyDelta, FoldError> {
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
            deferred_tasks: BTreeSet::new(),
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

    fn lineage_root(&self, key: TaskKey) -> TaskKey {
        self.registry
            .get(key)
            .and_then(|entry| entry.lineage)
            .map_or(key, |lineage| lineage.root)
    }

    fn lineage_has_question(&self, key: TaskKey) -> bool {
        let root = self.lineage_root(key);
        self.questions
            .values()
            .any(|open| self.lineage_root(open.question.key) == root)
    }
}

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
