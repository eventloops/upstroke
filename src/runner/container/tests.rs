//! Extended notes: `docs/internals/runner/container/tests.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
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
    PS_FORMAT, PS_LABELS, RacingPause, RecordingHooks, TERMINATION_OBSERVATIONS,
    classify_docker_failure, create_container, docker_gate, is_unreachable_diagnostic, launch,
    list_intents, mount_git_view, observe_terminated, parse_ps_output, read_intent, reclaim,
    release, remove_container, remove_intent, start_container, stop_container, unmount_git_view,
    write_intent,
};
use crate::error::UpstrokeError;
use crate::runner::{AgentId, CommandSpec, InvocationId, ProbeTarget, host};
use crate::topology::effects::{
    Adjacent, ContainerSite, DurableEvent, EffectSiteId, FaultRow, ResourceRow, SiteScope,
};

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

type RacingObserver = Box<dyn FnMut(usize, RacingPause)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RacingPerformed {
    Yielded,
    Slept(std::time::Duration),
}

thread_local! {
    static RACING_SCHEDULE: std::cell::RefCell<Vec<(usize, RacingPause, std::time::Instant)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static RACING_PERFORMED: std::cell::RefCell<Vec<(usize, RacingPerformed)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static RACING_OBSERVER: std::cell::RefCell<Option<RacingObserver>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn note_racing_performed(performed: RacingPerformed) {
    let after = RACING_SCHEDULE.with(|schedule| {
        schedule
            .try_borrow()
            .ok()
            .and_then(|schedule| schedule.last().map(|(failed, _, _)| *failed))
            .unwrap_or(0)
    });
    RACING_PERFORMED.with(|performed_log| {
        if let Ok(mut performed_log) = performed_log.try_borrow_mut() {
            performed_log.push((after, performed));
        }
    });
}

pub(super) fn note_racing_attempt(failed: usize, pause: RacingPause) {
    RACING_SCHEDULE.with(|schedule| {
        if let Ok(mut schedule) = schedule.try_borrow_mut() {
            schedule.push((failed, pause, std::time::Instant::now()));
        }
    });
    RACING_OBSERVER.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            if let Some(observer) = slot.as_mut() {
                observer(failed, pause);
            }
        }
    });
}

#[cfg(windows)]
struct RacingObservation {
    _not_send: std::marker::PhantomData<*const ()>,
}

#[cfg(windows)]
impl RacingObservation {
    fn schedule(&self) -> Vec<(usize, RacingPause)> {
        RACING_SCHEDULE.with(|schedule| {
            schedule
                .borrow()
                .iter()
                .map(|(failed, pause, _)| (*failed, *pause))
                .collect()
        })
    }

    fn assert_every_pause_was_performed_as_decided(&self, tag: &str) {
        use super::RACING_SLEEP;

        let decided = self.schedule();
        let expected: Vec<(usize, RacingPerformed)> = decided
            .iter()
            .filter_map(|(failed, pause)| match pause {
                RacingPause::Yield => Some((*failed, RacingPerformed::Yielded)),
                RacingPause::Sleep => Some((*failed, RacingPerformed::Slept(RACING_SLEEP))),
                RacingPause::Done => None,
            })
            .collect();
        RACING_PERFORMED.with(|performed| {
            assert_eq!(
                *performed.borrow(),
                expected,
                "[{tag}] what the performer asked of the thread after each failure must be what \
                 the schedule decided: a yield, a sleep of RACING_SLEEP, or nothing after the last"
            );
        });
    }

    fn assert_every_sleep_was_slept(&self, tag: &str) {
        use super::RACING_SLEEP;

        RACING_SCHEDULE.with(|schedule| {
            let entries = schedule.borrow();
            for pair in entries.windows(2) {
                let [(failed, pause, at), (_, _, next)] = pair else {
                    continue;
                };
                if *pause == RacingPause::Sleep {
                    let gap = next.duration_since(*at);
                    assert!(
                        gap >= RACING_SLEEP,
                        "[{tag}] failure {failed} was to be followed by a {RACING_SLEEP:?} \
                         sleep and the next attempt came {gap:?} later"
                    );
                }
            }
        });
    }
}

#[cfg(windows)]
impl Drop for RacingObservation {
    fn drop(&mut self) {
        RACING_OBSERVER.with(|slot| {
            if let Ok(mut slot) = slot.try_borrow_mut() {
                *slot = None;
            }
        });
    }
}

#[cfg(windows)]
fn observe_racing_attempts(observer: RacingObserver) -> RacingObservation {
    RACING_SCHEDULE.with(|schedule| schedule.borrow_mut().clear());
    RACING_PERFORMED.with(|performed| performed.borrow_mut().clear());
    RACING_OBSERVER.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            *slot = Some(observer);
        }
    });
    RacingObservation {
        _not_send: std::marker::PhantomData,
    }
}

fn expected_racing_schedule(failures: usize) -> Vec<(usize, RacingPause)> {
    use super::{RACING_ACCESS_ATTEMPTS, RACING_YIELD_ATTEMPTS};

    let yields = (1..=RACING_YIELD_ATTEMPTS).map(|failed| (failed, RacingPause::Yield));
    let sleeps = (RACING_YIELD_ATTEMPTS + 1..RACING_ACCESS_ATTEMPTS)
        .map(|failed| (failed, RacingPause::Sleep));
    let done = std::iter::once((RACING_ACCESS_ATTEMPTS, RacingPause::Done));
    yields.chain(sleeps).chain(done).take(failures).collect()
}

#[test]
fn the_racing_pause_is_sixteen_yields_then_forty_seven_sleeps_and_nothing_after_the_last() {
    use super::{RACING_ACCESS_ATTEMPTS, RACING_YIELD_ATTEMPTS, racing_pause_after};

    let schedule: Vec<_> = (1..=RACING_ACCESS_ATTEMPTS)
        .map(|failed| (failed, racing_pause_after(failed)))
        .collect();
    assert_eq!(schedule, expected_racing_schedule(RACING_ACCESS_ATTEMPTS));
    let count = |wanted: RacingPause| {
        schedule
            .iter()
            .filter(|(_, pause)| *pause == wanted)
            .count()
    };
    assert_eq!(
        (
            count(RacingPause::Yield),
            count(RacingPause::Sleep),
            count(RacingPause::Done)
        ),
        (16, 47, 1)
    );
    assert_eq!(RACING_YIELD_ATTEMPTS, 16);
    assert_eq!(RACING_ACCESS_ATTEMPTS, 64);
    assert_eq!(
        schedule.last(),
        Some(&(RACING_ACCESS_ATTEMPTS, RacingPause::Done)),
        "no attempt follows the last failure, so nothing may sleep after it"
    );
    assert_eq!(
        racing_pause_after(RACING_ACCESS_ATTEMPTS + 1),
        RacingPause::Done
    );
}

