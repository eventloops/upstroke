//! Extended notes: `docs/internals/topology/fold/start.md`

use super::*;

impl TopologyFold {
    pub(super) fn check_run_started(
        &self,
        started: &RunStarted4,
    ) -> Result<TaskRegistry, FoldError> {
        if self.run.is_some() {
            return Err(FoldError::AlreadyStarted);
        }
        if !started.is_topology_schema() {
            return Err(FoldError::NotTopologySchema {
                schema: started.schema,
            });
        }
        started
            .runner
            .completeness()
            .map_err(|defect| FoldError::IncompleteRunner {
                defect: defect.to_string(),
            })?;

        if started.normalized_plan_digest != self.inputs.normalized_plan_digest {
            return Err(FoldError::DigestMismatch {
                what: "normalized plan",
                recorded: started.normalized_plan_digest.clone(),
                actual: self.inputs.normalized_plan_digest.clone(),
            });
        }
        let registry = TaskRegistry::originals_with_agents(
            &self.inputs.plan,
            &started.registry_record(),
            &started.probed_agents,
        )
        .map_err(|error| FoldError::RegistryUnbuildable {
            detail: error.to_string(),
        })?;
        let actual = registry.digest();
        if actual != started.registry_digest {
            return Err(FoldError::DigestMismatch {
                what: "registry",
                recorded: started.registry_digest.clone(),
                actual,
            });
        }

        for entry in registry.entries() {
            check_ladder(entry.key, &entry.ladder)?;
        }
        Ok(registry)
    }

    pub(super) fn check_started_run(
        &self,
        run: &RunState,
        event: &TopologyEvent,
        kind: &'static str,
    ) -> Result<TopologyDelta, FoldError> {
        if let Some(outcome) = run.finished.as_ref() {
            let continues = match outcome {
                RunOutcome::Complete | RunOutcome::Halted => false,
                RunOutcome::Parked | RunOutcome::BudgetExceeded => {
                    matches!(event.body, TopologyEventBody::RunResumed { .. })
                }
            };
            if !continues {
                return Err(FoldError::RunIsOver {
                    kind,
                    outcome: outcome_name(outcome),
                });
            }
        }

        let derived = match &event.body {
            TopologyEventBody::RunStarted { .. } => Err(FoldError::AlreadyStarted),
            TopologyEventBody::RunResumed { data } => {
                run.check_run_resumed(data).map(|()| Derived::None)
            }
            TopologyEventBody::TaskSpawned { data } => {
                run.check_task_spawned(&data.spawn).map(|()| Derived::None)
            }
            TopologyEventBody::TaskDispatched { data } => {
                run.check_dispatched(data).map(|()| Derived::None)
            }
            TopologyEventBody::AttemptStarted { data } => {
                run.check_attempt_started(data).map(|()| Derived::None)
            }
            TopologyEventBody::AttemptFinished { data } => {
                run.check_attempt_finished(data).map(|()| Derived::None)
            }
            TopologyEventBody::AttemptInterrupted { data } => {
                run.check_attempt_interrupted(data).map(|()| Derived::None)
            }
            TopologyEventBody::GenerationClosed { data } => {
                run.check_generation_closed(data).map(|()| Derived::None)
            }
            TopologyEventBody::DeferWaitElapsed { .. } => {
                run.check_defer_wait_elapsed().map(|()| Derived::None)
            }
            TopologyEventBody::CandidatePrepared { data } => {
                run.check_candidate_prepared(data).map(|()| Derived::None)
            }
            TopologyEventBody::TaskCandidateCreated { data } => {
                run.check_candidate_created(data).map(|()| Derived::None)
            }
            TopologyEventBody::MergeVerificationStarted { data } => {
                run.check_verification_started(data).map(|()| Derived::None)
            }
            TopologyEventBody::MergeVerificationUnavailable { data } => run
                .check_verification_unavailable(data)
                .map(|()| Derived::None),
            TopologyEventBody::MergeVerificationInterrupted { data } => run
                .check_verification_interrupted(data)
                .map(|()| Derived::None),
            TopologyEventBody::MergePrepared { data } => {
                run.check_merge_prepared(data).map(|()| Derived::None)
            }
            TopologyEventBody::MergeRejected { data } => {
                run.check_merge_rejected(data).map(|()| Derived::None)
            }
            TopologyEventBody::TaskMerged { data } => {
                run.check_task_merged(data).map(|()| Derived::None)
            }
            TopologyEventBody::QuestionRaised { data } => run
                .check_question_raised(&data.question)
                .map(|()| Derived::None),
            TopologyEventBody::QuestionAnswered { data } => {
                run.check_question_answered(data).map(Derived::Answer)
            }
            TopologyEventBody::BudgetExceeded { data } => {
                run.check_budget_exceeded(data).map(|()| Derived::None)
            }
            TopologyEventBody::RunFinished { data } => {
                run.check_run_finished(data).map(|()| Derived::None)
            }
            TopologyEventBody::CapacitySnapshot { .. }
            | TopologyEventBody::PoolExhausted { .. }
            | TopologyEventBody::DesignDefect { .. } => Ok(Derived::None),
        };
        derived.map(|derived| self.delta(event, derived))
    }

