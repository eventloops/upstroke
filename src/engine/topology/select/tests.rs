use super::*;
use crate::events::RunOutcome;
use crate::ladder::Next;
use crate::topology::events::{
    CandidateLeaseEffect, CandidatePrepared, GitRef, RunStarted4, TaskCandidateCreated,
    TopologyLimits,
};
use crate::topology::fold::{TaskState, TopologyFold};

use super::super::settle;
use super::super::settle::tests::{
    ALEPH, BET, GIMEL, apply, dispatch, ev, finished, in_flight, inputs, label, question_for,
    record, record_failing, region, resume_event, retained_generation, settle_into, sha, started,
};

// -----------------------------------------------------------------------
// Fixtures the settlement lane does not need
// -----------------------------------------------------------------------

fn candidate_of(key: TaskKey, generation: u32) -> CandidateRef {
    CandidateRef {
        key,
        generation: GenerationId(generation),
        commit_sha: sha(&format!("commit-{}", label(key))),
        candidate_ref: GitRef(format!(
            "refs/upstroke/select/candidates/{}/{generation}",
            label(key)
        )),
    }
}

/// Take `key` all the way to a queued candidate: dispatch, attempt,
/// success, prepare, create.
fn queue_candidate(fold: &mut TopologyFold, key: TaskKey, generation: u32) -> CandidateRef {
    in_flight(fold, key, generation);
    // **No `attempt_finished` between the pin and `candidate_prepared`.**
    // `candidate_prepared` is the sole successful settlement for a
    // candidate-producing attempt, and the fold refuses either half of the
    // pair this fixture used to build — so a fixture that still appended one
    // would be refused by `apply` rather than quietly agreeing with itself.
    let candidate = candidate_of(key, generation);
    apply(
        fold,
        &ev(TopologyEventBody::CandidatePrepared {
            data: Box::new(CandidatePrepared {
                key,
                generation: GenerationId(generation),
                attempt: Box::new(record(1, Some(0.25))),
                base_sha: sha("base"),
                parent_sha: sha("base"),
                tree_sha: sha(&format!("tree-{}", label(key))),
                commit_sha: candidate.commit_sha.clone(),
                message: format!("{}: select candidate", label(key)),
                prepared_ref: GitRef(format!("refs/upstroke/select/prepared/{}", label(key))),
                candidate_ref: candidate.candidate_ref.clone(),
                actual_paths: region(key),
                lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region(key) },
            }),
        }),
    );
    apply(
        fold,
        &ev(TopologyEventBody::TaskCandidateCreated {
            data: TaskCandidateCreated {
                candidate: candidate.clone(),
            },
        }),
    );
    candidate
}

/// Every task terminal and nothing queued: the state a run ends from.
fn all_failed() -> TopologyFold {
    let mut fold = started();
    for key in [ALEPH, BET, GIMEL] {
        in_flight(&mut fold, key, 0);
        settle_into(&mut fold, &finished(key, 0, 1, Next::Fail));
    }
    fold
}

/// `started()` at a stated pipeline width.
///
/// Every other selection fixture runs at `max_parallel = 3`, and the
/// comment on that number is right about why: a test that ordered an
/// integration ahead of a dispatch because the *entitlement* excluded the
/// dispatch would prove nothing about `eligibility_order`. But 3 is a
/// width `config` refuses to create a run at — `DEFAULT_MAX_PARALLEL` is 1
/// and `[engine] max_parallel` above it is rejected outright — so a suite
/// with no fixture below 3 never binds the entitlement clause of any
/// predicate, and never asks what selection does at the only width
/// production runs.
fn started_at_width(max_parallel: u32) -> TopologyFold {
    let base = settle::tests::run_started();
    let limits = TopologyLimits {
        max_parallel,
        ..base.limits
    };
    let mut fold = TopologyFold::new(inputs());
    apply(
        &mut fold,
        &ev(TopologyEventBody::RunStarted {
            data: Box::new(RunStarted4 { limits, ..base }),
        }),
    );
    fold
}

fn no_spend() -> Spend {
    Spend::new()
}

fn review_costing(cost: Option<f64>) -> crate::events::ReviewRecord {
    crate::events::ReviewRecord {
        pass: "review".to_owned(),
        agent: "aleph-Frontier-agent".to_owned(),
        model: "aleph-Frontier-model".to_owned(),
        adapter: None,
        preflight_cli_version: None,
        effort: None,
        pool: None,
        cost_usd: cost,
        outcome: crate::events::ReviewPassOutcome::Passed,
    }
}

// -----------------------------------------------------------------------
// eligibility_order
// -----------------------------------------------------------------------

/// "eligible integration precedes ready_retry precedes new ordinary
/// dispatch".
///
/// Three states that differ by exactly one removed alternative, so each
/// assertion is about the branch that *lost*: in the first, a retry and a
/// dispatch were both live and the integration still won; in the second, a
/// dispatch was live and the retry still won.
#[test]
fn an_eligible_integration_precedes_a_retry_precedes_a_dispatch() {
    let mut fold = started();
    let candidate = queue_candidate(&mut fold, GIMEL, 0);
    retained_generation(&mut fold, BET, 0);

    assert!(fold.ready_retry(BET), "the retry alternative is not live");
    assert!(fold.ready(ALEPH), "the dispatch alternative is not live");
    assert!(fold.integration_admissible());
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Integrate {
            candidate: Box::new(candidate)
        }
    );

    // Without the candidate, the retry wins over the dispatch.
    let mut fold = started();
    retained_generation(&mut fold, BET, 0);
    assert!(fold.ready(ALEPH), "the dispatch alternative is not live");
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Retry {
            key: BET,
            generation: GenerationId(0),
            // The retry runs the *next* attempt of the generation that
            // retained the session, not a first attempt of a new one.
            attempt: AttemptNumber(2),
        }
    );

    // Without either, the dispatch. Lowest key first.
    let fold = started();
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Dispatch {
            key: ALEPH,
            generation: GenerationId(0),
            continuing: false,
        }
    );
}

