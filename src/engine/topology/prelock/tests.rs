//! Extended notes: `docs/internals/engine/topology/prelock/tests.md`

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;
use crate::rundir::{NoHooks, create_private_dir, remove_public_husk};
use crate::runner::container::runtime::{
    ContainerExecution, CreateSpec, CreatedContainer, DiscoveredContainer, ImageInspection,
    Liveness, RuntimeError, RuntimeOp, StopMode,
};
use crate::topology::events::RunnerContract;

const IMAGE_REFERENCE: &str = "ghcr.io/upstroke/sandbox:1";
const IMAGE_ID: &str = "sha256:aaaabbbbccccddddeeeeffff0000111122223333444455556666777788889999";

#[derive(Debug, Default)]
struct Inventory {
    reachable: bool,
    images: BTreeMap<String, (String, Option<String>)>,
    volumes: Vec<String>,
    asked: Mutex<Vec<String>>,
}

impl Inventory {
    fn reachable() -> Self {
        Self {
            reachable: true,
            ..Self::default()
        }
    }

    fn with_image(mut self, reference: &str, id: &str, digest: Option<&str>) -> Self {
        self.images.insert(
            reference.to_owned(),
            (id.to_owned(), digest.map(str::to_owned)),
        );
        self
    }

    fn with_volume(mut self, name: &str) -> Self {
        self.volumes.push(name.to_owned());
        self
    }

    fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn record(&self, what: &str) {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(what.to_owned());
    }

    fn unreachable(&self) -> RuntimeError {
        RuntimeError::Unreachable {
            operation: RuntimeOp::Probe,
            detail: "no runtime on this machine".to_owned(),
        }
    }
}

impl ContainerRuntime for Inventory {
    fn probe(&self) -> Result<(), RuntimeError> {
        self.record("probe");
        if self.reachable {
            Ok(())
        } else {
            Err(self.unreachable())
        }
    }

    fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        self.record(&format!("image_by_reference {reference}"));
        Ok(self
            .images
            .get(reference)
            .map(|(id, digest)| ImageInspection {
                id: id.clone(),
                digest: digest.clone(),
                references: vec![reference.to_owned()],
            }))
    }

    fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        self.record(&format!("image_by_id {id}"));
        Ok(self
            .images
            .values()
            .find(|(known, _)| known == id)
            .map(|(known, digest)| ImageInspection {
                id: known.clone(),
                digest: digest.clone(),
                references: Vec::new(),
            }))
    }

    fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
        self.record(&format!("volume_present {name}"));
        Ok(self.volumes.iter().any(|known| known == name))
    }

    fn containers_with_label(
        &self,
        _key: &str,
        _value: &str,
    ) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
        Ok(Vec::new())
    }

    fn observe(&self, _name: &str) -> Result<Liveness, RuntimeError> {
        Ok(Liveness::Gone)
    }

    fn collect(&self, _name: &str) -> Result<ContainerExecution, RuntimeError> {
        Ok(ContainerExecution {
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {
        Ok(CreatedContainer {
            name: spec.name.clone(),
            reported_image_id: spec.image_id.clone(),
        })
    }

    fn start(&self, _name: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn stop(&self, _name: &str, _mode: StopMode) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn remove(&self, _name: &str) -> Result<(), RuntimeError> {
        Ok(())
    }
}

#[derive(Debug)]
struct Ids;

impl IdSource for Ids {
    fn question_id(&self) -> crate::ir::QuestionId {
        crate::ir::QuestionId("q-fixed".to_owned())
    }

    fn run_id(&self) -> String {
        "01KZTPR7B00000000000000001".to_owned()
    }

    fn incarnation(&self) -> IncarnationId {
        IncarnationId("01KZTINCB0000000000000001".to_owned())
    }

    fn pid(&self) -> u32 {
        4242
    }
}

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "upstroke-prelock-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        create_private_dir(&root, &mut NoHooks).expect("scratch root");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let reclaimed = remove_public_husk(&self.root, &mut NoHooks);
        assert!(
            reclaimed.is_ok() || std::thread::panicking(),
            "the scratch root {} was not reclaimed: {reclaimed:?}",
            self.root.display()
        );
    }
}

fn container_selection(image: &str, volumes: &[(&str, &str)]) -> RunnerSelection {
    RunnerSelection {
        kind: RunnerKind::Container,
        image: Some(image.to_owned()),
        credential_volumes: volumes
            .iter()
            .map(|(agent, volume)| ((*agent).to_owned(), (*volume).to_owned()))
            .collect(),
        mounts: Vec::new(),
        from_config: true,
    }
}

#[test]
fn a_host_selection_resolves_host_v1_and_carries_its_digest() {
    let root = Scratch::new("host");
    let selection = RunnerSelection::host_default();
    let checked = check(&PreLock {
        selection: &selection,
        runtime: None,
        private_root: root.path(),
        ids: &Ids,
    })
    .expect("the host runner resolves with nothing to inspect");

    assert_eq!(checked.runner_policy().kind, RunnerKind::Host);
    assert_eq!(checked.runner_policy().policy, RunnerContract::HostV1);
    assert!(checked.runner_policy().image.is_none());
    assert_eq!(
        checked.runner_policy_sha256(),
        runner_policy_sha256(checked.runner_policy()),
        "the carried digest is the digest of the carried policy"
    );
    assert_eq!(checked.run_id(), "01KZTPR7B00000000000000001");
    assert_eq!(checked.incarnation().0, "01KZTINCB0000000000000001");
    assert_eq!(checked.pid(), 4242);
    assert_eq!(
        checked.private_dir(),
        checked.private_root().join("runs").join(checked.run_id()),
        "the locator is <R>/runs/<run_id> and nothing else"
    );
}

#[test]
fn the_pre_lock_checks_leave_no_residue() {
    let root = Scratch::new("residue");
    let before = names_under(root.path());
    let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
    let runtime = Inventory::reachable()
        .with_image(IMAGE_REFERENCE, IMAGE_ID, Some("sha256:manifest"))
        .with_volume("upstroke-codex");

    let checked = check(&PreLock {
        selection: &selection,
        runtime: Some(&runtime),
        private_root: root.path(),
        ids: &Ids,
    })
    .expect("a reachable runtime holding the image and the volume resolves");

    assert_eq!(
        names_under(root.path()),
        before,
        "the pre-lock phase created something under the private root"
    );
    assert!(
        !checked.private_dir().exists(),
        "the private half must not exist before P3a"
    );
    for question in runtime.asked() {
        assert!(
            question.starts_with("probe")
                || question.starts_with("image_by_")
                || question.starts_with("volume_present"),
            "`{question}` is not a read-only inspection"
        );
    }
}

fn names_under(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_container_inspections_run_in_order_and_the_first_failure_ends_them() {
    let root = Scratch::new("order");
    let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
    let runtime = Inventory::reachable()
        .with_image(IMAGE_REFERENCE, IMAGE_ID, None)
        .with_volume("upstroke-codex");
    check(&PreLock {
        selection: &selection,
        runtime: Some(&runtime),
        private_root: root.path(),
        ids: &Ids,
    })
    .expect("resolves");
    assert_eq!(
        runtime.asked(),
        vec![
            "probe".to_owned(),
            format!("image_by_reference {IMAGE_REFERENCE}"),
            "volume_present upstroke-codex".to_owned(),
        ],
        "reachability, then the image, then the volumes"
    );

    let unreachable = Inventory::default();
    let refusal = check(&PreLock {
        selection: &selection,
        runtime: Some(&unreachable),
        private_root: root.path(),
        ids: &Ids,
    })
    .expect_err("an unreachable runtime refuses");
    assert!(
        refusal.to_string().contains("runtime"),
        "the refusal names the runtime: {refusal}"
    );
    assert_eq!(
        unreachable.asked(),
        vec!["probe".to_owned()],
        "the image and the volumes were not asked about after the runtime refused"
    );
}

#[test]
fn an_absent_credential_volume_refuses() {
    let root = Scratch::new("volume");
    let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
    let runtime = Inventory::reachable().with_image(IMAGE_REFERENCE, IMAGE_ID, None);
    let refusal = check(&PreLock {
        selection: &selection,
        runtime: Some(&runtime),
        private_root: root.path(),
        ids: &Ids,
    })
    .expect_err("an absent volume refuses");
    assert!(
        refusal.to_string().contains("upstroke-codex"),
        "the refusal names the volume: {refusal}"
    );
}

#[test]
fn a_container_selection_without_a_runtime_refuses() {
    let root = Scratch::new("noruntime");
    let selection = container_selection(IMAGE_REFERENCE, &[]);
    let refusal = check(&PreLock {
        selection: &selection,
        runtime: None,
        private_root: root.path(),
        ids: &Ids,
    })
    .expect_err("no runtime to inspect");
    assert!(
        refusal.to_string().contains(IMAGE_REFERENCE),
        "the refusal names the image it could not look for: {refusal}"
    );
}

#[test]
fn a_private_root_that_is_not_a_real_directory_refuses_before_any_inspection() {
    let root = Scratch::new("root");
    let absent = root.path().join("absent");
    let selection = container_selection(IMAGE_REFERENCE, &[]);
    let runtime = Inventory::reachable().with_image(IMAGE_REFERENCE, IMAGE_ID, None);
    let refusal = check(&PreLock {
        selection: &selection,
        runtime: Some(&runtime),
        private_root: &absent,
        ids: &Ids,
    })
    .expect_err("an absent private root refuses");
    assert!(
        refusal.to_string().contains("absent"),
        "the refusal names the root: {refusal}"
    );
    assert!(
        runtime.asked().is_empty(),
        "the runtime was inspected before the root was even checked"
    );
}

#[test]
fn the_authorized_private_root_is_canonical() {
    let root = Scratch::new("canonical");
    let indirect = root.path().join(".").join("..").join(
        root.path()
            .file_name()
            .expect("the scratch root has a basename")
            .to_string_lossy()
            .as_ref(),
    );
    let selection = RunnerSelection::host_default();
    let checked = check(&PreLock {
        selection: &selection,
        runtime: None,
        private_root: &indirect,
        ids: &Ids,
    })
    .expect("a root reachable through `.`/`..` is the same root");
    assert_eq!(
        checked.private_root(),
        std::fs::canonicalize(root.path())
            .expect("the scratch root canonicalizes")
            .as_path(),
        "the witness carries the canonical root, not the spelling it was given"
    );
}

#[test]
fn a_scratch_root_is_reclaimed_on_every_exit_including_an_unwind() {
    let ordinary = {
        let root = Scratch::new("raii-ordinary");
        let path = root.path().to_path_buf();
        create_private_dir(&path.join("nested"), &mut NoHooks).expect("a child of the root");
        assert!(path.join("nested").is_dir(), "the child was not created");
        path
    };
    assert!(
        !ordinary.exists(),
        "the scratch root {} outlived its guard on the ordinary exit",
        ordinary.display()
    );

    let recorded = Mutex::new(None);
    let unwound = std::panic::catch_unwind(|| {
        let root = Scratch::new("raii-unwind");
        let path = root.path().to_path_buf();
        *recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path.clone());
        assert!(!path.is_dir(), "a deliberate failure, mid-test");
    });
    assert!(
        unwound.is_err(),
        "the closure was supposed to unwind, so nothing about the panic path was measured"
    );
    let path = recorded
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .expect("the closure recorded its root before it panicked");
    assert!(
        !path.exists(),
        "the scratch root {} survived the unwind",
        path.display()
    );
}

#[test]
fn a_scratch_root_that_cannot_be_reclaimed_is_reported_rather_than_discarded() {
    let reported = std::panic::catch_unwind(|| {
        let root = Scratch::new("raii-reported");
        remove_public_husk(root.path(), &mut NoHooks).expect("the tree reclaims early");
    })
    .expect_err("the guard discarded a failed reclamation");

    let message = reported
        .downcast_ref::<String>()
        .map_or_else(String::new, Clone::clone);
    assert!(
        message.contains("was not reclaimed") && message.contains("raii-reported"),
        "the report must name the root it could not reclaim: {message}"
    );
}

#[test]
#[ignore = "spawned by a_failed_reclamation_during_an_unwind_does_not_abort_the_process"]
fn scratch_unwind_with_a_failed_reclamation_child() {
    const PRIMARY: &str = "the primary failure this witness keeps observable";

    let caught = std::panic::catch_unwind(|| {
        let root = Scratch::new("raii-unwind-unreclaimable");
        remove_public_husk(root.path(), &mut NoHooks).expect("the tree reclaims early");
        assert!(
            !root.path().exists(),
            "the guard's own removal has to be the one that fails, and it would succeed \
             against a root that is still there"
        );
        panic!("{PRIMARY}");
    })
    .expect_err("the closure was supposed to unwind");

    let message = caught
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| caught.downcast_ref::<&str>().map(|m| (*m).to_owned()))
        .unwrap_or_default();
    assert_eq!(
        message, PRIMARY,
        "the destructor's own report displaced the primary panic, so the failure a test \
         would be diagnosing is the one that got lost"
    );
}

