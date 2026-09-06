//! Extended notes: `docs/internals/engine/topology/candidate/tests.md`

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::agent::ProcessOutput;
use crate::events::log::{EventLog, TopologyLine, site_for};
use crate::events::{BindingSummary, ChainSummary};
use crate::events::{GateSummary, ReviewPassOutcome, ReviewRecord};
use crate::gates::ShellKind;
use crate::ir::{Effort, Plan, PlanSource, ResolvedEffortPolicy, Task, TaskId, TaskKind, Tier};
use crate::review::{PassBinding, ReviewPlan};
use crate::runner::container::view::fixtures as git_fixtures;
use crate::runner::invocation::AttemptRole;
use crate::runner::{CommandSpec, InvocationId, Runner, gate_request, host::HostRunner};
use crate::topology::effects::{
    EventSite, HookHarness, HookPhase, Injection, InjectionMode, SnapshotSite, SubEffectPoint,
    WorktreeSite,
};
use crate::topology::events::{
    AttemptNumber, IncarnationId, LeaseGrant, RungBinding, TaskDispatched, TopologyEvent,
};
use crate::topology::events::{AttemptStarted4, RunStarted4, TopologyLimits};
use crate::topology::fold::{FrozenInputs, TopologyFold};
use crate::topology::paths::{GitPath, PathGrammar, PathPolicy, PathPolicyVersion};
use crate::topology::registry::TaskRegistry;
use crate::topology::schema::TOPOLOGY_SCHEMA;
use crate::util::DurabilityLedger;
use crate::workspace_manager::{
    EffectHooks, HarnessEffects, ObjectId, SnapshotInput, SnapshotName, unreachable_objects,
};

const RUN_ID: &str = "01KZCAND00000000000000000G";
const INCARNATION: &str = "01KZCANDINC0000000000000AG";
const ALPHA: TaskKey = TaskKey(0);
const GENERATION: GenerationId = GenerationId(0);
const FIXED_TS: &str = "2026-08-23T11:22:33Z";
const NORMALIZED_DIGEST: &str =
    "sha256:7777777777777777777777777777777777777777777777777777777777777777";

const ENV_BASE: &str = "UPSTROKE_TEST_CAND_BASE";
const ENV_PRIVATE: &str = "UPSTROKE_TEST_CAND_PRIVATE";
const ENV_SITE: &str = "UPSTROKE_TEST_CAND_SITE";

fn make_dir(path: &Path) {
    crate::rundir::create_public_dir(path, &mut crate::rundir::NoHooks)
        .expect("the scratch directory");
}

fn drop_dir(path: &Path) {
    let _ = crate::rundir::remove_public_husk(path, &mut crate::rundir::NoHooks);
}

struct Fixture {
    root: PathBuf,
    base: PathBuf,
    private: PathBuf,
    manager: WorkspaceManager,
    base_sha: CommitSha,
    task: Slot,
    tree_sha: CommitSha,
}

impl Fixture {
    fn at(root: PathBuf) -> Self {
        let base = root.join("repo");
        let private = root.join("private");
        make_dir(&private);
        let (head, _previous) = git_fixtures::repository(&base);

        let manager = WorkspaceManager::derive(&base, &private, RUN_ID, INCARNATION)
            .expect("derive the manager");
        manager
            .create_execution_root(&mut crate::workspace_manager::NoHooks)
            .expect("create the execution root");

        let task = Slot::Task {
            key: "alpha".to_owned(),
            generation: GENERATION.0,
        };
        manager
            .write_intent(&mut crate::workspace_manager::NoHooks, &task)
            .expect("the task intent");
        let worktree = manager
            .add_worktree(&mut crate::workspace_manager::NoHooks, &task, &head)
            .expect("the task worktree");

        let blob = git_fixtures::git_ok(
            &worktree,
            &["hash-object", "-w", "--stdin", "--path", "worker.txt"],
        );
        git_fixtures::git_ok(
            &worktree,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{blob},worker.txt"),
            ],
        );
        let tree = manager
            .candidate_write_tree(&mut crate::workspace_manager::NoHooks, &task)
            .expect("write-tree");

        Self {
            root,
            base,
            private,
            manager,
            base_sha: CommitSha(head),
            task,
            tree_sha: CommitSha(tree),
        }
    }

    fn new(tag: &str) -> Self {
        Self::at(git_fixtures::scratch(&format!("cand-{tag}")))
    }

    fn judged(&self) -> JudgedTree {
        JudgedTree {
            key: ALPHA,
            generation: GENERATION,
            attempt: Box::new(attempt_record()),
            base_sha: self.base_sha.clone(),
            tree_sha: self.tree_sha.clone(),
            message: "alpha: the judged tree".to_owned(),
            actual_paths: region(),
            lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region() },
        }
    }

    fn divergent_tree_commit(&self, hooks: &mut Hooks) -> (CommitSha, CommitSha) {
        let worktree = self.manager.slot_path(&self.task);
        let blob = git_fixtures::git_ok(
            &worktree,
            &["hash-object", "-w", "--stdin", "--path", "smuggled.txt"],
        );
        let blob = blob.trim().to_owned();
        git_fixtures::git_ok(
            &worktree,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{blob},smuggled.txt"),
            ],
        );
        let tree = self
            .manager
            .candidate_write_tree(&mut crate::workspace_manager::NoHooks, &self.task)
            .expect("write-tree");
        assert_ne!(
            tree, self.tree_sha.0,
            "the fixture wrote the same tree twice, so this commit is not divergent"
        );
        let judged = JudgedTree {
            message: "alpha: a tree nobody judged".to_owned(),
            tree_sha: CommitSha(tree.clone()),
            ..self.judged()
        };
        let commit = write_candidate_commit(&self.manager, hooks, RUN_ID, judged)
            .expect("commit-tree")
            .commit_sha()
            .clone();
        (commit, CommitSha(tree))
    }

    fn sibling_commit(&self, hooks: &mut Hooks) -> CommitSha {
        let judged = JudgedTree {
            message: "alpha: a sibling of the judged tree".to_owned(),
            ..self.judged()
        };
        write_candidate_commit(&self.manager, hooks, RUN_ID, judged)
            .expect("commit-tree")
            .commit_sha()
            .clone()
    }

    fn unreachable(&self) -> Vec<String> {
        unreachable_objects(&self.base).expect("fsck")
    }

    fn is_unreachable(&self, object: &str) -> bool {
        self.unreachable().iter().any(|id| id == object)
    }

    fn unreachable_commits(&self) -> Vec<String> {
        self.unreachable()
            .into_iter()
            .filter(|id| {
                git_fixtures::git_ok(&self.base, &["cat-file", "-t", id]).trim() == "commit"
            })
            .collect()
    }

    fn object_present(&self, object: &str) -> bool {
        git_fixtures::git(
            &self.base,
            &["cat-file", "-e", &format!("{object}^{{commit}}")],
        )
        .code
            == Some(0)
    }

    fn run_refs(&self) -> Vec<(String, String)> {
        self.manager
            .refs_under(&run_namespace(RUN_ID))
            .expect("for-each-ref")
    }

    fn journal(&self, hooks: &Hooks) -> Journal {
        Journal::open(&self.private, self.base_sha.clone(), hooks)
    }

    fn task_admin_dir(&self) -> PathBuf {
        self.manager
            .common_git_dir()
            .join("worktrees")
            .join("kalpha-g0")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop_dir(&self.root);
    }
}

