//! Extended notes: `docs/internals/engine/topology/settle/tests.md`

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
use crate::rundir::scratch_tree::{self, ScratchAcquireRefusal, ScratchTree};
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
            enabled: Some(true),
            alternative_available: Some(false),
            pass_timeout_secs: Some(89),
            primary: Some(PassBinding::new("aleph-Mid-agent", "aleph-Mid-model")),
            alternative: None,
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

pub(crate) fn apply(fold: &mut TopologyFold, event: &TopologyEvent) {
    let delta = fold
        .plan_transition(event)
        .unwrap_or_else(|error| panic!("the fixture's `{}` applies: {error}", event.body.kind()));
    fold.apply_delta(delta);
}

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

pub(crate) fn finished(key: TaskKey, generation: u32, attempt: u32, next: Next) -> FinishedAttempt {
    FinishedAttempt {
        key,
        generation: GenerationId(generation),
        attempt: AttemptNumber(attempt),
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

pub(crate) fn in_flight(fold: &mut TopologyFold, key: TaskKey, generation: u32) {
    apply(fold, &dispatch(key, generation));
    let event = attempt_started(fold, key, generation, 1);
    apply(fold, &event);
}

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

pub(crate) fn resuming(request: &mut FinishedAttempt, session: &SessionId) {
    request.session = Some(session.clone());
    request.record.session_id = Some(session.0.clone());
}

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

#[test]
fn each_ladder_decision_maps_to_its_own_settlement() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);

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
            Next::Defer,
            (FailureKind::RateLimited, FailureOrigin::Worker),
            SettlementTransition::Deferred {
                defers: 5,
                reason: "aleph failed its gates".to_owned(),
            },
            false,
        ),
        (
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

#[test]
fn the_lease_disposition_is_the_generations_own() {
    let mut fold = started();
    in_flight(&mut fold, BET, 0);
    let settled = settle_failed(&fold, &finished(BET, 0, 1, Next::Fail)).expect("settles");
    let AttemptSettlement::Closed { lease, .. } = settled.event.settlement else {
        panic!("a failure closes");
    };
    assert_eq!(lease, LeaseDisposition::PredictedReleased);
    apply(
        &mut fold,
        &ev(TopologyEventBody::AttemptFinished {
            data: Box::new(settled.event),
        }),
    );
    assert_eq!(fold.task_state(BET), Some(TaskState::Failed));
}

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

#[test]
fn a_question_is_read_back_from_the_event_and_not_re_decided() {
    let mut fold = started();
    in_flight(&mut fold, GIMEL, 0);
    let mut request = finished(GIMEL, 0, 1, Next::AskHuman(QuestionKind::Clarify));
    request.question = Some(question_for(GIMEL));
    let event = settle_into(&mut fold, &request);

    assert_eq!(rematerialize_question(&event), Some(&question_for(GIMEL)));
    let open = fold.open_questions().expect("started");
    assert_eq!(open.len(), 1);
    assert_eq!(
        open.get(&question_for(GIMEL).id).map(|held| &held.question),
        Some(&question_for(GIMEL))
    );
    assert_eq!(fold.task_state(GIMEL), Some(TaskState::AwaitingInput));

    let mut other = started();
    in_flight(&mut other, ALEPH, 0);
    let closed = settle_failed(&other, &finished(ALEPH, 0, 1, Next::Fail)).expect("settles");
    assert_eq!(rematerialize_question(&closed.event), None);
    let mut retaining = finished(ALEPH, 0, 1, Next::RetrySameRung { resume: true });
    resuming(&mut retaining, &SessionId("sess".to_owned()));
    let retained = settle_failed(&other, &retaining).expect("settles");
    assert_eq!(rematerialize_question(&retained.event), None);
}

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

#[test]
fn deferred_task_does_not_block_halted_or_budget_exceeded_closure() {
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

    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Fail));
    assert_eq!(fold.halted_at(), None);
    assert_ne!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Halted)
    );
}

