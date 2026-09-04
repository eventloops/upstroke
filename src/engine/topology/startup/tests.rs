//! The run-directory census suite — `T-RUNSTART`'s census-side rows.
//!
//! **Every fixture here is built through a funnel.** `src/engine/topology/**`
//! is a `TOPOLOGY_MODULE`: it may carry no module-level allow of a governed
//! lint, and `clippy.toml` denies `std::fs::write`, `create_dir_all`,
//! `remove_*`, `rename` and `std::os::*::fs::symlink` **in tests too**. So a
//! husk here is planted the way a creator plants one — `RunDir.CreatePublicDir`,
//! `StageMarker`, `PublishMarker`, `CreatePrivateDir`, the owner and commit
//! record pairs — and a committed log is written through the `Event` funnel.
//! Reads (`read_dir`, `read_to_string`, `symlink_metadata`, `canonicalize`) are
//! not effects and are used directly.
//!
//! One shape cannot be built under that rule and it is named where it bites:
//! [`locator_through_reparse_point_retained`].
//!
//! **Every "retained" test asserts the private target is byte-identical
//! afterwards.** "Nothing was deleted" is weaker than the packet's claim —
//! `startup_census` (iii) says the husk "is retained and reported", and a census
//! that emptied a private directory without removing it would satisfy the weak
//! reading.

// `effects::production_region` cuts a source at its FIRST `#[cfg(test)]`, and
// this file is reached only through `#[cfg(test)] mod tests;` so it has no
// attribute of its own to cut on. The marker below is redundant to the compiler
// and load-bearing to every reader that still consults the TRUNCATING region —
// `effects::externally_reachable_fns` and the three censuses in
// `src/runner/container/exec.rs` — for which it makes this file's production
// region empty. It does **not** do that for the four whole-tree censuses, which
// read `effects::production_code`: that excises this marker as the configured
// item it is and scans the file in full, and what keeps this file out of their
// domain is the `#[cfg(test)] mod tests;` declaration in
// `src/engine/topology/startup.rs`. Same device, and the same limit, as
// `src/runner/container/census/tests.rs`.
#[cfg(test)]
mod this_file_is_test_only {}

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::{
    CensusInputs, FailedStep, Planned, RunDirCensusReport, RunDirEntry, RunDirOutcome,
    WorktreeLocked, apply, census_run_dirs, scan, startup_census,
};
use crate::error::UpstrokeError;
use crate::events::log::EventLog;
use crate::events::{EventBody, RunStarted};
use crate::rundir::{
    self, COMMIT_RECORD, CommitRecord, CreatingMarker, MARKER, MARKER_STAGED, OWNER_RECORD,
    OWNER_RECORD_STAGED, OwnerField, OwnerRecord, RepoKey, RetainReason, RunDirClass, RunLock,
    UnboundShape, WorktreeLock,
};
use crate::runner::container::runtime::{
    ContainerExecution, ContainerRuntime, CreateSpec, CreatedContainer, DiscoveredContainer,
    ImageInspection, Liveness, LockProbe, RuntimeError, RuntimeOp, StopMode,
};
use crate::runner::container::{GitView, GitViewRequest};
use crate::runner::policy::{host_policy, runner_policy_sha256};
use crate::topology::effects::{
    EffectSiteId, EventSite, HookHarness, HookPhase, Injection, RunDirSite,
};
use crate::topology::events::RunnerPolicy;

// ===========================================================================
// The fixture
// ===========================================================================

const INCARNATION: &str = "01INCARNATION0000000000000";
const PID: u32 = 4242;

/// A scratch repository and a scratch authorized private root `R`.
///
/// `Drop` reclaims the whole tree through `RunDir.RemovePublicHusk`, which
/// removes every entry and then the directory itself. The suite leaks nothing:
/// on this project's build box a fixture leaked per test is inode exhaustion,
/// which `df -h` reports as 72% full while every write fails.
struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    git_dir: PathBuf,
    private_root: PathBuf,
    repo_key: RepoKey,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = rundir::remove_public_husk(&self.root, &mut rundir::NoHooks);
    }
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("upstroke-startup-census-{name}"));
        // A previous run of this test that died before its `Drop`.
        let _ = rundir::remove_public_husk(&root, &mut rundir::NoHooks);
        let hooks = &mut rundir::NoHooks;
        let repo = root.join("repo");
        let git_dir = repo.join(".git");
        let private_root = root.join("private");
        rundir::create_public_dir(&repo, hooks).expect("the scratch repository");
        rundir::create_private_dir(&git_dir, hooks).expect("the worktree git dir");
        // `runs` must exist before the proof canonicalizes it, and every
        // private half is created under it.
        rundir::create_private_dir(&private_root.join("runs"), hooks).expect("the private root");
        let private_root = fs::canonicalize(&private_root).expect("canonical private root");
        let repo_key = RepoKey::v1(&git_dir);
        Self {
            root,
            repo,
            git_dir,
            private_root,
            repo_key,
        }
    }

    fn inputs(&self) -> CensusInputs<'_> {
        CensusInputs {
            repo_root: &self.repo,
            repo_key: &self.repo_key,
            authorized_root: &self.private_root,
            incarnation: INCARNATION,
            runtime: &UnreachableRuntime,
            liveness: &LockProbe,
            view: &NeverView,
        }
    }

    fn public(&self, run_id: &str) -> PathBuf {
        rundir::public_dir(&self.repo, run_id)
    }

    fn private(&self, run_id: &str) -> PathBuf {
        self.private_root.join("runs").join(run_id)
    }

    /// Half (b) alone, with no run of this process's own. The function
    /// [`startup_census`] calls, driven directly so a shape suite does not pay
    /// for a container census per row.
    fn run_census(&self) -> RunDirCensusReport {
        self.run_census_owning(None)
    }

    fn run_census_owning(&self, own_run: Option<&str>) -> RunDirCensusReport {
        census_run_dirs(&mut rundir::NoHooks, &self.inputs(), own_run).expect("the census runs")
    }

    /// The census, with every `RunDir` funnel call recorded on one harness.
    fn run_census_observed(&self) -> (RunDirCensusReport, HookHarness) {
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let mut hooks = rundir::HarnessHooks::new(Arc::clone(&harness));
        let report =
            census_run_dirs(&mut hooks, &self.inputs(), None).expect("the observed census runs");
        let seen = harness.lock().expect("harness").clone();
        (report, seen)
    }

    /// The census, with one `(site, phase)` armed to return `Err` on its `n`-th
    /// call and every `RunDir` funnel call still recorded on the shared
    /// harness.
    fn run_census_armed(
        &self,
        own_run: Option<&str>,
        armed: (RunDirSite, HookPhase, usize),
    ) -> (RunDirCensusReport, HookHarness) {
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let mut hooks = ArmedRunDir::new(Arc::clone(&harness), armed);
        let report = census_run_dirs(&mut hooks, &self.inputs(), own_run)
            .expect("a funnel that refuses is an outcome, not the census's error");
        let seen = harness.lock().expect("harness").clone();
        (report, seen)
    }

    /// The worktree lock this repository's write commands hold.
    ///
    /// `acquire_in` rather than `acquire`: the public convenience API opens a
    /// [`crate::workspace::Workspace`], which shells out to git, and a topology
    /// module may not name `std::process::Command` to build a repository for it
    /// to open.
    fn worktree_lock(&self) -> WorktreeLock {
        WorktreeLock::acquire_in(&self.repo, &self.git_dir).expect("the worktree lock")
    }
}

// ---------------------------------------------------------------------------
// A funnel that refuses
// ---------------------------------------------------------------------------

/// A [`rundir::RunDirHooks`] that records into the shared [`HookHarness`] and
/// returns [`Injection::Error`] at one `(site, phase)`, on one nominated call.
///
/// Module-local because `HookHarness::arm` takes a [`SubEffectPoint`] and
/// `hook()` answers `Proceed` to `Before`/`After` unconditionally, so a
/// `RunDir` site's two phases are not armable through it. The recording still
/// goes to the shared harness, so an armed census contributes to the same
/// coverage evidence an unarmed one does.
///
/// The **nth** call is what makes it usable in a census over many directories:
/// `RunDir.RemovePublicHusk` runs for every reclaim, and a test that wants one
/// of them to refuse needs to say which. Run ids sort ascending, so "the first
/// call" is the first directory in run-id order that reaches the site.
struct ArmedRunDir {
    harness: Arc<Mutex<HookHarness>>,
    site: RunDirSite,
    phase: HookPhase,
    nth: usize,
    seen: usize,
}

impl ArmedRunDir {
    fn new(
        harness: Arc<Mutex<HookHarness>>,
        (site, phase, nth): (RunDirSite, HookPhase, usize),
    ) -> Self {
        Self {
            harness,
            site,
            phase,
            nth,
            seen: 0,
        }
    }
}

impl rundir::RunDirHooks for ArmedRunDir {
    fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .hook(site, phase);
        if site != EffectSiteId::RunDir(self.site) || phase != self.phase {
            return Injection::Proceed;
        }
        self.seen += 1;
        if self.seen == self.nth {
            Injection::Error
        } else {
            Injection::Proceed
        }
    }
}

// ---------------------------------------------------------------------------
// Planting a husk, prefix by prefix
// ---------------------------------------------------------------------------

/// A husk under construction, at whichever publication prefix a test stops at.
struct Husk<'a> {
    fixture: &'a Fixture,
    run_id: String,
    marker: CreatingMarker,
    owner: OwnerRecord,
}

