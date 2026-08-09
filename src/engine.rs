//! Sequential execution engine (DESIGN.md §14) and the verification ladder it
//! drives (§11.4, §12, §19).
//!
//! Pre-flight, run branch, then a scheduler that drains the task graph one
//! attempt at a time: agent run → engine-captured diff → gates with evidence
//! axes (§11.1) → read-only review with a structured verdict (§11.2) →
//! engine-owned commit. A failed attempt does not end the task — it feeds the
//! failure back to the same rung (resuming the session where the adapter
//! supports it), then escalates a rung on a fresh session with the accumulated
//! feedback, and finally asks a human, who is the top rung.
//!
//! The scheduler's defining property is invariant 6: **a question parks only
//! the tasks it affects.** Everything else keeps draining, and the run
//! hard-blocks only when the runnable frontier is empty and everything left is
//! waiting on an answer. That is the moment — and the only moment — a human is
//! asked.
//!
//! The event log and resume arrive in step 8; until then `report.json` and
//! `questions/<id>.json` are the durable record.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::{self, AgentAdapter, Caps, TaskRun, proc};
use crate::config::OnTaskFailure;
use crate::error::TactusError;
use crate::gates::{self, ShellGate};
use crate::interaction::{
    self, AnswerSource, InteractionMode, Notifier, QuestionRecord, RealSleeper, Sleeper,
};
use crate::ir::{
    Answer, Outcome, OutcomeStatus, PermissionMode, Plan, Question, QuestionId, QuestionKind, Task,
    TaskId, TaskKind, WorkerProfile,
};
use crate::ladder::{self, LadderPolicy, LadderState, Next};
use crate::review;
use crate::ulid;
use crate::util;
use crate::validate::{self, Analysis, ValidateOptions};
use crate::workspace::Workspace;

pub use crate::ladder::{AttemptFailure, FailureKind, FailureOrigin};

/// §14: per-attempt wall clock, default 30 minutes.
pub const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How many rate limits (or reviewer outages) one task rides out before the
/// pool counts as down and a human is asked instead. §13's reset times arrive
/// with the capacity engine; until then this bound is what keeps an exhausted
/// pool from deferring forever.
pub const DEFAULT_MAX_DEFERS: u32 = 3;

/// Most recent feedback entries carried into an escalated prompt. Older
/// failures are summarized; the newest keeps its full log tail.
const MAX_FEEDBACK_ENTRIES: usize = 6;

/// §12: how a worker flags a decision it should not make alone. The prompt
/// teaches this marker; nothing else in the engine parses agent prose.
const QUESTION_MARKER: &str = "TACTUS-QUESTION:";

/// The reviewer reads a diff and answers — it has no shell and no edit tools,
/// so it does not need the implementer's budget. Giving it the full one lets a
/// single task consume several multiples of the attempt timeout.
fn review_timeout(attempt_timeout: Duration) -> Duration {
    (attempt_timeout / 4).max(Duration::from_secs(60))
}

/// Where the engine finds agent adapters. Injectable so the engine is fully
/// testable without any real agent CLI on the machine.
pub trait AdapterSource {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter>;
}

pub struct BuiltinAdapters;

impl AdapterSource for BuiltinAdapters {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        agent::by_id(id).map(|a| a as &dyn AgentAdapter)
    }
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub plan_path: PathBuf,
    pub config_path: Option<PathBuf>,
    pub pools_path: Option<PathBuf>,
    /// Repo the run executes in (agents run at its root — §14).
    pub repo_root: PathBuf,
    pub attempt_timeout: Duration,
    /// CLI override for `[interaction] mode`; `None` takes the config's.
    pub interaction: Option<InteractionMode>,
    /// First wait after a rate-limited attempt, doubling per consecutive
    /// round of nothing-but-deferred-work.
    pub defer_backoff: Duration,
    pub max_defers: u32,
}

impl RunOptions {
    /// Everything but the paths at its documented default.
    pub fn new(plan_path: PathBuf, repo_root: PathBuf) -> Self {
        Self {
            plan_path,
            config_path: None,
            pools_path: None,
            repo_root,
            attempt_timeout: DEFAULT_ATTEMPT_TIMEOUT,
            interaction: None,
            defer_backoff: interaction::DEFAULT_DEFER_BACKOFF,
            max_defers: DEFAULT_MAX_DEFERS,
        }
    }
}

/// Injectable collaborators. `None` means "use the real one", chosen from
/// config where the config has a say.
pub struct Harness<'a> {
    pub adapters: &'a dyn AdapterSource,
    /// `None` derives the channel from `[interaction] mode` (§12).
    pub answers: Option<&'a dyn AnswerSource>,
    /// `None` really sleeps.
    pub sleeper: Option<&'a dyn Sleeper>,
}

impl<'a> Harness<'a> {
    pub fn new(adapters: &'a dyn AdapterSource) -> Self {
        Self {
            adapters,
            answers: None,
            sleeper: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TaskRunStatus {
    Committed {
        sha: String,
    },
    Failed {
        kind: FailureKind,
        reason: String,
    },
    /// Waiting on a human. The rest of the run kept moving (invariant 6), and
    /// nothing about this task is lost — the question carries its context.
    Parked {
        question: String,
        reason: String,
    },
    /// A dependency failed, or parked and was never answered.
    Blocked {
        by: String,
    },
    /// Not attempted because the run halted earlier.
    Skipped,
}

/// One attempt's ledger line: which rung it ran on, what it cost, and what
/// went wrong. This is the §21 definition-of-done (e) record, and the shape
/// step 8 folds into events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub attempt: u32,
    pub tier: String,
    pub model: String,
    /// Whether this attempt resumed the previous one's session (§11.4).
    pub resumed: bool,
    pub duration: Duration,
    pub cost_usd: Option<f64>,
    pub review_model: Option<String>,
    pub review_cost_usd: Option<f64>,
    pub session_id: Option<String>,
    /// `None` when the attempt passed.
    pub failure: Option<FailureRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    pub kind: FailureKind,
    pub origin: FailureOrigin,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReport {
    pub id: String,
    pub title: String,
    /// The final attempt's implementer model. `cost_usd` is the implementer's
    /// spend across every attempt; reviewer spend is a separate field because
    /// it is a different model at a different tier, and folding them together
    /// makes cheap rungs look expensive to anyone reading the ledger (§13).
    pub model: String,
    pub status: TaskRunStatus,
    pub duration: Duration,
    pub cost_usd: Option<f64>,
    pub review_model: Option<String>,
    pub review_cost_usd: Option<f64>,
    pub session_id: Option<String>,
    /// Every attempt, oldest first — the escalation trail.
    pub attempts: Vec<AttemptRecord>,
}

impl TaskReport {
    /// Implementer plus reviewer, across every attempt.
    pub fn total_cost_usd(&self) -> Option<f64> {
        match (self.cost_usd, self.review_cost_usd) {
            (None, None) => None,
            (worker, review) => Some(worker.unwrap_or(0.0) + review.unwrap_or(0.0)),
        }
    }

    /// Compact escalation trail, e.g. `small×2 failed → mid ok`.
    pub fn trail(&self) -> String {
        let mut parts: Vec<(String, u32, bool)> = Vec::new();
        for record in &self.attempts {
            let failed = record.failure.is_some();
            match parts.last_mut() {
                Some((tier, count, last_failed)) if *tier == record.tier => {
                    *count += 1;
                    *last_failed = failed;
                }
                _ => parts.push((record.tier.clone(), 1, failed)),
            }
        }
        parts
            .into_iter()
            .map(|(tier, count, failed)| {
                let count = if count > 1 {
                    format!("×{count}")
                } else {
                    String::new()
                };
                let verdict = if failed { "failed" } else { "ok" };
                format!("{tier}{count} {verdict}")
            })
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

/// How the run ended. `Parked` is deliberately not `Halted`: §12 requires CI
/// to tell a clean completion from one that left questions unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Complete,
    Halted,
    Parked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub branch: String,
    /// Effective gate names, and whether they came from config or derivation.
    pub gates: Vec<String>,
    pub gates_from_config: bool,
    pub warnings: Vec<String>,
    pub tasks: Vec<TaskReport>,
    /// Task id the run halted at, if any.
    pub halted_at: Option<String>,
    /// Every question raised, with its answer where one arrived (§12).
    pub questions: Vec<QuestionRecord>,
    pub total_cost_usd: f64,
}

impl RunReport {
    pub fn parked_tasks(&self) -> Vec<&str> {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskRunStatus::Parked { .. }))
            .map(|t| t.id.as_str())
            .collect()
    }

    pub fn outcome(&self) -> RunOutcome {
        if self.halted_at.is_some() {
            RunOutcome::Halted
        } else if self.parked_tasks().is_empty() {
            RunOutcome::Complete
        } else {
            RunOutcome::Parked
        }
    }
}

pub fn run(opts: &RunOptions) -> Result<RunReport, TactusError> {
    run_with(opts, &BuiltinAdapters)
}

pub fn run_with(opts: &RunOptions, adapters: &dyn AdapterSource) -> Result<RunReport, TactusError> {
    run_harness(opts, &Harness::new(adapters))
}

pub fn run_harness(opts: &RunOptions, harness: &Harness<'_>) -> Result<RunReport, TactusError> {
    // Pre-flight (§14): plan parses cycle-free, config loads, chains resolve.
    let analysis = validate::analyze(&ValidateOptions {
        plan_path: opts.plan_path.clone(),
        config_path: opts.config_path.clone(),
        config_root: opts.repo_root.clone(),
        pools_path: opts.pools_path.clone(),
    })?;

    let workspace = Workspace::open(&opts.repo_root)?;
    workspace.ensure_run_exclusions()?;
    if !workspace.is_clean()? {
        return Err(TactusError::Git {
            message: "working tree is not clean; commit or stash first (the engine refuses \
                      dirty trees)"
                .to_owned(),
        });
    }

    // Probe every agent the chains reference; a missing binary is a refusal
    // to start, not a task failure (§19). The capabilities are kept, not
    // discarded: §11.4's same-rung retry resumes a session only where the
    // adapter says it can.
    let review_binding = review_binding(&analysis.config);
    let mut agent_ids: Vec<&str> = analysis
        .chains
        .iter()
        .flat_map(|c| c.rungs.iter().map(|r| r.binding.agent.as_str()))
        // The reviewer's binary is as load-bearing as any implementer's, and
        // it is frequently an agent no chain rung names.
        .chain(review_binding.iter().map(|(agent, _)| agent.as_str()))
        .collect();
    agent_ids.sort_unstable();
    agent_ids.dedup();
    let mut caps: BTreeMap<String, Caps> = BTreeMap::new();
    for id in agent_ids {
        let adapter = harness.adapters.get(id).ok_or_else(|| TactusError::Agent {
            message: format!("no adapter registered for agent `{id}`"),
        })?;
        caps.insert(id.to_owned(), adapter.probe()?);
    }

    // Effective gates come from the shared analysis (single derivation point
    // with `validate`). §14 pre-flight: the shell and every gate command must
    // resolve before any agent tokens are spent.
    let mut warnings = analysis.warnings.clone();
    if !analysis.gates.is_empty() {
        gates::shell_available(analysis.config.shell)?;
        gates::resolve_programs(&analysis.gates, &opts.repo_root, &mut warnings)?;
    }
    let gate_cmds: Vec<String> = analysis.gates.iter().map(|g| g.cmd.clone()).collect();

    let mode = opts.interaction.unwrap_or(analysis.config.interaction_mode);
    let notifiers = interaction::notifiers_for(&analysis.config.notify, &mut warnings);

    let run_id = ulid::ulid();
    let branch = format!("tactus/run-{run_id}");
    let run_dir = opts.repo_root.join(".tactus").join("runs").join(&run_id);
    for dir in [
        "transcripts",
        "settings",
        "gates",
        "artifacts",
        "reviews",
        "questions",
    ] {
        let dir = run_dir.join(dir);
        fs::create_dir_all(&dir).map_err(|source| TactusError::Io {
            path: dir.clone(),
            source,
        })?;
    }
    util::write_json(&run_dir.join("plan.normalized.json"), &analysis.plan)?;

    workspace.create_branch(&branch)?;

    let task_count = analysis.plan.tasks.len();
    let mut run = Run {
        analysis: &analysis,
        workspace: &workspace,
        run_dir,
        gate_cmds,
        adapters: harness.adapters,
        answers: harness
            .answers
            .unwrap_or_else(|| interaction::answers_for(mode)),
        notifiers,
        sleeper: harness.sleeper.unwrap_or(&RealSleeper),
        caps,
        review_binding,
        attempt_timeout: opts.attempt_timeout,
        defer_backoff: opts.defer_backoff,
        max_defers: opts.max_defers,
        on_task_failure: analysis.config.on_task_failure,
        report_path: PathBuf::new(),
        run_id,
        branch,
        warnings,
        states: vec![TaskState::Pending; task_count],
        progress: (0..task_count).map(|_| Progress::default()).collect(),
        questions: Vec::new(),
        unanswerable: Vec::new(),
        order: Vec::new(),
        halted_at: None,
    };
    run.report_path = run.run_dir.join("report.json");

    // Persist what completed before an aborting error: a mid-run failure must
    // not take the record of already-committed work and spend with it.
    // (Replaced by the event log in step 8.)
    if let Err(error) = run.drain() {
        let partial = run.finish();
        let _ = util::write_json(&run.report_path, &partial);
        return Err(error);
    }
    let report = run.finish();
    util::write_json(&run.report_path, &report)?;
    Ok(report)
}

/// Scheduler state for one task. Readiness is derived (deps all `Done`), not
/// stored, so it can never drift from the graph.
#[derive(Debug, Clone)]
enum TaskState {
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
    Blocked(TaskId),
    Skipped,
}

/// Everything one task accumulates across its attempts.
#[derive(Debug, Default)]
struct Progress {
    /// Index into the resolved chain.
    rung: usize,
    /// Attempts spent on the current rung.
    attempts_on_rung: u32,
    /// Total attempts, which also numbers this task's run artifacts.
    attempts: u32,
    /// Session id from the most recent attempt, for §11.4's resume.
    session: Option<String>,
    /// Whether the next attempt should resume `session`.
    resume_next: bool,
    feedback: Vec<Feedback>,
    defers: u32,
    records: Vec<AttemptRecord>,
}

/// One thing the next attempt should know. `human` matters: an operator's
/// answer is an instruction, while a gate log or a reviewer's demand is
/// tool-authored text quoted back.
#[derive(Debug, Clone)]
struct Feedback {
    attempt: u32,
    tier: String,
    summary: String,
    detail: Option<String>,
    human: bool,
}

struct Run<'a> {
    analysis: &'a Analysis,
    workspace: &'a Workspace,
    run_dir: PathBuf,
    gate_cmds: Vec<String>,
    adapters: &'a dyn AdapterSource,
    answers: &'a dyn AnswerSource,
    notifiers: Vec<&'static dyn Notifier>,
    sleeper: &'a dyn Sleeper,
    /// Probe results per agent id — `session_resume` gates §11.4's resume.
    caps: BTreeMap<String, Caps>,
    /// (agent, model) the reviewer binds to, resolved once at pre-flight.
    review_binding: Option<(String, String)>,
    attempt_timeout: Duration,
    defer_backoff: Duration,
    max_defers: u32,
    on_task_failure: OnTaskFailure,
    report_path: PathBuf,
    run_id: String,
    branch: String,
    warnings: Vec<String>,
    states: Vec<TaskState>,
    progress: Vec<Progress>,
    questions: Vec<QuestionRecord>,
    /// Questions no channel could reach a human for. Never asked twice — that
    /// is what stops a hard block spinning.
    unanswerable: Vec<QuestionId>,
    /// Task indices in the order they first ran, so the report reads as the
    /// run happened.
    order: Vec<usize>,
    halted_at: Option<String>,
}

impl Run<'_> {
    /// Drain the graph (§14, §12).
    ///
    /// The three branches are the whole interaction model: run what is ready;
    /// if only deferred work is left, wait for the pool rather than burning
    /// attempts against it; and only when neither is possible — the precise
    /// definition of a hard block — ask a human.
    fn drain(&mut self) -> Result<(), TactusError> {
        let mut defer_round = 0u32;
        loop {
            if let Some(index) = self.next_ready() {
                let deferred = self.step_task(index)?;
                if !deferred {
                    defer_round = 0;
                }
                util::write_json(&self.report_path, &self.snapshot())?;
                continue;
            }
            if self.states.iter().any(|s| matches!(s, TaskState::Deferred))
                && self.halted_at.is_none()
            {
                self.sleeper
                    .sleep(interaction::defer_backoff(self.defer_backoff, defer_round));
                defer_round = defer_round.saturating_add(1);
                for state in &mut self.states {
                    if matches!(state, TaskState::Deferred) {
                        *state = TaskState::Pending;
                    }
                }
                continue;
            }
            // Guarded like the other two branches: once the run has halted,
            // no answer can reach an attempt this session, so asking would
            // spend a human's attention on a decision the scheduler cannot
            // act on — and a decline would route through `fail_task` and
            // relabel `halted_at` with a task that was not the cause. The
            // questions stay open on disk for a later resume (§15).
            if self.halted_at.is_none() && self.resolve_one_question()? {
                continue;
            }
            break;
        }
        Ok(())
    }

