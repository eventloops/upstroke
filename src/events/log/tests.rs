//! Extended notes: `docs/internals/events/log/tests.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use super::premove::PremoveEventLog;
use super::*;
use crate::events::{
    BudgetExceeded, BudgetKind, DesignDefect, EventBody, GateSummary, PoolExhausted, TaskCommitted,
};
use crate::gates::ShellKind;
use crate::ir::{
    Artifact, ArtifactId, Effort, Plan, PlanSource, QuestionId, ResolvedEffortPolicy, TaskId,
};
use crate::review::ReviewPlan;
use crate::topology::events::{
    CommitSha, DeferWaitElapsed4, GitRef, IncarnationId, RunStarted4, RunnerContract, RunnerKind,
    RunnerPolicy, TopologyEvent, TopologyEventBody, TopologyLimits,
};
use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion};
use crate::topology::schema::TOPOLOGY_SCHEMA;
use crate::util::{DurabilityLedger, DurableStep};

static SCRATCH: AtomicU32 = AtomicU32::new(0);

fn scratch(tag: &str) -> PathBuf {
    let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "upstroke-event-funnel-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn log_path(tag: &str) -> PathBuf {
    scratch(tag).join("events.jsonl")
}

fn event_log_message(error: &UpstrokeError) -> &str {
    match error {
        UpstrokeError::EventLog { message, .. } => message,
        other => panic!("not an event-log error: {other}"),
    }
}

fn commit(sha: &str, message: &str) -> EventBody {
    EventBody::TaskCommitted {
        task: format!("task-for-{sha}"),
        data: TaskCommitted {
            sha: sha.to_owned(),
            message: message.to_owned(),
        },
    }
}

fn defect(question: &str) -> EventBody {
    EventBody::DesignDefect {
        data: DesignDefect {
            question: QuestionId(question.to_owned()),
            context: "context Ünicode".to_owned(),
            answer: "answer".to_owned(),
        },
    }
}

fn lossy_duration_attempt() -> EventBody {
    EventBody::AttemptFinished {
        task: "t1".to_owned(),
        attempt: 1,
        rung: 0,
        profile: "impl-mid".to_owned(),
        data: Box::new(crate::events::AttemptRecord {
            attempt: 1,
            tier: "mid".to_owned(),
            model: "a-model".to_owned(),
            pool: None,
            resumed: false,
            duration: Duration::from_micros(1_500_123),
            cost_usd: None,
            reviews: Vec::new(),
            session_id: None,
            usage: None,
            failure: None,
        }),
        parking: None,
        transition: None,
        prepared_commit: None,
    }
}

const LOSSY_CONSTRUCTED: Duration = Duration::from_micros(1_500_123);
const LOSSY_AS_READ_BACK: Duration = Duration::from_millis(1_500);

fn duration_of(body: &EventBody) -> Duration {
    match body {
        EventBody::AttemptFinished { data, .. } => data.duration,
        other => panic!("not an attempt_finished: {}", other.kind()),
    }
}

fn unserializable() -> EventBody {
    EventBody::BudgetExceeded {
        data: BudgetExceeded {
            budget: BudgetKind::Run,
            limit_usd: f64::NAN,
            spent_usd: 1.0,
            task: "t1".to_owned(),
        },
    }
}

fn topology_event(round: u32) -> TopologyEvent {
    TopologyEvent {
        ts: "2026-08-20T09:41:02Z".to_owned(),
        body: TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 1_500,
                round,
            },
        },
    }
}

fn topology_line(round: u32) -> TopologyLine {
    TopologyLine::round_trip(&topology_event(round))
        .expect("a defer_wait_elapsed survives its own wire format")
        .0
}

fn run_started_event() -> TopologyEvent {
    TopologyEvent {
        ts: "2026-08-20T09:41:00Z".to_owned(),
        body: TopologyEventBody::RunStarted {
            data: Box::new(RunStarted4 {
                schema: TOPOLOGY_SCHEMA,
                upstroke_version: "0.1.0".to_owned(),
                run_id: "01J8ZQKB2M7NC5PQR0TVWXYZ12".to_owned(),
                incarnation: IncarnationId("01J8ZQKB2M7NC5PQR0TVWXYZ13".to_owned()),
                runner: RunnerPolicy {
                    kind: RunnerKind::Host,
                    policy: RunnerContract::HostV1,
                    image: None,
                    credential_volumes: None,
                },
                probed_agents: vec!["claude-code".to_owned()],
                branch: "upstroke/run-01J8ZQKB2M7NC5PQR0TVWXYZ12".to_owned(),
                integration_ref: GitRef::from("refs/upstroke/integration"),
                base_sha: CommitSha::from("0f5c1c4"),
                execution_root: "/var/lib/upstroke/roots".to_owned(),
                private_dir: "/var/lib/upstroke/private".to_owned(),
                plan_path: "docs/plan.md".to_owned(),
                config_path: Some("upstroke.toml".to_owned()),
                plan_hash: "frozen-hash".to_owned(),
                normalized_plan_digest: inputs().normalized_plan_digest,
                registry_digest:
                    "sha256:1111111111111111111111111111111111111111111111111111111111111111"
                        .to_owned(),
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
                    timeout: Duration::from_secs(60),
                    shell: ShellKind::Sh,
                }],
                interaction_mode: "never".to_owned(),
                chains: Vec::new(),
                effort_policy: ResolvedEffortPolicy {
                    small: Effort::Low,
                    mid: Effort::Medium,
                    frontier: Effort::High,
                    review: Effort::XHigh,
                },
                reviews: ReviewPlan::default(),
            }),
        },
    }
}

fn informational_event() -> TopologyEvent {
    TopologyEvent {
        ts: "2026-08-20T09:41:03Z".to_owned(),
        body: TopologyEventBody::PoolExhausted {
            data: PoolExhausted {
                pool: "claude-code".to_owned(),
                agent: "claude-code".to_owned(),
                reset_at: Some("2026-08-20T10:00:00Z".to_owned()),
                detail: "usage limit reached".to_owned(),
            },
        },
    }
}

fn append_site_lines() -> Vec<(EventSite, TopologyLine)> {
    let lines = vec![
        (
            EventSite::AppendFirst,
            TopologyLine::round_trip(&run_started_event())
                .expect("a run_started survives its own wire format")
                .0,
        ),
        (EventSite::Append, topology_line(1)),
        (
            EventSite::AppendInformational,
            TopologyLine::round_trip(&informational_event())
                .expect("a pool_exhausted survives its own wire format")
                .0,
        ),
    ];
    assert_eq!(
        lines.iter().map(|(site, _)| *site).collect::<Vec<_>>(),
        TOPOLOGY_APPEND_SITES,
        "every schema-4 append site the funnel accepts needs a line of its own kind"
    );
    assert_eq!(
        lines
            .iter()
            .map(|(_, line)| line.kind())
            .collect::<BTreeSet<_>>()
            .len(),
        3,
        "three sites, three distinct event kinds"
    );
    for (site, line) in &lines {
        assert_eq!(
            line.site(),
            *site,
            "`{}` was built from an event the frozen class puts elsewhere",
            site.name()
        );
    }
    lines
}

fn inputs() -> FrozenInputs {
    FrozenInputs {
        plan: Plan {
            source: PlanSource {
                adapter: "markdown".to_owned(),
                hash: "frozen-hash".to_owned(),
            },
            tasks: Vec::new(),
            artifacts: vec![Artifact {
                id: ArtifactId::from("contract"),
                produced_by: Some(TaskId::from("aay")),
            }],
        },
        normalized_plan_digest:
            "sha256:9999999999999999999999999999999999999999999999999999999999999999".to_owned(),
    }
}

#[derive(Debug, Default)]
struct Witness {
    phases: Vec<(EventSite, HookPhase)>,
    offered: Vec<(EventSite, SubEffectPoint, InjectionMode)>,
    ledger: Vec<SyncRecord>,
    durability: DurabilityLedger,
    at_consult: Vec<(SubEffectPoint, InjectionMode, Vec<DurableStep>)>,
}

impl EventHooks for Witness {
    fn phase(&mut self, site: EventSite, phase: HookPhase) {
        self.phases.push((site, phase));
    }

    fn point(&mut self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> Injection {
        self.offered.push((site, point, mode));
        self.at_consult.push((point, mode, self.durability.steps()));
        Injection::Proceed
    }

    fn durability_ledger(&self) -> DurabilityLedger {
        self.durability.clone()
    }

    fn synced(&mut self, record: &SyncRecord) {
        self.ledger.push(record.clone());
    }
}

impl Witness {
    fn recording_durability(mut self) -> Self {
        self.durability = DurabilityLedger::recording();
        self
    }

    fn steps(&self) -> Vec<DurableStep> {
        self.durability.steps()
    }

    fn offered_at(&self, point: SubEffectPoint, mode: InjectionMode) -> bool {
        self.offered
            .iter()
            .any(|(_, offered, offered_mode)| *offered == point && *offered_mode == mode)
    }

    fn file_syncs(&self) -> Vec<u64> {
        self.ledger
            .iter()
            .filter(|record| record.target == SyncTarget::LogFile)
            .map(|record| record.len)
            .collect()
    }

    fn directory_syncs(&self) -> Vec<SubEffectPoint> {
        self.ledger
            .iter()
            .filter(|record| record.target == SyncTarget::LogDirectory)
            .map(|record| record.point)
            .collect()
    }
}

#[derive(Debug)]
struct FailAt {
    point: SubEffectPoint,
    mode: InjectionMode,
    ledger: Vec<SyncRecord>,
    fired: u32,
}

impl FailAt {
    fn error(point: SubEffectPoint) -> Self {
        Self {
            point,
            mode: InjectionMode::ErrorReturn,
            ledger: Vec::new(),
            fired: 0,
        }
    }
}

impl EventHooks for FailAt {
    fn point(&mut self, _site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> Injection {
        if point == self.point && mode == self.mode {
            self.fired += 1;
            return Injection::Error;
        }
        Injection::Proceed
    }

    fn synced(&mut self, record: &SyncRecord) {
        self.ledger.push(record.clone());
    }
}

struct Rewrite {
    after_sync: Option<Vec<u8>>,
    after_proof: Option<Vec<u8>>,
    path: PathBuf,
}

impl Rewrite {
    fn after_sync(path: &Path, bytes: &[u8]) -> Self {
        Self {
            after_sync: Some(bytes.to_vec()),
            after_proof: None,
            path: path.to_path_buf(),
        }
    }

    fn after_proof(path: &Path, bytes: &[u8]) -> Self {
        Self {
            after_sync: None,
            after_proof: Some(bytes.to_vec()),
            path: path.to_path_buf(),
        }
    }
}

impl EventHooks for Rewrite {
    fn phase(&mut self, site: EventSite, phase: HookPhase) {
        if site == EventSite::ProvePrefixStable && phase == HookPhase::After {
            if let Some(bytes) = self.after_proof.take() {
                fs::write(&self.path, bytes).expect("rewrite after the proof");
            }
        }
    }

