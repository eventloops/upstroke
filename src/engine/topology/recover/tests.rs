//! Extended notes: `docs/internals/engine/topology/recover/tests.md`

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::*;
use crate::agent::{AdapterSource, AgentAdapter, Caps, ProcessOutput, TaskRun};
use crate::config::RunnerSelection;
use crate::events::log::{BarrierStep, EventLog, TopologyLine};
use crate::events::{AttemptRecord, BindingSummary, BudgetKind, ChainSummary, GateSummary};
use crate::gates::ShellKind;
use crate::ir::Outcome;
use crate::ir::{
    Artifact, ArtifactId, Effort, Plan, PlanSource, ResolvedEffortPolicy, Task, TaskId, TaskKind,
    Tier,
};
use crate::review::{PassBinding, ReviewPlan};
use crate::rundir::{
    self, CommitRecord, CreatingMarker, NoHooks, OwnerRecord, RepoKey, RetainReason,
};
use crate::runner::container::resolve::RunnerPreflight;
use crate::runner::container::runtime::{ContainerRuntime, ContainerTrace};
use crate::runner::container::{DisposableDirView, FakeOwnerLiveness, FakeRuntime};
use crate::runner::policy::runner_policy_sha256;
use crate::runner::{CommandSpec, Runner, RunnerRequest};
use crate::topology::effects::EventSite;
use crate::topology::effects::{
    EffectSiteId, HookHarness, HookPhase, Injection, InjectionMode, LockSite, ObjectSite, RefSite,
    RunDirSite, SubEffectPoint, WorktreeSite,
};
use crate::topology::events::{
    AttemptFinished4, AttemptSettlement, AttemptStarted4, BudgetExceeded4, CommitSha, Epoch,
    GitRef, ImageIdentity, IncarnationId, LeaseGrant, RunFinished4, RunStarted4, RungBinding,
    RunnerContract, RunnerKind, RunnerPolicy, SessionId, SettlementTransition, TaskDispatched,
    TopologyEvent, TopologyLimits,
};
use crate::topology::fold::{FrozenInputs, TaskState};
use crate::topology::paths::{GitPath, PathSet};
use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion};
use crate::topology::registry::{TaskKey, TaskRegistry};
use crate::topology::schema::TOPOLOGY_SCHEMA;

use crate::workspace_manager::Refusal;

use crate::engine::topology::identity::{InvocationLedger, ReservationKind, Reservations};
use crate::engine::topology::preflight::RunPreflight;
use crate::engine::topology::seams::{HarnessTopologyHooks, TimeSource, TopologyHooks};
use crate::engine::topology::startup::{FailedStep, RunDirOutcome};

const RUN_ID: &str = "01KZTPR7E00000000000000001";
const CREATOR: &str = "01KZTAAAAAAAAAAAAAAAAAAAAA";
const RESUMER: &str = "01KZTBBBBBBBBBBBBBBBBBBBBB";
const TS: &str = "2026-08-23T09:41:02Z";
const IMAGE_REF: &str = "ghcr.io/example/upstroke-runner:1.4";
const IMAGE_ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const VOLUME: &str = "upstroke-creds-claude";
const AGENT: &str = "claude-code";
const CREATOR_PID: u32 = 4242;

#[derive(Debug, Clone, Copy)]
struct Frozen;

impl TimeSource for Frozen {
    fn now_rfc3339(&self) -> String {
        TS.to_owned()
    }
}

fn fixture_root(tag: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "upstroke-pr7e-{}-{tag}-{ordinal}",
        std::process::id()
    ))
}

fn mkdir(path: &Path) {
    rundir::create_public_dir(path, &mut NoHooks).expect("the run-directory funnel creates a dir");
}

struct Fixture {
    root: PathBuf,
    base_sha: CommitSha,
    repo_root: PathBuf,
    git_dir: PathBuf,
    private_root: PathBuf,
    repo_key: RepoKey,
    started: RunStarted4,
    first_line: Vec<u8>,
    plan: Plan,
}

#[derive(Default)]
struct Damage {
    no_private_half: bool,
    no_owner_record: bool,
    owner: Option<fn(&mut OwnerRecord)>,
    commit: Option<fn(&mut CommitRecord)>,
    locator: Option<String>,
    host_runner: bool,
    extra: Vec<TopologyEventBody>,
    open_generation: bool,
    two_tasks: bool,
    two_tier: bool,
    deep_ladder: bool,
}

impl Fixture {
    fn manager(&self) -> crate::workspace_manager::WorkspaceManager {
        crate::workspace_manager::WorkspaceManager::derive(
            &self.repo_root,
            &self.private_root,
            &self.started.run_id,
            &self.started.incarnation.0,
        )
        .expect("the fixture's repository and private root are real directories")
    }

    fn build(tag: &str, damage: Damage) -> Self {
        let root = fixture_root(tag);
        let repo_root = root.join("repo");
        let git_dir = repo_root.join(".git");
        let private_root = root.join("private");
        mkdir(&repo_root);
        crate::workspace_manager::fixture::git(&repo_root, &["init", "-q", "-b", "main"]);
        for setting in [
            ["config", "user.email", "tests@upstroke.local"],
            ["config", "user.name", "upstroke tests"],
            ["config", "core.logAllRefUpdates", "true"],
        ] {
            crate::workspace_manager::fixture::git(&repo_root, &setting);
        }
        crate::workspace_manager::fixture::write_file(&repo_root.join("seed.txt"), b"seed\n");
        crate::workspace_manager::fixture::git(&repo_root, &["add", "-A"]);
        crate::workspace_manager::fixture::git(&repo_root, &["commit", "-q", "-m", "seed"]);
        let base_sha = CommitSha(crate::workspace_manager::fixture::git(
            &repo_root,
            &["rev-parse", "HEAD"],
        ));
        mkdir(&private_root);
        let repo_key = RepoKey::v1(&std::fs::canonicalize(&git_dir).expect("the git dir exists"));

        let public = rundir::public_dir(&repo_root, RUN_ID);
        mkdir(&public);
        let private_dir = private_root.join("runs").join(RUN_ID);
        if !damage.no_private_half {
            mkdir(&private_dir);
        }

        let plan = plan_with(damage.two_tasks);
        let recorded_locator = damage
            .locator
            .clone()
            .unwrap_or_else(|| private_dir.display().to_string());
        let runner = if damage.host_runner {
            host_runner()
        } else {
            container_runner()
        };
        let started = run_started(
            &plan,
            &recorded_locator,
            runner,
            &base_sha,
            damage.two_tier,
            damage.deep_ladder,
            damage.two_tasks,
        );

        let marker = CreatingMarker {
            run_id: RUN_ID.to_owned(),
            repo_key: repo_key.as_str().to_owned(),
            private_dir: recorded_locator.clone(),
            incarnation: CREATOR.to_owned(),
            pid: CREATOR_PID,
            runner_policy_sha256: runner_policy_sha256(&started.runner),
        };
        rundir::stage_marker(&public, &marker, &mut NoHooks).expect("P1a stages the marker");
        rundir::publish_marker(&public, &mut NoHooks).expect("P1b publishes it");

        let mut warnings = Vec::new();
        let mut log = EventLog::open(
            EventSite::OpenLog,
            &public.join(rundir::EVENT_LOG),
            &mut warnings,
        )
        .expect("the Event funnel opens a fresh log");
        let (line, _) = TopologyLine::round_trip(&event(TopologyEventBody::RunStarted {
            data: Box::new(started.clone()),
        }))
        .expect("run_started survives its own wire format");
        log.append_topology(EventSite::AppendFirst, &line)
            .expect("the commitment boundary");
        let first_line = line.committed_bytes()[..line.committed_bytes().len() - 1].to_vec();
        let mut later: Vec<TopologyEventBody> = Vec::new();
        if damage.open_generation {
            later.push(dispatched_at(&base_sha));
        }
        later.extend(damage.extra.iter().cloned());
        for body in &later {
            let site = crate::events::log::site_for(body);
            let (line, _) =
                TopologyLine::round_trip(&event(body.clone())).expect("a valid later event");
            log.append_topology(site, &line).expect("a later append");
        }
        drop(log);

        if !damage.no_private_half {
            if !damage.no_owner_record {
                let mut owner = OwnerRecord {
                    run_id: RUN_ID.to_owned(),
                    repo_key: repo_key.as_str().to_owned(),
                    public_dir: canonical(&public),
                    incarnation: CREATOR.to_owned(),
                    runner: started.runner.clone(),
                };
                if let Some(damage) = damage.owner {
                    damage(&mut owner);
                }
                rundir::stage_owner_record(&private_dir, &owner, &mut NoHooks)
                    .expect("P3a stages the owner record");
                rundir::publish_owner_record(&private_dir, &mut NoHooks).expect("P3b publishes it");
            }
            let mut commit = CommitRecord {
                run_id: RUN_ID.to_owned(),
                repo_key: repo_key.as_str().to_owned(),
                public_dir: canonical(&public),
                incarnation: CREATOR.to_owned(),
                run_started_sha256: rundir::run_started_sha256(&first_line),
            };
            if let Some(damage) = damage.commit {
                damage(&mut commit);
            }
            rundir::stage_commit_record(&private_dir, &commit, &mut NoHooks)
                .expect("P5a stages the commit record");
            rundir::publish_commit_record(&private_dir, &mut NoHooks).expect("P5b publishes it");
        }

        Self {
            root,
            base_sha,
            repo_root,
            git_dir,
            private_root,
            repo_key,
            started,
            first_line,
            plan,
        }
    }

    fn healthy(tag: &str) -> Self {
        Self::build(tag, Damage::default())
    }

    fn public(&self) -> PathBuf {
        rundir::public_dir(&self.repo_root, RUN_ID)
    }

    fn log(&self) -> PathBuf {
        self.public().join(rundir::EVENT_LOG)
    }

    fn log_bytes(&self) -> Vec<u8> {
        crate::util::read_file_bounded(&self.log()).unwrap_or_default()
    }

    fn inputs(&self) -> FrozenInputs {
        FrozenInputs {
            plan: self.plan.clone(),
            normalized_plan_digest: self.started.normalized_plan_digest.clone(),
        }
    }

    fn worktree_lock_file(&self) -> PathBuf {
        self.git_dir.join("upstroke-worktree.lock")
    }

    fn derive(&self, explicit: Option<&Path>) -> Result<RootDerived, UpstrokeError> {
        RootDerived::derive_with(&self.repo_root, RUN_ID, explicit, TOPOLOGY_SCHEMA)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = rundir::remove_public_husk(&self.root, &mut NoHooks);
    }
}

struct PlantedHusk {
    public: PathBuf,
    private: PathBuf,
}

fn plant_husk(fixture: &Fixture, run_id: &str, committed: bool) -> PlantedHusk {
    let public = rundir::public_dir(&fixture.repo_root, run_id);
    let private = fixture.private_root.join("runs").join(run_id);
    mkdir(&public);
    mkdir(&private);

    let runner = container_runner();
    let marker = CreatingMarker {
        run_id: run_id.to_owned(),
        repo_key: fixture.repo_key.as_str().to_owned(),
        private_dir: private.display().to_string(),
        incarnation: CREATOR.to_owned(),
        pid: CREATOR_PID,
        runner_policy_sha256: runner_policy_sha256(&runner),
    };
    rundir::stage_marker(&public, &marker, &mut NoHooks).expect("P1a stages the husk's marker");
    rundir::publish_marker(&public, &mut NoHooks).expect("P1b publishes it");

    let owner = OwnerRecord {
        run_id: run_id.to_owned(),
        repo_key: fixture.repo_key.as_str().to_owned(),
        public_dir: canonical(&public),
        incarnation: CREATOR.to_owned(),
        runner,
    };
    rundir::stage_owner_record(&private, &owner, &mut NoHooks)
        .expect("P3a stages the owner record");
    rundir::publish_owner_record(&private, &mut NoHooks).expect("P3b publishes it");

    if committed {
        let record = CommitRecord {
            run_id: run_id.to_owned(),
            repo_key: fixture.repo_key.as_str().to_owned(),
            public_dir: canonical(&public),
            incarnation: CREATOR.to_owned(),
            run_started_sha256: rundir::run_started_sha256(b"a first line of its own"),
        };
        rundir::stage_commit_record(&private, &record, &mut NoHooks)
            .expect("P5a stages the commit record");
        rundir::publish_commit_record(&private, &mut NoHooks).expect("P5b publishes it");
    }

    PlantedHusk { public, private }
}

fn tree_bytes(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.insert(
                relative,
                crate::util::read_file_bounded(&path).unwrap_or_default(),
            );
        }
    }
    out
}

fn event_kinds(log: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(log)
        .lines()
        .filter_map(|line| {
            let at = line.find("\"event\":\"")? + "\"event\":\"".len();
            let rest = line.get(at..)?;
            let end = rest.find('"')?;
            rest.get(..end).map(std::borrow::ToOwned::to_owned)
        })
        .collect()
}

fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn event(body: TopologyEventBody) -> TopologyEvent {
    TopologyEvent {
        ts: TS.to_owned(),
        body,
    }
}

fn plan_with(two_tasks: bool) -> Plan {
    let mut plan = plan();
    if two_tasks {
        let alpha = plan.tasks[0].clone();
        plan.tasks.push(Task {
            id: TaskId::from("beta"),
            title: "beta".to_owned(),
            body: "beta body".to_owned(),
            acceptance: vec!["beta passes".to_owned()],
            path_hints: vec!["src/beta/*.rs".to_owned()],
            artifacts_out: vec![ArtifactId::from("beta-out")],
            ..alpha
        });
        plan.artifacts.push(Artifact {
            id: ArtifactId::from("beta-out"),
            produced_by: Some(TaskId::from("beta")),
        });
    }
    plan
}

fn plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "frozen-plan-hash".to_owned(),
        },
        tasks: vec![Task {
            id: TaskId::from("alpha"),
            kind: TaskKind::Refactor,
            title: "alpha".to_owned(),
            body: "alpha body".to_owned(),
            depends_on: Vec::new(),
            acceptance: vec!["alpha passes".to_owned()],
            path_hints: vec!["src/alpha/*.rs".to_owned()],
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: vec![ArtifactId::from("alpha-out")],
        }],
        artifacts: vec![Artifact {
            id: ArtifactId::from("alpha-out"),
            produced_by: Some(TaskId::from("alpha")),
        }],
    }
}

fn container_runner() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Container,
        policy: RunnerContract::ContainerV1,
        image: Some(ImageIdentity {
            reference: IMAGE_REF.to_owned(),
            id: IMAGE_ID.to_owned(),
            digest: Some("sha256:2222".to_owned()),
        }),
        credential_volumes: Some(
            [(AGENT.to_owned(), VOLUME.to_owned())]
                .into_iter()
                .collect(),
        ),
    }
}

fn host_runner() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Host,
        policy: RunnerContract::HostV1,
        image: None,
        credential_volumes: None,
    }
}

fn chain() -> ChainSummary {
    ChainSummary {
        task: "alpha".to_owned(),
        tiers: vec![Tier::Mid],
        attempts_per: 2,
        bindings: Some(vec![BindingSummary {
            tier: Tier::Mid,
            agent: AGENT.to_owned(),
            model: "claude-opus-5".to_owned(),
            pinned: false,
        }]),
    }
}

fn escalating_chain() -> ChainSummary {
    ChainSummary {
        task: "alpha".to_owned(),
        tiers: vec![Tier::Mid, Tier::Frontier],
        attempts_per: 1,
        bindings: Some(vec![
            BindingSummary {
                tier: Tier::Mid,
                agent: AGENT.to_owned(),
                model: "claude-opus-5".to_owned(),
                pinned: false,
            },
            BindingSummary {
                tier: Tier::Frontier,
                agent: AGENT.to_owned(),
                model: "claude-fable-5".to_owned(),
                pinned: false,
            },
        ]),
    }
}

fn deep_chain() -> ChainSummary {
    ChainSummary {
        attempts_per: 2,
        ..escalating_chain()
    }
}

fn review_plan() -> ReviewPlan {
    ReviewPlan {
        enabled: Some(true),
        alternative_available: Some(false),
        pass_timeout_secs: Some(600),
        primary: Some(PassBinding::new(AGENT, "claude-opus-5")),
        alternative: None,
        second_opinion: vec![None],
    }
}

fn run_started(
    plan: &Plan,
    private_dir: &str,
    runner: RunnerPolicy,
    base: &CommitSha,
    two_tier: bool,
    deep_ladder: bool,
    two_tasks: bool,
) -> RunStarted4 {
    let unauthenticated = RunStarted4 {
        schema: TOPOLOGY_SCHEMA,
        upstroke_version: env!("CARGO_PKG_VERSION").to_owned(),
        run_id: RUN_ID.to_owned(),
        incarnation: IncarnationId(CREATOR.to_owned()),
        runner,
        probed_agents: vec![AGENT.to_owned()],
        branch: "upstroke/run".to_owned(),
        integration_ref: GitRef(format!("refs/upstroke/runs/{RUN_ID}/integration")),
        base_sha: base.clone(),
        execution_root: "/does/not/matter".to_owned(),
        private_dir: private_dir.to_owned(),
        plan_path: "PLAN.md".to_owned(),
        config_path: Some("upstroke.toml".to_owned()),
        plan_hash: "frozen-plan-hash".to_owned(),
        normalized_plan_digest: "sha256:aaaa".to_owned(),
        registry_digest: String::new(),
        path_policy: PathPolicy {
            version: PathPolicyVersion::V2,
            case_fold: false,
            grammar: PathGrammar::Globset,
        },
        limits: TopologyLimits {
            max_parallel: 1,
            max_defers: 3,
            max_merge_repairs: 1,
        },
        gates: vec!["clippy".to_owned()],
        gates_from_config: true,
        gate_cmds: vec![GateSummary {
            name: "clippy".to_owned(),
            cmd: "cargo clippy".to_owned(),
            timeout: Duration::from_secs(600),
            shell: ShellKind::Bash,
        }],
        interaction_mode: "attached".to_owned(),
        chains: {
            let first = if deep_ladder {
                deep_chain()
            } else if two_tier {
                escalating_chain()
            } else {
                chain()
            };
            let mut chains = vec![first.clone()];
            if two_tasks {
                chains.push(ChainSummary {
                    task: "beta".to_owned(),
                    ..first
                });
            }
            chains
        },
        effort_policy: ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::High,
            frontier: Effort::Max,
            review: Effort::Medium,
        },
        reviews: {
            let mut reviews = review_plan();
            if two_tasks {
                reviews.second_opinion.push(None);
            }
            reviews
        },
    };
    let registry_digest = TaskRegistry::originals_with_agents(
        plan,
        &unauthenticated.registry_record(),
        &unauthenticated.probed_agents,
    )
    .expect("the fixture record derives a registry")
    .digest();
    RunStarted4 {
        registry_digest,
        ..unauthenticated
    }
}

