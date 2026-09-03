//! The integration checks: opening a transaction, the verification records,
//! and the publication relations a merge is judged against (INV-09).

use super::region::{GitRefName, ineligible_detail, ordinal};
use super::*;

impl RunState {
    // --- integration: starting a transaction --------------------------------

    /// The checks every first append of an integration transaction shares:
    /// nothing else is open, the sequence is the next dense one, and the
    /// candidate is the first *eligible* entry in the queue.
    pub(super) fn check_transaction_start(
        &self,
        kind: &'static str,
        sequence: SequenceId,
        candidate: &CandidateRef,
    ) -> Result<&QueueEntry, FoldError> {
        // refusals[7]: one integration transaction at a time.
        if let Some(open) = &self.transaction {
            return Err(FoldError::TransactionAlreadyOpen {
                kind,
                sequence: sequence.0,
                open: open.sequence.0,
            });
        }
        // refusals[6] / refusals[10]: sequences are dense from 0 across the run.
        if sequence.0 != self.next_sequence {
            return Err(FoldError::NonDenseSequence {
                kind,
                sequence: sequence.0,
                next: self.next_sequence,
            });
        }
        // refusals[8]: the first eligible entry is integrated, and the fold
        // refuses an integration start for any other candidate.
        let first = self
            .queue
            .first_eligible(
                |key| self.task_is_awaiting_input(key),
                &self.leases,
                &self.started.path_policy,
            )
            .ok_or_else(|| FoldError::NotFirstEligible {
                kind,
                key: candidate.key.0,
                generation: candidate.generation.0,
                detail: "no queued candidate is eligible".to_owned(),
            })?;
        if first.candidate != *candidate {
            let detail = self
                .queue
                .get(candidate.key, candidate.generation)
                .map_or_else(
                    || "it holds no queue position at all".to_owned(),
                    |entry| {
                        CandidateQueue::ineligible(
                            entry,
                            &|key| self.task_is_awaiting_input(key),
                            &self.leases,
                            &self.started.path_policy,
                        )
                        .map_or_else(
                            || {
                                format!(
                                    "task {} generation {} is queued ahead of it and eligible",
                                    first.key().0,
                                    first.generation().0
                                )
                            },
                            |why| format!("it is not eligible: {}", ineligible_detail(why)),
                        )
                    },
                );
            return Err(FoldError::NotFirstEligible {
                kind,
                key: candidate.key.0,
                generation: candidate.generation.0,
                detail,
            });
        }
        Ok(first)
    }

    pub(super) fn task_is_awaiting_input(&self, key: TaskKey) -> bool {
        self.tasks
            .get(key.index())
            .is_some_and(|task| task.state == TaskState::AwaitingInput)
    }

    /// The open transaction this event must belong to (refusals[6]).
    pub(super) fn open_transaction(
        &self,
        kind: &'static str,
        sequence: SequenceId,
    ) -> Result<&Transaction, FoldError> {
        let open = self
            .transaction
            .as_ref()
            .ok_or_else(|| FoldError::WrongSequence {
                kind,
                sequence: sequence.0,
                open: "none".to_owned(),
            })?;
        if open.sequence != sequence {
            return Err(FoldError::WrongSequence {
                kind,
                sequence: sequence.0,
                open: open.sequence.0.to_string(),
            });
        }
        Ok(open)
    }

    // --- merge_verification_started ----------------------------------------

