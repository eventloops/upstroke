//! The fresh-process recovery order, (a0) through (i), as a chain of witnesses.
//!
//! `decisions.sequential_substrate.recovery_order` is "one checked fresh-process
//! order matching current practice (recovery events precede `run_resumed`)", and
//! O18 states the four orderings a resume has to get right:
//!
//! > on resume the private root is derived read-only **before any lock**, the
//! > owner and commit records are verified **before any private write**, the
//! > stable-prefix barrier holds **before** the census's fold-derived reclaim,
//! > before any promotion, cleanup, admission, or report, and **before any
//! > recovery event** (a failed sync, unstable reread, or replay refusal ends
//! > the command with nothing done), and the recorded Runner is rebuilt
//! > (inspection) and its `RunnerPreflight` executed (spawns) **after the
//! > census and before any recovery event**.
//!
//! # Why the order is a type and not a function body
//!
//! Of 29 classified findings across PR3–PR6, `wrong_internal_assumption` is
//! 48.3% — three times `wrong_external_fact`. Orderings are where this
//! project's defects live, and a recovery order written as eight statements in
//! one `fn` is an ordering a later edit can reorder silently and no gate will
//! notice. So each step produces a **witness**, and the next step's constructor
//! **consumes** it by value:
//!
//! ```text
//! RootDerived        (a0)  read-only, before any lock
//! LocksHeld          (a)   worktree lock then run lock; a reaper hold refuses
//! RecordsVerified    (a)   owner.json and committed.json, before any private write
//! BarrierHeld        (a1)  the stable-prefix barrier
//! ResumeCensused     (a)   the census, after the barrier
//! RunnerRebuilt      (c)   inspection refusals, before any spawn
//! PreflightCertified (c)   the shell probe then the agent probes
//! ```
//!
//! Each lives **alone in its own module** with private fields, derives no
//! `Clone`, `Copy` or `Default`, and has exactly one constructor. Rust privacy
//! is scoped to the defining module and its descendants, so a sibling witness
//! cannot build another out of its parts, and neither can [`chain`] itself.
//! "From X only" written in a comment is not a type; this is.
//!
//! Every emitter of a recovery event — steps (d) through (g) — takes
//! `&PreflightCertified`, and [`run_resumed`] at (h) **consumes** it by value,
//! so nothing after the resume can present one.
//!
//! # What this slice does *not* do
//!
//! **Step (b) is "terminal finalization then refuse continuation", and PR7
//! implements the refusal only.** `RunDir.WriteReport` carries `fault_row:
//! t_finalize`, which is not one of this slice's eleven rows; a lane that
//! finalized here would write an out-of-row effect with no fault coverage.
//! [`refuse_if_finished`] is the refusal, and it is the whole of PR7's (b).
//!
//! Step (f) is `checkpoint_refusals` territory for the same reason: "an
//! intermediate build refuses, before any append, any operation whose terminals
//! it does not implement (PR7: integration and run end beyond refusal)". A
//! prefix that leaves a promotion or an integration transaction unresolved is
//! refused rather than completed.
//!
//! # Where the P7/P8 integration-ref repair sits, and why
//!
//! `transaction_fault_matrix[T-RUNSTART].resume_action` gives the resume one
//! step this order would otherwise not have:
//!
//! > **P7/P8: create the ref zero-old at the recorded base if absent; if
//! > present == base continue (no spend repeats)**
//!
//! [`ensure_recorded_integration_ref`] is that step. Its body is
//! [`super::create::ensure_integration_ref`] — **P8's own body, called, not
//! copied**: two implementations of "if present == base continue" would be two
//! places for a run killed between P6 and P8 to be treated differently from one
//! that was not, which is the duplication that function exists to prevent.
//! What this module adds is the two arguments, and it takes them from the
//! record `RootDerived` resolved and `RecordsVerified` authenticated —
//! `run_started(4).integration_ref` and `run_started(4).base_sha` — never from
//! today's configuration.
//!
//! Its position is between step (f)'s [`refuse_unimplemented_terminals`] and
//! step (d)'s first append, and every bound on it is a separate clause:
//!
//! * **After (a1).** It is a durable effect on a repository ref. O18 puts the
//!   stable-prefix barrier before the census's fold-derived reclaim, before any
//!   promotion, cleanup, admission or report, and before any recovery event —
//!   that is, before every durable thing a resume derives from the record. A
//!   ref creation is such a thing, so it is not exempt.
//! * **After (b).** [`refuse_if_finished`] refuses a Complete or Halted run,
//!   and publishing a finished run's integration ref is continuing it.
//! * **After (c).** The repository is touched only once the recorded Runner has
//!   been rebuilt by inspection and its probes have answered, so a resume that
//!   cannot run at all leaves the object store exactly as it found it.
//! * **After (f).** This is the bound that is not merely tidy.
//!   [`refuse_unimplemented_terminals`] refuses a proven prefix that leaves an
//!   integration transaction unresolved, and an unresolved integration
//!   transaction is precisely the state in which the integration ref may be
//!   mid-move. The ref of such a run can still be *at* the recorded base — the
//!   CAS has not run yet — and "present == base continue" would then silently
//!   adopt a ref under a transaction this build cannot resolve. That case is
//!   the one the step's own refusals do not catch, so the checkpoint refusal
//!   runs first.
//! * **Before (d).** The step can refuse: a ref at another SHA, a symbolic ref,
//!   a ref checked out in a worktree. A refusal after `attempt_interrupted`,
//!   `generation_closed` and `run_resumed` is a resume half-performed — the
//!   epoch incremented and the generations closed for a command that then
//!   failed — and the next resume would append the same set again before
//!   refusing again. [`refuse_unimplemented_terminals`] gives the identical
//!   reason for its own position: a refusal after two appends is not "before
//!   any append".
//!
//! O15 is "run_started before integration ref", and on a resume it is satisfied
//! by construction rather than by placement: `run_started(4)` is the committed
//! first line (a0) read before this order began. Nothing here can put the ref
//! first.
//!
//! It is **not** a recovery event. It appends nothing, so
//! [`refuse_unimplemented_terminals`] does not gate it as an operation whose
//! terminal is missing, and it needs no terminal of its own — the effect either
//! happened or did not, and the next resume decides which by looking at the ref.
//!
//! # Nothing here is a production path
//!
//! `MAX_READABLE_SCHEMA` is 3 and `TOPOLOGY_ACTIVATION` is `Inactive`, so
//! [`RootDerived::derive`] refuses every schema-4 log in a released binary.
//! The reader ceiling is the seam a test raises — see [`RootDerived::derive`]
//! and [`RootDerived::derive_with`] beside it.
//!
//! (That sentence deliberately does not spell the test-configuration attribute
//! out. Two source censuses in this tree cut a file at its first occurrence of
//! that attribute to find the production half, and a *prose* occurrence in a
//! module comment cuts the whole file out of the scan — silently, and in the
//! direction that makes the census pass.)

use std::path::Path;

use crate::config::RunnerSelection;
use crate::error::UpstrokeError;
use crate::events::AttemptRecord;
use crate::events::RunOutcome;
use crate::rundir::{RepoKey, RunLock, WorktreeLock};
use crate::runner::container::GitView;
use crate::runner::container::resolve::RunnerPreflight;
use crate::runner::container::runtime::{ContainerRuntime, OwnerLiveness};
use crate::topology::events::{
    AttemptInterrupted4, AttemptNumber, CandidateLeaseEffect, CandidatePrepared, CommitSha,
    GenerationCloseReason, GenerationClosed, GenerationId, IncarnationId, LeaseDisposition,
    RunResumed4, RunStarted4, TopologyEvent, TopologyEventBody,
};
use crate::topology::fold::{FrozenInputs, GenerationClass, TopologyFold};
use crate::topology::leases::GenerationLease;
use crate::topology::registry::TaskKey;
use crate::workspace_manager::WorkspaceManager;

use super::create::{IntegrationRefs, ensure_integration_ref};
use super::dispatch::{OpenGeneration, Reuse, resume_open_no_attempt, task_slot};
use super::emit::{EmitState, RunIdentity};
use super::identity::{InvocationLedger, Reservations};
use super::seams::{TimeSource, TopologyHooks};

pub use chain::{
    BarrierHeld, CensusSeams, LocksHeld, PreflightCertified, RecordsVerified, ResumeCensused,
    RootDerived, RunnerRebuilt,
};

// ---------------------------------------------------------------------------
// The chain
// ---------------------------------------------------------------------------

/// The seven witnesses, each alone in its own module.
///
/// A nested module per witness, and not one module holding seven types: an item
/// private to a module is visible to that module **and its descendants**, so
/// seven types in one module could each build the others out of their parts and
/// the chain would be a naming convention again. Siblings see only what is
/// `pub`, which here is the constructor and the accessors.
pub mod chain {
    pub use barrier::BarrierHeld;
    pub use censused::{CensusSeams, ResumeCensused};
    pub use certified::PreflightCertified;
    pub use locks::LocksHeld;
    pub use rebuilt::RunnerRebuilt;
    pub use records::RecordsVerified;
    pub use root::RootDerived;

    // -- (a0) ---------------------------------------------------------------

    /// Recovery step (a0): everything a resume decides **before any lock**.
    pub mod root {
        use std::path::{Path, PathBuf};

        use crate::error::UpstrokeError;
        use crate::rundir;
        use crate::topology::events::{RunStarted4, TopologyEvent, TopologyEventBody};
        use crate::topology::schema::{
            MAX_READABLE_SCHEMA, ReaderSelection, probe_header, select_for_schema,
        };

        /// The run this resume is about, and the private root it is authorized
        /// to touch — derived read-only, with no lock held.
        ///
        /// `recovery_order` (a0): "resolve the run id among Committed
        /// directories (readers by commitment), probe the header of the
        /// committed first line, select the engine by schema, derive the
        /// authorized private root R from `run_started.private_dir` (refusing a
        /// locator of any other shape than `<root>/runs/<run_id>`), compare an
        /// explicit `--private-root` (refusing a mismatch naming both roots) —
        /// **every refusal here precedes `Lock.AcquireWorktree`, so no R17 hold
        /// is taken and no R25 lock file is created**".
        ///
        /// The R25 clause is why this is a separate step rather than the first
        /// paragraph of the one that locks: `Lock.AcquireWorktree`'s funnel
        /// opens the lock file with `create(true)`, so *reaching* the
        /// acquisition creates the repository-scoped file even when the hold
        /// then fails. A refusal that has not reached it leaves no file at all.
        #[derive(Debug)]
        pub struct RootDerived {
            run_id: String,
            public_dir: PathBuf,
            private_root: PathBuf,
            private_dir: PathBuf,
            reader: ReaderSelection,
            /// The committed first line, **without** its newline — the bytes
            /// `committed.json.run_started_sha256` names.
            first_line: Vec<u8>,
            started: Box<RunStarted4>,
        }

        impl RootDerived {
            /// Step (a0) against this binary's reader ceiling.
            ///
            /// Production's ceiling is 3 (`TOPOLOGY_ACTIVATION` is `Inactive`),
            /// so a released binary refuses every schema-4 log here and no
            /// production path reaches the rest of this file. That is
            /// `pr_sequence[8].production_effect: none` expressed as a
            /// refusal rather than as a promise.
            ///
            /// # Errors
            ///
            /// [`UpstrokeError::Refused`] for an unresolvable run id, a log
            /// whose header does not select the topology reader, a first line
            /// that is not a `run_started`, a recorded locator of any shape
            /// other than `<root>/runs/<run_id>`, or an explicit
            /// `--private-root` naming a different root.
            /// [`UpstrokeError::Io`] when the log cannot be read.
            pub fn derive(
                repo_root: &Path,
                wanted_run_id: &str,
                explicit_private_root: Option<&Path>,
            ) -> Result<Self, UpstrokeError> {
                Self::derive_with(
                    repo_root,
                    wanted_run_id,
                    explicit_private_root,
                    MAX_READABLE_SCHEMA,
                )
            }