const REPO_KEY: &str = "0123456789abcdef";
const RUN_A: &str = "01KZRN48A4ZK3AEDST3RJ8HMA4";
const RUN_B: &str = "01KZS7R0V1ZD6MC290MG350QXF";
const INCARNATION_1: &str = "01KZTAAAAAAAAAAAAAAAAAAAAA";
const INCARNATION_2: &str = "01KZTBBBBBBBBBBBBBBBBBBBBB";

const IMAGE_ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const OTHER_IMAGE_ID: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const IMAGE_REFERENCE: &str = "ghcr.io/example/upstroke-runner:v1";
const MANIFEST_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const POLICY_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

fn shell_probe() -> InvocationId {
    InvocationId::probe(ProbeTarget::Shell, 0).expect("the shell probe identity")
}

fn agent_probe() -> InvocationId {
    InvocationId::probe(ProbeTarget::Agent(AgentId::new("claude-code")), 0)
        .expect("the agent probe identity")
}

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

fn name_for(run: &str, incarnation: &str, invocation: &InvocationId) -> ContainerName {
    ContainerName::new(REPO_KEY, run, incarnation, invocation).expect("a container name")
}

fn labels_for(root: &Path, record: &ContainerIntent) -> BTreeMap<String, String> {
    record.labels(root)
}

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

fn skipped(reason: &str) {
    assert_eq!(
        reason,
        absent_reason(),
        "a Docker-gated test skipped for a reason the gate does not know about"
    );
}

fn no_image(reason: &str) {
    assert!(reason.contains("never pull"), "{reason}");
    assert!(
        std::env::var_os(super::fake::REQUIRE_DOCKER).is_none(),
        "{} is set and a gated test found no usable image: {reason}",
        super::fake::REQUIRE_DOCKER
    );
}

fn at(trace: &ContainerTrace, needle: &str) -> usize {
    trace.position(needle).unwrap_or_else(|| {
        panic!(
            "`{needle}` is not in the trace, which is {:#?}",
            trace.rendered()
        )
    })
}

#[test]
fn a_pre_clean_refuses_every_name_a_concurrent_run_could_also_ask_for() {
    const RUN: &str = "01KZTPRECLEAN0000000000000";
    const INCARNATION: &str = "01KZTPRECLEANINC0000000000";
    const STRANGERS: &str = "cccccccccccccccc";

    let invocation = shell_probe();
    let mine = ContainerName::new(super::fake::slot_repo_key(), RUN, INCARNATION, &invocation)
        .expect("a container name");
    let theirs =
        ContainerName::new(STRANGERS, RUN, INCARNATION, &invocation).expect("a container name");

    assert_ne!(
        mine, theirs,
        "this slot's key is `{STRANGERS}`, so the two halves of this test are the same name and \
         it asserts nothing"
    );
    assert!(
        super::fake::unscoped_names(&[&mine]).is_empty(),
        "a name carrying this slot's own repo key was refused, so the pre-clean can no longer \
         reclaim the residue of a killed run in this slot — the only thing it is for"
    );
    assert_eq!(
        super::fake::unscoped_names(&[&mine, &theirs]),
        vec![theirs.as_str().to_owned()],
        "the refusal must name exactly the name it refused: a report that says only `some name` \
         sends the reader to the wrong caller"
    );
}

#[test]
fn a_pre_clean_of_a_strangers_name_refuses_before_it_reclaims_anything() {
    const RUN: &str = "01KZTPRECLEAN0000000000000";
    const INCARNATION: &str = "01KZTPRECLEANINC0000000000";
    const STRANGERS: &str = "cccccccccccccccc";

    let trace = ContainerTrace::recording();
    let runtime = FakeRuntime::new(trace.clone());
    let view = DisposableDirView::new(ContainerTrace::off());
    let root = scratch("preclean-refusal");
    let theirs =
        ContainerName::new(STRANGERS, RUN, INCARNATION, &shell_probe()).expect("a container name");

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::fake::preclean_names(&runtime, &view, &root, &[&theirs]);
    }))
    .expect_err("the pre-clean accepted a name built from another slot's repo key");
    std::panic::set_hook(hook);

    let message = refused
        .downcast_ref::<String>()
        .map_or_else(String::new, Clone::clone);
    assert!(
        message.contains("is not this build slot's") && message.contains(theirs.as_str()),
        "the refusal must say what it refused and why: {message}"
    );
    assert!(
        trace.rendered().is_empty(),
        "the pre-clean reached the runtime before refusing, so a stranger's live container was \
         already killed by the time the rule fired: {:#?}",
        trace.rendered()
    );
}

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

    let without = runtime
        .image_by_id(OTHER_IMAGE_ID)
        .expect("reachable")
        .expect("present");
    assert_eq!(without.digest, None);

    assert_eq!(runtime.image_by_reference("ghcr.io/nobody:v9"), Ok(None));
    assert_eq!(runtime.image_by_id("sha256:absent"), Ok(None));
}

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

    let honest = runtime.create(&spec).expect("created");
    assert_eq!(honest.reported_image_id, IMAGE_ID);
    runtime.remove(&spec.name).expect("removed");

    runtime.substitute_reported_image_id(&spec.name, OTHER_IMAGE_ID);
    let substituted = runtime.create(&spec).expect("created");
    assert_eq!(substituted.reported_image_id, OTHER_IMAGE_ID);
    assert_ne!(
        substituted.reported_image_id, spec.image_id,
        "the reported id and the requested id are separate inputs; if this ever \
         becomes impossible, every image-verification test in this slice is vacuous"
    );

    let held = runtime.container(&spec.name).expect("held");
    assert_eq!(held.requested_image_id, IMAGE_ID);
    assert_eq!(held.reported_image_id, OTHER_IMAGE_ID);
}

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

