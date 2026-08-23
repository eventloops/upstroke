//! `T-DISPATCH`, and the two clauses whose directions disagree.

use std::path::{Path, PathBuf};

use super::*;
use crate::engine::topology::scaffold::{
    ALPHA, BETA, OUTCOME, Run, kill_child_and_adopt, kill_child_environment, kill_dir,
};
use crate::topology::effects::{
    EffectSiteId, HookPhase, Injection, ObjectSite, RefSite, ResidueElement, WorktreeSite,
};
use crate::topology::events::{CandidateRef, GitRef};
use crate::topology::fold::{GenerationClass, TaskState};
use crate::workspace_manager::WorkspaceManager;
use crate::workspace_manager::fixture::{git, remove_file, write_file};

const APPEND: EffectSiteId = EffectSiteId::Event(crate::topology::effects::EventSite::Append);
const INTENT: EffectSiteId = EffectSiteId::Worktree(WorktreeSite::WriteIntent);
const ADD: EffectSiteId = EffectSiteId::Worktree(WorktreeSite::Add);
const VERIFY: EffectSiteId = EffectSiteId::Worktree(WorktreeSite::Verify);
const REMOVE: EffectSiteId = EffectSiteId::Worktree(WorktreeSite::Remove);
const MATERIALIZE: EffectSiteId = EffectSiteId::Object(ObjectSite::RepairMaterialize);

/// The git directory of a linked worktree, asked of Git rather than derived.
///
/// `<worktree>/.git` is a **file** in a linked worktree, and the administrative
/// directory it points at is where every residue element of an interrupted
/// command lands. Deriving the path here would be a second implementation of
/// `workspace_manager`'s own `git_dir_of`; asking `git` is the one answer both
/// agree with.
fn git_dir(worktree: &Path) -> PathBuf {
    PathBuf::from(git(worktree, &["rev-parse", "--absolute-git-dir"]))
}

/// Plant one residue element in a worktree's git dir.
///
/// The five this covers are exactly the ones
/// `workspace_manager::element_breaks_quiescence` holds of and that
/// `administrative_residue_at` reads — the object-store two (`R27`,
/// unreferenced objects and temporary object files) are deliberately **not**
/// here, because `Worktree.Verify` must not consult the object store: every
/// amended commit in a real repository leaves an unreferenced object, and a
/// verify that read one would refuse to reuse an `OpenNoAttempt` worktree in
/// essentially every repository this engine will ever run in.
fn plant(worktree: &Path, element: ResidueElement) {
    let dir = git_dir(worktree);
    match element {
        ResidueElement::IndexLock => write_file(&dir.join("index.lock"), b""),
        ResidueElement::CherryPickHead => write_file(&dir.join("CHERRY_PICK_HEAD"), b"abc\n"),
        ResidueElement::MergeHead => write_file(&dir.join("MERGE_HEAD"), b"abc\n"),
        ResidueElement::MergeMsg => write_file(&dir.join("MERGE_MSG"), b"interrupted\n"),
        ResidueElement::SequencerState => write_file(&dir.join("sequencer/todo"), b"pick abc\n"),
        other => panic!("`{other:?}` is not administrative residue of the owning git dir"),
    }
}

/// The five administrative elements `Worktree.Verify` reads, with the
/// `VerifyFailure` each must produce.
const ADMINISTRATIVE: [ResidueElement; 5] = [
    ResidueElement::IndexLock,
    ResidueElement::CherryPickHead,
    ResidueElement::MergeHead,
    ResidueElement::MergeMsg,
    ResidueElement::SequencerState,
];

/// Whether `worktree` is a registered, populated worktree of `manager`'s
/// repository whose HEAD is `base`.
fn healthy_at(manager: &WorkspaceManager, worktree: &Path, base: &str) -> bool {
    let registered = manager
        .worktree_records()
        .expect("worktree records")
        .into_iter()
        .any(|record| crate::util::same_path(&record.path, worktree));
    registered && git(worktree, &["rev-parse", "HEAD"]) == base
}

