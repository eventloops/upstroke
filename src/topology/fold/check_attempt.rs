//! Extended notes: `docs/internals/topology/fold/check_attempt.md`

use super::region::{
    admission_name, check_lease_disposition, describe_region, spawn_admission_name,
};
use super::start::check_ladder;
use super::*;

impl RunState {
    pub(super) fn check_run_resumed(&self, resumed: &RunResumed4) -> Result<(), FoldError> {
        if let Some(field) = self.started.runner.difference(&resumed.runner) {
            return Err(FoldError::RunnerMoved {
                field: field.to_string(),
            });
        }
        Ok(())
    }

    pub(super) fn check_task_spawned(&self, spawn: &FrozenSpawn) -> Result<(), FoldError> {
        const KIND: &str = "task_spawned";
        self.check_spawn(spawn, KIND)?;
        if spawn.admission.question().is_some() {
            if let Some(lineage) = spawn.entry.lineage {
                self.check_question_can_park_lineage(KIND, lineage.root)?;
            }
        }
        Ok(())
    }

    pub(super) fn check_spawn(
        &self,
        spawn: &FrozenSpawn,
        kind: &'static str,
    ) -> Result<(), FoldError> {
        let malformed = |detail: String| FoldError::MalformedEntry {
            kind,
            key: spawn.key.0,
            detail,
        };
        if spawn.key.index() != self.registry.len() {
            return Err(FoldError::NonDenseKey {
                kind,
                key: spawn.key.0,
                len: self.registry.len(),
            });
        }
        let entry = &spawn.entry;
        if entry.key != spawn.key {
            return Err(malformed(format!(
                "the embedded entry calls itself {} and the event registers {}",
                entry.key, spawn.key
            )));
        }
        if self.registry.key_of(entry.display_id.as_str()).is_some() {
            return Err(malformed(format!(
                "the display id `{}` already names another task",
                entry.display_id
            )));
        }
        let Some(lineage) = entry.lineage else {
            return Err(malformed(
                "a registered task descends from the rejection that produced it, and this one \
                 records no lineage"
                    .to_owned(),
            ));
        };
        if lineage.root >= spawn.key || lineage.parent >= spawn.key {
            return Err(malformed(format!(
                "its lineage names root {} and parent {}, and a key may only refer backwards from \
                 {}",
                lineage.root, lineage.parent, spawn.key
            )));
        }
        self.entry(kind, lineage.root)?;
        self.entry(kind, lineage.parent)?;
        if self.lineage_root(lineage.parent) != lineage.root {
            return Err(malformed(format!(
                "parent {} belongs to lineage {}, not recorded root {}",
                lineage.parent,
                self.lineage_root(lineage.parent),
                lineage.root
            )));
        }
        for ancestor in [lineage.root, lineage.parent] {
            if self.task(kind, ancestor)?.state == TaskState::Failed {
                return Err(malformed(format!(
                    "ancestor {ancestor} has failed; a new repair cannot revive declined work"
                )));
            }
        }
        if entry.allowed_agents != self.started.probed_agents {
            return Err(malformed(format!(
                "it allows {:?} and this run probed {:?}",
                entry.allowed_agents, self.started.probed_agents
            )));
        }
        if entry.deps.len() != entry.display_deps.len() {
            return Err(malformed(format!(
                "it records {} dependency key(s) and {} display dependency(ies)",
                entry.deps.len(),
                entry.display_deps.len()
            )));
        }
        for (dep, display) in entry.deps.iter().zip(&entry.display_deps) {
            if *dep >= spawn.key {
                return Err(malformed(format!(
                    "it depends on {dep}, which is not registered before it"
                )));
            }
            let known = self.entry(kind, *dep)?;
            if known.display_id != *display {
                return Err(malformed(format!(
                    "it names dependency {dep} as `{display}`, and that key is `{}`",
                    known.display_id
                )));
            }
            if self.task(kind, *dep)?.state != TaskState::Merged {
                return Err(malformed(format!(
                    "it depends on {dep}, which is `{}` — a repair's dependencies are merged \
                     before it is registered",
                    self.task(kind, *dep)?.state.name()
                )));
            }
        }
        check_ladder(spawn.key, &entry.ladder)?;
        self.check_admission(spawn, &malformed)?;
        Ok(())
    }

