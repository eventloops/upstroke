//! The **startup container census** — discovery, the incarnation-aware
//! ownership rule, and the reclaim driver.
//!
//! `decisions.sequential_substrate.startup_census` step (a):
//!
//! > performed by every topology write command (**run, resume**) after taking
//! > the worktree lock and before any run-id use for creation, run-lock
//! > acquisition for a fresh run, slot or reservation initialization, admission,
//! > credential-volume use, or probe (a resume takes its run lock first,
//! > establishes the stable-prefix barrier of recovery step (a1), then censuses,
//! > so no census reclaim decided from the fold precedes durability of the
//! > prefix it is decided from) … (a) global container reclaim over
//! > `<R>/containers` and docker ps by the private-root label under the
//! > incarnation-aware liveness rule
//!
//! ## What this module owns and what it deliberately does not
//!
//! It owns the **decision** and the **sequence**. It performs no container
//! effect itself: every reclaim goes through [`super::reclaim`], the funnel API
//! whose five steps are the packet's own order, and the four effectful methods
//! of [`ContainerRuntime`] are on `clippy.toml`'s disallowed list so calling one
//! from here is a build error (`decisions.effect_site_inventory.mechanism`
//! (1)-(2)). What it calls directly is read-only: the namespace scan, `docker
//! ps` by label, and the owner-liveness probe.
//!
//! ## The consumers it precedes, and why they are a token rather than a comment
//!
//! `crash_reconstruction`: "the census completes **before** slot/reservation
//! state is initialized, **before** admission, **before** any invocation uses an
//! agent's credential volume, and **before** this incarnation's probes". Four
//! consumers, none of which exists in this slice — slots and admission are
//! PR11's, the credential-volume turn and the RunnerPreflight probes are PR7's.
//!
//! A comment saying "call this first" holds none of that. So the census returns
//! a [`CensusComplete`], whose fields are private to this module and which no
//! other code can construct: the four consumers take one by reference when they
//! are built, and until they are, the token is the thing this slice can hold and
//! the next slice can thread. [`crate::rundir::PrivateHalfProof`] is the same
//! device for the same reason — a proof obligation carried in the type system
//! rather than in prose.
//!
//! **What a later slice must connect** is stated once, here, so it is not
//! rediscovered: PR7's `TopologyRun` calls [`run_startup_census`] after the
//! worktree lock (fresh) or after the run lock and recovery step (a1) (resume),
//! and passes the resulting `&CensusComplete` into slot/reservation
//! initialization, admission, the first credential-volume use, and
//! `RunnerPreflight`. [`census_returns_the_only_token_that_reaches_a_consumer`]
//! is the source census that says no other construction exists.

// `PR6-LANEF-004`: the Container funnel's module-level allow is an INNER
// attribute, and a Rust lint level is scoped by the MODULE TREE rather than by
// the file, so every out-of-line child of `runner::container` inherited it --
// measured, a `ContainerRuntime::start` planted in a child module passed
// `cargo clippy --all-targets --all-features -- -D warnings`. Re-denying here
// is what makes `decisions.effect_site_inventory.mechanism` (1)'s BUILD error
// true of a lane's module, which is the leg the source census cannot supply.
// Enforced for every file in this directory by `runner::container::tests::
// every_child_module_of_the_container_funnel_states_its_own_lint_level`.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::UpstrokeError;
use crate::topology::effects::ContainerSite;

use super::intent::{
    ContainerName, LABEL_INCARNATION, LABEL_PRIVATE_ROOT, LABEL_RUN, LABEL_RUN_DIR,
};
use super::runtime::{ContainerRuntime, OwnerLiveness, RuntimeError, RuntimeOp};
use super::{ContainerHooks, FoundIntent, GitView, OrphanWindow, list_intents, orphan_window};

// ---------------------------------------------------------------------------
// R19: where an orphan view is found
// ---------------------------------------------------------------------------

/// The directory the disposable Git views live under, inside `<R>`.
///
/// R19's lifecycle is "pruned on complete or cancel; **orphan views reclaimed
/// during dead-owner or dead-incarnation container reclaim**", and a census
/// reclaiming an orphan holds no [`super::Launched`] to read a path out of — the
/// six intent fields the packet fixes carry no view path. So the location has to
/// be *derivable* from what a census already knows, which is the private root
/// and the container name.
///
/// **Cross-lane seam, and it is one definition rather than an agreement.** The
/// invocation path that *materialises* the projection reaches this function
/// too: [`super::exec::view_dir`] is a call to [`view_path`] and nothing else.
/// It used to be a second copy of the same `join`, and an independent review
/// measured what that costs — changing only the producer to
/// `<R>/views-v2/<name>` passed the whole suite and orphaned every view the
/// census would have pruned. A convention maintained in two places is a
/// convention only until somebody edits one of them.
pub const VIEWS_DIR: &str = "views";

/// `<R>/views/<container-name>` — the R19 view of one container invocation.
///
/// The single definition: [`super::exec::view_dir`] delegates here, and
/// `exec::tests::the_view_path_the_census_prunes_is_the_one_the_invocation_mounts`
/// holds the value against a literal rather than against either caller.
#[must_use]
pub fn view_path(private_root: &Path, name: &ContainerName) -> PathBuf {
    private_root.join(VIEWS_DIR).join(name.as_str())
}

/// The value of the `upstroke.private_root` label for this root.
///
/// **One definition, in the module that owns [`LABEL_PRIVATE_ROOT`]**, rather
/// than the rendering re-derived here and pinned to the funnel's by a test. The
/// two spellings were byte-identical and still not injective — `<R>/a\b` and
/// `<R>/a/b` are different roots on Unix and rendered to one label — and
/// repairing that in one copy would have left a census filtering on a value no
/// container carries, which discovers nothing and reports a clean machine.
/// [`super::intent::private_root_label`] carries the injectivity argument.
///
/// [`the_private_root_label_this_census_filters_on_is_the_one_the_intent_writes`]
/// still pins the census's filter value against `ContainerIntent::labels`; it
/// is now true by construction, and its oracle is an independent table of
/// hand-computed encodings rather than the other copy.
#[must_use]
pub fn private_root_label(private_root: &Path) -> String {
    super::intent::private_root_label(private_root)
}

// ---------------------------------------------------------------------------
// Recovery step (a1): the stable-prefix barrier
// ---------------------------------------------------------------------------

