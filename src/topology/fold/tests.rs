//! Extended notes: `docs/internals/topology/fold/tests.md`

use std::time::Duration;

use super::*;
use crate::events::{
    AttemptRecord, BindingSummary, BudgetKind, CapacitySnapshot, ChainSummary, DesignDefect,
    GateSummary, PoolExhausted, PoolSnapshot, ReviewPassOutcome, ReviewRecord,
};
use crate::gates::ShellKind;
use crate::ir::{
    Artifact, ArtifactId, Effort, PlanSource, QuestionKind, ResolvedEffortPolicy, Task, TaskId,
    TaskKind, Usage,
};
use crate::review::{PassBinding, ReviewPlan};
use crate::topology::events::{
    DeferWaitElapsed4, GenerationCloseReason, ImageIdentity, InfrastructureKind, Materialization,
    QuestionRaised4, RungBinding, RunnerContract, RunnerKind, RunnerPolicy, TOPOLOGY_EVENT_KINDS,
    TaskSpawned, TopologyLimits, UnavailableCause, VerificationRecord,
    VerificationVerdict as Verdict,
};
use crate::topology::leases::regions_overlap;
use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion};
use crate::topology::registry::{FrozenReviews, FrozenRung, FrozenTaskSpec, Lineage, Origin};
use crate::topology::schema::TOPOLOGY_SCHEMA;

#[cfg(test)]
mod outcome;
#[cfg(test)]
mod questions;

const RUN_ID: &str = "01FOLD0000000000000000000A";

type BreakRunner = fn(&mut RunnerPolicy);
type BreakLadder = fn(&mut FrozenLadder);
type BreakFrozenInputs = fn(&mut Plan, &mut ChainSummary);
type BreakSpawn = fn(&mut FrozenSpawn);
type BreakBinding = fn(&mut RungBinding);
type BreakPublication = fn(&mut MergePrepared);

type ForgeCandidate = fn(&mut MergePrepared);

type AddResidue = fn(&mut RunState);
type BreakRejection = fn(&mut MergeRejected);
const ZETA: TaskKey = TaskKey(0);
const ALPHA: TaskKey = TaskKey(1);
const MID: TaskKey = TaskKey(2);
const BETA: TaskKey = TaskKey(3);

fn sha(label: &str) -> CommitSha {
    let mut value: String = label
        .bytes()
        .map(|byte| char::from(b'a' + byte % 6))
        .collect();
    value.push_str(&"0".repeat(40));
    value.truncate(40);
    CommitSha(value)
}

fn git_ref(name: &str) -> GitRef {
    GitRef(format!("refs/upstroke/runs/{RUN_ID}/{name}"))
}

fn probed_agents() -> Vec<String> {
    vec![
        "  Codex-CLI  ".to_owned(),
        "ÜBER-agent-Ωmega".to_owned(),
        "claude-code".to_owned(),
        "z".repeat(200),
        "copilot".to_owned(),
    ]
}

fn task_of(id: &str, deps: &[&str], hints: &[&str], min_tier: Option<Tier>) -> Task {
    Task {
        id: TaskId::from(id),
        kind: match id {
            "zeta" => TaskKind::Fix,
            "alpha" => TaskKind::Refactor,
            _ => TaskKind::Test,
        },
        title: format!("  {id} — Ünicode title  "),
        body: format!("{id} body, {}", "long ".repeat(20)),
        depends_on: deps.iter().copied().map(TaskId::from).collect(),
        acceptance: vec![format!("{id} passes"), "and keeps passing".to_owned()],
        path_hints: hints.iter().copied().map(str::to_owned).collect(),
        suggested_tier: match id {
            "zeta" => Some(Tier::Frontier),
            "alpha" => None,
            _ => Some(Tier::Small),
        },
        min_tier,
        artifacts_in: vec![ArtifactId::from("contract")],
        artifacts_out: vec![ArtifactId::from(format!("{id}-out").as_str())],
    }
}

fn plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "frozen-Ünicode-hash".to_owned(),
        },
        tasks: vec![
            task_of("zeta", &["alpha"], &["src/Zebra/"], Some(Tier::Small)),
            task_of("alpha", &[], &["src/alpha/*.rs"], None),
            task_of(
                "mid",
                &["alpha", "zeta"],
                &["src/mid/", "build.rs"],
                Some(Tier::Mid),
            ),
        ],
        artifacts: vec![Artifact {
            id: ArtifactId::from("contract"),
            produced_by: Some(TaskId::from("alpha")),
        }],
    }
}

fn chain(task: &str) -> ChainSummary {
    let tiers = match task {
        "zeta" => vec![Tier::Small, Tier::Mid, Tier::Frontier],
        "alpha" => vec![Tier::Mid],
        _ => vec![Tier::Small, Tier::Frontier],
    };
    ChainSummary {
        task: task.to_owned(),
        attempts_per: match task {
            "zeta" => 2,
            "alpha" => 3,
            _ => 1,
        },
        bindings: Some(
            tiers
                .iter()
                .map(|tier| BindingSummary {
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

fn effort_policy() -> ResolvedEffortPolicy {
    ResolvedEffortPolicy {
        small: Effort::Low,
        mid: Effort::XHigh,
        frontier: Effort::Max,
        review: Effort::Medium,
    }
}

fn review_plan(tasks: usize) -> ReviewPlan {
    ReviewPlan {
        enabled: Some(true),
        alternative_available: Some(true),
        pass_timeout_secs: Some(1_337),
        primary: Some(PassBinding::new("claude-code", "claude-opus-5")),
        alternative: Some(PassBinding::new("copilot", "gpt-5.6")),
        second_opinion: (0..tasks)
            .map(|index| (index == 2).then(|| PassBinding::new("second-agent", "second-model")))
            .collect(),
    }
}

fn gate_summaries() -> Vec<GateSummary> {
    vec![GateSummary {
        name: "  Clippy Ünicode  ".to_owned(),
        cmd: "cargo clippy -- -D warnings".to_owned(),
        timeout: Duration::from_secs(909),
        shell: ShellKind::Bash,
    }]
}

fn path_policy() -> PathPolicy {
    PathPolicy {
        version: PathPolicyVersion::V1,
        case_fold: true,
        grammar: PathGrammar::Globset,
    }
}

fn container_runner() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Container,
        policy: RunnerContract::ContainerV1,
        image: Some(ImageIdentity {
            reference: "ghcr.io/example/Upstroke-Runner:2.1".to_owned(),
            id: "sha256:1111111111111111111111111111111111111111111111111111111111111111"
                .to_owned(),
            digest: Some(
                "sha256:2222222222222222222222222222222222222222222222222222222222222222"
                    .to_owned(),
            ),
        }),
        credential_volumes: Some(
            [
                (
                    "claude-code".to_owned(),
                    "upstroke-creds-Ünicode".to_owned(),
                ),
                (
                    "  Codex-CLI  ".to_owned(),
                    "upstroke-creds-codex".to_owned(),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    }
}

const NORMALIZED_DIGEST: &str =
    "sha256:9999999999999999999999999999999999999999999999999999999999999999";

fn inputs() -> FrozenInputs {
    FrozenInputs {
        plan: plan(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
    }
}

fn chain_plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "frozen-chain-Ünicode-hash".to_owned(),
        },
        tasks: vec![
            task_of("aay", &["bee"], &["src/aay/"], None),
            task_of("bee", &["cee"], &["src/bee/"], None),
            task_of("cee", &[], &["src/cee/"], None),
        ],
        artifacts: vec![Artifact {
            id: ArtifactId::from("contract"),
            produced_by: Some(TaskId::from("cee")),
        }],
    }
}

const AAY: TaskKey = TaskKey(0);
const BEE: TaskKey = TaskKey(1);
const CEE: TaskKey = TaskKey(2);

fn chain_inputs() -> FrozenInputs {
    FrozenInputs {
        plan: chain_plan(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
    }
}

fn chain_run_started_event() -> TopologyEvent {
    let plan = chain_plan();
    let unauthenticated = RunStarted4 {
        plan_hash: plan.source.hash.clone(),
        chains: plan.tasks.iter().map(|t| chain(t.id.as_str())).collect(),
        reviews: review_plan(plan.tasks.len()),
        registry_digest: String::new(),
        ..run_started_unauthenticated()
    };
    let digest = TaskRegistry::originals_with_agents(
        &plan,
        &unauthenticated.registry_record(),
        &unauthenticated.probed_agents,
    )
    .expect("the chain record derives a registry")
    .digest();
    ev(TopologyEventBody::RunStarted {
        data: Box::new(RunStarted4 {
            registry_digest: digest,
            ..unauthenticated
        }),
    })
}

fn wide_plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "frozen-wide-Ünicode-hash".to_owned(),
        },
        tasks: vec![
            task_of("zeta", &[], &["src/Zebra/"], Some(Tier::Small)),
            task_of("alpha", &[], &["src/alpha/*.rs"], None),
            task_of("mid", &[], &["src/mid/", "build.rs"], Some(Tier::Mid)),
            task_of("beta", &[], &["src/repairs/"], None),
        ],
        artifacts: vec![Artifact {
            id: ArtifactId::from("contract"),
            produced_by: Some(TaskId::from("alpha")),
        }],
    }
}

fn wide_inputs() -> FrozenInputs {
    FrozenInputs {
        plan: wide_plan(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
    }
}

fn wide_run_started_event(max_parallel: u32) -> TopologyEvent {
    let plan = wide_plan();
    let base = run_started_unauthenticated();
    let limits = TopologyLimits {
        max_parallel,
        ..base.limits
    };
    let unauthenticated = RunStarted4 {
        plan_hash: plan.source.hash.clone(),
        chains: plan.tasks.iter().map(|t| chain(t.id.as_str())).collect(),
        reviews: review_plan(plan.tasks.len()),
        limits,
        ..base
    };
    let digest = TaskRegistry::originals_with_agents(
        &plan,
        &unauthenticated.registry_record(),
        &unauthenticated.probed_agents,
    )
    .expect("the wide record derives a registry")
    .digest();
    ev(TopologyEventBody::RunStarted {
        data: Box::new(RunStarted4 {
            registry_digest: digest,
            ..unauthenticated
        }),
    })
}

fn wide_started(max_parallel: u32) -> TopologyFold {
    let mut fold = TopologyFold::new(wide_inputs());
    apply(&mut fold, &wide_run_started_event(max_parallel));
    fold
}

fn registry_digest() -> String {
    let plan = plan();
    let started = run_started_unauthenticated();
    TaskRegistry::originals_with_agents(&plan, &started.registry_record(), &started.probed_agents)
        .expect("the fixture record derives a registry")
        .digest()
}

fn run_started_unauthenticated() -> RunStarted4 {
    let plan = plan();
    RunStarted4 {
        schema: TOPOLOGY_SCHEMA,
        upstroke_version: "0.2.0-Ünicode".to_owned(),
        run_id: RUN_ID.to_owned(),
        incarnation: IncarnationId("01J8ZQKB2M7NC5PQR0TVWXYZ12".to_owned()),
        runner: container_runner(),
        probed_agents: probed_agents(),
        branch: format!("upstroke/run-{RUN_ID}"),
        integration_ref: git_ref("integration"),
        base_sha: sha("base"),
        execution_root: "/var/lib/Upstroke/execution roots".to_owned(),
        private_dir: "/var/lib/Upstroke/private runs".to_owned(),
        plan_path: "docs/Plan Ünicode.md".to_owned(),
        config_path: Some("upstroke.toml".to_owned()),
        plan_hash: plan.source.hash.clone(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
        registry_digest: String::new(),
        path_policy: path_policy(),
        limits: TopologyLimits {
            max_parallel: 3,
            max_defers: 2,
            max_merge_repairs: 1,
        },
        gates: vec!["fmt".to_owned(), "clippy".to_owned()],
        gates_from_config: true,
        gate_cmds: gate_summaries(),
        interaction_mode: "never".to_owned(),
        chains: plan.tasks.iter().map(|t| chain(t.id.as_str())).collect(),
        effort_policy: effort_policy(),
        reviews: review_plan(plan.tasks.len()),
    }
}

fn run_started() -> RunStarted4 {
    RunStarted4 {
        registry_digest: registry_digest(),
        ..run_started_unauthenticated()
    }
}

fn ev(body: TopologyEventBody) -> TopologyEvent {
    TopologyEvent {
        ts: "2026-08-17T09:41:02Z".to_owned(),
        body,
    }
}

fn run_started_event() -> TopologyEvent {
    ev(TopologyEventBody::RunStarted {
        data: Box::new(run_started()),
    })
}

fn started() -> TopologyFold {
    let mut fold = TopologyFold::new(inputs());
    apply(&mut fold, &run_started_event());
    fold
}

#[track_caller]
fn apply(fold: &mut TopologyFold, event: &TopologyEvent) {
    let delta = fold
        .plan_transition(event)
        .unwrap_or_else(|error| panic!("`{}` must apply: {error}", event.body.kind()));
    fold.apply_delta(delta);
}

#[track_caller]
fn refuse(fold: &TopologyFold, event: &TopologyEvent) -> FoldError {
    fold.plan_transition(event).expect_err(&format!(
        "`{}` must be refused by this state",
        event.body.kind()
    ))
}

#[track_caller]
fn accepts(fold: &TopologyFold, event: &TopologyEvent) {
    if let Err(error) = fold.plan_transition(event) {
        panic!("`{}` must apply: {error}", event.body.kind());
    }
}

fn review_pass(pass: &str, outcome: ReviewPassOutcome) -> ReviewRecord {
    ReviewRecord {
        pass: pass.to_owned(),
        agent: "copilot".to_owned(),
        model: "gpt-5.6".to_owned(),
        adapter: Some("copilot".to_owned()),
        preflight_cli_version: Some("0.9.3".to_owned()),
        effort: Some(Effort::Medium),
        pool: Some("copilot-business".to_owned()),
        cost_usd: None,
        outcome,
    }
}

fn attempt_record_for(key: TaskKey, attempt: u32) -> AttemptRecord {
    let mut record = attempt_record(attempt);
    let plan = review_plan(key.0 as usize + 1);
    if plan
        .second_opinion
        .get(key.0 as usize)
        .is_some_and(Option::is_some)
    {
        record
            .reviews
            .push(review_pass("second-opinion", ReviewPassOutcome::Passed));
    }
    record
}

fn attempt_record(attempt: u32) -> AttemptRecord {
    AttemptRecord {
        attempt,
        tier: "mid".to_owned(),
        model: "zeta-mid-model".to_owned(),
        pool: Some("codex-plus".to_owned()),
        resumed: false,
        duration: Duration::from_millis(123_456),
        cost_usd: Some(1.25),
        reviews: vec![review_pass("review", ReviewPassOutcome::Passed)],
        session_id: Some("sess-ÜNI-0042".to_owned()),
        usage: Some(Usage {
            input_tokens: Some(9_001),
            output_tokens: Some(313),
            cache_creation_input_tokens: Some(17),
            cache_read_input_tokens: Some(4_096),
            num_turns: Some(6),
            reasoning_output_tokens: Some(101),
        }),
        failure: None,
    }
}

fn question(id: &str, key: TaskKey) -> FrozenQuestion {
    FrozenQuestion {
        id: QuestionId::from(id),
        key,
        kind: QuestionKind::Unblock,
        context: "  A licence question only a person may settle.  ".to_owned(),
        options: vec![
            "  Codex-CLI  ".to_owned(),
            "ÜBER-agent-Ωmega".to_owned(),
            "claude-code".to_owned(),
        ],
    }
}

fn region(key: TaskKey) -> PathSet {
    let paths = match key {
        ZETA => vec!["src/Zebra"],
        ALPHA => vec!["src/alpha"],
        MID => vec!["src/mid", "build.rs"],
        _ => vec!["src/repairs"],
    };
    PathSet::Prefixes {
        paths: paths.into_iter().map(GitPath::from).collect(),
    }
}

fn dispatch(key: TaskKey, generation: u32, base: &CommitSha) -> TopologyEvent {
    ev(TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key,
            generation: GenerationId(generation),
            base_sha: base.clone(),
            worktree_path: format!("/private/workspaces/tasks/k{}-g{generation}", key.0),
            lease: LeaseGrant::Predicted { paths: region(key) },
            source_candidate: None,
        },
    })
}

fn dispatch_in(
    fold: &TopologyFold,
    key: TaskKey,
    generation: u32,
    base: &CommitSha,
) -> TopologyEvent {
    let mut event = dispatch(key, generation, base);
    if let TopologyEventBody::TaskDispatched { data } = &mut event.body {
        data.lease = LeaseGrant::Predicted {
            paths: fold
                .predicted_region(key)
                .expect("the fixture's run has started"),
        };
    }
    event
}

fn frozen_binding(fold: &TopologyFold, key: TaskKey, rung: usize) -> RungBinding {
    fold.frozen_rung_binding(key, u32::try_from(rung).expect("a small fixture rung"))
        .expect("the run has started and the fixture task has this rung")
}

#[test]
fn the_frozen_rung_binding_is_what_the_validator_accepts() {
    let mut fold = started();
    apply(&mut fold, &dispatch(ALPHA, 0, &sha("base")));

    let binding = fold
        .frozen_rung_binding(ALPHA, 0)
        .expect("the fixture task has rung 0");

    let accepted = ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: ALPHA,
            generation: GenerationId(0),
            attempt: AttemptNumber(1),
            rung: 0,
            binding: binding.clone(),
            pool: None,
            resume_session: None,
            materialization_observed: None,
        },
    });
    fold.plan_transition(&accepted)
        .expect("the validator accepts the binding this reader produced");

    for (label, mutate) in [
        (
            "model",
            (|b: &mut RungBinding| b.model.push_str("-x")) as fn(&mut RungBinding),
        ),
        ("agent", |b: &mut RungBinding| b.agent.push_str("-x")),
        ("effort", |b: &mut RungBinding| {
            b.effort = if b.effort == Effort::High {
                Effort::Low
            } else {
                Effort::High
            }
        }),
    ] {
        let mut wrong = binding.clone();
        mutate(&mut wrong);
        let refused = ev(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key: ALPHA,
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                rung: 0,
                binding: wrong,
                pool: None,
                resume_session: None,
                materialization_observed: None,
            },
        });
        assert!(
            matches!(
                fold.plan_transition(&refused),
                Err(FoldError::BindingMismatch { .. })
            ),
            "a binding differing only in `{label}` must be refused, or the \
                 positive half above is satisfied by a validator that is not \
                 looking"
        );
    }
}

fn attempt_started(
    fold: &TopologyFold,
    key: TaskKey,
    generation: u32,
    attempt: u32,
    rung: u32,
) -> TopologyEvent {
    ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key,
            generation: GenerationId(generation),
            attempt: AttemptNumber(attempt),
            rung,
            binding: frozen_binding(fold, key, rung as usize),
            pool: Some("codex-plus".to_owned()),
            resume_session: None,
            materialization_observed: None,
        },
    })
}

fn attempt_started_resuming(
    fold: &TopologyFold,
    key: TaskKey,
    generation: u32,
    attempt: u32,
    rung: u32,
    session: &str,
) -> TopologyEvent {
    let mut event = attempt_started(fold, key, generation, attempt, rung);
    if let TopologyEventBody::AttemptStarted { data } = &mut event.body {
        data.resume_session = Some(SessionId(session.to_owned()));
    }
    event
}

fn settle(
    key: TaskKey,
    generation: u32,
    attempt: u32,
    settlement: AttemptSettlement,
) -> TopologyEvent {
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
    ev(TopologyEventBody::AttemptFinished {
        data: Box::new(AttemptFinished4 {
            key,
            generation: GenerationId(generation),
            attempt: AttemptNumber(attempt),
            record: Box::new(record),
            settlement,
        }),
    })
}

fn settle_failing(
    key: TaskKey,
    generation: u32,
    attempt: u32,
    kind: crate::ladder::FailureKind,
    settlement: AttemptSettlement,
) -> TopologyEvent {
    let mut record = attempt_record(attempt);
    record.failure = Some(crate::events::FailureRecord {
        kind,
        origin: crate::ladder::FailureOrigin::Worker,
        reason: "the fixture's failure".to_owned(),
        detail: None,
    });
    ev(TopologyEventBody::AttemptFinished {
        data: Box::new(AttemptFinished4 {
            key,
            generation: GenerationId(generation),
            attempt: AttemptNumber(attempt),
            record: Box::new(record),
            settlement,
        }),
    })
}

fn candidate_of(key: TaskKey, generation: u32) -> CandidateRef {
    CandidateRef {
        key,
        generation: GenerationId(generation),
        commit_sha: sha(&format!("commit-{}-{generation}", key.0)),
        candidate_ref: git_ref(&format!("candidates/{}/{generation}", key.0)),
    }
}

fn candidate_prepared(key: TaskKey, generation: u32, base: &CommitSha) -> TopologyEvent {
    candidate_prepared_at(key, generation, 1, base)
}

fn candidate_prepared_at(
    key: TaskKey,
    generation: u32,
    attempt: u32,
    base: &CommitSha,
) -> TopologyEvent {
    ev(TopologyEventBody::CandidatePrepared {
        data: Box::new(CandidatePrepared {
            key,
            generation: GenerationId(generation),
            attempt: Box::new(attempt_record_for(key, attempt)),
            base_sha: base.clone(),
            parent_sha: base.clone(),
            tree_sha: sha(&format!("tree-{}-{generation}", key.0)),
            commit_sha: sha(&format!("commit-{}-{generation}", key.0)),
            message: format!("  {} candidate  ", key.0),
            prepared_ref: git_ref(&format!("candidate-prepared/{}/{generation}", key.0)),
            candidate_ref: git_ref(&format!("candidates/{}/{generation}", key.0)),
            actual_paths: region(key),
            lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region(key) },
        }),
    })
}

fn candidate_created(key: TaskKey, generation: u32) -> TopologyEvent {
    ev(TopologyEventBody::TaskCandidateCreated {
        data: TaskCandidateCreated {
            candidate: candidate_of(key, generation),
        },
    })
}

fn fast_publication(
    key: TaskKey,
    generation: u32,
    sequence: u32,
    head: &CommitSha,
    satisfies: Vec<TaskKey>,
) -> TopologyEvent {
    let candidate = candidate_of(key, generation);
    ev(TopologyEventBody::MergePrepared {
        data: Box::new(MergePrepared {
            sequence: SequenceId(sequence),
            disposition: PreparedDisposition::Fast,
            expected_head: head.clone(),
            proposed_sha: sha(&format!("commit-{}-{generation}", key.0)),
            key: candidate.key,
            generation: candidate.generation,
            candidate_sha: candidate.commit_sha,
            candidate_ref: candidate.candidate_ref,
            prepared_ref: None,
            verification_source: VerificationSource::CandidatePrepared {
                key,
                generation: GenerationId(generation),
            },
            verification: None,
            satisfies,
        }),
    })
}

fn merged(key: TaskKey, generation: u32, sequence: u32, satisfies: Vec<TaskKey>) -> TopologyEvent {
    ev(TopologyEventBody::TaskMerged {
        data: TaskMerged {
            sequence: SequenceId(sequence),
            merged_sha: sha(&format!("commit-{}-{generation}", key.0)),
            satisfies,
            lease_release: MergeLeaseRelease::Candidate {
                key,
                generation: GenerationId(generation),
            },
        },
    })
}

#[test]
fn selection_accessors_report_an_unstarted_run_as_holding_and_offering_nothing() {
    let fold = TopologyFold::new(inputs());
    assert_eq!(fold.pipeline_held(), 0, "nothing is dispatched yet");
    assert!(!fold.pipeline_reservable(), "there is no max_parallel yet");
    assert!(!fold.structurally_admissible());
    assert!(!fold.integration_admissible());
    for key in [ZETA, ALPHA, MID] {
        assert!(!fold.ready(key), "no task of an unstarted run is ready");
        assert!(!fold.ready_retry(key));
    }
}

#[test]
fn ready_names_only_the_task_whose_dependencies_are_merged() {
    let fold = started();
    assert!(fold.ready(ALPHA), "`alpha` has no dependencies");
    assert!(!fold.ready(ZETA), "`zeta` depends on `alpha`");
    assert!(!fold.ready(MID), "`mid` depends on `alpha` and `zeta`");
    assert!(
        fold.structurally_admissible(),
        "one ready task makes the run admissible"
    );
    assert!(
        !fold.integration_admissible(),
        "nothing is queued for integration"
    );
}

#[test]
fn pipeline_held_tracks_the_generation_classes_that_hold_the_entitlement() {
    let mut fold = started();
    assert_eq!(fold.pipeline_held(), 0);

    apply(&mut fold, &dispatch(ALPHA, 0, &sha("base")));
    assert_eq!(
        fold.pipeline_held(),
        1,
        "`OpenNoAttempt` holds a pipeline entitlement"
    );
    assert!(matches!(
        fold.task(ALPHA).and_then(TaskFold::open).map(|g| &g.class),
        Some(GenerationClass::OpenNoAttempt)
    ));
    assert!(
        !fold.ready(ALPHA),
        "a task with an open generation is not ready for a fresh dispatch"
    );

    let start = attempt_started(&fold, ALPHA, 0, 1, 0);
    apply(&mut fold, &start);
    assert_eq!(fold.pipeline_held(), 1, "`InFlight` holds one, not two");

    assert!(fold.pipeline_reservable());
}

#[test]
fn a_retained_generation_holds_no_pipeline_entitlement_and_is_ready_to_retry() {
    let mut fold = started();
    apply(&mut fold, &dispatch(ALPHA, 0, &sha("base")));
    let start = attempt_started(&fold, ALPHA, 0, 1, 0);
    apply(&mut fold, &start);
    apply(
        &mut fold,
        &settle(
            ALPHA,
            0,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId("sess-ÜNI-0007".to_owned()),
                retained_incarnation: Epoch(0),
            },
        ),
    );

    assert_eq!(
        fold.pipeline_held(),
        0,
        "`RetainedIdle` releases the entitlement"
    );
    assert!(
        fold.task(ALPHA).and_then(TaskFold::open).is_some(),
        "and keeps the generation open"
    );
    assert!(
        fold.ready_retry(ALPHA),
        "the retaining incarnation may retry in place"
    );
    assert!(
        !fold.ready(ALPHA),
        "a retained generation is retried, never re-dispatched"
    );
    assert!(fold.structurally_admissible());
}

#[test]
fn an_exhausted_generation_attempt_counter_is_refused_without_panicking() {
    const SESSION: &str = "counter-boundary-session";
    let mut fold = started();
    apply(&mut fold, &dispatch(ALPHA, 0, &sha("base")));
    let first = attempt_started(&fold, ALPHA, 0, 1, 0);
    apply(&mut fold, &first);
    apply(&mut fold, &retain(ALPHA, 1, SESSION, Epoch(0)));
    let retry = attempt_started_resuming(&fold, ALPHA, 0, 2, 0, SESSION);
    accepts(&fold, &retry);

    let run = fold.run.as_mut().expect("the checked prefix started a run");
    run.open_generation_mut(ALPHA)
        .expect("the checked prefix retained this generation")
        .attempts = u32::MAX;
    let before = fold.state().cloned();
    let error = fold
        .plan_transition(&retry)
        .expect_err("an exhausted counter has no representable next attempt");
    let FoldError::InconsistentRecord { kind, detail } = error else {
        panic!("counter exhaustion must return a contextual record refusal");
    };
    assert_eq!(kind, "attempt_started");
    assert!(
        detail.contains(&format!("task {ALPHA} generation 0")),
        "{detail}"
    );
    assert_eq!(fold.state(), before.as_ref());
}

fn retain(key: TaskKey, attempt: u32, session: &str, incarnation: Epoch) -> TopologyEvent {
    settle(
        key,
        0,
        attempt,
        AttemptSettlement::Retained {
            retained_session: SessionId(session.to_owned()),
            retained_incarnation: incarnation,
        },
    )
}

#[test]
fn a_retained_settlement_binds_its_envelope_to_its_record() {
    let base = sha("base");
    let session = "sess-ÜNI-retained";
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    accepts(&fold, &retain(ZETA, 1, session, Epoch(0)));

    let mut wrong_attempt = retain(ZETA, 1, session, Epoch(0));
    let TopologyEventBody::AttemptFinished { data } = &mut wrong_attempt.body else {
        unreachable!("built as an attempt_finished")
    };
    data.record.attempt = 2;
    assert!(
        matches!(
            refuse(&fold, &wrong_attempt),
            FoldError::WrongAttempt { attempt: 2, .. }
        ),
        "a retained settlement carried another attempt's ledger line"
    );

    let mut wrong_session = retain(ZETA, 1, session, Epoch(0));
    let TopologyEventBody::AttemptFinished { data } = &mut wrong_session.body else {
        unreachable!("built as an attempt_finished")
    };
    data.record.session_id = Some("sess-somebody-elses".to_owned());
    let error = refuse(&fold, &wrong_session);
    assert!(
        matches!(error, FoldError::InconsistentRecord { .. }),
        "a retained settlement kept a session its record does not report: {error:?}"
    );

    let mut sessionless = retain(ZETA, 1, session, Epoch(0));
    let TopologyEventBody::AttemptFinished { data } = &mut sessionless.body else {
        unreachable!("built as an attempt_finished")
    };
    data.record.session_id = None;
    let error = refuse(&fold, &sessionless);
    assert!(
        matches!(error, FoldError::InconsistentRecord { .. }),
        "a retained settlement reported no session to retain: {error:?}"
    );

    let mut wrong_generation = retain(ZETA, 1, session, Epoch(0));
    let TopologyEventBody::AttemptFinished { data } = &mut wrong_generation.body else {
        unreachable!("built as an attempt_finished")
    };
    data.generation = GenerationId(1);
    assert!(
        matches!(
            refuse(&fold, &wrong_generation),
            FoldError::NotTheOpenGeneration { .. }
        ),
        "a retained settlement named a generation this task does not hold open"
    );

    assert!(
        matches!(
            refuse(&fold, &retain(ZETA, 1, session, Epoch(7))),
            FoldError::StaleIncarnation { .. }
        ),
        "a retained settlement claimed an incarnation this run is not in"
    );

    let generation = fold
        .task(ZETA)
        .and_then(|task| task.generations.first())
        .expect("the generation is open");
    assert!(matches!(generation.class, GenerationClass::InFlight { .. }));
}

type SettlementArm = (&'static str, fn() -> AttemptSettlement);

#[test]
fn no_attempt_finished_arm_accepts_a_record_that_claims_success() {
    let base = sha("base");
    let session = "sess-ÜNI-unsettled";

    let claims_success = |event: &mut TopologyEvent| {
        let TopologyEventBody::AttemptFinished { data } = &mut event.body else {
            unreachable!("built as an attempt_finished")
        };
        data.record.failure = None;
        data.record.reviews = vec![review_pass("review", ReviewPassOutcome::Passed)];
        assert!(
            data.record.is_successful(),
            "the fixture is not the successful shape"
        );
    };

    let arms: Vec<SettlementArm> = vec![
        ("retained", || AttemptSettlement::Retained {
            retained_session: SessionId("sess-ÜNI-unsettled".to_owned()),
            retained_incarnation: Epoch(0),
        }),
        ("closed", || AttemptSettlement::Closed {
            transition: SettlementTransition::Retry,
            lease: LeaseDisposition::PredictedReleased,
        }),
    ];

    for (label, settlement) in arms {
        let mut fold = started();
        apply(&mut fold, &dispatch(ZETA, 0, &base));
        let start = attempt_started(&fold, ZETA, 0, 1, 0);
        apply(&mut fold, &start);

        accepts(&fold, &settle(ZETA, 0, 1, settlement()));

        let mut lying = settle(ZETA, 0, 1, settlement());
        claims_success(&mut lying);
        let error = refuse(&fold, &lying);
        assert!(
            matches!(error, FoldError::InconsistentRecord { .. }),
            "{label}: a record claiming success settled an attempt: {error:?}"
        );

        let generation = fold
            .task(ZETA)
            .and_then(|task| task.generations.first())
            .expect("the generation is open");
        assert!(
            matches!(generation.class, GenerationClass::InFlight { .. }),
            "{label}: the refused settlement moved the generation anyway"
        );

        let mut judged = settle(ZETA, 0, 1, settlement());
        let TopologyEventBody::AttemptFinished { data } = &mut judged.body else {
            unreachable!("built as an attempt_finished")
        };
        data.record.failure = None;
        data.record.reviews = vec![review_pass("review", ReviewPassOutcome::Failed)];
        assert!(
            !data.record.is_successful(),
            "{label}: the fixture claims success after all"
        );
        accepts(&fold, &judged);
    }

    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    accepts(&fold, &candidate_prepared(ZETA, 0, &base));
    let _ = session;
}

#[test]
fn a_retained_settlement_releases_the_pipeline_and_nothing_else() {
    let base = sha("base");
    let session = "sess-ÜNI-held";
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let held = fold.predicted_region(ZETA).expect("the run has a registry");
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    assert_eq!(
        fold.pipeline_held(),
        1,
        "the in-flight generation holds the entitlement, or the release below is unobservable"
    );

    apply(&mut fold, &retain(ZETA, 1, session, Epoch(0)));

    assert_eq!(fold.pipeline_held(), 0, "the entitlement was not released");
    let owner = LeaseOwner::Generation {
        key: ZETA,
        generation: GenerationId(0),
    };
    let leases = fold.leases().expect("the run has started");
    assert!(
        leases.holds(owner),
        "a retained generation keeps its worktree, so it keeps its predicted lease"
    );
    assert!(
        leases.overlaps_another(
            LeaseOwner::Generation {
                key: ALPHA,
                generation: GenerationId(0)
            },
            &held,
            &path_policy(),
        ),
        "the retained region no longer blocks another owner, so the lease is held in name only"
    );
    assert_eq!(
        fold.task_state(ZETA),
        Some(TaskState::Pending),
        "a retained attempt is unsettled: the task is neither merged nor failed"
    );
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::NotEnding,
        "a retained generation leaves the run running"
    );

    assert!(matches!(
        refuse(&fold, &retain(ZETA, 1, session, Epoch(0))),
        FoldError::NotTheOpenGeneration { .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &settle(
                ZETA,
                0,
                1,
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Retry,
                    lease: LeaseDisposition::PredictedReleased,
                }
            )
        ),
        FoldError::NotTheOpenGeneration { .. }
    ));

    let retry = attempt_started_resuming(&fold, ZETA, 0, 2, 0, session);
    accepts(&fold, &retry);
    let mut resumed = fold.clone();
    let runner = fold.started().expect("the run has started").runner.clone();
    apply(&mut resumed, &resume(runner));
    assert!(matches!(
        refuse(
            &resumed,
            &attempt_started_resuming(&resumed, ZETA, 0, 2, 0, session)
        ),
        FoldError::StaleIncarnation { .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &attempt_started_resuming(&fold, ZETA, 0, 2, 0, "sess-somebody-elses")
        ),
        FoldError::StaleIncarnation { .. }
    ));
}

