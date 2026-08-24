//! Lane B's suite: the pre-lock checks' witness, P0–P8, and the one deletion
//! boundary.
//!
//! Kept out of `create.rs` for the reason `src/runner/container/resolve/tests.rs`
//! is kept out of `resolve.rs`: `effects::production_region` cuts a source at
//! its **first** `#[cfg(test)]`, so a suite inline in the module shrinks every
//! source census's view of that module to whatever precedes it
//! (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`). Out here, `create.rs` is production
//! in full to every census that reads it.
//!
//! This file carries **no** module-level lint allow, and needs none: it creates
//! directories through the run-directory funnel, spawns its kill children
//! through the `Runner`, and reads with `std::fs`'s read-only calls, which are
//! not on the denylist.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use super::*;
use crate::agent::proc::SpawnHooks;
use crate::events::log::{EventHooks, SyncRecord, WrittenShape};
use crate::events::{BindingSummary, ChainSummary};
use crate::ir::{
    ArtifactId, Effort, Plan, PlanSource, ResolvedEffortPolicy, Task, TaskId, TaskKind, Tier,
};
use crate::review::ReviewPlan;
use crate::rundir::{
    COMMIT_RECORD, EVENT_LOG, MARKER, MARKER_STAGED, NoHooks as NoRunDirHooks, OWNER_RECORD,
    OWNER_RECORD_STAGED, PLAN, RunDirHooks,
};
#[allow(unused_imports)]
use crate::rundir::{HuskDisposition, Reclaimable};
use crate::runner::container::ContainerHooks;
use crate::runner::container::runtime::{
    ContainerExecution, CreateSpec, CreatedContainer, DiscoveredContainer, ImageInspection,
    Liveness, RuntimeError, RuntimeOp, StopMode,
};
use crate::topology::effects::{
    EffectSiteId, HookHarness, HookPhase, Injection, InjectionMode, LockSite, RunDirSite,
    SubEffectPoint,
};
use crate::topology::events::{CommitSha, GitRef, IncarnationId, RunnerPolicy, TopologyLimits};
use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion};
use crate::topology::registry::TaskRegistry;
use crate::topology::schema::TOPOLOGY_SCHEMA;
use crate::util::DurabilityLedger;
use crate::workspace_manager::EffectHooks;

use super::super::seams::{IdSource, TimeSource};

const RUN_ID: &str = "01KZTPR7BCREATE00000000001";
const INCARNATION: &str = "01KZTINCB0CREATE0000000001";
const NORMALIZED_DIGEST: &str =
    "sha256:9999999999999999999999999999999999999999999999999999999999999999";
const BASE_SHA: &str = "1111111111111111111111111111111111111111";
const OTHER_SHA: &str = "2222222222222222222222222222222222222222";
const INTEGRATION_REF: &str = "refs/heads/upstroke/run-01KZTPR7BCREATE00000000001";
const AGENT: &str = "codex";

// -----------------------------------------------------------------------
// Fixed identities and a fixed clock
// -----------------------------------------------------------------------

/// Fixed, because `run_started_sha256` is the digest of the exact line and
/// a live clock would move it.
#[derive(Debug, Clone)]
struct Fixed {
    run_id: String,
}

impl Default for Fixed {
    fn default() -> Self {
        Self {
            run_id: RUN_ID.to_owned(),
        }
    }
}

impl TimeSource for Fixed {
    fn now_rfc3339(&self) -> String {
        "2026-08-23T09:41:02Z".to_owned()
    }
}

impl IdSource for Fixed {
    fn run_id(&self) -> String {
        self.run_id.clone()
    }

    fn incarnation(&self) -> IncarnationId {
        IncarnationId(INCARNATION.to_owned())
    }

    fn pid(&self) -> u32 {
        4242
    }
}

// -----------------------------------------------------------------------
// The hook bundle: records into one `HookHarness`, and can be armed
// -----------------------------------------------------------------------

/// What a fault was armed at.
///
/// A module-local double is unavoidable: `HookHarness::arm` takes only a
/// `SubEffectPoint`, and `HookHarness::hook` answers `Proceed` to `Before`
/// and `After` unconditionally, so no phase of a `RunDir` or `Lock` site can
/// be armed through it. What is **not** local is the recording: every family
/// below still reports into the shared harness, or the sites this slice
/// drives would contribute nothing to the coverage evidence.
#[derive(Debug, Default)]
struct Armed {
    phases: Vec<(EffectSiteId, HookPhase, Injection)>,
    /// `(site, point, mode, consultations still to skip)`.
    ///
    /// A delay is needed for exactly one coordinate and for a real reason:
    /// `Event.OpenLog.SyncPrefix` is consulted by **every** open, and P5's
    /// own open is one of them. Arming it up front would fail P5 rather
    /// than the barrier the test is about.
    delayed: Vec<(EventSite, SubEffectPoint, InjectionMode, u32)>,
    torn: Option<EventSite>,
}

#[derive(Debug, Clone, Default)]
struct Faults(Arc<Mutex<Armed>>);

impl Faults {
    fn get(&self) -> std::sync::MutexGuard<'_, Armed> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn arm_phase(&self, site: EffectSiteId, phase: HookPhase, injection: Injection) -> &Self {
        self.get().phases.push((site, phase, injection));
        self
    }

    fn arm_point_after(
        &self,
        site: EventSite,
        point: SubEffectPoint,
        mode: InjectionMode,
        skip: u32,
    ) -> &Self {
        self.get().delayed.push((site, point, mode, skip));
        self
    }

    /// Whether a delayed arming is due now, consuming one of its skips.
    fn due(&self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> bool {
        let mut armed = self.get();
        let Some(entry) = armed
            .delayed
            .iter_mut()
            .find(|(armed_site, at, in_mode, _)| {
                *armed_site == site && *at == point && *in_mode == mode
            })
        else {
            return false;
        };
        if entry.3 == 0 {
            return true;
        }
        entry.3 -= 1;
        false
    }

    /// Clear every phase arming, so a test can drive the repair the same
    /// funnel would perform after the failure it injected.
    fn disarm(&self) {
        self.get().phases.clear();
    }

    fn tear_the_first_line(&self, site: EventSite) -> &Self {
        self.get().torn = Some(site);
        self
    }

    fn phase(&self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.get()
            .phases
            .iter()
            .find(|(armed, at, _)| *armed == site && *at == phase)
            .map_or(Injection::Proceed, |(_, _, injection)| *injection)
    }
}

/// The run-directory and lock families, armed and recording.
#[derive(Debug, Clone)]
struct ArmedRunDir {
    harness: Arc<Mutex<HookHarness>>,
    faults: Faults,
    ledger: DurabilityLedger,
}

impl RunDirHooks for ArmedRunDir {
    fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .hook(site, phase);
        self.faults.phase(site, phase)
    }

    fn durability_ledger(&self) -> DurabilityLedger {
        self.ledger.clone()
    }
}

/// The append funnel, armed and recording — including
/// `written_kill_shape`, without which `WrittenShape::Torn` is unreachable
/// and no torn first line can be produced at all.
#[derive(Debug, Clone)]
struct ArmedEvents {
    harness: Arc<Mutex<HookHarness>>,
    faults: Faults,
    ledger: DurabilityLedger,
    synced: Arc<Mutex<Vec<SyncRecord>>>,
}

impl EventHooks for ArmedEvents {
    fn phase(&mut self, site: EventSite, phase: HookPhase) {
        self.harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .hook(EffectSiteId::Event(site), phase);
    }

    /// Straight to the shared harness: `EventSite`'s points are the ones
    /// `HookHarness::arm` accepts, so the injection *and* the coverage
    /// record come from the same place. Only the two phases of a `RunDir`
    /// or `Lock` site need the module-local table, because `arm` takes no
    /// phase.
    fn point(&mut self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> Injection {
        let mut harness = self.harness.lock().unwrap_or_else(PoisonError::into_inner);
        if self.faults.due(site, point, mode) {
            // Armed in the shared harness at the moment it becomes due, so
            // the injection and the coverage record still come from one
            // place rather than two that could disagree.
            harness
                .arm(EffectSiteId::Event(site), point, mode)
                .expect("the site exposes this point in this mode");
        }
        harness.hook(EffectSiteId::Event(site), HookPhase::Point { point, mode })
    }

    fn written_kill_shape(&mut self, site: EventSite) -> WrittenShape {
        if self.faults.get().torn == Some(site) {
            WrittenShape::Torn
        } else {
            WrittenShape::Complete
        }
    }

    fn durability_ledger(&self) -> DurabilityLedger {
        self.ledger.clone()
    }

    fn synced(&mut self, record: &SyncRecord) {
        self.synced
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(record.clone());
    }
}

/// The container funnel, armed and recording.
///
/// Needed for the same reason `ArmedRunDir` is: `ContainerSite` exposes no
/// sub-effect point, so `Container.Start`'s two phases are the only
/// coordinates a fault can be placed at and `HookHarness::arm` reaches
/// neither.
#[derive(Debug, Clone)]
struct ArmedContainer {
    harness: Arc<Mutex<HookHarness>>,
    faults: Faults,
}

impl ContainerHooks for ArmedContainer {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .hook(site, phase);
        self.faults.phase(site, phase)
    }
}

/// All five families on one harness, four of them armable.
struct TestHooks {
    rundir: ArmedRunDir,
    events: ArmedEvents,
    effects: crate::workspace_manager::HarnessEffects,
    container: ArmedContainer,
    spawn: crate::runner::HarnessHooks,
    harness: Arc<Mutex<HookHarness>>,
    faults: Faults,
}

impl TestHooks {
    fn new() -> Self {
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let faults = Faults::default();
        let ledger = DurabilityLedger::recording();
        Self {
            rundir: ArmedRunDir {
                harness: Arc::clone(&harness),
                faults: faults.clone(),
                ledger: ledger.clone(),
            },
            events: ArmedEvents {
                harness: Arc::clone(&harness),
                faults: faults.clone(),
                ledger,
                synced: Arc::new(Mutex::new(Vec::new())),
            },
            effects: crate::workspace_manager::HarnessEffects::new(Arc::clone(&harness)),
            container: ArmedContainer {
                harness: Arc::clone(&harness),
                faults: faults.clone(),
            },
            spawn: crate::runner::HarnessHooks::new(Arc::clone(&harness)),
            harness,
            faults,
        }
    }

    fn faults(&self) -> Faults {
        self.faults.clone()
    }

    /// The container family as a value, for installing on a
    /// `ContainerRunner` — which owns its own observer.
    fn container_double(&self) -> ArmedContainer {
        self.container.clone()
    }

    fn arm(&self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) {
        self.harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .arm(EffectSiteId::Event(site), point, mode)
            .expect("the site exposes this point in this mode");
    }

    fn observed(&self, site: EffectSiteId, phase: HookPhase) -> bool {
        self.harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observed(site, phase)
    }
}

