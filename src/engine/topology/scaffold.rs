//! The schema-4 run a dispatch or attempt test drives.
//!
//! Shared by [`super::dispatch`] and [`super::attempt`] because the two halves
//! of one lifecycle are tested against one run: an attempt test needs a
//! dispatched generation, and a dispatch test needs the attempt that never
//! started. A second run fixture beside this one would be two hand-maintained
//! copies of a `run_started` record — the class this crate has recorded three
//! times.
//!
//! # Why the effects come from `workspace_manager::fixture`
//!
//! `src/engine/topology/**` is a topology module. `clippy.toml` denies
//! `std::fs::write`, `std::fs::create_dir_all` and `std::process::Command`
//! there, **including in `#[cfg(test)]` code** — measured. Everything this
//! module needs that no funnel owns (`git init`, bytes in a worktree, a child
//! to kill) therefore comes from
//! [`crate::workspace_manager::fixture`], which is inside the reviewed funnel
//! module. Nothing here carries an `allow`.

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
    fixture::{self, Fixture, died_by_abort, run_child_test, write_file},
};

use super::attempt::{AttemptPlan, GatePlan, ReviewerPlan};
use super::dispatch::{DispatchKind, DispatchRequest, Dispatched, EventEmitter, dispatch};

/// The two plan tasks every fixture run carries.
pub(super) const ALPHA: TaskKey = TaskKey(0);
/// The second, so an assertion about "this task" can be crossed against
/// another whose generations move independently.
pub(super) const BETA: TaskKey = TaskKey(1);

/// The agents this fixture's pre-flight probed.
pub(super) const AGENT: &str = "claude-code";
/// A second, so a slot pair taken for the worker is distinguishable from one
/// taken for a reviewer.
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

/// The digest the fold authenticates the normalized plan against. A literal
/// because this fixture never writes a `plan.normalized.json`; the fold
/// compares it to `run_started`'s own field and to nothing on disk.
const NORMALIZED_DIGEST: &str =
    "sha256:1010101010101010101010101010101010101010101010101010101010101010";

/// A `run_started` for a real repository: the execution root, the private root
/// and the base commit are the fixture's own, so an event this run appends is
/// checkable against the directory the funnels actually touched.
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
            version: PathPolicyVersion::V1,
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

// ---------------------------------------------------------------------------
// The emitter
// ---------------------------------------------------------------------------

/// `coordinator_integration.emit`, minus the append-error protocol.
///
/// "build event → serialize → round-trip → `plan_transition` → append the exact
/// bytes through the Event funnel", which is what an emit *is* when it
/// succeeds. The protocol for when it does not is `emit.rs`'s (O17) and is
/// deliberately absent: a second implementation of it living in a test fixture
/// would be a second thing to keep in step with the one that ships.
///
/// Owns its own [`crate::events::log::HarnessEventHooks`] over the **shared**
/// harness rather than borrowing the effect bundle's, so an ordering assertion
/// can read the append and the worktree add off one observation list while the
/// two values are borrowed independently.
pub(super) struct FoldedEmitter {
    log: EventLog,
    fold: TopologyFold,
    hooks: Box<dyn EventHooks>,
    clock: super::seams::SystemClock,
}

impl FoldedEmitter {
    /// The fold, for the state an event is supposed to have produced.
    pub(super) fn fold(&self) -> &TopologyFold {
        &self.fold
    }

    /// Every event in the log on disk, replayed from its bytes.
    pub(super) fn durable_events(&self) -> Vec<TopologyEvent> {
        let bytes = std::fs::read(self.log.path()).expect("read the log back");
        TopologyFold::parse_log(&bytes).expect("the log parses")
    }

