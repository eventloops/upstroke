//! Extended notes: `docs/internals/engine/tests.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use super::attempt::{
    QUESTION_MARKER, artifact_path, evaluate_outcome, materialize_prompt, review_failure,
    worker_question,
};
use super::coordinator::{question_options, run_harness_inner};
use super::preflight::{gates_differ, validate_inputs};
use super::report::{sum_opt, task_report, total_of};
use super::resume::resume_harness_inner;
use super::*;
use crate::agent::{AgentAdapter, Caps, ProcessOutput, TaskRun};
use crate::capacity;
use crate::config;
use crate::events::{self, EventBody, EventLog, GateSummary, Progress, RunState, TaskState};
use crate::interaction::{self, AnswerSource, QuestionRecord, Sleeper};
use crate::ir::{
    Answer, Effort, Outcome, OutcomeStatus, PermissionMode, Question, QuestionId, QuestionKind,
    ResolvedEffortPolicy, Task, TaskId, TaskKind, Usage, WorkerProfile,
};
use crate::review;
use crate::rundir::{self, RunLock, RunPaths, WorktreeLock};
use crate::runner::CommandSpec;
use crate::topology::effects::EventSite;
use crate::workspace::Workspace;

#[derive(Clone, Copy, PartialEq)]
enum Effect {
    EditFile,
    EditTest,
    LargeEdit,
    OpaqueEdit,
    IgnoredGateInput,
    FrozenCandidate,
    JamCleanupAfterReview,
    LargeEditQuestionWriteFailure,
    NoEdit,
    SpawnError,
    Error,
    RateLimited,
    AskQuestion,
    Exit,
}

const CRASH_EXIT_CODE: i32 = 42;

#[derive(Clone, Copy, PartialEq)]
enum ReviewBehavior {
    Pass,
    Fail,
    Unparseable,
    RateLimited,
    SpawnError,
    NeedsHuman,
}

struct FakeAdapter {
    id: &'static str,
    effects: Vec<Effect>,
    reviews: Vec<ReviewBehavior>,
    probe_error: Option<&'static str>,
    reports_cost: bool,
    calls: Mutex<Calls>,
}

#[derive(Default)]
struct Calls {
    worker: usize,
    review: usize,
    review_spawn_failures: usize,
    runs: Vec<RecordedRun>,
    review_snapshots: Vec<(String, String)>,
}

#[derive(Clone)]
struct RecordedRun {
    model: String,
    resume: Option<String>,
    prompt: String,
}

const REVIEW_MARKER: &str = "UPSTROKE-FAKE-REVIEW";

impl FakeAdapter {
    fn new(effects: Vec<Effect>, reviews: Vec<ReviewBehavior>) -> Self {
        Self {
            id: "claude-code",
            effects,
            reviews,
            probe_error: None,
            reports_cost: true,
            calls: Mutex::new(Calls::default()),
        }
    }

    fn copilot(reviews: Vec<ReviewBehavior>) -> Self {
        Self {
            id: "copilot",
            effects: Vec::new(),
            reviews,
            probe_error: None,
            reports_cost: true,
            calls: Mutex::new(Calls::default()),
        }
    }

    fn broken(mut self, message: &'static str) -> Self {
        self.probe_error = Some(message);
        self
    }

    fn unpriced(mut self) -> Self {
        self.reports_cost = false;
        self
    }

    fn runs(&self) -> Vec<RecordedRun> {
        self.calls
            .lock()
            .map(|c| c.runs.clone())
            .unwrap_or_default()
    }

    fn reviews_run(&self) -> usize {
        self.calls.lock().map(|c| c.review).unwrap_or_default()
    }

    fn review_spawn_failures(&self) -> usize {
        self.calls
            .lock()
            .map(|c| c.review_spawn_failures)
            .unwrap_or_default()
    }

    fn review_snapshots(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .map(|calls| calls.review_snapshots.clone())
            .unwrap_or_default()
    }
}

fn fake(effect: Effect) -> FakeSource {
    source(vec![effect], vec![ReviewBehavior::Pass])
}

fn source(effects: Vec<Effect>, reviews: Vec<ReviewBehavior>) -> FakeSource {
    FakeSource {
        adapter: FakeAdapter::new(effects, reviews),
        copilot: None,
    }
}

fn cross_vendor(
    effects: Vec<Effect>,
    reviews: Vec<ReviewBehavior>,
    second: Vec<ReviewBehavior>,
) -> FakeSource {
    FakeSource {
        adapter: FakeAdapter::new(effects, reviews),
        copilot: Some(FakeAdapter::copilot(second)),
    }
}

fn scripted<T: Copy>(script: &[T], index: usize, fallback: T) -> T {
    script
        .get(index)
        .copied()
        .or_else(|| script.last().copied())
        .unwrap_or(fallback)
}

impl AgentAdapter for FakeAdapter {
    fn id(&self) -> &'static str {
        self.id
    }

    fn probe(&self, _runner: &dyn crate::runner::Runner) -> Result<Caps, UpstrokeError> {
        if let Some(message) = self.probe_error {
            return Err(UpstrokeError::Agent {
                message: message.to_owned(),
            });
        }
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

    fn build(&self, run: &TaskRun) -> Result<CommandSpec, UpstrokeError> {
        if run.profile.permissions == PermissionMode::ReadOnly {
            let effect = self
                .calls
                .lock()
                .map(|calls| {
                    scripted(
                        &self.effects,
                        calls.worker.saturating_sub(1),
                        Effect::EditFile,
                    )
                })
                .unwrap_or(Effect::EditFile);
            let behavior = {
                let mut calls = self.calls.lock().map_err(|_| UpstrokeError::Agent {
                    message: "fake adapter lock poisoned".to_owned(),
                })?;
                let index = calls.review + calls.review_spawn_failures;
                let behavior = scripted(&self.reviews, index, ReviewBehavior::Pass);
                if behavior == ReviewBehavior::SpawnError {
                    calls.review_spawn_failures += 1;
                }
                behavior
            };
            if behavior == ReviewBehavior::SpawnError {
                return Ok(CommandSpec::new(
                    run.workspace
                        .join("missing-reviewer-executable")
                        .to_string_lossy(),
                ));
            }
            if effect == Effect::FrozenCandidate {
                let tree = Command::new("git")
                    .arg("-C")
                    .arg(&run.workspace)
                    .args(["rev-parse", "HEAD^{tree}"])
                    .output()
                    .map_err(|e| UpstrokeError::Agent {
                        message: format!("fake could not inspect reviewer tree: {e}"),
                    })?;
                if !tree.status.success() {
                    return Err(UpstrokeError::Agent {
                        message: format!(
                            "fake could not inspect reviewer tree: {}",
                            String::from_utf8_lossy(&tree.stderr).trim()
                        ),
                    });
                }
                let contents =
                    fs::read_to_string(run.workspace.join("agent-output.txt")).map_err(|e| {
                        UpstrokeError::Agent {
                            message: format!("fake could not inspect reviewer candidate: {e}"),
                        }
                    })?;
                self.calls
                    .lock()
                    .map_err(|_| UpstrokeError::Agent {
                        message: "fake adapter lock poisoned".to_owned(),
                    })?
                    .review_snapshots
                    .push((
                        String::from_utf8_lossy(&tree.stdout).trim().to_owned(),
                        contents,
                    ));
            }
            if effect == Effect::JamCleanupAfterReview {
                let common = Command::new("git")
                    .arg("-C")
                    .arg(&run.workspace)
                    .args(["rev-parse", "--git-common-dir"])
                    .output()
                    .map_err(|e| UpstrokeError::Agent {
                        message: format!("fake could not inspect git common dir: {e}"),
                    })?;
                if !common.status.success() {
                    return Err(UpstrokeError::Agent {
                        message: format!(
                            "fake could not inspect git common dir: {}",
                            String::from_utf8_lossy(&common.stderr).trim()
                        ),
                    });
                }
                let common = PathBuf::from(String::from_utf8_lossy(&common.stdout).trim());
                let common = if common.is_absolute() {
                    common
                } else {
                    run.workspace.join(common)
                };
                fs::write(common.join("index.lock"), "jam\n").map_err(|e| {
                    UpstrokeError::Agent {
                        message: format!("fake could not jam cleanup: {e}"),
                    }
                })?;
            }
            return Ok(shell_spec(&format!("echo {REVIEW_MARKER}")));
        }
        let index = {
            let mut calls = self.calls.lock().map_err(|_| UpstrokeError::Agent {
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
        let edit: Option<(&str, String)> = match scripted(&self.effects, index, Effect::EditFile) {
            Effect::Exit => {
                let _ = fs::write(
                    run.workspace.join("agent-output.txt"),
                    "half-written by an agent that never came back\n",
                );
                std::process::exit(CRASH_EXIT_CODE);
            }
            Effect::EditFile
            | Effect::AskQuestion
            | Effect::JamCleanupAfterReview
            | Effect::FrozenCandidate => {
                let marker = run.workspace.join("agent-output.txt");
                let previous = fs::read_to_string(&marker).unwrap_or_default();
                Some(("agent-output.txt", format!("{previous}edited: {index}\n")))
            }
            Effect::EditTest => Some((
                "widget_test.rs",
                "#[test]\nfn widget_works() {\n    assert!(true);\n}\n".to_owned(),
            )),
            Effect::LargeEdit | Effect::LargeEditQuestionWriteFailure => Some((
                "large-agent-output.txt",
                "x".repeat(review::MAX_DIFF_BYTES + 1),
            )),
            Effect::OpaqueEdit => Some(("opaque-agent-output.bin", "\0hidden bytes".to_owned())),
            Effect::IgnoredGateInput => {
                fs::write(run.workspace.join(".gitignore"), "ignored.flag\n").map_err(|e| {
                    UpstrokeError::Agent {
                        message: format!("fake ignore rule failed: {e}"),
                    }
                })?;
                fs::write(run.workspace.join("ignored.flag"), "gate-only input\n").map_err(
                    |e| UpstrokeError::Agent {
                        message: format!("fake ignored input failed: {e}"),
                    },
                )?;
                Some(("agent-output.txt", "reviewed edit\n".to_owned()))
            }
            Effect::SpawnError => {
                return Ok(CommandSpec::new(
                    run.workspace
                        .join("missing-worker-executable")
                        .to_string_lossy(),
                ));
            }
            Effect::NoEdit | Effect::Error | Effect::RateLimited => None,
        };
        if let Some((name, content)) = edit {
            fs::write(run.workspace.join(name), content).map_err(|e| UpstrokeError::Agent {
                message: format!("fake edit failed: {e}"),
            })?;
        }
        if scripted(&self.effects, index, Effect::EditFile) == Effect::LargeEditQuestionWriteFailure
        {
            let run_id =
                rundir::latest_run(&run.workspace).ok_or_else(|| UpstrokeError::Agent {
                    message: "fake could not find the active run".to_owned(),
                })?;
            let questions = rundir::public_dir(&run.workspace, &run_id).join("questions");
            fs::remove_dir(&questions).map_err(|e| UpstrokeError::Agent {
                message: format!("fake could not remove questions directory: {e}"),
            })?;
            fs::write(&questions, "not a directory\n").map_err(|e| UpstrokeError::Agent {
                message: format!("fake could not block question writes: {e}"),
            })?;
        }
        Ok(shell_spec("exit 0"))
    }

    fn materialize_permissions(
        &self,
        profile: &WorkerProfile,
        gate_cmds: &[String],
        dir: &Path,
        stem: &str,
    ) -> Result<Option<PathBuf>, UpstrokeError> {
        crate::agent::claude::ClaudeCodeAdapter
            .materialize_permissions(profile, gate_cmds, dir, stem)
    }

    fn parse(&self, out: &ProcessOutput) -> Result<Outcome, UpstrokeError> {
        if out.stdout.contains(REVIEW_MARKER) {
            let index = {
                let mut calls = self.calls.lock().map_err(|_| UpstrokeError::Agent {
                    message: "fake adapter lock poisoned".to_owned(),
                })?;
                let index = calls.review + calls.review_spawn_failures;
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
                    "```json\n{\"pass\": true, \"reasons\": [\"meets the acceptance \
                         criteria\"], \"required_changes\": []}\n```"
                }
                ReviewBehavior::Fail => {
                    "```json\n{\"pass\": false, \"reasons\": [\"no error handling for \
                         empty input\"], \"required_changes\": \
                         [\"handle the empty-input case\"]}\n```"
                }
                ReviewBehavior::NeedsHuman => {
                    "```json\n{\"pass\": false, \"reasons\": [\"the acceptance criteria \
                         contradict the API contract\"], \"needs_human\": true}\n```"
                }
                ReviewBehavior::Unparseable => "Looks fine to me, ship it.",
                ReviewBehavior::RateLimited => unreachable!("handled above"),
                ReviewBehavior::SpawnError => unreachable!("handled during command build"),
            };
            return Ok(fake_outcome(
                OutcomeStatus::Completed,
                Some(answer.to_owned()),
                "fake-review-session",
                self.reports_cost.then_some(0.05),
                out.duration,
            ));
        }
        let index = self
            .calls
            .lock()
            .map(|c| c.worker.saturating_sub(1))
            .unwrap_or(0);
        let effect = scripted(&self.effects, index, Effect::EditFile);
        let status = match effect {
            Effect::Error => OutcomeStatus::AgentError,
            Effect::RateLimited => OutcomeStatus::RateLimited,
            Effect::EditFile
            | Effect::EditTest
            | Effect::LargeEdit
            | Effect::OpaqueEdit
            | Effect::IgnoredGateInput
            | Effect::FrozenCandidate
            | Effect::JamCleanupAfterReview
            | Effect::LargeEditQuestionWriteFailure
            | Effect::NoEdit
            | Effect::AskQuestion
            | Effect::SpawnError
            | Effect::Exit => OutcomeStatus::Completed,
        };
        let detail = match effect {
            Effect::Error => Some("fake adapter error detail".to_owned()),
            Effect::RateLimited => Some("5-hour limit reached".to_owned()),
            Effect::AskQuestion => Some(
                "I made a start but stopped.\nUPSTROKE-QUESTION: should cursors be opaque or \
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
        transcript_path: PathBuf::new(),
        duration,
    }
}

struct FakeSource {
    adapter: FakeAdapter,
    copilot: Option<FakeAdapter>,
}

impl FakeSource {
    fn copilot(&self) -> &FakeAdapter {
        self.copilot.as_ref().expect("this source has a copilot")
    }
}

impl AdapterSource for FakeSource {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        if id == self.adapter.id {
            return Some(&self.adapter as &dyn AgentAdapter);
        }
        self.copilot
            .as_ref()
            .filter(|a| a.id == id)
            .map(|a| a as &dyn AgentAdapter)
    }
}

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

    fn resolve(&self, _question: &Question) -> Result<Answer, UpstrokeError> {
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

fn shell_spec(script: &str) -> CommandSpec {
    crate::gates::ShellKind::native().spec(script)
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

fn candidate_mutation_marker(repo: &Path) -> PathBuf {
    let name = repo
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_owned());
    repo.with_file_name(format!("{name}-candidate-mutation.txt"))
}

fn mutate_index_after_candidate_capture(
    workspace: &Workspace,
    candidate: &crate::workspace::CapturedCandidate,
) -> Result<(), UpstrokeError> {
    fs::write(
        workspace.root().join("agent-output.txt"),
        "tampered after capture\n",
    )
    .map_err(|error| UpstrokeError::Git {
        message: format!("test could not mutate the captured worktree: {error}"),
    })?;
    let add = Command::new("git")
        .arg("-C")
        .arg(workspace.root())
        .args(["add", "-A"])
        .output()
        .map_err(|error| UpstrokeError::Git {
            message: format!("test could not stage its post-capture mutation: {error}"),
        })?;
    if !add.status.success() {
        return Err(UpstrokeError::Git {
            message: format!(
                "test could not stage its post-capture mutation: {}",
                String::from_utf8_lossy(&add.stderr).trim()
            ),
        });
    }
    let tampered_tree = Command::new("git")
        .arg("-C")
        .arg(workspace.root())
        .arg("write-tree")
        .output()
        .map_err(|error| UpstrokeError::Git {
            message: format!("test could not inspect its post-capture tree: {error}"),
        })?;
    if !tampered_tree.status.success() {
        return Err(UpstrokeError::Git {
            message: format!(
                "test could not inspect its post-capture tree: {}",
                String::from_utf8_lossy(&tampered_tree.stderr).trim()
            ),
        });
    }
    let tampered_tree = String::from_utf8_lossy(&tampered_tree.stdout)
        .trim()
        .to_owned();
    if tampered_tree == candidate.tree_oid {
        return Err(UpstrokeError::Git {
            message: "test post-capture mutation did not change the staged tree".to_owned(),
        });
    }
    fs::write(
        candidate_mutation_marker(workspace.root()),
        format!(
            "{}\n{}\n{tampered_tree}\n",
            candidate.parent_oid, candidate.tree_oid
        ),
    )
    .map_err(|error| UpstrokeError::Git {
        message: format!("test could not record its capture identities: {error}"),
    })
}

fn temp_engine_repo(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("upstroke-engine-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create repo dir");
    git_in(&dir, &["init", "-q", "-b", "main"]);
    git_in(&dir, &["config", "user.email", "test@upstroke.local"]);
    git_in(&dir, &["config", "user.name", "upstroke tests"]);
    fs::write(dir.join("README.md"), "seed\n").expect("seed");
    fs::write(
        dir.join("plan.md"),
        "## Implement the widget\n<!-- upstroke: id=t1 depends= -->\nMake it.\n\n\
             ## Document the widget\n<!-- upstroke: id=t2 depends=t1 -->\nWrite it up.\n",
    )
    .expect("plan");
    git_in(&dir, &["add", "-A"]);
    git_in(&dir, &["commit", "-q", "-m", "seed"]);
    dir
}

fn seed(repo: &Path, plan: &str, config: Option<&str>) {
    fs::write(repo.join("plan.md"), plan).expect("plan");
    if let Some(config) = config {
        fs::write(repo.join("upstroke.toml"), config).expect("config");
    }
    git_in(repo, &["add", "-A"]);
    git_in(repo, &["commit", "-q", "-m", "fixture"]);
}

fn options(repo: &Path) -> RunOptions {
    let mut opts = RunOptions::new(repo.join("plan.md"), repo.to_path_buf());
    opts.pools_path = Some(no_pools());
    opts.attempt_timeout = Duration::from_secs(60);
    opts.defer_backoff = Duration::ZERO;
    opts.wait_on_block = Some(Duration::ZERO);
    opts.private_root = Some(private_root_for(repo));
    opts
}

fn no_pools() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("upstroke-engine-nopools-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("pools.toml");
        fs::write(
            &path,
            "# no pools
",
        )
        .expect("empty pools file");
        path
    })
    .clone()
}

fn private_root_for(repo: &Path) -> PathBuf {
    let name = repo
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "run".to_owned());
    repo.with_file_name(format!("{name}-home"))
}

fn resume_options(repo: &Path, run_id: &str) -> ResumeOptions {
    let mut opts = ResumeOptions::new(run_id.to_owned(), repo.to_path_buf());
    opts.pools_path = Some(no_pools());
    opts.attempt_timeout = Duration::from_secs(60);
    opts.defer_backoff = Duration::ZERO;
    opts.wait_on_block = Some(Duration::ZERO);
    opts.private_root = Some(private_root_for(repo));
    opts
}

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

#[derive(Default)]
struct FailTheThirdLegacyAppend {
    entered: u32,
}

impl crate::events::log::EventHooks for FailTheThirdLegacyAppend {
    fn point(
        &mut self,
        site: EventSite,
        point: crate::topology::effects::SubEffectPoint,
        mode: crate::topology::effects::InjectionMode,
    ) -> crate::topology::effects::Injection {
        use crate::topology::effects::{Injection, InjectionMode, SubEffectPoint};
        if site != EventSite::LegacyAppend
            || point != SubEffectPoint::Written
            || mode != InjectionMode::ErrorReturn
        {
            return Injection::Proceed;
        }
        self.entered += 1;
        if self.entered == 3 {
            Injection::Error
        } else {
            Injection::Proceed
        }
    }
}

fn fail_the_third_legacy_append() -> Box<dyn crate::events::log::EventHooks> {
    Box::<FailTheThirdLegacyAppend>::default()
}

#[test]
fn a_returned_legacy_append_error_stops_the_run() {
    let repo = temp_engine_repo("legacy-append-error");
    let mut opts = options(&repo);
    opts.log_hooks = Some(fail_the_third_legacy_append);
    let source = fake(Effect::EditFile);

    let error = run_with(&opts, &source)
        .expect_err("a returned append error must reach the caller, not be swallowed");
    let message = error.to_string();
    assert!(
        message.contains("Event.LegacyAppend"),
        "the error must be the append's own, naming its site: {message}"
    );
    assert!(
        message.contains("Written"),
        "…and its point, so an operator can tell which coordinate failed: {message}"
    );
}

#[test]
fn a_returned_legacy_append_error_still_leaves_the_partial_report() {
    let repo = temp_engine_repo("legacy-append-partial");
    let mut opts = options(&repo);
    opts.log_hooks = Some(fail_the_third_legacy_append);
    let source = fake(Effect::EditFile);

    run_with(&opts, &source).expect_err("the append error stops the run");

    let runs = opts.repo_root.join(".upstroke").join("runs");
    let public = fs::read_dir(&runs)
        .expect("the runs root")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.join("events.jsonl").is_file())
        .expect("the failed run left its public directory and its log");
    let log = fs::read_to_string(public.join("events.jsonl")).expect("the log");
    let complete = log.lines().filter(|line| line.ends_with('}')).count();
    assert!(
        complete >= 2,
        "only {complete} complete line(s) in the log: the injected failure landed on \
         a startup append, so this test never reached drain_and_report's branch"
    );

    let report = public.join("report.json");
    assert!(
        report.is_file(),
        "no report beside {}: the legacy engine's partial report is a courtesy for \
         whoever opens the directory next, and failing to write it must not be \
         silent (PR5-CONF-011)",
        public.display()
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report).expect("read the partial report"))
            .expect("the partial report is JSON");
    assert!(
        parsed.get("tasks").is_some(),
        "the partial report is a report, not a stub: {parsed}"
    );
}

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
    assert!(
        (report.total_cost_usd - 0.12).abs() < 1e-9,
        "worker and reviewer spend both counted: {}",
        report.total_cost_usd
    );

    let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert!(branch.trim().starts_with("upstroke/run-"), "on run branch");
    let count = git_in(&repo, &["rev-list", "--count", "main..HEAD"]);
    assert_eq!(count.trim(), "2", "one commit per task");
    let log = git_in(&repo, &["log", "--format=%s", "main..HEAD"]);
    assert!(
        log.contains("[upstroke] t1: Implement the widget"),
        "log: {log}"
    );
    assert!(log.contains("[upstroke] t2: Document the widget"));
    assert!(
        git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
        "clean tree after run"
    );
    assert!(
        repo.join(".upstroke").join("runs").exists(),
        "run dir written"
    );
}