struct Journal {
    log: EventLog,
    fold: TopologyFold,
    path: PathBuf,
    hooks: TracedEvents,
}

impl Journal {
    fn open(private: &Path, base_sha: CommitSha, hooks: &Hooks) -> Self {
        let path = private.join("events.jsonl");
        let mut warnings = Vec::new();
        let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings)
            .expect("open the schema-4 log");
        assert!(warnings.is_empty(), "a fresh log warns about nothing");
        let mut journal = Self {
            log,
            fold: TopologyFold::new(inputs()),
            path,
            hooks: hooks.events.clone(),
        };
        journal
            .emit(TopologyEventBody::RunStarted {
                data: Box::new(run_started(base_sha.clone())),
            })
            .expect("run_started");
        journal
            .emit(TopologyEventBody::TaskDispatched {
                data: TaskDispatched {
                    key: ALPHA,
                    generation: GENERATION,
                    base_sha,
                    worktree_path: "tasks/kalpha-g0".to_owned(),
                    lease: LeaseGrant::Predicted { paths: region() },
                    source_candidate: None,
                },
            })
            .expect("task_dispatched");
        let binding = journal.binding();
        journal
            .emit(TopologyEventBody::AttemptStarted {
                data: AttemptStarted4 {
                    key: ALPHA,
                    generation: GENERATION,
                    attempt: AttemptNumber(1),
                    rung: 0,
                    binding,
                    pool: None,
                    resume_session: None,
                    materialization_observed: None,
                },
            })
            .expect("attempt_started");
        journal
    }

    fn resume(private: &Path, hooks: &Hooks) -> Self {
        let path = private.join("events.jsonl");
        let bytes = std::fs::read(&path).expect("the log the dead run left");
        let events = TopologyFold::parse_log(&bytes).expect("the log parses");
        let fold = TopologyFold::replay(inputs(), &events).expect("the log replays");
        let mut warnings = Vec::new();
        let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings)
            .expect("reopen the schema-4 log");
        Self {
            log,
            fold,
            path,
            hooks: hooks.events.clone(),
        }
    }

    fn binding(&self) -> RungBinding {
        let registry = self.fold.registry().expect("the run has started");
        let entry = registry.get(ALPHA).expect("alpha is registered");
        let frozen = &entry.ladder.rungs[0];
        RungBinding::from_frozen(frozen, entry.ladder.effort.implementation_for(frozen.tier))
    }

    fn count(&self, kind: &str) -> usize {
        let bytes = std::fs::read(&self.path).expect("the log");
        TopologyFold::parse_log(&bytes)
            .expect("the log parses")
            .iter()
            .filter(|event| event.body.kind() == kind)
            .count()
    }

    fn generation_class(&self) -> Option<GenerationClass> {
        self.fold
            .task(ALPHA)
            .and_then(|task| task.generations.first())
            .map(|generation| generation.class.clone())
    }
}

impl CandidateJournal for Journal {
    fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError> {
        let event = TopologyEvent {
            ts: FIXED_TS.to_owned(),
            body,
        };
        let (line, checked) = TopologyLine::round_trip(&event)?;
        let delta =
            self.fold
                .plan_transition(&checked)
                .map_err(|error| UpstrokeError::Refused {
                    message: error.to_string(),
                })?;
        self.log
            .append_topology_hooked(site_for(&checked.body), &line, &mut self.hooks)?;
        self.fold.apply_delta(delta);
        Ok(())
    }

    fn fold(&self) -> &TopologyFold {
        &self.fold
    }
}

#[derive(Debug, Clone)]
struct ArmedEffects {
    inner: HarnessEffects,
    armed: Vec<(EffectSiteId, HookPhase, Injection)>,
    trace: Trace,
}

impl EffectHooks for ArmedEffects {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        let answered = self.inner.phase(site, phase);
        self.trace.push(site, phase);
        self.armed
            .iter()
            .find(|(armed, at, _)| *armed == site && *at == phase)
            .map_or(answered, |(_, _, injection)| *injection)
    }

    fn durability_ledger(&self) -> DurabilityLedger {
        self.inner.durability_ledger()
    }

    fn refusal_cause(&self) -> Option<String> {
        self.inner.refusal_cause()
    }
}

#[derive(Debug, Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

impl Trace {
    fn push(&self, site: EffectSiteId, phase: HookPhase) {
        if phase != HookPhase::Before {
            return;
        }
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(site.to_string());
    }

    fn reset(&self) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn order(&self, of_interest: &[EffectSiteId]) -> Vec<String> {
        let wanted: Vec<String> = of_interest.iter().map(ToString::to_string).collect();
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|seen| wanted.contains(seen))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone)]
struct TracedEvents {
    inner: crate::events::log::HarnessEventHooks,
    trace: Trace,
}

impl crate::events::log::EventHooks for TracedEvents {
    fn phase(&mut self, site: EventSite, phase: HookPhase) {
        self.inner.phase(site, phase);
        self.trace.push(EffectSiteId::Event(site), phase);
    }
}

struct Hooks {
    effects: ArmedEffects,
    events: TracedEvents,
    rest: crate::engine::topology::seams::HarnessTopologyHooks,
    harness: Arc<Mutex<HookHarness>>,
    trace: Trace,
}

impl Hooks {
    fn new() -> Self {
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let trace = Trace::default();
        Self {
            effects: ArmedEffects {
                inner: HarnessEffects::new(Arc::clone(&harness)),
                armed: Vec::new(),
                trace: trace.clone(),
            },
            events: TracedEvents {
                inner: crate::events::log::HarnessEventHooks::new(Arc::clone(&harness)),
                trace: trace.clone(),
            },
            rest: crate::engine::topology::seams::HarnessTopologyHooks::new(Arc::clone(&harness)),
            harness,
            trace,
        }
    }

    fn arm_phase(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {
        self.effects.armed.push((site, phase, injection));
    }

    fn arm_point(&mut self, site: EffectSiteId, point: SubEffectPoint, mode: InjectionMode) {
        self.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .arm(site, point, mode)
            .expect("the site exposes that point in that mode");
    }

    fn observed(&self, site: EffectSiteId, phase: HookPhase) -> bool {
        self.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observed(site, phase)
    }
}

impl TopologyHooks for Hooks {
    fn effects(&mut self) -> &mut dyn EffectHooks {
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

fn region() -> PathSet {
    PathSet::Prefixes {
        paths: vec![GitPath::from("worker.txt")],
    }
}

fn plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "candidate-frozen-hash".to_owned(),
        },
        tasks: vec![Task {
            id: TaskId::from("alpha"),
            kind: TaskKind::Refactor,
            title: "  alpha — Ünicode title  ".to_owned(),
            body: "alpha body".to_owned(),
            depends_on: Vec::new(),
            acceptance: vec!["alpha holds".to_owned()],
            path_hints: vec!["worker.txt".to_owned()],
            suggested_tier: Some(Tier::Mid),
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::new(),
        }],
        artifacts: Vec::new(),
    }
}

