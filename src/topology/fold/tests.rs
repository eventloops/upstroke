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

const RUN_ID: &str = "01FOLD0000000000000000000A";

/// One way to damage an otherwise valid record, for the refusal tables.
type BreakRunner = fn(&mut RunnerPolicy);
type BreakLadder = fn(&mut FrozenLadder);
type BreakFrozenInputs = fn(&mut Plan, &mut ChainSummary);
type BreakSpawn = fn(&mut FrozenSpawn);
type BreakBinding = fn(&mut RungBinding);
type BreakPublication = fn(&mut MergePrepared);

/// One coordinate of an embedded candidate identity, forged.
type ForgeCandidate = fn(&mut MergePrepared);

/// One residue a Complete run refuses to leave behind.
type AddResidue = fn(&mut RunState);
type BreakRejection = fn(&mut MergeRejected);
const ZETA: TaskKey = TaskKey(0);
const ALPHA: TaskKey = TaskKey(1);
const MID: TaskKey = TaskKey(2);
/// The fourth task of [`wide_plan`] only. `plan()` and `chain_plan()` have
/// three, and `region` already answers `src/repairs` for this key.
const BETA: TaskKey = TaskKey(3);

// -----------------------------------------------------------------------
// Fixtures
//
// Every independently meaningful field varies independently. Nothing sits
// at a default, no two fields that could be read for one another hold the
// same value, and every list that has an order is written in one that is
// neither sorted nor reversed. Where a value could be confused with
// another of its type — a commit sha with a tree sha, a task's floor with
// its ceiling, one epoch with another — the two are different literals.
// -----------------------------------------------------------------------

/// A distinct 40-character hex-shaped sha per label.
///
/// Distinct per role rather than per value: a base, a parent, a tree, a
/// commit and a head are five different claims, and a fixture that let any
/// two of them share a literal would pass under a relation that compared
/// the wrong pair.
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

/// The agents this run's pre-flight probed: padded, mixed case, multi-byte
/// and over-length, in an order that is neither sorted nor reversed, and
/// deliberately a superset of the agents the ladders bind.
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

/// Plan order, display-id order and topological order all disagree, and the
/// three tasks touch three disjoint regions so that a lease check has
/// something to be wrong about in both directions.
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

/// A ladder that belongs to one task and to no other: different length,
/// different attempts allowance, and every rung's agent, model and pin
/// derived from the task's own id.
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

/// Four distinct efforts, so a rung bound at the wrong tier's effort is a
/// different value rather than the same one.
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

/// A three-task chain whose dependencies all refer *forward* in key
/// order: `aay`(0) depends on `bee`(1), which depends on `cee`(2).
///
/// Keys are assigned in plan order (`keys_by_display_id`), and plan order
/// is not topological order, so this shape is an ordinary plan rather than
/// a contrived one. It is the shape the derived-`Blocked` predicate has to
/// be right about: `aay`'s only failure is two hops away, and a derivation
/// that decided each task from what it had decided so far would reach
/// `aay` before it had decided `bee`.
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

/// The chain plan's `run_started`, authenticated against its own registry.
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

/// Four tasks that wait on nothing and touch four disjoint regions.
///
/// `plan()` and `chain_plan()` are both chains, and in a chain at most one
/// of `ready`, `ready_retry` and `integration_admissible` can hold at a
/// time: everything waits on one task, and that task is pending, open, or
/// merged. A predicate that is never independently true is one no guard
/// over it can be measured against — which is how four of the five poison
/// guards came to be asserted by a test that would have passed without
/// them. The three original ids keep their kinds, tiers, hints and
/// ladders; only `depends_on` differs, and `beta` is the fourth holder a
/// held pipeline entitlement needs.
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

/// The wide plan's `run_started`, authenticated against its own registry,
/// at a stated pipeline width.
///
/// The width is a parameter because it is the one limit selection reads,
/// and because `DEFAULT_MAX_PARALLEL` is 1: a fixture fixed at 3 tests a
/// width `config` refuses to create a run at.
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

/// A fold over [`wide_plan`] that has recorded its `run_started`.
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

/// The run record with a digest field nothing has filled in yet, so that
/// the digest can be derived from it without deriving it from itself.
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
        // Three different numbers: a fold that read one limit where it
        // meant another lands on a value this fixture does not hold.
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

/// A fold that has recorded its `run_started` and nothing else.
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

// --- event builders ----------------------------------------------------

/// One review pass's ledger line, named and concluded.
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

/// The complete successful attempt **for this task under the frozen plan**.
///
/// `TaskKey` is the plan index, so whether a second opinion is configured is
/// derived from `review_plan` rather than asserted by the fixture: the
/// premise carries exactly the passes §11.2 requires of that task, and no
/// others. `review_plan` configures one for index 2 alone.
fn attempt_record_for(key: TaskKey, attempt: u32) -> AttemptRecord {
    let mut record = attempt_record(attempt);
    // Long enough to include this task's own slot; `review_plan` decides
    // each index by the same closure the real fixtures use, so slot `key.0`
    // holds exactly what the frozen plan gives that task.
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

/// A **complete** successful attempt for a task the plan gives no second
/// opinion.
///
/// The primary pass is present and `Passed`. This carried a lone
/// `second-opinion` entry and no primary at all — a record that satisfies
/// `is_successful` only because `all` over its passes never sees the pass
/// §11.2 actually requires. A positive premise that passes vacuously
/// witnesses nothing about the clause it is meant to exercise: delete the
/// review half of `is_successful` and no positive test here would notice.
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

/// The region a task's candidate touches. Disjoint per task, so an overlap
/// in a test is one the test put there.
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

/// An ordinary dispatch of a task **of the default [`plan`]**, taking the
/// region that plan's frozen hints derive.
///
/// `region` is keyed by [`TaskKey`] and the default plan is the only plan
/// those keys belong to, so a fixture on another plan — [`chain_plan`] is
/// the one — takes [`dispatch_in`] instead, which asks the fold. The
/// agreement between this table and the derivation is not assumed:
/// [`the_dispatch_fixture_records_the_region_the_fold_derives`] round-trips
/// all three.
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

/// [`dispatch`], with the predicted region taken from `fold` rather than
/// from the default plan's table.
///
/// What a conforming driver does — `TopologyRun` reads
/// [`TopologyFold::predicted_region`] — and what any fixture on a plan
/// other than [`plan`] needs, because the keys are dense per plan and
/// `region`'s table is the default plan's.
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

/// Delegates to the production reader rather than repeating its
/// composition.
///
/// It used to repeat it, and that made this file hold **two** derivations
/// of one value — the validator's, in `check_attempt_started`, and this
/// one. Every test that builds an `attempt_started` goes through here, so
/// routing it to [`TopologyFold::frozen_rung_binding`] puts that reader
/// under the whole existing attempt corpus: if it ever disagrees with the
/// validator beside it, dozens of tests fail rather than none.
fn frozen_binding(fold: &TopologyFold, key: TaskKey, rung: usize) -> RungBinding {
    fold.frozen_rung_binding(key, u32::try_from(rung).expect("a small fixture rung"))
        .expect("the run has started and the fixture task has this rung")
}

/// The reader's answer is exactly what the validator accepts.
///
/// **Round-tripped against `check_attempt_started`, not compared to a
/// literal.** A literal expectation would be a second transcription of the
/// same rule, and would agree with this reader for the same reason the
/// reader is right or wrong — the self-oracle shape. Feeding the reader's
/// output to the validator asks the only question that matters: do the two
/// halves of this file agree.
///
/// The negative half is what gives it teeth. Perturbing one field of the
/// binding must be refused, or the validator is not checking the thing the
/// reader produces and the positive half proves nothing.
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

/// [`attempt_started`], resuming `session` — the same-generation retry a
/// `Retained` settlement exists to admit.
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

/// `attempt_finished`, whose record **says the attempt failed**.
///
/// Every settlement this can build is a failure — `candidate_prepared` is the
/// sole successful one — so the record is derived to match rather than left
/// as the "worker ran and its work was accepted" shape. Built the other way,
/// each caller produced a settlement that fails a task while carrying a
/// ledger line saying the work passed, which `check_attempt_finished` has
/// refused since 2026-08-27.
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
    // **One session, in both places the event carries it.** Production
    // takes the settlement's `retained_session` and the record's
    // `session_id` from one value — `assessed.outcome.session_id` — and the
    // fold refuses a retained settlement whose two halves disagree about
    // which conversation was left open. A builder that left the record's
    // stock id in place would be constructing that disagreement in every
    // retained fixture in this file.
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

/// [`settle`], with a failure on the record.
///
/// The allowance is decided from `AttemptRecord.failure`, so a settlement
/// built without one is the "worker ran and its work was accepted" cell and
/// cannot exercise any other.
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

/// A `candidate_prepared` naming the attempt that produced it.
///
/// ST-06 binds the embedded record to the generation's current successful
/// attempt, so a fixture whose generation retried has to say so: after one
/// retry the candidate belongs to attempt 2, and a builder that hard-coded
/// 1 would be asserting the very mismatch the fold refuses.
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

// --- selection accessors -----------------------------------------------

/// The accessors answer for an unstarted run, and answer it as a statement
/// rather than as an `Option` the caller must decide what to do with.
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

/// `ready` is the fold's predicate, not a constant: exactly the task whose
/// dependencies are met is ready, and the two that depend on it are not.
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

/// `pipeline_held` counts what the packet says holds the entitlement, and
/// the count moves with the generation class.
///
/// This is the accessor a caller would otherwise re-derive by walking
/// `GenerationClass` itself, so the assertion is that the accessor agrees
/// with the classes actually present — not merely that it returns a number.
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

    // max_parallel is 3 in this fixture, so one held entitlement leaves
    // room — the reservable predicate is a comparison, not a boolean flag.
    assert!(fold.pipeline_reservable());
}

/// A settlement to `RetainedIdle` releases the pipeline entitlement while
/// keeping the generation open — the one class whose two properties differ.
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

// --- the Retained arm asks what the Closed arm asks ---------------------
//
// `PR7-G2-W1-RETAINED-ARM-UNGUARDED` (§2, §22e). Round 6's four new
// settlement refusals all construct `Closed`, which is why this arm was
// undriven: it checked the epoch and stopped.

/// A `Retained` settlement of `key`'s first attempt, session and all.
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

/// **The Retained arm asks the same questions the Closed arm asks.**
///
/// A settlement carries an envelope and a ledger line, and this arm bound
/// them to each other in one field — the incarnation — and left the rest
/// free. So a current-epoch retained settlement could carry a record
/// belonging to a different attempt of the same generation, and could name
/// a conversation the ledger line does not report.
///
/// The positive premise is asserted first in every row, so each refusal is
/// about the one field the row moves.
#[test]
fn a_retained_settlement_binds_its_envelope_to_its_record() {
    let base = sha("base");
    let session = "sess-ÜNI-retained";
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    // The premise: the coherent settlement applies.
    accepts(&fold, &retain(ZETA, 1, session, Epoch(0)));

    // The record's attempt is not the envelope's.
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

    // The record names another conversation.
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

    // And names none at all, which is the shape the scaffold emitted.
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

    // The envelope's generation is not the open one.
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

    // The incarnation is not this run's.
    assert!(
        matches!(
            refuse(&fold, &retain(ZETA, 1, session, Epoch(7))),
            FoldError::StaleIncarnation { .. }
        ),
        "a retained settlement claimed an incarnation this run is not in"
    );

    // Nothing moved on any of them: the generation is still in flight.
    let generation = fold
        .task(ZETA)
        .and_then(|task| task.generations.first())
        .expect("the generation is open");
    assert!(matches!(generation.class, GenerationClass::InFlight { .. }));
}

/// One arm of the settlement door: a label and a builder for the
/// settlement that reaches it.
type SettlementArm = (&'static str, fn() -> AttemptSettlement);

/// **No settlement of `attempt_finished` accepts a record that claims the
/// attempt succeeded — on either arm.**
///
/// The sibling-arm witness. `candidate_prepared` is the sole successful
/// settlement (INV-07,
/// `decisions/2026-08-12-merge-queue-execution-topology.md`), and the
/// `Closed` arm has enforced that against the record since round 6. The
/// `Retained` arm did not, so the invariant held on one path through the
/// door and not the other: a retained settlement could carry a record with
/// no failure and every configured pass green — a record
/// `check_candidate_prepared` would itself accept — and the ledger line an
/// operator reads would say the work passed while the fold held the
/// generation open for a retry.
///
/// **What "retained" means, and why requiring this is not requiring a
/// terminal failure.** `settle_failed` is the only producer of a `Retained`
/// settlement: it is reached on the failure path, for a same-rung retry
/// that has a session to resume. So a retained attempt has *not* succeeded,
/// by construction. `is_successful()` being false is the record saying that
/// much and no more — it does not require a `Failed` transition, which
/// `Retained` has no field for, and it does not make the generation
/// terminal.
///
/// Driven over the identical record on both arms, because the claim is that
/// the two agree rather than that each refuses something.
#[test]
fn no_attempt_finished_arm_accepts_a_record_that_claims_success() {
    let base = sha("base");
    let session = "sess-ÜNI-unsettled";

    // The one shape both arms must refuse: no failure, and the frozen
    // obligation all green — which is exactly what `candidate_prepared`
    // requires of the settlement that *is* a success.
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

        // The premise: with a record that does not claim success, this
        // exact settlement applies — so the refusal below is about the
        // claim and nothing else.
        accepts(&fold, &settle(ZETA, 0, 1, settlement()));

        let mut lying = settle(ZETA, 0, 1, settlement());
        claims_success(&mut lying);
        let error = refuse(&fold, &lying);
        assert!(
            matches!(error, FoldError::InconsistentRecord { .. }),
            "{label}: a record claiming success settled an attempt: {error:?}"
        );

        // Nothing moved.
        let generation = fold
            .task(ZETA)
            .and_then(|task| task.generations.first())
            .expect("the generation is open");
        assert!(
            matches!(generation.class, GenerationClass::InFlight { .. }),
            "{label}: the refused settlement moved the generation anyway"
        );

        // **And the predicate is the shared one, not half of it.** A record
        // whose failure field is empty and whose configured pass came back
        // `Failed` makes no success claim — §11.2's "every configured pass
        // passes" is the other half of `is_successful`, and it is the half
        // an arm re-deriving the question from `failure.is_none()` would
        // lose. Both arms take this record.
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

    // And the door that *does* take a successful record still takes it, so
    // this narrows `attempt_finished` rather than closing success off
    // altogether.
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    accepts(&fold, &candidate_prepared(ZETA, 0, &base));
    let _ = session;
}

/// **What a Retained settlement does to the run: pipeline released, lease
/// retained, generation open, task not terminal — and once.**
///
/// The row's other half. "Releases only the pipeline entitlement" is a
/// claim about the state after the arm applies, and the negatives are
/// double release (a second settlement of a generation that is no longer in
/// flight) and a new-process retry (a resume by an incarnation that did not
/// retain the session). Both are refusals the fold already makes and
/// neither was driven from this arm.
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

    // Released, exactly once, and nothing else went with it.
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

    // Double release: the generation is no longer in flight, so a second
    // settlement of it — retained or closed — is refused.
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

    // Same-generation retry: accepted in the retaining incarnation, and
    // only there. The second half is the new-process refusal.
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
    // And a retry naming some other conversation is refused in the
    // retaining incarnation too.
    assert!(matches!(
        refuse(
            &fold,
            &attempt_started_resuming(&fold, ZETA, 0, 2, 0, "sess-somebody-elses")
        ),
        FoldError::StaleIncarnation { .. }
    ));
}