fn runtime_holding_the_record() -> FakeRuntime {
    let runtime = FakeRuntime::new(ContainerTrace::default());
    runtime.add_image(IMAGE_ID, Some("sha256:2222"));
    runtime.tag(IMAGE_REF, IMAGE_ID);
    runtime.add_volume(VOLUME);
    runtime
}

#[derive(Debug, Default)]
struct RecordingRunner {
    seen: Mutex<Vec<RunnerRequest>>,
    failing: Mutex<Option<String>>,
    filters: Mutex<bool>,
    edits: Mutex<bool>,
}

impl RecordingRunner {
    fn filtering() -> Self {
        let runner = Self::editing();
        *runner
            .filters
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
        runner
    }

    fn editing() -> Self {
        let runner = Self::default();
        *runner.edits.lock().unwrap_or_else(PoisonError::into_inner) = true;
        runner
    }

    fn failing(program: &str) -> Self {
        let runner = Self::default();
        *runner
            .failing
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(program.to_owned());
        runner
    }

    fn requests(&self) -> Vec<RunnerRequest> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Runner for RecordingRunner {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.clone());
        let failing = self
            .failing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let code = if failing.as_deref() == Some(request.command.program.as_str()) {
            127
        } else {
            0
        };
        if code == 0
            && request.role == crate::runner::ExecutionRole::Implement
            && *self.edits.lock().unwrap_or_else(PoisonError::into_inner)
        {
            crate::workspace_manager::fixture::write_file(
                &request.workspace.join("worker.txt"),
                b"the worker's edit\n",
            );
            if *self.filters.lock().unwrap_or_else(PoisonError::into_inner) {
                crate::workspace_manager::fixture::write_file(
                    &request.workspace.join(".gitattributes"),
                    b"* filter=upstroke-test\n",
                );
            }
        }
        Ok(ProcessOutput {
            code: Some(code),
            stdout: "1.2.3".to_owned(),
            stderr: String::new(),
            duration: Duration::from_millis(1),
            timed_out: false,
            output_limited: false,
        })
    }
}

#[derive(Debug)]
struct StubAdapter;

impl AgentAdapter for StubAdapter {
    fn id(&self) -> &'static str {
        AGENT
    }

    fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {
        let request = crate::agent::probe_request(
            AGENT,
            CommandSpec::new("claude").arg("--version"),
            0,
            Duration::from_secs(30),
        )?;
        let output = runner.run(&request)?;
        if output.code != Some(0) {
            return Err(UpstrokeError::Agent {
                message: format!("`claude --version` exited {:?}", output.code),
            });
        }
        Ok(Caps {
            version: output.stdout.trim().to_owned(),
            json_output: true,
            session_resume: true,
            cost_reporting: true,
            read_only_mode: true,
            acp: false,
            model_list: true,
        })
    }

    fn build(&self, _run: &TaskRun) -> Result<CommandSpec, UpstrokeError> {
        Ok(CommandSpec::new("claude"))
    }

    fn parse(&self, _out: &ProcessOutput) -> Result<Outcome, UpstrokeError> {
        Err(UpstrokeError::Agent {
            message: "the fixture adapter runs no attempt".to_owned(),
        })
    }
}

struct StubAdapters;

impl AdapterSource for StubAdapters {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        (id == AGENT).then_some(&StubAdapter as &dyn AgentAdapter)
    }
}

struct AlwaysCertifies;

impl RunnerPreflight for AlwaysCertifies {
    fn certify(&self, _policy: &RunnerPolicy) -> Result<(), UpstrokeError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefShape {
    Direct,
    Symbolic,
    CheckedOut,
}

struct RecordingRefs {
    log: PathBuf,
    shape: RefShape,
    at: Mutex<Option<String>>,
    created: Mutex<Vec<(String, String)>>,
    targets_read: Mutex<usize>,
    entered: Mutex<Vec<Vec<u8>>>,
}

impl RecordingRefs {
    fn with_log(log: &Path, shape: RefShape, at: Option<String>) -> Self {
        Self {
            log: log.to_path_buf(),
            shape,
            at: Mutex::new(at),
            created: Mutex::new(Vec::new()),
            targets_read: Mutex::new(0),
            entered: Mutex::new(Vec::new()),
        }
    }

    fn absent(fixture: &Fixture) -> Self {
        Self::with_log(&fixture.log(), RefShape::Direct, None)
    }

    fn at(fixture: &Fixture, sha: &str) -> Self {
        Self::with_log(&fixture.log(), RefShape::Direct, Some(sha.to_owned()))
    }

    fn shaped(fixture: &Fixture, shape: RefShape) -> Self {
        Self::with_log(&fixture.log(), shape, None)
    }

    fn created(&self) -> Vec<(String, String)> {
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn target(&self) -> Option<String> {
        self.at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn targets_read(&self) -> usize {
        *self
            .targets_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn log_bytes_at_entries(&self) -> Vec<Vec<u8>> {
        self.entered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn log_kinds_at_entries(&self) -> Vec<Vec<String>> {
        self.log_bytes_at_entries()
            .iter()
            .map(|bytes| event_kinds(bytes))
            .collect()
    }
}

impl IntegrationRefs for RecordingRefs {
    fn assert_publishable(&self, refname: &str) -> Result<(), UpstrokeError> {
        match self.shape {
            RefShape::Direct => Ok(()),
            RefShape::Symbolic => Err(Refusal::SymbolicRef {
                refname: refname.to_owned(),
                target: "refs/heads/somebody-elses-branch".to_owned(),
            }
            .into()),
            RefShape::CheckedOut => Err(Refusal::CheckedOutRef {
                refname: refname.to_owned(),
                worktree: PathBuf::from("worktrees").join("alpha"),
            }
            .into()),
        }
    }

    fn direct_target(&self, refname: &str) -> Result<Option<String>, UpstrokeError> {
        *self
            .targets_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += 1;
        if self.shape == RefShape::Symbolic {
            return Err(Refusal::SymbolicRef {
                refname: refname.to_owned(),
                target: "refs/heads/somebody-elses-branch".to_owned(),
            }
            .into());
        }
        Ok(self.target())
    }

    fn create_zero_old(
        &self,
        hooks: &mut dyn crate::workspace_manager::EffectHooks,
        refname: &str,
        new: &str,
    ) -> Result<(), UpstrokeError> {
        crate::workspace_manager::refuse_new(refname, new)?;
        let site = EffectSiteId::Ref(RefSite::CreateIntegration);
        self.entered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(crate::util::read_file_bounded(&self.log).unwrap_or_default());
        injected(
            hooks.phase(site, HookPhase::Before),
            site,
            HookPhase::Before,
        )?;
        {
            let mut at = self.at.lock().unwrap_or_else(PoisonError::into_inner);
            if at.is_some() {
                return Err(UpstrokeError::Git {
                    message: format!("`{refname}` already exists; zero-old refuses"),
                });
            }
            *at = Some(new.to_owned());
        }
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((refname.to_owned(), new.to_owned()));
        injected(hooks.phase(site, HookPhase::After), site, HookPhase::After)
    }
}

#[test]
fn the_recording_refs_refuse_a_null_new_value_as_the_real_primitive_does() {
    let refs = RecordingRefs::with_log(Path::new("no-log"), RefShape::Direct, None);
    let null = "0".repeat(40);
    let error = refs
        .create_zero_old(
            &mut crate::workspace_manager::NoHooks,
            "refs/heads/upstroke/run-1",
            &null,
        )
        .expect_err("the double refuses a null new value");
    assert!(
        error.to_string().contains("null object id"),
        "the refusal must name its reason: {error}"
    );
    assert_eq!(refs.created(), Vec::<(String, String)>::new());
    assert_eq!(refs.target(), None);
    assert_eq!(
        refs.log_bytes_at_entries(),
        Vec::<Vec<u8>>::new(),
        "the funnel was not entered"
    );
}

fn injected(
    injection: Injection,
    site: EffectSiteId,
    phase: HookPhase,
) -> Result<(), UpstrokeError> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(UpstrokeError::Refused {
            message: format!("the `{site}` funnel was made to fail at its `{phase}` phase"),
        }),
    }
}

fn container_selection() -> RunnerSelection {
    RunnerSelection {
        kind: RunnerKind::Container,
        image: Some(IMAGE_REF.to_owned()),
        credential_volumes: [(AGENT.to_owned(), VOLUME.to_owned())]
            .into_iter()
            .collect(),
        mounts: Vec::new(),
        from_config: true,
    }
}

struct ArmedHooks {
    inner: HarnessTopologyHooks,
    rundir: ArmedRunDir,
}

struct ArmedRunDir {
    harness: Arc<Mutex<HookHarness>>,
    site: RunDirSite,
    phase: HookPhase,
    nth: usize,
    seen: usize,
}

impl ArmedHooks {
    fn new(
        harness: &Arc<Mutex<HookHarness>>,
        (site, phase, nth): (RunDirSite, HookPhase, usize),
    ) -> Self {
        Self {
            inner: HarnessTopologyHooks::new(Arc::clone(harness)),
            rundir: ArmedRunDir {
                harness: Arc::clone(harness),
                site,
                phase,
                nth,
                seen: 0,
            },
        }
    }
}

impl rundir::RunDirHooks for ArmedRunDir {
    fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .hook(site, phase);
        if site != EffectSiteId::RunDir(self.site) || phase != self.phase {
            return Injection::Proceed;
        }
        self.seen += 1;
        if self.seen == self.nth {
            Injection::Error
        } else {
            Injection::Proceed
        }
    }
}

impl TopologyHooks for ArmedHooks {
    fn effects(&mut self) -> &mut dyn crate::workspace_manager::EffectHooks {
        self.inner.effects()
    }

    fn rundir(&mut self) -> &mut dyn rundir::RunDirHooks {
        &mut self.rundir
    }

    fn events(&mut self) -> &mut dyn crate::events::log::EventHooks {
        self.inner.events()
    }

    fn container(&mut self) -> &mut dyn crate::runner::container::ContainerHooks {
        self.inner.container()
    }

    fn spawn(&mut self) -> &mut dyn crate::agent::proc::SpawnHooks {
        self.inner.spawn()
    }
}

struct Given<'a> {
    runtime: &'a dyn ContainerRuntime,
    preflight: &'a dyn RunnerPreflight,
    today: RunnerSelection,
    inputs: FrozenInputs,
    explicit_root: Option<PathBuf>,
    refs: RecordingRefs,
}

impl<'a> Given<'a> {
    fn healthy(
        fixture: &Fixture,
        runtime: &'a FakeRuntime,
        preflight: &'a dyn RunnerPreflight,
    ) -> Self {
        Self {
            runtime,
            preflight,
            today: container_selection(),
            inputs: fixture.inputs(),
            explicit_root: None,
            refs: RecordingRefs::absent(fixture),
        }
    }
}

fn resume(
    fixture: &Fixture,
    harness: &Arc<Mutex<HookHarness>>,
    given: &Given<'_>,
) -> (Result<Recovered, UpstrokeError>, Vec<String>) {
    let (outcome, warnings) = resume_holding(fixture, harness, given);
    (outcome.map(|(recovered, _handle)| recovered), warnings)
}

fn resume_holding(
    fixture: &Fixture,
    harness: &Arc<Mutex<HookHarness>>,
    given: &Given<'_>,
) -> (Result<(Recovered, RunHandle), UpstrokeError>, Vec<String>) {
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(harness)).recording_durability();
    resume_with(fixture, &mut hooks, given)
}

fn resume_with(
    fixture: &Fixture,
    hooks: &mut dyn TopologyHooks,
    given: &Given<'_>,
) -> (Result<(Recovered, RunHandle), UpstrokeError>, Vec<String>) {
    let liveness = FakeOwnerLiveness::new();
    let view = DisposableDirView::new(ContainerTrace::default());
    let incarnation = IncarnationId(RESUMER.to_owned());
    let manager = fixture.manager();
    let mut warnings = Vec::new();
    let outcome = fixture
        .derive(given.explicit_root.as_deref())
        .and_then(|root| {
            run_recovery_order(
                root,
                &ResumeSeams {
                    repo_root: &fixture.repo_root,
                    worktree_git_dir: &fixture.git_dir,
                    repo_key: &fixture.repo_key,
                    incarnation: &incarnation,
                    inputs: given.inputs.clone(),
                    today: &given.today,
                    runtime: given.runtime,
                    liveness: &liveness,
                    view: &view,
                    preflight: given.preflight,
                    refs: &given.refs,
                    manager: &manager,
                    clock: &Frozen,
                },
                hooks,
                &mut warnings,
            )
        });
    (outcome, warnings)
}

fn harness() -> Arc<Mutex<HookHarness>> {
    Arc::new(Mutex::new(HookHarness::new()))
}

fn any_lock_site_ran(harness: &Arc<Mutex<HookHarness>>) -> Vec<&'static str> {
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    LockSite::ALL
        .iter()
        .copied()
        .filter(|site| seen.touched(EffectSiteId::Lock(*site)))
        .map(LockSite::name)
        .collect()
}

fn first_observation(harness: &Arc<Mutex<HookHarness>>, site: EffectSiteId) -> Option<usize> {
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    seen.coverage()
        .iter()
        .position(|observation| observation.site == site)
}

fn message(error: &UpstrokeError) -> String {
    error.to_string()
}

#[test]
fn resume_with_explicit_private_root_mismatch_refused_before_any_lock() {
    let fixture = Fixture::healthy("explicit-root");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let mut given = Given::healthy(&fixture, &runtime, &certifies);
    given.explicit_root = Some(fixture.root.join("somewhere-else"));

    let (outcome, _) = resume(&fixture, &harness, &given);

    let error = outcome.expect_err("a root the run did not record is refused");
    let text = message(&error);
    assert!(
        text.contains(&fixture.private_root.display().to_string()),
        "the refusal must name the recorded root: {text}"
    );
    assert!(
        text.contains("somewhere-else"),
        "the refusal must name the root that was asked for: {text}"
    );
    assert!(
        any_lock_site_ran(&harness).is_empty(),
        "a refusal at (a0) precedes Lock.AcquireWorktree, so no R17 hold is taken: {:?}",
        any_lock_site_ran(&harness)
    );
    assert!(
        !fixture.worktree_lock_file().exists(),
        "no R25 lock file is created by a refusal that precedes the acquisition"
    );
}

#[test]
fn malformed_recorded_locator_refused_before_any_lock() {
    for (tag, locator) in [
        ("no-runs", format!("/tmp/upstroke-pr7e-root/{RUN_ID}")),
        (
            "wrong-tail",
            "/tmp/upstroke-pr7e-root/runs/another".to_owned(),
        ),
        (
            "escapes",
            format!("/tmp/upstroke-pr7e-root/runs/other/../runs/{RUN_ID}"),
        ),
    ] {
        let fixture = Fixture::build(
            &format!("locator-{tag}"),
            Damage {
                locator: Some(locator.clone()),
                ..Damage::default()
            },
        );
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let (outcome, _) = resume(&fixture, &harness, &given);

        let error = outcome.expect_err("a locator of another shape is refused");
        let text = message(&error);
        assert!(
            text.contains(&locator) && text.contains("is not of the shape"),
            "the refusal must quote the locator it refused ({tag}): {text}"
        );
        assert!(
            any_lock_site_ran(&harness).is_empty(),
            "no R17 hold is taken for a locator refusal ({tag}): {:?}",
            any_lock_site_ran(&harness)
        );
        assert!(
            !fixture.worktree_lock_file().exists(),
            "no R25 lock file is created for a locator refusal ({tag})"
        );
    }
}

#[test]
fn resume_derives_private_root_from_record_when_default_changed() {
    let fixture = Fixture::healthy("nondefault-root");
    let root = fixture.derive(None).expect("(a0) derives");

    let canonical =
        |path: &std::path::Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    assert_eq!(
        canonical(root.private_root()),
        canonical(&fixture.private_root),
        "the authorized root is the one `run_started.private_dir` names"
    );
    assert_ne!(
        root.private_root(),
        rundir::default_private_root(),
        "the fixture must not accidentally be the default root, or this test proves nothing"
    );
    assert_eq!(root.run_id(), RUN_ID);
    assert_eq!(root.first_line(), fixture.first_line.as_slice());
}

