//! The container substrate's own suite.
//!
//! Four things this file is organised around, each learned expensively on this
//! project:
//!
//! * **Orderings are most of the contract.** "intent synced before docker
//!   create", "verified before start", "view mounted before start", "stop/rm,
//!   view removal, intent removal after completion", and reclaim's own five
//!   steps are each an independently droppable predicate. Every one is asserted
//!   as a **sequence** taken from [`ContainerTrace`], never as membership.
//! * **A function may not be its own oracle.** Every expected digest and every
//!   expected name in this file is a literal, computed out of band with
//!   `python3 -c 'hashlib.sha256(...)'` against the packet's own template, and
//!   the tuple that produces it is written beside it.
//! * **Fixtures vary every independently meaningful field independently**, and
//!   hostility is asserted as **distinct-value counts**.
//! * **The dominant defect is two axes covered separately with the intersection
//!   never built.** Each test below names the second field it holds constant.

// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/runner/container.rs` -- the same shape `src/events/log.rs`
// and `src/events/log/tests.rs` have, which is PR5's precedent for a funnel's
// own test module. This file drives the eight site-taking APIs and plants the
// residue they are meant to find, so it names `fs::write`, `fs::create_dir_all`
// and the seam's own effectful methods directly.
//
// `PR6-LANEF-004`: it carries this allow **of its own** because the funnel's no
// longer reaches it. The two lints it does not need are re-denied, so a
// `std::process::Command` or a `println!` appearing here is still a build error.
// `decisions.effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types, clippy::disallowed_macros)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::fake::absent_reason;
use super::intent::{
    self, CONTAINERS_DIR, ContainerIntent, ContainerName, INTENT_SUFFIX, LABEL_INCARNATION,
    LABEL_INVOCATION, LABEL_PRIVATE_ROOT, LABEL_RUN, LABEL_RUN_DIR, LABELS, containers_dir,
    invocation_hash, private_root_label,
};
use super::runtime::{
    ContainerExecution, ContainerRuntime, ContainerTrace, CreateSpec, ImageInspection, Liveness,
    Mount, OwnerLiveness, RuntimeError, RuntimeOp, StopMode, TracePhase,
};
use super::{
    DOCKER_GATED_TESTS, DisposableDirView, DockerCli, FakeOwnerLiveness, FakeRuntime, FoundIntent,
    GitView, GitViewRequest, LaunchPlan, Launched, NoHooks, OrphanWindow, PS_FIELD_SEPARATOR,
    PS_FORMAT, PS_LABELS, RecordingHooks, TERMINATION_OBSERVATIONS, classify_docker_failure,
    create_container, docker_gate, is_unreachable_diagnostic, launch, list_intents, mount_git_view,
    observe_terminated, parse_ps_output, read_intent, reclaim, release, remove_container,
    remove_intent, start_container, stop_container, unmount_git_view, write_intent,
};
use crate::error::UpstrokeError;
use crate::runner::{AgentId, CommandSpec, InvocationId, ProbeTarget, host};
use crate::topology::effects::{
    Adjacent, ContainerSite, DurableEvent, EffectSiteId, FaultRow, ResourceRow, SiteScope,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A scratch private root, in the idiom of `effects::tests::scratch_dir`.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "upstroke-container-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch private root");
    dir
}

/// The four name components used across this file, each a distinct value so a
/// swap between two of them is visible.
const REPO_KEY: &str = "0123456789abcdef";
const RUN_A: &str = "01KZRN48A4ZK3AEDST3RJ8HMA4";
const RUN_B: &str = "01KZS7R0V1ZD6MC290MG350QXF";
const INCARNATION_1: &str = "01KZTAAAAAAAAAAAAAAAAAAAAA";
const INCARNATION_2: &str = "01KZTBBBBBBBBBBBBBBBBBBBBB";

/// The recorded image id, and a different one, and a third.
const IMAGE_ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const OTHER_IMAGE_ID: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const IMAGE_REFERENCE: &str = "ghcr.io/example/upstroke-runner:v1";
const MANIFEST_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const POLICY_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

/// A shell probe identity. Deterministic across incarnations **by
/// construction** — `InvocationId::Probe`'s own doc says so — which is why the
/// container name carries the incarnation.
fn shell_probe() -> InvocationId {
    InvocationId::probe(ProbeTarget::Shell, 0).expect("the shell probe identity")
}

/// An agent probe identity.
fn agent_probe() -> InvocationId {
    InvocationId::probe(ProbeTarget::Agent(AgentId::new("claude-code")), 0)
        .expect("the agent probe identity")
}

/// The intent record for `run`/`incarnation`, with every field a distinct
/// value.
fn intent_for(run: &str, incarnation: &str, invocation: &InvocationId) -> ContainerIntent {
    ContainerIntent {
        run_id: run.to_owned(),
        run_dir: format!("/srv/public/{run}"),
        incarnation: incarnation.to_owned(),
        repo_key: REPO_KEY.to_owned(),
        invocation: invocation.render(),
        runner_policy_sha256: POLICY_DIGEST.to_owned(),
    }
}

/// The name for `run`/`incarnation`.
fn name_for(run: &str, incarnation: &str, invocation: &InvocationId) -> ContainerName {
    ContainerName::new(REPO_KEY, run, incarnation, invocation).expect("a container name")
}

/// The five labels a container of this run carries.
fn labels_for(root: &Path, record: &ContainerIntent) -> BTreeMap<String, String> {
    record.labels(root)
}

/// A create spec that asks for `image_id`.
fn spec_for(
    name: &ContainerName,
    record: &ContainerIntent,
    root: &Path,
    image_id: &str,
) -> CreateSpec {
    CreateSpec {
        name: name.as_str().to_owned(),
        image_id: image_id.to_owned(),
        labels: labels_for(root, record),
        mounts: vec![Mount::Path {
            source: PathBuf::from("/srv/work/task"),
            target: "/work".to_owned(),
            read_only: false,
        }],
        env: vec![("HOME".to_owned(), "/home/upstroke".to_owned())],
        command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()],
        workdir: Some("/work".to_owned()),
        read_only_root: true,
    }
}

/// A whole plan, plus a fake runtime already holding the recorded image.
struct Fixture {
    root: PathBuf,
    trace: ContainerTrace,
    runtime: FakeRuntime,
    view: DisposableDirView,
    plan: LaunchPlan,
}

impl Fixture {
    fn new(tag: &str, run: &str, incarnation: &str, invocation: &InvocationId) -> Self {
        let root = scratch(tag);
        let trace = ContainerTrace::recording();
        let runtime = FakeRuntime::new(trace.clone());
        runtime.add_image(IMAGE_ID, Some(MANIFEST_DIGEST));
        runtime.add_image(OTHER_IMAGE_ID, None);
        runtime.tag(IMAGE_REFERENCE, IMAGE_ID);
        let record = intent_for(run, incarnation, invocation);
        let name = name_for(run, incarnation, invocation);
        let spec = spec_for(&name, &record, &root, IMAGE_ID);
        let view = GitViewRequest {
            path: root.join("views").join(name.as_str()),
            workspace: PathBuf::from("/srv/work/task"),
            head: Some("0".repeat(40)),
        };
        Self {
            plan: LaunchPlan {
                private_root: root.clone(),
                name,
                invocation: invocation.clone(),
                intent: record,
                spec,
                view,
            },
            view: DisposableDirView::new(trace.clone()),
            runtime,
            trace,
            root,
        }
    }

    fn hooks(&self) -> RecordingHooks {
        RecordingHooks::new(self.trace.clone())
    }
}

/// What a Docker-gated test does when there is no runtime.
///
/// It **reads** the reason rather than returning silently, so a skip that had
/// stopped saying why would not compile. Combined with
/// [`super::fake::REQUIRE_DOCKER`] — which turns a skip into a failure on a
/// machine that has Docker — and with
/// [`every_docker_gated_test_is_named_and_present`], which counts the gated
/// tests by name, this is the whole of "loud and counted, never silent".
fn skipped(reason: &str) {
    assert_eq!(
        reason,
        absent_reason(),
        "a Docker-gated test skipped for a reason the gate does not know about"
    );
}

/// What a Docker-gated test does when the runtime holds no usable image.
///
/// The second absence, and it is a different one: Docker answers, and there is
/// nothing to inspect. It is loud under the same variable, because a machine
/// that has a runtime and no image would otherwise pass three tests that never
/// touched it.
fn no_image(reason: &str) {
    assert!(reason.contains("never pull"), "{reason}");
    assert!(
        std::env::var_os(super::fake::REQUIRE_DOCKER).is_none(),
        "{} is set and a gated test found no usable image: {reason}",
        super::fake::REQUIRE_DOCKER
    );
}

/// Where `needle` first appears in the trace, or a failure naming the whole
/// sequence — because "x before y" is unreadable when the report is `None`.
fn at(trace: &ContainerTrace, needle: &str) -> usize {
    trace.position(needle).unwrap_or_else(|| {
        panic!(
            "`{needle}` is not in the trace, which is {:#?}",
            trace.rendered()
        )
    })
}

// ---------------------------------------------------------------------------
// 1. The fake's six required capabilities
// ---------------------------------------------------------------------------

/// (1) an image table keyed by **immutable id**, with references and digests.
///
/// Second field held constant: the runtime is reachable throughout, so what
/// varies is only which key the table is read by. Without an id-keyed table,
/// `image_by_id` could not answer at all and the rebuild path's refusal — "the
/// **recorded image id** is absent from the runtime" — would be unwritable.
#[test]
fn the_image_table_is_keyed_by_id_and_references_resolve_through_it() {
    let runtime = FakeRuntime::new(ContainerTrace::recording());
    runtime.add_image(IMAGE_ID, Some(MANIFEST_DIGEST));
    runtime.add_image(OTHER_IMAGE_ID, None);
    runtime.tag(IMAGE_REFERENCE, IMAGE_ID);

    let by_id = runtime
        .image_by_id(IMAGE_ID)
        .expect("reachable")
        .expect("present");
    assert_eq!(by_id.id, IMAGE_ID);
    assert_eq!(by_id.digest.as_deref(), Some(MANIFEST_DIGEST));

    let by_reference = runtime
        .image_by_reference(IMAGE_REFERENCE)
        .expect("reachable")
        .expect("present");
    assert_eq!(
        by_reference.id, IMAGE_ID,
        "the reference resolves to the id"
    );
    assert_eq!(by_reference.references, vec![IMAGE_REFERENCE.to_owned()]);

    // "the manifest digest **when reported**" — absent is a real state and a
    // separately encodable one, not a missing fixture.
    let without = runtime
        .image_by_id(OTHER_IMAGE_ID)
        .expect("reachable")
        .expect("present");
    assert_eq!(without.digest, None);

    // The two questions are independent: an id present under no reference is
    // findable by id and by no reference.
    assert_eq!(runtime.image_by_reference("ghcr.io/nobody:v9"), Ok(None));
    assert_eq!(runtime.image_by_id("sha256:absent"), Ok(None));
}

/// (2) a **mutable tag table** — a reference can be moved to another id while
/// the id stays.
///
/// ST-20: "a resume after the recorded reference was moved to another image
/// warns and creates every container from the recorded id". Without a mutable
/// tag table that sentence has no fixture at all.
///
/// Second field held constant: the image table itself. Both ids are present
/// before and after; only the tag moves — which is the whole point, because a
/// fixture that also deleted the old id would prove the wrong thing.
#[test]
fn a_reference_can_be_moved_to_another_id_and_the_old_id_stays() {
    let runtime = FakeRuntime::new(ContainerTrace::recording());
    runtime.add_image(IMAGE_ID, Some(MANIFEST_DIGEST));
    runtime.add_image(OTHER_IMAGE_ID, None);
    runtime.tag(IMAGE_REFERENCE, IMAGE_ID);

    runtime.move_tag(IMAGE_REFERENCE, OTHER_IMAGE_ID);

    assert_eq!(
        runtime
            .image_by_reference(IMAGE_REFERENCE)
            .expect("reachable")
            .expect("present")
            .id,
        OTHER_IMAGE_ID,
        "the reference now names another image"
    );
    assert!(
        runtime.image_by_id(IMAGE_ID).expect("reachable").is_some(),
        "the recorded id is still resolvable, which is what lets the rebuild \
         create from it while the reference has moved"
    );
    // Two distinct answers to two distinct questions about one reference: the
    // intersection {image id recorded} x {reference moved} rather than either
    // alone.
    let answers: BTreeSet<String> = [
        runtime
            .image_by_reference(IMAGE_REFERENCE)
            .expect("reachable")
            .expect("present")
            .id,
        runtime
            .image_by_id(IMAGE_ID)
            .expect("reachable")
            .expect("present")
            .id,
    ]
    .into_iter()
    .collect();
    assert_eq!(answers.len(), 2);
}

/// (3) per-container **reported image ids with substitution injection**.
///
/// The correlated-fixture trap this slice was warned about: if the reported id
/// were set from the requested id there would be no way to build a
/// substitution, and `substituted_image_id_refused_before_start` would be green
/// because it could not be written. This is the test that proves the two are
/// separate inputs.
#[test]
fn the_fake_can_report_an_image_id_that_differs_from_the_one_create_asked_for() {
    let trace = ContainerTrace::recording();
    let runtime = FakeRuntime::new(trace);
    runtime.add_image(IMAGE_ID, None);
    let spec = CreateSpec {
        name: "upstroke-a-b-c-d".to_owned(),
        image_id: IMAGE_ID.to_owned(),
        labels: BTreeMap::new(),
        mounts: Vec::new(),
        env: Vec::new(),
        command: Vec::new(),
        workdir: None,
        read_only_root: true,
    };

    // Healthy: the runtime reports what it was asked for.
    let honest = runtime.create(&spec).expect("created");
    assert_eq!(honest.reported_image_id, IMAGE_ID);
    runtime.remove(&spec.name).expect("removed");

    // Injected: it does not.
    runtime.substitute_reported_image_id(&spec.name, OTHER_IMAGE_ID);
    let substituted = runtime.create(&spec).expect("created");
    assert_eq!(substituted.reported_image_id, OTHER_IMAGE_ID);
    assert_ne!(
        substituted.reported_image_id, spec.image_id,
        "the reported id and the requested id are separate inputs; if this ever \
         becomes impossible, every image-verification test in this slice is vacuous"
    );

    // And the container the fake holds records both, separately.
    let held = runtime.container(&spec.name).expect("held");
    assert_eq!(held.requested_image_id, IMAGE_ID);
    assert_eq!(held.reported_image_id, OTHER_IMAGE_ID);
}

/// (4) **volume presence toggles**.
///
/// R20 is operator-owned and `persistent_output` in all five `at_run_end`
/// outcomes — "never created or pruned by a run" — so the only thing a run does
/// with a volume is *observe* it, and absence is a refusal. Second field held
/// constant: the image table, so a refusal here cannot be an image problem
/// wearing a volume's name.
#[test]
fn volume_presence_is_a_toggle_and_absence_refuses_a_create() {
    let trace = ContainerTrace::recording();
    let runtime = FakeRuntime::new(trace);
    runtime.add_image(IMAGE_ID, None);
    assert!(
        !runtime
            .volume_present("upstroke-claude")
            .expect("reachable")
    );
    runtime.add_volume("upstroke-claude");
    assert!(
        runtime
            .volume_present("upstroke-claude")
            .expect("reachable")
    );
    runtime.remove_volume("upstroke-claude");
    assert!(
        !runtime
            .volume_present("upstroke-claude")
            .expect("reachable")
    );

    let spec = CreateSpec {
        name: "upstroke-a-b-c-d".to_owned(),
        image_id: IMAGE_ID.to_owned(),
        labels: BTreeMap::new(),
        mounts: vec![Mount::Volume {
            name: "upstroke-claude".to_owned(),
            target: "/home/upstroke/.claude".to_owned(),
            read_only: false,
        }],
        env: Vec::new(),
        command: Vec::new(),
        workdir: None,
        read_only_root: true,
    };
    let refused = runtime.create(&spec).expect_err("an absent volume refuses");
    assert!(!refused.is_unreachable(), "the runtime answered; it failed");
    runtime.add_volume("upstroke-claude");
    assert!(runtime.create(&spec).is_ok(), "and present, it creates");
}

/// (5) an **availability toggle**, and it is per operation.
///
/// The reachability decision this lane made, stated as a test: a runtime that
/// answers `docker ps` and fails `docker inspect` is a real state, and a seam
/// with one global boolean could not express it. The intersection here is
/// {operation} x {reachable?}, which one boolean collapses.
#[test]
fn the_availability_toggle_is_per_operation_so_ps_can_answer_while_inspect_cannot() {
    let runtime = FakeRuntime::new(ContainerTrace::recording());
    runtime.add_image(IMAGE_ID, None);
    runtime.set_unreachable(RuntimeOp::InspectImageById);

    // `ps` answers.
    assert_eq!(
        runtime
            .containers_with_label(LABEL_PRIVATE_ROOT, "/srv/private")
            .expect("ps is reachable"),
        Vec::new()
    );
    // `inspect` does not, and says which operation could not be reached.
    let error = runtime
        .image_by_id(IMAGE_ID)
        .expect_err("inspect is unreachable");
    assert!(error.is_unreachable());
    assert_eq!(error.operation(), RuntimeOp::InspectImageById);

    // The whole daemon down is the other end of the same toggle, and every
    // operation reports it.
    runtime.set_all_unreachable();
    let unreachable: BTreeSet<RuntimeOp> = RuntimeOp::ALL
        .iter()
        .filter(|op| match op {
            RuntimeOp::Probe => runtime.probe().is_err(),
            RuntimeOp::InspectImageByReference => runtime.image_by_reference("x").is_err(),
            RuntimeOp::InspectImageById => runtime.image_by_id("x").is_err(),
            RuntimeOp::InspectVolume => runtime.volume_present("x").is_err(),
            RuntimeOp::ListByLabel => runtime.containers_with_label("k", "v").is_err(),
            RuntimeOp::Observe => runtime.observe("x").is_err(),
            RuntimeOp::Collect => runtime.collect("x").is_err(),
            RuntimeOp::Create => runtime
                .create(&CreateSpec {
                    name: "x".to_owned(),
                    image_id: IMAGE_ID.to_owned(),
                    labels: BTreeMap::new(),
                    mounts: Vec::new(),
                    env: Vec::new(),
                    command: Vec::new(),
                    workdir: None,
                    read_only_root: true,
                })
                .is_err(),
            RuntimeOp::Start => runtime.start("x").is_err(),
            RuntimeOp::Stop => runtime.stop("x", StopMode::Kill).is_err(),
            RuntimeOp::Remove => runtime.remove("x").is_err(),
        })
        .copied()
        .collect();
    assert_eq!(
        unreachable.len(),
        RuntimeOp::ALL.len(),
        "every operation of the seam has to be able to report unreachability, \
         or a refusal that depends on one of them cannot be written"
    );
    assert_eq!(RuntimeOp::ALL.len(), 11);
}

/// (6) owner **labels**, **incarnations**, and the two image ids as separate
/// inputs.
///
/// Second field held constant: the label *keys* are the packet's five for both
/// containers; what varies is the run and the incarnation, which is the axis
/// the census classifies on.
#[test]
fn a_seeded_container_carries_owner_labels_and_an_incarnation() {
    let runtime = FakeRuntime::new(ContainerTrace::recording());
    let root = PathBuf::from("/srv/private");
    let mine = intent_for(RUN_A, INCARNATION_1, &shell_probe());
    let earlier = intent_for(RUN_A, INCARNATION_2, &shell_probe());
    let foreign = intent_for(RUN_B, INCARNATION_1, &agent_probe());

    for (tag, record) in [
        ("mine", &mine),
        ("earlier", &earlier),
        ("foreign", &foreign),
    ] {
        runtime.seed_container(
            tag,
            record.labels(&root),
            IMAGE_ID,
            // Separate argument, always.
            IMAGE_ID,
            Liveness::Running,
        );
    }

    let found = runtime
        .containers_with_label(LABEL_PRIVATE_ROOT, "/srv/private")
        .expect("reachable");
    assert_eq!(found.len(), 3, "all three share one private root");

    let runs: BTreeSet<&str> = found.iter().filter_map(|c| c.label(LABEL_RUN)).collect();
    let incarnations: BTreeSet<&str> = found
        .iter()
        .filter_map(|c| c.label(LABEL_INCARNATION))
        .collect();
    let invocations: BTreeSet<&str> = found
        .iter()
        .filter_map(|c| c.label(LABEL_INVOCATION))
        .collect();
    // Distinct-value counts, not prose: two runs, two incarnations, two
    // invocations, and the pairs are not the same partition — which is what
    // makes {owner run} x {incarnation} a real grid rather than one axis twice.
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(incarnations.len(), 2, "{incarnations:?}");
    assert_eq!(invocations.len(), 2, "{invocations:?}");
    let pairs: BTreeSet<(&str, &str)> = found
        .iter()
        .filter_map(|c| Some((c.label(LABEL_RUN)?, c.label(LABEL_INCARNATION)?)))
        .collect();
    assert_eq!(pairs.len(), 3, "three distinct (run, incarnation) pairs");
}

/// (6b) **liveness simulation**, and the shape that makes an incarnation
/// unreadable from a lock.
///
/// `crash_reconstruction`: the incarnation id "is **never read from lock-file
/// contents**". [`OwnerLiveness`] answers one bit about a public run directory,
/// so there is no incarnation in the return type to read — the defect is not
/// refused, it is unexpressible.
#[test]
fn owner_liveness_answers_one_bit_and_carries_no_incarnation() {
    let liveness = FakeOwnerLiveness::new();
    let live = PathBuf::from("/srv/public/live");
    let dead = PathBuf::from("/srv/public/dead");
    liveness.set_live(&live);

    assert!(liveness.is_running(&live));
    assert!(!liveness.is_running(&dead));
    liveness.set_dead(&live);
    assert!(!liveness.is_running(&live));

    // The production probe is `rundir::is_running`, and it answers the same
    // shape for a directory that never held a run.
    let probe = super::runtime::LockProbe;
    assert!(
        !probe.is_running(&scratch("liveness")),
        "a directory with no run.lock has no live owner"
    );
}

