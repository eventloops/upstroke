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

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// The eleven funnel API groups (`decisions.effect_site_inventory.identity`).
///
/// A group is an API surface, not a resource: one group can span several
/// [`ResourceRow`]s, and one row can be reached from several groups. The
/// grouping is what makes `hook(Before, site) -> primitive -> hook(After, site)`
/// implementable once per funnel rather than once per site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunnelGroup {
    Worktree,
    Snapshot,
    Ref,
    Object,
    RunDir,
    Event,
    Answer,
    Lock,
    Report,
    Process,
    Container,
}

impl FunnelGroup {
    /// Every group, in the order `identity` names them.
    pub const ALL: &'static [Self] = &[
        Self::Worktree,
        Self::Snapshot,
        Self::Ref,
        Self::Object,
        Self::RunDir,
        Self::Event,
        Self::Answer,
        Self::Lock,
        Self::Report,
        Self::Process,
        Self::Container,
    ];

    /// The group's name as it appears in a site's dotted name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Worktree => "Worktree",
            Self::Snapshot => "Snapshot",
            Self::Ref => "Ref",
            Self::Object => "Object",
            Self::RunDir => "RunDir",
            Self::Event => "Event",
            Self::Answer => "Answer",
            Self::Lock => "Lock",
            Self::Report => "Report",
            Self::Process => "Process",
            Self::Container => "Container",
        }
    }

    /// The funnel module this group's effects are confined to
    /// (`decisions.effect_site_inventory.mechanism`, the funnel-module list).
    ///
    /// Recorded per group rather than per site because the allow-placement scan
    /// PR6 builds works on modules: a module either performs effects only
    /// inside site-taking APIs, or it does not.
    pub const fn module(self) -> &'static str {
        match self {
            Self::Worktree | Self::Snapshot | Self::Ref | Self::Object => {
                "src/workspace_manager.rs"
            }
            Self::RunDir | Self::Lock => "src/rundir.rs",
            Self::Event => "src/events/log.rs",
            Self::Answer => "src/interaction.rs",
            Self::Report => "src/util.rs",
            Self::Process => "src/runner/host.rs",
            Self::Container => "src/runner/container.rs",
        }
    }
}

impl fmt::Display for FunnelGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The resource-ledger rows an effect site can touch
/// (`decisions.resource_accounting.rows`).
///
/// Only the external-physical and process-local-OS rows appear: R1–R8 and
/// R13–R16 are the logical fold/broker domain, which
/// `resource_accounting.enforcement_domains` says takes no effect-site mapping
/// at all — "no effect-site mapping required or allowed". Their absence from
/// this enum is that rule expressed as a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRow {
    /// Task worktree + its durable synced intent, and the objects its index or
    /// HEAD references.
    R9,
    /// Staging worktree `merge/<seq>` + its intent (stale candidates only).
    R10,
    /// Candidates ref: authoritative candidate identity.
    R11,
    /// `prepared/<seq>` proposal pin (stale candidates only).
    R12,
    /// The coordinator's own lock holds (OS lock state only).
    R17,
    /// Execution root directory.
    R18,
    /// Disposable Git view directory (container runner).
    R19,
    /// Integration ref and run-scoped run-directory contents, public and
    /// private.
    R21,
    /// Host process handle / private job object / ambient job membership.
    R22,
    /// Candidate-prepared pin (non-authoritative).
    R23,
    /// Exact gate/review snapshot worktree + its intent.
    R24,
    /// The repository-scoped `upstroke-worktree.lock` file itself.
    R25,
    /// Container invocation: the container, its labels, and its global intent.
    R26,
    /// Engine-created Git objects no engine ref, pin, or worktree references.
    R27,
    /// A surviving Unix reaper's shared `cleanup.lock` hold.
    R28,
}

impl ResourceRow {
    /// Every row an effect site may name, in ledger order.
    pub const ALL: &'static [Self] = &[
        Self::R9,
        Self::R10,
        Self::R11,
        Self::R12,
        Self::R17,
        Self::R18,
        Self::R19,
        Self::R21,
        Self::R22,
        Self::R23,
        Self::R24,
        Self::R25,
        Self::R26,
        Self::R27,
        Self::R28,
    ];

    /// The row's ledger id.
    pub const fn name(self) -> &'static str {
        match self {
            Self::R9 => "R9",
            Self::R10 => "R10",
            Self::R11 => "R11",
            Self::R12 => "R12",
            Self::R17 => "R17",
            Self::R18 => "R18",
            Self::R19 => "R19",
            Self::R21 => "R21",
            Self::R22 => "R22",
            Self::R23 => "R23",
            Self::R24 => "R24",
            Self::R25 => "R25",
            Self::R26 => "R26",
            Self::R27 => "R27",
            Self::R28 => "R28",
        }
    }

    /// Which enforcement domain the row belongs to.
    ///
    /// The distinction matters to ST-09 rather than to ST-07, but it is a
    /// property of the row and belongs beside it: a process-local row is
    /// released by the OS at process death and is never released by cleanup,
    /// so an entry that tables a cleanup step for one is wrong on its face.
    pub const fn domain(self) -> EnforcementDomain {
        match self {
            Self::R17 | Self::R22 | Self::R28 => EnforcementDomain::ProcessLocalOs,
            Self::R9
            | Self::R10
            | Self::R11
            | Self::R12
            | Self::R18
            | Self::R19
            | Self::R21
            | Self::R23
            | Self::R24
            | Self::R25
            | Self::R26
            | Self::R27 => EnforcementDomain::ExternalPhysical,
        }
    }
}

impl fmt::Display for ResourceRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The two enforcement domains an effect site's row can belong to.
///
/// The logical fold/broker domain is deliberately absent: it has no sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementDomain {
    /// State on a real filesystem, ref store, or container runtime.
    ExternalPhysical,
    /// OS state bound to a process lifetime, released by the OS at its death.
    ProcessLocalOs,
}

/// Every tag the schema-4 vocabulary can write.
///
/// A mirror of [`crate::topology::events::TOPOLOGY_EVENT_KINDS`], typed so that
/// a site's adjacency is a value the compiler checks rather than a string a
/// typo can invent. The two lists are asserted equal element-for-element by a
/// unit test, so a change to the vocabulary breaks this module rather than
/// silently leaving a site pointing at an append that no longer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableEvent {
    RunStarted,
    RunResumed,
    TaskSpawned,
    TaskDispatched,
    AttemptStarted,
    AttemptFinished,
    AttemptInterrupted,
    GenerationClosed,
    DeferWaitElapsed,
    CandidatePrepared,
    TaskCandidateCreated,
    MergeVerificationStarted,
    MergeVerificationUnavailable,
    MergeVerificationInterrupted,
    MergePrepared,
    MergeRejected,
    TaskMerged,
    QuestionRaised,
    QuestionAnswered,
    BudgetExceeded,
    RunFinished,
    CapacitySnapshot,
    PoolExhausted,
    DesignDefect,
}

impl DurableEvent {
    /// Every kind, in the vocabulary's declaration order.
    pub const ALL: &'static [Self] = &[
        Self::RunStarted,
        Self::RunResumed,
        Self::TaskSpawned,
        Self::TaskDispatched,
        Self::AttemptStarted,
        Self::AttemptFinished,
        Self::AttemptInterrupted,
        Self::GenerationClosed,
        Self::DeferWaitElapsed,
        Self::CandidatePrepared,
        Self::TaskCandidateCreated,
        Self::MergeVerificationStarted,
        Self::MergeVerificationUnavailable,
        Self::MergeVerificationInterrupted,
        Self::MergePrepared,
        Self::MergeRejected,
        Self::TaskMerged,
        Self::QuestionRaised,
        Self::QuestionAnswered,
        Self::BudgetExceeded,
        Self::RunFinished,
        Self::CapacitySnapshot,
        Self::PoolExhausted,
        Self::DesignDefect,
    ];

    /// This kind's tag, as the log writes it.
    pub const fn kind(self) -> &'static str {
        match self {
            Self::RunStarted => "run_started",
            Self::RunResumed => "run_resumed",
            Self::TaskSpawned => "task_spawned",
            Self::TaskDispatched => "task_dispatched",
            Self::AttemptStarted => "attempt_started",
            Self::AttemptFinished => "attempt_finished",
            Self::AttemptInterrupted => "attempt_interrupted",
            Self::GenerationClosed => "generation_closed",
            Self::DeferWaitElapsed => "defer_wait_elapsed",
            Self::CandidatePrepared => "candidate_prepared",
            Self::TaskCandidateCreated => "task_candidate_created",
            Self::MergeVerificationStarted => "merge_verification_started",
            Self::MergeVerificationUnavailable => "merge_verification_unavailable",
            Self::MergeVerificationInterrupted => "merge_verification_interrupted",
            Self::MergePrepared => "merge_prepared",
            Self::MergeRejected => "merge_rejected",
            Self::TaskMerged => "task_merged",
            Self::QuestionRaised => "question_raised",
            Self::QuestionAnswered => "question_answered",
            Self::BudgetExceeded => "budget_exceeded",
            Self::RunFinished => "run_finished",
            Self::CapacitySnapshot => "capacity_snapshot",
            Self::PoolExhausted => "pool_exhausted",
            Self::DesignDefect => "design_defect",
        }
    }
}