#[test]
fn resume_refuses_missing_private_half() {
    let fixture = Fixture::build(
        "no-private-half",
        Damage {
            no_private_half: true,
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a missing private half refuses"));
    assert!(
        text.contains("is not recreated"),
        "the refusal says the half is not recreated: {text}"
    );
    assert!(
        !fixture.private_root.join("runs").join(RUN_ID).exists(),
        "the private half must still be absent: nothing recreates it"
    );
}

#[test]
fn resume_refuses_missing_or_disagreeing_owner_record() {
    let cases: Vec<(&str, Damage, &str)> = vec![
        (
            "absent",
            Damage {
                no_owner_record: true,
                ..Damage::default()
            },
            "owner.json",
        ),
        (
            "run-id",
            Damage {
                owner: Some(|owner| owner.run_id = "01KZTPR7E00000000000000009".to_owned()),
                ..Damage::default()
            },
            "run id",
        ),
        (
            "repo-key",
            Damage {
                owner: Some(|owner| owner.repo_key = "0123456789abcdef".to_owned()),
                ..Damage::default()
            },
            "repo key",
        ),
        (
            "public-dir",
            Damage {
                owner: Some(|owner| owner.public_dir = "/elsewhere/runs/x".to_owned()),
                ..Damage::default()
            },
            "public directory",
        ),
        (
            "incarnation",
            Damage {
                owner: Some(|owner| owner.incarnation = RESUMER.to_owned()),
                ..Damage::default()
            },
            "incarnation",
        ),
    ];
    for (tag, damage, expected) in cases {
        let fixture = Fixture::build(&format!("owner-{tag}"), damage);
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let (outcome, _) = resume(&fixture, &harness, &given);

        let text = message(&outcome.expect_err("a disagreeing owner record refuses"));
        assert!(
            text.contains(expected),
            "the refusal for `{tag}` must name `{expected}`: {text}"
        );
        let private = fixture.private_root.join("runs").join(RUN_ID);
        assert!(
            !private.join("questions").exists() && !private.join("report.json").exists(),
            "a record refusal precedes every private write ({tag})"
        );
    }
}

#[test]
fn resume_refuses_commit_record_digest_mismatch() {
    let fixture = Fixture::build(
        "commit-digest",
        Damage {
            commit: Some(|commit| {
                commit.run_started_sha256 = format!("sha256:{}", "0".repeat(64));
            }),
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a commit record that names another line refuses"));
    let actual = rundir::run_started_sha256(&fixture.first_line);
    assert!(
        text.contains(&format!("sha256:{}", "0".repeat(64))) && text.contains(&actual),
        "the refusal quotes what the record says and what the line digests: {text}"
    );
    assert!(
        !harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .touched(EffectSiteId::Event(EventSite::OpenLog)),
        "a commit-record refusal is at (a) and precedes the barrier's Event.OpenLog"
    );
}

#[test]
fn resume_refuses_owner_record_runner_mismatch() {
    let fixture = Fixture::build(
        "owner-runner",
        Damage {
            owner: Some(|owner| {
                if let Some(image) = owner.runner.image.as_mut() {
                    image.reference = "ghcr.io/example/another:9.9".to_owned();
                }
            }),
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a runner the two records disagree on refuses"));
    assert!(
        text.contains("image reference"),
        "the refusal names which field moved: {text}"
    );
    assert!(
        !harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .touched(EffectSiteId::Event(EventSite::OpenLog)),
        "the runner comparison is at (a), before the barrier"
    );
}

#[test]
fn resume_refuses_digest_mismatch() {
    let fixture = Fixture::healthy("digest-mismatch");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let mut given = Given::healthy(&fixture, &runtime, &certifies);
    given.inputs.normalized_plan_digest = "sha256:not-the-recorded-one".to_owned();

    let before = fixture.log_bytes();
    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a moved plan digest refuses"));
    assert!(
        text.contains(BarrierStep::CheckedReplay.name()),
        "the refusal names the barrier step: {text}"
    );
    assert!(
        text.contains("normalized plan"),
        "and the digest that disagreed: {text}"
    );
    assert_eq!(
        fixture.log_bytes(),
        before,
        "a barrier refusal appends nothing"
    );
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        !seen.touched(EffectSiteId::RunDir(RunDirSite::RemoveMarker)),
        "no census effect follows a refused replay"
    );
}

#[test]
fn resume_establishes_stable_prefix_barrier_before_any_fold_derived_effect() {
    let fixture = Fixture::healthy("barrier-order");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    assert_eq!(
        rundir::run_dir_names(&fixture.repo_root),
        vec![RUN_ID.to_owned()],
        "the anchor is the census's first effect only while this run's \
         directory is the only one in the tree"
    );

    let (outcome, _) = resume(&fixture, &harness, &given);
    outcome.expect("the healthy resume completes");

    let marker = first_observation(&harness, EffectSiteId::RunDir(RunDirSite::RemoveMarker))
        .expect("the census removes this run's stale marker");
    let open = first_observation(&harness, EffectSiteId::Event(EventSite::OpenLog))
        .expect("Event.OpenLog ran");
    let proven = first_observation(&harness, EffectSiteId::Event(EventSite::ProvePrefixStable))
        .expect("Event.ProvePrefixStable ran");
    let append = first_observation(&harness, EffectSiteId::Event(EventSite::Append))
        .or_else(|| first_observation(&harness, EffectSiteId::Event(EventSite::AppendFirst)));

    assert!(
        open < marker,
        "Event.OpenLog ({open}) before the census ({marker})"
    );
    assert!(
        proven < marker,
        "Event.ProvePrefixStable ({proven}) before the census ({marker})"
    );
    if let Some(append) = append {
        assert!(
            proven < append,
            "the barrier ({proven}) before every recovery event ({append}) — O33 and O18"
        );
    }
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        seen.reached_point(
            EffectSiteId::Event(EventSite::OpenLog),
            SubEffectPoint::SyncPrefix,
            InjectionMode::ErrorReturn
        ),
        "the SyncPrefix point is consulted, which is what makes it armable"
    );
}

#[test]
fn resume_refuses_before_any_fold_derived_effect_when_prefix_sync_fails() {
    let fixture = Fixture::healthy("sync-fails");
    let harness = harness();
    harness
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::OpenLog),
            SubEffectPoint::SyncPrefix,
            InjectionMode::ErrorReturn,
        )
        .expect("SyncPrefix supports an error return");
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let before = fixture.log_bytes();
    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a failed SyncPrefix refuses"));
    assert!(
        text.contains(BarrierStep::SyncPrefix.name()),
        "the refusal names the failed step: {text}"
    );
    assert_eq!(fixture.log_bytes(), before, "no append handle was used");
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        !seen.touched(EffectSiteId::RunDir(RunDirSite::RemoveMarker)),
        "no census reclaim follows a failed sync"
    );
    assert!(
        !seen.touched(EffectSiteId::Event(EventSite::Append)),
        "and no recovery event"
    );
}

const ALPHA: TaskKey = TaskKey(0);
const GEN: GenerationId = GenerationId(0);

fn dispatched_at(base: &CommitSha) -> TopologyEventBody {
    let TopologyEventBody::TaskDispatched { mut data } = dispatched() else {
        unreachable!("`dispatched` builds a `TaskDispatched`")
    };
    data.base_sha = base.clone();
    TopologyEventBody::TaskDispatched { data }
}

fn dispatched() -> TopologyEventBody {
    TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key: ALPHA,
            generation: GEN,
            base_sha: CommitSha("a".repeat(40)),
            worktree_path: "wt/g0".to_owned(),
            lease: LeaseGrant::Predicted {
                paths: PathSet::Prefixes {
                    paths: vec![GitPath("src/alpha".to_owned())],
                },
            },
            source_candidate: None,
        },
    }
}

fn for_task(key: TaskKey, prefix: &str, body: TopologyEventBody) -> TopologyEventBody {
    match body {
        TopologyEventBody::TaskDispatched { mut data } => {
            data.key = key;
            data.worktree_path = format!("wt/{prefix}-g0");
            data.lease = LeaseGrant::Predicted {
                paths: PathSet::Prefixes {
                    paths: vec![GitPath(format!("src/{prefix}"))],
                },
            };
            TopologyEventBody::TaskDispatched { data }
        }
        TopologyEventBody::AttemptStarted { mut data } => {
            data.key = key;
            TopologyEventBody::AttemptStarted { data }
        }
        TopologyEventBody::AttemptFinished { mut data } => {
            data.key = key;
            TopologyEventBody::AttemptFinished { data }
        }
        other => other,
    }
}

fn in_generation(generation: GenerationId, body: TopologyEventBody) -> TopologyEventBody {
    match body {
        TopologyEventBody::TaskDispatched { mut data } => {
            data.generation = generation;
            data.worktree_path = format!("wt/g{}", generation.0);
            TopologyEventBody::TaskDispatched { data }
        }
        TopologyEventBody::AttemptStarted { mut data } => {
            data.generation = generation;
            TopologyEventBody::AttemptStarted { data }
        }
        TopologyEventBody::AttemptFinished { mut data } => {
            data.generation = generation;
            TopologyEventBody::AttemptFinished { data }
        }
        other => other,
    }
}

fn attempt_started(attempt: u32) -> TopologyEventBody {
    TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: ALPHA,
            generation: GEN,
            attempt: AttemptNumber(attempt),
            rung: 0,
            binding: RungBinding {
                tier: Tier::Mid,
                agent: AGENT.to_owned(),
                model: "claude-opus-5".to_owned(),
                pinned: false,
                effort: Effort::High,
            },
            pool: None,
            resume_session: None,
            materialization_observed: None,
        },
    }
}

fn attempt_record(attempt: u32) -> AttemptRecord {
    AttemptRecord {
        attempt,
        tier: "mid".to_owned(),
        model: "claude-opus-5".to_owned(),
        pool: None,
        resumed: false,
        duration: Duration::from_millis(5),
        cost_usd: Some(0.5),
        reviews: Vec::new(),
        session_id: None,
        usage: None,
        failure: None,
    }
}

fn attempt_finished(attempt: u32, settlement: AttemptSettlement) -> TopologyEventBody {
    let mut record = attempt_record(attempt);
    record.failure = Some(crate::events::FailureRecord {
        kind: crate::ladder::FailureKind::GateFailed,
        origin: crate::ladder::FailureOrigin::Worker,
        reason: "the fixture's judged failure".to_owned(),
        detail: None,
    });
    if let AttemptSettlement::Retained {
        retained_session, ..
    } = &settlement
    {
        record.session_id = Some(retained_session.0.clone());
    }
    TopologyEventBody::AttemptFinished {
        data: Box::new(AttemptFinished4 {
            key: ALPHA,
            generation: GEN,
            attempt: AttemptNumber(attempt),
            record: Box::new(record),
            settlement,
        }),
    }
}

fn attempt_finished_failing(
    attempt: u32,
    kind: crate::ladder::FailureKind,
    reason: &str,
    detail: &str,
    settlement: AttemptSettlement,
) -> TopologyEventBody {
    let TopologyEventBody::AttemptFinished { mut data } = attempt_finished(attempt, settlement)
    else {
        unreachable!("attempt_finished builds an attempt_finished")
    };
    data.record.failure = Some(crate::events::FailureRecord {
        kind,
        origin: crate::ladder::FailureOrigin::Worker,
        reason: reason.to_owned(),
        detail: Some(detail.to_owned()),
    });
    TopologyEventBody::AttemptFinished { data }
}

fn budget_exceeded(epoch: u32) -> TopologyEventBody {
    TopologyEventBody::BudgetExceeded {
        data: BudgetExceeded4 {
            epoch: Epoch(epoch),
            budget: BudgetKind::Run,
            limit_usd: 1.0,
            spent_usd: 2.0,
            key: Some(ALPHA),
        },
    }
}

fn run_finished(outcome: RunOutcome, halted_at: Option<TaskKey>) -> TopologyEventBody {
    TopologyEventBody::RunFinished {
        data: RunFinished4 {
            outcome,
            halted_at,
            merged: 0,
            parked: 0,
        },
    }
}

#[test]
fn resume_finalizes_halted_then_refuses() {
    for (tag, outcome, extra) in [
        (
            "halted",
            RunOutcome::Halted,
            vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Failed {
                            halts_run: true,
                            reason: "the ladder ran out".to_owned(),
                        },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
                run_finished(RunOutcome::Halted, Some(ALPHA)),
            ],
        ),
        (
            "complete",
            RunOutcome::Complete,
            vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Failed {
                            halts_run: false,
                            reason: "the ladder ran out and the policy does not halt".to_owned(),
                        },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
                run_finished(RunOutcome::Complete, None),
            ],
        ),
    ] {
        let fixture = Fixture::build(
            &format!("finished-{tag}"),
            Damage {
                extra,
                ..Damage::default()
            },
        );
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let before = fixture.log_bytes();
        let (result, _) = resume(&fixture, &harness, &given);

        let text = message(&result.expect_err("a finished run does not continue"));
        assert!(
            text.contains("already finished"),
            "the refusal says the run is over ({tag}): {text}"
        );
        assert!(
            text.contains(match outcome {
                RunOutcome::Halted => "halted",
                _ => "complete",
            }),
            "and names the outcome ({tag}): {text}"
        );
        assert_eq!(
            fixture.log_bytes(),
            before,
            "a (b) refusal appends nothing ({tag})"
        );
        assert!(
            !fixture.public().join("report.json").exists(),
            "PR7 does not finalize: `RunDir.WriteReport` is `t_finalize`, out of this slice's \
             rows ({tag})"
        );
        assert!(
            !harness
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .touched(EffectSiteId::RunDir(RunDirSite::WriteReport)),
            "and the report site never ran ({tag})"
        );
    }
}

#[test]
fn resume_rebuilds_runner_from_record_and_warns_on_config_drift() {
    let fixture = Fixture::healthy("config-drift");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let mut given = Given::healthy(&fixture, &runtime, &certifies);
    given.today.credential_volumes = [(AGENT.to_owned(), "somebody-elses-volume".to_owned())]
        .into_iter()
        .collect();
    runtime.add_volume("somebody-elses-volume");

    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a config that differs is a warning, not a refusal");

    assert!(
        recovered
            .warnings
            .iter()
            .any(|warning| warning.contains("credential volume set")),
        "the warning names which field differs: {:?}",
        recovered.warnings
    );
    let log = String::from_utf8(fixture.log_bytes()).expect("the log is utf-8");
    let resumed = log.lines().last().expect("run_resumed is last");
    assert!(
        resumed.contains(VOLUME) && !resumed.contains("somebody-elses-volume"),
        "run_resumed records the recorded runner, not today's config: {resumed}"
    );
}

#[test]
fn resume_warns_when_reference_moved_and_uses_recorded_image_id() {
    let fixture = Fixture::healthy("moved-reference");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let other = format!("sha256:{}", "3".repeat(64));
    runtime.add_image(&other, None);
    runtime.move_tag(IMAGE_REF, &other);
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a moved reference is a warning, not a refusal");

    assert!(
        recovered
            .warnings
            .iter()
            .any(|warning| warning.contains(IMAGE_REF) && warning.contains(&other)),
        "the warning names the reference and where it now points: {:?}",
        recovered.warnings
    );
    let log = String::from_utf8(fixture.log_bytes()).expect("the log is utf-8");
    let resumed = log.lines().last().expect("run_resumed is last");
    assert!(
        resumed.contains(IMAGE_ID) && !resumed.contains(&other),
        "the run continues from its recorded image id: {resumed}"
    );
}

#[test]
fn resume_refuses_by_inspection_before_any_spawn_when_runtime_image_id_or_volume_absent() {
    struct NeverRuns;

    impl RunnerPreflight for NeverRuns {
        fn certify(&self, _policy: &RunnerPolicy) -> Result<(), UpstrokeError> {
            unreachable!("an inspection refusal precedes every spawn");
        }
    }

    type Damage = fn(&FakeRuntime);
    let cases: [(&str, Damage, &str); 3] = [
        (
            "runtime",
            |runtime| runtime.set_all_unreachable(),
            "cannot be reached",
        ),
        (
            "image-id",
            |runtime| runtime.move_tag(IMAGE_REF, "sha256:absent"),
            "no longer holds the recorded image id",
        ),
        (
            "volume",
            |runtime| runtime.remove_volume(VOLUME),
            "credential volume",
        ),
    ];
    for (tag, damage, expected) in cases {
        let fixture = Fixture::healthy(&format!("inspection-{tag}"));
        let harness = harness();
        let runtime = runtime_holding_the_record();
        if tag == "image-id" {
            runtime.add_image("sha256:absent", None);
        }
        damage(&runtime);
        if tag == "image-id" {
            let fresh = FakeRuntime::new(ContainerTrace::default());
            fresh.add_image("sha256:absent", None);
            fresh.tag(IMAGE_REF, "sha256:absent");
            fresh.add_volume(VOLUME);
            let never = NeverRuns;
            let mut given = Given::healthy(&fixture, &runtime, &never);
            given.runtime = &fresh;
            let (result, _) = resume(&fixture, &harness, &given);
            let text = message(&result.expect_err("an absent recorded id refuses"));
            assert!(
                text.contains(expected) && text.contains(IMAGE_ID),
                "the refusal names the recorded id ({tag}): {text}"
            );
            continue;
        }
        let never = NeverRuns;
        let given = Given::healthy(&fixture, &runtime, &never);
        let (result, _) = resume(&fixture, &harness, &given);
        let text = message(&result.expect_err("an inspection refusal"));
        assert!(
            text.contains(expected),
            "the refusal names what could not be re-established ({tag}): {text}"
        );
    }
}

fn chain_to_census(
    fixture: &Fixture,
    harness: &Arc<Mutex<HookHarness>>,
    runtime: &dyn ContainerRuntime,
    incarnation: &IncarnationId,
) -> Result<ResumeCensused, UpstrokeError> {
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(harness));
    chain_to_census_with(fixture, &mut hooks, runtime, incarnation)
}

fn chain_to_census_with(
    fixture: &Fixture,
    hooks: &mut dyn TopologyHooks,
    runtime: &dyn ContainerRuntime,
    incarnation: &IncarnationId,
) -> Result<ResumeCensused, UpstrokeError> {
    let liveness = FakeOwnerLiveness::new();
    let view = DisposableDirView::new(ContainerTrace::default());
    let mut warnings = Vec::new();
    let root = fixture.derive(None)?;
    let locks = LocksHeld::take(root, &fixture.repo_root, &fixture.git_dir, hooks.rundir())?;
    let records = RecordsVerified::verify(locks, &fixture.repo_key)?;
    let log_path = records.locks().root().log_path();
    let committed = records.commit().run_started_sha256.clone();
    let prefix = crate::events::log::establish_stable_prefix(
        &log_path,
        fixture.inputs(),
        Some(&committed),
        &mut warnings,
        hooks.events(),
    )?;
    let barrier = BarrierHeld::from(records, prefix)?;
    ResumeCensused::census(
        barrier,
        &CensusSeams {
            incarnation,
            repo_root: &fixture.repo_root,
            repo_key: &fixture.repo_key,
            runtime,
            liveness: &liveness,
            view: &view,
        },
        hooks,
    )
}