// ---------------------------------------------------------------------------
// O21
// ---------------------------------------------------------------------------

/// **O21.** `task_dispatched` is appended, and is durable, before either
/// worktree effect.
///
/// Two axes, because the clause is about two different things and one
/// assertion covers neither on its own.
///
/// *Order* is read off the one [`crate::topology::effects::HookHarness`] all
/// five hook families record into, so the append and the `git worktree add`
/// are positions in a single list rather than two lists a test has to
/// interleave by hand.
///
/// *Durability* is read off the bytes on disk. Order alone would stay green if
/// the append were buffered and the worktree created before the buffer
/// reached the file — and the whole point of writing the event first is that a
/// process that dies between them left the event behind. So the log is read
/// back from the filesystem and its second line must already be the dispatch.
#[test]
fn task_dispatched_is_durable_before_the_intent_and_the_add() {
    let mut run = Run::started("o21");
    let dispatched = run.dispatch(ALPHA, 0);

    let append = run.must_order_of(APPEND, HookPhase::Before);
    let intent = run.must_order_of(INTENT, HookPhase::Before);
    let add = run.must_order_of(ADD, HookPhase::Before);
    assert!(
        append < intent && intent < add,
        "O21: the order was append={append}, intent={intent}, add={add}, and it must be \
         task_dispatched -> Worktree.WriteIntent -> Worktree.Add"
    );

    assert_eq!(
        run.emitter.durable_kinds(),
        vec!["run_started", "task_dispatched"],
        "the dispatch is on disk, not in a buffer"
    );
    let events = run.emitter.durable_events();
    let crate::topology::events::TopologyEventBody::TaskDispatched { data } = &events[1].body
    else {
        panic!(
            "the second durable event is not a dispatch: {:?}",
            events[1]
        );
    };
    assert_eq!(data.key, ALPHA);
    assert_eq!(data.generation, dispatched.generation);
    assert_eq!(data.base_sha, dispatched.base);
    assert_eq!(
        PathBuf::from(&data.worktree_path),
        dispatched.worktree,
        "the recorded worktree path is the one the add returned"
    );
    assert!(
        data.source_candidate.is_none(),
        "an ordinary dispatch records no source candidate"
    );

    assert!(
        healthy_at(
            &run.fixture.manager,
            &dispatched.worktree,
            &dispatched.base.0
        ),
        "and the worktree the event promised exists, at the recorded base"
    );
    assert_eq!(
        run.emitter.generation_class(ALPHA, dispatched.generation),
        GenerationClass::OpenNoAttempt
    );
}

/// **O22, the half that is easiest to get backwards.** A fresh dispatch does
/// not verify.
///
/// `Worktree.Verify` guards *reuse*. If it guarded creation there would be no
/// state in which a worktree exists, carries residue, and is recreated — and
/// `residue_carrying_worktree_fails_verify_and_is_recreated` below would be
/// inexpressible rather than merely failing.
///
/// The second half of the assertion is what stops this being vacuous: the same
/// harness, one reuse later, *must* have seen the site. A test that only
/// asserted the absence would pass against a `Verify` that had been deleted
/// from the module entirely.
#[test]
fn a_fresh_dispatch_never_verifies_a_worktree_it_is_about_to_create() {
    let mut run = Run::started("o22-fresh");
    let dispatched = run.dispatch(ALPHA, 0);
    assert!(
        !run.observed(VERIFY, HookPhase::Before),
        "a fresh dispatch verified a worktree that did not exist when it started"
    );

    verify_or_recreate(
        &run.fixture.manager,
        &mut run.hooks,
        &dispatched,
        &dispatched.quiescence(),
    )
    .expect("reuse");
    assert!(
        run.observed(VERIFY, HookPhase::Before),
        "and a reuse must verify, or the assertion above is about a site nothing drives"
    );
}

// ---------------------------------------------------------------------------
// O22 — reuse
// ---------------------------------------------------------------------------

