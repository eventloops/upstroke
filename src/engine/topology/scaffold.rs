//! Extended notes: `docs/internals/engine/topology/scaffold.md`

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::agent::proc::{ProcessOutput, SpawnHooks};
use crate::error::UpstrokeError;
use crate::events::log::{EventHooks, EventLog, TopologyLine, site_for};
use crate::events::{AttemptRecord, BindingSummary, ChainSummary, GateSummary, RunOutcome};
use crate::gates::ShellKind;
use crate::ir::{
    Artifact, ArtifactId, Effort, Plan, PlanSource, ResolvedEffortPolicy, Task, TaskId, TaskKind,
    Tier,
};
use crate::review::{PassBinding, ReviewPlan};
use crate::rundir::RunDirHooks;
use crate::runner::container::ContainerHooks;
use crate::runner::{AgentId, CommandSpec, ExecutionRole, InvocationId, Runner, RunnerRequest};
use crate::topology::effects::{
    EffectSiteId, EventSite, HookHarness, HookPhase, Injection, InjectionMode, SubEffectPoint,
};
use crate::topology::events::{
    AttemptFinished4, AttemptNumber, AttemptSettlement, CommitSha, Epoch, FrozenSpawn,
    GenerationId, GitRef, IncarnationId, RunStarted4, RungBinding, RunnerContract, RunnerKind,
    RunnerPolicy, SessionId, SpawnAdmission, TaskSpawned, TopologyEvent, TopologyEventBody,
    TopologyLimits,
};
use crate::topology::fold::{FrozenInputs, GenerationClass, TaskFold, TaskState, TopologyFold};
use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion, PathSet};
use crate::topology::registry::{Lineage, Origin, TaskKey, TaskRegistry, repair_display_id};
use crate::topology::schema::TOPOLOGY_SCHEMA;
use crate::util::DurabilityLedger;
use crate::workspace_manager::{
    EffectHooks, HarnessEffects, WorkspaceManager,
    fixture::{Fixture, died_by_abort, run_kill_child, write_file},
};

use super::attempt::{AttemptPlan, GatePlan, ReviewerPlan};
use super::dispatch::{DispatchKind, DispatchRequest, Dispatched, EventEmitter, dispatch};

pub(super) const ALPHA: TaskKey = TaskKey(0);
pub(super) const BETA: TaskKey = TaskKey(1);

pub(super) const AGENT: &str = "claude-code";
pub(super) const REVIEW_AGENT: &str = "copilot";

fn probed_agents() -> Vec<String> {
    vec![AGENT.to_owned(), REVIEW_AGENT.to_owned()]
}

fn task_of(id: &str) -> Task {
    Task {
        id: TaskId::from(id),
        kind: TaskKind::Refactor,
        title: format!("{id} title"),
        body: format!("{id} body"),
        depends_on: Vec::new(),
        acceptance: vec![format!("{id} passes")],
        path_hints: vec![format!("src/{id}/")],
        suggested_tier: None,
        min_tier: None,
        artifacts_in: Vec::new(),
        artifacts_out: vec![ArtifactId::from(format!("{id}-out").as_str())],
    }
}

fn plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "scaffold-plan-hash".to_owned(),
        },
        tasks: vec![task_of("alpha"), task_of("beta")],
        artifacts: vec![Artifact {
            id: ArtifactId::from("alpha-out"),
            produced_by: Some(TaskId::from("alpha")),
        }],
    }
}

fn chain(task: &str) -> ChainSummary {
    let tiers = vec![Tier::Mid, Tier::Frontier];
    ChainSummary {
        task: task.to_owned(),
        attempts_per: 2,
        bindings: Some(
            tiers
                .iter()
                .map(|tier| BindingSummary {
                    tier: *tier,
                    agent: AGENT.to_owned(),
                    model: format!("{task}-{tier}-model"),
                    pinned: *tier == Tier::Frontier,
                })
                .collect(),
        ),
        tiers,
    }
}