    /// The kinds in the log on disk, in order.
    pub(super) fn durable_kinds(&self) -> Vec<&'static str> {
        self.durable_events()
            .iter()
            .map(|event| event.body.kind())
            .collect()
    }

    /// One task's fold state.
    pub(super) fn task(&self, key: TaskKey) -> &TaskFold {
        self.fold.task(key).expect("the task is registered")
    }

    /// The class of the generation `generation` of `key`.
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
    /// **`_hooks` is ignored, and that is the divergence rather than an
    /// oversight.** This emitter's own `EventHooks` is a `TimelineEvents`,
    /// which records each `(site, phase)` into the ordering timeline as well as
    /// into the harness. The shared bundle's `events` family is a bare
    /// `HarnessEventHooks` and does not. Using the parameter here would
    /// silently drop every append out of the timeline, and the ordering
    /// assertions that read it would go green having stopped observing the
    /// thing they order.
    ///
    /// The repair is to give the shared bundle the timeline wrapper, not to
    /// take it away from here — but that is a change to test infrastructure
    /// every topology test depends on, which is the shape PR5's round 7 was
    /// reverted for. Recorded instead.
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

// ---------------------------------------------------------------------------
// The hook bundle, with the two phases the shared harness cannot arm
// ---------------------------------------------------------------------------

/// Every `(site, phase)` any funnel reached, in order, with repeats.
///
/// The [`HookHarness`] cannot answer an ordering question on its own, and this
/// is not a defect in it: `coverage()` is a **set** in first-observation order,
/// because what it exists to prove is that every site executed at least once.
/// An ordering clause is about *occurrences* — O24 is "verification, then the
/// retry's append", and by the time a retry runs, both sites have already been
/// observed once by the dispatch that opened the generation, so a comparison of
/// first observations is a comparison of the wrong pair and passes or fails for
/// the wrong reason. Measured: it failed here, on a `retry` whose order was
/// right.
///
/// So the timeline is a second, ordered record kept beside the harness, fed by
/// the same calls. The harness still receives everything — nothing here
/// replaces an observation, it only adds one — so the coverage evidence is
/// unaffected.
#[derive(Clone, Default)]
pub(super) struct Timeline(Arc<Mutex<Vec<(EffectSiteId, HookPhase)>>>);

impl Timeline {
    fn push(&self, site: EffectSiteId, phase: HookPhase) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((site, phase));
    }

    /// Every position at which `(site, phase)` was reached.
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

    /// How much has happened so far — a fence a later assertion counts from.
    pub(super) fn mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

/// [`EventHooks`] that record into the shared harness and onto the timeline.
///
/// `EventHooks::phase` returns nothing — for that family the two phases are
/// reachability rather than injection points — so this adds no arming, only the
/// ordered record an append's position in a clause needs.
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

/// [`EffectHooks`] that record into the shared [`HookHarness`] **and** can be
/// armed at a `Before` or `After` phase.
///
/// [`HookHarness::arm`] takes a [`SubEffectPoint`], and `HookHarness::hook`
/// answers `Proceed` to both phases unconditionally — deliberately: a phase is
/// reachability, not an injection coordinate. But `T-DISPATCH` and `T-ATTEMPT`
/// table prefixes that are exactly "between these two effects", and the only
/// place to stand between two funnels is a phase of one of them.
///
/// So the arming is local and the **recording is not**: every call reaches
/// `HarnessEffects` first, so the observation lands in the one harness
/// `check_bijection` reads, and only the answer is this type's.
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

    /// Answer `injection` the next time `site` reaches `phase`.
    pub(super) fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {
        self.armed.push((site, phase, injection));
    }
}

impl EffectHooks for ArmedEffects {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        // Recorded first and unconditionally: an armed site is still a site the
        // suite executed, and a bundle that skipped the harness when it had an
        // answer of its own would drop exactly the observations the fault tests
        // produce.
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

    // Forwarded, so a poison the inner observer found is reported as poison
    // and not as a fault this bundle armed; the method is not defaulted for
    // exactly this reason.
    fn refusal_cause(&self) -> Option<String> {
        self.inner.refusal_cause()
    }
}

/// The five families, with [`ArmedEffects`] in the git seat.
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

    /// Arm a phase of a git funnel site.
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

// ---------------------------------------------------------------------------
// The runner double
// ---------------------------------------------------------------------------