    /// Stable order: among tasks whose dependencies are all done, lowest plan
    /// index first (§14). Parked, deferred, and blocked tasks are simply not
    /// ready — which is exactly the skip-ahead §14 asks for.
    fn next_ready(&self) -> Option<usize> {
        if self.halted_at.is_some() {
            return None;
        }
        let tasks = &self.analysis.plan.tasks;
        (0..tasks.len()).find(|&i| {
            matches!(self.states[i], TaskState::Pending)
                && tasks[i].depends_on.iter().all(|dep| {
                    tasks
                        .iter()
                        .position(|t| t.id == *dep)
                        // An unknown dependency cannot exist on a validated
                        // plan; treating it as satisfied keeps the scheduler
                        // total rather than deadlocking.
                        .is_none_or(|j| matches!(self.states[j], TaskState::Done(_)))
                })
        })
    }

    /// Drive one task until it yields the scheduler: done, failed, deferred,
    /// or parked. Retries and escalations happen *inside* — a resumed retry
    /// keeps the working tree (§14), so no other task may run in between, and
    /// this loop is what guarantees that.
    ///
    /// Returns whether the task ended deferred.
    fn step_task(&mut self, index: usize) -> Result<bool, TactusError> {
        if !self.order.contains(&index) {
            self.order.push(index);
        }
        // Copied out of `self` so they carry the run's lifetime rather than
        // this method's `&mut self` borrow.
        let analysis = self.analysis;
        let adapters = self.adapters;
        let workspace = self.workspace;
        let task = &analysis.plan.tasks[index];
        let chain = &analysis.chains[index];
        let policy = LadderPolicy {
            attempts_per: chain.attempts_per,
            rungs: chain.rungs.len(),
            max_defers: self.max_defers,
        };
        let stem = format!("{index:02}-{}", util::filename_component(task.id.as_str()));

        loop {
            let Some(rung) = chain.rungs.get(self.progress[index].rung) else {
                self.fail_task(
                    index,
                    FailureKind::NoChain,
                    "resolved chain has no rung to run on".to_owned(),
                );
                return Ok(false);
            };
            let profile = WorkerProfile {
                name: format!("{}-{}", rung.tier, rung.binding.model),
                agent: rung.binding.agent.clone(),
                model: rung.binding.model.clone(),
                pool: String::new(), // pool identity arrives with the capacity engine
                permissions: PermissionMode::Edit,
                max_turns: None,
                extra_args: Vec::new(),
            };
            let adapter = adapters
                .get(&profile.agent)
                .ok_or_else(|| TactusError::Agent {
                    message: format!("no adapter registered for agent `{}`", profile.agent),
                })?;

            let attempt = self.progress[index].attempts + 1;
            let resume = self.progress[index]
                .resume_next
                .then(|| self.progress[index].session.clone())
                .flatten();

            // Scoped so every borrow the attempt takes on `self` is released
            // before the ladder updates this task's progress below.
            let result = {
                let retry = (attempt > 1).then(|| RetryBrief {
                    resumed: resume.is_some(),
                    // Owned: the ladder appends to this task's feedback the
                    // moment the attempt returns, and one clone per attempt
                    // costs less than threading that borrow through.
                    feedback: self.progress[index].feedback.clone(),
                });
                let attempt_cx = AttemptCx {
                    task,
                    profile: profile.clone(),
                    adapter,
                    attempt,
                    stem: stem.clone(),
                    run_dir: &self.run_dir,
                    gates: &analysis.gates,
                    gate_cmds: &self.gate_cmds,
                    reviewer: self.reviewer()?,
                    timeout: self.attempt_timeout,
                    retry,
                };

                // Any error between the agent editing files and the verdict
                // leaves the tree dirty; the run cannot continue but must not
                // hand the user a half-staged workspace either (§14).
                match run_attempt(&attempt_cx, workspace, resume.clone()) {
                    Ok(result) => result,
                    Err(error) => {
                        let _ = workspace.discard_uncommitted();
                        return Err(error);
                    }
                }
            };

            let progress = &mut self.progress[index];
            progress.attempts = attempt;
            progress.attempts_on_rung += 1;
            progress.session = result
                .outcome
                .session_id
                .clone()
                .or_else(|| progress.session.clone());
            progress.records.push(AttemptRecord {
                attempt,
                tier: rung.tier.to_string(),
                model: profile.model.clone(),
                resumed: resume.is_some(),
                duration: result.outcome.duration,
                cost_usd: result.outcome.cost_usd,
                review_model: result.review_model.clone(),
                review_cost_usd: result.review_cost_usd,
                session_id: result.outcome.session_id.clone(),
                failure: result.failure.as_ref().map(|f| FailureRecord {
                    kind: f.kind,
                    origin: f.origin,
                    reason: f.reason.clone(),
                }),
            });

            let Some(failure) = result.failure else {
                let sha = self
                    .workspace
                    .commit(&format!("[tactus] {}: {}", task.id, task.title))?;
                // Scrub gate side-effects (build artifacts, lockfile churn) so
                // they cannot leak into the next task's captured diff; the
                // commit recorded exactly the verified staged set.
                self.workspace.discard_uncommitted()?;
                self.states[index] = TaskState::Done(sha);
                return Ok(false);
            };

            let resumable = self.progress[index].session.is_some()
                && self
                    .caps
                    .get(&profile.agent)
                    .is_some_and(|c| c.session_resume);
            let state = LadderState {
                rung: self.progress[index].rung,
                attempts_on_rung: self.progress[index].attempts_on_rung,
                defers: self.progress[index].defers,
                resumable,
            };
            let next = ladder::next_step(&failure, &state, &policy);

            // §14: the tree survives only for a resumed retry, where the
            // *cumulative* diff is what gets re-gated. Every other branch
            // hands the scheduler a clean workspace, because another task may
            // run before this one does again.
            if !matches!(next, Next::RetrySameRung { resume: true }) {
                self.workspace.discard_uncommitted()?;
            }

            match next {
                Next::RetrySameRung { resume } => {
                    self.record_feedback(index, rung.tier.to_string(), attempt, &failure);
                    self.progress[index].resume_next = resume;
                }
                Next::Escalate => {
                    self.record_feedback(index, rung.tier.to_string(), attempt, &failure);
                    let progress = &mut self.progress[index];
                    progress.rung += 1;
                    progress.attempts_on_rung = 0;
                    // Fresh session on a new rung (§11.4): a different model
                    // cannot inherit another's conversation, and the
                    // accumulated feedback is what carries the history over.
                    progress.session = None;
                    progress.resume_next = false;
                }
                Next::Defer => {
                    // No attempt was spent on the work itself, so give the
                    // rung its allowance back (§19).
                    let progress = &mut self.progress[index];
                    progress.attempts_on_rung = progress.attempts_on_rung.saturating_sub(1);
                    progress.defers += 1;
                    progress.resume_next = false;
                    self.states[index] = TaskState::Deferred;
                    return Ok(true);
                }
                Next::AskHuman(kind) => {
                    if kind == QuestionKind::Clarify {
                        // Nobody judged the code: the attempt is not spent.
                        let progress = &mut self.progress[index];
                        progress.attempts_on_rung = progress.attempts_on_rung.saturating_sub(1);
                    }
                    let question = self.raise_question(index, kind, &failure)?;
                    self.states[index] = TaskState::AwaitingInput(question);
                    return Ok(false);
                }
                Next::Fail => {
                    self.fail_task(index, failure.kind, failure.reason.clone());
                    return Ok(false);
                }
            }
        }
    }

