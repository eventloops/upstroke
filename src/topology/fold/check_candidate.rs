//! Extended notes: `docs/internals/topology/fold/check_candidate.md`

use super::*;

impl RunState {
    pub(super) fn check_candidate_prepared(
        &self,
        prepared: &CandidatePrepared,
    ) -> Result<(), FoldError> {
        const KIND: &str = "candidate_prepared";
        let entry = self.entry(KIND, prepared.key)?;
        let task = self.task(KIND, prepared.key)?;
        let generation = self.open_generation(KIND, task, prepared.key, prepared.generation)?;
        if !matches!(generation.class, GenerationClass::InFlight { .. }) {
            return Err(FoldError::NotTheOpenGeneration {
                kind: KIND,
                key: prepared.key.0,
                generation: prepared.generation.0,
                detail: format!(
                    "the generation is {}, and a candidate is prepared by a generation whose \
                     attempt is still in flight — this event is its settlement",
                    generation.class.name()
                ),
            });
        }
        if generation.candidate.is_some() {
            return Err(FoldError::NotTheOpenGeneration {
                kind: KIND,
                key: prepared.key.0,
                generation: prepared.generation.0,
                detail: "the generation has already prepared a candidate, and one generation \
                         prepares at most one"
                    .to_owned(),
            });
        }
        if !prepared.attempt.is_successful() {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "attempt {} of generation {} does not record a successful attempt — \
                     failure {:?}, review outcomes {:?} — and `candidate_prepared` is the \
                     settlement of an attempt that succeeded",
                    prepared.attempt.attempt,
                    prepared.generation.0,
                    prepared.attempt.failure.as_ref().map(|f| f.kind),
                    prepared
                        .attempt
                        .reviews
                        .iter()
                        .map(|pass| pass.outcome)
                        .collect::<Vec<_>>()
                ),
            });
        }
        let obliged: Vec<&str> = entry
            .reviews
            .obliged_lenses()
            .iter()
            .map(|lens| lens.name())
            .collect();
        let recorded: Vec<&str> = prepared
            .attempt
            .reviews
            .iter()
            .map(|pass| pass.pass.as_str())
            .collect();
        if recorded != obliged {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "attempt {} of generation {} records the review pass(es) {:?} and this task \
                     is frozen to require {:?}, in that order — every configured pass runs and \
                     passes, and a record does not choose which ones it is judged on",
                    prepared.attempt.attempt, prepared.generation.0, recorded, obliged
                ),
            });
        }
        if prepared.attempt.attempt != generation.attempts {
            return Err(FoldError::WrongAttempt {
                kind: KIND,
                key: prepared.key.0,
                generation: prepared.generation.0,
                attempt: prepared.attempt.attempt,
                expected: generation.attempts.to_string(),
            });
        }
        if !prepared.parent_is_base() {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "the candidate is parented on {} and the work started from {}",
                    prepared.parent_sha, prepared.base_sha
                ),
            });
        }
        if prepared.base_sha != generation.base_sha {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it records base {} and generation {} was dispatched at {}",
                    prepared.base_sha, prepared.generation.0, generation.base_sha
                ),
            });
        }
        match (&prepared.lease_effect, entry.lineage) {
            (CandidateLeaseEffect::ReplacesPredicted { paths }, None) => {
                if *paths != prepared.actual_paths {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: "the region it takes is not the region its diff touched".to_owned(),
                    });
                }
            }
            (CandidateLeaseEffect::WidensLineage { root, paths }, Some(lineage)) => {
                if *root != lineage.root {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "it widens lineage {root} and its task descends from {}",
                            lineage.root
                        ),
                    });
                }
                if *paths != prepared.actual_paths {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: "the region it widens by is not the region its diff touched"
                            .to_owned(),
                    });
                }
            }
            _ => {
                return Err(FoldError::InconsistentRecord {
                    kind: KIND,
                    detail: "a lineage member widens its lineage and an ordinary candidate \
                             replaces its predicted region; this does the other one"
                        .to_owned(),
                });
            }
        }
        Ok(())
    }

    pub(super) fn check_candidate_created(
        &self,
        created: &TaskCandidateCreated,
    ) -> Result<(), FoldError> {
        const KIND: &str = "task_candidate_created";
        let candidate = &created.candidate;
        let task = self.task(KIND, candidate.key)?;
        let generation = self.open_generation(KIND, task, candidate.key, candidate.generation)?;
        let prepared = match &generation.candidate {
            Some(prepared) if generation.class == GenerationClass::Promoting => prepared,
            _ => {
                return Err(FoldError::NotTheOpenGeneration {
                    kind: KIND,
                    key: candidate.key.0,
                    generation: candidate.generation.0,
                    detail: format!(
                        "the generation is {} and has prepared no candidate",
                        generation.class.name()
                    ),
                });
            }
        };
        if prepared.candidate != *candidate {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it promotes commit {} at `{}` and the prepared candidate is {} at `{}`",
                    candidate.commit_sha,
                    candidate.candidate_ref,
                    prepared.candidate.commit_sha,
                    prepared.candidate.candidate_ref
                ),
            });
        }
        Ok(())
    }
}
