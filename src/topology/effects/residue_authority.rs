//! The residue and recovery vocabulary, and the per-`(site, phase)` and
//! per-point authority that answers it.
//!
//! Split out of `topology::effects`; the parent re-exports every item here, so
//! `crate::topology::effects::ResidueClass` and its siblings are unchanged
//! paths. The `impl` blocks below are inherent impls of the site enums in
//! [`super::sites`] and of [`super::vocab::SubEffectPoint`] — the table stays
//! one table, in one place, exhaustive arm by exhaustive arm.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::sites::{
    AnswerSite, ContainerSite, EventSite, LockSite, ObjectSite, ProcessSite, RefSite, ReportSite,
    RunDirSite, SnapshotSite, WorktreeSite,
};
use super::vocab::{InjectionMode, ResourceRow, SubEffectPoint};

/// What `classify_object_residue` answers about one site's worktree.
///
/// Total over exactly these three for every [`ObjectSite`] and for
/// [`WorktreeSite::Add`] / [`SnapshotSite::Add`]. Totality is the property that
/// matters: a sampled residue that classifies into none of them fails ST-07,
/// because the run would then have durable state no tabled action recovers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectResidue {
    /// Nothing was written: the before-phase state.
    None,
    /// Objects written, their reference unpublished — the command-internal
    /// prefix no parent hook can observe.
    Internal,
    /// The object is present and referenced as the site's row says: the
    /// after-phase state.
    After,
}

impl ObjectResidue {
    /// The classifier's whole codomain.
    pub const ALL: &'static [Self] = &[Self::None, Self::Internal, Self::After];
}

/// A residue class an entry can be about.
///
/// One exists at design time. It is a separate type from [`ObjectResidue`] on
/// purpose: `ObjectResidue::None` and `ObjectResidue::After` are *outcomes of
/// the classifier*, not classes anything registers, and a registry keyed on the
/// classifier's codomain would let a slice register an entry for "nothing
/// happened" and count it as coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidueClass {
    /// `ObjectResidue::Internal`: objects written into the store before the
    /// command published their reference.
    ObjectInternal,
}

impl ResidueClass {
    /// Every registrable class.
    pub const ALL: &'static [Self] = &[Self::ObjectInternal];

    /// The classifier outcome this class is the class *of*.
    pub const fn classified_as(self) -> ObjectResidue {
        match self {
            Self::ObjectInternal => ObjectResidue::Internal,
        }
    }

    /// The label every entry about this class must carry.
    ///
    /// Constant, and constant on purpose: no residue class has, or can have,
    /// execution-observed evidence.
    pub const fn label(self) -> EvidenceLabel {
        match self {
            Self::ObjectInternal => EvidenceLabel::RecoveryProven,
        }
    }

    /// The class's name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ObjectInternal => "ObjectResidue::Internal",
        }
    }
}

/// One concrete artifact a residue class's synthetic construction must build.
///
/// The list comes from `command_internal_sub_effects` and from the fault
/// matrix's per-transaction residue descriptions; which elements a given site
/// can leave differs by command, which is why the list is per site rather than
/// per class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidueElement {
    /// An object in the store that nothing references (R27).
    UnreferencedObject,
    /// One of Git's own temporary object files; Git prunes them itself.
    TemporaryObjectFile,
    /// `index.lock` in the owning worktree's git dir.
    IndexLock,
    /// `CHERRY_PICK_HEAD`.
    CherryPickHead,
    /// `MERGE_HEAD`.
    MergeHead,
    /// `MERGE_MSG`.
    MergeMsg,
    /// `ORIG_HEAD`.
    OrigHead,
    /// Sequencer state left by an interrupted cherry-pick.
    SequencerState,
    /// A worktree Git registered but never populated.
    RegisteredUnpopulatedWorktree,
}

impl ResidueElement {
    /// Every element the classifier recognises.
    pub const ALL: &'static [Self] = &[
        Self::UnreferencedObject,
        Self::TemporaryObjectFile,
        Self::IndexLock,
        Self::CherryPickHead,
        Self::MergeHead,
        Self::MergeMsg,
        Self::OrigHead,
        Self::SequencerState,
        Self::RegisteredUnpopulatedWorktree,
    ];

    /// The class an element of this kind classifies into.
    ///
    /// Every one of them is `Internal`: that is what makes the classifier's
    /// answer a class rather than a list of files.
    pub const fn classifies_as(self) -> ObjectResidue {
        match self {
            Self::UnreferencedObject
            | Self::TemporaryObjectFile
            | Self::IndexLock
            | Self::CherryPickHead
            | Self::MergeHead
            | Self::MergeMsg
            | Self::OrigHead
            | Self::SequencerState
            | Self::RegisteredUnpopulatedWorktree => ObjectResidue::Internal,
        }
    }

    /// The element's name, spelled the way the wire form spells it.
    ///
    /// The vocabulary owns its own operator spelling. `SWEEP-WORKTREE-008`
    /// recorded the alternative: `worktree.rs`'s `Residue(element)` displays
    /// `{element:?}`, and `bijection.rs` writes `{element:?}` into three of its
    /// failures, so the words an operator reads are the derive's rather than
    /// chosen ones. Matching `#[serde(rename_all = "snake_case")]` means the
    /// name in a message and the name in a document are one name;
    /// `every_residue_element_displays_the_spelling_serde_writes` holds them
    /// together rather than leaving the two spellings free to drift.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::UnreferencedObject => "unreferenced_object",
            Self::TemporaryObjectFile => "temporary_object_file",
            Self::IndexLock => "index_lock",
            Self::CherryPickHead => "cherry_pick_head",
            Self::MergeHead => "merge_head",
            Self::MergeMsg => "merge_msg",
            Self::OrigHead => "orig_head",
            Self::SequencerState => "sequencer_state",
            Self::RegisteredUnpopulatedWorktree => "registered_unpopulated_worktree",
        }
    }
}

impl fmt::Display for ResidueElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name())
    }
}

/// How an entry's evidence was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLabel {
    /// A hook ran and the harness recorded it.
    ExecutionObserved,
    /// Nothing was executed: the residue was constructed and the tabled
    /// recovery converged. Never counted as an observed execution.
    RecoveryProven,
}

/// Which durable order a fault at a site can leave observable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservableOrder {
    /// The effect is durable, the adjacent append is not.
    EffectBeforeEvent,
    /// The adjacent append is durable, the effect is not.
    EventBeforeEffect,
}