/// At the width production runs, the entitlement decides every branch.
///
/// `DEFAULT_MAX_PARALLEL` is 1 and `[engine] max_parallel` above it is
/// refused for a fresh run, so one held entitlement is a full pipeline. An
/// `OpenNoAttempt` generation is what a crash between `task_dispatched`
/// and `attempt_started` leaves holding it, and recovery does not close it
/// — so this is the state the resumed loop's first `select` sees.
#[test]
fn nothing_is_selected_at_width_one_while_the_single_entitlement_is_held() {
    let mut narrow = started_at_width(1);
    let candidate = queue_candidate(&mut narrow, GIMEL, 0);
    assert_eq!(
        select(&narrow, &Ceiling::unlimited(), &no_spend()),
        Step::Integrate {
            candidate: Box::new(candidate.clone())
        },
        "an eligible candidate with the slot free is selected"
    );

    apply(&mut narrow, &dispatch(ALEPH, 0));
    assert_eq!(narrow.pipeline_held(), 1);
    assert!(!narrow.pipeline_reservable(), "one of one");

    // **The entitlement's holder is the one thing still selectable**, and
    // that is `T-DISPATCH`'s "continue attempt (no spend repeats)": this
    // dispatch opened a generation and started no attempt, so the loop's
    // job is to start one in it. What the held entitlement forbids is a
    // *second* claim on it — the queued integration above is no longer
    // selected, which is what this test is measuring.
    assert_eq!(
        select(&narrow, &Ceiling::unlimited(), &no_spend()),
        Step::Dispatch {
            key: ALEPH,
            generation: GenerationId(0),
            continuing: true,
        },
        "selection spent the entitlement a second time, or lost the \
         generation it already opened"
    );

    // One slot wider, the identical state selects the integration: what
    // this asserts is the count, not something else about the fixture.
    let mut wider = started_at_width(2);
    let candidate = queue_candidate(&mut wider, GIMEL, 0);
    apply(&mut wider, &dispatch(ALEPH, 0));
    assert_eq!(wider.pipeline_held(), 1);
    assert_eq!(
        select(&wider, &Ceiling::unlimited(), &no_spend()),
        Step::Integrate {
            candidate: Box::new(candidate)
        }
    );
}

/// A dispatch opens the next dense generation, not generation zero again.
#[test]
fn a_dispatch_opens_the_next_dense_generation() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    settle_into(
        &mut fold,
        &finished(ALEPH, 0, 1, Next::RetrySameRung { resume: false }),
    );
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Dispatch {
            key: ALEPH,
            generation: GenerationId(1),
            continuing: false,
        }
    );
}

/// The candidate the queue chooses, not the head of the queue.
///
/// `first_eligible` skips an entry whose task is awaiting input rather
/// than blocking behind it, and this is the selector inheriting that
/// rather than re-deriving it.
#[test]
fn selection_takes_the_first_eligible_candidate_and_not_the_head() {
    let mut fold = started();
    let blocked = queue_candidate(&mut fold, ALEPH, 0);
    let free = queue_candidate(&mut fold, BET, 0);
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Integrate {
            candidate: Box::new(blocked)
        },
        "the queue is FIFO while every entry is eligible"
    );

    // Park ALEPH's task: its candidate keeps its place and loses its turn.
    apply(
        &mut fold,
        &ev(TopologyEventBody::QuestionRaised {
            data: crate::topology::events::QuestionRaised4 {
                question: question_for(ALEPH),
            },
        }),
    );
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Integrate {
            candidate: Box::new(free)
        }
    );
}

// -----------------------------------------------------------------------
// The ceiling
// -----------------------------------------------------------------------

/// Reported spend is derived by replaying the log, and both event kinds
/// that carry a record contribute.
#[test]
fn reported_spend_replays_both_record_carrying_events() {
    let mut fold = started();
    let mut log = Vec::new();
    let started_event = ev(TopologyEventBody::RunStarted {
        data: Box::new(super::super::settle::tests::run_started()),
    });
    log.push(started_event);

    // A failure records on `attempt_finished`.
    in_flight(&mut fold, ALEPH, 0);
    let mut failing = finished(ALEPH, 0, 1, Next::Fail);
    // The record says failed, because the settlement does — `record`'s
    // `failure: None` is "judged and accepted", which is not what an
    // `attempt_finished` can carry.
    failing.record = record_failing(
        1,
        Some(0.75),
        Some((
            crate::ladder::FailureKind::GateFailed,
            crate::ladder::FailureOrigin::Worker,
        )),
    );
    let event = settle_into(&mut fold, &failing);
    log.push(ev(TopologyEventBody::AttemptFinished {
        data: Box::new(event),
    }));

    let spend = Spend::replay(&log);
    assert!((spend.run_usd() - 0.75).abs() < f64::EPSILON);
    assert!((spend.task_usd(ALEPH) - 0.75).abs() < f64::EPSILON);
    assert!(
        (spend.task_usd(BET)).abs() < f64::EPSILON,
        "spend leaked onto a task that never ran"
    );

    // A success records on `candidate_prepared`, and a replay that only
    // walked settlements would price the run at the cost of its failures.
    let mut fold = started();
    queue_candidate(&mut fold, BET, 0);
    let queued: Vec<TopologyEvent> = vec![ev(TopologyEventBody::CandidatePrepared {
        data: Box::new(CandidatePrepared {
            key: BET,
            generation: GenerationId(0),
            attempt: Box::new(record(1, Some(0.25))),
            base_sha: sha("base"),
            parent_sha: sha("base"),
            tree_sha: sha("tree-bet"),
            commit_sha: candidate_of(BET, 0).commit_sha,
            message: "bet".to_owned(),
            prepared_ref: GitRef("refs/upstroke/select/prepared/bet".to_owned()),
            candidate_ref: candidate_of(BET, 0).candidate_ref,
            actual_paths: region(BET),
            lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region(BET) },
        }),
    })];
    let spend = Spend::replay(&queued);
    assert!((spend.run_usd() - 0.25).abs() < f64::EPSILON);

    // An unpriced route contributes nothing, which is why the number is a
    // floor and the field says so.
    let mut floored = Spend::new();
    floored.record(GIMEL, &record(1, None));
    assert!(floored.run_usd().abs() < f64::EPSILON);

    // Review spend counts too: a ceiling that priced only the implementer
    // would let a two-reviewer route run past it. The worker's dollars and
    // the two passes' are three different numbers so a sum that dropped
    // one lands somewhere this fixture does not hold.
    let mut reviewed = record(1, Some(0.125));
    reviewed.reviews = vec![
        review_costing(Some(0.25)),
        review_costing(Some(0.5)),
        // An unpriced pass, which contributes nothing and makes the total
        // a floor rather than a figure.
        review_costing(None),
    ];
    let mut with_reviews = Spend::new();
    with_reviews.record(ALEPH, &reviewed);
    assert!(
        (with_reviews.run_usd() - 0.875).abs() < f64::EPSILON,
        "review spend was dropped or double counted: {}",
        with_reviews.run_usd()
    );
    assert!((with_reviews.task_usd(ALEPH) - 0.875).abs() < f64::EPSILON);
}

