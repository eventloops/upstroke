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
use crate::events::RunOutcome;
use crate::events::log::TopologyLine;
use crate::rundir::RepoKey;
use crate::runner::container::GitView;
use crate::runner::container::resolve::RunnerPreflight;
use crate::runner::container::runtime::{ContainerRuntime, OwnerLiveness};
use crate::topology::events::{
    AttemptInterrupted4, AttemptNumber, GenerationCloseReason, GenerationClosed, GenerationId,
    IncarnationId, LeaseDisposition, RunResumed4, TopologyEvent, TopologyEventBody,
};
use crate::topology::fold::{FrozenInputs, GenerationClass, TopologyFold};
use crate::topology::registry::TaskKey;

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
                let (log, bytes, fold) = prefix.into_log_and_fold();
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

            /// The fold built from exactly the proven bytes.
            #[must_use]
            pub fn fold(&self) -> &TopologyFold {
                &self.fold
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
        use crate::rundir::{self, HuskReport, RepoKey};
        use crate::runner::container::GitView;
        use crate::runner::container::census::{
            Census, CensusComplete, CensusReport, CensusStart, run_startup_census,
        };
        use crate::runner::container::runtime::{ContainerRuntime, OwnerLiveness};
        use crate::topology::events::IncarnationId;

        use crate::engine::topology::seams::TopologyHooks;

        /// The census of step (a), and the barrier it was decided under.
        ///
        /// **The census returns the witness; it does not get wrapped.** A
        /// wrapper would prove possession — the holder had a census result and
        /// a barrier, in either order — and the packet requires the barrier
        /// *first*: "a resume takes its run lock first, establishes the
        /// stable-prefix barrier of recovery step (a1), **then** censuses". The
        /// constructor consuming [`BarrierHeld`] by value is that ordering, as
        /// a call.
        ///
        /// The run-directory half reuses [`rundir::husk_report`], which is
        /// already what `status` drives. **One classifier, two callers** — the
        /// packet requires a husk "retained and reported with its locator and
        /// reason by every census **and by status**", and a second classifier
        /// would drift from the first.
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

        #[derive(Debug)]
        pub struct ResumeCensused {
            barrier: BarrierHeld,
            containers: CensusComplete,
            husks: Vec<HuskReport>,
        }

        impl ResumeCensused {
            /// Census under `barrier`: containers first, then run directories,
            /// then this run's own stale marker.
            ///
            /// The marker removal is the one *write* of this step and it is
            /// last, because `resource_accounting` has a stale marker removed
            /// "by a census with the lock free **or** by its owner on resume" —
            /// this is the owner, and it may only act after the barrier proved
            /// the prefix that says the run committed.
            ///
            /// # Errors
            ///
            /// [`UpstrokeError::Refused`] from the container census — an
            /// unreachable runtime with intents present, an intent naming this
            /// process's own incarnation, an unreclaimable dead owner — or from
            /// the marker removal.
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
                let public = barrier.records().locks().root().public_dir().to_path_buf();

                // (i) Containers, including every earlier incarnation of this
                // run under `<R>/containers`. The start value carries the
                // barrier this module derived, so `CensusStart::Resume` cannot
                // be built here without one.
                let start = CensusStart::Resume {
                    run_id,
                    incarnation: incarnation.0.clone(),
                    barrier: barrier.stable_prefix_barrier(),
                };
                let containers = run_startup_census(
                    hooks.container(),
                    &Census {
                        private_root: &private_root,
                        start: &start,
                        runtime: *runtime,
                        liveness: *liveness,
                        view: *view,
                    },
                )?;

                // (ii) Run directories: every husk classified and reported,
                // never deleted here.
                let husks = rundir::list_husks(repo_root)
                    .into_iter()
                    .map(|id| rundir::husk_report(repo_root, &id, repo_key, &private_root))
                    .collect();

                // (iii) This run's own stale marker, removed by its owner.
                rundir::remove_marker(&public, hooks.rundir())?;

                Ok(Self {
                    barrier,
                    containers,
                    husks,
                })
            }

            /// The barrier this census was decided under.
            #[must_use]
            pub fn barrier(&self) -> &BarrierHeld {
                &self.barrier
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

            /// Every husk, with its locator and its reason. Reported, never
            /// deleted.
            #[must_use]
            pub fn husks(&self) -> &[HuskReport] {
                &self.husks
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
    pub clock: &'a dyn TimeSource,
}

/// What one completed recovery did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// (d): how many in-flight identities were settled interrupted.
    pub interrupted: usize,
    /// (e): how many `RetainedIdle` generations were closed.
    pub retained_closed: usize,
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
/// before any recovery event); or an append error at (d)–(h), after which the
/// fold is poisoned and the next resume repeats from (a0).
pub fn run_recovery_order(
    root: RootDerived,
    seams: &ResumeSeams<'_>,
    hooks: &mut dyn TopologyHooks,
    warnings: &mut Vec<String>,
) -> Result<Recovered, UpstrokeError> {
    // (a) the two locks, then the two records — before any private write.
    let locks = LocksHeld::take(
        root,
        seams.repo_root,
        seams.worktree_git_dir,
        hooks.rundir(),
    )?;
    let records = RecordsVerified::verify(locks, seams.repo_key)?;

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

    // (c) the recorded Runner by inspection, then its probes.
    let rebuilt = RunnerRebuilt::rebuild(censused, seams.today, Some(seams.runtime))?;
    let drift: Vec<String> = rebuilt.warnings().to_vec();
    warnings.extend(drift.iter().cloned());
    let mut certified = PreflightCertified::certify(rebuilt, seams.preflight)?;

    // (f) the terminals this build does not implement, refused before any
    // append — which is why it precedes (d) and (e) rather than sitting in its
    // own numbered position: a refusal after two appends is not "before any
    // append".
    refuse_unimplemented_terminals(&certified)?;

    let mut context = EmitContext {
        clock: seams.clock,
        hooks,
    };
    // (d), (e) — recovery events, every one of them before (h).
    let interrupted = settle_interrupted(&mut certified, &mut context)?;
    let retained_closed = close_retained_idle(&mut certified, &mut context)?;

    // (h) — and the witness is consumed here.
    let resumed = run_resumed(certified, &mut context, seams.incarnation)?;
    Ok(Recovered {
        interrupted,
        retained_closed,
        resumed,
        warnings: drift,
    })
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
pub fn refuse_unimplemented_terminals(certified: &PreflightCertified) -> Result<(), UpstrokeError> {
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
    for key in task_keys(fold) {
        if let Some(task) = fold.task(key) {
            if task
                .generations
                .iter()
                .any(|generation| generation.class == GenerationClass::Promoting)
            {
                return Err(UpstrokeError::Refused {
                    message: format!(
                        "task {key} has a generation in promotion. Recovery step (f) completes \
                         Promoting promotions, and this build implements no promotion terminal, \
                         so it refuses before any append."
                    ),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The recovery events, (d) through (h)
// ---------------------------------------------------------------------------

/// What one recovery event append needs beyond the witness.
///
/// Bundled because the three travel together at every emitter and a signature
/// that spelled them out four times is four places for one of them to go
/// missing.
pub struct EmitContext<'a> {
    /// Where a durable event's timestamp comes from. Seamed so a byte-exact
    /// assertion over the log is possible at all.
    pub clock: &'a dyn TimeSource,
    /// The five effect-hook families. The Event funnel's are what a
    /// `T-APPEND` fault test arms.
    pub hooks: &'a mut dyn TopologyHooks,
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
) -> Result<Resumed, UpstrokeError> {
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
    Ok(Resumed {
        epoch: fold.epoch().map_or(0, |epoch| epoch.0),
        budget_stop_cleared: fold.budget_stop().is_none(),
    })
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
/// > build event -> serialize -> round-trip -> `plan_transition` -> append the
/// > exact bytes through the Event funnel (written, then synced; the newline is
/// > the commit marker) -> `apply_delta` **only after** the funnel returned Ok;
/// > a `FoldError` aborts before any write; an `Err` returned by the funnel
/// > after the append was entered runs the `append_error_protocol`.
///
/// The round-trip is [`TopologyLine::round_trip`], which is the only way to
/// make bytes the funnel accepts, so "append the exact bytes" is structural.
/// [`TopologyFold::poison`] is called explicitly on the error path: after an
/// append whose outcome is unknown, "every later transition attempt in this
/// process is refused", and a fold that merely stopped being used would be one
/// edit away from being used again.
///
/// # Errors
///
/// The [`crate::topology::fold::FoldError`] of a transition that does not
/// apply — before any write — or the Event funnel's error, after which the
/// fold is poisoned and the command ends.
fn emit(
    certified: &mut PreflightCertified,
    context: &mut EmitContext<'_>,
    body: TopologyEventBody,
) -> Result<(), UpstrokeError> {
    let event = TopologyEvent {
        ts: context.clock.now_rfc3339(),
        body,
    };
    let site = crate::events::log::site_for(&event.body);
    let (line, checked) = TopologyLine::round_trip(&event)?;
    let (log, fold) = certified
        .rebuilt_mut()
        .censused_mut()
        .barrier_mut()
        .writer();
    // A FoldError aborts before any write.
    let delta = fold
        .plan_transition(&checked)
        .map_err(|error| UpstrokeError::EventLog {
            path: log.path().to_path_buf(),
            message: error.to_string(),
        })?;
    match log.append_topology_hooked(site, &line, context.hooks.events()) {
        Ok(()) => {
            fold.apply_delta(delta);
            Ok(())
        }
        Err(error) => {
            // The append was entered and its outcome is unknown. No
            // `apply_delta`, no retry, no report from memory: the fold is
            // poisoned and the command ends. The next resume re-establishes the
            // barrier over whichever prefix survived and follows the fault row
            // of that prefix.
            fold.poison();
            Err(error)
        }
    }
}

// ---------------------------------------------------------------------------
// Reading the proven fold
// ---------------------------------------------------------------------------

/// The fold the barrier proved, from any point in the chain below it.
fn fold_of(certified: &PreflightCertified) -> &TopologyFold {
    certified.rebuilt().censused().barrier().fold()
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
