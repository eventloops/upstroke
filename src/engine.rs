//! Sequential execution engine (DESIGN.md §14) — step 4 scope: pre-flight,
//! run branch, one agent attempt per task on its chain's first rung,
//! engine-captured diff, engine-owned commit per task, rollback + halt on
//! failure. Gates arrive in step 5, review in step 6, retry/escalation in
//! step 7, the event log and resume in step 8.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::agent::{self, AgentAdapter, TaskRun, claude, proc};
use crate::error::TactusError;
use crate::ir::{Outcome, OutcomeStatus, PermissionMode, Plan, Task, WorkerProfile};
use crate::route::ResolvedChain;
use crate::ulid;
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

#[derive(Debug)]
pub enum TaskRunStatus {
    Committed {
        sha: String,
    },
    Failed {
        reason: String,
    },
    /// Not attempted because the run halted earlier.
    Skipped,
}

#[derive(Debug)]
pub struct TaskReport {
    pub id: String,
    pub title: String,
    pub model: String,
    pub status: TaskRunStatus,
    pub duration: Duration,
    pub cost_usd: Option<f64>,
    pub session_id: Option<String>,
}

#[derive(Debug)]
pub struct RunReport {
    pub run_id: String,
    pub branch: String,
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

    let run_id = ulid::ulid();
    let branch = format!("tactus/run-{run_id}");
    let run_dir = opts.repo_root.join(".tactus").join("runs").join(&run_id);
    let transcripts = run_dir.join("transcripts");
    let settings_dir = run_dir.join("settings");
    for dir in [&transcripts, &settings_dir] {
        fs::create_dir_all(dir).map_err(|source| TactusError::Io {
            path: dir.clone(),
            source,
        })?;
    }
    write_json(&run_dir.join("plan.normalized.json"), &analysis.plan)?;

    workspace.create_branch(&branch)?;

    let mut report = RunReport {
        run_id,
        branch,
        tasks: Vec::with_capacity(analysis.plan.tasks.len()),
        halted_at: None,
        total_cost_usd: 0.0,
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
        let task_report = attempt_task(task, chain, &workspace, &run_dir, opts, adapters)?;
        if let TaskRunStatus::Failed { .. } = task_report.status {
            report.halted_at = Some(task_report.id.clone());
        }
        report.total_cost_usd += task_report.cost_usd.unwrap_or(0.0);
        report.tasks.push(task_report);
    }
    Ok(report)
}

