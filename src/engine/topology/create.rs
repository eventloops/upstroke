//! P0–P8 — creating a schema-4 run, and the one deletion boundary.
//!
//! `decisions.workspace_candidates.run_creation`, verbatim on the order:
//!
//! > under the worktree lock a fresh run then proceeds in this order, **each
//! > step a registered site**: P0 create the public run directory
//! > (`RunDir.CreatePublicDir`); P1 publish the `.creating` marker atomically …
//! > P2 acquire `run.lock` (R17; the lock file is R21); P3 create the private
//! > half at the recorded locator … then publish the reciprocal owner record …
//! > **before any other private content** …, then the private skeleton
//! > directories; P4 run preflight through the resolved Runner as the
//! > `RunnerPreflight` — one non-slotted shell probe … and one slotted probe per
//! > recorded agent …; P5 write `plan.normalized.json` and open `events.jsonl`
//! > …; P5b publish the private commit record …; P6 append `run_started(4)` …
//! > durably …; P7 remove the marker …; P8 create the integration ref zero-old
//!
//! That is O04–O16, and this module is those clauses and nothing else. It
//! resolves nothing (that is [`super::prelock`]), censuses nothing, and runs no
//! loop.
//!
//! # The typestate
//!
//! Each prefix is a type in [`steps`]. Every one has private fields, derives no
//! `Clone`, `Copy` or `Default`, and has exactly one constructor, which takes
//! its predecessor **by value**. So P5b cannot be reached without P4 having
//! returned, and a witness cannot be duplicated to authorise a step twice. The
//! root of the chain is [`PreLockChecked`], which is itself unforgeable, so the
//! run id, the incarnation and the `RunnerPolicy` this sequence publishes are —
//! as a matter of type, not of review — the ones resolved before the worktree
//! lock.
//!
//! # The deletion boundary
//!
//! There is exactly one, it is the existence of `<private>/committed.json`, and
//! [`stat_after_error`] is the only place this module can delete anything. It
//! stats fail-closed, proves, and spends the proof token **in one call**, so no
//! proof value and no stat result ever crosses P5b in a local variable.
//!
//! Three consequences, each settled and each a separate test:
//!
//! * **At P3a the creator removes neither half.** ST-19: "P3a: the private
//!   directory exists without an owner record — unprovable — so **both halves
//!   are retained and reported** (content-free by ordering; deferred prune)".
//!   This needs no special case: `prove_private_half_ownership` already answers
//!   `Retained(OwnerRecordMissing)` for exactly that shape, and a `Retained`
//!   answer deletes nothing. Removing the public half instead — which an earlier
//!   draft did — would orphan the private one **permanently**, because the only
//!   production `read_dir` over a runs root is `rundir::run_dir_names` over
//!   `<repo>/.upstroke/runs` and the private half is reachable only through the
//!   marker inside the public husk.
//! * **The public half goes only if the private one went.** The same argument
//!   one step later: on the proven path the removal is ordered private-half
//!   first and public directory last, and `remove_public_husk` deletes
//!   `<public>/.creating` with it. So a `RunDir.RemovePrivateHusk` that returns
//!   `Err` short-circuits — removing the public half anyway would delete the
//!   private half's only locator on exactly the path where the private half may
//!   still be there. Retained, the pair is reachable in **all three** shapes a
//!   failed `remove_dir_all` can leave, which is what the short-circuit buys:
//!   the next census proves the pair and reclaims both if the private half
//!   survived intact; reclaims the public husk alone (`TargetAbsent`) if the
//!   whole private half went and the error came on the way out; and — the shape
//!   the arm exists for, an unwritable parent or a Windows handle on the
//!   directory itself — **retains the pair, reported**, when every child was
//!   removed and the directory was not, because the marker's target then exists
//!   with no `owner.json` in it and the proof answers `OwnerRecordMissing`. Two
//!   of the three converge by reclaiming and the third by reporting; none of
//!   them orphans anything, which is the property the short-circuit is for.
//! * **From the moment `committed.json` exists the creator deletes nothing**,
//!   including when `publish_commit_record` returned `Err` and a read-only stat
//!   shows the record present, and including when the stat cannot answer.
//! * **The creator uses the existing `PrivateHalfProof` constructor.** Every
//!   conjunct of `prove_private_half_ownership` passes for the creator at
//!   P3b–P5 — it published both records itself, one incarnation ago — so a
//!   second constructor would buy nothing and would cost the field privacy six
//!   compile-fail fixtures rest on.
//!
//! # `RunPaths::create_hooked` is not used here
//!
//! It creates the five private skeleton directories **before** the owner
//! record. O08 is "owner record before any other private content and before any
//! probe", so using it would put five directories on the wrong side of the
//! record and make the P3a residue five directories rather than an empty one.
//! Its only production caller is the legacy coordinator.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::agent::AdapterSource;
use crate::error::UpstrokeError;
use crate::events::log::{
    BarrierStep, EventLog, TopologyLine, establish_stable_prefix, first_line_digest,
};
use crate::gates::ShellKind;
use crate::rundir::{
    CommitRecord, CommitRecordPresence, CreatingMarker, MARKER_STAGED, OWNER_RECORD_STAGED,
    OwnerRecord, PrivateHalfOwnership, RepoKey, RetainReason, RunLock, RunPaths, UnboundShape,
    commit_record_after_error, create_private_dir, create_private_skeleton, create_public_dir,
    prove_private_half_ownership, publish_commit_record, publish_marker, publish_owner_record,
    remove_marker, remove_private_husk, remove_public_husk, stage_commit_record, stage_marker,
    stage_owner_record, write_plan,
};
use crate::runner::{InvocationId, Runner};
use crate::topology::effects::EventSite;
use crate::topology::events::{RunStarted4, TopologyEvent, TopologyEventBody};
use crate::topology::fold::{FrozenInputs, TopologyDelta, TopologyFold};

use super::identity::{InvocationLedger, PreflightIdentities, SlotAssertion};
use super::prelock::PreLockChecked;
use super::seams::{TimeSource, TopologyHooks};

pub use steps::Started;

// ---------------------------------------------------------------------------
// The two seams P4 and P8 need
// ---------------------------------------------------------------------------

/// P4's `RunnerPreflight`, as this module drives it.
///
/// INV-23: "one non-slotted shell probe (the recorded shell executing `exit 0`)
/// and one slotted probe per recorded agent, each a registered invocation
/// **through the run's Runner**". The asymmetry is why these are two methods —
/// the shell probe takes the identity this module minted and no slot, an agent
/// probe takes a slot and is driven by its adapter, which mints the probe
/// identities of its own pre-flight processes.
///
/// A seam rather than a concrete `&dyn Runner` plus a `&dyn AdapterSource`
/// because the *ordering* is what this module owns and the ordering is only
/// observable if a test can see which probe ran first. [`RunnerProbes`] is the
/// production implementation and is a thin composition of two existing
/// functions.
pub trait Probes {
    /// `runner_policy_sha256` of the policy the probes execute under.
    ///
    /// P4 refuses when this is not the digest the pre-lock checks resolved and
    /// P1/P3b published. INV-23 requires the digest "carried by every container
    /// intent" to be the run's, and a probe running under some other policy
    /// would write intents naming a boundary the owner record does not
    /// describe — the exact disagreement the census reports a husk for.
    fn policy_digest(&self) -> &str;

    /// The shell probe: the recorded shell executing `exit 0`, non-slotted.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when the shell cannot be run, times out, or
    /// does not exit 0.
    fn shell(&self, invocation: InvocationId) -> Result<(), UpstrokeError>;

    /// One recorded agent's probe, slotted.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError`] when the adapter is not registered or its CLI does not
    /// answer.
    fn agent(&self, agent: &str) -> Result<(), UpstrokeError>;

    /// R3's ledger for the processes **these** probes ran.
    ///
    /// **On the seam so that a second pair is unrepresentable.** `Request` used
    /// to carry its own ledger and slots beside a `&dyn Probes`, with nothing
    /// requiring the two to be the same: construct the probes over locks A and
    /// the request over empty locks B, and P4 runs through A while creation's
    /// closing assertion reads B, finds it vacuously balanced, and reports no
    /// leaked registration. The round-4 review of `09f9a99` set out that
    /// construction. The type system now refuses it — one owner, and the caller
    /// cannot supply a second.
    fn ledger(&self) -> &Mutex<InvocationLedger>;

    /// R4's slots, from the same owner and for the same reason.
    fn slots(&self) -> &Mutex<SlotAssertion>;
}