impl TopologyHooks for TestHooks {
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

// -----------------------------------------------------------------------
// The two seams
// -----------------------------------------------------------------------

/// Records the order the probes ran in, and can refuse either.
#[derive(Debug)]
struct RecordingProbes {
    digest: String,
    calls: Mutex<Vec<String>>,
    refuse_shell: bool,
    refuse_agent: Option<String>,
    /// `std::process::abort()` inside the probe — the same primitive
    /// `Injection::Kill` performs, at the one prefix that has no funnel
    /// site of its own.
    kill_shell: bool,
    kill_agent: bool,
}

impl RecordingProbes {
    fn new(digest: &str) -> Self {
        Self {
            digest: digest.to_owned(),
            calls: Mutex::new(Vec::new()),
            refuse_shell: false,
            refuse_agent: None,
            kill_shell: false,
            kill_agent: false,
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Probes for RecordingProbes {
    fn policy_digest(&self) -> &str {
        &self.digest
    }

    fn shell(&self, invocation: InvocationId) -> Result<(), UpstrokeError> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("shell {invocation}"));
        if self.kill_shell {
            std::process::abort();
        }
        if self.refuse_shell {
            return Err(UpstrokeError::Refused {
                message: "pre-flight: the recorded shell did not run `exit 0`".to_owned(),
            });
        }
        Ok(())
    }

    fn agent(&self, agent: &str) -> Result<(), UpstrokeError> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("agent {agent}"));
        if self.kill_agent {
            std::process::abort();
        }
        if self.refuse_agent.as_deref() == Some(agent) {
            return Err(UpstrokeError::Agent {
                message: format!("pre-flight: `{agent}` could not be probed"),
            });
        }
        Ok(())
    }
}

/// A ref namespace with no Git behind it.
#[derive(Debug, Default)]
struct FakeRefs {
    at: Mutex<Option<String>>,
    publishable: bool,
    created: Mutex<Vec<(String, String)>>,
    /// Die inside `Ref.CreateIntegration`, which is the P7 prefix.
    kill_on_create: bool,
}

impl FakeRefs {
    fn empty() -> Self {
        Self {
            at: Mutex::new(None),
            publishable: true,
            created: Mutex::new(Vec::new()),
            kill_on_create: false,
        }
    }

    fn at(sha: &str) -> Self {
        Self {
            at: Mutex::new(Some(sha.to_owned())),
            ..Self::empty()
        }
    }

