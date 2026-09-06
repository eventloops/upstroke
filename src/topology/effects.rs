//! The fault-seam framework: typed effect sites, the hook harness, and the
//! fault-injection registry format.
//!
//! Nothing here performs an effect, and nothing here is wired into a funnel
//! yet. What is here is the *vocabulary* every later slice's proof is written
//! in: an inventory of the external-effect contexts a schema-4 run has, typed
//! so that "which resource row does this touch", "which durable append is it
//! ordered against", and "which fault-matrix row does a kill here land in" are
//! compile-time exhaustive functions rather than comments.
//!
//! # Why a type and not a string
//!
//! The claim ST-07 makes is a *bijection*: every effect site, in both hook
//! phases and at every parent-side sub-effect point, is observed executed at
//! least once, has a registry entry for every observable order, and every entry
//! has evidence. A bijection over a set of strings is a bijection over whatever
//! the strings happened to be that day. [`EffectSiteId`] is closed — a site
//! that is not a variant does not exist, and an entry naming one is refused —
//! so the left-hand side of the bijection is fixed by the compiler and the
//! right-hand side is what the suite must fill in.
//!
//! # The three kinds of thing a registry entry can be about
//!
//! They are different in kind and the framework keeps them apart by type,
//! because conflating them is how a coverage report claims coverage it does
//! not have:
//!
//! * A **hook phase** ([`HookPhase::Before`], [`HookPhase::After`]) — parent
//!   code that runs immediately either side of the primitive. Observed by
//!   execution.
//! * A **parent-side sub-effect point** ([`SubEffectPoint`]) — parent code
//!   inside a funnel, between two steps of one logical effect, in one
//!   [`InjectionMode`]. Also observed by execution.
//! * A **command-internal residue class** ([`ResidueClass`]) — a durable prefix
//!   *inside* an external command that the parent provably cannot hook. Its
//!   evidence is [`EvidenceLabel::RecoveryProven`]: synthetic construction of
//!   every residue element plus a kill-sampling record. It is **never** an
//!   executed hook, and [`FaultRegistry::insert`] refuses any entry that claims
//!   it is.
//!
//! That last refusal is the load-bearing one. A framework that accepted a
//! residue-class entry carrying an executed-hook claim would report that the
//! suite had observed something no portable mechanism can observe.
//!
//! # What is not here
//!
//! The funnels themselves, the clippy disallowed lists, the allow-placement
//! scan, and every real site's implementation. This slice builds the frame such
//! that those can be dropped in; it asserts nothing about code that does not
//! exist yet.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod bijection;
mod export;
mod harness;
mod registry;
mod residue_authority;
mod sites;
mod vocab;

pub use self::bijection::{BijectionFailure, check_bijection};
pub use self::export::{
    EffectSiteExport, ExportError, PointExport, ResidueClassExport, effect_sites, effect_sites_json,
};
pub use self::harness::{
    FastSequence, HarnessError, HookHarness, HookPhase, Injection, Observation,
};
pub use self::registry::{
    ClassHistogram, EntryPhase, Evidence, ExpectedResidue, FaultRegistry, RegistryEntry,
    RegistryError, SamplingRecord, SyntheticRecord, validate_entry,
};
pub use self::residue_authority::{
    AfterEffect, BeforeState, EvidenceLabel, ObjectResidue, ObservableOrder, PhaseSemantics,
    ResidueArtifact, ResidueClass, ResidueElement, ResumeAction,
};
pub use self::sites::{
    AnswerSite, ContainerSite, EventSite, LockSite, ObjectSite, ProcessSite, RefSite, ReportSite,
    RunDirSite, SnapshotSite, WorktreeSite,
};
pub use self::vocab::{
    Adjacent, DurableEvent, EnforcementDomain, FaultRow, FunnelGroup, Host, InjectionMode,
    Platform, ResourceRow, SiteScope, SubEffectPoint,
};

// ---------------------------------------------------------------------------
// EffectSiteId
// ---------------------------------------------------------------------------