/// The call log is ordered and holds every operation.
///
/// The instrument the rest of this file rests on. Second field held constant:
/// one runtime, one trace; what varies is only how many operations have run.
#[test]
fn the_call_log_is_ordered_and_holds_every_operation() {
    let trace = ContainerTrace::recording();
    let runtime = FakeRuntime::new(trace.clone());
    runtime.add_image(IMAGE_ID, None);
    let spec = CreateSpec {
        name: "upstroke-a-b-c-d".to_owned(),
        image_id: IMAGE_ID.to_owned(),
        labels: BTreeMap::new(),
        mounts: Vec::new(),
        env: Vec::new(),
        command: Vec::new(),
        workdir: None,
        read_only_root: true,
    };
    runtime.probe().expect("reachable");
    runtime.create(&spec).expect("created");
    runtime.start(&spec.name).expect("started");
    runtime.stop(&spec.name, StopMode::Kill).expect("stopped");
    runtime.observe(&spec.name).expect("observed");
    runtime.remove(&spec.name).expect("removed");

    assert_eq!(
        runtime.calls(),
        vec![
            RuntimeOp::Probe,
            RuntimeOp::Create,
            RuntimeOp::Start,
            RuntimeOp::Stop,
            RuntimeOp::Observe,
            RuntimeOp::Remove,
        ],
        "the call log is a sequence; a set would hold none of this slice's orderings"
    );
    assert_eq!(
        trace.rendered().first().map(String::as_str),
        Some("rt:probe:daemon")
    );
}

// ---------------------------------------------------------------------------
// 2. The eight sites and the funnel's shape
// ---------------------------------------------------------------------------

/// The row, adjacency, fault row and scope of each of the eight sites,
/// transcribed from the packet rather than read back from the enum that
/// produces them.
///
/// `effect_site_inventory.identity`: "Container.* (R19/R26; Container.Create
/// verifies the created container's image id against the record before
/// Container.Start)", and `slice_contract.owned_resources` splits them:
/// "R26 container + labels + global intent incl. runner digest", "R19
/// disposable Git view per request".
#[test]
fn every_container_sites_row_adjacency_fault_row_and_scope_is_the_packets() {
    const EXPECTED: &[(ContainerSite, ResourceRow, Adjacent, FaultRow, SiteScope)] = &[
        (
            ContainerSite::WriteIntent,
            ResourceRow::R26,
            Adjacent::After(DurableEvent::AttemptStarted),
            FaultRow::TContainer,
            SiteScope::Topology,
        ),
        (
            ContainerSite::Create,
            ResourceRow::R26,
            Adjacent::After(DurableEvent::AttemptStarted),
            FaultRow::TContainer,
            SiteScope::Topology,
        ),
        (
            ContainerSite::Start,
            ResourceRow::R26,
            Adjacent::After(DurableEvent::AttemptStarted),
            FaultRow::TContainer,
            SiteScope::Topology,
        ),
        (
            ContainerSite::MountGitView,
            ResourceRow::R19,
            Adjacent::After(DurableEvent::AttemptStarted),
            FaultRow::TContainer,
            SiteScope::Topology,
        ),
        (
            ContainerSite::Stop,
            ResourceRow::R26,
            Adjacent::Before(DurableEvent::AttemptFinished),
            FaultRow::TContainer,
            SiteScope::Topology,
        ),
        (
            ContainerSite::Remove,
            ResourceRow::R26,
            Adjacent::Before(DurableEvent::AttemptFinished),
            FaultRow::TContainer,
            SiteScope::Topology,
        ),
        (
            ContainerSite::UnmountGitView,
            ResourceRow::R19,
            Adjacent::Before(DurableEvent::AttemptFinished),
            FaultRow::TContainer,
            SiteScope::Topology,
        ),
        (
            ContainerSite::RemoveIntent,
            ResourceRow::R26,
            Adjacent::Before(DurableEvent::AttemptFinished),
            FaultRow::TContainer,
            SiteScope::Topology,
        ),
    ];
    assert_eq!(EXPECTED.len(), ContainerSite::ALL.len());
    assert_eq!(
        ContainerSite::ALL.len(),
        8,
        "the frozen inventory has eight"
    );
    for (site, row, adjacent, fault, scope) in EXPECTED {
        assert_eq!(site.row(), *row, "{}", site.name());
        assert_eq!(site.adjacent(), *adjacent, "{}", site.name());
        assert_eq!(site.fault_row(), *fault, "{}", site.name());
        assert_eq!(site.scope(), *scope, "{}", site.name());
    }
    // Two of the eight are R19 and six are R26, which is the split
    // `owned_resources` states. A count, so a site moved between rows fails
    // here as well as in its own row above.
    let r19 = EXPECTED.iter().filter(|e| e.1 == ResourceRow::R19).count();
    assert_eq!(r19, 2);
    assert_eq!(EXPECTED.len() - r19, 6);

    // No Container site exposes a parent-side sub-effect point or registers a
    // command-internal residue class, and both absences are **stated** rather
    // than left unmentioned. `command_internal_sub_effects` registers
    // `ObjectResidue::Internal` for the Object sites because a Git child writes
    // objects before publishing their reference; a `docker create` publishes
    // nothing the parent can observe halfway, and the intent record is a
    // stage/rename whose torn half is writer-owned residue the scan skips.
    // `effect_site_inventory.scope` makes every Topology site owe evidence for
    // "every parent-side sub-effect point"; an empty list is that debt being
    // zero, and this is where a variant that grew one would be noticed.
    for site in ContainerSite::ALL {
        assert_eq!(site.sub_effects(), &[], "{}", site.name());
        assert_eq!(site.residue_classes(), &[], "{}", site.name());
        assert_eq!(site.residue_elements(), &[], "{}", site.name());
        assert!(!site.is_read_only(), "{}", site.name());
    }
}