    fn created(&self) -> Vec<(String, String)> {
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl IntegrationRefs for FakeRefs {
    fn assert_publishable(&self, refname: &str) -> Result<(), UpstrokeError> {
        if self.publishable {
            Ok(())
        } else {
            Err(UpstrokeError::Refused {
                message: format!("`{refname}` is checked out in a worktree"),
            })
        }
    }

    fn direct_target(&self, _refname: &str) -> Result<Option<String>, UpstrokeError> {
        Ok(self
            .at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn create_zero_old(
        &self,
        _hooks: &mut dyn EffectHooks,
        refname: &str,
        new: &str,
    ) -> Result<(), UpstrokeError> {
        if self.kill_on_create {
            std::process::abort();
        }
        let mut at = self.at.lock().unwrap_or_else(PoisonError::into_inner);
        if at.is_some() {
            return Err(UpstrokeError::Git {
                message: format!("`{refname}` already exists; zero-old refuses"),
            });
        }
        *at = Some(new.to_owned());
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((refname.to_owned(), new.to_owned()));
        Ok(())
    }
}

// -----------------------------------------------------------------------
// A repository, a private root, and a record the fold accepts
// -----------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    private_root: PathBuf,
    repo_key: RepoKey,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        // A ULID rather than the pid: two `cargo test` runs on one box can
        // share a pid, and a stale private half left by the previous run
        // would make a retention test pass without the code retaining
        // anything. Deletion is a funnel with a proof token here, so a
        // fixture cannot simply wipe its own root.
        let root =
            std::env::temp_dir().join(format!("upstroke-create-{tag}-{}", crate::ulid::ulid()));
        let repo = root.join("repo");
        let private_root = root.join("private");
        // The run-directory funnel, because `std::fs::create_dir_all` is a
        // build error in a `TOPOLOGY_MODULE` — tests included.
        create_private_dir(&repo, &mut NoRunDirHooks).expect("repo root");
        create_private_dir(&private_root, &mut NoRunDirHooks).expect("private root");
        Self {
            repo,
            private_root,
            repo_key: RepoKey::v1(&root.join("git-dir")),
            root,
        }
    }

    /// The same layout at a root that already exists — how a kill child
    /// lands in the directory its parent will inspect.
    fn at(root: &Path) -> Self {
        let repo = root.join("repo");
        let private_root = root.join("private");
        create_private_dir(&repo, &mut NoRunDirHooks).expect("repo root");
        create_private_dir(&private_root, &mut NoRunDirHooks).expect("private root");
        Self {
            repo,
            private_root,
            repo_key: RepoKey::v1(&root.join("git-dir")),
            root: root.to_path_buf(),
        }
    }

    fn checked(&self) -> PreLockChecked {
        let selection = crate::config::RunnerSelection::host_default();
        super::super::prelock::check(&super::super::prelock::PreLock {
            selection: &selection,
            runtime: None,
            private_root: &self.private_root,
            ids: &Fixed::default(),
        })
        .expect("the host runner resolves")
    }

    fn public(&self) -> PathBuf {
        crate::rundir::public_dir(&self.repo, RUN_ID)
    }

    fn private(&self) -> PathBuf {
        std::fs::canonicalize(&self.private_root)
            .expect("the private root canonicalizes")
            .join("runs")
            .join(RUN_ID)
    }

    /// The private root as the marker and the intents spell it.
    fn private_root_canonical(&self) -> PathBuf {
        std::fs::canonicalize(&self.private_root).expect("the private root canonicalizes")
    }

    fn husk(&self) -> crate::rundir::HuskReport {
        crate::rundir::husk_report(&self.repo, RUN_ID, &self.repo_key, &self.private_root)
    }
}

fn plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "frozen-create-hash".to_owned(),
        },
        tasks: vec![Task {
            id: TaskId("alpha".to_owned()),
            kind: TaskKind::Implement,
            title: "Alpha".to_owned(),
            body: "do the thing".to_owned(),
            depends_on: Vec::new(),
            acceptance: vec!["it works".to_owned()],
            path_hints: vec!["src/alpha/".to_owned()],
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::<ArtifactId>::new(),
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

fn normalized_plan() -> Vec<u8> {
    b"{\"tasks\":[\"alpha\"]}\n".to_vec()
}

/// A `run_started(4)` the fold accepts, authenticated against its own
/// registry over `agents`.
///
/// `probed_agents` is stamped by P4, so the digest has to be taken over the
/// agents the request will actually probe — which is what a real caller
/// does too.
fn record(agents: &[String], runner: RunnerPolicy) -> RunStarted4 {
    let plan = plan();
    let unauthenticated = RunStarted4 {
        schema: TOPOLOGY_SCHEMA,
        upstroke_version: "0.2.0".to_owned(),
        run_id: RUN_ID.to_owned(),
        incarnation: IncarnationId(INCARNATION.to_owned()),
        runner,
        probed_agents: agents.to_vec(),
        branch: "main".to_owned(),
        integration_ref: GitRef(INTEGRATION_REF.to_owned()),
        base_sha: CommitSha(BASE_SHA.to_owned()),
        execution_root: "/tmp/upstroke-exec".to_owned(),
        private_dir: "/tmp/upstroke-private".to_owned(),
        plan_path: "docs/plan.md".to_owned(),
        config_path: Some("upstroke.toml".to_owned()),
        plan_hash: plan.source.hash.clone(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
        registry_digest: String::new(),
        path_policy: PathPolicy {
            version: PathPolicyVersion::V1,
            case_fold: false,
            grammar: PathGrammar::Globset,
        },
        limits: TopologyLimits {
            max_parallel: 1,
            max_defers: 2,
            max_merge_repairs: 3,
        },
        gates: Vec::new(),
        gates_from_config: false,
        gate_cmds: Vec::new(),
        interaction_mode: "never".to_owned(),
        chains: vec![ChainSummary {
            task: "alpha".to_owned(),
            tiers: vec![Tier::Small],
            attempts_per: 1,
            bindings: Some(vec![BindingSummary {
                tier: Tier::Small,
                agent: AGENT.to_owned(),
                model: "gpt-5-codex".to_owned(),
                pinned: false,
            }]),
        }],
        effort_policy: ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::Medium,
            frontier: Effort::High,
            review: Effort::High,
        },
        reviews: ReviewPlan::default(),
    };
    let registry_digest = TaskRegistry::originals_with_agents(
        &plan,
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

fn agents() -> Vec<String> {
    vec![AGENT.to_owned()]
}

/// Everything a run needs, assembled around whichever doubles a test wants.
struct Driver<'a> {
    fixture: &'a Fixture,
    probes: &'a dyn Probes,
    refs: &'a dyn IntegrationRefs,
    agents: Vec<String>,
    runner: RunnerPolicy,
    ledger: InvocationLedger,
    slots: SlotAssertion,
    warnings: Vec<String>,
}

impl<'a> Driver<'a> {
    fn new(fixture: &'a Fixture, probes: &'a dyn Probes, refs: &'a dyn IntegrationRefs) -> Self {
        Self {
            fixture,
            probes,
            refs,
            agents: agents(),
            runner: crate::runner::policy::host_policy(),
            ledger: InvocationLedger::new(),
            slots: SlotAssertion::new(),
            warnings: Vec::new(),
        }
    }

    fn run(&mut self, hooks: &mut TestHooks) -> Result<Started, Refused> {
        let plan_bytes = normalized_plan();
        let clock = Fixed::default();
        let request = Request {
            repo_root: &self.fixture.repo,
            repo_key: self.fixture.repo_key.clone(),
            normalized_plan: &plan_bytes,
            inputs: inputs(),
            record: record(&self.agents, self.runner.clone()),
            agents: &self.agents,
            probes: self.probes,
            refs: self.refs,
            clock: &clock,
            ledger: &mut self.ledger,
            slots: &mut self.slots,
        };
        create_run(self.fixture.checked(), request, hooks, &mut self.warnings)
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The first committed line of a run's log, as the event it records.
fn committed_first_line(public: &Path) -> Option<TopologyEvent> {
    let bytes = std::fs::read(public.join(EVENT_LOG)).ok()?;
    let end = bytes.iter().position(|byte| *byte == b'\n')?;
    serde_json::from_slice(&bytes[..end]).ok()
}

fn marker_of(public: &Path) -> CreatingMarker {
    let text = std::fs::read_to_string(public.join(MARKER)).expect("the marker is published");
    serde_json::from_str(&text).expect("the marker parses")
}

fn owner_of(private: &Path) -> OwnerRecord {
    let text =
        std::fs::read_to_string(private.join(OWNER_RECORD)).expect("the owner record exists");
    serde_json::from_str(&text).expect("the owner record parses")
}

// =======================================================================
// The happy path, and the order it runs in
// =======================================================================

/// P0-P8, in `run_creation`'s order, each step through its own site.
///
/// The ordering claims that are not visible from the returned value are
/// asserted from the harness and from the tree: every site of the sequence
/// was reached, the private skeleton exists *and* the owner record does
/// (O08 is asserted separately by the P3a tests, which is where the
/// ordering is observable), and the marker is gone while the log is
/// committed.
#[test]
fn the_publication_prefixes_run_in_the_packets_order() {
    let fixture = Fixture::new("happy");
    let probes = RecordingProbes::new(&crate::runner::policy::runner_policy_sha256(
        &crate::runner::policy::host_policy(),
    ));
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let started = driver
        .run(&mut hooks)
        .map_err(Refused::into_error)
        .expect("P0-P8");

    assert_eq!(started.run_id(), RUN_ID);
    // P4: the shell probe, then the agent probe, in that order.
    let calls = probes.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].starts_with("shell "), "{calls:?}");
    assert_eq!(calls[1], format!("agent {AGENT}"));
    assert_eq!(
        started.run_started().probed_agents,
        agents(),
        "`run_started` records the agents pre-flight actually probed"
    );

    // P5, P5b, P6, P7, P8, on disk.
    assert!(fixture.public().join(PLAN).is_file(), "P5 wrote the plan");
    assert!(
        fixture.private().join(COMMIT_RECORD).is_file(),
        "P5b published the commit record"
    );
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Committed,
        "P6 made `run_started` durable"
    );
    assert!(
        !fixture.public().join(MARKER).exists(),
        "P7 removed the marker"
    );
    assert_eq!(
        refs.created(),
        vec![(INTEGRATION_REF.to_owned(), BASE_SHA.to_owned())],
        "P8 created the integration ref zero-old at the recorded base"
    );

    // The private skeleton exists, and so does the record that had to
    // precede it.
    let private = names_in(&fixture.private());
    assert!(private.contains(&OWNER_RECORD.to_owned()), "{private:?}");
    for name in [
        "transcripts",
        "reviews",
        "settings",
        "gates",
        "gate-worktrees",
    ] {
        assert!(private.contains(&name.to_owned()), "{private:?}");
    }

    // The run the caller is handed: the exact bytes on disk, folded once.
    assert_eq!(
        started.event().body.kind(),
        "run_started",
        "the event the fold applied is the one P6 appended"
    );
    assert_eq!(
        started.paths().public,
        fixture.public(),
        "both halves are the ones the witness carried"
    );
    assert_eq!(started.paths().private, fixture.private());
    assert!(
        !started.fold().is_poisoned(),
        "a run that started must not hand back a poisoned fold"
    );

    // Every publication site of the sequence was driven through its funnel.
    for site in [
        RunDirSite::CreatePublicDir,
        RunDirSite::StageMarker,
        RunDirSite::PublishMarker,
        RunDirSite::CreatePrivateDir,
        RunDirSite::StageOwnerRecord,
        RunDirSite::PublishOwnerRecord,
        RunDirSite::WritePlan,
        RunDirSite::StageCommitRecord,
        RunDirSite::PublishCommitRecord,
        RunDirSite::RemoveMarker,
    ] {
        assert!(
            hooks.observed(EffectSiteId::RunDir(site), HookPhase::Before),
            "`RunDir.{}` was never reached",
            site.name()
        );
    }
    assert!(
        hooks.observed(EffectSiteId::Lock(LockSite::AcquireRun), HookPhase::Before),
        "P2 did not go through the lock funnel"
    );
    assert!(
        hooks.observed(
            EffectSiteId::Event(EventSite::AppendFirst),
            HookPhase::Before
        ),
        "P6 did not go through the append funnel"
    );

    // And the run comes apart into the four things the loop drives.
    let mut started = started;
    assert_eq!(
        started.log().opened_at(),
        EventSite::OpenLog,
        "the append handle is the schema-4 one P5 opened, not a legacy handle"
    );
    let (paths, lock, log, fold, event) = started.into_parts();
    assert_eq!(paths.public, fixture.public());
    assert_eq!(log.path(), fixture.public().join(EVENT_LOG));
    assert!(!fold.is_poisoned());
    assert_eq!(event.body.kind(), "run_started");
    drop(lock);
}

/// INV-23: `run_started(4).runner` is the policy resolved **before the
/// worktree lock**, not the one the caller handed in.
///
/// The caller's record names a container boundary; the pre-lock checks
/// resolved the host. Four copies of one record have to agree — the
/// marker's digest (P1), `owner.json.runner` (P3b), the probes' boundary
/// (P4), and `run_started(4).runner` (P6) — and all four are asserted
/// against the witness rather than against each other, so a stamp that took
/// the *caller's* value would agree with itself and fail here.
#[test]
fn run_started_records_runner_policy_resolved_before_worktree_lock() {
    let fixture = Fixture::new("policy");
    let resolved = crate::runner::policy::host_policy();
    let digest = crate::runner::policy::runner_policy_sha256(&resolved);
    let probes = RecordingProbes::new(&digest);
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    let mut driver = Driver::new(&fixture, &probes, &refs);
    // What the caller claims, and what pre-flight actually resolved, differ.
    driver.runner = container_policy();
    assert_ne!(driver.runner, resolved);

    // The marker is read before P7 removes it, so the run is stopped at P6.
    hooks.faults().arm_phase(
        EffectSiteId::RunDir(RunDirSite::RemoveMarker),
        HookPhase::Before,
        Injection::Error,
    );
    let refused = driver.run(&mut hooks).expect_err("P7 was made to fail");
    assert_eq!(refused.reached, Prefix::P6);

    let marker = marker_of(&fixture.public());
    assert_eq!(
        marker.runner_policy_sha256, digest,
        "P1's marker carries the digest of the policy resolved before the lock"
    );
    let owner = owner_of(&fixture.private());
    assert_eq!(owner.runner, resolved, "P3b records it in full");
    let event = committed_first_line(&fixture.public()).expect("run_started is committed");
    let TopologyEventBody::RunStarted { data } = event.body else {
        panic!("the first line is not `run_started`");
    };
    assert_eq!(data.runner, resolved, "P6 records the same record");
    assert_eq!(
        crate::runner::policy::runner_policy_sha256(&data.runner),
        marker.runner_policy_sha256,
        "and the marker's digest is that record's"
    );
    assert_eq!(data.incarnation.0, INCARNATION);
    assert_eq!(data.run_id, RUN_ID);
    assert_eq!(
        data.private_dir,
        fixture.private().to_string_lossy(),
        "the recorded locator is the authorized one, not the caller's"
    );
}

// =======================================================================
// The deletion boundary
// =======================================================================

/// **P3a: the creator removes neither half.**
///
/// ST-19, verbatim: "the private directory exists without an owner record —
/// unprovable — so both halves are retained and reported (content-free by
/// ordering; deferred prune)". The named sibling of
/// `creator_error_before_commit_record_removes_both_halves`, which is scoped
/// to P3b-P5 precisely because this prefix answers differently.
///
/// Two windows, and both retain. The tree labels `stage_owner_record` P3a
/// and `publish_owner_record` P3b, while ST-19's P3a is the
/// private-directory-without-owner-record *prefix* — so "P3a" spans before
/// staging (empty, content-free) and after it (holding `owner.json.tmp`,
/// which is **not** content-free, and the report says so).
///
/// Removing the public half here instead would orphan the private one
/// permanently: the only production `read_dir` over a runs root is
/// `rundir::run_dir_names` over `<repo>/.upstroke/runs`, and the private half
/// is reachable only through the marker inside the public husk. The second
/// half of this test is that assertion.
#[test]
fn creator_error_at_p3a_retains_both_halves_and_reports_them() {
    for (tag, site, residue) in [
        ("p3a-empty", RunDirSite::StageOwnerRecord, Vec::new()),
        (
            "p3a-staged",
            RunDirSite::PublishOwnerRecord,
            vec![OWNER_RECORD_STAGED.to_owned()],
        ),
    ] {
        let fixture = Fixture::new(tag);
        let probes = RecordingProbes::new(&host_digest());
        let refs = FakeRefs::empty();
        let mut hooks = TestHooks::new();
        hooks.faults().arm_phase(
            EffectSiteId::RunDir(site),
            HookPhase::Before,
            Injection::Error,
        );
        let mut driver = Driver::new(&fixture, &probes, &refs);
        let refused = driver
            .run(&mut hooks)
            .expect_err("{tag}: the step was made to fail");

        assert!(
            !refused.disposition.removed_anything(),
            "{tag}: {:?}",
            refused.disposition
        );
        assert!(
            matches!(
                *refused.disposition,
                Disposition::Retained {
                    reason: RetainReason::OwnerRecordMissing,
                    ..
                }
            ),
            "{tag}: {:?}",
            refused.disposition
        );
        assert!(
            fixture.public().is_dir(),
            "{tag}: the public half was removed, which would orphan the private one"
        );
        assert!(
            fixture.private().is_dir(),
            "{tag}: the private half is gone"
        );
        assert_eq!(
            names_in(&fixture.private()),
            residue,
            "{tag}: the P3a residue is not what the report claims"
        );
        assert!(
            refused.disposition.describe().contains("retained"),
            "{tag}: the report does not say the halves were retained"
        );
        assert!(
            !hooks.observed(
                EffectSiteId::RunDir(RunDirSite::RemovePublicHusk),
                HookPhase::Before
            ) && !hooks.observed(
                EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
                HookPhase::Before
            ),
            "{tag}: a removal funnel was entered for an unprovable husk"
        );
        assert!(
            hooks.observed(EffectSiteId::Lock(LockSite::Release), HookPhase::Before),
            "{tag}: the run lock was not released through `Lock.Release`, so the next \
             census would skip this husk instead of reporting it"
        );

        // And the census reaches the same answer, which is the convergence
        // property: a kill here and an error here leave one shape.
        let husk = fixture.husk();
        assert!(
            matches!(
                husk.disposition,
                HuskDisposition::Retained(RetainReason::OwnerRecordMissing)
            ),
            "{tag}: {:?}",
            husk.disposition
        );
        assert_eq!(
            husk.locator.as_deref(),
            Some(fixture.private().as_path()),
            "{tag}: the report must name the private half it retained"
        );
    }
}

/// P3b-P5, and the P5b case where a read-only stat shows the record absent:
/// the creating process, which holds both locks and knows the run never
/// committed, removes both halves.
#[test]
fn creator_error_before_commit_record_removes_both_halves() {
    for (tag, site, phase) in [
        // P3b-P5: the plan write fails after the owner record exists.
        ("plan", RunDirSite::WritePlan, HookPhase::Before),
        // P5b with the record absent: the staging file never landed.
        ("stage", RunDirSite::StageCommitRecord, HookPhase::Before),
    ] {
        let fixture = Fixture::new(tag);
        let probes = RecordingProbes::new(&host_digest());
        let refs = FakeRefs::empty();
        let mut hooks = TestHooks::new();
        hooks
            .faults()
            .arm_phase(EffectSiteId::RunDir(site), phase, Injection::Error);
        let mut driver = Driver::new(&fixture, &probes, &refs);
        let refused = driver
            .run(&mut hooks)
            .expect_err("the step was made to fail");

        assert!(
            matches!(*refused.disposition, Disposition::BothHalvesRemoved { .. }),
            "{tag}: {:?}",
            refused.disposition
        );
        assert!(
            !fixture.private().exists(),
            "{tag}: the private half survived"
        );
        assert!(
            !fixture.public().exists(),
            "{tag}: the public half survived"
        );
        assert!(
            hooks.observed(
                EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
                HookPhase::Before
            ),
            "{tag}: the private half was removed outside the proof-token funnel"
        );
        assert!(
            hooks.observed(
                EffectSiteId::RunDir(RunDirSite::RemovePublicHusk),
                HookPhase::Before
            ),
            "{tag}: the public half was removed outside its funnel"
        );
        assert!(
            hooks.observed(EffectSiteId::Lock(LockSite::Release), HookPhase::Before),
            "{tag}: the run lock was not released through `Lock.Release` before the \
             directory holding `run.lock` was removed"
        );
    }
}

/// A failing pre-flight probe at P4 is a returned error **before P5b**, so
/// it removes both halves.
///
/// And the shell probe goes first: a machine whose shell does not run
/// `exit 0` never spends a slot on an agent.
#[test]
fn failing_preflight_probe_at_p4_removes_both_halves() {
    // (1) The agent probe fails, after the shell probe ran.
    let fixture = Fixture::new("probe-agent");
    let mut probes = RecordingProbes::new(&host_digest());
    probes.refuse_agent = Some(AGENT.to_owned());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver.run(&mut hooks).expect_err("the agent probe refused");
    assert!(
        matches!(*refused.disposition, Disposition::BothHalvesRemoved { .. }),
        "{:?}",
        refused.disposition
    );
    assert!(!fixture.private().exists());
    assert!(!fixture.public().exists());
    assert_eq!(probes.calls().len(), 2, "{:?}", probes.calls());

    // (2) The shell probe fails, and no agent is probed at all.
    let fixture = Fixture::new("probe-shell");
    let mut probes = RecordingProbes::new(&host_digest());
    probes.refuse_shell = true;
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver.run(&mut hooks).expect_err("the shell probe refused");
    assert!(
        matches!(*refused.disposition, Disposition::BothHalvesRemoved { .. }),
        "{:?}",
        refused.disposition
    );
    assert_eq!(
        probes.calls().len(),
        1,
        "an agent was probed after the shell probe failed: {:?}",
        probes.calls()
    );
    assert!(!fixture.public().exists());
}

/// A `PublishCommitRecord` error whose rename **never landed**: the stat
/// says absent, so both halves go.
#[test]
fn commit_record_rename_error_with_record_absent_removes_both_halves() {
    let fixture = Fixture::new("p5b-absent");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    // `Before`: the funnel returns `Err` without performing the rename.
    hooks.faults().arm_phase(
        EffectSiteId::RunDir(RunDirSite::PublishCommitRecord),
        HookPhase::Before,
        Injection::Error,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver.run(&mut hooks).expect_err("P5b was made to fail");

    assert!(
        matches!(*refused.disposition, Disposition::BothHalvesRemoved { .. }),
        "{:?}",
        refused.disposition
    );
    assert!(!fixture.private().exists());
    assert!(!fixture.public().exists());
}

/// A `PublishCommitRecord` error whose rename **did** land: the stat says
/// present, the run is treated as published, and nothing is deleted.
///
/// This is the error-return mode's whole point — the funnel returns `Err`
/// *after* performing the primitive — and it is why the boundary is decided
/// by a stat rather than by the error, which is the identical value on both
/// sides of the rename.
#[test]
fn commit_record_rename_error_with_record_present_treated_as_published_removes_nothing() {
    let fixture = Fixture::new("p5b-present");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    hooks.faults().arm_phase(
        EffectSiteId::RunDir(RunDirSite::PublishCommitRecord),
        HookPhase::After,
        Injection::Error,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver
        .run(&mut hooks)
        .expect_err("P5b was made to fail after the rename");

    assert!(
        fixture.private().join(COMMIT_RECORD).is_file(),
        "the rename landed, which is what `After` means"
    );
    assert!(
        matches!(
            *refused.disposition,
            Disposition::PossiblyCommitted {
                undecidable: None,
                ..
            }
        ),
        "the stat found the record, so `undecidable` must be `None`: {:?}",
        refused.disposition
    );
    assert!(!refused.disposition.removed_anything());
    assert!(fixture.private().is_dir(), "the private half was deleted");
    assert!(fixture.public().is_dir(), "the public half was deleted");
    assert!(
        !hooks.observed(
            EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
            HookPhase::Before
        ),
        "the private-half removal funnel was entered past the deletion boundary"
    );
}

/// The same crossing, reported: a retained, possibly committed husk that the
/// census classifies identically and the deferred prune is the only path to.
///
/// The sibling above asserts the **tree**; this asserts the **report** and
/// the census's agreement with it, which is the convergence property the
/// packet states ("the census later classifies the same shapes
/// identically").
#[test]
fn creator_error_after_commit_record_present_removes_nothing_and_reports_possibly_committed() {
    let fixture = Fixture::new("p5b-report");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    hooks.faults().arm_phase(
        EffectSiteId::RunDir(RunDirSite::PublishCommitRecord),
        HookPhase::After,
        Injection::Error,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver
        .run(&mut hooks)
        .expect_err("P5b was made to fail after the rename");
    let reached = refused.reached;
    let sentence = refused.into_error().to_string();

    assert_eq!(reached, Prefix::P5);
    assert!(
        sentence.contains("possibly") || sentence.contains("may have committed"),
        "the operator is not told the run may have committed: {sentence}"
    );
    assert!(
        sentence.contains("nothing was deleted"),
        "the operator is not told nothing was deleted: {sentence}"
    );

    let husk = fixture.husk();
    assert!(
        matches!(
            husk.disposition,
            HuskDisposition::Retained(RetainReason::PossiblyCommitted)
        ),
        "the census disagrees with the creator: {:?}",
        husk.disposition
    );
}

// =======================================================================
// The append-error protocol
// =======================================================================

/// A partial write: the reopen truncates the torn first line, the proven
/// prefix has no committed line, and nothing is deleted.
#[test]
fn append_first_error_after_partial_write_reopens_truncates_and_reports_not_committed_without_deletion()
 {
    let fixture = Fixture::new("append-partial");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    hooks.arm(
        EventSite::AppendFirst,
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver
        .run(&mut hooks)
        .expect_err("the append returned an error");

    assert!(
        matches!(
            *refused.disposition,
            Disposition::RetainedPossiblyCommittedHusk { .. }
        ),
        "{:?}",
        refused.disposition
    );
    assert!(!refused.disposition.removed_anything());
    assert!(fixture.private().is_dir(), "the private half was deleted");
    assert!(fixture.public().is_dir(), "the public half was deleted");
    let log = std::fs::read(fixture.public().join(EVENT_LOG)).expect("the log is readable");
    assert!(
        log.is_empty(),
        "the torn first line was not truncated by the reopen: {} byte(s)",
        log.len()
    );
    assert!(
        !driver.warnings.is_empty(),
        "the torn tail was truncated without a warning"
    );
    assert!(
        fixture.public().join(MARKER).is_file(),
        "the marker must still be present: nothing on this path is removed"
    );
}

/// A full line then a flush error: the barrier's replay shows the line, the
/// run is reported committed, and the append is **never retried** — one
/// line, not two.
#[test]
fn append_first_flush_error_after_full_line_reports_by_replay_without_retry() {
    let fixture = Fixture::new("append-flush");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    hooks.arm(
        EventSite::AppendFirst,
        SubEffectPoint::WrittenFull,
        InjectionMode::ErrorReturn,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver
        .run(&mut hooks)
        .expect_err("the flush returned an error");

    assert!(
        matches!(*refused.disposition, Disposition::Committed),
        "{:?}",
        refused.disposition
    );
    assert!(!refused.disposition.removed_anything());
    let log = std::fs::read(fixture.public().join(EVENT_LOG)).expect("the log is readable");
    assert_eq!(
        log.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "the append was retried: the log holds more than one committed line"
    );
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Committed,
        "the line the barrier proved is a valid committed `run_started`"
    );
}

/// A sync error after the data reached the disk: reported by replay, and
/// nothing is ever deleted.
#[test]
fn append_first_sync_error_reports_by_replay_and_never_deletes() {
    let fixture = Fixture::new("append-sync");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    hooks.arm(
        EventSite::AppendFirst,
        SubEffectPoint::Synced,
        InjectionMode::ErrorReturn,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver
        .run(&mut hooks)
        .expect_err("the sync returned an error");

    assert!(
        matches!(*refused.disposition, Disposition::Committed),
        "{:?}",
        refused.disposition
    );
    assert!(fixture.private().join(COMMIT_RECORD).is_file());
    assert!(fixture.private().is_dir());
    assert!(fixture.public().is_dir());
    assert!(
        !hooks.observed(
            EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
            HookPhase::Before
        ) && !hooks.observed(
            EffectSiteId::RunDir(RunDirSite::RemovePublicHusk),
            HookPhase::Before
        ),
        "a removal funnel was entered on the append-error path"
    );
}

/// The barrier's own sync fails: the outcome is **undetermined**, the step
/// is named, and nothing is deleted.
#[test]
fn append_first_error_with_failed_prefix_sync_reports_undetermined_and_never_deletes() {
    let fixture = Fixture::new("append-barrier");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    hooks.arm(
        EventSite::AppendFirst,
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
    );
    // Skip P5's own open, which consults the same coordinate.
    hooks.faults().arm_point_after(
        EventSite::OpenLog,
        SubEffectPoint::SyncPrefix,
        InjectionMode::ErrorReturn,
        1,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver
        .run(&mut hooks)
        .expect_err("the append returned an error");

    match &*refused.disposition {
        Disposition::Undetermined { step, .. } => {
            assert_eq!(*step, Some(BarrierStep::SyncPrefix));
        }
        other => panic!("{other:?}"),
    }
    assert!(!refused.disposition.removed_anything());
    assert!(fixture.private().is_dir());
    assert!(fixture.public().is_dir());
    assert!(
        refused.into_error().to_string().contains("undetermined"),
        "the operator is not told the outcome is undetermined"
    );
}

// =======================================================================
// P7 and P8
// =======================================================================

/// A ref that is symbolic, checked out, or at another SHA refuses — and the
/// refusal deletes nothing, because the run already exists.
#[test]
fn foreign_integration_ref_refused() {
    // (1) At another SHA.
    let fixture = Fixture::new("ref-foreign");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::at(OTHER_SHA);
    let mut hooks = TestHooks::new();
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver.run(&mut hooks).expect_err("a foreign ref refuses");
    assert_eq!(refused.reached, Prefix::P7);
    assert!(matches!(*refused.disposition, Disposition::Committed));
    assert!(refs.created().is_empty(), "the foreign ref was overwritten");
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Committed,
        "the run exists: O15 puts the ref after `run_started`"
    );
    assert!(fixture.private().is_dir(), "nothing is deleted at P8");
    let sentence = refused.into_error().to_string();
    assert!(sentence.contains(OTHER_SHA), "{sentence}");

    // (2) Checked out in a worktree.
    let fixture = Fixture::new("ref-checkedout");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs {
        publishable: false,
        ..FakeRefs::empty()
    };
    let mut hooks = TestHooks::new();
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver
        .run(&mut hooks)
        .expect_err("a checked-out ref refuses");
    assert!(matches!(*refused.disposition, Disposition::Committed));
    assert!(refs.created().is_empty());
}

/// P7/P8 after a kill: "create the ref zero-old at the recorded base if
/// absent; if present == base continue (**no spend repeats**)".
///
/// The recovery step is the same function P8 calls, so a resume cannot
/// diverge from a fresh run on the one question — whether the ref is
/// created, adopted, or refused.
#[test]
fn kill_after_run_started_creates_integration_ref() {
    let fixture = Fixture::new("ref-recovery");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    // Stop at P6: `run_started` is durable, the marker is present, and no
    // ref exists — exactly what a kill between P6 and P8 leaves.
    hooks.faults().arm_phase(
        EffectSiteId::RunDir(RunDirSite::RemoveMarker),
        HookPhase::Before,
        Injection::Error,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver.run(&mut hooks).expect_err("P7 was made to fail");
    assert_eq!(refused.reached, Prefix::P6);
    assert!(
        refs.created().is_empty(),
        "no ref exists after a kill before P8"
    );

    // The recovery step creates it, zero-old, at the recorded base.
    ensure_integration_ref(&refs, hooks.effects(), INTEGRATION_REF, BASE_SHA)
        .expect("P7/P8 recovery creates the ref");
    assert_eq!(
        refs.created(),
        vec![(INTEGRATION_REF.to_owned(), BASE_SHA.to_owned())]
    );

    // And repeating it is not a second spend.
    ensure_integration_ref(&refs, hooks.effects(), INTEGRATION_REF, BASE_SHA)
        .expect("present == base continues");
    assert_eq!(
        refs.created().len(),
        1,
        "the ref was created twice; `no spend repeats` is not held"
    );
}

/// A committed run whose marker is still there is **listed** by the readers
/// and repaired by the resume's step (a).
#[test]
fn committed_run_with_stale_marker_listed_and_repaired_by_resume() {
    let fixture = Fixture::new("stale-marker");
    let probes = RecordingProbes::new(&host_digest());
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    hooks.faults().arm_phase(
        EffectSiteId::RunDir(RunDirSite::RemoveMarker),
        HookPhase::Before,
        Injection::Error,
    );
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver.run(&mut hooks).expect_err("P7 was made to fail");
    assert!(matches!(*refused.disposition, Disposition::Committed));

    assert!(
        fixture.public().join(MARKER).is_file(),
        "the marker is what makes this the stale-marker shape"
    );
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Committed
    );
    assert_eq!(
        crate::rundir::list_runs(&fixture.repo),
        vec![RUN_ID.to_owned()],
        "a reader must not hide a committed run because of a marker"
    );

    // The repair: recovery step (a) removes the stale marker through the
    // same funnel P7 uses, and the run is unchanged otherwise.
    hooks.faults().disarm();
    remove_marker(&fixture.public(), hooks.rundir()).expect("the marker is removable");
    assert!(!fixture.public().join(MARKER).exists());
    assert!(!fixture.public().join(MARKER_STAGED).exists());
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Committed
    );
    assert_eq!(
        crate::rundir::list_runs(&fixture.repo),
        vec![RUN_ID.to_owned()]
    );
}

/// A torn first line is reclaimed without a commit record and retained with
/// one — the same bytes, two answers, decided by the boundary and nothing
/// else.
///
/// Both husks are built through the funnels rather than by `create_run`,
/// because the creator publishes `committed.json` **before** it appends: the
/// no-record half of this shape is a husk some other writer left, and the
/// census has to classify it either way.
#[test]
fn torn_first_line_without_commit_record_reclaimed_and_with_commit_record_retained() {
    // (1) No commit record: provable, and reclaimed.
    let fixture = Fixture::new("torn-norecord");
    let mut hooks = TestHooks::new();
    plant_husk_with_a_torn_first_line(&fixture, false, &mut hooks);
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Husk,
        "a torn first line is not a committed run"
    );
    let husk = fixture.husk();
    assert!(
        matches!(
            husk.disposition,
            HuskDisposition::Unstarted(Reclaimable::BothHalves)
        ),
        "{:?}",
        husk.disposition
    );

    // (2) With one: retained, possibly committed, and no token exists.
    let fixture = Fixture::new("torn-record");
    let mut hooks = TestHooks::new();
    plant_husk_with_a_torn_first_line(&fixture, true, &mut hooks);
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Husk,
        "a torn first line is not distinguishable from a truncated committed log"
    );
    let husk = fixture.husk();
    assert!(
        matches!(
            husk.disposition,
            HuskDisposition::Retained(RetainReason::PossiblyCommitted)
        ),
        "{:?}",
        husk.disposition
    );

    // (3) And the same shape left by a real process death inside the
    // append, rather than by an error return: `WrittenShape::Torn` is
    // reachable only through the observer's `written_kill_shape`, so
    // without that override this prefix has no kill at all.
    let fixture = Fixture::new("torn-kill");
    let code = spawn_and_wait(
        "engine::topology::create::tests::create_kill_child",
        &fixture.root,
        "p5btorn",
        60,
    );
    assert_ne!(code, Some(0), "the child must have died");
    let bytes = std::fs::read(fixture.public().join(EVENT_LOG)).expect("the log exists");
    assert!(
        !bytes.is_empty() && bytes.last() != Some(&b'\n'),
        "the kill did not leave a torn first line: {} byte(s)",
        bytes.len()
    );
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Husk
    );
    assert!(
        matches!(
            fixture.husk().disposition,
            HuskDisposition::Retained(RetainReason::PossiblyCommitted)
        ),
        "a torn first line past the commit record is not distinguishable from a truncated \
         committed log, and is retained"
    );
}

/// P0-P3b through the funnels, optionally P5b, then an append that returns
/// an error after a partial write.
fn plant_husk_with_a_torn_first_line(
    fixture: &Fixture,
    with_commit_record: bool,
    hooks: &mut TestHooks,
) {
    let checked = fixture.checked();
    let public = fixture.public();
    let private = fixture.private();
    let policy = crate::runner::policy::host_policy();
    let marker = CreatingMarker {
        run_id: RUN_ID.to_owned(),
        repo_key: fixture.repo_key.as_str().to_owned(),
        private_dir: checked.private_dir().to_string_lossy().into_owned(),
        incarnation: INCARNATION.to_owned(),
        pid: 4242,
        runner_policy_sha256: host_digest(),
    };
    create_public_dir(&public, hooks.rundir()).expect("P0");
    stage_marker(&public, &marker, hooks.rundir()).expect("P1a");
    publish_marker(&public, hooks.rundir()).expect("P1b");
    create_private_dir(&private, hooks.rundir()).expect("P3a");
    let owner = OwnerRecord {
        run_id: RUN_ID.to_owned(),
        repo_key: fixture.repo_key.as_str().to_owned(),
        public_dir: canonical_string(&public),
        incarnation: INCARNATION.to_owned(),
        runner: policy.clone(),
    };
    stage_owner_record(&private, &owner, hooks.rundir()).expect("P3a stage");
    publish_owner_record(&private, hooks.rundir()).expect("P3b");

    let event = TopologyEvent {
        ts: "2026-08-23T09:41:02Z".to_owned(),
        body: TopologyEventBody::RunStarted {
            data: Box::new(record(&agents(), policy)),
        },
    };
    let (line, _) = TopologyLine::round_trip(&event).expect("the record round-trips");
    if with_commit_record {
        let commit = CommitRecord {
            run_id: RUN_ID.to_owned(),
            repo_key: fixture.repo_key.as_str().to_owned(),
            public_dir: canonical_string(&public),
            incarnation: INCARNATION.to_owned(),
            run_started_sha256: first_line_digest(line.committed_bytes())
                .expect("the line carries a commit marker"),
        };
        stage_commit_record(&private, &commit, hooks.rundir()).expect("P5a");
        publish_commit_record(&private, hooks.rundir()).expect("P5b");
    }

    let mut warnings = Vec::new();
    let mut log = EventLog::open_hooked(
        EventSite::OpenLog,
        &public.join(EVENT_LOG),
        &mut warnings,
        hooks.events(),
    )
    .expect("P5 opens the log");
    hooks.arm(
        EventSite::AppendFirst,
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
    );
    log.append_topology_hooked(EventSite::AppendFirst, &line, hooks.events())
        .expect_err("the append was made to tear");
    drop(log);
    let bytes = std::fs::read(public.join(EVENT_LOG)).expect("the log is readable");
    assert!(
        !bytes.is_empty() && bytes.last() != Some(&b'\n'),
        "the fixture did not leave a torn first line: {} byte(s)",
        bytes.len()
    );
}

// =======================================================================
// Kills — a real process death, at every prefix
// =======================================================================

/// Which prefix a kill child stops at, and what a census must then say.
///
/// The table **is** the convergence claim: every entry names a prefix of
/// `run_creation`'s sequence and the answer ST-19's `resume_action` gives
/// for it, and the child reaches the prefix by dying inside a funnel rather
/// than by returning early — an early return unwinds, and what is under test
/// is what a coordinator that runs **no** cleanup leaves on disk.
const KILL_PREFIXES: &[(&str, Prefix)] = &[
    ("p0", Prefix::P0),
    ("p1staged", Prefix::P1Staged),
    ("p1", Prefix::P1),
    ("p2", Prefix::P2),
    ("p3a", Prefix::P3a),
    ("p3astaged", Prefix::P3aStaged),
    ("p3b", Prefix::P3b),
    ("p4", Prefix::P4),
    ("p5", Prefix::P5),
    ("p5b", Prefix::P5b),
    ("p6", Prefix::P6),
    ("p7", Prefix::P7),
    ("p8", Prefix::P8),
];

/// Arm `which` on a fresh bundle and drive `create_run` into it.
///
/// Shared by the child of every kill test so a prefix is reached by exactly
/// one description.
fn drive_into_the_kill(which: &str, fixture: &Fixture) -> ! {
    let mut hooks = TestHooks::new();
    let mut probes = RecordingProbes::new(&host_digest());
    let mut refs = FakeRefs::empty();
    let kill_before = |site| {
        hooks.faults().arm_phase(
            EffectSiteId::RunDir(site),
            HookPhase::Before,
            Injection::Kill,
        );
    };
    match which {
        "p0" => kill_before(RunDirSite::StageMarker),
        "p1staged" => kill_before(RunDirSite::PublishMarker),
        "p1" => {
            hooks.faults().arm_phase(
                EffectSiteId::Lock(LockSite::AcquireRun),
                HookPhase::Before,
                Injection::Kill,
            );
        }
        "p2" => kill_before(RunDirSite::CreatePrivateDir),
        "p3a" => kill_before(RunDirSite::StageOwnerRecord),
        "p3astaged" => kill_before(RunDirSite::PublishOwnerRecord),
        "p3b" => probes.kill_shell = true,
        "p4" => probes.kill_agent = true,
        "p5" => kill_before(RunDirSite::StageCommitRecord),
        "p5b" => {
            // `After`: the rename happened and nothing has been appended,
            // which is exactly the P5b prefix.
            hooks.faults().arm_phase(
                EffectSiteId::RunDir(RunDirSite::PublishCommitRecord),
                HookPhase::After,
                Injection::Kill,
            );
        }
        "p5btorn" => {
            // The other durable shape of a kill inside the first append:
            // a torn first line, past the commit record. `WrittenShape` is
            // an observer's answer and nothing else can produce one, which
            // is why the bundle overrides `written_kill_shape` at all.
            hooks.faults().tear_the_first_line(EventSite::AppendFirst);
            hooks.arm(
                EventSite::AppendFirst,
                SubEffectPoint::Written,
                InjectionMode::Kill,
            );
        }
        "p6" => kill_before(RunDirSite::RemoveMarker),
        "p7" => refs.kill_on_create = true,
        "p8" => {}
        other => panic!("unknown prefix `{other}`"),
    }
    let mut driver = Driver::new(fixture, &probes, &refs);
    let outcome = driver.run(&mut hooks);
    if which == "p8" {
        // Everything is done; the kill is what a coordinator dying with the
        // run complete leaves, which is the whole of P8's durable shape.
        assert!(outcome.is_ok(), "P8 must have been reached");
        std::process::abort();
    }
    unreachable!("the kill must have taken this process");
}

#[test]
#[ignore = "spawned as a subprocess by kill_at_each_prefix_p0_to_p8_converges"]
fn create_kill_child() {
    let root = PathBuf::from(std::env::var("UPSTROKE_TEST_KILL_DIR").expect("dir"));
    let which = std::env::var("UPSTROKE_TEST_KILL_SITE").expect("site");
    let fixture = Fixture::at(&root);
    drive_into_the_kill(&which, &fixture);
}

/// Spawn `child` with the two env vars and wait for it to die.
///
/// Through the `Runner`, because `std::process::Command` is a build error in
/// a `TOPOLOGY_MODULE` — the process funnel is `Process.Spawn` and every
/// process start goes through it, tests included.
fn spawn_and_wait(child: &str, root: &Path, site: &str, ordinal: u32) -> Option<i32> {
    let exe = std::env::current_exe().expect("the test binary");
    let mut command = crate::runner::CommandSpec::new(exe.to_string_lossy().into_owned());
    command.args = vec![
        "--exact".to_owned(),
        child.to_owned(),
        "--ignored".to_owned(),
        "--nocapture".to_owned(),
        "--test-threads".to_owned(),
        "1".to_owned(),
    ];
    command.env = vec![
        (
            "UPSTROKE_TEST_KILL_DIR".to_owned(),
            root.to_string_lossy().into_owned(),
        ),
        ("UPSTROKE_TEST_KILL_SITE".to_owned(), site.to_owned()),
    ];
    let request = crate::runner::gate_request(
        command,
        root.to_path_buf(),
        std::time::Duration::from_secs(120),
        InvocationId::attempt(
            crate::topology::registry::TaskKey(0),
            crate::topology::events::GenerationId(0),
            crate::topology::events::AttemptNumber(1),
            crate::runner::invocation::AttemptRole::Gate(ordinal),
            0,
        ),
    );
    let runner = crate::runner::host::HostRunner::new();
    let output = runner.run(&request).expect("the child spawns");
    assert!(
        !output.timed_out,
        "the child did not die within the timeout"
    );
    output.code
}

/// A kill at every prefix of P0-P8 leaves a shape the census classifies as
/// ST-19's `resume_action` says, and an error return at the same prefix
/// leaves the same one.
///
/// `Injection::Kill` is `std::process::abort()` — a real process death,
/// chosen so the claim is what a coordinator that runs **no** cleanup leaves
/// behind. An early `return` would unwind and prove something weaker.
#[test]
fn kill_at_each_prefix_p0_to_p8_converges() {
    for (ordinal, (which, prefix)) in KILL_PREFIXES.iter().enumerate() {
        let fixture = Fixture::new(&format!("kill-{which}"));
        let code = spawn_and_wait(
            "engine::topology::create::tests::create_kill_child",
            &fixture.root,
            which,
            u32::try_from(ordinal).expect("a small ordinal"),
        );
        assert_ne!(code, Some(0), "`{which}`: the child must have died");

        let public = fixture.public();
        match prefix {
            Prefix::P6 | Prefix::P7 | Prefix::P8 => {
                assert_eq!(
                    crate::rundir::classify_run_dir(&public),
                    crate::rundir::RunDirClass::Committed,
                    "`{which}`: a run killed from P6 on exists"
                );
                assert_eq!(
                    public.join(MARKER).is_file(),
                    *prefix == Prefix::P6,
                    "`{which}`: the marker is present at P6 and gone from P7"
                );
                assert_eq!(
                    crate::rundir::list_runs(&fixture.repo),
                    vec![RUN_ID.to_owned()],
                    "`{which}`: a reader must return it, marker or no marker"
                );
            }
            _ => {
                assert_eq!(
                    crate::rundir::classify_run_dir(&public),
                    crate::rundir::RunDirClass::Husk,
                    "`{which}`: nothing before P6 is a committed run"
                );
                let husk = fixture.husk();
                let expected = expected_disposition(*prefix);
                assert_eq!(
                    describe_disposition(&husk.disposition),
                    expected,
                    "`{which}`: the census does not converge on ST-19's answer ({:?})",
                    husk.disposition
                );
            }
        }
    }
}

/// ST-19's `resume_action`, as a function of the prefix.
fn expected_disposition(prefix: Prefix) -> &'static str {
    match prefix {
        // "P0-P1: the next write command's census reclaims the bare or
        // staged-only public directory (no private half exists by
        // ordering)".
        Prefix::P0 => "public-only:bare",
        Prefix::P1Staged => "public-only:staged",
        // "P1-published and P2: the marker's private target does not exist,
        // so the public husk alone is reclaimed".
        Prefix::P1 | Prefix::P2 => "public-only:target-absent",
        // "P3a: the private directory exists without an owner record —
        // unprovable — so both halves are retained and reported".
        Prefix::P3a | Prefix::P3aStaged => "retained:owner-record-missing",
        // "P3b-P5: the ownership proof passes ... and the census reclaims
        // the private half through the proof-token funnel, then the public
        // directory with the marker last".
        Prefix::P3b | Prefix::P4 | Prefix::P5 => "both-halves",
        // "P5b ...: committed.json exists, so both halves are retained and
        // reported as possibly committed with nothing deleted".
        Prefix::P5b => "retained:possibly-committed",
        Prefix::Nothing | Prefix::P6 | Prefix::P7 | Prefix::P8 => "committed",
    }
}

fn describe_disposition(disposition: &HuskDisposition) -> &'static str {
    match disposition {
        HuskDisposition::Unstarted(Reclaimable::BothHalves) => "both-halves",
        HuskDisposition::Unstarted(Reclaimable::PublicOnly(UnboundShape::Bare)) => {
            "public-only:bare"
        }
        HuskDisposition::Unstarted(Reclaimable::PublicOnly(UnboundShape::StagedMarkerOnly)) => {
            "public-only:staged"
        }
        HuskDisposition::Unstarted(Reclaimable::PublicOnly(UnboundShape::TargetAbsent)) => {
            "public-only:target-absent"
        }
        HuskDisposition::Retained(RetainReason::OwnerRecordMissing) => {
            "retained:owner-record-missing"
        }
        HuskDisposition::Retained(RetainReason::PossiblyCommitted) => "retained:possibly-committed",
        HuskDisposition::Retained(_) => "retained:other",
    }
}

/// The one window `run_creation` says holds no other step: `committed.json`
/// published, `run_started` not yet durable.
///
/// A kill here is indistinguishable from a kill *inside* the append, and
/// both are retained as possibly committed with nothing deleted — which is
/// why no separate never-entered proof exists.
#[test]
fn kill_between_commit_record_and_run_started_retained_as_possibly_committed() {
    let fixture = Fixture::new("kill-boundary");
    let code = spawn_and_wait(
        "engine::topology::create::tests::create_kill_child",
        &fixture.root,
        "p5b",
        90,
    );
    assert_ne!(code, Some(0), "the child must have died");

    assert!(
        fixture.private().join(COMMIT_RECORD).is_file(),
        "the commit record is what makes this the P5b prefix"
    );
    let log = std::fs::read(fixture.public().join(EVENT_LOG)).expect("the log exists");
    assert!(
        log.is_empty(),
        "nothing was appended: {} byte(s)",
        log.len()
    );
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Husk
    );
    let husk = fixture.husk();
    assert!(
        matches!(
            husk.disposition,
            HuskDisposition::Retained(RetainReason::PossiblyCommitted)
        ),
        "{:?}",
        husk.disposition
    );
    assert!(
        husk.disposition.describe().contains("possibly committed"),
        "{}",
        husk.disposition.describe()
    );
}

// =======================================================================
// The worktree lock
// =======================================================================

#[test]
#[ignore = "spawned as a subprocess by creator_versus_census_serialized_by_worktree_lock"]
fn worktree_lock_child() {
    let root = PathBuf::from(std::env::var("UPSTROKE_TEST_KILL_DIR").expect("dir"));
    let repo = root.join("repo");
    let git_dir = root.join("git-dir");
    // The evidence is a directory rather than a printed line: `println!` is
    // a build error in a `TOPOLOGY_MODULE`, and a directory created through
    // the run-directory funnel is a durable answer the parent can stat.
    let outcome = match crate::rundir::WorktreeLock::acquire_in(&repo, &git_dir) {
        Ok(lock) => {
            drop(lock);
            "acquired"
        }
        Err(_) => "refused",
    };
    create_private_dir(&root.join(outcome), &mut NoRunDirHooks).expect("the answer");
}

/// Two write commands in one worktree are serialized by the worktree lock:
/// the second is refused rather than racing the first's creation.
///
/// A second **process**, because the process-local claim table would refuse
/// a second acquisition in this process whatever the OS lock did, and it is
/// the OS lock that serializes a creator against a census.
#[test]
fn creator_versus_census_serialized_by_worktree_lock() {
    let fixture = Fixture::new("worktree-lock");
    let git_dir = fixture.root.join("git-dir");
    create_private_dir(&git_dir, &mut NoRunDirHooks).expect("a git dir to lock in");

    // The control half: with nothing holding it, the child acquires.
    let code = spawn_and_wait(
        "engine::topology::create::tests::worktree_lock_child",
        &fixture.root,
        "free",
        80,
    );
    assert_eq!(code, Some(0), "the child ran");
    assert!(
        fixture.root.join("acquired").is_dir(),
        "an unheld worktree lock must be acquirable, or the test below proves nothing"
    );
    assert!(!fixture.root.join("refused").exists());

    // Held by this process — the creator — the child is refused.
    let held = crate::rundir::WorktreeLock::acquire_in(&fixture.repo, &git_dir)
        .expect("the creator takes the worktree lock");
    let fixture2 = Fixture::new("worktree-lock-2");
    create_private_dir(&fixture2.root.join("repo"), &mut NoRunDirHooks).expect("repo");
    // The same git dir: the lock is repository-scoped, not run-scoped.
    let child_root = fixture.root.join("second");
    create_private_dir(&child_root.join("repo"), &mut NoRunDirHooks).expect("repo");
    let link = fixture.root.join("git-dir");
    assert!(link.is_dir());
    let code = spawn_and_wait(
        "engine::topology::create::tests::worktree_lock_child",
        &fixture.root,
        "held",
        81,
    );
    assert_eq!(code, Some(0), "the child ran");
    assert!(
        fixture.root.join("refused").is_dir(),
        "a second write command in the same worktree was not refused"
    );
    drop(held);
}

// =======================================================================
// Refused before any lock
// =======================================================================

/// An image reference absent from the runtime refuses **before any lock**,
/// and the assertion is that no R25 lock file was created — not merely that
/// the command refused.
///
/// `Lock.CreateWorktreeLockFile` is a site of its own precisely because the
/// file "spans runs; never removed by a run": once it exists, the refusal
/// has left a durable artifact behind, and "before any lock or **other
/// effect**" is false.
#[test]
fn image_absent_refused_before_any_lock_no_lock_file_created() {
    let fixture = Fixture::new("image-absent");
    let git_dir = fixture.root.join("git-dir");
    create_private_dir(&git_dir, &mut NoRunDirHooks).expect("a git dir");
    let lock_file = git_dir.join("upstroke-worktree.lock");
    let selection = crate::config::RunnerSelection {
        kind: crate::topology::events::RunnerKind::Container,
        image: Some("ghcr.io/upstroke/absent:9".to_owned()),
        credential_volumes: std::collections::BTreeMap::new(),
        mounts: Vec::new(),
        from_config: true,
    };

    // The write command's start, in order: the read-only checks, then the
    // lock. A refusal in the first statement never reaches the second.
    let refused = write_command_start(&fixture, &selection, &git_dir, &Inventory::reachable())
        .expect_err("an absent image reference refuses");
    assert!(
        refused.to_string().contains("absent:9"),
        "the refusal does not name the image: {refused}"
    );
    assert!(
        !lock_file.exists(),
        "the R25 worktree lock file was created by a command that refused before any lock"
    );
    assert!(
        !crate::rundir::runs_root(&fixture.repo).exists(),
        "a run directory was created by a command that refused before any lock"
    );

    // The control half. Without it this test passes against a
    // `write_command_start` that never takes a lock at all.
    let present =
        Inventory::reachable().with_image("ghcr.io/upstroke/absent:9", "sha256:abc", None);
    let lock =
        write_command_start(&fixture, &selection, &git_dir, &present).expect("the image is there");
    assert!(
        lock_file.is_file(),
        "the same sequence with the image present must create the R25 lock file"
    );
    drop(lock);
}

/// The two statements every schema-4 write command opens with: O01's
/// read-only checks, then O02's worktree lock.
fn write_command_start(
    fixture: &Fixture,
    selection: &crate::config::RunnerSelection,
    git_dir: &Path,
    runtime: &dyn crate::runner::container::runtime::ContainerRuntime,
) -> Result<crate::rundir::WorktreeLock, UpstrokeError> {
    let _checked = super::super::prelock::check(&super::super::prelock::PreLock {
        selection,
        runtime: Some(runtime),
        private_root: &fixture.private_root,
        ids: &Fixed::default(),
    })?;
    crate::rundir::WorktreeLock::acquire_in(&fixture.repo, git_dir)
}

// =======================================================================
// A container runtime with no daemon behind it
// =======================================================================

/// A `ContainerRuntime` that answers from an in-memory inventory.
///
/// Written here rather than reached for: `runner::container::fake` is
/// `#[cfg(test)] mod fake` and is private to `runner::container`, so a
/// sibling subtree cannot name it. Implementing the trait is not calling
/// one of its methods, which is what the effect denylist forbids — every
/// body below is pure bookkeeping.
///
/// **No daemon, deliberately.** The container-shaped assertions here are
/// about the intent record and the census's boundary report, both of which
/// are files and values rather than anything Docker does.
/// Every container the fake runtime holds: its liveness and its labels.
type ContainerState = BTreeMap<String, (Liveness, BTreeMap<String, String>)>;

#[derive(Debug, Default)]
struct Inventory {
    reachable: bool,
    images: BTreeMap<String, (String, Option<String>)>,
    volumes: Vec<String>,
    state: Mutex<ContainerState>,
    ops: Mutex<Vec<String>>,
}

impl Inventory {
    fn reachable() -> Self {
        Self {
            reachable: true,
            ..Self::default()
        }
    }

