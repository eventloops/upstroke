//! Extended notes: `docs/internals/runner/container/census/tests.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types, clippy::disallowed_macros)]

#[cfg(test)]
mod this_file_is_test_only {}

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex, PoisonError};

use super::{
    Boundary, Census, CensusComplete, CensusStart, DiscoveredBy, LABEL_FILTER, Ownership,
    PrefixBytes, PrefixReplay, PrefixReread, PrefixSync, StablePrefixBarrier, WriteCommand,
    private_root_label, run_startup_census, view_path,
};
use crate::error::UpstrokeError;
use crate::runner::container::intent::{
    ContainerIntent, ContainerName, LABEL_INCARNATION, LABEL_PRIVATE_ROOT, LABEL_RUN,
    LABEL_RUN_DIR, containers_dir, decode_path_label, owner_run_dir, path_label,
};
use crate::runner::container::runtime::{
    ContainerRuntime, ContainerTrace, Liveness, OwnerLiveness, RuntimeOp,
};
use crate::runner::container::{
    ContainerHooks, DisposableDirView, FakeRuntime, RecordingHooks, TERMINATION_OBSERVATIONS,
    write_intent,
};
use crate::runner::{AgentId, InvocationId, ProbeTarget};
use crate::topology::effects::ContainerSite;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "upstroke-census-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch private root");
    dir
}

const REPO_KEY_A: &str = "0123456789abcdef";
const REPO_KEY_B: &str = "fedcba9876543210";
const RUN_A: &str = "01KZRN48A4ZK3AEDST3RJ8HMA4";
const RUN_B: &str = "01KZS7R0V1ZD6MC290MG350QXF";
const RUN_C: &str = "01KZSCCCCCCCCCCCCCCCCCCCCC";
const INC_1: &str = "01KZTAAAAAAAAAAAAAAAAAAAAA";
const INC_2: &str = "01KZTBBBBBBBBBBBBBBBBBBBBB";
const INC_3: &str = "01KZTCCCCCCCCCCCCCCCCCCCCC";
const POLICY_A: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const POLICY_B: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const IMAGE_ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn shell_probe() -> InvocationId {
    InvocationId::probe(ProbeTarget::Shell, 0).expect("the shell probe identity")
}

fn agent_probe() -> InvocationId {
    InvocationId::probe(ProbeTarget::Agent(AgentId::new("claude-code")), 0).expect("an agent probe")
}

#[derive(Debug, Clone)]
struct Owner {
    run_id: &'static str,
    incarnation: &'static str,
    repo_key: &'static str,
    run_dir: PathBuf,
    policy: &'static str,
}

impl Owner {
    fn new(run_id: &'static str, incarnation: &'static str, repo_key: &'static str) -> Self {
        Self {
            run_id,
            incarnation,
            repo_key,
            run_dir: PathBuf::from(format!("/repo/.upstroke/runs/{run_id}")),
            policy: POLICY_A,
        }
    }

    fn with_policy(mut self, policy: &'static str) -> Self {
        self.policy = policy;
        self
    }

    fn name(&self, invocation: &InvocationId) -> ContainerName {
        ContainerName::new(self.repo_key, self.run_id, self.incarnation, invocation)
            .expect("a container name")
    }

    fn with_run_dir(mut self, run_dir: PathBuf) -> Self {
        self.run_dir = run_dir;
        self
    }