    /// §11.2/§10: the reviewer is its own read-only binding, at the configured
    /// review tier (frontier by default) rather than the implementer's rung —
    /// a small model reviewing its own work is not verification. A reviewer
    /// that cannot be built must never silently degrade the run to gates-only:
    /// verification vanishing without a word is worse than a refusal.
    fn reviewer(&self) -> Result<Option<Reviewer<'_>>, TactusError> {
        let adapters = self.adapters;
        match self.review_binding.as_ref() {
            None => Ok(None),
            Some((agent, model)) => Ok(Some(Reviewer {
                adapter: adapters.get(agent).ok_or_else(|| TactusError::Agent {
                    message: format!(
                        "review tier binds to agent `{agent}`, which has no adapter in this build"
                    ),
                })?,
                profile: review::profile_for(agent, model, &format!("review-{model}")),
            })),
        }
    }

    fn record_feedback(&mut self, index: usize, tier: String, attempt: u32, f: &AttemptFailure) {
        self.progress[index].feedback.push(Feedback {
            attempt,
            tier,
            summary: f.reason.clone(),
            detail: f.feedback.clone(),
            human: false,
        });
    }

    fn fail_task(&mut self, index: usize, kind: FailureKind, reason: String) {
        let id = self.analysis.plan.tasks[index].id.clone();
        self.states[index] = TaskState::Failed { kind, reason };
        if self.on_task_failure == OnTaskFailure::Halt {
            // First failure wins. `halted_at` is what the report and the CLI
            // name as the cause, so a later failure must not relabel it.
            self.halted_at.get_or_insert_with(|| id.to_string());
        }
    }

    /// §12: raise eagerly, park exactly the affected task, tell the notifiers,
    /// and write the payload where a UI or `tactus answer` can read it.
    fn raise_question(
        &mut self,
        index: usize,
        kind: QuestionKind,
        failure: &AttemptFailure,
    ) -> Result<QuestionId, TactusError> {
        let task = &self.analysis.plan.tasks[index];
        let question = Question {
            id: interaction::new_question_id(),
            kind,
            // v0.1 parks only the task that raised it. Dependents are held by
            // the graph, not by the question, so they stay eligible the moment
            // an answer arrives.
            affected_tasks: vec![task.id.clone()],
            context: question_context(task, kind, failure, &self.progress[index]),
            options: question_options(kind),
        };
        let id = question.id.clone();
        for notifier in &self.notifiers {
            // A notifier that cannot deliver must not take the run with it:
            // the question is on disk either way (§12).
            if let Err(error) = notifier.ask(&question) {
                self.warnings.push(format!(
                    "notifier `{}` could not deliver question {id}: {error}",
                    notifier.id()
                ));
            }
        }
        let record = QuestionRecord::open(question);
        interaction::write_question(&self.run_dir.join("questions"), &record)?;
        self.questions.push(record);
        Ok(id)
    }

    /// Resolve the oldest open question. Returns whether anything changed.
    ///
    /// This runs only at a hard block, and each question is asked at most
    /// once: an `Unanswered` result marks it unreachable rather than looping
    /// back to a channel that already said nobody is there.
    fn resolve_one_question(&mut self) -> Result<bool, TactusError> {
        let Some(position) = self.questions.iter().position(|record| {
            record.is_open() && !self.unanswerable.contains(&record.question.id)
        }) else {
            return Ok(false);
        };
        let answer = self.answers.resolve(&self.questions[position].question)?;
        let id = self.questions[position].question.id.clone();
        if answer == Answer::Unanswered {
            // §12 CI mode: the task stays parked and the run's exit status
            // reports it. Not a failure — nobody rejected anything.
            self.unanswerable.push(id);
            return Ok(true);
        }

        let kind = self.questions[position].question.kind;
        let affected = self.questions[position].question.affected_tasks.clone();
        self.questions[position].answer = Some(answer.clone());
        interaction::write_question(&self.run_dir.join("questions"), &self.questions[position])?;

        for task_id in affected {
            let Some(index) = self
                .analysis
                .plan
                .tasks
                .iter()
                .position(|t| t.id == task_id)
            else {
                continue;
            };
            if !matches!(&self.states[index], TaskState::AwaitingInput(q) if *q == id) {
                continue;
            }
            match &answer {
                Answer::Declined => {
                    self.fail_task(
                        index,
                        FailureKind::Declined,
                        format!(
                            "declined at the human rung: {}",
                            last_reason(&self.progress[index])
                        ),
                    );
                }
                Answer::Answered { text } => {
                    let progress = &mut self.progress[index];
                    progress.feedback.push(Feedback {
                        attempt: progress.attempts,
                        tier: String::new(),
                        summary: "the operator answered the open question".to_owned(),
                        detail: Some(text.clone()),
                        human: true,
                    });
                    // The answer buys a fresh allowance on the rung the task
                    // is standing on, and clears the deferrals that a pool
                    // outage racked up. It never moves the rung: if the chain
                    // exhausted, the task is already at the top of it.
                    if kind == QuestionKind::Unblock {
                        progress.attempts_on_rung = 0;
                    }
                    progress.defers = 0;
                    // Never resume out of a park, however tempting the warm
                    // session looks. Parking always discards the working tree
                    // — it has to, because another task runs before this one
                    // does again — so the session's account of what it wrote
                    // no longer matches the repository. Resuming would hand
                    // the agent a terse prompt and a conversation asserting
                    // edits that have been reverted. §14 pairs session resume
                    // with tree retention precisely so the two never diverge;
                    // the full prompt plus the operator's answer, which
                    // `feedback_section` labels as a human instruction, is
                    // what makes the retry correct rather than merely cheap.
                    progress.resume_next = false;
                    self.states[index] = TaskState::Pending;
                }
                Answer::Unanswered => unreachable!("handled above"),
            }
        }
        Ok(true)
    }

    /// Report of the run so far, safe to call mid-drain.
    fn snapshot(&self) -> RunReport {
        let tasks: Vec<TaskReport> = self
            .order
            .iter()
            .copied()
            // Tasks that never started append in plan order, so the report
            // reads as the run happened and still accounts for everything.
            .chain((0..self.analysis.plan.tasks.len()).filter(|i| !self.order.contains(i)))
            .map(|index| {
                task_report(
                    &self.analysis.plan.tasks[index],
                    &self.states[index],
                    &self.progress[index],
                )
            })
            .collect();
        let total_cost_usd = tasks
            .iter()
            .filter_map(TaskReport::total_cost_usd)
            .sum::<f64>();
        RunReport {
            run_id: self.run_id.clone(),
            branch: self.branch.clone(),
            gates: self.analysis.gates.iter().map(|g| g.name.clone()).collect(),
            gates_from_config: self.analysis.gates_from_config,
            warnings: self.warnings.clone(),
            tasks,
            halted_at: self.halted_at.clone(),
            questions: self.questions.clone(),
            total_cost_usd,
        }
    }

    /// Settle every task that never ran, then report.
    fn finish(&mut self) -> RunReport {
        let tasks = &self.analysis.plan.tasks;
        // Blocking propagates: a dependent of a blocked task is blocked too.
        // Repeat until stable rather than assuming plan order carries it.
        loop {
            let mut changed = false;
            for index in 0..tasks.len() {
                if !matches!(self.states[index], TaskState::Pending) {
                    continue;
                }
                let blocker = tasks[index].depends_on.iter().find(|dep| {
                    tasks
                        .iter()
                        .position(|t| t.id == **dep)
                        .is_some_and(|j| !matches!(self.states[j], TaskState::Done(_)))
                });
                if let Some(blocker) = blocker {
                    self.states[index] = TaskState::Blocked(blocker.clone());
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // Whatever is still Pending was never reached: the run halted.
        for state in &mut self.states {
            if matches!(state, TaskState::Pending) {
                *state = TaskState::Skipped;
            }
        }
        self.snapshot()
    }
}

/// Why a task is parked or failed, for the report.
///
/// The most recent *attempt failure* wins, not the most recent feedback entry:
/// the branches that park a task never record feedback, so once an operator has
/// answered anything, their answer would otherwise shadow every later failure
/// and the report would tell them a task is parked because they answered a
/// question. Human entries are excluded from the fallback for the same reason.
fn last_reason(progress: &Progress) -> String {
    progress
        .records
        .last()
        .and_then(|r| r.failure.as_ref())
        .map(|f| f.reason.clone())
        .or_else(|| {
            progress
                .feedback
                .iter()
                .rev()
                .find(|f| !f.human)
                .map(|f| f.summary.clone())
        })
        .unwrap_or_else(|| "no attempt on record".to_owned())
}

fn task_report(task: &Task, state: &TaskState, progress: &Progress) -> TaskReport {
    let records = &progress.records;
    let last = records.last();
    TaskReport {
        id: task.id.to_string(),
        title: task.title.clone(),
        model: last.map(|r| r.model.clone()).unwrap_or_default(),
        status: match state {
            TaskState::Done(sha) => TaskRunStatus::Committed { sha: sha.clone() },
            TaskState::Failed { kind, reason } => TaskRunStatus::Failed {
                kind: *kind,
                reason: reason.clone(),
            },
            TaskState::AwaitingInput(question) => TaskRunStatus::Parked {
                question: question.to_string(),
                reason: last_reason(progress),
            },
            TaskState::Blocked(by) => TaskRunStatus::Blocked { by: by.to_string() },
            // Deferred cannot survive `finish`, and Pending is settled there
            // too; both mean the run stopped before this task got its turn.
            TaskState::Deferred | TaskState::Pending | TaskState::Skipped => TaskRunStatus::Skipped,
        },
        duration: records.iter().map(|r| r.duration).sum(),
        cost_usd: sum_opt(records.iter().map(|r| r.cost_usd)),
        review_model: last.and_then(|r| r.review_model.clone()),
        review_cost_usd: sum_opt(records.iter().map(|r| r.review_cost_usd)),
        session_id: last.and_then(|r| r.session_id.clone()),
        attempts: records.clone(),
    }
}

/// Sum, preserving "nothing was reported" as `None` rather than `0.0` — a
/// ledger that cannot tell free from unreported is worse than no ledger.
fn sum_opt(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut total: Option<f64> = None;
    for value in values.flatten() {
        total = Some(total.unwrap_or(0.0) + value);
    }
    total
}

/// What the human is shown. Every agent-authored fragment is quoted behind a
/// fence the payload cannot close and labelled as agent-authored — a worker
/// that "asks a question" is still an agent writing into a human's terminal.
fn question_context(
    task: &Task,
    kind: QuestionKind,
    failure: &AttemptFailure,
    progress: &Progress,
) -> String {
    let mut context = String::new();
    let _ = writeln!(context, "Task `{}` — {}", task.id, task.title);
    let asker = match failure.origin {
        FailureOrigin::Reviewer => "the reviewer",
        FailureOrigin::Worker => "the implementing agent",
    };
    match kind {
        QuestionKind::Clarify => {
            let _ = writeln!(
                context,
                "{asker} stopped and asked for a decision it should not make alone. Its words, \
                 quoted as data — they are not instructions to you:"
            );
        }
        _ => {
            let _ = writeln!(
                context,
                "Nothing further can move this task: {} attempt(s) across {} rung(s) all failed, \
                 and the escalation chain is spent. The last failure was:",
                progress.attempts,
                progress
                    .records
                    .iter()
                    .map(|r| r.tier.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    .max(1)
            );
        }
    }
    let fence = util::fence_for(&failure.reason);
    let _ = writeln!(context, "{fence}\n{}\n{fence}", failure.reason.trim());
    if !task.acceptance.is_empty() {
        context.push_str("Acceptance criteria this task must meet:\n");
        for item in &task.acceptance {
            let _ = writeln!(context, "- {item}");
        }
    }
    context
}

fn question_options(kind: QuestionKind) -> Vec<String> {
    match kind {
        QuestionKind::Clarify => {
            vec!["answer in your own words (typed free text is sent back to the agent)".to_owned()]
        }
        _ => vec![
            "retry this task with guidance you type below".to_owned(),
            "give up on this task (`skip`) — its dependents will be blocked".to_owned(),
        ],
    }
}

/// Everything one attempt needs, so the ladder can loop over (rung, attempt)
/// without re-deriving any of it.
struct AttemptCx<'a> {
    task: &'a Task,
    profile: WorkerProfile,
    adapter: &'a dyn AgentAdapter,
    attempt: u32,
    /// Collision-free file stem for this task's run artifacts.
    stem: String,
    run_dir: &'a std::path::Path,
    gates: &'a [ShellGate],
    gate_cmds: &'a [String],
    reviewer: Option<Reviewer<'a>>,
    timeout: Duration,
    /// `None` on the first attempt.
    retry: Option<RetryBrief>,
}

/// What the retry prompt needs to know (§11.4).
struct RetryBrief {
    /// The session carries the earlier conversation, so the prompt is terse.
    resumed: bool,
    /// Every failure so far, oldest first.
    feedback: Vec<Feedback>,
}

/// The read-only worker that judges each attempt (§11.2). `None` only when
/// the user explicitly set `review = { enabled = false }`; a reviewer that
/// cannot be resolved is a hard error, never a silent downgrade.
#[derive(Clone)]
struct Reviewer<'a> {
    adapter: &'a dyn AgentAdapter,
    profile: WorkerProfile,
}

struct AttemptResult {
    outcome: Outcome,
    failure: Option<AttemptFailure>,
    /// The reviewer that actually judged this attempt, and its spend — both
    /// `None` when the cheap checks failed first and no review ran. Derived
    /// from the review having happened rather than from one being configured,
    /// so the ledger never credits a model with work it did not do (§13).
    review_model: Option<String>,
    review_cost_usd: Option<f64>,
}

/// Run one attempt and verify it, without deciding what happens next: the
/// caller owns commit, rollback, retry, and escalation (§11/§14).
fn run_attempt(
    cx: &AttemptCx<'_>,
    workspace: &Workspace,
    resume_session: Option<String>,
) -> Result<AttemptResult, TactusError> {
    let settings_path = cx.adapter.materialize_permissions(
        &cx.profile,
        cx.gate_cmds,
        &cx.run_dir.join("settings"),
        &format!("{}-{}", cx.stem, cx.attempt),
    )?;

    let task_run = TaskRun {
        prompt: materialize_prompt(cx.task, cx.gate_cmds, cx.run_dir, cx.retry.as_ref()),
        profile: cx.profile.clone(),
        workspace: workspace.root().to_path_buf(),
        resume_session,
        settings_path,
    };
    let command = cx.adapter.build(&task_run)?;
    let output = proc::run_with_timeout(command, cx.adapter.stdin_payload(&task_run), cx.timeout)?;

    let transcripts = cx.run_dir.join("transcripts");
    let transcript_path = transcripts.join(format!("{}-{}.json", cx.stem, cx.attempt));
    util::write_text(&transcript_path, &output.stdout)?;
    if !output.stderr.trim().is_empty() {
        util::write_text(
            &transcripts.join(format!("{}-{}.stderr.log", cx.stem, cx.attempt)),
            &output.stderr,
        )?;
    }

    let mut outcome: Outcome = cx.adapter.parse(&output)?;
    outcome.diff = workspace.capture_diff()?;
    outcome.transcript_path = transcript_path;

    // Verification ladder (§11): outcome sanity → cheap static provenance →
    // gates → review. Cheapest and most objective first.
    let mut failure = evaluate_outcome(&outcome, &output);
    if failure.is_none() && cx.task.kind == TaskKind::Test && !gates::diff_adds_tests(&outcome.diff)
    {
        failure = Some(
            AttemptFailure::new(
                FailureKind::TestProvenance,
                "test provenance: this Test task adds no test code — a Test task that changes no \
                 tests proves nothing",
            )
            .with_feedback(
                "The diff contains no test code. Add tests that would fail without your change."
                    .to_owned(),
            ),
        );
    }
    if failure.is_none()
        && let Some(gate_failure) = gates::run_all(
            cx.gates,
            workspace,
            &cx.run_dir.join("gates"),
            &cx.stem,
            cx.attempt,
        )?
    {
        failure = Some(
            AttemptFailure::new(
                FailureKind::GateFailed,
                format!(
                    "gate `{}` failed: {}",
                    gate_failure.gate, gate_failure.summary
                ),
            )
            .with_feedback(gate_failure.log_tail),
        );
    }

    // §11.2: gates are objective but shallow — a strong reviewer judges the
    // diff against the acceptance criteria only once the cheap checks pass.
    let mut review_model = None;
    let mut review_cost_usd = None;
    if failure.is_none()
        && let Some(reviewer) = &cx.reviewer
    {
        review_model = Some(reviewer.profile.model.clone());
        let review = review::run_review(&review::ReviewCx {
            adapter: reviewer.adapter,
            profile: reviewer.profile.clone(),
            task: cx.task,
            diff: &outcome.diff,
            artifacts: &load_artifacts(cx.run_dir, cx.task),
            workspace: workspace.root(),
            run_dir: cx.run_dir,
            stem: format!("{}-{}", cx.stem, cx.attempt),
            timeout: review_timeout(cx.timeout),
        })?;
        review_cost_usd = review.cost_usd;
        failure = review_failure(review.result);
    }

    Ok(AttemptResult {
        outcome,
        failure,
        review_model,
        review_cost_usd,
    })
}

/// Turn a review result into an attempt failure, or `None` if it passed.
fn review_failure(result: review::ReviewResult) -> Option<AttemptFailure> {
    let verdict = match result {
        // The judge could not run. That is an environment problem, not a
        // rejection of the code: it is attributed to the reviewer so the
        // ladder defers instead of blaming the implementer.
        review::ReviewResult::Unavailable { status, detail } => {
            let kind = match status {
                OutcomeStatus::RateLimited => FailureKind::RateLimited,
                OutcomeStatus::Timeout => FailureKind::Timeout,
                _ => FailureKind::ReviewUnavailable,
            };
            return Some(
                AttemptFailure::new(
                    kind,
                    format!("reviewer unavailable: {}", util::head(&detail, 400)),
                )
                .from_reviewer(),
            );
        }
        review::ReviewResult::Judged(verdict) => verdict,
    };

    // §12: the reviewer declined to judge and asked for a person. That is not
    // a rejection of the code, so it must not spend an attempt or escalate —
    // it parks the task and asks.
    if verdict.needs_human {
        let reasons = if verdict.reasons.is_empty() {
            "the reviewer asked for a human decision but gave no reason".to_owned()
        } else {
            verdict.reasons.join("; ")
        };
        return Some(
            AttemptFailure::new(
                FailureKind::NeedsHuman,
                format!("reviewer asked for a human decision: {reasons}"),
            )
            .from_reviewer(),
        );
    }

    // A pass carrying required changes contradicts itself, and the engine is
    // about to commit on the strength of it — fail closed and say why rather
    // than discard the blockers the reviewer took the trouble to write.
    let contradictory = verdict.pass && !verdict.required_changes.is_empty();
    if verdict.pass && !contradictory {
        return None;
    }
    let summary = if contradictory {
        format!(
            "reviewer passed the change but still required: {}",
            verdict.required_changes.join("; ")
        )
    } else if verdict.reasons.is_empty() {
        "no reasons given".to_owned()
    } else {
        verdict.reasons.join("; ")
    };
    // required_changes is what the retry gets back verbatim (§11.4).
    let feedback = if verdict.required_changes.is_empty() {
        summary.clone()
    } else {
        verdict
            .required_changes
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some(
        AttemptFailure::new(
            FailureKind::ReviewFailed,
            // Head, not tail: the reviewer's first reason is its primary
            // finding, and that is what has to reach the user.
            format!("review failed: {}", util::head(&summary, 400)),
        )
        .with_feedback(feedback),
    )
}

/// Artifacts this task should be judged against: its declared inputs, plus
/// the conventions brief whenever one exists (§11.2 injects it into every
/// downstream prompt).
fn load_artifacts(run_dir: &std::path::Path, task: &Task) -> Vec<(String, String)> {
    let mut wanted: Vec<String> = vec![CONVENTIONS_BRIEF.to_owned()];
    wanted.extend(task.artifacts_in.iter().map(|id| id.as_str().to_owned()));
    // A task's own outputs are not evidence for judging it: the reviewer
    // would be validating the change against a standard the same attempt just
    // wrote. Declared inputs and the brief only.
    let produced: Vec<&str> = task.artifacts_out.iter().map(|id| id.as_str()).collect();
    let mut seen: Vec<String> = Vec::new();
    wanted
        .into_iter()
        .filter(|id| !produced.contains(&id.as_str()))
        .filter(|id| {
            let fresh = !seen.contains(id);
            if fresh {
                seen.push(id.clone());
            }
            fresh
        })
        .filter_map(|id| {
            let content = fs::read_to_string(artifact_path(run_dir, &id)).ok()?;
            (!content.trim().is_empty()).then_some((id, content))
        })
        .collect()
}

const CONVENTIONS_BRIEF: &str = "conventions-brief";

/// Outcome-level failure reasons, before gates get a say.
fn evaluate_outcome(outcome: &Outcome, output: &proc::ProcessOutput) -> Option<AttemptFailure> {
    let detail = || {
        outcome
            .detail
            .clone()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| {
                let stderr = util::tail(&output.stderr, 400);
                if stderr.is_empty() {
                    "no diagnostic output; see the transcript".to_owned()
                } else {
                    stderr
                }
            })
    };
    match outcome.status {
        // §12: the marker is honoured only on a run that actually completed.
        // `detail` carries the agent's partial output on every failure path,
        // and the prompt puts the marker string in front of the agent on every
        // fresh attempt — so scanning before the status match let a timed-out
        // or rate-limited attempt reclassify itself as a question purely by
        // quoting its own instructions back. That silently defeated "a rate
        // limit defers rather than burning an attempt" (§19), which is most of
        // the point of dispatching on `FailureKind` at all.
        OutcomeStatus::Completed => {
            // An agent that stopped to ask has not failed at anything —
            // punishing it for the empty diff its own question explains would
            // teach it never to ask, so this precedes the evidence rules.
            if let Some(question) = worker_question(outcome.detail.as_deref()) {
                return Some(AttemptFailure::new(FailureKind::NeedsHuman, question));
            }
            if !outcome.diff.trim().is_empty() {
                return None;
            }
            // §11 evidence axis: an empty diff can never pass.
            Some(
                AttemptFailure::new(
                    FailureKind::EmptyDiff,
                    "agent reported success but the diff is empty — \"done\" claims require \
                     changed code",
                )
                .with_feedback(
                    "You reported the task complete, but the repository is unchanged. Either make \
                     the change the task asks for, or explain what blocks it using the \
                     TACTUS-QUESTION marker."
                        .to_owned(),
                ),
            )
        }
        OutcomeStatus::AgentError => Some(
            AttemptFailure::new(
                FailureKind::AgentError,
                format!("agent error (exit {:?}): {}", output.code, detail()),
            )
            .with_feedback(detail()),
        ),
        OutcomeStatus::Timeout => Some(
            AttemptFailure::new(
                FailureKind::Timeout,
                "attempt hit the wall-clock timeout",
            )
            // §19: the feedback is the transcript tail. Without it the retry
            // starts blind on a task already known to run long.
            .with_feedback(format!(
                "Your previous attempt was cut off at its time limit. Work in smaller steps and \
                 finish the highest-value change first. Its last output was:\n{}",
                util::tail(&outcome.detail.clone().unwrap_or_default(), 2000)
            )),
        ),
        OutcomeStatus::RateLimited => Some(AttemptFailure::new(
            FailureKind::RateLimited,
            format!("pool rate-limited: {}", detail()),
        )),
    }
}

/// §12: a worker may flag a decision it should not make alone. Everything from
/// the marker onward is taken, so a multi-line question survives.
///
/// The LAST marker wins, matching the prompt's "end your message with it" and
/// `review.rs`'s rule for verdicts: models restate an instruction before acting
/// on it, so an earlier occurrence is an echo, not the question. The engine
/// itself puts the marker in front of the agent — the empty-diff feedback names
/// it verbatim — so an echo is the expected case, not a rare one.
fn worker_question(detail: Option<&str>) -> Option<String> {
    let detail = detail?;
    let start = detail.rfind(QUESTION_MARKER)?;
    let text = detail[start + QUESTION_MARKER.len()..].trim();
    (!text.is_empty()).then(|| util::head(text, 2000))
}

/// §14 prompt materialization: body + acceptance + artifact inputs + the
/// exact gate commands the agent is permitted to run (the allow rules are
/// exact-match, so the agent must know the literal strings), plus — on a
/// retry — why the last attempt did not pass (§11.4).
fn materialize_prompt(
    task: &Task,
    gate_cmds: &[String],
    run_dir: &std::path::Path,
    retry: Option<&RetryBrief>,
) -> String {
    // A resumed session already holds the task, the artifacts, and the rules;
    // re-sending them buys nothing and buries the one thing that changed.
    if let Some(retry) = retry
        && retry.resumed
    {
        let mut prompt = String::new();
        prompt.push_str(
            "Your previous attempt did not pass verification. Fix it in this same session — the \
             task and its rules have not changed.\n\n",
        );
        prompt.push_str(&feedback_section(&retry.feedback, false));
        prompt.push_str(
            "\nMake the smallest change that resolves the above, then stop and summarize.\n",
        );
        return prompt;
    }

    let mut prompt = String::new();
    prompt.push_str(
        "You are executing one task from a frozen plan, conducted by the tactus engine.\n\n",
    );
    let _ = writeln!(prompt, "# Task: {}\n", task.title);
    if !task.body.is_empty() {
        prompt.push_str(&task.body);
        prompt.push_str("\n\n");
    }
    if !task.acceptance.is_empty() {
        prompt.push_str("Acceptance criteria (all must hold when you finish):\n");
        for item in &task.acceptance {
            let _ = writeln!(prompt, "- {item}");
        }
        prompt.push('\n');
    }
    // Artifacts are real files in the run directory: a consumer is shown the
    // content that exists, never told to look for something nothing wrote.
    for id in &task.artifacts_in {
        let path = artifact_path(run_dir, id.as_str());
        match fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                let _ = writeln!(
                    prompt,
                    "Input artifact `{id}` (produced by an earlier task):\n---\n{}\n---\n",
                    content.trim()
                );
            }
            _ => {
                let _ = writeln!(
                    prompt,
                    "Note: this task expected input artifact `{id}`, but the earlier task did \
                     not leave one. Work from the repository as it stands.\n"
                );
            }
        }
    }
    for id in &task.artifacts_out {
        let _ = writeln!(
            prompt,
            "Before you finish, write artifact `{id}` — the notes later tasks depend on — to:\n\
             {}\n",
            artifact_path(run_dir, id.as_str()).display()
        );
    }
    if !gate_cmds.is_empty() {
        prompt.push_str(
            "Verification gates run after you finish. You may run EXACTLY these commands \
             yourself to check your work (any other shell command is denied):\n",
        );
        for cmd in gate_cmds {
            let _ = writeln!(prompt, "- {cmd}");
        }
        prompt.push('\n');
    }
    // Whatever earlier rungs learned travels with the task, even though the
    // conversation does not (§11.4).
    if let Some(retry) = retry {
        prompt.push_str(&feedback_section(&retry.feedback, true));
    }
    prompt.push_str(
        "Rules:\n\
         - Complete ONLY this task; leave work that belongs to other tasks alone.\n\
         - Edit files inside this repository only.\n\
         - NEVER run git commit, branch, merge, push, or reset — the engine owns git.\n\
         - When the acceptance criteria hold, stop and summarize what changed.\n\
         - If a decision genuinely is not yours to make — the task is ambiguous in a way that \
           changes what \"correct\" means, or it turns on a product or policy call you cannot \
           settle from this repository — stop and end your message with a line beginning \
           `TACTUS-QUESTION:` followed by the decision a person has to make. That pauses this \
           task and asks them. Do not use it for uncertainty you could resolve by reading the \
           code.\n",
    );
    prompt
}