#[test]
fn the_availability_toggle_is_per_operation_so_ps_can_answer_while_inspect_cannot() {
    let runtime = FakeRuntime::new(ContainerTrace::recording());
    runtime.add_image(IMAGE_ID, None);
    runtime.set_unreachable(RuntimeOp::InspectImageById);

    assert_eq!(
        runtime
            .containers_with_label(LABEL_PRIVATE_ROOT, "/srv/private")
            .expect("ps is reachable"),
        Vec::new()
    );
    let error = runtime
        .image_by_id(IMAGE_ID)
        .expect_err("inspect is unreachable");
    assert!(error.is_unreachable());
    assert_eq!(error.operation(), RuntimeOp::InspectImageById);

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
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(incarnations.len(), 2, "{incarnations:?}");
    assert_eq!(invocations.len(), 2, "{invocations:?}");
    let pairs: BTreeSet<(&str, &str)> = found
        .iter()
        .filter_map(|c| Some((c.label(LABEL_RUN)?, c.label(LABEL_INCARNATION)?)))
        .collect();
    assert_eq!(pairs.len(), 3, "three distinct (run, incarnation) pairs");
}

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

    let probe = super::runtime::LockProbe;
    assert!(
        !probe.is_running(&scratch("liveness")),
        "a directory with no run.lock has no live owner"
    );
}

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
    let r19 = EXPECTED.iter().filter(|e| e.1 == ResourceRow::R19).count();
    assert_eq!(r19, 2);
    assert_eq!(EXPECTED.len() - r19, 6);

    for site in ContainerSite::ALL {
        assert_eq!(site.sub_effects(), &[], "{}", site.name());
        assert_eq!(site.residue_classes(), &[], "{}", site.name());
        assert_eq!(site.residue_elements(), &[], "{}", site.name());
        assert!(!site.is_read_only(), "{}", site.name());
    }
}

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

    let raw = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/internals/runner/container.md"),
    )
    .expect("the funnel's notes");
    let region = {
        let start = raw
            .find("## `pub enum OrphanWindow {`")
            .expect("the enum's section in the notes");
        let end = raw
            .find("## `impl OrphanWindow`")
            .expect("the impl's first section in the notes");
        assert!(start < end);
        &raw[start..end]
    };
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

    let unix_has_a_reaper = unix_said.contains("cleanup reaper");
    let windows_has_a_reaper = windows_said.contains("cleanup reaper");
    assert!(
        unix_has_a_reaper != windows_has_a_reaper,
        "exactly one platform has a reaper; `os_matrix` says Windows has none. \
         unix: `{unix_said}` / windows: `{windows_said}`"
    );

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
    assert!(!unix_said.contains("no reaper"), "{unix_said}");
    assert!(
        windows_said.contains("next write-command start"),
        "{windows_said}"
    );

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

        match own_site {
            ContainerSite::WriteIntent => {}
            ContainerSite::Create => {}
            ContainerSite::Start => seed(Liveness::Exited),
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
            assert_eq!(
                trace.rendered(),
                Vec::<String>::new(),
                "{} under `Container.{}` refused and something still happened",
                own_site.name(),
                wrong.name()
            );
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

#[test]
fn a_hook_armed_at_a_phase_fails_the_funnel_at_that_phase() {
    let fixture = Fixture::new("hook-arm", RUN_A, INCARNATION_1, &shell_probe());
    let name = fixture.plan.name.clone();

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

    assert!(
        !record.run_dir.starts_with("/srv/private"),
        "the public run directory and the private root are different values, so \
         a label that took one for the other is visible"
    );
}

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

    assert!(ContainerName::from_parts("a-b", "c", INCARNATION_1, "d").is_err());
    assert!(ContainerName::from_parts("a", "b-c", INCARNATION_1, "d").is_err());
}

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
    assert!(
        refusals.len() >= hostile.len(),
        "the refusals collapse to {} distinct messages for {} hostile values",
        refusals.len(),
        hostile.len()
    );

    let at_limit = "a".repeat(intent::MAX_COMPONENT_LEN);
    let over = "a".repeat(intent::MAX_COMPONENT_LEN + 1);
    assert!(ContainerName::from_parts(&at_limit, RUN_A, INCARNATION_1, "d").is_ok());
    assert!(ContainerName::from_parts(&over, RUN_A, INCARNATION_1, "d").is_err());
}

#[test]
fn probe_name_reuse_across_incarnations_never_collides() {
    let root = scratch("probe-reuse");
    let mut names = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for incarnation in [INCARNATION_1, INCARNATION_2] {
        for invocation in [shell_probe(), agent_probe()] {
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

    assert!(launched.intent_path.exists());
    assert_eq!(
        read_intent(&launched.intent_path).expect("read").run_id,
        RUN_A
    );
}

#[test]
fn container_created_from_recorded_image_id_and_verified() {
    let fixture = Fixture::new("created-from-id", RUN_A, INCARNATION_1, &shell_probe());
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
    let created = at(&fixture.trace, &format!("rt:create:{}", fixture.plan.name));
    let started = at(&fixture.trace, &format!("rt:start:{}", fixture.plan.name));
    assert!(created < started);
}

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
    let mounted = at(&fixture.trace, "site:MountGitView:after");
    let pruned = at(&fixture.trace, "site:UnmountGitView:after");
    assert!(
        mounted < pruned,
        "the view is mounted and then pruned by the cancel: {rendered:#?}"
    );

    assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
    assert!(
        !fixture.plan.view.path.exists(),
        "the refused launch left its R19 view behind"
    );
    assert!(!fixture.plan.name.intent_path(&fixture.root).exists());
    assert_eq!(list_intents(&fixture.root).expect("scan").len(), 0);
}

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

    let dir_synced = fixture
        .trace
        .position_starting("durable:dir-synced:")
        .unwrap_or_else(|| panic!("no directory barrier in {rendered:#?}"));
    assert!(
        dir_synced < mounted,
        "the intent is synced before anything else happens: {rendered:#?}"
    );
}

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
    assert!(!launched.view_path.exists(), "the view is pruned");
    assert!(!launched.intent_path.exists(), "the intent is removed");
    assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
}

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

        assert!(message.contains(OTHER_IMAGE_ID), "{armed:?}: {message}");
        assert!(message.contains(IMAGE_ID), "{armed:?}: {message}");
        assert!(message.contains("before start"), "{armed:?}: {message}");
        assert!(message.contains("INV-23"), "{armed:?}: {message}");
        assert!(
            message.contains("could not release everything"),
            "{armed:?}: {message}"
        );
        assert!(
            message.contains(armed.name()) || message.contains(step_phrase(armed)),
            "{armed:?}: the refusal does not say which step failed: {message}"
        );
        messages.insert(message.clone());

        assert!(
            fixture.trace.position_starting("rt:start:").is_none(),
            "{armed:?}: the container was started despite the mismatch"
        );

        let container_left = !fixture.runtime.container_names().is_empty();
        let view_left = fixture.plan.view.path.exists();
        let intent_left = fixture.plan.name.intent_path(&fixture.root).exists();
        let expected = match armed {
            ContainerSite::Stop => (false, false, false),
            ContainerSite::Remove => (true, false, false),
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

fn step_phrase(site: ContainerSite) -> &'static str {
    match site {
        ContainerSite::Stop => "could not be stopped",
        ContainerSite::Remove => "could not be removed",
        ContainerSite::UnmountGitView => "R19 Git view could not be pruned",
        ContainerSite::RemoveIntent => "R26 intent record could not be removed",
        _ => "unreachable",
    }
}

