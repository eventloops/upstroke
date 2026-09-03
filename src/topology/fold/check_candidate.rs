//! The candidate checks: the settlement that prepares one, and its creation.

use super::*;

impl RunState {
    // --- candidate_prepared ------------------------------------------------

    pub(super) fn check_candidate_prepared(
        &self,
        prepared: &CandidatePrepared,
    ) -> Result<(), FoldError> {
        const KIND: &str = "candidate_prepared";
        let entry = self.entry(KIND, prepared.key)?;
        let task = self.task(KIND, prepared.key)?;
        let generation = self.open_generation(KIND, task, prepared.key, prepared.generation)?;
        // **The generation is still in flight, because this event is what
        // settles it.** It used to require `Promoting`, which only an
        // `attempt_finished{Succeeded}` could produce — so the fold *required*
        // the dual pattern the 2026-08-12 record forbids. With the settlement
        // moved here, a `Promoting` generation means that record was appended
        // anyway, and the arm above already refuses it; this refuses the other
        // half of the same shape, so neither order can produce two settlements.
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
        // INV-06: "at most one candidate per generation", enforced_by "fold
        // refuses a second candidate for a generation". Refused here, before
        // any lease or candidate-state mutation could be planned: a second
        // record would replace the first and hand a later
        // `task_candidate_created` a candidate the queue never saw prepared.
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
        // **And the attempt it names must have succeeded.** This event is the
        // sole successful settlement for a candidate-producing attempt, so a
        // record carrying a failure is a settlement contradicting itself: the
        // candidate's own authoritative evidence would say a gate failed while
        // the fold promoted the generation and carried it to
        // `task_candidate_created`, queueing it as a success.
        //
        // Missing until 2026-08-27. The Class B change made this the successful
        // settlement and did not make the fold require success — the semantic
        // condition that motivated the change was the one condition not
        // enforced, and the round-4 review of `09f9a99` walked the five steps.
        // It also gives `TopologyRun`'s `Brief::replay` the property it already
        // assumed: a `candidate_prepared` record never carries feedback,
        // because it never carries a failure.
        //
        // `InconsistentRecord` rather than a new variant: the refusal inventory
        // is packet-enumerated, and "the event disagrees with the record it
        // cites" is exactly this kind.
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
        // **And it must have run the passes the run froze for this task.**
        // `is_successful` above asks `all` over *the passes the record happens
        // to carry*, which is a predicate the record's own author chooses the
        // domain of: a `candidate_prepared` carrying a lone passed
        // `second-opinion` — or an empty list — satisfies it, and the fold
        // charges the rung, enters `Promoting`, and permits
        // `task_candidate_created` for a tree the configured primary reviewer
        // never read. Round 6 of the `cfa1be8` review found it as its first P1;
        // that round fixed the *outcome* half — a pass recorded `Failed` or
        // `Unavailable` is refused — and this is the *presence* half.
        //
        // **Fold-side, and taking `(record, frozen)`.** The predicate needs the
        // plan and `AttemptRecord` does not carry it, so it cannot be a method
        // on the record; the entry is already in hand here for the lease and
        // lineage relations below.
        //
        // The comparison is the ordered list of pass names, so it refuses in
        // one place every way a record can disagree with its obligation: a
        // configured pass omitted, a pass duplicated, a pass nobody configured,
        // and the configured passes in another order. §11.3's own reason for
        // the order is that "a later pass only exists because every earlier one
        // approved" — a record whose second opinion precedes its acceptance
        // pass describes a review that did not happen.
        //
        // `FrozenReviews::obliged_lenses` is `review::passes_for`'s answer
        // rather than a second reading of §11.2/§11.3, and it is the same
        // reader the plan assembler dispatches from. That is the whole of why
        // this is safe to enforce: the obligation the fold requires and the
        // passes the driver runs are one derivation.
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
        // ST-06: a candidate is prepared *by the attempt that succeeded*, so
        // the embedded record names the generation's current attempt. Without
        // this the record is inert data and a candidate can be published
        // attributed to an attempt that did not produce it.
        if prepared.attempt.attempt != generation.attempts {
            return Err(FoldError::WrongAttempt {
                kind: KIND,
                key: prepared.key.0,
                generation: prepared.generation.0,
                attempt: prepared.attempt.attempt,
                expected: generation.attempts.to_string(),
            });
        }
        // INV-09 depends on this: the exact-base decision compares the
        // integration head against `base_sha` and then publishes `commit_sha`,
        // so a commit parented anywhere else would fast-forward the integration
        // ref onto history nobody judged.
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

    // --- task_candidate_created --------------------------------------------

    pub(super) fn check_candidate_created(
        &self,
        created: &TaskCandidateCreated,
    ) -> Result<(), FoldError> {
        const KIND: &str = "task_candidate_created";
        let candidate = &created.candidate;
        let task = self.task(KIND, candidate.key)?;
        let generation = self.open_generation(KIND, task, candidate.key, candidate.generation)?;
        // ST-06: a mismatched task_candidate_created.
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
