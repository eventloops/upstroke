//! Extended notes: `docs/internals/runner/container/exec.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::agent::ProcessOutput;
use crate::error::UpstrokeError;
use crate::rundir::RunPaths;
use crate::runner::policy::runner_policy_sha256;
use crate::runner::{AgentId, ExecutionRole, InvocationId, Runner, RunnerRequest};
use crate::topology::events::{RunnerContract, RunnerKind, RunnerPolicy};

use super::env::{BoundaryLayout, ContainerEnvironment, RoleScope, supplies_credential_location};
use super::intent::{ContainerIntent, ContainerName};
use super::runtime::{ContainerRuntime, ContainerTrace, CreateSpec, Mount, RuntimeError};
use super::view::{self, RoleGitView};
use super::{
    ContainerHooks, GitView, GitViewRequest, LaunchPlan, Launched, NoHooks, create_container,
    mount_git_view, start_container, write_intent,
};
use crate::topology::effects::ContainerSite;

pub const OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;

pub const SUPERVISION_POLL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunIdentity {
    pub private_root: PathBuf,
    pub run_id: String,
    pub run_dir: PathBuf,
    pub incarnation: String,
    pub repo_key: String,
}

#[must_use]
pub const fn receives_a_worktree(role: &ExecutionRole) -> bool {
    match role {
        ExecutionRole::Implement | ExecutionRole::Gate | ExecutionRole::Review => true,
        ExecutionRole::Probe(_) => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Withheld {
    PublicLog,
    SiblingWorktree,
    PrivateArtifacts,
    AuthoritativeGit,
}

impl Withheld {
    pub const ALL: &'static [Self] = &[
        Self::PublicLog,
        Self::SiblingWorktree,
        Self::PrivateArtifacts,
        Self::AuthoritativeGit,
    ];

    #[must_use]
    pub const fn passage(self) -> &'static str {
        match self {
            Self::PublicLog => "DESIGN.md:400 — it never receives the public log",
            Self::SiblingWorktree => "DESIGN.md:400 — it never receives sibling worktrees",
            Self::PrivateArtifacts => "DESIGN.md:400 — it never receives private artifacts",
            Self::AuthoritativeGit => {
                "DESIGN.md:612 — authoritative Git and the event log never cross the boundary"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confinement {
    entries: Vec<(Withheld, PathBuf)>,
}

impl Confinement {
    #[must_use]
    pub fn of_run(identity: &RunIdentity, repo_root: &Path) -> Self {
        let paths =
            RunPaths::with_private_root(repo_root, &identity.run_id, &identity.private_root);
        let execution_root = crate::workspace_manager::execution_root_of(
            &identity.private_root,
            &identity.repo_key,
            &identity.run_id,
        );
        let mut entries = vec![
            (Withheld::PublicLog, paths.public),
            (Withheld::PrivateArtifacts, paths.private),
            (
                Withheld::PrivateArtifacts,
                super::intent::containers_dir(&identity.private_root),
            ),
            (Withheld::AuthoritativeGit, repo_root.join(".git")),
            (Withheld::SiblingWorktree, execution_root.clone()),
        ];
        for namespace in worktree_namespaces(&execution_root) {
            entries.push((Withheld::SiblingWorktree, namespace));
        }
        Self { entries }
    }

    #[must_use]
    pub fn withholding(mut self, category: Withheld, path: impl Into<PathBuf>) -> Self {
        self.entries.push((category, path.into()));
        self
    }

    #[must_use]
    pub fn entries(&self) -> &[(Withheld, PathBuf)] {
        &self.entries
    }

    #[must_use]
    pub fn violations(&self, mounts: &[Mount]) -> Vec<String> {
        let mut found = Vec::new();
        for mount in mounts {
            let Mount::Path { source, target, .. } = mount else {
                continue;
            };
            for (category, withheld) in &self.entries {
                if withheld.starts_with(source) {
                    found.push(format!(
                        "the mount `{}` -> `{target}` would hand the container `{}` ({})",
                        source.display(),
                        withheld.display(),
                        category.passage()
                    ));
                }
            }
        }
        found
    }
}

fn worktree_namespaces(execution_root: &Path) -> Vec<PathBuf> {
    use crate::workspace_manager::{Slot, SnapshotName};
    let representatives = [
        Slot::Task {
            key: "0".to_owned(),
            generation: 0,
        },
        Slot::Staging { sequence: 0 },
        Slot::Snapshot {
            name: SnapshotName::gates(0, 0),
        },
    ];
    let mut namespaces: Vec<PathBuf> = representatives
        .iter()
        .filter_map(|slot| {
            slot.relative()
                .components()
                .next()
                .map(|first| execution_root.join(first))
        })
        .collect();
    namespaces.sort();
    namespaces.dedup();
    namespaces
}

pub fn recorded_image_id(policy: &RunnerPolicy) -> Result<&str, UpstrokeError> {
    if policy.kind != RunnerKind::Container || policy.policy != RunnerContract::ContainerV1 {
        return Err(UpstrokeError::Refused {
            message: format!(
                "the container runner was given a `{:?}`/`{:?}` RunnerPolicy; \
                 `container-v1` is the mount, environment, Git-view and supervision \
                 contract this runner implements (INV-23)",
                policy.kind, policy.policy
            ),
        });
    }
    let Some(image) = &policy.image else {
        return Err(UpstrokeError::Refused {
            message: "the recorded RunnerPolicy is a container policy with no image; INV-23 \
                      records `image: {reference, id, digest}` and every container is created \
                      from the recorded id"
                .to_owned(),
        });
    };
    if image.id.trim().is_empty() {
        return Err(UpstrokeError::Refused {
            message: "the recorded RunnerPolicy carries an empty image id".to_owned(),
        });
    }
    Ok(&image.id)
}

#[must_use]
pub fn recorded_volumes(policy: &RunnerPolicy) -> BTreeMap<String, String> {
    policy.credential_volumes.clone().unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct InvocationPlan {
    pub launch: LaunchPlan,
    pub git: Option<view::GitLayout>,
}

impl InvocationPlan {
    #[must_use]
    pub fn mounts(&self) -> &[Mount] {
        &self.launch.spec.mounts
    }

    #[must_use]
    pub fn env(&self) -> &[(String, String)] {
        &self.launch.spec.env
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Reached {
    view: Option<PathBuf>,
    container: bool,
}

impl Reached {
    const INTENT_ONLY: Self = Self {
        view: None,
        container: false,
    };
}

pub struct ContainerRunner {
    policy: RunnerPolicy,
    image_id: String,
    digest: String,
    volumes: BTreeMap<String, String>,
    identity: RunIdentity,
    environment: ContainerEnvironment,
    layout: BoundaryLayout,
    confinement: Confinement,
    runtime: Box<dyn ContainerRuntime>,
    view: Box<dyn GitView>,
    view_is_explicit: bool,
    hooks: Mutex<Box<dyn ContainerHooks + Send>>,
    poll: Duration,
    output_limit: usize,
}

impl std::fmt::Debug for ContainerRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerRunner")
            .field("policy", &self.policy)
            .field("digest", &self.digest)
            .field("identity", &self.identity)
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl ContainerRunner {
    pub fn new(
        policy: RunnerPolicy,
        identity: RunIdentity,
        repo_root: &Path,
        environment: ContainerEnvironment,
        runtime: Box<dyn ContainerRuntime>,
    ) -> Result<Self, UpstrokeError> {
        let image_id = recorded_image_id(&policy)?.to_owned();
        let digest = runner_policy_sha256(&policy);
        let volumes = recorded_volumes(&policy);
        let layout = BoundaryLayout::new();
        let view = RoleGitView::new(ContainerTrace::off())
            .for_reader(layout.git_view(), layout.git_objects());
        let confinement = Confinement::of_run(&identity, repo_root);
        Ok(Self {
            policy,
            image_id,
            digest,
            volumes,
            identity,
            environment,
            layout,
            confinement,
            runtime,
            view: Box::new(view),
            view_is_explicit: false,
            hooks: Mutex::new(Box::new(NoHooks)),
            poll: SUPERVISION_POLL,
            output_limit: OUTPUT_LIMIT_BYTES,
        })
    }

    #[must_use]
    pub fn with_layout(mut self, layout: BoundaryLayout) -> Self {
        self.layout = layout;
        self.rebuild_view();
        self
    }

    #[must_use]
    pub fn also_withholding(mut self, category: Withheld, path: impl Into<PathBuf>) -> Self {
        self.confinement = self.confinement.withholding(category, path);
        self
    }

    #[must_use]
    pub fn with_hooks(mut self, hooks: Box<dyn ContainerHooks + Send>) -> Self {
        self.hooks = Mutex::new(hooks);
        self.rebuild_view();
        self
    }

    #[must_use]
    pub fn with_view(mut self, view: Box<dyn GitView>) -> Self {
        self.view = view;
        self.view_is_explicit = true;
        self
    }

    fn rebuild_view(&mut self) {
        if self.view_is_explicit {
            return;
        }
        self.view = Box::new(
            RoleGitView::new(self.trace())
                .for_reader(self.layout.git_view(), self.layout.git_objects()),
        );
    }

    #[must_use]
    pub const fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    #[must_use]
    pub const fn with_output_limit(mut self, bytes: usize) -> Self {
        self.output_limit = bytes;
        self
    }

    #[must_use]
    pub const fn policy(&self) -> &RunnerPolicy {
        &self.policy
    }

    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.digest
    }

    #[must_use]
    pub const fn layout(&self) -> &BoundaryLayout {
        &self.layout
    }

    #[must_use]
    pub const fn environment(&self) -> &ContainerEnvironment {
        &self.environment
    }

    #[must_use]
    pub const fn confinement(&self) -> &Confinement {
        &self.confinement
    }

    fn trace(&self) -> ContainerTrace {
        self.hooks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .trace()
    }

    pub fn plan(&self, request: &RunnerRequest) -> Result<InvocationPlan, UpstrokeError> {
        let name = ContainerName::new(
            &self.identity.repo_key,
            &self.identity.run_id,
            &self.identity.incarnation,
            &request.invocation,
        )?;
        let intent = ContainerIntent::new(
            self.identity.run_id.clone(),
            &self.identity.run_dir,
            self.identity.incarnation.clone(),
            self.identity.repo_key.clone(),
            request.invocation.render(),
            self.digest.clone(),
        );

        let git = if receives_a_worktree(&request.role) {
            view::resolve(&request.workspace)?
        } else {
            None
        };
        let mounts = self.mounts(request, git.as_ref(), &name);
        let mut confinement = self.confinement.clone();
        if let Some(layout) = &git {
            confinement =
                confinement.withholding(Withheld::AuthoritativeGit, layout.common_dir.clone());
        }
        let violations = confinement.violations(&mounts);
        if !violations.is_empty() {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the container for `{}` would receive a path this run withholds: {}",
                    request.invocation.render(),
                    violations.join("; ")
                ),
            });
        }

        let scope = RoleScope {
            role: &request.role,
            agent: request.agent.as_ref(),
            volumes: &self.volumes,
            layout: &self.layout,
        };
        let env = self.environment.compose(&scope, &request.command.env)?;

        let mut command = vec![request.command.program.clone()];
        command.extend(request.command.args.iter().cloned());
        let view_path = view_dir(&self.identity.private_root, &name);

        Ok(InvocationPlan {
            launch: LaunchPlan {
                private_root: self.identity.private_root.clone(),
                invocation: request.invocation.clone(),
                spec: CreateSpec {
                    name: name.as_str().to_owned(),
                    image_id: self.image_id.clone(),
                    labels: intent.labels(&self.identity.private_root),
                    mounts,
                    env,
                    command,
                    workdir: Some(if receives_a_worktree(&request.role) {
                        self.layout.workspace().to_owned()
                    } else {
                        self.layout.scratch().to_owned()
                    }),
                    read_only_root: true,
                },
                view: GitViewRequest {
                    path: view_path.clone(),
                    workspace: if receives_a_worktree(&request.role) {
                        request.workspace.clone()
                    } else {
                        view_path
                    },
                    head: None,
                },
                name,
                intent,
            },
            git,
        })
    }

    fn mounts(
        &self,
        request: &RunnerRequest,
        git: Option<&view::GitLayout>,
        name: &ContainerName,
    ) -> Vec<Mount> {
        let mut mounts = Vec::new();
        if receives_a_worktree(&request.role) {
            mounts.push(Mount::Path {
                source: request.workspace.clone(),
                target: self.layout.workspace().to_owned(),
                read_only: request.role == ExecutionRole::Review,
            });
        }
        if let Some(layout) = git {
            let view = view_dir(&self.identity.private_root, name);
            mounts.push(Mount::Path {
                source: view.clone(),
                target: self.layout.git_view().to_owned(),
                read_only: false,
            });
            mounts.push(Mount::Path {
                source: layout.objects.clone(),
                target: self.layout.git_objects().to_owned(),
                read_only: true,
            });
            let source = if layout.dot_git_is_file {
                view.join(view::WORKTREE_GITFILE)
            } else {
                view
            };
            mounts.push(Mount::Path {
                source,
                target: self.layout.git_pointer(),
                read_only: false,
            });
        }
        if supplies_credential_location(&request.role) {
            if let Some(agent) = request.agent.as_ref() {
                if let Some(volume) = self.volumes.get(agent.as_str()) {
                    mounts.push(Mount::Volume {
                        name: volume.clone(),
                        target: self.layout.credentials(agent),
                        read_only: false,
                    });
                }
            }
        }
        mounts.push(Mount::Tmpfs {
            target: self.layout.scratch().to_owned(),
        });
        mounts
    }

    #[must_use]
    pub fn credential_volume_for(
        &self,
        role: &ExecutionRole,
        agent: Option<&AgentId>,
    ) -> Option<&str> {
        if !supplies_credential_location(role) {
            return None;
        }
        agent
            .and_then(|agent| self.volumes.get(agent.as_str()))
            .map(String::as_str)
    }

    fn launch(
        &self,
        hooks: &mut dyn ContainerHooks,
        plan: &LaunchPlan,
    ) -> Result<Launched, UpstrokeError> {
        let written = match write_intent(
            hooks,
            ContainerSite::WriteIntent,
            &plan.private_root,
            &plan.name,
            &plan.intent,
        ) {
            Ok(written) => written,
            Err(error) => {
                return Err(self.cancelled(hooks, plan, error, Reached::INTENT_ONLY));
            }
        };
        let intent_path = written.path().to_path_buf();
        let view_path = match mount_git_view(
            hooks,
            ContainerSite::MountGitView,
            self.view.as_ref(),
            &plan.view,
        ) {
            Ok(path) => path,
            Err(error) => {
                return Err(self.cancelled(
                    hooks,
                    plan,
                    error,
                    Reached {
                        view: Some(plan.view.path.clone()),
                        container: false,
                    },
                ));
            }
        };
        let created = match create_container(
            hooks,
            ContainerSite::Create,
            self.runtime.as_ref(),
            &written,
            &plan.spec,
        ) {
            Ok(created) => created,
            Err(error) => {
                return Err(self.cancelled(
                    hooks,
                    plan,
                    error,
                    Reached {
                        view: Some(view_path),
                        container: true,
                    },
                ));
            }
        };
        if created.reported_image_id != plan.spec.image_id {
            let refusal = ImageIdMismatch::of(&plan.invocation).error(
                &plan.name,
                &created.reported_image_id,
                &plan.spec.image_id,
            );
            return Err(self.cancelled(
                hooks,
                plan,
                refusal,
                Reached {
                    view: Some(view_path),
                    container: true,
                },
            ));
        }
        if let Err(error) =
            start_container(hooks, ContainerSite::Start, self.runtime.as_ref(), &written)
        {
            return Err(self.cancelled(
                hooks,
                plan,
                error,
                Reached {
                    view: Some(view_path),
                    container: true,
                },
            ));
        }
        Ok(Launched {
            name: plan.name.clone(),
            intent_path,
            view_path,
            reported_image_id: created.reported_image_id,
        })
    }

    fn cancelled(
        &self,
        hooks: &mut dyn ContainerHooks,
        plan: &LaunchPlan,
        cause: UpstrokeError,
        reached: Reached,
    ) -> UpstrokeError {
        let residue = self.cancel(hooks, &plan.private_root, &plan.name, &reached);
        if residue.is_empty() {
            return cause;
        }
        UpstrokeError::Refused {
            message: format!(
                "{cause}. The cancel could not release everything the failed launch created, \
                 so this run's R19/R26 ledgers do not balance and a census will find the \
                 residue: {}",
                residue.join("; ")
            ),
        }
    }

    fn cancel(
        &self,
        hooks: &mut dyn ContainerHooks,
        private_root: &Path,
        name: &ContainerName,
        reached: &Reached,
    ) -> Vec<String> {
        super::cancel_reached(
            hooks,
            self.runtime.as_ref(),
            self.view.as_ref(),
            private_root,
            name,
            reached.container,
            reached.view.as_deref(),
        )
    }

    fn release(
        &self,
        hooks: &mut dyn ContainerHooks,
        private_root: &Path,
        launched: &Launched,
    ) -> Result<(), UpstrokeError> {
        super::release(
            hooks,
            self.runtime.as_ref(),
            self.view.as_ref(),
            private_root,
            launched,
        )
    }

    fn supervise(&self, name: &ContainerName, deadline: Instant) -> Result<bool, UpstrokeError> {
        loop {
            let state = self
                .runtime
                .observe(name.as_str())
                .map_err(refused_by_runtime)?;
            if state.is_terminated() {
                return Ok(false);
            }
            if Instant::now() >= deadline {
                return Ok(true);
            }
            if !self.poll.is_zero() {
                std::thread::sleep(self.poll);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImageIdMismatch {
    RefusedBeforeStart,
    SpawnFailureOutage,
}

impl ImageIdMismatch {
    pub const ALL: &'static [Self] = &[Self::RefusedBeforeStart, Self::SpawnFailureOutage];

    #[must_use]
    pub const fn of(invocation: &InvocationId) -> Self {
        match invocation {
            InvocationId::Probe { .. } => Self::RefusedBeforeStart,
            InvocationId::Attempt { .. } | InvocationId::Sequence { .. } => {
                Self::SpawnFailureOutage
            }
        }
    }

    #[must_use]
    pub fn error(self, name: &ContainerName, reported: &str, recorded: &str) -> UpstrokeError {
        let message = format!(
            "the container runtime created `{name}` and reports image id `{reported}`, and the \
             run's recorded image id is `{recorded}`; a created container whose reported image \
             id differs from the record is refused before start (INV-23){}",
            match self {
                Self::RefusedBeforeStart => String::new(),
                Self::SpawnFailureOutage => format!(
                    ". This invocation is mid-run, so the boundary the run recorded could not be \
                     entered for `{name}` and the attempt settles as a RunnerSpawnFailure outage \
                     rather than failing on its own"
                ),
            }
        );
        match self {
            Self::RefusedBeforeStart => UpstrokeError::Refused { message },
            Self::SpawnFailureOutage => UpstrokeError::Agent { message },
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RefusedBeforeStart => "refused-before-start",
            Self::SpawnFailureOutage => "runner-spawn-failure-outage",
        }
    }
}

#[must_use]
pub fn view_dir(private_root: &Path, name: &ContainerName) -> PathBuf {
    super::census::view_path(private_root, name)
}

impl Runner for ContainerRunner {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        let plan = self.plan(request)?;
        let started = Instant::now();
        let deadline = started + request.timeout;
        let mut hooks = self.hooks.lock().unwrap_or_else(PoisonError::into_inner);

        let launched: Launched = self.launch(&mut **hooks, &plan.launch)?;

        let outcome = self.finish(&launched, started, deadline);
        let released = self.release(&mut **hooks, &self.identity.private_root, &launched);
        let output = outcome?;
        released?;
        Ok(output)
    }
}

impl ContainerRunner {
    fn finish(
        &self,
        launched: &Launched,
        started: Instant,
        deadline: Instant,
    ) -> Result<ProcessOutput, UpstrokeError> {
        let timed_out = self.supervise(&launched.name, deadline)?;
        let execution = self
            .runtime
            .collect(launched.name.as_str())
            .map_err(refused_by_runtime)?;
        let (stdout, stdout_limited) = bounded(&execution.stdout, self.output_limit);
        let (stderr, stderr_limited) = bounded(&execution.stderr, self.output_limit);
        Ok(ProcessOutput {
            code: if timed_out { None } else { execution.exit_code },
            stdout,
            stderr,
            duration: started.elapsed(),
            timed_out,
            output_limited: stdout_limited || stderr_limited,
        })
    }
}

fn refused_by_runtime(error: RuntimeError) -> UpstrokeError {
    UpstrokeError::Refused {
        message: error.to_string(),
    }
}

fn bounded(bytes: &[u8], limit: usize) -> (String, bool) {
    if bytes.len() <= limit {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let mut end = limit;
    while end > 0 && (bytes[end] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    (String::from_utf8_lossy(&bytes[..end]).into_owned(), true)
}

#[cfg(test)]
mod tests;