    fn synced(&mut self, _record: &SyncRecord) {
        if let Some(bytes) = self.after_sync.take() {
            fs::write(&self.path, bytes).expect("rewrite after the sync");
        }
    }
}

#[derive(Debug, Default)]
struct TornWriter;

impl EventHooks for TornWriter {
    fn written_kill_shape(&mut self, _site: EventSite) -> WrittenShape {
        WrittenShape::Torn
    }
}

const SITE_ROLES: &[(EventSite, &str)] = &[
    (EventSite::OpenLog, "open"),
    (EventSite::LegacyOpenLog, "open"),
    (EventSite::AppendFirst, "schema-4 append"),
    (EventSite::Append, "schema-4 append"),
    (EventSite::AppendInformational, "schema-4 append"),
    (EventSite::LegacyAppend, "schema-1..3 append"),
    (
        EventSite::ProvePrefixStable,
        "read-only barrier observation",
    ),
];

#[test]
fn every_event_site_is_classified_and_the_funnel_accepts_exactly_its_own() {
    let classified: BTreeSet<EventSite> = SITE_ROLES.iter().map(|(site, _)| *site).collect();
    let declared: BTreeSet<EventSite> = EventSite::ALL.iter().copied().collect();
    assert_eq!(
        classified, declared,
        "every site the frozen inventory declares needs a role in this table"
    );
    assert_eq!(
        SITE_ROLES
            .iter()
            .map(|(_, role)| *role)
            .collect::<BTreeSet<_>>()
            .len(),
        4,
        "four roles, and a site that acquired a fifth has to be argued about"
    );

    let dir = scratch("partition");
    for (site, role) in SITE_ROLES {
        let path = dir.join(format!("{}.jsonl", site.name()));
        let mut warnings = Vec::new();
        let opened = EventLog::open(*site, &path, &mut warnings);
        assert_eq!(
            opened.is_ok(),
            *role == "open",
            "`Event.{}` opening: role is {role}",
            site.name()
        );
        if let Err(error) = opened {
            assert!(
                error.to_string().contains("is not an open site"),
                "the refusal has to say why: {error}"
            );
            continue;
        }
    }

    let lines = append_site_lines();
    let legacy_path = dir.join("legacy-appends.jsonl");
    let shared_path = dir.join("shared-appends.jsonl");
    for (site, role) in SITE_ROLES {
        let mut warnings = Vec::new();
        let mut legacy = EventLog::open(EventSite::LegacyOpenLog, &legacy_path, &mut warnings)
            .expect("a legacy handle");
        let mut shared = EventLog::open(EventSite::OpenLog, &shared_path, &mut warnings)
            .expect("a shared handle");
        assert_eq!(
            legacy.append(*site, commit("a", "m")).is_ok(),
            *role == "schema-1..3 append",
            "`Event.{}` through the legacy append",
            site.name()
        );
        let line = lines
            .iter()
            .find(|(candidate, _)| candidate == site)
            .map_or_else(|| topology_line(1), |(_, line)| line.clone());
        assert_eq!(
            shared.append_topology(*site, &line).is_ok(),
            *role == "schema-4 append",
            "`Event.{}` through the schema-4 append",
            site.name()
        );
    }
}

#[test]
fn a_handle_does_not_mix_the_legacy_and_shared_scopes() {
    let path = log_path("scopes");
    let mut warnings = Vec::new();

    let mut legacy =
        EventLog::open(EventSite::LegacyOpenLog, &path, &mut warnings).expect("a legacy handle");
    let refused = legacy
        .append_topology(EventSite::Append, &topology_line(1))
        .expect_err("a schema-3 log does not take schema-4 lines");
    assert!(refused.to_string().contains("does not accept"), "{refused}");

    let shared_path = log_path("scopes-shared");
    let mut shared =
        EventLog::open(EventSite::OpenLog, &shared_path, &mut warnings).expect("a shared handle");
    let refused = shared
        .append(EventSite::LegacyAppend, commit("a", "m"))
        .expect_err("a schema-4 log does not take legacy events");
    assert!(refused.to_string().contains("does not accept"), "{refused}");

    assert_eq!(fs::read(&path).expect("legacy log").len(), 0);
    assert_eq!(fs::read(&shared_path).expect("shared log").len(), 0);
    assert_eq!(legacy.poisoned_at(), None, "a refusal is not a poisoning");
    assert_eq!(shared.poisoned_at(), None);
}

const INFORMATIONAL_KINDS: &[&str] = &["capacity_snapshot", "pool_exhausted", "design_defect"];

#[test]
fn an_events_append_site_is_decided_by_the_frozen_transaction_class() {
    let expected: &[(&str, EventSite)] = &[
        ("run_started", EventSite::AppendFirst),
        ("capacity_snapshot", EventSite::AppendInformational),
        ("pool_exhausted", EventSite::AppendInformational),
        ("design_defect", EventSite::AppendInformational),
        ("defer_wait_elapsed", EventSite::Append),
        ("task_merged", EventSite::Append),
        ("run_finished", EventSite::Append),
    ];
    for (kind, site) in expected {
        let derived = if *kind == "run_started" {
            EventSite::AppendFirst
        } else if INFORMATIONAL_KINDS.contains(kind) {
            EventSite::AppendInformational
        } else {
            EventSite::Append
        };
        assert_eq!(
            derived, *site,
            "the table disagrees with itself about {kind}"
        );
    }

    let line = topology_line(4);
    assert_eq!(line.kind(), "defer_wait_elapsed");
    assert_eq!(line.site(), EventSite::Append);

    let classified: Vec<(&str, EventSite)> = append_site_lines()
        .iter()
        .map(|(_, line)| (line.kind(), line.site()))
        .collect();
    for (kind, site) in &classified {
        let from_table = expected
            .iter()
            .find(|(candidate, _)| candidate == kind)
            .map(|(_, site)| *site)
            .unwrap_or_else(|| panic!("{kind} is not in the transcribed table"));
        assert_eq!(*site, from_table, "{kind} was filed at the wrong site");
    }
    assert_eq!(
        classified
            .iter()
            .map(|(_, site)| *site)
            .collect::<BTreeSet<_>>()
            .len(),
        3,
        "three kinds, three distinct sites: {classified:?}"
    );

    let path = log_path("site-for-kind");
    let mut warnings = Vec::new();
    let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    let refused = log
        .append_topology(EventSite::AppendInformational, &line)
        .expect_err("a transaction event does not belong at the lenient site");
    assert!(
        refused.to_string().contains("belongs at `Event.Append`"),
        "the refusal names where it does belong: {refused}"
    );
    assert_eq!(fs::read(&path).expect("log").len(), 0);
}

fn open_grid() -> Vec<(&'static str, Option<Vec<u8>>)> {
    let good = b"{\"ts\":\"2026-08-20T00:00:00Z\",\"event\":\"design_defect\"}".to_vec();
    let mut split_utf8 = good.clone();
    split_utf8.push(b'\n');
    split_utf8.extend_from_slice(&[0xE2, 0x82]);
    vec![
        ("absent", None),
        ("empty", Some(Vec::new())),
        (
            "one committed line",
            Some([good.clone(), b"\n".to_vec()].concat()),
        ),
        (
            "three committed lines",
            Some(
                [
                    good.clone(),
                    b"\n".to_vec(),
                    good.clone(),
                    b"\n".to_vec(),
                    good.clone(),
                    b"\n".to_vec(),
                ]
                .concat(),
            ),
        ),
        (
            "torn tail of 1 byte",
            Some([good.clone(), b"\n".to_vec(), b"{".to_vec()].concat()),
        ),
        (
            "torn tail of 5 bytes",
            Some([good.clone(), b"\n".to_vec(), b"{\"ts\"".to_vec()].concat()),
        ),
        (
            "torn tail of 12 bytes",
            Some([good.clone(), b"\n".to_vec(), b"{\"ts\":\"2026".to_vec()].concat()),
        ),
        (
            "torn tail only, no prefix",
            Some(b"{\"ts\":\"2026".to_vec()),
        ),
        (
            "torn tail that is valid JSON",
            Some([good.clone(), b"\n".to_vec(), good.clone()].concat()),
        ),
        ("torn tail of split UTF-8", Some(split_utf8)),
        (
            "blank committed line",
            Some([good.clone(), b"\n\n".to_vec()].concat()),
        ),
        ("a lone newline", Some(b"\n".to_vec())),
        (
            "trailing carriage return",
            Some([good.clone(), b"\n".to_vec(), b"\r".to_vec()].concat()),
        ),
    ]
}

#[test]
fn the_grid_varies_shape_and_tail_length_and_tail_content_independently() {
    let grid = open_grid();
    assert_eq!(grid.len(), 13, "thirteen shapes");
    let tails: BTreeSet<usize> = grid
        .iter()
        .filter_map(|(_, bytes)| bytes.as_ref())
        .map(|bytes| {
            let keep = bytes
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |index| index + 1);
            bytes.len() - keep
        })
        .collect();
    assert_eq!(
        tails.len(),
        6,
        "six distinct torn-tail lengths (0, 1, 5, 11, 12, 53): {tails:?}"
    );
    let committed: BTreeSet<usize> = grid
        .iter()
        .filter_map(|(_, bytes)| bytes.as_ref())
        .map(|bytes| bytes.iter().filter(|byte| **byte == b'\n').count())
        .collect();
    assert!(
        committed.len() >= 4,
        "at least four distinct committed-line counts: {committed:?}"
    );
    assert_eq!(
        grid.iter().filter(|(_, bytes)| bytes.is_none()).count(),
        1,
        "exactly one absent-file cell"
    );
}

#[test]
fn the_legacy_open_is_byte_identical_to_the_pre_move_writer() {
    for (name, seed) in open_grid() {
        let moved_dir = scratch("identity-moved");
        let premove_dir = scratch("identity-premove");
        let moved = moved_dir.join("events.jsonl");
        let premove = premove_dir.join("events.jsonl");
        if let Some(seed) = &seed {
            fs::write(&moved, seed).expect("seed");
            fs::write(&premove, seed).expect("seed");
        }

        let mut moved_warnings = Vec::new();
        let mut premove_warnings = Vec::new();
        let moved_result = EventLog::open(EventSite::LegacyOpenLog, &moved, &mut moved_warnings);
        let premove_result = PremoveEventLog::open(&premove, &mut premove_warnings);
        if let (Ok(moved_log), Ok(premove_log)) = (&moved_result, &premove_result) {
            assert_eq!(
                moved_log.path(),
                moved,
                "{name}: the moved writer kept its path"
            );
            assert_eq!(premove_log.path(), premove, "{name}: and so did the oracle");
        }

        assert_eq!(
            moved_result.is_ok(),
            premove_result.is_ok(),
            "{name}: one opened and the other did not"
        );
        assert_eq!(
            moved_warnings
                .iter()
                .map(|w| w.replace(&moved.display().to_string(), "<log>"))
                .collect::<Vec<_>>(),
            premove_warnings
                .iter()
                .map(|w| w.replace(&premove.display().to_string(), "<log>"))
                .collect::<Vec<_>>(),
            "{name}: the warnings differ"
        );
        assert_eq!(
            fs::read(&moved).expect("moved log"),
            fs::read(&premove).expect("premove log"),
            "{name}: the bytes on disk differ"
        );
    }
}

#[test]
fn the_legacy_append_is_byte_identical_to_the_pre_move_writer() {
    let bodies: Vec<EventBody> = vec![
        commit("0f5c1c4", "first"),
        defect("q-1"),
        lossy_duration_attempt(),
        commit("deadbee", "second Ünicode"),
    ];
    assert_ne!(
        LOSSY_CONSTRUCTED, LOSSY_AS_READ_BACK,
        "the lossy fixture must actually be lossy, or it witnesses nothing"
    );
    for (name, seed) in open_grid() {
        let moved_dir = scratch("append-moved");
        let premove_dir = scratch("append-premove");
        let moved = moved_dir.join("events.jsonl");
        let premove = premove_dir.join("events.jsonl");
        if let Some(seed) = &seed {
            fs::write(&moved, seed).expect("seed");
            fs::write(&premove, seed).expect("seed");
        }
        let mut warnings = Vec::new();
        let mut moved_log = EventLog::open(EventSite::LegacyOpenLog, &moved, &mut warnings)
            .expect("the moved writer opens");
        let mut premove_log =
            PremoveEventLog::open(&premove, &mut warnings).expect("the pre-move writer opens");

        let before = crate::util::rfc3339_utc_now();
        for body in &bodies {
            let moved_event = moved_log
                .append(EventSite::LegacyAppend, body.clone())
                .expect("moved append");
            let premove_event = premove_log.append(body.clone()).expect("premove append");
            assert_eq!(
                moved_event.body, premove_event.body,
                "{name}: the round-tripped bodies differ"
            );
            if matches!(body, EventBody::AttemptFinished { .. }) {
                assert_eq!(
                    duration_of(&moved_event.body),
                    LOSSY_AS_READ_BACK,
                    "{name}: the moved writer returned the constructed duration, \
                     not the one the log will read back"
                );
                assert_eq!(
                    duration_of(&premove_event.body),
                    LOSSY_AS_READ_BACK,
                    "{name}: and the oracle agrees, so this is the shared contract"
                );
            }
        }
        let after = crate::util::rfc3339_utc_now();

        assert_eq!(
            normalize_timestamps(&fs::read(&moved).expect("moved log")),
            normalize_timestamps(&fs::read(&premove).expect("premove log")),
            "{name}: the appended bytes differ"
        );
        for (writer, path) in [("moved", &moved), ("oracle", &premove)] {
            for stamp in appended_timestamps(path, committed_lines(seed.as_ref())) {
                assert!(
                    stamp >= before && stamp <= after,
                    "{name}/{writer}: appended ts `{stamp}` is not a time this \
                     append could have happened at ({before}..={after})"
                );
            }
        }
        let committed_before = committed_lines(seed.as_ref());
        let contents = fs::read(&moved).expect("moved log");
        assert_eq!(
            contents.iter().filter(|byte| **byte == b'\n').count(),
            committed_before + bodies.len(),
            "{name}: the wrong number of committed lines"
        );
        assert_eq!(contents.last(), Some(&b'\n'), "{name}: no commit marker");
    }
}

#[test]
fn a_legacy_open_that_fails_fails_the_way_the_pre_move_writer_did() {
    type Case = (&'static str, fn(&Path) -> PathBuf);
    let cases: &[Case] = &[
        ("a parent directory that does not exist", |dir| {
            dir.join("no-such-directory").join("events.jsonl")
        }),
        ("the path is an existing directory", |dir| {
            let path = dir.join("events.jsonl");
            fs::create_dir_all(&path).expect("a directory where the log goes");
            path
        }),
        ("a read-only file", |dir| {
            let path = dir.join("events.jsonl");
            fs::write(&path, b"{\"ts\":\"2026-08-20T09:41:00Z\"}\n").expect("seed");
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&path, permissions).expect("make it read-only");
            path
        }),
        ("a read-only file with a torn tail", |dir| {
            let path = dir.join("events.jsonl");
            fs::write(&path, b"{\"ts\":\"2026-08-20T09:41:00Z\"}\n{\"ts\"").expect("seed");
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&path, permissions).expect("make it read-only");
            path
        }),
    ];