fn inputs() -> FrozenInputs {
    FrozenInputs {
        plan: plan(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
    }
}

fn chain() -> ChainSummary {
    ChainSummary {
        task: "alpha".to_owned(),
        attempts_per: 1,
        bindings: Some(vec![BindingSummary {
            tier: Tier::Mid,
            agent: "alpha-agent".to_owned(),
            model: "alpha-model".to_owned(),
            pinned: false,
        }]),
        tiers: vec![Tier::Mid],
    }
}

fn run_started(base_sha: CommitSha) -> RunStarted4 {
    let started = RunStarted4 {
        schema: TOPOLOGY_SCHEMA,
        upstroke_version: "0.2.0-candidate".to_owned(),
        run_id: RUN_ID.to_owned(),
        incarnation: IncarnationId(INCARNATION.to_owned()),
        runner: crate::runner::policy::host_policy(),
        probed_agents: vec!["alpha-agent".to_owned()],
        branch: format!("upstroke/run-{RUN_ID}"),
        integration_ref: GitRef(format!("refs/heads/upstroke/run-{RUN_ID}")),
        base_sha,
        execution_root: "/var/lib/upstroke/candidate roots".to_owned(),
        private_dir: "/var/lib/upstroke/candidate private".to_owned(),
        plan_path: "docs/Candidate Plan.md".to_owned(),
        config_path: None,
        plan_hash: "candidate-frozen-hash".to_owned(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
        registry_digest: String::new(),
        path_policy: PathPolicy {
            version: PathPolicyVersion::V2,
            case_fold: true,
            grammar: PathGrammar::Globset,
        },
        limits: TopologyLimits {
            max_parallel: 3,
            max_defers: 2,
            max_merge_repairs: 1,
        },
        gates: vec!["fmt".to_owned()],
        gates_from_config: false,
        gate_cmds: vec![GateSummary {
            name: "fmt".to_owned(),
            cmd: "cargo fmt --check".to_owned(),
            timeout: Duration::from_secs(451),
            shell: ShellKind::Bash,
        }],
        interaction_mode: "never".to_owned(),
        chains: vec![chain()],
        effort_policy: ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::High,
            frontier: Effort::Max,
            review: Effort::Medium,
        },
        reviews: ReviewPlan {
            enabled: Some(true),
            alternative_available: Some(false),
            pass_timeout_secs: Some(97),
            primary: Some(PassBinding::new("alpha-agent", "alpha-model")),
            alternative: None,
            second_opinion: vec![None],
        },
    };
    let digest = TaskRegistry::originals_with_agents(
        &plan(),
        &started.registry_record(),
        &started.probed_agents,
    )
    .expect("the fixture derives a registry")
    .digest();
    RunStarted4 {
        registry_digest: digest,
        ..started
    }
}

#[test]
#[ignore = "spawned as a subprocess by the T-CAND-OBJ kill tests"]
fn candidate_kill_child() {
    let base = PathBuf::from(std::env::var(ENV_BASE).expect("the base"));
    let private = PathBuf::from(std::env::var(ENV_PRIVATE).expect("the private root"));
    let which = std::env::var(ENV_SITE).expect("the site");

    let manager = WorkspaceManager::derive(&base, &private, RUN_ID, INCARNATION)
        .expect("the child derives the same manager");
    let task = Slot::Task {
        key: "alpha".to_owned(),
        generation: GENERATION.0,
    };
    let tree = manager
        .candidate_write_tree(&mut crate::workspace_manager::NoHooks, &task)
        .expect("the tree the parent staged");
    let head = git_fixtures::git_ok(&base, &["rev-parse", "HEAD"]);
    let judged = JudgedTree {
        key: ALPHA,
        generation: GENERATION,
        attempt: Box::new(attempt_record()),
        base_sha: CommitSha(head),
        tree_sha: CommitSha(tree),
        message: "alpha: the judged tree".to_owned(),
        actual_paths: region(),
        lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region() },
    };

    let mut hooks = Hooks::new();
    let pin = EffectSiteId::Ref(RefSite::PinCandidatePrepared);
    match which.as_str() {
        "id-unread" => {
            hooks.arm_point(
                EffectSiteId::Object(ObjectSite::CandidateCommitTree),
                SubEffectPoint::IdUnread,
                InjectionMode::Kill,
            );
            let _ = write_candidate_commit(&manager, &mut hooks, RUN_ID, judged);
        }
        "before-pin" => {
            let unpinned =
                write_candidate_commit(&manager, &mut hooks, RUN_ID, judged).expect("commit-tree");
            hooks.arm_phase(pin, HookPhase::Before, Injection::Kill);
            let _ = pin_candidate(&manager, &mut hooks, unpinned);
        }
        "after-pin" => {
            let unpinned =
                write_candidate_commit(&manager, &mut hooks, RUN_ID, judged).expect("commit-tree");
            hooks.arm_phase(pin, HookPhase::After, Injection::Kill);
            let _ = pin_candidate(&manager, &mut hooks, unpinned);
        }
        "after-prepared" => {
            let mut journal = Journal::open(&private, judged.base_sha.clone(), &hooks);
            let unpinned =
                write_candidate_commit(&manager, &mut hooks, RUN_ID, judged).expect("commit-tree");
            let pinned = pin_candidate(&manager, &mut hooks, unpinned).expect("pin");
            let promoting =
                append_candidate_prepared(&mut journal, pinned).expect("candidate_prepared");
            hooks.arm_phase(
                EffectSiteId::Ref(RefSite::CreateCandidates),
                HookPhase::Before,
                Injection::Kill,
            );
            let _ = complete_promotion(&manager, &mut hooks, &mut journal, &task, promoting);
        }
        other => panic!("unknown site `{other}`"),
    }
    unreachable!("the kill must have taken this process");
}

fn spawn_kill_child(fixture: &Fixture, which: &str) -> ProcessOutput {
    let exe = std::env::current_exe().expect("the test binary");
    let spec = CommandSpec::new(exe.to_string_lossy().into_owned())
        .arg("--exact")
        .arg("engine::topology::candidate::tests::candidate_kill_child")
        .arg("--ignored")
        .arg("--nocapture")
        .env(ENV_BASE, fixture.base.to_string_lossy().into_owned())
        .env(ENV_PRIVATE, fixture.private.to_string_lossy().into_owned())
        .env(ENV_SITE, which);
    HostRunner::new()
        .run(&gate_request(
            spec,
            fixture.root.clone(),
            Duration::from_secs(120),
            InvocationId::attempt(ALPHA, GENERATION, AttemptNumber(1), AttemptRole::Gate(0), 0),
        ))
        .expect("the child runs through the process funnel")
}

