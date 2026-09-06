//! The eleven site enums, one per funnel group, and the `identity` functions
//! every variant carries.
//!
//! Split out of `topology::effects`; the parent re-exports every item here, so
//! `crate::topology::effects::WorktreeSite` and its siblings are unchanged
//! paths.

use super::residue_authority::{ResidueClass, ResidueElement};
use super::vocab::{Adjacent, DurableEvent, FaultRow, ResourceRow, SiteScope, SubEffectPoint};

// ---------------------------------------------------------------------------
// Site enums, one per funnel group
// ---------------------------------------------------------------------------

/// The task, staging and execution-root contexts of the worktree funnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorktreeSite {
    /// The run's execution root, inside which every worktree is created (R18).
    CreateExecutionRoot,
    /// Removing the execution root at finalization, when it is empty.
    RemoveExecutionRoot,
    /// The durable synced intent for a task worktree.
    WriteIntent,
    /// `git worktree add` for a task worktree.
    Add,
    /// Read-only quiescence observation: present, HEAD at the recorded base,
    /// index unlocked, no cherry-pick/merge/sequencer state. Performs no
    /// effect; its failure routes to forced removal and a fresh add.
    Verify,
    /// Forced removal of a task worktree, releasing its index-referenced
    /// objects to R27 and taking its administrative residue with it.
    Remove,
    /// Removing a task worktree's intent.
    RemoveIntent,
    /// The durable synced intent for a `merge/<seq>` staging worktree.
    WriteStagingIntent,
    /// `git worktree add` for a staging worktree — never executed for an
    /// exact-base fast sequence.
    AddStaging,
    /// Forced removal of a staging worktree.
    RemoveStaging,
    /// Removing a staging worktree's intent.
    RemoveStagingIntent,
}

impl WorktreeSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[
        Self::CreateExecutionRoot,
        Self::RemoveExecutionRoot,
        Self::WriteIntent,
        Self::Add,
        Self::Verify,
        Self::Remove,
        Self::RemoveIntent,
        Self::WriteStagingIntent,
        Self::AddStaging,
        Self::RemoveStaging,
        Self::RemoveStagingIntent,
    ];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::CreateExecutionRoot => "CreateExecutionRoot",
            Self::RemoveExecutionRoot => "RemoveExecutionRoot",
            Self::WriteIntent => "WriteIntent",
            Self::Add => "Add",
            Self::Verify => "Verify",
            Self::Remove => "Remove",
            Self::RemoveIntent => "RemoveIntent",
            Self::WriteStagingIntent => "WriteStagingIntent",
            Self::AddStaging => "AddStaging",
            Self::RemoveStaging => "RemoveStaging",
            Self::RemoveStagingIntent => "RemoveStagingIntent",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::CreateExecutionRoot | Self::RemoveExecutionRoot => ResourceRow::R18,
            Self::WriteIntent | Self::Add | Self::Verify | Self::Remove | Self::RemoveIntent => {
                ResourceRow::R9
            }
            Self::WriteStagingIntent
            | Self::AddStaging
            | Self::RemoveStaging
            | Self::RemoveStagingIntent => ResourceRow::R10,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::CreateExecutionRoot => Adjacent::Before(DurableEvent::RunStarted),
            Self::RemoveExecutionRoot => Adjacent::After(DurableEvent::RunFinished),
            Self::WriteIntent | Self::Add => Adjacent::After(DurableEvent::TaskDispatched),
            Self::Verify => Adjacent::Before(DurableEvent::AttemptStarted),
            Self::Remove | Self::RemoveIntent => {
                Adjacent::After(DurableEvent::TaskCandidateCreated)
            }
            Self::WriteStagingIntent | Self::AddStaging => {
                Adjacent::Before(DurableEvent::MergeVerificationStarted)
            }
            Self::RemoveStaging | Self::RemoveStagingIntent => {
                Adjacent::After(DurableEvent::TaskMerged)
            }
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::CreateExecutionRoot => FaultRow::TRunstart,
            Self::RemoveExecutionRoot => FaultRow::TFinalize,
            Self::WriteIntent | Self::Add => FaultRow::TDispatch,
            Self::Verify => FaultRow::TRetry,
            Self::Remove | Self::RemoveIntent => FaultRow::TScrub,
            Self::WriteStagingIntent
            | Self::AddStaging
            | Self::RemoveStaging
            | Self::RemoveStagingIntent => FaultRow::TProposal,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::CreateExecutionRoot
            | Self::RemoveExecutionRoot
            | Self::WriteIntent
            | Self::Add
            | Self::Verify
            | Self::Remove
            | Self::RemoveIntent
            | Self::WriteStagingIntent
            | Self::AddStaging
            | Self::RemoveStaging
            | Self::RemoveStagingIntent => SiteScope::Topology,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::Verify => true,
            Self::CreateExecutionRoot
            | Self::RemoveExecutionRoot
            | Self::WriteIntent
            | Self::Add
            | Self::Remove
            | Self::RemoveIntent
            | Self::WriteStagingIntent
            | Self::AddStaging
            | Self::RemoveStaging
            | Self::RemoveStagingIntent => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::CreateExecutionRoot
            | Self::RemoveExecutionRoot
            | Self::WriteIntent
            | Self::Add
            | Self::Verify
            | Self::Remove
            | Self::RemoveIntent
            | Self::WriteStagingIntent
            | Self::AddStaging
            | Self::RemoveStaging
            | Self::RemoveStagingIntent => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::Add | Self::AddStaging => &[ResidueClass::ObjectInternal],
            Self::CreateExecutionRoot
            | Self::RemoveExecutionRoot
            | Self::WriteIntent
            | Self::Verify
            | Self::Remove
            | Self::RemoveIntent
            | Self::WriteStagingIntent
            | Self::RemoveStaging
            | Self::RemoveStagingIntent => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::Add | Self::AddStaging => &[ResidueElement::RegisteredUnpopulatedWorktree],
            Self::CreateExecutionRoot
            | Self::RemoveExecutionRoot
            | Self::WriteIntent
            | Self::Verify
            | Self::Remove
            | Self::RemoveIntent
            | Self::WriteStagingIntent
            | Self::RemoveStaging
            | Self::RemoveStagingIntent => &[],
        }
    }
}

