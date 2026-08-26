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
    assert!(
        owed.is_empty(),
        "the branches this build has not written. Every one of them is carried \
         in the type so that no instrument here has to notice its absence. \
         `defer backoff` left this list when `TopologyRun::step` grew its arm, \
         which is the shape every entry here is expected to leave by. It is \
         empty now: {owed:?}"
    );

    // **What is another slice's, cited rather than owed.** `ingest answers` is
    // not debt and is not a checkpoint refusal — the packet authorises exactly
    // two of those — so it carries the contract passage that assigns it.
    let elsewhere: Vec<&str> = LoopBranch::ALL
        .iter()
        .filter(|branch| matches!(branch.disposition(), Disposition::NotThisSlice { .. }))
        .map(|branch| branch.label())
        .collect();
    assert_eq!(
        elsewhere,
        vec!["ingest answers"],
        "a branch left this build's scope without saying which slice took it"
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
                continuing: false,
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

/// **Every append the driver makes propagates its error.**
///
/// The append-error protocol is five obligations, and all five begin with the
/// error *reaching* the protocol. A `let _ = self.emit(..)` reaches none of
/// them: the fold is not poisoned, no reservation or invocation is cancelled,
/// and the command reports success for a run whose log does not contain the
/// line it just claimed to write.
///
/// Catalogue entry `PR7-SELECT-026` did exactly that to the
/// `Admitted::BudgetExceeded` arm and the whole suite stayed green, because the
/// arms whose append failure *is* armed by a fixture are not that one.
///
/// A **census rather than a fixture per arm**, for the reason the other four
/// single-authority censuses exist: a per-arm test proves the arm it names and
/// says nothing about the arm added next week. This proves the property over
/// every append site the driver has, including the ones not yet written.
///
/// The region is [`crate::effects::production_code`], which blanks comments and
/// strings — a `let _ = self.emit(` quoted in a doc comment must not fail this,
/// and a truncating region would let a site below the cut through, which is
/// `PR4-CENSUS-COMMENT-ORACLE` and is how the barrier census scanned 4.7% of
/// this very file.
#[test]
fn every_driver_append_propagates_its_error() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine/topology/run.rs"),
    )
    .expect("the driver's own source");
    let code = crate::effects::production_code(&source);

    assert!(
        code.len() * 10 > source.len(),
        "the production region is {} of {} bytes — a census over a fraction of a \
         file reports zero for the part it never read",
        code.len(),
        source.len()
    );

    let needle = "self.emit(";
    let mut sites = 0;
    let mut unpropagated = Vec::new();
    for (at, _) in code.match_indices(needle) {
        sites += 1;
        // Walk to the matching close paren, then check what follows it.
        let mut depth = 0_i32;
        let mut end = None;
        for (offset, ch) in code[at + needle.len() - 1..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(at + needle.len() - 1 + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            unpropagated.push(format!("unbalanced call at byte {at}"));
            continue;
        };
        if !code[end..].trim_start().starts_with('?') {
            let line = code[..at].matches('\n').count() + 1;
            unpropagated.push(format!("line {line} (of the blanked region)"));
        }
    }

    assert!(
        sites >= 4,
        "only {sites} append sites found, so a green result here would prove nothing"
    );
    assert!(
        unpropagated.is_empty(),
        "these driver appends do not propagate their error, so the append-error \
         protocol never runs for them: {unpropagated:?}"
    );
}