#[test]
fn gates_review_and_commit_use_one_frozen_candidate_tree() {
    let repo = temp_engine_repo("one-frozen-candidate");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [[gates]]\nname = \"frozen-candidate\"\n\
                 cmd = 'git grep -q \"edited: 0\" -- agent-output.txt'\n",
        ),
    );
    let base = git_in(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    let marker = candidate_mutation_marker(&repo);
    let _ = fs::remove_file(&marker);
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    opts.after_candidate_capture = Some(mutate_index_after_candidate_capture);
    let source = fake(Effect::FrozenCandidate);

    let report = run_with(&opts, &source).expect("the frozen candidate remains authoritative");
    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
    assert_eq!(report.gates, ["frozen-candidate"]);

    let capture: Vec<_> = fs::read_to_string(&marker)
        .expect("the post-capture mutation hook ran")
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(capture.len(), 3, "capture marker: {capture:?}");
    assert_eq!(capture[0], base, "captured parent is the attempt parent");
    assert_ne!(
        capture[1], capture[2],
        "the mutable index really changed after capture"
    );

    let logged = events_of(&repo, &report.run_id);
    let prepared = logged
        .iter()
        .find_map(|event| match &event.body {
            EventBody::AttemptFinished {
                prepared_commit, ..
            } => prepared_commit.as_deref().cloned(),
            _ => None,
        })
        .expect("successful settlement records its prepared object");
    assert_eq!(
        prepared.branch_ref,
        format!("refs/heads/{}", report.branch),
        "the durable settlement owns the exact run ref"
    );
    assert_eq!(prepared.parent_sha, capture[0]);
    assert_eq!(prepared.tree_sha, capture[1]);
    assert_ne!(prepared.tree_sha, capture[2]);

    let review_snapshots = source.adapter.review_snapshots();
    assert_eq!(review_snapshots.len(), 1, "one reviewer snapshot");
    assert_eq!(review_snapshots[0].0, prepared.tree_sha);
    assert_eq!(
        review_snapshots[0].1.replace("\r\n", "\n"),
        "edited: 0\n",
        "review sees the captured tree, not the later staged mutation"
    );

    let committed = logged
        .iter()
        .find_map(|event| match &event.body {
            EventBody::TaskCommitted { data, .. } => Some(data),
            _ => None,
        })
        .expect("task_committed follows the prepared settlement");
    let head = git_in(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    let head_tree = git_in(&repo, &["rev-parse", "HEAD^{tree}"])
        .trim()
        .to_owned();
    assert_eq!(head, prepared.commit_sha);
    assert_eq!(committed.sha, prepared.commit_sha);
    assert_eq!(head_tree, prepared.tree_sha);
    assert_eq!(
        git_in(&repo, &["show", "HEAD:agent-output.txt"]),
        "edited: 0\n",
        "the staged post-capture mutation is never published"
    );
}

#[test]
fn an_oversized_review_diff_is_settled_once_before_the_task_parks() {
    let repo = temp_engine_repo("oversizedreviewsettlement");
    seed(
        &repo,
        "## Generate the large fixture\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 3 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::LargeEdit);
    let report = run_with(&opts, &source).expect("policy failure is a settled run outcome");

    assert_eq!(report.outcome(), RunOutcome::Parked, "{report:?}");
    let task_report = task(&report, "t1");
    assert_eq!(
        task_report.attempts.len(),
        1,
        "the policy boundary is not retried"
    );
    let attempt = &task_report.attempts[0];
    let failure = attempt.failure.as_ref().expect("settled policy failure");
    assert_eq!(failure.kind, FailureKind::ReviewInputTooLarge);
    assert_eq!(failure.origin, FailureOrigin::Reviewer);
    assert_eq!(attempt.cost_usd, Some(0.01), "worker spend is retained");
    assert_eq!(attempt.session_id.as_deref(), Some("s0"));
    assert!(attempt.usage.is_some(), "worker usage is retained");
    assert!(attempt.reviews.is_empty(), "no reviewer was dispatched");
    assert_eq!(source.adapter.reviews_run(), 0);

    let logged = events_of(&repo, &report.run_id);
    assert_eq!(
        logged
            .iter()
            .filter(|event| matches!(event.body, EventBody::AttemptFinished { .. }))
            .count(),
        1,
        "the attempt has a terminal ledger event"
    );
    let parking = logged.iter().find_map(|event| match &event.body {
        EventBody::AttemptFinished { parking, .. } => parking.as_deref(),
        _ => None,
    });
    assert!(
        parking.is_some(),
        "the settlement atomically carries its parking question"
    );
    assert!(
        !logged.iter().any(|event| matches!(
            event.body,
            EventBody::QuestionRaised { .. } | EventBody::TaskParked { .. }
        )),
        "policy parking must not reopen a crash window with follow-up events"
    );
    assert!(
        !logged
            .iter()
            .any(|event| matches!(event.body, EventBody::AttemptInterrupted { .. })),
        "replay must never invent an interruption for the settled refusal"
    );
    assert!(
        git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
        "parking cleans the unreviewed oversized diff"
    );
    let question = report.questions.first().expect("scope question");
    assert_eq!(question.question.kind, QuestionKind::Unblock);
    assert!(question.question.context.contains("smaller diff"));
    assert!(question.question.context.contains("starting a new run"));
    assert!(
        !question.question.context.contains("chain is spent")
            && !question.question.context.contains("all failed"),
        "policy parking must not pretend the escalation chain was exhausted: {}",
        question.question.context
    );

    let paths = paths_of(&repo, &report.run_id);
    truncate_log_after(&paths, "attempt_finished");
    fs::write(repo.join("crash-residue.txt"), "unreviewed\n").expect("crash residue");
    let retry = fake(Effect::EditFile);
    let resumed =
        resume_with(&resume_options(&repo, &report.run_id), &retry).expect("resume parks");
    assert_eq!(resumed.outcome(), RunOutcome::Parked, "{resumed:?}");
    assert!(
        retry.adapter.runs().is_empty(),
        "resume paid for the oversized attempt again"
    );
    assert_eq!(task(&resumed, "t1").attempts.len(), 1);
    assert_eq!(resumed.questions.len(), 1);
    assert!(
        git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
        "resume did not discard crash residue"
    );
}

#[test]
fn opaque_review_input_has_distinct_failure_and_remediation() {
    let repo = temp_engine_repo("opaquereviewsettlement");
    seed(
        &repo,
        "## Generate an opaque artifact\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 3 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::OpaqueEdit);
    let report = run_with(&opts, &source).expect("opaque evidence parks fail-closed");

    assert_eq!(report.outcome(), RunOutcome::Parked, "{report:?}");
    let attempt = &task(&report, "t1").attempts[0];
    assert_eq!(
        attempt.failure.as_ref().map(|failure| failure.kind),
        Some(FailureKind::ReviewInputOpaque)
    );
    assert_eq!(source.adapter.reviews_run(), 0);
    let context = &report.questions[0].question.context;
    assert!(context.contains("hides changed content"), "{context}");
    assert!(!context.contains("smaller diff"), "{context}");
}

#[test]
fn opaque_test_task_parks_before_test_provenance_retry() {
    let repo = temp_engine_repo("opaquetestprovenance");
    seed(
        &repo,
        "## Add the regression\n<!-- upstroke: id=t1 kind=test depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\ntest = { chain = [\"small\"], attempts_per = 3 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::OpaqueEdit);
    let report = run_with(&opts, &source).expect("opaque evidence parks fail-closed");

    assert_eq!(report.outcome(), RunOutcome::Parked, "{report:?}");
    let task = task(&report, "t1");
    assert_eq!(task.attempts.len(), 1, "opaque evidence is not retried");
    assert_eq!(
        task.attempts[0]
            .failure
            .as_ref()
            .map(|failure| failure.kind),
        Some(FailureKind::ReviewInputOpaque),
        "the intrinsic evidence failure wins over Test provenance"
    );
    assert_eq!(source.adapter.reviews_run(), 0);
    assert!(
        report.questions[0]
            .question
            .context
            .contains("hides changed content")
    );
}

#[test]
fn failed_parking_payload_still_settles_and_cleans_the_attempt() {
    let repo = temp_engine_repo("oversizedreviewquestionwrite");
    seed(
        &repo,
        "## Generate the large fixture\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 3 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::LargeEditQuestionWriteFailure);
    let error = run_with(&opts, &source).expect_err("question projection must fail");
    assert!(
        error.to_string().contains("questions"),
        "wrong failure surfaced: {error}"
    );

    let run_id = rundir::latest_run(&repo).expect("failed run remains resumable");
    let logged = events_of(&repo, &run_id);
    let parking = logged.iter().find_map(|event| match &event.body {
        EventBody::AttemptFinished { parking, .. } => parking.as_deref(),
        _ => None,
    });
    assert!(
        parking.is_some(),
        "the event must retain parking even when its JSON projection fails"
    );
    assert_eq!(
        logged
            .iter()
            .filter(|event| matches!(event.body, EventBody::AttemptFinished { .. }))
            .count(),
        1,
        "the paid attempt must settle exactly once"
    );
    assert!(
        git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
        "a failed question write leaked the oversized unreviewed diff"
    );

    let paths = paths_of(&repo, &run_id);
    fs::remove_file(paths.questions()).expect("remove injected blocker");
    fs::create_dir(paths.questions()).expect("restore questions directory");
    let retry = fake(Effect::EditFile);
    let resumed = resume_with(&resume_options(&repo, &run_id), &retry)
        .expect("resume repairs the projection and remains parked");
    assert_eq!(resumed.outcome(), RunOutcome::Parked, "{resumed:?}");
    assert!(
        retry.adapter.runs().is_empty(),
        "resume paid for an already-settled attempt"
    );
    assert_eq!(task(&resumed, "t1").attempts.len(), 1);
    let question = resumed.questions.first().expect("restored question");
    assert!(
        interaction::answer_path(&paths.questions(), &question.question.id).exists(),
        "resume did not rematerialize the authoritative question"
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
fn sparse_checkout_preflight_refusal_leaves_worktree_clean() {
    let repo = temp_engine_repo("sparse-worker-preflight");
    git_in(&repo, &["update-index", "--skip-worktree", "README.md"]);
    let source = fake(Effect::EditFile);

    let error = run_with(&options(&repo), &source)
        .expect_err("incomplete materialization must be refused")
        .to_string();
    assert!(error.contains("sparse checkout is active"), "{error}");
    assert!(
        source.adapter.runs().is_empty(),
        "a worker was dispatched before sparse-checkout refusal"
    );
    assert_eq!(
        git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main",
        "preflight refusal must not create or switch a run branch"
    );
    git_in(&repo, &["update-index", "--no-skip-worktree", "README.md"]);
    let worktree_git_dir = Workspace::open(&repo)
        .expect("open worktree")
        .worktree_git_dir()
        .expect("resolve private git dir");
    assert!(
        worktree_git_dir.join("upstroke-worktree.lock").exists(),
        "the regression must exercise acquisition of the private worktree lease"
    );
    assert!(
        !repo.join(".upstroke").exists(),
        "a refused preflight must not create working-tree coordinator state"
    );
    assert!(
        git_in(&repo, &["status", "--porcelain", "--untracked-files=all"])
            .trim()
            .is_empty(),
        "a refused preflight left coordinator state visible to Git"
    );
}

#[test]
fn passing_configured_gates_commit_and_are_reported() {
    let repo = temp_engine_repo("gatepass");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 depends= -->\n",
        Some("[[gates]]\nname = \"version\"\ncmd = \"git --version\"\n"),
    );

    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::EditFile);
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
    assert_eq!(report.gates, ["version"]);
    assert!(report.gates_from_config);
    assert!(report.render().contains("gates: version [from config]"));
}

#[test]
fn ignored_worker_input_cannot_make_a_gate_pass() {
    let repo = temp_engine_repo("ignored-gate-input");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [[gates]]\nname = \"ignored-input\"\ncmd = \"git hash-object ignored.flag\"\n",
        ),
    );

    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::IgnoredGateInput);
    let report = run_with(&opts, &source).expect("gate failure settles the task");

    assert!(!committed(&report, "t1"), "report: {report:?}");
    assert!(task(&report, "t1").attempts.iter().any(|attempt| {
        attempt
            .failure
            .as_ref()
            .is_some_and(|failure| failure.kind == FailureKind::GateFailed)
    }));
    assert!(
        !repo.join("ignored.flag").exists(),
        "ignored worker-only input was cleaned from the authoritative workspace"
    );
}

#[test]
fn unresolvable_gate_refuses_at_preflight() {
    let repo = temp_engine_repo("gateresolve");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 depends= -->\n",
        Some("[[gates]]\nname = \"ghost\"\ncmd = \"definitely-not-a-real-tool-xyz build\"\n"),
    );

    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Test the widget\n<!-- upstroke: id=tt depends= -->\nAdd coverage.\n",
        Some("[routing]\ntest = { chain = [\"small\"], attempts_per = 1 }\n"),
    );

    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Test the widget\n<!-- upstroke: id=tt depends= -->\n",
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
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 depends= -->\n",
        Some("[[gates]]\nname = \"leaky\"\ncmd = \"echo residue> residue.txt\"\n"),
    );

    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Implement the widget\n<!-- upstroke: id=t1 depends= -->\n",
        Some("[routing]\nreview = { enabled = false }\n"),
    );

    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
    assert!(report.tasks[0].review_models.is_empty());
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
    assert_eq!(t1.review_models, ["claude-opus-5"]);
    assert!((t1.total_cost_usd().expect("both") - 0.06).abs() < 1e-9);
    let rendered = report.render();
    assert!(rendered.contains("+ review claude-opus-5"), "{rendered}");
}

const FRONTIER_AUTH_PLAN: &str = "## Rotate the signing key\n\
         <!-- upstroke: id=t1 kind=implement depends= tier=frontier paths=src/auth/** -->\n\
         Rotate it.\n";

const SECOND_OPINION_CONFIG: &str = "[routing]\n\
         implement = { chain = [\"frontier\"], attempts_per = 1 }\n\n\
         [[routing.overrides]]\n\
         paths = [\"src/auth/**\"]\n\
         second_opinion = \"different-vendor\"\n";

const FRONTIER_ONLY_CONFIG: &str =
    "[routing]\nimplement = { chain = [\"frontier\"], attempts_per = 1 }\n";

fn cross_vendor_opts(repo: &Path) -> RunOptions {
    let mut opts = options(repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    opts
}

#[test]
fn a_second_opinion_runs_a_second_family_and_leaves_the_primary_alone() {
    let repo = temp_engine_repo("secondopinion");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Pass],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

    assert!(committed(&report, "t1"), "both passed: {report:?}");
    let t1 = task(&report, "t1");
    assert_eq!(
        t1.review_models,
        ["claude-opus-5", "gpt-5.3-codex"],
        "one pass per family, primary first"
    );
    assert_eq!(t1.model, "claude-opus-5", "written by the frontier model");
    assert_eq!(source.adapter.reviews_run(), 1);
    assert_eq!(source.copilot().reviews_run(), 1);
    assert_eq!(t1.review_cost_usd, Some(0.10), "0.05 per pass");
    assert_eq!(t1.cost_usd, Some(0.01), "implementer's own");
    let rendered = report.render();
    assert!(
        rendered.contains("+ review claude-opus-5, gpt-5.3-codex"),
        "{rendered}"
    );
}

#[test]
fn a_second_opinion_that_fails_fails_the_attempt() {
    let repo = temp_engine_repo("secondopinionfail");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Pass],
        vec![ReviewBehavior::Fail],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

    assert!(!committed(&report, "t1"), "a rejected change cannot commit");
    let t1 = task(&report, "t1");
    let last = t1.attempts.last().expect("an attempt ran");
    assert_eq!(
        last.failure.as_ref().map(|f| f.kind),
        Some(FailureKind::ReviewFailed)
    );
    assert_eq!(
        last.reviews.iter().map(|r| r.outcome).collect::<Vec<_>>(),
        [
            events::ReviewPassOutcome::Passed,
            events::ReviewPassOutcome::Failed
        ],
        "the record says which pass objected, and that it really judged"
    );
    assert_eq!(last.reviews[1].agent, "copilot");
}

#[test]
fn a_failing_first_pass_never_spends_the_second_reviewer() {
    let repo = temp_engine_repo("shortcircuit");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Fail],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

    assert!(!committed(&report, "t1"));
    assert_eq!(source.adapter.reviews_run(), 1);
    assert_eq!(
        source.copilot().reviews_run(),
        0,
        "the second vendor was never asked"
    );
    let last = task(&report, "t1").attempts.last().expect("attempt");
    assert_eq!(last.reviews.len(), 1, "only what actually ran is recorded");
}

#[test]
fn a_frontier_task_is_not_reviewed_by_the_model_that_wrote_it() {
    let repo = temp_engine_repo("selfreview");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(FRONTIER_ONLY_CONFIG));
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Fail],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

    assert!(committed(&report, "t1"), "{report:?}");
    assert_eq!(task(&report, "t1").review_models, ["gpt-5.3-codex"]);
    assert_eq!(source.adapter.reviews_run(), 0, "never judged its own work");
    assert_eq!(source.copilot().reviews_run(), 1);
}

#[test]
fn a_lower_rung_keeps_the_frontier_reviewer() {
    let repo = temp_engine_repo("noneedtorebind");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"mid\"], attempts_per = 1 }\n"),
    );
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Pass],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

    assert_eq!(task(&report, "t1").model, "claude-sonnet-5");
    assert_eq!(task(&report, "t1").review_models, ["claude-opus-5"]);
    assert_eq!(source.copilot().reviews_run(), 0);
}

#[test]
fn a_configured_second_opinion_with_no_second_family_refuses_before_spending() {
    let repo = temp_engine_repo("nosecondfamily");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
    let error = run_with(&cross_vendor_opts(&repo), &source)
        .expect_err("a promised reviewer that cannot exist must stop the run");
    let message = error.to_string();
    assert!(message.contains("t1"), "names the task: {message}");
    assert!(
        message.contains("src/auth/**"),
        "names the override: {message}"
    );
    assert!(
        message.contains("second opinion"),
        "says what is missing: {message}"
    );
    assert_eq!(source.adapter.runs().len(), 0, "nothing was spent");
}

#[test]
fn without_a_second_vendor_self_review_warns_rather_than_refusing() {
    let repo = temp_engine_repo("selfreviewwarn");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(FRONTIER_ONLY_CONFIG));
    let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run still works");

    assert!(committed(&report, "t1"));
    assert_eq!(task(&report, "t1").review_models, ["claude-opus-5"]);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("t1") && w.contains("also the reviewer")),
        "warnings: {:?}",
        report.warnings
    );
}

#[test]
fn an_unprobeable_cross_family_reviewer_downgrades_instead_of_halting() {
    let repo = temp_engine_repo("brokencopilot");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(FRONTIER_ONLY_CONFIG));
    let source = FakeSource {
        adapter: FakeAdapter::new(vec![Effect::EditFile], vec![ReviewBehavior::Pass]),
        copilot: Some(FakeAdapter::copilot(vec![ReviewBehavior::Pass]).broken("not logged in")),
    };
    let report =
        run_with(&cross_vendor_opts(&repo), &source).expect("a broken upgrade is not a broken run");

    assert!(committed(&report, "t1"));
    assert_eq!(
        task(&report, "t1").review_models,
        ["claude-opus-5"],
        "fell back to same-model review"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("not logged in") && w.contains("same-model review")),
        "warnings: {:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("t1") && w.contains("also the reviewer")),
        "warnings: {:?}",
        report.warnings
    );
}

#[test]
fn the_same_broken_reviewer_is_fatal_when_the_config_asked_for_it() {
    let repo = temp_engine_repo("brokenrequired");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = FakeSource {
        adapter: FakeAdapter::new(vec![Effect::EditFile], vec![ReviewBehavior::Pass]),
        copilot: Some(FakeAdapter::copilot(vec![ReviewBehavior::Pass]).broken("not logged in")),
    };
    let error = run_with(&cross_vendor_opts(&repo), &source)
        .expect_err("a required reviewer that cannot run stops the run");
    assert!(error.to_string().contains("not logged in"), "got: {error}");
}

#[test]
fn a_resume_keeps_the_reviewers_the_run_started_with() {
    let repo = temp_engine_repo("resumereviewers");
    seed(
        &repo,
        "## Rotate the signing key\n\
             <!-- upstroke: id=t1 kind=implement depends= tier=frontier -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"frontier\"], attempts_per = 1 }\n",
        ),
    );

    let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
    let first = run_with(&cross_vendor_opts(&repo), &source).expect("run");
    assert!(
        matches!(task(&first, "t1").status, TaskRunStatus::Parked { .. }),
        "{first:?}"
    );

    let paths = paths_of(&repo, &first.run_id);
    let recorded = {
        let mut warnings = Vec::new();
        let events = events::read_all(&paths.events(), &mut warnings).expect("log");
        events::started_of(&events, &paths.events())
            .expect("run_started")
            .reviews
            .clone()
    };
    let recorded = recorded.expect("step 9 records who reviews");
    assert_eq!(
        recorded.alternative, None,
        "there was nothing to rebind to when this run started"
    );
    assert_eq!(recorded.pass_timeout_secs, Some(5400));

    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"frontier\"], attempts_per = 1 }\n\
             review = { timeout_secs = 60 }\n",
    )
    .expect("edit only the future review timeout");

    crate::answer::answer(
        &repo,
        &first.questions[0].question.id.to_string(),
        crate::answer::Reply::Text("put the key in src/auth/keys.rs".to_owned()),
    )
    .expect("answer");

    let later = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Pass],
        vec![ReviewBehavior::Pass],
    );
    let resumed =
        resume_with(&resume_options(&repo, &first.run_id), &later).expect("resume continues");

    assert!(committed(&resumed, "t1"), "{resumed:?}");
    assert_eq!(
        task(&resumed, "t1").review_models,
        ["claude-opus-5"],
        "the recorded reviewer judged the resumed attempt"
    );
    assert_eq!(
        later.copilot().reviews_run(),
        0,
        "a CLI installed since the run began must not become its judge"
    );
    let warning = resumed
        .warnings
        .iter()
        .find(|warning| warning.contains("review pass timeout"))
        .unwrap_or_else(|| panic!("no timeout-difference warning: {:?}", resumed.warnings));
    assert!(warning.contains("60s"), "{warning}");
    assert!(warning.contains("5400s"), "{warning}");
    assert!(warning.contains("Start a new run"), "{warning}");
}

#[test]
fn resume_runs_with_the_effort_policy_the_run_recorded_not_todays_config() {
    let original = "[interaction]\nmode = \"never\"\n\n\
                        [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                        [routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n";
    let (repo, run_id) = parked_run_with_config("resumeeffort", original);
    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
             [routing.effort]\nimplementation = \"low\"\nreview = \"high\"\n",
    )
    .expect("edit effort only");

    let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    let logged = events_of(&repo, &run_id);
    let resumed_worker = logged
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::AttemptStarted { data, .. } => Some(data),
            _ => None,
        })
        .next_back()
        .expect("resumed worker start");
    assert_eq!(resumed_worker.effort, Some(Effort::XHigh));
    let resumed_reviews = logged
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::AttemptFinished { data, .. } if !data.reviews.is_empty() => {
                Some(&data.reviews)
            }
            _ => None,
        })
        .next_back()
        .expect("resumed review records");
    assert!(
        resumed_reviews
            .iter()
            .all(|review| review.effort == Some(Effort::Max)),
        "every review pass keeps max: {resumed_reviews:?}"
    );
    let warning = resumed
        .warnings
        .iter()
        .find(|warning| warning.contains("today's effort policy"))
        .unwrap_or_else(|| panic!("no effort difference warning: {:?}", resumed.warnings));
    assert!(warning.contains("implementation small=low"), "{warning}");
    assert!(warning.contains("implementation small=xhigh"), "{warning}");
    assert!(warning.contains("review=max"), "{warning}");
    assert!(warning.contains("Start a new run"), "{warning}");
}

#[test]
fn resume_restores_the_recorded_worker_binding_before_preflight() {
    let original = "[interaction]\nmode = \"never\"\n\n\
                        [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n";
    let (repo, run_id) = parked_run_with_config("resumebinding", original);
    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
             [[pins]]\ntier = \"small\"\nagent = \"copilot\"\nmodel = \"gpt-5-mini\"\n",
    )
    .expect("edit only the binding");

    let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    let logged = events_of(&repo, &run_id);
    let worker = logged
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::AttemptStarted { data, .. } => Some(data),
            _ => None,
        })
        .next_back()
        .expect("resumed worker");
    assert_eq!(worker.agent, "claude-code");
    assert_eq!(worker.model, "claude-haiku-4-5");
    assert_eq!(
        worker.selection_origin,
        Some(events::SelectionOrigin::Auto),
        "the recorded absence of a pin is part of the snapshot too"
    );
    assert!(
        resumed
            .warnings
            .iter()
            .any(|warning| warning.contains("today's worker bindings")
                && warning.contains("gpt-5-mini")
                && warning.contains("claude-haiku-4-5")),
        "binding difference warning: {:?}",
        resumed.warnings
    );
}

#[test]
fn the_resume_that_rederives_an_old_logs_effort_records_it_for_the_next_one() {
    let original = "[interaction]\nmode = \"never\"\n\n\
                        [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                        [routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n";
    let (repo, run_id) = parked_run_with_config("oldlogeffort", original);
    let paths = paths_of(&repo, &run_id);
    rewrite_run_started_as_schema_one(&paths, &["effort_policy"]);

    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
             [routing.effort]\nimplementation = \"high\"\nreview = \"xhigh\"\n",
    )
    .expect("first derived policy");
    let first = resume_answering(&repo, &run_id, Effect::NoEdit);
    assert_eq!(first.outcome(), RunOutcome::Parked, "{first:?}");
    assert!(
        first
            .warnings
            .iter()
            .any(|warning| warning.contains("predates the effort-policy record")),
        "legacy warning: {:?}",
        first.warnings
    );
    let established = ResolvedEffortPolicy {
        small: Effort::High,
        mid: Effort::High,
        frontier: Effort::High,
        review: Effort::XHigh,
    };
    assert_eq!(
        events::recorded_effort_policy(&events_of(&repo, &run_id)),
        Some(established),
        "the first resume writes down what it derived"
    );
    let after_first = events_of(&repo, &run_id);
    assert!(events::recorded_chains(&after_first).is_some());
    assert_eq!(
        after_first
            .iter()
            .filter(|event| matches!(event.body, EventBody::RunSchemaUpgraded { .. }))
            .count(),
        1,
        "the first current-binary resume appends one downgrade barrier"
    );

    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
             [routing.effort]\nimplementation = \"low\"\nreview = \"medium\"\n",
    )
    .expect("later policy");
    let second = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(second.outcome(), RunOutcome::Complete, "{second:?}");
    let logged = events_of(&repo, &run_id);
    let worker = logged
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::AttemptStarted { data, .. } => Some(data),
            _ => None,
        })
        .next_back()
        .expect("second resumed worker");
    assert_eq!(worker.effort, Some(Effort::High));
    let reviews = logged
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::AttemptFinished { data, .. } if !data.reviews.is_empty() => {
                Some(&data.reviews)
            }
            _ => None,
        })
        .next_back()
        .expect("second resumed reviews");
    assert!(
        reviews
            .iter()
            .all(|review| review.effort == Some(Effort::XHigh)),
        "reviews retain the established legacy policy: {reviews:?}"
    );
    assert!(
        second
            .warnings
            .iter()
            .any(|warning| warning.contains("today's effort policy")),
        "the later edit is reported: {:?}",
        second.warnings
    );
    assert!(
        !second
            .warnings
            .iter()
            .any(|warning| warning.contains("predates the effort-policy record")),
        "the legacy absence was established once: {:?}",
        second.warnings
    );
    assert_eq!(
        events_of(&repo, &run_id)
            .iter()
            .filter(|event| matches!(event.body, EventBody::RunSchemaUpgraded { .. }))
            .count(),
        1,
        "later resumes must not append duplicate schema transitions"
    );
}