/// **O22.** A worktree carrying the residue of an interrupted Git command
/// fails `Worktree.Verify` and is removed with force and recreated.
///
/// Driven once per administrative residue element rather than once, because
/// the classifier's list is `ResidueElement`'s and a verify that recognised
/// four of the five would answer "reusable" for a worktree holding sequencer
/// state — which is a `git cherry-pick` that stopped half way, and reusing it
/// would run the next attempt on a tree nobody chose.
///
/// The two non-administrative failures are crossed in beside them: a HEAD that
/// moved off the base, and a worktree that is not registered at all. All three
/// shapes route to the same recovery, and asserting only one would leave the
/// other two to `NotRegistered`'s branch by accident.
#[test]
fn residue_carrying_worktree_fails_verify_and_is_recreated() {
    let mut run = Run::started("residue");
    let dispatched = run.dispatch(ALPHA, 0);
    let worktree = dispatched.worktree.clone();

    for element in ADMINISTRATIVE {
        plant(&worktree, element);
        assert!(
            !&run
                .fixture
                .manager
                .quiescence(&worktree, &dispatched.quiescence())
                .expect("observe")
                .is_ok(),
            "{element:?}: the worktree must not be quiescent once it is planted"
        );

        let reuse = verify_or_recreate(
            &run.fixture.manager,
            &mut run.hooks,
            &dispatched,
            &dispatched.quiescence(),
        )
        .expect("recreate converges");
        assert_eq!(
            reuse,
            Reuse::Recreated {
                failure: crate::workspace_manager::VerifyFailure::Residue(element)
            },
            "{element:?}: it must be recreated, and for this reason"
        );
        assert!(
            healthy_at(&run.fixture.manager, &worktree, &dispatched.base.0),
            "{element:?}: the recreated worktree is registered and at the recorded base"
        );
        assert!(
            &run.fixture
                .manager
                .quiescence(&worktree, &dispatched.quiescence())
                .expect("observe")
                .is_ok(),
            "{element:?}: and the residue left with the worktree that carried it"
        );
    }

    // A HEAD that moved. Not administrative residue, and the same recovery.
    git(
        &worktree,
        &["checkout", "-q", "--detach", &run.fixture.seed],
    );
    let reuse = verify_or_recreate(
        &run.fixture.manager,
        &mut run.hooks,
        &dispatched,
        &dispatched.quiescence(),
    )
    .expect("recreate converges");
    assert!(
        matches!(
            reuse,
            Reuse::Recreated {
                failure: crate::workspace_manager::VerifyFailure::HeadMismatch { .. }
            }
        ),
        "a worktree at another commit is not the one the generation recorded: {reuse:?}"
    );
    assert!(healthy_at(
        &run.fixture.manager,
        &worktree,
        &dispatched.base.0
    ));

    // And a worktree that is simply gone.
    run.fixture
        .manager
        .remove_worktree(run.hooks.effects(), &dispatched.slot)
        .expect("scrub");
    let reuse = verify_or_recreate(
        &run.fixture.manager,
        &mut run.hooks,
        &dispatched,
        &dispatched.quiescence(),
    )
    .expect("recreate converges");
    assert_eq!(
        reuse,
        Reuse::Recreated {
            failure: crate::workspace_manager::VerifyFailure::NotRegistered
        }
    );
    assert!(healthy_at(
        &run.fixture.manager,
        &worktree,
        &dispatched.base.0
    ));

    // The recreation really went through the two funnels, in the order the
    // clause gives them, and through the forced removal in between.
    for site in [REMOVE, INTENT, ADD] {
        assert!(
            run.observed(site, HookPhase::After),
            "`{site}` never executed, so nothing was actually recreated"
        );
    }
}

