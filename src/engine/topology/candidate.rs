//! Extended notes: `docs/internals/engine/topology/candidate.md`

use std::path::Path;

use thiserror::Error;

use super::seams::TopologyHooks;
use crate::error::UpstrokeError;
use crate::events::AttemptRecord;
use crate::topology::effects::{EffectSiteId, ObjectResidue, ObjectSite, RefSite};
use crate::topology::events::{
    CandidateLeaseEffect, CandidatePrepared, CandidateRef, CommitSha, GenerationId, GitRef,
    TaskCandidateCreated, TopologyEventBody,
};
use crate::topology::fold::{GenerationClass, TopologyFold};
use crate::topology::paths::PathSet;
use crate::topology::registry::TaskKey;
use crate::workspace_manager::{
    ResidueTarget, Slot, WorkspaceManager, classify_object_residue, is_object_id,
};

pub const RUN_REF_ROOT: &str = "refs/upstroke/runs";

#[must_use]
pub fn run_namespace(run_id: &str) -> String {
    format!("{RUN_REF_ROOT}/{run_id}/")
}

#[must_use]
pub fn candidate_pin_ref(run_id: &str, key: TaskKey, generation: GenerationId) -> GitRef {
    GitRef(format!(
        "{RUN_REF_ROOT}/{run_id}/candidate-prepared/{}/{}",
        key.0, generation.0
    ))
}

#[must_use]
pub fn candidates_ref(run_id: &str, key: TaskKey, generation: GenerationId) -> GitRef {
    GitRef(format!(
        "{RUN_REF_ROOT}/{run_id}/candidates/{}/{}",
        key.0, generation.0
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateNames {
    pub prepared_ref: GitRef,
    pub candidate_ref: GitRef,
}

impl CandidateNames {
    #[must_use]
    pub fn of(run_id: &str, key: TaskKey, generation: GenerationId) -> Self {
        Self {
            prepared_ref: candidate_pin_ref(run_id, key, generation),
            candidate_ref: candidates_ref(run_id, key, generation),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Refusal {
    #[error(
        "refusing to promote `{key}`/`{generation}`: its candidate commit {commit} is not an \
         object in this repository, and a candidates ref may only be created for the exact commit \
         `candidate_prepared` recorded"
    )]
    ObjectMissing {
        key: u32,
        generation: u32,
        commit: String,
    },

    #[error(
        "refusing to promote `{refname}`: it is present at {found} and `candidate_prepared` \
         recorded {expected}. The candidates ref is authoritative (R11) and is never moved, \
         and the prepared pin binds to the recorded commit; neither is removed on a \
         mismatch, so the substitution stays visible"
    )]
    RefAtAnotherSha {
        refname: String,
        found: String,
        expected: String,
    },

    #[error(
        "refusing `{value}` as the candidate commit of `{key}`/`{generation}`: the sequence takes \
         a full hexadecimal object id"
    )]
    MalformedCommit {
        key: u32,
        generation: u32,
        value: String,
    },
}

impl From<Refusal> for UpstrokeError {
    fn from(refusal: Refusal) -> Self {
        Self::Refused {
            message: refusal.to_string(),
        }
    }
}

pub trait CandidateJournal {
    fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError>;

    fn fold(&self) -> &TopologyFold;
}

#[derive(Debug, Clone, PartialEq)]
pub struct JudgedTree {
    pub key: TaskKey,
    pub generation: GenerationId,
    pub attempt: Box<AttemptRecord>,
    pub base_sha: CommitSha,
    pub tree_sha: CommitSha,
    pub message: String,
    pub actual_paths: PathSet,
    pub lease_effect: CandidateLeaseEffect,
}

#[derive(Debug, PartialEq)]
#[must_use = "an unpinned candidate commit is Git's until `pin_candidate` takes it"]
pub struct UnpinnedCandidate {
    judged: JudgedTree,
    names: CandidateNames,
    commit_sha: CommitSha,
}

impl UnpinnedCandidate {
    #[must_use]
    pub fn commit_sha(&self) -> &CommitSha {
        &self.commit_sha
    }
}