const DAEMON_ALREADY_STOPPED: &str = "Error response from daemon: cannot kill container: \
     upstroke-c: container 0079320fdf5654fbf3aa45a154e4d49328c1cc1de3b1af4a6cc24540519ecede \
     is not running";
const DAEMON_ABSENT_ON_KILL: &str =
    "Error response from daemon: cannot kill container: upstroke-c: No such container: upstroke-c";
const DAEMON_ABSENT_ON_STOP: &str = "Error response from daemon: No such container: upstroke-c";

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

    for detail in [
        "Error response from daemon: cannot kill container: upstroke-c: tried to kill \
         container, but did not receive an exit event",
        "Error response from daemon: cannot stop container: upstroke-c: permission denied",
    ] {
        let error = super::settle_stop(failed(detail)).expect_err("a real failure is a failure");
        assert!(!error.is_unreachable(), "{error}");
        assert_eq!(error.operation(), RuntimeOp::Stop);
    }

    let unreachable = super::settle_stop(Err(RuntimeError::Unreachable {
        operation: RuntimeOp::Stop,
        detail: DAEMON_ALREADY_STOPPED.to_owned(),
    }))
    .expect_err("unreachable is never `already settled`");
    assert!(unreachable.is_unreachable(), "{unreachable}");

    assert_eq!(super::settle_stop(Ok("upstroke-c\n".to_owned())), Ok(()));
}

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

    reclaim(
        &mut hooks,
        &docker_like,
        &fixture.view,
        &fixture.root,
        &name,
        Some(&view_path),
    )
    .expect("B converges on a container A already killed");

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

    assert!(!view_path.exists(), "B stopped before pruning the view");
    assert!(
        !launched.intent_path.exists(),
        "B stopped before removing the intent"
    );
    assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());
}

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

#[test]
fn a_failed_operation_and_an_unreachable_one_are_different_answers() {
    let runtime = FakeRuntime::new(ContainerTrace::recording());
    runtime.add_image(IMAGE_ID, None);

    assert!(runtime.image_by_id(IMAGE_ID).expect("reachable").is_some());

    runtime.set_failing(RuntimeOp::InspectImageById);
    let failed = runtime.image_by_id(IMAGE_ID).expect_err("armed failing");
    assert!(!failed.is_unreachable(), "{failed}");
    assert_eq!(failed.operation(), RuntimeOp::InspectImageById);
    assert!(failed.to_string().contains("refused"), "{failed}");

    runtime.set_unreachable(RuntimeOp::InspectImageById);
    let unreachable = runtime
        .image_by_id(IMAGE_ID)
        .expect_err("armed unreachable");
    assert!(unreachable.is_unreachable(), "{unreachable}");
    assert!(
        unreachable.to_string().contains("cannot be reached"),
        "{unreachable}"
    );

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
    assert_eq!(
        runtime.observe("never-existed").expect("observed"),
        Liveness::Gone
    );
}

#[test]
fn the_docker_gate_refuses_an_uncounted_test_and_names_what_is_absent() {
    let reason = absent_reason();
    assert!(reason.contains(super::DOCKER_PROGRAM), "{reason}");
    assert!(reason.contains("daemon"), "{reason}");

    let unlisted = ["a", "test", "nobody", "listed"].join("_");
    let refused = std::panic::catch_unwind(|| docker_gate(&unlisted, ContainerTrace::off()));
    assert!(
        refused.is_err(),
        "the gate accepted a test that is not in DOCKER_GATED_TESTS, so a gated \
         test could exist that nothing counts"
    );
}

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
    let mut sorted = found.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    sorted.sort();
    assert_eq!(
        found.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        sorted
    );
}

#[test]
fn an_absent_containers_directory_is_an_empty_namespace() {
    let root = scratch("empty-namespace");
    assert!(!containers_dir(&root).exists());
    assert_eq!(list_intents(&root).expect("scanned"), Vec::new());
}

#[test]
fn every_container_effect_in_the_tree_goes_through_the_funnel() {
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
        if relative == "src/runner/container/fake.rs"
            || relative == "src/runner/container/tests.rs"
            || relative == "src/runner/container/exec/tests.rs"
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

fn stated_lint_level(source: &str, lint: &str) -> Option<&'static str> {
    crate::effects::lint_levels::file_level_lint_state(source, lint)
}

fn closes_the_hole(stated: Option<&str>) -> bool {
    matches!(stated, Some("allow" | "deny" | "forbid"))
}

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

