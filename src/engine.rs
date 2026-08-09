//! Sequential execution engine (DESIGN.md §14): pre-flight, run branch, one
//! agent attempt per task on its chain's first rung, engine-captured diff,
//! gates with evidence axes (§11.1), engine-owned commit per task, rollback +
//! halt on failure. Review arrives in step 6, retry/escalation in step 7, the
//! event log and resume in step 8.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::{self, AgentAdapter, TaskRun, proc};
use crate::error::TactusError;
use crate::gates::{self, ShellGate};
use crate::ir::{Outcome, OutcomeStatus, PermissionMode, Plan, Task, TaskKind, WorkerProfile};
use crate::review;
use crate::route::ResolvedChain;
use crate::ulid;
use crate::util;
use crate::validate::{self, ValidateOptions};
use crate::workspace::Workspace;

/// §14: per-attempt wall clock, default 30 minutes.
pub const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

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
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TaskRunStatus {
    Committed {
        sha: String,
    },
    Failed {
        kind: FailureKind,
        reason: String,
    },
    /// Not attempted because the run halted earlier.
    Skipped,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskReport {
    pub id: String,
    pub title: String,
    pub model: String,
    pub status: TaskRunStatus,
    pub duration: Duration,
    pub cost_usd: Option<f64>,
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
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
    pub total_cost_usd: f64,
}

pub fn run(opts: &RunOptions) -> Result<RunReport, TactusError> {
    run_with(opts, &BuiltinAdapters)
}

pub fn run_with(opts: &RunOptions, adapters: &dyn AdapterSource) -> Result<RunReport, TactusError> {
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
    // to start, not a task failure (§19).
    let mut agent_ids: Vec<&str> = analysis
        .chains
        .iter()
        .flat_map(|c| c.rungs.iter().map(|r| r.binding.agent.as_str()))
        .collect();
    agent_ids.sort_unstable();
    agent_ids.dedup();
    for id in agent_ids {
        let adapter = adapters.get(id).ok_or_else(|| TactusError::Agent {
            message: format!("no adapter registered for agent `{id}`"),
        })?;
        adapter.probe()?;
    }

    // Effective gates come from the shared analysis (single derivation point
    // with `validate`). §14 pre-flight: the shell and every gate command must
    // resolve before any agent tokens are spent.
    let mut warnings = analysis.warnings.clone();
    let effective_gates: &[ShellGate] = &analysis.gates;
    if !effective_gates.is_empty() {
        gates::shell_available(analysis.config.shell)?;
        gates::resolve_programs(effective_gates, &opts.repo_root, &mut warnings)?;
    }
    let gate_cmds: Vec<String> = effective_gates.iter().map(|g| g.cmd.clone()).collect();

    let run_id = ulid::ulid();
    let branch = format!("tactus/run-{run_id}");
    let run_dir = opts.repo_root.join(".tactus").join("runs").join(&run_id);
    let transcripts = run_dir.join("transcripts");
    let settings_dir = run_dir.join("settings");
    let gates_dir = run_dir.join("gates");
    let artifacts_dir = run_dir.join("artifacts");
    let reviews_dir = run_dir.join("reviews");
    for dir in [
        &transcripts,
        &settings_dir,
        &gates_dir,
        &artifacts_dir,
        &reviews_dir,
    ] {
        fs::create_dir_all(dir).map_err(|source| TactusError::Io {
            path: dir.clone(),
            source,
        })?;
    }
    util::write_json(&run_dir.join("plan.normalized.json"), &analysis.plan)?;

    workspace.create_branch(&branch)?;

    let mut report = RunReport {
        run_id,
        branch,
        gates: effective_gates.iter().map(|g| g.name.clone()).collect(),
        gates_from_config: analysis.gates_from_config,
        warnings,
        tasks: Vec::with_capacity(analysis.plan.tasks.len()),
        halted_at: None,
        total_cost_usd: 0.0,
    };

    let report_path = run_dir.join("report.json");
    let cx = RunCx {
        workspace: &workspace,
        run_dir,
        gates: effective_gates,
        gate_cmds,
        adapters,
        review_binding: review_binding(&analysis.config),
        attempt_timeout: opts.attempt_timeout,
    };
    for index in topo_order(&analysis.plan) {
        let task = &analysis.plan.tasks[index];
        if report.halted_at.is_some() {
            report.tasks.push(TaskReport {
                id: task.id.to_string(),
                title: task.title.clone(),
                model: String::new(),
                status: TaskRunStatus::Skipped,
                duration: Duration::ZERO,
                cost_usd: None,
                session_id: None,
            });
            continue;
        }
        let chain = &analysis.chains[index];
        // Task ids are user-authored and may sanitize to the same string, so
        // the plan index makes each run-artifact stem unique.
        let stem = format!("{index:02}-{}", util::filename_component(task.id.as_str()));
        let task_report = attempt_task(task, chain, stem, &cx)
            // Persist what completed before an aborting error: a mid-run failure
            // must not take the record of already-committed work and spend with
            // it. (Replaced by the event log in step 8.)
            .inspect_err(|_| {
                let _ = util::write_json(&report_path, &report);
            })?;
        if let TaskRunStatus::Failed { .. } = task_report.status {
            report.halted_at = Some(task_report.id.clone());
        }
        report.total_cost_usd += task_report.cost_usd.unwrap_or(0.0);
        report.tasks.push(task_report);
        util::write_json(&report_path, &report)?;
    }
    util::write_json(&report_path, &report)?;
    Ok(report)
}

/// Why an attempt did not pass. The kind is kept separate from the prose so
/// the step-7 ladder can dispatch on it (rate limits defer without burning an
/// attempt; gate failures retry with feedback) and step 8 can log it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    NoChain,
    EmptyDiff,
    AgentError,
    Timeout,
    RateLimited,
    GateFailed,
    TestProvenance,
    ReviewFailed,
}