impl fmt::Display for DurableEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind())
    }
}

/// The durable append a site's effect is ordered against.
///
/// `Before` means the effect is designed to be durable before the append is;
/// `After` means the append is durable first. `None` is not "unknown": it is
/// the answer for a site that *is* the append (the whole [`EventSite`] append
/// group), and for a site that runs outside any run's log at all — the husk
/// census removes a run directory belonging to a run whose log it has refused
/// to fold.
///
/// The value decides [`EffectSiteId::observable_orders`], which is what the
/// registry's order axis ranges over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Adjacent {
    /// The effect precedes this append.
    Before(DurableEvent),
    /// This append precedes the effect.
    After(DurableEvent),
    /// No append is adjacent.
    None,
}

impl Adjacent {
    /// The append this site is ordered against, where there is one.
    pub const fn event(self) -> Option<DurableEvent> {
        match self {
            Self::Before(kind) | Self::After(kind) => Some(kind),
            Self::None => None,
        }
    }
}

/// The transaction fault-matrix row a fault at a site lands in.
///
/// One variant per row of `transaction_fault_matrix`, in its order. The row is
/// what says which durable prefix the fault leaves and what a resume does about
/// it; the site says where the fault can happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultRow {
    TRunstart,
    TDispatch,
    TAttempt,
    TRetry,
    TCandObj,
    TCandRef,
    TScrub,
    TFailed,
    TRetained,
    TFast,
    TProposal,
    TVerify,
    TPrepared,
    TReject,
    TRepairDispatch,
    TContainer,
    TAppend,
    TAnswer,
    TFinish,
    TFinalize,
    TResume,
}

impl FaultRow {
    /// Every row, in matrix order.
    pub const ALL: &'static [Self] = &[
        Self::TRunstart,
        Self::TDispatch,
        Self::TAttempt,
        Self::TRetry,
        Self::TCandObj,
        Self::TCandRef,
        Self::TScrub,
        Self::TFailed,
        Self::TRetained,
        Self::TFast,
        Self::TProposal,
        Self::TVerify,
        Self::TPrepared,
        Self::TReject,
        Self::TRepairDispatch,
        Self::TContainer,
        Self::TAppend,
        Self::TAnswer,
        Self::TFinish,
        Self::TFinalize,
        Self::TResume,
    ];

    /// The row's id, exactly as the matrix writes it.
    pub const fn id(self) -> &'static str {
        match self {
            Self::TRunstart => "T-RUNSTART",
            Self::TDispatch => "T-DISPATCH",
            Self::TAttempt => "T-ATTEMPT",
            Self::TRetry => "T-RETRY",
            Self::TCandObj => "T-CAND-OBJ",
            Self::TCandRef => "T-CAND-REF",
            Self::TScrub => "T-SCRUB",
            Self::TFailed => "T-FAILED",
            Self::TRetained => "T-RETAINED",
            Self::TFast => "T-FAST",
            Self::TProposal => "T-PROPOSAL",
            Self::TVerify => "T-VERIFY",
            Self::TPrepared => "T-PREPARED",
            Self::TReject => "T-REJECT",
            Self::TRepairDispatch => "T-REPAIR-DISPATCH",
            Self::TContainer => "T-CONTAINER",
            Self::TAppend => "T-APPEND",
            Self::TAnswer => "T-ANSWER",
            Self::TFinish => "T-FINISH",
            Self::TFinalize => "T-FINALIZE",
            Self::TResume => "T-RESUME",
        }
    }
}

impl fmt::Display for FaultRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Which claim a site is inside (`decisions.effect_site_inventory.scope`).
///
/// `Topology` and `Shared` carry the full ST-07 requirement. `Legacy` sites are
/// inventoried and row-mapped and carry no fault-registry requirement beyond
/// today's legacy tests — they exist because the Event funnel is shared and its
/// legacy callers have to pass *something*, and a scope is a safer thing for
/// them to pass than a site that also claims topology coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteScope {
    /// Reached only by schema-4 paths.
    Topology,
    /// Reached by both schema-4 and legacy paths through one funnel.
    Shared,
    /// Reached only by schema-1..3 paths.
    Legacy,
}

impl SiteScope {
    /// Whether this scope carries the ST-07 bijection requirement.
    pub const fn is_claimed(self) -> bool {
        match self {
            Self::Topology | Self::Shared => true,
            Self::Legacy => false,
        }
    }
}

/// How a fault is introduced at a parent-side sub-effect point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionMode {
    /// The process dies at the point.
    Kill,
    /// The funnel returns `Err` from the point, after performing or partially
    /// performing the primitive.
    ErrorReturn,
}

impl InjectionMode {
    /// Both modes.
    pub const ALL: &'static [Self] = &[Self::Kill, Self::ErrorReturn];
}

/// Which host a sub-effect point exists on.
///
/// The containment steps are the only points that differ: a Windows ambient
/// job has no Unix counterpart and a Unix reaper has no Windows one. ST-07's
/// evidence "executes each point on its platform", so the bijection check is
/// told which platform it is running on and does not require a point that
/// cannot exist there.
///
/// This is a property of a *point*, not a machine: [`Self::Any`] means "exists
/// wherever the parent runs". A host is [`Host`], which has no such value —
/// see the type for why the two are not one enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// Present on every host.
    Any,
    /// Present only on Windows.
    Windows,
    /// Present only on Unix.
    Unix,
}

/// The host a bijection check is running on.
///
/// Two values, and deliberately not [`Platform`]'s three. `required_on` used to
/// take a `Platform` as its host and answer the `(Windows, Any)` and
/// `(Unix, Any)` pairs through a `(Self::Windows, _) | (Self::Unix, _) => false`
/// wildcard — so `Platform::Any` named a host on which *neither* platform's
/// containment points were required, and `check_bijection` returned success for
/// `Process.Spawn` with all eight containment points unobserved and unentered.
/// A checker that can be handed a host meaning "no platform" is a checker whose
/// strongest claim is optional.
///
/// The fix is the type rather than a guard: a machine is Windows or it is Unix,
/// and there is no third value to reject at a boundary, forget to reject at the
/// next one, or serialize into a registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    /// A Windows host: the ambient- and private-job containment steps exist.
    Windows,
    /// A Unix host: the reaper containment steps exist.
    Unix,
}

impl Host {
    /// Both hosts. Every self-test that asserts a platform-dependent shape runs
    /// over this slice rather than over [`Self::current`], because a build that
    /// only ever checks its own host cannot fail on the other one until the
    /// other one's CI cell does.
    pub const ALL: &'static [Self] = &[Self::Windows, Self::Unix];

    /// The host's name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Unix => "unix",
        }
    }

    /// The host this build is running on.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }

    /// The other host.
    pub const fn other(self) -> Self {
        match self {
            Self::Windows => Self::Unix,
            Self::Unix => Self::Windows,
        }
    }

    /// This host as a point platform: the platform whose points it requires.
    pub const fn platform(self) -> Platform {
        match self {
            Self::Windows => Platform::Windows,
            Self::Unix => Platform::Unix,
        }
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Platform {
    /// Whether a point declared for `self` has to be observed on `host`.
    ///
    /// Every pair is written out. No wildcard stands in for the two negative
    /// answers, so a host value added later fails to compile here instead of
    /// quietly joining the `false` arm and excusing a platform's points from
    /// the bijection.
    pub const fn required_on(self, host: Host) -> bool {
        match (self, host) {
            (Self::Any, Host::Windows) | (Self::Any, Host::Unix) => true,
            (Self::Windows, Host::Windows) | (Self::Unix, Host::Unix) => true,
            (Self::Windows, Host::Unix) | (Self::Unix, Host::Windows) => false,
        }
    }
}

/// A parent-side point inside a funnel, between two steps of one logical
/// effect.
///
/// A hook is parent-executed code and can be executed only where the parent
/// runs. These are the places inside a funnel where that is still true: after
/// a child has exited but before the parent recorded what it printed
/// ([`Self::IdUnread`]), between a write and its sync ([`Self::Written`],
/// [`Self::Synced`]), inside the log-open sequence, and at the containment
/// steps of a spawn — which are parent-side or, for
/// [`Self::PreExecPgidAndRegister`], run in the forked child before `exec`,
/// which is still this crate's code and still under the harness's control.
///
/// Everything that is *not* on this list and not a hook phase is a command-
/// internal residue class instead. The distinction is the whole point:
/// `claim_scope` claims parent-observed execution only for parent-executed
/// code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubEffectPoint {
    /// A commit-tree child has exited with its object written; the coordinator
    /// has not read or recorded the printed id. R27 residue.
    IdUnread,
    /// Bytes written, possibly partially, not yet synced.
    ///
    /// In kill mode this is the whole of what the packet tables for a written
    /// append — "torn: truncated on the next open, previous prefix;
    /// complete-unsynced: either prefix". In error-return mode it is the
    /// *first* of three separately required cases: the partial write that
    /// returned `Err` before a newline was committed.
    Written,
    /// The complete newline-terminated line is written and the flush has not
    /// run.
    ///
    /// The second required error-return case
    /// (`fault_injection_registry.structure`: "error-return entries for
    /// Written-partial-then-Err, **Written-full-then-flush-Err**, and
    /// Synced-Err"). A separate point rather than a second entry at
    /// `Written/ErrorReturn`, because that key is one key: the registry is
    /// keyed by site x phase x order x mode, so two cases at one coordinate
    /// are a duplicate the format refuses, and one of the two would go
    /// unexecuted while the coordinate read as complete. The durable shapes
    /// differ too — a partial line is a torn tail the next open truncates, a
    /// complete unsynced line is a prefix the barrier makes durable — so they
    /// are two rows, not one row observed twice.
    ///
    /// Kill mode is deliberately absent: `structure` tables kill entries for
    /// `Written` and `Synced` only, and a kill here leaves the
    /// complete-unsynced prefix `Written`'s kill entry already covers.
    /// Declaring one would manufacture a coverage obligation the design does
    /// not make.
    WrittenFull,
    /// The append is synced.
    Synced,
    /// The log file was created (and its directory fsynced) because it was
    /// absent.
    Create,
    /// An unterminated final line was truncated before the append handle was
    /// taken.
    TruncateTornTail,
    /// The complete surviving prefix was synced — the durable half of the
    /// stable-prefix barrier.
    SyncPrefix,
    /// Windows: the coordinator process joined the ambient job at startup.
    AmbientJobJoined,
    /// Windows: the child was created suspended, already an ambient-job member.
    CreatedSuspended,
    /// Windows: the child was assigned to its private job object.
    PrivateJobAssigned,
    /// Windows: the suspended child was resumed.
    Resumed,
    /// Unix: the per-invocation reaper was forked and took its cleanup hold.
    ReaperStarted,
    /// Unix: in the forked child, before `exec`, the pgid was set and the group
    /// registered.
    PreExecPgidAndRegister,
    /// Unix: the child `exec`ed.
    Exec,
    /// Unix: the parent registered the running group.
    Registered,
}

