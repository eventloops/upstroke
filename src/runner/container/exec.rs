//! The container [`Runner`]: mounts, environment, supervision, and the one
//! path every container invocation of a run takes.
//!
//! DESIGN.md:118 gives a runner "cwd, mounts, environment, supervision, and
//! timeout, never agent semantics or Git", and DESIGN.md:612 narrows what this
//! one may know: "the runner learns nothing about agent semantics beyond
//! **which per-agent credential volume to mount**". That sentence is the whole
//! design of this module — the only agent-shaped thing in it is a volume name
//! taken from the run's recorded `RunnerPolicy`.
//!
//! ## Everything goes through one function, and that is load-bearing
//!
//! DESIGN.md:263: "**Probe and execution compose the same base, mounts,
//! reserved values, and overlay**, so pre-flight certifies the environment that
//! will actually spend." The natural implementation is two call sites that
//! happen to agree today, and it satisfies the sentence by accident until
//! somebody edits one of them. Here there is one: [`ContainerRunner::run`], and
//! the `RunnerPreflight` shell probe reaches it through
//! [`crate::runner::host::run_shell_probe`] — a free function over `&dyn
//! Runner`, written by PR4 for exactly this and not re-implemented here.
//! `tests::probe_and_execution_compose_through_one_code_path` counts the
//! composition sites in this module's production region and asserts there is
//! one.
//!
//! ## Ordering, and why this module does not call [`super::launch`]
//!
//! `slice_contract.side_effect_vs_event_ordering`: "no events; **intent synced
//! before docker create**; container created from the recorded id and
//! **verified before start**; **view mounted before start**; stop/rm, view
//! removal, intent removal after completion". Four independently droppable
//! predicates, and [`ContainerRunner::launch`] performs them in one place with
//! [`super::runtime::ContainerTrace`] recording the sequence.
//!
//! [`super::launch`] performs the same four sites in the order
//! `WriteIntent -> Create -> MountGitView -> Start`, which satisfies every
//! clause above and **cannot produce a working container**: the Git view is a
//! **bind-mount source** of the `docker create` call, and a bind source must
//! exist when the container is created. Measured against `docker` 29.7.2 —
//! `invalid mount config for type "bind": bind source path does not exist` —
//! which is what `real_docker_a_git_dependent_gate_sees_only_the_role_view`
//! reported the first time it ran. So the order here is
//! `WriteIntent -> MountGitView -> Create(+verify) -> Start`, which holds all
//! four clauses *and* works, and the eight site-taking APIs are called
//! directly rather than through a convenience whose order this caller cannot
//! use. The one-line repair to `super::launch` is
//! `PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE` in the report; it is a lane F
//! file and is not changed from here.
//!
//! **`T-CONTAINER.boundary` reads "docker start issued; Git view mounted" and
//! the contract clause reads the opposite.** `RECONCILIATION-OBLIGATION.md` §C1
//! rules that `side_effect_vs_event_ordering` governs, and the measurement
//! above is a third, independent reason: a bind mount is declared at `create`
//! and cannot be added to a running container, so `T-CONTAINER`'s prose order
//! is not merely non-conforming — it does not run.

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

/// How much of a container's output is captured.
///
/// The host funnel bounds capture at 16 MiB per stream and terminates the tree
/// that exceeds it (`agent::proc`). A container runtime hands back whatever the
/// container wrote, so the bound is applied here — and the container is stopped
/// and removed either way, which is the same disposition the host's supervisor
/// reaches. Without it `ProcessOutput::output_limited` would be `false` for
/// every container invocation and
/// [`crate::runner::host::run_shell_probe`]'s bounded-output refusal would be
/// unreachable at this boundary while remaining reachable at the other — a
/// pre-flight that certifies less than the one it is paired with.
pub const OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// How often the supervisor asks whether the container has finished.
///
/// `decisions.tests_acceptance.determinism` forbids sleeps in the suite, so
/// this is a value and every test sets it to zero. A container that finishes
/// between two observations is observed at the second; a container that does
/// not finish by the request's deadline is stopped and removed.
pub const SUPERVISION_POLL: Duration = Duration::from_millis(25);

// ---------------------------------------------------------------------------
// Who owns the containers
// ---------------------------------------------------------------------------

/// The run whose containers these are.
///
/// The five fields the intent record carries that are properties of the *run*
/// rather than of the invocation — `crash_reconstruction`'s "owner run id, run
/// directory (public path), coordinator incarnation id, repo key" plus the
/// private root the namespace lives under. The sixth and seventh, the
/// invocation and the runner digest, come from the request and the policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunIdentity {
    /// `<R>` — the run's **recorded** private root.
    pub private_root: PathBuf,
    /// The owner run id.
    pub run_id: String,
    /// The owner's **public** run directory.
    pub run_dir: PathBuf,
    /// The coordinator incarnation id: a per-process ULID, never read from a
    /// lock file.
    pub incarnation: String,
    /// The repo key.
    pub repo_key: String,
}

