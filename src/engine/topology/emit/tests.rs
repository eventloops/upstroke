//! The emit path and the append-error protocol.
//!
//! Everything here drives the **shared** [`HookHarness`] through
//! [`HarnessTopologyHooks`], never a private observer, because half of what
//! these tests assert is an *absence* — "no promotion, CAS, cleanup, admission,
//! report" — and an absence measured on an observer only one family reports to
//! is an absence of evidence rather than evidence of absence. Every such
//! assertion here is paired with a **control** that drives the very site it
//! claims did not run, through the same harness, so a vacuous scan fails.
//!
//! Three constraints shaped the file and are worth stating once:
//!
//! * `src/engine/topology/**` is a `TOPOLOGY_MODULE`. Raw `std::fs` mutation
//!   and `std::process::Command` are on the effect denylist **in tests too**,
//!   so every byte on disk here arrives through a funnel: the run directories
//!   through [`RunPaths::create`], the log through `Event.OpenLog`, the lines
//!   through `Event.Append*`, the commit record through
//!   `RunDir.{Stage,Publish}CommitRecord`. Which means the fault shapes are the
//!   **error-return** halves of `T-APPEND` — (e-w), (e-u), (e-s) — and the kill
//!   halves (w), (u), (s) are `src/events/log/tests.rs`'s, beside the kill
//!   apparatus that already exists there.
//! * A function may not be its own oracle. "Whether the proven prefix contains
//!   the line" is checked here against bytes read back off disk with
//!   [`std::fs::read`], never against [`AppendOutcome`].
//! * Cumulative ledgers hide orderings (`PR5-WORKSPACE-022`). Every ordering
//!   assertion below clears its ledger first and reads the sequence that
//!   follows.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::engine::topology::identity::{AttemptIdentities, ReservationKind};
use crate::engine::topology::seams::HarnessTopologyHooks;
use crate::events::log::{
    HarnessEventHooks, INJECTED_PREFIX, StablePrefix, SyncTarget, WrittenShape,
    establish_stable_prefix,
};
use crate::events::{BindingSummary, BudgetKind, ChainSummary, GateSummary, PoolExhausted};
use crate::gates::ShellKind;
use crate::ir::{
    Artifact, ArtifactId, Effort, Plan, PlanSource, ResolvedEffortPolicy, Task, TaskId, TaskKind,
    Tier,
};
use crate::review::ReviewPlan;
use crate::rundir::{
    CommitRecord, CommitRecordPresence, RunDirClass, RunPaths, classify_run_dir,
    commit_record_after_error, publish_commit_record, stage_commit_record,
};
use crate::topology::effects::{
    EffectSiteId, EventSite, HookHarness, HookPhase, InjectionMode, RunDirSite, SubEffectPoint,
};
use crate::topology::events::{
    AttemptNumber, BudgetExceeded4, CommitSha, Epoch, GenerationId, GitRef, IncarnationId,
    RunStarted4, RunnerContract, RunnerKind, RunnerPolicy, TopologyLimits,
};
use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion};
use crate::topology::registry::{TaskKey, TaskRegistry};
use crate::topology::schema::TOPOLOGY_SCHEMA;
use crate::util::{DurableRecord, DurableStep};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const RUN_ID: &str = "01KZTPR7D00000000000000001";
const INCARNATION: &str = "01KZTPR7D0000000000000000A";
const NORMALIZED_DIGEST: &str =
    "sha256:9999999999999999999999999999999999999999999999999999999999999999";
const AAY: TaskKey = TaskKey(0);

/// The gate timeout the fixture builds, and the one a replay of it yields.
/// Written out rather than computed from the codec, which would make the codec
/// its own oracle.
const LOSSY_CONSTRUCTED: Duration = Duration::from_micros(60_000_123);
const LOSSY_AS_READ_BACK: Duration = Duration::from_millis(60_000);

/// The gate timeout a run record — live or replayed — actually holds.
#[track_caller]
fn gate_timeout(started: &RunStarted4) -> Duration {
    started
        .gate_cmds
        .first()
        .expect("the fixture records one gate")
        .timeout
}

static SCRATCH: AtomicU32 = AtomicU32::new(0);

/// A run directory tree of this test's own, created through the `RunDir`
/// funnels rather than with `create_dir_all`, which this module may not name.
///
/// Numbered as well as named: several tests below want two independent runs.
fn run_paths(tag: &str) -> RunPaths {
    let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("upstroke-emit-{tag}-{}-{n}", std::process::id()));
    let paths = RunPaths::with_private_root(&root, RUN_ID, &root);
    paths.create().expect("the run directories");
    paths
}

/// A clock that does not move, so a committed line is a literal.
#[derive(Debug, Clone, Copy)]
struct FixedClock(&'static str);

impl TimeSource for FixedClock {
    fn now_rfc3339(&self) -> String {
        self.0.to_owned()
    }
}

/// One task, one artifact. Small on purpose: the registry derivation is not
/// what is under test here, and a fixture that fails to authenticate fails
/// every test in the file with the same message.
fn plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "emit-frozen-hash".to_owned(),
        },
        tasks: vec![Task {
            id: TaskId::from("aay"),
            kind: TaskKind::Test,
            title: "the only task".to_owned(),
            body: "a body long enough to be a body".to_owned(),
            depends_on: Vec::new(),
            acceptance: vec!["it passes".to_owned()],
            path_hints: vec!["src/aay/".to_owned()],
            suggested_tier: Some(Tier::Small),
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: vec![ArtifactId::from("aay-out")],
        }],
        artifacts: vec![Artifact {
            id: ArtifactId::from("aay-out"),
            produced_by: Some(TaskId::from("aay")),
        }],
    }
}

fn inputs() -> FrozenInputs {
    FrozenInputs {
        plan: plan(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
    }
}

/// The run record with the digest field nothing has filled in yet, so the
/// digest can be derived from it without being derived from itself.
fn run_started_unauthenticated() -> RunStarted4 {
    let plan = plan();
    RunStarted4 {
        schema: TOPOLOGY_SCHEMA,
        upstroke_version: "0.2.0".to_owned(),
        run_id: RUN_ID.to_owned(),
        incarnation: IncarnationId(INCARNATION.to_owned()),
        runner: RunnerPolicy {
            kind: RunnerKind::Host,
            policy: RunnerContract::HostV1,
            image: None,
            credential_volumes: None,
        },
        probed_agents: vec!["claude-code".to_owned(), "copilot".to_owned()],
        branch: format!("upstroke/run-{RUN_ID}"),
        integration_ref: GitRef::from("refs/upstroke/integration"),
        base_sha: CommitSha::from("0f5c1c4"),
        execution_root: "/var/lib/upstroke/roots".to_owned(),
        private_dir: "/var/lib/upstroke/private".to_owned(),
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
            max_parallel: 3,
            max_defers: 2,
            max_merge_repairs: 1,
        },
        gates: vec!["fmt".to_owned()],
        gates_from_config: true,
        gate_cmds: vec![GateSummary {
            name: "fmt".to_owned(),
            cmd: "cargo fmt --check".to_owned(),
            // **Deliberately sub-millisecond.** `GateSummary.timeout` is the
            // one field reachable from a `run_started` whose wire codec is
            // lossy (`duration_ms`), and without a lossy value in the fixture
            // "`emit` uses the round-tripped event" is not a testable claim at
            // all: `Ok(written) -> Ok(event)` is invisible to any comparison
            // whose inputs round-trip unchanged, which is how
            // `PR5-CORRECTNESS-015` survived a whole suite. It is not a digest
            // input (`TaskRegistry::originals_with_agents` never reads
            // `gate_cmds`), so the record still authenticates.
            timeout: LOSSY_CONSTRUCTED,
            shell: ShellKind::Sh,
        }],
        interaction_mode: "never".to_owned(),
        chains: vec![ChainSummary {
            task: "aay".to_owned(),
            tiers: vec![Tier::Small, Tier::Mid],
            attempts_per: 2,
            bindings: Some(vec![
                BindingSummary {
                    tier: Tier::Small,
                    agent: "claude-code".to_owned(),
                    model: "claude-small".to_owned(),
                    pinned: false,
                },
                BindingSummary {
                    tier: Tier::Mid,
                    agent: "copilot".to_owned(),
                    model: "copilot-mid".to_owned(),
                    pinned: true,
                },
            ]),
        }],
        effort_policy: ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::Medium,
            frontier: Effort::High,
            review: Effort::XHigh,
        },
        reviews: ReviewPlan {
            enabled: Some(false),
            alternative_available: Some(false),
            pass_timeout_secs: Some(1_337),
            primary: None,
            alternative: None,
            second_opinion: vec![None],
        },
    }
}