            /// [`Self::derive`] against an explicit reader ceiling.
            ///
            /// **This is the test seam, and it is the tree's own shape**:
            /// `schema::select_reader_with(bytes, ceiling)` exists for exactly
            /// this reason — "every decision that depends on the ceiling
            /// already reads it through `select_reader_with`, so nothing has to
            /// be rewritten when it moves". `pub(crate)` rather than public
            /// because raising the ceiling is not something a caller outside
            /// this crate may do.
            ///
            /// # Errors
            ///
            /// As [`Self::derive`].
            pub(crate) fn derive_with(
                repo_root: &Path,
                wanted_run_id: &str,
                explicit_private_root: Option<&Path>,
                ceiling: u32,
            ) -> Result<Self, UpstrokeError> {
                // (1) The run id, among Committed directories. `resolve_run_id`
                // reads `list_runs`, which is the by-commitment view: a husk is
                // refused here with the sentence that names it, which is
                // `refusal_condition`'s "resume of a husk id".
                let run_id = rundir::resolve_run_id(repo_root, wanted_run_id)?;
                let public_dir = rundir::public_dir(repo_root, &run_id);
                let log = public_dir.join(rundir::EVENT_LOG);
                let bytes =
                    crate::util::read_file_bounded(&log).map_err(|source| UpstrokeError::Io {
                        path: log.clone(),
                        source,
                    })?;

                // (2) The header, and the engine selection by schema.
                let header = probe_header(&bytes).map_err(|refusal| UpstrokeError::Refused {
                    message: format!("{} ({})", refusal, log.display()),
                })?;
                let reader = select_for_schema(header.schema, ceiling).map_err(|refusal| {
                    UpstrokeError::Refused {
                        message: format!("{} ({})", refusal, log.display()),
                    }
                })?;
                if reader != ReaderSelection::Topology {
                    return Err(UpstrokeError::Refused {
                        message: format!(
                            "run `{run_id}` is written in schema {}, which the legacy sequential \
                             engine drives; `selection` is \"schemas 1-3 always run the legacy \
                             engine; schema 4 always runs TopologyRun\", and this is the topology \
                             recovery order.",
                            header.schema
                        ),
                    });
                }

                // (3) The first line as the record, for its `private_dir`, its
                // `incarnation` and its `runner`. Read-only, and re-proven at
                // (a1): the barrier compares the reread first line's digest
                // against `committed.json`, so nothing here is trusted past the
                // barrier.
                let end = bytes.iter().position(|byte| *byte == b'\n').unwrap_or(0);
                let first_line = bytes[..end].to_vec();
                let event: TopologyEvent =
                    serde_json::from_slice(&first_line).map_err(|error| {
                        UpstrokeError::Refused {
                            message: format!(
                                "the committed first line of {} is not a topology event ({error})",
                                log.display()
                            ),
                        }
                    })?;
                let TopologyEventBody::RunStarted { data: started } = event.body else {
                    return Err(UpstrokeError::Refused {
                        message: format!(
                            "the committed first line of {} is not a `run_started`",
                            log.display()
                        ),
                    });
                };

                // (4) The authorized private root, from the record's locator.
                let private_dir = PathBuf::from(&started.private_dir);
                let private_root = authorized_root(&private_dir, &run_id)?;

                // (5) An explicit `--private-root` that names a different root
                // refuses, **naming both**.
                if let Some(explicit) = explicit_private_root {
                    let explicit = normalize(explicit);
                    if explicit != normalize(&private_root) {
                        return Err(UpstrokeError::Refused {
                            message: format!(
                                "run `{run_id}` records its private half under `{}`, and \
                                 `--private-root {}` names another root. A run always resumes \
                                 under the root it recorded — today's default is not authority — \
                                 so nothing was locked and nothing was touched.",
                                private_root.display(),
                                explicit.display()
                            ),
                        });
                    }
                }

                Ok(Self {
                    run_id,
                    public_dir,
                    private_root,
                    private_dir,
                    reader,
                    first_line,
                    started,
                })
            }

            /// The resolved run id.
            #[must_use]
            pub fn run_id(&self) -> &str {
                &self.run_id
            }

            /// `<repo>/.upstroke/runs/<run_id>`.
            #[must_use]
            pub fn public_dir(&self) -> &Path {
                &self.public_dir
            }

            /// The authorized private root R — never today's default.
            #[must_use]
            pub fn private_root(&self) -> &Path {
                &self.private_root
            }

            /// `<R>/runs/<run_id>`, as the record wrote it.
            #[must_use]
            pub fn private_dir(&self) -> &Path {
                &self.private_dir
            }

            /// The reader the header selected. Always
            /// [`ReaderSelection::Topology`] for a value that exists.
            #[must_use]
            pub fn reader(&self) -> ReaderSelection {
                self.reader
            }

            /// The committed first line's bytes, without the commit marker.
            #[must_use]
            pub fn first_line(&self) -> &[u8] {
                &self.first_line
            }

            /// The run record the first line carries.
            #[must_use]
            pub fn started(&self) -> &RunStarted4 {
                &self.started
            }

            /// The log this run appends to.
            #[must_use]
            pub fn log_path(&self) -> PathBuf {
                self.public_dir.join(rundir::EVENT_LOG)
            }
        }

        /// The root R such that the recorded locator is exactly
        /// `<R>/runs/<run_id>`.
        ///
        /// Refuses **any other shape**, which is stricter than "ends with the
        /// run id": a locator of `<R>/runs/<other>/../<run_id>` ends correctly
        /// and names a directory the run does not own, and a locator of
        /// `<R>/<run_id>` has no `runs` component at all. Both are the
        /// `malformed_recorded_locator_refused_before_any_lock` case.
        fn authorized_root(private_dir: &Path, run_id: &str) -> Result<PathBuf, UpstrokeError> {
            let malformed = || UpstrokeError::Refused {
                message: format!(
                    "run `{run_id}` records its private half at `{}`, which is not of the shape \
                     `<root>/runs/{run_id}`. A locator of any other shape names a directory this \
                     run cannot prove is its own, so nothing was locked and nothing was touched.",
                    private_dir.display()
                ),
            };
            // No `..`, no `.`, no prefix trickery: every component is checked,
            // because the whole value of this refusal is that the two trailing
            // components are the only thing below the root.
            if private_dir
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(malformed());
            }
            let mut components = private_dir.components().rev();
            let last = components.next().ok_or_else(malformed)?;
            let penultimate = components.next().ok_or_else(malformed)?;
            if last.as_os_str() != std::ffi::OsStr::new(run_id) {
                return Err(malformed());
            }
            if penultimate.as_os_str() != std::ffi::OsStr::new("runs") {
                return Err(malformed());
            }
            let root: PathBuf = components.rev().collect();
            if root.as_os_str().is_empty() {
                return Err(malformed());
            }
            Ok(root)
        }

        /// A comparable form of a root that need not exist yet.
        ///
        /// `fs::canonicalize` is the right answer for a path that is on disk
        /// and no answer at all for one that is not — and an explicit
        /// `--private-root` naming a directory that does not exist is exactly
        /// the mismatch this comparison has to report rather than fail on. So
        /// the canonical form is taken when it is available and the lexical one
        /// otherwise, and the *mismatch message prints what the operator wrote*.
        fn normalize(path: &Path) -> PathBuf {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
    }

    // -- (a) locks ----------------------------------------------------------

    /// Recovery step (a), first half: the two locks, in order.
    pub mod locks {
        use std::path::Path;

        use super::root::RootDerived;
        use crate::error::UpstrokeError;
        use crate::rundir::{RunDirHooks, RunLock, WorktreeLock};

        /// The worktree lease and this run's run lock, both held.
        ///
        /// `recovery_order` (a): "take the worktree lock **then** the run lock
        /// (refused while a surviving reaper hold R28 is observed, per existing
        /// rules)". The order is the constructor's two statements and the
        /// refusal is the funnels' own: `WorktreeLock` scans every committed
        /// run directory through `Lock.ObserveCleanupHold` and refuses while
        /// one is held, and `RunLock` takes the momentary exclusive
        /// `Lock.ProbeCleanupExclusive` on this run.
        ///
        /// Both holds are R17 — "released at process exit (OS-released on
        /// death)" — so this value owning them is what makes the release
        /// happen at the end of the command whether or not anything asks.
        #[derive(Debug)]
        pub struct LocksHeld {
            root: RootDerived,
            /// Dropped in declaration order, which releases the run lock before
            /// the worktree lease — the reverse of acquisition.
            _run: RunLock,
            _worktree: WorktreeLock,
        }

        impl LocksHeld {
            /// Take the worktree lease, then this run's run lock.
            ///
            /// # Errors
            ///
            /// [`UpstrokeError::Refused`] when either lock is held by another
            /// process, or while a surviving reaper's shared cleanup hold (R28)
            /// is observed. The value is consumed either way, because a
            /// refusal here ends the command.
            pub fn take(
                root: RootDerived,
                repo_root: &Path,
                worktree_git_dir: &Path,
                hooks: &mut dyn RunDirHooks,
            ) -> Result<Self, UpstrokeError> {
                let worktree = WorktreeLock::acquire_in_hooked(repo_root, worktree_git_dir, hooks)?;
                let run = RunLock::acquire_hooked(root.public_dir(), hooks)?;
                Ok(Self {
                    root,
                    _run: run,
                    _worktree: worktree,
                })
            }

            /// What (a0) derived.
            #[must_use]
            pub fn root(&self) -> &RootDerived {
                &self.root
            }

            /// Consume the witness and hand out the two lock guards, still
            /// held.
            ///
            /// The fields are `_run` and `_worktree` because nothing reads
            /// them — they exist to be dropped, in declaration order, so the
            /// run lock is released before the worktree lease. **Handing them
            /// out keeps that property and moves it**: the guards outlive this
            /// call, drop in the same order at the end of the loop, and are
            /// still unreadable. What changes is *when* they die, and the whole
            /// reason a loop can exist is that it is no longer at the end of
            /// the recovery order.
            #[must_use]
            pub fn into_guards(self) -> (RunLock, WorktreeLock, RootDerived) {
                (self._run, self._worktree, self.root)
            }
        }
    }

    // -- (a) records --------------------------------------------------------

    /// Recovery step (a), second half: the two private records, verified
    /// **before any private write**.
    pub mod records {
        use std::path::Path;

        use super::locks::LocksHeld;
        use crate::error::UpstrokeError;
        use crate::rundir::{self, CommitRecord, OwnerRecord, RepoKey};

        /// `owner.json` and `committed.json`, both read and both agreeing.
        ///
        /// `recovery_order` (a): "**before any private write** verify
        /// `<R>/runs/<run_id>/owner.json` (run_id, repo_key, canonical
        /// public_dir, incarnation == `run_started.incarnation`, runner ==
        /// `run_started(4).runner`) and `committed.json` (`run_started_sha256`
        /// equals the digest of the committed first line), refusing on a
        /// missing private half, a missing record, or any disagreement (a
        /// private half that is not provably this run's is never written into;
        /// **a missing schema-4 private half is not recreated** — deferred)".
        ///
        /// Five owner fields and one commit field, each with its own refusal,
        /// because `refusal_condition` enumerates them — "run id, repo key,
        /// canonical public path, incarnation, runner, `run_started` digest" —
        /// and a single "the records disagree" would be green for a fixture
        /// that damaged the wrong one.
        #[derive(Debug)]
        pub struct RecordsVerified {
            locks: LocksHeld,
            owner: OwnerRecord,
            commit: CommitRecord,
        }

