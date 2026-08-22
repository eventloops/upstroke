//! The **Container funnel** — `FunnelGroup::Container.module()` is this file.
//!
//! `decisions.effect_site_inventory.identity`: "every effectful funnel API
//! takes its group's site by value, and the funnel itself calls `hook(Before,
//! site)` -> primitive -> `hook(After, site)`, so hooks exist for every site by
//! construction". `ContainerSite` in the frozen `src/topology/effects.rs` has
//! **eight** variants and all eight are taken by value by an API here.
//!
//! ## Why this is a file and not `container/mod.rs`
//!
//! `FunnelGroup::Container.module()` returns the literal
//! `"src/runner/container.rs"` and
//! `effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
//! reads exactly that path. `container/mod.rs` would make the inventory's
//! `module` column false of this tree — `PR5-CONF-018` is the standing entry
//! for what that costs. Rust 2018 path style makes this file plus
//! `src/runner/container/*.rs` the ordinary layout, so both hold at once.
//!
//! ## What "impossible to bypass" can and cannot mean here
//!
//! Rust module privacy cannot isolate siblings under a shared ancestor: an item
//! private to `runner::container` is visible to `runner::container::census` and
//! to every other module a lane adds beside it, so no token, sealed trait or
//! private constructor makes a bypass a **compile** error from inside this
//! subtree. The project's own mechanism for exactly this is
//! `decisions.effect_site_inventory.mechanism` (1)-(2), and it is a **build**
//! error rather than a compile one:
//!
//! * every effectful method of [`runtime::ContainerRuntime`] and of [`GitView`]
//!   is on `clippy.toml`'s disallowed list — "docker invocation helpers" is the
//!   packet's own phrase for them — so a module that calls one fails
//!   `cargo clippy -- -D warnings` unless it is in `effects/allowlist.toml`,
//!   which is a reviewed artifact at every gate;
//!
//!   **This sentence was false between PR6 and repair round F1, and the shape
//!   of its falsehood is worth keeping.** The allow below is an *inner*
//!   attribute, and a Rust lint level is scoped by the **module tree** rather
//!   than by the file — so `runner::container::{census, env, exec, fake,
//!   intent, runtime, tests, view}` all inherited it, and a
//!   `ContainerRuntime::start` planted in one of them passed the exact clippy
//!   gate. Measured, twice: by lane A with a planted probe and by the lane-F
//!   review (`PR6A-CONTAINER-ALLOW-IS-INHERITED-BY-EVERY-LANE-MODULE` /
//!   `PR6-LANEF-004`). Each of those files now **re-denies** the three governed
//!   lints, and the three that need one for their *test region* carry an allow
//!   of their own with an `effects/allowlist.toml` entry to be read against.
//!   [`tests::every_child_module_of_the_container_funnel_states_its_own_lint_level`]
//!   refuses a new file here that states neither, which is what stops the hole
//!   reopening for the next lane rather than for this one;
//! * [`tests::every_container_effect_in_the_tree_goes_through_the_funnel`] is
//!   the source census beside it, in the idiom of
//!   `runner::tests::every_production_process_start_is_classified`: it names
//!   every file that may issue a container effect and fails when a new one
//!   appears.
//!
//! `PR5D-PROCESS-FUNNEL-TAKES-NO-SITE` records what happens when a group has no
//! funnel that names its sites, and `PR5D-FUNNEL-RETURNS-A-COMMAND` what
//! happens when one hands a writable handle back. Neither is repeated here: the
//! site travels with every call, and no API returns a runtime handle, a
//! `Command`, or a `File`.
//!
//! ## The orderings
//!
//! `slice_contract.side_effect_vs_event_ordering` is the whole of this module's
//! contract: "no events; intent synced before docker create; container created
//! from the recorded id and verified before start; view mounted before start;
//! stop/rm, view removal, intent removal after completion". Each clause is an
//! independently droppable predicate, so [`launch`] and [`release`] perform
//! them in one place and [`runtime::ContainerTrace`] records the sequence,
//! which is what the tests assert on.
// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
// carries this module's review clause -- effects only inside site-taking APIs,
// no writable handle returned. `decisions.effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

// -- module declarations -------------------------------------------------
// APPEND ONLY. Lane A adds `exec`, `view` and `env`; lane C adds `census`;
// lane B adds `resolve`.
// Keep every `#[cfg(test)]` declaration at the BOTTOM of this file:
// `effects::production_region` cuts a source at its FIRST `#[cfg(test)]`, so a
// test-only `mod` here would remove every funnel below it from the census that
// proves this group has a funnel at all (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).
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

use crate::error::TactusError;
use crate::topology::effects::{ContainerSite, EffectSiteId, HookPhase, Injection};
use crate::util;

use crate::runner::InvocationId;
use intent::{ContainerIntent, ContainerName, INTENT_STAGED_SUFFIX, containers_dir};
use runtime::{
    ContainerExecution, ContainerRuntime, ContainerTrace, CreateSpec, CreatedContainer,
    DiscoveredContainer, DurableStep, Liveness, RuntimeError, RuntimeOp, StopMode, TracePhase,
    ViewAction,
};

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

/// What the funnel consults at each phase of each site.
///
/// The sibling of [`crate::rundir::RunDirHooks`] and
/// [`crate::workspace_manager::EffectHooks`]. The site travels with the call
/// because this funnel serves eight sites, which is the shape
/// `effect_site_inventory.identity` describes in as many words.
pub trait ContainerHooks {
    /// The funnel reached `phase` of `site`. The answer says what to do there.
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection;

    /// Where this observer wants the funnel's ordered record kept.
    ///
    /// A *handle*, taken before the funnel body runs, because `funnel` holds
    /// `&mut dyn ContainerHooks` across the body — the same reason
    /// `EffectHooks::durability_ledger` is a handle. The default records
    /// nothing, which is what production passes.
    fn trace(&self) -> ContainerTrace {
        ContainerTrace::off()
    }
}

/// What production passes: nothing is armed and nothing is recorded.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl ContainerHooks for NoHooks {
    fn phase(&mut self, _site: EffectSiteId, _phase: HookPhase) -> Injection {
        Injection::Proceed
    }
}

/// Turn a hook's answer into what the funnel must do at that point.
fn apply(injection: Injection, site: EffectSiteId, phase: HookPhase) -> Result<(), TactusError> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(TactusError::Refused {
            message: format!("the container funnel was made to fail at `{site}` ({phase})"),
        }),
    }
}

