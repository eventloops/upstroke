//! The deterministic test substrate: the fake container runtime, the fake owner
//! liveness probe, a recording hooks observer, and the Docker gate.
//!
//! `decisions.tests_acceptance.determinism` specifies the fake **exactly**:
//!
//! > a fake container runtime with (6) owner labels, incarnations, liveness
//! > simulation, (1) an image table keyed by immutable id with references and
//! > digests, (2) a mutable tag table (a reference can be moved to another id),
//! > (3) per-container reported image ids with substitution injection, (4)
//! > volume presence toggles, and (5) an availability toggle for ST-16 and
//! > ST-20 plus Docker-gated real runs
//!
//! All six are here and each is exercised by at least one test that fails
//! without it; `runner::container::tests` says which, by name, in the doc
//! comment of each.
//!
//! ## The correlated-fixture trap, which this module is built around
//!
//! If a helper set a container's **reported** image id from the id it was
//! **created from**, no test in this slice could construct a substitution and
//! `substituted_image_id_refused_before_start` would be green because it could
//! not be written. So [`FakeRuntime::seed_container`] takes the two as
//! **separate arguments**, [`FakeRuntime::substitute_reported_image_id`] is the
//! injection point at create, and
//! `runner::container::tests::the_fake_can_report_an_image_id_that_differs_from_the_one_create_asked_for`
//! proves they can differ.

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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::runtime::{
    ContainerExecution, ContainerRuntime, ContainerTrace, CreateSpec, CreatedContainer,
    DiscoveredContainer, ImageInspection, Liveness, OwnerLiveness, RuntimeError, RuntimeOp,
    StopMode,
};
use super::{ContainerHooks, DockerCli};
use crate::topology::effects::{EffectSiteId, HookPhase, Injection};

// ---------------------------------------------------------------------------
// The fake runtime
// ---------------------------------------------------------------------------

/// One image in the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FakeImage {
    /// "the manifest digest **when reported**" — `None` is a real state, not a
    /// missing fixture.
    pub digest: Option<String>,
}

/// One container the fake holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FakeContainer {
    pub labels: BTreeMap<String, String>,
    /// The id `create` was **asked** for.
    pub requested_image_id: String,
    /// The id the runtime **reports**. A separate field, deliberately: see the
    /// module docs.
    pub reported_image_id: String,
    pub state: Liveness,
    pub execution: ContainerExecution,
}

#[derive(Debug, Default)]
struct State {
    /// (1) The image table, keyed by **immutable id**.
    images: BTreeMap<String, FakeImage>,
    /// (2) The **mutable** tag table: reference -> image id.
    tags: BTreeMap<String, String>,
    /// (4) Volume presence.
    volumes: BTreeSet<String>,
    /// (5) The availability toggle — per operation, because a runtime that
    /// answers `ps` and fails `inspect` is a real state (see
    /// `super::runtime`'s module docs).
    unreachable: BTreeSet<RuntimeOp>,
    /// An operation that reaches the runtime and fails, which is a different
    /// answer from unreachable and drives the other half of the refusal split.
    failing: BTreeSet<RuntimeOp>,
    /// A verbatim `docker` stderr this operation answers with, classified by
    /// the production classifier rather than by the test.
    ///
    /// `PR6-RECOV-005`: every other arming here mints a `RuntimeError` variant
    /// **directly**, so the whole suite could pass while the function that
    /// decides which variant a real diagnostic becomes was wrong. This is the
    /// one arming that goes through `super::classify_docker_failure`.
    diagnostics: BTreeMap<RuntimeOp, String>,
    /// (6) Containers, with their owner labels and incarnations.
    containers: BTreeMap<String, FakeContainer>,
    /// (3) Substitution injection: what `create` will report for this name.
    substitutions: BTreeMap<String, String>,
}

/// The fake container runtime.
///
/// Interior-mutable so it can be handed out as `&dyn ContainerRuntime` and
/// still be armed and inspected by the test that holds it.
#[derive(Debug, Default)]
pub(crate) struct FakeRuntime {
    state: Mutex<State>,
    trace: ContainerTrace,
}

