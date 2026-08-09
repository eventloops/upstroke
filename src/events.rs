//! The event log and the state it folds to (DESIGN.md §15, invariant 4).
//!
//! `events.jsonl` is the run's source of truth: every state transition is an
//! event, and state is what you get by replaying them. `status`, the ledger,
//! and `resume` are all folds over this file; `report.json` is a projection of
//! the same fold, written for humans and never read back as state.
//!
//! The load-bearing decision here is that **there is one fold, not two**.
//! [`RunState::apply`] is the only thing that mutates run state, and the live
//! engine reaches it the same way replay does — by emitting an event and
//! applying it. A live run and a replay of its own log cannot drift, because
//! neither has a private path to the state. Any bug is a bug in both, which is
//! a property a test can actually pin (see `live_state_equals_replayed_state`
//! in `engine.rs`).
//!
//! Two things deliberately do *not* survive replay, both for the same reason:
//! a session id and a `resume_next` flag describe a conversation that believed
//! it had left edits in the working tree. After a crash that tree is rolled
//! back, so the belief is false and §14's pairing of session-resume with
//! tree-retention is broken. `run_resumed` clears both.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::TactusError;
use crate::interaction::QuestionRecord;
use crate::ir::{Answer, Question, QuestionId, Tier};
use crate::ladder::{FailureKind, FailureOrigin};
use crate::util;

/// Bumped when an event's meaning changes in a way an older binary would
/// misread. A newer log is refused rather than folded on a guess — silently
/// deriving the wrong state from a log we half-understand is the one failure
/// mode an event-sourced design must not have.
pub const SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// One line of `events.jsonl`, in §15's shape:
/// `{ts, event, task?, attempt?, rung?, profile?, data}`.
///
/// `ts`, and the routing fields hoisted out of each variant, are what make the
/// raw file greppable — `rung` and `profile` in particular answer "what ran
/// where" without a JSON parser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub ts: String,
    #[serde(flatten)]
    pub body: EventBody,
}

impl Event {
    /// Stamp a body with the current time.
    pub fn now(body: EventBody) -> Self {
        Self {
            ts: util::rfc3339_utc_now(),
            body,
        }
    }

    /// The task this event concerns, if any.
    pub fn task(&self) -> Option<&str> {
        self.body.task()
    }
}

/// Every transition the engine records.
///
/// Internally tagged on `event`, with the routing fields alongside the tag and
/// the payload under `data` — one Rust type per event kind, so a variant and
/// its payload cannot disagree.
///
/// Two things are deliberately *not* events. **Blocked and skipped settlement**
/// is derived in `finish()` rather than recorded, because it is a view of an
/// ended run: a task blocked behind an unanswered question must become runnable
/// again the moment that question is answered, which a recorded state would
/// fight. And **an unreachable answer channel** is process-local — a question
/// nobody could answer at 2am is exactly the one the operator answers when they
/// come back, so `resume` must be free to ask again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventBody {
    RunStarted {
        data: Box<RunStarted>,
    },
    RunResumed {
        data: RunResumed,
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
    },
    /// The `attempt_finished` a dead process never got to write.
    ///
    /// Recorded by the resume that finds the attempt dangling, rather than
    /// merely derived in memory: a settlement that lives only in a reader's
    /// head is lost the moment the log is replayed by someone else, taking the
    /// ledger line *and* the rung's refunded allowance with it.
    AttemptInterrupted {
        task: String,
        attempt: u32,
        rung: u32,
        profile: String,
        data: Box<AttemptRecord>,
    },
    /// §11.4: feed the failure back and try the same rung again.
    LadderRetry {
        task: String,
        attempt: u32,
        rung: u32,
        data: LadderRetry,
    },
    /// §11.4: next rung, fresh session, accumulated feedback.
    LadderEscalated {
        task: String,
        attempt: u32,
        rung: u32,
        data: LadderEscalated,
    },
    /// §19: an outage, so the attempt is given back rather than spent.
    TaskDeferred {
        task: String,
        data: TaskDeferred,
    },
    /// The scheduler waited out a deferral and made that work runnable again.
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
    /// §5: every question that reaches a human at runtime is a design-phase
    /// defect, logged as one so the designer prompt can learn from it.
    DesignDefect {
        data: DesignDefect,
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
            | Self::QuestionRaised { task, .. } => Some(task),
            Self::RunStarted { .. }
            | Self::RunResumed { .. }
            | Self::DeferWaitElapsed { .. }
            | Self::QuestionAnswered { .. }
            | Self::DesignDefect { .. }
            | Self::RunFinished { .. } => None,
        }
    }

    /// The `event` tag as it appears in the log — for status rendering.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::RunResumed { .. } => "run_resumed",
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
            Self::RunFinished { .. } => "run_finished",
        }
    }
}