/// `(funnel group, site variant)` — the inventory unit.
///
/// Serialized as the dotted name (`"RunDir.PublishCommitRecord"`), and
/// deserialized only into a name a group enum actually declares. That is what
/// makes `fault_injection_registry.completeness_rule`'s "entries for sites
/// absent from the enums are refused" true of the wire format and not only of
/// the Rust API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum EffectSiteId {
    Worktree(WorktreeSite),
    Snapshot(SnapshotSite),
    Ref(RefSite),
    Object(ObjectSite),
    RunDir(RunDirSite),
    Event(EventSite),
    Answer(AnswerSite),
    Lock(LockSite),
    Report(ReportSite),
    Process(ProcessSite),
    Container(ContainerSite),
}

/// A site name that no group enum declares.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "`{name}` is not a site any funnel group declares; a fault-injection entry names a site the \
     enums define, or it names nothing at all"
)]
pub struct UnknownSite {
    /// The name as it was written.
    pub name: String,
}

impl EffectSiteId {
    /// Every site of every group, group by group.
    ///
    /// Derived from the groups' `ALL` slices rather than written out again:
    /// two hand-maintained lists of seventy sites would disagree eventually,
    /// and the one that disagreed silently would be this one.
    pub fn all() -> Vec<Self> {
        let mut sites = Vec::new();
        sites.extend(WorktreeSite::ALL.iter().copied().map(Self::Worktree));
        sites.extend(SnapshotSite::ALL.iter().copied().map(Self::Snapshot));
        sites.extend(RefSite::ALL.iter().copied().map(Self::Ref));
        sites.extend(ObjectSite::ALL.iter().copied().map(Self::Object));
        sites.extend(RunDirSite::ALL.iter().copied().map(Self::RunDir));
        sites.extend(EventSite::ALL.iter().copied().map(Self::Event));
        sites.extend(AnswerSite::ALL.iter().copied().map(Self::Answer));
        sites.extend(LockSite::ALL.iter().copied().map(Self::Lock));
        sites.extend(ReportSite::ALL.iter().copied().map(Self::Report));
        sites.extend(ProcessSite::ALL.iter().copied().map(Self::Process));
        sites.extend(ContainerSite::ALL.iter().copied().map(Self::Container));
        sites
    }

    /// Every site whose scope carries the ST-07 requirement.
    pub fn claimed() -> Vec<Self> {
        Self::all()
            .into_iter()
            .filter(|site| site.scope().is_claimed())
            .collect()
    }

    /// Which funnel group declares this site.
    pub const fn group(self) -> FunnelGroup {
        match self {
            Self::Worktree(_) => FunnelGroup::Worktree,
            Self::Snapshot(_) => FunnelGroup::Snapshot,
            Self::Ref(_) => FunnelGroup::Ref,
            Self::Object(_) => FunnelGroup::Object,
            Self::RunDir(_) => FunnelGroup::RunDir,
            Self::Event(_) => FunnelGroup::Event,
            Self::Answer(_) => FunnelGroup::Answer,
            Self::Lock(_) => FunnelGroup::Lock,
            Self::Report(_) => FunnelGroup::Report,
            Self::Process(_) => FunnelGroup::Process,
            Self::Container(_) => FunnelGroup::Container,
        }
    }