/// **T-CONTAINER (19)** `windows_orphan_window_documented`.
///
/// `decisions.admission_and_leases.permits.os_matrix`:
///
/// > Linux and macOS (cfg(unix)): the cleanup reaper survives coordinator
/// > death, settles the dead coordinator's process groups while holding R28,
/// > and **additionally kills the dead coordinator's labeled containers,
/// > closing the orphan window**; Windows: no reaper; … and **containers are
/// > reclaimed at the next upstroke write-command start (orphan window until
/// > then; documented; a portable watchdog is deferred)**.
///
/// The window is a **value** and not only a sentence, so the two platforms give
/// different answers and the Windows guest — which has no container runtime at
/// all — still asserts something about containers. The intersection here is
/// {platform} x {who closes the window}, and a constant would collapse it.
#[test]
fn windows_orphan_window_documented() {
    let window = super::orphan_window();
    if cfg!(windows) {
        assert_eq!(window, OrphanWindow::UntilNextWriteCommandStart);
        assert!(!window.closed_by_a_reaper());
    } else {
        assert_eq!(window, OrphanWindow::ClosedByTheUnixReaper);
        assert!(window.closed_by_a_reaper());
    }

    // Both answers exist and differ, so the value is a platform axis rather
    // than a constant this platform happens to agree with.
    let answers: BTreeSet<OrphanWindow> = OrphanWindow::ALL.iter().copied().collect();
    assert_eq!(answers.len(), 2);
    assert_eq!(
        OrphanWindow::ALL
            .iter()
            .filter(|w| w.closed_by_a_reaper())
            .count(),
        1,
        "exactly one platform has a reaper; `os_matrix` says Windows has none"
    );

    // And the sentence is in the tree, next to the reclaim path it governs, so
    // "documented" is a fact about this file rather than about the packet.
    let raw = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runner/container.rs"),
    )
    .expect("the funnel");
    // Only the region the documentation lives in, so what follows is a claim
    // about *that* documentation and not about whatever else the file says.
    let region = {
        let start = raw
            .find("// The orphan window")
            .expect("the section header");
        let end = raw.find("impl OrphanWindow {").expect("the impl block");
        assert!(start < end);
        &raw[start..end]
    };
    // Doc-comment markers, block quoting and emphasis removed and whitespace
    // collapsed, because a quoted sentence is wrapped by `rustfmt` at whatever
    // column it lands on and a phrase search over the raw bytes would be
    // asserting the wrap rather than the sentence.
    let source: String = region
        .replace("//!", " ")
        .replace("///", " ")
        .replace(['>', '*', '`'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for phrase in [
        "orphan window",
        "next write-command start",
        "no reaper",
        "a portable watchdog is deferred",
    ] {
        assert!(
            source.contains(phrase),
            "the orphan window's documentation no longer says `{phrase}`"
        );
    }

    // The four phrases above are a **set**, and a set survives having its
    // platform names swapped: documenting a Windows reaper and no Unix reaper
    // leaves every one of them present. So the documentation is read as a
    // *mapping* — each platform marker owns the prose up to the next marker —
    // and the mapping is then checked against the code's own `cfg`.
    const UNIX: &str = "cfg(unix)";
    const WINDOWS: &str = "Windows";
    let mut markers: Vec<(usize, bool)> = source
        .match_indices(UNIX)
        .map(|(at, _)| (at, true))
        .chain(source.match_indices(WINDOWS).map(|(at, _)| (at, false)))
        .collect();
    markers.sort_unstable();
    assert!(
        markers.len() >= 2,
        "the documentation names fewer than two platforms: {source}"
    );
    let mut said: BTreeMap<bool, String> = BTreeMap::new();
    for (index, (at, is_unix)) in markers.iter().enumerate() {
        let end = markers
            .get(index + 1)
            .map_or(source.len(), |(next, _)| *next);
        said.entry(*is_unix)
            .or_default()
            .push_str(&source[*at..end]);
    }
    let unix_said = said.get(&true).map(String::as_str).unwrap_or_default();
    let windows_said = said.get(&false).map(String::as_str).unwrap_or_default();
    assert!(
        !unix_said.is_empty() && !windows_said.is_empty(),
        "both platforms must be named: {source}"
    );

    // What each platform is documented as having, read out of its own prose.
    let unix_has_a_reaper = unix_said.contains("cleanup reaper");
    let windows_has_a_reaper = windows_said.contains("cleanup reaper");
    assert!(
        unix_has_a_reaper != windows_has_a_reaper,
        "exactly one platform has a reaper; `os_matrix` says Windows has none. \
         unix: `{unix_said}` / windows: `{windows_said}`"
    );

    // **The tie.** The platform this test is running on is a `cfg`, and
    // `orphan_window()` answers for that same `cfg`. A documentation block
    // whose platform names are reversed disagrees with it here — on both
    // platforms, in opposite directions — where the phrase set could not tell.
    assert_eq!(
        if cfg!(windows) {
            windows_has_a_reaper
        } else {
            unix_has_a_reaper
        },
        window.closed_by_a_reaper(),
        "the documentation and `orphan_window()`'s own `cfg` disagree about this platform. \
         unix: `{unix_said}` / windows: `{windows_said}`"
    );
    // The named consequences, each against the platform that has them.
    assert!(!unix_said.contains("no reaper"), "{unix_said}");
    assert!(
        windows_said.contains("next write-command start"),
        "{windows_said}"
    );

    // And a second tie, to code that is not `orphan_window` itself: arming the
    // reaper is a **no-op on Windows**, so a scope naming a program that cannot
    // be executed is refused on the platform that has a reaper and accepted on
    // the platform that has nothing to arm. Nothing is installed on either
    // path, so no other test in this process inherits a scope.
    let unarmable = crate::runner::container::census::ReaperContainerScope::new(
        "upstroke-definitely-not-a-real-docker",
        Path::new("/srv/upstroke-orphan-window/private"),
        "01KZTAAAAAAAAAAAAAAAAAAAAA",
    )
    .expect("a well-formed scope");
    assert_eq!(
        crate::agent::proc::set_container_reclaim_scope(Some(&unarmable)).is_err(),
        window.closed_by_a_reaper(),
        "a platform with a reaper checks the program that reaper would exec; a platform \
         without one has nothing to arm and must accept the call as the no-op it is"
    );
}

/// Every one of the eight sites is taken **by value** by a funnel API, and the
/// funnel records both hook phases around the primitive.
///
/// `identity`: "every effectful funnel API takes its group's site by value, and
/// the funnel itself calls hook(Before, site) -> primitive -> hook(After,
/// site), so hooks exist for every site by construction". This is the runtime
/// evidence for that sentence — `effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
/// is the source-level half.
#[test]
fn every_container_site_is_taken_by_value_by_a_funnel_that_hooks_both_phases() {
    let fixture = Fixture::new("all-sites", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();
    let name = fixture.plan.name.clone();

    let written = write_intent(
        &mut hooks,
        ContainerSite::WriteIntent,
        &fixture.root,
        &name,
        &fixture.plan.intent,
    )
    .expect("intent");
    create_container(
        &mut hooks,
        ContainerSite::Create,
        &fixture.runtime,
        &written,
        &fixture.plan.spec,
    )
    .expect("created");
    let view_path = mount_git_view(
        &mut hooks,
        ContainerSite::MountGitView,
        &fixture.view,
        &fixture.plan.view,
    )
    .expect("view");
    start_container(&mut hooks, ContainerSite::Start, &fixture.runtime, &written).expect("started");
    stop_container(
        &mut hooks,
        ContainerSite::Stop,
        &fixture.runtime,
        &name,
        StopMode::Graceful,
    )
    .expect("stopped");
    remove_container(&mut hooks, ContainerSite::Remove, &fixture.runtime, &name).expect("removed");
    unmount_git_view(
        &mut hooks,
        ContainerSite::UnmountGitView,
        &fixture.view,
        &view_path,
    )
    .expect("unmounted");
    remove_intent(
        &mut hooks,
        ContainerSite::RemoveIntent,
        &fixture.root,
        &name,
    )
    .expect("intent removed");

    let sites = fixture.trace.sites();
    // Both phases, once each, for all eight, in the order they were called.
    let expected: Vec<(ContainerSite, TracePhase)> = [
        ContainerSite::WriteIntent,
        ContainerSite::Create,
        ContainerSite::MountGitView,
        ContainerSite::Start,
        ContainerSite::Stop,
        ContainerSite::Remove,
        ContainerSite::UnmountGitView,
        ContainerSite::RemoveIntent,
    ]
    .into_iter()
    .flat_map(|site| [(site, TracePhase::Before), (site, TracePhase::After)])
    .collect();
    assert_eq!(sites, expected);
    let covered: BTreeSet<&str> = sites.iter().map(|(site, _)| site.name()).collect();
    assert_eq!(
        covered.len(),
        ContainerSite::ALL.len(),
        "a site with no funnel is the `PR5D-PROCESS-FUNNEL-TAKES-NO-SITE` shape; \
         the Container group must not become the third"
    );
}

/// A funnel API refuses a site that does not name its operation, and **no
/// primitive effect occurs**.
///
/// The site is a by-value parameter, which is what `identity` asks for; a free
/// parameter can be passed a wrong value, so the guard is what keeps the
/// parameter load-bearing rather than decorative. The grid is all eight sites
/// against all eight APIs: eight accept and fifty-six refuse.
///
/// **`PR6-LANEF-002` is why every cell asserts more than `is_err()`.** Seven of
/// the eight APIs used to count any `Err`, over a fixture holding nothing —
/// and an empty fixture makes the *runtime* supply the error. Deleting only
/// `expect_site(site, Operation::Start)` from [`start_container`] passed the
/// whole suite, because there was no container to start and the test counted
/// the runtime's incidental refusal. A refusal test that passes for the wrong
/// reason is exactly the class this project keeps paying for.
///
/// So each cell is prepared in a state where its primitive **would succeed if
/// it were reached** — a container to start, a container to stop, a view to
/// remove, a record to delete, a free name to create — and each asserts three
/// things:
///
/// 1. the call refused, with [`UpstrokeError::Refused`];
/// 2. **the trace is empty**: no site phase, no runtime operation, no view
///    action and no durability step, which is the whole observable surface this
///    module has and is what "before any effect" means;
/// 3. the API's own state is byte-for-byte what it was.
///
/// And every API is then driven with its **own** site as a positive control, so
/// a cell whose primitive could not have succeeded anyway fails here rather
/// than passing vacuously.
///
/// Second field held constant: the runtime is reachable and holds the recorded
/// image throughout, so no cell can refuse because the runtime was armed.
#[test]
fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {
    let mut accepted = 0;
    let mut refused = 0;
    for (index, own_site) in ContainerSite::ALL.iter().copied().enumerate() {
        let root = scratch(&format!("site-guard-{index}"));
        let trace = ContainerTrace::recording();
        let runtime = FakeRuntime::new(trace.clone());
        runtime.add_image(IMAGE_ID, Some(MANIFEST_DIGEST));
        let view = DisposableDirView::new(trace.clone());
        let ordinal = u32::try_from(index).expect("eight sites");
        let invocation =
            InvocationId::probe(ProbeTarget::Shell, ordinal).expect("a probe identity");
        let name = name_for(RUN_A, INCARNATION_1, &invocation);
        let record = intent_for(RUN_A, INCARNATION_1, &invocation);
        let spec = spec_for(&name, &record, &root, IMAGE_ID);
        let view_path = root.join("views").join(name.as_str());
        let request = GitViewRequest {
            path: view_path.clone(),
            workspace: PathBuf::from("/srv/work/task"),
            head: None,
        };
        let intent_path = name.intent_path(&root);
        let labels = record.labels(&root);
        let seed = |state: Liveness| {
            runtime.seed_container(name.as_str(), labels.clone(), IMAGE_ID, IMAGE_ID, state);
        };
        // `create_container` and `start_container` take an `IntentWritten`, and
        // there is no way to call them without one — that is
        // `expected_failures_refusals[6]`, "container start without an intent
        // is impossible by construction". The proof is minted from a record
        // written **directly** rather than through `write_intent`, so this
        // cell's trace still starts empty and assertion (2) keeps meaning what
        // it says.
        let proof = matches!(own_site, ContainerSite::Create | ContainerSite::Start).then(|| {
            fs::create_dir_all(containers_dir(&root)).expect("the namespace");
            fs::write(
                &intent_path,
                serde_json::to_vec(&record).expect("a serializable record"),
            )
            .expect("the record this container's proof reads");
            crate::runner::container::intent::IntentWritten::certify(&root, &name)
                .expect("the record is on disk, so it certifies")
        });

        // The state in which THIS API's primitive succeeds.
        match own_site {
            // Nothing on disk, so a write would land.
            ContainerSite::WriteIntent => {}
            // The name is free and the image is present, so a create would work.
            ContainerSite::Create => {}
            ContainerSite::Start => seed(Liveness::Exited),
            // No directory, so a materialize would create one.
            ContainerSite::MountGitView => {}
            ContainerSite::Stop => seed(Liveness::Running),
            ContainerSite::Remove => seed(Liveness::Exited),
            ContainerSite::UnmountGitView => {
                fs::create_dir_all(&view_path).expect("a view there is to remove");
            }
            ContainerSite::RemoveIntent => {
                fs::create_dir_all(containers_dir(&root)).expect("the namespace");
                fs::write(&intent_path, b"{}").expect("a record there is to remove");
            }
        }

        let drive = |site: ContainerSite, hooks: &mut RecordingHooks| match own_site {
            ContainerSite::WriteIntent => {
                write_intent(hooks, site, &root, &name, &record).map(|_| ())
            }
            ContainerSite::Create => {
                let proof = proof.as_ref().expect("the Create cell mints one");
                create_container(hooks, site, &runtime, proof, &spec).map(|_| ())
            }
            ContainerSite::Start => {
                let proof = proof.as_ref().expect("the Start cell mints one");
                start_container(hooks, site, &runtime, proof)
            }
            ContainerSite::MountGitView => mount_git_view(hooks, site, &view, &request).map(|_| ()),
            ContainerSite::Stop => stop_container(hooks, site, &runtime, &name, StopMode::Graceful),
            ContainerSite::Remove => remove_container(hooks, site, &runtime, &name),
            ContainerSite::UnmountGitView => unmount_git_view(hooks, site, &view, &view_path),
            ContainerSite::RemoveIntent => remove_intent(hooks, site, &root, &name),
        };

        for wrong in ContainerSite::ALL.iter().copied() {
            if wrong == own_site {
                continue;
            }
            trace.clear();
            let mut hooks = RecordingHooks::new(trace.clone());
            let Err(error) = drive(wrong, &mut hooks) else {
                panic!(
                    "{} accepted `Container.{}`, which names another operation",
                    own_site.name(),
                    wrong.name()
                );
            };
            assert!(
                matches!(error, UpstrokeError::Refused { .. }),
                "{}/{}: {error}",
                own_site.name(),
                wrong.name()
            );
            // (2) Nothing happened at all. The guard runs before `funnel`, so a
            // correct refusal records neither hook phase — and a broken one
            // records the phases AND the primitive's own entry.
            assert_eq!(
                trace.rendered(),
                Vec::<String>::new(),
                "{} under `Container.{}` refused and something still happened",
                own_site.name(),
                wrong.name()
            );
            // (3) And the state the primitive would have changed is untouched.
            let held = runtime.container(name.as_str());
            match own_site {
                ContainerSite::WriteIntent => assert!(!intent_path.exists()),
                ContainerSite::Create => {
                    assert_eq!(runtime.container_names(), Vec::<String>::new())
                }
                ContainerSite::Start => {
                    assert_eq!(held.map(|c| c.state), Some(Liveness::Exited));
                }
                ContainerSite::MountGitView => assert!(!view_path.exists()),
                ContainerSite::Stop => {
                    assert_eq!(held.map(|c| c.state), Some(Liveness::Running));
                }
                ContainerSite::Remove => assert!(held.is_some()),
                ContainerSite::UnmountGitView => assert!(view_path.exists()),
                ContainerSite::RemoveIntent => assert!(intent_path.exists()),
            }
            refused += 1;
        }

        // The positive control: the same API, its own site, and the primitive
        // really does run. Without this a cell whose primitive could not have
        // succeeded would satisfy every assertion above by doing nothing.
        trace.clear();
        let mut hooks = RecordingHooks::new(trace.clone());
        drive(own_site, &mut hooks).expect("the site that names the operation is accepted");
        assert!(
            !trace.rendered().is_empty(),
            "{}: the accepted call recorded nothing, so the refusals above are vacuous",
            own_site.name()
        );
        let held = runtime.container(name.as_str());
        match own_site {
            ContainerSite::WriteIntent => assert!(intent_path.exists()),
            ContainerSite::Create => assert!(held.is_some()),
            ContainerSite::Start => assert_eq!(held.map(|c| c.state), Some(Liveness::Running)),
            ContainerSite::MountGitView => assert!(view_path.is_dir()),
            ContainerSite::Stop => assert_eq!(held.map(|c| c.state), Some(Liveness::Exited)),
            ContainerSite::Remove => assert!(held.is_none()),
            ContainerSite::UnmountGitView => assert!(!view_path.exists()),
            ContainerSite::RemoveIntent => assert!(!intent_path.exists()),
        }
        accepted += 1;
    }
    assert_eq!(
        (accepted, refused),
        (8, 56),
        "eight APIs, each accepting its own site and refusing the other seven"
    );
}

/// A hook armed at a phase makes the funnel return `Err` there, and an `After`
/// error arrives **after** the primitive ran.
#[test]
fn a_hook_armed_at_a_phase_fails_the_funnel_at_that_phase() {
    let fixture = Fixture::new("hook-arm", RUN_A, INCARNATION_1, &shell_probe());
    let name = fixture.plan.name.clone();

    // Before: nothing is written.
    let mut hooks = fixture.hooks();
    hooks.fail_at(
        EffectSiteId::Container(ContainerSite::WriteIntent),
        crate::topology::effects::HookPhase::Before,
    );
    write_intent(
        &mut hooks,
        ContainerSite::WriteIntent,
        &fixture.root,
        &name,
        &fixture.plan.intent,
    )
    .expect_err("armed before");
    assert!(!name.intent_path(&fixture.root).exists());

    // After: the record is on disk and the call still fails.
    let mut hooks = fixture.hooks();
    hooks.fail_at(
        EffectSiteId::Container(ContainerSite::WriteIntent),
        crate::topology::effects::HookPhase::After,
    );
    write_intent(
        &mut hooks,
        ContainerSite::WriteIntent,
        &fixture.root,
        &name,
        &fixture.plan.intent,
    )
    .expect_err("armed after");
    assert!(
        name.intent_path(&fixture.root).exists(),
        "an Err from the After phase is returned after the primitive ran"
    );
}

// ---------------------------------------------------------------------------
// 3. The intent record — six fields, each read back
// ---------------------------------------------------------------------------

/// The six fields `crash_reconstruction` and R26 enumerate, each written and
/// each read back.
///
/// "A field written and never read is invisible to mutation witnessing", so
/// every field is given a value distinct from every other field's and the
/// round trip is asserted field by field. The distinct-value count is the
/// hostility assertion: six fields, six distinct values, so a record that
/// copied one field into another fails.
#[test]
fn the_intent_record_carries_the_six_fields_and_each_is_read_back() {
    let fixture = Fixture::new("six-fields", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();
    let written = write_intent(
        &mut hooks,
        ContainerSite::WriteIntent,
        &fixture.root,
        &fixture.plan.name,
        &fixture.plan.intent,
    )
    .expect("written");
    let path = written.path().to_path_buf();

    let read = read_intent(&path).expect("read back");
    // The proof `write_intent` mints carries the record it read back, so the
    // capability and the file are the same six fields rather than two.
    assert_eq!(written.record(), &read, "the proof and the file disagree");
    assert_eq!(written.name(), &fixture.plan.name);
    assert_eq!(read.run_id, RUN_A);
    assert_eq!(read.run_dir, format!("/srv/public/{RUN_A}"));
    assert_eq!(read.incarnation, INCARNATION_1);
    assert_eq!(read.repo_key, REPO_KEY);
    assert_eq!(read.invocation, "p.shell.o0");
    assert_eq!(read.runner_policy_sha256, POLICY_DIGEST);
    assert_eq!(read, fixture.plan.intent);

    let values: BTreeSet<&str> = [
        read.run_id.as_str(),
        read.run_dir.as_str(),
        read.incarnation.as_str(),
        read.repo_key.as_str(),
        read.invocation.as_str(),
        read.runner_policy_sha256.as_str(),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        values.len(),
        6,
        "six independently meaningful fields, six distinct values"
    );

    // The serialized document has exactly six keys, in the packet's order, and
    // the key names are pinned as literals rather than taken from the struct.
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("bytes")).expect("json");
    let object = document.as_object().expect("an object");
    assert_eq!(object.len(), 6);
    for key in [
        "run_id",
        "run_dir",
        "incarnation",
        "repo_key",
        "invocation",
        "runner_policy_sha256",
    ] {
        assert!(object.contains_key(key), "the record has no `{key}`");
    }
}

/// A record with a seventh field is not this engine's record.
#[test]
fn an_intent_record_with_an_unknown_field_is_refused() {
    let root = scratch("unknown-field");
    let path = root.join("bad.intent");
    fs::write(
        &path,
        br#"{"run_id":"r","run_dir":"d","incarnation":"i","repo_key":"k","invocation":"p.shell.o0","runner_policy_sha256":"s","extra":1}"#,
    )
    .expect("write");
    let error = read_intent(&path).expect_err("an unknown field is refused");
    assert!(matches!(error, UpstrokeError::Refused { .. }));
}

/// The five labels, each carrying its own field.
///
/// `crash_reconstruction`: "labels upstroke.private_root, upstroke.run,
/// upstroke.run_dir, upstroke.incarnation, upstroke.invocation". Written out as
/// literals, and each value asserted against the field it comes from — a label
/// map with five keys and one value repeated would pass a count and fails here.
#[test]
fn the_five_labels_are_the_packets_five_and_each_carries_its_own_field() {
    assert_eq!(
        LABELS,
        [
            "upstroke.private_root",
            "upstroke.run",
            "upstroke.run_dir",
            "upstroke.incarnation",
            "upstroke.invocation",
        ]
    );
    let root = PathBuf::from("/srv/private");
    let record = intent_for(RUN_A, INCARNATION_1, &shell_probe());
    let labels = record.labels(&root);
    assert_eq!(labels.len(), 5);
    assert_eq!(labels[LABEL_PRIVATE_ROOT], "/srv/private");
    assert_eq!(labels[LABEL_RUN], RUN_A);
    assert_eq!(labels[LABEL_RUN_DIR], format!("/srv/public/{RUN_A}"));
    assert_eq!(labels[LABEL_INCARNATION], INCARNATION_1);
    assert_eq!(labels[LABEL_INVOCATION], "p.shell.o0");
    let distinct: BTreeSet<&String> = labels.values().collect();
    assert_eq!(distinct.len(), 5, "five labels, five distinct values");

    // Discovery is by `upstroke.private_root` and the record's own location is
    // inside that root, so the one label with no field of its own is the one
    // the census already knows.
    assert!(
        !record.run_dir.starts_with("/srv/private"),
        "the public run directory and the private root are different values, so \
         a label that took one for the other is visible"
    );
}

// ---------------------------------------------------------------------------
// 4. The name
// ---------------------------------------------------------------------------

/// The name is the packet's template, and the expected value is a literal.
///
/// > the container name is `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`
///
/// The invocation hash is pinned against a value computed **out of band**:
///
/// ```text
/// python3 -c 'import hashlib; print(hashlib.sha256(
///     b"upstroke.container-invocation.v1" + b"\x00" + b"p.shell.o0").hexdigest()[:16])'
/// 1a8e276b273887c0
/// ```
///
/// A digest compared only against the code that produced it proves nothing.
#[test]
fn the_container_name_is_the_packets_template_and_its_hash_is_pinned() {
    assert_eq!(invocation_hash(&shell_probe()), "1a8e276b273887c0");
    assert_eq!(
        invocation_hash(&InvocationId::probe(ProbeTarget::Shell, 1).expect("o1")),
        "2886ac8ba70021d5"
    );
    assert_eq!(invocation_hash(&agent_probe()), "50012b960951553a");

    let name = name_for(RUN_A, INCARNATION_1, &shell_probe());
    assert_eq!(
        name.as_str(),
        "upstroke-0123456789abcdef-01KZRN48A4ZK3AEDST3RJ8HMA4-\
         01KZTAAAAAAAAAAAAAAAAAAAAA-1a8e276b273887c0"
    );
    assert_eq!(
        name.intent_file_name(),
        format!("{}{INTENT_SUFFIX}", name.as_str())
    );
    assert_eq!(
        name.intent_path(Path::new("/srv/private")),
        Path::new("/srv/private")
            .join(CONTAINERS_DIR)
            .join(name.intent_file_name())
    );

    let parts = ContainerName::parse(name.as_str()).expect("parses");
    assert_eq!(parts.repo_key, REPO_KEY);
    assert_eq!(parts.run_id, RUN_A);
    assert_eq!(parts.incarnation, INCARNATION_1);
    assert_eq!(parts.invocation_hash, "1a8e276b273887c0");
}

/// The parse is injective over a hostile component grid.
///
/// Every component is varied independently and the counts are asserted:
/// 2 repo keys x 2 run ids x 2 incarnations x 2 hashes = 16 tuples, 16 distinct
/// names, and 16 distinct parses that each round-trip. A name produced two ways
/// by two different tuples is an ownership record that lies.
#[test]
fn the_name_is_injective_over_every_component_varied_independently() {
    let repo_keys = ["0123456789abcdef", "fedcba9876543210"];
    let runs = [RUN_A, RUN_B];
    let incarnations = [INCARNATION_1, INCARNATION_2];
    let hashes = ["1a8e276b273887c0", "2886ac8ba70021d5"];

    let mut names = BTreeSet::new();
    let mut parsed = BTreeSet::new();
    let mut tuples = BTreeSet::new();
    for repo_key in repo_keys {
        for run in runs {
            for incarnation in incarnations {
                for hash in hashes {
                    let name = ContainerName::from_parts(repo_key, run, incarnation, hash)
                        .expect("a name");
                    let parts = ContainerName::parse(name.as_str()).expect("parses");
                    assert_eq!(parts.repo_key, repo_key);
                    assert_eq!(parts.run_id, run);
                    assert_eq!(parts.incarnation, incarnation);
                    assert_eq!(parts.invocation_hash, hash);
                    names.insert(name.as_str().to_owned());
                    parsed.insert((
                        parts.repo_key,
                        parts.run_id,
                        parts.incarnation,
                        parts.invocation_hash,
                    ));
                    tuples.insert((repo_key, run, incarnation, hash));
                }
            }
        }
    }
    assert_eq!(tuples.len(), 16);
    assert_eq!(names.len(), 16, "two tuples rendered to one name");
    assert_eq!(parsed.len(), 16, "two names parsed to one tuple");

    // The adversarial pair: components chosen so that a template joining them
    // without a refusal on the separator would collide. `a-b` + `c` and `a` +
    // `b-c` render the same string under a naive join; here both are refused.
    assert!(ContainerName::from_parts("a-b", "c", INCARNATION_1, "d").is_err());
    assert!(ContainerName::from_parts("a", "b-c", INCARNATION_1, "d").is_err());
}

/// A component carrying a separator, a `.`, or a path separator is refused.
///
/// The name goes into a **file name** — `<name>.intent` — so a component with a
/// path separator names a different file than the record says, which is the
/// same class `workspace_manager::remove_intent` validates its slot names
/// against.
#[test]
fn a_hostile_name_component_is_refused_and_the_refusal_says_why() {
    let hostile = [
        "with-separator",
        "with.dot",
        "with/slash",
        "with\\backslash",
        "with space",
        "with\u{0}nul",
        "",
    ];
    let mut refusals = BTreeSet::new();
    for bad in hostile {
        for position in 0..4 {
            let mut parts = [REPO_KEY, RUN_A, INCARNATION_1, "1a8e276b273887c0"];
            parts[position] = bad;
            let error = ContainerName::from_parts(parts[0], parts[1], parts[2], parts[3])
                .expect_err("a hostile component is refused");
            assert!(matches!(error, UpstrokeError::Refused { .. }));
            refusals.insert(error.to_string());
        }
    }
    // Seven hostile values in four positions, and the message names the
    // position, so the refusals are not one message repeated.
    assert!(
        refusals.len() >= hostile.len(),
        "the refusals collapse to {} distinct messages for {} hostile values",
        refusals.len(),
        hostile.len()
    );

    // Over-long is refused too, and the boundary is exact.
    let at_limit = "a".repeat(intent::MAX_COMPONENT_LEN);
    let over = "a".repeat(intent::MAX_COMPONENT_LEN + 1);
    assert!(ContainerName::from_parts(&at_limit, RUN_A, INCARNATION_1, "d").is_ok());
    assert!(ContainerName::from_parts(&over, RUN_A, INCARNATION_1, "d").is_err());
}

/// **T-CONTAINER (9)** `probe_name_reuse_across_incarnations_never_collides`.
///
/// `crash_reconstruction`: "the container name is
/// `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`, so
/// **deterministic InvocationIds never collide across incarnations and no
/// earlier ownership evidence is overwritten**". ST-16 (f) is the same claim
/// from the other side: "a probe invocation with the same deterministic
/// InvocationId, whose **new container name and intent path differ**".
///
/// The intersection: {probe kind} x {incarnation}. Both probe targets, both
/// incarnations, one run — so a name that dropped the incarnation collides in
/// **two** places, and one that dropped the invocation collides in two others.
#[test]
fn probe_name_reuse_across_incarnations_never_collides() {
    let root = scratch("probe-reuse");
    let mut names = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for incarnation in [INCARNATION_1, INCARNATION_2] {
        for invocation in [shell_probe(), agent_probe()] {
            // The identity really is the same across incarnations: that is the
            // premise the incarnation component exists for, and asserting it
            // here stops the test passing because the ids happened to differ.
            assert_eq!(
                invocation.render(),
                match invocation.probe_target() {
                    Some(ProbeTarget::Shell) => "p.shell.o0".to_owned(),
                    _ => "p.agent-claude-code.o0".to_owned(),
                }
            );
            let name = name_for(RUN_A, incarnation, &invocation);
            names.insert(name.as_str().to_owned());
            paths.insert(name.intent_path(&root));
        }
    }
    assert_eq!(
        names.len(),
        4,
        "2 incarnations x 2 probe targets: {names:?}"
    );
    assert_eq!(paths.len(), 4, "and four distinct intent paths");

    // And no earlier ownership evidence is overwritten: writing all four leaves
    // four records on disk.
    let mut hooks = RecordingHooks::new(ContainerTrace::recording());
    for incarnation in [INCARNATION_1, INCARNATION_2] {
        for invocation in [shell_probe(), agent_probe()] {
            let name = name_for(RUN_A, incarnation, &invocation);
            write_intent(
                &mut hooks,
                ContainerSite::WriteIntent,
                &root,
                &name,
                &intent_for(RUN_A, incarnation, &invocation),
            )
            .expect("written");
        }
    }
    let found = list_intents(&root).expect("scanned");
    assert_eq!(found.len(), 4);
    let incarnations: BTreeSet<&str> = found
        .iter()
        .map(|entry| entry.record.incarnation.as_str())
        .collect();
    assert_eq!(incarnations.len(), 2);
    let invocations: BTreeSet<&str> = found
        .iter()
        .map(|entry| entry.record.invocation.as_str())
        .collect();
    assert_eq!(invocations.len(), 2);
}

// ---------------------------------------------------------------------------
// 5. The orderings
// ---------------------------------------------------------------------------

/// **T-CONTAINER (1)** `container_intent_written_before_run`.
///
/// `side_effect_vs_event_ordering`: "**intent synced before docker create**".
/// Both halves: the record's *sync* (not merely its write) precedes the create,
/// and the create precedes the start.
///
/// Second field held constant: the runtime is reachable and reports the
/// recorded id, so nothing here can pass because the launch failed early.
#[test]
fn container_intent_written_before_run() {
    let fixture = Fixture::new("intent-before-run", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();
    let launched =
        launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");

    let trace = &fixture.trace;
    let rendered = trace.rendered();
    let file = fixture.plan.name.intent_file_name();

    let synced = at(trace, &format!("durable:synced:{file}.tmp"));
    let renamed = at(trace, &format!("durable:renamed:{file}"));
    let dir_synced = trace
        .position_starting("durable:dir-synced:")
        .unwrap_or_else(|| panic!("no directory barrier in {rendered:#?}"));
    let created = at(trace, &format!("rt:create:{}", fixture.plan.name));
    let started = at(trace, &format!("rt:start:{}", fixture.plan.name));

    assert!(
        synced < renamed && renamed < dir_synced && dir_synced < created,
        "the intent must be SYNCED before docker create, not merely written: {rendered:#?}"
    );
    assert!(
        created < started,
        "and created before started: {rendered:#?}"
    );

    // The record really is on disk, with the run that owns it.
    assert!(launched.intent_path.exists());
    assert_eq!(
        read_intent(&launched.intent_path).expect("read").run_id,
        RUN_A
    );
}

/// **T-CONTAINER (2)** `container_created_from_recorded_image_id_and_verified`.
///
/// INV-23: "every container of every epoch is created from the **recorded image
/// id** and its reported image id is verified equal to the record **before it
/// starts**".
///
/// The intersection: {image id recorded} x {reference moved}. The reference is
/// moved to another image *before* the launch, and the container is still
/// created from the recorded id — which is the sentence "so a moved reference
/// cannot change what executes" and is not provable by a fixture whose
/// reference never moved.
#[test]
fn container_created_from_recorded_image_id_and_verified() {
    let fixture = Fixture::new("created-from-id", RUN_A, INCARNATION_1, &shell_probe());
    // The reference now names another image. The record still names the id.
    fixture.runtime.move_tag(IMAGE_REFERENCE, OTHER_IMAGE_ID);
    assert_eq!(
        fixture
            .runtime
            .image_by_reference(IMAGE_REFERENCE)
            .expect("reachable")
            .expect("present")
            .id,
        OTHER_IMAGE_ID
    );

    let mut hooks = fixture.hooks();
    let launched =
        launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");

    assert_eq!(launched.reported_image_id, IMAGE_ID);
    let held = fixture
        .runtime
        .container(fixture.plan.name.as_str())
        .expect("held");
    assert_eq!(
        held.requested_image_id, IMAGE_ID,
        "created from the recorded id, not from what the reference now names"
    );
    assert_ne!(held.requested_image_id, OTHER_IMAGE_ID);
    // Verified *before* start: the verification is between create and start in
    // the sequence, and the start happened, so it passed there.
    let created = at(&fixture.trace, &format!("rt:create:{}", fixture.plan.name));
    let started = at(&fixture.trace, &format!("rt:start:{}", fixture.plan.name));
    assert!(created < started);
}

/// **T-CONTAINER (3)** `substituted_image_id_refused_before_start`.
///
/// INV-23: "a mismatch refuses during pre-flight or rebuild". The refusal is
/// **before start**, and the assertion is that `Container.Start` is absent from
/// the sequence — not that an error was returned, which a refusal after the
/// start would also produce.
///
/// The intersection: {reported id} x {start reached}. R26 balances afterwards,
/// because a refusal is a cancel and a cancel releases.
#[test]
fn substituted_image_id_refused_before_start() {
    let fixture = Fixture::new("substituted", RUN_A, INCARNATION_1, &shell_probe());
    fixture
        .runtime
        .substitute_reported_image_id(fixture.plan.name.as_str(), OTHER_IMAGE_ID);

    let mut hooks = fixture.hooks();
    let error = launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan)
        .expect_err("a substituted image id is refused");
    let message = error.to_string();
    assert!(message.contains(OTHER_IMAGE_ID), "{message}");
    assert!(message.contains(IMAGE_ID), "{message}");
    assert!(message.contains("before start"), "{message}");

    let rendered = fixture.trace.rendered();
    assert!(
        fixture.trace.position_starting("rt:start:").is_none(),
        "the container was started despite the mismatch: {rendered:#?}"
    );
    assert!(
        !fixture
            .trace
            .sites()
            .iter()
            .any(|(site, _)| *site == ContainerSite::Start),
        "the Start site executed despite the mismatch: {rendered:#?}"
    );
    // The view IS mounted — it precedes `Create`, because it is a bind-mount
    // source of it (`PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE`) — and the cancel
    // therefore has an R19 residue to prune. This assertion used to read "no
    // view is mounted for a container that will not start", which the corrected
    // order makes false; the claim it was standing for is R19 balancing, and
    // that is asserted here directly and more strongly.
    let mounted = at(&fixture.trace, "site:MountGitView:after");
    let pruned = at(&fixture.trace, "site:UnmountGitView:after");
    assert!(
        mounted < pruned,
        "the view is mounted and then pruned by the cancel: {rendered:#?}"
    );

    // R19 and R26 both balance: the container it created is released, the view
    // is gone and the intent is gone, so no census finds residue of a refusal.
    assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
    assert!(
        !fixture.plan.view.path.exists(),
        "the refused launch left its R19 view behind"
    );
    assert!(!fixture.plan.name.intent_path(&fixture.root).exists());
    assert_eq!(list_intents(&fixture.root).expect("scan").len(), 0);
}

/// "view mounted before start" — and before **create**, because it is a
/// bind-mount source of that call.
///
/// The contract clause is satisfied by two orders and only one of them runs:
/// Docker requires a bind source to exist at `docker create`, so
/// `WriteIntent -> Create -> MountGitView -> Start` refuses with
/// `invalid mount config for type "bind": bind source path does not exist`.
/// `PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE` — measured against docker 29.7.2
/// by lane A, invisible to the fake (whose `create` does not look at a mount
/// source) and invisible to this file's own gated test until it started
/// carrying the view as a real mount.
///
/// Second field held constant: the runtime reports the recorded id and the
/// launch succeeds, so nothing here passes because a step failed early. The
/// intersection is {which pair of steps} × {the sequence between them}, and the
/// directory's existence at the moment of the create is asserted as well as the
/// order, because "the site ran earlier" and "the directory was there" are two
/// claims.
#[test]
fn the_git_view_is_mounted_before_create_and_before_start() {
    let fixture = Fixture::new("view-before-create", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();
    let launched =
        launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");

    let rendered = fixture.trace.rendered();
    let mounted = at(&fixture.trace, "site:MountGitView:after");
    let created = at(&fixture.trace, &format!("rt:create:{}", fixture.plan.name));
    let started = at(&fixture.trace, &format!("rt:start:{}", fixture.plan.name));
    assert!(
        mounted < created,
        "the view is a bind-mount source of the create and must exist when the \
         container is created: {rendered:#?}"
    );
    assert!(
        created < started,
        "and the create still precedes the start: {rendered:#?}"
    );
    assert!(
        mounted < started,
        "the contract's own clause, stated on its own: {rendered:#?}"
    );
    assert!(launched.view_path.is_dir(), "R19's directory exists");
    assert_eq!(launched.view_path, fixture.plan.view.path);

    // The intent is still first, so moving the mount up did not move it past
    // "intent synced before docker create".
    let dir_synced = fixture
        .trace
        .position_starting("durable:dir-synced:")
        .unwrap_or_else(|| panic!("no directory barrier in {rendered:#?}"));
    assert!(
        dir_synced < mounted,
        "the intent is synced before anything else happens: {rendered:#?}"
    );
}

/// "stop/rm, view removal, intent removal after completion" — the four sites in
/// the contract's own order.
#[test]
fn release_stops_removes_unmounts_and_removes_the_intent_in_that_order() {
    let fixture = Fixture::new("release-order", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();
    let launched =
        launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");
    fixture.trace.clear();

    release(
        &mut hooks,
        &fixture.runtime,
        &fixture.view,
        &fixture.root,
        &launched,
    )
    .expect("released");

    assert_eq!(
        fixture
            .trace
            .sites()
            .into_iter()
            .filter(|(_, phase)| *phase == TracePhase::After)
            .map(|(site, _)| site)
            .collect::<Vec<_>>(),
        vec![
            ContainerSite::Stop,
            ContainerSite::Remove,
            ContainerSite::UnmountGitView,
            ContainerSite::RemoveIntent,
        ]
    );
    // R19 and R26 both balance.
    assert!(!launched.view_path.exists(), "the view is pruned");
    assert!(!launched.intent_path.exists(), "the intent is removed");
    assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
}

/// Reclaim, in the packet's order:
///
/// > reclaim = docker kill -> wait until observed exited/removed -> docker rm
/// > -> remove Git view -> remove intent
///
/// Five steps, and the **observation between the kill and the rm** is the one a
/// set-membership assertion would lose.
#[test]
fn reclaim_kills_observes_removes_the_view_and_then_the_intent() {
    let fixture = Fixture::new("reclaim-order", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();
    let launched =
        launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");
    assert_eq!(
        fixture
            .runtime
            .container(fixture.plan.name.as_str())
            .map(|c| c.state),
        Some(Liveness::Running),
        "the fixture really is reclaiming a RUNNING container"
    );
    fixture.trace.clear();

    reclaim(
        &mut hooks,
        &fixture.runtime,
        &fixture.view,
        &fixture.root,
        &launched.name,
        Some(&launched.view_path),
    )
    .expect("reclaimed");

    let rendered = fixture.trace.rendered();
    let killed = at(&fixture.trace, &format!("rt:stop:{}", launched.name));
    let observed = at(&fixture.trace, &format!("rt:observe:{}", launched.name));
    let removed = at(&fixture.trace, &format!("rt:remove:{}", launched.name));
    let view_gone = at(&fixture.trace, "site:UnmountGitView:after");
    let intent_gone = at(&fixture.trace, "site:RemoveIntent:after");
    assert!(
        killed < observed && observed < removed && removed < view_gone && view_gone < intent_gone,
        "reclaim's five steps are out of order: {rendered:#?}"
    );
    assert!(!launched.view_path.exists());
    assert!(!launched.intent_path.exists());
    assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
}

/// Reclaim is idempotent and tolerant of already-gone, so two reclaimers
/// converge.
///
/// The intersection: {intent present} x {container present}. All four cells are
/// driven, and each must converge on the same terminal state.
#[test]
fn reclaim_converges_from_every_combination_of_intent_and_container() {
    for (has_intent, has_container) in [(true, true), (true, false), (false, true), (false, false)]
    {
        let fixture = Fixture::new(
            &format!("converge-{has_intent}-{has_container}"),
            RUN_A,
            INCARNATION_1,
            &shell_probe(),
        );
        let mut hooks = fixture.hooks();
        let name = fixture.plan.name.clone();
        let view_path = fixture.plan.view.path.clone();
        if has_intent {
            write_intent(
                &mut hooks,
                ContainerSite::WriteIntent,
                &fixture.root,
                &name,
                &fixture.plan.intent,
            )
            .expect("intent");
        }
        if has_container {
            fixture.runtime.seed_container(
                name.as_str(),
                fixture.plan.intent.labels(&fixture.root),
                IMAGE_ID,
                IMAGE_ID,
                Liveness::Running,
            );
            fs::create_dir_all(&view_path).expect("view");
        }

        for round in 0..2 {
            reclaim(
                &mut hooks,
                &fixture.runtime,
                &fixture.view,
                &fixture.root,
                &name,
                Some(&view_path),
            )
            .unwrap_or_else(|error| {
                panic!("round {round} of ({has_intent}, {has_container}) refused: {error}")
            });
            assert!(!name.intent_path(&fixture.root).exists());
            assert!(!view_path.exists());
            assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
        }
    }
}

/// The intent's durability barriers are **entered**, not merely traced.
///
/// `PR6-LANEF-001`. `crash_reconstruction` requires every container invocation
/// to write a **synced** global intent, and [`super::write_synced`] records
/// `DurableStep::Synced` / `DirSynced` in the trace beside each barrier. That
/// record is written by the same function that performs the barrier, so it
/// certifies itself: **deleting `util::fsync_file` and `util::fsync_dir` while
/// leaving the two trace calls in place passed the entire suite** — every
/// ordering assertion in this file reads the record, and the record was still
/// there. Write and rename the intent, create the container, lose power before
/// either the file or its directory reaches stable storage, and Docker keeps a
/// container whose ownership record crash reconstruction cannot find.
///
/// So this reads the **syscall** instead. [`crate::util::barriers_on_this_thread`]
/// counts entries into the two barrier functions per thread and per half; a
/// funnel performs its barriers on the thread that called it, so the delta is
/// exact rather than the lower bound a process-wide counter can support while
/// the suite is threaded. The two halves are counted separately because
/// `fsync_file` and `fsync_dir` are two independently droppable predicates.
///
/// The two axes: {which barrier} × {which call}. The trace is read only to show
/// the two agree — never as the evidence that a barrier happened.
#[test]
fn the_intents_durability_barriers_are_entered_and_not_merely_traced() {
    let fixture = Fixture::new("barriers", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();

    let before = crate::util::barriers_on_this_thread();
    let written = write_intent(
        &mut hooks,
        ContainerSite::WriteIntent,
        &fixture.root,
        &fixture.plan.name,
        &fixture.plan.intent,
    )
    .expect("written");
    let path = written.path().to_path_buf();
    let after = crate::util::barriers_on_this_thread();

    assert_eq!(
        after.file - before.file,
        1,
        "`write_intent` must enter the FILE half of the durability barrier; the \
         staged record's own bytes have to be on stable storage before the rename"
    );
    assert_eq!(
        after.directory - before.directory,
        1,
        "`write_intent` must enter the DIRECTORY half; a rename is not durable \
         because the renamed file was synced — the durable thing is the entry"
    );

    // The other axis, and it is a different claim: the trace says the same
    // thing. If these two ever disagree the trace is the one that is wrong.
    let file = fixture.plan.name.intent_file_name();
    let rendered = fixture.trace.rendered();
    assert_eq!(
        rendered
            .iter()
            .filter(|entry| entry.starts_with("durable:synced:"))
            .count(),
        1,
        "{rendered:#?}"
    );
    assert!(
        fixture
            .trace
            .position(&format!("durable:synced:{file}.tmp"))
            < fixture.trace.position(&format!("durable:renamed:{file}"))
    );
    assert!(path.exists());

    // Second cell: the whole launch. Exactly one of each, still — the view, the
    // create and the start perform no barriers — so a barrier that quietly
    // appeared or disappeared elsewhere in the sequence is visible here too.
    let fixture = Fixture::new("barriers-launch", RUN_A, INCARNATION_1, &agent_probe());
    let mut hooks = fixture.hooks();
    let before = crate::util::barriers_on_this_thread();
    launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");
    let after = crate::util::barriers_on_this_thread();
    assert_eq!(
        (after.file - before.file, after.directory - before.directory),
        (1, 1),
        "one file barrier and one directory barrier for one launch"
    );
}

/// A cancel whose own cleanup fails still refuses with the **integrity** error,
/// and still attempts every remaining step.
///
/// `PR6-LANEF-006`. [`launch`]'s image-id refusal used to `?`-chain its cleanup,
/// so a failing `Container.Stop` returned the *stop* error before `rm`, the view
/// removal or the intent removal ran: one failure, three residues, and the fact
/// that the runtime had created a container from a substituted image went
/// unsaid. "Is that true at every point it can fail?" — it was not.
///
/// The grid is {which of the four cancel steps fails} × {what survives}, and
/// each of the four cells has a **distinct** observable, so a fix that merely
/// stopped returning the first error would still fail three of them:
///
/// | armed | container | view | intent |
/// |---|---|---|---|
/// | `Stop` | removed | pruned | removed |
/// | `Remove` | **left** | pruned | removed |
/// | `UnmountGitView` | removed | **left** | **left, deliberately** |
/// | `RemoveIntent` | removed | pruned | **left** |
///
/// ## The third row changed in repair round R3b, and it is the finding
///
/// `PR6-ACCT-005`. It read "`UnmountGitView` → removed / left / **removed**",
/// and that is the state a startup census cannot recover from: discovery is
/// `<R>/containers` plus `docker ps` by label, and the view path is derived
/// only *after* a candidate is found — `<R>/views` is never enumerated. So a
/// cancel that failed to prune the view and then removed the intent anyway had
/// deleted the only thing that could ever find the directory again. The test
/// pinned that state as correct.
///
/// The intent is the R19 view's **recovery anchor** and now outlives what it
/// anchors: the retained record is itself reported in the residue, so the
/// ledgers are still said not to balance, and
/// `census::tests::an_unpruned_view_is_reclaimed_because_its_intent_survived`
/// drives the census that closes it.
///
/// Second field held constant: the substitution, the image ids and the plan are
/// identical in all four cells, so what varies is only which step was armed.
#[test]
fn a_cancel_whose_cleanup_fails_still_refuses_with_the_integrity_error() {
    use crate::topology::effects::HookPhase;

    let cells = [
        ContainerSite::Stop,
        ContainerSite::Remove,
        ContainerSite::UnmountGitView,
        ContainerSite::RemoveIntent,
    ];
    let mut messages = BTreeSet::new();
    for armed in cells {
        let fixture = Fixture::new(
            &format!("cancel-{}", armed.name()),
            RUN_A,
            INCARNATION_1,
            &shell_probe(),
        );
        fixture
            .runtime
            .substitute_reported_image_id(fixture.plan.name.as_str(), OTHER_IMAGE_ID);
        let mut hooks = fixture.hooks();
        hooks.fail_at(EffectSiteId::Container(armed), HookPhase::Before);

        let error = launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan)
            .expect_err("a substituted image id is refused whatever the cleanup does");
        let message = error.to_string();

        // The integrity refusal survives the cleanup failure. This is the whole
        // finding: the operator needs to know the runtime executed something
        // other than the record, not that `docker stop` said no.
        assert!(message.contains(OTHER_IMAGE_ID), "{armed:?}: {message}");
        assert!(message.contains(IMAGE_ID), "{armed:?}: {message}");
        assert!(message.contains("before start"), "{armed:?}: {message}");
        assert!(message.contains("INV-23"), "{armed:?}: {message}");
        // And the residue is reported rather than swallowed: fail-closed means
        // the refusal names what it could not release.
        assert!(
            message.contains("could not release everything"),
            "{armed:?}: {message}"
        );
        assert!(
            message.contains(armed.name()) || message.contains(step_phrase(armed)),
            "{armed:?}: the refusal does not say which step failed: {message}"
        );
        messages.insert(message.clone());

        // Never started, whatever else happened.
        assert!(
            fixture.trace.position_starting("rt:start:").is_none(),
            "{armed:?}: the container was started despite the mismatch"
        );

        // The three steps that were NOT armed all ran.
        let container_left = !fixture.runtime.container_names().is_empty();
        let view_left = fixture.plan.view.path.exists();
        let intent_left = fixture.plan.name.intent_path(&fixture.root).exists();
        let expected = match armed {
            ContainerSite::Stop => (false, false, false),
            ContainerSite::Remove => (true, false, false),
            // The anchor rule: an unpruned view keeps its record.
            ContainerSite::UnmountGitView => (false, true, true),
            ContainerSite::RemoveIntent => (false, false, true),
            _ => unreachable!("only the four cancel steps are armed"),
        };
        assert_eq!(
            (container_left, view_left, intent_left),
            expected,
            "{armed:?}: exactly the armed step's residue should remain, and every \
             other step should have run anyway"
        );
        if armed == ContainerSite::UnmountGitView {
            // Retained on purpose, and said so — an operator reading "the view
            // could not be pruned" and finding the record gone would have no
            // way to know the directory exists.
            assert!(
                message.contains("deliberately retained"),
                "the refusal does not say the record was kept as the view's recovery anchor: \
                 {message}"
            );
            assert!(
                message.contains("R19"),
                "the refusal does not name the row it is protecting: {message}"
            );
        }
    }
    assert_eq!(
        messages.len(),
        4,
        "four armed steps, four distinct refusals — a message that did not name \
         the step would collapse these"
    );
}

/// The phrase [`super::cancel_created`] uses for each step, so the assertion
/// above reads the message rather than the enum that wrote it.
fn step_phrase(site: ContainerSite) -> &'static str {
    match site {
        ContainerSite::Stop => "could not be stopped",
        ContainerSite::Remove => "could not be removed",
        ContainerSite::UnmountGitView => "R19 Git view could not be pruned",
        ContainerSite::RemoveIntent => "R26 intent record could not be removed",
        _ => "unreachable",
    }
}

// ---------------------------------------------------------------------------
// 5b. Two reclaimers that actually race
// ---------------------------------------------------------------------------

/// What `docker` 29.7.2 writes to stderr, measured on the build box.
///
/// ```text
/// $ docker kill <exited>
/// Error response from daemon: cannot kill container: f1probe-a: container
/// 0079320fdf5654fbf3aa45a154e4d49328c1cc1de3b1af4a6cc24540519ecede is not running
/// $ docker kill <absent>
/// Error response from daemon: cannot kill container: f1probe-nope: No such container: f1probe-nope
/// $ docker stop <absent>
/// Error response from daemon: No such container: f1probe-nope
/// $ docker stop <exited>
/// f1probe-a                                            (exit 0)
/// ```
///
/// Transcribed, not invented — and
/// `real_docker_kill_on_an_already_exited_container_is_tolerated` asks the live
/// daemon the same question, so the table cannot drift into being its own
/// oracle.
const DAEMON_ALREADY_STOPPED: &str = "Error response from daemon: cannot kill container: \
     upstroke-c: container 0079320fdf5654fbf3aa45a154e4d49328c1cc1de3b1af4a6cc24540519ecede \
     is not running";
const DAEMON_ABSENT_ON_KILL: &str =
    "Error response from daemon: cannot kill container: upstroke-c: No such container: upstroke-c";
const DAEMON_ABSENT_ON_STOP: &str = "Error response from daemon: No such container: upstroke-c";

/// A `docker stop` answer meaning "already settled" is tolerated; a real
/// failure is not.
///
/// `PR6-LANEF-003`. `DockerCli::stop`'s tolerance is load-bearing — it is what
/// makes a reclaimer that arrives second converge instead of aborting — and
/// **removing it passed every test**, because every fixture serialized the
/// reclaimers and a serialized second reclaimer never sees the answer.
/// [`super::settle_stop`] is a free function taking the raw outcome for exactly
/// this reason: the branch is reachable without a daemon.
///
/// The intersection: {what the daemon said} × {is it tolerable}. Three
/// tolerable answers and three that are not, counted rather than described —
/// and the third intolerable one is `Unreachable` carrying tolerable *text*,
/// because a runtime that could not be reached did not tell us anything about
/// the container.
#[test]
fn a_stop_answer_meaning_already_settled_is_tolerated_and_a_real_failure_is_not() {
    let failed = |detail: &str| {
        Err(RuntimeError::Failed {
            operation: RuntimeOp::Stop,
            detail: detail.to_owned(),
        })
    };

    let tolerated = [
        DAEMON_ALREADY_STOPPED,
        DAEMON_ABSENT_ON_KILL,
        DAEMON_ABSENT_ON_STOP,
    ];
    for detail in tolerated {
        assert_eq!(
            super::settle_stop(failed(detail)),
            Ok(()),
            "a reclaimer that arrives second must converge on `{detail}`"
        );
    }
    assert_eq!(
        tolerated.iter().collect::<BTreeSet<_>>().len(),
        3,
        "three distinct daemon answers, not one repeated"
    );

    // Real failures stay failures. `--force` removal and a kill the daemon could
    // not deliver are things a reclaimer must NOT report as convergence.
    for detail in [
        "Error response from daemon: cannot kill container: upstroke-c: tried to kill \
         container, but did not receive an exit event",
        "Error response from daemon: cannot stop container: upstroke-c: permission denied",
    ] {
        let error = super::settle_stop(failed(detail)).expect_err("a real failure is a failure");
        assert!(!error.is_unreachable(), "{error}");
        assert_eq!(error.operation(), RuntimeOp::Stop);
    }

    // And unreachable is a different answer even when its text would be
    // tolerable: `crash_reconstruction` refuses a write command when the runtime
    // "cannot be reached", and swallowing that here would turn a refusal into a
    // convergence.
    let unreachable = super::settle_stop(Err(RuntimeError::Unreachable {
        operation: RuntimeOp::Stop,
        detail: DAEMON_ALREADY_STOPPED.to_owned(),
    }))
    .expect_err("unreachable is never `already settled`");
    assert!(unreachable.is_unreachable(), "{unreachable}");

    // The control: a stop that simply worked.
    assert_eq!(super::settle_stop(Ok("upstroke-c\n".to_owned())), Ok(()));
}

/// A runtime whose `stop` answers the way the daemon does, settled through the
/// production tolerance.
///
/// The point is that the **raw** answer is the daemon's and the settling is
/// [`super::settle_stop`], the production function — so a fixture built on this
/// is exercising the tolerance rather than a test-local copy of it. Every raw
/// answer is recorded, so a test can assert the already-stopped branch actually
/// fired instead of hoping it did.
struct DockerLikeStop<'a> {
    inner: &'a FakeRuntime,
    raw: std::sync::Mutex<Vec<String>>,
}

impl<'a> DockerLikeStop<'a> {
    fn new(inner: &'a FakeRuntime) -> Self {
        Self {
            inner,
            raw: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn raw_answers(&self) -> Vec<String> {
        self.raw
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl ContainerRuntime for DockerLikeStop<'_> {
    fn probe(&self) -> Result<(), RuntimeError> {
        self.inner.probe()
    }
    fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        self.inner.image_by_reference(reference)
    }
    fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        self.inner.image_by_id(id)
    }
    fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
        self.inner.volume_present(name)
    }
    fn containers_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<super::runtime::DiscoveredContainer>, RuntimeError> {
        self.inner.containers_with_label(key, value)
    }
    fn observe(&self, name: &str) -> Result<Liveness, RuntimeError> {
        self.inner.observe(name)
    }
    fn collect(&self, name: &str) -> Result<ContainerExecution, RuntimeError> {
        self.inner.collect(name)
    }
    fn create(&self, spec: &CreateSpec) -> Result<super::runtime::CreatedContainer, RuntimeError> {
        self.inner.create(spec)
    }
    fn start(&self, name: &str) -> Result<(), RuntimeError> {
        self.inner.start(name)
    }

    fn stop(&self, name: &str, mode: StopMode) -> Result<(), RuntimeError> {
        // The daemon's own three answers, chosen by the state the container is
        // actually in — which is what makes a second reclaimer see the
        // already-stopped one rather than a flag a test had to set.
        let outcome = match self.inner.observe(name)? {
            Liveness::Running => {
                self.inner.stop(name, mode)?;
                Ok(format!("{name}\n"))
            }
            Liveness::Exited => Err(RuntimeError::Failed {
                operation: RuntimeOp::Stop,
                detail: DAEMON_ALREADY_STOPPED.to_owned(),
            }),
            Liveness::Gone => Err(RuntimeError::Failed {
                operation: RuntimeOp::Stop,
                detail: DAEMON_ABSENT_ON_KILL.to_owned(),
            }),
        };
        self.raw
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(match &outcome {
                Ok(text) => format!("ok:{}", text.trim()),
                Err(error) => format!("err:{error}"),
            });
        super::settle_stop(outcome)
    }

    fn remove(&self, name: &str) -> Result<(), RuntimeError> {
        self.inner.remove(name)
    }
}

/// A reclaimer that arrives after another has already killed the container
/// **converges**, rather than aborting before observe / rm / view / intent.
///
/// `PR6-LANEF-003`'s scenario, made deterministic: reclaimer A kills the
/// container and then crashes; B reaches `docker kill` after the state has
/// become `Exited` and gets "is not running". Without the tolerance B returns an
/// error and R19/R26 both keep residue — which is the opposite of "every step
/// idempotent and tolerant of already-gone so **two concurrent reclaimers
/// converge**".
///
/// Second field held constant: the container, the intent and the view are all
/// present when B starts, so B has real work to do at every one of its five
/// steps and cannot pass by finding nothing.
#[test]
fn a_reclaimer_arriving_after_another_killed_the_container_converges() {
    let fixture = Fixture::new("second-reclaimer", RUN_A, INCARNATION_1, &shell_probe());
    let name = fixture.plan.name.clone();
    let view_path = fixture.plan.view.path.clone();
    let mut hooks = fixture.hooks();
    let launched =
        launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");
    assert!(view_path.is_dir() && launched.intent_path.exists());

    let docker_like = DockerLikeStop::new(&fixture.runtime);

    // Reclaimer A: kills it, and gets no further.
    stop_container(
        &mut hooks,
        ContainerSite::Stop,
        &docker_like,
        &name,
        StopMode::Kill,
    )
    .expect("A killed it");
    assert_eq!(
        fixture.runtime.container(name.as_str()).map(|c| c.state),
        Some(Liveness::Exited),
        "A really did settle the container before crashing"
    );

    // Reclaimer B: the whole sequence, over a container that is already stopped.
    reclaim(
        &mut hooks,
        &docker_like,
        &fixture.view,
        &fixture.root,
        &name,
        Some(&view_path),
    )
    .expect("B converges on a container A already killed");

    // The already-stopped branch actually fired — without this the test could
    // pass having never reached the tolerance at all.
    let answers = docker_like.raw_answers();
    assert!(
        answers
            .iter()
            .any(|answer| answer.contains("is not running")),
        "the daemon's already-stopped answer was never produced: {answers:#?}"
    );
    assert_eq!(
        answers.len(),
        2,
        "one kill from A, one from B: {answers:#?}"
    );

    // And B finished the job: nothing of R19 or R26 is left.
    assert!(!view_path.exists(), "B stopped before pruning the view");
    assert!(
        !launched.intent_path.exists(),
        "B stopped before removing the intent"
    );
    assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
}

/// **T-CONTAINER** convergence, with the two reclaimers genuinely concurrent.
///
/// The reviewer's refutation of lane F's claim was that two reclaimers that
/// actually **race** were not constructible in any fixture it built — running
/// `reclaim` twice proves idempotence, which is a different property. This is
/// the race: two threads, released together by a [`std::sync::Barrier`], both
/// inside `reclaim` on one container, one runtime, one intent and one view.
///
/// Every interleaving must converge, and the assertion is on the terminal state
/// rather than on who won — which is the only thing a race is allowed to
/// assert.
#[test]
fn two_reclaimers_racing_one_container_converge() {
    let fixture = Fixture::new("racing", RUN_A, INCARNATION_1, &shell_probe());
    let name = fixture.plan.name.clone();
    let view_path = fixture.plan.view.path.clone();
    let mut hooks = fixture.hooks();
    let launched =
        launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");

    let docker_like = DockerLikeStop::new(&fixture.runtime);
    let gate = std::sync::Barrier::new(2);
    let view = &fixture.view;
    let root = &fixture.root;

    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..2 {
            handles.push(scope.spawn(|| {
                let mut hooks = NoHooks;
                gate.wait();
                reclaim(
                    &mut hooks,
                    &docker_like,
                    view,
                    root,
                    &name,
                    Some(&view_path),
                )
            }));
        }
        for (index, handle) in handles.into_iter().enumerate() {
            handle
                .join()
                .expect("the reclaimer did not panic")
                .unwrap_or_else(|error| panic!("reclaimer {index} did not converge: {error}"));
        }
    });

    assert_eq!(docker_like.raw_answers().len(), 2, "both reclaimers ran");
    assert!(!view_path.exists());
    assert!(!launched.intent_path.exists());
    assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
}

/// A container that cannot be observed terminated refuses.
///
/// `refusal_condition`: "a dead owner's or dead incarnation's labeled container
/// that cannot be observed terminated **blocks admission**". The fake's stop is
/// armed failing so the container stays `Running` and the observation never
/// converges — the second field held constant is that the runtime is
/// *reachable* throughout, so this is not the unreachable refusal wearing
/// another name.
#[test]
fn a_container_that_cannot_be_observed_terminated_refuses() {
    let fixture = Fixture::new("unobservable", RUN_A, INCARNATION_1, &shell_probe());
    let name = fixture.plan.name.clone();
    fixture.runtime.seed_container(
        name.as_str(),
        fixture.plan.intent.labels(&fixture.root),
        IMAGE_ID,
        IMAGE_ID,
        Liveness::Running,
    );
    // Stop succeeds and the container stays running: a kill that was delivered
    // to a process the kernel has not reaped.
    let error = observe_terminated(&NeverTerminates(&fixture.runtime), &name)
        .expect_err("it cannot be observed terminated");
    let message = error.to_string();
    assert!(
        message.contains("cannot be observed terminated"),
        "{message}"
    );
    assert!(message.contains("blocks admission"), "{message}");
    assert_eq!(
        fixture
            .trace
            .ops()
            .iter()
            .filter(|op| **op == RuntimeOp::Observe)
            .count(),
        TERMINATION_OBSERVATIONS,
        "the bound is the bound, not one observation"
    );
}

/// Unreachable and failed are **different answers**, and the refusal split
/// rests on the difference.
///
/// `crash_reconstruction` refuses a write command when "any intent exists and
/// the runtime **cannot be reached**"; an operation that reached the runtime
/// and failed is a different thing, and a seam that reported one error kind
/// would make lane C's refusal unwritable. The intersection here is {operation}
/// x {reachable? failed? fine?} — three states over one operation, not two axes
/// tested apart.
#[test]
fn a_failed_operation_and_an_unreachable_one_are_different_answers() {
    let runtime = FakeRuntime::new(ContainerTrace::recording());
    runtime.add_image(IMAGE_ID, None);

    // Fine.
    assert!(runtime.image_by_id(IMAGE_ID).expect("reachable").is_some());

    // Reached and failed.
    runtime.set_failing(RuntimeOp::InspectImageById);
    let failed = runtime.image_by_id(IMAGE_ID).expect_err("armed failing");
    assert!(!failed.is_unreachable(), "{failed}");
    assert_eq!(failed.operation(), RuntimeOp::InspectImageById);
    assert!(failed.to_string().contains("refused"), "{failed}");

    // Not reached at all.
    runtime.set_unreachable(RuntimeOp::InspectImageById);
    let unreachable = runtime
        .image_by_id(IMAGE_ID)
        .expect_err("armed unreachable");
    assert!(unreachable.is_unreachable(), "{unreachable}");
    assert!(
        unreachable.to_string().contains("cannot be reached"),
        "{unreachable}"
    );

    // And back: the toggle is a toggle, so a fixture can restore a runtime
    // mid-test — which is what a census that refuses and then succeeds needs.
    runtime.set_reachable(RuntimeOp::InspectImageById);
    let still_failing = runtime.image_by_id(IMAGE_ID).expect_err("still failing");
    assert!(
        !still_failing.is_unreachable(),
        "reachability and failure are independent arms, so clearing one must not \
         clear the other: {still_failing}"
    );

    let kinds: BTreeSet<bool> = [failed.is_unreachable(), unreachable.is_unreachable()]
        .into_iter()
        .collect();
    assert_eq!(kinds.len(), 2);
}

/// A container's exit status and output come back through the seam, which is
/// what lane A turns into a `ProcessOutput`.
///
/// Second field held constant: one container, one runtime; what varies is only
/// what it exited with. Three distinct exit values and two distinct streams, so
/// a `collect` that returned a constant fails.
#[test]
fn a_containers_exit_status_and_streams_come_back_through_the_seam() {
    let runtime = FakeRuntime::new(ContainerTrace::recording());
    runtime.seed_container(
        "upstroke-a-b-c-d",
        BTreeMap::new(),
        IMAGE_ID,
        IMAGE_ID,
        Liveness::Running,
    );
    let mut seen = BTreeSet::new();
    for code in [Some(0), Some(17), None] {
        runtime.set_execution(
            "upstroke-a-b-c-d",
            ContainerExecution {
                exit_code: code,
                stdout: b"out".to_vec(),
                stderr: b"err".to_vec(),
            },
        );
        let collected = runtime.collect("upstroke-a-b-c-d").expect("collected");
        assert_eq!(collected.exit_code, code);
        assert_eq!(collected.stdout, b"out");
        assert_eq!(collected.stderr, b"err");
        seen.insert(collected.exit_code);
    }
    assert_eq!(
        seen.len(),
        3,
        "signalled, zero and non-zero are three states"
    );

    // Liveness is a separate axis from the exit status: a container can be
    // observed running while carrying an exit value from its previous state.
    runtime.set_container_state("upstroke-a-b-c-d", Liveness::Exited);
    assert_eq!(
        runtime.observe("upstroke-a-b-c-d").expect("observed"),
        Liveness::Exited
    );
    assert!(!Liveness::Running.is_terminated());
    assert!(Liveness::Exited.is_terminated());
    assert!(
        Liveness::Gone.is_terminated(),
        "reclaim waits for exited OR removed; collapsing them makes two \
         concurrent reclaimers block each other"
    );
    // A container the runtime does not hold is Gone, not an error: that is what
    // makes reclaim tolerant of already-gone.
    assert_eq!(
        runtime.observe("never-existed").expect("observed"),
        Liveness::Gone
    );
}

/// The Docker gate refuses a test nothing counts, and its absence reason says
/// what is missing.
#[test]
fn the_docker_gate_refuses_an_uncounted_test_and_names_what_is_absent() {
    let reason = absent_reason();
    assert!(reason.contains(super::DOCKER_PROGRAM), "{reason}");
    assert!(reason.contains("daemon"), "{reason}");

    // Built rather than written, so `every_docker_gated_test_is_named_and_present`
    // — which reads gate call sites out of the source — does not see this
    // negative control as a fourth gated test.
    let unlisted = ["a", "test", "nobody", "listed"].join("_");
    let refused = std::panic::catch_unwind(|| docker_gate(&unlisted, ContainerTrace::off()));
    assert!(
        refused.is_err(),
        "the gate accepted a test that is not in DOCKER_GATED_TESTS, so a gated \
         test could exist that nothing counts"
    );
}

/// A runtime that never reports termination, wrapping another.
///
/// A wrapper rather than a flag on the fake, because "still running after the
/// kill" is a property of the *sequence of answers*, and a fake that could only
/// be armed to fail would make the refusal an error rather than a
/// never-converging observation.
struct NeverTerminates<'a>(&'a FakeRuntime);