/// The production [`Probes`]: both probes through the run's own `Runner`.
pub struct RunnerProbes<'a> {
    /// The run's runner — host or container, whichever P0's policy resolved to.
    pub runner: &'a dyn Runner,
    /// The recorded shell.
    pub shell: ShellKind,
    /// Where the shell probe runs. A probe has no workspace of its own; this is
    /// [`crate::agent::probe_workspace`]'s answer at the host boundary and is
    /// ignored at the container one, which gives a probe no worktree at all.
    pub workspace: PathBuf,
    /// Where an agent id resolves to its adapter.
    pub adapters: &'a dyn AdapterSource,
    /// The digest of the policy `runner` executes under.
    pub policy_digest: String,
    /// R3's ledger, and R4's slots, for the **registering** runner both probes
    /// execute through.
    ///
    /// Behind locks because `Runner::run` takes `&self`: the wrapper registers,
    /// slots, runs and settles inside one call, and the two ledgers it mutates
    /// are shared with the caller that reads them afterwards. Resume's
    /// pre-flight holds them the same way and for the same reason.
    pub ledger: &'a Mutex<InvocationLedger>,
    pub slots: &'a Mutex<SlotAssertion>,
}

impl RunnerProbes<'_> {
    /// The runner both probes actually execute through: the run's, with R3 and
    /// R4 wrapped around **every** request an adapter makes.
    fn registering(&self) -> super::preflight::Registering<'_> {
        super::preflight::Registering {
            inner: self.runner,
            ledger: self.ledger,
            slots: self.slots,
        }
    }
}

impl Probes for RunnerProbes<'_> {
    fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    fn shell(&self, invocation: InvocationId) -> Result<(), UpstrokeError> {
        crate::runner::host::run_shell_probe(
            &self.registering(),
            self.shell,
            self.workspace.clone(),
            invocation,
        )
    }

    fn agent(&self, agent: &str) -> Result<(), UpstrokeError> {
        let adapter = self
            .adapters
            .get(agent)
            .ok_or_else(|| UpstrokeError::Agent {
                message: format!("no adapter registered for agent `{agent}`"),
            })?;
        // **Through the registering runner, not the raw one.** Every process the
        // adapter builds is its own registered invocation with its own slot, so
        // a probe that fails at its fourth request is recorded at its fourth
        // request. Handing `self.runner` here is what made the creation ledger
        // account one logical probe instead of the processes it ran.
        adapter.probe(&self.registering()).map(|_caps| ())
    }

    fn ledger(&self) -> &Mutex<InvocationLedger> {
        self.ledger
    }

    fn slots(&self) -> &Mutex<SlotAssertion> {
        self.slots
    }
}

/// P8's ref funnel — as much of [`crate::workspace_manager::WorkspaceManager`]
/// as the creator needs.
///
/// A seam because P8 is the one prefix whose effect is a Git object-store
/// mutation: a test that could not stand in for it would need a real repository
/// and `git` subprocesses, and this module is a `TOPOLOGY_MODULE` in which
/// `std::process::Command` is a build error.
pub trait IntegrationRefs {
    /// `assert_publishable()` — a symbolic ref, or one checked out in some
    /// worktree, refuses.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] for either.
    fn assert_publishable(&self, refname: &str) -> Result<(), UpstrokeError>;

    /// The ref's current direct target, or `None` when nothing is there.
    ///
    /// # Errors
    ///
    /// A Git error, or a symbolic ref.
    fn direct_target(&self, refname: &str) -> Result<Option<String>, UpstrokeError>;

    /// `Ref.CreateIntegration`: zero-old, `--no-deref`.
    ///
    /// # Errors
    ///
    /// A Git error, including the zero-old failure when the ref appeared
    /// between [`Self::direct_target`] and this call.
    fn create_zero_old(
        &self,
        hooks: &mut dyn crate::workspace_manager::EffectHooks,
        refname: &str,
        new: &str,
    ) -> Result<(), UpstrokeError>;
}

impl IntegrationRefs for crate::workspace_manager::WorkspaceManager {
    fn assert_publishable(&self, refname: &str) -> Result<(), UpstrokeError> {
        Self::assert_publishable(self, refname)
    }

    fn direct_target(&self, refname: &str) -> Result<Option<String>, UpstrokeError> {
        Self::direct_ref_target(self, refname)
    }

    fn create_zero_old(
        &self,
        hooks: &mut dyn crate::workspace_manager::EffectHooks,
        refname: &str,
        new: &str,
    ) -> Result<(), UpstrokeError> {
        self.create_ref_zero_old(
            hooks,
            crate::topology::effects::RefSite::CreateIntegration,
            refname,
            new,
        )
    }
}

// ---------------------------------------------------------------------------
// The prefixes, as types
// ---------------------------------------------------------------------------

/// Which prefix the sequence reached.
///
/// T-RUNSTART's own names, so a report, a kill test and a ledger row can all
/// cite the same coordinate. `P1Staged` and `P3aStaged` are separate because the
/// census classifies them separately: a directory holding only `.creating.tmp`
/// is reclaimable, and a private half holding only `owner.json.tmp` is retained
/// **and is not content-free**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Prefix {
    /// Before anything: `RunDir.CreatePublicDir` itself failed.
    Nothing,
    /// P0: the public run directory, bare.
    P0,
    /// P1a: `.creating.tmp` staged.
    P1Staged,
    /// P1b: `.creating` published.
    P1,
    /// P2: `run.lock` held.
    P2,
    /// P3a: the private directory, no owner record.
    P3a,
    /// P3a with `owner.json.tmp` staged. Retained, and **not** content-free.
    P3aStaged,
    /// P3b: the owner record published, then the private skeleton.
    P3b,
    /// P4: the `RunnerPreflight` certified.
    P4,
    /// P5: the plan written and the log open, with no committed first line.
    P5,
    /// P5b: `committed.json` published. **The deletion boundary.**
    P5b,
    /// P6: `run_started(4)` durable.
    P6,
    /// P7: the marker removed.
    P7,
    /// P8: the integration ref created.
    P8,
}

impl Prefix {
    /// Every prefix, in order.
    pub const ALL: &'static [Self] = &[
        Self::Nothing,
        Self::P0,
        Self::P1Staged,
        Self::P1,
        Self::P2,
        Self::P3a,
        Self::P3aStaged,
        Self::P3b,
        Self::P4,
        Self::P5,
        Self::P5b,
        Self::P6,
        Self::P7,
        Self::P8,
    ];

    /// The prefix's name, as T-RUNSTART writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Nothing => "before P0",
            Self::P0 => "P0",
            Self::P1Staged => "P1a",
            Self::P1 => "P1",
            Self::P2 => "P2",
            Self::P3a => "P3a",
            Self::P3aStaged => "P3a (staged)",
            Self::P3b => "P3b",
            Self::P4 => "P4",
            Self::P5 => "P5",
            Self::P5b => "P5b",
            Self::P6 => "P6",
            Self::P7 => "P7",
            Self::P8 => "P8",
        }
    }

    /// Whether the commit record exists at this prefix **by ordering**.
    ///
    /// Never consulted in place of the stat — the stat is the authority, because
    /// a `PublishCommitRecord` error is the same value on both sides of the
    /// rename. This is the ordering claim beside it, so "the creator deletes
    /// nothing from P5b on" is checkable as a property of the sequence rather
    /// than of one code path.
    #[must_use]
    pub fn is_past_the_deletion_boundary(self) -> bool {
        self >= Self::P5b
    }
}

/// A creation that stopped **before** the commit record existed, with
/// everything cleanup needs and nothing it does not.
///
/// It carries no proof and no stat result: [`stat_after_error`] computes both,
/// consecutively, in one call.
///
/// Always handed about boxed. It is the cold arm of seven `Result`s whose `Ok`
/// arm is a witness the sequence carries forward, and `clippy::result_large_err`
/// is right that a 256-byte failure value should not widen every one of them.
#[derive(Debug)]
struct Aborted {
    reached: Prefix,
    paths: RunPaths,
    /// The run lock, when one was taken, so cleanup can release it **through
    /// the funnel** before removing the directory it lives in.
    lock: Option<RunLock>,
    repo_key: RepoKey,
    private_root: PathBuf,
    run_id: String,
    error: UpstrokeError,
}

/// The P0–P8 witnesses.
///
/// Field privacy is the enforcement: nothing outside this module can name a
/// field, so nothing outside it can build a witness, and every constructor takes
/// its predecessor by value so nothing can build one out of order or build two
/// from one. None derives `Clone`, `Copy` or `Default`.
///
/// Each also has an `abort`, which takes the witness **by value** and moves the
/// run lock into the [`Aborted`]. Taking it by value is what makes a witness
/// unusable after the step that failed, and moving the lock is what lets
/// cleanup release it through `Lock.Release` before it removes the directory
/// `run.lock` lives in — on Windows an open handle without `FILE_SHARE_DELETE`
/// refuses that unlink outright.
mod steps {
    use super::{
        Aborted, CommitRecord, EventLog, OwnerRecord, PreLockChecked, Prefix, RepoKey, RunLock,
        RunPaths, RunStarted4, TopologyEvent, TopologyFold, TopologyLine, UpstrokeError,
    };

    /// What every prefix from P0 on carries unchanged.
    #[derive(Debug)]
    pub struct Facts {
        checked: PreLockChecked,
        paths: RunPaths,
        repo_key: RepoKey,
    }