fn run_started() -> RunStarted4 {
    let unauthenticated = run_started_unauthenticated();
    let digest = TaskRegistry::originals_with_agents(
        &plan(),
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

/// `Event.AppendFirst`: the run's commitment boundary.
fn run_started_body() -> TopologyEventBody {
    TopologyEventBody::RunStarted {
        data: Box::new(run_started()),
    }
}

/// `Event.Append`, and the one whose application the fold *shows*:
/// [`TopologyFold::budget_stop`] is `None` before it and `Some` after, so
/// "the delta was applied" and "the delta was not applied" are two different
/// observable states rather than two descriptions of one.
fn budget_body() -> TopologyEventBody {
    TopologyEventBody::BudgetExceeded {
        data: BudgetExceeded4 {
            epoch: Epoch(0),
            budget: BudgetKind::Run,
            limit_usd: 12.5,
            spent_usd: 13.0,
            key: Some(AAY),
        },
    }
}

/// `Event.AppendInformational`: one of the three kinds the frozen lenient class
/// names.
fn pool_body() -> TopologyEventBody {
    TopologyEventBody::PoolExhausted {
        data: PoolExhausted {
            pool: "claude-code".to_owned(),
            agent: "claude-code".to_owned(),
            reset_at: Some("2026-08-23T10:00:00Z".to_owned()),
            detail: "usage limit reached".to_owned(),
        },
    }
}

/// The whole of what one emit borrows, plus the harness the five families
/// record into.
struct Fixture {
    paths: RunPaths,
    harness: Arc<Mutex<HookHarness>>,
    hooks: HarnessTopologyHooks,
    identity: RunIdentity,
    fold: TopologyFold,
    log: EventLog,
    reservations: Reservations,
    invocations: InvocationLedger,
    warnings: Vec<String>,
    clock: FixedClock,
}

impl Fixture {
    /// A fresh run: directories created, and the append handle taken through
    /// the barrier exactly as P5 takes it ("a fresh run's `Event.OpenLog` at P5
    /// creates an empty log, so the barrier is trivially established").
    fn new(tag: &str) -> Self {
        let paths = run_paths(tag);
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness)).recording_durability();
        let mut warnings = Vec::new();
        let prefix = establish_stable_prefix(
            &paths.events(),
            inputs(),
            None,
            &mut warnings,
            hooks.events(),
        )
        .expect("an empty log establishes the barrier trivially");
        let (log, bytes, _events, fold) = prefix.into_log_and_fold();
        assert!(bytes.is_empty(), "a fresh run has no prefix");
        Self {
            paths,
            harness,
            hooks,
            identity: RunIdentity {
                run_id: RUN_ID.to_owned(),
                inputs: inputs(),
                committed_first_line_sha256: None,
            },
            fold,
            log,
            reservations: Reservations::new(),
            invocations: InvocationLedger::new(),
            warnings,
            clock: FixedClock("2026-08-23T09:41:02Z"),
        }
    }

    /// A run whose `run_started` is durable — the state every later append
    /// happens in.
    fn started(tag: &str) -> Self {
        let mut fixture = Self::new(tag);
        fixture
            .emit(run_started_body())
            .expect("the fixture's run_started applies and appends");
        assert!(fixture.fold.started().is_some());
        fixture
    }

    fn emit(&mut self, body: TopologyEventBody) -> Result<TopologyEvent, EmitError> {
        let mut state = EmitState {
            fold: &mut self.fold,
            log: &mut self.log,
            reservations: &mut self.reservations,
            warnings: &mut self.warnings,
        };
        emit(
            &self.identity,
            &mut state,
            &self.clock,
            body,
            &mut self.hooks,
        )
    }

    fn arm(&self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) {
        self.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .arm(EffectSiteId::Event(site), point, mode)
            .expect("the frozen inventory declares this coordinate");
    }

    fn disarm(&self) {
        self.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .disarm();
    }

    fn log_path(&self) -> std::path::PathBuf {
        self.paths.events()
    }

    fn log_bytes(&self) -> Vec<u8> {
        read(&self.paths.events())
    }

    /// The Event funnel's own durability trace, for the log and nothing else.
    fn durability(&self) -> Vec<DurableRecord> {
        self.hooks
            .event_observer()
            .ledger()
            .records_for(&self.paths.events())
    }

    /// Forget the trace so far, so the next sequence reads on its own.
    fn clear_durability(&self) {
        self.hooks.event_observer().ledger().clear();
    }

    /// Every `(site, phase)` the five families reported, in first-observation
    /// order, together with what they merely reached.
    fn sites_touched(&self) -> Vec<EffectSiteId> {
        let harness = self
            .harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        harness
            .coverage()
            .iter()
            .chain(harness.reached())
            .map(|seen| seen.site)
            .collect()
    }

    /// Every site outside the Event group that any funnel ran.
    ///
    /// The append-error protocol's whole negative half is about these: "no
    /// promotion, CAS, cleanup, admission, report, question payload, or other
    /// fold-derived mutation". Derived from the harness rather than from a list
    /// of sites somebody thought of, so a group added later is caught.
    fn non_event_sites(&self) -> Vec<EffectSiteId> {
        let mut sites: Vec<EffectSiteId> = self
            .sites_touched()
            .into_iter()
            .filter(|site| !matches!(site, EffectSiteId::Event(_)))
            .collect();
        sites.dedup();
        sites
    }

    /// The control every absence assertion is paired with: drive one real
    /// fold-derived effect through the same bundle and prove the scan sees it.
    ///
    /// `RunDir.WriteReport` on purpose — the report from memory is the exact
    /// thing `Run::drain_and_report` does and the protocol forbids.
    fn drive_a_report(&mut self) {
        crate::rundir::write_report(&self.paths.public, &"a report", self.hooks.rundir())
            .expect("the control report is written");
    }
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("the log is readable")
}

/// A fold built the way a fresh process builds one: recovery step (a1), over
/// whatever prefix survived.
fn resume(paths: &RunPaths) -> StablePrefix {
    let mut warnings = Vec::new();
    establish_stable_prefix(
        &paths.events(),
        inputs(),
        None,
        &mut warnings,
        &mut crate::events::log::NoEventHooks,
    )
    .expect("the surviving prefix replays")
}

#[track_caller]
fn append_error(error: &EmitError) -> &UncancelledAppend {
    error
        .append_error()
        .unwrap_or_else(|| panic!("not an outcome-unknown append: {error}"))
}

// ---------------------------------------------------------------------------
// The barrier, before anything acts on what it proved
// ---------------------------------------------------------------------------