#[test]
fn a_failed_reclamation_during_an_unwind_does_not_abort_the_process() {
    use crate::runner::host::HostRunner;
    use crate::runner::{
        CommandSpec, ExecutionRole, InvocationId, ProbeTarget, Runner, RunnerRequest,
    };

    let workspace = Scratch::new("raii-abort-isolation");
    let exe = std::env::current_exe().expect("the test binary knows where it is");
    let request = RunnerRequest {
        command: CommandSpec {
            program: exe.display().to_string(),
            args: vec![
                "--exact".to_owned(),
                "engine::topology::prelock::tests::scratch_unwind_with_a_failed_reclamation_child"
                    .to_owned(),
                "--ignored".to_owned(),
                "--test-threads".to_owned(),
                "1".to_owned(),
            ],
            env: Vec::new(),
            stdin: Vec::new(),
        },
        workspace: workspace.path().to_path_buf(),
        role: ExecutionRole::Gate,
        timeout: std::time::Duration::from_secs(120),
        agent: None,
        invocation: InvocationId::probe(ProbeTarget::Shell, 11)
            .expect("a probe identity for the spawned child"),
    };
    let output = HostRunner::new().run(&request).expect("the child runs");

    assert!(
        !output.timed_out,
        "the child never finished, so nothing about the unwind was observed"
    );
    assert_eq!(
        output.code,
        Some(0),
        "the child did not complete as designed — a destructor that panicked into the live \
         unwind aborts it, and a process killed by that abort reports no exit code at all. \
         Its stderr: {}",
        output.stderr
    );
    assert!(
        output.stdout.contains("test result: ok. 1 passed"),
        "the harness printed no passing result for exactly one selected test, so the child \
         either aborted before printing or selected nothing at all: {}",
        output.stdout
    );
}
