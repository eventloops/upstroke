//! `T-ATTEMPT`: five ordering clauses and nine tabled prefixes.

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

/// The bytes an "agent" leaves in the task worktree.
///
/// A file the fixture repository does not already carry, so the blob it stages
/// is one this test can find in the index by name and one nothing else in the
/// object store could be.
const WORKED: &[u8] = b"the agent edited this, and the capture stages it\n";
const WORKED_PATH: &str = "worked.txt";

/// The engine never edits a file; an agent does. A fake runner runs nothing, so
/// the test writes what the worker would have written.
fn agent_edits(worktree: &Path) {
    write_file(&worktree.join(WORKED_PATH), WORKED);
}

/// Every invocation identity of an attempt, and its ledger and slot table.
///
/// A helper rather than a repeated block, because the two ledgers are
/// process-lifetime state that must be *one* per run: a test that built a fresh
/// [`SlotAssertion`] per call would assert a single slotted invocation against
/// an empty table and never see the overlap the assertion exists to catch.
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

    /// The two process-end conditions, together.
    fn balances(&self) -> bool {
        self.slots.balances() && self.ledger.balances()
    }
}

/// The context one attempt runs in, built from disjoint fields of the run.
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

/// The blob ids the task worktree's index holds.
fn index_blobs(worktree: &Path) -> Vec<String> {
    git(worktree, &["ls-files", "-s"])
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .collect()
}

/// Every unreachable commit whose message is the ephemeral snapshot input's.
///
/// The message is `WorkspaceManager::snapshot_commit_tree`'s own, so this
/// identifies the object by what wrote it rather than by an id the killed child
/// never got to report. `--no-dangling` is already applied by
/// [`unreachable_objects`], so what comes back is exactly R27.
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

// ---------------------------------------------------------------------------
// O23, O25, O26, O27 — the order of one clean attempt
// ---------------------------------------------------------------------------

/// **O23.** `attempt_started` is durable before the worker exists.
///
/// The runner records every request it is given, so "before any spawn" is
/// checkable directly: at the moment the first request arrives the log on disk
/// must already carry the event. It is asserted two ways for the reason O21's
/// test gives — the harness order proves the *sequence*, and reading the bytes
/// back proves the append was not merely issued first but landed first.
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

    // **The oracle.** Every record above is read after both the append and the
    // spawn, so a `start` that spawned first and appended afterwards leaves all
    // of them identical — measured, this test stayed green under exactly that
    // reordering. `durable_at_spawn` is the log as it stood *at the instant the
    // process was requested*, which is the only moment the clause is about.
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

/// **O25, O26 and O27** in one clean attempt, read off the shared harness.
///
/// One test rather than three, because the clauses are one chain and splitting
/// them would let a reordering pass two of the three: a snapshot taken before
/// the capture, an intent written before the commit, or a reviewer running on
/// the gate set's snapshot are all failures of *the same sequence*.
///
/// The last position asserted is the last `Snapshot.Remove`, which is what
/// makes O27 checkable inside this lane at all: `candidate.rs` owns the
/// commit-tree, so what this module owes is that nothing here reaches it and
/// that every judgement is finished before anything could.
///
/// O26 is asserted **per snapshot**, from a fence that advances past each
/// one's add. A comparison of first observations would be a comparison of the
/// gate set's triple and of nothing else — the two reviewer snapshots would
/// execute unasserted, and a reviewer path that wrote its intent before its
/// commit would pass. The count check above the loop is what makes the loop
/// exhaustive rather than merely repeated.
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
    // Built before the context borrows `run` mutably.
    let review_inputs = run.review_inputs();
    // Through the production phase, over the same diff the reviewers are
    // shown: a fixture-built `Assessment` could show the judge a diff the
    // cheap rungs never saw.
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
            // Caller-supplied, ordinal included: nothing pass-shaped is
            // minted inside `judge`, so PR8's merge verification can
            // supply its `SequenceIdentities` here without a redesign.
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    // O25: both capture sites, in order, before any snapshot effect.
    let stage = run.must_order_of(STAGE, HookPhase::Before);
    let write_tree = run.must_order_of(WRITE_TREE, HookPhase::Before);
    let commit = run.must_order_of(SNAPSHOT_COMMIT, HookPhase::Before);
    assert!(
        stage < write_tree && write_tree < commit,
        "O25: capture (stage={stage}, write-tree={write_tree}) precedes the snapshots \
         (commit={commit})"
    );

    // O26, once per snapshot rather than once per test. Three snapshots are
    // created here — one for the gate set and one per reviewer — and comparing
    // *first* observations compares only the gate set's triple: a reviewer
    // snapshot that wrote its intent before its ephemeral commit would leave
    // that triple untouched and pass. Each iteration takes its fence past the
    // previous snapshot's add, so the three positions it compares are that
    // snapshot's own.
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

    // O27: everything judged, and nothing here is a commit-tree.
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

    // The tree the capture produced is the tree the snapshots were taken of.
    assert_eq!(
        capture.tree,
        git(&dispatched.worktree, &["write-tree"]),
        "the recorded tree is the worktree's index"
    );
    assert_eq!(capture.parent, dispatched.base.0);
}