/// One attempt on the chain's first rung (retry and escalation are step 7).
/// Environment failures (spawn, git) abort the run as `Err`; agent failures
/// roll back and report.
fn attempt_task(
    task: &Task,
    chain: &ResolvedChain,
    workspace: &Workspace,
    run_dir: &std::path::Path,
    opts: &RunOptions,
    adapters: &dyn AdapterSource,
) -> Result<TaskReport, TactusError> {
    let Some(rung) = chain.rungs.first() else {
        return Ok(TaskReport {
            id: task.id.to_string(),
            title: task.title.clone(),
            model: String::new(),
            status: TaskRunStatus::Failed {
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

    // Gates arrive in step 5 — until then edit profiles get no shell at all.
    let settings_path = run_dir.join("settings").join(format!("{}.json", task.id));
    write_json(&settings_path, &claude::permission_settings(&profile, &[]))?;

    let task_run = TaskRun {
        prompt: materialize_prompt(task),
        profile: profile.clone(),
        workspace: workspace.root().to_path_buf(),
        resume_session: None,
        settings_path: Some(settings_path),
    };
    let command = adapter.build(&task_run)?;
    let output = proc::run_with_timeout(command, &task_run.prompt, opts.attempt_timeout)?;

    let transcript_path = run_dir
        .join("transcripts")
        .join(format!("{}-1.json", task.id));
    write_text(&transcript_path, &output.stdout)?;
    if !output.stderr.trim().is_empty() {
        write_text(
            &run_dir
                .join("transcripts")
                .join(format!("{}-1.stderr.log", task.id)),
            &output.stderr,
        )?;
    }

    let mut outcome: Outcome = adapter.parse(&output)?;
    outcome.diff = workspace.capture_diff()?;
    outcome.transcript_path = transcript_path;

    let status = decide(task, &outcome, workspace, &output)?;
    Ok(TaskReport {
        id: task.id.to_string(),
        title: task.title.clone(),
        model: profile.model,
        status,
        duration: outcome.duration,
        cost_usd: outcome.cost_usd,
        session_id: outcome.session_id,
    })
}

fn decide(
    task: &Task,
    outcome: &Outcome,
    workspace: &Workspace,
    output: &proc::ProcessOutput,
) -> Result<TaskRunStatus, TactusError> {
    let failure = match outcome.status {
        OutcomeStatus::Completed if outcome.diff.trim().is_empty() => {
            // §11 evidence axis, enforced early: an empty diff can never pass.
            Some(
                "agent reported success but the diff is empty — \"done\" claims require \
                  changed code"
                    .to_owned(),
            )
        }
        OutcomeStatus::Completed => None,
        OutcomeStatus::AgentError => Some(format!(
            "agent error (exit {:?}): {}",
            output.code,
            tail(&output.stderr, 400)
        )),
        OutcomeStatus::Timeout => Some("attempt hit the wall-clock timeout".to_owned()),
        OutcomeStatus::RateLimited => {
            Some("pool rate-limited (deferral arrives with the capacity engine)".to_owned())
        }
    };
    match failure {
        None => {
            let sha = workspace.commit(&format!("[tactus] {}: {}", task.id, task.title))?;
            Ok(TaskRunStatus::Committed { sha })
        }
        Some(reason) => {
            workspace.rollback()?;
            Ok(TaskRunStatus::Failed { reason })
        }
    }
}

/// §14 prompt materialization: body + acceptance + artifact inputs. The
/// conventions brief joins once design-phase artifacts carry content.
fn materialize_prompt(task: &Task) -> String {
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
    if !task.artifacts_in.is_empty() {
        let needs: Vec<&str> = task.artifacts_in.iter().map(|a| a.as_str()).collect();
        let _ = writeln!(
            prompt,
            "Inputs produced by earlier tasks: {}\n",
            needs.join(", ")
        );
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

fn tail(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max {
        return trimmed.to_owned();
    }
    let start = trimmed.len() - max;
    let start = (start..trimmed.len())
        .find(|i| trimmed.is_char_boundary(*i))
        .unwrap_or(start);
    format!("…{}", &trimmed[start..])
}

fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) -> Result<(), TactusError> {
    let json = serde_json::to_string_pretty(value).map_err(|e| TactusError::Parse {
        message: format!("serializing {}: {e}", path.display()),
    })?;
    write_text(path, &(json + "\n"))
}

fn write_text(path: &std::path::Path, content: &str) -> Result<(), TactusError> {
    fs::write(path, content).map_err(|source| TactusError::Io {
        path: path.to_path_buf(),
        source,
    })
}

impl RunReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "run: {}", self.run_id);
        let _ = writeln!(out, "branch: {} (return with: git switch -)", self.branch);
        for task in &self.tasks {
            match &task.status {
                TaskRunStatus::Committed { sha } => {
                    let _ = writeln!(
                        out,
                        "  {}: committed {sha} ({}s, {}, ${:.4})",
                        task.id,
                        task.duration.as_secs(),
                        task.model,
                        task.cost_usd.unwrap_or(0.0),
                    );
                }
                TaskRunStatus::Failed { reason } => {
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
        /// Simulates a lying agent: success report, no changes.
        NoEdit,
        /// Simulates an agent-side failure.
        Error,
    }

    /// Scripted stand-in for a real CLI: `build` performs the "agent edit"
    /// directly (test-only shortcut) and returns a trivial command; `parse`
    /// reports the scripted outcome.
    struct FakeAdapter {
        effect: Effect,
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
            if matches!(self.effect, Effect::EditFile) {
                let marker = run.workspace.join("agent-output.txt");
                let previous = fs::read_to_string(&marker).unwrap_or_default();
                fs::write(&marker, format!("{previous}edited: {}\n", run.prompt.len())).map_err(
                    |e| TactusError::Agent {
                        message: format!("fake edit failed: {e}"),
                    },
                )?;
            }
            let mut cmd = if cfg!(windows) {
                let mut c = Command::new("cmd");
                c.args(["/C", "exit 0"]);
                c
            } else {
                let mut c = Command::new("sh");
                c.args(["-c", "exit 0"]);
                c
            };
            cmd.current_dir(&run.workspace);
            Ok(cmd)
        }

        fn parse(&self, out: &ProcessOutput) -> Result<Outcome, TactusError> {
            let status = match self.effect {
                Effect::Error => OutcomeStatus::AgentError,
                Effect::EditFile | Effect::NoEdit => OutcomeStatus::Completed,
            };
            Ok(Outcome {
                status,
                diff: String::new(),
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
        let source = FakeSource {
            adapter: FakeAdapter {
                effect: Effect::EditFile,
            },
        };
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
        assert!((report.total_cost_usd - 0.02).abs() < 1e-9);

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
        let source = FakeSource {
            adapter: FakeAdapter {
                effect: Effect::NoEdit,
            },
        };
        let report = run_with(&options(&repo), &source).expect("engine itself is fine");

        assert_eq!(report.halted_at.as_deref(), Some("t1"));
        let TaskRunStatus::Failed { reason } = &report.tasks[0].status else {
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
        let source = FakeSource {
            adapter: FakeAdapter {
                effect: Effect::Error,
            },
        };
        let report = run_with(&options(&repo), &source).expect("engine ok");
        assert_eq!(report.halted_at.as_deref(), Some("t1"));
        let TaskRunStatus::Failed { reason } = &report.tasks[0].status else {
            panic!("t1 should fail");
        };
        assert!(reason.contains("agent error"), "reason: {reason}");
    }

    #[test]
    fn dirty_tree_is_refused() {
        let repo = temp_engine_repo("dirty");
        fs::write(repo.join("stray.txt"), "uncommitted\n").expect("stray");
        let source = FakeSource {
            adapter: FakeAdapter {
                effect: Effect::EditFile,
            },
        };
        let err = run_with(&options(&repo), &source).expect_err("must refuse");
        assert!(err.to_string().contains("not clean"), "got: {err}");
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch.trim(), "main", "no run branch created");
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

        let source = FakeSource {
            adapter: FakeAdapter {
                effect: Effect::EditFile,
            },
        };
        let report = run_with(&options(&repo), &source).expect("run");
        let ids: Vec<&str> = report.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["early", "late"], "dependency beats document order");
    }
}
