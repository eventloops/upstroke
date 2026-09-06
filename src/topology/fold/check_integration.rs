//! Extended notes: `docs/internals/topology/fold/check_integration.md`

use super::region::{ineligible_detail, ordinal};
use super::*;

impl RunState {
    pub(super) fn check_transaction_start(
        &self,
        kind: &'static str,
        sequence: SequenceId,
        candidate: &CandidateRef,
    ) -> Result<&QueueEntry, FoldError> {
        if let Some(open) = &self.transaction {
            return Err(FoldError::TransactionAlreadyOpen {
                kind,
                sequence: sequence.0,
                open: open.sequence.0,
            });
        }
        if sequence.0 != self.next_sequence {
            return Err(FoldError::NonDenseSequence {
                kind,
                sequence: sequence.0,
                next: self.next_sequence,
            });
        }
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
        self.lineage_has_question(key)
            || self
                .tasks
                .get(key.index())
                .is_some_and(|task| task.state == TaskState::AwaitingInput)
    }

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

    pub(super) fn check_verification_started(
        &self,
        started: &MergeVerificationStarted,
    ) -> Result<(), FoldError> {
        const KIND: &str = "merge_verification_started";
        let queued = self.check_transaction_start(KIND, started.sequence, &started.candidate)?;
        let prepared = self.prepared_candidate(KIND, &started.candidate)?;

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
        let max = self.started.limits.max_defers;
        let next = queued.defers.saturating_add(1);
        match &unavailable.outcome {
            UnavailableOutcome::Deferred { defers } => {
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

    pub(super) fn check_merge_prepared(&self, prepared: &MergePrepared) -> Result<(), FoldError> {
        const KIND: &str = "merge_prepared";
        prepared
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: KIND,
                detail: defect.to_string(),
            })?;

        let candidate_record = self.prepared_candidate(KIND, &prepared.candidate())?;
        let inconsistent = |detail: String| FoldError::InconsistentRecord { kind: KIND, detail };

        if self.lineage_has_question(prepared.key) {
            return Err(inconsistent(format!(
                "task {} belongs to a lineage with an unanswered question",
                prepared.key
            )));
        }

        match prepared.disposition {
            PreparedDisposition::Fast => {
                self.check_transaction_start(KIND, prepared.sequence, &prepared.candidate())?;
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
                if prepared.expected_head != *expected_head {
                    return Err(inconsistent(format!(
                        "it expects head {} and the verification recorded head {expected_head}",
                        prepared.expected_head
                    )));
                }
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
                            prepared.prepared_ref.as_ref().map(GitRef::as_str),
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

    pub(super) fn check_merge_rejected(&self, rejected: &MergeRejected) -> Result<(), FoldError> {
        const KIND: &str = "merge_rejected";
        let inconsistent = |detail: String| FoldError::InconsistentRecord { kind: KIND, detail };
        match &rejected.disposition {
            RejectionDisposition::Conflict { .. } => {
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