/// **`decisions.workspace_candidates.snapshots`.** Gates and reviewers execute
/// only in exact snapshots, one per role, never reused, and never in the task
/// worktree.
///
/// Four claims, and the fourth is the one a weaker test misses: "worker
/// worktrees and the staging worktree are **never** used for verification
/// processes". Every workspace the runner was given is checked against the task
/// worktree, so a `judge` that ran a gate in place would fail here rather than
/// merely producing a snapshot nobody used.
///
/// "Never reused" is checked by counting distinct workspaces, not by counting
/// snapshot adds: three adds that all returned one path would pass a count of
/// adds and fail this.
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
    // Built before the context borrows `run` mutably.
    let review_inputs = run.review_inputs();
    // Through the production phase, over the same diff the reviewers are
    // shown: a fixture-built `Assessment` could show the judge a diff the
    // cheap rungs never saw.
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
            // Caller-supplied, ordinal included: nothing pass-shaped is
            // minted inside `judge`, so PR8's merge verification can
            // supply its `SequenceIdentities` here without a redesign.
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    // **Where the processes actually ran, not where the judgement says they
    // ran.** This used to read `Verdict::workspace` from both lists, and
    // `Judgement.reviews` is now `Vec<ReviewRecord>` — a wire type, which has
    // no path to carry and should not grow one. The runner's record is the
    // better evidence anyway: it observes the request each process was spawned
    // with, so a `judge` that reported one workspace and spawned in another
    // fails here, and the old assertion could not have seen that.
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
    // One shared snapshot for the gate set; one fresh per reviewer.
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

    // Cleaned on completion: nothing survives the judgement.
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

/// **`permits.agent_pool_slots`.** The worker and the reviewers take a slot
/// pair; the gates take none.
///
/// The exclusion is the whole content of the clause — "gate invocations and the
/// shell probe acquire no slot" — and a scheduler that gave a gate one would
/// halve the parallelism of every run without failing anything else. It is
/// asserted from **both** sides: [`is_slotted`] over each identity, and the
/// [`SlotAssertion`] refusing a gate outright.
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
    // Through the production phase, over the same diff the reviewers are
    // shown: a fixture-built `Assessment` could show the judge a diff the
    // cheap rungs never saw.
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
            // Caller-supplied, ordinal included: nothing pass-shaped is
            // minted inside `judge`, so PR8's merge verification can
            // supply its `SequenceIdentities` here without a redesign.
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    // Every process really went through the run's Runner, with the identity
    // this attempt assigns and the seat its role gives it.
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

// ---------------------------------------------------------------------------
// O24 — the retry
// ---------------------------------------------------------------------------

/// The retained cumulative tree of `worktree`, staged.
///
/// A retained generation "holds the retained cumulative tree", and that is what
/// its retry verifies against — not the base. Producing it here writes objects,
/// which is what `git write-tree` does and why `Worktree.Verify` may not run it
/// (`PR5-CONF-002`).
fn retained_tree(worktree: &Path) -> String {
    agent_edits(worktree);
    git(worktree, &["add", "-A"]);
    git(worktree, &["write-tree"])
}

/// The first two steps of O24, which are [`settle::retry`]'s: the provisional
/// `{pipeline}` reservation and the **one** `Worktree.Verify` against the
/// retained cumulative tree.
///
/// Driven through the production seam — [`ManagedWorktrees`] over the run's
/// real [`WorkspaceManager`] — rather than through a double, because the join
/// between the clause's two owners is the thing under test. `attempt.rs` has no
/// retry entry point of its own, so a test that reached the append without
/// going through here would be covering a composition no coordinator can take.
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

/// The plan this module appends, built from the event `settle::retry`
/// authorized.
///
/// Every field the fold checks is taken from that event rather than written as
/// a literal beside it. A plan that disagreed with the authorization would then
/// be the fold's refusal at the append, which is the whole point of the two
/// halves naming the same attempt.
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