#[derive(Debug, Clone)]
pub struct AttemptFailure {
    pub kind: FailureKind,
    pub reason: String,
    /// Feedback for the retry that step 7 will send back to the agent.
    pub feedback: Option<String>,
}

impl AttemptFailure {
    fn new(kind: FailureKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
            feedback: None,
        }
    }

    fn with_feedback(mut self, feedback: String) -> Self {
        self.feedback = Some(feedback);
        self
    }
}

/// Everything one attempt needs, so the step-7 ladder can loop over
/// (rung, attempt) without re-deriving any of it.
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
}

/// The read-only worker that judges each attempt (§11.2). `None` disables
/// review — only when no adapter can serve the configured review tier.
struct Reviewer<'a> {
    adapter: &'a dyn AgentAdapter,
    profile: WorkerProfile,
}

struct AttemptResult {
    outcome: Outcome,
    failure: Option<AttemptFailure>,
    /// Reviewer spend, kept separate from the implementer's so the ledger can
    /// attribute both (§13).
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
        prompt: materialize_prompt(cx.task, cx.gate_cmds, cx.run_dir),
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

    // Verification ladder so far (§11): outcome sanity → cheap static
    // provenance → gates. Review joins in step 6.
    let mut failure = evaluate_outcome(&outcome, &output);
    if failure.is_none() && cx.task.kind == TaskKind::Test && !gates::diff_adds_tests(&outcome.diff)
    {
        failure = Some(AttemptFailure::new(
            FailureKind::TestProvenance,
            "test provenance: this Test task adds no test code — a Test task that changes no \
             tests proves nothing",
        ));
    }
    if failure.is_none()
        && let Some(gate_failure) = gates::run_all(
            cx.gates,
            workspace,
            &cx.run_dir.join("gates"),
            cx.task.id.as_str(),
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
    let mut review_cost_usd = None;
    if failure.is_none()
        && let Some(reviewer) = &cx.reviewer
    {
        let review = review::run_review(&review::ReviewCx {
            adapter: reviewer.adapter,
            profile: reviewer.profile.clone(),
            task: cx.task,
            diff: &outcome.diff,
            artifacts: &load_artifacts(cx.run_dir, cx.task),
            workspace: workspace.root(),
            run_dir: cx.run_dir,
            stem: cx.stem.clone(),
            timeout: cx.timeout,
        })?;
        review_cost_usd = review.cost_usd;
        if !review.verdict.pass {
            let summary = if review.verdict.reasons.is_empty() {
                "no reasons given".to_owned()
            } else {
                review.verdict.reasons.join("; ")
            };
            // required_changes is what the retry gets back verbatim (§11.4).
            let feedback = if review.verdict.required_changes.is_empty() {
                summary.clone()
            } else {
                review.verdict.required_changes.join("\n- ")
            };
            failure = Some(
                AttemptFailure::new(
                    FailureKind::ReviewFailed,
                    format!("review failed: {}", util::tail(&summary, 400)),
                )
                .with_feedback(feedback),
            );
        }
    }

    Ok(AttemptResult {
        outcome,
        failure,
        review_cost_usd,
    })
}

/// Artifacts this task should be judged against: its declared inputs, plus
/// the conventions brief whenever one exists (§11.2 injects it into every
/// downstream prompt).
fn load_artifacts(run_dir: &std::path::Path, task: &Task) -> Vec<(String, String)> {
    let mut wanted: Vec<String> = vec![CONVENTIONS_BRIEF.to_owned()];
    for id in &task.artifacts_in {
        if id.as_str() != CONVENTIONS_BRIEF {
            wanted.push(id.as_str().to_owned());
        }
    }
    wanted
        .into_iter()
        .filter_map(|id| {
            let content = fs::read_to_string(artifact_path(run_dir, &id)).ok()?;
            (!content.trim().is_empty()).then_some((id, content))
        })
        .collect()
}

const CONVENTIONS_BRIEF: &str = "conventions-brief";

/// Run-wide context every task attempt draws on, fixed at pre-flight.
struct RunCx<'a> {
    workspace: &'a Workspace,
    run_dir: PathBuf,
    gates: &'a [ShellGate],
    gate_cmds: Vec<String>,
    adapters: &'a dyn AdapterSource,
    /// (agent, model) the reviewer binds to, resolved once at pre-flight.
    review_binding: Option<(String, String)>,
    attempt_timeout: Duration,
}