// ---------------------------------------------------------------------------
// The residue and recovery vocabulary
// ---------------------------------------------------------------------------

/// What a site's *before* phase finds already durable.
///
/// The other per-site half of the residue authority, and the one a generic
/// answer got wrong. `EntryPhase::Before => rows: Vec::new()` reads as a
/// statement about effects in general — nothing has been performed, so nothing
/// is there — and it is false for every site whose primitive acts on something
/// that has to exist first. `transaction_fault_matrix[T-SCRUB]`, which is live
/// and binding, puts the boundary at "task_candidate_created appended;
/// worktree, its intent, or snapshots **not yet removed**": a fault at
/// `Worktree.Remove`'s before hook leaves the task worktree and its
/// administrative residue exactly where they were, held by R9. Under the
/// generic answer the framework refused that packet-correct entry
/// (`WrongResidueRows`) and accepted an entry claiming the worktree was
/// already gone — the inversion of what the registry exists to catch.
///
/// **Scope, and where it now stops.** A site's own artifact is not always one
/// object: eight of the seventy are the *second half of a two-step protocol
/// the packet names as a pair*, and after the first half the artifact exists
/// in its intermediate form. `transaction_fault_matrix[T-DISPATCH]` puts the
/// boundary at "worktree **intent** or worktree not yet created" and tables
/// the resume as "remove it with force and recreate it (**intent then add**)";
/// [`ResourceRow::R9`] is, in the ledger's own words, "Task worktree **plus
/// its durable synced intent**". So a kill at `Worktree.Add`'s before hook leaves
/// R9 holding that intent, and an entry saying the row holds nothing is false
/// — which is what [`Self::PrecursorDurable`] is for.
///
/// What these rows still do **not** name is the whole durable prefix of the
/// transaction the site sits in. `Event.Append`'s before phase names the line
/// it is about to append, not the log `Event.OpenLog` created;
/// `RunDir.CreatePrivateDir`'s names nothing, though the public directory and
/// its marker are durable and R21 accounts for both. That boundary is not a
/// preference. `structure` keys an entry by
/// `EffectSiteId x phase x order x injection mode` and by nothing else, and a
/// cumulative prefix is not a function of that key: `Event.Append` occurs at
/// every prefix of every transaction, `Worktree.Remove` occurs in T-SCRUB, in
/// T-ATTEMPT's resume and in T-FINALIZE's cleanup, and each of those is a
/// different prefix at the same coordinate. Naming a prefix here would need a
/// prefix axis the frozen key does not have. What *is* a function of the site
/// is its own artifact, including the intermediate state its own two-step
/// protocol leaves — because that ordering is invariant: the primitive cannot
/// add a worktree that no intent registered, and a rename cannot publish what
/// was never staged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeforeState {
    /// The artifact this site's primitive acts on does not exist yet in any
    /// form, so no row holds it: every one-step creation, and the read-only
    /// observations, which perform nothing at either phase.
    ///
    /// `structure` says it of the whole Object group in its own words —
    /// "Object sites carry entries — before: no object (hook)".
    Absent,
    /// The artifact does not exist yet, and the first half of this site's own
    /// two-step protocol has already left a durable artifact that the row
    /// [`EffectSiteId::row`](crate::topology::effects::EffectSiteId::row) names accounts for: the intent behind an add, the
    /// staged temporary behind an atomic publication.
    ///
    /// The rows are [`Self::Present`]'s and the words are not, deliberately.
    /// The row holds something, so the entry must say so; the thing it holds
    /// is not the target intact, so the entry must not say that either.
    PrecursorDurable,
    /// The artifact this site's primitive acts on is already durable and the
    /// row [`EffectSiteId::row`](crate::topology::effects::EffectSiteId::row) names holds it: every removal, every release,
    /// and every in-place replacement of an artifact that has to exist for the
    /// primitive to be issued at all.
    Present,
}

/// What a site's *after* phase leaves durable.
///
/// The per-site half of the residue authority, and the reason
/// [`EffectSiteId::semantics`](crate::topology::effects::EffectSiteId::semantics) has no generic arm. `structure` does not give
/// every site the same after-phase: an effect that publishes something leaves
/// it referenced by the site's own row, a commit-tree leaves an object nothing
/// references, "the pruning sites' after-phase entries record the released
/// objects as R27 residue", a removal that releases nothing leaves the row
/// that accounted for what it removed holding nothing, and a momentary hold is
/// given back before the command returns. One `vec![self.row()]`
/// answers all six the same way and is wrong for five of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AfterEffect {
    /// The site performs no effect at all, so its after phase leaves nothing.
    NoEffect,
    /// The artifact is durable and the row [`EffectSiteId::row`](crate::topology::effects::EffectSiteId::row) names
    /// references it.
    Referenced,
    /// The object is durable and nothing references it yet: R27.
    Unreferenced,
    /// The removal is durable, releasing the objects it referenced to R27 and
    /// taking its administrative residue with it.
    Released,
    /// The removal is durable and released no object: the row that accounted
    /// for what it removed holds nothing.
    Removed,
    /// The site took a hold and gave it back before it returned, so no row is
    /// left holding it and a resume repeats the probe.
    ///
    /// Distinct from [`Self::NoEffect`], which is the read-only claim: this
    /// site does perform an effect (it creates the lock file it probes) and is
    /// classified `is_read_only() == false`. Distinct from [`Self::Referenced`]
    /// because the hold is deliberately not retained — see
    /// [`LockSite::after_effect`].
    MomentaryHold,
}