    /// The variant's name inside its group.
    pub const fn variant(self) -> &'static str {
        match self {
            Self::Worktree(site) => site.name(),
            Self::Snapshot(site) => site.name(),
            Self::Ref(site) => site.name(),
            Self::Object(site) => site.name(),
            Self::RunDir(site) => site.name(),
            Self::Event(site) => site.name(),
            Self::Answer(site) => site.name(),
            Self::Lock(site) => site.name(),
            Self::Report(site) => site.name(),
            Self::Process(site) => site.name(),
            Self::Container(site) => site.name(),
        }
    }

    /// Exactly one resource row.
    pub const fn row(self) -> ResourceRow {
        match self {
            Self::Worktree(site) => site.row(),
            Self::Snapshot(site) => site.row(),
            Self::Ref(site) => site.row(),
            Self::Object(site) => site.row(),
            Self::RunDir(site) => site.row(),
            Self::Event(site) => site.row(),
            Self::Answer(site) => site.row(),
            Self::Lock(site) => site.row(),
            Self::Report(site) => site.row(),
            Self::Process(site) => site.row(),
            Self::Container(site) => site.row(),
        }
    }

    /// The adjacent durable append.
    pub const fn adjacent(self) -> Adjacent {
        match self {
            Self::Worktree(site) => site.adjacent(),
            Self::Snapshot(site) => site.adjacent(),
            Self::Ref(site) => site.adjacent(),
            Self::Object(site) => site.adjacent(),
            Self::RunDir(site) => site.adjacent(),
            Self::Event(site) => site.adjacent(),
            Self::Answer(site) => site.adjacent(),
            Self::Lock(site) => site.adjacent(),
            Self::Report(site) => site.adjacent(),
            Self::Process(site) => site.adjacent(),
            Self::Container(site) => site.adjacent(),
        }
    }

    /// The fault-matrix row.
    pub const fn fault_row(self) -> FaultRow {
        match self {
            Self::Worktree(site) => site.fault_row(),
            Self::Snapshot(site) => site.fault_row(),
            Self::Ref(site) => site.fault_row(),
            Self::Object(site) => site.fault_row(),
            Self::RunDir(site) => site.fault_row(),
            Self::Event(site) => site.fault_row(),
            Self::Answer(site) => site.fault_row(),
            Self::Lock(site) => site.fault_row(),
            Self::Report(site) => site.fault_row(),
            Self::Process(site) => site.fault_row(),
            Self::Container(site) => site.fault_row(),
        }
    }

    /// The claim this site is inside.
    pub const fn scope(self) -> SiteScope {
        match self {
            Self::Worktree(site) => site.scope(),
            Self::Snapshot(site) => site.scope(),
            Self::Ref(site) => site.scope(),
            Self::Object(site) => site.scope(),
            Self::RunDir(site) => site.scope(),
            Self::Event(site) => site.scope(),
            Self::Answer(site) => site.scope(),
            Self::Lock(site) => site.scope(),
            Self::Report(site) => site.scope(),
            Self::Process(site) => site.scope(),
            Self::Container(site) => site.scope(),
        }
    }

    /// Whether the site performs no effect at all.
    pub const fn is_read_only(self) -> bool {
        match self {
            Self::Worktree(site) => site.is_read_only(),
            Self::Snapshot(site) => site.is_read_only(),
            Self::Ref(site) => site.is_read_only(),
            Self::Object(site) => site.is_read_only(),
            Self::RunDir(site) => site.is_read_only(),
            Self::Event(site) => site.is_read_only(),
            Self::Answer(site) => site.is_read_only(),
            Self::Lock(site) => site.is_read_only(),
            Self::Report(site) => site.is_read_only(),
            Self::Process(site) => site.is_read_only(),
            Self::Container(site) => site.is_read_only(),
        }
    }

    /// The parent-side sub-effect points this site exposes.
    pub const fn sub_effects(self) -> &'static [SubEffectPoint] {
        match self {
            Self::Worktree(site) => site.sub_effects(),
            Self::Snapshot(site) => site.sub_effects(),
            Self::Ref(site) => site.sub_effects(),
            Self::Object(site) => site.sub_effects(),
            Self::RunDir(site) => site.sub_effects(),
            Self::Event(site) => site.sub_effects(),
            Self::Answer(site) => site.sub_effects(),
            Self::Lock(site) => site.sub_effects(),
            Self::Report(site) => site.sub_effects(),
            Self::Process(site) => site.sub_effects(),
            Self::Container(site) => site.sub_effects(),
        }
    }

    /// The command-internal residue classes registered for this site.
    pub const fn residue_classes(self) -> &'static [ResidueClass] {
        match self {
            Self::Worktree(site) => site.residue_classes(),
            Self::Snapshot(site) => site.residue_classes(),
            Self::Ref(site) => site.residue_classes(),
            Self::Object(site) => site.residue_classes(),
            Self::RunDir(site) => site.residue_classes(),
            Self::Event(site) => site.residue_classes(),
            Self::Answer(site) => site.residue_classes(),
            Self::Lock(site) => site.residue_classes(),
            Self::Report(site) => site.residue_classes(),
            Self::Process(site) => site.residue_classes(),
            Self::Container(site) => site.residue_classes(),
        }
    }

    /// The residue elements this site's class must construct synthetically.
    pub const fn residue_elements(self) -> &'static [ResidueElement] {
        match self {
            Self::Worktree(site) => site.residue_elements(),
            Self::Snapshot(site) => site.residue_elements(),
            Self::Ref(site) => site.residue_elements(),
            Self::Object(site) => site.residue_elements(),
            Self::RunDir(site) => site.residue_elements(),
            Self::Event(site) => site.residue_elements(),
            Self::Answer(site) => site.residue_elements(),
            Self::Lock(site) => site.residue_elements(),
            Self::Report(site) => site.residue_elements(),
            Self::Process(site) => site.residue_elements(),
            Self::Container(site) => site.residue_elements(),
        }
    }

    /// The module the site's funnel lives in.
    pub const fn module(self) -> &'static str {
        self.group().module()
    }

    /// The durable orders a fault here can leave observable.
    ///
    /// One order, not two, wherever the design fixes which of the effect and
    /// the append is durable first — which it does everywhere it names an
    /// adjacency. A site with no adjacency has no order at all, and its entry
    /// carries `None` rather than an arbitrary one.
    pub const fn observable_orders(self) -> &'static [ObservableOrder] {
        match self.adjacent() {
            Adjacent::Before(_) => &[ObservableOrder::EffectBeforeEvent],
            Adjacent::After(_) => &[ObservableOrder::EventBeforeEffect],
            Adjacent::None => &[],
        }
    }

    /// The dotted name.
    pub fn name(self) -> String {
        format!("{}.{}", self.group().name(), self.variant())
    }

    /// The site a dotted name refers to, or an error naming what was written.
    pub fn from_name(name: &str) -> Result<Self, UnknownSite> {
        Self::all()
            .into_iter()
            .find(|site| site.name() == name)
            .ok_or_else(|| UnknownSite {
                name: name.to_owned(),
            })
    }

    /// Whether this site exposes `point` in `mode`.
    pub fn exposes(self, point: SubEffectPoint, mode: InjectionMode) -> bool {
        self.sub_effects().contains(&point) && point.supports(mode)
    }

    /// What this site's before phase finds already durable.
    ///
    /// Delegated to the group enums for the same reason [`Self::after_effect`]
    /// is: their matches are exhaustive over their own variants and carry no
    /// wildcard, so a site added to a group has to be classified rather than
    /// inheriting whatever a default said.
    ///
    /// Declared per group rather than derived from [`Self::after_effect`], and
    /// the two are close enough that the temptation is real. A derivation would
    /// make one table the sole authority for both phases: a mutation to
    /// `after_effect` would move the before phase with it and stay invisible to
    /// every test that checks the two against each other. That is the shape of
    /// the defect this function exists to repair, one level up.
    pub const fn before_state(self) -> BeforeState {
        match self {
            Self::Worktree(site) => site.before_state(),
            Self::Snapshot(site) => site.before_state(),
            Self::Ref(site) => site.before_state(),
            Self::Object(site) => site.before_state(),
            Self::RunDir(site) => site.before_state(),
            Self::Event(site) => site.before_state(),
            Self::Answer(site) => site.before_state(),
            Self::Lock(site) => site.before_state(),
            Self::Report(site) => site.before_state(),
            Self::Process(site) => site.before_state(),
            Self::Container(site) => site.before_state(),
        }
    }

    /// What this site's after phase leaves durable.
    ///
    /// Delegated to the group enums, whose `after_effect` matches are
    /// exhaustive over their own variants and carry no wildcard, so the
    /// classification cannot silently acquire a default.
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::Worktree(site) => site.after_effect(),
            Self::Snapshot(site) => site.after_effect(),
            Self::Ref(site) => site.after_effect(),
            Self::Object(site) => site.after_effect(),
            Self::RunDir(site) => site.after_effect(),
            Self::Event(site) => site.after_effect(),
            Self::Answer(site) => site.after_effect(),
            Self::Lock(site) => site.after_effect(),
            Self::Report(site) => site.after_effect(),
            Self::Process(site) => site.after_effect(),
            Self::Container(site) => site.after_effect(),
        }
    }

    /// The whole of what a fault at `phase` of this site leaves durable and
    /// what a resume does about it — `fault_injection_registry.structure`'s
    /// "expected residue ... resume action", as values.
    ///
    /// The authority, not the entry: an entry that named its own rows, its own
    /// artifacts and its own recovery would be a second authority on three
    /// questions and could name anything for all three. The rows have been
    /// derived here since the format was written; the artifacts and the
    /// recovery were the two the format asked only to be non-empty, which is
    /// why an entry could carry a unique false claim in either and pass.
    ///
    /// Phase by phase:
    ///
    /// * *before* — [`Self::before_state`], per site. Nothing has been
    ///   performed, so the rows are whatever the site's primitive was about to
    ///   act on and has not yet, in three answers: nothing at all for a
    ///   one-step creation ("Object sites carry entries — before: no object");
    ///   the row `row()` names for a removal or an in-place replacement, whose
    ///   target has to be there for the primitive to be issued
    ///   (`transaction_fault_matrix[T-SCRUB]`: "worktree, its intent, or
    ///   snapshots **not yet removed**"); and *the same row with different
    ///   words* for the second half of a two-step protocol, where what the row
    ///   holds is the intent or the staged temporary rather than the target
    ///   (T-DISPATCH: "worktree **intent** or worktree not yet created"). Its
    ///   recovery is the before-phase action by definition — uniformly, for
    ///   all three classifications — and it is the action `structure` binds
    ///   `IdUnread` and `Internal` to.
    /// * *after* — [`Self::after_effect`], per site. A publication leaves its
    ///   artifact referenced by [`Self::row`]; a commit-tree leaves an
    ///   unreferenced object; "the pruning sites' after-phase entries record
    ///   the released objects as R27 residue"; a removal that releases nothing
    ///   leaves nothing; and a read-only observation performs no effect at all.
    /// * *a sub-effect point* — [`SubEffectPoint::residue_rows`],
    ///   [`SubEffectPoint::residue_artifact`] and
    ///   [`SubEffectPoint::resume_action`], the last of which reads the mode
    ///   because the mode is half the coordinate. The rows are the point's, not
    ///   the site's: a Windows containment kill leaves no host process and so
    ///   no row, and a Unix one leaves the reaper's R28 hold rather than the
    ///   R22 handle the coordinator no longer has.
    /// * *a residue class* — "objects present and unreferenced, R27, with
    ///   administrative residue in the owning worktree", so R27 and the row
    ///   that holds the administrative residue. The list never repeats a row,
    ///   so a site whose own row is R27 lists it once and has no separate
    ///   administrative row to name. "resume action equal to the before-phase
    ///   action".
    /// * *no-execution* — nothing ran.
    pub fn semantics(self, phase: EntryPhase) -> PhaseSemantics {
        match phase {
            // The recovery is `ResumeUnperformed` for all three
            // classifications and
            // deliberately so: `resumes_as_before` binds `IdUnread` and the
            // `Internal` residue class to *this* action, and a before phase
            // whose action varied by site would make that binding a different
            // claim at every site it holds at.
            EntryPhase::Before => match self.before_state() {
                BeforeState::Absent => PhaseSemantics {
                    rows: Vec::new(),
                    artifact: ResidueArtifact::Nothing,
                    action: ResumeAction::ResumeUnperformed,
                },
                BeforeState::PrecursorDurable => PhaseSemantics {
                    rows: vec![self.row()],
                    artifact: ResidueArtifact::PrecursorDurable,
                    action: ResumeAction::ResumeUnperformed,
                },
                BeforeState::Present => PhaseSemantics {
                    rows: vec![self.row()],
                    artifact: ResidueArtifact::TargetIntact,
                    action: ResumeAction::ResumeUnperformed,
                },
            },
            EntryPhase::NoExecution => PhaseSemantics {
                rows: Vec::new(),
                artifact: ResidueArtifact::NotReached,
                action: ResumeAction::NotExecuted,
            },
            EntryPhase::After => match self.after_effect() {
                AfterEffect::NoEffect => PhaseSemantics {
                    rows: Vec::new(),
                    artifact: ResidueArtifact::NoEffectPerformed,
                    action: ResumeAction::RepeatObservation,
                },
                AfterEffect::Referenced => PhaseSemantics {
                    rows: vec![self.row()],
                    artifact: ResidueArtifact::Referenced,
                    action: ResumeAction::AdoptPerformed,
                },
                AfterEffect::Unreferenced => PhaseSemantics {
                    rows: vec![ResourceRow::R27],
                    artifact: ResidueArtifact::Unreferenced,
                    action: ResumeAction::AdoptPerformed,
                },
                AfterEffect::Released => PhaseSemantics {
                    rows: vec![ResourceRow::R27],
                    artifact: ResidueArtifact::Released,
                    action: ResumeAction::ReclaimReleased,
                },
                AfterEffect::Removed => PhaseSemantics {
                    rows: Vec::new(),
                    artifact: ResidueArtifact::Removed,
                    action: ResumeAction::AdoptPerformed,
                },
            },
            EntryPhase::Point { point, mode } => PhaseSemantics {
                rows: point.residue_rows(self.row()),
                artifact: point.residue_artifact(),
                action: point.resume_action(mode),
            },
            EntryPhase::Residue { .. } => {
                let rows = if self.row() == ResourceRow::R27 {
                    vec![ResourceRow::R27]
                } else {
                    vec![ResourceRow::R27, self.row()]
                };
                PhaseSemantics {
                    artifact: if rows.len() == 1 {
                        ResidueArtifact::ObjectsUnreferenced
                    } else {
                        ResidueArtifact::ObjectsAndAdministrativeResidue
                    },
                    rows,
                    action: ResumeAction::ResumeUnperformed,
                }
            }
        }
    }

    /// The ledger rows a fault at `phase` of this site leaves holding
    /// something — [`Self::semantics`]'s rows.
    pub fn expected_rows(self, phase: EntryPhase) -> Vec<ResourceRow> {
        self.semantics(phase).rows
    }

    /// Whether this site registers `class`.
    pub fn registers(self, class: ResidueClass) -> bool {
        self.residue_classes().contains(&class)
    }

    /// Whether a fast integration sequence skips this site entirely.
    ///
    /// Exactly the three the design names: an exact-base fast publication
    /// creates no staging worktree, cherry-picks nothing, and takes no prepared
    /// pin. They are the only sites a `NoExecution` entry may be written for.
    ///
    /// Being one of them exempts a site from nothing. All three are
    /// Topology-scoped and all three execute on the stale-candidate path, so
    /// `completeness_rule` requires their hook phases and points observed like
    /// any other site's; the no-execution entry is a second, trace-scoped
    /// claim laid on top of that one. See [`check_bijection`].
    pub const fn skipped_on_fast_path(self) -> bool {
        matches!(
            self,
            Self::Worktree(WorktreeSite::AddStaging)
                | Self::Object(ObjectSite::ProposalCherryPick)
                | Self::Ref(RefSite::PinPrepared)
        )
    }
}