/// Whether this role receives its role's worktree.
///
/// DESIGN.md:400: "A container receives only **its role's one worktree** mount".
/// A probe has no worktree. [`crate::agent::probe_workspace`]'s own words are
/// "a probe asks a CLI about itself and **has no workspace of its own**", and
/// the value it returns is the **coordinator's current working directory** —
/// which at the host boundary is harmless and at this one is the repository
/// itself: the public log and authoritative Git in a single mount. So a probe's
/// container receives no worktree, no Git projection and no working directory,
/// and certifies exactly what a probe is for: that the recorded shell, or the
/// recorded agent CLI, runs inside the recorded image.
///
/// This is a **boundary** decision, which is what DESIGN.md:118 gives a runner
/// ("owns cwd, mounts, environment"), not a change to what a probe *is*: the
/// request, its role, its slot accounting and its `InvocationId` are untouched,
/// and the same request executes on the host exactly as it did before.
/// `PR6A-PROBE-WORKSPACE-IS-THE-COORDINATORS-CWD` in the report records the
/// other half — that a caller which wants a probe to have a workspace has no
/// way to say so.
///
/// Exhaustive with no wildcard: a role added later has to be classified here.
#[must_use]
pub const fn receives_a_worktree(role: &ExecutionRole) -> bool {
    match role {
        ExecutionRole::Implement | ExecutionRole::Gate | ExecutionRole::Review => true,
        ExecutionRole::Probe(_) => false,
    }
}

// ---------------------------------------------------------------------------
// The negative space
// ---------------------------------------------------------------------------

/// What a container must never receive.
///
/// DESIGN.md:400 names three — "A container receives only its role's one
/// worktree mount; it never receives **the public log**, **sibling worktrees**,
/// or **private artifacts**" — and DESIGN.md:612 names the fourth: "Workers,
/// repository-controlled gates, and reviewers all cross the boundary;
/// **authoritative Git** and the event log never do."
///
/// An enumeration rather than a list of paths, because the paths are derived
/// per run and the *categories* are what the passages fix. A category added
/// later has to name its passage here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Withheld {
    /// `<repo>/.upstroke/runs/<run-id>` — `events.jsonl`, the frozen plan,
    /// questions, answers, artifacts.
    PublicLog,
    /// Every other role's worktree of this run, and the integration staging
    /// worktree.
    SiblingWorktree,
    /// `<R>/runs/<run-id>` — transcripts, reviews, per-attempt settings, gate
    /// logs — and `<R>/containers`, which is every container's ownership
    /// evidence.
    PrivateArtifacts,
    /// The repository's shared Git directory: every engine ref, and the
    /// coordinator's own `HEAD`.
    AuthoritativeGit,
}

impl Withheld {
    /// All four. Written out so a grid over categories is a grid over all of
    /// them.
    pub const ALL: &'static [Self] = &[
        Self::PublicLog,
        Self::SiblingWorktree,
        Self::PrivateArtifacts,
        Self::AuthoritativeGit,
    ];

    /// The passage that withholds it.
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

/// The host paths one run withholds from every container of that run.
///
/// Built from [`RunPaths`] and [`crate::workspace_manager::execution_root_of`]
/// rather than from a list written here, so a layout change moves this set with
/// it. That is the point: "a test that checks *the worktree is mounted* passes
/// on a container that also mounts `/`", and a hand-written forbidden list
/// passes on a layout that has moved.
/// **There is no empty `Confinement` and no `Default`**, which is the repair
/// for `PR6-CORRECTNESS-011` / `PR6-ENUM-002`: [`Self::of_run`] is the only
/// constructor, so a `ContainerRunner` that withholds nothing is not a value
/// this module can produce. The shape is `PR4-CONF-003`'s — the property is
/// established by the function that derives it from the layout, not asserted by
/// a caller who remembered to call a builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confinement {
    entries: Vec<(Withheld, PathBuf)>,
}

impl Confinement {
    /// Everything `identity`'s run withholds.
    ///
    /// Derived from the types that own the layout — [`RunPaths`] for the run's
    /// two halves and [`crate::workspace_manager::execution_root_of`] plus
    /// [`crate::workspace_manager::Slot`] for the worktree namespaces — so this
    /// set moves when the layout moves. `run` adds one more per invocation: the
    /// workspace's **resolved** common Git directory, which is where a linked
    /// worktree's refs really are rather than where an assumed `<repo>/.git`
    /// would be.
    ///
    /// ## The sibling-worktree namespace (`PR6-CORRECTNESS-013`)
    ///
    /// DESIGN.md:400 is "A container receives only **its role's one worktree**
    /// mount; it never receives … **sibling worktrees**". Until repair R1 this
    /// helper named no worktree path at all and every fixture added its own
    /// siblings by hand, so a Gate handed the run's **execution root** as its
    /// workspace was accepted and received every task and merge worktree in one
    /// mount.
    ///
    /// The check below is "a mount is, or is an ancestor of, a withheld path",
    /// so withholding the execution root refuses a mount of it or of anything
    /// above it. That leaves one directory level between the root and a
    /// worktree — `<root>/tasks`, `<root>/merge`, `<root>/snapshots` — which is
    /// *inside* the root and still contains two worktrees, so each namespace
    /// directory is withheld as well. They are derived from `Slot::relative()`
    /// rather than written out: `Slot` is the type that decides where a
    /// worktree lives, and a hand-written list is a list that keeps passing
    /// after the layout has moved.
    ///
    /// What is deliberately **not** refused is a mount of one worktree that
    /// happens to be another role's. The runner mounts the one workspace the
    /// request names and cannot tell "mine" from "a sibling"; which worktree a
    /// role gets is the engine's decision, and DESIGN.md:400's clause is about
    /// receiving *more than one*.
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

