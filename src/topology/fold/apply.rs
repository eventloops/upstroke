//! The application half of INV-02: what a checked transition does to the
//! state, and nothing that decides whether it may.

use super::*;

// ---------------------------------------------------------------------------
// RunState: the application
// ---------------------------------------------------------------------------

impl RunState {
    /// Apply a transition the check accepted.
    ///
    /// Total by construction: every lookup here was proved to succeed by the
    /// check that produced the delta, and each one is written so that a miss
    /// leaves the state alone rather than panicking. Nothing in this function
    /// decides anything — a decision made here would be a decision the live
    /// path and the replay path could reach differently, which is the one thing
    /// INV-02 forbids.
    #[allow(clippy::too_many_lines)]
    pub(super) fn apply(&mut self, body: &TopologyEventBody, derived: &Derived) {
        match body {
            TopologyEventBody::RunStarted { .. } => {}
            TopologyEventBody::RunResumed { data } => self.apply_resumed(data),
            TopologyEventBody::TaskSpawned { data } => self.register(&data.spawn),
            TopologyEventBody::TaskDispatched { data } => self.apply_dispatched(data),
            TopologyEventBody::AttemptStarted { data } => {
                if let Some(generation) = self.open_generation_mut(data.key) {
                    generation.class = GenerationClass::InFlight {
                        attempt: data.attempt,
                    };
                    generation.attempts = data.attempt.0;
                }
                // **Not counted here.** An attempt that has *started* has not
                // yet spent anything: `ladder::spends_allowance` is total over
                // `FailureKind` and its line is "the worker ran and produced
                // work to judge", which `attempt_started` cannot know. Counting
                // here made this fold a second authority for a rule that has one
                // production implementation, and made every interruption, park
                // and outage burn a rung the packet says they do not —
                // `transaction_fault_matrix[T-ATTEMPT]`'s "unknown spend,
                // **allowance refunded**". The count is taken at the settlement,
                // in `apply_settlement`, from the record the settlement carries.
            }
            TopologyEventBody::AttemptFinished { data } => self.apply_settlement(data),
            TopologyEventBody::AttemptInterrupted { data } => {
                // T-ATTEMPT: generation Closed, task Pending, later dispatch a
                // new generation. The close releases the ordinary generation's
                // own region exactly as every other closing settlement does.
                self.close_generation(data.key);
                self.set_state(data.key, TaskState::Pending);
            }
            TopologyEventBody::GenerationClosed { data } => {
                self.close_generation(data.key);
            }
            TopologyEventBody::DeferWaitElapsed { .. } => self.wake_backoff(),
            TopologyEventBody::CandidatePrepared { data } => self.apply_candidate_prepared(data),
            TopologyEventBody::TaskCandidateCreated { data } => {
                self.apply_candidate_created(&data.candidate);
            }
            TopologyEventBody::MergeVerificationStarted { data } => {
                self.apply_verification_started(data);
            }
            TopologyEventBody::MergeVerificationUnavailable { data } => {
                self.apply_verification_unavailable(data);
            }
            TopologyEventBody::MergeVerificationInterrupted { .. } => {
                self.release_transaction();
            }
            TopologyEventBody::MergePrepared { data } => self.apply_merge_prepared(data),
            TopologyEventBody::MergeRejected { data } => self.apply_merge_rejected(data),
            TopologyEventBody::TaskMerged { data } => self.apply_task_merged(data),
            TopologyEventBody::QuestionRaised { data } => {
                // A bare `question_raised` carries no admission and so
                // authorizes no binding.
                self.open_question(&data.question, QuestionOrigin::Admission, None);
                self.set_state(data.question.key, TaskState::AwaitingInput);
            }
            TopologyEventBody::QuestionAnswered { data } => self.apply_answer(data, derived),
            TopologyEventBody::BudgetExceeded { data } => {
                if !self.budget_stop_is_current() {
                    self.budget_stop = Some(data.stop());
                }
            }
            TopologyEventBody::RunFinished { data } => {
                self.finished = Some(data.outcome.clone());
            }
            TopologyEventBody::CapacitySnapshot { .. }
            | TopologyEventBody::PoolExhausted { .. }
            | TopologyEventBody::DesignDefect { .. } => {}
        }
    }

    pub(super) fn apply_resumed(&mut self, resumed: &RunResumed4) {
        self.epoch = Epoch(self.epoch.0.saturating_add(1));
        self.incarnation = resumed.incarnation.clone();
        // The stop belongs to the epoch that hit the old ceiling; the next
        // epoch starts without one, which is what makes "raise the budget and
        // resume" the response to it.
        self.budget_stop = None;
        self.finished = None;
        // Deferred items are woken by a resume exactly as they are by an
        // elapsed wait.
        self.wake_backoff();
    }