impl SubEffectPoint {
    /// Every point.
    pub const ALL: &'static [Self] = &[
        Self::IdUnread,
        Self::Written,
        Self::WrittenFull,
        Self::Synced,
        Self::Create,
        Self::TruncateTornTail,
        Self::SyncPrefix,
        Self::AmbientJobJoined,
        Self::CreatedSuspended,
        Self::PrivateJobAssigned,
        Self::Resumed,
        Self::ReaperStarted,
        Self::PreExecPgidAndRegister,
        Self::Exec,
        Self::Registered,
    ];

    /// The point's name inside its site.
    pub const fn name(self) -> &'static str {
        match self {
            Self::IdUnread => "IdUnread",
            Self::Written => "Written",
            Self::WrittenFull => "WrittenFull",
            Self::Synced => "Synced",
            Self::Create => "Create",
            Self::TruncateTornTail => "TruncateTornTail",
            Self::SyncPrefix => "SyncPrefix",
            Self::AmbientJobJoined => "AmbientJobJoined",
            Self::CreatedSuspended => "CreatedSuspended",
            Self::PrivateJobAssigned => "PrivateJobAssigned",
            Self::Resumed => "Resumed",
            Self::ReaperStarted => "ReaperStarted",
            Self::PreExecPgidAndRegister => "PreExecPgidAndRegister",
            Self::Exec => "Exec",
            Self::Registered => "Registered",
        }
    }

    /// The injection modes this point supports.
    ///
    /// Kill is universal: a coordinator can die anywhere. Error-return is
    /// narrower — it exists where the design gives the funnel an error contract
    /// to return *through*. The Event points all have one (the append-error
    /// protocol, and `SyncPrefix`'s resumable refusal), and Windows'
    /// `AmbientJobJoined` has one ("failure refuses the write command").
    /// [`Self::IdUnread`] has none: the packet describes it only as a durable
    /// prefix a kill leaves, and inventing an error contract for it would be
    /// inventing a resume action nothing tables.
    pub const fn modes(self) -> &'static [InjectionMode] {
        match self {
            Self::Written
            | Self::Synced
            | Self::Create
            | Self::TruncateTornTail
            | Self::SyncPrefix
            | Self::AmbientJobJoined => InjectionMode::ALL,
            Self::WrittenFull => &[InjectionMode::ErrorReturn],
            Self::IdUnread
            | Self::CreatedSuspended
            | Self::PrivateJobAssigned
            | Self::Resumed
            | Self::ReaperStarted
            | Self::PreExecPgidAndRegister
            | Self::Exec
            | Self::Registered => &[InjectionMode::Kill],
        }
    }

    /// The host this point exists on.
    pub const fn platform(self) -> Platform {
        match self {
            Self::AmbientJobJoined
            | Self::CreatedSuspended
            | Self::PrivateJobAssigned
            | Self::Resumed => Platform::Windows,
            Self::ReaperStarted | Self::PreExecPgidAndRegister | Self::Exec | Self::Registered => {
                Platform::Unix
            }
            Self::IdUnread
            | Self::Written
            | Self::WrittenFull
            | Self::Synced
            | Self::Create
            | Self::TruncateTornTail
            | Self::SyncPrefix => Platform::Any,
        }
    }

    /// Whether this point supports `mode`.
    pub fn supports(self, mode: InjectionMode) -> bool {
        self.modes().contains(&mode)
    }
}

impl fmt::Display for SubEffectPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

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
    /// [`EffectSiteId::row`] names accounts for: the intent behind an add, the
    /// staged temporary behind an atomic publication.
    ///
    /// The rows are [`Self::Present`]'s and the words are not, deliberately.
    /// The row holds something, so the entry must say so; the thing it holds
    /// is not the target intact, so the entry must not say that either.
    PrecursorDurable,
    /// The artifact this site's primitive acts on is already durable and the
    /// row [`EffectSiteId::row`] names holds it: every removal, every release,
    /// and every in-place replacement of an artifact that has to exist for the
    /// primitive to be issued at all.
    Present,
}

/// What a site's *after* phase leaves durable.
///
/// The per-site half of the residue authority, and the reason
/// [`EffectSiteId::semantics`] has no generic arm. `structure` does not give
/// every site the same after-phase: an effect that publishes something leaves
/// it referenced by the site's own row, a commit-tree leaves an object nothing
/// references, "the pruning sites' after-phase entries record the released
/// objects as R27 residue", and a removal that releases nothing leaves the row
/// that accounted for what it removed holding nothing. One `vec![self.row()]`
/// answers all five the same way and is wrong for four of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AfterEffect {
    /// The site performs no effect at all, so its after phase leaves nothing.
    NoEffect,
    /// The artifact is durable and the row [`EffectSiteId::row`] names
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
}

/// The concrete artifacts a fault at one `(site, phase)` leaves, in the fault
/// matrix's own words.
///
/// `structure` requires each entry to record "the expected residue
/// (refs/worktrees/pins/intents/containers/marker, owner-record, and
/// commit-record files/objects and the row referencing them/administrative
/// residue)". An entry free to write that prose itself is a second authority on
/// the same question — the argument [`EffectSiteId::expected_rows`] already
/// makes about the rows, and the rows were only half the claim. So the artifact
/// is a value, [`Self::detail`] is its words, and [`ExpectedResidue`]'s own
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
    /// A Windows containment point.
    NoHostProcess,
    /// A Unix containment point.
    ReaperHeldGroup,
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
        Self::ReaperHeldGroup,
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
            Self::ReaperHeldGroup => {
                "a process group the reaper settles while holding its shared cleanup hold, R28"
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
    pub const fn after_effect(self) -> AfterEffect {
        match self {
            Self::AcquireRun
            | Self::AcquireWorktree
            | Self::ProbeCleanupExclusive
            | Self::CreateWorktreeLockFile => AfterEffect::Referenced,
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

    /// The artifacts a fault at this point leaves.
    pub const fn residue_artifact(self) -> ResidueArtifact {
        match self {
            Self::IdUnread => ResidueArtifact::IdNotRecorded,
            Self::Written => ResidueArtifact::UnsyncedBytes,
            Self::WrittenFull => ResidueArtifact::UnsyncedLine,
            Self::Synced => ResidueArtifact::SyncedLine,
            Self::Create => ResidueArtifact::LogCreated,
            Self::TruncateTornTail => ResidueArtifact::TornTailTruncated,
            Self::SyncPrefix => ResidueArtifact::PrefixPossiblyNonDurable,
            Self::AmbientJobJoined
            | Self::CreatedSuspended
            | Self::PrivateJobAssigned
            | Self::Resumed => ResidueArtifact::NoHostProcess,
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
            // refuses the write command)".
            Self::AmbientJobJoined => match mode {
                InjectionMode::Kill => ResumeAction::AmbientHandleTerminates,
                InjectionMode::ErrorReturn => ResumeAction::RefuseResumably,
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

// ---------------------------------------------------------------------------
// The hook harness
// ---------------------------------------------------------------------------

/// A phase at which the parent executes a hook.
///
/// There is deliberately no residue-class variant. A residue class is not an
/// executed hook, and the type is the first of the two places this framework
/// says so — the second is [`FaultRegistry::insert`], which refuses an entry
/// that claims otherwise even though this type made the claim unsayable to the
/// harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HookPhase {
    /// Before the primitive.
    Before,
    /// After the primitive.
    After,
    /// At a parent-side sub-effect point, in one injection mode.
    Point {
        /// Which point.
        point: SubEffectPoint,
        /// Which mode the injection is armed in.
        mode: InjectionMode,
    },
}

impl HookPhase {
    /// The two hook phases every site has.
    pub const PHASES: &'static [Self] = &[Self::Before, Self::After];
}

impl fmt::Display for HookPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Before => f.write_str("before"),
            Self::After => f.write_str("after"),
            Self::Point { point, mode } => write!(
                f,
                "{point}/{}",
                match mode {
                    InjectionMode::Kill => "kill",
                    InjectionMode::ErrorReturn => "error-return",
                }
            ),
        }
    }
}

/// What a funnel must do when it returns from a hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Injection {
    /// Nothing is armed here: carry on.
    Proceed,
    /// Die at this point.
    Kill,
    /// Return `Err` from this point.
    Error,
}

/// One `(site, phase)` the harness saw executed, and how often.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// The site whose funnel called the hook.
    pub site: EffectSiteId,
    /// The phase it called it at.
    pub phase: HookPhase,
    /// How many times.
    pub count: u32,
}