#[test]
fn the_resume_that_rederives_an_old_review_plan_records_it_for_the_next_one() {
    let original = "[interaction]\nmode = \"never\"\n\n\
                        [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n";
    let (repo, run_id) = parked_run_with_config("oldlogreviews", original);
    let paths = paths_of(&repo, &run_id);
    rewrite_run_started_as_schema_two(&paths);
    strip_run_started_field(&paths, "reviews");

    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\
             review = { timeout_secs = 60 }\n",
    )
    .expect("first derived review plan");
    let first = resume_answering(&repo, &run_id, Effect::NoEdit);
    assert_eq!(first.outcome(), RunOutcome::Parked, "{first:?}");
    assert!(
        first
            .warnings
            .iter()
            .any(|warning| warning.contains("predates the review record")),
        "legacy warning: {:?}",
        first.warnings
    );
    let established = events::recorded_reviews(&events_of(&repo, &run_id))
        .cloned()
        .expect("the first resume writes down what it derived");
    assert_eq!(established.pass_timeout_secs, Some(60));

    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\
             review = { timeout_secs = 120 }\n",
    )
    .expect("later review plan");
    let second = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(second.outcome(), RunOutcome::Complete, "{second:?}");
    assert_eq!(
        events::recorded_reviews(&events_of(&repo, &run_id))
            .expect("record survives")
            .pass_timeout_secs,
        Some(60),
        "a later config edit cannot replace the established plan"
    );
    let warning = second
        .warnings
        .iter()
        .find(|warning| warning.contains("today's review pass timeout"))
        .unwrap_or_else(|| panic!("no timeout drift warning: {:?}", second.warnings));
    assert!(warning.contains("120s"), "{warning}");
    assert!(warning.contains("60s"), "{warning}");
    assert!(
        !second
            .warnings
            .iter()
            .any(|warning| warning.contains("predates the review record")),
        "the legacy absence is established exactly once: {:?}",
        second.warnings
    );
}

#[test]
fn a_schema_two_resume_records_the_complete_review_barrier_before_work() {
    let (repo, run_id) = parked_run("schema2reviewbarrier");
    let paths = paths_of(&repo, &run_id);
    rewrite_run_started_as_schema_two(&paths);
    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\
             review = { timeout_secs = 47 }\n",
    )
    .expect("first explicit complete-review timeout");

    let resumed = resume_answering(&repo, &run_id, Effect::NoEdit);
    assert_eq!(resumed.outcome(), RunOutcome::Parked, "{resumed:?}");

    let logged = events_of(&repo, &run_id);
    let barrier = logged
        .iter()
        .position(|event| {
            matches!(
                &event.body,
                EventBody::RunSchemaUpgraded { data }
                    if data.from == 2 && data.to == events::SCHEMA_VERSION
            )
        })
        .expect("schema 2 -> 3 downgrade barrier");
    let resumed_attempt = logged
        .iter()
        .enumerate()
        .skip(barrier + 1)
        .find(|(_, event)| matches!(event.body, EventBody::AttemptStarted { .. }))
        .map(|(index, _)| index)
        .expect("resumed attempt after the barrier");
    assert!(
        barrier < resumed_attempt,
        "the old verification contract must be fenced off before work starts"
    );
    let upgraded_reviews = events::recorded_complete_reviews(&logged)
        .expect("schema-3 resume records a complete review plan");
    assert_eq!(upgraded_reviews.pass_timeout_secs, Some(47));
    assert_eq!(upgraded_reviews.enabled, Some(true));
    assert_eq!(
        upgraded_reviews.alternative_available,
        Some(upgraded_reviews.alternative.is_some())
    );
    assert_eq!(upgraded_reviews.second_opinion.len(), 1);

    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\
             review = { timeout_secs = 83 }\n",
    )
    .expect("later configured timeout");
    let second = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(second.outcome(), RunOutcome::Complete, "{second:?}");
    assert_eq!(
        events::recorded_reviews(&events_of(&repo, &run_id))
            .expect("upgraded review plan survives")
            .pass_timeout_secs,
        Some(47),
        "a later binary/config default cannot reinterpret the upgraded timeout"
    );
    assert!(
        second.warnings.iter().any(|warning| {
            warning.contains("today's review pass timeout")
                && warning.contains("83s")
                && warning.contains("47s")
        }),
        "timeout drift warning: {:?}",
        second.warnings
    );
}

#[test]
fn max_parallel_above_one_refuses_before_the_run_touches_the_workspace() {
    let repo = temp_engine_repo("maxparallelrefusal");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[engine]\nmax_parallel = 3\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let head_before = git_in(&repo, &["rev-parse", "HEAD"]);
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::EditFile);

    let error = run_with(&opts, &source).expect_err("a refused ceiling must not start a run");
    assert!(error.to_string().contains("max_parallel = 3"), "{error}");

    assert!(
        source.adapter.runs().is_empty(),
        "nothing may be spawned, let alone paid for"
    );
    assert!(
        rundir::list_runs(&repo).is_empty(),
        "no run directory: {:?}",
        rundir::list_runs(&repo)
    );
    assert_eq!(
        git_in(&repo, &["branch", "--list", "upstroke/run-*"]),
        "",
        "no run branch"
    );
    assert_eq!(git_in(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git_in(&repo, &["status", "--porcelain"]),
        "",
        "working tree untouched"
    );
}

fn worktree_lock_path(repo: &Path) -> PathBuf {
    Workspace::open(repo)
        .expect("workspace")
        .worktree_git_dir()
        .expect("worktree git dir")
        .join("upstroke-worktree.lock")
}

#[test]
fn a_refused_ceiling_beats_the_lease_rather_than_racing_it() {
    let repo = temp_engine_repo("ceilingbeforelease");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[engine]\nmax_parallel = 3\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let lease = worktree_lock_path(&repo);
    assert!(
        !lease.exists(),
        "the fixture has never taken the lease, so the file cannot exist yet"
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));

    let source = fake(Effect::EditFile);
    let refused = run_with(&opts, &source)
        .expect_err("a ceiling this engine cannot honour must not start a run")
        .to_string();
    assert!(refused.contains("max_parallel = 3"), "{refused}");
    assert!(
        !lease.exists(),
        "the worktree lock file at {} was created before the config was read",
        lease.display()
    );

    let competitor = WorktreeLock::acquire(&repo).expect("a competing holder takes the lease");
    assert!(
        lease.exists(),
        "the competing holder is what creates the file, which is how (a) means anything"
    );
    let contended = run_with(&opts, &source)
        .expect_err("a refused ceiling still refuses while somebody holds the lease")
        .to_string();
    assert!(
        contended.contains("max_parallel = 3"),
        "the config error must win the race it never needed to enter: {contended}"
    );
    assert!(
        !contended.contains("another upstroke process"),
        "lock contention must not be the diagnosis for a config error: {contended}"
    );
    drop(competitor);

    assert!(
        source.adapter.runs().is_empty(),
        "nothing may be spawned, let alone paid for"
    );
    assert!(
        rundir::list_runs(&repo).is_empty(),
        "and no run directory: {:?}",
        rundir::list_runs(&repo)
    );
}

#[test]
fn a_refused_ceiling_beats_both_locks_on_resume() {
    let (repo, run_id) = parked_run("resumeceilingbeforelocks");
    let paths = paths_of(&repo, &run_id);
    rewrite_run_started_as_schema_two(&paths);
    fs::write(
        repo.join("upstroke.toml"),
        format!("{PARKED_RUN_CONFIG}\n[engine]\nmax_per_agent = 0\n"),
    )
    .expect("today's config");

    let lease = worktree_lock_path(&repo);
    let run_lock = rundir::lock_file(&paths.public);
    for path in [&lease, &run_lock] {
        fs::remove_file(path).unwrap_or_else(|error| {
            panic!("the fixture run left {}: {error}", path.display());
        });
    }

    let refused = resume_err(&repo, &run_id);
    assert!(refused.contains("max_per_agent"), "{refused}");
    assert!(
        !lease.exists(),
        "the worktree lease was taken before the config was read"
    );
    assert!(
        !run_lock.exists(),
        "the run lock was taken before the config was read"
    );

    let competitor = WorktreeLock::acquire(&repo).expect("a competing holder takes the lease");
    let contended = resume_err(&repo, &run_id);
    assert!(
        contended.contains("max_per_agent"),
        "the config error must win the race it never needed to enter: {contended}"
    );
    assert!(
        !contended.contains("another upstroke process"),
        "lock contention must not be the diagnosis for a config error: {contended}"
    );
    drop(competitor);
    assert!(
        !run_lock.exists(),
        "and the run lock stays untaken either way"
    );
}

fn config_with_repairs(repairs: u32) -> String {
    format!(
        "[engine]\nmax_merge_repairs = {repairs}\n\n\
         [routing]\nimplement = {{ chain = [\"small\"], attempts_per = 1 }}\n"
    )
}

#[test]
fn the_analysis_adopted_under_the_lease_is_the_one_its_own_bytes_were_validated_from() {
    let repo = temp_engine_repo("confirmunderlease");
    let config = repo.join("upstroke.toml");
    let mut opts = options(&repo);
    opts.config_path = Some(config.clone());

    fs::write(&config, config_with_repairs(5)).expect("the config before the lease");
    let validated = validate_inputs(&opts, config::EngineLimits::Fresh).expect("pre-lock check");
    fs::write(&config, config_with_repairs(7)).expect("the config at the lease");
    let analysis = validated
        .confirm_under_lease(&opts, config::EngineLimits::Fresh)
        .expect("a valid config is still valid");
    assert_eq!(
        analysis.config.max_merge_repairs, 7,
        "the adopted analysis must describe the bytes the run is holding"
    );

    fs::write(&config, config_with_repairs(5)).expect("the config before the lease");
    let validated = validate_inputs(&opts, config::EngineLimits::Fresh).expect("pre-lock check");
    fs::write(
        &config,
        "[engine]\nmax_parallel = 3\n\n\
         [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
    )
    .expect("a ceiling this engine cannot honour");
    let refused = validated
        .confirm_under_lease(&opts, config::EngineLimits::Fresh)
        .expect_err("an unhonourable ceiling must not be adopted because an older file was fine")
        .to_string();
    assert!(refused.contains("max_parallel = 3"), "{refused}");

    fs::write(&config, config_with_repairs(5)).expect("A");
    let validated = validate_inputs(&opts, config::EngineLimits::Fresh).expect("pre-lock check");
    fs::write(&config, config_with_repairs(9)).expect("B");
    fs::write(&config, config_with_repairs(5)).expect("A again");
    let analysis = validated
        .confirm_under_lease(&opts, config::EngineLimits::Fresh)
        .expect("A is what was captured and A is what is there");
    assert_eq!(
        analysis.config.max_merge_repairs, 5,
        "B was adopted from an excursion neither capture can see"
    );
}

#[test]
fn the_gate_derivation_is_taken_under_the_lease_not_carried_over_it() {
    let repo = temp_engine_repo("gatesunderlease");
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    fs::write(opts.config_path.as_ref().expect("config path"), "").expect("an empty config");

    let validated = validate_inputs(&opts, config::EngineLimits::Fresh).expect("pre-lock check");
    fs::write(repo.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("a rust repo now");
    let analysis = validated
        .confirm_under_lease(&opts, config::EngineLimits::Fresh)
        .expect("a shape change is not a refusal");
    assert_eq!(
        analysis
            .gates
            .iter()
            .map(|gate| gate.name.clone())
            .collect::<Vec<_>>(),
        vec!["check".to_owned(), "test".to_owned()],
        "the gates a run is held to must be derived from the worktree it holds"
    );
}

fn volatile_strings(repo: &Path, run_id: &str) -> Vec<String> {
    let mut volatile = vec![run_id.to_owned()];
    for path in [private_root_for(repo), repo.to_path_buf()] {
        let text = path.to_string_lossy().into_owned();
        volatile.push(text.replace('\\', "/"));
        volatile.push(text);
    }
    volatile
}

fn replace_exact_runs(
    text: &str,
    len: usize,
    token: &str,
    member: impl Fn(char) -> bool,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.chars().count() == len {
            out.push_str(token);
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for ch in text.chars() {
        if member(ch) {
            run.push(ch);
            continue;
        }
        flush(&mut run, &mut out);
        out.push(ch);
    }
    flush(&mut run, &mut out);
    out
}

fn canonicalize_json(value: &mut serde_json::Value, volatile: &[String]) {
    match value {
        serde_json::Value::String(text) => {
            let mut canonical = text.clone();
            for needle in volatile {
                canonical = canonical.replace(needle.as_str(), "<volatile>");
            }
            canonical = replace_exact_runs(&canonical, 40, "<sha>", |ch| {
                ch.is_ascii_digit() || ch.is_ascii_lowercase() && ch.is_ascii_hexdigit()
            });
            *text = replace_exact_runs(&canonical, 26, "<ulid>", |ch| {
                ch.is_ascii_digit() || ch.is_ascii_uppercase()
            });
        }
        serde_json::Value::Array(items) => {
            for item in items {
                canonicalize_json(item, volatile);
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                if matches!(key.as_str(), "ts" | "duration_ms" | "duration") {
                    *field = serde_json::Value::String(format!("<{key}>"));
                    continue;
                }
                canonicalize_json(field, volatile);
            }
        }
        _ => {}
    }
}

fn canonical_trace(events: &[events::Event], repo: &Path, run_id: &str) -> Vec<String> {
    let volatile = volatile_strings(repo, run_id);
    events
        .iter()
        .map(|event| {
            let mut value = serde_json::to_value(event).expect("an event serializes");
            canonicalize_json(&mut value, &volatile);
            value.to_string()
        })
        .collect()
}

fn canonical_projection(report: &RunReport, repo: &Path, run_id: &str) -> String {
    let volatile = volatile_strings(repo, run_id);
    let mut value = serde_json::to_value(report).expect("a report serializes");
    value
        .as_object_mut()
        .expect("a report is an object")
        .remove("warnings")
        .expect("a report records its warnings");
    canonicalize_json(&mut value, &volatile);
    value.to_string()
}

const LEGACY_RESUME_LIMITS: &str = "\n[engine]\nmax_parallel = 2\nmax_merge_repairs = 7\n\
                                    max_per_agent = 4\nmax_per_pool = 5\n";

const LEGACY_RESUME_NO_LIMITS: &str = "\n# no [engine] ceilings in this arm\n";

#[derive(Clone, Copy)]
enum LegacyFixture {
    Parked,
    InterruptedAttempt,
}

struct LegacyArm {
    report: RunReport,
    trace: Vec<String>,
    projection: String,
    tree: String,
    events: Vec<events::Event>,
}

fn legacy_resume_pair(tag: &str, fixture: LegacyFixture) -> Vec<LegacyArm> {
    let mut observed = Vec::new();
    for (arm, extra) in [
        ("control", LEGACY_RESUME_NO_LIMITS),
        ("limits", LEGACY_RESUME_LIMITS),
    ] {
        let (repo, run_id) = parked_run(&format!("legacylimits-{tag}-{arm}"));
        let paths = paths_of(&repo, &run_id);
        rewrite_run_started_as_schema_two(&paths);
        if matches!(fixture, LegacyFixture::InterruptedAttempt) {
            truncate_log_after(&paths, "attempt_started");
        }
        fs::write(
            repo.join("upstroke.toml"),
            format!("{PARKED_RUN_CONFIG}{extra}"),
        )
        .expect("today's config");

        let report = resume_answering(&repo, &run_id, Effect::EditFile);
        let events = events_of(&repo, &run_id);
        observed.push(LegacyArm {
            trace: canonical_trace(&events, &repo, &run_id),
            projection: canonical_projection(&report, &repo, &run_id),
            tree: git_in(&repo, &["rev-parse", "HEAD^{tree}"]),
            events,
            report,
        });
    }
    observed
}

#[test]
fn a_legacy_resume_is_not_reinterpreted_by_the_new_engine_limits() {
    for (fixture, tag) in [
        (LegacyFixture::Parked, "parked"),
        (LegacyFixture::InterruptedAttempt, "interrupted"),
    ] {
        let observed = legacy_resume_pair(tag, fixture);
        let control = &observed[0];
        let limits = &observed[1];

        assert_eq!(
            control.report.outcome(),
            RunOutcome::Complete,
            "the {tag} control resume continues to the end: {:?}",
            control.report
        );
        assert_eq!(
            limits.report.outcome(),
            control.report.outcome(),
            "the new keys must not change how a legacy run ends ({tag})"
        );
        assert_eq!(
            limits.trace, control.trace,
            "nor what it records, nor with what contents, nor in what order ({tag})"
        );
        assert_eq!(
            limits.projection, control.projection,
            "nor what it reports about each task ({tag})"
        );
        assert_eq!(
            limits.tree, control.tree,
            "nor the tree it committed ({tag})"
        );
        assert!(
            !control.tree.is_empty(),
            "the fixture must actually have committed something for that to mean anything"
        );

        let mut open = 0i32;
        let mut peak = 0i32;
        for event in &limits.events {
            match &event.body {
                EventBody::AttemptStarted { .. } => open += 1,
                EventBody::AttemptFinished { .. } | EventBody::AttemptInterrupted { .. } => {
                    open -= 1;
                }
                _ => continue,
            }
            peak = peak.max(open);
        }
        assert_eq!(peak, 1, "the resume ran one attempt at a time ({tag})");
        assert_eq!(open, 0, "and settled every one of them ({tag})");

        for key in [
            "max_parallel",
            "max_merge_repairs",
            "max_per_agent",
            "max_per_pool",
        ] {
            assert!(
                limits
                    .report
                    .warnings
                    .iter()
                    .any(|warning| warning.contains(key) && warning.contains("not acted on")),
                "`{key}` must be reported as unacted-on ({tag}): {:?}",
                limits.report.warnings
            );
            assert!(
                !control
                    .report
                    .warnings
                    .iter()
                    .any(|warning| warning.contains(key)),
                "and only when it was written ({tag}): {:?}",
                control.report.warnings
            );
        }
        assert!(
            limits.report.warnings.iter().any(|warning| {
                warning.contains("max_parallel = 2") && warning.contains("this resume")
            }),
            "and the refused-for-fresh-runs ceiling says which run it is talking about: {:?}",
            limits.report.warnings
        );
    }
}

#[test]
fn schema_two_review_markers_upgrade_independently_of_timeout() {
    let (repo, run_id) = parked_run("schema2reviewmarkers");
    let paths = paths_of(&repo, &run_id);
    let recorded_timeout = events::recorded_reviews(&events_of(&repo, &run_id))
        .and_then(|plan| plan.pass_timeout_secs)
        .expect("current run records a timeout");
    rewrite_run_started_as_schema_two_missing_review_fields(
        &paths,
        &["enabled", "alternative_available"],
    );

    let first = resume_answering(&repo, &run_id, Effect::NoEdit);
    assert_eq!(first.outcome(), RunOutcome::Parked, "{first:?}");
    assert!(
        first
            .warnings
            .iter()
            .any(|warning| warning.contains("explicit reviewer-identity markers")),
        "marker-upgrade warning: {:?}",
        first.warnings
    );
    let upgraded = events::recorded_complete_reviews(&events_of(&repo, &run_id))
        .cloned()
        .expect("schema-3 resume records the complete identity");
    assert_eq!(upgraded.pass_timeout_secs, Some(recorded_timeout));
    assert_eq!(upgraded.enabled, Some(upgraded.primary.is_some()));
    assert_eq!(
        upgraded.alternative_available,
        Some(upgraded.alternative.is_some())
    );

    let second = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(second.outcome(), RunOutcome::Complete, "{second:?}");
    assert_eq!(
        events::recorded_complete_reviews(&events_of(&repo, &run_id)),
        Some(&upgraded),
        "the next replay accepts and preserves the explicit markers"
    );
}

#[test]
fn schema_two_inconsistent_review_identity_is_refused_before_upgrade_and_spend() {
    let (repo, run_id) = parked_run("schema2badreviewidentity");
    let paths = paths_of(&repo, &run_id);
    let text = fs::read_to_string(paths.events()).expect("log");
    let mut rewritten = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            let mut value: serde_json::Value = serde_json::from_str(line).expect("event json");
            if value.get("event").and_then(serde_json::Value::as_str) == Some("run_started") {
                let data = value
                    .get_mut("data")
                    .and_then(serde_json::Value::as_object_mut)
                    .expect("run_started data");
                data.insert("schema".to_owned(), serde_json::Value::from(2));
                let reviews = data
                    .get_mut("reviews")
                    .and_then(serde_json::Value::as_object_mut)
                    .expect("review plan");
                reviews.insert("enabled".to_owned(), serde_json::Value::Bool(true));
                reviews.insert("primary".to_owned(), serde_json::Value::Null);
                rewritten = true;
            }
            value.to_string()
        })
        .collect();
    assert!(rewritten);
    fs::write(paths.events(), format!("{}\n", lines.join("\n"))).expect("rewrite");

    let question = events_of(&repo, &run_id)
        .iter()
        .find_map(|event| match &event.body {
            EventBody::QuestionRaised { data, .. } => Some(data.question.id.to_string()),
            EventBody::AttemptFinished {
                parking: Some(parking),
                ..
            } => Some(parking.question.id.to_string()),
            _ => None,
        })
        .expect("parked question");
    crate::answer::answer(
        &repo,
        &question,
        crate::answer::Reply::Text("continue".to_owned()),
    )
    .expect("answer");
    let source = fake(Effect::EditFile);
    let error = resume_harness_inner(
        &resume_options(&repo, &run_id),
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
    )
    .expect_err("an inconsistent inherited review identity must fail closed");
    assert!(
        error
            .to_string()
            .contains("reviews.enabled does not match the recorded primary reviewer"),
        "wrong error: {error}"
    );
    assert!(
        source.adapter.runs().is_empty(),
        "no worker may run under the malformed identity"
    );
    assert!(
        !events_of(&repo, &run_id)
            .iter()
            .any(|event| matches!(event.body, EventBody::RunSchemaUpgraded { .. })),
        "the malformed identity must not be blessed by a schema upgrade"
    );
}

#[test]
fn a_resume_whose_effort_policy_did_not_move_says_nothing_about_it() {
    let config = "[interaction]\nmode = \"never\"\n\n\
                      [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                      [routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n";
    let (repo, run_id) = parked_run_with_config("effortunmoved", config);
    let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    assert!(
        !resumed
            .warnings
            .iter()
            .any(|warning| warning.contains("effort policy") || warning.contains("effort-policy")),
        "an unchanged policy must be silent: {:?}",
        resumed.warnings
    );
}

#[test]
fn a_log_written_before_step_9_still_gets_reviewed_on_resume() {
    let repo = temp_engine_repo("oldlogresume");
    seed(
        &repo,
        "## Rotate the signing key\n\
             <!-- upstroke: id=t1 kind=implement depends= tier=frontier -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"frontier\"], attempts_per = 1 }\n",
        ),
    );
    let first_source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
    let first = run_with(&cross_vendor_opts(&repo), &first_source).expect("run");

    let paths = paths_of(&repo, &first.run_id);
    rewrite_run_started_as_schema_two(&paths);
    strip_run_started_field(&paths, "reviews");

    crate::answer::answer(
        &repo,
        &first.questions[0].question.id.to_string(),
        crate::answer::Reply::Text("put the key in src/auth/keys.rs".to_owned()),
    )
    .expect("answer");

    let later = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
    let resumed =
        resume_with(&resume_options(&repo, &first.run_id), &later).expect("resume continues");
    assert!(
        !committed(&resumed, "t1"),
        "an older log must not silently switch review off: {resumed:?}"
    );
}