/// **The selector's ceiling arm checks both budgets, not one.**
///
/// `the_run_ceiling_is_checked_before_the_task_ceiling` exercises
/// `Ceiling::breach` directly and is green for either half alone. The arm
/// is what the loop actually runs, and catalogue entries `PR7-SELECT-020`
/// and `PR7-SELECT-023` reduced `ceiling_or`'s call to
/// `ceiling.task_breach(..)` and `ceiling.run_breach(..)` respectively —
/// each dropping one comparison — and the whole suite stayed green **twice**.
///
/// The two halves need opposite fixtures, and that is the whole reason
/// neither was caught: a ceiling where both budgets are breached, or
/// neither, cannot tell the halves apart. Each case below has **headroom in
/// one budget and a breach in the other**.
#[test]
fn the_ceiling_arm_refuses_on_either_budget_alone() {
    // Case one: the task is over, the run has room. A dropped
    // `task_breach` admits an attempt the task's own budget refuses.
    let fold = started();
    assert!(fold.ready(ALEPH), "the dispatch alternative is not live");
    let mut spend = Spend::new();
    spend.record(ALEPH, &record(1, Some(0.6)));
    let only_task = Ceiling {
        run_usd: Some(10.0),
        task_usd: Some(0.5),
    };
    assert_eq!(
        only_task.run_breach(&spend),
        None,
        "the run budget must have headroom, or this case cannot tell a \
         dropped task comparison from a kept one"
    );
    match select(&fold, &only_task, &spend) {
        Step::BudgetExceeded(exceeded) => assert_eq!(
            exceeded.budget,
            BudgetKind::Task,
            "the arm named the wrong budget"
        ),
        other => panic!(
            "a task over its own ceiling was admitted because the run had \
             room: {other:?}"
        ),
    }

    // Case two, the mirror: the run is over, this task has spent nothing.
    let fold = started();
    let mut spend = Spend::new();
    spend.record(BET, &record(1, Some(2.0)));
    let only_run = Ceiling {
        run_usd: Some(1.0),
        task_usd: Some(10.0),
    };
    assert_eq!(
        only_run.task_breach(&spend, ALEPH),
        None,
        "the selected task must have headroom, or this case cannot tell a \
         dropped run comparison from a kept one"
    );
    match select(&fold, &only_run, &spend) {
        Step::BudgetExceeded(exceeded) => assert_eq!(
            exceeded.budget,
            BudgetKind::Run,
            "the arm named the wrong budget"
        ),
        other => panic!(
            "a run over its ceiling dispatched a task that had spent \
             nothing: {other:?}"
        ),
    }
}

/// The run ceiling is named before the task ceiling, and reaching a
/// ceiling is already a refusal.
#[test]
fn the_run_ceiling_is_checked_before_the_task_ceiling() {
    let ceiling = Ceiling {
        run_usd: Some(1.0),
        task_usd: Some(0.5),
    };
    let mut spend = Spend::new();
    spend.record(ALEPH, &record(1, Some(0.6)));

    // Over the task ceiling, under the run ceiling.
    assert_eq!(
        ceiling.breach(&spend, ALEPH).map(|breach| breach.budget),
        Some(BudgetKind::Task)
    );
    assert_eq!(ceiling.breach(&spend, BET), None);

    // Over both: the run ceiling is the stricter claim and is what the
    // operator is told to raise.
    spend.record(BET, &record(1, Some(0.5)));
    let breach = ceiling.breach(&spend, ALEPH).expect("over the run ceiling");
    assert_eq!(breach.budget, BudgetKind::Run);
    assert!((breach.limit_usd - 1.0).abs() < f64::EPSILON);
    assert!((breach.spent_usd - 1.1).abs() < f64::EPSILON);

    // Exactly at the ceiling refuses the next spawn.
    let mut exact = Spend::new();
    exact.record(ALEPH, &record(1, Some(1.0)));
    assert_eq!(
        ceiling.breach(&exact, GIMEL).map(|breach| breach.budget),
        Some(BudgetKind::Run)
    );
    assert_eq!(Ceiling::unlimited().breach(&exact, ALEPH), None);

    // And on the task arm, which is the same boundary and a separate
    // comparison. `0.5` and `0.5` are exact in binary, so `>` here admits
    // the spawn the operator's limit has already refused and `>=` does
    // not — there is no epsilon in which the two agree.
    let task_only = Ceiling {
        run_usd: None,
        task_usd: Some(0.5),
    };
    let mut at_task = Spend::new();
    at_task.record(BET, &record(1, Some(0.5)));
    let breach = task_only
        .breach(&at_task, BET)
        .expect("reaching the task ceiling is already a refusal");
    assert_eq!(breach.budget, BudgetKind::Task);
    assert!((breach.limit_usd - 0.5).abs() < f64::EPSILON);
    assert!((breach.spent_usd - 0.5).abs() < f64::EPSILON);
    assert_eq!(
        task_only.breach(&at_task, GIMEL),
        None,
        "one task's spend was charged to another"
    );
}

/// The ceiling is consulted only inside an admitting branch: a run with
/// nothing to spawn never records a refusal of a spawn.
#[test]
fn a_run_with_no_admissible_work_never_asks_the_ceiling() {
    let fold = all_failed();
    assert!(!fold.structurally_admissible());
    let ceiling = Ceiling {
        run_usd: Some(0.0),
        task_usd: None,
    };
    assert_eq!(
        select(&fold, &ceiling, &no_spend()),
        Step::Closure(DerivedOutcome::Ending(RunOutcome::Complete)),
        "a breached ceiling turned an ended run into a budget stop"
    );
}

// -----------------------------------------------------------------------
// checkpoint_refusals
// -----------------------------------------------------------------------

