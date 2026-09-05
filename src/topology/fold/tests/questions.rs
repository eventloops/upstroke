use super::*;

struct Trace {
    fold: TopologyFold,
    events: Vec<TopologyEvent>,
    inputs: FrozenInputs,
}

impl Trace {
    fn started() -> Self {
        Self {
            fold: started(),
            events: vec![run_started_event()],
            inputs: inputs(),
        }
    }

    fn wide_started() -> Self {
        Self {
            fold: wide_started(3),
            events: vec![wide_run_started_event(3)],
            inputs: wide_inputs(),
        }
    }

    fn record(&mut self, event: TopologyEvent) {
        apply(&mut self.fold, &event);
        self.events.push(event);
    }

    fn queue(&mut self, key: TaskKey) {
        let base = sha("base");
        self.record(dispatch(key, 0, &base));
        self.record(attempt_started(&self.fold, key, 0, 1, 0));
        self.record(candidate_prepared(key, 0, &base));
        self.record(candidate_created(key, 0));
    }

    fn replay(&self) {
        let parsed = TopologyFold::parse_log(&wire(&self.events)).expect("checked log parses");
        // Replay owns an independent copy of the same frozen inputs.
        let replay =
            TopologyFold::replay(self.inputs.clone(), &parsed).expect("checked log replays");
        assert_eq!(self.fold.state(), replay.state());
        assert_eq!(self.fold.derived_outcome(), replay.derived_outcome());
    }
}

fn answer(key: TaskKey, id: &str) -> TopologyEvent {
    answered(
        key,
        id,
        Answer4::Answered {
            option_index: 0,
            binding_override: None,
        },
    )
}

fn reject_into_question(root: TaskKey, repair: TaskKey, id: &str) -> TopologyEvent {
    let mut spawn = repair_spawn(repair, root, root);
    spawn.entry.deps.clear();
    spawn.entry.display_deps.clear();
    spawn.admission = SpawnAdmission::HumanRequired {
        limit: run_started().limits.max_merge_repairs,
        question: question(id, repair),
    };
    ev(TopologyEventBody::MergeRejected {
        data: Box::new(MergeRejected {
            sequence: SequenceId(0),
            candidate: candidate_of(root, 0),
            rejecting_head: sha("head"),
            disposition: RejectionDisposition::Conflict {
                paths: region(root),
            },
            repair: spawn,
            lease_effect: RejectionLeaseEffect::CreatesLineage {
                root,
                paths: region(root),
            },
        }),
    })
}

#[test]
fn answering_a_queued_task_preserves_its_candidate_and_prevents_redispatch() {
    let mut trace = Trace::started();
    trace.queue(ALPHA);
    trace.record(raised("queued", ALPHA));
    trace.record(answer(ALPHA, "queued"));

    assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::AwaitingMerge));
    assert!(trace.fold.queue().expect("started").holds_task(ALPHA));
    assert!(!trace.fold.ready(ALPHA));
    refuse(&trace.fold, &dispatch(ALPHA, 1, &sha("next-base")));
    accepts(
        &trace.fold,
        &fast_publication(ALPHA, 0, 0, &sha("base"), vec![ALPHA]),
    );
    trace.replay();
}

#[test]
fn answering_one_question_keeps_a_queued_task_parked_until_its_last_answer() {
    for order in [["first", "second"], ["second", "first"]] {
        let mut trace = Trace::started();
        trace.queue(ALPHA);
        trace.record(raised("first", ALPHA));
        trace.record(raised("second", ALPHA));
        trace.record(answer(ALPHA, order[0]));

        assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::AwaitingInput));
        assert!(!trace.fold.integration_admissible());
        refuse(
            &trace.fold,
            &fast_publication(ALPHA, 0, 0, &sha("base"), vec![ALPHA]),
        );
        trace.replay();

        trace.record(answer(ALPHA, order[1]));
        assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::AwaitingMerge));
        assert!(trace.fold.integration_admissible());
        trace.replay();
    }
}