#[test]
fn a_poisoned_fold_authorises_nothing_while_still_reporting_what_it_holds() {
    let mut fold = wide_started(3);
    queue_candidate(&mut fold, MID, 0);
    retained_generation(&mut fold, ZETA, 0);
    apply(&mut fold, &dispatch(BETA, 0, &sha("base")));

    assert!(
        fold.ready(ALPHA),
        "`alpha` waits on nothing and holds no generation"
    );
    assert!(
        fold.ready_retry(ZETA),
        "`zeta` retained a session in this incarnation"
    );
    assert!(
        fold.pipeline_reservable(),
        "one of three entitlements is held"
    );
    assert!(fold.structurally_admissible());
    assert!(
        fold.integration_admissible(),
        "`mid`'s candidate is queued and eligible"
    );
    assert_eq!(fold.pipeline_held(), 1, "`beta` holds one");

    fold.poison();

    assert!(fold.is_poisoned());
    assert!(!fold.ready(ALPHA), "a poisoned fold offered a dispatch");
    assert!(!fold.ready_retry(ZETA), "a poisoned fold offered a retry");
    assert!(
        !fold.pipeline_reservable(),
        "a poisoned fold offered an entitlement"
    );
    assert!(
        !fold.structurally_admissible(),
        "a poisoned fold called itself admissible"
    );
    assert!(
        !fold.integration_admissible(),
        "a poisoned fold offered an integration"
    );
    for key in [ZETA, ALPHA, MID, BETA] {
        assert!(!fold.ready(key), "a poisoned fold offered a dispatch");
        assert!(!fold.ready_retry(key), "a poisoned fold offered a retry");
    }

    assert_eq!(
        fold.pipeline_held(),
        1,
        "the entitlement is still held; only the authorisation is withdrawn"
    );
}

#[test]
fn an_integration_is_inadmissible_while_the_pipeline_entitlement_is_held() {
    let mut narrow = wide_started(1);
    queue_candidate(&mut narrow, MID, 0);
    assert_eq!(narrow.pipeline_held(), 0, "the generation closed");
    assert!(
        narrow.integration_admissible(),
        "an eligible candidate with the slot free is admissible"
    );

    apply(&mut narrow, &dispatch(ZETA, 0, &sha("base")));
    assert_eq!(narrow.pipeline_held(), 1);
    assert!(!narrow.pipeline_reservable(), "one of one");
    assert!(
        !narrow.ready(ALPHA),
        "a fresh dispatch is refused by the entitlement"
    );
    assert!(
        !narrow.integration_admissible(),
        "and so is an integration, which would hold one of its own"
    );
    assert!(
        !narrow.structurally_admissible(),
        "no branch is structurally admissible while the run's one slot is held"
    );

    let mut wider = wide_started(2);
    queue_candidate(&mut wider, MID, 0);
    apply(&mut wider, &dispatch(ZETA, 0, &sha("base")));
    assert_eq!(wider.pipeline_held(), 1);
    assert!(
        wider.integration_admissible(),
        "one of two entitlements held leaves room for the integration's"
    );
}

#[test]
fn a_ladder_position_is_derived_by_replay_and_not_assumed() {
    let base = sha("base");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    for attempt in 1..=2u32 {
        for event in [
            dispatch(ZETA, attempt - 1, &base),
            attempt_started(&live, ZETA, attempt - 1, 1, 0),
        ] {
            apply(&mut live, &event);
            trace.push(event);
        }
        let last = attempt == 2;
        let settlement = settle(
            ZETA,
            attempt - 1,
            1,
            AttemptSettlement::Closed {
                transition: if last {
                    SettlementTransition::Escalated { rung: 1 }
                } else {
                    SettlementTransition::Retry
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        );
        apply(&mut live, &settlement);
        trace.push(settlement);
    }

    let task = live.task(ZETA).expect("registered");
    assert_eq!(task.rung, 1, "the escalation did not move the task's rung");
    assert_eq!(
        task.attempts_on_rung, 0,
        "the allowance is per rung, so an escalation starts it again"
    );
    assert_eq!(
        task.state,
        TaskState::Pending,
        "an escalated task must be dispatchable again — this is what makes \
             the rung above load-bearing rather than decorative"
    );

    let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");
    let replayed = TopologyFold::replay(inputs(), &parsed).expect("the log replays");
    let after = replayed.task(ZETA).expect("registered");
    assert_eq!(
        (after.rung, after.attempts_on_rung),
        (task.rung, task.attempts_on_rung),
        "the ladder position did not survive the process that wrote it, so \
             the next one would dispatch this task on a rung the log contradicts"
    );

    assert_ne!(0, after.rung, "a process-local rung tally reads zero here");
}

#[test]
fn an_attempt_runs_at_the_replay_derived_rung_and_an_escalation_climbs_exactly_one() {
    let base = sha("base");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    for (generation, transition) in [
        (0, SettlementTransition::Retry),
        (1, SettlementTransition::Escalated { rung: 1 }),
    ] {
        for event in [
            dispatch(ZETA, generation, &base),
            attempt_started(&live, ZETA, generation, 1, 0),
            settle(
                ZETA,
                generation,
                1,
                AttemptSettlement::Closed {
                    transition,
                    lease: LeaseDisposition::PredictedReleased,
                },
            ),
        ] {
            apply(&mut live, &event);
            trace.push(event);
        }
    }
    let position = |fold: &TopologyFold| {
        let task = fold.task(ZETA).expect("registered");
        (task.rung, task.attempts_on_rung)
    };
    assert_eq!(position(&live), (1, 0), "the escalation put zeta on rung 1");

    let dispatched = dispatch(ZETA, 2, &base);
    apply(&mut live, &dispatched);
    trace.push(dispatched);

    for (label, rung) in [("below", 0), ("above", 2)] {
        let start = attempt_started(&live, ZETA, 2, 1, rung);
        let refused = refuse(&live, &start);
        assert!(
            matches!(
                refused,
                FoldError::WrongRung {
                    kind: "attempt_started",
                    key,
                    attempt: 1,
                    rung: refused_rung,
                    ..
                } if key == ZETA.0 && refused_rung == rung
            ),
            "a start {label} the task's rung, carrying rung {rung}'s own frozen binding, \
             was refused as `{refused}` rather than as the wrong rung"
        );
        assert_eq!(
            position(&live),
            (1, 0),
            "a refused start {label} the rung moved the ladder position"
        );
    }

    let at_position = attempt_started(&live, ZETA, 2, 1, 1);
    apply(&mut live, &at_position);
    trace.push(at_position);

    let escalate = |rung: u32| {
        settle(
            ZETA,
            2,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Escalated { rung },
                lease: LeaseDisposition::PredictedReleased,
            },
        )
    };
    for (label, rung) in [("backward", 0), ("sideways", 1), ("skipping a rung", 3)] {
        let refused = refuse(&live, &escalate(rung));
        assert!(
            matches!(
                refused,
                FoldError::WrongRung {
                    kind: "attempt_finished",
                    key,
                    attempt: 1,
                    rung: refused_rung,
                    ..
                } if key == ZETA.0 && refused_rung == rung
            ),
            "an escalation {label} onto rung {rung} was refused as `{refused}` rather than \
             as the wrong rung"
        );
    }
    let one_up = escalate(2);
    apply(&mut live, &one_up);
    trace.push(one_up);
    assert_eq!(
        position(&live),
        (2, 0),
        "the escalation onto the next rung did not move the task there"
    );

    for event in [
        dispatch(ZETA, 3, &base),
        attempt_started(&live, ZETA, 3, 1, 2),
    ] {
        apply(&mut live, &event);
        trace.push(event);
    }
    let off_the_top = settle(
        ZETA,
        3,
        1,
        AttemptSettlement::Closed {
            transition: SettlementTransition::Escalated { rung: 3 },
            lease: LeaseDisposition::PredictedReleased,
        },
    );
    assert!(
        matches!(
            refuse(&live, &off_the_top),
            FoldError::WrongRung {
                kind: "attempt_finished",
                rung: 3,
                ..
            }
        ),
        "zeta's ladder has three rungs and the human is the top one, so nothing escalates \
         onto rung 3"
    );
    let retried = settle(
        ZETA,
        3,
        1,
        AttemptSettlement::Closed {
            transition: SettlementTransition::Retry,
            lease: LeaseDisposition::PredictedReleased,
        },
    );
    apply(&mut live, &retried);
    trace.push(retried);
    assert_eq!(
        position(&live),
        (2, 1),
        "the failure on rung 2 was not charged to rung 2's allowance"
    );

    let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");
    let mut replayed = TopologyFold::replay(inputs(), &parsed).expect("the log replays");
    assert_eq!(
        position(&replayed),
        position(&live),
        "the ladder position did not survive the process that wrote it"
    );
    for fold in [&mut live, &mut replayed] {
        apply(fold, &dispatch(ZETA, 4, &base));
        for rung in [0, 1] {
            assert!(
                matches!(
                    refuse(fold, &attempt_started(fold, ZETA, 4, 1, rung)),
                    FoldError::WrongRung { rung: refused, .. } if refused == rung
                ),
                "a start on rung {rung} was accepted while the log holds rung 2"
            );
        }
        accepts(fold, &attempt_started(fold, ZETA, 4, 1, 2));
    }
}

const CHARGE_ALLOWANCE: fn(&mut RunState, TaskKey, &AttemptRecord) = RunState::charge_allowance;

#[test]
fn an_interrupted_attempt_refunds_the_rungs_allowance() {
    use crate::ladder::FailureKind;

    let base = sha("base");

    let mut spent = started();
    for event in [
        dispatch(ALPHA, 0, &base),
        attempt_started(&spent, ALPHA, 0, 1, 0),
        settle_failing(
            ALPHA,
            0,
            1,
            FailureKind::GateFailed,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Retry,
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    ] {
        apply(&mut spent, &event);
    }
    assert_eq!(
        spent.task(ALPHA).expect("registered").attempts_on_rung,
        1,
        "a judged rejection is a spent attempt — the worker ran and its work \
             was judged, which is `spends_allowance`'s line"
    );

    let mut refunded = started();
    for event in [
        dispatch(ALPHA, 0, &base),
        attempt_started(&refunded, ALPHA, 0, 1, 0),
        settle_failing(
            ALPHA,
            0,
            1,
            FailureKind::Interrupted,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Retry,
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    ] {
        apply(&mut refunded, &event);
    }
    assert_eq!(
        refunded.task(ALPHA).expect("registered").attempts_on_rung,
        0,
        "T-ATTEMPT refunds an interrupted attempt's allowance. A fold that \
             counted the START charged for a run that never got a verdict, and \
             an operator paid a pricier tier for the engine having died"
    );

    let label_of = |settlement: &AttemptSettlement| -> &'static str {
        match settlement {
            AttemptSettlement::Retained { .. } => "retained",
            AttemptSettlement::Closed { transition, .. } => match transition {
                SettlementTransition::Succeeded => "closed/succeeded",
                SettlementTransition::Retry => "closed/retry",
                SettlementTransition::Escalated { .. } => "closed/escalated",
                SettlementTransition::Deferred { .. } => "closed/deferred",
                SettlementTransition::Parked { .. } => "closed/parked",
                SettlementTransition::Failed { .. } => "closed/failed",
            },
        }
    };
    let arms: Vec<AttemptSettlement> = vec![
        AttemptSettlement::Retained {
            retained_session: SessionId("sess-ÜNI-allowance".to_owned()),
            retained_incarnation: Epoch(0),
        },
        AttemptSettlement::Closed {
            transition: SettlementTransition::Retry,
            lease: LeaseDisposition::PredictedReleased,
        },
        AttemptSettlement::Closed {
            transition: SettlementTransition::Deferred {
                defers: 1,
                reason: "  the fixture's backoff  ".to_owned(),
            },
            lease: LeaseDisposition::PredictedReleased,
        },
        AttemptSettlement::Closed {
            transition: SettlementTransition::Parked {
                question: question("q-allowance", ALPHA),
            },
            lease: LeaseDisposition::PredictedReleased,
        },
        AttemptSettlement::Closed {
            transition: SettlementTransition::Failed {
                halts_run: false,
                reason: "  the fixture's terminal failure  ".to_owned(),
            },
            lease: LeaseDisposition::PredictedReleased,
        },
    ];
    let mut driven: Vec<&str> = arms.iter().map(&label_of).collect();
    driven.sort_unstable();
    assert_eq!(
        driven,
        [
            "closed/deferred",
            "closed/failed",
            "closed/parked",
            "closed/retry",
            "retained"
        ],
        "the settlements driven below are not the vocabulary minus \
         `closed/succeeded` and `closed/escalated`, so an arm has left this \
         table and the pair is asserted about fewer settlements than the doc \
         above claims"
    );
    for settlement in arms {
        let label = label_of(&settlement);
        for (kind, spent) in [
            (FailureKind::GateFailed, 1_u32),
            (FailureKind::Interrupted, 0),
        ] {
            let mut fold = started();
            for event in [
                dispatch(ALPHA, 0, &base),
                attempt_started(&fold, ALPHA, 0, 1, 0),
            ] {
                apply(&mut fold, &event);
            }
            let mut event = settle_failing(ALPHA, 0, 1, kind, settlement.clone());
            if let TopologyEventBody::AttemptFinished { data } = &mut event.body {
                if let AttemptSettlement::Retained {
                    retained_session, ..
                } = &data.settlement
                {
                    data.record.session_id = Some(retained_session.0.clone());
                }
            }
            apply(&mut fold, &event);
            assert_eq!(
                fold.task(ALPHA).expect("registered").attempts_on_rung,
                spent,
                "{label} settling a {kind:?} attempt did not leave the rung's \
                 allowance where `ladder::spends_allowance` puts it. The charge \
                 is decided by the failure and by nothing about the \
                 settlement's shape, so an arm that skips it hands an operator \
                 retries on a rung already paid for"
            );
        }
    }

    let mut direct = started();
    apply(&mut direct, &dispatch(ALPHA, 0, &base));
    let mut run = direct.run.take().expect("the fixture's run has started");
    let mut judged = attempt_record(1);
    judged.failure = Some(crate::events::FailureRecord {
        kind: FailureKind::GateFailed,
        origin: crate::ladder::FailureOrigin::Worker,
        reason: "the fixture's judged failure".to_owned(),
        detail: None,
    });
    CHARGE_ALLOWANCE(&mut run, ALPHA, &judged);
    direct.run = Some(run);
    assert_eq!(
        direct.task(ALPHA).expect("registered").attempts_on_rung,
        1,
        "the path `RunState::charge_allowance` resolves to an item that does \
             not charge the rung, so the census's fixture names one thing and \
             the appliers call another"
    );
}

#[test]
fn a_deferral_count_is_derived_by_replay_and_not_by_a_process_local_tally() {
    let base = sha("base");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    for round in 1..=3u32 {
        for event in [
            dispatch(ALPHA, round - 1, &base),
            attempt_started(&live, ALPHA, round - 1, 1, 0),
        ] {
            apply(&mut live, &event);
            trace.push(event);
        }
        let settlement = settle(
            ALPHA,
            round - 1,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Deferred {
                    defers: round,
                    reason: "the pool is down".to_owned(),
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        );
        apply(&mut live, &settlement);
        trace.push(settlement);

        let woken = ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 30_000,
                round,
            },
        });
        apply(&mut live, &woken);
        trace.push(woken);
    }

    let live_defers = live.task(ALPHA).expect("the task is registered").defers;
    assert_eq!(live_defers, 3, "the writing process counted three");

    let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");
    let replayed = TopologyFold::replay(inputs(), &parsed).expect("the log replays");
    let replayed_defers = replayed.task(ALPHA).expect("registered").defers;

    assert_eq!(
        replayed_defers, live_defers,
        "the count did not survive the process that wrote it, so the next \
             one would decide the outage branch from a number the log \
             contradicts"
    );

    let process_local_tally: u32 = 0;
    assert_ne!(
        process_local_tally, replayed_defers,
        "a process-local tally is only wrong across a resume, which is \
             exactly when nothing is watching it"
    );
}

#[test]
fn the_statement_accessors_report_the_run_rather_than_authorising_anything() {
    let unstarted = TopologyFold::new(inputs());
    assert!(
        !unstarted.run_is_ending(),
        "a run that has recorded nothing has not ended"
    );
    assert!(!unstarted.backoff_pending());
    assert!(!unstarted.questions_open());

    let mut fold = started();
    assert!(!fold.run_is_ending());
    assert!(!fold.backoff_pending());
    assert!(!fold.questions_open());

    apply(&mut fold, &dispatch(ALPHA, 0, &sha("base")));
    let start = attempt_started(&fold, ALPHA, 0, 1, 0);
    apply(&mut fold, &start);
    apply(
        &mut fold,
        &settle(
            ALPHA,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Deferred {
                    defers: 1,
                    reason: "  the pool is down  ".to_owned(),
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    );
    assert!(
        fold.backoff_pending(),
        "a deferred task is waiting on a wait"
    );
    assert!(!fold.questions_open(), "and a wait is not a question");
    assert!(!fold.run_is_ending(), "neither ends the run");

    apply(&mut fold, &raised("q-ÜNI-statement", ZETA));
    assert!(fold.questions_open());
    assert!(fold.backoff_pending(), "a question is not a wait either");
    assert!(!fold.run_is_ending());

    let mut poisoned = fold.clone();
    poisoned.poison();
    assert!(
        poisoned.backoff_pending(),
        "poisoning unsaid a deferred task"
    );
    assert!(
        poisoned.questions_open(),
        "poisoning unsaid an open question"
    );
    assert!(
        !poisoned.structurally_admissible(),
        "and it did withdraw the authorisation"
    );

    let mut stopped = started();
    apply(&mut stopped, &budget_exceeded(0, Some(MID)));
    assert!(stopped.run_is_ending(), "a stop of this epoch ends the run");
    apply(&mut stopped, &resume(container_runner()));
    assert!(
        !stopped.run_is_ending(),
        "the resume raised the ceiling the stop was against"
    );
    stopped.poison();
    assert!(
        !stopped.run_is_ending(),
        "poisoning invented an ending the log does not record"
    );

    let mut halted = started();
    apply(&mut halted, &dispatch(ZETA, 0, &sha("base")));
    let start = attempt_started(&halted, ZETA, 0, 1, 0);
    apply(&mut halted, &start);
    apply(
        &mut halted,
        &settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Failed {
                    halts_run: true,
                    reason: "  the halt policy fired  ".to_owned(),
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    );
    assert_eq!(halted.halted_at(), Some(ZETA));
    assert!(halted.run_is_ending());
    halted.poison();
    assert!(halted.run_is_ending(), "poisoning unsaid a halt");
}

fn queue_candidate(fold: &mut TopologyFold, key: TaskKey, generation: u32) {
    let base = sha("base");
    apply(fold, &dispatch(key, generation, &base));
    let start = attempt_started(fold, key, generation, 1, 0);
    apply(fold, &start);
    apply(fold, &candidate_prepared(key, generation, &base));
    apply(fold, &candidate_created(key, generation));
}

fn retained_generation(fold: &mut TopologyFold, key: TaskKey, generation: u32) {
    let epoch = fold.epoch().expect("the run has started");
    apply(fold, &dispatch(key, generation, &sha("base")));
    let start = attempt_started(fold, key, generation, 1, 0);
    apply(fold, &start);
    apply(
        fold,
        &settle(
            key,
            generation,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId(format!("sess-ÜNI-{}-{generation}", key.0)),
                retained_incarnation: epoch,
            },
        ),
    );
}

fn merge_task(fold: &mut TopologyFold, key: TaskKey, generation: u32, sequence: u32) {
    let base = sha("base");
    apply(fold, &dispatch(key, generation, &base));
    let start = attempt_started(fold, key, generation, 1, 0);
    apply(fold, &start);
    apply(fold, &candidate_prepared(key, generation, &base));
    apply(fold, &candidate_created(key, generation));
    apply(
        fold,
        &fast_publication(key, generation, sequence, &base, vec![key]),
    );
    apply(fold, &merged(key, generation, sequence, vec![key]));
}

#[test]
fn a_topology_log_is_folded_from_its_run_started_and_from_nothing_else() {
    let fold = TopologyFold::new(inputs());
    let mut refused = 0;
    for event in every_kind() {
        if matches!(event.body, TopologyEventBody::RunStarted { .. }) {
            accepts(&fold, &event);
            continue;
        }
        assert_eq!(
            refuse(&fold, &event),
            FoldError::NotStarted {
                kind: event.body.kind()
            },
            "`{}` was folded into a run that has not started",
            event.body.kind()
        );
        refused += 1;
    }
    assert_eq!(
        refused,
        TOPOLOGY_EVENT_KINDS.len() - 1,
        "every kind but `run_started` has to be refused before a run starts"
    );
}

#[test]
fn a_run_begins_once_and_says_it_is_a_topology_run() {
    let fold = started();
    assert_eq!(
        refuse(&fold, &run_started_event()),
        FoldError::AlreadyStarted
    );

    for schema in [0, 1, 2, 3, 5, 99] {
        let event = ev(TopologyEventBody::RunStarted {
            data: Box::new(RunStarted4 {
                schema,
                ..run_started()
            }),
        });
        assert_eq!(
            refuse(&TopologyFold::new(inputs()), &event),
            FoldError::NotTopologySchema { schema }
        );
    }
}

#[test]
fn a_run_started_carries_a_runner_record_that_could_be_re_established() {
    let mut runner = container_runner();
    if let Some(image) = runner.image.as_mut() {
        image.digest = None;
    }
    accepts(
        &TopologyFold::new(inputs()),
        &ev(TopologyEventBody::RunStarted {
            data: Box::new(RunStarted4 {
                runner,
                ..run_started()
            }),
        }),
    );

    let cases: [(&str, BreakRunner); 5] = [
        ("contract does not match kind", |runner| {
            runner.policy = RunnerContract::HostV1;
        }),
        ("container without an image", |runner| {
            runner.image = None;
        }),
        ("image without a reference", |runner| {
            if let Some(image) = runner.image.as_mut() {
                image.reference = String::new();
            }
        }),
        ("container without credential volumes", |runner| {
            runner.credential_volumes = None;
        }),
        ("host carrying container fields", |runner| {
            runner.kind = RunnerKind::Host;
            runner.policy = RunnerContract::HostV1;
        }),
    ];
    let mut messages: BTreeSet<String> = BTreeSet::new();
    for (label, break_it) in cases {
        let mut runner = container_runner();
        break_it(&mut runner);
        let event = ev(TopologyEventBody::RunStarted {
            data: Box::new(RunStarted4 {
                runner,
                ..run_started()
            }),
        });
        let error = refuse(&TopologyFold::new(inputs()), &event);
        let FoldError::IncompleteRunner { defect } = error else {
            panic!("the {label} case was refused for another reason: {error}");
        };
        assert!(
            messages.insert(defect.clone()),
            "the {label} case reports what another case reports: {defect}"
        );
    }
}

#[test]
fn a_resume_that_established_a_different_runner_is_refused_field_by_field() {
    let fold = started();
    accepts(&fold, &resume(container_runner()));

    let cases: [(&str, &str, BreakRunner); 7] = [
        ("kind", "runner kind", |runner| {
            runner.kind = RunnerKind::Host;
        }),
        ("policy", "runner policy", |runner| {
            runner.policy = RunnerContract::HostV1;
        }),
        ("image presence", "presence of an image record", |runner| {
            runner.image = None;
        }),
        ("image reference", "image reference", |runner| {
            if let Some(image) = runner.image.as_mut() {
                image.reference = "ghcr.io/example/other:2.1".to_owned();
            }
        }),
        ("image id", "image id", |runner| {
            if let Some(image) = runner.image.as_mut() {
                image.id =
                    "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                        .to_owned();
            }
        }),
        ("image digest", "image digest", |runner| {
            if let Some(image) = runner.image.as_mut() {
                image.digest = None;
            }
        }),
        ("credential volumes", "credential volume set", |runner| {
            if let Some(volumes) = runner.credential_volumes.as_mut() {
                volumes.insert("copilot".to_owned(), "upstroke-creds-copilot".to_owned());
            }
        }),
    ];
    for (label, field, break_it) in cases {
        let mut runner = container_runner();
        break_it(&mut runner);
        assert_eq!(
            refuse(&fold, &resume(runner)),
            FoldError::RunnerMoved {
                field: field.to_owned()
            },
            "the {label} case"
        );
    }

    let mut reordered = container_runner();
    if let Some(volumes) = reordered.credential_volumes.as_mut() {
        let entries: Vec<(String, String)> = volumes.clone().into_iter().rev().collect();
        *volumes = entries.into_iter().collect();
    }
    accepts(&fold, &resume(reordered));
}

fn resume(runner: RunnerPolicy) -> TopologyEvent {
    ev(TopologyEventBody::RunResumed {
        data: Box::new(RunResumed4 {
            incarnation: IncarnationId("01J9AAAAAAAAAAAAAAAAAAAAAA".to_owned()),
            runner,
            probed_agents: probed_agents(),
            upstroke_version: "0.2.1-Ünicode".to_owned(),
        }),
    })
}

#[test]
fn a_resume_is_compared_with_run_started_by_value_and_by_agent() {
    let mut fold = started();
    accepts(&fold, &resume(container_runner()));

    let renamed = || {
        let mut runner = container_runner();
        if let Some(volumes) = runner.credential_volumes.as_mut() {
            volumes.insert(
                "claude-code".to_owned(),
                "upstroke-creds-renamed".to_owned(),
            );
        }
        runner
    };
    let swapped = || {
        let mut runner = container_runner();
        if let Some(volumes) = runner.credential_volumes.as_mut() {
            volumes.insert("claude-code".to_owned(), "upstroke-creds-codex".to_owned());
            volumes.insert(
                "  Codex-CLI  ".to_owned(),
                "upstroke-creds-Ünicode".to_owned(),
            );
        }
        runner
    };
    for (label, runner) in [
        ("a renamed volume", renamed()),
        ("swapped volumes", swapped()),
    ] {
        let original = container_runner()
            .credential_volumes
            .expect("the fixture mounts credentials");
        let moved = runner
            .credential_volumes
            .clone()
            .expect("the fixture mounts credentials");
        assert_eq!(
            moved.len(),
            original.len(),
            "{label} changed the cardinality"
        );
        assert_eq!(
            moved.keys().collect::<Vec<_>>(),
            original.keys().collect::<Vec<_>>(),
            "{label} changed the agent set"
        );
        assert!(
            matches!(
                refuse(&fold, &resume(runner)),
                FoldError::RunnerMoved { .. }
            ),
            "{label} re-established a runner the run never started with"
        );
    }

    apply(&mut fold, &resume(container_runner()));
    assert_eq!(fold.epoch(), Some(Epoch(1)));
    apply(&mut fold, &resume(container_runner()));
    assert_eq!(fold.epoch(), Some(Epoch(2)));
    assert!(matches!(
        refuse(&fold, &resume(renamed())),
        FoldError::RunnerMoved { .. }
    ));
    accepts(&fold, &resume(container_runner()));
    assert_eq!(
        fold.started().expect("started").runner,
        container_runner(),
        "the stored runner record is the one run_started froze"
    );
}

#[test]
fn both_recorded_digests_are_checked_against_the_frozen_inputs() {
    let moved_plan = ev(TopologyEventBody::RunStarted {
        data: Box::new(RunStarted4 {
            normalized_plan_digest: "sha256:0".to_owned() + &"1".repeat(63),
            ..run_started()
        }),
    });
    assert_eq!(
        refuse(&TopologyFold::new(inputs()), &moved_plan),
        FoldError::DigestMismatch {
            what: "normalized plan",
            recorded: "sha256:0".to_owned() + &"1".repeat(63),
            actual: NORMALIZED_DIGEST.to_owned(),
        }
    );

    let moved_registry = ev(TopologyEventBody::RunStarted {
        data: Box::new(RunStarted4 {
            registry_digest: "sha256:2".to_owned() + &"3".repeat(63),
            ..run_started()
        }),
    });
    let error = refuse(&TopologyFold::new(inputs()), &moved_registry);
    assert_eq!(
        error,
        FoldError::DigestMismatch {
            what: "registry",
            recorded: "sha256:2".to_owned() + &"3".repeat(63),
            actual: registry_digest(),
        }
    );

    let mut moved = plan();
    moved.tasks[0].body.push('!');
    let elsewhere = TopologyFold::new(FrozenInputs {
        plan: moved,
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
    });
    assert!(matches!(
        refuse(&elsewhere, &run_started_event()),
        FoldError::DigestMismatch {
            what: "registry",
            ..
        }
    ));

    let probed_elsewhere = ev(TopologyEventBody::RunStarted {
        data: Box::new(RunStarted4 {
            probed_agents: vec!["codex".to_owned()],
            ..run_started()
        }),
    });
    assert!(matches!(
        refuse(&TopologyFold::new(inputs()), &probed_elsewhere),
        FoldError::DigestMismatch {
            what: "registry",
            ..
        }
    ));

    let nudge = |value: &str| {
        let mut moved = value.to_owned();
        let last = moved.pop().expect("a digest has characters");
        moved.push(if last == '0' { '1' } else { '0' });
        moved
    };
    assert_ne!(registry_digest(), NORMALIZED_DIGEST);
    for (what, event) in [
        (
            "normalized plan",
            ev(TopologyEventBody::RunStarted {
                data: Box::new(RunStarted4 {
                    normalized_plan_digest: nudge(NORMALIZED_DIGEST),
                    ..run_started()
                }),
            }),
        ),
        (
            "registry",
            ev(TopologyEventBody::RunStarted {
                data: Box::new(RunStarted4 {
                    registry_digest: nudge(&registry_digest()),
                    ..run_started()
                }),
            }),
        ),
    ] {
        let error = refuse(&TopologyFold::new(inputs()), &event);
        assert!(
            matches!(&error, FoldError::DigestMismatch { what: named, .. } if *named == what),
            "a {what} digest differing in its last character alone was authenticated: \
                 {error:?}"
        );
    }
}

#[test]
fn a_malformed_ladder_is_refused_before_it_is_stored() {
    let cases: [(&str, BreakFrozenInputs); 3] = [
        ("floor above ceiling", |plan, chain| {
            plan.tasks[ZETA.index()].min_tier = Some(Tier::Frontier);
            chain.tiers = vec![Tier::Small, Tier::Mid];
            chain.bindings = Some(bindings_for(&chain.tiers));
        }),
        ("tiers that do not escalate", |_, chain| {
            chain.tiers = vec![Tier::Mid, Tier::Small, Tier::Frontier];
            chain.bindings = Some(bindings_for(&chain.tiers));
        }),
        ("a repeated tier", |_, chain| {
            chain.tiers = vec![Tier::Mid, Tier::Mid];
            chain.bindings = Some(bindings_for(&chain.tiers));
        }),
    ];
    let mut defects: BTreeSet<String> = BTreeSet::new();
    for (label, break_it) in cases {
        let (inputs, event) = run_started_with_ladder(break_it);
        let error = refuse(&TopologyFold::new(inputs), &event);
        let FoldError::MalformedLadder { key, defect } = error else {
            panic!("the {label} case was refused for another reason: {error}");
        };
        assert_eq!(key, ZETA.0, "the {label} case names the wrong task");
        assert!(
            defects.insert(defect.clone()),
            "the {label} case reports what another case reports: {defect}"
        );
    }

    let spawn_cases: [(&str, BreakLadder); 8] = [
        ("floor above ceiling", |ladder| {
            ladder.tiers = vec![Tier::Mid];
            ladder.rungs = rungs_for(&ladder.tiers);
            ladder.ceiling = Some(Tier::Mid);
            ladder.floor = Some(Tier::Frontier);
        }),
        ("tiers that do not escalate", |ladder| {
            ladder.tiers = vec![Tier::Frontier, Tier::Mid];
            ladder.rungs = rungs_for(&ladder.tiers);
            ladder.ceiling = Some(Tier::Frontier);
        }),
        ("a repeated tier", |ladder| {
            ladder.tiers = vec![Tier::Mid, Tier::Mid];
            ladder.rungs = rungs_for(&ladder.tiers);
            ladder.ceiling = Some(Tier::Mid);
        }),
        ("zero attempts per rung", |ladder| ladder.attempts_per = 0),
        ("a ceiling that is not the highest rung", |ladder| {
            ladder.ceiling = Some(Tier::Small);
        }),
        ("runnable with no rungs", |ladder| ladder.rungs.clear()),
        ("a human binding that already has rungs", |ladder| {
            ladder.admission = Admission::HumanBinding {
                options: vec!["  Codex-CLI  ".to_owned()],
            };
        }),
        ("a rung bound at another tier", |ladder| {
            ladder.rungs[0].tier = Tier::Small;
        }),
    ];
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    let mut spawn_defects: BTreeSet<String> = BTreeSet::new();
    for (label, break_it) in spawn_cases {
        let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);
        break_it(&mut spawn.entry.ladder);
        let error = refuse(&fold, &spawn_event(spawn));
        let FoldError::MalformedLadder { key, defect } = error else {
            panic!("the {label} spawn case was refused for another reason: {error}");
        };
        assert_eq!(key, 3, "the {label} spawn case names the wrong task");
        assert!(
            spawn_defects.insert(defect.clone()),
            "the {label} spawn case reports what another reports: {defect}"
        );
    }

    let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    clip_to_human_binding(&mut spawn, vec!["  Codex-CLI  ".to_owned()]);
    accepts(&fold, &spawn_event(spawn.clone()));
    clip_to_human_binding(&mut spawn, Vec::new());
    assert!(matches!(
        refuse(&fold, &spawn_event(spawn)),
        FoldError::MalformedLadder { key: 3, .. }
    ));
}

fn bindings_for(tiers: &[Tier]) -> Vec<BindingSummary> {
    tiers
        .iter()
        .map(|tier| BindingSummary {
            tier: *tier,
            agent: format!("zeta-{tier}-agent"),
            model: format!("zeta-{tier}-model"),
            pinned: *tier == Tier::Frontier,
        })
        .collect()
}

fn rungs_for(tiers: &[Tier]) -> Vec<FrozenRung> {
    tiers
        .iter()
        .map(|tier| FrozenRung {
            tier: *tier,
            agent: format!("repair-{tier}-agent"),
            model: format!("repair-{tier}-model"),
            pinned: *tier == Tier::Frontier,
        })
        .collect()
}

fn run_started_with_ladder(break_it: BreakFrozenInputs) -> (FrozenInputs, TopologyEvent) {
    let started = run_started();
    let mut plan = plan();
    let mut chains = started.chains.clone();
    let index = chains
        .iter()
        .position(|chain| chain.task == "zeta")
        .expect("zeta's chain");
    break_it(&mut plan, &mut chains[index]);
    let record = RunStarted4 { chains, ..started };
    let digest = TaskRegistry::originals_with_agents(
        &plan,
        &record.registry_record(),
        &record.probed_agents,
    )
    .expect("the derivation accepts every ladder in this table")
    .digest();
    (
        FrozenInputs {
            plan,
            normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
        },
        ev(TopologyEventBody::RunStarted {
            data: Box::new(RunStarted4 {
                registry_digest: digest,
                ..record
            }),
        }),
    )
}

fn repair_spawn(key: TaskKey, root: TaskKey, parent: TaskKey) -> FrozenSpawn {
    FrozenSpawn {
        key,
        entry: TaskEntry {
            key,
            display_id: TaskId::from(
                crate::topology::registry::repair_display_id(0, &TaskId::from("alpha")).as_str(),
            ),
            origin: Origin::MergeRepair,
            spec: FrozenTaskSpec {
                kind: TaskKind::Fix,
                title: "  Repair the alpha rejection — Ünicode  ".to_owned(),
                body: "Conflict against `src/Zebra/ÜBER.rs`; preserve merged behaviour.".to_owned(),
                acceptance: vec!["the conflict is resolved".to_owned()],
                path_hints: vec!["src/repairs/".to_owned()],
                suggested_tier: Some(Tier::Frontier),
                min_tier: Some(Tier::Mid),
                artifacts_in: vec![ArtifactId::from("contract")],
                artifacts_out: vec![ArtifactId::from("repair-out")],
            },
            deps: vec![parent],
            display_deps: vec![TaskId::from("alpha")],
            ladder: FrozenLadder {
                tiers: vec![Tier::Mid, Tier::Frontier],
                attempts_per: 4,
                rungs: rungs_for(&[Tier::Mid, Tier::Frontier]),
                floor: Some(Tier::Mid),
                ceiling: Some(Tier::Frontier),
                effort: effort_policy(),
                admission: Admission::Runnable,
            },
            reviews: FrozenReviews {
                enabled: true,
                alternative_available: true,
                pass_timeout_secs: 1_337,
                primary: Some(PassBinding::new("claude-code", "claude-opus-5")),
                alternative: Some(PassBinding::new("copilot", "gpt-5.6")),
                second_opinion: None,
            },
            allowed_agents: probed_agents(),
            lineage: Some(Lineage {
                root,
                parent,
                index: 0,
            }),
        },
        admission: SpawnAdmission::Runnable,
    }
}

fn spawn_event(spawn: FrozenSpawn) -> TopologyEvent {
    ev(TopologyEventBody::TaskSpawned {
        data: Box::new(TaskSpawned { spawn }),
    })
}

fn clip_to_human_binding(spawn: &mut FrozenSpawn, options: Vec<String>) {
    spawn.entry.ladder.rungs.clear();
    spawn.entry.ladder.admission = Admission::HumanBinding {
        options: options.clone(),
    };
    spawn.admission = SpawnAdmission::HumanBinding {
        options,
        question: question("q-binding-Ünicode", spawn.key),
    };
}

#[test]
fn a_registered_entry_is_the_entry_the_event_registers() {
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    accepts(&fold, &spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)));

    let cases: [(&str, BreakSpawn); 9] = [
        ("a key that is not the next dense index", |spawn| {
            spawn.key = TaskKey(4);
            spawn.entry.key = TaskKey(4);
        }),
        ("an entry that calls itself something else", |spawn| {
            spawn.entry.key = TaskKey(7);
        }),
        ("a display id another task already has", |spawn| {
            spawn.entry.display_id = TaskId::from("alpha");
        }),
        ("no lineage", |spawn| spawn.entry.lineage = None),
        ("a lineage root that refers forwards", |spawn| {
            spawn.entry.lineage = Some(Lineage {
                root: TaskKey(3),
                parent: ALPHA,
                index: 0,
            });
        }),
        ("a lineage parent that refers forwards", |spawn| {
            spawn.entry.lineage = Some(Lineage {
                root: ALPHA,
                parent: TaskKey(9),
                index: 0,
            });
        }),
        ("an allow-list the run never probed", |spawn| {
            spawn.entry.allowed_agents.push("smuggled-agent".to_owned());
        }),
        ("a dependency named as another task", |spawn| {
            spawn.entry.display_deps = vec![TaskId::from("zeta")];
        }),
        ("a dependency that is not merged", |spawn| {
            spawn.entry.deps = vec![ZETA];
            spawn.entry.display_deps = vec![TaskId::from("zeta")];
        }),
    ];
    let mut messages: BTreeSet<String> = BTreeSet::new();
    for (label, break_it) in cases {
        let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);
        break_it(&mut spawn);
        let error = refuse(&fold, &spawn_event(spawn));
        assert!(
            messages.insert(error.to_string()),
            "the {label} case reports what another case reports: {error}"
        );
    }

    let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    spawn.entry.display_deps.push(TaskId::from("zeta"));
    assert!(matches!(
        refuse(&fold, &spawn_event(spawn)),
        FoldError::MalformedEntry { key: 3, .. }
    ));
}