        impl RecordsVerified {
            /// Verify both records against the run record (a0) read.
            ///
            /// # Errors
            ///
            /// [`UpstrokeError::Refused`] for an absent private half, an absent
            /// or unparseable record, or a disagreement in any of the six
            /// fields.
            pub fn verify(locks: LocksHeld, repo_key: &RepoKey) -> Result<Self, UpstrokeError> {
                let root = locks.root();
                let run_id = root.run_id().to_owned();
                let private = root.private_dir().to_path_buf();
                let started = root.started();

                if !private.is_dir() {
                    return Err(UpstrokeError::Refused {
                        message: format!(
                            "run `{run_id}` records its private half at `{}` and nothing is \
                             there. A missing schema-4 private half is not recreated: the \
                             owner record, the commit record and every intent under it are the \
                             only evidence of what this run owns, and inventing an empty one \
                             would authorize deletions against a boundary nobody wrote.",
                            private.display()
                        ),
                    });
                }

                let owner: OwnerRecord = read_record(&private.join(rundir::OWNER_RECORD), &run_id)?;
                let disagreement =
                    |field: &str, recorded: &str, expected: &str| UpstrokeError::Refused {
                        message: format!(
                            "the owner record at `{}` records {field} `{recorded}`, and this run \
                             is `{expected}`. A private half that is not provably this run's is \
                             never written into.",
                            private.display()
                        ),
                    };
                if owner.run_id != run_id {
                    return Err(disagreement("run id", &owner.run_id, &run_id));
                }
                if owner.repo_key != repo_key.as_str() {
                    return Err(disagreement("repo key", &owner.repo_key, repo_key.as_str()));
                }
                let public = canonical_display(root.public_dir());
                if owner.public_dir != public {
                    return Err(disagreement("public directory", &owner.public_dir, &public));
                }
                if owner.incarnation != started.incarnation.0 {
                    return Err(disagreement(
                        "incarnation",
                        &owner.incarnation,
                        &started.incarnation.0,
                    ));
                }
                // INV-23: "every later incarnation rebuilds the Runner from
                // `run_started(4).runner` — **verified equal to
                // owner.json.runner** — before its RunnerPreflight". The
                // comparison is `difference`, which names WHICH field moved:
                // "the runner disagrees" is not something an operator can act
                // on, and the field is.
                if let Some(field) = started.runner.difference(&owner.runner) {
                    return Err(UpstrokeError::Refused {
                        message: format!(
                            "the owner record at `{}` records a different runner from \
                             `run_started(4).runner`: the {field} differs. A run's confinement \
                             boundary and image are fixed for its life, and the two records that \
                             carry them must agree before anything is rebuilt from either.",
                            private.display()
                        ),
                    });
                }

                let commit: CommitRecord =
                    read_record(&private.join(rundir::COMMIT_RECORD), &run_id)?;
                let digest = rundir::run_started_sha256(root.first_line());
                if commit.run_started_sha256 != digest {
                    return Err(UpstrokeError::Refused {
                        message: format!(
                            "the commit record at `{}` says the committed first line digests \
                             `{}`, and the line in the log digests `{digest}`. One of the two \
                             moved after the run committed, so nothing derived from either is \
                             acted on.",
                            private.display(),
                            commit.run_started_sha256
                        ),
                    });
                }

                Ok(Self {
                    locks,
                    owner,
                    commit,
                })
            }

            /// The locks this verification ran under.
            #[must_use]
            pub fn locks(&self) -> &LocksHeld {
                &self.locks
            }

            /// Consume this witness and hand on the one it was built from.
            ///
            /// **Mints nothing.** §2's rule is that a witness is constructible
            /// only by its own constructor from its own predecessor, and this
            /// goes the other way: it takes a witness apart, it does not put
            /// one together. Walking backwards by reference was already
            /// possible through the accessor above; what this adds is
            /// *ownership*, which the run loop needs and a reference cannot
            /// give — at the bottom of the chain the parts are the append
            /// handle and the two locks, and a borrowed lock is a lock this
            /// process is about to drop.
            #[must_use]
            pub fn into_locks(self) -> LocksHeld {
                self.locks
            }

            /// The verified owner record.
            #[must_use]
            pub fn owner(&self) -> &OwnerRecord {
                &self.owner
            }

            /// The verified commit record. Its `run_started_sha256` is what the
            /// stable-prefix barrier proves the reread first line against.
            #[must_use]
            pub fn commit(&self) -> &CommitRecord {
                &self.commit
            }
        }

        /// Read one JSON record, or refuse naming the file.
        fn read_record<T: serde::de::DeserializeOwned>(
            path: &Path,
            run_id: &str,
        ) -> Result<T, UpstrokeError> {
            let text = std::fs::read_to_string(path).map_err(|source| {
                if source.kind() == std::io::ErrorKind::NotFound {
                    return UpstrokeError::Refused {
                        message: format!(
                            "run `{run_id}`'s private half has no `{}`. Without it this process \
                             cannot prove the half is this run's, and an unprovable private half \
                             is never written into.",
                            path.display()
                        ),
                    };
                }
                UpstrokeError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            serde_json::from_str(&text).map_err(|error| UpstrokeError::Refused {
                message: format!(
                    "run `{run_id}`'s record at `{}` is not the record this build understands \
                     ({error}); a record this build cannot read is a record it must not act on.",
                    path.display()
                ),
            })
        }

        /// A path in the form a record writes it: canonical when the filesystem
        /// will say so, and lexical otherwise.
        fn canonical_display(path: &Path) -> String {
            std::fs::canonicalize(path)
                .unwrap_or_else(|_| path.to_path_buf())
                .display()
                .to_string()
        }
    }

    // -- (a1) barrier -------------------------------------------------------

    /// Recovery step (a1): the stable-prefix barrier.
    pub mod barrier {
        use super::records::RecordsVerified;
        use crate::error::UpstrokeError;
        use crate::events::log::StablePrefix;
        use crate::runner::container::census::{
            PrefixBytes, PrefixReplay, PrefixReread, PrefixSync, StablePrefixBarrier,
        };
        use crate::topology::fold::TopologyFold;

        /// The barrier of `coordinator_integration.stable_prefix_barrier`,
        /// established.
        ///
        /// # Why the constructor takes `StablePrefix` **by value**
        ///
        /// `StablePrefixBarrier::establish` takes `PrefixSync`, `PrefixReread`
        /// and `PrefixReplay`, all of which have public fields, and
        /// `PrefixBytes::of` is public — so `establish(PrefixSync{n},
        /// &PrefixReread{first: b, second: b}, &PrefixReplay{replayed: b})`
        /// returns `Ok` for **any** byte string `b`, and the whole recovery
        /// chain below this point would be reachable from three copies of one
        /// lie. Accepting a `StablePrefixBarrier` from a caller would inherit
        /// that.
        ///
        /// [`crate::events::log::StablePrefix`] has private fields, derives no
        /// `Clone`, and has exactly one constructor —
        /// `crate::events::log::establish_stable_prefix`, which performs the
        /// sync, the reread, the four proofs and the checked replay. So a
        /// `StablePrefix` is unforgeable outside `src/events/log.rs`, and this
        /// module **derives** the census's `StablePrefixBarrier` from it rather
        /// than being handed one.
        ///
        /// The derivation is trivially satisfiable *because the proof already
        /// happened*: the three measurements all come from the one proven byte
        /// string, so the four predicates hold by construction. That is the
        /// point — the barrier value carries the evidence, and the evidence is
        /// `StablePrefix`'s own.
        #[derive(Debug)]
        pub struct BarrierHeld {
            records: RecordsVerified,
            /// The append handle the barrier entitles this command to.
            log: crate::events::log::EventLog,
            /// The exact bytes that were synced, reread, proven, and replayed.
            bytes: Vec<u8>,
            /// The fold built from exactly those bytes, and no others.
            /// The events the barrier parsed from exactly those bytes.
            ///
            /// Carried rather than re-derived for the same reason the fold is:
            /// there is one production parse of a log and it is the barrier's.
            events: Vec<crate::topology::events::TopologyEvent>,
            fold: TopologyFold,
            barrier: StablePrefixBarrier,
        }

        impl BarrierHeld {
            /// Hold the barrier over `prefix`, under `records`.
            ///
            /// Both arguments by value: `records` because §2's rule is that
            /// every witness consumes its predecessor, and `prefix` because
            /// §4's is that the barrier's evidence cannot be borrowed from
            /// something a caller kept a second handle on.
            ///
            /// # Errors
            ///
            /// [`UpstrokeError::Refused`] from
            /// [`StablePrefixBarrier::establish`]. Unreachable for a
            /// `StablePrefix` — the three measurements are one value measured
            /// three times — and returned rather than unwrapped because this
            /// crate does not panic outside tests, and because an
            /// `establish` that grew a fifth predicate would then refuse here
            /// rather than silently pass.
            pub fn from(
                records: RecordsVerified,
                prefix: StablePrefix,
            ) -> Result<Self, UpstrokeError> {
                // Taken apart rather than kept whole: the barrier owns the
                // append handle from here on, and a `StablePrefix` that both
                // this value and a caller could reach would be two handles onto
                // one log.
                let (log, bytes, events, fold) = prefix.into_log_and_fold();
                let measured = PrefixBytes::of(&bytes);
                let barrier = StablePrefixBarrier::establish(
                    // Every byte the barrier proved is a byte
                    // `Event.OpenLog.SyncPrefix` successfully synced.
                    PrefixSync {
                        synced_len: measured.len,
                    },
                    &PrefixReread {
                        first: measured.clone(),
                        second: measured.clone(),
                    },
                    // "the replay consumed exactly those reread bytes":
                    // `establish_stable_prefix` replays `reread` and moves the
                    // same value into the result, so there is one byte string
                    // here and not two that happen to agree.
                    &PrefixReplay { replayed: measured },
                )?;
                Ok(Self {
                    events,
                    records,
                    log,
                    bytes,
                    fold,
                    barrier,
                })
            }

            /// The records verified before the barrier.
            #[must_use]
            pub fn records(&self) -> &RecordsVerified {
                &self.records
            }

            /// Consume the barrier and hand out the run's own state.
            ///
            /// **The append handle and the fold are one pair, and this is the
            /// only way to own them.** The log is the handle the barrier
            /// entitled this command to; the fold is built from *exactly* the
            /// bytes the barrier synced, reread, proved and replayed. A caller
            /// that reopened the log to get a handle would be appending to a
            /// prefix its own barrier never proved, which is the whole of what
            /// (a1) exists to prevent — so the pair leaves together or not at
            /// all.
            #[must_use]
            pub fn into_log_fold_and_records(
                self,
            ) -> (crate::events::log::EventLog, TopologyFold, RecordsVerified) {
                (self.log, self.fold, self.records)
            }

            /// The fold built from exactly the proven bytes.
            #[must_use]
            pub fn fold(&self) -> &TopologyFold {
                &self.fold
            }

            /// The events the barrier parsed from exactly those bytes.
            ///
            /// For a recovery step that needs what the fold does not keep — the
            /// `AttemptRecord` a durable settlement carried. Reading them here
            /// rather than parsing the log again is what keeps the barrier the
            /// one production parse.
            #[must_use]
            pub fn events(&self) -> &[crate::topology::events::TopologyEvent] {
                &self.events
            }

            /// The proven bytes.
            #[must_use]
            pub fn bytes(&self) -> &[u8] {
                &self.bytes
            }

            /// The census's evidence value, derived here and never accepted
            /// from a caller.
            #[must_use]
            pub fn stable_prefix_barrier(&self) -> StablePrefixBarrier {
                self.barrier.clone()
            }

            /// The append handle the barrier entitles this command to, and the
            /// fold it was built from.
            ///
            /// Split out rather than exposed as two `&mut` accessors because a
            /// recovery emitter needs both at once and Rust's borrow checker
            /// would otherwise force one of them through a clone.
            pub(in crate::engine::topology::recover) fn writer(
                &mut self,
            ) -> (&mut crate::events::log::EventLog, &mut TopologyFold) {
                (&mut self.log, &mut self.fold)
            }
        }
    }

    // -- (a) census ---------------------------------------------------------

    /// Recovery step (a), third half: the startup census, **after** the
    /// barrier.
    pub mod censused {
        use std::path::Path;

        use super::barrier::BarrierHeld;
        use crate::error::UpstrokeError;
        use crate::rundir::RepoKey;
        use crate::runner::container::GitView;
        use crate::runner::container::census::{
            Census, CensusComplete, CensusReport, CensusStart, run_startup_census,
        };
        use crate::runner::container::runtime::{ContainerRuntime, OwnerLiveness};
        use crate::topology::events::IncarnationId;

        use crate::engine::topology::seams::TopologyHooks;
        use crate::engine::topology::startup::{CensusInputs, RunDirCensusReport, census_run_dirs};

        /// What the census reads from the world: the four seams and the two
        /// identities it is censusing on behalf of.
        ///
        /// A bundle rather than six arguments because the six do not vary
        /// independently — they are one process's view of one repository — and
        /// a six-argument constructor is six places for two `&Path`s of the
        /// same type to be passed in the wrong order.
        pub struct CensusSeams<'a> {
            /// This coordinator process's per-process ULID.
            pub incarnation: &'a IncarnationId,
            pub repo_root: &'a Path,
            pub repo_key: &'a RepoKey,
            pub runtime: &'a dyn ContainerRuntime,
            pub liveness: &'a dyn OwnerLiveness,
            pub view: &'a dyn GitView,
        }

        /// The census of step (a), and the barrier it was decided under.
        ///
        /// **The census returns the witness; it does not get wrapped.** A
        /// wrapper would prove possession — the holder had a census result and
        /// a barrier, in either order — and the packet requires the barrier
        /// *first*: "a resume takes its run lock first, establishes the
        /// stable-prefix barrier of recovery step (a1), **then** censuses". The
        /// constructor consuming [`BarrierHeld`] by value is that ordering, as
        /// a call.
        #[derive(Debug)]
        pub struct ResumeCensused {
            barrier: BarrierHeld,
            containers: CensusComplete,
            run_dirs: RunDirCensusReport,
        }

        impl ResumeCensused {
            /// Census under `barrier`: containers first, then the run
            /// directories — this run's own stale marker among them.
            ///
            /// **A resume reclaims.** `recovery_order` (a1) is a "startup
            /// census … run-directory census incl. this run's own stale marker,
            /// which the owner removes here, **and husk reclamation under the
            /// ownership proof**", and INV-15 reclaims pre-run husks "at
            /// write-command start under the worktree lock". A resume is a
            /// write command and holds that lock, so the run-directory half is
            /// [`census_run_dirs`] — the same function `upstroke run` calls, not
            /// a read-only second pass. A pass that classified and reported
            /// would leave every husk beside the resuming run on disk for ever,
            /// with only a fresh `upstroke run` able to reclaim it.
            ///
            /// `own_run` is this run's id and licenses exactly one thing: the
            /// stale-marker repair that `resource_accounting` gives to "a census
            /// with the lock free **or** its owner on resume". It cannot reach
            /// the husk arms, which are gated on the lock alone — and this
            /// process holds its own run's lock, so its own directory is refused
            /// there whatever shape its log is in.
            ///
            /// **A husk beside this run cannot end this resume.** A reclaim that
            /// the filesystem refuses is that directory's
            /// [`crate::engine::topology::startup::RunDirOutcome::Unreclaimable`]
            /// entry and the census carries on: one dead run's unremovable
            /// residue used to fail `upstroke resume`
            /// for every run in the repository, on every attempt, and — because
            /// the walk is in ascending run-id order — took this run's own
            /// stale-marker repair with it whenever the husk sorted first.
            ///
            /// # Errors
            ///
            /// [`UpstrokeError::Refused`] from the container census — an
            /// unreachable runtime with intents present, an intent naming this
            /// process's own incarnation, an unreclaimable dead owner — or
            /// [`UpstrokeError::Io`] when `<repo>/.upstroke/runs` exists and
            /// cannot be enumerated, which is the run-directory half reporting
            /// that it did not happen rather than that it found nothing.
            pub fn census(
                barrier: BarrierHeld,
                seams: &CensusSeams<'_>,
                hooks: &mut dyn TopologyHooks,
            ) -> Result<Self, UpstrokeError> {
                let CensusSeams {
                    incarnation,
                    repo_root,
                    repo_key,
                    runtime,
                    liveness,
                    view,
                } = seams;
                let run_id = barrier.records().locks().root().run_id().to_owned();
                let private_root = barrier
                    .records()
                    .locks()
                    .root()
                    .private_root()
                    .to_path_buf();

                // One value for both halves. `CensusInputs` carries a single
                // `authorized_root`, so the root half (a) scans and the root
                // half (b) proves ownership under cannot disagree.
                let inputs = CensusInputs {
                    repo_root,
                    repo_key,
                    authorized_root: &private_root,
                    incarnation: incarnation.0.as_str(),
                    runtime: *runtime,
                    liveness: *liveness,
                    view: *view,
                };

                // (i) Containers, including every earlier incarnation of this
                // run under `<R>/containers`. The start value carries the
                // barrier this module derived, so `CensusStart::Resume` cannot
                // be built here without one.
                let start = CensusStart::Resume {
                    run_id: run_id.clone(),
                    incarnation: incarnation.0.clone(),
                    barrier: barrier.stable_prefix_barrier(),
                };
                let containers = run_startup_census(
                    hooks.container(),
                    &Census {
                        private_root: inputs.authorized_root,
                        start: &start,
                        runtime: inputs.runtime,
                        liveness: inputs.liveness,
                        view: inputs.view,
                    },
                )?;

                // (ii) Run directories: classified, then reclaimed under the
                // ownership proof — private half through the proof-token funnel
                // first, public directory with the marker last — and this run's
                // own stale marker repaired by its owner. `own_run` is also what
                // guarantees this run's directory is walked at all, whatever the
                // enumeration of the runs tree returned.
                let run_dirs = census_run_dirs(hooks.rundir(), &inputs, Some(&run_id))?;

                Ok(Self {
                    barrier,
                    containers,
                    run_dirs,
                })
            }

            /// The barrier this census was decided under.
            #[must_use]
            pub fn barrier(&self) -> &BarrierHeld {
                &self.barrier
            }

            /// Consume this witness and hand on the one it was built from.
            ///
            /// **Mints nothing.** §2's rule is that a witness is constructible
            /// only by its own constructor from its own predecessor, and this
            /// goes the other way: it takes a witness apart, it does not put
            /// one together. Walking backwards by reference was already
            /// possible through the accessor above; what this adds is
            /// *ownership*, which the run loop needs and a reference cannot
            /// give — at the bottom of the chain the parts are the append
            /// handle and the two locks, and a borrowed lock is a lock this
            /// process is about to drop.
            #[must_use]
            pub fn into_barrier(self) -> BarrierHeld {
                self.barrier
            }

            /// The barrier, mutably — the append handle lives inside it.
            pub(in crate::engine::topology::recover) fn barrier_mut(&mut self) -> &mut BarrierHeld {
                &mut self.barrier
            }

            /// What the container census reclaimed and left alone.
            ///
            /// Returns the **report** rather than the `CensusComplete` token.
            /// The token stays owned here on purpose: it is the value
            /// `crash_reconstruction`'s four "before"s are gated on, and
            /// handing it out would let a caller present census evidence
            /// without presenting the barrier this census ran under. The
            /// report is what a caller actually reads.
            ///
            /// (It also keeps this accessor out of
            /// `runner::container::census::tests::census_returns_the_only_token_that_reaches_a_consumer`,
            /// whose needle is the text `CensusComplete` followed by a brace
            /// and therefore matches a return type as well as a struct
            /// literal. That is a property of the needle rather than of this
            /// code, and the right response is not to construct one — which
            /// this does not — but there is no reason to sit on a false
            /// positive when the narrower return type is the better API.)
            #[must_use]
            pub fn containers(&self) -> &CensusReport {
                self.containers.report()
            }

            /// What the run-directory half found and did: one entry per
            /// directory under `<repo>/.upstroke/runs`, with its locator, its
            /// class and its outcome.
            ///
            /// Total over the runs directory, so "every husk retained and
            /// reported with its locator and reason" and "every husk reclaimed
            /// under the proof" are both read off the one report — and a
            /// directory this census did nothing to is still an entry.
            #[must_use]
            pub const fn run_dirs(&self) -> &RunDirCensusReport {
                &self.run_dirs
            }
        }
    }