    fn record(&self, invocation: &InvocationId) -> ContainerIntent {
        ContainerIntent::new(
            self.run_id.to_owned(),
            &self.run_dir,
            self.incarnation.to_owned(),
            self.repo_key.to_owned(),
            invocation.render(),
            self.policy.to_owned(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Present {
    Both,
    IntentOnly,
    IntentAndViewAfterReaper,
    LabelOnly,
}

fn seed(
    root: &Path,
    runtime: &FakeRuntime,
    owner: &Owner,
    invocation: &InvocationId,
    present: Present,
    state: Liveness,
) -> ContainerName {
    let name = owner.name(invocation);
    let record = owner.record(invocation);
    if present != Present::LabelOnly {
        let mut hooks = RecordingHooks::new(ContainerTrace::off());
        write_intent(&mut hooks, ContainerSite::WriteIntent, root, &name, &record)
            .expect("write the intent");
    }
    if !matches!(
        present,
        Present::IntentOnly | Present::IntentAndViewAfterReaper
    ) {
        runtime.seed_container(
            name.as_str(),
            record.labels(root),
            IMAGE_ID,
            IMAGE_ID,
            state,
        );
    }
    if present != Present::IntentOnly {
        fs::create_dir_all(view_path(root, &name)).expect("an orphan view directory");
    }
    name
}

#[derive(Debug, Default)]
struct RecordingLiveness {
    live: Mutex<BTreeSet<PathBuf>>,
    asked: Mutex<Vec<PathBuf>>,
}

impl RecordingLiveness {
    fn new() -> Self {
        Self::default()
    }

    fn set_live(&self, run_dir: &Path) {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(run_dir.to_path_buf());
    }

    fn asked(&self) -> Vec<PathBuf> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl OwnerLiveness for RecordingLiveness {
    fn is_running(&self, public_run_dir: &Path) -> bool {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(public_run_dir.to_path_buf());
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(public_run_dir)
    }
}

struct WedgedRuntime {
    inner: Arc<FakeRuntime>,
}

impl ContainerRuntime for WedgedRuntime {
    fn probe(&self) -> Result<(), crate::runner::container::runtime::RuntimeError> {
        self.inner.probe()
    }
    fn image_by_reference(
        &self,
        reference: &str,
    ) -> Result<
        Option<crate::runner::container::runtime::ImageInspection>,
        crate::runner::container::runtime::RuntimeError,
    > {
        self.inner.image_by_reference(reference)
    }
    fn image_by_id(
        &self,
        id: &str,
    ) -> Result<
        Option<crate::runner::container::runtime::ImageInspection>,
        crate::runner::container::runtime::RuntimeError,
    > {
        self.inner.image_by_id(id)
    }
    fn volume_present(
        &self,
        name: &str,
    ) -> Result<bool, crate::runner::container::runtime::RuntimeError> {
        self.inner.volume_present(name)
    }
    fn containers_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<
        Vec<crate::runner::container::runtime::DiscoveredContainer>,
        crate::runner::container::runtime::RuntimeError,
    > {
        self.inner.containers_with_label(key, value)
    }
    fn observe(
        &self,
        name: &str,
    ) -> Result<Liveness, crate::runner::container::runtime::RuntimeError> {
        self.inner.observe(name)
    }
    fn collect(
        &self,
        name: &str,
    ) -> Result<
        crate::runner::container::runtime::ContainerExecution,
        crate::runner::container::runtime::RuntimeError,
    > {
        self.inner.collect(name)
    }
    fn create(
        &self,
        _spec: &crate::runner::container::runtime::CreateSpec,
    ) -> Result<
        crate::runner::container::runtime::CreatedContainer,
        crate::runner::container::runtime::RuntimeError,
    > {
        unreachable!("a census creates nothing")
    }
    fn start(&self, _name: &str) -> Result<(), crate::runner::container::runtime::RuntimeError> {
        unreachable!("a census starts nothing")
    }
    fn stop(
        &self,
        _name: &str,
        _mode: crate::runner::container::runtime::StopMode,
    ) -> Result<(), crate::runner::container::runtime::RuntimeError> {
        Ok(())
    }
    fn remove(&self, _name: &str) -> Result<(), crate::runner::container::runtime::RuntimeError> {
        unreachable!("reclaim refuses before `rm` when termination cannot be observed")
    }
}

fn barrier() -> StablePrefixBarrier {
    let bytes = b"{\"event\":\"run_started\"}\n";
    let measured = PrefixBytes::of(bytes);
    StablePrefixBarrier::establish(
        PrefixSync {
            synced_len: measured.len,
        },
        &PrefixReread {
            first: measured.clone(),
            second: measured.clone(),
        },
        &PrefixReplay { replayed: measured },
    )
    .expect("a barrier over a stable prefix")
}

fn fresh(incarnation: &str) -> CensusStart {
    CensusStart::FreshRun {
        incarnation: incarnation.to_owned(),
    }
}

fn resume(run_id: &str, incarnation: &str) -> CensusStart {
    CensusStart::Resume {
        run_id: run_id.to_owned(),
        incarnation: incarnation.to_owned(),
        barrier: barrier(),
    }
}

struct Harness {
    root: PathBuf,
    trace: ContainerTrace,
    runtime: Arc<FakeRuntime>,
    liveness: RecordingLiveness,
    view: DisposableDirView,
}

impl Harness {
    fn new(tag: &str) -> Self {
        let root = scratch(tag);
        let trace = ContainerTrace::recording();
        Self {
            root,
            runtime: Arc::new(FakeRuntime::new(trace.clone())),
            liveness: RecordingLiveness::new(),
            view: DisposableDirView::new(trace.clone()),
            trace,
        }
    }

    fn census(&self, start: &CensusStart) -> Result<CensusComplete, UpstrokeError> {
        let mut hooks = RecordingHooks::new(self.trace.clone());
        self.run_with(&mut hooks, start)
    }

    fn run_with(
        &self,
        hooks: &mut dyn ContainerHooks,
        start: &CensusStart,
    ) -> Result<CensusComplete, UpstrokeError> {
        run_startup_census(
            hooks,
            &Census {
                private_root: &self.root,
                start,
                runtime: self.runtime.as_ref(),
                liveness: &self.liveness,
                view: &self.view,
            },
        )
    }

    fn holds(&self, name: &ContainerName) -> bool {
        self.runtime.container(name.as_str()).is_some()
    }

    fn intent_exists(&self, name: &ContainerName) -> bool {
        name.intent_path(&self.root).exists()
    }

    fn view_exists(&self, name: &ContainerName) -> bool {
        view_path(&self.root, name).exists()
    }
}

fn at(trace: &ContainerTrace, needle: &str) -> usize {
    trace.position(needle).unwrap_or_else(|| {
        panic!(
            "`{needle}` is not in the trace, which is {:#?}",
            trace.rendered()
        )
    })
}

fn refusal(error: &UpstrokeError) -> String {
    match error {
        UpstrokeError::Refused { message } => message.clone(),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn the_liveness_rule_classifies_every_cell_of_owner_run_by_incarnation_by_lock() {
    let liveness = RecordingLiveness::new();
    let live_dir = PathBuf::from("/repo/.upstroke/runs/live");
    let dead_dir = PathBuf::from("/repo/.upstroke/runs/dead");
    liveness.set_live(&live_dir);

    let mine = resume(RUN_A, INC_1);
    let cells: Vec<(&str, &str, &str, &Path, Ownership, bool)> = vec![
        (
            "own run, this incarnation, owner dir free",
            RUN_A,
            INC_1,
            dead_dir.as_path(),
            Ownership::OwnRunThisIncarnation,
            false,
        ),
        (
            "own run, this incarnation, owner dir held",
            RUN_A,
            INC_1,
            live_dir.as_path(),
            Ownership::OwnRunThisIncarnation,
            false,
        ),
        (
            "own run, earlier incarnation",
            RUN_A,
            INC_2,
            dead_dir.as_path(),
            Ownership::OwnRunEarlierIncarnation,
            false,
        ),
        (
            "own run, another earlier incarnation",
            RUN_A,
            INC_3,
            live_dir.as_path(),
            Ownership::OwnRunEarlierIncarnation,
            false,
        ),
        (
            "foreign run, lock held, my own incarnation",
            RUN_B,
            INC_1,
            live_dir.as_path(),
            Ownership::ForeignRunLiveOwner,
            true,
        ),
        (
            "foreign run, lock free, my own incarnation",
            RUN_B,
            INC_1,
            dead_dir.as_path(),
            Ownership::ForeignRunDeadOwner,
            true,
        ),
        (
            "foreign run, lock held, another incarnation",
            RUN_B,
            INC_2,
            live_dir.as_path(),
            Ownership::ForeignRunLiveOwner,
            true,
        ),
        (
            "foreign run, lock held, a third incarnation",
            RUN_B,
            INC_3,
            live_dir.as_path(),
            Ownership::ForeignRunLiveOwner,
            true,
        ),
        (
            "foreign run, lock free, another incarnation",
            RUN_B,
            INC_2,
            dead_dir.as_path(),
            Ownership::ForeignRunDeadOwner,
            true,
        ),
        (
            "foreign run, lock free, a third incarnation",
            RUN_B,
            INC_3,
            dead_dir.as_path(),
            Ownership::ForeignRunDeadOwner,
            true,
        ),
    ];

    let mut seen = BTreeSet::new();
    let mut probes = 0;
    for (what, run_id, incarnation, run_dir, expected, probed) in &cells {
        let before = liveness.asked().len();
        let got = super::classify_ownership(&mine, run_id, incarnation, run_dir, &liveness);
        assert_eq!(got, *expected, "{what}");
        let asked = liveness.asked().len() - before;
        assert_eq!(
            asked,
            usize::from(*probed),
            "{what}: the owner's lock was probed {asked} time(s) and the rule probes {}",
            usize::from(*probed)
        );
        if *probed {
            assert_eq!(
                liveness.asked().last(),
                Some(&run_dir.to_path_buf()),
                "{what}: arm (ii) probed a directory that is not the owner's"
            );
            probes += 1;
        }
        seen.insert(got);
    }
    assert_eq!(
        seen.len(),
        4,
        "the grid must reach all four classifications, not the three a one-axis fixture reaches"
    );
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        Ownership::ALL.to_vec(),
        "the classifications the grid reaches are exactly the ones the enum declares"
    );
    assert_eq!(liveness.asked().len(), probes);

    assert_eq!(
        Ownership::ALL
            .iter()
            .filter(|ownership| ownership.refuses())
            .copied()
            .collect::<Vec<_>>(),
        vec![Ownership::OwnRunThisIncarnation]
    );
    for ownership in Ownership::ALL {
        assert!(
            !(ownership.refuses() && ownership.reclaims()),
            "{} both refuses and reclaims",
            ownership.name()
        );
    }
    assert_eq!(
        Ownership::ALL
            .iter()
            .map(|ownership| ownership.name())
            .collect::<BTreeSet<_>>()
            .len(),
        Ownership::ALL.len(),
        "two classifications share one reported name"
    );

    let brand_new = fresh(INC_1);
    let fresh_liveness = RecordingLiveness::new();
    fresh_liveness.set_live(&live_dir);
    let mut own_incarnation_cells = 0;
    for (what, run_id, incarnation, run_dir, _, _) in &cells {
        let got =
            super::classify_ownership(&brand_new, run_id, incarnation, run_dir, &fresh_liveness);
        assert_ne!(
            got,
            Ownership::OwnRunEarlierIncarnation,
            "{what}: a fresh run drives no run, so nothing can be its earlier incarnation"
        );
        assert_ne!(got, Ownership::OwnRunThisIncarnation, "{what}");
        assert!(
            !got.refuses(),
            "{what}: a fresh run holds no run lock, so arm (i)'s refusal is unreachable for it \
             and every candidate is classified by the owner's lock"
        );
        let expected = if run_dir == &live_dir.as_path() {
            Ownership::ForeignRunLiveOwner
        } else {
            Ownership::ForeignRunDeadOwner
        };
        assert_eq!(got, expected, "{what}");
        if *incarnation == INC_1 {
            own_incarnation_cells += 1;
        }
    }
    assert!(
        own_incarnation_cells >= 4,
        "the grid must carry more than one cell naming this process's incarnation"
    );
    assert_eq!(
        fresh_liveness.asked().len(),
        cells.len(),
        "a fresh run's census left a candidate's owner lock unprobed: {:?}",
        fresh_liveness.asked()
    );
}

#[test]
fn the_owner_lock_is_probed_exactly_once_per_candidate() {
    let held = PathBuf::from("/repo/.upstroke/runs/held");
    let free = PathBuf::from("/repo/.upstroke/runs/free");
    for (what, owner_dir, expected) in [
        ("held", &held, Ownership::ForeignRunLiveOwner),
        ("free", &free, Ownership::ForeignRunDeadOwner),
    ] {
        let liveness = RecordingLiveness::new();
        liveness.set_live(&held);
        let got =
            super::classify_ownership(&resume(RUN_A, INC_1), RUN_B, INC_2, owner_dir, &liveness);
        assert_eq!(got, expected, "{what}");
        assert_eq!(
            liveness.asked(),
            vec![owner_dir.clone()],
            "{what}: the owner's run.lock is probed once, non-blocking; a retry loop around a \
             held lock is a census that waits on a live neighbour"
        );
    }
}

#[test]
fn a_live_runs_dead_earlier_incarnation_is_untouched_by_a_foreign_census() {
    for owner_is_live in [true, false] {
        let harness = Harness::new(if owner_is_live {
            "crossed-live"
        } else {
            "crossed-dead"
        });
        let earlier = Owner::new(RUN_B, INC_1, REPO_KEY_A);
        let current = Owner::new(RUN_B, INC_2, REPO_KEY_A);
        assert_eq!(
            earlier.run_dir, current.run_dir,
            "two incarnations of one run share one public run directory, which is what makes \
             this the crossed cell rather than two unrelated runs"
        );
        if owner_is_live {
            harness.liveness.set_live(&current.run_dir);
        }
        let old = seed(
            &harness.root,
            &harness.runtime,
            &earlier,
            &shell_probe(),
            Present::Both,
            Liveness::Running,
        );
        let new = seed(
            &harness.root,
            &harness.runtime,
            &current,
            &agent_probe(),
            Present::Both,
            Liveness::Running,
        );
        assert_ne!(old, new, "the incarnation component separates the names");

        let complete = harness
            .census(&fresh(INC_3))
            .expect("a foreign census of another run's containers");
        let report = complete.report();

        if owner_is_live {
            assert!(report.reclaimed.is_empty(), "{:#?}", report.reclaimed);
            assert_eq!(report.untouched.len(), 2);
            assert!(report.was_untouched(&old) && report.was_untouched(&new));
            assert!(
                harness.holds(&old) && harness.holds(&new),
                "a live owner's containers were killed, including the dead incarnation's"
            );
            assert!(harness.intent_exists(&old) && harness.intent_exists(&new));
            for entry in &report.untouched {
                assert_eq!(entry.ownership, Ownership::ForeignRunLiveOwner);
            }
        } else {
            assert_eq!(report.reclaimed.len(), 2, "{:#?}", report.reclaimed);
            assert!(report.untouched.is_empty());
            assert!(!harness.holds(&old) && !harness.holds(&new));
            assert!(!harness.intent_exists(&old) && !harness.intent_exists(&new));
            for entry in &report.reclaimed {
                assert_eq!(entry.ownership, Ownership::ForeignRunDeadOwner);
            }
            let incarnations: BTreeSet<&str> = report
                .reclaimed
                .iter()
                .map(|entry| entry.incarnation.as_str())
                .collect();
            assert_eq!(
                incarnations.len(),
                2,
                "a dead owner's containers span both its incarnations"
            );
        }
    }
}

#[test]
fn arm_two_gives_one_answer_whatever_the_incarnation_that_reaches_it() {
    let liveness = RecordingLiveness::new();
    let held = PathBuf::from("/repo/.upstroke/runs/held");
    let free = PathBuf::from("/repo/.upstroke/runs/free");
    liveness.set_live(&held);
    let me = resume(RUN_A, INC_1);
    let incarnations = [INC_2, INC_3, "01KZTDDDDDDDDDDDDDDDDDDDDD", INC_1];
    assert_eq!(
        incarnations.iter().collect::<BTreeSet<_>>().len(),
        4,
        "four distinct incarnations, one of them this process's own"
    );
    assert!(
        incarnations.contains(&INC_1),
        "the domain must include this process's own incarnation: that is the value the rule was \
         wrongly reading before arm (ii) ever saw it"
    );

    let mut held_answers = BTreeSet::new();
    let mut free_answers = BTreeSet::new();
    for incarnation in incarnations {
        held_answers.insert(super::classify_ownership(
            &me,
            RUN_B,
            incarnation,
            &held,
            &liveness,
        ));
        free_answers.insert(super::classify_ownership(
            &me,
            RUN_B,
            incarnation,
            &free,
            &liveness,
        ));
    }
    assert_eq!(
        held_answers.into_iter().collect::<Vec<_>>(),
        vec![Ownership::ForeignRunLiveOwner],
        "four incarnations of a live foreign owner, one answer"
    );
    assert_eq!(
        free_answers.into_iter().collect::<Vec<_>>(),
        vec![Ownership::ForeignRunDeadOwner],
        "four incarnations of a dead foreign owner, one answer"
    );

    assert_eq!(
        liveness.asked().len(),
        8,
        "a foreign candidate was classified without its owner's lock being probed: {:?}",
        liveness.asked()
    );
}

#[test]
fn the_census_learns_no_incarnation_from_the_owner_liveness_seam() {
    let harness = Harness::new("no-incarnation-from-lock");
    let dead = Owner::new(RUN_B, INC_2, REPO_KEY_A);
    seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    harness
        .census(&resume(RUN_A, INC_1))
        .expect("a census of a dead foreign owner");

    assert_eq!(harness.liveness.asked(), vec![dead.run_dir.clone()]);
    let one_bit: bool = harness.liveness.is_running(&dead.run_dir);
    assert!(!one_bit);

    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runner/container/census.rs"),
    )
    .expect("the census module");
    let production =
        crate::effects::blank_comments_and_strings(&crate::effects::production_region(&source));
    for needle in ["run.lock", "lock_file", "acquire", "holder"] {
        assert!(
            !production.contains(needle),
            "the census names `{needle}`; the incarnation comes from run_started(4)/\
             run_resumed(4) and is never read from lock-file contents"
        );
    }
    assert!(
        production.contains("is_running("),
        "the census asks the one-bit seam, so this scan is looking at the right file"
    );
}

#[test]
fn orphan_reclaimed_before_slot_reset() {
    let harness = Harness::new("before-slot-reset");
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let name = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );

    let complete = harness.census(&fresh(INC_2)).expect("the census completes");

    let sites: Vec<ContainerSite> = harness
        .trace
        .sites()
        .into_iter()
        .map(|(site, _)| site)
        .collect();
    let ordered: Vec<ContainerSite> = {
        let mut seen = Vec::new();
        for site in sites {
            if seen.last() != Some(&site) {
                seen.push(site);
            }
        }
        seen
    };
    assert_eq!(
        ordered,
        vec![
            ContainerSite::Stop,
            ContainerSite::Remove,
            ContainerSite::UnmountGitView,
            ContainerSite::RemoveIntent,
        ],
        "reclaim is kill -> observe -> rm -> remove view -> remove intent"
    );
    assert!(
        at(&harness.trace, &format!("rt:observe:{name}"))
            < at(&harness.trace, &format!("rt:remove:{name}")),
        "the observation wait was dropped: `rm` before termination was proven"
    );
    assert!(!harness.holds(&name) && !harness.intent_exists(&name) && !harness.view_exists(&name));
    assert_eq!(complete.report().reclaimed.len(), 1);
    assert_eq!(complete.private_root(), harness.root.as_path());

    let root = scratch("blocks-admission");
    let inner = Arc::new(FakeRuntime::new(ContainerTrace::off()));
    let stuck = seed(
        &root,
        &inner,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    let wedged = WedgedRuntime {
        inner: Arc::clone(&inner),
    };
    let liveness = RecordingLiveness::new();
    let view = DisposableDirView::new(ContainerTrace::off());
    let mut hooks = RecordingHooks::new(ContainerTrace::off());
    let start = fresh(INC_2);
    let error = run_startup_census(
        &mut hooks,
        &Census {
            private_root: &root,
            start: &start,
            runtime: &wedged,
            liveness: &liveness,
            view: &view,
        },
    )
    .expect_err("a container that cannot be observed terminated blocks admission");
    let message = refusal(&error);
    assert!(
        message.contains("cannot be observed terminated"),
        "{message}"
    );
    assert!(message.contains("blocks admission"), "{message}");
    assert!(
        inner.container(stuck.as_str()).is_some(),
        "the container is still there, and nothing admitted over it"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn live_owner_untouched_while_dead_orphan_reclaimed() {
    let harness = Harness::new("live-and-dead");
    let live = Owner::new(RUN_A, INC_1, REPO_KEY_A);
    let dead = Owner::new(RUN_B, INC_2, REPO_KEY_B);
    assert_ne!(live.repo_key, dead.repo_key, "different repositories");
    assert_ne!(live.run_dir, dead.run_dir);
    harness.liveness.set_live(&live.run_dir);

    let a = seed(
        &harness.root,
        &harness.runtime,
        &live,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    let b = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );

    let complete = harness.census(&fresh(INC_3)).expect("the census completes");
    let report = complete.report();

    assert_eq!(report.reclaimed.len(), 1);
    assert_eq!(report.reclaimed[0].name, b);
    assert_eq!(report.untouched.len(), 1);
    assert_eq!(report.untouched[0].name, a);
    assert!(
        harness.holds(&a),
        "the live coordinator's container continues"
    );
    assert!(harness.intent_exists(&a));
    assert!(!harness.holds(&b));

    let named_a: Vec<String> = harness
        .trace
        .rendered()
        .into_iter()
        .filter(|entry| entry.starts_with("rt:") && entry.ends_with(a.as_str()))
        .collect();
    assert!(
        named_a.is_empty(),
        "the census issued operations against a live owner's container: {named_a:?}"
    );

    assert!(
        at(&harness.trace, &format!("rt:observe:{b}"))
            < at(&harness.trace, &format!("rt:remove:{b}"))
    );
    assert!(
        !harness.trace.ops().contains(&RuntimeOp::InspectVolume),
        "a census inspects no volume; the turn is taken by a consumer of the token"
    );
}

#[test]
fn labeled_orphan_without_intent_reclaimed() {
    let harness = Harness::new("labeled-orphan");
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let recorded = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    let unrecorded = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &agent_probe(),
        Present::LabelOnly,
        Liveness::Running,
    );
    assert!(!harness.intent_exists(&unrecorded));

    let complete = harness.census(&fresh(INC_2)).expect("the census completes");
    let report = complete.report();
    assert_eq!(report.reclaimed.len(), 2);
    assert!(!harness.holds(&recorded) && !harness.holds(&unrecorded));

    let by_discovery: Vec<(ContainerName, DiscoveredBy, Boundary)> = report
        .reclaimed
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                entry.discovered_by,
                entry.boundary.clone(),
            )
        })
        .collect();
    let recorded_entry = by_discovery
        .iter()
        .find(|(name, ..)| name == &recorded)
        .expect("the recorded one");
    let unrecorded_entry = by_discovery
        .iter()
        .find(|(name, ..)| name == &unrecorded)
        .expect("the unrecorded one");
    assert_eq!(recorded_entry.1, DiscoveredBy::IntentAndLabel);
    assert_eq!(unrecorded_entry.1, DiscoveredBy::LabelOnly);
    assert_eq!(
        recorded_entry.2,
        Boundary::FromIntent(POLICY_A.to_owned()),
        "a record-backed container's boundary is its runner_policy_sha256"
    );
    assert_eq!(
        unrecorded_entry.2,
        Boundary::NoIntentRecord,
        "a labeled orphan with no record has no boundary from this side; PR7's owner \
         record is the other half, and saying so beats inventing a digest"
    );

    let verdicts: BTreeSet<Ownership> = report
        .reclaimed
        .iter()
        .map(|entry| entry.ownership)
        .collect();
    assert_eq!(
        verdicts.into_iter().collect::<Vec<_>>(),
        vec![Ownership::ForeignRunDeadOwner]
    );
}

#[test]
fn same_run_resume_reclaims_earlier_incarnation_orphan() {
    let harness = Harness::new("resume-earlier-incarnation");
    let earlier = Owner::new(RUN_A, INC_1, REPO_KEY_A);
    let probe = shell_probe();
    let orphan = seed(
        &harness.root,
        &harness.runtime,
        &earlier,
        &probe,
        Present::Both,
        Liveness::Running,
    );

    let mine = Owner::new(RUN_A, INC_2, REPO_KEY_A);
    let would_be = mine.name(&probe);
    assert_eq!(
        probe.render(),
        shell_probe().render(),
        "the probe identity repeats across incarnations by construction, which is why the \
         name carries the incarnation"
    );
    assert_ne!(orphan, would_be, "the container names differ");
    assert_ne!(
        orphan.intent_path(&harness.root),
        would_be.intent_path(&harness.root),
        "the intent paths differ, so no earlier ownership evidence is overwritten"
    );

    let complete = harness
        .census(&resume(RUN_A, INC_2))
        .expect("the resume's census completes");
    let report = complete.report();
    assert_eq!(report.command, WriteCommand::Resume);
    assert_eq!(report.reclaimed.len(), 1);
    assert_eq!(report.reclaimed[0].name, orphan);
    assert_eq!(
        report.reclaimed[0].ownership,
        Ownership::OwnRunEarlierIncarnation,
        "dead by construction: the run lock is exclusive and this process holds it"
    );
    assert!(
        harness.liveness.asked().is_empty(),
        "arm (i) probed the lock of the run this process is itself driving: {:?}",
        harness.liveness.asked()
    );
    assert!(!harness.holds(&orphan) && !harness.intent_exists(&orphan));

    assert!(
        !harness.holds(&would_be),
        "this incarnation has started nothing yet; the census precedes every invocation"
    );
}

#[test]
fn same_run_resume_censuses_recorded_root_after_default_changed() {
    let recorded = Harness::new("recorded-root");
    let other_root = scratch("default-root-that-moved");
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);

    let in_recorded = seed(
        &recorded.root,
        &recorded.runtime,
        &dead,
        &shell_probe(),
        Present::IntentOnly,
        Liveness::Gone,
    );
    let mut hooks = RecordingHooks::new(ContainerTrace::off());
    write_intent(
        &mut hooks,
        ContainerSite::WriteIntent,
        &other_root,
        &in_recorded,
        &dead.record(&shell_probe()),
    )
    .expect("an intent under the other root");

    let complete = recorded
        .census(&resume(RUN_A, INC_2))
        .expect("the census completes");
    assert_eq!(complete.private_root(), recorded.root.as_path());
    assert_eq!(complete.report().reclaimed.len(), 1);
    assert!(!recorded.intent_exists(&in_recorded));
    assert!(
        in_recorded.intent_path(&other_root).exists(),
        "the census reached into a root it was not given: different private roots are \
         disjoint worlds"
    );

    let filtered: Vec<String> = recorded
        .trace
        .rendered()
        .into_iter()
        .filter(|entry| entry.starts_with("rt:list-by-label:"))
        .collect();
    assert_eq!(
        filtered,
        vec![format!(
            "rt:list-by-label:{}",
            private_root_label(&recorded.root)
        )]
    );
    let _ = fs::remove_dir_all(&other_root);
}

#[test]
fn repeated_crashes_reclaim_every_dead_incarnation() {
    const INCARNATIONS: &[&str] = &[INC_1, INC_2, INC_3, "01KZTDDDDDDDDDDDDDDDDDDDDD"];
    const RESUMING: &str = "01KZTEEEEEEEEEEEEEEEEEEEEE";

    for dead_count in 1..=3_usize {
        let harness = Harness::new(&format!("incarnations-{dead_count}"));
        let probe = shell_probe();
        let mut names = Vec::new();
        for (ordinal, incarnation) in INCARNATIONS.iter().take(dead_count).enumerate() {
            let owner = Owner::new(RUN_A, incarnation, REPO_KEY_A);
            let invocation = if ordinal + 1 == dead_count {
                agent_probe()
            } else {
                probe.clone()
            };
            names.push(seed(
                &harness.root,
                &harness.runtime,
                &owner,
                &invocation,
                Present::Both,
                Liveness::Running,
            ));
        }

        assert_eq!(
            names.iter().collect::<BTreeSet<_>>().len(),
            dead_count,
            "{dead_count} distinct container names"
        );
        assert_eq!(
            names
                .iter()
                .map(|name| name.intent_path(&harness.root))
                .collect::<BTreeSet<_>>()
                .len(),
            dead_count,
            "{dead_count} distinct intent paths: no earlier ownership evidence was overwritten"
        );
        assert_eq!(
            fs::read_dir(containers_dir(&harness.root))
                .expect("the namespace")
                .count(),
            dead_count
        );

        let complete = harness
            .census(&resume(RUN_A, RESUMING))
            .expect("the census completes");
        let report = complete.report();
        assert_eq!(
            report.reclaimed.len(),
            dead_count,
            "{dead_count} dead incarnations, {} reclaimed",
            report.reclaimed.len()
        );
        let reclaimed_incarnations: BTreeSet<&str> = report
            .reclaimed
            .iter()
            .map(|entry| entry.incarnation.as_str())
            .collect();
        assert_eq!(
            reclaimed_incarnations,
            INCARNATIONS
                .iter()
                .take(dead_count)
                .copied()
                .collect::<BTreeSet<_>>(),
            "the reclaimed set is not exactly the dead incarnations"
        );
        for name in &names {
            assert!(!harness.holds(name) && !harness.intent_exists(name));
        }
        assert_eq!(
            fs::read_dir(containers_dir(&harness.root))
                .expect("the namespace")
                .count(),
            0,
            "the namespace is empty: every record was removed, not merely the last one"
        );
    }
}

#[test]
fn concurrent_reclaimers_converge() {
    const ROUNDS: usize = 24;
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let probe = shell_probe();
    let mut interleaved = 0_usize;

    for round in 0..ROUNDS {
        let root = scratch(&format!("converge-{round}"));
        let trace = ContainerTrace::off();
        let runtime = Arc::new(FakeRuntime::new(trace.clone()));
        let names: Vec<ContainerName> = (0..4)
            .map(|ordinal| {
                let invocation =
                    InvocationId::probe(ProbeTarget::Shell, ordinal).expect("a probe identity");
                seed(
                    &root,
                    &runtime,
                    &dead,
                    &invocation,
                    Present::Both,
                    Liveness::Running,
                )
            })
            .collect();
        let _ = &probe;
        assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 4);
        let gate = Arc::new(Barrier::new(2));

        let starts = [
            ("resuming incarnation", resume(RUN_B, INC_2)),
            (
                "foreign write command",
                CensusStart::FreshRun {
                    incarnation: INC_3.to_owned(),
                },
            ),
        ];
        let mut handles = Vec::new();
        for (label, start) in starts {
            let root = root.clone();
            let runtime = Arc::clone(&runtime);
            let gate = Arc::clone(&gate);
            handles.push(std::thread::spawn(move || {
                let liveness = RecordingLiveness::new();
                let view = DisposableDirView::new(ContainerTrace::off());
                let mut hooks = RecordingHooks::new(ContainerTrace::off());
                gate.wait();
                let outcome = run_startup_census(
                    &mut hooks,
                    &Census {
                        private_root: &root,
                        start: &start,
                        runtime: runtime.as_ref(),
                        liveness: &liveness,
                        view: &view,
                    },
                )
                .map(|complete| {
                    let report = complete.report();
                    (
                        report.reclaimed.len(),
                        report
                            .reclaimed
                            .iter()
                            .map(|entry| entry.ownership)
                            .collect::<BTreeSet<_>>(),
                    )
                });
                (label, outcome)
            }));
        }
        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("a reclaimer panicked"))
            .collect();
        for (label, outcome) in &outcomes {
            assert!(
                outcome.is_ok(),
                "the {label} refused instead of converging: {outcome:?}"
            );
            if let Ok((_, arms)) = outcome {
                let expected = if *label == "resuming incarnation" {
                    Ownership::OwnRunEarlierIncarnation
                } else {
                    Ownership::ForeignRunDeadOwner
                };
                for arm in arms {
                    assert_eq!(
                        *arm, expected,
                        "the {label} reclaimed through the wrong arm of the liveness rule"
                    );
                }
            }
        }
        for name in &names {
            assert!(runtime.container(name.as_str()).is_none());
            assert!(!name.intent_path(&root).exists());
            assert!(!view_path(&root, name).exists());
        }
        let counts: Vec<usize> = outcomes
            .iter()
            .map(|(_, outcome)| outcome.as_ref().map_or(0, |(count, _)| *count))
            .collect();
        let total: usize = counts.iter().sum();
        assert!(
            total >= names.len(),
            "round {round}: two reclaimers between them reported {total} of {} orphans they \
             removed",
            names.len()
        );
        if counts.iter().all(|count| *count > 0) {
            interleaved += 1;
        }
        let _ = fs::remove_dir_all(&root);
    }
    assert!(
        interleaved > 0,
        "in none of {ROUNDS} rounds did both reclaimers remove anything, so this fixture never \
         interleaved and `T-CONTAINER.resume_action`'s \"concurrent reclaimers converge\" was \
         not measured"
    );
}