/// A byte range of the event log, by length and content digest.
///
/// Two fields and not one: "**proven its bytes and boundary unchanged**" is two
/// claims, and a digest alone would let a boundary move under a prefix whose
/// bytes happen to hash the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixBytes {
    /// The boundary — how many bytes of the log the prefix is.
    pub len: u64,
    /// `sha256` of exactly those bytes, lowercase hex.
    pub sha256: String,
}

impl PrefixBytes {
    /// Measure a prefix.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(64);
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
        }
        Self {
            len: bytes.len() as u64,
            sha256: hex,
        }
    }
}

/// How far the surviving event-log prefix has been **synced**.
///
/// `crash_reconstruction`: "after the stable-prefix barrier of step (a1) has
/// **synced** the surviving event-log prefix, proven it stable, and
/// checked-replayed it, so that no fold-derived reclaim decision precedes
/// durability".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixSync {
    /// The number of bytes the recovery step has made durable.
    pub synced_len: u64,
}

/// The reread: the whole file read a second time, so its bytes and boundary can
/// be **proven** unchanged rather than assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixReread {
    /// What the first read saw.
    pub first: PrefixBytes,
    /// What rereading the whole file saw.
    pub second: PrefixBytes,
}

/// The checked replay: which bytes the fold was actually computed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixReplay {
    /// The prefix the replay consumed.
    pub replayed: PrefixBytes,
}

/// Recovery step (a1), established — the evidence a resume's census is
/// entitled to decide reclaim from the fold.
///
/// **Four separately droppable predicates**, each with its own refusal, because
/// "reclaim decided from a prefix that was synced but not proven stable, or
/// proven stable but not replayed, is reclaim on unproven authority":
///
/// 1. the reread's **boundary** equals the first read's,
/// 2. the reread's **bytes** equal the first read's,
/// 3. the **synced** extent covers the prefix the decision rests on,
/// 4. the replay consumed **exactly those reread bytes**.
///
/// The type has no public constructor and no public fields, so a resume census
/// cannot be reached with a barrier that was not established.
///
/// **What a later slice must connect**: PR7's recovery step (a1) supplies the
/// three measurements — it owns the event log, its `sync_data`, and the fold.
/// This slice owns the four comparisons and the refusals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StablePrefixBarrier {
    boundary: u64,
    digest: String,
}

impl StablePrefixBarrier {
    /// Establish the barrier, or refuse and say which predicate failed.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when the boundary moved, the bytes changed, the
    /// prefix is not durable to its boundary, or the replay was of other bytes.
    pub fn establish(
        sync: PrefixSync,
        reread: &PrefixReread,
        replay: &PrefixReplay,
    ) -> Result<Self, UpstrokeError> {
        if reread.first.len != reread.second.len {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the surviving event-log prefix was {} bytes and rereading it found {}; \
                     recovery step (a1) proves the prefix's bytes AND boundary unchanged before \
                     the census, so no fold-derived reclaim decision precedes durability of the \
                     prefix it is decided from",
                    reread.first.len, reread.second.len
                ),
            });
        }
        if reread.first.sha256 != reread.second.sha256 {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the surviving event-log prefix hashed `{}` and rereading the same {} bytes \
                     hashed `{}`; recovery step (a1) proves the prefix stable before the census",
                    reread.first.sha256, reread.first.len, reread.second.sha256
                ),
            });
        }
        if sync.synced_len < reread.first.len {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "recovery step (a1) synced {} bytes of the event log and the prefix the \
                     census would decide from is {} bytes; a reclaim decided from a prefix that \
                     is not durable is a reclaim decided from something a crash can take back",
                    sync.synced_len, reread.first.len
                ),
            });
        }
        if replay.replayed != reread.first {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "recovery step (a1) checked-replayed {} bytes hashing `{}` and the prefix \
                     proven stable is {} bytes hashing `{}`; the replay must consume exactly the \
                     reread bytes, or the census decides from a fold of something else",
                    replay.replayed.len,
                    replay.replayed.sha256,
                    reread.first.len,
                    reread.first.sha256
                ),
            });
        }
        Ok(Self {
            boundary: reread.first.len,
            digest: reread.first.sha256.clone(),
        })
    }

    /// The boundary the fold was computed to.
    #[must_use]
    pub const fn boundary(&self) -> u64 {
        self.boundary
    }

    /// The digest of the prefix the fold was computed over.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

// ---------------------------------------------------------------------------
// Who is censusing
// ---------------------------------------------------------------------------

/// Why this process is at a census, and what it is entitled to conclude.
///
/// The two arms are not decoration: arm (i) of the liveness rule is "owner run
/// **==** the run this process is driving (**this process holds its
/// run.lock**)", and a fresh run holds no run lock at census time —
/// `startup_census` puts the census "**before** … run-lock acquisition for a
/// fresh run". So a fresh run has no own-run arm at all and every candidate goes
/// to arm (ii), while a resume has one and owes recovery step (a1) first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CensusStart {
    /// `upstroke run`: the worktree lock is held, no run lock is, and the run id
    /// has not been used for creation yet.
    FreshRun {
        /// This process's per-process ULID.
        incarnation: String,
    },
    /// `upstroke resume`: this process holds `run_id`'s run lock and has
    /// established recovery step (a1).
    Resume {
        run_id: String,
        /// This process's per-process ULID. **Never read from lock-file
        /// contents** — `run.lock` content is never read, and a Windows
        /// exclusive lock makes it unreadable to non-holders. It is the value
        /// recorded in `run_resumed(4)`, handed in.
        incarnation: String,
        /// Recovery step (a1), established before this census.
        barrier: StablePrefixBarrier,
    },
}

impl CensusStart {
    /// The run this process is driving, if it holds one's lock.
    #[must_use]
    pub fn own_run(&self) -> Option<&str> {
        match self {
            Self::FreshRun { .. } => None,
            Self::Resume { run_id, .. } => Some(run_id),
        }
    }

    /// This process's incarnation.
    #[must_use]
    pub fn incarnation(&self) -> &str {
        match self {
            Self::FreshRun { incarnation } | Self::Resume { incarnation, .. } => incarnation,
        }
    }

    /// Which write command this is, for the report.
    #[must_use]
    pub const fn command(&self) -> WriteCommand {
        match self {
            Self::FreshRun { .. } => WriteCommand::Run,
            Self::Resume { .. } => WriteCommand::Resume,
        }
    }
}

/// The topology write commands that perform a startup census.
///
/// "performed by **every topology write command (run, resume)**" — both, not
/// resume only. A census guarded behind resume-only logic lets a dead run's
/// containers survive into a fresh run's admission, which is the failure the
/// sentence names two commands to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WriteCommand {
    Run,
    Resume,
}