/// A verified worktree is **reused**, and the recreation path is not entered.
///
/// The control half of the test above. Without it, a `verify_or_recreate` that
/// removed and rebuilt unconditionally would satisfy every assertion there —
/// the failures would still be reported, because they are read off the
/// verification, and the worktree would still be healthy afterwards.
#[test]
fn a_quiescent_worktree_is_reused_rather_than_rebuilt() {
    let mut run = Run::started("reuse");
    let dispatched = run.dispatch(ALPHA, 0);
    let sentinel = dispatched.worktree.join("sentinel.txt");
    write_file(&sentinel, b"an untracked file the reuse must not destroy\n");

    let reuse = verify_or_recreate(
        &run.fixture.manager,
        &mut run.hooks,
        &dispatched,
        &dispatched.quiescence(),
    )
    .expect("verify succeeds");
    assert_eq!(reuse, Reuse::Verified);
    assert!(reuse.reused());
    assert!(
        sentinel.is_file(),
        "a reuse that rebuilt the worktree would have taken this file with it"
    );
    assert!(
        !run.observed(REMOVE, HookPhase::Before),
        "a verified worktree must not be removed"
    );
}

// ---------------------------------------------------------------------------
// T-DISPATCH — the kill
// ---------------------------------------------------------------------------

/// The child of [`kill_after_dispatch_recreates_worktree_without_spend`].
///
/// Dies at one of the two prefixes `T-DISPATCH`'s boundary names: "worktree
/// intent or worktree not yet created" and "created without `attempt_started`".
#[test]
#[ignore = "spawned as a subprocess by kill_after_dispatch_recreates_worktree_without_spend"]
fn dispatch_kill_child() {
    let (dir, which) = kill_child_environment();
    let mut run = Run::started("killdispatch");
    run.hand_off(&dir);
    match which.as_str() {
        "before_intent" => run.arm(INTENT, HookPhase::Before, Injection::Kill),
        "after_add" => run.arm(ADD, HookPhase::After, Injection::Kill),
        other => panic!("unknown site `{other}`"),
    }
    let _ = run.try_dispatch(ALPHA, 0);
    unreachable!("the kill must have taken this process");
}

/// **`T-DISPATCH`.** A coordinator killed after `task_dispatched` leaves an
/// `OpenNoAttempt` generation with **no spend**, and the recovery rebuilds or
/// reuses its worktree without repeating one.
///
/// Both prefixes of the boundary, because their recoveries differ and only one
/// of the two branches would otherwise be executed: at `before_intent` nothing
/// on disk exists and the worktree is built from nothing, at `after_add` the
/// worktree exists and quiesces and is reused. A test that drove only the first
/// would pass against a recovery that force-removed every worktree it found.
///
/// "Without spend" is asserted three ways: no `attempt_started` in the durable
/// log, the generation still `OpenNoAttempt`, and the task still `Pending`.
/// The first is the durable claim; the other two are what a scheduler reads,
/// and a fold that admitted a spend the log did not record would be caught by
/// the disagreement rather than by any one of them.
#[test]
fn kill_after_dispatch_recreates_worktree_without_spend() {
    for site in ["before_intent", "after_add"] {
        let dir = kill_dir("killdispatch");
        let mut run = kill_child_and_adopt(
            "engine::topology::dispatch::tests::dispatch_kill_child",
            &dir,
            site,
        );

        assert_eq!(
            run.emitter.durable_kinds(),
            vec!["run_started", "task_dispatched"],
            "`{site}`: the dispatch is durable and nothing was spent"
        );
        assert_eq!(
            run.emitter
                .generation_class(ALPHA, crate::topology::events::GenerationId(0)),
            GenerationClass::OpenNoAttempt,
            "`{site}`"
        );
        assert_eq!(run.task_state(ALPHA), TaskState::Pending, "`{site}`");

        let dispatched = Dispatched {
            key: ALPHA,
            generation: crate::topology::events::GenerationId(0),
            base: run.base(),
            slot: task_slot(ALPHA, crate::topology::events::GenerationId(0)),
            worktree: run
                .fixture
                .manager
                .slot_path(&task_slot(ALPHA, crate::topology::events::GenerationId(0))),
            kind: DispatchKind::Ordinary {
                paths: run.predicted(),
            },
        };

        // What the child actually left, which is the difference between the
        // two prefixes and is asserted rather than assumed.
        let existed = dispatched.worktree.is_dir();
        assert_eq!(
            existed,
            site == "after_add",
            "`{site}`: the child left the wrong prefix on disk"
        );

        let reuse = resume_open_no_attempt(&run.fixture.manager, &mut run.hooks, &dispatched)
            .expect("recover");
        assert_eq!(
            reuse.reused(),
            site == "after_add",
            "`{site}`: an existing quiescent worktree is reused and an absent one is rebuilt \
             ({reuse:?})"
        );
        assert!(
            healthy_at(
                &run.fixture.manager,
                &dispatched.worktree,
                &dispatched.base.0
            ),
            "`{site}`: the worktree is at the recorded base after recovery"
        );
        assert_eq!(
            run.emitter.durable_kinds(),
            vec!["run_started", "task_dispatched"],
            "`{site}`: recovery of an OpenNoAttempt generation appends nothing and spends nothing"
        );
    }
}