    let mut failed = 0_usize;
    let mut unexercisable = Vec::new();
    for (name, build) in cases {
        let moved = build(&scratch("open-fail-moved"));
        let premove = build(&scratch("open-fail-premove"));

        let mut moved_warnings = Vec::new();
        let mut premove_warnings = Vec::new();
        let moved_result = EventLog::open(EventSite::LegacyOpenLog, &moved, &mut moved_warnings);
        let premove_result = PremoveEventLog::open(&premove, &mut premove_warnings);

        assert_eq!(
            moved_result.is_err(),
            premove_result.is_err(),
            "{name}: one writer failed and the other did not"
        );
        let (Err(moved_error), Err(premove_error)) = (&moved_result, &premove_result) else {
            unexercisable.push(*name);
            continue;
        };
        assert_eq!(
            std::mem::discriminant(moved_error),
            std::mem::discriminant(premove_error),
            "{name}: the moved writer returns a different UpstrokeError variant \
             than the pre-move one did ({moved_error:?} vs {premove_error:?})"
        );
        assert!(
            matches!(premove_error, UpstrokeError::Io { .. }),
            "{name}: the frozen oracle's legacy open contract is UpstrokeError::Io: \
             {premove_error:?}"
        );
        assert!(
            matches!(moved_error, UpstrokeError::Io { .. }),
            "{name}: the moved writer must keep it: {moved_error:?}"
        );
        assert_eq!(
            moved_error
                .to_string()
                .replace(&moved.display().to_string(), "<log>"),
            premove_error
                .to_string()
                .replace(&premove.display().to_string(), "<log>"),
            "{name}: the rendered errors differ"
        );
        assert!(
            moved_error
                .to_string()
                .contains(&moved.display().to_string()),
            "{name}: the error must name the log: {moved_error}"
        );
        assert!(
            moved_warnings.is_empty() && premove_warnings.is_empty(),
            "{name}: a failed open warns about nothing"
        );
        failed += 1;
    }

    assert!(
        failed >= 2,
        "at least two cells must really fail, or this grid compares two `Ok`s: \
         {failed} of {} (unexercisable here: {unexercisable:?})",
        cases.len()
    );
    assert_eq!(cases.len(), 4, "four failing-path shapes");
}

fn committed_lines(seed: Option<&Vec<u8>>) -> usize {
    seed.map(|bytes| bytes.iter().filter(|byte| **byte == b'\n').count())
        .unwrap_or(0)
}

fn appended_timestamps(path: &Path, skip: usize) -> Vec<String> {
    let text = String::from_utf8(fs::read(path).expect("log")).expect("utf-8 log");
    text.lines()
        .skip(skip)
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("an appended line must parse: {error}: {line}"));
            value
                .get("ts")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("an appended line carries a ts: {line}"))
                .to_owned()
        })
        .collect()
}

#[test]
fn the_legacy_append_stamps_the_clocks_answer_at_every_entry_point() {
    let path = log_path("ts-value");
    let mut warnings = Vec::new();
    let mut log =
        EventLog::open(EventSite::LegacyOpenLog, &path, &mut warnings).expect("open the log");

    let before = crate::util::rfc3339_utc_now();
    let plain = log
        .append(EventSite::LegacyAppend, commit("0f5c1c4", "plain"))
        .expect("append");
    let hooked = log
        .append_hooked(
            EventSite::LegacyAppend,
            commit("deadbee", "hooked"),
            &mut NoEventHooks,
        )
        .expect("append_hooked");
    let after = crate::util::rfc3339_utc_now();

    assert_ne!(
        before, "1970-01-01T00:00:00Z",
        "this machine's clock reads as the epoch, so the assertion below proves nothing"
    );
    for (entry, event) in [("append", &plain), ("append_hooked", &hooked)] {
        assert!(
            event.ts >= before && event.ts <= after,
            "{entry}: returned ts `{}` is not a time this append could have \
             happened at ({before}..={after})",
            event.ts
        );
    }
    let written = appended_timestamps(&path, 0);
    assert_eq!(
        written,
        vec![plain.ts.clone(), hooked.ts.clone()],
        "the persisted ts is not the returned one"
    );
    for stamp in &written {
        assert!(
            stamp.as_str() >= before.as_str() && stamp.as_str() <= after.as_str(),
            "persisted ts `{stamp}` is outside ({before}..={after})"
        );
        assert_eq!(stamp.len(), "2026-08-21T00:00:00Z".len(), "the fixed shape");
    }
}

#[test]
fn the_legacy_append_returns_the_event_a_replay_of_this_log_yields() {
    let path = log_path("readback");
    let mut warnings = Vec::new();
    let mut log =
        EventLog::open(EventSite::LegacyOpenLog, &path, &mut warnings).expect("open the log");

    let constructed = lossy_duration_attempt();
    assert_eq!(
        duration_of(&constructed),
        LOSSY_CONSTRUCTED,
        "the fixture carries the sub-millisecond duration"
    );
    let returned = log
        .append(EventSite::LegacyAppend, constructed.clone())
        .expect("append");
    let returned_hooked = log
        .append_hooked(
            EventSite::LegacyAppend,
            constructed.clone(),
            &mut NoEventHooks,
        )
        .expect("append_hooked");

    let mut replay_warnings = Vec::new();
    let replayed = crate::events::read_all(&path, &mut replay_warnings).expect("replay this log");
    assert!(
        replay_warnings.is_empty(),
        "a clean log replays without warnings"
    );
    assert_eq!(replayed.len(), 2, "two appends, two events");
    for (entry, event) in [("append", &returned), ("append_hooked", &returned_hooked)] {
        assert_eq!(
            duration_of(&event.body),
            LOSSY_AS_READ_BACK,
            "{entry}: the constructed duration survived the append, so live state \
             holds more than a replay can restore"
        );
        assert_ne!(
            duration_of(&event.body),
            LOSSY_CONSTRUCTED,
            "{entry}: the returned event is the constructed one"
        );
    }
    for (index, event) in replayed.iter().enumerate() {
        let returned = if index == 0 {
            &returned
        } else {
            &returned_hooked
        };
        assert_eq!(
            event.body, returned.body,
            "line {index}: the replayed event differs from the returned one"
        );
        assert_eq!(&event.ts, &returned.ts, "line {index}: and so does its ts");
    }
}