    pub(super) fn wake_backoff(&mut self) {
        self.queue.wake_deferred();
        for task in &mut self.tasks {
            if task.state == TaskState::Deferred {
                task.state = TaskState::Pending;
            }
        }
    }

    pub(super) fn register(&mut self, spawn: &FrozenSpawn) {
        self.registry.register(spawn.entry.clone());
        self.tasks.push(TaskFold::new());
        match &spawn.admission {
            SpawnAdmission::Runnable => {}
            SpawnAdmission::HumanRequired { question, .. } => {
                self.open_question(question, QuestionOrigin::Admission, None);
                self.set_state(spawn.key, TaskState::AwaitingInput);
            }
            SpawnAdmission::HumanBinding { options, question } => {
                // The one admission that authorizes an override, and the one
                // place its option list is frozen.
                self.open_question(question, QuestionOrigin::Admission, Some(options.clone()));
                self.set_state(spawn.key, TaskState::AwaitingInput);
            }
        }
    }

    pub(super) fn apply_dispatched(&mut self, dispatched: &TaskDispatched) {
        // The recorded region and `predicted_region(entry)` are one value by the
        // time this runs: `check_dispatched` refuses an ordinary dispatch whose
        // `Predicted { paths }` is anything else. Granting the event's copy is
        // therefore granting the derivation, and it stays the event's copy so
        // that the region in the lease table is demonstrably the region the log
        // holds rather than a second derivation of it.
        let (lease, region) = match &dispatched.lease {
            LeaseGrant::Predicted { paths } => (GenerationLease::Own, Some(paths.clone())),
            LeaseGrant::InheritedLineage { root } => {
                (GenerationLease::InheritedLineage { root: *root }, None)
            }
        };
        if let Some(paths) = region {
            self.leases.grant(
                LeaseOwner::Generation {
                    key: dispatched.key,
                    generation: dispatched.generation,
                },
                paths,
            );
        }
        if let Some(task) = self.tasks.get_mut(dispatched.key.index()) {
            task.generations.push(GenerationFold {
                id: dispatched.generation,
                class: GenerationClass::OpenNoAttempt,
                base_sha: dispatched.base_sha.clone(),
                lease,
                attempts: 0,
                candidate: None,
            });
        }
    }

    pub(super) fn apply_settlement(&mut self, finished: &AttemptFinished4) {
        // **The allowance, decided once, by the one function that decides it.**
        //
        // `ladder::spends_allowance` is documented as "the single production
        // implementation of the allowance rule" and is total over `FailureKind`
        // so a new variant stops the build rather than taking a default. This
        // fold consumes it; it does not re-derive it. `FailureRecord::shape`
        // exists for exactly this call — "a settlement holds a record rather
        // than the live failure, and the allowance decision is the same decision
        // either way".
        //
        // **Taken at the settlement, which is what makes the refund free.**
        // T-ATTEMPT refunds an interrupted attempt's allowance. An attempt that
        // never settled never counted, so there is nothing to give back and no
        // second rule to keep in step with the first — the refund is the absence
        // of a charge rather than a subtraction that could be forgotten.
        //
        // Before the `Escalated` arm below, which resets the count: an attempt
        // that escalates spent its allowance on the rung it is leaving, and the
        // rung it climbs onto starts again at zero.
        //
        // Nested rather than a `let`-chain: `if cond && let Some(x) = ..` is
        // unstable on **1.85**, which this crate's MSRV pins, and stable rustc
        // accepts it — so the local gates pass and only the MSRV leg refuses.
        self.charge_allowance(finished.key, &finished.record);

        match &finished.settlement {
            AttemptSettlement::Retained {
                retained_session,
                retained_incarnation,
            } => {
                if let Some(generation) = self.open_generation_mut(finished.key) {
                    generation.class = GenerationClass::RetainedIdle {
                        session: retained_session.clone(),
                        incarnation: *retained_incarnation,
                    };
                }
            }
            AttemptSettlement::Closed { transition, .. } => match transition {
                // Unreachable: `check_attempt_finished` refuses this
                // transition before `apply` is called, because
                // `candidate_prepared` is the sole successful settlement. The
                // arm stays so the match is total over the wire vocabulary —
                // the variant is still a legal *shape*, it is simply not a
                // settlement this fold accepts — and it does nothing, so a
                // check that stopped refusing would produce a generation stuck
                // in flight rather than a silently-promoted one.
                SettlementTransition::Succeeded => {}
                SettlementTransition::Retry => {
                    self.close_generation(finished.key);
                }
                SettlementTransition::Escalated { rung } => {
                    self.close_generation(finished.key);
                    // The settlement's own number: the packet defines it as the
                    // rung the escalation climbs *onto*. The allowance is per
                    // rung, so it starts again here.
                    if let Some(task) = self.tasks.get_mut(finished.key.index()) {
                        task.rung = *rung;
                        task.attempts_on_rung = 0;
                    }
                }
                SettlementTransition::Deferred { defers, .. } => {
                    self.close_generation(finished.key);
                    self.set_state(finished.key, TaskState::Deferred);
                    // The settlement's own number, not this fold's plus one.
                    // `settle_failed` computed it as `defers.saturating_add(1)`
                    // and appended it; recomputing here would be a second
                    // derivation of a value the log already holds, and a replay
                    // of the same log would then disagree with the process that
                    // wrote it.
                    self.set_defers(finished.key, *defers);
                }
                SettlementTransition::Parked { question } => {
                    self.close_generation(finished.key);
                    self.open_question(question, QuestionOrigin::Admission, None);
                    self.set_state(finished.key, TaskState::AwaitingInput);
                }
                SettlementTransition::Failed { halts_run, .. } => {
                    self.close_generation(finished.key);
                    self.set_state(finished.key, TaskState::Failed);
                    if *halts_run {
                        self.record_halt(finished.key);
                    }
                }
            },
        }
    }