#[test]
fn a_spawns_admission_and_its_entrys_admission_are_one_statement() {
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);

    let mut human_required = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    human_required.admission = SpawnAdmission::HumanRequired {
        limit: 1,
        question: question("q-admission-Ünicode", TaskKey(3)),
    };
    accepts(&fold, &spawn_event(human_required.clone()));

    let mut wrong_limit = human_required.clone();
    wrong_limit.admission = SpawnAdmission::HumanRequired {
        limit: 5,
        question: question("q-admission-Ünicode", TaskKey(3)),
    };
    assert!(matches!(
        refuse(&fold, &spawn_event(wrong_limit)),
        FoldError::MalformedEntry { key: 3, .. }
    ));

    let mut clipped = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    clip_to_human_binding(&mut clipped, vec!["  Codex-CLI  ".to_owned()]);
    let mut disagreeing = clipped.clone();
    disagreeing.admission = SpawnAdmission::HumanBinding {
        options: vec!["copilot".to_owned()],
        question: question("q-binding-Ünicode", TaskKey(3)),
    };
    assert!(matches!(
        refuse(&fold, &spawn_event(disagreeing)),
        FoldError::MalformedEntry { key: 3, .. }
    ));

    let mut runnable_over_clipped = clipped.clone();
    runnable_over_clipped.admission = SpawnAdmission::Runnable;
    assert!(matches!(
        refuse(&fold, &spawn_event(runnable_over_clipped)),
        FoldError::MalformedEntry { key: 3, .. }
    ));

    let mut binding_over_runnable = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    binding_over_runnable.admission = SpawnAdmission::HumanBinding {
        options: vec!["  Codex-CLI  ".to_owned()],
        question: question("q-binding-Ünicode", TaskKey(3)),
    };
    assert!(matches!(
        refuse(&fold, &spawn_event(binding_over_runnable)),
        FoldError::MalformedEntry { key: 3, .. }
    ));

    let mut unanswerable = clipped;
    unanswerable.admission = SpawnAdmission::HumanBinding {
        options: vec!["  Codex-CLI  ".to_owned()],
        question: FrozenQuestion {
            options: Vec::new(),
            ..question("q-binding-Ünicode", TaskKey(3))
        },
    };
    assert!(matches!(
        refuse(&fold, &spawn_event(unanswerable)),
        FoldError::UnanswerableQuestion { .. }
    ));
}

#[test]
fn a_spawn_parks_exactly_when_its_admission_needs_a_person() {
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    let mut runnable = fold.clone();
    apply(
        &mut runnable,
        &spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)),
    );
    assert_eq!(runnable.task_state(TaskKey(3)), Some(TaskState::Pending));
    assert!(runnable.open_questions().expect("started").is_empty());

    let mut clipped_fold = fold.clone();
    let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    clip_to_human_binding(&mut spawn, vec!["  Codex-CLI  ".to_owned()]);
    apply(&mut clipped_fold, &spawn_event(spawn));
    assert_eq!(
        clipped_fold.task_state(TaskKey(3)),
        Some(TaskState::AwaitingInput)
    );
    assert_eq!(clipped_fold.open_questions().expect("started").len(), 1);
}

#[test]
fn a_dispatch_opens_one_dense_generation_of_a_pending_task() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));

    assert!(matches!(
        refuse(&fold, &dispatch(ZETA, 1, &base)),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    apply(
        &mut fold,
        &settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Retry,
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    );
    assert!(matches!(
        refuse(&fold, &dispatch(ZETA, 2, &base)),
        FoldError::NonDenseKey { key: 2, len: 1, .. }
    ));
    accepts(&fold, &dispatch(ZETA, 1, &base));

    let mut merged_fold = started();
    merge_task(&mut merged_fold, ALPHA, 0, 0);
    assert!(matches!(
        refuse(&merged_fold, &dispatch(ALPHA, 1, &base)),
        FoldError::WrongTaskState {
            key: 1,
            state: "merged",
            ..
        }
    ));
    assert!(matches!(
        refuse(&merged_fold, &dispatch(TaskKey(9), 0, &base)),
        FoldError::UnknownKey { key: 9, .. }
    ));
}

struct HintShape {
    id: &'static str,
    hints: &'static [&'static str],
    derives: Option<&'static [&'static str]>,
}

const HINT_SHAPES: &[HintShape] = &[
    HintShape {
        id: "literal",
        hints: &["src/literal"],
        derives: Some(&["src/literal"]),
    },
    HintShape {
        id: "trailing",
        hints: &["src/trailing/"],
        derives: Some(&["src/trailing"]),
    },
    HintShape {
        id: "star",
        hints: &["src/star/*.rs"],
        derives: Some(&["src/star"]),
    },
    HintShape {
        id: "question",
        hints: &["src/question/?.rs"],
        derives: Some(&["src/question"]),
    },
    HintShape {
        id: "bracket",
        hints: &["src/bracket/[ab].rs"],
        derives: Some(&["src/bracket"]),
    },
    HintShape {
        id: "brace",
        hints: &["src/brace/{a,b}.rs"],
        derives: Some(&["src/brace"]),
    },
    HintShape {
        id: "backslash",
        hints: &[r"src\backslash\deep"],
        derives: Some(&["src/backslash/deep"]),
    },
    HintShape {
        id: "doubled",
        hints: &["src/doubled//inner/"],
        derives: Some(&["src/doubled//inner"]),
    },
    HintShape {
        id: "unicode",
        hints: &["src/Über/"],
        derives: Some(&["src/Über"]),
    },
    HintShape {
        id: "several",
        hints: &["zz/last", "aa/first", "build.rs"],
        derives: Some(&["zz/last", "aa/first", "build.rs"]),
    },
    HintShape {
        id: "leading-glob",
        hints: &["**/anywhere.rs"],
        derives: None,
    },
    HintShape {
        id: "empty-hint",
        hints: &[""],
        derives: None,
    },
    HintShape {
        id: "bare-separator",
        hints: &["/"],
        derives: None,
    },
    HintShape {
        id: "one-narrow-one-wide",
        hints: &["src/narrow", "**/wide.rs"],
        derives: None,
    },
    HintShape {
        id: "no-hints",
        hints: &[],
        derives: None,
    },
];

fn hint_shape_region(shape: &HintShape) -> PathSet {
    match shape.derives {
        None => PathSet::RepoWide,
        Some(prefixes) => PathSet::Prefixes {
            paths: prefixes.iter().copied().map(GitPath::from).collect(),
        },
    }
}

fn hint_shape_plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "frozen-hint-shape-hash".to_owned(),
        },
        tasks: HINT_SHAPES
            .iter()
            .map(|shape| task_of(shape.id, &[], shape.hints, None))
            .collect(),
        artifacts: vec![Artifact {
            id: ArtifactId::from("contract"),
            produced_by: Some(TaskId::from(HINT_SHAPES[0].id)),
        }],
    }
}

fn hint_shape_inputs() -> FrozenInputs {
    FrozenInputs {
        plan: hint_shape_plan(),
        normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
    }
}

fn hint_shape_started() -> TopologyFold {
    let plan = hint_shape_plan();
    let unauthenticated = RunStarted4 {
        plan_hash: plan.source.hash.clone(),
        chains: plan.tasks.iter().map(|t| chain(t.id.as_str())).collect(),
        reviews: review_plan(plan.tasks.len()),
        registry_digest: String::new(),
        ..run_started_unauthenticated()
    };
    let digest = TaskRegistry::originals_with_agents(
        &plan,
        &unauthenticated.registry_record(),
        &unauthenticated.probed_agents,
    )
    .expect("the hint-shape record derives a registry")
    .digest();
    let event = ev(TopologyEventBody::RunStarted {
        data: Box::new(RunStarted4 {
            registry_digest: digest,
            ..unauthenticated
        }),
    });
    let mut fold = TopologyFold::new(hint_shape_inputs());
    apply(&mut fold, &event);
    fold
}

fn hint_shape_dispatch(key: TaskKey, paths: PathSet) -> TopologyEvent {
    ev(TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key,
            generation: GenerationId(0),
            base_sha: sha("base"),
            worktree_path: format!("/private/workspaces/tasks/k{}-g0", key.0),
            lease: LeaseGrant::Predicted { paths },
            source_candidate: None,
        },
    })
}

#[test]
fn every_hint_shape_derives_the_region_the_rule_states_and_the_door_takes_it() {
    let fold = hint_shape_started();
    for (index, shape) in HINT_SHAPES.iter().enumerate() {
        let key = TaskKey(u32::try_from(index).expect("a small fixture registry"));
        let expected = hint_shape_region(shape);
        assert_eq!(
            fold.predicted_region(key),
            Some(expected.clone()),
            "`{}`: the derivation is not the region the rule states",
            shape.id
        );
        accepts(&fold, &hint_shape_dispatch(key, expected));
    }
    assert!(
        HINT_SHAPES.iter().any(|shape| shape.derives.is_none())
            && HINT_SHAPES.iter().any(|shape| shape.derives.is_some()),
        "a table with only one answer could not tell the two classifications apart"
    );
}

#[test]
fn a_dispatch_that_records_a_region_the_hints_do_not_derive_is_refused() {
    let fold = hint_shape_started();
    let key_of = |id: &str| {
        TaskKey(
            u32::try_from(
                HINT_SHAPES
                    .iter()
                    .position(|shape| shape.id == id)
                    .expect("a fixture shape"),
            )
            .expect("a small fixture registry"),
        )
    };
    let narrowed = |paths: &[&str]| PathSet::Prefixes {
        paths: paths.iter().copied().map(GitPath::from).collect(),
    };

    let cases: Vec<(&str, TaskKey, PathSet)> = vec![
        (
            "the hint, unstripped",
            key_of("star"),
            narrowed(&["src/star/*.rs"]),
        ),
        (
            "a component missing",
            key_of("several"),
            narrowed(&["zz/last", "aa/first"]),
        ),
        (
            "a component added",
            key_of("several"),
            narrowed(&["zz/last", "aa/first", "build.rs", "src/extra"]),
        ),
        (
            "the components reordered",
            key_of("several"),
            narrowed(&["aa/first", "build.rs", "zz/last"]),
        ),
        (
            "a separator normalised away",
            key_of("doubled"),
            narrowed(&["src/doubled/inner"]),
        ),
        (
            "the case folded",
            key_of("unicode"),
            narrowed(&["src/über"]),
        ),
        ("widened to repo-wide", key_of("literal"), PathSet::RepoWide),
        (
            "narrowed from repo-wide",
            key_of("leading-glob"),
            narrowed(&["src/anywhere"]),
        ),
        ("emptied", key_of("literal"), narrowed(&[])),
    ];

    for (label, key, recorded) in cases {
        let derived = fold
            .predicted_region(key)
            .expect("the fixture run has started");
        assert_ne!(
            recorded, derived,
            "{label}: the perturbation is not a perturbation"
        );
        let error = refuse(&fold, &hint_shape_dispatch(key, recorded));
        let FoldError::MalformedEntry {
            key: named, detail, ..
        } = &error
        else {
            panic!("{label}: refused as {error} rather than as a malformed entry");
        };
        assert_eq!(*named, key.0, "{label}: the refusal names another task");
        assert!(
            detail.contains("frozen path hints derive"),
            "{label}: the refusal does not say what it derived: {detail}"
        );
    }
}

#[test]
fn the_dispatch_fixture_records_the_region_the_fold_derives() {
    let fold = started();
    for key in [ZETA, ALPHA, MID] {
        let event = dispatch(key, 0, &sha("base"));
        let TopologyEventBody::TaskDispatched { data } = &event.body else {
            panic!("the dispatch fixture builds a task_dispatched");
        };
        let LeaseGrant::Predicted { paths } = &data.lease else {
            panic!("an ordinary dispatch takes a predicted lease");
        };
        assert_eq!(
            Some(paths.clone()),
            fold.predicted_region(key),
            "the fixture region for task {} is not the one the fold derives",
            key.0
        );
    }
}

#[test]
fn a_dispatch_takes_the_holding_its_origin_implies() {
    let base = sha("base");
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    apply(
        &mut fold,
        &spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)),
    );

    let repair_dispatch = |lease: LeaseGrant, source: Option<CandidateRef>| {
        ev(TopologyEventBody::TaskDispatched {
            data: TaskDispatched {
                key: TaskKey(3),
                generation: GenerationId(0),
                base_sha: base.clone(),
                worktree_path: "/private/workspaces/tasks/k3-g0".to_owned(),
                lease,
                source_candidate: source,
            },
        })
    };
    accepts(
        &fold,
        &repair_dispatch(
            LeaseGrant::InheritedLineage { root: ALPHA },
            Some(candidate_of(ALPHA, 0)),
        ),
    );
    assert!(matches!(
        refuse(
            &fold,
            &repair_dispatch(
                LeaseGrant::Predicted {
                    paths: region(TaskKey(3))
                },
                Some(candidate_of(ALPHA, 0))
            )
        ),
        FoldError::MalformedEntry { key: 3, .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &repair_dispatch(
                LeaseGrant::InheritedLineage { root: ZETA },
                Some(candidate_of(ALPHA, 0))
            )
        ),
        FoldError::MalformedEntry { key: 3, .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &repair_dispatch(LeaseGrant::InheritedLineage { root: ALPHA }, None)
        ),
        FoldError::MalformedEntry { key: 3, .. }
    ));

    let ordinary = ev(TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key: ZETA,
            generation: GenerationId(0),
            base_sha: base.clone(),
            worktree_path: "/private/workspaces/tasks/k0-g0".to_owned(),
            lease: LeaseGrant::InheritedLineage { root: ALPHA },
            source_candidate: None,
        },
    });
    assert!(matches!(
        refuse(&fold, &ordinary),
        FoldError::MalformedEntry { key: 0, .. }
    ));
    let materializing = ev(TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key: ZETA,
            generation: GenerationId(0),
            base_sha: base,
            worktree_path: "/private/workspaces/tasks/k0-g0".to_owned(),
            lease: LeaseGrant::Predicted {
                paths: region(ZETA),
            },
            source_candidate: Some(candidate_of(ALPHA, 0)),
        },
    });
    assert!(matches!(
        refuse(&fold, &materializing),
        FoldError::MalformedEntry { key: 0, .. }
    ));
}

#[test]
fn an_attempt_starts_in_the_open_generation_at_the_next_number() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));

    let elsewhere = attempt_started(&fold, ZETA, 1, 1, 0);
    assert!(matches!(
        refuse(&fold, &elsewhere),
        FoldError::NotTheOpenGeneration {
            key: 0,
            generation: 1,
            ..
        }
    ));
    let unopened = attempt_started(&fold, ALPHA, 0, 1, 0);
    assert!(matches!(
        refuse(&fold, &unopened),
        FoldError::NotTheOpenGeneration { key: 1, .. }
    ));

    for attempt in [0, 2, 7] {
        let event = attempt_started(&fold, ZETA, 0, attempt, 0);
        assert_eq!(
            refuse(&fold, &event),
            FoldError::WrongAttempt {
                kind: "attempt_started",
                key: 0,
                generation: 0,
                attempt,
                expected: "1".to_owned(),
            }
        );
    }
    let first = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &first);

    assert!(matches!(
        refuse(&fold, &attempt_started(&fold, ZETA, 0, 2, 0)),
        FoldError::NotTheOpenGeneration { .. }
    ));
    apply(
        &mut fold,
        &settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                retained_incarnation: Epoch(0),
            },
        ),
    );
    let resumed = |attempt: u32, session: &str, generation: u32| {
        ev(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key: ZETA,
                generation: GenerationId(generation),
                attempt: AttemptNumber(attempt),
                rung: 0,
                binding: frozen_binding(&fold, ZETA, 0),
                pool: Some("codex-plus".to_owned()),
                resume_session: Some(SessionId(session.to_owned())),
                materialization_observed: None,
            },
        })
    };
    assert_eq!(
        refuse(&fold, &resumed(3, "sess-ÜNI-0042", 0)),
        FoldError::WrongAttempt {
            kind: "attempt_started",
            key: 0,
            generation: 0,
            attempt: 3,
            expected: "2".to_owned(),
        }
    );
    accepts(&fold, &resumed(2, "sess-ÜNI-0042", 0));
}

#[test]
fn a_retained_session_belongs_to_the_incarnation_that_retained_it() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    assert!(matches!(
        refuse(
            &fold,
            &settle(
                ZETA,
                0,
                1,
                AttemptSettlement::Retained {
                    retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                    retained_incarnation: Epoch(4),
                }
            )
        ),
        FoldError::StaleIncarnation { key: 0, .. }
    ));
    apply(
        &mut fold,
        &settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                retained_incarnation: Epoch(0),
            },
        ),
    );

    let resume_with = |fold: &TopologyFold, session: &str| {
        ev(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key: ZETA,
                generation: GenerationId(0),
                attempt: AttemptNumber(2),
                rung: 0,
                binding: frozen_binding(fold, ZETA, 0),
                pool: Some("codex-plus".to_owned()),
                resume_session: Some(SessionId(session.to_owned())),
                materialization_observed: None,
            },
        })
    };
    assert!(matches!(
        refuse(&fold, &resume_with(&fold, "sess-other")),
        FoldError::StaleIncarnation { key: 0, .. }
    ));
    accepts(&fold, &resume_with(&fold, "sess-ÜNI-0042"));

    let mut next_epoch = fold.clone();
    apply(&mut next_epoch, &resume(container_runner()));
    assert_eq!(next_epoch.epoch(), Some(Epoch(1)));
    let error = refuse(&next_epoch, &resume_with(&next_epoch, "sess-ÜNI-0042"));
    let FoldError::StaleIncarnation { detail, .. } = error else {
        panic!("a stale incarnation must be refused as one");
    };
    assert!(
        detail.contains("incarnation 0") && detail.contains("1 time(s)"),
        "the refusal has to say which incarnation retained it: {detail}"
    );

    assert!(matches!(
        refuse(&fold, &attempt_started(&fold, ZETA, 0, 2, 0)),
        FoldError::NotTheOpenGeneration { .. }
    ));
    let mut fresh = started();
    apply(&mut fresh, &dispatch(ALPHA, 0, &base));
    let mistaken = ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: ALPHA,
            generation: GenerationId(0),
            attempt: AttemptNumber(1),
            rung: 0,
            binding: frozen_binding(&fresh, ALPHA, 0),
            pool: None,
            resume_session: Some(SessionId("sess-invented".to_owned())),
            materialization_observed: None,
        },
    });
    assert!(matches!(
        refuse(&fresh, &mistaken),
        FoldError::NotTheOpenGeneration { key: 1, .. }
    ));
}

#[test]
fn an_attempt_runs_the_frozen_binding_or_the_validated_override() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let exact = attempt_started(&fold, ZETA, 0, 1, 0);
    accepts(&fold, &exact);

    let cases: [(&str, BreakBinding); 4] = [
        ("agent", |binding| binding.agent = "copilot".to_owned()),
        ("model", |binding| {
            binding.model = "another-model".to_owned()
        }),
        ("tier", |binding| binding.tier = Tier::Frontier),
        ("effort", |binding| binding.effort = Effort::Medium),
    ];
    for (label, break_it) in cases {
        let mut binding = frozen_binding(&fold, ZETA, 0);
        break_it(&mut binding);
        let event = ev(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key: ZETA,
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                rung: 0,
                binding,
                pool: Some("codex-plus".to_owned()),
                resume_session: None,
                materialization_observed: None,
            },
        });
        assert!(
            matches!(
                refuse(&fold, &event),
                FoldError::BindingMismatch { key: 0, .. }
            ),
            "the {label} case ran a binding the run never froze and was folded anyway"
        );
    }

    let mut off_the_end = attempt_started(&fold, ZETA, 0, 1, 0);
    if let TopologyEventBody::AttemptStarted { data } = &mut off_the_end.body {
        data.rung = 9;
    }
    assert!(matches!(
        refuse(&fold, &off_the_end),
        FoldError::WrongRung { rung: 9, .. }
    ));

    let mut materializing = attempt_started(&fold, ZETA, 0, 1, 0);
    if let TopologyEventBody::AttemptStarted { data } = &mut materializing.body {
        data.materialization_observed = Some(Materialization::Clean);
    }
    assert!(matches!(
        refuse(&fold, &materializing),
        FoldError::MalformedEntry { key: 0, .. }
    ));

    let rungs = u32::try_from(
        fold.registry()
            .expect("started")
            .get(ZETA)
            .expect("zeta")
            .ladder
            .rungs
            .len(),
    )
    .expect("a small fixture ladder");
    assert_eq!(rungs, 3, "zeta's fixture ladder has three rungs to walk");
    for rung in 0..rungs {
        let generation = rung;
        accepts(&fold, &attempt_started(&fold, ZETA, generation, 1, rung));
        let entry = fold.registry().expect("started").get(ZETA).expect("zeta");
        let tier = entry
            .ladder
            .rungs
            .get(rung as usize)
            .expect("the walk stays on the ladder")
            .tier;
        let mut wrong_effort = frozen_binding(&fold, ZETA, rung as usize);
        wrong_effort.effort = entry.ladder.effort.review;
        assert_ne!(
            wrong_effort.effort,
            entry.ladder.effort.implementation_for(tier),
            "the fixture's review effort must differ from every rung's, or this proves nothing"
        );
        let event = ev(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key: ZETA,
                generation: GenerationId(generation),
                attempt: AttemptNumber(1),
                rung,
                binding: wrong_effort,
                pool: None,
                resume_session: None,
                materialization_observed: None,
            },
        });
        assert!(matches!(
            refuse(&fold, &event),
            FoldError::BindingMismatch { .. }
        ));
        if rung + 1 < rungs {
            let climb = attempt_started(&fold, ZETA, generation, 1, rung);
            apply(&mut fold, &climb);
            apply(
                &mut fold,
                &settle(
                    ZETA,
                    generation,
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Escalated { rung: rung + 1 },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            );
            apply(&mut fold, &dispatch(ZETA, generation + 1, &base));
        }
    }
}

#[test]
fn an_override_is_the_binding_the_frozen_admission_authorized_and_no_other() {
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    let options = vec!["  Codex-CLI  ".to_owned(), "copilot".to_owned()];
    clip_to_human_binding(&mut spawn, options.clone());
    apply(&mut fold, &spawn_event(spawn));

    let override_for = |option_index: u32, agent: &str| BindingOverride {
        key: TaskKey(3),
        question: QuestionId::from("q-binding-Ünicode"),
        option_index,
        agent: agent.to_owned(),
        model: "gpt-5.6".to_owned(),
        effort: Effort::XHigh,
    };
    let answer = |option_index: u32, binding: Option<BindingOverride>| {
        answered(
            TaskKey(3),
            "q-binding-Ünicode",
            Answer4::Answered {
                option_index,
                binding_override: binding,
            },
        )
    };

    for (index, agent) in options.iter().enumerate() {
        let index = u32::try_from(index).expect("two options");
        accepts(&fold, &answer(index, Some(override_for(index, agent))));
    }

    for (label, index, agent) in [
        ("the other option's agent", 0_u32, "copilot"),
        ("the other option's agent", 1, "  Codex-CLI  "),
        ("an unauthorized agent", 0, "claude-code"),
        ("an unauthorized agent", 1, "ÜBER-agent-Ωmega"),
    ] {
        assert!(
            matches!(
                refuse(&fold, &answer(index, Some(override_for(index, agent)))),
                FoldError::WrongQuestion { .. }
            ),
            "{label}: option {index} authorized `{}` and `{agent}` was installed anyway",
            options[index as usize]
        );
    }

    assert!(matches!(
        refuse(&fold, &answer(0, None)),
        FoldError::WrongQuestion { .. }
    ));

    apply(&mut fold, &raised("q-park-Ünicode", ZETA));
    let smuggled = answered(
        ZETA,
        "q-park-Ünicode",
        Answer4::Answered {
            option_index: 1,
            binding_override: Some(BindingOverride {
                key: ZETA,
                question: QuestionId::from("q-park-Ünicode"),
                option_index: 1,
                agent: "ÜBER-agent-Ωmega".to_owned(),
                model: "a-model-nobody-froze".to_owned(),
                effort: Effort::XHigh,
            }),
        },
    );
    assert!(
        matches!(refuse(&fold, &smuggled), FoldError::WrongQuestion { .. }),
        "an ordinary park installed a binding its admission never authorized"
    );
    accepts(
        &fold,
        &answered(
            ZETA,
            "q-park-Ünicode",
            Answer4::Answered {
                option_index: 1,
                binding_override: None,
            },
        ),
    );
    assert_eq!(
        fold.binding_override(ZETA),
        None,
        "no refused override was installed"
    );

    let mut required = repair_spawn(TaskKey(4), ALPHA, ALPHA);
    required.entry.display_id = TaskId::from(
        crate::topology::registry::repair_display_id(1, &TaskId::from("alpha")).as_str(),
    );
    required.admission = SpawnAdmission::HumanRequired {
        limit: 1,
        question: question("q-required-Ünicode", TaskKey(4)),
    };
    apply(&mut fold, &spawn_event(required));
    assert!(matches!(
        refuse(
            &fold,
            &answered(
                TaskKey(4),
                "q-required-Ünicode",
                Answer4::Answered {
                    option_index: 0,
                    binding_override: Some(BindingOverride {
                        key: TaskKey(4),
                        question: QuestionId::from("q-required-Ünicode"),
                        option_index: 0,
                        agent: "  Codex-CLI  ".to_owned(),
                        model: "gpt-5.6".to_owned(),
                        effort: Effort::XHigh,
                    }),
                },
            )
        ),
        FoldError::WrongQuestion { .. }
    ));
}