/// **O24, across both of its owners.** A retry verifies **once**, then appends
/// exactly the event that verification authorized, then spawns.
///
/// The clause is "reservation, worktree verification, `attempt_started`
/// (retry), spawn" and it has two owners. [`settle::retry`] takes the
/// `{pipeline}` reservation and performs the single `Worktree.Verify`;
/// [`AttemptContext::start`] appends and spawns. There is no retry entry point
/// on this side, so this test drives the join, and the order is asserted as
/// positions in the one harness list plus the runner's own log — a verify after
/// the append would let a retry start against a worktree nothing had looked at,
/// and a spawn before the append would put a paid-for process outside the log.
///
/// **The count of verifications is an assertion, not a detail.** A second
/// observation on the attempt side would be a second implementation of O24's
/// verification, and its refusal would be a pre-append failure — which
/// `permits.provisional_reservations` requires to cancel the reservation. But
/// the cancellation lives in the *first* verify's failure branch, and that
/// verify passed, so the branch is not taken: the reservation would be neither
/// converted nor cancelled. One observation is what makes the reservation's two
/// outcomes exhaustive, and `count_after(VERIFY)` is where that is checked.
///
/// The quiescence is `HoldsTree`, the retained generation's form of the check,
/// and the tree is deliberately made to differ from the base's so a
/// verification against `AtBase` could not pass in its place.
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

    // Steps one and two, which are `settle::retry`'s.
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

    // Steps three and four, which are this module's.
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

    // "Reused" as a fact about the worktree rather than as a tag any call
    // returned: nothing was removed after the mark, and the cumulative tree the
    // generation retained is still the one the worktree holds.
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

    // The two owners named the same attempt. This module builds its
    // `attempt_started` from `dispatched` and the plan, and `settle::retry`
    // built its own from the fold; the bytes on disk are what says they agree.
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

/// The git directory of a linked worktree, asked of Git rather than derived.
///
/// `<worktree>/.git` is a **file** in a linked worktree and the administrative
/// directory it points at is elsewhere, so deriving the path here would be a
/// second implementation of `workspace_manager`'s own `git_dir_of`.
fn git_dir(worktree: &Path) -> PathBuf {
    PathBuf::from(git(worktree, &["rev-parse", "--absolute-git-dir"]))
}

/// **INV-06 / O24.** A retry whose retained worktree fails `Worktree.Verify`
/// closes the generation, cancels its reservation, and destroys nothing.
///
/// `decisions.workspace_candidates.generation` gives the failure two recoveries
/// and they are not interchangeable: "failing verification an OpenNoAttempt or
/// repair worktree is removed with force and recreated, **and a RetainedIdle
/// generation is closed with `generation_closed{WorktreeMissing}`**". A retry
/// that took the first branch would force-remove the worktree — and a retained
/// worktree's whole content is a cumulative tree that **no base can be re-cut
/// into**, which is what INV-06's "never recreated" protects — and would then
/// append `attempt_started(retry)` carrying `resume_session`, so the next
/// worker would run against an empty tree and be gated as if it were the
/// retained work. The append is durable before any caller sees the outcome, so
/// there is no later place to catch it.
///
/// The recovery is driven end to end here rather than only observed: the
/// closure `settle::retry` builds is appended through the same fold-checked
/// emitter every other event uses, so "the generation closes" is a transition
/// the fold accepted and not a struct this test looked at.
///
/// Six assertions, and each is a different way the destructive branch or a
/// stranded reservation would show: nothing was removed, nothing was appended
/// before the closure, no process was asked for, the tree the worktree holds is
/// byte-for-byte the tree it held, the generation ends `Closed` rather than
/// rebuilt, and the `{pipeline}` reservation is **cancelled** —
/// `permits.provisional_reservations` requires "cancellation on any pre-append
/// failure", and a retry entry point that refused *after* this verify passed
/// would leave it held with nobody to settle it.
///
/// The residue planted is `index.lock`, which is exactly what the interrupted
/// Git command in the failure sequence leaves, and it is the cheapest way into
/// a failing verify that does not itself disturb the thing being protected.
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

    // An interrupted Git command, which is the case `Worktree.Verify` exists
    // for and the one the failure sequence describes.
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

    // It looked, which is what stops every assertion below being about a
    // function that returned without doing anything.
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

    // The recovery itself, through the fold that has to accept it.
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

    // The work itself. The lock is what `Worktree.Verify` refused for, so it
    // comes off before the index can be written out — removing it is this
    // test's own act and not part of what the retry did.
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