    fn with_image(mut self, reference: &str, id: &str, digest: Option<&str>) -> Self {
        self.images.insert(
            reference.to_owned(),
            (id.to_owned(), digest.map(str::to_owned)),
        );
        self
    }

    fn with_volume(mut self, name: &str) -> Self {
        self.volumes.push(name.to_owned());
        self
    }

    /// Put a running container into the runtime, the way one survives the
    /// process that started it.
    fn seed_running(&self, name: &str, labels: BTreeMap<String, String>) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_owned(), (Liveness::Running, labels));
    }

    fn ops(&self) -> Vec<String> {
        self.ops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn note(&self, what: String) {
        self.ops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(what);
    }
}

impl crate::runner::container::runtime::ContainerRuntime for Inventory {
    fn probe(&self) -> Result<(), RuntimeError> {
        if self.reachable {
            Ok(())
        } else {
            Err(RuntimeError::Unreachable {
                operation: RuntimeOp::Probe,
                detail: "no runtime on this machine".to_owned(),
            })
        }
    }

    fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        Ok(self
            .images
            .get(reference)
            .map(|(id, digest)| ImageInspection {
                id: id.clone(),
                digest: digest.clone(),
                references: vec![reference.to_owned()],
            }))
    }

    fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        Ok(self
            .images
            .values()
            .find(|(known, _)| known == id)
            .map(|(known, digest)| ImageInspection {
                id: known.clone(),
                digest: digest.clone(),
                references: Vec::new(),
            }))
    }

    fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
        Ok(self.volumes.iter().any(|known| known == name))
    }

    fn containers_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(_, (_, labels))| labels.get(key).map(String::as_str) == Some(value))
            .map(|(name, (_, labels))| DiscoveredContainer {
                name: name.clone(),
                labels: labels.clone(),
            })
            .collect())
    }

    fn observe(&self, name: &str) -> Result<Liveness, RuntimeError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .map_or(Liveness::Gone, |(liveness, _)| *liveness))
    }

    fn collect(&self, _name: &str) -> Result<ContainerExecution, RuntimeError> {
        Ok(ContainerExecution {
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {
        self.note(format!("create {}", spec.name));
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(spec.name.clone(), (Liveness::Exited, spec.labels.clone()));
        Ok(CreatedContainer {
            name: spec.name.clone(),
            reported_image_id: spec.image_id.clone(),
        })
    }

    fn start(&self, name: &str) -> Result<(), RuntimeError> {
        self.note(format!("start {name}"));
        Ok(())
    }

    fn stop(&self, name: &str, mode: StopMode) -> Result<(), RuntimeError> {
        self.note(format!("stop {name} {}", mode.name()));
        if let Some(entry) = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(name)
        {
            entry.0 = Liveness::Exited;
        }
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), RuntimeError> {
        self.note(format!("remove {name}"));
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
        Ok(())
    }
}

// =======================================================================
// Containerized probes
// =======================================================================

const IMAGE_REFERENCE: &str = "ghcr.io/upstroke/sandbox:1";
const IMAGE_ID: &str = "sha256:cccc111122223333444455556666777788889999aaaabbbbccccddddeeeeffff";
const CREDENTIAL_VOLUME: &str = "upstroke-creds-codex";

fn container_selection() -> crate::config::RunnerSelection {
    crate::config::RunnerSelection {
        kind: crate::topology::events::RunnerKind::Container,
        image: Some(IMAGE_REFERENCE.to_owned()),
        credential_volumes: [(AGENT.to_owned(), CREDENTIAL_VOLUME.to_owned())]
            .into_iter()
            .collect(),
        mounts: Vec::new(),
        from_config: true,
    }
}

fn container_runtime() -> Inventory {
    Inventory::reachable()
        .with_image(IMAGE_REFERENCE, IMAGE_ID, Some("sha256:manifest"))
        .with_volume(CREDENTIAL_VOLUME)
}

/// The probes of a containerized run: the real [`ContainerRunner`], over a
/// runtime with no daemon behind it.
///
/// [`ContainerRunner`]: crate::runner::container::exec::ContainerRunner
struct ContainerProbes {
    runner: crate::runner::container::exec::ContainerRunner,
    workspace: PathBuf,
}

impl ContainerProbes {
    /// `hooks` is installed **on the runner**: `Runner::run` takes no
    /// observer, so the container funnel's hooks are the ones the runner
    /// was built with. A bundle that only reached the funnel through
    /// `TopologyHooks::container` would arm nothing a probe executes.
    fn new(fixture: &Fixture, checked: &PreLockChecked, hooks: ArmedContainer) -> Self {
        let identity = crate::runner::container::exec::RunIdentity {
            private_root: checked.private_root().to_path_buf(),
            run_id: checked.run_id().to_owned(),
            run_dir: fixture.public(),
            incarnation: checked.incarnation().0.clone(),
            repo_key: fixture.repo_key.as_str().to_owned(),
        };
        let runner = crate::runner::container::exec::ContainerRunner::new(
            checked.runner_policy().clone(),
            identity,
            &fixture.repo,
            crate::runner::container::env::ContainerEnvironment::from_image(vec![(
                "PATH".to_owned(),
                "/usr/bin:/bin".to_owned(),
            )]),
            Box::new(container_runtime()),
        )
        .expect("a container runner over the resolved policy")
        .with_hooks(Box::new(hooks));
        Self {
            runner,
            workspace: fixture.repo.clone(),
        }
    }
}

impl Probes for ContainerProbes {
    fn policy_digest(&self) -> &str {
        self.runner.policy_digest()
    }

    fn shell(&self, invocation: InvocationId) -> Result<(), UpstrokeError> {
        crate::runner::host::run_shell_probe(
            &self.runner,
            crate::gates::ShellKind::Sh,
            self.workspace.clone(),
            invocation,
        )
    }

    fn agent(&self, _agent: &str) -> Result<(), UpstrokeError> {
        Ok(())
    }
}

#[test]
#[ignore = "spawned as a subprocess by the containerized-probe tests"]
fn container_probe_kill_child() {
    let root = PathBuf::from(std::env::var("UPSTROKE_TEST_KILL_DIR").expect("dir"));
    let fixture = Fixture::at(&root);
    let selection = container_selection();
    let runtime = container_runtime();
    let checked = super::super::prelock::check(&super::super::prelock::PreLock {
        selection: &selection,
        runtime: Some(&runtime),
        private_root: &fixture.private_root,
        ids: &Fixed::default(),
    })
    .expect("the container policy resolves by inspection");
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    let probes = ContainerProbes::new(&fixture, &checked, hooks.container_double());
    // A real process death **inside** the containerized probe, after
    // `Container.WriteIntent` and `Container.Create`: the durable residue is
    // an intent record and a container nobody owns.
    hooks.faults().arm_phase(
        EffectSiteId::Container(crate::topology::effects::ContainerSite::Start),
        HookPhase::Before,
        Injection::Kill,
    );
    let plan_bytes = normalized_plan();
    let clock = Fixed::default();
    let agents = agents();
    let mut ledger = InvocationLedger::new();
    let mut slots = SlotAssertion::new();
    let request = Request {
        repo_root: &fixture.repo,
        repo_key: fixture.repo_key.clone(),
        normalized_plan: &plan_bytes,
        inputs: inputs(),
        record: record(&agents, checked.runner_policy().clone()),
        agents: &agents,
        probes: &probes,
        refs: &refs,
        clock: &clock,
        ledger: &mut ledger,
        slots: &mut slots,
    };
    let mut warnings = Vec::new();
    let _ = create_run(checked, request, &mut hooks, &mut warnings);
    unreachable!("the kill must have taken this process");
}

/// Every intent under `<R>/containers`, with the name it was filed as.
fn intents(
    private_root: &Path,
) -> Vec<(String, crate::runner::container::intent::ContainerIntent)> {
    let dir = crate::runner::container::intent::containers_dir(private_root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".intent") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Ok(record) = serde_json::from_str(&text) {
            found.push((name, record));
        }
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

/// INV-23: "carried as a digest by every container intent", and the digest
/// is the run's — the one the owner record spells out in full.
///
/// Asserted against `sha256(owner.json.runner)` rather than against the
/// value the run was constructed with, so a probe executing under some
/// other boundary is caught rather than agreeing with itself. The other
/// half of the claim — that P4 refuses when the two disagree — is the
/// second block.
#[test]
fn probe_intent_carries_runner_policy_digest_matching_owner_record() {
    let fixture = Fixture::new("intent-digest");
    let code = spawn_and_wait(
        "engine::topology::create::tests::container_probe_kill_child",
        &fixture.root,
        "container",
        70,
    );
    assert_ne!(code, Some(0), "the child must have died");

    let owner = owner_of(&fixture.private());
    let expected = crate::runner::policy::runner_policy_sha256(&owner.runner);
    let found = intents(&fixture.private_root);
    assert!(
        !found.is_empty(),
        "a containerized probe left no intent under {}",
        crate::runner::container::intent::containers_dir(&fixture.private_root).display()
    );
    for (name, intent) in &found {
        assert_eq!(
            intent.runner_policy_sha256, expected,
            "`{name}`'s intent names a boundary the owner record does not describe"
        );
        assert_eq!(intent.run_id, RUN_ID, "`{name}`");
        assert_eq!(intent.incarnation, INCARNATION, "`{name}`");
        assert_eq!(intent.repo_key, fixture.repo_key.as_str(), "`{name}`");
    }
    assert_ne!(
        expected,
        host_digest(),
        "the fixture ran under the host boundary, so the assertion above is trivially true"
    );

    // And P4 refuses before the first probe when the probes' boundary is
    // not the one P1 and P3b published.
    let fixture = Fixture::new("intent-digest-refusal");
    let probes = RecordingProbes::new("sha256:not-this-run");
    let refs = FakeRefs::empty();
    let mut hooks = TestHooks::new();
    let mut driver = Driver::new(&fixture, &probes, &refs);
    let refused = driver
        .run(&mut hooks)
        .expect_err("a foreign boundary refuses");
    assert!(
        probes.calls().is_empty(),
        "a probe ran under a boundary the owner record does not describe"
    );
    assert!(
        matches!(*refused.disposition, Disposition::BothHalvesRemoved { .. }),
        "a P4 refusal is a creator error before P5b: {:?}",
        refused.disposition
    );
}

/// A kill during a containerized probe, before `run_started`: the next
/// census reclaims the container **and** the husk, and reports the
/// container's boundary from the intent's `runner_policy_sha256`.
#[test]
fn kill_during_containerized_probe_before_run_started_reclaims_container_and_husk_and_reports_boundary()
 {
    let fixture = Fixture::new("container-kill");
    let code = spawn_and_wait(
        "engine::topology::create::tests::container_probe_kill_child",
        &fixture.root,
        "container",
        71,
    );
    assert_ne!(code, Some(0), "the child must have died");

    // The durable residue of the kill: the husk, and an owned intent.
    assert_eq!(
        crate::rundir::classify_run_dir(&fixture.public()),
        crate::rundir::RunDirClass::Husk
    );
    let found = intents(&fixture.private_root);
    assert_eq!(found.len(), 1, "{found:?}");
    let (file_name, intent) = &found[0];
    let name = crate::runner::container::intent::ContainerName::from_intent_file_name(file_name)
        .expect("the intent file name parses")
        .expect("it is an intent file");

    // Step (a): the container outlives the process that started it, so a
    // fresh runtime is seeded with it exactly as `docker ps` would report.
    let runtime = container_runtime();
    runtime.seed_running(
        name.as_str(),
        intent.labels(&fixture.private_root_canonical()),
    );
    let start = crate::runner::container::census::CensusStart::FreshRun {
        incarnation: "01KZTOTHERINCARNATION00001".to_owned(),
    };
    let view = DiscardOnly;
    let liveness = crate::runner::container::runtime::LockProbe;
    let mut hooks = TestHooks::new();
    let complete = crate::runner::container::census::run_startup_census(
        hooks.container(),
        &crate::runner::container::census::Census {
            private_root: &fixture.private_root_canonical(),
            start: &start,
            runtime: &runtime,
            liveness: &liveness,
            view: &view,
        },
    )
    .expect("the census runs");

    let report = complete.report();
    assert_eq!(report.reclaimed.len(), 1, "{report:?}");
    let reclaimed = &report.reclaimed[0];
    assert_eq!(reclaimed.run_id, RUN_ID);
    assert_eq!(reclaimed.incarnation, INCARNATION);
    assert_eq!(
        reclaimed.boundary.digest(),
        Some(intent.runner_policy_sha256.as_str()),
        "the census does not report the reclaimed container's boundary from its intent"
    );
    assert!(
        runtime.ops().iter().any(|op| op.starts_with("stop ")),
        "the container was not killed: {:?}",
        runtime.ops()
    );
    assert!(
        runtime.ops().iter().any(|op| op.starts_with("remove ")),
        "the container was not removed: {:?}",
        runtime.ops()
    );
    assert!(
        intents(&fixture.private_root).is_empty(),
        "the intent survived its container's reclaim"
    );

    // Step (b): the husk. The proof passes — the creator published both
    // records and never crossed P5b — so both halves are reclaimed, private
    // first and the public directory with the marker last.
    let husk = fixture.husk();
    assert!(
        matches!(
            husk.disposition,
            HuskDisposition::Unstarted(Reclaimable::BothHalves)
        ),
        "{:?}",
        husk.disposition
    );
    let crate::rundir::PrivateHalfOwnership::Proven(token) =
        prove_private_half_ownership(&fixture.public(), &fixture.repo_key, &fixture.private_root)
    else {
        panic!("the bound husk must prove");
    };
    remove_private_husk(token, hooks.rundir()).expect("the token authorises this deletion");
    remove_public_husk(&fixture.public(), hooks.rundir()).expect("then the public half");
    assert!(!fixture.private().exists());
    assert!(!fixture.public().exists());
}

/// A `GitView` that materialises nothing and discards anything.
///
/// The census only ever discards, and the R19 directory the child left is
/// not what this test is about.
#[derive(Debug)]
struct DiscardOnly;

impl crate::runner::container::GitView for DiscardOnly {
    fn materialize(
        &self,
        request: &crate::runner::container::GitViewRequest,
    ) -> Result<PathBuf, UpstrokeError> {
        Ok(request.path.clone())
    }

    fn discard(&self, _path: &Path) -> Result<(), UpstrokeError> {
        Ok(())
    }
}

fn host_digest() -> String {
    crate::runner::policy::runner_policy_sha256(&crate::runner::policy::host_policy())
}

fn container_policy() -> RunnerPolicy {
    RunnerPolicy {
        kind: crate::topology::events::RunnerKind::Container,
        policy: crate::topology::events::RunnerContract::ContainerV1,
        image: Some(crate::topology::events::ImageIdentity {
            reference: "ghcr.io/upstroke/sandbox:1".to_owned(),
            id: "sha256:".to_owned() + &"a".repeat(64),
            digest: None,
        }),
        credential_volumes: Some(
            [(AGENT.to_owned(), "upstroke-codex".to_owned())]
                .into_iter()
                .collect(),
        ),
    }
}

// ===========================================================================
// The production `Probes`
// ===========================================================================

/// A `Runner` that records the request and answers exit 0.
#[derive(Debug, Default)]
struct RecordingRunner {
    requests: Mutex<Vec<crate::runner::RunnerRequest>>,
}

impl RecordingRunner {
    fn requests(&self) -> Vec<crate::runner::RunnerRequest> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl crate::runner::Runner for RecordingRunner {
    fn run(
        &self,
        request: &crate::runner::RunnerRequest,
    ) -> Result<crate::agent::proc::ProcessOutput, UpstrokeError> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.clone());
        Ok(crate::agent::proc::ProcessOutput {
            code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            duration: std::time::Duration::from_millis(1),
            timed_out: false,
            output_limited: false,
        })
    }
}

/// One adapter, whose `probe` is the only method this seam reaches.
#[derive(Debug, Default)]
struct OneAdapter {
    probed: Mutex<u32>,
}

impl crate::agent::AgentAdapter for OneAdapter {
    fn id(&self) -> &'static str {
        AGENT
    }

    fn probe(
        &self,
        runner: &dyn crate::runner::Runner,
    ) -> Result<crate::agent::Caps, UpstrokeError> {
        *self.probed.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        let request = crate::agent::probe_request(
            AGENT,
            crate::runner::CommandSpec::new(AGENT),
            0,
            std::time::Duration::from_secs(5),
        )?;
        runner.run(&request)?;
        Ok(crate::agent::Caps {
            version: "1.0".to_owned(),
            json_output: true,
            session_resume: false,
            cost_reporting: false,
            read_only_mode: false,
            acp: false,
            model_list: false,
        })
    }

    fn build(
        &self,
        _run: &crate::agent::TaskRun,
    ) -> Result<crate::runner::CommandSpec, UpstrokeError> {
        Ok(crate::runner::CommandSpec::new(AGENT))
    }

    fn parse(
        &self,
        _out: &crate::agent::proc::ProcessOutput,
    ) -> Result<crate::ir::Outcome, UpstrokeError> {
        Err(UpstrokeError::Agent {
            message: "this adapter exists to be probed".to_owned(),
        })
    }
}

#[derive(Debug, Default)]
struct OneSource {
    adapter: OneAdapter,
}

impl crate::agent::AdapterSource for OneSource {
    fn get(&self, id: &str) -> Option<&dyn crate::agent::AgentAdapter> {
        (id == AGENT).then_some(&self.adapter as &dyn crate::agent::AgentAdapter)
    }
}

/// The production [`Probes`]: both probes go through the run's own `Runner`,
/// and the shell probe carries the identity this module minted.
///
/// The seam has a test double everywhere else in this file, which is what makes
/// the *ordering* observable; this is the other half — that the implementation
/// production passes is the two existing functions and not a third one.
#[test]
fn the_production_probes_run_both_halves_through_the_runs_runner() {
    let runner = RecordingRunner::default();
    let source = OneSource::default();
    let probes = RunnerProbes {
        runner: &runner,
        shell: crate::gates::ShellKind::Sh,
        workspace: std::env::temp_dir(),
        adapters: &source,
        policy_digest: host_digest(),
    };
    assert_eq!(probes.policy_digest(), host_digest());

    let shell_id = PreflightIdentities::shell(0).expect("the shell probe identity");
    probes
        .shell(shell_id.clone())
        .expect("the recorded shell runs `exit 0`");
    let requests = runner.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].invocation, shell_id, "the identity is carried");
    assert_eq!(
        requests[0].role,
        crate::runner::ExecutionRole::Probe(crate::runner::ProbeTarget::Shell),
        "the shell probe is non-slotted, and the role says so"
    );
    assert!(requests[0].agent.is_none(), "it certifies no CLI");
    assert_eq!(
        requests[0].command.program,
        crate::gates::ShellKind::Sh.program(),
        "the recorded shell, not some other one"
    );

    probes.agent(AGENT).expect("the adapter is probed");
    assert_eq!(
        *source
            .adapter
            .probed
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
        1,
        "the agent probe did not reach its adapter"
    );
    let requests = runner.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(
        requests[1].role,
        crate::runner::ExecutionRole::Probe(crate::runner::ProbeTarget::Agent(
            crate::runner::AgentId::new(AGENT)
        )),
        "an agent probe is slotted, and the role says so"
    );

    // An agent this machine has no adapter for refuses rather than being
    // silently skipped: a run whose pre-flight certified nothing would still
    // record it in `probed_agents`.
    let refusal = probes
        .agent("no-such-agent")
        .expect_err("an unregistered agent refuses");
    assert!(
        refusal.to_string().contains("no-such-agent"),
        "the refusal does not name the agent: {refusal}"
    );
    assert_eq!(
        runner.requests().len(),
        2,
        "a process was started for an agent with no adapter"
    );
}