/// One attempt on the chain's first rung (retry and escalation are step 7).
/// Environment failures (spawn, git) abort the run as `Err`; agent failures
/// roll back and report.
fn attempt_task(
    task: &Task,
    chain: &ResolvedChain,
    stem: String,
    cx: &RunCx<'_>,
) -> Result<TaskReport, TactusError> {
    let RunCx {
        workspace,
        run_dir,
        gates: effective_gates,
        gate_cmds,
        adapters,
        attempt_timeout,
        ..
    } = cx;
    let Some(rung) = chain.rungs.first() else {
        return Ok(TaskReport {
            id: task.id.to_string(),
            title: task.title.clone(),
            model: String::new(),
            status: TaskRunStatus::Failed {
                kind: FailureKind::NoChain,
                reason: "resolved chain is empty".to_owned(),
            },
            duration: Duration::ZERO,
            cost_usd: None,
            session_id: None,
        });
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
    // §11.2/§10: the reviewer is its own read-only binding, at the configured
    // review tier (frontier by default) rather than the implementer's rung —
    // a small model reviewing its own work is not verification.
    let reviewer = cx.review_binding.as_ref().and_then(|(agent, model)| {
        adapters.get(agent).map(|adapter| Reviewer {
            adapter,
            profile: review::profile_for(agent, model, &format!("review-{model}")),
        })
    });

    let attempt_cx = AttemptCx {
        task,
        profile: profile.clone(),
        adapter,
        attempt: 1,
        stem,
        run_dir,
        gates: effective_gates,
        gate_cmds,
        reviewer,
        timeout: *attempt_timeout,
    };

    // Any error between the agent editing files and the verdict leaves the
    // tree dirty; the run cannot continue but must not hand the user a
    // half-staged workspace either (§14).
    let attempt = match run_attempt(&attempt_cx, workspace, None) {
        Ok(attempt) => attempt,
        Err(error) => {
            let _ = workspace.discard_uncommitted();
            return Err(error);
        }
    };

    let status = match attempt.failure {
        None => {
            let sha = workspace.commit(&format!("[tactus] {}: {}", task.id, task.title))?;
            // Scrub gate side-effects (build artifacts, lockfile churn) so
            // they cannot leak into the next task's captured diff; the commit
            // recorded exactly the verified staged set.
            workspace.discard_uncommitted()?;
            TaskRunStatus::Committed { sha }
        }
        Some(failure) => {
            workspace.discard_uncommitted()?;
            TaskRunStatus::Failed {
                kind: failure.kind,
                reason: failure.reason,
            }
        }
    };
    Ok(TaskReport {
        id: task.id.to_string(),
        title: task.title.clone(),
        model: profile.model,
        status,
        duration: attempt.outcome.duration,
        cost_usd: match (attempt.outcome.cost_usd, attempt.review_cost_usd) {
            (None, None) => None,
            (worker, review) => Some(worker.unwrap_or(0.0) + review.unwrap_or(0.0)),
        },
        session_id: attempt.outcome.session_id,
    })
}

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
        OutcomeStatus::Completed if outcome.diff.trim().is_empty() => {
            // §11 evidence axis, enforced early: an empty diff can never pass.
            Some(AttemptFailure::new(
                FailureKind::EmptyDiff,
                "agent reported success but the diff is empty — \"done\" claims require changed \
                 code",
            ))
        }
        OutcomeStatus::Completed => None,
        OutcomeStatus::AgentError => Some(
            AttemptFailure::new(
                FailureKind::AgentError,
                format!("agent error (exit {:?}): {}", output.code, detail()),
            )
            .with_feedback(detail()),
        ),
        OutcomeStatus::Timeout => Some(AttemptFailure::new(
            FailureKind::Timeout,
            "attempt hit the wall-clock timeout",
        )),
        OutcomeStatus::RateLimited => Some(AttemptFailure::new(
            FailureKind::RateLimited,
            format!(
                "pool rate-limited: {} (deferral arrives with the capacity engine)",
                detail()
            ),
        )),
    }
}

