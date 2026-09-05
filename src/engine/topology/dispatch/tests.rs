//! Extended notes: `docs/internals/engine/topology/dispatch/tests.md`

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

fn git_dir(worktree: &Path) -> PathBuf {
    PathBuf::from(git(worktree, &["rev-parse", "--absolute-git-dir"]))
}

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

const ADMINISTRATIVE: [ResidueElement; 5] = [
    ResidueElement::IndexLock,
    ResidueElement::CherryPickHead,
    ResidueElement::MergeHead,
    ResidueElement::MergeMsg,
    ResidueElement::SequencerState,
];

fn healthy_at(manager: &WorkspaceManager, worktree: &Path, base: &str) -> bool {
    let registered = manager
        .worktree_records()
        .expect("worktree records")
        .into_iter()
        .any(|record| crate::util::same_path(record.path(), worktree));
    registered && git(worktree, &["rev-parse", "HEAD"]) == base
}

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
        "the recorded worktree path is the one the add returned: the event's string is derived \
         from the slot before the append, because O21 puts the append first, and this field is \
         what `Worktree.Add` answered — two derivations rather than one local compared to \
         itself, so a dispatch that named one directory and created another fails here. What \
         it does not catch is Git creating a third: both sides re-derive from the slot, and \
         nothing reads the checkout's location back"
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

#[test]
fn a_containment_condition_that_fails_mid_run_refuses_before_the_append() {
    let mut run = Run::started("containment");
    let durable_before = run.emitter.durable_kinds();

    let foreign = run.manager().execution_root().join("foreign");
    let foreign_arg = foreign.to_string_lossy().into_owned();
    let head = run.fixture.head.clone();
    git(
        &run.fixture.base,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            &foreign_arg,
            &head,
        ],
    );

    let mark = run.mark();
    let error = run
        .try_dispatch(ALPHA, 0)
        .expect_err("a foreign worktree inside the execution root is a containment refusal");
    assert!(
        error.to_string().contains("is inside it"),
        "the refusal must be the containment one, and said: {error}"
    );

    assert_eq!(
        run.count_after(mark, APPEND, HookPhase::Before),
        0,
        "the refusal must arrive before `task_dispatched`, not after it"
    );
    assert_eq!(
        run.emitter.durable_kinds(),
        durable_before,
        "so the log carries no generation whose worktree can never be built"
    );
    assert!(
        !run.observed(INTENT, HookPhase::Before),
        "and nothing on disk was attempted either"
    );

    git(
        &run.fixture.base,
        &["worktree", "remove", "--force", &foreign_arg],
    );
    let dispatched = run.dispatch(ALPHA, 0);
    assert_eq!(
        run.emitter.durable_kinds().len(),
        durable_before.len() + 1,
        "the control dispatch appends exactly one event"
    );
    assert!(healthy_at(
        &run.fixture.manager,
        &dispatched.worktree,
        &dispatched.base.0
    ));
}

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
        &dispatched.open_generation(),
        &dispatched.quiescence(),
    )
    .expect("reuse");
    assert!(
        run.observed(VERIFY, HookPhase::Before),
        "and a reuse must verify, or the assertion above is about a site nothing drives"
    );
}

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
            &dispatched.open_generation(),
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

    git(
        &worktree,
        &["checkout", "-q", "--detach", &run.fixture.seed],
    );
    let reuse = verify_or_recreate(
        &run.fixture.manager,
        &mut run.hooks,
        &dispatched.open_generation(),
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

    run.fixture
        .manager
        .remove_worktree(run.hooks.effects(), &dispatched.slot)
        .expect("scrub");
    let reuse = verify_or_recreate(
        &run.fixture.manager,
        &mut run.hooks,
        &dispatched.open_generation(),
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

    for site in [REMOVE, INTENT, ADD] {
        assert!(
            run.observed(site, HookPhase::After),
            "`{site}` never executed, so nothing was actually recreated"
        );
    }
}

#[test]
fn a_quiescent_worktree_is_reused_rather_than_rebuilt() {
    let mut run = Run::started("reuse");
    let dispatched = run.dispatch(ALPHA, 0);
    let sentinel = dispatched.worktree.join("sentinel.txt");
    write_file(&sentinel, b"an untracked file the reuse must not destroy\n");

    let reuse = verify_or_recreate(
        &run.fixture.manager,
        &mut run.hooks,
        &dispatched.open_generation(),
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
                paths: run.predicted(ALPHA),
            },
        };

        let existed = dispatched.worktree.is_dir();
        assert_eq!(
            existed,
            site == "after_add",
            "`{site}`: the child left the wrong prefix on disk"
        );

        let reuse = resume_open_no_attempt(
            &run.fixture.manager,
            &mut run.hooks,
            &dispatched.open_generation(),
        )
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

        resume_open_no_attempt(
            &run.fixture.manager,
            &mut run.hooks,
            &dispatched.open_generation(),
        )
        .expect("`{site}`: the repair's resume converges");

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
        materialize_repair(
            &run.fixture.manager,
            &mut run.hooks,
            &control.open_generation(),
        )
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

#[test]
fn reproducing_a_materialization_an_ordinary_dispatch_never_had_is_refused() {
    let mut run = Run::started("nomaterialize");
    let dispatched = run.dispatch(ALPHA, 0);
    let error = materialize_repair(
        &run.fixture.manager,
        &mut run.hooks,
        &dispatched.open_generation(),
    )
    .expect_err("an ordinary dispatch materializes nothing");
    assert!(
        error.to_string().contains("no recorded materialization"),
        "{error}"
    );
    assert!(!run.observed(MATERIALIZE, HookPhase::Before));
}

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
    let close = run.order_after(mark, APPEND, HookPhase::After);
    let remove = run.order_after(mark, REMOVE, HookPhase::Before);
    assert!(
        close < remove,
        "the closure must be durable before the scrub: append={close}, remove={remove}"
    );
}

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