#[test]
fn a_reclaimer_suspended_mid_sequence_converges_with_one_that_finished() {
    struct BlockAt {
        trace: ContainerTrace,
        site: crate::topology::effects::EffectSiteId,
        phase: crate::topology::effects::HookPhase,
        release: Option<std::sync::mpsc::Receiver<()>>,
        arrived: Option<std::sync::mpsc::Sender<()>>,
    }
    impl ContainerHooks for BlockAt {
        fn phase(
            &mut self,
            site: crate::topology::effects::EffectSiteId,
            phase: crate::topology::effects::HookPhase,
        ) -> crate::topology::effects::Injection {
            if site == self.site && phase == self.phase {
                if let Some(arrived) = self.arrived.take() {
                    let _ = arrived.send(());
                }
                if let Some(release) = self.release.take() {
                    let _ = release.recv();
                }
            }
            crate::topology::effects::Injection::Proceed
        }
        fn trace(&self) -> ContainerTrace {
            self.trace.clone()
        }
    }

    let root = scratch("suspended-reclaimer");
    let runtime = Arc::new(FakeRuntime::new(ContainerTrace::off()));
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let name = seed(
        &root,
        &runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );

    let (arrived_tx, arrived_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let slow_root = root.clone();
    let slow_runtime = Arc::clone(&runtime);
    let slow = std::thread::spawn(move || {
        let liveness = RecordingLiveness::new();
        let view = DisposableDirView::new(ContainerTrace::off());
        let mut hooks = BlockAt {
            trace: ContainerTrace::off(),
            site: crate::topology::effects::EffectSiteId::Container(ContainerSite::Remove),
            phase: crate::topology::effects::HookPhase::Before,
            release: Some(release_rx),
            arrived: Some(arrived_tx),
        };
        let start = CensusStart::FreshRun {
            incarnation: INC_2.to_owned(),
        };
        run_startup_census(
            &mut hooks,
            &Census {
                private_root: &slow_root,
                start: &start,
                runtime: slow_runtime.as_ref(),
                liveness: &liveness,
                view: &view,
            },
        )
        .map(|complete| complete.report().reclaimed.len())
    });

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("the slow reclaimer reached Container.Remove");

    let fast = {
        let liveness = RecordingLiveness::new();
        let view = DisposableDirView::new(ContainerTrace::off());
        let mut hooks = RecordingHooks::new(ContainerTrace::off());
        let start = CensusStart::FreshRun {
            incarnation: INC_3.to_owned(),
        };
        run_startup_census(
            &mut hooks,
            &Census {
                private_root: &root,
                start: &start,
                runtime: runtime.as_ref(),
                liveness: &liveness,
                view: &view,
            },
        )
    };
    assert!(fast.is_ok(), "the second reclaimer refused: {fast:?}");
    assert!(!name.intent_path(&root).exists());

    release_tx.send(()).expect("release the slow reclaimer");
    let slow = slow.join().expect("the slow reclaimer panicked");
    assert!(
        slow.is_ok(),
        "a reclaimer resumed into an already-converged machine and refused: {slow:?}"
    );
    assert!(runtime.container(name.as_str()).is_none());
    assert!(!view_path(&root, &name).exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn schema4_probe_container_owned_during_preflight_untouched_by_foreign_census() {
    let harness = Harness::new("preflight-probes");
    let preflighting = Owner::new(RUN_A, INC_1, REPO_KEY_A).with_policy(POLICY_A);
    let dead = Owner::new(RUN_B, INC_2, REPO_KEY_B).with_policy(POLICY_B);
    harness.liveness.set_live(&preflighting.run_dir);

    let mut held = Vec::new();
    for invocation in [shell_probe(), agent_probe()] {
        held.push(seed(
            &harness.root,
            &harness.runtime,
            &preflighting,
            &invocation,
            Present::Both,
            Liveness::Running,
        ));
    }
    let orphan = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );

    let complete = harness
        .census(&fresh(INC_3))
        .expect("a concurrent foreign census");
    let report = complete.report();
    assert_eq!(report.untouched.len(), 2);
    for name in &held {
        assert!(report.was_untouched(name));
        assert!(harness.holds(name), "a preflighting run's probe was killed");
        assert!(harness.intent_exists(name));
    }
    assert_eq!(report.reclaimed.len(), 1);
    assert_eq!(report.reclaimed[0].name, orphan);
    assert!(!harness.holds(&orphan));

    assert_eq!(
        held.iter().collect::<BTreeSet<_>>().len(),
        2,
        "a shell probe and an agent probe, two distinct names"
    );
}

#[test]
fn census_refuses_when_intents_exist_without_reachable_runtime() {
    let harness = Harness::new("intents-without-runtime");
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let name = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::IntentOnly,
        Liveness::Gone,
    );
    harness.runtime.set_unreachable(RuntimeOp::ListByLabel);
    assert!(
        harness.runtime.probe().is_ok(),
        "`probe` still answers: an implementation that gated on it would proceed here"
    );

    let error = harness
        .census(&fresh(INC_2))
        .expect_err("intents exist and the runtime cannot be reached");
    let message = refusal(&error);
    assert!(message.contains("cannot be reached"), "{message}");
    assert!(
        message.contains("prove those containers terminated"),
        "{message}"
    );
    assert!(
        harness.intent_exists(&name),
        "the refusal happened before any effect: the record is untouched"
    );
    assert!(
        harness.trace.sites().is_empty(),
        "the refusal reached a funnel site: {:#?}",
        harness.trace.rendered()
    );

    harness.runtime.set_reachable(RuntimeOp::ListByLabel);
    let complete = harness.census(&fresh(INC_2)).expect("now it proceeds");
    assert_eq!(complete.report().reclaimed.len(), 1);
    assert!(!harness.intent_exists(&name));
}

#[test]
fn census_proceeds_without_runtime_when_no_intent_exists() {
    for (tag, command) in [("run", WriteCommand::Run), ("resume", WriteCommand::Resume)] {
        let harness = Harness::new(&format!("no-intent-no-runtime-{tag}"));
        harness.runtime.set_all_unreachable();
        assert!(
            harness.runtime.probe().is_err(),
            "the whole daemon is unreachable"
        );
        assert!(
            !containers_dir(&harness.root).exists(),
            "an empty namespace"
        );

        let start = match command {
            WriteCommand::Run => fresh(INC_1),
            WriteCommand::Resume => resume(RUN_A, INC_1),
        };
        let complete = harness
            .census(&start)
            .expect("with no intent and no reachable runtime it proceeds");
        let report = complete.report();
        assert_eq!(report.runtime_use, super::RuntimeUse::NotRequired);
        assert_eq!(report.command, command);
        assert!(report.reclaimed.is_empty() && report.untouched.is_empty());
        assert!(
            harness.trace.sites().is_empty(),
            "a census with nothing to do performed an effect"
        );
    }
}

#[test]
fn a_reachable_runtime_that_refuses_to_list_refuses_the_write_command() {
    let unreachable = Harness::new("list-unreachable");
    unreachable.runtime.set_unreachable(RuntimeOp::ListByLabel);
    assert!(
        unreachable.census(&fresh(INC_1)).is_ok(),
        "no intent, unreachable runtime: proceeds"
    );

    let failing = Harness::new("list-failing");
    failing.runtime.set_failing(RuntimeOp::ListByLabel);
    let error = failing
        .census(&fresh(INC_1))
        .expect_err("no intent, and a runtime that answered and would not list");
    let message = refusal(&error);
    assert!(message.contains("reached and refused"), "{message}");
    assert!(message.contains("cannot prove"), "{message}");
}

#[test]
fn census_report_names_reclaimed_probe_boundary() {
    let harness = Harness::new("boundary-from-digest");
    let one = Owner::new(RUN_B, INC_1, REPO_KEY_A).with_policy(POLICY_A);
    let two = Owner::new(RUN_C, INC_2, REPO_KEY_B).with_policy(POLICY_B);
    assert_ne!(one.policy, two.policy, "two distinct runner policies");

    let first = seed(
        &harness.root,
        &harness.runtime,
        &one,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    let second = seed(
        &harness.root,
        &harness.runtime,
        &two,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );

    let complete = harness.census(&fresh(INC_3)).expect("the census completes");
    let report = complete.report();
    assert_eq!(report.reclaimed.len(), 2);
    assert_eq!(
        report.boundary_of(&first),
        Some(&Boundary::FromIntent(POLICY_A.to_owned()))
    );
    assert_eq!(
        report.boundary_of(&second),
        Some(&Boundary::FromIntent(POLICY_B.to_owned()))
    );
    let digests: BTreeSet<Option<&str>> = report
        .reclaimed
        .iter()
        .map(|entry| entry.boundary.digest())
        .collect();
    assert_eq!(
        digests.len(),
        2,
        "the report carried one boundary for two containers with different policies"
    );

    let recorded: BTreeSet<String> = [POLICY_A, POLICY_B]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let reported: BTreeSet<String> = report
        .reclaimed
        .iter()
        .filter_map(|entry| entry.boundary.digest().map(str::to_owned))
        .collect();
    assert_eq!(reported, recorded);

    assert!(!harness.root.join("events.jsonl").exists());
}

#[test]
fn an_intent_naming_this_processs_own_incarnation_is_refused_before_any_effect() {
    let cells: [(&str, &str, &str, bool, bool); 6] = [
        ("own-run-own-incarnation", RUN_A, INC_1, false, true),
        ("own-run-earlier-incarnation", RUN_A, INC_2, false, false),
        (
            "foreign-run-own-incarnation-lock-free",
            RUN_C,
            INC_1,
            false,
            false,
        ),
        (
            "foreign-run-own-incarnation-lock-held",
            RUN_C,
            INC_1,
            true,
            false,
        ),
        (
            "foreign-run-earlier-incarnation-lock-free",
            RUN_C,
            INC_2,
            false,
            false,
        ),
        (
            "foreign-run-earlier-incarnation-lock-held",
            RUN_C,
            INC_2,
            true,
            false,
        ),
    ];
    assert_eq!(
        cells
            .iter()
            .map(|(_, run, incarnation, held, _)| (run, incarnation, held))
            .collect::<BTreeSet<_>>()
            .len(),
        6,
        "six distinct cells of {{owner run}} x {{incarnation}} x {{lock}}"
    );
    assert_eq!(
        cells
            .iter()
            .filter(|(_, _, _, _, refuses)| *refuses)
            .count(),
        1,
        "exactly one cell of the grid is arm (i)'s own-incarnation refusal; if a second one \
         refuses, the comparison has been hoisted in front of the owner-run split again"
    );

    for (tag, run_id, incarnation, lock_held, refuses) in cells {
        let harness = Harness::new(&format!("own-incarnation-{tag}"));
        let orphan_owner = Owner::new(RUN_B, INC_3, REPO_KEY_B);
        let orphan = seed(
            &harness.root,
            &harness.runtime,
            &orphan_owner,
            &shell_probe(),
            Present::Both,
            Liveness::Running,
        );
        let suspect_owner = Owner::new(run_id, incarnation, REPO_KEY_A);
        if lock_held {
            harness.liveness.set_live(&suspect_owner.run_dir);
        }
        let suspect = seed(
            &harness.root,
            &harness.runtime,
            &suspect_owner,
            &agent_probe(),
            Present::Both,
            Liveness::Running,
        );

        let outcome = harness.census(&resume(RUN_A, INC_1));
        if refuses {
            let message = refusal(&outcome.expect_err("refused"));
            assert!(message.contains("own incarnation"), "[{tag}] {message}");
            assert!(
                message.contains("cannot exist at census time"),
                "[{tag}] {message}"
            );
            assert!(
                harness.trace.sites().is_empty(),
                "[{tag}] the census reclaimed something and then refused: {:#?}",
                harness.trace.rendered()
            );
            assert!(
                harness.holds(&orphan) && harness.intent_exists(&orphan),
                "[{tag}] the other orphan was reclaimed on behalf of a write command that refused"
            );
            assert!(
                harness.holds(&suspect) && harness.intent_exists(&suspect),
                "[{tag}] the container the census refused over was killed anyway"
            );
        } else {
            let complete = outcome.expect("an earlier incarnation is reclaimable or skipped");
            assert!(!harness.holds(&orphan), "[{tag}] the dead orphan survived");
            assert_eq!(
                harness.holds(&suspect),
                lock_held,
                "[{tag}] a live foreign owner's container was killed, or a dead one survived"
            );
            assert_eq!(
                complete.report().was_untouched(&suspect),
                lock_held,
                "[{tag}] the report disagrees with what happened to the machine"
            );
        }
    }
}

#[test]
fn a_live_foreign_owners_container_naming_this_incarnation_is_refused_and_not_killed() {
    for (tag, incarnation) in [("own", INC_1), ("earlier", INC_2)] {
        let harness = Harness::new(&format!("held-foreign-{tag}"));
        let owner = Owner::new(RUN_C, incarnation, REPO_KEY_B);
        harness.liveness.set_live(&owner.run_dir);
        let container = seed(
            &harness.root,
            &harness.runtime,
            &owner,
            &shell_probe(),
            Present::Both,
            Liveness::Running,
        );

        let complete = harness
            .census(&resume(RUN_A, INC_1))
            .expect("a live foreign owner is skipped, whatever its incarnation");
        assert!(complete.report().was_untouched(&container), "[{tag}]");
        assert_eq!(
            complete
                .report()
                .untouched
                .iter()
                .map(|entry| entry.ownership)
                .collect::<Vec<_>>(),
            vec![Ownership::ForeignRunLiveOwner],
            "[{tag}]"
        );
        assert!(
            harness.holds(&container),
            "[{tag}] a live owner's container was killed"
        );
        assert!(harness.intent_exists(&container), "[{tag}]");
        assert!(harness.view_exists(&container), "[{tag}]");
        assert!(
            harness.trace.sites().is_empty(),
            "[{tag}] the funnel touched a live owner's container: {:#?}",
            harness.trace.rendered()
        );
    }
}

#[test]
fn a_dead_foreign_owners_container_naming_this_incarnation_is_reclaimed() {
    for (tag, lock_held) in [("free", false), ("held", true)] {
        let harness = Harness::new(&format!("dead-foreign-own-incarnation-{tag}"));
        let owner = Owner::new(RUN_C, INC_1, REPO_KEY_B);
        if lock_held {
            harness.liveness.set_live(&owner.run_dir);
        }
        let container = seed(
            &harness.root,
            &harness.runtime,
            &owner,
            &shell_probe(),
            Present::Both,
            Liveness::Running,
        );

        let complete = harness
            .census(&resume(RUN_A, INC_1))
            .expect("arm (ii) classifies by the owner's lock, whatever the incarnation");
        let report = complete.report();
        assert_eq!(
            report.was_untouched(&container),
            lock_held,
            "[{tag}] the report disagrees with the owner's lock"
        );
        assert_eq!(
            harness.holds(&container),
            lock_held,
            "[{tag}] a dead owner's container survived, or a live owner's was killed"
        );
        assert_eq!(
            harness.intent_exists(&container),
            lock_held,
            "[{tag}] the intent record and the container disagree"
        );
        assert_eq!(
            report
                .reclaimed
                .iter()
                .map(|entry| entry.ownership)
                .collect::<Vec<_>>(),
            if lock_held {
                Vec::new()
            } else {
                vec![Ownership::ForeignRunDeadOwner]
            },
            "[{tag}]"
        );
        assert_eq!(
            harness.liveness.asked(),
            vec![owner.run_dir.clone()],
            "[{tag}] arm (ii) reached without probing the owner's run.lock"
        );
    }
}

#[test]
fn a_labeled_container_this_census_cannot_own_blocks_admission() {
    use crate::runner::container::runtime::DiscoveredContainer;
    use std::collections::BTreeMap;

    let owner = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let good = owner.name(&shell_probe());

    let cases: [(&str, &str, Option<&str>, &str); 4] = [
        (
            "a name no funnel could have written",
            "someone-elses-container",
            None,
            "not a upstroke container name",
        ),
        (
            LABEL_RUN,
            good.as_str(),
            Some(LABEL_RUN),
            "blocks admission",
        ),
        (
            LABEL_INCARNATION,
            good.as_str(),
            Some(LABEL_INCARNATION),
            "blocks admission",
        ),
        (
            LABEL_RUN_DIR,
            good.as_str(),
            Some(LABEL_RUN_DIR),
            "blocks admission",
        ),
    ];

    let mut messages = BTreeSet::new();
    for (what, name, withheld, needle) in cases {
        let harness = Harness::new(&format!("unownable-{}", what.replace('.', "-")));
        let mut labels = BTreeMap::new();
        labels.insert(
            LABEL_PRIVATE_ROOT.to_owned(),
            private_root_label(&harness.root),
        );
        labels.insert(LABEL_RUN.to_owned(), RUN_B.to_owned());
        labels.insert(LABEL_INCARNATION.to_owned(), INC_1.to_owned());
        labels.insert(
            LABEL_RUN_DIR.to_owned(),
            "/repo/.upstroke/runs/x".to_owned(),
        );
        if let Some(key) = withheld {
            labels.remove(key);
        }
        let container = DiscoveredContainer {
            name: name.to_owned(),
            labels,
        };
        harness.runtime.seed_container(
            &container.name,
            container.labels.clone(),
            IMAGE_ID,
            IMAGE_ID,
            Liveness::Running,
        );
        let error = harness
            .census(&fresh(INC_2))
            .expect_err("an unownable labeled container blocks admission");
        let message = refusal(&error);
        assert!(message.contains(needle), "{what}: {message}");
        assert!(
            harness.trace.sites().is_empty(),
            "{what}: the refusal came after an effect"
        );
        messages.insert(message);
    }
    assert_eq!(
        messages.len(),
        4,
        "four distinct causes must give four distinct diagnostics, or the operator cannot \
         tell which label is missing"
    );
}

#[test]
fn a_name_that_disagrees_with_its_own_record_refuses() {
    let owner = Owner::new(RUN_A, INC_1, REPO_KEY_A);
    let name = owner.name(&shell_probe());
    let cases: [(&str, ContainerIntent, &str); 3] = [
        (
            "run id",
            {
                let mut record = owner.record(&shell_probe());
                record.run_id = RUN_B.to_owned();
                record
            },
            "named for run",
        ),
        (
            "incarnation",
            {
                let mut record = owner.record(&shell_probe());
                record.incarnation = INC_2.to_owned();
                record
            },
            "named for incarnation",
        ),
        (
            "repo key",
            {
                let mut record = owner.record(&shell_probe());
                record.repo_key = REPO_KEY_B.to_owned();
                record
            },
            "named for repo key",
        ),
    ];

    let mut seen = BTreeSet::new();
    for (what, record, needle) in cases {
        let harness = Harness::new(&format!("name-disagrees-{}", what.replace(' ', "-")));
        let mut hooks = RecordingHooks::new(ContainerTrace::off());
        write_intent(
            &mut hooks,
            ContainerSite::WriteIntent,
            &harness.root,
            &name,
            &record,
        )
        .expect("a record that disagrees with the name it is filed under");
        let error = harness
            .census(&fresh(INC_3))
            .expect_err("a record disagreeing with its own name is not ownership evidence");
        let message = refusal(&error);
        assert!(message.contains(needle), "{what}: {message}");
        seen.insert(needle);
    }
    assert_eq!(seen.len(), 3, "three components, three diagnostics");
}

#[test]
fn labels_and_a_record_that_disagree_about_the_owner_refuse() {
    let owner = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    for (key, forged) in [(LABEL_RUN, RUN_C), (LABEL_INCARNATION, INC_2)] {
        let harness = Harness::new(&format!("label-disagrees-{}", key.replace('.', "-")));
        let name = owner.name(&shell_probe());
        let record = owner.record(&shell_probe());
        let mut hooks = RecordingHooks::new(ContainerTrace::off());
        write_intent(
            &mut hooks,
            ContainerSite::WriteIntent,
            &harness.root,
            &name,
            &record,
        )
        .expect("write the intent");
        let mut labels = record.labels(&harness.root);
        labels.insert(key.to_owned(), forged.to_owned());
        harness.runtime.seed_container(
            name.as_str(),
            labels,
            IMAGE_ID,
            IMAGE_ID,
            Liveness::Running,
        );

        let error = harness
            .census(&fresh(INC_3))
            .expect_err("labels and record disagree");
        let message = refusal(&error);
        assert!(message.contains(key), "{message}");
        assert!(message.contains("will not choose"), "{message}");
        assert!(
            harness.trace.sites().is_empty(),
            "refused before any effect"
        );
    }
}

#[test]
fn the_stable_prefix_barrier_refuses_each_of_its_four_predicates_independently() {
    const PREFIX: &[u8] = b"{\"event\":\"run_started\"}\n";
    const PREFIX_SHA: &str = "2f9864f5b2e0acc40bf4a8b9fb5ae52b142cdcd0870db42ddcac489991b5206d";
    const LONGER: &[u8] = b"{\"event\":\"run_started\"}\n{\"event\":\"attempt_started\"}\n";
    const LONGER_SHA: &str = "9f6a5ec6a50778f18bc1fc9b3ff2286a43c4130479cf391cf321743450e5acc8";
    const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    let measured = PrefixBytes::of(PREFIX);
    assert_eq!(measured.len, 24);
    assert_eq!(measured.sha256, PREFIX_SHA);
    assert_eq!(PrefixBytes::of(LONGER).sha256, LONGER_SHA);
    assert_eq!(PrefixBytes::of(b"").sha256, EMPTY_SHA);

    let healthy = || PrefixReread {
        first: measured.clone(),
        second: measured.clone(),
    };
    let synced = PrefixSync {
        synced_len: measured.len,
    };
    let replayed = || PrefixReplay {
        replayed: measured.clone(),
    };

    let established =
        StablePrefixBarrier::establish(synced, &healthy(), &replayed()).expect("a healthy barrier");
    assert_eq!(established.boundary(), 24);
    assert_eq!(established.digest(), PREFIX_SHA);

    let mut reasons = BTreeSet::new();

    let mut moved = healthy();
    moved.second = PrefixBytes::of(LONGER);
    let message = refusal(
        &StablePrefixBarrier::establish(synced, &moved, &replayed()).expect_err("boundary moved"),
    );
    assert!(
        message.contains("bytes AND boundary unchanged"),
        "{message}"
    );
    reasons.insert("boundary");

    let mut rewritten = healthy();
    rewritten.second = PrefixBytes {
        len: measured.len,
        sha256: LONGER_SHA.to_owned(),
    };
    let message = refusal(
        &StablePrefixBarrier::establish(synced, &rewritten, &replayed())
            .expect_err("bytes changed under a stable boundary"),
    );
    assert!(message.contains("proves the prefix stable"), "{message}");
    reasons.insert("bytes");

    let message = refusal(
        &StablePrefixBarrier::establish(
            PrefixSync {
                synced_len: measured.len - 1,
            },
            &healthy(),
            &replayed(),
        )
        .expect_err("the prefix is not synced to its boundary"),
    );
    assert!(message.contains("is not durable"), "{message}");
    reasons.insert("synced");

    let message = refusal(
        &StablePrefixBarrier::establish(
            synced,
            &healthy(),
            &PrefixReplay {
                replayed: PrefixBytes::of(LONGER),
            },
        )
        .expect_err("the replay was of other bytes"),
    );
    assert!(message.contains("exactly the reread bytes"), "{message}");
    reasons.insert("replayed");

    assert_eq!(
        reasons.len(),
        4,
        "four predicates, four distinct refusals; a barrier that checked three would pass a \
         suite that only counted that it refused"
    );

    assert!(
        StablePrefixBarrier::establish(
            synced,
            &healthy(),
            &PrefixReplay {
                replayed: PrefixBytes {
                    len: measured.len,
                    sha256: EMPTY_SHA.to_owned(),
                },
            },
        )
        .is_err()
    );
}

#[test]
fn both_halves_of_discovery_are_scanned_and_every_cell_is_classified() {
    let harness = Harness::new("both-halves");
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let invocations = [
        InvocationId::probe(ProbeTarget::Shell, 0).expect("shell probe 0"),
        InvocationId::probe(ProbeTarget::Shell, 1).expect("shell probe 1"),
        agent_probe(),
    ];
    let cells = [Present::Both, Present::IntentOnly, Present::LabelOnly];
    let mut expected = Vec::new();
    for (invocation, present) in invocations.iter().zip(cells) {
        let name = seed(
            &harness.root,
            &harness.runtime,
            &dead,
            invocation,
            present,
            Liveness::Running,
        );
        expected.push((
            name,
            match present {
                Present::Both => DiscoveredBy::IntentAndLabel,
                Present::IntentOnly | Present::IntentAndViewAfterReaper => DiscoveredBy::IntentOnly,
                Present::LabelOnly => DiscoveredBy::LabelOnly,
            },
        ));
    }

    let complete = harness.census(&fresh(INC_2)).expect("the census completes");
    let report = complete.report();
    assert_eq!(report.reclaimed.len(), 3);
    for (name, discovered_by) in &expected {
        let entry = report
            .reclaimed
            .iter()
            .find(|entry| &entry.name == name)
            .unwrap_or_else(|| panic!("`{name}` was not reclaimed: {:#?}", report.reclaimed));
        assert_eq!(entry.discovered_by, *discovered_by);
        assert!(!harness.holds(name) && !harness.intent_exists(name));
    }
    let cells: BTreeSet<DiscoveredBy> = report
        .reclaimed
        .iter()
        .map(|entry| entry.discovered_by)
        .collect();
    assert_eq!(
        cells.into_iter().collect::<Vec<_>>(),
        DiscoveredBy::ALL.to_vec(),
        "the fixture reached every cell the enum declares"
    );

    let empty = Harness::new("neither-half");
    let complete = empty.census(&fresh(INC_2)).expect("an empty namespace");
    assert!(complete.report().reclaimed.is_empty());
}

#[test]
fn the_private_root_label_this_census_filters_on_is_the_one_the_intent_writes() {
    const EXPECTED: &[(&str, &str)] = &[
        ("/srv/upstroke/private", "/srv/upstroke/private"),
        ("/tmp/a b/c", "/tmp/a%20b/c"),
        ("/srv/a;b", "/srv/a%3Bb"),
        ("/srv/a,b", "/srv/a%2Cb"),
        ("/srv/a=b", "/srv/a%3Db"),
        ("/srv/a%b", "/srv/a%25b"),
        ("/srv/a%5Cb", "/srv/a%255Cb"),
        ("/srv/caf\u{e9}", "/srv/caf%C3%A9"),
    ];
    for (root, expected) in EXPECTED {
        let root = PathBuf::from(root);
        assert_eq!(
            private_root_label(&root),
            *expected,
            "the label encoding of {} moved",
            root.display()
        );
        let record = Owner::new(RUN_A, INC_1, REPO_KEY_A).record(&shell_probe());
        let written = record.labels(&root);
        assert_eq!(
            written.get(LABEL_PRIVATE_ROOT).map(String::as_str),
            Some(*expected),
            "the census's filter value and the funnel's label disagree for {}",
            root.display()
        );
    }

    let backslash = private_root_label(Path::new(r"/srv/a\b"));
    if cfg!(windows) {
        assert_eq!(backslash, "/srv/a/b");
    } else {
        assert_eq!(backslash, "/srv/a%5Cb");
    }
    let record = Owner::new(RUN_A, INC_1, REPO_KEY_A).record(&shell_probe());
    assert_eq!(
        record
            .labels(Path::new(r"/srv/a\b"))
            .get(LABEL_PRIVATE_ROOT)
            .map(String::as_str),
        Some(backslash.as_str())
    );
}

#[test]
fn the_private_root_label_is_injective_over_hostile_roots() {
    let base = Path::new("/srv/private");

    let universal = [
        "a/b",
        "a%5Cb",
        "a%2Fb",
        "a;b",
        "a,b",
        "a=b",
        "a b",
        "a%b",
        "a%25b",
        "caf\u{e9}",
        "a\u{fffd}b",
        "ab",
    ];
    let roots: Vec<PathBuf> = universal.iter().map(|leaf| base.join(leaf)).collect();
    assert_eq!(
        roots.iter().collect::<BTreeSet<_>>().len(),
        universal.len(),
        "the fixture itself must carry {} distinct roots",
        universal.len()
    );
    let labels: BTreeSet<String> = roots.iter().map(|root| private_root_label(root)).collect();
    assert_eq!(
        labels.len(),
        universal.len(),
        "{} distinct roots rendered to {} distinct labels; a census authorized for one of the \
         colliding roots queries and reclaims the other's containers: {:#?}",
        universal.len(),
        labels.len(),
        roots
            .iter()
            .map(|root| (root.display().to_string(), private_root_label(root)))
            .collect::<Vec<_>>()
    );

    #[cfg(unix)]
    {
        let with_backslash = base.join(r"a\b");
        let with_slash = base.join("a").join("b");
        assert_ne!(with_backslash, with_slash);
        assert_ne!(
            private_root_label(&with_backslash),
            private_root_label(&with_slash),
            "`<base>/a\\b` and `<base>/a/b` are different directories on Unix and must not be \
             one world"
        );
    }
    #[cfg(windows)]
    {
        assert_eq!(
            private_root_label(&base.join(r"a\b")),
            private_root_label(&base.join("a").join("b")),
            "on Windows both spellings name one directory, so one label is correct"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let ill_formed: Vec<PathBuf> = [b"/srv/private/a\xffb".as_slice(), b"/srv/private/a\xfeb"]
            .iter()
            .map(|bytes| PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
            .collect();
        let mut rendered: BTreeSet<String> = ill_formed
            .iter()
            .map(|root| private_root_label(root))
            .collect();
        assert_eq!(rendered.len(), 2, "two ill-formed roots, two labels");
        rendered.insert(private_root_label(&base.join("a\u{fffd}b")));
        assert_eq!(
            rendered.len(),
            3,
            "an ill-formed root and a root that really contains U+FFFD are different roots"
        );
    }

    for root in &roots {
        let label = private_root_label(root);
        assert!(
            !label.contains([',', '=', '\n', '\r']),
            "`{label}` would change what `--filter label=…` selects"
        );
    }
}

#[test]
fn every_topology_write_command_performs_the_census() {
    let mut reclaimed_by = Vec::new();
    for command in WriteCommand::ALL {
        let harness = Harness::new(&format!("write-command-{}", command.name()));
        let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
        let name = seed(
            &harness.root,
            &harness.runtime,
            &dead,
            &shell_probe(),
            Present::Both,
            Liveness::Running,
        );
        let start = match command {
            WriteCommand::Run => fresh(INC_2),
            WriteCommand::Resume => resume(RUN_A, INC_2),
        };
        let complete = harness.census(&start).expect("the census completes");
        assert_eq!(complete.report().command, *command);
        assert_eq!(complete.report().reclaimed.len(), 1);
        assert!(!harness.holds(&name));
        reclaimed_by.push(*command);
    }
    assert_eq!(reclaimed_by, WriteCommand::ALL.to_vec());
    assert_eq!(WriteCommand::ALL.len(), 2);
}

#[test]
fn census_returns_the_only_token_that_reaches_a_consumer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let census_module = root.join("src/runner/container/census.rs");
    let mut offenders = Vec::new();
    let mut scanned = 0;
    let mut scanned_bytes = 0_usize;
    for path in walk(&root.join("src")) {
        let source = fs::read_to_string(&path).expect("read source");
        let production = crate::effects::production_code(&source);
        let dense = production
            .as_bytes()
            .iter()
            .filter(|byte| !byte.is_ascii_whitespace())
            .count();
        assert!(
            dense > 0,
            "{}'s region is empty, so it contributes nothing to the count below",
            path.display()
        );
        scanned += 1;
        scanned_bytes += dense;
        if path == census_module {
            continue;
        }
        if constructs_the_token(&production) {
            offenders.push(path.display().to_string());
        }
    }
    assert!(scanned > 20, "the walk found the tree: {scanned}");
    assert!(
        scanned_bytes > 1_000_000,
        "the {scanned} regions hold {scanned_bytes} non-whitespace bytes between them; a file \
         count passes with every region empty and this is what does not"
    );
    assert!(
        offenders.is_empty(),
        "`CensusComplete` is constructed outside the census: {offenders:#?}"
    );

    let production = crate::effects::production_code(
        &fs::read_to_string(&census_module).expect("the census module"),
    );
    assert_eq!(
        production.matches("Ok(CensusComplete {").count(),
        1,
        "the census constructs its token exactly once, so the scan above is measuring \
         something"
    );
    assert_eq!(production.matches("CensusComplete {").count(), 3);

    let harness = Harness::new("token-shape");
    let complete = harness.census(&fresh(INC_1)).expect("an empty census");
    assert_eq!(complete.report().incarnation, INC_1);
    assert_eq!(complete.report().orphan_window, super::orphan_window());
}

fn constructs_the_token(production: &str) -> bool {
    production.match_indices("CensusComplete {").any(|(at, _)| {
        let before = production[..at].trim_end();
        let before = before.strip_suffix('&').unwrap_or(before);
        !before.trim_end().ends_with("->")
    })
}

#[test]
fn the_token_scan_excuses_a_return_position_and_nothing_else() {
    for excused in [
        "fn containers(&self) -> &CensusComplete {",
        "fn into_inner(self) -> CensusComplete {",
        "    pub fn report(&self) -> &CensusComplete  {",
    ] {
        assert!(
            !constructs_the_token(excused),
            "a return position was read as a construction: {excused}"
        );
    }

    for construction in [
        "let token = CensusComplete { report };",
        "consume(&CensusComplete { report });",
        "Ok(CensusComplete { report })",
        "Self::CensusComplete { report }",
    ] {
        assert!(
            constructs_the_token(construction),
            "a construction was excused: {construction}"
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

#[test]
fn r20_is_persistent_output_in_every_at_run_end_outcome_and_no_census_path_touches_it() {
    const AT_RUN_END: &[(&str, &str)] = &[
        ("Complete", "persistent_output"),
        ("Parked", "persistent_output"),
        ("Halted", "persistent_output"),
        ("BudgetExceeded", "persistent_output"),
        ("NoRunFinished", "persistent_output"),
    ];
    assert_eq!(AT_RUN_END.len(), 5);
    let dispositions: BTreeSet<&str> = AT_RUN_END.iter().map(|(_, value)| *value).collect();
    assert_eq!(
        dispositions.into_iter().collect::<Vec<_>>(),
        vec!["persistent_output"],
        "R20 is operator-owned in every outcome; a row with a `pruned` cell is a row a run \
         may clean up"
    );

    let harness = Harness::new("r20-untouched");
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    harness.runtime.add_volume("upstroke-claude-code");
    for invocation in [shell_probe(), agent_probe()] {
        seed(
            &harness.root,
            &harness.runtime,
            &dead,
            &invocation,
            Present::Both,
            Liveness::Running,
        );
    }
    let complete = harness.census(&fresh(INC_2)).expect("the census completes");
    assert_eq!(complete.report().reclaimed.len(), 2);
    assert!(
        !harness.trace.ops().contains(&RuntimeOp::InspectVolume),
        "a census inspected a volume: {:?}",
        harness.trace.ops()
    );
    assert!(
        harness
            .runtime
            .volume_present("upstroke-claude-code")
            .expect("ask the runtime"),
        "the volume this census reclaimed containers around is still there"
    );

    let production =
        crate::effects::blank_comments_and_strings(&crate::effects::production_region(
            &fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runner/container/census.rs"),
            )
            .expect("the census module"),
        ));
    for needle in ["volume_present", "add_volume", "remove_volume"] {
        assert!(!production.contains(needle), "the census names `{needle}`");
    }
}

#[test]
fn r26_is_released_in_four_outcomes_and_the_census_is_the_mechanism_for_no_run_finished() {
    const AT_RUN_END: &[(&str, &str)] = &[
        ("Complete", "released"),
        ("Parked", "released"),
        ("Halted", "released"),
        ("BudgetExceeded", "released"),
        ("NoRunFinished", "reclaimed at the next write-command start"),
    ];
    assert_eq!(AT_RUN_END.len(), 5);
    assert_eq!(
        AT_RUN_END
            .iter()
            .filter(|(_, value)| *value == "released")
            .count(),
        4,
        "a container surviving a park or a budget stop keeps spending while the run is \
         supposed to be quiescent"
    );

    let harness = Harness::new("r26-no-run-finished");
    let never_finished = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let name = seed(
        &harness.root,
        &harness.runtime,
        &never_finished,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    assert!(
        harness.view_exists(&name),
        "R19's directory is there to prune"
    );
    harness.census(&fresh(INC_2)).expect("the census completes");
    assert!(!harness.holds(&name), "R26: the container");
    assert!(!harness.view_exists(&name), "R19: the view");
    assert!(!harness.intent_exists(&name), "R26: the intent record");
    assert!(
        fs::read_dir(containers_dir(&harness.root))
            .expect("the namespace")
            .next()
            .is_none(),
        "the ledgers balance: nothing is left in the namespace"
    );
}

#[test]
fn a_container_that_never_terminates_exhausts_the_bounded_observation_and_refuses() {
    let root = scratch("never-terminates");
    let trace = ContainerTrace::recording();
    let inner = Arc::new(FakeRuntime::new(trace.clone()));
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let name = seed(
        &root,
        &inner,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    let wedged = WedgedRuntime {
        inner: Arc::clone(&inner),
    };
    let liveness = RecordingLiveness::new();
    let view = DisposableDirView::new(trace.clone());
    let mut hooks = RecordingHooks::new(trace.clone());
    let start = fresh(INC_2);
    let error = run_startup_census(
        &mut hooks,
        &Census {
            private_root: &root,
            start: &start,
            runtime: &wedged,
            liveness: &liveness,
            view: &view,
        },
    )
    .expect_err("a container that never terminates cannot be reclaimed");
    let message = refusal(&error);
    assert!(
        message.contains(&format!("after {TERMINATION_OBSERVATIONS} observations")),
        "{message}"
    );
    assert!(message.contains("blocks admission"), "{message}");
    assert_eq!(
        trace
            .ops()
            .into_iter()
            .filter(|op| *op == RuntimeOp::Observe)
            .count(),
        TERMINATION_OBSERVATIONS,
        "the wait is bounded: exactly the declared number of observations, not a spin"
    );
    assert!(
        trace.position_starting("rt:remove:").is_none(),
        "`docker rm` was issued before termination was proven: {:#?}",
        trace.rendered()
    );
    assert!(inner.container(name.as_str()).is_some());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_reapers_container_selector_names_the_incarnation_and_not_the_root_alone() {
    let roots = [Path::new("/srv/a"), Path::new("/srv/b")];
    let incarnations = [INC_1, INC_2];
    let mut rendered = BTreeSet::new();
    for root in roots {
        for incarnation in incarnations {
            let scope = super::ReaperContainerScope::new("/usr/bin/docker", root, incarnation)
                .expect("a scope");
            let argv = scope.list_argv();
            assert_eq!(argv[0], "/usr/bin/docker");
            assert_eq!(argv[1], "ps");
            assert!(
                argv.contains(&"--all".to_owned()),
                "an exited container still holds its name, its labels and its layer: {argv:?}"
            );
            let filters: Vec<&String> = argv
                .iter()
                .filter(|argument| argument.starts_with("label="))
                .collect();
            assert_eq!(filters.len(), 2, "{argv:?}");
            assert!(filters.contains(&&format!(
                "label={LABEL_PRIVATE_ROOT}={}",
                private_root_label(root)
            )));
            assert!(filters.contains(&&format!("label={LABEL_INCARNATION}={incarnation}")));
            assert_eq!(
                filters.iter().collect::<BTreeSet<_>>().len(),
                2,
                "two filters carrying one value is one filter"
            );
            rendered.insert(argv.join(" "));
        }
    }
    assert_eq!(
        rendered.len(),
        4,
        "two roots and two incarnations, varied independently, must give four distinct \
         selectors; a selector that dropped either component gives two"
    );

    let scope = super::ReaperContainerScope::new("docker", roots[0], INC_1).expect("a scope");
    assert_eq!(scope.kill_argv("abc"), vec!["docker", "kill", "abc"]);
    assert_eq!(
        scope.remove_argv("abc"),
        vec!["docker", "rm", "--force", "--volumes", "abc"]
    );
    assert_eq!(scope.program(), Path::new("docker"));
}

#[test]
fn a_reaper_scope_whose_label_value_could_widen_the_filter_cannot_reach_the_reaper() {
    let good_root = Path::new("/srv/private");
    let hostile = ["", "01KZ\nlabel=upstroke.run", "a,b", "a=b"];
    assert_eq!(
        hostile.iter().collect::<BTreeSet<_>>().len(),
        4,
        "four distinct hostile values"
    );
    for value in hostile {
        assert!(
            super::ReaperContainerScope::new("docker", good_root, value).is_err(),
            "`{value}` was accepted as an incarnation"
        );
    }

    assert!(
        super::ReaperContainerScope::new("docker", Path::new(""), INC_1).is_err(),
        "an empty private root renders an empty filter value"
    );
    for value in ["01KZ\nlabel=upstroke.run", "a,b", "a=b"] {
        let scope = super::ReaperContainerScope::new("docker", Path::new(value), INC_1)
            .unwrap_or_else(|error| {
                panic!("`{value}` is a legal directory name and was refused: {error}")
            });
        let argv = scope.list_argv();
        let filters: Vec<&String> = argv
            .iter()
            .filter(|argument| argument.starts_with(LABEL_FILTER))
            .collect();
        assert_eq!(
            filters.len(),
            2,
            "`{value}` produced {filters:?}, not two filters"
        );
        assert!(
            filters.contains(&&format!(
                "{LABEL_FILTER}{LABEL_PRIVATE_ROOT}={}",
                private_root_label(Path::new(value))
            )),
            "{argv:?}"
        );
        assert!(
            filters.contains(&&format!("{LABEL_FILTER}{LABEL_INCARNATION}={INC_1}")),
            "{argv:?}"
        );
    }

    assert!(super::ReaperContainerScope::new("docker", good_root, INC_1).is_ok());
}

fn real_docker_census_owners() -> (Owner, Owner) {
    const REAL_RUN_LIVE: &str = "01KZTREALLIVE00000000000AA";
    const REAL_RUN_DEAD: &str = "01KZTREALDEAD00000000000BB";
    let key = crate::runner::container::fake::slot_repo_key();
    (
        Owner::new(REAL_RUN_LIVE, INC_1, key),
        Owner::new(REAL_RUN_DEAD, INC_2, key),
    )
}

#[test]
fn the_gated_censuss_names_are_scoped_to_this_build_slot() {
    let (live, dead) = real_docker_census_owners();
    let names = [live.name(&shell_probe()), dead.name(&shell_probe())];
    let borrowed: Vec<&crate::runner::container::intent::ContainerName> = names.iter().collect();

    assert_ne!(
        names[0], names[1],
        "the two owners build one name, so the census creates one container and its \
         dead-versus-live comparison has nothing to compare"
    );
    assert!(
        crate::runner::container::fake::unscoped_names(&borrowed).is_empty(),
        "a name this test's pre-clean will kill is one another build slot's suite asks for too, \
         so the kill lands on that suite's live container: {:?}",
        crate::runner::container::fake::unscoped_names(&borrowed)
    );
}

#[test]
fn real_docker_census_reclaims_a_dead_owner_and_spares_a_live_one() {
    let trace = ContainerTrace::recording();
    let docker = match crate::runner::container::docker_gate(
        "real_docker_census_reclaims_a_dead_owner_and_spares_a_live_one",
        trace.clone(),
    ) {
        Ok(docker) => docker,
        Err(reason) => {
            assert_eq!(
                reason,
                crate::runner::container::fake::absent_reason(),
                "a Docker-gated test skipped for a reason the gate does not know about"
            );
            return;
        }
    };
    let image = ["alpine:3.20", "busybox:latest", "debian:stable-slim"]
        .into_iter()
        .find_map(|reference| docker.image_by_reference(reference).ok().flatten());
    let Some(image) = image else {
        assert!(
            std::env::var_os(crate::runner::container::fake::REQUIRE_DOCKER).is_none(),
            "UPSTROKE_REQUIRE_DOCKER is set and the runtime holds none of the images these \
             tests may use; they never pull (non_goals[1])"
        );
        return;
    };

    let root = scratch("real-docker-census");
    let (live, dead) = real_docker_census_owners();
    let liveness = RecordingLiveness::new();
    liveness.set_live(&live.run_dir);

    crate::runner::container::fake::preclean_names(
        docker.as_ref(),
        &DisposableDirView::new(ContainerTrace::off()),
        &root,
        &[&live.name(&shell_probe()), &dead.name(&shell_probe())],
    );

    let mut names = Vec::new();
    for owner in [&live, &dead] {
        let name = owner.name(&shell_probe());
        let record = owner.record(&shell_probe());
        let mut hooks = RecordingHooks::new(ContainerTrace::off());
        write_intent(
            &mut hooks,
            ContainerSite::WriteIntent,
            &root,
            &name,
            &record,
        )
        .expect("write the intent");
        let plan = crate::runner::container::LaunchPlan {
            private_root: root.clone(),
            name: name.clone(),
            invocation: shell_probe(),
            intent: record.clone(),
            spec: crate::runner::container::runtime::CreateSpec {
                name: name.as_str().to_owned(),
                image_id: image.id.clone(),
                labels: record.labels(&root),
                mounts: Vec::new(),
                env: Vec::new(),
                command: vec!["sleep".to_owned(), "120".to_owned()],
                workdir: None,
                read_only_root: true,
            },
            view: crate::runner::container::GitViewRequest {
                path: view_path(&root, &name),
                workspace: root.clone(),
                head: None,
            },
        };
        let view = DisposableDirView::new(ContainerTrace::off());
        let mut hooks = RecordingHooks::new(ContainerTrace::off());
        crate::runner::container::launch(&mut hooks, docker.as_ref(), &view, &plan)
            .expect("launch a real container from the recorded image id");
        names.push(name);
    }

    let view = DisposableDirView::new(trace.clone());
    let mut hooks = RecordingHooks::new(trace.clone());
    let start = fresh(INC_3);
    let outcome = run_startup_census(
        &mut hooks,
        &Census {
            private_root: &root,
            start: &start,
            runtime: docker.as_ref(),
            liveness: &liveness,
            view: &view,
        },
    );

    let cleanup = |name: &ContainerName| {
        let view = DisposableDirView::new(ContainerTrace::off());
        let mut hooks = RecordingHooks::new(ContainerTrace::off());
        let _ = crate::runner::container::reclaim(
            &mut hooks,
            docker.as_ref(),
            &view,
            &root,
            name,
            Some(&view_path(&root, name)),
        );
    };
    let report = match &outcome {
        Ok(complete) => complete.report().clone(),
        Err(error) => {
            for name in &names {
                cleanup(name);
            }
            let _ = fs::remove_dir_all(&root);
            panic!("the census refused against real Docker: {error}");
        }
    };
    let live_name = names[0].clone();
    let dead_name = names[1].clone();
    let live_still_there = docker
        .observe(live_name.as_str())
        .expect("observe the live owner's container");
    let dead_gone = docker
        .observe(dead_name.as_str())
        .expect("observe the dead owner's container");
    for name in &names {
        cleanup(name);
    }
    let _ = fs::remove_dir_all(&root);

    assert_eq!(report.reclaimed.len(), 1, "{report:#?}");
    assert_eq!(report.reclaimed[0].name, dead_name);
    assert_eq!(
        report.reclaimed[0].boundary,
        Boundary::FromIntent(POLICY_A.to_owned())
    );
    assert_eq!(report.untouched.len(), 1);
    assert_eq!(report.untouched[0].name, live_name);
    assert_eq!(
        dead_gone,
        Liveness::Gone,
        "the real runtime still holds the dead owner's container"
    );
    assert_eq!(
        live_still_there,
        Liveness::Running,
        "a live owner's real container was stopped by a foreign census"
    );
}

#[cfg(unix)]
#[test]
fn a_record_that_vanishes_between_the_scan_and_the_read_is_skipped() {
    let harness = Harness::new("vanishing-record");
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let real = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    let vanished = dead.name(&agent_probe());
    let path = vanished.intent_path(&harness.root);
    std::os::unix::fs::symlink(harness.root.join("this-record-is-gone"), &path)
        .expect("a dangling entry in the namespace");
    assert!(fs::symlink_metadata(&path).is_ok(), "read_dir will list it");
    assert_eq!(
        fs::read(&path)
            .expect_err("and reading it answers NotFound")
            .kind(),
        std::io::ErrorKind::NotFound,
        "the fixture must produce the losing reclaimer's exact answer"
    );

    let complete = harness
        .census(&fresh(INC_2))
        .expect("a record another reclaimer removed is not a reason to refuse a write command");
    let report = complete.report();
    assert_eq!(report.reclaimed.len(), 1, "{:#?}", report.reclaimed);
    assert_eq!(report.reclaimed[0].name, real);
    assert!(!harness.holds(&real));

    let malformed = Harness::new("malformed-record");
    let torn = dead.name(&shell_probe());
    let torn_path = torn.intent_path(&malformed.root);
    fs::create_dir_all(torn_path.parent().expect("the namespace")).expect("namespace");
    fs::write(&torn_path, b"{ this is not a container intent").expect("a damaged record");
    assert!(
        malformed.census(&fresh(INC_2)).is_err(),
        "a damaged record was treated as an absent one"
    );

    let protected = Harness::new("unreadable-record");
    let locked = dead.name(&agent_probe());
    let locked_path = locked.intent_path(&protected.root);
    fs::create_dir_all(locked_path.parent().expect("the namespace")).expect("namespace");
    fs::write(&locked_path, b"{}").expect("a record");
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o000))
            .expect("make the record unreadable");
    }
    let outcome = protected.census(&fresh(INC_2));
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o600));
    }
    assert!(
        outcome.is_err(),
        "a record that is THERE and cannot be read was treated as one that is gone; the \
         already-gone tolerance is about a delete in flight, not about every PermissionDenied"
    );
}

fn colliding_run_dir_pairs() -> Vec<(&'static str, PathBuf, PathBuf)> {
    let mut pairs: Vec<(&'static str, PathBuf, PathBuf)> = Vec::new();
    pairs.extend([
        (
            "a colon, which the reviewer's mutation rewrote next",
            PathBuf::from("/repo/.upstroke/runs/A:B"),
            PathBuf::from("/repo/.upstroke/runs/A/B"),
        ),
        (
            "a comma beside its own escape: `%` must escape itself",
            PathBuf::from("/repo/a,b/.upstroke/runs/X"),
            PathBuf::from("/repo/a%2Cb/.upstroke/runs/X"),
        ),
        (
            "a literal percent beside its escape",
            PathBuf::from("/repo/a%b/.upstroke/runs/X"),
            PathBuf::from("/repo/a%25b/.upstroke/runs/X"),
        ),
    ]);
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;
        pairs.extend([
            (
                "a backslash is an ordinary filename byte on Unix",
                PathBuf::from(r"/repo\a/.upstroke/runs/X"),
                PathBuf::from("/repo/a/.upstroke/runs/X"),
            ),
            (
                "and so is a backslash in the run id's own component",
                PathBuf::from(r"/repo/.upstroke/runs/A\B"),
                PathBuf::from("/repo/.upstroke/runs/A/B"),
            ),
            (
                "two ill-formed byte sequences, both `U+FFFD` under to_string_lossy",
                PathBuf::from(OsStr::from_bytes(b"/repo/.upstroke/runs/\xff")),
                PathBuf::from(OsStr::from_bytes(b"/repo/.upstroke/runs/\xfe")),
            ),
            (
                "an ill-formed sequence and a literal replacement character",
                PathBuf::from(OsStr::from_bytes(b"/repo/.upstroke/runs/\xff")),
                PathBuf::from("/repo/.upstroke/runs/\u{fffd}"),
            ),
        ]);
    }
    pairs
}

#[test]
fn the_recorded_run_directory_distinguishes_directories_a_lossy_rendering_merged() {
    let pairs = colliding_run_dir_pairs();
    let mut recorded: Vec<(String, PathBuf)> = Vec::new();
    for (what, left, right) in &pairs {
        assert_ne!(left, right, "{what}: the fixture's own pair is one path");
        let left_record = Owner::new(RUN_A, INC_1, REPO_KEY_A)
            .with_run_dir(left.clone())
            .record(&shell_probe());
        let right_record = Owner::new(RUN_A, INC_1, REPO_KEY_A)
            .with_run_dir(right.clone())
            .record(&shell_probe());
        assert_ne!(
            left_record.run_dir, right_record.run_dir,
            "{what}: two run directories recorded one string, so a census probes one run's lock \
             and reclaims the other's containers"
        );
        assert_eq!(
            &left_record.run_dir_path().expect("decodes"),
            left,
            "{what}"
        );
        assert_eq!(
            &right_record.run_dir_path().expect("decodes"),
            right,
            "{what}"
        );
        recorded.push((left_record.run_dir, left.clone()));
        recorded.push((right_record.run_dir, right.clone()));
    }
    let by_path: BTreeMap<&PathBuf, &String> = recorded
        .iter()
        .map(|(encoded, path)| (path, encoded))
        .collect();
    let distinct: BTreeSet<&&String> = by_path.values().collect();
    assert_eq!(
        distinct.len(),
        by_path.len(),
        "{} distinct run directories recorded {} distinct values: {:#?}",
        by_path.len(),
        distinct.len(),
        recorded
    );
    assert!(
        by_path.len() >= 6,
        "the table must carry more than one shape of collision, and it carries {}",
        by_path.len()
    );
    for (encoded, path) in &recorded {
        assert_eq!(
            &path_label(path),
            encoded,
            "the record and `intent::path_label` render one path two ways"
        );
    }
}

#[test]
#[cfg(unix)]
fn a_live_owner_under_a_hostile_run_directory_is_probed_where_it_actually_is() {
    let harness = Harness::new("hostile-run-dir-live-owner");
    let real = PathBuf::from(r"/repo\a/.upstroke/runs/B");
    let lossy = PathBuf::from("/repo/a/.upstroke/runs/B");
    assert_ne!(real, lossy);

    let live = Owner::new(RUN_B, INC_2, REPO_KEY_A).with_run_dir(real.clone());
    harness.liveness.set_live(&real);
    let held = seed(
        &harness.root,
        &harness.runtime,
        &live,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    let dead = Owner::new(RUN_C, INC_3, REPO_KEY_B);
    let orphan = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &agent_probe(),
        Present::Both,
        Liveness::Running,
    );

    let complete = harness.census(&fresh(INC_1)).expect("a foreign census");
    assert!(
        harness.liveness.asked().contains(&real),
        "the census never asked about the directory the owner actually locked: {:?}",
        harness.liveness.asked()
    );
    assert!(
        !harness.liveness.asked().contains(&lossy),
        "the census probed `{}`, a different real directory, where there is no lock: {:?}",
        lossy.display(),
        harness.liveness.asked()
    );
    assert!(
        complete.report().was_untouched(&held),
        "a live owner's container was reclaimed"
    );
    assert!(harness.holds(&held) && harness.intent_exists(&held));
    assert!(
        !harness.holds(&orphan),
        "the dead orphan beside it survived"
    );
}

#[test]
#[cfg(unix)]
fn a_label_only_container_under_a_hostile_run_directory_reaches_the_same_lock() {
    let harness = Harness::new("hostile-run-dir-label-only");
    let real = PathBuf::from(r"/repo\a/.upstroke/runs/B");
    let live = Owner::new(RUN_B, INC_2, REPO_KEY_A).with_run_dir(real.clone());
    harness.liveness.set_live(&real);
    let held = seed(
        &harness.root,
        &harness.runtime,
        &live,
        &shell_probe(),
        Present::LabelOnly,
        Liveness::Running,
    );

    let complete = harness.census(&fresh(INC_1)).expect("a foreign census");
    assert_eq!(
        harness.liveness.asked(),
        vec![real.clone()],
        "the label half of discovery probed a different directory than the record half"
    );
    assert!(complete.report().was_untouched(&held));
    assert_eq!(
        complete.report().untouched[0].discovered_by,
        DiscoveredBy::LabelOnly
    );
    assert!(harness.holds(&held));
}

#[test]
fn a_path_label_decodes_exactly_or_refuses() {
    let exact: &[(&str, &str)] = &[
        ("/repo/.upstroke/runs/X", "/repo/.upstroke/runs/X"),
        ("/repo%5Ca/runs/X", r"/repo\a/runs/X"),
        ("/repo/a%2Cb", "/repo/a,b"),
        ("/repo/a%3Db", "/repo/a=b"),
        ("/repo/a%25b", "/repo/a%b"),
        ("/repo/a%20b", "/repo/a b"),
        ("C:/repo/runs", "C:/repo/runs"),
        ("/repo/caf%C3%A9", "/repo/caf\u{e9}"),
    ];
    for (value, expected) in exact {
        assert_eq!(
            decode_path_label(value).expect("well formed"),
            PathBuf::from(expected),
            "`{value}`"
        );
        let backslash = expected.contains('\\');
        if !backslash || cfg!(unix) {
            assert_eq!(
                &path_label(Path::new(expected)).as_str(),
                value,
                "`{value}`"
            );
        } else {
            assert_eq!(
                path_label(Path::new(expected)),
                expected.replace('\\', "/"),
                "on Windows both spellings name one directory, so one label is correct"
            );
        }
    }

    for value in ["%", "%5", "%zz", "/repo/%g0/x", "/repo/x%", "/repo/%5c/x"] {
        let error = decode_path_label(value)
            .expect_err("a value no funnel could have written must be refused, not guessed at");
        assert!(
            error.to_string().contains(value),
            "the refusal must name the value: {error}"
        );
    }
    assert!(decode_path_label("/repo/%5c/x").is_err());
    assert!(decode_path_label("/repo/%5C/x").is_ok());
}

#[test]
fn a_run_directory_that_names_no_lock_blocks_admission_from_either_source() {
    let bad: [(&str, &str); 4] = [
        ("empty", ""),
        ("relative", "runs/B"),
        ("bare file name", "run.lock"),
        ("malformed encoding", "/repo/%zz/runs/B"),
    ];
    assert_eq!(
        bad.iter()
            .map(|(_, value)| value)
            .collect::<BTreeSet<_>>()
            .len(),
        bad.len(),
        "four distinct values"
    );

    for (tag, value) in bad {
        let harness = Harness::new(&format!("unownable-run-dir-label-{tag}"));
        let owner = Owner::new(RUN_B, INC_2, REPO_KEY_A);
        let name = owner.name(&shell_probe());
        let mut labels = owner.record(&shell_probe()).labels(&harness.root);
        labels.insert(LABEL_RUN_DIR.to_owned(), value.to_owned());
        harness.runtime.seed_container(
            name.as_str(),
            labels,
            IMAGE_ID,
            IMAGE_ID,
            Liveness::Running,
        );
        let error = harness.census(&fresh(INC_1)).expect_err(&format!(
            "[{tag}] labels: unownable evidence blocks admission"
        ));
        let message = refusal(&error);
        assert!(message.contains(LABEL_RUN_DIR), "[{tag}] labels: {message}");
        assert!(
            harness.holds(&name),
            "[{tag}] labels: the census killed the container it could not classify"
        );
        assert!(
            harness.liveness.asked().is_empty(),
            "[{tag}] labels: a lock was probed for a candidate whose owner directory is \
             unreadable: {:?}",
            harness.liveness.asked()
        );

        let harness = Harness::new(&format!("unownable-run-dir-record-{tag}"));
        let mut record = owner.record(&shell_probe());
        record.run_dir = value.to_owned();
        let mut hooks = RecordingHooks::new(ContainerTrace::off());
        write_intent(
            &mut hooks,
            ContainerSite::WriteIntent,
            &harness.root,
            &name,
            &record,
        )
        .expect("write the intent");
        let error = harness.census(&fresh(INC_1)).expect_err(&format!(
            "[{tag}] record: unownable evidence blocks admission"
        ));
        assert!(
            refusal(&error).contains(LABEL_RUN_DIR),
            "[{tag}] record: {}",
            refusal(&error)
        );
        assert!(
            harness.trace.sites().is_empty(),
            "[{tag}] record: the census performed an effect and then refused: {:#?}",
            harness.trace.rendered()
        );
    }

    for good in ["/repo/.upstroke/runs/B", "/repo/a%5Cb/runs/B"] {
        owner_run_dir(good, "test").unwrap_or_else(|error| panic!("`{good}`: {error}"));
    }
}

#[test]
fn a_census_with_no_intents_proceeds_past_every_diagnostic_that_means_unreachable() {
    let diagnostics: [(&str, &str, bool); 4] = [
        (
            "the process cannot use the socket",
            "permission denied while trying to connect to the docker API at \
             unix:///var/run/docker.sock",
            true,
        ),
        (
            "the socket is not there",
            "failed to connect to the docker API at unix:///nonexistent/docker.sock; check if \
             the path is correct and if the daemon is running: dial unix \
             /nonexistent/docker.sock: connect: no such file or directory",
            true,
        ),
        (
            "the daemon is not listening",
            "Cannot connect to the Docker daemon at tcp://127.0.0.1:1. Is the docker daemon \
             running?",
            true,
        ),
        (
            "the daemon answered and would not list",
            "Error response from daemon: conflict: unable to list containers",
            false,
        ),
    ];

    for (what, stderr, unreachable) in diagnostics {
        let harness = Harness::new("no-intents-diagnostic");
        harness
            .runtime
            .set_docker_stderr(RuntimeOp::ListByLabel, stderr);
        let outcome = harness.census(&fresh(INC_1));
        if unreachable {
            let complete = outcome.unwrap_or_else(|error| {
                panic!(
                    "[{what}] a machine with no container evidence refused to run at all: {error}"
                )
            });
            assert_eq!(
                complete.report().runtime_use,
                super::RuntimeUse::NotRequired,
                "[{what}] the report claims the runtime was consulted"
            );
            assert!(complete.report().reclaimed.is_empty(), "[{what}]");
        } else {
            let message =
                refusal(&outcome.expect_err("a runtime that answered and would not list"));
            assert!(
                message.contains("reached and refused"),
                "[{what}] {message}"
            );
        }

        let harness = Harness::new("one-intent-diagnostic");
        let owner = Owner::new(RUN_B, INC_2, REPO_KEY_A);
        seed(
            &harness.root,
            &harness.runtime,
            &owner,
            &shell_probe(),
            Present::IntentOnly,
            Liveness::Gone,
        );
        harness
            .runtime
            .set_docker_stderr(RuntimeOp::ListByLabel, stderr);
        let message = refusal(
            &harness
                .census(&fresh(INC_1))
                .expect_err("an intent exists and the runtime did not list"),
        );
        assert!(
            message.contains(stderr.split(':').next().unwrap_or(stderr)),
            "[{what}] the refusal must quote what the runtime said: {message}"
        );
        assert!(
            harness.trace.sites().is_empty(),
            "[{what}] the census performed an effect before refusing: {:#?}",
            harness.trace.rendered()
        );
    }
}

#[test]
fn a_resume_converges_on_a_container_a_foreign_fresh_census_already_removed() {
    for (tag, first, second) in [
        ("fresh then resume", fresh(INC_2), resume(RUN_A, INC_3)),
        ("resume then fresh", resume(RUN_A, INC_3), fresh(INC_2)),
    ] {
        let harness = Harness::new(&format!("cross-role-converge-{tag}"));
        let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
        let name = seed(
            &harness.root,
            &harness.runtime,
            &dead,
            &shell_probe(),
            Present::Both,
            Liveness::Running,
        );

        let one = harness
            .census(&first)
            .unwrap_or_else(|error| panic!("[{tag}] the first reclaimer refused: {error}"));
        assert_eq!(one.report().reclaimed.len(), 1, "[{tag}]");
        assert!(
            !harness.holds(&name) && !harness.intent_exists(&name),
            "[{tag}]"
        );

        let two = harness.census(&second).unwrap_or_else(|error| {
            panic!(
                "[{tag}] the second reclaimer refused over state the first had already \
                 reclaimed: {error}"
            )
        });
        assert!(two.report().reclaimed.is_empty(), "[{tag}]");
        assert!(two.report().untouched.is_empty(), "[{tag}]");
        assert_eq!(
            two.report().command,
            second.command(),
            "[{tag}] the report names the wrong write command"
        );
        assert_ne!(one.report().command, two.report().command, "[{tag}]");
    }
}

#[test]
fn a_fresh_and_a_resuming_census_race_one_container_and_converge() {
    const ROUNDS: usize = 24;
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);

    for round in 0..ROUNDS {
        let root = scratch(&format!("cross-role-race-{round}"));
        let runtime = Arc::new(FakeRuntime::new(ContainerTrace::off()));
        let names: Vec<ContainerName> = (0..4)
            .map(|ordinal| {
                let invocation =
                    InvocationId::probe(ProbeTarget::Shell, ordinal).expect("a probe identity");
                seed(
                    &root,
                    &runtime,
                    &dead,
                    &invocation,
                    Present::Both,
                    Liveness::Running,
                )
            })
            .collect();
        assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 4);
        let gate = Arc::new(Barrier::new(2));

        let mut handles = Vec::new();
        for start in [
            CensusStart::FreshRun {
                incarnation: INC_2.to_owned(),
            },
            resume(RUN_A, INC_3),
        ] {
            let root = root.clone();
            let runtime = Arc::clone(&runtime);
            let gate = Arc::clone(&gate);
            handles.push(std::thread::spawn(move || {
                let liveness = RecordingLiveness::new();
                let view = DisposableDirView::new(ContainerTrace::off());
                let mut hooks = RecordingHooks::new(ContainerTrace::off());
                gate.wait();
                run_startup_census(
                    &mut hooks,
                    &Census {
                        private_root: &root,
                        start: &start,
                        runtime: runtime.as_ref(),
                        liveness: &liveness,
                        view: &view,
                    },
                )
                .map(|complete| (complete.report().command, complete.report().reclaimed.len()))
            }));
        }
        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("a reclaimer panicked"))
            .collect();
        let mut commands = BTreeSet::new();
        let mut total = 0;
        for outcome in &outcomes {
            let (command, reclaimed) = outcome.as_ref().unwrap_or_else(|error| {
                panic!("[round {round}] a racer refused instead of converging: {error}")
            });
            commands.insert(*command);
            total += reclaimed;
        }
        assert_eq!(
            commands.len(),
            2,
            "[round {round}] both racers reported the same write command, so the roles did not \
             actually differ"
        );
        assert!(
            total >= names.len(),
            "[round {round}] the two racers between them reported {total} of {} orphans removed",
            names.len()
        );
        for name in &names {
            assert!(
                runtime.container(name.as_str()).is_none(),
                "[round {round}]"
            );
            assert!(!name.intent_path(&root).exists(), "[round {round}]");
            assert!(!view_path(&root, name).exists(), "[round {round}]");
        }
        let _ = fs::remove_dir_all(&root);
    }
}