fn normalize_timestamps(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_ref();
    while let Some(start) = rest.find("\"ts\":\"") {
        let (before, after) = rest.split_at(start + "\"ts\":\"".len());
        out.push_str(before);
        let end = after.find('"').expect("a ts value is a closed string");
        out.push_str("<ts>");
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[test]
fn the_legacy_open_performs_none_of_the_syncs_the_pre_move_open_did_not() {
    assert!(
        EventSite::LegacyOpenLog.sub_effects().is_empty(),
        "the frozen inventory gives the legacy open no points"
    );
    assert!(EventSite::LegacyAppend.sub_effects().is_empty());

    let path = log_path("legacy-no-sync");
    fs::write(&path, b"{\"a\":1}\ntorn").expect("seed");
    let mut witness = Witness::default();
    let mut warnings = Vec::new();
    let mut log =
        EventLog::open_hooked(EventSite::LegacyOpenLog, &path, &mut warnings, &mut witness)
            .expect("legacy open");
    log.append_hooked(EventSite::LegacyAppend, commit("a", "m"), &mut witness)
        .expect("legacy append");

    assert_eq!(warnings.len(), 1, "the torn tail is still warned about");
    assert!(
        witness.ledger.is_empty(),
        "the legacy path synced through the ledger: {:?}",
        witness.ledger
    );
    assert_eq!(
        witness
            .phases
            .iter()
            .filter(|(site, _)| *site == EventSite::LegacyOpenLog)
            .count(),
        2,
        "both hook phases still exist for the legacy open"
    );
}

#[test]
fn an_append_writes_the_whole_line_once_then_flushes_then_syncs() {
    let path = log_path("append-trace");
    let mut warnings = Vec::new();
    let mut witness = Witness::default().recording_durability();
    let mut log = EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut witness)
        .expect("open");
    let before = witness.durability.records().len();

    log.append_topology_hooked(EventSite::Append, &topology_line(1), &mut witness)
        .expect("append");

    let all = witness.durability.records();
    let appended: Vec<_> = all[before..].to_vec();
    assert_eq!(
        appended
            .iter()
            .map(|record| record.step)
            .collect::<Vec<_>>(),
        vec![
            DurableStep::Wrote,
            DurableStep::Flushed,
            DurableStep::SyncedData
        ],
        "the append's exact primitive sequence: {appended:?}"
    );
    let on_disk = fs::read(&path).expect("log");
    assert_eq!(
        appended[0].len,
        on_disk.len() as u64,
        "the one write carried the whole line, its LF commit marker included"
    );
    assert_eq!(
        on_disk.last(),
        Some(&b'\n'),
        "and the line the count is measured against really is complete"
    );
    assert_eq!(
        appended[2].len,
        on_disk.len() as u64,
        "and the sync made all of it durable"
    );
}

#[test]
fn the_synced_consults_are_offered_after_the_data_is_durable() {
    let path = log_path("synced-coordinate");
    let mut warnings = Vec::new();
    let mut witness = Witness::default().recording_durability();
    let mut log = EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut witness)
        .expect("open");
    witness.at_consult.clear();
    witness.durability.clear();
    log.append_topology_hooked(EventSite::Append, &topology_line(1), &mut witness)
        .expect("append");

    for mode in InjectionMode::ALL {
        let (_, _, at) = witness
            .at_consult
            .iter()
            .find(|(point, offered, _)| *point == SubEffectPoint::Synced && offered == mode)
            .unwrap_or_else(|| panic!("Synced/{mode:?} was never offered at all"));
        assert!(
            at.contains(&DurableStep::SyncedData),
            "Synced/{mode:?} was offered before the append's own sync_data ran, so a fault \
             injected there stands in place of the sync rather than following it: {at:?}"
        );
        assert_eq!(
            at.last(),
            Some(&DurableStep::SyncedData),
            "Synced/{mode:?} is offered immediately after the sync and not later: {at:?}"
        );
    }

    let written = witness
        .at_consult
        .iter()
        .find(|(point, mode, _)| {
            *point == SubEffectPoint::Written && *mode == InjectionMode::ErrorReturn
        })
        .map(|(_, _, at)| at.clone())
        .expect("Written/ErrorReturn is offered");
    assert!(
        written.is_empty(),
        "the partial-write coordinate is offered before anything is written: {written:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_real_write_failure_is_attempted_once_poisons_the_handle_and_is_not_retried() {
    assert!(
        Path::new("/dev/full").exists(),
        "this host has no always-failing device, so nothing here is measured"
    );
    let dir = scratch("real-enospc");
    let path = dir.join("events.jsonl");
    std::os::unix::fs::symlink("/dev/full", &path).expect("symlink");

    let mut warnings = Vec::new();
    let mut witness = Witness::default().recording_durability();
    let mut log =
        EventLog::open(EventSite::LegacyOpenLog, &path, &mut warnings).expect("the device opens");
    let before = witness.durability.records().len();

    let error = log
        .append_hooked(EventSite::LegacyAppend, commit("a", "first"), &mut witness)
        .expect_err("every write to /dev/full returns ENOSPC");
    assert!(
        matches!(error, UpstrokeError::Io { .. }),
        "a real failure keeps the exact error the pre-move writer returned: {error}"
    );

    let recorded = witness.durability.records();
    let attempts: Vec<_> = recorded[before..]
        .iter()
        .filter(|record| record.step == DurableStep::Wrote)
        .collect();
    assert_eq!(
        attempts.len(),
        1,
        "one primitive attempt and one error, never a retry: {attempts:?}"
    );
    assert!(
        !witness
            .durability
            .steps()
            .contains(&DurableStep::SyncedData),
        "and nothing past the failed write ran"
    );
    assert_eq!(
        log.poisoned_at(),
        Some(SubEffectPoint::Written),
        "the handle is poisoned at the point the real failure reached"
    );
    assert_eq!(
        log.poisoned_site(),
        Some(EventSite::LegacyAppend),
        "and names the site the failing append was made at"
    );
    let later = log
        .append_hooked(EventSite::LegacyAppend, commit("b", "second"), &mut witness)
        .expect_err("a poisoned handle refuses");
    assert!(
        later.to_string().contains(POISONED_PREFIX),
        "the refusal is the poison, not a second attempt: {later}"
    );
    let after_refusal = witness.durability.records();
    assert_eq!(
        after_refusal[before..]
            .iter()
            .filter(|record| record.step == DurableStep::Wrote)
            .count(),
        1,
        "and the refusal reached no primitive"
    );
}

#[test]
fn open_truncates_the_torn_tail_before_it_syncs_and_syncs_the_shortened_length() {
    let path = log_path("truncate-then-sync");
    let complete = b"{\"a\":1}\n".len() as u64;
    fs::write(&path, b"{\"a\":1}\nthis tail was never finished").expect("seed");
    let full = fs::metadata(&path).expect("metadata").len();
    assert!(full > complete, "the fixture really is torn");

    let mut warnings = Vec::new();
    let mut witness = Witness::default().recording_durability();
    let _log = EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut witness)
        .expect("reopen");

    let records = witness.durability.records();
    let expected: Vec<DurableStep> = vec![
        DurableStep::Truncated,
        DurableStep::SyncedFile,
        DurableStep::SyncedDirectory,
    ];
    assert_eq!(
        witness.steps(),
        expected,
        "truncate, then sync the surviving prefix, then its directory: {records:?}"
    );
    for record in &records {
        assert_eq!(
            record.len, complete,
            "every step of the barrier is about the SHORTENED length ({complete}), not the \
             pre-normalized {full}: {record:?}"
        );
    }
    assert_eq!(
        fs::metadata(&path).expect("metadata").len(),
        complete,
        "and the file itself agrees with the ledger"
    );
}

#[test]
fn open_syncs_the_surviving_prefix_and_the_ledger_agrees_with_the_filesystem() {
    let path = log_path("sync-prefix");

    let mut warnings = Vec::new();
    let mut first = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("create");
    let mut unsynced = FailAt::error(SubEffectPoint::WrittenFull);
    first
        .append_topology_hooked(EventSite::Append, &topology_line(1), &mut unsynced)
        .expect_err("the append returns at WrittenFull");
    drop(first);

    let on_disk = fs::read(&path).expect("log");
    assert!(!on_disk.is_empty(), "the unsynced line is in the file");
    assert_eq!(on_disk.last(), Some(&b'\n'), "and it is complete");

    let mut witness = Witness::default();
    let mut warnings = Vec::new();
    let _log = EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut witness)
        .expect("reopen");

    let length = fs::metadata(&path).expect("metadata").len();
    assert_eq!(
        witness.file_syncs(),
        vec![length],
        "one file sync, of the whole surviving prefix; the filesystem says {length}"
    );
    assert_eq!(
        length,
        on_disk.len() as u64,
        "and nothing was truncated: the line was complete"
    );
}

#[test]
fn open_fsyncs_the_directory_when_it_creates_the_log_and_after_a_truncation() {
    let expected_directory_syncs = 1;

    let created = log_path("dir-fsync-create");
    let mut witness = Witness::default();
    let mut warnings = Vec::new();
    EventLog::open_hooked(EventSite::OpenLog, &created, &mut warnings, &mut witness)
        .expect("create");
    assert_eq!(
        witness.directory_syncs().len(),
        expected_directory_syncs,
        "creating the log fsyncs its directory on this platform"
    );
    assert_eq!(witness.directory_syncs(), vec![SubEffectPoint::Create]);

    let torn = log_path("dir-fsync-truncate");
    fs::write(&torn, b"{\"a\":1}\nhalf").expect("seed");
    let mut witness = Witness::default();
    let mut warnings = Vec::new();
    EventLog::open_hooked(EventSite::OpenLog, &torn, &mut warnings, &mut witness).expect("reopen");
    assert_eq!(
        witness.directory_syncs().len(),
        expected_directory_syncs,
        "a truncation changed the length, so the directory is synced too"
    );
    assert_eq!(witness.directory_syncs(), vec![SubEffectPoint::SyncPrefix]);

    let untouched = log_path("dir-fsync-untouched");
    fs::write(&untouched, b"{\"a\":1}\n").expect("seed");
    let mut witness = Witness::default();
    let mut warnings = Vec::new();
    EventLog::open_hooked(EventSite::OpenLog, &untouched, &mut warnings, &mut witness)
        .expect("reopen");
    assert!(
        witness.directory_syncs().is_empty(),
        "nothing changed the directory, so nothing syncs it"
    );
    assert_eq!(witness.file_syncs().len(), 1);
}

#[test]
fn a_torn_tail_is_truncated_on_open_with_a_warning_at_both_open_sites() {
    for site in [EventSite::OpenLog, EventSite::LegacyOpenLog] {
        let path = log_path(&format!("torn-{}", site.name()));
        fs::write(&path, b"{\"a\":1}\n{\"b\":2 unfinished").expect("seed");
        let mut warnings = Vec::new();
        let log = EventLog::open(site, &path, &mut warnings).expect("open");
        assert_eq!(fs::read(&path).expect("log"), b"{\"a\":1}\n");
        assert_eq!(warnings.len(), 1, "one warning at {}", site.name());
        assert!(
            warnings[0].contains("discarded 17 trailing byte(s)"),
            "the warning counts the bytes: {}",
            warnings[0]
        );
        assert_eq!(log.opened_at(), site);
    }
}

#[test]
fn an_injected_sync_failure_at_open_names_syncprefix_and_hands_out_no_handle() {
    let path = log_path("sync-fails");
    fs::write(&path, b"{\"a\":1}\n").expect("seed");
    let mut failing = FailAt::error(SubEffectPoint::SyncPrefix);
    let mut warnings = Vec::new();

    let refused = EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut failing)
        .expect_err("a SyncPrefix error refuses the open");
    assert!(
        refused
            .to_string()
            .contains(SubEffectPoint::SyncPrefix.name()),
        "the error names the point: {refused}"
    );
    assert!(
        refused.to_string().contains(INJECTED_PREFIX),
        "and says it was simulated: {refused}"
    );
    assert_eq!(failing.fired, 1, "the coordinate fired exactly once");
    assert!(
        failing.ledger.is_empty(),
        "the coordinate is before the sync, so nothing was made durable"
    );

    let mut failing = FailAt::error(SubEffectPoint::SyncPrefix);
    let error = establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut failing)
        .expect_err("the barrier does not hold");
    assert_eq!(error.step, BarrierStep::SyncPrefix);
    assert!(
        error.to_string().contains("Event.OpenLog.SyncPrefix"),
        "the barrier error names the step: {error}"
    );
}

#[test]
fn every_open_point_is_offered_in_every_mode_the_frozen_inventory_declares() {
    let points = EventSite::OpenLog.sub_effects();
    assert_eq!(
        points,
        &[
            SubEffectPoint::Create,
            SubEffectPoint::TruncateTornTail,
            SubEffectPoint::SyncPrefix
        ],
        "the frozen inventory's three open points, in its order"
    );

    let mut offered = Vec::new();
    for (tag, seed) in [
        ("create", None),
        ("truncate", Some(b"{\"a\":1}\nhalf".to_vec())),
    ] {
        let path = log_path(&format!("offers-{tag}"));
        if let Some(seed) = seed {
            fs::write(&path, seed).expect("seed");
        }
        let mut witness = Witness::default();
        let mut warnings = Vec::new();
        EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut witness)
            .expect("open");
        offered.extend(witness.offered.iter().copied());
    }

    for point in points {
        for mode in point.modes() {
            assert!(
                offered
                    .iter()
                    .any(|(site, offered, offered_mode)| *site == EventSite::OpenLog
                        && offered == point
                        && offered_mode == mode),
                "`Event.OpenLog` never offered `{point}` in {mode:?} mode"
            );
        }
    }
    assert_eq!(offered.len(), 8, "and offered nothing else: {offered:?}");
}

const ERROR_RETURN_CASES: &[(SubEffectPoint, bool)] = &[
    (SubEffectPoint::Written, false),
    (SubEffectPoint::WrittenFull, true),
    (SubEffectPoint::Synced, true),
];

#[test]
fn every_error_return_case_leaves_its_tabled_shape_names_its_point_and_poisons_the_handle() {
    assert_eq!(
        ERROR_RETURN_CASES.len(),
        3,
        "three cases, and `fault_injection_registry.structure` names three"
    );
    assert_eq!(
        ERROR_RETURN_CASES
            .iter()
            .map(|(_, complete)| *complete)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );

    let mut sites: Vec<(EventSite, Option<TopologyLine>)> = append_site_lines()
        .into_iter()
        .map(|(site, line)| (site, Some(line)))
        .collect();
    sites.push((EventSite::LegacyAppend, None));
    assert_eq!(
        sites.len(),
        4,
        "three schema-4 append sites and the legacy one"
    );

    for (case, (point, leaves_complete_line)) in ERROR_RETURN_CASES.iter().enumerate() {
        for (index, (site, line)) in sites.iter().enumerate() {
            let site = *site;
            let path = log_path(&format!("err-{case}-{index}"));
            let open_site = if site == EventSite::LegacyAppend {
                EventSite::LegacyOpenLog
            } else {
                EventSite::OpenLog
            };
            let mut warnings = Vec::new();
            let mut log = EventLog::open(open_site, &path, &mut warnings).expect("open");

            let mut failing = FailAt::error(*point);
            let error = match line {
                None => log
                    .append_hooked(site, commit("a", "first"), &mut failing)
                    .expect_err("the append returns Err"),
                Some(line) => log
                    .append_topology_hooked(site, line, &mut failing)
                    .expect_err("the append returns Err"),
            };

            let quoted = |point: &SubEffectPoint| format!("`{}`", point.name());
            assert!(
                event_log_message(&error).contains(&quoted(point)),
                "`{}` must name its point: {error}",
                point.name()
            );
            for other in ERROR_RETURN_CASES.iter().map(|(other, _)| other) {
                assert_eq!(
                    event_log_message(&error).contains(&quoted(other)),
                    other == point,
                    "the message names exactly the injected point and no other: {error}"
                );
            }
            assert_eq!(
                log.poisoned_at(),
                Some(*point),
                "`{}` must poison the handle at its own point",
                point.name()
            );
            assert_eq!(
                log.poisoned_site(),
                Some(site),
                "and at the site the append was made at"
            );

            let durable = fs::read(&path).expect("log");
            assert!(!durable.is_empty(), "something was written");
            assert_eq!(
                durable.last() == Some(&b'\n'),
                *leaves_complete_line,
                "`{}` left the wrong durable shape: {:?}",
                point.name(),
                String::from_utf8_lossy(&durable)
            );

            for attempt in 1..=3 {
                let later = match line {
                    None => log
                        .append(site, commit("b", "second"))
                        .expect_err("a poisoned handle refuses"),
                    Some(line) => log
                        .append_topology(site, line)
                        .expect_err("a poisoned handle refuses"),
                };
                assert!(
                    event_log_message(&later).contains(POISONED_PREFIX)
                        && event_log_message(&later).contains(&quoted(point)),
                    "attempt {attempt}: the refusal names the poisoning point: {later}"
                );
                assert!(
                    event_log_message(&later).contains(&format!("`Event.{}`", site.name())),
                    "attempt {attempt}: and the site it was poisoned at: {later}"
                );
                assert_eq!(
                    log.poisoned_at(),
                    Some(*point),
                    "attempt {attempt}: the poison is still there afterwards"
                );
                assert_eq!(
                    fs::read(&path).expect("log"),
                    durable,
                    "attempt {attempt}: and appended nothing"
                );
            }
        }
    }
}