fn deferred_task() -> Trace {
    let mut trace = Trace::started();
    trace.record(dispatch(ALPHA, 0, &sha("base")));
    trace.record(attempt_started(&trace.fold, ALPHA, 0, 1, 0));
    trace.record(settle_failing(
        ALPHA,
        0,
        1,
        crate::ladder::FailureKind::RateLimited,
        AttemptSettlement::Closed {
            transition: SettlementTransition::Deferred {
                defers: 1,
                reason: "provider unavailable".to_owned(),
            },
            lease: LeaseDisposition::PredictedReleased,
        },
    ));
    trace
}

#[test]
fn answering_a_deferred_task_does_not_clear_its_unelapsed_backoff() {
    let mut trace = deferred_task();
    trace.record(raised("backoff", ALPHA));
    assert!(trace.fold.backoff_pending(), "the task still owes a wait");
    trace.record(answer(ALPHA, "backoff"));
    assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::Deferred));
    assert!(!trace.fold.ready(ALPHA));
    trace.replay();
}

#[test]
fn a_durable_wake_reaches_a_deferred_task_hidden_by_questions() {
    for wake in [
        ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 30_000,
                round: 1,
            },
        }),
        resume(container_runner()),
    ] {
        let mut trace = deferred_task();
        trace.record(raised("first", ALPHA));
        trace.record(raised("second", ALPHA));
        trace.record(wake);
        assert!(!trace.fold.backoff_pending());
        assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::AwaitingInput));
        trace.record(answer(ALPHA, "second"));
        assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::AwaitingInput));
        trace.record(answer(ALPHA, "first"));
        assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::Pending));
        assert!(trace.fold.ready(ALPHA), "one durable wake is sufficient");
        trace.replay();
    }
}

#[test]
fn declining_a_repair_admission_fails_the_lineage_and_allows_the_run_to_end() {
    let repair = TaskKey(3);
    let mut trace = Trace::started();
    trace.queue(ALPHA);
    trace.record(reject_into_question(ALPHA, repair, "repair-admission"));
    trace.record(answered(
        repair,
        "repair-admission",
        Answer4::Declined {
            decline_halts_run: false,
        },
    ));

    assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::Failed));
    assert_eq!(trace.fold.task_state(repair), Some(TaskState::Failed));
    assert!(trace.fold.queue().expect("started").is_empty());
    assert!(
        !trace
            .fold
            .leases()
            .expect("started")
            .any_candidate_or_lineage()
    );
    assert_eq!(trace.fold.transaction(), None);
    assert!(!trace.fold.questions_open());
    assert_eq!(
        trace.fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete)
    );
    trace.replay();
}

fn runnable_repair(key: TaskKey) -> FrozenSpawn {
    let mut spawn = repair_spawn(key, ALPHA, ALPHA);
    spawn.entry.display_id = TaskId::from(format!("repair-{}", key.0).as_str());
    spawn.entry.deps.clear();
    spawn.entry.display_deps.clear();
    spawn
}

fn start_repair(trace: &mut Trace, key: TaskKey) {
    trace.record(ev(TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key,
            generation: GenerationId(0),
            base_sha: sha("base"),
            worktree_path: format!("/private/workspaces/tasks/k{}-g0", key.0),
            lease: LeaseGrant::InheritedLineage { root: ALPHA },
            source_candidate: Some(candidate_of(ALPHA, 0)),
        },
    }));
    let mut start = attempt_started(&trace.fold, key, 0, 1, 0);
    if let TopologyEventBody::AttemptStarted { data } = &mut start.body {
        data.materialization_observed = Some(Materialization::Conflict);
    }
    trace.record(start);
}

fn sibling_attempts() -> Trace {
    let mut trace = Trace::started();
    trace.queue(ALPHA);
    let mut rejection = reject_into_question(ALPHA, TaskKey(3), "unused");
    if let TopologyEventBody::MergeRejected { data } = &mut rejection.body {
        data.repair.admission = SpawnAdmission::Runnable;
    }
    trace.record(rejection);
    trace.record(spawn_event(runnable_repair(TaskKey(4))));
    start_repair(&mut trace, TaskKey(3));
    start_repair(&mut trace, TaskKey(4));
    trace
}

