use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::agent::proc::ProcessOutput;
use crate::events::log::{EventLog, TopologyLine};
use crate::events::{ChainSummary, GateSummary, ReviewPassOutcome, ReviewRecord, RunOutcome};
use crate::gates::ShellKind;
use crate::ir::{
    Artifact, ArtifactId, Effort, Plan, PlanSource, QuestionId, QuestionKind, ResolvedEffortPolicy,
    Task, TaskId, TaskKind, Tier,
};
use crate::ladder::Next;
use crate::ladder::{FailureKind, FailureOrigin};
use crate::review::{PassBinding, ReviewPlan};
use crate::rundir::{self, RunPaths};
use crate::runner::host::{HostEnvironment, HostRunner, KeyCase};
use crate::runner::{CommandSpec, Runner, gate_request};
use crate::topology::effects::{
    EffectSiteId, EventSite, HookHarness, HookPhase, Injection, InjectionMode, RunDirSite,
    SubEffectPoint,
};
use crate::topology::events::{
    CommitSha, DerivedOutcome, GitRef, ImageIdentity, IncarnationId, LeaseDisposition, LeaseGrant,
    RunResumed4, RunStarted4, RunnerContract, RunnerKind, RunnerPolicy, TaskDispatched,
    TopologyEvent, TopologyEventBody, TopologyLimits,
};
use crate::topology::fold::{FrozenInputs, GenerationClass, TaskState};
use crate::topology::paths::{GitPath, PathGrammar, PathPolicy, PathPolicyVersion, PathSet};
use crate::topology::registry::TaskRegistry;
use crate::topology::schema::TOPOLOGY_SCHEMA;

use super::super::seams::{HarnessTopologyHooks, TopologyHooks};

// -----------------------------------------------------------------------
// Fixtures
//
// Written for this lane rather than shared with the fold's or the census's:
// a settlement that explored the fixture the transition table was built
// against would agree with it about a shape neither had questioned.
//
// Three tasks with **no dependencies between them**, over three disjoint
// regions. That is what lets an eligible integration, a `ready_retry` and a
// `ready` dispatch all be true at once, which is the only state in which
// `eligibility_order` says anything at all.
// -----------------------------------------------------------------------

pub(crate) const ALEPH: TaskKey = TaskKey(0);
pub(crate) const BET: TaskKey = TaskKey(1);
pub(crate) const GIMEL: TaskKey = TaskKey(2);

const RUN_ID: &str = "01SETTLE00000000000000000H";
const NORMALIZED_DIGEST: &str =
    "sha256:7171717171717171717171717171717171717171717171717171717171717171";

pub(crate) fn label(key: TaskKey) -> &'static str {
    match key {
        ALEPH => "aleph",
        BET => "bet",
        _ => "gimel",
    }
}

/// A 40-character symbolic sha, one per role, with no shared prefix.
pub(crate) fn sha(role: &str) -> CommitSha {
    let mut value = format!("{role:z<40}");
    value.truncate(40);
    CommitSha(value)
}

fn git_ref(name: &str) -> GitRef {
    GitRef(format!("refs/upstroke/settle/{RUN_ID}/{name}"))
}

fn task_of(id: &str, hint: &str, tier: Tier) -> Task {
    Task {
        id: TaskId::from(id),
        kind: TaskKind::Refactor,
        title: format!("  {id} — Ünicode  "),
        body: format!("{id} body"),
        depends_on: Vec::new(),
        acceptance: vec![format!("{id} holds")],
        path_hints: vec![hint.to_owned()],
        suggested_tier: Some(tier),
        min_tier: None,
        artifacts_in: Vec::new(),
        artifacts_out: vec![ArtifactId::from(format!("{id}-out").as_str())],
    }
}

pub(crate) fn plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "settle-frozen-hash".to_owned(),
        },
        tasks: vec![
            task_of("aleph", "src/aleph/", Tier::Mid),
            task_of("bet", "src/bet/", Tier::Small),
            task_of("gimel", "src/gimel/", Tier::Small),
        ],
        artifacts: vec![Artifact {
            id: ArtifactId::from("aleph-out"),
            produced_by: Some(TaskId::from("aleph")),
        }],
    }
}

fn chain(task: &str) -> ChainSummary {
    // Two rungs for `aleph` so an escalation has somewhere to go, one for
    // the others so the top rung is reachable in a single failure.
    let tiers = if task == "aleph" {
        vec![Tier::Mid, Tier::Frontier]
    } else {
        vec![Tier::Small]
    };
    ChainSummary {
        task: task.to_owned(),
        attempts_per: 2,
        bindings: Some(
            tiers
                .iter()
                .map(|tier| crate::events::BindingSummary {
                    tier: *tier,
                    agent: format!("{task}-{tier}-agent"),
                    model: format!("{task}-{tier}-model"),
                    pinned: *tier == Tier::Frontier,
                })
                .collect(),
        ),
        tiers,
    }
}

fn path_policy() -> PathPolicy {
    PathPolicy {
        version: PathPolicyVersion::V1,
        case_fold: true,
        grammar: PathGrammar::Globset,
    }
}

fn probed_agents() -> Vec<String> {
    vec![
        "aleph-Mid-agent".to_owned(),
        "aleph-Frontier-agent".to_owned(),
        "bet-Small-agent".to_owned(),
        "gimel-Small-agent".to_owned(),
    ]
}

fn runner_policy() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Host,
        policy: RunnerContract::HostV1,
        image: None,
        credential_volumes: None,
    }
}

fn container_runner_policy() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Container,
        policy: RunnerContract::ContainerV1,
        image: Some(ImageIdentity {
            reference: "ghcr.io/example/settle:9.1".to_owned(),
            id: "sha256:9999999999999999999999999999999999999999999999999999999999999999"
                .to_owned(),
            digest: None,
        }),
        credential_volumes: None,
    }
}

fn run_started_unauthenticated() -> RunStarted4 {
    RunStarted4 {
        schema: TOPOLOGY_SCHEMA,
        upstroke_version: "0.2.0-settle".to_owned(),
        run_id: RUN_ID.to_owned(),
        incarnation: IncarnationId("01SETTLEINCARNATION0000001".to_owned()),
        runner: runner_policy(),
        probed_agents: probed_agents(),
        branch: format!("upstroke/run-{RUN_ID}"),
        integration_ref: git_ref("integration"),
        base_sha: sha("base"),
        execution_root: "/var/lib/Upstroke/settle roots".to_owned(),
        private_dir: "/var/lib/Upstroke/settle private".to_owned(),
        plan_path: "docs/Settle Plan.md".to_owned(),
        config_path: None,
        plan_hash: "settle-frozen-hash".to_owned(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
        registry_digest: String::new(),
        path_policy: path_policy(),
        // `max_parallel` is 3 so that the pipeline entitlement never
        // decides an eligibility-order question: a test that ordered
        // integration ahead of a dispatch because the *ceiling on
        // parallelism* excluded the dispatch would prove nothing about
        // `eligibility_order`.
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
            timeout: Duration::from_secs(211),
            shell: ShellKind::Bash,
        }],
        interaction_mode: "never".to_owned(),
        chains: vec![chain("aleph"), chain("bet"), chain("gimel")],
        effort_policy: ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::High,
            frontier: Effort::Max,
            review: Effort::Medium,
        },
        reviews: ReviewPlan {
            // Enabled: this fixture's successful attempts record a passed
            // `review` pass, and a run that froze verification off obliges
            // none. The two together were a shape production cannot write.
            enabled: Some(true),
            alternative_available: Some(false),
            pass_timeout_secs: Some(89),
            primary: Some(PassBinding::new("aleph-Mid-agent", "aleph-Mid-model")),
            alternative: None,
            // One entry per task: the registry refuses a plan whose
            // second-opinion list is not aligned with `plan.tasks`, and
            // this fixture's plan has three.
            second_opinion: vec![None, None, None],
        },
    }
}