impl<'a> Husk<'a> {
    /// P0: the bare public directory, and the records the later prefixes will
    /// publish.
    fn at_p0(fixture: &'a Fixture, run_id: &str) -> Self {
        rundir::create_public_dir(&fixture.public(run_id), &mut rundir::NoHooks)
            .expect("P0: the public directory");
        let policy = host_policy();
        let marker = CreatingMarker {
            run_id: run_id.to_owned(),
            repo_key: fixture.repo_key.as_str().to_owned(),
            private_dir: fixture.private(run_id).to_string_lossy().into_owned(),
            incarnation: INCARNATION.to_owned(),
            pid: PID,
            runner_policy_sha256: runner_policy_sha256(&policy),
        };
        let owner = OwnerRecord {
            run_id: run_id.to_owned(),
            repo_key: fixture.repo_key.as_str().to_owned(),
            public_dir: fs::canonicalize(fixture.public(run_id))
                .expect("canonical public dir")
                .to_string_lossy()
                .into_owned(),
            incarnation: INCARNATION.to_owned(),
            runner: policy,
        };
        Self {
            fixture,
            run_id: run_id.to_owned(),
            marker,
            owner,
        }
    }

    fn public(&self) -> PathBuf {
        self.fixture.public(&self.run_id)
    }

    fn private(&self) -> PathBuf {
        self.fixture.private(&self.run_id)
    }

    /// P1a: `.creating.tmp`.
    fn stage_marker(self) -> Self {
        rundir::stage_marker(&self.public(), &self.marker, &mut rundir::NoHooks)
            .expect("P1a: the staged marker");
        self
    }

    /// P1b: the rename onto `.creating`.
    fn publish_marker(self) -> Self {
        rundir::publish_marker(&self.public(), &mut rundir::NoHooks).expect("P1b: the marker");
        self
    }

    /// P3a, first window: the private directory, with no owner record at all.
    fn create_private(self) -> Self {
        rundir::create_private_dir(&self.private(), &mut rundir::NoHooks)
            .expect("P3a: the private directory");
        self
    }

    /// P3a, second window: `owner.json.tmp` staged and not yet published.
    fn stage_owner(self) -> Self {
        rundir::stage_owner_record(&self.private(), &self.owner, &mut rundir::NoHooks)
            .expect("P3a: the staged owner record");
        self
    }

    /// P3b: the reciprocal owner record.
    fn publish_owner(self) -> Self {
        let private = self.private();
        rundir::stage_owner_record(&private, &self.owner, &mut rundir::NoHooks)
            .expect("P3b: the staged owner record");
        rundir::publish_owner_record(&private, &mut rundir::NoHooks)
            .expect("P3b: the owner record");
        self
    }

    /// P5b: the private commit record — the one deletion boundary.
    fn publish_commit_record(self) -> Self {
        let private = self.private();
        let record = CommitRecord {
            run_id: self.run_id.clone(),
            repo_key: self.fixture.repo_key.as_str().to_owned(),
            public_dir: self.owner.public_dir.clone(),
            incarnation: INCARNATION.to_owned(),
            run_started_sha256: "sha256:0000000000000000".to_owned(),
        };
        rundir::stage_commit_record(&private, &record, &mut rundir::NoHooks)
            .expect("P5a: the staged commit record");
        rundir::publish_commit_record(&private, &mut rundir::NoHooks)
            .expect("P5b: the commit record");
        self
    }

    /// P5: run-scoped content in the public half, so a marker-less husk is
    /// "carrying run-scoped content" rather than bare.
    fn write_plan(self) -> Self {
        rundir::write_plan(&self.public(), b"{\"tasks\":[]}\n", &mut rundir::NoHooks)
            .expect("P5: the frozen plan");
        self
    }

    /// P6: `events.jsonl` with a valid committed first line.
    ///
    /// Written through `Event.LegacyOpenLog`/`Event.LegacyAppend` because
    /// `classify_run_dir` is deliberately schema-agnostic — "a schema-4 log must
    /// classify through the same call as a schema-1 one" — so a legacy committed
    /// line is the cheapest *and* the more demanding fixture: it proves the
    /// census reads the header rather than a schema-4 event type.
    fn commit_log(self) -> Self {
        append_line(
            &self.public().join(rundir::EVENT_LOG),
            run_started(&self.run_id),
        );
        self
    }

    /// A public log that exists and whose first line is **not** a valid
    /// `run_started` — the hostile downgrade of `T-RUNSTART`'s boundary, "a
    /// committed run whose public first line was corrupted, truncated, or
    /// removed".
    fn corrupt_log(self) -> Self {
        append_line(
            &self.public().join(rundir::EVENT_LOG),
            EventBody::DeferWaitElapsed {
                data: crate::events::DeferWaitElapsed {
                    waited: std::time::Duration::from_millis(1),
                    round: 1,
                },
            },
        );
        self
    }
}

/// Append one legacy event to `path`, creating it. The `Event` funnel is the
/// only writer of a log in this tree, and it takes a path.
fn append_line(path: &Path, body: EventBody) {
    let mut warnings = Vec::new();
    let mut log =
        EventLog::open(EventSite::LegacyOpenLog, path, &mut warnings).expect("the log opens");
    log.append(EventSite::LegacyAppend, body).expect("the line");
}

fn run_started(run_id: &str) -> EventBody {
    EventBody::RunStarted {
        data: Box::new(RunStarted {
            schema: 3,
            upstroke_version: "0.1.0".to_owned(),
            run_id: run_id.to_owned(),
            branch: "main".to_owned(),
            base_sha: "0".repeat(40),
            plan_path: "PLAN.md".to_owned(),
            config_path: None,
            plan_hash: "sha256:00".to_owned(),
            normalized_plan_digest: None,
            private_dir: String::new(),
            gates: Vec::new(),
            gates_from_config: false,
            interaction_mode: "headless".to_owned(),
            chains: Vec::new(),
            effort_policy: None,
            gate_cmds: None,
            reviews: None,
        }),
    }
}

fn another_policy() -> RunnerPolicy {
    let mut policy = host_policy();
    policy.credential_volumes = Some(BTreeMap::from([(
        "claude-code".to_owned(),
        "upstroke-creds".to_owned(),
    )]));
    policy
}

// ---------------------------------------------------------------------------
// Reading the disk back
// ---------------------------------------------------------------------------

/// Every file under `root`, by relative path, with its bytes.
///
/// The comparison a "retained" test makes. Byte-identity, not existence: a
/// census that truncated `owner.json` to nothing would leave the directory
/// present and every weaker assertion green.
fn tree_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = fs::read(&path) {
                out.insert(
                    path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                    bytes,
                );
            }
        }
    }
    out
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// The single entry a one-directory census produced.
fn only(report: &RunDirCensusReport) -> &RunDirEntry {
    assert_eq!(
        report.entries().len(),
        1,
        "this fixture plants one run directory: {:#?}",
        report.entries()
    );
    &report.entries()[0]
}

/// Assert one husk was retained for `reason`, reported, and left byte-identical.
///
/// Every "retained" row of `T-RUNSTART` makes the same four claims, so they are
/// made in one place: a suite that spelled them out per test would drift, and
/// the fourth — byte-identity — is the one an implementation is most likely to
/// satisfy accidentally and then stop satisfying.
fn assert_retained(fixture: &Fixture, run_id: &str, expected: &RetainReason) {
    let private = fixture.private(run_id);
    let before = tree_bytes(&private);
    assert!(
        !before.is_empty(),
        "the fixture must plant a private half with content, or byte-identity proves nothing"
    );
    let report = fixture.run_census();
    let entry = only(&report);

    assert_eq!(
        entry.retain_reason(),
        Some(expected),
        "retained for the wrong reason: {}",
        entry.describe()
    );
    assert_eq!(entry.run_id, run_id);
    assert_eq!(
        entry.public,
        fixture.public(run_id),
        "the report must locate the husk"
    );
    assert_eq!(
        report.retained().len(),
        1,
        "the census must report it as retained"
    );
    assert!(
        !entry.outcome.reclaimed_anything(),
        "nothing may be reclaimed for {expected:?}"
    );
    assert!(
        exists(&fixture.public(run_id)),
        "the public half is retained too: only the deferred `runs prune` removes a retained husk"
    );
    assert_eq!(
        tree_bytes(&private),
        before,
        "the private target must be byte-identical afterwards"
    );
}

// ===========================================================================
// (i) Nothing private is bound
// ===========================================================================

/// `bare_directory_reclaimed` — P0.
///
/// "A bare directory … is reclaimed (no private half exists by ordering)."
#[test]
fn bare_directory_reclaimed() {
    let fixture = Fixture::new("bare");
    let husk = Husk::at_p0(&fixture, "01BARE00000000000000000000");

    let report = fixture.run_census();

    assert_eq!(
        only(&report).outcome,
        RunDirOutcome::ReclaimedPublicOnly(UnboundShape::Bare)
    );
    assert!(!exists(&husk.public()), "the public directory is gone");
    assert!(
        !exists(&fixture.private("01BARE00000000000000000000")),
        "no private half ever existed"
    );
}