/// The gate/review snapshot contexts (R24).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SnapshotSite {
    /// The durable synced intent for a snapshot worktree.
    WriteIntent,
    /// `git worktree add` for a snapshot worktree; its detached HEAD picks up
    /// the ephemeral commit and moves it out of R27.
    Add,
    /// Forced removal, releasing an ephemeral commit back to R27.
    Remove,
    /// Removing a snapshot's intent.
    RemoveIntent,
}

impl SnapshotSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[
        Self::WriteIntent,
        Self::Add,
        Self::Remove,
        Self::RemoveIntent,
    ];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::WriteIntent => "WriteIntent",
            Self::Add => "Add",
            Self::Remove => "Remove",
            Self::RemoveIntent => "RemoveIntent",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::WriteIntent | Self::Add | Self::Remove | Self::RemoveIntent => ResourceRow::R24,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::WriteIntent | Self::Add => Adjacent::After(DurableEvent::AttemptStarted),
            Self::Remove | Self::RemoveIntent => Adjacent::Before(DurableEvent::AttemptFinished),
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::WriteIntent | Self::Add => FaultRow::TAttempt,
            Self::Remove | Self::RemoveIntent => FaultRow::TScrub,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::WriteIntent | Self::Add | Self::Remove | Self::RemoveIntent => {
                SiteScope::Topology
            }
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::WriteIntent | Self::Add | Self::Remove | Self::RemoveIntent => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::WriteIntent | Self::Add | Self::Remove | Self::RemoveIntent => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::Add => &[ResidueClass::ObjectInternal],
            Self::WriteIntent | Self::Remove | Self::RemoveIntent => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::Add => &[ResidueElement::RegisteredUnpopulatedWorktree],
            Self::WriteIntent | Self::Remove | Self::RemoveIntent => &[],
        }
    }
}

/// The ref-store contexts: the integration ref, the candidates ref, and the two
/// pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefSite {
    /// Creating the run's integration ref (R21).
    CreateIntegration,
    /// The compare-and-swap that publishes an integration.
    CompareAndSwapIntegration,
    /// Creating a candidate's authoritative candidates ref (R11).
    CreateCandidates,
    /// Deleting a candidates ref at Complete finalization.
    DeleteCandidatesRef,
    /// Pinning the candidate commit before `candidate_prepared` (R23).
    PinCandidatePrepared,
    /// Deleting that pin expected-old once the candidate ref exists.
    DeleteCandidatePin,
    /// Pinning the proposal as `prepared/<seq>` (R12) — never executed for an
    /// exact-base fast sequence.
    PinPrepared,
    /// Deleting a prepared pin.
    DeletePreparedPin,
}

impl RefSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[
        Self::CreateIntegration,
        Self::CompareAndSwapIntegration,
        Self::CreateCandidates,
        Self::DeleteCandidatesRef,
        Self::PinCandidatePrepared,
        Self::DeleteCandidatePin,
        Self::PinPrepared,
        Self::DeletePreparedPin,
    ];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::CreateIntegration => "CreateIntegration",
            Self::CompareAndSwapIntegration => "CompareAndSwapIntegration",
            Self::CreateCandidates => "CreateCandidates",
            Self::DeleteCandidatesRef => "DeleteCandidatesRef",
            Self::PinCandidatePrepared => "PinCandidatePrepared",
            Self::DeleteCandidatePin => "DeleteCandidatePin",
            Self::PinPrepared => "PinPrepared",
            Self::DeletePreparedPin => "DeletePreparedPin",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::CreateIntegration | Self::CompareAndSwapIntegration => ResourceRow::R21,
            Self::CreateCandidates | Self::DeleteCandidatesRef => ResourceRow::R11,
            Self::PinCandidatePrepared | Self::DeleteCandidatePin => ResourceRow::R23,
            Self::PinPrepared | Self::DeletePreparedPin => ResourceRow::R12,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::CreateIntegration => Adjacent::Before(DurableEvent::RunStarted),
            Self::CompareAndSwapIntegration => Adjacent::Before(DurableEvent::TaskMerged),
            Self::CreateCandidates => Adjacent::Before(DurableEvent::TaskCandidateCreated),
            Self::DeleteCandidatesRef => Adjacent::After(DurableEvent::RunFinished),
            Self::PinCandidatePrepared => Adjacent::Before(DurableEvent::CandidatePrepared),
            Self::DeleteCandidatePin => Adjacent::After(DurableEvent::TaskCandidateCreated),
            Self::PinPrepared => Adjacent::Before(DurableEvent::MergeVerificationStarted),
            Self::DeletePreparedPin => Adjacent::After(DurableEvent::TaskMerged),
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::CreateIntegration => FaultRow::TRunstart,
            Self::CompareAndSwapIntegration => FaultRow::TFast,
            Self::CreateCandidates | Self::DeleteCandidatePin => FaultRow::TCandRef,
            Self::DeleteCandidatesRef | Self::DeletePreparedPin => FaultRow::TFinalize,
            Self::PinCandidatePrepared => FaultRow::TCandObj,
            Self::PinPrepared => FaultRow::TProposal,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::CreateIntegration
            | Self::CompareAndSwapIntegration
            | Self::CreateCandidates
            | Self::DeleteCandidatesRef
            | Self::PinCandidatePrepared
            | Self::DeleteCandidatePin
            | Self::PinPrepared
            | Self::DeletePreparedPin => SiteScope::Topology,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::CreateIntegration
            | Self::CompareAndSwapIntegration
            | Self::CreateCandidates
            | Self::DeleteCandidatesRef
            | Self::PinCandidatePrepared
            | Self::DeleteCandidatePin
            | Self::PinPrepared
            | Self::DeletePreparedPin => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::CreateIntegration
            | Self::CompareAndSwapIntegration
            | Self::CreateCandidates
            | Self::DeleteCandidatesRef
            | Self::PinCandidatePrepared
            | Self::DeleteCandidatePin
            | Self::PinPrepared
            | Self::DeletePreparedPin => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::CreateIntegration
            | Self::CompareAndSwapIntegration
            | Self::CreateCandidates
            | Self::DeleteCandidatesRef
            | Self::PinCandidatePrepared
            | Self::DeleteCandidatePin
            | Self::PinPrepared
            | Self::DeletePreparedPin => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::CreateIntegration
            | Self::CompareAndSwapIntegration
            | Self::CreateCandidates
            | Self::DeleteCandidatesRef
            | Self::PinCandidatePrepared
            | Self::DeleteCandidatePin
            | Self::PinPrepared
            | Self::DeletePreparedPin => &[],
        }
    }
}

