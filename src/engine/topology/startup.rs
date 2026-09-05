//! The startup census — **both** halves, in the packet's order, returning the
//! witness that says which one ran.
//!
//! `decisions.sequential_substrate.startup_census`: "the census then performs:
//! (a) global container reclaim over `<R>/containers` and docker ps by the
//! private-root label under the incarnation-aware liveness rule …; (b)
//! run-directory census over `<repo>/.upstroke/runs`".
//!
//! Half (a) is [`crate::runner::container::census::run_startup_census`], landed
//! by PR6 and documented in the tree as "**Step (a)** of the startup census".
//! This module calls it and does not reimplement it.
//!
//! Half (b) is written here. Before this module there was no run-directory
//! census anywhere in the tree: [`crate::rundir`] has `classify_run_dir`,
//! `husk_report` and `prove_private_half_ownership`, and none of them iterates a
//! runs directory or reclaims, repairs or retains anything.
//!
//! # One classifier, two callers
//!
//! Half (b) does **not** contain a second classifier. The packet requires a
//! husk to be "retained and reported with its locator and reason by every census
//! **and by status**", and [`rundir::husk_report`] already computes exactly that
//! trichotomy — `Unstarted(BothHalves | PublicOnly(shape))` / `Retained(reason)`
//! — and is already what `src/status.rs` drives. This module is
//! `run_dir_names` → `husk_report` → act. A classifier of its own would drift
//! from the one an operator reads.
//!
//! The one thing `husk_report` cannot hand over is the deletion token: it is
//! read-only and drops the [`PrivateHalfProof`] unspent. So the proof is
//! recomputed at the deletion site, and a second answer that is not `Proven`
//! wins — it is the one adjacent to the effect.
//!
//! # One run-directory census, both write commands
//!
//! [`census_run_dirs`] is half (b) and there is exactly one of it.
//! [`startup_census`] is `upstroke run`'s caller and
//! [`super::recover::ResumeCensused::census`] is `upstroke resume`'s; the
//! difference between them is the `own_run` argument and nothing else. INV-15
//! requires pre-run husks reclaimed "at write-command start under the worktree
//! lock", and a resume is a write command — a second, read-only run-directory
//! pass on the resume path would classify and report husks that no command ever
//! reclaimed.
//!
//! # The census returns the witness
//!
//! Wrapping a census result in a witness afterwards proves *possession*, not
//! *order*: the holder had a report and a lock, in either order. So the
//! predecessor is consumed **by value** by the census call itself and the
//! ordering *is* the call: [`FreshCensused::establish`] takes a
//! [`WorktreeLocked`], performs both halves itself, and returns the witness. It
//! is not constructible from a container [`CensusComplete`], which proves half
//! (a) alone, and there is no constructor anywhere that accepts a
//! [`StartupCensus`] a caller made some other way.
//!
//! The recovery chain's own witnesses — including the one `BarrierHeld` this
//! crate has — live in [`super::recover`], because the ordering they encode is
//! that chain's. This module defines only the two the creation path needs.
//!
//! # The two rules that decide correctness
//!
//! Both are INV-15's, and both are about *not* deleting:
//!
//! * **Nothing private that carries a commit record is ever deleted**, by any
//!   census. That is not a check in this module — it is conjunct 12 of
//!   [`rundir::prove_private_half_ownership`], and it is why the only value that
//!   reaches [`rundir::remove_private_husk`] is a token this module cannot
//!   construct.
//! * **Nothing private is deleted on shape, marker parse, basename or
//!   reparse-point checks alone.** Every one of those answers is a
//!   [`HuskDisposition::Retained`], and the retain arm of [`apply`] performs no
//!   effect at all.
//!
//! # A failed reclaim is an outcome, not a refusal
//!
//! [`apply`] is infallible. Every way a `RunDir` funnel can refuse becomes a
//! [`RunDirOutcome::Unreclaimable`] naming the [`FailedStep`], and the census
//! carries on to the next directory.
//!
//! That is the same answer INV-15 and `startup_census` give everywhere else:
//! "cannot be reclaimed" is *retained and reported*, never a command-fatal
//! error. It matters most on the resume path, where this function is the whole
//! of the run-directory half. A dead run that left a provable husk whose private
//! half cannot be removed — `EACCES`, `EPERM`, `EBUSY`, or on Windows any
//! still-open handle — used to make `upstroke resume <id>` fail for **every**
//! run in the repository, on every attempt, because of a different run's
//! residue. `run_dir_names` sorts ascending, so a husk id sorting before the
//! resuming run's also took the own-run stale-marker repair with it.
//!
//! The census's own run is not an exception, and that is deliberate. The only
//! effect the `own_run` licence reaches is [`rundir::remove_marker`] on a
//! Committed directory — the husk arms are gated on the run lock, which a resume
//! holds for its own directory — and a marker that outlives its repair is
//! residue, not state: nothing on the resume path reads `.creating`, the removal
//! is documented idempotent, and the next write command repairs it. A
//! `RunDir.RemoveMarker` failure is also a poor predictor of anything wider: an
//! unwritable public directory fails the log append too, with a message naming
//! the step that actually stopped the run, and a Windows handle held on the
//! marker file says nothing about the log. Refusing here would replace a precise
//! later error with an imprecise earlier one, and would put a second policy in
//! the one function whose two callers are supposed to differ in `own_run` and
//! nothing else.
//!
//! The one error [`census_run_dirs`] keeps is the opposite question: not "this
//! census could not finish one directory", which is an outcome, but "**no
//! census happened**", which no per-directory report can express. See
//! [`enumerate`]. Both halves of that policy have to be read together — a
//! reclaim that refuses is reported, and an enumeration that refuses is not
//! reportable at all.
//!
//! A caller that reports what the census did therefore reads
//! [`RunDirCensusReport::unreclaimable`] as well as
//! [`RunDirCensusReport::retained`]: the two are siblings, and a report that
//! printed only the second would hide exactly the directories an operator has to
//! act on by hand.