/// `staged_marker_only_reclaimed` — P1a.
///
/// "…or one holding only a staged `.creating.tmp` (no marker, no other
/// content)". The marker was never published, so by the publication order no
/// private half can exist.
#[test]
fn staged_marker_only_reclaimed() {
    let fixture = Fixture::new("staged");
    let husk = Husk::at_p0(&fixture, "01STAGED000000000000000000").stage_marker();
    assert!(exists(&husk.public().join(MARKER_STAGED)));
    assert!(!exists(&husk.public().join(MARKER)));

    let report = fixture.run_census();

    assert_eq!(
        only(&report).outcome,
        RunDirOutcome::ReclaimedPublicOnly(UnboundShape::StagedMarkerOnly)
    );
    assert!(!exists(&husk.public()));
}

/// `marker_with_absent_private_target_reclaims_public_only` — P1b/P2, **and the
/// convergence claim**.
///
/// "If the marker's private target does not exist the public husk alone is
/// reclaimed." This is also exactly the residue a kill *mid-census* leaves:
/// `apply` removes the private half first and the public directory with the
/// marker last, so a process that dies between the two leaves a published marker
/// whose target is gone — and the next census finishes the job. "A kill
/// mid-census leaves a husk the next census completes" is this row.
#[test]
fn marker_with_absent_private_target_reclaims_public_only() {
    let fixture = Fixture::new("target-absent");
    let husk = Husk::at_p0(&fixture, "01ABSENT000000000000000000")
        .stage_marker()
        .publish_marker();
    assert!(exists(&husk.public().join(MARKER)));
    assert!(!exists(&husk.private()), "the target was never created");

    let report = fixture.run_census();
    let entry = only(&report);

    assert_eq!(
        entry.outcome,
        RunDirOutcome::ReclaimedPublicOnly(UnboundShape::TargetAbsent)
    );
    assert_eq!(
        entry.locator.as_deref(),
        Some(husk.private().as_path()),
        "the report names the locator the marker recorded"
    );
    assert!(!exists(&husk.public()));
}

// ===========================================================================
// (ii) The ownership proof holds
// ===========================================================================

/// `bound_husk_without_commit_record_reclaimed_private_then_public` — P3b–P5.
///
/// "The census reclaims the private half through the proof-token funnel, then
/// the public directory with the marker last." Both halves of that sentence are
/// asserted: that both are gone, and that the two funnels ran **in that order**.
#[test]
fn bound_husk_without_commit_record_reclaimed_private_then_public() {
    let fixture = Fixture::new("bound");
    let husk = Husk::at_p0(&fixture, "01BOUND0000000000000000000")
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner()
        .write_plan();
    assert!(exists(&husk.private().join(OWNER_RECORD)));
    assert!(!exists(&husk.private().join(COMMIT_RECORD)));

    let (report, seen) = fixture.run_census_observed();
    let entry = only(&report);

    assert_eq!(entry.outcome, RunDirOutcome::ReclaimedBothHalves);
    assert!(entry.outcome.deleted_a_private_half());
    assert!(!exists(&husk.private()), "the private half is gone");
    assert!(!exists(&husk.public()), "the public half is gone");

    let order: Vec<EffectSiteId> = seen
        .coverage()
        .iter()
        .filter(|seen| seen.phase == HookPhase::Before)
        .map(|seen| seen.site)
        .collect();
    assert_eq!(
        order,
        vec![
            EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
            EffectSiteId::RunDir(RunDirSite::RemovePublicHusk),
        ],
        "the private half is deleted first, through the proof-token funnel, and the public \
         directory second — reversed, a kill between them leaves a private half no marker names \
         and no census can ever prove"
    );
}

// ===========================================================================
// (iii) Retained, and nothing private deleted
// ===========================================================================