#[test]
fn an_interruption_closes_its_generation_and_returns_its_task_to_pending() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    assert!(
        fold.leases()
            .expect("started")
            .holds(LeaseOwner::Generation {
                key: ZETA,
                generation: GenerationId(0),
            })
    );

    let interrupt = |lease| {
        ev(TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key: ZETA,
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                lease,
                detail: "  the coordinator died  ".to_owned(),
            },
        })
    };
    assert!(matches!(
        refuse(&fold, &interrupt(LeaseDisposition::PredictedRetained)),
        FoldError::InvalidLeaseDisposition { .. }
    ));
    assert!(matches!(
        refuse(&fold, &interrupt(LeaseDisposition::LineageHeld)),
        FoldError::InvalidLeaseDisposition { .. }
    ));
    apply(&mut fold, &interrupt(LeaseDisposition::PredictedReleased));

    assert_eq!(fold.task_state(ZETA), Some(TaskState::Pending));
    assert!(
        !fold
            .leases()
            .expect("started")
            .holds(LeaseOwner::Generation {
                key: ZETA,
                generation: GenerationId(0),
            }),
        "the ordinary lease survived a generation that closed"
    );
    let task = fold.task(ZETA).expect("zeta");
    assert!(
        task.open().is_none(),
        "the interrupted generation is still open"
    );
    assert_eq!(task.generations.len(), 1);

    assert!(matches!(
        refuse(
            &fold,
            &ev(TopologyEventBody::GenerationClosed {
                data: GenerationClosed {
                    key: ZETA,
                    generation: GenerationId(0),
                    reason: GenerationCloseReason::WorktreeMissing,
                    lease: LeaseDisposition::PredictedReleased,
                },
            })
        ),
        FoldError::NotTheOpenGeneration { .. }
    ));
    assert!(matches!(
        refuse(&fold, &attempt_started(&fold, ZETA, 0, 2, 0)),
        FoldError::NotTheOpenGeneration { .. }
    ));
    assert!(matches!(
        refuse(&fold, &dispatch(ZETA, 0, &base)),
        FoldError::NonDenseKey { .. }
    ));
    accepts(&fold, &dispatch(ZETA, 1, &base));

    apply(&mut fold, &dispatch(ZETA, 1, &base));
    let close = |generation: u32| {
        ev(TopologyEventBody::GenerationClosed {
            data: GenerationClosed {
                key: ZETA,
                generation: GenerationId(generation),
                reason: GenerationCloseReason::WorktreeMissing,
                lease: LeaseDisposition::PredictedReleased,
            },
        })
    };
    for stale in [0_u32, 2, 9] {
        assert!(
            matches!(
                refuse(&fold, &close(stale)),
                FoldError::NotTheOpenGeneration { .. }
            ),
            "a close naming generation {stale} was applied while 1 was the open one"
        );
    }
    let before = fold.task(ZETA).expect("zeta").generations.clone();
    let _ = fold.plan_transition(&close(0));
    assert_eq!(
        fold.task(ZETA).expect("zeta").generations,
        before,
        "a refused close changed the generation it was refused about"
    );
    accepts(&fold, &close(1));

    let mut lineage = started();
    merge_task(&mut lineage, ALPHA, 0, 0);
    apply(
        &mut lineage,
        &spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)),
    );
    apply(
        &mut lineage,
        &ev(TopologyEventBody::TaskDispatched {
            data: TaskDispatched {
                key: TaskKey(3),
                generation: GenerationId(0),
                base_sha: base.clone(),
                worktree_path: "/private/workspaces/tasks/k3-g0".to_owned(),
                lease: LeaseGrant::InheritedLineage { root: ALPHA },
                source_candidate: Some(candidate_of(ALPHA, 0)),
            },
        }),
    );
    let repair_start = ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: TaskKey(3),
            generation: GenerationId(0),
            attempt: AttemptNumber(1),
            rung: 0,
            binding: frozen_binding(&lineage, TaskKey(3), 0),
            pool: None,
            resume_session: None,
            materialization_observed: Some(Materialization::Clean),
        },
    });
    apply(&mut lineage, &repair_start);
    let held = lineage.leases().cloned();
    apply(
        &mut lineage,
        &ev(TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key: TaskKey(3),
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                lease: LeaseDisposition::LineageHeld,
                detail: "  the coordinator died  ".to_owned(),
            },
        }),
    );
    assert_eq!(lineage.task_state(TaskKey(3)), Some(TaskState::Pending));
    assert!(lineage.task(TaskKey(3)).expect("repair").open().is_none());
    assert_eq!(
        lineage.leases().cloned(),
        held,
        "an interrupted repair changed a holding, and a lineage member holds none of its own"
    );
}

#[test]
fn an_override_replaces_the_frozen_binding_for_every_later_attempt() {
    let base = sha("base");
    let mut fold = started();
    let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    merge_task(&mut fold, ALPHA, 0, 0);
    clip_to_human_binding(
        &mut spawn,
        vec!["  Codex-CLI  ".to_owned(), "copilot".to_owned()],
    );
    apply(&mut fold, &spawn_event(spawn));

    let override_binding = BindingOverride {
        key: TaskKey(3),
        question: QuestionId::from("q-binding-Ünicode"),
        option_index: 1,
        agent: "copilot".to_owned(),
        model: "gpt-5.6".to_owned(),
        effort: Effort::XHigh,
    };
    apply(
        &mut fold,
        &answered(
            TaskKey(3),
            "q-binding-Ünicode",
            Answer4::Answered {
                option_index: 1,
                binding_override: Some(override_binding.clone()),
            },
        ),
    );
    assert_eq!(
        fold.binding_override(TaskKey(3)),
        Some(&override_binding),
        "an accepted override is what later attempts are checked against"
    );
    assert_eq!(fold.task_state(TaskKey(3)), Some(TaskState::Pending));

    apply(
        &mut fold,
        &ev(TopologyEventBody::TaskDispatched {
            data: TaskDispatched {
                key: TaskKey(3),
                generation: GenerationId(0),
                base_sha: base,
                worktree_path: "/private/workspaces/tasks/k3-g0".to_owned(),
                lease: LeaseGrant::InheritedLineage { root: ALPHA },
                source_candidate: Some(candidate_of(ALPHA, 0)),
            },
        }),
    );
    let attempt = |agent: &str, model: &str, effort: Effort, tier: Tier| {
        ev(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key: TaskKey(3),
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                rung: 0,
                binding: RungBinding {
                    tier,
                    agent: agent.to_owned(),
                    model: model.to_owned(),
                    pinned: false,
                    effort,
                },
                pool: None,
                resume_session: None,
                materialization_observed: Some(Materialization::Conflict),
            },
        })
    };
    accepts(
        &fold,
        &attempt("copilot", "gpt-5.6", Effort::XHigh, Tier::Small),
    );
    accepts(
        &fold,
        &attempt("copilot", "gpt-5.6", Effort::XHigh, Tier::Frontier),
    );
    for (label, agent, model, effort) in [
        ("agent", "  Codex-CLI  ", "gpt-5.6", Effort::XHigh),
        ("model", "copilot", "claude-opus-5", Effort::XHigh),
        ("effort", "copilot", "gpt-5.6", Effort::Low),
    ] {
        assert!(
            matches!(
                refuse(&fold, &attempt(agent, model, effort, Tier::Mid)),
                FoldError::BindingMismatch { key: 3, .. }
            ),
            "the {label} case ran something the human did not name"
        );
    }
}

#[test]
fn a_settlement_records_the_disposition_its_holding_admits() {
    let base = sha("base");
    let mut ordinary = started();
    apply(&mut ordinary, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&ordinary, ZETA, 0, 1, 0);
    apply(&mut ordinary, &start);

    let mut lineage = started();
    merge_task(&mut lineage, ALPHA, 0, 0);
    apply(
        &mut lineage,
        &spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)),
    );
    apply(
        &mut lineage,
        &ev(TopologyEventBody::TaskDispatched {
            data: TaskDispatched {
                key: TaskKey(3),
                generation: GenerationId(0),
                base_sha: base.clone(),
                worktree_path: "/private/workspaces/tasks/k3-g0".to_owned(),
                lease: LeaseGrant::InheritedLineage { root: ALPHA },
                source_candidate: Some(candidate_of(ALPHA, 0)),
            },
        }),
    );
    let repair_start = ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: TaskKey(3),
            generation: GenerationId(0),
            attempt: AttemptNumber(1),
            rung: 0,
            binding: frozen_binding(&lineage, TaskKey(3), 0),
            pool: None,
            resume_session: None,
            materialization_observed: Some(Materialization::Clean),
        },
    });
    apply(&mut lineage, &repair_start);

    let dispositions = [
        LeaseDisposition::PredictedReleased,
        LeaseDisposition::PredictedRetained,
        LeaseDisposition::LineageHeld,
    ];
    for (holding, fold, key) in [
        ("ordinary", &ordinary, ZETA),
        ("lineage", &lineage, TaskKey(3)),
    ] {
        for disposition in dispositions {
            let closing = settle(
                key,
                0,
                1,
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Failed {
                        halts_run: false,
                        reason: "  the ladder ran out  ".to_owned(),
                    },
                    lease: disposition,
                },
            );
            let closing_ok = disposition
                == if holding == "ordinary" {
                    LeaseDisposition::PredictedReleased
                } else {
                    LeaseDisposition::LineageHeld
                };
            assert_eq!(
                fold.plan_transition(&closing).is_ok(),
                closing_ok,
                "a {holding} generation that closes and records {disposition:?}"
            );

            let interrupted = ev(TopologyEventBody::AttemptInterrupted {
                data: AttemptInterrupted4 {
                    key,
                    generation: GenerationId(0),
                    attempt: AttemptNumber(1),
                    lease: disposition,
                    detail: "  the coordinator died  ".to_owned(),
                },
            });
            assert_eq!(
                fold.plan_transition(&interrupted).is_ok(),
                closing_ok,
                "a {holding} generation that is interrupted and records {disposition:?}"
            );

            for recorded in [
                LeaseDisposition::PredictedRetained,
                LeaseDisposition::PredictedReleased,
                LeaseDisposition::LineageHeld,
            ] {
                let succeeded = settle(
                    key,
                    0,
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Succeeded,
                        lease: recorded,
                    },
                );
                assert!(
                    fold.plan_transition(&succeeded).is_err(),
                    "a {holding} generation accepted a `succeeded` settlement recording \
                         {recorded:?}; `candidate_prepared` is the sole successful settlement"
                );
            }
        }
    }
}

#[test]
fn a_settlement_applies_only_to_the_attempt_that_is_running() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let closed = || AttemptSettlement::Closed {
        transition: SettlementTransition::Retry,
        lease: LeaseDisposition::PredictedReleased,
    };
    assert!(matches!(
        refuse(&fold, &settle(ZETA, 0, 1, closed())),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    accepts(&fold, &settle(ZETA, 0, 1, closed()));
    assert!(matches!(
        refuse(&fold, &settle(ALPHA, 0, 1, closed())),
        FoldError::NotTheOpenGeneration { key: 1, .. }
    ));
    assert!(matches!(
        refuse(&fold, &settle(ZETA, 1, 1, closed())),
        FoldError::NotTheOpenGeneration {
            key: 0,
            generation: 1,
            ..
        }
    ));
    assert_eq!(
        refuse(&fold, &settle(ZETA, 0, 2, closed())),
        FoldError::WrongAttempt {
            kind: "attempt_finished",
            key: 0,
            generation: 0,
            attempt: 2,
            expected: "1".to_owned(),
        }
    );
    let interrupt = |key: TaskKey, generation: u32, attempt: u32| {
        ev(TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key,
                generation: GenerationId(generation),
                attempt: AttemptNumber(attempt),
                lease: LeaseDisposition::PredictedReleased,
                detail: "  the coordinator died  ".to_owned(),
            },
        })
    };
    accepts(&fold, &interrupt(ZETA, 0, 1));
    assert!(fold.plan_transition(&interrupt(ALPHA, 0, 1)).is_err());
    assert!(fold.plan_transition(&interrupt(ZETA, 1, 1)).is_err());
    assert!(fold.plan_transition(&interrupt(ZETA, 0, 2)).is_err());
}

#[test]
fn a_generation_is_closed_only_from_an_open_class_with_no_attempt() {
    let base = sha("base");
    let closed_event = |key: TaskKey, generation: u32, lease: LeaseDisposition| {
        ev(TopologyEventBody::GenerationClosed {
            data: GenerationClosed {
                key,
                generation: GenerationId(generation),
                reason: GenerationCloseReason::RunEnding {
                    outcome: RunOutcome::Parked,
                },
                lease,
            },
        })
    };

    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    accepts(
        &fold,
        &closed_event(ZETA, 0, LeaseDisposition::PredictedReleased),
    );

    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    assert!(matches!(
        refuse(
            &fold,
            &closed_event(ZETA, 0, LeaseDisposition::PredictedReleased)
        ),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));

    let mut retained = fold.clone();
    apply(
        &mut retained,
        &settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                retained_incarnation: Epoch(0),
            },
        ),
    );
    accepts(
        &retained,
        &closed_event(ZETA, 0, LeaseDisposition::PredictedReleased),
    );

    let mut promoting = fold.clone();
    apply(&mut promoting, &candidate_prepared(ZETA, 0, &base));
    assert!(matches!(
        refuse(
            &promoting,
            &closed_event(ZETA, 0, LeaseDisposition::PredictedReleased)
        ),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));

    let mut over = promoting.clone();
    apply(&mut over, &candidate_created(ZETA, 0));
    assert!(matches!(
        refuse(
            &over,
            &closed_event(ZETA, 0, LeaseDisposition::PredictedReleased)
        ),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));
}

#[test]
fn a_candidate_prepared_whose_record_failed_is_refused() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    accepts(&fold, &candidate_prepared(ZETA, 0, &base));

    let mut failed = candidate_prepared(ZETA, 0, &base);
    let TopologyEventBody::CandidatePrepared { data } = &mut failed.body else {
        unreachable!("built as a candidate_prepared")
    };
    data.attempt.failure = Some(crate::events::FailureRecord {
        kind: crate::ladder::FailureKind::GateFailed,
        origin: crate::ladder::FailureOrigin::Worker,
        reason: "gate `clippy` failed".to_owned(),
        detail: None,
    });

    let error = refuse(&fold, &failed);
    assert!(
        matches!(error, FoldError::InconsistentRecord { .. }),
        "the fold accepted a successful settlement whose record failed: {error:?}"
    );
    assert!(
        format!("{error}").contains("succeeded"),
        "the refusal must say what it required: {error}"
    );

    let generation = fold
        .task(ZETA)
        .and_then(|task| task.generations.first())
        .expect("the generation is open");
    assert!(
        matches!(generation.class, GenerationClass::InFlight { .. }),
        "the refused event promoted the generation anyway: {:?}",
        generation.class
    );
    assert!(generation.candidate.is_none());
}

#[test]
fn a_candidate_prepared_whose_review_did_not_pass_is_refused() {
    for outcome in [ReviewPassOutcome::Failed, ReviewPassOutcome::Unavailable] {
        let base = sha("base");
        let mut fold = started();
        apply(&mut fold, &dispatch(ZETA, 0, &base));
        let start = attempt_started(&fold, ZETA, 0, 1, 0);
        apply(&mut fold, &start);

        accepts(&fold, &candidate_prepared(ZETA, 0, &base));

        let mut judged = candidate_prepared(ZETA, 0, &base);
        let TopologyEventBody::CandidatePrepared { data } = &mut judged.body else {
            unreachable!("built as a candidate_prepared")
        };
        assert!(data.attempt.failure.is_none());
        data.attempt
            .reviews
            .last_mut()
            .expect("the premise carries the primary pass")
            .outcome = outcome;

        let error = refuse(&fold, &judged);
        assert!(
            matches!(error, FoldError::InconsistentRecord { .. }),
            "a `{outcome:?}` review was settled as a success: {error:?}"
        );
        let text = format!("{error}");
        assert!(
            text.contains("review outcomes") && text.contains(&format!("{outcome:?}")),
            "the refusal must name the outcome that decided it: {text}"
        );

        let generation = fold
            .task(ZETA)
            .and_then(|task| task.generations.first())
            .expect("the generation is open");
        assert!(
            matches!(generation.class, GenerationClass::InFlight { .. }),
            "the refused event promoted the generation anyway: {:?}",
            generation.class
        );
        assert!(generation.candidate.is_none());
    }
}

fn reviews_off_started() -> TopologyFold {
    let plan = plan();
    let unauthenticated = RunStarted4 {
        reviews: ReviewPlan {
            second_opinion: vec![None; plan.tasks.len()],
            ..ReviewPlan::default()
        },
        registry_digest: String::new(),
        ..run_started_unauthenticated()
    };
    let digest = TaskRegistry::originals_with_agents(
        &plan,
        &unauthenticated.registry_record(),
        &unauthenticated.probed_agents,
    )
    .expect("a review-free record derives a registry")
    .digest();
    let event = ev(TopologyEventBody::RunStarted {
        data: Box::new(RunStarted4 {
            registry_digest: digest,
            ..unauthenticated
        }),
    });
    let mut fold = TopologyFold::new(inputs());
    apply(&mut fold, &event);
    fold
}

type ObligationRow = (
    &'static str,
    TopologyFold,
    TaskKey,
    Vec<(&'static str, ReviewPassOutcome)>,
    Vec<(&'static str, Vec<(&'static str, ReviewPassOutcome)>)>,
);

fn prepared_with_passes(
    key: TaskKey,
    base: &CommitSha,
    passes: &[(&str, ReviewPassOutcome)],
) -> TopologyEvent {
    let mut event = candidate_prepared(key, 0, base);
    let TopologyEventBody::CandidatePrepared { data } = &mut event.body else {
        unreachable!("built as a candidate_prepared")
    };
    data.attempt.reviews = passes
        .iter()
        .map(|(pass, outcome)| review_pass(pass, *outcome))
        .collect();
    event
}

fn in_flight_at(fold: &mut TopologyFold, key: TaskKey, base: &CommitSha) {
    apply(fold, &dispatch(key, 0, base));
    let start = attempt_started(fold, key, 0, 1, 0);
    apply(fold, &start);
}

#[test]
fn candidate_success_is_judged_against_the_tasks_frozen_review_plan() {
    let base = sha("base");
    const REVIEW: &str = "review";
    const SECOND: &str = "second-opinion";
    let pass = |name: &'static str| (name, ReviewPassOutcome::Passed);

    let rows: Vec<ObligationRow> = vec![
        (
            "none configured",
            reviews_off_started(),
            ZETA,
            vec![],
            vec![
                ("a pass nobody configured", vec![pass(REVIEW)]),
                ("a second opinion nobody configured", vec![pass(SECOND)]),
            ],
        ),
        (
            "one configured",
            started(),
            ZETA,
            vec![pass(REVIEW)],
            vec![
                ("a lone second opinion", vec![pass(SECOND)]),
                ("no passes at all", vec![]),
                (
                    "the configured pass duplicated",
                    vec![pass(REVIEW), pass(REVIEW)],
                ),
                (
                    "a pass nobody configured, beside it",
                    vec![pass(REVIEW), pass(SECOND)],
                ),
                (
                    "a pass name nobody has ever configured",
                    vec![pass("security")],
                ),
                (
                    "the configured pass, failed",
                    vec![(REVIEW, ReviewPassOutcome::Failed)],
                ),
            ],
        ),
        (
            "two configured",
            started(),
            MID,
            vec![pass(REVIEW), pass(SECOND)],
            vec![
                ("the primary omitted", vec![pass(SECOND)]),
                ("the second opinion omitted", vec![pass(REVIEW)]),
                ("both in the other order", vec![pass(SECOND), pass(REVIEW)]),
                (
                    "one of the two failed",
                    vec![pass(REVIEW), (SECOND, ReviewPassOutcome::Failed)],
                ),
                (
                    "an unconfigured third",
                    vec![pass(REVIEW), pass(SECOND), pass("security")],
                ),
            ],
        ),
    ];

    for (label, mut fold, key, obliged, refusals) in rows {
        in_flight_at(&mut fold, key, &base);

        accepts(&fold, &prepared_with_passes(key, &base, &obliged));

        for (why, passes) in refusals {
            let error = refuse(&fold, &prepared_with_passes(key, &base, &passes));
            assert!(
                matches!(error, FoldError::InconsistentRecord { .. }),
                "{label}/{why}: refused as {error:?} rather than as a record disagreement"
            );
            let generation = fold
                .task(key)
                .and_then(|task| task.generations.first())
                .expect("the generation is open");
            assert!(
                matches!(generation.class, GenerationClass::InFlight { .. }),
                "{label}/{why}: the refused event promoted the generation anyway"
            );
            assert!(generation.candidate.is_none(), "{label}/{why}");
        }
    }
}

#[test]
fn a_run_that_froze_verification_off_obliges_no_pass_whatever_it_resolved() {
    let base = sha("base");
    let plan = plan();
    let unauthenticated = RunStarted4 {
        reviews: ReviewPlan {
            enabled: Some(false),
            ..review_plan(plan.tasks.len())
        },
        registry_digest: String::new(),
        ..run_started_unauthenticated()
    };
    let digest = TaskRegistry::originals_with_agents(
        &plan,
        &unauthenticated.registry_record(),
        &unauthenticated.probed_agents,
    )
    .expect("the record derives a registry")
    .digest();
    let event = ev(TopologyEventBody::RunStarted {
        data: Box::new(RunStarted4 {
            registry_digest: digest,
            ..unauthenticated
        }),
    });

    for key in [ZETA, MID] {
        let mut fold = TopologyFold::new(inputs());
        apply(&mut fold, &event);

        let entry = fold
            .registry()
            .and_then(|registry| registry.get(key))
            .expect("a registered task")
            .clone();
        assert!(
            entry.reviews.primary.is_some(),
            "task {}: the fixture resolved no primary, so the flag is not what is under test",
            key.0
        );
        assert!(
            entry.reviews.obliged_lenses().is_empty(),
            "task {}: a run that judged nothing obliged a pass",
            key.0
        );

        in_flight_at(&mut fold, key, &base);
        accepts(&fold, &prepared_with_passes(key, &base, &[]));
        for present in [
            vec![("review", ReviewPassOutcome::Passed)],
            vec![("second-opinion", ReviewPassOutcome::Passed)],
        ] {
            let error = refuse(&fold, &prepared_with_passes(key, &base, &present));
            assert!(
                matches!(error, FoldError::InconsistentRecord { .. }),
                "task {}: a pass the run never configured was admitted: {error:?}",
                key.0
            );
        }
    }
}

#[test]
fn the_door_accepts_exactly_the_passes_the_frozen_entry_obliges() {
    let base = sha("base");
    for key in [ZETA, ALPHA, MID] {
        let mut fold = started();
        in_flight_at(&mut fold, key, &base);
        let entry = fold
            .registry()
            .and_then(|registry| registry.get(key))
            .expect("a registered task")
            .clone();
        let obliged: Vec<(&str, ReviewPassOutcome)> = entry
            .reviews
            .obliged_lenses()
            .iter()
            .map(|lens| (lens.name(), ReviewPassOutcome::Passed))
            .collect();
        accepts(&fold, &prepared_with_passes(key, &base, &obliged));
        assert!(
            !obliged.is_empty(),
            "task {} obliges nothing under a plan that enabled review",
            key.0
        );
        for dropped in 0..obliged.len() {
            let mut short = obliged.clone();
            short.remove(dropped);
            let error = refuse(&fold, &prepared_with_passes(key, &base, &short));
            assert!(
                matches!(error, FoldError::InconsistentRecord { .. }),
                "task {} without its pass {dropped} was admitted: {error:?}",
                key.0
            );
        }
    }
}

#[test]
fn an_attempt_finished_whose_record_says_success_is_refused() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    let closed = || AttemptSettlement::Closed {
        transition: SettlementTransition::Failed {
            halts_run: false,
            reason: "gate `clippy` failed".to_owned(),
        },
        lease: LeaseDisposition::PredictedReleased,
    };

    accepts(&fold, &settle(ZETA, 0, 1, closed()));

    let mut lying = settle(ZETA, 0, 1, closed());
    let TopologyEventBody::AttemptFinished { data } = &mut lying.body else {
        unreachable!("built as an attempt_finished")
    };
    *data.record = attempt_record(1);
    assert!(data.record.is_successful());

    let error = refuse(&fold, &lying);
    assert!(
        matches!(error, FoldError::InconsistentRecord { .. }),
        "a failure settled while its record reported success: {error:?}"
    );
    assert!(
        format!("{error}").contains("says the attempt succeeded"),
        "the refusal must say why: {error}"
    );
}

#[test]
fn an_attempt_finished_whose_record_names_another_attempt_is_refused() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    let closed = || AttemptSettlement::Closed {
        transition: SettlementTransition::Failed {
            halts_run: false,
            reason: "gate `clippy` failed".to_owned(),
        },
        lease: LeaseDisposition::PredictedReleased,
    };
    accepts(&fold, &settle(ZETA, 0, 1, closed()));

    for named in [0, 2, 9] {
        let mut misattributed = settle(ZETA, 0, 1, closed());
        let TopologyEventBody::AttemptFinished { data } = &mut misattributed.body else {
            unreachable!("built as an attempt_finished")
        };
        data.record.attempt = named;

        let error = refuse(&fold, &misattributed);
        assert!(
            matches!(
                error,
                FoldError::WrongAttempt {
                    attempt, ref expected, ..
                } if attempt == named && expected == "1"
            ),
            "a settlement carried attempt {named}'s record on attempt 1's line: {error:?}"
        );
    }
}

#[test]
fn a_successful_attempt_charges_its_rung_live_and_on_replay() {
    let base = sha("base");

    for (label, failures_first) in [("first-attempt success", 0), ("second-attempt success", 1)] {
        let mut live = started();
        let mut trace = vec![run_started_event()];

        let mut generation = 0;
        for _ in 0..failures_first {
            push(&mut live, &mut trace, dispatch(ZETA, generation, &base));
            let start = attempt_started(&live, ZETA, generation, 1, 0);
            push(&mut live, &mut trace, start);
            push(
                &mut live,
                &mut trace,
                settle(
                    ZETA,
                    generation,
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            );
            generation += 1;
        }

        push(&mut live, &mut trace, dispatch(ZETA, generation, &base));
        let start = attempt_started(&live, ZETA, generation, 1, 0);
        push(&mut live, &mut trace, start);
        push(
            &mut live,
            &mut trace,
            candidate_prepared(ZETA, generation, &base),
        );

        let expected = failures_first + 1;
        let counted = live
            .task(ZETA)
            .map(|task| task.attempts_on_rung)
            .expect("the task is registered");
        assert_eq!(
            counted, expected,
            "{label}: the rung counts {counted} attempt(s) and {expected} were spent. \
                 `candidate_prepared` is the successful settlement, and a settlement that \
                 does not charge leaves an operator a free attempt on a rung already paid \
                 for"
        );

        let replayed = TopologyFold::replay(inputs(), &trace).expect("the trace replays");
        assert_eq!(
            replayed.task(ZETA).map(|task| task.attempts_on_rung),
            Some(expected),
            "{label}: the live fold counted {expected} and a replay of its own log did \
                 not — one fold, not two"
        );
    }
}

#[test]
fn a_candidate_is_prepared_by_the_generation_whose_attempt_is_in_flight() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    accepts(&fold, &candidate_prepared(ZETA, 0, &base));

    assert!(matches!(
        refuse(&fold, &candidate_prepared(ZETA, 1, &base)),
        FoldError::NotTheOpenGeneration {
            key: 0,
            generation: 1,
            ..
        }
    ));
    assert!(matches!(
        refuse(&fold, &candidate_prepared(ALPHA, 0, &base)),
        FoldError::NotTheOpenGeneration { key: 1, .. }
    ));

    let mut reparented = candidate_prepared(ZETA, 0, &base);
    if let TopologyEventBody::CandidatePrepared { data } = &mut reparented.body {
        data.parent_sha = sha("elsewhere");
    }
    assert!(matches!(
        refuse(&fold, &reparented),
        FoldError::InconsistentRecord { .. }
    ));
    let moved_base = candidate_prepared(ZETA, 0, &sha("another-base"));
    assert!(matches!(
        refuse(&fold, &moved_base),
        FoldError::InconsistentRecord { .. }
    ));

    let mut inconsistent_region = candidate_prepared(ZETA, 0, &base);
    if let TopologyEventBody::CandidatePrepared { data } = &mut inconsistent_region.body {
        data.lease_effect = CandidateLeaseEffect::ReplacesPredicted { paths: region(MID) };
    }
    assert!(matches!(
        refuse(&fold, &inconsistent_region),
        FoldError::InconsistentRecord { .. }
    ));

    let mut widening = candidate_prepared(ZETA, 0, &base);
    if let TopologyEventBody::CandidatePrepared { data } = &mut widening.body {
        data.lease_effect = CandidateLeaseEffect::WidensLineage {
            root: ALPHA,
            paths: region(ZETA),
        };
    }
    assert!(matches!(
        refuse(&fold, &widening),
        FoldError::InconsistentRecord { .. }
    ));

    for wrong in [0, 2, 9] {
        let mut misattributed = candidate_prepared(ZETA, 0, &base);
        if let TopologyEventBody::CandidatePrepared { data } = &mut misattributed.body {
            *data.attempt = attempt_record(wrong);
        }
        assert!(
            matches!(
                refuse(&fold, &misattributed),
                FoldError::WrongAttempt {
                    kind: "candidate_prepared",
                    key: 0,
                    ..
                }
            ),
            "a candidate attributed to attempt {wrong} of a generation that ran 1 was folded"
        );
    }

    apply(&mut fold, &candidate_prepared(ZETA, 0, &base));
    let leases = fold.leases().expect("started");
    assert!(leases.holds(LeaseOwner::Candidate {
        key: ZETA,
        generation: GenerationId(0)
    }));
    assert!(!leases.holds(LeaseOwner::Generation {
        key: ZETA,
        generation: GenerationId(0)
    }));
    assert_eq!(fold.task_state(ZETA), Some(TaskState::AwaitingMerge));

    assert!(matches!(
        refuse(&fold, &candidate_prepared(ZETA, 0, &base)),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));
    let mut second = candidate_prepared(ZETA, 0, &base);
    if let TopologyEventBody::CandidatePrepared { data } = &mut second.body {
        data.commit_sha = sha("a-second-commit");
        data.candidate_ref = git_ref("candidates/0/0-again");
    }
    assert!(
        matches!(
            refuse(&fold, &second),
            FoldError::NotTheOpenGeneration { key: 0, .. }
        ),
        "a second candidate replaced the first and left it abandoned"
    );
    let mut promotes_second = candidate_created(ZETA, 0);
    if let TopologyEventBody::TaskCandidateCreated { data } = &mut promotes_second.body {
        data.candidate.commit_sha = sha("a-second-commit");
        data.candidate.candidate_ref = git_ref("candidates/0/0-again");
    }
    assert!(matches!(
        refuse(&fold, &promotes_second),
        FoldError::InconsistentRecord { .. }
    ));
    accepts(&fold, &candidate_created(ZETA, 0));
}

#[test]
fn a_candidate_names_the_attempt_that_produced_it_live_and_on_replay() {
    let base = sha("base");
    let mut live = started();
    let mut trace = vec![run_started_event()];
    let push = |live: &mut TopologyFold, trace: &mut Vec<TopologyEvent>, event: TopologyEvent| {
        apply(live, &event);
        trace.push(event);
    };
    push(&mut live, &mut trace, dispatch(ALPHA, 0, &base));
    let start = attempt_started(&live, ALPHA, 0, 1, 0);
    push(&mut live, &mut trace, start);
    push(
        &mut live,
        &mut trace,
        settle(
            ALPHA,
            0,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                retained_incarnation: Epoch(0),
            },
        ),
    );
    let retry = ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: ALPHA,
            generation: GenerationId(0),
            attempt: AttemptNumber(2),
            rung: 0,
            binding: frozen_binding(&live, ALPHA, 0),
            pool: None,
            resume_session: Some(SessionId("sess-ÜNI-0042".to_owned())),
            materialization_observed: None,
        },
    });
    push(&mut live, &mut trace, retry);

    assert!(matches!(
        refuse(&live, &candidate_prepared_at(ALPHA, 0, 1, &base)),
        FoldError::WrongAttempt { .. }
    ));
    accepts(&live, &candidate_prepared_at(ALPHA, 0, 2, &base));

    let bytes = |trace: &[TopologyEvent]| -> Vec<u8> {
        let mut log = Vec::new();
        for event in trace {
            log.extend_from_slice(serde_json::to_string(event).expect("serialize").as_bytes());
            log.push(b'\n');
        }
        log
    };
    let mut hostile = trace.clone();
    hostile.push(candidate_prepared_at(ALPHA, 0, 1, &base));
    let parsed = TopologyFold::parse_log(&bytes(&hostile)).expect("the log parses");
    assert!(matches!(
        TopologyFold::replay(inputs(), &parsed)
            .expect_err("a misattributed candidate is refused on replay"),
        FoldError::WrongAttempt { .. }
    ));

    push(
        &mut live,
        &mut trace,
        candidate_prepared_at(ALPHA, 0, 2, &base),
    );
    let parsed = TopologyFold::parse_log(&bytes(&trace)).expect("the log parses");
    let replayed = TopologyFold::replay(inputs(), &parsed).expect("the authoritative log replays");
    assert_eq!(live.state(), replayed.state());
}