#[test]
fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {
    const GOVERNED: [&str; 3] = [
        "clippy::disallowed_methods",
        "clippy::disallowed_types",
        "clippy::disallowed_macros",
    ];
    const FUNNELS: [&str; 5] = [
        "src/runner/container.rs",
        "src/agent/proc.rs",
        "src/runner/host.rs",
        "src/rundir.rs",
        "src/workspace_manager.rs",
    ];
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut children: Vec<PathBuf> = Vec::new();
    let mut with_children = 0_usize;
    for funnel in FUNNELS {
        let directory = root.join(funnel.strip_suffix(".rs").unwrap_or(funnel));
        if !directory.is_dir() {
            continue;
        }
        with_children += 1;
        let arm = walk(&directory);
        assert!(
            !arm.is_empty(),
            "`{funnel}` has a child directory and the walk returned no file from it, so \
             this funnel's arm of the census is measuring nothing"
        );
        children.extend(arm);
    }
    children.sort();
    assert_eq!(
        with_children,
        FUNNELS.len(),
        "only {with_children} of the {} funnels have an out-of-line child directory; the \
         census is scoped to fewer module trees than the list names",
        FUNNELS.len()
    );
    assert!(
        children.len() >= 9,
        "the walk found only {} child modules; the census is measuring nothing",
        children.len()
    );
    assert!(
        children.contains(&root.join("src/agent/proc/test_support/readiness.rs")),
        "`src/agent/proc/test_support/readiness.rs` is not in the census domain: {children:#?}"
    );
    for child in [
        "src/rundir/classify.rs",
        "src/rundir/discovery.rs",
        "src/rundir/names.rs",
        "src/rundir/ownership.rs",
        "src/rundir/retention.rs",
    ] {
        assert!(
            children.contains(&root.join(child)),
            "the RunDir funnel's child `{child}` is not in the census domain: {children:#?}"
        );
    }
    for child in [
        "src/workspace_manager/containment.rs",
        "src/workspace_manager/fixture.rs",
        "src/workspace_manager/hooks.rs",
        "src/workspace_manager/naming.rs",
        "src/workspace_manager/object.rs",
        "src/workspace_manager/parsers.rs",
        "src/workspace_manager/residue.rs",
        "src/workspace_manager/snapshot_ref.rs",
        "src/workspace_manager/tests.rs",
        "src/workspace_manager/worktree.rs",
    ] {
        assert!(
            children.contains(&root.join(child)),
            "the schema-4 workspace funnel's child `{child}` is not in the census \
             domain: {children:#?}"
        );
    }

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
            let stated = stated_lint_level(&source, lint);
            if stated == Some("allow") && !allowlist_records(&relative, lint) {
                unlisted.push(format!(
                    "{relative} allows `{lint}` and effects/allowlist.toml does not record it"
                ));
            } else if !closes_the_hole(stated) {
                missing.push(match stated {
                    None => format!("{relative} states no file-module-level level for `{lint}`"),
                    Some(weaker) => format!(
                        "{relative} states `{weaker}` for `{lint}`, which is not a build error"
                    ),
                });
            }
        }
    }
    assert!(
        missing.is_empty(),
        "a child of a Process or Container funnel inherits its allow instead of stating a \
         level of its own, which is `PR6-LANEF-004` reopening:\n{missing:#?}"
    );
    assert!(unlisted.is_empty(), "{unlisted:#?}");
    assert_eq!(cells, children.len() * 3);

    for funnel in FUNNELS {
        let source = fs::read_to_string(root.join(funnel)).expect("a funnel module");
        for lint in GOVERNED {
            assert_eq!(
                stated_lint_level(&source, lint),
                Some("allow"),
                "the funnel `{funnel}` no longer allows `{lint}`"
            );
            assert!(allowlist_records(funnel, lint));
        }
    }

    let readiness = fs::read_to_string(root.join("src/agent/proc/test_support/readiness.rs"))
        .expect("the readiness child");
    for lint in GOVERNED {
        assert_eq!(
            stated_lint_level(&readiness, lint),
            Some("deny"),
            "`readiness.rs` no longer denies `{lint}` at file scope"
        );
    }
    assert!(allowlist_records(
        "src/agent/proc/test_support/readiness.rs",
        GOVERNED[0]
    ));
    for denied in [GOVERNED[1], GOVERNED[2]] {
        assert!(
            !allowlist_records("src/agent/proc/test_support/readiness.rs", denied),
            "the readiness row records `{denied}`, which the file denies without excepting \
             any site of it"
        );
    }

    assert!(closes_the_hole(Some("deny")));
    assert!(closes_the_hole(Some("forbid")));
    assert!(closes_the_hole(Some("allow")));
    assert!(!closes_the_hole(Some("warn")));
    assert!(!closes_the_hole(Some("expect")));
    assert!(!closes_the_hole(None));

    assert_eq!(
        stated_lint_level("fn go() {}\n", GOVERNED[0]),
        None,
        "a file that states nothing must read as stating nothing"
    );
    for item_level in [
        "#[deny(clippy::disallowed_methods)]\nfn go() {}\n",
        "#[allow(clippy::disallowed_methods)]\nfn go() {}\n",
        "fn go() {\n    #[deny(clippy::disallowed_methods)]\n    let _ = ();\n}\n",
        "mod inner {\n    #![deny(clippy::disallowed_methods)]\n}\n",
    ] {
        assert_eq!(
            stated_lint_level(item_level, GOVERNED[0]),
            None,
            "an attribute below the file module read as the file's own level: {item_level:?}"
        );
    }
    assert_eq!(
        stated_lint_level(
            "fn go() {}\n#![deny(clippy::disallowed_methods)]\n",
            GOVERNED[0]
        ),
        None,
        "an inner attribute after the first item is not a file-module-level one"
    );
    assert_eq!(
        stated_lint_level("#![forbid(clippy::disallowed_methods)]\n", GOVERNED[0]),
        Some("forbid")
    );
    assert_eq!(
        stated_lint_level("#![warn(clippy::disallowed_methods)]\n", GOVERNED[0]),
        Some("warn")
    );
    assert_eq!(
        stated_lint_level("#![deny(disallowed_methods)]\n", GOVERNED[0]),
        Some("deny")
    );
    assert_eq!(
        stated_lint_level(
            "//! docs\n#![allow(clippy::too_many_arguments)]\n#![deny(clippy::disallowed_types)]\n",
            GOVERNED[1]
        ),
        Some("deny")
    );
    assert_eq!(
        stated_lint_level("#![allowance(clippy::disallowed_methods)]\n", GOVERNED[0]),
        None,
        "a longer attribute name that merely starts with a level is not that level"
    );
    for (fixture, lint, expected) in [
        (
            "#![deny(clippy::disallowed_methods)]\n",
            GOVERNED[0],
            Some("deny"),
        ),
        (
            "#![forbid(clippy::disallowed_types)]\n",
            GOVERNED[1],
            Some("forbid"),
        ),
        (
            "//! docs\n#![allow(clippy::disallowed_methods)]\n#![deny(clippy::disallowed_macros)]\n",
            GOVERNED[2],
            Some("deny"),
        ),
        (
            "#[deny(clippy::disallowed_methods)]\nfn go() {}\n",
            GOVERNED[0],
            None,
        ),
        (
            "fn go() {}\n#![deny(clippy::disallowed_methods)]\n",
            GOVERNED[0],
            None,
        ),
    ] {
        assert_eq!(stated_lint_level(fixture, lint), expected, "{fixture:?}");
        assert_eq!(
            stated_lint_level(&fixture.replace('\n', "\r\n"), lint),
            expected,
            "CRLF changed the answer for {fixture:?}"
        );
    }
    for path in [
        "src/agent/proc/test_support/readiness.rs",
        "src/runner/container/env.rs",
    ] {
        let text = fs::read_to_string(root.join(path)).expect("a funnel child");
        for lint in GOVERNED {
            assert_eq!(
                stated_lint_level(&text, lint),
                stated_lint_level(&text.replace('\n', "\r\n"), lint),
                "{path} reads differently under CRLF for `{lint}`"
            );
        }
    }
    assert_eq!(
        stated_lint_level(
            "//! #![allow(clippy::disallowed_methods)]\nfn go() {}\n",
            GOVERNED[0]
        ),
        None,
        "a level quoted in a doc comment is not a level"
    );
    assert_eq!(
        stated_lint_level(
            "const P: &str = \"#![allow(clippy::disallowed_methods)]\";\n",
            GOVERNED[0]
        ),
        None,
        "a level quoted in a string literal is not a level"
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
    assert!(allowlist_records(
        "src/runner/container/tests.rs",
        GOVERNED[0]
    ));
    assert!(!allowlist_records(
        "src/runner/container/tests.rs",
        GOVERNED[1]
    ));
    assert!(!allowlist_records(
        "src/agent/proc/test_support/readiness/nowhere.rs",
        GOVERNED[0]
    ));
}