/// The concrete artifacts a fault at one `(site, phase)` leaves, in the fault
/// matrix's own words.
///
/// `structure` requires each entry to record "the expected residue
/// (refs/worktrees/pins/intents/containers/marker, owner-record, and
/// commit-record files/objects and the row referencing them/administrative
/// residue)". An entry free to write that prose itself is a second authority on
/// the same question — the argument [`EffectSiteId::expected_rows`](crate::topology::effects::EffectSiteId::expected_rows) already
/// makes about the rows, and the rows were only half the claim. So the artifact
/// is a value, [`Self::detail`] is its words, and [`ExpectedResidue`](crate::topology::effects::ExpectedResidue)'s own
/// detail is checked against them rather than being read by nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidueArtifact {
    /// The before phase of a site whose primitive brings its own artifact into
    /// existence — [`BeforeState::Absent`].
    Nothing,
    /// The before phase of a site whose primitive acts on something that is
    /// already there — [`BeforeState::Present`].
    TargetIntact,
    /// The before phase of a site whose own two-step protocol has already made
    /// its first half durable — [`BeforeState::PrecursorDurable`].
    PrecursorDurable,
    /// The no-execution record: the site was never reached.
    NotReached,
    /// The after phase of a read-only observation.
    NoEffectPerformed,
    /// The artifact is present and the site's own row references it.
    Referenced,
    /// The object is present and unreferenced.
    Unreferenced,
    /// A pruning site's after phase: the objects it referenced are released.
    Released,
    /// A removal that released no object.
    Removed,
    /// The `IdUnread` point of the two commit-tree sites.
    IdNotRecorded,
    /// The `Internal` residue class at a site whose own row is R27.
    ObjectsUnreferenced,
    /// The `Internal` residue class at a site with an owning worktree.
    ObjectsAndAdministrativeResidue,
    /// The `Written` append point.
    UnsyncedBytes,
    /// The `WrittenFull` append point.
    UnsyncedLine,
    /// The `Synced` append point.
    SyncedLine,
    /// `Event.OpenLog`'s `Create` point.
    LogCreated,
    /// `Event.OpenLog`'s `TruncateTornTail` point.
    TornTailTruncated,
    /// `Event.OpenLog`'s `SyncPrefix` point.
    PrefixPossiblyNonDurable,
    /// A Windows containment point a coordinator kill leaves.
    NoHostProcess,
    /// The Windows ambient join's *error* contract: the refusal precedes the
    /// join, so nothing was spawned and nothing was terminated.
    ///
    /// A separate artifact from [`Self::NoHostProcess`] because the two modes
    /// are at different states. `agent::proc::ambient::join_ambient_job_with`
    /// applies the error-return hook *before* `join()` and the kill hook
    /// *after* it, and refuses with "No process was spawned": no handle closes
    /// and the kernel terminates nothing. `NoHostProcess`'s own words name a
    /// mechanism that did not run.
    NoProcessSpawned,
    /// A Unix containment point.
    ReaperHeldGroup,
    /// A momentary hold the site took and gave back before it returned.
    HoldReleased,
}

impl ResidueArtifact {
    /// Every artifact, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::Nothing,
        Self::TargetIntact,
        Self::PrecursorDurable,
        Self::NotReached,
        Self::NoEffectPerformed,
        Self::Referenced,
        Self::Unreferenced,
        Self::Released,
        Self::Removed,
        Self::IdNotRecorded,
        Self::ObjectsUnreferenced,
        Self::ObjectsAndAdministrativeResidue,
        Self::UnsyncedBytes,
        Self::UnsyncedLine,
        Self::SyncedLine,
        Self::LogCreated,
        Self::TornTailTruncated,
        Self::PrefixPossiblyNonDurable,
        Self::NoHostProcess,
        Self::NoProcessSpawned,
        Self::ReaperHeldGroup,
        Self::HoldReleased,
    ];

    /// The words an entry's `expected_residue.detail` must carry.
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Nothing => "nothing has been performed, so no row holds anything",
            Self::TargetIntact => {
                "nothing has been performed: the artifact this site acts on is present and \
                 unchanged, held by the row row() names"
            }
            Self::PrecursorDurable => {
                "nothing has been performed: the artifact this site creates does not exist, and \
                 the durable first half of its own two-step protocol — the intent behind an add, \
                 the staged temporary behind an atomic publication — is held by the row row() \
                 names"
            }
            Self::NotReached => {
                "the site was not reached: an exact-base fast publication creates no staging \
                 worktree, cherry-picks nothing, and takes no prepared pin"
            }
            Self::NoEffectPerformed => {
                "no effect was performed: the site is a read-only observation"
            }
            Self::Referenced => "the artifact is present and referenced by the row row() names",
            Self::Unreferenced => "the object is present and unreferenced, R27",
            Self::Released => {
                "the removal is durable; the objects it referenced are released to R27 and its \
                 administrative residue went with it"
            }
            Self::Removed => {
                "the removal is durable and released no object; the row that accounted for what \
                 it removed holds nothing"
            }
            Self::IdNotRecorded => {
                "an R27 object without a recorded id: the child has exited with the object \
                 written and the coordinator has not read the printed id"
            }
            Self::ObjectsUnreferenced => "objects present and unreferenced, R27",
            Self::ObjectsAndAdministrativeResidue => {
                "objects present and unreferenced, R27, with administrative residue in the owning \
                 worktree or a registered-but-unpopulated worktree"
            }
            Self::UnsyncedBytes => "bytes written, possibly partially, and not synced",
            Self::UnsyncedLine => "a complete newline-terminated line written and not synced",
            Self::SyncedLine => "the appended line is synced",
            Self::LogCreated => "the log file exists and its directory is fsynced",
            Self::TornTailTruncated => "the unterminated final line is truncated",
            Self::PrefixPossiblyNonDurable => "the surviving prefix is possibly non-durable",
            Self::NoHostProcess => {
                "no host process: the ambient handle closes and the kernel terminates the stub or \
                 tree"
            }
            Self::NoProcessSpawned => {
                "no host process: the refusal precedes the ambient join, so nothing was spawned \
                 and nothing was terminated"
            }
            Self::ReaperHeldGroup => {
                "a process group the reaper settles while holding its shared cleanup hold, R28"
            }
            Self::HoldReleased => {
                "the momentary hold was taken and given back before the command returned, so no \
                 row is left holding it"
            }
        }
    }
}

impl fmt::Display for ResidueArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.detail())
    }
}

/// What a resume does about this entry's durable residue.
///
/// `structure` requires "the tabled resume action" per entry, and before this
/// type the format required only that the string be non-blank — so an entry
/// could table a recovery the matrix does not give it and read as accounted
/// for. The recovery is a value for the same reason the residue is: the site
/// and the phase decide it, not the document that reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeAction {
    /// The before-phase action: resume from the prefix in which nothing was
    /// performed. `structure` gives this to `IdUnread` and to the `Internal`
    /// residue class in the same words.
    ResumeUnperformed,
    /// Nothing ran at all.
    NotExecuted,
    /// The effect is durable: the resume adopts it.
    AdoptPerformed,
    /// The removal is durable and released objects Git prunes.
    ReclaimReleased,
    /// A read-only observation: it is repeated, and there is nothing to undo.
    RepeatObservation,
    /// The append-error protocol, with the barrier at its reopen.
    AppendErrorProtocol,
    /// No live action: the next open converges the surviving prefix.
    NextOpenConverges,
    /// The write command refuses resumably, and the next open repeats the
    /// barrier.
    RefuseResumably,
    /// A Windows containment point: the ambient handle closes.
    AmbientHandleTerminates,
    /// A Unix containment point: the reaper settles the group.
    ReaperSettlesGroup,
    /// The Windows ambient join's error contract: the write command refuses
    /// before any process exists.
    ///
    /// Not [`Self::RefuseResumably`], whose words are the event log's — "the
    /// next open repeats the barrier" is the stable-prefix barrier, and a job
    /// object has no open and no barrier.
    RefuseUnspawned,
    /// A momentary hold: it was given back before the command returned, and a
    /// resume repeats the probe.
    RepeatProbe,
}

