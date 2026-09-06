//! Extended notes: `docs/internals/topology/fold/check_candidate.md`

use super::*;

const CANDIDATE_PREPARED: &str = "candidate_prepared";

impl RunState {
    pub(super) fn check_candidate_prepared(
        &self,
        prepared: &CandidatePrepared,
    ) -> Result<(), FoldError> {
        let entry = self.entry(CANDIDATE_PREPARED, prepared.key)?;
        let task = self.task(CANDIDATE_PREPARED, prepared.key)?;
        let generation =
            self.open_generation(CANDIDATE_PREPARED, task, prepared.key, prepared.generation)?;
        if !matches!(generation.class, GenerationClass::InFlight { .. }) {
            return Err(FoldError::NotTheOpenGeneration {
                kind: CANDIDATE_PREPARED,
                key: prepared.key.0,
                generation: prepared.generation.0,
                detail: format!(
                    "the generation is {}, and a candidate is prepared by a generation whose \
                     attempt is still in flight — this event is its settlement",
                    generation.class.name()
                ),
            });
        }
        refuse_repeated_candidate(generation, prepared.key, prepared.generation)?;
        if !prepared.attempt.is_successful() {
            return Err(FoldError::InconsistentRecord {
                kind: CANDIDATE_PREPARED,
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
                kind: CANDIDATE_PREPARED,
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
                kind: CANDIDATE_PREPARED,
                key: prepared.key.0,
                generation: prepared.generation.0,
                attempt: prepared.attempt.attempt,
                expected: generation.attempts.to_string(),
            });
        }
        if !prepared.parent_is_base() {
            return Err(FoldError::InconsistentRecord {
                kind: CANDIDATE_PREPARED,
                detail: format!(
                    "the candidate is parented on {} and the work started from {}",
                    prepared.parent_sha, prepared.base_sha
                ),
            });
        }
        if prepared.base_sha != generation.base_sha {
            return Err(FoldError::InconsistentRecord {
                kind: CANDIDATE_PREPARED,
                detail: format!(
                    "it records base {} and generation {} was dispatched at {}",
                    prepared.base_sha, prepared.generation.0, generation.base_sha
                ),
            });
        }
        check_lease_effect(
            &prepared.lease_effect,
            entry.lineage.map(|lineage| lineage.root),
            &prepared.actual_paths,
        )
    }