use std::path::{Path, PathBuf};

use crate::error::UpstrokeError;
use crate::rundir::{
    self, HuskDisposition, PrivateHalfOwnership, PrivateHalfProof, Reclaimable, RepoKey,
    RetainReason, RunDirClass, RunDirHooks, UnboundShape,
};
use crate::runner::container::GitView;
use crate::runner::container::census::{Census, CensusComplete, CensusStart, run_startup_census};
use crate::runner::container::runtime::{ContainerRuntime, OwnerLiveness};

use super::seams::TopologyHooks;

pub use witness::{FreshCensused, WorktreeLocked};

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// Everything both halves need, as one value.
///
/// **There is one `authorized_root` and both halves census it.** `R` is computed
/// read-only by recovery step (a0) before any lock is taken, and half (a)'s
/// `private_root` and half (b)'s ownership root are the same field here rather
/// than two parameters that a caller could disagree with itself about. A
/// container census under one root followed by an ownership proof under another
/// would admit over containers nobody censused; making the two one field is that
/// refusal expressed as a shape rather than as a check.
pub struct CensusInputs<'a> {
    /// The repository whose `<repo>/.upstroke/runs` is censused.
    pub repo_root: &'a Path,
    /// This repository's key. The marker and the owner record must both carry
    /// it, or the husk is a directory copied from another repository.
    pub repo_key: &'a RepoKey,
    /// The authorized private root `R`, computed read-only before any lock.
    pub authorized_root: &'a Path,
    /// This process's per-process ULID.
    pub incarnation: &'a str,
    /// The container runtime seam. Required only when an intent exists or a
    /// labeled container is discoverable.
    pub runtime: &'a dyn ContainerRuntime,
    /// Whether another run's coordinator is alive.
    pub liveness: &'a dyn OwnerLiveness,
    /// The disposable Git view seam.
    pub view: &'a dyn GitView,
}

impl std::fmt::Debug for CensusInputs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CensusInputs")
            .field("repo_root", &self.repo_root)
            .field("repo_key", &self.repo_key)
            .field("authorized_root", &self.authorized_root)
            .field("incarnation", &self.incarnation)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

/// Which of the census's four effects returned an error.
///
/// Carried by [`RunDirOutcome::Unreclaimable`], and four values rather than one
/// because the residue each leaves is different and two of them are opposite.
/// [`Self::PublicHalfAfterPrivate`] is the only failure on which a private half
/// **is** gone, and [`Self::PrivateHalf`] is the only one on which nothing about
/// the private half is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedStep {
    /// `RunDir.RemovePublicHusk` on a directory with nothing private bound.
    /// Nothing private existed by ordering, so nothing private is at risk.
    PublicHalf,
    /// `RunDir.RemovePrivateHusk` under the proof token. **The public half is
    /// deliberately left where it is**, marker and all: `.creating` is the
    /// private half's only locator, and the census does not know whether that
    /// half is still there — `remove_dir_all` is not atomic and its error is the
    /// same value whether it removed nothing, every child, or the whole tree and
    /// then failed on the way out.
    PrivateHalf,
    /// `RunDir.RemovePublicHusk` **after** the private half went through the
    /// proof-token funnel. The private half is gone; the public husk survives
    /// carrying a marker whose target is absent, which the next census reclaims
    /// public-only.
    PublicHalfAfterPrivate,
    /// `RunDir.RemoveMarker` on a committed run's stale `.creating`. The run
    /// itself is untouched; the marker is residue the next census with the lock
    /// free, or the owner's next resume, removes.
    StaleMarker,
}

impl FailedStep {
    /// Every step, as a closed set.
    pub const ALL: &'static [Self] = &[
        Self::PublicHalf,
        Self::PrivateHalf,
        Self::PublicHalfAfterPrivate,
        Self::StaleMarker,
    ];

    /// This step's name, for a report and for a test's table.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PublicHalf => "public-half",
            Self::PrivateHalf => "private-half",
            Self::PublicHalfAfterPrivate => "public-half-after-private",
            Self::StaleMarker => "stale-marker",
        }
    }

    /// The operator-facing clause: what could not be done, and what that leaves.
    #[must_use]
    pub const fn what_failed(self) -> &'static str {
        match self {
            Self::PublicHalf => {
                "the public half could not be reclaimed; no private half existed by ordering, so \
                 nothing private is at risk"
            }
            Self::PrivateHalf => {
                "the private half could not be removed, so the public directory was left in place \
                 with its marker — `.creating` is that private half's only locator, and removing \
                 it would orphan a directory no census, no `status` and no deferred \
                 `upstroke runs prune` could ever reach again"
            }
            Self::PublicHalfAfterPrivate => {
                "the private half went through the proof-token funnel and the public directory \
                 could not be removed after it, so a husk whose marker names an absent target is \
                 left, which the next census reclaims public-only"
            }
            Self::StaleMarker => {
                "the stale `.creating` marker could not be removed; the run itself is untouched \
                 and the marker is residue the next census repairs"
            }
        }
    }
}