impl WriteCommand {
    /// Both of them, written out so a grid over write commands is a grid over
    /// all of them.
    pub const ALL: &'static [Self] = &[Self::Run, Self::Resume];

    /// As the report writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Resume => "resume",
        }
    }
}

// ---------------------------------------------------------------------------
// The liveness rule
// ---------------------------------------------------------------------------

/// How one container's owner classifies.
///
/// `crash_reconstruction`, verbatim:
///
/// > (i) owner run == the run this process is driving (this process holds its
/// > run.lock): incarnation != this process's incarnation -> **dead by
/// > construction** (the run lock is exclusive, so only one incarnation of a run
/// > is ever live) -> reclaim; incarnation == this process's incarnation
/// > **cannot exist at census time** (the census precedes every invocation incl.
/// > this incarnation's probes) and **is refused if observed**; (ii) owner run
/// > != this run: probe that run's run.lock non-blocking; **free** -> dead owner
/// > -> reclaim **every** container of that run **whatever its incarnation**;
/// > **held** -> live owner -> **never touched**
///
/// **The own-incarnation refusal belongs to arm (i), and that is a reading —
/// the opposite of the one that shipped** (`PR6-RECOV-003`).
///
/// The rule above is an exhaustive dichotomy on the *owner run*, and the
/// refusal clause is written **inside arm (i)**, after its colon. Arm (ii) then
/// says what it does with the incarnation, in as many words: "reclaim every
/// container of that run **whatever its incarnation**". "Whatever" includes
/// this process's own, so the two clauses do not overlap and there is nothing
/// to adjudicate between them; `transaction_fault_matrix.T-CONTAINER
/// .resume_action` states the same rule in the same order — "owner run == this
/// run -> incarnation != this process's incarnation -> … -> reclaim; owner run
/// != this run -> probe the owner's run.lock non-blocking; held -> skip; free
/// -> reclaim".
///
/// The shipped code hoisted the incarnation comparison **in front of** the
/// split and refused on it under any run id, on the strength of
/// `expected_failures_refusals[7]` — "an intent naming this process's own
/// incarnation at census time is refused" — read as unqualified. That line is
/// the contract's one-sentence summary of arm (i)'s clause, and a summary that
/// drops a qualifier is not a second rule. Two live passages state the
/// classification arm-first; one summary states it arm-free; the classification
/// wins.
///
/// **What the hoisted check cost.** A foreign run whose recorded incarnation
/// equals this process's never reached arm (ii) at all, so its owner's lock was
/// never probed and a perfectly dead owner's container blocked every write
/// command under that private root, permanently, with no operator remedy. The
/// hoisted check was not the safer choice either way: arm (ii) reclaims only on
/// a **free** lock, so the live-owner container it was supposed to protect is
/// protected by the probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ownership {
    /// Arm (i), `incarnation != mine`: dead by construction, because the run
    /// lock is exclusive and this process holds it.
    OwnRunEarlierIncarnation,
    /// Arm (i), `incarnation == mine`: cannot exist at census time — the census
    /// precedes every invocation including this incarnation's probes — and is
    /// **refused** if observed. `expected_failures_refusals[7]`.
    OwnRunThisIncarnation,
    /// Arm (ii), the owner's `run.lock` is **free**: reclaim, whatever the
    /// container's incarnation.
    ForeignRunDeadOwner,
    /// Arm (ii), the owner's `run.lock` is **held**: never touched, whatever the
    /// container's incarnation — "that owner reclaims its own earlier
    /// incarnations at its own startup census, which precedes its admission".
    ForeignRunLiveOwner,
}

impl Ownership {
    /// Every classification, written out.
    pub const ALL: &'static [Self] = &[
        Self::OwnRunEarlierIncarnation,
        Self::OwnRunThisIncarnation,
        Self::ForeignRunDeadOwner,
        Self::ForeignRunLiveOwner,
    ];

    /// Whether this census reclaims the container.
    #[must_use]
    pub const fn reclaims(self) -> bool {
        match self {
            Self::OwnRunEarlierIncarnation | Self::ForeignRunDeadOwner => true,
            Self::OwnRunThisIncarnation | Self::ForeignRunLiveOwner => false,
        }
    }

    /// Whether observing it refuses the write command.
    #[must_use]
    pub const fn refuses(self) -> bool {
        matches!(self, Self::OwnRunThisIncarnation)
    }

    /// As the report writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OwnRunEarlierIncarnation => "own-run-earlier-incarnation",
            Self::OwnRunThisIncarnation => "own-run-this-incarnation",
            Self::ForeignRunDeadOwner => "foreign-run-dead-owner",
            Self::ForeignRunLiveOwner => "foreign-run-live-owner",
        }
    }
}

/// The two-arm rule, as a pure decision.
///
/// **Arm (ii) does not consult the incarnation at all**, and that is the whole
/// of the residual the packet names: "a container of a dead incarnation of a
/// **live** run may run until that run's own census reclaims it … classified as
/// concurrent live-coordinator sharing of R20 (existing operator configuration;
/// **out of scope**)". An implementation that reclaimed dead incarnations of
/// live runs would pass every test that varies only one of `{owner run}` and
/// `{incarnation}`, and would kill a container a live coordinator is spending
/// through.
///
/// **Arm (i) does not probe the lock at all**: this process holds it, so a probe
/// would be asking whether it is itself alive.
///
/// **The owner-run split comes first, and the incarnation is read only inside
/// the arm that reads it** — see [`Ownership`] for the passages and for what
/// hoisting the comparison in front of the split cost (`PR6-RECOV-003`).
///
/// The probe is [`OwnerLiveness::is_running`], called **once**, because
/// `T-CONTAINER.resume_action` says "probe the owner's run.lock
/// **non-blocking**; held -> skip". A retry loop around a held lock is a census
/// that waits on a live neighbour, which is a stall at every write-command
/// start; `census::tests::the_owner_lock_is_probed_exactly_once_per_candidate`
/// asserts the call count rather than the answer.
#[must_use]
pub fn classify_ownership(
    start: &CensusStart,
    owner_run_id: &str,
    owner_incarnation: &str,
    owner_run_dir: &Path,
    liveness: &dyn OwnerLiveness,
) -> Ownership {
    if start.own_run() == Some(owner_run_id) {
        // Arm (i). The run lock is exclusive and this process holds it, so
        // every other incarnation of this run is dead; this one cannot exist.
        return if owner_incarnation == start.incarnation() {
            Ownership::OwnRunThisIncarnation
        } else {
            Ownership::OwnRunEarlierIncarnation
        };
    }
    // Arm (ii). The incarnation is deliberately not in scope here: "reclaim
    // every container of that run whatever its incarnation".
    if liveness.is_running(owner_run_dir) {
        Ownership::ForeignRunLiveOwner
    } else {
        Ownership::ForeignRunDeadOwner
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Which half of discovery found a container.
///
/// "discovery at every write-command start **scans the whole namespace
/// `<R>/containers`** of the command's authorized private root **and** docker ps
/// by `upstroke.private_root`" — two halves, and a container may be in either or
/// both. `{intent present} × {container present}` is a 2×2 grid and every cell
/// is a real state: intent-only is a crash after the intent write and before
/// `docker create`, or a Unix reaper that already killed and removed the
/// container; label-only is "a labeled container without an intent … treated as
/// an orphan of its labeled run and incarnation under the same rule".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiscoveredBy {
    /// A record in `<R>/containers` with no container in the runtime.
    IntentOnly,
    /// A container carrying `upstroke.private_root` with no record.
    LabelOnly,
    /// Both halves agree it exists.
    IntentAndLabel,
}

impl DiscoveredBy {
    /// Every cell of the grid.
    pub const ALL: &'static [Self] = &[Self::IntentOnly, Self::LabelOnly, Self::IntentAndLabel];

    /// As the report writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::IntentOnly => "intent-only",
            Self::LabelOnly => "label-only",
            Self::IntentAndLabel => "intent-and-label",
        }
    }
}

