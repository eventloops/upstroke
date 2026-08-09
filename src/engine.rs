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
//! Every transition here is an event (invariant 4). The engine never mutates
//! run state directly: it appends to `events.jsonl` and folds the event back in
//! through [`RunState::apply`], the same function `resume` and `status` use to
//! rebuild state from the file. A live run and a replay of its own log
//! therefore cannot disagree — there is no second path for them to disagree
//! along. `report.json` is written from that state as a projection for humans;
//! nothing ever reads it back.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::{self, AgentAdapter, Caps, TaskRun, proc};
use crate::config::OnTaskFailure;
use crate::error::TactusError;
use crate::events::{
    self, ChainSummary, EventBody, EventLog, Feedback, Progress, RunState, TaskState,
};
use crate::gates::{self, ShellGate};
use crate::interaction::{
    self, AnswerSource, InteractionMode, Notifier, QuestionRecord, RealSleeper, Sleeper,
};
use crate::ir::{
    Answer, Outcome, OutcomeStatus, PermissionMode, Plan, Question, QuestionId, QuestionKind, Task,
    TaskKind, WorkerProfile,
};
use crate::ladder::{self, LadderPolicy, LadderState, Next};
use crate::review;
use crate::rundir::{self, RunLock, RunPaths};
use crate::ulid;
use crate::util;
use crate::validate::{self, Analysis, ValidateOptions};
use crate::workspace::Workspace;

