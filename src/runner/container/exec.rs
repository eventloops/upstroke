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
use crate::error::TactusError;
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
    /// `<repo>/.tactus/runs/<run-id>` — `events.jsonl`, the frozen plan,
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
/// [`TactusError::Refused`] when the policy is not a container policy or
/// records no image.
pub fn recorded_image_id(policy: &RunnerPolicy) -> Result<&str, TactusError> {
    if policy.kind != RunnerKind::Container || policy.policy != RunnerContract::ContainerV1 {
        return Err(TactusError::Refused {
            message: format!(
                "the container runner was given a `{:?}`/`{:?}` RunnerPolicy; \
                 `container-v1` is the mount, environment, Git-view and supervision \
                 contract this runner implements (INV-23)",
                policy.kind, policy.policy
            ),
        });
    }
    let Some(image) = &policy.image else {
        return Err(TactusError::Refused {
            message: "the recorded RunnerPolicy is a container policy with no image; INV-23 \
                      records `image: {reference, id, digest}` and every container is created \
                      from the recorded id"
                .to_owned(),
        });
    };
    if image.id.trim().is_empty() {
        return Err(TactusError::Refused {
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
    /// [`TactusError::Refused`] when `policy` is not a usable container policy
    /// — see [`recorded_image_id`].
    pub fn new(
        policy: RunnerPolicy,
        identity: RunIdentity,
        repo_root: &Path,
        environment: ContainerEnvironment,
        runtime: Box<dyn ContainerRuntime>,
    ) -> Result<Self, TactusError> {
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
    /// [`TactusError::Refused`] when the overlay names a reserved key, when the
    /// container name cannot be built from the request's identity, or when the
    /// mount plan would hand the container a withheld path.
    pub fn plan(&self, request: &RunnerRequest) -> Result<InvocationPlan, TactusError> {
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
            return Err(TactusError::Refused {
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
    /// [`TactusError::Refused`] when the reported image id differs from the
    /// record, or whatever a step returns.
    fn launch(
        &self,
        hooks: &mut dyn ContainerHooks,
        plan: &LaunchPlan,
    ) -> Result<Launched, TactusError> {
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
        cause: TactusError,
        reached: Reached,
    ) -> TactusError {
        let residue = self.cancel(hooks, &plan.private_root, &plan.name, &reached);
        if residue.is_empty() {
            return cause;
        }
        TactusError::Refused {
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
    /// [`TactusError::Refused`] naming every step that could not be completed.
    fn release(
        &self,
        hooks: &mut dyn ContainerHooks,
        private_root: &Path,
        launched: &Launched,
    ) -> Result<(), TactusError> {
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
    fn supervise(&self, name: &ContainerName, deadline: Instant) -> Result<bool, TactusError> {
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
/// The shipped code returned one [`TactusError::Refused`] for both, so a caller
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
    /// reading an oracle nobody can keep stable. [`TactusError::Agent`] is this
    /// engine's existing channel for "the runner could not produce a usable
    /// process" — `agent::proc` returns it for a failed spawn and
    /// `gates::Scripted::SpawnFailure` returns it — which is the shape
    /// `InfrastructureKind::RunnerSpawnFailure` settles.
    #[must_use]
    pub fn error(self, name: &ContainerName, reported: &str, recorded: &str) -> TactusError {
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
            Self::RefusedBeforeStart => TactusError::Refused { message },
            Self::SpawnFailureOutage => TactusError::Agent { message },
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
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, TactusError> {
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
    ) -> Result<ProcessOutput, TactusError> {
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
fn refused_by_runtime(error: RuntimeError) -> TactusError {
    TactusError::Refused {
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
// At the BOTTOM: `effects::production_region` cuts a source at its first
// `#[cfg(test)]` (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).
//
// **The allow below is written ABOVE `#[cfg(test)]`, and that order is
// load-bearing.** `runner::tests::production_region` is line-based: it excludes
// a test module by matching a line that is exactly `#[cfg(test)]` followed by a
// line starting `mod `, so an attribute between the two makes this whole test
// region read as PRODUCTION and both
// `every_production_runner_request_is_built_by_its_roles_builder` and
// `every_production_command_spec_payload_is_classified` fail with these
// fixtures counted as production call sites. Measured in repair round F1 and
// filed as `PR6F1-RUNNER-PRODUCTION-REGION-BREAKS-ON-AN-ATTRIBUTE`;
// `effects::is_module_level` skips further attributes before requiring `mod`,
// so this order satisfies both readers.
//
// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/runner/container.rs`. It covers this file's TEST REGION
// ONLY — the production region above keeps the file-level `#![deny(...)]`, so a
// lane's production code here still cannot reach a container primitive
// (`PR6-LANEF-004`). `decisions.effect_site_inventory.mechanism` (2).
#[allow(clippy::disallowed_methods)]
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use super::*;
    use crate::gates::ShellKind;
    use crate::rundir::RunPaths;
    use crate::runner::container::intent::LABEL_PRIVATE_ROOT;
    use crate::runner::container::runtime::{
        ContainerExecution, ContainerTrace, CreatedContainer, DiscoveredContainer, ImageInspection,
        Liveness, RuntimeOp, StopMode,
    };
    use crate::runner::container::view::fixtures as repo;
    use crate::runner::container::{
        DOCKER_GATED_TESTS, FakeRuntime, GitView, RecordingHooks, docker_gate, list_intents,
    };
    use crate::runner::host::{self, HostEnvironment};
    use crate::runner::invocation::AttemptRole;
    use crate::runner::{
        AgentId, InvocationId, ProbeTarget, RunnerRequest, gate_request, review_request,
        worker_request,
    };
    use crate::topology::events::{AttemptNumber, GenerationId, ImageIdentity};
    use crate::topology::registry::TaskKey;

    const RUN_ID: &str = "01KZRN48A4ZK3AEDST3RJ8HMA4";
    const REPO_KEY: &str = "0123456789abcdef";
    const INCARNATION_1: &str = "01KZTAAAAAAAAAAAAAAAAAAAAA";
    const INCARNATION_2: &str = "01KZTBBBBBBBBBBBBBBBBBBBBB";
    const IMAGE_ID: &str =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const OTHER_IMAGE_ID: &str =
        "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    const IMAGE_REFERENCE: &str = "ghcr.io/example/tactus-runner:v1";
    const MANIFEST_DIGEST: &str =
        "sha256:3333333333333333333333333333333333333333333333333333333333333333";
    const VOLUMES: &[(&str, &str)] = &[
        ("claude-code", "tactus-creds-claude"),
        ("copilot", "tactus-creds-copilot"),
        ("codex", "tactus-creds-codex"),
    ];
    /// Written into the run's public log, so a container that could read it
    /// would be caught by content rather than by the absence of a file.
    const EVENT_LOG_MARKER: &str = "COORDINATOR-EVENT-LOG-a5f2";

    /// The `PATH` the fake fixtures' image environment carries.
    ///
    /// Absolute-only, which is what `ContainerEnvironment::certify_path` now
    /// requires: a runner whose composed environment names no `PATH`, or names
    /// one with a working-directory-relative component, refuses every
    /// invocation (`PR6-CORRECTNESS-006`). The value is the one every image
    /// this suite discovers actually carries, read off `docker image inspect`
    /// for `tactus-test/git:v1`, `alpine:3.20` and `busybox:latest`.
    const IMAGE_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

    /// A `PATH` whose second component is the working directory.
    ///
    /// `.` explicitly rather than an empty component, so the two shapes
    /// `cwd_dependent_path_components` classifies are exercised by different
    /// fixtures rather than by one.
    const CWD_RELATIVE_PATH: &str = "/usr/local/bin:.:/usr/bin";

    /// The image environment the fake fixtures compose over.
    ///
    /// Explicit rather than `ContainerEnvironment::inherited()`, which is now a
    /// base that refuses: DESIGN.md:260 has the runner supply `PATH`, and an
    /// empty base supplies nothing.
    fn image_environment() -> ContainerEnvironment {
        ContainerEnvironment::from_image(
            [
                ("PATH", IMAGE_PATH),
                ("HOME", "/root"),
                ("TACTUS_IMAGE_MARKER", IMAGE_MARKER_VALUE),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        )
    }

    fn container_policy() -> RunnerPolicy {
        RunnerPolicy {
            kind: RunnerKind::Container,
            policy: RunnerContract::ContainerV1,
            image: Some(ImageIdentity {
                reference: IMAGE_REFERENCE.to_owned(),
                id: IMAGE_ID.to_owned(),
                digest: Some(MANIFEST_DIGEST.to_owned()),
            }),
            credential_volumes: Some(
                VOLUMES
                    .iter()
                    .map(|(agent, volume)| ((*agent).to_owned(), (*volume).to_owned()))
                    .collect(),
            ),
        }
    }

    // -----------------------------------------------------------------------
    // A runtime that can finish, and that a test keeps a handle on
    // -----------------------------------------------------------------------

    /// The fake, wrapped so a test can hold it while the runner owns it, and so
    /// a container can be made to **finish**.
    ///
    /// `FakeRuntime::start` leaves a container `Running` and nothing in a
    /// synchronous `Runner::run` could move it afterwards, so the success path
    /// would be unreachable and only the timeout path would ever be measured. A
    /// decorator that exits the container at `start` — and, when asked, gives
    /// it an exit status and output — is what makes both paths constructible;
    /// the plain fake still drives the timeout.
    #[derive(Debug)]
    struct Scripted {
        fake: FakeRuntime,
        exit_on_start: bool,
        execution: Mutex<Option<ContainerExecution>>,
    }

    #[derive(Debug, Clone)]
    struct Runtime(Arc<Scripted>);

    impl Runtime {
        fn new(trace: ContainerTrace, exit_on_start: bool) -> Self {
            let fake = FakeRuntime::new(trace);
            fake.add_image(IMAGE_ID, Some(MANIFEST_DIGEST));
            fake.add_image(OTHER_IMAGE_ID, None);
            fake.tag(IMAGE_REFERENCE, IMAGE_ID);
            for (_, volume) in VOLUMES {
                fake.add_volume(volume);
            }
            Self(Arc::new(Scripted {
                fake,
                exit_on_start,
                execution: Mutex::new(None),
            }))
        }

        fn fake(&self) -> &FakeRuntime {
            &self.0.fake
        }

        /// What every container of this runtime reports when it finishes.
        fn scripts(&self, execution: ContainerExecution) -> Self {
            *self
                .0
                .execution
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(execution);
            self.clone()
        }
    }

    impl ContainerRuntime for Runtime {
        fn probe(&self) -> Result<(), RuntimeError> {
            self.0.fake.probe()
        }
        fn image_by_reference(
            &self,
            reference: &str,
        ) -> Result<Option<ImageInspection>, RuntimeError> {
            self.0.fake.image_by_reference(reference)
        }
        fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError> {
            self.0.fake.image_by_id(id)
        }
        fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
            self.0.fake.volume_present(name)
        }
        fn containers_with_label(
            &self,
            key: &str,
            value: &str,
        ) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
            self.0.fake.containers_with_label(key, value)
        }
        fn observe(&self, name: &str) -> Result<Liveness, RuntimeError> {
            self.0.fake.observe(name)
        }
        fn collect(&self, name: &str) -> Result<ContainerExecution, RuntimeError> {
            self.0.fake.collect(name)
        }
        fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {
            self.0.fake.create(spec)
        }
        fn start(&self, name: &str) -> Result<(), RuntimeError> {
            self.0.fake.start(name)?;
            if self.0.exit_on_start {
                self.0.fake.set_container_state(name, Liveness::Exited);
                if let Some(execution) = self
                    .0
                    .execution
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                {
                    self.0.fake.set_execution(name, execution);
                }
            }
            Ok(())
        }
        fn stop(&self, name: &str, mode: StopMode) -> Result<(), RuntimeError> {
            self.0.fake.stop(name, mode)
        }
        fn remove(&self, name: &str) -> Result<(), RuntimeError> {
            self.0.fake.remove(name)
        }
    }

    // -----------------------------------------------------------------------
    // A realistic run layout
    // -----------------------------------------------------------------------

    /// One run, laid out where the engine really puts things.
    ///
    /// Every path here comes from the type that owns it — [`RunPaths`] for the
    /// two halves of a run directory, `workspace_manager::execution_root_of`
    /// for the worktrees — rather than from string literals, so a layout change
    /// moves the fixture with it. A hand-built layout is a fixture that keeps
    /// passing after the thing it describes has moved.
    struct Fixture {
        root: PathBuf,
        repo: PathBuf,
        private_root: PathBuf,
        paths: RunPaths,
        identity: RunIdentity,
        task_a: PathBuf,
        task_b: PathBuf,
        merge: PathBuf,
        trace: ContainerTrace,
        runtime: Runtime,
    }

    impl Fixture {
        fn new(tag: &str, exit_on_start: bool) -> Self {
            let root = repo::scratch(tag);
            let repo_dir = root.join("repo");
            let (head, _) = repo::repository(&repo_dir);
            let private_root = root.join("private");
            let paths = RunPaths::with_private_root(&repo_dir, RUN_ID, &private_root);
            paths.create().expect("the run's two halves");
            std::fs::write(paths.events(), format!("{EVENT_LOG_MARKER}\n"))
                .expect("the public log");
            std::fs::write(
                paths.transcripts().join("k0-a1.md"),
                "PRIVATE-TRANSCRIPT-a5f2\n",
            )
            .expect("a private artifact");
            std::fs::create_dir_all(crate::runner::container::intent::containers_dir(
                &private_root,
            ))
            .expect("the container namespace");

            let execution_root =
                crate::workspace_manager::execution_root_of(&private_root, REPO_KEY, RUN_ID);
            let task_a = execution_root.join("tasks").join("kalpha-g0");
            let task_b = execution_root.join("tasks").join("kbeta-g0");
            let merge = execution_root.join("merge").join("s0");
            for at in [&task_a, &task_b, &merge] {
                repo::worktree(&repo_dir, at, &head);
            }
            std::fs::write(task_b.join("sibling.txt"), "SIBLING-WORKTREE-a5f2\n")
                .expect("a sibling file");

            let trace = ContainerTrace::recording();
            Self {
                identity: RunIdentity {
                    private_root: private_root.clone(),
                    run_id: RUN_ID.to_owned(),
                    run_dir: paths.public.clone(),
                    incarnation: INCARNATION_1.to_owned(),
                    repo_key: REPO_KEY.to_owned(),
                },
                runtime: Runtime::new(trace.clone(), exit_on_start),
                trace,
                root,
                repo: repo_dir,
                private_root,
                paths,
                task_a,
                task_b,
                merge,
            }
        }

        /// Everything this run withholds.
        ///
        /// **The two sibling worktrees are no longer added by hand.** They were
        /// until repair R1, and `PR6-CORRECTNESS-013` is what that cost:
        /// `Confinement::of_run` named no worktree path at all, so the helper
        /// production uses withheld nothing about worktrees and only the
        /// fixtures pretended otherwise. `of_run` now derives the execution
        /// root and its three namespaces, which is what makes this method a
        /// plain delegation — and what makes the sibling assertions in
        /// `the_mount_set_is_the_roles_own_and_reaches_nothing_of_the_coordinators`
        /// statements about the production helper rather than about the
        /// fixture.
        fn confinement(&self) -> Confinement {
            Confinement::of_run(&self.identity, &self.repo)
        }

        fn runner(&self) -> ContainerRunner {
            self.runner_with(self.identity.clone())
        }

        fn runner_with(&self, identity: RunIdentity) -> ContainerRunner {
            ContainerRunner::new(
                container_policy(),
                identity,
                &self.repo,
                image_environment(),
                Box::new(self.runtime.clone()),
            )
            .expect("a container policy")
            .with_hooks(Box::new(RecordingHooks::new(self.trace.clone())))
            .with_poll(Duration::ZERO)
        }

        /// The concrete host paths this run withholds, as a table a test can
        /// iterate — derived from the same accessors the layout is built from.
        fn withheld(&self) -> Vec<(Withheld, PathBuf)> {
            vec![
                (Withheld::PublicLog, self.paths.public.clone()),
                (Withheld::PublicLog, self.paths.events()),
                (Withheld::PrivateArtifacts, self.paths.private.clone()),
                (Withheld::PrivateArtifacts, self.paths.transcripts()),
                (
                    Withheld::PrivateArtifacts,
                    crate::runner::container::intent::containers_dir(&self.private_root),
                ),
                (Withheld::SiblingWorktree, self.task_b.clone()),
                (Withheld::SiblingWorktree, self.merge.clone()),
                (Withheld::AuthoritativeGit, self.repo.join(".git")),
            ]
        }
    }

    fn worker_id(ordinal: u32) -> InvocationId {
        InvocationId::attempt(
            TaskKey(0),
            GenerationId(0),
            AttemptNumber(1),
            AttemptRole::Worker,
            ordinal,
        )
    }

    fn gate_id(ordinal: u32) -> InvocationId {
        InvocationId::attempt(
            TaskKey(0),
            GenerationId(0),
            AttemptNumber(1),
            AttemptRole::Gate(ordinal),
            0,
        )
    }

    fn shell_probe_id() -> InvocationId {
        InvocationId::probe(ProbeTarget::Shell, 0).expect("the shell probe identity")
    }

    fn agent_probe_id(agent: &str) -> InvocationId {
        InvocationId::probe(ProbeTarget::Agent(AgentId::new(agent)), 0)
            .expect("an agent probe identity")
    }

    /// A request in every role, over one workspace, with the binding each role
    /// takes in production.
    fn requests(workspace: &Path) -> Vec<RunnerRequest> {
        let claude = AgentId::new("claude-code");
        let spec = ShellKind::Sh.spec("exit 0");
        vec![
            crate::agent::probe_request("claude-code", spec.clone(), 0, Duration::from_secs(10))
                .expect("an agent probe request"),
            host::shell_probe_request(ShellKind::Sh, workspace.to_path_buf(), shell_probe_id()),
            worker_request(
                spec.clone(),
                workspace.to_path_buf(),
                claude.clone(),
                Duration::from_secs(10),
                worker_id(0),
            ),
            gate_request(
                spec.clone(),
                workspace.to_path_buf(),
                Duration::from_secs(10),
                gate_id(0),
            ),
            review_request(
                spec,
                workspace.to_path_buf(),
                claude,
                Duration::from_secs(10),
                InvocationId::attempt(
                    TaskKey(0),
                    GenerationId(0),
                    AttemptNumber(1),
                    AttemptRole::ReviewPass(0),
                    0,
                ),
            ),
        ]
    }

    /// Requests whose **role and agent binding are varied independently**.
    ///
    /// `runner::gate_request` and `host::shell_probe_request` bind no agent, so
    /// a grid built only from the production builders never asks the question
    /// the role rule exists to answer: what happens to a role that takes no
    /// credentials and names an agent anyway. `host-v1`'s own
    /// `reserved_values` says it in as many words — "neither is told where an
    /// agent's credentials live, **whatever agent the request happens to
    /// name**" — and until this grid existed, deleting the role check from the
    /// container's mount plan changed nothing any test could see (measured:
    /// mutation `M8-credential-volume-for-every-role` survived the whole
    /// suite). That is `PR4-CONF-002`'s class exactly: a predicate keyed on a
    /// field no fixture varies on its own.
    fn hostile_bindings(workspace: &Path) -> Vec<RunnerRequest> {
        let claude = AgentId::new("claude-code");
        vec![
            RunnerRequest {
                command: ShellKind::Sh.spec("cargo test"),
                workspace: workspace.to_path_buf(),
                role: ExecutionRole::Gate,
                timeout: Duration::from_secs(10),
                agent: Some(claude.clone()),
                invocation: gate_id(7),
            },
            RunnerRequest {
                command: ShellKind::Sh.spec("exit 0"),
                workspace: workspace.to_path_buf(),
                role: ExecutionRole::Probe(ProbeTarget::Shell),
                timeout: Duration::from_secs(10),
                agent: Some(claude),
                invocation: shell_probe_id(),
            },
        ]
    }

    /// Every host path a mount hands over.
    fn sources(mounts: &[Mount]) -> Vec<PathBuf> {
        mounts
            .iter()
            .filter_map(|mount| match mount {
                Mount::Path { source, .. } => Some(source.clone()),
                // Neither carries a host path, so neither can hand one over.
                Mount::Volume { .. } | Mount::Tmpfs { .. } => None,
            })
            .collect()
    }

    fn target_of<'a>(mounts: &'a [Mount], target: &str) -> Option<&'a Mount> {
        mounts.iter().find(|mount| mount.target() == target)
    }

    // -----------------------------------------------------------------------
    // 1. Mounts, and the negative space
    // -----------------------------------------------------------------------

    /// The mount set is the role's one worktree, its view, its borrowed object
    /// store and its credential volume — and **nothing that reaches the
    /// coordinator**.
    ///
    /// Both halves, because either alone passes on a wrong implementation:
    /// a positive check ("the worktree is mounted") passes on a container that
    /// also mounts `/`, and a negative check alone passes on a container that
    /// mounts nothing at all. The withheld set is derived from [`RunPaths`] and
    /// `workspace_manager::execution_root_of`, so it moves when the layout does.
    ///
    /// Second field held constant: the role (`Implement`) and the agent
    /// binding; what varies is which withheld path is offered.
    /// The view path an invocation **mounts** is the view path a census
    /// **prunes**, taken from the plan the runner actually builds.
    ///
    /// `<R>/views/<container-name>` is a convention with a producer in one lane
    /// and a consumer in another, and the six intent fields the packet fixes
    /// carry no view path — so the census has to *derive* it. An independent
    /// review measured what two copies cost: changing only the producer to
    /// `<R>/views-v2/<name>` passed the entire suite while silently orphaning
    /// every view the census would have pruned, which is R19 quietly ceasing to
    /// balance after a crash. There is now one definition; this is what fails
    /// if a second one appears.
    ///
    /// The oracle is not either function: the expected value is the literal
    /// `<R>` joined with `views` joined with the name, written here.
    ///
    /// Second field held constant: one run identity and one private root; the
    /// only thing that moves is the invocation, and with it the container name.
    #[test]
    fn the_view_path_the_census_prunes_is_the_one_the_invocation_mounts() {
        let fixture = Fixture::new("view-path-convention", true);
        let runner = fixture.runner();
        let mut seen = std::collections::BTreeSet::new();
        for ordinal in 0..3 {
            let request = worker_request(
                ShellKind::Sh.spec("exit 0"),
                fixture.task_a.clone(),
                AgentId::new("claude-code"),
                Duration::from_secs(10),
                worker_id(ordinal),
            );
            let plan = runner.plan(&request).expect("plans");
            let name = plan.launch.name.clone();

            // The literal convention, written out rather than called.
            let expected = fixture.private_root.join("views").join(name.as_str());
            assert_eq!(
                plan.launch.view.path, expected,
                "the invocation mounts a view the census would not look for"
            );
            assert_eq!(
                crate::runner::container::census::view_path(&fixture.private_root, &name),
                expected,
                "the census prunes a view the invocation would not have mounted"
            );
            // And the mount the container is actually given carries that path,
            // so this is the path on the machine and not only in a field.
            assert!(
                plan.mounts().iter().any(|mount| matches!(
                    mount,
                    Mount::Path { source, .. } if source == &expected
                )),
                "{:?}",
                plan.mounts()
            );
            seen.insert(expected);
        }
        assert_eq!(
            seen.len(),
            3,
            "three invocations must give three view paths; a convention that ignored the \
             container name would give one"
        );
    }

    #[test]
    fn the_mount_set_is_the_roles_own_and_reaches_nothing_of_the_coordinators() {
        let fixture = Fixture::new("mounts", true);
        let runner = fixture.runner();
        let request = worker_request(
            ShellKind::Sh.spec("exit 0"),
            fixture.task_a.clone(),
            AgentId::new("claude-code"),
            Duration::from_secs(10),
            worker_id(0),
        );
        let plan = runner.plan(&request).expect("plans");

        // Positive: five mounts, each with its target and its disposition.
        let mounts = plan.mounts();
        let targets: Vec<&str> = mounts.iter().map(Mount::target).collect();
        assert_eq!(
            targets,
            vec![
                "/tactus/workspace",
                "/tactus/gitview",
                "/tactus/gitobjects",
                "/tactus/workspace/.git",
                "/tactus/credentials/claude-code",
                "/tmp",
            ],
            "the mount set moved"
        );
        assert_eq!(
            target_of(mounts, "/tactus/gitobjects").map(Mount::read_only),
            Some(true),
            "the borrowed object store is read-only (DESIGN.md:612)"
        );
        assert_eq!(
            target_of(mounts, "/tactus/workspace").map(Mount::read_only),
            Some(false),
            "an implementer writes to its worktree"
        );
        // The scratch surface is a tmpfs and therefore carries **no host
        // source**: it is the one writable place that is neither the role's own
        // worktree nor anything of the coordinator's, which is what lets
        // `CreateSpec::read_only_root` close the container layer without making
        // `sh` unusable.
        assert_eq!(
            target_of(mounts, "/tmp"),
            Some(&Mount::Tmpfs {
                target: "/tmp".to_owned()
            }),
            "the scratch surface is not a tmpfs"
        );
        assert!(
            plan.launch.spec.read_only_root,
            "the container layer is writable, so `gate write outside mount fails` does not hold"
        );

        // Negative: no mount source is a withheld path or an ancestor of one.
        let withheld = fixture.withheld();
        assert!(withheld.len() >= 8, "the fixture withholds {withheld:?}");
        for (category, path) in &withheld {
            assert!(
                path.exists(),
                "{path:?} does not exist, so withholding it proves nothing"
            );
            for source in sources(mounts) {
                assert!(
                    !path.starts_with(&source),
                    "the mount `{}` hands the container `{}` ({})",
                    source.display(),
                    path.display(),
                    category.passage()
                );
            }
        }
        assert!(
            fixture.confinement().violations(mounts).is_empty(),
            "{:?}",
            fixture.confinement().violations(mounts)
        );

        // The control: the same check over a mount set that *does* reach the
        // coordinator finds every category. Without it a `violations` that
        // always returned an empty vector would pass the assertion above.
        let hostile = vec![Mount::Path {
            source: fixture.root.clone(),
            target: "/everything".to_owned(),
            read_only: false,
        }];
        let found = fixture.confinement().violations(&hostile);
        let categories: BTreeSet<&str> = Withheld::ALL
            .iter()
            .filter(|category| found.iter().any(|entry| entry.contains(category.passage())))
            .map(|category| category.passage())
            .collect();
        assert_eq!(
            categories.len(),
            Withheld::ALL.len(),
            "a mount of the whole tree did not name every withheld category: {found:#?}"
        );
        assert_eq!(Withheld::ALL.len(), 4);
    }

    /// A workspace that contains a withheld path is refused, by name, before
    /// anything is created.
    ///
    /// This is the assertion a membership test cannot make. The repository root
    /// contains the public log and authoritative Git; `/` contains everything.
    /// Both are plausible values for `RunnerRequest.workspace` — the second is
    /// what a path-joining mistake produces — and both are refused with the
    /// paths named.
    ///
    /// Second field held constant: the role, the agent and the image; what
    /// varies is the workspace.
    #[test]
    fn a_workspace_that_contains_a_withheld_path_is_refused_before_any_effect() {
        let fixture = Fixture::new("hostile-ws", true);
        let runner = fixture.runner();
        for (tag, workspace, expected) in [
            (
                "the repository root",
                fixture.repo.clone(),
                vec![Withheld::PublicLog, Withheld::AuthoritativeGit],
            ),
            (
                "the private root",
                fixture.private_root.clone(),
                vec![Withheld::PrivateArtifacts, Withheld::SiblingWorktree],
            ),
            (
                // The **volume** root of the fixture's own tree, not a bare
                // `Component::RootDir`. `Path::starts_with` is component-wise,
                // and on Windows `C:\\x` begins with a `Prefix` component that
                // a bare `\\` does not have — so a bare root contains nothing
                // there and the refusal would not fire. Measured on the
                // Windows guest, where the first spelling of this row was the
                // slice's only guest failure.
                "the filesystem root",
                fixture
                    .repo
                    .ancestors()
                    .last()
                    .expect("every path has a root")
                    .to_path_buf(),
                Withheld::ALL.to_vec(),
            ),
        ] {
            let request = worker_request(
                ShellKind::Sh.spec("exit 0"),
                workspace,
                AgentId::new("claude-code"),
                Duration::from_secs(10),
                worker_id(0),
            );
            let refusal = runner
                .plan(&request)
                .expect_err("a workspace containing a withheld path is refused");
            let message = refusal.to_string();
            for category in expected {
                assert!(
                    message.contains(category.passage()),
                    "{tag}: the refusal does not name {category:?}: {message}"
                );
            }
        }
        // And nothing was created on the way to any of those refusals: the
        // refusal is in `plan`, which performs no effect at all.
        assert!(
            list_intents(&fixture.private_root)
                .expect("scan")
                .is_empty()
        );
        assert!(fixture.runtime.fake().container_names().is_empty());
    }

    /// Only the reviewer's worktree is read-only.
    ///
    /// DESIGN.md:610: "a `:ro` mount makes the reviewer's read-only
    /// **mechanically** perfect instead of flag-deep." A count, not a spot
    /// check: exactly one of the five roles gets `:ro`, and the other four do
    /// not — a runner that made every mount read-only would pass a test that
    /// only looked at the reviewer.
    ///
    /// Second field held constant: the workspace, the image and the agent
    /// binding each role takes in production; what varies is the role.
    #[test]
    fn only_the_reviewer_receives_a_read_only_worktree() {
        let fixture = Fixture::new("ro-review", true);
        let runner = fixture.runner();
        let mut read_only = Vec::new();
        let mut writable = Vec::new();
        let mut without = Vec::new();
        for request in requests(&fixture.task_a) {
            let plan = runner.plan(&request).expect("plans");
            match target_of(plan.mounts(), "/tactus/workspace") {
                Some(mount) if mount.read_only() => read_only.push(request.role.label()),
                Some(_) => writable.push(request.role.label()),
                None => without.push(request.role.label()),
            }
        }
        assert_eq!(read_only, vec!["review".to_owned()], "{read_only:?}");
        assert_eq!(
            writable,
            vec!["implement".to_owned(), "gate".to_owned()],
            "{writable:?}"
        );
        // The two probe roles receive no worktree at all — a probe has none.
        assert_eq!(
            without,
            vec!["probe(claude-code)".to_owned(), "probe(shell)".to_owned()],
            "{without:?}"
        );
        assert_eq!(read_only.len() + writable.len() + without.len(), 5);
    }

    /// The credential volume is mounted **exactly** when its location is
    /// supplied, and both follow one predicate.
    ///
    /// The intersection that makes this worth writing: {role} × {volume
    /// recorded}. A rule keyed only on the role mounts a volume the record does
    /// not name; a rule keyed only on the record hands a gate an agent's
    /// credentials. And the mount and the environment variable are asserted to
    /// agree cell by cell — two rules that happen to agree today is the shape
    /// this project keeps paying for.
    #[test]
    fn the_credential_volume_is_mounted_exactly_when_its_location_is_supplied() {
        let fixture = Fixture::new("creds", true);
        let with_volumes = fixture.runner();
        let without_volumes = ContainerRunner::new(
            RunnerPolicy {
                credential_volumes: None,
                ..container_policy()
            },
            fixture.identity.clone(),
            &fixture.repo,
            image_environment(),
            Box::new(fixture.runtime.clone()),
        )
        .expect("a container policy with no volumes")
        .with_poll(Duration::ZERO);

        let mut mounted = 0_usize;
        let mut cells = 0_usize;
        let mut volume_names: BTreeSet<String> = BTreeSet::new();
        for (recorded, runner) in [(true, &with_volumes), (false, &without_volumes)] {
            for request in requests(&fixture.task_a) {
                let plan = runner.plan(&request).expect("plans");
                let volume = plan.mounts().iter().find_map(|mount| match mount {
                    Mount::Volume { name, target, .. } => Some((name.clone(), target.clone())),
                    Mount::Path { .. } | Mount::Tmpfs { .. } => None,
                });
                let key = request.agent.as_ref().and_then(host::credential_location);
                // `filter(non-empty)`: since `PR6-CORRECTNESS-007` a location
                // the role is **not** given is named with nothing rather than
                // omitted, because `docker create --env` overlays the image's
                // environment and an omitted key is one the image decides. So
                // "supplied" is a value and not a presence.
                let in_env = key.and_then(|key| {
                    plan.env()
                        .iter()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.clone())
                        .filter(|value| !value.is_empty())
                });
                let expected = recorded
                    && supplies_credential_location(&request.role)
                    && request.agent.is_some();
                if let Some(key) = key {
                    assert!(
                        plan.env().iter().any(|(name, _)| name == key),
                        "{} (recorded: {recorded}): `{key}` is not named at all, so the recorded \
                         image's own value reaches the container",
                        request.role
                    );
                }
                assert_eq!(
                    volume.is_some(),
                    expected,
                    "{} (recorded: {recorded}) mounted {volume:?}",
                    request.role
                );
                assert_eq!(
                    in_env.is_some(),
                    volume.is_some(),
                    "{}: the mount and the location disagree — {volume:?} vs {in_env:?}",
                    request.role
                );
                if let (Some((name, target)), Some(value)) = (volume, in_env) {
                    assert_eq!(
                        target, value,
                        "{}: the variable points elsewhere",
                        request.role
                    );
                    volume_names.insert(name);
                    mounted += 1;
                }
                // The one predicate, asserted rather than assumed.
                assert_eq!(
                    runner
                        .credential_volume_for(&request.role, request.agent.as_ref())
                        .is_some(),
                    expected,
                    "{}",
                    request.role
                );
                cells += 1;
            }
        }
        assert_eq!(cells, 10, "five roles crossed with recorded/absent");
        assert_eq!(mounted, 3, "implement, review and the agent probe, once");

        // The cell the production builders cannot reach: a role that takes no
        // credentials, carrying an agent whose volume the record **does** name.
        // Without it the role check in the mount plan is unmeasured, because
        // `agent.is_some()` already excludes every such role in production.
        let mut hostile_cells = 0_usize;
        for request in hostile_bindings(&fixture.task_a) {
            assert!(request.agent.is_some(), "the fixture bound no agent");
            assert!(
                !supplies_credential_location(&request.role),
                "{}: this role does take credentials, so the cell is not hostile",
                request.role
            );
            assert!(
                fixture
                    .runtime
                    .volume_present("tactus-creds-claude")
                    .expect("reachable"),
                "the volume this role must not receive does not exist"
            );
            let plan = with_volumes.plan(&request).expect("plans");
            assert!(
                !plan
                    .mounts()
                    .iter()
                    .any(|mount| matches!(mount, Mount::Volume { .. })),
                "{}: a role that takes no credentials was handed a credential volume — \
                 `host-v1` refuses this shape whatever agent the request names",
                request.role
            );
            // Named with **nothing**, not omitted: an omitted key is one the
            // recorded image decides, and this role must be told it has no
            // location rather than left to inherit one
            // (`PR6-CORRECTNESS-007`).
            assert_eq!(
                plan.env()
                    .iter()
                    .find(|(key, _)| key == "CLAUDE_CONFIG_DIR")
                    .map(|(_, value)| value.as_str()),
                Some(""),
                "{}: and it was told where they live",
                request.role
            );
            assert_eq!(
                with_volumes.credential_volume_for(&request.role, request.agent.as_ref()),
                None,
                "{}",
                request.role
            );
            hostile_cells += 1;
        }
        assert_eq!(
            hostile_cells, 2,
            "a gate and a shell probe, each bound anyway"
        );
        assert_eq!(
            volume_names,
            BTreeSet::from(["tactus-creds-claude".to_owned()]),
            "the volume is the one the record names for that agent"
        );
    }

    // -----------------------------------------------------------------------
    // 2. Creation from the recorded image id
    // -----------------------------------------------------------------------

    /// Every container is created from the **recorded id**, and a moved
    /// reference does not change what executes.
    ///
    /// The intersection: {image id recorded} × {reference moved}. A runner that
    /// resolved the reference at each invocation passes every test that never
    /// moves the tag, which is every test that does not build this cell.
    #[test]
    fn every_container_is_created_from_the_recorded_id_even_after_the_reference_moves() {
        let fixture = Fixture::new("recorded-id", true);
        let runner = fixture.runner();
        let request = gate_request(
            ShellKind::Sh.spec("exit 0"),
            fixture.task_a.clone(),
            Duration::from_secs(10),
            gate_id(0),
        );

        let before = runner.plan(&request).expect("plans");
        assert_eq!(before.launch.spec.image_id, IMAGE_ID);

        // The reference now names another image, and the old id stays.
        fixture
            .runtime
            .fake()
            .move_tag(IMAGE_REFERENCE, OTHER_IMAGE_ID);
        assert_eq!(
            fixture
                .runtime
                .image_by_reference(IMAGE_REFERENCE)
                .expect("reachable")
                .expect("present")
                .id,
            OTHER_IMAGE_ID,
            "the fixture did not move the reference"
        );
        let after = runner.plan(&request).expect("plans");
        assert_eq!(
            after.launch.spec.image_id, IMAGE_ID,
            "a moved reference changed what executes (INV-23, DESIGN.md:610)"
        );

        // And it really runs from the recorded id. The trace is cleared first
        // so the fixture's own `image_by_reference` — which is how the moved
        // tag was verified above — cannot be mistaken for one the runner made.
        fixture.trace.clear();
        runner.run(&request).expect("runs");
        assert_eq!(
            fixture
                .trace
                .ops()
                .iter()
                .filter(|op| **op == RuntimeOp::Create)
                .count(),
            1
        );
        assert_eq!(
            fixture
                .trace
                .ops()
                .iter()
                .filter(|op| **op == RuntimeOp::InspectImageByReference)
                .count(),
            0,
            "the runner resolved a reference on the way to creating a container"
        );
    }

    /// A reported image id that differs from the record never reaches
    /// `Container.Start`, and **the two phases arrive at the caller as
    /// different things**.
    ///
    /// `expected_failures_refusals[3]` gives the mismatch two outcomes: "refused
    /// before start (**pre-flight/rebuild**)" or "settled as a
    /// **`RunnerSpawnFailure` outage** (mid-run)". The shipped code returned one
    /// `TactusError::Refused` for both, so the mid-run half was unreachable to
    /// any caller — `PR6-CORRECTNESS-001`, whose surviving mutation was to
    /// change the variant and keep the message, because the test checked only
    /// `expect_err`, substrings, `Start`'s absence and cleanup.
    ///
    /// So the grid is `{pre-flight probe, in-run worker, in-run integration
    /// sequence} × {mismatch}` and the assertion is on the **variant**, not on
    /// prose. What is common to every cell — never started, nothing left behind
    /// — is asserted in every cell too, because a fix that distinguished the
    /// phases by *starting* one of them would otherwise pass.
    ///
    /// The settlement event itself is PR7's: `invariants_introduced` makes the
    /// container transition "test-only until PR7 wires TopologyRun", and
    /// `src/topology/**` is frozen. What this slice owes is the distinction,
    /// and `ImageIdMismatch` is where PR7 reads it.
    ///
    /// Second field held constant: the runtime is reachable throughout and the
    /// same image id is substituted in every cell, so only the invocation's
    /// phase moves.
    #[test]
    fn a_substituted_reported_image_id_refuses_before_start_in_both_phases() {
        for (phase, expected, build) in [
            (
                "pre-flight",
                ImageIdMismatch::RefusedBeforeStart,
                (|fixture: &Fixture| {
                    host::shell_probe_request(
                        ShellKind::Sh,
                        fixture.task_a.clone(),
                        shell_probe_id(),
                    )
                }) as fn(&Fixture) -> RunnerRequest,
            ),
            (
                "mid-run worker",
                ImageIdMismatch::SpawnFailureOutage,
                |fixture: &Fixture| {
                    worker_request(
                        ShellKind::Sh.spec("exit 0"),
                        fixture.task_a.clone(),
                        AgentId::new("claude-code"),
                        Duration::from_secs(10),
                        worker_id(0),
                    )
                },
            ),
            (
                "mid-run sequence gate",
                ImageIdMismatch::SpawnFailureOutage,
                |fixture: &Fixture| {
                    let mut request = worker_request(
                        ShellKind::Sh.spec("exit 0"),
                        fixture.task_a.clone(),
                        AgentId::new("claude-code"),
                        Duration::from_secs(10),
                        worker_id(0),
                    );
                    request.invocation = InvocationId::sequence(
                        crate::topology::events::SequenceId(7),
                        crate::runner::invocation::SequenceRole::Gate(0),
                        0,
                    );
                    request
                },
            ),
        ] {
            let fixture = Fixture::new(&format!("mismatch-{phase}"), true);
            let runner = fixture.runner();
            let request = build(&fixture);
            let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
                .expect("a name");
            fixture
                .runtime
                .fake()
                .substitute_reported_image_id(name.as_str(), OTHER_IMAGE_ID);

            let refusal = runner.run(&request).expect_err("{phase}: refused");
            let message = refusal.to_string();
            assert!(message.contains(IMAGE_ID), "{phase}: {message}");
            assert!(message.contains(OTHER_IMAGE_ID), "{phase}: {message}");
            assert!(message.contains("INV-23"), "{phase}: {message}");

            // The phase, as the error's own shape. A caller settles from this.
            assert_eq!(
                ImageIdMismatch::of(&request.invocation),
                expected,
                "{phase}: the phase was classified from the invocation wrongly"
            );
            match expected {
                ImageIdMismatch::RefusedBeforeStart => assert!(
                    matches!(refusal, TactusError::Refused { .. }),
                    "{phase}: a pre-flight mismatch must be a refusal before any spend: \
                     {refusal:?}"
                ),
                ImageIdMismatch::SpawnFailureOutage => assert!(
                    matches!(refusal, TactusError::Agent { .. }),
                    "{phase}: a mid-run mismatch reached the caller as the same generic refusal a \
                     pre-flight one does, so the RunnerSpawnFailure outage settlement the \
                     contract requires is unreachable: {refusal:?}"
                ),
            }

            // Before start, and that is asserted as an absence rather than as
            // an error having come back.
            assert!(
                fixture.trace.position_starting("site:Start").is_none(),
                "{phase}: the container was started: {:#?}",
                fixture.trace.rendered()
            );
            assert!(!fixture.trace.ops().contains(&RuntimeOp::Start), "{phase}");

            // R26 and R19 balance: no container, no intent, no view.
            assert!(
                fixture.runtime.fake().container_names().is_empty(),
                "{phase}"
            );
            assert!(
                list_intents(&fixture.private_root)
                    .expect("scan")
                    .is_empty(),
                "{phase}"
            );
            assert!(
                !fixture
                    .private_root
                    .join("views")
                    .join(name.as_str())
                    .exists()
            );
        }
    }

    /// The phase is read from the invocation, over every form an invocation has.
    ///
    /// The oracle is an independent table over `InvocationId`'s three variants —
    /// derived from the type, not from `ImageIdMismatch::of` — and the
    /// distinct-value count is asserted, so a classifier that collapsed to one
    /// answer fails here whatever it collapsed to.
    #[test]
    fn the_image_mismatch_phase_is_read_from_the_invocation() {
        let cells: [(&str, InvocationId, ImageIdMismatch); 5] = [
            (
                "the RunnerPreflight shell probe",
                shell_probe_id(),
                ImageIdMismatch::RefusedBeforeStart,
            ),
            (
                "a RunnerPreflight agent probe",
                agent_probe_id("claude-code"),
                ImageIdMismatch::RefusedBeforeStart,
            ),
            (
                "an attempt's worker",
                worker_id(0),
                ImageIdMismatch::SpawnFailureOutage,
            ),
            (
                "an attempt's gate",
                gate_id(1),
                ImageIdMismatch::SpawnFailureOutage,
            ),
            (
                "an integration sequence's gate",
                InvocationId::sequence(
                    crate::topology::events::SequenceId(7),
                    crate::runner::invocation::SequenceRole::Gate(0),
                    0,
                ),
                ImageIdMismatch::SpawnFailureOutage,
            ),
        ];
        assert_eq!(
            cells
                .iter()
                .map(|(_, invocation, _)| invocation.render())
                .collect::<BTreeSet<_>>()
                .len(),
            cells.len(),
            "five distinct invocation identities"
        );

        let mut seen = BTreeSet::new();
        for (what, invocation, expected) in &cells {
            let got = ImageIdMismatch::of(invocation);
            assert_eq!(got, *expected, "{what}");
            seen.insert(got);
            // And the error the classification builds carries it in its variant
            // rather than in prose.
            let name =
                ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, invocation).expect("a name");
            let error = got.error(&name, OTHER_IMAGE_ID, IMAGE_ID);
            assert_eq!(
                matches!(error, TactusError::Refused { .. }),
                *expected == ImageIdMismatch::RefusedBeforeStart,
                "{what}: {error:?}"
            );
        }
        assert_eq!(
            seen.into_iter().collect::<Vec<_>>(),
            ImageIdMismatch::ALL.to_vec(),
            "the grid must reach both outcomes the contract names"
        );
        assert_eq!(
            ImageIdMismatch::ALL
                .iter()
                .map(|outcome| outcome.name())
                .collect::<BTreeSet<_>>()
                .len(),
            ImageIdMismatch::ALL.len()
        );
    }

    /// A policy that is not a usable container policy is refused at
    /// construction, before a runner exists to execute anything.
    #[test]
    fn a_policy_that_is_not_a_container_policy_is_refused_at_construction() {
        let fixture = Fixture::new("policy", true);
        let cases: Vec<(&str, RunnerPolicy)> = vec![
            ("a host policy", crate::runner::policy::host_policy()),
            (
                "a container policy with no image",
                RunnerPolicy {
                    image: None,
                    ..container_policy()
                },
            ),
            (
                "a container policy with an empty image id",
                RunnerPolicy {
                    image: Some(ImageIdentity {
                        reference: IMAGE_REFERENCE.to_owned(),
                        id: String::new(),
                        digest: None,
                    }),
                    ..container_policy()
                },
            ),
            (
                "a container kind under the host contract",
                RunnerPolicy {
                    policy: RunnerContract::HostV1,
                    ..container_policy()
                },
            ),
        ];
        for (tag, policy) in cases {
            ContainerRunner::new(
                policy,
                fixture.identity.clone(),
                &fixture.repo,
                image_environment(),
                Box::new(fixture.runtime.clone()),
            )
            .err()
            .unwrap_or_else(|| panic!("{tag} was accepted"));
        }
        // The control: the good one is accepted, and its digest is the record's.
        let runner = ContainerRunner::new(
            container_policy(),
            fixture.identity.clone(),
            &fixture.repo,
            image_environment(),
            Box::new(fixture.runtime.clone()),
        )
        .expect("accepted");
        assert_eq!(
            runner.policy_digest(),
            crate::runner::policy::runner_policy_sha256(&container_policy())
        );
        assert!(runner.policy_digest().starts_with("sha256:"));
    }

    // -----------------------------------------------------------------------
    // 3. Probes through the runner
    // -----------------------------------------------------------------------

    /// The `RunnerPreflight` shell probe executes through **this** runner, as a
    /// registered container invocation created from the recorded image id.
    ///
    /// `decisions.sequential_substrate.runner`: "both implement the
    /// RunnerPreflight shell probe (the recorded shell executing `exit 0`
    /// through the Runner: on the host as an ordinary supervised process, **in
    /// a container from the recorded image id**)". The probe is not
    /// re-implemented here — `host::run_shell_probe` is a free function over
    /// `&dyn Runner`, and this is the same call the host makes, with the runner
    /// varied and everything else held fixed.
    #[test]
    fn the_shell_probe_runs_through_this_runner_as_a_registered_container_invocation() {
        let fixture = Fixture::new("shell-probe", true);
        let runner = fixture.runner();
        let invocation = shell_probe_id();
        let name =
            ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &invocation).expect("a name");

        host::run_shell_probe(
            &runner,
            ShellKind::Sh,
            fixture.task_a.clone(),
            invocation.clone(),
        )
        .expect("the recorded shell runs inside the recorded image");

        // It was a container invocation, in the contract's order, from the
        // recorded id — and the intent that owns it was written first.
        let rendered = fixture.trace.rendered();
        let at = |needle: &str| {
            fixture
                .trace
                .position_starting(needle)
                .unwrap_or_else(|| panic!("`{needle}` is not in {rendered:#?}"))
        };
        assert!(at("durable:synced") < at("rt:create"), "{rendered:#?}");
        assert!(at("site:MountGitView") < at("rt:create"), "{rendered:#?}");
        assert!(at("site:MountGitView") < at("site:Start"), "{rendered:#?}");
        assert!(at("rt:create") < at("site:Start"), "{rendered:#?}");
        assert!(at("site:Start") < at("site:Stop"), "{rendered:#?}");

        // The command really was the recorded shell executing `exit 0`, and the
        // probe carries a probe-role identity.
        assert!(invocation.probe_target().is_some());
        assert_eq!(invocation.render(), "p.shell.o0");

        // A registered invocation: the intent named it, and the record carried
        // the runner digest. The intent is gone now, so the evidence is the
        // container the runtime saw.
        assert!(
            list_intents(&fixture.private_root)
                .expect("scan")
                .is_empty()
        );
        assert!(fixture.runtime.fake().container_names().is_empty());
        assert!(
            !fixture
                .private_root
                .join("views")
                .join(name.as_str())
                .exists()
        );
    }

    /// T-CONTAINER (17), PR6 half: a failing pre-flight probe refuses and its
    /// probe containers are reclaimed.
    ///
    /// **What PR7 completes**, and this test does not claim: the *ordering*
    /// against a recovery event ("refuses before any recovery event") and the
    /// resume that produces one. `decisions.pr_sequence[8].scope` puts "rebuild
    /// of the recorded Runner … with **RunnerPreflight before any recovery
    /// event**" in PR7, and this slice's `permitted_transitions` says the
    /// container transition is "test-only until PR7 wires TopologyRun". What is
    /// held here is the half the mechanism owns: the probe spawn is the only
    /// thing that observes the failure, the refusal names the shell, and the
    /// probe's container, view and intent are all gone afterwards so the run
    /// stays resumable.
    ///
    /// Both probe kinds, because `expected_failures_refusals` names both — "a
    /// recorded **shell or agent CLI** that fails inside the recorded image".
    /// Second field held constant: the image id, which matches the record in
    /// every cell, so what varies is only what the process did.
    ///
    /// ## Both cells go through the thing that turns failure into refusal
    ///
    /// `PR6-ENUM-007`. The agent cell called `runner.run` directly and asserted
    /// a nonzero `ProcessOutput`, which is not what the clause says: the shell
    /// cell reaches its refusal through `host::run_shell_probe`, and the agent
    /// half's equivalent is [`AgentAdapter::probe`], which is where a nonzero
    /// `--version` becomes a `TactusError`. Deleting that refusal left this
    /// named test green, because "the runner returned code 1" was all it
    /// asked. It now drives `ClaudeCodeAdapter::probe` over the container
    /// runner and asserts the **refusal**, so the cell holds
    /// `proof_tests[3]`'s "an agent probe **fails** when the CLI is absent".
    #[test]
    fn failing_preflight_probe_on_resume_refuses_before_recovery_event_and_reclaims_probe_containers()
     {
        for (tag, invocation, exit, stderr) in [
            (
                "shell",
                shell_probe_id(),
                Some(127),
                "sh: exec: not found".to_owned(),
            ),
            (
                "agent",
                agent_probe_id("claude-code"),
                Some(1),
                "claude: command not found".to_owned(),
            ),
        ] {
            let fixture = Fixture::new(&format!("failing-{tag}"), true);
            fixture.runtime.scripts(ContainerExecution {
                exit_code: exit,
                stdout: Vec::new(),
                stderr: stderr.clone().into_bytes(),
            });
            let runner = fixture.runner();
            let name =
                ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &invocation).expect("a name");

            // The observation is a spawn, not an inspection: `non_goals[2]` is
            // "non-spawn shell/CLI presence inspection", and the container was
            // created and started before anything knew the answer.
            if tag == "shell" {
                let refusal = host::run_shell_probe(
                    &runner,
                    ShellKind::Sh,
                    fixture.task_a.clone(),
                    invocation.clone(),
                )
                .expect_err("a shell that fails inside the image refuses");
                assert!(refusal.to_string().contains("sh"), "{refusal}");
                assert!(refusal.to_string().contains("127"), "{refusal}");
            } else {
                // Through the adapter, which is what turns a nonzero
                // `--version` into a refusal. `runner.run` alone returns
                // `Ok(ProcessOutput { code: Some(1) })` — a spawn that
                // succeeded — and a run that stopped there would settle a
                // resume as *certified* on a CLI that is not there.
                let adapter: &dyn crate::agent::AgentAdapter =
                    &crate::agent::claude::ClaudeCodeAdapter;
                let refusal = adapter
                    .probe(&runner)
                    .expect_err("an agent CLI that fails inside the image refuses");
                let message = refusal.to_string();
                assert!(
                    message.contains("claude"),
                    "{tag}: the refusal does not name the CLI: {message}"
                );
                // **The `--version` refusal specifically.** `probe` runs
                // `--version` and then `--help`, and the second has refusals of
                // its own; an `expect_err` alone is satisfied by either, so
                // deleting the nonzero-exit check on `--version` still left
                // this cell green (measured). The clause is "an agent probe
                // **fails when the CLI is absent**", and what observes that is
                // the first spawn's exit status.
                assert!(
                    message.contains("--version"),
                    "{tag}: the refusal came from somewhere other than the CLI's own exit \
                     status: {message}"
                );
                assert!(
                    message.contains(&format!("{exit:?}")),
                    "{tag}: the refusal does not carry the exit status: {message}"
                );
                // The control: the spawn itself succeeded, so the refusal is
                // the adapter's reading of the result and not a spawn failure
                // dressed up as one.
                let request = crate::agent::probe_request(
                    "claude-code",
                    ShellKind::Sh.spec("claude --version"),
                    0,
                    Duration::from_secs(10),
                )
                .expect("an agent probe request");
                let output = runner.run(&request).expect("the spawn itself succeeds");
                assert_eq!(output.code, exit, "the CLI failed inside the image");
                assert!(output.stderr.contains("command not found"));
            }
            assert!(
                fixture.trace.ops().contains(&RuntimeOp::Start),
                "{tag}: nothing was spawned, so nothing observed the failure"
            );

            // The probe containers are reclaimed, and the run stays resumable:
            // no container, no intent, no view.
            assert!(
                fixture.runtime.fake().container_names().is_empty(),
                "{tag}: a probe container survived"
            );
            assert!(
                list_intents(&fixture.private_root)
                    .expect("scan")
                    .is_empty(),
                "{tag}: a probe intent survived"
            );
            assert!(
                !fixture
                    .private_root
                    .join("views")
                    .join(name.as_str())
                    .exists()
            );
            // And the run's own record is untouched by any of it.
            assert!(fixture.paths.events().exists());
        }
    }

    /// One probe identity, two incarnations, two container invocations.
    ///
    /// The intersection {probe kind} × {epoch}. `InvocationId::probe` is
    /// deterministic **by construction**, so the same probe of a resumed run
    /// carries the same identity; without the incarnation in the name the
    /// second epoch's intent would overwrite the first's and the census would
    /// lose the evidence it needs. This is that property at the *runner* level:
    /// two runners differing in nothing but the incarnation.
    #[test]
    fn two_incarnations_of_one_probe_are_two_container_invocations() {
        let fixture = Fixture::new("epochs", true);
        let mut names = BTreeSet::new();
        let mut intents = BTreeSet::new();
        for incarnation in [INCARNATION_1, INCARNATION_2] {
            for invocation in [shell_probe_id(), agent_probe_id("claude-code")] {
                let identity = RunIdentity {
                    incarnation: incarnation.to_owned(),
                    ..fixture.identity.clone()
                };
                let runner = fixture.runner_with(identity);
                let request = if invocation.render().starts_with("p.shell") {
                    host::shell_probe_request(
                        ShellKind::Sh,
                        fixture.task_a.clone(),
                        invocation.clone(),
                    )
                } else {
                    crate::agent::probe_request(
                        "claude-code",
                        ShellKind::Sh.spec("claude --version"),
                        0,
                        Duration::from_secs(10),
                    )
                    .expect("an agent probe request")
                };
                let plan = runner.plan(&request).expect("plans");
                names.insert(plan.launch.name.as_str().to_owned());
                intents.insert(plan.launch.name.intent_path(&fixture.private_root));
                assert_eq!(
                    plan.launch.intent.incarnation, incarnation,
                    "the record does not carry the incarnation it was built for"
                );
            }
        }
        // The identity repeats across incarnations — which is why the name may
        // not — and the fixture proves that rather than assuming it.
        assert_eq!(shell_probe_id().render(), shell_probe_id().render());
        assert_eq!(names.len(), 4, "{names:?}");
        assert_eq!(intents.len(), 4, "{intents:?}");
    }

    // -----------------------------------------------------------------------
    // 4. Environment composition, and parity with the host
    // -----------------------------------------------------------------------

    /// Probe and execution compose through **one** code path, and produce the
    /// same environment.
    ///
    /// DESIGN.md:263. "Two call sites that happen to agree today" is the shape
    /// this sentence is most often satisfied by, so both halves are asserted:
    /// a source census that there is one composition site and one plan site in
    /// this module's production region, and a runtime comparison of the pair
    /// the sentence names.
    ///
    /// The one difference is stated rather than hidden: a probe receives no
    /// worktree ([`receives_a_worktree`]), so its mount set is the execution's
    /// minus the worktree, the view and the borrowed object store. Everything
    /// that decides what the process *is* — the image id, the credential
    /// volume, the reserved values, the overlay — is identical.
    #[test]
    fn probe_and_execution_compose_through_one_code_path() {
        // (a) the source census.
        let source = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("runner")
                .join("container")
                .join("exec.rs"),
        )
        .expect("read this module");
        let production =
            crate::effects::blank_comments(&crate::effects::production_region(&source));
        assert_eq!(
            production.matches("self.environment.compose(").count(),
            1,
            "the environment is composed in more than one place"
        );
        assert_eq!(
            production.matches("self.plan(").count(),
            1,
            "a request becomes a plan in more than one place"
        );
        assert_eq!(
            production.matches("self.mounts(").count(),
            1,
            "the mount set is built in more than one place"
        );

        // (b) the pair the sentence names, composed.
        let fixture = Fixture::new("parity-probe", true);
        let runner = fixture.runner();
        let overlay = (
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS".to_owned(),
            "8000".to_owned(),
        );
        let pairs: Vec<(&str, RunnerRequest, RunnerRequest)> = vec![
            (
                "the agent probe and the worker it certifies",
                crate::agent::probe_request(
                    "claude-code",
                    ShellKind::Sh
                        .spec("claude --version")
                        .env(&overlay.0, &overlay.1),
                    0,
                    Duration::from_secs(10),
                )
                .expect("an agent probe request"),
                worker_request(
                    ShellKind::Sh.spec("claude -p").env(&overlay.0, &overlay.1),
                    fixture.task_a.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(10),
                    worker_id(0),
                ),
            ),
            (
                "the shell probe and the gate it certifies",
                host::shell_probe_request(ShellKind::Sh, fixture.task_a.clone(), shell_probe_id()),
                gate_request(
                    ShellKind::Sh.spec("cargo test"),
                    fixture.task_a.clone(),
                    Duration::from_secs(10),
                    gate_id(0),
                ),
            ),
        ];
        for (tag, probe, execution) in pairs {
            let probed = runner.plan(&probe).expect("plans");
            let executed = runner.plan(&execution).expect("plans");
            assert_eq!(
                probed.launch.spec.image_id, executed.launch.spec.image_id,
                "{tag}: two boundaries"
            );
            // The overlay differs only where the *request* differs, so the
            // composed environments are compared as sets of (key, value).
            let composed = |plan: &InvocationPlan| -> BTreeMap<String, String> {
                plan.env().iter().cloned().collect()
            };
            assert_eq!(
                composed(&probed),
                composed(&executed),
                "{tag}: pre-flight certifies an environment the attempt does not run in"
            );
            // Mounts: the probe's set is the execution's minus the worktree.
            let probe_targets: BTreeSet<&str> = probed.mounts().iter().map(Mount::target).collect();
            let execution_targets: BTreeSet<&str> =
                executed.mounts().iter().map(Mount::target).collect();
            assert!(
                probe_targets.is_subset(&execution_targets),
                "{tag}: {probe_targets:?} vs {execution_targets:?}"
            );
            let difference: BTreeSet<&&str> =
                execution_targets.difference(&probe_targets).collect();
            assert_eq!(
                difference,
                BTreeSet::from([
                    &"/tactus/workspace",
                    &"/tactus/gitview",
                    &"/tactus/gitobjects",
                    &"/tactus/workspace/.git",
                ]),
                "{tag}: the probe and the execution differ by something other than the worktree"
            );
        }
    }

    /// `decisions.tests_acceptance.parity`: "host and container runners produce
    /// identical … **environment composition**".
    ///
    /// The runner is varied and **everything else is held fixed**: one explicit
    /// base, one name rule, one overlay, and all five `ExecutionRole` values
    /// including both probe targets — `ExecutionRole::all()` returns five for
    /// exactly this reason. The base is explicit rather than each runner's own,
    /// because the two bases are *supposed* to differ (the Tactus environment
    /// and the image environment) and a comparison of those would be a
    /// comparison of two fixtures rather than of two composition rules.
    ///
    /// The one place they legitimately differ is stated as an assertion rather
    /// than skipped: a credential *location* is a path at the boundary that
    /// executes, so the host names a host directory and the container names its
    /// mount target. Both are supplied for exactly the same three roles.
    #[test]
    fn host_and_container_compose_the_same_environment_for_every_role() {
        let base: Vec<(String, String)> = [
            ("PATH", "/usr/local/bin:/usr/bin:/bin"),
            ("HOME", "/root"),
            ("LANG", "C.UTF-8"),
            ("CLAUDE_CONFIG_DIR", "/host/claude"),
            ("TACTUS_SHARED", "shared"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        let host_base: Vec<(std::ffi::OsString, std::ffi::OsString)> = base
            .iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        let host_env = HostEnvironment::with_base(host_base, super::super::env::CONTAINER_KEY_CASE);
        let container_env =
            ContainerEnvironment::with_base(base.clone(), super::super::env::CONTAINER_KEY_CASE);
        let volumes: BTreeMap<String, String> = VOLUMES
            .iter()
            .map(|(agent, volume)| ((*agent).to_owned(), (*volume).to_owned()))
            .collect();
        let layout = BoundaryLayout::new();
        let overlay = vec![
            ("TACTUS_OVERLAY".to_owned(), "1".to_owned()),
            ("LANG".to_owned(), "en_GB.UTF-8".to_owned()),
        ];

        let mut supplied_locations = 0_usize;
        let mut rows = 0_usize;
        for role in ExecutionRole::all() {
            let agent = match &role {
                ExecutionRole::Probe(ProbeTarget::Agent(agent)) => Some(agent.clone()),
                ExecutionRole::Implement | ExecutionRole::Review => {
                    Some(AgentId::new("claude-code"))
                }
                ExecutionRole::Gate | ExecutionRole::Probe(ProbeTarget::Shell) => None,
            };
            let scope = RoleScope {
                role: &role,
                agent: agent.as_ref(),
                volumes: &volumes,
                layout: &layout,
            };
            let host_composed: BTreeMap<String, String> = host_env
                .compose(&role, agent.as_ref(), &overlay)
                .expect("the host composes")
                .into_iter()
                .map(|(key, value)| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
                .collect();
            let container_composed: BTreeMap<String, String> = container_env
                .compose(&scope, &overlay)
                .expect("the container composes")
                .into_iter()
                .collect();

            // Same keys, for every role — **plus** the locations the container
            // withholds explicitly.
            //
            // This is the one structural asymmetry between the two boundaries
            // and it is stated rather than skipped (`PR6-CORRECTNESS-007`): the
            // host runner calls `env_clear()` and installs the composed vector
            // as the *whole* environment, so a key it omits is genuinely
            // absent; the container runner's vector is `docker create --env`,
            // which **overlays** the image's environment, so a key it omits is
            // a key the image decides. Naming what is withheld is how the
            // container reaches the same *effective* environment the host
            // reaches by omission — which is what parity is about.
            let withheld: BTreeMap<String, String> = container_env
                .withheld_credential_locations(&scope)
                .into_iter()
                .collect();
            for (key, value) in &withheld {
                assert_eq!(value, "", "{role}: `{key}` is withheld with a value");
                assert!(
                    !host_composed.contains_key(key),
                    "{role}: the host supplies `{key}`, so the container must not withhold it"
                );
                assert_eq!(
                    container_composed.get(key).map(String::as_str),
                    Some(""),
                    "{role}: `{key}` is withheld and is not in the composed vector, so the \
                     image's own value reaches the container"
                );
            }
            let container_supplied: Vec<&String> = container_composed
                .keys()
                .filter(|key| !withheld.contains_key(*key))
                .collect();
            assert_eq!(
                host_composed.keys().collect::<Vec<_>>(),
                container_supplied,
                "{role}: the two runners composed different key sets"
            );
            // Same values everywhere except the credential location.
            let location = agent.as_ref().and_then(host::credential_location);
            for (key, host_value) in &host_composed {
                let container_value = &container_composed[key];
                if Some(key.as_str()) == location {
                    assert_eq!(host_value, "/host/claude");
                    assert_eq!(
                        container_value,
                        &layout.credentials(agent.as_ref().expect("an agent")),
                        "{role}: the container named a location that is not its own"
                    );
                    supplied_locations += 1;
                } else {
                    assert_eq!(
                        host_value, container_value,
                        "{role}: `{key}` composed differently"
                    );
                }
            }
            // And both refuse the same overlay keys.
            for reserved in host::reserved_keys() {
                let bad = vec![(reserved.to_owned(), "x".to_owned())];
                host_env
                    .compose(&role, agent.as_ref(), &bad)
                    .expect_err("the host refuses");
                container_env
                    .compose(&scope, &bad)
                    .expect_err("and so does the container");
            }
            rows += 1;
        }
        assert_eq!(rows, 5, "all five roles, both probe targets");
        assert_eq!(
            supplied_locations, 3,
            "implement, review and the agent probe get a location at both boundaries"
        );
        // The base really did carry a credential location, so "the reserved
        // copies are dropped" is a statement about this fixture.
        assert!(base.iter().any(|(key, _)| key == "CLAUDE_CONFIG_DIR"));
    }

    // -----------------------------------------------------------------------
    // 5. Supervision, release, and the resource ledgers
    // -----------------------------------------------------------------------

    /// A completed invocation stops, removes, unmounts the view and removes the
    /// intent — in that order — and reports what the container did.
    ///
    /// `side_effect_vs_event_ordering`: "stop/rm, view removal, intent removal
    /// **after completion**". Asserted as a sequence of positions in one
    /// ordered trace, not as membership: a release that performed the same four
    /// operations in any other order would satisfy a set.
    ///
    /// Second field held constant: the image id, which matches the record; what
    /// varies is only that the container finished.
    #[test]
    fn a_completed_invocation_releases_in_the_contracts_order_and_reports_the_result() {
        let fixture = Fixture::new("complete", true);
        fixture.runtime.scripts(ContainerExecution {
            exit_code: Some(3),
            stdout: b"the work is done\n".to_vec(),
            stderr: b"a warning\n".to_vec(),
        });
        let runner = fixture.runner();
        let request = worker_request(
            ShellKind::Sh.spec("exit 3"),
            fixture.task_a.clone(),
            AgentId::new("claude-code"),
            Duration::from_secs(10),
            worker_id(0),
        );
        let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
            .expect("a name");

        let output = runner
            .run(&request)
            .expect("a non-zero exit is not an error");
        assert_eq!(output.code, Some(3), "a non-zero exit is a ProcessOutput");
        assert_eq!(output.stdout, "the work is done\n");
        assert_eq!(
            output.stderr, "a warning\n",
            "the seam carries two streams and `ProcessOutput` keeps them apart"
        );
        assert!(!output.timed_out);
        assert!(!output.output_limited);

        let rendered = fixture.trace.rendered();
        let at = |needle: &str| {
            fixture
                .trace
                .position_starting(needle)
                .unwrap_or_else(|| panic!("`{needle}` is not in {rendered:#?}"))
        };
        // The whole sequence, in one chain.
        let order = [
            "site:WriteIntent:before",
            "durable:synced",
            "durable:renamed",
            "durable:dir-synced",
            "site:MountGitView:before",
            "site:Create:before",
            "site:Start:before",
            "rt:collect",
            "site:Stop:before",
            "site:Remove:before",
            "site:UnmountGitView:before",
            "site:RemoveIntent:before",
        ];
        for pair in order.windows(2) {
            assert!(
                at(pair[0]) < at(pair[1]),
                "`{}` is not before `{}` in {rendered:#?}",
                pair[0],
                pair[1]
            );
        }
        // The three clauses of `side_effect_vs_event_ordering`, each stated on
        // its own rather than only as a link in the chain above — a chain is
        // one assertion and the contract is three predicates.
        assert!(
            at("durable:synced") < at("rt:create"),
            "intent synced before docker create"
        );
        assert!(
            at("rt:create") < at("site:Start:before"),
            "created and verified before start"
        );
        assert!(
            at("view:materialized") < at("site:Start:before"),
            "view mounted before start"
        );
        // And the view really is materialised before the create, which is the
        // physical constraint the module docs record.
        assert!(at("view:materialized") < at("rt:create"), "{rendered:#?}");
        // Collected **before** the release, because `docker logs` answers for a
        // running container and not for a removed one.
        assert!(at("rt:collect") < at("rt:remove"), "{rendered:#?}");

        // R26 and R19 balance.
        assert!(fixture.runtime.fake().container_names().is_empty());
        assert!(
            list_intents(&fixture.private_root)
                .expect("scan")
                .is_empty()
        );
        assert!(
            !fixture
                .private_root
                .join("views")
                .join(name.as_str())
                .exists()
        );
    }

    /// A container that outlives its timeout is stopped and removed, and the
    /// output says so.
    ///
    /// `slice_contract.cancellation`: "timeout or shutdown **stops and
    /// removes** the container". The fixture's timeout is `Duration::ZERO`, so
    /// the deadline has passed by the first observation and the supervisor
    /// makes exactly one round trip — `determinism` forbids sleeps and a poll
    /// loop with a real timeout would be one.
    ///
    /// Second field held constant: everything except whether the container
    /// terminates — the same image, the same role, the same workspace as the
    /// completing case above.
    #[test]
    fn a_container_that_outlives_its_timeout_is_stopped_and_removed() {
        let fixture = Fixture::new("timeout", false);
        let runner = fixture.runner();
        let mut request = worker_request(
            ShellKind::Sh.spec("sleep 600"),
            fixture.task_a.clone(),
            AgentId::new("claude-code"),
            Duration::from_secs(10),
            worker_id(0),
        );
        request.timeout = Duration::ZERO;
        let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
            .expect("a name");

        let output = runner
            .run(&request)
            .expect("a timeout is an output, not an error");
        assert!(output.timed_out);
        assert_eq!(
            output.code, None,
            "a container the supervisor stopped did not exit on its own"
        );

        // Stopped and removed, in that order, and the ledgers balance.
        let rendered = fixture.trace.rendered();
        let at = |needle: &str| {
            fixture
                .trace
                .position_starting(needle)
                .unwrap_or_else(|| panic!("`{needle}` is not in {rendered:#?}"))
        };
        assert!(
            at("site:Start:before") < at("site:Stop:before"),
            "{rendered:#?}"
        );
        assert!(
            at("site:Stop:before") < at("site:Remove:before"),
            "{rendered:#?}"
        );
        assert!(
            at("site:Remove:before") < at("site:UnmountGitView:before"),
            "{rendered:#?}"
        );
        assert!(
            at("site:UnmountGitView:before") < at("site:RemoveIntent:before"),
            "{rendered:#?}"
        );
        assert!(fixture.runtime.fake().container_names().is_empty());
        assert!(
            list_intents(&fixture.private_root)
                .expect("scan")
                .is_empty()
        );
        assert!(
            !fixture
                .private_root
                .join("views")
                .join(name.as_str())
                .exists()
        );

        // Exactly one observation: no sleeps, and the loop is bounded by the
        // deadline rather than by a count.
        assert_eq!(
            fixture
                .trace
                .ops()
                .iter()
                .filter(|op| **op == RuntimeOp::Observe)
                .count(),
            1,
            "{rendered:#?}"
        );
    }

    /// Output beyond the bound is truncated and reported as limited.
    ///
    /// Without it `ProcessOutput::output_limited` would be `false` for every
    /// container invocation, and `host::run_shell_probe`'s bounded-output
    /// refusal — a real arm of that function — would be reachable at the host
    /// boundary and unreachable at this one. A pre-flight that certifies less
    /// than the one it is paired with is not the parity the packet asks for.
    ///
    /// Second field held constant: the exit status, which is 0 in both cells,
    /// so what varies is only how much the container printed.
    #[test]
    fn output_beyond_the_bound_is_truncated_and_reported_as_limited() {
        for (tag, bytes, limited) in [("under", 8_usize, false), ("over", 40_usize, true)] {
            let fixture = Fixture::new(&format!("bound-{tag}"), true);
            fixture.runtime.scripts(ContainerExecution {
                exit_code: Some(0),
                stdout: vec![b'x'; bytes],
                stderr: Vec::new(),
            });
            let runner = fixture.runner().with_output_limit(16);
            let request = gate_request(
                ShellKind::Sh.spec("yes"),
                fixture.task_a.clone(),
                Duration::from_secs(10),
                gate_id(0),
            );
            let output = runner.run(&request).expect("runs");
            assert_eq!(output.output_limited, limited, "{tag}");
            assert_eq!(output.stdout.len(), bytes.min(16), "{tag}");
            assert_eq!(output.code, Some(0), "{tag}: the exit status is held fixed");

            // And the probe refusal really is reachable at this boundary.
            let probe = host::run_shell_probe(
                &fixture.runner().with_output_limit(16),
                ShellKind::Sh,
                fixture.task_a.clone(),
                shell_probe_id(),
            );
            if limited {
                let refusal = probe.expect_err("a shell that printed too much is refused");
                assert!(
                    refusal.to_string().contains("bounded output allowance"),
                    "{refusal}"
                );
            } else {
                probe.expect("and an ordinary one is not");
            }
        }
    }

    /// R20: a credential volume is **never created or pruned by a run**, in
    /// every disposition this runner can reach.
    ///
    /// `resource_accounting.rows[R20]` is `operator_owned` and
    /// `persistent_output` in **all five** `at_run_end` outcomes — `Complete`,
    /// `Parked`, `Halted`, `BudgetExceeded`, `NoRunFinished`. A run-end outcome
    /// is a fold over the event log and PR6 has no events at all
    /// (`durable_events`: "none"), so what this slice can measure is the set of
    /// dispositions the runner itself reaches, and the five outcomes differ
    /// only in *which* of them ends the last invocation. Each is driven here
    /// and the volume is asserted present afterwards.
    ///
    /// The failure this prevents is one no ordinary test looks at: a runner
    /// that tidied up a volume it mounted would destroy operator credentials,
    /// and CLIs "rotate refresh tokens on use, and a discarded rotation forces
    /// re-login" (DESIGN.md:612).
    #[test]
    fn a_credential_volume_is_never_created_or_pruned_by_any_disposition() {
        let volume = "tactus-creds-claude";
        /// One way an invocation of this runner can end.
        type Disposition = (&'static str, fn(&Fixture));
        let dispositions: Vec<Disposition> = vec![
            ("complete", |fixture| {
                let request = worker_request(
                    ShellKind::Sh.spec("exit 0"),
                    fixture.task_a.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(10),
                    worker_id(0),
                );
                fixture.runner().run(&request).expect("completes");
            }),
            ("cancelled by timeout", |fixture| {
                let mut request = worker_request(
                    ShellKind::Sh.spec("sleep 600"),
                    fixture.task_a.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(10),
                    worker_id(1),
                );
                request.timeout = Duration::ZERO;
                fixture.runner().run(&request).expect("times out");
            }),
            ("refused for a substituted image id", |fixture| {
                let request = worker_request(
                    ShellKind::Sh.spec("exit 0"),
                    fixture.task_a.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(10),
                    worker_id(2),
                );
                let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
                    .expect("a name");
                fixture
                    .runtime
                    .fake()
                    .substitute_reported_image_id(name.as_str(), OTHER_IMAGE_ID);
                fixture.runner().run(&request).expect_err("refuses");
            }),
            ("refused at a funnel phase", |fixture| {
                let mut hooks = RecordingHooks::new(fixture.trace.clone());
                hooks.fail_at(
                    crate::topology::effects::EffectSiteId::Container(
                        crate::topology::effects::ContainerSite::Create,
                    ),
                    crate::topology::effects::HookPhase::Before,
                );
                let runner = ContainerRunner::new(
                    container_policy(),
                    fixture.identity.clone(),
                    &fixture.repo,
                    image_environment(),
                    Box::new(fixture.runtime.clone()),
                )
                .expect("a container policy")
                .with_hooks(Box::new(hooks))
                .with_poll(Duration::ZERO);
                let request = worker_request(
                    ShellKind::Sh.spec("exit 0"),
                    fixture.task_a.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(10),
                    worker_id(3),
                );
                runner
                    .run(&request)
                    .expect_err("the funnel was made to fail");
            }),
            ("reclaimed as an orphan", |fixture| {
                let request = worker_request(
                    ShellKind::Sh.spec("exit 0"),
                    fixture.task_a.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(10),
                    worker_id(4),
                );
                let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
                    .expect("a name");
                let mut hooks = RecordingHooks::new(fixture.trace.clone());
                crate::runner::container::reclaim(
                    &mut hooks,
                    &fixture.runtime,
                    &crate::runner::container::DisposableDirView::new(fixture.trace.clone()),
                    &fixture.private_root,
                    &name,
                    Some(&view_dir(&fixture.private_root, &name)),
                )
                .expect("reclaim converges on a container that never existed");
            }),
        ];
        assert_eq!(
            dispositions.len(),
            5,
            "one per `at_run_end` outcome: Complete, Parked, Halted, BudgetExceeded, NoRunFinished"
        );
        for (tag, drive) in dispositions {
            let fixture = Fixture::new(&format!("r20-{}", tag.replace(' ', "-")), true);
            assert!(
                fixture.runtime.volume_present(volume).expect("reachable"),
                "{tag}: the operator's volume was not there to begin with"
            );
            drive(&fixture);
            assert!(
                fixture.runtime.volume_present(volume).expect("reachable"),
                "{tag}: the run pruned an operator-owned credential volume"
            );
            for (_, other) in VOLUMES {
                assert!(
                    fixture.runtime.volume_present(other).expect("reachable"),
                    "{tag}: `{other}` is gone"
                );
            }
        }
    }

    /// Every `.rs` file of the container subtree, with each file's **own**
    /// production region.
    ///
    /// ## Why this is a function and not three lines at a call site
    ///
    /// `PR6-ACCT-002`, which is `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN` for the
    /// third time in this slice. The census below concatenated the sources and
    /// called `production_region` **once**: that function cuts a source at its
    /// **first** `#[cfg(test)]`, and `src/runner/container.rs` has one, so
    /// everything appended after it — `runtime.rs`, `intent.rs`, `exec.rs`,
    /// `env.rs`, `view.rs` — was cut away entirely. `census.rs` and `resolve.rs`
    /// were not in the list at all. The census was reading one file and
    /// reporting on seven, and its positive control happened to live before the
    /// cut, so it stayed green while measuring almost nothing.
    ///
    /// So: the directory is **enumerated**, not listed; each file is cut on its
    /// own; and the caller is handed the per-file regions so it can assert the
    /// domain did not shrink.
    /// The two answers: the production regions in the domain, and the files
    /// deliberately outside it.
    ///
    /// Membership is derived from **where `container.rs` declares the module**,
    /// which is the tree's own rule and is written at the top of that file:
    /// "Keep every `#[cfg(test)]` declaration at the BOTTOM". `production_region`
    /// cuts at the first `#[cfg(test)]`, so a `mod x;` above the cut is a
    /// production module and one below it is test-only. Deriving it this way
    /// rather than listing exclusions is what makes a *new* file a failure here
    /// instead of a silent addition to either set.
    ///
    /// A test-only file has no `#[cfg(test)]` of its own — its gate is at the
    /// declaration site — so `production_region` of it is the **whole file**,
    /// and including one would put every fixture's `docker volume create` into
    /// a census of production vocabulary.
    fn container_subtree_production_regions() -> (Vec<(String, String)>, BTreeSet<String>) {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("runner");
        let funnel = std::fs::read_to_string(dir.join("container.rs")).expect("the funnel");
        let declarations =
            crate::effects::blank_comments(&crate::effects::production_region(&funnel));
        let mut regions = vec![("container.rs".to_owned(), declarations.clone())];
        let mut excluded = BTreeSet::new();
        let mut names: Vec<String> = std::fs::read_dir(dir.join("container"))
            .expect("the container subtree")
            .map(|entry| entry.expect("an entry").file_name())
            .filter_map(|name| {
                let name = name.to_string_lossy().into_owned();
                name.ends_with(".rs").then_some(name)
            })
            .collect();
        names.sort();
        for name in names {
            let stem = name.trim_end_matches(".rs");
            if !declarations.contains(&format!("mod {stem};")) {
                excluded.insert(name);
                continue;
            }
            let source =
                std::fs::read_to_string(dir.join("container").join(&name)).expect("a module");
            regions.push((
                name,
                crate::effects::blank_comments(&crate::effects::production_region(&source)),
            ));
        }
        (regions, excluded)
    }

    /// Every exit of `launch` releases what it reached **even when the effect
    /// was already committed**.
    ///
    /// `PR6-ACCT-003`, the axis the fail-fast grid beside this one does not
    /// carry. That grid makes a *primitive* fail — a runtime armed failing, a
    /// view that refuses to materialise — so at every exit the effect never
    /// happened. The funnel's other failure mode is the opposite one: it runs
    /// the primitive and *then* consults the `After` phase
    /// (`container::funnel`), which is what an `Injection::Error` at `After`
    /// models and what a real `docker create` that succeeds and whose following
    /// inspect fails does. The state at the exit is therefore strictly larger:
    /// the record is published, the directory exists, the container exists.
    ///
    /// The intersection is **{which site} × {effect committed}**, and the
    /// committed column is the one that was empty. `Container.WriteIntent` is
    /// in it because that exit was a bare `?` until this round: a `Before`
    /// failure there has written nothing, so the exit looked harmless, and an
    /// `After` failure leaves a durable R26 record with no container and no
    /// view.
    ///
    /// In every cell: R26's container and record and R19's view are all gone,
    /// and R20's volume is untouched.
    ///
    /// Second field held constant: the request, the role and the recorded image
    /// id are identical in all eight cells; only the armed site and phase move.
    #[test]
    fn a_launch_that_fails_after_a_committed_effect_still_releases_everything() {
        use crate::topology::effects::{EffectSiteId, HookPhase};

        let sites = [
            ContainerSite::WriteIntent,
            ContainerSite::MountGitView,
            ContainerSite::Create,
            ContainerSite::Start,
        ];
        let mut cells = 0_usize;
        for site in sites {
            for phase in [HookPhase::Before, HookPhase::After] {
                let fixture = Fixture::new(&format!("committed-{}-{phase}", site.name()), true);
                let mut hooks = RecordingHooks::new(fixture.trace.clone());
                hooks.fail_at(EffectSiteId::Container(site), phase);
                let runner = ContainerRunner::new(
                    container_policy(),
                    fixture.identity.clone(),
                    &fixture.repo,
                    image_environment(),
                    Box::new(fixture.runtime.clone()),
                )
                .expect("a container policy")
                .with_hooks(Box::new(hooks))
                .with_poll(Duration::ZERO);

                let request = worker_request(
                    ShellKind::Sh.spec("exit 0"),
                    fixture.task_a.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(10),
                    worker_id(0),
                );
                let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
                    .expect("a name");

                let refusal = runner
                    .run(&request)
                    .expect_err("the funnel was made to fail");
                assert!(
                    refusal.to_string().contains(site.name()),
                    "[{site:?}/{phase}] the error does not name the site that failed: {refusal}"
                );

                // The premise of the committed column: at `After` the
                // primitive really did run, so there was something to release.
                if phase == HookPhase::After {
                    let ran = match site {
                        ContainerSite::WriteIntent => fixture
                            .trace
                            .rendered()
                            .iter()
                            .any(|entry| entry.starts_with("durable:renamed")),
                        ContainerSite::MountGitView => fixture
                            .trace
                            .rendered()
                            .iter()
                            .any(|entry| entry.starts_with("view:materialized")),
                        ContainerSite::Create => fixture.trace.ops().contains(&RuntimeOp::Create),
                        _ => fixture.trace.ops().contains(&RuntimeOp::Start),
                    };
                    assert!(
                        ran,
                        "[{site:?}/{phase}] the primitive did not run, so this is not the \
                         committed-effect cell it claims to be: {:#?}",
                        fixture.trace.rendered()
                    );
                }

                // R26 and R19 balance, whichever cell this is.
                assert!(
                    fixture.runtime.fake().container_names().is_empty(),
                    "[{site:?}/{phase}] a container survived"
                );
                assert!(
                    list_intents(&fixture.private_root)
                        .expect("scan")
                        .is_empty(),
                    "[{site:?}/{phase}] an R26 intent record survived"
                );
                assert!(
                    !crate::runner::container::intent::containers_dir(&fixture.private_root)
                        .join(format!("{}.intent.tmp", name.as_str()))
                        .exists(),
                    "[{site:?}/{phase}] a staged R26 record survived"
                );
                assert!(
                    !view_dir(&fixture.private_root, &name).exists(),
                    "[{site:?}/{phase}] an R19 view survived"
                );
                // R20 is untouched in every cell.
                for (_, volume) in VOLUMES {
                    assert!(
                        fixture.runtime.volume_present(volume).expect("reachable"),
                        "[{site:?}/{phase}] `{volume}` is gone"
                    );
                }
                cells += 1;
            }
        }
        assert_eq!(cells, 8, "four sites x {{Before, After}}");
    }

    /// The five `at_run_end` outcomes are **driven**, each through the
    /// mechanism its row names, and every physical resource is checked
    /// afterwards.
    ///
    /// `PR6-ACCT-008`. The tables were transcribed and counted — five constant
    /// strings, four `"released"` values, one seeded census — and a table copied
    /// into a test is a table. There was no R19 outcome table at all, and the
    /// R20 disposition grid asserted "one per outcome" by checking that a vector
    /// had five elements.
    ///
    /// ## What this slice can and cannot drive
    ///
    /// A run-end outcome is a fold over the event log and PR6 has
    /// `durable_events: "none"`, so `Complete`, `Parked`, `Halted` and
    /// `BudgetExceeded` are not states this slice can enter. What each of them
    /// *names* is a **mechanism**, and R26's lifecycle sentence enumerates
    /// them: "released on complete (stop/rm, view removed, intent removed),
    /// **cancel**, or **shutdown**", with `NoRunFinished` "reclaimed … at the
    /// next write-command start". Every mechanism in that sentence is
    /// reachable here, so the table below maps each outcome to one and the
    /// mapping is asserted **total** — a sixth outcome, or an outcome with no
    /// mechanism, fails here rather than being counted.
    ///
    /// ## Per resource, not per site
    ///
    /// INV-22 is "every physical or logical owned resource has, for every
    /// lifecycle state, exactly one accounting class and exactly one
    /// non-overlapping inventory row". The site-mapping test proves one row per
    /// *effect site*, which is a different statement: it cannot see the staged
    /// intent record, the implicitly created named volume, or a standalone
    /// view. So the ledger here is over the **resources** a container
    /// invocation owns, each named with its row, and each observed after every
    /// mechanism.
    #[test]
    fn every_at_run_end_outcome_is_driven_through_its_mechanism_and_the_ledgers_balance() {
        /// The mechanism a run-end outcome disposes of R19/R26 through.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum Mechanism {
            /// "released on **complete** (stop/rm, view removed, intent
            /// removed)".
            Complete,
            /// "…, **cancel**, …" — the invocation was stopped before it
            /// finished on its own. `slice_contract.cancellation`: "timeout or
            /// shutdown stops and removes the container".
            Cancel,
            /// "…, or **shutdown**" — the launch itself was refused and
            /// everything it reached was released.
            Shutdown,
            /// R26's fifth cell: "reclaimed when the owner or its incarnation
            /// is dead", at the next write-command start.
            Census,
        }

        /// `decisions.resource_accounting.rows[{R19,R20,R26}].at_run_end`,
        /// transcribed, with the mechanism this slice drives each through.
        const OUTCOMES: &[(&str, &str, &str, &str, Mechanism)] = &[
            (
                "Complete",
                "pruned",
                "persistent_output",
                "released",
                Mechanism::Complete,
            ),
            (
                "Parked",
                "pruned",
                "persistent_output",
                "released",
                Mechanism::Cancel,
            ),
            (
                "Halted",
                "pruned",
                "persistent_output",
                "released",
                Mechanism::Shutdown,
            ),
            (
                "BudgetExceeded",
                "pruned",
                "persistent_output",
                "released",
                Mechanism::Cancel,
            ),
            (
                "NoRunFinished",
                "pruned at the next write-command start after the owning container is observed \
                 terminated",
                "persistent_output",
                "reclaimed when the owner or its incarnation is dead",
                Mechanism::Census,
            ),
        ];
        assert_eq!(OUTCOMES.len(), 5, "the five `at_run_end` outcomes");
        assert_eq!(
            OUTCOMES
                .iter()
                .filter(|(_, r19, ..)| *r19 == "pruned")
                .count(),
            4,
            "R19 is `pruned` in four outcomes; the fifth is pruned by the census"
        );
        assert_eq!(
            OUTCOMES
                .iter()
                .filter(|(_, _, r20, ..)| *r20 == "persistent_output")
                .count(),
            5,
            "R20 is `persistent_output` in ALL five — the row has no cell in which a run may \
             touch it"
        );
        assert_eq!(
            OUTCOMES
                .iter()
                .filter(|(_, _, _, r26, _)| *r26 == "released")
                .count(),
            4,
            "a container surviving a park or a budget stop keeps spending while the run is \
             supposed to be quiescent"
        );
        // Total: every outcome has a mechanism, and every mechanism the
        // lifecycle sentence names is used by an outcome.
        let mechanisms: BTreeSet<Mechanism> =
            OUTCOMES.iter().map(|(.., mechanism)| *mechanism).collect();
        assert_eq!(
            mechanisms,
            BTreeSet::from([
                Mechanism::Complete,
                Mechanism::Cancel,
                Mechanism::Shutdown,
                Mechanism::Census,
            ]),
            "an outcome maps to a mechanism this slice does not drive, or a mechanism the \
             lifecycle sentence names is driven by no outcome"
        );

        let volume = "tactus-creds-claude";
        for (outcome, r19, r20, r26, mechanism) in OUTCOMES {
            let fixture = Fixture::new(&format!("outcome-{outcome}"), true);
            // R20's premise: the operator's volume is there before anything
            // runs. `persistent_output` is a claim about a resource that
            // exists.
            assert!(
                fixture.runtime.volume_present(volume).expect("reachable"),
                "[{outcome}] the operator's volume was not there to begin with"
            );

            let request = worker_request(
                ShellKind::Sh.spec("exit 0"),
                fixture.task_a.clone(),
                AgentId::new("claude-code"),
                Duration::from_secs(10),
                worker_id(0),
            );
            let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
                .expect("a name");
            let view = view_dir(&fixture.private_root, &name);
            let intent = name.intent_path(&fixture.private_root);
            let staged = crate::runner::container::intent::containers_dir(&fixture.private_root)
                .join(format!("{}.intent.tmp", name.as_str()));

            match mechanism {
                Mechanism::Complete => {
                    fixture.runner().run(&request).expect("completes");
                }
                Mechanism::Cancel => {
                    // The cancellation clause: "timeout or shutdown stops and
                    // removes the container". The container is left **running**
                    // by the fake, so the supervisor really does reach its
                    // deadline with something to stop — a fixture whose
                    // container exits on start would take the ordinary
                    // completion path and be a fifth copy of the cell above.
                    let fixture = Fixture::new(&format!("outcome-{outcome}-live"), false);
                    let mut request = request.clone();
                    request.timeout = Duration::ZERO;
                    let output = fixture.runner().run(&request).expect("times out");
                    assert!(
                        output.timed_out,
                        "[{outcome}] the cancellation cell did not cancel anything"
                    );
                    assert!(
                        fixture.trace.ops().contains(&RuntimeOp::Stop),
                        "[{outcome}] nothing was stopped"
                    );
                    assert!(
                        fixture.runtime.fake().container_names().is_empty()
                            && list_intents(&fixture.private_root)
                                .expect("scan")
                                .is_empty()
                            && !view_dir(&fixture.private_root, &name).exists(),
                        "[{outcome}] R19/R26 residue after a cancel"
                    );
                    assert!(
                        fixture.runtime.volume_present(volume).expect("reachable"),
                        "[{outcome}] the run pruned an operator-owned credential volume"
                    );
                    continue;
                }
                Mechanism::Shutdown => {
                    // The launch is refused mid-sequence and releases what it
                    // reached. R26's "shutdown" is the run stopping before an
                    // invocation could finish, which is the same disposal path.
                    fixture
                        .runtime
                        .fake()
                        .substitute_reported_image_id(name.as_str(), OTHER_IMAGE_ID);
                    fixture.runner().run(&request).expect_err("refuses");
                }
                Mechanism::Census => {
                    // Seeded rather than run: the owner is a *dead* incarnation,
                    // which by construction is not this process.
                    let mut hooks = RecordingHooks::new(fixture.trace.clone());
                    let dead = fixture.runner_with(RunIdentity {
                        incarnation: INCARNATION_2.to_owned(),
                        ..fixture.identity.clone()
                    });
                    let plan = dead.plan(&request).expect("plans");
                    crate::runner::container::launch(
                        &mut hooks,
                        &fixture.runtime,
                        &crate::runner::container::view::RoleGitView::new(fixture.trace.clone()),
                        &plan.launch,
                    )
                    .expect("the dead incarnation's container");
                    let orphan = plan.launch.name.clone();
                    assert!(
                        orphan.intent_path(&fixture.private_root).exists()
                            && view_dir(&fixture.private_root, &orphan).exists()
                            && !fixture.runtime.fake().container_names().is_empty(),
                        "[{outcome}] the orphan was not seeded"
                    );
                    let liveness = crate::runner::container::FakeOwnerLiveness::new();
                    let view_impl =
                        crate::runner::container::view::RoleGitView::new(fixture.trace.clone());
                    crate::runner::container::census::run_startup_census(
                        &mut hooks,
                        &crate::runner::container::census::Census {
                            private_root: &fixture.private_root,
                            start: &crate::runner::container::census::CensusStart::FreshRun {
                                incarnation: INCARNATION_1.to_owned(),
                            },
                            runtime: &fixture.runtime,
                            liveness: &liveness,
                            view: &view_impl,
                        },
                    )
                    .expect("the census completes");
                    assert!(
                        fixture.runtime.fake().container_names().is_empty(),
                        "[{outcome}] R26: the container"
                    );
                    assert!(
                        !orphan.intent_path(&fixture.private_root).exists(),
                        "[{outcome}] R26: the intent record"
                    );
                    assert!(
                        !view_dir(&fixture.private_root, &orphan).exists(),
                        "[{outcome}] R19: the view"
                    );
                    assert!(
                        fixture.runtime.volume_present(volume).expect("reachable"),
                        "[{outcome}] the census pruned an operator-owned credential volume"
                    );
                    continue;
                }
            }

            // The per-resource ledger, for the two mechanisms that fall
            // through: R26's container, R26's record (both halves), R19's
            // directory, R20's volume.
            assert!(
                fixture.runtime.fake().container_names().is_empty(),
                "[{outcome}] R26 `{r26}`: the container survived"
            );
            assert!(!intent.exists(), "[{outcome}] R26 `{r26}`: the record");
            assert!(!staged.exists(), "[{outcome}] R26 `{r26}`: the staged half");
            assert!(
                !view.exists(),
                "[{outcome}] R19 `{r19}`: the view directory"
            );
            for (_, other) in VOLUMES {
                assert!(
                    fixture.runtime.volume_present(other).expect("reachable"),
                    "[{outcome}] R20 `{r20}`: `{other}` is gone"
                );
            }
        }
    }

    /// Every physical resource a container invocation owns has exactly one row.
    ///
    /// `PR6-ACCT-008`'s second half, and INV-22's own sentence: "exactly one
    /// accounting class and exactly one **non-overlapping** inventory row per
    /// `decisions.resource_accounting`". The site-mapping census proves one row
    /// per *effect site*; a site is not a resource, and the resources with no
    /// site of their own are exactly the ones nothing was checking — the staged
    /// intent record, the anonymous volume a `VOLUME` declaration creates, and
    /// the named volume `docker create` creates implicitly.
    ///
    /// The table is the resources, not the sites, and each row names where in
    /// this tree that resource is disposed of. Everything here is asserted
    /// against the packet's row text rather than against the code.
    #[test]
    fn every_physical_resource_of_a_container_invocation_maps_to_exactly_one_row() {
        /// `(resource, row, class while it exists, what disposes of it)`.
        const RESOURCES: &[(&str, &str, &str, &str)] = &[
            (
                "the running container and its labels",
                "R26",
                "released",
                "Container.Stop + Container.Remove",
            ),
            (
                "the published `<name>.intent` record",
                "R26",
                "released",
                "Container.RemoveIntent",
            ),
            (
                "the staged `<name>.intent.tmp` half",
                "R26",
                "released",
                "Container.RemoveIntent, reached by the census's staged sweep",
            ),
            (
                "the anonymous volume an image's VOLUME declaration creates",
                "R26",
                "released",
                "Container.Remove, which issues `rm --force --volumes`",
            ),
            (
                "the disposable Git view directory",
                "R19",
                "pruned",
                "Container.UnmountGitView",
            ),
            (
                "the per-agent credential volume",
                "R20",
                "persistent_output",
                "nothing: never created or pruned by a run",
            ),
        ];

        // (1) One row per resource, and the rows are the three this slice owns.
        let rows: BTreeSet<&str> = RESOURCES.iter().map(|(_, row, ..)| *row).collect();
        assert_eq!(
            rows,
            BTreeSet::from(["R19", "R20", "R26"]),
            "a container invocation's resources map outside the three rows this slice owns"
        );
        let names: BTreeSet<&str> = RESOURCES.iter().map(|(name, ..)| *name).collect();
        assert_eq!(
            names.len(),
            RESOURCES.len(),
            "two rows name the same resource, so the inventory overlaps"
        );

        // (2) One class per row, over every resource in it: a row whose
        // resources disagreed about their class would be two rows.
        let mut by_row: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (_, row, class, _) in RESOURCES {
            by_row.entry(row).or_default().insert(class);
        }
        for (row, classes) in &by_row {
            assert_eq!(
                classes.len(),
                1,
                "`{row}` holds resources of {classes:?}, so it is not one accounting class"
            );
        }
        assert_eq!(by_row["R19"], BTreeSet::from(["pruned"]));
        assert_eq!(by_row["R20"], BTreeSet::from(["persistent_output"]));
        assert_eq!(by_row["R26"], BTreeSet::from(["released"]));
        // Every class named is one `decisions.resource_accounting.classes`
        // declares.
        for (_, _, class, _) in RESOURCES {
            assert!(
                [
                    "released",
                    "consumed",
                    "persistent_output",
                    "pruned",
                    "resumably_open"
                ]
                .contains(class),
                "`{class}` is not one of the five accounting classes"
            );
        }

        // (3) The R20 row is the only one with no disposer, and that is the
        // whole of `enforcement_domains.operator_owned`.
        let undisposed: Vec<&str> = RESOURCES
            .iter()
            .filter(|(_, _, _, by)| by.starts_with("nothing"))
            .map(|(name, ..)| *name)
            .collect();
        assert_eq!(
            undisposed,
            vec!["the per-agent credential volume"],
            "a resource other than R20 has nothing that releases it, or R20 acquired one"
        );

        // (4) Every disposer that names a site names one of the frozen eight,
        // so a resource cannot be disposed of by something outside the funnel.
        let site_names: BTreeSet<&str> =
            ContainerSite::ALL.iter().map(|site| site.name()).collect();
        let mut named = 0_usize;
        for (resource, _, _, by) in RESOURCES {
            for word in by.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
                let Some(site) = word.strip_prefix("Container.") else {
                    continue;
                };
                assert!(
                    site_names.contains(site),
                    "`{resource}` is disposed of at `Container.{site}`, which is not one of the \
                     frozen eight sites"
                );
                named += 1;
            }
        }
        assert!(named >= 4, "the disposers name no sites at all");
    }

    /// Nothing in the container subtree can create or prune a volume.
    ///
    /// The runtime assertion above measures the dispositions a test drove; this
    /// measures the *domain* — `enforcement_domains.operator_owned`: "R20
    /// credential volumes: **never created or pruned by a run**". The seam has
    /// one volume method and it returns a `bool`, and the `docker` CLI issues
    /// exactly one volume subcommand, which is `inspect`.
    ///
    /// **The domain has a control** (`PR6-ACCT-002`): every file of the subtree
    /// is enumerated from the directory rather than listed by hand, every one
    /// contributes a non-trivial production region, and the *set* of files is
    /// asserted to contain the seven this slice wrote. A future `#[cfg(test)]`
    /// hoisted to the top of any of them, or a new module added beside them,
    /// fails here rather than silently emptying the census.
    #[test]
    fn the_container_subtree_can_only_inspect_a_volume() {
        let (regions, excluded) = container_subtree_production_regions();

        // -- the control on the domain itself --------------------------------
        let files: BTreeSet<&str> = regions.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            files,
            [
                "container.rs",
                "census.rs",
                "env.rs",
                "exec.rs",
                "intent.rs",
                "resolve.rs",
                "runtime.rs",
                "view.rs",
            ]
            .into_iter()
            .collect::<BTreeSet<_>>(),
            "the domain of the R20 vocabulary census moved. Every production module of the \
             container subtree must be in it: `PR6-ACCT-002` measured this census reading \
             `container.rs` alone and reporting on seven files, because the sources were \
             concatenated and cut at the FIRST `#[cfg(test)]`"
        );
        assert_eq!(
            excluded,
            ["fake.rs", "tests.rs"]
                .into_iter()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>(),
            "a file of this subtree is neither declared above `container.rs`'s `#[cfg(test)]` cut \
             (production, in the domain) nor below it (test-only, excluded), so nothing decides \
             which it is"
        );
        for (name, production) in &regions {
            assert!(
                production.len() > 2_000,
                "`{name}`'s production region is {} bytes, so the census below is measuring \
                 almost nothing of it — this is `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`",
                production.len()
            );
        }
        let joined: String = regions
            .iter()
            .map(|(_, production)| production.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // The positive control: the read-only inspection really is there, so a
        // census that had stopped finding anything fails here rather than
        // reporting silence. It lives in `container.rs` **below** the old cut
        // point's neighbours, so it is also evidence the domain is whole.
        assert_eq!(
            joined.matches("\"volume\",").count(),
            1,
            "the `docker volume` census is measuring nothing"
        );
        assert!(joined.contains("\"inspect\""));
        for mutating in ["\"create\", ", "volume rm", "volume prune", "\"prune\""] {
            assert_eq!(
                joined.matches(mutating).count(),
                0,
                "the container subtree names `{mutating}`, so a run could create or prune a volume"
            );
        }

        // -- and the vocabulary census is not the whole claim -----------------
        // `docker create` creates an absent named volume **implicitly**, with
        // no `volume create` anywhere (measured, docker 29.7.2). No search over
        // this subtree's text can see that, so the domain census is paired with
        // the guard that can: every `Mount::Volume` is re-inspected before the
        // create, in `container::create_container`.
        // `a_create_whose_named_volume_is_absent_is_refused_before_any_effect`
        // drives it; this asserts the guard is *in the production region*, so
        // it cannot be deleted while the vocabulary census stays green.
        let funnel = &regions
            .iter()
            .find(|(name, _)| name == "container.rs")
            .expect("the funnel")
            .1;
        assert!(
            funnel.contains("fn expect_mounted_volumes_present"),
            "the create-time volume guard is gone, so `docker create` can create an R20 volume \
             without this subtree naming `volume create` at all"
        );
        assert_eq!(
            funnel.matches("expect_mounted_volumes_present").count(),
            2,
            "the guard must be defined and called exactly once"
        );

        // And the seam has one volume method.
        let seam = &regions
            .iter()
            .find(|(name, _)| name == "runtime.rs")
            .expect("the seam")
            .1;
        assert_eq!(seam.matches("fn volume").count(), 1, "one volume method");
        assert!(
            seam.contains("fn volume_present(&self, name: &str) -> Result<bool, RuntimeError>")
        );
    }

    /// The container runner is a `Runner` like any other: object-safe, `Send`
    /// and `Sync`.
    ///
    /// PR11 turns `run` into a boxed `Send` future behind the same `&dyn
    /// Runner` its callers hold, so a container runner that stopped being
    /// object-safe would fail to compile here rather than at the migration —
    /// the same guard `runner::tests::the_runner_trait_is_object_safe` gives
    /// the host.
    #[test]
    fn the_container_runner_is_object_safe_and_send_and_sync() {
        fn takes_dyn(_: &dyn Runner) {}
        fn is_send_sync<T: Send + Sync>() {}
        is_send_sync::<ContainerRunner>();

        let fixture = Fixture::new("object-safe", true);
        let runner = fixture.runner();
        takes_dyn(&runner);
        let boxed: Box<dyn Runner> = Box::new(fixture.runner());
        takes_dyn(boxed.as_ref());
        // And a `GitView` is object-safe too, which is what lets the funnel
        // take `&dyn GitView` and this module hand it a projection.
        let view: Box<dyn GitView> = Box::new(RoleGitView::new(ContainerTrace::off()));
        fn takes_view(_: &dyn GitView) {}
        takes_view(view.as_ref());
    }

    // -----------------------------------------------------------------------
    // 5b. Repair R1: confinement, the intent capability, and cwd-independence
    // -----------------------------------------------------------------------

    /// One row per invocation: the container's name, and the bytes of its
    /// intent record — `None` when there was none, which is the state
    /// catalogue survivor `PR6-INTENT-020` describes.
    type SeenIntents = Arc<Mutex<Vec<(String, Option<Vec<u8>>)>>>;

    /// A `GitView` that reads the container's intent record at the moment the
    /// view is materialised.
    ///
    /// `Container.MountGitView` runs **after** `Container.WriteIntent` and
    /// **before** `Container.Create`, so this observes `<R>/containers` at
    /// exactly the point the contract says the record must already be there:
    /// "intent synced before docker create". The record is removed again when
    /// the invocation completes, which is why a test that looked afterwards
    /// could only ever see an absence.
    #[derive(Debug)]
    struct IntentPeek {
        inner: crate::runner::container::DisposableDirView,
        private_root: PathBuf,
        seen: SeenIntents,
    }

    impl GitView for IntentPeek {
        fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, TactusError> {
            // The view directory is `<R>/views/<container-name>`, so its file
            // name is the container's.
            let name = request
                .path
                .file_name()
                .expect("a view path names a container")
                .to_string_lossy()
                .into_owned();
            let record = std::fs::read(
                crate::runner::container::intent::containers_dir(&self.private_root)
                    .join(format!("{name}.intent")),
            )
            .ok();
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((name, record));
            self.inner.materialize(request)
        }

        fn discard(&self, path: &Path) -> Result<(), TactusError> {
            self.inner.discard(path)
        }
    }

    /// A `GitView` whose materialisation fails, so `Container.MountGitView` can
    /// be the step a launch dies at.
    #[derive(Debug, Default)]
    struct FailingView;

    impl GitView for FailingView {
        fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, TactusError> {
            Err(TactusError::Refused {
                message: format!("VIEW-REFUSED for {}", request.path.display()),
            })
        }

        fn discard(&self, _path: &Path) -> Result<(), TactusError> {
            Ok(())
        }
    }

    /// The **public constructor** withholds every category from every role that
    /// receives a worktree, with no builder call at all.
    ///
    /// `PR6-CORRECTNESS-011` / `PR6-ENUM-002`. `ContainerRunner::new` used to
    /// default to `Confinement::none()` with the real set added by an optional
    /// `with_confinement`, so the runner a caller gets by construction
    /// withheld nothing: `PR6-ENUM-002`'s mutation is literally "omit
    /// `with_confinement` at a construction site and submit
    /// `identity.private_root` as a worker workspace", and `-011`'s is "apply
    /// configured confinement only to `ExecutionRole::Implement`". This runner
    /// is built with **`new` and nothing else**, and the grid crosses all four
    /// withheld categories with all five roles, so a rule that held for one
    /// role fails here.
    ///
    /// The two probe roles are the other half of the grid rather than an
    /// omission: a probe receives no worktree at all
    /// ([`receives_a_worktree`]), so a hostile workspace cannot reach it — and
    /// that is asserted as "no mount source is under the hostile path" rather
    /// than assumed.
    #[test]
    fn the_public_constructor_withholds_every_category_from_every_role() {
        let fixture = Fixture::new("default-confinement", true);
        let runner = ContainerRunner::new(
            container_policy(),
            fixture.identity.clone(),
            &fixture.repo,
            image_environment(),
            Box::new(fixture.runtime.clone()),
        )
        .expect("a container policy");

        let execution_root =
            crate::workspace_manager::execution_root_of(&fixture.private_root, REPO_KEY, RUN_ID);
        // One hostile workspace per category, each written from the type that
        // owns that path rather than read back out of `Confinement`.
        let hostile: Vec<(Withheld, PathBuf)> = vec![
            (Withheld::PublicLog, fixture.paths.public.clone()),
            (Withheld::PrivateArtifacts, fixture.private_root.clone()),
            (Withheld::SiblingWorktree, execution_root),
            (Withheld::AuthoritativeGit, fixture.repo.clone()),
        ];

        let mut refused = 0_usize;
        let mut probe_cells = 0_usize;
        let mut named: BTreeSet<Withheld> = BTreeSet::new();
        for (category, workspace) in &hostile {
            for request in requests(workspace) {
                if receives_a_worktree(&request.role) {
                    let refusal = runner
                        .plan(&request)
                        .expect_err("a workspace containing a withheld path is refused");
                    let message = refusal.to_string();
                    assert!(
                        message.contains(category.passage()),
                        "{} / {}: {message}",
                        request.role,
                        workspace.display()
                    );
                    named.insert(*category);
                    refused += 1;
                } else {
                    let plan = runner
                        .plan(&request)
                        .expect("a probe has no worktree to refuse");
                    for source in sources(plan.mounts()) {
                        assert!(
                            !source.starts_with(workspace),
                            "{}: a probe was handed `{}`",
                            request.role,
                            source.display()
                        );
                    }
                    probe_cells += 1;
                }
            }
        }
        assert_eq!(
            refused,
            4 * 3,
            "four categories crossed with three worktree roles"
        );
        assert_eq!(
            probe_cells,
            4 * 2,
            "four categories crossed with two probe roles"
        );
        assert_eq!(named.len(), Withheld::ALL.len(), "{named:?}");

        // The control: the role's own worktree plans on the same runner, so the
        // refusals above are about the path and not about `plan` being broken.
        let mut planned = 0_usize;
        for request in requests(&fixture.task_a) {
            runner
                .plan(&request)
                .expect("the role's own worktree plans");
            planned += 1;
        }
        assert_eq!(planned, 5);

        // And nothing was created on the way to any of it.
        assert!(
            list_intents(&fixture.private_root)
                .expect("scan")
                .is_empty()
        );
        assert!(fixture.runtime.fake().container_names().is_empty());
    }

    /// The execution root **and each of its three worktree namespaces** are
    /// withheld, and any one worktree is not.
    ///
    /// `PR6-CORRECTNESS-013`. `Confinement::of_run` named no worktree path at
    /// all — the fixtures added siblings by hand, so the production helper was
    /// unmeasured — and a Gate handed the run's execution root received every
    /// task and merge worktree in one mount. Withholding only the root would
    /// leave `<root>/tasks`, which still holds two, so the namespaces are
    /// withheld too.
    ///
    /// The expected paths are built from
    /// [`crate::workspace_manager::execution_root_of`] and the packet's own
    /// three namespace names, **not** read back from `Confinement::entries` —
    /// that would be the function's own oracle.
    ///
    /// Second field held constant: the run identity and the repository; what
    /// varies is only which directory of the worktree namespace is offered.
    #[test]
    fn the_execution_root_and_its_worktree_namespaces_are_withheld_and_one_worktree_is_not() {
        let fixture = Fixture::new("exec-root", true);
        let runner = fixture.runner();
        let execution_root =
            crate::workspace_manager::execution_root_of(&fixture.private_root, REPO_KEY, RUN_ID);
        // `decisions.workspace_candidates.manager`: "tasks/k<key>-g<gen>,
        // merge/s<seq>", plus `snapshots`.
        let namespaces: Vec<PathBuf> = vec![
            execution_root.clone(),
            execution_root.join("tasks"),
            execution_root.join("merge"),
            execution_root.join("snapshots"),
        ];

        // (a) `of_run` names each of them, under the sibling-worktree category.
        let confinement = Confinement::of_run(&fixture.identity, &fixture.repo);
        for path in &namespaces {
            assert!(
                confinement
                    .entries()
                    .iter()
                    .any(
                        |(category, entry)| *category == Withheld::SiblingWorktree && entry == path
                    ),
                "`of_run` does not withhold `{}`: {:?}",
                path.display(),
                confinement.entries()
            );
        }

        // (b) and a role handed one is refused, naming the category.
        let mut refused = 0_usize;
        for path in &namespaces {
            for request in requests(path) {
                if !receives_a_worktree(&request.role) {
                    continue;
                }
                let refusal = runner
                    .plan(&request)
                    .expect_err("a mount containing every worktree is refused");
                assert!(
                    refusal
                        .to_string()
                        .contains(Withheld::SiblingWorktree.passage()),
                    "{} / {}: {refusal}",
                    request.role,
                    path.display()
                );
                refused += 1;
            }
        }
        assert_eq!(
            refused,
            4 * 3,
            "four namespace directories, three worktree roles"
        );

        // (c) The over-refusal control, which is what this fix could most
        // easily get wrong: withholding the namespace must not withhold the
        // worktrees inside it. Each of the run's three real worktrees plans,
        // for each worktree role — a container receives *one* worktree, and
        // which one is the engine's decision, not this runner's.
        let mut planned = 0_usize;
        for workspace in [&fixture.task_a, &fixture.task_b, &fixture.merge] {
            for request in requests(workspace) {
                if !receives_a_worktree(&request.role) {
                    continue;
                }
                runner
                    .plan(&request)
                    .unwrap_or_else(|error| panic!("{}: {error}", workspace.display()));
                planned += 1;
            }
        }
        assert_eq!(planned, 3 * 3, "three worktrees, three worktree roles");
    }

    /// Every role — **both probe kinds included** — writes and syncs its own
    /// six-field intent record, and it is on disk before its container is
    /// created.
    ///
    /// Catalogue survivor `PR6-INTENT-020`: an agent probe container created
    /// with no intent record passed, because the suite exercised both probe
    /// kinds and never asserted that each writes its own record.
    /// `T-CONTAINER.boundary` is "the RunnerPreflight shell and agent probe
    /// containers are container invocations **like every other**", so the grid
    /// is all five roles rather than the two the finding names.
    ///
    /// The record is read at `Container.MountGitView`, which is between the
    /// write and the create: after the invocation completes the record is gone,
    /// so a test that looked afterwards could only ever see an absence. The six
    /// field values are literals from this module's constants and from the
    /// request's own `InvocationId` — never from `plan.launch.intent`, which is
    /// the runner's own answer.
    ///
    /// Second field held constant: the run, the incarnation and the image;
    /// what varies is the role, and with it the invocation.
    #[test]
    fn every_role_writes_and_syncs_its_own_six_field_intent_before_its_container_is_created() {
        let fixture = Fixture::new("intent-per-invocation", true);
        let seen: SeenIntents = Arc::new(Mutex::new(Vec::new()));
        let runner = ContainerRunner::new(
            container_policy(),
            fixture.identity.clone(),
            &fixture.repo,
            image_environment(),
            Box::new(fixture.runtime.clone()),
        )
        .expect("a container policy")
        .with_hooks(Box::new(RecordingHooks::new(fixture.trace.clone())))
        .with_view(Box::new(IntentPeek {
            inner: crate::runner::container::DisposableDirView::new(fixture.trace.clone()),
            private_root: fixture.private_root.clone(),
            seen: Arc::clone(&seen),
        }))
        .with_poll(Duration::ZERO);

        let all = requests(&fixture.task_a);
        assert_eq!(all.len(), 5, "all five roles");
        let mut expected: Vec<(String, String)> = Vec::new();
        for request in &all {
            let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
                .expect("a name");
            expected.push((name.as_str().to_owned(), request.invocation.render()));
            runner.run(request).expect("runs");
        }

        let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(seen.len(), 5, "five invocations, five views");
        let mut invocations: BTreeSet<String> = BTreeSet::new();
        for ((name, bytes), (expected_name, expected_invocation)) in seen.iter().zip(&expected) {
            assert_eq!(name, expected_name, "the view names another container");
            let bytes = bytes.as_ref().unwrap_or_else(|| {
                panic!("`{name}` reached Container.Create with no intent record of its own")
            });
            let record: ContainerIntent =
                serde_json::from_slice(bytes).expect("the six fields parse");
            assert_eq!(record.run_id, RUN_ID);
            // R1 wrote this against the backslash-rewrite encoding that repair
            // R2 replaced with a percent-encoding. Decode through the accessor
            // the census itself uses, which asserts the round-trip rather than
            // one side of it -- strictly stronger than comparing the stored
            // bytes to a hand-written transform.
            assert_eq!(
                crate::runner::container::intent::owner_run_dir(&record.run_dir, "intent record")
                    .expect("the recorded run dir decodes"),
                fixture.paths.public
            );
            assert_eq!(record.incarnation, INCARNATION_1);
            assert_eq!(record.repo_key, REPO_KEY);
            assert_eq!(
                &record.invocation, expected_invocation,
                "`{name}` carries another invocation's record"
            );
            assert_eq!(
                record.runner_policy_sha256,
                crate::runner::policy::runner_policy_sha256(&container_policy())
            );
            invocations.insert(record.invocation.clone());
        }
        assert_eq!(
            invocations.len(),
            5,
            "five invocations must write five distinct records: {invocations:?}"
        );

        // **Synced**, and before the create. The durability trio is counted
        // across the five invocations and each record's own two entries are
        // ordered against that container's own `create`.
        let rendered = fixture.trace.rendered();
        assert_eq!(
            rendered
                .iter()
                .filter(|entry| entry.as_str() == "durable:dir-synced:containers")
                .count(),
            5,
            "a rename is durable because the directory entry is, and there are five: {rendered:#?}"
        );
        for (name, _) in &expected {
            let at = |needle: String| {
                fixture
                    .trace
                    .position(&needle)
                    .unwrap_or_else(|| panic!("`{needle}` is not in {rendered:#?}"))
            };
            let synced = at(format!("durable:synced:{name}.intent.tmp"));
            let renamed = at(format!("durable:renamed:{name}.intent"));
            let created = at(format!("rt:create:{name}"));
            assert!(
                synced < renamed && renamed < created,
                "`{name}`: synced {synced}, renamed {renamed}, created {created}"
            );
        }
    }

    /// A container cannot be created or started except under **its own**
    /// published intent record.
    ///
    /// `PR6-CORRECTNESS-012` / `PR6-ENUM-001`.
    /// `expected_failures_refusals[6]` is "container start without an intent is
    /// **impossible by construction**", and it was impossible only by nobody
    /// having written the bypass: `create_container` and `start_container` were
    /// public, took a bare `ContainerName`, and a
    /// `ContainerRunner::start_existing(name)` added tomorrow would have
    /// compiled. They now take an `IntentWritten`, and this pins the two things
    /// the type system cannot say on its own:
    ///
    /// 1. the proof cannot be minted for a record that is not there, or that is
    ///    not a `ContainerIntent`;
    /// 2. a proof for **another** container is refused, before any effect — so
    ///    "an intent was written" cannot stand in for "this container's intent
    ///    was written".
    ///
    /// The third leg is a compile error and has no test: `start_container` has
    /// no parameter that names a container other than the proof.
    #[test]
    fn a_container_is_created_and_started_only_under_its_own_intent_record() {
        // `exit_on_start: false`, so a started container stays `Running` and the
        // control below observes the start rather than the decorator's own
        // exit.
        let fixture = Fixture::new("intent-capability", false);
        let root = fixture.private_root.clone();
        let mine =
            ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &worker_id(0)).expect("a name");
        let other =
            ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &worker_id(1)).expect("a name");

        // (1a) Absent: the proof cannot be minted at all.
        let refusal = crate::runner::container::intent::IntentWritten::certify(&root, &mine)
            .expect_err("there is no record, so there is no proof");
        assert!(
            matches!(
                &refusal,
                TactusError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
            ),
            "a missing record must refuse as a missing file: {refusal}"
        );

        // (1b) Present and not a record: still no proof. "The record could not
        // be parsed" and "the record is gone" are different answers and only
        // one of them is an absence.
        std::fs::create_dir_all(crate::runner::container::intent::containers_dir(&root))
            .expect("the namespace");
        std::fs::write(mine.intent_path(&root), b"{\"not\":\"an intent\"}")
            .expect("a malformed record");
        let refusal = crate::runner::container::intent::IntentWritten::certify(&root, &mine)
            .expect_err("a malformed record is not evidence");
        assert!(matches!(refusal, TactusError::Refused { .. }), "{refusal}");

        // The control: a real record certifies, so (1a) and (1b) are about the
        // record and not about `certify` never succeeding.
        let mut hooks = RecordingHooks::new(fixture.trace.clone());
        let plan = fixture
            .runner()
            .plan(&worker_request(
                ShellKind::Sh.spec("exit 0"),
                fixture.task_a.clone(),
                AgentId::new("claude-code"),
                Duration::from_secs(10),
                worker_id(0),
            ))
            .expect("plans");
        let proof = crate::runner::container::write_intent(
            &mut hooks,
            ContainerSite::WriteIntent,
            &root,
            &mine,
            &plan.launch.intent,
        )
        .expect("the record publishes and certifies");
        assert_eq!(proof.name(), &mine);

        // (2) A proof for another container is refused, before any effect.
        fixture.trace.clear();
        let mut spec = plan.launch.spec.clone();
        spec.name = other.as_str().to_owned();
        let refusal = crate::runner::container::create_container(
            &mut hooks,
            ContainerSite::Create,
            &fixture.runtime,
            &proof,
            &spec,
        )
        .expect_err("`other` has no record of its own");
        let message = refusal.to_string();
        assert!(message.contains(other.as_str()), "{message}");
        assert!(message.contains(mine.as_str()), "{message}");
        assert!(
            message.contains("expected_failures_refusals[6]"),
            "{message}"
        );
        assert_eq!(
            fixture.trace.rendered(),
            Vec::<String>::new(),
            "the mismatch refused and something still happened: {:#?}",
            fixture.trace.rendered()
        );
        assert!(fixture.runtime.fake().container_names().is_empty());

        // The control: the same call with the matching proof creates, so the
        // refusal above is about the name and not about the spec.
        crate::runner::container::create_container(
            &mut hooks,
            ContainerSite::Create,
            &fixture.runtime,
            &proof,
            &plan.launch.spec,
        )
        .expect("its own proof creates");
        assert_eq!(
            fixture.runtime.fake().container_names(),
            vec![mine.as_str().to_owned()]
        );
        crate::runner::container::start_container(
            &mut hooks,
            ContainerSite::Start,
            &fixture.runtime,
            &proof,
        )
        .expect("and starts the container the proof names");
        assert_eq!(
            fixture
                .runtime
                .fake()
                .container(mine.as_str())
                .map(|held| held.state),
            Some(Liveness::Running)
        );
    }

    /// A launch that fails at **any** step releases everything it reached, and
    /// answers with the original cause.
    ///
    /// `PR6-CORRECTNESS-003` / `PR6-ENUM-003`. There are four ways out of
    /// `ContainerRunner::launch` before a `Launched` value exists, and until
    /// repair R1 three of them returned through `?` with nothing to release —
    /// so `Runner::run`'s own release never ran and the container, the view and
    /// the intent all survived. The fourth, the reported-image-id mismatch,
    /// released **fail-fast**: a failing `Container.Stop` skipped the rm, the
    /// view and the intent *and masked the integrity refusal*.
    ///
    /// The grid is {failure point} × {cleanup healthy, `Container.Stop`
    /// failing} — the intersection, because a cleanup that runs and a cleanup
    /// that runs *after an earlier step failed* are different claims. In every
    /// cell: the error names the original cause, and R19/R26 balance.
    ///
    /// Second field held constant: the request, the role and the recorded image
    /// id, which match in every cell except the one whose subject is a
    /// mismatch.
    #[test]
    fn a_launch_that_fails_at_any_step_releases_everything_it_reached() {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Where {
            View,
            Create,
            Mismatch,
            Start,
        }
        let points: &[(&str, Where, &str)] = &[
            (
                "the view cannot be materialised",
                Where::View,
                "VIEW-REFUSED",
            ),
            ("`docker create` fails", Where::Create, "armed failing"),
            ("the reported image id differs", Where::Mismatch, "INV-23"),
            ("`docker start` fails", Where::Start, "armed failing"),
        ];

        let mut cells = 0_usize;
        let mut with_residue = 0_usize;
        for (tag, point, marker) in points {
            for stop_fails in [false, true] {
                let fixture = Fixture::new(
                    &format!(
                        "atomic-{}-{stop_fails}",
                        tag.replace(|c: char| !c.is_ascii_alphanumeric(), "-")
                    ),
                    true,
                );
                let request = worker_request(
                    ShellKind::Sh.spec("exit 0"),
                    fixture.task_a.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(10),
                    worker_id(0),
                );
                let name = ContainerName::new(REPO_KEY, RUN_ID, INCARNATION_1, &request.invocation)
                    .expect("a name");
                let mut runner = fixture.runner();
                match point {
                    Where::View => runner = runner.with_view(Box::new(FailingView)),
                    Where::Create => fixture.runtime.fake().set_failing(RuntimeOp::Create),
                    Where::Mismatch => fixture
                        .runtime
                        .fake()
                        .substitute_reported_image_id(name.as_str(), OTHER_IMAGE_ID),
                    Where::Start => fixture.runtime.fake().set_failing(RuntimeOp::Start),
                }
                if stop_fails {
                    fixture.runtime.fake().set_failing(RuntimeOp::Stop);
                }

                let refusal = runner.run(&request).expect_err("the launch fails");
                let message = refusal.to_string();
                // (1) The cause, never the cleanup's own failure.
                assert!(
                    message.contains(marker),
                    "{tag} (stop_fails: {stop_fails}): the original cause was masked: {message}"
                );

                // (2) R26 and R19 balance: nothing survives, whichever step
                // failed. A failing `Stop` does not stop the rm — `docker rm
                // --force` removes a running container — so the ledgers still
                // balance and the residue clause is the honest record of which
                // step could not be taken.
                assert!(
                    fixture.runtime.fake().container_names().is_empty(),
                    "{tag} (stop_fails: {stop_fails}): a container survived"
                );
                assert!(
                    list_intents(&fixture.private_root)
                        .expect("scan")
                        .is_empty(),
                    "{tag} (stop_fails: {stop_fails}): an intent survived"
                );
                assert!(
                    !fixture
                        .private_root
                        .join("views")
                        .join(name.as_str())
                        .exists(),
                    "{tag} (stop_fails: {stop_fails}): a view survived"
                );

                // (3) The container was never started, except in the cell whose
                // subject is a failing start.
                if *point != Where::Start {
                    assert!(
                        !fixture.trace.ops().contains(&RuntimeOp::Start),
                        "{tag}: the container was started"
                    );
                }

                // (4) A failing `Stop` is *named* rather than swallowed, at
                // every exit where the cancel attempts one.
                //
                // That is every exit **past** the create call, including the
                // one where the create itself failed (`PR6-ACCT-003`): the
                // funnel runs its primitive before consulting the `After`
                // phase, and `DockerCli::create` reads the reported image id
                // back afterwards, so a `Container.Create` that returns `Err`
                // may have left a container. The cancel cannot tell, so it
                // attempts the stop and the removal — both tolerant of
                // already-gone against a real daemon — and reports whatever the
                // runtime answered. This fake is armed to fail *every* stop,
                // including one against a container that was never created,
                // which is why the Create cell now names a stop failure too.
                let attempts_stop = matches!(point, Where::Create | Where::Mismatch | Where::Start);
                let expects_residue = stop_fails && attempts_stop;
                assert_eq!(
                    message.contains("could not be stopped"),
                    expects_residue,
                    "{tag} (stop_fails: {stop_fails}): {message}"
                );
                if expects_residue {
                    // And every later step was still attempted: the remove, the
                    // view and the intent all have their sites in the trace
                    // after the stop that failed.
                    let sites: Vec<String> = fixture
                        .trace
                        .rendered()
                        .into_iter()
                        .filter(|entry| entry.starts_with("site:"))
                        .collect();
                    for later in [
                        "site:Remove:before",
                        "site:UnmountGitView:before",
                        "site:RemoveIntent:before",
                    ] {
                        assert!(
                            sites.contains(&later.to_owned()),
                            "{tag}: `{later}` was skipped after the stop failed: {sites:#?}"
                        );
                    }
                    with_residue += 1;
                }
                cells += 1;
            }
        }
        assert_eq!(
            cells, 8,
            "four failure points crossed with two cleanup states"
        );
        assert_eq!(
            with_residue, 3,
            "three of the cells reach the cancel with a container that may exist"
        );
    }

    /// The working directory is the **runner's**, for every role, and never the
    /// image's.
    ///
    /// `PR6-CORRECTNESS-006`, the half `CreateSpec` can carry. A probe's
    /// `workdir` was `None`, which hands the working directory to the image's
    /// `WORKDIR` — so what a probe certified depended on a value the runner had
    /// not read, and the finding's mutation (`None` -> `Some("/")`) changed
    /// nothing any test could see. Both values are pinned here, so the mutation
    /// dies in either direction.
    #[test]
    fn the_working_directory_is_the_runners_own_for_every_role() {
        let fixture = Fixture::new("workdir", true);
        let runner = fixture.runner();
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for request in requests(&fixture.task_a) {
            let plan = runner.plan(&request).expect("plans");
            let workdir = plan.launch.spec.workdir.clone().unwrap_or_else(|| {
                panic!("{}: the image chose the working directory", request.role)
            });
            seen.insert(request.role.label(), workdir);
        }
        assert_eq!(
            seen,
            BTreeMap::from([
                ("implement".to_owned(), "/tactus/workspace".to_owned()),
                ("gate".to_owned(), "/tactus/workspace".to_owned()),
                ("review".to_owned(), "/tactus/workspace".to_owned()),
                ("probe(shell)".to_owned(), "/tmp".to_owned()),
                ("probe(claude-code)".to_owned(), "/tmp".to_owned()),
            ]),
            "the working directory rule moved"
        );
        // Both values are declared mounts, so no role runs in a directory the
        // runner did not give it.
        for request in requests(&fixture.task_a) {
            let plan = runner.plan(&request).expect("plans");
            let workdir = plan.launch.spec.workdir.clone().expect("pinned");
            assert!(
                plan.mounts().iter().any(|mount| mount.target() == workdir),
                "{}: the working directory `{workdir}` is not a declared mount",
                request.role
            );
        }
    }

    /// A `PATH` that resolves against the working directory is refused, for
    /// every role, before any effect.
    ///
    /// `PR6-CORRECTNESS-006`, the half that matters. A probe has no worktree and
    /// an attempt has one, so their working directories differ **by design**;
    /// with a relative `PATH` component the repository's own worktree is on the
    /// executable search path and repository content decides which `claude` the
    /// attempt runs while pre-flight certified another. Both refusals are here
    /// — the relative component, and the empty base that was the production
    /// default and supplied no `PATH` at all.
    ///
    /// Second field held constant: the workspace, the image and the run; what
    /// varies is the base and the role.
    #[test]
    fn a_cwd_relative_or_absent_path_refuses_every_role_before_any_effect() {
        let fixture = Fixture::new("hostile-path", true);
        let cases: Vec<(&str, ContainerEnvironment, &str)> = vec![
            (
                "a relative component",
                ContainerEnvironment::from_image(vec![(
                    "PATH".to_owned(),
                    CWD_RELATIVE_PATH.to_owned(),
                )]),
                "DESIGN.md:612",
            ),
            (
                "no PATH at all — the production default",
                ContainerEnvironment::inherited(),
                "names no `PATH`",
            ),
        ];
        let mut refused = 0_usize;
        for (tag, environment, expected) in cases {
            let runner = ContainerRunner::new(
                container_policy(),
                fixture.identity.clone(),
                &fixture.repo,
                environment,
                Box::new(fixture.runtime.clone()),
            )
            .expect("a container policy");
            for request in requests(&fixture.task_a) {
                let refusal = runner
                    .plan(&request)
                    .expect_err("an environment that cannot certify resolution is refused");
                assert!(
                    refusal.to_string().contains(expected),
                    "{tag} / {}: {refusal}",
                    request.role
                );
                refused += 1;
            }
        }
        assert_eq!(refused, 2 * 5, "two hostile bases crossed with five roles");

        // Nothing was created on the way to any of them: the refusal is in
        // `plan`, which performs no effect.
        assert!(
            list_intents(&fixture.private_root)
                .expect("scan")
                .is_empty()
        );
        assert!(fixture.runtime.fake().container_names().is_empty());

        // The control: the same runner over an absolute-only base plans and
        // supplies that value to every role, probe and attempt alike — which is
        // the property the refusals exist to protect.
        let runner = fixture.runner();
        let mut supplied: BTreeSet<String> = BTreeSet::new();
        for request in requests(&fixture.task_a) {
            let plan = runner.plan(&request).expect("plans");
            supplied.insert(
                plan.env()
                    .iter()
                    .find(|(key, _)| key == "PATH")
                    .map(|(_, value)| value.clone())
                    .expect("the runner supplies PATH"),
            );
        }
        assert_eq!(
            supplied,
            BTreeSet::from([IMAGE_PATH.to_owned()]),
            "the five roles do not execute under one PATH"
        );
    }

    /// Every role's container gets a **read-only root** and exactly one
    /// ephemeral scratch mount, and no other writable surface without a host
    /// source.
    ///
    /// `PR6-CORRECTNESS-008` / `PR6-ENUM-005`, the half a machine with no
    /// container runtime can assert — which is the Windows guest, and which is
    /// why it is here as well as in the gated test that runs the write.
    #[test]
    fn every_role_gets_a_read_only_root_and_one_ephemeral_scratch_mount() {
        let fixture = Fixture::new("read-only-root", true);
        let runner = fixture.runner();
        let mut rows = 0_usize;
        for request in requests(&fixture.task_a) {
            let plan = runner.plan(&request).expect("plans");
            assert!(
                plan.launch.spec.read_only_root,
                "{}: the container layer is writable, so a write outside every declared \
                 mount would succeed",
                request.role
            );
            let scratch: Vec<Mount> = plan
                .mounts()
                .iter()
                .filter(|mount| matches!(mount, Mount::Tmpfs { .. }))
                .cloned()
                .collect();
            assert_eq!(
                scratch,
                vec![Mount::Tmpfs {
                    target: "/tmp".to_owned()
                }],
                "{}: {:?}",
                request.role,
                plan.mounts()
            );
            // Every other mount has a source the coordinator can name — a host
            // path or an operator-owned volume — so the mount list is the whole
            // of what the container may write and none of it is anonymous.
            for mount in plan.mounts() {
                let declared = match mount {
                    Mount::Path { .. } | Mount::Volume { .. } => true,
                    Mount::Tmpfs { target } => target == "/tmp",
                };
                assert!(declared, "{}: {mount:?}", request.role);
            }
            rows += 1;
        }
        assert_eq!(rows, 5);
    }

    // -----------------------------------------------------------------------
    // 6. Docker-gated: what the fake cannot prove
    // -----------------------------------------------------------------------

    /// The references the gated tests prefer, in order.
    ///
    /// **These tests never pull.** `non_goals[1]` is "implicit image pull", and
    /// a fixture that pulled would exercise the behaviour the slice forbids on
    /// the very runtime the refusal is meant to be proven against. So the image
    /// is *discovered* among what the machine already holds. `tactus-test/git:v1`
    /// is first because it is the only local image carrying both a shell and
    /// `git`, and because its `TACTUS_IMAGE_MARKER` is how "the container runner
    /// starts from the **image** environment" is measured rather than asserted.
    const PREFERRED_IMAGES: &[&str] = &[
        "tactus-test/git:v1",
        "alpine:3.20",
        "busybox:latest",
        "debian:stable-slim",
    ];

    /// Images that carry `git`. A subset, named separately because the
    /// Git-view proof needs one and the others do not.
    ///
    /// **One entry, and `alpine/git` is deliberately not the second.** That
    /// image declares `VOLUME /git`, so every container created from it leaves
    /// an anonymous volume behind that `docker rm --force` does not remove —
    /// measured here, 29 of them from one run of this suite, which is
    /// `PR6A-ANONYMOUS-VOLUMES-LEAK`. A fallback that breaks
    /// `DOCKER-SUBSTRATE.md`'s "leave the daemon as you found it" on somebody
    /// else's machine is worse than a loud, counted absence.
    const GIT_IMAGES: &[&str] = &["tactus-test/git:v1"];

    /// The image whose environment carries a marker this suite can recognise.
    const MARKER_IMAGE: &str = "tactus-test/git:v1";
    const IMAGE_MARKER_VALUE: &str = "image-environment-v1";

    /// The image whose **own environment** sets credential-location variables.
    ///
    /// `PR6-CORRECTNESS-007` cannot be measured against an image that sets
    /// none: the defect is that `docker create --env` overlays the image
    /// environment, so a key the runner omits is a key the image supplies.
    /// `DOCKER-SUBSTRATE.md` records how it is built, from a base the machine
    /// already holds and with no network.
    const CREDENTIAL_ENV_IMAGE: &str = "tactus-test/credenv:v1";

    /// The image variable that is **not** a credential location, so "the
    /// withheld keys were overridden" is distinguishable from "the image
    /// environment was wiped".
    const CREDENTIAL_ENV_CONTROL: (&str, &str) = ("GH_CONFIG_DIR", "/image/gh");

    /// What a Docker-gated test does when there is no runtime.
    ///
    /// It **reads** the reason rather than returning silently, so a skip that
    /// stopped saying why would not compile.
    fn skipped(reason: &str) {
        assert_eq!(
            reason,
            crate::runner::container::fake::absent_reason(),
            "a Docker-gated test skipped for a reason the gate does not know about"
        );
    }

    /// What a Docker-gated test does when the runtime holds no usable image.
    ///
    /// Loud under the same variable as a missing runtime: a machine with Docker
    /// and no image would otherwise pass these tests without touching it.
    fn no_image(reason: &str) {
        assert!(reason.contains("never pull"), "{reason}");
        assert!(
            std::env::var_os(crate::runner::container::fake::REQUIRE_DOCKER).is_none(),
            "{} is set and a gated test found no usable image: {reason}",
            crate::runner::container::fake::REQUIRE_DOCKER
        );
    }

    /// A reference the runtime holds, with its id, or the reason there is none.
    fn discover(
        docker: &dyn ContainerRuntime,
        preferred: &[&str],
    ) -> Result<(String, String), String> {
        for reference in preferred {
            if let Ok(Some(found)) = docker.image_by_reference(reference) {
                return Ok(((*reference).to_owned(), found.id));
            }
        }
        Err(format!(
            "the container runtime holds none of {preferred:?} and these tests never pull \
             (non_goals[1])"
        ))
    }

    /// A container policy naming a real image id, and no credential volumes.
    ///
    /// R20 volumes are **operator-owned** and `persistent_output`; a test that
    /// created one would be creating operator state on the machine it runs on,
    /// which is the very thing the row forbids a run from doing. So the gated
    /// suite records none, and `a_credential_volume_is_never_created_or_pruned_by_any_disposition`
    /// carries the volume obligation against the fake.
    fn real_policy(image_id: &str) -> RunnerPolicy {
        RunnerPolicy {
            kind: RunnerKind::Container,
            policy: RunnerContract::ContainerV1,
            image: Some(ImageIdentity {
                reference: "discovered-locally".to_owned(),
                id: image_id.to_owned(),
                digest: None,
            }),
            credential_volumes: None,
        }
    }

    /// The **reserved** part of the recorded image's own environment, read from
    /// the daemon.
    ///
    /// DESIGN.md:259-260: "the container runner [starts] from the image
    /// environment; each supplies role-scoped `HOME`, `PATH`, and credential
    /// locations". A gated fixture is the one place in this suite that can
    /// honour the first clause literally, because it has a real image to read;
    /// the fake fixtures state an equivalent base as a literal. Either way the
    /// runner is given a base carrying an absolute-only `PATH`, which
    /// `ContainerEnvironment::certify_path` now requires.
    ///
    /// **Filtered to the reserved keys**, and that is the point rather than an
    /// economy: `docker create --env` *overlays* the image environment, so a
    /// variable the runner does not name still reaches the child. Passing the
    /// whole image environment back would restate every one of those in the
    /// runner's own `--env` list and make
    /// "overlays a runner-owned base rather than replacing it" unmeasurable —
    /// the marker assertion below would be comparing the fixture with itself.
    ///
    /// **Not a self-oracle.** This reads the *image's* declared environment out
    /// of the daemon; the assertions are about what a process *inside a
    /// container* sees and resolves, which a different mechanism decides.
    fn image_environment_of(
        docker: &crate::runner::container::DockerCli,
        image_id: &str,
    ) -> ContainerEnvironment {
        let raw = docker
            .raw(
                RuntimeOp::InspectImageById,
                image_id,
                &[
                    "image",
                    "inspect",
                    image_id,
                    "--format",
                    "{{join .Config.Env \"\u{1f}\"}}",
                ],
            )
            .expect("the image's environment");
        let declared: Vec<(String, String)> = raw
            .trim_end_matches('\n')
            .split('\u{1f}')
            .filter_map(|entry| entry.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        assert!(
            declared.iter().any(|(key, _)| key == "PATH"),
            "the discovered image declares no PATH, so nothing below measures \
             resolution: {declared:?}"
        );
        let reserved = host::reserved_keys();
        ContainerEnvironment::from_image(
            declared
                .into_iter()
                .filter(|(key, _)| reserved.iter().any(|name| name == key))
                .collect(),
        )
    }

    /// A `RunIdentity` for a gated test, under a scratch private root.
    ///
    /// `run_id` is a **parameter** because the container name is
    /// `tactus-<repo_key>-<run_id>-<incarnation>-<invocation-hash>` and carries
    /// no private root: two gated tests sharing a run id and an invocation
    /// ordinal produce the same container name, and `cargo test` runs them
    /// concurrently. Measured: the first version of this suite failed with
    /// `Conflict. The container name ... is already in use`. In production the
    /// run id is a ULID and the collision cannot arise; in a fixture it is
    /// whatever the fixture writes, which is why it is written per test.
    fn real_identity(root: &Path, repo: &Path, run_id: &str) -> RunIdentity {
        let private_root = root.join("private");
        let paths = RunPaths::with_private_root(repo, run_id, &private_root);
        paths.create().expect("the run's two halves");
        std::fs::write(paths.events(), format!("{EVENT_LOG_MARKER}\n")).expect("the public log");
        std::fs::write(
            paths.transcripts().join("k0-a1.md"),
            "PRIVATE-TRANSCRIPT-a5f2\n",
        )
        .expect("a private artifact");
        RunIdentity {
            private_root,
            run_id: run_id.to_owned(),
            run_dir: paths.public,
            incarnation: INCARNATION_1.to_owned(),
            repo_key: REPO_KEY.to_owned(),
        }
    }

    /// One run id per gated test. Distinct by construction, and asserted so.
    const GATED_RUNS: &[(&str, &str)] = &[
        ("env", "01KZGATEDA000000000000000A"),
        ("readonly", "01KZGATEDB000000000000000B"),
        ("confine", "01KZGATEDC000000000000000C"),
        ("gitview", "01KZGATEDD000000000000000D"),
        ("parity", "01KZGATEDE000000000000000E"),
        // Repair R1. The ids carry the round rather than continuing the
        // `…GATED<letter>` sequence, and that is not cosmetic: a container name
        // is `tactus-<repo_key>-<run_id>-<incarnation>-<invocation-hash>` and
        // carries no worktree, so two *trees* whose gated suites pick the same
        // run id and invocation ordinal fight over one container name on a
        // shared daemon. Measured: repair round R3 added a gated test with
        // `01KZGATEDG000000000000000G` at the same time this one did, and the
        // two collided with `Conflict. The container name … is already in use`.
        ("outside", "01KZR1GATED000000000000001"),
        ("daemonspec", "01KZR1GATED000000000000002"),
        ("shadow", "01KZR1GATED000000000000003"),
        // Repair R3b, with its own round prefix for the reason above.
        ("credenv", "01KZR3BGATED00000000000001"),
        ("descendant", "01KZR3BGATED00000000000002"),
    ];

    fn gated_run(tag: &str) -> &'static str {
        GATED_RUNS
            .iter()
            .find(|(name, _)| *name == tag)
            .map(|(_, run)| *run)
            .unwrap_or_else(|| panic!("`{tag}` has no gated run id"))
    }

    /// Leave the daemon as we found it, even if an assertion panics.
    ///
    /// `DOCKER-SUBSTRATE.md`'s first rule. `Drop` rather than a line at the end
    /// of each test, because the line at the end of a test does not run when
    /// the test fails — which is exactly when a container is most likely to be
    /// left behind.
    struct LeaveNoResidue {
        docker: crate::runner::container::DockerCli,
        private_root: PathBuf,
    }

    impl Drop for LeaveNoResidue {
        fn drop(&mut self) {
            let label = crate::runner::container::intent::private_root_label(&self.private_root);
            let Ok(found) = self
                .docker
                .containers_with_label(LABEL_PRIVATE_ROOT, &label)
            else {
                return;
            };
            for container in found {
                let Ok(name) = ContainerName::rebuild(&container.name) else {
                    continue;
                };
                let mut hooks = crate::runner::container::NoHooks;
                let view = crate::runner::container::DisposableDirView::default();
                let _ = crate::runner::container::reclaim(
                    &mut hooks,
                    &self.docker,
                    &view,
                    &self.private_root,
                    &name,
                    Some(&view_dir(&self.private_root, &name)),
                );
            }
        }
    }

    /// The container runner executes the recorded image id, composes **over**
    /// the image environment, and runs in the role's worktree.
    ///
    /// Three separately droppable claims against the real runtime:
    ///
    /// * the runner **supplies** `PATH` — DESIGN.md:260, "each supplies
    ///   role-scoped `HOME`, `PATH`, and credential locations" — and the value
    ///   the child sees is the one the runner named. A key the runner did *not*
    ///   name and the image did (`TACTUS_IMAGE_MARKER`) reaches the child
    ///   anyway, which is what "overlays a runner-owned base rather than
    ///   replacing it" means and is the half a `PATH` assertion alone cannot
    ///   make.
    /// * the adapter's overlay key lands.
    /// * the working directory is the role's worktree mount.
    ///
    /// **The first claim used to be its opposite** — "the composed
    /// `CreateSpec.env` does not name `PATH`" — and repair R1 inverted it
    /// deliberately, not to make anything pass. `ContainerEnvironment::inherited`
    /// composed an empty base, so the *image's* `PATH` decided which binary a
    /// bare program name resolved to; with a relative component in it, a probe
    /// (no worktree) and the attempt it certifies (a worktree) resolve
    /// different binaries. `PR6-CORRECTNESS-006`. The old assertion was true
    /// and was a statement that the runner supplied nothing.
    ///
    /// Second field held constant: the role and the workspace; what varies
    /// across the three claims is which part of the environment is read.
    #[test]
    fn real_docker_runs_from_the_recorded_image_id_and_composes_over_the_image_environment() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_runs_from_the_recorded_image_id_and_composes_over_the_image_environment",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (reference, image_id) = match discover(docker.as_ref(), PREFERRED_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-env");
        let run_id = gated_run("env");
        let repo_dir = root.join("repo");
        repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, run_id);
        let workspace = root.join("plain-workspace");
        std::fs::create_dir_all(&workspace).expect("a workspace");
        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };

        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_hooks(Box::new(RecordingHooks::new(trace.clone())))
        .with_poll(Duration::from_millis(10));

        let request = gate_request(
            ShellKind::Sh
                .spec(
                    "printf 'PATH=%s\\n' \"$PATH\"; \
                 printf 'OVERLAY=%s\\n' \"$TACTUS_OVERLAY\"; \
                 printf 'MARKER=%s\\n' \"$TACTUS_IMAGE_MARKER\"; \
                 printf 'PWD=%s\\n' \"$(pwd)\"",
                )
                .env("TACTUS_OVERLAY", "landed"),
            workspace.clone(),
            Duration::from_secs(60),
            gate_id(0),
        );

        // The runner supplies `PATH`, and every component of it is absolute.
        let plan = runner.plan(&request).expect("plans");
        let supplied_path = plan
            .env()
            .iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| value.clone())
            .expect("the runner supplies PATH (DESIGN.md:260)");
        assert!(
            super::super::env::cwd_dependent_path_components(&supplied_path).is_empty(),
            "the runner supplied a working-directory-relative PATH: {supplied_path}"
        );
        // And it did not *replace* the image environment: the marker key is one
        // the runner never names, and it is read back from inside the container
        // below. Without this the `PATH` assertion above would be consistent
        // with a runner that had thrown the image environment away.
        assert!(
            !plan
                .env()
                .iter()
                .any(|(key, _)| key == "TACTUS_IMAGE_MARKER"),
            "the runner named the image's own key, so the overlay claim below is vacuous: {:?}",
            plan.env()
        );
        assert_eq!(plan.launch.spec.image_id, image_id);

        let output = runner.run(&request).expect("runs");
        assert_eq!(output.code, Some(0), "stderr: {}", output.stderr);
        let line = |key: &str| -> String {
            output
                .stdout
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap_or_else(|| panic!("`{key}` is not in {:?}", output.stdout))
                .to_owned()
        };
        assert_eq!(
            line("PATH="),
            supplied_path,
            "the child ran under a PATH the runner did not supply, so pre-flight cannot \
             certify which binary an attempt resolves (DESIGN.md:612)"
        );
        assert_eq!(line("OVERLAY="), "landed", "the overlay did not land");
        assert_eq!(line("PWD="), "/tactus/workspace");
        if reference == MARKER_IMAGE {
            assert_eq!(
                line("MARKER="),
                IMAGE_MARKER_VALUE,
                "the marker image's own environment did not survive composition"
            );
        }

        // R26 and R19 balance against the real daemon.
        assert_eq!(
            docker
                .containers_with_label(
                    LABEL_PRIVATE_ROOT,
                    &crate::runner::container::intent::private_root_label(&identity.private_root)
                )
                .expect("reachable")
                .len(),
            0
        );
        assert!(
            list_intents(&identity.private_root)
                .expect("scan")
                .is_empty()
        );
    }

    /// `expected_failures_refusals`: "**reviewer write attempt fails**".
    ///
    /// DESIGN.md:610: "a `:ro` mount makes the reviewer's read-only
    /// *mechanically* perfect instead of flag-deep". The control is the same
    /// command in the `Implement` role over the same workspace: it writes, and
    /// the file appears on the host. Without it, a test in which nothing could
    /// write would pass.
    ///
    /// Second field held constant: the command, the image and the workspace;
    /// what varies is the role.
    #[test]
    fn real_docker_refuses_a_reviewer_write_to_its_read_only_mount() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_refuses_a_reviewer_write_to_its_read_only_mount",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), PREFERRED_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-ro");
        let run_id = gated_run("readonly");
        let repo_dir = root.join("repo");
        repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, run_id);
        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };
        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_poll(Duration::from_millis(10));

        let mut outcomes = Vec::new();
        for (tag, role_is_review, ordinal) in [("review", true, 0_u32), ("implement", false, 1)] {
            let workspace = root.join(format!("ws-{tag}"));
            std::fs::create_dir_all(&workspace).expect("a workspace");
            // The redirection is captured **inside** the container, because
            // `DockerCli::collect` returns only what `docker logs` wrote to its
            // own stdout and discards the container's stderr entirely
            // (`PR6A-DOCKERCLI-MERGES-STDERR-INTO-STDOUT`). Measured on docker
            // 29.7.2: `docker logs` really does separate the two streams, so
            // that is a repairable defect in the CLI adapter rather than a
            // property of the runtime — and this test does not depend on
            // either way.
            let spec =
                ShellKind::Sh.spec("( echo tactus-wrote-this > /tactus/workspace/probe.txt ) 2>&1");
            let request = if role_is_review {
                review_request(
                    spec,
                    workspace.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(60),
                    InvocationId::attempt(
                        TaskKey(0),
                        GenerationId(0),
                        AttemptNumber(1),
                        AttemptRole::ReviewPass(0),
                        ordinal,
                    ),
                )
            } else {
                worker_request(
                    spec,
                    workspace.clone(),
                    AgentId::new("claude-code"),
                    Duration::from_secs(60),
                    worker_id(ordinal),
                )
            };
            let plan = runner.plan(&request).expect("plans");
            assert_eq!(
                target_of(plan.mounts(), "/tactus/workspace").map(Mount::read_only),
                Some(role_is_review),
                "{tag}: the mount disposition"
            );
            let output = runner.run(&request).expect("runs");
            let wrote = workspace.join("probe.txt").exists();
            // Both streams, because `DockerCli::collect` merges the container's
            // stderr into its stdout — `docker logs` interleaves them on a
            // container without a TTY. Recorded as
            // `PR6A-DOCKERCLI-MERGES-STDERR-INTO-STDOUT`; the assertion is
            // written so it holds either way rather than pinning the residual.
            outcomes.push((
                tag,
                output.code,
                wrote,
                format!("{}{}", output.stdout, output.stderr),
            ));
        }

        let (_, review_code, review_wrote, review_output) = &outcomes[0];
        assert_ne!(*review_code, Some(0), "the reviewer's write succeeded");
        assert!(
            review_output.to_ascii_lowercase().contains("read-only"),
            "the failure is not the read-only mount: {review_output}"
        );
        assert!(!review_wrote, "the reviewer wrote into the workspace");

        let (_, implement_code, implement_wrote, _) = &outcomes[1];
        assert_eq!(
            *implement_code,
            Some(0),
            "the control could not write either, so the test above proves nothing"
        );
        assert!(*implement_wrote);
        assert_eq!(
            std::fs::read_to_string(root.join("ws-implement").join("probe.txt"))
                .expect("the control's file")
                .trim(),
            "tactus-wrote-this"
        );
    }

    /// `expected_failures_refusals`: "**gate write outside mount fails**", and
    /// DESIGN.md:400's whole sentence.
    ///
    /// Repository-controlled gate code — "which no agent permission surface can
    /// ever bound" (DESIGN.md:610) — is given every withheld path by absolute
    /// name and asked to read it and to write it. The assertions are on the
    /// **host**, because that is what the claim is about: a container is free
    /// to create whatever it likes inside its own writable layer, and none of
    /// it may reach the coordinator.
    ///
    /// The control is in the same command: the gate reads its own workspace,
    /// which it *can* see. A test in which the container could read nothing at
    /// all would pass without the confinement doing anything.
    #[test]
    fn real_docker_confines_a_gate_to_its_mount() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate("real_docker_confines_a_gate_to_its_mount", trace.clone()) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), PREFERRED_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-confine");
        let run_id = gated_run("confine");
        let repo_dir = root.join("repo");
        let (head, _) = repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, run_id);
        let paths = RunPaths::with_private_root(&repo_dir, run_id, &identity.private_root);
        let execution_root =
            crate::workspace_manager::execution_root_of(&identity.private_root, REPO_KEY, run_id);
        let mine = execution_root.join("tasks").join("kalpha-g0");
        let sibling = execution_root.join("tasks").join("kbeta-g0");
        repo::worktree(&repo_dir, &mine, &head);
        repo::worktree(&repo_dir, &sibling, &head);
        std::fs::write(sibling.join("sibling.txt"), "SIBLING-WORKTREE-a5f2\n")
            .expect("a sibling file");
        std::fs::write(mine.join("mine.txt"), "MY-OWN-WORKTREE-a5f2\n").expect("my file");

        let withheld: Vec<(Withheld, PathBuf)> = vec![
            (Withheld::PublicLog, paths.events()),
            (
                Withheld::PrivateArtifacts,
                paths.transcripts().join("k0-a1.md"),
            ),
            (Withheld::SiblingWorktree, sibling.join("sibling.txt")),
            (
                Withheld::AuthoritativeGit,
                repo_dir.join(".git").join("HEAD"),
            ),
        ];
        let before: Vec<Vec<u8>> = withheld
            .iter()
            .map(|(_, path)| std::fs::read(path).expect("a withheld file"))
            .collect();

        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };
        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_poll(Duration::from_millis(10));

        let mut script = String::from("cat /tactus/workspace/mine.txt;");
        for (_, path) in &withheld {
            let path = path.to_string_lossy().replace('\\', "/");
            script.push_str(&format!(
                " printf 'READ {path}: '; cat '{path}' 2>&1 | head -1; \
                 printf 'WRITE {path}: '; \
                 ( mkdir -p \"$(dirname '{path}')\" && \
                   echo tactus-container-wrote-this > '{path}' ) 2>&1 && echo WROTE || echo FAILED;"
            ));
        }
        let request = gate_request(
            ShellKind::Sh.spec(&script),
            mine.clone(),
            Duration::from_secs(60),
            gate_id(0),
        );
        let output = runner.run(&request).expect("runs");

        // The control: the gate can read its own worktree.
        assert!(
            output.stdout.contains("MY-OWN-WORKTREE-a5f2"),
            "the gate could not read its own workspace, so nothing here is measured: {:?}",
            output.stdout
        );
        // And it saw none of the withheld content.
        for marker in [
            EVENT_LOG_MARKER,
            "PRIVATE-TRANSCRIPT-a5f2",
            "SIBLING-WORKTREE-a5f2",
        ] {
            assert!(
                !output.stdout.contains(marker),
                "the gate read `{marker}`: {:?}",
                output.stdout
            );
        }
        // The host is byte-identical: whatever the container wrote stayed in
        // the container.
        for ((category, path), original) in withheld.iter().zip(&before) {
            assert_eq!(
                &std::fs::read(path).expect("still there"),
                original,
                "a gate changed `{}` ({})",
                path.display(),
                category.passage()
            );
        }
        // And the coordinator's Git is unmoved.
        assert_eq!(repo::git_ok(&repo_dir, &["rev-parse", "HEAD"]), head);
    }

    /// `expected_failures_refusals[5]`: "**gate write outside mount fails**" —
    /// the refusal itself, observed inside the container.
    ///
    /// `PR6-CORRECTNESS-008` / `PR6-ENUM-005`, and the distinction that produced
    /// them. `real_docker_confines_a_gate_to_its_mount` proves **"the host is
    /// unharmed"**: it explicitly permits container-layer writes and asserts on
    /// host bytes. That is true, and it is weaker than the contract's sentence —
    /// with no read-only root filesystem the gate's
    /// `printf owned >/outside-role-mount` exited **0**, and a test can prove a
    /// true, weaker statement indefinitely while the stated guarantee does not
    /// hold. So this test asserts the **write fails**, from the container's own
    /// report, and never looks at the host at all.
    ///
    /// The grid is {a path outside every declared mount} × {a declared writable
    /// mount}, and the second column is not decoration: a container in which
    /// *nothing* could be written would satisfy the first column while being
    /// unusable, so the two controls — the role's own worktree and the declared
    /// scratch surface — are what say the confinement is a boundary rather than
    /// a brick.
    ///
    /// The hostile paths are chosen to cover the three shapes a write can take:
    /// the root of the container filesystem, a directory the image itself
    /// populates, and — the interesting one — a **sibling of the role's own
    /// mount**, `/tactus/escape`, which a naive "only paths under the mount
    /// targets are writable" implementation would let through.
    #[test]
    fn real_docker_a_gate_write_outside_every_declared_mount_fails() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_a_gate_write_outside_every_declared_mount_fails",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), PREFERRED_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-outside");
        let run_id = gated_run("outside");
        let repo_dir = root.join("repo");
        let (head, _) = repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, run_id);
        let execution_root =
            crate::workspace_manager::execution_root_of(&identity.private_root, REPO_KEY, run_id);
        let mine = execution_root.join("tasks").join("kalpha-g0");
        repo::worktree(&repo_dir, &mine, &head);

        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };
        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_poll(Duration::from_millis(10));

        // Outside every declared mount, and inside the two that are writable.
        const OUTSIDE: &[&str] = &[
            "/outside-role-mount",
            "/etc/tactus-escaped",
            "/usr/local/bin/tactus-escaped",
            // A sibling of the role's own mount target.
            "/tactus/escape",
        ];
        let inside: Vec<String> = vec![
            format!("{}/written-by-the-gate", BoundaryLayout::DEFAULT_WORKSPACE),
            format!("{}/written-by-the-gate", BoundaryLayout::DEFAULT_SCRATCH),
        ];

        let mut script = String::new();
        for path in OUTSIDE
            .iter()
            .map(|p| (*p).to_owned())
            .chain(inside.clone())
        {
            script.push_str(&format!(
                "if ( printf owned > '{path}' ) 2>/dev/null; then echo 'WROTE {path}'; \
                 else echo 'FAILED {path}'; fi;"
            ));
        }
        let request = gate_request(
            ShellKind::Sh.spec(&script),
            mine.clone(),
            Duration::from_secs(60),
            gate_id(0),
        );
        // The mount set the claim is about, taken from the plan rather than
        // assumed, so "outside every declared mount" is checked against the
        // list the container is actually given.
        let plan = runner.plan(&request).expect("plans");
        let targets: Vec<&str> = plan.mounts().iter().map(Mount::target).collect();
        for path in OUTSIDE {
            assert!(
                !targets.iter().any(|target| path.starts_with(target)),
                "`{path}` is inside a declared mount, so refusing it proves nothing: {targets:?}"
            );
        }
        assert!(plan.launch.spec.read_only_root);

        let output = runner.run(&request).expect("runs");
        let said = |path: &str| -> String {
            output
                .stdout
                .lines()
                .find(|line| line.ends_with(path))
                .unwrap_or_else(|| {
                    panic!(
                        "`{path}` is in neither stream — stdout {:?} / stderr {:?}",
                        output.stdout, output.stderr
                    )
                })
                .to_owned()
        };
        for path in OUTSIDE {
            assert_eq!(
                said(path),
                format!("FAILED {path}"),
                "a gate wrote outside every declared mount, so \
                 `expected_failures_refusals[5]` does not hold: {:?}",
                output.stdout
            );
        }
        // The controls: the role's own worktree and the declared scratch
        // surface are writable, so the refusals above are a boundary and not a
        // container that can do nothing.
        for path in &inside {
            assert_eq!(said(path), format!("WROTE {path}"), "{:?}", output.stdout);
        }
        assert_eq!(
            std::fs::read_to_string(mine.join("written-by-the-gate"))
                .expect("the gate's own worktree write reached the host"),
            "owned"
        );
    }

    /// The daemon's own container carries **exactly** the spec's mounts and a
    /// read-only root.
    ///
    /// `PR6-ENUM-005`'s surviving mutation is not about the spec at all: it is
    /// "append `--mount type=bind,source=/tmp,target=/outside` directly to
    /// Docker's argv, **bypassing `CreateSpec.mounts`**". Every fake test sees
    /// the unchanged spec, and a gated test that only writes to paths it knows
    /// about never asks the daemon what the container really has. So this test
    /// asks: it reads `.Mounts` and `.HostConfig.ReadonlyRootfs` back off the
    /// created container and compares them to the plan, and an argv-appended
    /// mount is a destination the plan does not name.
    ///
    /// The container is created through the funnel and never started, which is
    /// what lets it be inspected: `Runner::run` releases on both paths, so a
    /// container that had run would be gone before anything could look at it.
    #[test]
    fn real_docker_the_daemon_holds_exactly_the_specs_mounts_and_a_read_only_root() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_the_daemon_holds_exactly_the_specs_mounts_and_a_read_only_root",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), PREFERRED_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-daemonspec");
        let run_id = gated_run("daemonspec");
        let repo_dir = root.join("repo");
        let (head, _) = repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, run_id);
        let execution_root =
            crate::workspace_manager::execution_root_of(&identity.private_root, REPO_KEY, run_id);
        let mine = execution_root.join("tasks").join("kalpha-g0");
        repo::worktree(&repo_dir, &mine, &head);

        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };
        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_poll(Duration::from_millis(10));

        let request = gate_request(
            ShellKind::Sh.spec("exit 0"),
            mine.clone(),
            Duration::from_secs(60),
            gate_id(0),
        );
        let plan = runner.plan(&request).expect("plans");
        let name = plan.launch.name.clone();

        // Write the intent, materialise the view, create — and stop there.
        // The view is the runner's own projection rather than a bare directory:
        // a linked worktree's `.git` pointer file is a bind **source** of the
        // create, so a directory-only view fails `docker create` outright.
        let mut hooks = crate::runner::container::NoHooks;
        let view = RoleGitView::new(ContainerTrace::off()).for_reader(
            BoundaryLayout::DEFAULT_GIT_VIEW,
            BoundaryLayout::DEFAULT_GIT_OBJECTS,
        );
        let written = crate::runner::container::write_intent(
            &mut hooks,
            ContainerSite::WriteIntent,
            &identity.private_root,
            &name,
            &plan.launch.intent,
        )
        .expect("the record publishes");
        crate::runner::container::mount_git_view(
            &mut hooks,
            ContainerSite::MountGitView,
            &view,
            &plan.launch.view,
        )
        .expect("the view is a bind source, so it exists before the create");
        crate::runner::container::create_container(
            &mut hooks,
            ContainerSite::Create,
            docker.as_ref(),
            &written,
            &plan.launch.spec,
        )
        .expect("created");

        let read_only = docker
            .raw(
                RuntimeOp::Observe,
                name.as_str(),
                &[
                    "container",
                    "inspect",
                    name.as_str(),
                    "--format",
                    "{{.HostConfig.ReadonlyRootfs}}",
                ],
            )
            .expect("the daemon answers");
        assert_eq!(
            read_only.trim(),
            "true",
            "the daemon gave the container a writable root filesystem"
        );

        let raw = docker
            .raw(
                RuntimeOp::Observe,
                name.as_str(),
                &[
                    "container",
                    "inspect",
                    name.as_str(),
                    "--format",
                    "{{range .Mounts}}{{.Type}}\u{1f}{{.Destination}}\u{1f}{{.RW}}\u{1e}{{end}}",
                ],
            )
            .expect("the daemon answers");
        let daemon: BTreeSet<(String, String, bool)> = raw
            .trim()
            .split('\u{1e}')
            .filter(|entry| !entry.trim().is_empty())
            .map(|entry| {
                let fields: Vec<&str> = entry.trim().split('\u{1f}').collect();
                (
                    fields.first().copied().unwrap_or_default().to_owned(),
                    fields.get(1).copied().unwrap_or_default().to_owned(),
                    fields.get(2).copied().unwrap_or_default() == "true",
                )
            })
            .collect();
        let planned: BTreeSet<(String, String, bool)> = plan
            .mounts()
            .iter()
            .map(|mount| {
                let kind = match mount {
                    Mount::Path { .. } => "bind",
                    Mount::Volume { .. } => "volume",
                    Mount::Tmpfs { .. } => "tmpfs",
                };
                (
                    kind.to_owned(),
                    mount.target().to_owned(),
                    !mount.read_only(),
                )
            })
            .collect();
        assert_eq!(
            daemon, planned,
            "the container the daemon holds is not the one `CreateSpec` describes — a mount \
             that reaches the daemon without going through `CreateSpec.mounts` is exactly \
             `PR6-ENUM-005`'s surviving mutation"
        );
        // The control: the comparison is not two empty sets.
        assert!(planned.len() >= 4, "{planned:?}");
        assert!(
            planned.contains(&("tmpfs".to_owned(), "/tmp".to_owned(), true)),
            "{planned:?}"
        );

        crate::runner::container::reclaim(
            &mut hooks,
            docker.as_ref(),
            &view,
            &identity.private_root,
            &name,
            Some(&view_dir(&identity.private_root, &name)),
        )
        .expect("reclaimed");
    }

    /// A binary planted in the worktree cannot become the CLI the attempt runs,
    /// and a probe and an attempt resolve the same name to the same thing.
    ///
    /// `PR6-CORRECTNESS-006`, end to end. DESIGN.md:612: "Probes run through
    /// that same runner, or pre-flight could certify a host CLI/version
    /// different from the one the attempt executes." A probe has no worktree
    /// and an attempt has one, so their working directories differ **by
    /// design**; with `PATH=.:/usr/bin` the attempt's `claude` is whatever the
    /// repository put in the worktree and pre-flight certified something else.
    ///
    /// Two cells, and they are the two halves of the repair:
    ///
    /// 1. an image environment whose `PATH` resolves against the working
    ///    directory is **refused before any effect** — no container is created
    ///    at all, checked against the daemon by label;
    /// 2. under the absolute-only `PATH` the runner supplies, the planted
    ///    binary is not resolvable by name in either the probe or the attempt,
    ///    and a name that *is* on the path resolves to the same absolute file
    ///    in both.
    ///
    /// The control is inside the same command: the gate proves the shim is
    /// really there and really executable in its own worktree, so "not found"
    /// is a statement about resolution and not about the fixture.
    #[test]
    fn real_docker_a_worktree_binary_cannot_shadow_the_certified_cli() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_a_worktree_binary_cannot_shadow_the_certified_cli",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), PREFERRED_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-shadow");
        let run_id = gated_run("shadow");
        let repo_dir = root.join("repo");
        let (head, _) = repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, run_id);
        let execution_root =
            crate::workspace_manager::execution_root_of(&identity.private_root, REPO_KEY, run_id);
        let mine = execution_root.join("tasks").join("kalpha-g0");
        repo::worktree(&repo_dir, &mine, &head);

        // The shim the repository controls, named as a CLI this engine drives.
        let shim = mine.join("claude");
        std::fs::write(&shim, "#!/bin/sh\necho SHIMMED\n").expect("the shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
                .expect("executable");
        }

        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };
        let label = crate::runner::container::intent::private_root_label(&identity.private_root);

        // (1) The refusal, before any effect.
        let hostile = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            ContainerEnvironment::from_image(vec![("PATH".to_owned(), format!(".:{IMAGE_PATH}"))]),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_poll(Duration::from_millis(10));
        let refusal = hostile
            .run(&gate_request(
                ShellKind::Sh.spec("claude"),
                mine.clone(),
                Duration::from_secs(60),
                gate_id(1),
            ))
            .expect_err("a cwd-relative PATH is refused");
        assert!(refusal.to_string().contains("DESIGN.md:612"), "{refusal}");
        assert_eq!(
            docker
                .containers_with_label(LABEL_PRIVATE_ROOT, &label)
                .expect("reachable")
                .len(),
            0,
            "the refusal created a container"
        );

        // (2) The guarantee, under the PATH the runner supplies.
        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_poll(Duration::from_millis(10));

        let script = "printf 'PWD=%s\\n' \"$(pwd)\"; \
             printf 'CLAUDE=%s\\n' \"$(command -v claude || echo NOTFOUND)\"; \
             printf 'SH=%s\\n' \"$(command -v sh || echo NOTFOUND)\"";
        let gate = gate_request(
            ShellKind::Sh.spec(&format!(
                "{script}; printf 'SHIM=%s\\n' \"$(test -x ./claude && echo PRESENT || echo ABSENT)\""
            )),
            mine.clone(),
            Duration::from_secs(60),
            gate_id(0),
        );
        let probe = crate::agent::probe_request(
            "claude-code",
            ShellKind::Sh.spec(script),
            0,
            Duration::from_secs(60),
        )
        .expect("an agent probe request");

        let mut answers: Vec<(&str, BTreeMap<String, String>)> = Vec::new();
        for (tag, request) in [("the attempt", gate), ("the probe", probe)] {
            let output = runner.run(&request).expect("runs");
            assert_eq!(output.code, Some(0), "{tag}: stderr {}", output.stderr);
            let mut read = BTreeMap::new();
            for line in output.stdout.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    read.insert(key.to_owned(), value.trim().to_owned());
                }
            }
            answers.push((tag, read));
        }

        // The two really do run in different working directories — the premise
        // of the finding, asserted rather than assumed.
        assert_eq!(answers[0].1["PWD"], BoundaryLayout::DEFAULT_WORKSPACE);
        assert_eq!(answers[1].1["PWD"], BoundaryLayout::DEFAULT_SCRATCH);
        assert_ne!(answers[0].1["PWD"], answers[1].1["PWD"]);
        // The control: the shim is there, and it is executable.
        assert_eq!(
            answers[0].1["SHIM"], "PRESENT",
            "the shim is not in the attempt's worktree, so NOTFOUND below proves nothing"
        );
        // Neither resolves it, and both resolve a name that is on the path to
        // the same absolute file.
        assert_eq!(
            answers[0].1["CLAUDE"], "NOTFOUND",
            "repository content became the CLI the attempt runs"
        );
        assert_eq!(answers[1].1["CLAUDE"], "NOTFOUND");
        assert_eq!(
            answers[0].1["SH"], answers[1].1["SH"],
            "pre-flight certified a different binary from the one the attempt executes \
             (DESIGN.md:612)"
        );
        assert!(
            answers[0].1["SH"].starts_with('/'),
            "{:?}",
            answers[0].1["SH"]
        );

        assert_eq!(
            docker
                .containers_with_label(LABEL_PRIVATE_ROOT, &label)
                .expect("reachable")
                .len(),
            0
        );
    }

    /// `proof_tests[1]`: "**Git-dependent gate sees only the role view**",
    /// against a real container.
    ///
    /// The four properties of DESIGN.md:612, each read out of a real `git`
    /// running inside the boundary: the exact detached HEAD, the exact index
    /// (`status --porcelain` is empty on a clean worktree), no engine refs, and
    /// objects that resolve. The coordinator's refs are re-read afterwards, so
    /// "without exposing **or mutating**" is both halves.
    #[test]
    fn real_docker_a_git_dependent_gate_sees_only_the_role_view() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_a_git_dependent_gate_sees_only_the_role_view",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), GIT_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-gitview");
        let run_id = gated_run("gitview");
        let repo_dir = root.join("repo");
        let (head, _) = repo::repository(&repo_dir);
        let planted = repo::engine_refs(&repo_dir, &head);
        let identity = real_identity(&root, &repo_dir, run_id);
        let execution_root =
            crate::workspace_manager::execution_root_of(&identity.private_root, REPO_KEY, run_id);
        let workspace = execution_root.join("tasks").join("kalpha-g0");
        repo::worktree(&repo_dir, &workspace, &head);
        repo::git_ok(&repo_dir, &["pack-refs", "--all"]);

        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };
        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_poll(Duration::from_millis(10));

        // `safe.directory` because the host paths are owned by the coordinator's
        // user and the container's process is not it — an ownership check, not a
        // confinement one.
        let git = "git -c safe.directory='*' -C /tactus/workspace";
        let leak = &planted[0];
        let script = format!(
            "{git} rev-parse HEAD | sed 's/^/HEAD=/'; \
             {git} rev-parse --absolute-git-dir | sed 's/^/GITDIR=/'; \
             {git} for-each-ref --format='%(refname)' | wc -l | tr -d ' ' | sed 's/^/REFS=/'; \
             {git} log -1 --format=%s | sed 's/^/SUBJECT=/'; \
             {git} status --porcelain | wc -l | tr -d ' ' | sed 's/^/DIRTY=/'; \
             {git} rev-parse --verify --quiet '{leak}' >/dev/null 2>&1 && echo LEAK=yes || echo LEAK=no"
        );
        let request = gate_request(
            ShellKind::Sh.spec(&script),
            workspace.clone(),
            Duration::from_secs(60),
            gate_id(0),
        );
        let output = runner.run(&request).expect("runs");
        assert_eq!(output.code, Some(0), "stderr: {}", output.stderr);
        let line = |key: &str| -> String {
            output
                .stdout
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap_or_else(|| {
                    panic!(
                        "`{key}` is not in stdout {:?} / stderr {:?}",
                        output.stdout, output.stderr
                    )
                })
                .trim()
                .to_owned()
        };
        assert_eq!(line("HEAD="), head, "the exact detached HEAD");
        assert_eq!(
            line("GITDIR="),
            BoundaryLayout::DEFAULT_GIT_VIEW,
            "the tool found the coordinator's Git directory, not the role view"
        );
        assert_eq!(line("REFS="), "0", "the view carries refs");
        assert_eq!(line("SUBJECT="), "second", "the objects do not resolve");
        assert_eq!(
            line("DIRTY="),
            "0",
            "the index the view carries is not exact"
        );
        assert_eq!(
            line("LEAK="),
            "no",
            "an engine ref resolved inside the view"
        );

        // Nothing was exposed and nothing was mutated: the coordinator's refs
        // are all still there and still where they were.
        for name in &planted {
            assert_eq!(
                repo::git_ok(&repo_dir, &["rev-parse", "--verify", name]),
                head
            );
        }
        assert_eq!(repo::git_ok(&repo_dir, &["rev-parse", "HEAD"]), head);
        assert!(
            list_intents(&identity.private_root)
                .expect("scan")
                .is_empty()
        );
    }

    /// `decisions.tests_acceptance.parity`: "host and container runners produce
    /// identical **adapter parsing**".
    ///
    /// The table, the fixtures and the expectations are PR4's — `runner::tests::
    /// adapter_parse_parity` was written for exactly this and its doc comment
    /// says so — and the **only** thing this test varies is the `&dyn Runner`.
    /// It is a real chain, spec -> runner -> `ProcessOutput` -> `AgentAdapter::
    /// parse`, because the claim is about the seam: an adapter never learns
    /// which runner produced the output it reads, and nothing but a runner
    /// actually producing it proves that.
    #[test]
    fn real_docker_adapter_parsing_matches_the_host_table() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_adapter_parsing_matches_the_host_table",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), PREFERRED_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-parity");
        let run_id = gated_run("parity");
        let repo_dir = root.join("repo");
        repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, run_id);
        let workspace = root.join("parity-workspace");
        std::fs::create_dir_all(&workspace).expect("a workspace");
        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };
        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_poll(Duration::from_millis(10));

        let container_rows = crate::runner::tests::adapter_parse_parity(&runner, &workspace);
        let host_rows = crate::runner::tests::adapter_parse_parity(
            &crate::runner::host::HostRunner::new(),
            &workspace,
        );
        assert_eq!(
            container_rows, host_rows,
            "the container runner's adapter parsing differs from the host's"
        );
        // The table is not empty and really did vary, so equality is a claim.
        assert_eq!(container_rows.len(), 3);
        let statuses: BTreeSet<String> = container_rows
            .iter()
            .map(|row| format!("{:?}", row.status))
            .collect();
        assert_eq!(statuses.len(), 2, "{statuses:?}");
        assert!(
            list_intents(&identity.private_root)
                .expect("scan")
                .is_empty()
        );
    }

    /// The recorded image sets a credential location, and a role that takes
    /// none does not receive it **inside the container**.
    ///
    /// `PR6-CORRECTNESS-007`, against the daemon. The unit-level assertion is
    /// about the composed vector, and the composed vector is not the
    /// container's environment: `docker create --env` **overlays** the image's
    /// own, so a key the runner omits is a key the image decides. This runs a
    /// gate — repository-controlled code, the one thing no agent permission
    /// surface bounds — inside an image whose `ENV` sets `CODEX_HOME` and
    /// `CLAUDE_CONFIG_DIR`, and reads the variables back from inside.
    ///
    /// Second field held constant: the same image and the same command in both
    /// halves; only the role moves. `GH_CONFIG_DIR` is the control — an image
    /// variable that is not a credential location — so a runner that wiped the
    /// image environment rather than overriding two keys of it fails here.
    #[test]
    fn real_docker_withholds_an_image_credential_variable_from_a_role_that_takes_none() {
        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_withholds_an_image_credential_variable_from_a_role_that_takes_none",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), &[CREDENTIAL_ENV_IMAGE]) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };
        // The premise, read from the daemon rather than assumed: this image
        // really does set a credential location.
        let declared = docker
            .raw(
                RuntimeOp::InspectImageById,
                &image_id,
                &[
                    "image",
                    "inspect",
                    &image_id,
                    "--format",
                    "{{join .Config.Env \"\u{1f}\"}}",
                ],
            )
            .expect("the image environment");
        assert!(
            declared.contains("CODEX_HOME=/image/codex"),
            "`{CREDENTIAL_ENV_IMAGE}` does not set a credential location, so this measurement \
             would be vacuous: {declared}"
        );

        let root = repo::scratch("real-credenv");
        let repo_dir = root.join("repo");
        repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, gated_run("credenv"));
        let workspace = root.join("plain-workspace");
        std::fs::create_dir_all(&workspace).expect("a workspace");
        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };

        // A policy that DOES record a credential volume for codex would be
        // creating operator state; the withholding is about the *variable*, and
        // the mount is separately asserted by
        // `the_credential_volume_is_mounted_exactly_when_its_location_is_supplied`.
        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity,
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_hooks(Box::new(RecordingHooks::new(trace.clone())))
        .with_poll(Duration::from_millis(10));

        let (control_key, control_value) = CREDENTIAL_ENV_CONTROL;
        let spec = ShellKind::Sh.spec(&format!(
            "printf 'CODEX=[%s]\\n' \"$CODEX_HOME\"; \
             printf 'CLAUDE=[%s]\\n' \"$CLAUDE_CONFIG_DIR\"; \
             printf 'CONTROL=[%s]\\n' \"${{{control_key}}}\""
        ));
        let request = gate_request(spec, workspace, Duration::from_secs(60), gate_id(0));
        let output = runner.run(&request).expect("runs");
        assert_eq!(output.code, Some(0), "stderr: {}", output.stderr);
        let line = |key: &str| -> String {
            output
                .stdout
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap_or_else(|| panic!("`{key}` is not in {:?}", output.stdout))
                .to_owned()
        };
        assert_eq!(
            line("CODEX="),
            "[]",
            "a gate ran with the image's own `CODEX_HOME`; DESIGN.md:258-262 has each runner \
             supply ROLE-SCOPED credential locations, and a location the composed vector does \
             not name is one the image supplies"
        );
        assert_eq!(line("CLAUDE="), "[]", "stdout: {}", output.stdout);
        assert_eq!(
            line("CONTROL="),
            format!("[{control_value}]"),
            "the image environment was wiped rather than overridden, so the two assertions \
             above hold for the wrong reason"
        );
    }

    /// A container contains its **descendants**, including one that has left
    /// the leader's session.
    ///
    /// `PR6-ENUM-006`. `invariants_introduced[0]` is "container contains
    /// descendants", and nothing in this suite observed one: the timeout
    /// fixture runs a single `sleep` and checks one container liveness bit, so
    /// a cancellation that terminated or forgot only the leader would pass it.
    ///
    /// The descendant is `setsid`-detached, which is exactly what escapes a
    /// host process-group kill (`agent::proc`'s whole subject), and it is
    /// observed **in the host's own process table** rather than by timing: a
    /// container's processes are ordinary host processes in another namespace,
    /// so `/proc/<pid>/cmdline` can name them. Each `sleep` carries a marker
    /// argument nothing else on the machine uses.
    ///
    /// The intersection: {leader, detached descendant} × {container running,
    /// container reclaimed}. The two "running" cells are the control — without
    /// them a test in which nothing ever started would pass.
    #[test]
    #[cfg(unix)]
    fn real_docker_a_container_contains_a_daemonised_descendant() {
        /// Distinct markers, so the leader and the descendant are told apart by
        /// value and not by order.
        ///
        /// They are the **durations** the two `sleep` calls are given, so they
        /// have to be numbers — `sleep 999111r3b` answers `invalid number` and
        /// exits 1, which is a container that ends before its timeout and a
        /// fixture that measures nothing. Two implausible second counts, far
        /// apart in value, are what makes them recognisable in `/proc` without
        /// making them unrunnable.
        const LEADER_MARKER: &str = "903222";
        const DESCENDANT_MARKER: &str = "903111";

        /// Every pid whose argv contains `marker`, read from `/proc`.
        ///
        /// `contains` over argv entries rather than a substring of the whole
        /// buffer, so this process's own command line — which carries the
        /// marker as a literal in the binary, not in argv — cannot match.
        fn pids_with(marker: &str) -> Vec<String> {
            let mut found = Vec::new();
            let Ok(entries) = std::fs::read_dir("/proc") else {
                return found;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
                    continue;
                };
                if raw
                    .split(|byte| *byte == 0)
                    .filter(|arg| !arg.is_empty())
                    .any(|arg| String::from_utf8_lossy(arg) == marker)
                {
                    found.push(name);
                }
            }
            found
        }

        let trace = ContainerTrace::recording();
        let docker = match docker_gate(
            "real_docker_a_container_contains_a_daemonised_descendant",
            trace.clone(),
        ) {
            Ok(docker) => docker,
            Err(reason) => return skipped(&reason),
        };
        let (_, image_id) = match discover(docker.as_ref(), PREFERRED_IMAGES) {
            Ok(found) => found,
            Err(reason) => return no_image(&reason),
        };

        let root = repo::scratch("real-descendant");
        let repo_dir = root.join("repo");
        repo::repository(&repo_dir);
        let identity = real_identity(&root, &repo_dir, gated_run("descendant"));
        let workspace = root.join("plain-workspace");
        std::fs::create_dir_all(&workspace).expect("a workspace");
        let _residue = LeaveNoResidue {
            docker: (*docker).clone(),
            private_root: identity.private_root.clone(),
        };

        let runner = ContainerRunner::new(
            real_policy(&image_id),
            identity.clone(),
            &repo_dir,
            image_environment_of(docker.as_ref(), &image_id),
            Box::new((*docker).clone()),
        )
        .expect("a container policy")
        .with_hooks(Box::new(RecordingHooks::new(trace.clone())))
        .with_poll(Duration::from_millis(10));

        // The leader spawns a detached descendant and then blocks. The runner's
        // own timeout is what stops the container, which is
        // `slice_contract.cancellation`: "timeout or shutdown stops and removes
        // the container".
        let mut request = gate_request(
            ShellKind::Sh.spec(&format!(
                "if command -v setsid >/dev/null 2>&1; then \
                   setsid sleep {DESCENDANT_MARKER} >/dev/null 2>&1 & \
                 else \
                   sleep {DESCENDANT_MARKER} >/dev/null 2>&1 & \
                 fi; \
                 sleep {LEADER_MARKER}"
            )),
            workspace,
            Duration::from_secs(120),
            gate_id(0),
        );
        request.timeout = Duration::from_secs(3);

        // The controls, sampled from another thread while the invocation is in
        // flight: both processes really are on this machine.
        let seen = std::thread::scope(|scope| {
            let sampler = scope.spawn(|| {
                let deadline = Instant::now() + Duration::from_secs(20);
                let mut leader = false;
                let mut descendant = false;
                while Instant::now() < deadline && !(leader && descendant) {
                    leader |= !pids_with(LEADER_MARKER).is_empty();
                    descendant |= !pids_with(DESCENDANT_MARKER).is_empty();
                    std::thread::sleep(Duration::from_millis(50));
                }
                (leader, descendant)
            });
            let output = runner.run(&request).expect("runs");
            assert!(output.timed_out, "the fixture did not reach its timeout");
            sampler.join().expect("the sampler panicked")
        });
        assert_eq!(
            seen,
            (true, true),
            "the fixture never had a leader and a detached descendant running at once, so the \
             containment assertion below would hold vacuously"
        );

        // The container is gone, and so is everything it contained. The
        // descendant left the leader's session, so a cancellation that killed
        // only the leader's process group would leave it here.
        let leader = pids_with(LEADER_MARKER);
        let descendant = pids_with(DESCENDANT_MARKER);
        // The descendant first: it is the invariant's subject, and a leader
        // assertion that fires before it would report the wrong thing about a
        // cancellation that left both alive.
        assert!(
            descendant.is_empty(),
            "a `setsid`-detached descendant survived its container, so \
             `invariants_introduced[0]` (\"container contains descendants\") does not hold: \
             {descendant:?}"
        );
        assert!(
            leader.is_empty(),
            "the invocation's leader survived its container: {leader:?}"
        );
    }

    /// Every Docker-gated test this lane adds is on the list that counts them.
    ///
    /// `every_docker_gated_test_is_named_and_present` in the substrate's own
    /// suite closes both directions across `src/runner/**`; this is the lane's
    /// half, so a name added here without being listed fails in this file
    /// rather than in another lane's.
    #[test]
    fn every_gated_test_of_this_lane_is_counted() {
        const MINE: &[&str] = &[
            "real_docker_runs_from_the_recorded_image_id_and_composes_over_the_image_environment",
            "real_docker_refuses_a_reviewer_write_to_its_read_only_mount",
            "real_docker_confines_a_gate_to_its_mount",
            "real_docker_a_git_dependent_gate_sees_only_the_role_view",
            "real_docker_adapter_parsing_matches_the_host_table",
            // Repair round R1.
            "real_docker_a_gate_write_outside_every_declared_mount_fails",
            "real_docker_the_daemon_holds_exactly_the_specs_mounts_and_a_read_only_root",
            "real_docker_a_worktree_binary_cannot_shadow_the_certified_cli",
            // Repair round R3b.
            "real_docker_withholds_an_image_credential_variable_from_a_role_that_takes_none",
            "real_docker_a_container_contains_a_daemonised_descendant",
        ];
        assert_eq!(MINE.len(), 10);
        assert_eq!(GATED_RUNS.len(), MINE.len(), "one run id per gated test");
        let ids: BTreeSet<&str> = GATED_RUNS.iter().map(|(_, run)| *run).collect();
        assert_eq!(
            ids.len(),
            GATED_RUNS.len(),
            "two gated tests share a run id, so they would fight over a container name"
        );
        let source = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("runner")
                .join("container")
                .join("exec.rs"),
        )
        .expect("read this module");
        for name in MINE {
            assert!(
                DOCKER_GATED_TESTS.contains(name),
                "`{name}` is gated and nothing counts it"
            );
            assert!(
                source.contains(&format!("fn {name}(")),
                "`{name}` is counted and is not a test here"
            );
        }
    }
}
