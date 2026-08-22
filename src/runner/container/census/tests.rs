//! The census suite.
//!
//! **Every test here names the second field it holds constant.** The dominant
//! defect shape on this project is two axes covered separately with their
//! intersection never built, and this module is unusually exposed to it: the
//! liveness rule is `{owner run} × {incarnation}`, discovery is `{intent
//! present} × {container present}`, and the write-command axis is `{run} ×
//! {resume}`. A suite that varies one at a time passes while an implementation
//! that reclaims a **live** run's dead earlier incarnation ships.

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

// `effects::production_region` cuts a source at its FIRST `#[cfg(test)]`, and
// several source censuses in this tree scan every `src/**/*.rs` — including
// this one, which is reached only through `#[cfg(test)] mod tests;` and so has
// no attribute of its own for them to cut on. The marker below is redundant to
// the compiler and load-bearing to those censuses: it makes this file's
// production region empty, so a fixture that names a primitive is not reported
// as a production offender (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`, used here in
// the direction it is wanted).
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
use crate::error::TactusError;
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

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A scratch private root. Thread id is in the name because
/// [`concurrent_reclaimers_converge`] runs two of these at once.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tactus-census-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch private root");
    dir
}

/// Distinct values for every independently meaningful field, so a swap between
/// any two is visible rather than accidentally equal.
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

/// One owner, fully specified. Every field varies independently in the grids
/// below, which is why they are arguments and not defaults.
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
            run_dir: PathBuf::from(format!("/repo/.tactus/runs/{run_id}")),
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

    /// The owner's run directory, whatever bytes it carries.
    ///
    /// A setter and not a second constructor so a hostile directory is a
    /// one-line variation on an otherwise identical owner — `PR6-RECOV-001`'s
    /// grids vary the run directory and hold everything else fixed.
    fn with_run_dir(mut self, run_dir: PathBuf) -> Self {
        self.run_dir = run_dir;
        self
    }

    /// Through `ContainerIntent::new`, so a fixture's record carries the same
    /// encoding a real invocation writes.
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

/// What a fixture puts on the machine for one container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Present {
    /// A record and a container: the ordinary running state.
    Both,
    /// A record, no container and **no view**: a crash between the intent write
    /// and `docker create`. Nothing was mounted, so there is nothing to prune.
    IntentOnly,
    /// A record, no container and a **view**: the ordinary state after the Unix
    /// reaper has run. It performs `kill/rm` and nothing else
    /// (`T-CONTAINER.resume_action`), so the invocation's R19 directory and its
    /// R26 record both outlive the container.
    ///
    /// `PR6-CONV-003`. `IntentOnly` was documented as covering **both**
    /// situations and seeded only for the first, so `{intent present} ×
    /// {container present}` and `{view present}` were correlated in every
    /// fixture: a regression that skipped view cleanup for an intent-only
    /// candidate removed the final record, returned `CensusComplete`, and
    /// stranded a now-undiscoverable R19 directory — with the whole suite
    /// green.
    IntentAndViewAfterReaper,
    /// A container and no record: "a labeled container without an intent".
    LabelOnly,
}

/// Put one container's evidence on the machine and return its name.
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
    // R19 exists whenever the invocation got as far as mounting it, which is
    // every state except "crashed before `docker create`". The view is
    // deliberately **not** tied to the container's presence: the post-reaper
    // state has one and no container.
    if present != Present::IntentOnly {
        fs::create_dir_all(view_path(root, &name)).expect("an orphan view directory");
    }
    name
}

/// An owner-liveness probe that records what it was asked.
///
/// Not [`crate::runner::container::FakeOwnerLiveness`], which answers but keeps
/// no log: "arm (i) does not probe the lock at all" and "arm (ii) does not read
/// the incarnation" are both claims about **what was asked**, and only a log can
/// hold them.
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

/// A runtime whose `stop` **succeeds and does not stop anything**.
///
/// The container is still running after every observation, which is the state
/// `refusal_condition`'s "cannot be observed terminated" is about: a wedged
/// supervisor, a container in `removing` that never leaves it, a daemon that
/// accepts a signal and delivers nothing. [`FakeRuntime`] cannot reach it —
/// its `stop` always moves the container to `Exited` — so a suite built only on
/// the fake would find that branch unconstructible and green.
///
/// It delegates only the **read-only** operations. The four effectful ones are
/// the funnel's primitives, and this wrapper implements rather than forwards
/// them, so it never becomes a second caller of one.
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
        // Accepted, delivered nowhere. This is the whole fixture.
        Ok(())
    }
    fn remove(&self, _name: &str) -> Result<(), crate::runner::container::runtime::RuntimeError> {
        unreachable!("reclaim refuses before `rm` when termination cannot be observed")
    }
}

/// A resume's recovery step (a1), established from bytes this fixture owns.
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