/// Why the harness refused to arm an injection.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HarnessError {
    #[error(
        "`{site}` exposes no parent-side sub-effect point `{point}`; arming one would record an \
         execution of a point that does not exist"
    )]
    NoSuchPoint {
        /// The site.
        site: String,
        /// The point that was asked for.
        point: SubEffectPoint,
    },

    #[error("`{site}`'s `{point}` point does not support {mode:?} injection")]
    UnsupportedMode {
        /// The site.
        site: String,
        /// The point.
        point: SubEffectPoint,
        /// The mode that was asked for.
        mode: InjectionMode,
    },
}

/// Records what the funnels actually executed.
///
/// The whole value of this type is negative: it can only report an execution
/// that a funnel told it about by calling [`Self::hook`]. Arming an injection
/// records nothing, because an armed injection that never fired is exactly the
/// case a coverage report must not count. A harness that recorded at arming
/// time would report full coverage for a suite that never reached a single
/// site.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookHarness {
    armed: Vec<(EffectSiteId, SubEffectPoint, InjectionMode)>,
    /// What executed: both hook phases, and the injected modes that fired.
    observed: Vec<Observation>,
    /// What a funnel walked past at a point, whether or not anything fired.
    reached: Vec<Observation>,
    /// The fast integration sequences the suite exercised, in order.
    fast: Vec<FastSequence>,
    /// The one being recorded, if a sequence is open.
    open_fast: Option<usize>,
}

/// One exercised fast integration sequence, and every site its funnels ran.
///
/// ST-07's no-execution claim is "no staging, cherry-pick, or prepared-pin
/// site executed **for any fast sequence**" — a statement about traces, not a
/// statement about a process. A harness that had run nothing satisfies "the
/// site was never touched" trivially, so the absence has to be proved *inside*
/// a sequence that demonstrably happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastSequence {
    name: String,
    touched: Vec<EffectSiteId>,
}

impl FastSequence {
    /// What the suite called this sequence.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every site whose funnel ran during it, in first-execution order.
    pub fn touched(&self) -> &[EffectSiteId] {
        &self.touched
    }

    /// Whether `site` ran during this sequence.
    pub fn ran(&self, site: EffectSiteId) -> bool {
        self.touched.contains(&site)
    }
}

impl HookHarness {
    /// A harness that has armed nothing and seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Arm an injection at one point of one site.
    ///
    /// Refuses a point the site does not expose and a mode the point does not
    /// support, so a suite cannot quietly arm a fault that no funnel will ever
    /// consult.
    pub fn arm(
        &mut self,
        site: EffectSiteId,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) -> Result<(), HarnessError> {
        if !site.sub_effects().contains(&point) {
            return Err(HarnessError::NoSuchPoint {
                site: site.name(),
                point,
            });
        }
        if !point.supports(mode) {
            return Err(HarnessError::UnsupportedMode {
                site: site.name(),
                point,
                mode,
            });
        }
        if !self.armed.contains(&(site, point, mode)) {
            self.armed.push((site, point, mode));
        }
        Ok(())
    }

    /// Disarm every injection, keeping everything already observed.
    pub fn disarm(&mut self) {
        self.armed.clear();
    }

    /// The call a funnel makes. Answers what to do, and records an execution
    /// only of what actually happened.
    ///
    /// The two are not the same claim, and the difference is the whole reason
    /// this type exists.
    /// `fault_injection_registry.completeness_rule` requires every point to be
    /// "observed executed at least once by the suite **in every injection mode
    /// it supports**", and a mode is executed when its fault fired — not when
    /// a funnel walked past the place it would have fired. A harness that
    /// counted the walk-past would report both modes of every point covered
    /// for a suite that armed nothing, which is the same false report as
    /// counting at arming time, one step later.
    ///
    /// So: `Before` and `After` are reachability and are counted whenever the
    /// funnel calls them; a `Point` is counted only when that exact `(site,
    /// point, mode)` was armed and therefore returns its specified `Kill` or
    /// `Error`. Reachability of a point in the generic sense is
    /// [`Self::reached`], which is recorded separately and is never what the
    /// bijection reads.
    pub fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        if let Some(open) = self.open_fast {
            if let Some(sequence) = self.fast.get_mut(open) {
                if !sequence.touched.contains(&site) {
                    sequence.touched.push(site);
                }
            }
        }
        let injection = match phase {
            HookPhase::Before | HookPhase::After => Injection::Proceed,
            HookPhase::Point { point, mode } => {
                if self.armed.contains(&(site, point, mode)) {
                    match mode {
                        InjectionMode::Kill => Injection::Kill,
                        InjectionMode::ErrorReturn => Injection::Error,
                    }
                } else {
                    Injection::Proceed
                }
            }
        };
        if let HookPhase::Point { point, mode } = phase {
            Self::record(&mut self.reached, site, HookPhase::Point { point, mode });
            if injection == Injection::Proceed {
                // Reached, and nothing was injected. Recorded as reachability
                // and as nothing else.
                return injection;
            }
        }
        Self::record(&mut self.observed, site, phase);
        injection
    }

    fn record(into: &mut Vec<Observation>, site: EffectSiteId, phase: HookPhase) {
        match into
            .iter_mut()
            .find(|seen| seen.site == site && seen.phase == phase)
        {
            Some(seen) => seen.count = seen.count.saturating_add(1),
            None => into.push(Observation {
                site,
                phase,
                count: 1,
            }),
        }
    }

    /// Begin recording an exact-base fast integration sequence under `name`.
    ///
    /// Everything a funnel hooks until [`Self::end_fast_sequence`] is recorded
    /// as having run inside this sequence, which is what a no-execution entry
    /// is measured against. A second `begin` closes the first.
    pub fn begin_fast_sequence(&mut self, name: &str) {
        self.end_fast_sequence();
        self.fast.push(FastSequence {
            name: name.to_owned(),
            touched: Vec::new(),
        });
        self.open_fast = Some(self.fast.len() - 1);
    }

    /// Stop recording the open fast sequence, keeping what it saw.
    pub fn end_fast_sequence(&mut self) {
        self.open_fast = None;
    }

    /// Every fast sequence the suite exercised.
    pub fn fast_sequences(&self) -> &[FastSequence] {
        &self.fast
    }

    /// The fast sequence of this name, if the suite exercised one.
    pub fn fast_sequence(&self, name: &str) -> Option<&FastSequence> {
        self.fast.iter().find(|sequence| sequence.name == name)
    }

    /// Every `(site, point-phase)` a funnel *reached*, armed or not.
    ///
    /// Kept apart from [`Self::coverage`] on purpose: reaching a point proves
    /// the hook is wired into the funnel, and injecting at it proves the mode
    /// does what the fault matrix says. Only the second is evidence of
    /// coverage, and only the first tells a suite author that an arming was
    /// mistargeted rather than the site unreached.
    pub fn reached(&self) -> &[Observation] {
        &self.reached
    }

    /// Whether a funnel reached this point at all, whatever was armed.
    pub fn reached_point(
        &self,
        site: EffectSiteId,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) -> bool {
        self.reached
            .iter()
            .any(|seen| seen.site == site && seen.phase == HookPhase::Point { point, mode })
    }

    /// Every `(site, phase)` observed, in first-observation order.
    pub fn coverage(&self) -> &[Observation] {
        &self.observed
    }

    /// Whether this exact `(site, phase)` was executed at least once.
    pub fn observed(&self, site: EffectSiteId, phase: HookPhase) -> bool {
        self.count(site, phase) > 0
    }

    /// How many times this exact `(site, phase)` was executed.
    pub fn count(&self, site: EffectSiteId, phase: HookPhase) -> u32 {
        self.observed
            .iter()
            .find(|seen| seen.site == site && seen.phase == phase)
            .map_or(0, |seen| seen.count)
    }

    /// Whether the harness saw this site execute at all, in any phase.
    ///
    /// Deliberately *not* what a no-execution record is measured against. That
    /// claim is scoped to a trace — "no staging, cherry-pick, or prepared-pin
    /// site executed **for any fast sequence**" — and its negation is
    /// [`FastSequence::ran`], per sequence. A suite that exercises a stale
    /// integration and a fast one touches all three sites and is exactly the
    /// suite ST-07 asks for; reading this answer as the no-execution test
    /// would reject it.
    pub fn touched(&self, site: EffectSiteId) -> bool {
        self.observed.iter().any(|seen| seen.site == site)
            || self.reached.iter().any(|seen| seen.site == site)
    }

    /// How many executions in total. Zero for a harness nothing has run
    /// through, whatever it has armed.
    pub fn executions(&self) -> u32 {
        self.observed.iter().map(|seen| seen.count).sum()
    }
}