fn queue_repair(trace: &mut Trace, key: TaskKey) {
    let mut prepared = candidate_prepared(key, 0, &sha("base"));
    if let TopologyEventBody::CandidatePrepared { data } = &mut prepared.body {
        data.lease_effect = CandidateLeaseEffect::WidensLineage {
            root: ALPHA,
            paths: region(key),
        };
    }
    trace.record(prepared);
    trace.record(candidate_created(key, 0));
}

fn park_sibling() -> TopologyEvent {
    park_repair(TaskKey(4), "sibling")
}

fn park_repair(key: TaskKey, id: &str) -> TopologyEvent {
    settle_failing(
        key,
        0,
        1,
        crate::ladder::FailureKind::NeedsHuman,
        AttemptSettlement::Closed {
            transition: SettlementTransition::Parked {
                question: question(id, key),
            },
            lease: LeaseDisposition::LineageHeld,
        },
    )
}

fn decline_sibling() -> TopologyEvent {
    answered(
        TaskKey(4),
        "sibling",
        Answer4::Declined {
            decline_halts_run: false,
        },
    )
}

fn verified_repair_publication() -> TopologyEvent {
    let mut event = fast_publication(TaskKey(3), 0, 1, &sha("base"), vec![ALPHA, TaskKey(3)]);
    if let TopologyEventBody::MergePrepared { data } = &mut event.body {
        data.disposition = PreparedDisposition::StaleClean;
        data.expected_head = sha("head");
        data.proposed_sha = sha("proposal");
        data.prepared_ref = Some(git_ref("prepared/1"));
        data.verification_source = VerificationSource::Verification {
            sequence: SequenceId(1),
        };
        data.verification = Some(verification_record(Verdict::Passed));
    }
    event
}

#[test]
fn an_embedded_question_prevents_a_siblings_existing_verification_from_authorizing_publication() {
    let mut trace = sibling_attempts();
    queue_repair(&mut trace, TaskKey(3));
    trace.record(verification_started(
        TaskKey(3),
        0,
        1,
        &sha("head"),
        &sha("proposal"),
    ));
    trace.record(park_sibling());
    refuse(&trace.fold, &verified_repair_publication());
    trace.record(answer(TaskKey(4), "sibling"));
    accepts(&trace.fold, &verified_repair_publication());
    trace.replay();
}

#[test]
fn declining_an_embedded_question_cancels_its_lineages_unprepared_verification() {
    let mut trace = sibling_attempts();
    queue_repair(&mut trace, TaskKey(3));
    trace.record(verification_started(
        TaskKey(3),
        0,
        1,
        &sha("head"),
        &sha("proposal"),
    ));
    trace.record(park_sibling());
    trace.record(decline_sibling());

    for key in [ALPHA, TaskKey(3), TaskKey(4)] {
        assert_eq!(trace.fold.task_state(key), Some(TaskState::Failed));
    }
    assert_eq!(trace.fold.transaction(), None);
    assert_eq!(trace.fold.pipeline_held(), 0);
    assert!(trace.fold.queue().expect("started").is_empty());
    assert!(
        !trace
            .fold
            .leases()
            .expect("started")
            .any_candidate_or_lineage()
    );
    refuse(&trace.fold, &verified_repair_publication());
    assert_eq!(
        trace.fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete)
    );
    trace.replay();
}

#[test]
fn declining_an_embedded_question_closes_a_running_sibling_and_refuses_its_late_result() {
    let mut trace = sibling_attempts();
    trace.record(park_sibling());
    let mut completion = candidate_prepared(TaskKey(3), 0, &sha("base"));
    if let TopologyEventBody::CandidatePrepared { data } = &mut completion.body {
        data.lease_effect = CandidateLeaseEffect::WidensLineage {
            root: ALPHA,
            paths: region(TaskKey(3)),
        };
    }
    accepts(&trace.fold, &completion);
    trace.record(decline_sibling());

    assert_eq!(trace.fold.pipeline_held(), 0);
    assert!(
        trace
            .fold
            .task(TaskKey(3))
            .expect("repair")
            .generations
            .iter()
            .all(|generation| { generation.class == GenerationClass::Closed })
    );
    refuse(&trace.fold, &completion);
    assert_eq!(
        trace.fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete)
    );
    trace.replay();
}

