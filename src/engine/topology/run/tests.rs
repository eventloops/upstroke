//! The loop's branches, checked against the packet's list rather than against
//! the implementation.

use super::*;
use crate::topology::events::{AttemptNumber, DerivedOutcome, GenerationId};
use crate::topology::registry::TaskKey;

/// The transcribed list is the packet's list — seven branches, these labels, in
/// this order.
///
/// `decisions.sequential_substrate.loop` names them in one sentence, split on
/// `->`. A branch dropped from [`LoopBranch::ALL`] would make every other test
/// in this file pass by asking for less, which is exactly how step (g) survived
/// two review rounds in `recover.rs`.
#[test]
fn the_transcribed_loop_branches_are_the_packets_seven() {
    assert_eq!(
        LoopBranch::ALL
            .iter()
            .map(|branch| branch.label())
            .collect::<Vec<_>>(),
        vec![
            "ingest answers",
            "integration",
            "ready_retry",
            "ready dispatch",
            "defer backoff",
            "hard block",
            "run-end closure",
        ],
        "transcribed from `decisions.sequential_substrate.loop`, in its order"
    );
}

/// Every branch this build does not perform says which, and why, in the type.
///
/// **The point of this test is the third disposition.** `RefusedByCheckpoint`
/// is a decision the packet licenses; `NotYetImplemented` is debt. A build that
/// conflated them would be indistinguishable from one that had quietly dropped
/// a branch — and "quietly dropped a branch" is the defect this whole module
/// exists because of.
#[test]
fn every_branch_states_what_this_build_does_with_it() {
    let refused: Vec<&str> = LoopBranch::ALL
        .iter()
        .filter(|branch| branch.disposition() == Disposition::RefusedByCheckpoint)
        .map(|branch| branch.label())
        .collect();
    assert_eq!(
        refused,
        vec!["integration", "run-end closure"],
        "`checkpoint_refusals` names exactly these two for PR7: \"integration \
         and run end beyond refusal\". A third refusal here is a build refusing \
         something the packet did not let it refuse"
    );

    // And the debt, named rather than implied. This assertion is expected to
    // shrink as branches land; it must never grow.
    let owed: Vec<&str> = LoopBranch::ALL
        .iter()
        .filter(|branch| branch.disposition() == Disposition::NotYetImplemented)
        .map(|branch| branch.label())
        .collect();
    assert_eq!(
        owed,
        vec!["ingest answers", "hard block"],
        "the branches this build has not written. Every one of them is carried \
         in the type so that no instrument here has to notice its absence. \
         `defer backoff` left this list when `TopologyRun::step` grew its arm, \
         which is the shape every entry here is expected to leave by"
    );

    // The half-built one, and both halves in the branch's own words. A branch
    // that performs a durable append and reports `NotYetImplemented` would be
    // claiming the log is untouched when it is not; one that reported
    // `Performed` would be claiming an attempt ran.
    assert_eq!(
        LoopBranch::ReadyDispatch.disposition(),
        Disposition::Performed,
        "`loop` states this branch as four clauses and this build performs \
         three; the type says which three"
    );
}

/// Every `Step` a selection can produce maps to exactly one branch, or to none
/// for a stated reason.
///
/// The mapping is total by construction — `LoopBranch::of` matches on `Step`
/// exhaustively, so a new variant does not compile until someone decides which
/// branch it belongs to. What this test adds is the *two `None` arms*, which a
/// compiler cannot check: they are the claim that neither is a branch of the
/// loop, and each is wrong in a different and specific way if the claim slips.
#[test]
fn every_step_belongs_to_one_branch_or_to_none_for_a_reason() {
    let cases: Vec<(Step, Option<LoopBranch>)> = vec![
        (Step::Poisoned, None),
        (
            Step::Retry {
                key: TaskKey(0),
                generation: GenerationId(0),
                attempt: AttemptNumber(1),
            },
            Some(LoopBranch::ReadyRetry),
        ),
        (
            Step::Dispatch {
                key: TaskKey(0),
                generation: GenerationId(0),
            },
            Some(LoopBranch::ReadyDispatch),
        ),
        (Step::Backoff, Some(LoopBranch::DeferBackoff)),
        (
            Step::HardBlock {
                questions: Vec::new(),
            },
            Some(LoopBranch::HardBlock),
        ),
        (
            Step::Closure(DerivedOutcome::NotEnding),
            Some(LoopBranch::Closure),
        ),
    ];
    for (step, expected) in cases {
        assert_eq!(
            LoopBranch::of(&step),
            expected,
            "`{step:?}` maps to the wrong branch"
        );
    }
}

/// A refusal says which branch, and — this is the part that matters — whether
/// anything happened.
///
/// **The two messages must not be interchangeable.** A branch that performed
/// nothing says so, and an operator reading it knows the log is untouched. A
/// branch that appended and then stopped says what it did, because an operator
/// told "not implemented" after a durable `task_dispatched` would go looking
/// for a run directory that does not match the message.
#[test]
fn a_refusal_names_the_branch_and_says_whether_anything_happened() {
    let untouched = LoopBranch::HardBlock.unimplemented().to_string();
    assert!(
        untouched.contains("hard block"),
        "the refusal names the branch: {untouched}"
    );
    assert!(
        untouched.contains("no effect was performed")
            && untouched.contains("no event was appended"),
        "and says the run is untouched: {untouched}"
    );

    // **No branch is `PartlyImplemented` today**, and that is a statement about
    // this build rather than about the type. `ReadyRetry` was the last one and
    // became `Performed` when its second half landed. The variant stays because
    // the next branch built in halves will need it, and this assertion is what
    // says so out loud the moment one appears — a half-built branch is the one
    // shape whose refusal has to say what it already did, because by then
    // `attempt_started` or `task_dispatched` is durable.
    assert!(
        LoopBranch::ALL
            .iter()
            .all(|branch| !matches!(branch.disposition(), Disposition::PartlyImplemented { .. })),
        "a branch is partly built again — assert its `performed ... does not ...` \
         message here, because an operator reading `not implemented` would look \
         for a run that had not started"
    );

    // Every refusal names its own branch, whatever its disposition. A message
    // that named the wrong one would send an operator to the wrong lane.
    for branch in LoopBranch::ALL {
        let refusal = branch.unimplemented().to_string();
        assert!(
            refusal.contains(branch.label()),
            "`{}`'s refusal does not name it: {refusal}",
            branch.label()
        );
    }
}