// ---------------------------------------------------------------------------
// The registry format
// ---------------------------------------------------------------------------

/// What a registry entry is about.
///
/// The four kinds are different in kind, and keeping them apart at the type
/// level is what stops a residue class from being counted as a hook: a
/// [`Self::Residue`] entry cannot carry a [`HookPhase`], and a hook entry
/// cannot carry a [`ResidueClass`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryPhase {
    /// The hook before the primitive.
    Before,
    /// The hook after the primitive.
    After,
    /// A parent-side sub-effect point in one injection mode.
    Point {
        /// Which point.
        point: SubEffectPoint,
        /// Which mode.
        mode: InjectionMode,
    },
    /// A command-internal residue class. Never an executed hook.
    Residue {
        /// Which class.
        class: ResidueClass,
    },
    /// The record that this site did *not* execute — the fast integration
    /// path's assertion about staging, cherry-pick and prepared-pin sites.
    NoExecution,
}

impl EntryPhase {
    /// The hook phase this entry is about, where it is about one.
    pub const fn hook_phase(self) -> Option<HookPhase> {
        match self {
            Self::Before => Some(HookPhase::Before),
            Self::After => Some(HookPhase::After),
            Self::Point { point, mode } => Some(HookPhase::Point { point, mode }),
            Self::Residue { .. } | Self::NoExecution => None,
        }
    }

    /// The residue class this entry is about, where it is about one.
    pub const fn residue_class(self) -> Option<ResidueClass> {
        match self {
            Self::Residue { class } => Some(class),
            Self::Before | Self::After | Self::Point { .. } | Self::NoExecution => None,
        }
    }

    /// Whether `structure` gives this phase the site's *before-phase* resume
    /// action rather than an action of its own.
    ///
    /// Two phases, and the packet says so of both in the same words:
    /// `IdUnread` ("R27 object without a recorded id; resume action = the
    /// before-phase action") and the `Internal` residue class ("objects
    /// present and unreferenced, R27, with administrative residue ...; resume
    /// action equal to the before-phase action"). Both are prefixes in which
    /// nothing was published, so recovery is what recovery from *nothing*
    /// would have been — and an entry free to name a different action could
    /// table a resume that adopts a prefix no reader can authenticate.
    pub const fn resumes_as_before(self) -> bool {
        matches!(
            self,
            Self::Point {
                point: SubEffectPoint::IdUnread,
                ..
            } | Self::Residue { .. }
        )
    }

    /// The evidence label an entry in this phase must carry.
    pub const fn required_label(self) -> EvidenceLabel {
        match self {
            Self::Before | Self::After | Self::Point { .. } | Self::NoExecution => {
                EvidenceLabel::ExecutionObserved
            }
            Self::Residue { .. } => EvidenceLabel::RecoveryProven,
        }
    }
}

impl fmt::Display for EntryPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Before => f.write_str("before"),
            Self::After => f.write_str("after"),
            Self::Point { point, mode } => write!(
                f,
                "{point}/{}",
                match mode {
                    InjectionMode::Kill => "kill",
                    InjectionMode::ErrorReturn => "error-return",
                }
            ),
            Self::Residue { class } => f.write_str(class.name()),
            Self::NoExecution => f.write_str("no-execution"),
        }
    }
}

/// What is left durable after a fault at this entry's point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedResidue {
    /// The ledger rows still holding something. Empty is a real answer — the
    /// before phase of a creation, and a Windows containment point, each leave
    /// no row holding anything — but it is not the *only* answer a before phase
    /// has: see [`BeforeState`].
    pub rows: Vec<ResourceRow>,
    /// The concrete artifacts, in the fault matrix's own words.
    pub detail: String,
}

/// One residue element's synthetic-construction record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticRecord {
    /// Which element.
    pub element: ResidueElement,
    /// Whether it was constructed in a real temporary repository.
    pub constructed: bool,
    /// What the classifier answered for it.
    pub classified: ObjectResidue,
    /// Whether the tabled recovery converged.
    pub recovered: bool,
}

/// How many of each class a site's kill sampling observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassHistogram {
    /// Samples that classified `None`.
    pub none: u32,
    /// Samples that classified `Internal`. Zero is legal: hitting the internal
    /// window is recorded, never required.
    pub internal: u32,
    /// Samples that classified `After`.
    pub after: u32,
}

impl ClassHistogram {
    /// How many samples the histogram accounts for.
    pub const fn total(self) -> u32 {
        self.none
            .saturating_add(self.internal)
            .saturating_add(self.after)
    }
}

/// The real-command kill-sampling record for one site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplingRecord {
    /// The frozen sample count for this site.
    pub n: u32,
    /// What the classifier answered, by class.
    pub histogram: ClassHistogram,
    /// Samples that classified into no class at all. Any is a failure: the run
    /// would have durable state no tabled action recovers.
    pub unclassified: u32,
    /// Whether every sampled residue recovered by its classified action.
    pub recovered: bool,
}

/// An entry's evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Evidence {
    /// A hook phase or point ran, and this test recorded it.
    Executed {
        /// The test that executed it.
        test: String,
        /// Its pass record.
        passed: bool,
    },
    /// Nothing executed: every listed residue element was constructed and
    /// recovered, and the site was kill-sampled.
    RecoveryProven {
        /// One record per element the site's class lists.
        synthetic: Vec<SyntheticRecord>,
        /// The sampling record.
        sampling: SamplingRecord,
    },
    /// This site was asserted *not* to have executed.
    NotExecuted {
        /// The test that asserted it.
        test: String,
        /// Its pass record.
        passed: bool,
        /// The exercised fast sequences the absence was proved within.
        ///
        /// "The fast-path no-execution record shows that no staging,
        /// cherry-pick, or prepared-pin site executed **for any fast
        /// sequence**": the claim is about traces, so the evidence names the
        /// traces. An entry naming none is a claim about a process that may
        /// never have run an integration at all.
        sequences: Vec<String>,
    },
}

impl Evidence {
    /// The label this evidence's shape implies.
    pub const fn label(&self) -> EvidenceLabel {
        match self {
            Self::Executed { .. } | Self::NotExecuted { .. } => EvidenceLabel::ExecutionObserved,
            Self::RecoveryProven { .. } => EvidenceLabel::RecoveryProven,
        }
    }

    /// Whether this evidence claims a hook was executed.
    pub const fn claims_execution(&self) -> bool {
        matches!(self, Self::Executed { .. })
    }
}

/// One entry of the fault-injection registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    /// The site.
    pub site: EffectSiteId,
    /// What the entry is about.
    pub phase: EntryPhase,
    /// Which durable order, where the site has one.
    pub order: Option<ObservableOrder>,
    /// The fault-matrix row. Must equal the site's own.
    pub fault_row: FaultRow,
    /// What is left durable.
    pub expected_residue: ExpectedResidue,
    /// What a resume does about it, in the matrix's words.
    pub resume_action: String,
    /// How the claim was obtained.
    pub label: EvidenceLabel,
    /// The evidence itself.
    pub evidence: Evidence,
}

impl RegistryEntry {
    /// This entry's key: site, phase, order.
    pub fn key(&self) -> (EffectSiteId, EntryPhase, Option<ObservableOrder>) {
        (self.site, self.phase, self.order)
    }
}