fn assert_killed(output: &ProcessOutput, which: &str) {
    assert!(
        !output.stderr.contains("panicked at"),
        "`{which}`: the child panicked instead of being killed: {}",
        output.stderr
    );
    assert_ne!(
        output.code,
        Some(0),
        "`{which}`: the child must not have exited cleanly"
    );
}

fn commit_body(fixture: &Fixture, object: &str) -> String {
    git_fixtures::git_ok(&fixture.base, &["cat-file", "commit", object])
}

#[test]
fn kill_after_commit_tree_before_pin_leaves_gc_owned_object_and_settles_interrupted() {
    let fixture = Fixture::new("kill-before-pin");
    assert!(
        fixture.unreachable_commits().is_empty(),
        "the fixture starts with no unreachable commit, so the one below is the child's"
    );

    let output = spawn_kill_child(&fixture, "before-pin");
    assert_killed(&output, "before-pin");

    let orphans = fixture.unreachable_commits();
    assert_eq!(
        orphans.len(),
        1,
        "the child wrote exactly one candidate commit and nothing referenced it: {orphans:?}"
    );
    let object = &orphans[0];
    assert!(
        fixture.object_present(object),
        "\"left to Git\" is not \"deleted\": the object is still in the store"
    );
    let body = commit_body(&fixture, object);
    assert!(
        body.contains(&format!("tree {}", fixture.tree_sha)),
        "it is the commit of the judged tree: {body}"
    );
    assert!(
        body.contains(&format!("parent {}", fixture.base_sha)),
        "parented on the base the work started from: {body}"
    );
    assert!(
        fixture.run_refs().is_empty(),
        "no pin exists: {:?}",
        fixture.run_refs()
    );

    let journal = fixture.journal(&Hooks::new());
    let recovery = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
    assert!(
        recovery.settles_interrupted,
        "the attempt is unsettled, so the resume settles it interrupted: {recovery:?}"
    );
    assert_eq!(
        recovery.orphan_pin, None,
        "an unpinned object leaves nothing for the resume to reclaim"
    );
    assert_eq!(
        recovery.promotion, None,
        "and nothing durable names a candidate, so nothing is promoted"
    );
}

#[test]
fn kill_at_commit_tree_id_unread_point_leaves_gc_owned_object() {
    assert_eq!(
        SubEffectPoint::IdUnread.modes(),
        &[InjectionMode::Kill],
        "an error-return sibling to this test would need a contract the packet does not give"
    );

    let fixture = Fixture::new("kill-id-unread");
    assert!(fixture.unreachable_commits().is_empty(), "clean start");

    let output = spawn_kill_child(&fixture, "id-unread");
    assert_killed(&output, "id-unread");

    let orphans = fixture.unreachable_commits();
    assert_eq!(
        orphans.len(),
        1,
        "commit-tree writes its object by temp-file-and-rename, so the object is whole \
         even though the id was never read: {orphans:?}"
    );
    let body = commit_body(&fixture, &orphans[0]);
    assert!(
        body.contains(&format!("tree {}", fixture.tree_sha))
            && body.contains(&format!("parent {}", fixture.base_sha)),
        "{body}"
    );
    assert!(
        fixture.run_refs().is_empty(),
        "nothing names it: {:?}",
        fixture.run_refs()
    );
}

#[test]
fn orphan_candidate_pin_removed_after_kill() {
    let fixture = Fixture::new("kill-after-pin");
    let output = spawn_kill_child(&fixture, "after-pin");
    assert_killed(&output, "after-pin");

    let refs = fixture.run_refs();
    assert_eq!(refs.len(), 1, "the pin, and only the pin: {refs:?}");
    let (refname, object) = &refs[0];
    assert_eq!(
        refname,
        candidate_pin_ref(RUN_ID, ALPHA, GENERATION).as_str(),
        "the pin is at the name the sequence derives"
    );
    assert!(
        !fixture.is_unreachable(object),
        "while the pin holds it the commit is R23, not R27"
    );

    let mut hooks = Hooks::new();
    let journal = fixture.journal(&hooks);
    let recovery = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
    assert!(recovery.settles_interrupted && recovery.promotion.is_none());
    let Some(pin) = recovery.orphan_pin else {
        panic!("an unsettled attempt with a pin on disk owes an orphan pin");
    };
    assert_eq!(pin.refname.as_str(), refname);
    assert_eq!(&pin.object.0, object, "the pin's *exact* recorded value");

    prune_orphan_pin(&fixture.manager, &mut hooks, pin).expect("prune the orphan");
    assert!(
        fixture.run_refs().is_empty(),
        "the pin is gone: {:?}",
        fixture.run_refs()
    );
    assert!(
        fixture.is_unreachable(object),
        "\"after which the object is again Git's\""
    );
    assert!(fixture.object_present(object), "again Git's is not deleted");
    assert!(
        hooks.observed(
            EffectSiteId::Ref(RefSite::DeleteCandidatePin),
            HookPhase::Before
        ),
        "the deletion went through its own funnel site"
    );
}

#[test]
fn unpinned_object_never_adopted_on_resume() {
    let fixture = Fixture::new("never-adopted");
    let mut hooks = Hooks::new();
    let journal = fixture.journal(&hooks);

    let unpinned = write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
        .expect("commit-tree");
    let object = unpinned.commit_sha().0.clone();
    drop(unpinned);

    assert!(fixture.object_present(&object));
    assert!(fixture.is_unreachable(&object));

    let recovery = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
    assert_eq!(
        recovery,
        CandidateRecovery {
            promotion: None,
            orphan_pin: None,
            settles_interrupted: true,
        },
        "the recovery names no object, so no later step has one to adopt"
    );

    assert_eq!(
        expected_refs(RUN_ID, journal.fold()),
        vec![candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0],
        "the pin alone, because a pin is written before anything records it"
    );
    fixture
        .manager
        .refuse_unexpected_refs(
            &run_namespace(RUN_ID),
            &expected_refs(RUN_ID, journal.fold()),
        )
        .expect("the namespace is empty, which is what it should be");

    assert!(
        fixture.object_present(&object),
        "still present after the recovery"
    );
    assert!(
        fixture.is_unreachable(&object),
        "still unreachable after the recovery: never adopted"
    );
}

fn run_to_queued(fixture: &Fixture, hooks: &mut Hooks, journal: &mut Journal) -> CommitSha {
    hooks.trace.reset();
    let unpinned = write_candidate_commit(&fixture.manager, hooks, RUN_ID, fixture.judged())
        .expect("commit-tree");
    let commit = unpinned.commit_sha().clone();
    let pinned = pin_candidate(&fixture.manager, hooks, unpinned).expect("pin");
    promote(&fixture.manager, hooks, journal, &fixture.task, pinned).expect("promote");
    commit
}