#[test]
fn an_unavailable_reviewer_is_recorded_as_such_not_as_a_rejection() {
    let repo = temp_engine_repo("outagerecord");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Pass],
        vec![ReviewBehavior::RateLimited, ReviewBehavior::Pass],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

    let first = &task(&report, "t1").attempts[0];
    assert_eq!(
        first.reviews.iter().map(|r| r.outcome).collect::<Vec<_>>(),
        [
            events::ReviewPassOutcome::Passed,
            events::ReviewPassOutcome::Unavailable
        ],
        "the second vendor was down, not unimpressed"
    );
    assert!(committed(&report, "t1"), "{report:?}");
}

#[test]
fn second_reviewer_spawn_failure_settles_worker_and_first_review_evidence() {
    let repo = temp_engine_repo("secondreviewerspawnsettlement");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Pass],
        vec![ReviewBehavior::SpawnError, ReviewBehavior::Pass],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("settled run");

    assert!(
        committed(&report, "t1"),
        "the deferred retry recovers: {report:?}"
    );
    let task = task(&report, "t1");
    assert_eq!(task.attempts.len(), 2, "one settled outage, one recovery");
    let first = &task.attempts[0];
    let failure = first.failure.as_ref().expect("spawn failure is recorded");
    assert_eq!(failure.kind, FailureKind::ReviewUnavailable);
    assert_eq!(failure.origin, FailureOrigin::Reviewer);
    assert_eq!(first.cost_usd, Some(0.01), "worker spend survives");
    assert_eq!(first.session_id.as_deref(), Some("s0"));
    assert!(first.usage.is_some(), "worker usage survives");
    assert_eq!(
        first.reviews.iter().map(|r| r.outcome).collect::<Vec<_>>(),
        [
            events::ReviewPassOutcome::Passed,
            events::ReviewPassOutcome::Unavailable
        ],
        "the completed first verdict is not discarded"
    );
    assert_eq!(first.reviews[0].cost_usd, Some(0.05));
    assert_eq!(first.reviews[1].cost_usd, None);
    assert_eq!(source.copilot().review_spawn_failures(), 1);

    let logged = events_of(&repo, &report.run_id);
    assert!(logged.iter().any(|event| matches!(
        &event.body,
        EventBody::AttemptFinished {
            task,
            attempt: 1,
            ..
        } if task == "t1"
    )));
    assert!(!logged.iter().any(|event| matches!(
        &event.body,
        EventBody::AttemptInterrupted {
            task,
            attempt: 1,
            ..
        } if task == "t1"
    )));
}

#[test]
fn a_total_missing_an_unreported_reviewer_is_marked_rather_than_implied() {
    let repo = temp_engine_repo("partialcost");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = FakeSource {
        adapter: FakeAdapter::new(vec![Effect::EditFile], vec![ReviewBehavior::Pass]),
        copilot: Some(FakeAdapter::copilot(vec![ReviewBehavior::Pass]).unpriced()),
    };
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

    let t1 = task(&report, "t1");
    assert_eq!(t1.review_cost_usd, Some(0.05), "only what was reported");
    assert!(t1.review_cost_incomplete, "and it is not the whole story");
    assert!(
        report.render().contains("$0.0500?"),
        "the summary marks it: {}",
        report.render()
    );
    let ledger = report.render_ledger();
    assert!(ledger.contains("$0.0500?"), "{ledger}");
    assert!(
        ledger.contains("reports no spend"),
        "legend present: {ledger}"
    );
}

#[test]
fn every_model_that_judged_a_task_is_listed_beside_the_cost_of_all_of_them() {
    let repo = temp_engine_repo("reviewtrail");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n",
        ),
    );
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Fail, ReviewBehavior::Pass],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");
    let t1 = task(&report, "t1");
    assert_eq!(t1.attempts.len(), 2, "escalated: {t1:?}");
    assert_eq!(
        t1.review_models,
        ["claude-opus-5", "gpt-5.3-codex"],
        "both judges, in the order they judged"
    );
}

