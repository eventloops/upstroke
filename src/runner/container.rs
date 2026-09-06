//! Extended notes: `docs/internals/runner/container.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

pub mod census;
pub mod env;
pub mod exec;
pub mod intent;
pub mod resolve;
pub mod runtime;
pub mod view;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::error::UpstrokeError;
use crate::topology::effects::{ContainerSite, EffectSiteId, HookHarness, HookPhase, Injection};
use crate::util;

use crate::runner::InvocationId;
use intent::{ContainerIntent, ContainerName, INTENT_STAGED_SUFFIX, IntentWritten, containers_dir};
use runtime::{
    ContainerExecution, ContainerRuntime, ContainerTrace, CreateSpec, CreatedContainer,
    DiscoveredContainer, DurableStep, Liveness, RuntimeError, RuntimeOp, StopMode, TracePhase,
    ViewAction,
};

pub trait ContainerHooks {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection;

    fn trace(&self) -> ContainerTrace {
        ContainerTrace::off()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl ContainerHooks for NoHooks {
    fn phase(&mut self, _site: EffectSiteId, _phase: HookPhase) -> Injection {
        Injection::Proceed
    }
}

#[derive(Debug, Clone, Default)]
pub struct HarnessHooks {
    harness: Arc<Mutex<HookHarness>>,
    trace: ContainerTrace,
}

impl HarnessHooks {
    #[must_use]
    pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {
        Self {
            harness,
            trace: ContainerTrace::off(),
        }
    }

    #[must_use]
    pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {
        &self.harness
    }

    #[must_use]
    pub fn recording_trace(mut self, trace: ContainerTrace) -> Self {
        self.trace = trace;
        self
    }
}

impl ContainerHooks for HarnessHooks {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .hook(site, phase)
    }

    fn trace(&self) -> ContainerTrace {
        self.trace.clone()
    }
}

fn apply(injection: Injection, site: EffectSiteId, phase: HookPhase) -> Result<(), UpstrokeError> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(UpstrokeError::Refused {
            message: format!("the container funnel was made to fail at `{site}` ({phase})"),
        }),
    }
}

fn funnel<T>(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    primitive: impl FnOnce() -> Result<T, UpstrokeError>,
) -> Result<T, UpstrokeError> {
    let id = EffectSiteId::Container(site);
    let trace = hooks.trace();
    trace.site(site, TracePhase::Before);
    apply(hooks.phase(id, HookPhase::Before), id, HookPhase::Before)?;
    let produced = primitive()?;
    apply(hooks.phase(id, HookPhase::After), id, HookPhase::After)?;
    trace.site(site, TracePhase::After);
    Ok(produced)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    WriteIntent,
    Create,
    Start,
    MountGitView,
    Stop,
    Remove,
    UnmountGitView,
    RemoveIntent,
}

const fn operation_of(site: ContainerSite) -> Operation {
    match site {
        ContainerSite::WriteIntent => Operation::WriteIntent,
        ContainerSite::Create => Operation::Create,
        ContainerSite::Start => Operation::Start,
        ContainerSite::MountGitView => Operation::MountGitView,
        ContainerSite::Stop => Operation::Stop,
        ContainerSite::Remove => Operation::Remove,
        ContainerSite::UnmountGitView => Operation::UnmountGitView,
        ContainerSite::RemoveIntent => Operation::RemoveIntent,
    }
}