/// What the census did with one run directory, and why.
///
/// The set is closed and mirrors `startup_census`'s own enumeration: arms (i)
/// and the target-absent half of (ii) reclaim the public half alone, arm (ii)
/// reclaims both halves under the proof, arm (iii) retains, the stale-marker
/// sentence repairs, and the held-`run.lock` sentence skips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDirOutcome {
    /// Arm (i), and the target-absent half of arm (ii): nothing private is
    /// bound, so the public half alone was reclaimed. "A bare directory or one
    /// holding only a staged `.creating.tmp` … is reclaimed (no private half
    /// exists by ordering)"; "if the marker's private target does not exist the
    /// public husk alone is reclaimed".
    ReclaimedPublicOnly(UnboundShape),
    /// Arm (ii): the bidirectional ownership proof held and `committed.json`
    /// was absent, so "the private half is deleted … the proof yields a
    /// `PrivateHalfProof` token that `RunDir.RemovePrivateHusk` alone accepts,
    /// then the public directory is removed with the marker last".
    ReclaimedBothHalves,
    /// Arm (iii): "retained and reported with its locator and reason by every
    /// census and by status". **Nothing private was deleted.**
    Retained(RetainReason),
    /// "A Committed directory still carrying `.creating` or `.creating.tmp` …
    /// has the stale marker removed when its `run.lock` is free". The run
    /// itself is untouched.
    RepairedStaleMarker,
    /// A Committed directory with no stale marker: a run, and nothing for a
    /// census to do. Reported so the census's answer is total over the runs
    /// directory rather than over the husks in it.
    Committed,
    /// "A Husk with a held `run.lock` is skipped (defense in depth …)", and the
    /// same sentence's other half for a Committed directory whose live owner
    /// "removes it in recovery step (a)".
    Skipped,
    /// The reclaim or the repair this census planned returned an error, so the
    /// directory is **retained with the error recorded** and the census carries
    /// on to the next one.
    ///
    /// Not a refusal, and that is the whole point of the arm. `startup_census`
    /// and INV-15 answer "cannot be reclaimed" with *retain and report*; the
    /// census "never establishes authority", so its failure to reclaim one
    /// directory may not withhold one from the command. Before this arm the
    /// error propagated, and one husk whose private half could not be removed —
    /// `EACCES`, `EPERM`, `EBUSY`, or on Windows any still-open handle — made
    /// `upstroke resume <id>` fail for **every** run in the repository, on every
    /// attempt, because of a different run's residue.
    ///
    /// [`super::create::Disposition::PrivateHalfRemovalFailed`] is the creator's
    /// side of the same answer, and states the same policy: "it is not a second
    /// error to report over the one that stopped the run".
    Unreclaimable {
        /// Which effect refused, and therefore what is left on disk.
        step: FailedStep,
        /// The error, as the operator sees it.
        detail: String,
    },
}

impl RunDirOutcome {
    /// Every outcome, as a closed set.
    ///
    /// The list a suite is measured against, so that an arm added later and
    /// exercised by nobody fails a count rather than passing quietly — the same
    /// device as [`RetainReason::KINDS`], and for the same reason: Rust has no
    /// reflection over variants, so [`Self::kind`]'s exhaustive match is what
    /// makes adding one here and not there impossible.
    pub const KINDS: &'static [&'static str] = &[
        "reclaimed-public-only",
        "reclaimed-both-halves",
        "retained",
        "repaired-stale-marker",
        "committed",
        "skipped",
        "unreclaimable",
    ];

    /// This outcome's kind. Exhaustive by construction.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::ReclaimedPublicOnly(_) => "reclaimed-public-only",
            Self::ReclaimedBothHalves => "reclaimed-both-halves",
            Self::Retained(_) => "retained",
            Self::RepairedStaleMarker => "repaired-stale-marker",
            Self::Committed => "committed",
            Self::Skipped => "skipped",
            Self::Unreclaimable { .. } => "unreclaimable",
        }
    }

    /// Whether a deletion **completed** for this outcome.
    ///
    /// Alethic, and [`FailedStep`] is what makes that word load-bearing. A
    /// `RunDir.RemovePublicHusk` that returned an error may have removed every
    /// entry of the directory, some of them, or none — `remove_dir_all` and this
    /// funnel's entry loop are not atomic and the error is the same value in all
    /// three cases — so [`FailedStep::PublicHalf`] answers `false` here rather
    /// than claim a reclaim that may not have happened.
    /// [`FailedStep::PublicHalfAfterPrivate`] answers `true`, because on that
    /// one the private half went through the proof-token funnel and the funnel
    /// returned `Ok`.
    #[must_use]
    pub const fn reclaimed_anything(&self) -> bool {
        matches!(
            self,
            Self::ReclaimedPublicOnly(_)
                | Self::ReclaimedBothHalves
                | Self::Unreclaimable {
                    step: FailedStep::PublicHalfAfterPrivate,
                    ..
                }
        )
    }

    /// Whether the **private** half is known to have been deleted.
    ///
    /// `startup_census`'s "nothing private is ever deleted on shape, marker
    /// parse, basename, or reparse-point checks alone" is a statement about
    /// which arm a shape reaches, and this is the predicate a test states it
    /// with — so it stays alethic. [`FailedStep::PrivateHalf`] answers `false`
    /// and [`Self::may_have_deleted_a_private_half`] is where it answers `true`.
    #[must_use]
    pub const fn deleted_a_private_half(&self) -> bool {
        matches!(
            self,
            Self::ReclaimedBothHalves
                | Self::Unreclaimable {
                    step: FailedStep::PublicHalfAfterPrivate,
                    ..
                }
        )
    }

    /// Whether the private half **may** have been deleted, in whole or in part.
    ///
    /// The epistemic sibling of [`Self::deleted_a_private_half`], and the pair
    /// exists so that "is the private half gone" and "is there residue nobody
    /// observed" are two questions with two answers.
    /// [`FailedStep::PrivateHalf`] is the arm that answers them differently:
    /// `false` to the first, because a failed `remove_dir_all` decides nothing,
    /// and `true` here, because it may have emptied the directory on the way to
    /// its error.
    #[must_use]
    pub const fn may_have_deleted_a_private_half(&self) -> bool {
        matches!(
            self,
            Self::ReclaimedBothHalves
                | Self::Unreclaimable {
                    step: FailedStep::PrivateHalf | FailedStep::PublicHalfAfterPrivate,
                    ..
                }
        )
    }
}