/// One site per Git-object creation context.
///
/// `row()` names the row that references the object *immediately after* the
/// effect, which is why the two commit-tree sites are R27: a commit-tree writes
/// its object and nothing points at it until a later site does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectSite {
    /// `git add -A` in the task worktree: blobs behind the worktree index.
    CandidateStage,
    /// `git write-tree`: trees behind the worktree index.
    CandidateWriteTree,
    /// The ephemeral commit for a tree-only snapshot input; unreferenced until
    /// `Snapshot::Add` makes it the snapshot HEAD.
    SnapshotCommitTree,
    /// The candidate commit; unreferenced until `Ref::PinCandidatePrepared`.
    CandidateCommitTree,
    /// `git cherry-pick` in the staging worktree of a stale candidate; never
    /// executed for a fast sequence.
    ProposalCherryPick,
    /// `git cherry-pick --no-commit` in a repair worktree.
    RepairMaterialize,
}

impl ObjectSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[
        Self::CandidateStage,
        Self::CandidateWriteTree,
        Self::SnapshotCommitTree,
        Self::CandidateCommitTree,
        Self::ProposalCherryPick,
        Self::RepairMaterialize,
    ];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::CandidateStage => "CandidateStage",
            Self::CandidateWriteTree => "CandidateWriteTree",
            Self::SnapshotCommitTree => "SnapshotCommitTree",
            Self::CandidateCommitTree => "CandidateCommitTree",
            Self::ProposalCherryPick => "ProposalCherryPick",
            Self::RepairMaterialize => "RepairMaterialize",
        }
    }

    /// The row that references the created object immediately after the effect.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::CandidateStage | Self::CandidateWriteTree | Self::RepairMaterialize => {
                ResourceRow::R9
            }
            Self::SnapshotCommitTree | Self::CandidateCommitTree => ResourceRow::R27,
            Self::ProposalCherryPick => ResourceRow::R10,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::CandidateStage | Self::CandidateWriteTree | Self::SnapshotCommitTree => {
                Adjacent::After(DurableEvent::AttemptStarted)
            }
            Self::CandidateCommitTree => Adjacent::Before(DurableEvent::CandidatePrepared),
            Self::ProposalCherryPick => Adjacent::Before(DurableEvent::MergeVerificationStarted),
            Self::RepairMaterialize => Adjacent::After(DurableEvent::TaskDispatched),
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::CandidateStage | Self::CandidateWriteTree | Self::SnapshotCommitTree => {
                FaultRow::TAttempt
            }
            Self::CandidateCommitTree => FaultRow::TCandObj,
            Self::ProposalCherryPick => FaultRow::TProposal,
            Self::RepairMaterialize => FaultRow::TRepairDispatch,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::CandidateStage
            | Self::CandidateWriteTree
            | Self::SnapshotCommitTree
            | Self::CandidateCommitTree
            | Self::ProposalCherryPick
            | Self::RepairMaterialize => SiteScope::Topology,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::CandidateStage
            | Self::CandidateWriteTree
            | Self::SnapshotCommitTree
            | Self::CandidateCommitTree
            | Self::ProposalCherryPick
            | Self::RepairMaterialize => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    ///
    /// Only the two commit-tree sites have one. Every other Object site's
    /// post-child prefix is command-internal: the parent has no place to stand
    /// between the object writes and the reference publication.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::SnapshotCommitTree | Self::CandidateCommitTree => &[SubEffectPoint::IdUnread],
            Self::CandidateStage
            | Self::CandidateWriteTree
            | Self::ProposalCherryPick
            | Self::RepairMaterialize => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::CandidateStage
            | Self::CandidateWriteTree
            | Self::SnapshotCommitTree
            | Self::CandidateCommitTree
            | Self::ProposalCherryPick
            | Self::RepairMaterialize => &[ResidueClass::ObjectInternal],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    ///
    /// Per command, from the fault matrix's own residue descriptions: a killed
    /// `git add` leaves an `index.lock`, a killed cherry-pick leaves sequencer
    /// state as well, and a killed `commit-tree` leaves neither because it
    /// writes one object by temp-file-and-rename and touches no index.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::CandidateStage | Self::CandidateWriteTree => &[
                ResidueElement::UnreferencedObject,
                ResidueElement::TemporaryObjectFile,
                ResidueElement::IndexLock,
            ],
            Self::SnapshotCommitTree | Self::CandidateCommitTree => &[
                ResidueElement::UnreferencedObject,
                ResidueElement::TemporaryObjectFile,
            ],
            Self::ProposalCherryPick => &[
                ResidueElement::UnreferencedObject,
                ResidueElement::TemporaryObjectFile,
                ResidueElement::IndexLock,
                ResidueElement::CherryPickHead,
                ResidueElement::MergeHead,
                ResidueElement::MergeMsg,
                ResidueElement::SequencerState,
            ],
            Self::RepairMaterialize => &[
                ResidueElement::UnreferencedObject,
                ResidueElement::TemporaryObjectFile,
                ResidueElement::IndexLock,
                ResidueElement::CherryPickHead,
            ],
        }
    }
}