    pub(super) fn check_admission<F>(
        &self,
        spawn: &FrozenSpawn,
        malformed: &F,
    ) -> Result<(), FoldError>
    where
        F: Fn(String) -> FoldError,
    {
        match (&spawn.admission, &spawn.entry.ladder.admission) {
            (SpawnAdmission::Runnable, Admission::Runnable) => {}
            (SpawnAdmission::HumanRequired { limit, .. }, Admission::Runnable) => {
                if *limit != self.started.limits.max_merge_repairs {
                    return Err(malformed(format!(
                        "it reports the automatic-repair limit as {limit} and this run froze {}",
                        self.started.limits.max_merge_repairs
                    )));
                }
            }
            (
                SpawnAdmission::HumanBinding { options, .. },
                Admission::HumanBinding {
                    options: frozen, ..
                },
            ) => {
                if options != frozen {
                    return Err(malformed(
                        "the event and the entry offer different bindings to choose from"
                            .to_owned(),
                    ));
                }
            }
            (event, _) => {
                return Err(malformed(format!(
                    "its admission is `{}` and its entry's is `{}`",
                    spawn_admission_name(event),
                    admission_name(&spawn.entry.ladder.admission)
                )));
            }
        }
        if let Some(question) = spawn.admission.question() {
            self.check_new_question("task_spawned", question, spawn.key)?;
        }
        Ok(())
    }

    pub(super) fn check_new_question(
        &self,
        kind: &'static str,
        question: &FrozenQuestion,
        key: TaskKey,
    ) -> Result<(), FoldError> {
        if !question.is_complete() {
            return Err(FoldError::UnanswerableQuestion {
                kind,
                detail: format!(
                    "`{}` has no identity, no context, or no options, so the task it parks has no \
                     way to continue",
                    question.id
                ),
            });
        }
        if question.key != key {
            return Err(FoldError::UnanswerableQuestion {
                kind,
                detail: format!(
                    "`{}` is keyed to task {} and this event parks task {key}",
                    question.id, question.key
                ),
            });
        }
        if self.seen_questions.contains(&question.id) {
            return Err(FoldError::WrongQuestion {
                kind,
                question: question.id.to_string(),
                detail: "this log has already used that identity; a question is asked once"
                    .to_owned(),
            });
        }
        Ok(())
    }