#[test]
fn a_handle_poisoned_at_one_site_names_that_site_when_refused_at_another() {
    let lines = append_site_lines();
    assert!(
        lines.len() >= 2,
        "this test needs two distinct append sites on one handle"
    );

    for (poison_at, poison_line) in &lines {
        for (attempt_at, attempt_line) in &lines {
            if poison_at == attempt_at {
                continue;
            }
            let path = log_path(&format!(
                "crossed-{}-{}",
                poison_at.name(),
                attempt_at.name()
            ));
            let mut warnings = Vec::new();
            let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");

            let mut failing = FailAt::error(SubEffectPoint::Written);
            log.append_topology_hooked(*poison_at, poison_line, &mut failing)
                .expect_err("the append returns Err");
            assert_eq!(
                log.poisoned_site(),
                Some(*poison_at),
                "the handle records the site it was poisoned at"
            );

            let refused = log
                .append_topology(*attempt_at, attempt_line)
                .expect_err("a poisoned handle refuses");
            let message = event_log_message(&refused);
            assert!(
                message.contains(&format!("`Event.{}`", poison_at.name())),
                "the refusal must name the site the handle was POISONED at \
                 (`Event.{}`), not the one now being attempted: {refused}",
                poison_at.name()
            );
            assert!(
                !message.contains(&format!("`Event.{}`", attempt_at.name())),
                "…and must not name `Event.{}`, which is where the outcome did NOT \
                 become unknown: {refused}",
                attempt_at.name()
            );
            assert!(
                message.contains("`Written`"),
                "the point half is held constant here and must still be named: {refused}"
            );
        }
    }
}

#[test]
fn a_value_the_wire_cannot_carry_does_not_enter_the_append_and_does_not_poison() {
    let path = log_path("unserializable");
    let mut warnings = Vec::new();
    let mut log = EventLog::open(EventSite::LegacyOpenLog, &path, &mut warnings).expect("open");

    let refused = log
        .append(EventSite::LegacyAppend, unserializable())
        .expect_err("a NaN does not survive JSON");
    assert!(
        refused
            .to_string()
            .contains("budget_exceeded does not survive its own wire format"),
        "{refused}"
    );
    assert_eq!(
        fs::read(&path).expect("log").len(),
        0,
        "nothing was written"
    );
    assert_eq!(log.poisoned_at(), None, "and the handle is still usable");

    log.append(EventSite::LegacyAppend, commit("a", "m"))
        .expect("the next append still works");
    assert!(fs::read(&path).expect("log").ends_with(b"\n"));
}

#[test]
fn reopening_through_openlog_is_what_clears_a_poisoning() {
    let path = log_path("reopen-clears");
    let mut warnings = Vec::new();
    let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    let mut failing = FailAt::error(SubEffectPoint::Synced);
    log.append_topology_hooked(EventSite::Append, &topology_line(1), &mut failing)
        .expect_err("Err at Synced");
    assert_eq!(log.poisoned_at(), Some(SubEffectPoint::Synced));
    drop(log);

    let mut reopened = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("reopen");
    assert_eq!(reopened.poisoned_at(), None, "the reopen is the clearing");
    reopened
        .append_topology(EventSite::Append, &topology_line(2))
        .expect("and the handle works");
    assert_eq!(
        fs::read(&path)
            .expect("log")
            .iter()
            .filter(|b| **b == b'\n')
            .count(),
        2,
        "the errored line was durable and the new one is beside it"
    );
}

#[test]
fn every_append_point_is_offered_in_every_mode_the_frozen_inventory_declares() {
    let points = EventSite::Append.sub_effects();
    assert_eq!(
        points,
        &[
            SubEffectPoint::Written,
            SubEffectPoint::WrittenFull,
            SubEffectPoint::Synced
        ],
        "the frozen inventory's three append points, in its order"
    );

    for (site, line) in append_site_lines() {
        assert_eq!(
            site.sub_effects(),
            points,
            "`{}` declares different points from `Event.Append`",
            site.name()
        );
        let path = log_path(&format!("append-offers-{}", site.name()));
        let mut warnings = Vec::new();
        let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
        let mut witness = Witness::default();
        log.append_topology_hooked(site, &line, &mut witness)
            .expect("append");

        for point in points {
            for mode in point.modes() {
                assert!(
                    witness.offered_at(*point, *mode),
                    "`Event.{}` never offered `{point}` in {mode:?} mode",
                    site.name()
                );
            }
        }
        assert!(
            witness
                .offered
                .iter()
                .all(|(offered_site, _, _)| *offered_site == site),
            "`Event.{}` offered a coordinate under another site's name: {:?}",
            site.name(),
            witness.offered
        );
        assert_eq!(
            witness.offered.len(),
            5,
            "`Event.{}` offered something else: {:?}",
            site.name(),
            witness.offered
        );
        assert!(
            !witness.offered_at(SubEffectPoint::WrittenFull, InjectionMode::Kill),
            "`WrittenFull` declares no kill mode; offering one would manufacture a \
             coverage obligation the design does not make"
        );
        assert_eq!(
            witness
                .phases
                .iter()
                .filter(|(phase_site, _)| *phase_site == site)
                .count(),
            2,
            "`Event.{}`: both hook phases",
            site.name()
        );
        assert!(
            fs::read(&path).expect("log").ends_with(b"\n"),
            "`Event.{}` committed its line",
            site.name()
        );
    }
}

#[test]
fn the_written_kill_shape_moves_where_a_kill_lands_and_not_what_is_durable() {
    let mut bytes = Vec::new();
    for (tag, shape) in [("complete", false), ("torn", true)] {
        let path = log_path(&format!("kill-shape-{tag}"));
        let mut warnings = Vec::new();
        let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
        if shape {
            log.append_topology_hooked(EventSite::Append, &topology_line(7), &mut TornWriter)
                .expect("append");
        } else {
            log.append_topology(EventSite::Append, &topology_line(7))
                .expect("append");
        }
        bytes.push(fs::read(&path).expect("log"));
    }
    assert_eq!(bytes[0], bytes[1], "the durable result is the same");
    assert_eq!(bytes[0].last(), Some(&b'\n'));
    assert_eq!(
        NoEventHooks.written_kill_shape(EventSite::Append),
        WrittenShape::Complete,
        "production asks for one write_all, which is what the pre-move writer did"
    );
}

#[derive(Debug)]
struct KillAt {
    point: SubEffectPoint,
    shape: WrittenShape,
}

impl EventHooks for KillAt {
    fn point(&mut self, _site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> Injection {
        if point == self.point && mode == InjectionMode::Kill {
            Injection::Kill
        } else {
            Injection::Proceed
        }
    }

    fn written_kill_shape(&mut self, _site: EventSite) -> WrittenShape {
        self.shape
    }
}

const KILL_CASES: &[(&str, SubEffectPoint, WrittenShape)] = &[
    ("create", SubEffectPoint::Create, WrittenShape::Complete),
    (
        "truncate-torn-tail",
        SubEffectPoint::TruncateTornTail,
        WrittenShape::Complete,
    ),
    (
        "sync-prefix",
        SubEffectPoint::SyncPrefix,
        WrittenShape::Complete,
    ),
    ("written-torn", SubEffectPoint::Written, WrittenShape::Torn),
    (
        "written-complete",
        SubEffectPoint::Written,
        WrittenShape::Complete,
    ),
    ("synced", SubEffectPoint::Synced, WrittenShape::Complete),
];

fn declared_kill_points() -> BTreeSet<SubEffectPoint> {
    EventSite::ALL
        .iter()
        .flat_map(|site| site.sub_effects())
        .filter(|point| point.modes().contains(&InjectionMode::Kill))
        .copied()
        .collect()
}

fn kill_at(case: &str, point: SubEffectPoint, path: &Path) -> Vec<u8> {
    let helper = format!(
        "{}::event_funnel_kill_helper",
        module_path!()
            .split_once("::")
            .expect("this module is not the crate root")
            .1
    );
    let output = std::process::Command::new(std::env::current_exe().expect("the test executable"))
        .args([helper.as_str(), "--ignored", "--exact"])
        .env(KILL_CASE_ENV, case)
        .env(KILL_LOG_ENV, path)
        .output()
        .unwrap_or_else(|error| panic!("{case}: spawning the helper: {error}"));

    assert!(
        !output.status.success(),
        "{case}: the helper exited cleanly, so the kill at `{}` never fired",
        point.name()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "{case}: the helper panicked rather than aborting, so what is on disk is not what a kill \
         leaves:\n{stderr}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            output.status.signal(),
            Some(libc::SIGABRT),
            "{case}: a kill is an abort, and this child died some other way"
        );
    }
    #[cfg(not(unix))]
    {
        assert_ne!(
            output.status.code(),
            Some(101),
            "{case}: 101 is the harness's panic status, not an abort"
        );
    }
    fs::read(path).expect("the log the killed process left")
}

const KILL_CASE_ENV: &str = "UPSTROKE_EVENT_FUNNEL_KILL";
const KILL_LOG_ENV: &str = "UPSTROKE_EVENT_FUNNEL_KILL_LOG";

#[test]
#[ignore = "subprocess helper"]
fn event_funnel_kill_helper() {
    let Ok(case) = std::env::var(KILL_CASE_ENV) else {
        return;
    };
    let path = PathBuf::from(std::env::var_os(KILL_LOG_ENV).expect("the parent names the log"));
    let (_, point, shape) = KILL_CASES
        .iter()
        .find(|(name, _, _)| *name == case)
        .unwrap_or_else(|| panic!("the parent named a case this helper does not have: {case}"));

    let mut warnings = Vec::new();
    let mut kill = KillAt {
        point: *point,
        shape: *shape,
    };
    if EventSite::OpenLog.sub_effects().contains(point) {
        let _ = EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut kill);
    } else {
        let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
        let _ = log.append_topology_hooked(EventSite::Append, &topology_line(1), &mut kill);
    }
    std::process::exit(0);
}