// ---------------------------------------------------------------------------
// The compile-time contract
// ---------------------------------------------------------------------------
//
// `effect_site_inventory.identity`: "every variant carries, through
// compile-time exhaustive const fns, its ResourceKind row (row(): ...), its
// adjacent durable event ..., its fault-matrix row id, and its scope ...; each
// enum exposes a const ALL slice".
//
// Every one of those functions was declared `const fn` and then called only
// from ordinary code — unit tests, `effect_sites()`, the registry — so the
// whole compile-time half of the contract was asserted in prose and checked at
// run time. Demote `pub const fn row` to `pub fn row` and the crate, its tests
// and its generated inventory all still build; the frozen API is broken and
// nothing says so. A compile-time contract is stated where the compiler
// enforces it, which is here: none of this module builds unless the four
// functions are callable in a const context over values taken from the groups'
// const `ALL` slices.

/// Walk every group's `ALL` slice at compile time and put every site of it
/// through the four `identity` functions and the residue authority.
///
/// A `while` over the slice rather than a list of variants, so the walk covers
/// whatever `ALL` holds and cannot fall behind a group that grows one.
macro_rules! const_identity_walk {
    ($($group:ident => $wrap:ident),+ $(,)?) => {
        const _: () = {
            $(
                let mut index = 0;
                while index < $group::ALL.len() {
                    let site = EffectSiteId::$wrap($group::ALL[index]);
                    let _ = site.row();
                    let _ = site.adjacent();
                    let _ = site.fault_row();
                    let _ = site.scope();
                    let _ = site.before_state();
                    let _ = site.after_effect();
                    index += 1;
                }
            )+
        };
    };
}