/// The boundary identity a reclaimed container is reported against.
///
/// `T-CONTAINER.resume_action`: "the census report names each reclaimed
/// container's boundary from its **`runner_policy_sha256`**". The intent carries
/// that field, so a container with a record has an exact boundary. A labeled
/// container with **no** record has none from this side; PR7's owner record
/// (`owner.json.runner` at P3b) is the other half, and this variant says so
/// rather than inventing a digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Boundary {
    /// From the intent's `runner_policy_sha256`.
    FromIntent(String),
    /// No record: the boundary is the owner record's, which is PR7's half.
    NoIntentRecord,
}

impl Boundary {
    /// The digest, when this side has one.
    #[must_use]
    pub fn digest(&self) -> Option<&str> {
        match self {
            Self::FromIntent(digest) => Some(digest),
            Self::NoIntentRecord => None,
        }
    }
}

/// One container the census has to decide about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: ContainerName,
    /// Owner run id — from the record, or from `upstroke.run`.
    pub run_id: String,
    /// Owning incarnation — from the record, or from `upstroke.incarnation`.
    pub incarnation: String,
    /// The owner's **public** run directory, which is what the lock probe is
    /// asked about.
    pub run_dir: PathBuf,
    pub boundary: Boundary,
    pub discovered_by: DiscoveredBy,
    /// Where the record is, when there is one.
    pub intent_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// The report, and the token
// ---------------------------------------------------------------------------

/// How the identity that owned a reclaimed container has to be settled.
///
/// `T-CONTAINER.resume_action` ends "… **then settle the owning identity
/// interrupted**", and `T-CONTAINER.authoritative_state` opens "**unknown
/// spend**". Those two clauses are one answer and there is only one of it:
/// *every* container a census reclaims belonged to an attempt or verification
/// (or to a probe's pre-run husk) that was cut off mid-flight, and no census can
/// know what the vendor charged for it.
///
/// **The value is a constant, and that is the whole point** (`PR6-RECOV-006`).
/// The state that tempts an implementation to say otherwise is
/// [`DiscoveredBy::IntentOnly`]: the container is not there, so it *looks* like
/// nothing ran — but that is exactly the post-Unix-reaper state, where the
/// reaper killed and removed a container whose invocation had been running and
/// spending for however long. Deriving the settlement from `discovered_by`
/// would record those attempts as completed, with their spend unaccounted. So
/// the settlement is a field of [`Reclaimed`] rather than something a consumer
/// infers, and [`Reclaimed::settlement`] is the same value for all three
/// discovery cells.
///
/// **What PR7 owns and this does not**: emitting the settlement *event*. PR6
/// has `durable_events: none` and the container transition is "test-only until
/// PR7 wires `TopologyRun`". What this slice owes is the value PR7 maps, stated
/// where the census produces it instead of left for a later reader to derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnerSettlement {
    /// The owning attempt, verification or probe husk is settled
    /// **interrupted**, with **unknown spend**.
    InterruptedWithUnknownSpend,
}

impl OwnerSettlement {
    /// Every settlement a reclaim can produce. One, deliberately: see the type.
    pub const ALL: &'static [Self] = &[Self::InterruptedWithUnknownSpend];

    /// As a report writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::InterruptedWithUnknownSpend => "interrupted-unknown-spend",
        }
    }

    /// Whether the owning identity's spend is known. Never.
    #[must_use]
    pub const fn spend_is_known(self) -> bool {
        match self {
            Self::InterruptedWithUnknownSpend => false,
        }
    }
}

/// One container this census reclaimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reclaimed {
    pub name: ContainerName,
    pub run_id: String,
    pub incarnation: String,
    pub ownership: Ownership,
    pub discovered_by: DiscoveredBy,
    /// "the census report names each reclaimed container's boundary from its
    /// `runner_policy_sha256`".
    pub boundary: Boundary,
    /// "then settle the owning identity interrupted" — with "unknown spend".
    /// See [`OwnerSettlement`] for why this does not depend on
    /// [`Self::discovered_by`].
    pub settlement: OwnerSettlement,
}

/// One container this census deliberately left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untouched {
    pub name: ContainerName,
    pub run_id: String,
    pub incarnation: String,
    pub ownership: Ownership,
    pub discovered_by: DiscoveredBy,
}

/// Whether the container runtime was consulted, and what it said.
///
/// "the container runtime is required **only** when an intent exists or a
/// labeled container is discoverable: if any intent exists and the runtime
/// cannot be reached the write command **refuses** …, and with no intent and no
/// reachable runtime it **proceeds**."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeUse {
    /// `docker ps` answered.
    Consulted,
    /// The runtime could not be reached and no intent existed, so the census
    /// proceeded without it. This is the ordinary state of every machine
    /// without a container runtime, which today is every machine.
    NotRequired,
}

