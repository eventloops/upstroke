//! Extended notes: `docs/internals/runner/container/census.md`

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

pub const VIEWS_DIR: &str = "views";

#[must_use]
pub fn view_path(private_root: &Path, name: &ContainerName) -> PathBuf {
    private_root.join(VIEWS_DIR).join(name.as_str())
}

#[must_use]
pub fn private_root_label(private_root: &Path) -> String {
    super::intent::private_root_label(private_root)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixBytes {
    pub len: u64,
    pub sha256: String,
}

impl PrefixBytes {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixSync {
    pub synced_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixReread {
    pub first: PrefixBytes,
    pub second: PrefixBytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixReplay {
    pub replayed: PrefixBytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StablePrefixBarrier {
    boundary: u64,
    digest: String,
}

impl StablePrefixBarrier {
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

    #[must_use]
    pub const fn boundary(&self) -> u64 {
        self.boundary
    }

    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CensusStart {
    FreshRun {
        incarnation: String,
    },
    Resume {
        run_id: String,
        incarnation: String,
        barrier: StablePrefixBarrier,
    },
}

impl CensusStart {
    #[must_use]
    pub fn own_run(&self) -> Option<&str> {
        match self {
            Self::FreshRun { .. } => None,
            Self::Resume { run_id, .. } => Some(run_id),
        }
    }

    #[must_use]
    pub fn incarnation(&self) -> &str {
        match self {
            Self::FreshRun { incarnation } | Self::Resume { incarnation, .. } => incarnation,
        }
    }

    #[must_use]
    pub const fn command(&self) -> WriteCommand {
        match self {
            Self::FreshRun { .. } => WriteCommand::Run,
            Self::Resume { .. } => WriteCommand::Resume,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WriteCommand {
    Run,
    Resume,
}

impl WriteCommand {
    pub const ALL: &'static [Self] = &[Self::Run, Self::Resume];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Resume => "resume",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ownership {
    OwnRunEarlierIncarnation,
    OwnRunThisIncarnation,
    ForeignRunDeadOwner,
    ForeignRunLiveOwner,
}

impl Ownership {
    pub const ALL: &'static [Self] = &[
        Self::OwnRunEarlierIncarnation,
        Self::OwnRunThisIncarnation,
        Self::ForeignRunDeadOwner,
        Self::ForeignRunLiveOwner,
    ];

    #[must_use]
    pub const fn reclaims(self) -> bool {
        match self {
            Self::OwnRunEarlierIncarnation | Self::ForeignRunDeadOwner => true,
            Self::OwnRunThisIncarnation | Self::ForeignRunLiveOwner => false,
        }
    }

    #[must_use]
    pub const fn refuses(self) -> bool {
        matches!(self, Self::OwnRunThisIncarnation)
    }

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

#[must_use]
pub fn classify_ownership(
    start: &CensusStart,
    owner_run_id: &str,
    owner_incarnation: &str,
    owner_run_dir: &Path,
    liveness: &dyn OwnerLiveness,
) -> Ownership {
    if start.own_run() == Some(owner_run_id) {
        return if owner_incarnation == start.incarnation() {
            Ownership::OwnRunThisIncarnation
        } else {
            Ownership::OwnRunEarlierIncarnation
        };
    }
    if liveness.is_running(owner_run_dir) {
        Ownership::ForeignRunLiveOwner
    } else {
        Ownership::ForeignRunDeadOwner
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiscoveredBy {
    IntentOnly,
    LabelOnly,
    IntentAndLabel,
}

impl DiscoveredBy {
    pub const ALL: &'static [Self] = &[Self::IntentOnly, Self::LabelOnly, Self::IntentAndLabel];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::IntentOnly => "intent-only",
            Self::LabelOnly => "label-only",
            Self::IntentAndLabel => "intent-and-label",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Boundary {
    FromIntent(String),
    NoIntentRecord,
}

impl Boundary {
    #[must_use]
    pub fn digest(&self) -> Option<&str> {
        match self {
            Self::FromIntent(digest) => Some(digest),
            Self::NoIntentRecord => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: ContainerName,
    pub run_id: String,
    pub incarnation: String,
    pub run_dir: PathBuf,
    pub boundary: Boundary,
    pub discovered_by: DiscoveredBy,
    pub intent_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnerSettlement {
    InterruptedWithUnknownSpend,
}

impl OwnerSettlement {
    pub const ALL: &'static [Self] = &[Self::InterruptedWithUnknownSpend];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::InterruptedWithUnknownSpend => "interrupted-unknown-spend",
        }
    }

    #[must_use]
    pub const fn spend_is_known(self) -> bool {
        match self {
            Self::InterruptedWithUnknownSpend => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reclaimed {
    pub name: ContainerName,
    pub run_id: String,
    pub incarnation: String,
    pub ownership: Ownership,
    pub discovered_by: DiscoveredBy,
    pub boundary: Boundary,
    pub settlement: OwnerSettlement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untouched {
    pub name: ContainerName,
    pub run_id: String,
    pub incarnation: String,
    pub ownership: Ownership,
    pub discovered_by: DiscoveredBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeUse {
    Consulted,
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StagedDisposition {
    Adopted,
    Removed,
    RetainedForeignOwner,
}

impl StagedDisposition {
    pub const ALL: &'static [Self] = &[Self::Adopted, Self::Removed, Self::RetainedForeignOwner];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Adopted => "adopted",
            Self::Removed => "removed",
            Self::RetainedForeignOwner => "retained-foreign-owner",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedResidue {
    pub name: ContainerName,
    pub path: PathBuf,
    pub disposition: StagedDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CensusReport {
    pub private_root: PathBuf,
    pub command: WriteCommand,
    pub incarnation: String,
    pub runtime_use: RuntimeUse,
    pub orphan_window: OrphanWindow,
    pub reclaimed: Vec<Reclaimed>,
    pub untouched: Vec<Untouched>,
    pub staged: Vec<StagedResidue>,
}

impl CensusReport {
    #[must_use]
    pub fn boundary_of(&self, name: &ContainerName) -> Option<&Boundary> {
        self.reclaimed
            .iter()
            .find(|entry| &entry.name == name)
            .map(|entry| &entry.boundary)
    }

    #[must_use]
    pub fn was_untouched(&self, name: &ContainerName) -> bool {
        self.untouched.iter().any(|entry| &entry.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CensusComplete {
    report: CensusReport,
}

impl CensusComplete {
    #[must_use]
    pub const fn report(&self) -> &CensusReport {
        &self.report
    }

    #[must_use]
    pub fn private_root(&self) -> &Path {
        &self.report.private_root
    }
}

pub struct Census<'a> {
    pub private_root: &'a Path,
    pub start: &'a CensusStart,
    pub runtime: &'a dyn ContainerRuntime,
    pub liveness: &'a dyn OwnerLiveness,
    pub view: &'a dyn GitView,
}

pub fn run_startup_census(
    hooks: &mut dyn ContainerHooks,
    census: &Census<'_>,
) -> Result<CensusComplete, UpstrokeError> {
    let private_root = census.private_root;
    let intents = list_intents(private_root)?;
    let staged = super::list_staged_intents(private_root)?;
    let (discovered, runtime_use) = discover_by_label(census.runtime, private_root, &intents)?;
    let mut candidates = merge(private_root, intents, discovered)?;
    let mut staged_residue = Vec::new();
    for entry in staged {
        match entry.record {
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
            None => staged_residue.push(StagedResidue {
                name: entry.name,
                path: entry.path,
                disposition: StagedDisposition::RetainedForeignOwner,
            }),
        }
    }
    candidates.sort_by(|left, right| left.name.cmp(&right.name));

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
            settlement: OwnerSettlement::InterruptedWithUnknownSpend,
        });
    }

    for residue in &mut staged_residue {
        if residue.disposition != StagedDisposition::RetainedForeignOwner {
            continue;
        }
        let parts = ContainerName::parse(residue.name.as_str())?;
        if census.start.own_run() != Some(parts.run_id.as_str())
            || parts.incarnation == census.start.incarnation()
        {
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

fn candidate_from_intent(found: FoundIntent) -> Result<Candidate, UpstrokeError> {
    check_name_against_record(&found)?;
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

const LABEL_FILTER: &str = "label=";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaperContainerScope {
    program: PathBuf,
    private_root: String,
    incarnation: String,
}

impl ReaperContainerScope {
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

    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

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

    #[must_use]
    pub fn kill_argv(&self, id: &str) -> Vec<String> {
        vec![
            self.program.to_string_lossy().into_owned(),
            "kill".to_owned(),
            id.to_owned(),
        ]
    }

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

#[must_use]
pub const fn proceeds_without(error: &RuntimeError) -> bool {
    error.is_unreachable()
}

#[cfg(test)]
mod tests;
