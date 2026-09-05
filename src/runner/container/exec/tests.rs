//! Extended notes: `docs/internals/runner/container/exec/tests.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types, clippy::disallowed_macros)]

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
    AgentId, InvocationId, ProbeTarget, RunnerRequest, gate_request, review_request, worker_request,
};
use crate::topology::events::{AttemptNumber, GenerationId, ImageIdentity};
use crate::topology::registry::TaskKey;

const RUN_ID: &str = "01KZRN48A4ZK3AEDST3RJ8HMA4";
fn repo_key() -> &'static str {
    crate::runner::container::fake::slot_repo_key()
}

#[test]
fn a_container_name_is_scoped_to_its_build_slot() {
    use crate::runner::container::fake::scoped_repo_key;

    let slot2 = scoped_repo_key("/mnt/ramtarget/slot2");
    let slot3 = scoped_repo_key("/mnt/ramtarget/slot3");

    assert_ne!(
        slot2, slot3,
        "two build slots share a repository key, so their container names \
         collide and each run's pre-clean kills the other's container"
    );
    assert_eq!(
        slot2,
        scoped_repo_key("/mnt/ramtarget/slot2"),
        "the key is not stable for one slot, so a pre-clean cannot match the \
         residue its own previous run left — the only thing it is for"
    );
    for (scope, key) in [
        ("/mnt/ramtarget/slot2", &slot2),
        ("/mnt/ramtarget/slot3", &slot3),
        ("", &scoped_repo_key("")),
    ] {
        assert!(
            key.len() == 16 && key.chars().all(|c| c.is_ascii_hexdigit()),
            "`{scope}` derives `{key}`, which is not the sixteen hex \
             characters `workspace_manager::REPO_KEY_HEX_CHARS` requires"
        );
    }
    assert_eq!(
        repo_key(),
        scoped_repo_key(&std::env::var("CARGO_TARGET_DIR").unwrap_or_default()),
        "the cached key is not this process's own scope's"
    );
}
const INCARNATION_1: &str = "01KZTAAAAAAAAAAAAAAAAAAAAA";
const INCARNATION_2: &str = "01KZTBBBBBBBBBBBBBBBBBBBBB";
const IMAGE_ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const OTHER_IMAGE_ID: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const IMAGE_REFERENCE: &str = "ghcr.io/example/upstroke-runner:v1";
const MANIFEST_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const VOLUMES: &[(&str, &str)] = &[
    ("claude-code", "upstroke-creds-claude"),
    ("copilot", "upstroke-creds-copilot"),
    ("codex", "upstroke-creds-codex"),
];
const EVENT_LOG_MARKER: &str = "COORDINATOR-EVENT-LOG-a5f2";

const IMAGE_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

const CWD_RELATIVE_PATH: &str = "/usr/local/bin:.:/usr/bin";