#[test]
fn each_pass_writes_its_own_verdict_transcript() {
    let repo = temp_engine_repo("passtranscripts");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let source = cross_vendor(
        vec![Effect::EditFile],
        vec![ReviewBehavior::Pass],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");
    let reviews = paths_of(&repo, &report.run_id).reviews();
    assert!(reviews.join("00-t1-1-review.json").is_file());
    assert!(
        reviews.join("00-t1-1-second-opinion-review.json").is_file(),
        "the second verdict cannot overwrite the first"
    );
}

#[test]
fn the_run_record_survives_completion() {
    let repo = temp_engine_repo("record");
    let source = fake(Effect::EditFile);
    let report = run_with(&options(&repo), &source).expect("run");
    let report_path = repo
        .join(".upstroke")
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
        "## Second by dependency\n<!-- upstroke: id=late depends=early -->\n\n\
             ## First by dependency\n<!-- upstroke: id=early depends= -->\n",
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
    let run_dir = std::env::temp_dir().join(format!("upstroke-prompt-{}", std::process::id()));
    fs::create_dir_all(run_dir.join("artifacts")).expect("run dir");
    let prompt = materialize_prompt(
        crate::engine::assembly::WorkerSubject::of(&task),
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
    let bare = materialize_prompt(
        crate::engine::assembly::WorkerSubject::of(&task),
        &[],
        &run_dir,
        None,
    );
    assert!(!bare.contains("EXACTLY these commands"));
}

#[test]
fn prompt_wires_artifacts_to_real_files() {
    let run_dir = std::env::temp_dir().join(format!("upstroke-artifact-{}", std::process::id()));
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

    let prompt = materialize_prompt(
        crate::engine::assembly::WorkerSubject::of(&task),
        &[],
        &run_dir,
        None,
    );
    assert!(prompt.contains("did \n     not leave one") || prompt.contains("did not leave one"));
    assert!(
        prompt.contains("write artifact `notes`"),
        "producer told where to write"
    );

    fs::write(
        artifact_path(&run_dir, "api-contract"),
        "cursor = base64(offset)",
    )
    .expect("artifact");
    let prompt = materialize_prompt(
        crate::engine::assembly::WorkerSubject::of(&task),
        &[],
        &run_dir,
        None,
    );
    assert!(
        prompt.contains("cursor = base64(offset)"),
        "content inlined"
    );

    task.artifacts_in.clear();
    task.artifacts_out.clear();
    let bare = materialize_prompt(
        crate::engine::assembly::WorkerSubject::of(&task),
        &[],
        &run_dir,
        None,
    );
    assert!(!bare.contains("artifact"));
}

#[test]
fn a_gate_failure_recovers_on_the_same_rung_via_session_resume() {
    let repo = temp_engine_repo("resume");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n\n\
                 [[gates]]\nname = \"needs-test\"\ncmd = \"git ls-files --error-unmatch \
                 widget_test.rs\"\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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

    let files = git_in(&repo, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.contains("agent-output.txt"), "files: {files}");
    assert!(files.contains("widget_test.rs"), "files: {files}");
}

#[test]
fn exhausting_a_rung_escalates_with_a_fresh_session_and_the_history() {
    let repo = temp_engine_repo("escalate");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
    let repo = temp_engine_repo("park");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- upstroke: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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

    let path = repo
        .join(".upstroke")
        .join("runs")
        .join(&report.run_id)
        .join("questions")
        .join(format!("{question}.json"));
    let record: QuestionRecord =
        serde_json::from_str(&fs::read_to_string(&path).expect("question file")).expect("parses");
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
fn resume_repairs_every_decline_settlement_crash_prefix() {
    for (tag, last_durable_event) in [
        ("answered", "question_answered"),
        ("defect", "design_defect"),
    ] {
        let repo = temp_engine_repo(&format!("declineprefix-{tag}"));
        seed(
            &repo,
            "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("upstroke.toml"));
        let initial = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let answers = ScriptedAnswers::new(vec![Answer::Declined]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &initial,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("build a complete decline sequence");
        let paths = paths_of(&repo, &report.run_id);
        truncate_log_after(&paths, last_durable_event);
        fs::write(
            repo.join("upstroke.toml"),
            "[engine]\non_task_failure = \"continue\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        )
        .expect("change today's policy after the decline was durable");

        let resumed_source = fake(Effect::EditFile);
        let resumed = resume_with(&resume_options(&repo, &report.run_id), &resumed_source)
            .expect("resume repairs the incomplete settlement");
        assert_eq!(
            resumed.outcome(),
            RunOutcome::Halted,
            "prefix {tag}: repair must use the policy recorded with the answer"
        );
        assert!(
            matches!(
                task(&resumed, "t1").status,
                TaskRunStatus::Failed {
                    kind: FailureKind::Declined,
                    ..
                }
            ),
            "prefix {tag}: {resumed:?}"
        );
        assert!(
            resumed_source.adapter.runs().is_empty(),
            "repair must settle the decline before another paid attempt"
        );

        let logged = events_of(&repo, &report.run_id);
        assert_eq!(
            logged
                .iter()
                .filter(|event| matches!(event.body, EventBody::DesignDefect { .. }))
                .count(),
            1,
            "the missing prefix is appended once"
        );
        assert_eq!(
            logged
                .iter()
                .filter(|event| matches!(event.body, EventBody::TaskFailed { .. }))
                .count(),
            1,
            "the declined task is settled once"
        );
    }
}

#[test]
fn schema_two_decline_prefix_preserves_or_refuses_unknown_halt_policy() {
    let repo = temp_engine_repo("legacydeclinepolicy");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let initial = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
    let answers = ScriptedAnswers::new(vec![Answer::Declined]);
    let report = run_harness(
        &opts,
        &Harness {
            adapters: &initial,
            answers: Some(&answers),
            sleeper: None,
        },
    )
    .expect("build a complete decline sequence");
    let paths = paths_of(&repo, &report.run_id);
    truncate_log_after(&paths, "question_answered");
    rewrite_run_started_as_schema_two(&paths);
    strip_event_data_field(&paths, "question_answered", "decline_halts_run");
    fs::write(
        repo.join("upstroke.toml"),
        "[engine]\non_task_failure = \"continue\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
    )
    .expect("today's policy differs");

    let error = resume_err(&repo, &report.run_id);
    assert!(
        error.contains("contemporaneous on_task_failure policy"),
        "{error}"
    );
    assert!(
        error.contains("cannot safely decide an old answer"),
        "{error}"
    );
}

#[test]
fn on_task_failure_continue_keeps_independent_work_moving() {
    let repo = temp_engine_repo("continue");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- upstroke: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
        Some(
            "[engine]\non_task_failure = \"continue\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
    assert!(committed(&report, "t3"));
    assert!(committed(&report, "t1"), "report: {report:?}");
    let t1 = task(&report, "t1");
    assert_eq!(
        t1.attempts.len(),
        2,
        "asking cost no attempt — only the retry after the answer"
    );
    assert!(
        !t1.attempts[1].resumed,
        "a parked task never resumes into a tree that was reverted underneath it"
    );
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
    let repo = temp_engine_repo("ci");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- upstroke: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
    let repo = temp_engine_repo("noloop");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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

    fn resolve(&self, _question: &Question) -> Result<Answer, UpstrokeError> {
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [[gates]]\nname = \"never\"\ncmd = \"git frobnicate-not-a-command\"\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
        review_models: Vec::new(),
        review_cost_usd: None,
        review_cost_incomplete: false,
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
        pool: None,
        resumed: false,
        duration: Duration::ZERO,
        cost_usd: None,
        reviews: Vec::new(),
        session_id: None,
        usage: None,
        failure: failed.then(|| FailureRecord {
            kind: FailureKind::GateFailed,
            origin: FailureOrigin::Worker,
            reason: "no".to_owned(),
            detail: None,
        }),
    }
}

fn raw_object_after(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let bytes: Vec<char> = line[start..].chars().collect();
    if bytes.first() != Some(&'{') {
        return None;
    }
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (index, c) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(bytes[..=index].iter().collect());
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn the_legacy_wire_and_report_carry_no_feedback_on_the_attempt_record() {
    let repo = temp_engine_repo("legacy-no-detail");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n\n\
                 [[gates]]\nname = \"needs-test\"\ncmd = \"git ls-files --error-unmatch \
                 widget_test.rs\"\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::EditFile, Effect::EditTest],
        vec![ReviewBehavior::Pass],
    );
    run_with(&opts, &source).expect("run");

    let runs = opts.repo_root.join(".upstroke").join("runs");
    let public = fs::read_dir(&runs)
        .expect("the runs root")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.join("events.jsonl").is_file())
        .expect("the run left a public directory");
    let log = fs::read_to_string(public.join("events.jsonl")).expect("the log");

    const PRE_CHANGE_FAILURE: &str = concat!(
        r#"{"kind":"gate_failed","origin":"worker","reason":"gate `needs-test` failed: "#,
        r#"error: pathspec 'widget_test.rs' did not match any file(s) known to git\n"#,
        r#"Did you forget to 'git add'?\n\nexit code: Some(1)"}"#,
    );

    let mut failures = 0;
    let mut carried_tail = false;
    for line in log.lines() {
        let event: serde_json::Value = serde_json::from_str(line).expect("a json line");
        if event.get("event").and_then(serde_json::Value::as_str) != Some("attempt_finished") {
            continue;
        }
        let Some(failure) = event.pointer("/data/failure").filter(|v| !v.is_null()) else {
            continue;
        };
        failures += 1;

        let bytes =
            raw_object_after(line, "\"failure\":").expect("the line carries a failure object");
        let stripped = bytes.replace(",\"detail\":null", "");
        assert_ne!(
            stripped, bytes,
            "the legacy failure record carries no `detail` key at all: {bytes}. This test \
             asserts the *only* difference from `610106b` is that one null, and if the key \
             is absent the assertion below is vacuous"
        );
        assert_eq!(
            bytes.matches(",\"detail\":").count(),
            1,
            "expected exactly one `detail` key in {bytes}"
        );

        assert_eq!(
            stripped, PRE_CHANGE_FAILURE,
            "the legacy failure record is not the bytes `610106b` wrote for this \
             scenario. If a newer git reworded its pathspec error, re-capture the \
             fixture at that commit rather than loosening this comparison"
        );

        let object = failure.as_object().expect("the failure is an object");
        assert_eq!(
            object.get("detail"),
            Some(&serde_json::Value::Null),
            "the legacy attempt record's `detail` is {:?}; it must be present and null — \
             a value would be §11.4's feedback duplicated onto the wire, and an absent \
             key would mean the field stopped serializing, which breaks schema 4's \
             strict door",
            object.get("detail")
        );

        if event
            .pointer("/transition/data/detail")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|d| d.contains("widget_test.rs"))
        {
            carried_tail = true;
        }
    }
    assert!(
        failures >= 1,
        "no failed attempt in the log, so this test asserted nothing"
    );
    assert!(
        carried_tail,
        "the gate tail reached no `ladder_retry` transition either, so the legacy engine \
         lost §11.4's feedback rather than keeping it where it belongs"
    );

    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(public.join("report.json")).expect("report.json is written"),
    )
    .expect("report.json is json");
    let mut checked = 0;
    for task in report["tasks"].as_array().into_iter().flatten() {
        for attempt in task["attempts"].as_array().into_iter().flatten() {
            let Some(failure) = attempt.get("failure").filter(|v| !v.is_null()) else {
                continue;
            };
            checked += 1;
            let object = failure.as_object().expect("the failure is an object");
            assert_eq!(
                object.get("detail"),
                Some(&serde_json::Value::Null),
                "report.json's attempt {} carries `detail` as {:?}; it must be present \
                 and null. `is_null()` was the earlier assertion and it cannot tell an \
                 explicit null from an absent key, so it would have passed had the \
                 field silently stopped serializing",
                attempt["attempt"],
                object.get("detail")
            );
        }
    }
    assert!(
        checked >= 1,
        "no failed attempt reached report.json, so its half of this test asserted nothing"
    );

    let runs = source.adapter.runs();
    assert!(
        runs[1].prompt.contains("gate `needs-test` failed"),
        "the legacy retry lost the gate's words: {}",
        runs[1].prompt
    );
}

#[test]
fn an_explicit_null_detail_survives_the_strict_door() {
    use crate::topology::events::{
        AttemptFinished4, AttemptNumber, AttemptSettlement, GenerationId, LeaseDisposition,
        SettlementTransition, TopologyEvent, TopologyEventBody,
    };

    let mut record = serde_json::to_value(attempt_record(1, "mid", true)).expect("serialize");
    let failure = record
        .get_mut("failure")
        .and_then(serde_json::Value::as_object_mut)
        .expect("a failed record carries a failure object");
    failure.insert("detail".to_owned(), serde_json::Value::Null);

    let mut value = serde_json::to_value(TopologyEvent::now(TopologyEventBody::AttemptFinished {
        data: Box::new(AttemptFinished4 {
            key: crate::topology::registry::TaskKey(0),
            generation: GenerationId(0),
            attempt: AttemptNumber(1),
            record: Box::new(attempt_record(1, "mid", true)),
            settlement: AttemptSettlement::Closed {
                transition: SettlementTransition::Retry,
                lease: LeaseDisposition::PredictedReleased,
            },
        }),
    }))
    .expect("the event serializes");
    value["data"]["record"] = record;

    let parsed: TopologyEvent = serde_json::from_value(value).expect(
        "an explicit null on a known optional field must pass the strict door; if this \
         refuses, the door is reporting a field the record does declare, and every \
         failed attempt's settlement is unreadable",
    );
    let TopologyEventBody::AttemptFinished { data } = parsed.body else {
        unreachable!("built as an attempt_finished")
    };
    assert_eq!(
        data.record
            .failure
            .as_ref()
            .expect("the failure survives")
            .detail,
        None,
        "an explicit null must read back as None"
    );
}

#[test]
fn both_feedback_sources_reach_the_durable_attempt_record() {
    use crate::gates::GateFailure;

    let tail =
        "error[E0308]: mismatched types\n  --> src/alpha.rs:12:9\n   expected `u32`, found `&str`";
    let gate = super::classify::gate_failure(&GateFailure {
        gate: "cargo test".to_owned(),
        summary: "1 failed".to_owned(),
        log_tail: tail.to_owned(),
    });
    assert_eq!(
        durable_detail(&gate).as_deref(),
        Some(tail),
        "§11.1's gate tail did not reach the record, so a resume cannot tell the \
         retry what the gate printed"
    );

    let review = super::attempt::review_failure(review::ReviewResult::Judged(crate::ir::Verdict {
        pass: false,
        reasons: vec!["the parser accepts a trailing comma".to_owned()],
        required_changes: vec![
            "reject a trailing comma in `parse_list`".to_owned(),
            "add a case for the empty list".to_owned(),
        ],
        needs_human: false,
    }))
    .expect("a failed verdict is a failure");
    assert_eq!(
        durable_detail(&review).as_deref(),
        Some("- reject a trailing comma in `parse_list`\n- add a case for the empty list"),
        "§11.2's required_changes did not reach the record verbatim, so an \
         escalation carries the reviewer's summary instead of its instructions"
    );
}

fn durable_detail(failure: &crate::ladder::AttemptFailure) -> Option<String> {
    let outcome = crate::ir::Outcome {
        status: crate::ir::OutcomeStatus::Completed,
        diff: String::new(),
        detail: None,
        session_id: None,
        usage: None,
        cost_usd: None,
        transcript_path: PathBuf::new(),
        duration: Duration::ZERO,
    };
    super::classify::attempt_record(
        1,
        super::classify::AttemptFacts {
            tier: crate::ir::Tier::Mid,
            model: "claude-opus-5",
            pool: None,
            resumed: false,
            outcome: &outcome,
            reviews: &[],
            failure: Some(failure),
            feedback: super::classify::FeedbackCarrier::AttemptRecord,
        },
    )
    .failure
    .expect("a classified failure produces a failure record")
    .detail
}

#[test]
fn a_worker_question_is_read_from_the_marker_onward() {
    assert_eq!(
        worker_question(Some("Did some work.\nUPSTROKE-QUESTION: opaque or signed?")).as_deref(),
        Some("opaque or signed?")
    );
    assert_eq!(
        worker_question(Some("UPSTROKE-QUESTION: which store?\nRedis or Postgres?")).as_deref(),
        Some("which store?\nRedis or Postgres?")
    );
    assert_eq!(worker_question(Some("UPSTROKE-QUESTION:   ")), None);
    assert_eq!(worker_question(Some("no marker here")), None);
    assert_eq!(worker_question(None), None);
}

#[test]
fn an_echoed_marker_does_not_swallow_the_real_question() {
    let reply = "The retry feedback says I can use the UPSTROKE-QUESTION: marker if I am \
                     blocked. I considered whether this needs one.\n\n\
                     UPSTROKE-QUESTION: should cursors be opaque or signed?";
    assert_eq!(
        worker_question(Some(reply)).as_deref(),
        Some("should cursors be opaque or signed?"),
        "last marker wins, matching the prompt and review.rs's verdict rule"
    );
}

#[test]
fn an_outage_is_never_reclassified_as_a_question() {
    let quoting = "I will end with the UPSTROKE-QUESTION: marker if I get stuck.";
    let output = crate::agent::ProcessOutput {
        stdout: String::new(),
        stderr: String::new(),
        code: Some(1),
        timed_out: false,
        output_limited: false,
        duration: Duration::ZERO,
    };
    for (status, expected) in [
        (OutcomeStatus::RateLimited, FailureKind::RateLimited),
        (OutcomeStatus::Timeout, FailureKind::Timeout),
        (OutcomeStatus::AgentError, FailureKind::AgentError),
    ] {
        let outcome = fake_outcome(status, Some(quoting.to_owned()), "s0", None, Duration::ZERO);
        let failure = evaluate_outcome(&outcome, &output).expect("still a failure");
        assert_eq!(failure.kind, expected, "{status:?} must keep its own kind");
    }

    let mut asked = fake_outcome(
        OutcomeStatus::Completed,
        Some("UPSTROKE-QUESTION: opaque or signed?".to_owned()),
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
    let repo = temp_engine_repo("haltpark");
    seed(
        &repo,
        "## Asks a question\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Exhausts its chain\n<!-- upstroke: id=t2 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::AskQuestion, Effect::NoEdit],
        vec![ReviewBehavior::Pass],
    );
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

fn replay_of(repo: &Path, run_id: &str) -> crate::status::RunStatus {
    crate::status::load(repo, Some(run_id)).expect("the run reads back")
}

struct Scenario {
    name: &'static str,
    config: &'static str,
    plan: Option<&'static str>,
    effects: Vec<Effect>,
    reviews: Vec<ReviewBehavior>,
    second_opinion: Option<Vec<ReviewBehavior>>,
    answers: Vec<Answer>,
}

impl Scenario {
    fn new(name: &'static str, config: &'static str, effects: Vec<Effect>) -> Self {
        Self {
            name,
            config,
            plan: None,
            effects,
            reviews: vec![ReviewBehavior::Pass],
            second_opinion: None,
            answers: Vec::new(),
        }
    }

    fn reviewed(mut self, reviews: Vec<ReviewBehavior>) -> Self {
        self.reviews = reviews;
        self
    }

    fn cross_vendor(mut self, plan: &'static str, second: Vec<ReviewBehavior>) -> Self {
        self.plan = Some(plan);
        self.second_opinion = Some(second);
        self
    }

    fn answered(mut self, answers: Vec<Answer>) -> Self {
        self.answers = answers;
        self
    }
}

fn assert_live_equals_replay(repo: &Path, live: &RunState, report: &RunReport) {
    let replayed = replay_of(repo, &report.run_id);
    assert_eq!(
        &replayed.state, live,
        "replaying the log produced different state than the run that wrote it"
    );
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
        Scenario::new(
            "second-opinion-passes",
            SECOND_OPINION_CONFIG,
            vec![Effect::EditFile],
        )
        .cross_vendor(FRONTIER_AUTH_PLAN, vec![ReviewBehavior::Pass]),
        Scenario::new(
            "second-opinion-rejects",
            SECOND_OPINION_CONFIG,
            vec![Effect::EditFile],
        )
        .cross_vendor(FRONTIER_AUTH_PLAN, vec![ReviewBehavior::Fail])
        .answered(vec![Answer::Declined]),
        Scenario::new(
            "self-review-rebind",
            FRONTIER_ONLY_CONFIG,
            vec![Effect::EditFile],
        )
        .cross_vendor(FRONTIER_AUTH_PLAN, vec![ReviewBehavior::Pass]),
        Scenario::new(
            "budget-stop",
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [budgets]\nrun_usd = 0.05\n",
            vec![Effect::EditFile],
        ),
        Scenario::new(
            "approve-spend",
            "[routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.005 }\n",
            vec![Effect::NoEdit, Effect::EditFile],
        )
        .answered(vec![Answer::Answered {
            text: "approve: run the escalated attempt".to_owned(),
        }]),
    ];

    for Scenario {
        name,
        config,
        plan,
        effects,
        reviews,
        second_opinion,
        answers,
    } in scenarios
    {
        let repo = temp_engine_repo(&format!("replay-{name}"));
        seed(
            &repo,
            plan.unwrap_or(
                "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
                     ## Independent\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
            ),
            Some(config),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("upstroke.toml"));
        let cross_vendor_scenario = second_opinion.is_some();
        let source = match second_opinion {
            Some(second) => cross_vendor(effects, reviews, second),
            None => source(effects, reviews),
        };
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
        if cross_vendor_scenario {
            let judged: Vec<&str> = report
                .tasks
                .iter()
                .flat_map(|t| &t.attempts)
                .flat_map(|a| &a.reviews)
                .map(|r| r.agent.as_str())
                .collect();
            assert!(
                judged.contains(&"copilot"),
                "{name}: the second vendor never judged anything, so this scenario \
                     exercises nothing new: {judged:?}"
            );
        }
        assert_live_equals_replay(&repo, &live, &report);
    }
}

#[test]
fn an_aborting_error_still_leaves_a_replayable_log() {
    let repo = temp_engine_repo("abortlog");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
    opts.attempt_timeout = Duration::from_secs(60);
    let report = run_with(&opts, &source).expect("the run itself succeeds");

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
fn a_run_that_has_spent_nothing_totals_positive_zero() {
    let nothing: [f64; 0] = [];
    assert!(
        nothing.iter().sum::<f64>().is_sign_negative(),
        "`sum` no longer folds from -0.0, so `total_of` is obsolete"
    );

    assert!(!total_of(&[]).is_sign_negative(), "a spent-nothing total");
    assert_eq!(format!("${:.4}", total_of(&[])), "$0.0000");

    let spent = vec![
        task_report_costing(Some(0.25), Some(1.5)),
        task_report_costing(None, None),
        task_report_costing(Some(0.0), None),
    ];
    assert!((total_of(&spent) - 1.75).abs() < f64::EPSILON);
}

fn task_report_costing(worker: Option<f64>, review: Option<f64>) -> TaskReport {
    TaskReport {
        id: "t".to_owned(),
        title: String::new(),
        model: String::new(),
        status: TaskRunStatus::Skipped,
        duration: Duration::ZERO,
        cost_usd: worker,
        review_models: Vec::new(),
        review_cost_usd: review,
        review_cost_incomplete: false,
        session_id: None,
        attempts: Vec::new(),
    }
}

#[test]
fn a_live_run_reads_as_running_rather_than_halted() {
    let repo = temp_engine_repo("livestatus");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Waits on the widget\n<!-- upstroke: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let paths = paths_of(&repo, &report.run_id);

    let text = fs::read_to_string(paths.events()).expect("log");
    let lines: Vec<&str> = text.lines().collect();
    let cut = lines
        .iter()
        .position(|line| line.contains("\"attempt_finished\""))
        .expect("an attempt");
    fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");

    let stopped = replay_of(&repo, &report.run_id);
    assert!(stopped.interrupted_run());
    let out = crate::status::render(&stopped);
    assert!(out.contains("skipped (run interrupted)"), "{out}");
    assert!(out.contains("t2: blocked by `t1`"), "{out}");

    let lock = RunLock::acquire(&paths.public).expect("simulate a live engine");

    let live = replay_of(&repo, &report.run_id);
    assert!(live.running, "a held lock means an engine is driving this");
    assert_eq!(
        live.interrupted, 0,
        "an attempt in flight has not been interrupted"
    );
    let out = crate::status::render(&live);
    assert!(out.contains("t1: running now"), "{out}");
    assert!(out.contains("t2: queued"), "{out}");
    assert!(out.contains("t3: queued"), "{out}");
    assert!(out.contains("run in progress"), "{out}");
    for lie in [
        "small failed",
        "skipped (run halted)",
        "skipped (run interrupted)",
        "run complete",
        "run interrupted",
        "blocked by",
    ] {
        assert!(!out.contains(lie), "a live run reported `{lie}`:\n{out}");
    }
    drop(lock);
}

#[test]
fn a_truncated_run_resumes_without_spending_the_interrupted_attempt() {
    let repo = temp_engine_repo("resumetrunc");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::EditFile);
    let report = run_with(&opts, &source).expect("run");
    let run_id = report.run_id.clone();
    let paths = paths_of(&repo, &run_id);

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
    let repo = temp_engine_repo("crashkill");
    seed(
        &repo,
        "## First\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Second\n<!-- upstroke: id=t2 kind=implement depends=t1 -->\n",
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
        .env("UPSTROKE_CRASH_REPO", &repo)
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
        crate::status::render(&before).contains(&format!("upstroke resume {run_id}")),
        "and tells the operator how to continue it"
    );

    assert!(
        !rundir::is_running(&paths.public),
        "the OS released the lock"
    );

    let rendered = crate::status::render(&before);
    assert!(
        rendered.contains("run interrupted: 1 task(s) committed so far"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("run complete"),
        "a killed run claimed it completed:\n{rendered}"
    );
    assert!(rendered.contains("skipped (run interrupted)"), "{rendered}");

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

#[test]
#[ignore = "spawned by killing_a_run_mid_attempt_leaves_a_resumable_record"]
fn crash_child_dies_inside_an_attempt() {
    let Ok(repo) = std::env::var("UPSTROKE_CRASH_REPO") else {
        return;
    };
    let repo = PathBuf::from(repo);
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::EditFile, Effect::Exit],
        vec![ReviewBehavior::Pass],
    );
    let _ = run_with(&opts, &source);
    std::process::exit(0);
}

#[test]
fn a_parked_run_is_answered_out_of_band_and_resumed() {
    let repo = temp_engine_repo("answerresume");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Depends on it\n<!-- upstroke: id=t2 kind=implement depends=t1 -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
    let repo = temp_engine_repo("midrun");
    seed(
        &repo,
        "## Asks a question\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::AskQuestion, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
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

struct AnsweringViaFile {
    repo: PathBuf,
}

impl AnswerSource for AnsweringViaFile {
    fn id(&self) -> &'static str {
        "test-file-writer"
    }

    fn resolve(&self, question: &Question) -> Result<Answer, UpstrokeError> {
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
    let repo = temp_engine_repo("blocked");
    seed(
        &repo,
        "## Last\n<!-- upstroke: id=late kind=implement depends=mid -->\n\n\
             ## Middle\n<!-- upstroke: id=mid kind=implement depends=first -->\n\n\
             ## First\n<!-- upstroke: id=first kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
    let repo = temp_engine_repo("unblock");
    seed(
        &repo,
        "## Last\n<!-- upstroke: id=late kind=implement depends=mid -->\n\n\
             ## Middle\n<!-- upstroke: id=mid kind=implement depends=first -->\n\n\
             ## First\n<!-- upstroke: id=first kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
    let repo = temp_engine_repo("terminate");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- upstroke: id=t2 kind=implement depends= -->\n\n\
             ## After one\n<!-- upstroke: id=t3 kind=implement depends=t1 -->\n",
        Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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

fn resume_err(repo: &Path, run_id: &str) -> String {
    let source = fake(Effect::EditFile);
    resume_with(&resume_options(repo, run_id), &source)
        .expect_err("resume must refuse")
        .to_string()
}

const PARKED_RUN_CONFIG: &str = "[interaction]\nmode = \"never\"\n\n\
         [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n";

fn parked_run(tag: &str) -> (PathBuf, String) {
    parked_run_with_config(tag, PARKED_RUN_CONFIG)
}

fn parked_run_with_config(tag: &str, config: &str) -> (PathBuf, String) {
    let repo = temp_engine_repo(tag);
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(config),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::Parked);
    (repo, report.run_id)
}

#[test]
fn resume_refuses_schema_two_failed_attempt_without_recorded_decision() {
    let (repo, run_id) = parked_run("legacyfailedprefix");
    let paths = paths_of(&repo, &run_id);
    rewrite_run_started_as_schema_two(&paths);
    strip_event_field(&paths, "attempt_finished", "parking");
    truncate_log_after(&paths, "attempt_finished");
    let before = fs::read(paths.events()).expect("legacy prefix");

    let error = resume_err(&repo, &run_id);
    assert!(error.contains("failed attempt 1"), "{error}");
    assert!(
        error.contains("without its durable ladder or parking decision"),
        "{error}"
    );
    assert_eq!(
        fs::read(paths.events()).expect("refused log"),
        before,
        "refusal must not upgrade or otherwise mutate the ambiguous prefix"
    );
}

#[test]
fn resume_refuses_when_the_branch_moved_under_it() {
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
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\nNow with a body.\n",
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
fn status_export_and_resume_refuse_mutated_normalized_plan_bytes() {
    let (repo, run_id) = parked_run("normalized-plan-tamper");
    let plan_path = paths_of(&repo, &run_id).plan_json();
    let mut plan: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&plan_path).expect("frozen plan"))
            .expect("valid frozen plan");
    plan["tasks"][0]["title"] =
        serde_json::Value::String("tampered but self-hash unchanged".to_owned());
    fs::write(
        &plan_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&plan).expect("serialize plan")
        ),
    )
    .expect("replace frozen plan");

    let status_error = match crate::status::load(&repo, Some(&run_id)) {
        Ok(_) => panic!("status must authenticate the exact normalized bytes"),
        Err(error) => error.to_string(),
    };
    assert!(
        status_error.contains("normalized plan digest"),
        "{status_error}"
    );

    let export_error = match crate::export::load(&repo, &run_id) {
        Ok(_) => panic!("export must authenticate the exact normalized bytes"),
        Err(error) => error.to_string(),
    };
    assert!(
        export_error.contains("normalized plan digest"),
        "{export_error}"
    );

    let resume_error = resume_err(&repo, &run_id);
    assert!(
        resume_error.contains("exact bytes") && resume_error.contains("normalized-plan digest"),
        "{resume_error}"
    );
}

#[test]
fn resume_refuses_schema_two_spend_question_without_task_parked() {
    let (repo, run_id) = parked_run("legacyspendprefix");
    let paths = paths_of(&repo, &run_id);
    rewrite_run_started_as_schema_two(&paths);
    strip_event_field(&paths, "attempt_finished", "parking");
    truncate_log_after(&paths, "attempt_finished");
    let mut warnings = Vec::new();
    let mut log = EventLog::open(EventSite::LegacyOpenLog, &paths.events(), &mut warnings)
        .expect("legacy log");
    log.append(
        EventSite::LegacyAppend,
        EventBody::LadderEscalated {
            task: "t1".to_owned(),
            attempt: 1,
            rung: 0,
            data: events::LadderEscalated {
                to_rung: 1,
                tier: "small".to_owned(),
                summary: "escalate".to_owned(),
                detail: None,
            },
        },
    )
    .expect("legacy escalation");
    log.append(
        EventSite::LegacyAppend,
        EventBody::QuestionRaised {
            task: "t1".to_owned(),
            data: Box::new(events::QuestionRaised {
                question: Question {
                    id: QuestionId::from("q-spend-prefix"),
                    kind: QuestionKind::ApproveSpend,
                    affected_tasks: vec![TaskId::from("t1")],
                    context: "approve spend".to_owned(),
                    options: Vec::new(),
                },
            }),
        },
    )
    .expect("legacy question");
    drop(log);
    let before = fs::read(paths.events()).expect("ambiguous prefix");

    let error = resume_err(&repo, &run_id);
    assert!(error.contains("ApproveSpend"), "{error}");
    assert!(error.contains("before durably parking the task"), "{error}");
    assert_eq!(
        fs::read(paths.events()).expect("refused log"),
        before,
        "refusal never upgrades the spend-approval gap"
    );
}

#[test]
fn legacy_status_still_refuses_a_mismatched_self_reported_plan_hash() {
    let (repo, run_id) = parked_run("legacy-status-plan-hash");
    let paths = paths_of(&repo, &run_id);
    rewrite_run_started_as_schema_two(&paths);
    let plan_path = paths.plan_json();
    let mut plan: serde_json::Value =
        serde_json::from_slice(&fs::read(&plan_path).expect("frozen plan"))
            .expect("valid frozen plan");
    plan["source"]["hash"] = serde_json::Value::String("different-plan".to_owned());
    fs::write(
        &plan_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&plan).expect("serialize plan")
        ),
    )
    .expect("replace frozen plan");

    let error = match crate::status::load(&repo, Some(&run_id)) {
        Ok(_) => panic!("legacy status retains its source-hash boundary"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("frozen plan hash"), "{error}");
    assert!(error.contains("different-plan"), "{error}");
}

#[test]
fn legacy_upgrade_never_blesses_a_modified_normalized_snapshot() {
    let (repo, run_id) = parked_run("legacy-plan-upgrade-tamper");
    let paths = paths_of(&repo, &run_id);
    rewrite_run_started_as_schema_two(&paths);
    let plan_path = paths.plan_json();
    let mut plan: serde_json::Value =
        serde_json::from_slice(&fs::read(&plan_path).expect("frozen plan"))
            .expect("valid frozen plan");
    plan["tasks"][0]["title"] = serde_json::Value::String("modified snapshot".to_owned());
    fs::write(
        &plan_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&plan).expect("serialize plan")
        ),
    )
    .expect("tamper legacy snapshot");
    let before = fs::read(paths.events()).expect("legacy log");

    let error = resume_err(&repo, &run_id);
    assert!(error.contains("Refusing to bless"), "{error}");
    assert_eq!(
        fs::read(paths.events()).expect("refused log"),
        before,
        "refusal happens before the schema upgrade append"
    );
}

#[test]
fn resume_compares_canonical_source_semantics_to_the_recorded_plan_digest() {
    let (repo, run_id) = parked_run("source-semantics-digest");
    fs::write(
        repo.join("plan.md"),
        "## Changed semantics\n<!-- upstroke: id=t1 kind=implement depends= -->\nDifferent body.\n",
    )
    .expect("change source plan");
    let new_hash = crate::ir::content_hash(&fs::read(repo.join("plan.md")).expect("plan"));
    let paths = paths_of(&repo, &run_id);
    let text = fs::read_to_string(paths.events()).expect("log");
    let rewritten = text
        .lines()
        .map(|line| {
            let mut value: serde_json::Value = serde_json::from_str(line).expect("event");
            if value["event"] == "run_started" {
                value["data"]["plan_hash"] = serde_json::Value::String(new_hash.clone());
            }
            value.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(paths.events(), format!("{rewritten}\n")).expect("force legacy hash guard equal");

    let error = resume_err(&repo, &run_id);
    assert!(
        error.contains("validated source plan now normalizes to digest"),
        "{error}"
    );
}

#[test]
fn resume_refuses_when_routing_moved_under_a_recorded_rung() {
    let (repo, run_id) = parked_run("chainmoved");
    fs::write(
        repo.join("upstroke.toml"),
        "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n",
    )
    .expect("edit config");

    let err = resume_err(&repo, &run_id);
    assert!(err.contains("routing has changed"), "got: {err}");
    assert!(err.contains("`t1` ran on [small]"), "names the task: {err}");
}

fn parked_run_with_gate(tag: &str, cmd: &str) -> (PathBuf, String) {
    parked_run_with_config(tag, &gate_config(cmd))
}

fn gate_config(cmd: &str) -> String {
    format!("{PARKED_RUN_CONFIG}\n[[gates]]\nname = \"check\"\ncmd = \"{cmd}\"\n")
}

fn resume_answering(repo: &Path, run_id: &str, effect: Effect) -> RunReport {
    let source = source(vec![effect], vec![ReviewBehavior::Pass]);
    let answers = ScriptedAnswers::new(vec![Answer::Answered {
        text: "carry on".to_owned(),
    }]);
    resume_harness(
        &resume_options(repo, run_id),
        &Harness {
            adapters: &source,
            answers: Some(&answers),
            sleeper: None,
        },
    )
    .expect("resume")
}

#[test]
fn resume_runs_the_gates_the_run_recorded_not_todays() {
    let (repo, run_id) = parked_run_with_gate("gaterecorded", "git --version");
    fs::write(
        repo.join("upstroke.toml"),
        gate_config("git frobnicate-not-a-command"),
    )
    .expect("edit config");

    let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    assert!(
        committed(&resumed, "t1"),
        "the recorded gate ran, not the one in today's config: {resumed:?}"
    );
    let warning = resumed
        .warnings
        .iter()
        .find(|w| w.contains("differ from the ones this run recorded"))
        .unwrap_or_else(|| panic!("no gate-difference warning: {:?}", resumed.warnings));
    assert!(
        warning.contains(
            "`check` runs `git --version` and today's config says `git \
                              frobnicate-not-a-command`"
        ),
        "names the edit: {warning}"
    );
    assert!(
        warning.contains("Start a new run to adopt them"),
        "and what to do about it: {warning}"
    );
    assert_eq!(resumed.gates, ["check"]);
}

#[test]
fn the_report_labels_gates_from_the_record_not_todays_config() {
    let (repo, run_id) = parked_run_with_gate("gatelabel", "git --version");
    fs::write(repo.join("upstroke.toml"), PARKED_RUN_CONFIG).expect("edit config");

    let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(resumed.gates, ["check"], "the recorded gate ran");
    assert!(
        resumed.gates_from_config,
        "and is labelled as the record has it, not as today's config would"
    );
    let replayed = replay_of(&repo, &run_id).report();
    assert_eq!(replayed.gates, resumed.gates);
    assert_eq!(replayed.gates_from_config, resumed.gates_from_config);
}

#[test]
fn a_resume_whose_gates_did_not_move_says_nothing_about_them() {
    let (repo, run_id) = parked_run_with_gate("gateunmoved", "git --version");
    let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    assert!(
        !resumed
            .warnings
            .iter()
            .any(|w| w.contains("gates") && w.contains("recorded")),
        "an untouched config must warn about nothing: {:?}",
        resumed.warnings
    );
}

#[test]
fn a_gate_difference_is_described_without_inventing_edits() {
    let gate = |name: &str, cmd: &str| GateSummary {
        name: name.to_owned(),
        cmd: cmd.to_owned(),
        timeout: Duration::from_secs(600),
        shell: crate::gates::ShellKind::Sh,
    };
    let check = gate("check", "cargo test");
    let only_check = std::slice::from_ref(&check);

    assert_eq!(
        gates_differ(only_check, only_check),
        None,
        "identical gates are not a difference"
    );

    let added =
        gates_differ(only_check, &[check.clone(), gate("check", "true")]).expect("a difference");
    assert!(
        added.contains("`check` (`true`) is in today's config and not in the record"),
        "names the added gate: {added}"
    );
    assert!(
        !added.contains("different order"),
        "and does not invent a reorder: {added}"
    );

    let removed = gates_differ(&[check.clone(), gate("check", "cargo clippy")], only_check)
        .expect("a difference");
    assert!(
        removed.contains("`check` (`cargo clippy`) is in the record and not in today's config"),
        "names the removed gate: {removed}"
    );

    let edited = gates_differ(only_check, &[gate("check", "true")]).expect("a difference");
    assert!(
        edited.contains("`check` runs `cargo test` and today's config says `true`"),
        "{edited}"
    );

    let renamed = gates_differ(only_check, &[gate("verify", "cargo test")]).expect("a difference");
    assert!(
        renamed.contains("`check` (`cargo test`) is in the record"),
        "{renamed}"
    );
    assert!(
        renamed.contains("`verify` (`cargo test`) is in today's config"),
        "{renamed}"
    );

    let reshelled = gates_differ(
        only_check,
        &[GateSummary {
            shell: crate::gates::ShellKind::Bash,
            ..check.clone()
        }],
    )
    .expect("a difference");
    assert!(
        reshelled.contains("`check` runs under `sh` and today's config says `bash`"),
        "{reshelled}"
    );

    let other = gate("test", "cargo test");
    let reordered =
        gates_differ(&[check.clone(), other.clone()], &[other, check]).expect("a difference");
    assert!(reordered.contains("in a different order"), "{reordered}");
    assert!(
        !reordered.contains("not in the record"),
        "nothing came or went: {reordered}"
    );
}

#[test]
fn a_log_that_predates_the_gate_record_rederives_and_says_what_it_can() {
    let (repo, run_id) = parked_run_with_gate("oldgatelog", "git --version");
    strip_run_started_field(&paths_of(&repo, &run_id), "gate_cmds");
    fs::write(
        repo.join("upstroke.toml"),
        format!("{PARKED_RUN_CONFIG}\n[[gates]]\nname = \"renamed\"\ncmd = \"git --version\"\n"),
    )
    .expect("edit config");

    let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    let warning = resumed
        .warnings
        .iter()
        .find(|w| w.contains("predates the gate record"))
        .unwrap_or_else(|| panic!("no warning: {:?}", resumed.warnings));
    assert!(
        warning.contains("the gate names have moved"),
        "an old log still knows this much: {warning}"
    );
    assert!(
        warning.contains("recorded [check]") && warning.contains("resolves [renamed]"),
        "and says which: {warning}"
    );
}

#[test]
fn the_resume_that_rederives_an_old_logs_gates_records_them_for_the_next_one() {
    let (repo, run_id) = parked_run_with_gate("oldgateestablish", "git --version");
    strip_run_started_field(&paths_of(&repo, &run_id), "gate_cmds");

    let first = resume_answering(&repo, &run_id, Effect::NoEdit);
    assert_eq!(first.outcome(), RunOutcome::Parked, "{first:?}");
    assert!(
        first
            .warnings
            .iter()
            .any(|w| w.contains("predates the gate record")),
        "the first resume re-derived: {:?}",
        first.warnings
    );

    let paths = paths_of(&repo, &run_id);
    let mut log_warnings = Vec::new();
    let logged = events::read_all(&paths.events(), &mut log_warnings).expect("log");
    let established = events::recorded_gates(&logged).expect("the resume recorded its gates");
    assert_eq!(established.len(), 1);
    assert_eq!(established[0].cmd, "git --version");

    fs::write(
        repo.join("upstroke.toml"),
        gate_config("git frobnicate-not-a-command"),
    )
    .expect("edit config");

    let second = resume_answering(&repo, &run_id, Effect::EditFile);
    assert_eq!(second.outcome(), RunOutcome::Complete, "{second:?}");
    assert!(
        committed(&second, "t1"),
        "the established gate ran, not the weakened one: {second:?}"
    );
    assert!(
        second
            .warnings
            .iter()
            .any(|w| w.contains("differ from the ones this run recorded")),
        "{:?}",
        second.warnings
    );
    assert!(
        !second
            .warnings
            .iter()
            .any(|w| w.contains("predates the gate record")),
        "the log is no longer speechless about its gates: {:?}",
        second.warnings
    );
}

#[test]
fn an_old_gateless_log_is_not_warned_at_about_nothing() {
    let (repo, run_id) = parked_run("oldgatelessslog");
    strip_run_started_field(&paths_of(&repo, &run_id), "gate_cmds");

    let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
    assert!(
        !resumed.warnings.iter().any(|w| w.contains("gate")),
        "nothing to say: {:?}",
        resumed.warnings
    );
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
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
    let _held = RunLock::acquire(&paths.public).expect("hold it");

    let err = resume_err(&repo, &run_id);
    assert!(err.contains("already driving run"), "got: {err}");
}

#[test]
fn an_unknown_run_id_lists_what_is_there() {
    let (repo, _) = parked_run("unknownid");
    let err = resume_err(&repo, "01NOPE");
    assert!(err.contains("known runs"), "got: {err}");
}

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
        rendered.contains("per-pool drain: no pool is connected"),
        "{rendered}"
    );
    assert!(
        rendered.contains(&loaded.paths.private.display().to_string()),
        "and points at where the transcripts actually are"
    );

    assert!(
        (loaded.report().total_cost_usd - report.total_cost_usd).abs() < 1e-9,
        "{} vs {}",
        loaded.report().total_cost_usd,
        report.total_cost_usd
    );

    let paths = paths_of(&repo, &report.run_id);
    let _held = RunLock::acquire(&paths.public).expect("claim the finished run");
    let claimed = replay_of(&repo, &report.run_id);
    assert!(!claimed.running, "a finished run is not running");
    assert!(claimed.held, "but something does hold it");
    let rendered = crate::status::render(&claimed);
    assert!(
        rendered.contains("another process holds this run"),
        "{rendered}"
    );
    assert!(rendered.contains("run complete"), "{rendered}");
    assert!(!rendered.contains("run in progress"), "{rendered}");
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
fn follow_ignores_a_terminal_marker_superseded_by_resume() {
    let repo = temp_engine_repo("followresume");
    let source = fake(Effect::EditFile);
    let report = run_with(&options(&repo), &source).expect("run");
    let paths = paths_of(&repo, &report.run_id);
    let mut warnings = Vec::new();
    let mut log = events::EventLog::open(EventSite::LegacyOpenLog, &paths.events(), &mut warnings)
        .expect("open log");
    log.append(
        EventSite::LegacyAppend,
        EventBody::RunResumed {
            data: events::RunResumed {
                head_sha: "second-epoch".to_owned(),
                interrupted_attempts: 0,
                discarded: Vec::new(),
                gates: None,
                effort_policy: None,
                reviews: None,
                chains: None,
                normalized_plan_digest: None,
            },
        },
    )
    .expect("resume marker");
    log.append(
        EventSite::LegacyAppend,
        EventBody::RunFinished {
            data: events::RunFinished {
                outcome: events::RunOutcome::Complete,
                halted_at: None,
                committed: 1,
                parked: 0,
            },
        },
    )
    .expect("second finish");
    drop(log);

    let loaded = replay_of(&repo, &report.run_id);
    let sleeper = RecordingSleeper::default();
    let mut out = Vec::new();
    crate::status::follow(&loaded, &sleeper, Duration::ZERO, 2, &mut out).expect("follow");
    let text = String::from_utf8(out).expect("utf8");
    assert!(text.contains("resumed at second-epo"), "{text}");
    assert_eq!(
        text.matches("run finished").count(),
        2,
        "the historical finish must not truncate the later epoch: {text}"
    );
}

#[test]
fn follow_waits_at_held_historical_terminal_until_resume_marker() {
    struct ResumeOnSleep {
        events: PathBuf,
        lock: Mutex<Option<RunLock>>,
    }

    impl Sleeper for ResumeOnSleep {
        fn sleep(&self, _: Duration) {
            let Ok(mut lock) = self.lock.lock() else {
                return;
            };
            if lock.is_none() {
                return;
            }
            let mut warnings = Vec::new();
            let mut log =
                events::EventLog::open(EventSite::LegacyOpenLog, &self.events, &mut warnings)
                    .expect("log");
            log.append(
                EventSite::LegacyAppend,
                EventBody::RunResumed {
                    data: events::RunResumed {
                        head_sha: "resumed-head".to_owned(),
                        interrupted_attempts: 0,
                        discarded: Vec::new(),
                        gates: None,
                        effort_policy: None,
                        reviews: None,
                        chains: None,
                        normalized_plan_digest: None,
                    },
                },
            )
            .expect("resume marker");
            log.append(
                EventSite::LegacyAppend,
                EventBody::RunFinished {
                    data: events::RunFinished {
                        outcome: events::RunOutcome::Complete,
                        halted_at: None,
                        committed: 1,
                        parked: 0,
                    },
                },
            )
            .expect("new terminal");
            drop(log);
            drop(lock.take());
        }
    }

    let repo = temp_engine_repo("followheldterminal");
    let report = run_with(&options(&repo), &fake(Effect::EditFile)).expect("run");
    let paths = paths_of(&repo, &report.run_id);
    let loaded = replay_of(&repo, &report.run_id);
    let sleeper = ResumeOnSleep {
        events: paths.events(),
        lock: Mutex::new(Some(
            RunLock::acquire(&paths.public).expect("resume owns lock before marker"),
        )),
    };
    let mut out = Vec::new();
    crate::status::follow(&loaded, &sleeper, Duration::ZERO, 1, &mut out).expect("follow");
    let text = String::from_utf8(out).expect("utf8");
    assert!(text.contains("resumed at resumed-he"), "{text}");
    assert_eq!(text.matches("run finished").count(), 2, "{text}");
}

#[test]
fn transcripts_live_outside_the_workspace_and_survive_a_rollback() {
    let repo = temp_engine_repo("private");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
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
    assert!(paths.events().starts_with(&repo));
    assert!(paths.questions().starts_with(&repo));
    let in_repo = repo.join(".upstroke").join("runs").join(&report.run_id);
    for leaked in ["transcripts", "reviews", "settings", "gates"] {
        assert!(
            !in_repo.join(leaked).exists(),
            "{leaked}/ must not exist inside the workspace"
        );
    }
}

fn strip_run_started_field(paths: &RunPaths, field: &str) {
    let text = fs::read_to_string(paths.events()).expect("log");
    let mut stripped = false;
    let rewritten: Vec<String> = text
        .lines()
        .map(|line| {
            let mut value: serde_json::Value =
                serde_json::from_str(line).expect("every line is an event");
            if value.get("event").and_then(|e| e.as_str()) == Some("run_started") {
                if let Some(data) = value.get_mut("data").and_then(|d| d.as_object_mut()) {
                    data.remove(field)
                        .unwrap_or_else(|| panic!("the run recorded no `{field}`"));
                    stripped = true;
                }
            }
            value.to_string()
        })
        .collect();
    assert!(
        stripped,
        "the log has no run_started to strip `{field}` from"
    );
    fs::write(paths.events(), format!("{}\n", rewritten.join("\n"))).expect("rewrite");
}

fn rewrite_run_started_as_schema_one(paths: &RunPaths, absent: &[&str]) {
    let text = fs::read_to_string(paths.events()).expect("log");
    let mut rewritten_start = false;
    let rewritten: Vec<String> = text
        .lines()
        .map(|line| {
            let mut value: serde_json::Value =
                serde_json::from_str(line).expect("every line is an event");
            if value.get("event").and_then(|event| event.as_str()) == Some("run_started") {
                let data = value
                    .get_mut("data")
                    .and_then(serde_json::Value::as_object_mut)
                    .expect("run_started data");
                data.insert("schema".to_owned(), serde_json::Value::from(1));
                data.remove("normalized_plan_digest");
                for field in absent {
                    data.remove(*field)
                        .unwrap_or_else(|| panic!("the run recorded no `{field}`"));
                }
                for chain in data
                    .get_mut("chains")
                    .and_then(serde_json::Value::as_array_mut)
                    .expect("run_started chains")
                {
                    chain
                        .as_object_mut()
                        .expect("chain object")
                        .remove("bindings")
                        .expect("a schema-2 run records chain bindings");
                }
                rewritten_start = true;
            }
            value.to_string()
        })
        .collect();
    assert!(rewritten_start, "the log has no run_started event");
    fs::write(paths.events(), format!("{}\n", rewritten.join("\n"))).expect("rewrite");
}

fn rewrite_run_started_as_schema_two(paths: &RunPaths) {
    rewrite_run_started_as_schema_two_missing_review_fields(paths, &["pass_timeout_secs"]);
}

fn rewrite_run_started_as_schema_two_missing_review_fields(
    paths: &RunPaths,
    absent_review_fields: &[&str],
) {
    let text = fs::read_to_string(paths.events()).expect("log");
    let mut rewritten_start = false;
    let rewritten: Vec<String> = text
        .lines()
        .map(|line| {
            let mut value: serde_json::Value =
                serde_json::from_str(line).expect("every line is an event");
            if value.get("event").and_then(|event| event.as_str()) == Some("run_started") {
                let data = value
                    .get_mut("data")
                    .and_then(serde_json::Value::as_object_mut)
                    .expect("run_started data");
                data.insert("schema".to_owned(), serde_json::Value::from(2));
                data.remove("normalized_plan_digest");
                let reviews = data
                    .get_mut("reviews")
                    .and_then(serde_json::Value::as_object_mut)
                    .expect("recorded review plan");
                for field in absent_review_fields {
                    reviews
                        .remove(*field)
                        .unwrap_or_else(|| panic!("current review plan records `{field}`"));
                }
                rewritten_start = true;
            }
            value.to_string()
        })
        .collect();
    assert!(rewritten_start, "the log has no run_started event");
    fs::write(paths.events(), format!("{}\n", rewritten.join("\n"))).expect("rewrite");
}

fn strip_event_field(paths: &RunPaths, event: &str, field: &str) {
    rewrite_event_field(paths, event, field, false);
}

fn strip_event_data_field(paths: &RunPaths, event: &str, field: &str) {
    rewrite_event_field(paths, event, field, true);
}

fn rewrite_event_field(paths: &RunPaths, event: &str, field: &str, nested_in_data: bool) {
    let text = fs::read_to_string(paths.events()).expect("log");
    let mut stripped = false;
    let rewritten: Vec<String> = text
        .lines()
        .map(|line| {
            let mut value: serde_json::Value =
                serde_json::from_str(line).expect("every line is an event");
            if value.get("event").and_then(serde_json::Value::as_str) == Some(event) {
                let object = if nested_in_data {
                    value
                        .get_mut("data")
                        .and_then(serde_json::Value::as_object_mut)
                        .expect("event data")
                } else {
                    value.as_object_mut().expect("event object")
                };
                object
                    .remove(field)
                    .unwrap_or_else(|| panic!("{event} records `{field}`"));
                stripped = true;
            }
            value.to_string()
        })
        .collect();
    assert!(stripped, "the log has no `{event}.{field}` to strip");
    fs::write(paths.events(), format!("{}\n", rewritten.join("\n"))).expect("rewrite");
}

fn truncate_log_before(paths: &RunPaths, event: &str) {
    let text = fs::read_to_string(paths.events()).expect("log");
    let lines: Vec<&str> = text.lines().collect();
    let cut = lines
        .iter()
        .position(|line| line.contains(&format!("\"{event}\"")))
        .unwrap_or_else(|| panic!("the run recorded no {event}"));
    fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");
}

fn truncate_log_after(paths: &RunPaths, event: &str) {
    let text = fs::read_to_string(paths.events()).expect("log");
    let lines: Vec<&str> = text.lines().collect();
    let cut = lines
        .iter()
        .position(|line| line.contains(&format!("\"{event}\"")))
        .unwrap_or_else(|| panic!("the run recorded no {event}"))
        + 1;
    fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");
}

fn prepared_commit_of(paths: &RunPaths) -> events::PreparedCommit {
    let mut warnings = Vec::new();
    events::read_all(&paths.events(), &mut warnings)
        .expect("read prepared settlement")
        .into_iter()
        .find_map(|event| match event.body {
            EventBody::AttemptFinished {
                prepared_commit: Some(prepared),
                ..
            } => Some(*prepared),
            _ => None,
        })
        .expect("successful settlement records its prepared commit")
}

fn recreate_prepared_pin(repo: &Path, prepared: &events::PreparedCommit, target: &str) {
    let zero = "0".repeat(target.len());
    git_in(repo, &["update-ref", &prepared.pin_ref, target, &zero]);
}

#[test]
fn resume_adopts_the_commit_it_made_but_never_recorded() {
    let repo = temp_engine_repo("adoptcommit");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let run_id = report.run_id.clone();
    let paths = paths_of(&repo, &run_id);
    let sha = git_in(&repo, &["rev-parse", "HEAD"]).trim().to_owned();

    truncate_log_before(&paths, "task_committed");

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
    assert!(committed(&resumed, "t1"), "adopted rather than redone");
    assert_eq!(
        git_in(&repo, &["rev-parse", "HEAD"]).trim(),
        sha,
        "and the branch was left exactly where it stood"
    );
    assert_eq!(
        git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
        "1",
        "one commit, not a second one for the same work"
    );
    assert_eq!(
        task(&resumed, "t1").attempts.len(),
        1,
        "the attempt that passed was not spent again: {resumed:?}"
    );
    assert!(
        resumed
            .warnings
            .iter()
            .any(|w| w.contains("adopted commit")),
        "and the operator is told: {:?}",
        resumed.warnings
    );
    assert_live_equals_replay(&repo, &state, &resumed);
}

#[test]
fn resume_recovers_every_prepared_commit_ref_crash_prefix() {
    for (tag, reset_to_parent, recreate_pin) in [
        ("prepared-same-head", true, true),
        ("prepared-head-with-pin", false, true),
        ("prepared-head-no-pin", false, false),
    ] {
        let repo = temp_engine_repo(tag);
        seed(
            &repo,
            "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("upstroke.toml"));
        let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
        let paths = paths_of(&repo, &report.run_id);
        let prepared = prepared_commit_of(&paths);
        truncate_log_before(&paths, "task_committed");
        if reset_to_parent {
            git_in(&repo, &["reset", "-q", "--soft", &prepared.parent_sha]);
        }
        if recreate_pin {
            let target = prepared.commit_sha.clone();
            recreate_prepared_pin(&repo, &prepared, &target);
        }

        let source = fake(Effect::EditFile);
        let resumed = resume_harness(
            &resume_options(&repo, &report.run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("recover exact prepared object");
        assert_eq!(
            resumed.outcome(),
            RunOutcome::Complete,
            "{tag}: {resumed:?}"
        );
        assert_eq!(
            git_in(&repo, &["rev-parse", "HEAD"]).trim(),
            prepared.commit_sha,
            "{tag}: the exact reviewed object is published"
        );
        assert_eq!(task(&resumed, "t1").attempts.len(), 1, "{tag}");
        let workspace = Workspace::open(&repo).expect("workspace");
        assert_eq!(
            workspace
                .prepared_pin_target(&prepared.pin_ref)
                .expect("pin lookup"),
            None,
            "{tag}: recovery cleans the private pin"
        );
    }
}

#[test]
fn resume_removes_a_pin_whose_successful_settlement_never_landed() {
    let repo = temp_engine_repo("prepared-orphan");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let paths = paths_of(&repo, &report.run_id);
    let prepared = prepared_commit_of(&paths);
    truncate_log_before(&paths, "attempt_finished");
    git_in(&repo, &["reset", "-q", "--soft", &prepared.parent_sha]);
    let target = prepared.commit_sha.clone();
    recreate_prepared_pin(&repo, &prepared, &target);

    let source = fake(Effect::EditFile);
    let resumed = resume_harness(
        &resume_options(&repo, &report.run_id),
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
    )
    .expect("orphan pin is not a settlement");
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    assert_eq!(task(&resumed, "t1").attempts.len(), 2);
    assert!(
        resumed
            .warnings
            .iter()
            .any(|warning| warning.contains("removed orphan prepared commit pin")),
        "{:?}",
        resumed.warnings
    );
    assert_eq!(
        Workspace::open(&repo)
            .expect("workspace")
            .prepared_pin_target(&prepared.pin_ref)
            .expect("pin lookup"),
        None
    );
}

#[test]
fn resume_refuses_a_substituted_prepared_pin_without_deleting_it() {
    let repo = temp_engine_repo("prepared-pin-mismatch");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let paths = paths_of(&repo, &report.run_id);
    let prepared = prepared_commit_of(&paths);
    truncate_log_before(&paths, "task_committed");
    git_in(&repo, &["reset", "-q", "--soft", &prepared.parent_sha]);
    recreate_prepared_pin(&repo, &prepared, &prepared.parent_sha);

    let err = resume_err(&repo, &report.run_id);
    assert!(err.contains("not pinned"), "{err}");
    assert_eq!(
        Workspace::open(&repo)
            .expect("workspace")
            .prepared_pin_target(&prepared.pin_ref)
            .expect("pin lookup")
            .as_deref(),
        Some(prepared.parent_sha.as_str()),
        "refusal never deletes the substituted target"
    );
    assert_eq!(
        git_in(&repo, &["rev-parse", "HEAD"]).trim(),
        prepared.parent_sha,
        "HEAD remains at the recorded parent"
    );
}

#[test]
fn resume_refuses_symbolic_run_ref_at_already_published_prepared_prefix() {
    let repo = temp_engine_repo("prepared-symbolic-run-ref");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let paths = paths_of(&repo, &report.run_id);
    let prepared = prepared_commit_of(&paths);
    truncate_log_before(&paths, "task_committed");
    recreate_prepared_pin(&repo, &prepared, &prepared.commit_sha);

    git_in(&repo, &["branch", "victim", prepared.commit_sha.as_str()]);
    git_in(
        &repo,
        &[
            "symbolic-ref",
            prepared.branch_ref.as_str(),
            "refs/heads/victim",
        ],
    );
    let events_before = fs::read(paths.events()).expect("event bytes before refusal");
    let victim_before = git_in(&repo, &["rev-parse", "refs/heads/victim"]);

    let error = resume_err(&repo, &report.run_id);
    assert!(error.contains("itself symbolic"), "{error}");
    assert_eq!(
        fs::read(paths.events()).expect("event bytes after refusal"),
        events_before,
        "refusal happens before task_committed or any other repair append"
    );
    assert_eq!(
        Workspace::open(&repo)
            .expect("workspace")
            .prepared_pin_target(&prepared.pin_ref)
            .expect("pin lookup")
            .as_deref(),
        Some(prepared.commit_sha.as_str()),
        "refusal preserves the durable prepared pin"
    );
    assert_eq!(
        git_in(&repo, &["rev-parse", "refs/heads/victim"]),
        victim_before,
        "the symbolic run ref never advances or deletes its victim"
    );
    assert_eq!(
        git_in(
            &repo,
            &["symbolic-ref", "--no-recurse", prepared.branch_ref.as_str(),],
        )
        .trim(),
        "refs/heads/victim",
        "refusal preserves the substituted symbolic run ref for inspection"
    );
}

#[test]
fn recovered_prepared_commit_precedes_unrelated_answer_defect_repair() {
    let repo = temp_engine_repo("prepared-before-repair");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let paths = paths_of(&repo, &report.run_id);
    truncate_log_before(&paths, "task_committed");

    let question_id = QuestionId::from("q-before-success");
    let question = Question {
        id: question_id.clone(),
        kind: QuestionKind::Unblock,
        affected_tasks: vec![TaskId::from("t1")],
        context: "an earlier question".to_owned(),
        options: Vec::new(),
    };
    let inserted = [
        events::Event::now(EventBody::QuestionRaised {
            task: "t1".to_owned(),
            data: Box::new(events::QuestionRaised {
                question: question.clone(),
            }),
        }),
        events::Event::now(EventBody::TaskParked {
            task: "t1".to_owned(),
            data: events::TaskParked {
                question: question_id.to_string(),
                refund_attempt: false,
            },
        }),
        events::Event::now(EventBody::QuestionAnswered {
            data: events::QuestionAnswered {
                question: question_id,
                answer: Answer::Answered {
                    text: "continue".to_owned(),
                },
                decline_halts_run: None,
                via: "answer-file".to_owned(),
            },
        }),
    ];
    let mut lines: Vec<String> = fs::read_to_string(paths.events())
        .expect("log")
        .lines()
        .map(str::to_owned)
        .collect();
    let before_attempt = lines
        .iter()
        .position(|line| line.contains("\"attempt_started\""))
        .expect("attempt start");
    lines.splice(
        before_attempt..before_attempt,
        inserted
            .iter()
            .map(|event| serde_json::to_string(event).expect("event json")),
    );
    fs::write(paths.events(), format!("{}\n", lines.join("\n"))).expect("insert prefix");

    let source = fake(Effect::EditFile);
    let resumed = resume_harness(
        &resume_options(&repo, &report.run_id),
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
    )
    .expect("resume closes settlement before repairing older metadata");
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    let logged = events_of(&repo, &report.run_id);
    let settlement = logged
        .iter()
        .rposition(|event| matches!(event.body, EventBody::AttemptFinished { .. }))
        .expect("settlement");
    assert!(
        matches!(
            logged.get(settlement + 1).map(|event| &event.body),
            Some(EventBody::TaskCommitted { task, .. }) if task == "t1"
        ),
        "task_committed must immediately close the prepared settlement"
    );
    assert!(
        logged
            .iter()
            .skip(settlement + 2)
            .any(|event| matches!(event.body, EventBody::DesignDefect { .. })),
        "the older answer repair still lands after the commit"
    );
    events::replay(logged, vec!["t1".to_owned()], &paths.events())
        .expect("the repaired log remains replayable");
}

#[test]
fn resume_refuses_an_arbitrary_tree_with_the_same_parent_and_subject() {
    let repo = temp_engine_repo("adoptforeign");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    truncate_log_before(&paths_of(&repo, &report.run_id), "task_committed");
    let subject = git_in(&repo, &["show", "-s", "--format=%s", "HEAD"]);
    fs::write(repo.join("foreign.txt"), "not reviewed\n").expect("foreign tree");
    git_in(&repo, &["add", "foreign.txt"]);
    git_in(&repo, &["commit", "-q", "--amend", "--no-edit"]);
    assert_eq!(
        git_in(&repo, &["show", "-s", "--format=%s", "HEAD"]),
        subject,
        "the substituted commit deliberately has the expected subject"
    );

    let err = resume_err(&repo, &report.run_id);
    assert!(err.contains("record ends at"), "got: {err}");
}

#[test]
fn legacy_success_without_prepared_identity_is_never_adopted_by_subject() {
    let repo = temp_engine_repo("legacy-subject");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let paths = paths_of(&repo, &report.run_id);
    truncate_log_before(&paths, "task_committed");
    rewrite_run_started_as_schema_two(&paths);
    strip_event_field(&paths, "attempt_finished", "prepared_commit");

    let err = resume_err(&repo, &report.run_id);
    assert!(err.contains("subject alone"), "{err}");
    assert_eq!(
        git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
        "1",
        "refusal preserves the plausible legacy commit"
    );
}

#[test]
fn resume_writes_where_the_run_recorded_not_where_defaults_point() {
    let repo = temp_engine_repo("privatedir");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let run_id = report.run_id.clone();
    let recorded = paths_of(&repo, &run_id);
    truncate_log_before(&recorded, "attempt_finished");
    git_in(&repo, &["reset", "-q", "--hard", "HEAD~1"]);

    let mut resume = resume_options(&repo, &run_id);
    resume.private_root = None;
    let source = fake(Effect::EditFile);
    let resumed = resume_harness(
        &resume,
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
    )
    .expect("resume");
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    assert!(
        recorded.transcripts().join("00-t1-2.json").is_file(),
        "the resumed attempt wrote under {}",
        recorded.transcripts().display()
    );
}

#[test]
fn resume_makes_a_stale_question_payload_agree_with_the_log() {
    let repo = temp_engine_repo("stalepayload");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let doomed = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
    let report = run_with(&opts, &doomed).expect("run");
    let run_id = report.run_id.clone();
    let first = report.questions[0].question.id.to_string();

    crate::answer::answer(
        &repo,
        &first,
        crate::answer::Reply::Text("try again".to_owned()),
    )
    .expect("answer");

    let retry = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
    let resumed = resume_with(&resume_options(&repo, &run_id), &retry).expect("resume");
    assert_eq!(resumed.outcome(), RunOutcome::Parked, "{resumed:?}");

    let questions = rundir::public_dir(&repo, &run_id).join("questions");
    let path = questions.join(format!("{first}.json"));
    let mut record: QuestionRecord =
        serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("parse");
    record.answer = None;
    interaction::write_question(&questions, &record).expect("rewrite");
    crate::answer::answer(&repo, &first, crate::answer::Reply::Decline)
        .expect("a stale payload is exactly what makes a second answer look acceptable");

    let source = fake(Effect::EditFile);
    resume_with(&resume_options(&repo, &run_id), &source).expect("second resume");

    let record: QuestionRecord =
        serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("parse");
    assert!(
        !record.is_open(),
        "the payload agrees with the log again, so nobody answers it twice"
    );
}

#[test]
fn a_run_that_never_started_leaves_no_directory_behind() {
    let repo = temp_engine_repo("husk");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    git_in(&repo, &["branch", "upstroke"]);

    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::EditFile);
    run_with(&opts, &source).expect_err("the run branch cannot be created");

    assert_eq!(
        rundir::latest_run(&repo),
        None,
        "no husk left behind to shadow the next run"
    );
}

struct BacklogAnswers {
    repo: PathBuf,
    used: Mutex<bool>,
}

impl AnswerSource for BacklogAnswers {
    fn id(&self) -> &'static str {
        "backlog"
    }

    fn resolve(&self, question: &Question) -> Result<Answer, UpstrokeError> {
        let Ok(mut used) = self.used.lock() else {
            return Ok(Answer::Unanswered);
        };
        if *used {
            return Ok(Answer::Unanswered);
        }
        *used = true;
        let run = rundir::latest_run(&self.repo).expect("a run");
        let dir = rundir::public_dir(&self.repo, &run).join("questions");
        let other = fs::read_dir(&dir)
            .expect("questions dir")
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".json").map(str::to_owned)
            })
            .find(|id| id.as_str() != question.id.as_str());
        if let Some(other) = other {
            let _ = crate::answer::answer(
                &self.repo,
                &other,
                crate::answer::Reply::Text("write src/other.rs".to_owned()),
            );
        }
        Ok(Answer::Answered {
            text: "write src/widget.rs".to_owned(),
        })
    }
}

#[test]
fn a_typed_answer_survives_another_question_being_answered_at_the_same_time() {
    let repo = temp_engine_repo("backlog");
    seed(
        &repo,
        "## First\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Second\n<!-- upstroke: id=t2 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::NoEdit, Effect::NoEdit, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
    let answers = BacklogAnswers {
        repo: repo.clone(),
        used: Mutex::new(false),
    };
    let report = run_harness(
        &opts,
        &Harness {
            adapters: &source,
            answers: Some(&answers),
            sleeper: None,
        },
    )
    .expect("run");

    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
    for id in ["t1", "t2"] {
        assert!(committed(&report, id), "{id} was released: {report:?}");
    }
}

#[test]
fn an_answer_file_that_changes_nothing_does_not_spin_the_scheduler() {
    let repo = temp_engine_repo("nullanswer");
    seed(
        &repo,
        "## Doomed\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::Parked);
    let run_id = report.run_id.clone();
    let question = report.questions[0].question.id.clone();

    let answers = rundir::public_dir(&repo, &run_id).join("answers");
    fs::create_dir_all(&answers).expect("answers dir");
    interaction::write_answer(&answers, &question, &Answer::Unanswered).expect("write");

    let source = fake(Effect::EditFile);
    let resumed = resume_with(&resume_options(&repo, &run_id), &source).expect("resume");
    assert_eq!(
        resumed.outcome(),
        RunOutcome::Parked,
        "still waiting on a real answer, and the run ended saying so: {resumed:?}"
    );
}

struct LockReleasingSleeper {
    waits: Mutex<u32>,
    release_after: u32,
    lock: Mutex<Option<RunLock>>,
}

impl LockReleasingSleeper {
    fn new(lock: RunLock, release_after: u32) -> Self {
        Self {
            waits: Mutex::new(0),
            release_after,
            lock: Mutex::new(Some(lock)),
        }
    }

    fn waits(&self) -> u32 {
        self.waits.lock().map(|w| *w).unwrap_or(0)
    }
}

impl Sleeper for LockReleasingSleeper {
    fn sleep(&self, _: Duration) {
        let Ok(mut waits) = self.waits.lock() else {
            return;
        };
        *waits += 1;
        if *waits == self.release_after {
            if let Ok(mut lock) = self.lock.lock() {
                drop(lock.take());
            }
        }
    }
}

#[test]
fn following_waits_out_a_silent_live_run_and_stops_once_it_dies() {
    let repo = temp_engine_repo("followlive");
    let source = fake(Effect::EditFile);
    let report = run_with(&options(&repo), &source).expect("run");
    let paths = paths_of(&repo, &report.run_id);

    let text = fs::read_to_string(paths.events()).expect("log");
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.contains("\"run_finished\""))
        .collect();
    fs::write(paths.events(), format!("{}\n", kept.join("\n"))).expect("truncate");

    let loaded = replay_of(&repo, &report.run_id);
    let held = RunLock::acquire(&paths.public).expect("simulate a live engine");
    let sleeper = LockReleasingSleeper::new(held, 5);
    let mut out: Vec<u8> = Vec::new();
    crate::status::follow(&loaded, &sleeper, Duration::ZERO, 1, &mut out).expect("follow");

    assert!(
        sleeper.waits() > 5,
        "watched the live run past its idle budget and stopped once the lock went, \
             instead of timing out its silence: {} sleeps",
        sleeper.waits()
    );
}

fn pools_file(repo: &Path, content: &str) -> PathBuf {
    let dir = private_root_for(repo);
    fs::create_dir_all(&dir).expect("pools dir");
    let path = dir.join("pools.toml");
    fs::write(&path, content).expect("pools file");
    path
}

const CLAUDE_POOL: &str = "[pools.claude-max]\nkind = \"subscription-window\"\nagent = \
                               \"claude-code\"\nsources = [\"signals\", \"self\"]\n";

fn events_of(repo: &Path, run_id: &str) -> Vec<events::Event> {
    let mut ignored = Vec::new();
    events::read_all(&paths_of(repo, run_id).events(), &mut ignored).expect("the log reads")
}

fn budget_events(events: &[events::Event]) -> Vec<&events::BudgetExceeded> {
    events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::BudgetExceeded { data } => Some(data),
            _ => None,
        })
        .collect()
}

#[test]
fn a_run_budget_stops_the_run_exactly_once_and_survives_replay() {
    let repo = temp_engine_repo("budgetstop");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- upstroke: id=t2 kind=implement depends= -->\n\n\
             ## Three\n<!-- upstroke: id=t3 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [budgets]\nrun_usd = 0.05\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::EditFile);
    let (report, live) = run_harness_inner(
        &opts,
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
    )
    .expect("a budget stop is not an engine error");

    assert_eq!(report.outcome(), RunOutcome::BudgetExceeded, "{report:?}");
    assert!(committed(&report, "t1"));
    let stop = report.budget_stop.as_ref().expect("a recorded stop");
    assert_eq!(stop.budget, events::BudgetKind::Run);
    assert_eq!(stop.task, "t2", "names the task that did not start");
    assert!(stop.spent_usd >= 0.05, "spent: {}", stop.spent_usd);

    let events = events_of(&repo, &report.run_id);
    assert_eq!(
        budget_events(&events).len(),
        1,
        "{:?}",
        budget_events(&events)
    );

    assert!(matches!(task(&report, "t2").status, TaskRunStatus::Skipped));
    assert!(task(&report, "t2").attempts.is_empty());
    assert_live_equals_replay(&repo, &live, &report);
}

#[test]
fn a_task_budget_also_ends_the_run_and_says_which_ceiling_it_was() {
    let repo = temp_engine_repo("taskbudget");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n\n\
                 [budgets]\ntask_usd = 0.005\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::NoEdit, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::BudgetExceeded, "{report:?}");
    let stop = report.budget_stop.as_ref().expect("a recorded stop");
    assert_eq!(stop.budget, events::BudgetKind::Task);
    assert_eq!(stop.task, "t1");
    assert_eq!(
        task(&report, "t1").attempts.len(),
        1,
        "the escalated attempt never spawned"
    );
    let rendered = report.render();
    assert!(rendered.contains("task_usd"), "{rendered}");
    assert!(rendered.contains("upstroke resume"), "{rendered}");
}

