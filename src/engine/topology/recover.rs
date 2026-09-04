//! Extended notes: `docs/internals/engine/topology/recover.md`

use std::path::Path;

use crate::config::RunnerSelection;
use crate::error::UpstrokeError;
use crate::events::RunOutcome;
use crate::rundir::{RepoKey, RunLock, WorktreeLock};
use crate::runner::container::GitView;
use crate::runner::container::resolve::RunnerPreflight;
use crate::runner::container::runtime::{ContainerRuntime, OwnerLiveness};
use crate::topology::events::{
    AttemptInterrupted4, AttemptNumber, GenerationCloseReason, GenerationClosed, GenerationId,
    IncarnationId, LeaseDisposition, RunResumed4, RunStarted4, TopologyEvent, TopologyEventBody,
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

pub mod chain {
    pub use barrier::BarrierHeld;
    pub use censused::{CensusSeams, ResumeCensused};
    pub use certified::PreflightCertified;
    pub use locks::LocksHeld;
    pub use rebuilt::RunnerRebuilt;
    pub use records::RecordsVerified;
    pub use root::RootDerived;

    pub mod root {
        use std::path::{Path, PathBuf};

        use crate::error::UpstrokeError;
        use crate::rundir;
        use crate::topology::events::{RunStarted4, TopologyEvent, TopologyEventBody};
        use crate::topology::schema::{
            MAX_READABLE_SCHEMA, ReaderSelection, probe_header, select_for_schema,
        };

        #[derive(Debug)]
        pub struct RootDerived {
            run_id: String,
            public_dir: PathBuf,
            private_root: PathBuf,
            private_dir: PathBuf,
            #[allow(dead_code)]
            reader: ReaderSelection,
            first_line: Vec<u8>,
            started: Box<RunStarted4>,
        }

        impl RootDerived {
            #[allow(dead_code)]
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

            pub(crate) fn derive_with(
                repo_root: &Path,
                wanted_run_id: &str,
                explicit_private_root: Option<&Path>,
                ceiling: u32,
            ) -> Result<Self, UpstrokeError> {
                let run_id = rundir::resolve_run_id(repo_root, wanted_run_id)?;
                let public_dir = rundir::public_dir(repo_root, &run_id);
                let log = public_dir.join(rundir::EVENT_LOG);
                let bytes =
                    crate::util::read_file_bounded(&log).map_err(|source| UpstrokeError::Io {
                        path: log.clone(),
                        source,
                    })?;

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

                let private_dir = PathBuf::from(&started.private_dir);
                let private_root = authorized_root(&private_dir, &run_id)?;

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

            #[must_use]
            pub fn run_id(&self) -> &str {
                &self.run_id
            }

            #[must_use]
            pub fn public_dir(&self) -> &Path {
                &self.public_dir
            }

            #[must_use]
            pub fn private_root(&self) -> &Path {
                &self.private_root
            }

            #[must_use]
            pub fn private_dir(&self) -> &Path {
                &self.private_dir
            }

            #[must_use]
            #[allow(dead_code)]
            pub fn reader(&self) -> ReaderSelection {
                self.reader
            }

            #[must_use]
            pub fn first_line(&self) -> &[u8] {
                &self.first_line
            }

            #[must_use]
            pub fn started(&self) -> &RunStarted4 {
                &self.started
            }

            #[must_use]
            pub fn log_path(&self) -> PathBuf {
                self.public_dir.join(rundir::EVENT_LOG)
            }
        }

        fn authorized_root(private_dir: &Path, run_id: &str) -> Result<PathBuf, UpstrokeError> {
            let malformed = || UpstrokeError::Refused {
                message: format!(
                    "run `{run_id}` records its private half at `{}`, which is not of the shape \
                     `<root>/runs/{run_id}`. A locator of any other shape names a directory this \
                     run cannot prove is its own, so nothing was locked and nothing was touched.",
                    private_dir.display()
                ),
            };
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

        fn normalize(path: &Path) -> PathBuf {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
    }

    pub mod locks {
        use std::path::Path;

        use super::root::RootDerived;
        use crate::error::UpstrokeError;
        use crate::rundir::{RunDirHooks, RunLock, WorktreeLock};

        #[derive(Debug)]
        pub struct LocksHeld {
            root: RootDerived,
            _run: RunLock,
            _worktree: WorktreeLock,
        }

        impl LocksHeld {
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

            #[must_use]
            pub fn root(&self) -> &RootDerived {
                &self.root
            }

            #[must_use]
            pub fn into_guards(self) -> (RunLock, WorktreeLock, RootDerived) {
                (self._run, self._worktree, self.root)
            }
        }
    }

    pub mod records {
        use std::path::Path;

        use super::locks::LocksHeld;
        use crate::error::UpstrokeError;
        use crate::rundir::{self, CommitRecord, OwnerRecord, RepoKey};

        #[derive(Debug)]
        pub struct RecordsVerified {
            locks: LocksHeld,
            #[allow(dead_code)]
            owner: OwnerRecord,
            commit: CommitRecord,
        }

        impl RecordsVerified {
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

            #[must_use]
            pub fn locks(&self) -> &LocksHeld {
                &self.locks
            }

            #[must_use]
            pub fn into_locks(self) -> LocksHeld {
                self.locks
            }

            #[must_use]
            #[allow(dead_code)]
            pub fn owner(&self) -> &OwnerRecord {
                &self.owner
            }

            #[must_use]
            pub fn commit(&self) -> &CommitRecord {
                &self.commit
            }
        }

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

        fn canonical_display(path: &Path) -> String {
            std::fs::canonicalize(path)
                .unwrap_or_else(|_| path.to_path_buf())
                .display()
                .to_string()
        }
    }

    pub mod barrier {
        use super::records::RecordsVerified;
        use crate::error::UpstrokeError;
        use crate::events::log::StablePrefix;
        use crate::runner::container::census::{
            PrefixBytes, PrefixReplay, PrefixReread, PrefixSync, StablePrefixBarrier,
        };
        use crate::topology::fold::TopologyFold;

        #[derive(Debug)]
        pub struct BarrierHeld {
            records: RecordsVerified,
            log: crate::events::log::EventLog,
            #[allow(dead_code)]
            bytes: Vec<u8>,
            events: Vec<crate::topology::events::TopologyEvent>,
            fold: TopologyFold,
            barrier: StablePrefixBarrier,
        }

        impl BarrierHeld {
            pub fn from(
                records: RecordsVerified,
                prefix: StablePrefix,
            ) -> Result<Self, UpstrokeError> {
                let (log, bytes, events, fold) = prefix.into_log_and_fold();
                let measured = PrefixBytes::of(&bytes);
                let barrier = StablePrefixBarrier::establish(
                    PrefixSync {
                        synced_len: measured.len,
                    },
                    &PrefixReread {
                        first: measured.clone(),
                        second: measured.clone(),
                    },
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

            #[must_use]
            pub fn records(&self) -> &RecordsVerified {
                &self.records
            }

            #[must_use]
            pub fn into_log_fold_and_records(
                self,
            ) -> (crate::events::log::EventLog, TopologyFold, RecordsVerified) {
                (self.log, self.fold, self.records)
            }

            #[must_use]
            pub fn fold(&self) -> &TopologyFold {
                &self.fold
            }

            #[must_use]
            pub fn events(&self) -> &[crate::topology::events::TopologyEvent] {
                &self.events
            }

            #[must_use]
            #[allow(dead_code)]
            pub fn bytes(&self) -> &[u8] {
                &self.bytes
            }

            #[must_use]
            pub fn stable_prefix_barrier(&self) -> StablePrefixBarrier {
                self.barrier.clone()
            }

            pub(in crate::engine::topology::recover) fn writer(
                &mut self,
            ) -> (&mut crate::events::log::EventLog, &mut TopologyFold) {
                (&mut self.log, &mut self.fold)
            }
        }
    }

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

        pub struct CensusSeams<'a> {
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
            run_dirs: RunDirCensusReport,
        }

        impl ResumeCensused {
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

                let inputs = CensusInputs {
                    repo_root,
                    repo_key,
                    authorized_root: &private_root,
                    incarnation: incarnation.0.as_str(),
                    runtime: *runtime,
                    liveness: *liveness,
                    view: *view,
                };

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

                let run_dirs = census_run_dirs(hooks.rundir(), &inputs, Some(&run_id))?;

                Ok(Self {
                    barrier,
                    containers,
                    run_dirs,
                })
            }

            #[must_use]
            pub fn barrier(&self) -> &BarrierHeld {
                &self.barrier
            }

            #[must_use]
            pub fn into_barrier(self) -> BarrierHeld {
                self.barrier
            }

            pub(in crate::engine::topology::recover) fn barrier_mut(&mut self) -> &mut BarrierHeld {
                &mut self.barrier
            }

            #[must_use]
            pub fn containers(&self) -> &CensusReport {
                self.containers.report()
            }

            #[must_use]
            pub const fn run_dirs(&self) -> &RunDirCensusReport {
                &self.run_dirs
            }
        }
    }

    pub mod rebuilt {
        use super::censused::ResumeCensused;
        use crate::config::RunnerSelection;
        use crate::error::UpstrokeError;
        use crate::runner::container::resolve::{InspectionRefusal, rebuild_by_inspection};
        use crate::runner::container::runtime::ContainerRuntime;
        use crate::topology::events::{RunnerKind, RunnerPolicy};

        #[derive(Debug)]
        pub struct RunnerRebuilt {
            censused: ResumeCensused,
            policy: RunnerPolicy,
            warnings: Vec<String>,
        }

        impl RunnerRebuilt {
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

            #[must_use]
            pub fn censused(&self) -> &ResumeCensused {
                &self.censused
            }

            #[must_use]
            pub fn into_censused(self) -> ResumeCensused {
                self.censused
            }

            pub(in crate::engine::topology::recover) fn censused_mut(
                &mut self,
            ) -> &mut ResumeCensused {
                &mut self.censused
            }

            #[must_use]
            pub fn policy(&self) -> &RunnerPolicy {
                &self.policy
            }

            #[must_use]
            pub fn warnings(&self) -> &[String] {
                &self.warnings
            }
        }
    }

    pub mod certified {
        use super::rebuilt::RunnerRebuilt;
        use crate::error::UpstrokeError;
        use crate::runner::container::resolve::RunnerPreflight;

        #[derive(Debug)]
        pub struct PreflightCertified {
            rebuilt: RunnerRebuilt,
            agents: Vec<String>,
        }

        impl PreflightCertified {
            pub fn certify(
                rebuilt: RunnerRebuilt,
                preflight: &dyn RunnerPreflight,
            ) -> Result<Self, UpstrokeError> {
                preflight.certify(rebuilt.policy())?;
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

            #[must_use]
            pub fn rebuilt(&self) -> &RunnerRebuilt {
                &self.rebuilt
            }

            #[must_use]
            pub fn into_rebuilt(self) -> RunnerRebuilt {
                self.rebuilt
            }

            pub(in crate::engine::topology::recover) fn rebuilt_mut(
                &mut self,
            ) -> &mut RunnerRebuilt {
                &mut self.rebuilt
            }

            #[must_use]
            pub fn probed_agents(&self) -> &[String] {
                &self.agents
            }
        }
    }
}

pub struct ResumeSeams<'a> {
    pub repo_root: &'a Path,
    pub worktree_git_dir: &'a Path,
    pub repo_key: &'a RepoKey,
    pub incarnation: &'a IncarnationId,
    pub inputs: FrozenInputs,
    pub today: &'a RunnerSelection,
    pub runtime: &'a dyn ContainerRuntime,
    pub liveness: &'a dyn OwnerLiveness,
    pub view: &'a dyn GitView,
    pub preflight: &'a dyn RunnerPreflight,
    pub refs: &'a dyn IntegrationRefs,
    pub manager: &'a WorkspaceManager,
    pub clock: &'a dyn TimeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecoveryStep {
    A0,
    A,
    A1,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Performer {
    ThisOrder,
    CallerBefore,
    LoopAfter,
}

impl RecoveryStep {
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

    #[must_use]
    pub const fn position_override(self) -> Option<&'static str> {
        match self {
            Self::F => Some("decisions.sequential_substrate.checkpoint_refusals"),
            _ => None,
        }
    }

    #[must_use]
    pub const fn performer(self) -> Performer {
        match self {
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
            Self::I => Performer::LoopAfter,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    pub interrupted: usize,
    pub retained_closed: usize,
    pub finished: Vec<TaskKey>,
    pub recreated: Vec<(TaskKey, GenerationId, Reuse)>,
    pub steps: Vec<RecoveryStep>,
    pub resumed: Resumed,
    pub warnings: Vec<String>,
}

pub fn run_recovery_order(
    root: RootDerived,
    seams: &ResumeSeams<'_>,
    hooks: &mut dyn TopologyHooks,
    warnings: &mut Vec<String>,
) -> Result<(Recovered, RunHandle), UpstrokeError> {
    let locks = LocksHeld::take(
        root,
        seams.repo_root,
        seams.worktree_git_dir,
        hooks.rundir(),
    )?;
    let records = RecordsVerified::verify(locks, seams.repo_key)?;
    let mut steps = vec![RecoveryStep::A];

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

    refuse_if_finished(&censused)?;
    steps.push(RecoveryStep::B);

    let rebuilt = RunnerRebuilt::rebuild(censused, seams.today, Some(seams.runtime))?;
    let drift: Vec<String> = rebuilt.warnings().to_vec();
    warnings.extend(drift.iter().cloned());
    let mut certified = PreflightCertified::certify(rebuilt, seams.preflight)?;
    steps.push(RecoveryStep::C);

    refuse_unimplemented_terminals(&certified)?;

    {
        let fold = fold_of(&certified);
        let run_id = fold
            .started()
            .ok_or_else(|| UpstrokeError::Refused {
                message: "the proven prefix has no run".to_owned(),
            })?
            .run_id
            .clone();
        let namespace = crate::engine::topology::candidate::run_namespace(&run_id);
        let expected = crate::engine::topology::candidate::expected_refs(&run_id, fold);
        seams
            .manager
            .refuse_unexpected_refs(&namespace, &expected)?;
    }

    ensure_recorded_integration_ref(&certified, seams.refs, hooks)?;

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
    let interrupted = settle_interrupted(&mut certified, &mut context)?;
    steps.push(RecoveryStep::D);
    let retained_closed = close_retained_idle(&mut certified, &mut context)?;
    steps.push(RecoveryStep::E);

    let finished = finish_promotions(&mut certified, seams.manager, &mut context)?;

    steps.push(RecoveryStep::F);
    let recreated = recreate_open_no_attempt(&certified, seams.manager, context.hooks)?;
    steps.push(RecoveryStep::G);

    let (resumed, handle) = run_resumed(certified, &mut context, seams.incarnation)?;
    steps.push(RecoveryStep::H);
    Ok((
        Recovered {
            interrupted,
            retained_closed,
            finished,
            recreated,
            steps,
            resumed,
            warnings: drift,
        },
        handle,
    ))
}

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
        RunOutcome::Parked | RunOutcome::BudgetExceeded => Ok(()),
    }
}

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

    Ok(())
}

pub fn finish_promotions(
    certified: &mut PreflightCertified,
    manager: &WorkspaceManager,
    context: &mut EmitContext<'_>,
) -> Result<Vec<TaskKey>, UpstrokeError> {
    let run_id = fold_of(certified)
        .started()
        .ok_or_else(|| UpstrokeError::Refused {
            message: "the proven prefix has no run".to_owned(),
        })?
        .run_id
        .clone();

    let mut unfinished = Vec::new();
    let mut orphans = Vec::new();
    {
        let fold = fold_of(certified);
        for key in task_keys(fold) {
            let recovery =
                crate::engine::topology::candidate::recovery_for(manager, &run_id, fold, key)?;
            if let Some(promoting) = recovery.promotion {
                unfinished.push(promoting);
            }
            if let Some(orphan) = recovery.orphan_pin {
                orphans.push(orphan);
            }
        }
    }

    for orphan in orphans {
        crate::engine::topology::candidate::prune_orphan_pin(manager, context.hooks, orphan)?;
    }

    let mut finished = Vec::new();
    for promoting in unfinished {
        let key = promoting.candidate().key;
        let generation = promoting.candidate().generation;
        let slot = crate::engine::topology::dispatch::task_slot(key, generation);

        let referenced = crate::engine::topology::candidate::create_candidates_ref(
            manager,
            &mut *context.hooks,
            promoting,
        )?;
        let created = {
            let mut journal = RecoveryJournal { certified, context };
            crate::engine::topology::candidate::append_candidate_created(&mut journal, referenced)?
        };
        crate::engine::topology::candidate::reclaim_after_creation(
            manager,
            &mut *context.hooks,
            &slot,
            created,
        )?;
        finished.push(key);
    }
    Ok(finished)
}

struct RecoveryJournal<'c, 'e, 'x> {
    certified: &'c mut PreflightCertified,
    context: &'e mut EmitContext<'x>,
}

impl crate::engine::topology::candidate::CandidateJournal for RecoveryJournal<'_, '_, '_> {
    fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError> {
        emit(self.certified, self.context, body)
    }

    fn fold(&self) -> &TopologyFold {
        fold_of(self.certified)
    }
}

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