pub(crate) fn run_started() -> RunStarted4 {
    let started = run_started_unauthenticated();
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

pub(crate) fn inputs() -> FrozenInputs {
    FrozenInputs {
        plan: plan(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
    }
}

pub(crate) fn ev(body: TopologyEventBody) -> TopologyEvent {
    TopologyEvent {
        ts: "2026-08-23T11:22:33Z".to_owned(),
        body,
    }
}

/// Apply `event`, refusing to continue if the fold does not accept it: a
/// fixture that silently skipped an event would put every later assertion
/// on a state nobody built.
pub(crate) fn apply(fold: &mut TopologyFold, event: &TopologyEvent) {
    let delta = fold
        .plan_transition(event)
        .unwrap_or_else(|error| panic!("the fixture's `{}` applies: {error}", event.body.kind()));
    fold.apply_delta(delta);
}

/// A fold that has recorded its `run_started` and nothing else.
pub(crate) fn started() -> TopologyFold {
    let mut fold = TopologyFold::new(inputs());
    apply(
        &mut fold,
        &ev(TopologyEventBody::RunStarted {
            data: Box::new(run_started()),
        }),
    );
    fold
}

/// The region an ordinary dispatch of `key` predicts.
///
/// The frozen hint is `src/{label}/`; the derivation trims the trailing
/// separator, and this is the derivation rather than the hint. The two
/// spellings are one region to `paths_overlap`, which is why the fixture
/// could carry the wrong one until `check_dispatched` began comparing the
/// recorded region against the derived one.
pub(crate) fn region(key: TaskKey) -> PathSet {
    PathSet::Prefixes {
        paths: vec![GitPath::from(format!("src/{}", label(key)).as_str())],
    }
}

pub(crate) fn dispatch(key: TaskKey, generation: u32) -> TopologyEvent {
    ev(TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key,
            generation: GenerationId(generation),
            base_sha: sha("base"),
            worktree_path: format!("/tmp/settle/{}", label(key)),
            lease: LeaseGrant::Predicted { paths: region(key) },
            source_candidate: None,
        },
    })
}

pub(crate) fn binding(fold: &TopologyFold, key: TaskKey, rung: usize) -> RungBinding {
    let registry = fold.registry().expect("started");
    let entry = registry.get(key).expect("a registered task");
    let frozen = &entry.ladder.rungs[rung];
    RungBinding::from_frozen(frozen, entry.ladder.effort.implementation_for(frozen.tier))
}

pub(crate) fn attempt_started(
    fold: &TopologyFold,
    key: TaskKey,
    generation: u32,
    attempt: u32,
) -> TopologyEvent {
    ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key,
            generation: GenerationId(generation),
            attempt: AttemptNumber(attempt),
            rung: 0,
            binding: binding(fold, key, 0),
            pool: None,
            resume_session: None,
            materialization_observed: None,
        },
    })
}

pub(crate) fn record(attempt: u32, cost: Option<f64>) -> AttemptRecord {
    record_failing(attempt, cost, None)
}

/// A record of an attempt that failed the way `failure` says.
///
/// **A settlement of a failure whose record carries none is a fixture that
/// cannot happen.** `record`'s `failure: None` means "the work was judged
/// and accepted", and every `settle_failed` case is by definition not that.
/// The allowance is decided from this field, so a grid that varied `Next`
/// and left the failure fixed varied one half of a correlated pair — the
/// class `reviews/FINDINGS.md` §4 records eleven of.
pub(crate) fn record_failing(
    attempt: u32,
    cost: Option<f64>,
    failure: Option<(FailureKind, FailureOrigin)>,
) -> AttemptRecord {
    AttemptRecord {
        attempt,
        tier: "mid".to_owned(),
        model: "aleph-Mid-model".to_owned(),
        pool: None,
        resumed: false,
        duration: Duration::from_millis(3_141),
        cost_usd: cost,
        // A success premise carries the primary pass §11.2 requires; an
        // empty list satisfies `is_successful` vacuously and witnesses
        // nothing about its review clause. A gate failure never reached a
        // reviewer, so the failing variant's list is empty because it is —
        // not for want of a fixture.
        reviews: if failure.is_none() {
            vec![ReviewRecord {
                pass: "review".to_owned(),
                agent: "claude-code".to_owned(),
                model: "claude-opus-5".to_owned(),
                adapter: Some("claude-code".to_owned()),
                preflight_cli_version: None,
                effort: None,
                pool: None,
                cost_usd: None,
                outcome: ReviewPassOutcome::Passed,
            }]
        } else {
            Vec::new()
        },
        session_id: None,
        usage: None,
        failure: failure.map(|(kind, origin)| FailureRecord {
            kind,
            origin,
            reason: "the fixture's failure".to_owned(),
            detail: None,
        }),
    }
}

pub(crate) fn question_for(key: TaskKey) -> FrozenQuestion {
    FrozenQuestion {
        id: QuestionId(format!("q-{}-park", label(key))),
        key,
        kind: QuestionKind::Unblock,
        context: format!("Every rung of {} failed on the same assertion.", label(key)),
        options: vec!["retry on frontier".to_owned(), "skip the task".to_owned()],
    }
}

/// A `FinishedAttempt` with every field at a value of its own, so a
/// settlement that read one field where it meant another lands somewhere
/// this fixture does not hold.
pub(crate) fn finished(key: TaskKey, generation: u32, attempt: u32, next: Next) -> FinishedAttempt {
    FinishedAttempt {
        key,
        generation: GenerationId(generation),
        attempt: AttemptNumber(attempt),
        // **A failed settlement's record says failed.** This used
        // `record(attempt, Some(0.5))`, whose `failure: None` means "the work
        // was judged and accepted" — the very shape the comment on
        // `record_failing` calls "a fixture that cannot happen", two hundred
        // lines above. `check_attempt_finished` refuses it since 2026-08-27.
        record: record_failing(
            attempt,
            Some(0.5),
            Some((FailureKind::GateFailed, FailureOrigin::Worker)),
        ),
        next,
        session: None,
        question: None,
        halts_run: false,
        defers: 4,
        reason: format!("{} failed its gates", label(key)),
        rung: 1,
    }
}

/// Dispatch `key` into generation `generation` and start attempt 1.
pub(crate) fn in_flight(fold: &mut TopologyFold, key: TaskKey, generation: u32) {
    apply(fold, &dispatch(key, generation));
    let event = attempt_started(fold, key, generation, 1);
    apply(fold, &event);
}

/// Settle `key`'s in-flight attempt through the module under test.
pub(crate) fn settle_into(fold: &mut TopologyFold, finished: &FinishedAttempt) -> AttemptFinished4 {
    let settled = settle_failed(fold, finished).expect("the fixture settles");
    apply(
        fold,
        &ev(TopologyEventBody::AttemptFinished {
            data: Box::new(settled.event.clone()),
        }),
    );
    settled.event
}

/// Give `request` a session to resume — in **both** places the attempt
/// carries one.
///
/// `FinishedAttempt` holds the id twice: on `session`, which the settlement
/// records, and on `record.session_id`, which the ledger line reports.
/// Production fills both from `assessed.outcome.session_id`, so they are
/// one value there; a fixture that set only the first would build a
/// retained settlement whose two halves name different conversations, which
/// `check_attempt_finished` refuses.
pub(crate) fn resuming(request: &mut FinishedAttempt, session: &SessionId) {
    request.session = Some(session.clone());
    request.record.session_id = Some(session.0.clone());
}

/// A retained generation of `key`, held by the current epoch.
pub(crate) fn retained_generation(
    fold: &mut TopologyFold,
    key: TaskKey,
    generation: u32,
) -> SessionId {
    in_flight(fold, key, generation);
    let session = SessionId(format!("sess-{}-{generation}", label(key)));
    let mut request = finished(key, generation, 1, Next::RetrySameRung { resume: true });
    resuming(&mut request, &session);
    settle_into(fold, &request);
    session
}

pub(crate) fn resume_event() -> TopologyEvent {
    ev(TopologyEventBody::RunResumed {
        data: Box::new(RunResumed4 {
            incarnation: IncarnationId("01SETTLEINCARNATION0000002".to_owned()),
            runner: runner_policy(),
            probed_agents: probed_agents(),
            upstroke_version: "0.2.0-settle".to_owned(),
        }),
    })
}