/// The run-directory funnel: everything under a run's public and private
/// halves. Every site is R21.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RunDirSite {
    /// P0: the bare public run directory.
    CreatePublicDir,
    /// P1: `.creating.tmp`.
    StageMarker,
    /// P1: the atomic rename to `.creating`.
    PublishMarker,
    /// P6: removing the marker once `run_started` is durable.
    RemoveMarker,
    /// P2: the private half.
    CreatePrivateDir,
    /// P3a: `owner.json.tmp`.
    StageOwnerRecord,
    /// P3b: the atomic rename publishing the reciprocal ownership record.
    PublishOwnerRecord,
    /// P5a: `committed.json.tmp`.
    StageCommitRecord,
    /// P5b: the atomic rename publishing the private commit record.
    ///
    /// The one deletion boundary: after this site returns, or when a read-only
    /// stat after its error shows the record present, no **run-lifecycle** path
    /// — creator or census — deletes the private half.
    ///
    /// "Run-lifecycle" is the scope the sentence always carried and now states:
    /// every path that reaches a run directory *as a run directory*. There is
    /// one exception and it is on none of them —
    /// `rundir::scratch_tree::remove_scratch_tree`, which reclaims a tree a test
    /// minted under a second token. That token binds a root that did not exist
    /// before the token did, so nothing beneath it predates the token and a
    /// `committed.json` inside it is a fixture the holder published rather than
    /// a run's boundary. The funnel is `#[cfg(test)]`: it is absent from the
    /// rlib, it takes no site here and adds none to `effect_sites.json`, it
    /// cannot mint or weaken a `PrivateHalfProof`, and conjunct 12 of the
    /// ownership proof is unmoved and still fail-closed.
    /// `DESIGN.md` §15 states the authority model and `CODING_STANDARDS.md` §8
    /// states it in its two-token form: `rundir::PrivateHalfProof` in every
    /// build, the `cfg(test)`-only scratch-tree token beside it. The
    /// 2026-08-30 decision record this sentence used to cite went with the
    /// `decisions/` directory on 2026-09-03, and `DESIGN.md`'s record index is
    /// what says where each such record's substance now lives.
    PublishCommitRecord,
    /// P4: `plan.normalized.json`.
    WritePlan,
    /// `report.json`.
    WriteReport,
    /// A question's payload file, written before the question is announced.
    WriteQuestionPayload,
    /// Removing the private half of a husk, under the ownership proof.
    RemovePrivateHusk,
    /// Removing the public half of a husk.
    RemovePublicHusk,
}