fn collapsed_prose(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let line = line.trim();
        let line = line
            .strip_prefix("//!")
            .or_else(|| line.strip_prefix("///"))
            .or_else(|| line.strip_prefix("//"))
            .unwrap_or(line);
        let line = line.strip_suffix('\\').unwrap_or(line);
        for wordish in line.split_whitespace() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(wordish);
        }
    }
    out
}

fn denied_call_needles() -> Vec<(String, Vec<String>)> {
    let denylist = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(crate::effects::CLIPPY_TOML),
    )
    .expect("clippy.toml");
    let table = denylist
        .split("disallowed-methods = [")
        .nth(1)
        .expect("the disallowed-methods table")
        .split("\n]")
        .next()
        .expect("the table ends");
    let mut needles = Vec::new();
    for line in table.lines() {
        let Some(rest) = line.split("path = \"").nth(1) else {
            continue;
        };
        let Some(path) = rest.split('"').next() else {
            continue;
        };
        let segments: Vec<&str> = path.split("::").collect();
        let Some(last) = segments.last() else {
            continue;
        };
        let mut forms = Vec::new();
        if let Some(penult) = segments.len().checked_sub(2).map(|at| segments[at]) {
            forms.push(format!("{penult}::{last}("));
            if penult.starts_with(char::is_uppercase) {
                forms.push(format!(".{last}("));
            }
        } else {
            forms.push(format!("{last}("));
        }
        needles.push((path.to_owned(), forms));
    }
    assert!(
        needles.len() > 50,
        "only {} denied methods were read out of clippy.toml",
        needles.len()
    );
    needles
}

fn denied_calls_in(source: &str) -> BTreeMap<String, usize> {
    let code = crate::effects::blank_comments_and_strings(source);
    let mut found = BTreeMap::new();
    for (path, forms) in denied_call_needles() {
        let count: usize = forms.iter().map(|form| code.matches(form).count()).sum();
        if count > 0 {
            found.insert(path, count);
        }
    }
    found
}

#[test]
fn the_readiness_allowance_names_the_paths_it_is_written_against() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let readiness = root.join("src/agent/proc/test_support/readiness.rs");
    let source = fs::read_to_string(&readiness).expect("the readiness module");

    let expected: BTreeMap<String, usize> = [
        ("std::fs::File::create_new", 1),
        ("std::io::Write::write_all", 1),
        ("std::io::Write::flush", 1),
        ("std::fs::rename", 1),
        ("std::fs::remove_file", 2),
    ]
    .into_iter()
    .map(|(path, count)| ((*path).to_owned(), count))
    .collect();

    let found = denied_calls_in(&source);
    assert_eq!(
        found, expected,
        "the denied primitives readiness.rs reaches are not the ones its allowlist row is \
         written against"
    );
    assert_eq!(found.len(), 5, "five distinct denied paths");
    assert_eq!(
        found.values().sum::<usize>(),
        6,
        "six sites across those five paths"
    );

    assert_eq!(
        denied_calls_in(&source.replace('\n', "\r\n")),
        found,
        "the denied-call census answers differently under CRLF"
    );

    let allowlist = fs::read_to_string(root.join("effects/allowlist.toml")).expect("the allowlist");
    let row = allowlist
        .split("[[")
        .find(|block| block.contains("path = \"src/agent/proc/test_support/readiness.rs\""))
        .expect("the readiness row");
    let notes =
        fs::read_to_string(root.join("docs/internals/agent/proc/test_support/readiness.md"))
            .expect("the readiness notes");
    for (record, text, phrase) in [
        (
            "effects/allowlist.toml",
            row,
            "FIVE DISTINCT DENIED PATHS ACROSS SIX SITES",
        ),
        (
            "docs/internals/agent/proc/test_support/readiness.md",
            notes.as_str(),
            "five distinct denied paths across six sites",
        ),
    ] {
        for spelling in [text.to_owned(), text.replace('\n', "\r\n")] {
            assert!(
                collapsed_prose(&spelling).contains(phrase),
                "{record} no longer states `{phrase}`"
            );
        }
    }

    let collapsed_row = collapsed_prose(row);
    for path in expected.keys() {
        assert!(
            collapsed_row.contains(path.as_str()),
            "the readiness allowlist row does not name `{path}`"
        );
    }
}

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
    let mut called: BTreeSet<String> = BTreeSet::new();
    let stripped = crate::effects::blank_comments(&sources);
    let opener = "docker_gate(";
    let mut rest = stripped.as_str();
    while let Some(index) = rest.find(opener) {
        rest = &rest[index + opener.len()..];
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

const PREFERRED_IMAGES: &[&str] = &["alpine:3.20", "busybox:latest", "debian:stable-slim"];

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

    docker
        .stop(name, StopMode::Kill)
        .expect("a reclaimer arriving second converges on an already-stopped container");

    docker.remove(name).expect("removed");
    assert_eq!(docker.observe(name).expect("reachable"), Liveness::Gone);
}

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
    assert!(
        docker.volume_present(&anonymous).expect("reachable"),
        "the fixture did not create an anonymous volume, so the assertion below \
         would hold vacuously"
    );

    docker.remove(name).expect("removed");

    let leaked = docker.volume_present(&anonymous).expect("reachable");
    let operators_survived = docker.volume_present(named).expect("reachable");
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

#[test]
fn a_label_value_carrying_a_comma_or_an_equals_is_read_whole() {
    let sep = PS_FIELD_SEPARATOR;
    let values = [
        "/repo/.upstroke/runs/B",
        "/repo/a%2Cb/.upstroke/runs/B",
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

    let empty = format!("c{sep}/srv/private{sep}RUNB{sep}{sep}INC2{sep}p.shell.o0");
    let found = parse_ps_output(&empty).expect("well formed");
    assert_eq!(found[0].label(LABEL_RUN_DIR), None);
    assert!(parse_ps_output("\n   \n").expect("blank").is_empty());
}

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

const ANSWERED_STDERR: &[&str] = &[
    "Error response from daemon: No such container: no-such-container-xyz",
    "Error response from daemon: cannot kill container: c: container 9f is not running",
    "Error response from daemon: conflict: unable to remove repository reference",
    "invalid reference format",
    "docker: 'nope' is not a docker command.",
    "Error response from daemon: pull access denied for private/image, repository does not exist",
];

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
    let racing =
        "Error response from daemon: cannot kill container: c: container 9f is not running";
    assert!(super::stop_already_settled(racing));
    assert!(!is_unreachable_diagnostic(racing));
}

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
        read_only_root: true,
    };
    docker.create(&spec).expect("create the labelled container");

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
        let output = host::test_support::build_command(&spec)
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