#[test]
fn resuming_with_a_higher_ceiling_continues_the_run_the_budget_stopped() {
    let repo = temp_engine_repo("budgetresume");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- upstroke: id=t2 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    opts.budget_usd = Some(0.05);
    let source = fake(Effect::EditFile);
    let stopped = run_with(&opts, &source).expect("run");
    assert_eq!(stopped.outcome(), RunOutcome::BudgetExceeded);
    assert!(!committed(&stopped, "t2"));

    let mut resume_opts = resume_options(&repo, &stopped.run_id);
    resume_opts.budget_usd = Some(10.0);
    let source = fake(Effect::EditFile);
    let resumed = resume_harness(
        &resume_opts,
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
    )
    .expect("a budget stop is exactly what resume is for");
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    assert!(committed(&resumed, "t2"));
    assert!(
        resumed.budget_stop.is_none(),
        "the stop the resume got past must not still be reported"
    );
}

#[test]
fn a_resume_that_does_not_raise_the_ceiling_stops_again_rather_than_running_past_it() {
    let repo = temp_engine_repo("budgetresumelow");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- upstroke: id=t2 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [budgets]\nrun_usd = 0.05\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::EditFile);
    let stopped = run_with(&opts, &source).expect("run");
    assert_eq!(stopped.outcome(), RunOutcome::BudgetExceeded);

    let source = fake(Effect::EditFile);
    let again = resume_harness(
        &resume_options(&repo, &stopped.run_id),
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
    )
    .expect("resume");
    assert_eq!(again.outcome(), RunOutcome::BudgetExceeded, "{again:?}");
    assert!(!committed(&again, "t2"));
}