/// Everything a census run needs, held together so a test varies one field.
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

    fn census(&self, start: &CensusStart) -> Result<CensusComplete, TactusError> {
        let mut hooks = RecordingHooks::new(self.trace.clone());
        self.run_with(&mut hooks, start)
    }

    fn run_with(
        &self,
        hooks: &mut dyn ContainerHooks,
        start: &CensusStart,
    ) -> Result<CensusComplete, TactusError> {
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

/// Where `needle` first appears in the trace, or a failure naming the sequence.
fn at(trace: &ContainerTrace, needle: &str) -> usize {
    trace.position(needle).unwrap_or_else(|| {
        panic!(
            "`{needle}` is not in the trace, which is {:#?}",
            trace.rendered()
        )
    })
}

fn refusal(error: &TactusError) -> String {
    match error {
        TactusError::Refused { message } => message.clone(),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. The liveness rule — two arms, and the intersection nobody builds
// ---------------------------------------------------------------------------

/// Every cell of `{owner run} × {incarnation} × {owner lock}`.
///
/// The rule has two arms and each arm has two outcomes, so the grid is the
/// product and not a list of the cases that came to mind. **Arm (i) has no lock
/// axis** (this process holds the lock, so a probe would be asking whether it is
/// itself alive) and **arm (ii) has no incarnation axis** — "reclaim every
/// container of that run whatever its incarnation", which includes this
/// process's own. So the tuples collapse to **four** classifications, and that
/// collapse is asserted as a distinct-value count rather than described.
///
/// **This test was rewritten by `PR6-RECOV-003`, and the previous oracle was
/// wrong.** It required `ForeignRunThisIncarnation` for the two cells where a
/// foreign run's recorded incarnation equals this process's — a refusal that
/// never reached arm (ii) and so never probed the owner's lock. The
/// classification rule splits on the owner run **first** and puts the
/// own-incarnation refusal inside arm (i)'s clause; arm (ii) then says
/// "whatever its incarnation" in as many words, and `T-CONTAINER.resume_action`
/// states the same order. The cost of the hoisted check was that a **dead**
/// foreign owner's container could never be reclaimed and blocked every write
/// command under that private root permanently. See `census::Ownership`.
///
/// Second field held constant: the container name, the repo key and the run
/// directory are the same shape in every cell, so nothing but the ownership
/// triple moves.
#[test]
fn the_liveness_rule_classifies_every_cell_of_owner_run_by_incarnation_by_lock() {
    let liveness = RecordingLiveness::new();
    let live_dir = PathBuf::from("/repo/.tactus/runs/live");
    let dead_dir = PathBuf::from("/repo/.tactus/runs/dead");
    liveness.set_live(&live_dir);

    let mine = resume(RUN_A, INC_1);
    // `(what, owner run, owner incarnation, owner run dir, expected, is the
    // owner's lock probed)`. The last field is a separate column because "arm
    // (i) does not probe" and "arm (ii) always probes" are independently
    // droppable predicates, and a fixture that only checked the classification
    // would pass an implementation that probed a lock it holds itself.
    let cells: Vec<(&str, &str, &str, &Path, Ownership, bool)> = vec![
        // Arm (i): the run this process drives. The lock is not probed, and the
        // lock state is varied anyway to prove it is not consulted.
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
        // Arm (ii) with the incarnation equal to this process's. **The two
        // cells `PR6-RECOV-003` is about**: the lock decides, exactly as it
        // does for any other incarnation, and it is asked.
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
        // Arm (ii): another run, another incarnation.
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

    // Exactly one of the four refuses, and it does not reclaim. A refusal that
    // also reclaimed would have performed an effect on behalf of a write
    // command that never ran.
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

    // {start kind} × {incarnation}. A **fresh** run has no own run at all, so
    // arm (i) is unreachable for it and every cell above is an arm (ii) cell —
    // including the ones naming the incarnation this process generated at
    // startup. Nothing is refused, every candidate's owner lock is probed, and
    // the answer is the lock's.
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

/// The probe is asked **once** per candidate, and never in a loop.
///
/// `T-CONTAINER.resume_action`: "probe the owner's run.lock **non-blocking**;
/// held -> skip". Catalogue entry `PR6-INTENT-031` survived the whole suite by
/// replacing the single non-blocking probe with a blocking retry loop, because
/// nothing looked at *how many times* the seam was asked — and a census that
/// waits on a live neighbour is a stall at every write-command start, which is
/// the one thing "non-blocking" is there to prevent.
///
/// The call **count** is the observable, not the elapsed time: a wall-clock
/// bound would be a flake on a loaded box, and a retry loop that gave up after
/// `n` attempts would pass one anyway.
///
/// Second field held constant: one candidate, one owner directory; what varies
/// is only whether that owner's lock is held.
#[test]
fn the_owner_lock_is_probed_exactly_once_per_candidate() {
    let held = PathBuf::from("/repo/.tactus/runs/held");
    let free = PathBuf::from("/repo/.tactus/runs/free");
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

/// **The crossed fixture.** A *live* run's *dead earlier incarnation*, seen by a
/// *foreign* census, is never touched.
///
/// `crash_reconstruction`: "held -> live owner -> **never touched** (that owner
/// reclaims its own earlier incarnations at its own startup census, which
/// precedes its admission)"; and the residual it names — "a container of a dead
/// incarnation of a live run may run until that run's own census reclaims it …
/// **out of scope**".
///
/// This is the cell an implementation that reclaims dead incarnations gets
/// wrong, and it passes every test that varies only `{owner run}` or only
/// `{incarnation}`. The same fixture is then run again with the owner's lock
/// **free** and the same two incarnations are both reclaimed, so the test cannot
/// pass by never reclaiming anything.
///
/// Second field held constant: the two containers, their names, their records
/// and their private root are byte-identical between the two halves; the **only**
/// thing that moves is whether the owner's lock is held.
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
            // "reclaim EVERY container of that run WHATEVER its incarnation".
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

/// Arm (ii) does not read the incarnation, over a domain of them —
/// **including this process's own**.
///
/// The lane's first version of this test asserted exactly this, an independent
/// review refuted it on `expected_failures_refusals[7]`, and `PR6-RECOV-003`
/// restored it: that line is the contract's one-sentence summary of arm (i)'s
/// clause, while the classification rule splits on the owner run first and arm
/// (ii) says "reclaim every container of that run **whatever its
/// incarnation**". `T-CONTAINER.resume_action` states it in the same order.
///
/// Second field held constant: the owner run and its lock state; only the
/// incarnation moves, across four distinct values, one of them this process's
/// own.
#[test]
fn arm_two_gives_one_answer_whatever_the_incarnation_that_reaches_it() {
    let liveness = RecordingLiveness::new();
    let held = PathBuf::from("/repo/.tactus/runs/held");
    let free = PathBuf::from("/repo/.tactus/runs/free");
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

    // And every one of the eight cells asked the owner's lock exactly once:
    // arm (ii) reaches the probe for this process's own incarnation too, which
    // is what the hoisted comparison prevented.
    assert_eq!(
        liveness.asked().len(),
        8,
        "a foreign candidate was classified without its owner's lock being probed: {:?}",
        liveness.asked()
    );
}

/// The incarnation is never read from the lock: the seam has no incarnation in
/// it, and this module never names a lock file.
///
/// `crash_reconstruction`: "the coordinator incarnation id is a per-process ULID
/// recorded in `run_started(4)`/`run_resumed(4)` and is **never read from
/// lock-file contents** (`run.lock` content is never read: `src/rundir.rs:886`;
/// a Windows exclusive lock makes it unreadable to non-holders)". Deriving it
/// from the lock is a plausible implementation and a real defect.
///
/// Second field held constant: the runtime and the namespace; only what the
/// liveness seam is handed and returns is under test.
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

    // What the seam was handed is the PUBLIC run directory and nothing else,
    // and what it gave back is one bit — there is no incarnation in the return
    // type to read.
    assert_eq!(harness.liveness.asked(), vec![dead.run_dir.clone()]);
    let one_bit: bool = harness.liveness.is_running(&dead.run_dir);
    assert!(!one_bit);

    // And the module does not reach around the seam: its production region
    // names no lock file at all.
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

// ---------------------------------------------------------------------------
// 2. The T-CONTAINER names
// ---------------------------------------------------------------------------

/// (4) An orphan is reclaimed **before slot reset, credential reuse, or
/// admission** — expressed as the token those consumers cannot be reached
/// without.
///
/// ST-16 (a): "single owner dies -> next write-command start reclaims
/// (inspect/kill/observe/rm/view/intent) **before slot reset, credential reuse,
/// or admission**". Slots and admission are PR11's and the credential-volume
/// turn is PR7's, so what this slice can hold is that (i) the whole five-step
/// reclaim is complete when the census returns, in the packet's order, and (ii)
/// a census that could not complete it returns **no token**, so nothing that
/// takes one can run. `census_returns_the_only_token_that_reaches_a_consumer`
/// is the structural half.
///
/// Second field held constant: the owner is dead in both halves and the fixture
/// is byte-identical; only whether the container can be observed terminated
/// moves.
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

    // The five steps, in the packet's order, all before the token existed.
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

    // The other half: a container that cannot be observed terminated blocks
    // admission, so there is no token at all.
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

/// (5) A live owner's containers are untouched while a dead owner's orphan in
/// the **same private root** is reclaimed.
///
/// ST-16 (b): "live coordinator A running while dead coordinator B's orphan
/// exists in the same private root (**same or different repository**) -> reclaim
/// kills only B's container, A's continues, and **no invocation uses the shared
/// credential volume before B's is observed terminated**".
///
/// The repositories differ — two repo keys under one private root, which is the
/// "different repository" half of that clause — and the run directories differ,
/// which is what the lock probe distinguishes them by. The credential-volume
/// clause is the token: B's observation is complete before `run_startup_census`
/// returns, and nothing that takes a `&CensusComplete` exists until then.
///
/// Second field held constant: both containers are `Running`, both have records,
/// both are under the same private root; only the owner run and its lock state
/// move.
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

    // Only B was touched: no runtime operation names A's container at all.
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

    // The credential-volume clause: B's termination is observed before the
    // token exists, and no volume operation happens in a census at all.
    assert!(
        at(&harness.trace, &format!("rt:observe:{b}"))
            < at(&harness.trace, &format!("rt:remove:{b}"))
    );
    assert!(
        !harness.trace.ops().contains(&RuntimeOp::InspectVolume),
        "a census inspects no volume; the turn is taken by a consumer of the token"
    );
}

/// (6) A labeled container with no intent is reclaimed under the same rule.
///
/// `crash_reconstruction`: "a labeled container **without an intent** is treated
/// as an orphan of its **labeled** run and incarnation under the same rule".
/// Its ownership therefore comes from `tactus.run` and `tactus.incarnation`, and
/// the census must reach the same verdict it would have reached from a record.
///
/// Second field held constant: the same owner, the same name and the same
/// liveness answer are used for a record-backed container in the same fixture,
/// so the two differ **only** in which half of discovery found them.
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

    // Both reached the same ownership verdict, which is what "under the same
    // rule" means.
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

/// (7) A resume reclaims its own earlier incarnation's orphan — including a
/// probe invocation with the **same deterministic `InvocationId`**.
///
/// ST-16 (f): "the resuming incarnation holds the run lock … and still reclaims
/// its own earlier incarnation's orphan (incl. a probe invocation with the same
/// deterministic `InvocationId`, whose new container name and intent path
/// differ) before slot init, admission, credential use, or its own probes, while
/// containers it starts afterwards are untouched".
///
/// Second field held constant: the invocation identity is **literally the same
/// value** for the dead incarnation and for this one, so the only thing that can
/// separate their names and intent paths is the incarnation component.
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

    // The same deterministic identity, this incarnation.
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

    // "while containers it starts afterwards are untouched": this incarnation's
    // own container appears only after the census, and a second census of the
    // same root would refuse it rather than reclaim it — which is the next test.
    assert!(
        !harness.holds(&would_be),
        "this incarnation has started nothing yet; the census precedes every invocation"
    );
}

/// (8) The census scans **exactly the root it is given**, after the default
/// moved.
///
/// ST-16 (f): "censuses the recorded private root **even when the default root
/// or `HOME` changed**". PR7 owns deriving that root from
/// `run_started.private_dir` (recovery step (a0)); what this slice owns is that
/// the census takes it as a parameter and reads no default — so a second root
/// holding a reclaimable orphan is left completely alone, and "different private
/// roots are disjoint worlds".
///
/// Second field held constant: the two roots hold **the same owner, the same
/// invocation and therefore the same container name**; the only thing that
/// differs is which root the census was handed.
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
    // The same container name and record, under the other root. If the census
    // read a default it would find this one instead, or as well.
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

    // And the label filter is the root it was given, not any other.
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

/// (10) Three incarnations, two crashes, every dead incarnation reclaimed with
/// no name or intent collision.
///
/// ST-16 (g): "repeated crashes across **three** incarnations leave orphans from
/// **two** dead incarnations that are all reclaimed with no name or intent
/// collision".
///
/// ## The cardinality is the clause (`PR6-ENUM-008`)
///
/// This seeded **three** dead incarnations and resumed as a **fourth**, which
/// is a different sentence: the variant says three incarnations *total*, of
/// which two are dead and the third is the one doing the censusing. The
/// enumerated cell — exactly two `OwnRunEarlierIncarnation` candidates — was
/// therefore never built, and an implementation that mishandled precisely two
/// while handling one and three passed.
///
/// Both cardinalities are driven now, as a grid, with the reclaimed count
/// asserted per cell: `{1, 2, 3} dead incarnations` × `the resuming
/// incarnation is the next one`. Two is ST-16 (g)'s cell; one and three are
/// what make it a measurement of the count rather than of a threshold.
///
/// Second field held constant: every orphan of a cell is the **same
/// deterministic probe identity** under the **same run** and the **same repo
/// key**, so the only thing separating the names and the intent paths is the
/// incarnation component — which is exactly the thing the packet says it is
/// for. The last dead incarnation of each cell carries a *different*
/// invocation, so no cell is n copies of one shape.
#[test]
fn repeated_crashes_reclaim_every_dead_incarnation() {
    /// Four incarnations, so the resuming one is always a fresh value and
    /// never one of the dead.
    const INCARNATIONS: &[&str] = &[INC_1, INC_2, INC_3, "01KZTDDDDDDDDDDDDDDDDDDDDD"];
    const RESUMING: &str = "01KZTEEEEEEEEEEEEEEEEEEEEE";

    for dead_count in 1..=3_usize {
        let harness = Harness::new(&format!("incarnations-{dead_count}"));
        let probe = shell_probe();
        let mut names = Vec::new();
        for (ordinal, incarnation) in INCARNATIONS.iter().take(dead_count).enumerate() {
            let owner = Owner::new(RUN_A, incarnation, REPO_KEY_A);
            // The last one carries a different invocation identity.
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

        // The next incarnation of the same run resumes and censuses. ST-16 (g)
        // is the `dead_count == 2` cell: three incarnations in total, orphans
        // from the two dead ones.
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

/// (11) Two reclaimers **actually racing** on one container converge.
///
/// "every step idempotent and tolerant of already-gone so **two concurrent
/// reclaimers converge**". A fixture that ran two censuses one after the other
/// would prove idempotence, which is a different claim: idempotence is about
/// repeating a completed operation, convergence is about two interleaved ones.
/// So the two run on two threads, released together by a
/// [`Barrier`], over many rounds so the interleaving actually varies.
///
/// ## The pair is ST-16 (h)'s pair, and the result is asserted converged
///
/// `PR6-CONV-002`. Both reclaimers used to be `CensusStart::FreshRun`, and the
/// closing assertion was `total >= 4` — which **a fully serialised run
/// satisfies**: one census reports 4, the other reports 0, and nothing about
/// the second one was ever a reclaim. ST-16 (h) names the pair exactly — "two
/// concurrent reclaimers (**a foreign write command and the resuming
/// incarnation**) converge idempotently on the same dead container" — so one
/// side is now a resume of the orphans' own run and the other a fresh foreign
/// write command under the same private root. They classify the same
/// containers through **different arms** of the liveness rule: the resuming
/// incarnation through arm (i) (own run, earlier incarnation, dead by
/// construction) and the foreign command through arm (ii) (another run, lock
/// free). That is the shape the packet describes and it is a strictly harder
/// fixture than two copies of one arm.
///
/// The serialised outcome is refused rather than accepted: **both** reclaimers
/// must report at least one reclaim in at least one round, and the run counts
/// how many rounds actually interleaved. A machine that serialised every round
/// fails here instead of passing.
///
/// Second field held constant: both reclaimers are handed the **same** runtime,
/// the same root and the same four containers; what differs is which write
/// command each one is and which thread gets there first.
#[test]
fn concurrent_reclaimers_converge() {
    const ROUNDS: usize = 24;
    let dead = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let probe = shell_probe();
    // How many rounds saw both sides do work. Convergence is about an
    // interleaving, so a run in which none did is a run that measured
    // idempotence and called it convergence.
    let mut interleaved = 0_usize;

    for round in 0..ROUNDS {
        let root = scratch(&format!("converge-{round}"));
        let trace = ContainerTrace::off();
        let runtime = Arc::new(FakeRuntime::new(trace.clone()));
        // FOUR containers, not one. The dangerous window is between the
        // namespace directory read and the per-record reads inside it, and one
        // record closes that window almost immediately: with a single orphan,
        // this fixture detected the `list_intents` intolerance measured below
        // in only 2 of 20 runs. Four records widen the scan enough for the
        // detection to be reliable, which is the difference between a test that
        // holds a claim and one that occasionally notices it.
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

        // ST-16 (h)'s two write commands. `RUN_B` owns the orphans, so the
        // resume reaches them through arm (i) and the foreign fresh run
        // reaches them through arm (ii) with `RUN_B`'s lock free.
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
            // Whichever arm did the work, it was the arm that side is for.
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
        // The converged result, asserted: nothing of any of the four remains,
        // whichever order they interleaved in.
        for name in &names {
            assert!(runtime.container(name.as_str()).is_none());
            assert!(!name.intent_path(&root).exists());
            assert!(!view_path(&root, name).exists());
        }
        // Somebody did the work. The loser may legitimately find a container
        // already gone and report fewer; between them they must account for all
        // four. What must never happen is a refusal, asserted above.
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
    // Two threads released at a barrier and then serialised every single time
    // is a fixture that proved idempotence and reported convergence. This is
    // the assertion that the interleaving actually happened.
    assert!(
        interleaved > 0,
        "in none of {ROUNDS} rounds did both reclaimers remove anything, so this fixture never \
         interleaved and `T-CONTAINER.resume_action`'s \"concurrent reclaimers converge\" was \
         not measured"
    );
}

/// The sharpest interleaving, made deterministic.
///
/// A racing fixture visits the dangerous window by luck. This one puts a
/// reclaimer to sleep at `Container.Remove`'s `Before` phase, lets a second
/// reclaimer run the whole sequence to completion underneath it, and then
/// releases the first — so the first issues `docker rm`, view removal and
/// intent removal against a machine where all three are already gone. Every one
/// must be tolerant of already-gone or the census refuses and blocks admission
/// forever.
///
/// Second field held constant: both reclaimers see the same root, the same
/// runtime and the same container; only the suspension point moves, and it is
/// the same point every run.
#[test]
fn a_reclaimer_suspended_mid_sequence_converges_with_one_that_finished() {
    /// Hooks that block once, at one phase of one site.
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

    // The second reclaimer finishes the whole sequence underneath it.
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

/// (12) A foreign census leaves a schema-4 run's probe containers alone while
/// its `run.lock` is held during preflight.
///
/// ST-16 (i): "a schema-4 run's probe containers (shell and agent probes) carry
/// an owner whose `run.lock` is **held** during preflight (T-RUNSTART P4) and
/// whose owner record already names the `RunnerPolicy`, and a concurrent foreign
/// census leaves them untouched".
///
/// **PR7 completes this.** The owner record at P3b and the P0-P8 sequence that
/// makes the lock held at P4 are `decisions.pr_sequence[8]`'s. What PR6 holds is
/// the half the census owns: a foreign census leaves untouched **every**
/// container of a run whose lock is held, including probe containers, and
/// including a probe container whose owner has not yet appended `run_started`.
///
/// Second field held constant: an identical dead-owner probe container is in the
/// same fixture, so the test cannot pass by leaving everything alone.
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

    // Both probe kinds were present, so the claim is about probe containers and
    // not about one of them.
    assert_eq!(
        held.iter().collect::<BTreeSet<_>>().len(),
        2,
        "a shell probe and an agent probe, two distinct names"
    );
}

/// (14) Intents present + the runtime unreachable = the write command refuses,
/// before any effect.
///
/// ST-16 (j) and `expected_failures_refusals[8]`: "intents present without a
/// reachable runtime refuse the write command". It "cannot prove those
/// containers terminated".
///
/// The reachability question is asked of the operation the census actually needs
/// — `ListByLabel` — and **not** of `probe`, whose `Ok` binds nothing: the
/// fixture arms `ListByLabel` unreachable while leaving `Probe` reachable, so an
/// implementation that gated on `probe` proceeds and fails this test.
///
/// Second field held constant: the same single intent is on disk in both halves;
/// only the runtime's answer moves.
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

    // And the same runtime, reachable again, reclaims it — so the refusal is
    // about reachability and not about the fixture being unreclaimable.
    harness.runtime.set_reachable(RuntimeOp::ListByLabel);
    let complete = harness.census(&fresh(INC_2)).expect("now it proceeds");
    assert_eq!(complete.report().reclaimed.len(), 1);
    assert!(!harness.intent_exists(&name));
}

/// (15) No intent + no reachable runtime = the census **proceeds**.
///
/// This is the half a plausible suite forgets, and getting it wrong makes the
/// engine unusable on every machine without a container runtime — which today
/// is every machine, because `production_effect` is "none". The whole daemon is
/// armed unreachable, not one operation.
///
/// Second field held constant: the private root and the write command are the
/// same as in the refusing half above; only the presence of an intent moves.
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

/// A runtime that is **reached** and refuses to list is not the same answer.
///
/// `RuntimeError` distinguishes `Unreachable` from `Failed` for exactly this:
/// "with no intent and no **reachable** runtime it proceeds" licenses proceeding
/// when the runtime is not there, and says nothing about one that is there and
/// will not answer. A daemon that answers and fails a `ps` cannot prove there is
/// no labeled orphan, so the census refuses rather than admitting over one.
///
/// Recorded as a judgement, not as a packet clause: it is the conservative
/// reading of a case the sentence does not enumerate, and the refusal names it.
///
/// Second field held constant: the namespace is empty in both halves — the one
/// state that *would* license proceeding — so the only thing under test is which
/// kind of runtime error it is.
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

/// (16) The census report names each reclaimed container's boundary from its
/// `runner_policy_sha256`.
///
/// ST-16 (k): "a probe container killed with its coordinator **before
/// `run_started`** is reclaimed by the next census, whose report names its
/// boundary from the intent's `runner_policy_sha256` **and the owner record**".
///
/// **PR7 completes this**: the owner-record half is `decisions.pr_sequence[8]`'s
/// "atomic owner record with the RunnerPolicy". PR6 holds the intent half, and
/// [`Boundary::NoIntentRecord`] is the honest name for the case where this side
/// has nothing.
///
/// Second field held constant: the two reclaimed containers are the same probe
/// kind under the same private root and are both dead-owner orphans; the only
/// thing that differs is which `RunnerPolicy` their record names, so a report
/// that carried one digest for both fails.
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

    // The values are the records' own — read back off disk rather than taken
    // from the fixture's variables, so the report cannot be its own oracle.
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

    // The probe was killed before `run_started`: nothing in this fixture wrote
    // an event log at all, and the boundary still has a name.
    assert!(!harness.root.join("events.jsonl").exists());
}

// ---------------------------------------------------------------------------
// 3. Refusals, each with the ordering predicate it carries
// ---------------------------------------------------------------------------

/// An intent naming this process's own incarnation refuses — **before any
/// effect**, including before a reclaim it would otherwise have performed.
///
/// `expected_failures_refusals[7]`, and "the one most likely to be written as a
/// `continue`". The fixture puts a perfectly reclaimable orphan beside it, so an
/// implementation that skipped the offending record and got on with its work
/// fails here rather than passing quietly.
///
/// **The refusal is arm (i)'s**, and this fixture was rewritten by
/// `PR6-RECOV-003`. The owner run stays an axis — `{own run, foreign run} ×
/// {this incarnation, an earlier one} × {owner lock held, free}` — because the
/// point of the grid is that the two arms give *different* answers to the same
/// incarnation, and the two foreign cells naming this process's incarnation are
/// now classified by the owner's lock like every other arm (ii) candidate:
/// **held -> never touched**, **free -> reclaimed**. The previous oracle
/// required a refusal in those two cells; that refusal never probed the lock,
/// so a dead foreign owner's container was unreclaimable and blocked every
/// write command under the root for good.
///
/// Second field held constant: the reclaimable orphan beside the suspect is
/// identical in every cell — same owner, same repo key, same state — so the only
/// thing that moves is the suspect's own ownership triple.
#[test]
fn an_intent_naming_this_processs_own_incarnation_is_refused_before_any_effect() {
    // `(tag, the suspect's owner run, its incarnation, is its lock held, does
    // it refuse)`. RUN_A is the run this process drives; RUN_C is not.
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

/// A **held** foreign owner's container is never touched, and its incarnation
/// does not change that — including when it is this process's own.
///
/// The mutation an independent review measured — an early branch classifying
/// any foreign candidate carrying the process incarnation as
/// `ForeignRunDeadOwner` — would **kill a held owner's container**, which arm
/// (ii) forbids in as many words ("held -> live owner -> never touched"). It is
/// still forbidden, and it is still what this fixture asserts; what
/// `PR6-RECOV-003` changed is that the protection comes from the **probe**
/// rather than from a refusal in front of it, which is the only version of the
/// protection that also lets a *dead* owner's container be reclaimed.
///
/// So the claim is made as a runtime state and not as a classification: the
/// container is still there, its record is still there, its view is still
/// there, and the funnel issued nothing at all.
///
/// Second field held constant: one owner, one container, one lock state (held);
/// only the incarnation the intent names moves, between this process's own and
/// an earlier one.
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
        // The state of the machine is identical in both halves, and it is the
        // untouched state.
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

/// A **dead** foreign owner's container carrying this process's incarnation is
/// reclaimed, which is the half the hoisted comparison made unreachable.
///
/// `PR6-RECOV-003`'s other cell, and the one that is not merely a different
/// classification of the same outcome: under the shipped rule this container
/// could never be reclaimed by anybody. Its owner is dead, so no census of
/// *that* run will ever run again; every write command under this private root
/// met the refusal and stopped. The grid is `{owner lock held, free}` with the
/// incarnation held at this process's own, and the two halves must differ in
/// what happens to the machine — a fixture that asserted only the free half
/// would pass an implementation that killed both.
///
/// Second field held constant: the same owner run, the same incarnation, the
/// same container and the same seeded state in both halves; only the owner's
/// lock moves.
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
        // The owner's lock was asked, once, about the owner's own directory —
        // the step the hoisted comparison skipped.
        assert_eq!(
            harness.liveness.asked(),
            vec![owner.run_dir.clone()],
            "[{tag}] arm (ii) reached without probing the owner's run.lock"
        );
    }
}

/// A labeled container whose name no funnel could have written blocks
/// admission, and one whose labels do not say who owns it blocks admission.
///
/// `refusal_condition`: "a dead owner's or dead incarnation's labeled container
/// that **cannot be observed terminated** blocks admission". A container
/// claiming this private root that the funnel cannot name, or whose ownership
/// cannot be established, is one this census cannot take through
/// kill/observe/rm — so it refuses rather than proceeding past it.
///
/// Second field held constant: every case carries a valid `tactus.private_root`
/// label under the censused root, so what is under test is only what is missing
/// beside it.
#[test]
fn a_labeled_container_this_census_cannot_own_blocks_admission() {
    use crate::runner::container::runtime::DiscoveredContainer;
    use std::collections::BTreeMap;

    let owner = Owner::new(RUN_B, INC_1, REPO_KEY_A);
    let good = owner.name(&shell_probe());

    // `(what, the name the runtime reports, the label to withhold, the needle)`.
    // Data rather than closures, so every case builds the same complete label
    // set and then breaks exactly one thing about it.
    let cases: [(&str, &str, Option<&str>, &str); 4] = [
        (
            "a name no funnel could have written",
            "someone-elses-container",
            None,
            "not a tactus container name",
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
        labels.insert(LABEL_RUN_DIR.to_owned(), "/repo/.tactus/runs/x".to_owned());
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

/// A name and its ownership evidence that disagree refuse.
///
/// The name is `tactus-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`, so
/// its components **are** ownership evidence. A record that says one incarnation
/// while its own file name says another would mean classifying on one value and
/// reclaiming a container named for the other.
///
/// Second field held constant: the container exists and is running in every
/// case; only which of the three components disagrees moves.
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

/// A container whose labels and whose record disagree about its owner refuses.
///
/// The labels are derived from the record when a container is created
/// (`ContainerIntent::labels`), so a disagreement is not a state this engine
/// wrote — and picking a winner would mean deciding, from corrupted evidence,
/// whether to kill a container.
///
/// Second field held constant: the container name, the private root and the
/// record are the same in both cases; only which label was tampered with moves.
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

// ---------------------------------------------------------------------------
// 4. Recovery step (a1) — the stable-prefix barrier
// ---------------------------------------------------------------------------

/// The four predicates of the barrier are separately droppable, and each has its
/// own refusal.
///
/// `crash_reconstruction`: the census happens "after the stable-prefix barrier
/// of step (a1) has **synced** the surviving event-log prefix, **proven it
/// stable**, and **checked-replayed it**, so that no fold-derived reclaim
/// decision precedes durability". Reclaim decided from a prefix that was synced
/// but not proven stable, or proven stable but not replayed, is reclaim on
/// unproven authority.
///
/// The digests are computed **out of band** (`python3 -c 'hashlib.sha256(...)'`)
/// and written here as literals, so the barrier is not compared against the
/// function that produced it.
///
/// Second field held constant: every case starts from the same healthy triple
/// and breaks exactly one predicate.
#[test]
fn the_stable_prefix_barrier_refuses_each_of_its_four_predicates_independently() {
    const PREFIX: &[u8] = b"{\"event\":\"run_started\"}\n";
    const PREFIX_SHA: &str = "2f9864f5b2e0acc40bf4a8b9fb5ae52b142cdcd0870db42ddcac489991b5206d";
    const LONGER: &[u8] = b"{\"event\":\"run_started\"}\n{\"event\":\"attempt_started\"}\n";
    const LONGER_SHA: &str = "9f6a5ec6a50778f18bc1fc9b3ff2286a43c4130479cf391cf321743450e5acc8";
    const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    // The measurement agrees with the out-of-band digests, so neither side is
    // the other's oracle.
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

    // 1. The boundary moved between the two reads.
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

    // 2. The bytes changed while the boundary stayed put.
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

    // 3. Proven stable, and not durable to its boundary.
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

    // 4. Synced and proven stable, and the replay consumed other bytes.
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

    // A replay of the same length but different content is refused too: length
    // alone is not identity.
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

// ---------------------------------------------------------------------------
// 5. Discovery, both halves, every cell
// ---------------------------------------------------------------------------

/// Both halves of discovery are scanned, and every cell of `{intent present} ×
/// {container present}` is classified.
///
/// "discovery at every write-command start scans the whole namespace
/// `<R>/containers` … **and** docker ps by `tactus.private_root`". A census that
/// read only the namespace misses a labeled orphan whose record was already
/// removed; one that read only `docker ps` misses an intent whose container the
/// Unix reaper already killed and removed — which is the *ordinary* state after
/// a Unix coordinator death, because the reaper does kill/rm and leaves the
/// record for the next census.
///
/// Second field held constant: one owner, one liveness answer, one private root;
/// only which halves hold evidence moves.
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
                // The two intent-only situations are indistinguishable to
                // discovery, which is the point of `PR6-CONV-003`: they differ
                // only in whether a view is on disk, and
                // `an_intent_only_candidate_after_the_reaper_still_has_its_view_pruned`
                // is the fixture that varies that.
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

    // The fourth cell — neither half — is the empty machine, and it is what the
    // census reports when nothing is there.
    let empty = Harness::new("neither-half");
    let complete = empty.census(&fresh(INC_2)).expect("an empty namespace");
    assert!(complete.report().reclaimed.is_empty());
}

/// The label this census filters on is the label the funnel writes, and its
/// value is the one an independent table says it is.
///
/// A census that filtered on a different spelling would discover nothing and
/// report a clean machine — the "green because the test could not run" shape,
/// with the runtime standing in for the test. There is now **one** rendering
/// (`intent::private_root_label`) and both sides call it, so the agreement is
/// by construction; what this test still has to hold is the *value*, and it
/// holds it against encodings computed **out of band** and written as literals.
/// Comparing the function against itself would prove nothing, which is how the
/// two-copy version stayed green while the encoding was wrong.
///
/// Second field held constant: one record and one owner across every cell, so
/// the only thing that moves is the root's bytes.
#[test]
fn the_private_root_label_this_census_filters_on_is_the_one_the_intent_writes() {
    // Computed with `python3 -c` from the rule "percent-encode every byte
    // outside [A-Za-z0-9/:.-_]", not by calling the function under test.
    const EXPECTED: &[(&str, &str)] = &[
        ("/srv/tactus/private", "/srv/tactus/private"),
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

    // The one byte whose rendering is platform-shaped, stated as the two
    // answers rather than as the one this platform happens to give.
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

/// **Different private roots are disjoint worlds** — proved with a collision
/// pair, not with a round-trip.
///
/// `crash_reconstruction` says it in those words, and the container **name**
/// was designed for it: its components are `[0-9A-Za-z_]` only, so the parse on
/// `-` is unambiguous. The **label** is the other half — it is what `docker ps
/// --filter label=tactus.private_root=…` selects on — and the rendering that
/// shipped, `to_string_lossy().replace('\\', "/")`, was not injective. On Unix a
/// backslash is an ordinary filename byte, so `<base>/a\b` and `<base>/a/b` are
/// **different directories** that rendered to one label, and a census
/// authorized for either queried and reclaimed the other's containers.
///
/// A round-trip test would not have caught it: the encoding round-trips
/// perfectly and still maps two inputs to one output. So this asserts a
/// **distinct-value count** over roots that differ only in the bytes an
/// encoding is tempted to fold, and it names the pair the review found.
///
/// Second field held constant: every root shares the same `<base>` prefix and
/// differs in exactly one interior byte, so nothing but that byte can be
/// producing the distinctness.
#[test]
fn the_private_root_label_is_injective_over_hostile_roots() {
    let base = Path::new("/srv/private");

    // Distinct on **every** platform: none of these pairs differ only by a
    // path separator.
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

    // The collision pair the review measured. On Unix these are two
    // directories; on Windows they are one, and folding them is
    // canonicalization rather than a collision — so the claim is made on the
    // platform where it is a claim.
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

    // The second collision the shipped rendering had, and the one a
    // `\u{fffd}` in the table above only gestures at: `to_string_lossy` maps
    // **every** ill-formed byte sequence to `U+FFFD`, so two distinct non-UTF-8
    // roots — and a root that literally contains `U+FFFD` — were one label.
    // Constructible only on Unix, where an `OsStr` is bytes.
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

    // A root may not carry a byte that ends the `--filter` argument or starts
    // another filter, whatever the operator called their directory. This is
    // what lets `ReaperContainerScope` stop worrying about the root half.
    for root in &roots {
        let label = private_root_label(root);
        assert!(
            !label.contains([',', '=', '\n', '\r']),
            "`{label}` would change what `--filter label=…` selects"
        );
    }
}

/// Every topology write command performs the census — `run` **and** `resume`.
///
/// `startup_census`: "performed by **every topology write command (run,
/// resume)**". Guarding it behind resume-only logic lets dead containers survive
/// into a fresh run's admission.
///
/// Second field held constant: the orphan, its owner, its liveness and the
/// private root are identical between the two halves; only the write command
/// moves.
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

// ---------------------------------------------------------------------------
// 6. The token, and what it precedes
// ---------------------------------------------------------------------------

/// [`CensusComplete`] is constructed in exactly one place.
///
/// `crash_reconstruction`'s four "before"s — slot/reservation initialization,
/// admission, credential-volume use, and this incarnation's probes — are
/// consumers PR7 and PR11 build. This slice cannot test against a consumer that
/// does not exist, so what it holds instead is that the token those consumers
/// will take can be made in exactly one way: by a census that completed.
///
/// The source census is the tree's own idiom
/// (`runner::container::tests::every_container_effect_in_the_tree_goes_through_the_funnel`),
/// and it has a positive control so a scan that stopped finding anything fails
/// rather than reporting silence.
///
/// Second field held constant: **none, and that is the answer rather than an
/// omission.** This is a census over the whole tree, so the axis it varies is
/// *which file* and there is no other field to pin. What replaces a second axis
/// here is the positive control — a scan whose needle stopped matching would
/// otherwise report an empty offender set and pass.
#[test]
fn census_returns_the_only_token_that_reaches_a_consumer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let census_module = root.join("src/runner/container/census.rs");
    let mut offenders = Vec::new();
    let mut scanned = 0;
    for path in walk(&root.join("src")) {
        let source = fs::read_to_string(&path).expect("read source");
        let production =
            crate::effects::blank_comments_and_strings(&crate::effects::production_region(&source));
        scanned += 1;
        if path == census_module {
            continue;
        }
        if production.contains("CensusComplete {") {
            offenders.push(path.display().to_string());
        }
    }
    assert!(scanned > 20, "the walk found the tree: {scanned}");
    assert!(
        offenders.is_empty(),
        "`CensusComplete` is constructed outside the census: {offenders:#?}"
    );

    let production =
        crate::effects::blank_comments_and_strings(&crate::effects::production_region(
            &fs::read_to_string(&census_module).expect("the census module"),
        ));
    // The positive control. `CensusComplete {` appears three times here — the
    // declaration, the `impl` header and the one construction — so the control
    // needle is the construction shape alone, and the scan above would find it
    // if it moved into another file.
    assert_eq!(
        production.matches("Ok(CensusComplete {").count(),
        1,
        "the census constructs its token exactly once, so the scan above is measuring \
         something"
    );
    assert_eq!(production.matches("CensusComplete {").count(), 3);

    // And the type really is closed: its field is private, so no other module
    // can build one even with a struct literal.
    let harness = Harness::new("token-shape");
    let complete = harness.census(&fresh(INC_1)).expect("an empty census");
    assert_eq!(complete.report().incarnation, INC_1);
    assert_eq!(complete.report().orphan_window, super::orphan_window());
}

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

// ---------------------------------------------------------------------------
// 7. The resource rows this census is accountable for
// ---------------------------------------------------------------------------

/// R20 is `operator_owned` and `persistent_output` in **all five**
/// `at_run_end` outcomes, and no census path touches it.
///
/// `resource_accounting[R20]`: "per-agent credential volume … `persistent_output`
/// (**never created or pruned by a run**)" for `Complete`, `Parked`, `Halted`,
/// `BudgetExceeded` and `NoRunFinished`. A run that tidied a volume it mounted
/// would destroy operator credentials, and the CLIs **rotate refresh tokens on
/// use**, so a discarded rotation forces a re-login.
///
/// Two halves, because either alone is weak: the five outcomes are transcribed
/// from the packet as an independent table, and the census is measured to issue
/// no volume operation at all on a fixture that reclaims two containers.
///
/// Second field held constant: a volume **is present** throughout, and two
/// containers really are reclaimed around it. Varying only the outcome column
/// would leave a table nothing executes; varying only the census would leave a
/// run that never had a volume to spare. The pair is what makes "never created
/// or pruned by a run" a measurement.
#[test]
fn r20_is_persistent_output_in_every_at_run_end_outcome_and_no_census_path_touches_it() {
    /// Transcribed from `decisions.resource_accounting.rows[R20].at_run_end`,
    /// not read back from any code.
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
    harness.runtime.add_volume("tactus-claude-code");
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
            .volume_present("tactus-claude-code")
            .expect("ask the runtime"),
        "the volume this census reclaimed containers around is still there"
    );

    // And the module names no volume operation at all.
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

/// R26 is `released` in `Complete`, `Parked`, `Halted` and `BudgetExceeded`, and
/// the census is the mechanism for the fifth cell.
///
/// `resource_accounting[R26].at_run_end`: four outcomes release the container
/// (`release`, which is the funnel's completion sequence), and `NoRunFinished`
/// is reclaimed at the next write-command start — which is this module. A
/// container surviving a **budget stop** or a **park** would keep spending while
/// the run is supposed to be quiescent, which is why the first four are
/// `released` rather than "left for the census".
///
/// The four `released` cells belong to `release` and are held by
/// `runner::container::tests`; what is executed here is the fifth, and that a
/// container left by a run that never finished is gone, record and view with it.
///
/// Second field held constant: the owner is dead and the container is running
/// in the executed half, so the only thing distinguishing `NoRunFinished` from
/// the four `released` outcomes is **which mechanism disposes of it** — the
/// census here, `release` there. All three of R26's container, R19's view and
/// R26's record are asserted gone, because a fifth cell that pruned two of
/// three would leave the ledgers unbalanced in a way a single assertion misses.
#[test]
fn r26_is_released_in_four_outcomes_and_the_census_is_the_mechanism_for_no_run_finished() {
    /// Transcribed from `decisions.resource_accounting.rows[R26].at_run_end`.
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

/// The observation wait is a step, not an implementation detail, and it is
/// **bounded**.
///
/// "reclaim = docker kill -> **wait until observed exited/removed** -> docker rm
/// …". Dropping the wait is the classic mutation: `kill` then `rm` still leaves
/// the container gone at the end, so a test that only checks the final state
/// passes. Here the container never terminates, the bound is exhausted, and the
/// refusal names the clause — and `docker rm` is never issued, which is what
/// says the wait sits **between** kill and rm rather than after both.
///
/// Second field held constant: the same container, owner and dead-owner verdict
/// as [`orphan_reclaimed_before_slot_reset`]; only whether `stop` actually stops
/// it moves.
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

// ---------------------------------------------------------------------------
// 8. The Unix reaper's selector — ST-16 (d)'s half that is pure
// ---------------------------------------------------------------------------

/// The reaper's selector names **both** labels, and every component varies the
/// rendering independently.
///
/// `os_matrix`: the reaper "kills the **dead coordinator's** labeled
/// containers". `tactus.private_root` alone names every container of every run
/// under `<R>`, including a **live** coordinator's — which
/// `T-CONTAINER.authoritative_state` forbids in as many words ("a live
/// incarnation's containers must not be touched"). The incarnation is a
/// per-process ULID and is what makes the selector name one coordinator.
///
/// Second field held constant: the program is the same in every cell, so the
/// only thing that moves is the pair of label values.
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

    // kill and rm carry the id and nothing else, and the reaper does only those
    // two: the view and the record are the next census's.
    let scope = super::ReaperContainerScope::new("docker", roots[0], INC_1).expect("a scope");
    assert_eq!(scope.kill_argv("abc"), vec!["docker", "kill", "abc"]);
    // `--volumes` is in the table because the reaper is the last thing that can
    // name a container's **anonymous** volumes: after it removes the container
    // the following intent-only census has no handle on them
    // (`PR6-ACCT-006`). The expected vector is written out here rather than
    // read back from the function.
    assert_eq!(
        scope.remove_argv("abc"),
        vec!["docker", "rm", "--force", "--volumes", "abc"]
    );
    assert_eq!(scope.program(), Path::new("docker"));
}

/// A label value that could change what the filter selects cannot reach the
/// reaper — refused for the incarnation, impossible for the root.
///
/// The reaper has no error channel and no allocator: it cannot report a
/// malformed selector, and a filter that matched more than it should would kill
/// a live coordinator's containers. The two halves of the selector are now
/// protected differently, and the difference is the point:
///
/// * the **incarnation** is used verbatim, so a hostile value is **refused**
///   here, on the parent side;
/// * the **root** is rendered by [`private_root_label`], which percent-encodes
///   every byte that could end the argument, so a hostile root is *accepted*
///   and cannot widen anything. This is strictly stronger than refusing it: an
///   operator whose private root contains a comma gets a working reaper rather
///   than a refusal. The scope's own check is kept as the post-condition on
///   that encoding — it inspects the **rendered** value, so an encoding that
///   regressed would still fail closed here.
///
/// Second field held constant: the private root is well-formed in the
/// incarnation cases and vice versa, so each case names one hostile value.
#[test]
fn a_reaper_scope_whose_label_value_could_widen_the_filter_cannot_reach_the_reaper() {
    let good_root = Path::new("/srv/private");
    let hostile = ["", "01KZ\nlabel=tactus.run", "a,b", "a=b"];
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

    // The root half. An empty root still refuses — it renders to an empty
    // label, and a filter that matches everything would kill a live
    // coordinator's containers. Every other hostile root is accepted, and the
    // selector it produces carries exactly two filters.
    assert!(
        super::ReaperContainerScope::new("docker", Path::new(""), INC_1).is_err(),
        "an empty private root renders an empty filter value"
    );
    for value in ["01KZ\nlabel=tactus.run", "a,b", "a=b"] {
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

    // And the well-formed pair is accepted, so this is not a function that
    // refuses everything.
    assert!(super::ReaperContainerScope::new("docker", good_root, INC_1).is_ok());
}

// ---------------------------------------------------------------------------
// 9. Docker-gated: a census against the real runtime
// ---------------------------------------------------------------------------

/// A census over **real Docker** reclaims a dead owner's labeled orphan and
/// leaves a live owner's container alone.
///
/// The fake proves the decision; this proves the decision survives contact with
/// the runtime the decision is about — `docker ps --filter label=…` really does
/// return the containers this census expects, `docker kill`/`rm` really are
/// idempotent, and `observe` really does report a removed container as gone.
///
/// **Never pulls** (`non_goals[1]`): the image is discovered among what the
/// machine already holds, and a machine holding none reports absence through the
/// same loud, counted gate.
///
/// Second field held constant: both containers are created from the same image
/// with the same command under the same private root; the only thing that
/// differs is whether their owner's run directory is reported live.
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
            "TACTUS_REQUIRE_DOCKER is set and the runtime holds none of the images these \
             tests may use; they never pull (non_goals[1])"
        );
        return;
    };

    // Owner constants THIS TEST ALONE uses. Container names are deterministic
    // and the daemon is one namespace shared with every other Docker-gated test
    // in this tree, which run concurrently: reusing the fixture constants above
    // made `docker create` fail with a name conflict against
    // `runner::container::tests`'s own gated test. Measured, and the reason
    // these four constants exist.
    const REAL_REPO_KEY: &str = "cccccccccccccccc";
    const REAL_RUN_LIVE: &str = "01KZTREALLIVE00000000000AA";
    const REAL_RUN_DEAD: &str = "01KZTREALDEAD00000000000BB";
    let root = scratch("real-docker-census");
    let live = Owner::new(REAL_RUN_LIVE, INC_1, REAL_REPO_KEY);
    let dead = Owner::new(REAL_RUN_DEAD, INC_2, REAL_REPO_KEY);
    let liveness = RecordingLiveness::new();
    liveness.set_live(&live.run_dir);

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

    // Whatever happened, do not leave real containers behind.
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

/// A record that disappears between the namespace scan and the read of it is
/// **skipped**, deterministically.
///
/// This is the discovery half of "every step idempotent and tolerant of
/// already-gone so **two concurrent reclaimers converge**", and the racing
/// fixture above reaches it only by luck. The state it reaches — a directory
/// entry whose file is not there — is constructible on demand as a **dangling
/// symlink**: `read_dir` lists it and `fs::read` answers `NotFound`, which is
/// byte-for-byte the answer the losing reclaimer gets.
///
/// Measured, not assumed: before the repair in `list_intents`, a whole write
/// command refused with `Io { NotFound }` because another write command was
/// tidying at the same moment.
///
/// Second field held constant: a real, readable record sits beside the vanished
/// one in the same namespace, so the test cannot pass by skipping everything.
///
/// Unix-only because a dangling symlink needs a privilege the Windows guest's
/// test user does not have; the racing fixture above covers the same property
/// on every platform, less sharply, and this comment is the record of which
/// half runs where.
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

    // And a record that is present but unreadable is still an error: "the
    // record could not be read" and "the record is gone" are different answers,
    // and only one of them licenses proceeding. Two shapes, because the
    // tolerance has two ways to be too wide.
    let malformed = Harness::new("malformed-record");
    let torn = dead.name(&shell_probe());
    let torn_path = torn.intent_path(&malformed.root);
    fs::create_dir_all(torn_path.parent().expect("the namespace")).expect("namespace");
    fs::write(&torn_path, b"{ this is not a container intent").expect("a damaged record");
    assert!(
        malformed.census(&fresh(INC_2)).is_err(),
        "a damaged record was treated as an absent one"
    );

    // The one that matters for the Windows repair: a record whose read fails
    // with **`PermissionDenied`** and keeps failing. The repair tolerates that
    // errno while a delete is pending, and a repair that tolerated it outright
    // would let a census admit over a container whose ownership evidence it
    // could not read. The bound is what separates the two, and this is the
    // fixture that holds the separation.
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

// ---------------------------------------------------------------------------
// 12. `PR6-RECOV-001` — the owner's run directory is recorded injectively
// ---------------------------------------------------------------------------

/// Run directories that a lossy rendering maps onto **each other**.
///
/// The oracle of every test in this section, and it is a table of *pairs*: an
/// encoding is proved wrong by a collision, and a round trip cannot see one.
/// Each entry is `(what, left, right)`, and both sides are directories a
/// filesystem can name.
///
/// The rendering this replaced — `to_string_lossy().replace('\\', "/")` —
/// collides on the platform-specific pairs below, and the mutation an
/// independent review measured (extending the rewrite to another valid byte
/// such as `:`) collides on the first universal one. The universal pairs run
/// **everywhere**, so the property is not one a Windows build stops checking.
fn colliding_run_dir_pairs() -> Vec<(&'static str, PathBuf, PathBuf)> {
    let mut pairs: Vec<(&'static str, PathBuf, PathBuf)> = Vec::new();
    pairs.extend([
        (
            "a colon, which the reviewer's mutation rewrote next",
            PathBuf::from("/repo/.tactus/runs/A:B"),
            PathBuf::from("/repo/.tactus/runs/A/B"),
        ),
        (
            "a comma beside its own escape: `%` must escape itself",
            PathBuf::from("/repo/a,b/.tactus/runs/X"),
            PathBuf::from("/repo/a%2Cb/.tactus/runs/X"),
        ),
        (
            "a literal percent beside its escape",
            PathBuf::from("/repo/a%b/.tactus/runs/X"),
            PathBuf::from("/repo/a%25b/.tactus/runs/X"),
        ),
    ]);
    // Unix only, and each for a stated reason. A backslash is an **ordinary
    // filename byte** there — `/repo\a/...` is a directory whose first
    // component is literally `repo\a` — while on Windows `\` and `/` are both
    // separators and folding them is canonicalization rather than a collision.
    // An ill-formed byte sequence is not constructible as a Windows path at
    // all.
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;
        pairs.extend([
            (
                "a backslash is an ordinary filename byte on Unix",
                PathBuf::from(r"/repo\a/.tactus/runs/X"),
                PathBuf::from("/repo/a/.tactus/runs/X"),
            ),
            (
                "and so is a backslash in the run id's own component",
                PathBuf::from(r"/repo/.tactus/runs/A\B"),
                PathBuf::from("/repo/.tactus/runs/A/B"),
            ),
            (
                "two ill-formed byte sequences, both `U+FFFD` under to_string_lossy",
                PathBuf::from(OsStr::from_bytes(b"/repo/.tactus/runs/\xff")),
                PathBuf::from(OsStr::from_bytes(b"/repo/.tactus/runs/\xfe")),
            ),
            (
                "an ill-formed sequence and a literal replacement character",
                PathBuf::from(OsStr::from_bytes(b"/repo/.tactus/runs/\xff")),
                PathBuf::from("/repo/.tactus/runs/\u{fffd}"),
            ),
        ]);
    }
    pairs
}

/// The recorded run directory is **injective**, proved on colliding pairs.
///
/// `crash_reconstruction` records "run directory (**public path**)" and arm (ii)
/// probes "that run's run.lock". `PR6-RECOV-001`: with the shipped rendering,
/// live run B under `/repo\a/...` recorded `/repo/a/...`, a **different, real**
/// directory; a foreign census probed there, found no lock, called B dead and
/// killed B's running container.
///
/// Asserted as a distinct-value count over the pairs and then again as a
/// pairwise inequality, so a rendering that collided *one* pair could not hide
/// inside a set that happened to stay the right size.
///
/// Second field held constant: one owner run id, one incarnation, one repo key,
/// one invocation — only the run directory moves.
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
        // And the record still names the directory it was built from: an
        // injective encoding nobody can undo would send the probe nowhere.
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
    // Across the whole table at once, and keyed by path because a directory may
    // appear in more than one pair: `n` distinct directories must record `n`
    // distinct values, so an encoding that merged two paths from *different*
    // pairs is caught as well as one that merged a pair.
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
    // The label the container carries is the same string, so the two halves of
    // discovery cannot disagree about where the owner's lock is.
    for (encoded, path) in &recorded {
        assert_eq!(
            &path_label(path),
            encoded,
            "the record and `intent::path_label` render one path two ways"
        );
    }
}

/// The census probes the directory the owner really used — a **live** owner
/// under a hostile path is not killed.
///
/// The end of `PR6-RECOV-001`'s failure sequence, as a runtime state rather than
/// as a string comparison. Live run B holds the lock of `/repo\a/.tactus/runs/B`
/// and a foreign census runs; the neighbouring directory `/repo/a/.tactus/runs/B`
/// is deliberately **free**, so a census that probes the lossy rendering
/// classifies B dead and kills its container.
///
/// Second field held constant: an ordinary dead owner is seeded beside B in
/// both halves and must be reclaimed either way, so "nothing happened" cannot
/// pass this.
#[test]
#[cfg(unix)]
fn a_live_owner_under_a_hostile_run_directory_is_probed_where_it_actually_is() {
    let harness = Harness::new("hostile-run-dir-live-owner");
    let real = PathBuf::from(r"/repo\a/.tactus/runs/B");
    let lossy = PathBuf::from("/repo/a/.tactus/runs/B");
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

/// A label-only container carries the same encoding, so the label half of
/// discovery reaches the same lock.
///
/// `{intent present} × {container present}` is a real grid and the label-only
/// cell has its own path into `Candidate.run_dir`
/// (`census::from_labels_alone`). An encoding applied on one side only would
/// pass every intent-carrying fixture.
///
/// Second field held constant: the same owner, the same hostile directory and
/// the same lock state as the intent-carrying case above; only which half of
/// discovery found it moves.
#[test]
#[cfg(unix)]
fn a_label_only_container_under_a_hostile_run_directory_reaches_the_same_lock() {
    let harness = Harness::new("hostile-run-dir-label-only");
    let real = PathBuf::from(r"/repo\a/.tactus/runs/B");
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

/// The encoding is undone **exactly**, and a value no funnel could have written
/// is refused rather than guessed at.
///
/// The fail-closed half. `decode_path_label` is what turns evidence into the
/// path a lock is probed in, so a malformed value must not become *some* path:
/// every wrong probe answers "free", and "free" reclaims.
///
/// Second field held constant: one decoder, one call shape; the table varies
/// only the value handed to it, across well-formed and malformed.
#[test]
fn a_path_label_decodes_exactly_or_refuses() {
    // Well-formed: `(the value, the path it names)`. The oracle is written out
    // by hand rather than taken from `path_label`, which is the function under
    // test's own inverse.
    let exact: &[(&str, &str)] = &[
        ("/repo/.tactus/runs/X", "/repo/.tactus/runs/X"),
        ("/repo%5Ca/runs/X", r"/repo\a/runs/X"),
        ("/repo/a%2Cb", "/repo/a,b"),
        ("/repo/a%3Db", "/repo/a=b"),
        ("/repo/a%25b", "/repo/a%b"),
        ("/repo/a%20b", "/repo/a b"),
        ("C:/repo/runs", "C:/repo/runs"),
        ("/repo/caf%C3%A9", "/repo/caf\u{e9}"),
    ];
    for (value, expected) in exact {
        // Decoding is the same on both platforms: it is a function of the
        // value's own bytes and knows nothing about separators.
        assert_eq!(
            decode_path_label(value).expect("well formed"),
            PathBuf::from(expected),
            "`{value}`"
        );
        // The encode direction is a fixed point too — **except** for the one
        // byte that is platform-shaped. On Windows `\` and `/` are both
        // separators, so `<x>\a` and `<x>/a` name one directory and rendering
        // the backslash as `/` maps *equal* paths to one label, which is the
        // canonicalization injectivity over paths asks for. On Unix `\` is an
        // ordinary filename byte and is escaped like any other.
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

    // Malformed. Each is a shape `path_label` cannot emit.
    for value in ["%", "%5", "%zz", "/repo/%g0/x", "/repo/x%", "/repo/%5c/x"] {
        let error = decode_path_label(value)
            .expect_err("a value no funnel could have written must be refused, not guessed at");
        assert!(
            error.to_string().contains(value),
            "the refusal must name the value: {error}"
        );
    }
    // Lower-case hex is deliberately refused: `path_label` emits upper case, so
    // accepting both would give one path two labels and lose injectivity in the
    // other direction.
    assert!(decode_path_label("/repo/%5c/x").is_err());
    assert!(decode_path_label("/repo/%5C/x").is_ok());
}

/// `PR6-CORRECTNESS-016` — a run directory that does not say where its owner's
/// lock is blocks admission, from **either** evidence source.
///
/// `expected_failures_refusals[8]`: "an unreclaimable labeled container blocks
/// admission". The shipped code refused a *missing* `tactus.run_dir` and
/// accepted `tactus.run_dir=`, which joined to `run.lock` — a path relative to
/// this process's working directory, where there is no lock — so a live foreign
/// owner was classified dead and its container killed.
///
/// The grid is `{empty, relative, malformed} × {from the record, from the
/// labels}`, because the two sources reach `Candidate.run_dir` down different
/// code paths and the shipped check was on neither.
///
/// Second field held constant: the container's name, run and incarnation labels
/// are valid and identical in every cell, so nothing but the run-directory value
/// moves — a cell that refused for the wrong reason would say so.
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
        // (a) From the labels, with no record: `from_labels_alone`.
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

        // (b) From the record: the same predicate, the other path in.
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

    // The rooted values the same function must **accept**, so the check is not
    // simply "refuse everything". `has_root` and not `is_absolute`: on Windows
    // `is_absolute` additionally wants a prefix, and `/repo/...` is the shape
    // every fixture and every Unix-written record carries.
    for good in ["/repo/.tactus/runs/B", "/repo/a%5Cb/runs/B"] {
        owner_run_dir(good, "test").unwrap_or_else(|error| panic!("`{good}`: {error}"));
    }
}

// ---------------------------------------------------------------------------
// 13. `PR6-RECOV-005` — the census's runtime-required rule, over the
//     diagnostics a real `docker` prints
// ---------------------------------------------------------------------------

/// `{intent present} × {verbatim docker diagnostic}`, through the production
/// classifier.
///
/// `crash_reconstruction`: "the container runtime is required **only** when an
/// intent exists or a labeled container is discoverable: if any intent exists
/// and the runtime cannot be reached the write command refuses …, and with no
/// intent and no reachable runtime it **proceeds**."
///
/// The finding's own note is why this test exists in this shape: every other
/// census fixture arms `RuntimeError::Unreachable` **directly**, so nothing
/// exercised the function that decides whether a real diagnostic *is*
/// unreachability — and the shipped one classified `permission denied while
/// trying to connect to the docker API` as an answered failure, which made a
/// census with **no container evidence at all** refuse. Here the fake is armed
/// with the verbatim stderr and `super::super::classify_docker_failure` picks
/// the variant.
///
/// Second field held constant: the same private root, the same absent
/// container, the same write command in every cell — only the diagnostic and
/// whether an intent is on disk move.
#[test]
fn a_census_with_no_intents_proceeds_past_every_diagnostic_that_means_unreachable() {
    // `(what, verbatim stderr, does it mean the daemon was never reached)`.
    // Measured on docker 29.7.2; see `container::tests::UNREACHABLE_STDERR`.
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
        // (a) No intents, no labeled container. "with no intent and no
        // reachable runtime it proceeds".
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

        // (b) The same diagnostic with **one intent** on disk: refused either
        // way. This is the axis a fixture that only varied the diagnostic
        // would miss — a classifier repaired into "always unreachable" passes
        // half (a) and admits over a container it cannot prove terminated.
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

// ---------------------------------------------------------------------------
// 14. `PR6-ENUM-009` — ST-16 (h)'s racers are a **foreign write command and a
//     resuming incarnation**, which is a role intersection, not one role twice
// ---------------------------------------------------------------------------

/// A **resuming** incarnation converges on a container a foreign **fresh**
/// census already removed.
///
/// ST-16 (h) is "(h) two concurrent reclaimers (**a foreign write command and
/// the resuming incarnation**) converge idempotently on the same dead
/// container", and the seam test's `slice` field says "PR11 (under
/// concurrency)". `concurrent_reclaimers_converge` races two `FreshRun`
/// censuses, so `{racer role} × {racer role}` has one cell filled and the named
/// one empty: the reviewer's mutation was to break **only** the Resume path
/// when it finds a container already gone, and every Fresh/Fresh fixture stays
/// green under it (`PR6-ENUM-009`).
///
/// **What PR6 owns and what PR11 owns**, stated here rather than in a table
/// somewhere else: PR6 owns that each *role* converges on already-gone state —
/// deterministically here, and interleaved in
/// [`a_fresh_and_a_resuming_census_race_one_container_and_converge`]. PR11 owns
/// the clause "under concurrency" in the sense ST-16 means it, which is **two
/// coordinator processes**: this slice has no `TopologyRun` to start a second
/// one with, and a resume's own precondition — holding its run lock — is PR7's
/// to establish.
///
/// The deterministic half first, because an interleaving test that passes for
/// the wrong reason is hard to see: the fresh census reclaims, then the
/// resuming one runs over the same root and must return a clean report rather
/// than an error.
///
/// Second field held constant: one root, one container, one owner; only which
/// role's census is second moves — and both orders are run.
#[test]
fn a_resume_converges_on_a_container_a_foreign_fresh_census_already_removed() {
    // `(tag, first, second)`. Both orders, because "converge" is symmetric and
    // an implementation that broke one direction is not converging.
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

        // The second reclaimer sees the container gone, the record gone and the
        // view gone — the ordinary post-reaper state — and must converge.
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
        // And the two roles really were different, which is the axis the
        // Fresh/Fresh fixtures hold constant.
        assert_ne!(one.report().command, two.report().command, "[{tag}]");
    }
}

/// A **fresh** census and a **resuming** one race one container and converge.
///
/// ST-16 (h)'s racers, interleaved rather than sequenced — the same instrument
/// [`concurrent_reclaimers_converge`] uses, with the second axis filled in. Two
/// threads, released together by a [`Barrier`], over many rounds; one starts as
/// `FreshRun` and the other as `Resume`, and neither may refuse.
///
/// This is still *in-process* concurrency: two real coordinator processes are
/// PR11's, and the run-lock precondition a resume carries is PR7's. What it
/// holds is that the reclaim steps converge when a resume and a foreign write
/// command interleave, which is the part expressible in this slice.
///
/// Second field held constant: both racers get the same runtime, the same root
/// and the same containers; only the `CensusStart` differs between them.
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
        // The two roles, distinct: a foreign write command that holds no run
        // lock, and an incarnation resuming a run of its own. RUN_A is neither
        // container's owner, so both racers reach arm (ii) and the containers
        // are reclaimable by either.
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
        // Somebody did the work, in each role. The loser of a step may
        // legitimately find a container already gone and report fewer; between
        // them they must account for all four, and neither may refuse — which
        // is asserted above. Both reporting all four is the ordinary outcome
        // of two interleaved idempotent reclaimers and is not a defect.
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

// ---------------------------------------------------------------------------
// 15. `PR6-ENUM-010` — ST-16 (j)'s "before any recovery event", split
// ---------------------------------------------------------------------------

/// The refusal of ST-16 (j) happens before the census has done **anything**,
/// and before it can hand anybody the token that reaches a recovery event.
///
/// ST-16 (j): "with container intents present and the runtime unreachable the
/// write command refuses **before any recovery event**". That clause is an
/// ordering between the refusal and an *event log* — and this slice has no
/// event log and no production caller (`production_effect` is "none"; PR7 wires
/// `TopologyRun`). `PR6-ENUM-010` is that the reconciliation assigned the whole
/// clause to PR6 with no deferral recorded, so the surviving mutation is a
/// future caller that appends a recovery event **before** invoking the census.
///
/// **The split, stated so it is not rediscovered.** PR6 owns two predicates,
/// both asserted below:
///
/// 1. the refusal precedes every effect this census could perform — no funnel
///    site, no runtime operation beyond the reachability question itself, no
///    record or view touched;
/// 2. the refusal precedes the **`CensusComplete`** token, which by
///    construction is the only value that reaches the four consumers
///    (`census_returns_the_only_token_that_reaches_a_consumer`).
///
/// PR7 owns the third: that its `TopologyRun` calls the census **before** it
/// appends any recovery event. Nothing in this slice can hold that, and saying
/// so is the deferral.
///
/// Second field held constant: the same single intent and the same root in both
/// halves; only the runtime's answer moves.
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

    // (1) No effect at all, and the runtime was asked exactly one question.
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

    // (2) No token. The token is the only value that reaches a consumer, so a
    // refusal that produced one would have licensed the four things the census
    // precedes — of which "any recovery event" is the one ST-16 (j) names.
    assert!(
        outcome.is_err(),
        "a refusing census must produce no CensusComplete"
    );
}

// ---------------------------------------------------------------------------
// R3b: the post-reaper state, the recovery anchor, the settlement, the staged
// half, and what a removal that never succeeds does to admission
// ---------------------------------------------------------------------------

/// The ordinary post-Unix-reaper state: no container, **a view**, an intent.
///
/// `PR6-CONV-003`. `DiscoveredBy::IntentOnly` covers two situations — a crash
/// between the intent write and `docker create`, and the state the Unix reaper
/// leaves, since it performs `kill/rm` and nothing else
/// (`T-CONTAINER.resume_action`). Every fixture seeded only the first, so
/// "intent-only" and "no view" were perfectly correlated and a reclaim that
/// skipped view cleanup for intent-only candidates removed the final record,
/// returned `CensusComplete`, and stranded an R19 directory nothing can ever
/// find again — with the suite green.
///
/// The grid is **{crash before create, after the reaper} × {view on disk}** and
/// its diagonal is the cell that was missing. The reclaimed report is
/// `IntentOnly` in both cells, which is what makes the two indistinguishable to
/// a consumer and is exactly why the *view* has to be handled unconditionally.
///
/// Second field held constant: the same owner, the same invocation and the same
/// resuming incarnation in both cells; only whether the view was mounted moves.
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
        // The premise of each cell, asserted rather than assumed.
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
        // And the whole namespace is empty, so nothing is left under `<R>` for
        // a later census to find — which is what "the ledgers balance" means.
        assert_eq!(
            fs::read_dir(containers_dir(&harness.root))
                .expect("the namespace")
                .count(),
            0,
            "[{label}]"
        );
    }
}

/// A view that could not be pruned keeps its intent, and the **next** census
/// reclaims it.
///
/// `PR6-ACCT-005`, end to end. Discovery is `<R>/containers` plus `docker ps`
/// by label, and the view path is derived only after a candidate exists —
/// `<R>/views` is never enumerated. So an intent removed after a failed view
/// prune is an R19 directory with no discoverable owner, permanently. The cure
/// is that the record outlives what it anchors; the proof is that a second
/// census, with the obstruction gone, finds it and prunes it.
///
/// The intersection: **{view removal fails, succeeds} × {census runs again}**.
/// Cell (fails, no second census) is the residue state; cell (fails, second
/// census) is the recovery; the success cells are the control that says the
/// obstruction is what did it.
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

    // First census: the view removal is made to fail.
    let mut hooks = RecordingHooks::new(harness.trace.clone());
    hooks.fail_at(
        EffectSiteId::Container(ContainerSite::UnmountGitView),
        HookPhase::Before,
    );
    let refused = harness
        .run_with(&mut hooks, &resume(RUN_A, INC_2))
        .expect_err("a census that could not prune a view must not report completion");
    let _ = refusal(&refused);

    // The residue state: the container is gone, the view is not, and the
    // record that names it is **still there**.
    assert!(!harness.holds(&name), "the container was removed");
    assert!(harness.view_exists(&name), "the fixture's obstruction held");
    assert!(
        harness.intent_exists(&name),
        "the R26 record was removed while the R19 view it is the only handle on survived; \
         `<R>/views` is never enumerated, so nothing can ever find that directory again"
    );

    // Second census, obstruction gone: the anchor is what makes this possible.
    let complete = harness
        .census(&resume(RUN_A, INC_3))
        .expect("the census completes");
    assert_eq!(complete.report().reclaimed.len(), 1);
    assert!(!harness.view_exists(&name), "R19 still has residue");
    assert!(!harness.intent_exists(&name));

    // The control: with nothing armed, one census does the whole thing.
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

/// Every reclaimed container settles its owning identity **interrupted, with
/// unknown spend** — whichever half of discovery found it.
///
/// `PR6-RECOV-006`. `T-CONTAINER.authoritative_state` opens "**unknown
/// spend**" and `resume_action` ends "then settle the owning identity
/// **interrupted**". The container tests asserted cleanup and record deletion
/// and nothing about the outcome, so a `Reclaimed` that derived a *success*
/// from `discovered_by == IntentOnly` compiled and passed — and that is the
/// tempting derivation, because an intent-only candidate has no container and
/// so looks like an attempt that never ran. It is the ordinary post-Unix-reaper
/// state: the container was killed *because* it was running, and whatever it
/// spent is unaccounted.
///
/// The grid is **{IntentOnly, LabelOnly, IntentAndLabel} × {the settlement}**,
/// which is every cell of `DiscoveredBy::ALL`, plus the post-reaper cell that
/// has a view — so the answer is asserted to be a constant over the whole
/// discovery axis rather than over the two cells that happened to be seeded.
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
    // One settlement, over the whole grid: the value is not a function of
    // anything the census observed.
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

/// A staged `<name>.intent.tmp` with no published half is accounted for, and
/// what happens to it depends on whose it is.
///
/// `PR6-ACCT-007`. `write_synced` durably creates the staged file before it
/// renames, so a crash between the two leaves one behind **before any container
/// exists** — `create_container` takes an `IntentWritten`, which is minted by
/// reading the *published* record back. `list_intents` skips the staged half
/// (writer-owned residue no reader may adopt), which is right for discovery and
/// left the file with no reclaim path at all: no candidate, no labeled
/// container, so nothing ever called `remove_intent` for it.
///
/// The grid is **{the staged bytes parse, are torn} × {whose name it carries}**:
///
/// | staged bytes | owner | disposition |
/// |---|---|---|
/// | a complete record | anyone | `Adopted` — an ordinary candidate under the ordinary rule |
/// | torn | this run, earlier incarnation | `Removed` — arm (i), dead by construction |
/// | torn | another run | `RetainedForeignOwner` — arm (ii) needs a run directory a torn file has none of |
///
/// The `Adopted` row is what makes the reclaim path exist at all; the third row
/// is the fail-closed one, and it is *reported* rather than silent so INV-22
/// has a class for it.
#[test]
fn a_staged_intent_with_no_published_half_is_accounted_for() {
    use crate::runner::container::census::StagedDisposition;

    let harness = Harness::new("staged");
    let probe = shell_probe();

    // (a) A complete record, staged but never renamed, owned by a dead
    //     incarnation of the run this process is resuming.
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
    // Its view exists: the crash was after the mount and before the rename.
    fs::create_dir_all(view_path(&harness.root, &adopted)).expect("a view");

    // (b) Torn bytes under this run's earlier incarnation.
    let mine = Owner::new(RUN_A, INC_2, REPO_KEY_A).name(&agent_probe());
    fs::write(staged_path(&mine), b"{\"run_id\":\"01KZ").expect("a torn record");

    // (c) Torn bytes under **another** run.
    let foreign = Owner::new(RUN_C, INC_1, REPO_KEY_B).name(&probe);
    fs::write(staged_path(&foreign), b"").expect("a torn record");

    // The premise: `list_intents` sees none of them, which is why nothing
    // reclaimed them.
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

    // The adopted one was reclaimed like any other candidate: both halves of
    // its record and its view are gone.
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

    // The torn ones went where the table says.
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
    // And nothing else in the namespace: exactly the foreign torn file.
    assert_eq!(
        fs::read_dir(containers_dir(&harness.root))
            .expect("the namespace")
            .count(),
        1
    );
}

/// A removal that keeps failing **blocks admission** rather than admitting over
/// residue.
///
/// `PR6-CONV-004`. `racing_removal` retries `RACING_ACCESS_ATTEMPTS` times and
/// then returns `Io`, and that final refusal is the fail-closed half of the
/// Windows delete-pending repair: a delete-pending name disappears within a few
/// attempts and a genuinely protected one still refuses after all of them.
/// Nothing kept a *view or intent* removal failing through the bound, so
/// turning that `Err` into `Ok(false)` — "treat it as gone" — passed: every
/// removal fixture reached `Ok` or `NotFound` first.
///
/// What that mutation costs is not a wrong return value, it is **admission**:
/// the census would return `CensusComplete`, the token that
/// `crash_reconstruction` requires before "slot/reservation initialization,
/// admission, an invocation's first use of an agent's credential volume, and
/// this incarnation's own probes" — over a view it could not remove and whose
/// intent it had just deleted.
///
/// The obstruction is a parent directory with its write bit cleared, which is
/// deterministic and is **not** delete-pending: the two are different states
/// and only one is transient. Skipped under a uid that ignores the bit.
///
/// The intersection: **{removal succeeds, removal never succeeds} × {is there a
/// census token}**. The success cell is the control, without which a test in
/// which nothing could be reclaimed would pass.
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
        // Running as root, or on a filesystem that ignores the mode.
        let _ = fs::set_permissions(&views, fs::Permissions::from_mode(0o755));
        return;
    }

    let error = harness
        .census(&resume(RUN_A, INC_2))
        .expect_err("a census that could not prune an orphan view must not hand out its token");
    assert!(
        matches!(error, TactusError::Io { .. }),
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

    // The control: with the obstruction gone, the same census completes and
    // hands out the token.
    let _ = fs::set_permissions(&views, fs::Permissions::from_mode(0o755));
    let complete = harness
        .census(&resume(RUN_A, INC_3))
        .expect("the census completes once the view can be removed");
    assert_eq!(complete.report().reclaimed.len(), 1);
    assert!(!view.exists() && !harness.intent_exists(&name));
    let _ = fs::remove_dir_all(&harness.root);
}