// ---------------------------------------------------------------------------
// T-DISPATCH — repairs
// ---------------------------------------------------------------------------

/// The candidate ref a repair fixture materializes from, and the `CandidateRef`
/// naming it.
///
/// The commit is the fixture's `side`, which is a real commit on `seed` adding
/// one file, so a cherry-pick of it onto `head` applies cleanly and its effect
/// is visible as a path in the index.
fn protected_candidate(run: &mut Run) -> CandidateRef {
    let refname = "refs/upstroke/runs/run-1/candidates/k0/0".to_owned();
    let commit = run.fixture.side.clone();
    run.fixture
        .manager
        .create_ref_zero_old(
            run.hooks.effects(),
            RefSite::CreateCandidates,
            &refname,
            &commit,
        )
        .expect("the authoritative candidates ref");
    CandidateRef {
        key: ALPHA,
        generation: crate::topology::events::GenerationId(0),
        commit_sha: crate::topology::events::CommitSha(commit),
        candidate_ref: GitRef(refname),
    }
}

/// The child of [`repair_materialization_reproduced_after_kill`]: dispatches a
/// repair and dies at `Object.RepairMaterialize`.
#[test]
#[ignore = "spawned as a subprocess by repair_materialization_reproduced_after_kill"]
fn repair_kill_child() {
    let (dir, which) = kill_child_environment();
    let mut run = Run::started("killrepair");
    run.hand_off(&dir);
    let source = protected_candidate(&mut run);
    let repair = run.spawn_repair(ALPHA);
    let phase = match which.as_str() {
        "before_materialize" => HookPhase::Before,
        "after_materialize" => HookPhase::After,
        other => panic!("unknown site `{other}`"),
    };
    run.arm(MATERIALIZE, phase, Injection::Kill);
    let request = DispatchRequest {
        key: repair,
        generation: crate::topology::events::GenerationId(0),
        base: run.base(),
        kind: DispatchKind::Repair {
            root: ALPHA,
            source,
        },
    };
    let _ = dispatch(
        &run.fixture.manager,
        &mut run.hooks,
        &mut run.emitter,
        &request,
    );
    unreachable!("the kill must have taken this process");
}