#[test]
fn a_promotion_names_the_candidate_that_was_prepared() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    assert!(matches!(
        refuse(&fold, &candidate_created(ZETA, 0)),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));
    apply(&mut fold, &candidate_prepared(ZETA, 0, &base));
    accepts(&fold, &candidate_created(ZETA, 0));

    let mismatched = |mutate: fn(&mut CandidateRef)| {
        let mut candidate = candidate_of(ZETA, 0);
        mutate(&mut candidate);
        ev(TopologyEventBody::TaskCandidateCreated {
            data: TaskCandidateCreated { candidate },
        })
    };
    assert!(matches!(
        refuse(
            &fold,
            &mismatched(|candidate| candidate.commit_sha = sha("smuggled"))
        ),
        FoldError::InconsistentRecord { .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &mismatched(|candidate| candidate.candidate_ref = git_ref("candidates/9/9"))
        ),
        FoldError::InconsistentRecord { .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &mismatched(|candidate| candidate.generation = GenerationId(1))
        ),
        FoldError::NotTheOpenGeneration { .. }
    ));
    assert!(matches!(
        refuse(&fold, &mismatched(|candidate| candidate.key = ALPHA)),
        FoldError::NotTheOpenGeneration { key: 1, .. }
    ));

    apply(&mut fold, &candidate_created(ZETA, 0));
    assert_eq!(fold.queue().expect("started").len(), 1);
    assert_eq!(
        fold.task(ZETA).expect("zeta").generations[0].class,
        GenerationClass::Closed
    );
}

fn two_queued() -> TopologyFold {
    let base = sha("base");
    let mut fold = started();
    for (key, generation) in [(MID, 0), (ZETA, 0)] {
        apply(&mut fold, &dispatch(key, generation, &base));
        let start = attempt_started(&fold, key, generation, 1, 0);
        apply(&mut fold, &start);
        apply(&mut fold, &candidate_prepared(key, generation, &base));
        apply(&mut fold, &candidate_created(key, generation));
    }
    fold
}

fn verification_started(
    key: TaskKey,
    generation: u32,
    sequence: u32,
    head: &CommitSha,
    proposal: &CommitSha,
) -> TopologyEvent {
    ev(TopologyEventBody::MergeVerificationStarted {
        data: MergeVerificationStarted {
            sequence: SequenceId(sequence),
            candidate: candidate_of(key, generation),
            basis: VerificationBasis::StaleClean {
                prepared_ref: git_ref(&format!("prepared/{sequence}")),
            },
            expected_head: head.clone(),
            proposed_sha: proposal.clone(),
        },
    })
}

#[test]
fn an_integration_starts_only_for_the_first_eligible_candidate() {
    let head = sha("head");
    let proposal = sha("proposal");
    let fold = two_queued();
    let queued: Vec<u32> = fold
        .queue()
        .expect("started")
        .entries()
        .iter()
        .map(|entry| entry.key().0)
        .collect();
    assert_eq!(queued, vec![MID.0, ZETA.0], "the queue is promotion order");

    accepts(&fold, &verification_started(MID, 0, 0, &head, &proposal));
    assert!(matches!(
        refuse(&fold, &verification_started(ZETA, 0, 0, &head, &proposal)),
        FoldError::NotFirstEligible { key: 0, .. }
    ));

    assert!(matches!(
        refuse(&fold, &verification_started(ALPHA, 0, 0, &head, &proposal)),
        FoldError::NotFirstEligible { key: 1, .. }
    ));

    let mut parked = fold.clone();
    apply(
        &mut parked,
        &ev(TopologyEventBody::QuestionRaised {
            data: QuestionRaised4 {
                question: question("q-park-Ünicode", MID),
            },
        }),
    );
    assert!(matches!(
        refuse(&parked, &verification_started(MID, 0, 0, &head, &proposal)),
        FoldError::NotFirstEligible { key: 2, .. }
    ));
    accepts(&parked, &verification_started(ZETA, 0, 0, &head, &proposal));
}

#[test]
fn sequences_are_dense_and_one_transaction_runs_at_a_time() {
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = two_queued();

    for sequence in [1, 2, 9] {
        assert_eq!(
            refuse(
                &fold,
                &verification_started(MID, 0, sequence, &head, &proposal)
            ),
            FoldError::NonDenseSequence {
                kind: "merge_verification_started",
                sequence,
                next: 0,
            }
        );
    }
    apply(
        &mut fold,
        &verification_started(MID, 0, 0, &head, &proposal),
    );

    assert_eq!(
        refuse(&fold, &verification_started(ZETA, 0, 1, &head, &proposal)),
        FoldError::TransactionAlreadyOpen {
            kind: "merge_verification_started",
            sequence: 1,
            open: 0,
        }
    );

    let unavailable = |sequence: u32| {
        ev(TopologyEventBody::MergeVerificationUnavailable {
            data: MergeVerificationUnavailable {
                sequence: SequenceId(sequence),
                cause: UnavailableCause::Infrastructure {
                    kind: InfrastructureKind::RateLimited,
                },
                outcome: UnavailableOutcome::Deferred { defers: 1 },
            },
        })
    };
    assert_eq!(
        refuse(&fold, &unavailable(1)),
        FoldError::WrongSequence {
            kind: "merge_verification_unavailable",
            sequence: 1,
            open: "0".to_owned(),
        }
    );
    accepts(&fold, &unavailable(0));

    apply(&mut fold, &unavailable(0));
    assert!(matches!(
        refuse(&fold, &verification_started(ZETA, 0, 0, &head, &proposal)),
        FoldError::NonDenseSequence { next: 1, .. }
    ));
    accepts(&fold, &verification_started(ZETA, 0, 1, &head, &proposal));

    assert_eq!(
        refuse(&two_queued(), &unavailable(0)),
        FoldError::WrongSequence {
            kind: "merge_verification_unavailable",
            sequence: 0,
            open: "none".to_owned(),
        }
    );
}

#[test]
fn a_stale_verification_runs_only_on_a_candidate_that_is_actually_stale() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let fold = two_queued();
    assert!(matches!(
        refuse(&fold, &verification_started(MID, 0, 0, &base, &proposal)),
        FoldError::InconsistentRecord { .. }
    ));
    accepts(&fold, &verification_started(MID, 0, 0, &head, &proposal));

    let mut stale_at_head = verification_started(MID, 0, 0, &head, &head);
    assert!(matches!(
        refuse(&fold, &stale_at_head),
        FoldError::InconsistentRecord { .. }
    ));
    if let TopologyEventBody::MergeVerificationStarted { data } = &mut stale_at_head.body {
        data.basis = VerificationBasis::AlreadyPresent;
    }
    accepts(&fold, &stale_at_head);

    let mut already_present_elsewhere = stale_at_head;
    if let TopologyEventBody::MergeVerificationStarted { data } =
        &mut already_present_elsewhere.body
    {
        data.proposed_sha = proposal;
    }
    assert!(matches!(
        refuse(&fold, &already_present_elsewhere),
        FoldError::InconsistentRecord { .. }
    ));
}

fn verification_record(verdict: Verdict) -> VerificationRecord {
    VerificationRecord {
        verdict,
        gates_passed: verdict != Verdict::GatesFailed,
        reviews: Vec::new(),
        detail: "  the integration verification  ".to_owned(),
    }
}

#[test]
fn the_publication_relations_hold_over_the_crossed_disposition_grid() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");

    let fold = two_queued();
    let fast = fast_publication(MID, 0, 0, &base, vec![MID]);
    accepts(&fold, &fast);

    let fast_cases: [(&str, BreakPublication); 5] = [
        ("a head that is not the candidate's base", |prepared| {
            prepared.expected_head = sha("moved-head");
        }),
        (
            "a proposal that is not the candidate's commit",
            |prepared| {
                prepared.proposed_sha = sha("smuggled");
                prepared.candidate_sha = sha("smuggled");
            },
        ),
        ("a proposal pin", |prepared| {
            prepared.prepared_ref = Some(git_ref("prepared/0"));
        }),
        ("a verification as its source", |prepared| {
            prepared.verification_source = VerificationSource::Verification {
                sequence: SequenceId(0),
            };
        }),
        ("another candidate's record as its source", |prepared| {
            prepared.verification_source = VerificationSource::CandidatePrepared {
                key: ZETA,
                generation: GenerationId(0),
            };
        }),
    ];
    for (label, break_it) in fast_cases {
        let mut event = fast.clone();
        if let TopologyEventBody::MergePrepared { data } = &mut event.body {
            break_it(data);
        }
        assert!(
            fold.plan_transition(&event).is_err(),
            "a fast publication with {label} was authorized"
        );
    }

    let mut stale = two_queued();
    apply(
        &mut stale,
        &verification_started(MID, 0, 0, &head, &proposal),
    );
    let stale_publication = |mutate: Option<BreakPublication>| {
        let mut prepared = MergePrepared {
            sequence: SequenceId(0),
            disposition: PreparedDisposition::StaleClean,
            expected_head: head.clone(),
            proposed_sha: proposal.clone(),
            key: MID,
            generation: GenerationId(0),
            candidate_sha: candidate_of(MID, 0).commit_sha,
            candidate_ref: candidate_of(MID, 0).candidate_ref,
            prepared_ref: Some(git_ref("prepared/0")),
            verification_source: VerificationSource::Verification {
                sequence: SequenceId(0),
            },
            verification: Some(verification_record(Verdict::Passed)),
            satisfies: vec![MID],
        };
        if let Some(mutate) = mutate {
            mutate(&mut prepared);
        }
        ev(TopologyEventBody::MergePrepared {
            data: Box::new(prepared),
        })
    };
    accepts(&stale, &stale_publication(None));

    let stale_cases: [(&str, BreakPublication); 7] = [
        ("a head the verification did not read", |prepared| {
            prepared.expected_head = sha("moved-head");
        }),
        ("a proposal the verification did not judge", |prepared| {
            prepared.proposed_sha = sha("another-proposal");
        }),
        ("no proposal pin", |prepared| prepared.prepared_ref = None),
        ("another pin than the one it verified", |prepared| {
            prepared.prepared_ref = Some(git_ref("prepared/9"));
        }),
        ("no verification record", |prepared| {
            prepared.verification = None;
        }),
        ("a verification that did not pass", |prepared| {
            prepared.verification = Some(VerificationRecord {
                verdict: Verdict::Rejected,
                gates_passed: true,
                reviews: Vec::new(),
                detail: "  rejected  ".to_owned(),
            });
        }),
        ("the candidate's own record as its source", |prepared| {
            prepared.verification_source = VerificationSource::CandidatePrepared {
                key: MID,
                generation: GenerationId(0),
            };
        }),
    ];
    for (label, break_it) in stale_cases {
        assert!(
            stale
                .plan_transition(&stale_publication(Some(break_it)))
                .is_err(),
            "a stale-clean publication with {label} was authorized"
        );
    }

    let mut present = two_queued();
    let mut basis = verification_started(MID, 0, 0, &head, &head);
    if let TopologyEventBody::MergeVerificationStarted { data } = &mut basis.body {
        data.basis = VerificationBasis::AlreadyPresent;
    }
    apply(&mut present, &basis);
    let present_publication = |mutate: Option<BreakPublication>| {
        let mut prepared = MergePrepared {
            sequence: SequenceId(0),
            disposition: PreparedDisposition::AlreadyPresent,
            expected_head: head.clone(),
            proposed_sha: head.clone(),
            key: MID,
            generation: GenerationId(0),
            candidate_sha: candidate_of(MID, 0).commit_sha,
            candidate_ref: candidate_of(MID, 0).candidate_ref,
            prepared_ref: None,
            verification_source: VerificationSource::Verification {
                sequence: SequenceId(0),
            },
            verification: Some(verification_record(Verdict::Passed)),
            satisfies: vec![MID],
        };
        if let Some(mutate) = mutate {
            mutate(&mut prepared);
        }
        ev(TopologyEventBody::MergePrepared {
            data: Box::new(prepared),
        })
    };
    accepts(&present, &present_publication(None));
    let present_cases: [(&str, BreakPublication); 3] = [
        ("a proposal that is not the head", |prepared| {
            prepared.proposed_sha = sha("another-proposal");
        }),
        ("a head the verification did not read", |prepared| {
            prepared.expected_head = sha("moved-head");
            prepared.proposed_sha = sha("moved-head");
        }),
        ("a verification that did not pass", |prepared| {
            prepared.verification = Some(verification_record(Verdict::GatesFailed));
        }),
    ];
    for (label, break_it) in present_cases {
        assert!(
            present
                .plan_transition(&present_publication(Some(break_it)))
                .is_err(),
            "an already-present publication with {label} was authorized"
        );
    }

    assert!(
        stale
            .plan_transition(&stale_publication(Some(|prepared| {
                prepared.disposition = PreparedDisposition::AlreadyPresent;
            })))
            .is_err(),
        "a stale-clean verification published as already-present"
    );
    assert!(
        present
            .plan_transition(&present_publication(Some(|prepared| {
                prepared.disposition = PreparedDisposition::StaleClean;
                prepared.prepared_ref = Some(git_ref("prepared/0"));
            })))
            .is_err(),
        "an already-present verification published as stale-clean"
    );
    assert!(
        two_queued()
            .plan_transition(&stale_publication(None))
            .is_err()
    );
    assert!(
        stale
            .plan_transition(&fast_publication(MID, 0, 0, &base, vec![MID]))
            .is_err()
    );
}

#[test]
fn a_publication_names_the_candidate_durable_history_recorded_and_no_decoy() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let recorded = candidate_of(MID, 0);

    let fold = two_queued();
    let mut decoy_ref = fast_publication(MID, 0, 0, &base, vec![MID]);
    if let TopologyEventBody::MergePrepared { data } = &mut decoy_ref.body {
        data.candidate_ref = git_ref("candidates/2/0-decoy");
        assert_ne!(data.candidate_ref, recorded.candidate_ref);
        assert_eq!(
            data.candidate_sha, recorded.commit_sha,
            "only the ref moved"
        );
        assert_eq!(data.proposed_sha, recorded.commit_sha, "only the ref moved");
    }
    assert!(
        matches!(
            refuse(&fold, &decoy_ref),
            FoldError::InconsistentRecord {
                kind: "merge_prepared",
                ..
            }
        ),
        "a fast publication naming a candidate ref no `candidate_prepared` recorded was \
             authorized"
    );

    let verified = |basis_stale: bool| {
        let mut fold = two_queued();
        let event = if basis_stale {
            verification_started(MID, 0, 0, &head, &proposal)
        } else {
            ev(TopologyEventBody::MergeVerificationStarted {
                data: MergeVerificationStarted {
                    sequence: SequenceId(0),
                    candidate: candidate_of(MID, 0),
                    basis: VerificationBasis::AlreadyPresent,
                    expected_head: head.clone(),
                    proposed_sha: head.clone(),
                },
            })
        };
        apply(&mut fold, &event);
        fold
    };
    let publication = |basis_stale: bool| MergePrepared {
        sequence: SequenceId(0),
        disposition: if basis_stale {
            PreparedDisposition::StaleClean
        } else {
            PreparedDisposition::AlreadyPresent
        },
        expected_head: head.clone(),
        proposed_sha: if basis_stale {
            proposal.clone()
        } else {
            head.clone()
        },
        key: MID,
        generation: GenerationId(0),
        candidate_sha: recorded.commit_sha.clone(),
        candidate_ref: recorded.candidate_ref.clone(),
        prepared_ref: basis_stale.then(|| git_ref("prepared/0")),
        verification_source: VerificationSource::Verification {
            sequence: SequenceId(0),
        },
        verification: Some(verification_record(Verdict::Passed)),
        satisfies: vec![MID],
    };

    let forgeries: [(&str, ForgeCandidate); 2] = [
        ("commit_sha", |prepared| {
            prepared.candidate_sha = sha("a-commit-nobody-prepared");
        }),
        ("candidate_ref", |prepared| {
            prepared.candidate_ref = git_ref("candidates/2/0-decoy");
        }),
    ];
    for basis_stale in [true, false] {
        let fold = verified(basis_stale);
        let disposition = if basis_stale {
            "stale_clean"
        } else {
            "already_present"
        };
        accepts(
            &fold,
            &ev(TopologyEventBody::MergePrepared {
                data: Box::new(publication(basis_stale)),
            }),
        );
        for (label, forge) in forgeries {
            let mut prepared = publication(basis_stale);
            forge(&mut prepared);
            let event = ev(TopologyEventBody::MergePrepared {
                data: Box::new(prepared),
            });
            if let TopologyEventBody::MergePrepared { data } = &event.body {
                data.self_consistency()
                    .expect("the forgery agrees with itself, which is what makes it one");
            }
            assert!(
                matches!(
                    refuse(&fold, &event),
                    FoldError::InconsistentRecord {
                        kind: "merge_prepared",
                        ..
                    }
                ),
                "a {disposition} publication whose {label} names nothing in durable history \
                     was authorized"
            );
        }
    }

    let mut trace = vec![run_started_event()];
    let mut live = started();
    for (key, generation) in [(MID, 0), (ZETA, 0)] {
        push(&mut live, &mut trace, dispatch(key, generation, &base));
        let start = attempt_started(&live, key, generation, 1, 0);
        push(&mut live, &mut trace, start);
        push(
            &mut live,
            &mut trace,
            candidate_prepared(key, generation, &base),
        );
        push(&mut live, &mut trace, candidate_created(key, generation));
    }
    let mut forged = trace.clone();
    forged.push(decoy_ref);
    forged.push(merged(MID, 0, 0, vec![MID]));
    let parsed = TopologyFold::parse_log(&wire(&forged)).expect("the log parses");
    assert!(
        matches!(
            TopologyFold::replay(inputs(), &parsed)
                .expect_err("a forged publication is refused on replay"),
            FoldError::InconsistentRecord {
                kind: "merge_prepared",
                ..
            }
        ),
        "a forged publication was applied on replay and its `task_merged` followed it"
    );
}

fn nudge_last(value: &CommitSha) -> CommitSha {
    let mut moved = value.0.clone();
    let last = moved.pop().expect("a SHA has characters");
    moved.push(if last == 'f' { 'e' } else { 'f' });
    assert_eq!(moved.len(), value.0.len());
    assert_ne!(moved, value.0);
    CommitSha(moved)
}

#[test]
fn a_publication_compares_whole_shas_and_not_prefixes() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let recorded = candidate_of(MID, 0);

    let fold = two_queued();
    let mut moved_head = fast_publication(MID, 0, 0, &base, vec![MID]);
    if let TopologyEventBody::MergePrepared { data } = &mut moved_head.body {
        data.expected_head = nudge_last(&base);
    }
    assert!(
        fold.plan_transition(&moved_head).is_err(),
        "a fast publication expecting a head one character from the candidate's base was \
             authorized"
    );
    let mut moved_commit = fast_publication(MID, 0, 0, &base, vec![MID]);
    if let TopologyEventBody::MergePrepared { data } = &mut moved_commit.body {
        data.proposed_sha = nudge_last(&recorded.commit_sha);
        data.candidate_sha = nudge_last(&recorded.commit_sha);
    }
    assert!(
        fold.plan_transition(&moved_commit).is_err(),
        "a fast publication proposing a commit one character from the candidate's was \
             authorized"
    );

    let mut stale = two_queued();
    apply(
        &mut stale,
        &verification_started(MID, 0, 0, &head, &proposal),
    );
    let publication = |expected_head: CommitSha, proposed_sha: CommitSha| {
        ev(TopologyEventBody::MergePrepared {
            data: Box::new(MergePrepared {
                sequence: SequenceId(0),
                disposition: PreparedDisposition::StaleClean,
                expected_head,
                proposed_sha,
                key: MID,
                generation: GenerationId(0),
                candidate_sha: recorded.commit_sha.clone(),
                candidate_ref: recorded.candidate_ref.clone(),
                prepared_ref: Some(git_ref("prepared/0")),
                verification_source: VerificationSource::Verification {
                    sequence: SequenceId(0),
                },
                verification: Some(verification_record(Verdict::Passed)),
                satisfies: vec![MID],
            }),
        })
    };
    accepts(&stale, &publication(head.clone(), proposal.clone()));
    assert!(
        stale
            .plan_transition(&publication(nudge_last(&head), proposal.clone()))
            .is_err(),
        "a stale publication expecting a head one character from the verification's was \
             authorized"
    );
    assert!(
        stale
            .plan_transition(&publication(head.clone(), nudge_last(&proposal)))
            .is_err(),
        "a stale publication proposing a commit one character from the judged one was \
             authorized"
    );
}

#[test]
fn a_verified_publication_belongs_to_its_own_sequence_and_its_own_candidate() {
    let head = sha("head");
    let proposal = sha("proposal");
    let recorded = candidate_of(MID, 0);
    let publication = |mutate: &dyn Fn(&mut MergePrepared)| {
        let mut prepared = MergePrepared {
            sequence: SequenceId(1),
            disposition: PreparedDisposition::StaleClean,
            expected_head: head.clone(),
            proposed_sha: proposal.clone(),
            key: MID,
            generation: GenerationId(0),
            candidate_sha: recorded.commit_sha.clone(),
            candidate_ref: recorded.candidate_ref.clone(),
            prepared_ref: Some(git_ref("prepared/1")),
            verification_source: VerificationSource::Verification {
                sequence: SequenceId(1),
            },
            verification: Some(verification_record(Verdict::Passed)),
            satisfies: vec![MID],
        };
        mutate(&mut prepared);
        ev(TopologyEventBody::MergePrepared {
            data: Box::new(prepared),
        })
    };

    let mut fold = two_queued();
    apply(
        &mut fold,
        &verification_started(MID, 0, 0, &head, &proposal),
    );
    apply(
        &mut fold,
        &ev(TopologyEventBody::MergeVerificationInterrupted {
            data: MergeVerificationInterrupted {
                sequence: SequenceId(0),
                detail: "  the coordinator died  ".to_owned(),
            },
        }),
    );
    apply(
        &mut fold,
        &verification_started(MID, 0, 1, &head, &proposal),
    );
    accepts(&fold, &publication(&|_| {}));
    assert!(
        matches!(
            refuse(
                &fold,
                &publication(&|prepared| {
                    prepared.verification_source = VerificationSource::Verification {
                        sequence: SequenceId(0),
                    };
                })
            ),
            FoldError::InconsistentRecord { .. }
        ),
        "a publication citing a verification that is not the one that authorized it was \
             accepted"
    );

    let zeta = candidate_of(ZETA, 0);
    assert!(
        matches!(
            refuse(
                &fold,
                &publication(&|prepared| {
                    prepared.key = ZETA;
                    prepared.candidate_sha = zeta.commit_sha.clone();
                    prepared.candidate_ref = zeta.candidate_ref.clone();
                    prepared.satisfies = vec![ZETA];
                })
            ),
            FoldError::InconsistentRecord { .. }
        ),
        "a publication of a candidate the open transaction never verified was authorized"
    );
}

#[test]
fn an_already_present_publication_expects_the_head_its_verification_read() {
    let head = sha("head");
    let mut fold = two_queued();
    apply(
        &mut fold,
        &ev(TopologyEventBody::MergeVerificationStarted {
            data: MergeVerificationStarted {
                sequence: SequenceId(0),
                candidate: candidate_of(MID, 0),
                basis: VerificationBasis::AlreadyPresent,
                expected_head: head.clone(),
                proposed_sha: head.clone(),
            },
        }),
    );
    let recorded = candidate_of(MID, 0);
    let publication = |value: &CommitSha| {
        ev(TopologyEventBody::MergePrepared {
            data: Box::new(MergePrepared {
                sequence: SequenceId(0),
                disposition: PreparedDisposition::AlreadyPresent,
                expected_head: value.clone(),
                proposed_sha: value.clone(),
                key: MID,
                generation: GenerationId(0),
                candidate_sha: recorded.commit_sha.clone(),
                candidate_ref: recorded.candidate_ref.clone(),
                prepared_ref: None,
                verification_source: VerificationSource::Verification {
                    sequence: SequenceId(0),
                },
                verification: Some(verification_record(Verdict::Passed)),
                satisfies: vec![MID],
            }),
        })
    };
    accepts(&fold, &publication(&head));
    let elsewhere = sha("a-head-nobody-verified");
    assert_ne!(elsewhere, head);
    let event = publication(&elsewhere);
    if let TopologyEventBody::MergePrepared { data } = &event.body {
        data.self_consistency()
            .expect("H2/H2 agrees with itself, which is what makes this the missing case");
    }
    assert!(
        matches!(refuse(&fold, &event), FoldError::InconsistentRecord { .. }),
        "an already-present publication at a head the verification never read was authorized"
    );
}

#[test]
fn one_integration_transaction_at_a_time_including_an_authorized_one() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = two_queued();
    apply(&mut fold, &fast_publication(MID, 0, 0, &base, vec![MID]));
    assert!(fold.transaction().is_some());

    assert!(
        matches!(
            refuse(&fold, &verification_started(ZETA, 0, 1, &head, &proposal)),
            FoldError::TransactionAlreadyOpen { .. }
        ),
        "an integration started while a fast publication was still owed"
    );
    assert!(
        matches!(
            refuse(&fold, &fast_publication(ZETA, 0, 1, &base, vec![ZETA])),
            FoldError::TransactionAlreadyOpen { .. }
        ),
        "a second fast publication opened while the first was still owed"
    );

    apply(&mut fold, &merged(MID, 0, 0, vec![MID]));
    assert!(fold.transaction().is_none());
    accepts(&fold, &verification_started(ZETA, 0, 1, &head, &proposal));
}

#[test]
fn the_queue_is_ordered_by_creation_and_not_by_preparation() {
    let base = sha("base");
    let mut fold = started();
    for (key, generation) in [(MID, 0), (ZETA, 0)] {
        apply(&mut fold, &dispatch(key, generation, &base));
        let start = attempt_started(&fold, key, generation, 1, 0);
        apply(&mut fold, &start);
        apply(&mut fold, &candidate_prepared(key, generation, &base));
    }
    apply(&mut fold, &candidate_created(ZETA, 0));
    apply(&mut fold, &candidate_created(MID, 0));

    let entries = fold.queue().expect("started").entries();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.candidate.key)
            .collect::<Vec<_>>(),
        vec![ZETA, MID],
        "the queue is in preparation order rather than creation order"
    );
    let head = sha("head");
    let proposal = sha("proposal");
    assert!(matches!(
        refuse(&fold, &verification_started(MID, 0, 0, &head, &proposal)),
        FoldError::NotFirstEligible { .. }
    ));
    accepts(&fold, &verification_started(ZETA, 0, 0, &head, &proposal));
}

#[test]
fn keys_and_generations_are_dense_in_both_directions() {
    let base = sha("base");
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    let registry_len = fold.registry().expect("started").len();
    assert_eq!(registry_len, 3);

    for key in [0_u32, 1, 2, 4, 9] {
        let mut spawn = repair_spawn(TaskKey(key), ALPHA, ALPHA);
        spawn.entry.key = TaskKey(key);
        assert!(
            matches!(
                refuse(&fold, &spawn_event(spawn)),
                FoldError::NonDenseKey { len: 3, .. }
            ),
            "a spawn at key {key} was registered where the registry holds 3"
        );
    }
    accepts(&fold, &spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)));

    let mut reopened = started();
    merge_task(&mut reopened, ALPHA, 0, 0);
    let mut run = reopened.run.take().expect("started");
    run.tasks[ALPHA.index()].state = TaskState::Pending;
    reopened.run = Some(run);
    assert_eq!(reopened.task(ALPHA).expect("alpha").generations.len(), 1);
    for generation in [0_u32, 2, 9] {
        assert!(
            matches!(
                refuse(&reopened, &dispatch(ALPHA, generation, &base)),
                FoldError::NonDenseKey { len: 1, .. }
            ),
            "generation {generation} was dispatched where the task holds 1"
        );
    }
    accepts(&reopened, &dispatch(ALPHA, 1, &base));
}

#[test]
fn a_wake_clears_every_waiter_in_one_delta() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = started();

    for key in [ALPHA, MID] {
        apply(&mut fold, &dispatch(key, 0, &base));
        let start = attempt_started(&fold, key, 0, 1, 0);
        apply(&mut fold, &start);
        apply(
            &mut fold,
            &settle(
                key,
                0,
                1,
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Deferred {
                        defers: 1,
                        reason: "  the pool is down  ".to_owned(),
                    },
                    lease: LeaseDisposition::PredictedReleased,
                },
            ),
        );
    }
    assert_eq!(fold.task_state(ALPHA), Some(TaskState::Deferred));
    assert_eq!(fold.task_state(MID), Some(TaskState::Deferred));

    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    apply(&mut fold, &candidate_prepared(ZETA, 0, &base));
    apply(&mut fold, &candidate_created(ZETA, 0));
    apply(
        &mut fold,
        &verification_started(ZETA, 0, 0, &head, &proposal),
    );
    apply(
        &mut fold,
        &unavailable_event(0, outage(), UnavailableOutcome::Deferred { defers: 1 }),
    );
    assert!(fold.queue().expect("started").entries()[0].verification_deferred);

    apply(
        &mut fold,
        &ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 30_000,
                round: 1,
            },
        }),
    );
    assert_eq!(
        fold.task_state(ALPHA),
        Some(TaskState::Pending),
        "the first deferred task woke"
    );
    assert_eq!(
        fold.task_state(MID),
        Some(TaskState::Pending),
        "the second deferred task did not wake"
    );
    assert!(
        !fold.queue().expect("started").entries()[0].verification_deferred,
        "the deferred candidate did not wake"
    );
    assert_eq!(fold.derived_outcome(), DerivedOutcome::NotEnding);

    assert_eq!(fold.queue().expect("started").entries()[0].defers, 1);
    apply(
        &mut fold,
        &verification_started(ZETA, 0, 1, &head, &proposal),
    );
    assert!(matches!(
        refuse(
            &fold,
            &unavailable_event(1, outage(), UnavailableOutcome::Deferred { defers: 1 })
        ),
        FoldError::InvalidDefers { .. }
    ));
}

#[test]
fn a_publication_settles_the_closure_the_fold_derives() {
    let base = sha("base");
    let fold = two_queued();
    for satisfies in [vec![], vec![ZETA], vec![MID, ZETA], vec![MID, MID]] {
        let event = fast_publication(MID, 0, 0, &base, satisfies.clone());
        assert!(
            matches!(
                fold.plan_transition(&event),
                Err(FoldError::InvalidSatisfies { .. })
            ),
            "a publication settling {satisfies:?} was authorized"
        );
    }
    accepts(&fold, &fast_publication(MID, 0, 0, &base, vec![MID]));

    let mut lineage = started();
    merge_task(&mut lineage, ALPHA, 0, 0);
    apply(
        &mut lineage,
        &spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)),
    );
    let mut second = repair_spawn(TaskKey(4), ALPHA, TaskKey(3));
    second.entry.display_id = TaskId::from(
        crate::topology::registry::repair_display_id(1, &TaskId::from("alpha")).as_str(),
    );
    second.entry.lineage = Some(Lineage {
        root: ALPHA,
        parent: TaskKey(3),
        index: 1,
    });
    second.entry.deps = vec![ALPHA];
    second.entry.display_deps = vec![TaskId::from("alpha")];
    apply(&mut lineage, &spawn_event(second));
    assert_eq!(
        lineage
            .state()
            .expect("started")
            .satisfies_closure(TaskKey(4)),
        vec![ALPHA, TaskKey(3), TaskKey(4)],
        "a repair settles itself, its parent and its root"
    );
    assert_eq!(
        lineage.state().expect("started").satisfies_closure(ZETA),
        vec![ZETA],
        "an ordinary candidate settles itself alone"
    );
}

#[test]
fn a_merge_copies_the_authorization_exactly() {
    let base = sha("base");
    let mut fold = two_queued();
    assert!(matches!(
        refuse(&fold, &merged(MID, 0, 0, vec![MID])),
        FoldError::WrongSequence { .. }
    ));
    apply(
        &mut fold,
        &verification_started(MID, 0, 0, &sha("head"), &sha("proposal")),
    );
    assert!(matches!(
        refuse(&fold, &merged(MID, 0, 0, vec![MID])),
        FoldError::InconsistentRecord { .. }
    ));

    let mut fast = two_queued();
    apply(&mut fast, &fast_publication(MID, 0, 0, &base, vec![MID]));
    accepts(&fast, &merged(MID, 0, 0, vec![MID]));

    let mut elsewhere = merged(MID, 0, 0, vec![MID]);
    if let TopologyEventBody::TaskMerged { data } = &mut elsewhere.body {
        data.merged_sha = sha("smuggled");
    }
    assert!(matches!(
        refuse(&fast, &elsewhere),
        FoldError::InconsistentRecord { .. }
    ));
    for wrong in [vec![MID, ZETA], vec![MID, MID], Vec::new(), vec![ZETA]] {
        assert!(
            matches!(
                refuse(&fast, &merged(MID, 0, 0, wrong.clone())),
                FoldError::InvalidSatisfies { .. }
            ),
            "a merge settling {wrong:?} was copied from an authorization of [MID]"
        );
    }
    let mut lineage_release = merged(MID, 0, 0, vec![MID]);
    if let TopologyEventBody::TaskMerged { data } = &mut lineage_release.body {
        data.lease_release = MergeLeaseRelease::Lineage { root: MID };
    }
    assert!(matches!(
        refuse(&fast, &lineage_release),
        FoldError::InconsistentRecord { .. }
    ));
    let mut other_candidate = merged(MID, 0, 0, vec![MID]);
    if let TopologyEventBody::TaskMerged { data } = &mut other_candidate.body {
        data.lease_release = MergeLeaseRelease::Candidate {
            key: ZETA,
            generation: GenerationId(0),
        };
    }
    assert!(matches!(
        refuse(&fast, &other_candidate),
        FoldError::InconsistentRecord { .. }
    ));

    apply(&mut fast, &merged(MID, 0, 0, vec![MID]));
    assert_eq!(fast.task_state(MID), Some(TaskState::Merged));
    assert_eq!(fast.queue().expect("started").len(), 1);
    assert!(
        !fast
            .leases()
            .expect("started")
            .holds(LeaseOwner::Candidate {
                key: MID,
                generation: GenerationId(0)
            })
    );
    assert!(fast.transaction().is_none());
}