/// What became of one `<name>.intent.tmp` whose published half never landed.
///
/// `PR6-ACCT-007`. The staged file is **R26** — it is the intent record, one
/// `rename` short of published — so it needs a disposition in every census that
/// sees it, not merely to be skipped by discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StagedDisposition {
    /// The staged bytes were a complete record, so the file was classified
    /// under the ordinary owner-liveness rule and appears in
    /// [`CensusReport::reclaimed`] or [`CensusReport::untouched`] like any other
    /// candidate. Its run directory came from the record it carries.
    Adopted,
    /// Genuinely torn, and the **name** says it belongs to a dead incarnation
    /// of the run this process is driving (arm (i), dead by construction). The
    /// file is removed.
    Removed,
    /// Genuinely torn, and the name says it belongs to **another run**. Arm
    /// (ii) probes that run's `run.lock`, and a torn file carries no run
    /// directory to probe — so this census cannot establish that its owner is
    /// dead and leaves it alone. That owner's own next write-command start
    /// classifies it under arm (i) and removes it; until then it is reported
    /// here rather than being silent residue.
    RetainedForeignOwner,
}

impl StagedDisposition {
    /// Every disposition, written out.
    pub const ALL: &'static [Self] = &[Self::Adopted, Self::Removed, Self::RetainedForeignOwner];

    /// As the report writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Adopted => "adopted",
            Self::Removed => "removed",
            Self::RetainedForeignOwner => "retained-foreign-owner",
        }
    }
}

/// One staged intent record this census accounted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedResidue {
    pub name: ContainerName,
    pub path: PathBuf,
    pub disposition: StagedDisposition,
}

/// What one census did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CensusReport {
    /// The root that was censused — the one it was **given**, never a default.
    pub private_root: PathBuf,
    pub command: WriteCommand,
    pub incarnation: String,
    pub runtime_use: RuntimeUse,
    /// Who closes the window between a coordinator's death and reclaim on this
    /// platform. Named rather than inferred, so a Windows report says which
    /// window it is closing.
    pub orphan_window: OrphanWindow,
    /// Sorted by container name.
    pub reclaimed: Vec<Reclaimed>,
    /// Sorted by container name.
    pub untouched: Vec<Untouched>,
    /// Every `<name>.intent.tmp` with no published half, and what became of it.
    /// Sorted by container name.
    pub staged: Vec<StagedResidue>,
}

impl CensusReport {
    /// The boundary this census reported for a reclaimed container.
    #[must_use]
    pub fn boundary_of(&self, name: &ContainerName) -> Option<&Boundary> {
        self.reclaimed
            .iter()
            .find(|entry| &entry.name == name)
            .map(|entry| &entry.boundary)
    }

    /// Whether a container was left alone.
    #[must_use]
    pub fn was_untouched(&self, name: &ContainerName) -> bool {
        self.untouched.iter().any(|entry| &entry.name == name)
    }
}

/// **The census completed.** Nothing that must follow it can be reached without
/// one of these.
///
/// Constructed only by [`run_startup_census`], and only on the path that
/// finished every reclaim: a census that refused returns `Err` and no token, so
/// "a dead owner's or dead incarnation's labeled container that cannot be
/// observed terminated **blocks admission**" is held by the type rather than by
/// a caller remembering to check.
///
/// The four things it precedes, from `crash_reconstruction`: slot/reservation
/// initialization, admission, an invocation's first use of an agent's
/// credential volume, and this incarnation's own probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CensusComplete {
    report: CensusReport,
}

impl CensusComplete {
    /// What the census found and did.
    #[must_use]
    pub const fn report(&self) -> &CensusReport {
        &self.report
    }

    /// The root the census actually scanned.
    ///
    /// A consumer that operates on a different root than the census scanned is
    /// operating on an uncensused root, and this is what lets it say so.
    #[must_use]
    pub fn private_root(&self) -> &Path {
        &self.report.private_root
    }
}

// ---------------------------------------------------------------------------
// The census
// ---------------------------------------------------------------------------

/// Everything one census needs.
///
/// `private_root` is a **parameter and never a default**: "a schema-4 resume
/// [censuses] the canonical root such that `run_started.private_dir`
/// canonicalizes to `R/runs/<run_id>`", and "a resume always censuses its
/// recorded root" even when the default root or `HOME` moved. Recovery step (a0)
/// computes it read-only before any lock; this module is handed the answer.
pub struct Census<'a> {
    pub private_root: &'a Path,
    pub start: &'a CensusStart,
    pub runtime: &'a dyn ContainerRuntime,
    pub liveness: &'a dyn OwnerLiveness,
    pub view: &'a dyn GitView,
}