/// The checkpoint refusal, in the three shapes `checkpoint_refusals` and
/// `loop` give it.
///
/// A budget breach with structurally admissible work appends
/// `budget_exceeded` **before any spawn**; integration and run end are
/// refused **before any start append**.
#[test]
fn a_breach_appends_budget_exceeded_and_integration_and_run_end_are_refused() {
    // (1) A breach with work to do. `select` is a pure function — it
    // performs no effect and appends nothing — so "before any spawn" is
    // structural, and what the loop is handed is the event itself.
    let fold = started();
    assert!(
        fold.structurally_admissible() && fold.ready(ALEPH),
        "there is no spawn for the ceiling to refuse"
    );
    let ceiling = Ceiling {
        run_usd: Some(2.0),
        task_usd: None,
    };
    let mut spend = Spend::new();
    spend.record(BET, &record(1, Some(2.5)));

    let step = select(&fold, &ceiling, &spend);
    let Step::BudgetExceeded(exceeded) = step.clone() else {
        panic!("a breached ceiling admitted the dispatch: {step:?}");
    };
    assert_eq!(exceeded.epoch, Epoch(0));
    assert_eq!(exceeded.budget, BudgetKind::Run);
    assert!((exceeded.limit_usd - 2.0).abs() < f64::EPSILON);
    assert!((exceeded.spent_usd - 2.5).abs() < f64::EPSILON);
    assert_eq!(
        exceeded.key,
        Some(ALEPH),
        "the record must name the task whose next attempt was refused"
    );

    // It is not a start, so the checkpoint admits it, and the fold takes
    // it — after which the run is ending.
    assert_eq!(
        checkpoint(step).expect("a budget stop is not a start"),
        Admitted::BudgetExceeded(exceeded.clone())
    );
    let mut fold = fold;
    apply(
        &mut fold,
        &ev(TopologyEventBody::BudgetExceeded {
            data: *exceeded.clone(),
        }),
    );
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::BudgetExceeded)
    );

    // (2) An eligible integration is refused before the
    // `merge_verification_started` that would start one.
    let mut fold = started();
    let candidate = queue_candidate(&mut fold, GIMEL, 0);
    let step = select(&fold, &Ceiling::unlimited(), &no_spend());
    assert_eq!(
        step,
        Step::Integrate {
            candidate: Box::new(candidate.clone())
        }
    );
    let error = checkpoint(step).expect_err("this build does not integrate");
    let message = format!("{error}");
    assert!(message.contains("does not integrate"), "{message}");
    assert!(
        message.contains(candidate.candidate_ref.0.as_str()),
        "the refusal does not name what it refused: {message}"
    );

    // The ceiling is checked *inside* the integration branch and before
    // it, so a breach with an eligible integration records the stop rather
    // than the refusal.
    let mut spend = Spend::new();
    spend.record(GIMEL, &record(1, Some(9.0)));
    let step = select(
        &fold,
        &Ceiling {
            run_usd: Some(1.0),
            task_usd: None,
        },
        &spend,
    );
    let Step::BudgetExceeded(exceeded) = step.clone() else {
        panic!("the ceiling was checked after the integration decision: {step:?}");
    };
    assert_eq!(exceeded.budget, BudgetKind::Run);
    assert!((exceeded.limit_usd - 1.0).abs() < f64::EPSILON);
    assert!((exceeded.spent_usd - 9.0).abs() < f64::EPSILON);
    assert_eq!(
        exceeded.key, None,
        "no task's next attempt was refused by an integration's stop"
    );
    assert!(checkpoint(step).is_ok());

    // (3) Run-end closure is refused before `run_finished`.
    let fold = all_failed();
    let step = select(&fold, &Ceiling::unlimited(), &no_spend());
    assert_eq!(
        step,
        Step::Closure(DerivedOutcome::Ending(RunOutcome::Complete))
    );
    let error = checkpoint(step).expect_err("this build does not end a run");
    assert!(format!("{error}").contains("does not end a run"), "{error}");
}

/// Every branch an intermediate build *is* entitled to perform survives
/// the checkpoint unchanged.
#[test]
fn the_checkpoint_admits_every_branch_this_build_implements() {
    let admitted = [
        (
            Step::Retry {
                key: BET,
                generation: GenerationId(3),
                attempt: AttemptNumber(4),
            },
            Admitted::Retry {
                key: BET,
                generation: GenerationId(3),
                attempt: AttemptNumber(4),
            },
        ),
        (
            Step::Dispatch {
                key: GIMEL,
                generation: GenerationId(2),
                continuing: false,
            },
            Admitted::Dispatch {
                key: GIMEL,
                generation: GenerationId(2),
                continuing: false,
            },
        ),
        (Step::Backoff, Admitted::Backoff),
        (
            Step::HardBlock {
                questions: vec![question_for(ALEPH).id],
            },
            Admitted::HardBlock {
                questions: vec![question_for(ALEPH).id],
            },
        ),
    ];
    for (step, expected) in admitted {
        assert_eq!(checkpoint(step.clone()).expect("admitted"), expected);
    }
}

/// **Which of `Step`'s variants cross the checkpoint, counted rather than
/// asserted in prose.**
///
/// `Admitted`'s doc said "[`Step`] has seven variants and this has five.
/// The two that are missing…" for as long as `Step` had **eight** and three
/// were missing. The undercount folded `Poisoned` into the two
/// `checkpoint_refusals` branches, which is a different thing: `Integrate`
/// and `Closure` are branches this build declines to perform, and
/// `Poisoned` is the absence of a branch — the fold is not authoritative
/// and nothing is selected at all.
///
/// The `match` below has **no wildcard arm**, so adding a variant to `Step`
/// stops this file compiling until someone says which side it falls on.
/// That is the part a count in a doc comment cannot do.
#[test]
fn every_step_variant_is_admitted_or_refused_and_the_split_is_five_three() {
    let every: Vec<Step> = vec![
        Step::Poisoned,
        budget_exceeded(
            Epoch(0),
            Breach {
                budget: BudgetKind::Run,
                limit_usd: 1.0,
                spent_usd: 2.0,
            },
            Some(ALEPH),
        ),
        Step::Integrate {
            candidate: Box::new(queue_candidate(&mut started(), GIMEL, 0)),
        },
        Step::Retry {
            key: BET,
            generation: GenerationId(3),
            attempt: AttemptNumber(4),
        },
        Step::Dispatch {
            key: GIMEL,
            generation: GenerationId(2),
            continuing: false,
        },
        Step::Backoff,
        Step::HardBlock {
            questions: vec![question_for(ALEPH).id],
        },
        Step::Closure(DerivedOutcome::Ending(RunOutcome::Complete)),
    ];

    // Exhaustive by construction: no `_` arm, so a ninth variant is a
    // compile error here rather than a silently untested branch.
    let mut names = Vec::new();
    for step in &every {
        names.push(match step {
            Step::Poisoned => "Poisoned",
            Step::BudgetExceeded(_) => "BudgetExceeded",
            Step::Integrate { .. } => "Integrate",
            Step::Retry { .. } => "Retry",
            Step::Dispatch { .. } => "Dispatch",
            Step::Backoff => "Backoff",
            Step::HardBlock { .. } => "HardBlock",
            Step::Closure(_) => "Closure",
        });
    }
    // On a COPY: `names` must stay in the list's order, because it is
    // zipped with it below. Sorting it in place paired every step with
    // another step's label and the assertion read
    // `["Backoff", "Closure", "Retry"]`.
    let mut distinct = names.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        every.len(),
        "the list repeats a variant, so some variant is untested: {distinct:?}"
    );

    let (crossed, refused): (Vec<_>, Vec<_>) = every
        .into_iter()
        .zip(names)
        .partition(|(step, _)| checkpoint(step.clone()).is_ok());
    let refused: Vec<&str> = refused.into_iter().map(|(_, name)| name).collect();

    assert_eq!(
        crossed.len(),
        5,
        "the admitted count moved: {:?}",
        crossed.iter().map(|(_, n)| *n).collect::<Vec<_>>()
    );
    assert_eq!(
        refused,
        vec!["Poisoned", "Integrate", "Closure"],
        "the set that does not cross the checkpoint changed"
    );
}

// -----------------------------------------------------------------------
// The remaining branches
// -----------------------------------------------------------------------