#[test]
fn resume_of_nondefault_root_run_reclaims_earlier_incarnation_intents_in_recorded_root() {
    let fixture = Fixture::healthy("earlier-incarnation");
    let harness = harness();
    let runtime = runtime_holding_the_record();

    let invocation = crate::runner::InvocationId::probe(
        crate::runner::ProbeTarget::Agent(crate::runner::AgentId::new(AGENT)),
        0,
    )
    .expect("the agent probe identity");
    let name = crate::runner::container::intent::ContainerName::new(
        fixture.repo_key.as_str(),
        RUN_ID,
        CREATOR,
        &invocation,
    )
    .expect("a container name for the creator incarnation");
    let record = crate::runner::container::intent::ContainerIntent::new(
        RUN_ID.to_owned(),
        &fixture.public(),
        CREATOR.to_owned(),
        fixture.repo_key.as_str().to_owned(),
        invocation.render(),
        crate::runner::policy::runner_policy_sha256(&fixture.started.runner),
    );
    let mut container_hooks = crate::runner::container::NoHooks;
    crate::runner::container::write_intent(
        &mut container_hooks,
        crate::topology::effects::ContainerSite::WriteIntent,
        &fixture.private_root,
        &name,
        &record,
    )
    .expect("the container funnel writes the intent");
    runtime.seed_container(
        name.as_str(),
        record.labels(&fixture.private_root),
        IMAGE_ID,
        IMAGE_ID,
        crate::runner::container::runtime::Liveness::Running,
    );

    let censused = chain_to_census(
        &fixture,
        &harness,
        &runtime,
        &IncarnationId(RESUMER.to_owned()),
    )
    .expect("the census completes");
    let report = censused.containers();

    assert_eq!(
        report.private_root, fixture.private_root,
        "the census scanned the recorded root, not today's default"
    );
    assert!(
        report
            .reclaimed
            .iter()
            .any(|entry| entry.name == name && entry.incarnation == CREATOR),
        "the creator incarnation's container is dead by construction and is reclaimed: {:?}",
        report.reclaimed
    );
    assert!(
        !runtime
            .container_names()
            .contains(&name.as_str().to_owned()),
        "and it is gone from the runtime"
    );
}

#[test]
fn resume_reclaims_a_provable_husk_beside_the_run_and_retains_a_possibly_committed_one() {
    const RECLAIMED: &str = "01KZTHUSK00000000000000002";
    const RETAINED: &str = "01KZTKEEP00000000000000003";

    let fixture = Fixture::healthy("husk-beside");
    let harness = harness();
    let runtime = runtime_holding_the_record();

    let reclaimed = plant_husk(&fixture, RECLAIMED, false);
    let retained = plant_husk(&fixture, RETAINED, true);
    let retained_before = tree_bytes(&retained.private);
    assert!(
        !retained_before.is_empty(),
        "the retained husk must have a private half, or its comparison proves nothing"
    );

    let censused = chain_to_census(
        &fixture,
        &harness,
        &runtime,
        &IncarnationId(RESUMER.to_owned()),
    )
    .expect("the census completes");
    let report = censused.run_dirs();

    assert_eq!(
        report
            .of(RECLAIMED)
            .expect("the provable husk is censused")
            .outcome,
        RunDirOutcome::ReclaimedBothHalves,
        "a resume reclaims under the ownership proof; it does not merely report"
    );
    assert!(!reclaimed.private.exists(), "the private half is gone");
    assert!(!reclaimed.public.exists(), "and so is the public directory");

    let private_at = first_observation(
        &harness,
        EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
    )
    .expect("the private half went through the proof-token funnel");
    let public_at = first_observation(&harness, EffectSiteId::RunDir(RunDirSite::RemovePublicHusk))
        .expect("and the public directory through its own");
    assert!(
        private_at < public_at,
        "the private half first ({private_at}), the public directory with the marker last \
         ({public_at})"
    );

    assert_eq!(
        report
            .of(RETAINED)
            .expect("the retained husk is censused")
            .outcome,
        RunDirOutcome::Retained(RetainReason::PossiblyCommitted),
    );
    assert_eq!(
        tree_bytes(&retained.private),
        retained_before,
        "nothing private that carries a commit record is deleted by any census"
    );
    assert!(retained.public.exists(), "nor is its public half");

    assert_eq!(
        report
            .of(RUN_ID)
            .expect("the resuming run is censused too")
            .outcome,
        RunDirOutcome::RepairedStaleMarker,
    );
    assert!(!fixture.public().join(rundir::MARKER).exists());
    assert!(fixture.log().exists(), "and the run itself is untouched");
}

#[test]
fn resume_completes_past_a_husk_whose_private_half_cannot_be_removed() {
    const STUCK: &str = "01AAAASTUCK000000000000000";
    assert!(STUCK < RUN_ID, "the husk must sort before this run's id");

    let fixture = Fixture::healthy("husk-unreclaimable");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let stuck = plant_husk(&fixture, STUCK, false);
    let before = tree_bytes(&stuck.private);
    assert!(
        !before.is_empty(),
        "the husk must have a private half, or its comparison proves nothing"
    );
    assert!(
        fixture.public().join(rundir::MARKER).exists(),
        "this run's own stale marker must be there, or the second claim is vacuous"
    );

    let mut hooks = ArmedHooks::new(
        &harness,
        (RunDirSite::RemovePrivateHusk, HookPhase::Before, 1),
    );
    let (outcome, _) = resume_with(&fixture, &mut hooks, &given);

    outcome.expect("a husk beside the run cannot end the resume");

    assert!(stuck.public.exists(), "the public half was removed anyway");
    assert!(
        stuck.public.join(rundir::MARKER).exists(),
        "`.creating` is the private half's only locator and it is gone"
    );
    assert_eq!(
        tree_bytes(&stuck.private),
        before,
        "the arming is `Before`, so the removal never ran"
    );

    assert!(
        !fixture.public().join(rundir::MARKER).exists(),
        "the own-run stale-marker repair was skipped because a husk sorting \
         earlier could not be reclaimed"
    );
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        seen.touched(EffectSiteId::RunDir(RunDirSite::RemoveMarker)),
        "recovery step (a1)'s own repair never reached its funnel"
    );
    assert!(
        !seen.touched(EffectSiteId::RunDir(RunDirSite::RemovePublicHusk)),
        "the public half was removed after the private removal refused, which \
         orphans the private half permanently"
    );
}

#[test]
fn the_resume_census_reports_the_husk_it_could_not_reclaim() {
    const STUCK: &str = "01AAAASTUCK000000000000000";

    let fixture = Fixture::healthy("husk-unreclaimable-report");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let stuck = plant_husk(&fixture, STUCK, false);

    let mut hooks = ArmedHooks::new(
        &harness,
        (RunDirSite::RemovePrivateHusk, HookPhase::Before, 1),
    );
    let censused = chain_to_census_with(
        &fixture,
        &mut hooks,
        &runtime,
        &IncarnationId(RESUMER.to_owned()),
    )
    .expect("the census completes over a husk it could not reclaim");
    let report = censused.run_dirs();

    let entry = report.of(STUCK).expect("the husk is still an entry");
    let RunDirOutcome::Unreclaimable { step, detail } = &entry.outcome else {
        panic!("the failure is not an outcome: {:?}", entry.outcome);
    };
    assert_eq!(*step, FailedStep::PrivateHalf);
    assert!(!detail.is_empty(), "the error was dropped");
    assert!(
        !entry.outcome.deleted_a_private_half(),
        "a removal that returned an error claims the half is gone"
    );
    assert!(
        entry.outcome.may_have_deleted_a_private_half(),
        "a removal that may have emptied the tree reports it untouched"
    );
    assert_eq!(
        entry.locator.as_deref(),
        Some(stuck.private.as_path()),
        "retained and reported **with its locator**"
    );
    assert_eq!(report.unreclaimable().len(), 1);

    assert_eq!(
        report
            .of(RUN_ID)
            .expect("the resuming run is censused too")
            .outcome,
        RunDirOutcome::RepairedStaleMarker,
    );
}

#[test]
fn resume_refused_while_reaper_hold_observed_then_succeeds() {
    let fixture = Fixture::healthy("reaper-hold");

    #[cfg(unix)]
    {
        let cleanup = fixture.public().join("cleanup.lock");
        mkdir(&cleanup);
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);
        let (result, _) = resume(&fixture, &harness, &given);
        let text = message(&result.expect_err("a surviving reaper hold refuses"));
        assert!(
            text.contains("still cleaning agent processes"),
            "the refusal names the hold it observed: {text}"
        );
        assert!(
            harness
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .observed(
                    EffectSiteId::Lock(LockSite::ObserveCleanupHold),
                    HookPhase::Before
                ),
            "R28 is observed, never owned — and the site says so"
        );
        rundir::remove_public_husk(&cleanup, &mut NoHooks).expect("the reaper released its hold");
    }

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    result.expect("with no hold observed, the resume proceeds");
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        seen.observed(
            EffectSiteId::Lock(LockSite::ObserveCleanupHold),
            HookPhase::Before
        ),
        "the hold is observed on every resume, not only when one is held"
    );
    assert!(
        seen.observed(
            EffectSiteId::Lock(LockSite::AcquireWorktree),
            HookPhase::Before
        ) && seen.observed(EffectSiteId::Lock(LockSite::AcquireRun), HookPhase::Before),
        "and both R17 holds were taken"
    );
}

fn replayed(fixture: &Fixture) -> TopologyFold {
    let bytes = fixture.log_bytes();
    let events = TopologyFold::parse_log(&bytes).expect("the log parses");
    TopologyFold::replay(fixture.inputs(), &events).expect("and folds")
}

#[test]
fn resume_clears_budget_stop_and_wakes_deferred() {
    let fixture = Fixture::build(
        "budget-stop",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Deferred {
                            defers: 1,
                            reason: "the pool was exhausted".to_owned(),
                        },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
                budget_exceeded(0),
            ],
            ..Damage::default()
        },
    );

    let before = replayed(&fixture);
    assert!(
        before.budget_stop().is_some(),
        "the fixture must carry a stop, or this test proves nothing"
    );
    assert_eq!(
        before.task_state(ALPHA),
        Some(TaskState::Deferred),
        "and a deferred task"
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a budget-stopped run resumes");

    assert!(recovered.resumed.budget_stop_cleared);
    assert_eq!(
        recovered.resumed.epoch, 1,
        "the resume opens the next epoch"
    );
    let after = replayed(&fixture);
    assert!(
        after.budget_stop().is_none(),
        "the stop belongs to the epoch that hit the old ceiling"
    );
    assert_eq!(
        after.task_state(ALPHA),
        Some(TaskState::Pending),
        "and every Deferred task is woken by the resume"
    );
}

#[test]
fn steps_d_and_e_reach_every_generation_not_the_first() {
    const BETA: TaskKey = TaskKey(1);

    let fixture = Fixture::build(
        "loops-reach-every",
        Damage {
            two_tasks: true,
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Retained {
                        retained_session: SessionId("alpha-session".to_owned()),
                        retained_incarnation: Epoch(0),
                    },
                ),
                for_task(BETA, "beta", dispatched()),
                for_task(BETA, "beta", attempt_started(1)),
                for_task(
                    BETA,
                    "beta",
                    attempt_finished(
                        1,
                        AttemptSettlement::Retained {
                            retained_session: SessionId("beta-session".to_owned()),
                            retained_incarnation: Epoch(0),
                        },
                    ),
                ),
            ],
            ..Damage::default()
        },
    );

    let before = replayed(&fixture);
    assert!(
        before.ready_retry(ALPHA) && before.ready_retry(BETA),
        "both tasks must be retryable before the resume, or a `.take(1)` would \
         pass this test by accident"
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a run with two retained sessions resumes");

    assert_eq!(
        recovered.retained_closed, 2,
        "step (e) closed {} of two retained generations — a loop that stops at \
         the first leaves the rest holding their entitlements for the whole run",
        recovered.retained_closed
    );

    let after = replayed(&fixture);
    for (key, name) in [(ALPHA, "alpha"), (BETA, "beta")] {
        assert!(
            !after.ready_retry(key),
            "{name}'s retained generation survived the resume"
        );
    }
}

#[test]
fn retry_refused_after_resume() {
    let fixture = Fixture::build(
        "retained-retry",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Retained {
                        retained_session: SessionId("session-of-the-dead-incarnation".to_owned()),
                        retained_incarnation: Epoch(0),
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let before = replayed(&fixture);
    assert!(
        before.ready_retry(ALPHA),
        "before the resume the retained generation is retryable, or this test proves nothing"
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a run with a retained session resumes");

    assert_eq!(
        recovered.retained_closed, 1,
        "step (e) closes every RetainedIdle generation"
    );
    let after = replayed(&fixture);
    assert!(
        !after.ready_retry(ALPHA),
        "the retained session is gone, so there is no same-session retry to take"
    );
    let refused = after
        .plan_transition(&event(attempt_started(2)))
        .expect_err("a retry into a closed generation is refused");
    assert!(
        format!("{refused}").contains("generation"),
        "the refusal is about the generation: {refused}"
    );
}

#[test]
fn run_resumed_records_identical_runner_identity() {
    let fixture = Fixture::healthy("identical-runner");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (result, _) = resume(&fixture, &harness, &given);
    result.expect("the healthy resume completes");

    let bytes = fixture.log_bytes();
    let events = TopologyFold::parse_log(&bytes).expect("the log parses");
    let resumed = events
        .iter()
        .rev()
        .find_map(|event| match &event.body {
            TopologyEventBody::RunResumed { data } => Some(data.clone()),
            _ => None,
        })
        .expect("the log ends with a run_resumed");

    assert_eq!(
        fixture.started.runner.difference(&resumed.runner),
        None,
        "the incarnation established exactly the recorded runner"
    );
    assert_eq!(resumed.incarnation.0, RESUMER, "and recorded its own id");
    assert_eq!(
        resumed.probed_agents, fixture.started.probed_agents,
        "and the agents its pre-flight certified"
    );
}

#[test]
fn forged_run_resumed_with_different_runner_identity_refused_on_replay() {
    let fixture = Fixture::healthy("forged-runner");
    let mut forged = fixture.started.runner.clone();
    if let Some(image) = forged.image.as_mut() {
        image.id = format!("sha256:{}", "9".repeat(64));
    }
    let mut warnings = Vec::new();
    let mut log =
        EventLog::open(EventSite::OpenLog, &fixture.log(), &mut warnings).expect("the log reopens");
    let (line, _) = TopologyLine::round_trip(&event(TopologyEventBody::RunResumed {
        data: Box::new(RunResumed4 {
            incarnation: IncarnationId(RESUMER.to_owned()),
            runner: forged,
            probed_agents: vec![AGENT.to_owned()],
            upstroke_version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
    }))
    .expect("the forged event serializes — the wire format is not the check");
    log.append_topology(EventSite::Append, &line)
        .expect("nothing stops a forged line reaching the file");
    drop(log);

    let bytes = fixture.log_bytes();
    let events = TopologyFold::parse_log(&bytes).expect("the forged log still parses");
    let error = TopologyFold::replay(fixture.inputs(), &events)
        .expect_err("the checked fold refuses the forged identity");
    assert!(
        format!("{error}").contains("image id"),
        "the refusal names which field moved: {error}"
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let text = message(&result.expect_err("the barrier refuses a forged prefix"));
    assert!(
        text.contains(BarrierStep::CheckedReplay.name()),
        "the refusal names the barrier step: {text}"
    );
}

#[test]
fn resume_after_append_error_follows_surviving_prefix() {
    let fixture = Fixture::healthy("append-error");
    let first = harness();
    first
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::Append),
            SubEffectPoint::Synced,
            InjectionMode::ErrorReturn,
        )
        .expect("the Synced point supports an error return");
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let lines_before = fixture.log_bytes().iter().filter(|b| **b == b'\n').count();
    let (result, _) = resume(&fixture, &first, &given);
    let text = message(&result.expect_err("an errored append ends the command"));
    assert!(
        text.contains(crate::events::log::INJECTED_PREFIX),
        "the error is the funnel's own: {text}"
    );
    assert!(
        !text.contains(crate::events::log::POISONED_PREFIX),
        "the command ended at the errored append; it did not attempt a second one: {text}"
    );
    let lines_after = fixture.log_bytes().iter().filter(|b| **b == b'\n').count();
    assert_eq!(
        lines_after,
        lines_before + 1,
        "the line is durable — this is the after-append order of T-APPEND (e-s)"
    );

    assert!(text.contains(RUN_ID), "the report names the run: {text}");
    assert!(
        text.contains("run_resumed"),
        "and the event kind whose outcome is unknown: {text}"
    );
    assert!(
        text.contains("Event.Append"),
        "and the site it was filed at: {text}"
    );
    assert!(
        text.contains("the proven prefix contains the line"),
        "and whether the proven prefix contains the line. Present here, and asserted as the \
         sentence rather than as \"some outcome\": the injection is at `Synced`, after the bytes \
         reached the file, so a protocol that reported `absent` would be wrong in the direction \
         that loses a durable transition: {text}"
    );
    assert!(
        text.contains("resumable"),
        "and the run is reported resumable, which is what makes ending here safe: {text}"
    );

    let seen = first.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(
        seen.count(EffectSiteId::Event(EventSite::OpenLog), HookPhase::Before),
        2,
        "`Event.OpenLog` twice: recovery step (a1)'s barrier, then the protocol's reopen after \
         the failed append. Once means no reopen happened and the outcome was never established."
    );
    assert_eq!(
        seen.count(
            EffectSiteId::Event(EventSite::ProvePrefixStable),
            HookPhase::Before
        ),
        2,
        "and the stable-prefix barrier is re-established over the reopened log before anything is \
         reported"
    );
    assert_eq!(
        seen.count(EffectSiteId::Event(EventSite::Append), HookPhase::Before),
        1,
        "and the append itself is never retried"
    );
    drop(seen);

    let second = harness();
    let runtime = runtime_holding_the_record();
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &second, &given);
    let recovered = result.expect("the next resume establishes its own barrier and continues");
    assert_eq!(
        recovered.resumed.epoch, 2,
        "the surviving prefix already carried one resume, so this is the second epoch"
    );
    assert!(
        first_observation(&second, EffectSiteId::Event(EventSite::ProvePrefixStable)).is_some(),
        "and it proved the prefix before acting on it"
    );
}

#[test]
fn an_append_error_during_recovery_cancels_the_reservation_and_every_running_invocation() {
    let fixture = Fixture::healthy("append-error-ledgers");
    let harness = harness();
    harness
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::Append),
            SubEffectPoint::Synced,
            InjectionMode::ErrorReturn,
        )
        .expect("the Synced point supports an error return");
    let runtime = runtime_holding_the_record();
    let incarnation = IncarnationId(RESUMER.to_owned());

    let censused =
        chain_to_census(&fixture, &harness, &runtime, &incarnation).expect("the census completes");
    let rebuilt = RunnerRebuilt::rebuild(censused, &container_selection(), Some(&runtime))
        .expect("the recorded runner rebuilds by inspection");
    let certified =
        PreflightCertified::certify(rebuilt, &AlwaysCertifies).expect("the pre-flight certifies");

    let mut reservations = Reservations::new();
    reservations
        .take(ALPHA, ReservationKind::Dispatch)
        .expect("a provisional reservation is held");
    let mut invocations = InvocationLedger::new();
    let invocation = crate::runner::InvocationId::probe(crate::runner::ProbeTarget::Shell, 11)
        .expect("an invocation identity");
    invocations
        .register(&invocation)
        .expect("and one invocation is running");
    assert!(
        !reservations.is_empty() && invocations.running().len() == 1,
        "the ledgers must be non-empty, or this test proves nothing"
    );

    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let mut warnings = Vec::new();
    let mut context = EmitContext {
        clock: &Frozen,
        hooks: &mut hooks,
        inputs: fixture.inputs(),
        reservations: &mut reservations,
        invocations: &mut invocations,
        warnings: &mut warnings,
    };
    let error = run_resumed(certified, &mut context, &incarnation)
        .expect_err("the injected append error ends the command");
    let text = message(&error);
    assert!(
        text.contains(crate::events::log::INJECTED_PREFIX),
        "the report carries the funnel's own error as its cause: {text}"
    );

    assert!(
        reservations.is_empty(),
        "obligation (2): whatever reservation was held is cancelled"
    );
    assert!(
        reservations.balances(),
        "and the reservation ledger balances — taken once, cancelled once"
    );
    assert_eq!(
        invocations.cancelled(),
        1,
        "obligation (3): every still-running invocation is cancelled"
    );
    assert!(
        invocations.running().is_empty() && invocations.balances(),
        "and the invocation ledger balances: no entry is left running"
    );
}

fn real_preflight<'a>(
    runner: &'a dyn Runner,
    adapters: &'a StubAdapters,
    fixture: &Fixture,
) -> RunPreflight<'a> {
    RunPreflight::new(
        runner,
        adapters,
        ShellKind::Bash,
        &fixture.repo_root,
        fixture.started.probed_agents.clone(),
    )
}