#[test]
#[ignore = "spawned as a subprocess by the_production_lock_probe_sees_a_lock_another_process_holds"]
fn container_lock_probe_child_holds_the_run() {
    let public = PathBuf::from(std::env::var("UPSTROKE_TEST_LOCK_DIR").expect("run dir"));
    let ready = PathBuf::from(std::env::var("UPSTROKE_TEST_READY").expect("readiness path"));
    let _held = crate::rundir::RunLock::acquire(&public).expect("the child takes the run lock");
    fs::write(&ready, b"held").expect("say the lock is held");
    std::thread::sleep(std::time::Duration::from_secs(30));
}

#[test]
fn the_production_lock_probe_sees_a_lock_another_process_holds() {
    let root = scratch("lock-probe-held");
    let paths =
        crate::rundir::RunPaths::with_private_root(&root, "01KZRN48A4ZK3AEDST3RJ8HMA4", &root);
    paths.create().expect("the run directories");
    let ready = root.join("held");
    let probe = super::runtime::LockProbe;

    assert!(
        !probe.is_running(&paths.public),
        "a run nobody is driving reads as live"
    );

    let exe = std::env::current_exe().expect("test binary");
    let spec = CommandSpec::new(exe.to_string_lossy().into_owned())
        .arg("--exact")
        .arg("runner::container::tests::container_lock_probe_child_holds_the_run")
        .arg("--ignored");
    let mut child = host::test_support::build_command(&spec)
        .env("UPSTROKE_TEST_LOCK_DIR", &paths.public)
        .env("UPSTROKE_TEST_READY", &ready)
        .spawn()
        .expect("spawn the owner process");

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

#[test]
fn every_view_discard_removes_through_the_one_racing_removal() {
    const SUBSTRATE: &[&str] = &[
        "src/runner/container/fake.rs",
        "src/runner/container/tests.rs",
        "src/runner/container/census/tests.rs",
        "src/runner/container/exec/tests.rs",
        "src/runner/container/resolve/tests.rs",
        "src/runner/host/tests.rs",
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

#[cfg(windows)]
fn windows_posix_delete_pending(path: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileDispositionInfoEx, SetFileInformationByHandle,
    };

    let handle = fs::File::options()
        .access_mode(DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .expect("open the name for deletion, as the winner's last step does");
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };
    let size = u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).expect("a small struct");
    // SAFETY: `handle` is open for the duration of the call, `disposition` is a
    // fully initialised `FILE_DISPOSITION_INFO_EX` and `size` is its size, which
    // is the contract `FileDispositionInfoEx` documents.
    let set = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle(),
            FileDispositionInfoEx,
            (&raw const disposition).cast(),
            size,
        )
    };
    assert_ne!(
        set,
        0,
        "mark the name delete-pending: {}",
        std::io::Error::last_os_error()
    );
    let error = fs::File::options()
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .expect_err("premise: while the marking handle is open the name is still present");
    assert_eq!(
        error.raw_os_error(),
        Some(5),
        "premise: the loser's own open of a delete-pending name answers ERROR_ACCESS_DENIED, \
         not {error}"
    );
    handle
}

#[cfg(windows)]
fn windows_stalled_removal_budget() -> std::time::Duration {
    use super::{RACING_ACCESS_ATTEMPTS, RACING_SLEEP, RACING_YIELD_ATTEMPTS};

    let slept = u32::try_from(RACING_ACCESS_ATTEMPTS - RACING_YIELD_ATTEMPTS - 1)
        .expect("a small attempt count");
    RACING_SLEEP * slept
}

#[cfg(windows)]
const WINDOWS_RELEASE_AFTER_FAILURES: usize = super::RACING_YIELD_ATTEMPTS + 4;

#[cfg(windows)]
fn windows_stall_then_release(pending: fs::File, work: impl FnOnce()) -> Vec<(usize, RacingPause)> {
    let mut pending = Some(pending);
    let observation = observe_racing_attempts(Box::new(move |failed, _| {
        if failed == WINDOWS_RELEASE_AFTER_FAILURES {
            drop(pending.take());
        }
    }));
    work();
    let schedule = observation.schedule();
    observation.assert_every_pause_was_performed_as_decided("stall");
    observation.assert_every_sleep_was_slept("stall");
    drop(observation);
    schedule
}

#[cfg(windows)]
fn windows_assert_converged_through_the_wait(tag: &str, schedule: &[(usize, RacingPause)]) {
    assert_eq!(
        schedule,
        expected_racing_schedule(WINDOWS_RELEASE_AFTER_FAILURES),
        "[{tag}] the loser must fail exactly until the winner's close, on the documented \
         pause schedule, and converge on the attempt after it"
    );
}

#[test]
#[cfg(windows)]
fn windows_a_view_whose_remover_stalls_delete_pending_converges_once_the_stall_ends() {
    let views: [(&str, Box<dyn GitView>); 2] = [
        (
            "disposable",
            Box::new(DisposableDirView::new(ContainerTrace::recording())),
        ),
        (
            "role",
            Box::new(super::view::RoleGitView::new(ContainerTrace::recording())),
        ),
    ];
    for (tag, view) in views {
        let root = scratch(&format!("stalled-remover-{tag}"));
        let path = root.join("views").join("upstroke-k-r-i-h");
        fs::create_dir_all(&path).expect("an orphan view, empty as the census seeds it");
        let pending = windows_posix_delete_pending(&path);

        let mut outcome = None;
        let schedule = windows_stall_then_release(pending, || outcome = Some(view.discard(&path)));

        outcome.expect("the discard ran").unwrap_or_else(|error| {
            panic!(
                "[{tag}] the loser of a removal race refused after {} failed attempts \
                     instead of waiting out a winner stalled between marking the name and \
                     closing its handle: {error}",
                schedule.len()
            )
        });
        windows_assert_converged_through_the_wait(tag, &schedule);
        assert!(
            !path.exists(),
            "[{tag}] the view is gone once the stall ends"
        );
        let _ = fs::remove_dir_all(&root);
    }
}