    /// Withhold one more path under `category`.
    #[must_use]
    pub fn withholding(mut self, category: Withheld, path: impl Into<PathBuf>) -> Self {
        self.entries.push((category, path.into()));
        self
    }

    /// Every withheld path, with its category.
    #[must_use]
    pub fn entries(&self) -> &[(Withheld, PathBuf)] {
        &self.entries
    }

    /// Which of `mounts` would hand a withheld path to the container.
    ///
    /// A mount **is** a withheld path, or is an **ancestor** of one. The
    /// ancestor half is the whole check: a container that mounts the repository
    /// root has mounted the public log, and a container that mounts `/` has
    /// mounted everything. A membership test — "is the public log in the mount
    /// list" — passes on both.
    #[must_use]
    pub fn violations(&self, mounts: &[Mount]) -> Vec<String> {
        let mut found = Vec::new();
        for mount in mounts {
            // A named volume has no host path, so it can carry none of these.
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

/// `<execution root>/{tasks, merge, snapshots}` — the directories one level
/// above a worktree, each of which holds several.
///
/// Derived from [`crate::workspace_manager::Slot::relative`], which is the
/// function that decides where a worktree lives, by taking the first component
/// of one representative path per variant. Deduplicated and sorted, so two
/// variants sharing a namespace would collapse rather than double.
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

// ---------------------------------------------------------------------------
// The recorded policy, read for what this runner needs
// ---------------------------------------------------------------------------

/// The recorded immutable image id.
///
/// INV-23: "every container of every epoch is created from **the recorded image
/// id** … so a moved reference cannot change what executes". The reference is
/// deliberately not read here and is not carried into [`CreateSpec`], which has
/// no field for one.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] when the policy is not a container policy or
/// records no image.
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

/// The recorded per-agent credential volume names, or an empty map.
#[must_use]
pub fn recorded_volumes(policy: &RunnerPolicy) -> BTreeMap<String, String> {
    policy.credential_volumes.clone().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The runner
// ---------------------------------------------------------------------------

/// What one request becomes before anything is created.
///
/// Returned by [`ContainerRunner::plan`] so a test can inspect the mounts, the
/// environment and the create spec **without** a runtime — which is what makes
/// the mount and environment obligations assertable on a machine with no
/// container runtime at all, including the Windows guest.
#[derive(Debug, Clone)]
pub struct InvocationPlan {
    /// The launch sequence's own plan: name, intent, create spec, view request.
    pub launch: LaunchPlan,
    /// The Git layout the view projects, when the workspace is a worktree.
    pub git: Option<view::GitLayout>,
}

impl InvocationPlan {
    /// The mounts this container receives.
    #[must_use]
    pub fn mounts(&self) -> &[Mount] {
        &self.launch.spec.mounts
    }

    /// The environment this container receives.
    #[must_use]
    pub fn env(&self) -> &[(String, String)] {
        &self.launch.spec.env
    }
}

/// How far a launch got before it failed, and therefore what has to be
/// released.
///
/// The intent is not a field: every exit of [`ContainerRunner::launch`] that
/// can fail is *after* the intent is written, and `remove_intent` is
/// idempotent, so releasing it is unconditional. A boolean for it would be a
/// field with one value.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reached {
    /// The R19 view directory, when it was materialised.
    view: Option<PathBuf>,
    /// Whether `docker create` returned a container.
    container: bool,
}

impl Reached {
    /// The intent is written and nothing else exists.
    const INTENT_ONLY: Self = Self {
        view: None,
        container: false,
    };
}

/// The `Container` / `container-v1` [`Runner`].
///
/// Holds the **recorded** `RunnerPolicy` rather than resolving one: resolution
/// by read-only inspection is a separate obligation (INV-23, "resolved once by
/// read-only inspection before the worktree lock"), and a runner that resolved
/// its own policy could not be rebuilt from a record — which is what every
/// later incarnation does.
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
    /// Whether [`ContainerRunner::with_view`] replaced the default projection.
    ///
    /// The default view has to be rebuilt whenever the layout or the observer
    /// moves — its alternate names the object store's in-container target and
    /// its trace is the observer's — and a builder whose result depended on the
    /// order its setters were called in is a builder that is wrong half the
    /// time. So the default is rebuilt by every setter and an explicit one
    /// never is.
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
    /// A runner for `identity`'s run in `repo_root`, executing in `policy`'s
    /// recorded image and composing over `environment`.
    ///
    /// **`repo_root` and `environment` are parameters and not builder calls,
    /// and that is the repair for `PR6-CORRECTNESS-011` / `PR6-ENUM-002`.** The
    /// confinement is computed here, from [`Confinement::of_run`], so there is
    /// no construction that yields a runner withholding nothing — the previous
    /// default was `Confinement::none()` with the real set added by an
    /// *optional* `with_confinement`, and a caller who forgot it got a runner
    /// that would mount the run's own public log for a Gate. The builder is
    /// gone; a caller who wants to withhold *more* uses
    /// [`Self::also_withholding`], which can only add.
    ///
    /// `environment` is mandatory for the same reason at the other boundary:
    /// the default was `ContainerEnvironment::inherited()`, an empty base, so
    /// the runner supplied no `PATH` at all and the image's own — possibly
    /// carrying a working-directory-relative component — decided which binary a
    /// bare program name resolved to (`PR6-CORRECTNESS-006`). DESIGN.md:260
    /// says the runner "supplies role-scoped `HOME`, `PATH`, and credential
    /// locations"; a runner that read no base could supply none of them.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when `policy` is not a usable container policy
    /// — see [`recorded_image_id`].
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

    /// Use an explicit boundary layout, and point the Git view's alternate at
    /// its object mount.
    #[must_use]
    pub fn with_layout(mut self, layout: BoundaryLayout) -> Self {
        self.layout = layout;
        self.rebuild_view();
        self
    }

    /// Withhold one **more** path from every container this runner starts.
    ///
    /// Monotone by construction: it appends to the set
    /// [`Confinement::of_run`] derived and there is no setter that replaces it,
    /// so no call sequence can leave a runner withholding less than its run
    /// does.
    #[must_use]
    pub fn also_withholding(mut self, category: Withheld, path: impl Into<PathBuf>) -> Self {
        self.confinement = self.confinement.withholding(category, path);
        self
    }

    /// Observe (and, for the fault subset, inject at) every container site this
    /// runner reaches.
    #[must_use]
    pub fn with_hooks(mut self, hooks: Box<dyn ContainerHooks + Send>) -> Self {
        self.hooks = Mutex::new(hooks);
        self.rebuild_view();
        self
    }

    /// Use an explicit Git view implementation.
    #[must_use]
    pub fn with_view(mut self, view: Box<dyn GitView>) -> Self {
        self.view = view;
        self.view_is_explicit = true;
        self
    }

    /// Put the default projection back in step with the layout and the
    /// observer. A no-op once [`Self::with_view`] has replaced it.
    fn rebuild_view(&mut self) {
        if self.view_is_explicit {
            return;
        }
        self.view = Box::new(
            RoleGitView::new(self.trace())
                .for_reader(self.layout.git_view(), self.layout.git_objects()),
        );
    }

    /// How often the supervisor asks whether the container has finished.
    /// `Duration::ZERO` is what the suite sets: no sleeps.
    #[must_use]
    pub const fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    /// Bound the captured output at `bytes`.
    #[must_use]
    pub const fn with_output_limit(mut self, bytes: usize) -> Self {
        self.output_limit = bytes;
        self
    }

    /// The record this runner executes under.
    #[must_use]
    pub const fn policy(&self) -> &RunnerPolicy {
        &self.policy
    }

    /// `runner_policy_sha256` of [`Self::policy`] — every container intent
    /// carries it.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.digest
    }

    /// The boundary layout.
    #[must_use]
    pub const fn layout(&self) -> &BoundaryLayout {
        &self.layout
    }

    /// The environment contract this runner composes under.
    #[must_use]
    pub const fn environment(&self) -> &ContainerEnvironment {
        &self.environment
    }

    /// What this run withholds from every container.
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

    /// Everything one request becomes, without performing any effect.
    ///
    /// **This is the composition site**, and there is one: `Runner::run` calls
    /// it and so does every test that inspects a mount set, so a mount or an
    /// environment key that pre-flight sees and the spending invocation does
    /// not is not expressible.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when the overlay names a reserved key, when the
    /// container name cannot be built from the request's identity, or when the
    /// mount plan would hand the container a withheld path.
    pub fn plan(&self, request: &RunnerRequest) -> Result<InvocationPlan, UpstrokeError> {
        let name = ContainerName::new(
            &self.identity.repo_key,
            &self.identity.run_id,
            &self.identity.incarnation,
            &request.invocation,
        )?;
        // `ContainerIntent::new` encodes the run directory (`PR6-RECOV-001`).
        // The rendering this replaced was `to_string_lossy().replace('\\',
        // "/")`, which on Unix mapped `<repo>\a/runs/X` — a real directory,
        // since a backslash is an ordinary filename byte there — onto
        // `<repo>/a/runs/X`, a *different* real directory. A foreign census
        // then probed the wrong `run.lock`, found none, and killed a live run's
        // container. `intent::path_label` carries the argument.
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
            // The worktree's *resolved* common directory, which is where a
            // linked worktree's refs really are — rather than an assumed
            // `<repo>/.git`.
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
                    // INV-23: the recorded **id**, never the reference.
                    image_id: self.image_id.clone(),
                    labels: intent.labels(&self.identity.private_root),
                    mounts,
                    env,
                    command,
                    // Always a value, never the image's own choice. A role
                    // with a worktree runs in it; a probe, which has none,
                    // runs in the ephemeral scratch mount — a directory that
                    // exists in every image because this runner declares it,
                    // that is writable under a read-only root, and that
                    // carries nothing. `PR6-CORRECTNESS-006`: leaving this
                    // `None` for probes handed the working directory to the
                    // image's `WORKDIR`, so what a probe certified depended on
                    // a value the runner had not read.
                    workdir: Some(if receives_a_worktree(&request.role) {
                        self.layout.workspace().to_owned()
                    } else {
                        self.layout.scratch().to_owned()
                    }),
                    // `expected_failures_refusals[5]`, for every role. A
                    // reviewer's `:ro` worktree is not the whole of "read-only"
                    // if the container layer around it is writable, and a gate
                    // is "repository-controlled code which no agent permission
                    // surface can ever bound" (DESIGN.md:610).
                    read_only_root: true,
                },
                view: GitViewRequest {
                    path: view_path.clone(),
                    // R19 is "per container invocation (**incl. shell and agent
                    // probes**)", so a probe gets its view directory too — and
                    // it has nothing to project. `GitViewRequest` has no
                    // "project nothing" state, so the request names a directory
                    // that is not a worktree, which is what the projection
                    // already treats as "no repository here". Recorded as a
                    // seam note rather than worked around silently.
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

    /// The mounts this request's role receives, and no others.
    ///
    /// DESIGN.md:400: "A container receives **only its role's one worktree
    /// mount**". Four kinds, and each is here because a live passage puts it
    /// here:
    ///
    /// 1. the role's **one** worktree, `:ro` for a reviewer — DESIGN.md:610's
    ///    "a `:ro` mount makes the reviewer's read-only *mechanically* perfect
    ///    instead of flag-deep";
    /// 2. the disposable Git view, over the worktree's own `.git` —
    ///    DESIGN.md:612;
    /// 3. the object store the view borrows, **read-only** — the same sentence;
    /// 4. this agent's credential volume, for the roles that execute an agent
    ///    CLI — DESIGN.md:612's "which per-agent credential volume to mount",
    ///    and R20's "persistent volumes, not ephemeral copies", so it is
    ///    writable: "some CLIs rotate refresh tokens on use, and a discarded
    ///    rotation forces re-login".
    ///
    /// (2) and (3) are absent when the workspace is not a worktree — a probe's
    /// scratch directory — and (4) is absent for a role
    /// [`supplies_credential_location`] refuses. Nothing else is ever added,
    /// which is the positive half of the confinement claim; the negative half
    /// is [`Confinement::violations`].
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
            // The overlay at `<workspace>/.git`. A bind mount's source and its
            // target must be the same kind — measured against `docker` 29.7.2,
            // which fails `runc create` with "Are you trying to mount a
            // directory onto a file" — so a linked worktree (a `.git` file)
            // receives the one-line pointer file and a main worktree (a `.git`
            // directory) receives the view directory itself. Either way what a
            // tool finds at `<workspace>/.git` is the disposable view.
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
        // (5) the ephemeral scratch surface, for **every** role. With
        // `CreateSpec::read_only_root` the container's own layer is closed, so
        // without this a `sh -c` gate could not write a temporary file and
        // `git` could not write its own. It carries no host source, so it is
        // the one writable surface that is neither the role's nor the
        // coordinator's — which is what keeps "gate write outside mount fails"
        // a claim about a mount list rather than about a hole.
        mounts.push(Mount::Tmpfs {
            target: self.layout.scratch().to_owned(),
        });
        mounts
    }

    /// The credential volume this request's role would be given, if any.
    ///
    /// Exposed so the mount rule and [`supplies_credential_location`] can be
    /// asserted to be **the same predicate** rather than two rules that agree
    /// today.
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

    /// The four sites `side_effect_vs_event_ordering` puts before the
    /// invocation, in the order it states and in the order a container runtime
    /// can execute.
    ///
    /// > intent synced before docker create; container created from the
    /// > recorded id and verified before start; view mounted before start
    ///
    /// The Git view is materialised **before** `Container.Create` because it is
    /// a bind-mount source of that call and a bind source must exist when the
    /// container is created — see the module docs. Every clause the contract
    /// states still holds: the intent is synced before the create, the reported
    /// image id is verified before the start, and the view is mounted before
    /// the start.
    ///
    /// **This is also what makes "container start without an intent is
    /// impossible by construction"** (`expected_failures_refusals[6]`) true of
    /// the shape a caller uses: the only sequence in this module that reaches
    /// `Container.Start` begins by writing the intent.
    ///
    /// ## Every way out of this function releases what it reached
    ///
    /// `PR6-CORRECTNESS-003` / `PR6-ENUM-003`. There are four exits and until
    /// repair R1 only one of them cleaned up:
    ///
    /// | fails at | reached | released before |
    /// |---|---|---|
    /// | `MountGitView` | intent | — nothing did |
    /// | `Create` | intent, view | — nothing did |
    /// | reported id mismatch | intent, view, container | fail-**fast**: a failing `Stop` skipped the rm, the view and the intent *and masked the integrity refusal* |
    /// | `Start` | intent, view, container | — nothing did |
    ///
    /// R26 is "released on complete (stop/rm, view removed, intent removed),
    /// **cancel**, or shutdown" and R19 is "pruned on complete or **cancel**".
    /// A `?` that returns without a [`Launched`] value returns without anything
    /// for `Runner::run`'s own release to act on, so each of those exits left a
    /// container, a view and an intent for the census to find. Every exit now
    /// goes through [`Self::cancel`], which attempts **every** step even after
    /// one fails and answers what it could not release; the error returned is
    /// always the *original* one with that residue appended, because "docker
    /// stop said no" is never the thing to report instead of "the runtime
    /// executed a substituted image".
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when the reported image id differs from the
    /// record, or whatever a step returns.
    fn launch(
        &self,
        hooks: &mut dyn ContainerHooks,
        plan: &LaunchPlan,
    ) -> Result<Launched, UpstrokeError> {
        // The **fifth** exit (`PR6-ACCT-003`). R1's table began at
        // `MountGitView` because a `write_intent` that fails has written
        // nothing — which is true only of a failure at the `Before` phase. The
        // funnel runs its primitive and *then* consults the `After` phase, and
        // `IntentWritten::certify` reads the published record back, so a
        // failure here is a durable R26 record with no container and no view:
        // residue a census has to reclaim, from a launch that never launched.
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
                // **The view path, not `INTENT_ONLY`** (`PR6-ACCT-003`). The
                // funnel runs its primitive and then consults the `After`
                // phase, so a `MountGitView` that fails may have materialised
                // the directory first — and the `Err` arm carries no path to
                // say so. The request's own `path` is where it would be, and
                // `GitView::discard` is idempotent and tolerant of
                // already-gone, so naming it costs a no-op in the `Before` cell
                // and is the difference between a pruned R19 directory and an
                // orphan in the `After` one. Measured: with `INTENT_ONLY` here,
                // arming `MountGitView`'s `After` phase left a view behind and
                // removed the intent that was the only handle on it.
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
                // `container: true`, for the reason the view path is named
                // above (`PR6-ACCT-003`): a `Container.Create` that fails at
                // the `After` phase has already created the container, and the
                // real equivalent is a `docker create` that succeeds and whose
                // following inspect fails — `DockerCli::create` reads the
                // reported image id back, and that read can fail on a container
                // that exists. `stop` and `remove` are both tolerant of
                // already-gone (`settle_stop`, `settle_remove`), so this is a
                // pair of no-ops when nothing was created. Measured: with
                // `container: false` here, arming `Create`'s `After` phase left
                // the container behind.
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
            // R2's error type, R1's cleanup. The two lanes repaired this one
            // path for different findings: R2 made a mid-run mismatch
            // *distinguishable* from a pre-flight one (they were the same
            // generic Refused), and R1 made the cleanup attempt **every** step
            // instead of stopping at the first failure and masking the
            // integrity error underneath it. Both are needed, so the
            // distinguishable error is the `cause` R1's `cancelled` carries.
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

    /// Release what a failed launch reached and answer `cause` with whatever
    /// could not be released appended.
    ///
    /// The answer is never the cleanup's own failure: an operator holding "the
    /// container could not be stopped" instead of "the runtime executed a
    /// substituted image" has been handed the symptom and not the diagnosis.
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

    /// Stop, remove, unmount and remove-intent for whatever `reached` says
    /// exists, **attempting every step even after one fails**.
    ///
    /// `PR6-LANEF-006`'s shape, applied to the runner's own launch rather than
    /// to `super::launch`: with `?` in place a failing `Container.Stop` left
    /// the container, the view and the intent behind — three residues from one
    /// failure. `docker rm --force` removes a running container, so `Remove`
    /// after a failed `Stop` is not a wasted call.
    ///
    /// **The body is [`super::cancel_reached`] and nothing else**
    /// (`PR6-ACCT-004`/`PR6-ACCT-005`): this was a second copy of the same four
    /// steps, and the copy in `super` grew the R19 recovery-anchor rule while
    /// this one did not. Two implementations of one cleanup rule is the shape
    /// `PR6E-005` measured on the view-path derivation, where the two halves
    /// were each self-consistent and nothing crossed them.
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

    /// "stop/rm, view removal, intent removal **after completion**".
    ///
    /// [`super::release`], which is the one place those four sites are
    /// performed in that order. It was a second copy until repair round R3b,
    /// and the copy was the fail-**fast** one: `PR6-ACCT-004` measured that a
    /// `Container.Stop` failure on a *completed* invocation skipped the still
    /// viable `rm`, the view prune and the intent removal, while the exhaustive
    /// implementation the cleanup-fault grid tests lived on the other path and
    /// never reached `Runner::run`.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] naming every step that could not be completed.
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

    /// Wait for the container, bounded by the request's own timeout.
    ///
    /// "timeout or shutdown stops and removes the container"
    /// (`slice_contract.cancellation`). The stop and the removal are the
    /// caller's [`super::release`]; this decides *which* disposition.
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

// ---------------------------------------------------------------------------
// INV-23's two outcomes, which differ by phase
// ---------------------------------------------------------------------------

/// What a created container's reported image id differing from the record
/// means, which depends on **when** it is observed.
///
/// `expected_failures_refusals[3]`, in full: "a created container whose
/// reported image id differs from the record is **refused before start
/// (pre-flight/rebuild)** or **settled as a `RunnerSpawnFailure` outage
/// (mid-run)**". Two outcomes, and the contract distinguishes them: a refusal
/// stops the write command before it has spent anything, and an outage defers
/// an already-running task's attempt without burning it
/// (`UnavailableOutcome::Deferred` — "an outage never fails a task on its
/// own").
///
/// The shipped code returned one [`UpstrokeError::Refused`] for both, so a caller
/// could not tell the two phases apart and the mid-run half of the clause was
/// unreachable — `PR6-CORRECTNESS-001`. The *settlement event* is PR7's
/// (`invariants_introduced`: the container transition is "test-only until PR7
/// wires `TopologyRun`"), and `src/topology/**` is frozen; what this slice owes
/// is that the two phases arrive at a caller as **different things**, so PR7
/// has something to map. This is that thing.
///
/// **The phase is read from the invocation and nothing else.** A
/// [`InvocationId::Probe`] is a `RunnerPreflight` container — the pre-flight
/// and rebuild path, which by construction runs before any work — and an
/// [`InvocationId::Attempt`] or [`InvocationId::Sequence`] is a worker, gate or
/// reviewer invocation inside a run that is already spending. Deriving it from
/// anything else (a flag on the runner, a phase the caller passes) would let
/// the two disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImageIdMismatch {
    /// Pre-flight or rebuild: **refuse before start**, before any spend.
    RefusedBeforeStart,
    /// Mid-run: the invocation could not be spawned on the recorded boundary,
    /// which the run settles as a `RunnerSpawnFailure` outage.
    SpawnFailureOutage,
}

impl ImageIdMismatch {
    /// Both outcomes, so a grid over phases is a grid over all of them.
    pub const ALL: &'static [Self] = &[Self::RefusedBeforeStart, Self::SpawnFailureOutage];

    /// Which outcome this invocation's mismatch has.
    #[must_use]
    pub const fn of(invocation: &InvocationId) -> Self {
        match invocation {
            InvocationId::Probe { .. } => Self::RefusedBeforeStart,
            InvocationId::Attempt { .. } | InvocationId::Sequence { .. } => {
                Self::SpawnFailureOutage
            }
        }
    }

    /// The error a caller settles from.
    ///
    /// **The variant is the classification**, not a substring of the message: a
    /// caller that had to grep prose to tell a refusal from an outage would be
    /// reading an oracle nobody can keep stable. [`UpstrokeError::Agent`] is this
    /// engine's existing channel for "the runner could not produce a usable
    /// process" — `agent::proc` returns it for a failed spawn and
    /// `gates::Scripted::SpawnFailure` returns it — which is the shape
    /// `InfrastructureKind::RunnerSpawnFailure` settles.
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

    /// As a report writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RefusedBeforeStart => "refused-before-start",
            Self::SpawnFailureOutage => "runner-spawn-failure-outage",
        }
    }
}