#[test]
fn resume_refuses_by_preflight_probe_when_shell_or_cli_fails_before_any_recovery_event() {
    for (tag, program, expected) in [
        ("shell", "bash", "the recorded shell"),
        ("cli", "claude", "the `claude-code` CLI"),
    ] {
        let fixture = Fixture::healthy(&format!("probe-{tag}"));
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let runner = RecordingRunner::failing(program);
        let adapters = StubAdapters;
        let preflight = real_preflight(&runner, &adapters, &fixture);
        let given = Given::healthy(&fixture, &runtime, &preflight);

        let before = fixture.log_bytes();
        let (result, _) = resume(&fixture, &harness, &given);

        let text = message(&result.expect_err("a failing probe refuses"));
        assert!(
            text.contains(expected),
            "the refusal names what did not answer ({tag}): {text}"
        );
        assert!(
            text.contains(IMAGE_REF),
            "and the image it was probed inside ({tag}): {text}"
        );
        assert_eq!(
            fixture.log_bytes(),
            before,
            "a probe refusal precedes every recovery event ({tag})"
        );
        assert!(
            preflight.ledgers_balance(),
            "every probe invocation is settled and every slot released ({tag}); still running: \
             {:?}",
            preflight.running()
        );
        let programs: Vec<String> = runner
            .requests()
            .into_iter()
            .map(|request| request.command.program)
            .collect();
        if tag == "shell" {
            assert_eq!(
                programs,
                vec!["bash".to_owned()],
                "no agent is probed after the shell fails"
            );
        } else {
            assert_eq!(
                programs,
                vec!["bash".to_owned(), "claude".to_owned()],
                "the shell probe runs first and the agent probe second"
            );
        }
    }
}

#[test]
fn ledgers_empty_after_resume() {
    let fixture = Fixture::healthy("ledgers-empty");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let runner = RecordingRunner::default();
    let adapters = StubAdapters;
    let preflight = real_preflight(&runner, &adapters, &fixture);
    let given = Given::healthy(&fixture, &runtime, &preflight);

    let (result, _) = resume(&fixture, &harness, &given);
    result.expect("the healthy resume completes");

    assert!(
        preflight.ledgers_balance(),
        "R3 and R4 balance at the end of the pre-flight"
    );
    assert!(
        preflight.running().is_empty(),
        "no invocation is still registered as running: {:?}",
        preflight.running()
    );
    let roles: Vec<(String, bool)> = runner
        .requests()
        .into_iter()
        .map(|request| {
            (
                request.command.program.clone(),
                crate::engine::topology::identity::is_slotted(&request.invocation),
            )
        })
        .collect();
    assert_eq!(
        roles,
        vec![("bash".to_owned(), false), ("claude".to_owned(), true)],
        "the shell probe is non-slotted and the agent probe is slotted"
    );
    assert!(crate::engine::topology::identity::Reservations::new().is_empty());
    assert!(crate::engine::topology::identity::SlotAssertion::new().is_empty());
    assert!(crate::engine::topology::identity::InvocationLedger::new().balances());
}

struct ProbeContainerRunner<'a> {
    runtime: &'a dyn ContainerRuntime,
    private_root: PathBuf,
    run_dir: PathBuf,
    repo_key: String,
    incarnation: String,
    policy_digest: String,
    failing: String,
}

impl Runner for ProbeContainerRunner<'_> {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        use crate::runner::container::intent::{ContainerIntent, ContainerName};
        use crate::runner::container::runtime::CreateSpec;
        use crate::runner::container::{
            GitViewRequest, NoHooks as ContainerNoHooks, launch, release,
        };

        let name = ContainerName::new(
            &self.repo_key,
            RUN_ID,
            &self.incarnation,
            &request.invocation,
        )?;
        let intent = ContainerIntent::new(
            RUN_ID.to_owned(),
            &self.run_dir,
            self.incarnation.clone(),
            self.repo_key.clone(),
            request.invocation.render(),
            self.policy_digest.clone(),
        );
        let plan = crate::runner::container::LaunchPlan {
            private_root: self.private_root.clone(),
            name: name.clone(),
            invocation: request.invocation.clone(),
            intent: intent.clone(),
            spec: CreateSpec {
                name: name.as_str().to_owned(),
                image_id: IMAGE_ID.to_owned(),
                labels: intent.labels(&self.private_root),
                mounts: Vec::new(),
                env: Vec::new(),
                command: std::iter::once(request.command.program.clone())
                    .chain(request.command.args.iter().cloned())
                    .collect(),
                workdir: Some("/".to_owned()),
                read_only_root: true,
            },
            view: GitViewRequest {
                path: crate::runner::container::exec::view_dir(&self.private_root, &name),
                workspace: request.workspace.clone(),
                head: None,
            },
        };
        let mut hooks = ContainerNoHooks;
        let view = DisposableDirView::new(ContainerTrace::off());
        let launched = launch(&mut hooks, self.runtime, &view, &plan)?;
        let code = if request.command.program == self.failing {
            127
        } else {
            0
        };
        release(
            &mut hooks,
            self.runtime,
            &view,
            &self.private_root,
            &launched,
        )?;
        Ok(ProcessOutput {
            code: Some(code),
            stdout: String::new(),
            stderr: "the recorded shell is not in this image".to_owned(),
            duration: Duration::from_millis(1),
            timed_out: false,
            output_limited: false,
        })
    }
}

#[test]
fn resume_preflight_probe_containers_reclaimed_after_refusal() {
    let fixture = Fixture::healthy("probe-reclaim");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let runner = ProbeContainerRunner {
        runtime: &runtime,
        private_root: fixture.private_root.clone(),
        run_dir: fixture.public(),
        repo_key: fixture.repo_key.as_str().to_owned(),
        incarnation: RESUMER.to_owned(),
        policy_digest: crate::runner::policy::runner_policy_sha256(&fixture.started.runner),
        failing: "bash".to_owned(),
    };
    let adapters = StubAdapters;
    let preflight = real_preflight(&runner, &adapters, &fixture);
    let given = Given::healthy(&fixture, &runtime, &preflight);

    let before = fixture.log_bytes();
    let (result, _) = resume(&fixture, &harness, &given);

    let text = message(&result.expect_err("a shell that fails inside the image refuses"));
    assert!(text.contains("the recorded shell"), "{text}");
    assert_eq!(
        fixture.log_bytes(),
        before,
        "the refusal precedes every recovery event"
    );
    assert!(
        runtime.container_names().is_empty(),
        "every probe container is reclaimed: {:?}",
        runtime.container_names()
    );
    assert!(
        crate::runner::container::list_intents(&fixture.private_root)
            .expect("the namespace scans")
            .is_empty(),
        "and its intent went with it"
    );
    assert!(
        preflight.ledgers_balance(),
        "and the probe invocations are settled: {:?}",
        preflight.running()
    );
}

fn create_ref_entries(harness: &Arc<Mutex<HookHarness>>) -> u32 {
    harness
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .count(
            EffectSiteId::Ref(RefSite::CreateIntegration),
            HookPhase::Before,
        )
}

#[test]
fn kill_after_run_started_creates_integration_ref() {
    let fixture = Fixture::healthy("ref-p78-create");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let committed = fixture.log_bytes();
    assert_eq!(
        given.refs.target(),
        None,
        "the fixture is the P6/P7 prefix: nothing created the ref"
    );

    let (result, _) = resume(&fixture, &harness, &given);
    result.expect("a resume of a run killed before P8 completes");

    assert_eq!(
        given.refs.created(),
        vec![(
            fixture.started.integration_ref.as_str().to_owned(),
            fixture.started.base_sha.as_str().to_owned(),
        )],
        "the ref is created once, at the name and base the record carries"
    );
    assert_eq!(
        create_ref_entries(&harness),
        1,
        "and the funnel was entered exactly once"
    );

    assert_eq!(
        given.refs.log_kinds_at_entries(),
        vec![vec!["run_started".to_owned()]],
        "the ref was created after a recovery event had already been appended"
    );
    assert_eq!(
        given.refs.log_bytes_at_entries(),
        vec![committed.clone()],
        "the log the funnel saw was not byte-identical to the committed prefix"
    );
    let after = fixture.log_bytes();
    assert!(
        after.len() > committed.len()
            && String::from_utf8_lossy(&after).contains("\"run_resumed\""),
        "the resume did not reach (h), so `before any recovery event` proves nothing"
    );
}

#[test]
fn a_resume_adopts_an_integration_ref_already_at_the_recorded_base() {
    {
        let fixture = Fixture::healthy("ref-p78-adopt");
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let mut given = Given::healthy(&fixture, &runtime, &certifies);
        given.refs = RecordingRefs::at(&fixture, fixture.started.base_sha.as_str());

        let (result, _) = resume(&fixture, &harness, &given);
        result.expect("present == base continues");

        assert_eq!(
            create_ref_entries(&harness),
            0,
            "the funnel was entered for a ref that was already at the base"
        );
        assert!(
            given.refs.created().is_empty(),
            "and nothing was created: {:?}",
            given.refs.created()
        );
        assert_eq!(
            given.refs.target().as_deref(),
            Some(fixture.started.base_sha.as_str()),
            "the ref still names the recorded base"
        );
    }

    {
        let fixture = Fixture::healthy("ref-p78-twice");
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let first = harness();
        let (result, _) = resume(&fixture, &first, &given);
        let opened = result.expect("the first resume completes").resumed.epoch;
        assert_eq!(create_ref_entries(&first), 1, "the first resume creates it");

        let second = harness();
        let (result, _) = resume(&fixture, &second, &given);
        let reopened = result.expect("the second resume completes").resumed.epoch;

        assert_eq!(
            create_ref_entries(&second),
            0,
            "the second resume entered `Ref.CreateIntegration` again; `no spend repeats` is not \
             held"
        );
        assert_eq!(
            given.refs.created().len(),
            1,
            "the ref was created twice: {:?}",
            given.refs.created()
        );
        assert!(
            reopened > opened,
            "the second resume did not open an epoch of its own ({opened} then {reopened}), so \
             it never reached the step this test is about"
        );
    }
}

#[test]
fn a_resume_refuses_an_integration_ref_at_another_sha_before_touching_anything() {
    let fixture = Fixture::healthy("ref-p78-elsewhere");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let mut given = Given::healthy(&fixture, &runtime, &certifies);
    let elsewhere = "b".repeat(40);
    given.refs = RecordingRefs::at(&fixture, &elsewhere);

    let committed = fixture.log_bytes();
    let (result, _) = resume(&fixture, &harness, &given);

    let text = message(&result.expect_err("a ref at another commit refuses"));
    assert!(
        text.contains(fixture.started.integration_ref.as_str())
            && text.contains(&elsewhere)
            && text.contains(fixture.started.base_sha.as_str()),
        "the refusal names the ref, where it is, and where the record says it should be: {text}"
    );
    assert_eq!(
        create_ref_entries(&harness),
        0,
        "the funnel was entered for a ref the step must have refused on sight"
    );
    assert!(given.refs.created().is_empty());
    assert_eq!(
        given.refs.target().as_deref(),
        Some(elsewhere.as_str()),
        "the ref was moved to make room for the run"
    );
    assert_eq!(
        fixture.log_bytes(),
        committed,
        "a P7/P8 refusal precedes every recovery event"
    );
}

#[test]
fn a_resume_refuses_a_symbolic_or_checked_out_integration_ref() {
    for (tag, shape, expected) in [
        ("symbolic", RefShape::Symbolic, "it is a symbolic ref"),
        (
            "checked-out",
            RefShape::CheckedOut,
            "it is checked out in the worktree",
        ),
    ] {
        let fixture = Fixture::healthy(&format!("ref-p78-{tag}"));
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let mut given = Given::healthy(&fixture, &runtime, &certifies);
        given.refs = RecordingRefs::shaped(&fixture, shape);

        let committed = fixture.log_bytes();
        let (result, _) = resume(&fixture, &harness, &given);

        let text = message(&result.expect_err("an unpublishable ref refuses"));
        assert!(
            text.contains(expected),
            "the refusal says which shape it found ({tag}): {text}"
        );
        assert!(
            text.contains(fixture.started.integration_ref.as_str()),
            "and names the recorded ref ({tag}): {text}"
        );
        assert_eq!(
            create_ref_entries(&harness),
            0,
            "the funnel ran for an unpublishable ref ({tag})"
        );
        assert_eq!(
            given.refs.target(),
            None,
            "and nothing was written to it ({tag})"
        );
        assert_eq!(
            given.refs.targets_read(),
            0,
            "`assert_publishable` did not refuse first: the target was read for an \
             unpublishable ref ({tag})"
        );
        assert_eq!(
            fixture.log_bytes(),
            committed,
            "a P7/P8 refusal precedes every recovery event ({tag})"
        );
    }
}

#[test]
fn the_p7_p8_step_runs_after_the_refusals_that_bound_it() {
    {
        let fixture = Fixture::build(
            "ref-p78-after-b",
            Damage {
                extra: vec![
                    dispatched(),
                    attempt_started(1),
                    attempt_finished(
                        1,
                        AttemptSettlement::Closed {
                            transition: SettlementTransition::Failed {
                                halts_run: true,
                                reason: "the ladder ran out".to_owned(),
                            },
                            lease: LeaseDisposition::PredictedReleased,
                        },
                    ),
                    run_finished(RunOutcome::Halted, Some(ALPHA)),
                ],
                ..Damage::default()
            },
        );
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let text = message(
            &resume(&fixture, &harness, &given)
                .0
                .expect_err("a finished run does not continue"),
        );
        assert!(text.contains("already finished"), "{text}");
        assert_eq!(
            create_ref_entries(&harness),
            0,
            "(b) refused and the ref was published anyway"
        );
        assert_eq!(given.refs.target(), None);
    }

    {
        let fixture = Fixture::healthy("ref-p78-after-c");
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let runner = RecordingRunner::failing("bash");
        let adapters = StubAdapters;
        let preflight = real_preflight(&runner, &adapters, &fixture);
        let given = Given::healthy(&fixture, &runtime, &preflight);

        let text = message(
            &resume(&fixture, &harness, &given)
                .0
                .expect_err("a failing probe refuses"),
        );
        assert!(text.contains("the recorded shell"), "{text}");
        assert_eq!(
            create_ref_entries(&harness),
            0,
            "(c) refused and the ref was published anyway"
        );
        assert_eq!(given.refs.target(), None);
    }
}