/// One run directory, what became of it, and the locator it recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDirEntry {
    /// The directory's basename, which is the run id a marker must agree with.
    pub run_id: String,
    /// `<repo>/.upstroke/runs/<run_id>` — where the husk is, reported whether or
    /// not a marker could be read.
    pub public: PathBuf,
    /// The private locator, exactly as [`rundir::husk_report`] reports it to
    /// `status`.
    ///
    /// `None` in three cases, and each is a decision:
    ///
    /// * there is no marker, or the marker does not parse — an unparseable
    ///   marker names no target this census is entitled to believe, and
    ///   reporting a guess would name `<R>/runs/<basename>`, the very path the
    ///   proof refused to bind;
    /// * a Committed directory, whose private half is bound by
    ///   `run_started.private_dir` and verified in recovery step (a), not by a
    ///   marker on the public half;
    /// * a directory whose `run.lock` is held, which this census does not read
    ///   the marker of at all. A skipped directory is one a live process owns,
    ///   and the packet's "reported with its locator and reason" is the
    ///   retention sentence, not this one.
    pub locator: Option<PathBuf>,
    /// What [`rundir::classify_run_dir`] answered.
    pub class: RunDirClass,
    /// What was done, and why.
    pub outcome: RunDirOutcome,
}

impl RunDirEntry {
    /// The reason this directory was retained, when it was.
    #[must_use]
    pub const fn retain_reason(&self) -> Option<&RetainReason> {
        match &self.outcome {
            RunDirOutcome::Retained(reason) => Some(reason),
            _ => None,
        }
    }

    /// Whether this is the third of `startup_census`'s three status sentences:
    /// "a possibly committed run whose public log has no valid committed first
    /// line".
    #[must_use]
    pub const fn is_possibly_committed(&self) -> bool {
        matches!(
            self.outcome,
            RunDirOutcome::Retained(RetainReason::PossiblyCommitted)
        )
    }

    /// The operator-facing sentence: what was done to this directory, and why.
    #[must_use]
    pub fn describe(&self) -> String {
        let what = match &self.outcome {
            RunDirOutcome::ReclaimedPublicOnly(shape) => format!(
                "reclaimed the public half alone ({})",
                match shape {
                    UnboundShape::Bare => "a bare directory",
                    UnboundShape::StagedMarkerOnly => "only a staged marker",
                    UnboundShape::TargetAbsent => "its recorded private half is gone",
                }
            ),
            RunDirOutcome::ReclaimedBothHalves => {
                "reclaimed the private half under the ownership proof, then the public \
                 directory with the marker last"
                    .to_owned()
            }
            RunDirOutcome::Retained(reason) => {
                format!("retained, nothing deleted: {reason}")
            }
            RunDirOutcome::RepairedStaleMarker => {
                "a committed run: removed its stale `.creating` marker".to_owned()
            }
            RunDirOutcome::Committed => "a committed run: nothing to do".to_owned(),
            RunDirOutcome::Skipped => {
                "skipped: its `run.lock` is held by a live process".to_owned()
            }
            RunDirOutcome::Unreclaimable { step, detail } => format!(
                "retained with the error recorded, and the census carried on: {} ({detail})",
                step.what_failed()
            ),
        };
        match &self.locator {
            Some(locator) => format!(
                "{} at {}: {what} (private locator {})",
                self.run_id,
                self.public.display(),
                locator.display()
            ),
            None => format!("{} at {}: {what}", self.run_id, self.public.display()),
        }
    }
}

/// What half (b) found and did, one entry per directory, in run-id order.
///
/// The census's answer is **total** over `<repo>/.upstroke/runs`: every entry
/// [`rundir::run_dir_names`] returns has exactly one [`RunDirEntry`] here.
/// `startup_census` requires every entry to classify before the write command
/// proceeds, and a report that only listed the directories something happened to
/// could not be read as evidence of that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDirCensusReport {
    entries: Vec<RunDirEntry>,
}

impl RunDirCensusReport {
    /// Every directory censused, in run-id order.
    #[must_use]
    pub fn entries(&self) -> &[RunDirEntry] {
        &self.entries
    }