fn expect_site(site: ContainerSite, wanted: Operation) -> Result<(), UpstrokeError> {
    if operation_of(site) == wanted {
        return Ok(());
    }
    Err(UpstrokeError::Refused {
        message: format!(
            "the container funnel was asked to perform {wanted:?} under site \
             `Container.{}`; every effectful funnel API takes its group's site by value \
             (decisions.effect_site_inventory.identity) and the site must name the \
             operation it accounts for",
            site.name()
        ),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitViewRequest {
    pub path: PathBuf,
    pub workspace: PathBuf,
    pub head: Option<String>,
}

pub trait GitView: Send + Sync {
    fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, UpstrokeError>;

    fn discard(&self, path: &Path) -> Result<(), UpstrokeError>;
}

#[derive(Debug, Clone, Default)]
pub struct DisposableDirView {
    trace: ContainerTrace,
}

impl DisposableDirView {
    #[must_use]
    pub fn new(trace: ContainerTrace) -> Self {
        Self { trace }
    }
}

impl GitView for DisposableDirView {
    fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, UpstrokeError> {
        fs::create_dir_all(&request.path).map_err(|source| UpstrokeError::Io {
            path: request.path.clone(),
            source,
        })?;
        self.trace.view(ViewAction::Materialized, &request.path);
        Ok(request.path.clone())
    }

    fn discard(&self, path: &Path) -> Result<(), UpstrokeError> {
        racing_removal(path, || fs::remove_dir_all(path))?;
        self.trace.view(ViewAction::Discarded, path);
        Ok(())
    }
}

pub const RACING_ACCESS_ATTEMPTS: usize = 64;

pub const RACING_YIELD_ATTEMPTS: usize = 16;

pub const RACING_SLEEP: Duration = Duration::from_millis(10);

fn racing_pause(attempt: usize) {
    if attempt < RACING_YIELD_ATTEMPTS {
        std::thread::yield_now();
    } else {
        std::thread::sleep(RACING_SLEEP);
    }
}

pub fn write_intent(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    private_root: &Path,
    name: &ContainerName,
    record: &ContainerIntent,
) -> Result<IntentWritten, UpstrokeError> {
    expect_site(site, Operation::WriteIntent)?;
    let path = name.intent_path(private_root);
    let trace = hooks.trace();
    let root = private_root.to_path_buf();
    funnel(hooks, site, || {
        let bytes = serde_json::to_vec(record).map_err(|error| UpstrokeError::Git {
            message: format!("serializing the container intent for `{name}`: {error}"),
        })?;
        write_synced(&path, &bytes, &trace)?;
        IntentWritten::certify(&root, name)
    })
}

pub fn create_container(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    runtime: &dyn ContainerRuntime,
    intent: &IntentWritten,
    spec: &CreateSpec,
) -> Result<CreatedContainer, UpstrokeError> {
    expect_site(site, Operation::Create)?;
    expect_intent_for(intent, &spec.name, "created")?;
    expect_mounted_volumes_present(runtime, spec)?;
    funnel(hooks, site, || runtime.create(spec).map_err(refused))
}

fn expect_mounted_volumes_present(
    runtime: &dyn ContainerRuntime,
    spec: &CreateSpec,
) -> Result<(), UpstrokeError> {
    for mount in &spec.mounts {
        let runtime::Mount::Volume { name, target, .. } = mount else {
            continue;
        };
        let present = runtime
            .volume_present(name)
            .map_err(|error| UpstrokeError::Refused {
                message: format!(
                    "the container runtime could not be asked whether the credential volume \
                     `{name}` exists before creating `{}`: {error}. A named volume that \
                     `docker create` does not find is created empty by the runtime, and R20 is \
                     `operator_owned` — \"never created or pruned by a run\" \
                     (decisions.resource_accounting.rows[R20]) — so a runtime that will not \
                     answer refuses rather than risking it",
                    spec.name
                ),
            })?;
        if !present {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the credential volume `{name}`, which `{}` mounts at `{target}`, is not \
                     present in the container runtime. R20 credential volumes are \
                     `operator_owned` and `persistent_output` — \"never created or pruned by a \
                     run\" (decisions.resource_accounting.rows[R20]) — and `docker create` \
                     creates an absent named volume rather than refusing, so this invocation is \
                     refused before any container exists",
                    spec.name
                ),
            });
        }
    }
    Ok(())
}

pub fn start_container(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    runtime: &dyn ContainerRuntime,
    intent: &IntentWritten,
) -> Result<(), UpstrokeError> {
    expect_site(site, Operation::Start)?;
    let name = intent.name().as_str().to_owned();
    funnel(hooks, site, || runtime.start(&name).map_err(refused))
}

fn expect_intent_for(intent: &IntentWritten, name: &str, verb: &str) -> Result<(), UpstrokeError> {
    if intent.name().as_str() == name {
        return Ok(());
    }
    Err(UpstrokeError::Refused {
        message: format!(
            "`{name}` cannot be {verb} under the intent record of `{}`; every container \
             invocation writes its own synced intent in `<R>/containers` \
             (decisions.admission_and_leases.permits.crash_reconstruction) and \
             `container start without an intent is impossible by construction` \
             (expected_failures_refusals[6])",
            intent.name()
        ),
    })
}