// -----------------------------------------------------------------------
// A verify double
// -----------------------------------------------------------------------

/// A [`WorktreeVerify`] whose answer the test fixes.
///
/// The seam exists because this module may name neither
/// `std::process::Command` nor raw `std::fs`, so no test here can build the
/// repository [`ManagedWorktrees`] is derived from. It records what it was
/// asked, so a test can assert that the retry verified **the retained
/// cumulative tree** rather than the base — the one distinction a double
/// that only answered yes or no would lose.
pub(crate) struct FixedVerify {
    answer: Result<(), VerifyFailure>,
    asked: Mutex<Vec<(Slot, Quiescence)>>,
}

impl FixedVerify {
    pub(crate) fn passing() -> Self {
        Self {
            answer: Ok(()),
            asked: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn failing(failure: VerifyFailure) -> Self {
        Self {
            answer: Err(failure),
            asked: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn asked(&self) -> Vec<(Slot, Quiescence)> {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl WorktreeVerify for FixedVerify {
    fn verify(
        &self,
        _hooks: &mut dyn EffectHooks,
        slot: &Slot,
        expected: &Quiescence,
    ) -> Result<Result<(), VerifyFailure>, UpstrokeError> {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((slot.clone(), expected.clone()));
        Ok(self.answer.clone())
    }
}

pub(crate) fn retry_request(key: TaskKey, generation: u32) -> RetryRequest {
    RetryRequest {
        key,
        slot: Slot::Task {
            key: key.0.to_string(),
            generation,
        },
        retained_tree: sha("cumulative-tree").0,
        binding: binding(&started(), key, 0),
        rung: 0,
        pool: None,
        materialization: None,
    }
}

/// A sleeper that records rather than sleeps.
#[derive(Debug, Default)]
pub(crate) struct Recorded(Mutex<Vec<Duration>>);

impl Recorded {
    pub(crate) fn waits(&self) -> Vec<Duration> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Sleeper for Recorded {
    fn sleep(&self, duration: Duration) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(duration);
    }
}

// =======================================================================
// T-FAILED
// =======================================================================

/// Every settlement the ladder can decide, mapped once.
///
/// A grid rather than six tests because the property is that the six
/// answers are **different**: a mapping that collapsed two of them would
/// pass any single-case test.
#[test]
fn each_ladder_decision_maps_to_its_own_settlement() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);

    // **Each decision beside the failure that produces it.** The allowance
    // is a function of the failure, not of the decision, so a grid that
    // varied `Next` against one fixed record would be asserting a mapping
    // that no `next_step` can reach — and would have kept passing while
    // `settle_failed` derived the allowance from the wrong field.
    let cases: Vec<(
        Next,
        (FailureKind, FailureOrigin),
        SettlementTransition,
        bool,
    )> = vec![
        (
            Next::RetrySameRung { resume: false },
            (FailureKind::GateFailed, FailureOrigin::Worker),
            SettlementTransition::Retry,
            true,
        ),
        (
            Next::Escalate,
            (FailureKind::GateFailed, FailureOrigin::Worker),
            SettlementTransition::Escalated { rung: 1 },
            true,
        ),
        (
            // The one deferral: an outage. `next_step` defers precisely so
            // that a busy pool does not burn an attempt.
            Next::Defer,
            (FailureKind::RateLimited, FailureOrigin::Worker),
            SettlementTransition::Deferred {
                // 4 recorded + this one.
                defers: 5,
                reason: "aleph failed its gates".to_owned(),
            },
            false,
        ),
        (
            // **This cell is the defect this grid now catches.** A park
            // from `NeedsHuman` spends nothing — "the code was never
            // judged, so nothing is spent and nothing escalates" — and the
            // settlement used to answer `true` here, because `AskHuman` is
            // not `Defer` and that was the whole of its rule.
            Next::AskHuman(QuestionKind::Unblock),
            (FailureKind::NeedsHuman, FailureOrigin::Worker),
            SettlementTransition::Parked {
                question: question_for(ALEPH),
            },
            false,
        ),
        (
            Next::Fail,
            (FailureKind::GateFailed, FailureOrigin::Worker),
            SettlementTransition::Failed {
                halts_run: false,
                reason: "aleph failed its gates".to_owned(),
            },
            true,
        ),
    ];

    for (next, failure, expected, spends) in cases {
        let mut request = finished(ALEPH, 0, 1, next);
        request.record = record_failing(1, Some(0.5), Some(failure));
        request.question = Some(question_for(ALEPH));
        let settled = settle_failed(&fold, &request).expect("settles");
        assert_eq!(
            settled.event.settlement,
            AttemptSettlement::Closed {
                transition: expected.clone(),
                // An ordinary generation that closes releases its
                // predicted region.
                lease: LeaseDisposition::PredictedReleased,
            },
            "{next:?} settled wrongly"
        );
        assert_eq!(
            settled.spent_attempt, spends,
            "{next:?} from {failure:?} decided the allowance wrongly. The rule is \
             `ladder::spends_allowance`'s and not this module's: an attempt spends iff \
             the worker ran and produced work to judge"
        );
    }

    // `RetrySameRung { resume: true }` with a session is the one that does
    // *not* close, and it is the only one that records an incarnation.
    let mut request = finished(ALEPH, 0, 1, Next::RetrySameRung { resume: true });
    resuming(&mut request, &SessionId("sess-aleph".to_owned()));
    let settled = settle_failed(&fold, &request).expect("settles");
    assert_eq!(
        settled.event.settlement,
        AttemptSettlement::Retained {
            retained_session: SessionId("sess-aleph".to_owned()),
            retained_incarnation: Epoch(0),
        }
    );

    // The ladder's permission without a session closes: there is nothing
    // to resume, so the retry starts a fresh generation.
    let sessionless = finished(ALEPH, 0, 1, Next::RetrySameRung { resume: true });
    let settled = settle_failed(&fold, &sessionless).expect("settles");
    assert!(matches!(
        settled.event.settlement,
        AttemptSettlement::Closed {
            transition: SettlementTransition::Retry,
            ..
        }
    ));
}

/// A repair's settlement never releases the lineage lease, and the
/// disposition is read from the fold rather than restated.
#[test]
fn the_lease_disposition_is_the_generations_own() {
    let mut fold = started();
    in_flight(&mut fold, BET, 0);
    let settled = settle_failed(&fold, &finished(BET, 0, 1, Next::Fail)).expect("settles");
    let AttemptSettlement::Closed { lease, .. } = settled.event.settlement else {
        panic!("a failure closes");
    };
    assert_eq!(lease, LeaseDisposition::PredictedReleased);
    // The fold is the authority: `check_lease_disposition` refuses any
    // other answer, so the settlement applying is the assertion that this
    // one came from `GenerationLease::expected` and not from a constant.
    apply(
        &mut fold,
        &ev(TopologyEventBody::AttemptFinished {
            data: Box::new(settled.event),
        }),
    );
    assert_eq!(fold.task_state(BET), Some(TaskState::Failed));
}

/// **`candidate_prepared` is the successful settlement, and the fold refuses
/// either half of the pair that used to stand in for it.**
///
/// Re-derived from `a_successful_settlement_promotes_the_generation_and_keeps_its_region`,
/// which asserted that an `attempt_finished{Succeeded}` promotes the generation
/// — the event `decisions/2026-08-12-merge-queue-execution-topology.md` says is
/// "not also emitted for that attempt". The old test was not wrong about the
/// build; it was a witness for a shape the record forbids, and re-deriving it
/// against the invariant is the point of the 2026-08-27 CONFORM ruling. It was
/// not patched to pass.
///
/// Three claims, because the invariant has three parts: the settlement lands on
/// `candidate_prepared`; an `attempt_finished` that settles `succeeded` is
/// refused whatever else is true; and a `candidate_prepared` for a generation
/// that is *already* promoted is refused, so neither order of the old pair can
/// be written.
#[test]
fn candidate_prepared_is_the_sole_successful_settlement() {
    use crate::topology::events::CandidatePrepared;

    let prepared_for = |key: TaskKey, generation: u32| CandidatePrepared {
        key,
        generation: GenerationId(generation),
        attempt: Box::new(record(1, Some(0.25))),
        base_sha: sha("base"),
        parent_sha: sha("base"),
        tree_sha: sha("tree"),
        commit_sha: sha("commit"),
        message: "aleph: the judged tree".to_owned(),
        prepared_ref: GitRef("refs/upstroke/prepared/x".to_owned()),
        candidate_ref: GitRef("refs/upstroke/candidates/x".to_owned()),
        actual_paths: PathSet::Prefixes {
            paths: vec![GitPath("src/aleph".to_owned())],
        },
        lease_effect: crate::topology::events::CandidateLeaseEffect::ReplacesPredicted {
            paths: PathSet::Prefixes {
                paths: vec![GitPath("src/aleph".to_owned())],
            },
        },
    };

    // (1) The settlement is this event. An in-flight generation reaches
    //     `Promoting` by applying it, with no `attempt_finished` in between.
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    apply(
        &mut fold,
        &ev(TopologyEventBody::CandidatePrepared {
            data: Box::new(prepared_for(ALEPH, 0)),
        }),
    );
    let promoted = fold
        .task(ALEPH)
        .and_then(|task| task.generations.first())
        .expect("the generation is open");
    assert_eq!(
        promoted.class,
        GenerationClass::Promoting,
        "`candidate_prepared` did not settle the attempt, so nothing did"
    );
    assert!(
        promoted.candidate.is_some(),
        "the settlement recorded no candidate"
    );

    // (2) An `attempt_finished` that settles `succeeded` is refused outright.
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    let refused = fold
        .plan_transition(&ev(TopologyEventBody::AttemptFinished {
            data: Box::new(AttemptFinished4 {
                key: ALEPH,
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                record: Box::new(record(1, Some(0.25))),
                settlement: AttemptSettlement::Closed {
                    transition: SettlementTransition::Succeeded,
                    lease: LeaseDisposition::PredictedRetained,
                },
            }),
        }))
        .expect_err("a succeeded attempt_finished is not a settlement this fold accepts");
    assert!(
        format!("{refused}").contains("sole successful settlement"),
        "the refusal must say why, so a reader is not left guessing: {refused}"
    );

    // (3) And the other order: a generation already promoted may not then
    //     prepare a candidate, so a log carrying both is refused whichever
    //     event it reaches first.
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    apply(
        &mut fold,
        &ev(TopologyEventBody::CandidatePrepared {
            data: Box::new(prepared_for(ALEPH, 0)),
        }),
    );
    let refused = fold
        .plan_transition(&ev(TopologyEventBody::CandidatePrepared {
            data: Box::new(prepared_for(ALEPH, 0)),
        }))
        .expect_err("a promoted generation prepares no second candidate");
    assert!(
        format!("{refused}").contains("still in flight"),
        "the refusal must name the class it required: {refused}"
    );
}

/// `T-FAILED.resume_action`: "rematerialize question from the event …
/// never re-decide".
#[test]
fn a_question_is_read_back_from_the_event_and_not_re_decided() {
    let mut fold = started();
    in_flight(&mut fold, GIMEL, 0);
    let mut request = finished(GIMEL, 0, 1, Next::AskHuman(QuestionKind::Clarify));
    request.question = Some(question_for(GIMEL));
    let event = settle_into(&mut fold, &request);

    assert_eq!(rematerialize_question(&event), Some(&question_for(GIMEL)));
    // The fold opened exactly that question, under exactly that id.
    let open = fold.open_questions().expect("started");
    assert_eq!(open.len(), 1);
    assert_eq!(
        open.get(&question_for(GIMEL).id).map(|held| &held.question),
        Some(&question_for(GIMEL))
    );
    assert_eq!(fold.task_state(GIMEL), Some(TaskState::AwaitingInput));

    // Every other settlement rematerializes nothing: a reader that
    // answered `Some` for a non-parking settlement would write a question
    // payload for a task nobody is waiting on.
    let mut other = started();
    in_flight(&mut other, ALEPH, 0);
    let closed = settle_failed(&other, &finished(ALEPH, 0, 1, Next::Fail)).expect("settles");
    assert_eq!(rematerialize_question(&closed.event), None);
    let mut retaining = finished(ALEPH, 0, 1, Next::RetrySameRung { resume: true });
    resuming(&mut retaining, &SessionId("sess".to_owned()));
    let retained = settle_failed(&other, &retaining).expect("settles");
    assert_eq!(rematerialize_question(&retained.event), None);
}

/// A park that carries no question is refused rather than settled with an
/// invented one.
#[test]
fn a_park_without_a_question_is_refused() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    let request = finished(ALEPH, 0, 1, Next::AskHuman(QuestionKind::Unblock));
    let error = settle_failed(&fold, &request).expect_err("a park records its question");
    assert!(
        format!("{error}").contains("records the question it raised"),
        "{error}"
    );
}

/// A settlement naming a generation that is not the open one is refused
/// before it can be built.
#[test]
fn a_settlement_naming_the_wrong_generation_is_refused() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    let error =
        settle_failed(&fold, &finished(ALEPH, 7, 1, Next::Fail)).expect_err("wrong generation");
    assert!(
        format!("{error}").contains("generation 0 is the open one"),
        "{error}"
    );

    let error = settle_failed(&fold, &finished(BET, 0, 1, Next::Fail)).expect_err("no generation");
    assert!(format!("{error}").contains("no open generation"), "{error}");
}

/// `deferred_task_woken_by_defer_wait_elapsed_or_resume`.
#[test]
fn deferred_task_woken_by_defer_wait_elapsed_or_resume() {
    for wake in ["elapsed", "resume"] {
        let mut fold = started();
        in_flight(&mut fold, ALEPH, 0);
        settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Defer));
        assert_eq!(fold.task_state(ALEPH), Some(TaskState::Deferred));
        assert!(
            !fold.ready(ALEPH),
            "a deferred task is not dispatched while it waits"
        );

        let event = match wake {
            "elapsed" => {
                let mut deferral = Deferral::new(Duration::from_millis(40));
                let sleeper = Recorded::default();
                let elapsed = deferral.wait(&sleeper);
                assert_eq!(
                    sleeper.waits(),
                    vec![Duration::from_millis(40)],
                    "the wait is slept before it is recorded as elapsed"
                );
                assert_eq!(elapsed.round, 1);
                assert_eq!(elapsed.waited_ms, 40);
                ev(TopologyEventBody::DeferWaitElapsed { data: elapsed })
            }
            _ => resume_event(),
        };
        apply(&mut fold, &event);

        assert_eq!(
            fold.task_state(ALEPH),
            Some(TaskState::Pending),
            "{wake} did not wake the deferred task"
        );
        assert!(fold.ready(ALEPH), "{wake} left it unready");
    }
}