impl ResumeAction {
    /// Every action, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::ResumeUnperformed,
        Self::NotExecuted,
        Self::AdoptPerformed,
        Self::ReclaimReleased,
        Self::RepeatObservation,
        Self::AppendErrorProtocol,
        Self::NextOpenConverges,
        Self::RefuseResumably,
        Self::AmbientHandleTerminates,
        Self::ReaperSettlesGroup,
        Self::RefuseUnspawned,
        Self::RepeatProbe,
    ];

    /// The words an entry's `resume_action` must carry.
    pub const fn text(self) -> &'static str {
        match self {
            Self::ResumeUnperformed => {
                "resume from the prefix in which nothing was performed: the site's before-phase \
                 action"
            }
            Self::NotExecuted => "nothing to resume: the site performed no effect",
            Self::AdoptPerformed => "resume adopting the completed effect",
            Self::ReclaimReleased => {
                "resume adopting the completed removal; the released objects are unreferenced and \
                 Git prunes them"
            }
            Self::RepeatObservation => {
                "nothing to resume: the observation performs no effect and is repeated"
            }
            Self::AppendErrorProtocol => {
                "the append-error protocol, with the stable-prefix barrier at its reopen"
            }
            Self::NextOpenConverges => {
                "no live action: the next open converges the surviving prefix through its \
                 stable-prefix barrier before any fold-derived effect"
            }
            Self::RefuseResumably => {
                "the write command refuses resumably with no fold-derived effect, and the next \
                 open repeats the barrier"
            }
            Self::AmbientHandleTerminates => {
                "nothing to resume: the ambient handle closes and the kernel terminates the stub \
                 or tree"
            }
            Self::ReaperSettlesGroup => {
                "nothing to resume: the reaper settles the group while holding its cleanup hold"
            }
            Self::RefuseUnspawned => {
                "nothing to resume: the write command refuses before the ambient join, and no \
                 process was spawned"
            }
            Self::RepeatProbe => {
                "nothing to resume: the momentary hold was given back before the command \
                 returned, and the probe is repeated"
            }
        }
    }
}

impl fmt::Display for ResumeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

/// The residue and recovery semantics of one `(site, phase)` — the whole of
/// what a registry entry may claim about them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseSemantics {
    /// The ledger rows still holding something.
    pub rows: Vec<ResourceRow>,
    /// The concrete artifacts.
    pub artifact: ResidueArtifact,
    /// The tabled recovery.
    pub action: ResumeAction,
}

// ---------------------------------------------------------------------------
// The per-site and per-point residue authority
// ---------------------------------------------------------------------------
//
// One table, in one place, and every arm of it is checked exhaustive by rustc
// because each `match` is over a concrete site enum and carries no wildcard.
// That is the exhaustiveness argument: a twelfth group, or a twelfth variant of
// any of the eleven, does not compile until someone says what its after phase
// leaves. The alternative the framework shipped with — one
// `After | Point => vec![self.row()]` arm over `EffectSiteId` — is a table that
// answers for sites nobody has classified, which is how
// `Worktree.Remove`'s after phase came to claim R9 for a worktree that had
// just been removed.

impl WorktreeSite {
    /// What this site's before phase finds already durable.
    ///
    /// The five removals act on something that has to be there:
    /// `transaction_fault_matrix[T-SCRUB]` puts `Remove` and `RemoveIntent` at
    /// "worktree, its intent, or snapshots not yet removed"; T-PROPOSAL's
    /// resume — "reclaim the staging worktree residue with force (intent then
    /// worktree, incl. any administrative residue)" — puts `RemoveStaging` and
    /// `RemoveStagingIntent` at a staging worktree that exists; and
    /// T-FINALIZE's "cleanup steps (worktrees, snapshots, staging, pins,
    /// candidates refs at Complete, execution root) partially applied" puts
    /// `RemoveExecutionRoot` at a root still there. The creations create —
    /// T-DISPATCH's boundary is "worktree intent or worktree not yet created"
    /// and T-FAST's is "no staging worktree, intent, cherry-pick, object, or
    /// pin exists at any point of a fast sequence" — and `Verify` performs no
    /// effect at either phase.
    ///
    /// The two *adds* are neither. Each is the second half of a pair the
    /// packet names as a pair — T-DISPATCH's resume is "recreate it (intent
    /// then add)" and T-PROPOSAL's is "reclaim the staging worktree residue
    /// with force (intent then worktree, incl. any administrative residue)" —
    /// and their rows account for the first half by name: R9 is "Task
    /// worktree plus its durable synced intent" and R10 is "Staging worktree
    /// `merge/<seq>` plus its intent". A kill at either add's before hook
    /// leaves that intent, and the row it leaves it in is this site's own.
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::RemoveExecutionRoot
            | Self::Remove
            | Self::RemoveIntent
            | Self::RemoveStaging
            | Self::RemoveStagingIntent => BeforeState::Present,
            Self::Add | Self::AddStaging => BeforeState::PrecursorDurable,
            Self::CreateExecutionRoot
            | Self::WriteIntent
            | Self::Verify
            | Self::WriteStagingIntent => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// The two forced removals are pruning sites: `Remove` releases "its
    /// index-referenced objects to R27 and takes its administrative residue
    /// with it", and `RemoveStaging` does the same for the staging worktree's
    /// cherry-pick objects (`effect_phases_covered`: "worktree/staging/snapshot
    /// intents and adds and removals (forced; with the objects they referenced
    /// released to R27 and administrative residue removed)"). The intent
    /// removals and the empty execution root release nothing.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::Verify => AfterEffect::NoEffect,
            Self::CreateExecutionRoot
            | Self::WriteIntent
            | Self::Add
            | Self::WriteStagingIntent
            | Self::AddStaging => AfterEffect::Referenced,
            Self::Remove | Self::RemoveStaging => AfterEffect::Released,
            Self::RemoveExecutionRoot | Self::RemoveIntent | Self::RemoveStagingIntent => {
                AfterEffect::Removed
            }
        }
    }
}