#[test]
fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {
    let fixture = Fixture::new("pin-substituted");
    let mut hooks = Hooks::new();
    let mut journal = fixture.journal(&hooks);

    let unpinned = write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
        .expect("commit-tree");
    let recorded = unpinned.commit_sha().clone();
    let pinned = pin_candidate(&fixture.manager, &mut hooks, unpinned).expect("pin");
    let promoting = append_candidate_prepared(&mut journal, pinned).expect("candidate_prepared");
    assert_eq!(journal.count("candidate_prepared"), 1);
    assert_eq!(journal.count("task_candidate_created"), 0);

    let impostor = fixture.sibling_commit(&mut hooks);
    assert_ne!(impostor, recorded);
    let names = CandidateNames::of(RUN_ID, ALPHA, GENERATION);
    git_fixtures::git_ok(
        &fixture.base,
        &["update-ref", names.prepared_ref.as_str(), impostor.as_str()],
    );

    let refused = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
        .expect_err("a pin that is not the recorded commit is a substitution");

    let text = refused.to_string();
    assert!(
        text.contains(impostor.as_str()) && text.contains(recorded.as_str()),
        "the refusal must name what the pin points at and what the record says: {text}"
    );

    assert_eq!(
        fixture
            .manager
            .direct_ref_target(names.prepared_ref.as_str())
            .expect("read the pin")
            .as_deref(),
        Some(impostor.as_str()),
        "the refusal removed or moved the pin, which is the evidence"
    );
    assert!(
        fixture
            .manager
            .direct_ref_target(names.candidate_ref.as_str())
            .expect("read the candidates ref")
            .is_none(),
        "a refused promotion created the authoritative candidate ref"
    );
    assert_eq!(journal.count("task_candidate_created"), 0);

    let referenced = create_candidates_ref(&fixture.manager, &mut hooks, promoting)
        .expect("the recorded commit still verifies");
    let created =
        append_candidate_created(&mut journal, referenced).expect("task_candidate_created");
    let refused = reclaim_after_creation(&fixture.manager, &mut hooks, &fixture.task, created)
        .expect_err("the prune compares against the recorded commit");
    assert!(
        refused.to_string().contains(impostor.as_str()),
        "the prune's refusal must name the substituted target: {refused}"
    );
    assert_eq!(
        fixture
            .manager
            .direct_ref_target(names.prepared_ref.as_str())
            .expect("read the pin")
            .as_deref(),
        Some(impostor.as_str()),
        "the prune deleted the substituted pin, which is the evidence"
    );
}

#[test]
fn kill_after_candidate_prepared_appends_candidate_created_once() {
    let fixture = Fixture::new("kill-after-prepared");
    let output = spawn_kill_child(&fixture, "after-prepared");
    assert_killed(&output, "after-prepared");

    let mut hooks = Hooks::new();
    let mut journal = Journal::resume(&fixture.private, &hooks);
    assert_eq!(journal.count("candidate_prepared"), 1);
    assert_eq!(journal.count("task_candidate_created"), 0);
    assert_eq!(journal.generation_class(), Some(GenerationClass::Promoting));
    let refs = fixture.run_refs();
    assert_eq!(
        refs.iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>(),
        vec![candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0],
        "the pin, and no candidates ref"
    );

    let recovery = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
    assert!(!recovery.settles_interrupted && recovery.orphan_pin.is_none());
    let Some(promoting) = recovery.promotion else {
        panic!("a durable candidate_prepared is an unfinished promotion");
    };
    assert_eq!(
        promoting.tree, fixture.tree_sha,
        "the recovered promotion carries {} where `candidate_prepared` recorded \
         {}, so adoption would verify against the wrong number and a divergent \
         tree would pass",
        promoting.tree.0, fixture.tree_sha.0
    );
    let commit = promoting.candidate().commit_sha.clone();
    complete_promotion(
        &fixture.manager,
        &mut hooks,
        &mut journal,
        &fixture.task,
        promoting,
    )
    .expect("the closure procedure finishes it");
    assert_eq!(journal.count("task_candidate_created"), 1);

    assert!(
        recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
            .expect("classify again")
            .is_empty(),
        "a completed promotion — queue position appended, pin pruned — owes nothing"
    );
    let again = PromotingCandidate {
        candidate: CandidateRef {
            key: ALPHA,
            generation: GENERATION,
            commit_sha: commit.clone(),
            candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
        },
        prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
        base: fixture.base_sha.clone(),
        tree: fixture.tree_sha.clone(),
    };
    complete_promotion(
        &fixture.manager,
        &mut hooks,
        &mut journal,
        &fixture.task,
        again,
    )
    .expect("idempotent");
    assert_eq!(
        journal.count("task_candidate_created"),
        1,
        "twice through the closure procedure is still one queue position"
    );
    assert_eq!(
        fixture.run_refs(),
        vec![(
            candidates_ref(RUN_ID, ALPHA, GENERATION).0,
            commit.0.clone()
        )],
        "the candidates ref alone, at the recorded commit"
    );

    let sibling = fixture.sibling_commit(&mut hooks);
    assert_ne!(sibling, commit, "a different commit on the same base");
    let forged = PromotingCandidate {
        candidate: CandidateRef {
            key: ALPHA,
            generation: GENERATION,
            commit_sha: sibling,
            candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
        },
        prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
        base: fixture.base_sha.clone(),
        tree: fixture.tree_sha.clone(),
    };
    let refused = complete_promotion(
        &fixture.manager,
        &mut hooks,
        &mut journal,
        &fixture.task,
        forged,
    )
    .expect_err("a candidates ref at another SHA refuses");
    assert!(
        refused.to_string().contains("is present at") && refused.to_string().contains(&commit.0),
        "the refusal names the ref's actual value: {refused}"
    );
}

#[test]
fn pin_pruned_after_promotion() {
    let fixture = Fixture::new("pin-pruned");
    let mut hooks = Hooks::new();
    let mut journal = fixture.journal(&hooks);
    let commit = run_to_queued(&fixture, &mut hooks, &mut journal);

    assert_eq!(
        hooks.trace.order(&[
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            EffectSiteId::Ref(RefSite::PinCandidatePrepared),
            EffectSiteId::Event(EventSite::Append),
            EffectSiteId::Ref(RefSite::CreateCandidates),
            EffectSiteId::Ref(RefSite::DeleteCandidatePin),
            EffectSiteId::Worktree(WorktreeSite::Remove),
        ]),
        vec![
            "Object.CandidateCommitTree".to_owned(),
            "Ref.PinCandidatePrepared".to_owned(),
            "Event.Append".to_owned(),
            "Ref.CreateCandidates".to_owned(),
            "Event.Append".to_owned(),
            "Ref.DeleteCandidatePin".to_owned(),
            "Worktree.Remove".to_owned(),
        ],
        "O28 to O31, as one observed order — three appends, because \
         `candidate_prepared` is the sole successful settlement"
    );

    assert_eq!(
        fixture.run_refs(),
        vec![(
            candidates_ref(RUN_ID, ALPHA, GENERATION).0,
            commit.0.clone()
        )],
        "the pin is pruned and the candidates ref is not"
    );
    assert!(
        !fixture.is_unreachable(&commit.0),
        "the candidates ref (R11) is what accounts for the commit now"
    );

    assert!(
        !hooks.observed(
            EffectSiteId::Ref(RefSite::DeleteCandidatesRef),
            HookPhase::Before
        ),
        "promotion pruned the authoritative ref"
    );
    assert_eq!(
        expected_refs(RUN_ID, journal.fold()),
        vec![
            candidates_ref(RUN_ID, ALPHA, GENERATION).0,
            candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0,
        ],
        "a prepared candidate accounts for both names, whether or not either is on disk"
    );
    fixture
        .manager
        .refuse_unexpected_refs(
            &run_namespace(RUN_ID),
            &expected_refs(RUN_ID, journal.fold()),
        )
        .expect("the namespace carries exactly what the fold accounts for");
}