impl RunDirSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[
        Self::CreatePublicDir,
        Self::StageMarker,
        Self::PublishMarker,
        Self::RemoveMarker,
        Self::CreatePrivateDir,
        Self::StageOwnerRecord,
        Self::PublishOwnerRecord,
        Self::StageCommitRecord,
        Self::PublishCommitRecord,
        Self::WritePlan,
        Self::WriteReport,
        Self::WriteQuestionPayload,
        Self::RemovePrivateHusk,
        Self::RemovePublicHusk,
    ];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::CreatePublicDir => "CreatePublicDir",
            Self::StageMarker => "StageMarker",
            Self::PublishMarker => "PublishMarker",
            Self::RemoveMarker => "RemoveMarker",
            Self::CreatePrivateDir => "CreatePrivateDir",
            Self::StageOwnerRecord => "StageOwnerRecord",
            Self::PublishOwnerRecord => "PublishOwnerRecord",
            Self::StageCommitRecord => "StageCommitRecord",
            Self::PublishCommitRecord => "PublishCommitRecord",
            Self::WritePlan => "WritePlan",
            Self::WriteReport => "WriteReport",
            Self::WriteQuestionPayload => "WriteQuestionPayload",
            Self::RemovePrivateHusk => "RemovePrivateHusk",
            Self::RemovePublicHusk => "RemovePublicHusk",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::RemoveMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan
            | Self::WriteReport
            | Self::WriteQuestionPayload
            | Self::RemovePrivateHusk
            | Self::RemovePublicHusk => ResourceRow::R21,
        }
    }

    /// The append this site's effect is ordered against.
    ///
    /// The husk-removal pair is `None`: a census removes the halves of a run
    /// whose log never committed, so there is no append on the other side of
    /// the order.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan => Adjacent::Before(DurableEvent::RunStarted),
            Self::RemoveMarker => Adjacent::After(DurableEvent::RunStarted),
            Self::WriteReport => Adjacent::After(DurableEvent::RunFinished),
            Self::WriteQuestionPayload => Adjacent::Before(DurableEvent::QuestionRaised),
            Self::RemovePrivateHusk | Self::RemovePublicHusk => Adjacent::None,
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::RemoveMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan
            | Self::RemovePrivateHusk
            | Self::RemovePublicHusk => FaultRow::TRunstart,
            Self::WriteReport => FaultRow::TFinalize,
            Self::WriteQuestionPayload => FaultRow::TFailed,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::RemoveMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan
            | Self::WriteReport
            | Self::WriteQuestionPayload
            | Self::RemovePrivateHusk
            | Self::RemovePublicHusk => SiteScope::Shared,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::RemoveMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan
            | Self::WriteReport
            | Self::WriteQuestionPayload
            | Self::RemovePrivateHusk
            | Self::RemovePublicHusk => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::RemoveMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan
            | Self::WriteReport
            | Self::WriteQuestionPayload
            | Self::RemovePrivateHusk
            | Self::RemovePublicHusk => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::RemoveMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan
            | Self::WriteReport
            | Self::WriteQuestionPayload
            | Self::RemovePrivateHusk
            | Self::RemovePublicHusk => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::RemoveMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan
            | Self::WriteReport
            | Self::WriteQuestionPayload
            | Self::RemovePrivateHusk
            | Self::RemovePublicHusk => &[],
        }
    }
}

/// The event-log funnel. Shared: legacy callers pass the Legacy-scoped sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventSite {
    /// Create the log if absent and fsync its directory; truncate an
    /// unterminated final line; sync the complete surviving prefix. Supersedes
    /// the run directory's create-log site.
    OpenLog,
    /// The read-only half of the stable-prefix barrier: reread the file, prove
    /// bytes and boundary equal to the synced prefix, checked-replay exactly
    /// those bytes. No effect.
    ProvePrefixStable,
    /// The `run_started` append: the commitment boundary.
    AppendFirst,
    /// Every later transaction append.
    Append,
    /// A lenient informational append.
    AppendInformational,
    /// A schema-1..3 caller opening its own log through this funnel.
    LegacyOpenLog,
    /// A schema-1..3 caller appending through this funnel.
    LegacyAppend,
}

impl EventSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[
        Self::OpenLog,
        Self::ProvePrefixStable,
        Self::AppendFirst,
        Self::Append,
        Self::AppendInformational,
        Self::LegacyOpenLog,
        Self::LegacyAppend,
    ];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::OpenLog => "OpenLog",
            Self::ProvePrefixStable => "ProvePrefixStable",
            Self::AppendFirst => "AppendFirst",
            Self::Append => "Append",
            Self::AppendInformational => "AppendInformational",
            Self::LegacyOpenLog => "LegacyOpenLog",
            Self::LegacyAppend => "LegacyAppend",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::OpenLog
            | Self::ProvePrefixStable
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational
            | Self::LegacyOpenLog
            | Self::LegacyAppend => ResourceRow::R21,
        }
    }

    /// The append this site's effect is ordered against.
    ///
    /// Always `None`, and not because it is unknown: an append site *is* the
    /// durable event, so there is no second thing for it to be ordered against
    /// and no observable order for the registry to range over. What a fault
    /// here leaves is a torn, unsynced, or synced *prefix*, which is what the
    /// site's sub-effect points are for.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::OpenLog
            | Self::ProvePrefixStable
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational
            | Self::LegacyOpenLog
            | Self::LegacyAppend => Adjacent::None,
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::OpenLog
            | Self::ProvePrefixStable
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational
            | Self::LegacyOpenLog
            | Self::LegacyAppend => FaultRow::TAppend,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::OpenLog
            | Self::ProvePrefixStable
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational => SiteScope::Shared,
            Self::LegacyOpenLog | Self::LegacyAppend => SiteScope::Legacy,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::ProvePrefixStable => true,
            Self::OpenLog
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational
            | Self::LegacyOpenLog
            | Self::LegacyAppend => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    ///
    /// The Legacy sites expose none: they are inventoried and row-mapped and
    /// carry no fault-registry requirement, so declaring points for them would
    /// manufacture a coverage obligation the design explicitly does not make.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::OpenLog => &[
                SubEffectPoint::Create,
                SubEffectPoint::TruncateTornTail,
                SubEffectPoint::SyncPrefix,
            ],
            Self::AppendFirst | Self::Append | Self::AppendInformational => &[
                SubEffectPoint::Written,
                SubEffectPoint::WrittenFull,
                SubEffectPoint::Synced,
            ],
            Self::ProvePrefixStable | Self::LegacyOpenLog | Self::LegacyAppend => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::OpenLog
            | Self::ProvePrefixStable
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational
            | Self::LegacyOpenLog
            | Self::LegacyAppend => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::OpenLog
            | Self::ProvePrefixStable
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational
            | Self::LegacyOpenLog
            | Self::LegacyAppend => &[],
        }
    }
}