#[test]
fn a_decline_cannot_cancel_a_prepared_publication_and_preserves_its_merged_ancestors() {
    let mut trace = sibling_attempts();
    queue_repair(&mut trace, TaskKey(3));
    trace.record(fast_publication(
        TaskKey(3),
        0,
        1,
        &sha("base"),
        vec![ALPHA, TaskKey(3)],
    ));
    trace.record(park_sibling());
    refuse(&trace.fold, &decline_sibling());
    assert!(matches!(
        trace
            .fold
            .transaction()
            .map(|transaction| &transaction.class),
        Some(TransactionClass::Prepared { .. })
    ));

    let mut publication = merged(TaskKey(3), 0, 1, vec![ALPHA, TaskKey(3)]);
    if let TopologyEventBody::TaskMerged { data } = &mut publication.body {
        data.lease_release = MergeLeaseRelease::Lineage { root: ALPHA };
    }
    trace.record(publication);
    trace.record(decline_sibling());
    assert_eq!(trace.fold.task_state(ALPHA), Some(TaskState::Merged));
    assert_eq!(trace.fold.task_state(TaskKey(3)), Some(TaskState::Merged));
    assert_eq!(trace.fold.task_state(TaskKey(4)), Some(TaskState::Failed));
    assert!(
        trace.fold.ready(ZETA),
        "already merged dependencies stay satisfied"
    );
    trace.replay();
}

#[test]
fn a_lineage_question_blocks_related_work_and_decline_preserves_an_unrelated_transaction() {
    let mut trace = Trace::started();
    trace.queue(ALPHA);
    trace.record(reject_into_question(ALPHA, TaskKey(3), "admission"));
    trace.record(spawn_event(runnable_repair(TaskKey(4))));
    assert!(
        !trace.fold.ready(TaskKey(4)),
        "the question affects this lineage"
    );
    trace.record(raised("root", ALPHA));
    trace.record(answer(ALPHA, "root"));
    assert_eq!(
        trace.fold.task_state(ALPHA),
        Some(TaskState::AwaitingRepair)
    );

    trace.queue(MID);
    trace.record(fast_publication(MID, 0, 1, &sha("base"), vec![MID]));
    trace.record(answered(
        TaskKey(3),
        "admission",
        Answer4::Declined {
            decline_halts_run: false,
        },
    ));
    assert_eq!(
        trace
            .fold
            .transaction()
            .expect("unrelated publication survives")
            .candidate
            .key,
        MID
    );
    accepts(&trace.fold, &merged(MID, 0, 1, vec![MID]));
    refuse(&trace.fold, &spawn_event(runnable_repair(TaskKey(5))));
    trace.replay();
}

#[test]
fn a_new_bare_or_standalone_admission_question_cannot_enter_an_active_lineage_transaction() {
    let mut trace = sibling_attempts();
    queue_repair(&mut trace, TaskKey(3));
    // Settle the sibling without a question so transaction ownership is the
    // only reason the later questions must be refused.
    trace.record(settle(
        TaskKey(4),
        0,
        1,
        AttemptSettlement::Closed {
            transition: SettlementTransition::Retry,
            lease: LeaseDisposition::LineageHeld,
        },
    ));
    trace.record(fast_publication(
        TaskKey(3),
        0,
        1,
        &sha("base"),
        vec![ALPHA, TaskKey(3)],
    ));
    for key in [ALPHA, TaskKey(3), TaskKey(4)] {
        refuse(&trace.fold, &raised("transaction", key));
    }
    let mut spawn = runnable_repair(TaskKey(5));
    spawn.admission = SpawnAdmission::HumanRequired {
        limit: run_started().limits.max_merge_repairs,
        question: question("new-admission", TaskKey(5)),
    };
    refuse(&trace.fold, &spawn_event(spawn));
    assert!(trace.fold.transaction().is_some());
    trace.replay();
}