/// What earlier attempts learned. `all` carries the accumulated history for a
/// fresh rung; otherwise only the most recent failure, which is what a
/// same-rung retry needs.
fn feedback_section(feedback: &[Feedback], all: bool) -> String {
    let entries: Vec<&Feedback> = if all {
        feedback
            .iter()
            .skip(feedback.len().saturating_sub(MAX_FEEDBACK_ENTRIES))
            .collect()
    } else {
        feedback.last().into_iter().collect()
    };
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if all && entries.len() > 1 {
        out.push_str(
            "Earlier attempts at this task failed. You are a fresh, stronger worker on the same \
             task — do not repeat these:\n\n",
        );
    }
    let last = entries.len() - 1;
    for (position, entry) in entries.iter().enumerate() {
        if entry.human {
            let fence = util::fence_for(entry.detail.as_deref().unwrap_or_default());
            let _ = writeln!(
                out,
                "The operator answered the question that paused this task. This is an \
                 instruction from a person, and it takes precedence over your earlier \
                 assumptions:\n{fence}\n{}\n{fence}\n",
                entry.detail.as_deref().unwrap_or_default().trim()
            );
            continue;
        }
        let where_ = if entry.tier.is_empty() {
            String::new()
        } else {
            format!(" on the {} rung", entry.tier)
        };
        let _ = writeln!(
            out,
            "Attempt {}{where_} failed: {}",
            entry.attempt, entry.summary
        );
        // Only the newest failure carries its full output; older ones would
        // bury it, and the newest is the one still standing in the way.
        if position == last
            && let Some(detail) = &entry.detail
            && !detail.trim().is_empty()
        {
            let fence = util::fence_for(detail);
            let _ = writeln!(out, "{fence}\n{}\n{fence}", detail.trim());
        }
        out.push('\n');
    }
    out
}