/// One effect, between its two hook phases, with its site recorded in the
/// trace on both sides.
///
/// An `Err` from the `After` phase is returned *after* the primitive ran, which
/// is the whole point of the error-return mode.
fn funnel<T>(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    primitive: impl FnOnce() -> Result<T, TactusError>,
) -> Result<T, TactusError> {
    let id = EffectSiteId::Container(site);
    let trace = hooks.trace();
    trace.site(site, TracePhase::Before);
    apply(hooks.phase(id, HookPhase::Before), id, HookPhase::Before)?;
    let produced = primitive()?;
    apply(hooks.phase(id, HookPhase::After), id, HookPhase::After)?;
    trace.site(site, TracePhase::After);
    Ok(produced)
}

// ---------------------------------------------------------------------------
// The site each API takes, and the guard that keeps the parameter honest
// ---------------------------------------------------------------------------

/// What a site names.
///
/// Every funnel API below takes `site: ContainerSite` **by value**, which is
/// what `identity` requires. A free parameter can be passed a wrong value, so
/// each API checks it against this map: passing `ContainerSite::Start` to
/// [`write_intent`] refuses, before any effect, rather than writing a record
/// under a label that lies about what happened.
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

/// The site-to-operation map, exhaustive over the frozen eight.
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

/// Refuse a site that does not name this operation.
fn expect_site(site: ContainerSite, wanted: Operation) -> Result<(), TactusError> {
    if operation_of(site) == wanted {
        return Ok(());
    }
    Err(TactusError::Refused {
        message: format!(
            "the container funnel was asked to perform {wanted:?} under site \
             `Container.{}`; every effectful funnel API takes its group's site by value \
             (decisions.effect_site_inventory.identity) and the site must name the \
             operation it accounts for",
            site.name()
        ),
    })
}

// ---------------------------------------------------------------------------
// The Git view (R19)
// ---------------------------------------------------------------------------

/// What a Git view needs to exist.
///
/// DESIGN.md:612: "the container overlays a disposable role-scoped Git view —
/// exact detached HEAD/index, no engine refs, read-only objects — so
/// Git-dependent tools work without exposing or mutating the coordinator's
/// refs."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitViewRequest {
    /// Where the view directory is materialised, on the host.
    pub path: PathBuf,
    /// The worktree the role is executing in.
    pub workspace: PathBuf,
    /// The commit the view is pinned to, when the projection needs one.
    pub head: Option<String>,
}

/// The R19 disposable Git view.
///
/// Its two methods are the primitives of `Container.MountGitView` and
/// `Container.UnmountGitView` and are on `clippy.toml`'s disallowed list, so
/// only this module calls them — [`mount_git_view`] and [`unmount_git_view`]
/// are the funnels, and they are what a caller uses.
///
/// **Lane A implements the projection.** [`DisposableDirView`] below is the
/// directory half — the R19 artifact whose lifecycle the resource row accounts
/// for ("mounted": "pruned on complete or cancel; orphan views reclaimed during
/// dead-owner or dead-incarnation container reclaim") — and it is what the
/// substrate's own tests and the reclaim path need. What it does **not** do is
/// the detached HEAD/index projection or the read-only object mount; those are
/// `src/runner/container/view.rs`.
pub trait GitView: Send + Sync {
    /// Bring the view into existence, returning where it is.
    ///
    /// # Errors
    ///
    /// [`TactusError::Io`] when the view cannot be materialised.
    fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, TactusError>;

    /// Remove it. **Idempotent**: an orphan view is reclaimed by whichever
    /// process gets there first, and reclaim converges.
    ///
    /// # Errors
    ///
    /// [`TactusError::Io`] when the view exists and cannot be removed.
    fn discard(&self, path: &Path) -> Result<(), TactusError>;
}

/// The directory half of the view: create it, remove it.
///
/// Not a stub — this is R19's whole physical artifact, and the row's lifecycle
/// is about the directory. Lane A's projection fills it.
#[derive(Debug, Clone, Default)]
pub struct DisposableDirView {
    trace: ContainerTrace,
}

impl DisposableDirView {
    /// A view whose actions are recorded in `trace`.
    #[must_use]
    pub fn new(trace: ContainerTrace) -> Self {
        Self { trace }
    }
}

impl GitView for DisposableDirView {
    fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, TactusError> {
        fs::create_dir_all(&request.path).map_err(|source| TactusError::Io {
            path: request.path.clone(),
            source,
        })?;
        self.trace.view(ViewAction::Materialized, &request.path);
        Ok(request.path.clone())
    }

    fn discard(&self, path: &Path) -> Result<(), TactusError> {
        // R19's half of "every step idempotent and tolerant of already-gone so
        // two concurrent reclaimers converge". The errno is not the question —
        // see [`RACING_ACCESS_ATTEMPTS`], and the Windows guest measurement that
        // put it there.
        racing_removal(path, || fs::remove_dir_all(path))?;
        self.trace.view(ViewAction::Discarded, path);
        Ok(())
    }
}

/// How many times a path that another reclaimer may be removing is asked about
/// before a failure is believed.
///
/// **The whole reason this exists is a platform difference, measured on the
/// Windows guest and invisible on Linux.** "every step idempotent and tolerant
/// of already-gone so two concurrent reclaimers converge" is usually written as
/// `if error.kind() == NotFound`, and on Windows the losing reclaimer does not
/// get `NotFound`: a file or directory another process is deleting is
/// **delete-pending**, and opening it answers `ERROR_ACCESS_DENIED` — `kind() ==
/// PermissionDenied` — until the winner's handle closes. An errno test
/// therefore cannot tell "somebody else is removing it" from "I may not touch
/// it", and tolerating `PermissionDenied` outright would silently treat a
/// genuinely protected path as reclaimed.
///
/// So the question asked is the **outcome**, not the errno: retry, and believe
/// the failure only once the path has stopped changing under it. Delete-pending
/// clears when the winner's own call returns, so this is a handoff rather than a
/// wait, and [`std::thread::yield_now`] is what it costs.
///
/// Bounded rather than timed, for the reason [`TERMINATION_OBSERVATIONS`] is: a
/// wait with no bound turns "this path cannot be removed" into "this write
/// command never returns".
pub const RACING_ACCESS_ATTEMPTS: usize = 64;

// ---------------------------------------------------------------------------
// The eight site-taking APIs
// ---------------------------------------------------------------------------

/// `Container.WriteIntent` (R26) — the synced global intent record.
///
/// "every container invocation writes a **synced** intent in the global
/// namespace `<R>/containers/<container-name>.intent`". Written the way every
/// other durable record in this engine is written: staged, fsynced, renamed,
/// and the directory fsynced — `run_creation`'s own four steps — each recorded
/// in the trace beside the primitive that performs it, so a deleted step is a
/// missing trace entry rather than an invisible loss of durability.
///
/// Returns the path of the published record, which is data and not a handle.
///
/// # Errors
///
/// [`TactusError::Refused`] when `site` does not name this operation,
/// [`TactusError::Io`] on any filesystem failure, [`TactusError::Git`] when the
/// record will not serialize.
pub fn write_intent(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    private_root: &Path,
    name: &ContainerName,
    record: &ContainerIntent,
) -> Result<PathBuf, TactusError> {
    expect_site(site, Operation::WriteIntent)?;
    let path = name.intent_path(private_root);
    let trace = hooks.trace();
    funnel(hooks, site, || {
        let bytes = serde_json::to_vec(record).map_err(|error| TactusError::Git {
            message: format!("serializing the container intent for `{name}`: {error}"),
        })?;
        write_synced(&path, &bytes, &trace)?;
        Ok(path.clone())
    })
}

