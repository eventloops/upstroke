//! Extended notes: `docs/internals/runner/container/fake.md`

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FakeImage {
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FakeContainer {
    pub labels: BTreeMap<String, String>,
    pub requested_image_id: String,
    pub reported_image_id: String,
    pub state: Liveness,
    pub execution: ContainerExecution,
}

#[derive(Debug, Default)]
struct State {
    images: BTreeMap<String, FakeImage>,
    tags: BTreeMap<String, String>,
    volumes: BTreeSet<String>,
    unreachable: BTreeSet<RuntimeOp>,
    failing: BTreeSet<RuntimeOp>,
    diagnostics: BTreeMap<RuntimeOp, String>,
    containers: BTreeMap<String, FakeContainer>,
    substitutions: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
pub(crate) struct FakeRuntime {
    state: Mutex<State>,
    trace: ContainerTrace,
}

impl FakeRuntime {
    pub(crate) fn new(trace: ContainerTrace) -> Self {
        Self {
            state: Mutex::new(State::default()),
            trace,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

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

    pub(crate) fn add_image(&self, id: &str, digest: Option<&str>) {
        self.state().images.insert(
            id.to_owned(),
            FakeImage {
                digest: digest.map(str::to_owned),
            },
        );
    }

    pub(crate) fn tag(&self, reference: &str, id: &str) {
        self.state()
            .tags
            .insert(reference.to_owned(), id.to_owned());
    }

    pub(crate) fn move_tag(&self, reference: &str, new_id: &str) {
        let mut state = self.state();
        assert!(
            state.tags.contains_key(reference),
            "`{reference}` is not tagged, so it cannot be moved; \
             a moved-reference fixture that started from nothing would be testing itself"
        );
        state.tags.insert(reference.to_owned(), new_id.to_owned());
    }

    pub(crate) fn substitute_reported_image_id(&self, name: &str, reported: &str) {
        self.state()
            .substitutions
            .insert(name.to_owned(), reported.to_owned());
    }

    pub(crate) fn add_volume(&self, name: &str) {
        self.state().volumes.insert(name.to_owned());
    }

    pub(crate) fn remove_volume(&self, name: &str) {
        self.state().volumes.remove(name);
    }

    pub(crate) fn set_unreachable(&self, op: RuntimeOp) {
        self.state().unreachable.insert(op);
    }

    pub(crate) fn set_all_unreachable(&self) {
        let mut state = self.state();
        for op in RuntimeOp::ALL {
            state.unreachable.insert(*op);
        }
    }

    pub(crate) fn set_reachable(&self, op: RuntimeOp) {
        self.state().unreachable.remove(&op);
    }

    pub(crate) fn set_docker_stderr(&self, op: RuntimeOp, detail: &str) {
        self.state().diagnostics.insert(op, detail.to_owned());
    }

    pub(crate) fn set_failing(&self, op: RuntimeOp) {
        self.state().failing.insert(op);
    }

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

    pub(crate) fn set_container_state(&self, name: &str, state: Liveness) {
        if let Some(container) = self.state().containers.get_mut(name) {
            container.state = state;
        }
    }

    pub(crate) fn set_execution(&self, name: &str, execution: ContainerExecution) {
        if let Some(container) = self.state().containers.get_mut(name) {
            container.execution = execution;
        }
    }

    pub(crate) fn container(&self, name: &str) -> Option<FakeContainer> {
        self.state().containers.get(name).cloned()
    }

    pub(crate) fn container_names(&self) -> Vec<String> {
        self.state().containers.keys().cloned().collect()
    }

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

#[derive(Debug, Default)]
pub(crate) struct FakeOwnerLiveness {
    live: Mutex<BTreeSet<PathBuf>>,
}

impl FakeOwnerLiveness {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_live(&self, public_run_dir: &Path) {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(public_run_dir.to_path_buf());
    }

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

#[derive(Debug, Default)]
pub(crate) struct RecordingHooks {
    trace: ContainerTrace,
    armed: Option<(EffectSiteId, HookPhase)>,
}

impl RecordingHooks {
    pub(crate) fn new(trace: ContainerTrace) -> Self {
        Self { trace, armed: None }
    }

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

pub(crate) const REQUIRE_DOCKER: &str = "UPSTROKE_REQUIRE_DOCKER";

pub(crate) const DOCKER_GATED_TESTS: &[&str] = &[
    "real_docker_reports_an_image_id_and_a_digest_for_a_reference_it_holds",
    "real_docker_refuses_a_reference_it_does_not_hold_without_pulling",
    "real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently",
    "real_docker_kill_on_an_already_exited_container_is_tolerated",
    "real_docker_returns_both_streams_of_a_container_separately",
    "real_docker_removing_a_container_reclaims_its_anonymous_volumes",
    "real_docker_runs_from_the_recorded_image_id_and_composes_over_the_image_environment",
    "real_docker_refuses_a_reviewer_write_to_its_read_only_mount",
    "real_docker_confines_a_gate_to_its_mount",
    "real_docker_a_git_dependent_gate_sees_only_the_role_view",
    "real_docker_adapter_parsing_matches_the_host_table",
    "real_docker_census_reclaims_a_dead_owner_and_spares_a_live_one",
    "real_docker_a_gate_write_outside_every_declared_mount_fails",
    "real_docker_the_daemon_holds_exactly_the_specs_mounts_and_a_read_only_root",
    "real_docker_a_worktree_binary_cannot_shadow_the_certified_cli",
    "real_docker_renders_a_comma_bearing_label_value_whole",
    "real_docker_prints_the_transcribed_unreachable_diagnostics",
    "real_docker_creates_an_absent_named_volume_rather_than_refusing",
    "real_docker_prints_the_transcribed_removal_in_progress_diagnostic",
    "real_docker_withholds_an_image_credential_variable_from_a_role_that_takes_none",
    "real_docker_a_container_contains_a_daemonised_descendant",
];

pub(crate) fn absent_reason() -> String {
    format!(
        "no container runtime: `{}` is not on PATH or its daemon does not answer",
        super::DOCKER_PROGRAM
    )
}

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

pub(crate) fn slot_repo_key() -> &'static str {
    static KEY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        scoped_repo_key(&std::env::var("CARGO_TARGET_DIR").unwrap_or_default())
    });
    &KEY
}

pub(crate) fn scoped_repo_key(scope: &str) -> String {
    if scope.is_empty() {
        return "0123456789abcdef".to_owned();
    }
    let digest = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(scope.as_bytes())
    );
    digest[..16].to_owned()
}

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

pub(crate) fn preclean_names(
    runtime: &dyn ContainerRuntime,
    view: &dyn super::GitView,
    private_root: &Path,
    names: &[&super::intent::ContainerName],
) {
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
