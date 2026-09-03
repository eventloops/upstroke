//! `run_started`, and the dispatch of everything after it.
//!
//! The two checks that bracket a run: the one that builds the registry a fold
//! is derived against, and the match that routes every later event to the
//! check that owns it.

use super::*;

impl TopologyFold {
    // -----------------------------------------------------------------------
    // run_started
    // -----------------------------------------------------------------------

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
        // refusals[5], first half: the record must name everything needed to
        // re-establish the runner. The digest is not required — it is the
        // manifest digest when the runtime reported one (INV-23).
        started
            .runner
            .completeness()
            .map_err(|defect| FoldError::IncompleteRunner {
                defect: defect.to_string(),
            })?;

        // refusals[4]: both digests, against the bytes this reader was handed.
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

        // Ladder validation at the fold boundary: a malformed ladder is refused
        // before it is stored, not when something tries to climb it.
        for entry in registry.entries() {
            check_ladder(entry.key, &entry.ladder)?;
        }
        Ok(registry)
    }

    // -----------------------------------------------------------------------
    // Everything after run_started
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    pub(super) fn check_started_run(
        &self,
        run: &RunState,
        event: &TopologyEvent,
        kind: &'static str,
    ) -> Result<TopologyDelta, FoldError> {
        // refusals[21]: a Complete or Halted run is finalized and then refused,
        // never continued. A Parked or BudgetExceeded run continues, and the
        // only event that continues it is the resume that opens the next epoch.
        if let Some(outcome) = run.finished.clone() {
            match outcome {
                RunOutcome::Complete | RunOutcome::Halted => {
                    return Err(FoldError::RunIsOver {
                        kind,
                        outcome: outcome_name(&outcome),
                    });
                }
                RunOutcome::Parked | RunOutcome::BudgetExceeded => {
                    if !matches!(event.body, TopologyEventBody::RunResumed { .. }) {
                        return Err(FoldError::RunIsOver {
                            kind,
                            outcome: outcome_name(&outcome),
                        });
                    }
                }
            }
        }

        match &event.body {
            TopologyEventBody::RunStarted { .. } => Err(FoldError::AlreadyStarted),
            TopologyEventBody::RunResumed { data } => run
                .check_run_resumed(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::TaskSpawned { data } => run
                .check_spawn(&data.spawn, kind)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::TaskDispatched { data } => run
                .check_dispatched(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::AttemptStarted { data } => run
                .check_attempt_started(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::AttemptFinished { data } => run
                .check_attempt_finished(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::AttemptInterrupted { data } => run
                .check_attempt_interrupted(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::GenerationClosed { data } => run
                .check_generation_closed(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::DeferWaitElapsed { .. } => run
                .check_defer_wait_elapsed()
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::CandidatePrepared { data } => run
                .check_candidate_prepared(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::TaskCandidateCreated { data } => run
                .check_candidate_created(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergeVerificationStarted { data } => run
                .check_verification_started(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergeVerificationUnavailable { data } => run
                .check_verification_unavailable(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergeVerificationInterrupted { data } => run
                .check_verification_interrupted(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergePrepared { data } => run
                .check_merge_prepared(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::MergeRejected { data } => run
                .check_merge_rejected(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::TaskMerged { data } => run
                .check_task_merged(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::QuestionRaised { data } => run
                .check_question_raised(&data.question)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::QuestionAnswered { data } => run
                .check_question_answered(data)
                .map(|origin| self.delta(event, Derived::Answer(origin))),
            TopologyEventBody::BudgetExceeded { data } => run
                .check_budget_exceeded(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::RunFinished { data } => run
                .check_run_finished(data)
                .map(|()| self.delta(event, Derived::None)),
            TopologyEventBody::CapacitySnapshot { .. }
            | TopologyEventBody::PoolExhausted { .. }
            | TopologyEventBody::DesignDefect { .. } => Ok(self.delta(event, Derived::None)),
        }
    }

    // -----------------------------------------------------------------------
    // The derived outcome
    // -----------------------------------------------------------------------

    /// The total outcome function (`decisions.run_end_policy.derived_outcome`).
    ///
    /// Computed from durable state alone: no spend, no capacity, no runner
    /// availability, no clock. The legacy precedence is preserved —
    /// halt > budget > parked > complete — and pending backoff makes `Parked`
    /// and `Complete` [`DerivedOutcome::NotEnding`] without ever blocking
    /// `Halted` or `BudgetExceeded`.
    ///
    /// A run that has not started is [`DerivedOutcome::NotEnding`]: nothing has
    /// been recorded, so nothing has ended.
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

/// Whether a frozen ladder is one an attempt could actually climb.
///
/// Fold-boundary work rather than registry work: the registry derives a ladder
/// from whatever the run recorded, and this decides whether that ladder may
/// enter a fold's state. Both malformations it names are invisible to the
/// registry — a floor above its ceiling clips to nothing on the first
/// escalation, and a tier list that does not ascend makes "the next rung" mean
/// two different things depending on whether it is read by position or by tier.
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