#[test]
fn halting_drain_settlement_after_budget_exceeded_yields_halted() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
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
    let next = dispatch(ALEPH, 1);
    fold.plan_transition(&next).expect("a fresh generation");
    assert!(
        close_retained(&fold, &GenerationCloseReason::ResumeDiscardsRetainedSession)
            .expect("no retained generations")
            .is_empty()
    );
}

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

    let mut fold = started();
    apply(&mut fold, &dispatch(BET, 0));
    let closed = close_generation(&fold, BET, run_ending(RunOutcome::Complete))
        .expect("an open generation with no attempt closes");
    assert_eq!(closed.generation, GenerationId(0));
}

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

        apply(
            &mut fold,
            &ev(TopologyEventBody::GenerationClosed { data: closed }),
        );
        assert!(!fold.ready_retry(ALEPH));
    }
}

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

type Acquire = fn(&Path, &str) -> Result<ScratchTree, ScratchAcquireRefusal>;

const SCRATCH_DRAWS: u32 = 3;

fn scratch(label: &str) -> ScratchTree {
    scratch_with(scratch_tree::acquire, label)
}

fn scratch_with(acquire: Acquire, label: &str) -> ScratchTree {
    let parent = std::env::temp_dir();
    let tag = format!("pr7h-{label}");
    let mut occupied = Vec::new();
    for _ in 0..SCRATCH_DRAWS {
        match acquire(&parent, &tag) {
            Ok(tree) => return tree,
            Err(refusal @ ScratchAcquireRefusal::Occupied { .. }) => occupied.push(refusal),
            Err(refusal) => panic!("a scratch tree for `{label}`: {refusal:?}"),
        }
    }
    panic!("a scratch tree for `{label}`: {SCRATCH_DRAWS} draws refused as occupied: {occupied:?}");
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or_default()
        .to_owned()
}

static REFUSED_ONCE: AtomicBool = AtomicBool::new(false);

static OCCUPIED_CALLS: AtomicU32 = AtomicU32::new(0);

static UNDECIDABLE_CALLS: AtomicU32 = AtomicU32::new(0);

fn refuse_once_then_acquire(
    parent: &Path,
    tag: &str,
) -> Result<ScratchTree, ScratchAcquireRefusal> {
    if REFUSED_ONCE.swap(true, Ordering::SeqCst) {
        scratch_tree::acquire(parent, tag)
    } else {
        Err(ScratchAcquireRefusal::Occupied {
            root: parent.join(tag),
        })
    }
}

fn always_occupied(parent: &Path, tag: &str) -> Result<ScratchTree, ScratchAcquireRefusal> {
    let call = OCCUPIED_CALLS.fetch_add(1, Ordering::SeqCst);
    Err(ScratchAcquireRefusal::Occupied {
        root: parent.join(format!("{tag}-refused-{call}")),
    })
}

fn undecidable(parent: &Path, tag: &str) -> Result<ScratchTree, ScratchAcquireRefusal> {
    UNDECIDABLE_CALLS.fetch_add(1, Ordering::SeqCst);
    Err(ScratchAcquireRefusal::Undecidable {
        root: parent.join(tag),
        source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    })
}

#[test]
fn an_occupied_draw_is_drawn_again() {
    REFUSED_ONCE.store(false, Ordering::SeqCst);
    let tree = scratch_with(refuse_once_then_acquire, "drawn-again");
    assert!(
        REFUSED_ONCE.load(Ordering::SeqCst),
        "the double never refused"
    );
    assert!(
        tree.path().is_dir(),
        "the second draw is not a tree: {}",
        tree.path().display()
    );
}