// ---------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------

/// Everything `resume` needs to decide whether continuing is still safe, plus
/// enough context that the log explains itself without the repo beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStarted {
    pub schema: u32,
    pub tactus_version: String,
    pub run_id: String,
    pub branch: String,
    /// Full sha of the commit the run branched from — the expected HEAD until
    /// the first task commits.
    pub base_sha: String,
    /// Plan path as given, relative to the repo root where possible so the
    /// record survives the repo moving.
    pub plan_path: String,
    pub config_path: Option<String>,
    /// Content hash of the plan text (`ir::content_hash`). A run is bound to
    /// the plan it froze; a different hash means the task graph moved under it.
    pub plan_hash: String,
    /// Where the agent-authored half of this run lives (§15 split).
    pub private_dir: String,
    pub gates: Vec<String>,
    pub gates_from_config: bool,
    pub interaction_mode: String,
    /// The resolved chain per task, in plan order. Recorded so resume can tell
    /// that config moved: `Progress.rung` is an index into this chain, and
    /// re-resolving a different one would silently point it at another tier.
    pub chains: Vec<ChainSummary>,
    /// Who judges this run's code (§11.2–§11.3), resolved at pre-flight.
    ///
    /// Recorded because it is a fact about the run, not about today's machine.
    /// The cross-family reviewer is chosen from what has an adapter *and*
    /// probes, so a Copilot CLI installed or removed between a run and its
    /// resume would otherwise change the verification standard halfway through
    /// — the same reasoning that made resume honour the recorded `private_dir`.
    #[serde(default)]
    pub reviews: crate::review::ReviewPlan,
}

/// One task's resolved escalation chain, as it stood when the run started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainSummary {
    pub task: String,
    pub tiers: Vec<Tier>,
    pub attempts_per: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunResumed {
    /// HEAD at the moment the run was picked up — the sha the continued work
    /// builds on.
    pub head_sha: String,
    /// Attempts that were in flight when the previous process died.
    pub interrupted_attempts: u32,
    /// Uncommitted paths this resume threw away: a dead agent's half-written
    /// edits (§14). Recorded rather than only warned about, so someone reading
    /// the run tomorrow can still see that work was discarded and what it was.
    #[serde(default)]
    pub discarded: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptStarted {
    pub tier: String,
    pub agent: String,
    pub model: String,
    /// The session this attempt resumed, if any (§11.4).
    pub resume_session: Option<String>,
}

/// One attempt's ledger line: which rung it ran on, what it cost, and what
/// went wrong. Shared by the log and `report.json` so the ledger has exactly
/// one shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub attempt: u32,
    pub tier: String,
    pub model: String,
    /// Whether this attempt resumed the previous one's session (§11.4).
    pub resumed: bool,
    #[serde(rename = "duration_ms", with = "crate::util::duration_millis")]
    pub duration: Duration,
    pub cost_usd: Option<f64>,
    /// The review passes that actually ran, in order (§11.3). Empty when the
    /// gates failed first and nothing was reviewed.
    ///
    /// A list rather than the single `review_model`/`review_cost_usd` pair it
    /// replaces: §11.5 generalizes review into a list of passes, and a
    /// second-opinion verdict has to be attributable to the model that gave it.
    /// Logs written before step 9 read back with this empty — their review
    /// spend does not replay, which is the price of the shape being right.
    #[serde(default)]
    pub reviews: Vec<ReviewRecord>,
    pub session_id: Option<String>,
    /// `None` when the attempt passed.
    pub failure: Option<FailureRecord>,
}

impl AttemptRecord {
    /// Total review spend for this attempt, or `None` when nothing reported any
    /// — which is not the same as nothing costing anything (§13: the Copilot
    /// route reports no spend at all).
    pub fn review_cost_usd(&self) -> Option<f64> {
        let reported: Vec<f64> = self.reviews.iter().filter_map(|r| r.cost_usd).collect();
        (!reported.is_empty()).then(|| reported.iter().sum())
    }

    /// The models that judged this attempt, in pass order.
    pub fn review_models(&self) -> Vec<String> {
        self.reviews.iter().map(|r| r.model.clone()).collect()
    }
}

/// One review pass's ledger line (§11.2–§11.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRecord {
    /// The lens that ran — `review` or `second-opinion`.
    pub pass: String,
    pub agent: String,
    pub model: String,
    /// `None` where the agent's route reports no spend.
    pub cost_usd: Option<f64>,
    /// Whether this pass approved. A later pass only exists because every
    /// earlier one passed, so at most the last entry is ever `false`.
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureRecord {
    pub kind: FailureKind,
    pub origin: FailureOrigin,
    pub reason: String,
}