/// The backoff doubles and is capped, and progress resets it.
#[test]
fn the_defer_backoff_doubles_caps_and_resets() {
    let sleeper = Recorded::default();
    let mut deferral = Deferral::new(Duration::from_secs(60));
    let mut recorded: Vec<u32> = Vec::new();
    for _ in 0..12 {
        recorded.push(deferral.wait(&sleeper).round);
    }
    let waits = sleeper.waits();
    assert_eq!(waits[0], Duration::from_secs(60));
    assert_eq!(waits[1], Duration::from_secs(120));
    assert_eq!(
        *waits.last().expect("twelve waits"),
        crate::interaction::MAX_DEFER_BACKOFF,
        "an uncapped backoff waits longer than asking a human is worth"
    );
    assert_eq!(deferral.round(), 12);

    // **And what the event carries**, which is a different claim from what
    // the accumulator holds — `reviews/FINDINGS.md` §4's "an accumulator's
    // witness proves the accumulation and not the read", at four
    // occurrences. `DeferWaitElapsed4.round` is documented on the wire, a
    // frontier reviewer reads it there, and until this line nothing asserted
    // the value a run actually writes.
    assert_eq!(
        recorded,
        (1..=12).collect::<Vec<u32>>(),
        "`wait` increments before it records, so the recorded round is one-based"
    );

    deferral.progressed();
    assert_eq!(deferral.round(), 0);
    let sleeper = Recorded::default();
    let after_progress = deferral.wait(&sleeper).round;
    assert_eq!(
        sleeper.waits(),
        vec![Duration::from_secs(60)],
        "progress did not reset the doubling"
    );
    assert_eq!(
        after_progress, 1,
        "the recorded sequence over one run is 1, 2, … 12, **1** — not a count across the \
         run. A reader taking `DeferWaitElapsed4.round` as \"which sleep this was\" reads \
         this thirteenth sleep as the first"
    );
    assert_eq!(
        Deferral::default_backoff().round(),
        0,
        "a fresh backoff has waited nothing"
    );
}