const NORMALIZED_DIGEST: &str =
    "sha256:1010101010101010101010101010101010101010101010101010101010101010";

fn run_started(fixture: &Fixture) -> RunStarted4 {
    let plan = plan();
    let unauthenticated = RunStarted4 {
        schema: TOPOLOGY_SCHEMA,
        upstroke_version: "0.2.0-scaffold".to_owned(),
        run_id: "01SCAFFOLD00000000000000AA".to_owned(),
        incarnation: IncarnationId("01SCAFFOLDINC0000000000000".to_owned()),
        runner: RunnerPolicy {
            kind: RunnerKind::Host,
            policy: RunnerContract::HostV1,
            image: None,
            credential_volumes: None,
        },
        probed_agents: probed_agents(),
        branch: "upstroke/run-01SCAFFOLD00000000000000AA".to_owned(),
        integration_ref: GitRef("refs/heads/upstroke/run-01SCAFFOLD00000000000000AA".to_owned()),
        base_sha: CommitSha(fixture.head.clone()),
        execution_root: fixture
            .manager
            .execution_root()
            .to_string_lossy()
            .into_owned(),
        private_dir: fixture.private.to_string_lossy().into_owned(),
        plan_path: "docs/plan.md".to_owned(),
        config_path: Some("upstroke.toml".to_owned()),
        plan_hash: plan.source.hash.clone(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
        registry_digest: String::new(),
        path_policy: PathPolicy {
            version: PathPolicyVersion::V2,
            case_fold: true,
            grammar: PathGrammar::Globset,
        },
        limits: TopologyLimits {
            max_parallel: 1,
            max_defers: 2,
            max_merge_repairs: 3,
        },
        gates: vec!["fmt".to_owned()],
        gates_from_config: true,
        gate_cmds: vec![GateSummary {
            name: "fmt".to_owned(),
            cmd: "cargo fmt --check".to_owned(),
            timeout: Duration::from_secs(60),
            shell: ShellKind::Bash,
        }],
        interaction_mode: "never".to_owned(),
        chains: plan.tasks.iter().map(|t| chain(t.id.as_str())).collect(),
        effort_policy: ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::High,
            frontier: Effort::Max,
            review: Effort::Medium,
        },
        reviews: ReviewPlan {
            enabled: Some(true),
            alternative_available: Some(true),
            pass_timeout_secs: Some(900),
            primary: Some(PassBinding::new(AGENT, "opus")),
            alternative: Some(PassBinding::new(REVIEW_AGENT, "gpt")),
            second_opinion: vec![None, None],
        },
    };
    let digest = TaskRegistry::originals_with_agents(
        &plan,
        &unauthenticated.registry_record(),
        &unauthenticated.probed_agents,
    )
    .expect("the fixture record derives a registry")
    .digest();
    RunStarted4 {
        registry_digest: digest,
        ..unauthenticated
    }
}

pub(super) struct FoldedEmitter {
    log: EventLog,
    fold: TopologyFold,
    hooks: Box<dyn EventHooks>,
    clock: super::seams::SystemClock,
}

impl FoldedEmitter {
    pub(super) fn fold(&self) -> &TopologyFold {
        &self.fold
    }

    pub(super) fn durable_events(&self) -> Vec<TopologyEvent> {
        let bytes = std::fs::read(self.log.path()).expect("read the log back");
        TopologyFold::parse_log(&bytes).expect("the log parses")
    }

    pub(super) fn durable_kinds(&self) -> Vec<&'static str> {
        self.durable_events()
            .iter()
            .map(|event| event.body.kind())
            .collect()
    }

    pub(super) fn task(&self, key: TaskKey) -> &TaskFold {
        self.fold.task(key).expect("the task is registered")
    }

    pub(super) fn generation_class(
        &self,
        key: TaskKey,
        generation: GenerationId,
    ) -> GenerationClass {
        self.task(key)
            .generations
            .iter()
            .find(|held| held.id == generation)
            .unwrap_or_else(|| panic!("task {key} has no generation {}", generation.0))
            .class
            .clone()
    }
}