pub fn mount_git_view(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    view: &dyn GitView,
    request: &GitViewRequest,
) -> Result<PathBuf, UpstrokeError> {
    expect_site(site, Operation::MountGitView)?;
    funnel(hooks, site, || view.materialize(request))
}

pub fn stop_container(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    runtime: &dyn ContainerRuntime,
    name: &ContainerName,
    mode: StopMode,
) -> Result<(), UpstrokeError> {
    expect_site(site, Operation::Stop)?;
    funnel(hooks, site, || {
        runtime.stop(name.as_str(), mode).map_err(refused)
    })
}

pub fn remove_container(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    runtime: &dyn ContainerRuntime,
    name: &ContainerName,
) -> Result<(), UpstrokeError> {
    expect_site(site, Operation::Remove)?;
    funnel(hooks, site, || {
        runtime.remove(name.as_str()).map_err(refused)
    })
}

pub fn unmount_git_view(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    view: &dyn GitView,
    path: &Path,
) -> Result<(), UpstrokeError> {
    expect_site(site, Operation::UnmountGitView)?;
    funnel(hooks, site, || view.discard(path))
}

pub fn remove_intent(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    private_root: &Path,
    name: &ContainerName,
) -> Result<(), UpstrokeError> {
    expect_site(site, Operation::RemoveIntent)?;
    let path = name.intent_path(private_root);
    let trace = hooks.trace();
    funnel(hooks, site, || {
        let staged = staged_path(&path);
        remove_if_present(&staged, &trace)?;
        remove_if_present(&path, &trace)
    })
}

#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub private_root: PathBuf,
    pub name: ContainerName,
    pub invocation: InvocationId,
    pub intent: ContainerIntent,
    pub spec: CreateSpec,
    pub view: GitViewRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    pub name: ContainerName,
    pub intent_path: PathBuf,
    pub view_path: PathBuf,
    pub reported_image_id: String,
}

pub fn launch(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    plan: &LaunchPlan,
) -> Result<Launched, UpstrokeError> {
    let written = write_intent(
        hooks,
        ContainerSite::WriteIntent,
        &plan.private_root,
        &plan.name,
        &plan.intent,
    )?;
    let intent_path = written.path().to_path_buf();
    let view_path = mount_git_view(hooks, ContainerSite::MountGitView, view, &plan.view)?;
    let created = create_container(hooks, ContainerSite::Create, runtime, &written, &plan.spec)?;
    if created.reported_image_id != plan.spec.image_id {
        let residue = cancel_created(
            hooks,
            runtime,
            view,
            &plan.private_root,
            &plan.name,
            Some(&view_path),
        );
        return Err(UpstrokeError::Refused {
            message: format!(
                "the container runtime created `{}` and reports image id `{}`, and the run's \
                 recorded image id is `{}`; a created container whose reported image id \
                 differs from the record is refused before start (INV-23){}",
                plan.name,
                created.reported_image_id,
                plan.spec.image_id,
                render_residue(&residue)
            ),
        });
    }
    start_container(hooks, ContainerSite::Start, runtime, &written)?;
    Ok(Launched {
        name: plan.name.clone(),
        intent_path,
        view_path,
        reported_image_id: created.reported_image_id,
    })
}

fn cancel_created(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    private_root: &Path,
    name: &ContainerName,
    view_path: Option<&Path>,
) -> Vec<String> {
    cancel_reached(hooks, runtime, view, private_root, name, true, view_path)
}

fn cancel_reached(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    private_root: &Path,
    name: &ContainerName,
    container_exists: bool,
    view_path: Option<&Path>,
) -> Vec<String> {
    let mut residue = Vec::new();
    if container_exists {
        if let Err(error) = stop_container(
            hooks,
            ContainerSite::Stop,
            runtime,
            name,
            StopMode::Graceful,
        ) {
            residue.push(format!("the container could not be stopped: {error}"));
        }
        if let Err(error) = remove_container(hooks, ContainerSite::Remove, runtime, name) {
            residue.push(format!("the container could not be removed: {error}"));
        }
    }
    let mut view_survives = false;
    if let Some(path) = view_path {
        if let Err(error) = unmount_git_view(hooks, ContainerSite::UnmountGitView, view, path) {
            residue.push(format!("the R19 Git view could not be pruned: {error}"));
            view_survives = true;
        }
    }
    if view_survives {
        residue.push(format!(
            "the R26 intent record of `{name}` is deliberately retained, because it is the only \
             thing a later census can discover that unpruned R19 view through \
             (decisions.resource_accounting.rows[R19].at_run_end.NoRunFinished)"
        ));
        return residue;
    }
    if let Err(error) = remove_intent(hooks, ContainerSite::RemoveIntent, private_root, name) {
        residue.push(format!(
            "the R26 intent record could not be removed: {error}"
        ));
    }
    residue
}