impl FakeRuntime {
    /// A runtime holding nothing, recording into `trace`.
    pub(crate) fn new(trace: ContainerTrace) -> Self {
        Self {
            state: Mutex::new(State::default()),
            trace,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Record the call and answer whether the operation may proceed.
    fn enter(&self, op: RuntimeOp, target: &str) -> Result<(), RuntimeError> {
        self.trace.runtime(op, target);
        let state = self.state();
        if let Some(detail) = state.diagnostics.get(&op) {
            return Err(super::classify_docker_failure(op, detail.clone()));
        }
        if state.unreachable.contains(&op) {
            return Err(RuntimeError::Unreachable {
                operation: op,
                detail: "the fake runtime is armed unreachable for this operation".to_owned(),
            });
        }
        if state.failing.contains(&op) {
            return Err(RuntimeError::Failed {
                operation: op,
                detail: "the fake runtime is armed failing for this operation".to_owned(),
            });
        }
        Ok(())
    }

    // -- (1) the image table, keyed by immutable id --------------------------

    /// Put an image in the table under its **id**, with its digest or without
    /// one.
    pub(crate) fn add_image(&self, id: &str, digest: Option<&str>) {
        self.state().images.insert(
            id.to_owned(),
            FakeImage {
                digest: digest.map(str::to_owned),
            },
        );
    }

    // -- (2) the mutable tag table -------------------------------------------

    /// Point a reference at an image id.
    pub(crate) fn tag(&self, reference: &str, id: &str) {
        self.state()
            .tags
            .insert(reference.to_owned(), id.to_owned());
    }

    /// **Move** an existing reference to another id, leaving the old id in the
    /// table.
    ///
    /// ST-20: "a resume after the recorded reference was moved to another image
    /// warns and creates every container from the recorded id". The old id must
    /// stay resolvable or that sentence has no fixture.
    pub(crate) fn move_tag(&self, reference: &str, new_id: &str) {
        let mut state = self.state();
        assert!(
            state.tags.contains_key(reference),
            "`{reference}` is not tagged, so it cannot be moved; \
             a moved-reference fixture that started from nothing would be testing itself"
        );
        state.tags.insert(reference.to_owned(), new_id.to_owned());
    }

    // -- (3) reported image ids with substitution injection ------------------

    /// Arm `create` to report `reported` for the container called `name`,
    /// whatever id it is asked to create from.
    pub(crate) fn substitute_reported_image_id(&self, name: &str, reported: &str) {
        self.state()
            .substitutions
            .insert(name.to_owned(), reported.to_owned());
    }

    // -- (4) volume presence toggles -----------------------------------------

    /// Make a volume present.
    pub(crate) fn add_volume(&self, name: &str) {
        self.state().volumes.insert(name.to_owned());
    }

    /// Make a volume absent.
    pub(crate) fn remove_volume(&self, name: &str) {
        self.state().volumes.remove(name);
    }

    // -- (5) the availability toggle -----------------------------------------

    /// Arm one operation unreachable.
    pub(crate) fn set_unreachable(&self, op: RuntimeOp) {
        self.state().unreachable.insert(op);
    }

    /// Arm every operation unreachable — the whole daemon being down.
    pub(crate) fn set_all_unreachable(&self) {
        let mut state = self.state();
        for op in RuntimeOp::ALL {
            state.unreachable.insert(*op);
        }
    }

    /// Make one operation reachable again.
    pub(crate) fn set_reachable(&self, op: RuntimeOp) {
        self.state().unreachable.remove(&op);
    }

    /// Arm one operation to answer with a **verbatim `docker` stderr**, whose
    /// classification is the production classifier's rather than the test's.
    pub(crate) fn set_docker_stderr(&self, op: RuntimeOp, detail: &str) {
        self.state().diagnostics.insert(op, detail.to_owned());
    }

    /// Arm one operation to reach the runtime and fail.
    pub(crate) fn set_failing(&self, op: RuntimeOp) {
        self.state().failing.insert(op);
    }

    // -- (6) owner labels, incarnations ---------------------------------------

    /// Put a container in the table.
    ///
    /// `requested_image_id` and `reported_image_id` are **two arguments** and
    /// never one: see the module docs.
    pub(crate) fn seed_container(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
        requested_image_id: &str,
        reported_image_id: &str,
        state: Liveness,
    ) {
        self.state().containers.insert(
            name.to_owned(),
            FakeContainer {
                labels,
                requested_image_id: requested_image_id.to_owned(),
                reported_image_id: reported_image_id.to_owned(),
                state,
                execution: ContainerExecution {
                    exit_code: Some(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
            },
        );
    }

    /// Move a container to another liveness state.
    pub(crate) fn set_container_state(&self, name: &str, state: Liveness) {
        if let Some(container) = self.state().containers.get_mut(name) {
            container.state = state;
        }
    }

    /// Give a container an exit status and output.
    pub(crate) fn set_execution(&self, name: &str, execution: ContainerExecution) {
        if let Some(container) = self.state().containers.get_mut(name) {
            container.execution = execution;
        }
    }

    /// What the fake holds for `name`, if anything.
    pub(crate) fn container(&self, name: &str) -> Option<FakeContainer> {
        self.state().containers.get(name).cloned()
    }

    /// Every container name the fake holds, sorted.
    pub(crate) fn container_names(&self) -> Vec<String> {
        self.state().containers.keys().cloned().collect()
    }

    /// The ordered call log — every operation this runtime was asked to
    /// perform.
    ///
    /// Most of this slice's obligations are orderings, and "a suite that proves
    /// the *set* of operations happened without pinning their order holds none
    /// of them". The log shares its handle with the funnel's trace, so a single
    /// sequence contains the funnel phases, the durability steps and these.
    pub(crate) fn calls(&self) -> Vec<RuntimeOp> {
        self.trace.ops()
    }
}

impl ContainerRuntime for FakeRuntime {
    fn probe(&self) -> Result<(), RuntimeError> {
        self.enter(RuntimeOp::Probe, "daemon")
    }

    fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        self.enter(RuntimeOp::InspectImageByReference, reference)?;
        let state = self.state();
        let Some(id) = state.tags.get(reference) else {
            return Ok(None);
        };
        Ok(state.images.get(id).map(|image| ImageInspection {
            id: id.clone(),
            digest: image.digest.clone(),
            references: state
                .tags
                .iter()
                .filter(|(_, target)| *target == id)
                .map(|(name, _)| name.clone())
                .collect(),
        }))
    }

    fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        self.enter(RuntimeOp::InspectImageById, id)?;
        let state = self.state();
        Ok(state.images.get(id).map(|image| ImageInspection {
            id: id.to_owned(),
            digest: image.digest.clone(),
            references: state
                .tags
                .iter()
                .filter(|(_, target)| target.as_str() == id)
                .map(|(name, _)| name.clone())
                .collect(),
        }))
    }

    fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
        self.enter(RuntimeOp::InspectVolume, name)?;
        Ok(self.state().volumes.contains(name))
    }

    fn containers_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
        self.enter(RuntimeOp::ListByLabel, value)?;
        Ok(self
            .state()
            .containers
            .iter()
            .filter(|(_, container)| container.labels.get(key).map(String::as_str) == Some(value))
            .map(|(name, container)| DiscoveredContainer {
                name: name.clone(),
                labels: container.labels.clone(),
            })
            .collect())
    }

