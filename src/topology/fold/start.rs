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

        let recorded_policy = match started.path_policy.version {
            PathPolicyVersion::V2 => None,
            PathPolicyVersion::V1 => Some("v1"),
        };
        if let Some(recorded) = recorded_policy {
            return Err(FoldError::InconsistentRecord {
                kind: "run_started",
                detail: format!(
                    "it freezes path policy `{recorded}`, and this binary derives dispatch \
                     regions under path policy `v2`; the derivation that wrote this run's \
                     dispatch records is not the one this binary applies, so a run recorded \
                     under `{recorded}` cannot be replayed here"
                ),
            });
        }

        if started.limits.max_parallel == 0 {
            return Err(FoldError::UnusableLimit {
                limit: "max_parallel",
                value: started.limits.max_parallel,
            });
        }

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
    if let (Some(floor), Some(start)) = (ladder.floor, ladder.tiers.first().copied()) {
        if floor > start {
            return Err(malformed(format!(
                "its floor is `{floor}` and its chain starts at `{start}`, so its first attempt \
                 runs below the floor the run recorded"
            )));
        }
    }
    if ladder.attempts_per == 0 {
        return Err(malformed(
            "it allows 0 attempts per rung, so no attempt is ever permitted".to_owned(),
        ));
    }
    for (previous, tier) in ladder.tiers.iter().zip(ladder.tiers.iter().skip(1)) {
        if tier <= previous {
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ir::{Effort, ResolvedEffortPolicy};
    use crate::topology::registry::FrozenRung;

    const KEY: TaskKey = TaskKey(7);

    fn rung(tier: Tier) -> FrozenRung {
        FrozenRung {
            tier,
            agent: format!("agent-{tier}"),
            model: format!("model-{tier}"),
            pinned: false,
        }
    }

    fn ladder(tiers: &[Tier], rungs: &[Tier], admission: Admission) -> FrozenLadder {
        FrozenLadder {
            tiers: tiers.to_vec(),
            attempts_per: 2,
            rungs: rungs.iter().copied().map(rung).collect(),
            floor: tiers.first().copied(),
            ceiling: tiers.iter().copied().max(),
            effort: ResolvedEffortPolicy {
                small: Effort::Low,
                mid: Effort::XHigh,
                frontier: Effort::Max,
                review: Effort::Medium,
            },
            admission,
        }
    }

    fn defect(ladder: &FrozenLadder) -> Option<String> {
        match check_ladder(KEY, ladder) {
            Err(FoldError::MalformedLadder { key, defect }) if key == KEY.0 => Some(defect),
            _ => None,
        }
    }

    #[test]
    fn a_runnable_ladder_binds_one_rung_for_every_tier() {
        let every = [Tier::Small, Tier::Mid, Tier::Frontier];
        assert_eq!(
            check_ladder(KEY, &ladder(&every, &every, Admission::Runnable)),
            Ok(())
        );

        let short = ladder(&every, &[Tier::Small], Admission::Runnable);
        assert_eq!(
            defect(&short).as_deref(),
            Some("it has 1 rung binding(s) for 3 tier(s)")
        );

        let long = ladder(
            &[Tier::Small],
            &[Tier::Small, Tier::Mid],
            Admission::Runnable,
        );
        assert_eq!(
            defect(&long).as_deref(),
            Some("it has 2 rung binding(s) for 1 tier(s)")
        );
    }

    #[test]
    fn a_ladder_may_not_start_below_its_recorded_floor() {
        // The floor clips the chain start: `design/07` writes `min_tier` as
        // "clips the chain start (binding)", `design/10` §2 has an override
        // truncating the chain start, and `design/26` has a repair's `mid`
        // floor intersected with the frozen pin and maximum. `src/route.rs`'s
        // `raise_start` implements the clip, so no router-produced chain holds
        // a tier below its floor -- but `TaskRegistry::frozen_ladder` copies
        // `task.min_tier` into `floor` and the recorded tiers into `tiers`
        // and compares neither with the other, so a recorded ladder that does
        // is exactly what this boundary exists to catch.
        let every = [Tier::Small, Tier::Mid, Tier::Frontier];

        let at_the_floor = FrozenLadder {
            floor: Some(Tier::Small),
            ..ladder(&every, &every, Admission::Runnable)
        };
        assert_eq!(check_ladder(KEY, &at_the_floor), Ok(()));

        let above_the_floor = FrozenLadder {
            floor: Some(Tier::Mid),
            ..ladder(&[Tier::Frontier], &[Tier::Frontier], Admission::Runnable)
        };
        assert_eq!(check_ladder(KEY, &above_the_floor), Ok(()));

        let below_the_floor = FrozenLadder {
            floor: Some(Tier::Mid),
            ..ladder(&every, &every, Admission::Runnable)
        };
        assert_eq!(
            defect(&below_the_floor).as_deref(),
            Some(
                "its floor is `mid` and its chain starts at `small`, so its first attempt runs \
                 below the floor the run recorded"
            )
        );
    }

    #[test]
    fn a_human_binding_ladder_keeps_its_tiers_and_binds_none_of_them() {
        let waiting = ladder(
            &[Tier::Mid, Tier::Frontier],
            &[],
            Admission::HumanBinding {
                options: vec!["codex-cli".to_owned()],
            },
        );
        assert_eq!(check_ladder(KEY, &waiting), Ok(()));
    }
}