impl ContainerRuntime for NeverTerminates<'_> {
    fn probe(&self) -> Result<(), RuntimeError> {
        self.0.probe()
    }
    fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        self.0.image_by_reference(reference)
    }
    fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        self.0.image_by_id(id)
    }
    fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
        self.0.volume_present(name)
    }
    fn containers_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<super::runtime::DiscoveredContainer>, RuntimeError> {
        self.0.containers_with_label(key, value)
    }
    fn observe(&self, name: &str) -> Result<Liveness, RuntimeError> {
        self.0.observe(name).map(|_| Liveness::Running)
    }
    fn collect(&self, name: &str) -> Result<ContainerExecution, RuntimeError> {
        self.0.collect(name)
    }
    fn create(&self, spec: &CreateSpec) -> Result<super::runtime::CreatedContainer, RuntimeError> {
        self.0.create(spec)
    }
    fn start(&self, name: &str) -> Result<(), RuntimeError> {
        self.0.start(name)
    }
    fn stop(&self, name: &str, mode: StopMode) -> Result<(), RuntimeError> {
        self.0.stop(name, mode)
    }
    fn remove(&self, name: &str) -> Result<(), RuntimeError> {
        self.0.remove(name)
    }
}

// ---------------------------------------------------------------------------
// 6. The namespace scan
// ---------------------------------------------------------------------------