/// `private_dir_without_owner_record_retained_and_reported` — P3a, **both
/// windows**.
///
/// "The private directory exists without an owner record — unprovable — so both
/// halves are retained and reported." The tree labels `stage_owner_record` P3a
/// and `publish_owner_record` P3b, so ST-19's P3a prefix spans two shapes: before
/// staging (the directory is empty) and after it (the directory holds
/// `owner.json.tmp`). The second is **not** content-free, and the creator
/// removes neither half in either, so both reach this census intact.
#[test]
fn private_dir_without_owner_record_retained_and_reported() {
    let fixture = Fixture::new("p3a");

    // Second window first, because it is the one that carries content and so
    // is the one `assert_retained`'s byte-identity claim can measure.
    let staged_id = "01P3ASTAGED000000000000000";
    let staged = Husk::at_p0(&fixture, staged_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .stage_owner();
    assert!(exists(&staged.private().join(OWNER_RECORD_STAGED)));
    assert!(!exists(&staged.private().join(OWNER_RECORD)));

    assert_retained(&fixture, staged_id, &RetainReason::OwnerRecordMissing);
    assert!(exists(&staged.private()), "the private half is retained");
    assert!(
        exists(&staged.private().join(OWNER_RECORD_STAGED)),
        "the staged record is what makes this window not content-free"
    );

    // First window: the empty private directory. Byte-identity over an empty
    // tree is vacuous, so this half asserts the directory itself survives and
    // that it is still empty.
    let empty_id = "01P3AEMPTY0000000000000000";
    let empty = Husk::at_p0(&fixture, empty_id)
        .stage_marker()
        .publish_marker()
        .create_private();
    let report = fixture.run_census();
    let entry = report.of(empty_id).expect("the empty P3a husk is reported");
    assert_eq!(
        entry.retain_reason(),
        Some(&RetainReason::OwnerRecordMissing),
        "{}",
        entry.describe()
    );
    assert!(exists(&empty.private()), "content-free is not deletable");
    assert!(tree_bytes(&empty.private()).is_empty());
    assert!(exists(&empty.public()));
}

/// `commit_record_present_without_committed_log_retained_and_reported` — P5b.
///
/// "`committed.json` exists, so both halves are retained and reported as
/// possibly committed with nothing deleted." The private half crossed the one
/// deletion boundary; the public log never got its first line.
#[test]
fn commit_record_present_without_committed_log_retained_and_reported() {
    let fixture = Fixture::new("p5b");
    let run_id = "01P5B000000000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner()
        .publish_commit_record();
    assert!(
        !exists(&husk.public().join(rundir::EVENT_LOG)),
        "P5b: the log has no committed first line because it has no line"
    );

    assert_retained(&fixture, run_id, &RetainReason::PossiblyCommitted);

    let report = fixture.run_census();
    assert_eq!(
        report.possibly_committed().len(),
        1,
        "the third of the three status sentences"
    );
    assert!(only(&report).is_possibly_committed());
}

/// `committed_run_downgraded_by_public_log_corruption_retains_private_half`.
///
/// The hostile downgrade: a run that reached P6, whose public first line was
/// then damaged, and whose marker "was re-published with agreeing fields by a
/// public-half writer". Everything an attacker controls — the marker, the log —
/// agrees; the private half's `committed.json` is what refuses, and it is the
/// only conjunct they cannot reach.
///
/// Distinct from the P5b row above: there the log does not exist, here it does
/// and its first line is a well-formed event that is not a `run_started`. Both
/// classify `Husk`, and neither may lose a private half.
#[test]
fn committed_run_downgraded_by_public_log_corruption_retains_private_half() {
    let fixture = Fixture::new("downgrade");
    let run_id = "01DOWNGRADE000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .create_private()
        .publish_owner()
        .publish_commit_record()
        .corrupt_log()
        // The re-published marker, last, with every field agreeing.
        .stage_marker()
        .publish_marker();
    assert!(exists(&husk.public().join(rundir::EVENT_LOG)));
    assert_eq!(
        rundir::classify_run_dir(&husk.public()),
        RunDirClass::Husk,
        "a log whose first line is not a valid run_started is not a committed run"
    );

    assert_retained(&fixture, run_id, &RetainReason::PossiblyCommitted);
    assert!(
        exists(&husk.private().join(COMMIT_RECORD)),
        "the commit record is what refused, and it is still there"
    );
}

/// `forged_marker_naming_foreign_run_never_deletes_private_half`.
///
/// A marker whose `run_id` names another run: the classic attack, a marker
/// dropped into a husk pointing at a live run's private half. Conjunct 2
/// refuses, and **nothing private is deleted on a basename check** — the husk
/// and the foreign private half both survive.
#[test]
fn forged_marker_naming_foreign_run_never_deletes_private_half() {
    let fixture = Fixture::new("forged");
    let run_id = "01FORGED000000000000000000";
    let victim = "01VICTIM000000000000000000";
    let control = "01CONTROL00000000000000000";

    // The victim: a live run, its own `run.lock` held, its private half
    // complete. Live because that is the attack — a forged marker aimed at a
    // half somebody is using — and because a victim whose own husk were
    // reclaimable would be deleted by its own row rather than by the forgery.
    let victim_husk = Husk::at_p0(&fixture, victim)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    let victim_private = victim_husk.private();
    let victim_before = tree_bytes(&victim_private);
    let victim_lock = RunLock::acquire(&victim_husk.public()).expect("the victim is live");

    // The control: an ordinary bare husk. Without it every assertion below
    // would pass against a census that refused everything.
    Husk::at_p0(&fixture, control);

    // The forgery: a husk whose marker names the victim and points at the
    // victim's private half.
    let mut forged = Husk::at_p0(&fixture, run_id);
    forged.marker.run_id = victim.to_owned();
    forged.marker.private_dir = victim_private.to_string_lossy().into_owned();
    let forged = forged.stage_marker().publish_marker().create_private();
    rundir::write_plan(&forged.private(), b"residue\n", &mut rundir::NoHooks).expect("residue");
    let forged_before = tree_bytes(&forged.private());

    let report = fixture.run_census();

    let entry = report.of(run_id).expect("the forged husk is reported");
    assert_eq!(
        entry.retain_reason(),
        Some(&RetainReason::MarkerRunIdMismatch {
            recorded: victim.to_owned(),
            directory: run_id.to_owned(),
        }),
        "{}",
        entry.describe()
    );
    assert_eq!(
        entry.locator.as_deref(),
        Some(victim_private.as_path()),
        "the report names the locator the forgery recorded, which is what makes it readable \
         as an attack"
    );
    assert_eq!(
        tree_bytes(&victim_private),
        victim_before,
        "the victim's private half must be byte-identical: a forged marker never authorises a \
         deletion of somebody else's half"
    );
    assert_eq!(
        tree_bytes(&forged.private()),
        forged_before,
        "and neither does it authorise a deletion of its own"
    );
    assert!(exists(&forged.public()));
    assert_eq!(
        report.of(victim).expect("the victim is censused").outcome,
        RunDirOutcome::Skipped,
        "a live run is skipped, whatever a husk beside it claims about its private half"
    );
    assert_eq!(
        report.of(control).expect("the control is censused").outcome,
        RunDirOutcome::ReclaimedPublicOnly(UnboundShape::Bare),
        "the control: this census did reclaim, so the retentions above are decisions"
    );
    drop(victim_lock);
}

/// `copied_husk_from_other_repository_retained_repo_key_mismatch`.
///
/// Conjunct 3: "a directory copied from another repository". The husk is
/// otherwise perfect — the locator resolves, the owner record agrees with the
/// marker on every field — and the only disagreement is with *this* repository.
#[test]
fn copied_husk_from_other_repository_retained_repo_key_mismatch() {
    let fixture = Fixture::new("copied");
    let run_id = "01COPIED000000000000000000";
    let foreign = RepoKey::v1(Path::new("/somewhere/else/.git"));

    let mut husk = Husk::at_p0(&fixture, run_id);
    husk.marker.repo_key = foreign.as_str().to_owned();
    husk.owner.repo_key = foreign.as_str().to_owned();
    let _husk = husk
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();

    assert_retained(
        &fixture,
        run_id,
        &RetainReason::MarkerRepoKeyMismatch {
            recorded: foreign.as_str().to_owned(),
            expected: fixture.repo_key.as_str().to_owned(),
        },
    );
}

/// `runner_digest_mismatch_retained`.
///
/// Conjunct 11: `sha256(owner.runner)` must equal the marker's
/// `runner_policy_sha256`. The two records are written at P1 and P3b by the same
/// creator, so a disagreement means one of them was rewritten.
#[test]
fn runner_digest_mismatch_retained() {
    let fixture = Fixture::new("digest");
    let run_id = "01DIGEST000000000000000000";

    let mut husk = Husk::at_p0(&fixture, run_id);
    let other = another_policy();
    let recorded = runner_policy_sha256(&other);
    husk.owner.runner = other;
    let _husk = husk
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();

    assert_retained(
        &fixture,
        run_id,
        &RetainReason::OwnerRecordDisagrees {
            field: OwnerField::RunnerDigest,
            recorded,
            expected: runner_policy_sha256(&host_policy()),
        },
    );
}

/// `locator_outside_authorized_private_root_retained`.
///
/// Conjunct 5 is an **equality** with `<R>/runs/<basename>`, not a containment
/// test. The locator here resolves to a real directory carrying a real owner
/// record — outside `R` it is still somebody's private half, and the census
/// deletes nothing it cannot place inside the root it was authorized for.
#[test]
fn locator_outside_authorized_private_root_retained() {
    let fixture = Fixture::new("outside");
    let run_id = "01OUTSIDE00000000000000000";
    let elsewhere = fixture.root.join("elsewhere").join("runs").join(run_id);
    rundir::create_private_dir(&elsewhere, &mut rundir::NoHooks).expect("a foreign private half");
    let elsewhere = fs::canonicalize(&elsewhere).expect("canonical");

    let mut husk = Husk::at_p0(&fixture, run_id);
    husk.marker.private_dir = elsewhere.to_string_lossy().into_owned();
    let husk = husk.stage_marker().publish_marker();
    rundir::stage_owner_record(&elsewhere, &husk.owner, &mut rundir::NoHooks).expect("staged");
    rundir::publish_owner_record(&elsewhere, &mut rundir::NoHooks).expect("the owner record");

    let before = tree_bytes(&elsewhere);
    assert!(!before.is_empty());

    let report = fixture.run_census();
    let entry = only(&report);

    assert_eq!(
        entry.retain_reason(),
        Some(&RetainReason::LocatorOutsideAuthorizedRoot {
            locator: elsewhere.clone(),
            expected: fs::canonicalize(fixture.private_root.join("runs"))
                .expect("canonical runs")
                .join(run_id),
        }),
        "{}",
        entry.describe()
    );
    assert_eq!(
        entry.locator.as_deref(),
        Some(elsewhere.as_path()),
        "the locator is reported so an operator can see where it pointed"
    );
    assert_eq!(
        tree_bytes(&elsewhere),
        before,
        "the foreign private half must be byte-identical"
    );
    assert!(exists(&husk.public()));
}

/// `locator_through_reparse_point_retained`.
///
/// **This lane cannot plant the link.** `clippy.toml` denies
/// `std::os::unix::fs::symlink`, `std::os::windows::fs::symlink_dir` and
/// `std::fs::soft_link`, and `src/engine/topology/**` may carry no module-level
/// allow of a governed lint, so a real reparse point on a locator chain is not
/// constructible in this module — in a test or anywhere else. The detection
/// itself is measured in `src/rundir.rs`'s `proof_cases`, whose "locator through
/// a reparse point" case plants a POSIX symlink or a Windows **junction**
/// through the module allow a funnel module carries.
///
/// So what is measured here is the half this lane owns: given that answer, the
/// census retains, reports the reason, and **executes no `RunDir` funnel at
/// all** while a real private half sits on disk. The last is the strong form of
/// "nothing private is deleted on … reparse-point checks alone": not "the
/// deletion did not happen" but "there is no funnel call on this path".
#[test]
fn locator_through_reparse_point_retained() {
    let fixture = Fixture::new("reparse");
    let run_id = "01REPARSE00000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    let private = husk.private();
    let before = tree_bytes(&private);
    assert!(!before.is_empty());

    let component = fixture.private_root.join("runs").join("linked");
    let reason = RetainReason::LocatorThroughReparsePoint {
        component: component.clone(),
    };

    let harness = Arc::new(Mutex::new(HookHarness::new()));
    let mut hooks = rundir::HarnessHooks::new(Arc::clone(&harness));
    // `apply` returns a `RunDirOutcome` and not a `Result`: "retaining performs
    // no effect" is now a property of the signature, so there is no `Ok` left to
    // assert.
    let outcome = apply(&mut hooks, &husk.public(), Planned::Retain(reason.clone()));

    assert_eq!(outcome, RunDirOutcome::Retained(reason));
    assert_eq!(
        harness.lock().expect("harness").executions(),
        0,
        "the retain arm reaches no funnel, so nothing can be deleted by it"
    );
    assert_eq!(
        tree_bytes(&private),
        before,
        "the private target must be byte-identical afterwards"
    );
    assert!(exists(&husk.public()));
}

/// **`scan` turns the proof's refusal into a plan that deletes nothing, and
/// its reclaiming answer into one that deletes.** Both halves, one fixture,
/// no privileged primitive.
///
/// This is the composition pass 2 found untested. The rundir suite asserts
/// what `prove_private_half_ownership` answers, and
/// [`every_retain_reason_kind_deletes_nothing`] asserts what `apply` does with
/// a `Planned::Retain` handed to it directly — but nothing asserted that `scan`
/// carries the one into the other. A mutation that special-cased
/// `ListingUnreadable` into `ReclaimPublicOnly(Bare)` inside `scan` left every
/// added test green while the real sequence deleted a committed run's log.
///
/// The unreadable-listing answer is reached here without touching a permission
/// bit: a run id whose public directory **does not exist** classifies `Husk`
/// (the log cannot be opened), `is_running` answers false (the lock file is
/// absent, which is `NotFound` and therefore an answer), the marker read is
/// `NotFound`, and `read_dir` is `NotFound` — which this pull request's rule
/// treats as a listing that did not happen rather than an empty directory. So
/// the proof answers `Retained(ListingUnreadable)` and `scan` must plan a
/// retention.
///
/// The second half is the control, and it is what stops the first half being
/// satisfied by a `scan` that retains everything: a **bare** public directory
/// is the shape the census is *supposed* to reclaim, and the same call must
/// answer `ReclaimPublicOnly(Bare)` for it. One mutation dies on each.
#[test]
fn scan_plans_a_retention_for_a_listing_that_did_not_answer_and_a_reclaim_for_a_bare_husk() {
    let fixture = Fixture::new("scan-composition");

    // (1) The refusal. No directory at all, so nothing to plant and nothing to
    // chmod: every probe on this path answers `NotFound`.
    let absent = "01ABSENTPUBLICDIR000000000";
    let scanned = scan(absent, &fixture.inputs(), None);
    assert_eq!(
        scanned.class,
        RunDirClass::Husk,
        "a directory that is not there is not a committed run"
    );
    match &scanned.plan {
        Planned::Retain(reason) => assert_eq!(
            reason.kind(),
            "listing-unreadable",
            "the proof's refusal must arrive as a retention: {reason}"
        ),
        other => panic!(
            "`scan` turned a listing that did not answer into {other:?}, which is the plan \
             that deletes"
        ),
    }
    let outcome = apply(&mut rundir::NoHooks, &scanned.public, scanned.plan);
    assert!(
        !outcome.reclaimed_anything(),
        "and the plan it produced reclaims nothing: {outcome:?}"
    );

    // (2) The control. A bare public directory is what the census reclaims, so
    // a `scan` that retained everything would fail here.
    let bare = "01BAREPUBLICDIR0000000000";
    let husk = Husk::at_p0(&fixture, bare);
    assert!(exists(&husk.public()), "P0 made the bare directory");
    let scanned = scan(bare, &fixture.inputs(), None);
    match &scanned.plan {
        Planned::ReclaimPublicOnly(shape) => assert_eq!(
            *shape,
            UnboundShape::Bare,
            "a bare husk is still reclaimed, and by its shape"
        ),
        other => panic!("`scan` refused to reclaim a bare husk: {other:?}"),
    }
    let outcome = apply(&mut rundir::NoHooks, &scanned.public, scanned.plan);
    assert_eq!(outcome, RunDirOutcome::ReclaimedPublicOnly(UnboundShape::Bare));
    assert!(!exists(&husk.public()), "and the bare husk is gone");
}

/// **The census-level witness for a listing that cannot be read is not here,
/// and this is where that bites.**
///
/// The sequence exists and was measured: with a public directory carrying
/// `--wx`, the classifier's `open` is refused and answers `Husk` for a
/// committed run, `is_running` still opens `run.lock` by name and answers
/// free, the proof finds no marker, and `unbound_shape` is handed a listing it
/// cannot read. Driving the real `scan` and `apply` across that failure, with
/// `unbound_shape`'s listing arm reverted to `unwrap_or_default()`, produced
/// `the census planned ReclaimPublicOnly(Bare) on a listing it could not read
/// and the outcome was ReclaimedPublicOnly(Bare); the committed log is GONE`.
///
/// It cannot be committed here. The fixture needs `fs::set_permissions`, which
/// `clippy.toml` denies, and this module may carry no module-level allow of a
/// governed lint — the rule this file's own header states, and the same reason
/// [`locator_through_reparse_point_retained`] cannot plant its link. An
/// allowlist row for this file would be a change to the effect governance, not
/// a repair, so the shape is named here instead of quietly missing. What
/// guards the fold in the tree is `rundir::tests::
/// a_committed_run_the_census_could_not_read_is_not_reclaimed` at the fold
/// itself, and [`every_retain_reason_kind_deletes_nothing`] for the census's
/// reason-agnostic retain arm.
///
/// Every retention reason, driven through the census's retain arm.
///
/// `RetainReason::KINDS` is the closed set, and this asserts the arm is
/// **reason-agnostic**: a future variant that fell through to a reclaim would
/// fail here rather than wait for someone to write its own row. It is also the
/// only place the reparse-point kind meets the census on a machine where the
/// link cannot be planted.
#[test]
fn every_retain_reason_kind_deletes_nothing() {
    let fixture = Fixture::new("all-reasons");
    let run_id = "01ALLREASONS00000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner()
        .publish_commit_record();
    let before = tree_bytes(&husk.private());
    let public_before = tree_bytes(&husk.public());

    let mut kinds = Vec::new();
    for reason in every_retain_reason() {
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let mut hooks = rundir::HarnessHooks::new(Arc::clone(&harness));
        let outcome = apply(&mut hooks, &husk.public(), Planned::Retain(reason.clone()));
        assert_eq!(outcome, RunDirOutcome::Retained(reason.clone()));
        assert!(!outcome.reclaimed_anything());
        assert!(!outcome.deleted_a_private_half());
        assert_eq!(
            harness.lock().expect("harness").executions(),
            0,
            "{} reached a funnel",
            reason.kind()
        );
        kinds.push(reason.kind());
    }

    assert_eq!(
        kinds,
        RetainReason::KINDS,
        "the table must cover the closed set, in its order"
    );
    assert_eq!(tree_bytes(&husk.private()), before);
    assert_eq!(tree_bytes(&husk.public()), public_before);
}

/// One value per [`RetainReason::KINDS`] entry, in that order.
fn every_retain_reason() -> Vec<RetainReason> {
    vec![
        RetainReason::MarkerUnparseable,
        RetainReason::MarkerRunIdMismatch {
            recorded: "a".to_owned(),
            directory: "b".to_owned(),
        },
        RetainReason::MarkerRepoKeyMismatch {
            recorded: "a".to_owned(),
            expected: "b".to_owned(),
        },
        RetainReason::TargetUndecidable {
            detail: "a: b".to_owned(),
        },
        RetainReason::LocatorOutsideAuthorizedRoot {
            locator: PathBuf::from("a"),
            expected: PathBuf::from("b"),
        },
        RetainReason::LocatorThroughReparsePoint {
            component: PathBuf::from("a"),
        },
        RetainReason::OwnerRecordMissing,
        RetainReason::OwnerRecordUnparseable,
        RetainReason::OwnerRecordDisagrees {
            field: OwnerField::RunId,
            recorded: "a".to_owned(),
            expected: "b".to_owned(),
        },
        RetainReason::MarkerlessWithContent,
        RetainReason::ListingUnreadable {
            detail: "a: b".to_owned(),
        },
        RetainReason::PossiblyCommitted,
    ]
}

/// `owner_record_disagreement_retained`.
///
/// Conjunct 8: the owner record must record this husk's **canonical** public
/// path. A record naming another directory means the two halves do not agree
/// about which husk owns which private half, and no census picks a winner.
#[test]
fn owner_record_disagreement_retained() {
    let fixture = Fixture::new("disagree");
    let run_id = "01DISAGREE0000000000000000";

    let mut husk = Husk::at_p0(&fixture, run_id);
    let wrong = fixture.public("01SOMEOTHERRUN000000000000");
    husk.owner.public_dir = wrong.to_string_lossy().into_owned();
    let _husk = husk
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();

    assert_retained(
        &fixture,
        run_id,
        &RetainReason::OwnerRecordDisagrees {
            field: OwnerField::PublicDir,
            recorded: wrong.to_string_lossy().into_owned(),
            expected: fs::canonicalize(fixture.public(run_id))
                .expect("canonical")
                .to_string_lossy()
                .into_owned(),
        },
    );
}

/// `markerless_husk_with_content_retained`.
///
/// "A marker-less husk carrying run-scoped content" is retained, not reclaimed:
/// content with no marker to bind it is content nothing can prove is unowned.
/// The contrast with `bare_directory_reclaimed` is the whole row — same absence
/// of a marker, opposite answer, and the difference is one file.
#[test]
fn markerless_husk_with_content_retained() {
    let fixture = Fixture::new("markerless");
    let run_id = "01MARKERLESS00000000000000";
    let husk = Husk::at_p0(&fixture, run_id).write_plan();
    assert!(!exists(&husk.public().join(MARKER)));
    // A private half planted beside it, so byte-identity has something to
    // measure and so the row is not weaker than the ones around it.
    let husk = husk.create_private().publish_owner();

    assert_retained(&fixture, run_id, &RetainReason::MarkerlessWithContent);
    assert!(
        exists(&husk.public().join(rundir::PLAN)),
        "the content that made it markerless-with-content is still there"
    );
}

/// `malformed_marker_retained_locator_reported`.
///
/// The marker is present and readable and is **not** a marker. Conjunct 1
/// refuses, and the locator it would have named is unknowable: `husk_report`
/// and this census parse the same marker with the same strict parse — there is
/// one classifier — so there is no lenient second read to guess with. What the
/// report carries instead is the husk's own location, which is what an operator
/// needs to go and look.
///
/// Reporting `None` rather than a guess is asserted, not merely observed: a
/// census that invented a locator from the directory name would name
/// `<R>/runs/<basename>` — the very path the proof refused to bind — and an
/// operator reading the report would believe the two halves were paired.
#[test]
fn malformed_marker_retained_locator_reported() {
    let fixture = Fixture::new("malformed");
    let run_id = "01MALFORMED000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .create_private()
        .publish_owner();
    // A `.creating` holding well-formed JSON that is not a `CreatingMarker`.
    // `CreatingMarker` is `deny_unknown_fields`, so an event line fails to
    // parse as one. The `Event` funnel is the only writer in this tree that
    // takes a caller-supplied path, and a topology module may name no raw `fs`
    // primitive even in a fixture.
    append_line(&husk.public().join(MARKER), run_started(run_id));
    assert!(exists(&husk.public().join(MARKER)));

    assert_retained(&fixture, run_id, &RetainReason::MarkerUnparseable);

    let report = fixture.run_census();
    let entry = only(&report);
    assert_eq!(
        entry.locator, None,
        "an unparseable marker names no target this census is entitled to believe"
    );
    assert_eq!(
        entry.public,
        fixture.public(run_id),
        "so the husk's own location is what the report carries"
    );
    assert!(
        entry
            .describe()
            .contains(&format!("{}", entry.public.display())),
        "and the sentence an operator reads names it: {}",
        entry.describe()
    );
}

// ===========================================================================
// The lock, and the committed directories
// ===========================================================================

/// `locked_husk_skipped`.
///
/// "A Husk with a held `run.lock` is skipped (defense in depth; under the
/// worktree lock no live creator can exist in this worktree)." The husk here is
/// otherwise fully provable — without the lock it would be reclaimed, both
/// halves — so the lock is the only thing the assertion can be measuring.
#[test]
fn locked_husk_skipped() {
    let fixture = Fixture::new("locked");
    let run_id = "01LOCKED000000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    let before = tree_bytes(&husk.private());

    let lock = RunLock::acquire(&husk.public()).expect("the run lock");
    assert!(rundir::is_running(&husk.public()));

    let report = fixture.run_census();
    let entry = only(&report);

    assert_eq!(entry.outcome, RunDirOutcome::Skipped);
    assert_eq!(report.skipped().len(), 1);
    assert!(exists(&husk.public()), "the public half is untouched");
    assert_eq!(
        tree_bytes(&husk.private()),
        before,
        "and so is the private half"
    );

    drop(lock);

    // The control: the same husk, the same census, the lock free. Without this
    // half the test would pass against a census that skipped everything.
    let report = fixture.run_census();
    assert_eq!(only(&report).outcome, RunDirOutcome::ReclaimedBothHalves);
    assert!(!exists(&husk.private()));
    assert!(!exists(&husk.public()));
}

/// `committed_run_with_stale_marker_repaired_by_census_when_lock_free`.
///
/// "A Committed directory still carrying `.creating` or `.creating.tmp` (kill
/// after `run_started` before marker removal …) has the stale marker removed
/// when its `run.lock` is free". The run itself is not a husk and nothing about
/// it is reclaimed: the repair is the marker and only the marker.
#[test]
fn committed_run_with_stale_marker_repaired_by_census_when_lock_free() {
    let fixture = Fixture::new("stale-marker");
    let run_id = "01STALE0000000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner()
        .publish_commit_record()
        .commit_log();
    assert_eq!(
        rundir::classify_run_dir(&husk.public()),
        RunDirClass::Committed
    );
    let private_before = tree_bytes(&husk.private());
    let log_before = fs::read(husk.public().join(rundir::EVENT_LOG)).expect("the log");

    let report = fixture.run_census();
    let entry = only(&report);

    assert_eq!(entry.outcome, RunDirOutcome::RepairedStaleMarker);
    assert_eq!(entry.class, RunDirClass::Committed);
    assert_eq!(report.repaired().len(), 1);
    assert!(
        !exists(&husk.public().join(MARKER)),
        "the stale marker is gone"
    );
    assert!(!exists(&husk.public().join(MARKER_STAGED)));
    assert!(exists(&husk.public()), "the run is not a husk");
    assert_eq!(
        fs::read(husk.public().join(rundir::EVENT_LOG)).expect("the log"),
        log_before,
        "the log is untouched"
    );
    assert_eq!(
        tree_bytes(&husk.private()),
        private_before,
        "and so is the private half, commit record included"
    );

    // Idempotent: a second census finds nothing to repair.
    let report = fixture.run_census();
    assert_eq!(only(&report).outcome, RunDirOutcome::Committed);

    // **Both spellings.** The packet says "still carrying `.creating` **or**
    // `.creating.tmp`", which is a kill between staging the marker and
    // publishing it, on a run that later committed. A repair that only knew the
    // published spelling would leave this one for ever, and the run's directory
    // would carry creation residue no reader ever removes.
    let staged_id = "01STALESTAGED00000000000000";
    let staged_id = &staged_id[..26];
    let staged = Husk::at_p0(&fixture, staged_id).stage_marker().commit_log();
    assert!(exists(&staged.public().join(MARKER_STAGED)));
    assert_eq!(
        rundir::classify_run_dir(&staged.public()),
        RunDirClass::Committed
    );

    let report = fixture.run_census();
    assert_eq!(
        report
            .of(staged_id)
            .expect("the staged-marker run is censused")
            .outcome,
        RunDirOutcome::RepairedStaleMarker,
    );
    assert!(!exists(&staged.public().join(MARKER_STAGED)));
    assert!(exists(&staged.public().join(rundir::EVENT_LOG)));
}

/// A committed run whose `run.lock` is held is skipped — "otherwise its live
/// owner removes it in recovery step (a)" — **unless the owner is this
/// process**, which is that step.
///
/// Both halves in one test because the pair is the claim: the same directory,
/// the same lock, and the answer turns only on whether this census is the
/// owner's own recovery.
#[test]
fn a_live_owners_stale_marker_is_removed_by_the_owner_and_by_nobody_else() {
    let fixture = Fixture::new("own-marker");
    let run_id = "01OWN000000000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner()
        .publish_commit_record()
        .commit_log();

    let lock = RunLock::acquire(&husk.public()).expect("the run lock");

    // Another process's census, with the lock held: skipped.
    let report = fixture.run_census_owning(None);
    assert_eq!(only(&report).outcome, RunDirOutcome::Skipped);
    assert!(exists(&husk.public().join(MARKER)));

    // The owner's own census — recovery step (a1) — removes it.
    let report = fixture.run_census_owning(Some(run_id));
    assert_eq!(only(&report).outcome, RunDirOutcome::RepairedStaleMarker);
    assert!(!exists(&husk.public().join(MARKER)));

    drop(lock);
}

/// A resume never reclaims the run it is resuming, even if that run's log has
/// been damaged into a husk under it.
///
/// The own-run exception licenses the marker repair and nothing else. The husk
/// arms are gated on the lock alone, and a resume holds its own run's lock, so
/// the exception cannot reach them. Worth a row of its own: the alternative
/// reading — "the owner may act on its own directory" — deletes the private half
/// of the run the command is in the middle of resuming.
#[test]
fn a_resume_never_reclaims_its_own_run_even_when_its_log_is_damaged() {
    let fixture = Fixture::new("own-husk");
    let run_id = "01OWNHUSK00000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    let before = tree_bytes(&husk.private());
    assert_eq!(rundir::classify_run_dir(&husk.public()), RunDirClass::Husk);

    let lock = RunLock::acquire(&husk.public()).expect("the run lock");
    let report = fixture.run_census_owning(Some(run_id));

    assert_eq!(only(&report).outcome, RunDirOutcome::Skipped);
    assert!(exists(&husk.public()));
    assert_eq!(tree_bytes(&husk.private()), before);
    drop(lock);
}

/// A committed run with no stale marker is reported and left alone.
///
/// The census's answer is total over the runs directory, so a run it does
/// nothing to is still an entry — otherwise "every entry is classified" would be
/// unmeasurable from the report.
#[test]
fn a_committed_run_without_a_marker_is_reported_and_untouched() {
    let fixture = Fixture::new("committed");
    let run_id = "01COMMITTED000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .create_private()
        .publish_owner()
        .publish_commit_record()
        .commit_log();
    let public_before = tree_bytes(&husk.public());
    let private_before = tree_bytes(&husk.private());

    let (report, seen) = fixture.run_census_observed();

    assert_eq!(only(&report).outcome, RunDirOutcome::Committed);
    assert_eq!(only(&report).class, RunDirClass::Committed);
    assert_eq!(seen.executions(), 0, "no funnel runs for a committed run");
    assert_eq!(tree_bytes(&husk.public()), public_before);
    assert_eq!(tree_bytes(&husk.private()), private_before);
}

// ===========================================================================
// The census as a whole
// ===========================================================================

/// The report is **total**: one entry per directory, in run-id order, and every
/// outcome kind of the closed set is reachable.
///
/// A single census over one directory of every shape. Two claims a per-shape
/// suite cannot make: that a census of many directories does not stop at the
/// first reclaim, and that [`RunDirOutcome::KINDS`] is covered rather than
/// merely declared.
#[test]
fn the_census_answer_is_total_and_covers_every_outcome_kind() {
    let fixture = Fixture::new("total");

    // reclaimed-public-only
    let bare = "01AAAA00000000000000000000";
    Husk::at_p0(&fixture, bare);
    // reclaimed-both-halves
    let bound = "01BBBB00000000000000000000";
    Husk::at_p0(&fixture, bound)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    // retained
    let retained = "01CCCC00000000000000000000";
    Husk::at_p0(&fixture, retained)
        .stage_marker()
        .publish_marker()
        .create_private();
    // repaired-stale-marker
    let repaired = "01DDDD00000000000000000000";
    Husk::at_p0(&fixture, repaired)
        .stage_marker()
        .publish_marker()
        .commit_log();
    // committed
    let committed = "01EEEE00000000000000000000";
    Husk::at_p0(&fixture, committed).commit_log();
    // skipped
    let skipped = "01FFFF00000000000000000000";
    let locked = Husk::at_p0(&fixture, skipped)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    let lock = RunLock::acquire(&locked.public()).expect("the run lock");
    // unreclaimable. Sorted **first**, so arming the first
    // `RunDir.RemovePublicHusk` call reaches this directory and not `bare`'s —
    // and so the census's "it did not stop at the first failure" is exercised by
    // every other row of this test.
    let stuck = "010000STUCK000000000000000";
    let stuck_husk = Husk::at_p0(&fixture, stuck);

    let (report, _) =
        fixture.run_census_armed(None, (RunDirSite::RemovePublicHusk, HookPhase::Before, 1));

    let ids: Vec<&str> = report
        .entries()
        .iter()
        .map(|entry| entry.run_id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![stuck, bare, bound, retained, repaired, committed, skipped],
        "one entry per directory, in run-id order"
    );
    assert_eq!(
        report
            .of(stuck)
            .expect("the refusing directory is an entry")
            .outcome
            .kind(),
        "unreclaimable"
    );
    assert!(
        exists(&stuck_husk.public()),
        "the arming is `Before`, so the directory is still there"
    );

    let mut kinds: Vec<&str> = report
        .entries()
        .iter()
        .map(|entry| entry.outcome.kind())
        .collect();
    kinds.sort_unstable();
    let mut expected: Vec<&str> = RunDirOutcome::KINDS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        kinds, expected,
        "every outcome of the closed set is exercised exactly once"
    );

    assert_eq!(report.reclaimed().len(), 2);
    assert_eq!(report.retained().len(), 1);
    assert_eq!(report.repaired().len(), 1);
    assert_eq!(report.skipped().len(), 1);
    assert_eq!(report.unreclaimable().len(), 1);
    assert!(report.possibly_committed().is_empty());
    assert!(
        report.retained().iter().all(|entry| entry.run_id != stuck),
        "an unreclaimable directory is a sibling of the retentions, not one of \
         them: it carries no `RetainReason`"
    );
    for entry in report.entries() {
        assert!(
            !entry.describe().is_empty(),
            "every entry names what was done and why"
        );
    }
    drop(lock);
}