#[test]
fn every_kill_point_the_inventory_declares_has_a_case_and_no_case_is_invented() {
    let declared = declared_kill_points();
    assert_eq!(
        declared,
        BTreeSet::from([
            SubEffectPoint::Create,
            SubEffectPoint::TruncateTornTail,
            SubEffectPoint::SyncPrefix,
            SubEffectPoint::Written,
            SubEffectPoint::Synced,
        ]),
        "the frozen inventory's kill points moved: {declared:?}"
    );
    let covered: BTreeSet<SubEffectPoint> = KILL_CASES.iter().map(|(_, point, _)| *point).collect();
    assert_eq!(
        covered, declared,
        "every declared kill point needs a case and no case may invent one"
    );
    assert_eq!(
        KILL_CASES.len(),
        6,
        "six cells over five points: `Written`'s one kill entry tables two durable shapes"
    );
    assert_eq!(
        KILL_CASES
            .iter()
            .map(|(case, _, _)| *case)
            .collect::<BTreeSet<_>>()
            .len(),
        6,
        "six distinct case names, or the helper cannot tell them apart"
    );
    assert!(
        !KILL_CASES
            .iter()
            .any(|(_, point, _)| *point == SubEffectPoint::WrittenFull),
        "`WrittenFull` declares no kill mode, and a case for it would manufacture a coverage \
         obligation the design does not make"
    );
}

#[test]
fn a_kill_at_each_open_point_leaves_the_shape_the_packet_tables() {
    let prefix = topology_line(0).committed_bytes().to_vec();
    let torn = [prefix.clone(), b"{\"ts\":\"2026".to_vec()].concat();

    let created = log_path("kill-create");
    let after = kill_at("create", SubEffectPoint::Create, &created);
    assert!(
        created.exists(),
        "the log the funnel created did not survive the kill"
    );
    assert!(
        after.is_empty(),
        "a created log holds no events yet: {after:?}"
    );

    let truncated = log_path("kill-truncate-torn-tail");
    fs::write(&truncated, &torn).expect("seed a torn tail");
    let after = kill_at(
        "truncate-torn-tail",
        SubEffectPoint::TruncateTornTail,
        &truncated,
    );
    assert_eq!(
        after, prefix,
        "the point's claim is that the unterminated line *was* truncated before the handle"
    );

    let unsynced = log_path("kill-sync-prefix");
    fs::write(&unsynced, &prefix).expect("seed a complete prefix");
    let after = kill_at("sync-prefix", SubEffectPoint::SyncPrefix, &unsynced);
    assert_eq!(
        after, prefix,
        "a kill at SyncPrefix leaves the prefix exactly as it found it"
    );
    let mut warnings = Vec::new();
    let mut witness = Witness::default();
    EventLog::open_hooked(EventSite::OpenLog, &unsynced, &mut warnings, &mut witness)
        .expect("the next open repeats the barrier");
    assert_eq!(
        witness.file_syncs(),
        vec![prefix.len() as u64],
        "\"the next open repeats the barrier\": it syncs the whole surviving prefix"
    );
    assert!(warnings.is_empty(), "nothing was torn: {warnings:?}");
}

#[test]
fn a_kill_at_each_append_point_leaves_the_shape_the_packet_tables() {
    let seed = topology_line(0).committed_bytes().to_vec();
    assert_eq!(
        seed.last(),
        Some(&b'\n'),
        "the previous prefix is committed"
    );
    let line = topology_line(1).committed_bytes().to_vec();

    let append_cases: Vec<&(&str, SubEffectPoint, WrittenShape)> = KILL_CASES
        .iter()
        .filter(|(_, point, _)| !EventSite::OpenLog.sub_effects().contains(point))
        .collect();
    assert_eq!(append_cases.len(), 3, "the three append-point cells");

    let mut durable = Vec::new();
    for (case, point, shape) in append_cases {
        let path = log_path(&format!("kill-{case}"));
        fs::write(&path, &seed).expect("seed the previous prefix");
        let after = kill_at(case, *point, &path);

        assert!(
            after.starts_with(&seed),
            "{case}: the previous prefix did not survive"
        );
        let appended = &after[seed.len()..];
        match shape {
            WrittenShape::Torn => {
                assert!(!appended.is_empty(), "{case}: nothing was written at all");
                assert_ne!(
                    after.last(),
                    Some(&b'\n'),
                    "{case}: `Written`'s torn entry leaves a line with no commit marker"
                );
                assert!(
                    line.starts_with(appended),
                    "{case}: the torn bytes are not a prefix of the line being written"
                );
            }
            WrittenShape::Complete => {
                assert_eq!(
                    appended, line,
                    "{case}: the whole newline-terminated line is what this entry leaves"
                );
            }
        }

        let mut warnings = Vec::new();
        let reopened = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("reopen");
        drop(reopened);
        let normalized = fs::read(&path).expect("the log after the next open");
        match shape {
            WrittenShape::Torn => {
                assert_eq!(
                    normalized, seed,
                    "{case}: \"truncated on the next open, previous prefix\""
                );
                assert_eq!(warnings.len(), 1, "{case}: and warned about it");
                assert!(
                    warnings[0].contains("never finished being written"),
                    "{case}: {}",
                    warnings[0]
                );
            }
            WrittenShape::Complete => {
                assert_eq!(
                    normalized, after,
                    "{case}: a committed line is not a torn tail and is not truncated"
                );
                assert!(warnings.is_empty(), "{case}: {warnings:?}");
            }
        }
        durable.push(normalized);
    }

    assert_eq!(durable.len(), 3);
    assert_ne!(durable[0], durable[1], "torn and complete are not the same");
    assert_eq!(
        durable[1], durable[2],
        "a complete-unsynced line and a synced one are indistinguishable to the next reader"
    );
}

#[test]
fn a_fresh_log_establishes_the_barrier_trivially_and_hands_out_a_handle() {
    let path = log_path("barrier-fresh");
    let mut warnings = Vec::new();
    let mut witness = Witness::default();
    let mut prefix = establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut witness)
        .expect("a fresh log establishes the barrier");

    assert!(prefix.bytes().is_empty(), "no prefix");
    assert!(prefix.fold().started().is_none(), "and nothing folded");
    assert_eq!(
        witness.file_syncs().len(),
        1,
        "the empty prefix is still synced"
    );
    prefix
        .log()
        .append_topology(EventSite::Append, &topology_line(1))
        .expect("the handle the barrier entitles this command to");
}

#[test]
fn the_barrier_syncs_before_it_rereads_and_proves_before_it_replays() {
    let path = log_path("barrier-order");
    let mut warnings = Vec::new();
    let mut witness = Witness::default();
    establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut witness)
        .expect("barrier holds");

    let phases: Vec<String> = witness
        .phases
        .iter()
        .map(|(site, phase)| format!("{}/{phase}", site.name()))
        .collect();
    assert_eq!(
        phases,
        vec![
            "OpenLog/before",
            "OpenLog/after",
            "ProvePrefixStable/before",
            "ProvePrefixStable/after",
        ],
        "the barrier's steps in the order stable_prefix_barrier states them"
    );
    assert_eq!(witness.ledger.len(), 2);
}

#[test]
fn an_unstable_reread_refuses_naming_prove_prefix_stable_and_hands_out_no_handle() {
    let committed = b"{\"ts\":\"2026-08-20T09:41:02Z\",\"event\":\"defer_wait_elapsed\",\"data\":{\"waited_ms\":1500,\"round\":1}}\n";
    let mut a_byte = committed.to_vec();
    let position = a_byte.len() - 4;
    a_byte[position] = b'9';
    let mut longer = committed.to_vec();
    longer.extend_from_slice(committed);
    let mut torn_again = committed.to_vec();
    torn_again.extend_from_slice(b"{\"ts\"");

    let cases: &[(&str, Vec<u8>, &str)] = &[
        (
            "a torn tail reappeared",
            torn_again,
            "does not end at a commit marker",
        ),
        (
            "the length changed",
            longer,
            "byte(s) where the prefix synced at open was",
        ),
        (
            "a byte changed",
            a_byte,
            "differs from the prefix synced at open at byte",
        ),
    ];
    assert_eq!(
        cases
            .iter()
            .map(|(_, bytes, _)| bytes.len())
            .collect::<BTreeSet<_>>()
            .len(),
        3,
        "three cells, three distinct rewritten lengths"
    );

    for (name, rewritten, expected) in cases {
        let path = log_path(&format!("unstable-{}", name.replace(' ', "-")));
        fs::write(&path, committed).expect("seed");
        let mut warnings = Vec::new();
        let mut rewriter = Rewrite::after_sync(&path, rewritten);
        let error = establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut rewriter)
            .expect_err("an unstable reread refuses");
        assert_eq!(
            error.step,
            BarrierStep::ProvePrefixStable,
            "{name}: the wrong step"
        );
        assert!(
            error.detail.contains(expected),
            "{name}: the detail says which clause failed: {}",
            error.detail
        );
        assert!(
            error
                .to_string()
                .contains("No append handle was handed out"),
            "{name}: {error}"
        );
    }

    let details: BTreeSet<String> = cases
        .iter()
        .map(|(_, rewritten, _)| {
            let path = log_path("unstable-detail");
            fs::write(&path, committed).expect("seed");
            let mut warnings = Vec::new();
            let mut rewriter = Rewrite::after_sync(&path, rewritten);
            establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut rewriter)
                .expect_err("unstable")
                .detail
        })
        .collect();
    assert_eq!(
        details.len(),
        3,
        "three clauses, three details: {details:?}"
    );
}

#[test]
fn checked_replay_consumes_exactly_the_reread_bytes() {
    let path = log_path("replay-exact-bytes");
    let mut warnings = Vec::new();
    let mut rewriter = Rewrite::after_proof(&path, b"not an event at all\n");
    let prefix = establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut rewriter)
        .expect("the barrier replays the bytes it proved, not the file");
    assert!(
        prefix.bytes().is_empty(),
        "and those bytes are the proven ones"
    );
    assert_eq!(
        fs::read(&path).expect("log"),
        b"not an event at all\n",
        "the file really was rewritten under it"
    );
}

#[test]
fn invalid_terminated_line_refused_not_repaired() {
    let path = log_path("rewritten");
    let corrupt: &[u8] = b"{\"ts\":\"2026-08-20T09:41:02Z\",\"event\":\"not_an_event\"}\n";
    fs::write(&path, corrupt).expect("seed");
    let mut warnings = Vec::new();
    let error = establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut NoEventHooks)
        .expect_err("a committed line that is not an event is corruption");
    assert_eq!(error.step, BarrierStep::CheckedReplay);
    assert!(
        error.detail.contains("line 1"),
        "the refusal names the line: {}",
        error.detail
    );
    assert_eq!(
        fs::read(&path).expect("the log after the refusal"),
        corrupt,
        "the refusal changed the log"
    );
    assert!(
        warnings.is_empty(),
        "and warned about nothing: {warnings:?}"
    );
}

#[test]
fn the_parsed_events_really_reach_the_checked_fold() {
    let path = log_path("replay-reached");
    let mut warnings = Vec::new();
    let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    log.append_topology(EventSite::Append, &topology_line(1))
        .expect("a defer_wait_elapsed is a well-formed line");
    drop(log);

    let error = establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut NoEventHooks)
        .expect_err("a topology log that does not start with run_started is refused");
    assert_eq!(error.step, BarrierStep::CheckedReplay);
    assert!(
        error.detail.contains("before this log's `run_started`"),
        "the fold's own refusal, not the parser's: {}",
        error.detail
    );
}

#[test]
fn a_first_line_digest_that_disagrees_with_the_commit_record_refuses() {
    let path = log_path("first-line-digest");
    let mut warnings = Vec::new();
    let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    log.append_topology(EventSite::Append, &topology_line(1))
        .expect("append");
    drop(log);

    let bytes = fs::read(&path).expect("log");
    let actual = first_line_digest(&bytes).expect("a committed first line");
    let expected = {
        let end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("newline");
        format!("sha256:{:x}", Sha256::digest(&bytes[..end]))
    };
    assert_eq!(
        actual, expected,
        "the digest is over the line without its newline"
    );

    let disagreeing = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    let error = establish_stable_prefix(
        &path,
        inputs(),
        Some(disagreeing),
        &mut warnings,
        &mut NoEventHooks,
    )
    .expect_err("a first line the commit record does not recognise refuses");
    assert_eq!(error.step, BarrierStep::ProvePrefixStable);
    assert!(error.detail.contains(disagreeing), "{}", error.detail);

    let empty = log_path("first-line-digest-empty");
    let error = establish_stable_prefix(
        &empty,
        inputs(),
        Some(disagreeing),
        &mut warnings,
        &mut NoEventHooks,
    )
    .expect_err("a commit record without its committed line refuses");
    assert_eq!(error.step, BarrierStep::ProvePrefixStable);
    assert!(
        error.detail.contains("no committed first line"),
        "{}",
        error.detail
    );
}