#[test]
fn a_frontier_escalation_over_the_threshold_parks_for_approval_then_runs_it() {
    let repo = temp_engine_repo("approvespend");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.005 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::NoEdit, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
    let scripted = ScriptedAnswers::new(vec![Answer::Answered {
        text: "approve: run the escalated attempt".to_owned(),
    }]);
    let (report, live) = run_harness_inner(
        &opts,
        &Harness {
            adapters: &source,
            answers: Some(&scripted),
            sleeper: None,
        },
    )
    .expect("run");

    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
    assert!(committed(&report, "t1"));
    let asked: Vec<&QuestionRecord> = report
        .questions
        .iter()
        .filter(|q| q.question.kind == QuestionKind::ApproveSpend)
        .collect();
    assert_eq!(asked.len(), 1, "asked once: {:?}", report.questions);
    assert!(
        asked[0].question.context.contains("frontier"),
        "the question names where the money is going: {}",
        asked[0].question.context
    );
    assert!(
        asked[0].question.context.contains("$0.0100"),
        "and quotes reported spend to date: {}",
        asked[0].question.context
    );

    let tiers: Vec<&str> = task(&report, "t1")
        .attempts
        .iter()
        .map(|a| a.tier.as_str())
        .collect();
    assert_eq!(tiers, ["mid", "frontier"], "{tiers:?}");
    assert_live_equals_replay(&repo, &live, &report);
}

#[test]
fn a_declined_spend_approval_fails_the_task_through_the_halt_policy() {
    let repo = temp_engine_repo("declinespend");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.005 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::NoEdit, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
    let scripted = ScriptedAnswers::new(vec![Answer::Declined]);
    let report = run_harness(
        &opts,
        &Harness {
            adapters: &source,
            answers: Some(&scripted),
            sleeper: None,
        },
    )
    .expect("run");

    assert_eq!(report.outcome(), RunOutcome::Halted, "{report:?}");
    assert!(matches!(
        task(&report, "t1").status,
        TaskRunStatus::Failed {
            kind: FailureKind::Declined,
            ..
        }
    ));
    assert_eq!(
        task(&report, "t1").attempts.len(),
        1,
        "declining must not have spent the frontier attempt"
    );
}

#[test]
fn a_chain_that_starts_at_frontier_never_asks_to_approve_spend() {
    let repo = temp_engine_repo("frontierstart");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"frontier\"], attempts_per = 2 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.0 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::NoEdit, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
    assert!(
        report
            .questions
            .iter()
            .all(|q| q.question.kind != QuestionKind::ApproveSpend),
        "questions: {:?}",
        report.questions
    );
}

#[test]
fn attempts_are_attributed_to_the_pool_that_paid_them() {
    let repo = temp_engine_repo("poolattrib");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    opts.pools_path = Some(pools_file(&repo, CLAUDE_POOL));
    let source = fake(Effect::EditFile);
    let report = run_with(&opts, &source).expect("run");

    let attempt = &task(&report, "t1").attempts[0];
    assert_eq!(attempt.pool.as_deref(), Some("claude-max"));
    assert!(
        attempt
            .reviews
            .iter()
            .all(|r| r.pool.as_deref() == Some("claude-max")),
        "the reviewer's own pool is attributed too: {:?}",
        attempt.reviews
    );

    let drain = &report.pool_drain;
    assert_eq!(drain.len(), 1, "{drain:?}");
    assert_eq!(drain[0].pool, "claude-max");
    assert_eq!(drain[0].attempts, 2, "implementer plus its reviewer");
    let ledger = report.render_ledger();
    assert!(ledger.contains("claude-max"), "{ledger}");

    let events = events_of(&repo, &report.run_id);
    let started = events
        .iter()
        .find_map(|event| match &event.body {
            EventBody::AttemptStarted { data, .. } => Some(data),
            _ => None,
        })
        .expect("worker start was emitted");
    assert_eq!(started.adapter.as_deref(), Some("claude-code"));
    assert_eq!(started.preflight_cli_version.as_deref(), Some("0.0.0-fake"));
    assert_eq!(started.effort, Some(Effort::XHigh));
    assert_eq!(
        started.selection_origin,
        Some(events::SelectionOrigin::Auto)
    );

    let review = events
        .iter()
        .find_map(|event| match &event.body {
            EventBody::AttemptFinished { data, .. } => data.reviews.first(),
            _ => None,
        })
        .expect("review pass actually ran");
    assert_eq!(review.adapter.as_deref(), Some("claude-code"));
    assert_eq!(review.preflight_cli_version.as_deref(), Some("0.0.0-fake"));
    assert_eq!(review.effort, Some(Effort::Max));
    let snapshots: Vec<&events::CapacitySnapshot> = events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::CapacitySnapshot { data } => Some(data),
            _ => None,
        })
        .collect();
    assert_eq!(snapshots.len(), 1, "one snapshot per run start (§14)");
    assert_eq!(snapshots[0].pools.len(), 1);
    assert_eq!(
        snapshots[0].pools[0].remaining, "unknown",
        "never optimistic: an unmeasured pool is unknown, not full"
    );
}

#[test]
fn a_pinned_live_attempt_records_its_selection_origin() {
    let repo = temp_engine_repo("pinorigin");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\
                 [[pins]]\ntier = \"small\"\nagent = \"claude-code\"\n\
                 model = \"claude-haiku-4-5\"\neffort = \"max\"\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
    let events = events_of(&repo, &report.run_id);
    let started = events
        .iter()
        .find_map(|event| match &event.body {
            EventBody::AttemptStarted { data, .. } => Some(data),
            _ => None,
        })
        .expect("worker start was emitted");
    assert_eq!(started.selection_origin, Some(events::SelectionOrigin::Pin));
    assert_eq!(started.effort, Some(Effort::Max));
}

#[test]
fn a_rate_limit_marks_its_pool_exhausted_and_a_recovery_retires_the_signal() {
    let repo = temp_engine_repo("poolexhausted");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let pools = pools_file(&repo, CLAUDE_POOL);
    opts.pools_path = Some(pools.clone());
    let source = source(
        vec![Effect::RateLimited, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");

    let events = events_of(&repo, &report.run_id);
    let signals: Vec<&events::PoolExhausted> = events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::PoolExhausted { data, .. } => Some(data),
            _ => None,
        })
        .collect();
    assert_eq!(signals.len(), 1, "{signals:?}");
    assert_eq!(signals[0].pool, "claude-max");
    assert_eq!(signals[0].agent, "claude-code");

    let signal_at = events
        .iter()
        .position(|e| matches!(e.body, EventBody::PoolExhausted { .. }))
        .expect("the signal is in the log");
    let through_signal = events[..=signal_at].to_vec();
    let mut warnings = Vec::new();
    let cfg = config::load(None, &repo, Some(&pools), &mut warnings).expect("pools");
    let at_the_signal = capacity::estimate(&cfg.pools, &capacity::observe(&through_signal));
    assert_eq!(at_the_signal[0].remaining, capacity::Remaining::Exhausted);
    assert_eq!(at_the_signal[0].confidence, capacity::Confidence::Signal);

    let settled = capacity::estimate(&cfg.pools, &capacity::observe(&events));
    assert_ne!(
        settled[0].remaining,
        capacity::Remaining::Exhausted,
        "{}",
        settled[0].describe()
    );
}

#[test]
fn reviewer_rate_limit_retires_recovered_implementer_pool_live() {
    let repo = temp_engine_repo("reviewerlimitretiresworker");
    seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
    let pools = pools_file(
        &repo,
        "[pools.claude-max]\nkind = \"subscription-window\"\nagent = \"claude-code\"\n\
             sources = [\"signals\"]\n\n[pools.copilot-window]\nkind = \"subscription-window\"\n\
             agent = \"copilot\"\nsources = [\"signals\"]\n",
    );
    let mut opts = cross_vendor_opts(&repo);
    opts.pools_path = Some(pools);
    let source = cross_vendor(
        vec![
            Effect::RateLimited,
            Effect::EditFile,
            Effect::RateLimited,
            Effect::EditFile,
        ],
        vec![ReviewBehavior::Pass],
        vec![ReviewBehavior::RateLimited, ReviewBehavior::Pass],
    );
    let report = run_with(&opts, &source).expect("outages eventually recover");
    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");

    let signals: Vec<String> = events_of(&repo, &report.run_id)
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::PoolExhausted { data, .. } => Some(data.pool.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        signals,
        ["claude-max", "copilot-window", "claude-max"],
        "the reviewer outage must not leave the successfully serving worker pool retired forever"
    );
}

#[test]
fn the_budget_flag_is_validated_like_the_config_key() {
    let repo = temp_engine_repo("budgetflag");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    for bad in [0.0, -5.0, f64::NAN] {
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("upstroke.toml"));
        opts.budget_usd = Some(bad);
        let source = fake(Effect::EditFile);
        let err = run_with(&opts, &source).expect_err("a meaningless ceiling must refuse");
        assert!(
            err.to_string().contains("not a spendable ceiling"),
            "--budget {bad}: {err}"
        );
    }
    let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(branch.trim(), "main", "refused before branching");
}

#[test]
fn a_spend_approval_is_not_fed_back_to_the_agent_as_an_instruction() {
    let repo = temp_engine_repo("approvalfeedback");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.005 }\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::NoEdit, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
    let scripted = ScriptedAnswers::new(vec![Answer::Answered {
        text: "approve: run the escalated attempt".to_owned(),
    }]);
    let report = run_harness(
        &opts,
        &Harness {
            adapters: &source,
            answers: Some(&scripted),
            sleeper: None,
        },
    )
    .expect("run");
    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");

    let frontier = &source.adapter.runs()[1].prompt;
    assert!(
        !frontier.contains("approve: run the escalated attempt"),
        "the approval reached the implementer as guidance:
{frontier}"
    );
    assert!(
        !frontier.contains("instruction from a person"),
        "and no human-instruction framing at all:
{frontier}"
    );
}

#[test]
fn picking_an_option_is_an_un_park_and_not_a_decision() {
    let repo = temp_engine_repo("cannedoption");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(
        vec![Effect::AskQuestion, Effect::EditFile],
        vec![ReviewBehavior::Pass],
    );
    let scripted = ScriptedAnswers::new(vec![Answer::Answered {
        text: question_options(QuestionKind::Clarify)[0].clone(),
    }]);
    let report = run_harness(
        &opts,
        &Harness {
            adapters: &source,
            answers: Some(&scripted),
            sleeper: None,
        },
    )
    .expect("run");
    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");

    let runs = source.adapter.runs();
    let retry = &runs
        .iter()
        .filter(|r| !r.prompt.contains("DATA UNDER REVIEW"))
        .nth(1)
        .expect("a second implementer attempt")
        .prompt;
    assert!(
        !retry.contains("answer in your own words"),
        "the option label reached the implementer as guidance:\n{retry}"
    );
    assert!(
        !retry.contains("instruction from a person"),
        "and with no human-instruction framing at all:\n{retry}"
    );
    for review in runs
        .iter()
        .filter(|r| r.prompt.contains("DATA UNDER REVIEW"))
    {
        assert!(
            !review.prompt.contains("answer in your own words"),
            "the option label reached the reviewer as a decision:\n{}",
            review.prompt
        );
    }
}

#[test]
fn one_outage_records_one_signal_however_many_deferrals_it_causes() {
    let repo = temp_engine_repo("onesignal");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    opts.pools_path = Some(pools_file(&repo, CLAUDE_POOL));
    let source = source(
        vec![
            Effect::RateLimited,
            Effect::RateLimited,
            Effect::RateLimited,
            Effect::EditFile,
        ],
        vec![ReviewBehavior::Pass],
    );
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
    let signals = events_of(&repo, &report.run_id)
        .iter()
        .filter(|e| matches!(e.body, EventBody::PoolExhausted { .. }))
        .count();
    assert_eq!(
        signals, 1,
        "one outage is one fact; the deferrals are already on `task_deferred`"
    );
}

#[test]
fn a_budget_stop_hands_back_a_clean_tree() {
    let repo = temp_engine_repo("budgetdirty");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    opts.budget_usd = Some(0.05);
    let rejected = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
    let stopped = run_with(&opts, &rejected).expect("run");
    assert_eq!(stopped.outcome(), RunOutcome::BudgetExceeded, "{stopped:?}");

    let workspace = Workspace::open(&repo).expect("open");
    let left = workspace.uncommitted_summary().expect("status");
    assert!(
        left.is_empty(),
        "a clean stop left the rejected attempt in the operator's tree: {left:?}"
    );

    let mut resume_opts = resume_options(&repo, &stopped.run_id);
    resume_opts.budget_usd = Some(10.0);
    let accepted = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
    let resumed = resume_harness(
        &resume_opts,
        &Harness {
            adapters: &accepted,
            answers: None,
            sleeper: None,
        },
    )
    .expect("resume");
    assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
    assert!(committed(&resumed, "t1"));
    assert!(
        !resumed
            .warnings
            .iter()
            .any(|w| w.contains("discarded") && w.contains("uncommitted")),
        "nothing should have been left for the resume to discard: {:?}",
        resumed.warnings
    );
}

fn priced_and_unpriced_attempts() -> TaskReport {
    let attempt = |cost: Option<f64>| AttemptRecord {
        attempt: 1,
        tier: "frontier".to_owned(),
        model: "m".to_owned(),
        pool: None,
        resumed: false,
        duration: Duration::ZERO,
        cost_usd: cost,
        reviews: Vec::new(),
        session_id: None,
        usage: None,
        failure: None,
    };
    let mut task = task_report_costing(Some(0.2020), None);
    task.status = TaskRunStatus::Committed {
        sha: "abc123".to_owned(),
    };
    task.attempts = vec![attempt(None), attempt(Some(0.2020))];
    task
}

fn empty_report() -> RunReport {
    RunReport {
        run_id: "01RUN".to_owned(),
        branch: "b".to_owned(),
        gates: Vec::new(),
        gates_from_config: false,
        warnings: Vec::new(),
        tasks: Vec::new(),
        halted_at: None,
        questions: Vec::new(),
        budget_stop: None,
        total_cost_usd: 0.0,
        pool_drain: Vec::new(),
        running: false,
        interrupted: false,
    }
}

#[test]
fn an_unpriced_worker_reads_as_unreported_rather_than_free() {
    let mut task = task_report_costing(None, None);
    task.id = "t1".to_owned();
    task.model = "gpt-5.6-sol".to_owned();
    task.status = TaskRunStatus::Committed {
        sha: "abc123".to_owned(),
    };
    task.attempts = vec![AttemptRecord {
        attempt: 1,
        tier: "frontier".to_owned(),
        model: "gpt-5.6-sol".to_owned(),
        pool: None,
        resumed: false,
        duration: Duration::from_secs(46),
        cost_usd: None,
        reviews: Vec::new(),
        session_id: None,
        usage: None,
        failure: None,
    }];
    let report = RunReport {
        tasks: vec![task],
        ..empty_report()
    };

    let rendered = report.render();
    assert!(rendered.contains("gpt-5.6-sol $?"), "{rendered}");
    let task_line = rendered
        .lines()
        .find(|l| l.contains("t1: committed"))
        .expect("the task line");
    assert!(
        !task_line.contains("$0.0000"),
        "unreported spend rendered as free: {task_line}"
    );
    assert!(
        report.total_is_floor(),
        "an unpriced worker makes it a floor"
    );
    assert!(rendered.contains("total: $0.0000?"), "{rendered}");
    let ledger = report.render_ledger();
    assert!(ledger.contains("total $0.0000?"), "{ledger}");
    assert!(ledger.contains("a floor, not a total"), "{ledger}");
    let row = ledger
        .lines()
        .find(|l| l.trim_start().starts_with("t1"))
        .expect("the ledger row");
    assert!(row.contains('—'), "{row}");

    let mut mixed = priced_and_unpriced_attempts();
    mixed.id = "t2".to_owned();
    let row = RunReport {
        tasks: vec![mixed],
        ..empty_report()
    }
    .render_ledger();
    let row = row
        .lines()
        .find(|l| l.trim_start().starts_with("t2"))
        .expect("the ledger row");
    assert!(row.contains("$0.2020?"), "a floor must say so: {row}");

    let mut priced = report;
    priced.tasks[0].cost_usd = Some(0.2020);
    assert!(priced.render().contains("$0.2020"), "{}", priced.render());
}

#[test]
fn a_status_from_a_newer_upstroke_does_not_fail_the_whole_report() {
    let text = r#"{
          "run_id": "01RUN", "branch": "b", "gates": [], "gates_from_config": false,
          "warnings": [], "halted_at": null, "questions": [], "total_cost_usd": 0.0,
          "tasks": [
            {"id": "t1", "title": "One", "model": "m",
             "status": {"status": "teleported", "destination": "elsewhere"},
             "duration": {"secs": 0, "nanos": 0}, "cost_usd": null,
             "review_models": [], "review_cost_usd": null,
             "review_cost_incomplete": false, "session_id": null, "attempts": []},
            {"id": "t2", "title": "Two", "model": "m",
             "status": {"status": "committed", "sha": "abc123"},
             "duration": {"secs": 0, "nanos": 0}, "cost_usd": null,
             "review_models": [], "review_cost_usd": null,
             "review_cost_incomplete": false, "session_id": null, "attempts": []}
          ]
        }"#;

    let report: RunReport =
        serde_json::from_str(text).expect("one unknown status must not sink the report");
    assert!(matches!(task(&report, "t1").status, TaskRunStatus::Unknown));
    assert!(
        matches!(&task(&report, "t2").status, TaskRunStatus::Committed { sha } if sha == "abc123")
    );
    let rendered = report.render();
    assert!(rendered.contains("t1: status not recognised"), "{rendered}");
    assert!(rendered.contains("t2: committed abc123"), "{rendered}");
}