#[test]
fn st16_j_refuses_before_any_effect_and_before_the_token_that_precedes_recovery() {
    let harness = Harness::new("st16-j-before-any-effect");
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let name = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::IntentOnly,
        Liveness::Gone,
    );
    harness.runtime.set_unreachable(RuntimeOp::ListByLabel);

    let outcome = harness.census(&fresh(INC_2));
    let message = refusal(
        outcome
            .as_ref()
            .expect_err("intents exist, runtime unreachable"),
    );
    assert!(message.contains("cannot be reached"), "{message}");

    assert!(
        harness.trace.sites().is_empty(),
        "a funnel site ran before the refusal: {:#?}",
        harness.trace.rendered()
    );
    assert_eq!(
        harness.runtime.calls(),
        vec![RuntimeOp::ListByLabel],
        "the census asked the runtime something other than the reachability question before \
         refusing: {:?}",
        harness.runtime.calls()
    );
    assert!(
        harness.intent_exists(&name),
        "the record was touched before the refusal"
    );
    assert!(
        harness.liveness.asked().is_empty(),
        "an owner's lock was probed on behalf of a write command that refused: {:?}",
        harness.liveness.asked()
    );

    assert!(
        outcome.is_err(),
        "a refusing census must produce no CensusComplete"
    );
}