fn unavailable_event(
    sequence: u32,
    cause: UnavailableCause,
    outcome: UnavailableOutcome,
) -> TopologyEvent {
    ev(TopologyEventBody::MergeVerificationUnavailable {
        data: MergeVerificationUnavailable {
            sequence: SequenceId(sequence),
            cause,
            outcome,
        },
    })
}

fn outage() -> UnavailableCause {
    UnavailableCause::Infrastructure {
        kind: InfrastructureKind::ReviewerTimeout,
    }
}

#[test]
fn a_deferred_verification_is_consecutive_and_within_the_frozen_allowance() {
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = two_queued();
    let max = fold.started().expect("started").limits.max_defers;
    assert_eq!(max, 2, "the fixture's allowance is what this test is about");

    apply(
        &mut fold,
        &verification_started(MID, 0, 0, &head, &proposal),
    );
    for count in [0, 2, 3, 9] {
        assert!(
            matches!(
                fold.plan_transition(&unavailable_event(
                    0,
                    outage(),
                    UnavailableOutcome::Deferred { defers: count }
                )),
                Err(FoldError::InvalidDefers { .. })
            ),
            "a deferral counted {count} where the candidate has 0 was folded"
        );
    }
    assert!(
        matches!(
            refuse(
                &fold,
                &unavailable_event(
                    0,
                    outage(),
                    UnavailableOutcome::Parked {
                        question: question("q-outage-early-Ünicode", MID),
                    },
                )
            ),
            FoldError::InvalidDefers { .. }
        ),
        "an infrastructure outage parked one deferral early, spending an allowance the run \
             still had"
    );
    accepts(
        &fold,
        &unavailable_event(0, outage(), UnavailableOutcome::Deferred { defers: 1 }),
    );
    apply(
        &mut fold,
        &unavailable_event(0, outage(), UnavailableOutcome::Deferred { defers: 1 }),
    );
    assert!(
        fold.queue().expect("started").entries()[0].verification_deferred,
        "a deferred candidate is ineligible until the backoff elapses"
    );
    assert_eq!(fold.task_state(MID), Some(TaskState::AwaitingMerge));
    apply(
        &mut fold,
        &ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 30_000,
                round: 1,
            },
        }),
    );

    apply(
        &mut fold,
        &verification_started(MID, 0, 1, &head, &proposal),
    );
    for count in [0, 1, 2, 3, 9] {
        assert!(
            matches!(
                fold.plan_transition(&unavailable_event(
                    1,
                    outage(),
                    UnavailableOutcome::Deferred { defers: count }
                )),
                Err(FoldError::InvalidDefers { .. })
            ),
            "the allowance was spent and a deferral counted {count} was folded"
        );
    }
    accepts(
        &fold,
        &unavailable_event(
            1,
            outage(),
            UnavailableOutcome::Parked {
                question: question("q-outage-Ünicode", MID),
            },
        ),
    );

    apply(
        &mut fold,
        &unavailable_event(
            1,
            outage(),
            UnavailableOutcome::Parked {
                question: question("q-outage-Ünicode", MID),
            },
        ),
    );
    let other = fold.queue().expect("started").entries()[1]
        .candidate
        .clone();
    assert_ne!(other.key, MID, "the fixture queues two distinct candidates");
    assert_eq!(
        fold.queue().expect("started").entries()[1].defers,
        0,
        "the second candidate has deferred nothing"
    );
    apply(
        &mut fold,
        &verification_started(other.key, other.generation.0, 2, &head, &proposal),
    );
    assert!(
        matches!(
            fold.plan_transition(&unavailable_event(
                2,
                outage(),
                UnavailableOutcome::Deferred { defers: 2 }
            )),
            Err(FoldError::InvalidDefers { .. })
        ),
        "a defer count summed across the queue was accepted for a candidate with none"
    );
    accepts(
        &fold,
        &unavailable_event(2, outage(), UnavailableOutcome::Deferred { defers: 1 }),
    );
}

#[test]
fn an_outage_that_needs_a_person_parks_with_a_question_that_can_be_answered() {
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = two_queued();
    apply(
        &mut fold,
        &verification_started(MID, 0, 0, &head, &proposal),
    );

    assert!(matches!(
        refuse(
            &fold,
            &unavailable_event(
                0,
                UnavailableCause::HumanRequired {
                    verdict: "  a licence question  ".to_owned(),
                },
                UnavailableOutcome::Deferred { defers: 1 },
            )
        ),
        FoldError::InconsistentRecord { .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &unavailable_event(
                0,
                outage(),
                UnavailableOutcome::Parked {
                    question: FrozenQuestion {
                        options: Vec::new(),
                        ..question("q-outage-Ünicode", MID)
                    },
                },
            )
        ),
        FoldError::InconsistentRecord { .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &unavailable_event(
                0,
                outage(),
                UnavailableOutcome::Parked {
                    question: question("q-outage-Ünicode", ZETA),
                },
            )
        ),
        FoldError::UnanswerableQuestion { .. }
    ));

    apply(
        &mut fold,
        &unavailable_event(
            0,
            UnavailableCause::HumanRequired {
                verdict: "  a licence question  ".to_owned(),
            },
            UnavailableOutcome::Parked {
                question: question("q-outage-Ünicode", MID),
            },
        ),
    );
    assert_eq!(fold.task_state(MID), Some(TaskState::AwaitingInput));
    apply(
        &mut fold,
        &answered(
            MID,
            "q-outage-Ünicode",
            Answer4::Answered {
                option_index: 2,
                binding_override: None,
            },
        ),
    );
    assert_eq!(
        fold.task_state(MID),
        Some(TaskState::AwaitingMerge),
        "an answered verification park returns to the queue, not to dispatch"
    );
    accepts(&fold, &verification_started(MID, 0, 1, &head, &proposal));
}

#[test]
fn a_rejection_creates_or_widens_exactly_one_lineage_and_registers_its_repair() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = two_queued();
    apply(
        &mut fold,
        &verification_started(MID, 0, 0, &head, &proposal),
    );

    let rejection = |sequence: u32, mutate: Option<BreakRejection>| {
        let mut rejected = MergeRejected {
            sequence: SequenceId(sequence),
            candidate: candidate_of(MID, 0),
            rejecting_head: head.clone(),
            disposition: RejectionDisposition::CodeRejected {
                verification: verification_record(Verdict::Rejected),
            },
            repair: repair_spawn(TaskKey(3), MID, MID),
            lease_effect: RejectionLeaseEffect::CreatesLineage {
                root: MID,
                paths: region(MID),
            },
        };
        rejected.repair.entry.deps = vec![ALPHA];
        rejected.repair.entry.display_deps = vec![TaskId::from("alpha")];
        if let Some(mutate) = mutate {
            mutate(&mut rejected);
        }
        ev(TopologyEventBody::MergeRejected {
            data: Box::new(rejected),
        })
    };
    assert!(matches!(
        refuse(&fold, &rejection(0, None)),
        FoldError::MalformedEntry { key: 3, .. }
    ));

    let mut ready = started();
    merge_task(&mut ready, ALPHA, 0, 0);
    apply(&mut ready, &dispatch(MID, 0, &base));
    let start = attempt_started(&ready, MID, 0, 1, 0);
    apply(&mut ready, &start);
    apply(&mut ready, &candidate_prepared(MID, 0, &base));
    apply(&mut ready, &candidate_created(MID, 0));
    apply(
        &mut ready,
        &verification_started(MID, 0, 1, &head, &proposal),
    );
    accepts(&ready, &rejection(1, None));

    let cases: [(&str, BreakRejection); 6] = [
        ("a head the verification did not read", |rejected| {
            rejected.rejecting_head = sha("moved-head");
        }),
        ("a verification that passed", |rejected| {
            rejected.disposition = RejectionDisposition::CodeRejected {
                verification: VerificationRecord {
                    verdict: Verdict::Passed,
                    gates_passed: true,
                    reviews: Vec::new(),
                    detail: "  passed  ".to_owned(),
                },
            };
        }),
        ("a lineage rooted elsewhere", |rejected| {
            rejected.lease_effect = RejectionLeaseEffect::CreatesLineage {
                root: ZETA,
                paths: region(MID),
            };
        }),
        ("a widening of a lineage that does not exist", |rejected| {
            rejected.lease_effect = RejectionLeaseEffect::WidensLineage {
                root: MID,
                paths: region(MID),
            };
        }),
        ("a repair parented on another task", |rejected| {
            rejected.repair.entry.lineage = Some(Lineage {
                root: MID,
                parent: ALPHA,
                index: 0,
            });
        }),
        ("a repair numbered as another member", |rejected| {
            rejected.repair.entry.lineage = Some(Lineage {
                root: MID,
                parent: MID,
                index: 3,
            });
        }),
    ];
    for (label, break_it) in cases {
        assert!(
            ready
                .plan_transition(&rejection(1, Some(break_it)))
                .is_err(),
            "a rejection with {label} was folded"
        );
    }

    apply(&mut ready, &rejection(1, None));
    assert_eq!(ready.task_state(MID), Some(TaskState::AwaitingRepair));
    assert_eq!(ready.task_state(TaskKey(3)), Some(TaskState::Pending));
    assert!(
        ready
            .queue()
            .expect("started")
            .get(MID, GenerationId(0))
            .is_none()
    );
    assert!(
        ready
            .leases()
            .expect("started")
            .holds(LeaseOwner::Lineage { root: MID })
    );
    assert!(
        !ready
            .leases()
            .expect("started")
            .holds(LeaseOwner::Candidate {
                key: MID,
                generation: GenerationId(0)
            })
    );
    assert!(ready.transaction().is_none());
}

#[test]
fn a_conflict_opens_and_closes_its_own_transaction() {
    let base = sha("base");
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    apply(&mut fold, &dispatch(MID, 0, &base));
    let start = attempt_started(&fold, MID, 0, 1, 0);
    apply(&mut fold, &start);
    apply(&mut fold, &candidate_prepared(MID, 0, &base));
    apply(&mut fold, &candidate_created(MID, 0));

    let conflict = |sequence: u32| {
        let mut repair = repair_spawn(TaskKey(3), MID, MID);
        repair.entry.deps = vec![ALPHA];
        repair.entry.display_deps = vec![TaskId::from("alpha")];
        ev(TopologyEventBody::MergeRejected {
            data: Box::new(MergeRejected {
                sequence: SequenceId(sequence),
                candidate: candidate_of(MID, 0),
                rejecting_head: sha("head"),
                disposition: RejectionDisposition::Conflict {
                    paths: region(ZETA),
                },
                repair,
                lease_effect: RejectionLeaseEffect::CreatesLineage {
                    root: MID,
                    paths: region(ZETA),
                },
            }),
        })
    };
    assert!(matches!(
        refuse(&fold, &conflict(3)),
        FoldError::NonDenseSequence { .. }
    ));
    apply(&mut fold, &conflict(1));
    assert!(fold.transaction().is_none());

    let leases = fold.leases().expect("started");
    let lineage = leases.lineage(MID).expect("the lineage exists");
    let mut held: Vec<&str> = lineage
        .paths
        .prefixes()
        .expect("a bounded region")
        .iter()
        .map(GitPath::as_str)
        .collect();
    held.sort_unstable();
    assert_eq!(held, vec!["build.rs", "src/Zebra", "src/mid"]);
}

fn answered(key: TaskKey, id: &str, answer: Answer4) -> TopologyEvent {
    ev(TopologyEventBody::QuestionAnswered {
        data: QuestionAnswered4 {
            key,
            question: QuestionId::from(id),
            answer,
            via: "  upstroke answer  ".to_owned(),
        },
    })
}

fn raised(id: &str, key: TaskKey) -> TopologyEvent {
    ev(TopologyEventBody::QuestionRaised {
        data: QuestionRaised4 {
            question: question(id, key),
        },
    })
}

#[test]
fn an_answer_names_an_open_question_of_that_task_and_an_option_it_offered() {
    let mut fold = started();
    apply(&mut fold, &raised("q-park-Ünicode", ZETA));

    assert!(matches!(
        refuse(
            &fold,
            &answered(
                ZETA,
                "q-invented",
                Answer4::Answered {
                    option_index: 0,
                    binding_override: None
                }
            )
        ),
        FoldError::WrongQuestion { .. }
    ));
    assert!(matches!(
        refuse(
            &fold,
            &answered(
                ALPHA,
                "q-park-Ünicode",
                Answer4::Answered {
                    option_index: 0,
                    binding_override: None
                }
            )
        ),
        FoldError::WrongQuestion { .. }
    ));
    for option_index in [3, 4, 99] {
        assert!(matches!(
            refuse(
                &fold,
                &answered(
                    ZETA,
                    "q-park-Ünicode",
                    Answer4::Answered {
                        option_index,
                        binding_override: None
                    }
                )
            ),
            FoldError::WrongQuestion { .. }
        ));
    }
    for option_index in 0..3 {
        accepts(
            &fold,
            &answered(
                ZETA,
                "q-park-Ünicode",
                Answer4::Answered {
                    option_index,
                    binding_override: None,
                },
            ),
        );
    }

    let mismatched = answered(
        ZETA,
        "q-park-Ünicode",
        Answer4::Answered {
            option_index: 1,
            binding_override: Some(BindingOverride {
                key: ZETA,
                question: QuestionId::from("q-park-Ünicode"),
                option_index: 2,
                agent: "copilot".to_owned(),
                model: "gpt-5.6".to_owned(),
                effort: Effort::Low,
            }),
        },
    );
    assert!(matches!(
        refuse(&fold, &mismatched),
        FoldError::InconsistentRecord { .. }
    ));

    apply(
        &mut fold,
        &answered(
            ZETA,
            "q-park-Ünicode",
            Answer4::Answered {
                option_index: 1,
                binding_override: None,
            },
        ),
    );
    let error = refuse(
        &fold,
        &answered(
            ZETA,
            "q-park-Ünicode",
            Answer4::Answered {
                option_index: 0,
                binding_override: None,
            },
        ),
    );
    let FoldError::WrongQuestion { detail, .. } = error else {
        panic!("an already-answered question must be refused as one");
    };
    assert!(
        detail.contains("already been answered"),
        "the refusal has to distinguish an answered question from an invented one: {detail}"
    );
    assert!(matches!(
        refuse(&fold, &raised("q-park-Ünicode", ALPHA)),
        FoldError::WrongQuestion { .. }
    ));
}

#[test]
fn a_decline_fails_its_task_and_halts_only_when_its_recorded_policy_says_so() {
    let mut lenient = started();
    apply(&mut lenient, &raised("q-park-Ünicode", ZETA));
    apply(
        &mut lenient,
        &answered(
            ZETA,
            "q-park-Ünicode",
            Answer4::Declined {
                decline_halts_run: false,
            },
        ),
    );
    assert_eq!(lenient.task_state(ZETA), Some(TaskState::Failed));
    assert_eq!(lenient.halted_at(), None);

    let mut halting = started();
    apply(&mut halting, &raised("q-park-Ünicode", ZETA));
    apply(
        &mut halting,
        &answered(
            ZETA,
            "q-park-Ünicode",
            Answer4::Declined {
                decline_halts_run: true,
            },
        ),
    );
    assert_eq!(halting.task_state(ZETA), Some(TaskState::Failed));
    assert_eq!(halting.halted_at(), Some(ZETA));
}

fn alpha_open_in_every_class() -> Vec<(&'static str, Vec<TopologyEvent>)> {
    let base = sha("base");
    let mut fold = started();
    let dispatched = dispatch(ALPHA, 0, &base);
    apply(&mut fold, &dispatched);
    let start = attempt_started(&fold, ALPHA, 0, 1, 0);

    vec![
        ("open with no attempt", vec![dispatched.clone()]),
        ("in flight", vec![dispatched.clone(), start.clone()]),
        (
            "retained idle",
            vec![
                dispatched.clone(),
                start.clone(),
                settle(
                    ALPHA,
                    0,
                    1,
                    AttemptSettlement::Retained {
                        retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                        retained_incarnation: Epoch(0),
                    },
                ),
            ],
        ),
        (
            "promoting",
            vec![dispatched, start, candidate_prepared(ALPHA, 0, &base)],
        ),
    ]
}

fn folded(events: &[TopologyEvent]) -> (TopologyFold, Vec<TopologyEvent>) {
    let mut fold = started();
    let mut log = vec![run_started_event()];
    for event in events {
        apply(&mut fold, event);
        log.push(event.clone());
    }
    (fold, log)
}

#[track_caller]
fn refused_live_and_on_replay(
    fold: &TopologyFold,
    log: &[TopologyEvent],
    event: &TopologyEvent,
) -> FoldError {
    let before = fold.state().cloned();
    let live = refuse(fold, event);
    assert_eq!(fold.state().cloned(), before);
    let mut replayed = log.to_vec();
    replayed.push(event.clone());
    let on_replay = TopologyFold::replay(inputs(), &replayed)
        .err()
        .unwrap_or_else(|| {
            panic!(
                "a replay of the log plus `{}` must refuse",
                event.body.kind()
            )
        });
    assert_eq!(
        live, on_replay,
        "the live path and the replay refuse differently"
    );
    live
}

#[test]
fn a_bare_question_is_refused_while_its_task_holds_an_open_generation() {
    for (class, events) in alpha_open_in_every_class() {
        let (fold, log) = folded(&events);
        let error = refused_live_and_on_replay(&fold, &log, &raised("q-park-Ünicode", ALPHA));
        assert_eq!(
            error,
            FoldError::InconsistentRecord {
                kind: "question_raised",
                detail: format!(
                    "lineage 1 has task 1 generation 0 still {class}; settle it before parking its tasks"
                ),
            },
            "a generation that is {class} is an open generation"
        );

        let mut parked = fold.clone();
        apply(&mut parked, &raised("q-park-Ünicode", MID));
        assert_eq!(parked.task_state(MID), Some(TaskState::AwaitingInput));
    }
}

#[test]
fn the_question_an_attempt_raises_rides_on_its_settlement_and_a_decline_then_ends_the_run() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ALPHA, 0, &base));
    let start = attempt_started(&fold, ALPHA, 0, 1, 0);
    apply(&mut fold, &start);
    let lease = LeaseOwner::Generation {
        key: ALPHA,
        generation: GenerationId(0),
    };
    assert!(fold.leases().is_some_and(|leases| leases.holds(lease)));
    assert!(matches!(
        refuse(&fold, &raised("q-park-Ünicode", ALPHA)),
        FoldError::InconsistentRecord {
            kind: "question_raised",
            ..
        }
    ));

    apply(
        &mut fold,
        &settle(
            ALPHA,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Parked {
                    question: question("q-park-Ünicode", ALPHA),
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    );
    assert_eq!(fold.task_state(ALPHA), Some(TaskState::AwaitingInput));
    assert!(fold.task(ALPHA).is_some_and(|task| task.open().is_none()));
    assert!(fold.leases().is_some_and(|leases| !leases.holds(lease)));

    apply(
        &mut fold,
        &answered(
            ALPHA,
            "q-park-Ünicode",
            Answer4::Declined {
                decline_halts_run: true,
            },
        ),
    );

    assert_eq!(fold.task_state(ALPHA), Some(TaskState::Failed));
    assert_eq!(fold.halted_at(), Some(ALPHA));
    assert!(fold.task(ALPHA).is_some_and(|task| task.open().is_none()));
    assert!(fold.leases().is_some_and(|leases| !leases.holds(lease)));

    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Halted)
    );
    accepts(&fold, &run_finished(RunOutcome::Halted, Some(ALPHA)));
}

#[test]
fn bare_questions_refuse_terminal_tasks_and_allow_quiet_parked_lineages() {
    let base = sha("base");
    let mut merged_log = Vec::new();
    {
        let mut fold = started();
        let start = attempt_started(&fold, ALPHA, 0, 1, 0);
        for event in [
            dispatch(ALPHA, 0, &base),
            start,
            candidate_prepared(ALPHA, 0, &base),
            candidate_created(ALPHA, 0),
            fast_publication(ALPHA, 0, 0, &base, vec![ALPHA]),
            merged(ALPHA, 0, 0, vec![ALPHA]),
        ] {
            apply(&mut fold, &event);
            merged_log.push(event);
        }
    }
    let mut failed_log = Vec::new();
    {
        let mut fold = started();
        let start = attempt_started(&fold, ALPHA, 0, 1, 0);
        for event in [
            dispatch(ALPHA, 0, &base),
            start,
            settle(
                ALPHA,
                0,
                1,
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Failed {
                        halts_run: false,
                        reason: "  the fixture's terminal failure  ".to_owned(),
                    },
                    lease: LeaseDisposition::PredictedReleased,
                },
            ),
        ] {
            apply(&mut fold, &event);
            failed_log.push(event);
        }
    }

    let parked_log = vec![raised("q-first-Ünicode", ALPHA)];

    let head = sha("head");
    let proposal = sha("proposal");
    let mut repair_log = Vec::new();
    {
        let mut fold = started();
        let start = attempt_started(&fold, ALPHA, 0, 1, 0);
        let mut rejected = MergeRejected {
            sequence: SequenceId(0),
            candidate: candidate_of(ALPHA, 0),
            rejecting_head: head.clone(),
            disposition: RejectionDisposition::CodeRejected {
                verification: verification_record(Verdict::Rejected),
            },
            repair: repair_spawn(TaskKey(3), ALPHA, ALPHA),
            lease_effect: RejectionLeaseEffect::CreatesLineage {
                root: ALPHA,
                paths: region(ALPHA),
            },
        };
        rejected.repair.entry.deps = Vec::new();
        rejected.repair.entry.display_deps = Vec::new();
        for event in [
            dispatch(ALPHA, 0, &base),
            start,
            candidate_prepared(ALPHA, 0, &base),
            candidate_created(ALPHA, 0),
            verification_started(ALPHA, 0, 0, &head, &proposal),
            ev(TopologyEventBody::MergeRejected {
                data: Box::new(rejected),
            }),
        ] {
            apply(&mut fold, &event);
            repair_log.push(event);
        }
    }
    for (state, events) in [("merged", merged_log), ("failed", failed_log)] {
        let (fold, log) = folded(&events);
        assert_eq!(fold.task_state(ALPHA).map(TaskState::name), Some(state));
        let error = refused_live_and_on_replay(&fold, &log, &raised("q-second-Ünicode", ALPHA));
        assert_eq!(
            error,
            FoldError::WrongTaskState {
                kind: "question_raised",
                key: 1,
                state,
                expected: "nonterminal",
            }
        );
    }
    for (state, events) in [
        (TaskState::AwaitingInput, parked_log),
        (TaskState::AwaitingRepair, repair_log),
    ] {
        let (mut fold, mut log) = folded(&events);
        for event in [
            raised("q-second-Ünicode", ALPHA),
            answered(
                ALPHA,
                "q-second-Ünicode",
                Answer4::Answered {
                    option_index: 0,
                    binding_override: None,
                },
            ),
        ] {
            apply(&mut fold, &event);
            log.push(event);
        }
        assert_eq!(fold.task_state(ALPHA), Some(state));
        let replayed = TopologyFold::replay(inputs(), &log).expect("accepted questions replay");
        assert_eq!(fold.state(), replayed.state());
    }
}

#[test]
fn a_bare_question_is_refused_on_the_candidate_under_integration() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut events = Vec::new();
    {
        let mut fold = started();
        let start = attempt_started(&fold, ALPHA, 0, 1, 0);
        for event in [
            dispatch(ALPHA, 0, &base),
            start,
            candidate_prepared(ALPHA, 0, &base),
            candidate_created(ALPHA, 0),
            verification_started(ALPHA, 0, 0, &head, &proposal),
        ] {
            apply(&mut fold, &event);
            events.push(event);
        }
    }
    let (fold, log) = folded(&events);
    assert_eq!(fold.task_state(ALPHA), Some(TaskState::AwaitingMerge));
    let error = refused_live_and_on_replay(&fold, &log, &raised("q-park-Ünicode", ALPHA));
    let FoldError::InconsistentRecord { kind, detail } = error else {
        panic!("a question on the candidate under integration is refused as one: {error}");
    };
    assert_eq!(kind, "question_raised");
    assert!(detail.contains("sequence 0"), "{detail}");

    let mut released = fold.clone();
    apply(
        &mut released,
        &ev(TopologyEventBody::MergeVerificationInterrupted {
            data: MergeVerificationInterrupted {
                sequence: SequenceId(0),
                detail: "  the coordinator died  ".to_owned(),
            },
        }),
    );
    apply(&mut released, &raised("q-park-Ünicode", ALPHA));
    assert_eq!(released.task_state(ALPHA), Some(TaskState::AwaitingInput));
}

#[test]
fn a_task_whose_candidate_is_queued_is_not_dispatched_again() {
    let base = sha("base");
    let mut events = Vec::new();
    {
        let mut fold = started();
        let start = attempt_started(&fold, ALPHA, 0, 1, 0);
        for event in [
            dispatch(ALPHA, 0, &base),
            start,
            candidate_prepared(ALPHA, 0, &base),
            candidate_created(ALPHA, 0),
            raised("q-park-Ünicode", ALPHA),
            answered(
                ALPHA,
                "q-park-Ünicode",
                Answer4::Answered {
                    option_index: 0,
                    binding_override: None,
                },
            ),
        ] {
            apply(&mut fold, &event);
            events.push(event);
        }
    }
    let (fold, log) = folded(&events);

    assert_eq!(fold.task_state(ALPHA), Some(TaskState::AwaitingMerge));
    assert!(fold.queue().is_some_and(|queue| queue.holds_task(ALPHA)));
    assert!(
        !fold.ready(ALPHA),
        "`ready` has always refused a queued task"
    );
    let error = refused_live_and_on_replay(&fold, &log, &dispatch(ALPHA, 1, &base));
    assert!(matches!(
        error,
        FoldError::WrongTaskState {
            kind: "task_dispatched",
            state: "awaiting merge",
            expected: "pending",
            ..
        }
    ));

    let mut integrated = fold.clone();
    apply(
        &mut integrated,
        &fast_publication(ALPHA, 0, 0, &base, vec![ALPHA]),
    );
    apply(&mut integrated, &merged(ALPHA, 0, 0, vec![ALPHA]));
    assert_eq!(integrated.task_state(ALPHA), Some(TaskState::Merged));
    assert!(matches!(
        refuse(&integrated, &dispatch(ALPHA, 1, &base)),
        FoldError::WrongTaskState {
            state: "merged",
            ..
        }
    ));
}

