//! The derived outcome (INV-15) and the structural predicates it and the
//! selection accessors are both answered from.

use super::*;

impl RunState {
    // -----------------------------------------------------------------------
    // derived_outcome
    // -----------------------------------------------------------------------

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

    /// No generation is open and no integration transaction is unresolved.
    pub(super) fn common(&self) -> bool {
        self.tasks.iter().all(|task| {
            task.open()
                .is_none_or(|generation| !generation.class.blocks_run_end())
        }) && self.transaction.is_none()
    }

    /// Some task could be dispatched, retried, or integrated from this state
    /// alone. Budget, capacity and runner availability are not consulted.
    pub(super) fn structurally_admissible(&self) -> bool {
        (0..self.tasks.len())
            .map(|index| TaskKey(u32::try_from(index).unwrap_or(u32::MAX)))
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
            && self.dispatch_lease_check(key, entry)
            && self.pipeline_reservable()
            && !self.run_is_ending()
    }

    /// A repair dispatch is never lease-blocked; an ordinary one is blocked by
    /// any overlapping active lease of another owner.
    ///
    /// The predicted region is not in the log until the dispatch that takes it,
    /// so the check the *fold* can make is over the run's own leases: a task
    /// with a repo-wide prediction is admissible exactly when nothing is held.
    pub(super) fn dispatch_lease_check(&self, key: TaskKey, entry: &TaskEntry) -> bool {
        if entry.lineage.is_some() {
            return true;
        }
        let predicted = predicted_region(entry);
        !self.leases.overlaps_another(
            LeaseOwner::Generation {
                key,
                generation: GenerationId(
                    u32::try_from(
                        self.tasks
                            .get(key.index())
                            .map_or(0, |task| task.generations.len()),
                    )
                    .unwrap_or(u32::MAX),
                ),
            },
            &predicted,
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

    pub(super) fn pipeline_reservable(&self) -> bool {
        self.pipeline_held()
            < usize::try_from(self.started.limits.max_parallel).unwrap_or(usize::MAX)
    }

    /// `permits.provisional_reservations` gives integration selection the
    /// `{pipeline, merge}` pair, and `deadlock_freedom` takes a reservation
    /// "only when the derived count permits" — so the entitlement is a clause
    /// of admissibility here for the same reason it is one in [`Self::ready`]
    /// and [`Self::ready_retry`], and not a check the caller is trusted to
    /// remember. `permits.pipeline` counts an unresolved integration
    /// transaction among the held, which is the other half of the same
    /// statement: a selector that admitted an integration while the count was
    /// at `max_parallel` would open the entitlement that is already held.
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
                || (task.state == TaskState::Pending && blocked.contains(&index))
        }) && self.queue.is_empty()
            && !self.leases.any_candidate_or_lineage()
    }

    /// Every task that can never run because a failure sits in its transitive
    /// dependency closure.
    pub(super) fn blocked_tasks(&self) -> BTreeSet<usize> {
        let mut blocked = BTreeSet::new();
        // To a fixed point, not in one pass. A *repair*'s dependencies refer
        // only backwards, but an original's keys are assigned in plan order
        // (`keys_by_display_id`) and plan order is not topological order, so
        // an ordinary plan can have a task depend on a later key. One forward
        // pass would then decide that task before it had decided what the task
        // waits on, and a failure two hops away would go unseen — which is the
        // difference between "directly failed dependency" and the transitive
        // closure the packet asks for.
        //
        // Each round adds at least one member or stops, and membership only
        // grows, so this runs at most `tasks.len()` rounds.
        loop {
            let mut grew = false;
            for (index, task) in self.tasks.iter().enumerate() {
                if task.state != TaskState::Pending || blocked.contains(&index) {
                    continue;
                }
                let Some(entry) = self
                    .registry
                    .get(TaskKey(u32::try_from(index).unwrap_or(u32::MAX)))
                else {
                    continue;
                };
                let poisoned = entry.deps.iter().any(|dep| {
                    blocked.contains(&dep.index())
                        || self
                            .tasks
                            .get(dep.index())
                            .is_some_and(|dep| dep.state == TaskState::Failed)
                });
                if poisoned {
                    blocked.insert(index);
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