    pub(super) fn check_candidate_created(
        &self,
        created: &TaskCandidateCreated,
    ) -> Result<(), FoldError> {
        const KIND: &str = "task_candidate_created";
        let candidate = &created.candidate;
        let task = self.task(KIND, candidate.key)?;
        let generation = self.open_generation(KIND, task, candidate.key, candidate.generation)?;
        let Some(prepared) = promoting_candidate(generation) else {
            return Err(FoldError::NotTheOpenGeneration {
                kind: KIND,
                key: candidate.key.0,
                generation: candidate.generation.0,
                detail: format!(
                    "the generation is {} and has prepared no candidate",
                    generation.class.name()
                ),
            });
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

fn check_lease_effect(
    lease_effect: &CandidateLeaseEffect,
    lineage_root: Option<TaskKey>,
    actual_paths: &PathSet,
) -> Result<(), FoldError> {
    match (lease_effect, lineage_root) {
        (CandidateLeaseEffect::ReplacesPredicted { paths }, None) => {
            if paths != actual_paths {
                return Err(FoldError::InconsistentRecord {
                    kind: CANDIDATE_PREPARED,
                    detail: "the region it takes is not the region its diff touched".to_owned(),
                });
            }
        }
        (CandidateLeaseEffect::WidensLineage { root, paths }, Some(lineage_root)) => {
            if *root != lineage_root {
                return Err(FoldError::InconsistentRecord {
                    kind: CANDIDATE_PREPARED,
                    detail: format!(
                        "it widens lineage {root} and its task descends from {lineage_root}"
                    ),
                });
            }
            if paths != actual_paths {
                return Err(FoldError::InconsistentRecord {
                    kind: CANDIDATE_PREPARED,
                    detail: "the region it widens by is not the region its diff touched".to_owned(),
                });
            }
        }
        (CandidateLeaseEffect::ReplacesPredicted { .. }, Some(_))
        | (CandidateLeaseEffect::WidensLineage { .. }, None) => {
            return Err(FoldError::InconsistentRecord {
                kind: CANDIDATE_PREPARED,
                detail: "a lineage member widens its lineage and an ordinary candidate \
                         replaces its predicted region; this does the other one"
                    .to_owned(),
            });
        }
    }
    Ok(())
}

fn refuse_repeated_candidate(
    generation: &GenerationFold,
    key: TaskKey,
    generation_id: GenerationId,
) -> Result<(), FoldError> {
    if generation.candidate.is_some() {
        return Err(FoldError::NotTheOpenGeneration {
            kind: CANDIDATE_PREPARED,
            key: key.0,
            generation: generation_id.0,
            detail: "the generation has already prepared a candidate, and one generation \
                     prepares at most one"
                .to_owned(),
        });
    }
    Ok(())
}

fn promoting_candidate(generation: &GenerationFold) -> Option<&PreparedCandidate> {
    match &generation.candidate {
        Some(prepared) if generation.class == GenerationClass::Promoting => Some(prepared),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: TaskKey = TaskKey(1);
    const ANOTHER_ROOT: TaskKey = TaskKey(7);
    const MISMATCHED_PAIRING: &str = "a lineage member widens its lineage and an ordinary \
                                      candidate replaces its predicted region; this does the \
                                      other one";

    fn region(name: &str) -> PathSet {
        PathSet::Prefixes {
            paths: vec![GitPath(format!("src/{name}/"))],
        }
    }

    fn refusal(result: &Result<(), FoldError>) -> Option<(&str, &str)> {
        match result {
            Err(FoldError::InconsistentRecord { kind, detail }) => Some((kind, detail.as_str())),
            _ => None,
        }
    }

    fn prepared() -> PreparedCandidate {
        PreparedCandidate {
            candidate: CandidateRef {
                key: TaskKey(0),
                generation: GenerationId(0),
                commit_sha: CommitSha("c".repeat(40)),
                candidate_ref: GitRef("refs/upstroke/runs/run/candidates/0/0".to_owned()),
            },
            base_sha: CommitSha("b".repeat(40)),
            tree_sha: CommitSha("t".repeat(40)),
            paths: region("alpha"),
        }
    }

    fn generation(class: GenerationClass, candidate: Option<PreparedCandidate>) -> GenerationFold {
        GenerationFold {
            id: GenerationId(0),
            class,
            base_sha: CommitSha("b".repeat(40)),
            lease: GenerationLease::Own,
            attempts: 1,
            candidate,
        }
    }

    #[test]
    fn an_ordinary_candidate_takes_exactly_the_region_its_diff_touched() {
        let taken = CandidateLeaseEffect::ReplacesPredicted {
            paths: region("alpha"),
        };
        assert_eq!(check_lease_effect(&taken, None, &region("alpha")), Ok(()));

        let elsewhere = CandidateLeaseEffect::ReplacesPredicted {
            paths: region("beta"),
        };
        let refused = check_lease_effect(&elsewhere, None, &region("alpha"));
        assert_eq!(
            refusal(&refused),
            Some((
                CANDIDATE_PREPARED,
                "the region it takes is not the region its diff touched"
            ))
        );
    }

    #[test]
    fn a_widening_that_names_a_lineage_its_task_does_not_descend_from_is_refused() {
        let widens = CandidateLeaseEffect::WidensLineage {
            root: ANOTHER_ROOT,
            paths: region("alpha"),
        };
        let refused = check_lease_effect(&widens, Some(ROOT), &region("alpha"));
        let expected =
            format!("it widens lineage {ANOTHER_ROOT} and its task descends from {ROOT}");
        assert_eq!(
            refusal(&refused),
            Some((CANDIDATE_PREPARED, expected.as_str()))
        );
    }

    #[test]
    fn a_widening_widens_by_exactly_the_region_its_diff_touched() {
        let widens = CandidateLeaseEffect::WidensLineage {
            root: ROOT,
            paths: region("alpha"),
        };
        assert_eq!(
            check_lease_effect(&widens, Some(ROOT), &region("alpha")),
            Ok(())
        );

        let elsewhere = CandidateLeaseEffect::WidensLineage {
            root: ROOT,
            paths: region("beta"),
        };
        let refused = check_lease_effect(&elsewhere, Some(ROOT), &region("alpha"));
        assert_eq!(
            refusal(&refused),
            Some((
                CANDIDATE_PREPARED,
                "the region it widens by is not the region its diff touched"
            ))
        );
    }

    #[test]
    fn a_lineage_member_widens_and_an_ordinary_candidate_replaces_and_neither_does_the_other() {
        let replaces = CandidateLeaseEffect::ReplacesPredicted {
            paths: region("alpha"),
        };
        let by_a_lineage_member = check_lease_effect(&replaces, Some(ROOT), &region("alpha"));
        assert_eq!(
            refusal(&by_a_lineage_member),
            Some((CANDIDATE_PREPARED, MISMATCHED_PAIRING))
        );

        let widens = CandidateLeaseEffect::WidensLineage {
            root: ROOT,
            paths: region("alpha"),
        };
        let by_an_ordinary_task = check_lease_effect(&widens, None, &region("alpha"));
        assert_eq!(
            refusal(&by_an_ordinary_task),
            Some((CANDIDATE_PREPARED, MISMATCHED_PAIRING))
        );
    }

    #[test]
    fn a_generation_that_already_prepared_a_candidate_is_refused() {
        let held = generation(
            GenerationClass::InFlight {
                attempt: AttemptNumber(1),
            },
            Some(prepared()),
        );
        let refused = refuse_repeated_candidate(&held, ROOT, GenerationId(3));
        assert_eq!(
            refused,
            Err(FoldError::NotTheOpenGeneration {
                kind: CANDIDATE_PREPARED,
                key: ROOT.0,
                generation: 3,
                detail: "the generation has already prepared a candidate, and one generation \
                         prepares at most one"
                    .to_owned(),
            })
        );
    }

    #[test]
    fn a_generation_with_no_candidate_yet_is_not_refused() {
        let held = generation(
            GenerationClass::InFlight {
                attempt: AttemptNumber(1),
            },
            None,
        );
        assert_eq!(
            refuse_repeated_candidate(&held, ROOT, GenerationId(3)),
            Ok(())
        );
    }

    #[test]
    fn a_promoting_generation_offers_the_candidate_it_prepared() {
        let held = generation(GenerationClass::Promoting, Some(prepared()));
        assert_eq!(promoting_candidate(&held), Some(&prepared()));
    }

    #[test]
    fn a_generation_that_is_not_promoting_offers_no_candidate_even_when_one_is_attached() {
        let classes = [
            GenerationClass::OpenNoAttempt,
            GenerationClass::InFlight {
                attempt: AttemptNumber(1),
            },
            GenerationClass::RetainedIdle {
                session: SessionId("session".to_owned()),
                incarnation: Epoch(0),
            },
            GenerationClass::Closed,
        ];
        for class in classes {
            let named = class.name();
            let held = generation(class, Some(prepared()));
            assert!(
                promoting_candidate(&held).is_none(),
                "a generation that is {named} offered a candidate to promote"
            );
        }
    }

    #[test]
    fn a_promoting_generation_that_prepared_nothing_offers_no_candidate() {
        let held = generation(GenerationClass::Promoting, None);
        assert!(promoting_candidate(&held).is_none());
    }
}