    /// The entry for one run id, if the census saw that directory.
    #[must_use]
    pub fn of(&self, run_id: &str) -> Option<&RunDirEntry> {
        self.entries.iter().find(|entry| entry.run_id == run_id)
    }

    /// Everything reclaimed, in either shape.
    #[must_use]
    pub fn reclaimed(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| outcome.reclaimed_anything())
    }

    /// Every committed run whose stale marker was removed.
    #[must_use]
    pub fn repaired(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| matches!(outcome, RunDirOutcome::RepairedStaleMarker))
    }

    /// Everything retained and reported, **including** the possibly committed.
    ///
    /// A possibly committed husk *is* retained — `startup_census` puts it in the
    /// same arm (iii) as every other retention and gives it a `RetainReason` of
    /// its own. [`Self::possibly_committed`] is the subset, not a sibling, and
    /// exists because the status trichotomy names it as its own sentence.
    #[must_use]
    pub fn retained(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| matches!(outcome, RunDirOutcome::Retained(_)))
    }

    /// The retained husks whose private half carries a commit record.
    #[must_use]
    pub fn possibly_committed(&self) -> Vec<&RunDirEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.is_possibly_committed())
            .collect()
    }

    /// Everything skipped because a live process holds its `run.lock`.
    #[must_use]
    pub fn skipped(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| matches!(outcome, RunDirOutcome::Skipped))
    }

    /// Every directory whose planned reclaim or repair returned an error.
    ///
    /// A sibling of [`Self::retained`] rather than a subset of it: these carry
    /// no [`RetainReason`], because nothing was *classified* as unremovable —
    /// the plan was to remove and the funnel refused. A caller that wants
    /// "everything still on disk after this census" asks both.
    #[must_use]
    pub fn unreclaimable(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| matches!(outcome, RunDirOutcome::Unreclaimable { .. }))
    }

    fn with(&self, keep: impl Fn(&RunDirOutcome) -> bool) -> Vec<&RunDirEntry> {
        self.entries
            .iter()
            .filter(|entry| keep(&entry.outcome))
            .collect()
    }
}

/// Both halves' results, and nothing else.
///
/// The fields are private and this module mints the only value, so a caller
/// holding a [`CensusComplete`] — which proves half (a) alone — cannot present
/// evidence of half (b). The witnesses hold one of these; they do not hold a
/// `CensusComplete`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupCensus {
    containers: CensusComplete,
    run_dirs: RunDirCensusReport,
}

impl StartupCensus {
    /// Half (a): what the global container reclaim found and did.
    #[must_use]
    pub const fn containers(&self) -> &CensusComplete {
        &self.containers
    }

    /// Half (b): what the run-directory census found and did.
    #[must_use]
    pub const fn run_dirs(&self) -> &RunDirCensusReport {
        &self.run_dirs
    }

    /// Both halves, for a caller that owns the value.
    #[must_use]
    #[allow(dead_code)] // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
    pub fn into_parts(self) -> (CensusComplete, RunDirCensusReport) {
        (self.containers, self.run_dirs)
    }
}

// ---------------------------------------------------------------------------
// The entry point
// ---------------------------------------------------------------------------

/// `upstroke run`'s census: the worktree lock is held, no run lock is.
///
/// Consumes the [`WorktreeLocked`] witness **by value** and hands the lock back
/// inside the [`FreshCensused`] it returns: `run_creation` holds the physical
/// worktree lock "across the startup census and the whole run", so a census that
/// dropped it would end the exclusion it was performed under. The order is the
/// call — there is no way to reach a `FreshCensused` without having held the
/// lock first, and no way to hold one afterwards without the census having run.
///
/// A fresh run has no own-run arm: `startup_census` puts the census "before …
/// run-lock acquisition for a fresh run", so every directory in the runs tree
/// belongs to somebody else and the held-`run.lock` rule governs all of them.
/// `upstroke resume`'s census is [`super::recover::ResumeCensused::census`],
/// which passes its own run id to the same [`census_run_dirs`].
///
/// # Errors
///
/// Whatever half (a) refuses with — an unreachable runtime with intents present,
/// an intent naming this process's own incarnation, a labeled container whose
/// ownership cannot be established, a dead container that cannot be observed
/// terminated — and [`UpstrokeError::Filesystem`] from half (b) when the runs directory
/// exists and cannot be enumerated. A reclaim or a repair that fails in half (b)
/// is a [`RunDirOutcome::Unreclaimable`] entry and not an error.
pub fn startup_census(
    locked: WorktreeLocked,
    hooks: &mut dyn TopologyHooks,
    inputs: &CensusInputs<'_>,
) -> Result<FreshCensused, UpstrokeError> {
    FreshCensused::establish(locked, hooks, inputs)
}

/// (a) then (b). The order is the packet's and is not an implementation detail.
///
/// Half (a) reclaims a husk's probe containers, and half (b) then deletes the
/// husk that owned them; run the other way round and the census would remove the
/// intent namespace's own evidence of who a running container belonged to.
fn both_halves(
    hooks: &mut dyn TopologyHooks,
    inputs: &CensusInputs<'_>,
    start: &CensusStart,
) -> Result<StartupCensus, UpstrokeError> {
    let census = Census {
        private_root: inputs.authorized_root,
        start,
        runtime: inputs.runtime,
        liveness: inputs.liveness,
        view: inputs.view,
    };
    // (a). Its own contract is "refuse before any effect" if any container
    // classification refuses, so an `Err` here means nothing was reclaimed and
    // half (b) has not touched the disk either.
    let containers = run_startup_census(hooks.container(), &census)?;
    let run_dirs = census_run_dirs(hooks.rundir(), inputs, start.own_run())?;
    Ok(StartupCensus {
        containers,
        run_dirs,
    })
}