    /// `halted_at` is first in wins, and is never cleared.
    pub(super) fn record_halt(&mut self, key: TaskKey) {
        if self.halted_at.is_none() {
            self.halted_at = Some(key);
            self.halted_epoch = Some(self.epoch);
        }
    }

    /// One settled attempt against its rung's allowance.
    ///
    /// **The single write, and both settlements reach it through here.** The
    /// increment used to live inline in [`Self::apply_settlement`], which was
    /// fine while `attempt_finished` was the only settlement — and stopped being
    /// fine on 2026-08-27, when `candidate_prepared` became the sole successful
    /// one. The settlement moved and the counting did not, so **a successful
    /// attempt stopped spending anything**: a first-attempt success left
    /// `attempts_on_rung` at zero, replay reproduced the undercount, and a later
    /// allowance reader could grant an extra attempt on a rung already paid for.
    /// The round-4 review of `09f9a99` found it, and the Class B approval this
    /// change was made under says the thing that did not happen — *"settlement
    /// counting moves to the sole event"*.
    ///
    /// A shared core rather than a second increment, because two increments are
    /// two rules: `the_rungs_allowance_is_counted_in_one_production_place` exists
    /// to forbid exactly that, and it counts **calls to this** so a settlement
    /// that stops charging is a failing census rather than a silent undercount.
    ///
    /// It consults `spends_allowance` and answers nothing itself. A successful
    /// record carries no failure, and `spends_allowance(None)` is `true`: the
    /// worker ran and produced work that was judged and accepted.
    pub(super) fn charge_allowance(&mut self, key: TaskKey, record: &crate::events::AttemptRecord) {
        if crate::ladder::spends_allowance(
            record
                .failure
                .as_ref()
                .map(crate::events::FailureRecord::shape),
        ) {
            if let Some(task) = self.tasks.get_mut(key.index()) {
                task.attempts_on_rung = task.attempts_on_rung.saturating_add(1);
            }
        }
    }

    pub(super) fn apply_candidate_prepared(&mut self, prepared: &CandidatePrepared) {
        let record = PreparedCandidate {
            candidate: prepared.candidate(),
            base_sha: prepared.base_sha.clone(),
            tree_sha: prepared.tree_sha.clone(),
            paths: prepared.actual_paths.clone(),
        };
        if let Some(generation) = self.open_generation_mut(prepared.key) {
            generation.candidate = Some(record);
            // **The settlement, which used to arrive on its own event.** A
            // candidate-producing attempt has exactly one successful
            // settlement and this is it, so the class transition belongs here
            // rather than to an `attempt_finished` the 2026-08-12 record says
            // is not emitted.
            generation.class = GenerationClass::Promoting;
        }
        // **The settlement's accounting, which moved with the settlement.**
        // Same core as the failure path, so there is one increment in this
        // build and both settlements reach it.
        self.charge_allowance(prepared.key, &prepared.attempt);
        match &prepared.lease_effect {
            CandidateLeaseEffect::ReplacesPredicted { paths } => {
                self.leases.release(LeaseOwner::Generation {
                    key: prepared.key,
                    generation: prepared.generation,
                });
                self.leases.grant(
                    LeaseOwner::Candidate {
                        key: prepared.key,
                        generation: prepared.generation,
                    },
                    paths.clone(),
                );
            }
            CandidateLeaseEffect::WidensLineage { root, paths } => {
                self.leases.widen_lineage(*root, paths);
            }
        }
        self.set_state(prepared.key, TaskState::AwaitingMerge);
    }