/// `Container.Create` (R26) — create the container **from an image id**.
///
/// INV-23: "every container of every epoch is created from the recorded image
/// id". [`CreateSpec`] carries no reference at all, so creating from one is not
/// expressible. The returned [`CreatedContainer::reported_image_id`] is the
/// runtime's own answer; [`launch`] verifies it against the record **before
/// start**.
///
/// # Errors
///
/// [`TactusError::Refused`] when `site` does not name this operation or the
/// runtime refuses.
pub fn create_container(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    runtime: &dyn ContainerRuntime,
    spec: &CreateSpec,
) -> Result<CreatedContainer, TactusError> {
    expect_site(site, Operation::Create)?;
    funnel(hooks, site, || runtime.create(spec).map_err(refused))
}

/// `Container.Start` (R26).
///
/// # Errors
///
/// [`TactusError::Refused`] when `site` does not name this operation or the
/// runtime refuses.
pub fn start_container(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    runtime: &dyn ContainerRuntime,
    name: &ContainerName,
) -> Result<(), TactusError> {
    expect_site(site, Operation::Start)?;
    funnel(hooks, site, || {
        runtime.start(name.as_str()).map_err(refused)
    })
}

/// `Container.MountGitView` (**R19**, not R26 — the view is its own row).
///
/// # Errors
///
/// [`TactusError::Refused`] when `site` does not name this operation, or
/// whatever the projection returns.
pub fn mount_git_view(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    view: &dyn GitView,
    request: &GitViewRequest,
) -> Result<PathBuf, TactusError> {
    expect_site(site, Operation::MountGitView)?;
    funnel(hooks, site, || view.materialize(request))
}

/// `Container.Stop` (R26) — completion's `docker stop` and reclaim's `docker
/// kill`, which the frozen inventory accounts to one site.
///
/// # Errors
///
/// [`TactusError::Refused`] when `site` does not name this operation or the
/// runtime refuses.
pub fn stop_container(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    runtime: &dyn ContainerRuntime,
    name: &ContainerName,
    mode: StopMode,
) -> Result<(), TactusError> {
    expect_site(site, Operation::Stop)?;
    funnel(hooks, site, || {
        runtime.stop(name.as_str(), mode).map_err(refused)
    })
}

/// `Container.Remove` (R26) — `docker rm`, idempotent.
///
/// # Errors
///
/// [`TactusError::Refused`] when `site` does not name this operation or the
/// runtime refuses.
pub fn remove_container(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    runtime: &dyn ContainerRuntime,
    name: &ContainerName,
) -> Result<(), TactusError> {
    expect_site(site, Operation::Remove)?;
    funnel(hooks, site, || {
        runtime.remove(name.as_str()).map_err(refused)
    })
}

/// `Container.UnmountGitView` (R19), idempotent.
///
/// # Errors
///
/// [`TactusError::Refused`] when `site` does not name this operation, or
/// whatever the projection returns.
pub fn unmount_git_view(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    view: &dyn GitView,
    path: &Path,
) -> Result<(), TactusError> {
    expect_site(site, Operation::UnmountGitView)?;
    funnel(hooks, site, || view.discard(path))
}

/// `Container.RemoveIntent` (R26), idempotent.
///
/// # Errors
///
/// [`TactusError::Refused`] when `site` does not name this operation,
/// [`TactusError::Io`] when the record exists and cannot be removed.
pub fn remove_intent(
    hooks: &mut dyn ContainerHooks,
    site: ContainerSite,
    private_root: &Path,
    name: &ContainerName,
) -> Result<(), TactusError> {
    expect_site(site, Operation::RemoveIntent)?;
    let path = name.intent_path(private_root);
    let trace = hooks.trace();
    funnel(hooks, site, || {
        // The staged half too: a crash between the stage and the rename leaves
        // `<name>.intent.tmp`, and a reclaim that removed only the published
        // name would leave writer-owned residue in a directory the census
        // enumerates.
        let staged = staged_path(&path);
        remove_if_present(&staged, &trace)?;
        remove_if_present(&path, &trace)
    })
}

// ---------------------------------------------------------------------------
// The sequences the contract states
// ---------------------------------------------------------------------------

/// Everything one container invocation needs.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    /// `<R>` — the run's **recorded** private root.
    pub private_root: PathBuf,
    pub name: ContainerName,
    /// Which invocation this container is, as the tuple rather than as the
    /// rendered string the intent carries.
    ///
    /// INV-23 gives an image-id mismatch **two** outcomes and the phase is what
    /// chooses between them, so the phase has to be readable at the point the
    /// mismatch is observed: see [`exec::ImageIdMismatch`].
    pub invocation: InvocationId,
    pub intent: ContainerIntent,
    /// The create arguments. `spec.image_id` is the record's image id, and it
    /// is what the reported id is verified against.
    pub spec: CreateSpec,
    pub view: GitViewRequest,
}

/// A container that is running, and what it took to get there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    pub name: ContainerName,
    pub intent_path: PathBuf,
    pub view_path: PathBuf,
    /// The id the runtime reported, already verified equal to the record.
    pub reported_image_id: String,
}