#[test]
fn engine_selection_skips_a_candidate_whose_lineage_has_a_question() {
    use crate::engine::topology::select::{Ceiling, Spend, Step, select};

    let mut trace = Trace::wide_started();
    trace.queue(ALPHA);
    trace.record(reject_into_question(ALPHA, TaskKey(4), "admission"));
    trace.record(answer(TaskKey(4), "admission"));
    start_repair(&mut trace, TaskKey(4));
    queue_repair(&mut trace, TaskKey(4));
    trace.record(raised("parent", ALPHA));
    assert!(
        trace.fold.ready(MID),
        "the unrelated frontier remains runnable"
    );
    trace.queue(MID);

    let Step::Integrate { candidate } = select(&trace.fold, &Ceiling::unlimited(), &Spend::new())
    else {
        panic!("the unrelated candidate must remain eligible for integration");
    };
    assert_eq!(
        candidate.key, MID,
        "selection must agree with the fold's lineage question check"
    );
    refuse(
        &trace.fold,
        &fast_publication(TaskKey(4), 0, 1, &sha("base"), vec![ALPHA, TaskKey(4)]),
    );
    accepts(
        &trace.fold,
        &fast_publication(MID, 0, 1, &sha("base"), vec![MID]),
    );
    trace.replay();
}

#[test]
fn an_embedded_question_blocks_a_retained_siblings_retry_until_answered_or_declined() {
    for decline in [false, true] {
        let mut trace = sibling_attempts();
        trace.record(retain(TaskKey(3), 1, "sess-ÜNI-0007", Epoch(0)));
        assert!(trace.fold.ready_retry(TaskKey(3)));
        trace.record(park_sibling());
        assert!(!trace.fold.ready_retry(TaskKey(3)));
        let mut retry = attempt_started_resuming(&trace.fold, TaskKey(3), 0, 2, 0, "sess-ÜNI-0007");
        if let TopologyEventBody::AttemptStarted { data } = &mut retry.body {
            data.materialization_observed = Some(Materialization::Conflict);
        }
        refuse(&trace.fold, &retry);
        trace.record(if decline {
            decline_sibling()
        } else {
            answer(TaskKey(4), "sibling")
        });
        if decline {
            assert!(
                trace
                    .fold
                    .task(TaskKey(3))
                    .expect("repair")
                    .open()
                    .is_none()
            );
            refuse(&trace.fold, &retry);
            assert_eq!(
                trace.fold.derived_outcome(),
                DerivedOutcome::Ending(RunOutcome::Complete)
            );
        } else {
            assert!(trace.fold.ready_retry(TaskKey(3)));
            accepts(&trace.fold, &retry);
        }
        trace.replay();
    }
}

#[test]
fn declining_one_of_two_embedded_questions_closes_the_whole_lineages_questions() {
    let mut trace = sibling_attempts();
    trace.record(park_repair(TaskKey(3), "first-sibling"));
    trace.record(park_sibling());
    trace.record(decline_sibling());
    assert!(!trace.fold.questions_open());
    refuse(&trace.fold, &answer(TaskKey(3), "first-sibling"));
    assert_eq!(
        trace.fold.derived_outcome(),
        DerivedOutcome::Ending(RunOutcome::Complete)
    );
    trace.replay();
}