// ---------------------------------------------------------------------------
// Half (b)
// ---------------------------------------------------------------------------

/// The run-directory census over `<repo>/.upstroke/runs`.
///
/// **Every write command calls this one function.** `upstroke run` reaches it
/// through [`both_halves`] with no `own_run`; `upstroke resume` reaches it from
/// [`super::recover::ResumeCensused::census`] with its own run id. INV-15's
/// "reclaims pre-run husks at write-command start under the worktree lock" is
/// not satisfied by a second pass that classifies and reports, so there is no
/// second pass: the two callers differ in `own_run` alone.
///
/// Two phases, and the split is deliberate. Every classification and every
/// ownership proof is read-only and completes **before** the first deletion, so
/// a census whose plan is wrong about one directory has not already reclaimed
/// another on behalf of a command that then refused — the same shape half (a)
/// states for itself ("step 4 completes before step 5 begins on purpose"). The
/// worktree lock is held across both phases, so nothing else can move a
/// directory between them.
///
/// **Phase 2 never stops.** A funnel that refuses on one directory is that
/// directory's [`RunDirOutcome::Unreclaimable`], not the command's error: see
/// that arm. So the one error this function has left is the one that means *no
/// census happened at all* — the runs directory could not be enumerated.
///
/// # Errors
///
/// [`UpstrokeError::Filesystem`] when `<repo>/.upstroke/runs` exists and cannot be read,
/// or an entry in it cannot be. A census that reported success having scanned
/// nothing would convert INV-15's "reclaims pre-run husks at write-command
/// start" from an unproven claim into an apparently-proven one. Phase 1 is
/// otherwise read-only and cannot fail.
pub(crate) fn census_run_dirs(
    hooks: &mut dyn RunDirHooks,
    inputs: &CensusInputs<'_>,
    own_run: Option<&str>,
) -> Result<RunDirCensusReport, UpstrokeError> {
    let scanned: Vec<Scanned> = enumerate(inputs.repo_root)?
        .iter()
        .map(|run_id| scan(run_id, inputs, own_run))
        .collect();

    let mut entries = Vec::with_capacity(scanned.len());
    for item in scanned {
        let outcome = apply(hooks, &item.public, item.plan);
        entries.push(RunDirEntry {
            run_id: item.run_id,
            public: item.public,
            locator: item.locator,
            class: item.class,
            outcome,
        });
    }
    Ok(RunDirCensusReport { entries })
}

/// Which run ids this census walks, in run-id order.
///
/// [`rundir::run_ids`], with its failure kept as a failure. "There is nothing
/// there" and "this process could not read it" used to be one value — the
/// enumeration answered an empty vector for a `read_dir` that failed — and only
/// the first is a census: the second reports success having scanned nothing,
/// which turns INV-15's "reclaims pre-run husks at write-command start" from an
/// unproven claim into an apparently-proven one. This function used to probe an
/// empty answer with a second `read_dir` to tell the two apart; the enumeration
/// says which now, and the probe is gone.
///
/// Refusing rather than reporting is deliberate, and it is not the refusal
/// [`RunDirOutcome::Unreclaimable`] removes: that one is "this census could not
/// finish one directory", which retains and reports; this one is "no census
/// happened", which no report can express. `status` and `rundir::list_husks`
/// walk the same enumeration, so the census cannot drift from what an operator
/// reads; what they do with its failure is theirs and is stated there.
fn enumerate(repo_root: &Path) -> Result<Vec<String>, UpstrokeError> {
    // The ID view skips names that are not UTF-8; worktree acquisition has
    // already checked cleanup leases using every exact filesystem name.
    // Only proven runs-root absence is empty. Open, entry, and metadata
    // failures propagate before the census can apply any plan.
    rundir::run_ids(repo_root)
}

/// One directory, classified and decided. Read-only from end to end.
#[derive(Debug)]
struct Scanned {
    run_id: String,
    public: PathBuf,
    locator: Option<PathBuf>,
    class: RunDirClass,
    plan: Planned,
}

/// What the census intends to do with one directory.
///
/// [`Planned::ReclaimBothHalves`] carries the proof token, which is not `Clone`
/// and is spent by [`rundir::remove_private_husk`]. Holding it here rather than
/// re-proving in phase 2 is what makes "the proof that authorized this deletion"
/// and "the proof this census computed" the same object.
#[derive(Debug)]
enum Planned {
    ReclaimPublicOnly(UnboundShape),
    ReclaimBothHalves(PrivateHalfProof),
    Retain(RetainReason),
    RepairStaleMarker,
    Committed,
    Skip,
}

