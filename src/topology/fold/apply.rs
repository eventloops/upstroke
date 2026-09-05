//! The application half of INV-02: what a checked transition does to the
//! state, and nothing that decides whether it may.
//!
//! **A pure function of `(state, event, derived)`.** Nothing here reads a
//! clock, the environment, or randomness, and nothing performs I/O; the only
//! values it computes rather than copies are `epoch + 1` on a resume, the
//! `next_sequence` increments, and a task's per-rung attempt count, each a
//! function of prior durable state alone. So a live fold and a replay of the
//! bytes it appended reach the same [`RunState`], which is the equality INV-02
//! turns into a property of the type: [`RunState`] derives `PartialEq`, and two
//! of them are how a live fold and a replay are proved identical. The one value
//! a transition needs that is not in its own body — a question's origin, gone
//! from `questions` the instant [`RunState::apply_answer`] removes it — is
//! carried by [`super::Derived`], decided by the check and therefore identical
//! on replay.
//!
//! **The state owns snapshots of what the log records.** Almost every `.clone()`
//! in this module copies a value into durable fold state — a `TaskEntry` into
//! the registry, a region into the lease table or the queue, a session, base or
//! candidate identity into a generation — the owned-snapshot semantics §6
//! blesses, since the fold outlives every event it folded. The one exception is
//! in `apply_merge_rejected`, which clones a held candidate region into a local
//! so the borrow of `self.tasks` is released before `self.leases` is widened: a
//! small owned value taken to unborrow, named here rather than claimed a
//! snapshot.

use super::*;

// ---------------------------------------------------------------------------
// RunState: the application
// ---------------------------------------------------------------------------