/// **R4 / `permits.protocol`.** A slot acquisition the assertion refuses must
/// not leave the invocation registered.
///
/// "The invocation ledger records registered/completed/cancelled exactly once
/// and **balances at process end**", and [`InvocationLedger::balances`] states
/// that as "no entry is `Running`". The register happens before the pair is
/// asked for, so a refused acquisition that propagated straight out would
/// abandon a `Running` entry — and at process end that entry is
/// *indistinguishable* from a process this coordinator genuinely lost. A leak
/// check that cannot tell a bookkeeping mistake from a lost process reports
/// both or neither.
///
/// The refusal is driven the way a real one arrives: a pair is already held.
/// At `max_parallel = 1` [`SlotAssertion`] refuses rather than queues, which is
/// its whole purpose, so this is the refusal the substrate actually produces
/// rather than a synthetic error injected at the seam.
///
/// The held pair is deliberately **not** registered in the ledger, so the
/// ledger's own balance is a statement about the worker alone: after the
/// refusal nothing is running, one entry is cancelled, and none is completed.
#[test]
fn a_refused_slot_acquisition_settles_the_registration_it_took() {
    let mut run = Run::started("slotrefusal");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    // Something else holds the one pair. `cancel_all_running` is not involved:
    // this invocation is in the slot table and not in the ledger.
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

    // What actually happened, so the assertions above are about the state this
    // test claims to have driven: O23's append is durable and no process was
    // ever asked for.
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

// ---------------------------------------------------------------------------
// T-ATTEMPT — the kills
// ---------------------------------------------------------------------------

/// The child every `T-ATTEMPT` kill test spawns.
///
/// One child with a site switch rather than six children, because every one of
/// them needs the same prefix built — a run, a dispatch, an attempt, and for
/// most of them a capture — and six copies of that prefix would be six chances
/// for one of them to build a different state than its name claims.
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
        // Sub-prefix (a): the worker ran and no capture has begun.
        run.arm(STAGE, HookPhase::Before, Injection::Kill);
        let _ = context!(run, process).capture(dispatched.site());
        unreachable!("the kill must have taken this process");
    }

    if which == "retry" {
        // The retry's own in-flight prefix, in the generation that retained,
        // built through both owners of O24 exactly as the parent test's
        // non-kill sibling does.
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
        // In flight, and now killed inside it: the arming is at the capture
        // because `retry` itself must succeed for the generation to be
        // `InFlight { attempt: 2 }` when the coordinator dies.
        let _ = context!(run, process).capture(dispatched.site());
        unreachable!("the kill must have taken this process");
    }

    agent_edits(&dispatched.worktree);
    if which == "after_capture" {
        // Sub-prefix (b): the staged blob and tree objects exist and are
        // referenced only by the task worktree's index.
        run.arm(WRITE_TREE, HookPhase::After, Injection::Kill);
        let _ = context!(run, process).capture(dispatched.site());
        unreachable!("the kill must have taken this process");
    }

    let capture = context!(run, process)
        .capture(dispatched.site())
        .expect("capture");
    match which.as_str() {
        // Sub-prefix (c), the after phase: the id was read and nothing durable
        // claims the commit.
        "after_snapshot_commit" => run.arm(SNAPSHOT_COMMIT, HookPhase::After, Injection::Kill),
        // Sub-prefix (c), the `IdUnread` point: the child exited with the
        // object written and the coordinator never recorded the id. Armed on
        // the shared harness, because a point is a real injection coordinate
        // and `IdUnread` supports `Kill` alone.
        "id_unread" => run.arm_point(
            SNAPSHOT_COMMIT,
            SubEffectPoint::IdUnread,
            InjectionMode::Kill,
        ),
        // Sub-prefix (d): the intent is durable and the snapshot worktree
        // registered, so its HEAD holds the ephemeral commit (R24).
        "after_snapshot_add" => run.arm(SNAPSHOT_ADD, HookPhase::After, Injection::Kill),
        other => panic!("unknown site `{other}`"),
    }
    let review_inputs = run.review_inputs();
    // Through the production phase, over the same diff the reviewers are
    // shown: a fixture-built `Assessment` could show the judge a diff the
    // cheap rungs never saw.
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
        // Caller-supplied, ordinal included: nothing pass-shaped is
        // minted inside `judge`, so PR8's merge verification can
        // supply its `SequenceIdentities` here without a redesign.
        &|pass| crate::review::ReviewInvocations {
            pass: started.identities.review_pass(pass, 0),
            reask: started.identities.review_reask(pass, 0),
        },
    );
    unreachable!("the kill must have taken this process");
}

/// The dispatched generation of the child's run, rebuilt in the parent.
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
            paths: run.predicted(),
        },
    }
}

const CHILD: &str = "engine::topology::attempt::tests::attempt_kill_child";

/// **`T-ATTEMPT`.** A kill during an attempt settles `attempt_interrupted`, and
/// the task is redispatched into a **new** generation.
///
/// Every clause of the tabled resume action is asserted: the terminal is
/// appended with the lease disposition its kind gives, the generation goes
/// `Closed`, the task returns `Pending`, the residue is discarded, and the next
/// dispatch opens generation 1 rather than reopening generation 0. The last is
/// the one that matters most — "later dispatch **new generation** (spend may
/// repeat)" — because a recovery that reused the generation would silently
/// claim the dead coordinator's unknown spend as its own.
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

    // The redispatch: a new generation, at the same base, and the fold accepts
    // it — which it would not if the old generation were still open.
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

/// **`T-ATTEMPT`, sub-prefix (b).** The staged objects are referenced by the
/// task index while the worktree stands, and the forced scrub releases them to
/// R27.
///
/// Both halves are the claim. "Referenced only by the task worktree index (R9)"
/// is checked by `git fsck --unreachable` **not** reporting the blob — the index
/// is one of fsck's roots, so an object it holds is reachable — and the release
/// is the same query answering differently after the scrub. Asserting only the
/// second would pass for an object that was already unreachable before the
/// scrub ran.
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