/// Classify one directory and decide, read-only.
fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: Option<&str>) -> Scanned {
    let public = rundir::public_dir(inputs.repo_root, run_id);
    let class = rundir::classify_run_dir(&public);
    // `is_running` is the read-only probe: on Unix `F_GETLK` asks who holds the
    // lock without taking it, and an absent lock file means the run never
    // started. "A Husk whose `run.lock` is **free or absent** is handled by
    // shape and proof."
    let lock_held = rundir::is_running(&public);

    if class == RunDirClass::Committed {
        // The own-run exception, stated twice by the packet: recovery step (a1)'s
        // census covers "this run's own stale marker, **which the owner removes
        // here**", and the stale-marker sentence's "otherwise its live owner
        // removes it in recovery step (a)" is the same removal from the other
        // side. It licenses the marker repair and nothing else.
        let own = own_run == Some(run_id);
        let plan = if !stale_marker_present(&public) {
            Planned::Committed
        } else if lock_held && !own {
            Planned::Skip
        } else {
            Planned::RepairStaleMarker
        };
        return Scanned {
            run_id: run_id.to_owned(),
            public,
            // A committed run's private half is bound by
            // `run_started.private_dir`, which recovery step (a) verifies. A
            // marker on it is stale residue, not a binding to report.
            locator: None,
            class,
            plan,
        };
    }

    // Every husk arm is gated on the lock alone. A husk whose lock is held is
    // skipped whoever holds it, this process included: under the worktree lock
    // no live creator can exist in this worktree, so a held lock on a husk is
    // either another repository's process or this resume's own run with a
    // damaged log — and neither is a directory a census may delete.
    if lock_held {
        return Scanned {
            run_id: run_id.to_owned(),
            public,
            locator: None,
            class,
            plan: Planned::Skip,
        };
    }

    // The one classifier. `status` drives the same call on the same directory
    // and gets the same locator and the same reason.
    let report = rundir::husk_report(
        inputs.repo_root,
        run_id,
        inputs.repo_key,
        inputs.authorized_root,
    );
    let plan = match report.disposition {
        HuskDisposition::Unstarted(Reclaimable::PublicOnly(shape)) => {
            Planned::ReclaimPublicOnly(shape)
        }
        // `husk_report` is read-only and drops its token unspent, so the proof
        // is recomputed here to mint one. A second answer that is not `Proven`
        // **wins**: it is the one adjacent to the deletion, and every way it can
        // differ — a commit record that has since appeared, an owner record that
        // has since been rewritten — is a reason not to delete.
        HuskDisposition::Unstarted(Reclaimable::BothHalves) => {
            match rundir::prove_private_half_ownership(
                &report.public,
                inputs.repo_key,
                inputs.authorized_root,
            ) {
                PrivateHalfOwnership::Proven(proof) => Planned::ReclaimBothHalves(proof),
                PrivateHalfOwnership::NothingBound(shape) => Planned::ReclaimPublicOnly(shape),
                PrivateHalfOwnership::Retained(reason) => Planned::Retain(reason),
            }
        }
        HuskDisposition::Retained(reason) => Planned::Retain(reason),
    };
    Scanned {
        run_id: report.run_id,
        public: report.public,
        locator: report.locator,
        class,
        plan,
    }
}

/// Perform one plan. The only place in this module that has an effect.
///
/// **Infallible, by type.** Every way a funnel can refuse is a
/// [`RunDirOutcome::Unreclaimable`] naming the [`FailedStep`], so one
/// directory's residue cannot end the command that censused it. The retain arm
/// keeps a second guarantee it had before: it reaches no funnel at all, which is
/// now visible in the signature as well as in the body.
fn apply(hooks: &mut dyn RunDirHooks, public: &Path, plan: Planned) -> RunDirOutcome {
    match plan {
        Planned::ReclaimPublicOnly(shape) => match rundir::remove_public_husk(public, hooks) {
            Ok(()) => RunDirOutcome::ReclaimedPublicOnly(shape),
            Err(error) => unreclaimable(FailedStep::PublicHalf, &error),
        },
        Planned::ReclaimBothHalves(proof) => {
            // The order is load-bearing: "the census reclaims the private half
            // through the proof-token funnel, **then** the public directory with
            // the marker last … so a kill mid-census leaves a husk the next
            // census completes". Reversed, a kill between the two would leave a
            // private half no marker names and no census can ever prove.
            //
            // And **the public half goes only if the private one went**: a
            // private removal that returned an error returns here rather than
            // falling through, because `remove_public_husk` deletes `.creating`
            // with the directory and that marker is the private half's only
            // locator. The creator states the identical rule at its own
            // `RunDir.RemovePrivateHusk`; this is the census's half of it.
            if let Err(error) = rundir::remove_private_husk(proof, hooks) {
                return unreclaimable(FailedStep::PrivateHalf, &error);
            }
            match rundir::remove_public_husk(public, hooks) {
                Ok(()) => RunDirOutcome::ReclaimedBothHalves,
                Err(error) => unreclaimable(FailedStep::PublicHalfAfterPrivate, &error),
            }
        }
        // Arm (iii). No effect, by construction rather than by discipline:
        // there is no funnel call on this path at all.
        Planned::Retain(reason) => RunDirOutcome::Retained(reason),
        Planned::RepairStaleMarker => match rundir::remove_marker(public, hooks) {
            Ok(()) => RunDirOutcome::RepairedStaleMarker,
            Err(error) => unreclaimable(FailedStep::StaleMarker, &error),
        },
        Planned::Committed => RunDirOutcome::Committed,
        Planned::Skip => RunDirOutcome::Skipped,
    }
}

/// One failed effect, as the outcome that replaces it.
fn unreclaimable(step: FailedStep, error: &UpstrokeError) -> RunDirOutcome {
    RunDirOutcome::Unreclaimable {
        step,
        detail: error.to_string(),
    }
}