/// One request the fake runner was given.
#[derive(Debug, Clone)]
pub(super) struct Ran {
    /// Which identity it carried.
    pub(super) invocation: InvocationId,
    /// Which seat it occupied.
    pub(super) role: ExecutionRole,
    /// Where it ran.
    pub(super) workspace: PathBuf,
    /// The agent it was bound to, if any.
    pub(super) agent: Option<AgentId>,
    /// Its program and arguments.
    pub(super) command: CommandSpec,
    /// The event kinds the log **on disk** held at the instant this process was
    /// requested.
    ///
    /// The only oracle O23 has. "`attempt_started` before spawn" is a claim
    /// about two things that happen at two moments, and every other record here
    /// is read *after* both — so a `start` that spawned first and appended
    /// afterwards leaves an identical `Ran`, an identical durable log and an
    /// identical fold. Measured: with the append moved after the spawn, the
    /// whole of this test stayed green until this field existed.
    pub(super) durable_at_spawn: Vec<String>,
}

/// A [`Runner`] that runs nothing and records everything.
///
/// The engine is the conductor: it never implements an agentic loop and never
/// calls a model. What a test of *ordering* needs from the runner is therefore
/// not an execution but a record — which identity, which seat, which workspace
/// — and the workspace is the load-bearing one here, because
/// `decisions.workspace_candidates.snapshots` says gates and reviewers execute
/// only in exact snapshots and "worker worktrees and the staging worktree are
/// never used for verification processes".
/// What a refused scaffold process prints, so a test can follow it into the
/// feedback a retry is given.
pub(super) const GATE_DIAGNOSTIC: &str = "scaffold gate rejected the diff";

#[derive(Debug, Default)]
pub(super) struct RecordingRunner {
    ran: Mutex<Vec<Ran>>,
    /// Exit codes to hand back, in order. Exhausted entries answer 0.
    codes: Mutex<Vec<i32>>,
    /// The run's event log, read at the instant of each request.
    log: Mutex<Option<PathBuf>>,
}

impl RecordingRunner {
    /// A runner every process succeeds under.
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Replace the queued exit codes on a runner already in a `Run`.
    ///
    /// `failing_with` builds one; a test that needs the fixture's whole run and
    /// only wants different codes cannot rebuild it, because the `Run` owns the
    /// runner and its harness.
    pub(super) fn set_codes(&self, codes: Vec<i32>) {
        *self
            .codes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = codes;
    }

    /// Hand back these exit codes, in order, then zeroes.
    pub(super) fn failing_with(codes: Vec<i32>) -> Self {
        Self {
            ran: Mutex::new(Vec::new()),
            codes: Mutex::new(codes),
            log: Mutex::new(None),
        }
    }

    /// Read `log` at the instant of every request, so an ordering clause about
    /// "before any spawn" has something to be true *at*.
    pub(super) fn watching(&self, log: &Path) {
        *self
            .log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(log.to_path_buf());
    }

    /// The kinds the log on disk holds right now.
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