/// The retry branch checks the ceiling before it admits the retry.
///
/// Its own branch and not the dispatch branch's: `loop` puts the check
/// inside **each** admitting branch, and `ALEPH` is `ready` here, so a
/// selector that admitted the retry unconditionally would still have a
/// later branch to fall through to and a `BudgetExceeded` to produce from
/// it. The assertion is therefore on `key`: only the retry's own check
/// names the retained task.
#[test]
fn the_retry_branch_checks_the_ceiling_and_names_the_retained_task() {
    let mut fold = started();
    retained_generation(&mut fold, BET, 0);
    assert!(fold.ready_retry(BET), "the retry branch is not live");
    assert!(fold.ready(ALEPH), "the branch below it is live");

    // `BET` is over its own ceiling; `ALEPH` has spent nothing.
    let ceiling = Ceiling {
        run_usd: None,
        task_usd: Some(1.5),
    };
    let mut spend = Spend::new();
    spend.record(BET, &record(1, Some(3.0)));

    let step = select(&fold, &ceiling, &spend);
    let Step::BudgetExceeded(exceeded) = step.clone() else {
        panic!("a breached ceiling admitted the retry: {step:?}");
    };
    assert_eq!(exceeded.epoch, Epoch(0));
    assert_eq!(exceeded.budget, BudgetKind::Task);
    assert!((exceeded.limit_usd - 1.5).abs() < f64::EPSILON);
    assert!((exceeded.spent_usd - 3.0).abs() < f64::EPSILON);
    assert_eq!(
        exceeded.key,
        Some(BET),
        "the stop must name the retained task whose next attempt was refused, not the \
         dispatch that would have run instead"
    );
    assert!(checkpoint(step).is_ok(), "a budget stop is not a start");

    // Under the ceiling, the same state runs the retry.
    assert_eq!(
        select(
            &fold,
            &Ceiling {
                run_usd: None,
                task_usd: Some(4.0),
            },
            &spend
        ),
        Step::Retry {
            key: BET,
            generation: GenerationId(0),
            attempt: AttemptNumber(2),
        }
    );
}

/// **A ready dispatch precedes the defer backoff, and both are live at once.**
///
/// The order `loop` fixes, and the one adjacent pair no fixture held.
/// `the_backoff_branch_precedes_the_hard_block_when_both_are_live` pins the
/// pair below this one; `an_eligible_integration_precedes_a_retry_precedes_a_dispatch`
/// pins the three above it. Between them sat `first_ready` / `backoff_pending`,
/// and S5 round 4 measured the swap — the defer backoff selected **before** a
/// ready dispatch, which is the starvation this module's header warns about
/// ("The order is not a scheduling preference") — leaving the **entire suite
/// green**.
///
/// A run with runnable work must not sleep on a wait that belongs to a task
/// which is not the one it could be running.
#[test]
fn a_ready_dispatch_precedes_the_backoff_when_both_are_live() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Defer));

    // Both premises, because "not Backoff" is satisfied by a fold where the
    // backoff was never pending in the first place.
    assert!(
        fold.backoff_pending(),
        "no task is deferred, so this asserts nothing about the branch order"
    );
    assert!(
        fold.ready(BET),
        "no task is ready, so the branch above the backoff is not live"
    );

    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Dispatch {
            key: BET,
            generation: GenerationId(0),
            continuing: false,
        },
        "a run with runnable work slept on another task's defer wait. `loop`'s order is not \
         a scheduling preference: the deferred task is waiting on a wait that elapses by \
         itself, and the ready one is waiting on nothing"
    );
}

/// Backoff precedes the hard block, and the two are live at once.
///
/// `loop`'s order is fixed, and no other fixture holds a `Deferred` task
/// **and** an open question at the same time — so with the two branches
/// swapped every one of them still passes. A deferred task is waiting on a
/// wait that will elapse on its own; a question waits on a person. Serving
/// the person first would park a run that was about to make progress.
#[test]
fn the_backoff_branch_precedes_the_hard_block_when_both_are_live() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Defer));
    assert_eq!(fold.task_state(ALEPH), Some(TaskState::Deferred));

    in_flight(&mut fold, BET, 0);
    let mut parking = finished(BET, 0, 1, Next::AskHuman(crate::ir::QuestionKind::Unblock));
    parking.question = Some(question_for(BET));
    settle_into(&mut fold, &parking);

    in_flight(&mut fold, GIMEL, 0);
    settle_into(&mut fold, &finished(GIMEL, 0, 1, Next::Fail));

    assert!(fold.backoff_pending(), "the backoff branch is not live");
    assert!(fold.questions_open(), "the hard-block branch is not live");
    assert!(
        !fold.structurally_admissible(),
        "a branch above both is live, and this asserts nothing about their order"
    );
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Backoff,
        "the hard-block rules were applied to a run that was about to wake"
    );

    // With the wait elapsed and the woken task run out, the question is
    // what is left — the other half of the same order.
    apply(&mut fold, &resume_event());
    assert!(!fold.backoff_pending(), "the resume woke the deferred task");
    in_flight(&mut fold, ALEPH, 1);
    settle_into(&mut fold, &finished(ALEPH, 1, 1, Next::Fail));
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::HardBlock {
            questions: vec![question_for(BET).id]
        }
    );
}

/// An integration is charged to the run and never to the candidate's task.
///
/// `BudgetExceeded4::key` is "the task whose next attempt was refused. Not
/// a failed task: nothing judged it and nothing was spent on it", and an
/// integration is neither half: it is not that task's next attempt, and
/// money *was* spent — the candidate exists because an attempt succeeded
/// and was paid for. Charging the task ceiling would refuse the merge of
/// work already bought, and refuse it permanently: the candidate can never
/// integrate and the task can never unspend.
#[test]
fn an_integration_is_charged_to_the_run_and_never_to_the_candidates_task() {
    let mut fold = started();
    let candidate = queue_candidate(&mut fold, GIMEL, 0);

    let mut spend = Spend::new();
    spend.record(GIMEL, &record(1, Some(9.0)));
    let task_only = Ceiling {
        run_usd: None,
        task_usd: Some(1.0),
    };
    assert_eq!(
        select(&fold, &task_only, &spend),
        Step::Integrate {
            candidate: Box::new(candidate)
        },
        "a task ceiling refused the merge of work it had already paid for"
    );
    // The same ceiling still refuses that task's next *attempt*, which is
    // what it is a ceiling on.
    assert_eq!(
        task_only.breach(&spend, GIMEL).map(|breach| breach.budget),
        Some(BudgetKind::Task)
    );
}