/// The scan reads every record and skips the writer-owned staged half.
///
/// "discovery at every write-command start scans the whole namespace
/// `<R>/containers`". A `<name>.intent.tmp` is a crash between the stage and
/// the rename; adopting it would be adopting a record that was never
/// published.
#[test]
fn the_namespace_scan_reads_every_record_and_skips_the_staged_half() {
    let fixture = Fixture::new("scan", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();
    for (run, incarnation, invocation) in [
        (RUN_A, INCARNATION_1, shell_probe()),
        (RUN_A, INCARNATION_2, shell_probe()),
        (RUN_B, INCARNATION_1, agent_probe()),
    ] {
        write_intent(
            &mut hooks,
            ContainerSite::WriteIntent,
            &fixture.root,
            &name_for(run, incarnation, &invocation),
            &intent_for(run, incarnation, &invocation),
        )
        .expect("written");
    }
    // Residue a reader must ignore: a staged half, and a file that is not an
    // intent at all.
    let dir = containers_dir(&fixture.root);
    fs::write(dir.join("upstroke-a-b-c-d.intent.tmp"), b"{}").expect("staged");
    fs::write(dir.join("README"), b"not an intent").expect("stray");

    let found: Vec<FoundIntent> = list_intents(&fixture.root).expect("scanned");
    assert_eq!(
        found.len(),
        3,
        "{:?}",
        found.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let runs: BTreeSet<&str> = found.iter().map(|f| f.record.run_id.as_str()).collect();
    let incarnations: BTreeSet<&str> = found
        .iter()
        .map(|f| f.record.incarnation.as_str())
        .collect();
    assert_eq!(runs.len(), 2);
    assert_eq!(incarnations.len(), 2);
    // Sorted by name, so a census's report is stable across filesystems whose
    // directory order is not.
    let mut sorted = found.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    sorted.sort();
    assert_eq!(
        found.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        sorted
    );
}

/// A private root with no `containers` directory is an **empty namespace**, not
/// an error.
///
/// `crash_reconstruction`: "with no intent and no reachable runtime it
/// proceeds". A run that has never launched a container has no directory, and a
/// scan that treated that as a failure would refuse every write command on a
/// host runner.
#[test]
fn an_absent_containers_directory_is_an_empty_namespace() {
    let root = scratch("empty-namespace");
    assert!(!containers_dir(&root).exists());
    assert_eq!(list_intents(&root).expect("scanned"), Vec::new());
}

// ---------------------------------------------------------------------------
// 7. Enforcement: nothing performs a container effect outside this funnel
// ---------------------------------------------------------------------------

/// Every container effect in the tree goes through the funnel.
///
/// The census beside the denylist, in the idiom of
/// `runner::tests::every_production_process_start_is_classified`. Module
/// privacy cannot make a bypass a compile error from inside this subtree — an
/// item private to `runner::container` is visible to every module a lane adds
/// beside this one — so the enforcement is the clippy denylist (a build error)
/// and this census (a red test), and the two fail for different reasons.
///
/// **Lanes A and C: if this test names your file, you are calling the runtime
/// or the view directly. Call the funnel instead.**
#[test]
fn every_container_effect_in_the_tree_goes_through_the_funnel() {
    /// The effectful primitives, and the only file that may name them.
    const PRIMITIVES: &[&str] = &[
        "runtime.create(",
        "runtime.start(",
        "runtime.stop(",
        "runtime.remove(",
        "view.materialize(",
        "view.discard(",
        ".materialize(",
        ".discard(",
    ];
    const FUNNEL: &str = "src/runner/container.rs";

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();
    let mut scanned = 0;
    for path in walk(&root.join("src")) {
        let relative = path
            .strip_prefix(&root)
            .expect("under the manifest")
            .to_string_lossy()
            .replace('\\', "/");
        if relative == FUNNEL {
            continue;
        }
        // Test modules of this subtree drive the funnel and may construct a
        // fake; they are excluded by name rather than by a pattern, so a new
        // one is a change here.
        if relative == "src/runner/container/fake.rs" || relative == "src/runner/container/tests.rs"
        {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read source");
        let production =
            crate::effects::blank_comments_and_strings(&crate::effects::production_region(&source));
        scanned += 1;
        for primitive in PRIMITIVES {
            if production.contains(primitive) {
                offenders.push(format!("{relative} names `{primitive}`"));
            }
        }
    }
    assert!(scanned > 20, "the walk found the tree: {scanned}");
    assert!(offenders.is_empty(), "{offenders:#?}");

    // The control: the funnel itself names every one of them, so a census that
    // had stopped finding anything fails here rather than reporting silence.
    let funnel = fs::read_to_string(root.join(FUNNEL)).expect("the funnel");
    let production =
        crate::effects::blank_comments_and_strings(&crate::effects::production_region(&funnel));
    for primitive in [
        "runtime.create(",
        "runtime.start(",
        "runtime.stop(",
        "runtime.remove(",
    ] {
        assert!(
            production.contains(primitive),
            "the funnel does not name `{primitive}`; the census above is measuring nothing"
        );
    }
}

/// `source` with every comment blanked and every string literal left intact.
///
/// [`crate::effects::blank_comments_and_strings`] blanks both, which is right
/// for finding *code* and wrong for finding a name that lives inside a string —
/// a gated test's own name, or a `docker` program name. Blanking only the
/// comments is the half this census needs, and it is the half that stops a doc
/// comment about the scan being counted by the scan.
/// Every `src/**/*.rs`, sorted.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read src")
        .map(|entry| entry.expect("entry").path())
        .collect();
    entries.sort();
    let mut found = Vec::new();
    for path in entries {
        if path.is_dir() {
            found.extend(walk(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    found
}

/// Which level `source` states for `lint`, or nothing.
///
/// Comments and string literals are blanked first, so a lint level quoted in a
/// doc comment — which the file above and this one both do — is invisible.
/// `PR4-CENSUS-COMMENT-ORACLE` is the standing entry for a census that counted
/// its own prose.
fn stated_lint_level(source: &str, lint: &str) -> Option<&'static str> {
    let blanked = crate::effects::blank_comments_and_strings(source);
    for (keyword, answer) in [("allow(", "allow"), ("deny(", "deny")] {
        let mut rest = blanked.as_str();
        while let Some(index) = rest.find(keyword) {
            let after = &rest[index + keyword.len()..];
            let end = after.find(')').unwrap_or(after.len());
            if after[..end].contains(lint) {
                return Some(answer);
            }
            rest = &rest[index + keyword.len()..];
        }
    }
    None
}

/// Whether `effects/allowlist.toml` records `path` as allowing `lint`.
fn allowlist_records(path: &str, lint: &str) -> bool {
    let raw = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("effects")
            .join("allowlist.toml"),
    )
    .expect("the allowlist");
    raw.split("[[")
        .filter(|block| block.contains(&format!("path = \"{path}\"")))
        .any(|block| {
            let Some(allows) = block.split("allows = [").nth(1) else {
                return false;
            };
            let end = allows.find(']').unwrap_or(allows.len());
            allows[..end].contains(lint)
        })
}

/// Every child module of the Container funnel **states its own lint level**.
///
/// `PR6-LANEF-004`, and it is the one finding of this slice whose repair is
/// about the *next* lane rather than this one. `src/runner/container.rs` opens
/// with `#![allow(clippy::disallowed_methods, disallowed_types,
/// disallowed_macros)]` — an **inner** attribute — and a Rust lint level is
/// scoped by the **module tree**, not by the file. So every out-of-line child of
/// `runner::container` inherited it, and the build-error leg of
/// `effect_site_inventory.mechanism` (1) was not holding for exactly the modules
/// it exists for: a `ContainerRuntime::start` planted in a child passed
/// `cargo clippy --all-targets --all-features -- -D warnings`, measured twice.
///
/// Every file in this directory now either **denies** a governed lint or
/// **allows** it with an `effects/allowlist.toml` entry a reviewer reads. The
/// grid is {file} × {which of the three governed lints}, every cell asserted:
/// a file that states nothing about a lint is inheriting, and inheriting is the
/// defect.
///
/// The negative controls at the end are what stop this being a census that
/// cannot refuse: the predicate is driven over sources that state nothing, that
/// state a level only inside a doc comment, and that state each level plainly.
#[test]
fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {
    const GOVERNED: [&str; 3] = [
        "clippy::disallowed_methods",
        "clippy::disallowed_types",
        "clippy::disallowed_macros",
    ];
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let children = walk(&root.join("src").join("runner").join("container"));
    assert!(
        children.len() >= 8,
        "the walk found only {} child modules; the census is measuring nothing",
        children.len()
    );

    let mut missing = Vec::new();
    let mut unlisted = Vec::new();
    let mut cells = 0;
    for path in &children {
        let relative = path
            .strip_prefix(&root)
            .expect("under the manifest")
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(path).expect("read source");
        for lint in GOVERNED {
            cells += 1;
            match stated_lint_level(&source, lint) {
                None => missing.push(format!("{relative} states nothing about `{lint}`")),
                Some("allow") if !allowlist_records(&relative, lint) => unlisted.push(format!(
                    "{relative} allows `{lint}` and effects/allowlist.toml does not record it"
                )),
                Some(_) => {}
            }
        }
    }
    assert!(
        missing.is_empty(),
        "a child of the Container funnel inherits its allow instead of stating a \
         level of its own, which is `PR6-LANEF-004` reopening:\n{missing:#?}"
    );
    assert!(unlisted.is_empty(), "{unlisted:#?}");
    assert_eq!(cells, children.len() * 3);

    // The funnel itself is the one file that legitimately carries the allow, and
    // it is in the allowlist. Asserted here so "everything denies" cannot become
    // true by the funnel quietly denying itself out of existence.
    let funnel = fs::read_to_string(root.join("src/runner/container.rs")).expect("the funnel");
    for lint in GOVERNED {
        assert_eq!(
            stated_lint_level(&funnel, lint),
            Some("allow"),
            "the Container funnel no longer allows `{lint}`"
        );
        assert!(allowlist_records("src/runner/container.rs", lint));
    }

    // Negative controls: the predicate refuses what it is for.
    assert_eq!(
        stated_lint_level("fn go() {}\n", GOVERNED[0]),
        None,
        "a file that states nothing must read as stating nothing"
    );
    assert_eq!(
        stated_lint_level(
            "//! #![allow(clippy::disallowed_methods)]\nfn go() {}\n",
            GOVERNED[0]
        ),
        None,
        "a level quoted in a doc comment is not a level"
    );
    assert_eq!(
        stated_lint_level("#![deny(clippy::disallowed_methods)]\n", GOVERNED[0]),
        Some("deny")
    );
    assert_eq!(
        stated_lint_level("#![allow(clippy::disallowed_methods)]\n", GOVERNED[0]),
        Some("allow")
    );
    assert!(!allowlist_records(
        "src/runner/container/env.rs",
        GOVERNED[0]
    ));
}

/// Every Docker-gated test is named in the list that counts them, and every
/// name in the list is a test in this tree.
///
/// The skip is loud because it is **counted**: `docker_gate` refuses a test
/// that is not on the list, and this test refuses a name on the list that is
/// not a test. A gated test that vanished would otherwise shorten the list and
/// nothing would say so.
#[test]
fn every_docker_gated_test_is_named_and_present() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = String::new();
    for path in walk(&root.join("src").join("runner")) {
        sources.push_str(&fs::read_to_string(&path).expect("read source"));
    }
    assert!(!DOCKER_GATED_TESTS.is_empty());
    for name in DOCKER_GATED_TESTS {
        assert!(
            sources.contains(&format!("fn {name}(")),
            "`{name}` is in DOCKER_GATED_TESTS and is not a test in src/runner/**"
        );
    }
    // And every test that calls the gate is on the list: the name is readable
    // from the call site. Comments are blanked first, so this file's own prose
    // about the gate is not mistaken for a call — measured, because the first
    // version of this scan reported the placeholder in a doc comment as a
    // fourth gated test (`PR4-CENSUS-COMMENT-ORACLE`, the fifth occurrence).
    let mut called: BTreeSet<String> = BTreeSet::new();
    let stripped = crate::effects::blank_comments(&sources);
    let opener = "docker_gate(";
    let mut rest = stripped.as_str();
    while let Some(index) = rest.find(opener) {
        rest = &rest[index + opener.len()..];
        // `rustfmt` may put the name on the next line, so the first quote after
        // the call site is what names the test rather than the byte after the
        // paren. Measured: with a contiguous `gate("` needle this census found
        // **zero** call sites and reported the whole list as missing.
        let Some(open) = rest.find('"') else { break };
        let Some(end) = rest[open + 1..].find('"') else {
            break;
        };
        let name = &rest[open + 1..open + 1 + end];
        if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            called.insert(name.to_owned());
        }
        rest = &rest[open + 1 + end..];
    }
    let listed: BTreeSet<String> = DOCKER_GATED_TESTS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        called, listed,
        "the set of tests that call the Docker gate and the set the list counts disagree"
    );
}

// ---------------------------------------------------------------------------
// 8. Docker-gated: the real runtime
// ---------------------------------------------------------------------------

/// The references the gated tests prefer, in order.
///
/// **These tests never pull.** `non_goals[1]` is "implicit image pull", and a
/// fixture that pulled would be exercising the behaviour the slice forbids on
/// the very runtime it is meant to prove the refusal against. So the image is
/// *discovered* among what the machine already holds, and a machine holding
/// none reports absence through the same loud, counted gate as a machine with
/// no Docker at all.
const PREFERRED_IMAGES: &[&str] = &["alpine:3.20", "busybox:latest", "debian:stable-slim"];

/// A reference the runtime holds, with its id and digest, or the reason there
/// is none.
fn gated_image(docker: &dyn ContainerRuntime) -> Result<(String, ImageInspection), String> {
    for reference in PREFERRED_IMAGES {
        if let Ok(Some(found)) = docker.image_by_reference(reference) {
            return Ok(((*reference).to_owned(), found));
        }
    }
    Err(format!(
        "the container runtime holds none of {PREFERRED_IMAGES:?} and these tests          never pull (non_goals[1])"
    ))
}

/// The real runtime resolves a reference it holds to an id and, when it has
/// one, a manifest digest.
#[test]
fn real_docker_reports_an_image_id_and_a_digest_for_a_reference_it_holds() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_reports_an_image_id_and_a_digest_for_a_reference_it_holds",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let (reference, found) = match gated_image(docker.as_ref()) {
        Ok(image) => image,
        Err(reason) => return no_image(&reason),
    };
    assert!(found.id.starts_with("sha256:"), "{}", found.id);
    assert!(
        found.references.contains(&reference),
        "{:?}",
        found.references
    );
    // The same image asked for by id gives the same id back, and a prefix of it
    // does not answer this question.
    let by_id = docker
        .image_by_id(&found.id)
        .expect("reachable")
        .expect("present");
    assert_eq!(by_id.id, found.id);
    assert_eq!(by_id.digest, found.digest);
    assert_eq!(
        docker
            .image_by_id(&found.id[..found.id.len() - 8])
            .expect("reachable"),
        None,
        "an id prefix resolves in docker and is not the recorded id"
    );
    assert!(trace.ops().contains(&RuntimeOp::InspectImageByReference));
}