/// **One directory the filesystem will not let go of does not end the census.**
///
/// The census's answer to "cannot be reclaimed" is INV-15's everywhere else:
/// retained and reported. It has to be here too, because this is the same
/// function `upstroke resume` calls: a dead run that left a provable husk whose
/// private half cannot be removed — `EACCES`, `EPERM`, `EBUSY`, or on Windows
/// any still-open handle — used to make `upstroke resume <id>` fail for every
/// run in the repository, on every attempt, for a different run's residue.
///
/// Three claims, and the third is what makes the first two mean anything:
///
/// * the census returns `Ok` and the failing directory is an entry naming the
///   step that refused and carrying the error;
/// * **the public half is still there, marker and all** — `.creating` is the
///   private half's only locator, so a private removal that refused must not be
///   followed by the public one;
/// * the directory sorting **after** it was still censused and still reclaimed.
///   A census that returned early would leave that one on disk and every
///   assertion about the first one green.
#[test]
fn a_husk_whose_private_half_cannot_be_removed_is_reported_and_the_census_continues() {
    let fixture = Fixture::new("unreclaimable");
    // Sorted: the refusing husk first, so "the census continued" is a claim
    // about the one after it.
    let stuck = "01AAAASTUCK000000000000000";
    let after = "01ZZZZAFTER000000000000000";
    let stuck_husk = Husk::at_p0(&fixture, stuck)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    let after_husk = Husk::at_p0(&fixture, after)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    let stuck_private = tree_bytes(&stuck_husk.private());
    assert!(!stuck_private.is_empty());

    let (report, seen) =
        fixture.run_census_armed(None, (RunDirSite::RemovePrivateHusk, HookPhase::Before, 1));

    let entry = report
        .of(stuck)
        .expect("the refusing directory is an entry");
    let RunDirOutcome::Unreclaimable { step, detail } = &entry.outcome else {
        panic!("the failure is not an outcome: {:?}", entry.outcome);
    };
    assert_eq!(*step, FailedStep::PrivateHalf);
    assert!(!detail.is_empty(), "the error was dropped");
    assert_eq!(report.unreclaimable().len(), 1);
    assert!(
        entry.describe().contains("only locator"),
        "the operator is not told why the public half was kept: {}",
        entry.describe()
    );

    // The public half, marker and all.
    assert!(exists(&stuck_husk.public()), "the public half is gone");
    assert!(
        exists(&stuck_husk.public().join(MARKER)),
        "the private half's only locator is gone"
    );
    assert_eq!(
        tree_bytes(&stuck_husk.private()),
        stuck_private,
        "the arming is `Before`, so the removal never ran"
    );

    // And the census did not stop.
    assert_eq!(
        report
            .of(after)
            .expect("the next directory is censused")
            .outcome,
        RunDirOutcome::ReclaimedBothHalves,
        "the census returned early and left the directory after the failure"
    );
    assert!(!exists(&after_husk.public()));
    assert!(!exists(&after_husk.private()));
    assert_eq!(
        seen.count(
            EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
            HookPhase::Before
        ),
        2,
        "the proof-token funnel was entered once per provable husk"
    );
}