impl RunState {
    /// Apply a transition the check accepted.
    ///
    /// Total by construction: every lookup here was proved to succeed by the
    /// check that produced the delta, so no lookup misses on a path this fold
    /// takes. The safety is that the miss is unreachable, not that each miss is
    /// inert — most lookups leave the state alone on a miss, but three do not:
    /// `apply_candidate_created` falls back to the conservative
    /// `PathSet::RepoWide` and still enqueues, `apply_verification_started`
    /// advances `next_sequence` before its queue lookup, and `open_question`'s
    /// task lookup defaults to `Pending` and inserts the question regardless.
    /// Nothing in this function decides anything — a decision made here would be
    /// one the live path and the replay path could reach differently, which
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
            TopologyEventBody::QuestionAnswered { data } => match derived {
                Derived::Answer(origin) => self.apply_answer(data, *origin),
                // Exhaustive over `Derived`, so a new variant is a compile error
                // here rather than a silent no-op: the check pairs every
                // `question_answered` with `Derived::Answer`, and these two
                // cannot be its delta, so they change nothing.
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
        // Exhaustive over the outcome, so a new `UnavailableOutcome` variant is
        // a compile error here rather than a silent no-op: two separate
        // `if let`s over a closed two-variant enum is a wildcard by another
        // spelling, which §5 forbids over a domain a new variant should force a
        // decision at. Behaviour is unchanged — both outcomes drop the open
        // sequence from the queue entry, only a defer sets the backoff, and only
        // a park raises a question and moves the task to awaiting input.
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
        // Exhaustive over the disposition, so a new self-opening
        // `PreparedDisposition` is a compile error here rather than one that
        // silently skips the increment: a fast publication opens and closes its
        // own transaction and consumes a sequence, while a stale-clean or
        // already-present one was opened by its `merge_verification_started`,
        // which already advanced `next_sequence`.
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

    pub(super) fn apply_merge_rejected(&mut self, rejected: &MergeRejected) {
        // Exhaustive over the disposition, same reason as `apply_merge_prepared`:
        // a conflict is decided at the cherry-pick and opens and closes its own
        // transaction, consuming a sequence, while a code rejection was opened
        // by its `merge_verification_started`, which already advanced
        // `next_sequence`.
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

    pub(super) fn apply_answer(&mut self, answered: &QuestionAnswered4, origin: QuestionOrigin) {
        let parked_from = self
            .questions
            .remove(&answered.question)
            .map(|open| open.parked_from);
        // `origin` is superseded by `parked_from` for the answer-return, and the
        // `VerificationPark => AwaitingMerge` cross-check that read it was
        // withdrawn under pass-3 review: a bare question can park a task before a
        // verification park raises its own on the same task, so a
        // `VerificationPark` question's parked-from state can be `AwaitingInput`,
        // not `AwaitingMerge` — the assertion was reachable from a checked log
        // and would panic every replay of it. `origin` is kept threaded only
        // because removing it reaches `Derived::Answer` in `start.rs` (row 38)
        // and `check_end.rs`'s return (row 32, merged from #153) — a second
        // branch in files this sweep must not open; the removal is
        // `SWEEP-FOLD-APPLY-ORIGIN-SUPERSEDED`.
        let _ = origin;
        match &answered.answer {
            Answer4::Answered {
                binding_override, ..
            } => {
                if let Some(binding) = binding_override {
                    self.overrides.insert(answered.key, binding.clone());
                }
                // Return the task to the state it was parked from — but only
                // while it is *still* parked. `check_end.rs` at this head does
                // not guarantee the task did not move between the question and
                // its answer: nothing refuses a `task_merged` or a failing
                // settlement on a task whose question is open, so restoring the
                // recorded `parked_from` unconditionally would un-merge a
                // `Merged` task and make `derived_outcome` a `FoldError`. **The
                // invariant this restoration requires is that the task is still
                // `AwaitingInput`.** When it is not, the move stands and the
                // answer only closes the question. Guarding here rather than
                // trusting a door in another file makes it correct whatever the
                // check admits.
                if let Some(state) = parked_from {
                    if self
                        .tasks
                        .get(answered.key.index())
                        .is_some_and(|task| task.state == TaskState::AwaitingInput)
                    {
                        self.set_state(answered.key, state);
                    }
                }
            }
            Answer4::Declined { decline_halts_run } => {
                // **A decline fails the whole lineage, not just the answered
                // task** — `design/26`, "Declining fails the lineage.", and
                // `release_holdings_of`'s own doc, "a declined lineage fails as a
                // whole". A repair and every task back to its root fail together,
                // because the original awaits a repair this decline abandons:
                // failing the repair alone leaves the root `AwaitingRepair` with
                // no queue, question, generation or runnable repair, so
                // `derived_outcome` has no answer for it (`FoldError`) and refuses
                // every `run_finished`. **Unconditional on `decline_halts_run`** —
                // that flag decides only whether the run also halts, and setting
                // it would otherwise hide this wedge behind the halt.
                let root = self
                    .registry
                    .get(answered.key)
                    .and_then(|entry| entry.lineage)
                    .map_or(answered.key, |lineage| lineage.root);
                let members: Vec<TaskKey> = std::iter::once(root)
                    .chain(
                        self.registry
                            .entries()
                            .iter()
                            .filter(|entry| {
                                entry.lineage.is_some_and(|lineage| lineage.root == root)
                            })
                            .map(|entry| entry.key),
                    )
                    .collect();
                for member in members {
                    self.set_state(member, TaskState::Failed);
                }
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
        // The state the question parks the task in, read before the caller sets
        // `AwaitingInput`: every caller opens the question while the task is
        // still in the state its answer should return it to. A missing task is
        // impossible here — the check registered it — and defaults to `Pending`.
        let parked_from = self
            .tasks
            .get(question.key.index())
            .map_or(TaskState::Pending, |task| task.state);
        self.seen_questions.insert(question.id.clone());
        self.questions.insert(
            question.id.clone(),
            OpenQuestion {
                question: question.clone(),
                origin,
                parked_from,
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
        // Exhaustive over the lease, so a new `GenerationLease` is a compile
        // error here rather than one that silently keeps its region held: an
        // own generation holds its predicted region and releases it when it
        // closes, and an inherited-lineage generation took none of its own and
        // releases nothing.
        let releases_own_region = match generation.lease {
            GenerationLease::Own => true,
            GenerationLease::InheritedLineage { .. } => false,
        };
        generation.class = GenerationClass::Closed;
        if releases_own_region {
            self.leases.release(LeaseOwner::Generation {
                key,
                generation: id,
            });
        }
    }
}