/// The answer funnel: the `upstroke answer` command's two writes, and the
/// coordinator's read-only ingestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnswerSite {
    /// `answers/<qid>.json.partial`, writer-owned staging residue.
    StageWrite,
    /// The atomic rename publishing `answers/<qid>.json`.
    PublishRename,
    /// Reading a published answer. No effect; a file for a closed or void
    /// question is inert.
    Ingest,
}

impl AnswerSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[Self::StageWrite, Self::PublishRename, Self::Ingest];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::StageWrite => "StageWrite",
            Self::PublishRename => "PublishRename",
            Self::Ingest => "Ingest",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::StageWrite | Self::PublishRename | Self::Ingest => ResourceRow::R21,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::StageWrite | Self::PublishRename | Self::Ingest => {
                Adjacent::Before(DurableEvent::QuestionAnswered)
            }
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::StageWrite | Self::PublishRename | Self::Ingest => FaultRow::TAnswer,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::StageWrite | Self::PublishRename | Self::Ingest => SiteScope::Shared,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::Ingest => true,
            Self::StageWrite | Self::PublishRename => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::StageWrite | Self::PublishRename | Self::Ingest => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::StageWrite | Self::PublishRename | Self::Ingest => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::StageWrite | Self::PublishRename | Self::Ingest => &[],
        }
    }
}

/// The lock funnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LockSite {
    /// The run-scoped `run.lock` exclusive hold.
    AcquireRun,
    /// The repository-scoped `upstroke-worktree.lock` exclusive hold — the first
    /// effect of every write command, after its read-only refusals.
    AcquireWorktree,
    /// The momentary exclusive `cleanup.lock` probe (Unix).
    ProbeCleanupExclusive,
    /// Releasing a hold this process took.
    Release,
    /// Creating the `upstroke-worktree.lock` file itself (R25), which spans runs
    /// and is never removed by one.
    CreateWorktreeLockFile,
    /// Observing a surviving reaper's shared cleanup hold (R28). Never owned,
    /// never reset; read-only.
    ObserveCleanupHold,
}

impl LockSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[
        Self::AcquireRun,
        Self::AcquireWorktree,
        Self::ProbeCleanupExclusive,
        Self::Release,
        Self::CreateWorktreeLockFile,
        Self::ObserveCleanupHold,
    ];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::AcquireRun => "AcquireRun",
            Self::AcquireWorktree => "AcquireWorktree",
            Self::ProbeCleanupExclusive => "ProbeCleanupExclusive",
            Self::Release => "Release",
            Self::CreateWorktreeLockFile => "CreateWorktreeLockFile",
            Self::ObserveCleanupHold => "ObserveCleanupHold",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::Release => ResourceRow::R17,
            Self::CreateWorktreeLockFile => ResourceRow::R25,
            Self::ObserveCleanupHold => ResourceRow::R28,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::CreateWorktreeLockFile
            | Self::ObserveCleanupHold => Adjacent::Before(DurableEvent::RunStarted),
            Self::Release => Adjacent::After(DurableEvent::RunFinished),
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::CreateWorktreeLockFile
            | Self::ObserveCleanupHold => FaultRow::TRunstart,
            Self::Release => FaultRow::TFinalize,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::Release
            | Self::CreateWorktreeLockFile
            | Self::ObserveCleanupHold => SiteScope::Shared,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::ObserveCleanupHold => true,
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::Release
            | Self::CreateWorktreeLockFile => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::Release
            | Self::CreateWorktreeLockFile
            | Self::ObserveCleanupHold => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::Release
            | Self::CreateWorktreeLockFile
            | Self::ObserveCleanupHold => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::Release
            | Self::CreateWorktreeLockFile
            | Self::ObserveCleanupHold => &[],
        }
    }
}

/// The report funnel.
///
/// One site, and `report.json` is named twice in the frozen inventory: here and
/// at [`RunDirSite::WriteReport`]. Both are declared because both are named —
/// not because both are reached. At this head one of the two has a funnel:
/// `RunDir.WriteReport` is funnelled in `src/rundir.rs`, while this site's
/// module, `src/util.rs`, names no `ReportSite` at all, which is what makes
/// `Report.Write` the single entry of `SITES_WITHOUT_A_FUNNEL` in
/// `src/effects/tests/artifacts.rs`. That census —
/// `every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
/// in `src/effects/tests.rs` — is the live answer to which sites a funnel
/// reaches; this sentence is a pointer to it and goes stale the moment it
/// disagrees.
///
/// One durable object under two inventory names is the owner's standing
/// finding `PR3-REPORT-DOUBLE-NAME` (`reviews/findings/`, history in
/// `reviews/FINDINGS.md` §2): ST-07 will demand two hook executions for one
/// write. It is not this module's to resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReportSite {
    /// Writing `report.json`.
    Write,
}

impl ReportSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[Self::Write];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Write => "Write",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::Write => ResourceRow::R21,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::Write => Adjacent::After(DurableEvent::RunFinished),
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::Write => FaultRow::TFinalize,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::Write => SiteScope::Shared,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::Write => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::Write => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::Write => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::Write => &[],
        }
    }
}

/// The process funnel (R22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProcessSite {
    /// Spawning a host process, with the platform containment steps as its
    /// sub-effect points.
    Spawn,
    /// Killing a host process group or closing its job handle on exit,
    /// timeout, cancel, or shutdown.
    Terminate,
}