#[test]
fn every_barrier_step_is_reachable_and_named() {
    assert_eq!(BarrierStep::ALL.len(), 4);
    let names: BTreeSet<&str> = BarrierStep::ALL.iter().map(|step| step.name()).collect();
    assert_eq!(names.len(), 4, "four distinct names");
    for step in BarrierStep::ALL {
        assert!(
            step.name().starts_with("Event.") || step.name() == "the checked replay",
            "a step's name has to be something the registry can be keyed by: {step}"
        );
    }

    let missing = scratch("barrier-open-fails")
        .join("no-such-directory")
        .join("events.jsonl");
    let mut warnings = Vec::new();
    let error = establish_stable_prefix(&missing, inputs(), None, &mut warnings, &mut NoEventHooks)
        .expect_err("an unopenable log refuses at the open");
    assert_eq!(error.step, BarrierStep::OpenLog);
}

fn digest_of(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn seeded_prefix(tag: &str) -> (PathBuf, Vec<u8>, Vec<TopologyEvent>) {
    let path = log_path(tag);
    let mut warnings = Vec::new();
    let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    log.append_topology(EventSite::Append, &topology_line(7))
        .expect("the before-append prefix");
    drop(log);
    let bytes = fs::read(&path).expect("the seeded log");
    let events = TopologyFold::parse_log(&bytes).expect("the seed replays");
    (path, bytes, events)
}

fn after_append_events(before: &[TopologyEvent]) -> Vec<TopologyEvent> {
    let mut events = before.to_vec();
    events.push(topology_event(1));
    events
}

#[test]
fn torn_tail_truncated_on_open_and_recovery_matches_before_append_row() {
    let (path, before, before_events) = seeded_prefix("torn-before-append-row");

    let killed = kill_at("written-torn", SubEffectPoint::Written, &path);
    assert!(
        killed.len() > before.len(),
        "the child died before it wrote anything, so there is no torn tail to truncate"
    );
    assert!(
        !killed.ends_with(b"\n"),
        "a torn tail has no commit marker: {}",
        String::from_utf8_lossy(&killed[before.len()..])
    );

    let mut warnings = Vec::new();
    let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    drop(log);

    let survived = fs::read(&path).expect("the log after the open");
    assert_eq!(
        digest_of(&survived),
        digest_of(&before),
        "the surviving prefix is not the before-append one"
    );
    assert_eq!(
        TopologyFold::parse_log(&survived).expect("the surviving prefix replays"),
        before_events,
        "recovery would follow the after-append order for a line that was never committed"
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("never finished being written")),
        "the truncation is reported: {warnings:?}"
    );
    assert_ne!(before_events, after_append_events(&before_events));
}

#[test]
fn unsynced_line_recovery_matches_whichever_prefix_survived() {
    let (path, _before, before_events) = seeded_prefix("unsynced-whichever");
    let after_events = after_append_events(&before_events);
    assert_ne!(before_events, after_events, "the two rows must differ");

    let killed = kill_at("written-complete", SubEffectPoint::Written, &path);
    assert!(
        killed.ends_with(b"\n"),
        "the complete-unsynced shape is the whole line, commit marker included"
    );

    let mut warnings = Vec::new();
    let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    drop(log);
    let survived = fs::read(&path).expect("the log after the open");
    let recovered = TopologyFold::parse_log(&survived).expect("the surviving prefix replays");

    assert!(
        recovered == before_events || recovered == after_events,
        "recovery followed neither row: {recovered:?}"
    );
    assert_eq!(
        recovered, after_events,
        "on this machine the line survived the kill, so recovery follows the after-append order"
    );
    assert!(
        warnings.is_empty(),
        "a complete line is not a torn tail and nothing was truncated: {warnings:?}"
    );
    assert_eq!(
        digest_of(&survived),
        digest_of(&killed),
        "the open changed a prefix that was already at a commit marker"
    );
}

#[test]
fn synced_line_recovery_matches_after_append_row() {
    let (path, before, before_events) = seeded_prefix("synced-after-append-row");
    let after_events = after_append_events(&before_events);

    let killed = kill_at("synced", SubEffectPoint::Synced, &path);
    assert!(killed.ends_with(b"\n"));
    assert!(killed.len() > before.len());

    let mut warnings = Vec::new();
    let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    drop(log);
    let survived = fs::read(&path).expect("the log after the open");

    assert_eq!(
        TopologyFold::parse_log(&survived).expect("the surviving prefix replays"),
        after_events,
        "a synced line is not 'either prefix'"
    );
    assert_ne!(
        TopologyFold::parse_log(&survived).expect("replays"),
        before_events,
        "recovery followed the before-append row for a line that was made durable"
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn unsynced_line_made_durable_by_barrier_survives_later_power_loss() {
    let (path, _before, before_events) = seeded_prefix("barrier-durability");
    let after_events = after_append_events(&before_events);

    let unsynced = kill_at("written-complete", SubEffectPoint::Written, &path);
    assert!(unsynced.ends_with(b"\n"));
    let full = unsynced.len() as u64;

    let mut witness = Witness::default().recording_durability();
    let mut warnings = Vec::new();
    let log = EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut witness)
        .expect("open");
    drop(log);
    assert_eq!(
        witness.file_syncs(),
        vec![full],
        "the open synced the surviving prefix at its full length"
    );
    assert!(witness.steps().contains(&DurableStep::SyncedFile));

    let torn = kill_at("written-torn", SubEffectPoint::Written, &path);
    assert!(
        !torn.ends_with(b"\n"),
        "the second crash left no commit marker"
    );
    assert!(torn.len() > unsynced.len());

    let mut warnings = Vec::new();
    let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    drop(log);
    let survived = fs::read(&path).expect("the log after the second open");

    assert_eq!(
        digest_of(&survived),
        digest_of(&unsynced),
        "the barrier-synced prefix did not survive the second crash"
    );
    assert_eq!(
        TopologyFold::parse_log(&survived).expect("the surviving prefix replays"),
        after_events,
        "the line the barrier made durable was reverted by a later loss"
    );
    assert_ne!(before_events, after_events);
}

#[test]
fn unsynced_line_lost_before_barrier_converges_to_before_append_order() {
    let (path, before, before_events) = seeded_prefix("unsynced-lost");
    let after_events = after_append_events(&before_events);

    let unsynced = kill_at("written-complete", SubEffectPoint::Written, &path);
    assert!(unsynced.ends_with(b"\n"), "the line reached the file");
    assert!(unsynced.len() > before.len());

    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for truncation")
        .set_len(before.len() as u64)
        .expect("the unsynced tail is lost");

    let mut warnings = Vec::new();
    let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    drop(log);
    let survived = fs::read(&path).expect("the log after the open");

    assert_eq!(digest_of(&survived), digest_of(&before));
    assert_eq!(
        TopologyFold::parse_log(&survived).expect("the surviving prefix replays"),
        before_events,
        "a lost line still steered recovery"
    );
    assert!(
        warnings.is_empty(),
        "a lost line is not a torn tail: {warnings:?}"
    );

    let (kept_path, _, kept_before) = seeded_prefix("unsynced-kept");
    let kept = kill_at("written-complete", SubEffectPoint::Written, &kept_path);
    assert!(kept.ends_with(b"\n"));
    let mut warnings = Vec::new();
    let log = EventLog::open(EventSite::OpenLog, &kept_path, &mut warnings).expect("open");
    drop(log);
    assert_eq!(
        TopologyFold::parse_log(&fs::read(&kept_path).expect("log")).expect("replays"),
        after_append_events(&kept_before),
        "without the loss the same fixture must reach the after-append order"
    );
    assert_ne!(before_events, after_events);
}

#[test]
fn unstable_reread_after_open_sync_refuses_resumably() {
    let path = log_path("unstable-resumable");
    let committed = topology_line(3);
    let mut warnings = Vec::new();
    let mut log = EventLog::open(EventSite::OpenLog, &path, &mut warnings).expect("open");
    log.append_topology(EventSite::Append, &committed)
        .expect("a committed prefix");
    drop(log);
    let synced = fs::read(&path).expect("log");
    assert!(!synced.is_empty());

    let mut rewriter = Rewrite::after_sync(&path, b"");
    let error = establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut rewriter)
        .expect_err("an unstable reread authorizes nothing");
    assert_eq!(error.step, BarrierStep::ProvePrefixStable);
    assert!(
        error.to_string().contains("the run is resumable"),
        "the refusal says so in the words the packet uses: {error}"
    );

    assert_eq!(
        fs::read(&path).expect("the log after the refusal"),
        Vec::<u8>::new(),
        "the refusal touched the log"
    );

    let mut prefix =
        establish_stable_prefix(&path, inputs(), None, &mut warnings, &mut NoEventHooks)
            .expect("the next resume establishes the barrier");
    assert!(prefix.bytes().is_empty());
    assert_eq!(prefix.log().opened_at(), EventSite::OpenLog);
    assert_eq!(prefix.log().poisoned_at(), None);

    let untouched = log_path("unstable-resumable-control");
    fs::write(&untouched, &synced).expect("seed");
    let control =
        establish_stable_prefix(&untouched, inputs(), None, &mut warnings, &mut NoEventHooks)
            .expect_err("these lines are refused by the checked fold, not by the proof");
    assert_eq!(
        control.step,
        BarrierStep::CheckedReplay,
        "with the reread stable the proof passes and the barrier reaches the replay"
    );
}

#[test]
fn the_event_log_is_written_in_exactly_one_module() {
    const PRIMITIVES: &[&str] = &["write_all(", "sync_data(", "set_len(", "OpenOptions::new()"];
    let funnel = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/events/log.rs");
    let module = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/events/mod.rs");

    let funnel_code = strip_comments(&fs::read_to_string(&funnel).expect("the funnel"));
    let module_source = fs::read_to_string(&module).expect("the module");
    let module_code = production_region(&strip_comments(&module_source)).to_owned();
    assert!(
        funnel_code.len() < fs::read_to_string(&funnel).expect("the funnel").len(),
        "the comment strip removed nothing, so the count below is measuring prose"
    );

    let production = production_region(&funnel_code);
    for primitive in PRIMITIVES {
        assert!(
            production.contains(primitive),
            "`{primitive}` left the funnel"
        );
        assert!(
            !module_code.contains(primitive),
            "`{primitive}` is in `src/events/mod.rs`, which is not the funnel module"
        );
    }
    assert_eq!(
        production.matches("self.file.write_all(").count(),
        1,
        "one write path, reached from every append shape; a second needs a reason"
    );
    assert_eq!(production.matches(".sync_data()").count(), 1);
    assert_eq!(
        production.matches("util::fsync_file(").count(),
        1,
        "the file's own barrier, once, and through the shared wrapper"
    );
    assert_eq!(
        production.matches("util::fsync_dir(").count(),
        1,
        "the directory's barrier, once, and through the shared wrapper"
    );
}