/// `stable_prefix_barrier` (2): "`Event.OpenLog.SyncPrefix` successfully syncs
/// the complete surviving prefix … so that every newline-terminated line the
/// process can see is durable, **incl. a line an earlier process wrote but
/// never synced**"; and (6): "only then may promotion, CAS, cleanup, admission,
/// reporting, or any other fold-derived mutation proceed".
///
/// Two claims, and the second is an absence, so it is paired with a control
/// below: the same scan is re-run after one real `RunDir` effect and must then
/// find it.
#[test]
fn open_syncs_surviving_prefix_before_any_recovery_effect() {
    // An earlier process: `run_started` synced, then a second line written in
    // full and never synced — T-APPEND (e-u), the shape the barrier exists to
    // make durable.
    let mut earlier = Fixture::started("open-syncs-prefix");
    earlier.arm(
        EventSite::Append,
        SubEffectPoint::WrittenFull,
        InjectionMode::ErrorReturn,
    );
    let error = earlier
        .emit(budget_body())
        .expect_err("the flush was made to fail");
    let unsynced = read(&earlier.log_path());
    assert!(
        unsynced.ends_with(b"\n"),
        "the whole line reached the file: the newline is the commit marker"
    );
    assert_eq!(
        append_error(&error).outcome,
        AppendOutcome::Present,
        "the line is on disk; only its durability is in doubt"
    );
    let paths = earlier.paths.clone();
    drop(earlier);

    // A fresh process. One harness, five families, nothing armed.
    let harness = Arc::new(Mutex::new(HookHarness::new()));
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness)).recording_durability();
    let mut warnings = Vec::new();
    let prefix = establish_stable_prefix(
        &paths.events(),
        inputs(),
        None,
        &mut warnings,
        hooks.events(),
    )
    .expect("the barrier holds over the surviving prefix");

    // (2). The synced length is the filesystem's own answer and it covers the
    // line the earlier handle never synced.
    let on_disk = u64::try_from(unsynced.len()).expect("a test log fits in u64");
    let syncs = hooks.event_observer().syncs();
    let prefix_sync = syncs
        .iter()
        .find(|record| {
            record.site == EventSite::OpenLog
                && record.point == SubEffectPoint::SyncPrefix
                && record.target == SyncTarget::LogFile
        })
        .expect("the open synced the log file at its SyncPrefix point");
    assert_eq!(
        prefix_sync.len, on_disk,
        "the sync ledger's length is the file length after open"
    );
    assert_eq!(prefix_sync.path, paths.events());
    assert!(
        hooks
            .event_observer()
            .ledger()
            .records_for(&paths.events())
            .iter()
            .any(|record| record.step == DurableStep::SyncedFile && record.len == on_disk),
        "and the durability ledger agrees with it"
    );

    // The prefix really did carry both lines through the checked replay.
    assert!(prefix.fold().started().is_some());
    assert!(
        prefix.fold().budget_stop().is_some(),
        "the unsynced line is part of the proven prefix"
    );

    // (6). At the moment the barrier returned, nothing outside the Event group
    // had run at all.
    let during: Vec<EffectSiteId> = {
        let seen = harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.coverage()
            .iter()
            .chain(seen.reached())
            .map(|seen| seen.site)
            .filter(|site| !matches!(site, EffectSiteId::Event(_)))
            .collect()
    };
    assert!(
        during.is_empty(),
        "a fold-derived effect ran before the barrier proved anything: {during:?}"
    );

    // The control. The same scan, after one real fold-derived effect, must find
    // it — otherwise the assertion above was measuring nothing.
    crate::rundir::write_report(&paths.public, &"a report", hooks.rundir())
        .expect("the control report is written");
    let after: Vec<EffectSiteId> = {
        let seen = harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.coverage()
            .iter()
            .chain(seen.reached())
            .map(|seen| seen.site)
            .filter(|site| !matches!(site, EffectSiteId::Event(_)))
            .collect()
    };
    assert!(
        after.contains(&EffectSiteId::RunDir(RunDirSite::WriteReport)),
        "the scan cannot see a fold-derived effect at all: {after:?}"
    );
}

/// `stable_prefix_barrier`: "a failed sync (`SyncPrefix` returning `Err`) …
/// performs none of those effects: the write command ends … no append handle is
/// used, the run is NoRunFinished and resumable, and the next resume
/// re-establishes the barrier from (a0)".
#[test]
fn open_sync_failure_refuses_resumably_with_no_fold_derived_effect() {
    let mut fixture = Fixture::started("open-sync-failure");
    fixture.emit(budget_body()).expect("a second durable line");
    let before = fixture.log_bytes();
    let paths = fixture.paths.clone();
    drop(fixture);

    let harness = Arc::new(Mutex::new(HookHarness::new()));
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness)).recording_durability();
    harness
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::OpenLog),
            SubEffectPoint::SyncPrefix,
            InjectionMode::ErrorReturn,
        )
        .expect("SyncPrefix declares an error-return mode");

    let mut warnings = Vec::new();
    let refusal = establish_stable_prefix(
        &paths.events(),
        inputs(),
        None,
        &mut warnings,
        hooks.events(),
    )
    .expect_err("a prefix that could not be synced authorizes nothing");

    // It names the step, and no handle came back — the `Result` is the whole of
    // "hands out nothing", because `StablePrefix` is the only thing that
    // carries one.
    assert_eq!(refusal.step, BarrierStep::SyncPrefix);
    assert!(
        refusal
            .to_string()
            .contains("No append handle was handed out"),
        "{refusal}"
    );

    // The sync never happened: the point is consulted *before* it, so an
    // injected failure stands in place of a successful sync rather than after
    // one.
    assert!(
        hooks.event_observer().syncs().is_empty(),
        "a refused SyncPrefix still reported a sync"
    );
    assert!(
        !hooks
            .event_observer()
            .ledger()
            .steps()
            .contains(&DurableStep::SyncedFile),
        "and still recorded one"
    );

    // Nothing was done. Nothing at all.
    assert_eq!(read(&paths.events()), before, "the refusal changed the log");
    let during: Vec<EffectSiteId> = {
        let seen = harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.coverage()
            .iter()
            .chain(seen.reached())
            .map(|seen| seen.site)
            .filter(|site| !matches!(site, EffectSiteId::Event(_)))
            .collect()
    };
    assert!(
        during.is_empty(),
        "an effect survived the refusal: {during:?}"
    );
    assert!(
        !paths.public.join("report.json").exists(),
        "a report was written from a barrier that did not hold"
    );

    // The arming really did fire — otherwise every assertion above is about a
    // barrier that simply succeeded and was never asked to fail.
    assert!(
        harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observed(
                EffectSiteId::Event(EventSite::OpenLog),
                HookPhase::Point {
                    point: SubEffectPoint::SyncPrefix,
                    mode: InjectionMode::ErrorReturn,
                }
            ),
        "the injection never reached the funnel"
    );

    // Resumable: the next open repeats the barrier and it holds.
    harness
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .disarm();
    let prefix = establish_stable_prefix(
        &paths.events(),
        inputs(),
        None,
        &mut warnings,
        hooks.events(),
    )
    .expect("the next resume establishes the barrier");
    assert_eq!(prefix.bytes(), &before[..]);
    assert!(prefix.fold().budget_stop().is_some());
}

// ---------------------------------------------------------------------------
// The first append: the run's commitment boundary
// ---------------------------------------------------------------------------