#[test]
fn an_intent_only_candidate_after_the_reaper_still_has_its_view_pruned() {
    for (label, present, view_seeded) in [
        ("crashed before docker create", Present::IntentOnly, false),
        (
            "the Unix reaper removed the container",
            Present::IntentAndViewAfterReaper,
            true,
        ),
    ] {
        let harness = Harness::new(&format!("post-reaper-{}", present as u8));
        let dead = Owner::new(RUN_A, INC_1, REPO_KEY_A);
        let name = seed(
            &harness.root,
            &harness.runtime,
            &dead,
            &shell_probe(),
            present,
            Liveness::Gone,
        );
        assert_eq!(
            harness.view_exists(&name),
            view_seeded,
            "[{label}] the fixture did not build the state it names"
        );
        assert!(!harness.holds(&name), "[{label}] the container is gone");
        assert!(harness.intent_exists(&name));

        let complete = harness
            .census(&resume(RUN_A, INC_2))
            .expect("the census completes");
        let report = complete.report();
        assert_eq!(report.reclaimed.len(), 1, "[{label}]");
        assert_eq!(
            report.reclaimed[0].discovered_by,
            DiscoveredBy::IntentOnly,
            "[{label}] both situations report the same discovery, which is why the view cannot \
             be handled by discovery"
        );
        assert!(
            !harness.view_exists(&name),
            "[{label}] the R19 view survived the reclaim; R19's `NoRunFinished` cell is `pruned \
             at the next write-command start after the owning container is observed terminated`, \
             and the intent that named it has just been removed"
        );
        assert!(!harness.intent_exists(&name), "[{label}]");
        assert_eq!(
            fs::read_dir(containers_dir(&harness.root))
                .expect("the namespace")
                .count(),
            0,
            "[{label}]"
        );
    }
}