    pub(super) fn check_dispatched(&self, dispatched: &TaskDispatched) -> Result<(), FoldError> {
        const KIND: &str = "task_dispatched";
        let entry = self.entry(KIND, dispatched.key)?;
        let task = self.task(KIND, dispatched.key)?;

        if task.state != TaskState::Pending {
            return Err(FoldError::WrongTaskState {
                kind: KIND,
                key: dispatched.key.0,
                state: task.state.name(),
                expected: "pending",
            });
        }
        if self.lineage_has_question(dispatched.key)
            || self.queue.holds_task(dispatched.key)
            || self
                .transaction
                .as_ref()
                .is_some_and(|transaction| transaction.candidate.key == dispatched.key)
        {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: "dispatch requires no outstanding lineage question or candidate for this \
                         task"
                    .to_owned(),
            });
        }
        if let Some(open) = task.open() {
            return Err(FoldError::NotTheOpenGeneration {
                kind: KIND,
                key: dispatched.key.0,
                generation: dispatched.generation.0,
                detail: format!("generation {} is still {}", open.id.0, open.class.name()),
            });
        }
        if usize::try_from(dispatched.generation.0).unwrap_or(usize::MAX) != task.generations.len()
        {
            return Err(FoldError::NonDenseKey {
                kind: KIND,
                key: dispatched.generation.0,
                len: task.generations.len(),
            });
        }

        let is_repair = entry.lineage.is_some();
        match (&dispatched.lease, entry.lineage) {
            (LeaseGrant::Predicted { paths }, None) => {
                let derived = predicted_region(entry);
                if *paths != derived {
                    return Err(FoldError::MalformedEntry {
                        kind: KIND,
                        key: dispatched.key.0,
                        detail: format!(
                            "it takes the predicted region {} and this entry's frozen path \
                             hints derive {}; an ordinary dispatch takes the region the fold \
                             admitted it on",
                            describe_region(paths),
                            describe_region(&derived)
                        ),
                    });
                }
            }
            (LeaseGrant::InheritedLineage { root }, Some(lineage)) => {
                if *root != lineage.root {
                    return Err(FoldError::MalformedEntry {
                        kind: KIND,
                        key: dispatched.key.0,
                        detail: format!(
                            "it executes inside lineage {root} and its entry descends from {}",
                            lineage.root
                        ),
                    });
                }
            }
            (LeaseGrant::Predicted { .. }, Some(_)) => {
                return Err(FoldError::MalformedEntry {
                    kind: KIND,
                    key: dispatched.key.0,
                    detail: "a repair takes no lease of its own; it executes inside the lineage \
                             lease its root already holds"
                        .to_owned(),
                });
            }
            (LeaseGrant::InheritedLineage { .. }, None) => {
                return Err(FoldError::MalformedEntry {
                    kind: KIND,
                    key: dispatched.key.0,
                    detail: "an ordinary task belongs to no lineage and cannot inherit one's lease"
                        .to_owned(),
                });
            }
        }
        if is_repair != dispatched.source_candidate.is_some() {
            return Err(FoldError::MalformedEntry {
                kind: KIND,
                key: dispatched.key.0,
                detail: if is_repair {
                    "a repair is materialized from the candidate its lineage rejected, and this \
                     dispatch names none"
                        .to_owned()
                } else {
                    "an ordinary dispatch materializes nothing and this one names a source \
                     candidate"
                        .to_owned()
                },
            });
        }
        Ok(())
    }

    pub(super) fn check_attempt_started(&self, started: &AttemptStarted4) -> Result<(), FoldError> {
        const KIND: &str = "attempt_started";
        let entry = self.entry(KIND, started.key)?;
        let task = self.task(KIND, started.key)?;
        let generation = self.open_generation(KIND, task, started.key, started.generation)?;

        if self.lineage_has_question(started.key) {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "task {} belongs to a lineage with an unanswered question",
                    started.key
                ),
            });
        }

        match (&generation.class, &started.resume_session) {
            (GenerationClass::OpenNoAttempt, None) => {}
            (
                GenerationClass::RetainedIdle {
                    session,
                    incarnation,
                },
                Some(resumed),
            ) => {
                if session != resumed {
                    return Err(FoldError::StaleIncarnation {
                        key: started.key.0,
                        attempt: started.attempt.0,
                        detail: format!(
                            "it resumes session `{resumed}` and the generation retained `{session}`"
                        ),
                    });
                }
                if *incarnation != self.epoch {
                    return Err(FoldError::StaleIncarnation {
                        key: started.key.0,
                        attempt: started.attempt.0,
                        detail: format!(
                            "the session was retained by incarnation {} and this run has resumed \
                             {} time(s)",
                            incarnation.0, self.epoch.0
                        ),
                    });
                }
            }
            (class, resumed) => {
                return Err(FoldError::NotTheOpenGeneration {
                    kind: KIND,
                    key: started.key.0,
                    generation: started.generation.0,
                    detail: if resumed.is_some() {
                        format!(
                            "it resumes a session and the generation is {}, not retained idle",
                            class.name()
                        )
                    } else {
                        format!(
                            "the generation is {} and a fresh attempt starts in one nothing has \
                             run in",
                            class.name()
                        )
                    },
                });
            }
        }

        let next_attempt =
            generation
                .attempts
                .checked_add(1)
                .ok_or_else(|| FoldError::InconsistentRecord {
                    kind: KIND,
                    detail: format!(
                        "task {} generation {} has exhausted its attempt numbers",
                        started.key, started.generation.0
                    ),
                })?;
        if started.attempt.0 != next_attempt {
            return Err(FoldError::WrongAttempt {
                kind: KIND,
                key: started.key.0,
                generation: started.generation.0,
                attempt: started.attempt.0,
                expected: next_attempt.to_string(),
            });
        }

        let mismatch = |detail: String| FoldError::BindingMismatch {
            key: started.key.0,
            attempt: started.attempt.0,
            detail,
        };
        if let Some(binding) = self.overrides.get(&started.key) {
            if !started.binding.matches_override(binding) {
                return Err(mismatch(format!(
                    "a human named `{}`/`{}` at effort `{}` for this task and it ran `{}`/`{}` at \
                     effort `{}`",
                    binding.agent,
                    binding.model,
                    binding.effort,
                    started.binding.agent,
                    started.binding.model,
                    started.binding.effort
                )));
            }
        } else {
            let rung = usize::try_from(started.rung).unwrap_or(usize::MAX);
            let frozen = entry.ladder.rungs.get(rung).ok_or_else(|| {
                mismatch(format!(
                    "it climbs rung {rung} of a ladder with {} rung(s)",
                    entry.ladder.rungs.len()
                ))
            })?;
            let effort = entry.ladder.effort.implementation_for(frozen.tier);
            if !started.binding.matches_frozen(frozen, effort) {
                return Err(mismatch(format!(
                    "rung {rung} is frozen as `{}`/`{}` at tier `{}` effort `{}` and it ran \
                     `{}`/`{}` at tier `{}` effort `{}`",
                    frozen.agent,
                    frozen.model,
                    frozen.tier,
                    effort,
                    started.binding.agent,
                    started.binding.model,
                    started.binding.tier,
                    started.binding.effort
                )));
            }
        }

        if entry.lineage.is_some() != started.materialization_observed.is_some() {
            return Err(FoldError::MalformedEntry {
                kind: KIND,
                key: started.key.0,
                detail: if entry.lineage.is_some() {
                    "a repair's attempt records what its worktree was materialized from".to_owned()
                } else {
                    "an ordinary attempt materializes nothing".to_owned()
                },
            });
        }
        Ok(())
    }

    pub(super) fn open_generation<'a>(
        &self,
        kind: &'static str,
        task: &'a TaskFold,
        key: TaskKey,
        generation: GenerationId,
    ) -> Result<&'a GenerationFold, FoldError> {
        let open = task.open().ok_or_else(|| FoldError::NotTheOpenGeneration {
            kind,
            key: key.0,
            generation: generation.0,
            detail: "no generation of this task is open".to_owned(),
        })?;
        if open.id != generation {
            return Err(FoldError::NotTheOpenGeneration {
                kind,
                key: key.0,
                generation: generation.0,
                detail: format!("generation {} is the open one", open.id.0),
            });
        }
        Ok(open)
    }

    pub(super) fn in_flight<'a>(
        &self,
        kind: &'static str,
        task: &'a TaskFold,
        key: TaskKey,
        generation: GenerationId,
        attempt: AttemptNumber,
    ) -> Result<&'a GenerationFold, FoldError> {
        let open = self.open_generation(kind, task, key, generation)?;
        let GenerationClass::InFlight { attempt: running } = &open.class else {
            return Err(FoldError::NotTheOpenGeneration {
                kind,
                key: key.0,
                generation: generation.0,
                detail: format!(
                    "the generation is {}, and no attempt is running",
                    open.class.name()
                ),
            });
        };
        if *running != attempt {
            return Err(FoldError::WrongAttempt {
                kind,
                key: key.0,
                generation: generation.0,
                attempt: attempt.0,
                expected: running.0.to_string(),
            });
        }
        Ok(open)
    }

    pub(super) fn check_attempt_finished(
        &self,
        finished: &AttemptFinished4,
    ) -> Result<(), FoldError> {
        const KIND: &str = "attempt_finished";
        let task = self.task(KIND, finished.key)?;
        let generation = self.in_flight(
            KIND,
            task,
            finished.key,
            finished.generation,
            finished.attempt,
        )?;

        match &finished.settlement {
            AttemptSettlement::Retained {
                retained_session,
                retained_incarnation,
            } => {
                if *retained_incarnation != self.epoch {
                    return Err(FoldError::StaleIncarnation {
                        key: finished.key.0,
                        attempt: finished.attempt.0,
                        detail: format!(
                            "it retains the session for incarnation {} and this run has resumed \
                             {} time(s)",
                            retained_incarnation.0, self.epoch.0
                        ),
                    });
                }
                if finished.record.attempt != finished.attempt.0 {
                    return Err(FoldError::WrongAttempt {
                        kind: KIND,
                        key: finished.key.0,
                        generation: finished.generation.0,
                        attempt: finished.record.attempt,
                        expected: finished.attempt.0.to_string(),
                    });
                }
                if finished.record.is_successful() {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "attempt {} of generation {} retains its session for a further \
                             attempt and its record says the attempt succeeded — failure {:?}, \
                             review outcomes {:?} — and `candidate_prepared` is the settlement \
                             of an attempt that succeeded",
                            finished.attempt.0,
                            finished.generation.0,
                            finished.record.failure.as_ref().map(|failure| failure.kind),
                            finished
                                .record
                                .reviews
                                .iter()
                                .map(|pass| pass.outcome)
                                .collect::<Vec<_>>()
                        ),
                    });
                }
                if finished.record.session_id.as_deref() != Some(retained_session.0.as_str()) {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "attempt {} of generation {} retains session `{retained_session}` \
                             and its record names {}; a retained settlement holds the session \
                             its own ledger line reports",
                            finished.attempt.0,
                            finished.generation.0,
                            match finished.record.session_id.as_deref() {
                                Some(other) => format!("`{other}`"),
                                None => "no session at all".to_owned(),
                            }
                        ),
                    });
                }
            }
            AttemptSettlement::Closed { transition, lease } => {
                if matches!(transition, SettlementTransition::Succeeded) {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "attempt {} of generation {} settles `succeeded`, and \
                             `candidate_prepared` is the sole successful settlement for a \
                             candidate-producing attempt",
                            finished.attempt.0, finished.generation.0
                        ),
                    });
                }
                if finished.record.is_successful() {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "attempt {} of generation {} settles as a failure and its record \
                             says the attempt succeeded — `candidate_prepared` is the \
                             settlement of a successful attempt",
                            finished.attempt.0, finished.generation.0
                        ),
                    });
                }
                if finished.record.attempt != finished.attempt.0 {
                    return Err(FoldError::WrongAttempt {
                        kind: KIND,
                        key: finished.key.0,
                        generation: finished.generation.0,
                        attempt: finished.record.attempt,
                        expected: finished.attempt.0.to_string(),
                    });
                }
                check_lease_disposition(KIND, finished.key, generation.lease, *lease)?;
                if let SettlementTransition::Parked { question } = transition {
                    self.check_new_question(KIND, question, finished.key)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn check_attempt_interrupted(
        &self,
        interrupted: &AttemptInterrupted4,
    ) -> Result<(), FoldError> {
        const KIND: &str = "attempt_interrupted";
        let task = self.task(KIND, interrupted.key)?;
        let generation = self.in_flight(
            KIND,
            task,
            interrupted.key,
            interrupted.generation,
            interrupted.attempt,
        )?;
        check_lease_disposition(KIND, interrupted.key, generation.lease, interrupted.lease)
    }

    pub(super) fn check_generation_closed(
        &self,
        closed: &GenerationClosed,
    ) -> Result<(), FoldError> {
        const KIND: &str = "generation_closed";
        let task = self.task(KIND, closed.key)?;
        let generation = self.open_generation(KIND, task, closed.key, closed.generation)?;
        match generation.class {
            GenerationClass::OpenNoAttempt | GenerationClass::RetainedIdle { .. } => {}
            ref class => {
                return Err(FoldError::NotTheOpenGeneration {
                    kind: KIND,
                    key: closed.key.0,
                    generation: closed.generation.0,
                    detail: format!(
                        "it is {}, and a generation is closed only from open-with-no-attempt or \
                         retained-idle",
                        class.name()
                    ),
                });
            }
        }
        check_lease_disposition(KIND, closed.key, generation.lease, closed.lease)
    }

    pub(super) fn check_defer_wait_elapsed(&self) -> Result<(), FoldError> {
        if self.halted_at.is_some() {
            return Err(FoldError::RunEnding {
                kind: "defer_wait_elapsed",
                what: "a halting settlement",
            });
        }
        if self.budget_stop_is_current() {
            return Err(FoldError::RunEnding {
                kind: "defer_wait_elapsed",
                what: "the budget stop",
            });
        }
        Ok(())
    }
}