impl SnapshotSite {
    /// What this site's before phase finds already durable.
    ///
    /// `transaction_fault_matrix[T-SCRUB]`: "worktree, its intent, or
    /// snapshots not yet removed" — both removals find their snapshot there,
    /// held by R24.
    ///
    /// `Add` is the second half of its own pair: T-ATTEMPT (d) is "snapshot
    /// intent written **and** snapshot worktree added", and R24 is "Exact
    /// gate/review snapshot worktree plus its intent".
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::Remove | Self::RemoveIntent => BeforeState::Present,
            Self::Add => BeforeState::PrecursorDurable,
            Self::WriteIntent => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// `Remove` is a pruning site: "Forced removal, releasing an ephemeral
    /// commit back to R27."
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::WriteIntent | Self::Add => AfterEffect::Referenced,
            Self::Remove => AfterEffect::Released,
            Self::RemoveIntent => AfterEffect::Removed,
        }
    }
}

impl RefSite {
    /// What this site's before phase finds already durable.
    ///
    /// The three deletions delete something that exists: T-CAND-REF's boundary
    /// leaves the candidate pin "not yet pruned", T-VERIFY's resume is "delete
    /// pin expected-old", and T-FINALIZE's cleanup steps name "pins,
    /// candidates refs at Complete".
    ///
    /// `CompareAndSwapIntegration` is the group's one in-place replacement and
    /// the one non-deletion here: T-FAST's boundary is "assert_publishable read
    /// the integration ref head H == candidate.base_sha", and a CAS is issued
    /// against that existing head, which R21 holds. The creations create, and
    /// the matrix says so of each — "no ref until P8" (T-RUNSTART), "candidates
    /// ref (R11) missing" (T-CAND-REF), "no pin exists" (T-CAND-OBJ), "no
    /// `prepared/<seq>` pin" (T-PROPOSAL).
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::CompareAndSwapIntegration
            | Self::DeleteCandidatesRef
            | Self::DeleteCandidatePin
            | Self::DeletePreparedPin => BeforeState::Present,
            Self::CreateIntegration
            | Self::CreateCandidates
            | Self::PinCandidatePrepared
            | Self::PinPrepared => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// `identity` names `Ref.Delete*` among the pruning sites, so all three
    /// deletions release what their ref referenced to R27.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::CreateIntegration
            | Self::CompareAndSwapIntegration
            | Self::CreateCandidates
            | Self::PinCandidatePrepared
            | Self::PinPrepared => AfterEffect::Referenced,
            Self::DeleteCandidatesRef | Self::DeleteCandidatePin | Self::DeletePreparedPin => {
                AfterEffect::Released
            }
        }
    }
}

impl ObjectSite {
    /// What this site's before phase finds already durable.
    ///
    /// Nothing, for the whole group, and `structure` says it in its own words:
    /// "Object sites carry entries — before: no object (hook)".
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::CandidateStage
            | Self::CandidateWriteTree
            | Self::SnapshotCommitTree
            | Self::CandidateCommitTree
            | Self::ProposalCherryPick
            | Self::RepairMaterialize => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// `structure`, exactly: "after: the object present and referenced by the
    /// row named by `row()`, or unreferenced R27 for the commit-tree sites".
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::CandidateStage
            | Self::CandidateWriteTree
            | Self::ProposalCherryPick
            | Self::RepairMaterialize => AfterEffect::Referenced,
            Self::SnapshotCommitTree | Self::CandidateCommitTree => AfterEffect::Unreferenced,
        }
    }
}

impl RunDirSite {
    /// What this site's before phase finds already durable.
    ///
    /// The three removals. `transaction_fault_matrix[T-RUNSTART]` walks its
    /// prefixes "P6 run_started durable ..., marker still present; P7 marker
    /// removed", so `RemoveMarker` finds its marker there; a husk removal is
    /// the removal of a husk that exists, and `identity` gives
    /// `RemovePrivateHusk` a proof token that "returns no token when
    /// committed.json exists" — a token about a private half that is there.
    ///
    /// Every other site of the group writes or publishes a file of its own,
    /// `WriteReport` included: T-FINALIZE regenerates the report "if missing or
    /// stale", and the primitive writes it either way rather than requiring one
    /// to be there.
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::RemoveMarker | Self::RemovePrivateHusk | Self::RemovePublicHusk => {
                BeforeState::Present
            }
            // The three atomic publications, each renaming a temporary its
            // own staging site made durable: T-RUNSTART's "P1 marker staged
            // (.creating.tmp) **or** published (.creating ...)", and
            // `effect_phases_covered`'s "marker staging and atomic
            // publication", "private-half creation and atomic owner-record
            // publication", "private commit-record staging and atomic
            // publication". R21 holds the staged temporary as it holds the
            // published record — "committed.json.tmp leaves with the private
            // half".
            Self::PublishMarker | Self::PublishOwnerRecord | Self::PublishCommitRecord => {
                BeforeState::PrecursorDurable
            }
            // `CreatePrivateDir` is *not* one of them, and the difference is
            // the whole boundary of this classification: the public directory
            // and its marker are durable at P3a and R21 accounts for both, but
            // neither is an earlier state of the private directory. A before
            // phase names the site's own artifact, not the transaction's
            // prefix.
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::StageCommitRecord
            | Self::WritePlan
            | Self::WriteReport
            | Self::WriteQuestionPayload => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// Run-directory contents are files, not Git objects, so the three
    /// removals release nothing to R27; they leave R21 holding nothing of what
    /// they removed.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::CreatePublicDir
            | Self::StageMarker
            | Self::PublishMarker
            | Self::CreatePrivateDir
            | Self::StageOwnerRecord
            | Self::PublishOwnerRecord
            | Self::StageCommitRecord
            | Self::PublishCommitRecord
            | Self::WritePlan
            | Self::WriteReport
            | Self::WriteQuestionPayload => AfterEffect::Referenced,
            Self::RemoveMarker | Self::RemovePrivateHusk | Self::RemovePublicHusk => {
                AfterEffect::Removed
            }
        }
    }
}