#[test]
fn a_report_for_a_dead_run_never_says_a_task_is_running() {
    let task = Task {
        id: TaskId::from("t1"),
        kind: TaskKind::Implement,
        title: "One".to_owned(),
        body: String::new(),
        depends_on: Vec::new(),
        acceptance: Vec::new(),
        path_hints: Vec::new(),
        suggested_tier: None,
        min_tier: None,
        artifacts_in: Vec::new(),
        artifacts_out: Vec::new(),
    };
    let mid_attempt = Progress {
        in_flight: Some(events::InFlight {
            attempt: 2,
            rung: 1,
            tier: "mid".to_owned(),
            model: "claude-sonnet-5".to_owned(),
            profile: "mid-claude-sonnet-5".to_owned(),
            pool: None,
        }),
        ..Progress::default()
    };

    for state in [TaskState::Pending, TaskState::Deferred] {
        let dead = task_report(&task, &state, &mid_attempt, false);
        assert!(
            matches!(dead.status, TaskRunStatus::Skipped),
            "a report for an ended run claimed a live attempt from {state:?}: {:?}",
            dead.status
        );
        let live = task_report(&task, &state, &mid_attempt, true);
        assert!(
            matches!(live.status, TaskRunStatus::Running { .. }),
            "and a live one still reports it: {:?}",
            live.status
        );
    }
}

#[test]
fn a_budget_stop_keeps_its_outcome_while_a_resume_holds_the_lock() {
    let repo = temp_engine_repo("resumewindow");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    opts.budget_usd = Some(0.05);
    let rejected = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
    let stopped = run_with(&opts, &rejected).expect("run");
    assert_eq!(stopped.outcome(), RunOutcome::BudgetExceeded, "{stopped:?}");

    let paths = paths_of(&repo, &stopped.run_id);
    let _held = RunLock::acquire(&paths.public).expect("the resume claims it");

    let seen = replay_of(&repo, &stopped.run_id);
    assert!(
        !seen.running,
        "a run that recorded its finish is not running"
    );
    assert!(seen.held, "though a resume does hold it");
    let out = crate::status::render(&seen);
    assert!(out.contains("run stopped at its budget"), "{out}");
    assert!(out.contains("upstroke resume"), "{out}");
    assert!(out.contains("another process holds this run"), "{out}");
    assert!(!out.contains("run in progress"), "{out}");
}

#[test]
fn a_budget_stop_survives_a_git_that_cannot_clean_the_tree() {
    let repo = temp_engine_repo("budgetjam");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    opts.budget_usd = Some(0.05);
    let rejected = source(
        vec![Effect::JamCleanupAfterReview],
        vec![ReviewBehavior::Fail],
    );
    let stopped = run_with(&opts, &rejected).expect("the ceiling still ends the run");

    assert_eq!(
        stopped.outcome(),
        RunOutcome::BudgetExceeded,
        "a failed cleanup relabelled the stop: {stopped:?}"
    );
    let stop = stopped
        .budget_stop
        .as_ref()
        .expect("the ceiling is on the record even when the cleanup failed");
    assert_eq!(stop.budget, events::BudgetKind::Run);
    assert!(
        stopped
            .warnings
            .iter()
            .any(|w| w.contains("could not be cleaned")),
        "the dirty tree went unmentioned: {:?}",
        stopped.warnings
    );
}

#[test]
fn a_budget_stop_survives_a_stale_decline_file() {
    let repo = temp_engine_repo("budgetdecline");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- upstroke: id=t2 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [budgets]\nrun_usd = 0.05\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = fake(Effect::EditFile);
    let report = run_with(&opts, &source).expect("run");
    assert_eq!(report.outcome(), RunOutcome::BudgetExceeded, "{report:?}");
    assert!(report.halted_at.is_none(), "nothing failed: {report:?}");
}

#[derive(Debug, Clone)]
struct RoutedProcess {
    role: crate::runner::ExecutionRole,
    program: String,
    invocation: String,
    workspace: PathBuf,
    agent: Option<String>,
    slotted: bool,
    stdin: String,
}

struct RecordingRunner {
    inner: crate::runner::host::HostRunner,
    seen: Mutex<Vec<RoutedProcess>>,
}

impl RecordingRunner {
    fn new() -> Self {
        Self {
            inner: crate::runner::host::HostRunner::new(),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn seen(&self) -> Vec<RoutedProcess> {
        self.seen.lock().expect("recorder").clone()
    }
}

impl crate::runner::Runner for RecordingRunner {
    fn run(
        &self,
        request: &crate::runner::RunnerRequest,
    ) -> Result<ProcessOutput, crate::error::UpstrokeError> {
        self.seen.lock().expect("recorder").push(RoutedProcess {
            role: request.role.clone(),
            program: request.command.program.clone(),
            invocation: request.invocation.render(),
            workspace: request.workspace.clone(),
            agent: request.agent.as_ref().map(|id| id.as_str().to_owned()),
            slotted: request.role.is_slotted(),
            stdin: String::from_utf8_lossy(&request.command.stdin).into_owned(),
        });
        crate::runner::Runner::run(&self.inner, request)
    }
}

fn program_stem(program: &str) -> String {
    Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[test]
fn the_legacy_engine_routes_every_process_through_the_runner() {
    let repo = temp_engine_repo("routed");
    seed(
        &repo,
        "## One\n<!-- upstroke: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- upstroke: id=t2 kind=implement depends= -->\n",
        Some(
            "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [[gates]]\nname = \"first\"\ncmd = \"echo gate-one\"\n\n\
                 [[gates]]\nname = \"second\"\ncmd = \"echo gate-two\"\n",
        ),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
    let runner = RecordingRunner::new();
    let report = run_harness_on(
        &opts,
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
        &runner,
    )
    .expect("run");

    assert!(committed(&report, "t1"), "report: {report:?}");
    assert!(committed(&report, "t2"), "report: {report:?}");
    let seen = runner.seen();

    let expected_ids = vec![
        "k0.g0.a1.worker.o0",
        "k0.g0.a1.gate0.o0",
        "k0.g0.a1.gate1.o0",
        "k0.g0.a1.review_pass0.o0",
        "k1.g0.a1.worker.o0",
        "k1.g0.a1.gate0.o0",
        "k1.g0.a1.gate1.o0",
        "k1.g0.a1.review_pass0.o0",
    ];
    let ids: Vec<&str> = seen.iter().map(|p| p.invocation.as_str()).collect();
    assert_eq!(ids, expected_ids, "recorded: {seen:#?}");
    assert_eq!(
        seen.len(),
        2 * (1 + 2 + 1),
        "two tasks x (worker + two gates + one review)"
    );
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        ids.len(),
        "two processes of one run share an identity"
    );
    assert!(
        ids.iter().all(|id| id.contains(".g0.")),
        "the legacy engine assigns legacy-scoped values: generation 0"
    );

    use crate::runner::ExecutionRole;
    let by_role = |role: &ExecutionRole| seen.iter().filter(|p| &p.role == role).count();
    assert_eq!(by_role(&ExecutionRole::Implement), 2);
    assert_eq!(by_role(&ExecutionRole::Gate), 4);
    assert_eq!(by_role(&ExecutionRole::Review), 2);
    for process in &seen {
        match process.role {
            ExecutionRole::Implement | ExecutionRole::Review => {
                assert!(process.slotted, "{process:?}");
                assert_eq!(process.agent.as_deref(), Some("claude-code"), "{process:?}");
            }
            ExecutionRole::Gate => {
                assert!(!process.slotted, "{process:?}");
                assert_eq!(process.agent, None, "{process:?}");
            }
            ExecutionRole::Probe(_) => panic!("the legacy engine probes nothing: {process:?}"),
        }
    }

    let worker_stdin = &seen
        .iter()
        .find(|p| p.role == ExecutionRole::Implement)
        .expect("a worker")
        .stdin;
    assert!(
        worker_stdin.contains("## One") || worker_stdin.contains("One"),
        "the worker prompt is delivered on stdin: {worker_stdin:?}"
    );
    assert!(
        worker_stdin.contains("Acceptance") || worker_stdin.len() > 200,
        "and it is the materialized prompt, not a token: {} bytes",
        worker_stdin.len()
    );
    let review_stdin = &seen
        .iter()
        .find(|p| p.role == ExecutionRole::Review)
        .expect("a review")
        .stdin;
    assert!(
        review_stdin.contains("READ-ONLY"),
        "the review prompt is delivered on stdin: {review_stdin:?}"
    );
    assert_ne!(
        worker_stdin, review_stdin,
        "a worker and a judge are not sent the same prompt"
    );
    for gate in seen.iter().filter(|p| p.role == ExecutionRole::Gate) {
        assert!(gate.stdin.is_empty(), "a gate reads no stdin: {gate:?}");
    }

    let shell_probes = |seen: &[RoutedProcess]| {
        seen.iter()
            .filter(|p| p.role == ExecutionRole::Probe(crate::runner::ProbeTarget::Shell))
            .count()
    };
    assert_eq!(
        shell_probes(&seen),
        0,
        "the legacy engine ran a shell probe"
    );
    crate::runner::host::run_shell_probe(
        &runner,
        crate::gates::ShellKind::native(),
        repo.clone(),
        crate::runner::InvocationId::probe(crate::runner::ProbeTarget::Shell, 0)
            .expect("the shell probe identity"),
    )
    .expect("the recorded shell runs `exit 0`");
    assert_eq!(
        shell_probes(&runner.seen()),
        1,
        "the recorder cannot see a shell probe, so the zero above proved nothing"
    );

    assert!(
        seen.iter().all(|p| program_stem(&p.program) != "git"),
        "a git process went through the Runner: {seen:#?}"
    );
    assert!(
        !git_in(&repo, &["log", "--oneline", &report.branch]).is_empty(),
        "the run's branch has commits, so authoritative Git did run"
    );

    let worker = seen
        .iter()
        .find(|p| p.role == ExecutionRole::Implement)
        .expect("a worker");
    assert!(
        crate::util::same_path(&worker.workspace, &repo),
        "the worker runs in the repo root: {} is not {}",
        worker.workspace.display(),
        repo.display()
    );
    for process in seen.iter().filter(|p| p.role != ExecutionRole::Implement) {
        assert!(
            !crate::util::same_path(&process.workspace, &repo),
            "a gate or reviewer judged the live worktree: {process:?}"
        );
        assert!(process.workspace.is_absolute(), "{process:?}");
    }
}

#[test]
fn a_retried_attempt_with_two_passes_and_a_reask_assigns_every_identity_from_production() {
    let repo = temp_engine_repo("identities");
    seed(
        &repo,
        FRONTIER_AUTH_PLAN,
        Some(
            "[routing]\n\
             implement = { chain = [\"frontier\"], attempts_per = 2 }\n\n\
             [[routing.overrides]]\n\
             paths = [\"src/auth/**\"]\n\
             second_opinion = \"different-vendor\"\n\n\
             [[gates]]\nname = \"first\"\ncmd = \"echo gate-one\"\n\n\
             [[gates]]\nname = \"second\"\ncmd = \"echo gate-two\"\n",
        ),
    );
    let source = cross_vendor(
        vec![Effect::NoEdit, Effect::EditFile],
        vec![ReviewBehavior::Unparseable, ReviewBehavior::Pass],
        vec![ReviewBehavior::Pass],
    );
    let runner = RecordingRunner::new();
    let report = run_harness_on(
        &cross_vendor_opts(&repo),
        &Harness {
            adapters: &source,
            answers: None,
            sleeper: None,
        },
        &runner,
    )
    .expect("run");
    assert!(committed(&report, "t1"), "report: {report:?}");

    let seen = runner.seen();
    let ids: Vec<&str> = seen.iter().map(|p| p.invocation.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "k0.g0.a1.worker.o0",
            "k0.g0.a2.worker.o0",
            "k0.g0.a2.gate0.o0",
            "k0.g0.a2.gate1.o0",
            "k0.g0.a2.review_pass0.o0",
            "k0.g0.a2.review_reask0.o0",
            "k0.g0.a2.review_pass1.o0",
        ],
        "recorded: {seen:#?}"
    );

    use std::collections::BTreeSet;
    let attempts: BTreeSet<&str> = ids
        .iter()
        .map(|id| id.split('.').nth(2).expect("the attempt field"))
        .collect();
    assert_eq!(
        attempts,
        BTreeSet::from(["a1", "a2"]),
        "two attempt numbers"
    );
    let roles: BTreeSet<&str> = ids
        .iter()
        .map(|id| id.split('.').nth(3).expect("the role field"))
        .collect();
    assert_eq!(
        roles,
        BTreeSet::from([
            "worker",
            "gate0",
            "gate1",
            "review_pass0",
            "review_reask0",
            "review_pass1",
        ]),
        "six distinct role members across the run"
    );
    assert_eq!(
        ids.iter().collect::<BTreeSet<_>>().len(),
        ids.len(),
        "two processes of one run share an identity"
    );
    assert_eq!(
        source.adapter.reviews_run(),
        2,
        "the primary reviewer's verdict and its one re-ask"
    );
    assert_eq!(
        source.copilot().reviews_run(),
        1,
        "the second family answered once, and was not re-asked"
    );
}

#[test]
fn a_worker_that_cannot_be_spawned_returns_an_error_and_settles_nothing() {
    let repo = temp_engine_repo("workerspawn");
    seed(
        &repo,
        "## Implement the widget\n<!-- upstroke: id=t1 kind=implement depends= -->\n",
        Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
    );
    let mut opts = options(&repo);
    opts.config_path = Some(repo.join("upstroke.toml"));
    let source = source(vec![Effect::SpawnError], vec![ReviewBehavior::Pass]);
    let error = run_with(&opts, &source).expect_err(
        "a worker that cannot be spawned is an infrastructure error, not a run that finished",
    );
    let message = error.to_string();
    assert!(
        message.contains("failed to spawn"),
        "the runner's own diagnostic reaches the caller: {message}"
    );
    assert!(
        message.contains("missing-worker-executable"),
        "and it names the program: {message}"
    );

    let run_id = rundir::latest_run(&repo).expect("the run created its directory");
    let log = fs::read_to_string(paths_of(&repo, &run_id).events()).expect("events.jsonl");
    let kinds: Vec<String> = log
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("an event")
                .get("event")
                .and_then(|kind| kind.as_str())
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "attempt_started")
            .count(),
        1,
        "exactly one attempt was dispatched: {kinds:?}"
    );
    for settled in [
        "attempt_finished",
        "task_failed",
        "task_completed",
        "run_finished",
    ] {
        assert_eq!(
            kinds.iter().filter(|kind| *kind == settled).count(),
            0,
            "a spawn failure synthesized `{settled}`: {kinds:?}"
        );
    }
    assert_eq!(source.adapter.runs().len(), 1, "the ladder bought no retry");
}

#[test]
fn the_engine_facade_exposes_exactly_the_items_the_packet_enumerates() {
    use std::collections::BTreeSet;

    let raw = include_str!("mod.rs");
    let blanked = crate::effects::production_code(raw);
    let source: &str = &blanked;
    assert!(
        source.len() * 2 > raw.len(),
        "the blanked region of the engine facade is {} of {} bytes, so a census over it says \
         little about the file",
        source.len(),
        raw.len()
    );

    let public_fns: BTreeSet<&str> = source
        .lines()
        .filter_map(|line| line.strip_prefix("pub fn "))
        .filter_map(|rest| rest.split(['(', '<', ' ']).next())
        .collect();
    assert_eq!(
        public_fns,
        BTreeSet::from([
            "run",
            "run_with",
            "run_harness",
            "resume",
            "resume_with",
            "resume_harness",
        ]),
        "the engine facade's public functions moved away from the packet's list"
    );

    for widening in [
        "pub(crate) fn",
        "pub(crate) use",
        "pub struct",
        "pub enum",
        "pub const",
        "pub mod ",
    ] {
        assert!(
            !source.contains(widening),
            "`{widening}` appeared in the engine facade, which the visibility rule forbids"
        );
    }

    let mut reexported: BTreeSet<&str> = BTreeSet::new();
    let mut rest = source;
    while let Some(start) = rest.find("pub use ") {
        rest = &rest[start + "pub use ".len()..];
        let end = rest.find(';').expect("a `pub use` ends in a semicolon");
        let statement = &rest[..end];
        rest = &rest[end..];
        match (statement.find('{'), statement.find('}')) {
            (Some(open), Some(close)) => {
                for name in statement[open + 1..close].split(',') {
                    let name = name.trim();
                    if !name.is_empty() {
                        reexported.insert(name);
                    }
                }
            }
            _ => {
                reexported.insert(
                    statement
                        .rsplit("::")
                        .next()
                        .expect("a path")
                        .trim()
                        .trim_end_matches(';'),
                );
            }
        }
    }
    assert_eq!(
        reexported,
        BTreeSet::from([
            "RunOptions",
            "ResumeOptions",
            "Harness",
            "DEFAULT_ATTEMPT_TIMEOUT",
            "DEFAULT_MAX_DEFERS",
            "RunReport",
            "TaskReport",
            "TaskRunStatus",
            "RunOutcome",
            "PoolDrainRow",
            "topo_order",
            "AdapterSource",
            "BuiltinAdapters",
            "AttemptRecord",
            "FailureRecord",
            "AttemptFailure",
            "FailureKind",
            "FailureOrigin",
        ]),
        "the engine facade's re-exports moved away from the packet's list"
    );
    assert_eq!(reexported.len(), 18, "five groups, eighteen names");

    for private in ["fn run_harness_on(", "fn resume_harness_on("] {
        assert!(source.contains(private), "`{private}` is gone");
    }
    assert!(
        !source.contains("pub fn run_harness_on") && !source.contains("pub fn resume_harness_on"),
        "an explicit-Runner entry point is public again"
    );
}

fn public_facade_entry_points() -> Vec<&'static str> {
    let source = include_str!("mod.rs");
    let mut names: Vec<&str> = source
        .lines()
        .filter_map(|line| line.strip_prefix("pub fn "))
        .filter_map(|rest| rest.split(['(', '<', ' ']).next())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[test]
fn every_public_write_coordinator_entry_point_establishes_containment() {
    let repo = temp_engine_repo("containment-facade");
    let mut run_opts = options(&repo);
    run_opts.plan_path = repo.join("absent-plan.md");
    let mut resume_opts = ResumeOptions::new("01ABSENTRUN".to_owned(), repo.clone());
    resume_opts.pools_path = Some(no_pools());
    resume_opts.private_root = Some(private_root_for(&repo));
    let adapters = BuiltinAdapters;

    type Call<'a> = Box<dyn Fn() -> Result<RunReport, UpstrokeError> + 'a>;
    let entry_points: Vec<(&str, Call<'_>)> = vec![
        ("run", Box::new(|| run(&run_opts))),
        ("run_with", Box::new(|| run_with(&run_opts, &adapters))),
        (
            "run_harness",
            Box::new(|| run_harness(&run_opts, &Harness::new(&adapters))),
        ),
        ("resume", Box::new(|| resume(&resume_opts))),
        (
            "resume_with",
            Box::new(|| resume_with(&resume_opts, &adapters)),
        ),
        (
            "resume_harness",
            Box::new(|| resume_harness(&resume_opts, &Harness::new(&adapters))),
        ),
    ];

    let mut driven: Vec<&str> = entry_points.iter().map(|(name, _)| *name).collect();
    driven.sort_unstable();
    assert_eq!(
        driven,
        public_facade_entry_points(),
        "a public engine entry point is not driven here; every one of them makes its caller a \
         write coordinator"
    );
    assert_eq!(driven.len(), 6, "six entry points, and this is the count");

    for (name, call) in &entry_points {
        let before = crate::runner::host::containment_establishments();
        let outcome = call();
        assert!(
            outcome.is_err(),
            "{name}: the fixture relies on this refusing on its own input"
        );
        assert_eq!(
            crate::runner::host::containment_establishments(),
            before + 1,
            "`engine::{name}` entered the write coordinator without establishing containment \
             (INV-18); a kill after CreateProcessW and before private-job assignment would leave \
             a suspended stub alive"
        );
        #[cfg(windows)]
        assert!(
            crate::agent::proc::ambient_job_established(),
            "`engine::{name}` returned without this process joining its ambient Job Object"
        );
    }
}

#[test]
fn no_read_only_public_entry_point_establishes_containment() {
    let repo = temp_engine_repo("containment-readonly");
    let scratch = private_root_for(&repo);
    fs::create_dir_all(&scratch).expect("scratch");
    let absent = repo.join("absent-plan.md");

    type Call<'a> = Box<dyn Fn() + 'a>;
    let read_only: Vec<(&str, Call<'_>)> = vec![
        (
            "validate::run",
            Box::new(|| {
                let _ = crate::validate::run(&crate::validate::ValidateOptions {
                    plan_path: absent.clone(),
                    config_path: None,
                    config_root: repo.clone(),
                    pools_path: Some(no_pools()),
                    engine_limits: config::EngineLimits::Fresh,
                });
            }),
        ),
        (
            "status::load",
            Box::new(|| {
                let _ = crate::status::load(&repo, None);
            }),
        ),
        (
            "export::load",
            Box::new(|| {
                let _ = crate::export::load(&repo, "01ABSENTRUN");
            }),
        ),
        (
            "answer::answer",
            Box::new(|| {
                let _ = crate::answer::answer(&repo, "q1", crate::answer::Reply::Decline);
            }),
        ),
        (
            "capacity::report",
            Box::new(|| {
                let _ = capacity::report(
                    &capacity::CapacityOptions {
                        config_path: Some(absent.clone()),
                        pools_path: Some(no_pools()),
                        repo_root: repo.clone(),
                    },
                    &BuiltinAdapters,
                );
            }),
        ),
        (
            "connect::run_with",
            Box::new(|| {
                let _ = crate::connect::run_with(
                    &crate::connect::ConnectOptions {
                        pools_path: Some(scratch.join("pools.toml")),
                        force: true,
                    },
                    &BuiltinAdapters,
                    std::iter::empty(),
                );
            }),
        ),
    ];
    assert_eq!(
        read_only.len(),
        6,
        "one library entry point per read-only subcommand — the same six \
         `src/main.rs` counts on the dispatch side"
    );

    for (name, call) in &read_only {
        let before = crate::runner::host::containment_establishments();
        call();
        assert_eq!(
            crate::runner::host::containment_establishments(),
            before,
            "`{name}` is not a write coordinator and established containment anyway"
        );
    }
}

#[test]
fn a_facade_run_refuses_before_any_effect_when_containment_fails() {
    let repo = temp_engine_repo("containment-order");
    let mut opts = options(&repo);
    opts.plan_path = repo.join("absent-plan.md");
    let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
    let harness = Harness::new(&source);
    let runner = RecordingRunner::new();

    let refused = run_contained(&opts, &harness, &runner, || {
        Err(UpstrokeError::Refused {
            message: "the ambient Job Object could not be established (simulated failure)"
                .to_owned(),
        })
    })
    .expect_err("a run whose ambient job cannot be established must refuse");
    let refused = refused.to_string();
    assert!(
        refused.contains("ambient Job Object"),
        "the refusal must diagnose the ambient job: {refused}"
    );
    assert!(
        !refused.contains("absent-plan"),
        "the coordinator ran before containment: {refused}"
    );
    assert!(
        runner.seen().is_empty(),
        "a run refused at startup spawned a process: {:?}",
        runner.seen()
    );

    let reached = run_contained(&opts, &harness, &runner, || {
        crate::runner::host::contain_write_command(&mut crate::agent::proc::NoHooks)
    })
    .expect_err("the coordinator then fails on its own, on the plan");
    let reached = reached.to_string();
    assert!(
        reached.contains("absent-plan"),
        "with containment established the coordinator must run: {reached}"
    );
    assert!(
        !reached.contains("ambient Job Object"),
        "a successful establishment must not be reported as a refusal: {reached}"
    );
}

#[test]
fn a_facade_resume_refuses_before_any_effect_when_containment_fails() {
    let repo = temp_engine_repo("containment-order-resume");
    let mut opts = ResumeOptions::new("01ABSENTRUN".to_owned(), repo.clone());
    opts.pools_path = Some(no_pools());
    opts.private_root = Some(private_root_for(&repo));
    let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
    let harness = Harness::new(&source);
    let runner = RecordingRunner::new();

    let refused = resume_contained(&opts, &harness, &runner, || {
        Err(UpstrokeError::Refused {
            message: "the ambient Job Object could not be established (simulated failure)"
                .to_owned(),
        })
    })
    .expect_err("a resume whose ambient job cannot be established must refuse");
    let looked_in = repo.display().to_string();
    let refused = refused.to_string();
    assert!(
        refused.contains("ambient Job Object"),
        "the refusal must diagnose the ambient job: {refused}"
    );
    assert!(
        !refused.contains(&looked_in),
        "the coordinator ran before containment: {refused}"
    );
    assert!(
        runner.seen().is_empty(),
        "a resume refused at startup spawned a process: {:?}",
        runner.seen()
    );

    let reached = resume_contained(&opts, &harness, &runner, || {
        crate::runner::host::contain_write_command(&mut crate::agent::proc::NoHooks)
    })
    .expect_err("the coordinator then fails on its own, on the run it cannot find");
    let reached = reached.to_string();
    assert!(
        reached.contains(&looked_in),
        "with containment established the coordinator must run: {reached}"
    );
    assert!(
        !reached.contains("ambient Job Object"),
        "a successful establishment must not be reported as a refusal: {reached}"
    );
}