/// The backoff branch, and the guard that keeps it out from under a halt
/// or a budget stop.
#[test]
fn the_backoff_branch_is_entered_only_while_the_run_is_not_ending() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Defer));
    in_flight(&mut fold, BET, 0);
    settle_into(&mut fold, &finished(BET, 0, 1, Next::Fail));
    in_flight(&mut fold, GIMEL, 0);
    settle_into(&mut fold, &finished(GIMEL, 0, 1, Next::Fail));
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Backoff
    );

    // Waking it returns the task to an ordinary dispatch.
    let mut woken = fold.clone();
    apply(&mut woken, &resume_event());
    assert_eq!(
        select(&woken, &Ceiling::unlimited(), &no_spend()),
        Step::Dispatch {
            key: ALEPH,
            generation: GenerationId(1),
            continuing: false,
        }
    );

    // A halt: the branch is not offered, and the closure is.
    let mut halted = started();
    in_flight(&mut halted, ALEPH, 0);
    settle_into(&mut halted, &finished(ALEPH, 0, 1, Next::Defer));
    in_flight(&mut halted, BET, 0);
    let mut halting = finished(BET, 0, 1, Next::Fail);
    halting.halts_run = true;
    settle_into(&mut halted, &halting);
    assert_eq!(
        select(&halted, &Ceiling::unlimited(), &no_spend()),
        Step::Closure(DerivedOutcome::Ending(RunOutcome::Halted))
    );

    // A budget stop in this epoch: likewise.
    let mut stopped = started();
    in_flight(&mut stopped, ALEPH, 0);
    settle_into(&mut stopped, &finished(ALEPH, 0, 1, Next::Defer));
    apply(
        &mut stopped,
        &super::super::settle::tests::budget_exceeded(Epoch(0), BET),
    );
    assert_eq!(
        select(&stopped, &Ceiling::unlimited(), &no_spend()),
        Step::Closure(DerivedOutcome::Ending(RunOutcome::BudgetExceeded))
    );
}

/// The label of a step, total over [`Step`].
///
/// The reason it is a `match` and not a list: adding a variant to `Step` is
/// a **compile error here**, which is what lets
/// [`an_ending_run_offers_no_work_from_any_arm`] claim "every arm" and have
/// the claim mean something. A list of names someone remembers to extend is
/// how that test came to cover three of six while its own doc said every.
fn arm_label(step: &Step) -> &'static str {
    match step {
        Step::Poisoned => "Poisoned",
        Step::BudgetExceeded(_) => "BudgetExceeded",
        Step::Integrate { .. } => "Integrate",
        Step::Retry { .. } => "Retry",
        Step::Dispatch {
            continuing: true, ..
        } => "Dispatch (continuing)",
        Step::Dispatch {
            continuing: false, ..
        } => "Dispatch",
        Step::Backoff => "Backoff",
        Step::HardBlock { .. } => "HardBlock",
        Step::Closure(_) => "Closure",
    }
}

/// Every label [`arm_label`] can return for a step that **offers work**.
///
/// **Below `arm_label`, not above it.** These two `const`s were inserted
/// between that function's doc block and the function, so the block
/// attached to `OFFERS_WORK` and `arm_label` rendered undocumented —
/// occurrence 10 of `reviews/FINDINGS.md` §4's doc-re-targeting class,
/// committed by the commit whose ledger entry corrected that class's own
/// count. `clippy::doc_lazy_continuation` does not fire here because the
/// stranded block's last line is prose rather than a list item, which is
/// the half §4 records that detector cannot see. `PR7-R6-ATT-005`.
const OFFERS_WORK: &[&str] = &[
    "Integrate",
    "Retry",
    "Dispatch",
    "Dispatch (continuing)",
    "Backoff",
    "HardBlock",
];

/// And every label for a step that does not. None of the three is work, and
/// each is a state an ending run is allowed to reach.
///
/// **Pinned by name, and that is what ties membership to behaviour.** The
/// census below checks only that `arm_label`'s literals equal the union of
/// these two lists, so moving a *work* label into this one would satisfy it
/// and quietly drop that arm from the ending witness's coverage
/// requirement — `PR7-R6-LOOP-008`. These three are structural and cannot
/// grow: a poisoned fold, a budget stop, and closure. So a seventh label has
/// exactly one place to go, and the coverage assertion then demands a case
/// for it.
const OFFERS_NO_WORK: &[&str] = &["Poisoned", "BudgetExceeded", "Closure"];

/// **Every label [`arm_label`] can return is classified.**
///
/// The half of "every arm" that the type does not give. `arm_label` is total
/// over [`Step`], so a new variant is a compile error there — measured by S5
/// round 5, which added a `Step::Provision` and saw `E0004` exactly as
/// claimed. But the claim went one step further and said the new arm "cannot
/// then be left out of this test without the coverage assertion failing",
/// and **that half was false**: once the author satisfies the compiler with
/// `Step::Provision => "Provision"`, [`OFFERS_WORK`] is a hand-written
/// `const` nothing forces them to extend, and
/// `an_ending_run_offers_no_work_from_any_arm` passes with the new arm
/// undriven. `PR7-R5-LOOP-002`, `R5-SEAMS-004`, `R5-SETTLE-003`.
///
/// So this closes the loop from the other end: it reads `arm_label`'s own
/// match body out of this file and asserts every literal it returns appears
/// in exactly one of the two lists. A new variant now costs three edits and
/// **none of them can be skipped** — the match arm (rustc), the
/// classification (this test), and, if it offers work, a case in the
/// witness (that test's own coverage assertion).
///
/// The body is bounded by brace matching rather than by a line count,
/// because this file's own history has three occurrences of an anchor going
/// stale under `cargo fmt` alone.
#[test]
fn every_label_the_arm_classifier_returns_is_classified() {
    const SIGNATURE: &str = "fn arm_label(step: &Step) -> &'static str {";

    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/engine/topology/select/tests.rs"),
    )
    .expect("this file is readable");
    let at = source
        .find(SIGNATURE)
        .expect("`arm_label`'s signature moved; this census cannot find its body");
    let open = at + SIGNATURE.len() - 1;
    let mut depth = 0_usize;
    let mut end = None;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &source[open..end.expect("`arm_label`'s body is brace-balanced")];

    let mut returned: Vec<String> = body
        .match_indices("=> \"")
        .map(|(at, _)| {
            let rest = &body[at + 4..];
            rest[..rest.find('"').expect("a closed string literal")].to_owned()
        })
        .collect();
    returned.sort_unstable();
    returned.dedup();

    let mut classified: Vec<String> = OFFERS_WORK
        .iter()
        .chain(OFFERS_NO_WORK.iter())
        .map(|label| (*label).to_owned())
        .collect();
    classified.sort_unstable();

    assert!(
        returned.len() >= 9,
        "`arm_label` returns {} distinct labels, so this census found a body it could not \
         read rather than a classifier: {returned:?}",
        returned.len()
    );
    assert_eq!(
        OFFERS_NO_WORK,
        ["Poisoned", "BudgetExceeded", "Closure"],
        "the not-work list is pinned by name: without that, moving a work label into it \
         satisfies the equality below and drops that arm from the ending witness's coverage \
         requirement, which is the one way a seventh arm can still be added and left undriven"
    );
    assert_eq!(
        returned, classified,
        "`arm_label` returns a label that neither `OFFERS_WORK` nor `OFFERS_NO_WORK` names, \
         or names one it cannot return. A new `Step` variant is a compile error in \
         `arm_label` and nothing else, so without this the author satisfies rustc, leaves \
         the lists alone, and the witness below passes with the new arm undriven"
    );
}