/// What the next attempt is told. Carried on the ladder events rather than on
/// the attempt record because this is the full text — a gate log tail runs to
/// kilobytes, and `report.json` should not grow one per attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LadderRetry {
    /// §14: a resumed retry keeps the working tree, so the *cumulative* diff
    /// is what gets re-gated.
    pub resume: bool,
    pub tier: String,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LadderEscalated {
    /// The rung index being moved to. Recorded rather than derived as "+1" so
    /// replay lands where the run actually went.
    pub to_rung: u32,
    pub tier: String,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDeferred {
    pub reason: String,
    /// Deferrals this task has taken, after this one.
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
    /// Whether the rung's allowance is given back. A worker or reviewer that
    /// stopped to ask never had its code judged (§12), so it costs nothing.
    pub refund_attempt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCommitted {
    /// Full sha. `resume` compares this against HEAD, and `--short` length
    /// varies with `core.abbrev`.
    pub sha: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFailed {
    pub kind: FailureKind,
    pub reason: String,
    /// Whether this failure halts the run (`[engine] on_task_failure`).
    /// Recorded rather than re-derived so a config edit between a run and its
    /// resume cannot rewrite which task the report blames.
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
    /// Which channel produced it — a terminal, an out-of-band `tactus answer`,
    /// or a resume picking up an answer written while the run was dead.
    pub via: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignDefect {
    pub question: QuestionId,
    /// The decision execution had to stop for — review material for the
    /// designer prompt (§5).
    pub context: String,
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Complete,
    Parked,
    Halted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFinished {
    pub outcome: RunOutcome,
    pub halted_at: Option<String>,
    pub committed: u32,
    pub parked: u32,
}

// ---------------------------------------------------------------------------
// Derived state
// ---------------------------------------------------------------------------

/// Scheduler state for one task. Readiness is derived (deps all `Done`), not
/// stored, so it can never drift from the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    /// Runnable once its dependencies are done — the state a task returns to
    /// after an answer un-parks it.
    Pending,
    /// A pool was busy. No attempt was spent; try again after a wait (§19).
    Deferred,
    /// Parked on a question (§12). Exactly this task, never its neighbours.
    AwaitingInput(QuestionId),
    Done(String),
    Failed {
        kind: FailureKind,
        reason: String,
    },
    /// Settlement only: derived when a run ends, never applied from an event,
    /// because an answered question has to make these runnable again.
    Blocked(String),
    /// Settlement only: the run stopped before this task got its turn.
    Skipped,
}

/// An attempt that started and never reported back — the shape a killed
/// process leaves in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub attempt: u32,
    pub rung: u32,
    pub tier: String,
    pub model: String,
    pub profile: String,
}

/// A dangling attempt, with the task it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedAttempt {
    pub task: String,
    pub flight: InFlight,
}

impl InterruptedAttempt {
    /// The event that stands in for the `attempt_finished` never written.
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
                resumed: false,
                duration: Duration::ZERO,
                cost_usd: None,
                // Nothing judged the code, so nothing is attributed to a
                // reviewer.
                reviews: Vec::new(),
                session_id: None,
                failure: Some(FailureRecord {
                    kind: FailureKind::Interrupted,
                    origin: FailureOrigin::Worker,
                    reason: "the engine stopped while this attempt was running; whatever it \
                             spent is unknown and nothing judged the result"
                        .to_owned(),
                }),
            }),
        }
    }
}

/// One thing the next attempt should know. `human` matters: an operator's
/// answer is an instruction, while a gate log or a reviewer's demand is
/// tool-authored text quoted back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    pub attempt: u32,
    pub tier: String,
    pub summary: String,
    pub detail: Option<String>,
    pub human: bool,
}

/// Everything one task accumulates across its attempts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// Index into the resolved chain.
    pub rung: usize,
    /// Attempts spent on the current rung.
    pub attempts_on_rung: u32,
    /// Total attempts, which also numbers this task's run artifacts.
    pub attempts: u32,
    /// Session id from the most recent attempt, for §11.4's resume.
    pub session: Option<String>,
    /// Whether the next attempt should resume `session`.
    pub resume_next: bool,
    pub feedback: Vec<Feedback>,
    pub defers: u32,
    pub records: Vec<AttemptRecord>,
    /// Set while an attempt is running; a value that survives to the end of a
    /// replay is an attempt the engine died inside.
    pub in_flight: Option<InFlight>,
}