    pub fn derived_outcome(&self) -> DerivedOutcome {
        self.run
            .as_ref()
            .map_or(DerivedOutcome::NotEnding, RunState::derived_outcome)
    }
}

pub(super) fn outcome_name(outcome: &RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Complete => "complete",
        RunOutcome::Parked => "parked",
        RunOutcome::Halted => "halted",
        RunOutcome::BudgetExceeded => "budget exceeded",
    }
}

pub(super) fn check_ladder(key: TaskKey, ladder: &FrozenLadder) -> Result<(), FoldError> {
    let malformed = |defect: String| FoldError::MalformedLadder { key: key.0, defect };

    if let (Some(floor), Some(ceiling)) = (ladder.floor, ladder.ceiling) {
        if floor > ceiling {
            return Err(malformed(format!(
                "its floor is `{floor}` and its ceiling is `{ceiling}`, so no tier satisfies both"
            )));
        }
    }
    if ladder.attempts_per == 0 {
        return Err(malformed(
            "it allows 0 attempts per rung, so no attempt is ever permitted".to_owned(),
        ));
    }
    let mut previous: Option<Tier> = None;
    for tier in &ladder.tiers {
        if let Some(previous) = previous {
            if *tier <= previous {
                return Err(malformed(format!(
                    "its tiers are recorded as `{}`, which does not escalate: `{tier}` does not \
                     outrank `{previous}`",
                    ladder
                        .tiers
                        .iter()
                        .map(Tier::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
        previous = Some(*tier);
    }
    if ladder.ceiling != ladder.tiers.iter().copied().max() {
        return Err(malformed(format!(
            "its recorded ceiling is {:?} and its highest rung is {:?}",
            ladder.ceiling.map(|tier| tier.to_string()),
            ladder
                .tiers
                .iter()
                .copied()
                .max()
                .map(|tier| tier.to_string())
        )));
    }
    match &ladder.admission {
        Admission::Runnable => {
            if ladder.rungs.is_empty() {
                return Err(malformed(
                    "it is admitted as runnable and has no rungs, so there is no binding to run"
                        .to_owned(),
                ));
            }
        }
        Admission::HumanBinding { options } => {
            if !ladder.rungs.is_empty() {
                return Err(malformed(
                    "it waits for a human binding and already has rungs, so two authorities name \
                     what runs"
                        .to_owned(),
                ));
            }
            if options.is_empty() {
                return Err(malformed(
                    "it waits for a human binding and offers no agent to choose from".to_owned(),
                ));
            }
        }
    }
    if !ladder.rungs.is_empty() && ladder.rungs.len() != ladder.tiers.len() {
        return Err(malformed(format!(
            "it has {} rung binding(s) for {} tier(s)",
            ladder.rungs.len(),
            ladder.tiers.len()
        )));
    }
    for (rung, tier) in ladder.rungs.iter().zip(&ladder.tiers) {
        if rung.tier != *tier {
            return Err(malformed(format!(
                "its `{tier}` rung is bound at `{}`",
                rung.tier
            )));
        }
    }
    Ok(())
}