/// A poisoned fold authorises nothing.
///
/// INV-20: "no completion is applied after the fold is poisoned by a
/// returned append error". `plan_transition` already refuses; a predicate
/// that kept answering `true` would let the coordinator select work from a
/// state this process can no longer vouch for.
#[test]
fn a_poisoned_fold_authorises_nothing_while_still_reporting_what_it_holds() {
    // Every one of the five predicates is **independently true** before
    // the poison, which is the whole of what makes the five assertions
    // after it load-bearing: `alpha` waits on nothing and holds no
    // generation, `zeta` holds a generation this incarnation retained,
    // `mid`'s candidate is queued and eligible, and `beta` holds one of the
    // three pipeline entitlements. This test used to poison a fold in
    // which `alpha` had just been dispatched and the other two waited on
    // it — nothing was admissible even unpoisoned, and four of the five
    // guards could be deleted without it going red.
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

    // Accounting, not authorisation: answering `0` here would be a false
    // statement about the run rather than a refusal. The rule that keeps a
    // report from being derived from this is the append-error protocol's,
    // and it belongs in the emit path.
    assert_eq!(
        fold.pipeline_held(),
        1,
        "the entitlement is still held; only the authorisation is withdrawn"
    );
}

/// The pipeline entitlement is a clause of `integration_admissible`, and
/// at the width production actually runs it is the binding one.
///
/// `permits.pipeline` counts an unresolved integration transaction among
/// the held, `permits.provisional_reservations` gives integration
/// selection `{pipeline, merge}`, and `deadlock_freedom` takes a
/// reservation "only when the derived count permits". So an integration is
/// admissible only within `max_parallel`, exactly as a dispatch and a
/// retry are.
///
/// At width 1 — `DEFAULT_MAX_PARALLEL`, and the only width `config`
/// accepts for a fresh run — this is reachable rather than theoretical: a
/// crash after `task_dispatched` and before `attempt_started` leaves an
/// `OpenNoAttempt` generation holding the single slot, and the resumed
/// loop's first selection is where an admissibility that ignored the count
/// would spend it twice.
#[test]
fn an_integration_is_inadmissible_while_the_pipeline_entitlement_is_held() {
    let mut narrow = wide_started(1);
    queue_candidate(&mut narrow, MID, 0);
    assert_eq!(narrow.pipeline_held(), 0, "the generation closed");
    assert!(
        narrow.integration_admissible(),
        "an eligible candidate with the slot free is admissible"
    );

    // `zeta` takes the only slot, and stops where a crash between the
    // dispatch and the first attempt stops it.
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

    // One slot wider, the identical state admits it: the clause under
    // test is the count and nothing else about this fixture.
    let mut wider = wide_started(2);
    queue_candidate(&mut wider, MID, 0);
    apply(&mut wider, &dispatch(ZETA, 0, &sha("base")));
    assert_eq!(wider.pipeline_held(), 1);
    assert!(
        wider.integration_admissible(),
        "one of two entitlements held leaves room for the integration's"
    );
}