/// `T-APPEND`: "a torn or absent first line is a non-committed run (T-RUNSTART
/// P5, **or P5b if the commit record exists** — the creator deletes nothing
/// after P5b)"; and, for `Event.AppendFirst`, "the creator additionally never
/// deletes either half (the commit record already exists) and reports the run
/// as committed, as a retained possibly committed husk, or as undetermined and
/// retained".
///
/// Both halves of "per commit record", in one test, because the classification
/// is identical on both sides and only the record decides what may be done
/// about it. A test that drove one side would pass with the record never read.
#[test]
fn torn_first_line_is_husk_or_possibly_committed_per_commit_record() {
    // (a) No commit record. A run killed before P5b: the first line is torn,
    //     so the directory is a husk and the creator may remove both halves.
    let mut before_p5b = Fixture::new("torn-first-no-record");
    before_p5b.arm(
        EventSite::AppendFirst,
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
    );
    let error = before_p5b
        .emit(run_started_body())
        .expect_err("the partial write fails the append");
    let torn = append_error(&error);
    assert_eq!(torn.site, EventSite::AppendFirst);

    assert_eq!(
        classify_run_dir(&before_p5b.paths.public),
        RunDirClass::Husk,
        "a torn first line is not a committed first line"
    );
    assert_eq!(
        commit_record_after_error(&before_p5b.paths.private),
        CommitRecordPresence::Absent
    );
    assert!(
        commit_record_after_error(&before_p5b.paths.private).permits_deletion(),
        "before P5b the creator, which holds both locks, may remove both halves"
    );

    // (b) The commit record exists — which at `Event.AppendFirst` it always
    //     does, because P5b precedes P6. Same torn first line; a different
    //     answer about what may be done with it.
    let mut after_p5b = Fixture::new("torn-first-with-record");
    let record = CommitRecord {
        run_id: RUN_ID.to_owned(),
        repo_key: "0123456789abcdef".to_owned(),
        public_dir: after_p5b.paths.public.display().to_string(),
        incarnation: INCARNATION.to_owned(),
        run_started_sha256: "sha256:".to_owned() + &"a".repeat(64),
    };
    stage_commit_record(&after_p5b.paths.private, &record, after_p5b.hooks.rundir())
        .expect("P5b stages");
    publish_commit_record(&after_p5b.paths.private, after_p5b.hooks.rundir())
        .expect("P5b publishes");

    after_p5b.arm(
        EventSite::AppendFirst,
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
    );
    let error = after_p5b
        .emit(run_started_body())
        .expect_err("the partial write fails the append");
    let torn = append_error(&error);

    assert_eq!(
        classify_run_dir(&after_p5b.paths.public),
        RunDirClass::Husk,
        "the classification does not read the commit record"
    );
    assert_eq!(
        commit_record_after_error(&after_p5b.paths.private),
        CommitRecordPresence::Present
    );
    assert!(
        !commit_record_after_error(&after_p5b.paths.private).permits_deletion(),
        "after P5b nothing deletes either half"
    );

    // The protocol's own report for that shape.
    assert_eq!(
        torn.outcome,
        AppendOutcome::Absent,
        "the torn tail was truncated"
    );
    assert_eq!(
        torn.creator_disposition(),
        Some(FirstAppendDisposition::RetainedPossiblyCommitted)
    );
    assert!(
        torn.to_string().contains("neither half is deleted"),
        "{torn}"
    );

    // And it kept its word: both halves survived the protocol.
    assert!(
        after_p5b.paths.public.is_dir(),
        "the public half was removed"
    );
    assert!(
        after_p5b.paths.private.join("committed.json").is_file(),
        "the private half was removed"
    );

    // The disposition is scoped to the first append and to nothing else. The
    // control that says so: the same protocol at `Event.Append` has none.
    let mut later = Fixture::started("torn-first-control");
    later.arm(
        EventSite::Append,
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
    );
    let error = later
        .emit(budget_body())
        .expect_err("the partial write fails");
    assert_eq!(append_error(&error).site, EventSite::Append);
    assert_eq!(
        append_error(&error).creator_disposition(),
        None,
        "a later append's outcome says nothing about the run's commitment"
    );
}

// ---------------------------------------------------------------------------
// The append-error protocol, one fault shape at a time
// ---------------------------------------------------------------------------

/// T-APPEND (e-w): "`write_all` failed after a partial write". The durable
/// state is the previous prefix, and `authoritative_state` is explicit that
/// **the live process does not know that** — "in the error-return cases the
/// live process does not know which prefix survived until it reopens, syncs,
/// rereads, and replays".
#[test]
fn append_error_after_partial_write_is_outcome_unknown_until_reopen_replay() {
    let mut fixture = Fixture::started("partial-write");
    let before = fixture.log_bytes();
    fixture.clear_durability();

    fixture.arm(
        EventSite::Append,
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
    );
    let error = fixture
        .emit(budget_body())
        .expect_err("a partial write fails the append");
    let failed = append_error(&error);

    // What the *process* had, before the reopen: one `write_all` of fewer bytes
    // than the line, and nothing after it. That is the whole of what it knew,
    // and it is not enough to decide anything.
    let trace = fixture.durability();
    let wrote: Vec<&DurableRecord> = trace
        .iter()
        .filter(|record| record.step == DurableStep::Wrote)
        .collect();
    assert_eq!(
        wrote.len(),
        1,
        "one primitive attempt, not a retry: {trace:?}"
    );
    assert!(
        wrote[0].len > 0,
        "an (e-w) fault performs the partial write itself"
    );
    assert!(
        !trace
            .iter()
            .any(|record| matches!(record.step, DurableStep::Flushed | DurableStep::SyncedData)),
        "the failure was at Written; nothing after it ran: {trace:?}"
    );
    assert!(
        failed.cause.to_string().contains(INJECTED_PREFIX),
        "the funnel's own error is what says where the outcome became unknown: {}",
        failed.cause
    );
    assert!(
        failed
            .cause
            .to_string()
            .contains(SubEffectPoint::Written.name()),
        "and it names the point: {}",
        failed.cause
    );

    // And what the reopen, sync, reread and replay then established.
    assert_eq!(failed.outcome, AppendOutcome::Absent);
    assert_eq!(
        read(&fixture.log_path()),
        before,
        "the reopen truncated the torn tail back to the previous prefix"
    );
    assert!(
        fixture
            .warnings
            .iter()
            .any(|warning| warning.contains("never finished being written")),
        "the truncation is reported: {:?}",
        fixture.warnings
    );
    assert!(
        failed
            .to_string()
            .contains("the previous prefix stands and is durable"),
        "{failed}"
    );
    assert!(failed.resumable());
}

/// T-APPEND (e-u): "`write_all` succeeded (full line, newline present) and
/// flush or `sync_data` returned an error". The line is on disk, so the reopen
/// replay shows it — and **the append is never retried**, which is a claim
/// about the bytes and about the primitive count, so both are asserted.
#[test]
fn append_flush_error_after_full_line_reopen_replay_shows_line_and_no_retry() {
    let mut fixture = Fixture::started("flush-error");
    let before = fixture.log_bytes();
    fixture.clear_durability();

    fixture.arm(
        EventSite::Append,
        SubEffectPoint::WrittenFull,
        InjectionMode::ErrorReturn,
    );
    let error = fixture
        .emit(budget_body())
        .expect_err("the flush was made to fail");
    let failed = append_error(&error);

    assert_eq!(failed.outcome, AppendOutcome::Present);
    assert!(
        failed.to_string().contains("committed and durable"),
        "{failed}"
    );

    // The bytes: exactly one more line than before, ending at a commit marker.
    let after = read(&fixture.log_path());
    assert!(after.starts_with(&before), "the prefix was rewritten");
    let appended = &after[before.len()..];
    assert!(appended.ends_with(b"\n"), "the full line, newline included");
    assert_eq!(
        after
            .windows(appended.len())
            .filter(|w| *w == appended)
            .count(),
        1,
        "the line is in the log twice: the append was retried"
    );

    // The primitives: one `write_all` of the whole line, one `flush` attempt,
    // and nothing else. A retry is two of something here even when the file
    // happens to look the same.
    let trace = fixture.durability();
    assert_eq!(
        trace
            .iter()
            .filter(|r| r.step == DurableStep::Wrote)
            .count(),
        1,
        "{trace:?}"
    );
    assert_eq!(
        trace
            .iter()
            .filter(|r| r.step == DurableStep::Flushed)
            .count(),
        0,
        "the (e-u) fault stands in place of the flush: {trace:?}"
    );
    assert_eq!(
        u64::try_from(appended.len()).expect("a test line fits in u64"),
        trace
            .iter()
            .find(|r| r.step == DurableStep::Wrote)
            .expect("one write")
            .len,
        "one `write_all` containing both the JSON and its LF commit marker"
    );

    // The reopen replay is where "shows the line" is established, and it is the
    // protocol's own reopen: a fresh resume agrees with it.
    assert!(resume(&fixture.paths).fold().budget_stop().is_some());
}