impl EventSite {
    /// What this site's before phase finds already durable.
    ///
    /// Nothing, group-wide. Each append brings its own line into existence and
    /// requires no previous one — T-APPEND's durable shapes are all shapes of
    /// the line being appended. `OpenLog` "create\[s\] the log if absent", so its
    /// primitive does not require the log either, and `ProvePrefixStable`
    /// performs no effect at either phase.
    ///
    /// The log R21 accounts for is `OpenLog`'s own after-phase claim; a before
    /// phase that repeated it would make every append restate the open.
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::OpenLog
            | Self::ProvePrefixStable
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational
            | Self::LegacyOpenLog
            | Self::LegacyAppend => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// `ProvePrefixStable` is the read-only half of the stable-prefix barrier
    /// and performs no effect; every other site of the group leaves the log,
    /// R21.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::ProvePrefixStable => AfterEffect::NoEffect,
            Self::OpenLog
            | Self::AppendFirst
            | Self::Append
            | Self::AppendInformational
            | Self::LegacyOpenLog
            | Self::LegacyAppend => AfterEffect::Referenced,
        }
    }
}

impl AnswerSite {
    /// What this site's before phase finds already durable.
    ///
    /// Nothing: `T-ANSWER`'s boundary is "answer staged as
    /// `answers/<qid>.json.partial`, **or** published as `answers/<qid>.json`",
    /// two artifacts and one site each, and `Ingest` performs no effect.
    pub const fn before_state(self) -> BeforeState {
        match self {
            // `effect_phases_covered`: "answer staging (.partial) **and**
            // publication by the answer command". The rename publishes the
            // `.partial` the stage wrote, and R21 holds it either way.
            Self::PublishRename => BeforeState::PrecursorDurable,
            Self::StageWrite | Self::Ingest => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::StageWrite | Self::PublishRename => AfterEffect::Referenced,
            Self::Ingest => AfterEffect::NoEffect,
        }
    }
}

impl LockSite {
    /// What this site's before phase finds already durable.
    ///
    /// `Release` releases a hold that is held, and R17 is "the coordinator's
    /// own lock holds (OS lock state only)" — it accounts for that hold until
    /// the release ends it. The acquisitions and the lock-file creation create.
    /// `ObserveCleanupHold` performs no effect: the R28 hold it observes is a
    /// surviving reaper's, never this coordinator's to leave behind.
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::Release => BeforeState::Present,
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::CreateWorktreeLockFile
            | Self::ObserveCleanupHold => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// A hold is process-local OS state the row accounts for while it is held;
    /// `Release` ends it and releases no object.
    ///
    /// `ProbeCleanupExclusive` is the one site of the group that ends its own
    /// hold. `rundir`'s `cleanup::take` takes `LOCK_EX | LOCK_NB` and then
    /// `LOCK_UN` before it returns, and says why in its own words — "Do not
    /// retain the lock in the conductor: arbitrary forked children would
    /// inherit its open file description and recreate the false-liveness
    /// window" — returning a lease that is a path and no hold. R17 is "the
    /// coordinator's own lock holds (**OS lock state only**)", so after this
    /// site's after hook R17 holds nothing of what the probe took, and a
    /// resume cannot "adopt the completed effect": a new process holds no
    /// cleanup lock and re-probes. `AcquireRun` and `AcquireWorktree` are the
    /// contrast that makes this a site distinction and not an argument about
    /// locks in general — each retains its `File` for the lifetime of the
    /// guard, so R17 does hold their hold at their after hook.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::AcquireRun | Self::AcquireWorktree | Self::CreateWorktreeLockFile => {
                AfterEffect::Referenced
            }
            Self::ProbeCleanupExclusive => AfterEffect::MomentaryHold,
            Self::Release => AfterEffect::Removed,
            Self::ObserveCleanupHold => AfterEffect::NoEffect,
        }
    }
}

impl ReportSite {
    /// What this site's before phase finds already durable.
    ///
    /// Nothing: the write produces the report it writes.
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::Write => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::Write => AfterEffect::Referenced,
        }
    }
}

impl ProcessSite {
    /// What this site's before phase finds already durable.
    ///
    /// `Terminate` terminates a process that is running, and R22 — "host
    /// process handle / private job object / ambient job membership" —
    /// accounts for it until it ends. `Spawn` creates one.
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::Terminate => BeforeState::Present,
            Self::Spawn => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::Spawn => AfterEffect::Referenced,
            Self::Terminate => AfterEffect::Removed,
        }
    }
}

impl ContainerSite {
    /// What this site's before phase finds already durable.
    ///
    /// `transaction_fault_matrix[T-CONTAINER]` walks the boundary in order:
    /// "global container intent written (name incl. incarnation; record incl.
    /// runner_policy_sha256); container created from the recorded image id and
    /// verified; docker start issued; Git view mounted; the invocation running
    /// or completed; coordinator dies at any of these points". So `Start`,
    /// `Stop` and `Remove` are each issued against a container that exists —
    /// R26 accounts for "the container, its labels, and its global intent" —
    /// `RemoveIntent` against the written intent, and `UnmountGitView` against
    /// the mounted view, R19. `Create` creates the container; `WriteIntent` and
    /// `MountGitView` each bring their own artifact into existence.
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::Start | Self::Stop | Self::Remove | Self::UnmountGitView | Self::RemoveIntent => {
                BeforeState::Present
            }
            // `effect_phases_covered`: "container intent write (name incl.
            // incarnation; record incl. runner digest), container creation
            // from the recorded image id with image-id verification", and R26
            // is "Container invocation: the container, its labels, and its
            // **global intent**".
            Self::Create => BeforeState::PrecursorDurable,
            Self::WriteIntent | Self::MountGitView => BeforeState::Absent,
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// A stopped container is still a container: `Stop` leaves the row holding
    /// it, and only `Remove` ends it.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::WriteIntent | Self::Create | Self::Start | Self::MountGitView | Self::Stop => {
                AfterEffect::Referenced
            }
            Self::Remove | Self::UnmountGitView | Self::RemoveIntent => AfterEffect::Removed,
        }
    }
}

