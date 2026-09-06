//! Extended notes: `docs/internals/topology/fold/tests/outcome.md`

use super::*;

fn four_offers() -> TopologyFold {
    let mut fold = wide_started(3);
    queue_candidate(&mut fold, MID, 0);
    retained_generation(&mut fold, ZETA, 0);
    apply(&mut fold, &dispatch(BETA, 0, &sha("base")));
    fold
}

#[track_caller]
fn assert_four_offers(fold: &TopologyFold, offered: bool, label: &str) {
    assert_eq!(fold.ready(ALPHA), offered, "{label}: a fresh dispatch");
    assert_eq!(fold.ready_retry(ZETA), offered, "{label}: a retry");
    assert_eq!(
        fold.eligible_continuation(BETA).is_some(),
        offered,
        "{label}: a continuation"
    );
    assert_eq!(
        fold.eligible_integration_candidate().is_some(),
        offered,
        "{label}: an integration"
    );
}

#[test]
fn a_run_that_is_ending_offers_no_dispatch_retry_continuation_or_integration() {
    let fold = four_offers();
    assert_four_offers(&fold, true, "a running run");
    assert_eq!(
        fold.eligible_continuation(BETA),
        Some(GenerationId(0)),
        "the continuation is beta's own open generation"
    );

    let mut stopped = fold.clone();
    apply(&mut stopped, &budget_exceeded(0, Some(MID)));
    assert!(stopped.run_is_ending());
    assert_four_offers(&stopped, false, "a run stopped for budget");

    let mut halted = fold;
    let mut run = halted.run.take().expect("started");
    let epoch = run.epoch;
    run.halted_at = Some(MID);
    run.halted_epoch = Some(epoch);
    halted.run = Some(run);
    assert!(halted.run_is_ending());
    assert_four_offers(&halted, false, "a halted run");
}

#[test]
fn a_continuation_is_offered_at_the_parallel_ceiling_and_a_dispatch_is_not() {
    let mut fold = wide_started(1);
    apply(&mut fold, &dispatch(ZETA, 0, &sha("base")));

    assert!(
        !fold.pipeline_reservable(),
        "zeta's open generation holds the run's one entitlement"
    );
    assert!(!fold.ready(ALPHA), "a fresh dispatch would hold a second");
    assert!(!fold.integration_admissible(), "so would an integration");
    assert_eq!(
        fold.eligible_continuation(ZETA),
        Some(GenerationId(0)),
        "a continuation attempts the generation that already holds the entitlement"
    );
}

#[test]
fn the_integration_offered_is_the_first_eligible_candidate_and_not_merely_an_eligible_one() {
    let mut fold = two_queued();
    let queued: Vec<TaskKey> = fold
        .queue()
        .expect("started")
        .entries()
        .iter()
        .map(QueueEntry::key)
        .collect();
    assert_eq!(queued, vec![MID, ZETA], "the fixture's queue order");

    assert_eq!(
        fold.eligible_integration_candidate(),
        Some(&candidate_of(MID, 0)),
        "both are eligible, so the first queued one is offered"
    );

    apply(&mut fold, &raised("q-integration-Ünicode", MID));
    assert_eq!(
        fold.task_state(MID),
        Some(TaskState::AwaitingInput),
        "the question parks the head of the queue"
    );
    assert_eq!(
        fold.eligible_integration_candidate(),
        Some(&candidate_of(ZETA, 0)),
        "the first ELIGIBLE one is offered, and the parked head is not it"
    );
}
