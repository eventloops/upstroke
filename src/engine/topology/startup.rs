//! Extended notes: `docs/internals/engine/topology/startup.md`

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

pub struct CensusInputs<'a> {
    pub repo_root: &'a Path,
    pub repo_key: &'a RepoKey,
    pub authorized_root: &'a Path,
    pub incarnation: &'a str,
    pub runtime: &'a dyn ContainerRuntime,
    pub liveness: &'a dyn OwnerLiveness,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedStep {
    PublicHalf,
    PrivateHalf,
    PublicHalfAfterPrivate,
    StaleMarker,
}

impl FailedStep {
    pub const ALL: &'static [Self] = &[
        Self::PublicHalf,
        Self::PrivateHalf,
        Self::PublicHalfAfterPrivate,
        Self::StaleMarker,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PublicHalf => "public-half",
            Self::PrivateHalf => "private-half",
            Self::PublicHalfAfterPrivate => "public-half-after-private",
            Self::StaleMarker => "stale-marker",
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDirOutcome {
    ReclaimedPublicOnly(UnboundShape),
    ReclaimedBothHalves,
    Retained(RetainReason),
    RepairedStaleMarker,
    Committed,
    Skipped,
    Unreclaimable { step: FailedStep, detail: String },
}

impl RunDirOutcome {
    pub const KINDS: &'static [&'static str] = &[
        "reclaimed-public-only",
        "reclaimed-both-halves",
        "retained",
        "repaired-stale-marker",
        "committed",
        "skipped",
        "unreclaimable",
    ];

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDirEntry {
    pub run_id: String,
    pub public: PathBuf,
    pub locator: Option<PathBuf>,
    pub class: RunDirClass,
    pub outcome: RunDirOutcome,
}

impl RunDirEntry {
    #[must_use]
    pub const fn retain_reason(&self) -> Option<&RetainReason> {
        match &self.outcome {
            RunDirOutcome::Retained(reason) => Some(reason),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_possibly_committed(&self) -> bool {
        matches!(
            self.outcome,
            RunDirOutcome::Retained(RetainReason::PossiblyCommitted)
        )
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDirCensusReport {
    entries: Vec<RunDirEntry>,
}

impl RunDirCensusReport {
    #[must_use]
    pub fn entries(&self) -> &[RunDirEntry] {
        &self.entries
    }

    #[must_use]
    pub fn of(&self, run_id: &str) -> Option<&RunDirEntry> {
        self.entries.iter().find(|entry| entry.run_id == run_id)
    }

    #[must_use]
    pub fn reclaimed(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| outcome.reclaimed_anything())
    }

    #[must_use]
    pub fn repaired(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| matches!(outcome, RunDirOutcome::RepairedStaleMarker))
    }

    #[must_use]
    pub fn retained(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| matches!(outcome, RunDirOutcome::Retained(_)))
    }

    #[must_use]
    pub fn possibly_committed(&self) -> Vec<&RunDirEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.is_possibly_committed())
            .collect()
    }

    #[must_use]
    pub fn skipped(&self) -> Vec<&RunDirEntry> {
        self.with(|outcome| matches!(outcome, RunDirOutcome::Skipped))
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupCensus {
    containers: CensusComplete,
    run_dirs: RunDirCensusReport,
}

impl StartupCensus {
    #[must_use]
    pub const fn containers(&self) -> &CensusComplete {
        &self.containers
    }

    #[must_use]
    pub const fn run_dirs(&self) -> &RunDirCensusReport {
        &self.run_dirs
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn into_parts(self) -> (CensusComplete, RunDirCensusReport) {
        (self.containers, self.run_dirs)
    }
}

pub fn startup_census(
    locked: WorktreeLocked,
    hooks: &mut dyn TopologyHooks,
    inputs: &CensusInputs<'_>,
) -> Result<FreshCensused, UpstrokeError> {
    FreshCensused::establish(locked, hooks, inputs)
}

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
    let containers = run_startup_census(hooks.container(), &census)?;
    let run_dirs = census_run_dirs(hooks.rundir(), inputs, start.own_run())?;
    Ok(StartupCensus {
        containers,
        run_dirs,
    })
}

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

fn enumerate(repo_root: &Path) -> Result<Vec<String>, UpstrokeError> {
    let names = rundir::run_dir_names(repo_root);
    if names.is_empty() {
        let runs = rundir::runs_root(repo_root);
        match std::fs::read_dir(&runs) {
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(UpstrokeError::Io { path: runs, source }),
        }
    }
    Ok(names)
}

#[derive(Debug)]
struct Scanned {
    run_id: String,
    public: PathBuf,
    locator: Option<PathBuf>,
    class: RunDirClass,
    plan: Planned,
}

#[derive(Debug)]
enum Planned {
    ReclaimPublicOnly(UnboundShape),
    ReclaimBothHalves(PrivateHalfProof),
    Retain(RetainReason),
    RepairStaleMarker,
    Committed,
    Skip,
}

fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: Option<&str>) -> Scanned {
    let public = rundir::public_dir(inputs.repo_root, run_id);
    let class = rundir::classify_run_dir(&public);
    let lock_held = rundir::is_running(&public);

    if class == RunDirClass::Committed {
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
            locator: None,
            class,
            plan,
        };
    }