/// The three predicates, over the three failing steps that can leave a private
/// half in three different states.
///
/// `deleted_a_private_half` stays **alethic** — a test that states "nothing
/// private was deleted" reads it, so it may not answer `true` on a removal that
/// returned an error. `may_have_deleted_a_private_half` is the epistemic
/// sibling, and the pair is what tells the two opposite failures apart: the
/// private removal that refused (nothing known) from the public removal that
/// refused after the private half went (the private half really is gone).
#[test]
fn a_failed_step_answers_the_alethic_and_epistemic_predicates_apart() {
    let table = [
        (FailedStep::PublicHalf, false, false, false),
        (FailedStep::PrivateHalf, false, false, true),
        (FailedStep::PublicHalfAfterPrivate, true, true, true),
        (FailedStep::StaleMarker, false, false, false),
    ];
    assert_eq!(
        table.len(),
        FailedStep::ALL.len(),
        "a step was added to the closed set and not to this table"
    );
    let mut names = Vec::new();
    for (step, reclaimed, deleted, may_have) in table {
        let outcome = RunDirOutcome::Unreclaimable {
            step,
            detail: "permission denied".to_owned(),
        };
        assert_eq!(outcome.reclaimed_anything(), reclaimed, "{}", step.name());
        assert_eq!(outcome.deleted_a_private_half(), deleted, "{}", step.name());
        assert_eq!(
            outcome.may_have_deleted_a_private_half(),
            may_have,
            "{}",
            step.name()
        );
        assert_eq!(outcome.kind(), "unreclaimable");
        assert!(!step.what_failed().is_empty());
        names.push(step.name());
    }
    assert_eq!(
        names,
        FailedStep::ALL
            .iter()
            .map(|step| step.name())
            .collect::<Vec<_>>(),
        "the table must cover the closed set, in its order"
    );

    // The pair the epistemic predicate exists for: two failures whose private
    // halves are in opposite states must not answer the two questions alike.
    let refused = RunDirOutcome::Unreclaimable {
        step: FailedStep::PrivateHalf,
        detail: "e".to_owned(),
    };
    let after = RunDirOutcome::Unreclaimable {
        step: FailedStep::PublicHalfAfterPrivate,
        detail: "e".to_owned(),
    };
    assert_ne!(
        (
            refused.deleted_a_private_half(),
            refused.may_have_deleted_a_private_half()
        ),
        (
            after.deleted_a_private_half(),
            after.may_have_deleted_a_private_half()
        ),
    );
}