#[test]
fn an_unpruned_view_is_reclaimed_because_its_intent_survived() {
    use crate::topology::effects::{EffectSiteId, HookPhase};

    let harness = Harness::new("anchor");
    let dead = Owner::new(RUN_A, INC_1, REPO_KEY_A);
    let name = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    assert!(harness.view_exists(&name), "the fixture's premise");

    let mut hooks = RecordingHooks::new(harness.trace.clone());
    hooks.fail_at(
        EffectSiteId::Container(ContainerSite::UnmountGitView),
        HookPhase::Before,
    );
    let refused = harness
        .run_with(&mut hooks, &resume(RUN_A, INC_2))
        .expect_err("a census that could not prune a view must not report completion");
    let _ = refusal(&refused);

    assert!(!harness.holds(&name), "the container was removed");
    assert!(harness.view_exists(&name), "the fixture's obstruction held");
    assert!(
        harness.intent_exists(&name),
        "the R26 record was removed while the R19 view it is the only handle on survived; \
         `<R>/views` is never enumerated, so nothing can ever find that directory again"
    );

    let complete = harness
        .census(&resume(RUN_A, INC_3))
        .expect("the census completes");
    assert_eq!(complete.report().reclaimed.len(), 1);
    assert!(!harness.view_exists(&name), "R19 still has residue");
    assert!(!harness.intent_exists(&name));

    let clean = Harness::new("anchor-control");
    let name = seed(
        &clean.root,
        &clean.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    clean
        .census(&resume(RUN_A, INC_2))
        .expect("the census completes");
    assert!(!clean.view_exists(&name) && !clean.intent_exists(&name));
}

#[test]
fn every_reclaimed_container_settles_its_owner_interrupted_with_unknown_spend() {
    use crate::runner::container::census::OwnerSettlement;

    let cells = [
        (Present::Both, DiscoveredBy::IntentAndLabel),
        (Present::IntentOnly, DiscoveredBy::IntentOnly),
        (Present::IntentAndViewAfterReaper, DiscoveredBy::IntentOnly),
        (Present::LabelOnly, DiscoveredBy::LabelOnly),
    ];
    let mut covered = BTreeSet::new();
    for (present, expected) in cells {
        let harness = Harness::new(&format!("settle-{}", present as u8));
        let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
        let name = seed(
            &harness.root,
            &harness.runtime,
            &dead,
            &shell_probe(),
            present,
            Liveness::Running,
        );
        let complete = harness.census(&fresh(INC_2)).expect("the census completes");
        let report = complete.report();
        assert_eq!(report.reclaimed.len(), 1, "{present:?}");
        let entry = &report.reclaimed[0];
        assert_eq!(entry.name, name);
        assert_eq!(entry.discovered_by, expected, "{present:?}");
        assert_eq!(
            entry.settlement,
            OwnerSettlement::InterruptedWithUnknownSpend,
            "{present:?}: a reclaimed container's owning identity settles interrupted, and its \
             spend is unknown — a container with no record and no runtime object is the state \
             the Unix reaper leaves behind, not evidence that nothing ran"
        );
        assert!(
            !entry.settlement.spend_is_known(),
            "{present:?}: `authoritative_state` opens `unknown spend`"
        );
        covered.insert(entry.discovered_by);
    }
    assert_eq!(
        covered,
        DiscoveredBy::ALL.iter().copied().collect::<BTreeSet<_>>(),
        "every cell of the discovery grid must produce a settlement"
    );
    assert_eq!(
        OwnerSettlement::ALL.len(),
        1,
        "a second settlement would need a rule saying which candidates get it"
    );
    assert_eq!(
        OwnerSettlement::InterruptedWithUnknownSpend.name(),
        "interrupted-unknown-spend"
    );
}

#[test]
fn a_staged_intent_with_no_published_half_is_accounted_for() {
    use crate::runner::container::census::StagedDisposition;

    let harness = Harness::new("staged");
    let probe = shell_probe();

    let adopted_owner = Owner::new(RUN_A, INC_1, REPO_KEY_A);
    let adopted = adopted_owner.name(&probe);
    let staged_path = |name: &ContainerName| {
        containers_dir(&harness.root).join(format!("{}.intent.tmp", name.as_str()))
    };
    fs::create_dir_all(containers_dir(&harness.root)).expect("the namespace");
    fs::write(
        staged_path(&adopted),
        serde_json::to_vec(&adopted_owner.record(&probe)).expect("serialize"),
    )
    .expect("a staged record");
    fs::create_dir_all(view_path(&harness.root, &adopted)).expect("a view");

    let mine = Owner::new(RUN_A, INC_2, REPO_KEY_A).name(&agent_probe());
    fs::write(staged_path(&mine), b"{\"run_id\":\"01KZ").expect("a torn record");

    let foreign = Owner::new(RUN_C, INC_1, REPO_KEY_B).name(&probe);
    fs::write(staged_path(&foreign), b"").expect("a torn record");

    assert!(
        crate::runner::container::list_intents(&harness.root)
            .expect("scan")
            .is_empty(),
        "a staged file was adopted by discovery, which is a different defect"
    );

    let complete = harness
        .census(&resume(RUN_A, INC_3))
        .expect("the census completes");
    let report = complete.report();
    let by_name: BTreeMap<&str, StagedDisposition> = report
        .staged
        .iter()
        .map(|entry| (entry.name.as_str(), entry.disposition))
        .collect();
    assert_eq!(by_name.len(), 3, "{:?}", report.staged);
    assert_eq!(
        by_name[adopted.as_str()],
        StagedDisposition::Adopted,
        "a complete staged record carries the owner's run directory, so it classifies under the \
         ordinary rule"
    );
    assert_eq!(by_name[mine.as_str()], StagedDisposition::Removed);
    assert_eq!(
        by_name[foreign.as_str()],
        StagedDisposition::RetainedForeignOwner,
        "arm (ii) probes the owner's `run.lock` and a torn record names no run directory, so \
         this census cannot establish that its owner is dead"
    );

    assert!(!staged_path(&adopted).exists());
    assert!(!harness.view_exists(&adopted));
    assert_eq!(
        report
            .reclaimed
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec![adopted.as_str()],
        "the adopted staged record is a candidate; the torn ones are not"
    );

    assert!(!staged_path(&mine).exists(), "this run's own torn residue");
    assert!(
        staged_path(&foreign).exists(),
        "another run's torn residue was removed on evidence this census does not have"
    );
    assert_eq!(
        StagedDisposition::ALL.len(),
        3,
        "every disposition is exercised above; a fourth would be unexercised"
    );
    assert_eq!(
        fs::read_dir(containers_dir(&harness.root))
            .expect("the namespace")
            .count(),
        1
    );
}

#[test]
#[cfg(unix)]
fn a_view_removal_that_never_succeeds_blocks_admission() {
    use std::os::unix::fs::PermissionsExt as _;

    let harness = Harness::new("removal-exhausted");
    let dead = Owner::new(RUN_A, INC_1, REPO_KEY_A);
    let name = seed(
        &harness.root,
        &harness.runtime,
        &dead,
        &shell_probe(),
        Present::Both,
        Liveness::Running,
    );
    let view = view_path(&harness.root, &name);
    fs::write(view.join("HEAD"), b"0000\n").expect("a file in the view");
    let views = view.parent().expect("the views directory").to_path_buf();
    fs::set_permissions(&views, fs::Permissions::from_mode(0o500)).expect("clear the write bit");
    if fs::remove_dir_all(&view).is_ok() {
        let _ = fs::set_permissions(&views, fs::Permissions::from_mode(0o755));
        return;
    }

    let error = harness
        .census(&resume(RUN_A, INC_2))
        .expect_err("a census that could not prune an orphan view must not hand out its token");
    assert!(
        matches!(error, UpstrokeError::Io { .. }),
        "the refusal must carry the IO error that stopped it, after the retry bound: {error:?}"
    );
    assert!(
        view.exists(),
        "the fixture's premise: the view is still there"
    );
    assert!(
        harness.intent_exists(&name),
        "the only handle on the unreclaimed view was deleted"
    );

    let _ = fs::set_permissions(&views, fs::Permissions::from_mode(0o755));
    let complete = harness
        .census(&resume(RUN_A, INC_3))
        .expect("the census completes once the view can be removed");
    assert_eq!(complete.report().reclaimed.len(), 1);
    assert!(!view.exists() && !harness.intent_exists(&name));
    let _ = fs::remove_dir_all(&harness.root);
}