/// Step (a) of the startup census: global container reclaim.
///
/// The sequence, and every step of it is separately droppable:
///
/// 1. scan `<R>/containers` — no runtime needed, and an absent directory is an
///    empty namespace;
/// 2. decide whether the runtime is required, and refuse or proceed;
/// 3. `docker ps` by `upstroke.private_root`, and merge the two halves;
/// 4. classify **every** candidate, and refuse **before any effect** if any
///    classification refuses or cannot be made;
/// 5. reclaim every dead candidate through [`super::reclaim`], in name order;
/// 6. return the token.
///
/// Step 4 completes before step 5 begins on purpose. Every other refusal in this
/// slice's contract is "before any effect", and a census that killed three
/// containers and then refused on the fourth would have performed effects on
/// behalf of a write command that never ran.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] when the runtime is required and cannot be reached,
/// when an intent names this process's own incarnation, when a labeled
/// container's ownership cannot be established, or when a dead container cannot
/// be observed terminated. [`UpstrokeError::Io`] from the namespace scan.
pub fn run_startup_census(
    hooks: &mut dyn ContainerHooks,
    census: &Census<'_>,
) -> Result<CensusComplete, UpstrokeError> {
    let private_root = census.private_root;
    let intents = list_intents(private_root)?;
    // `PR6-ACCT-007`: the staged half of the namespace, read in the same scan
    // and before any effect, because a torn one that names this process's own
    // incarnation refuses for the same reason a published one does and a
    // refusal must precede every reclaim.
    let staged = super::list_staged_intents(private_root)?;
    let (discovered, runtime_use) = discover_by_label(census.runtime, private_root, &intents)?;
    let mut candidates = merge(private_root, intents, discovered)?;
    let mut staged_residue = Vec::new();
    for entry in staged {
        match entry.record {
            // A finished write whose rename did not land. The record carries
            // the owner's run directory, so this is an ordinary candidate under
            // the ordinary rule — arm (ii) included.
            Some(record) => {
                let found = FoundIntent {
                    name: entry.name.clone(),
                    path: entry.path.clone(),
                    record,
                };
                candidates.push(candidate_from_intent(found)?);
                staged_residue.push(StagedResidue {
                    name: entry.name,
                    path: entry.path,
                    disposition: StagedDisposition::Adopted,
                });
            }
            // Genuinely torn. The ownership evidence is the name.
            None => staged_residue.push(StagedResidue {
                name: entry.name,
                path: entry.path,
                disposition: StagedDisposition::RetainedForeignOwner,
            }),
        }
    }
    candidates.sort_by(|left, right| left.name.cmp(&right.name));

    // Step 4: classify everything, and refuse before any effect.
    let mut decided = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let ownership = classify_ownership(
            census.start,
            &candidate.run_id,
            &candidate.incarnation,
            &candidate.run_dir,
            census.liveness,
        );
        if ownership.refuses() {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the container intent `{}` names run `{}` and incarnation `{}`, which is this \
                     process's own run and its own incarnation; an intent naming this process's \
                     own incarnation cannot exist at census time — the census precedes every \
                     invocation including this incarnation's probes — and is refused if observed \
                     (decisions.pr_sequence[7].slice_contract.expected_failures_refusals[7]). \
                     Nothing was reclaimed and nothing was probed on its behalf",
                    candidate.name, candidate.run_id, candidate.incarnation
                ),
            });
        }
        decided.push((candidate, ownership));
    }

    // Step 5: reclaim, in name order.
    decided.sort_by(|left, right| left.0.name.cmp(&right.0.name));
    let mut reclaimed = Vec::new();
    let mut untouched = Vec::new();
    for (candidate, ownership) in decided {
        if !ownership.reclaims() {
            untouched.push(Untouched {
                name: candidate.name,
                run_id: candidate.run_id,
                incarnation: candidate.incarnation,
                ownership,
                discovered_by: candidate.discovered_by,
            });
            continue;
        }
        let view = view_path(private_root, &candidate.name);
        super::reclaim(
            hooks,
            census.runtime,
            census.view,
            private_root,
            &candidate.name,
            Some(&view),
        )?;
        reclaimed.push(Reclaimed {
            name: candidate.name,
            run_id: candidate.run_id,
            incarnation: candidate.incarnation,
            ownership,
            discovered_by: candidate.discovered_by,
            boundary: candidate.boundary,
            // The same value for every cell of {intent present} x {container
            // present}, deliberately: see `OwnerSettlement`.
            settlement: OwnerSettlement::InterruptedWithUnknownSpend,
        });
    }

    // Step 5b: the torn staging files, after the reclaims that may have removed
    // some of them. Arm (i) only — a torn record carries no run directory, so
    // arm (ii)'s lock probe has nothing to ask about and its owner reclaims its
    // own at its next write-command start (`PR6-ACCT-007`).
    for residue in &mut staged_residue {
        if residue.disposition != StagedDisposition::RetainedForeignOwner {
            continue;
        }
        let parts = ContainerName::parse(residue.name.as_str())?;
        if census.start.own_run() != Some(parts.run_id.as_str())
            || parts.incarnation == census.start.incarnation()
        {
            // A foreign run's torn file, or this incarnation's own — and this
            // incarnation has launched nothing, so its own staged file is
            // residue of a *previous* process that happened to share the id,
            // which no census may adopt. Both are left alone and reported.
            continue;
        }
        super::remove_staged_intent(
            hooks,
            ContainerSite::RemoveIntent,
            private_root,
            &residue.name,
        )?;
        residue.disposition = StagedDisposition::Removed;
    }
    staged_residue.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(CensusComplete {
        report: CensusReport {
            private_root: private_root.to_path_buf(),
            command: census.start.command(),
            incarnation: census.start.incarnation().to_owned(),
            runtime_use,
            orphan_window: orphan_window(),
            reclaimed,
            untouched,
            staged: staged_residue,
        },
    })
}

/// The `docker ps` half of discovery, and the runtime-required rule.
///
/// The reachability question is asked of [`RuntimeOp::ListByLabel`] — the
/// operation actually needed — and not of [`ContainerRuntime::probe`], whose
/// `Ok` binds nothing about a later call. A runtime that answers `probe` and
/// fails `ps` would otherwise classify reachable, the write command would
/// proceed past the refusal point, and the failure would land after "before any
/// recovery event".
///
/// The decision table, which is the packet's sentence split into its cells:
///
/// | intents | `ListByLabel` | outcome |
/// |---|---|---|
/// | none | `Ok` | proceed, with whatever it found |
/// | none | `Unreachable` | **proceed** — "with no intent and no reachable runtime it proceeds" |
/// | none | `Failed` | **refuse** — the runtime answered and would not say; nothing proves there is no labeled orphan |
/// | some | `Ok` | proceed |
/// | some | `Unreachable` | **refuse** — "it cannot prove those containers terminated" |
/// | some | `Failed` | **refuse**, same reason |
fn discover_by_label(
    runtime: &dyn ContainerRuntime,
    private_root: &Path,
    intents: &[FoundIntent],
) -> Result<(Vec<super::runtime::DiscoveredContainer>, RuntimeUse), UpstrokeError> {
    let label = private_root_label(private_root);
    let error = match runtime.containers_with_label(LABEL_PRIVATE_ROOT, &label) {
        Ok(found) => return Ok((found, RuntimeUse::Consulted)),
        Err(error) => error,
    };
    if !proceeds_without(&error) {
        return Err(UpstrokeError::Refused {
            message: format!(
                "the container runtime was reached and refused `{}` under `{}`: {error}. \
                 A runtime that answers and will not list cannot prove that no labeled \
                 container of a dead owner is still running, so this write command refuses \
                 rather than admitting over one",
                RuntimeOp::ListByLabel,
                private_root.display(),
            ),
        });
    }
    if !intents.is_empty() {
        return Err(UpstrokeError::Refused {
            message: format!(
                "{} container intent(s) exist under `{}` and the container runtime cannot be \
                 reached for `{}`: {error}. The runtime is required only when an intent exists \
                 or a labeled container is discoverable, and this write command cannot prove \
                 those containers terminated, so it refuses",
                intents.len(),
                private_root.display(),
                RuntimeOp::ListByLabel,
            ),
        });
    }
    Ok((Vec::new(), RuntimeUse::NotRequired))
}

