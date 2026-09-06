//! Extended notes: `docs/internals/topology/fold/check_integration.md`

use super::region::{ineligible_detail, ordinal};
use super::*;
use crate::topology::registry::Lineage;

const MERGE_VERIFICATION_STARTED: &str = "merge_verification_started";
const MERGE_VERIFICATION_UNAVAILABLE: &str = "merge_verification_unavailable";
const MERGE_VERIFICATION_INTERRUPTED: &str = "merge_verification_interrupted";
const MERGE_PREPARED: &str = "merge_prepared";
const MERGE_REJECTED: &str = "merge_rejected";
const TASK_MERGED: &str = "task_merged";

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
            let detail = match self.queue.get(candidate.key, candidate.generation) {
                None => "it holds no queue position at all".to_owned(),
                Some(entry) => match CandidateQueue::ineligible(
                    entry,
                    &|key| self.task_is_awaiting_input(key),
                    &self.leases,
                    &self.started.path_policy,
                ) {
                    Some(why) => format!("it is not eligible: {}", ineligible_detail(why)),
                    None => format!(
                        "task {} generation {} is queued ahead of it and eligible",
                        first.key().0,
                        first.generation().0
                    ),
                },
            };
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
        self.check_transaction_start(
            MERGE_VERIFICATION_STARTED,
            started.sequence,
            &started.candidate,
        )?;
        let prepared = self.prepared_candidate(MERGE_VERIFICATION_STARTED, &started.candidate)?;

        if started.expected_head == prepared.base_sha {
            return Err(FoldError::InconsistentRecord {
                kind: MERGE_VERIFICATION_STARTED,
                detail: format!(
                    "the head is {} and the candidate's base is the same commit, which is the \
                     exact-base case and publishes the candidate itself",
                    started.expected_head
                ),
            });
        }
        match &started.basis {
            VerificationBasis::AlreadyPresent => {
                if started.proposed_sha != started.expected_head {
                    return Err(FoldError::InconsistentRecord {
                        kind: MERGE_VERIFICATION_STARTED,
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
                        kind: MERGE_VERIFICATION_STARTED,
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
        let transaction =
            self.open_transaction(MERGE_VERIFICATION_UNAVAILABLE, unavailable.sequence)?;
        if !matches!(
            transaction.class,
            TransactionClass::VerificationStarted { .. }
        ) {
            return Err(FoldError::InconsistentRecord {
                kind: MERGE_VERIFICATION_UNAVAILABLE,
                detail: "the transaction is already authorized to publish; an outage refuses a \
                         verification that is still running"
                    .to_owned(),
            });
        }
        unavailable
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: MERGE_VERIFICATION_UNAVAILABLE,
                detail: defect.to_string(),
            })?;

        let queued = self
            .queue
            .get(transaction.candidate.key, transaction.candidate.generation)
            .ok_or_else(|| FoldError::InconsistentRecord {
                kind: MERGE_VERIFICATION_UNAVAILABLE,
                detail: "the candidate under verification holds no queue position".to_owned(),
            })?;
        if let UnavailableOutcome::Parked { question } = &unavailable.outcome {
            self.check_new_question(
                MERGE_VERIFICATION_UNAVAILABLE,
                question,
                transaction.candidate.key,
            )?;
        }
        check_defer_allowance(
            &unavailable.cause,
            &unavailable.outcome,
            queued.defers,
            self.started.limits.max_defers,
        )
    }

    pub(super) fn check_verification_interrupted(
        &self,
        interrupted: &MergeVerificationInterrupted,
    ) -> Result<(), FoldError> {
        let transaction =
            self.open_transaction(MERGE_VERIFICATION_INTERRUPTED, interrupted.sequence)?;
        if !matches!(
            transaction.class,
            TransactionClass::VerificationStarted { .. }
        ) {
            return Err(FoldError::InconsistentRecord {
                kind: MERGE_VERIFICATION_INTERRUPTED,
                detail: "the transaction is already authorized to publish; an authorized \
                         publication is completed, never abandoned"
                    .to_owned(),
            });
        }
        Ok(())
    }

    pub(super) fn check_merge_prepared(&self, prepared: &MergePrepared) -> Result<(), FoldError> {
        prepared
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: MERGE_PREPARED,
                detail: defect.to_string(),
            })?;

        let candidate_record = self.prepared_candidate(MERGE_PREPARED, &prepared.candidate())?;
        let inconsistent = |detail: String| FoldError::InconsistentRecord {
            kind: MERGE_PREPARED,
            detail,
        };

        if self.lineage_has_question(prepared.key) {
            return Err(inconsistent(format!(
                "task {} belongs to a lineage with an unanswered question",
                prepared.key
            )));
        }

        match prepared.disposition {
            PreparedDisposition::Fast => {
                self.check_transaction_start(
                    MERGE_PREPARED,
                    prepared.sequence,
                    &prepared.candidate(),
                )?;
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
                let transaction = self.open_transaction(MERGE_PREPARED, prepared.sequence)?;
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
                check_proposal_pin(basis, prepared.prepared_ref.as_ref())?;
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
                kind: MERGE_PREPARED,
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
        let inconsistent = |detail: String| FoldError::InconsistentRecord {
            kind: MERGE_REJECTED,
            detail,
        };
        match &rejected.disposition {
            RejectionDisposition::Conflict { .. } => {
                self.check_transaction_start(
                    MERGE_REJECTED,
                    rejected.sequence,
                    &rejected.candidate,
                )?;
            }
            RejectionDisposition::CodeRejected { verification } => {
                let transaction = self.open_transaction(MERGE_REJECTED, rejected.sequence)?;
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

        let entry = self.entry(MERGE_REJECTED, rejected.candidate.key)?;
        let root = rejection_lineage_root(
            &rejected.lease_effect,
            entry.lineage,
            rejected.candidate.key,
        )?;

        self.check_spawn(&rejected.repair, MERGE_REJECTED)?;
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
        let transaction = self.open_transaction(TASK_MERGED, merged.sequence)?;
        let TransactionClass::Prepared {
            proposed_sha,
            satisfies,
        } = &transaction.class
        else {
            return Err(FoldError::InconsistentRecord {
                kind: TASK_MERGED,
                detail: "the integration ref moves only after `merge_prepared`, and this \
                         transaction has not authorized a publication"
                    .to_owned(),
            });
        };
        if merged.merged_sha != *proposed_sha {
            return Err(FoldError::InconsistentRecord {
                kind: TASK_MERGED,
                detail: format!(
                    "the ref now points at {} and the authorization proposed {proposed_sha}",
                    merged.merged_sha
                ),
            });
        }
        if merged.satisfies != *satisfies {
            return Err(FoldError::InvalidSatisfies {
                kind: TASK_MERGED,
                recorded: merged.satisfies.iter().map(|key| key.0).collect(),
                derived: satisfies.iter().map(|key| key.0).collect(),
            });
        }
        let entry = self.entry(TASK_MERGED, transaction.candidate.key)?;
        check_lease_release(
            &merged.lease_release,
            entry.lineage.map(|lineage| lineage.root),
            &transaction.candidate,
        )
    }
}

fn check_defer_allowance(
    cause: &UnavailableCause,
    outcome: &UnavailableOutcome,
    taken: u32,
    max: u32,
) -> Result<(), FoldError> {
    let next = taken.saturating_add(1);
    match outcome {
        UnavailableOutcome::Deferred { defers } => {
            if *defers != next {
                return Err(FoldError::InvalidDefers {
                    defers: *defers,
                    detail: format!(
                        "this candidate has been deferred {taken} time(s), so the next deferral \
                         is {next}"
                    ),
                });
            }
            if *defers >= max {
                return Err(FoldError::InvalidDefers {
                    defers: *defers,
                    detail: format!(
                        "this run allows {max}, and the {max}th outage parks rather than defers"
                    ),
                });
            }
        }
        UnavailableOutcome::Parked { .. } => {
            if matches!(cause, UnavailableCause::Infrastructure { .. }) && next < max {
                return Err(FoldError::InvalidDefers {
                    defers: next,
                    detail: format!(
                        "an infrastructure outage parks at {max} deferral(s) and this candidate \
                         has been deferred {taken} time(s), so this one defers"
                    ),
                });
            }
        }
    }
    Ok(())
}

fn check_proposal_pin(basis: &VerificationBasis, pin: Option<&GitRef>) -> Result<(), FoldError> {
    match basis {
        VerificationBasis::StaleClean { prepared_ref } => {
            if pin != Some(prepared_ref) {
                return Err(FoldError::InconsistentRecord {
                    kind: MERGE_PREPARED,
                    detail: format!(
                        "it pins the proposal at {:?} and the verification pinned it at \
                         `{prepared_ref}`",
                        pin.map(GitRef::as_str)
                    ),
                });
            }
        }
        VerificationBasis::AlreadyPresent => {
            if let Some(pin) = pin {
                return Err(FoldError::InconsistentRecord {
                    kind: MERGE_PREPARED,
                    detail: format!(
                        "it pins the proposal at `{}` and an already-present publication \
                         manufactures no commit to pin",
                        pin.name()
                    ),
                });
            }
        }
    }
    Ok(())
}

fn rejection_lineage_root(
    effect: &RejectionLeaseEffect,
    lineage: Option<Lineage>,
    key: TaskKey,
) -> Result<TaskKey, FoldError> {
    let inconsistent = |detail: String| FoldError::InconsistentRecord {
        kind: MERGE_REJECTED,
        detail,
    };
    match (effect, lineage) {
        (RejectionLeaseEffect::CreatesLineage { root, .. }, None) => {
            if *root != key {
                return Err(inconsistent(format!(
                    "it creates lineage {root} from the rejection of task {}",
                    key.0
                )));
            }
            Ok(*root)
        }
        (RejectionLeaseEffect::WidensLineage { root, .. }, Some(lineage)) => {
            if *root != lineage.root {
                return Err(inconsistent(format!(
                    "it widens lineage {root} and the rejected task descends from {}",
                    lineage.root
                )));
            }
            Ok(*root)
        }
        (RejectionLeaseEffect::CreatesLineage { .. }, Some(_))
        | (RejectionLeaseEffect::WidensLineage { .. }, None) => Err(inconsistent(
            "a rejection creates a lineage from an ordinary candidate and widens the lineage of a \
             member; this does the other one"
                .to_owned(),
        )),
    }
}

fn check_lease_release(
    release: &MergeLeaseRelease,
    settled: Option<TaskKey>,
    candidate: &CandidateRef,
) -> Result<(), FoldError> {
    let inconsistent = |detail: String| FoldError::InconsistentRecord {
        kind: TASK_MERGED,
        detail,
    };
    match (release, settled) {
        (MergeLeaseRelease::Candidate { key, generation }, None) => {
            if *key != candidate.key || *generation != candidate.generation {
                return Err(inconsistent(format!(
                    "it releases the lease of task {} generation {} and publishes task {} \
                     generation {}",
                    key.0, generation.0, candidate.key.0, candidate.generation.0
                )));
            }
        }
        (MergeLeaseRelease::Lineage { root }, Some(settled)) => {
            if *root != settled {
                return Err(inconsistent(format!(
                    "it releases lineage {root} and settles lineage {settled}"
                )));
            }
        }
        (MergeLeaseRelease::Candidate { .. }, Some(_))
        | (MergeLeaseRelease::Lineage { .. }, None) => {
            return Err(inconsistent(
                "a publication releases the candidate's lease, or the lineage lease when it \
                 settles that lineage's root; this releases the other one"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::QuestionKind;
    use crate::topology::events::InfrastructureKind;

    const ROOT: TaskKey = TaskKey(1);
    const CANDIDATE: TaskKey = TaskKey(2);
    const ELSEWHERE: TaskKey = TaskKey(7);

    const MISMATCHED_REJECTION: &str = "a rejection creates a lineage from an ordinary candidate \
                                        and widens the lineage of a member; this does the other \
                                        one";
    const MISMATCHED_RELEASE: &str = "a publication releases the candidate's lease, or the \
                                      lineage lease when it settles that lineage's root; this \
                                      releases the other one";

    fn region() -> PathSet {
        PathSet::Prefixes {
            paths: vec![GitPath("src/alpha".to_owned())],
        }
    }

    fn refusal<T>(result: &Result<T, FoldError>) -> Option<(&str, &str)> {
        match result {
            Err(FoldError::InconsistentRecord { kind, detail }) => Some((kind, detail.as_str())),
            _ => None,
        }
    }

    fn invalid_defers(result: &Result<(), FoldError>) -> Option<(u32, &str)> {
        match result {
            Err(FoldError::InvalidDefers { defers, detail }) => Some((*defers, detail.as_str())),
            _ => None,
        }
    }

    fn member(root: TaskKey) -> Option<Lineage> {
        Some(Lineage {
            root,
            parent: root,
            index: 0,
        })
    }

    fn candidate_ref() -> CandidateRef {
        CandidateRef {
            key: CANDIDATE,
            generation: GenerationId(3),
            commit_sha: CommitSha("c".repeat(40)),
            candidate_ref: GitRef("refs/upstroke/runs/run/candidates/2/3".to_owned()),
        }
    }

    fn parked() -> UnavailableOutcome {
        UnavailableOutcome::Parked {
            question: FrozenQuestion {
                id: QuestionId::from("q-outage"),
                key: CANDIDATE,
                kind: QuestionKind::Unblock,
                context: "the reviewer has been unavailable".to_owned(),
                options: vec!["wait".to_owned(), "abandon".to_owned()],
            },
        }
    }

    fn infrastructure() -> UnavailableCause {
        UnavailableCause::Infrastructure {
            kind: InfrastructureKind::ReviewerTimeout,
        }
    }

    fn human_required() -> UnavailableCause {
        UnavailableCause::HumanRequired {
            verdict: "a person decides".to_owned(),
        }
    }

    #[test]
    fn a_rejection_creates_a_lineage_from_an_ordinary_candidate_and_widens_a_members() {
        let creates = |root| RejectionLeaseEffect::CreatesLineage {
            root,
            paths: region(),
        };
        let widens = |root| RejectionLeaseEffect::WidensLineage {
            root,
            paths: region(),
        };

        assert_eq!(
            rejection_lineage_root(&creates(CANDIDATE), None, CANDIDATE),
            Ok(CANDIDATE),
            "an ordinary candidate roots the lineage its own rejection creates"
        );
        assert_eq!(
            refusal(&rejection_lineage_root(
                &creates(ELSEWHERE),
                None,
                CANDIDATE
            )),
            Some((
                MERGE_REJECTED,
                "it creates lineage 7 from the rejection of task 2"
            ))
        );

        assert_eq!(
            rejection_lineage_root(&widens(ROOT), member(ROOT), CANDIDATE),
            Ok(ROOT),
            "a member widens the lineage it descends from"
        );
        assert_eq!(
            refusal(&rejection_lineage_root(
                &widens(ELSEWHERE),
                member(ROOT),
                CANDIDATE
            )),
            Some((
                MERGE_REJECTED,
                "it widens lineage 7 and the rejected task descends from 1"
            ))
        );

        assert_eq!(
            refusal(&rejection_lineage_root(
                &creates(CANDIDATE),
                member(ROOT),
                CANDIDATE
            )),
            Some((MERGE_REJECTED, MISMATCHED_REJECTION)),
            "a member's rejection may not create a second lineage"
        );
        assert_eq!(
            refusal(&rejection_lineage_root(&widens(ROOT), None, CANDIDATE)),
            Some((MERGE_REJECTED, MISMATCHED_REJECTION)),
            "an ordinary candidate's rejection widens no lineage"
        );
    }

    #[test]
    fn a_publication_releases_the_lease_its_own_candidate_or_its_lineage_holds() {
        let candidate = candidate_ref();
        let own = MergeLeaseRelease::Candidate {
            key: CANDIDATE,
            generation: GenerationId(3),
        };

        assert_eq!(check_lease_release(&own, None, &candidate), Ok(()));
        assert_eq!(
            refusal(&check_lease_release(
                &MergeLeaseRelease::Candidate {
                    key: ELSEWHERE,
                    generation: GenerationId(3),
                },
                None,
                &candidate,
            )),
            Some((
                TASK_MERGED,
                "it releases the lease of task 7 generation 3 and publishes task 2 generation 3"
            ))
        );
        assert_eq!(
            refusal(&check_lease_release(
                &MergeLeaseRelease::Candidate {
                    key: CANDIDATE,
                    generation: GenerationId(4),
                },
                None,
                &candidate,
            )),
            Some((
                TASK_MERGED,
                "it releases the lease of task 2 generation 4 and publishes task 2 generation 3"
            ))
        );

        assert_eq!(
            check_lease_release(
                &MergeLeaseRelease::Lineage { root: ROOT },
                Some(ROOT),
                &candidate,
            ),
            Ok(())
        );
        assert_eq!(
            refusal(&check_lease_release(
                &MergeLeaseRelease::Lineage { root: ELSEWHERE },
                Some(ROOT),
                &candidate,
            )),
            Some((TASK_MERGED, "it releases lineage 7 and settles lineage 1"))
        );

        assert_eq!(
            refusal(&check_lease_release(&own, Some(ROOT), &candidate)),
            Some((TASK_MERGED, MISMATCHED_RELEASE)),
            "settling a lineage releases the lineage lease, not the candidate's"
        );
        assert_eq!(
            refusal(&check_lease_release(
                &MergeLeaseRelease::Lineage { root: ROOT },
                None,
                &candidate,
            )),
            Some((TASK_MERGED, MISMATCHED_RELEASE)),
            "an ordinary candidate holds no lineage lease to release"
        );
    }

    #[test]
    fn only_a_stale_clean_publication_pins_a_proposal() {
        const PIN: &str = "refs/upstroke/runs/run/prepared/0";
        let pinned = GitRef(PIN.to_owned());
        let other = GitRef("refs/upstroke/runs/run/prepared/9".to_owned());
        let stale = VerificationBasis::StaleClean {
            prepared_ref: GitRef(PIN.to_owned()),
        };

        assert_eq!(check_proposal_pin(&stale, Some(&pinned)), Ok(()));
        assert_eq!(
            refusal(&check_proposal_pin(&stale, None)),
            Some((
                MERGE_PREPARED,
                "it pins the proposal at None and the verification pinned it at \
                 `refs/upstroke/runs/run/prepared/0`"
            ))
        );
        assert_eq!(
            refusal(&check_proposal_pin(&stale, Some(&other))),
            Some((
                MERGE_PREPARED,
                "it pins the proposal at Some(\"refs/upstroke/runs/run/prepared/9\") and the \
                 verification pinned it at `refs/upstroke/runs/run/prepared/0`"
            ))
        );

        assert_eq!(
            check_proposal_pin(&VerificationBasis::AlreadyPresent, None),
            Ok(())
        );
        assert_eq!(
            refusal(&check_proposal_pin(
                &VerificationBasis::AlreadyPresent,
                Some(&pinned)
            )),
            Some((
                MERGE_PREPARED,
                "it pins the proposal at `refs/upstroke/runs/run/prepared/0` and an \
                 already-present publication manufactures no commit to pin"
            )),
            "an already-present publication manufactures no commit, so there is nothing to pin"
        );
    }

    #[test]
    fn an_outage_defers_consecutively_inside_the_allowance_and_parks_at_it() {
        let deferred = |defers| UnavailableOutcome::Deferred { defers };

        assert_eq!(
            check_defer_allowance(&infrastructure(), &deferred(1), 0, 2),
            Ok(())
        );
        for count in [0, 2, 3, 9] {
            assert_eq!(
                invalid_defers(&check_defer_allowance(
                    &infrastructure(),
                    &deferred(count),
                    0,
                    2
                )),
                Some((
                    count,
                    "this candidate has been deferred 0 time(s), so the next deferral is 1"
                )),
                "a deferral counted {count} where the candidate has 0 was accepted"
            );
        }
        assert_eq!(
            invalid_defers(&check_defer_allowance(&infrastructure(), &parked(), 0, 2)),
            Some((
                1,
                "an infrastructure outage parks at 2 deferral(s) and this candidate has been \
                 deferred 0 time(s), so this one defers"
            )),
            "parking one deferral early spends an allowance the run still has"
        );

        assert_eq!(
            invalid_defers(&check_defer_allowance(
                &infrastructure(),
                &deferred(2),
                1,
                2
            )),
            Some((
                2,
                "this run allows 2, and the 2th outage parks rather than defers"
            ))
        );
        assert_eq!(
            check_defer_allowance(&infrastructure(), &parked(), 1, 2),
            Ok(())
        );

        assert_eq!(
            check_defer_allowance(&human_required(), &parked(), 0, 2),
            Ok(()),
            "a human-required outage parks whatever the count"
        );
    }

    #[test]
    fn a_run_that_allows_no_deferral_parks_the_first_outage() {
        assert_eq!(
            invalid_defers(&check_defer_allowance(
                &infrastructure(),
                &UnavailableOutcome::Deferred { defers: 1 },
                0,
                0
            )),
            Some((
                1,
                "this run allows 0, and the 0th outage parks rather than defers"
            )),
            "a run whose allowance is zero takes no deferral"
        );
        assert_eq!(
            check_defer_allowance(&infrastructure(), &parked(), 0, 0),
            Ok(()),
            "and the outage it cannot defer parks instead of being refused outright"
        );
    }
}
