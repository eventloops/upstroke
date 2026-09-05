//! Extended notes: `docs/internals/engine/topology/attempt/tests.md`

use std::path::{Path, PathBuf};

use super::*;
use crate::engine::topology::dispatch::DispatchKind;
use crate::engine::topology::identity::{ReservationKind, Reservations};
use crate::engine::topology::scaffold::{
    AGENT, ALPHA, RETAINED_SESSION, Run, kill_child_and_adopt, kill_child_environment, kill_dir,
};
use crate::engine::topology::settle::{self, ManagedWorktrees, RetryOutcome, RetryRequest};
use crate::topology::effects::{
    EffectSiteId, EventSite, HookPhase, Injection, InjectionMode, ObjectResidue, ObjectSite,
    ResidueElement, SnapshotSite, SubEffectPoint, WorktreeSite,
};
use crate::topology::events::{GenerationCloseReason, GenerationId, SessionId};
use crate::topology::fold::{GenerationClass, TaskState};
use crate::workspace_manager::fixture::Fixture;
use crate::workspace_manager::fixture::{
    KillableGitChild, died_by_kill, git, remove_file, time_git, write_file,
};
use crate::workspace_manager::{
    NoHooks, ResidueTarget, VerifyFailure, classify_object_residue, object_directory,
    observed_residue_elements, temporary_object_files, unreachable_objects,
};

const APPEND: EffectSiteId = EffectSiteId::Event(EventSite::Append);
const STAGE: EffectSiteId = EffectSiteId::Object(ObjectSite::CandidateStage);
const WRITE_TREE: EffectSiteId = EffectSiteId::Object(ObjectSite::CandidateWriteTree);
const SNAPSHOT_COMMIT: EffectSiteId = EffectSiteId::Object(ObjectSite::SnapshotCommitTree);
const SNAPSHOT_INTENT: EffectSiteId = EffectSiteId::Snapshot(SnapshotSite::WriteIntent);
const SNAPSHOT_ADD: EffectSiteId = EffectSiteId::Snapshot(SnapshotSite::Add);
const SNAPSHOT_REMOVE: EffectSiteId = EffectSiteId::Snapshot(SnapshotSite::Remove);
const VERIFY: EffectSiteId = EffectSiteId::Worktree(WorktreeSite::Verify);
const SCRUB: EffectSiteId = EffectSiteId::Worktree(WorktreeSite::Remove);

const GENERATION: GenerationId = GenerationId(0);

const WORKED: &[u8] = b"the agent edited this, and the capture stages it\n";
const WORKED_PATH: &str = "worked.txt";

fn agent_edits(worktree: &Path) {
    write_file(&worktree.join(WORKED_PATH), WORKED);
}

struct Process {
    slots: SlotAssertion,
    ledger: InvocationLedger,
}

impl Process {
    fn new() -> Self {
        Self {
            slots: SlotAssertion::new(),
            ledger: InvocationLedger::new(),
        }
    }

    fn balances(&self) -> bool {
        self.slots.balances() && self.ledger.balances()
    }
}

macro_rules! context {
    ($run:expr, $process:expr) => {
        AttemptContext {
            manager: &$run.fixture.manager,
            hooks: &mut $run.hooks,
            emitter: &mut $run.emitter,
            runner: &$run.runner,
            slots: &mut $process.slots,
            ledger: &mut $process.ledger,
            adapters: &crate::engine::topology::scaffold::ScaffoldAdapters::new(),
            paths: &$run.paths,
            reviews: &crate::engine::attempt::LegacyReviewPasses,
            input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        }
    };
}

fn index_blobs(worktree: &Path) -> Vec<String> {
    git(worktree, &["ls-files", "-s"])
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .collect()
}

fn unreachable_ephemeral_commits(base: &Path) -> Vec<String> {
    unreachable_objects(base)
        .expect("fsck")
        .into_iter()
        .filter(|id| {
            git(base, &["cat-file", "-t", id]) == "commit"
                && git(base, &["log", "-1", "--format=%s", id])
                    == "upstroke: ephemeral snapshot input"
        })
        .collect()
}

#[test]
fn attempt_started_is_durable_before_any_spawn() {
    let mut run = Run::started("o23");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let mark = run.mark();
    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("the attempt starts");

    assert_eq!(
        run.emitter.durable_kinds(),
        vec!["run_started", "task_dispatched", "attempt_started"],
        "O23: the attempt is durable"
    );
    let events = run.emitter.durable_events();
    let crate::topology::events::TopologyEventBody::AttemptStarted { data } = &events[2].body
    else {
        panic!("the third durable event is not an attempt");
    };
    assert_eq!(data.attempt, plan.attempt);
    assert_eq!(
        data.binding, plan.binding,
        "INV-19: the frozen rung binding"
    );
    assert!(
        data.resume_session.is_none() && data.materialization_observed.is_none(),
        "a fresh ordinary attempt resumes nothing and materializes nothing"
    );

    let ran = run.runner.ran();
    assert_eq!(ran.len(), 1, "one worker, and nothing else yet");
    assert_eq!(ran[0].invocation, started.identities.worker());
    assert_eq!(
        ran[0].workspace, dispatched.worktree,
        "the worker runs in the task worktree"
    );
    assert_eq!(
        ran[0].durable_at_spawn,
        vec!["run_started", "task_dispatched", "attempt_started"],
        "O23: `attempt_started` must already be on disk when the worker is asked for"
    );
    assert_eq!(
        run.count_after(mark, APPEND, HookPhase::After),
        1,
        "one append for the attempt, and it is the one the worker ran after"
    );

    assert_eq!(
        run.emitter.generation_class(ALPHA, GENERATION),
        GenerationClass::InFlight {
            attempt: plan.attempt
        }
    );
    assert!(
        process.balances(),
        "the worker's slot and registration settled"
    );
}