    // -- (c) rebuild --------------------------------------------------------

    /// Recovery step (c), first half: the recorded Runner, rebuilt by
    /// **read-only inspection**, before any spawn.
    pub mod rebuilt {
        use super::censused::ResumeCensused;
        use crate::config::RunnerSelection;
        use crate::error::UpstrokeError;
        use crate::runner::container::resolve::{InspectionRefusal, rebuild_by_inspection};
        use crate::runner::container::runtime::ContainerRuntime;
        use crate::topology::events::{RunnerKind, RunnerPolicy};

        /// The Runner this incarnation established, equal to
        /// `run_started(4).runner` field for field.
        ///
        /// `recovery_order` (c): "rebuild the Runner from
        /// `run_started(4).runner` (today's `[runner]` config that differs
        /// warns naming the difference and is ignored; a reference that now
        /// names another image warns while the recorded id is used; **refusals
        /// by read-only inspection — container runtime unavailable, recorded
        /// image id absent from the runtime, credential volume absent — refuse
        /// before any spawn**)".
        ///
        /// The split between this witness and [`super::certified`] **is** the
        /// refusal split: a `RunnerRebuilt` exists only when every inspection
        /// passed, and nothing has been spawned to make one.
        #[derive(Debug)]
        pub struct RunnerRebuilt {
            censused: ResumeCensused,
            policy: RunnerPolicy,
            warnings: Vec<String>,
        }

        impl RunnerRebuilt {
            /// Rebuild by inspection alone.
            ///
            /// `runtime` is consulted only for a recorded **container** runner.
            /// A host record needs no runtime, and demanding one would refuse a
            /// host run on a machine with no container runtime — which is every
            /// machine the host runner exists for.
            ///
            /// **The record wins, exactly.** The returned policy is the
            /// recorded one field for field; today's config reaches only the
            /// warnings.
            ///
            /// # Errors
            ///
            /// [`UpstrokeError::Refused`] for an unavailable runtime, a
            /// recorded image id the runtime no longer holds, an absent
            /// credential volume, or a recorded container runner with no
            /// runtime seam to ask.
            pub fn rebuild(
                censused: ResumeCensused,
                today: &RunnerSelection,
                runtime: Option<&dyn ContainerRuntime>,
            ) -> Result<Self, UpstrokeError> {
                let record = censused
                    .barrier()
                    .records()
                    .locks()
                    .root()
                    .started()
                    .runner
                    .clone();
                let mut warnings = Vec::new();
                let policy = match record.kind {
                    RunnerKind::Container => {
                        let runtime = runtime.ok_or(UpstrokeError::Refused {
                            message: InspectionRefusal::RuntimeUnavailable {
                                operation: crate::runner::container::runtime::RuntimeOp::Probe,
                                detail: "this process was given no container runtime to inspect"
                                    .to_owned(),
                            }
                            .to_string(),
                        })?;
                        rebuild_by_inspection(runtime, &record, today, &mut warnings)?
                    }
                    // A host runner has nothing to inspect: `host-v1` names no
                    // image and no volume, and `resolve_host` is total. What
                    // remains is the config-drift warning, which applies to
                    // both kinds.
                    RunnerKind::Host => {
                        if today.kind != RunnerKind::Host {
                            warnings.push(format!(
                                "[runner] in the config selects the `{:?}` runner and this run \
                                 recorded the host runner. A run keeps the boundary and image it \
                                 started with, so the recorded runner is rebuilt and the \
                                 configured one is ignored.",
                                today.kind
                            ));
                        }
                        record.clone()
                    }
                };
                Ok(Self {
                    censused,
                    policy,
                    warnings,
                })
            }

            /// The census this rebuild followed.
            #[must_use]
            pub fn censused(&self) -> &ResumeCensused {
                &self.censused
            }

            /// Consume this witness and hand on the one it was built from.
            ///
            /// **Mints nothing.** §2's rule is that a witness is constructible
            /// only by its own constructor from its own predecessor, and this
            /// goes the other way: it takes a witness apart, it does not put
            /// one together. Walking backwards by reference was already
            /// possible through the accessor above; what this adds is
            /// *ownership*, which the run loop needs and a reference cannot
            /// give — at the bottom of the chain the parts are the append
            /// handle and the two locks, and a borrowed lock is a lock this
            /// process is about to drop.
            #[must_use]
            pub fn into_censused(self) -> ResumeCensused {
                self.censused
            }

            /// The census, mutably.
            pub(in crate::engine::topology::recover) fn censused_mut(
                &mut self,
            ) -> &mut ResumeCensused {
                &mut self.censused
            }

            /// The rebuilt policy — the record, field for field.
            #[must_use]
            pub fn policy(&self) -> &RunnerPolicy {
                &self.policy
            }

            /// What the operator is told about a config that moved under the
            /// run, or a reference that now names another image.
            #[must_use]
            pub fn warnings(&self) -> &[String] {
                &self.warnings
            }
        }
    }

    // -- (c) preflight ------------------------------------------------------

    /// Recovery step (c), second half: the `RunnerPreflight` probes.
    pub mod certified {
        use super::rebuilt::RunnerRebuilt;
        use crate::error::UpstrokeError;
        use crate::runner::container::resolve::RunnerPreflight;