#[test]
fn promoting_completed_at_run_end() {
    let fixture = Fixture::new("promoting-at-run-end");
    let mut hooks = Hooks::new();
    let mut journal = fixture.journal(&hooks);

    let unpinned = write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
        .expect("commit-tree");
    let pinned = pin_candidate(&fixture.manager, &mut hooks, unpinned).expect("pin");
    let promoting = append_candidate_prepared(&mut journal, pinned).expect("candidate_prepared");

    assert_eq!(journal.generation_class(), Some(GenerationClass::Promoting));
    assert_eq!(
        journal.fold().derived_outcome(),
        crate::topology::events::DerivedOutcome::NotEnding,
        "a Promoting generation blocks the run from ending"
    );
    let refused = journal
        .emit(TopologyEventBody::RunFinished {
            data: crate::topology::events::RunFinished4 {
                outcome: crate::events::RunOutcome::Complete,
                halted_at: None,
                merged: 0,
                parked: 0,
            },
        })
        .expect_err("run_finished before the promotion");
    assert!(
        refused.to_string().contains("not ending"),
        "the fold refuses it for the reason ST-17 gives: {refused}"
    );

    let Some(promoting_again) = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
        .expect("classify")
        .promotion
    else {
        panic!("a Promoting generation owes a promotion");
    };
    assert_eq!(
        promoting_again.candidate(),
        promoting.candidate(),
        "the closure procedure recovers the same candidate the live path holds"
    );
    complete_promotion(
        &fixture.manager,
        &mut hooks,
        &mut journal,
        &fixture.task,
        promoting,
    )
    .expect("promote at run end");

    assert_eq!(
        journal.generation_class(),
        Some(GenerationClass::Closed),
        "Promoting ends with `task_candidate_created`"
    );
    assert!(
        journal
            .fold()
            .task(ALPHA)
            .expect("alpha")
            .generations
            .iter()
            .all(|generation| generation.class != GenerationClass::Promoting),
        "no generation is left promoting"
    );
    assert_eq!(
        journal
            .fold()
            .queue()
            .expect("started")
            .entries()
            .iter()
            .map(|entry| entry.candidate.commit_sha.clone())
            .collect::<Vec<_>>(),
        vec![promoting_again.candidate().commit_sha.clone()],
        "and it holds its queue position"
    );
}

#[test]
fn worktree_removal_idempotent_after_candidate_created() {
    let fixture = Fixture::new("scrub-idempotent");
    let mut hooks = Hooks::new();
    let mut journal = fixture.journal(&hooks);
    let path = fixture.manager.slot_path(&fixture.task);
    assert!(
        path.is_dir(),
        "the task worktree exists before the promotion"
    );

    run_to_queued(&fixture, &mut hooks, &mut journal);

    assert_eq!(journal.count("task_candidate_created"), 1);
    assert!(!path.exists(), "the worktree is scrubbed");
    assert!(
        !fixture.manager.intent_path(&fixture.task).exists(),
        "and its intent leaves with it"
    );
    assert!(
        !fixture.task_admin_dir().exists(),
        "and the row Git registered for it"
    );

    let order = hooks.trace.order(&[
        EffectSiteId::Event(EventSite::Append),
        EffectSiteId::Worktree(WorktreeSite::Remove),
    ]);
    assert_eq!(
        order.last().map(String::as_str),
        Some("Worktree.Remove"),
        "the scrub is after every append of the sequence: {order:?}"
    );

    for round in 1..=2 {
        fixture
            .manager
            .remove_worktree(hooks.effects(), &fixture.task)
            .unwrap_or_else(|error| panic!("round {round}: {error}"));
        fixture
            .manager
            .remove_intent(hooks.effects(), &fixture.task)
            .unwrap_or_else(|error| panic!("round {round}: {error}"));
        assert!(!path.exists(), "round {round}");
    }

    let escaping = Slot::Task {
        key: "../../escape".to_owned(),
        generation: GENERATION.0,
    };
    let refused = fixture
        .manager
        .remove_worktree(hooks.effects(), &escaping)
        .expect_err("a slot whose name leaves the execution root refuses");
    assert!(
        refused.to_string().contains("escape"),
        "the refusal names what it refused: {refused}"
    );
}

#[test]
fn worktree_removal_succeeds_with_index_lock_present() {
    let fixture = Fixture::new("scrub-index-lock");
    let admin = fixture.task_admin_dir();
    let lock = admin.join("index.lock");
    assert!(admin.is_dir(), "the worktree's row: {}", admin.display());

    git_fixtures::git_ok(
        &fixture.base,
        &[
            "config",
            "--file",
            &lock.to_string_lossy(),
            "upstroke.residue",
            "interrupted",
        ],
    );
    assert!(lock.is_file(), "the lock is planted");

    let worktree = fixture.manager.slot_path(&fixture.task);
    let blocked = git_fixtures::git(&worktree, &["add", "-A"]);
    assert_ne!(
        blocked.code,
        Some(0),
        "an index.lock that blocks nothing would make this test vacuous"
    );
    assert!(
        blocked.stderr.contains("index.lock"),
        "and it is the lock that blocked it: {}",
        blocked.stderr
    );

    let mut hooks = Hooks::new();
    fixture
        .manager
        .remove_worktree(hooks.effects(), &fixture.task)
        .expect("forced removal reclaims through administrative residue");

    assert!(!worktree.exists(), "the checkout is gone");
    assert!(
        !admin.exists(),
        "the row is gone, and the residue left with it: {}",
        admin.display()
    );
    assert!(!lock.exists(), "including the lock itself");
    assert!(
        !git_fixtures::git_ok(&fixture.base, &["worktree", "list", "--porcelain"])
            .contains("kalpha-g0"),
        "and Git no longer lists it"
    );
}