/// §14 prompt materialization: body + acceptance + artifact inputs + the
/// exact gate commands the agent is permitted to run (the allow rules are
/// exact-match, so the agent must know the literal strings). The conventions
/// brief joins once design-phase artifacts carry content.
fn materialize_prompt(task: &Task, gate_cmds: &[String], run_dir: &std::path::Path) -> String {
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
    prompt.push_str(
        "Rules:\n\
         - Complete ONLY this task; leave work that belongs to other tasks alone.\n\
         - Edit files inside this repository only.\n\
         - NEVER run git commit, branch, merge, push, or reset — the engine owns git.\n\
         - When the acceptance criteria hold, stop and summarize what changed.\n",
    );
    prompt
}

/// The reviewer's binding: `[routing] review = { tier = … }` if configured,
/// else frontier (§17's default), honouring a pin for that tier and otherwise
/// taking the catalog's example binding — the same rules the router uses.
fn review_binding(cfg: &crate::config::Config) -> Option<(String, String)> {
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
/// The graph is already cycle-free (checked in pre-flight).
fn topo_order(plan: &Plan) -> Vec<usize> {
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
                    let _ = writeln!(
                        out,
                        "  {}: committed {sha} — {} ({:.1}s, {}, ${:.4})",
                        task.id,
                        task.title,
                        task.duration.as_secs_f64(),
                        task.model,
                        task.cost_usd.unwrap_or(0.0),
                    );
                }
                TaskRunStatus::Failed { reason, .. } => {
                    let _ = writeln!(out, "  {}: FAILED — {reason}", task.id);
                }
                TaskRunStatus::Skipped => {
                    let _ = writeln!(out, "  {}: skipped (run halted)", task.id);
                }
            }
        }
        let _ = writeln!(out, "total: ${:.4} (api-equivalent)", self.total_cost_usd);
        match &self.halted_at {
            Some(id) => {
                let _ = writeln!(
                    out,
                    "run halted at `{id}`; completed tasks are committed on {}",
                    self.branch
                );
            }
            None => {
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
    use crate::ir::Usage;
    use std::path::Path;
    use std::process::Command;

    #[derive(Clone, Copy)]
    enum Effect {
        /// Simulates an agent that edits the workspace and succeeds.
        EditFile,
        /// Simulates an agent that writes real test code.
        EditTest,
        /// Simulates a lying agent: success report, no changes.
        NoEdit,
        /// Simulates an agent-side failure.
        Error,
    }

    /// Scripted stand-in for a real CLI: `build` performs the "agent edit"
    /// directly (test-only shortcut) and returns a trivial command; `parse`
    /// reports the scripted outcome. Read-only profiles are review
    /// invocations and answer with a verdict, exercising the real
    /// command → stdout → parse → verdict path.
    struct FakeAdapter {
        effect: Effect,
        review: ReviewBehavior,
    }

    #[derive(Clone, Copy, PartialEq)]
    enum ReviewBehavior {
        Pass,
        Fail,
        /// Prose with no verdict block: drives the re-ask path.
        Unparseable,
    }

    /// Marker the fake's review command prints so `parse` can tell a review
    /// invocation from an implementation one.
    const REVIEW_MARKER: &str = "TACTUS-FAKE-REVIEW";

    fn fake(effect: Effect) -> FakeSource {
        FakeSource {
            adapter: FakeAdapter {
                effect,
                review: ReviewBehavior::Pass,
            },
        }
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
            let edit: Option<(&str, String)> = match self.effect {
                Effect::EditFile => {
                    let marker = run.workspace.join("agent-output.txt");
                    let previous = fs::read_to_string(&marker).unwrap_or_default();
                    Some((
                        "agent-output.txt",
                        format!("{previous}edited: {}\n", run.prompt.len()),
                    ))
                }
                Effect::EditTest => Some((
                    "widget_test.rs",
                    "#[test]\nfn widget_works() {\n    assert!(true);\n}\n".to_owned(),
                )),
                Effect::NoEdit | Effect::Error => None,
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
                let answer = match self.review {
                    ReviewBehavior::Pass => {
                        "Checked every criterion.\n```json\n{\"pass\": true, \"reasons\": \
                         [\"meets the acceptance criteria\"], \"required_changes\": []}\n```"
                    }
                    ReviewBehavior::Fail => {
                        "The diff misses a case.\n```json\n{\"pass\": false, \"reasons\": \
                         [\"no error handling for empty input\"], \"required_changes\": \
                         [\"handle the empty-input case\"]}\n```"
                    }
                    ReviewBehavior::Unparseable => "Looks fine to me, ship it.",
                };
                return Ok(Outcome {
                    status: OutcomeStatus::Completed,
                    diff: String::new(),
                    detail: Some(answer.to_owned()),
                    session_id: Some("fake-review-session".to_owned()),
                    usage: None,
                    cost_usd: Some(0.05),
                    pool_drain: None,
                    transcript_path: PathBuf::new(),
                    duration: out.duration,
                });
            }
            let status = match self.effect {
                Effect::Error => OutcomeStatus::AgentError,
                Effect::EditFile | Effect::EditTest | Effect::NoEdit => OutcomeStatus::Completed,
            };
            Ok(Outcome {
                status,
                diff: String::new(),
                detail: matches!(self.effect, Effect::Error)
                    .then(|| "fake adapter error detail".to_owned()),
                session_id: Some("fake-session".to_owned()),
                usage: Some(Usage::default()),
                cost_usd: Some(0.01),
                pool_drain: None,
                transcript_path: PathBuf::new(),
                duration: out.duration,
            })
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

    fn options(repo: &Path) -> RunOptions {
        RunOptions {
            plan_path: repo.join("plan.md"),
            config_path: None,
            pools_path: Some(
                std::env::temp_dir()
                    .join("tactus-engine-missing")
                    .join("pools.toml"),
            ),
            repo_root: repo.to_path_buf(),
            attempt_timeout: Duration::from_secs(60),
        }
    }

    #[test]
    fn happy_path_commits_one_commit_per_task() {
        let repo = temp_engine_repo("happy");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run succeeds");

        assert!(report.halted_at.is_none());
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
    fn empty_diff_fails_rolls_back_and_halts() {
        let repo = temp_engine_repo("nodiff");
        let source = fake(Effect::NoEdit);
        let report = run_with(&options(&repo), &source).expect("engine itself is fine");

        assert_eq!(report.halted_at.as_deref(), Some("t1"));
        let TaskRunStatus::Failed { reason, .. } = &report.tasks[0].status else {
            panic!("t1 should fail: {report:?}");
        };
        assert!(reason.contains("empty"), "evidence rule cited: {reason}");
        assert!(matches!(report.tasks[1].status, TaskRunStatus::Skipped));
        let count = git_in(&repo, &["rev-list", "--count", "main..HEAD"]);
        assert_eq!(count.trim(), "0", "nothing committed");
        assert!(git_in(&repo, &["status", "--porcelain"]).trim().is_empty());
    }

    #[test]
    fn agent_error_halts_with_reason() {
        let repo = temp_engine_repo("agenterr");
        let source = fake(Effect::Error);
        let report = run_with(&options(&repo), &source).expect("engine ok");
        assert_eq!(report.halted_at.as_deref(), Some("t1"));
        let TaskRunStatus::Failed { reason, .. } = &report.tasks[0].status else {
            panic!("t1 should fail");
        };
        assert!(reason.contains("agent error"), "reason: {reason}");
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
        fs::write(
            repo.join("tactus.toml"),
            "[[gates]]\nname = \"version\"\ncmd = \"git --version\"\n",
        )
        .expect("config");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "config"]);

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        assert!(report.halted_at.is_none(), "report: {report:?}");
        assert_eq!(report.gates, ["version"]);
        assert!(report.gates_from_config);
        assert!(report.render().contains("gates: version [from config]"));
    }

    #[test]
    fn failing_gate_halts_with_log_and_rollback() {
        let repo = temp_engine_repo("gatefail");
        fs::write(
            repo.join("tactus.toml"),
            "[[gates]]\nname = \"never\"\ncmd = \"git frobnicate-not-a-command\"\n",
        )
        .expect("config");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "config"]);

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("engine ok");
        assert_eq!(report.halted_at.as_deref(), Some("t1"));
        let TaskRunStatus::Failed { reason, .. } = &report.tasks[0].status else {
            panic!("t1 should fail on the gate");
        };
        assert!(reason.contains("gate `never` failed"), "reason: {reason}");
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "rolled back"
        );
        let gates_dir = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("gates");
        assert!(gates_dir.join("t1-1-never.log").is_file(), "full log kept");
    }

    #[test]
    fn unresolvable_gate_refuses_at_preflight() {
        let repo = temp_engine_repo("gateresolve");
        fs::write(
            repo.join("tactus.toml"),
            "[[gates]]\nname = \"ghost\"\ncmd = \"definitely-not-a-real-tool-xyz build\"\n",
        )
        .expect("config");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "config"]);

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
        fs::write(
            repo.join("plan.md"),
            "## Test the widget\n<!-- tactus: id=tt depends= -->\nAdd coverage.\n",
        )
        .expect("plan");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "test plan"]);

        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("engine ok");
        assert_eq!(report.halted_at.as_deref(), Some("tt"));
        let TaskRunStatus::Failed { reason, .. } = &report.tasks[0].status else {
            panic!("provenance should fail");
        };
        assert!(reason.contains("provenance"), "reason: {reason}");
    }

    #[test]
    fn test_task_adding_real_tests_passes_provenance() {
        let repo = temp_engine_repo("provenance-ok");
        fs::write(
            repo.join("plan.md"),
            "## Test the widget\n<!-- tactus: id=tt depends= -->\n",
        )
        .expect("plan");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "test plan"]);

        let source = fake(Effect::EditTest);
        let report = run_with(&options(&repo), &source).expect("engine ok");
        assert!(report.halted_at.is_none(), "report: {report:?}");
        assert!(matches!(
            report.tasks[0].status,
            TaskRunStatus::Committed { .. }
        ));
    }

    #[test]
    fn gate_residue_is_scrubbed_not_committed() {
        let repo = temp_engine_repo("residue");
        // A gate that creates a file: residue must never reach a commit nor
        // survive the task.
        fs::write(
            repo.join("tactus.toml"),
            "[[gates]]\nname = \"leaky\"\ncmd = \"echo residue> residue.txt\"\n",
        )
        .expect("config");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "config"]);

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        assert!(report.halted_at.is_none(), "report: {report:?}");
        assert!(!repo.join("residue.txt").exists(), "residue scrubbed");
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "clean tree after run"
        );
        // Neither commit contains the gate's file.
        let log = git_in(&repo, &["log", "--name-only", "--format=", "main..HEAD"]);
        assert!(!log.contains("residue.txt"), "log: {log}");
    }

    #[test]
    fn prompt_names_the_allowed_gate_commands() {
        let task = Task {
            id: crate::ir::TaskId::from("t1"),
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
        let prompt = materialize_prompt(&task, &["cargo check --all-targets".to_owned()], &run_dir);
        assert!(prompt.contains("EXACTLY these commands"));
        assert!(prompt.contains("- cargo check --all-targets"));
        let bare = materialize_prompt(&task, &[], &run_dir);
        assert!(!bare.contains("EXACTLY these commands"));
    }

    #[test]
    fn a_failed_review_blocks_the_commit() {
        let repo = temp_engine_repo("reviewfail");
        let source = FakeSource {
            adapter: FakeAdapter {
                effect: Effect::EditFile,
                review: ReviewBehavior::Fail,
            },
        };
        let report = run_with(&options(&repo), &source).expect("engine ok");

        assert_eq!(report.halted_at.as_deref(), Some("t1"));
        let TaskRunStatus::Failed { kind, reason } = &report.tasks[0].status else {
            panic!("review should fail the task: {report:?}");
        };
        assert_eq!(*kind, FailureKind::ReviewFailed);
        assert!(
            reason.contains("no error handling for empty input"),
            "the reviewer's reasons reach the report: {reason}"
        );
        // Gates passed and the diff was real — only the reviewer objected.
        assert_eq!(
            git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
            "0",
            "nothing commits without a passing verdict"
        );
        assert!(git_in(&repo, &["status", "--porcelain"]).trim().is_empty());
    }

    #[test]
    fn an_unparseable_reviewer_fails_after_one_reask() {
        let repo = temp_engine_repo("reviewprose");
        let source = FakeSource {
            adapter: FakeAdapter {
                effect: Effect::EditFile,
                review: ReviewBehavior::Unparseable,
            },
        };
        let report = run_with(&options(&repo), &source).expect("engine ok");

        let TaskRunStatus::Failed { kind, reason } = &report.tasks[0].status else {
            panic!("a reviewer that never answers cannot pass a task");
        };
        assert_eq!(*kind, FailureKind::ReviewFailed);
        assert!(reason.contains("re-ask"), "reason: {reason}");
        // The re-ask actually happened, and both sides are on record.
        let reviews = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("reviews");
        assert!(reviews.join("00-t1-review.json").is_file());
        assert!(
            reviews.join("00-t1-review-reask.json").is_file(),
            "one re-ask before giving up (§11.2)"
        );
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

        let reviewer = allow_list("00-t1-review.json");
        assert_eq!(reviewer, ["Read", "Glob", "Grep"], "read-only, no shell");

        let implementer = allow_list("00-t1-1.json");
        assert!(
            implementer.contains(&"Edit".to_owned()),
            "implementer can edit"
        );
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
    }

    #[test]
    fn failures_carry_a_machine_readable_kind() {
        let repo = temp_engine_repo("kind");
        let source = fake(Effect::NoEdit);
        let report = run_with(&options(&repo), &source).expect("engine ok");
        let TaskRunStatus::Failed { kind, .. } = &report.tasks[0].status else {
            panic!("t1 should fail");
        };
        assert_eq!(*kind, FailureKind::EmptyDiff, "step 7 dispatches on this");
    }

    #[test]
    fn agent_error_reports_the_adapter_detail() {
        let repo = temp_engine_repo("detail");
        let source = fake(Effect::Error);
        let report = run_with(&options(&repo), &source).expect("engine ok");
        let TaskRunStatus::Failed { kind, reason } = &report.tasks[0].status else {
            panic!("t1 should fail");
        };
        assert_eq!(*kind, FailureKind::AgentError);
        assert!(
            reason.contains("fake adapter error detail"),
            "the JSON-body detail reaches the report: {reason}"
        );
    }

    #[test]
    fn prompt_wires_artifacts_to_real_files() {
        let run_dir = std::env::temp_dir().join(format!("tactus-artifact-{}", std::process::id()));
        let _ = fs::remove_dir_all(&run_dir);
        fs::create_dir_all(run_dir.join("artifacts")).expect("run dir");
        let mut task = Task {
            id: crate::ir::TaskId::from("t1"),
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
        let prompt = materialize_prompt(&task, &[], &run_dir);
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
        let prompt = materialize_prompt(&task, &[], &run_dir);
        assert!(
            prompt.contains("cursor = base64(offset)"),
            "content inlined"
        );

        task.artifacts_in.clear();
        task.artifacts_out.clear();
        let bare = materialize_prompt(&task, &[], &run_dir);
        assert!(!bare.contains("artifact"));
    }

    #[test]
    fn forward_dependencies_run_in_topo_order_not_plan_order() {
        let repo = temp_engine_repo("topo");
        fs::write(
            repo.join("plan.md"),
            "## Second by dependency\n<!-- tactus: id=late depends=early -->\n\n\
             ## First by dependency\n<!-- tactus: id=early depends= -->\n",
        )
        .expect("plan");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "forward plan"]);

        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let ids: Vec<&str> = report.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["early", "late"], "dependency beats document order");
    }
}