pub use crate::events::{AttemptRecord, FailureRecord};
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
    /// Where the agent-authored half of the run directory goes (§15 split).
    /// `None` takes `~/.tactus`; tests point it at a scratch directory so they
    /// never touch the real one.
    pub private_root: Option<PathBuf>,
    /// Override `[interaction] wait_on_block_secs` — how long a detached
    /// interactive run waits at a hard block. `None` takes the config's.
    pub wait_on_block: Option<Duration>,
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
            private_root: None,
            wait_on_block: None,
        }
    }

    fn paths(&self, run_id: &str) -> RunPaths {
        match &self.private_root {
            Some(root) => RunPaths::with_private_root(&self.repo_root, run_id, root),
            None => RunPaths::new(&self.repo_root, run_id),
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

/// Everything `run` and `resume` both establish before an agent is spawned.
///
/// Shared so the two cannot drift: §15 requires a resume to re-probe agents
/// and re-check gates, and the surest way to guarantee it performs the same
/// checks as a fresh run is for there to be one function that performs them.
struct Preflight {
    analysis: Analysis,
    caps: BTreeMap<String, Caps>,
    review_binding: Option<(String, String)>,
    gate_cmds: Vec<String>,
    warnings: Vec<String>,
    mode: InteractionMode,
    notifiers: Vec<&'static dyn Notifier>,
}

fn preflight(opts: &RunOptions, harness: &Harness<'_>) -> Result<Preflight, TactusError> {
    // §14: plan parses cycle-free, config loads, chains resolve.
    let analysis = validate::analyze(&ValidateOptions {
        plan_path: opts.plan_path.clone(),
        config_path: opts.config_path.clone(),
        config_root: opts.repo_root.clone(),
        pools_path: opts.pools_path.clone(),
    })?;

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

    Ok(Preflight {
        analysis,
        caps,
        review_binding,
        gate_cmds,
        warnings,
        mode,
        notifiers,
    })
}

pub fn run_harness(opts: &RunOptions, harness: &Harness<'_>) -> Result<RunReport, TactusError> {
    run_harness_inner(opts, harness).map(|(report, _)| report)
}

/// Also hands back the state the run ended with — its own fold of its own log.
///
/// Only tests use the second half, to hold the live fold and a replay of the
/// same file side by side. Nothing in the engine reads state back.
fn run_harness_inner(
    opts: &RunOptions,
    harness: &Harness<'_>,
) -> Result<(RunReport, RunState), TactusError> {
    let Preflight {
        analysis,
        caps,
        review_binding,
        gate_cmds,
        mut warnings,
        mode,
        notifiers,
    } = preflight(opts, harness)?;

    let workspace = Workspace::open(&opts.repo_root)?;
    workspace.ensure_run_exclusions()?;
    if !workspace.is_clean()? {
        return Err(TactusError::Git {
            message: "working tree is not clean; commit or stash first (the engine refuses \
                      dirty trees)"
                .to_owned(),
        });
    }
    let base_sha = workspace.head_sha_full()?;
    let wait_on_block = opts.wait_on_block;

    let run_id = ulid::ulid();
    let branch = format!("tactus/run-{run_id}");
    let paths = opts.paths(&run_id);
    paths.create()?;
    // Held for the whole run, released by the OS if this process dies — so a
    // crash leaves nothing for `resume` to clear by hand.
    let _lock = RunLock::acquire(&paths)?;
    util::write_json(&paths.plan_json(), &analysis.plan)?;

    workspace.create_branch(&branch)?;

    let started = events::RunStarted {
        schema: events::SCHEMA_VERSION,
        tactus_version: env!("CARGO_PKG_VERSION").to_owned(),
        run_id: run_id.clone(),
        branch: branch.clone(),
        base_sha,
        plan_path: repo_relative(&opts.repo_root, &opts.plan_path),
        config_path: opts
            .config_path
            .as_ref()
            .map(|path| repo_relative(&opts.repo_root, path)),
        plan_hash: analysis.plan.source.hash.clone(),
        private_dir: paths.private.to_string_lossy().into_owned(),
        gates: analysis.gates.iter().map(|g| g.name.clone()).collect(),
        gates_from_config: analysis.gates_from_config,
        interaction_mode: mode.to_string(),
        chains: chain_summaries(&analysis),
    };

    let sleeper = harness.sleeper.unwrap_or(&RealSleeper);
    let default_answers = interaction::answers_for(
        mode,
        paths.answers(),
        wait_on_block.unwrap_or(analysis.config.wait_on_block),
        sleeper,
    );
    let log = EventLog::open(&paths.events(), &mut warnings)?;
    let mut run = Run {
        state: RunState::new(
            analysis
                .plan
                .tasks
                .iter()
                .map(|task| task.id.to_string())
                .collect(),
        ),
        analysis: &analysis,
        workspace: &workspace,
        paths,
        log,
        gate_cmds,
        adapters: harness.adapters,
        answers: harness.answers.unwrap_or(default_answers.as_ref()),
        notifiers,
        sleeper,
        caps,
        review_binding,
        attempt_timeout: opts.attempt_timeout,
        defer_backoff: opts.defer_backoff,
        max_defers: opts.max_defers,
        on_task_failure: analysis.config.on_task_failure,
        run_id,
        branch,
        warnings,
        unanswerable: Vec::new(),
    };
    run.emit(EventBody::RunStarted {
        data: Box::new(started),
    })?;
    let report = run.drain_and_report()?;
    Ok((report, run.state.clone()))
}

/// A path as the run record should carry it: relative to the repo root where
/// possible, so the record survives the repository being moved or cloned
/// somewhere else before a resume.
fn repo_relative(repo_root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// The resolved chain per task, as it stood at this moment.
fn chain_summaries(analysis: &Analysis) -> Vec<ChainSummary> {
    analysis
        .plan
        .tasks
        .iter()
        .zip(&analysis.chains)
        .map(|(task, chain)| ChainSummary {
            task: task.id.to_string(),
            tiers: chain.rungs.iter().map(|rung| rung.tier).collect(),
            attempts_per: chain.attempts_per,
        })
        .collect()
}

/// What to continue, and what may be overridden while continuing it.
#[derive(Debug, Clone)]
pub struct ResumeOptions {
    /// Run id, or any unambiguous prefix of one.
    pub run_id: String,
    pub repo_root: PathBuf,
    /// `None` takes the config the run recorded.
    pub config_path: Option<PathBuf>,
    pub pools_path: Option<PathBuf>,
    pub interaction: Option<InteractionMode>,
    pub attempt_timeout: Duration,
    pub defer_backoff: Duration,
    pub max_defers: u32,
    pub private_root: Option<PathBuf>,
    pub wait_on_block: Option<Duration>,
}

impl ResumeOptions {
    pub fn new(run_id: String, repo_root: PathBuf) -> Self {
        Self {
            run_id,
            repo_root,
            config_path: None,
            pools_path: None,
            interaction: None,
            attempt_timeout: DEFAULT_ATTEMPT_TIMEOUT,
            defer_backoff: interaction::DEFAULT_DEFER_BACKOFF,
            max_defers: DEFAULT_MAX_DEFERS,
            private_root: None,
            wait_on_block: None,
        }
    }
}

pub fn resume(opts: &ResumeOptions) -> Result<RunReport, TactusError> {
    resume_with(opts, &BuiltinAdapters)
}

pub fn resume_with(
    opts: &ResumeOptions,
    adapters: &dyn AdapterSource,
) -> Result<RunReport, TactusError> {
    resume_harness(opts, &Harness::new(adapters))
}

/// §15: replay, verify the run branch still matches the record, re-probe, and
/// continue — parked questions intact.
///
/// Every refusal below exists because continuing would produce a *wrong*
/// result rather than merely an awkward one, and each says which of the three
/// things moved — the run, the plan, or the branch — because that is what
/// decides what the operator does next.
pub fn resume_harness(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
) -> Result<RunReport, TactusError> {
    resume_harness_inner(opts, harness).map(|(report, _)| report)
}

fn resume_harness_inner(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
) -> Result<(RunReport, RunState), TactusError> {
    let run_id = rundir::resolve_run_id(&opts.repo_root, &opts.run_id)?;
    let public = rundir::public_dir(&opts.repo_root, &run_id);
    let refuse = |message: String| TactusError::Resume {
        run_id: run_id.clone(),
        message,
    };

    // Claimed before anything is read, so two resumes cannot race each other
    // into the same branch.
    let probe_paths = RunPaths::from_parts(public.clone(), public.clone());
    let _lock = RunLock::acquire(&probe_paths)?;

    let mut warnings = Vec::new();
    let events_path = public.join("events.jsonl");
    let events = events::read_all(&events_path, &mut warnings)?;
    let started = events::started_of(&events, &events_path)?.clone();

    // The run knows its own plan and config; the CLI may override the config
    // but never the plan, which is frozen (§5).
    let mut run_opts = RunOptions::new(
        opts.repo_root.join(&started.plan_path),
        opts.repo_root.clone(),
    );
    run_opts.config_path = opts
        .config_path
        .clone()
        .or_else(|| started.config_path.as_ref().map(|p| opts.repo_root.join(p)));
    run_opts.pools_path = opts.pools_path.clone();
    run_opts.interaction = opts.interaction;
    run_opts.attempt_timeout = opts.attempt_timeout;
    run_opts.defer_backoff = opts.defer_backoff;
    run_opts.max_defers = opts.max_defers;
    run_opts.private_root = opts.private_root.clone();
    run_opts.wait_on_block = opts.wait_on_block;
    let wait_on_block = opts.wait_on_block;

    // Re-probes agents and re-resolves gates, exactly as a fresh run does.
    let Preflight {
        analysis,
        caps,
        review_binding,
        gate_cmds,
        warnings: preflight_warnings,
        mode,
        notifiers,
    } = preflight(&run_opts, harness)?;
    warnings.extend(preflight_warnings);

    // The plan is frozen. A different hash means the file moved under the run,
    // so every task index in the log — which is what `Progress` is keyed by —
    // may now mean a different task.
    if analysis.plan.source.hash != started.plan_hash {
        return Err(refuse(format!(
            "the plan at {} has changed since this run froze it (recorded {}, now {}). Task \
             progress is recorded per task, so replaying it against a different plan would \
             attribute work to the wrong tasks. Restore the plan, or start a new run.",
            run_opts.plan_path.display(),
            started.plan_hash,
            analysis.plan.source.hash
        )));
    }

    // Chains moved means config moved. `Progress.rung` is an index into the
    // chain, so re-resolving a different one silently points a task at a
    // different tier than the one it actually reached.
    let chains = chain_summaries(&analysis);
    if chains != started.chains {
        let moved: Vec<String> = chains
            .iter()
            .zip(&started.chains)
            .filter(|(now, then)| now != then)
            .map(|(now, then)| {
                format!(
                    "`{}` ran on [{}] and would now run on [{}]",
                    now.task,
                    render_tiers(then),
                    render_tiers(now)
                )
            })
            .collect();
        return Err(refuse(format!(
            "routing has changed since this run started, so a recorded rung would now mean a \
             different tier: {}. Restore the config it ran with, or start a new run.",
            moved.join("; ")
        )));
    }

    let task_ids: Vec<String> = analysis
        .plan
        .tasks
        .iter()
        .map(|task| task.id.to_string())
        .collect();
    let replayed = events::replay(events, task_ids, &events_path)?;

    match replayed.state.finished.as_ref().map(|f| &f.outcome) {
        Some(events::RunOutcome::Complete) => {
            return Err(refuse(
                "this run already completed; there is nothing left to continue".to_owned(),
            ));
        }
        Some(events::RunOutcome::Halted) => {
            return Err(refuse(format!(
                "this run halted at `{}` under `on_task_failure = \"halt\"`. Nothing can run \
                 while it is halted — fix what failed and start a new run.",
                replayed.state.halted_at.as_deref().unwrap_or("?")
            )));
        }
        // Ended parked, or never ended at all — both are exactly what resume
        // is for.
        Some(events::RunOutcome::Parked) | None => {}
    }

    let workspace = Workspace::open(&opts.repo_root)?;
    workspace.ensure_run_exclusions()?;
    if !workspace.branch_exists(&started.branch)? {
        return Err(refuse(format!(
            "the run branch `{}` no longer exists. Its commits are what this run's record \
             refers to; without it there is nothing to continue onto.",
            started.branch
        )));
    }
    if workspace.current_branch()? != started.branch {
        if !workspace.is_clean()? {
            return Err(refuse(format!(
                "you have uncommitted changes and are not on `{}`. Commit or stash them, then \
                 resume — switching branches over them would lose work that is not this run's \
                 to discard.",
                started.branch
            )));
        }
        workspace.switch_branch(&started.branch)?;
    }

    // §15's check, before anything is discarded: if HEAD moved, refusing has
    // to leave the operator's tree exactly as they left it.
    let expected_head = last_committed_sha(&replayed.events).unwrap_or(started.base_sha.clone());
    let head = workspace.head_sha_full()?;
    if head != expected_head {
        return Err(refuse(format!(
            "`{}` is at {head}, but this run's record ends at {expected_head}. Something \
             committed, reset, or rebased the branch after the run stopped, so replaying the \
             log would describe work that is no longer what is on the branch. Move the branch \
             back to {expected_head}, or start a new run.",
            started.branch
        )));
    }

    // Crash residue: a dead agent's half-written edits. §14 rolls a failed
    // attempt back to the last commit, and an attempt that never reported is
    // no different — the session that would have explained these edits is
    // gone, so nothing can verify them.
    let discarded = workspace.uncommitted_summary()?;
    if !discarded.is_empty() {
        warnings.push(format!(
            "discarded {} uncommitted path(s) left by the interrupted run: {}",
            discarded.len(),
            discarded.join(", ")
        ));
        workspace.discard_uncommitted()?;
    }

    let paths = run_opts.paths(&run_id);
    paths.create()?;
    let sleeper = harness.sleeper.unwrap_or(&RealSleeper);
    let default_answers = interaction::answers_for(
        mode,
        paths.answers(),
        wait_on_block.unwrap_or(analysis.config.wait_on_block),
        sleeper,
    );
    let log = EventLog::open(&paths.events(), &mut warnings)?;
    let mut run = Run {
        state: replayed.state,
        analysis: &analysis,
        workspace: &workspace,
        paths,
        log,
        gate_cmds,
        adapters: harness.adapters,
        answers: harness.answers.unwrap_or(default_answers.as_ref()),
        notifiers,
        sleeper,
        caps,
        review_binding,
        attempt_timeout: opts.attempt_timeout,
        defer_backoff: opts.defer_backoff,
        max_defers: opts.max_defers,
        on_task_failure: analysis.config.on_task_failure,
        run_id,
        branch: started.branch.clone(),
        warnings,
        unanswerable: Vec::new(),
    };
    // Write the `attempt_finished` the dead process never got to.
    //
    // Recorded rather than settled in memory, because a settlement only a
    // reader performs is lost the moment someone else replays the log: the
    // ledger line vanishes and, worse, the rung's refunded allowance vanishes
    // with it, so a later resume would think the attempt had been spent.
    let interrupted = run.state.interrupted_attempts();
    for attempt in &interrupted {
        run.emit(attempt.event())?;
    }

    // Applying this is what drops every session and wakes deferred work — the
    // §14 pairing, enforced by the same fold a replay uses rather than by this
    // function remembering to do it.
    run.emit(EventBody::RunResumed {
        data: events::RunResumed {
            head_sha: head,
            interrupted_attempts: u32::try_from(interrupted.len()).unwrap_or(u32::MAX),
            discarded,
        },
    })?;
    let report = run.drain_and_report()?;
    Ok((report, run.state.clone()))
}

fn render_tiers(chain: &ChainSummary) -> String {
    chain
        .tiers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" → ")
}

/// The sha the run's record ends at — what HEAD must still be.
fn last_committed_sha(events: &[events::Event]) -> Option<String> {
    events.iter().rev().find_map(|event| match &event.body {
        EventBody::TaskCommitted { data, .. } => Some(data.sha.clone()),
        _ => None,
    })
}

struct Run<'a> {
    analysis: &'a Analysis,
    workspace: &'a Workspace,
    paths: RunPaths,
    /// The append-only record. Every mutation below goes through
    /// [`Run::emit`], never straight at `state`.
    log: EventLog,
    /// Derived state — the same fold `resume` and `status` build from the log.
    state: RunState,
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
    run_id: String,
    branch: String,
    warnings: Vec<String>,
    /// Questions no channel could reach a human for. Never asked twice — that
    /// is what stops a hard block spinning.
    ///
    /// Deliberately *not* replayed: it records that a channel was unreachable
    /// in this process, not something true about the run. A question nobody
    /// could answer at 2am is exactly the one the operator answers when they
    /// come back, so a resume has to be free to ask it again.
    unanswerable: Vec<QuestionId>,
}

impl Run<'_> {
    /// Append an event and fold it in.
    ///
    /// The only way run state changes. Everything below emits; nothing reaches
    /// past this into `state`, which is what makes a live run and a replay of
    /// its own log the same computation rather than two that agree by
    /// inspection.
    fn emit(&mut self, body: EventBody) -> Result<(), TactusError> {
        let event = self.log.append(body)?;
        self.state.apply(&event);
        Ok(())
    }

    /// Drain, settle, and report.
    fn drain_and_report(&mut self) -> Result<RunReport, TactusError> {
        if let Err(error) = self.drain() {
            // The log already holds everything that happened, including the
            // attempt this died inside — that is what `resume` reads. The
            // report beside it is a courtesy for whoever opens the directory
            // next, and failing to write it must not mask the error that
            // actually stopped the run.
            let partial = self.finish();
            let _ = util::write_json(&self.paths.report_json(), &partial);
            return Err(error);
        }
        let report = self.finish();
        let committed = report
            .tasks
            .iter()
            .filter(|task| matches!(task.status, TaskRunStatus::Committed { .. }))
            .count();
        self.emit(EventBody::RunFinished {
            data: events::RunFinished {
                outcome: match report.outcome() {
                    RunOutcome::Complete => events::RunOutcome::Complete,
                    RunOutcome::Parked => events::RunOutcome::Parked,
                    RunOutcome::Halted => events::RunOutcome::Halted,
                },
                halted_at: report.halted_at.clone(),
                committed: u32::try_from(committed).unwrap_or(u32::MAX),
                parked: u32::try_from(report.parked_tasks().len()).unwrap_or(u32::MAX),
            },
        })?;
        util::write_json(&self.paths.report_json(), &report)?;
        Ok(report)
    }

    /// Drain the graph (§14, §12).
    ///
    /// The four branches are the whole interaction model: pick up answers that
    /// arrived from somewhere else; run what is ready; if only deferred work is
    /// left, wait for the pool rather than burning attempts against it; and
    /// only when none of those is possible — the precise definition of a hard
    /// block — ask a human.
    ///
    /// **Why this terminates.** Every branch consumes something finite and
    /// nothing replenishes any of them:
    ///
    /// - the answer sweep fires only for an *open* question and closes it, and
    ///   questions are created only by `step_task`;
    /// - `step_task` moves its task out of `Pending`, and the only routes back
    ///   are a deferral — bounded by `max_defers`, after which the ladder parks
    ///   the task instead — or an answer, which closed a question to get there;
    /// - the wait branch requires a `Deferred` task, which only a deferral
    ///   creates;
    /// - the ask branch either closes a question or adds it to `unanswerable`,
    ///   which is only ever appended to and is checked before asking.
    ///
    /// So no cycle exists that does not spend an attempt, a deferral, or a
    /// question. `an_exhausted_pool_and_a_silent_operator_still_terminate`
    /// holds it to that against an adapter that never succeeds and an operator
    /// who never replies.
    fn drain(&mut self) -> Result<(), TactusError> {
        let mut defer_round = 0u32;
        loop {
            // Invariant 6 in its most useful form: an answer that arrives while
            // other work is still running un-parks its task there and then,
            // rather than waiting for the run to have nothing else to do.
            if self.sweep_answers()? {
                continue;
            }
            if let Some(index) = self.next_ready() {
                let deferred = self.step_task(index)?;
                if !deferred {
                    defer_round = 0;
                }
                continue;
            }
            if self.state.states.contains(&TaskState::Deferred) && self.state.halted_at.is_none() {
                let waited = interaction::defer_backoff(self.defer_backoff, defer_round);
                self.sleeper.sleep(waited);
                defer_round = defer_round.saturating_add(1);
                self.emit(EventBody::DeferWaitElapsed {
                    data: events::DeferWaitElapsed {
                        waited,
                        round: defer_round,
                    },
                })?;
                continue;
            }
            // Guarded like the other branches: once the run has halted, no
            // answer can reach an attempt this session, so asking would spend
            // a human's attention on a decision the scheduler cannot act on —
            // and a decline would relabel `halted_at` with a task that was not
            // the cause. The questions stay open on disk for a resume (§15).
            if self.state.halted_at.is_none() && self.resolve_one_question()? {
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
        if self.state.halted_at.is_some() {
            return None;
        }
        let tasks = &self.analysis.plan.tasks;
        (0..tasks.len()).find(|&i| {
            matches!(self.state.states[i], TaskState::Pending)
                && tasks[i].depends_on.iter().all(|dep| {
                    tasks
                        .iter()
                        .position(|t| t.id == *dep)
                        // An unknown dependency cannot exist on a validated
                        // plan; treating it as satisfied keeps the scheduler
                        // total rather than deadlocking.
                        .is_none_or(|j| matches!(self.state.states[j], TaskState::Done(_)))
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
        // Copied out of `self` so they carry the run's lifetime rather than
        // this method's `&mut self` borrow.
        let analysis = self.analysis;
        let adapters = self.adapters;
        let workspace = self.workspace;
        let task = &analysis.plan.tasks[index];
        let task_id = task.id.to_string();
        let chain = &analysis.chains[index];
        let policy = LadderPolicy {
            attempts_per: chain.attempts_per,
            rungs: chain.rungs.len(),
            max_defers: self.max_defers,
        };
        let stem = format!("{index:02}-{}", util::filename_component(task.id.as_str()));

        loop {
            let rung_index = self.state.progress[index].rung;
            let Some(rung) = chain.rungs.get(rung_index) else {
                self.fail_task(
                    index,
                    FailureKind::NoChain,
                    "resolved chain has no rung to run on".to_owned(),
                )?;
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

            let attempt = self.state.progress[index].attempts + 1;
            let resume = self.state.progress[index]
                .resume_next
                .then(|| self.state.progress[index].session.clone())
                .flatten();

            // Recorded *before* the agent is spawned, so a process that dies
            // mid-attempt leaves an `attempt_started` with no
            // `attempt_finished`. That dangling pair is precisely what tells a
            // later replay an attempt was interrupted (§19's crash row) — the
            // engine cannot write a record of its own death afterwards.
            let rung_number = u32::try_from(rung_index).unwrap_or(u32::MAX);
            self.emit(EventBody::AttemptStarted {
                task: task_id.clone(),
                attempt,
                rung: rung_number,
                profile: profile.name.clone(),
                data: events::AttemptStarted {
                    tier: rung.tier.to_string(),
                    agent: profile.agent.clone(),
                    model: profile.model.clone(),
                    resume_session: resume.clone(),
                },
            })?;

            // Scoped so every borrow the attempt takes on `self` is released
            // before the ladder updates this task's progress below.
            let result = {
                let retry = (attempt > 1).then(|| RetryBrief {
                    resumed: resume.is_some(),
                    // Owned: the ladder appends to this task's feedback the
                    // moment the attempt returns, and one clone per attempt
                    // costs less than threading that borrow through.
                    feedback: self.state.progress[index].feedback.clone(),
                });
                let attempt_cx = AttemptCx {
                    task,
                    profile: profile.clone(),
                    adapter,
                    attempt,
                    stem: stem.clone(),
                    paths: &self.paths,
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

            self.emit(EventBody::AttemptFinished {
                task: task_id.clone(),
                attempt,
                rung: rung_number,
                profile: profile.name.clone(),
                data: Box::new(AttemptRecord {
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
                }),
            })?;

            let Some(failure) = result.failure else {
                let message = format!("[tactus] {}: {}", task.id, task.title);
                self.workspace.commit(&message)?;
                // The full sha, not `commit`'s abbreviated one: `resume`
                // compares this against HEAD, and abbreviation length varies
                // with `core.abbrev` and the repo's object count.
                let full_sha = self.workspace.head_sha_full()?;
                // Scrub gate side-effects (build artifacts, lockfile churn) so
                // they cannot leak into the next task's captured diff; the
                // commit recorded exactly the verified staged set.
                self.workspace.discard_uncommitted()?;
                self.emit(EventBody::TaskCommitted {
                    task: task_id.clone(),
                    data: events::TaskCommitted {
                        sha: full_sha,
                        message,
                    },
                })?;
                return Ok(false);
            };

            let resumable = self.state.progress[index].session.is_some()
                && self
                    .caps
                    .get(&profile.agent)
                    .is_some_and(|c| c.session_resume);
            let state = LadderState {
                rung: self.state.progress[index].rung,
                attempts_on_rung: self.state.progress[index].attempts_on_rung,
                defers: self.state.progress[index].defers,
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
                    self.emit(EventBody::LadderRetry {
                        task: task_id.clone(),
                        attempt,
                        rung: rung_number,
                        data: events::LadderRetry {
                            resume,
                            tier: rung.tier.to_string(),
                            summary: failure.reason.clone(),
                            detail: failure.feedback.clone(),
                        },
                    })?;
                }
                Next::Escalate => {
                    self.emit(EventBody::LadderEscalated {
                        task: task_id.clone(),
                        attempt,
                        rung: rung_number,
                        data: events::LadderEscalated {
                            to_rung: rung_number.saturating_add(1),
                            tier: rung.tier.to_string(),
                            summary: failure.reason.clone(),
                            detail: failure.feedback.clone(),
                        },
                    })?;
                }
                Next::Defer => {
                    // No attempt was spent on the work itself, so the event's
                    // fold gives the rung its allowance back (§19).
                    self.emit(EventBody::TaskDeferred {
                        task: task_id.clone(),
                        data: events::TaskDeferred {
                            reason: failure.reason.clone(),
                            defers: self.state.progress[index].defers.saturating_add(1),
                        },
                    })?;
                    return Ok(true);
                }
                Next::AskHuman(kind) => {
                    let question = self.raise_question(index, kind, &failure)?;
                    self.emit(EventBody::TaskParked {
                        task: task_id.clone(),
                        data: events::TaskParked {
                            question: question.to_string(),
                            // Nobody judged the code, so the attempt is not
                            // spent (§12).
                            refund_attempt: kind == QuestionKind::Clarify,
                        },
                    })?;
                    return Ok(false);
                }
                Next::Fail => {
                    self.fail_task(index, failure.kind, failure.reason.clone())?;
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

    fn fail_task(
        &mut self,
        index: usize,
        kind: FailureKind,
        reason: String,
    ) -> Result<(), TactusError> {
        let task = self.analysis.plan.tasks[index].id.to_string();
        // The halt policy is resolved here and recorded, not re-derived on
        // replay: a `tactus.toml` edited between a run and its resume must not
        // rewrite which task the report blames for stopping.
        let halts_run = self.on_task_failure == OnTaskFailure::Halt;
        self.emit(EventBody::TaskFailed {
            task,
            data: events::TaskFailed {
                kind,
                reason,
                halts_run,
            },
        })
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
            context: question_context(task, kind, failure, &self.state.progress[index]),
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
        // The payload lands on disk before the event, so a reader that sees
        // `question_raised` can always open the file it names.
        interaction::write_question(
            &self.paths.questions(),
            &QuestionRecord::open(question.clone()),
        )?;
        self.emit(EventBody::QuestionRaised {
            task: self.analysis.plan.tasks[index].id.to_string(),
            data: Box::new(events::QuestionRaised { question }),
        })?;
        Ok(id)
    }

    /// Ingest answers left by `tactus answer` in another process.
    ///
    /// Returns whether anything changed. This is what makes the answer command
    /// useful while a run is alive rather than only between runs: an operator
    /// answering from a phone at 2am un-parks the task on the next scheduler
    /// turn, with no resume needed.
    fn sweep_answers(&mut self) -> Result<bool, TactusError> {
        let open: Vec<QuestionId> = self
            .state
            .open_questions()
            .iter()
            .map(|record| record.question.id.clone())
            .collect();
        if open.is_empty() {
            return Ok(false);
        }
        let dir = self.paths.answers();
        let mut changed = false;
        for id in open {
            let Some(answer) = interaction::read_answer(&dir, &id)? else {
                continue;
            };
            self.ingest_answer(&id, answer, "answer-file")?;
            changed = true;
        }
        Ok(changed)
    }

    /// Record an answer and let it take effect.
    ///
    /// One path for every channel — a terminal reply, a file written by
    /// `tactus answer`, or an answer picked up on resume — so what an answer
    /// *does* cannot depend on where it came from.
    fn ingest_answer(
        &mut self,
        id: &QuestionId,
        answer: Answer,
        via: &str,
    ) -> Result<(), TactusError> {
        let Some(record) = self
            .state
            .questions
            .iter()
            .find(|record| record.question.id == *id)
        else {
            return Ok(());
        };
        if !record.is_open() || answer == Answer::Unanswered {
            return Ok(());
        }
        let context = record.question.context.clone();
        let affected = record.question.affected_tasks.clone();

        self.emit(EventBody::QuestionAnswered {
            data: events::QuestionAnswered {
                question: id.clone(),
                answer: answer.clone(),
                via: via.to_owned(),
            },
        })?;

        // §5: a question that reached a human at runtime is, by definition, a
        // design-phase defect — logged as one so the accumulated defects can
        // become review material for the designer prompt.
        self.emit(EventBody::DesignDefect {
            data: events::DesignDefect {
                question: id.clone(),
                context: util::head(context.trim(), 600),
                answer: match &answer {
                    Answer::Answered { text } => text.clone(),
                    _ => "declined".to_owned(),
                },
            },
        })?;

        // A decline is the task's failure, not the question's, so it goes
        // through the one place that owns the halt policy. `apply` leaves a
        // declined task parked precisely so this can still see who was waiting.
        if answer == Answer::Declined {
            for task_id in affected {
                let Some(index) = self.state.index_of(task_id.as_str()) else {
                    continue;
                };
                if !matches!(&self.state.states[index], TaskState::AwaitingInput(q) if q == id) {
                    continue;
                }
                let reason = format!(
                    "declined at the human rung: {}",
                    last_reason(&self.state.progress[index])
                );
                self.fail_task(index, FailureKind::Declined, reason)?;
            }
        }

        // Rewrite the payload so a late reader — a UI, or someone opening the
        // directory tomorrow — sees the whole exchange, not just the question.
        if let Some(record) = self
            .state
            .questions
            .iter()
            .find(|record| record.question.id == *id)
        {
            interaction::write_question(&self.paths.questions(), record)?;
        }
        Ok(())
    }

    /// Ask about the oldest open question. Returns whether anything changed.
    ///
    /// This runs only at a hard block, and each question is asked at most
    /// once: an `Unanswered` result marks it unreachable rather than looping
    /// back to a channel that already said nobody is there.
    fn resolve_one_question(&mut self) -> Result<bool, TactusError> {
        let Some(position) = self.state.questions.iter().position(|record| {
            record.is_open() && !self.unanswerable.contains(&record.question.id)
        }) else {
            return Ok(false);
        };
        let question = self.state.questions[position].question.clone();
        let answer = self.answers.resolve(&question)?;

        // The channel may have been waiting on the very file the sweep reads;
        // ingesting first means an answer that arrived during the wait is
        // applied once, by whichever path saw it, and never twice.
        if self.sweep_answers()? {
            return Ok(true);
        }
        if answer == Answer::Unanswered {
            // §12 CI mode: the task stays parked and the run's exit status
            // reports it. Not a failure — nobody rejected anything.
            self.unanswerable.push(question.id);
            return Ok(true);
        }
        self.ingest_answer(&question.id, answer, self.answers.id())?;
        Ok(true)
    }

    /// Settle every task that never ran, then report.
    fn finish(&self) -> RunReport {
        build_report(
            &self.run_id,
            &self.branch,
            self.analysis.gates.iter().map(|g| g.name.clone()).collect(),
            self.analysis.gates_from_config,
            self.warnings.clone(),
            &self.analysis.plan,
            &self.state,
        )
    }
}

impl RunReport {
    /// Build a report from a replayed log.
    ///
    /// `status` and the `report.json` a run writes go through the same
    /// function, so what an operator sees mid-run and what the file says
    /// afterwards cannot drift into disagreeing.
    pub fn from_state(
        started: &events::RunStarted,
        plan: &Plan,
        state: &RunState,
        warnings: Vec<String>,
    ) -> Self {
        build_report(
            &started.run_id,
            &started.branch,
            started.gates.clone(),
            started.gates_from_config,
            warnings,
            plan,
            state,
        )
    }
}

fn build_report(
    run_id: &str,
    branch: &str,
    gates: Vec<String>,
    gates_from_config: bool,
    warnings: Vec<String>,
    plan: &Plan,
    state: &RunState,
) -> RunReport {
    let settled = settle(plan, &state.states);
    let tasks: Vec<TaskReport> = state
        .order
        .iter()
        .copied()
        // Tasks that never started append in plan order, so the report reads
        // as the run happened and still accounts for everything.
        .chain((0..plan.tasks.len()).filter(|i| !state.order.contains(i)))
        .map(|index| task_report(&plan.tasks[index], &settled[index], &state.progress[index]))
        .collect();
    let total_cost_usd = tasks
        .iter()
        .filter_map(TaskReport::total_cost_usd)
        .sum::<f64>();
    RunReport {
        run_id: run_id.to_owned(),
        branch: branch.to_owned(),
        gates,
        gates_from_config,
        warnings,
        tasks,
        halted_at: state.halted_at.clone(),
        questions: state.questions.clone(),
        total_cost_usd,
    }
}

/// Derive how an ended run's untouched tasks are reported.
///
/// This is a *view*, not state, and deliberately not recorded as events. A
/// task blocked behind an unanswered question has to become runnable again the
/// moment that question is answered — so if `Blocked` were folded in from the
/// log, every resume would have to un-fold it. Deriving it fresh from whatever
/// the log says is true right now means there is nothing to undo.
fn settle(plan: &Plan, states: &[TaskState]) -> Vec<TaskState> {
    let tasks = &plan.tasks;
    let mut settled = states.to_vec();
    // Blocking propagates: a dependent of a blocked task is blocked too.
    // Repeat until stable rather than assuming plan order carries it — a plan
    // may list a dependent before the task it waits on.
    loop {
        let mut changed = false;
        for index in 0..tasks.len() {
            if settled[index] != TaskState::Pending {
                continue;
            }
            let blocker = tasks[index].depends_on.iter().find(|dep| {
                tasks
                    .iter()
                    .position(|t| t.id == **dep)
                    .is_some_and(|j| !matches!(settled[j], TaskState::Done(_)))
            });
            if let Some(blocker) = blocker {
                settled[index] = TaskState::Blocked(blocker.to_string());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Whatever is still Pending was never reached: the run halted.
    for state in &mut settled {
        if *state == TaskState::Pending {
            *state = TaskState::Skipped;
        }
    }
    settled
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
            TaskState::Blocked(by) => TaskRunStatus::Blocked { by: by.clone() },
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
    paths: &'a RunPaths,
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
        &cx.paths.settings(),
        &format!("{}-{}", cx.stem, cx.attempt),
    )?;

    let task_run = TaskRun {
        prompt: materialize_prompt(
            cx.task,
            cx.gate_cmds,
            &cx.paths.artifacts(),
            cx.retry.as_ref(),
        ),
        profile: cx.profile.clone(),
        workspace: workspace.root().to_path_buf(),
        resume_session,
        settings_path,
    };
    let command = cx.adapter.build(&task_run)?;
    let output = proc::run_with_timeout(command, cx.adapter.stdin_payload(&task_run), cx.timeout)?;

    let transcripts = cx.paths.transcripts();
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
        && let Some(gate_failure) =
            gates::run_all(cx.gates, workspace, &cx.paths.gates(), &cx.stem, cx.attempt)?
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
            artifacts: &load_artifacts(&cx.paths.artifacts(), cx.task),
            workspace: workspace.root(),
            settings_dir: &cx.paths.settings(),
            reviews_dir: &cx.paths.reviews(),
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
fn load_artifacts(artifacts_dir: &Path, task: &Task) -> Vec<(String, String)> {
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
            let content = fs::read_to_string(artifact_path(artifacts_dir, &id)).ok()?;
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
    artifacts_dir: &Path,
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
        let path = artifact_path(artifacts_dir, id.as_str());
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
            artifact_path(artifacts_dir, id.as_str()).display()
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
fn artifact_path(artifacts_dir: &Path, id: &str) -> PathBuf {
    artifacts_dir.join(format!("{}.md", util::filename_component(id)))
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

    /// §21's definition-of-done (e): what each task cost, and on what.
    ///
    /// Implementer and reviewer spend stay in separate columns because they
    /// are different models at different tiers — folding them together makes a
    /// cheap rung look expensive to anyone reading the ledger (§13). An
    /// unreported cost prints as `—` rather than `$0.0000`: a ledger that
    /// cannot tell free from unreported is worse than no ledger.
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        let money = |value: Option<f64>| match value {
            Some(amount) => format!("${amount:.4}"),
            None => "—".to_owned(),
        };
        let rows: Vec<[String; 6]> = self
            .tasks
            .iter()
            .map(|task| {
                [
                    task.id.clone(),
                    task.attempts.len().to_string(),
                    if task.trail().is_empty() {
                        "—".to_owned()
                    } else {
                        task.trail()
                    },
                    money(task.cost_usd),
                    money(task.review_cost_usd),
                    money(task.total_cost_usd()),
                ]
            })
            .collect();
        let headers = ["task", "attempts", "trail", "worker", "review", "total"];
        let widths: Vec<usize> = (0..headers.len())
            .map(|column| {
                rows.iter()
                    .map(|row| row[column].chars().count())
                    .chain(std::iter::once(headers[column].chars().count()))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let line = |cells: &[String]| {
            let mut rendered = String::from("  ");
            for (index, cell) in cells.iter().enumerate() {
                let pad = widths[index].saturating_sub(cell.chars().count());
                let _ = write!(rendered, "{cell}{:pad$}", "", pad = pad);
                if index + 1 < cells.len() {
                    rendered.push_str("  ");
                }
            }
            rendered.trim_end().to_owned()
        };

        let _ = writeln!(out, "ledger:");
        let _ = writeln!(out, "{}", line(&headers.map(str::to_owned)));
        for row in &rows {
            let _ = writeln!(out, "{}", line(row));
        }
        let _ = writeln!(
            out,
            "  total ${:.4} (api-equivalent; subscription spend is notional — §13)",
            self.total_cost_usd
        );
        // Pool drain arrives with the capacity engine; saying so beats an
        // empty column that looks like "nothing was spent".
        let _ = writeln!(out, "  per-pool drain: not connected");
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
        /// Kills the whole process partway through the attempt, leaving the
        /// on-disk shape a `kill -9` or a power loss leaves: a dirty working
        /// tree and an `attempt_started` with no `attempt_finished`.
        Exit,
    }

    /// Distinctive so the parent can tell a deliberate death from a panic,
    /// which would also exit non-zero.
    const CRASH_EXIT_CODE: i32 = 42;

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
                    Effect::Exit => {
                        // Half-finished edits first, then die without
                        // unwinding — no destructors, no flush of anything the
                        // engine has not already synced. That is what makes
                        // this a faithful stand-in for a kill rather than a
                        // tidy shutdown, and it happens at a deterministic
                        // point instead of racing a signal.
                        let _ = fs::write(
                            run.workspace.join("agent-output.txt"),
                            "half-written by an agent that never came back\n",
                        );
                        std::process::exit(CRASH_EXIT_CODE);
                    }
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
                // `Exit` never reaches here — `build` ends the process.
                Effect::EditFile
                | Effect::EditTest
                | Effect::NoEdit
                | Effect::AskQuestion
                | Effect::Exit => OutcomeStatus::Completed,
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
        // Tests must never actually wait — not out a rate limit, and not at a
        // hard block either. The test harness has no terminal, so an
        // interactive mode resolves to the waiting answer channel; without a
        // zero budget every parking test would sit out the real one.
        opts.defer_backoff = Duration::ZERO;
        opts.wait_on_block = Some(Duration::ZERO);
        opts.private_root = Some(private_root_for(repo));
        opts
    }

    /// A scratch stand-in for `~/.tactus`, so tests never touch the real one.
    ///
    /// A *sibling* of the repo, never a directory inside it. That is not
    /// tidiness: §14's rollback is `git clean -fd`, which deletes untracked
    /// directories — a private root inside the workspace would have its
    /// transcripts and verdicts destroyed by the first failed attempt. The
    /// same reasoning is why production puts it under the user's home.
    fn private_root_for(repo: &Path) -> PathBuf {
        let name = repo
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "run".to_owned());
        repo.with_file_name(format!("{name}-home"))
    }

    /// Resume options matching [`options`], for the same reasons.
    fn resume_options(repo: &Path, run_id: &str) -> ResumeOptions {
        let mut opts = ResumeOptions::new(run_id.to_owned(), repo.to_path_buf());
        opts.pools_path = Some(
            std::env::temp_dir()
                .join("tactus-engine-missing")
                .join("pools.toml"),
        );
        opts.attempt_timeout = Duration::from_secs(60);
        opts.defer_backoff = Duration::ZERO;
        opts.wait_on_block = Some(Duration::ZERO);
        opts.private_root = Some(private_root_for(repo));
        opts
    }

    /// The paths a test's run wrote to.
    fn paths_of(repo: &Path, run_id: &str) -> RunPaths {
        RunPaths::with_private_root(repo, run_id, &private_root_for(repo))
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
        let settings = paths_of(&repo, &report.run_id).settings();
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

        // §15 split: the file describing an agent's own sandbox is not
        // somewhere that agent can read.
        assert!(
            !settings.starts_with(&repo),
            "settings live outside the workspace: {}",
            settings.display()
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
        let reviews = paths_of(&repo, &report.run_id).reviews();
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
        let gates_dir = paths_of(&repo, &report.run_id).gates();
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

    // ---- step 8: the event log is the state ------------------------------

    /// Fold a run's log the way `status` and `resume` do.
    fn replay_of(repo: &Path, run_id: &str) -> crate::status::RunStatus {
        crate::status::load(repo, Some(run_id)).expect("the run reads back")
    }

    /// One path through the ladder, for the live-equals-replay property.
    struct Scenario {
        name: &'static str,
        config: &'static str,
        effects: Vec<Effect>,
        reviews: Vec<ReviewBehavior>,
        answers: Vec<Answer>,
    }

    impl Scenario {
        fn new(name: &'static str, config: &'static str, effects: Vec<Effect>) -> Self {
            Self {
                name,
                config,
                effects,
                reviews: vec![ReviewBehavior::Pass],
                answers: Vec::new(),
            }
        }

        fn reviewed(mut self, reviews: Vec<ReviewBehavior>) -> Self {
            self.reviews = reviews;
            self
        }

        fn answered(mut self, answers: Vec<Answer>) -> Self {
            self.answers = answers;
            self
        }
    }

    /// The property the whole design rests on: a live run and a replay of its
    /// own log are the same computation, not two that happen to agree.
    ///
    /// Asserted on `RunState` rather than on the report, because the report is
    /// a lossy projection — it drops `feedback`, `resume_next`, `session`, and
    /// the rung a task is standing on, which are exactly the fields a resume
    /// depends on being right.
    fn assert_live_equals_replay(repo: &Path, live: &RunState, report: &RunReport) {
        let replayed = replay_of(repo, &report.run_id);
        assert_eq!(
            &replayed.state, live,
            "replaying the log produced different state than the run that wrote it"
        );
        // Warnings are the one field deliberately excluded. They are
        // diagnostics of the *process* — what this invocation noticed about a
        // missing notifier or a discarded working tree — not facts about the
        // run, so a later reader legitimately has different ones. Anything
        // that genuinely belongs to the run is an event instead (a discarded
        // tree, for instance, rides on `run_resumed`).
        let strip = |report: &RunReport| {
            let mut value = serde_json::to_value(report).expect("serialize");
            if let Some(object) = value.as_object_mut() {
                object.remove("warnings");
            }
            value
        };
        assert_eq!(
            strip(&replayed.report()),
            strip(report),
            "the report derived from the log differs from the one the run wrote"
        );
    }

    #[test]
    fn live_state_equals_replayed_state_across_every_ladder_path() {
        // One scenario per branch the engine can take, so the equality is
        // exercised against commits, retries, escalations, deferrals, parks,
        // answers, and a halt — not just the happy path.
        let scenarios = vec![
            Scenario::new(
                "commit",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
                vec![Effect::EditFile],
            ),
            Scenario::new(
                "retry",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n",
                vec![Effect::NoEdit, Effect::EditFile],
            ),
            Scenario::new(
                "escalate",
                "[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n",
                vec![Effect::NoEdit, Effect::EditFile],
            ),
            Scenario::new(
                "defer",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
                vec![Effect::RateLimited, Effect::EditFile],
            ),
            Scenario::new(
                "park-then-answer",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
                vec![Effect::NoEdit, Effect::EditFile],
            )
            .answered(vec![Answer::Answered {
                text: "the widget lives in src/widget.rs".to_owned(),
            }]),
            Scenario::new(
                "decline-and-halt",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
                vec![Effect::NoEdit],
            )
            .answered(vec![Answer::Declined]),
            Scenario::new(
                "reviewer-asks-for-a-human",
                "[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n",
                vec![Effect::EditFile],
            )
            .reviewed(vec![ReviewBehavior::NeedsHuman]),
        ];

        for Scenario {
            name,
            config,
            effects,
            reviews,
            answers,
        } in scenarios
        {
            let repo = temp_engine_repo(&format!("replay-{name}"));
            seed(
                &repo,
                "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
                 ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
                Some(config),
            );
            let mut opts = options(&repo);
            opts.config_path = Some(repo.join("tactus.toml"));
            let source = source(effects, reviews);
            let scripted = ScriptedAnswers::new(answers);
            let (report, live) = run_harness_inner(
                &opts,
                &Harness {
                    adapters: &source,
                    answers: Some(&scripted),
                    sleeper: None,
                },
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_live_equals_replay(&repo, &live, &report);
        }
    }

    #[test]
    fn an_aborting_error_still_leaves_a_replayable_log() {
        // The engine dying between the agent's edits and a verdict is §19's
        // "engine crash" row. Nothing gets to write a tidy ending, so the log
        // has to be enough on its own.
        let repo = temp_engine_repo("abortlog");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // No adapter for the agent the chain names: the failure lands inside
        // `step_task`, after `attempt_started` would have been emitted.
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
        opts.attempt_timeout = Duration::from_secs(60);
        let report = run_with(&opts, &source).expect("this one succeeds");

        // Now truncate the log to the moment before the attempt reported, the
        // exact on-disk shape a kill leaves, and confirm it still folds.
        let paths = paths_of(&repo, &report.run_id);
        let text = fs::read_to_string(paths.events()).expect("log");
        let lines: Vec<&str> = text.lines().collect();
        let cut = lines
            .iter()
            .position(|line| line.contains("\"attempt_finished\""))
            .expect("the run recorded an attempt");
        fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");

        let replayed = replay_of(&repo, &report.run_id);
        assert_eq!(replayed.interrupted, 1, "the dangling attempt is settled");
        assert!(
            replayed.interrupted_run(),
            "and the run reads as interrupted rather than finished"
        );
        assert_eq!(replayed.state.states[0], TaskState::Pending);
    }

    #[test]
    fn a_truncated_run_resumes_without_spending_the_interrupted_attempt() {
        // Decision 3, end to end: the attempt shows up in the ledger, the
        // rung's allowance does not, and the task completes on the retry.
        let repo = temp_engine_repo("resumetrunc");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            // One attempt on one rung: if the interrupted attempt had been
            // counted, the task could never commit.
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        let run_id = report.run_id.clone();
        let paths = paths_of(&repo, &run_id);

        // Rewind the record to mid-attempt and put the tree back the way a
        // dead agent would have left it.
        let text = fs::read_to_string(paths.events()).expect("log");
        let lines: Vec<&str> = text.lines().collect();
        let cut = lines
            .iter()
            .position(|line| line.contains("\"attempt_finished\""))
            .expect("an attempt");
        fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");
        git_in(&repo, &["reset", "-q", "--hard", "HEAD~1"]);
        fs::write(repo.join("agent-output.txt"), "half-written\n").expect("residue");

        let source = fake(Effect::EditFile);
        let (resumed, state) = resume_harness_inner(
            &resume_options(&repo, &run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume");

        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t1"));

        let t1 = task(&resumed, "t1");
        assert_eq!(
            t1.attempts.len(),
            2,
            "the interrupted attempt is on the record beside the one that worked"
        );
        assert_eq!(
            t1.attempts[0].failure.as_ref().map(|f| f.kind),
            Some(FailureKind::Interrupted)
        );
        assert_eq!(
            t1.attempts[0].cost_usd, None,
            "unknown spend is reported as unknown, not as free"
        );
        assert_eq!(t1.attempts[1].tier, "small", "still on the same rung");
        assert!(
            !t1.attempts[1].resumed,
            "§14: the tree was discarded, so the session cannot be trusted"
        );

        // The residue is gone and the branch is linear.
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "crash residue discarded"
        );
        assert_eq!(
            git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
            "1",
            "one commit, not a duplicate of the interrupted attempt's work"
        );
        assert!(
            resumed
                .warnings
                .iter()
                .any(|w| w.contains("discarded") && w.contains("agent-output.txt")),
            "the operator is told what was thrown away: {:?}",
            resumed.warnings
        );
        assert_live_equals_replay(&repo, &state, &resumed);
    }

    #[test]
    fn killing_a_run_mid_attempt_leaves_a_resumable_record() {
        // The real thing: a separate process is driven into an attempt and
        // dies inside it, exactly as `kill -9` or a power cut would.
        let repo = temp_engine_repo("crashkill");
        seed(
            &repo,
            "## First\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Second\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );

        let exe = std::env::current_exe().expect("test binary");
        let status = Command::new(exe)
            .args([
                "--exact",
                "engine::tests::crash_child_dies_inside_an_attempt",
                "--ignored",
                "--test-threads",
                "1",
            ])
            .env("TACTUS_CRASH_REPO", &repo)
            .output()
            .expect("spawn the child run");
        assert_eq!(
            status.status.code(),
            Some(CRASH_EXIT_CODE),
            "the child must die inside the attempt, not finish or panic: {}",
            String::from_utf8_lossy(&status.stderr)
        );

        let run_id = rundir::latest_run(&repo).expect("the child started a run");
        let paths = paths_of(&repo, &run_id);

        // What a kill leaves: a dirty tree and an attempt that never reported.
        assert!(
            !git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "the dead agent's edits are still in the tree"
        );
        let log = fs::read_to_string(paths.events()).expect("log");
        let last = log.lines().last().expect("events");
        assert!(
            last.contains("\"attempt_started\"") && last.contains("\"t2\""),
            "the log ends mid-attempt: {last}"
        );
        assert!(
            !log.contains("\"run_finished\""),
            "a killed run never records an ending"
        );

        let before = replay_of(&repo, &run_id);
        assert!(before.interrupted_run(), "status calls it interrupted");
        assert_eq!(before.interrupted, 1);
        assert!(
            crate::status::render(&before).contains(&format!("tactus resume {run_id}")),
            "and tells the operator how to continue it"
        );

        // The lock died with the process, so nothing has to be cleared by hand.
        assert!(!rundir::is_running(&paths), "the OS released the lock");

        let source = fake(Effect::EditFile);
        let (resumed, state) = resume_harness_inner(
            &resume_options(&repo, &run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume the killed run");

        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t1"), "the work it did survived");
        assert!(
            committed(&resumed, "t2"),
            "and the work it died in got done"
        );
        assert_eq!(
            git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
            "2",
            "one commit per task, with nothing duplicated by the resume"
        );
        let t2 = task(&resumed, "t2");
        assert_eq!(
            t2.attempts[0].failure.as_ref().map(|f| f.kind),
            Some(FailureKind::Interrupted),
            "the attempt it died in is on the record: {t2:?}"
        );
        assert_live_equals_replay(&repo, &state, &resumed);
    }

    /// Spawned by `killing_a_run_mid_attempt_leaves_a_resumable_record`.
    /// Ends its own process on purpose, which is why it must never run as part
    /// of the ordinary suite.
    #[test]
    #[ignore = "spawned by killing_a_run_mid_attempt_leaves_a_resumable_record"]
    fn crash_child_dies_inside_an_attempt() {
        let Ok(repo) = std::env::var("TACTUS_CRASH_REPO") else {
            return;
        };
        let repo = PathBuf::from(repo);
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // t1 commits; the process dies inside t2's first attempt.
        let source = source(
            vec![Effect::EditFile, Effect::Exit],
            vec![ReviewBehavior::Pass],
        );
        let _ = run_with(&opts, &source);
        // Only reachable if the adapter never got a second invocation, which
        // would mean this test is not exercising what it claims to.
        std::process::exit(0);
    }

    #[test]
    fn a_parked_run_is_answered_out_of_band_and_resumed() {
        // §21's definition-of-done (d) across processes: the run ends parked,
        // a person answers with `tactus answer` while nothing is running, and
        // the resume picks the answer up.
        let repo = temp_engine_repo("answerresume");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Depends on it\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");

        assert_eq!(report.outcome(), RunOutcome::Parked);
        let run_id = report.run_id.clone();
        let question = report
            .questions
            .first()
            .expect("a question was raised")
            .question
            .id
            .to_string();

        // Nothing is running; the answer is written by the CLI path.
        let recorded = crate::answer::answer(
            &repo,
            &question[..8],
            crate::answer::Reply::Text("the widget lives in src/widget.rs".to_owned()),
        )
        .expect("answer by prefix");
        assert_eq!(recorded.run_id, run_id);
        assert!(!recorded.run_is_live);

        let source = fake(Effect::EditFile);
        let (resumed, state) = resume_harness_inner(
            &resume_options(&repo, &run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume");

        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t1"), "the answer un-parked it");
        assert!(committed(&resumed, "t2"), "and its dependent ran");

        // This adapter is fresh for the resume, so its first invocation is
        // t1's retry — the one the answer released. t2 runs after it.
        let runs = source.adapter.runs();
        let retry = runs.first().expect("a retry ran");
        assert!(
            retry.prompt.contains("src/widget.rs"),
            "the operator's answer reached the agent: {}",
            retry.prompt
        );
        assert!(
            retry.prompt.contains("instruction from a person"),
            "labelled as an instruction, not quoted as data"
        );
        assert_live_equals_replay(&repo, &state, &resumed);
    }

    #[test]
    fn an_answer_arriving_mid_run_unparks_without_a_hard_block() {
        // Invariant 6 at its most useful: the operator answers from elsewhere
        // while other work is still going, and the task is released on the
        // next scheduler turn rather than at the end of the run.
        let repo = temp_engine_repo("midrun");
        seed(
            &repo,
            "## Asks a question\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::AskQuestion, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        // Nobody is reachable through the answer *channel* at all: if the
        // sweep did not exist, t1 could only ever end parked.
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&AnsweringViaFile { repo: repo.clone() }),
                sleeper: None,
            },
        )
        .expect("run");

        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
        assert!(committed(&report, "t1"), "the file answer released it");
        assert!(committed(&report, "t3"));
        let answered = report.questions.first().expect("one question");
        assert!(
            matches!(&answered.answer, Some(Answer::Answered { text }) if text.contains("opaque")),
            "the answer is recorded against the question: {answered:?}"
        );
    }

    /// Stands in for an operator running `tactus answer` in another terminal
    /// while the run is still going: it writes the file and tells the engine
    /// nobody replied, so only the sweep can find it.
    struct AnsweringViaFile {
        repo: PathBuf,
    }

    impl AnswerSource for AnsweringViaFile {
        fn id(&self) -> &'static str {
            "test-file-writer"
        }

        fn resolve(&self, question: &Question) -> Result<Answer, TactusError> {
            let _ = crate::answer::answer(
                &self.repo,
                question.id.as_str(),
                crate::answer::Reply::Text("opaque cursors".to_owned()),
            );
            Ok(Answer::Unanswered)
        }
    }

    #[test]
    fn blocking_propagates_transitively_and_against_plan_order() {
        // The chain is listed backwards on purpose: a single pass in plan
        // order would settle `late` before `mid` was known to be blocked, and
        // report it as merely skipped.
        let repo = temp_engine_repo("blocked");
        seed(
            &repo,
            "## Last\n<!-- tactus: id=late kind=implement depends=mid -->\n\n\
             ## Middle\n<!-- tactus: id=mid kind=implement depends=first -->\n\n\
             ## First\n<!-- tactus: id=first kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");

        assert!(matches!(
            task(&report, "first").status,
            TaskRunStatus::Parked { .. }
        ));
        assert!(
            matches!(&task(&report, "mid").status, TaskRunStatus::Blocked { by } if by == "first"),
            "the direct dependent is blocked: {report:?}"
        );
        assert!(
            matches!(&task(&report, "late").status, TaskRunStatus::Blocked { by } if by == "mid"),
            "and so is its dependent, naming the nearest blocker: {report:?}"
        );
    }

    #[test]
    fn answering_a_blocker_releases_the_chain_behind_it() {
        // Blocked is a *view*, not recorded state — which is what lets an
        // answer make a whole chain runnable again on resume.
        let repo = temp_engine_repo("unblock");
        seed(
            &repo,
            "## Last\n<!-- tactus: id=late kind=implement depends=mid -->\n\n\
             ## Middle\n<!-- tactus: id=mid kind=implement depends=first -->\n\n\
             ## First\n<!-- tactus: id=first kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");
        let run_id = report.run_id.clone();
        let question = report.questions[0].question.id.to_string();

        crate::answer::answer(
            &repo,
            &question,
            crate::answer::Reply::Text("write src/first.rs".to_owned()),
        )
        .expect("answer");

        let source = fake(Effect::EditFile);
        let resumed = resume_with(&resume_options(&repo, &run_id), &source).expect("resume");
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        for id in ["first", "mid", "late"] {
            assert!(committed(&resumed, id), "{id} should have run: {resumed:?}");
        }
    }

    #[test]
    fn an_exhausted_pool_and_a_silent_operator_still_terminate() {
        // The drain loop's termination argument, executed: an adapter that
        // never succeeds, a pool that never returns, and a channel nobody
        // answers. Every branch of the loop fires and the run still ends.
        let repo = temp_engine_repo("terminate");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- tactus: id=t2 kind=implement depends= -->\n\n\
             ## After one\n<!-- tactus: id=t3 kind=implement depends=t1 -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.max_defers = 2;
        let source = source(vec![Effect::RateLimited], vec![ReviewBehavior::Pass]);
        let answers = CountingAnswers::default();
        let sleeper = RecordingSleeper::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: Some(&sleeper),
            },
        )
        .expect("the run terminates rather than spinning");

        assert_eq!(report.outcome(), RunOutcome::Parked);
        for id in ["t1", "t2"] {
            assert!(
                matches!(task(&report, id).status, TaskRunStatus::Parked { .. }),
                "{id}: {report:?}"
            );
        }
        assert!(matches!(
            task(&report, "t3").status,
            TaskRunStatus::Blocked { .. }
        ));
        assert_eq!(
            answers.count(),
            2,
            "each question is asked exactly once, however many times the loop turns"
        );
        assert!(
            !sleeper.waits().is_empty(),
            "the deferral branch really fired"
        );
        assert!(
            sleeper.waits().len() <= 8,
            "and it was bounded: {:?}",
            sleeper.waits()
        );
    }

    // ---- step 8: resume refuses rather than guessing -----------------------

    fn resume_err(repo: &Path, run_id: &str) -> String {
        let source = fake(Effect::EditFile);
        resume_with(&resume_options(repo, run_id), &source)
            .expect_err("resume must refuse")
            .to_string()
    }

    /// A run that ends parked — the resumable shape every refusal test starts
    /// from, so each one isolates exactly the thing it breaks.
    fn parked_run(tag: &str) -> (PathBuf, String) {
        let repo = temp_engine_repo(tag);
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Parked);
        (repo, report.run_id)
    }

    #[test]
    fn resume_refuses_when_the_branch_moved_under_it() {
        // §15's HEAD check. Something committed after the run stopped, so the
        // log no longer describes what is on the branch.
        let (repo, run_id) = parked_run("headmoved");
        fs::write(repo.join("someone-else.txt"), "a hand-made commit\n").expect("file");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "not the engine's work"]);

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("record ends at"), "got: {err}");
        assert!(
            err.contains("Move the branch back"),
            "and says what to do: {err}"
        );
    }

    #[test]
    fn resume_refuses_when_the_frozen_plan_changed() {
        let (repo, run_id) = parked_run("planmoved");
        fs::write(
            repo.join("plan.md"),
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\nNow with a body.\n",
        )
        .expect("edit the plan");

        let err = resume_err(&repo, &run_id);
        assert!(
            err.contains("has changed since this run froze it"),
            "got: {err}"
        );
        assert!(
            err.contains("attribute work to the wrong tasks"),
            "and why it matters: {err}"
        );
    }

    #[test]
    fn resume_refuses_when_routing_moved_under_a_recorded_rung() {
        // `Progress.rung` is an index into the chain; re-resolving a different
        // chain would point it at another tier without saying so.
        let (repo, run_id) = parked_run("chainmoved");
        fs::write(
            repo.join("tactus.toml"),
            "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n",
        )
        .expect("edit config");

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("routing has changed"), "got: {err}");
        assert!(err.contains("`t1` ran on [small]"), "names the task: {err}");
    }

    #[test]
    fn resume_refuses_when_the_run_branch_is_gone() {
        let (repo, run_id) = parked_run("branchgone");
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim()
            .to_owned();
        git_in(&repo, &["switch", "-q", "main"]);
        git_in(&repo, &["branch", "-q", "-D", &branch]);

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("no longer exists"), "got: {err}");
    }

    #[test]
    fn resume_refuses_to_switch_over_uncommitted_work() {
        let (repo, run_id) = parked_run("dirtyelsewhere");
        git_in(&repo, &["switch", "-q", "main"]);
        fs::write(
            repo.join("my-own-work.txt"),
            "not the engine's to discard\n",
        )
        .expect("file");

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("Commit or stash"), "got: {err}");
        assert!(
            repo.join("my-own-work.txt").exists(),
            "a refusal must not destroy the work it refused over"
        );
    }

    #[test]
    fn resume_refuses_a_run_that_already_finished_or_halted() {
        let repo = temp_engine_repo("finished");
        let complete = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &complete).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete);
        let err = resume_err(&repo, &report.run_id);
        assert!(err.contains("already completed"), "got: {err}");

        let repo = temp_engine_repo("halted");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n",
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
        assert_eq!(report.outcome(), RunOutcome::Halted);
        let err = resume_err(&repo, &report.run_id);
        assert!(err.contains("halted at `t1`"), "got: {err}");
    }

    #[test]
    fn resume_refuses_while_another_process_holds_the_run() {
        let (repo, run_id) = parked_run("locked");
        let paths = paths_of(&repo, &run_id);
        let _held = RunLock::acquire(&paths).expect("hold it");

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("already driving run"), "got: {err}");
    }

    #[test]
    fn an_unknown_run_id_lists_what_is_there() {
        let (repo, _) = parked_run("unknownid");
        let err = resume_err(&repo, "01NOPE");
        assert!(err.contains("known runs"), "got: {err}");
    }

    // ---- step 8: status and the ledger -------------------------------------

    #[test]
    fn status_reports_a_live_run_and_the_ledger_reads_from_the_log() {
        let repo = temp_engine_repo("statusledger");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");

        let loaded = replay_of(&repo, &report.run_id);
        assert!(!loaded.running, "nothing holds a finished run");
        assert!(!loaded.interrupted_run());

        let rendered = crate::status::render(&loaded);
        assert!(rendered.contains("ledger:"), "{rendered}");
        assert!(rendered.contains("t1"), "{rendered}");
        assert!(
            rendered.contains("not connected"),
            "pool drain is honest about arriving with the capacity engine"
        );
        assert!(
            rendered.contains(&loaded.paths.private.display().to_string()),
            "and points at where the transcripts actually are"
        );

        // The ledger totals are the run's, derived from the log rather than
        // carried over from the process that wrote it.
        assert!(
            (loaded.report().total_cost_usd - report.total_cost_usd).abs() < 1e-9,
            "{} vs {}",
            loaded.report().total_cost_usd,
            report.total_cost_usd
        );

        // A live run says so.
        let paths = paths_of(&repo, &report.run_id);
        let _held = RunLock::acquire(&paths).expect("simulate a live engine");
        let live = replay_of(&repo, &report.run_id);
        assert!(live.running);
        assert!(crate::status::render(&live).contains("running now"));
    }

    #[test]
    fn following_a_finished_run_replays_it_and_stops() {
        let repo = temp_engine_repo("follow");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");

        let loaded = replay_of(&repo, &report.run_id);
        let sleeper = RecordingSleeper::default();
        let mut out: Vec<u8> = Vec::new();
        crate::status::follow(&loaded, &sleeper, Duration::ZERO, 2, &mut out).expect("follow");
        let text = String::from_utf8(out).expect("utf8");

        assert!(text.contains("run"), "{text}");
        assert!(text.contains("t1: committed"), "{text}");
        assert!(
            text.contains("run finished"),
            "it stops at the ending rather than idling: {text}"
        );
        assert!(
            sleeper.waits().is_empty(),
            "a finished run needs no waiting at all"
        );
        for line in text.lines() {
            assert!(!line.is_empty());
        }
    }

    #[test]
    fn transcripts_live_outside_the_workspace_and_survive_a_rollback() {
        // The §15 split, and the reason the private root cannot be inside the
        // repo: §14's rollback is `git clean -fd`, which would delete it.
        let repo = temp_engine_repo("private");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // The first attempt fails, so a rollback happens before the second.
        let adapters = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &adapters).expect("run");
        let paths = paths_of(&repo, &report.run_id);

        for private in [paths.transcripts(), paths.reviews(), paths.settings()] {
            assert!(
                !private.starts_with(&repo),
                "{} must be outside the workspace",
                private.display()
            );
            assert!(
                fs::read_dir(&private).into_iter().flatten().count() > 0,
                "{} kept its contents across the rollback",
                private.display()
            );
        }
        // The ops surface stays where §15 documents it.
        assert!(paths.events().starts_with(&repo));
        assert!(paths.questions().starts_with(&repo));
        // And nothing agent-authored is reachable from the repo.
        let in_repo = repo.join(".tactus").join("runs").join(&report.run_id);
        for leaked in ["transcripts", "reviews", "settings", "gates"] {
            assert!(
                !in_repo.join(leaked).exists(),
                "{leaked}/ must not exist inside the workspace"
            );
        }
    }
}