/// The reviewer's binding: `[routing] review = { tier = … }` if configured,
/// else frontier (§17's default), honouring a pin for that tier and otherwise
/// taking the catalog's example binding — the same rules the router uses.
fn review_binding(cfg: &crate::config::Config) -> Option<(String, String)> {
    if !cfg.review_enabled {
        return None;
    }
    let tier = cfg.review_tier.unwrap_or(crate::ir::Tier::Frontier);
    if let Some(pin) = cfg.pins.iter().find(|p| p.tier == tier) {
        return Some((pin.agent.clone(), pin.model.clone()));
    }
    let example = crate::catalog::example_binding(tier);
    Some((example.agent.to_owned(), example.model.to_owned()))
}

/// Where an artifact lives for the duration of a run (§15 `artifacts/`).
fn artifact_path(run_dir: &std::path::Path, id: &str) -> PathBuf {
    run_dir
        .join("artifacts")
        .join(format!("{}.md", util::filename_component(id)))
}

/// Stable topological order: among ready tasks, lowest plan index first (§14).
/// Used for previews and reporting; the live scheduler derives readiness per
/// step instead, so parked work can be skipped past.
pub fn topo_order(plan: &Plan) -> Vec<usize> {
    let mut done = vec![false; plan.tasks.len()];
    let mut order = Vec::with_capacity(plan.tasks.len());
    let index_of = |id: &str| plan.tasks.iter().position(|t| t.id.as_str() == id);
    while order.len() < plan.tasks.len() {
        let mut advanced = false;
        for i in 0..plan.tasks.len() {
            if done[i] {
                continue;
            }
            let ready = plan.tasks[i]
                .depends_on
                .iter()
                .all(|d| index_of(d.as_str()).is_none_or(|j| done[j]));
            if ready {
                done[i] = true;
                order.push(i);
                advanced = true;
                break;
            }
        }
        if !advanced {
            // Unreachable on a validated plan; degrade to plan order.
            for (i, flag) in done.iter_mut().enumerate() {
                if !*flag {
                    *flag = true;
                    order.push(i);
                }
            }
        }
    }
    order
}