    fn observe(&self, name: &str) -> Result<Liveness, RuntimeError> {
        self.enter(RuntimeOp::Observe, name)?;
        Ok(self
            .state()
            .containers
            .get(name)
            .map_or(Liveness::Gone, |container| container.state))
    }

    fn collect(&self, name: &str) -> Result<ContainerExecution, RuntimeError> {
        self.enter(RuntimeOp::Collect, name)?;
        self.state()
            .containers
            .get(name)
            .map(|container| container.execution.clone())
            .ok_or_else(|| RuntimeError::Failed {
                operation: RuntimeOp::Collect,
                detail: format!("`{name}` is gone"),
            })
    }

    fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {
        self.enter(RuntimeOp::Create, &spec.name)?;
        let mut state = self.state();
        if !state.images.contains_key(&spec.image_id) {
            return Err(RuntimeError::Failed {
                operation: RuntimeOp::Create,
                detail: format!("no image with id `{}`", spec.image_id),
            });
        }
        if state.containers.contains_key(&spec.name) {
            return Err(RuntimeError::Failed {
                operation: RuntimeOp::Create,
                detail: format!("a container named `{}` already exists", spec.name),
            });
        }
        for mount in &spec.mounts {
            if let super::runtime::Mount::Volume { name, .. } = mount {
                if !state.volumes.contains(name) {
                    return Err(RuntimeError::Failed {
                        operation: RuntimeOp::Create,
                        detail: format!("no volume named `{name}`"),
                    });
                }
            }
        }
        // The reported id is a **separate input** from the requested one. With
        // nothing injected the healthy runtime reports what it was asked for;
        // with an injection it reports something else, and that is the only
        // reason `substituted_image_id_refused_before_start` is constructible.
        let reported = state
            .substitutions
            .get(&spec.name)
            .cloned()
            .unwrap_or_else(|| spec.image_id.clone());
        state.containers.insert(
            spec.name.clone(),
            FakeContainer {
                labels: spec.labels.clone(),
                requested_image_id: spec.image_id.clone(),
                reported_image_id: reported.clone(),
                state: Liveness::Exited,
                execution: ContainerExecution {
                    exit_code: Some(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
            },
        );
        Ok(CreatedContainer {
            name: spec.name.clone(),
            reported_image_id: reported,
        })
    }

    fn start(&self, name: &str) -> Result<(), RuntimeError> {
        self.enter(RuntimeOp::Start, name)?;
        let mut state = self.state();
        let Some(container) = state.containers.get_mut(name) else {
            return Err(RuntimeError::Failed {
                operation: RuntimeOp::Start,
                detail: format!("no such container `{name}`"),
            });
        };
        container.state = Liveness::Running;
        Ok(())
    }

    fn stop(&self, name: &str, _mode: StopMode) -> Result<(), RuntimeError> {
        self.enter(RuntimeOp::Stop, name)?;
        // Idempotent and tolerant of already-gone: a container the fake does
        // not hold is a stop that succeeded, because two concurrent reclaimers
        // must converge.
        if let Some(container) = self.state().containers.get_mut(name) {
            container.state = Liveness::Exited;
        }
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), RuntimeError> {
        self.enter(RuntimeOp::Remove, name)?;
        self.state().containers.remove(name);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// (6b) Liveness simulation
// ---------------------------------------------------------------------------

/// The fake owner-liveness probe.
///
/// `determinism` lists "liveness simulation" among the fake container runtime's
/// capabilities; in this tree it is a **separate seam**, because liveness is a
/// non-blocking probe of another run's `run.lock` on this host
/// (`crash_reconstruction`: "probe that run's run.lock non-blocking (is_running
/// semantics: src/rundir.rs:619-652)") and not a question the container runtime
/// could answer. Splitting it is also what makes "the coordinator incarnation
/// id … is **never read from lock-file contents**" structurally true: the
/// answer is one bit and carries no incarnation to read.
#[derive(Debug, Default)]
pub(crate) struct FakeOwnerLiveness {
    live: Mutex<BTreeSet<PathBuf>>,
}

impl FakeOwnerLiveness {
    /// Nobody is alive.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Say that a coordinator holds this public run directory's lock.
    pub(crate) fn set_live(&self, public_run_dir: &Path) {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(public_run_dir.to_path_buf());
    }

    /// Say that it does not.
    pub(crate) fn set_dead(&self, public_run_dir: &Path) {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(public_run_dir);
    }
}

impl OwnerLiveness for FakeOwnerLiveness {
    fn is_running(&self, public_run_dir: &Path) -> bool {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(public_run_dir)
    }
}

// ---------------------------------------------------------------------------
// A recording observer
// ---------------------------------------------------------------------------

/// Hooks that record into a trace and can be armed to fail at one site phase.
///
/// The fault arm is the error-return mode at a hook phase, which is what proves
/// a partial sequence converges — an `Err` from `After` is returned *after* the
/// primitive ran.
#[derive(Debug, Default)]
pub(crate) struct RecordingHooks {
    trace: ContainerTrace,
    armed: Option<(EffectSiteId, HookPhase)>,
}

impl RecordingHooks {
    /// An observer sharing `trace` with the runtime.
    pub(crate) fn new(trace: ContainerTrace) -> Self {
        Self { trace, armed: None }
    }

    /// Make the funnel return `Err` at one phase of one site.
    pub(crate) fn fail_at(&mut self, site: EffectSiteId, phase: HookPhase) {
        self.armed = Some((site, phase));
    }
}

impl ContainerHooks for RecordingHooks {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        if self.armed == Some((site, phase)) {
            return Injection::Error;
        }
        Injection::Proceed
    }

    fn trace(&self) -> ContainerTrace {
        self.trace.clone()
    }
}

// ---------------------------------------------------------------------------
// The Docker gate — loud and counted, never silent
// ---------------------------------------------------------------------------

/// The environment variable that turns a skip into a failure.
///
/// A gated suite that skips silently everywhere is the "green because the test
/// could not run" failure this project keeps paying for. On a machine with
/// Docker the suite is run **with this set**, so a skip is a red test; CI and
/// the Windows guest have no container runtime and run without it.
pub(crate) const REQUIRE_DOCKER: &str = "UPSTROKE_REQUIRE_DOCKER";

/// Every Docker-gated test in this slice, by name.
///
/// Written out rather than counted, because *which* gated test disappeared is
/// the finding. `runner::container::tests::every_docker_gated_test_is_named_and_present`
/// asserts each name is a `fn` in the container sources, so a gated test that is
/// deleted or renamed fails rather than silently leaving the list shorter.
///
/// **Lanes A, B and C: append your gated test's name here.**
pub(crate) const DOCKER_GATED_TESTS: &[&str] = &[
    "real_docker_reports_an_image_id_and_a_digest_for_a_reference_it_holds",
    "real_docker_refuses_a_reference_it_does_not_hold_without_pulling",
    "real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently",
    // Repair round F1: the three defects real Docker found in this file.
    "real_docker_kill_on_an_already_exited_container_is_tolerated",
    "real_docker_returns_both_streams_of_a_container_separately",
    "real_docker_removing_a_container_reclaims_its_anonymous_volumes",
    // Lane A: the ContainerRunner against the real runtime.
    "real_docker_runs_from_the_recorded_image_id_and_composes_over_the_image_environment",
    "real_docker_refuses_a_reviewer_write_to_its_read_only_mount",
    "real_docker_confines_a_gate_to_its_mount",
    "real_docker_a_git_dependent_gate_sees_only_the_role_view",
    "real_docker_adapter_parsing_matches_the_host_table",
    // Lane C: the startup census against the real runtime.
    "real_docker_census_reclaims_a_dead_owner_and_spares_a_live_one",
    // Repair round R1: what "confines gate-executed repository code" and
    // "pre-flight certifies the environment that will actually spend" mean
    // against a real daemon rather than against a spec.
    "real_docker_a_gate_write_outside_every_declared_mount_fails",
    "real_docker_the_daemon_holds_exactly_the_specs_mounts_and_a_read_only_root",
    "real_docker_a_worktree_binary_cannot_shadow_the_certified_cli",
    // Repair round R2: the two tables whose oracle has to be the daemon.
    "real_docker_renders_a_comma_bearing_label_value_whole",
    "real_docker_prints_the_transcribed_unreachable_diagnostics",
    // Repair round R3b: the two daemon behaviours the engine has to compensate
    // for, and the two claims whose only honest oracle is a live container.
    "real_docker_creates_an_absent_named_volume_rather_than_refusing",
    "real_docker_prints_the_transcribed_removal_in_progress_diagnostic",
    "real_docker_withholds_an_image_credential_variable_from_a_role_that_takes_none",
    "real_docker_a_container_contains_a_daemonised_descendant",
];

/// Why a gated test skipped.
///
/// A value rather than a comment, so a skipping test *reads* the reason and a
/// skip that had stopped saying anything is a compile-time unused value rather
/// than a silence.
pub(crate) fn absent_reason() -> String {
    format!(
        "no container runtime: `{}` is not on PATH or its daemon does not answer",
        super::DOCKER_PROGRAM
    )
}

/// Ask whether the Docker-gated half of this suite can run.
///
/// `Ok` is the CLI bound to the trace; `Err` is the reason, which the caller
/// must assert on before returning — that is what makes the skip loud in the
/// test body rather than an unremarked early `return`.
///
/// # Panics
///
/// When `test` is not in [`DOCKER_GATED_TESTS`], so an uncounted gated test
/// cannot exist; and when [`REQUIRE_DOCKER`] is set and Docker is not
/// available, which is how a skip becomes a **failure** on a machine that has a
/// runtime.
pub(crate) fn docker_gate(test: &str, trace: ContainerTrace) -> Result<Box<DockerCli>, String> {
    assert!(
        DOCKER_GATED_TESTS.contains(&test),
        "`{test}` is Docker-gated and is not in DOCKER_GATED_TESTS, so nothing counts it"
    );
    if DockerCli::available() {
        return Ok(Box::new(DockerCli::new(trace)));
    }
    let reason = absent_reason();
    assert!(
        std::env::var_os(REQUIRE_DOCKER).is_none(),
        "{REQUIRE_DOCKER} is set and `{test}` would have skipped: {reason}"
    );
    Err(reason)
}

/// The repository key every container name **that a pre-clean touches** is
/// built from, **scoped to the build slot this process is running in**.
///
/// **Not "every container name in a Docker-gated test".** That is what this
/// sentence said after the move from `exec.rs`, where the narrower "every
/// container name in this module" was true; `runner::container::tests` names
/// Docker-gated containers with bare literals carrying no repo key at all and
/// reclaims them with `let _ = docker.remove(name);` rather than through
/// [`preclean_names`]. Those names are outside this rule and outside the guard
/// that enforces it. `R5-SEAMS-003`.
///
/// **And the guard is inert when there is no slot.** With `CARGO_TARGET_DIR`
/// unset, [`scoped_repo_key`] returns the fixed `0123456789abcdef`, so two
/// concurrent bare `cargo test` runs on one box derive the same key, pass
/// [`unscoped_names`], and each pre-clean kills the other's live container —
/// the state the guard exists to refuse. It is inert in exactly the
/// configuration with no other protection. Recorded rather than repaired,
/// because the alternative — a per-process key — makes the pre-clean useless,
/// which is the trade the "why this is not a constant" section below is about.
/// On this box every gate command goes through `upstroke-build`, and CI's bare
/// `cargo test` runs one job per machine. `R5-SEAMS-006`.
///
/// # Why this is not a constant
///
/// [`preclean_names`] kills and removes a container by name before creating it,
/// because no in-process cleanup runs when a process is SIGKILLed and the name a
/// killed run left behind is exactly the name the next `docker create` asks for.
/// That is correct and it is why the helper exists.
///
/// With a **fixed** key it is also hostile: two suite runs share every name, so
/// the second run's pre-clean kills the first run's **live** container.
/// `PR7-R3-CONTRACT-001`. This box runs concurrent suites by design — the whole
/// point of `upstroke-build`'s slot pool — so a fixed name is not a theoretical
/// collision, it is the normal case.
///
/// **Scoped to the slot rather than to the process** deliberately. A PID would
/// make every run's names unique and thereby make the pre-clean useless — it
/// could never match the killed run's leftovers, which is the one thing it is
/// for. `CARGO_TARGET_DIR` is stable across runs *in* a slot and distinct
/// *between* slots, so a slot reclaims its own residue and touches nobody
/// else's. That is the same discriminator the slot pool already uses.
///
/// **Here rather than in one caller's test module.** `b44040a` put this in
/// `exec.rs`'s and left `census/tests.rs` — the other of
/// [`preclean_names`]'s two callers — on a fixed `"cccccccccccccccc"`, so the
/// class stayed live on that path. A rule with one implementation per caller is
/// a rule each caller can be missing. It sits beside the helper it is a
/// precondition of, and [`unscoped_names`] makes it one the helper checks.
pub(crate) fn slot_repo_key() -> &'static str {
    static KEY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        scoped_repo_key(&std::env::var("CARGO_TARGET_DIR").unwrap_or_default())
    });
    &KEY
}