/// The ordering `side_effect_vs_event_ordering` states, in one place.
///
/// > intent synced before docker create; container created from the recorded id
/// > and verified before start; view mounted before start
///
/// Four sites, and the verification between `Create` and everything after it.
/// **This is also what makes "container start without an intent is impossible
/// by construction"** (`expected_failures_refusals[6]`) true of the shape a
/// caller uses: the only sequence that reaches `Container.Start` begins by
/// writing the intent.
///
/// ## Why `MountGitView` precedes `Create` and not merely `Start`
///
/// The clause says "view mounted before start", which two orders satisfy. Only
/// one of them runs: **the view is a bind-mount *source* of the `docker create`
/// call**, and a bind source must exist when the container is created.
/// Measured against `docker` 29.7.2 with `WriteIntent -> Create ->
/// MountGitView -> Start` —
///
/// ```text
/// the container runtime refused `create`: Error response from daemon: invalid
/// mount config for type "bind": bind source path does not exist: …/views/tactus-…
/// ```
///
/// — so that order cannot produce a working container for any `Implement`,
/// `Gate` or `Review` invocation at all. `PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE`
/// is the finding; it was invisible to the fake, whose `create` does not care
/// whether a source path exists, and to a real-runtime test whose `LaunchPlan`
/// carried no mounts. `real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently`
/// now carries the view as a real mount, which is what makes it able to see it.
///
/// `WriteIntent -> MountGitView -> Create(+verify) -> Start` holds every clause
/// the contract states — intent synced before create, created and verified
/// before start, view mounted before start — and is the only order a container
/// runtime accepts.
///
/// ## The cancel path, at every point it can fail
///
/// On a reported image id that differs from the record the invocation is
/// **refused before start** and everything it created is released — R26's
/// "released on complete …, **cancel**, or shutdown" and R19's "pruned on
/// complete or **cancel**" — so both ledgers balance and no census finds
/// residue of a refusal. The view is now mounted before the create, so the
/// cancel has an R19 residue to prune as well as an R26 one.
///
/// **The cancel is failure-atomic in both directions** (`PR6-LANEF-006`): every
/// step is attempted even after an earlier one fails, and the error returned is
/// always the **image-integrity refusal**. Returning a cleanup failure instead
/// would leave the operator holding "docker stop said no" while the fact that
/// the runtime executed a substituted image went unsaid, and stopping at the
/// first failure would leave a container *and* an intent where attempting both
/// leaves neither — `docker rm --force` removes a running container, so
/// `Remove` after a failed `Stop` is not a wasted call. What could not be
/// released is named in the refusal rather than swallowed.
///
/// # Errors
///
/// [`TactusError::Refused`] when the reported image id differs from the record,
/// or whatever a step returns.
pub fn launch(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    plan: &LaunchPlan,
) -> Result<Launched, TactusError> {
    let intent_path = write_intent(
        hooks,
        ContainerSite::WriteIntent,
        &plan.private_root,
        &plan.name,
        &plan.intent,
    )?;
    let view_path = mount_git_view(hooks, ContainerSite::MountGitView, view, &plan.view)?;
    let created = create_container(hooks, ContainerSite::Create, runtime, &plan.spec)?;
    if created.reported_image_id != plan.spec.image_id {
        let residue = cancel_created(
            hooks,
            runtime,
            view,
            &plan.private_root,
            &plan.name,
            Some(&view_path),
        );
        return Err(TactusError::Refused {
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
    start_container(hooks, ContainerSite::Start, runtime, &plan.name)?;
    Ok(Launched {
        name: plan.name.clone(),
        intent_path,
        view_path,
        reported_image_id: created.reported_image_id,
    })
}

/// Release everything a refused launch created, **attempting every step even
/// after one fails**, and answer what could not be released.
///
/// The four sites of [`release`], in the same order, with the `?` taken off.
/// `PR6-LANEF-006`: with `?` in place a failing `Container.Stop` returned the
/// stop error and left the container, the view and the intent behind — three
/// residues and a masked integrity refusal, from one failure.
///
/// The answer is a list of descriptions rather than a `Result`, because the
/// caller must return the refusal it already has: this function's failure is
/// never the thing to report *instead of* an integrity violation.
fn cancel_created(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    private_root: &Path,
    name: &ContainerName,
    view_path: Option<&Path>,
) -> Vec<String> {
    let mut residue = Vec::new();
    let mut note = |what: &str, error: &TactusError| {
        residue.push(format!("{what}: {error}"));
    };
    if let Err(error) = stop_container(
        hooks,
        ContainerSite::Stop,
        runtime,
        name,
        StopMode::Graceful,
    ) {
        note("the container could not be stopped", &error);
    }
    if let Err(error) = remove_container(hooks, ContainerSite::Remove, runtime, name) {
        note("the container could not be removed", &error);
    }
    if let Some(path) = view_path {
        if let Err(error) = unmount_git_view(hooks, ContainerSite::UnmountGitView, view, path) {
            note("the R19 Git view could not be pruned", &error);
        }
    }
    if let Err(error) = remove_intent(hooks, ContainerSite::RemoveIntent, private_root, name) {
        note("the R26 intent record could not be removed", &error);
    }
    residue
}

/// What a cancel could not release, appended to the refusal that caused it.
///
/// Empty when the cancel balanced, which is the ordinary case and leaves the
/// refusal's own wording untouched.
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

/// The completion half: "stop/rm, view removal, intent removal after
/// completion".
///
/// # Errors
///
/// Whatever a step returns.
pub fn release(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    private_root: &Path,
    launched: &Launched,
) -> Result<(), TactusError> {
    stop_container(
        hooks,
        ContainerSite::Stop,
        runtime,
        &launched.name,
        StopMode::Graceful,
    )?;
    remove_container(hooks, ContainerSite::Remove, runtime, &launched.name)?;
    unmount_git_view(
        hooks,
        ContainerSite::UnmountGitView,
        view,
        &launched.view_path,
    )?;
    remove_intent(
        hooks,
        ContainerSite::RemoveIntent,
        private_root,
        &launched.name,
    )
}

/// How many times reclaim asks whether a container has terminated.
///
/// `determinism` forbids sleeps, so this is a bounded number of round trips and
/// not a timed wait: `docker kill` returns after the signal has been delivered,
/// and each observation is a fresh inspection. A container still running after
/// all of them "cannot be observed terminated", which
/// `crash_reconstruction` says "blocks admission".
pub const TERMINATION_OBSERVATIONS: usize = 8;

/// One container reclaimed, in the packet's own order.
///
/// > reclaim = docker kill -> wait until observed exited/removed -> docker rm
/// > -> remove Git view -> remove intent, every step idempotent and tolerant of
/// > already-gone so two concurrent reclaimers converge
///
/// The Git view path is the caller's, because a census reads it from the
/// intent's run directory rather than from a live [`Launched`].
///
/// # Errors
///
/// [`TactusError::Refused`] when the container cannot be observed terminated
/// within [`TERMINATION_OBSERVATIONS`] observations, or whatever a step
/// returns.
pub fn reclaim(
    hooks: &mut dyn ContainerHooks,
    runtime: &dyn ContainerRuntime,
    view: &dyn GitView,
    private_root: &Path,
    name: &ContainerName,
    view_path: Option<&Path>,
) -> Result<(), TactusError> {
    stop_container(hooks, ContainerSite::Stop, runtime, name, StopMode::Kill)?;
    observe_terminated(runtime, name)?;
    remove_container(hooks, ContainerSite::Remove, runtime, name)?;
    if let Some(path) = view_path {
        unmount_git_view(hooks, ContainerSite::UnmountGitView, view, path)?;
    }
    remove_intent(hooks, ContainerSite::RemoveIntent, private_root, name)
}

/// "wait until observed exited/removed" — a read-only observation, and so not
/// a site.
///
/// # Errors
///
/// [`TactusError::Refused`] when the container is still running after
/// [`TERMINATION_OBSERVATIONS`] observations, or when the runtime refuses.
pub fn observe_terminated(
    runtime: &dyn ContainerRuntime,
    name: &ContainerName,
) -> Result<Liveness, TactusError> {
    for _ in 0..TERMINATION_OBSERVATIONS {
        let state = runtime.observe(name.as_str()).map_err(refused)?;
        if state.is_terminated() {
            return Ok(state);
        }
    }
    Err(TactusError::Refused {
        message: format!(
            "`{name}` is still running after {TERMINATION_OBSERVATIONS} observations and \
             cannot be observed terminated; a dead owner's or dead incarnation's labeled \
             container that cannot be observed terminated blocks admission \
             (transaction_fault_matrix[T-CONTAINER].refusal_condition)"
        ),
    })
}

// ---------------------------------------------------------------------------
// The orphan window
// ---------------------------------------------------------------------------

/// Who closes the window between a coordinator's death and its containers being
/// reclaimed.
///
/// `decisions.admission_and_leases.permits.os_matrix`, in full:
///
/// > Linux and macOS (cfg(unix)): the cleanup reaper survives coordinator
/// > death, settles the dead coordinator's process groups while holding R28,
/// > and additionally kills the dead coordinator's labeled containers, closing
/// > the **orphan window**; Windows: **no reaper**; the ambient
/// > coordinator-joined Job Object ends every ordinary host descendant incl.
/// > suspended stubs at any spawn sub-step, private per-invocation jobs scope
/// > timeouts, and containers are reclaimed at the **next write-command start**
/// > (orphan window until then; documented; **a portable watchdog is
/// > deferred**).
///
/// A value rather than a comment, so the Windows guest — which has no container
/// runtime at all — still asserts something about containers, and so lane C's
/// census can report the window it is closing rather than infer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrphanWindow {
    /// `cfg(unix)`: the per-invocation cleanup reaper outlives the coordinator
    /// and kills its labeled containers.
    ClosedByTheUnixReaper,
    /// Windows: nothing runs between the death and the next `tactus` write
    /// command, so the containers survive until its startup census.
    UntilNextWriteCommandStart,
}

impl OrphanWindow {
    /// Both answers. Written out so the platform is an axis and not a constant.
    pub const ALL: &'static [Self] = &[
        Self::ClosedByTheUnixReaper,
        Self::UntilNextWriteCommandStart,
    ];

    /// Whether a reaper closes it.
    #[must_use]
    pub const fn closed_by_a_reaper(self) -> bool {
        match self {
            Self::ClosedByTheUnixReaper => true,
            Self::UntilNextWriteCommandStart => false,
        }
    }
}

/// This platform's orphan window.
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

// ---------------------------------------------------------------------------
// Read-only observations of the global namespace
// ---------------------------------------------------------------------------

/// One intent record found in `<R>/containers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundIntent {
    pub name: ContainerName,
    pub path: PathBuf,
    pub record: ContainerIntent,
}