impl EventEmitter for FoldedEmitter {
    fn emit(
        &mut self,
        body: TopologyEventBody,
        _hooks: &mut dyn super::seams::TopologyHooks,
    ) -> Result<(), crate::engine::topology::emit::EmitFailure> {
        let event = TopologyEvent {
            ts: <super::seams::SystemClock as super::seams::TimeSource>::now_rfc3339(&self.clock),
            body,
        };
        let (line, round_tripped) = TopologyLine::round_trip(&event)?;
        let delta =
            self.fold
                .plan_transition(&round_tripped)
                .map_err(|error| UpstrokeError::Refused {
                    message: format!("the fold refused `{}`: {error}", event.body.kind()),
                })?;
        let site = site_for(&round_tripped.body);
        self.log
            .append_topology_hooked(site, &line, self.hooks.as_mut())?;
        self.fold.apply_delta(delta);
        Ok(())
    }
}

#[derive(Clone, Default)]
pub(super) struct Timeline(Arc<Mutex<Vec<(EffectSiteId, HookPhase)>>>);

impl Timeline {
    fn push(&self, site: EffectSiteId, phase: HookPhase) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((site, phase));
    }

    pub(super) fn positions(&self, site: EffectSiteId, phase: HookPhase) -> Vec<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .enumerate()
            .filter(|(_, seen)| **seen == (site, phase))
            .map(|(index, _)| index)
            .collect()
    }

    pub(super) fn mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

struct TimelineEvents {
    inner: crate::events::log::HarnessEventHooks,
    timeline: Timeline,
}

impl EventHooks for TimelineEvents {
    fn phase(&mut self, site: EventSite, phase: HookPhase) {
        self.timeline.push(EffectSiteId::Event(site), phase);
        self.inner.phase(site, phase);
    }

    fn point(&mut self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> Injection {
        self.inner.point(site, point, mode)
    }
}

pub(super) struct ArmedEffects {
    inner: HarnessEffects,
    timeline: Timeline,
    armed: Vec<(EffectSiteId, HookPhase, Injection)>,
}

impl ArmedEffects {
    fn new(harness: &Arc<Mutex<HookHarness>>, timeline: &Timeline) -> Self {
        Self {
            inner: HarnessEffects::new(Arc::clone(harness)).recording_durability(),
            timeline: timeline.clone(),
            armed: Vec::new(),
        }
    }

    pub(super) fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {
        self.armed.push((site, phase, injection));
    }
}

impl EffectHooks for ArmedEffects {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.timeline.push(site, phase);
        let shared = self.inner.phase(site, phase);
        for (armed_site, armed_phase, injection) in &self.armed {
            if *armed_site == site && *armed_phase == phase {
                return *injection;
            }
        }
        shared
    }

    fn durability_ledger(&self) -> DurabilityLedger {
        self.inner.durability_ledger()
    }

    fn refusal_cause(&self) -> Option<String> {
        self.inner.refusal_cause()
    }
}

pub(super) struct Hooks {
    effects: ArmedEffects,
    rundir: crate::rundir::HarnessHooks,
    events: crate::events::log::HarnessEventHooks,
    container: crate::runner::container::HarnessHooks,
    spawn: crate::runner::HarnessHooks,
}

impl Hooks {
    fn new(harness: &Arc<Mutex<HookHarness>>, timeline: &Timeline) -> Self {
        Self {
            effects: ArmedEffects::new(harness, timeline),
            rundir: crate::rundir::HarnessHooks::new(Arc::clone(harness)),
            events: crate::events::log::HarnessEventHooks::new(Arc::clone(harness)),
            container: crate::runner::container::HarnessHooks::new(Arc::clone(harness)),
            spawn: crate::runner::HarnessHooks::new(Arc::clone(harness)),
        }
    }

    pub(super) fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {
        self.effects.arm(site, phase, injection);
    }
}