/// The run state every reader derives and the engine mutates — the only thing
/// [`apply`](RunState::apply) touches.
#[derive(Debug, Clone, PartialEq)]
pub struct RunState {
    /// Task ids in plan order; every other vector here is aligned to it.
    pub task_ids: Vec<String>,
    pub states: Vec<TaskState>,
    pub progress: Vec<Progress>,
    pub questions: Vec<QuestionRecord>,
    /// Task indices in the order they first ran, so a report reads as the run
    /// happened.
    pub order: Vec<usize>,
    pub halted_at: Option<String>,
    pub finished: Option<RunFinished>,
}

impl RunState {
    /// A fresh state for a plan's tasks, before any event.
    pub fn new(task_ids: Vec<String>) -> Self {
        let count = task_ids.len();
        Self {
            task_ids,
            states: vec![TaskState::Pending; count],
            progress: (0..count).map(|_| Progress::default()).collect(),
            questions: Vec::new(),
            order: Vec::new(),
            halted_at: None,
            finished: None,
        }
    }

    pub fn index_of(&self, task: &str) -> Option<usize> {
        self.task_ids.iter().position(|id| id == task)
    }

    /// Fold one event in.
    ///
    /// The engine calls this immediately after appending the event, and replay
    /// calls it for every event in the file. Unknown tasks are skipped rather
    /// than panicking: a log paired with a plan that no longer contains the
    /// task is a resume refusal, caught before this is ever reached.
    pub fn apply(&mut self, event: &Event) {
        match &event.body {
            // Metadata for the reader; contributes no task state.
            EventBody::RunStarted { .. } | EventBody::DesignDefect { .. } => {}

            // §14: a resumed run cannot trust a session that believed it left
            // edits in a tree that has since been rolled back, and deferred
            // work has by definition already waited.
            EventBody::RunResumed { .. } => {
                for progress in &mut self.progress {
                    progress.session = None;
                    progress.resume_next = false;
                }
                for state in &mut self.states {
                    if *state == TaskState::Deferred {
                        *state = TaskState::Pending;
                    }
                }
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
                });
            }