/// `<R>/views/<container-name>`.
///
/// Under the run's recorded private root, beside `<R>/containers`, so a census
/// that reclaims an orphan container has the view path without a live
/// [`Launched`] — which is exactly how [`super::reclaim`] takes it.
///
/// # It delegates, and that is the whole point (`PR6E-005` / `PR6-LANEC-003`)
///
/// This module *mounts* the view; [`super::census`] *finds* it after a crash,
/// and the two halves were written in different lanes. **The six intent fields
/// the packet fixes carry no view path**, so the consumer has to *derive* it
/// from the private root and the container name — which means the two
/// derivations have to be the same derivation, and they were two copies of one
/// `join` with nothing asserting they agree.
///
/// Measured independently, twice: lane E changed `census::VIEWS_DIR` to
/// `"views-mutated"` and **all 1324 tests passed**; the lane-C review changed
/// only this side to `<R>/views-v2/<name>` and the entire suite passed. Each
/// half is self-consistent — lane C's fixtures plant orphan views through
/// `census::view_path` and lane A's assert this literal — and **no test crosses
/// them**. A real divergence leaves every orphan view unreclaimed after a
/// coordinator death: `resource_accounting` R19's `NoRunFinished` is "pruned at
/// the next write-command start after the owning container is observed
/// terminated", and ST-16's closing clause is "ledgers R19/R26 balance".
///
/// `census::view_path` is now the one definition and this is a delegation, so
/// the divergence is **unrepresentable** rather than merely untested — the shape
/// `PR4-CONF-003` established, where deleting a guarantee is a compile error
/// instead of a silent regression.
/// `effects::tests::the_view_directory_has_one_definition_in_the_tree` guards
/// against a second one being written.
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

        // WriteIntent -> MountGitView -> Create (+ verify the reported image
        // id) -> Start, in that order and in one place.
        let launched: Launched = self.launch(&mut **hooks, &plan.launch)?;

        let outcome = self.finish(&launched, started, deadline);
        // Release whatever the invocation reached, whether or not it succeeded:
        // R26 is "released on complete (stop/rm, view removed, intent removed),
        // **cancel**, or shutdown", and R19's "pruned on complete or cancel".
        // So the release runs on both paths and its own failure is reported
        // only when there is no earlier one to report — a release that could
        // not finish leaves residue the census reclaims, and hiding the reason
        // the invocation failed behind it would trade a diagnosis for a
        // symptom.
        let released = self.release(&mut **hooks, &self.identity.private_root, &launched);
        let output = outcome?;
        released?;
        Ok(output)
    }
}