impl super::seams::TopologyHooks for Hooks {
    fn effects(&mut self) -> &mut dyn EffectHooks {
        &mut self.effects
    }

    fn rundir(&mut self) -> &mut dyn RunDirHooks {
        &mut self.rundir
    }

    fn events(&mut self) -> &mut dyn EventHooks {
        &mut self.events
    }

    fn container(&mut self) -> &mut dyn ContainerHooks {
        &mut self.container
    }

    fn spawn(&mut self) -> &mut dyn SpawnHooks {
        &mut self.spawn
    }
}

#[derive(Debug, Clone)]
pub(super) struct Ran {
    pub(super) invocation: InvocationId,
    pub(super) role: ExecutionRole,
    pub(super) workspace: PathBuf,
    pub(super) agent: Option<AgentId>,
    pub(super) command: CommandSpec,
    pub(super) durable_at_spawn: Vec<String>,
}

pub(super) const GATE_DIAGNOSTIC: &str = "scaffold gate rejected the diff";

#[derive(Debug, Default)]
pub(super) struct RecordingRunner {
    ran: Mutex<Vec<Ran>>,
    codes: Mutex<Vec<i32>>,
    log: Mutex<Option<PathBuf>>,
}

impl RecordingRunner {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn set_codes(&self, codes: Vec<i32>) {
        *self
            .codes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = codes;
    }

    pub(super) fn failing_with(codes: Vec<i32>) -> Self {
        Self {
            ran: Mutex::new(Vec::new()),
            codes: Mutex::new(codes),
            log: Mutex::new(None),
        }
    }

    pub(super) fn watching(&self, log: &Path) {
        *self
            .log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(log.to_path_buf());
    }