fn render_residue(residue: &[String]) -> String {
    if residue.is_empty() {
        return String::new();
    }
    format!(
        ". The cancel could not release everything the refused launch created, \
         so this run's R19/R26 ledgers do not balance and a census will find the \
         residue: {}",
        residue.join("; ")
    )
}

pub fn release(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    private_root: &Path,
    launched: &Launched,
) -> Result<(), UpstrokeError> {
    let residue = cancel_reached(
        hooks,
        runtime,
        view,
        private_root,
        &launched.name,
        true,
        Some(&launched.view_path),
    );
    if residue.is_empty() {
        return Ok(());
    }
    Err(UpstrokeError::Refused {
        message: format!(
            "the release of `{}` could not complete every step, so this run's R19/R26 ledgers \
             do not balance and a census will find the residue: {}",
            launched.name,
            residue.join("; ")
        ),
    })
}

pub const TERMINATION_OBSERVATIONS: usize = 8;

pub fn reclaim(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    private_root: &Path,
    name: &ContainerName,
    view_path: Option<&Path>,
) -> Result<(), UpstrokeError> {
    stop_container(hooks, ContainerSite::Stop, runtime, name, StopMode::Kill)?;
    observe_terminated(runtime, name)?;
    remove_container(hooks, ContainerSite::Remove, runtime, name)?;
    if let Some(path) = view_path {
        unmount_git_view(hooks, ContainerSite::UnmountGitView, view, path)?;
    }
    remove_intent(hooks, ContainerSite::RemoveIntent, private_root, name)
}