impl SubEffectPoint {
    /// The rows a fault at this point leaves holding something, given the
    /// site's own row.
    ///
    /// A list, and not one row, because two of the four answers the packet
    /// gives are not the site's row at all and one of them is *no* row. The
    /// predecessor of this function returned `site_row` for every point but
    /// `IdUnread`, so `Process.Spawn`'s eight containment points all claimed
    /// R22 — while their own `residue_artifact` said `NoHostProcess` on
    /// Windows and `ReaperHeldGroup` ("... its shared cleanup hold, R28") on
    /// Unix. The registry read as accounted for while its two halves
    /// contradicted each other at every containment coordinate.
    ///
    /// The four answers, each in `containment_sub_effects`' or `structure`'s
    /// own words:
    ///
    /// * `IdUnread` — "R27 object without a recorded id". Stated here rather
    ///   than inherited from `row()`, which happens to be R27 for both
    ///   commit-tree sites today — a coincidence that let a mutation move
    ///   `SnapshotCommitTree` to R24 without a test noticing.
    /// * the append and open points — the log the site's own row accounts for.
    /// * the Windows containment points — "a coordinator kill after any of
    ///   these leaves **no host process** (the ambient handle closes and the
    ///   kernel terminates the stub or tree; a private-job handle close does
    ///   the same)". No row holds anything: R22 is the row for a host process
    ///   handle, and there is none.
    /// * the Unix containment points — "a coordinator kill after any of these
    ///   leaves a group the reaper settles **while holding R28**". R28 is
    ///   "a surviving Unix reaper's shared `cleanup.lock` hold"; R22 is not
    ///   left holding a handle the dying coordinator owned.
    ///
    /// The platform is not a new axis of the authority: it is already a
    /// function of the point ([`Self::platform`]), and this match is over the
    /// same fifteen variants with no wildcard, so it stays total by
    /// exhaustiveness rather than by a default.
    pub fn residue_rows(self, site_row: ResourceRow) -> Vec<ResourceRow> {
        match self {
            Self::IdUnread => vec![ResourceRow::R27],
            Self::Written
            | Self::WrittenFull
            | Self::Synced
            | Self::Create
            | Self::TruncateTornTail
            | Self::SyncPrefix => vec![site_row],
            Self::AmbientJobJoined
            | Self::CreatedSuspended
            | Self::PrivateJobAssigned
            | Self::Resumed => Vec::new(),
            Self::ReaperStarted | Self::PreExecPgidAndRegister | Self::Exec | Self::Registered => {
                vec![ResourceRow::R28]
            }
        }
    }

    /// The artifacts a fault at this point in this mode leaves.
    ///
    /// The mode is half the coordinate here for the same reason it is half of
    /// [`Self::resume_action`]'s, and this half was left behind: every point
    /// but one leaves the same durable shape whichever way the fault arrives —
    /// an `Err` injected *at* `Create` still created the log, and a partial
    /// write is a partial write however it ended — but `AmbientJobJoined` does
    /// not. `agent::proc::ambient::join_ambient_job_with` applies the
    /// error-return hook **before** `join()` and the kill hook **after** it, so
    /// a kill leaves the ambient handle to close and the kernel to terminate
    /// the stub or tree, while an `Err` refuses with "No process was spawned"
    /// and terminates nothing. One artifact for both gave the error-return
    /// coordinate a kill's mechanism, which is the same contradiction between
    /// an entry's two halves that [`Self::residue_rows`] was written to end.
    pub const fn residue_artifact(self, mode: InjectionMode) -> ResidueArtifact {
        match self {
            Self::IdUnread => ResidueArtifact::IdNotRecorded,
            Self::Written => ResidueArtifact::UnsyncedBytes,
            Self::WrittenFull => ResidueArtifact::UnsyncedLine,
            Self::Synced => ResidueArtifact::SyncedLine,
            Self::Create => ResidueArtifact::LogCreated,
            Self::TruncateTornTail => ResidueArtifact::TornTailTruncated,
            Self::SyncPrefix => ResidueArtifact::PrefixPossiblyNonDurable,
            Self::AmbientJobJoined => match mode {
                InjectionMode::Kill => ResidueArtifact::NoHostProcess,
                InjectionMode::ErrorReturn => ResidueArtifact::NoProcessSpawned,
            },
            Self::CreatedSuspended | Self::PrivateJobAssigned | Self::Resumed => {
                ResidueArtifact::NoHostProcess
            }
            Self::ReaperStarted | Self::PreExecPgidAndRegister | Self::Exec | Self::Registered => {
                ResidueArtifact::ReaperHeldGroup
            }
        }
    }