/// **A task's ladder position survives the process that wrote it.**
///
/// The companion to the deferral witness, and the same disease. A
/// settlement that escalates closes the generation and leaves the task
/// `Pending` — so the ready-dispatch branch selects it again, and the rung
/// it runs at is a fact only the log holds.
///
/// A driver that assumed rung 0 would dispatch an escalated task on rung 0
/// forever, never reaching the tier its chain escalated it to. A driver
/// that assumed attempt 1 would hand `next_step` the first attempt of the
/// allowance every time, so the task would retry forever and never
/// escalate at all. Both were true of `TopologyRun` until this field
/// existed, and neither was visible as a wrong number — only as a run that
/// behaves differently after a restart.
#[test]
fn a_ladder_position_is_derived_by_replay_and_not_assumed() {
    let base = sha("base");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    // Rung 0, two attempts, allowance spent -> escalate onto rung 1.
    for attempt in 1..=2u32 {
        for event in [
            dispatch(ALPHA, attempt - 1, &base),
            attempt_started(&live, ALPHA, attempt - 1, 1, 0),
        ] {
            apply(&mut live, &event);
            trace.push(event);
        }
        let last = attempt == 2;
        let settlement = settle(
            ALPHA,
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

    let task = live.task(ALPHA).expect("registered");
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

    // Through the wire, because a resume reads bytes.
    let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");
    let replayed = TopologyFold::replay(inputs(), &parsed).expect("the log replays");
    let after = replayed.task(ALPHA).expect("registered");
    assert_eq!(
        (after.rung, after.attempts_on_rung),
        (task.rung, task.attempts_on_rung),
        "the ladder position did not survive the process that wrote it, so \
             the next one would dispatch this task on a rung the log contradicts"
    );

    // The two assumptions this replaces, shown wrong. A fresh process
    // starts both at zero and agrees with the fold on every reading until
    // a resume — which is exactly when nothing is watching.
    assert_ne!(0, after.rung, "a process-local rung tally reads zero here");
}

/// `RunState::charge_allowance`, as a value.
///
/// `runner::tests::the_rungs_allowance_is_counted_in_one_production_place`
/// carries a `SPELLINGS` fixture listing the ways this call can be written.
/// That fixture is a `&str`, so rustc never reads it and a path in it can name
/// nothing at all — it named `TaskFold::charge_allowance` for a round. This is
/// the same path where the compiler does read it.
const CHARGE_ALLOWANCE: fn(&mut RunState, TaskKey, &AttemptRecord) = RunState::charge_allowance;

/// **An interrupted attempt does not spend the rung's allowance.**
///
/// `transaction_fault_matrix[T-ATTEMPT]`'s `resume_action` in its own words:
/// append `attempt_interrupted` *"(unknown spend, **allowance refunded**…)"*.
/// `ladder::spends_allowance` agrees from the other direction —
/// `FailureKind::Interrupted` is `false`, because "the engine died between
/// an attempt starting and finishing, so nothing judged the code".
///
/// **This fold disagreed with both for the whole of PR7.** It counted every
/// `attempt_started`, so an interruption, a park and an outage each burned a
/// rung the packet says they do not — and the divergence was invisible
/// because the count is only ever read across a resume. Found by S5 round 2
/// (`emit` and `settle`, independently).
///
/// The pair is asserted, not just the repair: a **judged** rejection spends,
/// an **interruption** does not, and the difference is the only thing that
/// changed between the two halves. A fold that stopped counting altogether
/// would satisfy half of this and fail the other.
///
/// # The behavioural half of the `runner` census
///
/// `runner::tests::the_rungs_allowance_is_counted_in_one_production_place`
/// counts the *spelling* `charge_allowance(` in each applier's body, and a
/// count over text cannot enforce a property about calls: an alias and a
/// closure of the same name leave its per-applier map and its subtree total
/// both reading exactly what they read today while a whole settlement arm
/// stops charging. That was measured at `823ad36`, against the whole suite
/// and not only the census.
///
/// This test reads `attempts_on_rung` off the state instead, so a spelling is
/// invisible to it by construction, and it drives **every settlement the
/// vocabulary has** rather than the one arm the repair was written on — an
/// escape that skips a single arm is the shape this class arrives in.
/// `apply_candidate_prepared`, the successful settlement, is the sibling half
/// and is driven by
/// [`a_successful_attempt_charges_its_rung_live_and_on_replay`].
///
/// **`Escalated` is excluded, and that is not a gap.** The arm resets
/// `attempts_on_rung` to zero on the rung it climbs onto, *after* the charge,
/// so the charge has no observable effect there — not by this test and not by
/// anything else. There is nothing for an escape to gain by skipping it.
#[test]
fn an_interrupted_attempt_refunds_the_rungs_allowance() {
    use crate::ladder::FailureKind;

    let base = sha("base");

    // Half one: a judged rejection. The worker ran and produced work to
    // judge, so it spends — this is the cell that keeps the count honest.
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

    // Half two: the same shape, interrupted. Same dispatch, same start, and
    // the settlement is the only difference.
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

    // **The same pair on every settlement `apply_settlement` can be handed.**
    // The two halves above both settle `Closed`/`Retry`, so an applier that
    // charged on that arm and nowhere else satisfied them — and that is
    // precisely the escape the lexical census cannot see:
    //
    //     let real_charge = Self::charge_allowance;
    //     let charge_allowance = |state: &mut Self| {
    //         if !matches!(&finished.settlement, AttemptSettlement::Retained { .. }) {
    //             real_charge(state, finished.key, &finished.record);
    //         }
    //     };
    //     charge_allowance(self);
    //
    // With `attempts_per = 2` a retained failure then never persists its
    // spend, the next rejection derives `0 + 1 < 2`, and the run retries the
    // rung it should have escalated off — indefinitely, while every count in
    // `runner`'s census still reads what it reads today.
    //
    // **The label is derived by an exhaustive match, not written beside each
    // arm.** A hand-written list of arms with hand-written names is how an arm
    // nobody thought to charge arrives: it is missing, and nothing says so.
    // `label_of` matches every shape the wire vocabulary has, so a variant
    // added to `AttemptSettlement` or `SettlementTransition` stops the build
    // here, and the coverage assertion below is over names this match produced
    // rather than over names this test asserted about itself.
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
    // The two the vocabulary has and this test does not drive, named rather
    // than absent. `closed/succeeded` is refused by `check_attempt_finished`
    // before `apply` is reached — `candidate_prepared` is the sole successful
    // settlement — and `closed/escalated` resets the count to zero on the rung
    // it climbs onto, *after* the charge, so the charge has no observable
    // effect there for an escape to gain.
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
        // The judged/interrupted pair, so an arm that stopped charging
        // altogether and an arm that charges everything are told apart by the
        // same two cells the halves above use.
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
            // One session in both places the event carries it, as `settle`
            // does: the fold refuses a retained settlement whose two halves
            // disagree about which conversation was left open.
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

    // **The compiled half of that census's `SPELLINGS` fixture.** The fixture
    // is a string the census counts over, so it named
    // `TaskFold::charge_allowance` — an item that does not exist, the method
    // being defined on `RunState` — for a whole round with nothing able to
    // report it. [`CHARGE_ALLOWANCE`] is the same path as a value: a rename or
    // a move to another type stops the build here rather than silently
    // emptying a control there. Called, not merely bound, so the item it names
    // is shown to be the one that moves the count.
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

/// **A deferral count survives the process that wrote it, and a
/// driver-side tally does not.**
///
/// The witness for why this count is the fold's. `ladder::next_step` reads
/// it on exactly one branch — an outage defers while `defers < max_defers`
/// and parks at it — so a run that has already spent its allowance must
/// park rather than defer again.
///
/// A driver keeping its own tally is correct for as long as its process
/// lives. This test is the case where that stops being true: the log holds
/// three deferrals, the process dies, and the next one replays. The fold
/// reaches three. A fresh in-memory counter reaches **zero**, and with
/// `max_defers = 3` the run would defer a fourth time, and a fifth, and
/// never park — the allowance silently becoming unbounded across a resume.
///
/// That is `predicted_region`'s shape with a resume-shaped fuse: two
/// derivations of one number, agreeing until the moment they do not.
#[test]
fn a_deferral_count_is_derived_by_replay_and_not_by_a_process_local_tally() {
    let base = sha("base");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    // Three deferrals of one task, each one a fresh generation the way a
    // `defer_wait_elapsed` wake produces.
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

        // `Deferred -> Pending via defer_wait_elapsed`, which is the
        // transition the contract names and the only way back to a
        // dispatchable state. The fold refuses a re-dispatch without it.
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

    // Through the wire, because a resume reads bytes and not values.
    let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");
    let replayed = TopologyFold::replay(inputs(), &parsed).expect("the log replays");
    let replayed_defers = replayed.task(ALPHA).expect("registered").defers;

    assert_eq!(
        replayed_defers, live_defers,
        "the count did not survive the process that wrote it, so the next \
             one would decide the outage branch from a number the log \
             contradicts"
    );

    // The tally the driver is forbidden from keeping, shown failing. A new
    // process starts one at zero: it agrees with the fold on every reading
    // until a resume, and this is the reading after one.
    let process_local_tally: u32 = 0;
    assert_ne!(
        process_local_tally, replayed_defers,
        "a process-local tally is only wrong across a resume, which is \
             exactly when nothing is watching it"
    );
}

/// The three statements the selector delegates rather than re-derives.
///
/// Statements about the run and not authorisations, which is why poisoning
/// does not flip them: a poisoned fold of a run with a deferred task still
/// has one, and `false` there would be a false statement rather than a
/// refusal. `pipeline_held` is exempted for the same reason and by the
/// same sentence.
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

    // `run_is_ending` is the epoch-aware half, which is why a caller must
    // not read `budget_stop` for itself: a stop of **this** epoch ends the
    // run, and the resume that raised the ceiling clears it.
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

    // A halt ends it in every epoch, poisoned or not.
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

/// Take one task to a **queued** candidate: dispatch, attempt, success,
/// prepare, create.
///
/// [`merge_task`] minus its last two events. The generation closes at
/// `task_candidate_created` and releases the entitlement it held, so a
/// fold built this way holds a queued candidate and nothing else.
fn queue_candidate(fold: &mut TopologyFold, key: TaskKey, generation: u32) {
    let base = sha("base");
    apply(fold, &dispatch(key, generation, &base));
    let start = attempt_started(fold, key, generation, 1, 0);
    apply(fold, &start);
    apply(fold, &candidate_prepared(key, generation, &base));
    apply(fold, &candidate_created(key, generation));
}

/// A generation of `key` retained by the incarnation the fold is in.
///
/// The incarnation is read from the fold rather than written as `0`:
/// `ready_retry` is false in every incarnation but the retaining one, so a
/// fixture that hard-coded the epoch would silently stop being a
/// `ready_retry` state the moment it was used after a resume.
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

/// Drive one task from pending to merged over the fast path, at the head
/// the integration ref is currently at.
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

// -----------------------------------------------------------------------
// The header: what a fold may be started with (refusals 4, 5, and the
// ladder validation the fold boundary owns)
// -----------------------------------------------------------------------

#[test]
fn a_topology_log_is_folded_from_its_run_started_and_from_nothing_else() {
    // Every kind, not a sample: the first line of a topology log records
    // the registry, the runner and the limits that every later event is
    // checked against, so there is no event that means anything without it
    // — including the informational ones, which a poisoned or unstarted
    // process still may not append.
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

    // A record that does not claim the topology schema is not one this
    // fold may interpret, whatever else it says.
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
    // refusals[5], first half, over every defect the record can exhibit —
    // and, at the top, over the one shape that is *not* a defect: a
    // container whose runtime reported no manifest digest. INV-23 makes the
    // digest "the manifest digest when reported", so a record without one
    // is complete, and a fold that refused it would refuse a legitimate
    // run on a runtime that reports none.
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
    // refusals[5], second half / INV-23: exact equality, and the refusal
    // names *which* field moved, because a config edit, a moved tag and a
    // rebuilt image behind an unchanged tag are indistinguishable as
    // "runner mismatch" and have completely different fixes.
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

    // And the set is a set: the same volumes enumerated in another order
    // established the same runner.
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
    // refusals[5]: a `run_resumed` "whose runner kind, policy, image
    // reference, image id, image digest, or credential-volume set differs
    // from run_started(4).runner" is refused. Two things that a
    // field-by-field fixture leaves unpinned: the credential volumes are a
    // *map*, so its cardinality and its keys are not its value; and the
    // record it is compared with is `run_started`'s, not the previous
    // resume's.
    let mut fold = started();
    accepts(&fold, &resume(container_runner()));

    // Same size, same agents, one value moved — and then the values
    // swapped between the two agents, which keeps the multiset of values
    // as well.
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

    // The baseline is `run_started`, so an accepted resume does not become
    // the thing the next one is measured against. Drift A -> A -> B -> A:
    // B is refused where it stands, and A is still the record afterwards.
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
    // refusals[4]. Two digests, moved one at a time: a fold that compared
    // one where it meant the other, or that compared neither, is caught by
    // whichever case it does not implement.
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

    // The refusal is about the *plan* as much as the record: the same
    // record against a plan that moved by one field is the same refusal,
    // which is the case the digest exists for.
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

    // And the allow-list is one of the inputs it authenticates: a run that
    // probed something else derives a different registry.
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

    // The comparison is of the whole value. The cases above move a digest
    // to something unrelated, which a truncated or prefix comparison
    // rejects just as well; these move the *last* character of each,
    // independently, so a comparison of anything short of the whole
    // accepts them. The two digests are pairwise unrelated in this
    // fixture, so neither can supply the other's expected equality.
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
    // Fold-boundary work, not registry work: the registry derives whatever
    // the record says, and this decides whether that ladder may enter a
    // fold's state.
    //
    // The three cases here are the ones a *frozen plan and run record* can
    // express — every one of them is a registry the derivation builds
    // without complaint, which is precisely why the check has to live
    // here. The rest of the malformations cannot be written into a chain
    // at all (the derivation recomputes the ceiling, refuses an empty
    // ladder, refuses a misaligned binding) and are exercised below on the
    // path where an entry *is* the record: a spawn.
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

    // The same check on the way in through a spawn, over every
    // malformation an embedded entry can carry.
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

    // An empty clipped ladder waiting for a human binding is not malformed
    // — it is the shape a repair takes when its floor and its root's
    // ceiling do not intersect — but one that offers nothing to choose
    // from is.
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

/// A `run_started` whose frozen inputs give `zeta` a broken ladder, with
/// the recorded digest recomputed so the fold reaches the ladder check
/// rather than stopping at the digest.
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
    // The fold derives from *its* frozen plan, so the frozen inputs move
    // with the record: the floor lives in the plan, and a fixture that
    // moved only the record would be refused for the digest instead.
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

// -----------------------------------------------------------------------
// Registration and dispatch (refusals 10, and what a registered entry is)
// -----------------------------------------------------------------------

/// A repair entry, complete, as its registering event carries it.
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

    // Each case moves exactly one thing about an otherwise valid spawn,
    // and each reports something no other case reports.
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

    // The dependency-count mismatch is its own case: two lists that
    // describe one relation have to describe the same one.
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

    // The three legal pairings, and the run's frozen repair limit.
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

    // A binding question whose options are not the entry's.
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

    // A runnable event over an entry that has no binding, and the reverse.
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

    // And a question nobody could answer parks a task nothing un-parks.
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

    // A second generation while one is open.
    assert!(matches!(
        refuse(&fold, &dispatch(ZETA, 1, &base)),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));
    // A generation that skips a number, once the first has closed.
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

    // A task that is not pending.
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
    // And a task nobody registered.
    assert!(matches!(
        refuse(&merged_fold, &dispatch(TaskKey(9), 0, &base)),
        FoldError::UnknownKey { key: 9, .. }
    ));
}

// --- the recorded region, derivation-checked --------------------------
//
// `TASK-DISPATCHED-REGION-UNVALIDATED` (§2, §22). The sibling for the
// recorded *binding* is `the_frozen_rung_binding_is_what_the_validator_
// accepts` above; this is the same question one event earlier, and the
// asymmetry between the two is what the row was written about.

/// One hint shape, and the region the contract says it derives.
///
/// **Transcribed from the rule, not from the code.** The rule is "the
/// plan's path hints, taken literally: a hint with no glob metacharacter is
/// its own literal prefix; anything else — an absent hint list, or a hint
/// whose literal prefix is empty — classifies repo-wide". Reading
/// `predicted_region`'s body to build this table would make the grid agree
/// with the derivation for the reason the derivation is right or wrong,
/// which is the self-oracle shape `CODING_STANDARDS.md` names.
struct HintShape {
    /// The task's display id, which is also its fixture name.
    id: &'static str,
    /// What the plan froze.
    hints: &'static [&'static str],
    /// The prefixes the rule derives, or `None` for repo-wide.
    derives: Option<&'static [&'static str]>,
}

/// Every hint shape the rule distinguishes, one axis varied at a time.
///
/// The four glob metacharacters get a case each rather than one case with
/// all four, because a truncation that stopped at only three of them would
/// pass a combined case on the first one it did handle.
const HINT_SHAPES: &[HintShape] = &[
    // A literal is its own prefix, unchanged.
    HintShape {
        id: "literal",
        hints: &["src/literal"],
        derives: Some(&["src/literal"]),
    },
    // A trailing separator is not part of the name of the directory.
    HintShape {
        id: "trailing",
        hints: &["src/trailing/"],
        derives: Some(&["src/trailing"]),
    },
    // The four metacharacters, one each. Everything from the first one is
    // dropped, and the separator that precedes it goes with the trim.
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
    // A Windows-shaped hint names Git paths once its separators are.
    HintShape {
        id: "backslash",
        hints: &[r"src\backslash\deep"],
        derives: Some(&["src/backslash/deep"]),
    },
    // A doubled separator is **kept**: the rule trims the tail and
    // substitutes nothing. `src/doubled//inner` and `src/doubled/inner`
    // name one region to `paths_overlap`, which filters empty components —
    // and they are still two different literals, which is the whole reason
    // the comparison below is exact rather than semantic.
    HintShape {
        id: "doubled",
        hints: &["src/doubled//inner/"],
        derives: Some(&["src/doubled//inner"]),
    },
    // Case and non-ASCII survive: the region is the hint's own bytes.
    HintShape {
        id: "unicode",
        hints: &["src/Über/"],
        derives: Some(&["src/Über"]),
    },
    // Every hint contributes a prefix, in the frozen order.
    HintShape {
        id: "several",
        hints: &["zz/last", "aa/first", "build.rs"],
        derives: Some(&["zz/last", "aa/first", "build.rs"]),
    },
    // A leading glob leaves an empty literal prefix, which is repo-wide —
    // and repo-wide for **one** hint is repo-wide for the task, because an
    // unbounded region cannot be narrowed by a bounded sibling.
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
    // No hints at all: nothing was said about where the work lands.
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

/// The hint-shape plan's `run_started`, authenticated against its own
/// registry — the same construction [`chain_run_started_event`] uses.
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

/// **The derivation is one function of the frozen hints, and the door
/// accepts exactly its answer.**
///
/// Two halves over one table. The first is that the fold's own reader
/// returns what the *rule* says, independently transcribed above — so a
/// derivation that quietly changed (a metacharacter dropped from the stop
/// set, a trim that also collapsed separators) fails here rather than
/// somewhere downstream of it. The second is that a `task_dispatched`
/// recording that answer is admitted, which is what makes the refusal in
/// the sibling test a statement about divergence rather than about the
/// region being checked at all.
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

/// **A dispatch recording any other region is refused, and the refusal
/// names both regions.**
///
/// The negative half of the table above, and the finding itself:
/// `check_dispatched` used to match the lease's *shape* alone, so the fold
/// admitted on `predicted_region`'s answer and `apply_dispatched` granted
/// whatever the event carried — and the lease table's copy is the one every
/// later overlap check consults.
///
/// Each perturbation is one axis of the way two regions can disagree:
/// a component missing, one added, the same components in another order,
/// one component rewritten to something that *overlaps identically*, the
/// case folded under a run whose `PathPolicy` folds case, a narrowed region
/// widened to repo-wide, and a repo-wide region narrowed. The last two are
/// the pair that matters most at width: `RepoWide` overlaps everything, so
/// recording a narrow region for a repo-wide prediction is how a task that
/// should have serialized against every other runs beside them.
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
        // The literal hint, taken literally *including* the glob — the
        // shape the driver actually wrote, and the one `84a3978` repaired
        // in the driver while leaving the door open.
        (
            "the hint, unstripped",
            key_of("star"),
            narrowed(&["src/star/*.rs"]),
        ),
        // A component dropped.
        (
            "a component missing",
            key_of("several"),
            narrowed(&["zz/last", "aa/first"]),
        ),
        // A component added.
        (
            "a component added",
            key_of("several"),
            narrowed(&["zz/last", "aa/first", "build.rs", "src/extra"]),
        ),
        // The same components, sorted. Sorting is a normalisation a caller
        // could think harmless; the frozen order is the plan's.
        (
            "the components reordered",
            key_of("several"),
            narrowed(&["aa/first", "build.rs", "zz/last"]),
        ),
        // Normalised to a region that overlaps identically. `paths_overlap`
        // filters empty components, so this collides with the derived one
        // exactly as the derived one collides with itself — and it is still
        // not the region the frozen hints derive.
        (
            "a separator normalised away",
            key_of("doubled"),
            narrowed(&["src/doubled/inner"]),
        ),
        // Case-folded, under a run whose policy folds case. The policy
        // decides what *overlaps*; it does not decide what a region is.
        (
            "the case folded",
            key_of("unicode"),
            narrowed(&["src/über"]),
        ),
        // A bounded prediction recorded as unbounded.
        ("widened to repo-wide", key_of("literal"), PathSet::RepoWide),
        // And an unbounded prediction recorded as bounded, which is the
        // one that lets a task run beside work it should have blocked.
        (
            "narrowed from repo-wide",
            key_of("leading-glob"),
            narrowed(&["src/anywhere"]),
        ),
        // The empty region is a real answer and not the derived one.
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

/// The default plan's dispatch fixture records the region the fold derives.
///
/// `region` is a table and `predicted_region` is a rule, so the corpus held
/// two answers to one question and every `task_dispatched` in this file
/// depended on their agreeing. They did — for the default plan — and the
/// same table was wrong for [`chain_plan`], which is why [`dispatch_in`]
/// exists. This is the round trip that keeps the surviving table honest:
/// it is not what proves the door right, it is what stops a fixture edit
/// from silently making every other test in this file dispatch a region the
/// run never predicted.
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

    // An ordinary task may not inherit a lineage lease, and a repair may
    // not take one of its own; a repair names the candidate it was
    // materialized from, and an ordinary dispatch names none.
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

// -----------------------------------------------------------------------
// ST-06: a completion applies only while its identity is the open one
// -----------------------------------------------------------------------

#[test]
fn an_attempt_starts_in_the_open_generation_at_the_next_number() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));

    // The generation: not another task's, not a closed one, not one that
    // does not exist.
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

    // The number: dense from 1 within the generation, in both directions.
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

    // A second attempt starts only after the first settles, and then at 2.
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
    // refusals[12], over the three ways a resume can be wrong and the one
    // way it can be right.
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    // A settlement cannot retain a session for another incarnation.
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
    // Another session than the one retained.
    assert!(matches!(
        refuse(&fold, &resume_with(&fold, "sess-other")),
        FoldError::StaleIncarnation { key: 0, .. }
    ));
    // The right session, in the incarnation that retained it.
    accepts(&fold, &resume_with(&fold, "sess-ÜNI-0042"));

    // And the same event after a resume: the working tree was rolled back,
    // so the conversation's belief about what it left behind is false.
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

    // A fresh attempt in a retained generation is not a resume, and a
    // resume in a fresh generation is not a retry.
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
    // refusals[11] / INV-19, one component at a time. Each case moves one
    // field of an otherwise exact binding: a check that compared the whole
    // record, or that compared none of it, fails on the case it skipped.
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

    // The effort is the ladder's effort *for that rung's tier*, not the
    // run's default and not another tier's: zeta's rungs are small, mid and
    // frontier, resolving to three different efforts.
    for rung in 0..3u32 {
        accepts(&fold, &attempt_started(&fold, ZETA, 0, 1, rung));
        let entry = fold.registry().expect("started").get(ZETA).expect("zeta");
        let tier = entry.ladder.rungs[rung as usize].tier;
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
                generation: GenerationId(0),
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
    }

    // A rung the ladder does not have.
    let mut off_the_end = attempt_started(&fold, ZETA, 0, 1, 0);
    if let TopologyEventBody::AttemptStarted { data } = &mut off_the_end.body {
        data.rung = 9;
    }
    assert!(matches!(
        refuse(&fold, &off_the_end),
        FoldError::BindingMismatch { .. }
    ));

    // A repair's attempt records what its worktree was materialized from,
    // and an ordinary one records nothing.
    let mut materializing = attempt_started(&fold, ZETA, 0, 1, 0);
    if let TopologyEventBody::AttemptStarted { data } = &mut materializing.body {
        data.materialization_observed = Some(Materialization::Clean);
    }
    assert!(matches!(
        refuse(&fold, &materializing),
        FoldError::MalformedEntry { key: 0, .. }
    ));
}