#[test]
fn declining_a_lineage_preserves_unrelated_verification_and_its_candidate() {
    let mut trace = Trace::started();
    trace.queue(ALPHA);
    trace.record(reject_into_question(ALPHA, TaskKey(3), "admission"));
    trace.queue(MID);
    trace.record(verification_started(
        MID,
        0,
        1,
        &sha("head"),
        &sha("proposal"),
    ));
    trace.record(answered(
        TaskKey(3),
        "admission",
        Answer4::Declined {
            decline_halts_run: false,
        },
    ));
    assert_eq!(
        trace
            .fold
            .transaction()
            .expect("unrelated verification survives")
            .candidate
            .key,
        MID
    );
    assert!(trace.fold.queue().expect("started").holds_task(MID));
    assert!(
        trace
            .fold
            .leases()
            .expect("started")
            .holds(LeaseOwner::Candidate {
                key: MID,
                generation: GenerationId(0)
            })
    );
    let mut prepared = verified_repair_publication();
    if let TopologyEventBody::MergePrepared { data } = &mut prepared.body {
        data.key = MID;
        let candidate = candidate_of(MID, 0);
        data.candidate_sha = candidate.commit_sha;
        data.candidate_ref = candidate.candidate_ref;
        data.satisfies = vec![MID];
    }
    accepts(&trace.fold, &prepared);
    trace.replay();
}

#[test]
fn bare_questions_and_active_generations_exclude_each_other_across_a_lineage() {
    for queried in [ALPHA, TaskKey(3), TaskKey(4)] {
        let mut trace = Trace::started();
        trace.queue(ALPHA);
        trace.record(reject_into_question(ALPHA, TaskKey(3), "admission"));
        trace.record(answer(TaskKey(3), "admission"));
        trace.record(spawn_event(runnable_repair(TaskKey(4))));
        // Question first: the affected lineage cannot acquire a generation.
        trace.record(raised("quiet", queried));
        assert!(!trace.fold.ready(TaskKey(3)));
        let mut dispatched = dispatch(TaskKey(3), 0, &sha("base"));
        if let TopologyEventBody::TaskDispatched { data } = &mut dispatched.body {
            data.lease = LeaseGrant::InheritedLineage { root: ALPHA };
            data.source_candidate = Some(candidate_of(ALPHA, 0));
        }
        refuse(&trace.fold, &dispatched);
        trace.record(answer(queried, "quiet"));
        accepts(&trace.fold, &dispatched);
        // Generation first: bare or standalone questions cannot enter it.
        trace.record(dispatched);
        refuse(&trace.fold, &raised("reserved", queried));
        let mut spawn = runnable_repair(TaskKey(5));
        spawn.admission = SpawnAdmission::HumanRequired {
            limit: run_started().limits.max_merge_repairs,
            question: question("admission-active", TaskKey(5)),
        };
        refuse(&trace.fold, &spawn_event(spawn));
        let mut start = attempt_started(&trace.fold, TaskKey(3), 0, 1, 0);
        if let TopologyEventBody::AttemptStarted { data } = &mut start.body {
            data.materialization_observed = Some(Materialization::Conflict);
        }
        trace.record(start);
        refuse(&trace.fold, &raised("running", queried));
        trace.record(retain(TaskKey(3), 1, "sess-ÜNI-0007", Epoch(0)));
        refuse(&trace.fold, &raised("retained", queried));
        trace.replay();
    }
}

#[test]
fn terminal_tasks_refuse_new_questions_without_resurrecting_work() {
    let mut merged_trace = Trace::started();
    merged_trace.queue(ALPHA);
    merged_trace.record(fast_publication(ALPHA, 0, 0, &sha("base"), vec![ALPHA]));
    merged_trace.record(merged(ALPHA, 0, 0, vec![ALPHA]));
    refuse(&merged_trace.fold, &raised("after-merge", ALPHA));
    assert_eq!(merged_trace.fold.task_state(ALPHA), Some(TaskState::Merged));
    merged_trace.replay();

    let mut declined_trace = Trace::started();
    declined_trace.record(raised("pending", ALPHA));
    declined_trace.record(answered(
        ALPHA,
        "pending",
        Answer4::Declined {
            decline_halts_run: false,
        },
    ));
    refuse(&declined_trace.fold, &raised("after-decline", ALPHA));
    assert_eq!(
        declined_trace.fold.task_state(ALPHA),
        Some(TaskState::Failed)
    );
    declined_trace.replay();
}