/// `deferred_task_does_not_block_halted_or_budget_exceeded_closure`.
#[test]
fn deferred_task_does_not_block_halted_or_budget_exceeded_closure() {
    // Halted.
    let mut fold = started();
    in_flight(&mut fold, BET, 0);
    settle_into(&mut fold, &finished(BET, 0, 1, Next::Defer));
    in_flight(&mut fold, ALEPH, 0);
    let mut halting = finished(ALEPH, 0, 1, Next::Fail);
    halting.halts_run = true;
    settle_into(&mut fold, &halting);

    assert_eq!(fold.task_state(BET), Some(TaskState::Deferred));
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Halted),
        "a deferred task delayed a halted closure"
    );
    // And the wait it is deferred behind can no longer elapse.
    let refused = fold
        .plan_transition(&ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 1,
                round: 1,
            },
        }))
        .expect_err("halt outranks backoff");
    assert!(
        format!("{refused}").contains("halting settlement"),
        "{refused}"
    );

    // BudgetExceeded.
    let mut fold = started();
    in_flight(&mut fold, BET, 0);
    settle_into(&mut fold, &finished(BET, 0, 1, Next::Defer));
    apply(&mut fold, &budget_exceeded(Epoch(0), ALEPH));
    assert_eq!(fold.task_state(BET), Some(TaskState::Deferred));
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::BudgetExceeded),
        "a deferred task delayed a budget closure"
    );
    let refused = fold
        .plan_transition(&ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 1,
                round: 1,
            },
        }))
        .expect_err("the budget stop outranks backoff");
    assert!(format!("{refused}").contains("budget stop"), "{refused}");
}

pub(crate) fn budget_exceeded(epoch: Epoch, key: TaskKey) -> TopologyEvent {
    ev(TopologyEventBody::BudgetExceeded {
        data: crate::topology::events::BudgetExceeded4 {
            epoch,
            budget: crate::events::BudgetKind::Run,
            limit_usd: 4.0,
            spent_usd: 4.25,
            key: Some(key),
        },
    })
}

/// `halting_settlement_starts_closure`.
#[test]
fn halting_settlement_starts_closure() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    let mut halting = finished(ALEPH, 0, 1, Next::Fail);
    halting.halts_run = true;
    let settled = settle_failed(&fold, &halting).expect("settles");

    assert!(
        settled.event.halts_run(),
        "the settlement did not carry the halt"
    );
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::NotEnding,
        "the run was already ending before the halting settlement"
    );
    apply(
        &mut fold,
        &ev(TopologyEventBody::AttemptFinished {
            data: Box::new(settled.event),
        }),
    );
    assert_eq!(fold.halted_at(), Some(ALEPH));
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Halted)
    );

    // A non-halting terminal failure of the same shape does not: the
    // control that separates "a task failed" from "the run is over".
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Fail));
    assert_eq!(fold.halted_at(), None);
    assert_ne!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Halted)
    );
}

/// `halting_drain_settlement_after_budget_exceeded_yields_halted` (ST-17).
#[test]
fn halting_drain_settlement_after_budget_exceeded_yields_halted() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    // The ceiling refused BET's next attempt; ALEPH's attempt is still in
    // flight and drains.
    apply(&mut fold, &budget_exceeded(Epoch(0), BET));
    assert_eq!(
        fold.budget_stop().map(|stop| stop.epoch),
        Some(Epoch(0)),
        "the stop belongs to the epoch that hit the ceiling"
    );

    let mut halting = finished(ALEPH, 0, 1, Next::Fail);
    halting.halts_run = true;
    settle_into(&mut fold, &halting);

    assert_eq!(fold.halted_at(), Some(ALEPH));
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Halted),
        "a halt that arrived during the budget drain reported the budget instead"
    );
}

// =======================================================================
// T-RETAINED
// =======================================================================

/// `retained_generation_closed_before_run_resumed`.
///
/// The ordering is the whole protection and this test says why: **before**
/// recovery step (e) the fold cannot tell a fresh process from the
/// retaining one, because `retained_incarnation == state.resumes` and a
/// fresh process has not resumed yet. `ready_retry` is therefore *true* at
/// that prefix, and what keeps a fresh process out of the retained session
/// is that (e) runs before (h) and `ready_retry` is never evaluated before
/// (h).
#[test]
fn retained_generation_closed_before_run_resumed() {
    let mut fold = started();
    retained_generation(&mut fold, ALEPH, 0);

    assert!(
        fold.ready_retry(ALEPH),
        "the fold alone cannot tell a fresh process from the retaining one inside one epoch, \
         which is why recovery order rather than the predicate is the protection"
    );

    let closed = close_retained(&fold, &GenerationCloseReason::ResumeDiscardsRetainedSession)
        .expect("recovery step (e) closes it");
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].key, ALEPH);
    assert_eq!(closed[0].generation, GenerationId(0));
    assert_eq!(closed[0].lease, LeaseDisposition::PredictedReleased);
    assert_eq!(
        closed[0].reason,
        GenerationCloseReason::ResumeDiscardsRetainedSession
    );

    apply(
        &mut fold,
        &ev(TopologyEventBody::GenerationClosed {
            data: closed[0].clone(),
        }),
    );
    assert!(!fold.ready_retry(ALEPH), "the closed generation is retried");

    apply(&mut fold, &resume_event());
    assert_eq!(fold.epoch(), Some(Epoch(1)));
    assert!(!fold.ready_retry(ALEPH));
    assert!(
        fold.ready(ALEPH),
        "T-RETAINED: the next dispatch creates a fresh generation"
    );
    // "any attempt to recreate a Retained worktree at base" is refused:
    // the new generation is a *new* one, at the current head.
    let next = dispatch(ALEPH, 1);
    fold.plan_transition(&next).expect("a fresh generation");
    // Nothing is retained any more.
    assert!(
        close_retained(&fold, &GenerationCloseReason::ResumeDiscardsRetainedSession)
            .expect("no retained generations")
            .is_empty()
    );
}

/// `retained_generation_closed_when_worktree_missing` (ST-11).
#[test]
fn retained_generation_closed_when_worktree_missing() {
    let mut fold = started();
    retained_generation(&mut fold, ALEPH, 0);
    let mut reservations = Reservations::new();
    let worktrees = FixedVerify::failing(VerifyFailure::Missing);
    let mut hooks = HarnessTopologyHooks::new(Arc::new(Mutex::new(HookHarness::new())));

    let outcome = retry(
        &fold,
        &mut reservations,
        &worktrees,
        hooks.effects(),
        &retry_request(ALEPH, 0),
    )
    .expect("a missing worktree is a decision, not an error");

    let RetryOutcome::Close { closed, failure } = outcome else {
        panic!("a missing worktree must not start an attempt");
    };
    assert_eq!(failure, VerifyFailure::Missing);
    assert_eq!(closed.reason, GenerationCloseReason::WorktreeMissing);
    assert_eq!(closed.key, ALEPH);
    assert_eq!(closed.generation, GenerationId(0));
    assert!(
        reservations.is_empty() && reservations.balances(),
        "a pre-append failure left the provisional reservation held"
    );

    apply(
        &mut fold,
        &ev(TopologyEventBody::GenerationClosed { data: closed }),
    );
    assert!(!fold.ready_retry(ALEPH));
    assert!(
        fold.ready(ALEPH),
        "the task returns to an ordinary dispatch"
    );
}