fn image_environment() -> ContainerEnvironment {
    ContainerEnvironment::from_image(
        [
            ("PATH", IMAGE_PATH),
            ("HOME", "/root"),
            ("UPSTROKE_IMAGE_MARKER", IMAGE_MARKER_VALUE),
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
    fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError> {
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
        std::fs::write(paths.events(), format!("{EVENT_LOG_MARKER}\n")).expect("the public log");
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
            crate::workspace_manager::execution_root_of(&private_root, repo_key(), RUN_ID);
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
                repo_key: repo_key().to_owned(),
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

fn sources(mounts: &[Mount]) -> Vec<PathBuf> {
    mounts
        .iter()
        .filter_map(|mount| match mount {
            Mount::Path { source, .. } => Some(source.clone()),
            Mount::Volume { .. } | Mount::Tmpfs { .. } => None,
        })
        .collect()
}

fn target_of<'a>(mounts: &'a [Mount], target: &str) -> Option<&'a Mount> {
    mounts.iter().find(|mount| mount.target() == target)
}

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

    let mounts = plan.mounts();
    let targets: Vec<&str> = mounts.iter().map(Mount::target).collect();
    assert_eq!(
        targets,
        vec![
            "/upstroke/workspace",
            "/upstroke/gitview",
            "/upstroke/gitobjects",
            "/upstroke/workspace/.git",
            "/upstroke/credentials/claude-code",
            "/tmp",
        ],
        "the mount set moved"
    );
    assert_eq!(
        target_of(mounts, "/upstroke/gitobjects").map(Mount::read_only),
        Some(true),
        "the borrowed object store is read-only (DESIGN.md:612)"
    );
    assert_eq!(
        target_of(mounts, "/upstroke/workspace").map(Mount::read_only),
        Some(false),
        "an implementer writes to its worktree"
    );
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
    assert!(
        list_intents(&fixture.private_root)
            .expect("scan")
            .is_empty()
    );
    assert!(fixture.runtime.fake().container_names().is_empty());
}

#[test]
fn only_the_reviewer_receives_a_read_only_worktree() {
    let fixture = Fixture::new("ro-review", true);
    let runner = fixture.runner();
    let mut read_only = Vec::new();
    let mut writable = Vec::new();
    let mut without = Vec::new();
    for request in requests(&fixture.task_a) {
        let plan = runner.plan(&request).expect("plans");
        match target_of(plan.mounts(), "/upstroke/workspace") {
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
    assert_eq!(
        without,
        vec!["probe(claude-code)".to_owned(), "probe(shell)".to_owned()],
        "{without:?}"
    );
    assert_eq!(read_only.len() + writable.len() + without.len(), 5);
}

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
            let in_env = key.and_then(|key| {
                plan.env()
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value.clone())
                    .filter(|value| !value.is_empty())
            });
            let expected =
                recorded && supplies_credential_location(&request.role) && request.agent.is_some();
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
                .volume_present("upstroke-creds-claude")
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
        BTreeSet::from(["upstroke-creds-claude".to_owned()]),
        "the volume is the one the record names for that agent"
    );
}

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