/// [`slot_repo_key`]'s derivation, as a pure function of the scope.
///
/// Separated from the `LazyLock` so it is testable: the cache is computed once
/// per process, so a test that set the variable and called [`slot_repo_key`]
/// would assert whatever the first caller in that process happened to see.
///
/// Sixteen hex characters, because `workspace_manager`'s `REPO_KEY_HEX_CHARS`
/// says a repo key is.
pub(crate) fn scoped_repo_key(scope: &str) -> String {
    if scope.is_empty() {
        // No slot -- a bare `cargo test`, where nothing else is running.
        return "0123456789abcdef".to_owned();
    }
    let digest = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(scope.as_bytes())
    );
    digest[..16].to_owned()
}

/// The names among `names` that a **concurrent run could also ask for**, as the
/// rendered strings, so a refusal can say what it refused.
///
/// A name whose repo-key component is not this slot's is a name another slot's
/// suite builds identically, and [`preclean_names`] kills by name with no
/// liveness check. Checking the *component* rather than the whole name is
/// deliberately narrower than the property ("no two concurrent runs ask for the
/// same name"): a caller that scoped some other component instead gets a loud
/// refusal it can widen this rule for, rather than a silent stranger-kill.
///
/// A name that will not parse is reported too. The pre-clean is the one place
/// that acts on a name it did not build, and an unparseable one is a name whose
/// scope cannot be established at all.
pub(crate) fn unscoped_names(names: &[&super::intent::ContainerName]) -> Vec<String> {
    let slot = slot_repo_key();
    names
        .iter()
        .filter(|name| {
            !super::intent::ContainerName::parse(name.as_str())
                .is_ok_and(|parts| parts.repo_key == slot)
        })
        .map(|name| name.as_str().to_owned())
        .collect()
}

