//! Extended notes: `docs/internals/topology/fold/predicates.md`

use super::*;

impl TopologyFold {
    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub fn started(&self) -> Option<&RunStarted4> {
        self.run.as_ref().map(|run| &*run.started)
    }

    pub fn registry(&self) -> Option<&TaskRegistry> {
        self.run.as_ref().map(|run| &run.registry)
    }

    pub fn task(&self, key: TaskKey) -> Option<&TaskFold> {
        self.run.as_ref()?.tasks.get(key.index())
    }

    pub fn task_state(&self, key: TaskKey) -> Option<TaskState> {
        self.task(key).map(|task| task.state)
    }

    pub fn queue(&self) -> Option<&CandidateQueue> {
        self.run.as_ref().map(|run| &run.queue)
    }

    pub fn leases(&self) -> Option<&LeaseTable> {
        self.run.as_ref().map(|run| &run.leases)
    }

    pub fn transaction(&self) -> Option<&Transaction> {
        self.run.as_ref()?.transaction.as_ref()
    }

    pub fn epoch(&self) -> Option<Epoch> {
        self.run.as_ref().map(|run| run.epoch)
    }

    pub fn halted_at(&self) -> Option<TaskKey> {
        self.run.as_ref()?.halted_at
    }

    pub fn budget_stop(&self) -> Option<BudgetStop> {
        self.run.as_ref()?.budget_stop
    }

    pub fn finished(&self) -> Option<&RunOutcome> {
        self.run.as_ref()?.finished.as_ref()
    }

    pub fn state(&self) -> Option<&RunState> {
        self.run.as_ref()
    }

    pub fn open_questions(&self) -> Option<&BTreeMap<QuestionId, OpenQuestion>> {
        self.run.as_ref().map(|run| &run.questions)
    }

    pub fn binding_override(&self, key: TaskKey) -> Option<&BindingOverride> {
        self.run.as_ref()?.overrides.get(&key)
    }

    #[must_use]
    pub fn ready(&self, key: TaskKey) -> bool {
        !self.poisoned && self.run.as_ref().is_some_and(|run| run.ready(key))
    }

    #[must_use]
    pub fn ready_retry(&self, key: TaskKey) -> bool {
        !self.poisoned && self.run.as_ref().is_some_and(|run| run.ready_retry(key))
    }

    #[must_use]
    pub(crate) fn eligible_continuation(&self, key: TaskKey) -> Option<GenerationId> {
        if self.poisoned {
            return None;
        }
        self.run
            .as_ref()
            .and_then(|run| run.eligible_continuation(key))
    }

    #[must_use]
    pub fn pipeline_held(&self) -> usize {
        self.run.as_ref().map_or(0, RunState::pipeline_held)
    }

    #[must_use]
    pub fn pipeline_reservable(&self) -> bool {
        !self.poisoned && self.run.as_ref().is_some_and(RunState::pipeline_reservable)
    }

    #[must_use]
    pub fn structurally_admissible(&self) -> bool {
        !self.poisoned
            && self
                .run
                .as_ref()
                .is_some_and(RunState::structurally_admissible)
    }

    #[must_use]
    pub fn integration_admissible(&self) -> bool {
        !self.poisoned
            && self
                .run
                .as_ref()
                .is_some_and(RunState::integration_admissible)
    }

    pub(crate) fn eligible_integration_candidate(&self) -> Option<&CandidateRef> {
        if self.poisoned {
            return None;
        }
        self.run
            .as_ref()
            .and_then(RunState::eligible_integration_candidate)
    }

    #[must_use]
    pub fn run_is_ending(&self) -> bool {
        self.run.as_ref().is_some_and(RunState::run_is_ending)
    }

    #[must_use]
    pub fn backoff_pending(&self) -> bool {
        self.run.as_ref().is_some_and(RunState::backoff_pending)
    }

    #[must_use]
    pub fn frozen_rung_binding(&self, key: TaskKey, rung: u32) -> Option<RungBinding> {
        let entry = self.registry()?.get(key)?;
        let frozen = entry.ladder.rungs.get(usize::try_from(rung).ok()?)?;
        Some(RungBinding::from_frozen(
            frozen,
            entry.ladder.effort.implementation_for(frozen.tier),
        ))
    }

    #[must_use]
    pub fn open_no_attempt(&self, key: TaskKey) -> Option<GenerationId> {
        self.task(key)?
            .generations
            .iter()
            .find(|generation| generation.class == GenerationClass::OpenNoAttempt)
            .map(|generation| generation.id)
    }

    #[must_use]
    pub fn predicted_region(&self, key: TaskKey) -> Option<PathSet> {
        self.registry()
            .and_then(|registry| registry.get(key))
            .map(predicted_region)
    }

    #[must_use]
    pub fn questions_open(&self) -> bool {
        self.run.as_ref().is_some_and(RunState::questions_open)
    }
}