/// **`T-DISPATCH`.** "for repairs re-run the recorded materialization in a
/// verified or fresh worktree".
///
/// Both sides of the materialization, because they leave different worktrees
/// and the recovery has to converge from each: killed *before* it, the worktree
/// is at the base with a clean index; killed *after* it, the worktree carries
/// the merge objects **and** `CHERRY_PICK_HEAD`, which `Worktree.Verify` reads
/// as administrative residue and refuses.
///
/// The oracle is the recorded source, not a path list: after recovery the
/// worktree's index must hold exactly what an uninterrupted materialization
/// would have produced. That is computed by materializing the same candidate in
/// a **second, independent** generation and comparing the two indexes — so the
/// assertion is "the same as doing it once, uninterrupted" rather than "some
/// file appeared", which a half-applied cherry-pick also satisfies.
#[test]
fn repair_materialization_reproduced_after_kill() {
    for site in ["before_materialize", "after_materialize"] {
        let dir = kill_dir("killrepair");
        let mut run = kill_child_and_adopt(
            "engine::topology::dispatch::tests::repair_kill_child",
            &dir,
            site,
        );
        let repair = crate::topology::registry::TaskKey(2);
        let generation = crate::topology::events::GenerationId(0);

        assert_eq!(
            run.emitter.durable_kinds(),
            vec!["run_started", "task_spawned", "task_dispatched"],
            "`{site}`: the repair's dispatch is durable"
        );
        let events = run.emitter.durable_events();
        let crate::topology::events::TopologyEventBody::TaskDispatched { data } = &events[2].body
        else {
            panic!("`{site}`: the third durable event is not a dispatch");
        };
        let source = data
            .source_candidate
            .clone()
            .unwrap_or_else(|| panic!("`{site}`: a repair records its source candidate"));
        assert_eq!(
            source.commit_sha.0, run.fixture.side,
            "`{site}`: and it is the protected candidate"
        );

        let dispatched = Dispatched {
            key: repair,
            generation,
            base: run.base(),
            slot: task_slot(repair, generation),
            worktree: run
                .fixture
                .manager
                .slot_path(&task_slot(repair, generation)),
            kind: DispatchKind::Repair {
                root: ALPHA,
                source: source.clone(),
            },
        };

        resume_open_no_attempt(&run.fixture.manager, &mut run.hooks, &dispatched)
            .expect("`{site}`: the repair's resume converges");

        // The independent oracle: the same candidate, materialized once, in a
        // worktree nothing ever killed.
        let control = Dispatched {
            key: BETA,
            generation: crate::topology::events::GenerationId(0),
            base: run.base(),
            slot: task_slot(BETA, crate::topology::events::GenerationId(0)),
            worktree: run
                .manager()
                .slot_path(&task_slot(BETA, crate::topology::events::GenerationId(0))),
            kind: DispatchKind::Repair {
                root: ALPHA,
                source,
            },
        };
        run.fixture
            .manager
            .write_intent(run.hooks.effects(), &control.slot)
            .expect("control intent");
        run.fixture
            .manager
            .add_worktree(run.hooks.effects(), &control.slot, &control.base.0)
            .expect("control worktree");
        materialize_repair(&run.fixture.manager, &mut run.hooks, &control)
            .expect("control materialize");

        assert_eq!(
            git(&dispatched.worktree, &["write-tree"]),
            git(&control.worktree, &["write-tree"]),
            "`{site}`: the reproduced materialization must be the tree an uninterrupted one \
             produces"
        );
        assert!(
            run.observed(MATERIALIZE, HookPhase::After),
            "`{site}`: the recovery re-ran the recorded materialization"
        );
    }
}