/// `retained_generation_closed_at_run_end` (ST-17).
#[test]
fn retained_generation_closed_at_run_end() {
    let mut fold = started();
    retained_generation(&mut fold, ALEPH, 0);
    in_flight(&mut fold, BET, 0);
    let mut halting = finished(BET, 0, 1, Next::Fail);
    halting.halts_run = true;
    settle_into(&mut fold, &halting);

    assert_eq!(fold.halted_at(), Some(BET));
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::NotEnding,
        "the open retained generation is what a run-end closure has to close"
    );

    let closed = close_retained(&fold, &run_ending(RunOutcome::Halted)).expect("closes");
    assert_eq!(closed.len(), 1);
    assert_eq!(
        closed[0].reason,
        GenerationCloseReason::RunEnding {
            outcome: RunOutcome::Halted
        },
        "a run-end closure that reused the resume reason would claim a fresh process"
    );
    apply(
        &mut fold,
        &ev(TopologyEventBody::GenerationClosed {
            data: closed[0].clone(),
        }),
    );
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Halted)
    );
}

/// A generation with an attempt in flight is not closed: `refusals[15]`.
#[test]
fn only_an_idle_generation_is_closed() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    let error = close_generation(&fold, ALEPH, GenerationCloseReason::WorktreeMissing)
        .expect_err("an in-flight generation is settled, not closed");
    assert!(format!("{error}").contains("in flight"), "{error}");

    let error = close_generation(&fold, GIMEL, GenerationCloseReason::WorktreeMissing)
        .expect_err("no open generation");
    assert!(format!("{error}").contains("no open generation"), "{error}");

    // An `OpenNoAttempt` generation *is* closed — the other half of the
    // rule, so a refusal that had swallowed both would fail here.
    let mut fold = started();
    apply(&mut fold, &dispatch(BET, 0));
    let closed = close_generation(&fold, BET, run_ending(RunOutcome::Complete))
        .expect("an open generation with no attempt closes");
    assert_eq!(closed.generation, GenerationId(0));
}

// =======================================================================
// T-RETRY
// =======================================================================

/// `same_generation_retry_regates_cumulative_tree` (ST-15).
#[test]
fn same_generation_retry_regates_cumulative_tree() {
    let mut fold = started();
    let session = retained_generation(&mut fold, ALEPH, 0);
    let mut reservations = Reservations::new();
    let worktrees = FixedVerify::passing();
    let mut hooks = HarnessTopologyHooks::new(Arc::new(Mutex::new(HookHarness::new())));
    let request = retry_request(ALEPH, 0);

    let outcome = retry(
        &fold,
        &mut reservations,
        &worktrees,
        hooks.effects(),
        &request,
    )
    .expect("the retaining incarnation retries in place");

    // The verify asked for the *cumulative tree*, not the base. A retry
    // verified against the base passes on a worktree that was reset, and
    // then re-gates an empty tree as if it were the retained one.
    assert_eq!(
        worktrees.asked(),
        vec![(
            Slot::Task {
                key: "0".to_owned(),
                generation: 0
            },
            Quiescence::HoldsTree(sha("cumulative-tree").0)
        )]
    );

    let RetryOutcome::Start(started_event) = outcome else {
        panic!("a verified worktree starts the attempt");
    };
    assert_eq!(started_event.key, ALEPH);
    assert_eq!(
        started_event.generation,
        GenerationId(0),
        "a retry runs in the generation that retained the session"
    );
    assert_eq!(started_event.attempt, AttemptNumber(2));
    assert_eq!(started_event.resume_session, Some(session));

    // The reservation bridges the selection to the append and is converted
    // at it.
    assert!(!reservations.is_empty(), "the retry took no reservation");
    reservations
        .convert(ALEPH, ReservationKind::Retry)
        .expect("converted at the append");
    assert!(reservations.balances());

    apply(
        &mut fold,
        &ev(TopologyEventBody::AttemptStarted {
            data: *started_event,
        }),
    );
    assert!(matches!(
        fold.task(ALEPH).and_then(|task| task.generations.first()),
        Some(held) if held.class == GenerationClass::InFlight { attempt: AttemptNumber(2) }
    ));
}

/// `retry_refused_after_resume`.
#[test]
fn retry_refused_after_resume() {
    let mut fold = started();
    retained_generation(&mut fold, ALEPH, 0);
    apply(&mut fold, &resume_event());
    assert_eq!(fold.epoch(), Some(Epoch(1)));
    assert!(
        !fold.ready_retry(ALEPH),
        "a resumed run offered the retained session to the new incarnation"
    );

    let mut reservations = Reservations::new();
    let worktrees = FixedVerify::passing();
    let mut hooks = HarnessTopologyHooks::new(Arc::new(Mutex::new(HookHarness::new())));
    let error = retry(
        &fold,
        &mut reservations,
        &worktrees,
        hooks.effects(),
        &retry_request(ALEPH, 0),
    )
    .expect_err("a resumed run may not retry a session it did not retain");
    assert!(
        format!("{error}").contains("retained by incarnation 0")
            && format!("{error}").contains("resumed 1 time(s)"),
        "{error}"
    );
    assert!(
        worktrees.asked().is_empty(),
        "the refusal came after the worktree was already verified"
    );
    assert!(
        reservations.is_empty() && reservations.balances(),
        "the refusal left a provisional reservation held"
    );
}

/// `retry_refused_with_stale_incarnation`.
///
/// Two directions, because the field has two ends. Writing it: a
/// settlement takes `retained_incarnation` from the fold's **epoch**, so a
/// run that has resumed once retains for `Epoch(1)`. Reading it: an
/// `attempt_started` naming an epoch the run has moved past is refused by
/// the fold itself, whatever a caller decided.
#[test]
fn retry_refused_with_stale_incarnation() {
    let mut fold = started();
    apply(&mut fold, &resume_event());
    let session = retained_generation(&mut fold, ALEPH, 0);
    assert!(
        matches!(
            fold.task(ALEPH).and_then(|task| task.generations.first()),
            Some(held)
                if held.class == GenerationClass::RetainedIdle {
                    session: session.clone(),
                    incarnation: Epoch(1),
                }
        ),
        "the settlement wired `retained_incarnation` from something other than the epoch"
    );

    // Reading it: a hand-built retry naming the previous epoch's session.
    let stale = ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: ALEPH,
            generation: GenerationId(0),
            attempt: AttemptNumber(2),
            rung: 0,
            binding: binding(&fold, ALEPH, 0),
            pool: None,
            resume_session: Some(SessionId("sess-from-a-dead-incarnation".to_owned())),
            materialization_observed: None,
        },
    });
    let error = fold
        .plan_transition(&stale)
        .expect_err("the fold refuses a stale-incarnation retry");
    assert!(format!("{error}").contains("retained"), "{error}");

    // And a settlement that claimed an epoch other than the fold's is
    // refused too — the field is checked on the way in as well as out.
    let mut other = started();
    in_flight(&mut other, BET, 0);
    let forged = ev(TopologyEventBody::AttemptFinished {
        data: Box::new(AttemptFinished4 {
            key: BET,
            generation: GenerationId(0),
            attempt: AttemptNumber(1),
            record: Box::new(record(1, Some(0.1))),
            settlement: AttemptSettlement::Retained {
                retained_session: SessionId("sess-bet".to_owned()),
                retained_incarnation: Epoch(9),
            },
        }),
    });
    let error = other
        .plan_transition(&forged)
        .expect_err("a settlement cannot retain for an epoch this run is not in");
    assert!(format!("{error}").contains("resumed 0 time(s)"), "{error}");
}