/// **An ending run offers no work — from every arm, not from the empty fold.**
///
/// The property is "an ending run proceeds to closure". What was asserted
/// before `PR7-R3-LOOP-001` was "an *idle* ending run does":
/// `a_run_with_no_admissible_work_never_asks_the_ceiling` drives an
/// `all_failed()` fold, where **nothing else is live**, and
/// `a_breach_appends_budget_exceeded_and_integration_and_run_end_are_refused`
/// asserts the closure on the same shape. That is the scoping gap round 3
/// harvested.
///
/// **A correction, because the version of this comment that shipped cited a
/// test that does not exist.** It named `an_ending_run_reaches_closure` as
/// the predecessor whose scope this widens;
/// ```text
/// $ grep -rn 'an_ending_run_reaches_closure' --include='*.rs' src/ | grep -v '///'
/// (no output)
/// ```
///
/// **Zero code occurrences.** The doc-comment filter is not tidiness: this
/// sentence quotes the name, so the unfiltered command matches *itself* and
/// reports a hit for a test that does not exist. `reviews/FINDINGS.md` §4
/// carries that as a class — a command quoted as evidence becomes part of
/// its own input — and it is the documentation half of
/// `PR4-CENSUS-COMMENT-ORACLE`. The two tests named above are the real
/// predecessors. §19, claim (1).
///
/// `PR7-R3-LOOP-001` is what got through the gap.
/// `TopologyFold::open_no_attempt` is a statement accessor and — correctly,
/// and unlike `ready`, `ready_retry` and `integration_admissible` — consults
/// no run state, so the continuation arm offered work on a budget-stopped
/// run. Measured end to end by that lens: five `step()` calls, five
/// duplicate `budget_exceeded` records, no closure; and with `halted_at`
/// set, `Dispatch { continuing: true }` — a halted run spawning a worker.
///
/// **"Every arm" is now the whole of `select`'s work-offering surface**, and
/// it is checkable rather than asserted: [`arm_label`] is total over `Step`,
/// [`OFFERS_WORK`] is the subset of its labels that offer work, and the six
/// cases below are asserted to *cover that subset exactly*. A seventh arm
/// cannot be added without `arm_label` failing to compile, and it cannot
/// then be left out of this test without the coverage assertion failing.
/// The version this replaces covered `Dispatch`, `Dispatch (continuing)` and
/// `Retry`, and its doc claimed all of them — §19, claim (5).
///
/// The historical top-guard mutation, before continuation acquired its own
/// eligibility reader, reported two arms:
///
/// ```text
/// Dispatch (continuing) -> Dispatch { …, continuing: true }
/// HardBlock             -> HardBlock { questions: [QuestionId("q-aleph-park")] }
/// ```
///
/// Five of the six now have another ending check: `ready`, `ready_retry`,
/// `integration_admissible` and `eligible_continuation` embed it in the fold,
/// and this module's `backoff_pending` wrapper embeds it here. `HardBlock`
/// still depends on the top guard because `questions_open` is an accounting
/// accessor. The historical two-arm mutation is not a measurement of this
/// revised implementation; the assertions retain all six positive cases.
///
/// `HardBlock` was not witnessed before this widening. The guard already
/// covered it, so this is not an open defect — it is the difference between
/// a guard that happens to be correct and one that is held to it, and the
/// three-arm version could not tell them apart.
#[test]
fn an_ending_run_offers_no_work_from_any_arm() {
    // Each case: a fold where THIS arm is live, then the same fold ended.
    // The live assertion is the premise, and it names the arm — asserting
    // merely "not closure" let a case pass on a *different* arm being live,
    // which is how three cases were mistaken for six.
    /// A fixture builder for one arm, and the label its fold must select.
    type Arm = (&'static str, fn() -> TopologyFold);

    let cases: Vec<Arm> = vec![
        ("Dispatch (continuing)", || {
            let mut fold = started();
            apply(&mut fold, &dispatch(ALEPH, 0));
            fold
        }),
        ("Dispatch", started),
        ("Retry", || {
            let mut fold = started();
            retained_generation(&mut fold, BET, 0);
            fold
        }),
        ("Integrate", || {
            let mut fold = started();
            let _ = queue_candidate(&mut fold, GIMEL, 0);
            fold
        }),
        ("Backoff", || {
            // Deferred work and nothing else runnable: `Backoff` sits below
            // the three dispatching arms, so every other task must be out.
            let mut fold = started();
            in_flight(&mut fold, ALEPH, 0);
            settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Defer));
            in_flight(&mut fold, BET, 0);
            settle_into(&mut fold, &finished(BET, 0, 1, Next::Fail));
            in_flight(&mut fold, GIMEL, 0);
            settle_into(&mut fold, &finished(GIMEL, 0, 1, Next::Fail));
            fold
        }),
        ("HardBlock", || {
            // An open question and nothing else — including no deferred
            // task, because `Backoff` precedes this branch.
            let mut fold = started();
            in_flight(&mut fold, ALEPH, 0);
            let mut parking = finished(
                ALEPH,
                0,
                1,
                Next::AskHuman(crate::ir::QuestionKind::Unblock),
            );
            parking.question = Some(question_for(ALEPH));
            settle_into(&mut fold, &parking);
            for key in [BET, GIMEL] {
                in_flight(&mut fold, key, 0);
                settle_into(&mut fold, &finished(key, 0, 1, Next::Fail));
            }
            fold
        }),
    ];

    let mut covered: Vec<&'static str> = Vec::new();
    let mut offered: Vec<String> = Vec::new();
    for (arm, build) in cases {
        let live = build();
        let selected = select(&live, &Ceiling::unlimited(), &no_spend());
        assert_eq!(
            arm_label(&selected),
            arm,
            "{arm}: this fixture selects `{}` before the run ends, so ending it says nothing \
             about `{arm}` — it says a second time what the `{}` case already says",
            arm_label(&selected),
            arm_label(&selected)
        );
        covered.push(arm);

        let mut ending = build();
        apply(
            &mut ending,
            &super::super::settle::tests::budget_exceeded(Epoch(0), GIMEL),
        );
        let after = select(&ending, &Ceiling::unlimited(), &no_spend());
        if !matches!(after, Step::Closure(_)) {
            // Accumulated rather than asserted in the loop: a guard that
            // stops covering three arms should report three, not the first
            // one the case order happens to reach.
            offered.push(format!("{arm} -> {after:?}"));
        }
    }

    assert!(
        offered.is_empty(),
        "an ending run offered work from {} of the {} arms. `loop` says a breach proceeds to \
         closure, and a run that keeps selecting an arm it then refuses appends a duplicate \
         stop record every iteration and never terminates:\n  {}",
        offered.len(),
        OFFERS_WORK.len(),
        offered.join("\n  ")
    );

    covered.sort_unstable();
    let mut expected = OFFERS_WORK.to_vec();
    expected.sort_unstable();
    assert_eq!(
        covered, expected,
        "the arms this test drives are not the arms `select` can offer work from. An arm in \
         the second list and not the first is an arm nothing here holds to the rule — which \
         is the defect this test carried while its own doc said `every`"
    );
}