#[test]
fn the_draws_are_bounded_and_every_refused_root_is_named() {
    OCCUPIED_CALLS.store(0, Ordering::SeqCst);
    let outcome = std::panic::catch_unwind(|| scratch_with(always_occupied, "bounded"));
    let message = panic_message(&outcome.expect_err("every draw was refused"));
    assert_eq!(
        OCCUPIED_CALLS.load(Ordering::SeqCst),
        SCRATCH_DRAWS,
        "the double was not asked exactly {SCRATCH_DRAWS} times"
    );
    assert!(
        message.contains("3 draws refused as occupied"),
        "the bound is not reported: {message}"
    );
    for call in 0..SCRATCH_DRAWS {
        assert!(
            message.contains(&format!("pr7h-bounded-refused-{call}")),
            "refused root {call} is not named: {message}"
        );
    }
}

#[test]
fn an_undecidable_refusal_is_not_drawn_again() {
    UNDECIDABLE_CALLS.store(0, Ordering::SeqCst);
    let outcome = std::panic::catch_unwind(|| scratch_with(undecidable, "undecidable"));
    let message = panic_message(&outcome.expect_err("the refusal is raised"));
    assert_eq!(
        UNDECIDABLE_CALLS.load(Ordering::SeqCst),
        1,
        "an undecidable answer was asked again"
    );
    assert!(message.contains("Undecidable"), "{message}");
    assert!(
        !message.contains("draws refused"),
        "an undecidable answer was reported as occupied: {message}"
    );
}

#[test]
fn a_kill_tests_scratch_tree_is_reclaimed_when_its_guard_drops() {
    let tree = scratch("reclaimed");
    let path = tree.path().to_path_buf();
    assert!(path.is_dir(), "the tree was created: {}", path.display());
    drop(tree);
    assert!(
        scratch_tree::proves_absent(&path),
        "the tree was not reclaimed: {}",
        path.display()
    );
}

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

fn append(log: &mut EventLog, hooks: &mut HarnessTopologyHooks, event: &TopologyEvent) {
    let (line, _) = TopologyLine::round_trip(event).expect("the event round-trips");
    log.append_topology_hooked(line.site(), &line, hooks.events())
        .expect("the append lands");
}

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
    assert!(
        !output.stderr.contains("must have taken this process"),
        "the child ran past the injection: {}",
        output.stderr
    );
    output
}

fn committed(dir: &Path) -> Vec<TopologyEvent> {
    let bytes = std::fs::read(dir.join("public").join("events.jsonl")).expect("the log");
    TopologyFold::parse_log(&bytes).expect("the log parses")
}

#[test]
fn kill_after_failed_settlement_rematerializes_question() {
    let tree = scratch("question");
    let dir = tree
        .checked_path()
        .expect("the acquired tree is current before child launch");
    let output = spawn_kill_child(dir, "question");
    let dir = tree
        .checked_path()
        .expect("the acquired tree is current before reading child residue");

    let payload = dir
        .join("public")
        .join("questions")
        .join(format!("{}.json", question_for(ALEPH).id.as_str()));

    assert!(
        scratch_tree::proves_absent(&payload),
        "the child wrote the question file it was killed before writing: {}{}",
        output.stdout,
        output.stderr
    );

    let events = committed(dir);
    let last = events.last().expect("the log has lines");
    let TopologyEventBody::AttemptFinished { data } = &last.body else {
        panic!(
            "the settlement is not the last durable line: {:?}",
            last.body
        );
    };

    assert_eq!(rematerialize_question(data), Some(&question_for(ALEPH)));

    let fold = TopologyFold::replay(inputs(), &events).expect("the log replays");
    let open = fold.open_questions().expect("started");
    assert_eq!(
        open.get(&question_for(ALEPH).id).map(|held| &held.question),
        Some(&question_for(ALEPH))
    );
    assert_eq!(fold.task_state(ALEPH), Some(TaskState::AwaitingInput));
}

#[test]
fn retained_generation_not_continued_after_kill() {
    let tree = scratch("retained");
    let dir = tree
        .checked_path()
        .expect("the acquired tree is current before child launch");
    spawn_kill_child(dir, "retained");
    let dir = tree
        .checked_path()
        .expect("the acquired tree is current before reading child residue");

    let events = committed(dir);
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