/// **`T-ATTEMPT`, sub-prefix (c).** An ephemeral snapshot commit written before
/// any intent is Git's, and there is nothing to reclaim.
///
/// The object is identified by the message `snapshot_commit_tree` writes rather
/// than by an id, because the point of this prefix is that the coordinator died
/// without recording one. What is asserted beside its presence is the
/// *absence* of everything that would make it the engine's: no snapshot intent,
/// no snapshot worktree, and after the tabled recovery the object is still
/// there — "an ephemeral commit without a snapshot … is left to Git (nothing to
/// reclaim)". An engine that pruned it would be establishing authority over the
/// object store.
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
                .path
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

/// **`T-ATTEMPT`, sub-prefix (c), the `IdUnread` point.**
///
/// The same durable residue as the test above and a different way of reaching
/// it: the child exited with the object written and the coordinator never read
/// the printed id. `Object.SnapshotCommitTree` exposes the point and
/// `SubEffectPoint::IdUnread` supports **`Kill` only** — it has no error-return
/// contract, and inventing one would be inventing a resume action nothing
/// tables.
///
/// What proves the kill landed *at the point* rather than somewhere else is
/// the child's own `unreachable!`: nothing else in that path is armed, so a
/// point that was never consulted would let `judge` finish and the child would
/// fail rather than die.
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
    // The point supports one mode, and arming the other is refused rather than
    // silently ignored — which is what stops a suite claiming coverage of an
    // error contract this point does not have.
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

/// **`T-ATTEMPT`, sub-prefix (d).** A snapshot whose add completed is reclaimed
/// by its intent, and its ephemeral commit returns to R27.
///
/// The two states are asserted on either side of the reclaim: while the
/// snapshot stands its HEAD references the commit (R24), so fsck does not
/// report it; once the snapshot is removed nothing does, so fsck does. A test
/// that checked only the second would pass against a snapshot that never
/// referenced the commit at all.
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

/// **`T-RETRY` meeting `T-ATTEMPT`.** A kill during a retry closes the
/// generation it was retrying.
///
/// The distinction this holds is the one `generation` draws: "a same-session
/// retry re-enters InFlight in the **same** generation", and an interruption of
/// it closes that generation rather than retaining it — "the generation does
/// *not* survive an interruption". So the recovered state is generation 0
/// `Closed` with attempt **2** named in the terminal, and the retained session
/// is gone with it.
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