/// T-APPEND (e-s): "`sync_data` returned an error **after the data reached the
/// disk**". Indistinguishable from (e-u) to the process, and the interesting
/// half is the one the name states: the command ends **without a stale fold
/// mutation** — the live fold never applied the delta, while the durable log
/// holds the line.
#[test]
fn append_sync_error_reopen_replay_and_end_without_stale_fold_mutation() {
    let mut fixture = Fixture::started("sync-error");
    assert_eq!(
        fixture.fold.budget_stop(),
        None,
        "the state this test is about starts empty"
    );
    fixture.clear_durability();

    fixture.arm(
        EventSite::Append,
        SubEffectPoint::Synced,
        InjectionMode::ErrorReturn,
    );
    let error = fixture
        .emit(budget_body())
        .expect_err("the sync was made to fail");
    let failed = append_error(&error);
    assert_eq!(failed.outcome, AppendOutcome::Present);

    // The data reached the disk before the error: write, flush, sync, in order.
    assert_eq!(
        fixture
            .durability()
            .iter()
            .map(|record| record.step)
            .collect::<Vec<_>>(),
        vec![
            DurableStep::Wrote,
            DurableStep::Flushed,
            DurableStep::SyncedData,
            // The protocol's reopen syncs the surviving prefix.
            DurableStep::SyncedFile,
        ],
        "the (e-s) coordinate is after the sync, not instead of it"
    );

    // No stale fold mutation. The live fold did not apply the delta and never
    // will; the durable prefix did get the line. The two disagree, and that
    // disagreement is the property — a fold that had applied it would agree
    // here and would be indistinguishable from a correct one.
    assert!(fixture.fold.is_poisoned());
    assert_eq!(
        fixture.fold.budget_stop(),
        None,
        "the delta was applied to a fold this process cannot vouch for"
    );
    assert!(
        resume(&fixture.paths).fold().budget_stop().is_some(),
        "and the durable log does hold the line"
    );

    // The command ends. It does not continue even though the prefix shows the
    // line present.
    assert!(!error.wrote_nothing());
    assert!(failed.resumable());
}

/// `append_error_protocol`: "when the barrier's sync fails, the reread is
/// unstable, or the replay refuses, it ends the command **without asserting
/// either** (outcome undetermined …) and still performs no retry, report from
/// memory, cleanup, or fold mutation".
#[test]
fn append_error_with_failed_prefix_sync_reports_undetermined_without_effects() {
    let mut fixture = Fixture::started("undetermined");
    let before = fixture.log_bytes();

    // Both faults at once: the append fails, and so does the barrier the
    // protocol then tries to establish.
    fixture.arm(
        EventSite::Append,
        SubEffectPoint::WrittenFull,
        InjectionMode::ErrorReturn,
    );
    fixture.arm(
        EventSite::OpenLog,
        SubEffectPoint::SyncPrefix,
        InjectionMode::ErrorReturn,
    );
    let error = fixture
        .emit(budget_body())
        .expect_err("the flush was made to fail");
    let failed = append_error(&error);

    match &failed.outcome {
        AppendOutcome::Undetermined { step, detail } => {
            assert_eq!(*step, BarrierStep::SyncPrefix);
            assert!(!detail.is_empty(), "the refusal says what it found");
        }
        other => panic!("a barrier that did not hold asserted an outcome: {other:?}"),
    }
    // Asserting *neither* is the point, so both words are checked absent.
    let rendered = failed.to_string();
    assert!(rendered.contains("undetermined"), "{rendered}");
    assert!(
        !rendered.contains("committed and durable"),
        "it asserted presence: {rendered}"
    );
    assert!(
        !rendered.contains("the previous prefix stands"),
        "it asserted absence: {rendered}"
    );

    // No retry: exactly one line was appended and nothing touched it after.
    let after = read(&fixture.log_path());
    assert!(after.starts_with(&before));
    assert_eq!(
        after.len() - before.len(),
        after[before.len()..].len(),
        "arithmetic sanity for the slice below"
    );
    assert!(after[before.len()..].ends_with(b"\n"));
    assert_eq!(
        after
            .windows(after.len() - before.len())
            .filter(|w| *w == &after[before.len()..])
            .count(),
        1,
        "the append was retried"
    );

    // No effects, and the fold is poisoned rather than mutated.
    assert!(fixture.fold.is_poisoned());
    assert_eq!(fixture.fold.budget_stop(), None);
    assert!(
        fixture.non_event_sites().is_empty(),
        "a fold-derived effect ran: {:?}",
        fixture.non_event_sites()
    );
    assert!(!fixture.paths.public.join("report.json").exists());
    assert!(failed.resumable());

    // The control: the scan can see a fold-derived effect when there is one.
    fixture.drive_a_report();
    assert!(
        fixture
            .non_event_sites()
            .contains(&EffectSiteId::RunDir(RunDirSite::WriteReport)),
        "the absence above was not measurable"
    );
}

/// `append_error_protocol`: "no report, status, question payload, or cleanup is
/// derived from the poisoned fold" — and the reason this is a test of its own
/// is that the legacy engine does exactly that. `Run::drain_and_report` catches
/// the propagated `Err`, calls `self.finish()` on in-memory state, writes the
/// report, and re-returns. Copying that shape here would leave every other test
/// in this file passing.
#[test]
fn append_error_never_triggers_cleanup_or_report_from_memory() {
    let mut fixture = Fixture::started("no-report-from-memory");

    // A reservation held and an invocation running, so the protocol has
    // something to settle and "cancelled" is distinguishable from "there was
    // nothing".
    fixture
        .reservations
        .take(AAY, ReservationKind::Dispatch)
        .expect("the ledger is empty at process start");
    let worker = AttemptIdentities::new(AAY, GenerationId(0), AttemptNumber(1)).worker();
    fixture
        .invocations
        .register(&worker)
        .expect("a fresh identity registers");

    fixture.arm(
        EventSite::Append,
        SubEffectPoint::Synced,
        InjectionMode::ErrorReturn,
    );
    let error = fixture
        .emit(budget_body())
        .expect_err("the sync was made to fail");
    let failed = append_error(&error);

    // Nothing outside the Event group ran at all: no cleanup site, no report
    // site, no ref, no container, no spawn. Derived from the harness, so a
    // group added later is covered without editing a list.
    let ran = fixture.non_event_sites();
    assert!(ran.is_empty(), "the protocol performed an effect: {ran:?}");
    assert!(
        !fixture.paths.public.join("report.json").exists(),
        "a report was written from the poisoned fold"
    );
    assert!(
        fixture.paths.public.is_dir() && fixture.paths.private.is_dir(),
        "the protocol removed a run directory"
    );

    // What it *did* do is settle the two process-local ledgers, which is not a
    // report: neither is derived from the fold, and both are obligations of the
    // protocol in their own right.
    assert!(failed.cancelled_reservation);

    // **Obligation (3) is the caller's, and this is the caller.** The report
    // does not exist until the ledger is handed over: `AppendError` has a
    // private witness field and `UncancelledAppend::cancelling` is its only
    // constructor, so a caller that skipped this could not have reached a count
    // to assert. That is the compile-time half; this is the behavioural one.
    let report = match error {
        EmitError::AppendFailed(append) => append.cancelling(&mut fixture.invocations),
        other => panic!("the entered append did not report as one: {other}"),
    };
    assert_eq!(report.cancelled_invocations, 1);
    assert!(
        fixture.reservations.is_empty() && fixture.reservations.balances(),
        "the provisional reservation was left held"
    );
    assert!(
        fixture.invocations.balances() && fixture.invocations.running().is_empty(),
        "an invocation was left running"
    );

    // The control. A `RunDir.WriteReport` driven through the same bundle *is*
    // seen, so the emptiness above measures something.
    fixture.drive_a_report();
    let after = fixture.non_event_sites();
    assert!(
        after.contains(&EffectSiteId::RunDir(RunDirSite::WriteReport)),
        "the scan is blind to the very site the protocol must not reach: {after:?}"
    );
    assert!(fixture.paths.public.join("report.json").exists());
}

