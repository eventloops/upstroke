//! The shared vocabulary an effect site is described in: the funnel groups,
//! the resource-ledger rows, the durable events and adjacency, the fault-matrix
//! rows, the scopes, the injection modes and hosts, and the parent-side
//! sub-effect points.
//!
//! Split out of `topology::effects`; the parent re-exports every item here, so
//! `crate::topology::effects::FunnelGroup` and its siblings are unchanged
//! paths.

use std::fmt;

use serde::{Deserialize, Serialize};

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
/// typo can invent. The two lists are asserted equal element-for-element by
/// `topology::effects::tests::the_adjacency_vocabulary_is_the_logs_vocabulary`,
/// so a change to the vocabulary breaks **that assertion** rather than silently
/// leaving a site pointing at an append that no longer exists.
///
/// The guard is named rather than called "a unit test in this module" because
/// the split moved this type out of the root: "this module" now reads as
/// `vocab`, and the assertion is in a *sibling* of it. The property is
/// unchanged -- the test still fires -- but a reader who took the locality at
/// its word would be looking for the guard in the wrong place, and the danger
/// in that is removing the real one believing the documented one applies.
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
/// the answer for a site that *is* the append (the whole [`EventSite`](crate::topology::effects::EventSite) append
/// group), and for a site that runs outside any run's log at all — the husk
/// census removes a run directory belonging to a run whose log it has refused
/// to fold.
///
/// The value decides [`EffectSiteId::observable_orders`](crate::topology::effects::EffectSiteId::observable_orders), which is what the
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