/// A public removal that refuses **after** the private half went is the one
/// failure on which a private half really is gone — and the husk it leaves is
/// one the next census finishes.
#[test]
fn a_public_removal_that_refuses_after_the_private_half_went_says_so() {
    let fixture = Fixture::new("public-after-private");
    let run_id = "01AFTERPRIV0000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();

    let (report, _) =
        fixture.run_census_armed(None, (RunDirSite::RemovePublicHusk, HookPhase::Before, 1));

    let entry = only(&report);
    assert_eq!(
        entry.outcome,
        RunDirOutcome::Unreclaimable {
            step: FailedStep::PublicHalfAfterPrivate,
            detail: match &entry.outcome {
                RunDirOutcome::Unreclaimable { detail, .. } => detail.clone(),
                other => panic!("{other:?}"),
            },
        }
    );
    assert!(
        entry.outcome.deleted_a_private_half(),
        "the private half went through the proof-token funnel and the report \
         will not say so"
    );
    assert!(!exists(&husk.private()), "the private half is still there");
    assert!(exists(&husk.public()), "the public husk is gone");

    // What is left is a husk whose marker names an absent target, which the
    // next census reclaims public-only.
    let second = fixture.run_census();
    assert_eq!(
        only(&second).outcome,
        RunDirOutcome::ReclaimedPublicOnly(UnboundShape::TargetAbsent)
    );
    assert!(!exists(&husk.public()));
}

/// A runs directory that exists and cannot be enumerated **refuses**.
///
/// `rundir::run_dir_names` answers an unreadable runs root and an empty one with
/// the same empty vector, so a census that believed it would report success
/// having scanned nothing — and, since the walk is the only way this census
/// reaches a directory, would silently skip the resuming run's own stale-marker
/// repair too. That is not a retention: no census happened, and no report can
/// say so.
///
/// The runs root is planted as a **file**, through the `Event` funnel, which is
/// the one writer of an arbitrary path this module can reach; a permission bit
/// is not, because `std::fs::set_permissions` is on the effect denylist. A file
/// gives `ENOTDIR` on Unix and `ERROR_DIRECTORY` on Windows, and neither is
/// `NotFound` — which is the one error this function is allowed to read as an
/// emptiness.
#[test]
fn an_unreadable_runs_directory_refuses_rather_than_censusing_nothing() {
    let fixture = Fixture::new("unreadable-runs");
    let runs = rundir::runs_root(&fixture.repo);
    rundir::create_private_dir(runs.parent().expect("`.upstroke`"), &mut rundir::NoHooks)
        .expect("the `.upstroke` directory");
    append_line(&runs, run_started("01FILEATRUNSROOT0000000000"));
    assert!(runs.is_file(), "the runs root must be a file for this test");

    assert!(
        rundir::run_dir_names(&fixture.repo).is_empty(),
        "the swallow this test is about did not happen"
    );
    let error = census_run_dirs(&mut rundir::NoHooks, &fixture.inputs(), None)
        .expect_err("an unreadable runs directory is not an empty one");
    let text = error.to_string();
    assert!(
        text.contains(&runs.display().to_string()),
        "the refusal does not name what could not be enumerated: {text}"
    );
}