impl ProcessSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[Self::Spawn, Self::Terminate];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Spawn => "Spawn",
            Self::Terminate => "Terminate",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::Spawn | Self::Terminate => ResourceRow::R22,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::Spawn => Adjacent::After(DurableEvent::AttemptStarted),
            Self::Terminate => Adjacent::Before(DurableEvent::AttemptFinished),
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::Spawn | Self::Terminate => FaultRow::TAttempt,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::Spawn | Self::Terminate => SiteScope::Topology,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::Spawn | Self::Terminate => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    ///
    /// All eight containment steps, Windows and Unix. Which of them a given
    /// suite has to observe is decided by [`Platform::required_on`](crate::topology::effects::Platform::required_on), not by
    /// omitting the other platform's points from the inventory: a Windows CI
    /// run and a Unix one have to be checkable against the same enum.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::Spawn => &[
                SubEffectPoint::AmbientJobJoined,
                SubEffectPoint::CreatedSuspended,
                SubEffectPoint::PrivateJobAssigned,
                SubEffectPoint::Resumed,
                SubEffectPoint::ReaperStarted,
                SubEffectPoint::PreExecPgidAndRegister,
                SubEffectPoint::Exec,
                SubEffectPoint::Registered,
            ],
            Self::Terminate => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::Spawn | Self::Terminate => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::Spawn | Self::Terminate => &[],
        }
    }
}

/// The container funnel (R19 for the Git view, R26 for the container itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContainerSite {
    /// The durable global intent record, named with the run id and incarnation.
    WriteIntent,
    /// Creating the container from the recorded image id, verifying the created
    /// container's image id against the record before start.
    Create,
    /// Starting it.
    Start,
    /// Mounting the disposable Git view (R19).
    MountGitView,
    /// Stopping it.
    Stop,
    /// Removing it.
    Remove,
    /// Unmounting the Git view.
    UnmountGitView,
    /// Removing the intent record.
    RemoveIntent,
}