/// **A halted run offers no work either — the guard's other disjunct.**
///
/// `run_is_ending()` is `halted_at.is_some() || budget_stop_is_current()`
/// and every case in [`an_ending_run_offers_no_work_from_any_arm`] ends its
/// run the second way. So the halted half was unpinned: S5 round 4 measured
/// `if fold.run_is_ending() && fold.halted_at().is_none()` — the guard with
/// the halted disjunct dropped — surviving the **whole suite**, twice.
///
/// A halted run that keeps offering work is the worse of the two: a budget
/// stop at least appends a record each iteration, and `halts_run` is set by
/// a task that asked the run to stop.
///
/// The two cases retain the original continuation and question witnesses.
/// Continuation now also consults a fold eligibility reader that refuses a
/// halted run. The question branch still depends on the top guard alone.
#[test]
fn a_halted_run_offers_no_work_from_the_arms_that_rest_on_the_guard() {
    /// `BET` fails, and `halts` decides whether it asks the run to stop.
    ///
    /// **One field varied and everything else held constant**, because a
    /// halted run cannot be built by *adding* a settlement to a fold where
    /// the hard block is already live: the hard block needs every task
    /// settled, and a halt needs a task left to settle. The control fold is
    /// the same fold with `halts_run = false`, so the comparison isolates
    /// the flag rather than the shape.
    fn settle_bet(fold: &mut TopologyFold, halts: bool) {
        in_flight(fold, BET, 0);
        let mut settlement = finished(BET, 0, 1, Next::Fail);
        settlement.halts_run = halts;
        settle_into(fold, &settlement);
    }

    /// The continuation arm now has both the top guard and its fold reader's
    /// ending check. Keep its accepted live control and halted refusal.
    fn continuation(halts: bool) -> TopologyFold {
        let mut fold = started();
        apply(&mut fold, &dispatch(ALEPH, 0));
        settle_bet(&mut fold, halts);
        fold
    }

    /// The hard block: `TopologyFold::questions_open` is the same shape one
    /// accessor over.
    fn hard_block(halts: bool) -> TopologyFold {
        let mut fold = started();
        in_flight(&mut fold, ALEPH, 0);
        let mut parking = finished(
            ALEPH,
            0,
            1,
            Next::AskHuman(crate::ir::QuestionKind::Unblock),
        );
        parking.question = Some(question_for(ALEPH));
        settle_into(&mut fold, &parking);
        in_flight(&mut fold, GIMEL, 0);
        settle_into(&mut fold, &finished(GIMEL, 0, 1, Next::Fail));
        settle_bet(&mut fold, halts);
        fold
    }

    for (arm, build) in [
        (
            "Dispatch (continuing)",
            continuation as fn(bool) -> TopologyFold,
        ),
        ("HardBlock", hard_block),
    ] {
        let live = build(false);
        assert!(
            live.halted_at().is_none(),
            "{arm}: the control fold halted the run, so it is not a control"
        );
        assert_eq!(
            arm_label(&select(&live, &Ceiling::unlimited(), &no_spend())),
            arm,
            "{arm}: the arm is not live in the control fold, so halting the same fold proves \
             nothing about it"
        );

        let halted = build(true);
        // The premise: this ends the run the **other** way. A fixture that
        // also carried a current budget stop would pass with the halted
        // disjunct deleted, which is the mutation this test exists for.
        assert!(
            halted.halted_at().is_some(),
            "{arm}: the fixture did not halt the run"
        );
        assert!(
            halted.budget_stop().is_none(),
            "{arm}: the fixture also budget-stopped the run, so it cannot tell the guard's \
             two disjuncts apart"
        );

        let after = select(&halted, &Ceiling::unlimited(), &no_spend());
        assert!(
            matches!(after, Step::Closure(_)),
            "{arm}: a halted run offered work. A budget stop at least appends a record each \
             iteration; a halted run that keeps selecting was asked to stop by one of its own \
             tasks: {after:?}"
        );
    }
}

/// The hard-block branch: open questions and nothing else runnable.
#[test]
fn open_questions_reach_the_hard_block_branch_before_closure() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    let mut parking = finished(
        ALEPH,
        0,
        1,
        Next::AskHuman(crate::ir::QuestionKind::Unblock),
    );
    parking.question = Some(question_for(ALEPH));
    settle_into(&mut fold, &parking);
    for key in [BET, GIMEL] {
        in_flight(&mut fold, key, 0);
        settle_into(&mut fold, &finished(key, 0, 1, Next::Fail));
    }
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::HardBlock {
            questions: vec![question_for(ALEPH).id]
        },
        "the loop applies the hard-block rules before it closes the run"
    );
    // Left to itself the fold would already end this run Parked, which is
    // exactly why the branch order matters.
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Parked)
    );
}

/// A poisoned fold selects nothing and is refused.
#[test]
fn a_poisoned_fold_selects_nothing() {
    let mut fold = started();
    assert!(fold.ready(ALEPH));
    fold.poison();
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Poisoned
    );
    let error = checkpoint(Step::Poisoned).expect_err("a poisoned fold authorises nothing");
    assert!(format!("{error}").contains("poisoned"), "{error}");
}

/// A fold with no `run_started` has recorded nothing, so nothing is
/// selectable and nothing has ended.
#[test]
fn an_unstarted_run_selects_nothing() {
    let fold = TopologyFold::new(inputs());
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Closure(DerivedOutcome::NotEnding)
    );
    checkpoint(select(&fold, &Ceiling::unlimited(), &no_spend()))
        .expect_err("nothing is admitted from a run that has not started");
}

/// The retry the selector names is the one the settlement module runs.
///
/// Two modules deciding "which generation, which attempt" independently is
/// two rules that can disagree; this is the assertion that they do not.
#[test]
fn the_selected_retry_is_the_one_the_settlement_module_runs() {
    let mut fold = started();
    retained_generation(&mut fold, BET, 0);
    let Step::Retry {
        key,
        generation,
        attempt,
    } = select(&fold, &Ceiling::unlimited(), &no_spend())
    else {
        panic!("a retained generation is not selected for retry");
    };

    let mut reservations = super::super::identity::Reservations::new();
    let worktrees = settle::tests::FixedVerify::passing();
    let mut hooks = super::super::seams::HarnessTopologyHooks::new(std::sync::Arc::new(
        std::sync::Mutex::new(crate::topology::effects::HookHarness::new()),
    ));
    let outcome = settle::retry(
        &fold,
        &mut reservations,
        &worktrees,
        <super::super::seams::HarnessTopologyHooks as super::super::seams::TopologyHooks>::effects(
            &mut hooks,
        ),
        &settle::tests::retry_request(key, generation.0),
    )
    .expect("the retry runs");
    let settle::RetryOutcome::Start(started_event) = outcome else {
        panic!("a verified worktree starts the attempt");
    };
    assert_eq!(started_event.key, key);
    assert_eq!(started_event.generation, generation);
    assert_eq!(started_event.attempt, attempt);
}