const_identity_walk! {
    WorktreeSite => Worktree,
    SnapshotSite => Snapshot,
    RefSite => Ref,
    ObjectSite => Object,
    RunDirSite => RunDir,
    EventSite => Event,
    AnswerSite => Answer,
    LockSite => Lock,
    ReportSite => Report,
    ProcessSite => Process,
    ContainerSite => Container,
}

/// How many sites the eleven groups declare, summed from their `ALL` slices at
/// compile time.
///
/// `EffectSiteId::all()` is a `Vec` and cannot be one; this is the const half
/// of the same count, and `the_generated_inventory_describes_every_site_and_invents_none`
/// asserts the two agree.
pub const INVENTORY_SIZE: usize = WorktreeSite::ALL.len()
    + SnapshotSite::ALL.len()
    + RefSite::ALL.len()
    + ObjectSite::ALL.len()
    + RunDirSite::ALL.len()
    + EventSite::ALL.len()
    + AnswerSite::ALL.len()
    + LockSite::ALL.len()
    + ReportSite::ALL.len()
    + ProcessSite::ALL.len()
    + ContainerSite::ALL.len();

const _: () = assert!(INVENTORY_SIZE == 70, "the inventory this slice ships");

/// One site's row, resolved at compile time — the downstream `const`
/// declaration `identity` promises a caller of [`EffectSiteId::row`] can write.
///
/// The commit-tree sites are the ones worth stating: `row()` names the row that
/// references the created object *immediately after* the effect, and nothing
/// references a commit-tree's object until a later site does.
pub const CANDIDATE_COMMIT_TREE_ROW: ResourceRow =
    EffectSiteId::Object(ObjectSite::CandidateCommitTree).row();