    /// Everything it was asked to run, in order.
    pub(super) fn ran(&self) -> Vec<Ran> {
        self.ran
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Runner for RecordingRunner {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        // Read before the request is recorded, so what it captures is the log
        // as it stood when the process was asked for.
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
            // A refused process says something, the way a real one does: §11.1
            // makes the tail the feedback a retry is given, and a fixture whose
            // processes print nothing cannot tell a carried tail from a dropped
            // one.
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

// ---------------------------------------------------------------------------
// The agent boundary, doubled
// ---------------------------------------------------------------------------

/// An agent CLI that answers, without an agent CLI.
///
/// **A double, not a re-implementation.** It implements the real
/// [`AgentAdapter`] trait, builds a real [`CommandSpec`], and every invocation
/// it produces is spawned through [`RecordingRunner`] — so what the tests
/// observe is the engine's own request, at the engine's own boundary. Nothing
/// here re-implements what an adapter *does*; it stands in for what an adapter
/// *talks to*.
///
/// It exists because the scaffold used to name real adapter ids —
/// `claude-code` and `copilot` — which `BuiltinAdapters` resolves, sending
/// `run_review` off to locate an actual CLI that is not there. A fixture that
/// points at a real boundary and hopes is the same defect as a fixture that
/// invents a shape production never builds, arriving from the other side.
///
/// `review.rs`'s three private fakes were considered and are the wrong shape:
/// `NeverInvokedAdapter` panics on every method by design, and the other two
/// model an outage and a deadline. All three are **negative-case** doubles, and
/// teaching one to answer would change what it means for the tests that own it.
pub(super) struct AnsweringAdapter {
    id: &'static str,
    verdict: &'static str,
    /// What this agent reports about its own run.
    ///
    /// A field rather than a constant because an **outage** is a distinct path
    /// through the ladder and needs a fixture that reaches it: `RateLimited` is
    /// what `AttemptFailure::is_outage` recognises, and it is the difference
    /// between an attempt that spends one of its rung's allowances and one that
    /// defers spending none.
    status: crate::ir::OutcomeStatus,
}

impl AnsweringAdapter {
    /// A reviewer that passes.
    /// An agent whose CLI reports its own error.
    ///
    /// `FailureKind::AgentError` is neither an outage nor a question, so
    /// `next_step` retries on the same rung while the allowance lasts — and
    /// `resume: true` when the agent can resume and returned a session, which
    /// is what makes the generation `Retained` rather than closed.
    pub(super) const fn erroring(id: &'static str) -> Self {
        Self {
            status: crate::ir::OutcomeStatus::AgentError,
            ..Self::passing(id)
        }
    }

    /// An agent that stops and asks rather than working.
    ///
    /// `evaluate_outcome` reads `UPSTROKE-QUESTION:` out of the outcome's
    /// detail **before** the evidence rules, because "an agent that stopped to
    /// ask has not failed at anything". That is `FailureKind::NeedsHuman`,
    /// which `next_step` sends straight to a park.
    pub(super) const fn asking(id: &'static str) -> Self {
        Self {
            verdict: "UPSTROKE-QUESTION: the spec names two incompatible \
                      formats and I should not pick one alone",
            ..Self::passing(id)
        }
    }

    /// An agent whose CLI reports it is rate-limited: `evaluate_outcome` maps
    /// that to `FailureKind::RateLimited`, which `is_outage` recognises and
    /// `next_step` defers rather than blames on the implementer.
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
        // A real spec, carrying the prompt, so a test that reads the recorded
        // command sees what was actually asked for.
        //
        // **And the session, when there is one to resume.** Every real adapter
        // puts it in argv; one that dropped it here would make a retry that
        // lost its session indistinguishable from one that kept it, which is a
        // fixture blind spot rather than a simplification — measured, by a
        // mutation that survived until this line existed.
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
        // `detail` is where `run_review` reads the verdict from, and `cost_usd`
        // is what `ReviewRecord` requires — both come from the adapter because
        // both are things only the agent's own CLI knows.
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

/// The scaffold's adapters: the two the fixture's plans name, and nothing else.
///
/// Deliberately not `BuiltinAdapters`. An unknown agent must be a refusal a
/// test can see, not a silent fall-through to a real CLI.
pub(super) struct ScaffoldAdapters {
    primary: AnsweringAdapter,
    second: AnsweringAdapter,
}

impl ScaffoldAdapters {
    /// The same two agents, with the implementer reporting its own error.
    pub(super) const fn erroring() -> Self {
        Self {
            primary: AnsweringAdapter::erroring(AGENT),
            second: AnsweringAdapter::passing(REVIEW_AGENT),
        }
    }

    /// The same two agents, with the implementer stopping to ask.
    pub(super) const fn asking() -> Self {
        Self {
            primary: AnsweringAdapter::asking(AGENT),
            second: AnsweringAdapter::passing(REVIEW_AGENT),
        }
    }

    /// The same two agents, with the implementer reporting a rate limit.
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

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// A real repository, a real event log, a fold over it, and the five hook
/// families on one harness.
pub(super) struct Run {
    /// The repository and its manager.
    pub(super) fixture: Fixture,
    /// The run's directories, for the seams that write under them.
    pub(super) paths: crate::rundir::RunPaths,
    /// The one harness every family records into.
    pub(super) harness: Arc<Mutex<HookHarness>>,
    /// The five families.
    pub(super) hooks: Hooks,
    /// Every `(site, phase)` in order, with repeats.
    pub(super) timeline: Timeline,
    /// The log, the fold, and the emit sequence over them.
    pub(super) emitter: FoldedEmitter,
    /// What a spawn would have been.
    pub(super) runner: RecordingRunner,
    /// The R4 ledger this fixture discharges obligation (3) against. In
    /// production the driver owns it; here the fixture is the caller.
    pub(super) invocations: crate::engine::topology::identity::InvocationLedger,
}

impl Run {
    /// A started schema-4 run over a fresh repository.
    pub(super) fn started(tag: &str) -> Self {
        // Under the parent's tree when this is a kill child, so what an abort
        // leaves behind is still owned; under the temporary directory
        // otherwise, which is every ordinary test.
        let fixture = match std::env::var_os(KILL_SCRATCH) {
            Some(parent) => Fixture::created_under(Path::new(&parent)),
            None => Fixture::created(tag),
        };
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
                // `RunPaths`'s own doc: "Callers do this once at run start;
                // every accessor below assumes it has happened." The scaffold
                // was handing out paths without it, so a review's transcript
                // write failed into `unavailable_after_error` and the pass was
                // recorded as an OUTAGE — which spends no attempt. A fixture
                // that skips a documented precondition does not fail loudly;
                // it produces a plausible wrong answer.
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

    /// The manager.
    pub(super) fn manager(&self) -> &WorkspaceManager {
        &self.fixture.manager
    }

    /// The base every dispatch of this fixture is made at.
    pub(super) fn base(&self) -> CommitSha {
        CommitSha(self.fixture.head.clone())
    }

    /// The predicted region an ordinary dispatch of `key` takes.
    ///
    /// **Read off the fold, not restated.** It answered `RepoWide` for every
    /// task while the fixture's entries freeze `src/{id}/` hints, so every
    /// dispatch this scaffold emitted recorded a region the fold did not
    /// derive — the exact disagreement `check_dispatched` now refuses. A
    /// literal here would be a second derivation of the run's own rule, which
    /// is what let the two drift in the first place.
    pub(super) fn predicted(&self, key: TaskKey) -> PathSet {
        self.emitter
            .fold()
            .predicted_region(key)
            .expect("the scaffold's run has started, so its registry answers")
    }

    /// Whether the harness saw `site` at `phase`.
    pub(super) fn observed(&self, site: EffectSiteId, phase: HookPhase) -> bool {
        self.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observed(site, phase)
    }

    /// Where `(site, phase)` **first** appears on the timeline, or `None` if
    /// nothing drove it.
    ///
    /// This is how an ordering clause over a prefix that runs once is asserted:
    /// every family records onto one timeline, so an append and a
    /// `git worktree add` are two positions in one list.
    pub(super) fn order_of(&self, site: EffectSiteId, phase: HookPhase) -> Option<usize> {
        self.timeline.positions(site, phase).first().copied()
    }

    /// [`Self::order_of`], or a panic naming what never ran.
    pub(super) fn must_order_of(&self, site: EffectSiteId, phase: HookPhase) -> usize {
        self.order_of(site, phase)
            .unwrap_or_else(|| panic!("nothing drove `{site}` at its `{phase}` phase"))
    }

    /// Everything that has happened so far, as a fence.
    ///
    /// A clause about a *second* occurrence — O24's retry runs in a generation
    /// whose dispatch already drove both of its sites — is asserted from a mark
    /// taken before the step, so the positions compared are the step's own.
    pub(super) fn mark(&self) -> usize {
        self.timeline.mark()
    }

    /// The first position at or after `mark` at which `(site, phase)` ran.
    pub(super) fn order_after(&self, mark: usize, site: EffectSiteId, phase: HookPhase) -> usize {
        self.timeline
            .positions(site, phase)
            .into_iter()
            .find(|position| *position >= mark)
            .unwrap_or_else(|| {
                panic!("nothing drove `{site}` at its `{phase}` phase after position {mark}")
            })
    }

    /// How many times `(site, phase)` ran at or after `mark`.
    pub(super) fn count_after(&self, mark: usize, site: EffectSiteId, phase: HookPhase) -> usize {
        self.timeline
            .positions(site, phase)
            .into_iter()
            .filter(|position| *position >= mark)
            .count()
    }

    /// Arm `injection` at a phase of a git funnel site.
    pub(super) fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {
        self.hooks.arm(site, phase, injection);
    }

    /// Arm a parent-side sub-effect point on the shared harness, which is where
    /// a point genuinely belongs.
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

    /// One task's state.
    pub(super) fn task_state(&self, key: TaskKey) -> TaskState {
        self.emitter.task(key).state
    }
}

impl Run {
    /// Re-open the run a kill child left behind, and replay its log.
    ///
    /// This is `recover.rs`'s shape reduced to what `T-DISPATCH` and
    /// `T-ATTEMPT` need: open the log through `Event.OpenLog` (which truncates
    /// a torn tail), parse the surviving bytes, and replay them. It is
    /// deliberately **not** a call into the recovery order — that order is
    /// another lane's and this fixture may not be a second implementation of
    /// it. What it is is the smallest thing that makes the child's durable log
    /// readable, so an assertion can be about the log rather than about a
    /// message the child never got to send.
    pub(super) fn adopt(root: PathBuf, owner: crate::rundir::scratch_tree::ScratchTree) -> Self {
        let fixture = Fixture::adopt(root, owner);
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
                // `RunPaths`'s own doc: "Callers do this once at run start;
                // every accessor below assumes it has happened." The scaffold
                // was handing out paths without it, so a review's transcript
                // write failed into `unavailable_after_error` and the pass was
                // recorded as an OUTAGE — which spends no attempt. A fixture
                // that skips a documented precondition does not fail loudly;
                // it produces a plausible wrong answer.
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

    /// Tell the parent where this child's repository is.
    ///
    /// Written **before** anything is armed, so a child that dies at its first
    /// site still hands over a readable pointer. The parent has no other way to
    /// learn it: the scratch directory is keyed by the child's own process id.
    pub(super) fn hand_off(&self, dir: &Path) {
        write_file(
            &dir.join(HANDOFF),
            self.fixture.root.to_string_lossy().as_bytes(),
        );
    }

    /// An ordinary dispatch of `key` at this fixture's head.
    pub(super) fn dispatch(&mut self, key: TaskKey, generation: u32) -> Dispatched {
        self.try_dispatch(key, generation).expect("dispatch")
    }

    /// [`Self::dispatch`], keeping the error.
    ///
    /// Obligation (3) is discharged against a ledger of this fixture's own:
    /// `dispatch` emits without holding one, so the failure carries the
    /// obligation out, and something has to be the caller. In production that
    /// is `TopologyRun`, which owns the run's ledger.
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

    /// Register a repair of `root`, the way a merge rejection will.
    ///
    /// PR9 owns the production producer; what this needs to be is an entry the
    /// **fold** accepts, so that a repair dispatch is checked by the same rules
    /// a real one will be. Everything but the identity is cloned from the root's
    /// own entry — ladder, reviews, allowed agents — because every one of those
    /// is a value `check_spawn` compares against the run header, and inventing
    /// them here would be inventing a way to fail.
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

/// The session a retained settlement holds, so a retry has one to resume.
pub(super) const RETAINED_SESSION: &str = "session-01SCAFFOLD";

impl Run {
    /// The binding rung `rung` of `key`'s frozen ladder gives.
    ///
    /// Read out of the registry rather than written here, because
    /// `check_attempt_started` compares `attempt_started`'s binding against
    /// exactly this value (INV-19) and a fixture that spelled it out would be
    /// asserting the fold against a literal instead of against the ladder.
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

    /// One attempt of `key`: a worker, one gate, two reviewers.
    ///
    /// Two reviewers rather than one, because
    /// `decisions.workspace_candidates.snapshots` requires "one **fresh**
    /// snapshot per reviewer, never reused across roles or attempts", and a
    /// single reviewer cannot distinguish a fresh snapshot from a reused one.
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
            // Through the production assembler, not invented here. A fixture
            // that built its own `(command, timeout)` pair would be a second
            // derivation of the one thing `ShellGate::command` exists to be —
            // the `frozen_binding` precedent, where a fixture repeating a
            // production composition kept a fifth copy of it alive.
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
            // Identity and policy, no command: the shared review machinery
            // builds one per invocation, because a re-ask's prompt is not the
            // first pass's. A fixture carrying a pre-built command would be a
            // pass shape production never builds.
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

    /// What every review pass of one scaffold attempt reads.
    ///
    /// Owned fixture data rather than a plan shape invented here: a review that
    /// could not be produced from these inputs would be a pass shape production
    /// never builds.
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

    /// Settle the in-flight attempt as `Retained`, so the generation becomes
    /// `RetainedIdle` and a same-session retry is admissible.
    ///
    /// `settle.rs` owns this transition in production; what is needed here is
    /// only the *state*, so the event is emitted through the same fold-checked
    /// emitter every other event uses and is refused if it is not a transition
    /// the fold allows.
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
                            // **A retained attempt did not succeed.**
                            // `settle::settle_failed` is the only producer of a
                            // `Retained` settlement and it is reached on the
                            // failure path, so production's record always
                            // carries this. This fixture recorded `failure:
                            // None` with no reviews — a record every other door
                            // in the fold calls *successful* — which is the
                            // shape `check_attempt_finished`'s retained arm now
                            // refuses.
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

/// The file a kill child writes its repository root into.
const HANDOFF: &str = "fixture-root";

/// The tree a kill child must build its fixture under.
///
/// **This protocol's variable, read here and nowhere else.** The parent
/// acquires the tree before the child exists and keeps the guard, so a child
/// that dies by `std::process::abort()` — which is every child this protocol
/// runs — leaves a subtree the parent still owns and reclaims. It is not read
/// inside `workspace_manager::fixture`, which takes a path argument instead:
/// that module's whole claim this round is that it trusts no ambient input,
/// and a variable it read would be that claim's own counterexample.
const KILL_SCRATCH: &str = "UPSTROKE_TEST_KILL_SCRATCH";

/// A directory this process owns, unique to this call, for one kill test.
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

/// Run `test` as a child that must die, and adopt the run it left behind.
///
/// The `unreachable!` inside the child is what fails the test when an
/// injection stops killing; this end asserts the other half — that the process
/// really did not exit successfully. Both are needed: a child that returned
/// early would satisfy neither, and a child that panicked would satisfy only
/// this one.
pub(super) fn kill_child_and_adopt(test: &str, dir: &Path, site: &str) -> Run {
    // Minted here, before the child exists, because a token cannot be made
    // from a path: the child builds its own subtree inside this one, and a
    // child that dies by `std::process::abort()` leaves a tree this guard
    // still owns. Without it the adopted tree outlived every kill test, and a
    // temporary directory leaked per fixture is what exhausted this box's
    // inodes once already.
    // Under the temporary directory and not under `dir`: every component here
    // is a prefix of the manager's worktree paths inside the child, and the
    // Windows guest refuses `git worktree add` with `Filename too long` once
    // they run long. A short tag, one level.
    let owner = fixture::scratch_tree("kc");
    let status = run_child_test(
        test,
        &[
            ("UPSTROKE_TEST_KILL_DIR", dir.as_os_str()),
            ("UPSTROKE_TEST_KILL_SITE", std::ffi::OsStr::new(site)),
            (KILL_SCRATCH, owner.path().as_os_str()),
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
    Run::adopt(PathBuf::from(root), owner)
}

/// The directory and site a kill child is given.
pub(super) fn kill_child_environment() -> (PathBuf, String) {
    (
        PathBuf::from(std::env::var("UPSTROKE_TEST_KILL_DIR").expect("UPSTROKE_TEST_KILL_DIR")),
        std::env::var("UPSTROKE_TEST_KILL_SITE").expect("UPSTROKE_TEST_KILL_SITE"),
    )
}

/// The run outcome a run-end closure records in these tests.
pub(super) const OUTCOME: RunOutcome = RunOutcome::Complete;