/// `refusal_condition`: "any transition, retry, report, or cleanup attempted
/// from a poisoned fold" refuses. INV-20 says the same from the other side: "no
/// completion is applied after the fold is poisoned by an append error".
///
/// The claim is over *every later transition*, so this drives one event of each
/// of the three append sites and asserts all three refuse with the same
/// refusal — and, because "they all refuse" is trivially true of a fixture
/// whose events were never applicable, a control fixture emits the same two
/// non-first events successfully.
#[test]
fn poisoned_fold_refuses_every_later_transition() {
    let mut fixture = Fixture::started("poisoned-fold");
    fixture.arm(
        EventSite::Append,
        SubEffectPoint::Synced,
        InjectionMode::ErrorReturn,
    );
    fixture
        .emit(budget_body())
        .expect_err("the sync was made to fail");
    assert!(fixture.fold.is_poisoned());
    fixture.disarm();

    let sealed = fixture.log_bytes();
    for body in [run_started_body(), budget_body(), pool_body()] {
        let kind = body.kind();
        let error = match fixture.emit(body) {
            Ok(_) => panic!("`{kind}` was folded into a poisoned state"),
            Err(error) => error,
        };
        match &error {
            EmitError::Refused(FoldError::Poisoned) => {}
            other => panic!("`{kind}` was refused for the wrong reason: {other}"),
        }
        assert!(
            error_wrote_nothing(&fixture, &sealed),
            "`{kind}` reached the log"
        );
    }

    // The fold's own answer, directly: a delta cannot even be planned.
    let event = TopologyEvent {
        ts: "2026-08-23T09:41:02Z".to_owned(),
        body: pool_body(),
    };
    assert_eq!(
        fixture
            .fold
            .plan_transition(&event)
            .expect_err("a poisoned fold refuses the transition"),
        FoldError::Poisoned
    );

    // The control: the same two later events apply and append in a run whose
    // fold was never poisoned. Without this, "everything refuses" is also true
    // of a fixture that never had a valid transition to offer.
    let mut healthy = Fixture::started("poisoned-fold-control");
    let before = healthy.log_bytes().len();
    healthy.emit(budget_body()).expect("a budget stop applies");
    healthy.emit(pool_body()).expect("an informational applies");
    assert!(healthy.fold.budget_stop().is_some());
    assert!(healthy.log_bytes().len() > before);
}

/// A helper the loop above reads better with: the log did not move.
fn error_wrote_nothing(fixture: &Fixture, sealed: &[u8]) -> bool {
    fixture.log_bytes() == sealed
}

/// `resume_action`: "the run is NoRunFinished and resumable and the next resume
/// follows the fault row of **the surviving prefix** after its own barrier".
///
/// Two shapes in one test, because "the surviving prefix" is the whole content
/// of the claim: an (e-w) failure leaves the before-append prefix and an (e-s)
/// failure leaves the after-append one, and a resume that followed this
/// process's poisoned fold would produce the *first* answer in both cases.
#[test]
fn resume_after_append_error_follows_surviving_prefix() {
    // (e-w): the torn tail is truncated, so the surviving prefix is the
    // previous one and the resume follows the before-append order.
    let mut torn = Fixture::started("resume-after-torn");
    let before = torn.log_bytes();
    torn.arm(
        EventSite::Append,
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
    );
    let error = torn.emit(budget_body()).expect_err("a partial write");
    assert_eq!(append_error(&error).outcome, AppendOutcome::Absent);

    let resumed = resume(&torn.paths);
    assert_eq!(resumed.bytes(), &before[..], "the surviving prefix");
    assert!(resumed.fold().started().is_some(), "the run is still a run");
    assert!(
        resumed.fold().budget_stop().is_none(),
        "the resume followed the after-append order for a line that is not there"
    );
    assert!(
        !resumed.fold().is_poisoned(),
        "a fresh process's fold is clean"
    );

    // (e-s): the line reached the disk, so the surviving prefix includes it and
    // the resume follows the after-append order — from the same in-memory
    // state, which said nothing either way.
    let mut synced = Fixture::started("resume-after-synced");
    let before = synced.log_bytes();
    synced.arm(
        EventSite::Append,
        SubEffectPoint::Synced,
        InjectionMode::ErrorReturn,
    );
    let error = synced.emit(budget_body()).expect_err("a failed sync");
    assert_eq!(append_error(&error).outcome, AppendOutcome::Present);

    let resumed = resume(&synced.paths);
    assert!(
        resumed.bytes().len() > before.len(),
        "the surviving prefix carries the line"
    );
    assert!(
        resumed.fold().budget_stop().is_some(),
        "the resume did not follow the surviving prefix"
    );

    // The two live folds are identical and the two resumed folds are not, which
    // is the shape of the claim: what the next process does is decided by the
    // prefix, never by what this one held.
    assert_eq!(torn.fold.budget_stop(), synced.fold.budget_stop());
    assert!(torn.fold.is_poisoned() && synced.fold.is_poisoned());
}

// ---------------------------------------------------------------------------
// The emit path's own order
// ---------------------------------------------------------------------------

/// `emit`: "a `FoldError` aborts **before any write**", and `apply_delta` runs
/// "only after the funnel returned `Ok`".
///
/// The two aborts and the one success, each read off the log's bytes rather
/// than off the returned value.
#[test]
fn a_refused_transition_is_never_appended_and_a_successful_one_is_applied_after_it() {
    let mut fixture = Fixture::started("emit-order");
    let sealed = fixture.log_bytes();

    // A second `run_started` is refused by the fold. Nothing is written.
    let error = fixture
        .emit(run_started_body())
        .expect_err("a run begins once");
    assert!(matches!(
        error,
        EmitError::Refused(FoldError::AlreadyStarted)
    ));
    assert!(error.wrote_nothing());
    assert_eq!(
        fixture.log_bytes(),
        sealed,
        "a refused transition was appended"
    );
    assert!(
        !fixture.fold.is_poisoned(),
        "a pre-append refusal poisons nothing"
    );

    // A `budget_exceeded` for an epoch this run is not in is refused by the
    // fold too — a different refusal, so "nothing is written" is not resting on
    // one code path.
    let error = fixture
        .emit(TopologyEventBody::BudgetExceeded {
            data: BudgetExceeded4 {
                epoch: Epoch(7),
                ..match budget_body() {
                    TopologyEventBody::BudgetExceeded { data } => data,
                    _ => unreachable!("budget_body is a budget_exceeded"),
                }
            },
        })
        .expect_err("a stop belongs to the epoch that hit the ceiling");
    assert!(matches!(error, EmitError::Refused(_)));
    assert_eq!(fixture.log_bytes(), sealed);

    // And the success: applied, and applied *after* the bytes were durable.
    let applied = fixture.emit(budget_body()).expect("this one applies");
    assert_eq!(applied.body.kind(), "budget_exceeded");
    assert!(fixture.fold.budget_stop().is_some());
    let after = fixture.log_bytes();
    assert!(after.len() > sealed.len());
    assert!(after.ends_with(b"\n"));
}

/// INV-02, first clause: "live state and replay use **one checked transition
/// over the exact wire event**".
///
/// Three things have to be one value for that to be true — what `emit` returns,
/// what the *live fold* holds, and what a replay of the log builds — and the
/// fixture's gate timeout is sub-millisecond precisely so that they *can*
/// differ. Each of the three is read from its own source: the return value,
/// `fold.started()`, and the committed bytes parsed back off disk.
///
/// This is `PR5-CORRECTNESS-015`'s shape. `Ok(written) -> Ok(event)`, and
/// `plan_transition(&checked) -> plan_transition(&event)`, are both invisible
/// to a fixture whose every field round-trips unchanged.
#[test]
fn emit_returns_the_event_a_replay_of_this_log_yields() {
    let mut fixture = Fixture::new("round-trip");
    assert_eq!(
        gate_timeout(&run_started()),
        LOSSY_CONSTRUCTED,
        "the fixture must carry a value the wire cannot hold, or this proves nothing"
    );
    assert_ne!(LOSSY_CONSTRUCTED, LOSSY_AS_READ_BACK);

    let returned = fixture
        .emit(run_started_body())
        .expect("run_started applies");

    // (1) What `emit` returned.
    match &returned.body {
        TopologyEventBody::RunStarted { data } => assert_eq!(
            gate_timeout(data),
            LOSSY_AS_READ_BACK,
            "emit returned the event it built rather than the one the wire gives back"
        ),
        other => panic!("not a run_started: {}", other.kind()),
    }
    assert_eq!(
        returned.ts, "2026-08-23T09:41:02Z",
        "the clock seam decides the timestamp"
    );

    // (2) What the live fold holds. `apply_delta` applies the delta
    // `plan_transition` produced, and that delta carries whichever event
    // `plan_transition` was handed — so this is the assertion that it was the
    // checked one.
    assert_eq!(
        gate_timeout(fixture.fold.started().expect("the run started")),
        LOSSY_AS_READ_BACK,
        "the live fold holds more than a replay could ever restore"
    );

    // (3) What a replay builds, from the bytes rather than from either.
    let bytes = fixture.log_bytes();
    let replayed = TopologyFold::parse_log(&bytes).expect("the log parses");
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0], returned, "the log and memory hold two values");
    assert_eq!(
        gate_timeout(
            resume(&fixture.paths)
                .fold()
                .started()
                .expect("the run started")
        ),
        LOSSY_AS_READ_BACK
    );
}

