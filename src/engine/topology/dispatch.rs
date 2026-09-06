//! Extended notes: `docs/internals/engine/topology/dispatch.md`

use std::path::PathBuf;

use crate::error::UpstrokeError;
use crate::events::RunOutcome;
use crate::topology::events::{
    CandidateRef, CommitSha, GenerationCloseReason, GenerationClosed, GenerationId,
    LeaseDisposition, LeaseGrant, TaskDispatched, TopologyEventBody,
};
use crate::topology::paths::PathSet;
use crate::topology::registry::TaskKey;
use crate::workspace_manager::{Quiescence, Slot, VerifyFailure, WorkspaceManager};

use super::seams::TopologyHooks;

pub trait EventEmitter {
    fn emit(
        &mut self,
        body: TopologyEventBody,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(), super::emit::EmitFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchKind {
    Ordinary { paths: PathSet },
    Repair { root: TaskKey, source: CandidateRef },
}

impl DispatchKind {
    fn grant(&self) -> LeaseGrant {
        match self {
            Self::Ordinary { paths } => LeaseGrant::Predicted {
                paths: paths.clone(),
            },
            Self::Repair { root, .. } => LeaseGrant::InheritedLineage { root: *root },
        }
    }

    fn source_candidate(&self) -> Option<CandidateRef> {
        match self {
            Self::Ordinary { .. } => None,
            Self::Repair { source, .. } => Some(source.clone()),
        }
    }

    fn closing_disposition(&self) -> LeaseDisposition {
        self.lease().expected(false)
    }

    const fn lease(&self) -> crate::topology::leases::GenerationLease {
        match self {
            Self::Ordinary { .. } => crate::topology::leases::GenerationLease::Own,
            Self::Repair { root, .. } => {
                crate::topology::leases::GenerationLease::InheritedLineage { root: *root }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatched {
    pub key: TaskKey,
    pub generation: GenerationId,
    pub base: CommitSha,
    pub slot: Slot,
    pub worktree: PathBuf,
    pub kind: DispatchKind,
}

impl Dispatched {
    #[must_use]
    pub fn quiescence(&self) -> Quiescence {
        Quiescence::AtBase(self.base.0.clone())
    }

    #[must_use]
    pub fn open_generation(&self) -> OpenGeneration {
        OpenGeneration {
            key: self.key,
            generation: self.generation,
            base: self.base.clone(),
            slot: self.slot.clone(),
            source: self.source().cloned(),
        }
    }

    #[must_use]
    pub fn closing_disposition(&self) -> LeaseDisposition {
        self.kind.closing_disposition()
    }

    #[must_use]
    pub const fn source(&self) -> Option<&CandidateRef> {
        match &self.kind {
            DispatchKind::Ordinary { .. } => None,
            DispatchKind::Repair { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenGeneration {
    pub key: TaskKey,
    pub generation: GenerationId,
    pub base: CommitSha,
    pub slot: Slot,
    pub source: Option<CandidateRef>,
}

impl OpenGeneration {
    #[must_use]
    pub fn quiescence(&self) -> Quiescence {
        Quiescence::AtBase(self.base.0.clone())
    }
}

#[must_use]
pub fn task_slot(key: TaskKey, generation: GenerationId) -> Slot {
    Slot::Task {
        key: key.0.to_string(),
        generation: generation.0,
    }
}

pub fn dispatch(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    emitter: &mut dyn EventEmitter,
    request: &DispatchRequest,
) -> Result<Dispatched, super::emit::EmitFailure> {
    manager.revalidate()?;
    if let DispatchKind::Repair { source, .. } = &request.kind {
        refuse_absent_source(manager, source)?;
    }

    let slot = task_slot(request.key, request.generation);
    let worktree = manager.slot_path(&slot);

    emitter.emit(
        TopologyEventBody::TaskDispatched {
            data: TaskDispatched {
                key: request.key,
                generation: request.generation,
                base_sha: request.base.clone(),
                worktree_path: worktree.to_string_lossy().into_owned(),
                lease: request.kind.grant(),
                source_candidate: request.kind.source_candidate(),
            },
        },
        hooks,
    )?;

    let mut dispatched = Dispatched {
        key: request.key,
        generation: request.generation,
        base: request.base.clone(),
        slot,
        worktree,
        kind: request.kind.clone(),
    };
    dispatched.worktree = create_worktree(manager, hooks, &dispatched.open_generation())?;

    if dispatched.source().is_some() {
        materialize_repair(manager, hooks, &dispatched.open_generation())?;
    }
    Ok(dispatched)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchRequest {
    pub key: TaskKey,
    pub generation: GenerationId,
    pub base: CommitSha,
    pub kind: DispatchKind,
}

fn create_worktree(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
) -> Result<PathBuf, UpstrokeError> {
    manager.write_intent(hooks.effects(), &open.slot)?;
    manager.add_worktree(hooks.effects(), &open.slot, &open.base.0)
}

fn refuse_absent_source(
    manager: &WorkspaceManager,
    source: &CandidateRef,
) -> Result<(), UpstrokeError> {
    if !manager.object_exists(&source.commit_sha.0)? {
        return Err(UpstrokeError::Refused {
            message: format!(
                "refusing to dispatch a repair of candidate {} of task {}: its commit `{}` is not \
                 an object in this repository, and `T-DISPATCH` refuses a dispatch whose source \
                 candidate object is missing",
                source.generation.0, source.key, source.commit_sha.0
            ),
        });
    }
    match manager.direct_ref_target(&source.candidate_ref.0)? {
        Some(target) if target.eq_ignore_ascii_case(&source.commit_sha.0) => Ok(()),
        Some(target) => Err(UpstrokeError::Refused {
            message: format!(
                "refusing to dispatch a repair of candidate {} of task {}: `{}` names `{target}` \
                 and the recorded candidate is `{}`",
                source.generation.0, source.key, source.candidate_ref.0, source.commit_sha.0
            ),
        }),
        None => Err(UpstrokeError::Refused {
            message: format!(
                "refusing to dispatch a repair of candidate {} of task {}: its authoritative ref \
                 `{}` does not exist, and it is what keeps the candidate reachable",
                source.generation.0, source.key, source.candidate_ref.0
            ),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reuse {
    Verified,
    Recreated { failure: VerifyFailure },
}

impl Reuse {
    #[must_use]
    pub const fn reused(&self) -> bool {
        matches!(self, Self::Verified)
    }
}

pub fn verify_or_recreate(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
    quiescence: &Quiescence,
) -> Result<Reuse, UpstrokeError> {
    match verify_reuse(manager, hooks, open, quiescence)? {
        Ok(()) => Ok(Reuse::Verified),
        Err(failure) => {
            manager.remove_worktree(hooks.effects(), &open.slot)?;
            create_worktree(manager, hooks, open)?;
            Ok(Reuse::Recreated { failure })
        }
    }
}

pub fn verify_reuse(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
    quiescence: &Quiescence,
) -> Result<Result<(), VerifyFailure>, UpstrokeError> {
    manager.verify_worktree(hooks.effects(), &open.slot, quiescence)
}

pub fn materialize_repair(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
) -> Result<(), UpstrokeError> {
    let Some(source) = open.source.as_ref() else {
        return Err(UpstrokeError::Refused {
            message: format!(
                "task {} generation {} is an ordinary dispatch and has no recorded \
                 materialization to reproduce",
                open.key, open.generation.0
            ),
        });
    };
    manager.repair_materialize(hooks.effects(), &open.slot, &source.commit_sha.0)
}

pub fn resume_open_no_attempt(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
) -> Result<Reuse, UpstrokeError> {
    let reuse = verify_or_recreate(manager, hooks, open, &open.quiescence())?;
    if open.source.is_some() {
        materialize_repair(manager, hooks, open)?;
    }
    Ok(reuse)
}

pub fn close_at_run_end(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    emitter: &mut dyn EventEmitter,
    dispatched: &Dispatched,
    outcome: RunOutcome,
) -> Result<(), super::emit::EmitFailure> {
    emitter.emit(
        TopologyEventBody::GenerationClosed {
            data: GenerationClosed {
                key: dispatched.key,
                generation: dispatched.generation,
                reason: GenerationCloseReason::RunEnding { outcome },
                lease: dispatched.kind.closing_disposition(),
            },
        },
        hooks,
    )?;
    Ok(scrub(manager, hooks, &dispatched.slot)?)
}

pub(super) fn scrub(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    slot: &Slot,
) -> Result<(), UpstrokeError> {
    manager.remove_worktree(hooks.effects(), slot)?;
    manager.remove_intent(hooks.effects(), slot)
}

#[cfg(test)]
mod tests;