    pub(super) fn apply_candidate_created(&mut self, candidate: &CandidateRef) {
        let paths = self
            .tasks
            .get(candidate.key.index())
            .and_then(TaskFold::open)
            .and_then(|generation| generation.candidate.as_ref())
            .map_or(PathSet::RepoWide, |prepared| prepared.paths.clone());
        let lineage_root = self
            .registry
            .get(candidate.key)
            .and_then(|entry| entry.lineage)
            .map(|lineage| lineage.root);
        self.close_generation(candidate.key);
        self.queue.push(QueueEntry {
            candidate: candidate.clone(),
            paths,
            lineage_root,
            verification_deferred: false,
            defers: 0,
            sequence: None,
        });
    }

    pub(super) fn apply_verification_started(&mut self, started: &MergeVerificationStarted) {
        self.next_sequence = self.next_sequence.saturating_add(1);
        if let Some(entry) = self
            .queue
            .get_mut(started.candidate.key, started.candidate.generation)
        {
            entry.sequence = Some(started.sequence);
        }
        self.transaction = Some(Transaction {
            sequence: started.sequence,
            candidate: started.candidate.clone(),
            class: TransactionClass::VerificationStarted {
                basis: started.basis.clone(),
                expected_head: started.expected_head.clone(),
                proposed_sha: started.proposed_sha.clone(),
            },
        });
    }