/// **`T-DISPATCH`'s `refusal_condition`.** "source candidate object missing",
/// and the refusal costs no durable state.
///
/// Three shapes, because a missing candidate arrives three ways and only one of
/// them is literally an absent object: the commit is gone, the authoritative
/// ref that keeps it reachable is gone, or the ref names something else. Each
/// must refuse **before the append**, which is what the durable-log assertion
/// after each one is for — a refusal raised after `task_dispatched` would leave
/// an open generation whose worktree can never be built.
#[test]
fn a_repair_whose_source_candidate_is_missing_is_refused_before_any_append() {
    let mut run = Run::started("repairrefusal");
    let source = protected_candidate(&mut run);
    let repair = run.spawn_repair(ALPHA);
    let before = run.emitter.durable_kinds().len();

    let absent_object = CandidateRef {
        commit_sha: crate::topology::events::CommitSha(
            "0123456789abcdef0123456789abcdef01234567".to_owned(),
        ),
        ..source.clone()
    };
    let absent_ref = CandidateRef {
        candidate_ref: GitRef("refs/upstroke/runs/run-1/candidates/k9/9".to_owned()),
        ..source.clone()
    };
    let wrong_target = CandidateRef {
        commit_sha: crate::topology::events::CommitSha(run.fixture.seed.clone()),
        ..source.clone()
    };

    for (what, candidate, expected) in [
        ("an absent object", absent_object, "is not an object"),
        ("an absent ref", absent_ref, "does not exist"),
        ("a ref that names another commit", wrong_target, "names"),
    ] {
        let request = DispatchRequest {
            key: repair,
            generation: crate::topology::events::GenerationId(0),
            base: run.base(),
            kind: DispatchKind::Repair {
                root: ALPHA,
                source: candidate,
            },
        };
        let error = dispatch(
            &run.fixture.manager,
            &mut run.hooks,
            &mut run.emitter,
            &request,
        )
        .expect_err(what);
        let message = error.to_string();
        assert!(
            message.contains(expected),
            "{what}: the refusal must name the reason, and said: {message}"
        );
        assert_eq!(
            run.emitter.durable_kinds().len(),
            before,
            "{what}: the refusal appended something"
        );
        assert!(
            !run.observed(INTENT, HookPhase::Before),
            "{what}: the refusal wrote an intent"
        );
    }

    // The control: the same dispatch with the real candidate is accepted, so
    // the three refusals above are about the candidate rather than about the
    // request being malformed in some other way.
    let request = DispatchRequest {
        key: repair,
        generation: crate::topology::events::GenerationId(0),
        base: run.base(),
        kind: DispatchKind::Repair {
            root: ALPHA,
            source,
        },
    };
    dispatch(
        &run.fixture.manager,
        &mut run.hooks,
        &mut run.emitter,
        &request,
    )
    .expect("the real candidate dispatches");
    assert_eq!(run.emitter.durable_kinds().len(), before + 1);
}

/// An ordinary dispatch has no materialization to reproduce, and asking for one
/// is a refusal rather than a silent success.
#[test]
fn reproducing_a_materialization_an_ordinary_dispatch_never_had_is_refused() {
    let mut run = Run::started("nomaterialize");
    let dispatched = run.dispatch(ALPHA, 0);
    let error = materialize_repair(&run.fixture.manager, &mut run.hooks, &dispatched)
        .expect_err("an ordinary dispatch materializes nothing");
    assert!(
        error.to_string().contains("no recorded materialization"),
        "{error}"
    );
    assert!(!run.observed(MATERIALIZE, HookPhase::Before));
}

// ---------------------------------------------------------------------------
// Run end
// ---------------------------------------------------------------------------

/// **ST-17 / `T-DISPATCH`.** "at run end: `generation_closed{RunEnding}`", and
/// the worktree is scrubbed **after** it.
///
/// The ordering is `cleanup`'s: "task worktree scrubbed only after
/// `task_candidate_created` is durable **or the generation is Closed**". A
/// scrub that ran first would remove a worktree the log still calls resumably
/// open, and a resume between the two would try to verify it.
#[test]
fn open_no_attempt_closed_at_run_end() {
    let mut run = Run::started("runend");
    let dispatched = run.dispatch(ALPHA, 0);
    let intent = &run.fixture.manager.intent_path(&dispatched.slot);
    assert!(intent.is_file() && dispatched.worktree.is_dir());

    let mark = run.mark();
    close_at_run_end(
        &run.fixture.manager,
        &mut run.hooks,
        &mut run.emitter,
        &dispatched,
        OUTCOME,
    )
    .expect("close at run end");

    assert_eq!(
        run.emitter.durable_kinds(),
        vec!["run_started", "task_dispatched", "generation_closed"]
    );
    let events = run.emitter.durable_events();
    let crate::topology::events::TopologyEventBody::GenerationClosed { data } = &events[2].body
    else {
        panic!("the third durable event is not a closure");
    };
    assert_eq!(
        data.reason,
        crate::topology::events::GenerationCloseReason::RunEnding { outcome: OUTCOME }
    );
    assert_eq!(
        data.lease,
        crate::topology::events::LeaseDisposition::PredictedReleased,
        "an ordinary generation that closes releases the region it held"
    );
    assert_eq!(
        run.emitter.generation_class(ALPHA, dispatched.generation),
        GenerationClass::Closed
    );

    assert!(
        !dispatched.worktree.exists() && !intent.exists(),
        "the worktree and its intent left with the generation"
    );
    // Counted from a mark, not from the first observation of each site: the
    // dispatch that opened this generation already drove both, so a comparison
    // of first observations compares the wrong pair and is true whatever this
    // function does. Measured — with the scrub moved in front of the closure it
    // stayed green.
    let close = run.order_after(mark, APPEND, HookPhase::After);
    let remove = run.order_after(mark, REMOVE, HookPhase::Before);
    assert!(
        close < remove,
        "the closure must be durable before the scrub: append={close}, remove={remove}"
    );
}