#[test]
fn an_override_is_the_binding_the_frozen_admission_authorized_and_no_other() {
    // `task_registry.binding_override`: the override is "validated against
    // the frozen options of that task's open HumanBinding question", and
    // refusals[12] refuses one "for a wrong question ... or mismatched
    // fields". A1 proves the override names the same task, question and
    // option as the answer carrying it; the authority it is measured
    // against is the fold's, and it has to survive from the `task_spawned`
    // that froze it to the answer that draws on it.
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

    // Every option, named exactly, is authorized.
    for (index, agent) in options.iter().enumerate() {
        let index = u32::try_from(index).expect("two options");
        accepts(&fold, &answer(index, Some(override_for(index, agent))));
    }

    // An option the admission froze for somebody else. Both directions of
    // the pairing are wrong: the agent of the *other* option, and an agent
    // the option list never held at all. Neither is caught by the range
    // check or by A1's internal agreement, because both are self-consistent
    // and in range.
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

    // An answer to a HumanBinding admission with no override at all leaves
    // its task with an empty ladder and nothing to run: `Admission::
    // HumanBinding` says the entry "cannot move until an answer records an
    // explicit one-off binding", and `Answer4.binding_override` is
    // "present exactly when the question was asking for a binding".
    assert!(matches!(
        refuse(&fold, &answer(0, None)),
        FoldError::WrongQuestion { .. }
    ));

    // And the converse, which is the half nothing checked: an override on
    // a question that authorized no binding. The question here is an
    // ordinary park of another task, and the override is internally exact
    // — it names that question, that task and that option — so only the
    // admission authority distinguishes it.
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
    // The same answer without the override is the ordinary one.
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

    // A `HumanRequired` admission asks for a person, not for a binding.
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
    // transaction_fault_matrix[T-ATTEMPT].resume_action: "append
    // attempt_interrupted (unknown spend, allowance refunded, generation
    // Closed, lease by kind); discard residue ... the task worktree
    // scrubbed with force ... task returns Pending; later dispatch new
    // generation". Nothing was judged and the spend is unknown, so the
    // generation is over — not idled and not reusable.
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
    // "lease by kind", for a generation that closes: an ordinary one gives
    // up the region it predicted.
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

    // Generation 0 is over, so it is not closed again and not restarted;
    // the run continues by dispatching the *next* dense generation.
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

    // refusals[15], the coordinate that only matters once a *later*
    // generation is open: `generation_closed(0)` names generation 0, and
    // generation 1 is the open one. A close that took "whatever is open"
    // would close the newer generation under the older one's name, which
    // is a state no reader could recompute from the log.
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

    // A repair holds nothing of its own, so its interruption records
    // `LineageHeld` and its lineage lease is untouched.
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
    // The other half of refusals[11]: when a human named a binding, that is
    // the authority, and the frozen rung is no longer one.
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
    // The tier is not compared: an override chooses an agent from a frozen
    // option list, and the tier it lands on is whatever that agent is
    // bound at.
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
    // refusals[14], as a crossed grid: two kinds of holding, three events
    // (one that keeps the generation, two that end it), three dispositions.
    // Exactly one cell per (holding, fate) is accepted.
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
            // A terminal failure ends the generation.
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

            // An interruption *closes* the generation
            // (transaction_fault_matrix[T-ATTEMPT]: "generation Closed,
            // lease by kind"), so it records the same disposition a
            // terminal failure does — an ordinary generation releases its
            // predicted region, a lineage member goes on holding its
            // root's.
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

            // **No `attempt_finished` leaves a generation open, so there is
            // no surviving disposition to enumerate here any more.** This
            // block asserted that a `succeeded` settlement recording
            // `PredictedRetained` (ordinary) or `LineageHeld` (lineage) is
            // accepted — the one case where a settlement kept its region to
            // hand to a candidate. Since the 2026-08-27 CONFORM ruling that
            // event is refused whatever it records, because
            // `candidate_prepared` is the sole successful settlement.
            //
            // Re-derived rather than deleted: the claim becomes *refused
            // for every disposition*, which is stronger than the row it
            // replaces and fails if the transition is ever readmitted.
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

            // And the region a candidate inherits is decided on the event
            // that now settles the attempt: `check_candidate_prepared`
            // matches `CandidateLeaseEffect` against the entry's lineage.
            // `a_lineage_lease_only_ever_grows_and_a_released_one_is_gone`
            // holds that half.
        }
    }
}