/// A refusal from *before* the append was entered is not an outcome-unknown
/// append, and the protocol does not run for it.
///
/// The distinction is decidable rather than described: the funnel poisons its
/// handle on every entered failure and on no pre-entry one, so `emit` reads
/// `poisoned_at` on both sides of the call. Without that, a poisoned-handle
/// refusal would run the whole protocol — poison the fold, cancel the
/// reservations, reopen the log — for an append that never touched a byte.
#[test]
fn a_refusal_before_the_append_was_entered_does_not_run_the_protocol() {
    let mut fixture = Fixture::started("not-entered");
    let sealed = fixture.log_bytes();

    // Poison the handle without poisoning the fold, which is exactly the state
    // the funnel is in after an entered failure and the fold is not.
    fixture.arm(
        EventSite::Append,
        SubEffectPoint::Synced,
        InjectionMode::ErrorReturn,
    );
    let error = fixture.emit(budget_body()).expect_err("the sync fails");
    assert!(error.append_error().is_some());
    assert!(fixture.log.poisoned_at().is_some());
    fixture.disarm();

    // Now un-poison the fold, leaving the handle poisoned. A real coordinator
    // never reaches this state; the point is that if it did, the protocol would
    // not run a second time on an append that was refused at the door.
    fixture.fold = resume(&fixture.paths).into_log_and_fold().3;
    let after_reopen = fixture.log_bytes();
    let error = fixture
        .emit(pool_body())
        .expect_err("a poisoned handle appends nothing");
    match &error {
        EmitError::NotEntered(cause) => {
            assert!(cause.to_string().contains("handle is poisoned"), "{cause}")
        }
        other => panic!("a pre-entry refusal ran the protocol: {other}"),
    }
    assert!(error.wrote_nothing());
    assert_eq!(
        fixture.log_bytes(),
        after_reopen,
        "a refusal at the door still wrote"
    );
    assert!(
        after_reopen.starts_with(&sealed) && after_reopen.len() > sealed.len(),
        "the first failure's line is durable, so the second emit really is a later one"
    );
    assert!(
        !fixture.fold.is_poisoned(),
        "the protocol ran for an append that was never entered"
    );
    // And nothing else the protocol does happened either.
    assert!(fixture.reservations.is_empty() && fixture.invocations.balances());
    assert!(
        fixture.non_event_sites().is_empty(),
        "{:?}",
        fixture.non_event_sites()
    );
}

/// Every schema-4 append site reaches the protocol, and each reports under its
/// own site.
///
/// `PR4-CONF-002` is in the standing ledger for exactly this shape: a grid that
/// drove one site and reasoned about the others left both contract-named paths
/// emitting no evidence with the whole suite green.
#[test]
fn every_append_site_reaches_the_protocol_under_its_own_site() {
    /// One append site, the kind that belongs at it, and whether that kind
    /// needs a started run in front of it.
    struct Case {
        site: EventSite,
        kind: &'static str,
        body: fn() -> TopologyEventBody,
        needs_start: bool,
    }

    let cases = vec![
        Case {
            site: EventSite::AppendFirst,
            kind: "run_started",
            body: run_started_body,
            needs_start: false,
        },
        Case {
            site: EventSite::Append,
            kind: "budget_exceeded",
            body: budget_body,
            needs_start: true,
        },
        Case {
            site: EventSite::AppendInformational,
            kind: "pool_exhausted",
            body: pool_body,
            needs_start: true,
        },
    ];
    assert_eq!(
        cases.iter().map(|case| case.site).collect::<Vec<_>>(),
        crate::events::log::TOPOLOGY_APPEND_SITES,
        "every site the funnel accepts needs a case of its own kind"
    );

    for Case {
        site,
        kind,
        body,
        needs_start,
    } in cases
    {
        let tag = format!("site-{}", site.name());
        let mut fixture = if needs_start {
            Fixture::started(&tag)
        } else {
            Fixture::new(&tag)
        };
        fixture.arm(site, SubEffectPoint::Synced, InjectionMode::ErrorReturn);
        let error = match fixture.emit(body()) {
            Ok(_) => panic!("`{kind}` was appended despite the injected sync failure"),
            Err(error) => error,
        };
        let failed = append_error(&error);
        assert_eq!(failed.site, site, "`{kind}` was filed under the wrong site");
        assert_eq!(failed.kind, kind);
        assert_eq!(failed.outcome, AppendOutcome::Present);
        assert_eq!(failed.run_id, RUN_ID, "the refusal names the run");
        assert!(failed.to_string().contains(RUN_ID), "{failed}");
        assert!(failed.to_string().contains(kind), "{failed}");
        assert_eq!(
            failed.creator_disposition().is_some(),
            site == EventSite::AppendFirst
        );
    }
}

/// `HarnessEventHooks` answers all four of `EventHooks`'s questions.
///
/// The regression this pins is the one PR7 found: the observer overrode `phase`
/// and `point` only, so the shared bundle recorded that `Event.OpenLog` ran and
/// nothing about what it made durable — for the funnel whose contract is almost
/// entirely durability. With the two overrides missing, `ledger()` is empty and
/// `syncs()` is empty while the funnel demonstrably synced.
#[test]
fn the_harness_event_observer_reports_durability_as_well_as_sites() {
    let harness = Arc::new(Mutex::new(HookHarness::new()));
    let observer = HarnessEventHooks::new(Arc::clone(&harness));
    assert!(
        !observer.ledger().is_recording(),
        "recording is opt-in, as it is for every other family"
    );

    let mut recording = HarnessEventHooks::new(Arc::clone(&harness)).recording_durability();
    let paths = run_paths("harness-durability");
    let mut warnings = Vec::new();
    let mut log = EventLog::open_hooked(
        EventSite::OpenLog,
        &paths.events(),
        &mut warnings,
        &mut recording,
    )
    .expect("open");
    let (line, _) = TopologyLine::round_trip(&TopologyEvent {
        ts: "2026-08-23T09:41:02Z".to_owned(),
        body: run_started_body(),
    })
    .expect("run_started survives its wire format");
    log.append_topology_hooked(EventSite::AppendFirst, &line, &mut recording)
        .expect("append");

    // The site half still reaches the shared harness.
    assert!(
        harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observed(
                EffectSiteId::Event(EventSite::AppendFirst),
                HookPhase::Before
            ),
        "the sites stopped reaching the harness"
    );

    // The durability half now reaches the observer, in order.
    assert_eq!(
        recording.ledger().steps(),
        vec![
            // The open created the log and fsynced its directory, then synced
            // the (empty) prefix.
            DurableStep::SyncedDirectory,
            DurableStep::SyncedFile,
            // The append: one write of the whole line, then flush, then sync.
            DurableStep::Wrote,
            DurableStep::Flushed,
            DurableStep::SyncedData,
        ],
        "the funnel's own order, which no `(site, phase)` key can express"
    );

    // And the per-sync records, which are what `proof_tests[9]` reads.
    let syncs = recording.syncs();
    assert!(
        syncs.iter().any(|record| record.site == EventSite::OpenLog
            && record.point == SubEffectPoint::SyncPrefix
            && record.target == SyncTarget::LogFile),
        "the prefix sync was not reported: {syncs:?}"
    );

    // Clones share both logs, which is what lets a bundle hand a clone into a
    // funnel body and a test still read what the body recorded.
    let clone = recording.clone();
    assert_eq!(clone.syncs().len(), syncs.len());
    assert_eq!(
        clone.ledger().records().len(),
        recording.ledger().records().len()
    );
}