/// `retained_worktree_with_residue_closed_not_retried`.
#[test]
fn retained_worktree_with_residue_closed_not_retried() {
    for element in [
        crate::topology::effects::ResidueElement::IndexLock,
        crate::topology::effects::ResidueElement::CherryPickHead,
        crate::topology::effects::ResidueElement::SequencerState,
        crate::topology::effects::ResidueElement::RegisteredUnpopulatedWorktree,
    ] {
        let mut fold = started();
        retained_generation(&mut fold, ALEPH, 0);
        let mut reservations = Reservations::new();
        let worktrees = FixedVerify::failing(VerifyFailure::Residue(element));
        let mut hooks = HarnessTopologyHooks::new(Arc::new(Mutex::new(HookHarness::new())));

        let outcome = retry(
            &fold,
            &mut reservations,
            &worktrees,
            hooks.effects(),
            &retry_request(ALEPH, 0),
        )
        .expect("residue is a decision, not an error");

        let RetryOutcome::Close { closed, failure } = outcome else {
            panic!("{element:?}: administrative residue must not be retried into");
        };
        assert_eq!(failure, VerifyFailure::Residue(element));
        assert_eq!(closed.reason, GenerationCloseReason::WorktreeMissing);
        assert!(reservations.balances());

        // INV-06: it is closed, and it is **never recreated** at its base.
        apply(
            &mut fold,
            &ev(TopologyEventBody::GenerationClosed { data: closed }),
        );
        assert!(!fold.ready_retry(ALEPH));
    }
}

/// A retry of a generation that is not retained-idle is refused before it
/// takes a reservation or verifies anything.
#[test]
fn only_a_retained_generation_is_retried_in_place() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    let mut reservations = Reservations::new();
    let worktrees = FixedVerify::passing();
    let mut hooks = HarnessTopologyHooks::new(Arc::new(Mutex::new(HookHarness::new())));
    let error = retry(
        &fold,
        &mut reservations,
        &worktrees,
        hooks.effects(),
        &retry_request(ALEPH, 0),
    )
    .expect_err("an in-flight generation is not retried");
    assert!(format!("{error}").contains("in flight"), "{error}");
    assert!(worktrees.asked().is_empty());
    assert!(reservations.balances());
}

// =======================================================================
// The kill tests
//
// `Injection::Kill` is `std::process::abort()` — a real process death,
// chosen so the claim is *what a coordinator that runs no cleanup leaves
// on disk*. An early `return` would unwind and prove something weaker.
// =======================================================================

static SCRATCH: AtomicU32 = AtomicU32::new(0);

/// How many names [`scratch_in`] tries before giving up. A temp directory
/// carries one leftover per `(process id, label)` for every harness that
/// died holding that id, so reaching this is a directory that needs
/// emptying, and the panic says which.
const SCRATCH_LEFTOVER_BOUND: u32 = 64;

/// The name of this process's `nth` scratch directory for `label`.
fn scratch_name(label: &str, nth: u32) -> String {
    format!("upstroke-pr7h-{label}-{}-{nth}", std::process::id())
}

/// A scratch directory, created through the run-directory funnel because
/// this module may not name `std::fs`.
///
/// The name carries the process id, and a process id is unique only among
/// **live** processes. Nothing here removes a scratch directory — the kill
/// tests' claim is what a coordinator that runs no cleanup leaves on disk
/// — so a harness handed a dead harness's id finds that harness's
/// directory, and the funnel's `create_dir_all` accepts it. The kill child
/// then opens the log the dead harness's child left, appends a second run
/// to it, and `replay` refuses the second `RunStarted` with
/// `AlreadyStarted`: both kill tests fail together, at their
/// `the log replays` expectations, with every earlier assertion passing.
/// Windows hands a freed id out again within hours, and the `test
/// (winguest)` image's temp directory carries one such pair per harness
/// that ever ran on it, so that is what the pair of reds on that leg was.
/// [`scratch_in`] passes an existing name over: no live process shares
/// this id, so an existing name is always a leftover, never a neighbour.
fn scratch(label: &str) -> PathBuf {
    scratch_in(&std::env::temp_dir(), label)
}

/// [`scratch`] under `root`, which is what lets its witness plant leftovers
/// in a root of its own.
fn scratch_in(root: &Path, label: &str) -> PathBuf {
    for _ in 0..SCRATCH_LEFTOVER_BOUND {
        let nth = SCRATCH.fetch_add(1, Ordering::Relaxed);
        let dir = root.join(scratch_name(label, nth));
        if dir.exists() {
            continue;
        }
        rundir::create_public_dir(&dir, &mut rundir::NoHooks).expect("a scratch directory");
        return dir;
    }
    panic!(
        "{SCRATCH_LEFTOVER_BOUND} scratch names for `{label}` under {} already exist for process {}",
        root.display(),
        std::process::id()
    );
}

/// [`scratch_in`] never hands a test a directory a dead process left: a
/// name that already exists under this process's id is passed over, not
/// reused. Without the `exists` check the fixture returns the first planted
/// name, on every platform.
#[test]
fn a_scratch_directory_is_never_a_leftover() {
    let root = scratch("leftover-root");
    // The names this process would choose next, planted as leftovers. Eight,
    // because each of the two kill tests may advance the counter once between
    // the read and the call.
    let next = SCRATCH.load(Ordering::Relaxed);
    let planted: Vec<PathBuf> = (next..next + 8)
        .map(|nth| root.join(scratch_name("leftover", nth)))
        .collect();
    for dir in &planted {
        rundir::create_public_dir(dir, &mut rundir::NoHooks).expect("a leftover");
    }
    let fresh = scratch_in(&root, "leftover");
    assert!(
        !planted.contains(&fresh),
        "the fixture reused a leftover: {}",
        fresh.display()
    );
    assert!(fresh.starts_with(&root));
}

/// A [`RunDirHooks`] that records into the shared harness **and** answers
/// `Kill` at one `(site, phase)`.
///
/// `HookHarness::arm` takes a `SubEffectPoint`, and
/// `RunDir.WriteQuestionPayload` exposes none — so arming its `Before`
/// phase needs a local double. The *recording* still goes to the shared
/// harness, or this site would contribute nothing to the coverage evidence.
struct KillAtPhase {
    inner: rundir::HarnessHooks,
    site: EffectSiteId,
    phase: HookPhase,
}

impl crate::rundir::RunDirHooks for KillAtPhase {
    fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        let recorded = self.inner.hook(site, phase);
        if site == self.site && phase == self.phase {
            return Injection::Kill;
        }
        recorded
    }

    fn durability_ledger(&self) -> crate::util::DurabilityLedger {
        self.inner.durability_ledger()
    }
}

/// Append `event` through the real funnel.
fn append(log: &mut EventLog, hooks: &mut HarnessTopologyHooks, event: &TopologyEvent) {
    let (line, _) = TopologyLine::round_trip(event).expect("the event round-trips");
    log.append_topology_hooked(line.site(), &line, hooks.events())
        .expect("the append lands");
}