impl ContainerSite {
    /// Every site of the group.
    pub const ALL: &'static [Self] = &[
        Self::WriteIntent,
        Self::Create,
        Self::Start,
        Self::MountGitView,
        Self::Stop,
        Self::Remove,
        Self::UnmountGitView,
        Self::RemoveIntent,
    ];

    /// The variant's name inside its group.
    pub const fn name(self) -> &'static str {
        match self {
            Self::WriteIntent => "WriteIntent",
            Self::Create => "Create",
            Self::Start => "Start",
            Self::MountGitView => "MountGitView",
            Self::Stop => "Stop",
            Self::Remove => "Remove",
            Self::UnmountGitView => "UnmountGitView",
            Self::RemoveIntent => "RemoveIntent",
        }
    }

    /// The row that accounts for what this site touches.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::MountGitView | Self::UnmountGitView => ResourceRow::R19,
            Self::WriteIntent
            | Self::Create
            | Self::Start
            | Self::Stop
            | Self::Remove
            | Self::RemoveIntent => ResourceRow::R26,
        }
    }

    /// The append this site's effect is ordered against.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::WriteIntent | Self::Create | Self::Start | Self::MountGitView => {
                Adjacent::After(DurableEvent::AttemptStarted)
            }
            Self::Stop | Self::Remove | Self::UnmountGitView | Self::RemoveIntent => {
                Adjacent::Before(DurableEvent::AttemptFinished)
            }
        }
    }

    /// The fault-matrix row a fault here lands in.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::WriteIntent
            | Self::Create
            | Self::Start
            | Self::MountGitView
            | Self::Stop
            | Self::Remove
            | Self::UnmountGitView
            | Self::RemoveIntent => FaultRow::TContainer,
        }
    }

    /// Which claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::WriteIntent
            | Self::Create
            | Self::Start
            | Self::MountGitView
            | Self::Stop
            | Self::Remove
            | Self::UnmountGitView
            | Self::RemoveIntent => SiteScope::Topology,
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::WriteIntent
            | Self::Create
            | Self::Start
            | Self::MountGitView
            | Self::Stop
            | Self::Remove
            | Self::UnmountGitView
            | Self::RemoveIntent => false,
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::WriteIntent
            | Self::Create
            | Self::Start
            | Self::MountGitView
            | Self::Stop
            | Self::Remove
            | Self::UnmountGitView
            | Self::RemoveIntent => &[],
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::WriteIntent
            | Self::Create
            | Self::Start
            | Self::MountGitView
            | Self::Stop
            | Self::Remove
            | Self::UnmountGitView
            | Self::RemoveIntent => &[],
        }
    }

    /// The residue elements a class at this site must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::WriteIntent
            | Self::Create
            | Self::Start
            | Self::MountGitView
            | Self::Stop
            | Self::Remove
            | Self::UnmountGitView
            | Self::RemoveIntent => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every group's `name()` answers the spelling of the variant itself.
    ///
    /// The oracle is `stringify!` over the variant identifier, so nothing here
    /// is a second hand-written table that can be copied wrong the same way
    /// `name()` can. Each `match` is exhaustive over its enum, so a variant a
    /// list omits fails to compile and a variant listed twice is an unreachable
    /// arm the `-D warnings` leg refuses; the length check ties the list to
    /// `ALL`, so a walk cannot skip one either.
    ///
    /// Why it is worth a test of its own: the dotted name is the wire identity
    /// of a site, and `EffectSiteId::from_name` resolves a registry entry by
    /// it, so two sites of one group whose whole attribute profile agrees can
    /// have their names exchanged and every entry written against either then
    /// names the other. Eight such groups of sites exist -- among them
    /// `RunDir.RemovePrivateHusk` and `RunDir.RemovePublicHusk`, which agree on
    /// row, adjacency, fault row, scope, read-only, points, classes, elements,
    /// before state and after effect, and differ in that only one of the two
    /// halves is behind the ownership proof. Measured: exchanging that pair's
    /// names leaves every other test of `topology::effects` passing and this
    /// one failing, with the checked-in `effect_sites.json` comparison in
    /// `src/effects/tests.rs` failing beside it.
    ///
    /// `Worktree.Remove` and `Worktree.RemoveIntent` are *not* such a pair, and
    /// an earlier draft of this comment said they were: they differ in
    /// `after_effect` (`Released` against `Removed`), so two tests of
    /// `topology::effects::tests` catch that exchange as well.
    #[test]
    fn every_sites_name_is_the_spelling_of_its_own_variant() {
        macro_rules! spellings {
            ($enum:ident: $($variant:ident),+ $(,)?) => {{
                assert_eq!(
                    [$(stringify!($variant)),+].len(),
                    $enum::ALL.len(),
                    concat!(stringify!($enum), ": this list and `ALL` are different lengths"),
                );
                for site in $enum::ALL {
                    let spelling = match site {
                        $($enum::$variant => stringify!($variant),)+
                    };
                    assert_eq!(
                        site.name(),
                        spelling,
                        concat!(
                            stringify!($enum),
                            " answers a name that is not the spelling of its own variant",
                        ),
                    );
                }
            }};
        }

        spellings!(WorktreeSite:
            CreateExecutionRoot,
            RemoveExecutionRoot,
            WriteIntent,
            Add,
            Verify,
            Remove,
            RemoveIntent,
            WriteStagingIntent,
            AddStaging,
            RemoveStaging,
            RemoveStagingIntent,
        );
        spellings!(SnapshotSite: WriteIntent, Add, Remove, RemoveIntent);
        spellings!(RefSite:
            CreateIntegration,
            CompareAndSwapIntegration,
            CreateCandidates,
            DeleteCandidatesRef,
            PinCandidatePrepared,
            DeleteCandidatePin,
            PinPrepared,
            DeletePreparedPin,
        );
        spellings!(ObjectSite:
            CandidateStage,
            CandidateWriteTree,
            SnapshotCommitTree,
            CandidateCommitTree,
            ProposalCherryPick,
            RepairMaterialize,
        );
        spellings!(RunDirSite:
            CreatePublicDir,
            StageMarker,
            PublishMarker,
            RemoveMarker,
            CreatePrivateDir,
            StageOwnerRecord,
            PublishOwnerRecord,
            StageCommitRecord,
            PublishCommitRecord,
            WritePlan,
            WriteReport,
            WriteQuestionPayload,
            RemovePrivateHusk,
            RemovePublicHusk,
        );
        spellings!(EventSite:
            OpenLog,
            ProvePrefixStable,
            AppendFirst,
            Append,
            AppendInformational,
            LegacyOpenLog,
            LegacyAppend,
        );
        spellings!(AnswerSite: StageWrite, PublishRename, Ingest);
        spellings!(LockSite:
            AcquireRun,
            AcquireWorktree,
            ProbeCleanupExclusive,
            Release,
            CreateWorktreeLockFile,
            ObserveCleanupHold,
        );
        spellings!(ReportSite: Write);
        spellings!(ProcessSite: Spawn, Terminate);
        spellings!(ContainerSite:
            WriteIntent,
            Create,
            Start,
            MountGitView,
            Stop,
            Remove,
            UnmountGitView,
            RemoveIntent,
        );
    }

    /// A site lists residue elements exactly where it registers a residue
    /// class, and nine sites do.
    ///
    /// `residue_elements()` is read in one place,
    /// `registry::validate_entry`'s `Evidence::RecoveryProven` arm, and that
    /// arm is reachable only for an `EntryPhase::Residue { class }` the site
    /// registers. An element list on a site with no class is therefore data no
    /// consumer can reach and no reader can be held to. The suite asserts the
    /// other direction -- a registered class has a non-empty list -- and this
    /// one is the half nothing pinned.
    ///
    /// The count of sites with a class is asserted too, so the biconditional
    /// cannot be satisfied by emptying both sides.
    #[test]
    fn a_site_lists_residue_elements_exactly_where_it_registers_a_class() {
        let mut walked = 0_usize;
        let mut registering = 0_usize;

        macro_rules! coupled {
            ($($enum:ident),+ $(,)?) => {{
                $(
                    for site in $enum::ALL {
                        assert_eq!(
                            site.residue_elements().is_empty(),
                            site.residue_classes().is_empty(),
                            "{}.{} lists {} residue element(s) against {} residue class(es)",
                            stringify!($enum),
                            site.name(),
                            site.residue_elements().len(),
                            site.residue_classes().len(),
                        );
                        walked += 1;
                        if !site.residue_classes().is_empty() {
                            registering += 1;
                        }
                    }
                )+
            }};
        }

        coupled!(
            WorktreeSite,
            SnapshotSite,
            RefSite,
            ObjectSite,
            RunDirSite,
            EventSite,
            AnswerSite,
            LockSite,
            ReportSite,
            ProcessSite,
            ContainerSite,
        );

        assert_eq!(
            walked, 70,
            "the domain is every site of every group this file declares",
        );
        assert_eq!(
            registering, 9,
            "the two `Add` worktrees, the snapshot `Add`, and all six Object sites",
        );
    }
}