/// The vocabulary the report and the fault registry share.
///
/// `Prefix::ALL` is the closed set a suite is measured against, and
/// `is_past_the_deletion_boundary` is the **ordering** claim beside the stat —
/// never in place of it, because a `PublishCommitRecord` error is the same value
/// on both sides of the rename.
#[test]
fn the_deletion_boundary_falls_between_p5_and_p5b() {
    assert_eq!(
        Prefix::ALL.len(),
        14,
        "a prefix was added to the sequence and not to its closed set"
    );
    let mut names: Vec<&str> = Prefix::ALL.iter().map(|prefix| prefix.name()).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), before, "two prefixes share a name: {names:?}");

    for prefix in Prefix::ALL {
        assert_eq!(
            prefix.is_past_the_deletion_boundary(),
            matches!(prefix, Prefix::P5b | Prefix::P6 | Prefix::P7 | Prefix::P8),
            "`{}` is on the wrong side of the deletion boundary",
            prefix.name()
        );
    }
    assert!(!Prefix::P5.is_past_the_deletion_boundary());
    assert!(Prefix::P5b.is_past_the_deletion_boundary());

    // And the two questions a report answers about what is on disk.
    let removed = Disposition::BothHalvesRemoved {
        private: PathBuf::from("/private/runs/x"),
    };
    assert!(removed.removed_anything() && removed.removed_the_private_half());
    let public_only = Disposition::PublicHalfRemoved(UnboundShape::Bare);
    assert!(public_only.removed_anything() && !public_only.removed_the_private_half());
    for kept in [
        Disposition::NothingCreated,
        Disposition::Retained {
            reason: RetainReason::OwnerRecordMissing,
            locator: PathBuf::from("/private/runs/x"),
        },
        Disposition::PossiblyCommitted {
            locator: PathBuf::from("/private/runs/x"),
            undecidable: None,
        },
        Disposition::Committed,
        Disposition::RetainedPossiblyCommittedHusk {
            locator: PathBuf::from("/private/runs/x"),
        },
        Disposition::Undetermined {
            step: Some(BarrierStep::SyncPrefix),
            detail: "the prefix could not be synced".to_owned(),
        },
    ] {
        assert!(
            !kept.removed_anything() && !kept.removed_the_private_half(),
            "{kept:?} claims to have removed something"
        );
        assert!(!kept.describe().is_empty());
    }
}