            // The attempt nobody was alive to finish. Recorded — it really ran
            // and really drained a pool, and a ledger that hides that is lying
            // — but it does not spend the rung's allowance, because nothing
            // judged the code. That is the rule §19 applies to an outage and
            // step 7 applies to a worker that stopped to ask.
            //
            // `attempts` is deliberately not rolled back: it numbers this
            // task's artifacts, and reusing the interrupted attempt's number
            // would overwrite its transcript with the retry's.
            EventBody::AttemptInterrupted { task, data, .. } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                let progress = &mut self.progress[index];
                progress.in_flight = None;
                progress.attempts_on_rung = progress.attempts_on_rung.saturating_sub(1);
                // §14: whatever session that attempt held described a working
                // tree that has since been rolled back.
                progress.session = None;
                progress.resume_next = false;
                progress.records.push((**data).clone());
            }

            EventBody::AttemptFinished { task, data, .. } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                let progress = &mut self.progress[index];
                progress.in_flight = None;
                if let Some(session) = &data.session_id {
                    progress.session = Some(session.clone());
                }
                progress.records.push((**data).clone());
            }

            EventBody::LadderRetry {
                task,
                attempt,
                data,
                ..
            } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                let progress = &mut self.progress[index];
                progress.feedback.push(Feedback {
                    attempt: *attempt,
                    tier: data.tier.clone(),
                    summary: data.summary.clone(),
                    detail: data.detail.clone(),
                    human: false,
                });
                progress.resume_next = data.resume;
            }

            EventBody::LadderEscalated {
                task,
                attempt,
                data,
                ..
            } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                let progress = &mut self.progress[index];
                progress.feedback.push(Feedback {
                    attempt: *attempt,
                    tier: data.tier.clone(),
                    summary: data.summary.clone(),
                    detail: data.detail.clone(),
                    human: false,
                });
                progress.rung = data.to_rung as usize;
                progress.attempts_on_rung = 0;
                // §11.4: a different model cannot inherit another's
                // conversation; the accumulated feedback carries the history.
                progress.session = None;
                progress.resume_next = false;
            }

            EventBody::TaskDeferred { task, data } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                let progress = &mut self.progress[index];
                // No attempt was spent on the work itself (§19).
                progress.attempts_on_rung = progress.attempts_on_rung.saturating_sub(1);
                progress.defers = data.defers;
                progress.resume_next = false;
                self.states[index] = TaskState::Deferred;
            }

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

            EventBody::TaskFailed { task, data } => {
                let Some(index) = self.index_of(task) else {
                    return;
                };
                self.states[index] = TaskState::Failed {
                    kind: data.kind,
                    reason: data.reason.clone(),
                };
                if data.halts_run {
                    // First failure wins: `halted_at` is what the report and
                    // the CLI name as the cause.
                    self.halted_at.get_or_insert_with(|| task.clone());
                }
            }

            EventBody::QuestionRaised { data, .. } => {
                self.questions
                    .push(QuestionRecord::open(data.question.clone()));
            }

            EventBody::QuestionAnswered { data } => self.answer_question(data),

            EventBody::RunFinished { data } => self.finished = Some(data.clone()),
        }
    }

    /// Record an answer and un-park what it releases.
    ///
    /// A decline changes no task state here — the caller emits `task_failed`
    /// for that, so the halt policy lives in exactly one place.
    fn answer_question(&mut self, data: &QuestionAnswered) {
        let Some(position) = self
            .questions
            .iter()
            .position(|record| record.question.id == data.question)
        else {
            return;
        };
        // An answer that arrives twice — a late file alongside a terminal
        // reply — must not push the operator's words into the prompt twice.
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
            progress.feedback.push(Feedback {
                attempt: progress.attempts,
                tier: String::new(),
                summary: "the operator answered the open question".to_owned(),
                detail: Some(text.clone()),
                human: true,
            });
            // The answer buys a fresh allowance on the rung the task is
            // standing on, and clears the deferrals a pool outage racked up.
            // It never moves the rung: if the chain exhausted, the task is
            // already at the top of it.
            if kind == crate::ir::QuestionKind::Unblock {
                progress.attempts_on_rung = 0;
            }
            progress.defers = 0;
            // Never resume out of a park, however warm the session looks:
            // parking always discards the working tree, so the session's
            // account of what it wrote no longer matches the repository (§14).
            progress.resume_next = false;
            self.states[index] = TaskState::Pending;
        }
    }

    /// Attempts this log ends mid-flight — one per process that died inside
    /// an attempt without a resume having settled it since.
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

    /// Settle dangling attempts *in memory*, for readers.
    ///
    /// `status` uses this so an interrupted run reads correctly without
    /// writing anything. A `resume` deliberately does not: it emits the same
    /// events instead, so the settlement lands in the log where the next
    /// reader will find it. Both go through [`RunState::apply`], so what a
    /// reader sees and what a resume records cannot disagree.
    pub fn settle_interrupted(&mut self) -> u32 {
        let dangling = self.interrupted_attempts();
        for interrupted in &dangling {
            self.apply(&Event::now(interrupted.event()));
        }
        u32::try_from(dangling.len()).unwrap_or(u32::MAX)
    }

    /// Open questions, oldest first.
    pub fn open_questions(&self) -> Vec<&QuestionRecord> {
        self.questions
            .iter()
            .filter(|record| record.is_open())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Log IO
// ---------------------------------------------------------------------------

/// The append-only writer. One per run, held by the engine — `tactus answer`
/// deliberately does not write here (it drops a file the engine ingests), so
/// the log has exactly one writer and interleaved lines are impossible.
#[derive(Debug)]
pub struct EventLog {
    path: PathBuf,
    file: File,
}

impl EventLog {
    /// Open for appending, discarding an incomplete trailing record first.
    ///
    /// A process killed mid-write can leave a line with no newline. Appending
    /// straight after it would splice the fragment and the next event into one
    /// unparseable line, losing both.
    ///
    /// Terminating the fragment with a newline instead is worse than it looks:
    /// it promotes a torn *tail*, which [`read_all`] recovers from, into an
    /// unparseable line in the *middle*, which [`read_all`] must treat as a
    /// rewritten log and refuse. So the fragment is truncated away. That is
    /// not rewriting history — those bytes are by construction an event that
    /// never finished being written, and no reader could ever have parsed
    /// them — and it keeps "damage anywhere but the end means corruption" a
    /// statement the reader can still trust.
    pub fn open(path: &Path, warnings: &mut Vec<String>) -> Result<Self, TactusError> {
        let io = |source| TactusError::Io {
            path: path.to_path_buf(),
            source,
        };
        // Truncate before taking the append handle, through a handle of its
        // own. On Windows an append-only handle is opened with
        // FILE_APPEND_DATA and *not* FILE_WRITE_DATA, so `set_len` on it fails
        // outright with access denied.
        match std::fs::read(path) {
            Ok(existing) if !existing.is_empty() && existing.last() != Some(&b'\n') => {
                let keep = existing
                    .iter()
                    .rposition(|byte| *byte == b'\n')
                    .map_or(0, |index| index + 1);
                OpenOptions::new()
                    .write(true)
                    .open(path)
                    .map_err(io)?
                    .set_len(keep as u64)
                    .map_err(io)?;
                warnings.push(format!(
                    "{}: discarded {} trailing byte(s) of an event that was never finished being \
                     written — the shape an interrupted run leaves behind",
                    path.display(),
                    existing.len() - keep
                ));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(io(source)),
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(io)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }

    /// Append one event and get it back **as it will be read back**.
    ///
    /// Returning the round-tripped event rather than the one just constructed
    /// is what keeps "the log is the source of truth" literally true. Anything
    /// the wire format cannot represent — a sub-millisecond duration, say —
    /// must not survive in the engine's memory either, or live state would
    /// quietly hold more than a replay could ever restore and the two would
    /// disagree in a way no amount of care at the call sites would catch.
    ///
    /// Flushed and synced before returning: §19 promises a crash or power loss
    /// is recoverable by replaying this file, which is only true if the event
    /// reached the disk before the work it describes carried on. A run emits
    /// tens of events, so the cost is noise beside a single attempt.
    pub fn append(&mut self, body: EventBody) -> Result<Event, TactusError> {
        let event = Event::now(body);
        let mut line = serde_json::to_string(&event).map_err(|e| TactusError::EventLog {
            path: self.path.clone(),
            message: format!("serializing {}: {e}", event.body.kind()),
        })?;
        let written = serde_json::from_str(&line).map_err(|e| TactusError::EventLog {
            path: self.path.clone(),
            message: format!(
                "{} does not survive its own wire format ({e}); the log could not be replayed",
                event.body.kind()
            ),
        })?;
        line.push('\n');
        let io = |source| TactusError::Io {
            path: self.path.clone(),
            source,
        };
        self.file.write_all(line.as_bytes()).map_err(io)?;
        self.file.flush().map_err(io)?;
        self.file.sync_data().map_err(io)?;
        Ok(written)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Read a whole log.
///
/// An unparseable **final** line is a torn tail — the shape a kill leaves — and
/// is dropped with a warning. An unparseable line anywhere **else** is
/// corruption: something rewrote history, and deriving state from the
/// survivors would produce a confident wrong answer. That errors.
pub fn read_all(path: &Path, warnings: &mut Vec<String>) -> Result<Vec<Event>, TactusError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(TactusError::EventLog {
                path: path.to_path_buf(),
                message: "no event log here — this run never started, or its directory was \
                          removed"
                    .to_owned(),
            });
        }
        Err(source) => {
            return Err(TactusError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    parse_lines(path, &text, warnings)
}

fn parse_lines(
    path: &Path,
    text: &str,
    warnings: &mut Vec<String>,
) -> Result<Vec<Event>, TactusError> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let mut events = Vec::with_capacity(lines.len());
    let last = lines.len().saturating_sub(1);
    for (position, line) in lines.iter().enumerate() {
        match serde_json::from_str::<Event>(line) {
            Ok(event) => events.push(event),
            Err(error) if position == last => {
                warnings.push(format!(
                    "{}: dropped an incomplete final line ({error}) — the shape an interrupted \
                     write leaves behind",
                    path.display()
                ));
            }
            Err(error) => {
                return Err(TactusError::EventLog {
                    path: path.to_path_buf(),
                    message: format!(
                        "line {} is not a valid event ({error}). This is not a torn tail — the \
                         log has been rewritten, and state derived from what is left would be \
                         confidently wrong.",
                        position + 1
                    ),
                });
            }
        }
    }
    Ok(events)
}

/// The result of folding a log: the state, plus the run metadata a reader
/// needs but that is not task state.
///
/// The state is **not** settled — attempts left mid-flight are still marked as
/// such. Settling is the caller's decision, because a reader does it in memory
/// and a resume records it (see [`RunState::settle_interrupted`]).
#[derive(Debug)]
pub struct Replay {
    pub state: RunState,
    pub started: RunStarted,
    /// How many times this run has been picked up again.
    pub resumes: u32,
    pub events: Vec<Event>,
}

/// The `run_started` a log opens with — how a run describes itself.
pub fn started_of<'a>(events: &'a [Event], path: &Path) -> Result<&'a RunStarted, TactusError> {
    events
        .iter()
        .find_map(|event| match &event.body {
            EventBody::RunStarted { data } => Some(&**data),
            _ => None,
        })
        .ok_or_else(|| TactusError::EventLog {
            path: path.to_path_buf(),
            message: "no run_started event — this log never recorded how the run began, so \
                      there is nothing to verify a resume against"
                .to_owned(),
        })
}

/// Replay a log into state.
///
/// The plan's task ids are supplied rather than read from the log: they define
/// the index space every `Progress` lives in, and the caller has already
/// checked the plan is the one this run froze.
pub fn replay(
    events: Vec<Event>,
    task_ids: Vec<String>,
    path: &Path,
) -> Result<Replay, TactusError> {
    let started = started_of(&events, path)?.clone();
    if started.schema > SCHEMA_VERSION {
        return Err(TactusError::EventLog {
            path: path.to_path_buf(),
            message: format!(
                "written by a newer tactus (event schema {}, this binary understands {}). \
                 Upgrade rather than replay it — folding a log we only half understand would \
                 derive the wrong state silently.",
                started.schema, SCHEMA_VERSION
            ),
        });
    }

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

/// Incremental reader for `status --follow`.
///
/// Reads only complete lines: a poll that catches the writer mid-line stops at
/// the last newline and picks the rest up next time, so a follower never sees
/// half an event.
#[derive(Debug)]
pub struct LogTail {
    path: PathBuf,
    offset: u64,
}

impl LogTail {
    pub fn new(path: PathBuf) -> Self {
        Self { path, offset: 0 }
    }

    /// Start from the end, so a follower attached to a live run reports only
    /// what happens from now on.
    pub fn skip_existing(&mut self) {
        self.offset = std::fs::metadata(&self.path).map_or(0, |meta| meta.len());
    }

    pub fn poll(&mut self, warnings: &mut Vec<String>) -> Result<Vec<Event>, TactusError> {
        let io = |source| TactusError::Io {
            path: self.path.clone(),
            source,
        };
        let Ok(mut file) = File::open(&self.path) else {
            return Ok(Vec::new());
        };
        let length = file.metadata().map_err(io)?.len();
        if length <= self.offset {
            // Truncated or replaced underneath us: start over rather than
            // read from an offset that now means something else.
            if length < self.offset {
                self.offset = 0;
            }
            if length == self.offset {
                return Ok(Vec::new());
            }
        }
        file.seek(SeekFrom::Start(self.offset)).map_err(io)?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer).map_err(io)?;
        let Some(end) = buffer.rfind('\n') else {
            return Ok(Vec::new());
        };
        let complete = &buffer[..=end];
        self.offset += complete.len() as u64;
        parse_lines(&self.path, complete, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{QuestionKind, TaskId};

    fn started() -> EventBody {
        EventBody::RunStarted {
            data: Box::new(RunStarted {
                schema: SCHEMA_VERSION,
                tactus_version: "0.0.1".to_owned(),
                run_id: "01RUN".to_owned(),
                branch: "tactus/run-01RUN".to_owned(),
                base_sha: "abc123".to_owned(),
                plan_path: "plan.md".to_owned(),
                config_path: None,
                plan_hash: "deadbeef".to_owned(),
                private_dir: "/home/x/.tactus/runs/01RUN".to_owned(),
                gates: vec!["check".to_owned()],
                gates_from_config: true,
                reviews: crate::review::ReviewPlan::default(),
                interaction_mode: "on_block".to_owned(),
                chains: vec![ChainSummary {
                    task: "t1".to_owned(),
                    tiers: vec![Tier::Small, Tier::Mid],
                    attempts_per: 2,
                }],
            }),
        }
    }

    fn attempt_started(task: &str, attempt: u32, rung: u32, tier: &str) -> EventBody {
        EventBody::AttemptStarted {
            task: task.to_owned(),
            attempt,
            rung,
            profile: format!("{tier}-model"),
            data: AttemptStarted {
                tier: tier.to_owned(),
                agent: "claude-code".to_owned(),
                model: "model".to_owned(),
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
            data: Box::new(AttemptRecord {
                attempt,
                tier: tier.to_owned(),
                model: "model".to_owned(),
                resumed: false,
                duration: Duration::from_millis(1500),
                cost_usd: Some(0.01),
                reviews: Vec::new(),
                session_id: Some("s0".to_owned()),
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
        let dir = std::env::temp_dir().join(format!("tactus-events-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn the_envelope_matches_the_shape_the_spec_documents() {
        // §15: {ts, event, task?, attempt?, rung?, profile?, data}. The
        // routing fields are hoisted so the raw file is greppable.
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
        // An event with no task omits the field rather than nulling it.
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
                },
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
                    message: "[tactus] t1: do it".to_owned(),
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
        // Readability in the log, and it survives serde's internally-tagged
        // buffering, which the default Duration shape does not reliably do.
        let event = Event::now(attempt_finished("t1", 1, 0, "small"));
        let line = serde_json::to_string(&event).expect("serialize");
        assert!(line.contains("\"duration_ms\":1500"), "{line}");
        assert!(!line.contains("nanos"), "{line}");
        let back: Event = serde_json::from_str(&line).expect("round-trip");
        assert_eq!(back, event);
    }

    #[test]
    fn a_torn_final_line_is_dropped_but_a_rewritten_middle_is_an_error() {
        let dir = scratch("torn");
        let path = dir.join("events.jsonl");
        let good = serde_json::to_string(&Event::now(started())).expect("serialize");
        let also_good = serde_json::to_string(&Event::now(attempt_started("t1", 1, 0, "small")))
            .expect("serialize");

        // A kill mid-write: the last line stops partway through.
        let torn = format!("{good}\n{also_good}\n{{\"ts\":\"2026-01-0");
        std::fs::write(&path, &torn).expect("write");
        let mut warnings = Vec::new();
        let events = read_all(&path, &mut warnings).expect("torn tail is recoverable");
        assert_eq!(events.len(), 2);
        assert!(
            warnings.iter().any(|w| w.contains("incomplete final line")),
            "warnings: {warnings:?}"
        );

        // Damage anywhere else means the file was rewritten, not interrupted.
        let corrupt = format!("{good}\nnot json at all\n{also_good}\n");
        std::fs::write(&path, corrupt).expect("write");
        let mut warnings = Vec::new();
        let err = read_all(&path, &mut warnings).expect_err("must not fold a rewritten log");
        assert!(err.to_string().contains("line 2"), "got: {err}");
        assert!(err.to_string().contains("confidently wrong"), "got: {err}");
    }

    #[test]
    fn appending_after_a_torn_line_discards_it_rather_than_splicing() {
        let dir = scratch("repair");
        let path = dir.join("events.jsonl");
        let good = serde_json::to_string(&Event::now(started())).expect("serialize");
        std::fs::write(&path, format!("{good}\n{{\"ts\":\"trunc")).expect("write");

        let mut warnings = Vec::new();
        let mut log = EventLog::open(&path, &mut warnings).expect("open");
        assert!(
            warnings.iter().any(|w| w.contains("never finished")),
            "the discard is reported, not silent: {warnings:?}"
        );
        log.append(attempt_started("t1", 1, 0, "small"))
            .expect("append");

        // Splicing would have lost both the fragment and the new event;
        // newline-terminating the fragment would have left an unparseable
        // line in the middle, which the reader must refuse outright.
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
        // The pathological case: killed while writing the very first event.
        let dir = scratch("alltorn");
        let path = dir.join("events.jsonl");
        std::fs::write(&path, "{\"ts\":\"2026").expect("write");

        let mut warnings = Vec::new();
        let mut log = EventLog::open(&path, &mut warnings).expect("open");
        log.append(started()).expect("append");

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
    fn a_log_without_a_beginning_cannot_be_verified() {
        let events = vec![Event::now(attempt_started("t1", 1, 0, "small"))];
        let err = replay(events, vec!["t1".to_owned()], Path::new("events.jsonl"))
            .expect_err("no run_started");
        assert!(err.to_string().contains("run_started"), "got: {err}");
    }

    #[test]
    fn an_interrupted_attempt_is_recorded_but_does_not_spend_the_rung() {
        // Decision 3, and the property a killed run depends on: the attempt
        // shows up in the ledger, the allowance does not.
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
    fn resuming_drops_the_session_and_wakes_deferred_work() {
        // §14's pairing: tree retention and session resume travel together, so
        // a resume that discards the tree must also drop the session.
        let events = vec![
            Event::now(started()),
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
            Event::now(started()),
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
        assert!(!progress.resume_next, "never resume out of a park (§14)");
        let last = progress.feedback.last().expect("the answer is feedback");
        assert!(last.human, "labelled as an instruction, not quoted data");
        assert_eq!(last.detail.as_deref(), Some("write it in src/widget.rs"));
        assert!(replayed.state.open_questions().is_empty());
    }

    #[test]
    fn an_answer_that_arrives_twice_is_applied_once() {
        // A terminal reply racing an out-of-band answer file must not push the
        // operator's words into the prompt twice.
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
                via: "terminal".to_owned(),
            },
        });
        state.apply(&answered);
        state.apply(&answered);
        assert_eq!(state.progress[0].feedback.len(), 1);
    }

    #[test]
    fn a_decline_leaves_the_task_to_the_failure_event() {
        // The halt policy lives in exactly one place: task_failed.
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
        let mut log = EventLog::open(&path, &mut warnings).expect("open");
        log.append(started()).expect("append");

        let mut tail = LogTail::new(path.clone());
        assert_eq!(tail.poll(&mut warnings).expect("poll").len(), 1);
        assert!(tail.poll(&mut warnings).expect("poll").is_empty());

        log.append(attempt_started("t1", 1, 0, "small"))
            .expect("append");
        let seen = tail.poll(&mut warnings).expect("poll");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body.kind(), "attempt_started");

        // A partial line is left for the next poll rather than parsed.
        let mut file = OpenOptions::new().append(true).open(&path).expect("open");
        file.write_all(b"{\"ts\":\"2026").expect("partial write");
        assert!(tail.poll(&mut warnings).expect("poll").is_empty());
        assert!(warnings.is_empty(), "not an error, just not finished yet");
    }
}