        /// The shell and every recorded agent CLI, certified **inside the
        /// recorded image**, before any recovery event.
        ///
        /// This is the witness every recovery emitter takes by reference and
        /// that `run_resumed` consumes by value. Holding one means: the locks
        /// are held, the records agree, the barrier proved the prefix, the
        /// census ran under it, the runner was re-established by inspection,
        /// and a shell and every recorded CLI answered inside the boundary.
        /// There is no other way to have one.
        #[derive(Debug)]
        pub struct PreflightCertified {
            rebuilt: RunnerRebuilt,
            agents: Vec<String>,
        }

        impl PreflightCertified {
            /// Run the pre-flight through the rebuilt runner.
            ///
            /// # Errors
            ///
            /// [`UpstrokeError::Refused`] naming the shell or the agent whose
            /// CLI did not answer. `expected_failures_refusals[2]`: the refusal
            /// lands "before any recovery event or work spawn", with the probe
            /// invocations reclaimed like every probe.
            pub fn certify(
                rebuilt: RunnerRebuilt,
                preflight: &dyn RunnerPreflight,
            ) -> Result<Self, UpstrokeError> {
                preflight.certify(rebuilt.policy())?;
                // `run_resumed(4).probed_agents` is "what this incarnation's
                // pre-flight probes found", and what it probed is what the run
                // recorded: the pre-flight is constructed from
                // `run_started(4).probed_agents` and certifies all of them or
                // refuses. Taking the list from the record rather than from the
                // seam is what keeps a `RunnerPreflight` double from being able
                // to widen the run's agent allow-list.
                let agents = rebuilt
                    .censused()
                    .barrier()
                    .records()
                    .locks()
                    .root()
                    .started()
                    .probed_agents
                    .clone();
                Ok(Self { rebuilt, agents })
            }

            /// The rebuilt runner these probes executed through.
            #[must_use]
            pub fn rebuilt(&self) -> &RunnerRebuilt {
                &self.rebuilt
            }

            /// Consume this witness and hand on the one it was built from.
            ///
            /// **Mints nothing.** §2's rule is that a witness is constructible
            /// only by its own constructor from its own predecessor, and this
            /// goes the other way: it takes a witness apart, it does not put
            /// one together. Walking backwards by reference was already
            /// possible through the accessor above; what this adds is
            /// *ownership*, which the run loop needs and a reference cannot
            /// give — at the bottom of the chain the parts are the append
            /// handle and the two locks, and a borrowed lock is a lock this
            /// process is about to drop.
            #[must_use]
            pub fn into_rebuilt(self) -> RunnerRebuilt {
                self.rebuilt
            }

            /// The rebuilt runner, mutably — the append handle is under it.
            pub(in crate::engine::topology::recover) fn rebuilt_mut(
                &mut self,
            ) -> &mut RunnerRebuilt {
                &mut self.rebuilt
            }

            /// The agents this pre-flight certified.
            #[must_use]
            pub fn probed_agents(&self) -> &[String] {
                &self.agents
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The order, driven
// ---------------------------------------------------------------------------

/// Everything the recovery order reads from the world after (a0).
///
/// (a0) is deliberately **not** in here: it takes a repository, a wanted run id
/// and an optional `--private-root`, and nothing else, because everything else
/// is derived from the record it has not read yet.
pub struct ResumeSeams<'a> {
    pub repo_root: &'a Path,
    /// The git dir of the worktree this run drives — where `upstroke-worktree.lock`
    /// (R25) lives. Passed in rather than derived here: deriving it opens a
    /// `Workspace`, which runs `git`, and the recovery order's own refusals
    /// must not depend on a subprocess.
    pub worktree_git_dir: &'a Path,
    pub repo_key: &'a RepoKey,
    /// This coordinator process's per-process ULID.
    pub incarnation: &'a IncarnationId,
    /// The frozen plan and its digest, which the checked replay authenticates
    /// the recorded registry against.
    pub inputs: FrozenInputs,
    /// Today's `[runner]` selection. Warned about when it differs; never used.
    pub today: &'a RunnerSelection,
    pub runtime: &'a dyn ContainerRuntime,
    pub liveness: &'a dyn OwnerLiveness,
    pub view: &'a dyn GitView,
    pub preflight: &'a dyn RunnerPreflight,
    /// P8's ref funnel, for the P7/P8 repair — the same seam the creator is
    /// given, and [`crate::workspace_manager::WorkspaceManager`] is the
    /// production implementation of it.
    ///
    /// A seam and not a `WorkspaceManager` for the reason [`IntegrationRefs`]
    /// itself is one: this file is a `TOPOLOGY_MODULE`, in which
    /// `std::process::Command` is a build error, so a resume test that had to
    /// stand up a real repository to reach the ref could not be written here at
    /// all.
    pub refs: &'a dyn IntegrationRefs,
    /// The workspace manager step (g) rebuilds worktrees through. Every Git
    /// effect of the recovery order that is not an append goes through it.
    pub manager: &'a WorkspaceManager,
    pub clock: &'a dyn TimeSource,
}

/// One step of `decisions.sequential_substrate.recovery_order`, as the packet
/// names it.
///
/// **This type is the reason the order can be checked for completeness.** The
/// packet names eleven steps in one sentence, and a step that no code performs
/// is invisible to every technique this project runs: a mutation catalogue
/// measures whether existing code is pinned, and **omission has nothing to
/// mutate**. Step (g) was absent from this module for the whole of PR7's
/// implementation and two review rounds, with 117 named tests passing and every
/// gate green, because no test read the packet's list.
///
/// So the list is a type. Adding a step to the packet means adding a variant,
/// and a variant with no arm does not compile. Removing a step's call leaves a
/// hole in [`Recovered::steps`] that
/// `the_recovery_order_performs_every_step_the_packet_names` fails on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecoveryStep {
    /// Read-only root derivation, before any lock.
    A0,
    /// The two locks, the two records, the census and the residue reclaim.
    A,
    /// The stable-prefix barrier.
    A1,
    /// Complete or Halted: terminal finalization then refuse continuation.
    B,
    /// Rebuild the recorded Runner, then its pre-flight probes.
    C,
    /// Settle every in-flight identity interrupted.
    D,
    /// Close every `RetainedIdle` generation.
    E,
    /// Complete `Promoting` promotions and authorized publications.
    F,
    /// Recreate `OpenNoAttempt` worktrees at their bases.
    G,
    /// Append `run_resumed(4)`.
    H,
    /// Admission.
    I,
}

/// Which part of the system performs a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Performer {
    /// [`run_recovery_order`] itself.
    ThisOrder,
    /// The caller, before the order is entered — `(a0)` is read-only and
    /// precedes every lock, so it cannot be inside a function that has already
    /// taken one.
    CallerBefore,
    /// The loop, after the order returns.
    LoopAfter,
}

impl RecoveryStep {
    /// The eleven steps, in the packet's order.
    ///
    /// Transcribed from `decisions.sequential_substrate.recovery_order`. The
    /// order of this array **is** the claim; a test compares the trace against
    /// it rather than against a second list written from memory.
    pub const ALL: [Self; 11] = [
        Self::A0,
        Self::A,
        Self::A1,
        Self::B,
        Self::C,
        Self::D,
        Self::E,
        Self::F,
        Self::G,
        Self::H,
        Self::I,
    ];

    /// The packet's own label for this step.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::A0 => "a0",
            Self::A => "a",
            Self::A1 => "a1",
            Self::B => "b",
            Self::C => "c",
            Self::D => "d",
            Self::E => "e",
            Self::F => "f",
            Self::G => "g",
            Self::H => "h",
            Self::I => "i",
        }
    }

    /// The live clause that moves this step out of the packet's sequence
    /// position, where one does.
    ///
    /// **A deviation with a reason is not the same thing as a step in the wrong
    /// place, and the difference has to be stated somewhere a test can read.**
    /// Left implicit, an argued reordering and an accidental one look
    /// identical — which is how a slice ends up with a comment claiming "steps
    /// (a) through (h), in the packet's order" over a body that performs nine
    /// of ten in a different one.
    #[must_use]
    pub const fn position_override(self) -> Option<&'static str> {
        match self {
            // `checkpoint_refusals`: "an intermediate build refuses, **before
            // any append**, any operation whose terminals it does not
            // implement". PR7's (f) is a refusal — it does not complete a
            // promotion, it declines to — and a refusal taken after (d) and
            // (e) is a refusal after two appends, which that sentence forbids.
            // So (f) runs before them, and the authorized publication it does
            // perform (the recorded integration ref) rides with it for the same
            // reason: the ref is created before the first append of the resume,
            // which `kill_after_run_started_creates_integration_ref` asserts by
            // reading the log at the funnel's entry.
            Self::F => Some("decisions.sequential_substrate.checkpoint_refusals"),
            _ => None,
        }
    }

    /// Who performs it, and — for the two this order does not — why.
    ///
    /// The two exceptions are stated here rather than left implicit, because
    /// "this module does not do that one" is exactly the sentence that hid step
    /// (g): a reader who accepts it without a reason cannot tell a delegated
    /// step from a missing one.
    #[must_use]
    pub const fn performer(self) -> Performer {
        match self {
            // Read-only and before `Lock.AcquireWorktree`, so no R17 hold is
            // taken and no R25 lock file is created by a refusal here. A
            // function that has already taken the locks cannot perform it.
            Self::A0 => Performer::CallerBefore,
            Self::A
            | Self::A1
            | Self::B
            | Self::C
            | Self::D
            | Self::E
            | Self::F
            | Self::G
            | Self::H => Performer::ThisOrder,
            // `checkpoint_refusals` gives the loop's refusals to `select.rs`,
            // and admission is the loop's first act, not recovery's last.
            Self::I => Performer::LoopAfter,
        }
    }
}

/// What one completed recovery did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// (d): how many in-flight identities were settled interrupted.
    pub interrupted: usize,
    /// (e): how many `RetainedIdle` generations were closed.
    pub retained_closed: usize,
    /// The `Promoting` generations whose `candidate_prepared` this resume
    /// appended — erratum **E6**'s convergence. Empty on a healthy resume.
    pub promoted: Vec<TaskKey>,
    /// (g): every `OpenNoAttempt` generation rebuilt, and whether its worktree
    /// verified or had to be recreated. A value rather than a count, because
    /// "the step ran" and "the step ran and found nothing to do" are the two
    /// states a test of an ordered sequence has to tell apart.
    pub recreated: Vec<(TaskKey, GenerationId, Reuse)>,
    /// Every step this order performed, in the order it performed them.
    ///
    /// Pushed as each step returns `Ok`, so a step whose call is deleted
    /// disappears from the trace. That is what makes
    /// [`RecoveryStep`]'s list checkable against something other than itself.
    pub steps: Vec<RecoveryStep>,
    /// (h): what `run_resumed` established.
    pub resumed: Resumed,
    /// (c): the config drift and moved-reference warnings, in order.
    pub warnings: Vec<String>,
}