/// Why the registry format refused an entry.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error(
        "`{site}`'s entry for the residue class `{class}` carries executed-hook evidence. A \
         residue class is a prefix inside an external command that no parent hook can observe; \
         its evidence is recovery-proven, and an entry claiming otherwise would report coverage \
         the suite does not have."
    )]
    ResidueClaimsExecution {
        /// The site.
        site: String,
        /// The class.
        class: &'static str,
    },

    #[error(
        "`{site}`'s `{phase}` entry carries recovery-proven evidence, but a hook phase is \
         observed by execution; recovery-proven is the label for what no hook can reach"
    )]
    HookClaimsRecoveryProof {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
    },

    #[error("`{site}`'s `{phase}` entry is labelled {found:?} but its phase requires {required:?}")]
    MislabelledEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// The label the entry carried.
        found: EvidenceLabel,
        /// The label its phase requires.
        required: EvidenceLabel,
    },

    #[error("`{site}` records fault row {found} but the site's row is {expected}")]
    WrongFaultRow {
        /// The site.
        site: String,
        /// What the entry said.
        found: FaultRow,
        /// What the site says.
        expected: FaultRow,
    },

    #[error(
        "`{site}`'s `{phase}` entry records order {found:?}, which is not an order a fault at \
         this site can leave observable"
    )]
    WrongOrder {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry said.
        found: Option<ObservableOrder>,
    },

    #[error("`{site}` exposes no `{point}` point in {mode:?} mode")]
    NoSuchPoint {
        /// The site.
        site: String,
        /// The point.
        point: SubEffectPoint,
        /// The mode.
        mode: InjectionMode,
    },

    #[error("`{site}` registers no residue class `{class}`")]
    NoSuchResidueClass {
        /// The site.
        site: String,
        /// The class.
        class: &'static str,
    },

    #[error(
        "`{site}`'s recovery-proven entry has no synthetic-construction record for the `{element:?}` \
         residue element its class lists"
    )]
    MissingSyntheticElement {
        /// The site.
        site: String,
        /// The element with no record.
        element: ResidueElement,
    },

    #[error(
        "`{site}`'s recovery-proven entry records a synthetic construction of `{element:?}`, which its class does not list"
    )]
    UnlistedSyntheticElement {
        /// The site.
        site: String,
        /// The element that does not belong.
        element: ResidueElement,
    },

    #[error("`{site}`'s `{phase}` entry names no test")]
    UnnamedTest {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
    },

    #[error(
        "`{site}` carries a no-execution record, but only the three sites a fast integration \
         sequence skips — Worktree.AddStaging, Object.ProposalCherryPick, Ref.PinPrepared — may \
         record that they did not run"
    )]
    NoExecutionNotSkipped {
        /// The site.
        site: String,
    },

    #[error(
        "`{site}`'s `{phase}` entry expects {found:?} to hold residue and this site's `{phase}` \
         leaves {expected:?}"
    )]
    WrongResidueRows {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry claimed.
        found: Vec<ResourceRow>,
        /// What the site's own semantics leave.
        expected: Vec<ResourceRow>,
    },

    #[error(
        "`{site}`'s no-execution record names no fast sequence it holds within. Absence is proved \
         inside an exercised trace or it is a claim about a process that ran no integration at all."
    )]
    UnwitnessedNoExecution {
        /// The site.
        site: String,
    },

    #[error("`{site}`'s `{phase}` entry names no resume action")]
    UnnamedResumeAction {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
    },

    #[error(
        "`{site}`'s `{phase}` entry describes its residue as `{found}` and this site's `{phase}` \
         leaves `{expected}`"
    )]
    WrongResidueDetail {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry claimed.
        found: String,
        /// What the site's own semantics leave.
        expected: &'static str,
    },

    #[error(
        "`{site}`'s `{phase}` entry tables the resume action `{found}` and the matrix tables \
         `{expected}` for this phase of this site"
    )]
    WrongResumeAction {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry claimed.
        found: String,
        /// What the site's own semantics table.
        expected: &'static str,
    },

    #[error("`{site}` already has an entry for `{phase}` in order {order:?}")]
    DuplicateEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// The order.
        order: Option<ObservableOrder>,
    },
}

/// The fault-injection registry: entries, and the format that refuses a bad
/// one.
///
/// `insert` is the format. Everything it refuses is refused *before* the
/// bijection check runs, so a registry that exists at all is one whose entries
/// are internally consistent with the enums; the bijection is then only about
/// whether the entries and the executions cover the inventory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FaultRegistry {
    entries: Vec<RegistryEntry>,
}

impl FaultRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entry, or say why it is not one.
    pub fn insert(&mut self, entry: RegistryEntry) -> Result<(), RegistryError> {
        validate_entry(&entry)?;
        if self.entries.iter().any(|held| held.key() == entry.key()) {
            return Err(RegistryError::DuplicateEntry {
                site: entry.site.name(),
                phase: entry.phase.to_string(),
                order: entry.order,
            });
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Every entry, in insertion order.
    pub fn entries(&self) -> &[RegistryEntry] {
        &self.entries
    }

    /// The entry for one key, if there is one.
    pub fn get(
        &self,
        site: EffectSiteId,
        phase: EntryPhase,
        order: Option<ObservableOrder>,
    ) -> Option<&RegistryEntry> {
        self.entries
            .iter()
            .find(|entry| entry.key() == (site, phase, order))
    }

    /// How many entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The whole of the format's validity rule, as one function.
///
/// Separate from [`FaultRegistry::insert`] so the bijection check can apply it
/// again to entries handed to it as a bare slice — a registry.json that was
/// hand-edited between a gate and a review never went through `insert`, and
/// "the bijection check fails on a residue-class entry claiming executed-hook
/// evidence" has to be true of that document too.
pub fn validate_entry(entry: &RegistryEntry) -> Result<(), RegistryError> {
    let site = entry.site;
    let name = site.name();

    if entry.fault_row != site.fault_row() {
        return Err(RegistryError::WrongFaultRow {
            site: name,
            found: entry.fault_row,
            expected: site.fault_row(),
        });
    }

    // A no-execution record is not about an order: nothing was performed, so
    // there is no effect to be durable before or after the append. Every other
    // phase carries the site's one order, or `None` where the site has none.
    let orders = site.observable_orders();
    let order_ok = match (entry.phase, entry.order) {
        (EntryPhase::NoExecution, order) => order.is_none(),
        (_, Some(order)) => orders.contains(&order),
        (_, None) => orders.is_empty(),
    };
    if !order_ok {
        return Err(RegistryError::WrongOrder {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.order,
        });
    }

    if entry.phase == EntryPhase::NoExecution && !site.skipped_on_fast_path() {
        return Err(RegistryError::NoExecutionNotSkipped { site: name });
    }

    // The expected residue and the tabled recovery are the site's own
    // semantics, not the entry's opinion of them. Without this an otherwise
    // complete entry can name an unrelated row — or none — describe residue
    // the site does not leave, and table a resume the matrix does not give it,
    // and the registry reads as evidence that a fault there was accounted for
    // when nothing checked any of the three.
    //
    // All three come from one call, so they cannot be checked against two
    // tables that disagree.
    let semantics = site.semantics(entry.phase);
    if entry.expected_residue.rows != semantics.rows {
        return Err(RegistryError::WrongResidueRows {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.expected_residue.rows.clone(),
            expected: semantics.rows,
        });
    }
    if entry.resume_action.trim().is_empty() {
        return Err(RegistryError::UnnamedResumeAction {
            site: name,
            phase: entry.phase.to_string(),
        });
    }
    if entry.expected_residue.detail != semantics.artifact.detail() {
        return Err(RegistryError::WrongResidueDetail {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.expected_residue.detail.clone(),
            expected: semantics.artifact.detail(),
        });
    }
    if entry.resume_action != semantics.action.text() {
        return Err(RegistryError::WrongResumeAction {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.resume_action.clone(),
            expected: semantics.action.text(),
        });
    }

    match entry.phase {
        EntryPhase::Point { point, mode } => {
            if !site.exposes(point, mode) {
                return Err(RegistryError::NoSuchPoint {
                    site: name,
                    point,
                    mode,
                });
            }
        }
        EntryPhase::Residue { class } => {
            if !site.registers(class) {
                return Err(RegistryError::NoSuchResidueClass {
                    site: name,
                    class: class.name(),
                });
            }
        }
        EntryPhase::Before | EntryPhase::After | EntryPhase::NoExecution => {}
    }

    // The load-bearing refusal, stated first and stated by itself: a residue
    // class is not a hook, and an entry that claims one executed is refused
    // whatever else about it is well-formed.
    if let Some(class) = entry.phase.residue_class() {
        if entry.evidence.claims_execution() || entry.label == EvidenceLabel::ExecutionObserved {
            return Err(RegistryError::ResidueClaimsExecution {
                site: name,
                class: class.name(),
            });
        }
    }
    if entry.phase.residue_class().is_none()
        && matches!(entry.evidence, Evidence::RecoveryProven { .. })
    {
        return Err(RegistryError::HookClaimsRecoveryProof {
            site: name,
            phase: entry.phase.to_string(),
        });
    }
    if entry.label != entry.phase.required_label() {
        return Err(RegistryError::MislabelledEntry {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.label,
            required: entry.phase.required_label(),
        });
    }
    if entry.label != entry.evidence.label() {
        return Err(RegistryError::MislabelledEntry {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.label,
            required: entry.evidence.label(),
        });
    }

    // The two evidence shapes that are legal for a hook entry are legal only
    // for the phase kind that matches them: `NoExecution` records that nothing
    // ran, and a before/after/point entry records that something did.
    match (&entry.phase, &entry.evidence) {
        (EntryPhase::NoExecution, Evidence::Executed { .. }) => {
            return Err(RegistryError::MislabelledEntry {
                site: name,
                phase: entry.phase.to_string(),
                found: EvidenceLabel::ExecutionObserved,
                required: EvidenceLabel::ExecutionObserved,
            });
        }
        (
            EntryPhase::Before | EntryPhase::After | EntryPhase::Point { .. },
            Evidence::NotExecuted { .. },
        ) => {
            return Err(RegistryError::MislabelledEntry {
                site: name,
                phase: entry.phase.to_string(),
                found: EvidenceLabel::ExecutionObserved,
                required: EvidenceLabel::ExecutionObserved,
            });
        }
        _ => {}
    }

    match &entry.evidence {
        Evidence::Executed { test, .. } => {
            if test.trim().is_empty() {
                return Err(RegistryError::UnnamedTest {
                    site: name,
                    phase: entry.phase.to_string(),
                });
            }
        }
        Evidence::NotExecuted {
            test, sequences, ..
        } => {
            if test.trim().is_empty() {
                return Err(RegistryError::UnnamedTest {
                    site: name,
                    phase: entry.phase.to_string(),
                });
            }
            if sequences.is_empty() || sequences.iter().any(|name| name.trim().is_empty()) {
                return Err(RegistryError::UnwitnessedNoExecution { site: name });
            }
        }
        Evidence::RecoveryProven { synthetic, .. } => {
            for element in site.residue_elements() {
                if !synthetic.iter().any(|record| record.element == *element) {
                    return Err(RegistryError::MissingSyntheticElement {
                        site: name,
                        element: *element,
                    });
                }
            }
            for record in synthetic {
                if !site.residue_elements().contains(&record.element) {
                    return Err(RegistryError::UnlistedSyntheticElement {
                        site: name,
                        element: record.element,
                    });
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// The bijection check
// ---------------------------------------------------------------------------

/// One way the bijection is not a bijection.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BijectionFailure {
    #[error("`{site}` was never observed executing its `{phase}` hook")]
    Unobserved {
        /// The site.
        site: String,
        /// The phase or point that never ran.
        phase: String,
    },

    #[error("`{site}` has no registry entry for `{phase}` in order {order:?}")]
    MissingEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// The order.
        order: Option<ObservableOrder>,
    },

    #[error("`{site}`'s `{phase}` entry has no passing evidence")]
    MissingEvidence {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
    },

    #[error("`{site}`'s sampling record classified {count} residues into no class at all")]
    UnclassifiableResidue {
        /// The site.
        site: String,
        /// How many.
        count: u32,
    },

    #[error(
        "`{site}`'s sampling record covers {n} samples but its histogram accounts for {counted}"
    )]
    SamplingUnaccounted {
        /// The site.
        site: String,
        /// The frozen sample count.
        n: u32,
        /// What the histogram and the unclassified count add up to.
        counted: u32,
    },

    #[error("`{site}` has a residue class but no sampling record: its frozen N is zero")]
    MissingSampling {
        /// The site.
        site: String,
    },

    #[error("`{site}`'s sampled residues did not all recover by their classified action")]
    UnrecoveredSampling {
        /// The site.
        site: String,
    },

    #[error("`{site}`'s residue-class entry claims executed-hook evidence")]
    ResidueClaimsExecution {
        /// The site.
        site: String,
    },

    #[error(
        "`{site}` carries a no-execution record and the suite exercised no fast sequence; an \
         empty harness is not evidence that a site a fast sequence skips was skipped"
    )]
    NoFastSequenceExercised {
        /// The site.
        site: String,
    },

    #[error(
        "`{site}`'s no-execution record does not hold within the exercised fast sequence \
         `{sequence}`"
    )]
    UnwitnessedFastSequence {
        /// The site.
        site: String,
        /// The sequence it says nothing about.
        sequence: String,
    },

    #[error("`{site}`'s no-execution record names `{sequence}`, which the harness never exercised")]
    UnknownFastSequence {
        /// The site.
        site: String,
        /// The sequence it named.
        sequence: String,
    },

    #[error("`{site}` executed during the fast sequence `{sequence}` its record says it skipped")]
    ExecutedInFastSequence {
        /// The site.
        site: String,
        /// The sequence it ran in.
        sequence: String,
    },

    #[error("the registry holds an entry for `{site}`, which the inventory under check does not")]
    EntryOutsideInventory {
        /// The site.
        site: String,
    },

    #[error(
        "`{site}` has {count} entries for `{phase}` in order {order:?}; a registry key is one \
         entry, and a checker that kept the first or the last would report whichever of two \
         disagreeing claims it happened to reach"
    )]
    DuplicateEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// The order.
        order: Option<ObservableOrder>,
        /// How many entries carried the key.
        count: usize,
    },

    #[error(
        "`{site}`'s `{phase}` entry resumes by `{found}` and its before-phase entry resumes by \
         `{expected}`; this phase's resume action is the before-phase action"
    )]
    ResumeActionNotBeforeAction {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry said.
        found: String,
        /// The site's before-phase action.
        expected: String,
    },

    #[error("`{site}`'s `{phase}` entry is not a valid entry: {reason}")]
    InvalidEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// Why.
        reason: String,
    },
}