/// A reference the runtime does not hold is **absent**, and nothing pulls it.
#[test]
fn real_docker_refuses_a_reference_it_does_not_hold_without_pulling() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_refuses_a_reference_it_does_not_hold_without_pulling",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let absent = "ghcr.io/upstroke-does-not-exist/nothing:v0";
    assert_eq!(
        docker.image_by_reference(absent).expect("reachable"),
        None,
        "an absent reference is absence, not a pull"
    );
    assert_eq!(
        docker
            .image_by_id("sha256:0000000000000000000000000000000000000000000000000000000000000000")
            .expect("reachable"),
        None
    );
    assert!(
        !docker
            .volume_present("upstroke-volume-that-does-not-exist")
            .expect("reachable")
    );
}

/// Poll a real container until it is no longer running.
///
/// Bounded round trips rather than a sleep, in the idiom of
/// [`super::observe_terminated`] — `determinism` forbids sleeps, and each
/// `docker container inspect` is itself a round trip that takes tens of
/// milliseconds, so the bound is a real one.
fn wait_until_terminated(docker: &dyn ContainerRuntime, name: &str) -> Liveness {
    for _ in 0..200 {
        let state = docker.observe(name).expect("reachable");
        if state.is_terminated() {
            return state;
        }
        std::thread::yield_now();
    }
    panic!("`{name}` is still running after 200 observations");
}

/// The whole R26 lifecycle against the real runtime: create from an id, verify
/// what it reports, launch through the funnel, reclaim, and reclaim again.
///
/// **The plan carries the Git view as a real bind mount**, and that is the whole
/// reason this test can see `PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE`. It used
/// to carry `mounts: Vec::new()`, and a real-runtime test with no mounts cannot
/// see a mount defect: `launch` mounted the view *after* `docker create`, the
/// view is a bind-mount **source** of that call, and Docker requires a bind
/// source to exist at create time — so `launch` could not produce a working
/// container with a Git view at all, and this test passed anyway.
#[test]
fn real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let (_, image) = match gated_image(docker.as_ref()) {
        Ok(image) => image,
        Err(reason) => return no_image(&reason),
    };

    let root = scratch("real-docker");
    let invocation = shell_probe();
    let record = intent_for(RUN_A, INCARNATION_1, &invocation);
    let name = name_for(RUN_A, INCARNATION_1, &invocation);
    let view = DisposableDirView::new(trace.clone());
    let view_path = root.join("views").join(name.as_str());
    let plan = LaunchPlan {
        private_root: root.clone(),
        name: name.clone(),
        invocation: invocation.clone(),
        intent: record.clone(),
        spec: CreateSpec {
            name: name.as_str().to_owned(),
            image_id: image.id.clone(),
            labels: record.labels(&root),
            // The R19 view, as a REAL bind mount whose source is the directory
            // `Container.MountGitView` materialises. This is the mount the
            // daemon refuses if the view is not there when the container is
            // created, and it is what makes this test able to see the ordering.
            mounts: vec![Mount::Path {
                source: view_path.clone(),
                target: "/upstroke/gitview".to_owned(),
                read_only: false,
            }],
            env: Vec::new(),
            command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()],
            workdir: None,
            read_only_root: true,
        },
        view: GitViewRequest {
            path: view_path.clone(),
            workspace: root.clone(),
            head: None,
        },
    };

    let mut hooks = RecordingHooks::new(trace.clone());
    let launched: Launched = match launch(&mut hooks, docker.as_ref(), &view, &plan) {
        Ok(launched) => launched,
        Err(error) => {
            // Leave nothing behind even when the launch itself failed.
            let _ = reclaim(
                &mut hooks,
                docker.as_ref(),
                &view,
                &root,
                &name,
                Some(&plan.view.path),
            );
            panic!("the real runtime refused the launch: {error}");
        }
    };
    assert_eq!(launched.reported_image_id, image.id);
    assert!(launched.intent_path.exists());

    // Discovery finds it by `upstroke.private_root`, with its five labels.
    let discovered = docker
        .containers_with_label(LABEL_PRIVATE_ROOT, &private_root_label(&root))
        .expect("reachable");
    assert_eq!(discovered.len(), 1, "{discovered:?}");
    for label in LABELS {
        assert!(
            discovered[0].labels.contains_key(*label),
            "the real container is missing `{label}`"
        );
    }

    // Reclaim, twice: idempotent and tolerant of already-gone.
    for round in 0..2 {
        reclaim(
            &mut hooks,
            docker.as_ref(),
            &view,
            &root,
            &name,
            Some(&launched.view_path),
        )
        .unwrap_or_else(|error| panic!("round {round} refused: {error}"));
    }
    assert!(!launched.intent_path.exists());
    assert!(!launched.view_path.exists());
    assert_eq!(
        docker
            .containers_with_label(LABEL_PRIVATE_ROOT, &private_root_label(&root))
            .expect("reachable")
            .len(),
        0
    );
}

/// The daemon really does answer "is not running", and `DockerCli::stop`
/// tolerates it.
///
/// `PR6-LANEF-003`'s other half. `a_stop_answer_meaning_already_settled_is_tolerated_and_a_real_failure_is_not`
/// drives [`super::settle_stop`] over a **transcribed** table, and a transcribed
/// table becomes its own oracle the moment the daemon's wording changes. This
/// asks the live daemon the same question — a `docker kill` of a container that
/// has already exited, which is exactly what the second of two racing reclaimers
/// issues — and asserts both that the phrase is still there and that the seam's
/// `stop` converges on it.
///
/// Second field held constant: the same container, in one state; what varies is
/// whether the question is asked raw or through the seam.
#[test]
fn real_docker_kill_on_an_already_exited_container_is_tolerated() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_kill_on_an_already_exited_container_is_tolerated",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let (_, image) = match gated_image(docker.as_ref()) {
        Ok(image) => image,
        Err(reason) => return no_image(&reason),
    };

    let name = "upstroke-f1-already-exited";
    let spec = CreateSpec {
        name: name.to_owned(),
        image_id: image.id.clone(),
        labels: BTreeMap::new(),
        mounts: Vec::new(),
        env: Vec::new(),
        command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()],
        workdir: None,
        read_only_root: true,
    };
    let _ = docker.remove(name);
    docker.create(&spec).expect("created");
    docker.start(name).expect("started");
    assert!(
        wait_until_terminated(docker.as_ref(), name).is_terminated(),
        "the fixture container has to actually be exited"
    );

    // The raw answer, from the daemon, verbatim.
    let raw = docker
        .raw(RuntimeOp::Stop, name, &["kill", name])
        .expect_err("a kill of an exited container fails");
    let RuntimeError::Failed { detail, .. } = &raw else {
        panic!("the daemon was reached and answered a failure, not: {raw}");
    };
    assert!(
        detail.contains("is not running"),
        "the daemon's already-stopped wording moved, and the transcribed table in \
         this file no longer matches it: {detail}"
    );
    assert!(
        super::stop_already_settled(detail),
        "the tolerance does not recognise the daemon's own answer: {detail}"
    );

    // And through the seam, which is what a reclaimer calls: it converges.
    docker
        .stop(name, StopMode::Kill)
        .expect("a reclaimer arriving second converges on an already-stopped container");

    docker.remove(name).expect("removed");
    assert_eq!(docker.observe(name).expect("reachable"), Liveness::Gone);
}

/// A container's **stderr** comes back, and separately from its stdout.
///
/// `PR6A-DOCKERCLI-MERGES-STDERR-INTO-STDOUT`. `collect` returned
/// `stderr: Vec::new()` with a comment claiming `docker logs` interleaves the
/// streams; measured on docker 29.7.2 it **separates** them, and the code did
/// not merge them — it discarded one. A gate's failure output becomes retry
/// feedback (DESIGN.md:576), so a gate that fails with everything on stderr
/// produced empty feedback, and `host::run_shell_probe`'s refusal quotes
/// `output.stderr` and would have quoted nothing.
///
/// The intersection: {which stream a byte was written to} × {which field it
/// arrives in}. Both directions of the cross are asserted — stdout must **not**
/// carry the stderr marker and stderr must **not** carry the stdout one — so a
/// `collect` that merged the two into both fields fails here as surely as one
/// that dropped one.
#[test]
fn real_docker_returns_both_streams_of_a_container_separately() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_returns_both_streams_of_a_container_separately",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let (_, image) = match gated_image(docker.as_ref()) {
        Ok(image) => image,
        Err(reason) => return no_image(&reason),
    };

    let name = "upstroke-f1-two-streams";
    let spec = CreateSpec {
        name: name.to_owned(),
        image_id: image.id.clone(),
        labels: BTreeMap::new(),
        mounts: Vec::new(),
        env: Vec::new(),
        command: vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "echo ON-STDOUT; echo ON-STDERR 1>&2; exit 3".to_owned(),
        ],
        workdir: None,
        read_only_root: true,
    };
    let _ = docker.remove(name);
    docker.create(&spec).expect("created");
    docker.start(name).expect("started");
    wait_until_terminated(docker.as_ref(), name);

    let collected = docker.collect(name).expect("collected");
    let stdout = String::from_utf8_lossy(&collected.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&collected.stderr).into_owned();
    docker.remove(name).expect("removed");

    assert_eq!(
        collected.exit_code,
        Some(3),
        "the exit status comes back too, and is not the CLI's own"
    );
    assert!(stdout.contains("ON-STDOUT"), "stdout was {stdout:?}");
    assert!(
        stderr.contains("ON-STDERR"),
        "the container's stderr was discarded; a failing gate's diagnostic is its \
         stderr and becomes retry feedback: stderr was {stderr:?}"
    );
    assert!(
        !stdout.contains("ON-STDERR"),
        "the two streams were merged into stdout: {stdout:?}"
    );
    assert!(
        !stderr.contains("ON-STDOUT"),
        "the two streams were merged into stderr: {stderr:?}"
    );
}

/// Removing a container reclaims the **anonymous** volumes it was created with.
///
/// `PR6A-ANONYMOUS-VOLUMES-LEAK`. `docker rm --force` without `--volumes` leaves
/// one anonymous volume per container behind for any image declaring `VOLUME`
/// or any create carrying `--volume <path>`; **29 leaked from one run of this
/// suite**, counted by lane A. Those volumes are not R20 — R20 is the
/// operator's *named*, per-agent credential volume, `operator_owned` and
/// `persistent_output` in all five `at_run_end` outcomes — they belong to
/// **R26**, the container's own row, and `Container.Remove` is where R26
/// balances.
///
/// The volume is identified **by name**, read back from the container the test
/// created, rather than by counting `docker volume ls`: a count is polluted by
/// whatever else on this machine is making volumes, and a named volume is not.
/// The control is the assertion **before** the removal — without it, a fixture
/// that had stopped creating an anonymous volume at all would pass silently.
///
/// The intersection: {a volume the run created} × {a volume the operator owns}.
/// `--volumes` removes only the first kind, which is what makes this repair a
/// discharge of R26 rather than a violation of R20 — and the R20 half is
/// asserted here too, with a named volume that must survive.
#[test]
fn real_docker_removing_a_container_reclaims_its_anonymous_volumes() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_removing_a_container_reclaims_its_anonymous_volumes",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let (_, image) = match gated_image(docker.as_ref()) {
        Ok(image) => image,
        Err(reason) => return no_image(&reason),
    };

    let name = "upstroke-f1-anonymous-volume";
    let named = "upstroke-f1-operator-owned";
    let _ = docker.remove(name);
    // R20's half: a NAMED volume the operator owns, mounted into the same
    // container. `--volumes` must not touch it.
    let _ = docker.raw(
        RuntimeOp::InspectVolume,
        named,
        &["volume", "create", named],
    );
    assert!(
        docker.volume_present(named).expect("reachable"),
        "the operator-owned control volume was not created"
    );

    docker
        .raw(
            RuntimeOp::Create,
            name,
            &[
                "create",
                "--name",
                name,
                // An ANONYMOUS volume: `CreateSpec::Mount::Volume` cannot express
                // one because it requires a name, which is why this goes through
                // the test-only raw accessor.
                "--volume",
                "/upstroke-anonymous",
                "--volume",
                &format!("{named}:/upstroke-named"),
                &image.id,
                "/bin/sh",
                "-c",
                "exit 0",
            ],
        )
        .expect("created");

    let anonymous = docker
        .raw(
            RuntimeOp::Observe,
            name,
            &[
                "container",
                "inspect",
                name,
                "--format",
                "{{range .Mounts}}{{if not .Name}}{{else}}{{.Name}}\n{{end}}{{end}}",
            ],
        )
        .expect("reachable")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != named)
        .map(str::to_owned)
        .next()
        .expect("the container carries an anonymous volume");
    // The control: it really is there before the removal, so this test cannot
    // pass by never having created one.
    assert!(
        docker.volume_present(&anonymous).expect("reachable"),
        "the fixture did not create an anonymous volume, so the assertion below \
         would hold vacuously"
    );

    docker.remove(name).expect("removed");

    let leaked = docker.volume_present(&anonymous).expect("reachable");
    let operators_survived = docker.volume_present(named).expect("reachable");
    // Clean up before asserting, so a failure does not leave the daemon dirty.
    let _ = docker.raw(
        RuntimeOp::InspectVolume,
        &anonymous,
        &["volume", "rm", &anonymous],
    );
    let _ = docker.raw(RuntimeOp::InspectVolume, named, &["volume", "rm", named]);

    assert!(
        !leaked,
        "`docker rm` left the container's anonymous volume `{anonymous}` behind; \
         nothing else can ever refer to it and no resource row accounts for it"
    );
    assert!(
        operators_survived,
        "the removal took the OPERATOR's named volume with it — R20 is \
         `operator_owned` and `persistent_output` in all five at_run_end outcomes"
    );
}

// ---------------------------------------------------------------------------
// `PR6-RECOV-002` — `docker ps` renders labels ambiguously, so this does not
// ask it to
// ---------------------------------------------------------------------------

/// The format string asks for exactly the labels the parser names, in order.
///
/// Two lists that must agree; the failure mode if they drift is a field read
/// under the wrong name, which for `upstroke.run_dir` is a probe of another run's
/// lock. The oracle is the format string's own text, scanned for `{{.Label
/// "…"}}` — an independent derivation from `PS_LABELS`, not a restatement.
#[test]
fn the_ps_format_asks_for_exactly_the_labels_the_parser_names() {
    let mut asked = Vec::new();
    let mut rest = PS_FORMAT;
    while let Some(at) = rest.find("{{.Label \"") {
        rest = &rest[at + "{{.Label \"".len()..];
        let end = rest.find('"').expect("an unterminated .Label placeholder");
        asked.push(&rest[..end]);
        rest = &rest[end..];
    }
    assert_eq!(
        asked,
        PS_LABELS.to_vec(),
        "the format string and PS_LABELS disagree about which label is which field"
    );
    assert_eq!(
        asked.iter().collect::<BTreeSet<_>>().len(),
        LABELS.len(),
        "the census asks for all five labels the intent writes, each once"
    );
    assert_eq!(
        asked.iter().copied().collect::<BTreeSet<_>>(),
        LABELS.iter().copied().collect::<BTreeSet<_>>()
    );
    // The name field, and exactly one separator per field boundary.
    assert!(PS_FORMAT.starts_with("{{.Names}}"));
    assert_eq!(
        PS_FORMAT.matches(PS_FIELD_SEPARATOR).count(),
        PS_LABELS.len(),
        "one separator between each of the {} fields",
        PS_LABELS.len() + 1
    );
    assert!(
        !PS_FORMAT.contains("{{.Labels}}"),
        "`{{{{.Labels}}}}` renders every label as an unescaped comma-joined string and a label \
         value may contain commas; asking for it is `PR6-RECOV-002`"
    );
}

/// A label value carrying the delimiters of the *old* rendering survives whole.
///
/// The parse's oracle is a hand-written table of `(rendered line, expected
/// name, expected labels)`, built from what `docker ps` really prints — see
/// `real_docker_renders_a_comma_bearing_label_value_whole` for the same values
/// checked against the live daemon.
///
/// Second field held constant: every line carries the same container name and
/// the same four other labels; only `upstroke.run_dir`'s bytes move, across
/// values that are and are not hostile to a comma-joined format.
#[test]
fn a_label_value_carrying_a_comma_or_an_equals_is_read_whole() {
    let sep = PS_FIELD_SEPARATOR;
    let values = [
        "/repo/.upstroke/runs/B",
        "/repo/a%2Cb/.upstroke/runs/B",
        // Not values `path_label` emits — a foreign container may carry
        // anything, and the parser must still read the field it was given
        // rather than the prefix before the first comma.
        "/repo/a,b/.upstroke/runs/B",
        "/repo/a=b/.upstroke/runs/B",
        "/repo/a,upstroke.run=IMPOSTOR/.upstroke/runs/B",
    ];
    assert_eq!(
        values.iter().collect::<BTreeSet<_>>().len(),
        values.len(),
        "five distinct run directories"
    );
    for value in values {
        let line = format!(
            "upstroke-k-r-i-h{sep}/srv/private{sep}RUNB{sep}{value}{sep}INC2{sep}p.shell.o0"
        );
        let found = parse_ps_output(&line).expect("one container");
        assert_eq!(found.len(), 1, "`{value}`");
        assert_eq!(found[0].name, "upstroke-k-r-i-h", "`{value}`");
        assert_eq!(
            found[0].label(LABEL_RUN_DIR),
            Some(value),
            "`{value}`: the run directory was truncated, and arm (ii) probes a shorter path"
        );
        assert_eq!(found[0].label(LABEL_RUN), Some("RUNB"), "`{value}`");
        assert_eq!(found[0].label(LABEL_INCARNATION), Some("INC2"), "`{value}`");
        assert_eq!(
            found[0].label(LABEL_PRIVATE_ROOT),
            Some("/srv/private"),
            "`{value}`"
        );
        assert_eq!(
            found[0].label(LABEL_INVOCATION),
            Some("p.shell.o0"),
            "`{value}`"
        );
    }
}