/// Steps (a) through (h), in the packet's order, each step consuming the last
/// step's witness.
///
/// The ordering claim of this function is not in its statements — it is in the
/// types. `RecordsVerified::verify` cannot be called without a `LocksHeld`;
/// `ResumeCensused::census` cannot be called without a `BarrierHeld`; and
/// [`run_resumed`] eats the `PreflightCertified` that every recovery emitter
/// needs. Reordering the body does not compile.
///
/// Step (i), admission, is the loop's and is not here: `checkpoint_refusals`
/// gives the loop's refusals to `select.rs`, and this file owns step (b)'s.
///
/// # Errors
///
/// The first refusal of the order: a lock held elsewhere or a surviving reaper
/// hold (a); a missing or disagreeing record (a); a failed sync, unstable
/// reread or refused replay (a1); a census refusal (a); a Complete or Halted
/// run (b); an inspection refusal (c, before any spawn); a probe refusal (c,
/// before any recovery event); a recorded integration ref that is symbolic,
/// checked out, or at a SHA other than the recorded base (the P7/P8 repair,
/// also before any recovery event); or an append error at (d)–(h), after which
/// the fold is poisoned and the next resume repeats from (a0).
pub fn run_recovery_order(
    root: RootDerived,
    seams: &ResumeSeams<'_>,
    hooks: &mut dyn TopologyHooks,
    warnings: &mut Vec<String>,
) -> Result<(Recovered, RunHandle), UpstrokeError> {
    // (a) the two locks, then the two records — before any private write.
    let locks = LocksHeld::take(
        root,
        seams.repo_root,
        seams.worktree_git_dir,
        hooks.rundir(),
    )?;
    let records = RecordsVerified::verify(locks, seams.repo_key)?;
    let mut steps = vec![RecoveryStep::A];

    // (a1) the barrier, before every fold-derived effect of every later step.
    let log_path = records.locks().root().log_path();
    let committed = records.commit().run_started_sha256.clone();
    let prefix = crate::events::log::establish_stable_prefix(
        &log_path,
        seams.inputs.clone(),
        Some(&committed),
        warnings,
        hooks.events(),
    )?;
    let barrier = BarrierHeld::from(records, prefix)?;
    steps.push(RecoveryStep::A1);

    // (a) the census, under the barrier and never before it.
    let censused = ResumeCensused::census(
        barrier,
        &CensusSeams {
            incarnation: seams.incarnation,
            repo_root: seams.repo_root,
            repo_key: seams.repo_key,
            runtime: seams.runtime,
            liveness: seams.liveness,
            view: seams.view,
        },
        hooks,
    )?;

    // (b) Complete or Halted: finalize then refuse. PR7 refuses.
    refuse_if_finished(&censused)?;
    steps.push(RecoveryStep::B);

    // (c) the recorded Runner by inspection, then its probes.
    let rebuilt = RunnerRebuilt::rebuild(censused, seams.today, Some(seams.runtime))?;
    let drift: Vec<String> = rebuilt.warnings().to_vec();
    warnings.extend(drift.iter().cloned());
    let mut certified = PreflightCertified::certify(rebuilt, seams.preflight)?;
    steps.push(RecoveryStep::C);

    // (f) the terminals this build does not implement, refused before any
    // append — which is why it precedes (d) and (e) rather than sitting in its
    // own numbered position: a refusal after two appends is not "before any
    // append".
    refuse_unimplemented_terminals(&certified, seams.manager)?;

    // T-RUNSTART's P7/P8 repair, after (f) and before the first append. The
    // module comment argues each bound; the one that is not merely tidy is (f),
    // because a prefix with an unresolved integration transaction can have its
    // ref still sitting at the recorded base, and "present == base continue"
    // would adopt it under a transaction this build cannot resolve.
    ensure_recorded_integration_ref(&certified, seams.refs, hooks)?;

    // The append-error protocol's two ledgers. The recovery order takes no
    // provisional reservation and registers no invocation of its own — (c)'s
    // probes are the Runner's and are reclaimed there — so on this path both are
    // empty and the protocol cancels nothing. They exist here rather than inside
    // `emit` because "nothing was held" has to be an observation the ledgers
    // make, not an assumption the emitter is written around.
    let mut reservations = Reservations::new();
    let mut invocations = InvocationLedger::new();
    let mut context = EmitContext {
        clock: seams.clock,
        hooks,
        inputs: seams.inputs.clone(),
        reservations: &mut reservations,
        invocations: &mut invocations,
        warnings,
    };
    // (d), (e) — recovery events, every one of them before (h).
    let interrupted = settle_interrupted(&mut certified, &mut context)?;
    steps.push(RecoveryStep::D);
    let retained_closed = close_retained_idle(&mut certified, &mut context)?;
    steps.push(RecoveryStep::E);

    // (f)'s converging half, which **appends** and so cannot sit with its
    // refusing half above: erratum E6 puts the settled-but-unrecorded candidate
    // in `T-CAND-REF`, and that row converges forward. The refusal that stays
    // before every append is the integration transaction's, which is PR8's
    // terminal and one of the two `checkpoint_refusals` authorises.
    let promoted = complete_promotions(&mut certified, seams.manager, &mut context)?;

    // (g) — after (e) and before (h), the packet's own position. A worktree
    // effect and not an append, so it takes no `EmitContext`; the borrow of
    // `hooks` that `context` holds ends here for the same reason the step must
    // run before (h): `run_resumed` consumes the witness this step reads.
    steps.push(RecoveryStep::F);
    let recreated = recreate_open_no_attempt(&certified, seams.manager, context.hooks)?;
    steps.push(RecoveryStep::G);

    // (h) — and the witness is consumed here.
    let (resumed, handle) = run_resumed(certified, &mut context, seams.incarnation)?;
    steps.push(RecoveryStep::H);
    Ok((
        Recovered {
            interrupted,
            retained_closed,
            promoted,
            recreated,
            steps,
            resumed,
            warnings: drift,
        },
        handle,
    ))
}

// ---------------------------------------------------------------------------
// Step (b) — the refusal, and only the refusal
// ---------------------------------------------------------------------------

/// Step (b): "if the fold outcome is Complete or Halted: terminal finalization
/// then refuse continuation" — **PR7 implements the refusal**.
///
/// `RunDir.WriteReport` carries `fault_row: t_finalize`, which is not one of
/// this slice's eleven rows, so a lane that finalized here would write an
/// out-of-row effect with no fault coverage in this slice. The finalization is
/// therefore deferred and this is the half that is in range:
/// `refusal_condition`'s "continuation of Complete or Halted after
/// finalization".
///
/// Read from the barrier-proven fold and nowhere else — that is what O18's
/// "before any promotion, cleanup, admission, or report" buys, and a (b) that
/// consulted a fold built anywhere else would be deciding a run's outcome from
/// bytes nobody proved.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] when the proven prefix ends in `run_finished`
/// with [`RunOutcome::Complete`] or [`RunOutcome::Halted`].
pub fn refuse_if_finished(censused: &ResumeCensused) -> Result<(), UpstrokeError> {
    let Some(outcome) = censused.barrier().fold().finished() else {
        return Ok(());
    };
    match outcome {
        RunOutcome::Complete | RunOutcome::Halted => Err(UpstrokeError::Refused {
            message: format!(
                "this run already finished as `{}`, and a finished run does not continue. \
                 Recovery step (b) finalizes such a run and then refuses continuation; this \
                 build performs the refusal and leaves finalization to the slice that owns \
                 `RunDir.WriteReport`'s fault row, so nothing was written and nothing was \
                 deleted.",
                outcome_name(outcome)
            ),
        }),
        // Parked and BudgetExceeded are resumable outcomes: the fold's own
        // guard lets `run_resumed` through for exactly these two, which is what
        // makes "raise the ceiling and resume" the response to a budget stop.
        RunOutcome::Parked | RunOutcome::BudgetExceeded => Ok(()),
    }
}

/// Step (f)'s checkpoint refusal.
///
/// `checkpoint_refusals`: "an intermediate build refuses, **before any
/// append**, any operation whose terminals it does not implement (PR7:
/// integration and run end beyond refusal)". A proven prefix that leaves a
/// promotion or an integration transaction unresolved is exactly such an
/// operation: completing it means `task_candidate_created` or a CAS, and PR7
/// implements neither terminal.
///
/// Takes `&PreflightCertified` because it is a step-(f) emitter's predicate and
/// every recovery emitter takes one — the refusal has to be reachable from the
/// same place the append would have been, or it is refusing somewhere else.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] naming the task whose generation is `Promoting`,
/// or the unresolved integration transaction.
pub fn refuse_unimplemented_terminals(
    certified: &PreflightCertified,
    manager: &WorkspaceManager,
) -> Result<(), UpstrokeError> {
    let fold = fold_of(certified);
    if fold.transaction().is_some() {
        return Err(UpstrokeError::Refused {
            message: "the proven prefix leaves an integration transaction unresolved. Recovery \
                      step (f) completes authorized publications, and this build implements no \
                      integration terminal, so it refuses before any append rather than \
                      resolving a transaction it cannot finish."
                .to_owned(),
        });
    }
    // **A `Promoting` generation whose pin is gone.** E6 converges the ordinary
    // window by reconstructing the commit identity from the pin. With no pin
    // there is nothing to reconstruct from, and a settled attempt with no pin is
    // neither `T-CAND-OBJ` (which leaves an unpinned object to Git) nor a
    // completable `T-CAND-REF`.
    //
    // Refused **here**, with the other refusal, because a refusal belongs before
    // any effect and this one is a predicate over durable state alone. Leaving
    // it to the converging half would put it after P7/P8 publishes the ref,
    // which is the ordering `the_p7_p8_step_runs_after_the_refusals_that_bound_it`
    // exists to hold.
    let run_id = fold
        .started()
        .map(|started| started.run_id.clone())
        .unwrap_or_default();
    for key in task_keys(fold) {
        let Some(generation) = fold.task(key).and_then(|task| {
            task.generations
                .iter()
                .find(|held| held.class == GenerationClass::Promoting && held.candidate.is_none())
        }) else {
            continue;
        };
        let names =
            crate::engine::topology::candidate::CandidateNames::of(&run_id, key, generation.id);
        if manager
            .direct_ref_target(names.prepared_ref.as_str())?
            .is_none()
        {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "task {key} is promoting and its candidate pin is absent, so the commit its \
                     settlement authorised cannot be named. `T-CAND-OBJ` governs an unpinned \
                     object and leaves it to Git; a settled attempt with no pin is neither row \
                     and is refused before any effect rather than guessed"
                ),
            });
        }
    }

    // **A `Promoting` generation WITH its pin is no longer refused here.** It was, and that
    // was a third checkpoint refusal: `checkpoint_refusals` authorises this
    // build to refuse exactly two things, integration and run end, and a
    // generation whose settlement is durable is neither. Erratum **E6** places
    // the window in `T-CAND-REF` — whose boundary begins at the settlement, not
    // at `candidate_prepared` — and that row converges forward. The convergence
    // is [`complete_promotions`], and it appends, so it runs with the other
    // appending steps rather than here before any of them.
    Ok(())
}

/// One `Promoting` generation whose `candidate_prepared` never landed.
struct Pending {
    key: TaskKey,
    generation: GenerationId,
    base_sha: CommitSha,
    record: Box<AttemptRecord>,
}

/// The generation this task is promoting, if its candidate was never recorded.
///
/// `GenerationFold::candidate` is `Some` once `candidate_prepared` applied, so
/// `Promoting` with `None` is exactly erratum **E6**'s window and nothing else.
fn promoting_without_candidate(
    fold: &TopologyFold,
    events: &[TopologyEvent],
    key: TaskKey,
) -> Option<Pending> {
    let generation = fold
        .task(key)?
        .generations
        .iter()
        .find(|held| held.class == GenerationClass::Promoting && held.candidate.is_none())?;
    // The record the settlement carried, from the proven bytes.
    let record = events.iter().rev().find_map(|event| match &event.body {
        TopologyEventBody::AttemptFinished { data }
            if data.key == key && data.generation == generation.id =>
        {
            Some(data.record.clone())
        }
        _ => None,
    })?;
    Some(Pending {
        key,
        generation: generation.id,
        base_sha: generation.base_sha.clone(),
        record,
    })
}