/// `startup_census` consumes the worktree-lock witness and **returns**
/// `FreshCensused`, having run both halves.
///
/// The ordering is the call: there is no `FreshCensused` without a
/// `WorktreeLocked` having been given up, and the lock comes back inside the
/// witness because `run_creation` holds it "across the startup census and the
/// whole run". Both halves censused the **same** `R` — one field, not two
/// parameters a caller can disagree with itself about.
#[test]
fn startup_census_returns_the_witness_and_runs_both_halves() {
    let fixture = Fixture::new("fresh-witness");
    let run_id = "01FRESH0000000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();

    let locked = WorktreeLocked::from(fixture.worktree_lock());
    let mut hooks = crate::engine::topology::seams::NoTopologyHooks::new();
    let censused = startup_census(locked, &mut hooks, &fixture.inputs()).expect("the census");

    assert_eq!(
        censused.census().containers().private_root(),
        fixture.private_root.as_path(),
        "half (a) censused the one authorized root, which is half (b)'s root too"
    );
    assert!(
        censused.census().containers().report().reclaimed.is_empty(),
        "no containers to reclaim in this fixture"
    );
    assert_eq!(
        censused
            .census()
            .run_dirs()
            .of(run_id)
            .expect("half (b) ran")
            .outcome,
        RunDirOutcome::ReclaimedBothHalves,
        "half (b) ran, and the witness carries its report"
    );
    assert!(!exists(&husk.private()));

    // The lock is still held: the witness carries it, so a caller that takes
    // the witness apart is the one that decides when the exclusion ends.
    let (locked, census) = censused.into_parts();
    assert_eq!(census.run_dirs().entries().len(), 1);
    drop(locked);
}

/// A census of an empty repository is an empty report, not a failure.
///
/// Also the control for
/// [`an_unreadable_runs_directory_refuses_rather_than_censusing_nothing`]:
/// without it, "refuses when it cannot enumerate" would be satisfied by
/// refusing always, and the first write command in a repository would never
/// run. The absent runs root is asserted rather than assumed, because that is
/// the shape being distinguished from the unreadable one.
#[test]
fn an_empty_runs_directory_censuses_to_an_empty_report() {
    let fixture = Fixture::new("empty");
    assert!(
        !rundir::runs_root(&fixture.repo).exists(),
        "the control must be the absent runs root, not an empty one"
    );
    let report = fixture.run_census();
    assert!(report.entries().is_empty());
    assert!(report.reclaimed().is_empty());
    assert!(report.retained().is_empty());
    assert!(report.unreclaimable().is_empty());
}

/// Half (a) refusing means half (b) never touches the disk.
///
/// The runtime is **reached** and refuses to list: `discover_by_label` will not
/// proceed without an answer, because "a runtime that answers and will not list
/// cannot prove that no labeled container of a dead owner is still running".
/// The husk beside it is fully provable, so a census that had proceeded to half
/// (b) would have deleted it — which is what makes "(a) then (b)" measurable
/// rather than asserted.
#[test]
fn a_refusal_in_half_a_leaves_half_b_untouched() {
    let fixture = Fixture::new("half-a-refuses");
    let run_id = "01HALFA0000000000000000000";
    let husk = Husk::at_p0(&fixture, run_id)
        .stage_marker()
        .publish_marker()
        .create_private()
        .publish_owner();
    let before = tree_bytes(&husk.private());
    assert!(!before.is_empty());

    let inputs = CensusInputs {
        runtime: &RefusingRuntime,
        ..fixture.inputs()
    };
    let locked = WorktreeLocked::from(fixture.worktree_lock());
    let mut hooks = crate::engine::topology::seams::NoTopologyHooks::new();

    let error = startup_census(locked, &mut hooks, &inputs)
        .expect_err("half (a) refuses when the runtime answers and will not list");

    assert!(
        matches!(error, UpstrokeError::Refused { .. }),
        "expected a refusal, got {error:?}"
    );
    assert_eq!(
        tree_bytes(&husk.private()),
        before,
        "half (a) refused, so half (b) performed no effect"
    );
    assert!(exists(&husk.public()));

    // The control: the same fixture, a runtime that is merely unreachable, so
    // half (a) proceeds and half (b) runs. Without it the assertions above
    // would pass against a census that never ran either half.
    let locked = WorktreeLocked::from(fixture.worktree_lock());
    let mut hooks = crate::engine::topology::seams::NoTopologyHooks::new();
    let censused = startup_census(locked, &mut hooks, &fixture.inputs()).expect("the census");
    assert_eq!(
        censused
            .census()
            .run_dirs()
            .of(run_id)
            .expect("half (b) ran")
            .outcome,
        RunDirOutcome::ReclaimedBothHalves,
    );
    assert!(!exists(&husk.private()));
}

// ===========================================================================
// Doubles for half (a)
// ===========================================================================

/// A runtime that cannot be reached.
///
/// `discover_by_label` proceeds without one when the intent namespace is empty:
/// "the runtime is required only when an intent exists or a labeled container is
/// discoverable". Every fixture here has an empty `<R>/containers`, so half (a)
/// is a read-only scan that reclaims nothing — which is exactly what a
/// run-directory suite wants from it.
#[derive(Debug)]
struct UnreachableRuntime;

fn unreachable_for(operation: RuntimeOp) -> RuntimeError {
    RuntimeError::Unreachable {
        operation,
        detail: "no container runtime in this test".to_owned(),
    }
}

impl ContainerRuntime for UnreachableRuntime {
    fn probe(&self) -> Result<(), RuntimeError> {
        Err(unreachable_for(RuntimeOp::Probe))
    }

    fn image_by_reference(&self, _: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        Err(unreachable_for(RuntimeOp::InspectImageByReference))
    }

    fn image_by_id(&self, _: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        Err(unreachable_for(RuntimeOp::InspectImageById))
    }

    fn volume_present(&self, _: &str) -> Result<bool, RuntimeError> {
        Err(unreachable_for(RuntimeOp::InspectVolume))
    }

    fn containers_with_label(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
        Err(unreachable_for(RuntimeOp::ListByLabel))
    }

    fn observe(&self, _: &str) -> Result<Liveness, RuntimeError> {
        Err(unreachable_for(RuntimeOp::Observe))
    }

    fn collect(&self, _: &str) -> Result<ContainerExecution, RuntimeError> {
        Err(unreachable_for(RuntimeOp::Collect))
    }

    fn create(&self, _: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {
        Err(unreachable_for(RuntimeOp::Create))
    }

    fn start(&self, _: &str) -> Result<(), RuntimeError> {
        Err(unreachable_for(RuntimeOp::Start))
    }

    fn stop(&self, _: &str, _: StopMode) -> Result<(), RuntimeError> {
        Err(unreachable_for(RuntimeOp::Stop))
    }

    fn remove(&self, _: &str) -> Result<(), RuntimeError> {
        Err(unreachable_for(RuntimeOp::Remove))
    }
}

/// A runtime that is reached and refuses the listing.
///
/// The other side of `proceeds_without`: an unreachable runtime lets a census
/// with no intents proceed, a runtime that *answers* and will not list does not.
#[derive(Debug)]
struct RefusingRuntime;

impl ContainerRuntime for RefusingRuntime {
    fn probe(&self) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn image_by_reference(&self, _: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        Ok(None)
    }

    fn image_by_id(&self, _: &str) -> Result<Option<ImageInspection>, RuntimeError> {
        Ok(None)
    }

    fn volume_present(&self, _: &str) -> Result<bool, RuntimeError> {
        Ok(false)
    }

    fn containers_with_label(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
        Err(RuntimeError::Failed {
            operation: RuntimeOp::ListByLabel,
            detail: "the daemon answered and refused the listing".to_owned(),
        })
    }

    fn observe(&self, _: &str) -> Result<Liveness, RuntimeError> {
        Err(unreachable_for(RuntimeOp::Observe))
    }

    fn collect(&self, _: &str) -> Result<ContainerExecution, RuntimeError> {
        Err(unreachable_for(RuntimeOp::Collect))
    }

    fn create(&self, _: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {
        Err(unreachable_for(RuntimeOp::Create))
    }

    fn start(&self, _: &str) -> Result<(), RuntimeError> {
        Err(unreachable_for(RuntimeOp::Start))
    }

    fn stop(&self, _: &str, _: StopMode) -> Result<(), RuntimeError> {
        Err(unreachable_for(RuntimeOp::Stop))
    }

    fn remove(&self, _: &str) -> Result<(), RuntimeError> {
        Err(unreachable_for(RuntimeOp::Remove))
    }
}

/// A Git view seam that must never be asked for anything: no fixture here plants
/// an orphan view, so a call is a fixture that stopped matching its own claim.
#[derive(Debug)]
struct NeverView;

impl GitView for NeverView {
    fn materialize(&self, _: &GitViewRequest) -> Result<PathBuf, UpstrokeError> {
        panic!("no fixture in this suite has a Git view");
    }

    fn discard(&self, _: &Path) -> Result<(), UpstrokeError> {
        panic!("no fixture in this suite has a Git view");
    }
}