#[test]
fn every_process_of_an_attempt_is_recorded_reviewers_included() {
    let mut run = Run::started("ledger-covers-reviews");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let expected = 1 + plan.gates.len() + plan.reviewers.len();
    assert!(
        plan.reviewers.len() >= 2,
        "this fixture plans {} reviewer(s); with none the count below would \
         pass without covering the case it exists for",
        plan.reviewers.len()
    );

    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");
    agent_edits(&dispatched.worktree);
    let capture = context!(run, process)
        .capture(dispatched.site())
        .expect("capture");
    let review_inputs = run.review_inputs();
    let assessed = context!(run, process)
        .assess(
            dispatched.site(),
            &plan,
            &started,
            &capture,
            &review_inputs.diff,
            crate::ir::TaskKind::Implement,
        )
        .expect("the scaffold's adapter parses its own worker output");
    context!(run, process)
        .judge(
            dispatched.site(),
            &plan,
            Judging {
                run: &started,
                capture: &capture,
                assessed: &assessed,
            },
            &review_inputs,
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    assert_eq!(
        process.ledger.completed(),
        expected,
        "the ledger holds {} settled invocation(s) for an attempt that ran one \
         worker, {} gate(s) and {} reviewer(s)",
        process.ledger.completed(),
        plan.gates.len(),
        plan.reviewers.len()
    );
    assert!(process.balances(), "and every one of them settled");
}

#[test]
fn a_refused_gate_ends_the_set_and_its_cause_survives() {
    let mut run = Run::started("gate-short-circuit");
    let dispatched = run.dispatch(ALPHA, 0);
    let mut plan = run.attempt_plan(ALPHA, 1);
    let second = plan.gates[0].clone();
    plan.gates.push(second);
    run.runner.set_codes(vec![0, 2, 127]);
    let mut process = Process::new();

    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");
    agent_edits(&dispatched.worktree);
    let capture = context!(run, process)
        .capture(dispatched.site())
        .expect("capture");
    let review_inputs = run.review_inputs();
    let assessed = context!(run, process)
        .assess(
            dispatched.site(),
            &plan,
            &started,
            &capture,
            &review_inputs.diff,
            crate::ir::TaskKind::Implement,
        )
        .expect("the scaffold's adapter parses its own worker output");
    let judgement = context!(run, process)
        .judge(
            dispatched.site(),
            &plan,
            Judging {
                run: &started,
                capture: &capture,
                assessed: &assessed,
            },
            &review_inputs,
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    assert_eq!(
        judgement.gates.len(),
        1,
        "the second gate ran after the first refused, so a rejected diff bought \
         a gate that could not change the verdict"
    );
    let failure = judgement
        .failure
        .as_ref()
        .expect("a refused gate fails the attempt");
    assert_eq!(
        failure.kind,
        crate::ladder::FailureKind::GateFailed,
        "the first cause did not survive: the attempt is recorded as {:?}, \
         which is what the SECOND gate would have produced",
        failure.kind
    );
    assert!(
        !judgement.accepted(),
        "an attempt whose gate refused was accepted"
    );

    assert_eq!(
        failure.feedback.as_deref().map(str::trim),
        Some(
            format!(
                "{} (exit 2)",
                crate::engine::topology::scaffold::GATE_DIAGNOSTIC
            )
            .as_str()
        ),
        "the gate's output did not reach the failure's feedback: {:?}",
        failure.feedback
    );
    assert!(
        failure.reason.contains("scaffold"),
        "the failure does not name the gate: {}",
        failure.reason
    );
}

#[test]
fn capture_precedes_the_snapshots_and_every_snapshot_commits_before_its_intent() {
    let mut run = Run::started("o25-27");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");
    agent_edits(&dispatched.worktree);
    let capture = context!(run, process)
        .capture(dispatched.site())
        .expect("capture");
    let mark = run.mark();
    let review_inputs = run.review_inputs();
    let assessed = context!(run, process)
        .assess(
            dispatched.site(),
            &plan,
            &started,
            &capture,
            &review_inputs.diff,
            crate::ir::TaskKind::Implement,
        )
        .expect("the scaffold's adapter parses its own worker output");
    let judgement = context!(run, process)
        .judge(
            dispatched.site(),
            &plan,
            Judging {
                run: &started,
                capture: &capture,
                assessed: &assessed,
            },
            &review_inputs,
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    let stage = run.must_order_of(STAGE, HookPhase::Before);
    let write_tree = run.must_order_of(WRITE_TREE, HookPhase::Before);
    let commit = run.must_order_of(SNAPSHOT_COMMIT, HookPhase::Before);
    assert!(
        stage < write_tree && write_tree < commit,
        "O25: capture (stage={stage}, write-tree={write_tree}) precedes the snapshots \
         (commit={commit})"
    );

    let snapshots = 1 + plan.reviewers.len();
    for (site, what) in [
        (SNAPSHOT_COMMIT, "ephemeral commit"),
        (SNAPSHOT_INTENT, "intent"),
        (SNAPSHOT_ADD, "add"),
    ] {
        assert_eq!(
            run.count_after(mark, site, HookPhase::Before),
            snapshots,
            "one {what} per snapshot, and there are {snapshots} of them"
        );
    }
    let mut fence = mark;
    for index in 0..snapshots {
        let commit = run.order_after(fence, SNAPSHOT_COMMIT, HookPhase::Before);
        let intent = run.order_after(fence, SNAPSHOT_INTENT, HookPhase::Before);
        let add = run.order_after(fence, SNAPSHOT_ADD, HookPhase::Before);
        assert!(
            commit < intent && intent < add,
            "O26, snapshot {index} of {snapshots}: the order was commit={commit}, \
             intent={intent}, add={add}"
        );
        fence = add + 1;
    }

    assert!(judgement.accepted());
    assert_eq!(judgement.gates.len(), 1);
    assert_eq!(judgement.reviews.len(), 2);
    assert!(
        !run.observed(
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            HookPhase::Before
        ),
        "O27: the candidate commit is `candidate.rs`'s and nothing here may write one"
    );

    assert_eq!(
        capture.tree,
        git(&dispatched.worktree, &["write-tree"]),
        "the recorded tree is the worktree's index"
    );
    assert_eq!(capture.parent, dispatched.base.0);
}

#[test]
fn gates_and_reviewers_run_on_fresh_exact_snapshots_and_never_in_the_task_worktree() {
    let mut run = Run::started("snapshots");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");
    agent_edits(&dispatched.worktree);
    let capture = context!(run, process)
        .capture(dispatched.site())
        .expect("capture");
    let review_inputs = run.review_inputs();
    let assessed = context!(run, process)
        .assess(
            dispatched.site(),
            &plan,
            &started,
            &capture,
            &review_inputs.diff,
            crate::ir::TaskKind::Implement,
        )
        .expect("the scaffold's adapter parses its own worker output");
    let judgement = context!(run, process)
        .judge(
            dispatched.site(),
            &plan,
            Judging {
                run: &started,
                capture: &capture,
                assessed: &assessed,
            },
            &review_inputs,
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    let workspaces: Vec<PathBuf> = run
        .runner
        .ran()
        .into_iter()
        .filter(|ran| {
            matches!(
                ran.role,
                crate::runner::ExecutionRole::Gate | crate::runner::ExecutionRole::Review
            )
        })
        .map(|ran| ran.workspace)
        .collect();
    for workspace in &workspaces {
        assert_ne!(
            *workspace, dispatched.worktree,
            "a verification process ran in the worker's worktree"
        );
        assert!(
            workspace.starts_with(run.fixture.manager.execution_root().join("snapshots")),
            "{} is not an exact snapshot",
            workspace.display()
        );
    }
    let distinct: std::collections::BTreeSet<&PathBuf> = workspaces.iter().collect();
    assert_eq!(
        distinct.len(),
        3,
        "one snapshot for the gate set and one fresh per reviewer, never shared across roles \
         or between reviewers: {workspaces:?}"
    );
    assert_eq!(
        judgement.reviews.len(),
        2,
        "both passes produced a record; `AttemptRecord.reviews` being empty \
         MEANS nothing was reviewed, so a pass that ran and recorded nothing \
         would write a false statement into the log"
    );

    assert!(
        run.fixture.manager.intents().expect("intents").len() == 1,
        "only the task worktree's intent is left"
    );
    for workspace in &distinct {
        assert!(
            !workspace.exists(),
            "{} survived its judgement",
            workspace.display()
        );
    }
    assert_eq!(
        run.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .count(SNAPSHOT_REMOVE, HookPhase::After),
        3,
        "three snapshots created, three removed"
    );
    assert!(process.balances());
}

#[test]
fn gates_take_no_slot_and_the_worker_and_reviewers_do() {
    let mut run = Run::started("slots");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");
    assert!(is_slotted(&started.identities.worker()));
    assert!(is_slotted(&started.identities.review_pass(0, 0)));
    assert!(!is_slotted(&started.identities.gate(0, 0)));

    let refusal = process
        .slots
        .acquire(
            &started.identities.gate(0, 0),
            SlotPair {
                agent: "claude-code".to_owned(),
                pool: None,
            },
        )
        .expect_err("a gate takes no pair");
    assert!(
        refusal.to_string().contains("acquires no slot"),
        "{refusal}"
    );

    agent_edits(&dispatched.worktree);
    let capture = context!(run, process)
        .capture(dispatched.site())
        .expect("capture");
    let review_inputs = run.review_inputs();
    let assessed = context!(run, process)
        .assess(
            dispatched.site(),
            &plan,
            &started,
            &capture,
            &review_inputs.diff,
            crate::ir::TaskKind::Implement,
        )
        .expect("the scaffold's adapter parses its own worker output");
    context!(run, process)
        .judge(
            dispatched.site(),
            &plan,
            Judging {
                run: &started,
                capture: &capture,
                assessed: &assessed,
            },
            &review_inputs,
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    let ran = run.runner.ran();
    assert_eq!(ran.len(), 4, "one worker, one gate, two reviewers");
    assert_eq!(ran[0].role, crate::runner::ExecutionRole::Implement);
    assert_eq!(
        ran[0].command, plan.worker,
        "the worker's request carries the plan's command, not a rebuilt one"
    );
    assert_eq!(ran[1].command, plan.gates[0].command);
    assert_eq!(ran[1].role, crate::runner::ExecutionRole::Gate);
    assert!(
        ran[1].agent.is_none(),
        "a gate runs no agent CLI and is bound to no agent"
    );
    assert_eq!(ran[2].role, crate::runner::ExecutionRole::Review);
    assert_eq!(ran[3].role, crate::runner::ExecutionRole::Review);
    assert_ne!(
        ran[2].agent, ran[3].agent,
        "the two reviewers are two agents, so a pair taken for one is not the other's"
    );
    assert!(process.balances(), "every pair and registration settled");
}

fn retained_tree(worktree: &Path) -> String {
    agent_edits(worktree);
    git(worktree, &["add", "-A"]);
    git(worktree, &["write-tree"])
}

fn settle_retry(
    run: &mut Run,
    reservations: &mut Reservations,
    dispatched: &Dispatched,
    retained: &str,
) -> RetryOutcome {
    let plan = run.attempt_plan(ALPHA, 2);
    let request = RetryRequest {
        key: ALPHA,
        slot: dispatched.slot.clone(),
        retained_tree: retained.to_owned(),
        binding: plan.binding.clone(),
        rung: plan.rung,
        pool: plan.pool.clone(),
        materialization: None,
    };
    settle::retry(
        run.emitter.fold(),
        reservations,
        &ManagedWorktrees::new(&run.fixture.manager),
        run.hooks.effects(),
        &request,
    )
    .expect("a worktree that does not verify is a decision, not an error")
}

fn authorized_plan(run: &Run, authorized: &AttemptStarted4) -> AttemptPlan {
    AttemptPlan {
        attempt: authorized.attempt,
        rung: authorized.rung,
        binding: authorized.binding.clone(),
        pool: authorized.pool.clone(),
        resume_session: authorized.resume_session.clone(),
        materialization_observed: authorized.materialization_observed,
        ..run.attempt_plan(ALPHA, authorized.attempt.0)
    }
}

#[test]
fn a_retry_verifies_once_then_appends_then_spawns() {
    let mut run = Run::started("o24");
    let dispatched = run.dispatch(ALPHA, 0);
    let mut process = Process::new();

    let plan = run.attempt_plan(ALPHA, 1);
    context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("the first attempt");
    let tree = retained_tree(&dispatched.worktree);
    assert_ne!(
        tree,
        git(
            &run.fixture.base,
            &["rev-parse", &format!("{}^{{tree}}", dispatched.base.0)]
        ),
        "the retained tree must differ from the base's, or `HoldsTree` and `AtBase` \
         cannot be told apart"
    );
    run.retain(ALPHA, GENERATION, 1);
    assert!(matches!(
        run.emitter.generation_class(ALPHA, GENERATION),
        GenerationClass::RetainedIdle { .. }
    ));

    let mark = run.mark();
    let mut reservations = Reservations::new();
    let outcome = settle_retry(&mut run, &mut reservations, &dispatched, &tree);
    let RetryOutcome::Start(authorized) = outcome else {
        panic!("a retained worktree that verifies starts the attempt");
    };
    assert!(
        !reservations.is_empty(),
        "`permits.provisional_reservations` calls the reservation the bridge between the \
         selection and its first append, and nothing is holding it"
    );
    assert_eq!(
        run.count_after(mark, VERIFY, HookPhase::Before),
        1,
        "the retry observed the worktree exactly once"
    );

    let retry = authorized_plan(&run, &authorized);
    let started = context!(run, process)
        .start(dispatched.site(), &retry)
        .expect("the retry starts");
    reservations
        .convert(ALPHA, ReservationKind::Retry)
        .expect("converted at `attempt_started(retry)`");
    assert!(
        reservations.balances(),
        "the reservation was taken once and settled once"
    );

    assert_eq!(
        run.count_after(mark, VERIFY, HookPhase::Before),
        1,
        "and still exactly once after the append: the second half of O24 re-observes nothing"
    );

    assert_eq!(
        run.count_after(mark, SCRUB, HookPhase::Before),
        0,
        "the retained worktree is reused, not rebuilt"
    );
    assert_eq!(
        git(&dispatched.worktree, &["write-tree"]),
        tree,
        "and it still holds the retained cumulative tree"
    );

    let verify = run.order_after(mark, VERIFY, HookPhase::Before);
    let append = run.order_after(mark, APPEND, HookPhase::Before);
    assert!(
        verify < append,
        "O24: this retry's verification is at {verify} and its append at {append}, and the \
         verification must come first"
    );
    assert_eq!(
        run.count_after(mark, APPEND, HookPhase::Before),
        1,
        "exactly one append for the retry"
    );

    let durable = run.emitter.durable_events();
    assert_eq!(
        durable.last().map(|event| &event.body),
        Some(
            &crate::topology::events::TopologyEventBody::AttemptStarted {
                data: (*authorized).clone(),
            }
        ),
        "the appended event is not the one the verification authorized"
    );
    assert_eq!(authorized.generation, GENERATION);
    assert_eq!(
        authorized.attempt,
        crate::topology::events::AttemptNumber(2),
        "a retry is a new attempt number"
    );
    assert_eq!(
        authorized.resume_session,
        Some(SessionId(RETAINED_SESSION.to_owned())),
        "and it resumes the session the generation retained"
    );

    assert_eq!(
        run.runner.ran().len(),
        2,
        "the retry's worker is the second process, and it ran after the append"
    );
    assert_eq!(
        run.runner.ran()[1].invocation,
        started.identities.worker(),
        "INV-20: a retry is a new attempt number, so its worker is a new identity"
    );
    assert_eq!(
        run.runner.ran()[1]
            .durable_at_spawn
            .last()
            .map(String::as_str),
        Some("attempt_started"),
        "O24's fourth step: the retry's append is on disk when its worker is asked for"
    );
    assert_ne!(
        run.runner.ran()[0].invocation,
        run.runner.ran()[1].invocation
    );
    assert!(process.balances());
}

fn git_dir(worktree: &Path) -> PathBuf {
    PathBuf::from(git(worktree, &["rev-parse", "--absolute-git-dir"]))
}

#[test]
fn a_retry_whose_retained_worktree_fails_verification_closes_and_destroys_nothing() {
    let mut run = Run::started("inv06");
    let dispatched = run.dispatch(ALPHA, 0);
    let mut process = Process::new();

    let plan = run.attempt_plan(ALPHA, 1);
    context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("the first attempt");
    let tree = retained_tree(&dispatched.worktree);
    let base_tree = git(
        &run.fixture.base,
        &["rev-parse", &format!("{}^{{tree}}", dispatched.base.0)],
    );
    assert_ne!(
        tree, base_tree,
        "the retained tree must differ from the base's, or a recreate at the base would \
         reproduce it and this test could not tell the two apart"
    );
    run.retain(ALPHA, GENERATION, 1);

    let lock = git_dir(&dispatched.worktree).join("index.lock");
    write_file(&lock, b"");

    let durable_before = run.emitter.durable_kinds();
    let mark = run.mark();
    let mut reservations = Reservations::new();
    let outcome = settle_retry(&mut run, &mut reservations, &dispatched, &tree);

    let RetryOutcome::Close { closed, failure } = outcome else {
        panic!("a retained worktree that does not verify is not retried into");
    };
    assert_eq!(
        failure,
        VerifyFailure::Residue(ResidueElement::IndexLock),
        "what `Worktree.Verify` actually observed"
    );
    assert_eq!(closed.reason, GenerationCloseReason::WorktreeMissing);
    assert_eq!(closed.key, ALPHA);
    assert_eq!(closed.generation, GENERATION);
    assert!(
        reservations.is_empty() && reservations.balances(),
        "a pre-append failure left the provisional `{{pipeline}}` reservation held"
    );

    assert_eq!(
        run.count_after(mark, VERIFY, HookPhase::Before),
        1,
        "the retry verified exactly once"
    );
    assert_eq!(
        run.count_after(mark, SCRUB, HookPhase::Before),
        0,
        "INV-06: a retained worktree is never removed by a retry"
    );
    assert_eq!(
        run.count_after(mark, APPEND, HookPhase::Before),
        0,
        "O24: nothing durable follows a verification that failed"
    );
    assert_eq!(
        run.emitter.durable_kinds(),
        durable_before,
        "and in particular no `attempt_started(retry)` claiming the retained session"
    );
    assert_eq!(
        run.emitter.generation_class(ALPHA, GENERATION),
        GenerationClass::RetainedIdle {
            session: SessionId(RETAINED_SESSION.to_owned()),
            incarnation: crate::topology::events::Epoch(0),
        },
        "the generation closes at the closure's append and not before it"
    );
    assert!(
        run.runner.ran().len() == 1,
        "only the first attempt's worker ever ran; the retry asked for no process"
    );

    run.emitter
        .emit(
            TopologyEventBody::GenerationClosed { data: closed },
            &mut run.hooks,
        )
        .expect("`generation_closed{WorktreeMissing}` is the tabled recovery");
    assert!(
        matches!(
            run.emitter.generation_class(ALPHA, GENERATION),
            GenerationClass::Closed
        ),
        "the retained generation is closed, not rebuilt"
    );
    assert_eq!(
        run.count_after(mark, SCRUB, HookPhase::Before),
        0,
        "and closing it removed nothing either"
    );

    assert!(
        dispatched.worktree.is_dir(),
        "the retained worktree is still there"
    );
    remove_file(&lock);
    assert_eq!(
        git(&dispatched.worktree, &["write-tree"]),
        tree,
        "and it still holds the cumulative tree the generation retained, byte for byte"
    );
    assert!(process.balances());
}

#[test]
fn a_refused_slot_acquisition_settles_the_registration_it_took() {
    let mut run = Run::started("slotrefusal");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let squatter = AttemptIdentities::new(ALPHA, GenerationId(9), AttemptNumber(9)).worker();
    process
        .slots
        .acquire(
            &squatter,
            SlotPair {
                agent: AGENT.to_owned(),
                pool: Some("scaffold-pool".to_owned()),
            },
        )
        .expect("the pair a worker's role takes");

    let worker = AttemptIdentities::new(dispatched.key, dispatched.generation, plan.attempt)
        .worker()
        .render();
    let error = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect_err("a second slotted invocation is refused at max_parallel = 1");
    assert!(
        error.to_string().contains("asked for a slot pair while"),
        "the refusal must be the slot assertion's, and said: {error}"
    );

    assert!(
        !process.ledger.running().contains(&worker.as_str()),
        "the worker's registration was abandoned `Running`: {:?}",
        process.ledger.running()
    );
    assert!(
        process.ledger.balances(),
        "and the ledger no longer balances, so a real leak would be indistinguishable from \
         this: {:?}",
        process.ledger.running()
    );
    assert_eq!(
        process.ledger.cancelled(),
        1,
        "cancelled, not completed: no process ran"
    );
    assert_eq!(process.ledger.completed(), 0);
    assert_eq!(
        process.ledger.duplicates(),
        0,
        "and it was settled exactly once"
    );

    assert_eq!(
        run.emitter.durable_kinds().last().copied(),
        Some("attempt_started"),
        "the refusal is after O23's append, which is where the register sits"
    );
    assert!(
        run.runner.ran().is_empty(),
        "and the Runner was never reached"
    );
}

#[test]
#[ignore = "spawned as a subprocess by the T-ATTEMPT kill tests"]
fn attempt_kill_child() {
    let (dir, which) = kill_child_environment();
    let mut run = Run::started("killattempt");
    run.hand_off(&dir);
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();
    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");

    if which == "in_attempt" {
        run.arm(STAGE, HookPhase::Before, Injection::Kill);
        let _ = context!(run, process).capture(dispatched.site());
        unreachable!("the kill must have taken this process");
    }

    if which == "retry" {
        let tree = retained_tree(&dispatched.worktree);
        run.retain(ALPHA, GENERATION, 1);
        let mut reservations = Reservations::new();
        let RetryOutcome::Start(authorized) =
            settle_retry(&mut run, &mut reservations, &dispatched, &tree)
        else {
            panic!("the retained worktree of this prefix verifies");
        };
        let retry = authorized_plan(&run, &authorized);
        run.arm(STAGE, HookPhase::Before, Injection::Kill);
        context!(run, process)
            .start(dispatched.site(), &retry)
            .expect("the retry starts");
        reservations
            .convert(ALPHA, ReservationKind::Retry)
            .expect("converted at `attempt_started(retry)`");
        let _ = context!(run, process).capture(dispatched.site());
        unreachable!("the kill must have taken this process");
    }

    agent_edits(&dispatched.worktree);
    if which == "after_capture" {
        run.arm(WRITE_TREE, HookPhase::After, Injection::Kill);
        let _ = context!(run, process).capture(dispatched.site());
        unreachable!("the kill must have taken this process");
    }

    let capture = context!(run, process)
        .capture(dispatched.site())
        .expect("capture");
    match which.as_str() {
        "after_snapshot_commit" => run.arm(SNAPSHOT_COMMIT, HookPhase::After, Injection::Kill),
        "id_unread" => run.arm_point(
            SNAPSHOT_COMMIT,
            SubEffectPoint::IdUnread,
            InjectionMode::Kill,
        ),
        "after_snapshot_add" => run.arm(SNAPSHOT_ADD, HookPhase::After, Injection::Kill),
        other => panic!("unknown site `{other}`"),
    }
    let review_inputs = run.review_inputs();
    let assessed = context!(run, process)
        .assess(
            dispatched.site(),
            &plan,
            &started,
            &capture,
            &review_inputs.diff,
            crate::ir::TaskKind::Implement,
        )
        .expect("the scaffold's adapter parses its own worker output");
    let _ = context!(run, process).judge(
        dispatched.site(),
        &plan,
        Judging {
            run: &started,
            capture: &capture,
            assessed: &assessed,
        },
        &review_inputs,
        &|pass| crate::review::ReviewInvocations {
            pass: started.identities.review_pass(pass, 0),
            reask: started.identities.review_reask(pass, 0),
        },
    );
    unreachable!("the kill must have taken this process");
}

fn adopted_generation(run: &Run) -> Dispatched {
    Dispatched {
        key: ALPHA,
        generation: GENERATION,
        base: run.base(),
        slot: crate::engine::topology::dispatch::task_slot(ALPHA, GENERATION),
        worktree: run
            .fixture
            .manager
            .slot_path(&crate::engine::topology::dispatch::task_slot(
                ALPHA, GENERATION,
            )),
        kind: DispatchKind::Ordinary {
            paths: run.predicted(ALPHA),
        },
    }
}

const CHILD: &str = "engine::topology::attempt::tests::attempt_kill_child";

#[test]
fn kill_during_attempt_settles_interrupted_and_redispatches_new_generation() {
    let dir = kill_dir("killattempt");
    let mut run = kill_child_and_adopt(CHILD, &dir, "in_attempt");
    let dispatched = adopted_generation(&run);
    let mut process = Process::new();

    assert_eq!(
        run.emitter.durable_kinds(),
        vec!["run_started", "task_dispatched", "attempt_started"],
        "the child died in flight"
    );
    assert_eq!(
        run.emitter.generation_class(ALPHA, GENERATION),
        GenerationClass::InFlight {
            attempt: crate::topology::events::AttemptNumber(1)
        }
    );

    context!(run, process)
        .settle_interrupted(
            &dispatched,
            crate::topology::events::AttemptNumber(1),
            AttemptOutcome::Interrupted,
        )
        .expect("settle");

    let events = run.emitter.durable_events();
    let crate::topology::events::TopologyEventBody::AttemptInterrupted { data } =
        &events.last().expect("a terminal").body
    else {
        panic!("the last durable event is not an interruption");
    };
    assert_eq!(
        data.lease,
        crate::topology::events::LeaseDisposition::PredictedReleased,
        "an ordinary generation releases its predicted region when it closes"
    );
    assert!(data.detail.contains("unknown"), "{}", data.detail);
    assert_eq!(
        run.emitter.generation_class(ALPHA, GENERATION),
        GenerationClass::Closed
    );
    assert_eq!(run.task_state(ALPHA), TaskState::Pending);
    assert!(
        !dispatched.worktree.exists(),
        "the task worktree is scrubbed with force"
    );
    assert!(
        run.fixture.manager.intents().expect("intents").is_empty(),
        "and its intent left with it"
    );

    let next = run.dispatch(ALPHA, 1);
    assert_eq!(next.generation, GenerationId(1));
    assert_eq!(
        run.emitter.generation_class(ALPHA, GenerationId(1)),
        GenerationClass::OpenNoAttempt
    );
    assert!(next.worktree.is_dir());
    assert_ne!(
        next.worktree, dispatched.worktree,
        "a new generation, a new worktree"
    );
}

#[test]
fn kill_after_capture_leaves_index_referenced_objects_then_scrub_releases_them() {
    let dir = kill_dir("killcapture");
    let mut run = kill_child_and_adopt(CHILD, &dir, "after_capture");
    let dispatched = adopted_generation(&run);
    let mut process = Process::new();

    let staged = index_blobs(&dispatched.worktree);
    assert!(
        !staged.is_empty(),
        "the child's capture left an index with staged blobs"
    );
    let worked = git(&dispatched.worktree, &["hash-object", WORKED_PATH]);
    assert!(
        staged.contains(&worked),
        "the agent's file is staged: {staged:?} does not hold {worked}"
    );

    let before = unreachable_objects(&run.fixture.base).expect("fsck");
    assert!(
        !before.contains(&worked),
        "R9: an object the task index holds is reachable, and fsck reported it unreachable"
    );

    context!(run, process)
        .settle_interrupted(
            &dispatched,
            crate::topology::events::AttemptNumber(1),
            AttemptOutcome::Interrupted,
        )
        .expect("settle");

    assert!(!dispatched.worktree.exists());
    let after = unreachable_objects(&run.fixture.base).expect("fsck");
    assert!(
        after.contains(&worked),
        "R27: the scrub released the staged object to Git, and it is still referenced"
    );
    assert!(
        run.observed(SCRUB, HookPhase::After),
        "the release is the forced scrub's, not something else's"
    );
}

#[test]
fn kill_after_ephemeral_snapshot_commit_before_worktree_leaves_gc_owned_object() {
    let dir = kill_dir("killephemeral");
    let mut run = kill_child_and_adopt(CHILD, &dir, "after_snapshot_commit");
    let dispatched = adopted_generation(&run);
    let mut process = Process::new();

    let orphans = unreachable_ephemeral_commits(&run.fixture.base);
    assert_eq!(
        orphans.len(),
        1,
        "exactly one ephemeral commit, unreferenced: {orphans:?}"
    );
    assert!(
        run.fixture
            .manager
            .intents()
            .expect("intents")
            .iter()
            .all(|slot| !matches!(slot, crate::workspace_manager::Slot::Snapshot { .. })),
        "nothing durable claims it"
    );
    assert!(
        !run.fixture
            .manager
            .worktree_records()
            .expect("worktree records")
            .iter()
            .any(|record| record
                .path()
                .starts_with(run.fixture.manager.execution_root().join("snapshots"))),
        "and no snapshot worktree was ever registered"
    );

    context!(run, process)
        .settle_interrupted(
            &dispatched,
            crate::topology::events::AttemptNumber(1),
            AttemptOutcome::Interrupted,
        )
        .expect("settle");

    assert_eq!(
        unreachable_ephemeral_commits(&run.fixture.base),
        orphans,
        "the recovery leaves an unreferenced object to Git rather than pruning it"
    );
}

#[test]
fn kill_at_snapshot_commit_id_unread_point_leaves_gc_owned_object() {
    let dir = kill_dir("killidunread");
    let run = kill_child_and_adopt(CHILD, &dir, "id_unread");

    let orphans = unreachable_ephemeral_commits(&run.fixture.base);
    assert_eq!(
        orphans.len(),
        1,
        "the object was written before the coordinator could record its id: {orphans:?}"
    );
    assert!(
        run.fixture
            .manager
            .intents()
            .expect("intents")
            .iter()
            .all(|slot| !matches!(slot, crate::workspace_manager::Slot::Snapshot { .. })),
        "and nothing durable names it"
    );
    let refusal = run
        .harness
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .arm(
            SNAPSHOT_COMMIT,
            SubEffectPoint::IdUnread,
            InjectionMode::ErrorReturn,
        )
        .expect_err("IdUnread supports Kill only");
    assert!(
        refusal.to_string().contains("ErrorReturn"),
        "the refusal must name the mode it will not arm: {refusal}"
    );
}

#[test]
fn kill_after_snapshot_add_reclaims_snapshot_and_releases_its_commit() {
    let dir = kill_dir("killsnapshotadd");
    let mut run = kill_child_and_adopt(CHILD, &dir, "after_snapshot_add");
    let dispatched = adopted_generation(&run);
    let mut process = Process::new();

    let snapshots: Vec<crate::workspace_manager::Slot> = run
        .fixture
        .manager
        .intents()
        .expect("intents")
        .into_iter()
        .filter(|slot| matches!(slot, crate::workspace_manager::Slot::Snapshot { .. }))
        .collect();
    assert_eq!(
        snapshots.len(),
        1,
        "one snapshot intent survives: {snapshots:?}"
    );
    let path = run.fixture.manager.slot_path(&snapshots[0]);
    assert!(path.is_dir(), "and its worktree was added");

    let head = git(&path, &["rev-parse", "HEAD"]);
    assert!(
        !unreachable_objects(&run.fixture.base)
            .expect("fsck")
            .contains(&head),
        "R24: the snapshot's HEAD keeps its ephemeral commit reachable"
    );
    assert_eq!(
        git(&run.fixture.base, &["log", "-1", "--format=%s", &head]),
        "upstroke: ephemeral snapshot input"
    );

    context!(run, process)
        .settle_interrupted(
            &dispatched,
            crate::topology::events::AttemptNumber(1),
            AttemptOutcome::Interrupted,
        )
        .expect("settle");

    assert!(!path.exists(), "the snapshot worktree was reclaimed");
    assert!(
        run.fixture.manager.intents().expect("intents").is_empty(),
        "with its intent"
    );
    assert!(
        unreachable_objects(&run.fixture.base)
            .expect("fsck")
            .contains(&head),
        "R27: and the ephemeral commit went back to Git"
    );
}

#[test]
fn kill_during_retry_attempt_closes_generation() {
    let dir = kill_dir("killretry");
    let mut run = kill_child_and_adopt(CHILD, &dir, "retry");
    let dispatched = adopted_generation(&run);
    let mut process = Process::new();

    assert_eq!(
        run.emitter.durable_kinds(),
        vec![
            "run_started",
            "task_dispatched",
            "attempt_started",
            "attempt_finished",
            "attempt_started"
        ],
        "the child retained and then started a retry"
    );
    assert_eq!(
        run.emitter.generation_class(ALPHA, GENERATION),
        GenerationClass::InFlight {
            attempt: crate::topology::events::AttemptNumber(2)
        },
        "the retry re-entered the same generation"
    );

    context!(run, process)
        .settle_interrupted(
            &dispatched,
            crate::topology::events::AttemptNumber(2),
            AttemptOutcome::Interrupted,
        )
        .expect("settle");

    let events = run.emitter.durable_events();
    let crate::topology::events::TopologyEventBody::AttemptInterrupted { data } =
        &events.last().expect("a terminal").body
    else {
        panic!("the last durable event is not an interruption");
    };
    assert_eq!(
        data.attempt,
        crate::topology::events::AttemptNumber(2),
        "the terminal names the retry, not the attempt that retained"
    );
    assert_eq!(
        run.emitter.generation_class(ALPHA, GENERATION),
        GenerationClass::Closed,
        "a generation does not survive an interruption"
    );
    assert_eq!(run.task_state(ALPHA), TaskState::Pending);
    assert!(!dispatched.worktree.exists());
}

#[test]
fn halt_cancels_in_flight_attempt() {
    let mut run = Run::started("halt");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");

    let reviewer = started.identities.review_pass(0, 0);
    process.ledger.register(&reviewer).expect("register");
    process
        .slots
        .acquire(
            &reviewer,
            SlotPair {
                agent: "claude-code".to_owned(),
                pool: Some("scaffold-pool".to_owned()),
            },
        )
        .expect("the pair its role takes");
    assert!(!process.balances(), "the run is genuinely in flight");
    assert_eq!(process.ledger.running(), vec![reviewer.render()]);

    let cancelled = context!(run, process)
        .cancel_in_flight(&dispatched, crate::topology::events::AttemptNumber(1))
        .expect("halt");

    assert_eq!(cancelled, 1, "the in-flight invocation was cancelled");
    assert!(
        process.balances(),
        "and both ledgers balance: slots={:?} running={:?}",
        process.slots.is_empty(),
        process.ledger.running()
    );

    let events = run.emitter.durable_events();
    let crate::topology::events::TopologyEventBody::AttemptInterrupted { data } =
        &events.last().expect("a terminal").body
    else {
        panic!("a halt appends the same terminal an interruption does");
    };
    assert!(data.detail.contains("halted"), "{}", data.detail);
    assert_eq!(
        run.emitter.generation_class(ALPHA, GENERATION),
        GenerationClass::Closed
    );
    assert_eq!(run.task_state(ALPHA), TaskState::Pending);
    assert!(
        !dispatched.worktree.exists(),
        "and the residue is discarded, the same way an interruption discards it"
    );
}

fn stage_elements() -> Vec<ResidueElement> {
    STAGE.residue_elements().to_vec()
}

fn unstaged_work(worktree: &Path) {
    write_file(
        &worktree.join("staging.txt"),
        b"work the interrupted `git add` never finished staging\n",
    );
}

fn plant_stage_residue(base: &Path, worktree: &Path, element: ResidueElement) {
    match element {
        ResidueElement::UnreferencedObject => {
            write_file(
                &worktree.join("orphan.txt"),
                b"an object nothing references\n",
            );
            let id = git(worktree, &["hash-object", "-w", "orphan.txt"]);
            assert!(
                unreachable_objects(base).expect("fsck").contains(&id),
                "the planted blob must really be unreachable"
            );
        }
        ResidueElement::TemporaryObjectFile => {
            let objects = object_directory(worktree).expect("the object directory");
            write_file(&objects.join("tmp_obj_synthetic"), b"half an object\n");
            assert!(temporary_object_files(worktree).expect("temp files"));
        }
        ResidueElement::IndexLock => {
            let dir = PathBuf::from(git(worktree, &["rev-parse", "--absolute-git-dir"]));
            write_file(&dir.join("index.lock"), b"");
        }
        other => panic!("`{other:?}` is not registered for Object.CandidateStage"),
    }
}

#[test]
fn synthetic_git_add_residue_unreferenced_objects_and_index_lock_then_forced_scrub_converges() {
    let elements = stage_elements();
    assert_eq!(
        elements.len(),
        3,
        "Object.CandidateStage registers three elements and this test constructs each: \
         {elements:?}"
    );

    for element in &elements {
        let fixture = Fixture::created("synthetic-stage");
        let manager = &fixture.manager;
        let slot = crate::workspace_manager::Slot::Task {
            key: "synth".to_owned(),
            generation: 0,
        };
        manager.write_intent(&mut NoHooks, &slot).expect("intent");
        let worktree = manager
            .add_worktree(&mut NoHooks, &slot, &fixture.head)
            .expect("worktree");

        let target = ResidueTarget::new(&fixture.base).at(&worktree);
        assert_eq!(
            classify_object_residue(STAGE, &target).expect("classify"),
            ObjectResidue::After,
            "{element:?}: a worktree whose index reflects its tree is the *finished* state"
        );
        unstaged_work(&worktree);
        assert_eq!(
            classify_object_residue(STAGE, &target).expect("classify"),
            ObjectResidue::None,
            "{element:?}: the after-phase reference is now absent and no element is present \
             yet, which is neither class"
        );

        plant_stage_residue(&fixture.base, &worktree, *element);
        assert_eq!(
            observed_residue_elements(STAGE, &target).expect("observe"),
            vec![*element],
            "{element:?}: exactly this element is present, so the classification below is \
             about it and not about a neighbour"
        );
        assert_eq!(
            classify_object_residue(STAGE, &target).expect("classify"),
            ObjectResidue::Internal,
            "{element:?}: the objects-written-reference-unpublished prefix is `Internal`"
        );

        manager
            .remove_worktree(&mut NoHooks, &slot)
            .expect("forced removal converges");
        manager
            .remove_intent(&mut NoHooks, &slot)
            .expect("intent removal converges");
        assert!(!worktree.exists(), "{element:?}: the worktree is gone");
        assert!(
            !manager
                .worktree_records()
                .expect("records")
                .iter()
                .any(|record| crate::util::same_path(record.path(), &worktree)),
            "{element:?}: and it is no longer registered"
        );
        assert!(
            !manager.intent_path(&slot).exists(),
            "{element:?}: and its durable intent left with it"
        );
        manager
            .remove_worktree(&mut NoHooks, &slot)
            .expect("a second removal converges");
        manager
            .remove_intent(&mut NoHooks, &slot)
            .expect("a second intent removal converges");
    }

    let fixture = Fixture::created("synthetic-stage-all");
    let manager = &fixture.manager;
    let slot = crate::workspace_manager::Slot::Task {
        key: "synth".to_owned(),
        generation: 0,
    };
    manager.write_intent(&mut NoHooks, &slot).expect("intent");
    let worktree = manager
        .add_worktree(&mut NoHooks, &slot, &fixture.head)
        .expect("worktree");
    unstaged_work(&worktree);
    for element in &elements {
        plant_stage_residue(&fixture.base, &worktree, *element);
    }
    let target = ResidueTarget::new(&fixture.base).at(&worktree);
    let mut observed = observed_residue_elements(STAGE, &target).expect("observe");
    observed.sort();
    let mut expected = elements.clone();
    expected.sort();
    assert_eq!(observed, expected, "every registered element is present");
    assert_eq!(
        classify_object_residue(STAGE, &target).expect("classify"),
        ObjectResidue::Internal
    );

    let orphan = git(&worktree, &["hash-object", "orphan.txt"]);
    manager
        .remove_worktree(&mut NoHooks, &slot)
        .expect("forced removal converges");
    manager
        .remove_intent(&mut NoHooks, &slot)
        .expect("intent removal converges");
    assert!(!worktree.exists());
    assert!(
        unreachable_objects(&fixture.base)
            .expect("fsck")
            .contains(&orphan),
        "the orphan blob is R27 and is Git's to prune, not the engine's"
    );
    assert!(
        temporary_object_files(&fixture.base).expect("temp files"),
        "and so is the temporary object file"
    );
}

const SAMPLED: [EffectSiteId; 2] = [STAGE, WRITE_TREE];

const SAMPLING_N: u32 = 8;

const HISTOGRAM: &str = "effects/attempt-residue-histogram.json";

struct Sample {
    argv: Vec<String>,
    after: std::time::Duration,
    ran: Option<std::time::Duration>,
    fired: Option<std::time::Duration>,
    killed: bool,
    failed: Option<i32>,
    class: Option<ObjectResidue>,
    recovered: bool,
}

fn bulk(worktree: &Path) {
    for directory in 0..60 {
        for index in 0..20 {
            write_file(
                &worktree.join(format!("bulk{directory}/f{index}.txt")),
                format!("{directory}-{index}-{}", "x".repeat(2048)).as_bytes(),
            );
        }
    }
}

fn sampled_argv(site: EffectSiteId) -> Vec<String> {
    let fixed = |argv: &[&str]| -> Vec<String> { argv.iter().map(|a| (*a).to_owned()).collect() };
    match site {
        STAGE => fixed(&crate::workspace_manager::WorkspaceManager::CANDIDATE_STAGE_ARGV),
        WRITE_TREE => fixed(&crate::workspace_manager::WorkspaceManager::CANDIDATE_WRITE_TREE_ARGV),
        other => panic!("`{other}` is not one of the two capture commands"),
    }
}

fn populate_for(site: EffectSiteId, worktree: &Path) {
    bulk(worktree);
    if site == WRITE_TREE {
        git(worktree, &["add", "-A"]);
    }
}

fn sample_slot(generation: u32) -> crate::workspace_manager::Slot {
    crate::workspace_manager::Slot::Task {
        key: "sample".to_owned(),
        generation,
    }
}

fn measure_budget(site: EffectSiteId, fixture: &Fixture) -> std::time::Duration {
    const PROBE_SLOTS: [u32; 4] = [9_996, 9_997, 9_998, 9_999];

    let mut measured = Vec::with_capacity(PROBE_SLOTS.len());
    for slot_id in PROBE_SLOTS {
        let probe = sample_slot(slot_id);
        fixture
            .manager
            .write_intent(&mut NoHooks, &probe)
            .expect("probe intent");
        let path = fixture
            .manager
            .add_worktree(&mut NoHooks, &probe, &fixture.head)
            .expect("probe worktree");
        populate_for(site, &path);
        measured.push(time_git(&path, &sampled_argv(site)));
        fixture
            .manager
            .remove_worktree(&mut NoHooks, &probe)
            .expect("remove the probe");
        fixture
            .manager
            .remove_intent(&mut NoHooks, &probe)
            .expect("remove the probe intent");
    }
    median(&measured[1..])
}

fn median(durations: &[std::time::Duration]) -> std::time::Duration {
    let mut sorted = durations.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2].max(std::time::Duration::from_micros(200))
}

fn sample(site: EffectSiteId) -> Vec<Sample> {
    let fixture = Fixture::created("sampler");
    let budget = measure_budget(site, &fixture);
    let first = sample_once(site, &fixture, budget, 0);
    if first.iter().any(|sample| sample.killed) {
        return first;
    }

    let observed: Vec<std::time::Duration> = first.iter().filter_map(|sample| sample.ran).collect();
    if observed.is_empty() {
        return first;
    }
    sample_once(site, &fixture, median(&observed), SAMPLING_N)
}

fn sample_once(
    site: EffectSiteId,
    fixture: &Fixture,
    budget: std::time::Duration,
    slot_base: u32,
) -> Vec<Sample> {
    let mut samples = Vec::new();

    for run in 0..SAMPLING_N {
        let slot = sample_slot(slot_base + run);
        fixture
            .manager
            .write_intent(&mut NoHooks, &slot)
            .expect("intent");
        let path = fixture
            .manager
            .add_worktree(&mut NoHooks, &slot, &fixture.head)
            .expect("worktree");
        populate_for(site, &path);

        let argv = sampled_argv(site);
        let after = budget.mul_f64(f64::from(run + 1) / f64::from(SAMPLING_N + 1));
        let mut child = KillableGitChild::spawn(&path, &argv);
        let deadline = std::time::Instant::now() + after;
        let mut ran = None;
        while std::time::Instant::now() < deadline {
            if ran.is_none() {
                ran = child.exited();
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        child.kill();
        let status = child.wait();

        let target = ResidueTarget::new(&fixture.base).at(&path);
        let class = classify_object_residue(site, &target).ok();

        fixture
            .manager
            .remove_worktree(&mut NoHooks, &slot)
            .expect("forced removal converges");
        fixture
            .manager
            .remove_intent(&mut NoHooks, &slot)
            .expect("intent removal converges");
        let recovered = !path.exists()
            && !fixture
                .manager
                .worktree_records()
                .expect("records")
                .iter()
                .any(|record| crate::util::same_path(record.path(), &path));

        samples.push(Sample {
            argv,
            after,
            ran,
            fired: child.fired(),
            killed: died_by_kill(&status),
            failed: (!status.success() && !died_by_kill(&status))
                .then(|| status.code())
                .flatten(),
            class,
            recovered,
        });
    }
    samples
}

#[test]
fn sampled_git_add_and_write_tree_child_kills_every_residue_classified_and_recovered() {
    let mut per_site = Vec::new();
    for site in SAMPLED {
        let samples = sample(site);
        assert_eq!(
            samples.len(),
            SAMPLING_N as usize,
            "{site}: one observation per sample"
        );

        let counted = |wanted: ObjectResidue| -> u32 {
            u32::try_from(
                samples
                    .iter()
                    .filter(|sample| sample.class == Some(wanted))
                    .count(),
            )
            .expect("a sample count fits in u32")
        };
        let (none, internal, after) = (
            counted(ObjectResidue::None),
            counted(ObjectResidue::Internal),
            counted(ObjectResidue::After),
        );
        let unclassified = u32::try_from(
            samples
                .iter()
                .filter(|sample| sample.class.is_none())
                .count(),
        )
        .expect("a sample count fits in u32");
        assert_eq!(
            none + internal + after + unclassified,
            SAMPLING_N,
            "{site}: every sample is accounted for by exactly one class"
        );
        assert_eq!(
            unclassified, 0,
            "{site}: an unclassifiable residue is durable state no tabled action recovers"
        );
        assert!(
            samples.iter().all(|sample| sample.recovered),
            "{site}: every sample recovered by its classified action"
        );

        let failed: Vec<Option<i32>> = samples.iter().filter_map(|s| s.failed.map(Some)).collect();
        assert!(
            failed.is_empty(),
            "{site}: a sampled child neither died by the kill nor reached its own successful \
             exit (codes {failed:?}), so what the classifier saw is this fixture's failure"
        );

        let shape: Vec<&Sample> = samples
            .iter()
            .filter(|sample| sample.argv == sampled_argv(site))
            .collect();
        assert_eq!(
            shape.len(),
            SAMPLING_N as usize,
            "{site}: every sample ran this command and not a neighbouring one"
        );
        let delays: Vec<std::time::Duration> = shape.iter().map(|sample| sample.after).collect();
        assert!(
            delays.windows(2).all(|rungs| rungs[0] < rungs[1]),
            "{site}: the N kills must be aimed at N distinct, increasing points through the \
             command, not at one point N times: {delays:?}"
        );

        for sample in &shape {
            let fired = sample.fired.unwrap_or_else(|| {
                panic!(
                    "{site}: a sampled child was never fired at, so no count over these \
                        samples is about kills"
                )
            });
            assert!(
                fired >= sample.after,
                "{site}: a kill fired {fired:?} after its child was spawned, sooner than the \
                 {:?} rung it was aimed at",
                sample.after
            );
        }

        per_site.push((site, none, internal, after, unclassified, samples));
    }
    assert_eq!(per_site.len(), 2, "the two commands sub-prefix (b') names");

    let landed: usize = per_site
        .iter()
        .map(|(_, _, _, _, _, samples)| samples.iter().filter(|sample| sample.killed).count())
        .sum();
    assert!(
        landed > 0,
        "not one of the {} sampled Git children died by the kill — this harness then sampled \
         the residue its commands left when they FINISHED, and every other assertion here \
         accepts that residue. `sample` has already recalibrated from the durations the runs \
         actually took and retried once, so this is the kill failing to land rather than an \
         unrepresentative probe",
        2 * SAMPLING_N
    );

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(HISTOGRAM);
    let emitted = serde_json::to_string_pretty(&serde_json::json!({
        "note": "decisions.effect_site_inventory.outputs, the observed-class histogram half, \
                 for T-ATTEMPT's capture commands. Written by engine::topology::attempt::tests\
                 ::sampled_git_add_and_write_tree_child_kills_every_residue_classified_and_\
                 recovered on every run. Machine-varying by construction -- which class a \
                 sample lands in is a race between the kill and Git -- so it is emitted here \
                 rather than pinned into effects/residue-classes.json.",
        "sampling_n": SAMPLING_N,
        "sites": per_site
            .iter()
            .map(|(site, none, internal, after, unclassified, samples)| serde_json::json!({
                "site": site.name(),
                "n": SAMPLING_N,
                "none": none,
                "internal": internal,
                "after": after,
                "unclassified": unclassified,
                "killed": samples.iter().filter(|sample| sample.killed).count(),
                "recovered": samples.iter().all(|sample| sample.recovered),
                "ladder_us": samples
                    .iter()
                    .map(|sample| u64::try_from(sample.after.as_micros()).unwrap_or(u64::MAX))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    }))
    .expect("the histogram serializes");
    write_file(&path, (emitted + "\n").as_bytes());

    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read the histogram back"))
            .expect("the emitted histogram parses");
    let sites = back["sites"].as_array().expect("a sites array");
    assert_eq!(sites.len(), 2, "one histogram per sampled command");
    for (entry, (site, ..)) in sites.iter().zip(&per_site) {
        assert_eq!(
            entry["site"],
            site.name(),
            "the sites are in sampling order"
        );
        let total = ["none", "internal", "after", "unclassified"]
            .iter()
            .map(|class| entry[*class].as_u64().expect("a count"))
            .sum::<u64>();
        assert_eq!(
            total,
            u64::from(SAMPLING_N),
            "{site}: the written histogram accounts for every sample"
        );
    }
}

#[test]
fn a_failing_gate_rejects_the_judgement_and_its_snapshot_is_still_cleaned() {
    let mut run = Run::started("rejected");
    run.runner = crate::engine::topology::scaffold::RecordingRunner::failing_with(vec![0, 2]);
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");
    agent_edits(&dispatched.worktree);
    let capture = context!(run, process)
        .capture(dispatched.site())
        .expect("capture");
    let review_inputs = run.review_inputs();
    let assessed = context!(run, process)
        .assess(
            dispatched.site(),
            &plan,
            &started,
            &capture,
            &review_inputs.diff,
            crate::ir::TaskKind::Implement,
        )
        .expect("the scaffold's adapter parses its own worker output");
    let judgement = context!(run, process)
        .judge(
            dispatched.site(),
            &plan,
            Judging {
                run: &started,
                capture: &capture,
                assessed: &assessed,
            },
            &review_inputs,
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    assert!(!judgement.gates[0].passed(), "the gate exited 2");
    assert_eq!(judgement.gates[0].code, Some(2));
    assert!(!judgement.accepted(), "a failed gate rejects the attempt");

    assert!(
        judgement.reviews.is_empty(),
        "no reviewer runs after a gate has already rejected the work"
    );
    assert!(
        run.runner
            .ran()
            .iter()
            .all(|ran| !matches!(ran.role, crate::runner::ExecutionRole::Review)),
        "and none was spawned — asserted on the runner's record, because an \
         empty `reviews` list could also mean a pass that ran and recorded \
         nothing, which is the one thing `AttemptRecord.reviews` must never be \
         ambiguous about"
    );
    assert!(
        run.fixture
            .manager
            .intents()
            .expect("intents")
            .iter()
            .all(|slot| !matches!(slot, crate::workspace_manager::Slot::Snapshot { .. })),
        "every snapshot was cleaned on completion, pass or fail"
    );
    assert!(process.balances());
}

#[test]
fn a_malformed_captured_id_is_a_git_error_naming_where_the_value_came_from() {
    let malformed = "not-an-object-id".to_owned();
    let error = captured_object_id("`git write-tree`", malformed.clone())
        .expect_err("a value that is not an object id");
    let UpstrokeError::Git { message } = &error else {
        panic!("the engine's own malformed value is a Git error, not a refusal: {error}");
    };
    assert!(
        message.contains("`git write-tree` did not yield an object id")
            && message.contains(&malformed),
        "the message names the source and the value: {message}"
    );

    let good = "0123456789abcdef0123456789abcdef01234567".to_owned();
    assert_eq!(
        captured_object_id("the recorded base commit", good.clone())
            .expect("a full hexadecimal id")
            .as_str(),
        good,
        "and a well-formed id passes through unchanged"
    );
}