/// Read one record back.
///
/// # Errors
///
/// [`TactusError::Io`] when the file cannot be read, [`TactusError::Refused`]
/// when it is not a `ContainerIntent`.
pub fn read_intent(path: &Path) -> Result<ContainerIntent, TactusError> {
    let bytes = fs::read(path).map_err(|source| TactusError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|error| TactusError::Refused {
        message: format!("`{}` is not a container intent: {error}", path.display()),
    })
}

/// Read one record back, answering `None` when it went away under the read.
///
/// The read half of [`RACING_ACCESS_ATTEMPTS`]: on Windows a file another
/// process is deleting answers `PermissionDenied` until that delete completes,
/// so "is it gone?" is a question about the outcome and not about the first
/// errno. A record that is present and unreadable is still an error, after the
/// bound — silently skipping one would let a census admit over a container whose
/// ownership evidence it could not read.
///
/// # Errors
///
/// [`TactusError::Refused`] when the file is not a `ContainerIntent`,
/// [`TactusError::Io`] when it is still there and still unreadable after
/// [`RACING_ACCESS_ATTEMPTS`] attempts.
fn read_racing(path: &Path) -> Result<Option<ContainerIntent>, TactusError> {
    let mut last = None;
    for _ in 0..RACING_ACCESS_ATTEMPTS {
        match read_intent(path) {
            Ok(record) => return Ok(Some(record)),
            Err(TactusError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(None);
            }
            Err(TactusError::Io { source, .. }) => {
                last = Some(source);
                std::thread::yield_now();
            }
            // Not an IO answer at all: the bytes were read and are not a
            // record. Retrying cannot change that.
            Err(other) => return Err(other),
        }
    }
    Err(TactusError::Io {
        path: path.to_path_buf(),
        source: last.unwrap_or_else(|| {
            std::io::Error::other("the record could not be read and reported no reason")
        }),
    })
}