pub struct EmitContext<'a> {
    pub clock: &'a dyn TimeSource,
    pub hooks: &'a mut dyn TopologyHooks,
    pub inputs: FrozenInputs,
    pub reservations: &'a mut Reservations,
    pub invocations: &'a mut InvocationLedger,
    pub warnings: &'a mut Vec<String>,
}

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

    let events = certified.rebuilt().censused().barrier().events().to_vec();

    let (log, fold, records) = certified
        .into_rebuilt()
        .into_censused()
        .into_barrier()
        .into_log_fold_and_records();
    let committed_first_line_sha256 = records.commit().run_started_sha256.clone();
    let (run_lock, worktree_lock, root) = records.into_locks().into_guards();
    Ok((
        resumed,
        RunHandle {
            started: root.started().clone(),
            committed_first_line_sha256,
            log,
            fold,
            events,
            _run: run_lock,
            _worktree: worktree_lock,
        },
    ))
}

pub struct RunHandle {
    pub committed_first_line_sha256: String,
    pub log: crate::events::log::EventLog,
    pub fold: TopologyFold,
    pub started: RunStarted4,
    pub events: Vec<TopologyEvent>,
    _run: RunLock,
    _worktree: WorktreeLock,
}

impl RunHandle {
    #[must_use]
    pub fn created(
        started: RunStarted4,
        committed_first_line_sha256: String,
        log: crate::events::log::EventLog,
        fold: TopologyFold,
        run: RunLock,
        worktree: WorktreeLock,
    ) -> Self {
        Self {
            started,
            committed_first_line_sha256,
            log,
            fold,
            events: Vec::new(),
            _run: run,
            _worktree: worktree,
        }
    }
}