#[test]
#[ignore = "spawned as a subprocess by kill_during_recovery_repeats_recovery"]
fn recovery_kill_child() {
    let repo_root = PathBuf::from(
        std::env::var("UPSTROKE_TEST_KILL_REPO").expect("the parent names the repository"),
    );
    let git_dir = PathBuf::from(
        std::env::var("UPSTROKE_TEST_KILL_GITDIR").expect("the parent names the git dir"),
    );
    let repo_key = RepoKey::v1(&std::fs::canonicalize(&git_dir).expect("the git dir exists"));

    let harness = harness();
    harness
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::Append),
            SubEffectPoint::Written,
            InjectionMode::Kill,
        )
        .expect("the Written point supports a kill");
    let mut hooks = HarnessTopologyHooks::new(harness);
    let runtime = runtime_holding_the_record();
    let liveness = FakeOwnerLiveness::new();
    let view = DisposableDirView::new(ContainerTrace::default());
    let certifies = AlwaysCertifies;
    let incarnation = IncarnationId(RESUMER.to_owned());
    let today = container_selection();
    let refs = RecordingRefs::with_log(
        &rundir::public_dir(&repo_root, RUN_ID).join(rundir::EVENT_LOG),
        RefShape::Direct,
        None,
    );
    let mut warnings = Vec::new();

    let root = RootDerived::derive_with(&repo_root, RUN_ID, None, TOPOLOGY_SCHEMA)
        .expect("(a0) derives in the child");
    let manager = crate::workspace_manager::WorkspaceManager::derive(
        &repo_root,
        root.private_root(),
        RUN_ID,
        RESUMER,
    )
    .expect("the child's repository and private root are real directories");
    let _ = run_recovery_order(
        root,
        &ResumeSeams {
            repo_root: &repo_root,
            worktree_git_dir: &git_dir,
            repo_key: &repo_key,
            incarnation: &incarnation,
            inputs: FrozenInputs {
                plan: plan(),
                normalized_plan_digest: "sha256:aaaa".to_owned(),
            },
            today: &today,
            runtime: &runtime,
            liveness: &liveness,
            view: &view,
            preflight: &certifies,
            refs: &refs,
            manager: &manager,
            clock: &Frozen,
        },
        &mut hooks,
        &mut warnings,
    );
    unreachable!("the kill must have taken this process");
}

#[test]
fn kill_during_recovery_repeats_recovery() {
    let fixture = Fixture::healthy("kill-recovery");
    let before = fixture.log_bytes();

    let exe = std::env::current_exe().expect("the test binary knows where it is");
    let request = RunnerRequest {
        command: CommandSpec {
            program: exe.display().to_string(),
            args: vec![
                "--exact".to_owned(),
                "engine::topology::recover::tests::recovery_kill_child".to_owned(),
                "--ignored".to_owned(),
                "--test-threads".to_owned(),
                "1".to_owned(),
            ],
            env: vec![
                (
                    "UPSTROKE_TEST_KILL_REPO".to_owned(),
                    fixture.repo_root.display().to_string(),
                ),
                (
                    "UPSTROKE_TEST_KILL_GITDIR".to_owned(),
                    fixture.git_dir.display().to_string(),
                ),
            ],
            stdin: Vec::new(),
        },
        workspace: fixture.repo_root.clone(),
        role: crate::runner::ExecutionRole::Gate,
        timeout: Duration::from_secs(120),
        agent: None,
        invocation: crate::runner::InvocationId::probe(crate::runner::ProbeTarget::Shell, 7)
            .expect("a probe identity for the spawned child"),
    };
    let output = crate::runner::host::HostRunner::new()
        .run(&request)
        .expect("the child runs");
    assert_ne!(
        output.code,
        Some(0),
        "the child must have died rather than finished: {output:?}"
    );
    assert!(
        !output.stdout.contains("test result:"),
        "the child printed a test result, so it finished rather than dying: {}",
        output.stdout
    );
    assert!(
        !output
            .stdout
            .contains("the kill must have taken this process"),
        "the child reached its `unreachable!`, so the injection did not kill it: {}",
        output.stdout
    );

    let after_kill = fixture.log_bytes();
    assert!(
        after_kill.len() > before.len(),
        "the kill is at `Written`, after the bytes reached the file"
    );

    const AFTER_THE_KILL: &str = "01KZTKILL00000000000000004";
    let husk = plant_husk(&fixture, AFTER_THE_KILL, false);
    assert!(husk.private.exists() && husk.public.exists());

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("the next process recovers");

    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    for site in [
        EffectSiteId::Lock(LockSite::AcquireWorktree),
        EffectSiteId::Lock(LockSite::AcquireRun),
        EffectSiteId::Event(EventSite::OpenLog),
        EffectSiteId::Event(EventSite::ProvePrefixStable),
        EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
        EffectSiteId::RunDir(RunDirSite::RemovePublicHusk),
    ] {
        assert!(
            seen.observed(site, HookPhase::Before),
            "the repeat runs `{site}` again — a kill repeats from (a0), it does not resume from a \
             checkpoint"
        );
    }
    drop(seen);
    assert!(
        !husk.private.exists() && !husk.public.exists(),
        "and the repeat's census reclaimed the husk it found, both halves"
    );
    assert_eq!(
        recovered.resumed.epoch, 2,
        "the killed process's `run_resumed` line survived, so this resume opens the epoch after it"
    );
}

#[test]
fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {
    const ENTRY: &str = "into_log_and_fold(";
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let test_modules = {
        let mut all = Vec::new();
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("src is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    all.push(path);
                }
            }
        }
        crate::effects::census_domain::whole_file_test_modules(&src, &all, 13)
    };
    let mut stack = vec![src.clone()];
    let mut callers: Vec<(String, usize)> = Vec::new();
    let mut regions: Vec<(String, usize, usize)> = Vec::new();
    let mut scanned = 0_usize;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("src is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let relative = path
                .strip_prefix(&src)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if !relative.starts_with("engine/topology") {
                continue;
            }
            if test_modules.contains(&path) {
                continue;
            }
            scanned += 1;
            let source = std::fs::read_to_string(&path).expect("a source file");
            let production = crate::effects::production_code(&source);
            let production = production.as_str();
            regions.push((relative.clone(), production.len(), source.len()));

            let count = production
                .match_indices(ENTRY)
                .filter(|(at, _)| !production[..*at].trim_end().ends_with("fn"))
                .count();
            if count > 0 {
                callers.push((relative, count));
            }
        }
    }
    callers.sort();

    assert!(
        scanned >= 4,
        "the walk found only {scanned} topology sources, so its zero counts would prove nothing"
    );
    for (file, region, whole) in &regions {
        assert!(
            *region * 10 > *whole,
            "{file}'s production region is {region} of {whole} bytes. A census over a fraction \
             of a file reports zero for the part it never read — this is `PR4-CENSUS-COMMENT-ORACLE`, \
             and it is how the driver was scanned at 4.7% while reading as a pass"
        );
    }
    assert_eq!(
        callers,
        vec![("engine/topology/recover.rs".to_owned(), 1)],
        "a proven prefix becomes an append handle in exactly one production place in the topology \
         engine, and that place is `BarrierHeld::from`"
    );
}

#[test]
fn the_recovery_order_performs_every_step_the_packet_names() {
    let fixture = Fixture::healthy("every-step");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);
    let recovered = outcome.expect("the healthy resume completes");

    let owed: Vec<RecoveryStep> = RecoveryStep::ALL
        .into_iter()
        .filter(|step| step.performer() == Performer::ThisOrder)
        .collect();

    let mut performed = recovered.steps.clone();
    performed.sort_unstable();
    let mut expected = owed.clone();
    expected.sort_unstable();
    assert_eq!(
        performed,
        expected,
        "the order performed {:?} and the packet names {:?} for it; a step in \
         the second list and not the first is a step no code performs, which \
         is the defect this test exists for",
        recovered
            .steps
            .iter()
            .map(|step| step.label())
            .collect::<Vec<_>>(),
        owed.iter().map(|step| step.label()).collect::<Vec<_>>()
    );

    let packet_order: Vec<RecoveryStep> = owed
        .iter()
        .copied()
        .filter(|step| step.position_override().is_none())
        .collect();
    let performed_order: Vec<RecoveryStep> = recovered
        .steps
        .iter()
        .copied()
        .filter(|step| step.position_override().is_none())
        .collect();
    assert_eq!(
        performed_order, packet_order,
        "the steps the packet alone positions ran out of order; a step that \
         must move carries the clause that moves it"
    );

    let at = |step: RecoveryStep| {
        recovered
            .steps
            .iter()
            .position(|performed| *performed == step)
            .expect("every owed step was performed")
    };
    assert!(
        at(RecoveryStep::D) < at(RecoveryStep::F) && at(RecoveryStep::E) < at(RecoveryStep::F),
        "(f)'s converging half appends, so it belongs with (d) and (e) rather \
         than before them. Its refusing half is unmarked because a refusal ends \
         the command and records no step"
    );
    assert!(
        at(RecoveryStep::F) < at(RecoveryStep::G) && at(RecoveryStep::G) < at(RecoveryStep::H),
        "and it stays in the packet's position: after (e), before (g) and (h)"
    );
}

#[test]
fn the_transcribed_recovery_steps_are_the_packets_eleven() {
    assert_eq!(
        RecoveryStep::ALL
            .iter()
            .map(|step| step.label())
            .collect::<Vec<_>>(),
        vec!["a0", "a", "a1", "b", "c", "d", "e", "f", "g", "h", "i"],
        "transcribed from `decisions.sequential_substrate.recovery_order`"
    );
    assert_eq!(
        RecoveryStep::ALL
            .iter()
            .filter(|step| step.performer() != Performer::ThisOrder)
            .map(|step| (step.label(), step.performer()))
            .collect::<Vec<_>>(),
        vec![("a0", Performer::CallerBefore), ("i", Performer::LoopAfter)],
        "exactly two steps are delegated, and each is delegated to a named \
         performer with a reason: a step whose performer nobody states is \
         indistinguishable from a step nobody performs"
    );
}

#[test]
fn resume_recreates_an_open_no_attempt_worktree_at_its_base() {
    let fixture = Fixture::build(
        "step-g-recreate",
        Damage {
            open_generation: true,
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let manager = fixture.manager();
    let slot = crate::engine::topology::dispatch::task_slot(ALPHA, GEN);
    let worktree = manager.slot_path(&slot);
    assert!(
        !worktree.exists(),
        "the fixture leaves the generation open with no worktree, which is what \
         the kill leaves and what (g) has to answer"
    );

    let (outcome, _) = resume(&fixture, &harness, &given);
    let recovered = outcome.expect("the resume completes");

    assert_eq!(
        recovered
            .recreated
            .iter()
            .map(|(key, generation, _)| (*key, *generation))
            .collect::<Vec<_>>(),
        vec![(ALPHA, GEN)],
        "(g) acts on exactly the open generation, and on nothing else"
    );
    assert!(
        worktree.exists(),
        "(g) recreates the worktree the generation records; without it the \
         resumed loop has a dispatched generation whose checkout does not exist"
    );
    assert_eq!(
        crate::workspace_manager::fixture::git(&worktree, &["rev-parse", "HEAD"]),
        fixture.base_sha.0,
        "at its **base** — `recovery_order` (g) says where, and a worktree cut \
         anywhere else silently changes what the next attempt starts from"
    );
}

#[test]
fn a_repair_generation_cannot_reach_step_g_in_this_slice() {
    let repair = {
        let TopologyEventBody::TaskDispatched { mut data } = dispatched() else {
            unreachable!("`dispatched` builds a `TaskDispatched`")
        };
        data.lease = LeaseGrant::InheritedLineage { root: TaskKey(1) };
        TopologyEventBody::TaskDispatched { data }
    };
    let fixture = Fixture::build(
        "step-g-repair",
        Damage {
            extra: vec![repair],
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);
    let text = outcome
        .expect_err("the fold refuses the shape before any step sees it")
        .to_string();
    assert!(
        text.contains("an ordinary task belongs to no lineage and cannot inherit one's lease"),
        "the refusal is the fold's, at the replay, and not step (g)'s: {text}"
    );
    assert!(
        text.contains("stable-prefix barrier"),
        "and it lands at the barrier, so nothing fold-derived was acted on: {text}"
    );

    let registry = TaskRegistry::originals_with_agents(
        &fixture.plan,
        &fixture.started.registry_record(),
        &fixture.started.probed_agents,
    )
    .expect("the fixture's plan registers");
    assert!(
        !registry.entries().is_empty(),
        "a registry with no entries would satisfy the next assertion by having \
         nothing to check"
    );
    assert!(
        registry
            .entries()
            .iter()
            .all(|entry| entry.lineage.is_none()),
        "no entry this slice can build descends from a lineage, so no \
         `task_dispatched` carrying an inherited lease can be valid"
    );
}

#[test]
fn the_recovery_order_hands_the_run_on_rather_than_dropping_it() {
    let fixture = Fixture::healthy("hand-on");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (recovered, handle) = outcome.expect("the healthy resume completes");

    assert_eq!(
        handle.started.run_id, RUN_ID,
        "the handle names the run the order recovered"
    );
    assert!(
        !handle.fold.is_poisoned(),
        "and hands on a fold that may still be transitioned"
    );
    assert_eq!(
        handle.fold.epoch().map(|epoch| epoch.0),
        Some(recovered.resumed.epoch),
        "the fold in the handle is the one `(h)` incremented, not a second \
         derivation of the same log — a rebuilt fold is a rule that can \
         disagree with the one the barrier proved"
    );

    let contested = rundir::RunLock::acquire(&rundir::public_dir(&fixture.repo_root, RUN_ID));
    assert!(
        contested.is_err(),
        "the run lock is still held by the handle; a loop that had to retake \
         it would be racing itself"
    );

    drop(handle);
    rundir::RunLock::acquire(&rundir::public_dir(&fixture.repo_root, RUN_ID))
        .expect("dropping the handle releases the run lock");
}

#[test]
fn the_driver_takes_over_from_the_recovery_order_and_steps() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::healthy("driver-steps");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::default();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let kinds_before = durable_kinds(&fixture);
    let progress = run
        .step(&seams, &mut hooks)
        .expect("the branch performs its first four clauses");

    let Progress::Settled {
        key,
        accepted,
        spent_attempt,
    } = progress
    else {
        panic!("the ready-dispatch branch did not run an attempt: {progress:?}");
    };
    assert_eq!(key, TaskKey(0));

    assert!(
        !accepted,
        "a worker that edited nothing was judged acceptable, which means the \
         cheap rungs of the verification ladder did not run"
    );

    assert_eq!(
        durable_kinds(&fixture),
        {
            let mut expected = kinds_before.clone();
            expected.push("task_dispatched".to_owned());
            expected.push("attempt_started".to_owned());
            expected.push("attempt_finished".to_owned());
            expected
        },
        "the whole branch, in order: the dispatch, the attempt, the settlement"
    );

    assert!(
        spent_attempt,
        "an attempt whose worker ran and produced a diff to judge did not spend \
         one of its rung's attempts"
    );

    assert!(
        !runner.requests().is_empty(),
        "the attempt appended `attempt_started` and never spawned anything"
    );

    let recorded = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::TaskDispatched { data } => Some(data.lease),
            _ => None,
        })
        .expect("the dispatch is durable");
    let LeaseGrant::Predicted { paths: recorded } = recorded else {
        panic!("an ordinary dispatch takes a predicted lease")
    };
    assert_eq!(
        Some(recorded),
        run.fold().predicted_region(ALPHA),
        "the region in the log is the one the fold admitted on. Compared \
         against the fold rather than against a literal, because a literal \
         would agree with whichever derivation this test happened to use"
    );

    assert_eq!(
        run.entitlements_held(),
        0,
        "the dispatch reservation was converted at `task_dispatched`, not left \
         held across the refusal. A leaked entitlement here is
         `PR7-INTEGRATION-NO-ENTITLEMENT`'s failure wearing a different hat: at \
         the only width production creates, one held entitlement is a full \
         pipeline and nothing is ever selected again"
    );
}

#[test]
fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::healthy("driver-promotes");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = TracedHooks::new(&harness);
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let kinds_before = durable_kinds(&fixture);
    let progress = run
        .step(&seams, &mut hooks)
        .expect("the branch performs its first four clauses");

    let Progress::Settled { key, accepted, .. } = progress else {
        panic!("the ready-dispatch branch did not run an attempt: {progress:?}");
    };
    assert_eq!(key, TaskKey(0));
    assert!(
        accepted,
        "the worker left a change and the plan configures no gates or reviewers, \
     so nothing could reject it"
    );

    assert_eq!(
        durable_kinds(&fixture),
        {
            let mut expected = kinds_before.clone();
            expected.push("task_dispatched".to_owned());
            expected.push("attempt_started".to_owned());
            expected.push("candidate_prepared".to_owned());
            expected.push("task_candidate_created".to_owned());
            expected
        },
        "the whole branch, in the order the packet specifies"
    );

    assert_eq!(
        hooks.timeline.order(&[
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            EffectSiteId::Ref(RefSite::PinCandidatePrepared),
            EffectSiteId::Event(EventSite::Append),
            EffectSiteId::Ref(RefSite::CreateCandidates),
            EffectSiteId::Ref(RefSite::DeleteCandidatePin),
            EffectSiteId::Worktree(WorktreeSite::Remove),
        ]),
        vec![
            "Event.Append".to_owned(),
            "Event.Append".to_owned(),
            "Object.CandidateCommitTree".to_owned(),
            "Ref.PinCandidatePrepared".to_owned(),
            "Event.Append".to_owned(),
            "Ref.CreateCandidates".to_owned(),
            "Event.Append".to_owned(),
            "Ref.DeleteCandidatePin".to_owned(),
            "Worktree.Remove".to_owned(),
        ],
        "the driver's candidate sequence, as one observed order over both families"
    );
}