/// Every intent record under `<R>/containers`, sorted by name.
///
/// "discovery at every write-command start **scans the whole namespace
/// `<R>/containers`** of the command's authorized private root **and** docker
/// ps by `tactus.private_root`" — this is the first half; the second is
/// [`runtime::ContainerRuntime::containers_with_label`]. A missing directory is
/// an empty namespace, not an error: a run that has never launched a container
/// has none.
///
/// The staged `<name>.intent.tmp` half is skipped: it is writer-owned residue
/// that no reader may adopt, exactly as `Answer.StageWrite`'s `.partial` is.
///
/// # Errors
///
/// [`TactusError::Io`] when the directory cannot be read, or whatever
/// [`read_intent`] returns.
pub fn list_intents(private_root: &Path) -> Result<Vec<FoundIntent>, TactusError> {
    let dir = containers_dir(private_root);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(TactusError::Io { path: dir, source }),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| TactusError::Io {
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
        // A record that vanished between the directory read and this one is a
        // record another reclaimer removed, and that is not an error: "every
        // step idempotent and tolerant of already-gone so **two concurrent
        // reclaimers converge**".
        //
        // Measured by lane C, not reasoned. With a bare `?` here,
        // `census::tests::concurrent_reclaimers_converge` refused with
        // `Io { NotFound }` on Linux in 2 of 20 runs, and with
        // `Io { PermissionDenied }` on the Windows guest — a whole write
        // command failing because another write command was tidying at the same
        // moment. A **malformed** record is still an error: "the record could
        // not be parsed" and "the record is gone" are different answers, and
        // only one of them licenses proceeding.
        let Some(record) = read_racing(&path)? else {
            continue;
        };
        found.push(FoundIntent { name, path, record });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// A runtime failure, as the engine's error type.
fn refused(error: RuntimeError) -> TactusError {
    TactusError::Refused {
        message: error.to_string(),
    }
}

/// `<name>.intent` -> `<name>.intent.tmp`.
fn staged_path(path: &Path) -> PathBuf {
    let mut staged = path.as_os_str().to_owned();
    staged.push(".tmp");
    PathBuf::from(staged)
}

/// Write `bytes` durably: stage, fsync, rename, fsync the directory.
///
/// `run_creation`'s own four steps, each recorded in `trace` beside the
/// primitive that performs it. The file and directory barriers are
/// [`util::fsync_file`] and [`util::fsync_dir`] — the one call each that
/// `effects::tests::every_file_durability_barrier_in_a_funnel_module_goes_through_one_call`
/// censuses — rather than a `sync_all` of this module's own.
fn write_synced(path: &Path, bytes: &[u8], trace: &ContainerTrace) -> Result<(), TactusError> {
    let parent = path.parent().ok_or_else(|| TactusError::Git {
        message: format!("{} has no parent directory", path.display()),
    })?;
    fs::create_dir_all(parent).map_err(|source| TactusError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let staged = staged_path(path);
    {
        let mut file = fs::File::create(&staged).map_err(|source| TactusError::Io {
            path: staged.clone(),
            source,
        })?;
        file.write_all(bytes).map_err(|source| TactusError::Io {
            path: staged.clone(),
            source,
        })?;
        util::fsync_file(&file).map_err(|source| TactusError::Io {
            path: staged.clone(),
            source,
        })?;
    }
    trace.durable(DurableStep::Synced, &staged);
    fs::rename(&staged, path).map_err(|source| TactusError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    trace.durable(DurableStep::Renamed, path);
    util::fsync_dir(parent).map_err(|source| TactusError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    trace.durable(DurableStep::DirSynced, parent);
    Ok(())
}

/// Remove a file that may not be there, or may be going away under another
/// reclaimer.
fn remove_if_present(path: &Path, trace: &ContainerTrace) -> Result<(), TactusError> {
    if racing_removal(path, || fs::remove_file(path))? {
        trace.durable(DurableStep::Removed, path);
    }
    Ok(())
}

/// Perform `remove` until the path is gone, however it went.
///
/// Answers whether **this** caller was the one that removed it, so a trace
/// records the removal once rather than once per reclaimer.
///
/// See [`RACING_ACCESS_ATTEMPTS`] for why this is not `if kind() == NotFound`.
///
/// # Errors
///
/// [`TactusError::Io`] when the path is still there, and still refusing, after
/// [`RACING_ACCESS_ATTEMPTS`] attempts.
fn racing_removal(
    path: &Path,
    mut remove: impl FnMut() -> Result<(), std::io::Error>,
) -> Result<bool, TactusError> {
    let mut last = None;
    for _ in 0..RACING_ACCESS_ATTEMPTS {
        match remove() {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                last = Some(error);
                std::thread::yield_now();
            }
        }
    }
    Err(TactusError::Io {
        path: path.to_path_buf(),
        source: last.unwrap_or_else(|| {
            std::io::Error::other("the path could not be removed and reported no reason")
        }),
    })
}

// ---------------------------------------------------------------------------
// The real runtime: the `docker` CLI
// ---------------------------------------------------------------------------

/// The program name. `non_goals[3]` is "remote runners", so this is the local
/// CLI and nothing configures a socket.
pub const DOCKER_PROGRAM: &str = "docker";

/// The `docker` CLI.
///
/// Every process it starts is a **coordinator-side control-plane** call, and
/// deliberately does **not** go through the Runner: DESIGN.md:612 is "Workers,
/// repository-controlled gates, and reviewers all cross the boundary;
/// authoritative Git and the event log never do", and asking the container
/// runtime what it holds is the same kind of thing as authoritative Git — it is
/// how the boundary is *built*, so it cannot execute inside it.
/// `runner::tests::every_production_process_start_is_classified` carries the
/// row that says so.
#[derive(Debug, Clone, Default)]
pub struct DockerCli {
    trace: ContainerTrace,
}

impl DockerCli {
    /// A CLI whose operations are recorded in `trace`.
    #[must_use]
    pub fn new(trace: ContainerTrace) -> Self {
        Self { trace }
    }

    /// Whether `docker` is on this machine and its daemon answers.
    ///
    /// Two questions and not one, because `docker` exits **non-zero** with a
    /// daemon-unreachable message when the binary is present and the daemon is
    /// not — the same shape as `codex login status`, whose exit code and output
    /// disagree.
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

    /// Run one `docker` subcommand and capture it.
    ///
    /// `target` is what the call log names — the container, image or volume the
    /// operation is about, rather than the subcommand, so the trace of a real
    /// run and the trace of a fake one are the same shape.
    fn exec(&self, op: RuntimeOp, target: &str, args: &[&str]) -> Result<String, RuntimeError> {
        self.exec_streams(op, target, args)
            .map(|(stdout, _)| String::from_utf8_lossy(&stdout).into_owned())
    }

    /// The same call, keeping **both** streams.
    ///
    /// `PR6A-DOCKERCLI-MERGES-STDERR-INTO-STDOUT`: [`Self::exec`] returns only
    /// stdout and drops the child's stderr on success, so every container
    /// invocation's stderr was lost. A gate's failure output is its stderr and
    /// becomes retry feedback (DESIGN.md:576); both shipped adapters read
    /// stderr; `host::run_shell_probe`'s refusal quotes `output.stderr` and
    /// would have quoted nothing. Measured on docker 29.7.2, `docker logs`
    /// **separates** the two streams — `docker logs <c> 2>/dev/null` gives the
    /// container's stdout and `2>&1 >/dev/null` gives its stderr — so there was
    /// never anything to merge and the previous comment's premise was wrong as
    /// well as its consequence.
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

    /// `docker inspect` a thing, tolerating "no such object" as absence.
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

    /// An image inspection from `docker image inspect`'s Go-template output.
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

/// What `docker image inspect` reported, before it becomes an
/// [`runtime::ImageInspection`].
struct ImageInspectionRaw {
    id: String,
    digests: Vec<String>,
    tags: Vec<String>,
}

impl ImageInspectionRaw {
    fn into_inspection(self) -> runtime::ImageInspection {
        runtime::ImageInspection {
            id: self.id,
            // "the manifest digest **when reported**". An image built locally
            // and never pushed has no repo digest, and `None` is the record
            // INV-23 asks for in that case.
            digest: self
                .digests
                .into_iter()
                .next()
                .and_then(|entry| entry.rsplit('@').next().map(str::to_owned)),
            references: self.tags,
        }
    }
}

/// A comma-separated Go-template list, with the empty case as an empty vector.
fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The field separator of [`PS_FORMAT`]'s output.
///
/// `U+001F` INFORMATION SEPARATOR ONE, the byte its name is for. It is not a
/// character any of this engine's own label values carry — a path label is
/// percent-encoded ASCII, a run id and an incarnation are ULIDs, an invocation
/// is `[0-9A-Za-z._]` — but a **foreign** container carrying this private
/// root's label may carry anything at all, so [`parse_ps_output`] treats a line
/// with the wrong field count as unreadable evidence and refuses rather than
/// trusting a mis-split.
const PS_FIELD_SEPARATOR: char = '\u{1f}';

/// `docker ps --format` for the census: **one placeholder per label**.
///
/// `{{.Labels}}` renders every label of a container as an **unescaped
/// comma-joined `key=value` string**, and a label value may itself contain
/// commas and `=`. Measured on this box, `docker` 29.7.2, a container labelled
/// `tactus.run=RUN1` and `tactus.run_dir=/a,b=c`:
///
/// ```text
/// --format '{{.Names}}|{{.Labels}}'      -> r2probe|tactus.run=RUN1,tactus.run_dir=/a,b=c
/// --format '{{.Label "tactus.run_dir"}}' -> /a,b=c
/// ```
///
/// The first is ambiguous *in the daemon's own output* — `tactus.run_dir=/a`
/// and a label `b=c` is an equally good reading of those bytes, and it is the
/// reading the shipped `split(',')` took. For `tactus.run_dir` that truncation
/// sends arm (ii)'s probe to a shorter, different directory, finds no
/// `run.lock`, and reclaims a **live** owner's container (`PR6-RECOV-002`). So
/// this asks the daemon for each field on its own, which is the one rendering
/// that cannot be ambiguous, rather than parsing a format that permits its own
/// delimiter inside a field.
///
/// All five labels, not the three the census reads today: the discovered
/// container's map is the same shape it always was, so a later reader is not
/// silently handed a map that lost the two nobody happened to need.
const PS_FORMAT: &str = "{{.Names}}\u{1f}{{.Label \"tactus.private_root\"}}\
     \u{1f}{{.Label \"tactus.run\"}}\u{1f}{{.Label \"tactus.run_dir\"}}\
     \u{1f}{{.Label \"tactus.incarnation\"}}\u{1f}{{.Label \"tactus.invocation\"}}";

/// The labels [`PS_FORMAT`] asks for, in the order it asks for them.
///
/// Written out beside the format string and checked against it by
/// `tests::the_ps_format_asks_for_exactly_the_labels_the_parser_names`, so a
/// placeholder added to one and not the other is a failing test rather than a
/// field read under the wrong name.
const PS_LABELS: &[&str] = &[
    intent::LABEL_PRIVATE_ROOT,
    intent::LABEL_RUN,
    intent::LABEL_RUN_DIR,
    intent::LABEL_INCARNATION,
    intent::LABEL_INVOCATION,
];

/// `docker ps`'s answer, one container per line.
///
/// **A line whose field count is not `1 + PS_LABELS.len()` is refused**, and
/// that is the fail-closed half of [`PS_FIELD_SEPARATOR`]: a label value
/// carrying the separator splits into too many fields and one carrying a
/// newline splits its container across two lines with too few, and either way
/// the values no longer line up with the names. Guessing which field is which
/// at that point is how a census probes the wrong lock.
///
/// An **empty** rendered value is recorded as an absent label rather than as a
/// present empty one, because `{{.Label "x"}}` renders both as the empty string
/// and this side cannot tell them apart. Both are refused downstream —
/// `census::from_labels_alone` refuses a missing ownership label and
/// `intent::owner_run_dir` refuses an empty run directory — so the collapse
/// costs a diagnostic's precision and never an admission.
///
/// # Errors
///
/// [`RuntimeError::Failed`] naming the line, when a line does not carry exactly
/// the fields [`PS_FORMAT`] asks for. `Failed` and not `Unreachable`: the
/// daemon answered.
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

/// The diagnostics that mean **the daemon was never reached**, lower-cased.
///
/// `crash_reconstruction` hangs a whole branch on this distinction: "the
/// container runtime is required only when an intent exists or a labeled
/// container is discoverable … with **no intent and no reachable runtime it
/// proceeds**". A machine with `docker` installed and the daemon not reachable
/// by *this* process is the ordinary configuration, not a fault, and every
/// diagnostic below is one this engine must proceed past when it holds no
/// container evidence. Getting it wrong the other way — classifying "cannot
/// reach" as "reached and refused" — makes `tactus run` refuse to start at all
/// on a machine that has no container work to do (`PR6-RECOV-005`).
///
/// **Measured, not recalled.** Every entry was produced on this project's build
/// box against `docker` 29.7.2 and is transcribed from that run's stderr; the
/// command that produced each is beside it, and
/// `tests::the_transcribed_unreachable_diagnostics_are_what_the_live_cli_prints`
/// replays two of them through the real CLI so the table's oracle is the daemon
/// rather than this list.
///
/// | # | command | stderr |
/// |---|---|---|
/// | 1 | `sudo -u nobody docker ps` | `permission denied while trying to connect to the docker API at unix:///var/run/docker.sock` |
/// | 2 | `DOCKER_HOST=unix:///nonexistent/docker.sock docker ps` | `failed to connect to the docker API at unix:///nonexistent/docker.sock; check if the path is correct and if the daemon is running: dial unix …: connect: no such file or directory` |
/// | 3 | `DOCKER_HOST=tcp://127.0.0.1:1 docker ps` | `Cannot connect to the Docker daemon at tcp://127.0.0.1:1. Is the docker daemon running?` |
///
/// Rows 1 **and** 2 were both classified `Failed` by the three-string test this
/// replaced: row 1 is the socket-permission case the review reproduced, and row
/// 2 — `docker` 29's wording for an absent socket — is the shape a machine with
/// the daemon stopped produces, so the misclassification was not the rare case
/// it looked like. Row 2 does contain the words "if the daemon is running", but
/// the shipped predicate looked for the older `Is the docker daemon running`,
/// case-sensitively, and matched neither.
///
/// **Windows** keeps `error during connect` and the named-pipe wording it
/// wraps (`open //./pipe/docker_engine: The system cannot find the file
/// specified`), which is why that entry stays even though no row above produced
/// it on Linux.
///
/// Nothing here may overlap [`stop_already_settled`]'s `is not running`, which
/// is a **reached** daemon reporting a container's state; entries are whole
/// connection phrases for that reason, and
/// `tests::the_two_docker_diagnostic_tables_never_claim_one_message` holds the
/// two tables apart.
const UNREACHABLE_DIAGNOSTICS: &[&str] = &[
    "cannot connect to the docker daemon",
    "error during connect",
    "is the docker daemon running",
    "if the daemon is running",
    "permission denied while trying to connect",
    "failed to connect to the docker api",
    "the docker daemon is not running",
];

/// Whether a `docker` failure means the daemon was never reached.
///
/// Case-insensitive: `docker` 29 lower-cased "docker API" where earlier
/// versions wrote "Docker daemon", and a classification that turns on the case
/// of a vendor's prose is a classification that breaks on the next release.
///
/// See [`UNREACHABLE_DIAGNOSTICS`] for the measured table and for why this is a
/// named function with its own tests rather than a `matches!` at the call site:
/// every fake in this slice injects [`RuntimeError::Unreachable`] *directly*,
/// so nothing but a test of this function tests the thing that decides it.
#[must_use]
pub fn is_unreachable_diagnostic(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    UNREACHABLE_DIAGNOSTICS
        .iter()
        .any(|shape| lower.contains(shape))
}

/// One failed `docker` invocation, as the seam's error.
///
/// A free function taking the raw stderr rather than a branch inside
/// [`DockerCli::exec_streams`], for the reason [`settle_stop`] is one: the
/// classification is reachable — and testable — **without a daemon**, and the
/// census tests can hand a fake runtime the error a verbatim diagnostic really
/// produces instead of asserting on an `Unreachable` they minted themselves.
#[must_use]
pub fn classify_docker_failure(operation: RuntimeOp, detail: String) -> RuntimeError {
    if is_unreachable_diagnostic(&detail) {
        return RuntimeError::Unreachable { operation, detail };
    }
    RuntimeError::Failed { operation, detail }
}

/// Whether a `docker` failure means "the object is not there".
fn is_absent(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("no such object")
        || lower.contains("no such container")
        || lower.contains("no such image")
        || lower.contains("no such volume")
        || lower.contains("is already in progress")
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
        // `docker image inspect` resolves a *prefix* of an id and a tag alike,
        // so an answer whose id is not the value asked for is not an answer to
        // this question. The rebuild path's refusal is "the recorded image id
        // is absent from the runtime", and a different id present is exactly
        // that.
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
        // Both streams, separately. `docker logs` writes the container's stdout
        // to its own stdout and the container's stderr to its own stderr
        // (measured, docker 29.7.2) — see [`Self::exec_streams`].
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
        // The **image id**, never a reference (INV-23).
        args.push(spec.image_id.clone());
        args.extend(spec.command.iter().cloned());
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.exec(RuntimeOp::Create, &spec.name, &borrowed)?;
        // The id the runtime says it used, read back from the created
        // container. Never `spec.image_id`: the whole point of the check the
        // caller then performs is that these two can differ.
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
        // `--volumes` (`PR6A-ANONYMOUS-VOLUMES-LEAK`). An image declaring
        // `VOLUME` gets an **anonymous** volume per container, and `docker rm`
        // without it leaves one behind per invocation: measured, **29** leaked
        // from one run of this suite. Those volumes are not R20 — R20 is the
        // operator's *named*, per-agent credential volume, "operator_owned" and
        // `persistent_output` in all five `at_run_end` outcomes. An anonymous
        // volume is created by `docker create` as part of the container and
        // nothing else can ever refer to it, so it belongs to **R26** ("container
        // + labels + global intent"), and `Container.Remove` is where R26
        // balances. `--volumes` removes only anonymous volumes attached to this
        // container and **never a named one**, which is what makes reclaiming it
        // here a discharge of R26 rather than a violation of R20.
        match self.exec(
            RuntimeOp::Remove,
            name,
            &["rm", "--force", "--volumes", name],
        ) {
            Ok(_) => Ok(()),
            Err(RuntimeError::Failed { detail, .. }) if is_absent(&detail) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Whether a `docker stop` / `docker kill` failure means the container has
/// already reached the state the caller asked for.
///
/// "every step idempotent and tolerant of already-gone so **two concurrent
/// reclaimers converge**" has two shapes on a real daemon, and only one of them
/// is "gone". Measured on `docker` 29.7.2, verbatim:
///
/// ```text
/// docker kill <exited> -> Error response from daemon: cannot kill container: <n>: container <id> is not running
/// docker kill <absent> -> Error response from daemon: cannot kill container: <n>: No such container: <n>
/// docker stop <absent> -> Error response from daemon: No such container: <n>
/// docker stop <exited> -> succeeds, and prints the name
/// ```
///
/// The **first** row is the load-bearing one and it is what a *racing*
/// reclaimer sees: A kills the container, B reaches `docker kill` after the
/// state has become `Exited`, and without this tolerance B returns an error
/// before observe / rm / view / intent cleanup — the opposite of the sentence.
/// `PR6-LANEF-003` is the entry for it: deleting the tolerance passed every
/// test, because every fixture serialized the reclaimers.
fn stop_already_settled(detail: &str) -> bool {
    is_absent(detail) || detail.contains("is not running")
}

/// `docker stop` / `docker kill`'s raw answer, as the seam's.
///
/// A free function taking the raw outcome rather than a `match` inside
/// [`DockerCli::stop`], so the tolerance is reachable **without a daemon**: the
/// branch that matters is one a real runtime only produces in a race, and a
/// gated test is the wrong and only place it could otherwise be observed.
fn settle_stop(outcome: Result<String, RuntimeError>) -> Result<(), RuntimeError> {
    match outcome {
        Ok(_) => Ok(()),
        Err(RuntimeError::Failed { detail, .. }) if stop_already_settled(&detail) => Ok(()),
        Err(error) => Err(error),
    }
}

/// One `--mount` argument.
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
    }
    if mount.read_only() {
        parts.push("readonly".to_owned());
    }
    parts.join(",")
}

// -- test-only declarations ----------------------------------------------
// At the BOTTOM, deliberately: `effects::production_region` cuts a source at
// its first `#[cfg(test)]`, so a test module declared above would remove every
// funnel in this file from the census that proves the Container group has one
// (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).
#[cfg(test)]
mod fake;

#[cfg(test)]
pub(crate) use fake::{
    DOCKER_GATED_TESTS, FakeOwnerLiveness, FakeRuntime, RecordingHooks, docker_gate,
};

/// One `docker` subcommand, raw, for the Docker-gated tests only.
///
/// Two of them need what the seam deliberately does not expose: the daemon's
/// **verbatim** stderr for an already-stopped container, so the transcribed
/// table in `tests` can be checked against the live daemon rather than against
/// itself; and a container carrying an **anonymous** volume, which `CreateSpec`
/// cannot express because `Mount::Volume` requires a name.
///
/// Below the `#[cfg(test)]` cut deliberately. `effects::production_region` stops
/// at the first `#[cfg(test)]` in a file, so this is invisible to every source
/// census — including `exec::tests::the_container_subtree_can_only_inspect_a_volume`,
/// which is the census that keeps production able to *inspect* a volume and
/// nothing else. A test fixture is not a production capability, and this is
/// where the tree draws that line.
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
