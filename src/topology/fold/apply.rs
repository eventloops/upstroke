//! Extended notes: `docs/internals/topology/fold/apply.md`

use super::*;

impl RunState {
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
            }
            TopologyEventBody::AttemptFinished { data } => self.apply_settlement(data),
            TopologyEventBody::AttemptInterrupted { data } => {
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
                self.open_question(&data.question, QuestionOrigin::Admission, None);
                self.set_state(data.question.key, TaskState::AwaitingInput);
            }
            TopologyEventBody::QuestionAnswered { data } => match derived {
                Derived::Answer(QuestionOrigin::VerificationPark | QuestionOrigin::Admission) => {
                    self.apply_answer(data);
                }
                Derived::None | Derived::Registry(_) => {}
            },
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

    fn apply_resumed(&mut self, resumed: &RunResumed4) {
        self.epoch = Epoch(self.epoch.0.saturating_add(1));
        self.incarnation = resumed.incarnation.clone();
        self.budget_stop = None;
        self.finished = None;
        self.wake_backoff();
    }

    fn wake_backoff(&mut self) {
        self.queue.wake_deferred();
        for key in std::mem::take(&mut self.deferred_tasks) {
            self.refresh_task_state(key);
        }
    }

    fn register(&mut self, spawn: &FrozenSpawn) {
        self.registry.register(spawn.entry.clone());
        self.tasks.push(TaskFold::new());
        match &spawn.admission {
            SpawnAdmission::Runnable => {}
            SpawnAdmission::HumanRequired { question, .. } => {
                self.open_question(question, QuestionOrigin::Admission, None);
                self.set_state(spawn.key, TaskState::AwaitingInput);
            }
            SpawnAdmission::HumanBinding { options, question } => {
                self.open_question(question, QuestionOrigin::Admission, Some(options.clone()));
                self.set_state(spawn.key, TaskState::AwaitingInput);
            }
        }
    }

    fn apply_dispatched(&mut self, dispatched: &TaskDispatched) {
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

    fn apply_settlement(&mut self, finished: &AttemptFinished4) {
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
                SettlementTransition::Succeeded => {}
                SettlementTransition::Retry => {
                    self.close_generation(finished.key);
                }
                SettlementTransition::Escalated { rung } => {
                    self.close_generation(finished.key);
                    if let Some(task) = self.tasks.get_mut(finished.key.index()) {
                        task.rung = *rung;
                        task.attempts_on_rung = 0;
                    }
                }
                SettlementTransition::Deferred { defers, .. } => {
                    self.close_generation(finished.key);
                    self.set_state(finished.key, TaskState::Deferred);
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

    fn record_halt(&mut self, key: TaskKey) {
        if self.halted_at.is_none() {
            self.halted_at = Some(key);
            self.halted_epoch = Some(self.epoch);
        }
    }

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

    fn apply_candidate_prepared(&mut self, prepared: &CandidatePrepared) {
        let record = PreparedCandidate {
            candidate: prepared.candidate(),
            base_sha: prepared.base_sha.clone(),
            tree_sha: prepared.tree_sha.clone(),
            paths: prepared.actual_paths.clone(),
        };
        if let Some(generation) = self.open_generation_mut(prepared.key) {
            generation.candidate = Some(record);
            generation.class = GenerationClass::Promoting;
        }
        self.charge_allowance(prepared.key, &prepared.attempt);
        take_candidate_region(
            &mut self.leases,
            &prepared.lease_effect,
            prepared.key,
            prepared.generation,
        );
        self.set_state(prepared.key, TaskState::AwaitingMerge);
    }

    fn apply_candidate_created(&mut self, candidate: &CandidateRef) {
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

    fn apply_verification_started(&mut self, started: &MergeVerificationStarted) {
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

    fn apply_verification_unavailable(&mut self, unavailable: &MergeVerificationUnavailable) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        let candidate = transaction.candidate;
        match &unavailable.outcome {
            UnavailableOutcome::Deferred { defers } => {
                if let Some(entry) = self.queue.get_mut(candidate.key, candidate.generation) {
                    entry.sequence = None;
                    entry.verification_deferred = true;
                    entry.defers = *defers;
                }
            }
            UnavailableOutcome::Parked { question } => {
                if let Some(entry) = self.queue.get_mut(candidate.key, candidate.generation) {
                    entry.sequence = None;
                }
                self.open_question(question, QuestionOrigin::VerificationPark, None);
                self.set_state(candidate.key, TaskState::AwaitingInput);
            }
        }
    }

    fn release_transaction(&mut self) {
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

    fn apply_merge_prepared(&mut self, prepared: &MergePrepared) {
        match prepared.disposition {
            PreparedDisposition::Fast => {
                self.next_sequence = self.next_sequence.saturating_add(1);
            }
            PreparedDisposition::StaleClean | PreparedDisposition::AlreadyPresent => {}
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

    fn apply_merge_rejected(&mut self, rejected: &MergeRejected) {
        match rejected.disposition {
            RejectionDisposition::Conflict { .. } => {
                self.next_sequence = self.next_sequence.saturating_add(1);
            }
            RejectionDisposition::CodeRejected { .. } => {}
        }
        self.transaction = None;
        let candidate = &rejected.candidate;
        self.queue.remove(candidate.key, candidate.generation);
        match &rejected.lease_effect {
            RejectionLeaseEffect::CreatesLineage { root, paths } => {
                let held = self
                    .tasks
                    .get(candidate.key.index())
                    .and_then(|task| {
                        task.generations
                            .iter()
                            .find(|generation| generation.id == candidate.generation)
                    })
                    .and_then(|generation| generation.candidate.as_ref())
                    .map(|prepared| &prepared.paths);
                if let Some(held) = held {
                    self.leases.widen_lineage(*root, held);
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

    fn apply_task_merged(&mut self, merged: &TaskMerged) {
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

    fn apply_answer(&mut self, answered: &QuestionAnswered4) {
        self.questions.remove(&answered.question);
        match &answered.answer {
            Answer4::Answered {
                binding_override, ..
            } => {
                if let Some(binding) = binding_override {
                    self.overrides.insert(answered.key, binding.clone());
                }
                self.refresh_task_state(answered.key);
            }
            Answer4::Declined { decline_halts_run } => {
                self.fail_lineage(answered.key);
                if *decline_halts_run {
                    self.record_halt(answered.key);
                }
            }
        }
    }

    fn refresh_task_state(&mut self, key: TaskKey) {
        let Some(task) = self.tasks.get(key.index()) else {
            return;
        };
        let facts = VisibleFacts {
            terminal: task.state.is_terminal(),
            own_question: self.open_question_for(key).is_some(),
            queued_candidate: self.queue.holds_task(key),
            owns_transaction: self
                .transaction
                .as_ref()
                .is_some_and(|transaction| transaction.candidate.key == key),
            repair_child: self
                .registry
                .entries()
                .iter()
                .any(|entry| entry.lineage.is_some_and(|lineage| lineage.parent == key)),
            unelapsed_backoff: self.deferred_tasks.contains(&key),
        };
        if let Some(state) = derived_state(facts) {
            self.set_state(key, state);
        }
    }

    fn fail_lineage(&mut self, key: TaskKey) {
        let root = self.lineage_root(key);
        let cancels_verification = self.transaction.as_ref().is_some_and(|transaction| {
            self.lineage_root(transaction.candidate.key) == root
                && match &transaction.class {
                    TransactionClass::VerificationStarted { .. } => true,
                    TransactionClass::Prepared { .. } => false,
                }
        });
        if cancels_verification {
            self.release_transaction();
        }
        let members: Vec<TaskKey> = self
            .registry
            .entries()
            .iter()
            .filter(|entry| entry.key == root || entry.lineage.is_some_and(|l| l.root == root))
            .map(|entry| entry.key)
            .collect();
        for member in members {
            self.close_generation(member);
            self.release_holdings_of(member);
            self.deferred_tasks.remove(&member);
            if self
                .tasks
                .get(member.index())
                .is_some_and(|task| task.state != TaskState::Merged)
            {
                self.set_state(member, TaskState::Failed);
            }
        }
        let registry = &self.registry;
        self.questions.retain(|_, open| {
            open.question.key != root
                && !registry
                    .get(open.question.key)
                    .and_then(|entry| entry.lineage)
                    .is_some_and(|lineage| lineage.root == root)
        });
    }

    fn release_holdings_of(&mut self, key: TaskKey) {
        if let Some(task) = self.tasks.get(key.index()) {
            for generation in &task.generations {
                self.queue.remove(key, generation.id);
                self.leases.release(LeaseOwner::Candidate {
                    key,
                    generation: generation.id,
                });
            }
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
        match state {
            TaskState::Deferred => {
                self.deferred_tasks.insert(key);
            }
            TaskState::AwaitingInput => {}
            TaskState::Pending
            | TaskState::AwaitingMerge
            | TaskState::AwaitingRepair
            | TaskState::Merged
            | TaskState::Failed => {
                self.deferred_tasks.remove(&key);
            }
        }
        if let Some(task) = self.tasks.get_mut(key.index()) {
            task.state = state;
        }
    }

    fn set_defers(&mut self, key: TaskKey, defers: u32) {
        if let Some(task) = self.tasks.get_mut(key.index()) {
            task.defers = defers;
        }
    }

    pub(super) fn open_generation_mut(&mut self, key: TaskKey) -> Option<&mut GenerationFold> {
        self.tasks.get_mut(key.index())?.open_mut()
    }

    fn close_generation(&mut self, key: TaskKey) {
        let Some(generation) = self.open_generation_mut(key) else {
            return;
        };
        let id = generation.id;
        let releases_region = releases_own_region(generation.lease);
        generation.class = GenerationClass::Closed;
        if releases_region {
            self.leases.release(LeaseOwner::Generation {
                key,
                generation: id,
            });
        }
    }
}

fn take_candidate_region(
    leases: &mut LeaseTable,
    effect: &CandidateLeaseEffect,
    key: TaskKey,
    generation: GenerationId,
) {
    match effect {
        CandidateLeaseEffect::ReplacesPredicted { paths } => {
            leases.release(LeaseOwner::Generation { key, generation });
            leases.grant(LeaseOwner::Candidate { key, generation }, paths.clone());
        }
        CandidateLeaseEffect::WidensLineage { root, paths } => {
            leases.widen_lineage(*root, paths);
        }
    }
}

fn releases_own_region(lease: GenerationLease) -> bool {
    match lease {
        GenerationLease::Own => true,
        GenerationLease::InheritedLineage { .. } => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisibleFacts {
    terminal: bool,
    own_question: bool,
    queued_candidate: bool,
    owns_transaction: bool,
    repair_child: bool,
    unelapsed_backoff: bool,
}

fn derived_state(facts: VisibleFacts) -> Option<TaskState> {
    if facts.terminal {
        return None;
    }
    Some(if facts.own_question {
        TaskState::AwaitingInput
    } else if facts.queued_candidate || facts.owns_transaction {
        TaskState::AwaitingMerge
    } else if facts.repair_child {
        TaskState::AwaitingRepair
    } else if facts.unelapsed_backoff {
        TaskState::Deferred
    } else {
        TaskState::Pending
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion};

    const KEY: TaskKey = TaskKey(2);
    const OTHER: TaskKey = TaskKey(5);
    const ROOT: TaskKey = TaskKey(1);
    const GEN: GenerationId = GenerationId(0);

    fn policy() -> PathPolicy {
        PathPolicy {
            version: PathPolicyVersion::V1,
            case_fold: false,
            grammar: PathGrammar::Globset,
        }
    }

    fn region(name: &str) -> PathSet {
        PathSet::Prefixes {
            paths: vec![GitPath(format!("src/{name}"))],
        }
    }

    fn quiet() -> VisibleFacts {
        VisibleFacts {
            terminal: false,
            own_question: false,
            queued_candidate: false,
            owns_transaction: false,
            repair_child: false,
            unelapsed_backoff: false,
        }
    }

    #[test]
    fn an_ordinary_candidate_gives_up_its_generations_region_and_holds_the_one_it_touched() {
        let mut leases = LeaseTable::new();
        leases.grant(
            LeaseOwner::Generation {
                key: KEY,
                generation: GEN,
            },
            region("predicted"),
        );
        take_candidate_region(
            &mut leases,
            &CandidateLeaseEffect::ReplacesPredicted {
                paths: region("actual"),
            },
            KEY,
            GEN,
        );

        assert!(
            !leases.holds(LeaseOwner::Generation {
                key: KEY,
                generation: GEN
            }),
            "the prediction the dispatch took is released by the candidate that replaces it"
        );
        assert!(leases.holds(LeaseOwner::Candidate {
            key: KEY,
            generation: GEN
        }));
        let onlooker = LeaseOwner::Generation {
            key: OTHER,
            generation: GEN,
        };
        assert!(
            leases.overlaps_another(onlooker, &region("actual"), &policy()),
            "the region it now holds is the one its diff touched"
        );
        assert!(
            !leases.overlaps_another(onlooker, &region("predicted"), &policy()),
            "and the prediction is no longer held by anyone"
        );
    }

    #[test]
    fn a_lineage_members_candidate_widens_its_lineage_and_takes_no_lease_of_its_own() {
        let mut leases = LeaseTable::new();
        leases.widen_lineage(ROOT, &region("root"));
        take_candidate_region(
            &mut leases,
            &CandidateLeaseEffect::WidensLineage {
                root: ROOT,
                paths: region("repair"),
            },
            KEY,
            GEN,
        );

        let lease = leases
            .lineage(ROOT)
            .expect("the lineage still holds a lease");
        assert_eq!(
            lease.paths,
            PathSet::Prefixes {
                paths: vec![
                    GitPath("src/root".to_owned()),
                    GitPath("src/repair".to_owned())
                ]
            },
            "the lineage's region grows by exactly the region the repair's diff touched"
        );
        assert!(
            !leases.holds(LeaseOwner::Candidate {
                key: KEY,
                generation: GEN
            }),
            "a lineage member's candidate takes no holding of its own"
        );
        assert!(
            !leases.holds(LeaseOwner::Generation {
                key: KEY,
                generation: GEN
            }),
            "and releases none, because its generation inherited the lineage's"
        );
    }

    #[test]
    fn only_a_generation_that_holds_its_own_region_releases_one_when_it_closes() {
        assert!(releases_own_region(GenerationLease::Own));
        assert!(!releases_own_region(GenerationLease::InheritedLineage {
            root: ROOT
        }));
    }

    #[test]
    fn a_terminal_task_keeps_the_state_it_reached() {
        let every_other_fact = VisibleFacts {
            terminal: true,
            own_question: true,
            queued_candidate: true,
            owns_transaction: true,
            repair_child: true,
            unelapsed_backoff: true,
        };
        assert_eq!(derived_state(every_other_fact), None);
        assert_eq!(
            derived_state(VisibleFacts {
                terminal: true,
                ..quiet()
            }),
            None
        );
    }

    #[test]
    fn an_open_question_of_its_own_outranks_every_other_visible_fact() {
        let alone = VisibleFacts {
            own_question: true,
            ..quiet()
        };
        assert_eq!(derived_state(alone), Some(TaskState::AwaitingInput));
        for (name, facts) in [
            (
                "a queued candidate",
                VisibleFacts {
                    queued_candidate: true,
                    ..alone
                },
            ),
            (
                "an owned transaction",
                VisibleFacts {
                    owns_transaction: true,
                    ..alone
                },
            ),
            (
                "a registered repair child",
                VisibleFacts {
                    repair_child: true,
                    ..alone
                },
            ),
            (
                "an unelapsed backoff",
                VisibleFacts {
                    unelapsed_backoff: true,
                    ..alone
                },
            ),
        ] {
            assert_eq!(
                derived_state(facts),
                Some(TaskState::AwaitingInput),
                "{name} does not outrank a question the task still has of its own"
            );
        }
    }

    #[test]
    fn a_queued_candidate_or_an_owned_transaction_is_awaiting_merge() {
        for (name, facts) in [
            (
                "queued",
                VisibleFacts {
                    queued_candidate: true,
                    ..quiet()
                },
            ),
            (
                "under an integration it owns",
                VisibleFacts {
                    owns_transaction: true,
                    ..quiet()
                },
            ),
            (
                "both",
                VisibleFacts {
                    queued_candidate: true,
                    owns_transaction: true,
                    ..quiet()
                },
            ),
            (
                "queued with a repair child and a backoff",
                VisibleFacts {
                    queued_candidate: true,
                    repair_child: true,
                    unelapsed_backoff: true,
                    ..quiet()
                },
            ),
        ] {
            assert_eq!(
                derived_state(facts),
                Some(TaskState::AwaitingMerge),
                "a task that is {name} is awaiting merge"
            );
        }
    }

    #[test]
    fn a_repair_child_outranks_an_unelapsed_backoff_and_a_quiet_task_is_pending() {
        assert_eq!(
            derived_state(VisibleFacts {
                repair_child: true,
                unelapsed_backoff: true,
                ..quiet()
            }),
            Some(TaskState::AwaitingRepair)
        );
        assert_eq!(
            derived_state(VisibleFacts {
                repair_child: true,
                ..quiet()
            }),
            Some(TaskState::AwaitingRepair)
        );
        assert_eq!(
            derived_state(VisibleFacts {
                unelapsed_backoff: true,
                ..quiet()
            }),
            Some(TaskState::Deferred)
        );
        assert_eq!(derived_state(quiet()), Some(TaskState::Pending));
    }
}