/// A line whose fields do not line up is **refused**, not mis-split.
///
/// The fail-closed half of choosing a delimiter: this engine's own label values
/// never carry `U+001F` or a newline, but a foreign container carrying this
/// private root's label may carry anything, and a census that guessed which
/// field was the owner's run directory would probe another run's lock. `Failed`
/// and not `Unreachable`, because the daemon answered.
///
/// Second field held constant: every line below is one container's worth of
/// output with a valid name; only the field count moves.
#[test]
fn a_ps_line_whose_fields_do_not_line_up_is_refused() {
    let sep = PS_FIELD_SEPARATOR;
    let good = format!("c{sep}/srv/private{sep}RUNB{sep}/repo/runs/B{sep}INC2{sep}p.shell.o0");
    assert_eq!(parse_ps_output(&good).expect("well formed").len(), 1);

    for (what, line) in [
        (
            "a value carrying the field separator",
            format!("{good}{sep}extra"),
        ),
        ("a short line", format!("c{sep}/srv/private{sep}RUNB")),
        (
            "a value carrying a newline, which splits one container across two lines",
            good.replace("/repo/runs/B", "/repo/runs\nB"),
        ),
    ] {
        let error = parse_ps_output(&line).expect_err(what);
        assert!(
            !error.is_unreachable(),
            "{what}: the daemon answered, so this is not unreachability: {error}"
        );
        assert_eq!(error.operation(), RuntimeOp::ListByLabel, "{what}");
    }

    // An empty rendered value is an absent label, and both are refused
    // downstream. A container with no name at all is skipped rather than
    // refused: `docker ps` renders a blank line for nothing this filter
    // selected.
    let empty = format!("c{sep}/srv/private{sep}RUNB{sep}{sep}INC2{sep}p.shell.o0");
    let found = parse_ps_output(&empty).expect("well formed");
    assert_eq!(found[0].label(LABEL_RUN_DIR), None);
    assert!(parse_ps_output("\n   \n").expect("blank").is_empty());
}

// ---------------------------------------------------------------------------
// `PR6-RECOV-005` — "permission denied" is not "no runtime"
// ---------------------------------------------------------------------------

/// The verbatim stderr of a `docker` that could not be reached, and the command
/// that produced each.
///
/// Transcribed from runs on this project's build box against `docker` 29.7.2.
/// This is the table `is_unreachable_diagnostic` is measured against, and it is
/// **not** derived from `UNREACHABLE_DIAGNOSTICS`:
/// `real_docker_prints_the_transcribed_unreachable_diagnostics` replays two of
/// these through the live CLI so the oracle is the daemon.
const UNREACHABLE_STDERR: &[(&str, &str)] = &[
    (
        "sudo -u nobody docker ps",
        "permission denied while trying to connect to the docker API at unix:///var/run/docker.sock",
    ),
    (
        "DOCKER_HOST=unix:///nonexistent/docker.sock docker ps",
        "failed to connect to the docker API at unix:///nonexistent/docker.sock; check if the \
         path is correct and if the daemon is running: dial unix /nonexistent/docker.sock: \
         connect: no such file or directory",
    ),
    (
        "DOCKER_HOST=tcp://127.0.0.1:1 docker ps",
        "Cannot connect to the Docker daemon at tcp://127.0.0.1:1. Is the docker daemon running?",
    ),
    (
        "an older client, kept because the wording is still shipped",
        "Got permission denied while trying to connect to the Docker daemon socket at \
         unix:///var/run/docker.sock: Head \"http://%2Fvar%2Frun%2Fdocker.sock/_ping\": dial unix \
         /var/run/docker.sock: connect: permission denied",
    ),
    (
        "docker on Windows with no engine, named pipe absent",
        "error during connect: Get \"http://%2F%2F.%2Fpipe%2Fdocker_engine/_ping\": open \
         //./pipe/docker_engine: The system cannot find the file specified.",
    ),
];

/// The stderr of a daemon that **answered** and refused.
///
/// The other half of the classification, and it has to be a table of its own:
/// a predicate that returned `true` for everything would pass the table above
/// and turn every real failure into "proceed without a runtime".
const ANSWERED_STDERR: &[&str] = &[
    "Error response from daemon: No such container: no-such-container-xyz",
    "Error response from daemon: cannot kill container: c: container 9f is not running",
    "Error response from daemon: conflict: unable to remove repository reference",
    "invalid reference format",
    "docker: 'nope' is not a docker command.",
    "Error response from daemon: pull access denied for private/image, repository does not exist",
];

/// Every measured "could not be reached" classifies as unreachable, and every
/// measured "answered and refused" does not.
///
/// `crash_reconstruction`: "with no intent and no reachable runtime it
/// **proceeds**". `PR6-RECOV-005`: the shipped three-string test classified the
/// socket-permission diagnostic as `Failed`, so a census with no intents at all
/// refused — on the single most common configuration of a machine that has
/// Docker installed and not configured. Measuring it turned up a second one:
/// `docker` 29's wording for an **absent socket** was also classified `Failed`.
///
/// Second field held constant: one operation and one call shape in every cell;
/// only the diagnostic text moves.
#[test]
fn the_docker_diagnostic_classifier_tells_unreachable_from_answered() {
    for (command, detail) in UNREACHABLE_STDERR {
        assert!(
            is_unreachable_diagnostic(detail),
            "`{command}` produced a diagnostic classified as an answered failure, so a census \
             with no container evidence refuses instead of proceeding: {detail}"
        );
        let error = classify_docker_failure(RuntimeOp::ListByLabel, (*detail).to_owned());
        assert!(error.is_unreachable(), "`{command}`");
        assert!(
            super::census::proceeds_without(&error),
            "`{command}`: the census would refuse"
        );
    }
    for detail in ANSWERED_STDERR {
        assert!(
            !is_unreachable_diagnostic(detail),
            "a daemon that answered was classified unreachable, so a write command proceeds past \
             a runtime that will not list: {detail}"
        );
        let error = classify_docker_failure(RuntimeOp::ListByLabel, (*detail).to_owned());
        assert!(!error.is_unreachable(), "{detail}");
        assert!(!super::census::proceeds_without(&error), "{detail}");
    }
    assert!(UNREACHABLE_STDERR.len() >= 5 && ANSWERED_STDERR.len() >= 5);
}

/// The two `docker` diagnostic tables never claim one message.
///
/// `stop_already_settled` matches "is not running", which is a **reached**
/// daemon reporting a container's state; if an unreachable shape ever matched
/// it, a racing reclaimer's tolerated error would become "the runtime cannot be
/// reached" and refuse the write command. Asserted over both tables at once so
/// a future entry in either is checked against the other.
#[test]
fn the_two_docker_diagnostic_tables_never_claim_one_message() {
    for (command, detail) in UNREACHABLE_STDERR {
        assert!(
            !super::is_absent(detail),
            "`{command}`: an unreachable runtime read as an absent object, which is tolerated \
             silently: {detail}"
        );
    }
    for detail in ANSWERED_STDERR {
        assert!(!is_unreachable_diagnostic(detail), "{detail}");
    }
    // And the one message that is *most* at risk: a racing reclaimer's kill.
    let racing =
        "Error response from daemon: cannot kill container: c: container 9f is not running";
    assert!(super::stop_already_settled(racing));
    assert!(!is_unreachable_diagnostic(racing));
}

/// The live daemon's `.Labels` really is ambiguous, and `.Label "…"` really is
/// not.
///
/// `PR6-RECOV-002`'s premise, checked against `docker` rather than against a
/// transcription of it: a container is created whose `upstroke.run_dir` contains
/// a comma, and the two renderings are compared. The comma-joined one is
/// **asserted to be ambiguous** — it is byte-identical to what a container with
/// an extra label would print — and the census's own path is asserted to give
/// the value back whole.
///
/// Second field held constant: one container, one label set; only the format
/// string handed to `docker ps` moves.
#[test]
fn real_docker_renders_a_comma_bearing_label_value_whole() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_renders_a_comma_bearing_label_value_whole",
        trace,
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let (_, image) = match gated_image(docker.as_ref()) {
        Ok(image) => image,
        Err(reason) => return no_image(&reason),
    };

    let root = format!("/srv/private/r2-labels-{}", std::process::id());
    let hostile = "/repo/a,b=c/.upstroke/runs/RUNB";
    let name = format!("upstroke-r2labels-{}", std::process::id());
    let mut labels = BTreeMap::new();
    labels.insert(LABEL_PRIVATE_ROOT.to_owned(), root.clone());
    labels.insert(LABEL_RUN.to_owned(), "RUNB".to_owned());
    labels.insert(LABEL_RUN_DIR.to_owned(), hostile.to_owned());
    labels.insert(LABEL_INCARNATION.to_owned(), "INC2".to_owned());
    labels.insert(LABEL_INVOCATION.to_owned(), "p.shell.o0".to_owned());
    let spec = CreateSpec {
        name: name.clone(),
        image_id: image.id.clone(),
        labels,
        mounts: Vec::new(),
        env: Vec::new(),
        command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()],
        workdir: None,
        // Repair R1 added this field after R2 wrote this test. `true` matches
        // every other CreateSpec in the suite and is what the runner now
        // supplies; this container only runs `exit 0` and writes nothing.
        read_only_root: true,
    };
    docker.create(&spec).expect("create the labelled container");

    // (a) What the census asks for, through the production seam.
    let found = docker
        .containers_with_label(LABEL_PRIVATE_ROOT, &root)
        .expect("list by label");
    let listed = found
        .iter()
        .find(|container| container.name == name)
        .unwrap_or_else(|| panic!("the container is not in {found:#?}"));
    assert_eq!(
        listed.label(LABEL_RUN_DIR),
        Some(hostile),
        "the owner's run directory came back truncated, so arm (ii) would probe a shorter path"
    );
    assert_eq!(listed.label(LABEL_RUN), Some("RUNB"));
    assert_eq!(listed.label(LABEL_INCARNATION), Some("INC2"));

    // (b) The rendering that shipped, from the same daemon, asserted to be
    // ambiguous. `{{.Labels}}` prints the comma inside the value exactly as it
    // prints the separator between labels, so the bytes below are also a
    // perfectly good rendering of a *different* label set — which is what makes
    // parsing them a guess rather than a read.
    let raw = docker
        .raw(
            RuntimeOp::ListByLabel,
            &root,
            &[
                "ps",
                "--all",
                "--filter",
                &format!("label={LABEL_PRIVATE_ROOT}={root}"),
                "--format",
                "{{.Labels}}",
            ],
        )
        .expect("the daemon answers");
    let line = raw.lines().next().expect("one line").to_owned();
    assert!(
        line.contains(hostile),
        "the daemon did not print the value at all: {line:?}"
    );
    let truncated = line
        .split(',')
        .find_map(|pair| pair.strip_prefix(&format!("{LABEL_RUN_DIR}=")))
        .expect("the shipped parser's answer");
    assert_ne!(
        truncated, hostile,
        "this test's premise is that `{{{{.Labels}}}}` is ambiguous, and on this daemon it was \
         not: {line:?}"
    );
    assert!(
        hostile.starts_with(truncated),
        "the shipped parser truncated to `{truncated}`, which is a prefix of the real value and \
         therefore a different, shorter directory"
    );

    docker.remove(&name).expect("reclaim the container");
}

/// The transcribed unreachable diagnostics are what the live CLI prints.
///
/// `UNREACHABLE_STDERR` is a table of strings, and a table compared only
/// against the classifier built from it proves nothing. This asks the real
/// `docker` binary for two of them — an absent socket and a socket this process
/// may not use — and classifies **its** stderr.
///
/// It drives `docker` directly rather than through [`DockerCli`], because
/// `DOCKER_HOST` is process-wide and the seam deliberately configures no
/// socket (`non_goals[3]`, "remote runners").
#[test]
fn real_docker_prints_the_transcribed_unreachable_diagnostics() {
    let trace = ContainerTrace::recording();
    if let Err(reason) = docker_gate(
        "real_docker_prints_the_transcribed_unreachable_diagnostics",
        trace,
    ) {
        return skipped(&reason);
    }

    let mut cases: Vec<(&str, String)> = Vec::new();
    cases.push((
        "an absent socket",
        format!("unix:///nonexistent-{}/docker.sock", std::process::id()),
    ));

    // A socket path this process may not reach into. `chmod 000` is the
    // deterministic way to produce the *permission* diagnostic without a second
    // user account; running as root would defeat it, so that case is skipped
    // rather than asserted falsely.
    #[cfg(unix)]
    let denied = {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = std::env::temp_dir().join(format!("upstroke-r2-denied-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch directory");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).expect("chmod 000");
        let reachable = fs::read_dir(&dir).is_ok();
        if reachable {
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o755));
            let _ = fs::remove_dir_all(&dir);
            None
        } else {
            cases.push((
                "a socket this process may not use",
                format!("unix://{}/docker.sock", dir.display()),
            ));
            Some(dir)
        }
    };

    for (what, host) in &cases {
        let spec = CommandSpec::new(super::DOCKER_PROGRAM)
            .arg("ps")
            .arg("--all")
            .arg("--quiet");
        let output = host::build_command(&spec)
            .env("DOCKER_HOST", host)
            .output()
            .expect("docker starts");
        assert!(!output.status.success(), "[{what}] docker succeeded");
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        assert!(
            is_unreachable_diagnostic(&stderr),
            "[{what}] the live CLI printed a diagnostic this classifier calls an answered \
             failure, so a census with no container evidence would refuse: {stderr:?}"
        );
        assert!(
            classify_docker_failure(RuntimeOp::ListByLabel, stderr.clone()).is_unreachable(),
            "[{what}] {stderr:?}"
        );
    }
    assert!(
        cases.len() >= 2 || cfg!(not(unix)),
        "the permission case did not run: this process may be root"
    );

    #[cfg(unix)]
    if let Some(dir) = denied {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o755));
        let _ = fs::remove_dir_all(&dir);
    }
}

// ---------------------------------------------------------------------------
// `PR6-RECOV-004` — the production liveness probe, against a lock a real other
// process holds
// ---------------------------------------------------------------------------

/// The child of [`the_production_lock_probe_sees_a_lock_another_process_holds`].
///
/// Takes a real `RunLock` on the directory it is given, creates the readiness
/// file it was told to, and waits. `#[ignore]`d because it is a fixture rather
/// than a test: it is invoked by name, as a subprocess, in the idiom
/// `rundir::tests::lock_child_holds_the_run` established for exactly this.
///
/// It signals through a **file** rather than through stdout, so the fixture
/// needs neither `println!` nor a piped `Stdio` — `clippy::disallowed_macros`
/// is re-denied in this file by `PR6-LANEF-004` and this repair does not widen
/// that.
#[test]
#[ignore = "spawned as a subprocess by the_production_lock_probe_sees_a_lock_another_process_holds"]
fn container_lock_probe_child_holds_the_run() {
    let public = PathBuf::from(std::env::var("UPSTROKE_TEST_LOCK_DIR").expect("run dir"));
    let ready = PathBuf::from(std::env::var("UPSTROKE_TEST_READY").expect("readiness path"));
    let _held = crate::rundir::RunLock::acquire(&public).expect("the child takes the run lock");
    fs::write(&ready, b"held").expect("say the lock is held");
    std::thread::sleep(std::time::Duration::from_secs(30));
}

/// [`LockProbe`] answers **true** for a run another **process** is really
/// driving, and **false** once it lets go.
///
/// `PR6-RECOV-004`. Arm (ii) is "probe that run's run.lock non-blocking; free ->
/// dead owner -> reclaim …; **held -> live owner -> never touched**", and every
/// census fixture in this slice injects a `RecordingLiveness` or a
/// `FakeOwnerLiveness`. The only assertion against the production adapter used a
/// directory with **no lock** and expected `false` — so `is_running` returning a
/// constant `false` passed the whole suite, and a constant `false` classifies
/// every live owner as dead and kills its containers.
///
/// It has to be a real second **process**, and that is not incidental:
/// `fcntl` locks are per-process, and `rundir::is_running` answers from this
/// process's own `claims` table before it opens anything. A lock taken *here*
/// would exercise the claims path and bless an adapter that consulted only
/// that — which is precisely the "reports false while foreign run B holds it"
/// shape the finding names, since a foreign run is by definition another
/// process. `rundir::tests::a_second_process_is_refused_the_run_lock` spawns a
/// child for the same reason, and this borrows its shape.
///
/// The child is started through [`host::build_command`], the crate's one
/// producer of a `std::process::Command`, so this file never names the
/// disallowed type.
///
/// Second field held constant: one directory, one probe, one process asking;
/// only whether the owner process is alive moves.
#[test]
fn the_production_lock_probe_sees_a_lock_another_process_holds() {
    let root = scratch("lock-probe-held");
    let paths =
        crate::rundir::RunPaths::with_private_root(&root, "01KZRN48A4ZK3AEDST3RJ8HMA4", &root);
    paths.create().expect("the run directories");
    let ready = root.join("held");
    let probe = super::runtime::LockProbe;

    // Before: nobody holds it.
    assert!(
        !probe.is_running(&paths.public),
        "a run nobody is driving reads as live"
    );

    let exe = std::env::current_exe().expect("test binary");
    let spec = CommandSpec::new(exe.to_string_lossy().into_owned())
        .arg("--exact")
        .arg("runner::container::tests::container_lock_probe_child_holds_the_run")
        .arg("--ignored");
    let mut child = host::build_command(&spec)
        .env("UPSTROKE_TEST_LOCK_DIR", &paths.public)
        .env("UPSTROKE_TEST_READY", &ready)
        .spawn()
        .expect("spawn the owner process");

    // Wait for it to say it has the lock rather than sleeping and hoping.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !ready.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the owner process never took the lock"
        );
        assert!(
            child.try_wait().expect("child status").is_none(),
            "the owner process ended before taking the lock"
        );
        std::thread::yield_now();
    }

    // Held, and the probe says so **and returns**: `T-CONTAINER.resume_action`
    // is "probe the owner's run.lock **non-blocking**", so this call is the one
    // a blocking implementation would never come back from. The bound is
    // generous — it is here to fail a `LockProbe` that waits, not to measure
    // one that does not.
    let started = std::time::Instant::now();
    assert!(
        probe.is_running(&paths.public),
        "a run another process is really driving reads as dead, so a foreign census would kill \
         its containers"
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "the probe took {elapsed:?} on a held lock; a census that waits on a live neighbour \
         stalls every write command"
    );

    // And the answer follows the world rather than being a constant: once the
    // owner is gone the same directory reads free. Without this half a probe
    // hard-coded to `true` would pass the assertion above.
    let _ = child.kill();
    let _ = child.wait();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while probe.is_running(&paths.public) {
        assert!(
            std::time::Instant::now() < deadline,
            "the lock was still held long after its owner died"
        );
        std::thread::yield_now();
    }
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// `PR6-CORRECTNESS-009` — the R19 view's removal converges, and fails closed
// ---------------------------------------------------------------------------

/// Every `GitView::discard` in the tree removes through the **one** retrying
/// removal.
///
/// `crash_reconstruction`: "every step idempotent and tolerant of already-gone
/// so **two concurrent reclaimers converge**". On Windows the loser of that
/// race does not get `NotFound` — a directory whose last handle has closed is
/// delete-pending and `remove_dir_all` reports `PermissionDenied` until the
/// name goes away — so a `match` that tolerates only `NotFound` refuses one of
/// two converging write commands. `DisposableDirView` had been repaired;
/// `RoleGitView`, the projection a real run mounts, had not, and every
/// concurrent-census fixture used the other one (`PR6-CORRECTNESS-009`).
///
/// This is a source census because the platform behaviour it is about cannot be
/// produced deterministically on Linux, and because "there is one removal" is a
/// claim about the tree rather than about a call. It is the shape
/// `the_view_directory_has_one_definition_in_the_tree` already uses for the
/// other half of this same seam. The behavioural halves are the two tests
/// below.
#[test]
fn every_view_discard_removes_through_the_one_racing_removal() {
    /// The out-of-line test substrate of this subtree, excluded **by name**
    /// rather than by a pattern, so a new one is a change here. Everything else
    /// is cut at its first `#[cfg(test)]` by `production_region`; these files
    /// have none, being test modules in their entirety.
    const SUBSTRATE: &[&str] = &[
        "src/runner/container/fake.rs",
        "src/runner/container/tests.rs",
        "src/runner/container/census/tests.rs",
        "src/runner/container/resolve/tests.rs",
    ];

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();
    let mut occurrences = 0;
    let mut excluded = 0;
    for path in walk(&root.join("src/runner")) {
        let relative = path
            .strip_prefix(&root)
            .expect("under the manifest")
            .to_string_lossy()
            .replace('\\', "/");
        if SUBSTRATE.contains(&relative.as_str()) {
            excluded += 1;
            continue;
        }
        let source = fs::read_to_string(&path).expect("read source");
        // Strings blanked as well: this test's own doc comment and the literal
        // it scans for would otherwise be findings about itself. Whitespace
        // flattened so a rustfmt wrap is not a false finding.
        let production =
            crate::effects::blank_comments_and_strings(&crate::effects::production_region(&source));
        let flat = production.split_whitespace().collect::<Vec<_>>().join(" ");
        let removals = flat.matches("remove_dir_all(").count();
        let authorised = flat
            .matches("racing_removal(path, || fs::remove_dir_all(path))")
            .count();
        occurrences += removals;
        if removals != authorised {
            offenders.push(format!(
                "{relative} has {removals} directory removal(s) and {authorised} of them go \
                 through `racing_removal`; a `match` on `remove_dir_all` that tolerates only \
                 `NotFound` refuses the loser of a Windows delete-pending race"
            ));
        }
    }
    assert_eq!(
        excluded,
        SUBSTRATE.len(),
        "a file named in SUBSTRATE is not in the tree, so the exclusion is stale"
    );
    assert!(
        occurrences >= 2,
        "the census found {occurrences} directory removals in the production region of \
         src/runner; it is measuring nothing"
    );
    assert!(offenders.is_empty(), "{offenders:#?}");
}