pub fn observe_terminated(
    runtime: &dyn ContainerRuntime,
    name: &ContainerName,
) -> Result<Liveness, UpstrokeError> {
    for _ in 0..TERMINATION_OBSERVATIONS {
        let state = runtime.observe(name.as_str()).map_err(refused)?;
        if state.is_terminated() {
            return Ok(state);
        }
    }
    Err(UpstrokeError::Refused {
        message: format!(
            "`{name}` is still running after {TERMINATION_OBSERVATIONS} observations and \
             cannot be observed terminated; a dead owner's or dead incarnation's labeled \
             container that cannot be observed terminated blocks admission \
             (transaction_fault_matrix[T-CONTAINER].refusal_condition)"
        ),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrphanWindow {
    ClosedByTheUnixReaper,
    UntilNextWriteCommandStart,
}

impl OrphanWindow {
    pub const ALL: &'static [Self] = &[
        Self::ClosedByTheUnixReaper,
        Self::UntilNextWriteCommandStart,
    ];

    #[must_use]
    pub const fn closed_by_a_reaper(self) -> bool {
        match self {
            Self::ClosedByTheUnixReaper => true,
            Self::UntilNextWriteCommandStart => false,
        }
    }
}

#[must_use]
pub const fn orphan_window() -> OrphanWindow {
    #[cfg(unix)]
    {
        OrphanWindow::ClosedByTheUnixReaper
    }
    #[cfg(not(unix))]
    {
        OrphanWindow::UntilNextWriteCommandStart
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundIntent {
    pub name: ContainerName,
    pub path: PathBuf,
    pub record: ContainerIntent,
}

pub fn read_intent(path: &Path) -> Result<ContainerIntent, UpstrokeError> {
    let bytes = fs::read(path).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|error| UpstrokeError::Refused {
        message: format!("`{}` is not a container intent: {error}", path.display()),
    })
}

fn read_racing(path: &Path) -> Result<Option<ContainerIntent>, UpstrokeError> {
    let mut last = None;
    for attempt in 0..RACING_ACCESS_ATTEMPTS {
        match read_intent(path) {
            Ok(record) => return Ok(Some(record)),
            Err(UpstrokeError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(None);
            }
            Err(UpstrokeError::Io { source, .. }) => {
                last = Some(source);
                racing_pause(attempt);
            }
            Err(other) => return Err(other),
        }
    }
    Err(UpstrokeError::Io {
        path: path.to_path_buf(),
        source: last.unwrap_or_else(|| {
            std::io::Error::other("the record could not be read and reported no reason")
        }),
    })
}

pub fn list_intents(private_root: &Path) -> Result<Vec<FoundIntent>, UpstrokeError> {
    let dir = containers_dir(private_root);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(UpstrokeError::Io { path: dir, source }),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| UpstrokeError::Io {
            path: dir.clone(),
            source,
        })?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.ends_with(INTENT_STAGED_SUFFIX) {
            continue;
        }
        let Some(name) = ContainerName::from_intent_file_name(&file_name)? else {
            continue;
        };
        let path = entry.path();
        let Some(record) = read_racing(&path)? else {
            continue;
        };
        found.push(FoundIntent { name, path, record });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedIntent {
    pub name: ContainerName,
    pub path: PathBuf,
    pub record: Option<ContainerIntent>,
}

pub fn list_staged_intents(private_root: &Path) -> Result<Vec<StagedIntent>, UpstrokeError> {
    let dir = containers_dir(private_root);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(UpstrokeError::Io { path: dir, source }),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| UpstrokeError::Io {
            path: dir.clone(),
            source,
        })?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = file_name.strip_suffix(INTENT_STAGED_SUFFIX) else {
            continue;
        };
        let name = ContainerName::rebuild(stem)?;
        if name.intent_path(private_root).exists() {
            continue;
        }
        let path = entry.path();
        let record = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<ContainerIntent>(&bytes).ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(UpstrokeError::Io { path, source }),
        };
        found.push(StagedIntent { name, path, record });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

pub fn remove_staged_intent(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    private_root: &Path,
    name: &ContainerName,
) -> Result<(), UpstrokeError> {
    remove_intent(hooks, site, private_root, name)
}

fn refused(error: RuntimeError) -> UpstrokeError {
    UpstrokeError::Refused {
        message: error.to_string(),
    }
}

fn staged_path(path: &Path) -> PathBuf {
    let mut staged = path.as_os_str().to_owned();
    staged.push(".tmp");
    PathBuf::from(staged)
}

fn write_synced(path: &Path, bytes: &[u8], trace: &ContainerTrace) -> Result<(), UpstrokeError> {
    let parent = path.parent().ok_or_else(|| UpstrokeError::Git {
        message: format!("{} has no parent directory", path.display()),
    })?;
    fs::create_dir_all(parent).map_err(|source| UpstrokeError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let staged = staged_path(path);
    {
        let mut file = fs::File::create(&staged).map_err(|source| UpstrokeError::Io {
            path: staged.clone(),
            source,
        })?;
        file.write_all(bytes).map_err(|source| UpstrokeError::Io {
            path: staged.clone(),
            source,
        })?;
        util::fsync_file(&file).map_err(|source| UpstrokeError::Io {
            path: staged.clone(),
            source,
        })?;
    }
    trace.durable(DurableStep::Synced, &staged);
    fs::rename(&staged, path).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    trace.durable(DurableStep::Renamed, path);
    util::fsync_dir(parent).map_err(|source| UpstrokeError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    trace.durable(DurableStep::DirSynced, parent);
    Ok(())
}

fn remove_if_present(path: &Path, trace: &ContainerTrace) -> Result<(), UpstrokeError> {
    if racing_removal(path, || fs::remove_file(path))? {
        trace.durable(DurableStep::Removed, path);
    }
    Ok(())
}

fn racing_removal(
    path: &Path,
    mut remove: impl FnMut() -> Result<(), std::io::Error>,
) -> Result<bool, UpstrokeError> {
    let mut last = None;
    for attempt in 0..RACING_ACCESS_ATTEMPTS {
        match remove() {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                last = Some(error);
                racing_pause(attempt);
            }
        }
    }
    Err(UpstrokeError::Io {
        path: path.to_path_buf(),
        source: last.unwrap_or_else(|| {
            std::io::Error::other("the path could not be removed and reported no reason")
        }),
    })
}

pub const DOCKER_PROGRAM: &str = "docker";

#[derive(Debug, Clone, Default)]
pub struct DockerCli {
    trace: ContainerTrace,
}

impl DockerCli {
    #[must_use]
    pub fn new(trace: ContainerTrace) -> Self {
        Self { trace }
    }

    #[must_use]
    pub fn available() -> bool {
        util::find_program(DOCKER_PROGRAM).is_some()
            && Self::default()
                .exec(
                    RuntimeOp::Probe,
                    "daemon",
                    &["version", "--format", "{{.Server.Version}}"],
                )
                .is_ok()
    }

    fn exec(&self, op: RuntimeOp, target: &str, args: &[&str]) -> Result<String, RuntimeError> {
        self.exec_streams(op, target, args)
            .map(|(stdout, _)| String::from_utf8_lossy(&stdout).into_owned())
    }

    fn exec_streams(
        &self,
        op: RuntimeOp,
        target: &str,
        args: &[&str],
    ) -> Result<(Vec<u8>, Vec<u8>), RuntimeError> {
        self.trace.runtime(op, target);
        let output = Command::new(DOCKER_PROGRAM)
            .args(args)
            .output()
            .map_err(|error| RuntimeError::Unreachable {
                operation: op,
                detail: format!("{DOCKER_PROGRAM} could not be started: {error}"),
            })?;
        if output.status.success() {
            return Ok((output.stdout, output.stderr));
        }
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(classify_docker_failure(op, detail))
    }

    fn inspect(
        &self,
        op: RuntimeOp,
        target: &str,
        args: &[&str],
    ) -> Result<Option<String>, RuntimeError> {
        match self.exec(op, target, args) {
            Ok(text) => Ok(Some(text)),
            Err(RuntimeError::Failed { detail, .. }) if is_absent(&detail) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn image(
        &self,
        op: RuntimeOp,
        reference: &str,
    ) -> Result<Option<ImageInspectionRaw>, RuntimeError> {
        let Some(text) = self.inspect(
            op,
            reference,
            &[
                "image",
                "inspect",
                reference,
                "--format",
                "{{.Id}}\u{1f}{{join .RepoDigests \",\"}}\u{1f}{{join .RepoTags \",\"}}",
            ],
        )?
        else {
            return Ok(None);
        };
        let line = text.lines().next().unwrap_or_default();
        let fields: Vec<&str> = line.split('\u{1f}').collect();
        let id = fields
            .first()
            .copied()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if id.is_empty() {
            return Err(RuntimeError::Failed {
                operation: op,
                detail: format!("`docker image inspect {reference}` reported no image id"),
            });
        }
        Ok(Some(ImageInspectionRaw {
            id,
            digests: split_list(fields.get(1).copied().unwrap_or_default()),
            tags: split_list(fields.get(2).copied().unwrap_or_default()),
        }))
    }
}

struct ImageInspectionRaw {
    id: String,
    digests: Vec<String>,
    tags: Vec<String>,
}

impl ImageInspectionRaw {
    fn into_inspection(self) -> runtime::ImageInspection {
        runtime::ImageInspection {
            id: self.id,
            digest: self
                .digests
                .into_iter()
                .next()
                .and_then(|entry| entry.rsplit('@').next().map(str::to_owned)),
            references: self.tags,
        }
    }
}

fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

const PS_FIELD_SEPARATOR: char = '\u{1f}';

const PS_FORMAT: &str = "{{.Names}}\u{1f}{{.Label \"upstroke.private_root\"}}\
     \u{1f}{{.Label \"upstroke.run\"}}\u{1f}{{.Label \"upstroke.run_dir\"}}\
     \u{1f}{{.Label \"upstroke.incarnation\"}}\u{1f}{{.Label \"upstroke.invocation\"}}";

const PS_LABELS: &[&str] = &[
    intent::LABEL_PRIVATE_ROOT,
    intent::LABEL_RUN,
    intent::LABEL_RUN_DIR,
    intent::LABEL_INCARNATION,
    intent::LABEL_INVOCATION,
];

fn parse_ps_output(text: &str) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
    let expected = 1 + PS_LABELS.len();
    let mut found = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split(PS_FIELD_SEPARATOR).collect();
        if fields.len() != expected {
            return Err(RuntimeError::Failed {
                operation: RuntimeOp::ListByLabel,
                detail: format!(
                    "`docker ps` rendered {} field(s) for one container and this format asks for \
                     {expected}: {line:?}. A label value carrying the field separator or a line \
                     terminator makes the values stop lining up with their names, and a census \
                     that guessed which field was the owner's run directory would probe another \
                     run's lock",
                    fields.len()
                ),
            });
        }
        let name = fields[0].trim().to_owned();
        if name.is_empty() {
            continue;
        }
        let mut labels = BTreeMap::new();
        for (key, value) in PS_LABELS.iter().zip(&fields[1..]) {
            if !value.is_empty() {
                labels.insert((*key).to_owned(), (*value).to_owned());
            }
        }
        found.push(DiscoveredContainer { name, labels });
    }
    Ok(found)
}

const UNREACHABLE_DIAGNOSTICS: &[&str] = &[
    "cannot connect to the docker daemon",
    "error during connect",
    "is the docker daemon running",
    "if the daemon is running",
    "permission denied while trying to connect",
    "failed to connect to the docker api",
    "the docker daemon is not running",
];

#[must_use]
pub fn is_unreachable_diagnostic(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    UNREACHABLE_DIAGNOSTICS
        .iter()
        .any(|shape| lower.contains(shape))
}

#[must_use]
pub fn classify_docker_failure(operation: RuntimeOp, detail: String) -> RuntimeError {
    if is_unreachable_diagnostic(&detail) {
        return RuntimeError::Unreachable { operation, detail };
    }
    RuntimeError::Failed { operation, detail }
}

fn is_absent(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("no such object")
        || lower.contains("no such container")
        || lower.contains("no such image")
        || lower.contains("no such volume")
}

pub const REMOVAL_IN_PROGRESS: &str = "is already in progress";

fn remove_already_settled(detail: &str) -> bool {
    is_absent(detail) || detail.to_ascii_lowercase().contains(REMOVAL_IN_PROGRESS)
}

fn settle_remove(outcome: Result<String, RuntimeError>) -> Result<(), RuntimeError> {
    match outcome {
        Ok(_) => Ok(()),
        Err(RuntimeError::Failed { detail, .. }) if remove_already_settled(&detail) => Ok(()),
        Err(error) => Err(error),
    }
}

impl ContainerRuntime for DockerCli {
    fn probe(&self) -> Result<(), RuntimeError> {
        self.exec(
            RuntimeOp::Probe,
            "daemon",
            &["version", "--format", "{{.Server.Version}}"],
        )
        .map(|_| ())
    }

    fn image_by_reference(
        &self,
        reference: &str,
    ) -> Result<Option<runtime::ImageInspection>, RuntimeError> {
        Ok(self
            .image(RuntimeOp::InspectImageByReference, reference)?
            .map(ImageInspectionRaw::into_inspection))
    }

    fn image_by_id(&self, id: &str) -> Result<Option<runtime::ImageInspection>, RuntimeError> {
        let Some(found) = self.image(RuntimeOp::InspectImageById, id)? else {
            return Ok(None);
        };
        if found.id != id {
            return Ok(None);
        }
        Ok(Some(found.into_inspection()))
    }

    fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
        Ok(self
            .inspect(
                RuntimeOp::InspectVolume,
                name,
                &["volume", "inspect", name, "--format", "{{.Name}}"],
            )?
            .is_some())
    }

    fn containers_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
        let filter = format!("label={key}={value}");
        let text = self.exec(
            RuntimeOp::ListByLabel,
            value,
            &["ps", "--all", "--filter", &filter, "--format", PS_FORMAT],
        )?;
        parse_ps_output(&text)
    }

    fn observe(&self, name: &str) -> Result<Liveness, RuntimeError> {
        let Some(text) = self.inspect(
            RuntimeOp::Observe,
            name,
            &[
                "container",
                "inspect",
                name,
                "--format",
                "{{.State.Status}}",
            ],
        )?
        else {
            return Ok(Liveness::Gone);
        };
        match text.trim() {
            "running" | "restarting" | "paused" | "removing" => Ok(Liveness::Running),
            _ => Ok(Liveness::Exited),
        }
    }

    fn collect(&self, name: &str) -> Result<ContainerExecution, RuntimeError> {
        let status = self
            .inspect(
                RuntimeOp::Collect,
                name,
                &[
                    "container",
                    "inspect",
                    name,
                    "--format",
                    "{{.State.ExitCode}}",
                ],
            )?
            .ok_or_else(|| RuntimeError::Failed {
                operation: RuntimeOp::Collect,
                detail: format!("`{name}` is gone, so its exit status cannot be collected"),
            })?;
        let exit_code = status.trim().parse::<i32>().ok();
        let (stdout, stderr) = self.exec_streams(RuntimeOp::Collect, name, &["logs", name])?;
        Ok(ContainerExecution {
            exit_code,
            stdout,
            stderr,
        })
    }

    fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {
        let mut args: Vec<String> =
            vec!["create".to_owned(), "--name".to_owned(), spec.name.clone()];
        if spec.read_only_root {
            args.push("--read-only".to_owned());
        }
        for (key, value) in &spec.labels {
            args.push("--label".to_owned());
            args.push(format!("{key}={value}"));
        }
        for mount in &spec.mounts {
            args.push("--mount".to_owned());
            args.push(mount_argument(mount));
        }
        for (key, value) in &spec.env {
            args.push("--env".to_owned());
            args.push(format!("{key}={value}"));
        }
        if let Some(workdir) = &spec.workdir {
            args.push("--workdir".to_owned());
            args.push(workdir.clone());
        }
        args.push(spec.image_id.clone());
        args.extend(spec.command.iter().cloned());
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.exec(RuntimeOp::Create, &spec.name, &borrowed)?;
        let reported = self
            .inspect(
                RuntimeOp::Create,
                &spec.name,
                &["container", "inspect", &spec.name, "--format", "{{.Image}}"],
            )?
            .ok_or_else(|| RuntimeError::Failed {
                operation: RuntimeOp::Create,
                detail: format!("`{}` was created and cannot be inspected", spec.name),
            })?;
        Ok(CreatedContainer {
            name: spec.name.clone(),
            reported_image_id: reported.trim().to_owned(),
        })
    }

    fn start(&self, name: &str) -> Result<(), RuntimeError> {
        self.exec(RuntimeOp::Start, name, &["start", name])
            .map(|_| ())
    }

    fn stop(&self, name: &str, mode: StopMode) -> Result<(), RuntimeError> {
        let verb = match mode {
            StopMode::Graceful => "stop",
            StopMode::Kill => "kill",
        };
        settle_stop(self.exec(RuntimeOp::Stop, name, &[verb, name]))
    }

    fn remove(&self, name: &str) -> Result<(), RuntimeError> {
        settle_remove(self.exec(
            RuntimeOp::Remove,
            name,
            &["rm", "--force", "--volumes", name],
        ))
    }
}

// The daemon owns container state and arbitrates concurrent reclaimers' stop/remove
// requests. The winner moves running -> stopped -> removing -> absent; a loser
// seeing stopped, removing or absent continues observe/remove/view/intent cleanup.
// Other failures remain errors so failed or cancelled reclamation can be retried
// from the retained intent. "Removing" is settled for stop, not proof of absence.
fn stop_already_settled(detail: &str) -> bool {
    remove_already_settled(detail) || detail.contains("is not running")
}

fn settle_stop(outcome: Result<String, RuntimeError>) -> Result<(), RuntimeError> {
    match outcome {
        Ok(_) => Ok(()),
        Err(RuntimeError::Failed { detail, .. }) if stop_already_settled(&detail) => Ok(()),
        Err(error) => Err(error),
    }
}

fn mount_argument(mount: &runtime::Mount) -> String {
    let mut parts = Vec::new();
    match mount {
        runtime::Mount::Path { source, target, .. } => {
            parts.push("type=bind".to_owned());
            parts.push(format!(
                "source={}",
                source.to_string_lossy().replace('\\', "/")
            ));
            parts.push(format!("target={target}"));
        }
        runtime::Mount::Volume { name, target, .. } => {
            parts.push("type=volume".to_owned());
            parts.push(format!("source={name}"));
            parts.push(format!("target={target}"));
        }
        runtime::Mount::Tmpfs { target } => {
            parts.push("type=tmpfs".to_owned());
            parts.push(format!("target={target}"));
        }
    }
    if mount.read_only() {
        parts.push("readonly".to_owned());
    }
    parts.join(",")
}

#[cfg(test)]
mod fake;

#[cfg(test)]
pub(crate) use fake::{
    DOCKER_GATED_TESTS, FakeOwnerLiveness, FakeRuntime, RecordingHooks, docker_gate,
};

#[cfg(test)]
impl DockerCli {
    pub(crate) fn raw(
        &self,
        op: RuntimeOp,
        target: &str,
        args: &[&str],
    ) -> Result<String, RuntimeError> {
        self.exec(op, target, args)
    }
}

#[cfg(test)]
mod tests;
