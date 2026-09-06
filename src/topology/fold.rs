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
        "`{kind}` puts attempt {attempt} of task {key} on rung {rung}, and {detail}; a task's \
         ladder position is derived by replay, an attempt runs at that position, and only an \
         escalation moves it, one rung up the frozen ladder"
    )]
    WrongRung {
        kind: &'static str,
        key: u32,
        attempt: u32,
        rung: u32,
        detail: String,
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
        match (event.body, derived) {
            (TopologyEventBody::RunStarted { data }, Derived::Registry(registry)) => {
                self.run = Some(RunState::start(data, *registry));
            }
            (body, derived) => {
                if let Some(run) = self.run.as_mut() {
                    run.apply(&body, &derived);
                }
            }
        }
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
        let Some(prefix) = hint_prefix(hint) else {
            return PathSet::RepoWide;
        };
        paths.push(prefix);
    }
    PathSet::Prefixes { paths }
}

fn hint_prefix(hint: &str) -> Option<GitPath> {
    const METACHARACTERS: [char; 4] = ['*', '?', '[', '{'];
    let normalized = hint.replace('\\', "/");
    let mut prefix = String::with_capacity(normalized.len());
    for (position, component) in normalized
        .split('/')
        .take_while(|component| !component.contains(METACHARACTERS))
        .enumerate()
    {
        if matches!(component, "." | "..") {
            return None;
        }
        if position > 0 {
            prefix.push('/');
        }
        prefix.push_str(component);
    }
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    Some(GitPath(trimmed.to_owned()))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod parent_tests {
    use super::*;
    use crate::topology::leases::paths_overlap;
    use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion};

    fn policy() -> PathPolicy {
        PathPolicy {
            version: PathPolicyVersion::V1,
            case_fold: false,
            grammar: PathGrammar::Globset,
        }
    }

    fn generation(id: u32, class: GenerationClass) -> GenerationFold {
        GenerationFold {
            id: GenerationId(id),
            class,
            base_sha: CommitSha(format!("base-{id}")),
            lease: GenerationLease::Own,
            attempts: 0,
            candidate: None,
        }
    }

    fn variant_name(error: &FoldError) -> &'static str {
        match error {
            FoldError::NotStarted { .. } => "NotStarted",
            FoldError::AlreadyStarted => "AlreadyStarted",
            FoldError::NotTopologySchema { .. } => "NotTopologySchema",
            FoldError::IncompleteRunner { .. } => "IncompleteRunner",
            FoldError::RunnerMoved { .. } => "RunnerMoved",
            FoldError::DigestMismatch { .. } => "DigestMismatch",
            FoldError::RegistryUnbuildable { .. } => "RegistryUnbuildable",
            FoldError::MalformedLadder { .. } => "MalformedLadder",
            FoldError::UnknownKey { .. } => "UnknownKey",
            FoldError::NonDenseKey { .. } => "NonDenseKey",
            FoldError::MalformedEntry { .. } => "MalformedEntry",
            FoldError::WrongTaskState { .. } => "WrongTaskState",
            FoldError::NotTheOpenGeneration { .. } => "NotTheOpenGeneration",
            FoldError::WrongAttempt { .. } => "WrongAttempt",
            FoldError::WrongRung { .. } => "WrongRung",
            FoldError::StaleIncarnation { .. } => "StaleIncarnation",
            FoldError::BindingMismatch { .. } => "BindingMismatch",
            FoldError::InvalidLeaseDisposition { .. } => "InvalidLeaseDisposition",
            FoldError::NonDenseSequence { .. } => "NonDenseSequence",
            FoldError::WrongSequence { .. } => "WrongSequence",
            FoldError::TransactionAlreadyOpen { .. } => "TransactionAlreadyOpen",
            FoldError::NotFirstEligible { .. } => "NotFirstEligible",
            FoldError::InconsistentRecord { .. } => "InconsistentRecord",
            FoldError::InvalidSatisfies { .. } => "InvalidSatisfies",
            FoldError::InvalidDefers { .. } => "InvalidDefers",
            FoldError::UnanswerableQuestion { .. } => "UnanswerableQuestion",
            FoldError::WrongQuestion { .. } => "WrongQuestion",
            FoldError::RunEnding { .. } => "RunEnding",
            FoldError::RunIsOver { .. } => "RunIsOver",
            FoldError::OutcomeMismatch { .. } => "OutcomeMismatch",
            FoldError::Poisoned => "Poisoned",
            FoldError::RewrittenLog { .. } => "RewrittenLog",
        }
    }

    fn refusals() -> Vec<(FoldError, Vec<String>)> {
        vec![
            (
                FoldError::NotStarted { kind: "kind-01" },
                vec!["kind-01".to_owned()],
            ),
            (FoldError::AlreadyStarted, Vec::new()),
            (
                FoldError::NotTopologySchema { schema: 902 },
                vec!["902".to_owned()],
            ),
            (
                FoldError::IncompleteRunner {
                    defect: "defect-03".to_owned(),
                },
                vec!["defect-03".to_owned()],
            ),
            (
                FoldError::RunnerMoved {
                    field: "field-04".to_owned(),
                },
                vec!["field-04".to_owned()],
            ),
            (
                FoldError::DigestMismatch {
                    what: "what-05",
                    recorded: "recorded-05".to_owned(),
                    actual: "actual-05".to_owned(),
                },
                vec![
                    "what-05".to_owned(),
                    "recorded-05".to_owned(),
                    "actual-05".to_owned(),
                ],
            ),
            (
                FoldError::RegistryUnbuildable {
                    detail: "detail-06".to_owned(),
                },
                vec!["detail-06".to_owned()],
            ),
            (
                FoldError::MalformedLadder {
                    key: 907,
                    defect: "defect-07".to_owned(),
                },
                vec!["907".to_owned(), "defect-07".to_owned()],
            ),
            (
                FoldError::UnknownKey {
                    kind: "kind-08",
                    key: 908,
                },
                vec!["kind-08".to_owned(), "908".to_owned()],
            ),
            (
                FoldError::NonDenseKey {
                    kind: "kind-09",
                    key: 909,
                    len: 709,
                },
                vec!["kind-09".to_owned(), "909".to_owned(), "709".to_owned()],
            ),
            (
                FoldError::MalformedEntry {
                    kind: "kind-10",
                    key: 910,
                    detail: "detail-10".to_owned(),
                },
                vec![
                    "kind-10".to_owned(),
                    "910".to_owned(),
                    "detail-10".to_owned(),
                ],
            ),
            (
                FoldError::WrongTaskState {
                    kind: "kind-11",
                    key: 911,
                    state: "state-11",
                    expected: "expected-11",
                },
                vec![
                    "kind-11".to_owned(),
                    "911".to_owned(),
                    "state-11".to_owned(),
                    "expected-11".to_owned(),
                ],
            ),
            (
                FoldError::NotTheOpenGeneration {
                    kind: "kind-12",
                    key: 912,
                    generation: 712,
                    detail: "detail-12".to_owned(),
                },
                vec![
                    "kind-12".to_owned(),
                    "912".to_owned(),
                    "712".to_owned(),
                    "detail-12".to_owned(),
                ],
            ),
            (
                FoldError::WrongAttempt {
                    kind: "kind-13",
                    key: 913,
                    generation: 713,
                    attempt: 513,
                    expected: "expected-13".to_owned(),
                },
                vec![
                    "kind-13".to_owned(),
                    "913".to_owned(),
                    "713".to_owned(),
                    "513".to_owned(),
                    "expected-13".to_owned(),
                ],
            ),
            (
                FoldError::WrongRung {
                    kind: "kind-14",
                    key: 914,
                    attempt: 714,
                    rung: 514,
                    detail: "detail-14".to_owned(),
                },
                vec![
                    "kind-14".to_owned(),
                    "914".to_owned(),
                    "714".to_owned(),
                    "514".to_owned(),
                    "detail-14".to_owned(),
                ],
            ),
            (
                FoldError::StaleIncarnation {
                    key: 915,
                    attempt: 715,
                    detail: "detail-15".to_owned(),
                },
                vec!["915".to_owned(), "715".to_owned(), "detail-15".to_owned()],
            ),
            (
                FoldError::BindingMismatch {
                    key: 916,
                    attempt: 716,
                    detail: "detail-16".to_owned(),
                },
                vec!["916".to_owned(), "716".to_owned(), "detail-16".to_owned()],
            ),
            (
                FoldError::InvalidLeaseDisposition {
                    kind: "kind-17",
                    key: 917,
                    recorded: "recorded-17".to_owned(),
                    owner: "owner-17",
                    fate: "fate-17",
                    expected: "expected-17".to_owned(),
                },
                vec![
                    "kind-17".to_owned(),
                    "917".to_owned(),
                    "recorded-17".to_owned(),
                    "owner-17".to_owned(),
                    "fate-17".to_owned(),
                    "expected-17".to_owned(),
                ],
            ),
            (
                FoldError::NonDenseSequence {
                    kind: "kind-18",
                    sequence: 918,
                    next: 718,
                },
                vec!["kind-18".to_owned(), "918".to_owned(), "718".to_owned()],
            ),
            (
                FoldError::WrongSequence {
                    kind: "kind-19",
                    sequence: 919,
                    open: "open-19".to_owned(),
                },
                vec!["kind-19".to_owned(), "919".to_owned(), "open-19".to_owned()],
            ),
            (
                FoldError::TransactionAlreadyOpen {
                    kind: "kind-20",
                    sequence: 920,
                    open: 720,
                },
                vec!["kind-20".to_owned(), "920".to_owned(), "720".to_owned()],
            ),
            (
                FoldError::NotFirstEligible {
                    kind: "kind-21",
                    key: 921,
                    generation: 721,
                    detail: "detail-21".to_owned(),
                },
                vec![
                    "kind-21".to_owned(),
                    "921".to_owned(),
                    "721".to_owned(),
                    "detail-21".to_owned(),
                ],
            ),
            (
                FoldError::InconsistentRecord {
                    kind: "kind-22",
                    detail: "detail-22".to_owned(),
                },
                vec!["kind-22".to_owned(), "detail-22".to_owned()],
            ),
            (
                FoldError::InvalidSatisfies {
                    kind: "kind-23",
                    recorded: vec![923],
                    derived: vec![723],
                },
                vec!["kind-23".to_owned(), "923".to_owned(), "723".to_owned()],
            ),
            (
                FoldError::InvalidDefers {
                    defers: 924,
                    detail: "detail-24".to_owned(),
                },
                vec!["924".to_owned(), "detail-24".to_owned()],
            ),
            (
                FoldError::UnanswerableQuestion {
                    kind: "kind-25",
                    detail: "detail-25".to_owned(),
                },
                vec!["kind-25".to_owned(), "detail-25".to_owned()],
            ),
            (
                FoldError::WrongQuestion {
                    kind: "kind-26",
                    question: "question-26".to_owned(),
                    detail: "detail-26".to_owned(),
                },
                vec![
                    "kind-26".to_owned(),
                    "question-26".to_owned(),
                    "detail-26".to_owned(),
                ],
            ),
            (
                FoldError::RunEnding {
                    kind: "kind-27",
                    what: "what-27",
                },
                vec!["kind-27".to_owned(), "what-27".to_owned()],
            ),
            (
                FoldError::RunIsOver {
                    kind: "kind-28",
                    outcome: "outcome-28",
                },
                vec!["kind-28".to_owned(), "outcome-28".to_owned()],
            ),
            (
                FoldError::OutcomeMismatch {
                    recorded: "recorded-29",
                    derived: "derived-29".to_owned(),
                },
                vec!["recorded-29".to_owned(), "derived-29".to_owned()],
            ),
            (FoldError::Poisoned, Vec::new()),
            (
                FoldError::RewrittenLog {
                    line: 931,
                    detail: "detail-31".to_owned(),
                },
                vec!["931".to_owned(), "detail-31".to_owned()],
            ),
        ]
    }

    #[test]
    fn every_refusal_names_the_record_it_refused_and_the_value_it_disagreed_with() {
        let refusals = refusals();
        assert_eq!(
            refusals.len(),
            32,
            "the sample list is one refusal per `FoldError` variant; `variant_name` is exhaustive, \
             so a new variant cannot compile without an arm there and a sample here"
        );
        let named: BTreeSet<&'static str> = refusals
            .iter()
            .map(|(error, _)| variant_name(error))
            .collect();
        assert_eq!(
            named.len(),
            refusals.len(),
            "two samples name one variant, so some variant is unmeasured"
        );

        let mut rendered: BTreeSet<String> = BTreeSet::new();
        for (error, fields) in &refusals {
            let message = error.to_string();
            assert!(
                !message.is_empty(),
                "{} renders nothing",
                variant_name(error)
            );
            for field in fields {
                assert!(
                    message.contains(field.as_str()),
                    "{} drops `{field}` from its message: {message}",
                    variant_name(error)
                );
            }
            assert!(
                rendered.insert(message.clone()),
                "{} reports what another refusal reports: {message}",
                variant_name(error)
            );
        }
    }

    #[test]
    fn a_hint_with_no_metacharacter_is_its_own_prefix_and_a_glob_cuts_whole_components() {
        let cases: [(&str, Option<&str>); 24] = [
            ("src/literal", Some("src/literal")),
            ("build.rs", Some("build.rs")),
            ("src/trailing/", Some("src/trailing")),
            ("src/star/*.rs", Some("src/star")),
            ("src/question/?.rs", Some("src/question")),
            ("src/bracket/[ab].rs", Some("src/bracket")),
            ("src/brace/{a,b}.rs", Some("src/brace")),
            (r"src\backslash\deep", Some("src/backslash/deep")),
            ("src/doubled//inner/", Some("src/doubled//inner")),
            ("src/\u{dc}ber/", Some("src/\u{dc}ber")),
            ("src/star*.rs", Some("src")),
            ("src/eng*", Some("src")),
            ("src/a*/b", Some("src")),
            ("src/deep/mod.rs*", Some("src/deep")),
            (r"src\deep\mod.rs*", Some("src/deep")),
            ("**/anywhere.rs", None),
            ("*.rs", None),
            ("star*.rs", None),
            ("{a,b}/c", None),
            ("", None),
            ("/", None),
            ("src/./alpha", None),
            ("src/../src/alpha", None),
            ("./src/alpha", None),
        ];
        for (hint, expected) in cases {
            assert_eq!(
                hint_prefix(hint).as_ref().map(GitPath::as_str),
                expected,
                "`{hint}`"
            );
        }
    }

    #[test]
    fn a_derived_prefix_overlaps_every_path_its_hint_can_match() {
        let policy = policy();
        let covered: [(&str, &str); 5] = [
            ("src/eng*", "src/engine/mod.rs"),
            ("src/star*.rs", "src/starship.rs"),
            ("src/a*/b", "src/alpha/b"),
            ("src/deep/mod.rs*", "src/deep/mod.rs.bak"),
            ("src/star/*.rs", "src/star/one.rs"),
        ];
        for (hint, matched) in covered {
            let prefix = hint_prefix(hint).expect("the hint bounds a region");
            assert!(
                paths_overlap(&prefix, &GitPath::from(matched), &policy),
                "`{hint}` derives `{prefix}`, which does not overlap `{matched}` it matches"
            );
        }
        assert!(
            !paths_overlap(
                &GitPath::from("src/eng"),
                &GitPath::from("src/engine/mod.rs"),
                &policy
            ),
            "the comparator is component-wise; a prefix cut inside a component is not an ancestor"
        );
    }

    #[test]
    fn a_hint_whose_prefix_has_a_dot_component_bounds_nothing_it_can_be_compared_against() {
        let policy = policy();
        for spelling in ["src/./alpha", "src/../src/alpha", "./src/alpha", "../alpha"] {
            assert!(
                hint_prefix(spelling).is_none(),
                "`{spelling}` was kept as a prefix"
            );
            assert!(
                !paths_overlap(
                    &GitPath::from(spelling),
                    &GitPath::from("src/alpha"),
                    &policy
                ),
                "`{spelling}` would have been a second spelling the comparator does not match"
            );
        }
    }

    #[test]
    fn a_task_state_and_a_generation_class_each_name_themselves_distinctly() {
        let states = [
            (TaskState::Pending, "pending", false),
            (TaskState::AwaitingMerge, "awaiting merge", false),
            (TaskState::AwaitingRepair, "awaiting repair", false),
            (TaskState::AwaitingInput, "awaiting input", false),
            (TaskState::Deferred, "deferred", false),
            (TaskState::Merged, "merged", true),
            (TaskState::Failed, "failed", true),
        ];
        let mut names: BTreeSet<&'static str> = BTreeSet::new();
        for (state, name, terminal) in states {
            assert_eq!(state.name(), name, "{state:?}");
            assert_eq!(state.is_terminal(), terminal, "{state:?}");
            assert!(names.insert(name), "`{name}` names two states");
        }
        assert_eq!(names.len(), 7);

        let classes = [
            (GenerationClass::OpenNoAttempt, "open with no attempt", true),
            (
                GenerationClass::InFlight {
                    attempt: AttemptNumber(1),
                },
                "in flight",
                true,
            ),
            (
                GenerationClass::RetainedIdle {
                    session: SessionId("s".to_owned()),
                    incarnation: Epoch(0),
                },
                "retained idle",
                false,
            ),
            (GenerationClass::Promoting, "promoting", true),
            (GenerationClass::Closed, "closed", false),
        ];
        let mut class_names: BTreeSet<&'static str> = BTreeSet::new();
        for (class, name, holds) in classes {
            assert_eq!(class.name(), name, "{class:?}");
            assert_eq!(class.holds_pipeline(), holds, "{class:?}");
            assert_eq!(
                class.blocks_run_end(),
                class != GenerationClass::Closed,
                "{class:?}"
            );
            assert!(class_names.insert(name), "`{name}` names two classes");
        }
        assert_eq!(class_names.len(), 5);
    }

    #[test]
    fn a_tasks_open_generation_is_the_one_that_is_not_closed() {
        let mut task = TaskFold::new();
        assert_eq!(task.state, TaskState::Pending);
        assert_eq!(task.defers, 0);
        assert_eq!(task.rung, 0);
        assert_eq!(task.attempts_on_rung, 0);
        assert!(task.open().is_none(), "a task with no generation has none");

        task.generations
            .push(generation(0, GenerationClass::Closed));
        assert!(
            task.open().is_none(),
            "a closed generation is not the open one"
        );

        task.generations
            .push(generation(1, GenerationClass::Promoting));
        assert_eq!(
            task.open().map(|generation| generation.id),
            Some(GenerationId(1))
        );
        task.generations
            .push(generation(2, GenerationClass::Closed));
        assert_eq!(
            task.open().map(|generation| generation.id),
            Some(GenerationId(1)),
            "the open one is found past a closed one and before a later closed one"
        );

        let opened = task.open_mut().expect("the open generation");
        assert_eq!(opened.id, GenerationId(1));
        opened.class = GenerationClass::Closed;
        assert!(
            task.open().is_none(),
            "closing the last open generation leaves none"
        );
    }
}