/// The checked bijection over an inventory
/// (`fault_injection_registry.completeness_rule`).
///
/// Returns every way the claim fails; an empty answer is the claim holding.
/// `inventory` is a parameter rather than [`EffectSiteId::all`] because the
/// framework has to be checkable long before every site exists: PR3 runs it
/// over the handful of sites its self-test drives, and PR10 runs it over
/// everything. A slice that narrows the inventory narrows its own claim, which
/// is why the self-test also runs the check over the *full* claimed inventory
/// and asserts that it fails.
///
/// Legacy-scoped sites are skipped: `scope` says they are inventoried and
/// row-mapped and carry no fault-registry requirement.
pub fn check_bijection(
    inventory: &[EffectSiteId],
    harness: &HookHarness,
    entries: &[RegistryEntry],
    host: Host,
) -> Vec<BijectionFailure> {
    let mut failures = Vec::new();

    // `FaultRegistry::insert` refuses a duplicate key, but this function is
    // documented to take a bare slice precisely because a registry.json that
    // was hand-edited between a gate and a review never went through `insert`.
    // `structure` keys entries by site x phase x order, so two entries at one
    // key are two answers to one question — and `check_evidence` would silently
    // read the first of them. Restated here so the bare-slice path carries the
    // same invariant the constructor does.
    for (index, entry) in entries.iter().enumerate() {
        let key = entry.key();
        if entries[..index].iter().any(|held| held.key() == key) {
            // Already reported at its first occurrence.
            continue;
        }
        let count = entries.iter().filter(|held| held.key() == key).count();
        if count > 1 {
            failures.push(BijectionFailure::DuplicateEntry {
                site: entry.site.name(),
                phase: entry.phase.to_string(),
                order: entry.order,
                count,
            });
        }
    }

    for entry in entries {
        if let Err(error) = validate_entry(entry) {
            // Restated rather than folded into `InvalidEntry`, because ST-07
            // names this one direction explicitly and a reviewer looking for it
            // should find it under its own name.
            if matches!(error, RegistryError::ResidueClaimsExecution { .. }) {
                failures.push(BijectionFailure::ResidueClaimsExecution {
                    site: entry.site.name(),
                });
            } else {
                failures.push(BijectionFailure::InvalidEntry {
                    site: entry.site.name(),
                    phase: entry.phase.to_string(),
                    reason: error.to_string(),
                });
            }
        }
        if !inventory.contains(&entry.site) {
            failures.push(BijectionFailure::EntryOutsideInventory {
                site: entry.site.name(),
            });
        }
        // The relation `validate_entry` cannot make, because it sees one entry:
        // the phases `structure` gives "the before-phase action" have to name
        // the action this site's own before-phase entry names.
        if entry.phase.resumes_as_before() {
            let before = entries
                .iter()
                .find(|held| held.site == entry.site && held.phase == EntryPhase::Before);
            match before {
                Some(before) if before.resume_action == entry.resume_action => {}
                Some(before) => failures.push(BijectionFailure::ResumeActionNotBeforeAction {
                    site: entry.site.name(),
                    phase: entry.phase.to_string(),
                    found: entry.resume_action.clone(),
                    expected: before.resume_action.clone(),
                }),
                None => failures.push(BijectionFailure::MissingEntry {
                    site: entry.site.name(),
                    phase: EntryPhase::Before.to_string(),
                    order: entry.order,
                }),
            }
        }
    }

    for site in inventory {
        let site = *site;
        if !site.scope().is_claimed() {
            continue;
        }
        let name = site.name();

        // A no-execution record is *additional* evidence about the fast
        // traces, not an alternative to ordinary coverage. The three sites it
        // may be written for are Topology-scoped sites on the stale-candidate
        // path: a staging worktree is added, a proposal is cherry-picked and a
        // prepared pin is taken whenever the base is not exact, and
        // `completeness_rule` requires "every site x hook phase ... observed
        // executed at least once by the suite" of them like any other. What
        // `structure` says is narrower and is a statement about traces: "for a
        // fast sequence Worktree.AddStaging, Object.ProposalCherryPick, and
        // Ref.PinPrepared are asserted not executed".
        //
        // So this block adds requirements and removes none. It does not ask
        // whether the harness ever touched the site — a global `touched` test
        // rejects the valid evidence of a suite that exercised both paths, and
        // accepts nothing extra: execution inside a named fast sequence is
        // caught by `ExecutedInFastSequence` below, where the claim actually
        // lives. And it does not `continue`, because skipping the phase and
        // point bijection is how a site excuses itself from coverage by
        // declaring that it did not run.
        //
        // The condition is `skipped_on_fast_path()` — a property of the site —
        // and emphatically not "does a no-execution entry exist for it". The
        // predecessor asked the second question, so deleting all three records
        // made the entire branch unreachable and `check_bijection` reported
        // nothing: a completeness oracle that derives *whether* a requirement
        // exists from the very entries it is checking cannot report a missing
        // one. `completeness_rule` is explicit that "any missing link fails",
        // and ST-07 requires the record itself — "the fast-path no-execution
        // record shows that no staging, cherry-pick, or prepared-pin site
        // executed for any fast sequence". The `check_evidence` call at the end
        // of the block is what reports the record's absence, and it is now
        // reached whether or not the record is there.
        //
        // Exactly one record, not at least one: `check_evidence` finds the
        // entry at the key `(site, NoExecution, None)` and the duplicate sweep
        // above refuses a second at the same key, so the two together admit one
        // and only one.
        if site.skipped_on_fast_path() {
            // "The fast-path no-execution record shows that no staging,
            // cherry-pick, or prepared-pin site executed for any fast
            // sequence" — so there has to *be* a fast sequence, the record has
            // to hold within every one the suite exercised, and it may not
            // name one that never happened. Without all three an empty harness
            // substantiates the claim, which is the same false report as an
            // empty coverage table.
            if harness.fast_sequences().is_empty() {
                failures.push(BijectionFailure::NoFastSequenceExercised { site: name.clone() });
            }
            let claimed: Vec<&str> = entries
                .iter()
                .filter(|entry| entry.site == site && entry.phase == EntryPhase::NoExecution)
                .filter_map(|entry| match &entry.evidence {
                    Evidence::NotExecuted { sequences, .. } => Some(sequences),
                    _ => None,
                })
                .flatten()
                .map(String::as_str)
                .collect();
            for sequence in harness.fast_sequences() {
                if !claimed.contains(&sequence.name()) {
                    failures.push(BijectionFailure::UnwitnessedFastSequence {
                        site: name.clone(),
                        sequence: sequence.name().to_owned(),
                    });
                } else if sequence.ran(site) {
                    failures.push(BijectionFailure::ExecutedInFastSequence {
                        site: name.clone(),
                        sequence: sequence.name().to_owned(),
                    });
                }
            }
            for sequence in &claimed {
                if harness.fast_sequence(sequence).is_none() {
                    failures.push(BijectionFailure::UnknownFastSequence {
                        site: name.clone(),
                        sequence: (*sequence).to_owned(),
                    });
                }
            }
            check_evidence(&mut failures, entries, site, EntryPhase::NoExecution, None);
        }

        let mut required = vec![EntryPhase::Before, EntryPhase::After];
        for point in site.sub_effects() {
            if !point.platform().required_on(host) {
                continue;
            }
            for mode in point.modes() {
                required.push(EntryPhase::Point {
                    point: *point,
                    mode: *mode,
                });
            }
        }

        for phase in required {
            #[expect(
                clippy::expect_used,
                reason = "before, after and point phases all have a hook phase"
            )]
            let hook = phase
                .hook_phase()
                .expect("before, after and point phases all have a hook phase");
            if !harness.observed(site, hook) {
                failures.push(BijectionFailure::Unobserved {
                    site: name.clone(),
                    phase: phase.to_string(),
                });
            }
            let orders = site.observable_orders();
            if orders.is_empty() {
                check_evidence(&mut failures, entries, site, phase, None);
            } else {
                for order in orders {
                    check_evidence(&mut failures, entries, site, phase, Some(*order));
                }
            }
        }

        for class in site.residue_classes() {
            let phase = EntryPhase::Residue { class: *class };
            let orders = site.observable_orders();
            let order = if orders.is_empty() {
                None
            } else {
                Some(orders[0])
            };
            check_evidence(&mut failures, entries, site, phase, order);
        }
    }

    failures
}