#[test]
fn a_runs_spend_is_the_same_live_as_on_replay() {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::{Ceiling, Spend};

    let fixture = Fixture::healthy("spend-parity");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    run.step(&seams, &mut hooks)
        .expect("the accepted attempt runs the candidate sequence");

    let live = run.spend().run_total();

    let events = TopologyFold::parse_log(&fixture.log_bytes()).expect("the log parses");
    let replayed = Spend::replay(&events).run_total();

    assert!(
        live > 0.0,
        "the fixture priced nothing, so this asserts two zeroes and proves \
         nothing: give the scaffold adapter a cost"
    );
    assert!(
        (live - replayed).abs() < 1e-9,
        "a live run and a replay of its own log price it differently: live \
         {live}, replay {replayed}. A resumed run would refuse work it could \
         afford, or buy work it could not"
    );
}

#[test]
fn the_driver_settles_an_outage_from_the_folds_deferral_count() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "driver-outage",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Deferred {
                            defers: 1,
                            reason: "the pool was exhausted".to_owned(),
                        },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::rate_limiting();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run
        .step(&seams, &mut hooks)
        .expect("the branch settles an outage");

    let Progress::Settled {
        accepted,
        spent_attempt,
        ..
    } = progress
    else {
        panic!("the ready-dispatch branch did not settle: {progress:?}");
    };
    assert!(
        !accepted,
        "a rate-limited worker produced nothing to accept"
    );

    assert!(
        !spent_attempt,
        "an outage spent one of the rung's attempts, which is the cell \
         `ladder::spends_allowance` exists to get right"
    );

    let settlements: Vec<u32> = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => match data.settlement {
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Deferred { defers, .. },
                    ..
                } => Some(defers),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        settlements,
        vec![1, 2],
        "the second deferral did not continue the first. A driver reading a \
         process-local zero records `1` here and defers forever"
    );
}

#[test]
fn the_driver_parks_an_attempt_with_the_question_it_raised() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::healthy("driver-parks");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::asking();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run.step(&seams, &mut hooks).expect("the branch parks");

    let Progress::Settled {
        accepted,
        spent_attempt,
        ..
    } = progress
    else {
        panic!("the ready-dispatch branch did not settle: {progress:?}");
    };
    assert!(
        !accepted,
        "an agent that asked a question produced no verdict"
    );

    assert!(
        !spent_attempt,
        "a park spent one of the rung's attempts, which is the cell the \
         allowance fix exists for"
    );

    let parked = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => match data.settlement {
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Parked { question },
                    ..
                } => Some(question),
                _ => None,
            },
            _ => None,
        })
        .expect("a parking settlement is durable");

    assert_eq!(parked.id, crate::ir::QuestionId("q-park-fixed".to_owned()));
    assert_eq!(parked.key, TaskKey(0));
    assert_eq!(parked.kind, crate::ir::QuestionKind::Clarify);

    assert!(
        parked.context.contains("stopped and asked for a decision"),
        "the context is not `question_context`'s: {}",
        parked.context
    );
    assert!(
        parked
            .context
            .contains("two incompatible \\\n                      formats")
            || parked.context.contains("incompatible"),
        "the agent's own words are not quoted back: {}",
        parked.context
    );
    assert_eq!(
        parked.options,
        crate::engine::coordinator::question_options(crate::ir::QuestionKind::Clarify),
        "the options are not `question_options`'s"
    );
}

#[test]
fn the_driver_refuses_a_tree_a_filter_has_transformed() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::healthy("driver-filtered");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::filtering();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run
        .step(&seams, &mut hooks)
        .expect("the branch settles the refusal");

    let Progress::Settled { accepted, .. } = progress else {
        panic!("the ready-dispatch branch did not settle: {progress:?}");
    };
    assert!(
        !accepted,
        "a tree a filter has transformed was accepted, so the ladder's third \
         cheap rung did not run"
    );

    let failure = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => data.record.failure.clone(),
            _ => None,
        })
        .expect("the settlement records a failure");

    assert_eq!(failure.kind, crate::ladder::FailureKind::ReviewInputOpaque);
    assert_eq!(failure.origin, crate::ladder::FailureOrigin::Reviewer);
    assert!(
        failure.reason.contains("filter"),
        "the reason is not the policy's: {}",
        failure.reason
    );
}

#[test]
fn the_retaining_incarnation_retries_in_place() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    const RETRY_POOL: &str = "the-retrying-agents-pool";

    let fixture = Fixture::healthy("driver-retries");
    let caps = vec![(
        crate::engine::topology::scaffold::AGENT.to_owned(),
        crate::agent::Caps {
            version: "1.2.3".to_owned(),
            json_output: true,
            session_resume: true,
            cost_reporting: true,
            read_only_mode: true,
            acp: false,
            model_list: false,
        },
    )];
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let pools = vec![crate::capacity::Pool::discovered(
        RETRY_POOL,
        crate::capacity::PoolKind::SubscriptionWindow,
        crate::engine::topology::scaffold::AGENT,
        vec![crate::capacity::Source::Signals],
    )];
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &pools,
        caps: &caps,
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let first = run
        .step(&seams, &mut hooks)
        .expect("the first attempt settles");
    let Progress::Settled { accepted, .. } = first else {
        panic!("the first iteration did not settle: {first:?}");
    };
    assert!(!accepted, "an agent error is not an acceptable attempt");

    let retained = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => Some(data.settlement),
            _ => None,
        })
        .next_back()
        .expect("the first attempt settled");
    assert!(
        matches!(retained, AttemptSettlement::Retained { .. }),
        "the generation did not retain its session: {retained:?}"
    );

    let second = run
        .step(&seams, &mut hooks)
        .expect("the retry runs in the retained generation");
    let Progress::Settled { key, .. } = second else {
        panic!("the second iteration did not run a retry: {second:?}");
    };
    assert_eq!(key, TaskKey(0));

    let starts: Vec<(u32, bool, Option<String>)> = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptStarted { data } => Some((
                data.attempt.0,
                data.resume_session.is_some(),
                data.pool.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        starts,
        vec![
            (1, false, Some(RETRY_POOL.to_owned())),
            (2, true, Some(RETRY_POOL.to_owned())),
        ],
        "the retry is not the same generation's second attempt on a resumed session, drawing on \
         the pool the assembler resolves for its agent. A `None` here is the ledger and the plan \
         disagreeing about which subscription one attempt drained"
    );
    assert_eq!(
        durable_kinds(&fixture)
            .iter()
            .filter(|kind| *kind == "task_dispatched")
            .count(),
        1,
        "the retry opened a fresh generation instead of continuing the retained one"
    );

    let briefed = runner
        .requests()
        .iter()
        .filter(|request| request.role == crate::runner::ExecutionRole::Implement)
        .filter(|request| {
            request
                .command
                .args
                .iter()
                .any(|arg| arg.contains("agent error"))
        })
        .count();
    assert_eq!(
        briefed, 1,
        "exactly one of the two worker prompts should carry the previous \
         attempt's failure, and it is the second"
    );

    assert!(
        run.invocations_balance(),
        "the invocation ledger does not balance, so some process was \
         registered and never settled"
    );

    let resumed = runner
        .requests()
        .iter()
        .filter(|request| request.role == crate::runner::ExecutionRole::Implement)
        .filter(|request| request.command.args.iter().any(|arg| arg == "--resume"))
        .count();
    assert_eq!(
        resumed, 1,
        "exactly one of the two worker invocations should carry a session to \
         resume, and it is the second"
    );
}

#[test]
fn a_refused_step_leaves_no_entitlement_held() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "refused-entitlement",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(
        handle,
        fixture.inputs(),
        Ceiling {
            run_usd: Some(0.000_001),
            task_usd: None,
        },
    );
    assert!(
        run.spend().run_total() > 0.000_001,
        "the seeded attempt must cost more than the ceiling, or this test drives \
         an ordinary dispatch and asserts nothing about a refusal"
    );

    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run.step(&seams, &mut hooks).expect("the ceiling refuses");
    assert!(
        matches!(progress, Progress::BudgetExceeded),
        "the seeded spend did not breach the ceiling: {progress:?}"
    );
    assert!(
        !run.holds_entitlement(),
        "the refused step is still holding a pipeline entitlement. At \
         `max_parallel = 1` that is the whole pipeline, held by a step that \
         did nothing"
    );
}

#[test]
fn a_retried_worker_is_told_what_the_last_attempt_failed_on() {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "driver-brief",
        Damage {
            two_tier: true,
            ..Damage::default()
        },
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    run.step(&seams, &mut hooks)
        .expect("the first attempt settles");
    run.step(&seams, &mut hooks)
        .expect("the second attempt settles");

    let prompts: Vec<String> = runner
        .requests()
        .into_iter()
        .filter(|request| request.role == crate::runner::ExecutionRole::Implement)
        .map(|request| String::from_utf8_lossy(&request.command.stdin).into_owned())
        .collect();
    assert!(
        prompts.len() >= 2,
        "the fixture ran {} implementer(s); this test needs a second attempt to \
         have a prompt at all",
        prompts.len()
    );
    assert!(
        prompts[1].len() > prompts[0].len(),
        "the second worker's prompt is no longer than the first's, so nothing \
         was carried forward:\n--- first ---\n{}\n--- second ---\n{}",
        prompts[0],
        prompts[1]
    );

    let settled: Vec<serde_json::Value> = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => {
                serde_json::to_value(data.record.failure.as_ref()?).ok()
            }
            _ => None,
        })
        .collect();
    assert!(
        !settled.is_empty(),
        "no failed settlement in the log, so the assertion below is vacuous"
    );
    assert!(
        settled
            .iter()
            .any(|failure| failure["detail"].as_str().is_some_and(|d| !d.is_empty())),
        "every failed attempt this run settled carries `detail: null`, so the \
         schema-4 driver is asking for the legacy carrier and §11.4's feedback \
         is durable nowhere: {settled:?}"
    );
}

#[derive(Clone, Default)]
struct Timeline(Arc<Mutex<Vec<String>>>);

impl Timeline {
    fn push(&self, site: EffectSiteId, phase: HookPhase) {
        if phase != HookPhase::Before {
            return;
        }
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(site.to_string());
    }

    fn order(&self, of_interest: &[EffectSiteId]) -> Vec<String> {
        let names: Vec<String> = of_interest.iter().map(ToString::to_string).collect();
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|seen| names.contains(seen))
            .cloned()
            .collect()
    }
}

struct TracedEffects {
    inner: crate::workspace_manager::HarnessEffects,
    timeline: Timeline,
}

impl crate::workspace_manager::EffectHooks for TracedEffects {
    fn phase(
        &mut self,
        site: EffectSiteId,
        phase: HookPhase,
    ) -> crate::topology::effects::Injection {
        let answered = self.inner.phase(site, phase);
        self.timeline.push(site, phase);
        answered
    }

    fn durability_ledger(&self) -> crate::util::DurabilityLedger {
        self.inner.durability_ledger()
    }

    fn refusal_cause(&self) -> Option<String> {
        self.inner.refusal_cause()
    }
}

struct TracedEvents {
    inner: crate::events::log::HarnessEventHooks,
    timeline: Timeline,
}

impl crate::events::log::EventHooks for TracedEvents {
    fn phase(&mut self, site: crate::topology::effects::EventSite, phase: HookPhase) {
        self.inner.phase(site, phase);
        self.timeline.push(EffectSiteId::Event(site), phase);
    }
}

struct TracedHooks {
    effects: TracedEffects,
    events: TracedEvents,
    rest: HarnessTopologyHooks,
    timeline: Timeline,
}

impl TracedHooks {
    fn new(harness: &Arc<Mutex<HookHarness>>) -> Self {
        let timeline = Timeline::default();
        Self {
            effects: TracedEffects {
                inner: crate::workspace_manager::HarnessEffects::new(Arc::clone(harness)),
                timeline: timeline.clone(),
            },
            events: TracedEvents {
                inner: crate::events::log::HarnessEventHooks::new(Arc::clone(harness)),
                timeline: timeline.clone(),
            },
            rest: HarnessTopologyHooks::new(Arc::clone(harness)),
            timeline,
        }
    }
}

impl TopologyHooks for TracedHooks {
    fn effects(&mut self) -> &mut dyn crate::workspace_manager::EffectHooks {
        &mut self.effects
    }

    fn rundir(&mut self) -> &mut dyn crate::rundir::RunDirHooks {
        self.rest.rundir()
    }

    fn events(&mut self) -> &mut dyn crate::events::log::EventHooks {
        &mut self.events
    }

    fn container(&mut self) -> &mut dyn crate::runner::container::ContainerHooks {
        self.rest.container()
    }

    fn spawn(&mut self) -> &mut dyn crate::agent::proc::SpawnHooks {
        self.rest.spawn()
    }
}

#[test]
fn the_driver_escalates_onto_the_rung_above() {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "driver-escalates",
        Damage {
            two_tier: true,
            ..Damage::default()
        },
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    run.step(&seams, &mut hooks)
        .expect("the first attempt settles");

    let escalated = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => match data.settlement {
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Escalated { rung },
                    ..
                } => Some(rung),
                _ => None,
            },
            _ => None,
        })
        .expect("the exhausted rung escalates");

    assert_eq!(
        escalated, 1,
        "the driver recorded an escalation onto rung {escalated}, the rung it is \
         leaving. The fold assigns `task.rung` from this number and resets the \
         allowance, so the task is selected again at the same tier and loops \
         forever — never reaching the tier its chain escalated it to"
    );

    run.step(&seams, &mut hooks)
        .expect("the second attempt settles");
    let ran_at = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptStarted { data } => Some(data.binding.model.clone()),
            _ => None,
        })
        .next_back()
        .expect("the driver started a second attempt");
    assert_eq!(
        ran_at, "claude-fable-5",
        "the escalated task ran at {ran_at}, which is rung 0's model"
    );

    run.step(&seams, &mut hooks)
        .expect("the exhausted chain settles");
    let parked = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => match data.settlement {
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Parked { question },
                    ..
                } => Some(question),
                _ => None,
            },
            _ => None,
        })
        .expect("the exhausted chain parks a question");

    assert!(
        parked.context.contains("2 attempt(s) across 2 rung(s)"),
        "the human is told the wrong history of this task. Two attempts across \
         two rungs failed; the question says:\n{}",
        parked.context
    );
}

#[test]
fn the_driver_dispatches_at_the_rung_the_log_records() {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "driver-rung",
        Damage {
            two_tier: true,
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Escalated { rung: 1 },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    run.step(&seams, &mut hooks).expect("the attempt settles");

    let ran_at = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptStarted { data } => Some(data.binding.model.clone()),
            _ => None,
        })
        .next_back()
        .expect("the driver started an attempt");

    assert_eq!(
        ran_at, "claude-fable-5",
        "the task escalated onto rung 1 and the driver ran it at {ran_at}, which \
         is rung 0's model. An escalated task dispatched at rung 0 never reaches \
         the tier its chain escalated it to, and the only symptom is a task that \
         never gets better"
    );
}