#[test]
fn an_answer_is_refused_after_a_halt_or_a_budget_stop_in_the_same_epoch() {
    let base = sha("base");
    let mut budget = started();
    apply(&mut budget, &raised("q-park-Ünicode", ZETA));
    apply(&mut budget, &budget_exceeded(0, Some(ZETA)));
    let answer = answered(
        ZETA,
        "q-park-Ünicode",
        Answer4::Answered {
            option_index: 0,
            binding_override: None,
        },
    );
    assert_eq!(
        refuse(&budget, &answer),
        FoldError::RunEnding {
            kind: "question_answered",
            what: "the budget stop",
        }
    );
    apply(&mut budget, &resume(container_runner()));
    accepts(&budget, &answer);

    let mut halted = started();
    apply(&mut halted, &dispatch(ALPHA, 0, &base));
    let start = attempt_started(&halted, ALPHA, 0, 1, 0);
    apply(&mut halted, &start);
    apply(&mut halted, &raised("q-park-Ünicode", ZETA));
    apply(
        &mut halted,
        &settle(
            ALPHA,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Failed {
                    halts_run: true,
                    reason: "  the ladder ran out  ".to_owned(),
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    );
    assert_eq!(
        refuse(&halted, &answer),
        FoldError::RunEnding {
            kind: "question_answered",
            what: "a halting settlement",
        }
    );
    apply(&mut halted, &resume(container_runner()));
    assert_eq!(halted.halted_at(), Some(ALPHA));
}

fn budget_exceeded(epoch: u32, key: Option<TaskKey>) -> TopologyEvent {
    ev(TopologyEventBody::BudgetExceeded {
        data: BudgetExceeded4 {
            epoch: Epoch(epoch),
            budget: BudgetKind::Run,
            limit_usd: 12.5,
            spent_usd: 13.75,
            key,
        },
    })
}

#[test]
fn a_budget_stop_belongs_to_the_epoch_that_hit_the_ceiling() {
    let mut fold = started();
    assert!(matches!(
        refuse(&fold, &budget_exceeded(3, None)),
        FoldError::InconsistentRecord { .. }
    ));
    assert!(matches!(
        refuse(&fold, &budget_exceeded(0, Some(TaskKey(9)))),
        FoldError::UnknownKey { key: 9, .. }
    ));
    apply(&mut fold, &budget_exceeded(0, Some(ZETA)));
    assert_eq!(
        fold.budget_stop(),
        Some(BudgetStop {
            epoch: Epoch(0),
            budget: BudgetKind::Run,
        })
    );
    apply(&mut fold, &resume(container_runner()));
    assert_eq!(fold.budget_stop(), None);
    assert!(matches!(
        refuse(&fold, &budget_exceeded(0, None)),
        FoldError::InconsistentRecord { .. }
    ));
    accepts(&fold, &budget_exceeded(1, None));
}

#[test]
fn a_wait_never_elapses_under_a_halt_or_a_budget_stop() {
    let base = sha("base");
    let elapsed = ev(TopologyEventBody::DeferWaitElapsed {
        data: DeferWaitElapsed4 {
            waited_ms: 30_000,
            round: 1,
        },
    });

    let mut deferred = started();
    apply(&mut deferred, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&deferred, ZETA, 0, 1, 0);
    apply(&mut deferred, &start);
    apply(
        &mut deferred,
        &settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Deferred {
                    defers: 1,
                    reason: "  the pool was down  ".to_owned(),
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    );
    assert_eq!(deferred.task_state(ZETA), Some(TaskState::Deferred));
    accepts(&deferred, &elapsed);

    let mut budget = deferred.clone();
    apply(&mut budget, &budget_exceeded(0, None));
    assert_eq!(
        refuse(&budget, &elapsed),
        FoldError::RunEnding {
            kind: "defer_wait_elapsed",
            what: "the budget stop",
        }
    );
    apply(&mut budget, &resume(container_runner()));
    assert_eq!(
        budget.task_state(ZETA),
        Some(TaskState::Pending),
        "a resume wakes what the wait would have"
    );

    let mut halted = deferred.clone();
    apply(&mut halted, &dispatch(ALPHA, 0, &base));
    let start = attempt_started(&halted, ALPHA, 0, 1, 0);
    apply(&mut halted, &start);
    apply(
        &mut halted,
        &settle(
            ALPHA,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Failed {
                    halts_run: true,
                    reason: "  the ladder ran out  ".to_owned(),
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    );
    assert_eq!(
        refuse(&halted, &elapsed),
        FoldError::RunEnding {
            kind: "defer_wait_elapsed",
            what: "a halting settlement",
        }
    );

    let head = sha("head");
    let proposal = sha("proposal");
    let mut both = two_queued();
    apply(
        &mut both,
        &verification_started(MID, 0, 0, &head, &proposal),
    );
    apply(
        &mut both,
        &unavailable_event(0, outage(), UnavailableOutcome::Deferred { defers: 1 }),
    );
    assert!(both.queue().expect("started").entries()[0].verification_deferred);
    apply(&mut both, &elapsed);
    assert!(
        both.queue()
            .expect("started")
            .entries()
            .iter()
            .all(|entry| !entry.verification_deferred),
        "one wait wakes every waiter, so the order they deferred in cannot become an order \
             they retry in"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocker {
    Nothing,
    OpenNoAttempt,
    OpenGeneration,
    Promoting,
    RetainedIdle,
    Transaction,
    VerifyingTransaction,
}

const BLOCKERS: [Blocker; 7] = [
    Blocker::Nothing,
    Blocker::OpenNoAttempt,
    Blocker::OpenGeneration,
    Blocker::Promoting,
    Blocker::RetainedIdle,
    Blocker::Transaction,
    Blocker::VerifyingTransaction,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Budget {
    None,
    Older,
    Current,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backoff {
    None,
    DeferredTask,
    DeferredCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    AllTerminal,
    BlockedByFailure,
    AdmissiblePending,
    Stuck,
}

impl Shape {
    fn admissible(self) -> bool {
        self == Self::AdmissiblePending
    }

    fn complete(self) -> bool {
        matches!(self, Self::AllTerminal | Self::BlockedByFailure)
    }
}

fn expected_outcome(
    blocker: Blocker,
    halting: bool,
    budget: Budget,
    backoff: Backoff,
    questions: bool,
    shape: Shape,
) -> DerivedOutcome {
    if blocker != Blocker::Nothing {
        return DerivedOutcome::NotEnding;
    }
    if halting {
        return DerivedOutcome::Ending(RunOutcome::Halted);
    }
    if budget == Budget::Current {
        return DerivedOutcome::Ending(RunOutcome::BudgetExceeded);
    }
    if shape.admissible() || backoff != Backoff::None {
        return DerivedOutcome::NotEnding;
    }
    if questions {
        return DerivedOutcome::Ending(RunOutcome::Parked);
    }
    if shape.complete() {
        return DerivedOutcome::Ending(RunOutcome::Complete);
    }
    DerivedOutcome::FoldError
}

fn grid_state(
    blocker: Blocker,
    halting: bool,
    budget: Budget,
    backoff: Backoff,
    questions: bool,
    shape: Shape,
) -> TopologyFold {
    let mut fold = started();
    fold.run = Some({
        let mut run = fold.run.take().expect("started");
        match shape {
            Shape::AllTerminal => {
                for task in &mut run.tasks {
                    task.state = TaskState::Merged;
                }
            }
            Shape::BlockedByFailure => {
                run.tasks[ALPHA.index()].state = TaskState::Failed;
                run.tasks[ZETA.index()].state = TaskState::Pending;
                run.tasks[MID.index()].state = TaskState::Pending;
            }
            Shape::AdmissiblePending => {
                run.tasks[ALPHA.index()].state = TaskState::Merged;
                run.tasks[ZETA.index()].state = TaskState::Pending;
                run.tasks[MID.index()].state = TaskState::Merged;
            }
            Shape::Stuck => {
                run.tasks[ALPHA.index()].state = TaskState::Merged;
                run.tasks[ZETA.index()].state = TaskState::AwaitingRepair;
                run.tasks[MID.index()].state = TaskState::Merged;
            }
        }
        let open = |class: GenerationClass| GenerationFold {
            id: GenerationId(0),
            class,
            base_sha: sha("base"),
            lease: GenerationLease::Own,
            attempts: 1,
            candidate: None,
        };
        let generations = &mut run.tasks[MID.index()].generations;
        match blocker {
            Blocker::Nothing => {}
            Blocker::OpenNoAttempt => generations.push(open(GenerationClass::OpenNoAttempt)),
            Blocker::OpenGeneration => generations.push(open(GenerationClass::InFlight {
                attempt: AttemptNumber(1),
            })),
            Blocker::Promoting => generations.push(open(GenerationClass::Promoting)),
            Blocker::RetainedIdle => {
                let incarnation = run.epoch;
                run.tasks[MID.index()]
                    .generations
                    .push(open(GenerationClass::RetainedIdle {
                        session: SessionId("sess-ÜNI-0042".to_owned()),
                        incarnation,
                    }));
            }
            Blocker::Transaction => {
                run.transaction = Some(Transaction {
                    sequence: SequenceId(0),
                    candidate: candidate_of(MID, 0),
                    class: TransactionClass::Prepared {
                        proposed_sha: sha("commit-2-0"),
                        satisfies: vec![MID],
                    },
                });
            }
            Blocker::VerifyingTransaction => {
                run.transaction = Some(Transaction {
                    sequence: SequenceId(0),
                    candidate: candidate_of(MID, 0),
                    class: TransactionClass::VerificationStarted {
                        basis: VerificationBasis::AlreadyPresent,
                        expected_head: sha("head"),
                        proposed_sha: sha("head"),
                    },
                });
            }
        }
        if halting {
            run.halted_at = Some(ALPHA);
            run.halted_epoch = Some(run.epoch);
        }
        run.budget_stop = match budget {
            Budget::None => None,
            Budget::Older => Some(BudgetStop {
                epoch: Epoch(run.epoch.0 + 1),
                budget: BudgetKind::Task,
            }),
            Budget::Current => Some(BudgetStop {
                epoch: run.epoch,
                budget: BudgetKind::Run,
            }),
        };
        match backoff {
            Backoff::None => {}
            Backoff::DeferredTask => run.set_state(MID, TaskState::Deferred),
            Backoff::DeferredCandidate => run.queue.push(QueueEntry {
                candidate: candidate_of(MID, 0),
                paths: region(MID),
                lineage_root: None,
                verification_deferred: true,
                defers: 1,
                sequence: None,
            }),
        }
        if questions {
            run.open_question(
                &question("q-grid-Ünicode", MID),
                QuestionOrigin::Admission,
                None,
            );
        }
        run
    });
    fold
}

#[test]
fn the_derived_outcome_is_total_over_the_crossed_fold_state() {
    let mut cells = 0;
    let mut reached: BTreeSet<String> = BTreeSet::new();
    for blocker in BLOCKERS {
        for halting in [false, true] {
            for budget in [Budget::None, Budget::Older, Budget::Current] {
                for backoff in [
                    Backoff::None,
                    Backoff::DeferredTask,
                    Backoff::DeferredCandidate,
                ] {
                    for questions in [false, true] {
                        for shape in [
                            Shape::AllTerminal,
                            Shape::BlockedByFailure,
                            Shape::AdmissiblePending,
                            Shape::Stuck,
                        ] {
                            let fold =
                                grid_state(blocker, halting, budget, backoff, questions, shape);
                            let expected = expected_outcome(
                                blocker, halting, budget, backoff, questions, shape,
                            );
                            assert_eq!(
                                fold.derived_outcome(),
                                expected,
                                "blocker {blocker:?}, halting {halting}, budget {budget:?}, \
                                     backoff {backoff:?}, questions {questions}, shape {shape:?}"
                            );
                            reached.insert(format!("{expected:?}"));
                            cells += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cells, 1008);
    assert_eq!(reached.len(), 6, "arms reached: {reached:?}");
}

#[test]
fn pending_backoff_blocks_parked_and_complete_and_never_blocks_halted_or_budget() {
    for blocker in BLOCKERS {
        for halting in [false, true] {
            for budget in [Budget::None, Budget::Current] {
                for questions in [false, true] {
                    for shape in [
                        Shape::AllTerminal,
                        Shape::BlockedByFailure,
                        Shape::AdmissiblePending,
                        Shape::Stuck,
                    ] {
                        let without =
                            grid_state(blocker, halting, budget, Backoff::None, questions, shape)
                                .derived_outcome();
                        for backoff in [Backoff::DeferredTask, Backoff::DeferredCandidate] {
                            let with =
                                grid_state(blocker, halting, budget, backoff, questions, shape)
                                    .derived_outcome();
                            let expected = match &without {
                                DerivedOutcome::Ending(RunOutcome::Parked)
                                | DerivedOutcome::Ending(RunOutcome::Complete)
                                | DerivedOutcome::FoldError => DerivedOutcome::NotEnding,
                                other => other.clone(),
                            };
                            assert_eq!(
                                with, expected,
                                "{backoff:?} against {without:?} (blocker {blocker:?}, \
                                     halting {halting}, budget {budget:?}, questions {questions}, \
                                     shape {shape:?})"
                            );
                        }
                    }
                }
            }
        }
    }
}

fn run_finished(outcome: RunOutcome, halted_at: Option<TaskKey>) -> TopologyEvent {
    ev(TopologyEventBody::RunFinished {
        data: RunFinished4 {
            outcome,
            halted_at,
            merged: 1,
            parked: 0,
        },
    })
}

#[test]
fn a_run_ends_at_the_outcome_its_state_implies_or_not_at_all() {
    let outcomes = [
        RunOutcome::Complete,
        RunOutcome::Parked,
        RunOutcome::Halted,
        RunOutcome::BudgetExceeded,
    ];

    let complete = grid_state(
        Blocker::Nothing,
        false,
        Budget::None,
        Backoff::None,
        false,
        Shape::AllTerminal,
    );
    assert_accepts_exactly(&complete, &outcomes, RunOutcome::Complete, None);

    let parked = grid_state(
        Blocker::Nothing,
        false,
        Budget::None,
        Backoff::None,
        true,
        Shape::AllTerminal,
    );
    assert_accepts_exactly(&parked, &outcomes, RunOutcome::Parked, None);

    let halted = grid_state(
        Blocker::Nothing,
        true,
        Budget::Current,
        Backoff::DeferredTask,
        true,
        Shape::Stuck,
    );
    assert_accepts_exactly(&halted, &outcomes, RunOutcome::Halted, Some(ALPHA));

    let budget = grid_state(
        Blocker::Nothing,
        false,
        Budget::Current,
        Backoff::DeferredCandidate,
        true,
        Shape::Stuck,
    );
    assert_accepts_exactly(&budget, &outcomes, RunOutcome::BudgetExceeded, None);

    let running = grid_state(
        Blocker::OpenGeneration,
        false,
        Budget::None,
        Backoff::None,
        false,
        Shape::AdmissiblePending,
    );
    for outcome in &outcomes {
        assert!(matches!(
            refuse(&running, &run_finished(outcome.clone(), None)),
            FoldError::OutcomeMismatch { .. }
        ));
    }

    assert!(matches!(
        refuse(&halted, &run_finished(RunOutcome::Halted, None)),
        FoldError::InconsistentRecord { .. }
    ));
    assert!(matches!(
        refuse(&halted, &run_finished(RunOutcome::Halted, Some(MID))),
        FoldError::InconsistentRecord { .. }
    ));
    assert!(matches!(
        refuse(&complete, &run_finished(RunOutcome::Complete, Some(ALPHA))),
        FoldError::InconsistentRecord { .. }
    ));
}

#[track_caller]
fn assert_accepts_exactly(
    fold: &TopologyFold,
    outcomes: &[RunOutcome; 4],
    accepted: RunOutcome,
    halted_at: Option<TaskKey>,
) {
    for outcome in outcomes {
        let event = run_finished(outcome.clone(), halted_at);
        if *outcome == accepted {
            accepts(fold, &event);
        } else {
            assert!(
                matches!(
                    fold.plan_transition(&event),
                    Err(FoldError::OutcomeMismatch { .. })
                ),
                "`{outcome:?}` was accepted where the state implies `{accepted:?}`"
            );
        }
    }
}

#[test]
fn a_finished_run_is_continued_only_by_the_resume_its_outcome_allows() {
    let base = sha("base");
    for (outcome, resumable) in [
        (RunOutcome::Complete, false),
        (RunOutcome::Halted, false),
        (RunOutcome::Parked, true),
        (RunOutcome::BudgetExceeded, true),
    ] {
        let (mut fold, halted_at) = match outcome {
            RunOutcome::Complete => (
                grid_state(
                    Blocker::Nothing,
                    false,
                    Budget::None,
                    Backoff::None,
                    false,
                    Shape::AllTerminal,
                ),
                None,
            ),
            RunOutcome::Parked => (
                grid_state(
                    Blocker::Nothing,
                    false,
                    Budget::None,
                    Backoff::None,
                    true,
                    Shape::AllTerminal,
                ),
                None,
            ),
            RunOutcome::Halted => (
                grid_state(
                    Blocker::Nothing,
                    true,
                    Budget::None,
                    Backoff::None,
                    false,
                    Shape::AllTerminal,
                ),
                Some(ALPHA),
            ),
            RunOutcome::BudgetExceeded => (
                grid_state(
                    Blocker::Nothing,
                    false,
                    Budget::Current,
                    Backoff::None,
                    false,
                    Shape::AllTerminal,
                ),
                None,
            ),
        };
        apply(&mut fold, &run_finished(outcome.clone(), halted_at));
        assert_eq!(fold.finished(), Some(&outcome));

        let continuation = dispatch(ZETA, 0, &base);
        assert!(
            matches!(
                refuse(&fold, &continuation),
                FoldError::RunIsOver {
                    kind: "task_dispatched",
                    ..
                }
            ),
            "a {outcome:?} run continued with ordinary work"
        );
        let resumption = resume(container_runner());
        if resumable {
            accepts(&fold, &resumption);
            apply(&mut fold, &resumption);
            assert_eq!(
                fold.finished(),
                None,
                "a resume reopens the run it continues"
            );
            assert!(
                !matches!(
                    fold.plan_transition(&continuation),
                    Err(FoldError::RunIsOver { .. })
                ),
                "a resumed run still refuses ordinary work as a finished one"
            );
        } else {
            assert!(
                matches!(refuse(&fold, &resumption), FoldError::RunIsOver { .. }),
                "a {outcome:?} run was resumed"
            );
        }
    }
}

fn every_kind() -> Vec<TopologyEvent> {
    let base = sha("base");
    let events = vec![
        run_started_event(),
        resume(container_runner()),
        spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)),
        dispatch(ZETA, 0, &base),
        ev(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key: ZETA,
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                rung: 0,
                binding: RungBinding {
                    tier: Tier::Small,
                    agent: "zeta-small-agent".to_owned(),
                    model: "zeta-small-model".to_owned(),
                    pinned: false,
                    effort: Effort::Low,
                },
                pool: None,
                resume_session: None,
                materialization_observed: None,
            },
        }),
        settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Retry,
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
        ev(TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key: ZETA,
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                lease: LeaseDisposition::PredictedReleased,
                detail: "  the coordinator died  ".to_owned(),
            },
        }),
        ev(TopologyEventBody::GenerationClosed {
            data: GenerationClosed {
                key: ZETA,
                generation: GenerationId(0),
                reason: GenerationCloseReason::WorktreeMissing,
                lease: LeaseDisposition::PredictedReleased,
            },
        }),
        ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 30_000,
                round: 1,
            },
        }),
        candidate_prepared(ZETA, 0, &base),
        candidate_created(ZETA, 0),
        verification_started(ZETA, 0, 0, &sha("head"), &sha("proposal")),
        unavailable_event(0, outage(), UnavailableOutcome::Deferred { defers: 1 }),
        ev(TopologyEventBody::MergeVerificationInterrupted {
            data: MergeVerificationInterrupted {
                sequence: SequenceId(0),
                detail: "  the coordinator died  ".to_owned(),
            },
        }),
        fast_publication(ZETA, 0, 0, &base, vec![ZETA]),
        ev(TopologyEventBody::MergeRejected {
            data: Box::new(MergeRejected {
                sequence: SequenceId(0),
                candidate: candidate_of(ZETA, 0),
                rejecting_head: sha("head"),
                disposition: RejectionDisposition::Conflict { paths: region(MID) },
                repair: repair_spawn(TaskKey(3), ZETA, ZETA),
                lease_effect: RejectionLeaseEffect::CreatesLineage {
                    root: ZETA,
                    paths: region(MID),
                },
            }),
        }),
        merged(ZETA, 0, 0, vec![ZETA]),
        raised("q-park-Ünicode", ZETA),
        answered(
            ZETA,
            "q-park-Ünicode",
            Answer4::Declined {
                decline_halts_run: true,
            },
        ),
        budget_exceeded(0, Some(ZETA)),
        run_finished(RunOutcome::Complete, None),
        ev(TopologyEventBody::CapacitySnapshot {
            data: CapacitySnapshot {
                strategy: "  Least-Loaded  ".to_owned(),
                pools: vec![PoolSnapshot {
                    pool: "codex-plus".to_owned(),
                    agent: "  Codex-CLI  ".to_owned(),
                    kind: "session".to_owned(),
                    remaining: "3".to_owned(),
                    confidence: "reported".to_owned(),
                    reset_at: Some("2026-08-17T10:00:00Z".to_owned()),
                }],
            },
        }),
        ev(TopologyEventBody::PoolExhausted {
            data: PoolExhausted {
                pool: "codex-plus".to_owned(),
                agent: "  Codex-CLI  ".to_owned(),
                reset_at: Some("2026-08-17T10:00:00Z".to_owned()),
                detail: "  rate limited  ".to_owned(),
            },
        }),
        ev(TopologyEventBody::DesignDefect {
            data: DesignDefect {
                question: QuestionId::from("q-design"),
                context: "  the contract is ambiguous  ".to_owned(),
                answer: "  ask the designer  ".to_owned(),
            },
        }),
    ];
    assert_eq!(
        events.len(),
        TOPOLOGY_EVENT_KINDS.len(),
        "the table has to hold one of every kind"
    );
    for (event, kind) in events.iter().zip(TOPOLOGY_EVENT_KINDS) {
        assert_eq!(event.body.kind(), kind, "the table is in vocabulary order");
    }
    events
}

#[test]
fn a_poisoned_fold_refuses_every_transition() {
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    fold.poison();
    assert!(fold.is_poisoned());
    for event in every_kind() {
        assert_eq!(
            refuse(&fold, &event),
            FoldError::Poisoned,
            "`{}` was folded into a poisoned state",
            event.body.kind()
        );
    }
    let mut clean = started();
    merge_task(&mut clean, ALPHA, 0, 0);
    assert!(
        clean
            .plan_transition(&raised("q-park-Ünicode", ZETA))
            .is_ok(),
        "the same event applies to the same state when it is not poisoned"
    );
}

#[test]
fn a_committed_line_that_is_not_an_event_is_a_rewritten_log() {
    let first = serde_json::to_string(&run_started_event()).expect("serialize");
    let second = serde_json::to_string(&raised("q-park-Ünicode", ZETA)).expect("serialize");

    let whole = format!("{first}\n{second}\n");
    assert_eq!(
        TopologyFold::parse_log(whole.as_bytes())
            .expect("a whole log parses")
            .len(),
        2
    );

    let torn = format!("{first}\n{second}");
    let parsed = TopologyFold::parse_log(torn.as_bytes()).expect("a torn tail is not an error");
    assert_eq!(parsed.len(), 1, "an uncommitted line is not an event");

    for position in 0..3 {
        let mut lines = [first.clone(), second.clone(), second.clone()];
        lines[position] = "{\"event\":\"not_a_kind\"}".to_owned();
        let log = lines.join("\n") + "\n";
        let error = TopologyFold::parse_log(log.as_bytes())
            .expect_err("a committed invalid line is refused");
        let FoldError::RewrittenLog { line, .. } = error else {
            panic!("a rewritten log must be refused as one");
        };
        assert_eq!(line, position + 1, "the refusal names the line it refused");
    }

    let mut bytes = format!("{first}\n").into_bytes();
    bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
    assert!(matches!(
        TopologyFold::parse_log(&bytes),
        Err(FoldError::RewrittenLog { line: 2, .. })
    ));

    for (label, blank) in [
        ("empty", ""),
        ("spaces", "   "),
        ("tab", "\t"),
        ("unicode space", "\u{00a0}"),
    ] {
        for position in 0..3 {
            let mut lines = [first.clone(), second.clone(), second.clone()];
            lines[position] = blank.to_owned();
            let log = lines.join("\n") + "\n";
            let Err(error) = TopologyFold::parse_log(log.as_bytes()) else {
                panic!("a committed {label} line at {position} was skipped");
            };
            let FoldError::RewrittenLog { line, .. } = error else {
                panic!("a committed {label} line at {position} was not a rewritten log");
            };
            assert_eq!(
                line,
                position + 1,
                "the refusal names the {label} line it refused"
            );
        }
    }
}

fn push(live: &mut TopologyFold, trace: &mut Vec<TopologyEvent>, event: TopologyEvent) {
    apply(live, &event);
    trace.push(event);
}

fn long_trace() -> Vec<TopologyEvent> {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    push(&mut live, &mut trace, dispatch(ALPHA, 0, &base));
    let start = attempt_started(&live, ALPHA, 0, 1, 0);
    push(&mut live, &mut trace, start);
    push(
        &mut live,
        &mut trace,
        settle(
            ALPHA,
            0,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                retained_incarnation: Epoch(0),
            },
        ),
    );
    let resumed = ev(TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: ALPHA,
            generation: GenerationId(0),
            attempt: AttemptNumber(2),
            rung: 0,
            binding: frozen_binding(&live, ALPHA, 0),
            pool: None,
            resume_session: Some(SessionId("sess-ÜNI-0042".to_owned())),
            materialization_observed: None,
        },
    });
    push(&mut live, &mut trace, resumed);
    push(
        &mut live,
        &mut trace,
        candidate_prepared_at(ALPHA, 0, 2, &base),
    );
    push(&mut live, &mut trace, candidate_created(ALPHA, 0));
    push(
        &mut live,
        &mut trace,
        fast_publication(ALPHA, 0, 0, &base, vec![ALPHA]),
    );
    push(&mut live, &mut trace, merged(ALPHA, 0, 0, vec![ALPHA]));

    push(&mut live, &mut trace, dispatch(ZETA, 0, &base));
    let start = attempt_started(&live, ZETA, 0, 1, 0);
    push(&mut live, &mut trace, start);
    push(&mut live, &mut trace, candidate_prepared(ZETA, 0, &base));
    push(&mut live, &mut trace, candidate_created(ZETA, 0));
    push(
        &mut live,
        &mut trace,
        verification_started(ZETA, 0, 1, &head, &proposal),
    );
    push(
        &mut live,
        &mut trace,
        unavailable_event(1, outage(), UnavailableOutcome::Deferred { defers: 1 }),
    );
    push(
        &mut live,
        &mut trace,
        ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 30_000,
                round: 1,
            },
        }),
    );
    push(
        &mut live,
        &mut trace,
        verification_started(ZETA, 0, 2, &head, &proposal),
    );
    let mut repair = repair_spawn(TaskKey(3), ZETA, ZETA);
    repair.entry.deps = vec![ALPHA];
    repair.entry.display_deps = vec![TaskId::from("alpha")];
    push(
        &mut live,
        &mut trace,
        ev(TopologyEventBody::MergeRejected {
            data: Box::new(MergeRejected {
                sequence: SequenceId(2),
                candidate: candidate_of(ZETA, 0),
                rejecting_head: head.clone(),
                disposition: RejectionDisposition::CodeRejected {
                    verification: verification_record(Verdict::Rejected),
                },
                repair,
                lease_effect: RejectionLeaseEffect::CreatesLineage {
                    root: ZETA,
                    paths: region(MID),
                },
            }),
        }),
    );
    push(&mut live, &mut trace, budget_exceeded(0, Some(MID)));
    push(&mut live, &mut trace, resume(container_runner()));

    assert!(
        trace.len() >= 20,
        "the trace has to exercise more than a path"
    );
    trace
}

fn settled_trace() -> Vec<TopologyEvent> {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    push(&mut live, &mut trace, dispatch(ZETA, 0, &base));
    let start = attempt_started(&live, ZETA, 0, 1, 0);
    push(&mut live, &mut trace, start);
    push(
        &mut live,
        &mut trace,
        ev(TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key: ZETA,
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
                lease: LeaseDisposition::PredictedReleased,
                detail: "  the coordinator died  ".to_owned(),
            },
        }),
    );
    push(&mut live, &mut trace, dispatch(ZETA, 1, &base));
    push(
        &mut live,
        &mut trace,
        ev(TopologyEventBody::GenerationClosed {
            data: GenerationClosed {
                key: ZETA,
                generation: GenerationId(1),
                reason: GenerationCloseReason::WorktreeMissing,
                lease: LeaseDisposition::PredictedReleased,
            },
        }),
    );

    push(&mut live, &mut trace, dispatch(ALPHA, 0, &base));
    let start = attempt_started(&live, ALPHA, 0, 1, 0);
    push(&mut live, &mut trace, start);
    push(&mut live, &mut trace, candidate_prepared(ALPHA, 0, &base));
    push(&mut live, &mut trace, candidate_created(ALPHA, 0));
    push(
        &mut live,
        &mut trace,
        fast_publication(ALPHA, 0, 0, &base, vec![ALPHA]),
    );
    push(&mut live, &mut trace, merged(ALPHA, 0, 0, vec![ALPHA]));
    push(
        &mut live,
        &mut trace,
        spawn_event(repair_spawn(TaskKey(3), ALPHA, ALPHA)),
    );

    push(&mut live, &mut trace, dispatch(ZETA, 2, &base));
    let start = attempt_started(&live, ZETA, 2, 1, 0);
    push(&mut live, &mut trace, start);
    push(&mut live, &mut trace, candidate_prepared(ZETA, 2, &base));
    push(&mut live, &mut trace, candidate_created(ZETA, 2));
    push(
        &mut live,
        &mut trace,
        verification_started(ZETA, 2, 1, &head, &proposal),
    );
    push(
        &mut live,
        &mut trace,
        ev(TopologyEventBody::MergeVerificationInterrupted {
            data: MergeVerificationInterrupted {
                sequence: SequenceId(1),
                detail: "  the coordinator died  ".to_owned(),
            },
        }),
    );

    push(&mut live, &mut trace, raised("q-park-Ünicode", MID));
    push(
        &mut live,
        &mut trace,
        answered(
            MID,
            "q-park-Ünicode",
            Answer4::Answered {
                option_index: 2,
                binding_override: None,
            },
        ),
    );
    trace
}

fn finished_trace() -> Vec<TopologyEvent> {
    let mut live = started();
    let mut trace = vec![run_started_event()];
    let base = sha("base");
    for (key, sequence) in [(ALPHA, 0), (ZETA, 1), (MID, 2)] {
        push(&mut live, &mut trace, dispatch(key, 0, &base));
        let start = attempt_started(&live, key, 0, 1, 0);
        push(&mut live, &mut trace, start);
        push(&mut live, &mut trace, candidate_prepared(key, 0, &base));
        push(&mut live, &mut trace, candidate_created(key, 0));
        push(
            &mut live,
            &mut trace,
            fast_publication(key, 0, sequence, &base, vec![key]),
        );
        push(&mut live, &mut trace, merged(key, 0, sequence, vec![key]));
    }
    push(
        &mut live,
        &mut trace,
        run_finished(RunOutcome::Complete, None),
    );
    trace
}

#[test]
fn live_and_replay_reach_the_same_state_over_a_long_trace() {
    for trace in [long_trace(), settled_trace(), finished_trace()] {
        let mut live = TopologyFold::new(inputs());
        for event in &trace {
            apply(&mut live, event);
        }
        let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");
        assert_eq!(parsed, trace, "every event survives the round trip");
        let replayed = TopologyFold::replay(inputs(), &parsed).expect("the log replays");

        assert_eq!(
            live.state(),
            replayed.state(),
            "a live fold and a replay of what it appended have to be one state"
        );
        assert_eq!(live.derived_outcome(), replayed.derived_outcome());
    }
}

fn wire(trace: &[TopologyEvent]) -> Vec<u8> {
    let mut log = Vec::new();
    for event in trace {
        log.extend_from_slice(serde_json::to_string(event).expect("serialize").as_bytes());
        log.push(b'\n');
    }
    log
}

#[allow(clippy::too_many_lines)]
fn one_field_invalid(event: &TopologyEvent) -> Vec<(String, TopologyEvent)> {
    let mut out = Vec::new();
    let mut case = |label: &str, body: TopologyEventBody| {
        out.push((format!("{}/{label}", event.body.kind()), ev(body)));
    };
    match &event.body {
        TopologyEventBody::RunStarted { data } => {
            let mut moved = data.clone();
            moved.registry_digest = format!(
                "{}0",
                &data.registry_digest[..data.registry_digest.len() - 1]
            );
            case(
                "registry_digest",
                TopologyEventBody::RunStarted { data: moved },
            );
            let mut moved = data.clone();
            moved.normalized_plan_digest =
                "sha256:8888888888888888888888888888888888888888888888888888888888888888"
                    .to_owned();
            case(
                "normalized_plan_digest",
                TopologyEventBody::RunStarted { data: moved },
            );
        }
        TopologyEventBody::RunResumed { data } => {
            let mut moved = data.clone();
            if let Some(image) = moved.runner.image.as_mut() {
                image.reference = "ghcr.io/example/Another-Runner:2.1".to_owned();
            }
            case(
                "runner.image.reference",
                TopologyEventBody::RunResumed { data: moved },
            );
            let mut moved = data.clone();
            if let Some(volumes) = moved.runner.credential_volumes.as_mut() {
                volumes.insert("claude-code".to_owned(), "upstroke-creds-codex".to_owned());
            }
            case(
                "runner.credential_volumes",
                TopologyEventBody::RunResumed { data: moved },
            );
        }
        TopologyEventBody::TaskSpawned { data } => {
            let mut moved = data.clone();
            moved.spawn.key = TaskKey(moved.spawn.key.0 + 1);
            moved.spawn.entry.key = moved.spawn.key;
            case("key", TopologyEventBody::TaskSpawned { data: moved });
            let mut moved = data.clone();
            moved
                .spawn
                .entry
                .allowed_agents
                .push("an-unprobed-agent".to_owned());
            case(
                "allowed_agents",
                TopologyEventBody::TaskSpawned { data: moved },
            );
        }
        TopologyEventBody::TaskDispatched { data } => {
            let mut moved = data.clone();
            moved.generation = GenerationId(moved.generation.0 + 1);
            case(
                "generation",
                TopologyEventBody::TaskDispatched { data: moved },
            );
            let mut moved = data.clone();
            if let LeaseGrant::Predicted { paths } = &mut moved.lease {
                *paths = PathSet::Prefixes {
                    paths: vec![GitPath::from("src/somewhere-nobody-predicted")],
                };
                case(
                    "lease.paths",
                    TopologyEventBody::TaskDispatched { data: moved },
                );
            }
        }
        TopologyEventBody::AttemptStarted { data } => {
            let mut moved = data.clone();
            moved.attempt = AttemptNumber(moved.attempt.0 + 1);
            case("attempt", TopologyEventBody::AttemptStarted { data: moved });
            let mut moved = data.clone();
            moved.binding.agent = "an-agent-nobody-froze".to_owned();
            case(
                "binding.agent",
                TopologyEventBody::AttemptStarted { data: moved },
            );
        }
        TopologyEventBody::AttemptFinished { data } => {
            let mut moved = data.clone();
            moved.attempt = AttemptNumber(moved.attempt.0 + 1);
            case(
                "attempt",
                TopologyEventBody::AttemptFinished { data: moved },
            );
            let mut moved = data.clone();
            if let AttemptSettlement::Closed { lease, .. } = &mut moved.settlement {
                *lease = LeaseDisposition::LineageHeld;
                case(
                    "settlement.lease",
                    TopologyEventBody::AttemptFinished { data: moved },
                );
            }
            let mut moved = data.clone();
            if matches!(moved.settlement, AttemptSettlement::Retained { .. }) {
                moved.record.attempt = moved.record.attempt.saturating_add(1);
                case(
                    "record.attempt",
                    TopologyEventBody::AttemptFinished { data: moved },
                );
            }
            let mut moved = data.clone();
            if matches!(moved.settlement, AttemptSettlement::Retained { .. }) {
                moved.record.session_id = Some("sess-somebody-elses".to_owned());
                case(
                    "record.session_id",
                    TopologyEventBody::AttemptFinished { data: moved },
                );
            }
            let mut moved = data.clone();
            if matches!(moved.settlement, AttemptSettlement::Retained { .. }) {
                moved.record.failure = None;
                moved.record.reviews = vec![review_pass("review", ReviewPassOutcome::Passed)];
                case(
                    "record.claims-success",
                    TopologyEventBody::AttemptFinished { data: moved },
                );
            }
        }
        TopologyEventBody::AttemptInterrupted { data } => {
            let mut moved = data.clone();
            moved.generation = GenerationId(moved.generation.0 + 1);
            case(
                "generation",
                TopologyEventBody::AttemptInterrupted { data: moved },
            );
            let mut moved = data.clone();
            moved.lease = LeaseDisposition::PredictedRetained;
            case(
                "lease",
                TopologyEventBody::AttemptInterrupted { data: moved },
            );
        }
        TopologyEventBody::GenerationClosed { data } => {
            let mut moved = data.clone();
            moved.generation = GenerationId(moved.generation.0 + 1);
            case(
                "generation",
                TopologyEventBody::GenerationClosed { data: moved },
            );
        }
        TopologyEventBody::DeferWaitElapsed { .. } => {}
        TopologyEventBody::CandidatePrepared { data } => {
            let mut moved = data.clone();
            moved.attempt = Box::new(attempt_record(moved.attempt.attempt + 1));
            case(
                "attempt",
                TopologyEventBody::CandidatePrepared { data: moved },
            );
            let mut moved = data.clone();
            moved.parent_sha = sha("somewhere-else");
            case(
                "parent_sha",
                TopologyEventBody::CandidatePrepared { data: moved },
            );
            let mut moved = data.clone();
            moved.attempt.reviews.clear();
            case(
                "attempt.reviews",
                TopologyEventBody::CandidatePrepared { data: moved },
            );
        }
        TopologyEventBody::TaskCandidateCreated { data } => {
            let mut moved = data.clone();
            moved.candidate.commit_sha = sha("a-commit-nobody-prepared");
            case(
                "candidate.commit_sha",
                TopologyEventBody::TaskCandidateCreated { data: moved },
            );
        }
        TopologyEventBody::MergeVerificationStarted { data } => {
            let mut moved = data.clone();
            moved.sequence = SequenceId(moved.sequence.0 + 1);
            case(
                "sequence",
                TopologyEventBody::MergeVerificationStarted { data: moved },
            );
            let mut moved = data.clone();
            moved.candidate.commit_sha = sha("a-commit-nobody-prepared");
            case(
                "candidate.commit_sha",
                TopologyEventBody::MergeVerificationStarted { data: moved },
            );
        }
        TopologyEventBody::MergeVerificationUnavailable { data } => {
            let mut moved = data.clone();
            if let UnavailableOutcome::Deferred { defers } = &mut moved.outcome {
                *defers += 1;
                case(
                    "outcome.defers",
                    TopologyEventBody::MergeVerificationUnavailable { data: moved },
                );
            }
            let mut moved = data.clone();
            moved.sequence = SequenceId(moved.sequence.0 + 1);
            case(
                "sequence",
                TopologyEventBody::MergeVerificationUnavailable { data: moved },
            );
        }
        TopologyEventBody::MergeVerificationInterrupted { data } => {
            let mut moved = data.clone();
            moved.sequence = SequenceId(moved.sequence.0 + 1);
            case(
                "sequence",
                TopologyEventBody::MergeVerificationInterrupted { data: moved },
            );
        }
        TopologyEventBody::MergePrepared { data } => {
            let mut moved = data.clone();
            moved.expected_head = sha("a-head-nobody-read");
            case(
                "expected_head",
                TopologyEventBody::MergePrepared { data: moved },
            );
            let mut moved = data.clone();
            moved.candidate_ref = git_ref("candidates/decoy");
            case(
                "candidate_ref",
                TopologyEventBody::MergePrepared { data: moved },
            );
        }
        TopologyEventBody::MergeRejected { data } => {
            let mut moved = data.clone();
            moved.sequence = SequenceId(moved.sequence.0 + 1);
            case("sequence", TopologyEventBody::MergeRejected { data: moved });
            let mut moved = data.clone();
            moved.candidate.commit_sha = sha("a-commit-nobody-prepared");
            case(
                "candidate.commit_sha",
                TopologyEventBody::MergeRejected { data: moved },
            );
        }
        TopologyEventBody::TaskMerged { data } => {
            let mut moved = data.clone();
            moved.merged_sha = sha("a-sha-nobody-authorized");
            case("merged_sha", TopologyEventBody::TaskMerged { data: moved });
            let mut moved = data.clone();
            moved.sequence = SequenceId(moved.sequence.0 + 1);
            case("sequence", TopologyEventBody::TaskMerged { data: moved });
        }
        TopologyEventBody::QuestionRaised { data } => {
            let mut moved = data.clone();
            moved.question.key = TaskKey(9);
            case(
                "question.key",
                TopologyEventBody::QuestionRaised { data: moved },
            );
            let mut moved = data.clone();
            moved.question.options.clear();
            case(
                "question.options",
                TopologyEventBody::QuestionRaised { data: moved },
            );
        }
        TopologyEventBody::QuestionAnswered { data } => {
            let mut moved = data.clone();
            moved.question = QuestionId::from("q-this-log-never-asked");
            if let Answer4::Answered {
                binding_override, ..
            } = &mut moved.answer
            {
                if let Some(binding) = binding_override.as_mut() {
                    binding.question = QuestionId::from("q-this-log-never-asked");
                }
            }
            case(
                "question",
                TopologyEventBody::QuestionAnswered { data: moved },
            );
        }
        TopologyEventBody::BudgetExceeded { data } => {
            let mut moved = data.clone();
            moved.epoch = Epoch(moved.epoch.0 + 1);
            case("epoch", TopologyEventBody::BudgetExceeded { data: moved });
        }
        TopologyEventBody::RunFinished { data } => {
            let mut moved = data.clone();
            moved.outcome = match moved.outcome {
                RunOutcome::Complete => RunOutcome::Halted,
                _ => RunOutcome::Complete,
            };
            case("outcome", TopologyEventBody::RunFinished { data: moved });
        }
        TopologyEventBody::CapacitySnapshot { .. }
        | TopologyEventBody::PoolExhausted { .. }
        | TopologyEventBody::DesignDefect { .. } => {}
    }
    out
}