#[derive(Debug, PartialEq)]
#[must_use = "a pin with no `candidate_prepared` is the orphan a resume prunes"]
pub struct PinnedCandidate {
    judged: JudgedTree,
    names: CandidateNames,
    commit_sha: CommitSha,
}

#[derive(Debug, PartialEq, Eq)]
#[must_use = "a Promoting generation is always promoted before any run_finished"]
pub struct PromotingCandidate {
    candidate: CandidateRef,
    prepared_ref: GitRef,
    base: CommitSha,
    tree: CommitSha,
}

impl PromotingCandidate {
    #[must_use]
    pub fn candidate(&self) -> &CandidateRef {
        &self.candidate
    }

    #[must_use]
    pub fn prepared_ref(&self) -> &GitRef {
        &self.prepared_ref
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn base(&self) -> &CommitSha {
        &self.base
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct QueuedCandidate {
    candidate: CandidateRef,
}

impl QueuedCandidate {
    #[must_use]
    pub fn candidate(&self) -> &CandidateRef {
        &self.candidate
    }
}

pub fn write_candidate_commit(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    run_id: &str,
    judged: JudgedTree,
) -> Result<UnpinnedCandidate, UpstrokeError> {
    let names = CandidateNames::of(run_id, judged.key, judged.generation);
    let commit = manager.candidate_commit_tree(
        hooks.effects(),
        judged.tree_sha.as_str(),
        judged.base_sha.as_str(),
        &judged.message,
    )?;
    let commit_sha = CommitSha(commit);
    refuse_malformed_commit(judged.key, judged.generation, &commit_sha)?;
    Ok(UnpinnedCandidate {
        judged,
        names,
        commit_sha,
    })
}

pub fn pin_candidate(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    unpinned: UnpinnedCandidate,
) -> Result<PinnedCandidate, UpstrokeError> {
    let UnpinnedCandidate {
        judged,
        names,
        commit_sha,
    } = unpinned;
    manager.create_ref_zero_old(
        hooks.effects(),
        RefSite::PinCandidatePrepared,
        names.prepared_ref.as_str(),
        commit_sha.as_str(),
    )?;
    Ok(PinnedCandidate {
        judged,
        names,
        commit_sha,
    })
}

pub fn append_candidate_prepared(
    journal: &mut dyn CandidateJournal,
    pinned: PinnedCandidate,
) -> Result<PromotingCandidate, UpstrokeError> {
    let PinnedCandidate {
        judged,
        names,
        commit_sha,
    } = pinned;
    let prepared = CandidatePrepared {
        key: judged.key,
        generation: judged.generation,
        attempt: judged.attempt,
        base_sha: judged.base_sha.clone(),
        parent_sha: judged.base_sha.clone(),
        tree_sha: judged.tree_sha.clone(),
        commit_sha,
        message: judged.message,
        prepared_ref: names.prepared_ref.clone(),
        candidate_ref: names.candidate_ref,
        actual_paths: judged.actual_paths,
        lease_effect: judged.lease_effect,
    };
    let candidate = prepared.candidate();
    journal.emit(TopologyEventBody::CandidatePrepared {
        data: Box::new(prepared),
    })?;
    Ok(PromotingCandidate {
        candidate,
        prepared_ref: names.prepared_ref,
        base: judged.base_sha,
        tree: judged.tree_sha,
    })
}

pub fn complete_promotion(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    journal: &mut dyn CandidateJournal,
    worktree: &Slot,
    promoting: PromotingCandidate,
) -> Result<QueuedCandidate, UpstrokeError> {
    let referenced = create_candidates_ref(manager, hooks, promoting)?;
    let created = append_candidate_created(journal, referenced)?;
    reclaim_after_creation(manager, hooks, worktree, created)
}

#[derive(Debug)]
#[must_use = "a candidate with its ref and no `task_candidate_created` is a \
              generation still Promoting"]
pub struct ReferencedCandidate {
    candidate: CandidateRef,
    prepared_ref: GitRef,
}

#[derive(Debug)]
#[must_use = "a created candidate still owns its pin and its worktree"]
pub struct CreatedCandidate {
    candidate: CandidateRef,
    prepared_ref: GitRef,
}

pub fn create_candidates_ref(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    promoting: PromotingCandidate,
) -> Result<ReferencedCandidate, UpstrokeError> {
    let PromotingCandidate {
        candidate,
        prepared_ref,
        base,
        tree,
    } = promoting;

    verify_object(manager, &candidate, &base, &tree)?;

    match manager.direct_ref_target(candidate.candidate_ref.as_str())? {
        None => manager.create_ref_zero_old(
            hooks.effects(),
            RefSite::CreateCandidates,
            candidate.candidate_ref.as_str(),
            candidate.commit_sha.as_str(),
        )?,
        Some(found) if found == candidate.commit_sha.0 => {}
        Some(found) => {
            return Err(Refusal::RefAtAnotherSha {
                refname: candidate.candidate_ref.0.clone(),
                found,
                expected: candidate.commit_sha.0.clone(),
            }
            .into());
        }
    }

    Ok(ReferencedCandidate {
        candidate,
        prepared_ref,
    })
}

pub fn append_candidate_created(
    journal: &mut dyn CandidateJournal,
    referenced: ReferencedCandidate,
) -> Result<CreatedCandidate, UpstrokeError> {
    let ReferencedCandidate {
        candidate,
        prepared_ref,
    } = referenced;

    if is_promoting(journal.fold(), candidate.key, candidate.generation) {
        journal.emit(TopologyEventBody::TaskCandidateCreated {
            data: TaskCandidateCreated {
                candidate: candidate.clone(),
            },
        })?;
    }

    Ok(CreatedCandidate {
        candidate,
        prepared_ref,
    })
}

pub fn reclaim_after_creation(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    worktree: &Slot,
    created: CreatedCandidate,
) -> Result<QueuedCandidate, UpstrokeError> {
    let CreatedCandidate {
        candidate,
        prepared_ref,
    } = created;

    if let Some(found) = manager.direct_ref_target(prepared_ref.as_str())? {
        if found != candidate.commit_sha.0 {
            return Err(Refusal::RefAtAnotherSha {
                refname: prepared_ref.0.clone(),
                found,
                expected: candidate.commit_sha.0.clone(),
            }
            .into());
        }
        manager.delete_ref_expected_old(
            hooks.effects(),
            RefSite::DeleteCandidatePin,
            prepared_ref.as_str(),
            &found,
        )?;
    }

    manager.remove_worktree(hooks.effects(), worktree)?;
    manager.remove_intent(hooks.effects(), worktree)?;

    Ok(QueuedCandidate { candidate })
}

pub fn promote(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    journal: &mut dyn CandidateJournal,
    worktree: &Slot,
    pinned: PinnedCandidate,
) -> Result<QueuedCandidate, UpstrokeError> {
    let promoting = append_candidate_prepared(journal, pinned)?;
    complete_promotion(manager, hooks, journal, worktree, promoting)
}

#[derive(Debug, PartialEq, Eq)]
pub struct OrphanPin {
    pub refname: GitRef,
    pub object: CommitSha,
}

#[derive(Debug, PartialEq, Eq)]
#[must_use = "what a resume owes is not discharged by classifying it"]
pub struct CandidateRecovery {
    pub promotion: Option<PromotingCandidate>,

    pub orphan_pin: Option<OrphanPin>,

    pub settles_interrupted: bool,
}

impl CandidateRecovery {
    pub const NOTHING: Self = Self {
        promotion: None,
        orphan_pin: None,
        settles_interrupted: false,
    };

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.promotion.is_none() && self.orphan_pin.is_none() && !self.settles_interrupted
    }
}

pub fn recovery_for(
    manager: &WorkspaceManager,
    run_id: &str,
    fold: &TopologyFold,
    key: TaskKey,
) -> Result<CandidateRecovery, UpstrokeError> {
    let Some(generation) = fold.task(key).and_then(|task| task.generations.last()) else {
        return Ok(CandidateRecovery::NOTHING);
    };
    let names = CandidateNames::of(run_id, key, generation.id);
    let pin = manager.direct_ref_target(names.prepared_ref.as_str())?;

    let Some(prepared) = generation.candidate.as_ref() else {
        return Ok(CandidateRecovery {
            promotion: None,
            orphan_pin: pin.map(|object| OrphanPin {
                refname: names.prepared_ref,
                object: CommitSha(object),
            }),
            settles_interrupted: matches!(generation.class, GenerationClass::InFlight { .. }),
        });
    };

    if let Some(found) = pin.as_deref() {
        if found != prepared.candidate.commit_sha.as_str() {
            return Err(Refusal::RefAtAnotherSha {
                refname: names.prepared_ref.0.clone(),
                found: found.to_owned(),
                expected: prepared.candidate.commit_sha.0.clone(),
            }
            .into());
        }
    }

    let unfinished = generation.class == GenerationClass::Promoting || pin.is_some();
    Ok(CandidateRecovery {
        promotion: unfinished.then(|| PromotingCandidate {
            candidate: prepared.candidate.clone(),
            prepared_ref: names.prepared_ref,
            base: prepared.base_sha.clone(),
            tree: prepared.tree_sha.clone(),
        }),
        orphan_pin: None,
        settles_interrupted: false,
    })
}

pub fn prune_orphan_pin(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    pin: OrphanPin,
) -> Result<(), UpstrokeError> {
    manager.delete_ref_expected_old(
        hooks.effects(),
        RefSite::DeleteCandidatePin,
        pin.refname.as_str(),
        pin.object.as_str(),
    )
}

#[must_use]
pub fn expected_refs(run_id: &str, fold: &TopologyFold) -> Vec<String> {
    let mut expected = Vec::new();
    let Some(registry) = fold.registry() else {
        return expected;
    };
    for entry in registry.entries() {
        let key = entry.key;
        let Some(task) = fold.task(key) else {
            continue;
        };
        for generation in &task.generations {
            let names = CandidateNames::of(run_id, key, generation.id);
            if generation.candidate.is_some() {
                expected.push(names.candidate_ref.0);
            }
            expected.push(names.prepared_ref.0);
        }
    }
    expected
}

fn is_promoting(fold: &TopologyFold, key: TaskKey, generation: GenerationId) -> bool {
    fold.task(key).is_some_and(|task| {
        task.generations
            .iter()
            .any(|open| open.id == generation && open.class == GenerationClass::Promoting)
    })
}

fn verify_object(
    manager: &WorkspaceManager,
    candidate: &CandidateRef,
    base: &CommitSha,
    tree: &CommitSha,
) -> Result<(), UpstrokeError> {
    let repository: &Path = manager.base();
    let residue = classify_object_residue(
        EffectSiteId::Object(ObjectSite::CandidateCommitTree),
        &ResidueTarget::new(repository).published(candidate.commit_sha.as_str()),
    )?;
    if residue != ObjectResidue::After {
        return Err(Refusal::ObjectMissing {
            key: candidate.key.0,
            generation: candidate.generation.0,
            commit: candidate.commit_sha.0.clone(),
        }
        .into());
    }

    let parent = manager.commit_parent(candidate.commit_sha.as_str())?;
    if parent.as_deref() != Some(base.as_str()) {
        return Err(Refusal::ObjectMissing {
            key: candidate.key.0,
            generation: candidate.generation.0,
            commit: candidate.commit_sha.0.clone(),
        }
        .into());
    }

    let found = manager.commit_tree_sha(candidate.commit_sha.as_str())?;
    if found.as_deref() != Some(tree.as_str()) {
        return Err(Refusal::ObjectMissing {
            key: candidate.key.0,
            generation: candidate.generation.0,
            commit: candidate.commit_sha.0.clone(),
        }
        .into());
    }
    Ok(())
}

fn refuse_malformed_commit(
    key: TaskKey,
    generation: GenerationId,
    commit: &CommitSha,
) -> Result<(), Refusal> {
    if is_object_id(commit.as_str()) {
        return Ok(());
    }
    Err(Refusal::MalformedCommit {
        key: key.0,
        generation: generation.0,
        value: commit.0.clone(),
    })
}

#[cfg(test)]
mod tests;
