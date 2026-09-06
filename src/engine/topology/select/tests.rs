//! Extended notes: `docs/internals/engine/topology/select/tests.md`

use super::*;
use crate::events::RunOutcome;
use crate::ladder::Next;
use crate::topology::events::{
    CandidateLeaseEffect, CandidatePrepared, GitRef, RunStarted4, TaskCandidateCreated,
    TopologyLimits,
};
use crate::topology::fold::TopologyFold;

use super::super::settle;
use super::super::settle::tests::{
    ALEPH, BET, GIMEL, apply, dispatch, ev, finished, in_flight, inputs, label, question_for,
    record, record_failing, region, resume_event, retained_generation, settle_into, sha, started,
};

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

fn queue_candidate(fold: &mut TopologyFold, key: TaskKey, generation: u32) -> CandidateRef {
    in_flight(fold, key, generation);
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

fn all_failed() -> TopologyFold {
    let mut fold = started();
    for key in [ALEPH, BET, GIMEL] {
        in_flight(&mut fold, key, 0);
        settle_into(&mut fold, &finished(key, 0, 1, Next::Fail));
    }
    fold
}

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

    let mut fold = started();
    retained_generation(&mut fold, BET, 0);
    assert!(fold.ready(ALEPH), "the dispatch alternative is not live");
    assert_eq!(
        select(&fold, &Ceiling::unlimited(), &no_spend()),
        Step::Retry {
            key: BET,
            generation: GenerationId(0),
            attempt: AttemptNumber(2),
        }
    );

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

#[test]
fn reported_spend_replays_both_record_carrying_events() {
    let mut fold = started();
    let mut log = Vec::new();
    let started_event = ev(TopologyEventBody::RunStarted {
        data: Box::new(super::super::settle::tests::run_started()),
    });
    log.push(started_event);

    in_flight(&mut fold, ALEPH, 0);
    let mut failing = finished(ALEPH, 0, 1, Next::Fail);
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

    let mut floored = Spend::new();
    floored.record(GIMEL, &record(1, None));
    assert!(floored.run_usd().abs() < f64::EPSILON);

    let mut reviewed = record(1, Some(0.125));
    reviewed.reviews = vec![
        review_costing(Some(0.25)),
        review_costing(Some(0.5)),
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

#[test]
fn the_ceiling_arm_refuses_on_either_budget_alone() {
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

#[test]
fn the_run_ceiling_is_checked_before_the_task_ceiling() {
    let ceiling = Ceiling {
        run_usd: Some(1.0),
        task_usd: Some(0.5),
    };
    let mut spend = Spend::new();
    spend.record(ALEPH, &record(1, Some(0.6)));

    assert_eq!(
        ceiling.breach(&spend, ALEPH).map(|breach| breach.budget),
        Some(BudgetKind::Task)
    );
    assert_eq!(ceiling.breach(&spend, BET), None);

    spend.record(BET, &record(1, Some(0.5)));
    let breach = ceiling.breach(&spend, ALEPH).expect("over the run ceiling");
    assert_eq!(breach.budget, BudgetKind::Run);
    assert!((breach.limit_usd - 1.0).abs() < f64::EPSILON);
    assert!((breach.spent_usd - 1.1).abs() < f64::EPSILON);

    let mut exact = Spend::new();
    exact.record(ALEPH, &record(1, Some(1.0)));
    assert_eq!(
        ceiling.breach(&exact, GIMEL).map(|breach| breach.budget),
        Some(BudgetKind::Run)
    );
    assert_eq!(Ceiling::unlimited().breach(&exact, ALEPH), None);

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

#[test]
fn a_breach_appends_budget_exceeded_and_integration_and_run_end_are_refused() {
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

    let fold = all_failed();
    let step = select(&fold, &Ceiling::unlimited(), &no_spend());
    assert_eq!(
        step,
        Step::Closure(DerivedOutcome::Ending(RunOutcome::Complete))
    );
    let error = checkpoint(step).expect_err("this build does not end a run");
    assert!(format!("{error}").contains("does not end a run"), "{error}");
}

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

#[test]
fn the_retry_branch_checks_the_ceiling_and_names_the_retained_task() {
    let mut fold = started();
    retained_generation(&mut fold, BET, 0);
    assert!(fold.ready_retry(BET), "the retry branch is not live");
    assert!(fold.ready(ALEPH), "the branch below it is live");

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

#[test]
fn a_ready_dispatch_precedes_the_backoff_when_both_are_live() {
    let mut fold = started();
    in_flight(&mut fold, ALEPH, 0);
    settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Defer));

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
    assert_eq!(
        task_only.breach(&spend, GIMEL).map(|breach| breach.budget),
        Some(BudgetKind::Task)
    );
}

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

const OFFERS_WORK: &[&str] = &[
    "Integrate",
    "Retry",
    "Dispatch",
    "Dispatch (continuing)",
    "Backoff",
    "HardBlock",
];

const OFFERS_NO_WORK: &[&str] = &["Poisoned", "BudgetExceeded", "Closure"];

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

#[test]
fn an_ending_run_offers_no_work_from_any_arm() {
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

#[test]
fn a_halted_run_offers_no_work_from_the_arms_that_rest_on_the_guard() {
    fn settle_bet(fold: &mut TopologyFold, halts: bool) {
        in_flight(fold, BET, 0);
        let mut settlement = finished(BET, 0, 1, Next::Fail);
        settlement.halts_run = halts;
        settle_into(fold, &settlement);
    }

    fn continuation(halts: bool) -> TopologyFold {
        let mut fold = started();
        apply(&mut fold, &dispatch(ALEPH, 0));
        settle_bet(&mut fold, halts);
        fold
    }

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
    assert_eq!(
        fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Parked)
    );
}

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