#[test]
fn a_substituted_reported_image_id_refuses_before_start_in_both_phases() {
    for (phase, expected, build) in [
        (
            "pre-flight",
            ImageIdMismatch::RefusedBeforeStart,
            (|fixture: &Fixture| {
                host::shell_probe_request(ShellKind::Sh, fixture.task_a.clone(), shell_probe_id())
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
        let name = ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation)
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

        assert_eq!(
            ImageIdMismatch::of(&request.invocation),
            expected,
            "{phase}: the phase was classified from the invocation wrongly"
        );
        match expected {
            ImageIdMismatch::RefusedBeforeStart => assert!(
                matches!(refusal, UpstrokeError::Refused { .. }),
                "{phase}: a pre-flight mismatch must be a refusal before any spend: \
                 {refusal:?}"
            ),
            ImageIdMismatch::SpawnFailureOutage => assert!(
                matches!(refusal, UpstrokeError::Agent { .. }),
                "{phase}: a mid-run mismatch reached the caller as the same generic refusal a \
                 pre-flight one does, so the RunnerSpawnFailure outage settlement the \
                 contract requires is unreachable: {refusal:?}"
            ),
        }

        assert!(
            fixture.trace.position_starting("site:Start").is_none(),
            "{phase}: the container was started: {:#?}",
            fixture.trace.rendered()
        );
        assert!(!fixture.trace.ops().contains(&RuntimeOp::Start), "{phase}");

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
        let name =
            ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, invocation).expect("a name");
        let error = got.error(&name, OTHER_IMAGE_ID, IMAGE_ID);
        assert_eq!(
            matches!(error, UpstrokeError::Refused { .. }),
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

#[test]
fn the_shell_probe_runs_through_this_runner_as_a_registered_container_invocation() {
    let fixture = Fixture::new("shell-probe", true);
    let runner = fixture.runner();
    let invocation = shell_probe_id();
    let name = ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &invocation).expect("a name");

    host::run_shell_probe(
        &runner,
        ShellKind::Sh,
        fixture.task_a.clone(),
        invocation.clone(),
    )
    .expect("the recorded shell runs inside the recorded image");

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

    assert!(invocation.probe_target().is_some());
    assert_eq!(invocation.render(), "p.shell.o0");

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

#[test]
fn failing_preflight_probe_on_resume_refuses_before_recovery_event_and_reclaims_probe_containers() {
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
            ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &invocation).expect("a name");

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
            let adapter: &dyn crate::agent::AgentAdapter = &crate::agent::claude::ClaudeCodeAdapter;
            let refusal = adapter
                .probe(&runner)
                .expect_err("an agent CLI that fails inside the image refuses");
            let message = refusal.to_string();
            assert!(
                message.contains("claude"),
                "{tag}: the refusal does not name the CLI: {message}"
            );
            assert!(
                message.contains("--version"),
                "{tag}: the refusal came from somewhere other than the CLI's own exit \
                 status: {message}"
            );
            assert!(
                message.contains(&format!("{exit:?}")),
                "{tag}: the refusal does not carry the exit status: {message}"
            );
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
        assert!(fixture.paths.events().exists());
    }
}

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
                host::shell_probe_request(ShellKind::Sh, fixture.task_a.clone(), invocation.clone())
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
    assert_eq!(shell_probe_id().render(), shell_probe_id().render());
    assert_eq!(names.len(), 4, "{names:?}");
    assert_eq!(intents.len(), 4, "{intents:?}");
}

#[test]
fn probe_and_execution_compose_through_one_code_path() {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("runner")
            .join("container")
            .join("exec.rs"),
    )
    .expect("read this module");
    let production = crate::effects::blank_comments(&crate::effects::production_region(&source));
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
        let composed = |plan: &InvocationPlan| -> BTreeMap<String, String> {
            plan.env().iter().cloned().collect()
        };
        assert_eq!(
            composed(&probed),
            composed(&executed),
            "{tag}: pre-flight certifies an environment the attempt does not run in"
        );
        let probe_targets: BTreeSet<&str> = probed.mounts().iter().map(Mount::target).collect();
        let execution_targets: BTreeSet<&str> =
            executed.mounts().iter().map(Mount::target).collect();
        assert!(
            probe_targets.is_subset(&execution_targets),
            "{tag}: {probe_targets:?} vs {execution_targets:?}"
        );
        let difference: BTreeSet<&&str> = execution_targets.difference(&probe_targets).collect();
        assert_eq!(
            difference,
            BTreeSet::from([
                &"/upstroke/workspace",
                &"/upstroke/gitview",
                &"/upstroke/gitobjects",
                &"/upstroke/workspace/.git",
            ]),
            "{tag}: the probe and the execution differ by something other than the worktree"
        );
    }
}

#[test]
fn host_and_container_compose_the_same_environment_for_every_role() {
    let base: Vec<(String, String)> = [
        ("PATH", "/usr/local/bin:/usr/bin:/bin"),
        ("HOME", "/root"),
        ("LANG", "C.UTF-8"),
        ("CLAUDE_CONFIG_DIR", "/host/claude"),
        ("UPSTROKE_SHARED", "shared"),
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
        ("UPSTROKE_OVERLAY".to_owned(), "1".to_owned()),
        ("LANG".to_owned(), "en_GB.UTF-8".to_owned()),
    ];

    let mut supplied_locations = 0_usize;
    let mut rows = 0_usize;
    for role in ExecutionRole::all() {
        let agent = match &role {
            ExecutionRole::Probe(ProbeTarget::Agent(agent)) => Some(agent.clone()),
            ExecutionRole::Implement | ExecutionRole::Review => Some(AgentId::new("claude-code")),
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
    assert!(base.iter().any(|(key, _)| key == "CLAUDE_CONFIG_DIR"));
}

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
    let name =
        ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation).expect("a name");

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
    assert!(at("view:materialized") < at("rt:create"), "{rendered:#?}");
    assert!(at("rt:collect") < at("rt:remove"), "{rendered:#?}");

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
    let name =
        ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation).expect("a name");

    let output = runner
        .run(&request)
        .expect("a timeout is an output, not an error");
    assert!(output.timed_out);
    assert_eq!(
        output.code, None,
        "a container the supervisor stopped did not exit on its own"
    );

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