impl ContainerRunner {
    /// Supervise, then collect. Split out so `run` can release on either path.
    fn finish(
        &self,
        launched: &Launched,
        started: Instant,
        deadline: Instant,
    ) -> Result<ProcessOutput, UpstrokeError> {
        let timed_out = self.supervise(&launched.name, deadline)?;
        // Collected **before** the release: `docker logs` answers for a running
        // container and not for a removed one, so a timed-out invocation still
        // reports what it printed.
        let execution = self
            .runtime
            .collect(launched.name.as_str())
            .map_err(refused_by_runtime)?;
        let (stdout, stdout_limited) = bounded(&execution.stdout, self.output_limit);
        let (stderr, stderr_limited) = bounded(&execution.stderr, self.output_limit);
        Ok(ProcessOutput {
            // A container the supervisor stopped did not exit on its own,
            // whatever status the runtime reports afterwards — the same
            // disposition `agent::proc` gives a killed tree.
            code: if timed_out { None } else { execution.exit_code },
            stdout,
            stderr,
            duration: started.elapsed(),
            timed_out,
            output_limited: stdout_limited || stderr_limited,
        })
    }
}

/// A runtime failure, as the engine's error type.
fn refused_by_runtime(error: RuntimeError) -> UpstrokeError {
    UpstrokeError::Refused {
        message: error.to_string(),
    }
}