/// The union of the two halves of discovery, keyed by container name.
///
/// Every candidate's ownership fields come from **one** source and the other is
/// checked against it: a container whose record and whose labels disagree about
/// its owner is not something this census may pick a winner for.
///
/// The container **name** is ownership evidence too — it is
/// `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>` — so its
/// components are checked against the record's fields. A name and a record that
/// disagree about the incarnation would mean classifying on one value and
/// killing a container named for another.
fn merge(
    private_root: &Path,
    intents: Vec<FoundIntent>,
    discovered: Vec<super::runtime::DiscoveredContainer>,
) -> Result<Vec<Candidate>, UpstrokeError> {
    let mut by_name: BTreeMap<String, Candidate> = BTreeMap::new();
    for found in intents {
        by_name.insert(
            found.name.as_str().to_owned(),
            candidate_from_intent(found)?,
        );
    }
    for container in discovered {
        if let Some(existing) = by_name.get_mut(&container.name) {
            check_labels_against_record(existing, &container)?;
            existing.discovered_by = DiscoveredBy::IntentAndLabel;
            continue;
        }
        let candidate = from_labels_alone(private_root, &container)?;
        by_name.insert(container.name.clone(), candidate);
    }
    Ok(by_name.into_values().collect())
}

/// One record — published or staged-but-complete — as a candidate.
///
/// Factored out of [`merge`] so the staged half of the namespace is classified
/// by the **same** derivation and not by a second copy of it: the record's run
/// directory is decoded and checked rooted here, and the name is checked against
/// the record's fields, whichever half of the namespace the record came from
/// (`PR6-ACCT-007`).
fn candidate_from_intent(found: FoundIntent) -> Result<Candidate, UpstrokeError> {
    check_name_against_record(&found)?;
    // Decoded and checked rooted here, not turned into a `PathBuf` by
    // assumption: this is the directory arm (ii) probes a `run.lock` in,
    // and every wrong answer to that probe is "free", which reclaims.
    let run_dir = found.record.run_dir_path()?;
    Ok(Candidate {
        name: found.name,
        run_id: found.record.run_id,
        incarnation: found.record.incarnation,
        run_dir,
        boundary: Boundary::FromIntent(found.record.runner_policy_sha256),
        discovered_by: DiscoveredBy::IntentOnly,
        intent_path: Some(found.path),
    })
}

/// A labeled container with no record — "treated as an orphan of its **labeled**
/// run and incarnation under the same rule".
fn from_labels_alone(
    private_root: &Path,
    container: &super::runtime::DiscoveredContainer,
) -> Result<Candidate, UpstrokeError> {
    let name = ContainerName::rebuild(&container.name).map_err(|error| UpstrokeError::Refused {
        message: format!(
            "the container `{}` carries `{LABEL_PRIVATE_ROOT}={}` and its name is not a upstroke \
             container name ({error}); a container claiming this private root that no funnel \
             could have named cannot be reclaimed through the funnel or observed terminated, \
             and an unreclaimable labeled container blocks admission",
            container.name,
            private_root.display(),
        ),
    })?;
    let mut fields = Vec::new();
    for key in [LABEL_RUN, LABEL_INCARNATION, LABEL_RUN_DIR] {
        let Some(value) = container.label(key) else {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the container `{}` carries `{LABEL_PRIVATE_ROOT}` and not `{key}`, so its \
                     labeled run and incarnation cannot be established; a labeled container \
                     without an intent is classified from its labels alone, and one whose \
                     labels do not say who owns it cannot be observed terminated under the \
                     liveness rule, which blocks admission",
                    container.name
                ),
            });
        };
        fields.push(value.to_owned());
    }
    // `PR6-CORRECTNESS-016`: a *present* `upstroke.run_dir` still has to say
    // where its owner's lock is. The missing-key arm above and this one are
    // separate predicates — the shipped code held only the first, so a label
    // set that varied which key was absent passed while `upstroke.run_dir=`
    // reached the probe as `./run.lock`.
    let run_dir = super::intent::owner_run_dir(&fields[2], "container's labels")?;
    let candidate = Candidate {
        name,
        run_id: fields[0].clone(),
        incarnation: fields[1].clone(),
        run_dir,
        boundary: Boundary::NoIntentRecord,
        discovered_by: DiscoveredBy::LabelOnly,
        intent_path: None,
    };
    check_name_against(
        &candidate.name,
        &candidate.run_id,
        &candidate.incarnation,
        "labels",
    )?;
    Ok(candidate)
}

/// The record's own name must be the name its fields build.
fn check_name_against_record(found: &FoundIntent) -> Result<(), UpstrokeError> {
    check_name_against(
        &found.name,
        &found.record.run_id,
        &found.record.incarnation,
        "intent record",
    )?;
    let parts = ContainerName::parse(found.name.as_str())?;
    if parts.repo_key != found.record.repo_key {
        return Err(UpstrokeError::Refused {
            message: format!(
                "the container intent `{}` is named for repo key `{}` and its record says `{}`; \
                 the name is `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>` and a \
                 record that disagrees with its own name is not ownership evidence this census \
                 may act on",
                found.name, parts.repo_key, found.record.repo_key
            ),
        });
    }
    Ok(())
}

/// The two ownership components the classification is made on.
fn check_name_against(
    name: &ContainerName,
    run_id: &str,
    incarnation: &str,
    source: &str,
) -> Result<(), UpstrokeError> {
    let parts = ContainerName::parse(name.as_str())?;
    if parts.run_id != run_id {
        return Err(UpstrokeError::Refused {
            message: format!(
                "the container `{name}` is named for run `{}` and its {source} says `{run_id}`; \
                 the liveness rule classifies on the owner run, and a name that disagrees would \
                 mean classifying one run and reclaiming a container named for another",
                parts.run_id
            ),
        });
    }
    if parts.incarnation != incarnation {
        return Err(UpstrokeError::Refused {
            message: format!(
                "the container `{name}` is named for incarnation `{}` and its {source} says \
                 `{incarnation}`; the incarnation is the component that keeps deterministic \
                 invocation ids from colliding across incarnations, and a name that disagrees \
                 with its own ownership evidence overwrites what the census needs",
                parts.incarnation
            ),
        });
    }
    Ok(())
}