impl std::fmt::Debug for RunHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunHandle")
            .field("run_id", &self.started.run_id)
            .field("poisoned", &self.fold.is_poisoned())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resumed {
    pub epoch: u32,
    pub budget_stop_cleared: bool,
}

fn emit(
    certified: &mut PreflightCertified,
    context: &mut EmitContext<'_>,
    body: TopologyEventBody,
) -> Result<(), UpstrokeError> {
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
    super::emit::emit(&identity, &mut state, context.clock, body, context.hooks)
        .map(|_| ())
        .map_err(|error| super::emit::EmitFailure::from(error).discharging(context.invocations))
}

fn fold_of(certified: &PreflightCertified) -> &TopologyFold {
    certified.rebuilt().censused().barrier().fold()
}

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

fn open_no_attempt(fold: &TopologyFold) -> Result<Vec<OpenGeneration>, UpstrokeError> {
    let mut found = Vec::new();
    for key in task_keys(fold) {
        let Some(task) = fold.task(key) else { continue };
        let Some(open) = fold.open_no_attempt(key) else {
            continue;
        };
        for generation in task.generations.iter().filter(|held| held.id == open) {
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
                source: None,
            });
        }
    }
    Ok(found)
}

fn task_keys(fold: &TopologyFold) -> Vec<TaskKey> {
    fold.registry()
        .map(|registry| {
            (0..registry.len())
                .map(|index| TaskKey(index as u32))
                .collect()
        })
        .unwrap_or_default()
}

fn in_flight(fold: &TopologyFold) -> Vec<(TaskKey, GenerationId, AttemptNumber, LeaseDisposition)> {
    let mut found = Vec::new();
    for key in task_keys(fold) {
        let Some(task) = fold.task(key) else { continue };
        for generation in &task.generations {
            if let GenerationClass::InFlight { attempt } = generation.class {
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