#[test]
fn a_crash_does_not_erase_what_the_last_attempt_was_told_to_fix() {
    const TAIL: &str = "error[E0308]: mismatched types\n  --> src/alpha.rs:12:9\n   \
                        expected `u32`, found `&str`";

    let fixture = Fixture::build(
        "driver-brief-resume",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished_failing(
                    1,
                    crate::ladder::FailureKind::GateFailed,
                    "gate `cargo test` failed: 1 failed",
                    TAIL,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let runner = RecordingRunner::editing();
    let prompts = drive_one_attempt(&fixture, &runner);

    assert_eq!(
        prompts.len(),
        1,
        "the resumed run dispatched {} implementer(s); this test needs exactly \
         the one the log entitles it to",
        prompts.len()
    );
    assert!(
        prompts[0].contains(TAIL),
        "the retry after the crash was not told what the gate printed. §11.4 \
         sends the gate log back to the same rung, and this prompt carries none \
         of it:\n--- prompt ---\n{}",
        prompts[0]
    );
}

#[test]
fn an_escalation_after_a_crash_carries_the_accumulated_feedback() {
    const FIRST_SUMMARY: &str = "review failed: the parser accepts a trailing comma";
    const SECOND_SUMMARY: &str = "review failed: the empty list still panics";
    const SECOND_DETAIL: &str = "- reject a trailing comma in `parse_list`\n\
                                 - the empty list must round-trip";

    let fixture = Fixture::build(
        "driver-brief-escalate",
        Damage {
            deep_ladder: true,
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished_failing(
                    1,
                    crate::ladder::FailureKind::ReviewFailed,
                    FIRST_SUMMARY,
                    "- reject a trailing comma in `parse_list`",
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
                in_generation(GenerationId(1), dispatched()),
                in_generation(GenerationId(1), attempt_started(1)),
                in_generation(
                    GenerationId(1),
                    attempt_finished_failing(
                        1,
                        crate::ladder::FailureKind::ReviewFailed,
                        SECOND_SUMMARY,
                        SECOND_DETAIL,
                        AttemptSettlement::Closed {
                            transition: SettlementTransition::Escalated { rung: 1 },
                            lease: LeaseDisposition::PredictedReleased,
                        },
                    ),
                ),
            ],
            ..Damage::default()
        },
    );

    let runner = RecordingRunner::editing();
    let prompts = drive_one_attempt(&fixture, &runner);
    assert_eq!(
        prompts.len(),
        1,
        "the resumed run dispatched {} implementer(s); this test needs the \
         escalation the log entitles it to",
        prompts.len()
    );
    let prompt = &prompts[0];

    for summary in [FIRST_SUMMARY, SECOND_SUMMARY] {
        assert!(
            prompt.contains(summary),
            "the escalated worker was not told `{summary}`. §11.4 carries the \
             accumulated feedback onto the next rung, and this prompt carries \
             part of it at best:\n--- prompt ---\n{prompt}"
        );
    }
    assert!(
        prompt.contains(SECOND_DETAIL),
        "the escalated worker was not given the reviewer's required changes \
         verbatim. §11.2 is what the retry gets back, and after a crash it \
         reached this prompt as a summary or not at \
         all:\n--- prompt ---\n{prompt}"
    );
    assert!(
        prompt.contains("Earlier attempts at this task failed"),
        "the escalated worker's prompt has no accumulated section, so at most \
         one record below its rung reached it:\n--- prompt ---\n{prompt}"
    );
}

#[test]
fn a_log_predating_the_detail_field_folds_and_resumes() {
    let fixture = Fixture::build(
        "driver-brief-oldlog",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished_failing(
                    1,
                    crate::ladder::FailureKind::GateFailed,
                    "gate `cargo test` failed: 1 failed",
                    "error[E0308]: mismatched types",
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let current = String::from_utf8(fixture.log_bytes()).expect("the log is utf-8");
    let aged: String = current
        .lines()
        .enumerate()
        .map(|(position, line)| {
            if position == 0 {
                return format!("{line}\n");
            }
            let mut value: serde_json::Value =
                serde_json::from_str(line).expect("every log line is a json object");
            if let Some(failure) = value.pointer_mut("/data/record/failure") {
                if let Some(object) = failure.as_object_mut() {
                    object.remove("detail");
                }
            }
            format!("{value}\n")
        })
        .collect();
    assert!(
        !aged.contains("\"detail\""),
        "the aged log still carries a detail key, so this test is reading the \
         current shape and proving nothing about the older one"
    );
    assert!(
        aged.contains("attempt_finished"),
        "the aged log has no settlement in it, so the field being absent is \
         vacuous"
    );

    let events = TopologyFold::parse_log(aged.as_bytes()).expect(
        "a log written before the detail field existed still parses — if this \
         refuses, the field is not additive and SCHEMA_VERSION had to move",
    );
    let details: Vec<Option<String>> = events
        .iter()
        .filter_map(|event| match &event.body {
            TopologyEventBody::AttemptFinished { data } => {
                Some(data.record.failure.as_ref()?.detail.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        details,
        vec![None],
        "an absent detail key must read back as None; anything else means an \
         older log folds to a different value than it was written with"
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    crate::workspace_manager::fixture::write_file(&fixture.log(), aged.as_bytes());
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("a run whose log predates the field still resumes");

    let brief = crate::engine::topology::run::Brief::replay(&handle.events);
    let lines = brief.lines(ALPHA);
    assert_eq!(
        lines.len(),
        1,
        "an older log's failure must still contribute its summary; the brief holds \
         {} line(s): {lines:?}",
        lines.len()
    );
    assert_eq!(
        lines[0].summary, "gate `cargo test` failed: 1 failed",
        "the summary is what an older log preserved and it must reach the next worker"
    );
    assert_eq!(
        lines[0].detail, None,
        "a log that never recorded the tail cannot produce one"
    );
}

fn drive_one_attempt(fixture: &Fixture, runner: &RecordingRunner) -> Vec<String> {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    run.step(&seams, &mut hooks)
        .expect("the resumed attempt settles");

    runner
        .requests()
        .into_iter()
        .filter(|request| request.role == crate::runner::ExecutionRole::Implement)
        .map(|request| String::from_utf8_lossy(&request.command.stdin).into_owned())
        .collect()
}

#[test]
fn the_driver_spends_the_allowance_the_log_records() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "driver-allowance",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run
        .step(&seams, &mut hooks)
        .expect("the second attempt settles");
    let Progress::Settled { accepted, .. } = progress else {
        panic!("the ready-dispatch branch did not settle: {progress:?}");
    };
    assert!(!accepted, "an agent error is not an acceptable attempt");

    let last = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => Some(data.settlement),
            _ => None,
        })
        .next_back()
        .expect("the attempt settled");

    let AttemptSettlement::Closed { transition, .. } = last else {
        panic!("the second attempt did not close its generation: {last:?}");
    };
    let SettlementTransition::Parked { question } = transition else {
        panic!(
            "the second attempt on a two-attempt rung with nowhere to escalate \
             settled as {transition:?}. A driver reading a constant \
             `attempts_on_rung: 1` gets `Retry` here and the task retries forever"
        );
    };

    assert!(
        question.context.contains("2 attempt(s)"),
        "the question quotes the wrong attempt count: {}",
        question.context
    );
}

#[test]
fn the_loop_continues_an_attempt_recovery_recreated() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "continue-open",
        Damage {
            open_generation: true,
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let before = durable_kinds(&fixture);
    assert_eq!(
        before.iter().filter(|k| *k == "task_dispatched").count(),
        1,
        "the fixture must leave exactly one dispatch for the continuation to reuse"
    );

    let progress = run
        .step(&seams, &mut hooks)
        .expect("the loop continues the attempt rather than stalling");
    let Progress::Settled { key, .. } = progress else {
        panic!("the ready-dispatch branch did not continue the attempt: {progress:?}");
    };
    assert_eq!(key, TaskKey(0));

    let after = durable_kinds(&fixture);
    assert_eq!(
        after.iter().filter(|k| *k == "task_dispatched").count(),
        1,
        "the continuation opened a fresh generation instead of continuing the \
         one recovery recreated — `T-DISPATCH` says continue attempt, no spend \
         repeats"
    );
    assert_eq!(
        after.iter().filter(|k| *k == "attempt_started").count(),
        1,
        "the continuation started no attempt, so the entitlement is still held \
         by a generation nothing can drive"
    );
}

#[test]
fn a_reviewer_runs_at_the_review_effort_not_the_implementers() {
    let fixture = Fixture::healthy("review-effort");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let entry = handle
        .fold
        .registry()
        .and_then(|registry| registry.get(TaskKey(0)))
        .expect("the fixture registers alpha");
    assert_eq!(
        entry.ladder.effort.review,
        Effort::Medium,
        "the fixture's review axis moved; this test needs it to differ from the rung's"
    );
    assert_eq!(
        entry
            .ladder
            .effort
            .implementation_for(entry.ladder.rungs[0].tier),
        Effort::High,
        "the fixture's Mid rung moved; this test needs it to differ from review"
    );

    let manager = fixture.manager();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    use crate::engine::topology::scaffold::REVIEW_AGENT;

    let mut reviewer_bound = entry.clone();
    reviewer_bound.reviews.primary = Some(crate::review::PassBinding::new(REVIEW_AGENT, "gpt"));
    let entry = &reviewer_bound;
    let pools = vec![
        crate::capacity::Pool::discovered(
            "the-implementers-pool",
            crate::capacity::PoolKind::SubscriptionWindow,
            AGENT,
            vec![crate::capacity::Source::Signals],
        ),
        crate::capacity::Pool::discovered(
            "the-reviewers-own-pool",
            crate::capacity::PoolKind::SubscriptionWindow,
            REVIEW_AGENT,
            vec![crate::capacity::Source::Signals],
        ),
    ];
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &pools,
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let binding = handle
        .fold
        .frozen_rung_binding(TaskKey(0), 0)
        .expect("rung 0 is frozen");
    let plan = crate::engine::topology::attempt::AttemptPlans::plan(
        &plans,
        &crate::engine::topology::attempt::PlanRequest {
            key: TaskKey(0),
            entry,
            attempt: crate::topology::events::AttemptNumber(1),
            rung: 0,
            binding,
            workspace: &fixture.repo_root,
            resume_session: None,
            feedback: Vec::new(),
            materialization_observed: None,
        },
    )
    .expect("the plan assembles");

    assert!(
        !plan.reviewers.is_empty(),
        "this fixture plans no reviewer, so the effort below is unasserted"
    );
    assert_eq!(
        plan.pool.as_deref(),
        Some("the-implementers-pool"),
        "the implementer did not resolve its own agent's pool, so a reviewer carrying that value \
         would not tell us anything"
    );
    for reviewer in &plan.reviewers {
        assert_eq!(
            reviewer.agent.as_str(),
            REVIEW_AGENT,
            "reviewer `{}` runs on the implementer's agent, so its pool lookup and the \
             implementer's are one lookup and both behaviours pass",
            reviewer.lens.name()
        );
        assert_eq!(
            reviewer.profile.effort,
            Some(Effort::Medium),
            "reviewer `{}` runs at the implementer's effort",
            reviewer.lens.name()
        );

        assert_eq!(
            reviewer.profile.pool,
            "the-reviewers-own-pool",
            "reviewer `{}` carries pool `{}`",
            reviewer.lens.name(),
            reviewer.profile.pool
        );
    }
    let _ = manager;
}

#[test]
fn the_loop_inherits_the_committed_digest_recovery_verified() {
    let fixture = Fixture::healthy("digest-inherited");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let expected = crate::rundir::run_started_sha256(&fixture.first_line);

    assert_eq!(
        handle.committed_first_line_sha256, expected,
        "the handle carries a digest that is not the committed first line's"
    );
    assert!(
        !handle.committed_first_line_sha256.is_empty(),
        "an empty digest asserts nothing: this fixture must publish a commit \
         record for the comparison to mean anything"
    );

    let run = crate::engine::topology::run::TopologyRun::resumed(
        handle,
        fixture.inputs(),
        crate::engine::topology::select::Ceiling::unlimited(),
    );
    assert_eq!(
        run.commitment_digest(),
        Some(expected.as_str()),
        "the loop's appends cannot prove their committed first line"
    );
}

#[test]
fn a_prepared_pin_without_a_candidate_record_is_orphan_residue() {
    let fixture = Fixture::build(
        "e6-orphan",
        Damage {
            open_generation: true,
            extra: vec![attempt_started(1)],
            ..Damage::default()
        },
    );
    let commit = seed_candidate_commit(&fixture, 0);

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (recovered, _handle) = outcome.expect("the resume completes rather than refusing");

    assert_eq!(
        recovered.interrupted, 1,
        "the attempt was running and nothing settled it, so the resume settles it \
         interrupted"
    );
    assert!(
        recovered.finished.is_empty(),
        "there is no candidate record, so there is no promotion to carry through: {:?}",
        recovered.finished
    );

    let prepared: Vec<_> = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter(|event| matches!(event.body, TopologyEventBody::CandidatePrepared { .. }))
        .collect();
    assert!(
        prepared.is_empty(),
        "recovery synthesised a candidate around the pinned commit {commit:?}; with one \
         atomic settlement a pin without a record is residue, not authorization"
    );
}

fn seed_candidate_commit(fixture: &Fixture, generation: u32) -> String {
    use crate::workspace_manager::fixture::{git, write_file};

    let repo = &fixture.repo_root;
    write_file(&repo.join("candidate.txt"), b"the worker's edit\n");
    git(repo, &["add", "--", "candidate.txt"]);
    let tree = git(repo, &["write-tree"]);
    let commit = git(
        repo,
        &[
            "commit-tree",
            &tree,
            "-p",
            fixture.base_sha.as_str(),
            "-m",
            "upstroke: alpha attempt 1",
        ],
    );
    git(repo, &["rm", "-q", "-f", "--", "candidate.txt"]);
    let pin = crate::engine::topology::candidate::candidate_pin_ref(
        RUN_ID,
        TaskKey(0),
        GenerationId(generation),
    );
    git(repo, &["update-ref", pin.as_str(), &commit]);
    commit
}

struct FixedIds;

impl crate::engine::topology::seams::IdSource for FixedIds {
    fn run_id(&self) -> String {
        RUN_ID.to_owned()
    }

    fn incarnation(&self) -> crate::topology::events::IncarnationId {
        crate::topology::events::IncarnationId("inc-fixed".to_owned())
    }

    fn pid(&self) -> u32 {
        4242
    }

    fn question_id(&self) -> crate::ir::QuestionId {
        crate::ir::QuestionId("q-park-fixed".to_owned())
    }
}

fn durable_kinds(fixture: &Fixture) -> Vec<String> {
    TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .iter()
        .map(|event| event.body.kind().to_owned())
        .collect()
}

#[derive(Default)]
struct RecordingSleeper {
    slept: std::sync::Mutex<Vec<Duration>>,
}

impl crate::interaction::Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.slept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(duration);
    }
}

#[test]
fn a_call_census_needle_is_not_satisfied_by_a_longer_name_ending_in_it() {
    assert_eq!(
        crate::effects::census_domain::production_calls(
            "            .refuse_unexpected_refs(&namespace, &expected)?;\n",
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        0,
        "a longer identifier ending in the entry's name satisfied its census entry"
    );
    assert_eq!(
        crate::effects::census_domain::production_calls(
            "        let expected = crate::engine::topology::candidate::expected_refs(&r, f);\n",
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        1,
        "a genuine call through a path was rejected: `:` is not an identifier byte"
    );
    assert_eq!(
        crate::effects::census_domain::production_calls(
            "    let e = expected_refs(&run_id, fold);\n",
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        1,
        "a genuine bare call was rejected"
    );
    assert_eq!(
        crate::effects::census_domain::production_calls(
            "pub fn expected_refs(run_id: &str, fold: &TopologyFold) -> Vec<String> {\n",
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        0,
        "a definition is not a call: a function that calls only itself is what this census exists \
         to catch"
    );

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source =
        std::fs::read_to_string(manifest.join("src/workspace_manager.rs")).expect("a source file");
    let moved = std::fs::read_to_string(manifest.join("src/workspace_manager/tests.rs"))
        .expect("a source file");
    let code = crate::effects::production_code(&source);
    let whole = source.matches("expected_refs(").count() + moved.matches("expected_refs(").count();
    let region = code.matches("expected_refs(").count();
    assert!(
        whole >= 4 && region >= 1,
        "`workspace_manager` no longer carries the substring this test is about ({whole} in \
         the module, {region} in the production region), so the zero below proves nothing"
    );
    assert_eq!(
        crate::effects::census_domain::production_calls(
            &code,
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        0,
        "the production region of `workspace_manager.rs` has {region} occurrence(s) of \
         `expected_refs(` and every one of them belongs to `refuse_unexpected_refs`; counting \
         them is how a census entry gets proved by a function that is not the one it names"
    );
}

#[test]
fn every_packet_named_recovery_action_has_a_production_caller() {
    const CLAUSES: &[(&str, crate::effects::census_domain::Call, &str)] = &[
        (
            "prune_orphan_pin",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-CAND-OBJ].resume_action (b): delete the exact orphan pin \
             expected-old",
        ),
        (
            "refuse_unexpected_refs",
            crate::effects::census_domain::Call::Method,
            "transaction_fault_matrix[T-CAND-OBJ].refusal_condition: an unexpected ref under the \
             run namespace",
        ),
        (
            "expected_refs",
            crate::effects::census_domain::Call::Free,
            "the entitlement `refuse_unexpected_refs` refuses against, derived from the fold",
        ),
        (
            "finish_promotions",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-CAND-REF].resume_action: verify, create the ref, append \
             task_candidate_created, prune the pin",
        ),
        (
            "recreate_open_no_attempt",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-DISPATCH].resume_action: verify the worktree or recreate it",
        ),
        (
            "settle_interrupted",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-ATTEMPT].resume_action: append attempt_interrupted",
        ),
        (
            "close_retained_idle",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-RETAINED].resume_action: a fresh process closes it in \
             recovery",
        ),
        (
            "ensure_recorded_integration_ref",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-RUNSTART].resume_action: P7/P8 create the ref zero-old at \
             the recorded base",
        ),
        (
            "refuse_unimplemented_terminals",
            crate::effects::census_domain::Call::Free,
            "checkpoint_refusals: refuse, before any append, any operation whose terminals this \
             build does not implement",
        ),
        (
            "resume_open_no_attempt",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-DISPATCH].resume_action: continue attempt (no spend \
             repeats)",
        ),
    ];

    let mut test_files_skipped = 0_usize;
    let sources: Vec<(String, String)> = {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut all = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("src is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    all.push(path);
                }
            }
        }
        let test_modules = crate::effects::census_domain::whole_file_test_modules(&root, &all, 13);
        let mut out = Vec::new();
        {
            for path in all {
                if test_modules.contains(&path) {
                    test_files_skipped += 1;
                    continue;
                }
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let source = std::fs::read_to_string(&path).expect("a source file");
                out.push((relative, crate::effects::production_code(&source)));
            }
        }
        out
    };
    assert!(
        sources.len() > 20,
        "the walk found {} sources, so its zero counts would prove nothing",
        sources.len()
    );
    assert!(
        test_files_skipped >= crate::effects::tests::cfg::WHOLE_FILE_TEST_MODULES.len()
            && sources.iter().all(|(rel, _)| !rel.ends_with("tests.rs")
                && !rel.ends_with("scaffold.rs")
                && !rel.ends_with("premove.rs")
                && !rel.ends_with("fake.rs")
                && !rel.ends_with("fixture.rs")
                && !rel.ends_with("scratch_tree.rs")
                && !rel.ends_with("readiness.rs")),
        "the out-of-line test modules are not being skipped ({test_files_skipped} skipped of all \
         the crate declares), so a fixture's call can satisfy a clause on production's behalf. \
         The six named here are the ones a file-name rule misses, and they are named rather \
         than counted because the count above cannot see a substitution: a skip set of the \
         right size that dropped one of these and gained an unrelated production file \
         satisfies it, and one of these carrying a needle then answers a census on \
         production's behalf"
    );

    let mut uncalled: Vec<String> = Vec::new();
    let mut undefined: Vec<String> = Vec::new();
    for (name, form, clause) in CLAUSES {
        let defined: usize = sources
            .iter()
            .map(|(_, code)| code.matches(&format!("fn {name}(")).count())
            .sum();
        if defined == 0 {
            undefined.push((*name).to_owned());
        }
        let calls: usize = sources
            .iter()
            .map(|(_, code)| crate::effects::census_domain::production_calls(code, name, *form))
            .sum();
        if calls == 0 {
            uncalled.push(format!("`{name}` performs `{clause}`"));
        }
    }

    assert!(
        undefined.is_empty(),
        "these are named as performing a packet clause and no production item of that name \
         exists, so the row below cannot fail for the right reason and has been passing on \
         somebody else's call sites: {undefined:?}"
    );

    assert!(
        uncalled.is_empty(),
        "these implement a packet clause and nothing in production calls them, so the clause is \
         not performed by any run — which is how this slice shipped a converged promotion that \
         stalled forever and a resumed run that forgot its spend:\n  {}",
        uncalled.join("\n  ")
    );
}