/// A container found by both halves must have both halves saying one thing.
fn check_labels_against_record(
    candidate: &Candidate,
    container: &super::runtime::DiscoveredContainer,
) -> Result<(), UpstrokeError> {
    for (key, recorded) in [
        (LABEL_RUN, candidate.run_id.as_str()),
        (LABEL_INCARNATION, candidate.incarnation.as_str()),
    ] {
        let Some(labeled) = container.label(key) else {
            continue;
        };
        if labeled != recorded {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the container `{}` carries `{key}={labeled}` and its intent record says \
                     `{recorded}`; the labels are derived from the record when a container is \
                     created, so a disagreement is not a state this engine wrote and the census \
                     will not choose which of the two owns it",
                    container.name
                ),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The Unix reaper's half of the orphan window (ST-16 (d))
// ---------------------------------------------------------------------------

/// `docker`'s argument for "carrying this label".
const LABEL_FILTER: &str = "label=";

/// What this process's cleanup reapers kill when the coordinator dies.
///
/// `decisions.admission_and_leases.permits.os_matrix`: "Linux and macOS
/// (`cfg(unix)`): the cleanup reaper survives coordinator death, settles the
/// dead coordinator's process groups while holding R28, and **additionally
/// kills the dead coordinator's labeled containers**, closing the orphan
/// window".
///
/// **The selector is the incarnation, not the private root.** `upstroke.private_
/// root` alone names every container of every run under `<R>`, including a
/// **live** coordinator's — and "a live incarnation's containers must not be
/// touched" (`T-CONTAINER.authoritative_state`). The incarnation is a
/// per-process ULID, so it names this coordinator and nothing else; the private
/// root is kept beside it because "different private roots are disjoint worlds"
/// should be true of the reaper as well as of the census, and because two
/// filters cost nothing.
///
/// This type is a **value**, deliberately: the reaper is a `fork`-only child in
/// a multithreaded process and may call nothing that allocates, so every string
/// it will ever need is rendered here, on the parent side, before the fork.
/// [`crate::agent::proc::set_container_reclaim_scope`] is where it is handed
/// over.
///
/// **What a later slice must connect.** Nothing registers a scope in this
/// slice: `production_effect` is "none" and no run selects a container Runner
/// until PR12. PR7's `TopologyRun` registers it once run identity exists — the
/// private root from `run_started.private_dir` and the incarnation from
/// `run_started(4)`/`run_resumed(4)` — and must ensure a supervisor is live
/// across a container invocation, or the window is closed only by the next
/// write command's census.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaperContainerScope {
    program: PathBuf,
    private_root: String,
    incarnation: String,
}

impl ReaperContainerScope {
    /// Build the scope, refusing a label value `docker` could not carry.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when the incarnation is empty or when either
    /// label value carries a byte that would end the argument or start another
    /// filter — a newline, a comma, or an `=`. The reaper cannot report a
    /// malformed selector: it has no error channel and no allocator, so the
    /// check is here.
    pub fn new(
        program: impl Into<PathBuf>,
        private_root: &Path,
        incarnation: &str,
    ) -> Result<Self, UpstrokeError> {
        let root = private_root_label(private_root);
        for (what, value) in [
            ("private root", root.as_str()),
            ("incarnation", incarnation),
        ] {
            if value.is_empty() {
                return Err(UpstrokeError::Refused {
                    message: format!(
                        "the Unix reaper's container scope has an empty {what}; a filter that \
                         matches everything would kill a live coordinator's containers"
                    ),
                });
            }
            if let Some(bad) = value.chars().find(|c| matches!(c, '\n' | '\r' | ',' | '=')) {
                return Err(UpstrokeError::Refused {
                    message: format!(
                        "the Unix reaper's container scope {what} carries `{}`, which would \
                         change what `{LABEL_FILTER}` selects",
                        bad.escape_default()
                    ),
                });
            }
        }
        Ok(Self {
            program: program.into(),
            private_root: root,
            incarnation: incarnation.to_owned(),
        })
    }

    /// The `docker` binary the reaper execs.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// `docker ps --all --quiet --no-trunc --filter label=… --filter label=…`,
    /// including `argv[0]`.
    ///
    /// `--all`, because a container that exited still holds its name, its
    /// labels and its writable layer until it is removed. `--no-trunc`, so the
    /// ids the reaper then kills are unambiguous.
    #[must_use]
    pub fn list_argv(&self) -> Vec<String> {
        vec![
            self.program.to_string_lossy().into_owned(),
            "ps".to_owned(),
            "--all".to_owned(),
            "--quiet".to_owned(),
            "--no-trunc".to_owned(),
            "--filter".to_owned(),
            format!("{LABEL_FILTER}{LABEL_PRIVATE_ROOT}={}", self.private_root),
            "--filter".to_owned(),
            format!("{LABEL_FILTER}{LABEL_INCARNATION}={}", self.incarnation),
        ]
    }

    /// `docker kill <id>`, including `argv[0]`.
    #[must_use]
    pub fn kill_argv(&self, id: &str) -> Vec<String> {
        vec![
            self.program.to_string_lossy().into_owned(),
            "kill".to_owned(),
            id.to_owned(),
        ]
    }

    /// `docker rm --force --volumes <id>`, including `argv[0]`.
    ///
    /// The reaper does **kill/rm** and nothing else: `T-CONTAINER.resume_action`
    /// is "on Unix the cleanup reaper performs **kill/rm** earlier when the
    /// coordinator dies". The Git view and the intent record are removed by the
    /// next write command's census, which is why every step of
    /// [`super::reclaim`] is idempotent and tolerant of already-gone — the
    /// ordinary post-reaper state is an intent whose container is already gone,
    /// which is [`DiscoveredBy::IntentOnly`].
    ///
    /// **`--volumes`, the same removal `DockerCli::remove` issues**
    /// (`PR6-ACCT-006`). The anonymous volume an image's `VOLUME` declaration
    /// creates per container is R26 — part of the container, referable by
    /// nothing else — and the reaper is the last thing that can name it: once
    /// the container is gone the next census sees `DiscoveredBy::IntentOnly`
    /// and has no handle on the volume at all. Measured on docker 29.7.2:
    /// `rm --force --volumes` removes the container's anonymous volumes and
    /// leaves a mounted **named** one intact, so this discharges R26 without
    /// touching R20.
    ///
    /// `proc::tests::the_unix_reaper_kills_labeled_containers_before_releasing_r28`
    /// asserts the argv the forked reaper **actually executed** against this
    /// function, so the fork-side `c"…"` literals — which nothing can read back
    /// at runtime — cannot drift from it.
    #[must_use]
    pub fn remove_argv(&self, id: &str) -> Vec<String> {
        vec![
            self.program.to_string_lossy().into_owned(),
            "rm".to_owned(),
            "--force".to_owned(),
            "--volumes".to_owned(),
            id.to_owned(),
        ]
    }
}

/// Whether a runtime error is the shape that lets a census proceed.
///
/// Kept as a named function rather than a `matches!` at the call site so the
/// distinction between "could not be reached" and "answered and failed" is one
/// thing with one reason, and so the two branches of the decision table above
/// cannot drift apart.
#[must_use]
pub const fn proceeds_without(error: &RuntimeError) -> bool {
    error.is_unreachable()
}

#[cfg(test)]
mod tests;