/// **The loop chooses its branch through one selector.**
///
/// `decisions.sequential_substrate.loop` gives seven branches in one order, and
/// `select` is where that order lives. Catalogue entry `PR7-SELECT-015` added a
/// **second** selector — `select_rescan`, ordered Dispatch/Retry/Integrate
/// instead of Integrate/Retry/Dispatch — pointed `TopologyRun::step` at it, and
/// left canonical `select` untouched with every one of its tests still passing.
/// The whole suite was green.
///
/// That is the seams category in its purest form: `select.rs` is coherent,
/// `run.rs` is coherent, and the branch order the packet specifies is not the
/// one the run takes. No per-function test can see it, because each function is
/// right about itself.
///
/// The fifth single-authority census this slice owns, and the cheapest: the
/// driver reaches its branch order through exactly one call, and `checkpoint`
/// guards exactly that call's result. A second selector makes this count zero,
/// not two — which is why the assertion is on the **canonical** name rather than
/// on a total.
#[test]
fn the_loop_selects_through_one_function() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine/topology/run.rs"),
    )
    .expect("the driver's own source");
    let code = crate::effects::production_code(&source);

    assert!(
        code.len() * 10 > source.len(),
        "the production region is {} of {} bytes",
        code.len(),
        source.len()
    );

    // Calls, not definitions — neither is defined here, but the filter is the
    // one the barrier census learned to use and costs nothing.
    let calls = |needle: &str| {
        code.match_indices(needle)
            .filter(|(at, _)| !code[..*at].trim_end().ends_with("fn"))
            .count()
    };

    assert_eq!(
        calls("select("),
        1,
        "the driver reaches its branch order through {} calls to `select`. Zero \
         means a second selector was written and this one bypassed — the branch \
         order the packet specifies is then not the order the run takes, and \
         `select`'s own tests still pass",
        calls("select(")
    );
    assert_eq!(
        calls("checkpoint("),
        1,
        "`checkpoint` refuses the terminals this build does not implement. One \
         selector guarded by one checkpoint is the pair; a selected step that \
         reached the loop unguarded is `INV-07`'s failure"
    );
}

/// **Both arms of `attempt_started` get their pool from an authority.**
///
/// `attempt_started` is appended from two places and they reach it differently:
/// the dispatch arm builds its plan first and reads `plan.pool`; the retry arm
/// appends **before** its plan exists, because `settle::retry` produces the
/// event and the plan is built after. Sol's `R3-SEAMS-001` is what that
/// asymmetry produced — the retry passed `pool: None`, so a resumed run's ledger
/// recorded no pool while the plan it then built resolved one, and the two
/// disagreed about the same attempt.
///
/// **A source census rather than a behavioural test, and the reason is
/// structural.** A retry is only reachable *within* one process: recovery step
/// (e) closes every `RetainedIdle` generation, so a resumed run never has one to
/// retry, and no driver fixture can reach the arm. Asserting the property over
/// the construction sites is what is available, and it is what actually failed —
/// a literal `None` where the other arm had an authority.
///
/// The needle is the field's value in each production `AttemptStarted4` literal.
/// A hard-coded `None` fails; anything that names something does not, because
/// this census's claim is "not invented here", not "non-empty".
#[test]
fn both_attempt_started_arms_take_their_pool_from_an_authority() {
    const SITES: &[(&str, &str)] = &[
        (
            "src/engine/topology/attempt.rs",
            "the dispatch arm: `plan.pool`, resolved by the assembler that owns the pool table",
        ),
        (
            "src/engine/topology/settle.rs",
            "the retry arm: `request.pool`, which the driver fills from `AttemptPlans::pool_for` \
             — the same authority, asked one step earlier",
        ),
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut invented: Vec<String> = Vec::new();
    let mut checked = 0_usize;
    for (file, why) in SITES {
        let source = std::fs::read_to_string(root.join(file)).expect("a source file");
        let code = crate::effects::production_code(&source);
        let at = code
            .find("AttemptStarted4 {")
            .unwrap_or_else(|| panic!("{file} no longer constructs an `AttemptStarted4`"));
        let rest = &code[at..];
        let body = &rest[..rest.find("})").unwrap_or(rest.len())];
        let pool = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("pool:"))
            .unwrap_or_else(|| panic!("{file}'s `AttemptStarted4` has no `pool` field"));
        checked += 1;
        if pool.trim().starts_with("None") {
            invented.push(format!("{file} — {why}"));
        }
    }

    assert_eq!(checked, SITES.len(), "a site stopped being found");
    assert!(
        invented.is_empty(),
        "these append `attempt_started` with a hard-coded `pool: None`, so the ledger and the \
         plan disagree about which pool the attempt drained: {invented:?}"
    );
}