/// Whether one required key has an entry, and whether that entry's evidence
/// says anything.
fn check_evidence(
    failures: &mut Vec<BijectionFailure>,
    entries: &[RegistryEntry],
    site: EffectSiteId,
    phase: EntryPhase,
    order: Option<ObservableOrder>,
) {
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.key() == (site, phase, order))
    else {
        failures.push(BijectionFailure::MissingEntry {
            site: site.name(),
            phase: phase.to_string(),
            order,
        });
        return;
    };

    match &entry.evidence {
        Evidence::Executed { passed, .. } | Evidence::NotExecuted { passed, .. } => {
            if !passed {
                failures.push(BijectionFailure::MissingEvidence {
                    site: site.name(),
                    phase: phase.to_string(),
                });
            }
        }
        Evidence::RecoveryProven {
            synthetic,
            sampling,
        } => {
            for record in synthetic {
                if !record.constructed
                    || !record.recovered
                    || record.classified != ObjectResidue::Internal
                {
                    failures.push(BijectionFailure::MissingEvidence {
                        site: site.name(),
                        phase: phase.to_string(),
                    });
                    break;
                }
            }
            if sampling.n == 0 {
                failures.push(BijectionFailure::MissingSampling { site: site.name() });
            }
            if sampling.unclassified > 0 {
                failures.push(BijectionFailure::UnclassifiableResidue {
                    site: site.name(),
                    count: sampling.unclassified,
                });
            }
            let counted = sampling
                .histogram
                .total()
                .saturating_add(sampling.unclassified);
            if counted != sampling.n {
                failures.push(BijectionFailure::SamplingUnaccounted {
                    site: site.name(),
                    n: sampling.n,
                    counted,
                });
            }
            if !sampling.recovered {
                failures.push(BijectionFailure::UnrecoveredSampling { site: site.name() });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// effect_sites.json
// ---------------------------------------------------------------------------

/// One point of a site, as the generated inventory records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointExport {
    /// Which point.
    pub point: SubEffectPoint,
    /// The host it exists on.
    pub platform: Platform,
    /// Every mode it supports.
    pub modes: Vec<InjectionMode>,
}

/// One residue class of a site, as the generated inventory records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidueClassExport {
    /// Which class.
    pub class: ResidueClass,
    /// The label it must carry. Always recovery-proven.
    pub label: EvidenceLabel,
    /// The classifier outcome it is the class of.
    pub classified_as: ObjectResidue,
    /// Every element its synthetic construction must build.
    pub elements: Vec<ResidueElement>,
}

/// One site of `effect_sites.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSiteExport {
    /// The dotted name.
    pub site: EffectSiteId,
    /// Its group.
    pub group: FunnelGroup,
    /// Its row.
    pub row: ResourceRow,
    /// The row's enforcement domain.
    pub domain: EnforcementDomain,
    /// Its adjacency.
    pub adjacent: Adjacent,
    /// The orders a fault here can leave observable.
    pub observable_orders: Vec<ObservableOrder>,
    /// Its fault-matrix row.
    pub fault_row: FaultRow,
    /// Its scope.
    pub scope: SiteScope,
    /// The module its funnel lives in.
    pub module: String,
    /// Whether it performs no effect.
    pub read_only: bool,
    /// Its parent-side sub-effect points.
    pub sub_effect_points: Vec<PointExport>,
    /// Its residue classes.
    pub residue_classes: Vec<ResidueClassExport>,
}

/// The generated inventory, in group and declaration order.
///
/// Generated *from* the enums, so it cannot describe a site that does not
/// exist and cannot omit one that does.
pub fn effect_sites() -> Vec<EffectSiteExport> {
    EffectSiteId::all()
        .into_iter()
        .map(|site| EffectSiteExport {
            site,
            group: site.group(),
            row: site.row(),
            domain: site.row().domain(),
            adjacent: site.adjacent(),
            observable_orders: site.observable_orders().to_vec(),
            fault_row: site.fault_row(),
            scope: site.scope(),
            module: site.module().to_owned(),
            read_only: site.is_read_only(),
            sub_effect_points: site
                .sub_effects()
                .iter()
                .map(|point| PointExport {
                    point: *point,
                    platform: point.platform(),
                    modes: point.modes().to_vec(),
                })
                .collect(),
            residue_classes: site
                .residue_classes()
                .iter()
                .map(|class| ResidueClassExport {
                    class: *class,
                    label: class.label(),
                    classified_as: class.classified_as(),
                    elements: site.residue_elements().to_vec(),
                })
                .collect(),
        })
        .collect()
}

/// `effect_sites.json`, pretty-printed for a gate report to attach.
pub fn effect_sites_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&effect_sites())
}

#[cfg(test)]
mod tests;