impl RunReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "run: {}", self.run_id);
        let _ = writeln!(out, "branch: {} (return with: git switch -)", self.branch);
        if self.gates.is_empty() {
            let _ = writeln!(out, "gates: none");
        } else {
            let _ = writeln!(
                out,
                "gates: {} [{}]",
                self.gates.join(", "),
                if self.gates_from_config {
                    "from config"
                } else {
                    "derived"
                }
            );
        }
        for warning in &self.warnings {
            let _ = writeln!(out, "warning: {warning}");
        }
        for task in &self.tasks {
            match &task.status {
                TaskRunStatus::Committed { sha } => {
                    let review = match (&task.review_model, task.review_cost_usd) {
                        (Some(model), Some(cost)) => format!(" + review {model} ${cost:.4}"),
                        (Some(model), None) => format!(" + review {model}"),
                        _ => String::new(),
                    };
                    let _ = writeln!(
                        out,
                        "  {}: committed {sha} — {} [{}] ({:.1}s, {} ${:.4}{review})",
                        task.id,
                        task.title,
                        task.trail(),
                        task.duration.as_secs_f64(),
                        task.model,
                        task.cost_usd.unwrap_or(0.0),
                    );
                }
                TaskRunStatus::Failed { reason, .. } => {
                    let _ = writeln!(out, "  {}: FAILED [{}] — {reason}", task.id, task.trail());
                }
                TaskRunStatus::Parked { question, reason } => {
                    let _ = writeln!(
                        out,
                        "  {}: PARKED on {question} [{}] — {reason}",
                        task.id,
                        task.trail()
                    );
                }
                TaskRunStatus::Blocked { by } => {
                    let _ = writeln!(out, "  {}: blocked by `{by}`", task.id);
                }
                TaskRunStatus::Skipped => {
                    let _ = writeln!(out, "  {}: skipped (run halted)", task.id);
                }
            }
        }
        let open: Vec<&QuestionRecord> = self.questions.iter().filter(|q| q.is_open()).collect();
        if !open.is_empty() {
            let _ = writeln!(out, "open questions ({}):", open.len());
            for record in open {
                let _ = writeln!(
                    out,
                    "  {} [{}] — {}",
                    record.question.id,
                    record.question.kind,
                    util::head(
                        record
                            .question
                            .context
                            .lines()
                            .next()
                            .unwrap_or("(no context)"),
                        120
                    )
                );
            }
            let _ = writeln!(
                out,
                "  payloads: {}",
                std::path::Path::new(".tactus")
                    .join("runs")
                    .join(&self.run_id)
                    .join("questions")
                    .display()
            );
        }
        let _ = writeln!(out, "total: ${:.4} (api-equivalent)", self.total_cost_usd);
        match self.outcome() {
            RunOutcome::Halted => {
                let _ = writeln!(
                    out,
                    "run halted at `{}`; completed tasks are committed on {}",
                    self.halted_at.as_deref().unwrap_or("?"),
                    self.branch
                );
            }
            RunOutcome::Parked => {
                let _ = writeln!(
                    out,
                    "run ended with {} task(s) parked on unanswered questions: {}",
                    self.parked_tasks().len(),
                    self.parked_tasks().join(", ")
                );
            }
            RunOutcome::Complete => {
                let committed = self
                    .tasks
                    .iter()
                    .filter(|t| matches!(t.status, TaskRunStatus::Committed { .. }))
                    .count();
                let _ = writeln!(
                    out,
                    "run complete: {committed} task(s) committed on {}",
                    self.branch
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Caps, ProcessOutput};
    use crate::ir::{TaskId, Usage};
    use std::path::Path;
    use std::process::Command;
    use std::sync::Mutex;

    #[derive(Clone, Copy, PartialEq)]
    enum Effect {
        /// Simulates an agent that edits the workspace and succeeds.
        EditFile,
        /// Simulates an agent that writes real test code.
        EditTest,
        /// Simulates a lying agent: success report, no changes.
        NoEdit,
        /// Simulates an agent-side failure.
        Error,
        /// Simulates the pool being exhausted.
        RateLimited,
        /// Edits, then stops and asks the operator a question (§12).
        AskQuestion,
    }

    #[derive(Clone, Copy, PartialEq)]
    enum ReviewBehavior {
        Pass,
        Fail,
        /// Prose with no verdict block: drives the re-ask path.
        Unparseable,
        /// The judge itself could not run.
        RateLimited,
        /// §12: the reviewer declines to judge and asks for a person.
        NeedsHuman,
    }

    /// Scripted stand-in for a real CLI. `build` performs the "agent edit"
    /// directly (test-only shortcut) and returns a trivial command; `parse`
    /// reports the scripted outcome. Read-only profiles are review
    /// invocations and answer with a verdict, exercising the real
    /// command → stdout → parse → verdict path.
    ///
    /// Both scripts are consumed per invocation and the final entry repeats,
    /// so a one-element script behaves exactly like the fixed adapter did.
    struct FakeAdapter {
        effects: Vec<Effect>,
        reviews: Vec<ReviewBehavior>,
        calls: Mutex<Calls>,
    }

    #[derive(Default)]
    struct Calls {
        worker: usize,
        review: usize,
        runs: Vec<RecordedRun>,
    }

    #[derive(Clone)]
    struct RecordedRun {
        model: String,
        resume: Option<String>,
        prompt: String,
    }

    /// Marker the fake's review command prints so `parse` can tell a review
    /// invocation from an implementation one.
    const REVIEW_MARKER: &str = "TACTUS-FAKE-REVIEW";

    impl FakeAdapter {
        fn new(effects: Vec<Effect>, reviews: Vec<ReviewBehavior>) -> Self {
            Self {
                effects,
                reviews,
                calls: Mutex::new(Calls::default()),
            }
        }

        fn runs(&self) -> Vec<RecordedRun> {
            self.calls
                .lock()
                .map(|c| c.runs.clone())
                .unwrap_or_default()
        }
    }

    fn fake(effect: Effect) -> FakeSource {
        source(vec![effect], vec![ReviewBehavior::Pass])
    }

    fn source(effects: Vec<Effect>, reviews: Vec<ReviewBehavior>) -> FakeSource {
        FakeSource {
            adapter: FakeAdapter::new(effects, reviews),
        }
    }

    /// The last scripted entry repeats forever.
    fn scripted<T: Copy>(script: &[T], index: usize, fallback: T) -> T {
        script
            .get(index)
            .copied()
            .or_else(|| script.last().copied())
            .unwrap_or(fallback)
    }

    impl AgentAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            "claude-code"
        }

        fn probe(&self) -> Result<Caps, TactusError> {
            Ok(Caps {
                version: "0.0.0-fake".to_owned(),
                json_output: true,
                session_resume: true,
                cost_reporting: true,
                read_only_mode: true,
                acp: false,
                model_list: false,
            })
        }

        fn build(&self, run: &TaskRun) -> Result<std::process::Command, TactusError> {
            if run.profile.permissions == PermissionMode::ReadOnly {
                let mut cmd = shell_command(&format!("echo {REVIEW_MARKER}"));
                cmd.current_dir(&run.workspace);
                return Ok(cmd);
            }
            let index = {
                let mut calls = self.calls.lock().map_err(|_| TactusError::Agent {
                    message: "fake adapter lock poisoned".to_owned(),
                })?;
                let index = calls.worker;
                calls.worker += 1;
                calls.runs.push(RecordedRun {
                    model: run.profile.model.clone(),
                    resume: run.resume_session.clone(),
                    prompt: run.prompt.clone(),
                });
                index
            };
            let edit: Option<(&str, String)> =
                match scripted(&self.effects, index, Effect::EditFile) {
                    Effect::EditFile | Effect::AskQuestion => {
                        let marker = run.workspace.join("agent-output.txt");
                        let previous = fs::read_to_string(&marker).unwrap_or_default();
                        Some(("agent-output.txt", format!("{previous}edited: {index}\n")))
                    }
                    Effect::EditTest => Some((
                        "widget_test.rs",
                        "#[test]\nfn widget_works() {\n    assert!(true);\n}\n".to_owned(),
                    )),
                    Effect::NoEdit | Effect::Error | Effect::RateLimited => None,
                };
            if let Some((name, content)) = edit {
                fs::write(run.workspace.join(name), content).map_err(|e| TactusError::Agent {
                    message: format!("fake edit failed: {e}"),
                })?;
            }
            let mut cmd = shell_command("exit 0");
            cmd.current_dir(&run.workspace);
            Ok(cmd)
        }

        // Delegate to the real generator so the engine's permission wiring is
        // exercised, not stubbed out.
        fn materialize_permissions(
            &self,
            profile: &WorkerProfile,
            gate_cmds: &[String],
            dir: &Path,
            stem: &str,
        ) -> Result<Option<PathBuf>, TactusError> {
            crate::agent::claude::ClaudeCodeAdapter
                .materialize_permissions(profile, gate_cmds, dir, stem)
        }

        fn parse(&self, out: &ProcessOutput) -> Result<Outcome, TactusError> {
            if out.stdout.contains(REVIEW_MARKER) {
                let index = {
                    let mut calls = self.calls.lock().map_err(|_| TactusError::Agent {
                        message: "fake adapter lock poisoned".to_owned(),
                    })?;
                    let index = calls.review;
                    calls.review += 1;
                    index
                };
                let behavior = scripted(&self.reviews, index, ReviewBehavior::Pass);
                if behavior == ReviewBehavior::RateLimited {
                    return Ok(fake_outcome(
                        OutcomeStatus::RateLimited,
                        Some("5-hour limit reached".to_owned()),
                        "fake-review-session",
                        Some(0.0),
                        out.duration,
                    ));
                }
                let answer = match behavior {
                    ReviewBehavior::Pass => {
                        "Checked every criterion.\n```json\n{\"pass\": true, \"reasons\": \
                         [\"meets the acceptance criteria\"], \"required_changes\": []}\n```"
                    }
                    ReviewBehavior::Fail => {
                        "The diff misses a case.\n```json\n{\"pass\": false, \"reasons\": \
                         [\"no error handling for empty input\"], \"required_changes\": \
                         [\"handle the empty-input case\"]}\n```"
                    }
                    ReviewBehavior::NeedsHuman => {
                        "This turns on a product decision.\n```json\n{\"pass\": false, \
                         \"reasons\": [\"the acceptance criteria contradict the API contract\"], \
                         \"needs_human\": true}\n```"
                    }
                    ReviewBehavior::Unparseable => "Looks fine to me, ship it.",
                    ReviewBehavior::RateLimited => unreachable!("handled above"),
                };
                return Ok(fake_outcome(
                    OutcomeStatus::Completed,
                    Some(answer.to_owned()),
                    "fake-review-session",
                    Some(0.05),
                    out.duration,
                ));
            }
            // `build` already consumed this invocation's slot.
            let index = self
                .calls
                .lock()
                .map(|c| c.worker.saturating_sub(1))
                .unwrap_or(0);
            let effect = scripted(&self.effects, index, Effect::EditFile);
            let status = match effect {
                Effect::Error => OutcomeStatus::AgentError,
                Effect::RateLimited => OutcomeStatus::RateLimited,
                Effect::EditFile | Effect::EditTest | Effect::NoEdit | Effect::AskQuestion => {
                    OutcomeStatus::Completed
                }
            };
            let detail = match effect {
                Effect::Error => Some("fake adapter error detail".to_owned()),
                Effect::RateLimited => Some("5-hour limit reached".to_owned()),
                Effect::AskQuestion => Some(
                    "I made a start but stopped.\nTACTUS-QUESTION: should cursors be opaque or \
                     signed?"
                        .to_owned(),
                ),
                _ => None,
            };
            let mut outcome = fake_outcome(
                status,
                detail,
                &format!("s{index}"),
                Some(0.01),
                out.duration,
            );
            outcome.usage = Some(Usage::default());
            Ok(outcome)
        }
    }

    fn fake_outcome(
        status: OutcomeStatus,
        detail: Option<String>,
        session: &str,
        cost_usd: Option<f64>,
        duration: Duration,
    ) -> Outcome {
        Outcome {
            status,
            diff: String::new(),
            detail,
            session_id: Some(session.to_owned()),
            usage: None,
            cost_usd,
            pool_drain: None,
            transcript_path: PathBuf::new(),
            duration,
        }
    }

    struct FakeSource {
        adapter: FakeAdapter,
    }

    impl AdapterSource for FakeSource {
        fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
            (id == "claude-code").then_some(&self.adapter as &dyn AgentAdapter)
        }
    }

    /// Answers handed out in order; anything past the script is unanswered,
    /// which is exactly how a detached terminal behaves.
    struct ScriptedAnswers {
        answers: Mutex<std::collections::VecDeque<Answer>>,
    }

    impl ScriptedAnswers {
        fn new(answers: Vec<Answer>) -> Self {
            Self {
                answers: Mutex::new(answers.into()),
            }
        }
    }

    impl AnswerSource for ScriptedAnswers {
        fn id(&self) -> &'static str {
            "scripted"
        }

        fn resolve(&self, _question: &Question) -> Result<Answer, TactusError> {
            Ok(self
                .answers
                .lock()
                .ok()
                .and_then(|mut a| a.pop_front())
                .unwrap_or(Answer::Unanswered))
        }
    }

    #[derive(Default)]
    struct RecordingSleeper {
        waits: Mutex<Vec<Duration>>,
    }

    impl Sleeper for RecordingSleeper {
        fn sleep(&self, duration: Duration) {
            if let Ok(mut waits) = self.waits.lock() {
                waits.push(duration);
            }
        }
    }

    impl RecordingSleeper {
        fn waits(&self) -> Vec<Duration> {
            self.waits.lock().map(|w| w.clone()).unwrap_or_default()
        }
    }

    /// Shared with the production path so tests exercise the same shell
    /// invocation (including its Windows quoting) rather than a parallel one.
    fn shell_command(script: &str) -> Command {
        crate::gates::ShellKind::native().command(script)
    }

    fn git_in(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn temp_engine_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tactus-engine-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create repo dir");
        git_in(&dir, &["init", "-q", "-b", "main"]);
        git_in(&dir, &["config", "user.email", "test@tactus.local"]);
        git_in(&dir, &["config", "user.name", "tactus tests"]);
        fs::write(dir.join("README.md"), "seed\n").expect("seed");
        fs::write(
            dir.join("plan.md"),
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\nMake it.\n\n\
             ## Document the widget\n<!-- tactus: id=t2 depends=t1 -->\nWrite it up.\n",
        )
        .expect("plan");
        git_in(&dir, &["add", "-A"]);
        git_in(&dir, &["commit", "-q", "-m", "seed"]);
        dir
    }

    /// Replace the plan and config, then commit so the tree is clean.
    fn seed(repo: &Path, plan: &str, config: Option<&str>) {
        fs::write(repo.join("plan.md"), plan).expect("plan");
        if let Some(config) = config {
            fs::write(repo.join("tactus.toml"), config).expect("config");
        }
        git_in(repo, &["add", "-A"]);
        git_in(repo, &["commit", "-q", "-m", "fixture"]);
    }

    fn options(repo: &Path) -> RunOptions {
        let mut opts = RunOptions::new(repo.join("plan.md"), repo.to_path_buf());
        opts.pools_path = Some(
            std::env::temp_dir()
                .join("tactus-engine-missing")
                .join("pools.toml"),
        );
        opts.attempt_timeout = Duration::from_secs(60);
        // Tests must never actually wait out a rate limit.
        opts.defer_backoff = Duration::ZERO;
        opts
    }

    fn committed(report: &RunReport, id: &str) -> bool {
        report
            .tasks
            .iter()
            .any(|t| t.id == id && matches!(t.status, TaskRunStatus::Committed { .. }))
    }

    fn task<'a>(report: &'a RunReport, id: &str) -> &'a TaskReport {
        report
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("no task `{id}` in {report:?}"))
    }

    // ---- step 1-6 behaviour, unchanged by the ladder ----------------------

    #[test]
    fn happy_path_commits_one_commit_per_task() {
        let repo = temp_engine_repo("happy");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run succeeds");

        assert_eq!(report.outcome(), RunOutcome::Complete);
        assert_eq!(report.tasks.len(), 2);
        assert!(
            report
                .tasks
                .iter()
                .all(|t| matches!(t.status, TaskRunStatus::Committed { .. })),
            "report: {report:?}"
        );
        // Per task: implementer 0.01 + reviewer 0.05 (§11.2 reviews every
        // attempt), so both spends are accounted for.
        assert!(
            (report.total_cost_usd - 0.12).abs() < 1e-9,
            "worker and reviewer spend both counted: {}",
            report.total_cost_usd
        );

        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert!(branch.trim().starts_with("tactus/run-"), "on run branch");
        let count = git_in(&repo, &["rev-list", "--count", "main..HEAD"]);
        assert_eq!(count.trim(), "2", "one commit per task");
        let log = git_in(&repo, &["log", "--format=%s", "main..HEAD"]);
        assert!(
            log.contains("[tactus] t1: Implement the widget"),
            "log: {log}"
        );
        assert!(log.contains("[tactus] t2: Document the widget"));
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "clean tree after run"
        );
        assert!(
            repo.join(".tactus").join("runs").exists(),
            "run dir written"
        );
    }

    #[test]
    fn dirty_tree_is_refused() {
        let repo = temp_engine_repo("dirty");
        fs::write(repo.join("stray.txt"), "uncommitted\n").expect("stray");
        let source = fake(Effect::EditFile);
        let err = run_with(&options(&repo), &source).expect_err("must refuse");
        assert!(err.to_string().contains("not clean"), "got: {err}");
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch.trim(), "main", "no run branch created");
    }

    #[test]
    fn passing_configured_gates_commit_and_are_reported() {
        let repo = temp_engine_repo("gatepass");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\n",
            Some("[[gates]]\nname = \"version\"\ncmd = \"git --version\"\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
        assert_eq!(report.gates, ["version"]);
        assert!(report.gates_from_config);
        assert!(report.render().contains("gates: version [from config]"));
    }

    #[test]
    fn unresolvable_gate_refuses_at_preflight() {
        let repo = temp_engine_repo("gateresolve");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\n",
            Some("[[gates]]\nname = \"ghost\"\ncmd = \"definitely-not-a-real-tool-xyz build\"\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let err = run_with(&opts, &source).expect_err("must refuse");
        assert!(err.to_string().contains("not found on PATH"), "got: {err}");
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch.trim(), "main", "refused before branching");
    }

    #[test]
    fn test_task_without_test_code_fails_provenance() {
        let repo = temp_engine_repo("provenance");
        seed(
            &repo,
            "## Test the widget\n<!-- tactus: id=tt depends= -->\nAdd coverage.\n",
            // One rung, one attempt: the provenance failure is what is under
            // test, not the ladder's reaction to it.
            Some("[routing]\ntest = { chain = [\"small\"], attempts_per = 1 }\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("engine ok");
        let reason = &task(&report, "tt").attempts[0]
            .failure
            .as_ref()
            .expect("provenance should fail")
            .reason;
        assert!(reason.contains("provenance"), "reason: {reason}");
    }

    #[test]
    fn test_task_adding_real_tests_passes_provenance() {
        let repo = temp_engine_repo("provenance-ok");
        seed(
            &repo,
            "## Test the widget\n<!-- tactus: id=tt depends= -->\n",
            None,
        );

        let source = fake(Effect::EditTest);
        let report = run_with(&options(&repo), &source).expect("engine ok");
        assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
        assert!(committed(&report, "tt"));
    }

    #[test]
    fn gate_residue_is_scrubbed_not_committed() {
        let repo = temp_engine_repo("residue");
        // A gate that creates a file: residue must never reach a commit nor
        // survive the task.
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\n",
            Some("[[gates]]\nname = \"leaky\"\ncmd = \"echo residue> residue.txt\"\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
        assert!(!repo.join("residue.txt").exists(), "residue scrubbed");
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "clean tree after run"
        );
        let log = git_in(&repo, &["log", "--name-only", "--format=", "main..HEAD"]);
        assert!(!log.contains("residue.txt"), "log: {log}");
    }

    #[test]
    fn the_reviewer_is_read_only_and_bound_to_the_review_tier() {
        let repo = temp_engine_repo("reviewbinding");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let settings = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("settings");
        let allow_list = |file: &str| -> Vec<String> {
            let text = fs::read_to_string(settings.join(file)).expect("settings written");
            let value: serde_json::Value = serde_json::from_str(&text).expect("json");
            value["permissions"]["allow"]
                .as_array()
                .expect("allow list")
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_owned())
                .collect()
        };

        let reviewer = allow_list("00-t1-1-review.json");
        assert_eq!(reviewer, ["Read", "Glob", "Grep"], "read-only, no shell");

        let implementer = allow_list("00-t1-1.json");
        assert!(
            implementer.contains(&"Edit".to_owned()),
            "implementer can edit"
        );
    }

    #[test]
    fn review_can_be_switched_off_explicitly() {
        let repo = temp_engine_repo("noreview");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\n",
            Some("[routing]\nreview = { enabled = false }\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // A reviewer that would REJECT everything: if review still ran, the
        // task would never commit.
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
        assert!(report.tasks[0].review_model.is_none());
        assert!(report.tasks[0].review_cost_usd.is_none());
    }

    #[test]
    fn reviewer_spend_is_attributed_separately() {
        let repo = temp_engine_repo("reviewcost");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let t1 = task(&report, "t1");
        assert_eq!(t1.cost_usd, Some(0.01), "implementer's own spend");
        assert_eq!(t1.review_cost_usd, Some(0.05), "reviewer's, kept apart");
        assert_eq!(t1.review_model.as_deref(), Some("claude-opus-5"));
        assert!((t1.total_cost_usd().expect("both") - 0.06).abs() < 1e-9);
        let rendered = report.render();
        assert!(rendered.contains("+ review claude-opus-5"), "{rendered}");
    }

    #[test]
    fn the_run_record_survives_completion() {
        let repo = temp_engine_repo("record");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let report_path = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("report.json");
        let text = fs::read_to_string(&report_path).expect("report.json written");
        let restored: RunReport = serde_json::from_str(&text).expect("report round-trips");
        assert_eq!(restored.tasks.len(), 2);
        assert_eq!(restored.branch, report.branch);
        assert!(matches!(
            restored.tasks[0].status,
            TaskRunStatus::Committed { .. }
        ));
        assert_eq!(
            restored.tasks[0].attempts.len(),
            1,
            "the per-attempt ledger persists too"
        );
    }

    #[test]
    fn forward_dependencies_run_in_topo_order_not_plan_order() {
        let repo = temp_engine_repo("topo");
        seed(
            &repo,
            "## Second by dependency\n<!-- tactus: id=late depends=early -->\n\n\
             ## First by dependency\n<!-- tactus: id=early depends= -->\n",
            None,
        );

        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let ids: Vec<&str> = report.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["early", "late"], "dependency beats document order");
    }

    #[test]
    fn a_contradictory_pass_fails_closed() {
        let failure = review_failure(review::ReviewResult::Judged(crate::ir::Verdict {
            pass: true,
            reasons: vec!["looks fine".to_owned()],
            required_changes: vec!["parameterize the SQL".to_owned()],
            needs_human: false,
        }))
        .expect("a pass that still demands changes cannot commit");
        assert_eq!(failure.kind, FailureKind::ReviewFailed);
        assert!(
            failure.reason.contains("parameterize the SQL"),
            "{}",
            failure.reason
        );

        // A clean pass still passes.
        assert!(
            review_failure(review::ReviewResult::Judged(crate::ir::Verdict {
                pass: true,
                reasons: vec!["meets the criteria".to_owned()],
                required_changes: Vec::new(),
                needs_human: false,
            }))
            .is_none()
        );
    }

    #[test]
    fn an_unavailable_reviewer_is_not_a_rejection() {
        // A rate-limited or hung judge must not read as "your code is wrong",
        // or the ladder retries the implementer for an outage.
        let failure = review_failure(review::ReviewResult::Unavailable {
            status: OutcomeStatus::RateLimited,
            detail: "5-hour limit reached".to_owned(),
        })
        .expect("still fails the attempt");
        assert_eq!(failure.kind, FailureKind::RateLimited);
        assert_eq!(failure.origin, FailureOrigin::Reviewer);
        assert!(failure.is_outage(), "defers instead of blaming the worker");
        assert!(failure.reason.contains("reviewer unavailable"));

        let failure = review_failure(review::ReviewResult::Unavailable {
            status: OutcomeStatus::Timeout,
            detail: String::new(),
        })
        .expect("still fails");
        assert_eq!(failure.kind, FailureKind::Timeout);
        assert_eq!(failure.origin, FailureOrigin::Reviewer);

        let failure = review_failure(review::ReviewResult::Unavailable {
            status: OutcomeStatus::AgentError,
            detail: "spawn failed".to_owned(),
        })
        .expect("still fails");
        assert_eq!(failure.kind, FailureKind::ReviewUnavailable);
    }

    #[test]
    fn required_changes_reach_the_retry_as_a_clean_list() {
        let failure = review_failure(review::ReviewResult::Judged(crate::ir::Verdict {
            pass: false,
            reasons: vec!["incomplete".to_owned()],
            required_changes: vec![
                "handle the empty-input case".to_owned(),
                "add a round-trip test".to_owned(),
            ],
            needs_human: false,
        }))
        .expect("fails");
        assert_eq!(
            failure.feedback.as_deref(),
            Some("- handle the empty-input case\n- add a round-trip test"),
            "every item bulleted, including the first"
        );
        assert_eq!(
            failure.origin,
            FailureOrigin::Worker,
            "a rejected diff is the worker's to fix"
        );
    }

    #[test]
    fn prompt_names_the_allowed_gate_commands() {
        let task = Task {
            id: TaskId::from("t1"),
            kind: TaskKind::Implement,
            title: "Do the thing".to_owned(),
            body: String::new(),
            depends_on: Vec::new(),
            acceptance: Vec::new(),
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::new(),
        };
        let run_dir = std::env::temp_dir().join(format!("tactus-prompt-{}", std::process::id()));
        fs::create_dir_all(run_dir.join("artifacts")).expect("run dir");
        let prompt = materialize_prompt(
            &task,
            &["cargo check --all-targets".to_owned()],
            &run_dir,
            None,
        );
        assert!(prompt.contains("EXACTLY these commands"));
        assert!(prompt.contains("- cargo check --all-targets"));
        assert!(
            prompt.contains(QUESTION_MARKER),
            "the worker is told how to ask (§12)"
        );
        let bare = materialize_prompt(&task, &[], &run_dir, None);
        assert!(!bare.contains("EXACTLY these commands"));
    }

    #[test]
    fn prompt_wires_artifacts_to_real_files() {
        let run_dir = std::env::temp_dir().join(format!("tactus-artifact-{}", std::process::id()));
        let _ = fs::remove_dir_all(&run_dir);
        fs::create_dir_all(run_dir.join("artifacts")).expect("run dir");
        let mut task = Task {
            id: TaskId::from("t1"),
            kind: TaskKind::Implement,
            title: "Build it".to_owned(),
            body: String::new(),
            depends_on: Vec::new(),
            acceptance: Vec::new(),
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: vec![crate::ir::ArtifactId::from("api-contract")],
            artifacts_out: vec![crate::ir::ArtifactId::from("notes")],
        };

        // Missing input: say so plainly rather than pointing at nothing.
        let prompt = materialize_prompt(&task, &[], &run_dir, None);
        assert!(
            prompt.contains("did \n     not leave one") || prompt.contains("did not leave one")
        );
        assert!(
            prompt.contains("write artifact `notes`"),
            "producer told where to write"
        );

        // Present input: content is inlined.
        fs::write(
            artifact_path(&run_dir, "api-contract"),
            "cursor = base64(offset)",
        )
        .expect("artifact");
        let prompt = materialize_prompt(&task, &[], &run_dir, None);
        assert!(
            prompt.contains("cursor = base64(offset)"),
            "content inlined"
        );

        task.artifacts_in.clear();
        task.artifacts_out.clear();
        let bare = materialize_prompt(&task, &[], &run_dir, None);
        assert!(!bare.contains("artifact"));
    }

    // ---- step 7: the ladder in the engine ---------------------------------

    #[test]
    fn a_gate_failure_recovers_on_the_same_rung_via_session_resume() {
        // §21 definition-of-done (b). The gate demands a file only the second
        // attempt writes, so recovery is real rather than scripted around.
        let repo = temp_engine_repo("resume");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n\n\
                 [[gates]]\nname = \"needs-test\"\ncmd = \"git ls-files --error-unmatch \
                 widget_test.rs\"\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::EditFile, Effect::EditTest],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");

        assert!(committed(&report, "t1"), "report: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2, "one retry, not an escalation");
        assert_eq!(t1.attempts[0].tier, "small");
        assert_eq!(t1.attempts[1].tier, "small", "same rung");
        assert!(!t1.attempts[0].resumed);
        assert!(t1.attempts[1].resumed, "§11.4 retries in-session");

        let runs = source.adapter.runs();
        assert_eq!(runs[0].resume, None);
        assert_eq!(
            runs[1].resume.as_deref(),
            Some("s0"),
            "the retry resumed the failed attempt's session"
        );
        assert!(
            runs[1].prompt.contains("gate `needs-test` failed"),
            "the gate's own words go back: {}",
            runs[1].prompt
        );
        assert!(
            !runs[1].prompt.contains("# Task:"),
            "a resumed session already holds the task; the prompt stays terse"
        );

        // §14: a resumed retry keeps the tree, so the commit carries BOTH
        // attempts' work rather than only the last one's.
        let files = git_in(&repo, &["show", "--name-only", "--format=", "HEAD"]);
        assert!(files.contains("agent-output.txt"), "files: {files}");
        assert!(files.contains("widget_test.rs"), "files: {files}");
    }

    #[test]
    fn exhausting_a_rung_escalates_with_a_fresh_session_and_the_history() {
        // §21 definition-of-done (c).
        let repo = temp_engine_repo("escalate");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");

        assert!(committed(&report, "t1"), "report: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2);
        assert_eq!(t1.attempts[0].tier, "small");
        assert_eq!(t1.attempts[0].model, "claude-haiku-4-5");
        assert_eq!(t1.attempts[1].tier, "mid", "one rung up");
        assert_eq!(t1.attempts[1].model, "claude-sonnet-5");
        assert!(
            !t1.attempts[1].resumed,
            "§11.4: a new rung is a new session — a different model cannot \
             inherit another's conversation"
        );
        assert_eq!(t1.trail(), "small failed → mid ok");

        let runs = source.adapter.runs();
        // The adapter's own record, not just the report echoing what the
        // engine intended: the second attempt really was dispatched to the
        // higher rung's model.
        assert_eq!(runs[0].model, "claude-haiku-4-5");
        assert_eq!(runs[1].model, "claude-sonnet-5");
        assert_eq!(runs[1].resume, None, "fresh session");
        assert!(
            runs[1].prompt.contains("# Task:"),
            "a fresh worker gets the whole task again"
        );
        assert!(
            runs[1].prompt.contains("diff is empty"),
            "and what the previous rung got wrong: {}",
            runs[1].prompt
        );
    }

    #[test]
    fn a_parked_question_does_not_stop_the_runnable_frontier() {
        // §21 definition-of-done (d) and invariant 6: t1 exhausts its chain
        // and parks; the independent t3 must still commit.
        let repo = temp_engine_repo("park");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // t1 fails; every later attempt (t3's) edits and passes.
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");

        let TaskRunStatus::Parked { question, .. } = &task(&report, "t1").status else {
            panic!("t1 should park on a question: {report:?}");
        };
        assert!(committed(&report, "t3"), "independent work kept going");
        assert!(
            matches!(&task(&report, "t2").status, TaskRunStatus::Blocked { by } if by == "t1"),
            "a dependent of a parked task is blocked, not failed"
        );
        assert!(report.halted_at.is_none(), "parking never halts a run");
        assert_eq!(report.outcome(), RunOutcome::Parked);
        assert_eq!(report.parked_tasks(), ["t1"]);

        // The question is on disk where a notifier, `tactus answer`, or a UI
        // can read it — that file is the contract, not the terminal output.
        let path = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("questions")
            .join(format!("{question}.json"));
        let record: QuestionRecord =
            serde_json::from_str(&fs::read_to_string(&path).expect("question file"))
                .expect("parses");
        assert_eq!(record.question.kind, QuestionKind::Unblock);
        assert_eq!(record.question.affected_tasks, [TaskId::from("t1")]);
        assert!(record.answer.is_none(), "still open");
        assert!(record.question.context.contains("Doomed"));

        let rendered = report.render();
        assert!(rendered.contains("PARKED"), "{rendered}");
        assert!(rendered.contains("open questions (1)"), "{rendered}");
    }

    #[test]
    fn answering_the_question_retries_the_task_with_the_operators_words() {
        let repo = temp_engine_repo("answered");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let answers = ScriptedAnswers::new(vec![Answer::Answered {
            text: "the widget lives in src/widget.rs — write it there".to_owned(),
        }]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        assert!(committed(&report, "t1"), "report: {report:?}");
        assert_eq!(report.outcome(), RunOutcome::Complete);
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2, "the answer bought a fresh allowance");
        assert_eq!(
            t1.attempts[1].tier, "small",
            "an answer does not move the rung — the chain was already spent"
        );

        let runs = source.adapter.runs();
        assert!(
            runs[1].prompt.contains("src/widget.rs"),
            "the operator's answer reaches the agent: {}",
            runs[1].prompt
        );
        assert!(
            runs[1].prompt.contains("instruction from a person"),
            "and is labelled as an instruction, not quoted data"
        );

        let record = report.questions.first().expect("one question");
        assert!(
            matches!(&record.answer, Some(Answer::Answered { text }) if text.contains("widget.rs")),
            "the answer is recorded against the question: {record:?}"
        );
    }

    #[test]
    fn declining_fails_the_task_and_halt_is_the_default() {
        let repo = temp_engine_repo("declined");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let answers = ScriptedAnswers::new(vec![Answer::Declined]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        let TaskRunStatus::Failed { kind, reason } = &task(&report, "t1").status else {
            panic!("a declined question fails its task: {report:?}");
        };
        assert_eq!(*kind, FailureKind::Declined);
        assert!(!reason.is_empty());
        assert_eq!(
            report.halted_at.as_deref(),
            Some("t1"),
            "§17's default on_task_failure is halt"
        );
        assert_eq!(report.outcome(), RunOutcome::Halted);
    }

    #[test]
    fn on_task_failure_continue_keeps_independent_work_moving() {
        let repo = temp_engine_repo("continue");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some(
                "[engine]\non_task_failure = \"continue\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let answers = ScriptedAnswers::new(vec![Answer::Declined]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        assert!(report.halted_at.is_none(), "configured to continue");
        assert!(matches!(
            task(&report, "t1").status,
            TaskRunStatus::Failed { .. }
        ));
        assert!(committed(&report, "t3"));
        assert!(
            matches!(&task(&report, "t2").status, TaskRunStatus::Blocked { by } if by == "t1"),
            "§19: dependents of a failed task are blocked"
        );
    }

    #[test]
    fn a_rate_limit_defers_without_spending_an_attempt() {
        let repo = temp_engine_repo("ratelimit");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            // A single attempt on a single rung: if the rate limit spent it,
            // the task could never commit.
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::RateLimited, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let sleeper = RecordingSleeper::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: Some(&sleeper),
            },
        )
        .expect("run");

        assert!(
            committed(&report, "t1"),
            "the rate limit cost no attempt: {report:?}"
        );
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2);
        assert_eq!(
            t1.attempts[0].failure.as_ref().map(|f| f.kind),
            Some(FailureKind::RateLimited)
        );
        assert_eq!(t1.attempts[1].tier, "small", "never escalated for a pool");
        assert_eq!(
            sleeper.waits().len(),
            1,
            "waited once, because deferred work was all that was left"
        );
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "a deferred task hands back a clean tree — another task may run next"
        );
    }

    #[test]
    fn a_pool_that_never_returns_ends_at_the_human_rung() {
        let repo = temp_engine_repo("ratelimit-forever");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.max_defers = 2;
        let source = source(vec![Effect::RateLimited], vec![ReviewBehavior::Pass]);
        let sleeper = RecordingSleeper::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: Some(&sleeper),
            },
        )
        .expect("run");

        assert!(
            matches!(task(&report, "t1").status, TaskRunStatus::Parked { .. }),
            "an exhausted pool becomes a question, not an infinite retry: {report:?}"
        );
        assert_eq!(
            task(&report, "t1").attempts.len(),
            3,
            "two deferrals, then the attempt that gave up"
        );
        assert!(
            task(&report, "t1")
                .attempts
                .iter()
                .all(|a| a.tier == "small"),
            "a busy pool never pushes the task up-tier"
        );
        assert_eq!(sleeper.waits().len(), 2, "one wait per deferral");
    }

    #[test]
    fn an_unavailable_reviewer_defers_the_task_instead_of_escalating_it() {
        let repo = temp_engine_repo("reviewdown");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::EditFile],
            vec![ReviewBehavior::RateLimited, ReviewBehavior::Pass],
        );
        let sleeper = RecordingSleeper::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: Some(&sleeper),
            },
        )
        .expect("run");

        assert!(committed(&report, "t1"), "report: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2);
        assert_eq!(
            t1.attempts[0].failure.as_ref().map(|f| f.origin),
            Some(FailureOrigin::Reviewer),
            "the outage is attributed to the judge"
        );
        assert_eq!(
            t1.attempts[1].tier, "small",
            "the implementer was never escalated for the reviewer being down"
        );
    }

    #[test]
    fn a_reviewer_asking_for_a_human_parks_without_spending_the_chain() {
        let repo = temp_engine_repo("needshuman");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::NeedsHuman]);
        let report = run_with(&opts, &source).expect("run");

        let t1 = task(&report, "t1");
        assert!(
            matches!(t1.status, TaskRunStatus::Parked { .. }),
            "report: {report:?}"
        );
        assert_eq!(
            t1.attempts.len(),
            1,
            "the reviewer declined to judge, so nothing was retried or escalated"
        );
        assert_eq!(
            t1.attempts[0].failure.as_ref().map(|f| f.kind),
            Some(FailureKind::NeedsHuman)
        );
        let record = report.questions.first().expect("question raised");
        assert_eq!(record.question.kind, QuestionKind::Clarify);
        assert!(
            record
                .question
                .context
                .contains("contradict the API contract"),
            "the reviewer's reason reaches the person: {}",
            record.question.context
        );
        assert!(
            record.question.context.contains("not instructions to you"),
            "agent-authored text is labelled as data"
        );
    }

    #[test]
    fn a_worker_can_stop_and_ask_rather_than_guess() {
        let repo = temp_engine_repo("workerasks");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::AskQuestion, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let answers = ScriptedAnswers::new(vec![Answer::Answered {
            text: "opaque cursors".to_owned(),
        }]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        let record = report.questions.first().expect("the worker's question");
        assert_eq!(record.question.kind, QuestionKind::Clarify);
        assert!(
            record.question.context.contains("opaque or signed"),
            "context: {}",
            record.question.context
        );
        // Independent work ran while t1 waited, and the answer resumed it.
        assert!(committed(&report, "t3"));
        assert!(committed(&report, "t1"), "report: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(
            t1.attempts.len(),
            2,
            "asking cost no attempt — only the retry after the answer"
        );
        // Parking rolled the tree back, so the session's account of what it
        // wrote no longer matches the repository (§14 pairs resume with tree
        // retention). The retry therefore starts fresh and carries the whole
        // task again, with the operator's answer as an instruction.
        assert!(
            !t1.attempts[1].resumed,
            "a parked task never resumes into a tree that was reverted underneath it"
        );
        // Invocation order across the whole run, not just this task: t1 asks
        // (0), the independent t3 proceeds while t1 is parked (1), then t1
        // retries once the answer arrives (2). That interleaving is the point
        // of invariant 6, so the retry is the third invocation.
        let runs = source.adapter.runs();
        let retry = &runs[2];
        assert_eq!(retry.resume, None, "fresh session, not --resume");
        assert!(
            retry.prompt.contains("# Task:"),
            "the whole task is re-sent, since the session no longer carries it: {}",
            retry.prompt
        );
        assert!(
            retry.prompt.contains("opaque cursors"),
            "and the operator's answer travels with it: {}",
            retry.prompt
        );
        assert!(
            retry.prompt.contains("instruction from a person"),
            "labelled as an instruction rather than quoted as data"
        );
    }

    #[test]
    fn ci_mode_parks_rather_than_failing_and_says_so() {
        // §12: `interaction = "never"` degrades questions to parked-task
        // reporting, and the outcome is distinguishable from both a clean run
        // and a halt.
        let repo = temp_engine_repo("ci");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");

        assert_eq!(report.outcome(), RunOutcome::Parked);
        assert!(report.halted_at.is_none(), "parked is not halted");
        assert!(matches!(
            task(&report, "t1").status,
            TaskRunStatus::Parked { .. }
        ));
        assert!(committed(&report, "t3"));
        assert!(matches!(
            task(&report, "t2").status,
            TaskRunStatus::Blocked { .. }
        ));
        assert!(
            report.questions.iter().all(QuestionRecord::is_open),
            "nothing answered it, and nothing pretended to"
        );
    }

    #[test]
    fn an_unanswerable_question_is_never_asked_twice() {
        // Without this the hard block spins: ask, get nothing, ask again.
        let repo = temp_engine_repo("noloop");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let answers = CountingAnswers::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run terminates");
        assert_eq!(report.outcome(), RunOutcome::Parked);
        assert_eq!(
            answers.count(),
            1,
            "asked once; an unreachable channel is not retried"
        );
    }

    #[derive(Default)]
    struct CountingAnswers {
        calls: Mutex<usize>,
    }

    impl CountingAnswers {
        fn count(&self) -> usize {
            self.calls.lock().map(|c| *c).unwrap_or(0)
        }
    }

    impl AnswerSource for CountingAnswers {
        fn id(&self) -> &'static str {
            "counting"
        }

        fn resolve(&self, _question: &Question) -> Result<Answer, TactusError> {
            if let Ok(mut calls) = self.calls.lock() {
                *calls += 1;
            }
            Ok(Answer::Unanswered)
        }
    }

    #[test]
    fn agent_errors_and_empty_diffs_carry_feedback_the_retry_can_use() {
        let repo = temp_engine_repo("feedback");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::Error, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");
        assert!(committed(&report, "t1"), "report: {report:?}");
        let runs = source.adapter.runs();
        assert!(
            runs[1].prompt.contains("fake adapter error detail"),
            "the adapter's own diagnosis reaches the retry: {}",
            runs[1].prompt
        );
    }

    #[test]
    fn an_unparseable_reviewer_fails_after_one_reask() {
        let repo = temp_engine_repo("reviewprose");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Unparseable]);
        let report = run_with(&opts, &source).expect("engine ok");

        let failure = task(&report, "t1").attempts[0]
            .failure
            .as_ref()
            .expect("a reviewer that never answers cannot pass a task");
        assert_eq!(failure.kind, FailureKind::ReviewFailed);
        assert!(
            failure.reason.contains("re-ask"),
            "reason: {}",
            failure.reason
        );
        // The re-ask actually happened, and both sides are on record.
        let reviews = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("reviews");
        assert!(reviews.join("00-t1-1-review.json").is_file());
        assert!(
            reviews.join("00-t1-1-review-reask.json").is_file(),
            "one re-ask before giving up (§11.2)"
        );
        assert_eq!(
            git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
            "0",
            "nothing commits without a passing verdict"
        );
    }

    #[test]
    fn gate_logs_are_named_by_the_collision_free_stem() {
        let repo = temp_engine_repo("gatelogs");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [[gates]]\nname = \"never\"\ncmd = \"git frobnicate-not-a-command\"\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("engine ok");

        let failure = task(&report, "t1").attempts[0]
            .failure
            .as_ref()
            .expect("gate should fail");
        assert_eq!(failure.kind, FailureKind::GateFailed);
        let gates_dir = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("gates");
        assert!(
            gates_dir.join("00-t1-1-never.log").is_file(),
            "the log stem matches the task's other artifacts, so two ids that \
             sanitize alike cannot overwrite each other"
        );
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "rolled back"
        );
    }

    #[test]
    fn the_trail_summarizes_the_ladder() {
        let report = TaskReport {
            id: "t1".to_owned(),
            title: "x".to_owned(),
            model: "m".to_owned(),
            status: TaskRunStatus::Skipped,
            duration: Duration::ZERO,
            cost_usd: None,
            review_model: None,
            review_cost_usd: None,
            session_id: None,
            attempts: vec![
                attempt_record(1, "small", true),
                attempt_record(2, "small", true),
                attempt_record(3, "mid", false),
            ],
        };
        assert_eq!(report.trail(), "small×2 failed → mid ok");
    }

    fn attempt_record(attempt: u32, tier: &str, failed: bool) -> AttemptRecord {
        AttemptRecord {
            attempt,
            tier: tier.to_owned(),
            model: "m".to_owned(),
            resumed: false,
            duration: Duration::ZERO,
            cost_usd: None,
            review_model: None,
            review_cost_usd: None,
            session_id: None,
            failure: failed.then(|| FailureRecord {
                kind: FailureKind::GateFailed,
                origin: FailureOrigin::Worker,
                reason: "no".to_owned(),
            }),
        }
    }

    #[test]
    fn a_worker_question_is_read_from_the_marker_onward() {
        assert_eq!(
            worker_question(Some("Did some work.\nTACTUS-QUESTION: opaque or signed?")).as_deref(),
            Some("opaque or signed?")
        );
        // Multi-line questions survive, because the prompt asks for it last.
        assert_eq!(
            worker_question(Some("TACTUS-QUESTION: which store?\nRedis or Postgres?")).as_deref(),
            Some("which store?\nRedis or Postgres?")
        );
        assert_eq!(worker_question(Some("TACTUS-QUESTION:   ")), None);
        assert_eq!(worker_question(Some("no marker here")), None);
        assert_eq!(worker_question(None), None);
    }

    #[test]
    fn an_echoed_marker_does_not_swallow_the_real_question() {
        // The engine hands the agent this marker in every fresh prompt, and
        // the empty-diff feedback names it verbatim — so an agent mentioning
        // it before asking is the expected shape, not a corner case. Taking
        // the first occurrence would hand the operator the agent's reasoning
        // with the question buried at the end.
        let reply = "The retry feedback says I can use the TACTUS-QUESTION: marker if I am \
                     blocked. I considered whether this needs one.\n\n\
                     TACTUS-QUESTION: should cursors be opaque or signed?";
        assert_eq!(
            worker_question(Some(reply)).as_deref(),
            Some("should cursors be opaque or signed?"),
            "last marker wins, matching the prompt and review.rs's verdict rule"
        );
    }

    #[test]
    fn an_outage_is_never_reclassified_as_a_question() {
        // `detail` carries the agent's partial output on every failure path,
        // and that output routinely quotes the prompt. Reading the marker
        // before the status would turn a rate limit into a parked question —
        // silently defeating "RateLimited defers rather than burning an
        // attempt", and losing the timeout's transcript-tail feedback.
        let quoting = "I will end with the TACTUS-QUESTION: marker if I get stuck.";
        let output = crate::agent::ProcessOutput {
            stdout: String::new(),
            stderr: String::new(),
            code: Some(1),
            timed_out: false,
            duration: Duration::ZERO,
        };
        for (status, expected) in [
            (OutcomeStatus::RateLimited, FailureKind::RateLimited),
            (OutcomeStatus::Timeout, FailureKind::Timeout),
            (OutcomeStatus::AgentError, FailureKind::AgentError),
        ] {
            let outcome =
                fake_outcome(status, Some(quoting.to_owned()), "s0", None, Duration::ZERO);
            let failure = evaluate_outcome(&outcome, &output).expect("still a failure");
            assert_eq!(failure.kind, expected, "{status:?} must keep its own kind");
        }

        // A genuine question on a completed run still parks the task.
        let mut asked = fake_outcome(
            OutcomeStatus::Completed,
            Some("TACTUS-QUESTION: opaque or signed?".to_owned()),
            "s0",
            None,
            Duration::ZERO,
        );
        asked.diff = "diff --git a/x b/x\n+x\n".to_owned();
        assert_eq!(
            evaluate_outcome(&asked, &output).expect("parks").kind,
            FailureKind::NeedsHuman
        );
    }

    #[test]
    fn a_halted_run_stops_asking_and_keeps_naming_the_real_cause() {
        // t1 parks on a question, t2 fails terminally under the default halt
        // policy. Asking about t1 afterwards spends the operator's attention
        // on an answer no attempt can consume, and a decline would relabel
        // `halted_at` with t1 — sending triage at the wrong task.
        let repo = temp_engine_repo("haltpark");
        seed(
            &repo,
            "## Asks a question\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Exhausts its chain\n<!-- tactus: id=t2 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // t1 asks and parks; t2 changes nothing and parks on chain exhaustion.
        let source = source(
            vec![Effect::AskQuestion, Effect::NoEdit],
            vec![ReviewBehavior::Pass],
        );
        // Declining t1 fails it, which halts the run under the default policy.
        // The second answer must never be consumed: t2's question cannot be
        // asked once nothing can act on the reply.
        let answers = ScriptedAnswers::new(vec![
            Answer::Declined,
            Answer::Answered {
                text: "this answer must never be used".to_owned(),
            },
        ]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        assert!(
            matches!(
                task(&report, "t1").status,
                TaskRunStatus::Failed {
                    kind: FailureKind::Declined,
                    ..
                }
            ),
            "the decline is what halts the run: {report:?}"
        );
        assert_eq!(
            report.halted_at.as_deref(),
            Some("t1"),
            "halted_at names the task that actually caused the halt"
        );
        // The distinguishing assertion. Unguarded, t2's question would be
        // asked, answered, and flipped back to Pending — where `next_ready`
        // refuses it because the run has halted, so it would surface as
        // `Skipped` with the operator's answer sitting unused on disk.
        assert!(
            matches!(task(&report, "t2").status, TaskRunStatus::Parked { .. }),
            "t2 is never asked after the halt, so it stays parked rather than \
             silently consuming an answer: {report:?}"
        );
        let t2_question = report
            .questions
            .iter()
            .find(|q| q.question.affected_tasks.iter().any(|t| t.as_str() == "t2"))
            .expect("t2 raised a question");
        assert!(
            t2_question.is_open(),
            "left open on disk for a later resume (§15)"
        );
    }

    #[test]
    fn unreported_cost_stays_unreported_rather_than_zero() {
        assert_eq!(sum_opt([None, None].into_iter()), None);
        assert_eq!(
            sum_opt([Some(0.01), None, Some(0.02)].into_iter()),
            Some(0.03)
        );
    }
}