    if lock_held {
        return Scanned {
            run_id: run_id.to_owned(),
            public,
            locator: None,
            class,
            plan: Planned::Skip,
        };
    }

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

fn apply(hooks: &mut dyn RunDirHooks, public: &Path, plan: Planned) -> RunDirOutcome {
    match plan {
        Planned::ReclaimPublicOnly(shape) => match rundir::remove_public_husk(public, hooks) {
            Ok(()) => RunDirOutcome::ReclaimedPublicOnly(shape),
            Err(error) => unreclaimable(FailedStep::PublicHalf, &error),
        },
        Planned::ReclaimBothHalves(proof) => {
            if let Err(error) = rundir::remove_private_husk(proof, hooks) {
                return unreclaimable(FailedStep::PrivateHalf, &error);
            }
            match rundir::remove_public_husk(public, hooks) {
                Ok(()) => RunDirOutcome::ReclaimedBothHalves,
                Err(error) => unreclaimable(FailedStep::PublicHalfAfterPrivate, &error),
            }
        }
        Planned::Retain(reason) => RunDirOutcome::Retained(reason),
        Planned::RepairStaleMarker => match rundir::remove_marker(public, hooks) {
            Ok(()) => RunDirOutcome::RepairedStaleMarker,
            Err(error) => unreclaimable(FailedStep::StaleMarker, &error),
        },
        Planned::Committed => RunDirOutcome::Committed,
        Planned::Skip => RunDirOutcome::Skipped,
    }
}

fn unreclaimable(step: FailedStep, error: &UpstrokeError) -> RunDirOutcome {
    RunDirOutcome::Unreclaimable {
        step,
        detail: error.to_string(),
    }
}

fn stale_marker_present(public: &Path) -> bool {
    std::fs::symlink_metadata(public.join(rundir::MARKER)).is_ok()
        || std::fs::symlink_metadata(public.join(rundir::MARKER_STAGED)).is_ok()
}

mod witness {
    pub use fresh::FreshCensused;
    pub use locked::WorktreeLocked;

    mod locked {
        use crate::rundir::WorktreeLock;

        #[derive(Debug)]
        pub struct WorktreeLocked {
            #[allow(dead_code)]
            lock: WorktreeLock,
        }

        impl WorktreeLocked {
            #[must_use]
            pub fn from(lock: WorktreeLock) -> Self {
                Self { lock }
            }

            #[must_use]
            #[allow(dead_code)]
            pub const fn lock(&self) -> &WorktreeLock {
                &self.lock
            }
        }
    }

    mod fresh {
        use super::super::{CensusInputs, StartupCensus, both_halves};
        use super::locked::WorktreeLocked;
        use crate::engine::topology::seams::TopologyHooks;
        use crate::error::UpstrokeError;
        use crate::runner::container::census::CensusStart;

        #[derive(Debug)]
        pub struct FreshCensused {
            locked: WorktreeLocked,
            census: StartupCensus,
        }

        impl FreshCensused {
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

            #[must_use]
            pub const fn census(&self) -> &StartupCensus {
                &self.census
            }

            #[must_use]
            #[allow(dead_code)]
            pub const fn locked(&self) -> &WorktreeLocked {
                &self.locked
            }

            #[must_use]
            pub fn into_parts(self) -> (WorktreeLocked, StartupCensus) {
                (self.locked, self.census)
            }
        }
    }
}

#[cfg(test)]
mod tests;
