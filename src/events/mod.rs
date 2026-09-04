//! Extended notes: `docs/internals/events/mod.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fmt;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::UpstrokeError;
use crate::interaction::QuestionRecord;
use crate::ir::{Answer, Effort, Question, QuestionId, ResolvedEffortPolicy, Tier};
use crate::ladder::{FailureKind, FailureOrigin};
use crate::util;

pub mod log;

pub use log::{EventLog, LogTail, read_all};
pub(crate) use log::{ParsedLines, parse_bytes, read_bytes};

pub const SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub ts: String,
    #[serde(flatten)]
    pub body: EventBody,
}

impl Event {
    pub fn now(body: EventBody) -> Self {
        Self {
            ts: util::rfc3339_utc_now(),
            body,
        }
    }

    pub fn task(&self) -> Option<&str> {
        self.body.task()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventBody {
    RunStarted {
        data: Box<RunStarted>,
    },
    RunResumed {
        data: RunResumed,
    },
    RunSchemaUpgraded {
        data: RunSchemaUpgraded,
    },
    AttemptStarted {
        task: String,
        attempt: u32,
        rung: u32,
        profile: String,
        data: AttemptStarted,
    },
    AttemptFinished {
        task: String,
        attempt: u32,
        rung: u32,
        profile: String,
        data: Box<AttemptRecord>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parking: Option<Box<AttemptParking>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition: Option<Box<AttemptTransition>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prepared_commit: Option<Box<PreparedCommit>>,
    },
    AttemptInterrupted {
        task: String,
        attempt: u32,
        rung: u32,
        profile: String,
        data: Box<AttemptRecord>,
    },
    LadderRetry {
        task: String,
        attempt: u32,
        rung: u32,
        data: LadderRetry,
    },
    LadderEscalated {
        task: String,
        attempt: u32,
        rung: u32,
        data: LadderEscalated,
    },
    TaskDeferred {
        task: String,
        data: TaskDeferred,
    },
    DeferWaitElapsed {
        data: DeferWaitElapsed,
    },
    TaskParked {
        task: String,
        data: TaskParked,
    },
    TaskCommitted {
        task: String,
        data: TaskCommitted,
    },
    TaskFailed {
        task: String,
        data: TaskFailed,
    },
    QuestionRaised {
        task: String,
        data: Box<QuestionRaised>,
    },
    QuestionAnswered {
        data: QuestionAnswered,
    },
    DesignDefect {
        data: DesignDefect,
    },
    CapacitySnapshot {
        data: CapacitySnapshot,
    },
    PoolExhausted {
        task: String,
        data: PoolExhausted,
    },
    BudgetExceeded {
        data: BudgetExceeded,
    },
    RunFinished {
        data: RunFinished,
    },
}

impl EventBody {
    pub fn task(&self) -> Option<&str> {
        match self {
            Self::AttemptStarted { task, .. }
            | Self::AttemptFinished { task, .. }
            | Self::AttemptInterrupted { task, .. }
            | Self::LadderRetry { task, .. }
            | Self::LadderEscalated { task, .. }
            | Self::TaskDeferred { task, .. }
            | Self::TaskParked { task, .. }
            | Self::TaskCommitted { task, .. }
            | Self::TaskFailed { task, .. }
            | Self::PoolExhausted { task, .. }
            | Self::QuestionRaised { task, .. } => Some(task),
            Self::RunStarted { .. }
            | Self::RunResumed { .. }
            | Self::RunSchemaUpgraded { .. }
            | Self::DeferWaitElapsed { .. }
            | Self::QuestionAnswered { .. }
            | Self::DesignDefect { .. }
            | Self::CapacitySnapshot { .. }
            | Self::BudgetExceeded { .. }
            | Self::RunFinished { .. } => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::RunResumed { .. } => "run_resumed",
            Self::RunSchemaUpgraded { .. } => "run_schema_upgraded",
            Self::AttemptStarted { .. } => "attempt_started",
            Self::AttemptFinished { .. } => "attempt_finished",
            Self::AttemptInterrupted { .. } => "attempt_interrupted",
            Self::LadderRetry { .. } => "ladder_retry",
            Self::LadderEscalated { .. } => "ladder_escalated",
            Self::TaskDeferred { .. } => "task_deferred",
            Self::DeferWaitElapsed { .. } => "defer_wait_elapsed",
            Self::TaskParked { .. } => "task_parked",
            Self::TaskCommitted { .. } => "task_committed",
            Self::TaskFailed { .. } => "task_failed",
            Self::QuestionRaised { .. } => "question_raised",
            Self::QuestionAnswered { .. } => "question_answered",
            Self::DesignDefect { .. } => "design_defect",
            Self::CapacitySnapshot { .. } => "capacity_snapshot",
            Self::PoolExhausted { .. } => "pool_exhausted",
            Self::BudgetExceeded { .. } => "budget_exceeded",
            Self::RunFinished { .. } => "run_finished",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStarted {
    pub schema: u32,
    pub upstroke_version: String,
    pub run_id: String,
    pub branch: String,
    pub base_sha: String,
    pub plan_path: String,
    pub config_path: Option<String>,
    pub plan_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_plan_digest: Option<String>,
    pub private_dir: String,
    pub gates: Vec<String>,
    pub gates_from_config: bool,
    pub interaction_mode: String,
    pub chains: Vec<ChainSummary>,
    #[serde(default)]
    pub effort_policy: Option<ResolvedEffortPolicy>,
    #[serde(default)]
    pub gate_cmds: Option<Vec<GateSummary>>,
    #[serde(default)]
    pub reviews: Option<crate::review::ReviewPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainSummary {
    pub task: String,
    pub tiers: Vec<Tier>,
    pub attempts_per: u32,
    #[serde(default)]
    pub bindings: Option<Vec<BindingSummary>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingSummary {
    pub tier: Tier,
    pub agent: String,
    pub model: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSummary {
    pub name: String,
    pub cmd: String,
    #[serde(rename = "timeout_ms", with = "crate::util::duration_millis")]
    pub timeout: Duration,
    pub shell: crate::gates::ShellKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunResumed {
    pub head_sha: String,
    pub interrupted_attempts: u32,
    #[serde(default)]
    pub discarded: Vec<String>,
    #[serde(default)]
    pub gates: Option<Vec<GateSummary>>,
    #[serde(default)]
    pub effort_policy: Option<ResolvedEffortPolicy>,
    #[serde(default)]
    pub reviews: Option<crate::review::ReviewPlan>,
    #[serde(default)]
    pub chains: Option<Vec<ChainSummary>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_plan_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSchemaUpgraded {
    pub from: u32,
    pub to: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptStarted {
    pub tier: String,
    pub agent: String,
    pub model: String,
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub preflight_cli_version: Option<String>,
    #[serde(default)]
    pub effort: Option<Effort>,
    #[serde(default)]
    pub selection_origin: Option<SelectionOrigin>,
    #[serde(default)]
    pub pool: Option<String>,
    pub resume_session: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub attempt: u32,
    pub tier: String,
    pub model: String,
    #[serde(default)]
    pub pool: Option<String>,
    pub resumed: bool,
    #[serde(rename = "duration_ms", with = "crate::util::duration_millis")]
    pub duration: Duration,
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub reviews: Vec<ReviewRecord>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub usage: Option<crate::ir::Usage>,
    pub failure: Option<FailureRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedCommit {
    pub branch_ref: String,
    pub parent_sha: String,
    pub tree_sha: String,
    pub commit_sha: String,
    pub message: String,
    pub pin_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptParking {
    pub question: Question,
    pub refund_attempt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "data", rename_all = "snake_case")]
pub enum AttemptTransition {
    Retry(LadderRetry),
    Escalate(LadderEscalated),
    Defer(TaskDeferred),
    Fail(TaskFailed),
}

impl AttemptRecord {
    #[must_use]
    pub fn is_successful(&self) -> bool {
        self.failure.is_none() && self.reviews.iter().all(|pass| pass.outcome.passed())
    }

    pub fn review_cost_usd(&self) -> Option<f64> {
        let reported: Vec<f64> = self.reviews.iter().filter_map(|r| r.cost_usd).collect();
        (!reported.is_empty()).then(|| reported.iter().sum())
    }

    pub fn review_cost_incomplete(&self) -> bool {
        self.reviews.iter().any(|r| r.cost_usd.is_none())
    }

    pub fn review_models(&self) -> Vec<String> {
        self.reviews.iter().map(|r| r.model.clone()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub pass: String,
    pub agent: String,
    pub model: String,
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub preflight_cli_version: Option<String>,
    #[serde(default)]
    pub effort: Option<Effort>,
    #[serde(default)]
    pub pool: Option<String>,
    pub cost_usd: Option<f64>,
    pub outcome: ReviewPassOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionOrigin {
    Auto,
    Pin,
    UserOverride,
    Exploration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPassOutcome {
    Passed,
    Failed,
    Unavailable,
}

impl ReviewPassOutcome {
    pub fn passed(self) -> bool {
        self == Self::Passed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureRecord {
    pub kind: FailureKind,
    pub origin: FailureOrigin,
    pub reason: String,
    #[serde(default)]
    pub detail: Option<String>,
}

impl FailureRecord {
    #[must_use]
    pub const fn shape(&self) -> crate::ladder::FailureShape {
        crate::ladder::FailureShape {
            kind: self.kind,
            origin: self.origin,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LadderRetry {
    pub resume: bool,
    pub tier: String,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LadderEscalated {
    pub to_rung: u32,
    pub tier: String,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDeferred {
    pub reason: String,
    pub defers: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeferWaitElapsed {
    #[serde(rename = "waited_ms", with = "crate::util::duration_millis")]
    pub waited: Duration,
    pub round: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskParked {
    pub question: String,
    pub refund_attempt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCommitted {
    pub sha: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFailed {
    pub kind: FailureKind,
    pub reason: String,
    pub halts_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionRaised {
    pub question: Question,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionAnswered {
    pub question: QuestionId,
    pub answer: Answer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decline_halts_run: Option<bool>,
    pub via: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignDefect {
    pub question: QuestionId,
    pub context: String,
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacitySnapshot {
    pub strategy: String,
    pub pools: Vec<PoolSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSnapshot {
    pub pool: String,
    pub agent: String,
    pub kind: String,
    pub remaining: String,
    pub confidence: String,
    pub reset_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolExhausted {
    pub pool: String,
    pub agent: String,
    pub reset_at: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    Run,
    Task,
}

impl fmt::Display for BudgetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Run => "run_usd",
            Self::Task => "task_usd",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetExceeded {
    pub budget: BudgetKind,
    pub limit_usd: f64,
    pub spent_usd: f64,
    pub task: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Complete,
    Parked,
    Halted,
    BudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFinished {
    pub outcome: RunOutcome,
    pub halted_at: Option<String>,
    pub committed: u32,
    pub parked: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Deferred,
    AwaitingInput(QuestionId),
    Done(String),
    Failed { kind: FailureKind, reason: String },
    Blocked(String),
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub attempt: u32,
    pub rung: u32,
    pub tier: String,
    pub model: String,
    pub profile: String,
    pub pool: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedAttempt {
    pub task: String,
    pub flight: InFlight,
}

impl InterruptedAttempt {
    pub fn event(&self) -> EventBody {
        EventBody::AttemptInterrupted {
            task: self.task.clone(),
            attempt: self.flight.attempt,
            rung: self.flight.rung,
            profile: self.flight.profile.clone(),
            data: Box::new(AttemptRecord {
                attempt: self.flight.attempt,
                tier: self.flight.tier.clone(),
                model: self.flight.model.clone(),
                pool: self.flight.pool.clone(),
                resumed: false,
                duration: Duration::ZERO,
                cost_usd: None,
                reviews: Vec::new(),
                session_id: None,
                usage: None,
                failure: Some(FailureRecord {
                    kind: FailureKind::Interrupted,
                    origin: FailureOrigin::Worker,
                    reason: "the engine stopped while this attempt was running; whatever it \
                             spent is unknown and nothing judged the result"
                        .to_owned(),
                    detail: None,
                }),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    pub attempt: u32,
    pub tier: String,
    pub summary: String,
    pub detail: Option<String>,
    pub human: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    pub rung: usize,
    pub attempts_on_rung: u32,
    pub attempts: u32,
    pub session: Option<String>,
    pub resume_next: bool,
    pub feedback: Vec<Feedback>,
    pub defers: u32,
    pub records: Vec<AttemptRecord>,
    pub in_flight: Option<InFlight>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunState {
    pub task_ids: Vec<String>,
    pub states: Vec<TaskState>,
    pub progress: Vec<Progress>,
    pub questions: Vec<QuestionRecord>,
    pub order: Vec<usize>,
    pub halted_at: Option<String>,
    pub budget_stop: Option<BudgetExceeded>,
    pub finished: Option<RunFinished>,
}

impl RunState {
    pub fn new(task_ids: Vec<String>) -> Self {
        let count = task_ids.len();
        Self {
            task_ids,
            states: vec![TaskState::Pending; count],
            progress: (0..count).map(|_| Progress::default()).collect(),
            questions: Vec::new(),
            order: Vec::new(),
            halted_at: None,
            budget_stop: None,
            finished: None,
        }
    }

    pub fn index_of(&self, task: &str) -> Option<usize> {
        self.task_ids.iter().position(|id| id == task)
    }

    pub fn apply(&mut self, event: &Event) {
        match &event.body {
            EventBody::RunStarted { .. }
            | EventBody::RunSchemaUpgraded { .. }
            | EventBody::DesignDefect { .. }
            | EventBody::CapacitySnapshot { .. }
            | EventBody::PoolExhausted { .. } => {}

            EventBody::BudgetExceeded { data } => {
                self.budget_stop.get_or_insert_with(|| data.clone());
            }

            EventBody::RunResumed { .. } => {
                self.finished = None;
                for progress in &mut self.progress {
                    progress.session = None;
                    progress.resume_next = false;
                }
                for state in &mut self.states {
                    if *state == TaskState::Deferred {
                        *state = TaskState::Pending;
                    }
                }
                self.budget_stop = None;
            }

            EventBody::AttemptStarted {
                task,
                attempt,
                rung,
                profile,
                data,
            } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                if !self.order.contains(&index) {
                    self.order.push(index);
                }
                let progress = &mut self.progress[index];
                progress.rung = *rung as usize;
                progress.attempts = *attempt;
                progress.attempts_on_rung = progress.attempts_on_rung.saturating_add(1);
                progress.in_flight = Some(InFlight {
                    attempt: *attempt,
                    rung: *rung,
                    tier: data.tier.clone(),
                    model: data.model.clone(),
                    profile: profile.clone(),
                    pool: data.pool.clone(),
                });
                progress.session = data.resume_session.clone();
            }

            EventBody::AttemptInterrupted { task, data, .. } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                let progress = &mut self.progress[index];
                progress.in_flight = None;
                progress.attempts_on_rung = progress.attempts_on_rung.saturating_sub(1);
                progress.session = None;
                progress.resume_next = false;
                progress.records.push((**data).clone());
            }

            EventBody::AttemptFinished {
                task,
                attempt,
                data,
                parking,
                transition,
                ..
            } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                {
                    let progress = &mut self.progress[index];
                    progress.in_flight = None;
                    if let Some(session) = &data.session_id {
                        progress.session = Some(session.clone());
                    }
                    progress.records.push((**data).clone());
                }
                if let Some(transition) = transition {
                    self.apply_attempt_transition(task, *attempt, transition);
                }
                if let Some(parking) = parking {
                    self.progress[index].session = None;
                    self.progress[index].resume_next = false;
                    if parking.refund_attempt {
                        self.progress[index].attempts_on_rung =
                            self.progress[index].attempts_on_rung.saturating_sub(1);
                    }
                    self.questions
                        .push(QuestionRecord::open(parking.question.clone()));
                    self.states[index] = TaskState::AwaitingInput(parking.question.id.clone());
                }
            }

            EventBody::LadderRetry {
                task,
                attempt,
                data,
                ..
            } => self.apply_ladder_retry(task, *attempt, data),

            EventBody::LadderEscalated {
                task,
                attempt,
                data,
                ..
            } => self.apply_ladder_escalated(task, *attempt, data),

            EventBody::TaskDeferred { task, data } => self.apply_task_deferred(task, data),

            EventBody::DeferWaitElapsed { .. } => {
                for state in &mut self.states {
                    if *state == TaskState::Deferred {
                        *state = TaskState::Pending;
                    }
                }
            }

            EventBody::TaskParked { task, data } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                self.progress[index].session = None;
                self.progress[index].resume_next = false;
                if data.refund_attempt {
                    let progress = &mut self.progress[index];
                    progress.attempts_on_rung = progress.attempts_on_rung.saturating_sub(1);
                }
                self.states[index] = TaskState::AwaitingInput(QuestionId(data.question.clone()));
            }

            EventBody::TaskCommitted { task, data } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                self.states[index] = TaskState::Done(data.sha.clone());
            }

            EventBody::TaskFailed { task, data } => self.apply_task_failed(task, data),

            EventBody::QuestionRaised { data, .. } => {
                self.questions
                    .push(QuestionRecord::open(data.question.clone()));
            }

            EventBody::QuestionAnswered { data } => self.answer_question(data),

            EventBody::RunFinished { data } => self.finished = Some(data.clone()),
        }
    }

    fn apply_attempt_transition(
        &mut self,
        task: &str,
        attempt: u32,
        transition: &AttemptTransition,
    ) {
        match transition {
            AttemptTransition::Retry(data) => self.apply_ladder_retry(task, attempt, data),
            AttemptTransition::Escalate(data) => {
                self.apply_ladder_escalated(task, attempt, data);
            }
            AttemptTransition::Defer(data) => self.apply_task_deferred(task, data),
            AttemptTransition::Fail(data) => self.apply_task_failed(task, data),
        }
    }

    fn apply_ladder_retry(&mut self, task: &str, attempt: u32, data: &LadderRetry) {
        let Some(index) = self.index_of(task) else {
            return;
        };
        let progress = &mut self.progress[index];
        progress.feedback.push(Feedback {
            attempt,
            tier: data.tier.clone(),
            summary: data.summary.clone(),
            detail: data.detail.clone(),
            human: false,
        });
        progress.resume_next = data.resume;
    }

    fn apply_ladder_escalated(&mut self, task: &str, attempt: u32, data: &LadderEscalated) {
        let Some(index) = self.index_of(task) else {
            return;
        };
        let progress = &mut self.progress[index];
        progress.feedback.push(Feedback {
            attempt,
            tier: data.tier.clone(),
            summary: data.summary.clone(),
            detail: data.detail.clone(),
            human: false,
        });
        progress.rung = data.to_rung as usize;
        progress.attempts_on_rung = 0;
        progress.session = None;
        progress.resume_next = false;
    }

    fn apply_task_deferred(&mut self, task: &str, data: &TaskDeferred) {
        let Some(index) = self.index_of(task) else {
            return;
        };
        let progress = &mut self.progress[index];
        progress.attempts_on_rung = progress.attempts_on_rung.saturating_sub(1);
        progress.defers = data.defers;
        progress.session = None;
        progress.resume_next = false;
        self.states[index] = TaskState::Deferred;
    }

    fn apply_task_failed(&mut self, task: &str, data: &TaskFailed) {
        let Some(index) = self.index_of(task) else {
            return;
        };
        self.states[index] = TaskState::Failed {
            kind: data.kind,
            reason: data.reason.clone(),
        };
        if data.halts_run {
            self.halted_at.get_or_insert_with(|| task.to_owned());
        }
    }

    fn answer_question(&mut self, data: &QuestionAnswered) {
        let Some(position) = self
            .questions
            .iter()
            .position(|record| record.question.id == data.question)
        else {
            return;
        };
        if !self.questions[position].is_open() {
            return;
        }
        self.questions[position].answer = Some(data.answer.clone());
        let Answer::Answered { text } = &data.answer else {
            return;
        };
        let kind = self.questions[position].question.kind;
        let affected = self.questions[position].question.affected_tasks.clone();
        for task_id in affected {
            let Some(index) = self.index_of(task_id.as_str()) else {
                continue;
            };
            if self.states[index] != TaskState::AwaitingInput(data.question.clone()) {
                continue;
            }
            let progress = &mut self.progress[index];
            let canned = self.questions[position]
                .question
                .options
                .iter()
                .any(|option| option == text);
            if kind != crate::ir::QuestionKind::ApproveSpend && !canned {
                progress.feedback.push(Feedback {
                    attempt: progress.attempts,
                    tier: String::new(),
                    summary: "the operator answered the open question".to_owned(),
                    detail: Some(text.clone()),
                    human: true,
                });
            }
            if kind == crate::ir::QuestionKind::Unblock {
                progress.attempts_on_rung = 0;
            }
            progress.defers = 0;
            progress.resume_next = false;
            self.states[index] = TaskState::Pending;
        }
    }

    pub fn interrupted_attempts(&self) -> Vec<InterruptedAttempt> {
        self.task_ids
            .iter()
            .zip(&self.progress)
            .filter_map(|(task, progress)| {
                progress.in_flight.clone().map(|flight| InterruptedAttempt {
                    task: task.clone(),
                    flight,
                })
            })
            .collect()
    }

    pub fn settle_interrupted(&mut self) -> u32 {
        let dangling = self.interrupted_attempts();
        for interrupted in &dangling {
            self.apply(&Event::now(interrupted.event()));
        }
        u32::try_from(dangling.len()).unwrap_or(u32::MAX)
    }

    pub fn open_questions(&self) -> Vec<&QuestionRecord> {
        self.questions
            .iter()
            .filter(|record| record.is_open())
            .collect()
    }
}

#[derive(Debug)]
pub struct Replay {
    pub state: RunState,
    pub started: RunStarted,
    pub resumes: u32,
    pub events: Vec<Event>,
}

pub(crate) fn normalized_plan_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub(crate) fn recorded_normalized_plan_digest(events: &[Event]) -> Option<&str> {
    let mut schema = 0;
    for event in events {
        match &event.body {
            EventBody::RunStarted { data } => {
                schema = data.schema;
                if schema >= 3 {
                    if let Some(digest) = data.normalized_plan_digest.as_deref() {
                        return Some(digest);
                    }
                }
            }
            EventBody::RunSchemaUpgraded { data } => schema = data.to,
            EventBody::RunResumed { data } if schema >= 3 => {
                if let Some(digest) = data.normalized_plan_digest.as_deref() {
                    return Some(digest);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn recorded_gates(events: &[Event]) -> Option<&Vec<GateSummary>> {
    events.iter().find_map(|event| match &event.body {
        EventBody::RunStarted { data } => data.gate_cmds.as_ref(),
        EventBody::RunResumed { data } => data.gates.as_ref(),
        _ => None,
    })
}

pub fn recorded_effort_policy(events: &[Event]) -> Option<ResolvedEffortPolicy> {
    events.iter().find_map(|event| match &event.body {
        EventBody::RunStarted { data } => data.effort_policy,
        EventBody::RunResumed { data } => data.effort_policy,
        _ => None,
    })
}

pub fn recorded_reviews(events: &[Event]) -> Option<&crate::review::ReviewPlan> {
    recorded_complete_reviews(events).or_else(|| {
        events.iter().find_map(|event| match &event.body {
            EventBody::RunStarted { data } => data.reviews.as_ref(),
            EventBody::RunResumed { data } => data.reviews.as_ref(),
            _ => None,
        })
    })
}

pub fn recorded_complete_reviews(events: &[Event]) -> Option<&crate::review::ReviewPlan> {
    let started = events.iter().find_map(|event| match &event.body {
        EventBody::RunStarted { data } => Some(&**data),
        _ => None,
    })?;
    if started.schema >= 3 {
        return started
            .reviews
            .as_ref()
            .filter(|plan| plan.pass_timeout_secs.is_some());
    }

    let mut schema = started.schema;
    events.iter().find_map(|event| match &event.body {
        EventBody::RunSchemaUpgraded { data } if data.from == schema && data.to > schema => {
            schema = data.to;
            None
        }
        EventBody::RunResumed { data } if schema >= 3 => data
            .reviews
            .as_ref()
            .filter(|plan| plan.pass_timeout_secs.is_some()),
        _ => None,
    })
}

pub fn recorded_chains(events: &[Event]) -> Option<&Vec<ChainSummary>> {
    events.iter().find_map(|event| match &event.body {
        EventBody::RunStarted { data }
            if data.chains.iter().all(|chain| chain.bindings.is_some()) =>
        {
            Some(&data.chains)
        }
        EventBody::RunResumed { data } => data.chains.as_ref(),
        _ => None,
    })
}

pub fn started_of<'a>(events: &'a [Event], path: &Path) -> Result<&'a RunStarted, UpstrokeError> {
    events
        .iter()
        .find_map(|event| match &event.body {
            EventBody::RunStarted { data } => Some(&**data),
            _ => None,
        })
        .ok_or_else(|| UpstrokeError::EventLog {
            path: path.to_path_buf(),
            message: "no run_started event — this log never recorded how the run began, so \
                      there is nothing to verify a resume against"
                .to_owned(),
        })
}

pub fn replay(
    events: Vec<Event>,
    task_ids: Vec<String>,
    path: &Path,
) -> Result<Replay, UpstrokeError> {
    let started = started_of(&events, path)?.clone();
    ensure_supported_schema(&started, &events, path)?;

    let mut state = RunState::new(task_ids);
    let mut resumes = 0;
    for event in &events {
        if matches!(event.body, EventBody::RunResumed { .. }) {
            resumes += 1;
        }
        state.apply(event);
    }
    Ok(Replay {
        state,
        started,
        resumes,
        events,
    })
}

pub(crate) fn ensure_supported_schema(
    started: &RunStarted,
    events: &[Event],
    path: &Path,
) -> Result<u32, UpstrokeError> {
    let mut effective = started.schema;
    for event in events {
        let EventBody::RunSchemaUpgraded { data } = &event.body else {
            continue;
        };
        if data.from != effective || data.to <= data.from {
            return Err(UpstrokeError::EventLog {
                path: path.to_path_buf(),
                message: format!(
                    "invalid schema transition {} -> {} while the log was at schema {}",
                    data.from, data.to, effective
                ),
            });
        }
        effective = data.to;
    }
    if effective > SCHEMA_VERSION {
        return Err(UpstrokeError::EventLog {
            path: path.to_path_buf(),
            message: format!(
                "written by a newer upstroke (event schema {}, this binary understands {}). \
                 Upgrade rather than interpret it — reading a log we only half understand would \
                 derive the wrong state silently.",
                effective, SCHEMA_VERSION
            ),
        });
    }
    let mut event_schema = started.schema;
    let mut pending_prepared: Option<(String, PreparedCommit)> = None;
    for event in events {
        if let EventBody::RunSchemaUpgraded { data } = &event.body {
            event_schema = data.to;
            continue;
        }
        if event_schema < 3 {
            continue;
        }
        if let Some((task, prepared)) = pending_prepared.as_ref() {
            match &event.body {
                EventBody::TaskCommitted {
                    task: committed_task,
                    data,
                } if committed_task == task
                    && data.sha == prepared.commit_sha
                    && data.message == prepared.message =>
                {
                    pending_prepared = None;
                    continue;
                }
                _ => {
                    return Err(UpstrokeError::EventLog {
                        path: path.to_path_buf(),
                        message: format!(
                            "event schema 3 requires the successful settlement for `{task}` to \
                             be followed by task_committed for its exact prepared commit"
                        ),
                    });
                }
            }
        }
        match &event.body {
            EventBody::AttemptFinished {
                task,
                attempt,
                data,
                parking,
                transition,
                prepared_commit,
                ..
            } => {
                let failed = data.failure.is_some();
                if data.attempt != *attempt {
                    return Err(UpstrokeError::EventLog {
                        path: path.to_path_buf(),
                        message: "event schema 3 attempt_finished envelope and record disagree on the attempt number".to_owned(),
                    });
                }
                let decided = parking.is_some() || transition.is_some();
                if failed != decided {
                    return Err(UpstrokeError::EventLog {
                        path: path.to_path_buf(),
                        message: "event schema 3 requires every failed attempt_finished to carry its ladder/parking decision, and forbids one on a successful attempt".to_owned(),
                    });
                }
                match (failed, prepared_commit.as_deref()) {
                    (true, Some(_)) => {
                        return Err(UpstrokeError::EventLog {
                            path: path.to_path_buf(),
                            message: "event schema 3 forbids a failed attempt_finished from carrying a prepared commit".to_owned(),
                        });
                    }
                    (false, None) => {
                        return Err(UpstrokeError::EventLog {
                            path: path.to_path_buf(),
                            message: "event schema 3 requires every successful attempt_finished to bind the exact prepared commit".to_owned(),
                        });
                    }
                    (false, Some(prepared)) if !valid_prepared_commit_shape(prepared) => {
                        return Err(UpstrokeError::EventLog {
                            path: path.to_path_buf(),
                            message: "event schema 3 successful attempt_finished carries an invalid prepared commit identity".to_owned(),
                        });
                    }
                    (false, Some(prepared)) => {
                        let Some(task_index) =
                            started.chains.iter().position(|chain| chain.task == *task)
                        else {
                            return Err(UpstrokeError::EventLog {
                                path: path.to_path_buf(),
                                message: format!(
                                    "event schema 3 successful settlement names unknown task `{task}`"
                                ),
                            });
                        };
                        let expected_pin = format!(
                            "refs/upstroke/prepared/{}/{task_index}-{}",
                            started.run_id, attempt
                        );
                        let expected_branch = format!("refs/heads/{}", started.branch);
                        let expected_prefix = format!("[upstroke] {task}: ");
                        if prepared.branch_ref != expected_branch
                            || prepared.pin_ref != expected_pin
                            || !prepared.message.starts_with(&expected_prefix)
                        {
                            return Err(UpstrokeError::EventLog {
                                path: path.to_path_buf(),
                                message: format!(
                                    "event schema 3 successful settlement for `{task}` carries a \
                                     branch, prepared ref, or message that is not deterministic for this run"
                                ),
                            });
                        }
                        pending_prepared = Some((task.clone(), prepared.clone()));
                    }
                    _ => {}
                }
                if let Some(failure) = data.failure.as_ref() {
                    if !valid_attempt_decision(
                        task,
                        failure,
                        transition.as_deref(),
                        parking.as_deref(),
                    ) {
                        return Err(UpstrokeError::EventLog {
                            path: path.to_path_buf(),
                            message: "event schema 3 attempt_finished carries a ladder/parking decision inconsistent with its failure".to_owned(),
                        });
                    }
                }
            }
            EventBody::QuestionAnswered { data }
                if data.answer == Answer::Declined && data.decline_halts_run.is_none() =>
            {
                return Err(UpstrokeError::EventLog {
                    path: path.to_path_buf(),
                    message: "event schema 3 requires a declined question_answered to record its contemporaneous halt policy".to_owned(),
                });
            }
            EventBody::TaskCommitted { task, .. } => {
                return Err(UpstrokeError::EventLog {
                    path: path.to_path_buf(),
                    message: format!(
                        "event schema 3 task_committed for `{task}` has no immediately preceding \
                         successful settlement with an exact prepared commit"
                    ),
                });
            }
            _ => {}
        }
    }
    if started.schema >= 3 {
        if !started
            .normalized_plan_digest
            .as_deref()
            .is_some_and(valid_normalized_plan_digest)
        {
            return Err(UpstrokeError::EventLog {
                path: path.to_path_buf(),
                message: "event schema 3 requires run_started.normalized_plan_digest to bind the exact frozen plan bytes".to_owned(),
            });
        }
        let plan = started.reviews.as_ref().ok_or_else(|| UpstrokeError::EventLog {
            path: path.to_path_buf(),
            message: "event schema 3 requires run_started.reviews; refusing to re-derive a missing verification identity".to_owned(),
        })?;
        match plan.pass_timeout_secs {
            Some(seconds) if seconds > 0 => {}
            Some(_) => {
                return Err(UpstrokeError::EventLog {
                    path: path.to_path_buf(),
                    message: "event schema 3 requires run_started.reviews.pass_timeout_secs to be positive".to_owned(),
                });
            }
            None => {
                return Err(UpstrokeError::EventLog {
                    path: path.to_path_buf(),
                    message: "event schema 3 requires run_started.reviews.pass_timeout_secs to be present; refusing to inherit a binary default".to_owned(),
                });
            }
        }
        validate_review_identity(plan, started.chains.len(), path)?;
    } else if effective >= 3 {
        let mut schema = started.schema;
        let mut complete = false;
        for event in events {
            match &event.body {
                EventBody::RunSchemaUpgraded { data } => schema = data.to,
                EventBody::RunResumed { data } if schema >= 3 => {
                    if !data
                        .normalized_plan_digest
                        .as_deref()
                        .is_some_and(valid_normalized_plan_digest)
                    {
                        return Err(UpstrokeError::EventLog {
                            path: path.to_path_buf(),
                            message: "the first schema-3 run_resumed must record the exact normalized-plan byte digest".to_owned(),
                        });
                    }
                    let plan = data.reviews.as_ref().ok_or_else(|| UpstrokeError::EventLog {
                        path: path.to_path_buf(),
                        message: "the first schema-3 run_resumed must record the complete review identity".to_owned(),
                    })?;
                    match plan.pass_timeout_secs {
                        Some(seconds) if seconds > 0 => {}
                        _ => {
                            return Err(UpstrokeError::EventLog {
                                path: path.to_path_buf(),
                                message: "the first schema-3 run_resumed requires a positive recorded review timeout".to_owned(),
                            });
                        }
                    }
                    validate_review_identity(plan, started.chains.len(), path)?;
                    complete = true;
                }
                _ => {}
            }
            if complete {
                break;
            }
        }
    }
    Ok(effective)
}

fn valid_normalized_plan_digest(digest: &str) -> bool {
    digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn valid_prepared_commit_shape(prepared: &PreparedCommit) -> bool {
    let valid_oid = |oid: &str| {
        matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    prepared.branch_ref.starts_with("refs/heads/")
        && !prepared.branch_ref.contains("..")
        && valid_oid(&prepared.parent_sha)
        && valid_oid(&prepared.tree_sha)
        && valid_oid(&prepared.commit_sha)
        && prepared.parent_sha.len() == prepared.tree_sha.len()
        && prepared.parent_sha.len() == prepared.commit_sha.len()
        && prepared.parent_sha != prepared.commit_sha
        && !prepared.message.trim().is_empty()
        && !prepared.message.contains('\r')
        && !prepared.message.contains('\n')
        && prepared.pin_ref.starts_with("refs/upstroke/prepared/")
        && !prepared.pin_ref.contains("..")
        && prepared
            .pin_ref
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyUnsettledFailure {
    pub task: String,
    pub attempt: u32,
    pub rung: u32,
    pub kind: LegacyUnsettledFailureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyUnsettledFailureKind {
    MissingDecision,
    MissingSpendParking,
}

pub(crate) fn legacy_unsettled_failure(
    started_schema: u32,
    events: &[Event],
) -> Option<LegacyUnsettledFailure> {
    let mut schema = started_schema;
    let mut pending = Vec::<LegacyUnsettledFailure>::new();
    let mut latest_escalations = Vec::<LegacyUnsettledFailure>::new();
    let mut pending_spend_parks = Vec::<LegacyUnsettledFailure>::new();

    for event in events {
        match &event.body {
            EventBody::RunSchemaUpgraded { data } => schema = data.to,
            EventBody::AttemptFinished {
                task,
                attempt,
                rung,
                data,
                parking,
                transition,
                ..
            } if schema < 3
                && data.failure.is_some()
                && parking.is_none()
                && transition.is_none() =>
            {
                pending.push(LegacyUnsettledFailure {
                    task: task.clone(),
                    attempt: *attempt,
                    rung: *rung,
                    kind: LegacyUnsettledFailureKind::MissingDecision,
                });
            }
            EventBody::LadderRetry {
                task,
                attempt,
                rung,
                ..
            } => pending.retain(|failure| {
                failure.task != *task || failure.attempt != *attempt || failure.rung != *rung
            }),
            EventBody::LadderEscalated {
                task,
                attempt,
                rung,
                ..
            } => {
                pending.retain(|failure| {
                    failure.task != *task || failure.attempt != *attempt || failure.rung != *rung
                });
                latest_escalations.retain(|failure| failure.task != *task);
                latest_escalations.push(LegacyUnsettledFailure {
                    task: task.clone(),
                    attempt: *attempt,
                    rung: *rung,
                    kind: LegacyUnsettledFailureKind::MissingSpendParking,
                });
            }
            EventBody::QuestionRaised { task, data }
                if data.question.kind == crate::ir::QuestionKind::ApproveSpend =>
            {
                if let Some(escalation) = latest_escalations
                    .iter()
                    .rev()
                    .find(|failure| failure.task == *task)
                    .cloned()
                {
                    pending_spend_parks.retain(|failure| failure.task != *task);
                    pending_spend_parks.push(escalation);
                }
            }
            EventBody::TaskDeferred { task, .. } | EventBody::TaskFailed { task, .. } => {
                pending.retain(|failure| failure.task != *task);
                latest_escalations.retain(|failure| failure.task != *task);
            }
            EventBody::TaskParked { task, .. } => {
                pending.retain(|failure| failure.task != *task);
                latest_escalations.retain(|failure| failure.task != *task);
                pending_spend_parks.retain(|failure| failure.task != *task);
            }
            EventBody::AttemptStarted { task, .. } => {
                latest_escalations.retain(|failure| failure.task != *task);
            }
            _ => {}
        }
    }

    pending_spend_parks
        .into_iter()
        .next()
        .or_else(|| pending.into_iter().next())
}

pub(crate) fn validate_review_identity(
    plan: &crate::review::ReviewPlan,
    task_count: usize,
    path: &Path,
) -> Result<(), UpstrokeError> {
    let enabled = plan.enabled.ok_or_else(|| UpstrokeError::EventLog {
        path: path.to_path_buf(),
        message: "event schema 3 requires reviews.enabled; refusing to infer whether verification was intentionally disabled".to_owned(),
    })?;
    if enabled != plan.primary.is_some() {
        return Err(UpstrokeError::EventLog {
            path: path.to_path_buf(),
            message: "event schema 3 reviews.enabled does not match the recorded primary reviewer"
                .to_owned(),
        });
    }
    let alternative_available =
        plan.alternative_available
            .ok_or_else(|| UpstrokeError::EventLog {
                path: path.to_path_buf(),
                message: "event schema 3 requires reviews.alternative_available; refusing to infer a missing reviewer binding".to_owned(),
            })?;
    if alternative_available != plan.alternative.is_some() {
        return Err(UpstrokeError::EventLog {
            path: path.to_path_buf(),
            message: "event schema 3 reviews.alternative_available does not match the recorded alternative reviewer".to_owned(),
        });
    }
    if enabled && plan.second_opinion.len() != task_count {
        return Err(UpstrokeError::EventLog {
            path: path.to_path_buf(),
            message: format!(
                "event schema 3 records {task_count} task chains but {} second-opinion slots; refusing a misaligned review identity",
                plan.second_opinion.len()
            ),
        });
    }
    if !enabled && (plan.alternative.is_some() || plan.second_opinion.iter().any(Option::is_some)) {
        return Err(UpstrokeError::EventLog {
            path: path.to_path_buf(),
            message: "event schema 3 disables review but still records review-pass bindings"
                .to_owned(),
        });
    }
    Ok(())
}

fn valid_attempt_decision(
    task: &str,
    failure: &FailureRecord,
    transition: Option<&AttemptTransition>,
    parking: Option<&AttemptParking>,
) -> bool {
    let associated = |parking: &AttemptParking| {
        parking.question.affected_tasks.len() == 1
            && parking.question.affected_tasks[0].as_str() == task
    };
    let outage = matches!(
        (failure.kind, failure.origin),
        (FailureKind::RateLimited | FailureKind::ReviewUnavailable, _)
            | (FailureKind::Timeout, FailureOrigin::Reviewer)
    );

    if failure.kind == FailureKind::NeedsHuman {
        return transition.is_none()
            && parking.is_some_and(|parking| {
                associated(parking)
                    && parking.question.kind == crate::ir::QuestionKind::Clarify
                    && parking.refund_attempt
            });
    }
    if matches!(
        failure.kind,
        FailureKind::ReviewInputTooLarge | FailureKind::ReviewInputOpaque
    ) {
        return failure.origin == FailureOrigin::Reviewer
            && transition.is_none()
            && parking.is_some_and(|parking| {
                associated(parking)
                    && parking.question.kind == crate::ir::QuestionKind::Unblock
                    && !parking.refund_attempt
            });
    }
    if outage {
        return matches!(
            (transition, parking),
            (Some(AttemptTransition::Defer(_)), None)
        ) || matches!((transition, parking), (None, Some(parking))
                if associated(parking)
                    && parking.question.kind == crate::ir::QuestionKind::Unblock
                    && parking.refund_attempt);
    }
    if matches!(
        failure.kind,
        FailureKind::Declined | FailureKind::Interrupted
    ) {
        return false;
    }

    match (transition, parking) {
        (Some(AttemptTransition::Retry(_)), None)
        | (Some(AttemptTransition::Escalate(_)), None) => true,
        (Some(AttemptTransition::Escalate(_)), Some(parking)) => {
            associated(parking)
                && parking.question.kind == crate::ir::QuestionKind::ApproveSpend
                && !parking.refund_attempt
        }
        (Some(AttemptTransition::Fail(data)), None) => data.kind == failure.kind,
        (None, Some(parking)) => {
            associated(parking)
                && parking.question.kind == crate::ir::QuestionKind::Unblock
                && !parking.refund_attempt
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{QuestionKind, TaskId};
    use crate::topology::effects::EventSite;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;

    fn effort_policy() -> ResolvedEffortPolicy {
        ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::Medium,
            frontier: Effort::XHigh,
            review: Effort::Max,
        }
    }

    fn started() -> EventBody {
        EventBody::RunStarted {
            data: Box::new(RunStarted {
                schema: SCHEMA_VERSION,
                upstroke_version: "0.0.1".to_owned(),
                run_id: "01RUN".to_owned(),
                branch: "upstroke/run-01RUN".to_owned(),
                base_sha: "abc123".to_owned(),
                plan_path: "plan.md".to_owned(),
                config_path: None,
                plan_hash: "deadbeef".to_owned(),
                normalized_plan_digest: Some(format!("sha256:{}", "0".repeat(64))),
                private_dir: "/home/x/.upstroke/runs/01RUN".to_owned(),
                gates: vec!["check".to_owned()],
                gates_from_config: true,
                reviews: Some(crate::review::ReviewPlan::default()),
                interaction_mode: "on_block".to_owned(),
                chains: vec![ChainSummary {
                    task: "t1".to_owned(),
                    tiers: vec![Tier::Small, Tier::Mid],
                    attempts_per: 2,
                    bindings: Some(vec![
                        BindingSummary {
                            tier: Tier::Small,
                            agent: "claude-code".to_owned(),
                            model: "claude-haiku-4-5".to_owned(),
                            pinned: false,
                        },
                        BindingSummary {
                            tier: Tier::Mid,
                            agent: "claude-code".to_owned(),
                            model: "claude-sonnet-5".to_owned(),
                            pinned: false,
                        },
                    ]),
                }],
                effort_policy: Some(effort_policy()),
                gate_cmds: Some(vec![GateSummary {
                    name: "check".to_owned(),
                    cmd: "cargo check".to_owned(),
                    timeout: Duration::from_secs(600),
                    shell: crate::gates::ShellKind::Sh,
                }]),
            }),
        }
    }

    fn legacy_started() -> EventBody {
        let mut body = started();
        let EventBody::RunStarted { data } = &mut body else {
            unreachable!();
        };
        data.schema = 2;
        data.normalized_plan_digest = None;
        body
    }

    fn attempt_started(task: &str, attempt: u32, rung: u32, tier: &str) -> EventBody {
        EventBody::AttemptStarted {
            task: task.to_owned(),
            attempt,
            rung,
            profile: format!("{tier}-model"),
            data: AttemptStarted {
                adapter: None,
                preflight_cli_version: None,
                effort: None,
                selection_origin: None,
                tier: tier.to_owned(),
                agent: "claude-code".to_owned(),
                model: "model".to_owned(),
                pool: Some("claude-max".to_owned()),
                resume_session: None,
            },
        }
    }

    fn attempt_finished(task: &str, attempt: u32, rung: u32, tier: &str) -> EventBody {
        EventBody::AttemptFinished {
            task: task.to_owned(),
            attempt,
            rung,
            profile: format!("{tier}-model"),
            parking: None,
            transition: None,
            prepared_commit: Some(Box::new(PreparedCommit {
                branch_ref: "refs/heads/upstroke/run-01RUN".to_owned(),
                parent_sha: "1".repeat(40),
                tree_sha: "2".repeat(40),
                commit_sha: "3".repeat(40),
                message: "[upstroke] t1: task".to_owned(),
                pin_ref: format!("refs/upstroke/prepared/01RUN/0-{attempt}"),
            })),
            data: Box::new(AttemptRecord {
                attempt,
                tier: tier.to_owned(),
                model: "model".to_owned(),
                pool: Some("claude-max".to_owned()),
                resumed: false,
                duration: Duration::from_millis(1500),
                cost_usd: Some(0.01),
                reviews: Vec::new(),
                session_id: Some("s0".to_owned()),
                usage: None,
                failure: None,
            }),
        }
    }

    fn question(id: &str, task: &str) -> Question {
        Question {
            id: QuestionId::from(id),
            kind: QuestionKind::Unblock,
            affected_tasks: vec![TaskId::from(task)],
            context: "nothing else can move this".to_owned(),
            options: vec!["retry".to_owned()],
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("upstroke-events-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn the_envelope_matches_the_shape_the_spec_documents() {
        let event = Event::now(attempt_started("t1", 2, 1, "mid"));
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&event).expect("serialize"))
                .expect("valid json");
        assert_eq!(json["event"], "attempt_started");
        assert_eq!(json["task"], "t1");
        assert_eq!(json["attempt"], 2);
        assert_eq!(json["rung"], 1);
        assert_eq!(json["profile"], "mid-model");
        assert_eq!(json["data"]["tier"], "mid");
        assert!(
            json["ts"].as_str().is_some_and(|ts| ts.ends_with('Z')),
            "{json}"
        );
        let plain = Event::now(EventBody::DeferWaitElapsed {
            data: DeferWaitElapsed {
                waited: Duration::from_secs(60),
                round: 0,
            },
        });
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&plain).expect("serialize"))
                .expect("valid json");
        assert!(json.get("task").is_none(), "{json}");
        assert_eq!(json["data"]["waited_ms"], 60_000);
    }

    #[test]
    fn every_event_kind_round_trips() {
        let bodies = vec![
            started(),
            EventBody::RunResumed {
                data: RunResumed {
                    head_sha: "abc".to_owned(),
                    interrupted_attempts: 1,
                    discarded: Vec::new(),
                    gates: None,
                    effort_policy: None,
                    reviews: None,
                    chains: None,
                    normalized_plan_digest: None,
                },
            },
            EventBody::RunSchemaUpgraded {
                data: RunSchemaUpgraded { from: 1, to: 2 },
            },
            attempt_started("t1", 1, 0, "small"),
            attempt_finished("t1", 1, 0, "small"),
            EventBody::LadderRetry {
                task: "t1".to_owned(),
                attempt: 1,
                rung: 0,
                data: LadderRetry {
                    resume: true,
                    tier: "small".to_owned(),
                    summary: "gate failed".to_owned(),
                    detail: Some("error[E0308]".to_owned()),
                },
            },
            EventBody::LadderEscalated {
                task: "t1".to_owned(),
                attempt: 2,
                rung: 0,
                data: LadderEscalated {
                    to_rung: 1,
                    tier: "small".to_owned(),
                    summary: "still failing".to_owned(),
                    detail: None,
                },
            },
            EventBody::TaskDeferred {
                task: "t1".to_owned(),
                data: TaskDeferred {
                    reason: "rate limited".to_owned(),
                    defers: 1,
                },
            },
            EventBody::DeferWaitElapsed {
                data: DeferWaitElapsed {
                    waited: Duration::from_secs(60),
                    round: 0,
                },
            },
            EventBody::TaskParked {
                task: "t1".to_owned(),
                data: TaskParked {
                    question: "q-1".to_owned(),
                    refund_attempt: true,
                },
            },
            EventBody::TaskCommitted {
                task: "t1".to_owned(),
                data: TaskCommitted {
                    sha: "abc123".to_owned(),
                    message: "[upstroke] t1: do it".to_owned(),
                },
            },
            EventBody::TaskFailed {
                task: "t1".to_owned(),
                data: TaskFailed {
                    kind: FailureKind::Declined,
                    reason: "declined".to_owned(),
                    halts_run: true,
                },
            },
            EventBody::QuestionRaised {
                task: "t1".to_owned(),
                data: Box::new(QuestionRaised {
                    question: question("q-1", "t1"),
                }),
            },
            EventBody::QuestionAnswered {
                data: QuestionAnswered {
                    question: QuestionId::from("q-1"),
                    answer: Answer::Answered {
                        text: "use base64".to_owned(),
                    },
                    decline_halts_run: None,
                    via: "terminal".to_owned(),
                },
            },
            EventBody::DesignDefect {
                data: DesignDefect {
                    question: QuestionId::from("q-1"),
                    context: "cursor format was never decided".to_owned(),
                    answer: "use base64".to_owned(),
                },
            },
            EventBody::RunFinished {
                data: RunFinished {
                    outcome: RunOutcome::Parked,
                    halted_at: None,
                    committed: 2,
                    parked: 1,
                },
            },
        ];
        for body in bodies {
            let event = Event::now(body);
            let line = serde_json::to_string(&event).expect("serialize");
            let back: Event = serde_json::from_str(&line).expect(&line);
            assert_eq!(back, event, "{line}");
        }
    }

    #[test]
    fn durations_are_milliseconds_not_a_struct() {
        let event = Event::now(attempt_finished("t1", 1, 0, "small"));
        let line = serde_json::to_string(&event).expect("serialize");
        assert!(line.contains("\"duration_ms\":1500"), "{line}");
        assert!(!line.contains("nanos"), "{line}");
        let back: Event = serde_json::from_str(&line).expect("round-trip");
        assert_eq!(back, event);
    }

    #[test]
    fn a_torn_final_line_is_dropped_but_committed_invalid_events_are_errors() {
        let dir = scratch("torn");
        let path = dir.join("events.jsonl");
        let good = serde_json::to_string(&Event::now(started())).expect("serialize");
        let also_good = serde_json::to_string(&Event::now(attempt_started("t1", 1, 0, "small")))
            .expect("serialize");

        let torn = format!("{good}\n{also_good}\n{{\"ts\":\"2026-01-0");
        std::fs::write(&path, &torn).expect("write");
        let mut warnings = Vec::new();
        let events = read_all(&path, &mut warnings).expect("torn tail is recoverable");
        assert_eq!(events.len(), 2);
        assert!(
            warnings.iter().any(|w| w.contains("incomplete final line")),
            "warnings: {warnings:?}"
        );

        let mut invalid_utf8_tail = format!("{good}\n").into_bytes();
        invalid_utf8_tail.extend_from_slice(&[0xf0, 0x9f]);
        std::fs::write(&path, invalid_utf8_tail).expect("write split UTF-8 tail");
        let mut warnings = Vec::new();
        let events = read_all(&path, &mut warnings).expect("split UTF-8 tail is recoverable");
        assert_eq!(events.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("2 trailing byte(s)"));

        let corrupt = format!("{good}\nnot json at all\n{also_good}\n");
        std::fs::write(&path, corrupt).expect("write");
        let mut warnings = Vec::new();
        let err = read_all(&path, &mut warnings).expect_err("must not fold a rewritten log");
        assert!(err.to_string().contains("line 2"), "got: {err}");
        assert!(err.to_string().contains("confidently wrong"), "got: {err}");

        let mut invalid: serde_json::Value =
            serde_json::from_str(&also_good).expect("attempt-start JSON");
        invalid["data"]["selection_origin"] = serde_json::json!("unknown");
        let invalid = serde_json::to_string(&invalid).expect("invalid event JSON");
        std::fs::write(&path, format!("{good}\n{invalid}\n")).expect("write invalid tail");
        let mut warnings = Vec::new();
        let err = read_all(&path, &mut warnings).expect_err("semantic errors are not torn tails");
        assert!(err.to_string().contains("line 2"), "got: {err}");
        assert!(err.to_string().contains("unknown variant"), "got: {err}");
        assert!(warnings.is_empty(), "corruption is an error, not a warning");
    }

    #[test]
    fn a_valid_json_event_without_its_commit_newline_is_a_torn_tail() {
        let dir = scratch("uncommitted-valid-tail");
        let path = dir.join("events.jsonl");
        let good = serde_json::to_string(&Event::now(started())).expect("serialize");
        let uncommitted = serde_json::to_string(&Event::now(attempt_started("t1", 1, 0, "small")))
            .expect("serialize");
        std::fs::write(&path, format!("{good}\n{uncommitted}")).expect("write uncommitted tail");

        let mut warnings = Vec::new();
        let events = read_all(&path, &mut warnings).expect("uncommitted tail is recoverable");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body.kind(), "run_started");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("incomplete final line"));
    }

    #[test]
    fn appending_after_a_torn_line_discards_it_rather_than_splicing() {
        let dir = scratch("repair");
        let path = dir.join("events.jsonl");
        let good = serde_json::to_string(&Event::now(started())).expect("serialize");
        std::fs::write(&path, format!("{good}\n{{\"ts\":\"trunc")).expect("write");

        let mut warnings = Vec::new();
        let mut log = EventLog::open(EventSite::LegacyOpenLog, &path, &mut warnings).expect("open");
        assert!(
            warnings.iter().any(|w| w.contains("never finished")),
            "the discard is reported, not silent: {warnings:?}"
        );
        log.append(
            EventSite::LegacyAppend,
            attempt_started("t1", 1, 0, "small"),
        )
        .expect("append");

        let mut warnings = Vec::new();
        let events = read_all(&path, &mut warnings).expect("the log is clean again");
        assert_eq!(events.len(), 2, "the good first line and the new one");
        assert_eq!(events[1].body.kind(), "attempt_started");
        assert!(
            warnings.is_empty(),
            "nothing left to warn about: {warnings:?}"
        );
    }

    #[test]
    fn a_log_that_is_nothing_but_a_torn_line_opens_empty() {
        let dir = scratch("alltorn");
        let path = dir.join("events.jsonl");
        std::fs::write(&path, "{\"ts\":\"2026").expect("write");

        let mut warnings = Vec::new();
        let mut log = EventLog::open(EventSite::LegacyOpenLog, &path, &mut warnings).expect("open");
        log.append(EventSite::LegacyAppend, started())
            .expect("append");

        let mut warnings = Vec::new();
        let events = read_all(&path, &mut warnings).expect("read");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body.kind(), "run_started");
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_guessed_at() {
        let EventBody::RunStarted { mut data } = started() else {
            panic!("started() builds a run_started");
        };
        data.schema = SCHEMA_VERSION + 1;
        let events = vec![Event::now(EventBody::RunStarted { data })];
        let err = replay(events, vec!["t1".to_owned()], Path::new("events.jsonl"))
            .expect_err("must refuse a newer log");
        assert!(err.to_string().contains("Upgrade"), "got: {err}");
    }

    #[test]
    fn a_schema_upgrade_event_is_a_real_downgrade_barrier() {
        #[derive(Debug, serde::Deserialize)]
        struct SchemaOneEvent {
            #[allow(dead_code)]
            ts: String,
            #[serde(flatten)]
            #[allow(dead_code)]
            body: SchemaOneBody,
        }
        #[derive(Debug, serde::Deserialize)]
        #[serde(tag = "event", rename_all = "snake_case")]
        #[allow(dead_code)]
        enum SchemaOneBody {
            RunStarted { data: serde_json::Value },
            RunResumed { data: serde_json::Value },
        }

        let marker = Event::now(EventBody::RunSchemaUpgraded {
            data: RunSchemaUpgraded { from: 1, to: 2 },
        });
        let line = serde_json::to_string(&marker).expect("serialize marker");
        let error = serde_json::from_str::<SchemaOneEvent>(&line)
            .expect_err("a schema-1 reader must refuse the new event");
        assert!(error.to_string().contains("run_schema_upgraded"), "{error}");

        let EventBody::RunStarted { mut data } = started() else {
            panic!("started() builds a run_started");
        };
        data.schema = 1;
        for chain in &mut data.chains {
            chain.bindings = None;
        }
        let events = vec![Event::now(EventBody::RunStarted { data }), marker];
        let replayed = replay(events, vec!["t1".to_owned()], Path::new("events.jsonl"))
            .expect("the current binary follows the valid 1 -> 2 transition");
        assert_eq!(replayed.started.schema, 1);
    }

    #[test]
    fn a_future_appended_schema_transition_is_refused() {
        let events = vec![
            Event::now(started()),
            Event::now(EventBody::RunSchemaUpgraded {
                data: RunSchemaUpgraded {
                    from: SCHEMA_VERSION,
                    to: SCHEMA_VERSION + 1,
                },
            }),
        ];
        let error = replay(events, vec!["t1".to_owned()], Path::new("events.jsonl"))
            .expect_err("the opening schema is not the only compatibility boundary");
        assert!(error.to_string().contains("Upgrade"), "{error}");
    }

    #[test]
    fn run_resumed_clears_the_prior_terminal_marker() {
        let mut state = RunState::new(vec!["t1".to_owned()]);
        state.apply(&Event::now(EventBody::RunFinished {
            data: RunFinished {
                outcome: RunOutcome::Parked,
                halted_at: None,
                committed: 0,
                parked: 1,
            },
        }));
        assert!(state.finished.is_some());

        state.apply(&Event::now(EventBody::RunResumed {
            data: RunResumed {
                head_sha: "abc".to_owned(),
                interrupted_attempts: 0,
                discarded: Vec::new(),
                gates: None,
                effort_policy: None,
                reviews: None,
                chains: None,
                normalized_plan_digest: None,
            },
        }));
        assert_eq!(state.finished, None);
    }

    #[test]
    fn schema_three_rejects_incomplete_review_identity_without_legacy_defaults() {
        let EventBody::RunStarted { mut data } = started() else {
            panic!("started() builds a run_started");
        };
        data.reviews = None;
        let error = replay(
            vec![Event::now(EventBody::RunStarted { data })],
            vec!["t1".to_owned()],
            Path::new("events.jsonl"),
        )
        .expect_err("schema 3 cannot re-derive a missing reviewer identity");
        assert!(error.to_string().contains("run_started.reviews"), "{error}");

        let EventBody::RunStarted { mut data } = started() else {
            panic!("started() builds a run_started");
        };
        data.reviews
            .as_mut()
            .expect("current start records reviews")
            .pass_timeout_secs = None;
        let error = replay(
            vec![Event::now(EventBody::RunStarted { data })],
            vec!["t1".to_owned()],
            Path::new("events.jsonl"),
        )
        .expect_err("schema 3 cannot inherit a timeout from this binary");
        assert!(error.to_string().contains("pass_timeout_secs"), "{error}");

        let complete = || {
            let EventBody::RunStarted { mut data } = started() else {
                unreachable!();
            };
            let plan = data.reviews.as_mut().expect("review plan");
            plan.enabled = Some(true);
            plan.alternative_available = Some(false);
            plan.primary = Some(crate::review::PassBinding::new("codex", "gpt-5.6-sol"));
            plan.second_opinion = vec![None];
            Event::now(EventBody::RunStarted { data })
        };

        for missing in ["enabled", "alternative_available", "primary"] {
            let mut json = serde_json::to_value(complete()).expect("serialize");
            json["data"]["reviews"]
                .as_object_mut()
                .expect("review object")
                .remove(missing);
            let event: Event = serde_json::from_value(json).expect("additive field parses");
            let error = replay(
                vec![event],
                vec!["t1".to_owned()],
                Path::new("events.jsonl"),
            )
            .expect_err("schema 3 cannot default away a reviewer identity field");
            assert!(error.to_string().contains("review"), "{missing}: {error}");
        }

        let mut json = serde_json::to_value(complete()).expect("serialize");
        json["data"]["reviews"]["second_opinion"] = serde_json::json!([]);
        let event: Event = serde_json::from_value(json).expect("short vector parses");
        let error = replay(
            vec![event],
            vec!["t1".to_owned()],
            Path::new("events.jsonl"),
        )
        .expect_err("a short pass vector silently removes required task reviews");
        assert!(error.to_string().contains("misaligned"), "{error}");

        let mut undecided = attempt_finished("t1", 1, 0, "small");
        let EventBody::AttemptFinished { data, .. } = &mut undecided else {
            unreachable!();
        };
        data.failure = Some(FailureRecord {
            kind: FailureKind::GateFailed,
            origin: FailureOrigin::Worker,
            reason: "failed".to_owned(),
            detail: None,
        });
        let error = replay(
            vec![
                Event::now(started()),
                Event::now(attempt_started("t1", 1, 0, "small")),
                Event::now(undecided),
            ],
            vec!["t1".to_owned()],
            Path::new("events.jsonl"),
        )
        .expect_err("schema 3 cannot replay a failed attempt without its decision");
        assert!(
            error.to_string().contains("ladder/parking decision"),
            "{error}"
        );
    }

    #[test]
    fn schema_three_rejects_human_outage_and_review_input_decision_contradictions() {
        let retry = || {
            Some(Box::new(AttemptTransition::Retry(LadderRetry {
                resume: false,
                tier: "small".to_owned(),
                summary: "retry".to_owned(),
                detail: None,
            })))
        };
        let escalate = || {
            Some(Box::new(AttemptTransition::Escalate(LadderEscalated {
                to_rung: 1,
                tier: "small".to_owned(),
                summary: "escalate".to_owned(),
                detail: None,
            })))
        };
        let park = |kind, refund_attempt| {
            let mut question = question("q-special", "t1");
            question.kind = kind;
            Some(Box::new(AttemptParking {
                question,
                refund_attempt,
            }))
        };
        let cases = vec![
            (
                FailureKind::NeedsHuman,
                FailureOrigin::Worker,
                retry(),
                None,
            ),
            (
                FailureKind::NeedsHuman,
                FailureOrigin::Reviewer,
                None,
                park(QuestionKind::Unblock, true),
            ),
            (
                FailureKind::RateLimited,
                FailureOrigin::Worker,
                retry(),
                None,
            ),
            (
                FailureKind::Timeout,
                FailureOrigin::Reviewer,
                escalate(),
                None,
            ),
            (
                FailureKind::ReviewUnavailable,
                FailureOrigin::Reviewer,
                None,
                park(QuestionKind::Unblock, false),
            ),
            (
                FailureKind::ReviewInputTooLarge,
                FailureOrigin::Reviewer,
                retry(),
                None,
            ),
            (
                FailureKind::ReviewInputOpaque,
                FailureOrigin::Worker,
                None,
                park(QuestionKind::Unblock, false),
            ),
        ];

        for (kind, origin, transition, parking) in cases {
            let mut finished = attempt_finished("t1", 1, 0, "small");
            let EventBody::AttemptFinished {
                data,
                transition: recorded_transition,
                parking: recorded_parking,
                prepared_commit,
                ..
            } = &mut finished
            else {
                unreachable!();
            };
            data.failure = Some(FailureRecord {
                kind,
                origin,
                reason: "special failure".to_owned(),
                detail: None,
            });
            *recorded_transition = transition;
            *recorded_parking = parking;
            *prepared_commit = None;
            let error = replay(
                vec![
                    Event::now(started()),
                    Event::now(attempt_started("t1", 1, 0, "small")),
                    Event::now(finished),
                ],
                vec!["t1".to_owned()],
                Path::new("events.jsonl"),
            )
            .expect_err("schema 3 must reject a policy-contradictory settlement");
            assert!(
                error.to_string().contains("inconsistent with its failure"),
                "{kind:?}/{origin:?}: {error}"
            );
        }
    }

    #[test]
    fn legacy_schema_two_spend_question_without_task_parked_is_unsettled() {
        let mut failed = attempt_finished("t1", 1, 0, "small");
        let EventBody::AttemptFinished {
            data,
            prepared_commit,
            ..
        } = &mut failed
        else {
            unreachable!();
        };
        data.failure = Some(FailureRecord {
            kind: FailureKind::GateFailed,
            origin: FailureOrigin::Worker,
            reason: "failed".to_owned(),
            detail: None,
        });
        *prepared_commit = None;
        let mut approval = question("q-spend", "t1");
        approval.kind = QuestionKind::ApproveSpend;
        let mut log = vec![
            Event::now(legacy_started()),
            Event::now(attempt_started("t1", 1, 0, "small")),
            Event::now(failed),
            Event::now(EventBody::LadderEscalated {
                task: "t1".to_owned(),
                attempt: 1,
                rung: 0,
                data: LadderEscalated {
                    to_rung: 1,
                    tier: "small".to_owned(),
                    summary: "escalate".to_owned(),
                    detail: None,
                },
            }),
            Event::now(EventBody::QuestionRaised {
                task: "t1".to_owned(),
                data: Box::new(QuestionRaised { question: approval }),
            }),
        ];
        let unsettled = legacy_unsettled_failure(2, &log).expect("parking append is missing");
        assert_eq!(
            unsettled.kind,
            LegacyUnsettledFailureKind::MissingSpendParking
        );

        log.push(Event::now(EventBody::TaskParked {
            task: "t1".to_owned(),
            data: TaskParked {
                question: "q-spend".to_owned(),
                refund_attempt: false,
            },
        }));
        assert_eq!(legacy_unsettled_failure(2, &log), None);
    }

    #[test]
    fn schema_three_binds_task_committed_to_the_immediately_prepared_object() {
        let success = attempt_finished("t1", 1, 0, "small");
        let EventBody::AttemptFinished {
            prepared_commit: Some(prepared),
            ..
        } = &success
        else {
            unreachable!();
        };
        let prepared = (**prepared).clone();
        let committed = EventBody::TaskCommitted {
            task: "t1".to_owned(),
            data: TaskCommitted {
                sha: prepared.commit_sha.clone(),
                message: prepared.message.clone(),
            },
        };
        replay(
            vec![
                Event::now(started()),
                Event::now(attempt_started("t1", 1, 0, "small")),
                Event::now(success.clone()),
                Event::now(committed),
            ],
            vec!["t1".to_owned()],
            Path::new("events.jsonl"),
        )
        .expect("the exact prepared identity closes the settlement");

        let mut wrong_branch = success.clone();
        let EventBody::AttemptFinished {
            prepared_commit: Some(wrong_prepared),
            ..
        } = &mut wrong_branch
        else {
            unreachable!();
        };
        wrong_prepared.branch_ref = "refs/heads/unrelated".to_owned();
        let branch_error = replay(
            vec![
                Event::now(started()),
                Event::now(attempt_started("t1", 1, 0, "small")),
                Event::now(wrong_branch),
            ],
            vec!["t1".to_owned()],
            Path::new("events.jsonl"),
        )
        .expect_err("the prepared identity cannot substitute another branch");
        assert!(
            branch_error.to_string().contains("branch"),
            "{branch_error}"
        );

        let error = replay(
            vec![
                Event::now(started()),
                Event::now(attempt_started("t1", 1, 0, "small")),
                Event::now(success),
                Event::now(EventBody::TaskCommitted {
                    task: "t1".to_owned(),
                    data: TaskCommitted {
                        sha: "4".repeat(40),
                        message: prepared.message.clone(),
                    },
                }),
            ],
            vec!["t1".to_owned()],
            Path::new("events.jsonl"),
        )
        .expect_err("same subject cannot substitute another commit tree");
        assert!(
            error.to_string().contains("exact prepared commit"),
            "{error}"
        );
    }

    #[test]
    fn pre_upgrade_digest_fields_cannot_bless_a_legacy_snapshot() {
        let spoofed = format!("sha256:{}", "1".repeat(64));
        let authoritative = format!("sha256:{}", "2".repeat(64));
        let resumed = |digest| {
            Event::now(EventBody::RunResumed {
                data: RunResumed {
                    head_sha: "abc".to_owned(),
                    interrupted_attempts: 0,
                    discarded: Vec::new(),
                    gates: None,
                    effort_policy: None,
                    reviews: None,
                    chains: None,
                    normalized_plan_digest: Some(digest),
                },
            })
        };
        let mut start = legacy_started();
        let EventBody::RunStarted { data } = &mut start else {
            unreachable!();
        };
        data.normalized_plan_digest = Some(spoofed.clone());

        let before_upgrade = vec![Event::now(start.clone()), resumed(spoofed.clone())];
        assert_eq!(
            recorded_normalized_plan_digest(&before_upgrade),
            None,
            "schema-1/2 additive fields are not an authority"
        );

        let after_upgrade = vec![
            Event::now(start),
            resumed(spoofed),
            Event::now(EventBody::RunSchemaUpgraded {
                data: RunSchemaUpgraded { from: 2, to: 3 },
            }),
            resumed(authoritative.clone()),
        ];
        assert_eq!(
            recorded_normalized_plan_digest(&after_upgrade),
            Some(authoritative.as_str()),
            "only the first schema-3 resume can establish a legacy digest"
        );
    }

    #[test]
    fn a_run_started_without_gate_commands_reads_as_unrecorded() {
        let EventBody::RunStarted { mut data } = started() else {
            panic!("started() builds a run_started");
        };
        data.schema = 1;
        for chain in &mut data.chains {
            chain.bindings = None;
        }
        let mut json =
            serde_json::to_value(Event::now(EventBody::RunStarted { data })).expect("serialize");
        assert!(
            json["data"]
                .as_object_mut()
                .expect("data")
                .remove("gate_cmds")
                .is_some(),
            "a fresh run records its gates"
        );
        let event: Event = serde_json::from_value(json).expect("an old log still parses");
        let EventBody::RunStarted { data } = event.body else {
            panic!("still a run_started");
        };
        assert_eq!(data.gate_cmds, None);
    }

    #[test]
    fn a_run_started_without_an_effort_policy_reads_as_unrecorded() {
        let EventBody::RunStarted { mut data } = started() else {
            panic!("started() builds a run_started");
        };
        data.schema = 1;
        for chain in &mut data.chains {
            chain.bindings = None;
        }
        let mut json =
            serde_json::to_value(Event::now(EventBody::RunStarted { data })).expect("serialize");
        assert!(
            json["data"]
                .as_object_mut()
                .expect("data")
                .remove("effort_policy")
                .is_some(),
            "a fresh run records its effort policy"
        );
        let event: Event = serde_json::from_value(json).expect("a legacy log still parses");
        let EventBody::RunStarted { data } = &event.body else {
            panic!("still a run_started");
        };
        assert_eq!(data.schema, 1, "the legacy opening remains schema 1");
        assert_eq!(data.effort_policy, None);
        assert_eq!(recorded_effort_policy(&[event]), None);
    }

    #[test]
    fn a_recorded_effort_policy_round_trips_every_role_and_tier_exactly() {
        let EventBody::RunStarted { data } = started() else {
            panic!("started() builds a run_started");
        };
        let event = Event::now(EventBody::RunStarted { data });
        let line = serde_json::to_string(&event).expect("serialize");
        let json: serde_json::Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(json["data"]["effort_policy"]["small"], "low");
        assert_eq!(json["data"]["effort_policy"]["mid"], "medium");
        assert_eq!(json["data"]["effort_policy"]["frontier"], "xhigh");
        assert_eq!(json["data"]["effort_policy"]["review"], "max");

        let read_back: Event = serde_json::from_str(&line).expect("round trip");
        assert_eq!(recorded_effort_policy(&[read_back]), Some(effort_policy()));
    }

    #[test]
    fn the_first_recorded_effort_policy_is_the_run_authority() {
        let original = effort_policy();
        let later = ResolvedEffortPolicy {
            small: Effort::High,
            mid: Effort::High,
            frontier: Effort::High,
            review: Effort::High,
        };
        let resumed = |policy| {
            Event::now(EventBody::RunResumed {
                data: RunResumed {
                    head_sha: "abc".to_owned(),
                    interrupted_attempts: 0,
                    discarded: Vec::new(),
                    gates: None,
                    effort_policy: Some(policy),
                    reviews: None,
                    chains: None,
                    normalized_plan_digest: None,
                },
            })
        };

        let EventBody::RunStarted { data } = started() else {
            panic!("started() builds a run_started");
        };
        let current = vec![Event::now(EventBody::RunStarted { data }), resumed(later)];
        assert_eq!(recorded_effort_policy(&current), Some(original));

        let EventBody::RunStarted { mut data } = started() else {
            panic!("started() builds a run_started");
        };
        data.effort_policy = None;
        let legacy = vec![
            Event::now(EventBody::RunStarted { data }),
            resumed(original),
            resumed(later),
        ];
        assert_eq!(
            recorded_effort_policy(&legacy),
            Some(original),
            "the first establishing resume wins"
        );
    }

    #[test]
    fn the_first_complete_binding_snapshot_is_the_run_authority() {
        let EventBody::RunStarted { data } = started() else {
            panic!("started() builds a run_started");
        };
        let original = data.chains.clone();
        let current = vec![Event::now(EventBody::RunStarted { data })];
        assert_eq!(recorded_chains(&current), Some(&original));

        let EventBody::RunStarted { mut data } = started() else {
            panic!("started() builds a run_started");
        };
        for chain in &mut data.chains {
            chain.bindings = None;
        }
        let later = original
            .iter()
            .cloned()
            .map(|mut chain| {
                for binding in chain.bindings.iter_mut().flatten() {
                    binding.model = "later-model-must-not-win".to_owned();
                }
                chain
            })
            .collect();
        let legacy = vec![
            Event::now(EventBody::RunStarted { data }),
            Event::now(EventBody::RunResumed {
                data: RunResumed {
                    head_sha: "abc".to_owned(),
                    interrupted_attempts: 0,
                    discarded: Vec::new(),
                    gates: None,
                    effort_policy: None,
                    reviews: None,
                    chains: Some(original.clone()),
                    normalized_plan_digest: None,
                },
            }),
            Event::now(EventBody::RunResumed {
                data: RunResumed {
                    head_sha: "def".to_owned(),
                    interrupted_attempts: 0,
                    discarded: Vec::new(),
                    gates: None,
                    effort_policy: None,
                    reviews: None,
                    chains: Some(later),
                    normalized_plan_digest: None,
                },
            }),
        ];
        assert_eq!(recorded_chains(&legacy), Some(&original));
    }

    #[test]
    fn a_recorded_gate_survives_the_wire_intact_enough_to_run_again() {
        let EventBody::RunStarted { data } = started() else {
            panic!("started() builds a run_started");
        };
        let recorded_gates = data.gate_cmds.clone();
        let line =
            serde_json::to_string(&Event::now(EventBody::RunStarted { data })).expect("serialize");
        let json: serde_json::Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(json["data"]["gate_cmds"][0]["timeout_ms"], 600_000);
        assert_eq!(json["data"]["gate_cmds"][0]["shell"], "sh");

        let event: Event = serde_json::from_str(&line).expect("round trip");
        let EventBody::RunStarted { data: read_back } = event.body else {
            panic!("still a run_started");
        };
        assert_eq!(read_back.gate_cmds, recorded_gates);
        let recorded = read_back.gate_cmds.expect("gates");
        assert_eq!(
            crate::gates::ShellKind::parse("sh"),
            Some(recorded[0].shell)
        );
    }

    #[test]
    fn selection_origins_round_trip_and_old_starts_stay_absent() {
        for origin in [
            SelectionOrigin::Auto,
            SelectionOrigin::Pin,
            SelectionOrigin::UserOverride,
            SelectionOrigin::Exploration,
        ] {
            let json = serde_json::to_string(&origin).expect("serialize origin");
            assert_eq!(
                serde_json::from_str::<SelectionOrigin>(&json).expect("read origin"),
                origin
            );
        }

        let mut json = serde_json::to_value(Event::now(attempt_started("t1", 1, 0, "small")))
            .expect("serialize start");
        let data = json["data"].as_object_mut().expect("start data");
        data.remove("adapter");
        data.remove("preflight_cli_version");
        data.remove("effort");
        data.remove("selection_origin");
        let event: Event = serde_json::from_value(json).expect("old start still parses");
        let EventBody::AttemptStarted { data, .. } = event.body else {
            panic!("still an attempt start");
        };
        assert_eq!(data.adapter, None);
        assert_eq!(data.preflight_cli_version, None);
        assert_eq!(data.effort, None);
        assert_eq!(data.selection_origin, None);
        assert!(
            serde_json::from_str::<SelectionOrigin>("\"unknown\"").is_err(),
            "unknown is an export-only sentinel"
        );

        let review = ReviewRecord {
            pass: "review".to_owned(),
            agent: "claude-code".to_owned(),
            model: "claude-opus-5".to_owned(),
            adapter: Some("claude-code".to_owned()),
            preflight_cli_version: Some("1.2.3".to_owned()),
            effort: Some(Effort::High),
            pool: Some("claude-max".to_owned()),
            cost_usd: Some(0.05),
            outcome: ReviewPassOutcome::Passed,
        };
        let mut json = serde_json::to_value(review).expect("serialize review");
        let data = json.as_object_mut().expect("review data");
        data.remove("adapter");
        data.remove("preflight_cli_version");
        data.remove("effort");
        let review: ReviewRecord = serde_json::from_value(json).expect("old review still parses");
        assert_eq!(review.adapter, None);
        assert_eq!(review.preflight_cli_version, None);
        assert_eq!(review.effort, None);
        assert_eq!(
            SCHEMA_VERSION, 3,
            "the complete-review contract must remain behind a schema boundary"
        );
    }

    #[test]
    fn a_log_without_a_beginning_cannot_be_verified() {
        let events = vec![Event::now(attempt_started("t1", 1, 0, "small"))];
        let err = replay(events, vec!["t1".to_owned()], Path::new("events.jsonl"))
            .expect_err("no run_started");
        assert!(err.to_string().contains("run_started"), "got: {err}");
    }

    #[test]
    fn an_interrupted_attempt_is_recorded_but_does_not_spend_the_rung() {
        let events = vec![
            Event::now(started()),
            Event::now(attempt_started("t1", 1, 0, "small")),
        ];
        let mut replayed =
            replay(events, vec!["t1".to_owned()], Path::new("events.jsonl")).expect("replay");
        assert_eq!(replayed.state.settle_interrupted(), 1);

        let progress = &replayed.state.progress[0];
        assert_eq!(progress.attempts, 1, "the attempt happened");
        assert_eq!(
            progress.attempts_on_rung, 0,
            "but nothing judged it, so the rung's allowance is intact"
        );
        assert_eq!(progress.rung, 0, "and it did not escalate");
        assert!(
            progress.session.is_none(),
            "§14: the session is not trusted"
        );
        assert!(!progress.resume_next);
        assert!(progress.in_flight.is_none(), "settled");

        let record = progress.records.last().expect("a ledger line");
        assert_eq!(
            record.failure.as_ref().map(|f| f.kind),
            Some(FailureKind::Interrupted)
        );
        assert_eq!(record.cost_usd, None, "unknown spend stays unknown");
        assert_eq!(
            replayed.state.states[0],
            TaskState::Pending,
            "the scheduler picks it straight back up"
        );
    }

    #[test]
    fn a_finished_attempt_leaves_nothing_in_flight() {
        let events = vec![
            Event::now(started()),
            Event::now(attempt_started("t1", 1, 0, "small")),
            Event::now(attempt_finished("t1", 1, 0, "small")),
        ];
        let mut replayed =
            replay(events, vec!["t1".to_owned()], Path::new("events.jsonl")).expect("replay");
        assert_eq!(replayed.state.settle_interrupted(), 0);
        assert_eq!(replayed.state.progress[0].records.len(), 1);
        assert_eq!(replayed.state.progress[0].attempts_on_rung, 1);
        assert_eq!(
            replayed.state.progress[0].session.as_deref(),
            Some("s0"),
            "a live session survives within one process"
        );
    }

    #[test]
    fn resume_repairs_each_attempt_settlement_transition_prefix() {
        let cases = [
            AttemptTransition::Retry(LadderRetry {
                resume: true,
                tier: "small".to_owned(),
                summary: "retry".to_owned(),
                detail: Some("fix it".to_owned()),
            }),
            AttemptTransition::Escalate(LadderEscalated {
                to_rung: 1,
                tier: "small".to_owned(),
                summary: "escalate".to_owned(),
                detail: None,
            }),
            AttemptTransition::Defer(TaskDeferred {
                reason: "outage".to_owned(),
                defers: 1,
            }),
            AttemptTransition::Fail(TaskFailed {
                kind: FailureKind::NoChain,
                reason: "no chain".to_owned(),
                halts_run: true,
            }),
        ];

        for transition in cases {
            let mut finished = attempt_finished("t1", 1, 0, "small");
            let EventBody::AttemptFinished {
                data,
                transition: recorded,
                prepared_commit,
                ..
            } = &mut finished
            else {
                unreachable!();
            };
            data.failure = Some(FailureRecord {
                kind: match &transition {
                    AttemptTransition::Defer(_) => FailureKind::RateLimited,
                    AttemptTransition::Fail(data) => data.kind,
                    _ => FailureKind::GateFailed,
                },
                origin: FailureOrigin::Worker,
                reason: "settled".to_owned(),
                detail: None,
            });
            *prepared_commit = None;
            *recorded = Some(Box::new(transition.clone()));
            let replayed = replay(
                vec![
                    Event::now(started()),
                    Event::now(attempt_started("t1", 1, 0, "small")),
                    Event::now(finished),
                ],
                vec!["t1".to_owned()],
                Path::new("events.jsonl"),
            )
            .expect("the settlement prefix is complete on its own");
            let progress = &replayed.state.progress[0];
            match transition {
                AttemptTransition::Retry(_) => {
                    assert_eq!(replayed.state.states[0], TaskState::Pending);
                    assert!(progress.resume_next);
                    assert_eq!(progress.feedback.len(), 1);
                }
                AttemptTransition::Escalate(_) => {
                    assert_eq!(progress.rung, 1);
                    assert_eq!(progress.attempts_on_rung, 0);
                    assert!(progress.session.is_none());
                }
                AttemptTransition::Defer(_) => {
                    assert_eq!(replayed.state.states[0], TaskState::Deferred);
                    assert_eq!(progress.attempts_on_rung, 0);
                    assert_eq!(progress.defers, 1);
                }
                AttemptTransition::Fail(_) => {
                    assert!(matches!(replayed.state.states[0], TaskState::Failed { .. }));
                    assert_eq!(replayed.state.halted_at.as_deref(), Some("t1"));
                }
            }
        }
    }

    #[test]
    fn defer_then_sessionless_fresh_attempt_never_resumes_stale_session() {
        let mut first = attempt_finished("t1", 1, 0, "small");
        let EventBody::AttemptFinished { data, .. } = &mut first else {
            unreachable!();
        };
        data.session_id = Some("stale-session".to_owned());

        let mut second = attempt_finished("t1", 2, 0, "small");
        let EventBody::AttemptFinished {
            data,
            transition,
            prepared_commit,
            ..
        } = &mut second
        else {
            unreachable!();
        };
        data.session_id = None;
        data.failure = Some(FailureRecord {
            kind: FailureKind::GateFailed,
            origin: FailureOrigin::Worker,
            reason: "failed without a session".to_owned(),
            detail: None,
        });
        *prepared_commit = None;
        *transition = Some(Box::new(AttemptTransition::Retry(LadderRetry {
            resume: false,
            tier: "small".to_owned(),
            summary: "retry fresh".to_owned(),
            detail: None,
        })));

        let replayed = replay(
            vec![
                Event::now(legacy_started()),
                Event::now(attempt_started("t1", 1, 0, "small")),
                Event::now(first),
                Event::now(EventBody::TaskDeferred {
                    task: "t1".to_owned(),
                    data: TaskDeferred {
                        reason: "review outage".to_owned(),
                        defers: 1,
                    },
                }),
                Event::now(attempt_started("t1", 2, 0, "small")),
                Event::now(second),
            ],
            vec!["t1".to_owned()],
            Path::new("events.jsonl"),
        )
        .expect("replay");
        assert!(replayed.state.progress[0].session.is_none());
        assert!(!replayed.state.progress[0].resume_next);
    }

    #[test]
    fn atomic_attempt_parking_discards_the_finished_sessions_tree_identity() {
        let mut finished = attempt_finished("t1", 1, 0, "small");
        let EventBody::AttemptFinished {
            data,
            parking,
            prepared_commit,
            ..
        } = &mut finished
        else {
            unreachable!("the helper always returns an attempt settlement");
        };
        data.failure = Some(FailureRecord {
            kind: FailureKind::ReviewInputTooLarge,
            origin: FailureOrigin::Reviewer,
            reason: "too large".to_owned(),
            detail: None,
        });
        *prepared_commit = None;
        *parking = Some(Box::new(AttemptParking {
            question: question("q-parked", "t1"),
            refund_attempt: false,
        }));
        let events = vec![
            Event::now(started()),
            Event::now(attempt_started("t1", 1, 0, "small")),
            Event::now(finished),
        ];
        let replayed =
            replay(events, vec!["t1".to_owned()], Path::new("events.jsonl")).expect("replay");

        let progress = &replayed.state.progress[0];
        assert!(
            progress.session.is_none(),
            "parking discarded the tree, so its session cannot be resumed"
        );
        assert!(!progress.resume_next);
        assert_eq!(
            replayed.state.states[0],
            TaskState::AwaitingInput(QuestionId::from("q-parked"))
        );
        assert_eq!(replayed.state.open_questions().len(), 1);
    }

    #[test]
    fn resuming_drops_the_session_and_wakes_deferred_work() {
        let events = vec![
            Event::now(legacy_started()),
            Event::now(attempt_started("t1", 1, 0, "small")),
            Event::now(attempt_finished("t1", 1, 0, "small")),
            Event::now(EventBody::TaskDeferred {
                task: "t1".to_owned(),
                data: TaskDeferred {
                    reason: "rate limited".to_owned(),
                    defers: 1,
                },
            }),
            Event::now(EventBody::RunResumed {
                data: RunResumed {
                    head_sha: "abc".to_owned(),
                    interrupted_attempts: 0,
                    discarded: Vec::new(),
                    gates: None,
                    effort_policy: None,
                    reviews: None,
                    chains: None,
                    normalized_plan_digest: None,
                },
            }),
        ];
        let replayed =
            replay(events, vec!["t1".to_owned()], Path::new("events.jsonl")).expect("replay");
        assert!(replayed.state.progress[0].session.is_none());
        assert!(!replayed.state.progress[0].resume_next);
        assert_eq!(
            replayed.state.states[0],
            TaskState::Pending,
            "the wait already happened; do not wait again"
        );
        assert_eq!(replayed.resumes, 1);
    }

    #[test]
    fn answering_unparks_the_task_and_carries_the_operators_words() {
        let events = vec![
            Event::now(legacy_started()),
            Event::now(attempt_started("t1", 1, 0, "small")),
            Event::now(attempt_finished("t1", 1, 0, "small")),
            Event::now(EventBody::QuestionRaised {
                task: "t1".to_owned(),
                data: Box::new(QuestionRaised {
                    question: question("q-1", "t1"),
                }),
            }),
            Event::now(EventBody::TaskParked {
                task: "t1".to_owned(),
                data: TaskParked {
                    question: "q-1".to_owned(),
                    refund_attempt: false,
                },
            }),
            Event::now(EventBody::QuestionAnswered {
                data: QuestionAnswered {
                    question: QuestionId::from("q-1"),
                    answer: Answer::Answered {
                        text: "write it in src/widget.rs".to_owned(),
                    },
                    decline_halts_run: None,
                    via: "answer-file".to_owned(),
                },
            }),
        ];
        let replayed =
            replay(events, vec!["t1".to_owned()], Path::new("events.jsonl")).expect("replay");
        assert_eq!(replayed.state.states[0], TaskState::Pending, "un-parked");

        let progress = &replayed.state.progress[0];
        assert_eq!(
            progress.attempts_on_rung, 0,
            "an Unblock answer buys a fresh allowance on the same rung"
        );
        assert!(
            progress.session.is_none(),
            "a parked tree has no live session"
        );
        assert!(!progress.resume_next, "never resume out of a park (§14)");
        let last = progress.feedback.last().expect("the answer is feedback");
        assert!(last.human, "labelled as an instruction, not quoted data");
        assert_eq!(last.detail.as_deref(), Some("write it in src/widget.rs"));
        assert!(replayed.state.open_questions().is_empty());
    }

    #[test]
    fn an_answer_that_arrives_twice_is_applied_once() {
        let mut state = RunState::new(vec!["t1".to_owned()]);
        state.apply(&Event::now(EventBody::QuestionRaised {
            task: "t1".to_owned(),
            data: Box::new(QuestionRaised {
                question: question("q-1", "t1"),
            }),
        }));
        state.apply(&Event::now(EventBody::TaskParked {
            task: "t1".to_owned(),
            data: TaskParked {
                question: "q-1".to_owned(),
                refund_attempt: false,
            },
        }));
        let answered = Event::now(EventBody::QuestionAnswered {
            data: QuestionAnswered {
                question: QuestionId::from("q-1"),
                answer: Answer::Answered {
                    text: "once".to_owned(),
                },
                decline_halts_run: None,
                via: "terminal".to_owned(),
            },
        });
        state.apply(&answered);
        state.apply(&answered);
        assert_eq!(state.progress[0].feedback.len(), 1);
    }

    #[test]
    fn a_decline_leaves_the_task_to_the_failure_event() {
        let mut state = RunState::new(vec!["t1".to_owned()]);
        state.apply(&Event::now(EventBody::QuestionRaised {
            task: "t1".to_owned(),
            data: Box::new(QuestionRaised {
                question: question("q-1", "t1"),
            }),
        }));
        state.apply(&Event::now(EventBody::TaskParked {
            task: "t1".to_owned(),
            data: TaskParked {
                question: "q-1".to_owned(),
                refund_attempt: false,
            },
        }));
        state.apply(&Event::now(EventBody::QuestionAnswered {
            data: QuestionAnswered {
                question: QuestionId::from("q-1"),
                answer: Answer::Declined,
                decline_halts_run: Some(true),
                via: "terminal".to_owned(),
            },
        }));
        assert!(state.questions[0].answer.is_some(), "recorded");
        assert!(
            matches!(state.states[0], TaskState::AwaitingInput(_)),
            "still parked until task_failed says otherwise"
        );

        state.apply(&Event::now(EventBody::TaskFailed {
            task: "t1".to_owned(),
            data: TaskFailed {
                kind: FailureKind::Declined,
                reason: "declined at the human rung".to_owned(),
                halts_run: true,
            },
        }));
        assert!(matches!(state.states[0], TaskState::Failed { .. }));
        assert_eq!(state.halted_at.as_deref(), Some("t1"));
    }

    #[test]
    fn the_first_failure_keeps_the_halt_label() {
        let mut state = RunState::new(vec!["t1".to_owned(), "t2".to_owned()]);
        for task in ["t1", "t2"] {
            state.apply(&Event::now(EventBody::TaskFailed {
                task: task.to_owned(),
                data: TaskFailed {
                    kind: FailureKind::GateFailed,
                    reason: "no".to_owned(),
                    halts_run: true,
                },
            }));
        }

        assert_eq!(
            state.halted_at.as_deref(),
            Some("t1"),
            "a later failure must not relabel the cause"
        );
    }

    #[test]
    fn escalation_moves_to_the_recorded_rung_and_starts_cold() {
        let mut state = RunState::new(vec!["t1".to_owned()]);
        state.apply(&Event::now(attempt_started("t1", 1, 0, "small")));
        state.apply(&Event::now(attempt_finished("t1", 1, 0, "small")));
        state.apply(&Event::now(EventBody::LadderEscalated {
            task: "t1".to_owned(),
            attempt: 1,
            rung: 0,
            data: LadderEscalated {
                to_rung: 1,
                tier: "small".to_owned(),
                summary: "empty diff".to_owned(),
                detail: None,
            },
        }));
        let progress = &state.progress[0];
        assert_eq!(progress.rung, 1);
        assert_eq!(progress.attempts_on_rung, 0);
        assert!(progress.session.is_none(), "a new rung is a new session");
        assert!(!progress.resume_next);
        assert_eq!(progress.feedback.len(), 1, "the history travels with it");
    }

    #[test]
    fn a_tail_never_yields_half_an_event() {
        let dir = scratch("tail");
        let path = dir.join("events.jsonl");
        let mut warnings = Vec::new();
        let mut log = EventLog::open(EventSite::LegacyOpenLog, &path, &mut warnings).expect("open");
        log.append(EventSite::LegacyAppend, started())
            .expect("append");

        let mut tail = LogTail::new(path.clone());
        assert_eq!(tail.poll(&mut warnings).expect("poll").len(), 1);
        assert!(tail.poll(&mut warnings).expect("poll").is_empty());

        log.append(
            EventSite::LegacyAppend,
            attempt_started("t1", 1, 0, "small"),
        )
        .expect("append");
        let seen = tail.poll(&mut warnings).expect("poll");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body.kind(), "attempt_started");

        let mut file = OpenOptions::new().append(true).open(&path).expect("open");
        file.write_all(b"{\"ts\":\"2026").expect("partial write");
        assert!(tail.poll(&mut warnings).expect("poll").is_empty());
        assert!(warnings.is_empty(), "not an error, just not finished yet");
    }
}