/// The third override, and the only way `WrittenShape::Torn` is reachable.
///
/// `SubEffectPoint::Written`'s frozen kill entry tables **two** durable shapes
/// under one key — torn, and complete-unsynced — so no arming can choose
/// between them and `EventHooks::written_kill_shape` is the choice. While
/// `HarnessEventHooks` left that default in place, T-APPEND's (w) row was
/// unreachable through the shared bundle however a suite armed it, and the
/// omission was invisible: with nothing armed the two shapes write the same
/// bytes.
///
/// Which is why this asserts on the **ledger** rather than on the file. The
/// torn shape is two `write_all`s with the kill consult between them; the
/// complete one is the single `write_all` production performs. Same bytes,
/// different primitives, and only the second observable is the one the shape
/// moves.
#[test]
fn the_harness_event_observer_can_ask_for_the_torn_written_shape() {
    fn append_through(shape: Option<WrittenShape>) -> (Vec<u8>, Vec<DurableRecord>) {
        let paths = run_paths("written-shape");
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness)).recording_durability();
        if let Some(shape) = shape {
            hooks = hooks.with_written_kill_shape(shape);
        }
        let mut warnings = Vec::new();
        let mut log = EventLog::open_hooked(
            EventSite::OpenLog,
            &paths.events(),
            &mut warnings,
            hooks.events(),
        )
        .expect("open");
        hooks.event_observer().ledger().clear();
        let (line, _) = TopologyLine::round_trip(&TopologyEvent {
            ts: "2026-08-23T09:41:02Z".to_owned(),
            body: run_started_body(),
        })
        .expect("run_started survives its wire format");
        log.append_topology_hooked(EventSite::AppendFirst, &line, hooks.events())
            .expect("append");
        let records = hooks
            .event_observer()
            .ledger()
            .records_for(&paths.events())
            .into_iter()
            .filter(|record| record.step == DurableStep::Wrote)
            .collect();
        (read(&paths.events()), records)
    }

    // The default is production's, and nothing about the bundle changes it.
    let (default_bytes, default_writes) = append_through(None);
    assert_eq!(
        default_writes.len(),
        1,
        "the default shape is one `write_all` of the whole line: {default_writes:?}"
    );

    // The complete shape asked for explicitly is the same shape.
    let (complete_bytes, complete_writes) = append_through(Some(WrittenShape::Complete));
    assert_eq!(complete_writes.len(), 1);

    // And the torn shape splits it, which is what puts a kill armed at
    // `Written` in the torn half of its entry.
    let (torn_bytes, torn_writes) = append_through(Some(WrittenShape::Torn));
    assert_eq!(
        torn_writes.len(),
        2,
        "the torn shape did not split the write: {torn_writes:?}"
    );
    assert!(
        torn_writes[0].len > 0 && torn_writes[1].len > 0,
        "both halves carry bytes: {torn_writes:?}"
    );
    assert_eq!(
        torn_writes[0].len + torn_writes[1].len,
        default_writes[0].len,
        "the two halves are the whole line"
    );

    // "moves where a kill lands and **not** what is durable": with nothing
    // armed all three produce the identical file.
    assert_eq!(default_bytes, complete_bytes);
    assert_eq!(default_bytes, torn_bytes);
    assert!(default_bytes.ends_with(b"\n"));
}

// ---------------------------------------------------------------------------
// The production emitter is this protocol, not a second copy of it
// ---------------------------------------------------------------------------

/// [`super::super::run::RunEmitter`] reaches the append-error protocol, and
/// **all five obligations are observed with the caller in the loop**.
///
/// **The assertion is that the forwarder forwards.** `EventEmitter` had one
/// implementation in this tree before the driver existed, and it was
/// `scaffold::FoldedEmitter` — which re-implements the append and therefore
/// runs none of the protocol's obligations. Every dispatch, attempt, settle and
/// candidate test drives through it, so "the pipeline's appends are protected
/// by the protocol" was a claim nothing checked.
///
/// So this drives the *production* emitter over an armed append and asks all
/// five. A `RunEmitter` that had grown its own append path — the duplication
/// shape this slice has paid for four times — would fail here.
///
/// # Why the caller is in the loop
///
/// Obligation (3) is no longer emit's. The ledger belongs to the driver,
/// because it is the same object every Runner process of an attempt registers
/// in and an emitter holding it could not lend it to the attempt that is
/// running. So the emitter hands back an [`super::EmitFailure`] and the caller
/// discharges — and `discharging` is the exact expression
/// `TopologyRun::emit` runs.
///
/// Asserting four here and hoping for the fifth is what this test refuses to
/// do: the obligation moved across this boundary, so this is the test that has
/// to watch it cross.
#[test]
fn the_production_emitter_reaches_the_append_error_protocol() {
    use super::super::dispatch::EventEmitter;
    use super::super::run::RunEmitter;

    let mut fixture = Fixture::started("run-emitter-forwards");

    // A reservation held and an invocation running, so "cancelled" is
    // distinguishable from "there was nothing to cancel" for (2) and (3).
    fixture
        .reservations
        .take(AAY, ReservationKind::Dispatch)
        .expect("the ledger is empty at process start");
    let worker = AttemptIdentities::new(AAY, GenerationId(0), AttemptNumber(1)).worker();
    fixture
        .invocations
        .register(&worker)
        .expect("a fresh identity registers");

    fixture.arm(
        EventSite::Append,
        SubEffectPoint::WrittenFull,
        InjectionMode::ErrorReturn,
    );

    let failure = {
        let mut emitter = RunEmitter {
            identity: &fixture.identity,
            state: EmitState {
                fold: &mut fixture.fold,
                log: &mut fixture.log,
                reservations: &mut fixture.reservations,
                warnings: &mut fixture.warnings,
            },
            clock: &fixture.clock,
        };
        emitter
            .emit(budget_body(), &mut fixture.hooks)
            .expect_err("the flush was made to fail")
    };

    // (3), and it is first because it is the one that moved. The report does
    // not exist until the ledger is handed over — `AppendError` carries a
    // private witness and `UncancelledAppend::cancelling` is its only
    // constructor — so reaching a count to assert is itself the proof.
    let super::EmitFailure::Undischarged(append) = failure else {
        panic!("an entered append did not report as one");
    };
    let report = append.cancelling(&mut fixture.invocations);
    assert_eq!(report.cancelled_invocations, 1);
    assert!(
        fixture.invocations.balances() && fixture.invocations.running().is_empty(),
        "an invocation was left running after the caller discharged"
    );

    // (1)
    assert!(
        fixture.fold.is_poisoned(),
        "the protocol's first obligation. An emitter that appended for itself \
         would leave the fold live here, and every effect after it would be \
         derived from state this process cannot vouch for"
    );

    // (2)
    assert!(report.report.cancelled_reservation);
    assert!(
        fixture.reservations.is_empty() && fixture.reservations.balances(),
        "the provisional reservation was left held"
    );

    // (4): no retry, no cleanup, no report from memory. Derived from the
    // harness so a site group added later is covered without editing a list.
    let ran = fixture.non_event_sites();
    assert!(ran.is_empty(), "the protocol performed an effect: {ran:?}");
    assert!(
        !fixture.paths.public.join("report.json").exists(),
        "a report was written from the poisoned fold"
    );
    assert!(
        fixture.paths.public.is_dir() && fixture.paths.private.is_dir(),
        "the protocol removed a run directory"
    );

    // (5): the prefix was reopened and the barrier established, so the outcome
    // is an answer rather than an absence. Which answer depends on where the
    // injection cut, and the test does not care — it cares that one was
    // reached, because that is what the reopen produces.
    assert!(
        matches!(
            report.report.outcome,
            AppendOutcome::Present | AppendOutcome::Absent | AppendOutcome::Undetermined { .. }
        ),
        "the stable-prefix barrier did not run"
    );
    assert!(report.report.resumable());
}