#[test]
fn a_settlement_applies_only_to_the_attempt_that_is_running() {
    // refusals[16] / ST-06 for settlements, over each coordinate of the
    // identity in turn.
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let closed = || AttemptSettlement::Closed {
        transition: SettlementTransition::Retry,
        lease: LeaseDisposition::PredictedReleased,
    };
    // No attempt is running yet.
    assert!(matches!(
        refuse(&fold, &settle(ZETA, 0, 1, closed())),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    accepts(&fold, &settle(ZETA, 0, 1, closed()));
    // Another task, another generation, another attempt.
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
    // The same three, for an interruption.
    let interrupt = |key: TaskKey, generation: u32, attempt: u32| {
        ev(TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key,
                generation: GenerationId(generation),
                attempt: AttemptNumber(attempt),
                // T-ATTEMPT closes the generation, so an ordinary one
                // releases the region it predicted.
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
    // refusals[15], over every class a generation can be in.
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

    // OpenNoAttempt: closable.
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    accepts(
        &fold,
        &closed_event(ZETA, 0, LeaseDisposition::PredictedReleased),
    );

    // InFlight: not closable — the attempt is settled or interrupted first.
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);
    assert!(matches!(
        refuse(
            &fold,
            &closed_event(ZETA, 0, LeaseDisposition::PredictedReleased)
        ),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));

    // RetainedIdle: closable — this is how a resume discards a session it
    // may not resume.
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

    // Promoting: not closable — a promoting generation is promoted.
    //
    // **Reached by preparing a candidate, which is what promotes it.** This
    // cloned the in-flight fold and applied `succeeded(ZETA, 0, 1)`; since
    // the 2026-08-27 CONFORM ruling that event is refused, and a clone
    // alone would have left this case asserting about an *in-flight*
    // generation while calling itself the promoting one — the same
    // assertion passing for the wrong reason. `cargo` said so: the binding
    // stopped needing `mut`.
    let mut promoting = fold.clone();
    apply(&mut promoting, &candidate_prepared(ZETA, 0, &base));
    assert!(matches!(
        refuse(
            &promoting,
            &closed_event(ZETA, 0, LeaseDisposition::PredictedReleased)
        ),
        FoldError::NotTheOpenGeneration { key: 0, .. }
    ));

    // Closed: not closable twice.
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

// -----------------------------------------------------------------------
// Candidates, the queue, and the publication relations
// -----------------------------------------------------------------------

/// **A `candidate_prepared` whose record says the attempt failed is refused.**
///
/// The round-4 review of `09f9a99` set out the sequence exactly, and this is
/// it: a valid `run_started`, `task_dispatched` and `attempt_started`, then an
/// otherwise-consistent `candidate_prepared` whose embedded `AttemptRecord`
/// carries `failure: Some(GateFailed)`. Before this check the fold accepted
/// it, recorded the candidate, entered `Promoting`, and the task was carried
/// to `task_candidate_created` — **durably queued as a successful candidate
/// whose own authoritative evidence says a gate failed.**
///
/// The 2026-08-27 Class B change made this event the sole successful
/// settlement and enforced everything about it except the one thing that made
/// it *successful*. The fold is the authority against malformed, reconstructed
/// and faulty future writers, not just against this build's own driver, which
/// happens to supply a passing record.
///
/// It also earns the property `TopologyRun`'s brief already assumed: a
/// `candidate_prepared` record never carries feedback, because it never
/// carries a failure.
#[test]
fn a_candidate_prepared_whose_record_failed_is_refused() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    // The premise: with a passing record this exact event is accepted, so the
    // refusal below is about the failure and not about anything else in it.
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

    // And nothing moved: a refused transition changes nothing, so the
    // generation is still in flight and has no candidate.
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

/// **A review outcome is authoritative, and both are.**
///
/// [`a_candidate_prepared_whose_record_failed_is_refused`] covers the
/// failure field. This covers the other half of the same predicate: a record
/// carrying no failure at all, whose reviews say `Failed` or `Unavailable`.
///
/// §11.2 requires *every* configured pass to pass, and a reviewer that could
/// not run "says nothing about the code" — which is not approval. Before
/// `AttemptRecord::is_successful` existed this door read `failure.is_none()`
/// alone, so a record whose primary reviewer returned `Failed` was promoted,
/// charged against the rung allowance and queued as a candidate. The
/// `b1f54a5` review walked that sequence.
#[test]
fn a_candidate_prepared_whose_review_did_not_pass_is_refused() {
    for outcome in [ReviewPassOutcome::Failed, ReviewPassOutcome::Unavailable] {
        let base = sha("base");
        let mut fold = started();
        apply(&mut fold, &dispatch(ZETA, 0, &base));
        let start = attempt_started(&fold, ZETA, 0, 1, 0);
        apply(&mut fold, &start);

        // The premise: the same event with the pass *passed* is accepted, so
        // the refusal below is about the outcome and nothing else.
        accepts(&fold, &candidate_prepared(ZETA, 0, &base));

        let mut judged = candidate_prepared(ZETA, 0, &base);
        let TopologyEventBody::CandidatePrepared { data } = &mut judged.body else {
            unreachable!("built as a candidate_prepared")
        };
        // The failure field stays empty on purpose: this is the shape the
        // old `failure.is_none()` door called successful.
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

        // Nothing moved.
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

// --- the frozen review plan is the success domain ----------------------
//
// `PR7-G2-W1-SUCCESS-IGNORES-THE-FROZEN-PLAN` (§2, §22e). The witnesses
// above are round 6's *outcome* half — a configured pass that ran and did
// not pass. These are the *presence* half: a pass the plan configured and
// the record does not carry at all.

/// A run whose frozen plan obliges **nothing**: verification off.
///
/// `plan_for`'s disabled branch resolves no `primary` either, so this is
/// the shape production writes and not merely `enabled = false` bolted onto
/// a resolved reviewer.
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

/// One row of the obligation grid: a label, the fold whose frozen plan is
/// under test, the task, the passes that plan obliges, and the labelled
/// lists it refuses.
type ObligationRow = (
    &'static str,
    TopologyFold,
    TaskKey,
    Vec<(&'static str, ReviewPassOutcome)>,
    Vec<(&'static str, Vec<(&'static str, ReviewPassOutcome)>)>,
);

/// A `candidate_prepared` for `key` carrying exactly `passes`, all passed.
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

/// A fold with `key`'s first attempt in flight, ready for its settlement.
fn in_flight_at(fold: &mut TopologyFold, key: TaskKey, base: &CommitSha) {
    apply(fold, &dispatch(key, 0, base));
    let start = attempt_started(fold, key, 0, 1, 0);
    apply(fold, &start);
}

/// **Zero, one and many configured passes: the record carries the frozen
/// obligation and nothing else.**
///
/// The arity grid, because the defect is about the *domain* of a predicate
/// and a one-pass fixture cannot show a domain. `review_plan` configures a
/// second opinion for index 2 alone, so `MID` obliges two passes and `ZETA`
/// one; a run that froze verification off obliges none.
///
/// Each row asserts both directions: the obliged list is accepted, and the
/// same event carrying any other list is refused. The negative half is what
/// makes the positive half a measurement — `is_successful` was true of every
/// one of these records, which is exactly why it could not tell them apart.
#[test]
fn candidate_success_is_judged_against_the_tasks_frozen_review_plan() {
    let base = sha("base");
    const REVIEW: &str = "review";
    const SECOND: &str = "second-opinion";
    let pass = |name: &'static str| (name, ReviewPassOutcome::Passed);

    // (label, the fold's frozen plan, the task, what it obliges, what it refuses)
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
                // The finding's own shape: a lone passed second opinion.
                // Every entry green, and the pass §11.2 requires absent.
                ("a lone second opinion", vec![pass(SECOND)]),
                // An empty list: `all` over nothing is true.
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

        // The premise: the obliged list is what the door takes.
        accepts(&fold, &prepared_with_passes(key, &base, &obliged));

        for (why, passes) in refusals {
            let error = refuse(&fold, &prepared_with_passes(key, &base, &passes));
            assert!(
                matches!(error, FoldError::InconsistentRecord { .. }),
                "{label}/{why}: refused as {error:?} rather than as a record disagreement"
            );
            // Nothing moved: the generation is still in flight and holds no
            // candidate, so a refusal cannot have charged the rung.
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

/// **A run that froze verification off obliges no pass, whatever it
/// resolved.**
///
/// The `enabled` flag and the resolved bindings are independent fields, and
/// `plan_for`'s disabled branch happens to leave `primary` unset — so a
/// grid built only from what that function produces cannot tell the flag
/// from the absence of a reviewer. The fold reads **logs**, and
/// `enabled: false` beside a resolved primary and a resolved second opinion
/// is a shape the wire admits: `run_started(4).reviews` carries both, and a
/// `task_spawned` embeds a whole frozen entry.
///
/// Three of this file's fixtures froze exactly that combination while their
/// records carried a passed `review`, which is what makes this the shape
/// worth pinning rather than a hypothetical: read one way it obliges a pass
/// nobody ran, read the other it obliges none.
#[test]
fn a_run_that_froze_verification_off_obliges_no_pass_whatever_it_resolved() {
    let base = sha("base");
    let plan = plan();
    // Resolved reviewers, and the switch off. The second opinion is
    // resolved for `MID` too, so the "many" arm is off as well as the
    // "one" arm.
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

        // The premise: the reviewers *are* resolved on this entry, so an
        // obligation derived from the bindings alone would not be empty.
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

/// **The obligation is the plan's, read through the plan's own reader.**
///
/// The round trip: whatever `FrozenReviews::obliged_lenses` says a task
/// owes is exactly what the door accepts, and the door accepts nothing
/// else. It is deliberately *not* how
/// [`candidate_success_is_judged_against_the_tasks_frozen_review_plan`]
/// is written — that grid transcribes the obligation by hand, so the two
/// together say both "the fold agrees with the reader" and "the reader says
/// what §11.2 says".
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
        // And one fewer is refused, whichever pass is dropped.
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

/// **A failed settlement whose record says the attempt succeeded is refused.**
///
/// The mirror of the candidate door, through the same predicate. This door
/// refused `Succeeded` and asked nothing further, so an `attempt_finished`
/// could fail a task — halting the run — while carrying a ledger line whose
/// failure field is empty and whose every review passed. That line is what a
/// person reads when deciding whether to trust a run.
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

    // The premise: with a record that says failed, this exact settlement
    // applies.
    accepts(&fold, &settle(ZETA, 0, 1, closed()));

    let mut lying = settle(ZETA, 0, 1, closed());
    let TopologyEventBody::AttemptFinished { data } = &mut lying.body else {
        unreachable!("built as an attempt_finished")
    };
    // Exactly the successful shape: no failure, every pass passed.
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

/// **The envelope and the record name one attempt.**
///
/// Without this the ledger line a settlement carries can belong to a
/// different attempt of the same generation — attempt 2's cost, duration and
/// model recorded against attempt 1's settlement, with every derived total
/// reading it as authoritative.
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

/// **A successful attempt spends one of its rung's allowance, and the count
/// survives replay.**
///
/// `spends_allowance(None)` is `true` — the worker ran and its work was judged
/// and accepted — so a success charges the rung exactly as a judged failure
/// does. That was true while `attempt_finished{Succeeded}` was the settlement,
/// and it stopped being true on 2026-08-27 when the settlement moved to
/// `candidate_prepared` and the increment stayed behind in `apply_settlement`.
///
/// Nothing noticed. The suite was green, the allowance census went on finding
/// its one write site, and the replacement witness asserted `Promoting` and
/// candidate presence — none of which is the allowance. A **first-attempt
/// success left `attempts_on_rung` at zero**, so a later reader could grant an
/// extra attempt on a rung already paid for. The round-4 review of `09f9a99`
/// found it.
///
/// Both positions are driven, because they fail differently: a first-attempt
/// success is the count going 0 → 1 with nothing before it, and a
/// second-attempt success is the successful charge landing *on top of* a
/// failure's. And the live count is compared against a replay of the same log,
/// because a fold that counts live and not on replay is the divergence this
/// project measures everything else against.
#[test]
fn a_successful_attempt_charges_its_rung_live_and_on_replay() {
    let base = sha("base");

    for (label, failures_first) in [("first-attempt success", 0), ("second-attempt success", 1)] {
        let mut live = started();
        let mut trace = vec![run_started_event()];

        // Optionally a judged failure first, which retries into a new
        // generation — the shape a second-attempt success actually has.
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

        // And a replay of exactly those bytes reaches the same number.
        let replayed = TopologyFold::replay(inputs(), &trace).expect("the trace replays");
        assert_eq!(
            replayed.task(ZETA).map(|task| task.attempts_on_rung),
            Some(expected),
            "{label}: the live fold counted {expected} and a replay of its own log did \
                 not — one fold, not two"
        );
    }
}

/// **A candidate is prepared by the generation whose attempt is in flight,
/// and preparing it is what settles that attempt.**
///
/// Re-derived, not adjusted. This was
/// `a_candidate_is_prepared_by_the_generation_whose_attempt_succeeded`, and it
/// asserted the opposite of the first claim below: that `candidate_prepared`
/// is **refused** while the attempt is still running, and accepted only after
/// an `attempt_finished{Succeeded}` had promoted the generation. That is the
/// dual-settlement pattern `decisions/2026-08-12-merge-queue-execution-topology.md`
/// forbids — "`attempt_finished` is not also emitted for that attempt" — and the
/// fold was *requiring* it. Ruled CONFORM 2026-08-27.
///
/// The other three claims are unchanged and still ST-06's: not another
/// generation's, not another task's, and parented on the base the generation
/// was dispatched at.
#[test]
fn a_candidate_is_prepared_by_the_generation_whose_attempt_is_in_flight() {
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    // The attempt is running, and this event settles it.
    accepts(&fold, &candidate_prepared(ZETA, 0, &base));

    // ST-06: not another generation's, and not another task's.
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

    // The commit is parented on the base the work started from, and that
    // base is the one the generation was dispatched at. INV-09's
    // exact-base decision compares the head against `base_sha` and then
    // publishes `commit_sha`, so both claims have to hold.
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

    // The region it takes is the region its diff touched.
    let mut inconsistent_region = candidate_prepared(ZETA, 0, &base);
    if let TopologyEventBody::CandidatePrepared { data } = &mut inconsistent_region.body {
        data.lease_effect = CandidateLeaseEffect::ReplacesPredicted { paths: region(MID) };
    }
    assert!(matches!(
        refuse(&fold, &inconsistent_region),
        FoldError::InconsistentRecord { .. }
    ));

    // An ordinary candidate replaces its predicted region; only a lineage
    // member widens a lineage.
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

    // ST-06's "wrong attempt number", for the record the candidate
    // carries. The generation ran attempt 1, so 0, 2 and 9 all name an
    // attempt that did not produce this commit. Without this the embedded
    // record is inert data and a candidate can be published attributed to
    // an attempt that failed.
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

    // Preparing takes the actual region and gives up the predicted one.
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

    // INV-06: "at most one candidate per generation", enforced_by "fold
    // refuses a second candidate for a generation". The second record is
    // valid in isolation — it is the *same* event that was just accepted,
    // and so is a differing one — and it is refused because the generation
    // has already prepared.
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
    // And the first candidate is still the one the generation holds, so a
    // promotion of the second has nothing to promote.
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
    // ST-06 for `candidate_prepared`, through the durable path as well as
    // the live one: the generation retried, so attempt 2 is the authority
    // and the number the earlier attempt carried is no longer one.
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

    // Attempt 1 ran and did not produce this candidate; attempt 2 did.
    assert!(matches!(
        refuse(&live, &candidate_prepared_at(ALPHA, 0, 1, &base)),
        FoldError::WrongAttempt { .. }
    ));
    accepts(&live, &candidate_prepared_at(ALPHA, 0, 2, &base));

    // The same pair over the wire: a log whose candidate names attempt 1
    // stops at that line, and the authoritative one replays.
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
    // ST-06's "a mismatched task_candidate_created", over every coordinate
    // of the reference: a promotion that named another commit would give
    // the queue a position pointing at an object nothing judged.
    let base = sha("base");
    let mut fold = started();
    apply(&mut fold, &dispatch(ZETA, 0, &base));
    let start = attempt_started(&fold, ZETA, 0, 1, 0);
    apply(&mut fold, &start);

    // Before anything was prepared.
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

    // Promotion ends the generation and takes the queue position.
    apply(&mut fold, &candidate_created(ZETA, 0));
    assert_eq!(fold.queue().expect("started").len(), 1);
    assert_eq!(
        fold.task(ZETA).expect("zeta").generations[0].class,
        GenerationClass::Closed
    );
}

/// Two candidates queued in an order the fixture chose, so "first" is a
/// position rather than a coincidence.
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
    // refusals[8]. The queue is FIFO by promotion order and the *first
    // eligible* entry is integrated, which is not the same as the first
    // one: three of the four ineligibility rules move the answer past the
    // head of the queue, and the fourth is the head itself being fine.
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

    // A candidate holding no position at all.
    assert!(matches!(
        refuse(&fold, &verification_started(ALPHA, 0, 0, &head, &proposal)),
        FoldError::NotFirstEligible { key: 1, .. }
    ));

    // Its task parked: the entry keeps its place and the next eligible one
    // is integrated instead.
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
    // refusals[6], [7] and the sequence half of [10].
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

    // A second transaction while one is unresolved.
    assert_eq!(
        refuse(&fold, &verification_started(ZETA, 0, 1, &head, &proposal)),
        FoldError::TransactionAlreadyOpen {
            kind: "merge_verification_started",
            sequence: 1,
            open: 0,
        }
    );

    // An event that names a sequence other than the open one.
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

    // Resolving one consumes its number: the next transaction is 1.
    apply(&mut fold, &unavailable(0));
    assert!(matches!(
        refuse(&fold, &verification_started(ZETA, 0, 0, &head, &proposal)),
        FoldError::NonDenseSequence { next: 1, .. }
    ));
    accepts(&fold, &verification_started(ZETA, 0, 1, &head, &proposal));

    // And an event that belongs to no transaction at all.
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
    // INV-09: the exact-base decision is made from the head before any
    // staging effect, so a candidate whose base *is* the head is published
    // fast and is never cherry-picked or re-verified.
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let fold = two_queued();
    assert!(matches!(
        refuse(&fold, &verification_started(MID, 0, 0, &base, &proposal)),
        FoldError::InconsistentRecord { .. }
    ));
    accepts(&fold, &verification_started(MID, 0, 0, &head, &proposal));

    // A stale-clean verification judges the proposal the cherry-pick
    // produced; an already-present one judges the head itself. Each refuses
    // the other's shape.
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
    // refusals[9] and the fold half of refusals[22], as relations rather
    // than examples: for each disposition, the accepted publication and
    // every single-field departure from it. A lookup table keyed on these
    // inputs would have to hold every row of this grid, and the rows are
    // generated from the same fixture the accepted case is.
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");

    // --- fast: the head is exactly the candidate's base -----------------
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

    // --- stale_clean: the pinned proposal, at the head that was read ----
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

    // --- already_present: the head is what was verified -----------------
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

    // --- the dispositions do not stand in for one another ---------------
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
    // And a verified publication cannot open its own transaction, nor a
    // fast one join somebody else's.
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
    // refusals[8]: a publication's relations are against "the candidate's
    // recorded base_sha" and "the candidate's recorded commit_sha" — the
    // record `candidate_prepared` left and the queue entry
    // `task_candidate_created` took, not a copy the event brought with it.
    //
    // The disposition grid moves one field of the *event* and leaves the
    // record alone, so an event that disagrees with itself is what it
    // catches. What it cannot catch is a forgery: an embedded CandidateRef
    // that is internally exact and agrees with every intra-event relation
    // A1 checks, and simply names something durable history never
    // recorded. Each case below moves exactly one coordinate of that
    // identity away from history while keeping the event self-consistent,
    // so a fold that matched on the remaining coordinates accepts it.
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let recorded = candidate_of(MID, 0);

    // --- fast ----------------------------------------------------------
    // A1 pins proposed_sha == candidate_sha for a fast publication, so the
    // one coordinate a forger is free to move is the ref.
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

    // --- stale_clean and already_present --------------------------------
    // Here the proposal is the pinned one rather than the candidate's
    // commit, so `candidate_sha` is free too: both coordinates of the
    // cross-record identity can be forged one at a time.
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
        // The unforged shape is authorized, so the refusals below are
        // about the forged coordinate and about nothing else.
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
            // Self-consistent: A1 has nothing to say about it.
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

    // --- and the same, through the durable path -------------------------
    // A forged publication in a log must stop the replay at its own line,
    // not be applied and then contradicted later.
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

/// The same SHA with its last character moved: a value that differs from
/// the original in one position out of forty and agrees on every prefix
/// shorter than the whole.
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
    // refusals[8] names four SHA relations, and every one of them is
    // equality of a commit identity. A comparison that truncated, folded
    // case, or matched a prefix would still reject the grid's cases, which
    // move a SHA to an unrelated value. These move one character of forty,
    // at the end, so a comparison of anything less than the whole accepts
    // them.
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
    // refusals[8] for the two coordinates that identify *which* verification
    // authorized a publication: the source's sequence, and the candidate
    // the open transaction is verifying. Any `Verification` source and any
    // open transaction are the right ones as long as only one exists, so
    // both need a state where more than one identity is available.
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

    // Sequence 0 ran and was interrupted; sequence 1 is the open one. Both
    // are `Verification` sources, so the variant alone no longer decides.
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

    // The open transaction is verifying mid; a publication of zeta copies
    // its head, proposal, pin and source and is refused because the
    // transaction is not about zeta.
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
    // refusals[8]: "merge_prepared(already_present) whose proposed_sha
    // differs from expected_head **or from the verified head**". The two
    // are separate relations, and a self-consistent event satisfies the
    // first while contradicting the second: H2/H2 agrees with itself and
    // names a head no verification of this sequence ever read.
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
    // refusals[7], and the class it is easiest to lose: a fast
    // `merge_prepared` opens a transaction that stays unresolved until
    // `task_merged`. "An authorized publication is always completed
    // (recovery or run-end closure), never abandoned" (INV-09), so the
    // next start waits for it.
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

    // Once the ref has moved and the merge is recorded, the next one may
    // start — at the adjacent sequence.
    apply(&mut fold, &merged(MID, 0, 0, vec![MID]));
    assert!(fold.transaction().is_none());
    accepts(&fold, &verification_started(ZETA, 0, 1, &head, &proposal));
}

#[test]
fn the_queue_is_ordered_by_creation_and_not_by_preparation() {
    // `coordinator_integration.queue`: "FIFO by **task_candidate_created**
    // append order". Preparation and creation are separate events and a
    // fixture that always pairs them cannot tell which clock the order
    // came from. Here they are deliberately crossed: mid prepares first
    // and zeta is created first, so the two clocks disagree and only one
    // of them produces the queue the packet describes.
    let base = sha("base");
    let mut fold = started();
    for (key, generation) in [(MID, 0), (ZETA, 0)] {
        apply(&mut fold, &dispatch(key, generation, &base));
        let start = attempt_started(&fold, key, generation, 1, 0);
        apply(&mut fold, &start);
        apply(&mut fold, &candidate_prepared(key, generation, &base));
    }
    // Prepared mid, then zeta. Created zeta, then mid.
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
    // And the first *eligible* entry is the one an integration may start
    // for, which is the same statement read through the refusal.
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
    // refusals[10]: "non-dense keys, generations". The tested direction has
    // always been the gap above; the direction nothing reached is the one
    // below, where a duplicate or earlier key would re-register a task or
    // re-open a generation that is over.
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

    // Generations are dense per task, and alpha's generation 0 is over.
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
    // `defer_wait_elapsed` is a run-level event, not a per-item one: the
    // closure procedure's step (5b) and `coordinator_integration.queue`
    // both describe deferral as a flag cleared "until the next
    // defer_wait_elapsed or run_resumed", with no notion of which waiter it
    // is about. A wake that cleared the first of each kind is
    // indistinguishable from one that cleared all of them unless more than
    // one of each is waiting.
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = started();

    // Two tasks deferred by their settlements.
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

    // And a candidate deferred by an outage.
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

    // The count survives the wake, so the next deferral is the next
    // consecutive one rather than a restart.
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
    // refusals[10]'s "invalid satisfies", over a lineage deep enough that
    // the closure is neither the candidate alone nor the whole registry.
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

    // A repair carries the work of everything it descends from, so
    // publishing it settles the whole chain back to the root.
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
    // The ref moves only after a publication was authorized.
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

    // A different commit than the one authorized.
    let mut elsewhere = merged(MID, 0, 0, vec![MID]);
    if let TopologyEventBody::TaskMerged { data } = &mut elsewhere.body {
        data.merged_sha = sha("smuggled");
    }
    assert!(matches!(
        refuse(&fast, &elsewhere),
        FoldError::InconsistentRecord { .. }
    ));
    // A closure that is not the authorization's — as a *vector*, so a
    // duplicated or emptied list is as wrong as a widened one and a
    // set-shaped comparison is not enough.
    for wrong in [vec![MID, ZETA], vec![MID, MID], Vec::new(), vec![ZETA]] {
        assert!(
            matches!(
                refuse(&fast, &merged(MID, 0, 0, wrong.clone())),
                FoldError::InvalidSatisfies { .. }
            ),
            "a merge settling {wrong:?} was copied from an authorization of [MID]"
        );
    }
    // A lease release that is not the one this publication owes.
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

    // Merging settles the closure, frees the position and the region.
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

// -----------------------------------------------------------------------
// Outages, rejections and lineage
// -----------------------------------------------------------------------

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
    // refusals[16] and `coordinator_integration.dispositions`, as the
    // partition they are: an Infrastructure outage defers "while defers <
    // the frozen max_defers" and parks "at max_defers". The run froze
    // max_defers = 2, so exactly one deferral is available and the second
    // outage parks. Both arms are crossed against every count, so a fold
    // that moved the boundary either way is caught in one direction or the
    // other.
    //
    // The allowance is read from the frozen record and the expected
    // verdicts are computed from the packet's inequality, not from the
    // function under test.
    let head = sha("head");
    let proposal = sha("proposal");
    let mut fold = two_queued();
    let max = fold.started().expect("started").limits.max_defers;
    assert_eq!(max, 2, "the fixture's allowance is what this test is about");

    // Count 0 -> the run may still defer, and may not yet park.
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

    // Count 1 -> the next deferral would be the max_defers'th, so the
    // allowance is spent: the outage parks and may not defer at all. This
    // is the cell `defers > max_defers` accepted and `defers >= max_defers`
    // refuses.
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

    // The count is this candidate's own history, not the run's. The second
    // queued candidate has deferred nothing, so its own first deferral is
    // still 1 while MID sits at 1 — a fold that summed the queue would
    // demand 2 here and refuse the count the packet requires.
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

    // A human finding cannot be waited out.
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
    // A park that offers nothing to answer with.
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
    // A park whose question is about somebody else.
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

    // Parking moves the task to awaiting input, and its answer returns it
    // to awaiting merge to be re-verified under a new sequence.
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
    // The repair's dependency has to be merged, and `alpha` is not yet.
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

    // Applying it: the candidate leaves the queue, the task awaits its
    // repair, and the lineage holds the region.
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
    // A conflict is decided at the cherry-pick, before any verification
    // starts, so it is the first append of its sequence rather than a
    // terminal of somebody else's.
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

    // The lineage holds the candidate's region *and* the conflict's.
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

// -----------------------------------------------------------------------
// Questions, budget, and the end of a run
// -----------------------------------------------------------------------

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
    // refusals[13]. A1's half — the override must name the same question,
    // task and option as the answer carrying it — is wired in; this adds
    // the three the fold owns.
    let mut fold = started();
    apply(&mut fold, &raised("q-park-Ünicode", ZETA));

    // A question this log never asked.
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
    // The right question, about another task.
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
    // An option it did not offer: the fixture's question has three.
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

    // An override that disagrees with the answer carrying it.
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

    // Answered once: the second answer has no open question to name.
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
    // And its id is never reused for a new question either.
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

/// ALPHA with generation 0 open in each class a generation can be open in,
/// as the log that puts it there — the fewest events that reach the class,
/// after `run_started`.
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

/// Fold `events` after `run_started`, live, and return the fold and the
/// whole log including the `run_started`.
fn folded(events: &[TopologyEvent]) -> (TopologyFold, Vec<TopologyEvent>) {
    let mut fold = started();
    let mut log = vec![run_started_event()];
    for event in events {
        apply(&mut fold, event);
        log.push(event.clone());
    }
    (fold, log)
}

/// `event` is refused live, the refusal leaves the state alone, and a replay
/// of the same log plus `event` stops with the same refusal (INV-02).
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
    // **What the pre-repair fold answered for every one of these inputs:
    // accepted.** The bare `question_raised` set the task `AwaitingInput`
    // and left the generation exactly as it was, and the decline that
    // answered it then set the task `Failed` with generation 0 still open
    // and its predicted lease still held, from which `derived_outcome` read
    // `NotEnding` for the rest of the log. That log is the one in
    // `the_question_an_attempt_raises_rides_on_its_settlement_and_a_decline_then_ends_the_run`,
    // where the same history is written the way the fold can end it.
    //
    // **What this asserts, and what it does not.** It asserts the rule the
    // door states — a bare question parks a task at rest, and an open
    // generation in any class is not at rest — and that the refusal is the
    // same live and on replay. It does not claim a decline is unrecoverable
    // from every class: `generation_closed` can close `OpenNoAttempt` and
    // `RetainedIdle` after a decline. What those two classes share with
    // `InFlight` and `Promoting` is that `attempt_started` asks nothing about
    // the task's state, so an attempt can start under the open question and
    // the decline then lands on `InFlight`; the constructed states in
    // `an_answer_applies_only_to_a_task_still_parked_with_nothing_open` are
    // what that ordering produces, and show the answer refused there.
    for (class, events) in alpha_open_in_every_class() {
        let (fold, log) = folded(&events);
        let error = refused_live_and_on_replay(&fold, &log, &raised("q-park-Ünicode", ALPHA));
        assert_eq!(
            error,
            FoldError::GenerationOpen {
                kind: "question_raised",
                key: 1,
                generation: 0,
                class,
            },
            "a generation that is {class} is an open generation"
        );
        // The refusal is about *this* task's generation. A question about a
        // task with no generation is still asked, and the id it did not
        // consume is still free.
        let mut parked = fold.clone();
        apply(&mut parked, &raised("q-park-Ünicode", MID));
        assert_eq!(parked.task_state(MID), Some(TaskState::AwaitingInput));
    }
}

#[test]
fn the_question_an_attempt_raises_rides_on_its_settlement_and_a_decline_then_ends_the_run() {
    // The history the refused log was trying to record — an attempt of ALPHA
    // ran, asked, and a person declined — written as the fold accepts it: the
    // question is the attempt's parking settlement, which closes the
    // generation and releases its region on the same event, so the decline
    // finds nothing open and the run can end.
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
        FoldError::GenerationOpen { .. }
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
    // The three facts the wedge inverted, in the order it inverted them:
    // the decision is kept, nothing is open, and the run ends.
    assert_eq!(fold.task_state(ALPHA), Some(TaskState::Failed));
    assert_eq!(fold.halted_at(), Some(ALPHA));
    assert!(fold.task(ALPHA).is_some_and(|task| task.open().is_none()));
    assert!(fold.leases().is_some_and(|leases| !leases.holds(lease)));
    // Bounded: `derived_outcome` is a pure function of the state, so the
    // wrong answer here is a value, never a wait.
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Halted)
    );
    accepts(&fold, &run_finished(RunOutcome::Halted, Some(ALPHA)));
}

#[test]
fn a_bare_question_is_refused_on_a_task_that_is_not_at_rest() {
    // **Pre-repair: accepted** against a merged or a failed task, and an
    // answer then returned it to `Pending`, where `ready` would dispatch it
    // again. The three states below are the ones another event would move
    // before the answer arrived; each is named with the event that moves it.
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
    // `AwaitingInput`: a second question while one is open. The pass-1
    // review of `15c37e4` reached the wedge through this door — q1 and q2
    // raised, q1 answered (`Pending`), the task dispatched and its attempt
    // started, q2 declined onto the in-flight generation.
    let parked_log = vec![raised("q-first-Ünicode", ALPHA)];
    // `AwaitingRepair`: its lineage's `task_merged` moves it to `Merged`.
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
    for (state, events) in [
        ("merged", merged_log),
        ("failed", failed_log),
        ("awaiting input", parked_log),
        ("awaiting repair", repair_log),
    ] {
        let (fold, log) = folded(&events);
        assert_eq!(fold.task_state(ALPHA).map(TaskState::name), Some(state));
        let error = refused_live_and_on_replay(&fold, &log, &raised("q-second-Ünicode", ALPHA));
        assert_eq!(
            error,
            FoldError::WrongTaskState {
                kind: "question_raised",
                key: 1,
                state,
                expected: "pending, awaiting merge or deferred",
            }
        );
    }
}

#[test]
fn a_bare_question_is_refused_on_the_candidate_under_integration() {
    // **Pre-repair: accepted.** The pass-1 review of `15c37e4` drove it on:
    // `merge_prepared` and `task_merged` continue the recorded transaction
    // without asking the task's state, so the task went `Merged` with the
    // question open, and the answer returned it to `Pending`, where a fresh
    // generation was dispatched for merged work. `first_eligible` skips a
    // parked task's candidate at the *start* of a transaction; this is the
    // same rule for one already open.
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
    // The same task, once the transaction is over and the candidate is back
    // in the queue, can be parked — this is the shape `select`'s tests use.
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
fn an_answer_applies_only_to_a_task_still_parked_with_nothing_open() {
    // The other end of the same rule, and the invariant the raise-time doors
    // exist to keep: whatever the log did between a question and its
    // answer, the answer is applied only to a task that is still
    // `AwaitingInput` with no open generation, because that is the state
    // `apply_answer`'s two effects are written against.
    //
    // **Constructed, not folded, and therefore live-only.** With the
    // raise-time doors shut no legal log reaches these states, which is the
    // point; the states are the ones the pass-1 sequences produced at
    // `15c37e4` — an attempt started under an open question, and a task
    // merged under one — built by hand the way the derived-outcome grid
    // builds its shapes. A state no log reaches cannot be replayed, so this
    // test asserts the live refusal alone; the replay half of the invariant
    // is carried by the three log-driven tests above through
    // `refused_live_and_on_replay`.
    let generation = |class: GenerationClass| GenerationFold {
        id: GenerationId(0),
        class,
        base_sha: sha("base"),
        lease: GenerationLease::Own,
        attempts: 1,
        candidate: None,
    };
    let answers = [
        Answer4::Answered {
            option_index: 0,
            binding_override: None,
        },
        Answer4::Declined {
            decline_halts_run: true,
        },
        Answer4::Declined {
            decline_halts_run: false,
        },
    ];

    // Parked, and an attempt then started in a generation it holds.
    for (class, name) in [
        (GenerationClass::OpenNoAttempt, "open with no attempt"),
        (
            GenerationClass::InFlight {
                attempt: AttemptNumber(1),
            },
            "in flight",
        ),
        (
            GenerationClass::RetainedIdle {
                session: SessionId("sess-ÜNI-0042".to_owned()),
                incarnation: Epoch(0),
            },
            "retained idle",
        ),
        (GenerationClass::Promoting, "promoting"),
    ] {
        let mut fold = started();
        apply(&mut fold, &raised("q-park-Ünicode", ALPHA));
        fold.run
            .as_mut()
            .and_then(|run| run.tasks.get_mut(ALPHA.index()))
            .expect("alpha is registered")
            .generations
            .push(generation(class));
        for answer in &answers {
            assert_eq!(
                refuse(&fold, &answered(ALPHA, "q-park-Ünicode", answer.clone())),
                FoldError::GenerationOpen {
                    kind: "question_answered",
                    key: 1,
                    generation: 0,
                    class: name,
                },
                "an answer under a generation that is {name}"
            );
        }
    }

    // Parked, and the task then moved by something other than its answer.
    for state in [
        TaskState::Pending,
        TaskState::AwaitingMerge,
        TaskState::AwaitingRepair,
        TaskState::Deferred,
        TaskState::Merged,
        TaskState::Failed,
    ] {
        let mut fold = started();
        apply(&mut fold, &raised("q-park-Ünicode", ALPHA));
        fold.run
            .as_mut()
            .and_then(|run| run.tasks.get_mut(ALPHA.index()))
            .expect("alpha is registered")
            .state = state;
        for answer in &answers {
            assert_eq!(
                refuse(&fold, &answered(ALPHA, "q-park-Ünicode", answer.clone())),
                FoldError::WrongTaskState {
                    kind: "question_answered",
                    key: 1,
                    state: state.name(),
                    expected: "awaiting input",
                },
                "an answer to a task that is {}",
                state.name()
            );
        }
    }

    // And the state every legal question leaves, answered.
    let mut fold = started();
    apply(&mut fold, &raised("q-park-Ünicode", ALPHA));
    accepts(
        &fold,
        &answered(
            ALPHA,
            "q-park-Ünicode",
            Answer4::Declined {
                decline_halts_run: false,
            },
        ),
    );
}

#[test]
fn a_task_whose_candidate_is_queued_is_not_dispatched_again() {
    // **Pre-repair: accepted.** The pass-2 review of `784449e` drove it: a
    // bare question on a queued task, answered, returned the task to
    // `Pending` with its candidate still queued (the return state is
    // `PR153-FOLD-ANSWER-RETURNS-TO-PENDING`); `check_dispatched` asked the
    // state and the open generation and nothing about the queue, so a second
    // generation was dispatched, the queued candidate merged under it, and
    // `attempt_interrupted` returned the merged task to `Pending`, from which
    // a third generation ran the merged work again. `ready` has always
    // carried `!queue.holds_task(key)`; this is the door asking the same
    // question, live and on replay.
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
    // The state the answer leaves — and the queue position it does not touch.
    assert_eq!(fold.task_state(ALPHA), Some(TaskState::Pending));
    assert!(fold.queue().is_some_and(|queue| queue.holds_task(ALPHA)));
    assert!(
        !fold.ready(ALPHA),
        "`ready` has always refused a queued task"
    );
    let error = refused_live_and_on_replay(&fold, &log, &dispatch(ALPHA, 1, &base));
    let FoldError::InconsistentRecord { kind, detail } = error else {
        panic!("a dispatch of a queued task is refused as one: {error}");
    };
    assert_eq!(kind, "task_dispatched");
    assert!(detail.contains("generation 0"), "{detail}");

    // The candidate keeps its place and integrates; the task ends `Merged`
    // and, being terminal, is refused a dispatch on that ground instead.
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
    // refusals[20], and the epoch scope that makes a resume the way back:
    // a budget-stopped run ingests the answer after its resume, and a
    // halted one never does, because `halted_at` is never cleared.
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
    // A halt is epoch-scoped for ingestion and permanent for the outcome:
    // the answer file stays on disk, and a resumed halted run still
    // derives Halted.
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
    // A resume starts a new epoch without one, and the next breach belongs
    // to that epoch rather than the old one.
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
    // refusals[18]: halt and budget outrank backoff.
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
    // Cleared by the resume that raises the ceiling.
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

    // And what it does when it is allowed: wakes every deferred task and
    // every verification-deferred candidate at once.
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

// -----------------------------------------------------------------------
// The derived outcome (INV-15, refusals[19])
// -----------------------------------------------------------------------

/// What is holding the run open, if anything.
///
/// Every open generation class and both transaction classes, because
/// `common` is the claim that *none* of them is outstanding: a fold that
/// counted only the ones somebody remembered would end a run holding a
/// retained session or an authorized publication.
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

/// Every value of [`Blocker`], so the grid crosses the whole dimension.
const BLOCKERS: [Blocker; 7] = [
    Blocker::Nothing,
    Blocker::OpenNoAttempt,
    Blocker::OpenGeneration,
    Blocker::Promoting,
    Blocker::RetainedIdle,
    Blocker::Transaction,
    Blocker::VerifyingTransaction,
];

/// Whether a budget stop exists, and whether it belongs to this epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Budget {
    None,
    Older,
    Current,
}

/// What is backing off, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backoff {
    None,
    DeferredTask,
    DeferredCandidate,
}

/// The shape of the task set. Chosen so that "some task could still be
/// admitted" and "every task has settled" are both determined by it, since
/// no state can hold them independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Every task merged.
    AllTerminal,
    /// A failure, and the tasks that can never run because of it.
    BlockedByFailure,
    /// A task that could be dispatched right now.
    AdmissiblePending,
    /// Neither settled nor admissible: the shape the design argues is
    /// unreachable, kept here because "unreachable" is a claim about
    /// histories and this is a claim about states.
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

/// The packet's total function, written from its text over the dimensions
/// rather than over a state.
///
/// This is the whole point of the grid: production derives each dimension
/// from state and then applies the precedence, and this applies the
/// precedence to the dimensions directly. A defect in either half — a
/// dimension read wrongly from state, or a precedence applied in the wrong
/// order — separates the two.
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

/// A state realizing one cell of the grid.
///
/// Built by writing the fold's own state rather than by replaying a
/// history: the obligation is that the function is total over states, and
/// which of those states a history can reach is the bounded census's
/// question, not this one's.
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
            Backoff::DeferredTask => run.tasks[MID.index()].state = TaskState::Deferred,
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
    // 1008 cells: seven blockers (nothing, each of the four open
    // generation classes, and each of the two transaction classes),
    // halting or not, three budget scopes, three backoff shapes, questions
    // or not, four task-set shapes.
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
    // Every arm of the function, including the one the design argues is
    // unreachable: a value a census can assert about rather than a panic.
    assert_eq!(reached.len(), 6, "arms reached: {reached:?}");
}

#[test]
fn pending_backoff_blocks_parked_and_complete_and_never_blocks_halted_or_budget() {
    // The one precedence consequence the packet states in its own words,
    // asserted as a relation over the crossed grid rather than as an
    // example: for every cell, adding backoff moves Parked and Complete to
    // NotEnding and leaves every other answer exactly where it was.
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
    // refusals[19]: every outcome has an accepted and a refused instance,
    // and the refusals are the four the packet names by hand.
    let outcomes = [
        RunOutcome::Complete,
        RunOutcome::Parked,
        RunOutcome::Halted,
        RunOutcome::BudgetExceeded,
    ];

    // Complete: every task settled, nothing queued, nothing held.
    let complete = grid_state(
        Blocker::Nothing,
        false,
        Budget::None,
        Backoff::None,
        false,
        Shape::AllTerminal,
    );
    assert_accepts_exactly(&complete, &outcomes, RunOutcome::Complete, None);

    // Parked: an open question and nothing admissible.
    let parked = grid_state(
        Blocker::Nothing,
        false,
        Budget::None,
        Backoff::None,
        true,
        Shape::AllTerminal,
    );
    assert_accepts_exactly(&parked, &outcomes, RunOutcome::Parked, None);

    // Halted: a halting settlement, whatever else is true.
    let halted = grid_state(
        Blocker::Nothing,
        true,
        Budget::Current,
        Backoff::DeferredTask,
        true,
        Shape::Stuck,
    );
    assert_accepts_exactly(&halted, &outcomes, RunOutcome::Halted, Some(ALPHA));

    // BudgetExceeded: a stop in this epoch and no halting settlement —
    // accepted with a deferred task present, which Parked and Complete are
    // not.
    let budget = grid_state(
        Blocker::Nothing,
        false,
        Budget::Current,
        Backoff::DeferredCandidate,
        true,
        Shape::Stuck,
    );
    assert_accepts_exactly(&budget, &outcomes, RunOutcome::BudgetExceeded, None);

    // NotEnding: nothing is accepted at all.
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

    // And the attribution has to be the fold's: a halt recorded against
    // another task, or none at all, is a report of a run that did not
    // happen.
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
    // refusals[21]: Complete and Halted are terminal — finalized and then
    // refused. Parked and BudgetExceeded resume, and the only event that
    // continues them is that resume.
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

// -----------------------------------------------------------------------
// INV-02: one transition, poisoning, and the whole-log parse
// -----------------------------------------------------------------------

/// One of every kind, so a table over the vocabulary is a table over all of
/// it rather than over the ones somebody remembered.
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
        // **`attempt_finished`, on a transition this fold accepts.** The
        // table held `succeeded(ZETA, 0, 1)` here, and since the 2026-08-27
        // CONFORM ruling that is not a settlement the fold accepts at all —
        // `candidate_prepared` further down is the successful one. A
        // *poisoned* fold must still refuse `attempt_finished`, so the kind
        // stays in the table on a transition a healthy fold would take.
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
                // T-ATTEMPT closes the generation, so an ordinary one
                // releases the region it predicted.
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
    // refusals[24]: the command has already ended. Nothing is appended and
    // nothing is derived from memory — including the informational records,
    // which a process that cannot vouch for its own state may not write
    // either.
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
    // And it is not a state a later event clears.
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
    // refusals[23], and the boundary it is distinguished from: the newline
    // is the commit marker, so an unterminated final line is a torn tail
    // and is dropped, while a terminated one that will not parse means the
    // log was rewritten.
    let first = serde_json::to_string(&run_started_event()).expect("serialize");
    let second = serde_json::to_string(&raised("q-park-Ünicode", ZETA)).expect("serialize");

    let whole = format!("{first}\n{second}\n");
    assert_eq!(
        TopologyFold::parse_log(whole.as_bytes())
            .expect("a whole log parses")
            .len(),
        2
    );

    // A torn tail: syntactically complete and never committed.
    let torn = format!("{first}\n{second}");
    let parsed = TopologyFold::parse_log(torn.as_bytes()).expect("a torn tail is not an error");
    assert_eq!(parsed.len(), 1, "an uncommitted line is not an event");

    // A committed line that is not an event, at every position.
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

    // Invalid UTF-8 inside a committed line is the same situation.
    let mut bytes = format!("{first}\n").into_bytes();
    bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
    assert!(matches!(
        TopologyFold::parse_log(&bytes),
        Err(FoldError::RewrittenLog { line: 2, .. })
    ));

    // A committed line that is *blank* is the same situation again, and
    // the one the refusal is easiest to lose: the newline is the commit
    // marker, so an empty or whitespace-only terminated line is a
    // committed record that is not an event. Skipping it would fold a log
    // whose physical shape no reader can account for — and would let a
    // rewrite that blanked a line read back as a shorter valid log.
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

/// Apply an event to a live fold and record it in the trace it came from.
fn push(live: &mut TopologyFold, trace: &mut Vec<TopologyEvent>, event: TopologyEvent) {
    apply(live, &event);
    trace.push(event);
}

/// A run that retries on a retained session, merges fast, verifies stale,
/// defers on an outage, wakes, is rejected into a repair, exceeds its
/// budget and resumes.
fn long_trace() -> Vec<TopologyEvent> {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    // alpha: dispatched, retried on a retained session, then merged fast.
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
    // Attempt 2 is the one that succeeded, so it is the one the candidate
    // is attributed to.
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

    // zeta: verified stale, deferred by an outage, woken, then rejected —
    // which registers a repair — and the repair is dispatched and parked.
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

/// A run that is interrupted, closes a generation, merges, registers a
/// repair by hand, has a verification interrupted, and parks and answers a
/// question — the guarded kinds the long trace does not reach.
fn settled_trace() -> Vec<TopologyEvent> {
    let base = sha("base");
    let head = sha("head");
    let proposal = sha("proposal");
    let mut live = started();
    let mut trace = vec![run_started_event()];

    // An interruption closes generation 0 and returns zeta to pending.
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
    // Generation 1 is dispatched and closed without an attempt.
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

    // alpha merges fast, which gives a repair something to depend on.
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

    // zeta's third generation prepares a candidate whose verification is
    // interrupted.
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

    // And a question is asked about a third task and answered.
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

/// Every task merged, and the run saying so.
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
    // INV-02, as the property rather than as the claim: a fold driven
    // event by event and a fold replayed from the same bytes hold the same
    // state — and the bytes are what a writer would have appended, so the
    // comparison is over a serialization round trip too.
    for trace in [long_trace(), settled_trace(), finished_trace()] {
        let mut live = TopologyFold::new(inputs());
        for event in &trace {
            apply(&mut live, event);
        }
        // Through the wire, not through the values: a replay reads bytes.
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

/// Serialize a trace the way a writer would append it.
fn wire(trace: &[TopologyEvent]) -> Vec<u8> {
    let mut log = Vec::new();
    for event in trace {
        log.extend_from_slice(serde_json::to_string(event).expect("serialize").as_bytes());
        log.push(b'\n');
    }
    log
}

/// Copies of `event` with exactly one coordinate moved to a value the fold
/// must refuse *in this event's own position*.
///
/// One field at a time is the whole point: an event that disagreed with its
/// state in several places at once could be caught by any one of the
/// relations, and would not say which. Everything here is a relation the
/// fold owns — an identity, a count, a SHA, a disposition — never a shape
/// serialization would already have refused.
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
            // The recorded region, moved one component. Before it was
            // derivation-checked this line was the whole finding: the fold
            // admitted on `predicted_region`'s answer and the lease table
            // kept the event's, so a hostile log carried one region past
            // the door and every later overlap check consulted the other.
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
            // The Retained arm's own two cells. `long_trace` carries a
            // retained settlement, and until this arm bound the envelope to
            // the record a hostile log could move either past it.
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
            // A retained record turned into one that claims the attempt
            // succeeded. Both arms refuse this shape; only the `Closed` one
            // did before, so a hostile log could carry it past the door on
            // the retained path.
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
            // The configured passes, emptied. Every remaining entry is
            // green — there are none — so `is_successful` is true of this
            // record and only the frozen plan can tell it apart from a
            // reviewed one.
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
    // INV-02: "Live state and replay use one checked transition over the
    // exact wire event; an invalid transition is never appended."
    //
    // Equal *valid* traces cannot prove this: a replay that applied every
    // event unchecked, or that skipped the ones the checked transition
    // refused and carried on, reaches exactly the same state over a valid
    // log. The witness has to be a log a writer would never have produced —
    // a valid prefix, one event with one field moved, and a valid suffix —
    // and the claim is that replay stops on that line with the refusal the
    // live path gives, rather than reaching a state at all.
    //
    // The expected refusal is taken from the live path over the same
    // prefix, which is the other half of the invariant and not the
    // function under test explaining itself: two independent entry points
    // are required to answer identically.
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
                // Live: refused, and asking left the state exactly as it
                // was.
                let live_error = prefix
                    .plan_transition(&invalid)
                    .err()
                    .unwrap_or_else(|| panic!("{label} is not an invalid transition"));
                assert_eq!(
                    prefix.state().cloned(),
                    before,
                    "{label} mutated on refusal"
                );

                // Replay: the same refusal, over the wire, with a valid
                // suffix behind it that a lenient reader would have gone on
                // to apply.
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

    // The sweep is over the vocabulary, not over what was remembered. The
    // three informational kinds are never refused, and `defer_wait_elapsed`
    // carries no field a fold relation reads — both are witnessed on their
    // own below.
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

    // `defer_wait_elapsed`'s guard is the state rather than a field
    // (refusals[18]: no wait elapses under a halt or the epoch's budget
    // stop), so its hostile witness is one appended where the prefix
    // forbids it.
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
    // The emit contract is: build the event, round-trip it, plan the
    // transition, append *the exact bytes*, apply the delta. A delta whose
    // event is a rebuilt or normalized copy of the one it was asked about
    // would let a writer append one record and fold another — which is the
    // divergence between live state and replay that INV-02 forbids, in the
    // one place the two are not literally the same call.
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
    // The other half of INV-02: an invalid transition is never applied,
    // which is a property of `plan_transition` being a question rather
    // than an action.
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
    // The authentication value is over the *originals*: a reader rebuilds
    // them from the frozen plan and the run record and compares. A dynamic
    // entry has no frozen input behind it to rebuild from, so a digest that
    // grew with one would be a value no reader could recompute.
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

    // The other half, and the one that has no producer yet: the canonical
    // serialization is of the *registry*, so it covers every constructible
    // entry. The digest is narrow because a reader rebuilds only the
    // originals; the encoding is not, because a dynamic entry no encoder
    // ever visits is a value nothing downstream can compare — which is how
    // a stored entry can differ from the event that registered it and
    // nobody notices.
    let bytes = registry.canonical_bytes();
    assert_ne!(
        bytes, before_bytes,
        "a registered repair left the canonical serialization unchanged"
    );
    assert!(
        bytes.len() > before_bytes.len(),
        "the encoding did not grow by an entry"
    );
    // Its own fields are in there, including the allow-list, which is the
    // field a derivation could quietly substitute for.
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(text.contains(registered.display_id.as_str()));
    assert!(text.contains("Repair the alpha rejection"));
    for agent in &registered.allowed_agents {
        assert!(
            text.contains(agent.as_str()),
            "the stored allow-list entry `{agent}` is not in the canonical encoding"
        );
    }

    // And the stored entry is the entry the event registered, field for
    // field — not one derived from its ladder rungs or its admission
    // options. Nothing else in this slice reads a dynamic entry back.
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
    // And the repair is addressable by both of its identities.
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

// -----------------------------------------------------------------------
// Regions, holdings and queue eligibility
// -----------------------------------------------------------------------

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

    // Equal, ancestor and descendant overlap; a byte prefix that is not a
    // component prefix does not. `src/foo` and `src/foobar` are the case
    // that separates a component comparison from a `starts_with`.
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

    // Case folding is the run's, resolved once, and it folds beyond ASCII:
    // a case-folding filesystem folds `Ü` the same way it folds `U`.
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

    // Repo-wide overlaps everything, including the empty region — the
    // asymmetry the variant exists for.
    for other in [PathSet::RepoWide, prefixes(&[]), prefixes(&["src/foo"])] {
        assert!(regions_overlap(&PathSet::RepoWide, &other, &folding));
        assert!(regions_overlap(&other, &PathSet::RepoWide, &folding));
    }
    // And an empty region overlaps nothing else: a diff that touched
    // nothing is not a diff that touched everything.
    assert!(!regions_overlap(
        &prefixes(&[]),
        &prefixes(&["src/foo"]),
        &folding
    ));

    // A set overlaps when any member does, not only the first.
    assert!(regions_overlap(
        &prefixes(&["docs", "src/foo"]),
        &prefixes(&["build.rs", "src/foo/bar.rs"]),
        &folding
    ));
}

#[test]
fn an_ordinary_candidate_waits_for_any_lineage_and_a_member_only_for_older_ones() {
    // `decisions.coordinator_integration.queue`, as the relation it is: a
    // lineage holds the region a rejection made contentious, so ordinary
    // work stays out of it entirely, and two lineages contending for one
    // region resolve by age rather than taking turns blocking each other.
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

    // Parking and deferral outrank both, and are distinguished from each
    // other so a queue that reported one for the other is visible.
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

    // A region nobody holds is eligible whatever the lineages are.
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

    // Repo-wide absorbs: a region nobody could read stays unbounded.
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

    // A holding belongs to its owner: the same region held by somebody
    // else is a collision, and held by yourself is not.
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
    // Releasing what nobody holds is a statement, not an operation.
    table.release(LeaseOwner::Lineage { root: MID });
    assert!(!table.holds(LeaseOwner::Lineage { root: MID }));
}

#[test]
fn a_generations_holding_decides_the_disposition_its_settlements_record() {
    // The relation refusals[14] is checked against, stated on its own: two
    // holdings, two fates, and exactly one disposition per cell.
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
    // `admission_and_leases.path_policy.prediction`: the literal prefix
    // before the first glob metacharacter, and repo-wide for anything
    // unsafe or absent — the classification that costs parallelism and
    // never costs correctness.
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

    // Absent, and unsafe, both classify repo-wide.
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
    // A backslash-separated hint is a Windows spelling of the same region,
    // not a one-component path with a backslash in its name.
    let mut windows = zeta.clone();
    windows.spec.path_hints = vec!["src\\Zebra\\mod.rs".to_owned()];
    assert_eq!(predicted_region(&windows), prefixes(&["src/Zebra/mod.rs"]));
}

#[test]
fn the_pipeline_entitlement_is_what_the_fold_derives_it_to_be() {
    // `admission_and_leases.permits.pipeline`: held by generations that are
    // open with no attempt, in flight, or promoting, plus one for an
    // unresolved integration transaction — and by nothing else. Retained
    // and closed generations hold none, and neither does a queued
    // candidate.
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
    // The end-to-end shape, driven by events rather than by writing state:
    // three tasks merged over the fast path, and the outcome moving from
    // NotEnding to Complete exactly at the last one.
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
    // `run_end_policy.derived_outcome`'s precedence, source by source:
    // "if not common -> NotEnding; else if halting -> Halted; else if
    // budget -> BudgetExceeded; else if structurally_admissible or
    // backoff_pending -> NotEnding". A singleton example cannot reveal an
    // order, so each structural source is isolated and then crossed with a
    // halt and with the epoch's budget stop.
    let base = sha("base");

    // Source 1: a dispatchable task. A fresh run has exactly one — alpha
    // depends on nothing; zeta and mid wait on it — and an empty queue, so
    // `ready` is the only source alight.
    let ready_state = || started();
    assert_eq!(ready_state().derived_outcome(), DerivedOutcome::NotEnding);

    // Source 2: an eligible queued candidate and nothing dispatchable.
    // alpha is failed so no task is ready, and the two prepared candidates
    // are eligible, so `integration_admissible` is the only source alight.
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

    // Source 3, and why it can never be crossed with either: a retry is
    // admissible only while a RetainedIdle generation is open, and an open
    // generation of any class makes `common` false, which outranks
    // everything. The state is recorded here rather than argued, because
    // "unreachable" is the kind of claim that stops being true quietly.
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
    // The Complete arm's conjuncts past the task predicate: "the queue is
    // empty (no R6 open), and no candidate or lineage lease is active
    // (R7/R8 none)". Every task is held terminal throughout, so each
    // residue is the only thing between this state and Complete and a
    // conjunct that was dropped shows up as Complete rather than as a
    // different refusal.
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

    // A generation lease is not one of the two: an ordinary generation's
    // predicted region is released when the generation closes, and the
    // Complete arm names the candidate and lineage holdings only.
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
    // `backoff_pending` is "any task is Deferred or any candidate is
    // verification_deferred (both are woken only by the durable
    // defer_wait_elapsed or run_resumed)". The historical defer *count* is
    // kept for the consecutiveness rule and is not a waiting state, so a
    // candidate that has deferred once and been woken does not block a
    // closure. The two stay correlated unless a fixture separates them.
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

    // Woken: the flag is clear and the history is not.
    let entry = &fold.queue().expect("started").entries()[0];
    assert!(!entry.verification_deferred, "the wake cleared the flag");
    assert_eq!(entry.defers, 1, "and kept the count it is measured against");

    // Settle everything around it, so the only thing that could still make
    // this run NotEnding is that retained count.
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
    // And with the same entry still queued but *not* waiting, the queue
    // conjunct is what stops it — not the count.
    let mut with_entry = fold.clone();
    let mut run = with_entry.run.take().expect("started");
    run.queue.push(carried);
    with_entry.run = Some(run);
    assert_eq!(with_entry.derived_outcome(), DerivedOutcome::NotEnding);
    assert!(
        !with_entry.queue().expect("started").entries()[0].verification_deferred,
        "the entry that blocks Complete is queued, not backing off"
    );

    // The state where the two readings disagree about an *outcome* rather
    // than about a reason: a parked verification. The candidate stays
    // queued and ineligible with its history intact and its flag clear,
    // the task is AwaitingInput, and `derived_outcome` is Parked — which
    // `backoff_pending` outranks. A fold that read the retained count as a
    // waiting state answers NotEnding here and refuses the closure the
    // packet requires.
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
    // Silence the other structural sources so the Parked arm is what is
    // being read: alpha is terminal, and zeta's candidate leaves the queue
    // with the holding it took.
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
    // `run_end_policy.derived_outcome`: Complete requires "every task is
    // Merged, Failed, or Pending with a Failed task in its **transitive**
    // dependency closure (derived Blocked)".
    //
    // The 1008-cell grid cannot prove this, because its BlockedByFailure
    // fixture makes every pending task depend on the failed one directly:
    // there, "directly failed dependency" and "failed anywhere in the
    // closure" are the same predicate. Here they are not. `cee` fails,
    // `bee` depends on `cee` and is blocked directly, and `aay` depends
    // only on `bee` and is blocked by two hops and by nothing else. A
    // derivation that recognized only a directly failed dependency leaves
    // `aay` Pending-and-unblocked, so no arm of the total function claims
    // the state and it lands on FoldError.
    let base = sha("base");
    let started_event = chain_run_started_event();
    let mut live = TopologyFold::new(chain_inputs());
    apply(&mut live, &started_event);
    let mut trace = vec![started_event];
    let push = |live: &mut TopologyFold, trace: &mut Vec<TopologyEvent>, event: TopologyEvent| {
        apply(live, &event);
        trace.push(event);
    };

    // The dependency shape is the fixture's, read back rather than assumed.
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

    // Nothing else can move: `bee` waits on a task that failed and `aay`
    // waits on `bee`. Every Complete conjunct holds.
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

    // Live and replay, through the wire, reach the same verdict — and the
    // run may say so.
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

    // And the direction that says the predicate is not vacuous: with `cee`
    // still Pending rather than Failed, nothing is Blocked, `cee` is
    // admissible, and the run is not ending.
    let prefix = TopologyFold::replay(chain_inputs(), &trace[..1]).expect("the prefix replays");
    assert_eq!(prefix.task_state(CEE), Some(TaskState::Pending));
    assert_eq!(prefix.derived_outcome(), DerivedOutcome::NotEnding);
}

#[test]
fn a_bare_question_is_refused_on_a_lineage_member() {
    // **Pre-repair: accepted**, and the pass-3 review of `671949e` proved
    // the consequence with a unit test at that head: the repair, declined
    // without halting, went `Failed` and released the lineage lease, the
    // rejected original stayed `AwaitingRepair` with nothing carrying it,
    // `derived_outcome` read `FoldError`, and every `run_finished` was
    // refused — a run that cannot end. §26: "Declining fails the lineage";
    // a bare question's answer settles one task.
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
    let error = refused_live_and_on_replay(&fold, &log, &raised("q-park-Ünicode", TaskKey(3)));
    let FoldError::InconsistentRecord { kind, detail } = error else {
        panic!("a question on a lineage member is refused as one: {error}");
    };
    assert_eq!(kind, "question_raised");
    assert!(detail.contains("rooted at 1"), "{detail}");
    // The run this leaves is one that can still end: the repair is runnable.
    assert!(fold.ready(TaskKey(3)), "the repair carries the lineage");
    assert_eq!(fold.derived_outcome(), DerivedOutcome::NotEnding);
}