    /// The tabled recovery for a fault at this point in this mode.
    ///
    /// The mode is part of the coordinate, not a decoration on it: an `Err`
    /// from an append point drives the append-error protocol, and a kill at the
    /// same point drives nothing live at all — "a fully written but unsynced
    /// line converges to whichever tabled prefix survives the next open". A
    /// table that answered per point and ignored the mode would give a kill the
    /// live protocol only an error contract has.
    ///
    /// Total over every `(point, mode)` pair, including the pairs
    /// [`Self::modes`] does not admit: the format refuses those separately
    /// (`RegistryError::NoSuchPoint`), and a partial table here would be a
    /// second place for an unsupported mode to be decided.
    pub const fn resume_action(self, mode: InjectionMode) -> ResumeAction {
        match self {
            // "resume action = the before-phase action"
            Self::IdUnread => ResumeAction::ResumeUnperformed,
            Self::Written | Self::WrittenFull | Self::Synced => match mode {
                InjectionMode::Kill => ResumeAction::NextOpenConverges,
                InjectionMode::ErrorReturn => ResumeAction::AppendErrorProtocol,
            },
            // A kill leaves the created log or the truncated tail for the next
            // open; an Err from either fails the open itself.
            Self::Create | Self::TruncateTornTail => match mode {
                InjectionMode::Kill => ResumeAction::NextOpenConverges,
                InjectionMode::ErrorReturn => ResumeAction::RefuseResumably,
            },
            // "a kill before it or an Err from it ... the command refuses
            // resumably, and the next open repeats the barrier" — one action
            // for both modes, and the packet says so of both.
            Self::SyncPrefix => ResumeAction::RefuseResumably,
            // "Spawn.AmbientJobJoined (once per process at startup; failure
            // refuses the write command)". The refusal is not
            // `RefuseResumably`: that action's words end "the next open repeats
            // the barrier", which is the event log's stable-prefix barrier, and
            // an ambient join has neither an open nor a barrier. What it does
            // have is its own refusal, `AMBIENT_REFUSAL_PREFIX` + "No process
            // was spawned".
            Self::AmbientJobJoined => match mode {
                InjectionMode::Kill => ResumeAction::AmbientHandleTerminates,
                InjectionMode::ErrorReturn => ResumeAction::RefuseUnspawned,
            },
            Self::CreatedSuspended | Self::PrivateJobAssigned | Self::Resumed => {
                ResumeAction::AmbientHandleTerminates
            }
            Self::ReaperStarted | Self::PreExecPgidAndRegister | Self::Exec | Self::Registered => {
                ResumeAction::ReaperSettlesGroup
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        AfterEffect, BeforeState, InjectionMode, LockSite, ResidueArtifact, ResidueElement,
        ResumeAction, SubEffectPoint,
    };
    use crate::topology::effects::{EffectSiteId, EntryPhase};

    #[test]
    fn every_residue_element_displays_the_spelling_serde_writes() {
        // The spelling in an operator message and the spelling in a document
        // are one spelling, and this is what keeps them one: a `wire_name` arm
        // edited away from its serde name fails here rather than surfacing as
        // two vocabularies for one element.
        for element in ResidueElement::ALL {
            let wire = serde_json::to_string(element).expect("a fieldless enum serializes");
            assert_eq!(wire, format!("\"{element}\""), "{element:?}");
        }
        let spellings: BTreeSet<String> = ResidueElement::ALL
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            spellings.len(),
            ResidueElement::ALL.len(),
            "one spelling for two elements is not a spelling: {spellings:?}"
        );
        // And it is not the derive's, which is what the operator messages in
        // `workspace_manager::worktree` and `topology::effects::bijection`
        // still carry (`SWEEP-WORKTREE-008`).
        assert_ne!(
            ResidueElement::IndexLock.to_string(),
            format!("{:?}", ResidueElement::IndexLock)
        );
    }

    #[test]
    fn the_ambient_join_answers_a_different_residue_and_recovery_in_each_mode() {
        let point = SubEffectPoint::AmbientJobJoined;
        // `join_ambient_job_with` applies the error-return hook before
        // `join()` and the kill hook after it, so the two modes are at
        // different states and cannot share one residue.
        assert_eq!(
            point.residue_artifact(InjectionMode::Kill),
            ResidueArtifact::NoHostProcess
        );
        assert_eq!(
            point.residue_artifact(InjectionMode::ErrorReturn),
            ResidueArtifact::NoProcessSpawned
        );
        // The kill's words name a mechanism the refusal does not run, which is
        // what made one artifact for both a false claim at the error-return
        // coordinate rather than a merely coarse one.
        assert!(
            ResidueArtifact::NoHostProcess
                .detail()
                .contains("the kernel terminates")
        );
        assert!(
            !ResidueArtifact::NoProcessSpawned
                .detail()
                .contains("the kernel terminates")
        );
        assert!(
            ResidueArtifact::NoProcessSpawned
                .detail()
                .contains("nothing was spawned")
        );

        assert_eq!(
            point.resume_action(InjectionMode::Kill),
            ResumeAction::AmbientHandleTerminates
        );
        assert_eq!(
            point.resume_action(InjectionMode::ErrorReturn),
            ResumeAction::RefuseUnspawned
        );
        // And the recovery is not the event log's refusal: that one ends at a
        // barrier an ambient join has not got.
        assert!(
            ResumeAction::RefuseResumably
                .text()
                .contains("the next open repeats the barrier")
        );
        assert!(!ResumeAction::RefuseUnspawned.text().contains("barrier"));

        // The split is this point's, not a blanket change: every other point
        // leaves the same durable shape whichever way the fault arrives.
        for other in SubEffectPoint::ALL
            .iter()
            .copied()
            .filter(|candidate| *candidate != point)
        {
            assert_eq!(
                other.residue_artifact(InjectionMode::Kill),
                other.residue_artifact(InjectionMode::ErrorReturn),
                "{other}"
            );
        }

        // Through the parent, which is where an entry meets the authority.
        let spawn = EffectSiteId::Process(crate::topology::effects::ProcessSite::Spawn);
        let refused = spawn.semantics(EntryPhase::Point {
            point,
            mode: InjectionMode::ErrorReturn,
        });
        assert!(refused.rows.is_empty());
        assert_eq!(refused.artifact, ResidueArtifact::NoProcessSpawned);
        assert_eq!(refused.action, ResumeAction::RefuseUnspawned);
    }

    #[test]
    fn the_cleanup_probe_gives_its_hold_back_and_leaves_no_row_holding_it() {
        // `rundir`'s `cleanup::take` takes `LOCK_EX | LOCK_NB` and then
        // `LOCK_UN` before it returns, deliberately; R17 is OS lock state
        // only, so nothing of what the probe took survives its own after hook
        // and no resume can adopt it.
        assert_eq!(
            LockSite::ProbeCleanupExclusive.after_effect(),
            AfterEffect::MomentaryHold
        );
        let after =
            EffectSiteId::Lock(LockSite::ProbeCleanupExclusive).semantics(EntryPhase::After);
        assert!(
            after.rows.is_empty(),
            "the released hold is held by no row: {:?}",
            after.rows
        );
        assert_eq!(after.artifact, ResidueArtifact::HoldReleased);
        assert_eq!(after.action, ResumeAction::RepeatProbe);

        // It is the only site of the group that gives its hold back. The two
        // acquisitions retain their `File` for the guard's lifetime, so R17
        // does hold theirs at their own after hook, and the lock-file creation
        // leaves a file that outlives every run.
        for retained in [
            LockSite::AcquireRun,
            LockSite::AcquireWorktree,
            LockSite::CreateWorktreeLockFile,
        ] {
            assert_eq!(
                retained.after_effect(),
                AfterEffect::Referenced,
                "{}",
                retained.name()
            );
            assert_eq!(
                EffectSiteId::Lock(retained)
                    .semantics(EntryPhase::After)
                    .rows,
                vec![retained.row()],
                "{}",
                retained.name()
            );
        }
        assert_eq!(LockSite::Release.after_effect(), AfterEffect::Removed);
        assert_eq!(
            LockSite::ObserveCleanupHold.after_effect(),
            AfterEffect::NoEffect
        );

        // Not the read-only claim: the probe creates the lock file it probes,
        // and the group's one read-only site is the hold observation.
        assert!(!LockSite::ProbeCleanupExclusive.is_read_only());
        assert!(LockSite::ObserveCleanupHold.is_read_only());
        // Its before phase is unchanged: the probe requires no earlier
        // artifact of its own.
        assert_eq!(
            LockSite::ProbeCleanupExclusive.before_state(),
            BeforeState::Absent
        );
        // The words are its own, and neither the publication's nor the
        // removal's.
        assert!(
            ResidueArtifact::HoldReleased
                .detail()
                .contains("given back")
        );
        assert!(!ResidueArtifact::Referenced.detail().contains("given back"));
        assert!(
            ResumeAction::RepeatProbe
                .text()
                .contains("the probe is repeated")
        );
    }
}