/// `bytes` as text, truncated at `limit`, and whether it was.
fn bounded(bytes: &[u8], limit: usize) -> (String, bool) {
    if bytes.len() <= limit {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let mut end = limit;
    // Do not split a UTF-8 sequence: back up to a character boundary.
    while end > 0 && (bytes[end] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    (String::from_utf8_lossy(&bytes[..end]).into_owned(), true)
}

// -- test-only declarations ----------------------------------------------
// At the BOTTOM: `effects::production_region`, which
// `effects::externally_reachable_fns` and the three censuses further down this
// file still use, cuts a source at its first `#[cfg(test)]`
// (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).
//
// **The allow below is written ABOVE `#[cfg(test)]`. That order used to be
// load-bearing and is now a convention.** The reader that made it load-bearing
// was `runner::tests::production_region`, which was line-based: it excluded a
// test module by matching a line that is exactly `#[cfg(test)]` followed by a
// line starting `mod `, so an attribute between the two made this whole test
// region read as PRODUCTION and both
// `every_production_runner_request_is_built_by_its_roles_builder` and
// `every_production_command_spec_payload_is_classified` failed with these
// fixtures counted as production call sites. Measured in repair round F1 and
// filed as `PR6F1-RUNNER-PRODUCTION-REGION-BREAKS-ON-AN-ATTRIBUTE`.
//
// **That reader is deleted.** `effects::production_code`, which those censuses
// share now, finds the item's extent by delimiter matching, so it removes this
// module with the attribute in either position — measured both ways on this
// file. The order is kept because it is the one `effects::is_module_level`
// reads without relying on its skip-further-attributes step, and because with
// the allow written below `#[cfg(test)]` it becomes part of the configured
// item and is blanked along with it.
//
// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/runner/container.rs`. It covers this file's TEST REGION
// ONLY — the production region above keeps the file-level `#![deny(...)]`, so a
// lane's production code here still cannot reach a container primitive
// (`PR6-LANEF-004`). `decisions.effect_site_inventory.mechanism` (2).
#[allow(clippy::disallowed_methods)]
#[cfg(test)]
mod tests;