#[test]
fn snapshot_residue_reclaimed() {
    let fixture = Fixture::new("snapshot-residue");
    let mut hooks = Hooks::new();
    let input = SnapshotInput::Tree {
        tree: ObjectId::new(fixture.tree_sha.0.clone()).expect("the judged tree is an object id"),
        parent: ObjectId::new(fixture.base_sha.0.clone()).expect("the base is an object id"),
    };

    let gates = fixture
        .manager
        .add_snapshot(hooks.effects(), &SnapshotName::gates(0, 1), &input)
        .expect("the gate snapshot");
    let review = fixture
        .manager
        .add_snapshot(hooks.effects(), &SnapshotName::review(0, 1, 0), &input)
        .expect("the reviewer's snapshot");
    let ephemeral = gates.ephemeral().expect("a tree input commits");
    assert_eq!(
        ephemeral,
        review.ephemeral().expect("and so does the other"),
        "the same tree on the same parent is the same commit"
    );
    assert!(
        !fixture.is_unreachable(ephemeral.as_str()),
        "while a snapshot has it checked out it is R24, not R27"
    );

    fixture
        .manager
        .remove_snapshot(hooks.effects(), &gates)
        .expect("prune the gate snapshot");
    assert!(!gates.path().exists());
    assert!(!fixture.manager.intent_path(gates.slot()).exists());

    let reclaimed = fixture
        .manager
        .reclaim_intents(hooks.effects())
        .expect("reclaim");
    assert!(
        reclaimed.slots.contains(review.slot()),
        "the reviewer's snapshot is reclaimed as residue: {reclaimed:?}"
    );
    assert!(!review.path().exists(), "its worktree is gone");
    assert!(
        !fixture.manager.intent_path(review.slot()).exists(),
        "and so is its intent"
    );

    assert!(
        fixture.is_unreachable(ephemeral.as_str()),
        "the ephemeral snapshot commit returns to R27"
    );
    assert!(
        fixture.object_present(ephemeral.as_str()),
        "returned to R27, not deleted"
    );
    assert!(
        hooks.observed(
            EffectSiteId::Snapshot(SnapshotSite::Remove),
            HookPhase::Before
        ) && hooks.observed(
            EffectSiteId::Snapshot(SnapshotSite::RemoveIntent),
            HookPhase::Before
        ),
        "both snapshot removal sites ran through their funnels"
    );
}

#[test]
fn the_candidate_refs_are_the_names_the_packet_gives() {
    let names = CandidateNames::of("01RUN", TaskKey(7), GenerationId(3));
    assert_eq!(
        names.prepared_ref.as_str(),
        "refs/upstroke/runs/01RUN/candidate-prepared/7/3"
    );
    assert_eq!(
        names.candidate_ref.as_str(),
        "refs/upstroke/runs/01RUN/candidates/7/3"
    );
    assert_eq!(
        names.prepared_ref,
        candidate_pin_ref("01RUN", TaskKey(7), GenerationId(3))
    );
    assert_eq!(
        names.candidate_ref,
        candidates_ref("01RUN", TaskKey(7), GenerationId(3))
    );

    assert_eq!(run_namespace("01RUN"), "refs/upstroke/runs/01RUN/");
    assert!(
        names
            .prepared_ref
            .as_str()
            .starts_with(&run_namespace("01RUN"))
    );
    assert!(
        !candidate_pin_ref("01RUNNER", TaskKey(7), GenerationId(3))
            .as_str()
            .starts_with(&run_namespace("01RUN")),
        "a sibling run's namespace is not a prefix of this one's"
    );
}

#[test]
fn promotion_refuses_a_commit_that_is_not_in_the_repository() {
    let fixture = Fixture::new("object-missing");
    let mut hooks = Hooks::new();
    let mut journal = fixture.journal(&hooks);

    let absent = CommitSha("0123456789abcdef0123456789abcdef01234567".to_owned());
    assert!(
        !fixture.object_present(absent.as_str()),
        "and it really is absent"
    );
    let forged = PromotingCandidate {
        candidate: CandidateRef {
            key: ALPHA,
            generation: GENERATION,
            commit_sha: absent.clone(),
            candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
        },
        prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
        base: fixture.base_sha.clone(),
        tree: fixture.tree_sha.clone(),
    };
    let refused = complete_promotion(
        &fixture.manager,
        &mut hooks,
        &mut journal,
        &fixture.task,
        forged,
    )
    .expect_err("an absent object refuses");
    assert!(
        refused.to_string().contains(absent.as_str())
            && refused
                .to_string()
                .contains("not an object in this repository"),
        "{refused}"
    );
    assert!(
        fixture.run_refs().is_empty(),
        "and it refuses before creating anything: {:?}",
        fixture.run_refs()
    );
    assert_eq!(journal.count("task_candidate_created"), 0);
}

#[test]
fn promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged() {
    let fixture = Fixture::new("tree-never-judged");
    let mut hooks = Hooks::new();
    let mut journal = fixture.journal(&hooks);

    let (impostor, impostor_tree) = fixture.divergent_tree_commit(&mut hooks);
    assert_ne!(
        impostor_tree, fixture.tree_sha,
        "the fixture built the judged tree again, so there is nothing to refuse"
    );

    assert!(
        fixture
            .manager
            .object_exists(impostor.as_str())
            .expect("cat-file"),
        "the impostor is not present, so the existence check refuses it first"
    );
    assert_eq!(
        fixture
            .manager
            .commit_parent(impostor.as_str())
            .expect("rev-parse")
            .as_deref(),
        Some(fixture.base_sha.as_str()),
        "the impostor is not on the recorded base, so the parent check refuses \
         it first and the tree is never reached"
    );

    let forged = PromotingCandidate {
        candidate: CandidateRef {
            key: ALPHA,
            generation: GENERATION,
            commit_sha: impostor.clone(),
            candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
        },
        prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
        base: fixture.base_sha.clone(),
        tree: fixture.tree_sha.clone(),
    };
    let Err(refused) = complete_promotion(
        &fixture.manager,
        &mut hooks,
        &mut journal,
        &fixture.task,
        forged,
    ) else {
        panic!(
            "recovery adopted a commit whose tree is {} where the record says {}: the \
             authoritative candidate ref would name an object nothing judged",
            impostor_tree.0, fixture.tree_sha.0
        )
    };
    assert!(
        refused.to_string().contains(impostor.as_str()),
        "the refusal must name the object it refused: {refused}"
    );

    assert_eq!(
        journal.count("task_candidate_created"),
        0,
        "a refused adoption still took a queue position"
    );
    assert!(
        fixture
            .manager
            .direct_ref_target(candidates_ref(RUN_ID, ALPHA, GENERATION).as_str())
            .expect("read the ref")
            .is_none(),
        "a refused adoption still created the authoritative candidate ref"
    );
}

#[test]
fn promotion_refuses_an_object_that_is_not_the_judged_candidate() {
    for (label, present) in [
        ("a tree, not a commit", true),
        ("a commit that is not on the base", false),
    ] {
        let fixture = Fixture::new("object-not-the-candidate");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);

        let impostor = if present {
            CommitSha(fixture.tree_sha.0.clone())
        } else {
            fixture.base_sha.clone()
        };
        assert!(
            fixture
                .manager
                .object_exists(impostor.as_str())
                .expect("cat-file"),
            "{label}: the impostor is present to the residue classifier, so existence alone \
             would pass"
        );

        let forged = PromotingCandidate {
            candidate: CandidateRef {
                key: ALPHA,
                generation: GENERATION,
                commit_sha: impostor.clone(),
                candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
            },
            prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
            base: fixture.base_sha.clone(),
            tree: fixture.tree_sha.clone(),
        };
        let Err(refused) = complete_promotion(
            &fixture.manager,
            &mut hooks,
            &mut journal,
            &fixture.task,
            forged,
        ) else {
            panic!("{label}: an impostor object must refuse");
        };
        assert!(
            refused.to_string().contains(impostor.as_str()),
            "{label}: {refused}"
        );
        assert!(
            fixture.run_refs().is_empty(),
            "{label}: and it refuses before creating anything: {:?}",
            fixture.run_refs()
        );
        assert_eq!(journal.count("task_candidate_created"), 0, "{label}");
    }
}

