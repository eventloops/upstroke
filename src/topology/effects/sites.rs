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
    /// `decisions/2026-08-30-test-scratch-tree-ownership.md` states the
    /// authority model in its two-token form.
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
/// One site. `report.json` is also named by [`RunDirSite::WriteReport`] in the
/// frozen inventory; both are implemented because both are named, and the two
/// are the same durable object reached through two funnels — see this module's
/// worker report for the note against the design.
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
    /// suite has to observe is decided by [`Platform::required_on`], not by
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