/// A repair's run-end closure records `LineageHeld`, not `PredictedReleased`.
///
/// The one field the two dispatch kinds disagree about at a terminal, and the
/// fold refuses the wrong one — `check_lease_disposition` compares it against
/// `GenerationLease::expected(false)`. Without this the ordinary case above
/// would be the only disposition ever executed, and a `closing_disposition`
/// that answered `PredictedReleased` unconditionally would be invisible.
#[test]
fn a_repairs_run_end_closure_holds_the_lineage_lease() {
    let mut run = Run::started("runend-repair");
    let source = protected_candidate(&mut run);
    let repair = run.spawn_repair(ALPHA);
    let request = DispatchRequest {
        key: repair,
        generation: crate::topology::events::GenerationId(0),
        base: run.base(),
        kind: DispatchKind::Repair {
            root: ALPHA,
            source,
        },
    };
    let dispatched = dispatch(
        &run.fixture.manager,
        &mut run.hooks,
        &mut run.emitter,
        &request,
    )
    .expect("the repair dispatches");
    assert!(
        run.observed(MATERIALIZE, HookPhase::After),
        "a repair materializes its source as part of the dispatch"
    );

    close_at_run_end(
        &run.fixture.manager,
        &mut run.hooks,
        &mut run.emitter,
        &dispatched,
        OUTCOME,
    )
    .expect("close at run end");
    let events = run.emitter.durable_events();
    let crate::topology::events::TopologyEventBody::GenerationClosed { data } =
        &events.last().expect("a closure").body
    else {
        panic!("the last durable event is not a closure");
    };
    assert_eq!(
        data.lease,
        crate::topology::events::LeaseDisposition::LineageHeld,
        "a repair never changes a lineage lease"
    );
}

/// The intent is durable before the add, and the add refuses without it.
///
/// `Refusal::AddWithoutIntent` is `workspace_manager`'s, and this is the
/// dispatch-side statement of what it protects: a worktree created without a
/// durable intent is one `reclaim_intents` can never find. Driven by removing
/// the intent and re-adding, because the funnel cannot be made to skip it.
#[test]
fn an_add_whose_intent_is_gone_is_refused_rather_than_leaking_a_worktree() {
    let mut run = Run::started("addwithoutintent");
    let dispatched = run.dispatch(ALPHA, 0);
    run.fixture
        .manager
        .remove_worktree(run.hooks.effects(), &dispatched.slot)
        .expect("scrub");
    remove_file(&run.fixture.manager.intent_path(&dispatched.slot));

    let error = run
        .fixture
        .manager
        .add_worktree(run.hooks.effects(), &dispatched.slot, &dispatched.base.0)
        .expect_err("an add without an intent is refused");
    assert!(error.to_string().contains("durable intent"), "{error}");
    assert!(!dispatched.worktree.exists());
}