/// Reclaim the container names a gated test is about to create, before it
/// creates them.
///
/// `reviews/FINDINGS.md` §16. Two gated tests went red on a head whose only
/// change was a documentation edit, and passed in isolation minutes later:
/// four containers from an earlier run this session had been **SIGKILLed** when
/// the box exhausted its inodes — two `Exited (137)`, two still `Created` —
/// and their names were the deterministic ones those tests recreate, so
/// `docker create` answered `Conflict. The container name … is already in use`.
///
/// Both tests already clean up after themselves, one with a closure on every
/// exit path and one with a `LeaveNoResidue` guard. Both are correct and
/// neither can help: **no in-process cleanup runs when the process is
/// SIGKILLed.** The only cleanup that survives is one the *next* run performs.
///
/// The idiom is correct here **only because the names are deterministic**:
///
/// > A pre-clean removes the previous run's residue exactly when the name
/// > recurs. Keyed by something unique per process — a pid, a ULID — it can
/// > never name anything an earlier run created, and it degrades into an
/// > unconditional retry that cleans nothing.
///
/// So this takes the exact names the caller is about to use, and callers must
/// build them from their own fixed constants. A caller that passes a pid-keyed
/// or ULID-keyed name gets a no-op that looks like protection.
///
/// This goes through [`super::reclaim`] rather than calling `stop`/`remove`
/// directly. That is not ceremony: `fake.rs` **re-denies** the effect lints at
/// its own module level (`PR6-LANEF-004` — a lint level is scoped by the module
/// tree, so every out-of-line child of `runner::container` had been silently
/// inheriting the funnel's allow), so a raw primitive here does not compile.
/// The funnel is also the right answer on its merits: it is the packet's own
/// reclaim order, every step idempotent and tolerant of already-gone.
///
/// `view_path` is `None` because a previous run's Git view lives under **its**
/// scratch root, which was keyed by that run's pid and is unreachable from
/// this one. The container name is the only part of the residue that is global
/// to the daemon, and it is the only part that can collide.
///
/// # Panics
///
/// When a name cannot be reclaimed for any reason other than its absence —
/// a pre-clean that fails quietly leaves the conflict it exists to prevent,
/// and the test then fails somewhere far less informative.
pub(crate) fn preclean_names(
    runtime: &dyn ContainerRuntime,
    view: &dyn super::GitView,
    private_root: &Path,
    names: &[&super::intent::ContainerName],
) {
    // The precondition, checked rather than documented. Before this the doc
    // below said "callers must build them from their own fixed constants" and
    // one of the two callers did exactly that -- fixed, and therefore shared
    // with every concurrent slot. See [`unscoped_names`].
    let unscoped = unscoped_names(names);
    assert!(
        unscoped.is_empty(),
        "pre-cleaning {unscoped:?}: the repo-key component is not this build slot's `{}`, so a \
         concurrent suite in another slot asks for the same name and this kill lands on its live \
         container rather than on a previous run's residue. `PR7-R3-CONTRACT-001`",
        slot_repo_key()
    );
    for name in names {
        super::reclaim(&mut super::NoHooks, runtime, view, private_root, name, None)
            .unwrap_or_else(|error| {
                panic!("pre-clean could not reclaim a possibly-stranded `{name}`: {error}")
            });
    }
}