/// **Recovery step (f), the converging half.** Complete every `Promoting`
/// generation whose `candidate_prepared` never landed.
///
/// # Erratum E6
///
/// `T-CAND-OBJ`'s window ends where its own `durable_state` says: "attempt_started
/// only", with the attempt unsettled. `T-CAND-REF`'s `boundary` begins at the
/// settlement — **not** at `candidate_prepared`, which is what the text said
/// before E6 and which left the prefix "settlement durable, `candidate_prepared`
/// absent" governed by no row at all. The fold makes that prefix mandatory:
/// `attempt_finished{Closed{Succeeded}}` is the only thing that sets `Promoting`
/// and `check_candidate_prepared` refuses every other class, so the two appends
/// cannot be collapsed.
///
/// **Every input is derived from durable state and nothing is re-decided.** The
/// pin names the commit; the commit names its tree and its message; the
/// generation names the base; `diff-tree base commit` names the region the diff
/// actually touched, which is the same primitive
/// `decisions.admission_and_leases.path_policy.actual` specifies. The attempt
/// record is the one the durable settlement already carried.
///
/// After the append the generation is exactly what `T-CAND-REF`'s existing
/// `resume_action` describes, and the rest of that row — verify object, create
/// the exact candidates ref, append `task_candidate_created`, prune the pin —
/// is unchanged.
///
/// # Errors
///
/// A Git error reading the pin or the commit, a refusal when the pin is missing
/// (the object is then Git's and `T-CAND-OBJ` governs), or whatever the append
/// returns.
pub fn complete_promotions(
    certified: &mut PreflightCertified,
    manager: &WorkspaceManager,
    context: &mut EmitContext<'_>,
) -> Result<Vec<TaskKey>, UpstrokeError> {
    let mut converged = Vec::new();
    let run_id = fold_of(certified)
        .started()
        .ok_or_else(|| UpstrokeError::Refused {
            message: "the proven prefix has no run".to_owned(),
        })?
        .run_id
        .clone();

    // **The barrier's own parse**, for the one thing the fold does not keep: the
    // `AttemptRecord` the durable settlement carried. `candidate_prepared`
    // records it, and inventing one would re-decide what the settlement already
    // decided.
    //
    // Read through `StablePrefix::events` rather than parsing the log again.
    // A second parse is reachable around the barrier by anyone, which is what
    // `the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold`
    // refuses — and it refused this convergence's first draft.
    let pendings: Vec<Pending> = {
        let barrier = certified.rebuilt().censused().barrier();
        let events = barrier.events();
        let fold = barrier.fold();
        task_keys(fold)
            .into_iter()
            .filter_map(|key| promoting_without_candidate(fold, events, key))
            .collect()
    };

    for pending in pendings {
        let key = pending.key;
        let names = crate::engine::topology::candidate::CandidateNames::of(
            &run_id,
            key,
            pending.generation,
        );
        // Step (f)'s refusing half already proved the pin is there, before any
        // effect, and nothing between them touches a ref.
        let Some(commit) = manager.direct_ref_target(names.prepared_ref.as_str())? else {
            return Err(UpstrokeError::Refused {
                message: format!("task {key}'s candidate pin vanished during recovery"),
            });
        };
        let (tree, message) = manager.commit_identity(&commit)?;
        let actual_paths = manager.changed_paths_between(pending.base_sha.as_str(), &commit)?;

        let prepared = CandidatePrepared {
            key,
            generation: pending.generation,
            attempt: pending.record,
            base_sha: pending.base_sha.clone(),
            parent_sha: pending.base_sha,
            tree_sha: CommitSha(tree),
            commit_sha: CommitSha(commit),
            message,
            prepared_ref: names.prepared_ref,
            candidate_ref: names.candidate_ref,
            actual_paths: actual_paths.clone(),
            lease_effect: CandidateLeaseEffect::ReplacesPredicted {
                paths: actual_paths,
            },
        };
        emit(
            certified,
            context,
            TopologyEventBody::CandidatePrepared {
                data: Box::new(prepared),
            },
        )?;
        converged.push(key);
    }
    Ok(converged)
}

// ---------------------------------------------------------------------------
// T-RUNSTART's P7/P8 repair — a durable effect, and not an event
// ---------------------------------------------------------------------------

/// `transaction_fault_matrix[T-RUNSTART].resume_action`: "**P7/P8: create the
/// ref zero-old at the recorded base if absent; if present == base continue (no
/// spend repeats)**".
///
/// A run killed between P6 and P8 is committed — `run_started(4)` is durable and
/// `committed.json` names its digest — but has no `integration_ref`. Nothing
/// else in this build creates one, so without this step such a run resumes into
/// a namespace its own record describes and the repository does not have.
///
/// **The body is P8's, called rather than copied.**
/// [`super::create::ensure_integration_ref`] answers all three dispositions —
/// absent, present at the base, present at anything else — and its doc states
/// why there may be only one of it. This function contributes the two
/// arguments and nothing else; if it ever grows a comparison of its own,
/// that is the duplication the shared body exists to prevent.
///
/// **Both arguments come from the record.** `run_started(4).integration_ref` and
/// `run_started(4).base_sha`, reached through the witness chain from the
/// committed first line (a0) read and (a) authenticated against
/// `committed.json.run_started_sha256`. Not from today's `[runner]` selection,
/// not from a `Workspace`, and not from the fold's current view of the run: a
/// resume that recomputed either would be able to publish a ref the run was
/// never started against.
///
/// Takes `&PreflightCertified` for the same reason
/// [`refuse_unimplemented_terminals`] does — it is what makes "after (c)"
/// unstateable as anything else — and returns `()` rather than a witness because
/// nothing downstream may depend on it having run: it is a repair of a prefix,
/// not a link in the order.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] when the recorded ref is symbolic, checked out in
/// some worktree, or already at any SHA other than the recorded base; a Git
/// error from the creation itself, including the zero-old failure when the ref
/// appeared between the read and the write.
pub fn ensure_recorded_integration_ref(
    certified: &PreflightCertified,
    refs: &dyn IntegrationRefs,
    hooks: &mut dyn TopologyHooks,
) -> Result<(), UpstrokeError> {
    let started = started_of(certified);
    ensure_integration_ref(
        refs,
        hooks.effects(),
        started.integration_ref.as_str(),
        started.base_sha.as_str(),
    )
}

// ---------------------------------------------------------------------------
// The recovery events, (d) through (h)
// ---------------------------------------------------------------------------

/// What one recovery event append needs beyond the witness.
///
/// Bundled because they travel together at every emitter and a signature that
/// spelled them out four times is four places for one of them to go missing.
///
/// Everything below `hooks` is here because [`super::emit::emit`] needs it, and
/// it needs it because the append-error protocol does: the barrier it
/// establishes at obligation (5) is established over `inputs` and the committed
/// first line's digest, and obligations (2) and (3) are `cancel_any` and
/// `cancel_all_running` on these two ledgers. Passing them in rather than
/// making them here is what lets a caller that *does* hold a reservation or a
/// running invocation have it cancelled — "recovery holds neither today" is a
/// fact about today's callers, not a licence to drop the obligation.
pub struct EmitContext<'a> {
    /// Where a durable event's timestamp comes from. Seamed so a byte-exact
    /// assertion over the log is possible at all.
    pub clock: &'a dyn TimeSource,
    /// The five effect-hook families. The Event funnel's are what a
    /// `T-APPEND` fault test arms.
    pub hooks: &'a mut dyn TopologyHooks,
    /// The frozen plan and its digest. The protocol's reopened barrier is
    /// established over exactly these — the same two inputs recovery step (a1)
    /// used — so a protocol that took its own copy could prove a prefix against
    /// a plan the run was never folded from.
    pub inputs: FrozenInputs,
    /// The provisional-reservation ledger. `cancel_any` on any outcome-unknown
    /// append.
    pub reservations: &'a mut Reservations,
    /// The invocation ledger. Every still-running entry is cancelled; the
    /// Runner half of that is the caller's.
    pub invocations: &'a mut InvocationLedger,
    /// Where the protocol's reopen reports a torn-tail normalization.
    pub warnings: &'a mut Vec<String>,
}

/// (d) Settle every in-flight identity interrupted.
///
/// `recovery_order` (d), and `T-ATTEMPT`'s resume action: an attempt whose
/// coordinator died is not retried in place, it is settled `interrupted` and
/// its generation closed, so the next dispatch opens a fresh generation at the
/// task's base.
///
/// # Errors
///
/// Whatever [`emit`] refuses or fails at.
pub fn settle_interrupted(
    certified: &mut PreflightCertified,
    context: &mut EmitContext<'_>,
) -> Result<usize, UpstrokeError> {
    let mut settled = 0;
    for (key, generation, attempt, lease) in in_flight(fold_of(certified)) {
        let body = TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key,
                generation,
                attempt,
                lease,
                detail: "the coordinator that started this attempt did not survive it; recovery \
                         step (d) settles every in-flight identity interrupted before any \
                         resume"
                    .to_owned(),
            },
        };
        emit(certified, context, body)?;
        settled += 1;
    }
    Ok(settled)
}

/// (e) Close every `RetainedIdle` generation with
/// `generation_closed{ResumeDiscardsRetainedSession}`.
///
/// A retained session belongs to the incarnation that retained it, and this is
/// not that incarnation: `T-RESUME`'s authoritative state says "retained_session
/// authority already invalid for the new incarnation", so the generation closes
/// rather than being resumed into.
///
/// # Errors
///
/// Whatever [`emit`] refuses or fails at.
pub fn close_retained_idle(
    certified: &mut PreflightCertified,
    context: &mut EmitContext<'_>,
) -> Result<usize, UpstrokeError> {
    let mut closed = 0;
    for (key, generation, lease) in retained_idle(fold_of(certified)) {
        let body = TopologyEventBody::GenerationClosed {
            data: GenerationClosed {
                key,
                generation,
                reason: GenerationCloseReason::ResumeDiscardsRetainedSession,
                lease,
            },
        };
        emit(certified, context, body)?;
        closed += 1;
    }
    Ok(closed)
}

/// (h) `run_resumed(4)` — and the step that **consumes** the pre-flight
/// witness.
///
/// O33 is "recovery events before `run_resumed`", and this signature is that
/// clause: `certified` is taken **by value**, so no emitter of a recovery event
/// can present a `PreflightCertified` after this returns. The witness is gone.
///
/// INV-23: "`run_resumed(4).runner` records what the incarnation established
/// and **must equal `run_started(4).runner` exactly** (a `FoldError`
/// otherwise)". The value written is [`RunnerRebuilt::policy`], which
/// `rebuild_by_inspection` returns as the record field for field — so the
/// equality is a property of the rebuild rather than a comparison this function
/// performs and could get wrong.
///
/// # Errors
///
/// Whatever [`emit`] refuses or fails at — including the fold's own
/// `RunnerMoved` refusal if a runner identity ever reached here that did not
/// equal the record's.
pub fn run_resumed(
    mut certified: PreflightCertified,
    context: &mut EmitContext<'_>,
    incarnation: &IncarnationId,
) -> Result<(Resumed, RunHandle), UpstrokeError> {
    let body = TopologyEventBody::RunResumed {
        data: Box::new(RunResumed4 {
            incarnation: incarnation.clone(),
            runner: certified.rebuilt().policy().clone(),
            probed_agents: certified.probed_agents().to_vec(),
            upstroke_version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
    };
    emit(&mut certified, context, body)?;
    let fold = fold_of(&certified);
    let resumed = Resumed {
        epoch: fold.epoch().map_or(0, |epoch| epoch.0),
        budget_stop_cleared: fold.budget_stop().is_none(),
    };

    // The witness is spent; what it was carrying is not. Unwound rather than
    // dropped, because everything below it is the run's own state and the loop
    // is the thing that needs it — see [`RunHandle`].
    let (log, fold, records) = certified
        .into_rebuilt()
        .into_censused()
        .into_barrier()
        .into_log_fold_and_records();
    // Taken before the records are unwound: the digest step (a) verified is the
    // one the loop's own appends must be able to check themselves against.
    let committed_first_line_sha256 = records.commit().run_started_sha256.clone();
    let (run_lock, worktree_lock, root) = records.into_locks().into_guards();
    Ok((
        resumed,
        RunHandle {
            started: root.started().clone(),
            committed_first_line_sha256,
            log,
            fold,
            _run: run_lock,
            _worktree: worktree_lock,
        },
    ))
}

/// The run's own state, handed from a completed start to the loop that drives
/// it.
///
/// **Every field of this was being dropped at the end of the recovery order**,
/// and that is the mechanical reason `TopologyRun` could not exist. Not a
/// missing function — a missing *value*. `run_resumed` consumed the last
/// witness and returned a two-field summary, so the append handle the barrier
/// had just entitled the command to, the fold built from exactly the proven
/// bytes, and both locks died with it.
///
/// Each of the three matters for a different reason:
///
/// - **The log** is the handle `(a1)` proved. A loop that reopened the log
///   would append to a prefix its own barrier never proved, which is the whole
///   of what the barrier exists to prevent.
/// - **The fold** is derived from exactly the bytes the barrier synced, reread
///   and replayed. Rebuilding it anywhere else is a second derivation that can
///   disagree with the first.
/// - **The locks** make this process the run's only writer. A loop that had to
///   retake them would be racing itself, and `run_creation` requires the
///   worktree lock held "across the startup census **and the whole run**".
///
/// The lock fields keep their `_` names and stay private: nothing reads them,
/// they exist to be dropped, and they drop in declaration order so the run lock
/// is released before the worktree lease. All this changes is *when* — the end
/// of the loop rather than the end of recovery.
pub struct RunHandle {
    /// The digest recovery verified `committed.json.run_started_sha256` against.
    ///
    /// **Carried because the loop's appends need it too.** The append-error
    /// protocol's creator disposition is a projection of the outcome onto the
    /// run's commitment boundary, and without this the loop's `RunIdentity`
    /// answers `None` where recovery's own emitter answers `Some` — two
    /// emitters of one run disagreeing about whether it is committed.
    pub committed_first_line_sha256: String,
    /// The append handle the stable-prefix barrier entitled this command to.
    pub log: crate::events::log::EventLog,
    /// The fold built from exactly the barrier-proven bytes.
    pub fold: TopologyFold,
    /// The record the run started from, which the loop's emitter stamps from.
    pub started: RunStarted4,
    _run: RunLock,
    _worktree: WorktreeLock,
}

impl std::fmt::Debug for RunHandle {
    /// Names the run and says nothing about the locks.
    ///
    /// A derived `Debug` would print two guards whose whole contract is that
    /// nothing reads them, and a lock that appears in a log line is a lock
    /// someone will eventually reason about.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunHandle")
            .field("run_id", &self.started.run_id)
            .field("poisoned", &self.fold.is_poisoned())
            .finish_non_exhaustive()
    }
}

/// What (h) established, for the caller that reports it.
///
/// A value rather than `()` so that "the epoch's budget stop cleared" and
/// "state.resumes increments" are things a test asserts about the fold rather
/// than about the absence of an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resumed {
    /// The epoch this resume opened. `run_resumed` increments it.
    pub epoch: u32,
    /// Whether the previous epoch's budget stop is gone.
    pub budget_stop_cleared: bool,
}