#[test]
fn every_guarded_event_is_refused_the_same_way_live_and_on_a_hostile_replay() {
    let mut covered: BTreeSet<&'static str> = BTreeSet::new();
    let mut cases = 0_u32;
    for trace in [long_trace(), settled_trace(), finished_trace()] {
        for index in 0..trace.len() {
            let kind = trace[index].body.kind();
            let variants = one_field_invalid(&trace[index]);
            if variants.is_empty() {
                continue;
            }
            let prefix = TopologyFold::replay(inputs(), &trace[..index])
                .unwrap_or_else(|error| panic!("the prefix before {kind} replays: {error}"));
            let before = prefix.state().cloned();
            for (label, invalid) in variants {
                let live_error = prefix
                    .plan_transition(&invalid)
                    .err()
                    .unwrap_or_else(|| panic!("{label} is not an invalid transition"));
                assert_eq!(
                    prefix.state().cloned(),
                    before,
                    "{label} mutated on refusal"
                );

                let mut hostile = trace[..index].to_vec();
                hostile.push(invalid);
                hostile.extend_from_slice(&trace[index + 1..]);
                assert!(
                    hostile.len() == trace.len() && index < trace.len(),
                    "{label}: the hostile log is the trace with one line replaced"
                );
                let parsed =
                    TopologyFold::parse_log(&wire(&hostile)).expect("the hostile log parses");
                let replay_error = TopologyFold::replay(inputs(), &parsed)
                    .err()
                    .unwrap_or_else(|| {
                        panic!("{label}: a hostile log replayed to a state instead of refusing")
                    });
                assert_eq!(
                    replay_error, live_error,
                    "{label}: replay and live disagree about the same event over the same \
                         prefix"
                );
            }
            covered.insert(kind);
            cases += 1;
        }
    }

    let unguarded: BTreeSet<&'static str> = [
        "defer_wait_elapsed",
        "capacity_snapshot",
        "pool_exhausted",
        "design_defect",
    ]
    .into_iter()
    .collect();
    let expected: BTreeSet<&'static str> = TOPOLOGY_EVENT_KINDS
        .iter()
        .copied()
        .filter(|kind| !unguarded.contains(kind))
        .collect();
    assert_eq!(
        covered, expected,
        "a guarded kind was never swept for a hostile replay"
    );
    assert!(cases > 20, "the sweep was not vacuous: {cases}");

    let mut live = started();
    let mut trace = vec![run_started_event()];
    push(&mut live, &mut trace, budget_exceeded(0, Some(ZETA)));
    let elapsed = ev(TopologyEventBody::DeferWaitElapsed {
        data: DeferWaitElapsed4 {
            waited_ms: 30_000,
            round: 1,
        },
    });
    let live_error = refuse(&live, &elapsed);
    let mut hostile = trace.clone();
    hostile.push(elapsed);
    hostile.push(resume(container_runner()));
    let parsed = TopologyFold::parse_log(&wire(&hostile)).expect("the hostile log parses");
    assert_eq!(
        TopologyFold::replay(inputs(), &parsed)
            .expect_err("a wait that elapsed under a budget stop is refused on replay"),
        live_error
    );
}

#[test]
fn a_delta_carries_the_exact_event_it_was_checked_against() {
    let base = sha("base");
    let mut fold = TopologyFold::new(inputs());
    for event in [
        run_started_event(),
        dispatch(ZETA, 0, &base),
        raised("q-park-Ünicode", ALPHA),
        ev(TopologyEventBody::CapacitySnapshot {
            data: CapacitySnapshot {
                strategy: "  Least-Loaded  ".to_owned(),
                pools: Vec::new(),
            },
        }),
    ] {
        let delta = fold
            .plan_transition(&event)
            .unwrap_or_else(|error| panic!("`{}` must apply: {error}", event.body.kind()));
        assert_eq!(
            delta.event(),
            &event,
            "`{}` was checked against a copy of itself",
            event.body.kind()
        );
        assert_eq!(
            serde_json::to_string(delta.event()).expect("serialize"),
            serde_json::to_string(&event).expect("serialize"),
            "`{}` would be appended as different bytes from the ones checked",
            event.body.kind()
        );
        fold.apply_delta(delta);
    }
}

#[test]
fn a_refused_transition_changes_nothing() {
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    let before = fold.state().cloned();
    for event in every_kind() {
        let _ = fold.plan_transition(&event);
    }
    assert_eq!(
        fold.state().cloned(),
        before,
        "asking whether an event applies must not apply it"
    );
}

#[test]
fn the_registry_digest_does_not_widen_when_a_repair_is_registered() {
    let mut fold = started();
    merge_task(&mut fold, ALPHA, 0, 0);
    let before = fold.registry().expect("started").digest();
    let before_bytes = fold.registry().expect("started").canonical_bytes();
    assert_eq!(before, run_started().registry_digest);

    let spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);
    let registered = spawn.entry.clone();
    apply(&mut fold, &spawn_event(spawn));
    let registry = fold.registry().expect("started");
    assert_eq!(registry.len(), 4, "the repair joined the registry");
    assert_eq!(registry.originals_len(), 3);
    assert_eq!(
        registry.digest(),
        before,
        "registering a repair moved the value that authenticates the frozen plan"
    );

    let bytes = registry.canonical_bytes();
    assert_ne!(
        bytes, before_bytes,
        "a registered repair left the canonical serialization unchanged"
    );
    assert!(
        bytes.len() > before_bytes.len(),
        "the encoding did not grow by an entry"
    );
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(text.contains(registered.display_id.as_str()));
    assert!(text.contains("Repair the alpha rejection"));
    for agent in &registered.allowed_agents {
        assert!(
            text.contains(agent.as_str()),
            "the stored allow-list entry `{agent}` is not in the canonical encoding"
        );
    }

    assert_eq!(
        registry.get(TaskKey(3)),
        Some(&registered),
        "the registry stored something other than what the event carried"
    );
    assert_eq!(
        registry.get(TaskKey(3)).expect("the repair").allowed_agents,
        run_started().probed_agents,
        "the stored allow-list is the run's probe list"
    );
    let rung_agents: Vec<String> = registered
        .ladder
        .rungs
        .iter()
        .map(|rung| rung.agent.clone())
        .collect();
    assert_ne!(
        registry.get(TaskKey(3)).expect("the repair").allowed_agents,
        rung_agents,
        "the fixture's rung agents must differ from the probe list, or this proves nothing"
    );
    assert_eq!(
        registry.key_of(
            registry
                .get(TaskKey(3))
                .expect("the repair")
                .display_id
                .as_str()
        ),
        Some(TaskKey(3))
    );
}

fn prefixes(paths: &[&str]) -> PathSet {
    PathSet::Prefixes {
        paths: paths.iter().copied().map(GitPath::from).collect(),
    }
}

#[test]
fn regions_overlap_component_wise_and_repo_wide_overlaps_everything() {
    let sensitive = PathPolicy {
        case_fold: false,
        ..path_policy()
    };
    let folding = path_policy();

    for (left, right, overlaps) in [
        ("src/foo", "src/foo", true),
        ("src/foo", "src/foo/bar.rs", true),
        ("src/foo/bar.rs", "src/foo", true),
        ("src/foo", "src/foobar", false),
        ("src/foobar", "src/foo", false),
        ("src/foo", "src/bar", false),
        ("src", "docs", false),
        ("src/foo/", "src/foo", true),
        ("", "src/foo", true),
    ] {
        assert_eq!(
            regions_overlap(&prefixes(&[left]), &prefixes(&[right]), &sensitive),
            overlaps,
            "`{left}` against `{right}`"
        );
    }

    for (left, right) in [("src/Zebra", "src/zebra"), ("src/ÜBER", "src/über")] {
        assert!(
            !regions_overlap(&prefixes(&[left]), &prefixes(&[right]), &sensitive),
            "`{left}` and `{right}` are two files where case is significant"
        );
        assert!(
            regions_overlap(&prefixes(&[left]), &prefixes(&[right]), &folding),
            "`{left}` and `{right}` are one file where it is not"
        );
    }

    for other in [PathSet::RepoWide, prefixes(&[]), prefixes(&["src/foo"])] {
        assert!(regions_overlap(&PathSet::RepoWide, &other, &folding));
        assert!(regions_overlap(&other, &PathSet::RepoWide, &folding));
    }
    assert!(!regions_overlap(
        &prefixes(&[]),
        &prefixes(&["src/foo"]),
        &folding
    ));

    assert!(regions_overlap(
        &prefixes(&["docs", "src/foo"]),
        &prefixes(&["build.rs", "src/foo/bar.rs"]),
        &folding
    ));
}

#[test]
fn an_ordinary_candidate_waits_for_any_lineage_and_a_member_only_for_older_ones() {
    let policy = path_policy();
    let mut leases = LeaseTable::new();
    leases.grant(
        LeaseOwner::Lineage { root: ZETA },
        prefixes(&["src/shared"]),
    );
    leases.grant(LeaseOwner::Lineage { root: MID }, prefixes(&["src/shared"]));
    assert_eq!(leases.lineage(ZETA).expect("older").age, 0);
    assert_eq!(leases.lineage(MID).expect("younger").age, 1);

    let entry = |lineage_root: Option<TaskKey>| QueueEntry {
        candidate: candidate_of(ALPHA, 0),
        paths: prefixes(&["src/shared/thing.rs"]),
        lineage_root,
        verification_deferred: false,
        defers: 0,
        sequence: None,
    };
    let never_parked = |_: TaskKey| false;

    assert_eq!(
        CandidateQueue::ineligible(&entry(None), &never_parked, &leases, &policy),
        Some(Ineligible::InsideLineage { root: ZETA }),
        "an ordinary candidate waits for the oldest lineage it overlaps"
    );
    assert_eq!(
        CandidateQueue::ineligible(&entry(Some(ZETA)), &never_parked, &leases, &policy),
        None,
        "the oldest lineage's own member is not held back by the lineage it belongs to"
    );
    assert_eq!(
        CandidateQueue::ineligible(&entry(Some(MID)), &never_parked, &leases, &policy),
        Some(Ineligible::BehindOlderLineage { root: ZETA }),
        "a younger lineage's member waits for the older one"
    );

    assert_eq!(
        CandidateQueue::ineligible(&entry(Some(ZETA)), &|key| key == ALPHA, &leases, &policy),
        Some(Ineligible::AwaitingInput)
    );
    let deferred = QueueEntry {
        verification_deferred: true,
        ..entry(Some(ZETA))
    };
    assert_eq!(
        CandidateQueue::ineligible(&deferred, &never_parked, &leases, &policy),
        Some(Ineligible::VerificationDeferred)
    );

    let elsewhere = QueueEntry {
        paths: prefixes(&["docs/guide.md"]),
        ..entry(None)
    };
    assert_eq!(
        CandidateQueue::ineligible(&elsewhere, &never_parked, &leases, &policy),
        None
    );
}

#[test]
fn a_lineage_lease_only_ever_grows_and_a_released_one_is_gone() {
    let policy = path_policy();
    let mut leases = LeaseTable::new();
    leases.widen_lineage(ZETA, &prefixes(&["src/a"]));
    leases.widen_lineage(ZETA, &prefixes(&["src/b", "src/a"]));
    let held = leases.lineage(ZETA).expect("the lineage");
    let mut paths: Vec<&str> = held
        .paths
        .prefixes()
        .expect("bounded")
        .iter()
        .map(GitPath::as_str)
        .collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec!["src/a", "src/b"],
        "widening is a union, not an append"
    );

    leases.widen_lineage(ZETA, &PathSet::RepoWide);
    assert!(
        leases
            .lineage(ZETA)
            .expect("the lineage")
            .paths
            .is_repo_wide()
    );
    leases.widen_lineage(ZETA, &prefixes(&["src/a"]));
    assert!(
        leases
            .lineage(ZETA)
            .expect("the lineage")
            .paths
            .is_repo_wide()
    );

    let mut table = LeaseTable::new();
    let owner = LeaseOwner::Generation {
        key: ZETA,
        generation: GenerationId(0),
    };
    table.grant(owner, prefixes(&["src/foo"]));
    assert!(!table.overlaps_another(owner, &prefixes(&["src/foo/bar.rs"]), &policy));
    assert!(table.overlaps_another(
        LeaseOwner::Generation {
            key: ALPHA,
            generation: GenerationId(0)
        },
        &prefixes(&["src/foo/bar.rs"]),
        &policy
    ));
    assert!(!table.any_candidate_or_lineage());
    table.grant(
        LeaseOwner::Candidate {
            key: ZETA,
            generation: GenerationId(0),
        },
        prefixes(&["src/foo"]),
    );
    assert!(table.any_candidate_or_lineage());
    table.release(LeaseOwner::Candidate {
        key: ZETA,
        generation: GenerationId(0),
    });
    assert!(!table.any_candidate_or_lineage());
    table.release(LeaseOwner::Lineage { root: MID });
    assert!(!table.holds(LeaseOwner::Lineage { root: MID }));
}

#[test]
fn a_generations_holding_decides_the_disposition_its_settlements_record() {
    for (lease, survives, expected) in [
        (
            GenerationLease::Own,
            true,
            LeaseDisposition::PredictedRetained,
        ),
        (
            GenerationLease::Own,
            false,
            LeaseDisposition::PredictedReleased,
        ),
        (
            GenerationLease::InheritedLineage { root: ZETA },
            true,
            LeaseDisposition::LineageHeld,
        ),
        (
            GenerationLease::InheritedLineage { root: ZETA },
            false,
            LeaseDisposition::LineageHeld,
        ),
    ] {
        assert_eq!(lease.expected(survives), expected, "{lease:?} / {survives}");
    }
}

#[test]
fn a_predicted_region_is_the_literal_prefix_of_every_hint() {
    let registry = started().registry().expect("started").clone();
    let zeta = registry.get(ZETA).expect("zeta");
    assert_eq!(
        predicted_region(zeta),
        prefixes(&["src/Zebra"]),
        "a trailing separator is not part of the prefix"
    );
    let alpha = registry.get(ALPHA).expect("alpha");
    assert_eq!(
        predicted_region(alpha),
        prefixes(&["src/alpha"]),
        "the literal prefix stops at the first metacharacter"
    );
    let mid = registry.get(MID).expect("mid");
    assert_eq!(predicted_region(mid), prefixes(&["src/mid", "build.rs"]));

    let mut hintless = zeta.clone();
    hintless.spec.path_hints.clear();
    assert!(predicted_region(&hintless).is_repo_wide());
    for unsafe_hint in ["*.rs", "**/mod.rs", "/", "{a,b}/c"] {
        let mut entry = zeta.clone();
        entry.spec.path_hints = vec![unsafe_hint.to_owned()];
        assert!(
            predicted_region(&entry).is_repo_wide(),
            "`{unsafe_hint}` bounds nothing and must classify repo-wide"
        );
    }
    let mut windows = zeta.clone();
    windows.spec.path_hints = vec!["src\\Zebra\\mod.rs".to_owned()];
    assert_eq!(predicted_region(&windows), prefixes(&["src/Zebra/mod.rs"]));
}

#[test]
fn the_pipeline_entitlement_is_what_the_fold_derives_it_to_be() {
    let base = sha("base");
    let mut fold = started();
    let run = |fold: &TopologyFold| fold.state().expect("started").pipeline_held();
    assert_eq!(run(&fold), 0);

    apply(&mut fold, &dispatch(ZETA, 0, &base));
    assert_eq!(run(&fold), 1, "open with no attempt holds one");
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    assert_eq!(run(&fold), 1, "in flight holds the same one");

    let mut retained = fold.clone();
    apply(
        &mut retained,
        &settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                retained_incarnation: Epoch(0),
            },
        ),
    );
    assert_eq!(run(&retained), 0, "a retained generation holds none");

    assert_eq!(run(&fold), 1, "promoting still holds it");
    apply(&mut fold, &candidate_prepared(ZETA, 0, &base));
    assert_eq!(run(&fold), 1);
    apply(&mut fold, &candidate_created(ZETA, 0));
    assert_eq!(
        run(&fold),
        0,
        "promotion releases it and a queued candidate holds none"
    );

    apply(&mut fold, &fast_publication(ZETA, 0, 0, &base, vec![ZETA]));
    assert_eq!(run(&fold), 1, "an unresolved transaction holds one");
    apply(&mut fold, &merged(ZETA, 0, 0, vec![ZETA]));
    assert_eq!(run(&fold), 0, "and the terminal releases it");
}

#[test]
fn a_run_reaches_complete_only_when_every_task_has_settled() {
    let mut fold = started();
    assert_eq!(fold.derived_outcome(), DerivedOutcome::NotEnding);
    merge_task(&mut fold, ALPHA, 0, 0);
    assert_eq!(fold.derived_outcome(), DerivedOutcome::NotEnding);
    merge_task(&mut fold, ZETA, 0, 1);
    assert_eq!(fold.derived_outcome(), DerivedOutcome::NotEnding);
    merge_task(&mut fold, MID, 0, 2);
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete)
    );
    assert!(fold.queue().expect("started").is_empty());
    assert!(!fold.leases().expect("started").any_candidate_or_lineage());
    apply(&mut fold, &run_finished(RunOutcome::Complete, None));
    assert_eq!(fold.finished(), Some(&RunOutcome::Complete));
}

#[test]
fn halt_and_budget_outrank_every_structural_source_that_can_coexist_with_them() {
    let base = sha("base");

    let ready_state = || started();
    assert_eq!(ready_state().derived_outcome(), DerivedOutcome::NotEnding);

    let integration_state = || {
        let mut fold = two_queued();
        let mut run = fold.run.take().expect("started");
        run.tasks[ALPHA.index()].state = TaskState::Failed;
        fold.run = Some(run);
        fold
    };
    let staged = integration_state();
    assert_eq!(staged.derived_outcome(), DerivedOutcome::NotEnding);
    assert!(
        !staged.queue().expect("started").is_empty(),
        "the integration source has to be a queued candidate"
    );

    for (label, build) in [
        (
            "a dispatchable task",
            &ready_state as &dyn Fn() -> TopologyFold,
        ),
        ("an eligible integration", &integration_state),
    ] {
        let mut halted = build();
        let mut run = halted.run.take().expect("started");
        let epoch = run.epoch;
        run.halted_at = Some(ALPHA);
        run.halted_epoch = Some(epoch);
        halted.run = Some(run);
        assert_eq!(
            halted.derived_outcome(),
            DerivedOutcome::Ending(RunOutcome::Halted),
            "{label} outranked a halting settlement"
        );

        let mut stopped = build();
        let mut run = stopped.run.take().expect("started");
        let epoch = run.epoch;
        run.budget_stop = Some(BudgetStop {
            epoch,
            budget: BudgetKind::Run,
        });
        stopped.run = Some(run);
        assert_eq!(
            stopped.derived_outcome(),
            DerivedOutcome::Ending(RunOutcome::BudgetExceeded),
            "{label} outranked the epoch's budget stop"
        );
    }

    let mut retained = started();
    apply(&mut retained, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&retained, ZETA, 0, 1, 0);
    apply(&mut retained, &start);
    apply(
        &mut retained,
        &settle(
            ZETA,
            0,
            1,
            AttemptSettlement::Retained {
                retained_session: SessionId("sess-ÜNI-0042".to_owned()),
                retained_incarnation: Epoch(0),
            },
        ),
    );
    assert_eq!(retained.task_state(ZETA), Some(TaskState::Pending));
    assert!(matches!(
        retained
            .task(ZETA)
            .expect("zeta")
            .open()
            .map(|generation| &generation.class),
        Some(GenerationClass::RetainedIdle { .. })
    ));
    let mut run = retained.run.take().expect("started");
    let epoch = run.epoch;
    run.halted_at = Some(ALPHA);
    run.halted_epoch = Some(epoch);
    retained.run = Some(run);
    assert_eq!(
        retained.derived_outcome(),
        DerivedOutcome::NotEnding,
        "an open generation is not common, and not-common outranks the halt"
    );
}

#[test]
fn complete_refuses_each_residue_it_leaves_behind_one_at_a_time() {
    let terminal = || {
        let mut fold = started();
        let mut run = fold.run.take().expect("started");
        for task in &mut run.tasks {
            task.state = TaskState::Merged;
        }
        fold.run = Some(run);
        fold
    };
    assert_eq!(
        terminal().derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete),
        "the fixture has to be Complete before a residue is added, or nothing is isolated"
    );

    let residues: [(&str, AddResidue); 3] = [
        ("a queue position", |run| {
            run.queue.push(QueueEntry {
                candidate: candidate_of(MID, 0),
                paths: region(MID),
                lineage_root: None,
                verification_deferred: false,
                defers: 0,
                sequence: None,
            });
        }),
        ("a candidate lease", |run| {
            run.leases.grant(
                LeaseOwner::Candidate {
                    key: MID,
                    generation: GenerationId(0),
                },
                region(MID),
            );
        }),
        ("a lineage lease", |run| {
            run.leases
                .grant(LeaseOwner::Lineage { root: ALPHA }, region(ALPHA));
        }),
    ];
    for (label, add) in residues {
        let mut fold = terminal();
        let mut run = fold.run.take().expect("started");
        add(&mut run);
        fold.run = Some(run);
        assert_ne!(
            fold.derived_outcome(),
            DerivedOutcome::Ending(RunOutcome::Complete),
            "a run that still holds {label} was Complete"
        );
        assert!(
            matches!(
                refuse(&fold, &run_finished(RunOutcome::Complete, None)),
                FoldError::OutcomeMismatch { .. }
            ),
            "a run that still holds {label} said it was Complete"
        );
    }

    let mut generation_only = terminal();
    let mut run = generation_only.run.take().expect("started");
    run.leases.grant(
        LeaseOwner::Generation {
            key: MID,
            generation: GenerationId(0),
        },
        region(MID),
    );
    generation_only.run = Some(run);
    assert_eq!(
        generation_only.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete)
    );
}

#[test]
fn backoff_is_what_is_waiting_now_and_not_what_once_waited() {
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = two_queued();
    apply(
        &mut fold,
        &verification_started(MID, 0, 0, &head, &proposal),
    );
    apply(
        &mut fold,
        &unavailable_event(0, outage(), UnavailableOutcome::Deferred { defers: 1 }),
    );
    assert_eq!(fold.derived_outcome(), DerivedOutcome::NotEnding);
    apply(
        &mut fold,
        &ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 30_000,
                round: 1,
            },
        }),
    );

    let entry = &fold.queue().expect("started").entries()[0];
    assert!(!entry.verification_deferred, "the wake cleared the flag");
    assert_eq!(entry.defers, 1, "and kept the count it is measured against");

    let woken = fold.clone();
    let mut run = fold.run.take().expect("started");
    run.queue = CandidateQueue::new();
    run.leases = LeaseTable::new();
    for task in &mut run.tasks {
        task.state = TaskState::Merged;
    }
    let carried = QueueEntry {
        candidate: candidate_of(MID, 0),
        paths: region(MID),
        lineage_root: None,
        verification_deferred: false,
        defers: 1,
        sequence: None,
    };
    fold.run = Some(run);
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete)
    );
    let mut with_entry = fold.clone();
    let mut run = with_entry.run.take().expect("started");
    run.queue.push(carried);
    with_entry.run = Some(run);
    assert_eq!(with_entry.derived_outcome(), DerivedOutcome::NotEnding);
    assert!(
        !with_entry.queue().expect("started").entries()[0].verification_deferred,
        "the entry that blocks Complete is queued, not backing off"
    );

    let mut parked = woken;
    apply(
        &mut parked,
        &verification_started(MID, 0, 1, &head, &proposal),
    );
    apply(
        &mut parked,
        &unavailable_event(
            1,
            outage(),
            UnavailableOutcome::Parked {
                question: question("q-outage-Ünicode", MID),
            },
        ),
    );
    let mut run = parked.run.take().expect("started");
    run.tasks[ALPHA.index()].state = TaskState::Failed;
    run.queue.remove(ZETA, GenerationId(0));
    run.leases.release(LeaseOwner::Candidate {
        key: ZETA,
        generation: GenerationId(0),
    });
    parked.run = Some(run);

    let entry = &parked.queue().expect("started").entries()[0];
    assert_eq!(entry.candidate.key, MID);
    assert_eq!(entry.defers, 1, "the history the mutation would read");
    assert!(
        !entry.verification_deferred,
        "and the flag, which is what backoff_pending is about"
    );
    assert_eq!(parked.task_state(MID), Some(TaskState::AwaitingInput));
    assert_eq!(
        parked.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Parked),
        "a candidate that has deferred before and is now parked is parked, not backing off"
    );
    accepts(&parked, &run_finished(RunOutcome::Parked, None));
}

#[test]
fn a_failure_blocks_the_whole_dependency_closure_and_not_only_its_neighbours() {
    let base = sha("base");
    let started_event = chain_run_started_event();
    let mut live = TopologyFold::new(chain_inputs());
    apply(&mut live, &started_event);
    let mut trace = vec![started_event];
    let push = |live: &mut TopologyFold, trace: &mut Vec<TopologyEvent>, event: TopologyEvent| {
        apply(live, &event);
        trace.push(event);
    };

    let registry = live.registry().expect("started");
    assert_eq!(registry.get(AAY).expect("aay").deps, vec![BEE]);
    assert_eq!(registry.get(BEE).expect("bee").deps, vec![CEE]);
    assert!(registry.get(CEE).expect("cee").deps.is_empty());
    assert!(
        !registry.get(AAY).expect("aay").deps.contains(&CEE),
        "the first task must not depend on the failure directly, or this proves nothing"
    );

    let cee_dispatch = dispatch_in(&live, CEE, 0, &base);
    push(&mut live, &mut trace, cee_dispatch);
    let start = attempt_started(&live, CEE, 0, 1, 0);
    push(&mut live, &mut trace, start);
    assert_eq!(live.derived_outcome(), DerivedOutcome::NotEnding);
    push(
        &mut live,
        &mut trace,
        settle(
            CEE,
            0,
            1,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Failed {
                    halts_run: false,
                    reason: "  the ladder ran out  ".to_owned(),
                },
                lease: LeaseDisposition::PredictedReleased,
            },
        ),
    );

    assert_eq!(live.task_state(CEE), Some(TaskState::Failed));
    assert_eq!(live.task_state(BEE), Some(TaskState::Pending));
    assert_eq!(live.task_state(AAY), Some(TaskState::Pending));
    assert!(live.queue().expect("started").is_empty());
    assert!(!live.leases().expect("started").any_candidate_or_lineage());
    assert!(live.open_questions().expect("started").is_empty());
    assert_eq!(live.halted_at(), None);
    assert_eq!(
        live.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete),
        "a transitively blocked task is Blocked, and a run of Merged, Failed and Blocked \
             tasks is Complete"
    );

    push(
        &mut live,
        &mut trace,
        run_finished(RunOutcome::Complete, None),
    );
    assert_eq!(live.finished(), Some(&RunOutcome::Complete));

    let mut log = Vec::new();
    for event in &trace {
        log.extend_from_slice(serde_json::to_string(event).expect("serialize").as_bytes());
        log.push(b'\n');
    }
    let parsed = TopologyFold::parse_log(&log).expect("the log parses");
    assert_eq!(parsed, trace);
    let replayed =
        TopologyFold::replay(chain_inputs(), &parsed).expect("a blocked-closure log replays");
    assert_eq!(live.state(), replayed.state());
    assert_eq!(
        replayed.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete)
    );

    let prefix = TopologyFold::replay(chain_inputs(), &trace[..1]).expect("the prefix replays");
    assert_eq!(prefix.task_state(CEE), Some(TaskState::Pending));
    assert_eq!(prefix.derived_outcome(), DerivedOutcome::NotEnding);
}

#[test]
fn a_quiet_lineage_member_accepts_questions_and_decline_settles_the_lineage() {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut events = Vec::new();
    {
        let mut fold = started();
        let start = attempt_started(&fold, ALPHA, 0, 1, 0);
        let mut rejected = MergeRejected {
            sequence: SequenceId(0),
            candidate: candidate_of(ALPHA, 0),
            rejecting_head: head.clone(),
            disposition: RejectionDisposition::CodeRejected {
                verification: verification_record(Verdict::Rejected),
            },
            repair: repair_spawn(TaskKey(3), ALPHA, ALPHA),
            lease_effect: RejectionLeaseEffect::CreatesLineage {
                root: ALPHA,
                paths: region(ALPHA),
            },
        };
        rejected.repair.entry.deps = Vec::new();
        rejected.repair.entry.display_deps = Vec::new();
        for event in [
            dispatch(ALPHA, 0, &base),
            start,
            candidate_prepared(ALPHA, 0, &base),
            candidate_created(ALPHA, 0),
            verification_started(ALPHA, 0, 0, &head, &proposal),
            ev(TopologyEventBody::MergeRejected {
                data: Box::new(rejected),
            }),
        ] {
            apply(&mut fold, &event);
            events.push(event);
        }
    }
    let (fold, log) = folded(&events);
    assert_eq!(fold.task_state(TaskKey(3)), Some(TaskState::Pending));
    assert_eq!(fold.task_state(ALPHA), Some(TaskState::AwaitingRepair));
    for answer in [
        Answer4::Answered {
            option_index: 0,
            binding_override: None,
        },
        Answer4::Declined {
            decline_halts_run: false,
        },
    ] {
        let mut after = fold.clone();
        let mut replay_log = log.clone();
        let declined = matches!(answer, Answer4::Declined { .. });
        for event in [
            raised("q-park-Ünicode", TaskKey(3)),
            answered(TaskKey(3), "q-park-Ünicode", answer),
        ] {
            apply(&mut after, &event);
            replay_log.push(event);
        }
        if declined {
            assert_eq!(after.task_state(ALPHA), Some(TaskState::Failed));
            assert_eq!(after.task_state(TaskKey(3)), Some(TaskState::Failed));
            assert_ne!(after.derived_outcome(), DerivedOutcome::FoldError);
        } else {
            assert_eq!(after.task_state(ALPHA), Some(TaskState::AwaitingRepair));
            assert!(after.ready(TaskKey(3)));
        }
        let replayed = TopologyFold::replay(inputs(), &replay_log).expect("lineage answers replay");
        assert_eq!(after.state(), replayed.state());
    }
}