/// Whether a committed run still carries the marker its creator publishes at P1.
///
/// Both spellings: "a Committed directory still carrying `.creating` **or**
/// `.creating.tmp`". `symlink_metadata` rather than `exists`, so a marker that
/// is a dangling link is still a marker to remove rather than a file that reads
/// as absent.
fn stale_marker_present(public: &Path) -> bool {
    std::fs::symlink_metadata(public.join(rundir::MARKER)).is_ok()
        || std::fs::symlink_metadata(public.join(rundir::MARKER_STAGED)).is_ok()
}

// ---------------------------------------------------------------------------
// The witnesses
// ---------------------------------------------------------------------------

/// The two witnesses the creation path consumes and mints, **each alone in its
/// own module**.
///
/// A nested module per witness, and not one module holding both: an item private
/// to a module is visible to that module **and its descendants**, so two types
/// sharing one module could each build the other out of its parts, and every
/// function in `startup` — its own tests included — could mint either from
/// hand-built fields. That is a naming convention, not a type. Siblings see only
/// what is `pub`, which here is the constructor and the accessors. The same rule
/// [`super::super::recover::chain`] states for the recovery order's seven, and
/// for the same reason.
///
/// Neither derives `Clone`, `Copy` or `Default`: a `Clone` would let one census
/// authorise two, and a `Default` would mint evidence out of nothing. The same
/// device `rundir::ownership` uses for [`PrivateHalfProof`].
///
/// **Ownership note.** [`WorktreeLocked`] is a *predecessor* of this module's
/// census — the creation chain's third link — and is defined here because
/// [`super::startup_census`] cannot be typed without it; the lane that owns
/// `prelock.rs`/`create.rs` extends it with whatever else its steps carry
/// forward. The recovery chain's predecessors are **not** restated here.
/// `BarrierHeld` in particular belongs to [`super::super::recover`], where its
/// constructor consumes a `RecordsVerified` that consumed a `LocksHeld` that
/// consumed a `RootDerived`: a second one defined beside this census would be a
/// barrier reachable with no locks, no records and no bound run id, which is
/// exactly the hole it was.
mod witness {
    pub use fresh::FreshCensused;
    pub use locked::WorktreeLocked;

    /// The creation chain's third link.
    mod locked {
        use crate::rundir::WorktreeLock;

        /// The physical worktree lock is held.
        ///
        /// Holds the lock **by value**, so possessing the witness *is* holding
        /// the lock: `run_creation` takes it "across the startup census and the
        /// whole run", and a witness that merely remembered an acquisition would
        /// outlive the exclusion it claims.
        #[derive(Debug)]
        pub struct WorktreeLocked {
            #[allow(dead_code)]
            // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
            lock: WorktreeLock,
        }

        impl WorktreeLocked {
            /// The only constructor. Takes the lock by value.
            #[must_use]
            pub fn from(lock: WorktreeLock) -> Self {
                Self { lock }
            }

            /// The lock this witness is holding.
            #[must_use]
            #[allow(dead_code)] // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
            pub const fn lock(&self) -> &WorktreeLock {
                &self.lock
            }
        }
    }

    /// A fresh run's completed startup census.
    mod fresh {
        use super::super::{CensusInputs, StartupCensus, both_halves};
        use super::locked::WorktreeLocked;
        use crate::engine::topology::seams::TopologyHooks;
        use crate::error::UpstrokeError;
        use crate::runner::container::census::CensusStart;

        /// A fresh run's startup census completed, under the worktree lock.
        ///
        /// **The census returns the witness; it does not wrap one.**
        /// [`Self::establish`] is the only constructor and it runs both halves
        /// itself, so there is no signature anywhere that accepts a
        /// [`StartupCensus`] a caller obtained some other way — a hand-built one
        /// whose run-directory half never ran included. Not constructible from a
        /// container `CensusComplete` either, which proves half (a) alone.
        #[derive(Debug)]
        pub struct FreshCensused {
            locked: WorktreeLocked,
            census: StartupCensus,
        }

        impl FreshCensused {
            /// The only constructor: both halves, under the lock it consumes.
            ///
            /// `pub(in …startup)` rather than `pub`, because
            /// [`super::super::startup_census`] is the entry point and a second
            /// public one would be a second answer to "what did `upstroke run`
            /// census".
            ///
            /// # Errors
            ///
            /// As [`super::super::startup_census`].
            pub(in crate::engine::topology::startup) fn establish(
                locked: WorktreeLocked,
                hooks: &mut dyn TopologyHooks,
                inputs: &CensusInputs<'_>,
            ) -> Result<Self, UpstrokeError> {
                let start = CensusStart::FreshRun {
                    incarnation: inputs.incarnation.to_owned(),
                };
                let census = both_halves(hooks, inputs, &start)?;
                Ok(Self { locked, census })
            }

            /// Both halves' results.
            #[must_use]
            pub const fn census(&self) -> &StartupCensus {
                &self.census
            }

            /// The worktree lock, still held.
            #[must_use]
            #[allow(dead_code)] // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
            pub const fn locked(&self) -> &WorktreeLocked {
                &self.locked
            }

            /// Give the lock and the report back to a caller that owns the
            /// witness.
            #[must_use]
            pub fn into_parts(self) -> (WorktreeLocked, StartupCensus) {
                (self.locked, self.census)
            }
        }
    }
}

#[cfg(test)]
mod tests;