/// `coordinator_integration.emit`, for one recovery event.
///
/// **One line of body, and that is the point.** Every recovery event — (d)'s
/// `attempt_interrupted`, (e)'s `generation_closed`, (h)'s `run_resumed` — is an
/// `Event.Append`, and `append_error_protocol` applies to `Event.Append` without
/// exception: poison the fold, `Reservations::cancel_any`,
/// `InvocationLedger::cancel_all_running`, no retry and no report from memory,
/// then reopen through `Event.OpenLog`, establish the stable-prefix barrier, and
/// end naming the run id, the event kind and **whether the proven prefix
/// contains the line** — present, absent, or undetermined.
///
/// [`super::emit::emit`] is those five obligations and the six steps above them.
/// This function is the call, and `dispatch.rs` states why it is only the call:
/// "a module that held the log would hold the append-error protocol with it …
/// and there would be two implementations of it, which is the duplication class
/// this crate has already paid for three times". [`super::create`] keeps one of
/// its own on purpose — `Event.AppendFirst` has to answer *absent first line* as
/// one of three creator dispositions rather than as a barrier failure — and the
/// recovery order has no such difference to justify a third.
///
/// The two shapes an open-coded version got wrong, recorded because they are
/// what a reader would otherwise reintroduce:
///
/// * a `FoldError` is a **refusal**, not [`UpstrokeError::EventLog`]. Nothing
///   was written, so an error naming the log file as an I/O path says the wrong
///   thing about what happened.
/// * a funnel `Err` **before the append was entered** — a poisoned handle, a
///   legacy handle, a site that is not this line's — must not poison the fold.
///   `emit` decides that from `EventLog::poisoned_at()` on both sides of the
///   call rather than from the error value, so "entered" is decidable and a
///   wrong-site refusal leaves the fold usable.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] for a transition the checked fold rejects — before
/// any write — and for an append that was entered and returned an error, in
/// which case the protocol has already run and the message carries its report.
/// [`UpstrokeError::Io`] or [`UpstrokeError::EventLog`] for a refusal the funnel
/// raised before entry.
fn emit(
    certified: &mut PreflightCertified,
    context: &mut EmitContext<'_>,
    body: TopologyEventBody,
) -> Result<(), UpstrokeError> {
    // Built before the mutable borrow of the chain below it, and from the
    // witness rather than from a caller: the run id is the one (a0) resolved and
    // the digest is the one (a) verified, so the protocol's barrier is the same
    // barrier over the same two inputs as recovery step (a1)'s.
    let records = certified.rebuilt().censused().barrier().records();
    let identity = RunIdentity {
        run_id: records.locks().root().run_id().to_owned(),
        inputs: context.inputs.clone(),
        committed_first_line_sha256: Some(records.commit().run_started_sha256.clone()),
    };

    let (log, fold) = certified
        .rebuilt_mut()
        .censused_mut()
        .barrier_mut()
        .writer();
    let mut state = EmitState {
        fold,
        log,
        reservations: context.reservations,
        warnings: context.warnings,
    };
    // Recovery holds the run's ledger, so it discharges obligation (3) here
    // rather than carrying it further: the recovery order is the one caller
    // that both emits and owns every invocation this process registered.
    super::emit::emit(&identity, &mut state, context.clock, body, context.hooks)
        .map(|_| ())
        .map_err(|error| super::emit::EmitFailure::from(error).discharging(context.invocations))
}

// ---------------------------------------------------------------------------
// Reading the proven fold
// ---------------------------------------------------------------------------

/// The fold the barrier proved, from any point in the chain below it.
fn fold_of(certified: &PreflightCertified) -> &TopologyFold {
    certified.rebuilt().censused().barrier().fold()
}

/// The committed `run_started(4)`, from the same point in the chain.
///
/// The record (a0) resolved and (a) authenticated against
/// `committed.json.run_started_sha256` — reached through the witnesses rather
/// than re-read, so the bytes a later step publishes a ref from are the bytes
/// the commit record proved.
fn started_of(certified: &PreflightCertified) -> &RunStarted4 {
    certified
        .rebuilt()
        .censused()
        .barrier()
        .records()
        .locks()
        .root()
        .started()
}

/// **(g)** Recreate `OpenNoAttempt` worktrees at their bases.
///
/// `decisions.sequential_substrate.recovery_order`: "(g) recreate
/// `OpenNoAttempt` worktrees at their bases (through `Worktree.Verify` or
/// forced recreate)". It is one of the order's eleven steps and it was the one
/// this module did not perform — the omission that `resume_open_no_attempt`,
/// written and tested by the dispatch lane, had no production caller for.
///
/// **The recovery this step performs is not chosen here.**
/// `decisions.workspace_candidates.generation` gives a failed verification two
/// different recoveries and says which applies is a property of the
/// generation's *class*: an `OpenNoAttempt` or repair worktree "is removed with
/// force and recreated", a `RetainedIdle` generation "is closed". This step
/// enumerates one class and hands each member to the single function that
/// implements that class's recovery. The retained class is (e)'s and reaches
/// `Worktree.Verify` through its own seam, so no retained worktree can arrive
/// here to be handed the recreate branch.
///
/// **Every field is read off the proven prefix, not invented — and the value
/// asks for nothing else.** `base` is the generation's recorded `base_sha`, and
/// the slot is [`task_slot`], which derives it from `{key, generation}` so no
/// two callers can disagree about which worktree a generation owns. There is no
/// third field to get wrong, and that is deliberate: the rebuild family takes
/// [`OpenGeneration`] rather than a full `Dispatched` precisely so recovery
/// never has to reconstruct a predicted region the fold does not hand back.
/// Inventing one would be a field that lies about a lease; reaching into
/// `src/topology/`'s lease table for the real one would be an edit to PR3's
/// layer for a value no path below this one reads.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] for a generation whose lease is an inherited
/// lineage — a repair, whose resume action is to re-materialize its source
/// candidate, and whose source the fold does not retain. `checkpoint_refusals`
/// gives repair execution to PR8, so this build refuses rather than
/// reconstructing a materialization it cannot prove. **The arm is unreachable
/// in this slice and both walls are measured** by
/// `a_repair_generation_cannot_reach_step_g_in_this_slice`: the fold refuses an
/// inherited lease on an ordinary task at the barrier's checked replay, and
/// `TaskRegistry::originals_with_agents` gives every entry `lineage: None`, so
/// there is no task the lease would be legal on. That test fails the day a
/// slice admits repairs, which is when this arm becomes reachable. Also
/// refused when a generation holding its lease has no recorded region, which
/// is a fold that disagrees with itself rather than a state to guess at.
/// Otherwise the containment refusals or a Git error from
/// [`resume_open_no_attempt`].
pub fn recreate_open_no_attempt(
    certified: &PreflightCertified,
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
) -> Result<Vec<(TaskKey, GenerationId, Reuse)>, UpstrokeError> {
    let mut rebuilt = Vec::new();
    for open in open_no_attempt(fold_of(certified))? {
        let reuse = resume_open_no_attempt(manager, hooks, &open)?;
        rebuilt.push((open.key, open.generation, reuse));
    }
    Ok(rebuilt)
}

/// Every `OpenNoAttempt` generation the proven prefix records, with what a
/// rebuild of it needs.
///
/// Sibling of [`retained_idle`] and [`in_flight`], and deliberately shaped like
/// them: one enumerator per generation class, so "which class does this step
/// act on" is a property of the function the step calls rather than of a
/// predicate the step re-derives. Two rules that can disagree is the shape this
/// slice has paid for repeatedly.
fn open_no_attempt(fold: &TopologyFold) -> Result<Vec<OpenGeneration>, UpstrokeError> {
    let mut found = Vec::new();
    for key in task_keys(fold) {
        let Some(task) = fold.task(key) else { continue };
        for generation in &task.generations {
            if !matches!(generation.class, GenerationClass::OpenNoAttempt) {
                continue;
            }
            if let GenerationLease::InheritedLineage { root } = generation.lease {
                return Err(UpstrokeError::Refused {
                    message: format!(
                        "task {} generation {} is a repair executing inside lineage {}'s lease, \
                         and its resume action is to re-materialize the candidate it was \
                         dispatched from, which the fold does not record; repair execution is \
                         not implemented by this build",
                        key.0, generation.id.0, root.0
                    ),
                });
            }
            found.push(OpenGeneration {
                key,
                generation: generation.id,
                base: generation.base_sha.clone(),
                slot: task_slot(key, generation.id),
                // `None` is not a guess. An ordinary generation has no
                // materialization to reproduce, and the repair case returned
                // above rather than reaching here — so the field is decided by
                // the same match that decided the refusal, and there is no
                // third path that could leave it wrong.
                source: None,
            });
        }
    }
    Ok(found)
}

/// Every task key the registry holds.
fn task_keys(fold: &TopologyFold) -> Vec<TaskKey> {
    fold.registry()
        .map(|registry| {
            (0..registry.len())
                .map(|index| TaskKey(index as u32))
                .collect()
        })
        .unwrap_or_default()
}

/// Every `(key, generation, attempt)` whose attempt was running when the last
/// coordinator died.
fn in_flight(fold: &TopologyFold) -> Vec<(TaskKey, GenerationId, AttemptNumber, LeaseDisposition)> {
    let mut found = Vec::new();
    for key in task_keys(fold) {
        let Some(task) = fold.task(key) else { continue };
        for generation in &task.generations {
            if let GenerationClass::InFlight { attempt } = generation.class {
                // `survives: false` — "the generation does *not* survive an
                // interruption" (T-ATTEMPT: generation Closed). The disposition
                // is therefore the lease's own answer to that question rather
                // than a constant, which is what keeps a lineage member
                // recording `LineageHeld` where an ordinary generation records
                // `PredictedReleased`.
                found.push((
                    key,
                    generation.id,
                    attempt,
                    generation.lease.expected(false),
                ));
            }
        }
    }
    found
}

/// Every `(key, generation)` settled holding a session.
fn retained_idle(fold: &TopologyFold) -> Vec<(TaskKey, GenerationId, LeaseDisposition)> {
    let mut found = Vec::new();
    for key in task_keys(fold) {
        let Some(task) = fold.task(key) else { continue };
        for generation in &task.generations {
            if matches!(generation.class, GenerationClass::RetainedIdle { .. }) {
                found.push((key, generation.id, generation.lease.expected(false)));
            }
        }
    }
    found
}

/// The outcome as `run_finished` writes it.
fn outcome_name(outcome: &RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Complete => "complete",
        RunOutcome::Parked => "parked",
        RunOutcome::Halted => "halted",
        RunOutcome::BudgetExceeded => "budget_exceeded",
    }
}

#[cfg(test)]
mod tests;