    pub(super) fn check_verification_started(
        &self,
        started: &MergeVerificationStarted,
    ) -> Result<(), FoldError> {
        const KIND: &str = "merge_verification_started";
        let queued = self.check_transaction_start(KIND, started.sequence, &started.candidate)?;
        let prepared = self.prepared_candidate(KIND, &started.candidate)?;

        // INV-09: the exact-base decision is made before any staging effect, so
        // a candidate whose base *is* the head is published fast and is never
        // cherry-picked or re-verified.
        if started.expected_head == prepared.base_sha {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "the head is {} and the candidate's base is the same commit, which is the \
                     exact-base case and publishes the candidate itself",
                    started.expected_head
                ),
            });
        }
        let _ = queued;
        match &started.basis {
            VerificationBasis::AlreadyPresent => {
                if started.proposed_sha != started.expected_head {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "an already-present verification judges the head itself, and this one \
                             judges {} against head {}",
                            started.proposed_sha, started.expected_head
                        ),
                    });
                }
            }
            VerificationBasis::StaleClean { .. } => {
                if started.proposed_sha == started.expected_head {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: "a stale-clean verification judges the proposal the cherry-pick \
                                 produced, and this one judges the head"
                            .to_owned(),
                    });
                }
            }
        }
        Ok(())
    }

    /// What `candidate_prepared` recorded for this candidate.
    pub(super) fn prepared_candidate(
        &self,
        kind: &'static str,
        candidate: &CandidateRef,
    ) -> Result<&PreparedCandidate, FoldError> {
        let task = self.task(kind, candidate.key)?;
        task.generations
            .iter()
            .filter_map(|generation| generation.candidate.as_ref())
            .find(|prepared| prepared.candidate.generation == candidate.generation)
            .filter(|prepared| prepared.candidate == *candidate)
            .ok_or_else(|| FoldError::InconsistentRecord {
                kind,
                detail: format!(
                    "no `candidate_prepared` in this log records task {} generation {} as commit \
                     {}",
                    candidate.key.0, candidate.generation.0, candidate.commit_sha
                ),
            })
    }

    // --- merge_verification_unavailable ------------------------------------

    pub(super) fn check_verification_unavailable(
        &self,
        unavailable: &MergeVerificationUnavailable,
    ) -> Result<(), FoldError> {
        const KIND: &str = "merge_verification_unavailable";
        let transaction = self.open_transaction(KIND, unavailable.sequence)?;
        if !matches!(
            transaction.class,
            TransactionClass::VerificationStarted { .. }
        ) {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: "the transaction is already authorized to publish; an outage refuses a \
                         verification that is still running"
                    .to_owned(),
            });
        }
        unavailable
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: KIND,
                detail: defect.to_string(),
            })?;

        let queued = self
            .queue
            .get(transaction.candidate.key, transaction.candidate.generation)
            .ok_or_else(|| FoldError::InconsistentRecord {
                kind: KIND,
                detail: "the candidate under verification holds no queue position".to_owned(),
            })?;
        // The boundary is the same number read from both sides: the deferral
        // this outage *would* be. `coordinator_integration.dispositions` gives
        // Infrastructure `Deferred{defers}` while `defers < max_defers` and
        // `Parked{question}` at `max_defers`, so the two arms partition on
        // `next` and neither may take the other's cell.
        let max = self.started.limits.max_defers;
        let next = queued.defers.saturating_add(1);
        match &unavailable.outcome {
            UnavailableOutcome::Deferred { defers } => {
                // refusals[17]: consecutive, and within the frozen allowance.
                if *defers != next {
                    return Err(FoldError::InvalidDefers {
                        defers: *defers,
                        detail: format!(
                            "this candidate has been deferred {} time(s), so the next deferral is \
                             {next}",
                            queued.defers,
                        ),
                    });
                }
                // refusals[16]: "Deferred at max_defers" is refused. The
                // allowance is the number of deferrals the run may *take*, so
                // the last one it may take is `max_defers - 1` and the outage
                // that would be the `max_defers`th parks instead.
                if *defers >= max {
                    return Err(FoldError::InvalidDefers {
                        defers: *defers,
                        detail: format!(
                            "this run allows {max}, and the {max}th outage parks rather than \
                             defers"
                        ),
                    });
                }
            }
            UnavailableOutcome::Parked { question } => {
                self.check_new_question(KIND, question, transaction.candidate.key)?;
                // refusals[16], the other half: `HumanRequired` always parks,
                // whatever the count, and an Infrastructure outage parks
                // exactly at the boundary — one earlier would consume an
                // allowance the run still has.
                if matches!(unavailable.cause, UnavailableCause::Infrastructure { .. })
                    && next != max
                {
                    return Err(FoldError::InvalidDefers {
                        defers: next,
                        detail: format!(
                            "an infrastructure outage parks at {max} deferral(s) and this \
                             candidate has been deferred {} time(s), so this one defers",
                            queued.defers
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    // --- merge_verification_interrupted ------------------------------------

    pub(super) fn check_verification_interrupted(
        &self,
        interrupted: &MergeVerificationInterrupted,
    ) -> Result<(), FoldError> {
        const KIND: &str = "merge_verification_interrupted";
        let transaction = self.open_transaction(KIND, interrupted.sequence)?;
        if !matches!(
            transaction.class,
            TransactionClass::VerificationStarted { .. }
        ) {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: "the transaction is already authorized to publish; an authorized \
                         publication is completed, never abandoned"
                    .to_owned(),
            });
        }
        Ok(())
    }

    // --- merge_prepared ----------------------------------------------------

    pub(super) fn check_merge_prepared(&self, prepared: &MergePrepared) -> Result<(), FoldError> {
        const KIND: &str = "merge_prepared";
        // A1's intra-event relations first: a record that disagrees with itself
        // is refused before it is compared with anything else.
        prepared
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: KIND,
                detail: defect.to_string(),
            })?;

        let candidate_record = self.prepared_candidate(KIND, &prepared.candidate())?;
        let inconsistent = |detail: String| FoldError::InconsistentRecord { kind: KIND, detail };

        match prepared.disposition {
            PreparedDisposition::Fast => {
                // A fast publication opens and closes its own transaction: no
                // verification ran, so there is nothing already open.
                self.check_transaction_start(KIND, prepared.sequence, &prepared.candidate())?;
                // refusals[9]: expected_head == the candidate's recorded base,
                // proposed_sha == the candidate's recorded commit.
                if prepared.expected_head != candidate_record.base_sha {
                    return Err(inconsistent(format!(
                        "a fast publication expects the head to be the candidate's base {} and \
                         this one expects {}",
                        candidate_record.base_sha, prepared.expected_head
                    )));
                }
                if prepared.proposed_sha != candidate_record.candidate.commit_sha {
                    return Err(inconsistent(format!(
                        "it publishes {} and the candidate's recorded commit is {}",
                        prepared.proposed_sha, candidate_record.candidate.commit_sha
                    )));
                }
                match &prepared.verification_source {
                    VerificationSource::CandidatePrepared { key, generation } => {
                        if *key != prepared.key || *generation != prepared.generation {
                            return Err(inconsistent(format!(
                                "it cites the record of task {} generation {} and publishes task \
                                 {} generation {}",
                                key.0, generation.0, prepared.key.0, prepared.generation.0
                            )));
                        }
                    }
                    VerificationSource::Verification { .. } => {
                        return Err(inconsistent(
                            "a fast publication cites the candidate's own record".to_owned(),
                        ));
                    }
                }
            }
            PreparedDisposition::StaleClean | PreparedDisposition::AlreadyPresent => {
                let transaction = self.open_transaction(KIND, prepared.sequence)?;
                let TransactionClass::VerificationStarted {
                    basis,
                    expected_head,
                    proposed_sha,
                } = &transaction.class
                else {
                    return Err(inconsistent(
                        "the transaction is already authorized to publish".to_owned(),
                    ));
                };
                if transaction.candidate != prepared.candidate() {
                    return Err(inconsistent(format!(
                        "it publishes task {} generation {} and the open transaction is verifying \
                         task {} generation {}",
                        prepared.key.0,
                        prepared.generation.0,
                        transaction.candidate.key.0,
                        transaction.candidate.generation.0
                    )));
                }
                let stale = prepared.disposition == PreparedDisposition::StaleClean;
                if stale != matches!(basis, VerificationBasis::StaleClean { .. }) {
                    return Err(inconsistent(
                        "the disposition it publishes under is not the basis its verification ran \
                         on"
                        .to_owned(),
                    ));
                }
                // refusals[22], fold half: the head the CAS expects is the head
                // the transaction read.
                if prepared.expected_head != *expected_head {
                    return Err(inconsistent(format!(
                        "it expects head {} and the verification recorded head {expected_head}",
                        prepared.expected_head
                    )));
                }
                // refusals[9]: the proposal is the one that was verified — the
                // pinned proposal for a stale publication, the head itself for
                // an already-present one.
                if prepared.proposed_sha != *proposed_sha {
                    return Err(inconsistent(format!(
                        "it publishes {} and the verification judged {proposed_sha}",
                        prepared.proposed_sha
                    )));
                }
                if let VerificationBasis::StaleClean { prepared_ref } = basis {
                    if prepared.prepared_ref.as_ref() != Some(prepared_ref) {
                        return Err(inconsistent(format!(
                            "it pins the proposal at {:?} and the verification pinned it at `{}`",
                            prepared.prepared_ref.as_ref().map(GitRefName::name),
                            prepared_ref
                        )));
                    }
                }
                match &prepared.verification_source {
                    VerificationSource::Verification { sequence } => {
                        if *sequence != prepared.sequence {
                            return Err(inconsistent(format!(
                                "it cites verification {} and belongs to transaction {}",
                                sequence.0, prepared.sequence.0
                            )));
                        }
                    }
                    VerificationSource::CandidatePrepared { .. } => {
                        return Err(inconsistent(
                            "a verified publication cites the verification that judged what it \
                             publishes"
                                .to_owned(),
                        ));
                    }
                }
            }
        }

        // refusals[10]: the closure this publication settles is derived, not
        // asserted.
        let derived = self.satisfies_closure(prepared.key);
        if prepared.satisfies != derived {
            return Err(FoldError::InvalidSatisfies {
                kind: KIND,
                recorded: prepared.satisfies.iter().map(|key| key.0).collect(),
                derived: derived.iter().map(|key| key.0).collect(),
            });
        }
        Ok(())
    }

    /// Every task one publication settles: the candidate's own task and, for a
    /// repair, every entry back up its lineage to the root.
    ///
    /// A repair carries the work of everything it descends from — that is what
    /// it was materialized from — so publishing it settles the whole chain.
    /// Ascending key order, because the value is derived and two readers must
    /// derive the same list.
    pub(super) fn satisfies_closure(&self, key: TaskKey) -> Vec<TaskKey> {
        let mut chain = vec![key];
        let mut current = key;
        while let Some(lineage) = self.registry.get(current).and_then(|entry| entry.lineage) {
            if lineage.parent >= current {
                break;
            }
            chain.push(lineage.parent);
            current = lineage.parent;
        }
        chain.sort_unstable();
        chain.dedup();
        chain
    }

    // --- merge_rejected ----------------------------------------------------

    pub(super) fn check_merge_rejected(&self, rejected: &MergeRejected) -> Result<(), FoldError> {
        const KIND: &str = "merge_rejected";
        let inconsistent = |detail: String| FoldError::InconsistentRecord { kind: KIND, detail };
        match &rejected.disposition {
            RejectionDisposition::Conflict { .. } => {
                // A conflict is decided at the cherry-pick, before any
                // verification starts: it opens and closes its own transaction.
                self.check_transaction_start(KIND, rejected.sequence, &rejected.candidate)?;
            }
            RejectionDisposition::CodeRejected { verification } => {
                let transaction = self.open_transaction(KIND, rejected.sequence)?;
                let TransactionClass::VerificationStarted { expected_head, .. } =
                    &transaction.class
                else {
                    return Err(inconsistent(
                        "the transaction is already authorized to publish".to_owned(),
                    ));
                };
                if transaction.candidate != rejected.candidate {
                    return Err(inconsistent(format!(
                        "it rejects task {} generation {} and the open transaction is verifying \
                         task {} generation {}",
                        rejected.candidate.key.0,
                        rejected.candidate.generation.0,
                        transaction.candidate.key.0,
                        transaction.candidate.generation.0
                    )));
                }
                if rejected.rejecting_head != *expected_head {
                    return Err(inconsistent(format!(
                        "it was judged against head {} and the verification recorded head \
                         {expected_head}",
                        rejected.rejecting_head
                    )));
                }
                if verification.verdict == VerificationVerdict::Passed {
                    return Err(inconsistent(
                        "a code rejection carries the verification that rejected it, and this one \
                         passed"
                            .to_owned(),
                    ));
                }
            }
        }

        // The lease effect and the repair are one decision: a non-lineage
        // candidate's lease becomes the new lineage's, and a lineage member's
        // rejection widens the lineage it already belongs to.
        let entry = self.entry(KIND, rejected.candidate.key)?;
        let root = match (&rejected.lease_effect, entry.lineage) {
            (RejectionLeaseEffect::CreatesLineage { root, .. }, None) => {
                if *root != rejected.candidate.key {
                    return Err(inconsistent(format!(
                        "it creates lineage {root} from the rejection of task {}",
                        rejected.candidate.key.0
                    )));
                }
                *root
            }
            (RejectionLeaseEffect::WidensLineage { root, .. }, Some(lineage)) => {
                if *root != lineage.root {
                    return Err(inconsistent(format!(
                        "it widens lineage {root} and the rejected task descends from {}",
                        lineage.root
                    )));
                }
                *root
            }
            _ => {
                return Err(inconsistent(
                    "a rejection creates a lineage from an ordinary candidate and widens the \
                     lineage of a member; this does the other one"
                        .to_owned(),
                ));
            }
        };

        self.check_spawn(&rejected.repair, KIND)?;
        let lineage =
            rejected.repair.entry.lineage.ok_or_else(|| {
                inconsistent("the repair it registers records no lineage".to_owned())
            })?;
        if lineage.root != root {
            return Err(inconsistent(format!(
                "the repair descends from lineage {} and the rejection widens {root}",
                lineage.root
            )));
        }
        if lineage.parent != rejected.candidate.key {
            return Err(inconsistent(format!(
                "the repair's parent is {} and the rejected candidate is task {}",
                lineage.parent, rejected.candidate.key.0
            )));
        }
        let index = self.lineage_members(root);
        if lineage.index != index {
            return Err(inconsistent(format!(
                "the repair is the {} member of lineage {root} and records index {}",
                ordinal(index),
                lineage.index
            )));
        }
        Ok(())
    }

    /// How many repairs lineage `root` already holds.
    pub(super) fn lineage_members(&self, root: TaskKey) -> u32 {
        u32::try_from(
            self.registry
                .entries()
                .iter()
                .filter(|entry| entry.lineage.is_some_and(|lineage| lineage.root == root))
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    // --- task_merged -------------------------------------------------------

    pub(super) fn check_task_merged(&self, merged: &TaskMerged) -> Result<(), FoldError> {
        const KIND: &str = "task_merged";
        let transaction = self.open_transaction(KIND, merged.sequence)?;
        let TransactionClass::Prepared {
            proposed_sha,
            satisfies,
        } = &transaction.class
        else {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: "the integration ref moves only after `merge_prepared`, and this \
                         transaction has not authorized a publication"
                    .to_owned(),
            });
        };
        if merged.merged_sha != *proposed_sha {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "the ref now points at {} and the authorization proposed {proposed_sha}",
                    merged.merged_sha
                ),
            });
        }
        // "copied exactly from the authorization", not re-derived here.
        if merged.satisfies != *satisfies {
            return Err(FoldError::InvalidSatisfies {
                kind: KIND,
                recorded: merged.satisfies.iter().map(|key| key.0).collect(),
                derived: satisfies.iter().map(|key| key.0).collect(),
            });
        }
        let root_settled = self
            .registry
            .get(transaction.candidate.key)
            .and_then(|entry| entry.lineage)
            .map(|lineage| lineage.root);
        match (&merged.lease_release, root_settled) {
            (MergeLeaseRelease::Candidate { key, generation }, None) => {
                if *key != transaction.candidate.key
                    || *generation != transaction.candidate.generation
                {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!(
                            "it releases the lease of task {} generation {} and publishes task {} \
                             generation {}",
                            key.0,
                            generation.0,
                            transaction.candidate.key.0,
                            transaction.candidate.generation.0
                        ),
                    });
                }
            }
            (MergeLeaseRelease::Lineage { root }, Some(settled)) => {
                if *root != settled {
                    return Err(FoldError::InconsistentRecord {
                        kind: KIND,
                        detail: format!("it releases lineage {root} and settles lineage {settled}"),
                    });
                }
            }
            _ => {
                return Err(FoldError::InconsistentRecord {
                    kind: KIND,
                    detail: "a publication releases the candidate's lease, or the lineage lease \
                             when it settles that lineage's root; this releases the other one"
                        .to_owned(),
                });
            }
        }
        Ok(())
    }
}