/// **ST-17.** "at Halted the same terminal is appended by cancellation".
///
/// Two things, and only the second needs constructing. The terminal is the
/// same `attempt_interrupted` — an interruption is a statement about a
/// coordinator, not a judgement of the work — with a detail that says the run
/// halted.
///
/// The in-flight *invocation* is built directly, because a synchronous
/// substrate cannot leave one any other way: `Runner::run` returns before the
/// coordinator can observe a halt, so a registration that never settled is
/// exactly the state a halt arriving **during** a run leaves, and the honest
/// way to test the cancellation is to put the ledgers in it. Both ledgers are
/// then required to balance, which is the process-end condition
/// `permits.protocol` states.
#[test]
fn halt_cancels_in_flight_attempt() {
    let mut run = Run::started("halt");
    let dispatched = run.dispatch(ALPHA, 0);
    let plan = run.attempt_plan(ALPHA, 1);
    let mut process = Process::new();

    let started = context!(run, process)
        .start(dispatched.site(), &plan)
        .expect("start");

    // A reviewer whose completion never ran, holding the pair its role takes.
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

// ---------------------------------------------------------------------------
// The `Internal` residue class of the two capture commands
//
// `command_internal_sub_effects` gives this class two kinds of evidence and
// both are here: **(i)** synthetic construction of every registered element,
// each classifying `Internal` and recovering by the tabled action, and **(ii)**
// a real-command kill-sampling record with the observed-class histogram. It is
// deliberately *recovery-proven rather than execution-observed*: a killed
// `git add` is not a hook point, so nothing can stand inside it and say "this
// is what it left". A never-hit `Internal` does not fail; an **unclassifiable**
// residue does.
// ---------------------------------------------------------------------------

/// The three elements `Object.CandidateStage` registers, planted one at a time.
///
/// Read off the frozen enum rather than written out, so an element added to the
/// site fails this until it is constructed — `bounded_grid`, the failure this
/// project has recorded three times, is a grid over the elements its author
/// remembered.
fn stage_elements() -> Vec<ResidueElement> {
    STAGE.residue_elements().to_vec()
}

/// The half of an interrupted `git add` that is not an element: work in the
/// tree that the command had not finished staging.
///
/// `command_internal_sub_effects` defines the class as the elements "**with the
/// after-phase reference absent**", and the order in `classify_object_residue`
/// is that sentence's — the after-phase reference decides `After` first, and
/// only its absence lets residue decide `Internal`. For
/// `Object.CandidateStage` the after-phase reference is "an index that reflects
/// the working tree", so a worktree whose index is clean classifies `After`
/// however much R27 residue is lying around, and correctly: a `git add` that
/// finished is not one that was killed. Measured — a temporary object file
/// planted in a pristine worktree classifies `After`.
///
/// So every synthetic element is planted into a worktree that also carries
/// unstaged work, which is what a `git add` killed part-way through leaves.
fn unstaged_work(worktree: &Path) {
    write_file(
        &worktree.join("staging.txt"),
        b"work the interrupted `git add` never finished staging\n",
    );
}

/// Plant one element of `Object.CandidateStage`'s residue in `worktree`.
///
/// The two object-store elements are R27 — Git's — and live in the **shared**
/// object directory, which is why they survive the scrub below while the
/// index lock does not. That difference is the point of planting them
/// separately rather than as one blob of "residue".
fn plant_stage_residue(base: &Path, worktree: &Path, element: ResidueElement) {
    match element {
        ResidueElement::UnreferencedObject => {
            // An orphan blob: written into the store and referenced by nothing.
            // Untracked on purpose — the index must not hold it, or it would be
            // reachable and the classifier would be right to ignore it.
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

/// **`T-ATTEMPT`, sub-prefix (b'), evidence (i).** Every residue element a
/// killed `git add` can leave, constructed, classified `Internal`, and
/// recovered by the tabled forced scrub.
///
/// **A repository per element.** Two of the three live in the *shared* object
/// store and are permanent until Git prunes them, so planting them in sequence
/// in one repository would leave the second element's slot carrying the first's
/// and a classifier that recognised only `UnreferencedObject` would answer
/// `Internal` for all three. Measured: it did.
///
/// Convergence is asserted for what the scrub owns and **not** for what it does
/// not. The lock leaves with the worktree's git dir; the orphan blob and the
/// temporary object file are R27 and stay, because "objects left unreferenced
/// by any of these prunings … are Git's" and an engine that pruned them would
/// be establishing authority over the object store, which `cleanup` forbids.
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

        // Two controls, and the pair is what makes the `Internal` below mean
        // something. A classifier that answered `Internal` unconditionally
        // fails the first; one that ignored its element list and read only the
        // after-phase reference fails the second.
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

        // The tabled recovery: forced removal of the worktree, then its intent.
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
                .any(|record| crate::util::same_path(&record.path, &worktree)),
            "{element:?}: and it is no longer registered"
        );
        assert!(
            !manager.intent_path(&slot).exists(),
            "{element:?}: and its durable intent left with it"
        );
        // Idempotent, which `cleanup` requires of every reclaim.
        manager
            .remove_worktree(&mut NoHooks, &slot)
            .expect("a second removal converges");
        manager
            .remove_intent(&mut NoHooks, &slot)
            .expect("a second intent removal converges");
    }

    // And all three at once, which is the state a killed `git add` leaves.
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

// ---------------------------------------------------------------------------
// Evidence (ii): the real-command kill-sampling record
// ---------------------------------------------------------------------------

/// The two commands `T-ATTEMPT`'s sub-prefix (b') names: "git add or write-tree
/// killed after writing objects and before publishing the index or cache-tree".
const SAMPLED: [EffectSiteId; 2] = [STAGE, WRITE_TREE];

/// The frozen sample count, per command.
///
/// `command_internal_sub_effects`: "the Git child of the site is killed at
/// uncontrolled points through the process funnel across N runs (N frozen per
/// site in the registry)". The claim each sample carries is *per sample* —
/// every observed residue classifies into exactly one class and recovers by the
/// classified action — and is not a coverage claim about the classes, which is
/// why N does not have to be large enough to hit `Internal`.
const SAMPLING_N: u32 = 8;

/// The observed-class histogram, which is a property of the machine and cannot
/// be pinned.
///
/// `effect_site_inventory.outputs` asks for "sampling N **and observed-class
/// histogram**" per site. Which class a sample lands in is a race between the
/// kill and Git, so it goes to a machine-varying evidence file rather than into
/// a byte-compared artifact — the same split, and for the same reason, as
/// `effects/residue-histogram.json`. This file is that one's `T-ATTEMPT`
/// sibling and is written to a **different path** so the two samplers cannot
/// overwrite each other's record.
const HISTOGRAM: &str = "effects/attempt-residue-histogram.json";

/// One sample: what it ran, which rung its kill was aimed at, when the kill
/// actually fired, how the child ended, and what the classifier answered.
struct Sample {
    argv: Vec<String>,
    after: std::time::Duration,
    /// The child's **own** duration, when it finished before the kill.
    ///
    /// `None` when the kill got there first, which is the case this harness
    /// wants. When every sample is `Some`, the schedule raced a number that
    /// does not describe these runs, and these are the durations to rebuild it
    /// from.
    ran: Option<std::time::Duration>,
    fired: Option<std::time::Duration>,
    killed: bool,
    failed: Option<i32>,
    class: Option<ObjectResidue>,
    recovered: bool,
}

/// Enough work in the worktree that the sampled command has a middle to be
/// killed in.
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

/// The exact argv the site's funnel runs.
///
/// Read from the funnel's own frozen lists, never transcribed: a funnel that
/// grew a flag beside a transcribed copy would leave this sampler killing a
/// stale command with every assertion here still green.
fn sampled_argv(site: EffectSiteId) -> Vec<String> {
    let fixed = |argv: &[&str]| -> Vec<String> { argv.iter().map(|a| (*a).to_owned()).collect() };
    match site {
        STAGE => fixed(&crate::workspace_manager::WorkspaceManager::CANDIDATE_STAGE_ARGV),
        WRITE_TREE => fixed(&crate::workspace_manager::WorkspaceManager::CANDIDATE_WRITE_TREE_ARGV),
        other => panic!("`{other}` is not one of the two capture commands"),
    }
}

/// Populate a worktree for `site`, leaving it in the state the funnel would
/// find it in.
fn populate_for(site: EffectSiteId, worktree: &Path) {
    bulk(worktree);
    if site == WRITE_TREE {
        // `write-tree` reads an index, so the bulk has to be in one.
        git(worktree, &["add", "-A"]);
    }
}

/// A slot of the sampling fixture.
fn sample_slot(generation: u32) -> crate::workspace_manager::Slot {
    crate::workspace_manager::Slot::Task {
        key: "sample".to_owned(),
        generation,
    }
}

/// How long the same command takes when nothing kills it.
///
/// Measured in a **probe slot of its own**, which is then removed. Measuring it
/// in the worktree the next sample will kill in makes the probe *perform* the
/// command first, and the samples then classify a fixture artefact rather than
/// a kill — the "environment assumption in a test" class this project has
/// recorded.
/// A duration the sampled command plausibly takes, measured **warm**.
///
/// **The first invocation is the one that lies.** A cold worktree pays for a
/// filesystem cache miss and, on Windows CI, for an antivirus scan of files it
/// has just seen created — so a budget taken from run one is inflated relative
/// to every run that follows, and a schedule derived from it puts every kill
/// after its child has already exited. Measured: two consecutive
/// `test (windows-latest)` legs at `b07b8cc` in which **zero of sixteen**
/// sampled kills landed, on a commit that changed one line of a Markdown file.
///
/// So one run is discarded as warm-up and the median of the next three is
/// taken. The median rather than the mean because the failure mode is a single
/// outlier, and a mean carries an outlier's weight into the schedule that a
/// median discards.
fn measure_budget(site: EffectSiteId, fixture: &Fixture) -> std::time::Duration {
    /// Slots the probes use, distinct from every sampled run's.
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
    // Discard the warm-up, then the median of what is left.
    median(&measured[1..])
}

/// The median of a non-empty slice of durations.
fn median(durations: &[std::time::Duration]) -> std::time::Duration {
    let mut sorted = durations.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2].max(std::time::Duration::from_micros(200))
}

/// Sample one command `SAMPLING_N` times and classify what each kill left.
///
/// **Self-healing against an unrepresentative probe, and only against that.**
/// The schedule races a measured duration, so a budget that does not describe
/// the runs it schedules puts every kill after its child has exited and the
/// sampling observes nothing. When that happens the runs themselves are the
/// better measurement — an unkilled run ran to completion, so its duration is
/// the true one — and the schedule is rebuilt from their median and retried
/// **once**.
///
/// Bounded at one retry on purpose. A second miss is not an unlucky probe; it
/// is the kill failing to land at all, which is a defect this harness exists to
/// report. The caller's vacuity refusal is what reports it, and nothing here
/// weakens that assertion — this only removes the case where it fires for an
/// environment rather than for a bug.
fn sample(site: EffectSiteId) -> Vec<Sample> {
    let fixture = Fixture::created("sampler");
    let budget = measure_budget(site, &fixture);
    let first = sample_once(site, &fixture, budget, 0);
    if first.iter().any(|sample| sample.killed) {
        return first;
    }

    // Premise failed: no kill landed mid-run. Every run therefore completed, so
    // every `ran` is a full duration and their median is the budget the probe
    // should have produced.
    let observed: Vec<std::time::Duration> = first.iter().filter_map(|sample| sample.ran).collect();
    if observed.is_empty() {
        // Nothing landed and nothing finished either: the schedule is not the
        // explanation, so there is nothing honest to recalibrate from. Hand
        // back the first pass and let the caller's vacuity refusal report it.
        return first;
    }
    sample_once(site, &fixture, median(&observed), SAMPLING_N)
}

/// One pass of `SAMPLING_N` runs against `budget`, taking slots from
/// `slot_base` so a retry never reuses the first pass's worktrees.
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
        // Sleep the schedule, but notice if the child finishes first — and
        // record its OWN duration when it does. Wall time to the reap would
        // include this sleep, so an over-long schedule would report itself
        // back as the number it should have been, and the recalibration below
        // would inherit exactly the error it exists to correct. Measured: it
        // did, on the first version of this fix.
        // Poll to the deadline WITHOUT shortening it. Noticing that the child
        // finished is a measurement; acting on it is not. Breaking out early
        // and killing there fires the kill sooner than the rung it was aimed
        // at, which the shape assertions below refuse — measured on the
        // Windows guest, where a kill fired at 40.3ms against a 48.5ms rung.
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

        // The tabled recovery for every class this prefix can leave: forced
        // removal of the worktree, then its intent. Idempotent for `None` and
        // `After`, which is why one action covers all three.
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
                .any(|record| crate::util::same_path(&record.path, &path));

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

/// **`T-ATTEMPT`, sub-prefix (b'), evidence (ii).** Real `git add` and
/// `write-tree` children, killed at N uncontrolled points each; every observed
/// residue classifies into exactly one class and recovers by that class's
/// action.
///
/// # What is asserted and what is only recorded
///
/// The class counts are **not** asserted: which class a sample lands in is a
/// race between the kill and Git, so a suite that required `Internal` would be
/// red whenever the machine was fast. What is asserted is that every sample
/// classified into one of the three and recovered, and that `unclassified` is
/// zero — an unclassifiable residue is durable state no tabled action recovers,
/// and that is the failure this evidence exists to exclude.
///
/// # The oracles that a green completion does not also satisfy
///
/// A sampler whose kills all missed would still spawn `2 × N` children, still
/// classify a legal residue from each, still recover, and still write its
/// evidence file — of *completion* residue, filed under the kill's name. Three
/// things separate the two, and all three are here:
///
/// * **the ladder**, asserted per command: N kills aimed at N distinct,
///   increasing points, because the clause says "killed at **uncontrolled
///   points**" and one fixed delay is one point sampled N times;
/// * **every child fired at**, asserted per command and exactly, because
///   `fired` is written inside `KillableGitChild::kill` and a kill that was
///   skipped leaves no record to count;
/// * **at least one kill landing**, asserted over the sampling as a whole,
///   because only a wait status distinguishes a killed child from a finished
///   one.
///
/// The floor is over the whole sampling and not per command deliberately.
/// `git add` measures roughly **1 in 8** on this project's machines — the
/// budget probe writes the very blobs the samples then find already in the
/// object store, so a sample runs in about a fifth of the time its ladder was
/// scaled to — and a per-command floor would stand on a margin of one sample
/// and be red on the next machine. The per-command counts are recorded in the
/// evidence file so the margin stays visible without being load-bearing.
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

        // The classifier's answers, tallied here rather than by the code under
        // test: a histogram that counted a class under the wrong name agrees
        // with itself, and only a second expression over the same list can see
        // it.
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

        // The premise of every count below: a child that failed on its own left
        // the fixture's residue rather than the kill's.
        let failed: Vec<Option<i32>> = samples.iter().filter_map(|s| s.failed.map(Some)).collect();
        assert!(
            failed.is_empty(),
            "{site}: a sampled child neither died by the kill nor reached its own successful \
             exit (codes {failed:?}), so what the classifier saw is this fixture's failure"
        );

        // The ladder, per command shape rather than per site label: the
        // contract names two *commands*, and two sites that sampled one shape
        // would leave two records intact.
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

        // A kill fired at every child, and no earlier than the rung it was
        // aimed at. `fired` is the clock read inside the kill, so deleting the
        // wait moves it and deleting the kill removes it.
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

    // The kill itself, over the sampling as a whole. Nothing else in this
    // harness changes when `KillableGitChild::kill` stops killing.
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

    // The evidence file `outputs` asks for, written and read back.
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

/// A judgement is not a constant: a gate that fails rejects the attempt, and
/// its snapshot is still cleaned.
///
/// Without this, [`Judgement::accepted`] could `return true` and every other
/// test here would stay green — all of them drive a runner whose processes
/// succeed. The cleanup half matters as much: `snapshots` says they are
/// "cleaned on completion", and a completion is not the same thing as a pass.
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
    // Built before the context borrows `run` mutably.
    let review_inputs = run.review_inputs();
    // Through the production phase, over the same diff the reviewers are
    // shown: a fixture-built `Assessment` could show the judge a diff the
    // cheap rungs never saw.
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
            // Caller-supplied, ordinal included: nothing pass-shaped is
            // minted inside `judge`, so PR8's merge verification can
            // supply its `SequenceIdentities` here without a redesign.
            &|pass| crate::review::ReviewInvocations {
                pass: started.identities.review_pass(pass, 0),
                reask: started.identities.review_reask(pass, 0),
            },
        )
        .expect("judge");

    assert!(!judgement.gates[0].passed(), "the gate exited 2");
    assert_eq!(judgement.gates[0].code, Some(2));
    assert!(!judgement.accepted(), "a failed gate rejects the attempt");

    // **The reviewers do not run, and that is a deliberate change.** This test
    // used to assert that they ran after the failing gate and passed, because
    // the old `judge` ran every gate and then every reviewer unconditionally.
    // The legacy engine does not: §11.2 is "a strong reviewer judges the diff
    // against the acceptance criteria **only once the cheap checks pass**", and
    // `run_attempt` guards its review block on `failure.is_none()`. Buying a
    // frontier invocation to judge a diff the gates have already refused is
    // spend for information the run cannot act on.
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