#[test]
fn a_credential_volume_is_never_created_or_pruned_by_any_disposition() {
    let volume = "upstroke-creds-claude";
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
            let name = ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation)
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
            let name = ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation)
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

fn container_subtree_production_regions() -> (Vec<(String, String)>, BTreeSet<String>) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("runner");
    let funnel = std::fs::read_to_string(dir.join("container.rs")).expect("the funnel");
    let declarations = crate::effects::blank_comments(&crate::effects::production_region(&funnel));
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
        let source = std::fs::read_to_string(dir.join("container").join(&name)).expect("a module");
        regions.push((
            name,
            crate::effects::blank_comments(&crate::effects::production_region(&source)),
        ));
    }
    (regions, excluded)
}

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
            let name = ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation)
                .expect("a name");

            let refusal = runner
                .run(&request)
                .expect_err("the funnel was made to fail");
            assert!(
                refusal.to_string().contains(site.name()),
                "[{site:?}/{phase}] the error does not name the site that failed: {refusal}"
            );

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

#[test]
fn every_at_run_end_outcome_is_driven_through_its_mechanism_and_the_ledgers_balance() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Mechanism {
        Complete,
        Cancel,
        Shutdown,
        Census,
    }

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

    let volume = "upstroke-creds-claude";
    for (outcome, r19, r20, r26, mechanism) in OUTCOMES {
        let fixture = Fixture::new(&format!("outcome-{outcome}"), true);
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
        let name = ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation)
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
                fixture
                    .runtime
                    .fake()
                    .substitute_reported_image_id(name.as_str(), OTHER_IMAGE_ID);
                fixture.runner().run(&request).expect_err("refuses");
            }
            Mechanism::Census => {
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

#[test]
fn every_physical_resource_of_a_container_invocation_maps_to_exactly_one_row() {
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

    let site_names: BTreeSet<&str> = ContainerSite::ALL.iter().map(|site| site.name()).collect();
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

#[test]
fn the_container_subtree_can_only_inspect_a_volume() {
    let (regions, excluded) = container_subtree_production_regions();

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

    let seam = &regions
        .iter()
        .find(|(name, _)| name == "runtime.rs")
        .expect("the seam")
        .1;
    assert_eq!(seam.matches("fn volume").count(), 1, "one volume method");
    assert!(seam.contains("fn volume_present(&self, name: &str) -> Result<bool, RuntimeError>"));
}

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
    let view: Box<dyn GitView> = Box::new(RoleGitView::new(ContainerTrace::off()));
    fn takes_view(_: &dyn GitView) {}
    takes_view(view.as_ref());
}

type SeenIntents = Arc<Mutex<Vec<(String, Option<Vec<u8>>)>>>;

#[derive(Debug)]
struct IntentPeek {
    inner: crate::runner::container::DisposableDirView,
    private_root: PathBuf,
    seen: SeenIntents,
}

impl GitView for IntentPeek {
    fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, UpstrokeError> {
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

    fn discard(&self, path: &Path) -> Result<(), UpstrokeError> {
        self.inner.discard(path)
    }
}

#[derive(Debug, Default)]
struct FailingView;

impl GitView for FailingView {
    fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, UpstrokeError> {
        Err(UpstrokeError::Refused {
            message: format!("VIEW-REFUSED for {}", request.path.display()),
        })
    }

    fn discard(&self, _path: &Path) -> Result<(), UpstrokeError> {
        Ok(())
    }
}

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
        crate::workspace_manager::execution_root_of(&fixture.private_root, repo_key(), RUN_ID);
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

    let mut planned = 0_usize;
    for request in requests(&fixture.task_a) {
        runner
            .plan(&request)
            .expect("the role's own worktree plans");
        planned += 1;
    }
    assert_eq!(planned, 5);

    assert!(
        list_intents(&fixture.private_root)
            .expect("scan")
            .is_empty()
    );
    assert!(fixture.runtime.fake().container_names().is_empty());
}

#[test]
fn the_execution_root_and_its_worktree_namespaces_are_withheld_and_one_worktree_is_not() {
    let fixture = Fixture::new("exec-root", true);
    let runner = fixture.runner();
    let execution_root =
        crate::workspace_manager::execution_root_of(&fixture.private_root, repo_key(), RUN_ID);
    let namespaces: Vec<PathBuf> = vec![
        execution_root.clone(),
        execution_root.join("tasks"),
        execution_root.join("merge"),
        execution_root.join("snapshots"),
    ];

    let confinement = Confinement::of_run(&fixture.identity, &fixture.repo);
    for path in &namespaces {
        assert!(
            confinement
                .entries()
                .iter()
                .any(|(category, entry)| *category == Withheld::SiblingWorktree && entry == path),
            "`of_run` does not withhold `{}`: {:?}",
            path.display(),
            confinement.entries()
        );
    }

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
        let name = ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation)
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
        let record: ContainerIntent = serde_json::from_slice(bytes).expect("the six fields parse");
        assert_eq!(record.run_id, RUN_ID);
        assert_eq!(
            crate::runner::container::intent::owner_run_dir(&record.run_dir, "intent record")
                .expect("the recorded run dir decodes"),
            fixture.paths.public
        );
        assert_eq!(record.incarnation, INCARNATION_1);
        assert_eq!(record.repo_key, repo_key());
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

#[test]
fn a_container_is_created_and_started_only_under_its_own_intent_record() {
    let fixture = Fixture::new("intent-capability", false);
    let root = fixture.private_root.clone();
    let mine =
        ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &worker_id(0)).expect("a name");
    let other =
        ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &worker_id(1)).expect("a name");

    let refusal = crate::runner::container::intent::IntentWritten::certify(&root, &mine)
        .expect_err("there is no record, so there is no proof");
    assert!(
        matches!(
            &refusal,
            UpstrokeError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
        ),
        "a missing record must refuse as a missing file: {refusal}"
    );

    std::fs::create_dir_all(crate::runner::container::intent::containers_dir(&root))
        .expect("the namespace");
    std::fs::write(mine.intent_path(&root), b"{\"not\":\"an intent\"}")
        .expect("a malformed record");
    let refusal = crate::runner::container::intent::IntentWritten::certify(&root, &mine)
        .expect_err("a malformed record is not evidence");
    assert!(
        matches!(refusal, UpstrokeError::Refused { .. }),
        "{refusal}"
    );

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
            let name = ContainerName::new(repo_key(), RUN_ID, INCARNATION_1, &request.invocation)
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
            assert!(
                message.contains(marker),
                "{tag} (stop_fails: {stop_fails}): the original cause was masked: {message}"
            );

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

            if *point != Where::Start {
                assert!(
                    !fixture.trace.ops().contains(&RuntimeOp::Start),
                    "{tag}: the container was started"
                );
            }

            let attempts_stop = matches!(point, Where::Create | Where::Mismatch | Where::Start);
            let expects_residue = stop_fails && attempts_stop;
            assert_eq!(
                message.contains("could not be stopped"),
                expects_residue,
                "{tag} (stop_fails: {stop_fails}): {message}"
            );
            if expects_residue {
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

#[test]
fn the_working_directory_is_the_runners_own_for_every_role() {
    let fixture = Fixture::new("workdir", true);
    let runner = fixture.runner();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for request in requests(&fixture.task_a) {
        let plan = runner.plan(&request).expect("plans");
        let workdir =
            plan.launch.spec.workdir.clone().unwrap_or_else(|| {
                panic!("{}: the image chose the working directory", request.role)
            });
        seen.insert(request.role.label(), workdir);
    }
    assert_eq!(
        seen,
        BTreeMap::from([
            ("implement".to_owned(), "/upstroke/workspace".to_owned()),
            ("gate".to_owned(), "/upstroke/workspace".to_owned()),
            ("review".to_owned(), "/upstroke/workspace".to_owned()),
            ("probe(shell)".to_owned(), "/tmp".to_owned()),
            ("probe(claude-code)".to_owned(), "/tmp".to_owned()),
        ]),
        "the working directory rule moved"
    );
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

    assert!(
        list_intents(&fixture.private_root)
            .expect("scan")
            .is_empty()
    );
    assert!(fixture.runtime.fake().container_names().is_empty());

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

const PREFERRED_IMAGES: &[&str] = &[
    "upstroke-test/git:v1",
    "alpine:3.20",
    "busybox:latest",
    "debian:stable-slim",
];

const GIT_IMAGES: &[&str] = &["upstroke-test/git:v1"];

const MARKER_IMAGE: &str = "upstroke-test/git:v1";
const IMAGE_MARKER_VALUE: &str = "image-environment-v1";

const CREDENTIAL_ENV_IMAGE: &str = "upstroke-test/credenv:v1";

const CREDENTIAL_ENV_CONTROL: (&str, &str) = ("GH_CONFIG_DIR", "/image/gh");

fn skipped(reason: &str) {
    assert_eq!(
        reason,
        crate::runner::container::fake::absent_reason(),
        "a Docker-gated test skipped for a reason the gate does not know about"
    );
}

fn no_image(reason: &str) {
    assert!(reason.contains("never pull"), "{reason}");
    assert!(
        std::env::var_os(crate::runner::container::fake::REQUIRE_DOCKER).is_none(),
        "{} is set and a gated test found no usable image: {reason}",
        crate::runner::container::fake::REQUIRE_DOCKER
    );
}

fn discover(docker: &dyn ContainerRuntime, preferred: &[&str]) -> Result<(String, String), String> {
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
        repo_key: repo_key().to_owned(),
    }
}

const GATED_RUNS: &[(&str, &str)] = &[
    ("env", "01KZGATEDA000000000000000A"),
    ("readonly", "01KZGATEDB000000000000000B"),
    ("confine", "01KZGATEDC000000000000000C"),
    ("gitview", "01KZGATEDD000000000000000D"),
    ("parity", "01KZGATEDE000000000000000E"),
    ("outside", "01KZR1GATED000000000000001"),
    ("daemonspec", "01KZR1GATED000000000000002"),
    ("shadow", "01KZR1GATED000000000000003"),
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
             printf 'OVERLAY=%s\\n' \"$UPSTROKE_OVERLAY\"; \
             printf 'MARKER=%s\\n' \"$UPSTROKE_IMAGE_MARKER\"; \
             printf 'PWD=%s\\n' \"$(pwd)\"",
            )
            .env("UPSTROKE_OVERLAY", "landed"),
        workspace.clone(),
        Duration::from_secs(60),
        gate_id(0),
    );

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
    assert!(
        !plan
            .env()
            .iter()
            .any(|(key, _)| key == "UPSTROKE_IMAGE_MARKER"),
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
    assert_eq!(line("PWD="), "/upstroke/workspace");
    if reference == MARKER_IMAGE {
        assert_eq!(
            line("MARKER="),
            IMAGE_MARKER_VALUE,
            "the marker image's own environment did not survive composition"
        );
    }

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
        let spec =
            ShellKind::Sh.spec("( echo upstroke-wrote-this > /upstroke/workspace/probe.txt ) 2>&1");
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
            target_of(plan.mounts(), "/upstroke/workspace").map(Mount::read_only),
            Some(role_is_review),
            "{tag}: the mount disposition"
        );
        let output = runner.run(&request).expect("runs");
        let wrote = workspace.join("probe.txt").exists();
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
        "upstroke-wrote-this"
    );
}

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
        crate::workspace_manager::execution_root_of(&identity.private_root, repo_key(), run_id);
    let mine = execution_root.join("tasks").join("kalpha-g0");
    let sibling = execution_root.join("tasks").join("kbeta-g0");
    repo::worktree(&repo_dir, &mine, &head);
    repo::worktree(&repo_dir, &sibling, &head);
    std::fs::write(sibling.join("sibling.txt"), "SIBLING-WORKTREE-a5f2\n").expect("a sibling file");
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

    let mut script = String::from("cat /upstroke/workspace/mine.txt;");
    for (_, path) in &withheld {
        let path = path.to_string_lossy().replace('\\', "/");
        script.push_str(&format!(
            " printf 'READ {path}: '; cat '{path}' 2>&1 | head -1; \
             printf 'WRITE {path}: '; \
             ( mkdir -p \"$(dirname '{path}')\" && \
               echo upstroke-container-wrote-this > '{path}' ) 2>&1 && echo WROTE || echo FAILED;"
        ));
    }
    let request = gate_request(
        ShellKind::Sh.spec(&script),
        mine.clone(),
        Duration::from_secs(60),
        gate_id(0),
    );
    let output = runner.run(&request).expect("runs");

    assert!(
        output.stdout.contains("MY-OWN-WORKTREE-a5f2"),
        "the gate could not read its own workspace, so nothing here is measured: {:?}",
        output.stdout
    );
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
    for ((category, path), original) in withheld.iter().zip(&before) {
        assert_eq!(
            &std::fs::read(path).expect("still there"),
            original,
            "a gate changed `{}` ({})",
            path.display(),
            category.passage()
        );
    }
    assert_eq!(repo::git_ok(&repo_dir, &["rev-parse", "HEAD"]), head);
}

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
        crate::workspace_manager::execution_root_of(&identity.private_root, repo_key(), run_id);
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

    const OUTSIDE: &[&str] = &[
        "/outside-role-mount",
        "/etc/upstroke-escaped",
        "/usr/local/bin/upstroke-escaped",
        "/upstroke/escape",
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
    for path in &inside {
        assert_eq!(said(path), format!("WROTE {path}"), "{:?}", output.stdout);
    }
    assert_eq!(
        std::fs::read_to_string(mine.join("written-by-the-gate"))
            .expect("the gate's own worktree write reached the host"),
        "owned"
    );
}

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
        crate::workspace_manager::execution_root_of(&identity.private_root, repo_key(), run_id);
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

    crate::runner::container::fake::preclean_names(
        docker.as_ref(),
        &RoleGitView::new(ContainerTrace::off()).for_reader(
            BoundaryLayout::DEFAULT_GIT_VIEW,
            BoundaryLayout::DEFAULT_GIT_OBJECTS,
        ),
        &identity.private_root,
        &[&name],
    );

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
        crate::workspace_manager::execution_root_of(&identity.private_root, repo_key(), run_id);
    let mine = execution_root.join("tasks").join("kalpha-g0");
    repo::worktree(&repo_dir, &mine, &head);

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

    assert_eq!(answers[0].1["PWD"], BoundaryLayout::DEFAULT_WORKSPACE);
    assert_eq!(answers[1].1["PWD"], BoundaryLayout::DEFAULT_SCRATCH);
    assert_ne!(answers[0].1["PWD"], answers[1].1["PWD"]);
    assert_eq!(
        answers[0].1["SHIM"], "PRESENT",
        "the shim is not in the attempt's worktree, so NOTFOUND below proves nothing"
    );
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
        crate::workspace_manager::execution_root_of(&identity.private_root, repo_key(), run_id);
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

    let git = "git -c safe.directory='*' -C /upstroke/workspace";
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

#[test]
#[cfg(unix)]
fn real_docker_a_container_contains_a_daemonised_descendant() {
    const LEADER_MARKER: &str = "903222";
    const DESCENDANT_MARKER: &str = "903111";

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

    let leader = pids_with(LEADER_MARKER);
    let descendant = pids_with(DESCENDANT_MARKER);
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

#[test]
fn every_gated_test_of_this_lane_is_counted() {
    const MINE: &[&str] = &[
        "real_docker_runs_from_the_recorded_image_id_and_composes_over_the_image_environment",
        "real_docker_refuses_a_reviewer_write_to_its_read_only_mount",
        "real_docker_confines_a_gate_to_its_mount",
        "real_docker_a_git_dependent_gate_sees_only_the_role_view",
        "real_docker_adapter_parsing_matches_the_host_table",
        "real_docker_a_gate_write_outside_every_declared_mount_fails",
        "real_docker_the_daemon_holds_exactly_the_specs_mounts_and_a_read_only_root",
        "real_docker_a_worktree_binary_cannot_shadow_the_certified_cli",
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
            .join("exec")
            .join("tests.rs"),
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