fn relative_slashed(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[test]
fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold() {
    const FOLD_ENTRIES: &[&str] = &["TopologyFold::replay(", "TopologyFold::parse_log("];

    const FOLD_MENTIONS: &[&str] = &[
        "src/engine/topology/candidate.rs",
        "src/engine/topology/create.rs",
        "src/engine/topology/emit.rs",
        "src/engine/topology/recover.rs",
        "src/engine/topology/run.rs",
        "src/engine/topology/select.rs",
        "src/engine/topology/settle.rs",
        "src/events/log.rs",
        "src/topology/census.rs",
        "src/topology/fold.rs",
        "src/topology/fold/parse.rs",
        "src/topology/fold/predicates.rs",
        "src/topology/fold/start.rs",
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let funnel = src.join("events").join("log.rs");

    let mut files = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("src is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();

    let test_modules: BTreeSet<PathBuf> =
        crate::effects::census_domain::whole_file_test_modules(&src, &files, 13);
    assert!(
        test_modules.contains(&src.join("events").join("log").join("tests.rs")),
        "this file is declared `#[cfg(test)] mod tests;` and the scan has to know it: {test_modules:?}"
    );

    let mut scanned = 0_usize;
    let mut scanned_bytes = 0_usize;
    let mut mentioning: Vec<String> = Vec::new();
    let mut callers: Vec<(PathBuf, &str, usize)> = Vec::new();
    for path in &files {
        if test_modules.contains(path) {
            continue;
        }
        let source = fs::read_to_string(path).expect("a source file");
        let production = crate::effects::production_code(&source);
        let dense = production
            .as_bytes()
            .iter()
            .filter(|byte| !byte.is_ascii_whitespace())
            .count();
        assert!(
            dense > 0,
            "{}'s region is empty, so it contributes nothing to the counts below",
            path.display()
        );
        scanned += 1;
        scanned_bytes += dense;
        if production.contains("TopologyFold") {
            mentioning.push(relative_slashed(path));
        }
        for entry in FOLD_ENTRIES {
            let count = production.matches(entry).count();
            if count > 0 {
                callers.push((path.clone(), entry, count));
            }
        }
    }

    assert!(scanned > 40, "the walk found only {scanned} source files");
    assert!(
        scanned_bytes > 750_000,
        "the {scanned} regions hold {scanned_bytes} non-whitespace bytes between them, so the \
         counts below are over almost nothing"
    );

    let mut sorted = FOLD_MENTIONS.to_vec();
    sorted.sort_unstable();
    assert_eq!(
        FOLD_MENTIONS,
        &sorted[..],
        "`FOLD_MENTIONS` is compared sorted, so it has to be written sorted"
    );
    let repeated: Vec<&&str> = FOLD_MENTIONS
        .iter()
        .filter(|path| FOLD_MENTIONS.iter().filter(|other| other == path).count() > 1)
        .collect();
    assert!(
        repeated.is_empty(),
        "`FOLD_MENTIONS` names {repeated:?} twice — the shape two merges of the same addition \
         produce, and the one a length comparison would report as a bare number"
    );
    mentioning.sort();
    assert_eq!(
        mentioning,
        FOLD_MENTIONS
            .iter()
            .map(|path| (*path).to_owned())
            .collect::<Vec<_>>(),
        "the control: exactly these production regions name `TopologyFold`. A module missing from \
         the list is one nobody classified; a module in the list that no longer appears means the \
         regions this census scanned are not the ones it thinks they are, and its zero counts \
         below would prove nothing"
    );

    assert_eq!(
        callers,
        FOLD_ENTRIES
            .iter()
            .map(|entry| (funnel.clone(), *entry, 1))
            .collect::<Vec<_>>(),
        "a topology fold is built from a log in exactly one production place, once each"
    );

    let funnel_code =
        crate::effects::production_code(&fs::read_to_string(&funnel).expect("the funnel"));
    let barrier = fn_body(&funnel_code, "pub fn establish_stable_prefix(");
    for below in ["pub fn read_all(", "impl LogTail {"] {
        assert!(
            funnel_code.contains(below),
            "`{below}` left the funnel, so the bound below proves nothing"
        );
        assert!(
            !barrier.contains(below),
            "the barrier slice reaches `{below}`, so it is a suffix of the file rather than \
             the function"
        );
    }
    for entry in FOLD_ENTRIES {
        assert_eq!(
            barrier.matches(entry).count(),
            1,
            "`{entry}` is in the funnel but not in `establish_stable_prefix`"
        );
    }
}

#[test]
fn the_blanking_this_census_depends_on_is_live() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut raw_attributes = 0_usize;
    let mut code_attributes = 0_usize;
    let mut raw_folds = 0_usize;
    let mut code_folds = 0_usize;
    let mut files = 0_usize;
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("src is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path.extension().is_some_and(|ext| ext == "rs") {
                continue;
            }
            let source = fs::read_to_string(&path).expect("a source file");
            let blanked = crate::effects::blank_comments_and_strings(&source);
            files += 1;
            raw_attributes += source.matches("#[cfg(test)]").count();
            code_attributes += blanked.matches("#[cfg(test)]").count();
            raw_folds += source.matches("TopologyFold").count();
            code_folds += crate::effects::production_code(&source)
                .matches("TopologyFold")
                .count();
        }
    }
    assert!(files > 40, "the walk found only {files} source files");
    assert!(
        code_attributes < raw_attributes,
        "the tree names `#[cfg(test)]` {raw_attributes} times and the blanking left \
         {code_attributes} of them; a `#[cfg(test)]` quoted in prose is what collapses a \
         production region to nothing"
    );
    assert!(
        code_folds < raw_folds,
        "the tree names `TopologyFold` {raw_folds} times and the production regions hold \
         {code_folds}; the census above counts this token, so a blanking that removed no \
         prose would be counting sentences"
    );
    assert!(
        code_folds > 0,
        "and it removed everything, which is the other way for a census to prove nothing"
    );
}

fn fn_body(source: &str, header: &str) -> String {
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("`{header}` is still here"));
    let mut depth = 0_usize;
    let mut opened = false;
    for (offset, byte) in source.as_bytes().iter().enumerate().skip(start) {
        match byte {
            b'{' => {
                depth += 1;
                opened = true;
            }
            b'}' => {
                assert!(opened, "`{header}` closes a brace it never opened");
                depth -= 1;
                if depth == 0 {
                    return source[start..=offset].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("`{header}` never closes its body");
}

fn production_region(source: &str) -> &str {
    let end = source
        .find("#[cfg(test)]")
        .expect("the funnel declares its test submodules");
    &source[..end]
}

fn strip_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug)]
struct BuildRefusal {
    code: String,
    line: usize,
    body: String,
}

fn declared_build_refusals() -> Vec<BuildRefusal> {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/internals/events/log.md"),
    )
    .expect("the funnel's notes");
    let mut refusals = Vec::new();
    let mut open: Option<BuildRefusal> = None;
    for (index, doc) in source.lines().enumerate() {
        if let Some(refusal) = open.as_mut() {
            if doc.trim_end() == "```" {
                refusals.push(open.take().expect("a block is open"));
            } else {
                refusal.body.push_str(doc);
                refusal.body.push('\n');
            }
            continue;
        }
        if let Some(info) = doc.trim_end().strip_prefix("```compile_fail") {
            let code = info.trim_start_matches(',').trim().to_owned();
            assert!(
                code.len() == 5 && code.starts_with('E') && code[1..].chars().all(char::is_numeric),
                "a compile_fail fence at docs/internals/events/log.md:{} declares `{code}`, which is not an \
                 error code — a fence with no code is green whether it failed for the intended \
                 reason or a typo",
                index + 1
            );
            open = Some(BuildRefusal {
                code,
                line: index + 1,
                body: String::new(),
            });
        }
    }
    assert!(
        open.is_none(),
        "an unterminated compile_fail block in docs/internals/events/log.md"
    );
    refusals
}

fn crate_under_test() -> (PathBuf, PathBuf) {
    let exe = std::env::current_exe().expect("the test executable");
    let deps = exe
        .parent()
        .expect("the test executable is in a directory")
        .to_path_buf();
    let mut rlibs: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(&deps)
        .expect("the deps directory")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            (name.starts_with("libupstroke-") && name.ends_with(".rlib")).then(|| {
                let stamp = path
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                (stamp, path)
            })
        })
        .collect();
    rlibs.sort();
    let rlib = rlibs
        .pop()
        .unwrap_or_else(|| {
            panic!(
                "no libupstroke-*.rlib beside the test executable in {}",
                deps.display()
            )
        })
        .1;
    (deps, rlib)
}

fn typecheck(dir: &Path, name: &str, body: &str) -> (bool, String) {
    let (deps, rlib) = crate_under_test();
    let source = dir.join(format!("{name}.rs"));
    fs::write(&source, format!("fn main() {{\n{body}\n}}\n")).expect("the fixture");
    let out = dir.join(format!("{name}-out"));
    fs::create_dir_all(&out).expect("an output directory");
    let output = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "bin",
            "--emit=metadata",
        ])
        .arg("--out-dir")
        .arg(&out)
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("--extern")
        .arg(format!("upstroke={}", rlib.display()))
        .arg(&source)
        .output()
        .expect("rustc runs; it is the compiler that built this test");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn error_codes(stderr: &str) -> BTreeSet<String> {
    stderr
        .match_indices("error[")
        .filter_map(|(at, _)| {
            let rest = &stderr[at + "error[".len()..];
            let end = rest.find(']')?;
            Some(rest[..end].to_owned())
        })
        .collect()
}

#[test]
fn every_declared_build_refusal_fails_for_the_reason_it_declares() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("the manifest");
    assert!(
        manifest.contains("edition = \"2024\""),
        "this harness compiles its fixtures at edition 2024 and the crate no longer is"
    );

    let dir = scratch("build-refusal");

    let (control_ok, control_stderr) = typecheck(
        &dir,
        "control",
        "use std::path::Path;\n\
         use upstroke::events::{EventLog, LogTail, read_all};\n\
         use upstroke::topology::effects::EventSite;\n\
         let mut warnings = Vec::new();\n\
         let log = EventLog::open(EventSite::OpenLog, Path::new(\"events.jsonl\"), &mut warnings)\n\
         .expect(\"open\");\n\
         let _ = log.path();\n\
         let _ = read_all(Path::new(\"events.jsonl\"), &mut warnings);\n\
         let _ = LogTail::new(Path::new(\"events.jsonl\").to_path_buf());\n",
    );
    assert!(
        control_ok,
        "the control did not compile. Either this harness cannot tell a refusal from a broken \
         invocation, or a public path an external caller names has moved:\n{control_stderr}"
    );

    let refusals = declared_build_refusals();
    assert_eq!(
        refusals.len(),
        3,
        "three build-refusal fixtures: the private handle, the schema-4 event handed to the \
         schema-1..3 append, and the un-round-tripped line. Found {:?}",
        refusals.iter().map(|r| &r.code).collect::<Vec<_>>()
    );
    assert_eq!(
        refusals
            .iter()
            .map(|refusal| refusal.code.clone())
            .collect::<BTreeSet<_>>()
            .len(),
        3,
        "three fixtures, three distinct reasons; two that fail the same way test one thing twice"
    );

    for refusal in &refusals {
        let name = format!("refusal-{}", refusal.code);
        let (compiled, stderr) = typecheck(&dir, &name, &refusal.body);
        assert!(
            !compiled,
            "docs/internals/events/log.md:{} declares `{}` and the fixture compiled",
            refusal.line, refusal.code
        );
        assert_eq!(
            error_codes(&stderr),
            BTreeSet::from([refusal.code.clone()]),
            "docs/internals/events/log.md:{} must fail with exactly `{}` — anything else means it failed for \
             a reason the fixture is not about:\n{stderr}",
            refusal.line,
            refusal.code
        );
    }
}

#[test]
fn the_legacy_engine_reports_and_stops_on_a_returned_append_error() {
    let coordinator = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine/coordinator.rs");
    let source = fs::read_to_string(&coordinator).expect("the coordinator");
    let code = strip_comments(&source);
    assert!(
        code.len() < source.len(),
        "the comment strip removed nothing"
    );

    let branch = code
        .split_once("if let Err(error) = settlement {")
        .expect("the settlement append-error branch is still there")
        .1;
    let branch = &branch[..branch
        .find("if let Some(question) = parking_question")
        .expect("the branch ends where the next statement begins")];
    assert!(
        branch.contains("return Err(error)"),
        "the branch must still report the append's own error"
    );
    assert!(
        !branch.contains(".emit("),
        "the branch must not append anything after a returned append error: {branch}"
    );
    let squeezed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(
        squeezed.matches("self.log.append_hooked(").count(),
        1,
        "the engine has exactly one place that appends"
    );
    assert_eq!(
        squeezed.matches("self.log.append(").count(),
        0,
        "…and it goes through the observer, or a live run's append cannot be made \
         to fail and both of PR5-CONF-010/-011 come back"
    );
}