// And the values, in a const context, so the walk above is not a compile-time
// call over an answer nothing pins. The two commit-tree sites are R27 because
// nothing references what a commit-tree writes; a forced worktree removal is
// R9 because R9 is the row the worktree it removes occupies.
const _: () = {
    assert!(matches!(
        EffectSiteId::Object(ObjectSite::SnapshotCommitTree).row(),
        ResourceRow::R27
    ));
    assert!(matches!(CANDIDATE_COMMIT_TREE_ROW, ResourceRow::R27));
    assert!(matches!(
        EffectSiteId::Worktree(WorktreeSite::Remove).row(),
        ResourceRow::R9
    ));
    assert!(matches!(
        EffectSiteId::Event(EventSite::AppendFirst).row(),
        ResourceRow::R21
    ));
    assert!(matches!(
        EffectSiteId::Event(EventSite::AppendFirst).adjacent(),
        Adjacent::None
    ));
    assert!(matches!(
        EffectSiteId::Object(ObjectSite::CandidateCommitTree).fault_row(),
        FaultRow::TCandObj
    ));
    assert!(matches!(
        EffectSiteId::Event(EventSite::LegacyAppend).scope(),
        SiteScope::Legacy
    ));
    assert!(matches!(
        EffectSiteId::Worktree(WorktreeSite::Remove).after_effect(),
        AfterEffect::Released
    ));
    assert!(matches!(
        EffectSiteId::Worktree(WorktreeSite::Verify).after_effect(),
        AfterEffect::NoEffect
    ));
};

impl fmt::Display for EffectSiteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.group().name(), self.variant())
    }
}

impl From<EffectSiteId> for String {
    fn from(site: EffectSiteId) -> Self {
        site.name()
    }
}

impl TryFrom<String> for EffectSiteId {
    type Error = UnknownSite;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_name(&value)
    }
}

#[cfg(test)]
mod tests;