    fn durable_now(&self) -> Vec<String> {
        let path = self
            .log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(path) = path else {
            return Vec::new();
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return Vec::new();
        };
        TopologyFold::parse_log(&bytes)
            .map(|events| {
                events
                    .iter()
                    .map(|event| event.body.kind().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn ran(&self) -> Vec<Ran> {
        self.ran
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Runner for RecordingRunner {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        let durable_at_spawn = self.durable_now();
        self.ran
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Ran {
                invocation: request.invocation.clone(),
                role: request.role.clone(),
                workspace: request.workspace.clone(),
                agent: request.agent.clone(),
                command: request.command.clone(),
                durable_at_spawn,
            });
        let mut codes = self
            .codes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let code = if codes.is_empty() { 0 } else { codes.remove(0) };
        Ok(ProcessOutput {
            code: Some(code),
            stdout: if code == 0 {
                String::new()
            } else {
                format!("{GATE_DIAGNOSTIC} (exit {code})\n")
            },
            stderr: String::new(),
            duration: Duration::from_millis(1),
            timed_out: false,
            output_limited: false,
        })
    }
}

pub(super) struct AnsweringAdapter {
    id: &'static str,
    verdict: &'static str,
    status: crate::ir::OutcomeStatus,
}

impl AnsweringAdapter {
    pub(super) const fn erroring(id: &'static str) -> Self {
        Self {
            status: crate::ir::OutcomeStatus::AgentError,
            ..Self::passing(id)
        }
    }

    pub(super) const fn asking(id: &'static str) -> Self {
        Self {
            verdict: "UPSTROKE-QUESTION: the spec names two incompatible \
                      formats and I should not pick one alone",
            ..Self::passing(id)
        }
    }

    pub(super) const fn rate_limited(id: &'static str) -> Self {
        Self {
            status: crate::ir::OutcomeStatus::RateLimited,
            ..Self::passing(id)
        }
    }

    pub(super) const fn passing(id: &'static str) -> Self {
        Self {
            id,
            verdict: "```json\n{\"pass\": true, \"reasons\": [], \"required_changes\": []}\n```",
            status: crate::ir::OutcomeStatus::Completed,
        }
    }
}

impl crate::agent::AgentAdapter for AnsweringAdapter {
    fn id(&self) -> &'static str {
        self.id
    }

    fn probe(&self, _runner: &dyn Runner) -> Result<crate::agent::Caps, UpstrokeError> {
        panic!("the scaffold's attempts do not pre-flight; `preflight.rs` owns that path")
    }

    fn build(&self, run: &crate::agent::TaskRun) -> Result<CommandSpec, UpstrokeError> {
        let spec = CommandSpec::new(self.id)
            .arg("--prompt")
            .arg(run.prompt.clone());
        match &run.resume_session {
            Some(session) => Ok(spec.arg("--resume").arg(session.clone())),
            None => Ok(spec),
        }
    }

    fn parse(
        &self,
        out: &crate::agent::ProcessOutput,
    ) -> Result<crate::ir::Outcome, UpstrokeError> {
        Ok(crate::ir::Outcome {
            status: self.status,
            diff: String::new(),
            detail: Some(self.verdict.to_owned()),
            session_id: Some(format!("{}-session", self.id)),
            usage: None,
            cost_usd: Some(0.25),
            transcript_path: PathBuf::new(),
            duration: out.duration,
        })
    }

    fn materialize_permissions(
        &self,
        _profile: &crate::ir::WorkerProfile,
        _gate_cmds: &[String],
        _dir: &Path,
        _stem: &str,
    ) -> Result<Option<PathBuf>, UpstrokeError> {
        Ok(None)
    }
}

pub(super) struct ScaffoldAdapters {
    primary: AnsweringAdapter,
    second: AnsweringAdapter,
}

impl ScaffoldAdapters {
    pub(super) const fn erroring() -> Self {
        Self {
            primary: AnsweringAdapter::erroring(AGENT),
            second: AnsweringAdapter::passing(REVIEW_AGENT),
        }
    }

    pub(super) const fn asking() -> Self {
        Self {
            primary: AnsweringAdapter::asking(AGENT),
            second: AnsweringAdapter::passing(REVIEW_AGENT),
        }
    }

    pub(super) const fn rate_limiting() -> Self {
        Self {
            primary: AnsweringAdapter::rate_limited(AGENT),
            second: AnsweringAdapter::passing(REVIEW_AGENT),
        }
    }

    pub(super) const fn new() -> Self {
        Self {
            primary: AnsweringAdapter::passing(AGENT),
            second: AnsweringAdapter::passing(REVIEW_AGENT),
        }
    }
}

impl crate::agent::AdapterSource for ScaffoldAdapters {
    fn get(&self, id: &str) -> Option<&dyn crate::agent::AgentAdapter> {
        if id == AGENT {
            Some(&self.primary)
        } else if id == REVIEW_AGENT {
            Some(&self.second)
        } else {
            None
        }
    }
}

pub(super) struct Run {
    pub(super) fixture: Fixture,
    pub(super) paths: crate::rundir::RunPaths,
    pub(super) harness: Arc<Mutex<HookHarness>>,
    pub(super) hooks: Hooks,
    pub(super) timeline: Timeline,
    pub(super) emitter: FoldedEmitter,
    pub(super) runner: RecordingRunner,
    pub(super) invocations: crate::engine::topology::identity::InvocationLedger,
}

impl Run {
    pub(super) fn started(tag: &str) -> Self {
        let fixture = Fixture::created(tag);
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let timeline = Timeline::default();
        let log = EventLog::open(
            EventSite::OpenLog,
            &fixture.private.join("events.jsonl"),
            &mut Vec::new(),
        )
        .expect("open the schema-4 log");
        let mut run = Self {
            emitter: FoldedEmitter {
                log,
                fold: TopologyFold::new(FrozenInputs {
                    plan: plan(),
                    normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
                }),
                hooks: Box::new(TimelineEvents {
                    inner: crate::events::log::HarnessEventHooks::new(Arc::clone(&harness)),
                    timeline: timeline.clone(),
                }),
                clock: super::seams::SystemClock,
            },
            hooks: Hooks::new(&harness, &timeline),
            invocations: crate::engine::topology::identity::InvocationLedger::new(),
            runner: RecordingRunner::new(),
            timeline,
            harness,
            paths: {
                let paths =
                    crate::rundir::RunPaths::new(&fixture.base, "01SCAFFOLD00000000000000AA");
                paths.create().expect("the scaffold's run directories");
                paths
            },
            fixture,
        };
        let started = run_started(&run.fixture);
        run.emitter
            .emit(
                TopologyEventBody::RunStarted {
                    data: Box::new(started),
                },
                &mut run.hooks,
            )
            .expect("run_started");
        run.runner.watching(run.emitter.log.path());
        run
    }

    pub(super) fn manager(&self) -> &WorkspaceManager {
        &self.fixture.manager
    }

    pub(super) fn base(&self) -> CommitSha {
        CommitSha(self.fixture.head.clone())
    }

    pub(super) fn predicted(&self, key: TaskKey) -> PathSet {
        self.emitter
            .fold()
            .predicted_region(key)
            .expect("the scaffold's run has started, so its registry answers")
    }

    pub(super) fn observed(&self, site: EffectSiteId, phase: HookPhase) -> bool {
        self.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observed(site, phase)
    }

    pub(super) fn order_of(&self, site: EffectSiteId, phase: HookPhase) -> Option<usize> {
        self.timeline.positions(site, phase).first().copied()
    }

    pub(super) fn must_order_of(&self, site: EffectSiteId, phase: HookPhase) -> usize {
        self.order_of(site, phase)
            .unwrap_or_else(|| panic!("nothing drove `{site}` at its `{phase}` phase"))
    }

    pub(super) fn mark(&self) -> usize {
        self.timeline.mark()
    }

    pub(super) fn order_after(&self, mark: usize, site: EffectSiteId, phase: HookPhase) -> usize {
        self.timeline
            .positions(site, phase)
            .into_iter()
            .find(|position| *position >= mark)
            .unwrap_or_else(|| {
                panic!("nothing drove `{site}` at its `{phase}` phase after position {mark}")
            })
    }

    pub(super) fn count_after(&self, mark: usize, site: EffectSiteId, phase: HookPhase) -> usize {
        self.timeline
            .positions(site, phase)
            .into_iter()
            .filter(|position| *position >= mark)
            .count()
    }

    pub(super) fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {
        self.hooks.arm(site, phase, injection);
    }

    pub(super) fn arm_point(
        &mut self,
        site: EffectSiteId,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) {
        self.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .arm(site, point, mode)
            .expect("the site exposes this point in this mode");
    }

    pub(super) fn task_state(&self, key: TaskKey) -> TaskState {
        self.emitter.task(key).state
    }
}

impl Run {
    pub(super) fn adopt(root: PathBuf) -> Self {
        let fixture = Fixture::adopt(root);
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let timeline = Timeline::default();
        let log_path = fixture.private.join("events.jsonl");
        let bytes = std::fs::read(&log_path).expect("the child's log survives it");
        let events = TopologyFold::parse_log(&bytes).expect("the child's log parses");
        let fold = TopologyFold::replay(
            FrozenInputs {
                plan: plan(),
                normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
            },
            &events,
        )
        .expect("the child's log replays");
        let log = EventLog::open(EventSite::OpenLog, &log_path, &mut Vec::new())
            .expect("reopen the schema-4 log");
        let adopted = Self {
            emitter: FoldedEmitter {
                log,
                fold,
                hooks: Box::new(TimelineEvents {
                    inner: crate::events::log::HarnessEventHooks::new(Arc::clone(&harness)),
                    timeline: timeline.clone(),
                }),
                clock: super::seams::SystemClock,
            },
            hooks: Hooks::new(&harness, &timeline),
            runner: RecordingRunner::new(),
            invocations: crate::engine::topology::identity::InvocationLedger::new(),
            timeline,
            harness,
            paths: {
                let paths =
                    crate::rundir::RunPaths::new(&fixture.base, "01SCAFFOLD00000000000000AA");
                paths.create().expect("the scaffold's run directories");
                paths
            },
            fixture,
        };
        adopted.runner.watching(adopted.emitter.log.path());
        adopted
    }

    pub(super) fn hand_off(&self, dir: &Path) {
        write_file(
            &dir.join(HANDOFF),
            self.fixture.root.to_string_lossy().as_bytes(),
        );
    }

    pub(super) fn dispatch(&mut self, key: TaskKey, generation: u32) -> Dispatched {
        self.try_dispatch(key, generation).expect("dispatch")
    }

    pub(super) fn try_dispatch(
        &mut self,
        key: TaskKey,
        generation: u32,
    ) -> Result<Dispatched, UpstrokeError> {
        let request = DispatchRequest {
            key,
            generation: GenerationId(generation),
            base: self.base(),
            kind: DispatchKind::Ordinary {
                paths: self.predicted(key),
            },
        };
        dispatch(
            &self.fixture.manager,
            &mut self.hooks,
            &mut self.emitter,
            &request,
        )
        .map_err(|failure| failure.discharging(&mut self.invocations))
    }

    pub(super) fn spawn_repair(&mut self, root: TaskKey) -> TaskKey {
        let registry = self
            .emitter
            .fold()
            .registry()
            .expect("the run has a registry");
        let key = TaskKey(u32::try_from(registry.len()).expect("a small fixture registry"));
        let parent = registry.get(root).expect("the root is registered").clone();
        let mut entry = parent.clone();
        entry.key = key;
        entry.display_id =
            crate::ir::TaskId::from(repair_display_id(1, &parent.display_id).as_str());
        entry.origin = Origin::MergeRepair;
        entry.deps = Vec::new();
        entry.display_deps = Vec::new();
        entry.lineage = Some(Lineage {
            root,
            parent: root,
            index: 1,
        });
        self.emitter
            .emit(
                TopologyEventBody::TaskSpawned {
                    data: Box::new(TaskSpawned {
                        spawn: FrozenSpawn {
                            key,
                            entry,
                            admission: SpawnAdmission::Runnable,
                        },
                    }),
                },
                &mut self.hooks,
            )
            .expect("task_spawned");
        key
    }
}

pub(super) const RETAINED_SESSION: &str = "session-01SCAFFOLD";

impl Run {
    pub(super) fn binding(&self, key: TaskKey, rung: u32) -> RungBinding {
        let entry = self
            .emitter
            .fold()
            .registry()
            .expect("a registry")
            .get(key)
            .expect("the task is registered");
        let frozen = entry
            .ladder
            .rungs
            .get(rung as usize)
            .expect("the ladder has this rung");
        let effort = entry.ladder.effort.implementation_for(frozen.tier);
        RungBinding::from_frozen(frozen, effort)
    }

    pub(super) fn attempt_plan(&self, key: TaskKey, attempt: u32) -> AttemptPlan {
        AttemptPlan {
            attempt: AttemptNumber(attempt),
            rung: 0,
            binding: self.binding(key, 0),
            pool: Some("scaffold-pool".to_owned()),
            resume_session: None,
            materialization_observed: None,
            agent: AgentId::new(AGENT),
            session_resume: true,
            worker: CommandSpec::new("worker").arg("--implement"),
            worker_timeout: Duration::from_secs(300),
            gates: vec![{
                let (command, timeout) = crate::gates::ShellGate {
                    name: "scaffold".to_owned(),
                    cmd: "gate --check".to_owned(),
                    timeout: Duration::from_secs(60),
                    shell: crate::gates::ShellKind::native(),
                }
                .command();
                GatePlan {
                    name: "scaffold".to_owned(),
                    command,
                    timeout,
                }
            }],
            reviewers: vec![
                ReviewerPlan {
                    agent: AgentId::new(AGENT),
                    profile: crate::review::profile_for(
                        AGENT,
                        "scaffold-model",
                        "primary",
                        Effort::High,
                    ),
                    lens: crate::review::Lens::Acceptance,
                    preflight_cli_version: Some("scaffold-cli/1".to_owned()),
                    timeout: Duration::from_secs(120),
                },
                ReviewerPlan {
                    agent: AgentId::new(REVIEW_AGENT),
                    profile: crate::review::profile_for(
                        REVIEW_AGENT,
                        "scaffold-second-model",
                        "second_opinion",
                        Effort::High,
                    ),
                    lens: crate::review::Lens::SecondOpinion,
                    preflight_cli_version: None,
                    timeout: Duration::from_secs(120),
                },
            ],
        }
    }

    pub(super) fn review_inputs(&self) -> super::attempt::ReviewInputs {
        super::attempt::ReviewInputs {
            title: "scaffold task".to_owned(),
            body: String::new(),
            acceptance: vec!["it works".to_owned()],
            diff: "diff --git a/a b/a\n".to_owned(),
            artifacts: Vec::new(),
            decisions: Vec::new(),
            stem: "scaffold".to_owned(),
        }
    }

    pub(super) fn retain(&mut self, key: TaskKey, generation: GenerationId, attempt: u32) {
        self.emitter
            .emit(
                TopologyEventBody::AttemptFinished {
                    data: Box::new(AttemptFinished4 {
                        key,
                        generation,
                        attempt: AttemptNumber(attempt),
                        record: Box::new(AttemptRecord {
                            attempt,
                            tier: "mid".to_owned(),
                            model: "alpha-mid-model".to_owned(),
                            pool: Some("scaffold-pool".to_owned()),
                            resumed: false,
                            duration: Duration::from_millis(7),
                            cost_usd: None,
                            reviews: Vec::new(),
                            session_id: Some(RETAINED_SESSION.to_owned()),
                            usage: None,
                            failure: Some(crate::events::FailureRecord {
                                kind: crate::ladder::FailureKind::GateFailed,
                                origin: crate::ladder::FailureOrigin::Worker,
                                reason: "the scaffold's judged failure".to_owned(),
                                detail: None,
                            }),
                        }),
                        settlement: AttemptSettlement::Retained {
                            retained_session: SessionId(RETAINED_SESSION.to_owned()),
                            retained_incarnation: Epoch(0),
                        },
                    }),
                },
                &mut self.hooks,
            )
            .expect("attempt_finished(retained)");
    }
}

const HANDOFF: &str = "fixture-root";

pub(super) fn kill_dir(tag: &str) -> PathBuf {
    static ORDINAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let ordinal = ORDINAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "upstroke-topo-{tag}-{}-{ordinal}",
        std::process::id()
    ));
    crate::workspace_manager::fixture::create_dir(&dir);
    dir
}

pub(super) fn kill_child_and_adopt(test: &str, dir: &Path, site: &str) -> Run {
    let status = run_kill_child(
        test,
        &[
            ("UPSTROKE_TEST_KILL_DIR", dir.as_os_str()),
            ("UPSTROKE_TEST_KILL_SITE", std::ffi::OsStr::new(site)),
        ],
    );
    assert!(
        died_by_abort(&status),
        "`{site}`: the child must have died by `std::process::abort()`, and it ended {status:?} \
         — a child that reached its own `unreachable!` panics instead, which means the injection \
         stopped killing"
    );
    let root = std::fs::read_to_string(dir.join(HANDOFF))
        .unwrap_or_else(|error| panic!("`{site}`: the child left no handoff: {error}"));
    Run::adopt(PathBuf::from(root))
}

pub(super) fn kill_child_environment() -> (PathBuf, String) {
    (
        PathBuf::from(std::env::var("UPSTROKE_TEST_KILL_DIR").expect("UPSTROKE_TEST_KILL_DIR")),
        std::env::var("UPSTROKE_TEST_KILL_SITE").expect("UPSTROKE_TEST_KILL_SITE"),
    )
}

pub(super) const OUTCOME: RunOutcome = RunOutcome::Complete;