/// Discarding a view twice converges, and the second call is not an error.
///
/// The already-gone half of "two concurrent reclaimers converge", made
/// deterministic by serialising the two reclaimers rather than racing them —
/// the race itself is `census::tests::concurrent_reclaimers_converge`, and this
/// is the predicate underneath it, held against the **`RoleGitView`** the
/// concurrent fixtures do not use.
///
/// Second field held constant: the same path, the same view, the same trace in
/// both calls; only whether the directory is still there moves.
#[test]
fn discarding_a_role_view_twice_converges() {
    let trace = ContainerTrace::recording();
    let view: &dyn GitView = &super::view::RoleGitView::new(trace.clone());
    let root = scratch("role-view-twice");
    let path = root.join("views").join("upstroke-k-r-i-h");
    fs::create_dir_all(path.join("objects").join("pack")).expect("a view with depth");
    fs::write(path.join("HEAD"), b"0000\n").expect("a file in it");

    view.discard(&path).expect("the first reclaimer");
    assert!(!path.exists());
    view.discard(&path)
        .expect("the second reclaimer must converge, not refuse");
    assert!(!path.exists());
}

/// A view that genuinely **cannot** be removed refuses, and says nothing was
/// discarded.
///
/// The fail-closed half, and the test for what this repair could have got
/// wrong. The smaller fix — tolerating `PermissionDenied` outright — would make
/// a protected view report success; the census would then go on to remove the
/// intent, and admission would proceed over R19 residue that nothing can ever
/// reclaim, because the record naming it is gone. `resource_accounting` R19
/// requires orphan views reclaimed, and "reclaimed" is not "forgotten".
///
/// Constructed on Unix by clearing the parent directory's write bit, which is
/// deterministic and is not delete-pending — the two are different states and
/// only one of them is transient. Skipped under a uid that ignores the bit.
///
/// Second field held constant: the same view, the same path, the same content;
/// only the parent's permissions move.
#[test]
#[cfg(unix)]
fn a_role_view_that_cannot_be_removed_refuses_and_records_nothing() {
    use std::os::unix::fs::PermissionsExt as _;

    let trace = ContainerTrace::recording();
    let view: &dyn GitView = &super::view::RoleGitView::new(trace.clone());
    let root = scratch("role-view-protected");
    let parent = root.join("views");
    let path = parent.join("upstroke-k-r-i-h");
    fs::create_dir_all(&path).expect("the view");
    fs::write(path.join("HEAD"), b"0000\n").expect("a file in it");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o500)).expect("clear the write bit");
    if fs::remove_dir_all(&path).is_ok() {
        // Running as root, or on a filesystem that ignores the mode. Restore
        // and say so rather than asserting something that is not true here.
        let _ = fs::set_permissions(&parent, fs::Permissions::from_mode(0o755));
        return;
    }

    let error = view
        .discard(&path)
        .expect_err("a view that is still there must not be reported discarded");
    assert!(
        matches!(error, UpstrokeError::Io { .. }),
        "the refusal must carry the IO error that stopped it: {error:?}"
    );
    assert!(
        path.exists(),
        "the fixture's premise: the view is still there"
    );
    assert!(
        !trace
            .rendered()
            .iter()
            .any(|entry| entry.contains("discard")),
        "a view that was not removed was recorded as discarded, so the census would remove the \
         intent that names it and leave R19 residue nothing can reclaim: {:#?}",
        trace.rendered()
    );

    let _ = fs::set_permissions(&parent, fs::Permissions::from_mode(0o755));
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// R3b: R20 is never created by a run, and the runtime does not enforce that
// ---------------------------------------------------------------------------

/// A create whose named volume is absent is **refused before any effect**.
///
/// `PR6-ACCT-001` / `PR6-CORRECTNESS-014`. R20 is `operator_owned` and
/// `persistent_output` — "never created or pruned by a run" — in all five
/// `at_run_end` outcomes, and `docker create` does not honour it: measured
/// against `docker` 29.7.2, `--mount type=volume,source=<absent>,target=/creds`
/// **succeeds and creates an empty named volume**. Resolution inspects the
/// volumes once, before the worktree lock; a volume removed between that
/// inspection and the invocation is seen by nothing but a check at the create
/// itself.
///
/// The grid is **{which agent's volume is missing} × {how the runtime answers}**
/// and its second axis is the one a single-cell fixture misses: a runtime that
/// *will not say* whether the volume exists must refuse too, because "the
/// runtime did not answer" is not "the volume is there". Every cell holds the
/// same spec, the same intent proof and the same image; only the volume state
/// moves.
///
/// "Before any effect" is asserted on the **trace**, not inferred: no
/// `rt:create` and no `site:Create` entry at all, so the refusal precedes even
/// the funnel's `Before` phase.
#[test]
fn a_create_whose_named_volume_is_absent_is_refused_before_any_effect() {
    /// Three agents, three volume names — all distinct, so a check that
    /// inspected the wrong one is visible.
    const CREDENTIALS: &[(&str, &str)] = &[
        ("claude-code", "upstroke-creds-claude"),
        ("copilot", "upstroke-creds-copilot"),
        ("codex", "upstroke-creds-codex"),
    ];

    let mut refusals = BTreeSet::new();
    for (agent, volume) in CREDENTIALS {
        for (label, present, reachable) in [
            ("absent", false, true),
            ("present", true, true),
            ("unreachable", true, false),
        ] {
            let fixture = Fixture::new(
                &format!("volume-{agent}-{label}"),
                RUN_A,
                INCARNATION_1,
                &shell_probe(),
            );
            for (_, other) in CREDENTIALS {
                if other != volume {
                    fixture.runtime.add_volume(other);
                }
            }
            if present {
                fixture.runtime.add_volume(volume);
            }
            let mut spec = fixture.plan.spec.clone();
            spec.mounts.push(Mount::Volume {
                name: (*volume).to_owned(),
                target: format!("/upstroke/credentials/{agent}"),
                read_only: false,
            });
            if !reachable {
                // Only the volume inspection is armed: the whole daemon being
                // down is a different refusal, and this cell is about a runtime
                // that answers everything else and will not answer this.
                fixture.runtime.set_unreachable(RuntimeOp::InspectVolume);
            }

            let mut hooks = fixture.hooks();
            let written = write_intent(
                &mut hooks,
                ContainerSite::WriteIntent,
                &fixture.root,
                &fixture.plan.name,
                &fixture.plan.intent,
            )
            .expect("the intent");
            let outcome = create_container(
                &mut hooks,
                ContainerSite::Create,
                &fixture.runtime,
                &written,
                &spec,
            );

            if present && reachable {
                outcome.expect("a provisioned volume creates");
                assert!(fixture.trace.position_starting("rt:create:").is_some());
                continue;
            }

            let error = outcome.expect_err("an unprovisioned R20 volume refuses");
            let message = error.to_string();
            // **Before ANY effect, asserted first.** This fake happens to
            // refuse an absent mounted volume too, and a real daemon does the
            // opposite — it creates one — so an assertion on *who* refused
            // would be an assertion about the fixture. The ordering is the
            // claim that belongs to the engine: the create site was never
            // entered at all, which is only true of a check that runs before
            // the funnel.
            assert!(
                fixture.trace.position_starting("rt:create:").is_none(),
                "[{agent}/{label}] the runtime was asked to create: {:#?}",
                fixture.trace.rendered()
            );
            assert!(
                !fixture
                    .trace
                    .rendered()
                    .iter()
                    .any(|entry| entry == "site:Create:before"),
                "[{agent}/{label}] the funnel entered `Container.Create`: {:#?}",
                fixture.trace.rendered()
            );
            assert!(
                message.contains(volume),
                "[{agent}/{label}] the refusal does not name the volume: {message}"
            );
            assert!(
                message.contains("R20"),
                "[{agent}/{label}] the refusal does not name the row: {message}"
            );
            refusals.insert(message);
            // And nothing was created on the way past: the other two volumes
            // are still exactly the two the fixture provisioned.
            if reachable {
                assert!(!fixture.runtime.volume_present(volume).expect("reachable"));
            }
        }
    }
    assert_eq!(
        refusals.len(),
        6,
        "three agents x two refusing runtime states, each naming its own volume: a refusal that \
         did not name the volume would collapse these"
    );
}

/// The daemon really does create the volume, so the guard above is not
/// defending against a fake.
///
/// `PR6-ACCT-001`'s premise, measured rather than asserted. The engine's own
/// `create` is never reached: this drives `docker create` directly through the
/// test-only raw accessor with a volume name the daemon does not hold, and then
/// asks the daemon whether it now holds one. R20's "never created by a run" is
/// therefore an **engine** guarantee and not a runtime one, which is exactly
/// why the check lives in `create_container`.
///
/// Second field held constant: the same image and the same container name in
/// both halves; only whether the volume was provisioned first moves.
#[test]
fn real_docker_creates_an_absent_named_volume_rather_than_refusing() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_creates_an_absent_named_volume_rather_than_refusing",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let (_, image) = match gated_image(docker.as_ref()) {
        Ok(image) => image,
        Err(reason) => return no_image(&reason),
    };

    let name = "upstroke-r3b-implicit-volume";
    let volume = "upstroke-r3b-absent-credential-volume";
    let _ = docker.remove(name);
    let _ = docker.raw(
        RuntimeOp::InspectVolume,
        volume,
        &["volume", "rm", "--force", volume],
    );
    // The premise: it is not there.
    assert!(
        !docker.volume_present(volume).expect("reachable"),
        "the fixture could not clear `{volume}`, so the measurement below would be vacuous"
    );

    let created = docker.raw(
        RuntimeOp::Create,
        name,
        &[
            "create",
            "--name",
            name,
            "--mount",
            &format!("type=volume,source={volume},target=/upstroke/credentials/codex"),
            &image.id,
            "/bin/sh",
            "-c",
            "exit 0",
        ],
    );
    let now_present = docker.volume_present(volume).unwrap_or(false);
    // Clean up before asserting.
    let _ = docker.remove(name);
    let _ = docker.raw(
        RuntimeOp::InspectVolume,
        volume,
        &["volume", "rm", "--force", volume],
    );

    created.expect("`docker create` accepts a volume name it does not hold");
    assert!(
        now_present,
        "the daemon refused to create the absent volume, which would make \
         `expect_mounted_volumes_present` unnecessary — if this ever fails, re-derive \
         `PR6-ACCT-001` against this docker version before deleting anything"
    );
}

// ---------------------------------------------------------------------------
// R3b: the third answer a racing reclaimer gets
// ---------------------------------------------------------------------------

/// What `docker` 29.7.2 answers the **loser** of two overlapping removals,
/// measured on the build box.
///
/// ```text
/// $ docker create --name c alpine sh -c 'dd if=/dev/zero of=/big bs=1M count=800; sleep 5'
/// $ docker start c
/// $ for i in 1..8; do docker rm --force --volumes c & done
/// c                                                          (one winner, exit 0)
/// Error response from daemon: removal of container c is already in progress   (x7)
/// ```
///
/// Transcribed, not invented, and
/// [`real_docker_prints_the_transcribed_removal_in_progress_diagnostic`] asks
/// the live daemon the same question so the table cannot become its own oracle.
const DAEMON_REMOVAL_IN_PROGRESS: &str =
    "Error response from daemon: removal of container upstroke-c is already in progress";

/// A `docker rm` answer meaning "somebody else is already removing it" is
/// tolerated; a real failure is not.
///
/// `PR6-CONV-002`. This is the **third** state a racing reclaimer sees, and it
/// is neither "gone" nor "already stopped": `T-CONTAINER.resume_action`'s
/// "(idempotent; **concurrent reclaimers converge**)" is false without it,
/// because the loser returns an error before `rm`, the view prune and the
/// intent removal, and the write command driving it refuses instead of
/// converging. Deleting the tolerance passed the whole suite: `FakeRuntime`'s
/// `remove` cannot produce this answer at all, and the one real-Docker reclaim
/// is sequential.
///
/// [`super::settle_remove`] is a free function over the raw outcome for the
/// reason [`super::settle_stop`] is: the branch is reachable — and testable —
/// **without a daemon**, which is what makes it assertable at all on CI and on
/// the Windows guest.
///
/// The intersection: {what the daemon said} × {is it tolerable}, with the
/// tolerable answers counted as **distinct** values so three cells cannot be
/// one string repeated. The `Unreachable` cell carries tolerable *text*,
/// because a runtime that could not be reached said nothing about the
/// container.
#[test]
fn a_removal_answer_meaning_already_in_progress_is_tolerated_and_a_real_failure_is_not() {
    let failed = |detail: &str| {
        Err(RuntimeError::Failed {
            operation: RuntimeOp::Remove,
            detail: detail.to_owned(),
        })
    };

    let tolerated = [
        DAEMON_REMOVAL_IN_PROGRESS,
        DAEMON_ABSENT_ON_STOP,
        "Error response from daemon: No such object: upstroke-c",
    ];
    for detail in tolerated {
        assert_eq!(
            super::settle_remove(failed(detail)),
            Ok(()),
            "a reclaimer that arrives second must converge on `{detail}`"
        );
    }
    assert_eq!(
        tolerated.iter().collect::<BTreeSet<_>>().len(),
        3,
        "three distinct daemon answers, not one repeated"
    );
    // The clause is its own predicate, and it is not covered by absence: the
    // in-progress answer contains none of the "no such …" shapes.
    assert!(
        !super::is_absent(DAEMON_REMOVAL_IN_PROGRESS),
        "an in-progress removal is not an absent container; if `is_absent` starts covering it, \
         the tolerance below stops being an independently droppable predicate"
    );
    assert!(super::remove_already_settled(DAEMON_REMOVAL_IN_PROGRESS));
    // Case-insensitively, because a vendor that recapitalises its prose must
    // not turn a convergence into a refusal.
    assert!(super::remove_already_settled(
        &DAEMON_REMOVAL_IN_PROGRESS.to_ascii_uppercase()
    ));

    for detail in [
        "Error response from daemon: cannot remove container: upstroke-c: permission denied",
        "Error response from daemon: You cannot remove a running container upstroke-c",
    ] {
        let error = super::settle_remove(failed(detail)).expect_err("a real failure is a failure");
        assert!(!error.is_unreachable(), "{error}");
        assert_eq!(error.operation(), RuntimeOp::Remove);
    }

    let unreachable = super::settle_remove(Err(RuntimeError::Unreachable {
        operation: RuntimeOp::Remove,
        detail: DAEMON_REMOVAL_IN_PROGRESS.to_owned(),
    }))
    .expect_err("unreachable is never `already settled`");
    assert!(unreachable.is_unreachable(), "{unreachable}");

    // A `docker kill` racing a removal gets the same answer, and it is on its
    // way out either way.
    assert!(super::stop_already_settled(DAEMON_REMOVAL_IN_PROGRESS));

    // The control: a removal that simply worked.
    assert_eq!(super::settle_remove(Ok("upstroke-c\n".to_owned())), Ok(()));
}

/// The live daemon really does answer overlapping removals that way.
///
/// The oracle for [`DAEMON_REMOVAL_IN_PROGRESS`]. A transcribed diagnostic
/// checked only against the code that reads it proves nothing, so this races
/// real `docker rm` calls against a container whose removal takes long enough
/// to overlap — 800 MiB of zeroes written into its layer — and asserts that the
/// loser's verbatim stderr both (a) matches the shape the table carries and
/// (b) settles through the production tolerance.
///
/// Second field held constant: every racer issues the **same** removal against
/// the **same** container; only which one the daemon serves first moves. A run
/// in which no racer loses is reported as a skip of the measurement rather than
/// as a pass, so a machine fast enough to serialise them cannot make this
/// vacuously green.
#[test]
fn real_docker_prints_the_transcribed_removal_in_progress_diagnostic() {
    let trace = ContainerTrace::recording();
    let docker = match docker_gate(
        "real_docker_prints_the_transcribed_removal_in_progress_diagnostic",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => return skipped(&reason),
    };
    let (_, image) = match gated_image(docker.as_ref()) {
        Ok(image) => image,
        Err(reason) => return no_image(&reason),
    };

    let name = "upstroke-r3b-removal-race";
    let mut observed: Option<String> = None;
    // A few attempts: the race is real and a machine may serve one removal
    // before the others are issued.
    for _ in 0..4 {
        let _ = docker.remove(name);
        docker
            .raw(
                RuntimeOp::Create,
                name,
                &[
                    "create",
                    "--name",
                    name,
                    &image.id,
                    "/bin/sh",
                    "-c",
                    "dd if=/dev/zero of=/big bs=1M count=800 2>/dev/null; sleep 5",
                ],
            )
            .expect("created");
        docker.raw(RuntimeOp::Start, name, &["start", name]).ok();
        // Let it write, so the removal has something to tear down.
        std::thread::sleep(std::time::Duration::from_millis(1_500));

        let answers: Vec<Result<String, RuntimeError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        DockerCli::new(ContainerTrace::off()).raw(
                            RuntimeOp::Remove,
                            name,
                            &["rm", "--force", "--volumes", name],
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("a racer panicked"))
                .collect()
        });
        observed = answers.into_iter().find_map(|answer| match answer {
            Err(RuntimeError::Failed { detail, .. })
                if detail
                    .to_ascii_lowercase()
                    .contains(super::REMOVAL_IN_PROGRESS) =>
            {
                Some(detail)
            }
            _ => None,
        });
        if observed.is_some() {
            break;
        }
    }
    let _ = docker.remove(name);

    let Some(detail) = observed else {
        // Not a pass: the measurement did not happen, and this says so in the
        // same voice a missing image does.
        return no_image(
            "no `docker rm` lost the race after four rounds, so the removal-in-progress \
             diagnostic was not measured on this machine and these tests never pull (non_goals[1])",
        );
    };
    // (a) The transcribed table names the same shape the daemon printed.
    assert!(
        detail
            .to_ascii_lowercase()
            .contains(super::REMOVAL_IN_PROGRESS),
        "the daemon's answer is `{detail}`, and the transcribed shape is \
         `{}`",
        super::REMOVAL_IN_PROGRESS
    );
    assert!(
        detail.contains("removal of container"),
        "the daemon's answer changed shape: {detail}"
    );
    // (b) The production tolerance settles it.
    assert_eq!(
        super::settle_remove(Err(RuntimeError::Failed {
            operation: RuntimeOp::Remove,
            detail: detail.clone(),
        })),
        Ok(()),
        "the loser of a real removal race does not converge: {detail}"
    );
}

// ---------------------------------------------------------------------------
// R3b: the completion path releases exhaustively too
// ---------------------------------------------------------------------------

/// A release whose own cleanup fails still attempts every remaining step, and
/// says what it could not release.
///
/// `PR6-ACCT-004`. [`release`] — the "stop/rm, view removal, intent removal
/// **after completion**" half — `?`-chained its four sites, so a
/// `Container.Stop` that failed on an invocation that had **completed** skipped
/// the still-viable forced remove, the view prune and the intent removal: three
/// residues from one failure, on the ordinary path rather than on a refusal.
/// `docker rm --force` removes a running container, so `Remove` after a failed
/// `Stop` is not a wasted call.
///
/// The cleanup-fault grid already existed for the *cancel* path and the two
/// were separate implementations, so the exhaustive one was tested and the
/// fail-fast one shipped. There is now one implementation
/// ([`super::cancel_reached`]) and this is the completion path's grid over it.
///
/// | armed | container | view | intent |
/// |---|---|---|---|
/// | `Stop` | removed | pruned | removed |
/// | `Remove` | **left** | pruned | removed |
/// | `UnmountGitView` | removed | **left** | **left** (the R19 anchor) |
/// | `RemoveIntent` | removed | pruned | **left** |
///
/// Second field held constant: the launch succeeds identically in all four
/// cells — same image ids, same plan, no substitution — so what varies is only
/// which release step was armed.
#[test]
fn a_release_whose_cleanup_fails_still_attempts_every_remaining_step() {
    use crate::topology::effects::HookPhase;

    let mut messages = BTreeSet::new();
    for armed in [
        ContainerSite::Stop,
        ContainerSite::Remove,
        ContainerSite::UnmountGitView,
        ContainerSite::RemoveIntent,
    ] {
        let fixture = Fixture::new(
            &format!("release-{}", armed.name()),
            RUN_A,
            INCARNATION_1,
            &shell_probe(),
        );
        let mut hooks = fixture.hooks();
        let launched = launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan)
            .expect("the launch itself succeeds");
        // The control: everything the release has to remove is really there.
        assert!(!fixture.runtime.container_names().is_empty());
        assert!(launched.view_path.exists());
        assert!(launched.intent_path.exists());

        hooks.fail_at(EffectSiteId::Container(armed), HookPhase::Before);
        let error = release(
            &mut hooks,
            &fixture.runtime,
            &fixture.view,
            &fixture.root,
            &launched,
        )
        .expect_err("a release that could not finish must say so");
        let message = error.to_string();
        assert!(
            message.contains("could not complete every step"),
            "{armed:?}: {message}"
        );
        assert!(
            message.contains(armed.name()) || message.contains(step_phrase(armed)),
            "{armed:?}: the error does not say which step failed: {message}"
        );
        messages.insert(message.clone());

        let container_left = !fixture.runtime.container_names().is_empty();
        let view_left = launched.view_path.exists();
        let intent_left = launched.intent_path.exists();
        let expected = match armed {
            ContainerSite::Stop => (false, false, false),
            ContainerSite::Remove => (true, false, false),
            ContainerSite::UnmountGitView => (false, true, true),
            ContainerSite::RemoveIntent => (false, false, true),
            _ => unreachable!("only the four release steps are armed"),
        };
        assert_eq!(
            (container_left, view_left, intent_left),
            expected,
            "{armed:?}: exactly the armed step's residue should remain, and every other step \
             should have run anyway"
        );
        if armed == ContainerSite::UnmountGitView {
            assert!(
                message.contains("deliberately retained"),
                "the R19 anchor rule does not hold on the completion path: {message}"
            );
        }
    }
    assert_eq!(
        messages.len(),
        4,
        "four armed steps, four distinct errors — an error that did not name the step would \
         collapse these"
    );
}