    impl Facts {
        pub(super) fn checked(&self) -> &PreLockChecked {
            &self.checked
        }

        pub(super) fn paths(&self) -> &RunPaths {
            &self.paths
        }

        pub(super) fn repo_key(&self) -> &RepoKey {
            &self.repo_key
        }

        fn abort(
            self,
            reached: Prefix,
            lock: Option<RunLock>,
            error: UpstrokeError,
        ) -> Box<Aborted> {
            Box::new(Aborted {
                reached,
                run_id: self.checked.run_id().to_owned(),
                private_root: self.checked.private_root().to_path_buf(),
                paths: self.paths,
                lock,
                repo_key: self.repo_key,
                error,
            })
        }
    }

    /// P0 — the public run directory exists, **bare**.
    #[derive(Debug)]
    pub struct PublicDirCreated {
        facts: Facts,
    }

    impl PublicDirCreated {
        /// The one place a [`Facts`] comes into existence, and it consumes the
        /// pre-lock witness.
        pub(super) fn new(checked: PreLockChecked, paths: RunPaths, repo_key: RepoKey) -> Self {
            Self {
                facts: Facts {
                    checked,
                    paths,
                    repo_key,
                },
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        pub(super) fn abort(self, reached: Prefix, error: UpstrokeError) -> Box<Aborted> {
            self.facts.abort(reached, None, error)
        }
    }

    /// P1 — `.creating` published.
    #[derive(Debug)]
    pub struct MarkerPublished {
        facts: Facts,
    }

    impl MarkerPublished {
        pub(super) fn new(previous: PublicDirCreated) -> Self {
            Self {
                facts: previous.facts,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        pub(super) fn abort(self, reached: Prefix, error: UpstrokeError) -> Box<Aborted> {
            self.facts.abort(reached, None, error)
        }
    }

    /// P2 — `run.lock` held by this process.
    #[derive(Debug)]
    pub struct RunLockHeld {
        facts: Facts,
        lock: RunLock,
    }

    impl RunLockHeld {
        pub(super) fn new(previous: MarkerPublished, lock: RunLock) -> Self {
            Self {
                facts: previous.facts,
                lock,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        pub(super) fn abort(self, reached: Prefix, error: UpstrokeError) -> Box<Aborted> {
            self.facts.abort(reached, Some(self.lock), error)
        }
    }

    /// P3a — the private directory exists, with no owner record.
    #[derive(Debug)]
    pub struct PrivateDirCreated {
        facts: Facts,
        lock: RunLock,
    }

    impl PrivateDirCreated {
        pub(super) fn new(previous: RunLockHeld) -> Self {
            Self {
                facts: previous.facts,
                lock: previous.lock,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        pub(super) fn abort(self, reached: Prefix, error: UpstrokeError) -> Box<Aborted> {
            self.facts.abort(reached, Some(self.lock), error)
        }
    }

    /// P3b — the owner record published, then the private skeleton.
    #[derive(Debug)]
    pub struct OwnerRecordPublished {
        facts: Facts,
        lock: RunLock,
        owner: OwnerRecord,
    }

    impl OwnerRecordPublished {
        pub(super) fn new(previous: PrivateDirCreated, owner: OwnerRecord) -> Self {
            Self {
                facts: previous.facts,
                lock: previous.lock,
                owner,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        /// The record this run published, so P4's refusal can name the boundary
        /// it is about to probe under.
        pub(super) fn owner(&self) -> &OwnerRecord {
            &self.owner
        }

        pub(super) fn abort(self, reached: Prefix, error: UpstrokeError) -> Box<Aborted> {
            self.facts.abort(reached, Some(self.lock), error)
        }
    }

    /// P4 — the `RunnerPreflight` certified: the shell probe, then one probe per
    /// recorded agent.
    #[derive(Debug)]
    pub struct ProbesCertified {
        facts: Facts,
        lock: RunLock,
        probed: Vec<String>,
    }

    impl ProbesCertified {
        pub(super) fn new(previous: OwnerRecordPublished, probed: Vec<String>) -> Self {
            Self {
                facts: previous.facts,
                lock: previous.lock,
                probed,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        pub(super) fn abort(self, reached: Prefix, error: UpstrokeError) -> Box<Aborted> {
            self.facts.abort(reached, Some(self.lock), error)
        }
    }

    /// P5 — the plan written, the log open, no committed first line.
    #[derive(Debug)]
    pub struct LogOpened {
        facts: Facts,
        lock: RunLock,
        log: EventLog,
        probed: Vec<String>,
    }

    impl LogOpened {
        pub(super) fn new(previous: ProbesCertified, log: EventLog) -> Self {
            Self {
                facts: previous.facts,
                lock: previous.lock,
                log,
                probed: previous.probed,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        pub(super) fn probed(&self) -> &[String] {
            &self.probed
        }

        /// The handle is dropped with the witness: an abort here has not
        /// crossed P5b, and cleanup removes the log with the public half.
        pub(super) fn abort(self, reached: Prefix, error: UpstrokeError) -> Box<Aborted> {
            drop(self.log);
            self.facts.abort(reached, Some(self.lock), error)
        }
    }

    /// P5b — `committed.json` published. **From here the creator deletes
    /// nothing**, so this witness has no `abort`.
    #[derive(Debug)]
    pub struct CommitRecordPublished {
        facts: Facts,
        lock: RunLock,
        log: EventLog,
        record: CommitRecord,
        line: TopologyLine,
        stamped: RunStarted4,
    }

    impl CommitRecordPublished {
        pub(super) fn new(
            previous: LogOpened,
            record: CommitRecord,
            line: TopologyLine,
            stamped: RunStarted4,
        ) -> Self {
            Self {
                facts: previous.facts,
                lock: previous.lock,
                log: previous.log,
                record,
                line,
                stamped,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        pub(super) fn record(&self) -> &CommitRecord {
            &self.record
        }

        pub(super) fn line(&self) -> &TopologyLine {
            &self.line
        }

        pub(super) fn log_mut(&mut self) -> &mut EventLog {
            &mut self.log
        }

        /// Give the poisoned handle back so the append-error protocol can drop
        /// it and reopen through `Event.OpenLog`.
        pub(super) fn into_parts(self) -> (Facts, RunLock, EventLog, CommitRecord) {
            (self.facts, self.lock, self.log, self.record)
        }
    }

    /// P6 — `run_started(4)` durable, marker still present.
    #[derive(Debug)]
    pub struct RunStartedDurable {
        facts: Facts,
        lock: RunLock,
        log: EventLog,
        fold: TopologyFold,
        event: TopologyEvent,
        /// `committed.json`, carried so the loop can name the digest the
        /// creator computed. Dropped at P6 before erratum-E6-adjacent work
        /// showed the loop needs it too.
        commit: CommitRecord,
        record: RunStarted4,
    }

    impl RunStartedDurable {
        pub(super) fn new(
            previous: CommitRecordPublished,
            fold: TopologyFold,
            event: TopologyEvent,
        ) -> Self {
            let record = previous.stamped.clone();
            let (facts, lock, log, commit) = previous.into_parts();
            Self {
                facts,
                lock,
                commit,
                log,
                fold,
                event,
                record,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }
    }

    /// P7 — the marker removed.
    #[derive(Debug)]
    pub struct MarkerRemoved {
        facts: Facts,
        lock: RunLock,
        log: EventLog,
        fold: TopologyFold,
        event: TopologyEvent,
        /// `committed.json`, carried so the loop can name the digest the
        /// creator computed. Dropped at P6 before erratum-E6-adjacent work
        /// showed the loop needs it too.
        commit: CommitRecord,
        record: RunStarted4,
    }

    impl MarkerRemoved {
        pub(super) fn new(previous: RunStartedDurable) -> Self {
            Self {
                commit: previous.commit,
                facts: previous.facts,
                lock: previous.lock,
                log: previous.log,
                fold: previous.fold,
                event: previous.event,
                record: previous.record,
            }
        }

        pub(super) fn facts(&self) -> &Facts {
            &self.facts
        }

        /// The record P6 committed — the authority for P8's ref and base.
        pub(super) fn record(&self) -> &RunStarted4 {
            &self.record
        }
    }

    /// P8 — the integration ref created zero-old. **The run exists.**
    #[derive(Debug)]
    pub struct Started {
        facts: Facts,
        lock: RunLock,
        log: EventLog,
        fold: TopologyFold,
        event: TopologyEvent,
        /// `committed.json`, carried so the loop can name the digest the
        /// creator computed. Dropped at P6 before erratum-E6-adjacent work
        /// showed the loop needs it too.
        commit: CommitRecord,
        record: RunStarted4,
    }

    impl Started {
        pub(super) fn new(previous: MarkerRemoved) -> Self {
            Self {
                commit: previous.commit,
                facts: previous.facts,
                lock: previous.lock,
                log: previous.log,
                fold: previous.fold,
                event: previous.event,
                record: previous.record,
            }
        }

        /// The run this created.
        #[must_use]
        pub fn run_id(&self) -> &str {
            self.facts.checked().run_id()
        }

        /// Both halves of the run directory.
        #[must_use]
        pub fn paths(&self) -> &RunPaths {
            self.facts.paths()
        }

        /// The `run_started(4)` this run is founded on, with the five fields the
        /// witnesses stamped.
        #[must_use]
        pub fn run_started(&self) -> &RunStarted4 {
            &self.record
        }

        /// The exact event the fold applied — the bytes on disk, read back,
        /// never a second serialization.
        #[must_use]
        pub fn event(&self) -> &TopologyEvent {
            &self.event
        }

        /// The append handle P6 committed through.
        pub fn log(&mut self) -> &mut EventLog {
            &mut self.log
        }

        /// The fold, with `run_started` applied.
        #[must_use]
        pub fn fold(&self) -> &TopologyFold {
            &self.fold
        }

        /// Take the run apart for the loop that will drive it.
        #[must_use]
        /// The run this created, as the loop's own state.
        ///
        /// **`decisions.sequential_substrate.engine` is one sentence about both
        /// paths**: "`TopologyRun` drives schema 4 at max_parallel = 1
        /// synchronously; every path exists here before Tokio." A fresh run is
        /// created by P0-P8 and a resumed one by the recovery order, and both
        /// then reach the same loop. Before this, only the resumed one could:
        /// nothing consumed `Started`, so `pr_sequence[8]`'s scope — which names
        /// "serialized run creation P0-P8" and the dispatch chain in one
        /// sentence — was half unreachable.
        ///
        /// The worktree lock is a parameter because the creator holds it across
        /// the census and the whole run, and it is not this chain's to mint.
        ///
        /// **Infallible, and the signature says so.** An earlier draft returned
        /// a `Result` "for symmetry with the recovery path", and the `# Errors`
        /// section that justified it outlived the `Result` itself — a doc
        /// promising a failure mode the type cannot express.
        pub fn into_handle(
            self,
            worktree: crate::rundir::WorktreeLock,
        ) -> super::super::recover::RunHandle {
            super::super::recover::RunHandle::created(
                self.record,
                self.commit.run_started_sha256,
                self.log,
                self.fold,
                self.lock,
                worktree,
            )
        }

        /// The created run's parts, for the caller that assembles its own
        /// handle rather than taking [`Self::into_handle`]'s.
        ///
        /// **Every element is a resource.** The two locks are guards whose whole
        /// contract is to be held; dropping the tuple drops both and releases a
        /// run's claim on its worktree while the run is still live.
        #[must_use]
        pub fn into_parts(self) -> (RunPaths, RunLock, EventLog, TopologyFold, TopologyEvent) {
            (self.facts.paths, self.lock, self.log, self.fold, self.event)
        }
    }
}

use steps::{
    CommitRecordPublished, LogOpened, MarkerPublished, MarkerRemoved, OwnerRecordPublished,
    PrivateDirCreated, ProbesCertified, PublicDirCreated, RunLockHeld, RunStartedDurable,
};

// ---------------------------------------------------------------------------
// What a stopped creation left behind
// ---------------------------------------------------------------------------

/// What the creator left on disk, and what it tells the operator.
///
/// The trichotomy T-RUNSTART's `resume_action` names, plus the append-error
/// protocol's three answers. Every variant that does not say "removed" deletes
/// nothing.
#[derive(Debug)]
pub enum Disposition {
    /// `RunDir.CreatePublicDir` itself failed: no run directory came to exist.
    NothingCreated,
    /// Nothing private was bound, so the public half alone was reclaimed.
    PublicHalfRemoved(UnboundShape),
    /// The ownership proof held and no commit record existed: the private half
    /// through the proof-token funnel, then the public directory with the marker
    /// last.
    BothHalvesRemoved { private: PathBuf },
    /// The ownership proof held and `RunDir.RemovePrivateHusk` returned an
    /// error, so **the public half was deliberately left where it is**.
    ///
    /// Distinct from [`Self::Retained`] because nothing about it was observed:
    /// the owner record is present (the proof that minted the spent token read
    /// it), the retention is a consequence of a failed removal rather than of an
    /// unprovable shape, and a removal was attempted.
    PrivateHalfRemovalFailed {
        /// The private half the spent token named.
        private: PathBuf,
        /// The public husk, marker and all, still on disk.
        public: PathBuf,
        /// The removal error, as the operator sees it.
        detail: String,
    },
    /// Retained and reported, nothing deleted. P3a is this, and so is every
    /// unprovable shape.
    Retained {
        reason: RetainReason,
        locator: PathBuf,
    },
    /// `committed.json` exists, or the filesystem would not say: retained,
    /// possibly committed, nothing deleted.
    PossiblyCommitted {
        locator: PathBuf,
        /// Set when the stat could not answer at all.
        undecidable: Option<String>,
    },
    /// The append-error protocol's proven prefix contains the committed first
    /// line: the run exists and is resumable. Also what a P7 or P8 failure
    /// reports.
    Committed {
        /// Whether `.creating` is still on disk.
        ///
        /// True at P5b and at P6, where nothing has removed it yet. **False at
        /// P7**, which is the step that removes it — so a P8 failure has no
        /// stale marker for a resume to repair, and the sentence must not
        /// promise one.
        stale_marker: bool,
    },
    /// The proven prefix does not contain it: a retained, possibly committed
    /// husk that the deferred prune removes.
    RetainedPossiblyCommittedHusk { locator: PathBuf },
    /// The barrier's sync failed, the reread was unstable, or the replay
    /// refused: undetermined and retained, nothing deleted.
    Undetermined {
        step: Option<BarrierStep>,
        detail: String,
    },
}

impl Disposition {
    /// Whether a reclaim this creator drove **completed**: the private half went
    /// through the proof-token funnel, or nothing private was bound and the
    /// public half alone was reclaimed.
    ///
    /// **Alethic**, and [`Self::PrivateHalfRemovalFailed`] answers `false` for
    /// that reason. It used to answer `true`, which gave it the identical pair
    /// of answers to [`Self::PublicHalfRemoved`] — `(true, false)` — for the
    /// opposite tree: on that arm the public half is gone and nothing private
    /// ever existed, on this one the public half is **deliberately still on
    /// disk** and the private half is in a state nobody observed. A caller
    /// reading both concluded "the public half went", which is exactly wrong.
    ///
    /// Mixing an epistemic reading into one of two sibling predicates is what
    /// made the arms indistinguishable, so both siblings are alethic and
    /// [`Self::may_have_removed_the_private_half`] carries what this arm
    /// actually knows.
    #[must_use]
    pub const fn removed_anything(&self) -> bool {
        matches!(
            self,
            Self::PublicHalfRemoved(_) | Self::BothHalvesRemoved { .. }
        )
    }

    /// Whether the private half is **known** to have been removed.
    ///
    /// [`Self::PrivateHalfRemovalFailed`] answers `false`: a failed removal's
    /// outcome is not decided by its error, so nothing here may claim the half
    /// is gone.
    #[must_use]
    pub const fn removed_the_private_half(&self) -> bool {
        matches!(self, Self::BothHalvesRemoved { .. })
    }

    /// Whether the private half **may** have been removed, in whole or in part.
    ///
    /// The epistemic sibling of [`Self::removed_the_private_half`], and the one
    /// question [`Self::PrivateHalfRemovalFailed`] can answer `true` to.
    /// `remove_dir_all` is not atomic and its error is the same value whether it
    /// removed nothing, every child, or the whole tree and then failed on the
    /// way out — so "is the private half gone" and "is there residue nobody
    /// observed" are two questions, and this is the second one. No arm that left
    /// the tree untouched answers `true` here, which is what separates this arm
    /// from [`Self::Retained`] and [`Self::PossiblyCommitted`].
    #[must_use]
    pub const fn may_have_removed_the_private_half(&self) -> bool {
        matches!(
            self,
            Self::BothHalvesRemoved { .. } | Self::PrivateHalfRemovalFailed { .. }
        )
    }

    /// The operator-facing sentence.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::NothingCreated => "no run directory was created".to_owned(),
            Self::PublicHalfRemoved(shape) => format!(
                "the public run directory was reclaimed ({}); no private half existed by ordering",
                match shape {
                    UnboundShape::Bare => "bare",
                    UnboundShape::StagedMarkerOnly => "only a staged marker",
                    UnboundShape::TargetAbsent => "its recorded private half was never created",
                }
            ),
            Self::BothHalvesRemoved { private } => format!(
                "both halves were reclaimed by the creating process, which held both locks and \
                 knew the run never committed: the private half at {} through the proof-token \
                 funnel, then the public directory with the marker last",
                private.display()
            ),
            Self::PrivateHalfRemovalFailed {
                private,
                public,
                detail,
            } => format!(
                "the private half at {} could not be removed ({detail}), so the public directory \
                 at {} was left in place with its marker: `.creating` is that private half's only \
                 locator, and removing it would orphan a directory no census, no `status` and no \
                 deferred `upstroke runs prune` could ever reach again. A removal that returns an \
                 error decides nothing, so the next census finishes whichever of three shapes is \
                 there: it reclaims the pair if the private half is still whole, the public husk \
                 alone if the private half is gone, and — if the removal emptied the private \
                 directory without removing the directory itself — it reports the pair as \
                 retained and deletes neither half",
                private.display(),
                public.display()
            ),
            Self::Retained { reason, locator } => format!(
                "both halves are retained and reported, with nothing deleted: {reason}. The \
                 private half is at {}; the deferred `upstroke runs prune` is the only path that \
                 removes it",
                locator.display()
            ),
            Self::PossiblyCommitted {
                locator,
                undecidable,
            } => match undecidable {
                Some(detail) => format!(
                    "the private commit record at {} could not be stat-ed ({detail}), so this run \
                     is treated as possibly committed and nothing was deleted",
                    locator.display()
                ),
                None => format!(
                    "the private commit record at {} exists, so this run may have committed and \
                     nothing was deleted",
                    locator.display()
                ),
            },
            Self::Committed { stale_marker: true } => {
                "the run exists and is resumable; its stale marker is repaired by the resume"
                    .to_owned()
            }
            Self::Committed {
                stale_marker: false,
            } => "the run exists and is resumable; P7 already removed its `.creating`, so a \
                  resume has nothing there to repair. Its integration ref is what P8 did not \
                  establish, and the resume creates the integration ref zero-old at the recorded \
                  base — the same `ensure_integration_ref` P8 calls, so a ref already sitting at \
                  that base is adopted rather than created a second time"
                .to_owned(),
            Self::RetainedPossiblyCommittedHusk { locator } => format!(
                "the proven prefix has no committed first line, so this is a retained, possibly \
                 committed husk with its private half at {}; nothing was deleted",
                locator.display()
            ),
            Self::Undetermined { step, detail } => format!(
                "the outcome is undetermined and the run is retained as possibly committed{}: \
                 {detail}. Nothing was deleted; the next write command establishes the barrier \
                 before acting",
                step.map_or_else(String::new, |step| format!(" at {step}"))
            ),
        }
    }
}

/// A creation that stopped, with everything the operator needs.
#[derive(Debug)]
pub struct Refused {
    /// The last prefix that completed.
    pub reached: Prefix,
    /// What is on disk now.
    ///
    /// Boxed, with [`Self::error`], so `Result<Started, Refused>` stays small:
    /// the `Ok` arm is what a run returns and the `Err` arm is cold, and
    /// `clippy::result_large_err` is right that the cold arm should not widen
    /// every frame of the hot one.
    pub disposition: Box<Disposition>,
    /// Why the sequence stopped.
    pub error: Box<UpstrokeError>,
    /// The run id, so the report names it even when the directory is gone.
    pub run_id: String,
}

impl Refused {
    /// The one error the write command ends with.
    #[must_use]
    pub fn into_error(self) -> UpstrokeError {
        UpstrokeError::Refused {
            message: format!(
                "creating run `{}` stopped at {}: {}. {}",
                self.run_id,
                self.reached.name(),
                self.error,
                self.disposition.describe()
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// The one cleanup call
// ---------------------------------------------------------------------------

/// The creator's cleanup: stat fail-closed, prove, and spend the token, **in one
/// call**.
///
/// `run_creation`'s deletion boundary, in the order it states it:
///
/// > creator cleanup on a returned error obeys the one deletion boundary: while
/// > `<private>/committed.json` does not exist … the creating process, which
/// > knows the run never committed and holds both locks, removes the probe
/// > containers, the private half, and the public directory (best-effort …);
/// > from the moment `committed.json` exists … the creator deletes nothing.
///
/// **One function, on purpose.** Stat-ing in one place and proving in another
/// removes a *minting* path but not the stale-*use* sequence: a
/// `CommitRecordPresence::Absent` computed before P5b can still be held in a
/// local across it and spent afterwards. Here the stat, the proof and the spend
/// are three consecutive statements of one call, so no value crosses the
/// boundary and there is nothing to hold stale. The name carries the obligation
/// rather than a comment at the call site.
///
/// **The proof is the existing one.** Every conjunct of
/// [`prove_private_half_ownership`] passes for the creator at P3b–P5: it wrote
/// the marker and the owner record itself, statements apart, with the same run
/// id, repo key, canonical paths, incarnation and runner digest. So the creator
/// needs no second `PrivateHalfProof` constructor — and adding one would cost
/// the field privacy that makes the token unforgeable.
///
/// **The probe containers are not removed here.** `crash_reconstruction` gives
/// them to step (a) of the next write command's census, which reclaims by the
/// private-root label under the incarnation-aware liveness rule and reports each
/// reclaimed container's boundary from its intent. A creator that killed them
/// here would be a second reclaim authority for the same rows, and would have to
/// re-derive the boundary the census already reports.
fn stat_after_error(aborted: &mut Aborted, hooks: &mut dyn TopologyHooks) -> Disposition {
    if aborted.reached == Prefix::Nothing {
        return Disposition::NothingCreated;
    }
    let private = aborted.paths.private.clone();
    let public = aborted.paths.public.clone();

    // The run lock goes back **through its funnel**, and it goes back before
    // anything else: `run.lock` lives inside the public directory, and on
    // Windows an open handle without `FILE_SHARE_DELETE` refuses that unlink
    // outright. Ahead of the stat rather than after the proof, so that every
    // path out of this function has named `Lock.Release` — the two possibly
    // committed ones included. They are the paths the rationale is *about*: the
    // creating process is ending either way, and a husk whose lock is still
    // held is one the next census **skips** rather than reports, and these are
    // the answers that end in a reported husk.
    if let Some(lock) = aborted.lock.take() {
        lock.release(hooks.rundir());
    }

    // (1) The stat, fail-closed. `Unknown` is not an answer, and it is treated
    // as `Present`, because the cost of being wrong is asymmetric: a retained
    // husk is reported until an operator prunes it, and a deleted committed run
    // is gone.
    match commit_record_after_error(&private) {
        CommitRecordPresence::Present => {
            return Disposition::PossiblyCommitted {
                locator: private,
                undecidable: None,
            };
        }
        CommitRecordPresence::Unknown(detail) => {
            return Disposition::PossiblyCommitted {
                locator: private,
                undecidable: Some(detail),
            };
        }
        CommitRecordPresence::Absent => {}
    }

    // (2) The proof.
    let proof = prove_private_half_ownership(&public, &aborted.repo_key, &aborted.private_root);

    // (3) The spend, in the census's own order: the private half through the
    // token funnel, then the public directory **with the marker last**, so a
    // kill inside the removal leaves a husk the next census completes.
    match proof {
        PrivateHalfOwnership::Proven(token) => {
            let target = token.target().to_path_buf();
            // **The public half goes only if the private one went.**
            // `remove_public_husk` deletes `<public>` including `.creating`, and
            // that marker is the private half's only locator: the sole
            // production `read_dir` over a runs root is `rundir::run_dir_names`
            // over `<repo>/.upstroke/runs`, nothing enumerates `<R>/runs`, and a
            // private half no marker names is one no census, no `status` and no
            // deferred prune can ever reach again. So a failed private removal
            // returns here rather than falling through — and every shape it can
            // leave is one a later pass can still act on. `remove_dir_all` has
            // three outcomes behind one error value, not two: nothing removed,
            // so the next census proves the pair and reclaims both; everything
            // removed and the error raised on the way out, so the marker names
            // an absent target and the next census reclaims the public husk
            // alone (`TargetAbsent`); and every child removed but not the
            // directory — an unwritable parent, or on Windows a handle on the
            // directory — so the target exists with no `owner.json` in it and
            // the next census **retains the pair and reports it**
            // (`OwnerRecordMissing`), which is the deferred prune's shape rather
            // than a reclaim. Nothing is orphaned in any of the three.
            if let Err(error) = remove_private_husk(token, hooks.rundir()) {
                return Disposition::PrivateHalfRemovalFailed {
                    private: target,
                    public,
                    detail: error.to_string(),
                };
            }
            // Best-effort, as today: a public removal that failed leaves a husk
            // whose marker names an absent target, which the next census
            // reclaims public-only, and it is not a second error to report over
            // the one that stopped the run.
            let _ = remove_public_husk(&public, hooks.rundir());
            Disposition::BothHalvesRemoved { private: target }
        }
        PrivateHalfOwnership::NothingBound(shape) => {
            let _ = remove_public_husk(&public, hooks.rundir());
            Disposition::PublicHalfRemoved(shape)
        }
        // **P3a, and every other unprovable shape: nothing is removed.** Not
        // the private half, which is unprovable, and not the public half
        // either — the marker inside it is the private half's only locator, so
        // removing it would orphan a directory no census, no `status` and no
        // deferred prune could ever reach again.
        PrivateHalfOwnership::Retained(reason) => Disposition::Retained {
            reason,
            locator: private,
        },
    }
}

// ---------------------------------------------------------------------------
// The request
// ---------------------------------------------------------------------------

/// Everything P0–P8 needs that the pre-lock checks did not establish.
pub struct Request<'a> {
    /// The repository whose `.upstroke/runs` the public half goes in.
    pub repo_root: &'a Path,
    /// This repository's key, as the marker and both private records carry it.
    pub repo_key: RepoKey,
    /// The exact `plan.normalized.json` bytes P5 writes.
    pub normalized_plan: &'a [u8],
    /// What the fold is derived against — and what the append-error protocol's
    /// checked replay replays the surviving prefix through.
    pub inputs: FrozenInputs,
    /// The record P6 appends. Five fields are **stamped** from the witnesses:
    /// see [`create_run`].
    pub record: RunStarted4,
    /// The agents P4 probes, in the order they are recorded.
    pub agents: &'a [String],
    /// P4's `RunnerPreflight`.
    pub probes: &'a dyn Probes,
    /// P8's ref funnel.
    pub refs: &'a dyn IntegrationRefs,
    /// Where `run_started`'s timestamp comes from.
    pub clock: &'a dyn TimeSource,
}

// ---------------------------------------------------------------------------
// P0–P8
// ---------------------------------------------------------------------------

/// Create a schema-4 run: P0 through P8, under a worktree lock the caller holds.
///
/// The caller must hold `Lock.AcquireWorktree` and must have censused (O02,
/// O03); `startup.rs` owns both and this module takes neither, so "the census
/// precedes creation" stays a fact about the call site rather than something two
/// modules could each believe the other did.
///
/// **Five fields of `request.record` are stamped from the witnesses**: `run_id`,
/// `incarnation`, `runner`, `private_dir` and `probed_agents`. Not checked —
/// stamped. INV-23 requires `run_started(4).runner` to be the policy "resolved
/// once by read-only inspection **before the worktree lock**", and a check would
/// leave a caller able to pass a policy that disagrees and get a refusal *after*
/// P0–P5 had run; taking the value from the witness makes the disagreement
/// inexpressible.
///
/// # Errors
///
/// [`Refused`], which names the prefix reached, what is on disk now, and why. It
/// is never `Ok` with a partially created run: every failure path either cleans
/// up under the deletion boundary or reports what it retained.
pub fn create_run(
    checked: PreLockChecked,
    mut request: Request<'_>,
    hooks: &mut dyn TopologyHooks,
    warnings: &mut Vec<String>,
) -> Result<Started, Refused> {
    /// Every step before P5b returns its residue rather than its error: the
    /// cleanup decision is the same for all of them and is made in one place.
    macro_rules! before_the_boundary {
        ($step:expr) => {
            match $step {
                Ok(reached) => reached,
                Err(mut aborted) => {
                    let disposition = stat_after_error(&mut aborted, hooks);
                    return Err(Refused {
                        reached: aborted.reached,
                        disposition: Box::new(disposition),
                        error: Box::new(aborted.error),
                        run_id: aborted.run_id,
                    });
                }
            }
        };
    }

    let p0 = before_the_boundary!(p0_create_public_dir(checked, &request, hooks));
    let p1 = before_the_boundary!(p1_publish_marker(p0, hooks));
    let p2 = before_the_boundary!(p2_acquire_run_lock(p1, hooks));
    let p3a = before_the_boundary!(p3a_create_private_dir(p2, hooks));
    let p3b = before_the_boundary!(p3b_publish_owner_record(p3a, hooks));
    let p4 = before_the_boundary!(p4_run_preflight(p3b, &mut request, hooks));
    let p5 = before_the_boundary!(p5_write_plan_and_open_log(p4, &request, hooks, warnings));
    let (p5b, fold, delta) = before_the_boundary!(p5b_publish_commit_record(p5, &request, hooks));

    // From here the creator deletes nothing, whatever fails.
    let p6 = p6_append_run_started(p5b, fold, delta, &request, hooks, warnings)?;
    let p7 = p7_remove_marker(p6, hooks)?;
    p8_create_integration_ref(p7, hooks, request.refs)
}

/// P0 — `RunDir.CreatePublicDir`. The directory is **bare**: T-RUNSTART's own
/// word, and the census's reclaim of a bare directory depends on it.
fn p0_create_public_dir(
    checked: PreLockChecked,
    request: &Request<'_>,
    hooks: &mut dyn TopologyHooks,
) -> Result<PublicDirCreated, Box<Aborted>> {
    let paths = RunPaths::from_parts(
        crate::rundir::public_dir(request.repo_root, checked.run_id()),
        checked.private_dir(),
    );
    let created = create_public_dir(&paths.public, hooks.rundir());
    let p0 = PublicDirCreated::new(checked, paths, request.repo_key.clone());
    match created {
        Ok(()) => Ok(p0),
        Err(error) => Err(p0.abort(Prefix::Nothing, error)),
    }
}

/// P1 — `RunDir.StageMarker` then `RunDir.PublishMarker`: write `.creating.tmp`,
/// fsync, rename, fsync the directory.
fn p1_publish_marker(
    p0: PublicDirCreated,
    hooks: &mut dyn TopologyHooks,
) -> Result<MarkerPublished, Box<Aborted>> {
    let facts = p0.facts();
    let public = facts.paths().public.clone();
    let marker = CreatingMarker {
        run_id: facts.checked().run_id().to_owned(),
        repo_key: facts.repo_key().as_str().to_owned(),
        private_dir: facts.checked().private_dir().to_string_lossy().into_owned(),
        incarnation: facts.checked().incarnation().0.clone(),
        pid: facts.checked().pid(),
        runner_policy_sha256: facts.checked().runner_policy_sha256().to_owned(),
    };
    if let Err(error) = stage_marker(&public, &marker, hooks.rundir()) {
        let reached = if staged(&public.join(MARKER_STAGED)) {
            Prefix::P1Staged
        } else {
            Prefix::P0
        };
        return Err(p0.abort(reached, error));
    }
    if let Err(error) = publish_marker(&public, hooks.rundir()) {
        return Err(p0.abort(Prefix::P1Staged, error));
    }
    Ok(MarkerPublished::new(p0))
}

/// P2 — `Lock.AcquireRun` (R17) on `<public>/run.lock` (R21).
fn p2_acquire_run_lock(
    p1: MarkerPublished,
    hooks: &mut dyn TopologyHooks,
) -> Result<RunLockHeld, Box<Aborted>> {
    let public = p1.facts().paths().public.clone();
    match RunLock::acquire_hooked(&public, hooks.rundir()) {
        Ok(lock) => Ok(RunLockHeld::new(p1, lock)),
        Err(error) => Err(p1.abort(Prefix::P1, error)),
    }
}

/// P3a — `RunDir.CreatePrivateDir` at the **recorded** locator.
fn p3a_create_private_dir(
    p2: RunLockHeld,
    hooks: &mut dyn TopologyHooks,
) -> Result<PrivateDirCreated, Box<Aborted>> {
    let private = p2.facts().paths().private.clone();
    if let Err(error) = create_private_dir(&private, hooks.rundir()) {
        return Err(p2.abort(Prefix::P2, error));
    }
    Ok(PrivateDirCreated::new(p2))
}

/// P3b — the reciprocal owner record, **before any other private content**, then
/// the private skeleton.
fn p3b_publish_owner_record(
    p3a: PrivateDirCreated,
    hooks: &mut dyn TopologyHooks,
) -> Result<OwnerRecordPublished, Box<Aborted>> {
    let facts = p3a.facts();
    let public = facts.paths().public.clone();
    let private = facts.paths().private.clone();
    let owner = OwnerRecord {
        run_id: facts.checked().run_id().to_owned(),
        repo_key: facts.repo_key().as_str().to_owned(),
        // The proof compares this against `canonicalize(<public>)`, so it is
        // taken the same way rather than assembled from the repo root.
        public_dir: canonical_string(&public),
        incarnation: facts.checked().incarnation().0.clone(),
        // The full policy, not its digest: the marker carries the digest and the
        // proof compares the two.
        runner: facts.checked().runner_policy().clone(),
    };
    if let Err(error) = stage_owner_record(&private, &owner, hooks.rundir()) {
        let reached = if staged(&private.join(OWNER_RECORD_STAGED)) {
            Prefix::P3aStaged
        } else {
            Prefix::P3a
        };
        return Err(p3a.abort(reached, error));
    }
    if let Err(error) = publish_owner_record(&private, hooks.rundir()) {
        return Err(p3a.abort(Prefix::P3aStaged, error));
    }
    if let Err(error) = create_private_skeleton(&private, hooks.rundir()) {
        return Err(p3a.abort(Prefix::P3b, error));
    }
    Ok(OwnerRecordPublished::new(p3a, owner))
}

/// P4 — the `RunnerPreflight`: **the shell probe, then one probe per recorded
/// agent**, each a registered invocation.
///
/// The order is INV-23's and is not incidental: the shell is what every gate and
/// every agent process is executed *through*, so a machine whose shell does not
/// run `exit 0` cannot certify anything about a CLI, and asking anyway would
/// spend a slot and a container to learn nothing.
fn p4_run_preflight(
    p3b: OwnerRecordPublished,
    request: &mut Request<'_>,
    hooks: &mut dyn TopologyHooks,
) -> Result<ProbesCertified, Box<Aborted>> {
    let _ = hooks;
    let expected = p3b.facts().checked().runner_policy_sha256().to_owned();
    if request.probes.policy_digest() != expected {
        let recorded = crate::runner::policy::runner_policy_sha256(&p3b.owner().runner);
        return Err(p3b.abort(
            Prefix::P3b,
            UpstrokeError::Refused {
                message: format!(
                    "the pre-flight probes execute under `{}`, and this run's recorded \
                     `RunnerPolicy` digests to `{expected}` (the owner record's own runner \
                     digests to `{recorded}`). INV-23 records the policy in the marker (P1) and \
                     in the owner record (P3b) before the first probe, and every container intent \
                     carries that digest — a probe under another boundary would own containers \
                     the owner record does not describe",
                    request.probes.policy_digest()
                ),
            },
        ));
    }

    // The shell probe: non-slotted, and the identity says so rather than a
    // comment — `is_slotted` is a total function of the id and answers `false`
    // for `ProbeTarget::Shell`.
    let shell_id = match PreflightIdentities::shell(0) {
        Ok(id) => id,
        Err(error) => return Err(p3b.abort(Prefix::P3b, error)),
    };
    // Same boundary as the agent probes below: the shell probe's own request
    // registers itself through `Registering`, so there is nothing to settle
    // here. The identity is still built here because the *ordinal* is this
    // module's — `recovery_order` (c) puts the shell probe first.
    if let Err(error) = request.probes.shell(shell_id.clone()) {
        return Err(p3b.abort(Prefix::P3b, error));
    }

    // **No outer registration around the adapter call.** This wrapped one
    // `probe(agent, 0)` identity, with one slot pair, around the *whole* of
    // `probes.agent(...)` — and an adapter runs a process per request. A current
    // Codex probe runs ten: version, two help probes, six strict-config probes
    // and the model catalog. Only ordinal 0 reached the ledger, so a failure at
    // ordinal 1 was recorded as **ordinal 0 cancelled**: the ledger named the
    // process that succeeded and held no record of the one that failed. The
    // `bf927f3` review's third P1.
    //
    // Registration now happens where the process is built, in
    // `preflight::Registering`, which `RunnerProbes` wraps its runner in — the
    // same boundary resume has used since it was written, and whose own doc says
    // "one place, so that 'each a registered invocation' is true of a process an
    // adapter built as much as of one this module built". This module was the
    // other place.
    let mut probed = Vec::with_capacity(request.agents.len());
    for agent in request.agents {
        if let Err(error) = request.probes.agent(agent) {
            return Err(p3b.abort(Prefix::P3b, error));
        }
        probed.push(agent.clone());
    }
    Ok(ProbesCertified::new(p3b, probed))
}

/// P5 — `RunDir.WritePlan`, then `Event.OpenLog` (create, fsync the directory).
///
/// The log is left with **no committed first line**, which is the whole of what
/// makes P5 a distinguishable prefix: `rundir::classify_run_dir` reads exactly
/// that and calls the directory a Husk.
fn p5_write_plan_and_open_log(
    p4: ProbesCertified,
    request: &Request<'_>,
    hooks: &mut dyn TopologyHooks,
    warnings: &mut Vec<String>,
) -> Result<LogOpened, Box<Aborted>> {
    let public = p4.facts().paths().public.clone();
    if let Err(error) = write_plan(&public, request.normalized_plan, hooks.rundir()) {
        return Err(p4.abort(Prefix::P4, error));
    }
    let events = p4.facts().paths().events();
    match EventLog::open_hooked(EventSite::OpenLog, &events, warnings, hooks.events()) {
        Ok(log) => Ok(LogOpened::new(p4, log)),
        Err(error) => Err(p4.abort(Prefix::P4, error)),
    }
}

/// P5b — the private commit record. **The deletion boundary.**
///
/// `run_started_sha256` is "the digest of the exact `run_started` line bytes
/// **about to be appended**", so the event is built, round-tripped and checked
/// against the fold *before* this record is published: `emit`'s "build event →
/// serialize → round-trip → `plan_transition` → append the exact bytes", and "a
/// `FoldError` aborts before any write". A record published before the
/// transition check would put a run past the deletion boundary on the strength
/// of an event the fold would have refused.
fn p5b_publish_commit_record(
    p5: LogOpened,
    request: &Request<'_>,
    hooks: &mut dyn TopologyHooks,
) -> Result<(CommitRecordPublished, TopologyFold, TopologyDelta), Box<Aborted>> {
    let facts = p5.facts();
    let public = facts.paths().public.clone();
    let private = facts.paths().private.clone();
    let events = facts.paths().events();

    // Stamp the five fields the witnesses own.
    let mut stamped = request.record.clone();
    stamped.run_id = facts.checked().run_id().to_owned();
    stamped.incarnation = facts.checked().incarnation().clone();
    stamped.runner = facts.checked().runner_policy().clone();
    stamped.private_dir = private.to_string_lossy().into_owned();
    stamped.probed_agents = p5.probed().to_vec();

    let event = TopologyEvent {
        ts: request.clock.now_rfc3339(),
        body: TopologyEventBody::RunStarted {
            data: Box::new(stamped.clone()),
        },
    };
    let (line, written) = match TopologyLine::round_trip(&event) {
        Ok(pair) => pair,
        Err(error) => return Err(p5.abort(Prefix::P5, error)),
    };
    let fold = TopologyFold::new(request.inputs.clone());
    let delta = match fold.plan_transition(&written) {
        Ok(delta) => delta,
        Err(error) => {
            return Err(p5.abort(
                Prefix::P5,
                UpstrokeError::EventLog {
                    path: events,
                    message: format!(
                        "the fold refuses this run's own `run_started(4)`, so nothing was written \
                         and the run never crossed the deletion boundary: {error}"
                    ),
                },
            ));
        }
    };

    let Some(run_started_sha256) = first_line_digest(line.committed_bytes()) else {
        return Err(p5.abort(
            Prefix::P5,
            UpstrokeError::EventLog {
                path: events,
                message: "the round-tripped `run_started` line carries no commit marker".to_owned(),
            },
        ));
    };
    let commit = CommitRecord {
        run_id: facts.checked().run_id().to_owned(),
        repo_key: facts.repo_key().as_str().to_owned(),
        public_dir: canonical_string(&public),
        incarnation: facts.checked().incarnation().0.clone(),
        run_started_sha256,
    };
    if let Err(error) = stage_commit_record(&private, &commit, hooks.rundir()) {
        return Err(p5.abort(Prefix::P5, error));
    }
    if let Err(error) = publish_commit_record(&private, hooks.rundir()) {
        // The one place the prefix is not enough to decide: the funnel's
        // error-return mode returns `Err` *after* performing the rename, so this
        // error is the same value whether or not the record landed.
        // `stat_after_error` asks the filesystem, fail-closed.
        return Err(p5.abort(Prefix::P5, error));
    }
    Ok((
        CommitRecordPublished::new(p5, commit, line, stamped),
        fold,
        delta,
    ))
}

/// P6 — `Event.AppendFirst`: `run_started(4)`, written and synced.
///
/// On an error the **append-error protocol** runs and this function deletes
/// nothing: no `apply_delta`, no retry, no report from the in-memory fold, no
/// cleanup. The fold is poisoned explicitly, the poisoned handle is dropped, the
/// log is reopened through `Event.OpenLog` (which truncates a torn first line),
/// and the stable-prefix barrier decides which of the three answers the operator
/// is given.
fn p6_append_run_started(
    mut p5b: CommitRecordPublished,
    mut fold: TopologyFold,
    delta: TopologyDelta,
    request: &Request<'_>,
    hooks: &mut dyn TopologyHooks,
    warnings: &mut Vec<String>,
) -> Result<RunStartedDurable, Refused> {
    let line = p5b.line().clone();
    let expected = p5b.record().run_started_sha256.clone();
    let events = p5b.facts().paths().events();
    let private = p5b.facts().paths().private.clone();
    let run_id = p5b.facts().checked().run_id().to_owned();

    let appended =
        p5b.log_mut()
            .append_topology_hooked(EventSite::AppendFirst, &line, hooks.events());

    match appended {
        Ok(()) => {
            // Only now: `apply_delta` runs after the funnel returned `Ok`.
            let event = delta.event().clone();
            fold.apply_delta(delta);
            Ok(RunStartedDurable::new(p5b, fold, event))
        }
        Err(error) => {
            // (a) No `apply_delta`, and the fold is poisoned **explicitly**
            //     rather than by being dropped: `emit` says "the in-memory fold
            //     is marked poisoned (every later transition attempt in this
            //     process is refused)".
            drop(delta);
            fold.poison();
            // (b) Nothing is cancelled because nothing is in flight: P4's probes
            //     were all settled, which is why the ledger balances here. The
            //     claim is asserted rather than assumed.
            //
            //     **Both halves, since 2026-08-27.** P4 used to acquire and
            //     release each probe's slot pair itself, so "every pair
            //     released" was visible in this module's own code. Registration
            //     moved to `preflight::Registering`, where the process is built,
            //     and the R4 half moved with it — so the claim is now checked
            //     here rather than being implied by calls that are no longer in
            //     view. A held pair at this point means a probe took a slot and
            //     did not give it back.
            //     **Read through the probes**, which own them, so this cannot
            //     be checking a different pair from the one P4 used.
            let balanced = request
                .probes
                .ledger()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .balances()
                && request
                    .probes
                    .slots()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .held()
                    .is_none();
            // (c) The handle is dropped, unretried, and the log reopened.
            let (facts, lock, log, _record) = p5b.into_parts();
            drop(log);

            let disposition = append_error_protocol(
                &events,
                &private,
                &expected,
                request.inputs.clone(),
                warnings,
                hooks,
            );
            // Nothing is deleted on this path, so the lock is out of nothing's
            // way; it goes back on the ordinary drop.
            drop(lock);
            drop(facts);

            Err(Refused {
                reached: Prefix::P5b,
                disposition: Box::new(disposition),
                error: Box::new(UpstrokeError::EventLog {
                    path: events,
                    message: format!(
                        "`run_started` returned an error from the append funnel, so its outcome \
                         is unknown and it is never retried{}: {error}",
                        if balanced {
                            ""
                        } else {
                            " (and this process still holds a registered invocation, which a \
                             fresh run's P6 never should)"
                        }
                    ),
                }),
                run_id,
            })
        }
    }
}

/// The append-error protocol's three answers.
///
/// `coordinator_integration.append_error_protocol`: reopen through
/// `Event.OpenLog` (torn-tail normalization), establish the stable-prefix
/// barrier — the surviving prefix successfully synced, reread stable, and
/// checked-replayed from those same bytes — and end reporting the run as
/// committed, as a retained possibly committed husk, or as undetermined.
///
/// The commit record's digest is compared **here** rather than handed to
/// `establish_stable_prefix` as its `committed_first_line_sha256` argument: that
/// argument makes an absent first line a *barrier failure*, and an absent first
/// line is one of the three answers this protocol has to be able to give.
fn append_error_protocol(
    events: &Path,
    private: &Path,
    expected: &str,
    inputs: FrozenInputs,
    warnings: &mut Vec<String>,
    hooks: &mut dyn TopologyHooks,
) -> Disposition {
    let prefix = match establish_stable_prefix(events, inputs, None, warnings, hooks.events()) {
        Ok(prefix) => prefix,
        Err(barrier) => {
            return Disposition::Undetermined {
                step: Some(barrier.step),
                detail: barrier.to_string(),
            };
        }
    };
    match first_line_digest(prefix.bytes()) {
        // P7 has not run, so `.creating` is still there for the resume to
        // repair.
        Some(actual) if actual == expected => Disposition::Committed { stale_marker: true },
        None => Disposition::RetainedPossiblyCommittedHusk {
            locator: private.to_path_buf(),
        },
        Some(actual) => Disposition::Undetermined {
            step: None,
            detail: format!(
                "the proven prefix's committed first line digests {actual}, and the private \
                 commit record says {expected}: this log's first line is not the one this process \
                 was appending"
            ),
        },
    }
}

/// P7 — `RunDir.RemoveMarker`, once `run_started` is durable.
fn p7_remove_marker(
    p6: RunStartedDurable,
    hooks: &mut dyn TopologyHooks,
) -> Result<MarkerRemoved, Refused> {
    let public = p6.facts().paths().public.clone();
    let run_id = p6.facts().checked().run_id().to_owned();
    if let Err(error) = remove_marker(&public, hooks.rundir()) {
        // The run exists. Nothing is deleted, and the stale marker is repaired
        // by the next census with the lock free, or by the owner on resume.
        return Err(Refused {
            reached: Prefix::P6,
            // P7 is the step that just failed, so the marker it was removing is
            // still on disk.
            disposition: Box::new(Disposition::Committed { stale_marker: true }),
            error: Box::new(error),
            run_id,
        });
    }
    Ok(MarkerRemoved::new(p6))
}

/// P8 — `Ref.CreateIntegration`, zero-old, at the recorded base.
///
/// `resume_action`: "P7/P8: create the ref zero-old at the recorded base if
/// absent; if present == base continue (**no spend repeats**)". A ref at any
/// other SHA, a symbolic ref, and a ref checked out in some worktree all refuse
/// — and refuse *after* `run_started`, because O15 puts the ref after the event
/// and a run that exists is not un-created by a ref that cannot be published.
fn p8_create_integration_ref(
    p7: MarkerRemoved,
    hooks: &mut dyn TopologyHooks,
    refs: &dyn IntegrationRefs,
) -> Result<Started, Refused> {
    let refname = p7.record().integration_ref.0.clone();
    let base = p7.record().base_sha.0.clone();
    match ensure_integration_ref(refs, hooks.effects(), &refname, &base) {
        Ok(()) => Ok(Started::new(p7)),
        Err(error) => Err(Refused {
            reached: Prefix::P7,
            // P7 returned, so the marker is **gone**: this is the one committed
            // disposition with nothing stale left for a resume to repair.
            disposition: Box::new(Disposition::Committed {
                stale_marker: false,
            }),
            error: Box::new(error),
            run_id: p7.facts().checked().run_id().to_owned(),
        }),
    }
}

/// The integration ref at `base`, created if absent and adopted if already
/// there — the body of P8 **and** of the P7/P8 recovery step.
///
/// One function because `resume_action` gives both the same sentence, and two
/// implementations of "if present == base continue" are two places for a run
/// that was killed between P6 and P8 to be treated differently from one that
/// was not.
///
/// **Both callers exist.** P8 is one; the other is
/// [`super::recover::ensure_recorded_integration_ref`], which is the P7/P8
/// recovery step and supplies `run_started(4).integration_ref` and
/// `run_started(4).base_sha` from the authenticated record. So the sentence
/// above is a claim about a step a resume performs, and
/// [`Disposition::Committed`]'s report promises exactly that action —
/// `create::tests::the_p8_report_promises_exactly_the_resume_action_the_resume_performs`
/// keeps the two in correspondence in both directions.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] when the ref is symbolic, checked out, or at any
/// SHA other than `base`; a Git error from the creation.
pub fn ensure_integration_ref(
    refs: &dyn IntegrationRefs,
    hooks: &mut dyn crate::workspace_manager::EffectHooks,
    refname: &str,
    base: &str,
) -> Result<(), UpstrokeError> {
    refs.assert_publishable(refname)?;
    match refs.direct_target(refname)? {
        None => refs.create_zero_old(hooks, refname, base),
        // Already there, at the recorded base: continue. Creating it again
        // would fail zero-old, and re-pointing it would repeat a spend.
        Some(at) if at == base => Ok(()),
        Some(at) => Err(UpstrokeError::Refused {
            message: format!(
                "the integration ref `{refname}` is at {at} and this run records its base as \
                 {base}. A ref that already names another commit belongs to something else; it is \
                 never moved to make room for a run"
            ),
        }),
    }
}

/// Whether a staging file is on disk — the one label a `RunDir.Stage*` error
/// cannot decide on its own.
///
/// `stage_json` creates the `.tmp`, writes it, and *then* fsyncs it, so a
/// staging error is the second error in this sequence — `PublishCommitRecord`'s
/// is the first — whose residue is not a function of its value: nothing when the
/// create failed, a staging file when the sync did. The prefix is the coordinate
/// the operator is given and it is the census's own vocabulary (`P1Staged` is
/// reclaimable, `P3aStaged` is retained and **not** content-free), so it is
/// stat-ed rather than assumed. Read-only, and it deletes nothing: it decides a
/// name, not a disposition.
///
/// `symlink_metadata`, so a staging file that is a dangling symlink still reads
/// as residue — `startup::stale_marker_present` reads the marker the same way. A
/// stat that cannot answer reads as absent, which is what the census's own
/// `read_dir` of that directory would report for the same path.
fn staged(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// A path as the records spell it: canonical when the filesystem can say, and
/// the path itself when it cannot.
///
/// The fallback matters and is deliberate: the proof compares the record against
/// `canonicalize(<public>)` with the same fallback, so a filesystem that will
/// not canonicalize produces two equal non-canonical strings rather than one
/// canonical and one not.
fn canonical_string(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests;