    pub(super) fn apply_verification_unavailable(
        &mut self,
        unavailable: &MergeVerificationUnavailable,
    ) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        let candidate = transaction.candidate;
        if let Some(entry) = self.queue.get_mut(candidate.key, candidate.generation) {
            entry.sequence = None;
            if let UnavailableOutcome::Deferred { defers } = &unavailable.outcome {
                entry.verification_deferred = true;
                entry.defers = *defers;
            }
        }
        if let UnavailableOutcome::Parked { question } = &unavailable.outcome {
            self.open_question(question, QuestionOrigin::VerificationPark, None);
            self.set_state(candidate.key, TaskState::AwaitingInput);
        }
    }

    pub(super) fn release_transaction(&mut self) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        if let Some(entry) = self
            .queue
            .get_mut(transaction.candidate.key, transaction.candidate.generation)
        {
            entry.sequence = None;
        }
    }

    pub(super) fn apply_merge_prepared(&mut self, prepared: &MergePrepared) {
        if prepared.disposition == PreparedDisposition::Fast {
            self.next_sequence = self.next_sequence.saturating_add(1);
        }
        self.transaction = Some(Transaction {
            sequence: prepared.sequence,
            candidate: prepared.candidate(),
            class: TransactionClass::Prepared {
                proposed_sha: prepared.proposed_sha.clone(),
                satisfies: prepared.satisfies.clone(),
            },
        });
    }

    pub(super) fn apply_merge_rejected(&mut self, rejected: &MergeRejected) {
        if matches!(rejected.disposition, RejectionDisposition::Conflict { .. }) {
            self.next_sequence = self.next_sequence.saturating_add(1);
        }
        self.transaction = None;
        let candidate = &rejected.candidate;
        self.queue.remove(candidate.key, candidate.generation);
        match &rejected.lease_effect {
            RejectionLeaseEffect::CreatesLineage { root, paths } => {
                // The rejected candidate's own holding becomes the lineage's,
                // widened by the region the conflict named.
                let held = self
                    .tasks
                    .get(candidate.key.index())
                    .and_then(|task| {
                        task.generations
                            .iter()
                            .find(|generation| generation.id == candidate.generation)
                    })
                    .and_then(|generation| generation.candidate.as_ref())
                    .map(|prepared| prepared.paths.clone());
                if let Some(held) = held {
                    self.leases.widen_lineage(*root, &held);
                }
                self.leases.widen_lineage(*root, paths);
                self.leases.release(LeaseOwner::Candidate {
                    key: candidate.key,
                    generation: candidate.generation,
                });
            }
            RejectionLeaseEffect::WidensLineage { root, paths } => {
                self.leases.widen_lineage(*root, paths);
            }
        }
        self.set_state(candidate.key, TaskState::AwaitingRepair);
        self.register(&rejected.repair);
    }

    pub(super) fn apply_task_merged(&mut self, merged: &TaskMerged) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        let candidate = transaction.candidate;
        self.queue.remove(candidate.key, candidate.generation);
        for key in &merged.satisfies {
            self.set_state(*key, TaskState::Merged);
        }
        match &merged.lease_release {
            MergeLeaseRelease::Candidate { key, generation } => {
                self.leases.release(LeaseOwner::Candidate {
                    key: *key,
                    generation: *generation,
                });
            }
            MergeLeaseRelease::Lineage { root } => {
                self.leases.release(LeaseOwner::Lineage { root: *root });
            }
        }
    }

    pub(super) fn apply_answer(&mut self, answered: &QuestionAnswered4, derived: &Derived) {
        self.questions.remove(&answered.question);
        match &answered.answer {
            Answer4::Answered {
                binding_override, ..
            } => {
                if let Some(binding) = binding_override {
                    self.overrides.insert(answered.key, binding.clone());
                }
                let state = match derived {
                    Derived::Answer(QuestionOrigin::VerificationPark) => TaskState::AwaitingMerge,
                    _ => TaskState::Pending,
                };
                self.set_state(answered.key, state);
            }
            Answer4::Declined { decline_halts_run } => {
                self.set_state(answered.key, TaskState::Failed);
                self.release_holdings_of(answered.key);
                if *decline_halts_run {
                    self.record_halt(answered.key);
                }
            }
        }
    }

    /// A declined question consumes the task's queue position and releases what
    /// it held: its candidate lease, or the lineage lease when the task belongs
    /// to a lineage — a declined lineage fails as a whole.
    pub(super) fn release_holdings_of(&mut self, key: TaskKey) {
        let generations: Vec<GenerationId> = self
            .tasks
            .get(key.index())
            .map(|task| {
                task.generations
                    .iter()
                    .map(|generation| generation.id)
                    .collect()
            })
            .unwrap_or_default();
        for generation in generations {
            self.queue.remove(key, generation);
            self.leases
                .release(LeaseOwner::Candidate { key, generation });
        }
        let root = self
            .registry
            .get(key)
            .and_then(|entry| entry.lineage)
            .map(|lineage| lineage.root);
        if let Some(root) = root {
            self.leases.release(LeaseOwner::Lineage { root });
        } else if self.leases.holds(LeaseOwner::Lineage { root: key }) {
            self.leases.release(LeaseOwner::Lineage { root: key });
        }
    }

    /// Open a question, carrying the binding authority it was asked under.
    ///
    /// `binding` is `Some` for a `HumanBinding` admission and `None` for every
    /// other question this run can ask — a `HumanRequired` admission, a parked
    /// settlement, a verification park, a bare `question_raised`. That is the
    /// whole of what an override may be validated against.
    pub(super) fn open_question(
        &mut self,
        question: &FrozenQuestion,
        origin: QuestionOrigin,
        binding: Option<Vec<String>>,
    ) {
        self.seen_questions.insert(question.id.clone());
        self.questions.insert(
            question.id.clone(),
            OpenQuestion {
                question: question.clone(),
                origin,
                binding,
            },
        );
    }

    pub(super) fn set_state(&mut self, key: TaskKey, state: TaskState) {
        if let Some(task) = self.tasks.get_mut(key.index()) {
            task.state = state;
        }
    }

    /// Record the deferral count a `Deferred` settlement carried.
    ///
    /// Assignment rather than increment: the number is the settlement's, which
    /// is what makes a replay of the same log reach the same count as the
    /// process that wrote it.
    pub(super) fn set_defers(&mut self, key: TaskKey, defers: u32) {
        if let Some(task) = self.tasks.get_mut(key.index()) {
            task.defers = defers;
        }
    }

    pub(super) fn open_generation_mut(&mut self, key: TaskKey) -> Option<&mut GenerationFold> {
        self.tasks.get_mut(key.index())?.open_mut()
    }

    /// Close the open generation, releasing the region it held on its own.
    pub(super) fn close_generation(&mut self, key: TaskKey) {
        let Some(generation) = self.open_generation_mut(key) else {
            return;
        };
        let id = generation.id;
        let own = generation.lease == GenerationLease::Own;
        generation.class = GenerationClass::Closed;
        if own {
            self.leases.release(LeaseOwner::Generation {
                key,
                generation: id,
            });
        }
    }
}