#[test]
#[cfg(windows)]
fn windows_a_view_held_delete_pending_past_the_budget_refuses_and_keeps_the_intent() {
    use std::time::Instant;

    use super::RACING_ACCESS_ATTEMPTS;

    let fixture = Fixture::new("held-past-budget", RUN_A, INCARNATION_1, &shell_probe());
    let mut hooks = fixture.hooks();
    let name = fixture.plan.name.clone();
    let view_path = fixture.plan.view.path.clone();
    let launched =
        launch(&mut hooks, &fixture.runtime, &fixture.view, &fixture.plan).expect("launched");
    let pending = windows_posix_delete_pending(&view_path);

    let observation = observe_racing_attempts(Box::new(|_, _| {}));
    let started = Instant::now();
    let error = reclaim(
        &mut hooks,
        &fixture.runtime,
        &fixture.view,
        &fixture.root,
        &name,
        Some(&view_path),
    )
    .expect_err("a view that is still delete-pending after the whole budget must refuse");
    let elapsed = started.elapsed();
    let schedule = observation.schedule();
    observation.assert_every_pause_was_performed_as_decided("held");
    observation.assert_every_sleep_was_slept("held");
    drop(observation);

    assert!(
        matches!(&error, UpstrokeError::Io { source, .. } if source.raw_os_error() == Some(5)),
        "the refusal carries the native error that stopped it: {error:?}"
    );
    assert_eq!(
        schedule,
        expected_racing_schedule(RACING_ACCESS_ATTEMPTS),
        "the refusal comes after exactly the bound, yields then sleeps strictly between \
         attempts, and nothing after the last"
    );
    let budget = windows_stalled_removal_budget();
    assert!(
        elapsed >= budget,
        "the refusal came after {elapsed:?}, before the {budget:?} sleep budget was spent"
    );
    assert!(
        elapsed < budget * 10,
        "the refusal came after {elapsed:?}: the wait is bounded, and this is not a bound"
    );
    assert!(
        launched.intent_path.exists(),
        "the intent is retained when the view could not be removed, so the next census can \
         finish the job rather than admit over residue nothing names"
    );
    assert!(
        !fixture
            .trace
            .rendered()
            .iter()
            .any(|entry| entry.contains("discard")),
        "a view that was not removed was recorded as discarded: {:#?}",
        fixture.trace.rendered()
    );

    drop(pending);
    reclaim(
        &mut hooks,
        &fixture.runtime,
        &fixture.view,
        &fixture.root,
        &name,
        Some(&view_path),
    )
    .expect("once the name has gone the retained intent is reclaimed by the next census");
    assert!(!view_path.exists());
    assert!(!launched.intent_path.exists());
    let _ = fs::remove_dir_all(&fixture.root);
}

#[test]
#[cfg(windows)]
fn windows_an_intent_whose_remover_stalls_delete_pending_is_read_and_removed_once_the_stall_ends() {
    let fixture = Fixture::new(
        "stalled-intent-remover",
        RUN_A,
        INCARNATION_1,
        &shell_probe(),
    );
    let mut hooks = fixture.hooks();
    let name = fixture.plan.name.clone();
    let write = |hooks: &mut RecordingHooks| {
        write_intent(
            hooks,
            ContainerSite::WriteIntent,
            &fixture.root,
            &name,
            &fixture.plan.intent,
        )
        .expect("the intent");
        name.intent_path(&fixture.root)
    };

    // The read half: a census discovering intents while another reclaimer is
    // mid-way through deleting one.
    let path = write(&mut hooks);
    let pending = windows_posix_delete_pending(&path);
    let mut found = None;
    let schedule =
        windows_stall_then_release(pending, || found = Some(list_intents(&fixture.root)));
    let found = found.expect("the listing ran").unwrap_or_else(|error| {
        panic!(
            "discovery refused after {} failed attempts over an intent another reclaimer \
                 was still closing: {error}",
            schedule.len()
        )
    });
    assert!(
        found.is_empty(),
        "an intent whose deletion completed during the wait is not a discovered record: \
         {found:?}"
    );
    windows_assert_converged_through_the_wait("discovery", &schedule);
    assert!(!path.exists());

    // The removal half: two reclaimers on one record.
    let path = write(&mut hooks);
    let pending = windows_posix_delete_pending(&path);
    let mut outcome = None;
    let schedule = windows_stall_then_release(pending, || {
        outcome = Some(remove_intent(
            &mut hooks,
            ContainerSite::RemoveIntent,
            &fixture.root,
            &name,
        ));
    });
    outcome.expect("the removal ran").unwrap_or_else(|error| {
        panic!(
            "the loser of an intent-removal race refused after {} failed attempts instead \
                 of waiting out the winner's close: {error}",
            schedule.len()
        )
    });
    windows_assert_converged_through_the_wait("removal", &schedule);
    assert!(!path.exists());
    let _ = fs::remove_dir_all(&fixture.root);
}

#[test]
fn a_create_whose_named_volume_is_absent_is_refused_before_any_effect() {
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

const DAEMON_REMOVAL_IN_PROGRESS: &str =
    "Error response from daemon: removal of container upstroke-c is already in progress";

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
    assert!(
        !super::is_absent(DAEMON_REMOVAL_IN_PROGRESS),
        "an in-progress removal is not an absent container; if `is_absent` starts covering it, \
         the tolerance below stops being an independently droppable predicate"
    );
    assert!(super::remove_already_settled(DAEMON_REMOVAL_IN_PROGRESS));
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

    assert!(super::stop_already_settled(DAEMON_REMOVAL_IN_PROGRESS));

    assert_eq!(super::settle_remove(Ok("upstroke-c\n".to_owned())), Ok(()));
}

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
        return no_image(
            "no `docker rm` lost the race after four rounds, so the removal-in-progress \
             diagnostic was not measured on this machine and these tests never pull (non_goals[1])",
        );
    };
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
    assert_eq!(
        super::settle_remove(Err(RuntimeError::Failed {
            operation: RuntimeOp::Remove,
            detail: detail.clone(),
        })),
        Ok(()),
        "the loser of a real removal race does not converge: {detail}"
    );
}

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

/// `PR163-ASTRA-RUSTDOC-LINKS`. `runtime.md` carries Rustdoc shortcut
/// references like `` [`RuntimeOp`] `` with no Markdown reference
/// definition. A CommonMark parser renders that as literal text around an
/// inline-code span — never a link — losing the cross-reference. Detect that
/// exact failure shape (a text run ending in `[`, an inline-code span, then a
/// text run starting with `]`, with no link in between) rather than counting
/// links, so the test still catches a reference that loses its definition
/// even if the surrounding prose changes.
#[test]
fn runtime_notes_rustdoc_shortcuts_resolve_to_links() {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    const NOTES: &str = include_str!("../../../docs/internals/runner/container/runtime.md");

    let events: Vec<Event> = Parser::new(NOTES).collect();
    let mut broken = Vec::new();
    let mut depth = 0i32;
    for window in events.windows(3) {
        if let Event::Start(Tag::Link { .. }) = window[0] {
            depth += 1;
        }
        if let Event::End(TagEnd::Link) = window[0] {
            depth -= 1;
        }
        if depth > 0 {
            continue;
        }
        if let (Event::Text(before), Event::Code(name), Event::Text(after)) =
            (&window[0], &window[1], &window[2])
        {
            if before.ends_with('[') && after.starts_with(']') {
                broken.push(name.to_string());
            }
        }
    }
    assert!(
        broken.is_empty(),
        "unresolved Rustdoc shortcut reference(s) with no Markdown link target: {broken:?}"
    );
}
