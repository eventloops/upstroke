//! Extended notes: `docs/internals/topology/fold/outcome.md`

use super::*;

impl RunState {
    pub(super) fn derived_outcome(&self) -> DerivedOutcome {
        if !self.common() {
            return DerivedOutcome::NotEnding;
        }
        if self.halted_at.is_some() {
            return DerivedOutcome::Ending(RunOutcome::Halted);
        }
        if self.budget_stop_is_current() {
            return DerivedOutcome::Ending(RunOutcome::BudgetExceeded);
        }
        if self.structurally_admissible() || self.backoff_pending() {
            return DerivedOutcome::NotEnding;
        }
        if self.questions_open() {
            return DerivedOutcome::Ending(RunOutcome::Parked);
        }
        if self.complete_shape() {
            return DerivedOutcome::Ending(RunOutcome::Complete);
        }
        DerivedOutcome::FoldError
    }

    pub(super) fn common(&self) -> bool {
        self.tasks.iter().all(|task| {
            task.open()
                .is_none_or(|generation| !generation.class.blocks_run_end())
        }) && self.transaction.is_none()
    }

    pub(super) fn structurally_admissible(&self) -> bool {
        let keys = u32::try_from(self.tasks.len()).unwrap_or(u32::MAX);
        (0..keys)
            .map(TaskKey)
            .any(|key| self.ready(key) || self.ready_retry(key))
            || self.integration_admissible()
    }

    pub(super) fn ready(&self, key: TaskKey) -> bool {
        let (Some(task), Some(entry)) = (self.tasks.get(key.index()), self.registry.get(key))
        else {
            return false;
        };
        task.state == TaskState::Pending
            && task.open().is_none()
            && entry.deps.iter().all(|dep| {
                self.tasks
                    .get(dep.index())
                    .is_some_and(|dep| dep.state == TaskState::Merged)
            })
            && !self.lineage_has_question(key)
            && !self.queue.holds_task(key)
            && self
                .transaction
                .as_ref()
                .is_none_or(|open| open.candidate.key != key)
            && self.dispatch_lease_check(key, entry, task)
            && self.pipeline_reservable()
            && !self.run_is_ending()
    }

    pub(super) fn dispatch_lease_check(
        &self,
        key: TaskKey,
        entry: &TaskEntry,
        task: &TaskFold,
    ) -> bool {
        if entry.lineage.is_some() {
            return true;
        }
        let Ok(generation) = u32::try_from(task.generations.len()) else {
            return false;
        };
        !self.leases.overlaps_another(
            LeaseOwner::Generation {
                key,
                generation: GenerationId(generation),
            },
            &predicted_region(entry),
            &self.started.path_policy,
        )
    }

    pub(super) fn ready_retry(&self, key: TaskKey) -> bool {
        let Some(task) = self.tasks.get(key.index()) else {
            return false;
        };
        let retained = task.open().is_some_and(|generation| {
            matches!(
                &generation.class,
                GenerationClass::RetainedIdle { incarnation, .. } if *incarnation == self.epoch
            )
        });
        task.state == TaskState::Pending
            && retained
            && !self.lineage_has_question(key)
            && self
                .transaction
                .as_ref()
                .is_none_or(|open| open.candidate.key != key)
            && self.pipeline_reservable()
            && !self.run_is_ending()
    }

    pub(super) fn eligible_continuation(&self, key: TaskKey) -> Option<GenerationId> {
        if self.run_is_ending() || self.lineage_has_question(key) {
            return None;
        }
        self.tasks
            .get(key.index())
            .and_then(TaskFold::open)
            .filter(|generation| generation.class == GenerationClass::OpenNoAttempt)
            .map(|generation| generation.id)
    }

    pub(super) fn pipeline_reservable(&self) -> bool {
        self.pipeline_held()
            < usize::try_from(self.started.limits.max_parallel).unwrap_or(usize::MAX)
    }

    pub(super) fn integration_admissible(&self) -> bool {
        self.eligible_integration_candidate().is_some()
    }

    pub(super) fn eligible_integration_candidate(&self) -> Option<&CandidateRef> {
        if self.transaction.is_some() || !self.pipeline_reservable() || self.run_is_ending() {
            return None;
        }
        self.queue
            .first_eligible(
                |key| self.task_is_awaiting_input(key),
                &self.leases,
                &self.started.path_policy,
            )
            .map(|entry| &entry.candidate)
    }

    pub(super) fn backoff_pending(&self) -> bool {
        !self.deferred_tasks.is_empty()
            || self
                .queue
                .entries()
                .iter()
                .any(|entry| entry.verification_deferred)
    }

    pub(super) fn questions_open(&self) -> bool {
        !self.questions.is_empty()
    }

    pub(super) fn complete_shape(&self) -> bool {
        let blocked = self.blocked_tasks();
        self.tasks.iter().enumerate().all(|(index, task)| {
            task.state.is_terminal()
                || (task.state == TaskState::Pending
                    && u32::try_from(index).is_ok_and(|key| blocked.contains(&TaskKey(key))))
        }) && self.queue.is_empty()
            && !self.leases.any_candidate_or_lineage()
    }

    pub(super) fn blocked_tasks(&self) -> BTreeSet<TaskKey> {
        let mut blocked = BTreeSet::new();
        loop {
            let mut grew = false;
            for (index, task) in self.tasks.iter().enumerate() {
                let Ok(key) = u32::try_from(index).map(TaskKey) else {
                    continue;
                };
                if task.state != TaskState::Pending || blocked.contains(&key) {
                    continue;
                }
                let Some(entry) = self.registry.get(key) else {
                    continue;
                };
                let poisoned = entry.deps.iter().any(|dep| {
                    blocked.contains(dep)
                        || self
                            .tasks
                            .get(dep.index())
                            .is_some_and(|dep| dep.state == TaskState::Failed)
                });
                if poisoned {
                    blocked.insert(key);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        blocked
    }
}