/// The child of both kill tests: build a run whose settlement is durable,
/// then die at the boundary the site names.
#[test]
#[ignore = "spawned as a subprocess by the T-FAILED kill tests"]
fn settlement_kill_child() {
    let dir = PathBuf::from(std::env::var("UPSTROKE_TEST_KILL_DIR").expect("dir"));
    let which = std::env::var("UPSTROKE_TEST_KILL_SITE").expect("site");

    let harness = Arc::new(Mutex::new(HookHarness::new()));
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let paths = RunPaths::from_parts(dir.join("public"), dir.join("private"));
    paths
        .create_hooked(hooks.rundir())
        .expect("the run directory");

    let mut warnings = Vec::new();
    let mut log = EventLog::open_hooked(
        EventSite::OpenLog,
        &paths.events(),
        &mut warnings,
        hooks.events(),
    )
    .expect("the log opens");

    let mut fold = TopologyFold::new(inputs());
    for event in [
        ev(TopologyEventBody::RunStarted {
            data: Box::new(run_started()),
        }),
        dispatch(ALEPH, 0),
    ] {
        append(&mut log, &mut hooks, &event);
        apply(&mut fold, &event);
    }
    let event = attempt_started(&fold, ALEPH, 0, 1);
    append(&mut log, &mut hooks, &event);
    apply(&mut fold, &event);

    if which == "retained" {
        // T-APPEND (s): the line is synced and the process dies. The
        // settlement is durable and nothing after it was ever attempted.
        let mut request = finished(ALEPH, 0, 1, Next::RetrySameRung { resume: true });
        resuming(&mut request, &SessionId("sess-aleph-retained".to_owned()));
        let settled = settle_failed(&fold, &request).expect("settles");
        harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .arm(
                EffectSiteId::Event(EventSite::Append),
                SubEffectPoint::Synced,
                InjectionMode::Kill,
            )
            .expect("Event.Append syncs");
        append(
            &mut log,
            &mut hooks,
            &ev(TopologyEventBody::AttemptFinished {
                data: Box::new(settled.event),
            }),
        );
        unreachable!("the kill at Event.Append/Synced must have taken this process");
    }

    // T-FAILED's boundary: the settlement is appended and the question
    // file is not applied.
    let mut request = finished(ALEPH, 0, 1, Next::AskHuman(QuestionKind::Unblock));
    request.question = Some(question_for(ALEPH));
    let settled = settle_failed(&fold, &request).expect("settles");
    let event = ev(TopologyEventBody::AttemptFinished {
        data: Box::new(settled.event.clone()),
    });
    append(&mut log, &mut hooks, &event);
    apply(&mut fold, &event);

    let question = rematerialize_question(&settled.event).expect("a park records its question");
    let mut killer = KillAtPhase {
        inner: rundir::HarnessHooks::new(Arc::clone(&harness)),
        site: EffectSiteId::RunDir(RunDirSite::WriteQuestionPayload),
        phase: HookPhase::Before,
    };
    let _ = rundir::write_question_payload(
        &paths.questions(),
        question.id.as_str(),
        question,
        &mut killer,
    );
    unreachable!("the kill at RunDir.WriteQuestionPayload must have taken this process");
}

/// Spawn [`settlement_kill_child`] through the host Runner and wait for it
/// to die.
///
/// Through the Runner rather than `std::process::Command`, which this
/// module may not name: `Process.Spawn` is the funnel that owns process
/// start, and a test that reached around it would be the exact bypass the
/// denylist exists to prevent.
fn spawn_kill_child(dir: &Path, site: &str) -> ProcessOutput {
    let exe = std::env::current_exe().expect("test executable");
    let mut base: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    base.push((
        OsString::from("UPSTROKE_TEST_KILL_DIR"),
        dir.as_os_str().to_owned(),
    ));
    base.push((
        OsString::from("UPSTROKE_TEST_KILL_SITE"),
        OsString::from(site),
    ));
    let runner =
        HostRunner::new().with_environment(HostEnvironment::with_base(base, KeyCase::current()));
    let command = CommandSpec::new(exe.to_string_lossy().into_owned())
        .arg("--exact")
        .arg("--ignored")
        .arg("--nocapture")
        .arg("engine::topology::settle::tests::settlement_kill_child");
    let identities =
        super::super::identity::AttemptIdentities::new(ALEPH, GenerationId(0), AttemptNumber(1));
    let request = gate_request(
        command,
        dir.to_path_buf(),
        Duration::from_secs(120),
        identities.gate(0, 0),
    );
    let output = runner.run(&request).expect("the child spawns");
    assert_ne!(
        output.code,
        Some(0),
        "the child exited cleanly, so the injection stopped killing: {}{}",
        output.stdout,
        output.stderr
    );
    // The `unreachable!` is what fails this test if the injection silently
    // stopped killing — and it only fails it if the parent looks. A panic
    // and an abort both exit non-zero, so the exit code alone cannot tell
    // "the process died at the injection" from "the process ran past it
    // and panicked one line later".
    assert!(
        !output.stderr.contains("must have taken this process"),
        "the child ran past the injection: {}",
        output.stderr
    );
    output
}

/// Every committed event of the log the child left behind.
fn committed(dir: &Path) -> Vec<TopologyEvent> {
    let bytes = std::fs::read(dir.join("public").join("events.jsonl")).expect("the log");
    TopologyFold::parse_log(&bytes).expect("the log parses")
}

/// `kill_after_failed_settlement_rematerializes_question`.
#[test]
fn kill_after_failed_settlement_rematerializes_question() {
    let dir = scratch("question");
    let output = spawn_kill_child(&dir, "question");

    let payload = dir
        .join("public")
        .join("questions")
        .join(format!("{}.json", question_for(ALEPH).id.as_str()));
    assert!(
        !payload.exists(),
        "the child wrote the question file it was killed before writing: {}{}",
        output.stdout,
        output.stderr
    );

    let events = committed(&dir);
    let last = events.last().expect("the log has lines");
    let TopologyEventBody::AttemptFinished { data } = &last.body else {
        panic!(
            "the settlement is not the last durable line: {:?}",
            last.body
        );
    };

    // Rematerialized from the event, byte for byte, and never re-decided.
    assert_eq!(rematerialize_question(data), Some(&question_for(ALEPH)));

    // And a replay reaches the same open question, which is what makes the
    // answer the operator already wrote answer *this* question.
    let fold = TopologyFold::replay(inputs(), &events).expect("the log replays");
    let open = fold.open_questions().expect("started");
    assert_eq!(
        open.get(&question_for(ALEPH).id).map(|held| &held.question),
        Some(&question_for(ALEPH))
    );
    assert_eq!(fold.task_state(ALEPH), Some(TaskState::AwaitingInput));
}

/// `retained_generation_not_continued_after_kill`.
#[test]
fn retained_generation_not_continued_after_kill() {
    let dir = scratch("retained");
    spawn_kill_child(&dir, "retained");

    let events = committed(&dir);
    let last = events.last().expect("the log has lines");
    let TopologyEventBody::AttemptFinished { data } = &last.body else {
        panic!(
            "the settlement is not the last durable line: {:?}",
            last.body
        );
    };
    assert_eq!(
        data.retained().map(|(_, incarnation)| incarnation),
        Some(Epoch(0)),
        "the settlement did not retain, so this test proves nothing about a retained one"
    );
    assert!(
        !events.iter().any(|event| matches!(
            event.body,
            TopologyEventBody::AttemptStarted {
                data: AttemptStarted4 {
                    attempt: AttemptNumber(2),
                    ..
                }
            }
        )),
        "the dead process continued the retained generation"
    );

    // The fresh process: recovery step (e) closes it, and only then does
    // the run resume. After that, nothing can retry it.
    let mut fold = TopologyFold::replay(inputs(), &events).expect("the log replays");
    let closed = close_retained(&fold, &GenerationCloseReason::ResumeDiscardsRetainedSession)
        .expect("recovery closes it");
    assert_eq!(closed.len(), 1);
    for event in closed {
        apply(
            &mut fold,
            &ev(TopologyEventBody::GenerationClosed { data: event }),
        );
    }
    apply(&mut fold, &resume_event());
    assert!(!fold.ready_retry(ALEPH));

    let mut reservations = Reservations::new();
    let worktrees = FixedVerify::passing();
    let mut hooks = HarnessTopologyHooks::new(Arc::new(Mutex::new(HookHarness::new())));
    retry(
        &fold,
        &mut reservations,
        &worktrees,
        hooks.effects(),
        &retry_request(ALEPH, 0),
    )
    .expect_err("a fresh process continued a session it did not retain");
    assert!(worktrees.asked().is_empty());
}

/// The container runner policy is a value this fixture never uses, and
/// this is what says so: `run_started`'s runner is `host-v1`, so a resume
/// carrying the container record is refused rather than folded.
#[test]
fn a_resume_that_moved_the_runner_is_refused() {
    let mut fold = started();
    let moved = ev(TopologyEventBody::RunResumed {
        data: Box::new(RunResumed4 {
            incarnation: IncarnationId("01SETTLEINCARNATION0000003".to_owned()),
            runner: container_runner_policy(),
            probed_agents: probed_agents(),
            upstroke_version: "0.2.0-settle".to_owned(),
        }),
    });
    fold.plan_transition(&moved)
        .expect_err("a resume must rebuild the recorded runner exactly");
    apply(&mut fold, &resume_event());
    assert_eq!(fold.epoch(), Some(Epoch(1)));
}