#[test]
fn a_commit_id_that_is_not_a_full_object_id_refuses() {
    for value in ["", "abc", "HEAD", "0123456789abcdef0123456789abcdef0123456"] {
        let refusal = refuse_malformed_commit(ALPHA, GENERATION, &CommitSha(value.to_owned()))
            .expect_err("not a full hexadecimal object id");
        assert!(
            refusal.to_string().contains("full hexadecimal object id"),
            "{value:?}: {refusal}"
        );
    }
    refuse_malformed_commit(
        ALPHA,
        GENERATION,
        &CommitSha("0123456789abcdef0123456789abcdef01234567".to_owned()),
    )
    .expect("forty hexadecimal characters is an object id");
}

#[test]
fn a_pin_left_by_an_interrupted_promotion_is_pruned_by_the_closure_procedure() {
    let fixture = Fixture::new("pin-left-behind");
    let mut hooks = Hooks::new();
    let mut journal = fixture.journal(&hooks);

    let unpinned = write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
        .expect("commit-tree");
    let commit = unpinned.commit_sha().clone();
    let pinned = pin_candidate(&fixture.manager, &mut hooks, unpinned).expect("pin");
    hooks.arm_phase(
        EffectSiteId::Ref(RefSite::DeleteCandidatePin),
        HookPhase::Before,
        Injection::Error,
    );
    promote(
        &fixture.manager,
        &mut hooks,
        &mut journal,
        &fixture.task,
        pinned,
    )
    .expect_err("the prune was made to fail");

    assert_eq!(journal.count("task_candidate_created"), 1);
    assert_eq!(journal.generation_class(), Some(GenerationClass::Closed));
    let mut names: Vec<String> = fixture
        .run_refs()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0,
            candidates_ref(RUN_ID, ALPHA, GENERATION).0,
        ],
        "the pin outlived the promotion"
    );
    fixture
        .manager
        .refuse_unexpected_refs(
            &run_namespace(RUN_ID),
            &expected_refs(RUN_ID, journal.fold()),
        )
        .expect("a pin the promotion has not pruned yet is not an unexpected ref");

    let recovery = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
    assert!(
        !recovery.settles_interrupted && recovery.orphan_pin.is_none(),
        "a durable candidate is not an orphan and its attempt is settled: {recovery:?}"
    );
    let promoting = recovery.promotion.expect("the promotion is unfinished");
    assert_eq!(
        promoting.prepared_ref(),
        &candidate_pin_ref(RUN_ID, ALPHA, GENERATION)
    );

    let mut clean = Hooks::new();
    let queued = complete_promotion(
        &fixture.manager,
        &mut clean,
        &mut journal,
        &fixture.task,
        promoting,
    )
    .expect("the closure procedure finishes it");
    assert_eq!(queued.candidate().commit_sha, commit);
    assert_eq!(
        journal.count("task_candidate_created"),
        1,
        "and appends nothing: the generation is no longer Promoting"
    );
    assert_eq!(
        fixture.run_refs(),
        vec![(candidates_ref(RUN_ID, ALPHA, GENERATION).0, commit.0)],
        "the pin is pruned and the authoritative ref is untouched"
    );
    assert!(
        recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
            .expect("classify again")
            .is_empty()
    );
}

#[test]
fn a_symbolic_pin_refuses_on_both_the_write_and_the_read() {
    let fixture = Fixture::new("symbolic-pin");
    let mut hooks = Hooks::new();
    let journal = fixture.journal(&hooks);
    let pin = candidate_pin_ref(RUN_ID, ALPHA, GENERATION);
    git_fixtures::git_ok(
        &fixture.base,
        &["symbolic-ref", pin.as_str(), "refs/heads/main"],
    );

    let unpinned = write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
        .expect("commit-tree");
    let refused =
        pin_candidate(&fixture.manager, &mut hooks, unpinned).expect_err("a symbolic pin refuses");
    assert!(
        refused.to_string().contains("symbolic ref")
            && refused.to_string().contains("refs/heads/main"),
        "the refusal names what it found: {refused}"
    );

    let read = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
        .expect_err("and the resume refuses to read it too");
    assert!(read.to_string().contains("symbolic ref"), "{read}");
}

#[test]
fn an_unexpected_ref_under_the_run_namespace_refuses() {
    let fixture = Fixture::new("unexpected-ref");
    let mut hooks = Hooks::new();
    let mut journal = fixture.journal(&hooks);
    let namespace = run_namespace(RUN_ID);
    let candidates = candidates_ref(RUN_ID, ALPHA, GENERATION);

    assert_eq!(
        expected_refs(RUN_ID, journal.fold()),
        vec![candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0],
    );
    git_fixtures::git_ok(
        &fixture.base,
        &["update-ref", candidates.as_str(), fixture.base_sha.as_str()],
    );
    let refused = fixture
        .manager
        .refuse_unexpected_refs(&namespace, &expected_refs(RUN_ID, journal.fold()))
        .expect_err("no candidate is prepared, so no candidates ref is accounted for");
    assert!(
        refused.to_string().contains("unexpected ref")
            && refused.to_string().contains(candidates.as_str()),
        "{refused}"
    );
    git_fixtures::git_ok(&fixture.base, &["update-ref", "-d", candidates.as_str()]);

    run_to_queued(&fixture, &mut hooks, &mut journal);
    let expected = expected_refs(RUN_ID, journal.fold());
    assert!(expected.contains(&candidates.0));
    fixture
        .manager
        .refuse_unexpected_refs(&namespace, &expected)
        .expect("what promotion left is what the fold accounts for");

    let stowaway = candidates_ref(RUN_ID, ALPHA, GenerationId(9));
    git_fixtures::git_ok(
        &fixture.base,
        &["update-ref", stowaway.as_str(), fixture.base_sha.as_str()],
    );
    let refused = fixture
        .manager
        .refuse_unexpected_refs(&namespace, &expected)
        .expect_err("generation 9 does not exist");
    assert!(refused.to_string().contains(stowaway.as_str()), "{refused}");
}

fn attempt_record() -> AttemptRecord {
    AttemptRecord {
        attempt: 1,
        tier: "mid".to_owned(),
        model: "alpha-model".to_owned(),
        pool: None,
        resumed: false,
        duration: Duration::from_millis(1_234),
        cost_usd: Some(0.5),
        reviews: vec![ReviewRecord {
            pass: "review".to_owned(),
            agent: "claude-code".to_owned(),
            model: "claude-opus-5".to_owned(),
            adapter: Some("claude-code".to_owned()),
            preflight_cli_version: None,
            effort: None,
            pool: None,
            cost_usd: None,
            outcome: ReviewPassOutcome::Passed,
        }],
        session_id: None,
        usage: None,
        failure: None,
    }
}
