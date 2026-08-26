//! The recovery order, exercised against real directories, a real event log,
//! real locks and the fake container runtime.
//!
//! # No raw effect primitive appears here
//!
//! `src/engine/topology/**` is a `TOPOLOGY_MODULE`: it may carry no
//! module-level `allow` of a governed lint, and `std::fs`'s writing half is on
//! the clippy denylist **in tests too**. So every byte this file puts on disk
//! goes through the funnel that owns its site — `rundir::create_public_dir`
//! for a directory, `rundir::stage_/publish_owner_record` and its commit-record
//! pair for the two private records, and `EventLog` for the log. That is not a
//! ceremony: a fixture that planted `owner.json` with `fs::write` would be
//! asserting against a file the production writer never produced.
//!
//! `rundir::remove_public_husk` is what takes a fixture down. It removes a
//! directory's children and then the directory, which is exactly a recursive
//! delete through a site-taking funnel, and it is the only such funnel this
//! module can reach.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::*;
use crate::agent::{AdapterSource, AgentAdapter, Caps, ProcessOutput, TaskRun};
use crate::config::RunnerSelection;
use crate::events::log::{BarrierStep, EventLog, TopologyLine};
use crate::events::{AttemptRecord, BindingSummary, BudgetKind, ChainSummary, GateSummary};
use crate::gates::ShellKind;
use crate::ir::Outcome;
use crate::ir::{
    Artifact, ArtifactId, Effort, Plan, PlanSource, ResolvedEffortPolicy, Task, TaskId, TaskKind,
    Tier,
};
use crate::review::{PassBinding, ReviewPlan};
use crate::rundir::{
    self, CommitRecord, CreatingMarker, NoHooks, OwnerRecord, RepoKey, RetainReason,
};
use crate::runner::container::resolve::RunnerPreflight;
use crate::runner::container::runtime::{ContainerRuntime, ContainerTrace};
use crate::runner::container::{DisposableDirView, FakeOwnerLiveness, FakeRuntime};
use crate::runner::policy::runner_policy_sha256;
use crate::runner::{CommandSpec, Runner, RunnerRequest};
use crate::topology::effects::EventSite;
use crate::topology::effects::{
    EffectSiteId, HookHarness, HookPhase, Injection, InjectionMode, LockSite, ObjectSite, RefSite,
    RunDirSite, SubEffectPoint, WorktreeSite,
};
use crate::topology::events::{
    AttemptFinished4, AttemptSettlement, AttemptStarted4, BudgetExceeded4, CommitSha, Epoch,
    GitRef, ImageIdentity, IncarnationId, LeaseGrant, RunFinished4, RunStarted4, RungBinding,
    RunnerContract, RunnerKind, RunnerPolicy, SessionId, SettlementTransition, TaskDispatched,
    TopologyEvent, TopologyLimits,
};
use crate::topology::fold::{FrozenInputs, TaskState};
use crate::topology::paths::{GitPath, PathSet};
use crate::topology::paths::{PathGrammar, PathPolicy, PathPolicyVersion};
use crate::topology::registry::{TaskKey, TaskRegistry};
use crate::topology::schema::TOPOLOGY_SCHEMA;

use crate::workspace_manager::Refusal;

use crate::engine::topology::identity::{InvocationLedger, ReservationKind, Reservations};
use crate::engine::topology::preflight::RunPreflight;
use crate::engine::topology::seams::{HarnessTopologyHooks, TimeSource, TopologyHooks};
use crate::engine::topology::startup::{FailedStep, RunDirOutcome};

// ---------------------------------------------------------------------------
// Fixed identities
// ---------------------------------------------------------------------------

const RUN_ID: &str = "01KZTPR7E00000000000000001";
const CREATOR: &str = "01KZTAAAAAAAAAAAAAAAAAAAAA";
const RESUMER: &str = "01KZTBBBBBBBBBBBBBBBBBBBBB";
const TS: &str = "2026-08-23T09:41:02Z";
const IMAGE_REF: &str = "ghcr.io/example/upstroke-runner:1.4";
const IMAGE_ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const VOLUME: &str = "upstroke-creds-claude";
const AGENT: &str = "claude-code";
/// The pid the creator wrote into its `.creating` marker. Never consulted by
/// the ownership proof — the marker's pid is not one of the twelve conjuncts —
/// but a marker is not a marker without one.
const CREATOR_PID: u32 = 4242;

/// A clock that does not move, so a durable byte can be asserted against a
/// literal.
#[derive(Debug, Clone, Copy)]
struct Frozen;

impl TimeSource for Frozen {
    fn now_rfc3339(&self) -> String {
        TS.to_owned()
    }
}

// ---------------------------------------------------------------------------
// The fixture
// ---------------------------------------------------------------------------

/// A unique directory per fixture, in one per-process tree.
fn fixture_root(tag: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "upstroke-pr7e-{}-{tag}-{ordinal}",
        std::process::id()
    ))
}

fn mkdir(path: &Path) {
    rundir::create_public_dir(path, &mut NoHooks).expect("the run-directory funnel creates a dir");
}

/// One repository, one committed schema-4 run, and both private records.
///
/// Every knob is a field rather than a constructor argument, because the
/// refusal tests differ from the healthy case in exactly one of them and a
/// nine-argument builder call would hide which.
struct Fixture {
    root: PathBuf,
    /// The seed commit every fixture's repository holds — the base a step (g)
    /// worktree is cut at.
    base_sha: CommitSha,
    repo_root: PathBuf,
    git_dir: PathBuf,
    private_root: PathBuf,
    repo_key: RepoKey,
    started: RunStarted4,
    /// The committed first line, without its newline.
    first_line: Vec<u8>,
    plan: Plan,
}

/// What a fixture may be built wrong in.
#[derive(Default)]
struct Damage {
    /// Write no private half at all.
    no_private_half: bool,
    /// Write no `owner.json`.
    no_owner_record: bool,
    /// Rewrite one field of the owner record.
    owner: Option<fn(&mut OwnerRecord)>,
    /// Rewrite one field of the commit record.
    commit: Option<fn(&mut CommitRecord)>,
    /// Record a private locator of another shape.
    locator: Option<String>,
    /// Record a host runner rather than the container one.
    host_runner: bool,
    /// Extra events, appended after `run_started` in order.
    extra: Vec<TopologyEventBody>,
    /// Leave one generation **open with no attempt** — the state a crash
    /// between `task_dispatched` and `attempt_started` leaves, and the only
    /// state recovery step (g) has anything to do in.
    open_generation: bool,
    /// Register a **second task**, `beta`, beside `alpha`.
    ///
    /// The default plan has one task, so a recovery step that loops over tasks
    /// or generations cannot be told apart from one that handles the first and
    /// stops. Not hypothetical: catalogue entry `PR7-PIPELINE-010` reduced step
    /// (e) to `.take(1)` and the whole suite stayed green. Opt-in rather than
    /// default, so no existing fixture's registry size moves.
    two_tasks: bool,
    /// Freeze a **two-tier** chain instead of the default one-tier one.
    ///
    /// The default chain has a single tier, so a task's rung is always 0 and a
    /// driver that read the rung from the fold is indistinguishable from one
    /// that assumed zero. That is not a hypothetical: it is why the `rung` half
    /// of `PR7-FOLD-LADDER-POSITION`'s reader stayed unwitnessed through the
    /// repair filed against it, and why S5 round 2 found it still open.
    two_tier: bool,
    /// Freeze a two-tier chain with **two attempts per rung**.
    ///
    /// Neither existing chain can show an *accumulated* brief. `chain()` has one
    /// tier, so its second failure exhausts the ladder and the task parks with
    /// no third dispatch; `escalating_chain()` allows one attempt per rung, so
    /// nothing ever carries two entries onto a rung. §11.4's second half —
    /// "next rung, fresh session, **accumulated feedback summary included**" —
    /// needs a ladder deep enough to hold two failures below the rung that reads
    /// them, and this is that ladder. Additive so no existing fixture's chain
    /// moves.
    deep_ladder: bool,
}

impl Fixture {
    /// The manager recovery step (g) rebuilds worktrees through.
    ///
    /// Derived from the fixture's own repository and private root rather than
    /// stubbed: (g)'s whole subject is a real `Worktree.Verify` against a real
    /// checkout, and a manager that could not reach one would make every
    /// assertion about the step vacuous.
    fn manager(&self) -> crate::workspace_manager::WorkspaceManager {
        crate::workspace_manager::WorkspaceManager::derive(
            &self.repo_root,
            &self.private_root,
            &self.started.run_id,
            &self.started.incarnation.0,
        )
        .expect("the fixture's repository and private root are real directories")
    }

    fn build(tag: &str, damage: Damage) -> Self {
        let root = fixture_root(tag);
        let repo_root = root.join("repo");
        let git_dir = repo_root.join(".git");
        let private_root = root.join("private");
        mkdir(&repo_root);
        // A **real** repository, not a `.git` directory made with `mkdir`.
        // Recovery step (g) rebuilds worktrees through a `WorkspaceManager`,
        // and `WorkspaceManager::derive` asks Git where the common dir is — so
        // a fixture whose `.git` is an empty directory cannot express the step
        // at all, and every assertion about it would be vacuous.
        crate::workspace_manager::fixture::git(&repo_root, &["init", "-q", "-b", "main"]);
        // A seed commit, so the repository has a real base a worktree can be
        // cut at. Step (g) recreates `OpenNoAttempt` worktrees "at their bases",
        // and a base that names no object makes the step's own funnel fail for
        // a reason that has nothing to do with what is being tested.
        for setting in [
            ["config", "user.email", "tests@upstroke.local"],
            ["config", "user.name", "upstroke tests"],
            ["config", "core.logAllRefUpdates", "true"],
        ] {
            crate::workspace_manager::fixture::git(&repo_root, &setting);
        }
        crate::workspace_manager::fixture::write_file(&repo_root.join("seed.txt"), b"seed\n");
        crate::workspace_manager::fixture::git(&repo_root, &["add", "-A"]);
        crate::workspace_manager::fixture::git(&repo_root, &["commit", "-q", "-m", "seed"]);
        let base_sha = CommitSha(crate::workspace_manager::fixture::git(
            &repo_root,
            &["rev-parse", "HEAD"],
        ));
        mkdir(&private_root);
        let repo_key = RepoKey::v1(&std::fs::canonicalize(&git_dir).expect("the git dir exists"));

        let public = rundir::public_dir(&repo_root, RUN_ID);
        mkdir(&public);
        let private_dir = private_root.join("runs").join(RUN_ID);
        if !damage.no_private_half {
            mkdir(&private_dir);
        }

        let plan = plan_with(damage.two_tasks);
        let recorded_locator = damage
            .locator
            .clone()
            .unwrap_or_else(|| private_dir.display().to_string());
        let runner = if damage.host_runner {
            host_runner()
        } else {
            container_runner()
        };
        let started = run_started(
            &plan,
            &recorded_locator,
            runner,
            &base_sha,
            damage.two_tier,
            damage.deep_ladder,
            damage.two_tasks,
        );

        // P1: the `.creating` marker the creator published and never removed,
        // because this run was interrupted between P5b's commit record and P8's
        // `RunDir.RemoveMarker`. That is the shape a resume exists for, and it
        // is what makes recovery step (a1)'s "this run's own stale marker,
        // **which the owner removes here**" a removal that removes something.
        // Without it every "no census effect followed this refusal" assertion
        // below is vacuously true, and the census's own write has nothing to be
        // the anchor of.
        let marker = CreatingMarker {
            run_id: RUN_ID.to_owned(),
            repo_key: repo_key.as_str().to_owned(),
            private_dir: recorded_locator.clone(),
            incarnation: CREATOR.to_owned(),
            pid: CREATOR_PID,
            runner_policy_sha256: runner_policy_sha256(&started.runner),
        };
        rundir::stage_marker(&public, &marker, &mut NoHooks).expect("P1a stages the marker");
        rundir::publish_marker(&public, &mut NoHooks).expect("P1b publishes it");

        // The log, through the Event funnel and nothing else.
        let mut warnings = Vec::new();
        let mut log = EventLog::open(
            EventSite::OpenLog,
            &public.join(rundir::EVENT_LOG),
            &mut warnings,
        )
        .expect("the Event funnel opens a fresh log");
        let (line, _) = TopologyLine::round_trip(&event(TopologyEventBody::RunStarted {
            data: Box::new(started.clone()),
        }))
        .expect("run_started survives its own wire format");
        log.append_topology(EventSite::AppendFirst, &line)
            .expect("the commitment boundary");
        let first_line = line.committed_bytes()[..line.committed_bytes().len() - 1].to_vec();
        let mut later: Vec<TopologyEventBody> = Vec::new();
        if damage.open_generation {
            later.push(dispatched_at(&base_sha));
        }
        later.extend(damage.extra.iter().cloned());
        for body in &later {
            let site = crate::events::log::site_for(body);
            let (line, _) =
                TopologyLine::round_trip(&event(body.clone())).expect("a valid later event");
            log.append_topology(site, &line).expect("a later append");
        }
        drop(log);

        if !damage.no_private_half {
            if !damage.no_owner_record {
                let mut owner = OwnerRecord {
                    run_id: RUN_ID.to_owned(),
                    repo_key: repo_key.as_str().to_owned(),
                    public_dir: canonical(&public),
                    incarnation: CREATOR.to_owned(),
                    runner: started.runner.clone(),
                };
                if let Some(damage) = damage.owner {
                    damage(&mut owner);
                }
                rundir::stage_owner_record(&private_dir, &owner, &mut NoHooks)
                    .expect("P3a stages the owner record");
                rundir::publish_owner_record(&private_dir, &mut NoHooks).expect("P3b publishes it");
            }
            let mut commit = CommitRecord {
                run_id: RUN_ID.to_owned(),
                repo_key: repo_key.as_str().to_owned(),
                public_dir: canonical(&public),
                incarnation: CREATOR.to_owned(),
                run_started_sha256: rundir::run_started_sha256(&first_line),
            };
            if let Some(damage) = damage.commit {
                damage(&mut commit);
            }
            rundir::stage_commit_record(&private_dir, &commit, &mut NoHooks)
                .expect("P5a stages the commit record");
            rundir::publish_commit_record(&private_dir, &mut NoHooks).expect("P5b publishes it");
        }

        Self {
            root,
            base_sha,
            repo_root,
            git_dir,
            private_root,
            repo_key,
            started,
            first_line,
            plan,
        }
    }

    fn healthy(tag: &str) -> Self {
        Self::build(tag, Damage::default())
    }

    fn public(&self) -> PathBuf {
        rundir::public_dir(&self.repo_root, RUN_ID)
    }

    fn log(&self) -> PathBuf {
        self.public().join(rundir::EVENT_LOG)
    }

    fn log_bytes(&self) -> Vec<u8> {
        crate::util::read_file_bounded(&self.log()).unwrap_or_default()
    }

    fn inputs(&self) -> FrozenInputs {
        FrozenInputs {
            plan: self.plan.clone(),
            normalized_plan_digest: self.started.normalized_plan_digest.clone(),
        }
    }

    /// The repository-scoped R25 lock file, whose *existence* is what a
    /// `*_before_any_lock` test asserts about.
    fn worktree_lock_file(&self) -> PathBuf {
        self.git_dir.join("upstroke-worktree.lock")
    }

    /// (a0), with the reader ceiling raised so a schema-4 log is readable at
    /// all. Production's ceiling is 3 and refuses here; see
    /// `RootDerived::derive_with`.
    fn derive(&self, explicit: Option<&Path>) -> Result<RootDerived, UpstrokeError> {
        RootDerived::derive_with(&self.repo_root, RUN_ID, explicit, TOPOLOGY_SCHEMA)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // `remove_public_husk` removes a directory's children and then the
        // directory. It is the one recursive delete this module can reach
        // through a site-taking funnel, and a fixture per test is what
        // exhausts inodes on the build box when nothing does.
        let _ = rundir::remove_public_husk(&self.root, &mut NoHooks);
    }
}

// ---------------------------------------------------------------------------
// A husk beside the run
// ---------------------------------------------------------------------------

/// A husk this repository's next write command may reclaim, planted through the
/// same funnels a creator would have used.
///
/// The prefix is a creator that died after P3b and before P5b: a published
/// `.creating`, the private half it names, and the reciprocal `owner.json` — the
/// twelve conjuncts of [`rundir::prove_private_half_ownership`] all satisfied,
/// so [`crate::rundir::PrivateHalfOwnership::Proven`] and both halves
/// reclaimable. `committed` publishes `committed.json` as well, which fails
/// conjunct 12 and turns the same shape into a retention: the control half, so
/// "the census reclaimed it" is a claim about the proof rather than about the
/// census deleting whatever it walks over.
struct PlantedHusk {
    public: PathBuf,
    private: PathBuf,
}

fn plant_husk(fixture: &Fixture, run_id: &str, committed: bool) -> PlantedHusk {
    let public = rundir::public_dir(&fixture.repo_root, run_id);
    let private = fixture.private_root.join("runs").join(run_id);
    mkdir(&public);
    mkdir(&private);

    let runner = container_runner();
    let marker = CreatingMarker {
        run_id: run_id.to_owned(),
        repo_key: fixture.repo_key.as_str().to_owned(),
        private_dir: private.display().to_string(),
        incarnation: CREATOR.to_owned(),
        pid: CREATOR_PID,
        runner_policy_sha256: runner_policy_sha256(&runner),
    };
    rundir::stage_marker(&public, &marker, &mut NoHooks).expect("P1a stages the husk's marker");
    rundir::publish_marker(&public, &mut NoHooks).expect("P1b publishes it");

    let owner = OwnerRecord {
        run_id: run_id.to_owned(),
        repo_key: fixture.repo_key.as_str().to_owned(),
        public_dir: canonical(&public),
        incarnation: CREATOR.to_owned(),
        runner,
    };
    rundir::stage_owner_record(&private, &owner, &mut NoHooks)
        .expect("P3a stages the owner record");
    rundir::publish_owner_record(&private, &mut NoHooks).expect("P3b publishes it");

    if committed {
        let record = CommitRecord {
            run_id: run_id.to_owned(),
            repo_key: fixture.repo_key.as_str().to_owned(),
            public_dir: canonical(&public),
            incarnation: CREATOR.to_owned(),
            run_started_sha256: rundir::run_started_sha256(b"a first line of its own"),
        };
        rundir::stage_commit_record(&private, &record, &mut NoHooks)
            .expect("P5a stages the commit record");
        rundir::publish_commit_record(&private, &mut NoHooks).expect("P5b publishes it");
    }

    PlantedHusk { public, private }
}

/// Every file under `root`, by relative path, with its bytes.
///
/// What a "retained" assertion compares. Byte-identity, not existence: a census
/// that emptied `owner.json` would leave the directory present and every weaker
/// assertion green.
fn tree_bytes(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.insert(
                relative,
                crate::util::read_file_bounded(&path).unwrap_or_default(),
            );
        }
    }
    out
}

/// Every `"event":"<kind>"` in a log, in order.
fn event_kinds(log: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(log)
        .lines()
        .filter_map(|line| {
            let at = line.find("\"event\":\"")? + "\"event\":\"".len();
            let rest = line.get(at..)?;
            let end = rest.find('"')?;
            rest.get(..end).map(std::borrow::ToOwned::to_owned)
        })
        .collect()
}

fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn event(body: TopologyEventBody) -> TopologyEvent {
    TopologyEvent {
        ts: TS.to_owned(),
        body,
    }
}

// ---------------------------------------------------------------------------
// The recorded run
// ---------------------------------------------------------------------------

fn plan_with(two_tasks: bool) -> Plan {
    let mut plan = plan();
    if two_tasks {
        let alpha = plan.tasks[0].clone();
        plan.tasks.push(Task {
            id: TaskId::from("beta"),
            title: "beta".to_owned(),
            body: "beta body".to_owned(),
            acceptance: vec!["beta passes".to_owned()],
            path_hints: vec!["src/beta/*.rs".to_owned()],
            artifacts_out: vec![ArtifactId::from("beta-out")],
            ..alpha
        });
        plan.artifacts.push(Artifact {
            id: ArtifactId::from("beta-out"),
            produced_by: Some(TaskId::from("beta")),
        });
    }
    plan
}

fn plan() -> Plan {
    Plan {
        source: PlanSource {
            adapter: "markdown".to_owned(),
            hash: "frozen-plan-hash".to_owned(),
        },
        tasks: vec![Task {
            id: TaskId::from("alpha"),
            kind: TaskKind::Refactor,
            title: "alpha".to_owned(),
            body: "alpha body".to_owned(),
            depends_on: Vec::new(),
            acceptance: vec!["alpha passes".to_owned()],
            path_hints: vec!["src/alpha/*.rs".to_owned()],
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: vec![ArtifactId::from("alpha-out")],
        }],
        artifacts: vec![Artifact {
            id: ArtifactId::from("alpha-out"),
            produced_by: Some(TaskId::from("alpha")),
        }],
    }
}

fn container_runner() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Container,
        policy: RunnerContract::ContainerV1,
        image: Some(ImageIdentity {
            reference: IMAGE_REF.to_owned(),
            id: IMAGE_ID.to_owned(),
            digest: Some("sha256:2222".to_owned()),
        }),
        credential_volumes: Some(
            [(AGENT.to_owned(), VOLUME.to_owned())]
                .into_iter()
                .collect(),
        ),
    }
}

fn host_runner() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Host,
        policy: RunnerContract::HostV1,
        image: None,
        credential_volumes: None,
    }
}

fn chain() -> ChainSummary {
    ChainSummary {
        task: "alpha".to_owned(),
        tiers: vec![Tier::Mid],
        attempts_per: 2,
        bindings: Some(vec![BindingSummary {
            tier: Tier::Mid,
            agent: AGENT.to_owned(),
            model: "claude-opus-5".to_owned(),
            pinned: false,
        }]),
    }
}

/// A chain with a rung above the first, so an escalation has somewhere to go.
///
/// The binding's **model differs per tier**, which is what makes the rung
/// observable in the dispatched attempt: a driver reading rung 0 for an
/// escalated task runs it on the cheap model forever, and the only visible
/// symptom is a task that never gets better.
fn escalating_chain() -> ChainSummary {
    ChainSummary {
        task: "alpha".to_owned(),
        tiers: vec![Tier::Mid, Tier::Frontier],
        attempts_per: 1,
        bindings: Some(vec![
            // **Rung 0 matches the default chain's binding on purpose.** The
            // `attempt_started` helper seeds that binding, and the fold refuses
            // an attempt whose binding is not the one the run froze for its rung
            // — `check_attempt_started`'s `BindingMismatch`, which caught the
            // first draft of this fixture. Only the rung *above* differs, which
            // is the rung this test is about.
            BindingSummary {
                tier: Tier::Mid,
                agent: AGENT.to_owned(),
                model: "claude-opus-5".to_owned(),
                pinned: false,
            },
            BindingSummary {
                tier: Tier::Frontier,
                agent: AGENT.to_owned(),
                model: "claude-fable-5".to_owned(),
                pinned: false,
            },
        ]),
    }
}

/// Two tiers **and** two attempts per rung.
///
/// The only chain in this file on which a rung can read more than one earlier
/// failure. Rung 0 spends two attempts, both fail, and the escalation lands on
/// rung 1 with two records below it — which is what §11.4's "accumulated
/// feedback summary" is a claim about. Its bindings match
/// [`escalating_chain`]'s for the same reason that one's match [`chain`]'s: the
/// seeded `attempt_started` carries rung 0's binding, and the fold refuses an
/// attempt whose binding is not the one the run froze for its rung.
fn deep_chain() -> ChainSummary {
    ChainSummary {
        attempts_per: 2,
        ..escalating_chain()
    }
}

fn review_plan() -> ReviewPlan {
    ReviewPlan {
        enabled: Some(true),
        alternative_available: Some(false),
        pass_timeout_secs: Some(600),
        primary: Some(PassBinding::new(AGENT, "claude-opus-5")),
        alternative: None,
        second_opinion: vec![None],
    }
}

/// A `run_started` whose two digests authenticate against the frozen plan.
///
/// The registry digest is derived the way the fold derives it — from the plan,
/// this record and the probed agents — rather than written as a literal,
/// because a literal would be a second authority on the same number and the
/// fixture would drift from the fold the first time either changed.
/// `base` is the repository's real seed commit rather than a literal, because
/// the driver dispatches at it: a recorded base that names no object makes
/// `git worktree add` fail for a reason that has nothing to do with what is
/// being tested, and a fixture whose record disagrees with its own repository
/// is not the shape any real run has.
fn run_started(
    plan: &Plan,
    private_dir: &str,
    runner: RunnerPolicy,
    base: &CommitSha,
    two_tier: bool,
    deep_ladder: bool,
    two_tasks: bool,
) -> RunStarted4 {
    let unauthenticated = RunStarted4 {
        schema: TOPOLOGY_SCHEMA,
        upstroke_version: env!("CARGO_PKG_VERSION").to_owned(),
        run_id: RUN_ID.to_owned(),
        incarnation: IncarnationId(CREATOR.to_owned()),
        runner,
        probed_agents: vec![AGENT.to_owned()],
        branch: "upstroke/run".to_owned(),
        integration_ref: GitRef(format!("refs/upstroke/runs/{RUN_ID}/integration")),
        base_sha: base.clone(),
        execution_root: "/does/not/matter".to_owned(),
        private_dir: private_dir.to_owned(),
        plan_path: "PLAN.md".to_owned(),
        config_path: Some("upstroke.toml".to_owned()),
        plan_hash: "frozen-plan-hash".to_owned(),
        normalized_plan_digest: "sha256:aaaa".to_owned(),
        registry_digest: String::new(),
        path_policy: PathPolicy {
            version: PathPolicyVersion::V1,
            case_fold: false,
            grammar: PathGrammar::Globset,
        },
        limits: TopologyLimits {
            max_parallel: 1,
            max_defers: 3,
            max_merge_repairs: 1,
        },
        gates: vec!["clippy".to_owned()],
        gates_from_config: true,
        gate_cmds: vec![GateSummary {
            name: "clippy".to_owned(),
            cmd: "cargo clippy".to_owned(),
            timeout: Duration::from_secs(600),
            shell: ShellKind::Bash,
        }],
        interaction_mode: "attached".to_owned(),
        chains: {
            let first = if deep_ladder {
                deep_chain()
            } else if two_tier {
                escalating_chain()
            } else {
                chain()
            };
            let mut chains = vec![first.clone()];
            if two_tasks {
                chains.push(ChainSummary {
                    task: "beta".to_owned(),
                    ..first
                });
            }
            chains
        },
        effort_policy: ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::High,
            frontier: Effort::Max,
            review: Effort::Medium,
        },
        reviews: {
            // `second_opinion` is per task, so a second task needs a second
            // entry — the registry refuses a record whose review alignment does
            // not match its plan, which is the check working.
            let mut reviews = review_plan();
            if two_tasks {
                reviews.second_opinion.push(None);
            }
            reviews
        },
    };
    let registry_digest = TaskRegistry::originals_with_agents(
        plan,
        &unauthenticated.registry_record(),
        &unauthenticated.probed_agents,
    )
    .expect("the fixture record derives a registry")
    .digest();
    RunStarted4 {
        registry_digest,
        ..unauthenticated
    }
}

// ---------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------

/// A runtime holding this run's recorded image and its credential volume.
fn runtime_holding_the_record() -> FakeRuntime {
    let runtime = FakeRuntime::new(ContainerTrace::default());
    runtime.add_image(IMAGE_ID, Some("sha256:2222"));
    runtime.tag(IMAGE_REF, IMAGE_ID);
    runtime.add_volume(VOLUME);
    runtime
}

/// A `Runner` that answers every request with `exit 0` and records what it saw.
#[derive(Debug, Default)]
struct RecordingRunner {
    seen: Mutex<Vec<RunnerRequest>>,
    /// A program whose invocation fails, so a probe refusal can be constructed.
    failing: Mutex<Option<String>>,
    /// Whether the worker also declares a clean/smudge filter.
    ///
    /// A `.gitattributes` naming a filter makes the staged bytes and the bytes
    /// a gate would see potentially different, which is what the ladder's third
    /// cheap rung refuses.
    filters: Mutex<bool>,
    /// Whether an `Implement` invocation edits the worktree it was given.
    ///
    /// Off by default, because most tests here only care that a process ran.
    /// A driver test that means to reach the **candidate sequence** needs a
    /// non-empty diff: the ladder's cheap rungs reject an empty one, which is
    /// what `pr_sequence[8]`'s "empty-diff attempt failures" names.
    edits: Mutex<bool>,
}

impl RecordingRunner {
    /// A worker that leaves a change behind **and** a filter declaration, so
    /// the staged evidence is not the evidence a gate would see.
    fn filtering() -> Self {
        let runner = Self::editing();
        *runner
            .filters
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
        runner
    }

    /// A runner whose worker leaves a change behind, so an attempt can succeed.
    fn editing() -> Self {
        let runner = Self::default();
        *runner.edits.lock().unwrap_or_else(PoisonError::into_inner) = true;
        runner
    }

    fn failing(program: &str) -> Self {
        let runner = Self::default();
        *runner
            .failing
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(program.to_owned());
        runner
    }

    fn requests(&self) -> Vec<RunnerRequest> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Runner for RecordingRunner {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.clone());
        let failing = self
            .failing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let code = if failing.as_deref() == Some(request.command.program.as_str()) {
            127
        } else {
            0
        };
        // The worker's edit, which is the whole difference between an attempt
        // the cheap rungs reject and one that reaches a candidate.
        if code == 0
            && request.role == crate::runner::ExecutionRole::Implement
            && *self.edits.lock().unwrap_or_else(PoisonError::into_inner)
        {
            // Through the fixture funnel every other test in this file uses:
            // `std::fs::write` is on the effect denylist here, and a test that
            // reached around it would be the first.
            crate::workspace_manager::fixture::write_file(
                &request.workspace.join("worker.txt"),
                b"the worker's edit\n",
            );
            if *self.filters.lock().unwrap_or_else(PoisonError::into_inner) {
                crate::workspace_manager::fixture::write_file(
                    &request.workspace.join(".gitattributes"),
                    b"* filter=upstroke-test\n",
                );
            }
        }
        Ok(ProcessOutput {
            code: Some(code),
            stdout: "1.2.3".to_owned(),
            stderr: String::new(),
            duration: Duration::from_millis(1),
            timed_out: false,
            output_limited: false,
        })
    }
}

/// An adapter that reports itself through one probe process.
#[derive(Debug)]
struct StubAdapter;

impl AgentAdapter for StubAdapter {
    fn id(&self) -> &'static str {
        AGENT
    }

    fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {
        let request = crate::agent::probe_request(
            AGENT,
            CommandSpec::new("claude").arg("--version"),
            0,
            Duration::from_secs(30),
        )?;
        let output = runner.run(&request)?;
        if output.code != Some(0) {
            return Err(UpstrokeError::Agent {
                message: format!("`claude --version` exited {:?}", output.code),
            });
        }
        Ok(Caps {
            version: output.stdout.trim().to_owned(),
            json_output: true,
            session_resume: true,
            cost_reporting: true,
            read_only_mode: true,
            acp: false,
            model_list: true,
        })
    }

    fn build(&self, _run: &TaskRun) -> Result<CommandSpec, UpstrokeError> {
        Ok(CommandSpec::new("claude"))
    }

    fn parse(&self, _out: &ProcessOutput) -> Result<Outcome, UpstrokeError> {
        Err(UpstrokeError::Agent {
            message: "the fixture adapter runs no attempt".to_owned(),
        })
    }
}

struct StubAdapters;

impl AdapterSource for StubAdapters {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        (id == AGENT).then_some(&StubAdapter as &dyn AgentAdapter)
    }
}

/// A `RunnerPreflight` that certifies without spawning, for the tests whose
/// subject is a step other than (c).
struct AlwaysCertifies;

impl RunnerPreflight for AlwaysCertifies {
    fn certify(&self, _policy: &RunnerPolicy) -> Result<(), UpstrokeError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The integration ref namespace, with no Git behind it
// ---------------------------------------------------------------------------

/// What `assert_publishable` finds at the recorded ref.
///
/// The two refusing shapes are the two `WorkspaceManager::assert_publishable`
/// has: `refuse_symbolic` first, then a walk of the worktree records. Both are
/// reproduced with the production [`Refusal`] values rather than with invented
/// messages, so an assertion on the sentence an operator reads is an assertion
/// on the sentence the real funnel would have produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefShape {
    /// A direct ref nothing has checked out. Publishable.
    Direct,
    /// A symbolic ref. `INV-17` makes every engine ref direct.
    Symbolic,
    /// A direct ref some worktree has checked out.
    CheckedOut,
}

/// [`IntegrationRefs`] with no repository behind it, which still enters the
/// `Ref.CreateIntegration` funnel positions.
///
/// **It must enter them.** Every ordering claim in this file reads its evidence
/// out of the shared [`HookHarness`], and a double that performed the effect
/// without consulting `hooks.phase` would leave the one durable Git effect of
/// the whole recovery order invisible to it — a site contributing nothing to
/// the coverage evidence. `hooks` here is the bundle's own
/// [`crate::workspace_manager::EffectHooks`], so the recording lands in the same
/// harness the other four families record into and needs no second wiring.
///
/// It also snapshots the run's event log at each entry. The position claim this
/// file has to make about the P7/P8 step is "the ref was created **before any
/// recovery event was appended**", and the log's bytes at the instant of the
/// effect are that claim directly, with no ordering index standing in for it.
struct RecordingRefs {
    /// Where this run's `events.jsonl` is, so an entry can snapshot it.
    log: PathBuf,
    shape: RefShape,
    at: Mutex<Option<String>>,
    created: Mutex<Vec<(String, String)>>,
    /// How many times `direct_target` was asked. The control half of the
    /// unpublishable-ref test: `assert_publishable` runs **first**, so a build
    /// that dropped it would still refuse a symbolic ref — at
    /// `direct_ref_target`'s own `refuse_symbolic` — and a test that asserted
    /// only "it refused" would stay green through the loss.
    targets_read: Mutex<usize>,
    /// The log's bytes at each entry into `Ref.CreateIntegration`.
    entered: Mutex<Vec<Vec<u8>>>,
}

impl RecordingRefs {
    /// The general constructor, by log path — the kill child has no
    /// [`Fixture`], only the repository the parent named it.
    fn with_log(log: &Path, shape: RefShape, at: Option<String>) -> Self {
        Self {
            log: log.to_path_buf(),
            shape,
            at: Mutex::new(at),
            created: Mutex::new(Vec::new()),
            targets_read: Mutex::new(0),
            entered: Mutex::new(Vec::new()),
        }
    }

    /// Nothing is there — a run killed between P6 and P8.
    fn absent(fixture: &Fixture) -> Self {
        Self::with_log(&fixture.log(), RefShape::Direct, None)
    }

    /// A direct ref already at `sha`.
    fn at(fixture: &Fixture, sha: &str) -> Self {
        Self::with_log(&fixture.log(), RefShape::Direct, Some(sha.to_owned()))
    }

    /// Nothing is there, and `assert_publishable` answers `shape`.
    fn shaped(fixture: &Fixture, shape: RefShape) -> Self {
        Self::with_log(&fixture.log(), shape, None)
    }

    /// Every `(refname, sha)` the funnel actually created.
    fn created(&self) -> Vec<(String, String)> {
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The ref's current target.
    fn target(&self) -> Option<String> {
        self.at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many times the ref's target was read.
    fn targets_read(&self) -> usize {
        *self
            .targets_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The log's bytes at each entry into the funnel.
    fn log_bytes_at_entries(&self) -> Vec<Vec<u8>> {
        self.entered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The event kinds the log held at each entry into the funnel.
    ///
    /// Beside [`Self::log_bytes_at_entries`] and asserted first, because the
    /// byte comparison's failure output is two `run_started` lines rendered as
    /// `Vec<u8>` and nobody can read which of them grew. The kinds say it in
    /// one line, and the bytes still catch a difference the kinds cannot see.
    fn log_kinds_at_entries(&self) -> Vec<Vec<String>> {
        self.log_bytes_at_entries()
            .iter()
            .map(|bytes| event_kinds(bytes))
            .collect()
    }
}

impl IntegrationRefs for RecordingRefs {
    fn assert_publishable(&self, refname: &str) -> Result<(), UpstrokeError> {
        match self.shape {
            RefShape::Direct => Ok(()),
            RefShape::Symbolic => Err(Refusal::SymbolicRef {
                refname: refname.to_owned(),
                target: "refs/heads/somebody-elses-branch".to_owned(),
            }
            .into()),
            RefShape::CheckedOut => Err(Refusal::CheckedOutRef {
                refname: refname.to_owned(),
                worktree: PathBuf::from("worktrees").join("alpha"),
            }
            .into()),
        }
    }

    fn direct_target(&self, refname: &str) -> Result<Option<String>, UpstrokeError> {
        *self
            .targets_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += 1;
        // `WorkspaceManager::direct_ref_target` opens with `refuse_symbolic`
        // too, and reproducing that is what makes the symbolic case's
        // `targets_read` assertion load-bearing rather than decorative: without
        // it, dropping `assert_publishable` would leave the symbolic arm still
        // refusing here and nothing would notice which check caught it.
        if self.shape == RefShape::Symbolic {
            return Err(Refusal::SymbolicRef {
                refname: refname.to_owned(),
                target: "refs/heads/somebody-elses-branch".to_owned(),
            }
            .into());
        }
        Ok(self.target())
    }

    fn create_zero_old(
        &self,
        hooks: &mut dyn crate::workspace_manager::EffectHooks,
        refname: &str,
        new: &str,
    ) -> Result<(), UpstrokeError> {
        let site = EffectSiteId::Ref(RefSite::CreateIntegration);
        self.entered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(crate::util::read_file_bounded(&self.log).unwrap_or_default());
        injected(
            hooks.phase(site, HookPhase::Before),
            site,
            HookPhase::Before,
        )?;
        {
            let mut at = self.at.lock().unwrap_or_else(PoisonError::into_inner);
            if at.is_some() {
                // What `git update-ref --no-deref <ref> <new> ""` answers when
                // the ref appeared between the read and the write.
                return Err(UpstrokeError::Git {
                    message: format!("`{refname}` already exists; zero-old refuses"),
                });
            }
            *at = Some(new.to_owned());
        }
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((refname.to_owned(), new.to_owned()));
        injected(hooks.phase(site, HookPhase::After), site, HookPhase::After)
    }
}

/// `workspace_manager::apply`, which is private to that module — the same three
/// answers, so an arming at this site does here what it does at every other Git
/// funnel.
fn injected(
    injection: Injection,
    site: EffectSiteId,
    phase: HookPhase,
) -> Result<(), UpstrokeError> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(UpstrokeError::Refused {
            message: format!("the `{site}` funnel was made to fail at its `{phase}` phase"),
        }),
    }
}

fn container_selection() -> RunnerSelection {
    RunnerSelection {
        kind: RunnerKind::Container,
        image: Some(IMAGE_REF.to_owned()),
        credential_volumes: [(AGENT.to_owned(), VOLUME.to_owned())]
            .into_iter()
            .collect(),
        mounts: Vec::new(),
        from_config: true,
    }
}

// ---------------------------------------------------------------------------
// A funnel that refuses, inside the whole recovery order
// ---------------------------------------------------------------------------

/// [`HarnessTopologyHooks`] with its run-directory family replaced by one that
/// returns [`Injection::Error`] at a nominated `(site, phase, nth)`, and records
/// into the same [`HookHarness`] the other four families do.
///
/// Module-local, because `HookHarness::arm` takes a [`SubEffectPoint`] and
/// `hook()` answers `Proceed` to `Before`/`After` unconditionally, so a
/// `RunDir` site's two phases are not armable through it.
struct ArmedHooks {
    inner: HarnessTopologyHooks,
    rundir: ArmedRunDir,
}

struct ArmedRunDir {
    harness: Arc<Mutex<HookHarness>>,
    site: RunDirSite,
    phase: HookPhase,
    nth: usize,
    seen: usize,
}

impl ArmedHooks {
    fn new(
        harness: &Arc<Mutex<HookHarness>>,
        (site, phase, nth): (RunDirSite, HookPhase, usize),
    ) -> Self {
        Self {
            inner: HarnessTopologyHooks::new(Arc::clone(harness)),
            rundir: ArmedRunDir {
                harness: Arc::clone(harness),
                site,
                phase,
                nth,
                seen: 0,
            },
        }
    }
}

impl rundir::RunDirHooks for ArmedRunDir {
    fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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

impl TopologyHooks for ArmedHooks {
    fn effects(&mut self) -> &mut dyn crate::workspace_manager::EffectHooks {
        self.inner.effects()
    }

    fn rundir(&mut self) -> &mut dyn rundir::RunDirHooks {
        &mut self.rundir
    }

    fn events(&mut self) -> &mut dyn crate::events::log::EventHooks {
        self.inner.events()
    }

    fn container(&mut self) -> &mut dyn crate::runner::container::ContainerHooks {
        self.inner.container()
    }

    fn spawn(&mut self) -> &mut dyn crate::agent::proc::SpawnHooks {
        self.inner.spawn()
    }
}

// ---------------------------------------------------------------------------
// Driving one resume
// ---------------------------------------------------------------------------

/// What one resume was given, beyond the fixture.
struct Given<'a> {
    runtime: &'a dyn ContainerRuntime,
    preflight: &'a dyn RunnerPreflight,
    today: RunnerSelection,
    inputs: FrozenInputs,
    explicit_root: Option<PathBuf>,
    /// The integration ref namespace the P7/P8 step publishes into.
    ///
    /// Owned rather than borrowed, so [`Given::healthy`] can build the
    /// default — a resume killed between P6 and P8 finds **no** ref, which is
    /// the shape every other test in this file already implied and none of them
    /// could say. A test that needs another shape assigns the field.
    refs: RecordingRefs,
}

impl<'a> Given<'a> {
    /// The healthy case: the runtime holds the record, the pre-flight
    /// certifies, today's config is the recorded one, and the recorded
    /// integration ref is not there yet.
    fn healthy(
        fixture: &Fixture,
        runtime: &'a FakeRuntime,
        preflight: &'a dyn RunnerPreflight,
    ) -> Self {
        Self {
            runtime,
            preflight,
            today: container_selection(),
            inputs: fixture.inputs(),
            explicit_root: None,
            refs: RecordingRefs::absent(fixture),
        }
    }
}

/// Run (a0) and then the whole order, recording every site into `harness`.
fn resume(
    fixture: &Fixture,
    harness: &Arc<Mutex<HookHarness>>,
    given: &Given<'_>,
) -> (Result<Recovered, UpstrokeError>, Vec<String>) {
    let (outcome, warnings) = resume_holding(fixture, harness, given);
    // The handle is dropped here, which releases the run lock and then the
    // worktree lease — the same thing that happened at the end of the recovery
    // order before the loop existed to hold them. A test that needs them alive
    // takes [`resume_holding`].
    (outcome.map(|(recovered, _handle)| recovered), warnings)
}

/// [`resume`], keeping the [`RunHandle`] the order hands back.
fn resume_holding(
    fixture: &Fixture,
    harness: &Arc<Mutex<HookHarness>>,
    given: &Given<'_>,
) -> (Result<(Recovered, RunHandle), UpstrokeError>, Vec<String>) {
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(harness)).recording_durability();
    resume_with(fixture, &mut hooks, given)
}

/// [`resume`], with the hook bundle supplied — so a test can arm one.
fn resume_with(
    fixture: &Fixture,
    hooks: &mut dyn TopologyHooks,
    given: &Given<'_>,
) -> (Result<(Recovered, RunHandle), UpstrokeError>, Vec<String>) {
    let liveness = FakeOwnerLiveness::new();
    let view = DisposableDirView::new(ContainerTrace::default());
    let incarnation = IncarnationId(RESUMER.to_owned());
    let manager = fixture.manager();
    let mut warnings = Vec::new();
    let outcome = fixture
        .derive(given.explicit_root.as_deref())
        .and_then(|root| {
            run_recovery_order(
                root,
                &ResumeSeams {
                    repo_root: &fixture.repo_root,
                    worktree_git_dir: &fixture.git_dir,
                    repo_key: &fixture.repo_key,
                    incarnation: &incarnation,
                    inputs: given.inputs.clone(),
                    today: &given.today,
                    runtime: given.runtime,
                    liveness: &liveness,
                    view: &view,
                    preflight: given.preflight,
                    refs: &given.refs,
                    manager: &manager,
                    clock: &Frozen,
                },
                hooks,
                &mut warnings,
            )
        });
    (outcome, warnings)
}

fn harness() -> Arc<Mutex<HookHarness>> {
    Arc::new(Mutex::new(HookHarness::new()))
}

/// Whether any lock site ran — the R17 half of "no hold was taken".
fn any_lock_site_ran(harness: &Arc<Mutex<HookHarness>>) -> Vec<&'static str> {
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    LockSite::ALL
        .iter()
        .copied()
        .filter(|site| seen.touched(EffectSiteId::Lock(*site)))
        .map(LockSite::name)
        .collect()
}

/// The index of a site's first observation, for an ordering assertion.
fn first_observation(harness: &Arc<Mutex<HookHarness>>, site: EffectSiteId) -> Option<usize> {
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    seen.coverage()
        .iter()
        .position(|observation| observation.site == site)
}

fn message(error: &UpstrokeError) -> String {
    error.to_string()
}

// ===========================================================================
// (a0) — the read-only refusals, before any lock
// ===========================================================================

/// An explicit `--private-root` that names another root refuses **before any
/// lock**, and "before any lock" is asserted as the packet states it: no R17
/// hold was taken and no R25 lock file was created.
///
/// "The command refused" is a weaker claim and would be green for an
/// implementation that took the worktree lease, created
/// `upstroke-worktree.lock`, and then noticed. The lock file is the one that
/// bites: `Lock.AcquireWorktree`'s funnel opens it with `create(true)`, so
/// merely *reaching* the acquisition leaves a repository-scoped artifact behind
/// on a command that was supposed to end read-only.
#[test]
fn resume_with_explicit_private_root_mismatch_refused_before_any_lock() {
    let fixture = Fixture::healthy("explicit-root");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let mut given = Given::healthy(&fixture, &runtime, &certifies);
    given.explicit_root = Some(fixture.root.join("somewhere-else"));

    let (outcome, _) = resume(&fixture, &harness, &given);

    let error = outcome.expect_err("a root the run did not record is refused");
    let text = message(&error);
    assert!(
        text.contains(&fixture.private_root.display().to_string()),
        "the refusal must name the recorded root: {text}"
    );
    assert!(
        text.contains("somewhere-else"),
        "the refusal must name the root that was asked for: {text}"
    );
    assert!(
        any_lock_site_ran(&harness).is_empty(),
        "a refusal at (a0) precedes Lock.AcquireWorktree, so no R17 hold is taken: {:?}",
        any_lock_site_ran(&harness)
    );
    assert!(
        !fixture.worktree_lock_file().exists(),
        "no R25 lock file is created by a refusal that precedes the acquisition"
    );
}

/// A recorded locator of any shape other than `<root>/runs/<run_id>` refuses
/// before any lock, and every shape is refused rather than only the obvious
/// one.
///
/// Three shapes, because each fails a different clause: a missing `runs`
/// component, a trailing component that is not the run id, and a locator whose
/// path escapes upwards. The third is the one a "does it end with the run id"
/// check would accept.
#[test]
fn malformed_recorded_locator_refused_before_any_lock() {
    for (tag, locator) in [
        ("no-runs", format!("/tmp/upstroke-pr7e-root/{RUN_ID}")),
        (
            "wrong-tail",
            "/tmp/upstroke-pr7e-root/runs/another".to_owned(),
        ),
        (
            "escapes",
            format!("/tmp/upstroke-pr7e-root/runs/other/../runs/{RUN_ID}"),
        ),
    ] {
        let fixture = Fixture::build(
            &format!("locator-{tag}"),
            Damage {
                locator: Some(locator.clone()),
                ..Damage::default()
            },
        );
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let (outcome, _) = resume(&fixture, &harness, &given);

        let error = outcome.expect_err("a locator of another shape is refused");
        let text = message(&error);
        assert!(
            text.contains(&locator) && text.contains("is not of the shape"),
            "the refusal must quote the locator it refused ({tag}): {text}"
        );
        assert!(
            any_lock_site_ran(&harness).is_empty(),
            "no R17 hold is taken for a locator refusal ({tag}): {:?}",
            any_lock_site_ran(&harness)
        );
        assert!(
            !fixture.worktree_lock_file().exists(),
            "no R25 lock file is created for a locator refusal ({tag})"
        );
    }
}

/// A resume takes the private root **from the record**, not from today's
/// default — even when the default root has moved somewhere else entirely.
///
/// The fixture's root is a temporary directory that is never
/// `rundir::default_private_root()`, so a `derive` that consulted the default
/// would produce a different path and the census below it would scan the wrong
/// tree. Asserted as an equality against the recorded locator's parent rather
/// than as "the resume succeeded".
#[test]
fn resume_derives_private_root_from_record_when_default_changed() {
    let fixture = Fixture::healthy("nondefault-root");
    let root = fixture.derive(None).expect("(a0) derives");

    // Compared canonical-to-canonical, because `authorized_root` is
    // deliberately **lexical**: it refuses a locator whose shape is not
    // `<root>/runs/<run_id>` and resolves nothing, so it hands back the root in
    // whatever form the record wrote. Canonicalising only the right-hand side
    // compares two spellings of one directory and fails wherever the temporary
    // directory sits under a symlink — which is macOS, where `TMPDIR` is under
    // `/var` and `/var` is a link to `/private/var`. Linux's `/tmp` is real, so
    // this passed there and failed only in CI's macOS leg.
    let canonical =
        |path: &std::path::Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    assert_eq!(
        canonical(root.private_root()),
        canonical(&fixture.private_root),
        "the authorized root is the one `run_started.private_dir` names"
    );
    assert_ne!(
        root.private_root(),
        rundir::default_private_root(),
        "the fixture must not accidentally be the default root, or this test proves nothing"
    );
    assert_eq!(root.run_id(), RUN_ID);
    assert_eq!(root.first_line(), fixture.first_line.as_slice());
}

// ===========================================================================
// (a) — the records, before any private write
// ===========================================================================

/// A recorded private half that is not on disk refuses, and **is not
/// recreated**.
///
/// `recovery_order` (a): "a missing schema-4 private half is not recreated —
/// deferred". So the assertion is two-sided: the command refuses, *and* the
/// directory the record names is still absent afterwards. A build that
/// helpfully created it would satisfy "refuses" for one more line and then
/// authorize deletions against a boundary nobody wrote.
#[test]
fn resume_refuses_missing_private_half() {
    let fixture = Fixture::build(
        "no-private-half",
        Damage {
            no_private_half: true,
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a missing private half refuses"));
    assert!(
        text.contains("is not recreated"),
        "the refusal says the half is not recreated: {text}"
    );
    assert!(
        !fixture.private_root.join("runs").join(RUN_ID).exists(),
        "the private half must still be absent: nothing recreates it"
    );
}

/// A missing `owner.json`, and a present one disagreeing in any of the four
/// identity fields, both refuse — and each refusal names the field.
///
/// One test over five cases rather than five tests, because the claim is that
/// the check is a *conjunction*: a build that compared only the run id passes
/// any single-case test that happens to damage the run id.
#[test]
fn resume_refuses_missing_or_disagreeing_owner_record() {
    let cases: Vec<(&str, Damage, &str)> = vec![
        (
            "absent",
            Damage {
                no_owner_record: true,
                ..Damage::default()
            },
            "owner.json",
        ),
        (
            "run-id",
            Damage {
                owner: Some(|owner| owner.run_id = "01KZTPR7E00000000000000009".to_owned()),
                ..Damage::default()
            },
            "run id",
        ),
        (
            "repo-key",
            Damage {
                owner: Some(|owner| owner.repo_key = "0123456789abcdef".to_owned()),
                ..Damage::default()
            },
            "repo key",
        ),
        (
            "public-dir",
            Damage {
                owner: Some(|owner| owner.public_dir = "/elsewhere/runs/x".to_owned()),
                ..Damage::default()
            },
            "public directory",
        ),
        (
            "incarnation",
            Damage {
                owner: Some(|owner| owner.incarnation = RESUMER.to_owned()),
                ..Damage::default()
            },
            "incarnation",
        ),
    ];
    for (tag, damage, expected) in cases {
        let fixture = Fixture::build(&format!("owner-{tag}"), damage);
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let (outcome, _) = resume(&fixture, &harness, &given);

        let text = message(&outcome.expect_err("a disagreeing owner record refuses"));
        assert!(
            text.contains(expected),
            "the refusal for `{tag}` must name `{expected}`: {text}"
        );
        // Before any private write: the private half still holds exactly the
        // two records the creator left, and nothing new.
        let private = fixture.private_root.join("runs").join(RUN_ID);
        assert!(
            !private.join("questions").exists() && !private.join("report.json").exists(),
            "a record refusal precedes every private write ({tag})"
        );
    }
}

/// `committed.json`'s `run_started_sha256` must equal the digest of the
/// committed first line, and a mismatch refuses quoting **both** numbers.
#[test]
fn resume_refuses_commit_record_digest_mismatch() {
    let fixture = Fixture::build(
        "commit-digest",
        Damage {
            commit: Some(|commit| {
                commit.run_started_sha256 = format!("sha256:{}", "0".repeat(64));
            }),
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a commit record that names another line refuses"));
    let actual = rundir::run_started_sha256(&fixture.first_line);
    assert!(
        text.contains(&format!("sha256:{}", "0".repeat(64))) && text.contains(&actual),
        "the refusal quotes what the record says and what the line digests: {text}"
    );
    assert!(
        !harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .touched(EffectSiteId::Event(EventSite::OpenLog)),
        "a commit-record refusal is at (a) and precedes the barrier's Event.OpenLog"
    );
}

/// `owner.json.runner` must equal `run_started(4).runner` exactly, and the
/// refusal names **which field** moved.
///
/// INV-23 makes this an (a) refusal rather than a (c) one: "every later
/// incarnation rebuilds the Runner from `run_started(4).runner` — **verified
/// equal to `owner.json.runner`** — before its RunnerPreflight". A build that
/// checked only at the rebuild would already have censused, which is a
/// fold-derived reclaim decided under a runner identity nobody agreed on.
#[test]
fn resume_refuses_owner_record_runner_mismatch() {
    let fixture = Fixture::build(
        "owner-runner",
        Damage {
            owner: Some(|owner| {
                if let Some(image) = owner.runner.image.as_mut() {
                    image.reference = "ghcr.io/example/another:9.9".to_owned();
                }
            }),
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a runner the two records disagree on refuses"));
    assert!(
        text.contains("image reference"),
        "the refusal names which field moved: {text}"
    );
    assert!(
        !harness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .touched(EffectSiteId::Event(EventSite::OpenLog)),
        "the runner comparison is at (a), before the barrier"
    );
}

// ===========================================================================
// (a1) — the stable-prefix barrier
// ===========================================================================

/// A plan whose digest is not the one the log recorded refuses at the barrier's
/// **checked replay**, and nothing fold-derived happens.
///
/// `refusal_condition`'s first clause is "plan or registry digest mismatch",
/// and `stable_prefix_barrier` step (5) is where a log is replayed through the
/// checked fold. So the refusal is the replay's, and the assertion is that it
/// names `CheckedReplay` — not merely that something went wrong.
#[test]
fn resume_refuses_digest_mismatch() {
    let fixture = Fixture::healthy("digest-mismatch");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let mut given = Given::healthy(&fixture, &runtime, &certifies);
    given.inputs.normalized_plan_digest = "sha256:not-the-recorded-one".to_owned();

    let before = fixture.log_bytes();
    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a moved plan digest refuses"));
    assert!(
        text.contains(BarrierStep::CheckedReplay.name()),
        "the refusal names the barrier step: {text}"
    );
    assert!(
        text.contains("normalized plan"),
        "and the digest that disagreed: {text}"
    );
    assert_eq!(
        fixture.log_bytes(),
        before,
        "a barrier refusal appends nothing"
    );
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        !seen.touched(EffectSiteId::RunDir(RunDirSite::RemoveMarker)),
        "no census effect follows a refused replay"
    );
}

/// `Event.OpenLog`, its `SyncPrefix` point and `Event.ProvePrefixStable` all
/// execute **before** the first fold-derived effect of the census.
///
/// The ordering is asserted over the harness's first-observation order, which
/// is what makes this a claim about the *sequence* rather than about
/// possession. `RunDir.RemoveMarker` is the census's own write and, **in this
/// fixture**, the earliest fold-derived effect the order performs — the runs
/// tree holds this run's directory and nothing else, so no husk reclaim can
/// precede it. That is a property of the fixture rather than of the order: a
/// husk sorting before this run's id would put `RunDir.RemovePrivateHusk`
/// first, and the census walks in ascending run-id order. So the fixture's
/// emptiness is asserted rather than assumed, and the anchor is the census's
/// first effect *here*: if the barrier's three sites do not all precede it, the
/// resume decided something from a prefix it had not proven.
#[test]
fn resume_establishes_stable_prefix_barrier_before_any_fold_derived_effect() {
    let fixture = Fixture::healthy("barrier-order");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    // Asserted **before** the resume, because the resume reclaims what it walks:
    // afterwards a husk that had preceded this run in the walk is gone and the
    // same assertion passes vacuously.
    assert_eq!(
        rundir::run_dir_names(&fixture.repo_root),
        vec![RUN_ID.to_owned()],
        "the anchor is the census's first effect only while this run's \
         directory is the only one in the tree"
    );

    let (outcome, _) = resume(&fixture, &harness, &given);
    outcome.expect("the healthy resume completes");

    let marker = first_observation(&harness, EffectSiteId::RunDir(RunDirSite::RemoveMarker))
        .expect("the census removes this run's stale marker");
    let open = first_observation(&harness, EffectSiteId::Event(EventSite::OpenLog))
        .expect("Event.OpenLog ran");
    let proven = first_observation(&harness, EffectSiteId::Event(EventSite::ProvePrefixStable))
        .expect("Event.ProvePrefixStable ran");
    let append = first_observation(&harness, EffectSiteId::Event(EventSite::Append))
        .or_else(|| first_observation(&harness, EffectSiteId::Event(EventSite::AppendFirst)));

    assert!(
        open < marker,
        "Event.OpenLog ({open}) before the census ({marker})"
    );
    assert!(
        proven < marker,
        "Event.ProvePrefixStable ({proven}) before the census ({marker})"
    );
    if let Some(append) = append {
        assert!(
            proven < append,
            "the barrier ({proven}) before every recovery event ({append}) — O33 and O18"
        );
    }
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        seen.reached_point(
            EffectSiteId::Event(EventSite::OpenLog),
            SubEffectPoint::SyncPrefix,
            InjectionMode::ErrorReturn
        ),
        "the SyncPrefix point is consulted, which is what makes it armable"
    );
}

/// A `SyncPrefix` that returns `Err` ends the command with **nothing done**.
///
/// `stable_prefix_barrier`: "a failed sync … performs none of those effects:
/// the write command ends … with an infrastructure error naming the run id and
/// the failed step, no append handle is used, the run is NoRunFinished and
/// resumable". Three assertions, because "it returned an error" is true of a
/// build that censused first and refused afterwards.
#[test]
fn resume_refuses_before_any_fold_derived_effect_when_prefix_sync_fails() {
    let fixture = Fixture::healthy("sync-fails");
    let harness = harness();
    harness
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::OpenLog),
            SubEffectPoint::SyncPrefix,
            InjectionMode::ErrorReturn,
        )
        .expect("SyncPrefix supports an error return");
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let before = fixture.log_bytes();
    let (outcome, _) = resume(&fixture, &harness, &given);

    let text = message(&outcome.expect_err("a failed SyncPrefix refuses"));
    assert!(
        text.contains(BarrierStep::SyncPrefix.name()),
        "the refusal names the failed step: {text}"
    );
    assert_eq!(fixture.log_bytes(), before, "no append handle was used");
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        !seen.touched(EffectSiteId::RunDir(RunDirSite::RemoveMarker)),
        "no census reclaim follows a failed sync"
    );
    assert!(
        !seen.touched(EffectSiteId::Event(EventSite::Append)),
        "and no recovery event"
    );
}

// ---------------------------------------------------------------------------
// Later events, for the prefixes a resume has to recover from
// ---------------------------------------------------------------------------

const ALPHA: TaskKey = TaskKey(0);
const GEN: GenerationId = GenerationId(0);

/// [`dispatched`], at a base that names a real object.
///
/// The constant-SHA version is enough for every test whose subject is the
/// fold, because the fold does not resolve a base. Step (g) does — it cuts a
/// worktree at it — so its fixture has to name the repository's own commit.
fn dispatched_at(base: &CommitSha) -> TopologyEventBody {
    let TopologyEventBody::TaskDispatched { mut data } = dispatched() else {
        unreachable!("`dispatched` builds a `TaskDispatched`")
    };
    data.base_sha = base.clone();
    TopologyEventBody::TaskDispatched { data }
}

fn dispatched() -> TopologyEventBody {
    TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key: ALPHA,
            generation: GEN,
            base_sha: CommitSha("a".repeat(40)),
            worktree_path: "wt/g0".to_owned(),
            lease: LeaseGrant::Predicted {
                paths: PathSet::Prefixes {
                    paths: vec![GitPath("src/alpha".to_owned())],
                },
            },
            source_candidate: None,
        },
    }
}

/// Re-key an event built for `alpha` onto another task.
///
/// The seeded-event helpers are all `ALPHA`'s, which was enough while every
/// fixture had one task. A step that loops needs a second, and re-keying is
/// cheaper than a second set of builders — and keeps the two tasks' events
/// identical apart from the key, which is what makes "the step handled both" a
/// claim about the step rather than about the fixture.
///
/// The predicted region moves with the key: two tasks holding the same region
/// is an overlap the fold refuses, and rightly.
fn for_task(key: TaskKey, prefix: &str, body: TopologyEventBody) -> TopologyEventBody {
    match body {
        TopologyEventBody::TaskDispatched { mut data } => {
            data.key = key;
            data.worktree_path = format!("wt/{prefix}-g0");
            data.lease = LeaseGrant::Predicted {
                paths: PathSet::Prefixes {
                    paths: vec![GitPath(format!("src/{prefix}"))],
                },
            };
            TopologyEventBody::TaskDispatched { data }
        }
        TopologyEventBody::AttemptStarted { mut data } => {
            data.key = key;
            TopologyEventBody::AttemptStarted { data }
        }
        TopologyEventBody::AttemptFinished { mut data } => {
            data.key = key;
            TopologyEventBody::AttemptFinished { data }
        }
        other => other,
    }
}

/// Re-key an event built for generation 0 onto a later generation.
///
/// `for_task` moves a seeded event sideways; this moves it forward. A
/// **closed** generation is what a sessionless retry and every escalation leave
/// behind — `settle::failed` closes it and the next attempt runs in a fresh one
/// — so a log with two failures below one rung has two generations in it, and
/// `attempt_started` on a closed generation is a barrier refusal rather than a
/// fixture. The worktree path moves with the generation because two live
/// dispatches may not name the same one.
fn in_generation(generation: GenerationId, body: TopologyEventBody) -> TopologyEventBody {
    match body {
        TopologyEventBody::TaskDispatched { mut data } => {
            data.generation = generation;
            data.worktree_path = format!("wt/g{}", generation.0);
            TopologyEventBody::TaskDispatched { data }
        }
        TopologyEventBody::AttemptStarted { mut data } => {
            data.generation = generation;
            TopologyEventBody::AttemptStarted { data }
        }
        TopologyEventBody::AttemptFinished { mut data } => {
            data.generation = generation;
            TopologyEventBody::AttemptFinished { data }
        }
        other => other,
    }
}

fn attempt_started(attempt: u32) -> TopologyEventBody {
    TopologyEventBody::AttemptStarted {
        data: AttemptStarted4 {
            key: ALPHA,
            generation: GEN,
            attempt: AttemptNumber(attempt),
            rung: 0,
            binding: RungBinding {
                tier: Tier::Mid,
                agent: AGENT.to_owned(),
                model: "claude-opus-5".to_owned(),
                pinned: false,
                effort: Effort::High,
            },
            pool: None,
            resume_session: None,
            materialization_observed: None,
        },
    }
}

fn attempt_record(attempt: u32) -> AttemptRecord {
    AttemptRecord {
        attempt,
        tier: "mid".to_owned(),
        model: "claude-opus-5".to_owned(),
        pool: None,
        resumed: false,
        duration: Duration::from_millis(5),
        cost_usd: Some(0.5),
        reviews: Vec::new(),
        session_id: None,
        usage: None,
        failure: None,
    }
}

fn attempt_finished(attempt: u32, settlement: AttemptSettlement) -> TopologyEventBody {
    TopologyEventBody::AttemptFinished {
        data: Box::new(AttemptFinished4 {
            key: ALPHA,
            generation: GEN,
            attempt: AttemptNumber(attempt),
            record: Box::new(attempt_record(attempt)),
            settlement,
        }),
    }
}

/// `attempt_finished` carrying the failure — and the feedback — a crash left
/// durable.
///
/// [`attempt_finished`] records `failure: None`, which is the shape of an
/// attempt nothing judged. A crash-resume claim is about the other shape: the
/// ladder decided something, and §11.4's feedback is on the record it decided
/// from. `detail` is what the next attempt is told, and it is the field this
/// helper exists to put in a log.
fn attempt_finished_failing(
    attempt: u32,
    kind: crate::ladder::FailureKind,
    reason: &str,
    detail: &str,
    settlement: AttemptSettlement,
) -> TopologyEventBody {
    let TopologyEventBody::AttemptFinished { mut data } = attempt_finished(attempt, settlement)
    else {
        unreachable!("attempt_finished builds an attempt_finished")
    };
    data.record.failure = Some(crate::events::FailureRecord {
        kind,
        origin: crate::ladder::FailureOrigin::Worker,
        reason: reason.to_owned(),
        detail: Some(detail.to_owned()),
    });
    TopologyEventBody::AttemptFinished { data }
}

fn budget_exceeded(epoch: u32) -> TopologyEventBody {
    TopologyEventBody::BudgetExceeded {
        data: BudgetExceeded4 {
            epoch: Epoch(epoch),
            budget: BudgetKind::Run,
            limit_usd: 1.0,
            spent_usd: 2.0,
            key: Some(ALPHA),
        },
    }
}

fn run_finished(outcome: RunOutcome, halted_at: Option<TaskKey>) -> TopologyEventBody {
    TopologyEventBody::RunFinished {
        data: RunFinished4 {
            outcome,
            halted_at,
            merged: 0,
            parked: 0,
        },
    }
}

// ===========================================================================
// (b) — Complete or Halted
// ===========================================================================

/// A Halted run does not continue.
///
/// # About the word "finalizes" in this test's name
///
/// Step (b) is "terminal finalization **then** refuse continuation", and this
/// slice implements the refusal only: `RunDir.WriteReport` carries
/// `fault_row: t_finalize`, which is not one of PR7's eleven rows, so writing a
/// report here would be an out-of-row effect with no fault coverage in this
/// slice. The name is the packet's and is kept unchanged so the row and the
/// test still correspond; what it asserts is the half in range, and it asserts
/// the other half's **absence** explicitly rather than leaving it unstated —
/// no `report.json`, and no `RunDir.WriteReport`.
#[test]
fn resume_finalizes_halted_then_refuses() {
    for (tag, outcome, extra) in [
        (
            "halted",
            RunOutcome::Halted,
            vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Failed {
                            halts_run: true,
                            reason: "the ladder ran out".to_owned(),
                        },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
                run_finished(RunOutcome::Halted, Some(ALPHA)),
            ],
        ),
        (
            "complete",
            RunOutcome::Complete,
            vec![
                dispatched(),
                attempt_started(1),
                // `halts_run: false`: the task ends terminal and the run does
                // not halt, so the derived outcome is Complete rather than
                // Halted — which is what makes both arms of (b) constructible
                // without any integration terminal this slice does not
                // implement.
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Failed {
                            halts_run: false,
                            reason: "the ladder ran out and the policy does not halt".to_owned(),
                        },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
                run_finished(RunOutcome::Complete, None),
            ],
        ),
    ] {
        let fixture = Fixture::build(
            &format!("finished-{tag}"),
            Damage {
                extra,
                ..Damage::default()
            },
        );
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let before = fixture.log_bytes();
        let (result, _) = resume(&fixture, &harness, &given);

        let text = message(&result.expect_err("a finished run does not continue"));
        assert!(
            text.contains("already finished"),
            "the refusal says the run is over ({tag}): {text}"
        );
        assert!(
            text.contains(match outcome {
                RunOutcome::Halted => "halted",
                _ => "complete",
            }),
            "and names the outcome ({tag}): {text}"
        );
        assert_eq!(
            fixture.log_bytes(),
            before,
            "a (b) refusal appends nothing ({tag})"
        );
        assert!(
            !fixture.public().join("report.json").exists(),
            "PR7 does not finalize: `RunDir.WriteReport` is `t_finalize`, out of this slice's \
             rows ({tag})"
        );
        assert!(
            !harness
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .touched(EffectSiteId::RunDir(RunDirSite::WriteReport)),
            "and the report site never ran ({tag})"
        );
    }
}

// ===========================================================================
// (c) — the rebuild and its warnings
// ===========================================================================

/// A `[runner]` config that differs from the record **warns naming the
/// difference** and is ignored: the run resumes on its recorded runner.
///
/// Both halves asserted. A build that warned and then used today's config
/// would satisfy the warning half, and `run_resumed(4).runner` would then
/// differ from `run_started(4).runner` — which the fold refuses, but only if
/// the record actually reaches it.
#[test]
fn resume_rebuilds_runner_from_record_and_warns_on_config_drift() {
    let fixture = Fixture::healthy("config-drift");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let mut given = Given::healthy(&fixture, &runtime, &certifies);
    given.today.credential_volumes = [(AGENT.to_owned(), "somebody-elses-volume".to_owned())]
        .into_iter()
        .collect();
    runtime.add_volume("somebody-elses-volume");

    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a config that differs is a warning, not a refusal");

    assert!(
        recovered
            .warnings
            .iter()
            .any(|warning| warning.contains("credential volume set")),
        "the warning names which field differs: {:?}",
        recovered.warnings
    );
    // The record won: `run_resumed` carries the recorded volume, not today's.
    let log = String::from_utf8(fixture.log_bytes()).expect("the log is utf-8");
    let resumed = log.lines().last().expect("run_resumed is last");
    assert!(
        resumed.contains(VOLUME) && !resumed.contains("somebody-elses-volume"),
        "run_resumed records the recorded runner, not today's config: {resumed}"
    );
}

/// A recorded reference that now names another image warns, and the run keeps
/// running **from the recorded id**.
///
/// INV-23: "a moved reference cannot change what executes". The fake's mutable
/// tag table is what makes this constructible at all — the reference is moved
/// to a second image the runtime also holds, so the refusal path (an absent id)
/// is not what is being exercised.
#[test]
fn resume_warns_when_reference_moved_and_uses_recorded_image_id() {
    let fixture = Fixture::healthy("moved-reference");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let other = format!("sha256:{}", "3".repeat(64));
    runtime.add_image(&other, None);
    runtime.move_tag(IMAGE_REF, &other);
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a moved reference is a warning, not a refusal");

    assert!(
        recovered
            .warnings
            .iter()
            .any(|warning| warning.contains(IMAGE_REF) && warning.contains(&other)),
        "the warning names the reference and where it now points: {:?}",
        recovered.warnings
    );
    let log = String::from_utf8(fixture.log_bytes()).expect("the log is utf-8");
    let resumed = log.lines().last().expect("run_resumed is last");
    assert!(
        resumed.contains(IMAGE_ID) && !resumed.contains(&other),
        "the run continues from its recorded image id: {resumed}"
    );
}

/// An unavailable runtime, a recorded image id the runtime no longer holds, and
/// an absent credential volume each refuse **before any spawn**.
///
/// The predicate is the type, not the prose: `RunnerRebuilt::rebuild` runs
/// `rebuild_by_inspection`, and `PreflightCertified::certify` is the only thing
/// that spawns — so a refusal that produced no `RunnerRebuilt` cannot have
/// spawned. Asserted here through a pre-flight that would *panic* if it were
/// reached.
#[test]
fn resume_refuses_by_inspection_before_any_spawn_when_runtime_image_id_or_volume_absent() {
    /// A pre-flight that must never run. `certify` is unreachable if the
    /// inspection refusals really do precede every spawn, and this is what
    /// turns "unreachable" into a failing test rather than a comment.
    struct NeverRuns;

    impl RunnerPreflight for NeverRuns {
        fn certify(&self, _policy: &RunnerPolicy) -> Result<(), UpstrokeError> {
            unreachable!("an inspection refusal precedes every spawn");
        }
    }

    /// One way to leave a recorded runner un-re-establishable.
    type Damage = fn(&FakeRuntime);
    let cases: [(&str, Damage, &str); 3] = [
        (
            "runtime",
            |runtime| runtime.set_all_unreachable(),
            "cannot be reached",
        ),
        (
            "image-id",
            |runtime| runtime.move_tag(IMAGE_REF, "sha256:absent"),
            "no longer holds the recorded image id",
        ),
        (
            "volume",
            |runtime| runtime.remove_volume(VOLUME),
            "credential volume",
        ),
    ];
    for (tag, damage, expected) in cases {
        let fixture = Fixture::healthy(&format!("inspection-{tag}"));
        let harness = harness();
        let runtime = runtime_holding_the_record();
        if tag == "image-id" {
            // Remove the image itself: moving the tag alone leaves the id
            // present, and the id is what the rebuild asks about.
            runtime.add_image("sha256:absent", None);
        }
        damage(&runtime);
        if tag == "image-id" {
            let fresh = FakeRuntime::new(ContainerTrace::default());
            fresh.add_image("sha256:absent", None);
            fresh.tag(IMAGE_REF, "sha256:absent");
            fresh.add_volume(VOLUME);
            let never = NeverRuns;
            let mut given = Given::healthy(&fixture, &runtime, &never);
            given.runtime = &fresh;
            let (result, _) = resume(&fixture, &harness, &given);
            let text = message(&result.expect_err("an absent recorded id refuses"));
            assert!(
                text.contains(expected) && text.contains(IMAGE_ID),
                "the refusal names the recorded id ({tag}): {text}"
            );
            continue;
        }
        let never = NeverRuns;
        let given = Given::healthy(&fixture, &runtime, &never);
        let (result, _) = resume(&fixture, &harness, &given);
        let text = message(&result.expect_err("an inspection refusal"));
        assert!(
            text.contains(expected),
            "the refusal names what could not be re-established ({tag}): {text}"
        );
    }
}

// ---------------------------------------------------------------------------
// Driving only as far as the census
// ---------------------------------------------------------------------------

/// (a0) → (a) → (a1) → (a), stopping at the census, so a test can read what it
/// found. The full order consumes the witness at (h) and nothing survives it.
fn chain_to_census(
    fixture: &Fixture,
    harness: &Arc<Mutex<HookHarness>>,
    runtime: &dyn ContainerRuntime,
    incarnation: &IncarnationId,
) -> Result<ResumeCensused, UpstrokeError> {
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(harness));
    chain_to_census_with(fixture, &mut hooks, runtime, incarnation)
}

/// [`chain_to_census`], with the hook bundle supplied — so a test can arm one.
fn chain_to_census_with(
    fixture: &Fixture,
    hooks: &mut dyn TopologyHooks,
    runtime: &dyn ContainerRuntime,
    incarnation: &IncarnationId,
) -> Result<ResumeCensused, UpstrokeError> {
    let liveness = FakeOwnerLiveness::new();
    let view = DisposableDirView::new(ContainerTrace::default());
    let mut warnings = Vec::new();
    let root = fixture.derive(None)?;
    let locks = LocksHeld::take(root, &fixture.repo_root, &fixture.git_dir, hooks.rundir())?;
    let records = RecordsVerified::verify(locks, &fixture.repo_key)?;
    let log_path = records.locks().root().log_path();
    let committed = records.commit().run_started_sha256.clone();
    let prefix = crate::events::log::establish_stable_prefix(
        &log_path,
        fixture.inputs(),
        Some(&committed),
        &mut warnings,
        hooks.events(),
    )?;
    let barrier = BarrierHeld::from(records, prefix)?;
    ResumeCensused::census(
        barrier,
        &CensusSeams {
            incarnation,
            repo_root: &fixture.repo_root,
            repo_key: &fixture.repo_key,
            runtime,
            liveness: &liveness,
            view: &view,
        },
        hooks,
    )
}

// ===========================================================================
// (a) — the census, in the recorded root
// ===========================================================================

/// A run whose private root is not today's default still has its **earlier
/// incarnations'** containers reclaimed, and they are reclaimed **in the
/// recorded root**.
///
/// Three assertions, and the third is the one the test is named for: the
/// census's own report has to name the recorded root. A build that censused
/// `default_private_root()` would find nothing, reclaim nothing, and return a
/// perfectly successful report — so "the container was reclaimed" alone is not
/// enough; the root the census scanned is part of the claim.
#[test]
fn resume_of_nondefault_root_run_reclaims_earlier_incarnation_intents_in_recorded_root() {
    let fixture = Fixture::healthy("earlier-incarnation");
    let harness = harness();
    let runtime = runtime_holding_the_record();

    // An intent this run's *creator* incarnation left behind, in the recorded
    // root. It is dead by construction: the run lock is exclusive, so only one
    // incarnation of a run is ever live, and this process is a different one.
    let invocation = crate::runner::InvocationId::probe(
        crate::runner::ProbeTarget::Agent(crate::runner::AgentId::new(AGENT)),
        0,
    )
    .expect("the agent probe identity");
    let name = crate::runner::container::intent::ContainerName::new(
        fixture.repo_key.as_str(),
        RUN_ID,
        CREATOR,
        &invocation,
    )
    .expect("a container name for the creator incarnation");
    let record = crate::runner::container::intent::ContainerIntent::new(
        RUN_ID.to_owned(),
        &fixture.public(),
        CREATOR.to_owned(),
        fixture.repo_key.as_str().to_owned(),
        invocation.render(),
        crate::runner::policy::runner_policy_sha256(&fixture.started.runner),
    );
    let mut container_hooks = crate::runner::container::NoHooks;
    crate::runner::container::write_intent(
        &mut container_hooks,
        crate::topology::effects::ContainerSite::WriteIntent,
        &fixture.private_root,
        &name,
        &record,
    )
    .expect("the container funnel writes the intent");
    runtime.seed_container(
        name.as_str(),
        record.labels(&fixture.private_root),
        IMAGE_ID,
        IMAGE_ID,
        crate::runner::container::runtime::Liveness::Running,
    );

    let censused = chain_to_census(
        &fixture,
        &harness,
        &runtime,
        &IncarnationId(RESUMER.to_owned()),
    )
    .expect("the census completes");
    let report = censused.containers();

    assert_eq!(
        report.private_root, fixture.private_root,
        "the census scanned the recorded root, not today's default"
    );
    assert!(
        report
            .reclaimed
            .iter()
            .any(|entry| entry.name == name && entry.incarnation == CREATOR),
        "the creator incarnation's container is dead by construction and is reclaimed: {:?}",
        report.reclaimed
    );
    assert!(
        !runtime
            .container_names()
            .contains(&name.as_str().to_owned()),
        "and it is gone from the runtime"
    );
}

/// A resume **reclaims** the husks beside the run it is resuming: the private
/// half first, through the proof-token funnel, then the public directory with
/// the marker last.
///
/// `recovery_order` (a1)'s census is a "run-directory census incl. this run's
/// own stale marker, which the owner removes here, **and husk reclamation under
/// the ownership proof**", and INV-15 reclaims pre-run husks "at write-command
/// start under the worktree lock". A resume is a write command and holds that
/// lock. A run-directory pass that classified and reported would leave a
/// provable husk on disk for ever: every later resume would report it again, and
/// only a fresh `upstroke run` would ever reclaim it.
///
/// Three claims, and the third is what makes the first two mean anything:
///
/// * the provable husk is gone, both halves, and the report names the arm;
/// * `RunDir.RemovePrivateHusk` precedes `RunDir.RemovePublicHusk` — reversed, a
///   kill between the two leaves a private half no marker names and no later
///   census can ever prove;
/// * the husk carrying `committed.json` is byte-identical afterwards. A census
///   that deleted whatever it walked over would pass the first two.
#[test]
fn resume_reclaims_a_provable_husk_beside_the_run_and_retains_a_possibly_committed_one() {
    const RECLAIMED: &str = "01KZTHUSK00000000000000002";
    const RETAINED: &str = "01KZTKEEP00000000000000003";

    let fixture = Fixture::healthy("husk-beside");
    let harness = harness();
    let runtime = runtime_holding_the_record();

    let reclaimed = plant_husk(&fixture, RECLAIMED, false);
    let retained = plant_husk(&fixture, RETAINED, true);
    let retained_before = tree_bytes(&retained.private);
    assert!(
        !retained_before.is_empty(),
        "the retained husk must have a private half, or its comparison proves nothing"
    );

    let censused = chain_to_census(
        &fixture,
        &harness,
        &runtime,
        &IncarnationId(RESUMER.to_owned()),
    )
    .expect("the census completes");
    let report = censused.run_dirs();

    assert_eq!(
        report
            .of(RECLAIMED)
            .expect("the provable husk is censused")
            .outcome,
        RunDirOutcome::ReclaimedBothHalves,
        "a resume reclaims under the ownership proof; it does not merely report"
    );
    assert!(!reclaimed.private.exists(), "the private half is gone");
    assert!(!reclaimed.public.exists(), "and so is the public directory");

    let private_at = first_observation(
        &harness,
        EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
    )
    .expect("the private half went through the proof-token funnel");
    let public_at = first_observation(&harness, EffectSiteId::RunDir(RunDirSite::RemovePublicHusk))
        .expect("and the public directory through its own");
    assert!(
        private_at < public_at,
        "the private half first ({private_at}), the public directory with the marker last \
         ({public_at})"
    );

    assert_eq!(
        report
            .of(RETAINED)
            .expect("the retained husk is censused")
            .outcome,
        RunDirOutcome::Retained(RetainReason::PossiblyCommitted),
    );
    assert_eq!(
        tree_bytes(&retained.private),
        retained_before,
        "nothing private that carries a commit record is deleted by any census"
    );
    assert!(retained.public.exists(), "nor is its public half");

    // And the run being resumed: its own stale marker repaired by its owner, and
    // nothing else. The husk arms are gated on the run lock, which this process
    // holds for its own directory.
    assert_eq!(
        report
            .of(RUN_ID)
            .expect("the resuming run is censused too")
            .outcome,
        RunDirOutcome::RepairedStaleMarker,
    );
    assert!(!fixture.public().join(rundir::MARKER).exists());
    assert!(fixture.log().exists(), "and the run itself is untouched");
}

/// **A husk this resume cannot reclaim does not fail this resume.**
///
/// Before the census was shared, the resume's run-directory half was
/// `list_husks` + `husk_report` — both infallible — plus one `remove_marker`.
/// Sharing the reclaiming census moved a command-fatal error path onto the
/// resume: one dead run whose private half the filesystem will not release
/// (`EACCES`, `EPERM`, `EBUSY`, or on Windows any still-open handle) made
/// `upstroke resume <id>` fail for **every** run in the repository, on every
/// attempt, for a different run's residue. T-RESUME enumerates its refusals and
/// this is not among them, and `startup_census` and INV-15 answer "cannot be
/// reclaimed" with *retain and report* everywhere else.
///
/// The husk sorts **before** this run's id, which is what makes the second claim
/// worth making: `run_dir_names` sorts ascending, so a census that stopped at
/// the failure never reached this run's own directory at all — and recovery step
/// (a1) gives this run's stale-marker repair to its owner, which is this
/// process. So the repair was collateral damage of a different run's residue.
#[test]
fn resume_completes_past_a_husk_whose_private_half_cannot_be_removed() {
    const STUCK: &str = "01AAAASTUCK000000000000000";
    assert!(STUCK < RUN_ID, "the husk must sort before this run's id");

    let fixture = Fixture::healthy("husk-unreclaimable");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let stuck = plant_husk(&fixture, STUCK, false);
    let before = tree_bytes(&stuck.private);
    assert!(
        !before.is_empty(),
        "the husk must have a private half, or its comparison proves nothing"
    );
    assert!(
        fixture.public().join(rundir::MARKER).exists(),
        "this run's own stale marker must be there, or the second claim is vacuous"
    );

    let mut hooks = ArmedHooks::new(
        &harness,
        (RunDirSite::RemovePrivateHusk, HookPhase::Before, 1),
    );
    let (outcome, _) = resume_with(&fixture, &mut hooks, &given);

    outcome.expect("a husk beside the run cannot end the resume");

    // The husk: retained where it was, with the locator the next census needs.
    assert!(stuck.public.exists(), "the public half was removed anyway");
    assert!(
        stuck.public.join(rundir::MARKER).exists(),
        "`.creating` is the private half's only locator and it is gone"
    );
    assert_eq!(
        tree_bytes(&stuck.private),
        before,
        "the arming is `Before`, so the removal never ran"
    );

    // And this run's own stale marker, which sorts after the failure, was still
    // repaired by its owner.
    assert!(
        !fixture.public().join(rundir::MARKER).exists(),
        "the own-run stale-marker repair was skipped because a husk sorting \
         earlier could not be reclaimed"
    );
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        seen.touched(EffectSiteId::RunDir(RunDirSite::RemoveMarker)),
        "recovery step (a1)'s own repair never reached its funnel"
    );
    assert!(
        !seen.touched(EffectSiteId::RunDir(RunDirSite::RemovePublicHusk)),
        "the public half was removed after the private removal refused, which \
         orphans the private half permanently"
    );
}

/// The same husk, from the report's side: an entry naming the step that refused
/// and carrying its error, beside the own run's completed repair.
///
/// The sibling above asserts the **tree** and that the command survived; this
/// asserts that the census *said* what happened rather than merely surviving
/// it. INV-15's answer is retained **and reported**, and a census that swallowed
/// the failure into a `Skipped` or an `Ok` with no entry would pass the sibling.
#[test]
fn the_resume_census_reports_the_husk_it_could_not_reclaim() {
    const STUCK: &str = "01AAAASTUCK000000000000000";

    let fixture = Fixture::healthy("husk-unreclaimable-report");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let stuck = plant_husk(&fixture, STUCK, false);

    let mut hooks = ArmedHooks::new(
        &harness,
        (RunDirSite::RemovePrivateHusk, HookPhase::Before, 1),
    );
    let censused = chain_to_census_with(
        &fixture,
        &mut hooks,
        &runtime,
        &IncarnationId(RESUMER.to_owned()),
    )
    .expect("the census completes over a husk it could not reclaim");
    let report = censused.run_dirs();

    let entry = report.of(STUCK).expect("the husk is still an entry");
    let RunDirOutcome::Unreclaimable { step, detail } = &entry.outcome else {
        panic!("the failure is not an outcome: {:?}", entry.outcome);
    };
    assert_eq!(*step, FailedStep::PrivateHalf);
    assert!(!detail.is_empty(), "the error was dropped");
    assert!(
        !entry.outcome.deleted_a_private_half(),
        "a removal that returned an error claims the half is gone"
    );
    assert!(
        entry.outcome.may_have_deleted_a_private_half(),
        "a removal that may have emptied the tree reports it untouched"
    );
    assert_eq!(
        entry.locator.as_deref(),
        Some(stuck.private.as_path()),
        "retained and reported **with its locator**"
    );
    assert_eq!(report.unreclaimable().len(), 1);

    // And the entry after it: this run's own repair, performed.
    assert_eq!(
        report
            .of(RUN_ID)
            .expect("the resuming run is censused too")
            .outcome,
        RunDirOutcome::RepairedStaleMarker,
    );
}

// ===========================================================================
// (a) — the surviving reaper hold
// ===========================================================================

/// A resume refuses while a surviving reaper's shared cleanup hold (R28) is
/// observed, and succeeds once it is released.
///
/// The observation is [`rundir::observe_cleanup_hold`], which is fail-closed:
/// a `cleanup.lock` it cannot inspect is a hold, because "an observation that
/// was made to fail is not an observation that found nothing". A directory in
/// the lock file's place is exactly that state and is constructible on every
/// platform through the directory funnel, which is why it is what stands in for
/// a live reaper here — the alternative is `libc::flock`, which is on the
/// effect denylist and which this module may not reach.
///
/// The refusal half is `#[cfg(unix)]` because the hold is: R28 is "a surviving
/// **Unix** reaper's shared cleanup hold", and `rundir`'s non-Unix `cleanup`
/// module answers `false` unconditionally. The success half runs everywhere,
/// and asserts on both platforms that the observation site executed — a Windows
/// build that skipped the question entirely would pass a test that only
/// asserted the outcome.
#[test]
fn resume_refused_while_reaper_hold_observed_then_succeeds() {
    let fixture = Fixture::healthy("reaper-hold");

    #[cfg(unix)]
    {
        // Bound inside the `cfg`, because only the `cfg` uses it. Bound
        // outside, Windows compiles an unused local and CI's `lint (windows)`
        // leg refuses it under `-D warnings` — which is exactly the gap
        // recorded as `windows-gate-lint-level-gap`: a local
        // `--target x86_64-pc-windows-msvc` check accepts code the guest does
        // not, because only the guest sets the lint level.
        let cleanup = fixture.public().join("cleanup.lock");
        mkdir(&cleanup);
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);
        let (result, _) = resume(&fixture, &harness, &given);
        let text = message(&result.expect_err("a surviving reaper hold refuses"));
        assert!(
            text.contains("still cleaning agent processes"),
            "the refusal names the hold it observed: {text}"
        );
        assert!(
            harness
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .observed(
                    EffectSiteId::Lock(LockSite::ObserveCleanupHold),
                    HookPhase::Before
                ),
            "R28 is observed, never owned — and the site says so"
        );
        rundir::remove_public_husk(&cleanup, &mut NoHooks).expect("the reaper released its hold");
    }

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    result.expect("with no hold observed, the resume proceeds");
    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        seen.observed(
            EffectSiteId::Lock(LockSite::ObserveCleanupHold),
            HookPhase::Before
        ),
        "the hold is observed on every resume, not only when one is held"
    );
    assert!(
        seen.observed(
            EffectSiteId::Lock(LockSite::AcquireWorktree),
            HookPhase::Before
        ) && seen.observed(EffectSiteId::Lock(LockSite::AcquireRun), HookPhase::Before),
        "and both R17 holds were taken"
    );
}

// ===========================================================================
// (d), (e), (h)
// ===========================================================================

/// Replay the fixture's log from disk, which is the only way to read state a
/// resume left behind: `run_resumed` consumes the witness that carried the
/// live fold.
///
/// Replaying rather than keeping the live fold is also the stronger assertion.
/// INV-02's "live state and replay use one checked transition over the exact
/// wire event" means a claim made against the replayed fold is a claim about
/// the bytes, not about a `TopologyFold` this process happens to hold.
fn replayed(fixture: &Fixture) -> TopologyFold {
    let bytes = fixture.log_bytes();
    let events = TopologyFold::parse_log(&bytes).expect("the log parses");
    TopologyFold::replay(fixture.inputs(), &events).expect("and folds")
}

/// A resume clears the previous epoch's budget stop and wakes every Deferred
/// task.
///
/// Both halves, and both read off the **replayed** log rather than off the
/// return value: the epoch-scoped stop is what makes "raise the ceiling and
/// resume" the answer to a budget stop, and a build that cleared it only in
/// memory would leave the next process refusing for a stop the log still
/// carries.
#[test]
fn resume_clears_budget_stop_and_wakes_deferred() {
    let fixture = Fixture::build(
        "budget-stop",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Deferred {
                            defers: 1,
                            reason: "the pool was exhausted".to_owned(),
                        },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
                budget_exceeded(0),
            ],
            ..Damage::default()
        },
    );

    let before = replayed(&fixture);
    assert!(
        before.budget_stop().is_some(),
        "the fixture must carry a stop, or this test proves nothing"
    );
    assert_eq!(
        before.task_state(ALPHA),
        Some(TaskState::Deferred),
        "and a deferred task"
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a budget-stopped run resumes");

    assert!(recovered.resumed.budget_stop_cleared);
    assert_eq!(
        recovered.resumed.epoch, 1,
        "the resume opens the next epoch"
    );
    let after = replayed(&fixture);
    assert!(
        after.budget_stop().is_none(),
        "the stop belongs to the epoch that hit the old ceiling"
    );
    assert_eq!(
        after.task_state(ALPHA),
        Some(TaskState::Pending),
        "and every Deferred task is woken by the resume"
    );
}

/// **Steps (d) and (e) handle every entry, not the first one.**
///
/// Two catalogue entries survived the whole suite at `6a21be6` for one reason —
/// no fixture had a second thing for these loops to reach:
///
/// - `PR7-PIPELINE-010` reduced step (e) to
///   `retained_idle(..).into_iter().take(1)`, closing only the first
///   `RetainedIdle` generation. Green.
/// - `PR7-PIPELINE-008` added `if lease == LineageHeld { continue; }` to step
///   (d)'s loop, skipping a whole lease class. Green.
///
/// Both loops were already correct. What was missing was a fixture that could
/// tell a loop from a `.first()`, which is why this is a witness and not a
/// repair. `Damage::two_tasks` registers `beta` beside `alpha` so there are two
/// of everything for the steps to walk.
///
/// **Live above `max_parallel = 1`, latent at it** — which is exactly the
/// condition a carried row would have named. It is cheaper to hold it than to
/// write it down: PR11 inherits a substrate whose recovery loops are witnessed
/// rather than a note saying they are not.
#[test]
fn steps_d_and_e_reach_every_generation_not_the_first() {
    const BETA: TaskKey = TaskKey(1);

    let fixture = Fixture::build(
        "loops-reach-every",
        Damage {
            two_tasks: true,
            extra: vec![
                // alpha: retained and idle — step (e)'s subject.
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Retained {
                        retained_session: SessionId("alpha-session".to_owned()),
                        retained_incarnation: Epoch(0),
                    },
                ),
                // beta: the same, and the second entry the loop must reach.
                for_task(BETA, "beta", dispatched()),
                for_task(BETA, "beta", attempt_started(1)),
                for_task(
                    BETA,
                    "beta",
                    attempt_finished(
                        1,
                        AttemptSettlement::Retained {
                            retained_session: SessionId("beta-session".to_owned()),
                            retained_incarnation: Epoch(0),
                        },
                    ),
                ),
            ],
            ..Damage::default()
        },
    );

    // The premise: two retained generations before the resume. Without this the
    // assertion below is satisfied by a fixture that only ever had one.
    let before = replayed(&fixture);
    assert!(
        before.ready_retry(ALPHA) && before.ready_retry(BETA),
        "both tasks must be retryable before the resume, or a `.take(1)` would \
         pass this test by accident"
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a run with two retained sessions resumes");

    assert_eq!(
        recovered.retained_closed, 2,
        "step (e) closed {} of two retained generations — a loop that stops at \
         the first leaves the rest holding their entitlements for the whole run",
        recovered.retained_closed
    );

    let after = replayed(&fixture);
    for (key, name) in [(ALPHA, "alpha"), (BETA, "beta")] {
        assert!(
            !after.ready_retry(key),
            "{name}'s retained generation survived the resume"
        );
    }
}

/// A retained session belongs to the incarnation that retained it. Step (e)
/// closes the generation, so after the resume there is no retry to evaluate —
/// and the fold refuses one.
///
/// `recovery_order` (i): "`ready_retry` is never evaluated before (h) and the
/// fold refuses a stale-incarnation retry". The first clause is structural
/// here: nothing in this file evaluates `ready_retry`, and the loop that does
/// is behind `run_resumed`, which consumes the witness. The second is asserted
/// directly, against the replayed fold.
#[test]
fn retry_refused_after_resume() {
    let fixture = Fixture::build(
        "retained-retry",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Retained {
                        retained_session: SessionId("session-of-the-dead-incarnation".to_owned()),
                        retained_incarnation: Epoch(0),
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let before = replayed(&fixture);
    assert!(
        before.ready_retry(ALPHA),
        "before the resume the retained generation is retryable, or this test proves nothing"
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("a run with a retained session resumes");

    assert_eq!(
        recovered.retained_closed, 1,
        "step (e) closes every RetainedIdle generation"
    );
    let after = replayed(&fixture);
    assert!(
        !after.ready_retry(ALPHA),
        "the retained session is gone, so there is no same-session retry to take"
    );
    // And the transition itself is refused: a forged retry into the closed
    // generation does not plan.
    let refused = after
        .plan_transition(&event(attempt_started(2)))
        .expect_err("a retry into a closed generation is refused");
    assert!(
        format!("{refused}").contains("generation"),
        "the refusal is about the generation: {refused}"
    );
}

/// `run_resumed(4).runner` equals `run_started(4).runner` field for field.
///
/// Read off the log rather than off the value this process passed in, and
/// compared with `RunnerPolicy::difference` — which names which field moved —
/// rather than with `assert_eq!`, so the failure message is the field rather
/// than two pretty-printed records.
#[test]
fn run_resumed_records_identical_runner_identity() {
    let fixture = Fixture::healthy("identical-runner");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (result, _) = resume(&fixture, &harness, &given);
    result.expect("the healthy resume completes");

    let bytes = fixture.log_bytes();
    let events = TopologyFold::parse_log(&bytes).expect("the log parses");
    let resumed = events
        .iter()
        .rev()
        .find_map(|event| match &event.body {
            TopologyEventBody::RunResumed { data } => Some(data.clone()),
            _ => None,
        })
        .expect("the log ends with a run_resumed");

    assert_eq!(
        fixture.started.runner.difference(&resumed.runner),
        None,
        "the incarnation established exactly the recorded runner"
    );
    assert_eq!(resumed.incarnation.0, RESUMER, "and recorded its own id");
    assert_eq!(
        resumed.probed_agents, fixture.started.probed_agents,
        "and the agents its pre-flight certified"
    );
}

/// A `run_resumed` whose runner differs from `run_started`'s is refused **on
/// replay**, not merely at the point it would be written.
///
/// The forged line is appended straight through the Event funnel, which is
/// exactly what a hand-edited log or a hostile process would produce: the fold
/// never saw it. So the refusal has to come from the reader, and it does — the
/// barrier's checked replay refuses the whole prefix, which is what stops a
/// forged identity from authorizing anything.
#[test]
fn forged_run_resumed_with_different_runner_identity_refused_on_replay() {
    let fixture = Fixture::healthy("forged-runner");
    let mut forged = fixture.started.runner.clone();
    if let Some(image) = forged.image.as_mut() {
        image.id = format!("sha256:{}", "9".repeat(64));
    }
    let mut warnings = Vec::new();
    let mut log =
        EventLog::open(EventSite::OpenLog, &fixture.log(), &mut warnings).expect("the log reopens");
    let (line, _) = TopologyLine::round_trip(&event(TopologyEventBody::RunResumed {
        data: Box::new(RunResumed4 {
            incarnation: IncarnationId(RESUMER.to_owned()),
            runner: forged,
            probed_agents: vec![AGENT.to_owned()],
            upstroke_version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
    }))
    .expect("the forged event serializes — the wire format is not the check");
    log.append_topology(EventSite::Append, &line)
        .expect("nothing stops a forged line reaching the file");
    drop(log);

    let bytes = fixture.log_bytes();
    let events = TopologyFold::parse_log(&bytes).expect("the forged log still parses");
    let error = TopologyFold::replay(fixture.inputs(), &events)
        .expect_err("the checked fold refuses the forged identity");
    assert!(
        format!("{error}").contains("image id"),
        "the refusal names which field moved: {error}"
    );

    // And a resume over that prefix refuses at the barrier, before anything.
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let text = message(&result.expect_err("the barrier refuses a forged prefix"));
    assert!(
        text.contains(BarrierStep::CheckedReplay.name()),
        "the refusal names the barrier step: {text}"
    );
}

/// An append that returns an error ends the command, and the **next** resume
/// establishes the barrier over whichever prefix survived and continues from
/// it.
///
/// The injection is at `Synced`, which is the case where the line is on disk
/// and the process cannot tell whether it is durable. `append_error_protocol`:
/// "the event is outcome-unknown; `apply_delta` is not run and the in-memory
/// fold is marked poisoned … the append is never retried … the run is
/// NoRunFinished and resumable and the next resume follows the fault row of the
/// surviving prefix (T-APPEND) only after its own barrier".
///
/// So: the first resume fails with the line present, and the second resume sees
/// a prefix ending in `run_resumed` and opens the epoch after it. Two
/// `run_resumed` lines is the correct convergence for the after-append order,
/// not a duplicate.
#[test]
fn resume_after_append_error_follows_surviving_prefix() {
    let fixture = Fixture::healthy("append-error");
    let first = harness();
    first
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::Append),
            SubEffectPoint::Synced,
            InjectionMode::ErrorReturn,
        )
        .expect("the Synced point supports an error return");
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let lines_before = fixture.log_bytes().iter().filter(|b| **b == b'\n').count();
    let (result, _) = resume(&fixture, &first, &given);
    let text = message(&result.expect_err("an errored append ends the command"));
    assert!(
        text.contains(crate::events::log::INJECTED_PREFIX),
        "the error is the funnel's own: {text}"
    );
    // **The append is never retried.** A second attempt through the same handle
    // would come back as the *poison* error rather than the injected one — the
    // funnel poisons the handle at the point that failed — so the error the
    // command ends with is what tells a retry from an end. `INJECTED_PREFIX`
    // present and `POISONED_PREFIX` absent is that distinction, and it is the
    // only observable one: a retry cannot succeed through a poisoned handle, so
    // the line count is the same either way.
    assert!(
        !text.contains(crate::events::log::POISONED_PREFIX),
        "the command ended at the errored append; it did not attempt a second one: {text}"
    );
    let lines_after = fixture.log_bytes().iter().filter(|b| **b == b'\n').count();
    assert_eq!(
        lines_after,
        lines_before + 1,
        "the line is durable — this is the after-append order of T-APPEND (e-s)"
    );

    // **The protocol ran, and its report is what the command ends with.**
    // Everything above this point is true of a build that merely poisoned the
    // fold and returned the funnel's error, which is why none of it can stand
    // for `append_error_protocol`. Obligation (5) is the observable one: reopen
    // through `Event.OpenLog` (torn-tail normalization), establish the
    // stable-prefix barrier, and end "naming the run id, the event kind, and
    // whether the proven prefix contains the line".
    assert!(text.contains(RUN_ID), "the report names the run: {text}");
    assert!(
        text.contains("run_resumed"),
        "and the event kind whose outcome is unknown: {text}"
    );
    assert!(
        text.contains("Event.Append"),
        "and the site it was filed at: {text}"
    );
    assert!(
        text.contains("the proven prefix contains the line"),
        "and whether the proven prefix contains the line. Present here, and asserted as the \
         sentence rather than as \"some outcome\": the injection is at `Synced`, after the bytes \
         reached the file, so a protocol that reported `absent` would be wrong in the direction \
         that loses a durable transition: {text}"
    );
    assert!(
        text.contains("resumable"),
        "and the run is reported resumable, which is what makes ending here safe: {text}"
    );

    let seen = first.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(
        seen.count(EffectSiteId::Event(EventSite::OpenLog), HookPhase::Before),
        2,
        "`Event.OpenLog` twice: recovery step (a1)'s barrier, then the protocol's reopen after \
         the failed append. Once means no reopen happened and the outcome was never established."
    );
    assert_eq!(
        seen.count(
            EffectSiteId::Event(EventSite::ProvePrefixStable),
            HookPhase::Before
        ),
        2,
        "and the stable-prefix barrier is re-established over the reopened log before anything is \
         reported"
    );
    assert_eq!(
        seen.count(EffectSiteId::Event(EventSite::Append), HookPhase::Before),
        1,
        "and the append itself is never retried"
    );
    drop(seen);

    // The next resume: a fresh harness, nothing armed, and it follows the
    // surviving prefix.
    let second = harness();
    let runtime = runtime_holding_the_record();
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &second, &given);
    let recovered = result.expect("the next resume establishes its own barrier and continues");
    assert_eq!(
        recovered.resumed.epoch, 2,
        "the surviving prefix already carried one resume, so this is the second epoch"
    );
    assert!(
        first_observation(&second, EffectSiteId::Event(EventSite::ProvePrefixStable)).is_some(),
        "and it proved the prefix before acting on it"
    );
}

/// An outcome-unknown append during recovery cancels the provisional
/// reservation and every still-running invocation.
///
/// `append_error_protocol` obligations (2) and (3):
/// [`Reservations::cancel_any`] — `permits`: "cancellation on any pre-append
/// failure, run end, shutdown, or a poisoned fold" — and
/// [`InvocationLedger::cancel_all_running`], the ledger half of "in-flight
/// invocations are cancelled through the Runner".
///
/// The recovery order's own ledgers are empty, so on that path both obligations
/// are satisfied vacuously and no test of `resume` could tell a build that ran
/// them from one that did not. So this test hands the emitter ledgers that are
/// **not** empty — one held reservation, one registered running invocation —
/// which is exactly why they are `EmitContext` fields rather than locals inside
/// the recovery order. Both ledgers balance afterwards: every entry settled
/// exactly once, which is the process-end condition R4 states.
#[test]
fn an_append_error_during_recovery_cancels_the_reservation_and_every_running_invocation() {
    let fixture = Fixture::healthy("append-error-ledgers");
    let harness = harness();
    harness
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::Append),
            SubEffectPoint::Synced,
            InjectionMode::ErrorReturn,
        )
        .expect("the Synced point supports an error return");
    let runtime = runtime_holding_the_record();
    let incarnation = IncarnationId(RESUMER.to_owned());

    let censused =
        chain_to_census(&fixture, &harness, &runtime, &incarnation).expect("the census completes");
    let rebuilt = RunnerRebuilt::rebuild(censused, &container_selection(), Some(&runtime))
        .expect("the recorded runner rebuilds by inspection");
    let certified =
        PreflightCertified::certify(rebuilt, &AlwaysCertifies).expect("the pre-flight certifies");

    let mut reservations = Reservations::new();
    reservations
        .take(ALPHA, ReservationKind::Dispatch)
        .expect("a provisional reservation is held");
    let mut invocations = InvocationLedger::new();
    let invocation = crate::runner::InvocationId::probe(crate::runner::ProbeTarget::Shell, 11)
        .expect("an invocation identity");
    invocations
        .register(&invocation)
        .expect("and one invocation is running");
    assert!(
        !reservations.is_empty() && invocations.running().len() == 1,
        "the ledgers must be non-empty, or this test proves nothing"
    );

    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let mut warnings = Vec::new();
    let mut context = EmitContext {
        clock: &Frozen,
        hooks: &mut hooks,
        inputs: fixture.inputs(),
        reservations: &mut reservations,
        invocations: &mut invocations,
        warnings: &mut warnings,
    };
    let error = run_resumed(certified, &mut context, &incarnation)
        .expect_err("the injected append error ends the command");
    let text = message(&error);
    assert!(
        text.contains(crate::events::log::INJECTED_PREFIX),
        "the report carries the funnel's own error as its cause: {text}"
    );

    assert!(
        reservations.is_empty(),
        "obligation (2): whatever reservation was held is cancelled"
    );
    assert!(
        reservations.balances(),
        "and the reservation ledger balances — taken once, cancelled once"
    );
    assert_eq!(
        invocations.cancelled(),
        1,
        "obligation (3): every still-running invocation is cancelled"
    );
    assert!(
        invocations.running().is_empty() && invocations.balances(),
        "and the invocation ledger balances: no entry is left running"
    );
}

// ===========================================================================
// (c) — the RunnerPreflight probes
// ===========================================================================

/// The real pre-flight, over a runner that answers every process.
fn real_preflight<'a>(
    runner: &'a dyn Runner,
    adapters: &'a StubAdapters,
    fixture: &Fixture,
) -> RunPreflight<'a> {
    RunPreflight::new(
        runner,
        adapters,
        ShellKind::Bash,
        &fixture.repo_root,
        fixture.started.probed_agents.clone(),
    )
}

/// A failing shell, and a failing agent CLI, each refuse **before any recovery
/// event**.
///
/// Two cases and not one, because they are two different processes with two
/// different accountings: the shell probe is non-slotted and the agent probe
/// takes a slot pair. A build that refused correctly on one could hold a slot
/// forever on the other, so both assert the ledgers as well as the refusal.
#[test]
fn resume_refuses_by_preflight_probe_when_shell_or_cli_fails_before_any_recovery_event() {
    for (tag, program, expected) in [
        ("shell", "bash", "the recorded shell"),
        ("cli", "claude", "the `claude-code` CLI"),
    ] {
        let fixture = Fixture::healthy(&format!("probe-{tag}"));
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let runner = RecordingRunner::failing(program);
        let adapters = StubAdapters;
        let preflight = real_preflight(&runner, &adapters, &fixture);
        let given = Given::healthy(&fixture, &runtime, &preflight);

        let before = fixture.log_bytes();
        let (result, _) = resume(&fixture, &harness, &given);

        let text = message(&result.expect_err("a failing probe refuses"));
        assert!(
            text.contains(expected),
            "the refusal names what did not answer ({tag}): {text}"
        );
        assert!(
            text.contains(IMAGE_REF),
            "and the image it was probed inside ({tag}): {text}"
        );
        assert_eq!(
            fixture.log_bytes(),
            before,
            "a probe refusal precedes every recovery event ({tag})"
        );
        assert!(
            preflight.ledgers_balance(),
            "every probe invocation is settled and every slot released ({tag}); still running: \
             {:?}",
            preflight.running()
        );
        // The shell probe fails first, so the agent CLI is never asked. That is
        // the sequence `runner` states — "probes execute through it
        // sequentially at pre-flight" — and it is what makes the shell the
        // cheaper refusal.
        let programs: Vec<String> = runner
            .requests()
            .into_iter()
            .map(|request| request.command.program)
            .collect();
        if tag == "shell" {
            assert_eq!(
                programs,
                vec!["bash".to_owned()],
                "no agent is probed after the shell fails"
            );
        } else {
            assert_eq!(
                programs,
                vec!["bash".to_owned(), "claude".to_owned()],
                "the shell probe runs first and the agent probe second"
            );
        }
    }
}

/// Every process-local ledger is empty after a resume, and the shell probe took
/// no slot while the agent probe did.
///
/// `crash_reconstruction` requires "provisional reservations, slot table,
/// invocation ledger, and the coordinator's own lock holds are empty at process
/// start", and the resume path is what has to leave them that way. The
/// asymmetry is asserted from the recorded requests rather than from the
/// ledger's totals, because "one slot was taken" is true of a build that took
/// it for the wrong process.
#[test]
fn ledgers_empty_after_resume() {
    let fixture = Fixture::healthy("ledgers-empty");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let runner = RecordingRunner::default();
    let adapters = StubAdapters;
    let preflight = real_preflight(&runner, &adapters, &fixture);
    let given = Given::healthy(&fixture, &runtime, &preflight);

    let (result, _) = resume(&fixture, &harness, &given);
    result.expect("the healthy resume completes");

    assert!(
        preflight.ledgers_balance(),
        "R3 and R4 balance at the end of the pre-flight"
    );
    assert!(
        preflight.running().is_empty(),
        "no invocation is still registered as running: {:?}",
        preflight.running()
    );
    let roles: Vec<(String, bool)> = runner
        .requests()
        .into_iter()
        .map(|request| {
            (
                request.command.program.clone(),
                crate::engine::topology::identity::is_slotted(&request.invocation),
            )
        })
        .collect();
    assert_eq!(
        roles,
        vec![("bash".to_owned(), false), ("claude".to_owned(), true)],
        "the shell probe is non-slotted and the agent probe is slotted"
    );
    // And the process-local ledgers a fresh coordinator starts with are empty
    // by construction, which is the other half of the row.
    assert!(crate::engine::topology::identity::Reservations::new().is_empty());
    assert!(crate::engine::topology::identity::SlotAssertion::new().is_empty());
    assert!(crate::engine::topology::identity::InvocationLedger::new().balances());
}

/// A `Runner` that gives every probe a real container through the container
/// funnel, and releases it on both paths.
///
/// This is the shape `ContainerRunner::run` has — `launch` then `release`,
/// with the release running whether or not the invocation succeeded — driven
/// against the fake runtime so a test can read what survived. Built here rather
/// than reused because `ContainerRunner` owns its runtime by value and hands
/// back no way to inspect it, and because the four effectful `ContainerRuntime`
/// methods are on the effect denylist for every module but the funnel — so a
/// delegating wrapper around the fake is not something this module may write.
struct ProbeContainerRunner<'a> {
    runtime: &'a dyn ContainerRuntime,
    private_root: PathBuf,
    run_dir: PathBuf,
    repo_key: String,
    incarnation: String,
    policy_digest: String,
    /// The program whose container exits non-zero.
    failing: String,
}

impl Runner for ProbeContainerRunner<'_> {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        use crate::runner::container::intent::{ContainerIntent, ContainerName};
        use crate::runner::container::runtime::CreateSpec;
        use crate::runner::container::{
            GitViewRequest, NoHooks as ContainerNoHooks, launch, release,
        };

        let name = ContainerName::new(
            &self.repo_key,
            RUN_ID,
            &self.incarnation,
            &request.invocation,
        )?;
        let intent = ContainerIntent::new(
            RUN_ID.to_owned(),
            &self.run_dir,
            self.incarnation.clone(),
            self.repo_key.clone(),
            request.invocation.render(),
            self.policy_digest.clone(),
        );
        let plan = crate::runner::container::LaunchPlan {
            private_root: self.private_root.clone(),
            name: name.clone(),
            invocation: request.invocation.clone(),
            intent: intent.clone(),
            spec: CreateSpec {
                name: name.as_str().to_owned(),
                image_id: IMAGE_ID.to_owned(),
                labels: intent.labels(&self.private_root),
                mounts: Vec::new(),
                env: Vec::new(),
                command: std::iter::once(request.command.program.clone())
                    .chain(request.command.args.iter().cloned())
                    .collect(),
                workdir: Some("/".to_owned()),
                read_only_root: true,
            },
            view: GitViewRequest {
                path: crate::runner::container::exec::view_dir(&self.private_root, &name),
                workspace: request.workspace.clone(),
                head: None,
            },
        };
        let mut hooks = ContainerNoHooks;
        let view = DisposableDirView::new(ContainerTrace::off());
        let launched = launch(&mut hooks, self.runtime, &view, &plan)?;
        let code = if request.command.program == self.failing {
            127
        } else {
            0
        };
        // Released on both paths: R26 is "released on complete …, cancel, or
        // shutdown" and R19's view is "pruned on complete or cancel".
        release(
            &mut hooks,
            self.runtime,
            &view,
            &self.private_root,
            &launched,
        )?;
        Ok(ProcessOutput {
            code: Some(code),
            stdout: String::new(),
            stderr: "the recorded shell is not in this image".to_owned(),
            duration: Duration::from_millis(1),
            timed_out: false,
            output_limited: false,
        })
    }
}

/// After a pre-flight refusal, the probe containers are reclaimed: no
/// container, no intent, no Git view survives.
///
/// `expected_failures_refusals[2]` ends "…refuses before any recovery event or
/// work spawn, **the probe containers reclaimed**", and R19/R26 both say
/// "pruned/released on complete **or cancel**". A refusal is a cancel, so the
/// namespace has to be empty afterwards — otherwise the next write command's
/// census finds residue from a command that never started.
#[test]
fn resume_preflight_probe_containers_reclaimed_after_refusal() {
    let fixture = Fixture::healthy("probe-reclaim");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let runner = ProbeContainerRunner {
        runtime: &runtime,
        private_root: fixture.private_root.clone(),
        run_dir: fixture.public(),
        repo_key: fixture.repo_key.as_str().to_owned(),
        incarnation: RESUMER.to_owned(),
        policy_digest: crate::runner::policy::runner_policy_sha256(&fixture.started.runner),
        failing: "bash".to_owned(),
    };
    let adapters = StubAdapters;
    let preflight = real_preflight(&runner, &adapters, &fixture);
    let given = Given::healthy(&fixture, &runtime, &preflight);

    let before = fixture.log_bytes();
    let (result, _) = resume(&fixture, &harness, &given);

    let text = message(&result.expect_err("a shell that fails inside the image refuses"));
    assert!(text.contains("the recorded shell"), "{text}");
    assert_eq!(
        fixture.log_bytes(),
        before,
        "the refusal precedes every recovery event"
    );
    assert!(
        runtime.container_names().is_empty(),
        "every probe container is reclaimed: {:?}",
        runtime.container_names()
    );
    assert!(
        crate::runner::container::list_intents(&fixture.private_root)
            .expect("the namespace scans")
            .is_empty(),
        "and its intent went with it"
    );
    assert!(
        preflight.ledgers_balance(),
        "and the probe invocations are settled: {:?}",
        preflight.running()
    );
}

// ===========================================================================
// T-RUNSTART's P7/P8 repair
// ===========================================================================

/// The `Ref.CreateIntegration` funnel's `Before` count, which is what "the
/// funnel was entered" means everywhere below.
///
/// Counted rather than tested for presence: "no spend repeats" is a claim about
/// *how many times* the effect ran, and `touched` would be green for a build
/// that created the ref, then created it again.
fn create_ref_entries(harness: &Arc<Mutex<HookHarness>>) -> u32 {
    harness
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .count(
            EffectSiteId::Ref(RefSite::CreateIntegration),
            HookPhase::Before,
        )
}

/// `transaction_fault_matrix[T-RUNSTART].resume_action`, first clause:
/// "**P7/P8: create the ref zero-old at the recorded base if absent**".
///
/// The fixture *is* the prefix a kill between P6 and P8 leaves —
/// `run_started(4)` durable, `committed.json` naming its digest, the creator's
/// `.creating` still on disk because P7 never ran — and the ref namespace is
/// empty. A resume over it must leave the ref there.
///
/// # What this asserts that calling the function could not
///
/// This test used to live in `create::tests` and called
/// [`super::super::create::ensure_integration_ref`] directly with two literals.
/// That proved the *function* creates a ref, which was never in doubt. Driving
/// [`run_recovery_order`] proves the three things that actually were:
///
/// 1. **that the recovery order calls it at all** — its only production caller
///    used to be P8, so a run killed between P6 and P8 resumed with no ref and
///    nothing to create one;
/// 2. **with the recorded arguments** — asserted against
///    `fixture.started.integration_ref` and `fixture.started.base_sha` rather
///    than against constants, so a resume that published today's configured ref
///    name, or the fold's current head, fails here;
/// 3. **at a point before any recovery event** — the funnel snapshots the log
///    on entry, and the bytes it saw are compared against the committed prefix.
#[test]
fn kill_after_run_started_creates_integration_ref() {
    let fixture = Fixture::healthy("ref-p78-create");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let committed = fixture.log_bytes();
    assert_eq!(
        given.refs.target(),
        None,
        "the fixture is the P6/P7 prefix: nothing created the ref"
    );

    let (result, _) = resume(&fixture, &harness, &given);
    result.expect("a resume of a run killed before P8 completes");

    assert_eq!(
        given.refs.created(),
        vec![(
            fixture.started.integration_ref.as_str().to_owned(),
            fixture.started.base_sha.as_str().to_owned(),
        )],
        "the ref is created once, at the name and base the record carries"
    );
    assert_eq!(
        create_ref_entries(&harness),
        1,
        "and the funnel was entered exactly once"
    );

    // The position claim, read off the effect itself rather than off an index:
    // when `Ref.CreateIntegration` ran, the log was still exactly the prefix
    // the creator committed — no `attempt_interrupted`, no `generation_closed`,
    // no `run_resumed`.
    assert_eq!(
        given.refs.log_kinds_at_entries(),
        vec![vec!["run_started".to_owned()]],
        "the ref was created after a recovery event had already been appended"
    );
    assert_eq!(
        given.refs.log_bytes_at_entries(),
        vec![committed.clone()],
        "the log the funnel saw was not byte-identical to the committed prefix"
    );
    // And the appends did happen — otherwise the assertion above is green for a
    // resume that never got as far as (d)–(h) at all.
    let after = fixture.log_bytes();
    assert!(
        after.len() > committed.len()
            && String::from_utf8_lossy(&after).contains("\"run_resumed\""),
        "the resume did not reach (h), so `before any recovery event` proves nothing"
    );
}

/// The second clause: "**if present == base continue (no spend repeats)**".
///
/// Two ways in, because they fail differently. A resume that *finds* the ref
/// already at the recorded base is the ordinary case — some other process, or
/// an earlier resume, got there first. A **second** resume of the same run is
/// the idempotence case, and it is the one that would catch a step that
/// remembered nothing and re-pointed the ref every time.
///
/// Both assert the funnel's entry count and not the command's exit status: an
/// implementation that called `create_zero_old` again would get an `Err` back
/// from Git ("already exists; zero-old refuses"), and a build that swallowed it
/// would be green on `result.is_ok()` while having repeated the spend.
#[test]
fn a_resume_adopts_an_integration_ref_already_at_the_recorded_base() {
    // (1) Already there when the resume arrives.
    {
        let fixture = Fixture::healthy("ref-p78-adopt");
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let mut given = Given::healthy(&fixture, &runtime, &certifies);
        given.refs = RecordingRefs::at(&fixture, fixture.started.base_sha.as_str());

        let (result, _) = resume(&fixture, &harness, &given);
        result.expect("present == base continues");

        assert_eq!(
            create_ref_entries(&harness),
            0,
            "the funnel was entered for a ref that was already at the base"
        );
        assert!(
            given.refs.created().is_empty(),
            "and nothing was created: {:?}",
            given.refs.created()
        );
        assert_eq!(
            given.refs.target().as_deref(),
            Some(fixture.started.base_sha.as_str()),
            "the ref still names the recorded base"
        );
    }

    // (2) Two resumes of one run: the second adopts what the first created.
    {
        let fixture = Fixture::healthy("ref-p78-twice");
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let first = harness();
        let (result, _) = resume(&fixture, &first, &given);
        let opened = result.expect("the first resume completes").resumed.epoch;
        assert_eq!(create_ref_entries(&first), 1, "the first resume creates it");

        let second = harness();
        let (result, _) = resume(&fixture, &second, &given);
        let reopened = result.expect("the second resume completes").resumed.epoch;

        assert_eq!(
            create_ref_entries(&second),
            0,
            "the second resume entered `Ref.CreateIntegration` again; `no spend repeats` is not \
             held"
        );
        assert_eq!(
            given.refs.created().len(),
            1,
            "the ref was created twice: {:?}",
            given.refs.created()
        );
        assert!(
            reopened > opened,
            "the second resume did not open an epoch of its own ({opened} then {reopened}), so \
             it never reached the step this test is about"
        );
    }
}

/// A ref at any other SHA refuses — and refuses **before anything the step
/// would otherwise have done**.
///
/// `ensure_integration_ref`'s third disposition. "It refused" is the weak half
/// of the claim; the load-bearing half is that the refusal costs nothing:
/// `Ref.CreateIntegration` is never entered, the ref keeps the target it had,
/// and the log is byte-identical to the prefix the resume started from. A ref
/// that already names another commit belongs to something else, and a run is
/// never made room for by moving it.
#[test]
fn a_resume_refuses_an_integration_ref_at_another_sha_before_touching_anything() {
    let fixture = Fixture::healthy("ref-p78-elsewhere");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let mut given = Given::healthy(&fixture, &runtime, &certifies);
    let elsewhere = "b".repeat(40);
    given.refs = RecordingRefs::at(&fixture, &elsewhere);

    let committed = fixture.log_bytes();
    let (result, _) = resume(&fixture, &harness, &given);

    let text = message(&result.expect_err("a ref at another commit refuses"));
    assert!(
        text.contains(fixture.started.integration_ref.as_str())
            && text.contains(&elsewhere)
            && text.contains(fixture.started.base_sha.as_str()),
        "the refusal names the ref, where it is, and where the record says it should be: {text}"
    );
    assert_eq!(
        create_ref_entries(&harness),
        0,
        "the funnel was entered for a ref the step must have refused on sight"
    );
    assert!(given.refs.created().is_empty());
    assert_eq!(
        given.refs.target().as_deref(),
        Some(elsewhere.as_str()),
        "the ref was moved to make room for the run"
    );
    assert_eq!(
        fixture.log_bytes(),
        committed,
        "a P7/P8 refusal precedes every recovery event"
    );
}

/// A symbolic ref, and a checked-out one, refuse at `assert_publishable` —
/// before the target is ever read.
///
/// Two shapes and not one: they are the two arms of
/// `WorkspaceManager::assert_publishable`, and `refuse_symbolic` is also the
/// first statement of `direct_ref_target`, so a symbolic ref has two chances to
/// be caught and a build that lost the first would still pass a test that only
/// asserted "it refused". The `direct_target` count is what separates them —
/// neither shape may reach it.
#[test]
fn a_resume_refuses_a_symbolic_or_checked_out_integration_ref() {
    for (tag, shape, expected) in [
        ("symbolic", RefShape::Symbolic, "it is a symbolic ref"),
        (
            "checked-out",
            RefShape::CheckedOut,
            "it is checked out in the worktree",
        ),
    ] {
        let fixture = Fixture::healthy(&format!("ref-p78-{tag}"));
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let mut given = Given::healthy(&fixture, &runtime, &certifies);
        given.refs = RecordingRefs::shaped(&fixture, shape);

        let committed = fixture.log_bytes();
        let (result, _) = resume(&fixture, &harness, &given);

        let text = message(&result.expect_err("an unpublishable ref refuses"));
        assert!(
            text.contains(expected),
            "the refusal says which shape it found ({tag}): {text}"
        );
        assert!(
            text.contains(fixture.started.integration_ref.as_str()),
            "and names the recorded ref ({tag}): {text}"
        );
        assert_eq!(
            create_ref_entries(&harness),
            0,
            "the funnel ran for an unpublishable ref ({tag})"
        );
        assert_eq!(
            given.refs.target(),
            None,
            "and nothing was written to it ({tag})"
        );
        assert_eq!(
            given.refs.targets_read(),
            0,
            "`assert_publishable` did not refuse first: the target was read for an \
             unpublishable ref ({tag})"
        );
        assert_eq!(
            fixture.log_bytes(),
            committed,
            "a P7/P8 refusal precedes every recovery event ({tag})"
        );
    }
}

/// The step's three lower bounds, each asserted by the refusal that must
/// precede it: **(b)**, **(c)** and **(f)** all leave the ref untouched.
///
/// The bounds are stated in this module's own comment and this is what makes
/// them checkable rather than asserted in prose:
///
/// * **(b)** a Complete or Halted run does not continue, and publishing a
///   finished run's integration ref is continuing it;
/// * **(c)** the repository is touched only once the recorded Runner has been
///   rebuilt and its probes have answered, so a resume that cannot run leaves
///   the object store as it found it;
/// * **(f)** an unresolved promotion — and, by the same clause, an unresolved
///   integration transaction — is a prefix whose integration ref may be
///   mid-move, and "present == base continue" would adopt one under a
///   transaction this build cannot resolve. This case is also the first
///   coverage [`refuse_unimplemented_terminals`] has had.
///
/// The fourth bound, "**before (d)**", is not here: it is asserted positively by
/// [`kill_after_run_started_creates_integration_ref`], which reads the log at
/// the instant the funnel ran.
#[test]
fn the_p7_p8_step_runs_after_the_refusals_that_bound_it() {
    // (b): a Halted run.
    {
        let fixture = Fixture::build(
            "ref-p78-after-b",
            Damage {
                extra: vec![
                    dispatched(),
                    attempt_started(1),
                    attempt_finished(
                        1,
                        AttemptSettlement::Closed {
                            transition: SettlementTransition::Failed {
                                halts_run: true,
                                reason: "the ladder ran out".to_owned(),
                            },
                            lease: LeaseDisposition::PredictedReleased,
                        },
                    ),
                    run_finished(RunOutcome::Halted, Some(ALPHA)),
                ],
                ..Damage::default()
            },
        );
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let text = message(
            &resume(&fixture, &harness, &given)
                .0
                .expect_err("a finished run does not continue"),
        );
        assert!(text.contains("already finished"), "{text}");
        assert_eq!(
            create_ref_entries(&harness),
            0,
            "(b) refused and the ref was published anyway"
        );
        assert_eq!(given.refs.target(), None);
    }

    // (c): a shell probe that does not answer.
    {
        let fixture = Fixture::healthy("ref-p78-after-c");
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let runner = RecordingRunner::failing("bash");
        let adapters = StubAdapters;
        let preflight = real_preflight(&runner, &adapters, &fixture);
        let given = Given::healthy(&fixture, &runtime, &preflight);

        let text = message(
            &resume(&fixture, &harness, &given)
                .0
                .expect_err("a failing probe refuses"),
        );
        assert!(text.contains("the recorded shell"), "{text}");
        assert_eq!(
            create_ref_entries(&harness),
            0,
            "(c) refused and the ref was published anyway"
        );
        assert_eq!(given.refs.target(), None);
    }

    // (f): a generation the log left in promotion.
    {
        let fixture = Fixture::build(
            "ref-p78-after-f",
            Damage {
                extra: vec![
                    dispatched(),
                    attempt_started(1),
                    // `Succeeded` is what puts the generation in `Promoting`.
                    // Under erratum **E6** step (f) *converges* that window
                    // rather than refusing it — but the convergence needs the
                    // pin, which names the commit the settlement authorised,
                    // and this fixture seeds events without one. A settled
                    // attempt whose pin is gone is neither `T-CAND-OBJ` (which
                    // governs an unpinned object and leaves it to Git) nor a
                    // completable `T-CAND-REF`, so it is refused rather than
                    // guessed — and that refusal still precedes P7/P8, which is
                    // what this case is about.
                    attempt_finished(
                        1,
                        AttemptSettlement::Closed {
                            transition: SettlementTransition::Succeeded,
                            lease: LeaseDisposition::PredictedRetained,
                        },
                    ),
                ],
                ..Damage::default()
            },
        );
        let harness = harness();
        let runtime = runtime_holding_the_record();
        let certifies = AlwaysCertifies;
        let given = Given::healthy(&fixture, &runtime, &certifies);

        let text = message(
            &resume(&fixture, &harness, &given)
                .0
                .expect_err("a promoting generation with no pin cannot be converged"),
        );
        assert!(
            text.contains("candidate pin is absent"),
            "the refusal names what it could not name: {text}"
        );
        assert_eq!(
            create_ref_entries(&harness),
            0,
            "(f) refused and the ref was published anyway"
        );
        assert_eq!(given.refs.target(), None);
    }
}

// ===========================================================================
// A kill during recovery
// ===========================================================================

/// The child half of [`kill_during_recovery_repeats_recovery`].
///
/// `Injection::Kill` is `std::process::abort()` — a real process death, chosen
/// so the claim is *what a coordinator that runs no cleanup leaves on disk*.
/// The `unreachable!` at the end is load-bearing: it is what fails the test if
/// the injection ever silently stops killing.
#[test]
#[ignore = "spawned as a subprocess by kill_during_recovery_repeats_recovery"]
fn recovery_kill_child() {
    let repo_root = PathBuf::from(
        std::env::var("UPSTROKE_TEST_KILL_REPO").expect("the parent names the repository"),
    );
    let git_dir = PathBuf::from(
        std::env::var("UPSTROKE_TEST_KILL_GITDIR").expect("the parent names the git dir"),
    );
    let repo_key = RepoKey::v1(&std::fs::canonicalize(&git_dir).expect("the git dir exists"));

    let harness = harness();
    harness
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .arm(
            EffectSiteId::Event(EventSite::Append),
            SubEffectPoint::Written,
            InjectionMode::Kill,
        )
        .expect("the Written point supports a kill");
    let mut hooks = HarnessTopologyHooks::new(harness);
    let runtime = runtime_holding_the_record();
    let liveness = FakeOwnerLiveness::new();
    let view = DisposableDirView::new(ContainerTrace::default());
    let certifies = AlwaysCertifies;
    let incarnation = IncarnationId(RESUMER.to_owned());
    let today = container_selection();
    // The child's ref namespace is process-local and empty, which is what a run
    // killed before P8 has. The P7/P8 step runs before the first append, so the
    // child creates it here and then dies at `Event.Append`'s `Written` point —
    // nothing of it survives, and the parent's assertions are about the disk.
    let refs = RecordingRefs::with_log(
        &rundir::public_dir(&repo_root, RUN_ID).join(rundir::EVENT_LOG),
        RefShape::Direct,
        None,
    );
    let mut warnings = Vec::new();

    let root = RootDerived::derive_with(&repo_root, RUN_ID, None, TOPOLOGY_SCHEMA)
        .expect("(a0) derives in the child");
    // Step (g)'s manager, derived from the root (a0) just computed rather than
    // from an env var the parent would have to pass: the private root is the
    // one thing (a0) exists to establish, and taking it from anywhere else
    // would let the child rebuild worktrees under a root the order refused.
    let manager = crate::workspace_manager::WorkspaceManager::derive(
        &repo_root,
        root.private_root(),
        RUN_ID,
        RESUMER,
    )
    .expect("the child's repository and private root are real directories");
    let _ = run_recovery_order(
        root,
        &ResumeSeams {
            repo_root: &repo_root,
            worktree_git_dir: &git_dir,
            repo_key: &repo_key,
            incarnation: &incarnation,
            inputs: FrozenInputs {
                plan: plan(),
                normalized_plan_digest: "sha256:aaaa".to_owned(),
            },
            today: &today,
            runtime: &runtime,
            liveness: &liveness,
            view: &view,
            preflight: &certifies,
            refs: &refs,
            manager: &manager,
            clock: &Frozen,
        },
        &mut hooks,
        &mut warnings,
    );
    unreachable!("the kill must have taken this process");
}

/// A kill at a recovery event's append leaves the run resumable, and the next
/// process **repeats the whole order from (a0)**.
///
/// `recovery_order` (i): "a kill at any point repeats from (a0)". So the
/// assertion is not only that a second resume succeeds — it is that the second
/// process re-derived the root, re-took the locks, re-established the barrier
/// and re-censused, all of which are (a0), (a) and (a1) running again over a
/// prefix a dead process left. A build that resumed from a checkpoint would
/// skip them and still finish.
///
/// The child is spawned **through the host Runner**, not through
/// `std::process::Command`: `std::process::Command` is on the effect denylist
/// and `src/engine/topology/**` may not reach it even in tests. The Runner is
/// the funnel that owns `Process.Spawn`, which is exactly the rule.
#[test]
fn kill_during_recovery_repeats_recovery() {
    let fixture = Fixture::healthy("kill-recovery");
    let before = fixture.log_bytes();

    let exe = std::env::current_exe().expect("the test binary knows where it is");
    let request = RunnerRequest {
        command: CommandSpec {
            program: exe.display().to_string(),
            args: vec![
                "--exact".to_owned(),
                "engine::topology::recover::tests::recovery_kill_child".to_owned(),
                "--ignored".to_owned(),
                "--test-threads".to_owned(),
                "1".to_owned(),
            ],
            env: vec![
                (
                    "UPSTROKE_TEST_KILL_REPO".to_owned(),
                    fixture.repo_root.display().to_string(),
                ),
                (
                    "UPSTROKE_TEST_KILL_GITDIR".to_owned(),
                    fixture.git_dir.display().to_string(),
                ),
            ],
            stdin: Vec::new(),
        },
        workspace: fixture.repo_root.clone(),
        role: crate::runner::ExecutionRole::Gate,
        timeout: Duration::from_secs(120),
        agent: None,
        invocation: crate::runner::InvocationId::probe(crate::runner::ProbeTarget::Shell, 7)
            .expect("a probe identity for the spawned child"),
    };
    let output = crate::runner::host::HostRunner::new()
        .run(&request)
        .expect("the child runs");
    assert_ne!(
        output.code,
        Some(0),
        "the child must have died rather than finished: {output:?}"
    );
    // **Died, rather than failed.** `Injection::Kill` is `std::process::abort()`,
    // which takes the process before the test harness can print anything about
    // the test — so an aborted child emits no result line at all. A child whose
    // injection silently stopped killing reaches the `unreachable!`, panics, and
    // the harness prints both its message and a result line. Asserting only a
    // non-zero exit cannot tell those apart, because a failed test is also
    // non-zero; this is what makes the `unreachable!` load-bearing.
    assert!(
        !output.stdout.contains("test result:"),
        "the child printed a test result, so it finished rather than dying: {}",
        output.stdout
    );
    assert!(
        !output
            .stdout
            .contains("the kill must have taken this process"),
        "the child reached its `unreachable!`, so the injection did not kill it: {}",
        output.stdout
    );

    // What the dead coordinator left: the line it was writing, unsynced, and no
    // cleanup of any kind.
    let after_kill = fixture.log_bytes();
    assert!(
        after_kill.len() > before.len(),
        "the kill is at `Written`, after the bytes reached the file"
    );

    // And the next process repeats the order from (a0).
    //
    // The census's evidence has to be something the *repeat* can act on. The
    // dead child had already censused before it reached the append it died at,
    // so this run's stale marker is gone and stays gone: `RunDir.RemoveMarker`
    // would be absent from a build that repeated the census perfectly. A husk
    // planted now is the evidence instead — another crashed run, arriving
    // between the two processes — and it is the stronger one, because reclaiming
    // it is a census *effect* rather than a repair that finds nothing to do.
    const AFTER_THE_KILL: &str = "01KZTKILL00000000000000004";
    let husk = plant_husk(&fixture, AFTER_THE_KILL, false);
    assert!(husk.private.exists() && husk.public.exists());

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (result, _) = resume(&fixture, &harness, &given);
    let recovered = result.expect("the next process recovers");

    let seen = harness.lock().unwrap_or_else(PoisonError::into_inner);
    for site in [
        EffectSiteId::Lock(LockSite::AcquireWorktree),
        EffectSiteId::Lock(LockSite::AcquireRun),
        EffectSiteId::Event(EventSite::OpenLog),
        EffectSiteId::Event(EventSite::ProvePrefixStable),
        EffectSiteId::RunDir(RunDirSite::RemovePrivateHusk),
        EffectSiteId::RunDir(RunDirSite::RemovePublicHusk),
    ] {
        assert!(
            seen.observed(site, HookPhase::Before),
            "the repeat runs `{site}` again — a kill repeats from (a0), it does not resume from a \
             checkpoint"
        );
    }
    drop(seen);
    assert!(
        !husk.private.exists() && !husk.public.exists(),
        "and the repeat's census reclaimed the husk it found, both halves"
    );
    assert_eq!(
        recovered.resumed.epoch, 2,
        "the killed process's `run_resumed` line survived, so this resume opens the epoch after it"
    );
}

// ===========================================================================
// The chain's one entry point, as a source census
// ===========================================================================

/// `StablePrefix::into_log_and_fold` is reached from exactly one production region of
/// the topology engine: [`BarrierHeld::from`].
///
/// # Why this is a census and not a visibility
///
/// Design v4 §4 makes `BarrierHeld` unforgeable by taking a `StablePrefix` **by
/// value**, and `StablePrefix`'s only constructor is
/// `events::log::establish_stable_prefix` — so barrier *evidence* cannot be
/// manufactured. What it does not close is the other direction:
/// `StablePrefix::into_log_and_fold` is `pub`, so a topology module could take a
/// proven prefix apart and hold the append handle and the fold **without**
/// wrapping them in a `BarrierHeld`, and then everything the chain hangs off —
/// `ResumeCensused`, and through it every recovery emitter — would be reachable
/// beside the chain rather than through it.
///
/// Narrowing the visibility cannot fix that here. `pub(crate)` does not stop
/// one topology module reaching another's dependency, and anything tighter than
/// `pub(in crate::events)` would break `BarrierHeld::from` itself, which *is*
/// built on `into_parts`. So the claim is the honest one — `BarrierHeld` is the
/// only route **the topology engine takes** — and this is what makes it a
/// checkable claim rather than a convention. Same idiom, and same reason, as
/// `events::log::tests::the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold`.
#[test]
fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {
    const ENTRY: &str = "into_log_and_fold(";
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let test_modules = {
        let mut all = Vec::new();
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("src is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    all.push(path);
                }
            }
        }
        crate::effects::census_domain::whole_file_test_modules(&all, 13)
    };
    let mut stack = vec![src.clone()];
    let mut callers: Vec<(String, usize)> = Vec::new();
    let mut regions: Vec<(String, usize, usize)> = Vec::new();
    let mut scanned = 0_usize;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("src is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let relative = path
                .strip_prefix(&src)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            // Only the topology engine is in scope: the funnel that defines
            // `into_parts` and its own tests are not a second route into the
            // chain, they are where it lives.
            if !relative.starts_with("engine/topology") {
                continue;
            }
            // A file the crate declares as a whole-file test module is test
            // code in full and has no production half; counting one would
            // count a fixture as a second route. **Through the crate's own
            // declarations**, not through the file name: three of the
            // seventeen are not called `tests.rs`, and one of those is
            // `engine/topology/scaffold.rs` — inside this very census's
            // `engine/topology` domain. `PR7-R5-ATT-001`.
            if test_modules.contains(&path) {
                continue;
            }
            scanned += 1;
            let source = std::fs::read_to_string(&path).expect("a source file");
            // The production half only. A test that takes a prefix apart is a
            // fixture, not a path a run can take.
            //
            // **`effects::production_code`, not a cut at the first
            // `#[cfg(test)]`.** The cut was the bug: it fired on the first *raw*
            // occurrence of the text, comments included, and in
            // `engine/topology/run.rs` that is **line 83 of 1777** — inside a
            // doc comment — so this census was scanning 4.7% of the driver, the
            // single most likely file for a second route to appear in. In
            // `engine/topology.rs` it was line 39, inside the module doc.
            //
            // An earlier repair built the needle with `format!` so that a
            // mention *in this file* could not cut it. That fixed one instance
            // of `PR4-CENSUS-COMMENT-ORACLE` and left the class open in every
            // file this walk reads. `production_code` blanks comments and
            // string literals and removes each `#[cfg(test)]` **item** in place
            // rather than truncating, which is the repair the four whole-tree
            // censuses already have.
            //
            // Found by S5 round 2's `seams` lens, and it lands on this slice's
            // own evidence: the guard was cited as proving that
            // `StablePrefix::events` did not become a second entry point, and
            // that check ran against the truncated domain.
            let production = crate::effects::production_code(&source);
            let production = production.as_str();
            // Calls, not definitions — a definition is not a route.
            //
            // The needle used to be the bare `into_parts(`, and at integration
            // it reported five false routes: three definitions in `startup.rs`
            // and two calls in `create.rs`, every one of them a typestate
            // witness of that lane handing back its own fields. The comment
            // here said the fix was "to rename, not to widen the needle", and
            // that is what was done: `StablePrefix`'s accessor is
            // `into_log_and_fold`, a name nothing else in the crate carries,
            // so the needle now means what it says.
            // **The control this census did not have.** A zero count from an
            // empty region is indistinguishable from a zero count from a clean
            // file, and that is exactly how the truncation hid: the driver's
            // region was 83 lines of 1777 and its zero looked like a pass. The
            // four whole-tree censuses each carry this control; this one did
            // not, which is why the class survived here.
            regions.push((relative.clone(), production.len(), source.len()));

            let count = production
                .match_indices(ENTRY)
                .filter(|(at, _)| !production[..*at].trim_end().ends_with("fn"))
                .count();
            if count > 0 {
                callers.push((relative, count));
            }
        }
    }
    callers.sort();

    assert!(
        scanned >= 4,
        "the walk found only {scanned} topology sources, so its zero counts would prove nothing"
    );
    // Every region is a real fraction of its file. A tenth is a generous floor
    // and still an order of magnitude above what the truncation left behind.
    for (file, region, whole) in &regions {
        assert!(
            *region * 10 > *whole,
            "{file}'s production region is {region} of {whole} bytes. A census over a fraction \
             of a file reports zero for the part it never read — this is `PR4-CENSUS-COMMENT-ORACLE`, \
             and it is how the driver was scanned at 4.7% while reading as a pass"
        );
    }
    assert_eq!(
        callers,
        vec![("engine/topology/recover.rs".to_owned(), 1)],
        "a proven prefix becomes an append handle in exactly one production place in the topology \
         engine, and that place is `BarrierHeld::from`"
    );
}

// ---------------------------------------------------------------------------
// The order's completeness against the packet's own list
// ---------------------------------------------------------------------------

/// **The recovery order performs every step `recovery_order` names.**
///
/// This is the test that did not exist while step (g) did not exist. For the
/// whole of PR7's implementation and two review rounds, `run_recovery_order`
/// performed nine of the ten steps it owns, with all 117 named tests passing,
/// every gate green on three platforms, and its own doc comment claiming
/// "steps (a) through (h)". Nothing could see it: a mutation catalogue measures
/// whether existing code is pinned, and **omission has nothing to mutate**.
///
/// So the assertion is against [`RecoveryStep::ALL`], which is the packet's
/// sentence transcribed into a type, and not against a second list written from
/// the implementation. Two steps are excluded **by name and with a reason**
/// rather than by being quietly absent — see [`RecoveryStep::performer`].
#[test]
fn the_recovery_order_performs_every_step_the_packet_names() {
    let fixture = Fixture::healthy("every-step");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);
    let recovered = outcome.expect("the healthy resume completes");

    let owed: Vec<RecoveryStep> = RecoveryStep::ALL
        .into_iter()
        .filter(|step| step.performer() == Performer::ThisOrder)
        .collect();

    // Completeness first, and it is the half this test exists for: every step
    // the packet gives this order was performed, exactly once. Sorted, so a
    // step that moved for a stated reason cannot fail the completeness claim —
    // that is the next assertion's subject and it is a different question.
    let mut performed = recovered.steps.clone();
    performed.sort_unstable();
    let mut expected = owed.clone();
    expected.sort_unstable();
    assert_eq!(
        performed,
        expected,
        "the order performed {:?} and the packet names {:?} for it; a step in \
         the second list and not the first is a step no code performs, which \
         is the defect this test exists for",
        recovered
            .steps
            .iter()
            .map(|step| step.label())
            .collect::<Vec<_>>(),
        owed.iter().map(|step| step.label()).collect::<Vec<_>>()
    );

    // Then the order, for every step whose position the packet alone decides.
    // `(f)` is excluded **by a named live clause**, not by being skipped: see
    // `RecoveryStep::position_override`.
    let packet_order: Vec<RecoveryStep> = owed
        .iter()
        .copied()
        .filter(|step| step.position_override().is_none())
        .collect();
    let performed_order: Vec<RecoveryStep> = recovered
        .steps
        .iter()
        .copied()
        .filter(|step| step.position_override().is_none())
        .collect();
    assert_eq!(
        performed_order, packet_order,
        "the steps the packet alone positions ran out of order; a step that \
         must move carries the clause that moves it"
    );

    // And the one that does move, moved for its reason and not by accident:
    // **(f) has two halves and erratum E6 separated them.** Its refusing half —
    // the unresolved integration transaction, one of the two things
    // `checkpoint_refusals` authorises — still runs before any append. Its
    // converging half appends `candidate_prepared` for a settled-but-unrecorded
    // candidate, so it runs *with* the appending steps and is what `steps`
    // records. A `Promoting` generation was refused here until E6, and that was
    // a third checkpoint refusal.
    let at = |step: RecoveryStep| {
        recovered
            .steps
            .iter()
            .position(|performed| *performed == step)
            .expect("every owed step was performed")
    };
    assert!(
        at(RecoveryStep::D) < at(RecoveryStep::F) && at(RecoveryStep::E) < at(RecoveryStep::F),
        "(f)'s converging half appends, so it belongs with (d) and (e) rather \
         than before them. Its refusing half is unmarked because a refusal ends \
         the command and records no step"
    );
    assert!(
        at(RecoveryStep::F) < at(RecoveryStep::G) && at(RecoveryStep::G) < at(RecoveryStep::H),
        "and it stays in the packet's position: after (e), before (g) and (h)"
    );
}

/// The transcribed list is the packet's list — eleven steps, these labels, in
/// this order.
///
/// The companion to the test above, and it guards the *other* direction. That
/// one proves the implementation covers [`RecoveryStep::ALL`]; this one proves
/// `ALL` is still the packet's sentence, because a variant deleted from `ALL`
/// would make the first test pass by asking for less.
#[test]
fn the_transcribed_recovery_steps_are_the_packets_eleven() {
    assert_eq!(
        RecoveryStep::ALL
            .iter()
            .map(|step| step.label())
            .collect::<Vec<_>>(),
        vec!["a0", "a", "a1", "b", "c", "d", "e", "f", "g", "h", "i"],
        "transcribed from `decisions.sequential_substrate.recovery_order`"
    );
    assert_eq!(
        RecoveryStep::ALL
            .iter()
            .filter(|step| step.performer() != Performer::ThisOrder)
            .map(|step| (step.label(), step.performer()))
            .collect::<Vec<_>>(),
        vec![("a0", Performer::CallerBefore), ("i", Performer::LoopAfter)],
        "exactly two steps are delegated, and each is delegated to a named \
         performer with a reason: a step whose performer nobody states is \
         indistinguishable from a step nobody performs"
    );
}

// ---------------------------------------------------------------------------
// (g) — recreate `OpenNoAttempt` worktrees at their bases
// ---------------------------------------------------------------------------

/// **The step does work, and the work is a worktree at the recorded base.**
///
/// The companion to `the_recovery_order_performs_every_step_the_packet_names`,
/// and it is the half that test cannot give: over a healthy fixture (g) runs
/// and finds nothing, so "the step ran" and "the step is a no-op" are the same
/// observation. This fixture leaves the one state (g) exists for — a generation
/// dispatched and never attempted, which is what a crash between
/// `task_dispatched` and `attempt_started` leaves.
#[test]
fn resume_recreates_an_open_no_attempt_worktree_at_its_base() {
    let fixture = Fixture::build(
        "step-g-recreate",
        Damage {
            open_generation: true,
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let manager = fixture.manager();
    let slot = crate::engine::topology::dispatch::task_slot(ALPHA, GEN);
    let worktree = manager.slot_path(&slot);
    assert!(
        !worktree.exists(),
        "the fixture leaves the generation open with no worktree, which is what \
         the kill leaves and what (g) has to answer"
    );

    let (outcome, _) = resume(&fixture, &harness, &given);
    let recovered = outcome.expect("the resume completes");

    assert_eq!(
        recovered
            .recreated
            .iter()
            .map(|(key, generation, _)| (*key, *generation))
            .collect::<Vec<_>>(),
        vec![(ALPHA, GEN)],
        "(g) acts on exactly the open generation, and on nothing else"
    );
    assert!(
        worktree.exists(),
        "(g) recreates the worktree the generation records; without it the \
         resumed loop has a dispatched generation whose checkout does not exist"
    );
    assert_eq!(
        crate::workspace_manager::fixture::git(&worktree, &["rev-parse", "HEAD"]),
        fixture.base_sha.0,
        "at its **base** — `recovery_order` (g) says where, and a worktree cut \
         anywhere else silently changes what the next attempt starts from"
    );
}

/// A repair generation cannot reach step (g) in this slice, and the reason is
/// measured rather than asserted.
///
/// (g) refuses a generation whose lease is an inherited lineage: `T-DISPATCH`'s
/// resume action for a repair is to re-run the recorded materialization, whose
/// source candidate the fold does not retain, and `checkpoint_refusals` gives
/// repair execution to PR8. That arm is **unreachable here**, and this test
/// pins both walls that make it so, because "unreachable" written in a comment
/// is the same sentence as "I did not check".
///
/// The wall this test will lose first is the second one: the day a slice admits
/// repairs, `TaskRegistry::from_plan` starts producing entries with a lineage,
/// this test fails, and (g)'s arm becomes reachable — which is precisely when
/// someone should be made to look at it.
#[test]
fn a_repair_generation_cannot_reach_step_g_in_this_slice() {
    // Wall one: the fold refuses an inherited lease on an ordinary task, at the
    // barrier's checked replay — so the event never becomes fold state at all.
    let repair = {
        let TopologyEventBody::TaskDispatched { mut data } = dispatched() else {
            unreachable!("`dispatched` builds a `TaskDispatched`")
        };
        data.lease = LeaseGrant::InheritedLineage { root: TaskKey(1) };
        TopologyEventBody::TaskDispatched { data }
    };
    let fixture = Fixture::build(
        "step-g-repair",
        Damage {
            extra: vec![repair],
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume(&fixture, &harness, &given);
    let text = outcome
        .expect_err("the fold refuses the shape before any step sees it")
        .to_string();
    assert!(
        text.contains("an ordinary task belongs to no lineage and cannot inherit one's lease"),
        "the refusal is the fold's, at the replay, and not step (g)'s: {text}"
    );
    assert!(
        text.contains("stable-prefix barrier"),
        "and it lands at the barrier, so nothing fold-derived was acted on: {text}"
    );

    // Wall two: and there is no task it *would* be legal on, because this
    // slice's registry gives every entry `lineage: None`.
    let registry = TaskRegistry::originals_with_agents(
        &fixture.plan,
        &fixture.started.registry_record(),
        &fixture.started.probed_agents,
    )
    .expect("the fixture's plan registers");
    assert!(
        !registry.entries().is_empty(),
        "a registry with no entries would satisfy the next assertion by having \
         nothing to check"
    );
    assert!(
        registry
            .entries()
            .iter()
            .all(|entry| entry.lineage.is_none()),
        "no entry this slice can build descends from a lineage, so no \
         `task_dispatched` carrying an inherited lease can be valid"
    );
}

/// **The recovery order hands its state on rather than dropping it.**
///
/// This is the assertion that did not exist while `TopologyRun` did not exist.
/// `run_resumed` consumed the last witness and returned a two-field summary, so
/// the append handle `(a1)` had just proved, the fold built from exactly those
/// bytes, and both locks were destroyed at the end of the order. A loop cannot
/// be written against a function that ends by throwing the run away — so the
/// missing driver was not only a missing function, it was a missing *value*.
///
/// What is asserted is that the three survive and are the *same* three, not
/// replacements: the log still appends to the proven prefix, the fold is the
/// one the barrier replayed, and the locks are still held.
#[test]
fn the_recovery_order_hands_the_run_on_rather_than_dropping_it() {
    let fixture = Fixture::healthy("hand-on");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (recovered, handle) = outcome.expect("the healthy resume completes");

    assert_eq!(
        handle.started.run_id, RUN_ID,
        "the handle names the run the order recovered"
    );
    assert!(
        !handle.fold.is_poisoned(),
        "and hands on a fold that may still be transitioned"
    );
    assert_eq!(
        handle.fold.epoch().map(|epoch| epoch.0),
        Some(recovered.resumed.epoch),
        "the fold in the handle is the one `(h)` incremented, not a second \
         derivation of the same log — a rebuilt fold is a rule that can \
         disagree with the one the barrier proved"
    );

    // The run lock is still held, which is the property that lets a loop run
    // at all. Measured by asking for it: a second acquisition must be refused
    // while the handle is alive.
    let contested = rundir::RunLock::acquire(&rundir::public_dir(&fixture.repo_root, RUN_ID));
    assert!(
        contested.is_err(),
        "the run lock is still held by the handle; a loop that had to retake \
         it would be racing itself"
    );

    // And released when the handle dies, in declaration order.
    drop(handle);
    rundir::RunLock::acquire(&rundir::public_dir(&fixture.repo_root, RUN_ID))
        .expect("dropping the handle releases the run lock");
}

// ---------------------------------------------------------------------------
// The driver, taking over from the order
// ---------------------------------------------------------------------------

/// **`TopologyRun` drives a resumed run, and `Step` finally has a consumer.**
///
/// This test lives here rather than beside `run.rs` because the only thing that
/// produces a real [`RunHandle`] is a real recovery, and the fixture for that
/// is this file's. Duplicating it there to keep the test adjacent to its
/// subject would be a second fixture for one state — the duplication shape this
/// slice has paid for four times.
///
/// What it asserts is the seam that did not exist: the order hands the run on,
/// the driver takes it, and one iteration of `loop` selects a branch and acts.
/// Before `RunHandle`, there was no value to hand over; before `run.rs`,
/// nothing outside `select.rs` so much as matched on a `Step`.
#[test]
fn the_driver_takes_over_from_the_recovery_order_and_steps() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::healthy("driver-steps");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::default();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let kinds_before = durable_kinds(&fixture);
    let progress = run
        .step(&seams, &mut hooks)
        .expect("the branch performs its first four clauses");

    // **The driver ran an attempt.** Named exactly, not with a `matches!` that
    // would pass whichever branch the fixture happened to reach — a fixture
    // that silently started reaching a different one would take the assertion
    // with it.
    let Progress::Settled {
        key,
        accepted,
        spent_attempt,
    } = progress
    else {
        panic!("the ready-dispatch branch did not run an attempt: {progress:?}");
    };
    assert_eq!(key, TaskKey(0));

    // **Not accepted, and the reason is the contract's.** This fixture's runner
    // answers every request with `exit 0` and never touches the worktree, so
    // the capture's tree is the base's and the diff is empty.
    // `pr_sequence[8].slice_contract.expected_failures_refusals` names
    // "empty-diff and unresolved-index attempt failures" as this slice's, and
    // this is the driver reaching one.
    //
    // It asserted `accepted` before the ladder's cheap rungs were wired, and
    // passed: `judge` starts at gates, the plan configures none, and nothing
    // had asked what the diff contained. A driver that accepted this would have
    // pinned a candidate whose commit is its own parent.
    assert!(
        !accepted,
        "a worker that edited nothing was judged acceptable, which means the \
         cheap rungs of the verification ladder did not run"
    );

    // The dispatch AND the attempt are real and durable, in that order. Both
    // went through the production emitter, which is what makes them subject to
    // the append-error protocol; the scaffold's emitter re-implements the
    // append and runs none of it.
    assert_eq!(
        durable_kinds(&fixture),
        {
            let mut expected = kinds_before.clone();
            expected.push("task_dispatched".to_owned());
            expected.push("attempt_started".to_owned());
            expected.push("attempt_finished".to_owned());
            expected
        },
        "the whole branch, in order: the dispatch, the attempt, the settlement"
    );

    // **The allowance, from `ladder::spends_allowance` and nowhere else.** An
    // empty diff spends: the line is "the worker ran", not "a verdict was
    // reached". The settlement carries the answer out of the branch because it
    // is the input the *next* ladder decision reads.
    assert!(
        spent_attempt,
        "an attempt whose worker ran and produced a diff to judge did not spend \
         one of its rung's attempts"
    );

    // **The worker ran through the Runner**, which is what makes this the
    // fourth clause rather than a plan that was built and dropped. The whole
    // point of the driver is that something calls the machinery.
    assert!(
        !runner.requests().is_empty(),
        "the attempt appended `attempt_started` and never spawned anything"
    );

    // **The recorded region is the fold's, not a second derivation.**
    // `dispatch_lease_check` admits this task by computing the region and
    // asking the lease table what it overlaps; the log then holds whatever the
    // dispatch recorded, and the lease table keeps the log's. Two derivations
    // means the fold admits on one answer and the run is protected by another.
    //
    // The fixture's hint is a glob (`src/alpha/*.rs`), which is what makes this
    // assertion able to fail: the fold strips it to the literal prefix
    // `src/alpha`, and a driver taking hints literally would record a prefix
    // that overlaps nothing. Measured — that shipped, for one commit.
    let recorded = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::TaskDispatched { data } => Some(data.lease),
            _ => None,
        })
        .expect("the dispatch is durable");
    let LeaseGrant::Predicted { paths: recorded } = recorded else {
        panic!("an ordinary dispatch takes a predicted lease")
    };
    assert_eq!(
        Some(recorded),
        run.fold().predicted_region(ALPHA),
        "the region in the log is the one the fold admitted on. Compared \
         against the fold rather than against a literal, because a literal \
         would agree with whichever derivation this test happened to use"
    );

    // And the provisional reservation did not leak. O24 converts it AT the
    // append; a refusal after that must not leave an entitlement held, or the
    // next selection at width 1 sees a full pipeline forever.
    assert_eq!(
        run.entitlements_held(),
        0,
        "the dispatch reservation was converted at `task_dispatched`, not left \
         held across the refusal. A leaked entitlement here is
         `PR7-INTEGRATION-NO-ENTITLEMENT`'s failure wearing a different hat: at \
         the only width production creates, one held entitlement is a full \
         pipeline and nothing is ever selected again"
    );
}

/// **The driver carries an accepted attempt through the whole candidate
/// sequence.**
///
/// The companion to `the_driver_takes_over_from_the_recovery_order_and_steps`,
/// which is the rejection case: there the fixture's worker edits nothing and
/// the ladder's cheap rungs stop it at the empty diff. Here it leaves a change
/// behind, so nothing rejects and the branch runs the sequence
/// `side_effect_vs_event_ordering` specifies — commit object, pin, settlement,
/// `candidate_prepared`, candidates ref, `task_candidate_created`, then the pin
/// prune and the forced scrub.
///
/// Two tests rather than one parameterised over a flag: the two paths append
/// different events in different orders, and a grid would assert the union of
/// them.
#[test]
fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::healthy("driver-promotes");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = TracedHooks::new(&harness);
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let kinds_before = durable_kinds(&fixture);
    let progress = run
        .step(&seams, &mut hooks)
        .expect("the branch performs its first four clauses");

    let Progress::Settled { key, accepted, .. } = progress else {
        panic!("the ready-dispatch branch did not run an attempt: {progress:?}");
    };
    assert_eq!(key, TaskKey(0));
    assert!(
        accepted,
        "the worker left a change and the plan configures no gates or reviewers, \
     so nothing could reject it"
    );

    // **The settlement lands between the pin and `candidate_prepared`**, which
    // is what `settle_succeeded`'s own note requires: `T-CAND-OBJ`'s window
    // covers the commit object and the pin with `authoritative_state: attempt
    // unsettled`. The order here is the assertion — a sequence that appended
    // the settlement after `candidate_prepared` would leave the generation in a
    // class the fold refuses that event from.
    assert_eq!(
        durable_kinds(&fixture),
        {
            let mut expected = kinds_before.clone();
            expected.push("task_dispatched".to_owned());
            expected.push("attempt_started".to_owned());
            expected.push("attempt_finished".to_owned());
            expected.push("candidate_prepared".to_owned());
            expected.push("task_candidate_created".to_owned());
            expected
        },
        "the whole branch, in the order the packet specifies"
    );

    // -----------------------------------------------------------------------
    // **The same clause over the EFFECTS, not only the events.**
    //
    // `side_effect_vs_event_ordering`: "commit object (R27) before pin
    // (IdUnread between); **pin before `candidate_prepared`**; **candidates ref
    // after `candidate_prepared`** and before `task_candidate_created`". The
    // event list above cannot see any of that — it holds no refs and no objects.
    //
    // `candidate::tests::pin_pruned_after_promotion` asserts exactly this, over
    // `candidate::promote`. The driver assembles the same steps from the three
    // split halves, and **no ordering assertion reached that composition**:
    // four `PR7-PIPELINE-*` catalogue mutations that reorder it — the pin moved
    // after `candidate_prepared`, the candidates ref moved before it, the commit
    // object moved to just after capture, the pin created before `commit-tree` —
    // were all green. One rule, two production compositions, one witness.
    assert_eq!(
        hooks.timeline.order(&[
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            EffectSiteId::Ref(RefSite::PinCandidatePrepared),
            EffectSiteId::Event(EventSite::Append),
            EffectSiteId::Ref(RefSite::CreateCandidates),
            EffectSiteId::Ref(RefSite::DeleteCandidatePin),
            EffectSiteId::Worktree(WorktreeSite::Remove),
        ]),
        vec![
            // task_dispatched and attempt_started: the branch's own prologue,
            // which this fixture drives in the same step.
            "Event.Append".to_owned(),
            "Event.Append".to_owned(),
            "Object.CandidateCommitTree".to_owned(),
            "Ref.PinCandidatePrepared".to_owned(),
            // attempt_finished(succeeded).
            "Event.Append".to_owned(),
            // candidate_prepared.
            "Event.Append".to_owned(),
            "Ref.CreateCandidates".to_owned(),
            // task_candidate_created.
            "Event.Append".to_owned(),
            "Ref.DeleteCandidatePin".to_owned(),
            // O31's scrub, which `PR7-PIPELINE-029` moved to immediately after
            // `candidate_prepared` — three appends too early — and was green.
            "Worktree.Remove".to_owned(),
        ],
        "the driver's candidate sequence, as one observed order over both families"
    );
}

/// **A run's spend is the same live as it is on replay.**
///
/// The ground-truth invariant, pinned as a property rather than as a count. The
/// ceiling reads `Spend`; a live process keeps it current as it settles, and
/// every fresh process rebuilds it with `Spend::replay` from the log. If those
/// two disagree, a resumed run either refuses work it could afford or buys work
/// it could not, and neither shows up as a wrong number anywhere — it shows up
/// as a run that behaves differently after a restart.
///
/// **Why this class, not this instance.** Both `attempt_finished` and
/// `candidate_prepared` carry an `AttemptRecord`, and for a successful attempt
/// the driver appends both. `Spend::replay` counted each occurrence, so replay
/// priced every success twice while live priced it once. Asserting a corrected
/// number would have fixed the instance; asserting **live == replay over the
/// run's own log** kills the class, including the next event kind that carries
/// a record.
#[test]
fn a_runs_spend_is_the_same_live_as_on_replay() {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::{Ceiling, Spend};

    let fixture = Fixture::healthy("spend-parity");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    run.step(&seams, &mut hooks)
        .expect("the accepted attempt runs the candidate sequence");

    // What the process believes it has spent, after settling one success.
    let live = run.spend().run_total();

    // What any fresh process would believe, from the same bytes.
    let events = TopologyFold::parse_log(&fixture.log_bytes()).expect("the log parses");
    let replayed = Spend::replay(&events).run_total();

    assert!(
        live > 0.0,
        "the fixture priced nothing, so this asserts two zeroes and proves \
         nothing: give the scaffold adapter a cost"
    );
    assert!(
        (live - replayed).abs() < 1e-9,
        "a live run and a replay of its own log price it differently: live \
         {live}, replay {replayed}. A resumed run would refuse work it could \
         afford, or buy work it could not"
    );
}

/// **The driver settles an outage from the fold's deferral count.**
///
/// The witness that closes the mutation named in `deferrals_recorded`'s own
/// doc. The fold-level witness
/// (`fold::tests::a_deferral_count_is_derived_by_replay_and_not_by_a_process_local_tally`)
/// covers the *accumulation*; this covers the **read**, which is load-bearing
/// on exactly one branch and so needed a fixture that reaches it.
///
/// The chain is the one the ladder specifies: an agent whose CLI reports a rate
/// limit -> `evaluate_outcome` maps it to `FailureKind::RateLimited` ->
/// `is_outage` recognises it -> `next_step` defers rather than blaming the
/// implementer -> `settle_failed` records `Deferred`.
///
/// **The prior deferral is what makes the read load-bearing.** The fixture's
/// log already holds one, so the settlement must record `defers: 2`. Without
/// it, a driver reading a constant zero would record `1` and be
/// indistinguishable from a correct one — which is precisely why the mutation
/// survived before this test existed.
#[test]
fn the_driver_settles_an_outage_from_the_folds_deferral_count() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    // One deferral already in the log, and the resume wakes the task back to
    // `Pending` so the driver can dispatch it again.
    let fixture = Fixture::build(
        "driver-outage",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Deferred {
                            defers: 1,
                            reason: "the pool was exhausted".to_owned(),
                        },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::rate_limiting();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run
        .step(&seams, &mut hooks)
        .expect("the branch settles an outage");

    let Progress::Settled {
        accepted,
        spent_attempt,
        ..
    } = progress
    else {
        panic!("the ready-dispatch branch did not settle: {progress:?}");
    };
    assert!(
        !accepted,
        "a rate-limited worker produced nothing to accept"
    );

    // **An outage spends no allowance.** `next_step` defers precisely so that
    // "retrying would burn attempts on a run that never got a verdict" does not
    // happen, and `spends_allowance` prices it the same way.
    assert!(
        !spent_attempt,
        "an outage spent one of the rung's attempts, which is the cell \
         `ladder::spends_allowance` exists to get right"
    );

    // **The count came from the fold**, not from a tally this process kept.
    // One deferral was already durable, so the settlement records two.
    let settlements: Vec<u32> = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => match data.settlement {
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Deferred { defers, .. },
                    ..
                } => Some(defers),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        settlements,
        vec![1, 2],
        "the second deferral did not continue the first. A driver reading a \
         process-local zero records `1` here and defers forever"
    );
}

/// **The driver parks an attempt, and the question it raises is durable.**
///
/// The last case of the ready-dispatch branch. An agent that stops and asks has
/// not failed at anything — `evaluate_outcome` reads `UPSTROKE-QUESTION:` out
/// of the outcome before the evidence rules, precisely so that an agent is not
/// punished for the empty diff its own question explains — so the chain is
/// `NeedsHuman` -> `Next::AskHuman(Clarify)` -> a parking settlement.
///
/// **`settle_failed` refuses a park that carries no question**, so reaching a
/// durable settlement at all is half the assertion. The other half is that the
/// question is the one the legacy engine would have asked: its context comes
/// from `coordinator::question_context` and its options from
/// `coordinator::question_options`, and this test reads both back out of the
/// log rather than out of the builder.
#[test]
fn the_driver_parks_an_attempt_with_the_question_it_raised() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::healthy("driver-parks");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::asking();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run.step(&seams, &mut hooks).expect("the branch parks");

    let Progress::Settled {
        accepted,
        spent_attempt,
        ..
    } = progress
    else {
        panic!("the ready-dispatch branch did not settle: {progress:?}");
    };
    assert!(
        !accepted,
        "an agent that asked a question produced no verdict"
    );

    // **A park spends no allowance.** "The code was never judged, so nothing is
    // spent and nothing escalates" — `next_step`'s own words, and the cell that
    // was wrong when the settlement derived the allowance from `Next` instead
    // of from the failure.
    assert!(
        !spent_attempt,
        "a park spent one of the rung's attempts, which is the cell the \
         allowance fix exists for"
    );

    // The settlement is durable and carries its question.
    let parked = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => match data.settlement {
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Parked { question },
                    ..
                } => Some(question),
                _ => None,
            },
            _ => None,
        })
        .expect("a parking settlement is durable");

    assert_eq!(parked.id, crate::ir::QuestionId("q-park-fixed".to_owned()));
    assert_eq!(parked.key, TaskKey(0));
    assert_eq!(parked.kind, crate::ir::QuestionKind::Clarify);

    // **The words are the legacy authorities', not the driver's.** The context
    // quotes the agent as data and names the task; the options are what
    // `question_options` gives a `Clarify`. A driver that worded its own would
    // pass every assertion above and fail these.
    assert!(
        parked.context.contains("stopped and asked for a decision"),
        "the context is not `question_context`'s: {}",
        parked.context
    );
    assert!(
        parked
            .context
            .contains("two incompatible \\\n                      formats")
            || parked.context.contains("incompatible"),
        "the agent's own words are not quoted back: {}",
        parked.context
    );
    assert_eq!(
        parked.options,
        crate::engine::coordinator::question_options(crate::ir::QuestionKind::Clarify),
        "the options are not `question_options`'s"
    );
}

/// **The driver refuses a tree whose bytes a gate would not see.**
///
/// The ladder's third cheap rung, and the one that was owed longest.
/// `Workspace::review_input_problem_for_tree` refuses staged evidence a
/// clean/smudge filter has transformed, or a worktree still holding unstaged or
/// dirty nested state — either makes the reviewed diff describe something other
/// than what the gates run against.
///
/// The worker here leaves a real edit **and** a `.gitattributes` naming a
/// filter, so the diff is non-empty (the first two rungs pass) and the tree is
/// still unreviewable. Without this rung the attempt would be accepted and a
/// candidate pinned from a transformed blob.
#[test]
fn the_driver_refuses_a_tree_a_filter_has_transformed() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::healthy("driver-filtered");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::filtering();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run
        .step(&seams, &mut hooks)
        .expect("the branch settles the refusal");

    let Progress::Settled { accepted, .. } = progress else {
        panic!("the ready-dispatch branch did not settle: {progress:?}");
    };
    assert!(
        !accepted,
        "a tree a filter has transformed was accepted, so the ladder's third \
         cheap rung did not run"
    );

    // **The refusal is the policy's own words, attributed to the reviewer.**
    // `classify::review_input_failure` is the one place that decides what an
    // unreviewable tree means for the attempt, and the message is
    // `Workspace`'s, not a driver paraphrase.
    let failure = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => data.record.failure.clone(),
            _ => None,
        })
        .expect("the settlement records a failure");

    assert_eq!(failure.kind, crate::ladder::FailureKind::ReviewInputOpaque);
    assert_eq!(failure.origin, crate::ladder::FailureOrigin::Reviewer);
    assert!(
        failure.reason.contains("filter"),
        "the reason is not the policy's: {}",
        failure.reason
    );
}

/// **The retaining incarnation takes its next attempt in place.**
///
/// The ready-retry branch, end to end and in two iterations of the loop.
///
/// The first settles `Retained`: the agent reports its own error, which is
/// neither an outage nor a question, so `next_step` retries on the same rung —
/// and `resume: true`, because pre-flight probed the agent as
/// `session_resume` and the attempt returned a session. **Both halves are
/// required**, which is why the caps are given here and were empty everywhere
/// else: with either missing the generation closes and the task retries from a
/// fresh one instead.
///
/// The second is the retry itself: `{pipeline}` reservation, `Worktree.Verify`
/// against the retained tree, `attempt_started(retry)` carrying the session,
/// then the attempt and its settlement.
///
/// `Quiescence::HoldsTree` is the reason this needs a real worktree: a retry
/// verified against the base would pass on a tree that had been reset and would
/// re-gate an empty one as if it were the retained work.
#[test]
fn the_retaining_incarnation_retries_in_place() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    /// The pool this fixture's agent resolves to. Named rather than empty so
    /// that "took the pool from the authority" and "took nothing" are different
    /// observations.
    const RETRY_POOL: &str = "the-retrying-agents-pool";

    let fixture = Fixture::healthy("driver-retries");
    let caps = vec![(
        crate::engine::topology::scaffold::AGENT.to_owned(),
        crate::agent::Caps {
            version: "1.2.3".to_owned(),
            json_output: true,
            session_resume: true,
            cost_reporting: true,
            read_only_mode: true,
            acp: false,
            model_list: false,
        },
    )];
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    // **A pool the implementer's agent resolves to**, because the two
    // `attempt_started` appends below are asserted to carry it. With `pools:
    // &[]` — what this fixture had — `AttemptPlans::pool_for` returns `None`
    // for every agent, so the retry arm's `pool: None` and its repair are
    // indistinguishable, and `run.rs` passing a literal `None` left the whole
    // suite green. Measured, twice: once as `R3-SEAMS-001` and once when round
    // 4 restored the literal.
    let pools = vec![crate::capacity::Pool::discovered(
        RETRY_POOL,
        crate::capacity::PoolKind::SubscriptionWindow,
        crate::engine::topology::scaffold::AGENT,
        vec![crate::capacity::Source::Signals],
    )];
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &pools,
        caps: &caps,
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let first = run
        .step(&seams, &mut hooks)
        .expect("the first attempt settles");
    let Progress::Settled { accepted, .. } = first else {
        panic!("the first iteration did not settle: {first:?}");
    };
    assert!(!accepted, "an agent error is not an acceptable attempt");

    // The generation is retained, not closed: only a retained one is retried in
    // place, and `settle::retry` refuses any other class by name.
    let retained = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => Some(data.settlement),
            _ => None,
        })
        .next_back()
        .expect("the first attempt settled");
    assert!(
        matches!(retained, AttemptSettlement::Retained { .. }),
        "the generation did not retain its session: {retained:?}"
    );

    let second = run
        .step(&seams, &mut hooks)
        .expect("the retry runs in the retained generation");
    let Progress::Settled { key, .. } = second else {
        panic!("the second iteration did not run a retry: {second:?}");
    };
    assert_eq!(key, TaskKey(0));

    // **Two attempts in one generation, the second resuming the first.** A
    // driver that opened a fresh generation would append `task_dispatched`
    // again; a driver that lost the session would append `attempt_started`
    // with none.
    //
    // **And the pool each attempt drained**, which is the field `R3-SEAMS-001`
    // was about and the one no test held. The dispatch arm reads `plan.pool`;
    // the retry arm appends before its plan exists and takes the same answer
    // from `AttemptPlans::pool_for` one step earlier. Both are asserted here, in
    // one run, against the pool the assembler actually resolves — which is the
    // behavioural witness `79cd9c8` said was unavailable because "no driver
    // fixture can reach the arm". This fixture reaches it, and reached it then.
    // `reviews/FINDINGS.md` §19, claims (2) and (3).
    let starts: Vec<(u32, bool, Option<String>)> = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptStarted { data } => Some((
                data.attempt.0,
                data.resume_session.is_some(),
                data.pool.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        starts,
        vec![
            (1, false, Some(RETRY_POOL.to_owned())),
            (2, true, Some(RETRY_POOL.to_owned())),
        ],
        "the retry is not the same generation's second attempt on a resumed session, drawing on \
         the pool the assembler resolves for its agent. A `None` here is the ledger and the plan \
         disagreeing about which subscription one attempt drained"
    );
    assert_eq!(
        durable_kinds(&fixture)
            .iter()
            .filter(|kind| *kind == "task_dispatched")
            .count(),
        1,
        "the retry opened a fresh generation instead of continuing the retained one"
    );

    // **And told what went wrong.** §11.4 sends the failure back to the same
    // rung; the plan hard-coded `retry: None`, so the second attempt got the
    // first attempt's prompt verbatim and no reason to behave differently. A
    // retry that is not informed is a rung's allowance spent to learn nothing.
    let briefed = runner
        .requests()
        .iter()
        .filter(|request| request.role == crate::runner::ExecutionRole::Implement)
        .filter(|request| {
            request
                .command
                .args
                .iter()
                .any(|arg| arg.contains("agent error"))
        })
        .count();
    assert_eq!(
        briefed, 1,
        "exactly one of the two worker prompts should carry the previous \
         attempt's failure, and it is the second"
    );

    // Balance, which says every registration was settled. It does **not** say
    // the reviewers were registered — an empty ledger balances too — so R4's
    // review coverage is asserted where reviewers actually run, in
    // `attempt::tests`.
    assert!(
        run.invocations_balance(),
        "the invocation ledger does not balance, so some process was \
         registered and never settled"
    );

    // **The worker was actually told to resume.** The event records that a
    // session was retained; this records that the command carried it. They are
    // different claims, and a retry that appended the first without the second
    // would re-implement the task from scratch on a worktree that already holds
    // its previous work.
    let resumed = runner
        .requests()
        .iter()
        .filter(|request| request.role == crate::runner::ExecutionRole::Implement)
        .filter(|request| request.command.args.iter().any(|arg| arg == "--resume"))
        .count();
    assert_eq!(
        resumed, 1,
        "exactly one of the two worker invocations should carry a session to \
         resume, and it is the second"
    );
}

/// **A step that refused holds no entitlement afterwards.**
///
/// `permits.protocol` is "every Runner process registered exactly once, settled
/// exactly once", and `append_error_protocol`'s obligation (2) is
/// `Reservations::cancel_any` on any outcome-unknown path. Three catalogue
/// entries take an entitlement **before** the step that can refuse and leak it
/// on the refusing path — `PR7-PIPELINE-014` (the `Dispatch` take moved into the
/// `Ok` arm), `PR7-SELECT-024` (a `Retry` reservation taken before `select`),
/// `PR7-SELECT-033` (an `Integration` pair taken before `checkpoint` refuses) —
/// and all three were green, because nothing asked the ledger anything after a
/// refusal.
///
/// The budget breach is the refusal this drives, because it is the one a fixture
/// can reach without arming an injection: seed a settled attempt that cost
/// something, resume, and set a ceiling below it. That the spend is visible at
/// all is itself new — `Spend::replay` had no production caller until `6d3fc6f`.
#[test]
fn a_refused_step_leaves_no_entitlement_held() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "refused-entitlement",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    // A ceiling the log's own spend has already passed.
    let mut run = TopologyRun::resumed(
        handle,
        fixture.inputs(),
        Ceiling {
            run_usd: Some(0.000_001),
            task_usd: None,
        },
    );
    assert!(
        run.spend().run_total() > 0.000_001,
        "the seeded attempt must cost more than the ceiling, or this test drives \
         an ordinary dispatch and asserts nothing about a refusal"
    );

    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run.step(&seams, &mut hooks).expect("the ceiling refuses");
    assert!(
        matches!(progress, Progress::BudgetExceeded),
        "the seeded spend did not breach the ceiling: {progress:?}"
    );
    assert!(
        !run.holds_entitlement(),
        "the refused step is still holding a pipeline entitlement. At \
         `max_parallel = 1` that is the whole pipeline, held by a step that \
         did nothing"
    );
}

/// **A retried worker is told what the last one failed on.**
///
/// §11.4, quoted on `PlanRequest::feedback` itself: "failure feedback goes back
/// to the same rung, and an escalation carries the accumulated feedback with
/// it."
///
/// The driver accumulated the brief inside [`Retained`], which `settle::retry`
/// produces **only** for a resumable same-rung retry that returned a session.
/// Every escalation and every sessionless retry — which is every Copilot
/// attempt, `DESIGN.md:452` — therefore dispatched with `feedback: Vec::new()`
/// and handed the next worker attempt 1's prompt verbatim: a rung's allowance
/// spent to be told nothing. Found by round 2's `contract`, `seams` and
/// `attempt` lenses independently.
///
/// This drives **two** attempts through the real assembler and asserts on the
/// second worker's own stdin, because the prompt is the only place the claim is
/// observable — a brief the driver holds and does not send is the defect.
#[test]
fn a_retried_worker_is_told_what_the_last_attempt_failed_on() {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let fixture = Fixture::build(
        "driver-brief",
        Damage {
            two_tier: true,
            ..Damage::default()
        },
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    // Attempt one fails and, with one attempt per rung, escalates onto rung 1.
    run.step(&seams, &mut hooks)
        .expect("the first attempt settles");
    // Attempt two, on the rung above, from a fresh generation.
    run.step(&seams, &mut hooks)
        .expect("the second attempt settles");

    let prompts: Vec<String> = runner
        .requests()
        .into_iter()
        .filter(|request| request.role == crate::runner::ExecutionRole::Implement)
        .map(|request| String::from_utf8_lossy(&request.command.stdin).into_owned())
        .collect();
    assert!(
        prompts.len() >= 2,
        "the fixture ran {} implementer(s); this test needs a second attempt to \
         have a prompt at all",
        prompts.len()
    );
    assert!(
        prompts[1].len() > prompts[0].len(),
        "the second worker's prompt is no longer than the first's, so nothing \
         was carried forward:\n--- first ---\n{}\n--- second ---\n{}",
        prompts[0],
        prompts[1]
    );
}

/// The order the funnels ran in, for the **driver's** composition of the
/// candidate sequence.
///
/// `candidate.rs` has one of these and it covers `candidate::promote`. The
/// driver assembles the same four steps from the three split halves
/// (`create_candidates_ref`, `append_candidate_created`,
/// `reclaim_after_creation`), and until this existed **no ordering assertion
/// reached that composition** — the only two `trace.order(` calls in the
/// topology engine were both in `candidate.rs`. Found by S5 round 2's catalogue:
/// four `PR7-PIPELINE-*` mutations that reorder the driver's sequence were green,
/// while `pin_pruned_after_promotion` would have caught every one of them on the
/// other path.
#[derive(Clone, Default)]
struct Timeline(Arc<Mutex<Vec<String>>>);

impl Timeline {
    fn push(&self, site: EffectSiteId, phase: HookPhase) {
        // `Before` only: one entry per funnel, at the point it begins, which is
        // what an ordering clause is about.
        if phase != HookPhase::Before {
            return;
        }
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(site.to_string());
    }

    /// The recorded sequence with everything not `of_interest` dropped.
    ///
    /// Filtered rather than compared whole: a driver step also creates an
    /// execution root, writes an intent and adds a worktree, and an assertion
    /// over the unfiltered list would be an assertion about the fixture.
    fn order(&self, of_interest: &[EffectSiteId]) -> Vec<String> {
        let names: Vec<String> = of_interest.iter().map(ToString::to_string).collect();
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|seen| names.contains(seen))
            .cloned()
            .collect()
    }
}

struct TracedEffects {
    inner: crate::workspace_manager::HarnessEffects,
    timeline: Timeline,
}

impl crate::workspace_manager::EffectHooks for TracedEffects {
    fn phase(
        &mut self,
        site: EffectSiteId,
        phase: HookPhase,
    ) -> crate::topology::effects::Injection {
        let answered = self.inner.phase(site, phase);
        self.timeline.push(site, phase);
        answered
    }

    fn durability_ledger(&self) -> crate::util::DurabilityLedger {
        self.inner.durability_ledger()
    }
}

struct TracedEvents {
    inner: crate::events::log::HarnessEventHooks,
    timeline: Timeline,
}

impl crate::events::log::EventHooks for TracedEvents {
    fn phase(&mut self, site: crate::topology::effects::EventSite, phase: HookPhase) {
        self.inner.phase(site, phase);
        self.timeline.push(EffectSiteId::Event(site), phase);
    }
}

/// [`HarnessTopologyHooks`] with the two families an ordering clause spans
/// recorded onto one timeline.
struct TracedHooks {
    effects: TracedEffects,
    events: TracedEvents,
    rest: HarnessTopologyHooks,
    timeline: Timeline,
}

impl TracedHooks {
    fn new(harness: &Arc<Mutex<HookHarness>>) -> Self {
        let timeline = Timeline::default();
        Self {
            effects: TracedEffects {
                inner: crate::workspace_manager::HarnessEffects::new(Arc::clone(harness)),
                timeline: timeline.clone(),
            },
            events: TracedEvents {
                inner: crate::events::log::HarnessEventHooks::new(Arc::clone(harness)),
                timeline: timeline.clone(),
            },
            rest: HarnessTopologyHooks::new(Arc::clone(harness)),
            timeline,
        }
    }
}

impl TopologyHooks for TracedHooks {
    fn effects(&mut self) -> &mut dyn crate::workspace_manager::EffectHooks {
        &mut self.effects
    }

    fn rundir(&mut self) -> &mut dyn crate::rundir::RunDirHooks {
        self.rest.rundir()
    }

    fn events(&mut self) -> &mut dyn crate::events::log::EventHooks {
        &mut self.events
    }

    fn container(&mut self) -> &mut dyn crate::runner::container::ContainerHooks {
        self.rest.container()
    }

    fn spawn(&mut self) -> &mut dyn crate::agent::proc::SpawnHooks {
        self.rest.spawn()
    }
}

/// **The driver writes the rung an escalation climbs ONTO, not the one it leaves.**
///
/// The **write** side of `PR7-FOLD-LADDER-POSITION`'s class, and the exact
/// mirror of the read-side gap closed at `6d3fc6f`. That repair witnessed the
/// driver *reading* `ladder_position`; nothing witnessed the driver *writing*
/// the escalation target, and `PR7-R3-ESCALATED-RUNG-WRITER-UNPINNED` is what
/// got through: replacing `rung: position.0.saturating_add(1)` with
/// `rung: position.0` left the whole suite green.
///
/// The consequence of that mutation is not a wrong number in a record. The fold
/// assigns `task.rung = *rung` and resets the allowance, so the task escalates
/// onto the rung it is **leaving**, `ready` selects it again, the binding
/// resolves, and it loops without bound — never reaching the tier its chain
/// escalated it to and never exhausting the chain.
///
/// **Written as the round trip, which is the class boundary.** Asserting the
/// recorded number alone would pin the write and leave the same gap one step
/// over. This drives the escalation, reads the durable settlement, and then
/// drives the *next* attempt and asserts the model it actually ran at — so the
/// value the driver wrote and the value it later reads are held by one test.
#[test]
fn the_driver_escalates_onto_the_rung_above() {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    // Two tiers, one attempt per rung: the first failure exhausts rung 0.
    let fixture = Fixture::build(
        "driver-escalates",
        Damage {
            two_tier: true,
            ..Damage::default()
        },
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    // Attempt one, on rung 0, fails and exhausts the rung's allowance.
    run.step(&seams, &mut hooks)
        .expect("the first attempt settles");

    let escalated = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => match data.settlement {
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Escalated { rung },
                    ..
                } => Some(rung),
                _ => None,
            },
            _ => None,
        })
        .expect("the exhausted rung escalates");

    assert_eq!(
        escalated, 1,
        "the driver recorded an escalation onto rung {escalated}, the rung it is \
         leaving. The fold assigns `task.rung` from this number and resets the \
         allowance, so the task is selected again at the same tier and loops \
         forever — never reaching the tier its chain escalated it to"
    );

    // And the read side, in the same test: the next attempt runs at rung 1's
    // binding. Pinning the written number alone would leave the same gap one
    // step over, which is how this class keeps recurring.
    run.step(&seams, &mut hooks)
        .expect("the second attempt settles");
    let ran_at = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptStarted { data } => Some(data.binding.model.clone()),
            _ => None,
        })
        .next_back()
        .expect("the driver started a second attempt");
    assert_eq!(
        ran_at, "claude-fable-5",
        "the escalated task ran at {ran_at}, which is rung 0's model"
    );

    // **And the third step exhausts the chain, which is where the human is
    // told a number.** This is the class boundary the frontier review of
    // `75da796` (finding 3) found unguarded: `park_question` hard-coded
    // `rungs_spent: 1` and passed *this rung's* attempts as the total, so a
    // two-rung exhaustion said "1 attempt(s) across 1 rung(s) all failed" when
    // two attempts across two rungs had. Nothing asserted it — this file's
    // two-rung test stopped at the escalated model, its count test drove a
    // single rung, and **no topology test asserted `rung(s)` at all**.
    //
    // Asserted here rather than in a fixture of its own because the numbers are
    // only true of a task that has *actually* climbed: a single-rung fixture
    // reports 1 and 1 whether the code derives them or hard-codes them, which
    // is exactly how the constant survived.
    run.step(&seams, &mut hooks)
        .expect("the exhausted chain settles");
    let parked = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => match data.settlement {
                AttemptSettlement::Closed {
                    transition: SettlementTransition::Parked { question },
                    ..
                } => Some(question),
                _ => None,
            },
            _ => None,
        })
        .expect("the exhausted chain parks a question");

    assert!(
        parked.context.contains("2 attempt(s) across 2 rung(s)"),
        "the human is told the wrong history of this task. Two attempts across \
         two rungs failed; the question says:\n{}",
        parked.context
    );
}

/// **The driver dispatches at the rung the log records, not at rung 0.**
///
/// The **other** driver-side half of `PR7-FOLD-LADDER-POSITION`, and the one
/// that stayed open through the repair filed against it.
/// `the_driver_spends_the_allowance_the_log_records` witnesses the
/// `attempts_on_rung` half of the same reader; nothing witnessed `rung`, because
/// that fixture's chain has one tier and a one-tier chain makes rung 0 the only
/// rung there is. Measured at `cf22a8c`: replacing the driver's
/// `self.ladder_position(key)?.0` with a literal `0` failed **no** topology
/// test.
///
/// That is occurrence 4 of `reviews/FINDINGS.md` §4's accumulator class, and the
/// sharpest argument for its re-scoping: the class was filed *from* this
/// instance, and half of this instance was still open.
///
/// The fold half — `fold::tests::a_ladder_position_is_derived_by_replay_and_not_assumed`
/// — already states the consequence in words: "A driver that assumed rung 0
/// would dispatch an escalated task on rung 0 forever, never reaching the tier
/// its chain escalated it to." This asserts it.
#[test]
fn the_driver_dispatches_at_the_rung_the_log_records() {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    // A two-tier chain, one attempt per rung, and an attempt already escalated
    // off rung 0 in the durable log. The task is `Pending` on **rung 1**.
    let fixture = Fixture::build(
        "driver-rung",
        Damage {
            two_tier: true,
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Escalated { rung: 1 },
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    run.step(&seams, &mut hooks).expect("the attempt settles");

    // **The model the attempt actually ran at.** The rung selects the binding,
    // and the two tiers of this chain differ by model — so this is the rung,
    // observed rather than asserted about itself.
    let ran_at = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptStarted { data } => Some(data.binding.model.clone()),
            _ => None,
        })
        .next_back()
        .expect("the driver started an attempt");

    assert_eq!(
        ran_at, "claude-fable-5",
        "the task escalated onto rung 1 and the driver ran it at {ran_at}, which \
         is rung 0's model. An escalated task dispatched at rung 0 never reaches \
         the tier its chain escalated it to, and the only symptom is a task that \
         never gets better"
    );
}

/// **A crash does not erase what the last attempt was told to fix.**
///
/// §11.4's first half: "failure feedback (gate log or `required_changes`) goes
/// back to the *same rung*". The brief that carries it was a process-local
/// `BTreeMap` the live loop pushed to, and `TopologyRun::resumed` created it
/// **empty** — so the sequence the 2026-08-26 frontier review of `75da796` set
/// out in finding 2 held exactly: attempt 1 fails a gate with an 8-KiB
/// diagnostic tail, `attempt_finished` is durably appended, the conductor
/// crashes before the next dispatch, and the retry is handed attempt 1's prompt
/// verbatim. A rung's allowance spent to be told nothing, and the same defect
/// free to repeat.
///
/// The fixture is that crash: one attempt already settled in the durable log,
/// carrying the tail, and a chain with a second attempt left on the rung. The
/// process that wrote it is gone — this run is built by `resumed` from the log
/// alone, which is the only path a real resume has.
///
/// **Asserted on the worker's own stdin**, because the prompt is the only place
/// the claim is observable: a brief the driver rebuilds and does not send is the
/// same defect one rung further along. And asserted on the tail's exact text
/// rather than on the prompt's length — a longer prompt is evidence that
/// *something* was carried, which is what the live-mode witness
/// `a_retried_worker_is_told_what_the_last_attempt_failed_on` could say and is
/// not what §11.4 requires.
#[test]
fn a_crash_does_not_erase_what_the_last_attempt_was_told_to_fix() {
    // §11.1's payload: what the gate printed, not the summary of it.
    const TAIL: &str = "error[E0308]: mismatched types\n  --> src/alpha.rs:12:9\n   \
                        expected `u32`, found `&str`";

    // One tier, `attempts_per = 2`: the retry the log entitles this run to is on
    // the **same rung**, which is the half of §11.4 this test is about.
    let fixture = Fixture::build(
        "driver-brief-resume",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished_failing(
                    1,
                    crate::ladder::FailureKind::GateFailed,
                    "gate `cargo test` failed: 1 failed",
                    TAIL,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    let runner = RecordingRunner::editing();
    let prompts = drive_one_attempt(&fixture, &runner);

    assert_eq!(
        prompts.len(),
        1,
        "the resumed run dispatched {} implementer(s); this test needs exactly \
         the one the log entitles it to",
        prompts.len()
    );
    assert!(
        prompts[0].contains(TAIL),
        "the retry after the crash was not told what the gate printed. §11.4 \
         sends the gate log back to the same rung, and this prompt carries none \
         of it:\n--- prompt ---\n{}",
        prompts[0]
    );
}

/// **An escalation after a crash carries the accumulated feedback.**
///
/// §11.4's second half: "`attempts_per` exhausted → next rung, fresh session,
/// **accumulated feedback summary included**", and its other named source — the
/// reviewer's `required_changes`, which §11.2 says the retry gets back verbatim.
///
/// Two failures are already durable on rung 0 and the ladder has a rung above,
/// so the only dispatch this run can make is the escalation. The empty brief a
/// resume used to rebuild sent it none of them: a fresh, stronger worker on the
/// same task, given attempt 1's prompt and no reason to do anything different.
///
/// **What "accumulated" means here is what `feedback_section` actually sends**,
/// and this asserts that rather than a stronger reading of the sentence. Every
/// earlier attempt contributes its summary line; only the newest carries its
/// full detail, because "older ones would bury it, and the newest is the one
/// still standing in the way" — the production comment on that decision. So the
/// claim under test is: both summaries reach the rung above, the newest
/// reviewer's required changes reach it verbatim, and the accumulated section
/// exists at all — `feedback_section` writes its header **only** when it is
/// rendering more than one entry for a fresh rung, so that sentence is the
/// accumulation itself and not a statement about it.
#[test]
fn an_escalation_after_a_crash_carries_the_accumulated_feedback() {
    const FIRST_SUMMARY: &str = "review failed: the parser accepts a trailing comma";
    const SECOND_SUMMARY: &str = "review failed: the empty list still panics";
    const SECOND_DETAIL: &str = "- reject a trailing comma in `parse_list`\n\
                                 - the empty list must round-trip";

    let fixture = Fixture::build(
        "driver-brief-escalate",
        Damage {
            deep_ladder: true,
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished_failing(
                    1,
                    crate::ladder::FailureKind::ReviewFailed,
                    FIRST_SUMMARY,
                    "- reject a trailing comma in `parse_list`",
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
                // A closed generation cannot take another attempt, so the
                // second failure is in the generation the retry opened —
                // which is exactly what a sessionless retry leaves in a log.
                in_generation(GenerationId(1), dispatched()),
                in_generation(GenerationId(1), attempt_started(1)),
                in_generation(
                    GenerationId(1),
                    attempt_finished_failing(
                        1,
                        crate::ladder::FailureKind::ReviewFailed,
                        SECOND_SUMMARY,
                        SECOND_DETAIL,
                        AttemptSettlement::Closed {
                            transition: SettlementTransition::Escalated { rung: 1 },
                            lease: LeaseDisposition::PredictedReleased,
                        },
                    ),
                ),
            ],
            ..Damage::default()
        },
    );

    let runner = RecordingRunner::editing();
    let prompts = drive_one_attempt(&fixture, &runner);
    assert_eq!(
        prompts.len(),
        1,
        "the resumed run dispatched {} implementer(s); this test needs the \
         escalation the log entitles it to",
        prompts.len()
    );
    let prompt = &prompts[0];

    for summary in [FIRST_SUMMARY, SECOND_SUMMARY] {
        assert!(
            prompt.contains(summary),
            "the escalated worker was not told `{summary}`. §11.4 carries the \
             accumulated feedback onto the next rung, and this prompt carries \
             part of it at best:\n--- prompt ---\n{prompt}"
        );
    }
    assert!(
        prompt.contains(SECOND_DETAIL),
        "the escalated worker was not given the reviewer's required changes \
         verbatim. §11.2 is what the retry gets back, and after a crash it \
         reached this prompt as a summary or not at \
         all:\n--- prompt ---\n{prompt}"
    );
    assert!(
        prompt.contains("Earlier attempts at this task failed"),
        "the escalated worker's prompt has no accumulated section, so at most \
         one record below its rung reached it:\n--- prompt ---\n{prompt}"
    );
}

/// **A log written before the field existed still folds, and still resumes.**
///
/// `FailureRecord::detail` is additive and `SCHEMA_VERSION` does not move, which
/// is a claim about *older logs* rather than about new ones:
/// `decisions/2026-08-26-durable-retry-feedback.md` argues that a line without
/// the key reads back as `None`, folds unchanged, and passes schema 4's strict
/// door — the door being a witness comparison that reports "any key the input
/// carried that the record did not claim back", so an added output key is not an
/// unknown input key.
///
/// That argument is worth exactly as much as a log that tests it. This deletes
/// the key from every `attempt_finished` in a real fixture's bytes — the shape a
/// binary one commit older wrote — and resumes from the result through the
/// production parse.
#[test]
fn a_log_predating_the_detail_field_folds_and_resumes() {
    let fixture = Fixture::build(
        "driver-brief-oldlog",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished_failing(
                    1,
                    crate::ladder::FailureKind::GateFailed,
                    "gate `cargo test` failed: 1 failed",
                    "error[E0308]: mismatched types",
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );

    // The older binary's bytes: the same log with the key it never wrote.
    let current = String::from_utf8(fixture.log_bytes()).expect("the log is utf-8");
    let aged: String = current
        .lines()
        .enumerate()
        .map(|(position, line)| {
            // **The first line is passed through byte for byte.** The commit
            // record pins its sha256, and a re-serialization that only reorders
            // keys is enough to make recovery refuse for a reason that has
            // nothing to do with this field.
            if position == 0 {
                return format!("{line}\n");
            }
            let mut value: serde_json::Value =
                serde_json::from_str(line).expect("every log line is a json object");
            // Nested rather than a let-chain: MSRV is 1.85 and let-chains
            // are 1.88.
            if let Some(failure) = value.pointer_mut("/data/record/failure") {
                if let Some(object) = failure.as_object_mut() {
                    object.remove("detail");
                }
            }
            format!("{value}\n")
        })
        .collect();
    assert!(
        !aged.contains("\"detail\""),
        "the aged log still carries a detail key, so this test is reading the \
         current shape and proving nothing about the older one"
    );
    assert!(
        aged.contains("attempt_finished"),
        "the aged log has no settlement in it, so the field being absent is \
         vacuous"
    );

    let events = TopologyFold::parse_log(aged.as_bytes()).expect(
        "a log written before the detail field existed still parses — if this \
         refuses, the field is not additive and SCHEMA_VERSION had to move",
    );
    let details: Vec<Option<String>> = events
        .iter()
        .filter_map(|event| match &event.body {
            TopologyEventBody::AttemptFinished { data } => {
                Some(data.record.failure.as_ref()?.detail.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        details,
        vec![None],
        "an absent detail key must read back as None; anything else means an \
         older log folds to a different value than it was written with"
    );

    // And the run it describes still resumes: the brief is simply empty for the
    // attempts that predate the field, which is the honest answer for a log that
    // never recorded what they were told.
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    // `std::fs::write` is on the effect denylist here; the fixture writer is
    // the sanctioned way a test plants bytes.
    crate::workspace_manager::fixture::write_file(&fixture.log(), aged.as_bytes());
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    outcome.expect("a run whose log predates the field still resumes");
}

/// Resume the fixture from its log alone, take one step, and return the
/// implementer prompts the run actually sent.
///
/// The whole apparatus of the driver tests above, factored out because the two
/// crash-resume witnesses differ only in what their logs already hold and in
/// what they expect to come back. **Recovery is not stubbed**: this is
/// `resume_holding` into `TopologyRun::resumed`, the same pair every other
/// driver test in this file uses, so the brief under test is rebuilt from the
/// barrier's own parse of the durable bytes.
fn drive_one_attempt(fixture: &Fixture, runner: &RecordingRunner) -> Vec<String> {
    use crate::engine::topology::run::{RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    run.step(&seams, &mut hooks)
        .expect("the resumed attempt settles");

    runner
        .requests()
        .into_iter()
        .filter(|request| request.role == crate::runner::ExecutionRole::Implement)
        .map(|request| String::from_utf8_lossy(&request.command.stdin).into_owned())
        .collect()
}

/// **The driver spends the allowance the log records, not the one it assumed.**
///
/// The driver-level half of `PR7-FOLD-LADDER-POSITION`. The fold half is
/// `fold::tests::a_ladder_position_is_derived_by_replay_and_not_assumed`; this
/// is the read, and it needed a fixture that makes the read observable.
///
/// This fixture's chain has **one** tier and `attempts_per = 2`, so an
/// escalation has nowhere to climb and the allowance is the whole ladder. One
/// attempt is already durable in the log. The driver's next attempt is
/// therefore the **second** on that rung, which exhausts the allowance, and
/// `next_step` has no rung to escalate onto — so the task fails terminally.
///
/// A driver that assumed `attempts_on_rung: 1` would hand `next_step` the first
/// attempt of two and get `RetrySameRung` instead: the task would retry
/// forever, spending a rung's allowance on every restart and never failing.
/// That is what the constant did before this test existed.
#[test]
fn the_driver_spends_the_allowance_the_log_records() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    // One attempt already spent on rung 0, settled as a same-rung retry so the
    // task returns to `Pending` and this branch selects it again.
    let fixture = Fixture::build(
        "driver-allowance",
        Damage {
            extra: vec![
                dispatched(),
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Retry,
                        lease: LeaseDisposition::PredictedReleased,
                    },
                ),
            ],
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let progress = run
        .step(&seams, &mut hooks)
        .expect("the second attempt settles");
    let Progress::Settled { accepted, .. } = progress else {
        panic!("the ready-dispatch branch did not settle: {progress:?}");
    };
    assert!(!accepted, "an agent error is not an acceptable attempt");

    let last = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .filter_map(|event| match event.body {
            TopologyEventBody::AttemptFinished { data } => Some(data.settlement),
            _ => None,
        })
        .next_back()
        .expect("the attempt settled");

    let AttemptSettlement::Closed { transition, .. } = last else {
        panic!("the second attempt did not close its generation: {last:?}");
    };
    // **Parked, not failed** — and that is `next_step`'s answer, not a
    // weakening of the assertion. A spent chain asks a human rather than
    // failing the task: "Nothing further can move this task ... and the
    // escalation chain is spent." What matters here is that the allowance was
    // seen as spent at all.
    let SettlementTransition::Parked { question } = transition else {
        panic!(
            "the second attempt on a two-attempt rung with nowhere to escalate \
             settled as {transition:?}. A driver reading a constant \
             `attempts_on_rung: 1` gets `Retry` here and the task retries forever"
        );
    };

    // **And the human is told how many attempts actually ran.** The count in
    // the question is the task's spend on this rung, not the new generation's
    // attempt number — a park that said "1 attempt" after two would send an
    // operator looking for a run that had barely started.
    assert!(
        question.context.contains("2 attempt(s)"),
        "the question quotes the wrong attempt count: {}",
        question.context
    );
}

/// **The loop continues an attempt in a generation recovery recreated.**
///
/// `T-DISPATCH`'s `resume_action` in its own words: "verify the worktree at the
/// recorded base ... or remove it with force and recreate it ... **continue
/// attempt (no spend repeats)**".
///
/// Step (g) recreated those worktrees and nothing then started an attempt in
/// them. `fold::ready` excludes the task — correctly, since a task with an open
/// generation is not *ready to be dispatched* — and `ready_retry` wants
/// `RetainedIdle`, so no branch could select it. The run stalled with its only
/// pipeline entitlement held by a generation nothing could drive, and the loop
/// fell through to a closure it refuses.
///
/// `fold::open_no_attempt` is the predicate that makes it selectable, and
/// `resume_open_no_attempt` — which had no production caller — is what reuses
/// the ground.
#[test]
fn the_loop_continues_an_attempt_recovery_recreated() {
    use crate::engine::topology::run::{Progress, RunSeams, TopologyRun};
    use crate::engine::topology::select::Ceiling;

    // Killed after `task_dispatched`, before `attempt_started`.
    let fixture = Fixture::build(
        "continue-open",
        Damage {
            open_generation: true,
            ..Damage::default()
        },
    );
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let mut run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    let mut hooks = HarnessTopologyHooks::new(Arc::clone(&harness));
    let sleeper = RecordingSleeper::default();
    let manager = fixture.manager();
    let runner = RecordingRunner::editing();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::erroring();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **Through the production assembler, not a fixture plan shape.** The
    // condition on this extraction was that the scaffold be re-pointed at the
    // real one or round-tripped against it; a fixture that hand-built an
    // `AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
    // precedent warns about.
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &[],
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let seams = RunSeams {
        manager: &manager,
        clock: &Frozen,
        sleeper: &sleeper,
        runner: &runner,
        adapters: &adapters,
        paths: &paths,
        plans: &plans,
        reviews: &crate::engine::attempt::LegacyReviewPasses,
        input_policy: &crate::engine::attempt::LegacyReviewInputPolicy,
        answers: &crate::interaction::UnattendedAnswers,
        ids: &FixedIds,
        halts_run: false,
    };

    let before = durable_kinds(&fixture);
    assert_eq!(
        before.iter().filter(|k| *k == "task_dispatched").count(),
        1,
        "the fixture must leave exactly one dispatch for the continuation to reuse"
    );

    let progress = run
        .step(&seams, &mut hooks)
        .expect("the loop continues the attempt rather than stalling");
    let Progress::Settled { key, .. } = progress else {
        panic!("the ready-dispatch branch did not continue the attempt: {progress:?}");
    };
    assert_eq!(key, TaskKey(0));

    // **No spend repeats**, in `T-DISPATCH`'s own words: the generation was
    // already open, so continuing it appends an attempt and never a second
    // `task_dispatched`.
    let after = durable_kinds(&fixture);
    assert_eq!(
        after.iter().filter(|k| *k == "task_dispatched").count(),
        1,
        "the continuation opened a fresh generation instead of continuing the \
         one recovery recreated — `T-DISPATCH` says continue attempt, no spend \
         repeats"
    );
    assert_eq!(
        after.iter().filter(|k| *k == "attempt_started").count(),
        1,
        "the continuation started no attempt, so the entitlement is still held \
         by a generation nothing can drive"
    );
}

/// **A reviewer runs at §10's review effort, not the implementer's.**
///
/// `ResolvedEffortPolicy` has four axes and `review` is one of them: the tier a
/// rung binds decides what the *work* costs, and review has its own budget.
/// `FrozenPlans` passed `request.binding.effort` — the implementer's — while its
/// own comment said "the reviewer's effort, not the implementer's". A comment
/// asserting the opposite of its line is worse than none: it answers the
/// question a reader would otherwise ask.
///
/// This fixture's Mid rung is `High` and its review axis is `Medium`, so the two
/// are distinguishable. A fixture where they matched would assert nothing.
#[test]
fn a_reviewer_runs_at_the_review_effort_not_the_implementers() {
    let fixture = Fixture::healthy("review-effort");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    let entry = handle
        .fold
        .registry()
        .and_then(|registry| registry.get(TaskKey(0)))
        .expect("the fixture registers alpha");
    assert_eq!(
        entry.ladder.effort.review,
        Effort::Medium,
        "the fixture's review axis moved; this test needs it to differ from the rung's"
    );
    assert_eq!(
        entry
            .ladder
            .effort
            .implementation_for(entry.ladder.rungs[0].tier),
        Effort::High,
        "the fixture's Mid rung moved; this test needs it to differ from review"
    );

    let manager = fixture.manager();
    let adapters = crate::engine::topology::scaffold::ScaffoldAdapters::new();
    let paths = crate::rundir::RunPaths::with_private_root(
        &fixture.repo_root,
        &fixture.started.run_id,
        &fixture.private_root,
    );
    paths.create().expect("the run directories are creatable");
    // **The reviewer is bound to a different agent than the implementer, and it
    // has to be.** The comment here used to say the single pool was "named so
    // that a plan inheriting the implementer's pool and one looking up the
    // reviewer's own could not both pass" — and the fixture's primary reviewer
    // is `(claude-code, opus)` while its rung-0 implementer is
    // `(claude-code, alpha-Mid-model)`. `review::passes_for` rebinds only on
    // **exact `(agent, model)` equality**, so it does not fire and the pass
    // keeps agent `claude-code`: the two lookups were the same lookup, both
    // behaviours passed, and the mutation recorded as killed died because the
    // pool became *empty* rather than wrong. `reviews/FINDINGS.md` §19,
    // claim (8).
    //
    // With `REVIEW_AGENT` on the primary and a pool for each agent, inheriting
    // the implementer's pool yields `the-implementers-pool` and fails.
    //
    // Through the scaffold's own constant rather than a literal: it is the
    // agent that fixture's `alternative` binding already names, so this is the
    // second agent the run actually probed and not one invented here.
    use crate::engine::topology::scaffold::REVIEW_AGENT;

    let mut reviewer_bound = entry.clone();
    reviewer_bound.reviews.primary = Some(crate::review::PassBinding::new(REVIEW_AGENT, "gpt"));
    let entry = &reviewer_bound;
    let pools = vec![
        crate::capacity::Pool::discovered(
            "the-implementers-pool",
            crate::capacity::PoolKind::SubscriptionWindow,
            AGENT,
            vec![crate::capacity::Source::Signals],
        ),
        crate::capacity::Pool::discovered(
            "the-reviewers-own-pool",
            crate::capacity::PoolKind::SubscriptionWindow,
            REVIEW_AGENT,
            vec![crate::capacity::Source::Signals],
        ),
    ];
    let plans = crate::engine::assembly::FrozenPlans {
        adapters: &adapters,
        paths: &paths,
        gates: &[],
        pools: &pools,
        caps: &[],
        worker_timeout: std::time::Duration::from_secs(300),
        decisions: &[],
    };
    let binding = handle
        .fold
        .frozen_rung_binding(TaskKey(0), 0)
        .expect("rung 0 is frozen");
    let plan = crate::engine::topology::attempt::AttemptPlans::plan(
        &plans,
        &crate::engine::topology::attempt::PlanRequest {
            key: TaskKey(0),
            entry,
            attempt: crate::topology::events::AttemptNumber(1),
            rung: 0,
            binding,
            workspace: &fixture.repo_root,
            resume_session: None,
            feedback: Vec::new(),
            materialization_observed: None,
        },
    )
    .expect("the plan assembles");

    assert!(
        !plan.reviewers.is_empty(),
        "this fixture plans no reviewer, so the effort below is unasserted"
    );
    // The implementer's own pool, so the two values in play are distinguishable
    // and the assertion below is about which one the reviewer got.
    assert_eq!(
        plan.pool.as_deref(),
        Some("the-implementers-pool"),
        "the implementer did not resolve its own agent's pool, so a reviewer carrying that value \
         would not tell us anything"
    );
    for reviewer in &plan.reviewers {
        assert_eq!(
            reviewer.agent.as_str(),
            REVIEW_AGENT,
            "reviewer `{}` runs on the implementer's agent, so its pool lookup and the \
             implementer's are one lookup and both behaviours pass",
            reviewer.lens.name()
        );
        assert_eq!(
            reviewer.profile.effort,
            Some(Effort::Medium),
            "reviewer `{}` runs at the implementer's effort",
            reviewer.lens.name()
        );

        // **And its own agent's pool**, which is the other cell of
        // `a_reviewers_profile_is_accounted_for_at_both_callers` whose value the
        // extraction dropped. That census checks the roll is complete and cannot
        // check a value — a cell is prose. This is the value.
        //
        // §11.3/§13: a cross-vendor second opinion draws on a different
        // subscription than the implementer, so the pool is looked up from the
        // reviewer's own agent. `coordinator.rs` did it and `assembly.rs` did
        // not, leaving `profile_for`'s empty string — so the capacity engine
        // attributed a reviewer's spend to a pool with no name. Sol's
        // independent `seams` read, round 3.
        assert_eq!(
            reviewer.profile.pool,
            "the-reviewers-own-pool",
            "reviewer `{}` carries pool `{}`",
            reviewer.lens.name(),
            reviewer.profile.pool
        );
    }
    let _ = manager;
}

/// **The loop inherits the digest recovery verified.**
///
/// `committed.json.run_started_sha256` is what step (a) checks the committed
/// first line against, and the append-error protocol reads it back: the creator
/// disposition is a projection of the outcome onto the run's *commitment*
/// boundary, and a run that cannot say whether it is committed cannot report
/// one.
///
/// Recovery's own emitter passes `Some(...)`. `TopologyRun::resumed` passed
/// `None` — so over one run, the two emitters disagreed about whether it was
/// committed, and only the loop's appends lost the answer. Nothing observed it
/// because nothing compared them.
#[test]
fn the_loop_inherits_the_committed_digest_recovery_verified() {
    let fixture = Fixture::healthy("digest-inherited");
    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);

    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the healthy resume completes");

    // Computed independently from the run's own committed first line, rather
    // than read back from the record the handle came through: a comparison of
    // the record with itself would pass however the digest was carried.
    let expected = crate::rundir::run_started_sha256(&fixture.first_line);

    assert_eq!(
        handle.committed_first_line_sha256, expected,
        "the handle carries a digest that is not the committed first line's"
    );
    assert!(
        !handle.committed_first_line_sha256.is_empty(),
        "an empty digest asserts nothing: this fixture must publish a commit \
         record for the comparison to mean anything"
    );

    // **And it survives into the loop's own identity**, which is the hop that
    // matters: `establish_stable_prefix` skips its check entirely when the
    // digest is `None`, so a loop that carried the handle's value and then
    // dropped it would reopen after an append error and accept a first line the
    // commit record does not name. `PR31-CONTRACT-006`.
    let run = crate::engine::topology::run::TopologyRun::resumed(
        handle,
        fixture.inputs(),
        crate::engine::topology::select::Ceiling::unlimited(),
    );
    assert_eq!(
        run.commitment_digest(),
        Some(expected.as_str()),
        "the loop's appends cannot prove their committed first line"
    );
}

/// **Erratum E6: a resume converges the settled-but-unrecorded candidate.**
///
/// The window `attempt_finished{Closed{Succeeded}}` durable, `candidate_prepared`
/// absent. The fold makes it mandatory — that settlement is the only thing that
/// sets `Promoting` and `check_candidate_prepared` refuses every other class —
/// and before E6 it was governed by no fault-matrix row: `T-CAND-OBJ` ends at
/// "attempt_started only, attempt unsettled", and `T-CAND-REF` used to begin at
/// `candidate_prepared`. Recovery **refused**, which was a third checkpoint
/// refusal where the packet authorises exactly two.
///
/// E6 moves `T-CAND-REF`'s boundary to the settlement, and that row converges
/// forward. Every input is derived from durable state: the pin names the commit,
/// the commit names its tree and message, the generation names the base,
/// `diff-tree base commit` names the region. Nothing is re-decided.
#[test]
fn a_resume_converges_a_settled_candidate_that_was_never_recorded() {
    let fixture = Fixture::build(
        "e6-converges",
        Damage {
            // **The dispatch comes from `open_generation`, not from `extra`.**
            // It records the fixture's real base, and the convergence diffs the
            // candidate commit against it — a placeholder sha makes `diff-tree`
            // fail rather than report a region. Measured.
            open_generation: true,
            extra: vec![
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Succeeded,
                        lease: LeaseDisposition::PredictedRetained,
                    },
                ),
            ],
            ..Damage::default()
        },
    );
    let commit = seed_candidate_commit(&fixture, 0);

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (recovered, _handle) = outcome.expect("the resume converges rather than refusing");

    assert_eq!(
        recovered.promoted,
        vec![TaskKey(0)],
        "recovery did not converge the promoting generation"
    );

    // **The candidate names the object the pin named.** A convergence that
    // invented an identity would still append something; this is what makes the
    // append the settlement's candidate rather than a new one.
    let prepared = TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .into_iter()
        .find_map(|event| match event.body {
            TopologyEventBody::CandidatePrepared { data } => Some(data),
            _ => None,
        })
        .expect("`candidate_prepared` is durable after the convergence");

    assert_eq!(
        prepared.commit_sha.0, commit,
        "a different commit was named"
    );

    // **And the identity derived FROM that commit**, not just the commit. An
    // earlier draft asserted only the sha, and a mutation that invented the tree
    // and the message survived it — the pin is read either way, so the sha alone
    // proves nothing about `commit_identity`. Measured.
    let tree = crate::workspace_manager::fixture::git(
        &fixture.repo_root,
        &["rev-parse", &format!("{commit}^{{tree}}")],
    );
    assert_eq!(prepared.tree_sha.0, tree, "the tree is not the commit's");
    assert_eq!(
        prepared.message, "upstroke: alpha attempt 1",
        "the message is not the commit's"
    );
    assert_eq!(prepared.base_sha, fixture.base_sha, "the base moved");
    assert_eq!(prepared.parent_sha, fixture.base_sha);
    assert_eq!(prepared.key, TaskKey(0));

    // The attempt record is the settlement's, not a fresh one: the convergence
    // reads it from the events the barrier itself parsed.
    assert_eq!(prepared.attempt.attempt, 1);

    // -----------------------------------------------------------------------
    // **And the row's continuation ran.** E6 says "append `candidate_prepared`,
    // then continue as `T-CAND-REF`", and everything above this line witnesses
    // only the append. The whole suite passed with the continuation absent —
    // which is what left the converged generation stalled at `Promoting` with
    // no loop branch able to advance it, and every other task blocked behind
    // its pipeline entitlement at `max_parallel = 1`.
    // -----------------------------------------------------------------------
    assert_eq!(
        recovered.finished,
        vec![TaskKey(0)],
        "the convergence entered `T-CAND-REF`'s sequence and did not leave it"
    );

    // "append task_candidate_created" — the durable half of the row's own
    // resume_action, and the event `eligible_integration` waits for.
    let events = TopologyFold::parse_log(&fixture.log_bytes()).expect("the log parses");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.body, TopologyEventBody::TaskCandidateCreated { .. }))
            .count(),
        1,
        "`task_candidate_created` is what fixes the FIFO position; without it the \
         generation holds its entitlement forever"
    );

    // The generation has **left** `Promoting`. This is the assertion that speaks
    // to the stall rather than to the paperwork: `select` has no branch for a
    // `Promoting` generation, so while it stays there the run cannot progress
    // whatever else is in the log.
    let fold = TopologyFold::replay(fixture.inputs(), &events).expect("the converged log folds");
    assert_ne!(
        fold.task(TaskKey(0))
            .and_then(|task| task.generations.last())
            .map(|generation| generation.class.clone()),
        Some(GenerationClass::Promoting),
        "a generation still Promoting after recovery is the stall itself"
    );

    // "create exact candidates ref zero-old if absent" and "prune the pin", the
    // row's two effect steps, in the repository rather than in the log.
    let refs = crate::workspace_manager::fixture::git(
        &fixture.repo_root,
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    );
    assert!(
        refs.lines()
            .any(|line| line.contains("candidates") && line.ends_with(commit.as_str())),
        "the candidates ref is missing or at another sha:\n{refs}"
    );
    assert!(
        !refs.contains("prepared"),
        "the pin was not pruned:\n{refs}"
    );
}

/// **"The closure procedure performs the same steps at any run end"** — twice.
///
/// `T-CAND-REF`'s `resume_action` ends with that sentence, and it is a claim
/// about *repetition*: the continuation must be safe to run again on a
/// generation it already finished. `candidate::tests::
/// kill_after_candidate_prepared_appends_candidate_created_once` proves the
/// once-ness at the unit level, by calling the sequence twice directly. This
/// proves it where it actually repeats — a second **resume**, which is what a
/// second crash produces.
///
/// The second run also exercises the path the first cannot: on resume one,
/// `candidate_prepared` is appended by the convergence and finished in the same
/// pass; on resume two it is already durable, which is `T-CAND-REF`'s *own*
/// window rather than E6's. `recovery_for` reads the durable record either way,
/// so the two windows differ only in who wrote the record — and this is the
/// assertion that says so.
#[test]
fn a_second_resume_finishes_nothing_and_appends_nothing() {
    let fixture = Fixture::build(
        "e6-converges-twice",
        Damage {
            open_generation: true,
            extra: vec![
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Succeeded,
                        lease: LeaseDisposition::PredictedRetained,
                    },
                ),
            ],
            ..Damage::default()
        },
    );
    seed_candidate_commit(&fixture, 0);

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;

    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (first, _) = resume_holding(&fixture, &harness, &given);
    let (first, handle) = first.expect("the first resume converges and finishes");
    assert_eq!(first.promoted, vec![TaskKey(0)]);
    assert_eq!(first.finished, vec![TaskKey(0)]);

    let after_first = TopologyFold::parse_log(&fixture.log_bytes()).expect("parses");
    let created_once = after_first
        .iter()
        .filter(|event| matches!(event.body, TopologyEventBody::TaskCandidateCreated { .. }))
        .count();
    assert_eq!(created_once, 1);

    // The first run's handle holds the worktree lock for the whole run, so a
    // second resume while it lives is refused — correctly, and by the guard
    // `PR5-R2-WORKTREE-LOCK-RETENTION` says nothing witnesses. Dropping it is
    // what makes this a *second run* rather than a second driver.
    drop(handle);

    // The same fixture, resumed again — a second crash, or an operator running
    // `resume` twice.
    let second_harness = crate::engine::topology::recover::tests::harness();
    let runtime2 = runtime_holding_the_record();
    let given2 = Given::healthy(&fixture, &runtime2, &certifies);
    let (second, _) = resume_holding(&fixture, &second_harness, &given2);
    let (second, _handle2) = second.expect("the second resume is a no-op, not a refusal");

    assert!(
        second.promoted.is_empty(),
        "the convergence ran again on a generation that already has its candidate"
    );
    assert!(
        second.finished.is_empty(),
        "the continuation ran again on a promotion that already left `Promoting`"
    );

    let after_second = TopologyFold::parse_log(&fixture.log_bytes()).expect("parses");
    assert_eq!(
        after_second
            .iter()
            .filter(|event| matches!(event.body, TopologyEventBody::TaskCandidateCreated { .. }))
            .count(),
        1,
        "a second `task_candidate_created` is a line the fold refuses on the next replay — \
         a log that cannot be resumed"
    );
    assert_eq!(
        after_second
            .iter()
            .filter(|event| matches!(event.body, TopologyEventBody::CandidatePrepared { .. }))
            .count(),
        1,
        "and a second `candidate_prepared` is the same defect one event earlier"
    );
}

/// **The convergence is a resume, so its spend is the log's.**
///
/// The parity that `a_runs_spend_is_the_same_live_as_on_replay` asserts for a
/// live run, over a log a *resume* completed. Both `attempt_finished{Succeeded}`
/// and `candidate_prepared` carry the record, and the convergence appends the
/// second one — so a spend reader that counted occurrences would price this
/// attempt twice, and the double-count would arrive from recovery rather than
/// from the driver.
#[test]
fn a_converged_log_prices_its_attempt_once() {
    use crate::engine::topology::run::TopologyRun;
    use crate::engine::topology::select::{Ceiling, Spend};

    let fixture = Fixture::build(
        "e6-parity",
        Damage {
            // **The dispatch comes from `open_generation`, not from `extra`.**
            // It records the fixture's real base, and the convergence diffs the
            // candidate commit against it — a placeholder sha makes `diff-tree`
            // fail rather than report a region. Measured.
            open_generation: true,
            extra: vec![
                attempt_started(1),
                attempt_finished(
                    1,
                    AttemptSettlement::Closed {
                        transition: SettlementTransition::Succeeded,
                        lease: LeaseDisposition::PredictedRetained,
                    },
                ),
            ],
            ..Damage::default()
        },
    );
    seed_candidate_commit(&fixture, 0);

    let harness = harness();
    let runtime = runtime_holding_the_record();
    let certifies = AlwaysCertifies;
    let given = Given::healthy(&fixture, &runtime, &certifies);
    let (outcome, _) = resume_holding(&fixture, &harness, &given);
    let (_recovered, handle) = outcome.expect("the resume converges");

    let events = TopologyFold::parse_log(&fixture.log_bytes()).expect("the log parses");
    let settlements = events
        .iter()
        .filter(|event| {
            matches!(
                event.body,
                TopologyEventBody::AttemptFinished { .. }
                    | TopologyEventBody::CandidatePrepared { .. }
            )
        })
        .count();
    assert_eq!(
        settlements, 2,
        "the converged log should carry both events for one attempt — if it \
         does not, this test is not measuring the double-count it exists for"
    );

    let priced = Spend::replay(&events).run_total();
    let once = events
        .iter()
        .find_map(|event| match &event.body {
            TopologyEventBody::AttemptFinished { data } => Some(
                data.record.cost_usd.unwrap_or(0.0) + data.record.review_cost_usd().unwrap_or(0.0),
            ),
            _ => None,
        })
        .expect("the settlement is durable");
    assert!(
        (priced - once).abs() < 1e-9,
        "a converged log prices its one attempt at {priced}, and the attempt \
         cost {once}: recovery's append made the run look more expensive than \
         it was"
    );

    // -----------------------------------------------------------------------
    // **The driver half.** Everything above measures `Spend::replay` — the
    // accumulator. This measures whether the run *reads* it, and until the
    // handle carried the barrier's events it did not: `TopologyRun::resumed`
    // built a `Spend::new()`, so `Spend::replay` had no production caller at
    // all and every restart handed the run its whole budget back.
    //
    // This is the §4 class's own prescription, applied to the accumulator the
    // class's narrow name let through. A witness for the accumulation is not a
    // witness for the read, and the two had to be written as two assertions
    // because they are two claims.
    // -----------------------------------------------------------------------
    assert!(
        priced > 0.0,
        "the fixture's attempt costs nothing, so a driver that read no spend at \
         all would agree with one that read it correctly — this test would \
         assert nothing"
    );
    let run = TopologyRun::resumed(handle, fixture.inputs(), Ceiling::unlimited());
    assert!(
        (run.spend().run_total() - priced).abs() < 1e-9,
        "the resumed run believes it has spent {} where its own log says {priced}. \
         A ceiling counted from zero after every restart is a budget that only \
         binds runs that never crash",
        run.spend().run_total()
    );
}

/// Put a real candidate commit and its pin in the fixture's repository.
///
/// **Erratum E6's window needs both halves to be real.** The convergence
/// reconstructs the candidate's identity from the object the pin points at —
/// tree and message from the commit, region from `diff-tree base commit` — so a
/// fixture that seeded only events would exercise the refusal, not the
/// convergence. This writes an actual commit on top of the fixture's base and
/// creates the pin at it, which is exactly what a run killed after its
/// settlement leaves behind.
///
/// Returns the commit sha, so a test can assert `candidate_prepared` names the
/// object the pin named rather than one recovery invented.
fn seed_candidate_commit(fixture: &Fixture, generation: u32) -> String {
    use crate::workspace_manager::fixture::{git, write_file};

    let repo = &fixture.repo_root;
    // A change on top of the base, committed without moving the branch: the
    // candidate commit is unreferenced except by its pin, which is R23's shape.
    //
    // **One path, never `add -A`.** The run's own directory lives under
    // `.upstroke/` inside this repository, so `add -A` stages it and any
    // subsequent worktree restore deletes it — measured, as a resume that could
    // not find its own run.
    write_file(&repo.join("candidate.txt"), b"the worker's edit\n");
    git(repo, &["add", "--", "candidate.txt"]);
    let tree = git(repo, &["write-tree"]);
    let commit = git(
        repo,
        &[
            "commit-tree",
            &tree,
            "-p",
            fixture.base_sha.as_str(),
            "-m",
            "upstroke: alpha attempt 1",
        ],
    );
    // Unstage and remove just that path, so the index matches the base again
    // and the pin is the only thing referencing the commit. Targeted for the
    // same reason the add was.
    //
    // `git rm` rather than `std::fs::remove_file`: deletion is a funnel in this
    // crate and the effect denylist refuses the raw call even in a fixture,
    // which is the rule working rather than getting in the way.
    git(repo, &["rm", "-q", "-f", "--", "candidate.txt"]);
    let pin = crate::engine::topology::candidate::candidate_pin_ref(
        RUN_ID,
        TaskKey(0),
        GenerationId(generation),
    );
    git(repo, &["update-ref", pin.as_str(), &commit]);
    commit
}

/// An [`IdSource`] whose question id is a constant.
///
/// A park appends the id it minted, and `rematerialize_question` reads it back
/// on resume rather than re-deciding it — so a test that asserts on the durable
/// question needs the id to be the same bytes every run. `RealIds` gives a
/// ULID, which is right in production and unpinnable here.
struct FixedIds;

impl crate::engine::topology::seams::IdSource for FixedIds {
    fn run_id(&self) -> String {
        RUN_ID.to_owned()
    }

    fn incarnation(&self) -> crate::topology::events::IncarnationId {
        crate::topology::events::IncarnationId("inc-fixed".to_owned())
    }

    fn pid(&self) -> u32 {
        4242
    }

    fn question_id(&self) -> crate::ir::QuestionId {
        crate::ir::QuestionId("q-park-fixed".to_owned())
    }
}

/// The kinds in a fixture's durable log, in order.
fn durable_kinds(fixture: &Fixture) -> Vec<String> {
    TopologyFold::parse_log(&fixture.log_bytes())
        .expect("the log parses")
        .iter()
        .map(|event| event.body.kind().to_owned())
        .collect()
}

/// A sleeper that records rather than sleeps.
#[derive(Default)]
struct RecordingSleeper {
    slept: std::sync::Mutex<Vec<Duration>>,
}

impl crate::interaction::Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.slept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(duration);
    }
}

/// **A call census's needle is not satisfied by a longer name ending in it.**
///
/// The class boundary, not the instance. S5 round 4 found that
/// `every_packet_named_recovery_action_has_a_production_caller` counted
/// `refuse_unexpected_refs(` as a call to `expected_refs` — but the interesting
/// half is that the same needle is built for **every** entry from a name the
/// packet chose, so any future clause whose function name is a suffix of another
/// identifier is satisfied by that other identifier's call sites, silently and
/// in the passing direction.
///
/// So this asserts the needle's rule over the four shapes that decide it, and
/// then over the real file the collision was found in — a unit assertion alone
/// would pass against a helper that was never wired into the census.
#[test]
fn a_call_census_needle_is_not_satisfied_by_a_longer_name_ending_in_it() {
    assert_eq!(
        crate::effects::census_domain::production_calls(
            "            .refuse_unexpected_refs(&namespace, &expected)?;\n",
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        0,
        "a longer identifier ending in the entry's name satisfied its census entry"
    );
    assert_eq!(
        crate::effects::census_domain::production_calls(
            "        let expected = crate::engine::topology::candidate::expected_refs(&r, f);\n",
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        1,
        "a genuine call through a path was rejected: `:` is not an identifier byte"
    );
    assert_eq!(
        crate::effects::census_domain::production_calls(
            "    let e = expected_refs(&run_id, fold);\n",
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        1,
        "a genuine bare call was rejected"
    );
    assert_eq!(
        crate::effects::census_domain::production_calls(
            "pub fn expected_refs(run_id: &str, fold: &TopologyFold) -> Vec<String> {\n",
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        0,
        "a definition is not a call: a function that calls only itself is what this census exists \
         to catch"
    );

    // And on the file the collision was measured in, through the same region
    // the census reads — a unit assertion over literals would pass against a
    // helper nothing was wired into.
    //
    // **The two counts differ, and the difference is worth keeping.** The whole
    // file carries four occurrences of `expected_refs(`; the region the census
    // reads carries one, because three of the four sit inside `#[cfg(test)]`
    // items that `production_code` blanks. The one that survives is the
    // **definition line** of `refuse_unexpected_refs`, which the "calls, not
    // definitions" filter does not catch: the text before the match is
    // `pub fn refuse_un`, and that does not end in `fn`.
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/workspace_manager.rs"),
    )
    .expect("a source file");
    let code = crate::effects::production_code(&source);
    let whole = source.matches("expected_refs(").count();
    let region = code.matches("expected_refs(").count();
    assert!(
        whole >= 4 && region >= 1,
        "`workspace_manager.rs` no longer carries the substring this test is about ({whole} in \
         the file, {region} in the production region), so the zero below proves nothing"
    );
    assert_eq!(
        crate::effects::census_domain::production_calls(
            &code,
            "expected_refs",
            crate::effects::census_domain::Call::Free
        ),
        0,
        "the production region of `workspace_manager.rs` has {region} occurrence(s) of \
         `expected_refs(` and every one of them belongs to `refuse_unexpected_refs`; counting \
         them is how a census entry gets proved by a function that is not the one it names"
    );
}

/// **Every packet-named recovery action and refusal has a production caller.**
///
/// The class this slice produced more than any other, and the one census that
/// closes it. Across three review rounds, ten separate things were found
/// **built, correct, and never called**: `TopologyRun` itself, `settle_*`, the
/// candidate sequence, `resume_open_no_attempt`, `Started`, `CandidateJournal`,
/// `Spend::replay`, `complete_promotion`'s continuation, `prune_orphan_pin` and
/// `refuse_unexpected_refs`.
///
/// Two of those were P0/P1 liveness defects — a converged promotion that stalled
/// the run forever, and a resumed run that forgot its whole spend. The rest were
/// coverage gaps that would have become defects the moment a caller appeared.
/// Each was found separately, by a different reviewer noticing a different
/// symptom, over four rounds.
///
/// **This asserts the property the packet states, rather than waiting for a
/// reviewer to notice its absence.** A function that implements a
/// `resume_action` or a `refusal_condition` and has no production caller is not
/// an implementation of that clause — it is a plan to implement it.
///
/// **What this census covers, exactly.** The eleven entries below and nothing
/// else. Of the ten never-called things listed above, `Spend::replay`,
/// `TopologyRun`, `Started`, `CandidateJournal`, `settle_*` and
/// `complete_promotion`'s continuation are **not** among them — this would not
/// have caught them, and the commit that added it said otherwise. Corrected in
/// `reviews/FINDINGS.md` §19, claim (7); recorded here because the reader who
/// needs it is the one adding the twelfth entry.
///
/// Four ways this could pass while a clause stayed unperformed, and each is
/// closed by a named thing rather than by the needle being "obviously right":
///
/// * **A mention in a doc comment or a string.** The region is
///   `effects::production_code`, which blanks comments and string literals.
/// * **A `#[cfg(test)]` caller in the same file.** The same region removes each
///   configured item in place.
/// * **A caller in an out-of-line `tests.rs`**, where the attribute is on the
///   parent's declaration and there is nothing in the file to blank. Skipped by
///   file stem in the walk below. This was live until S5 round 4.
/// * **A longer identifier ending in the entry's name.** `expected_refs(` was
///   satisfied by `refuse_unexpected_refs(`. Closed by [`crate::effects::census_domain::production_calls`],
///   whose own witness is
///   `a_call_census_needle_is_not_satisfied_by_a_longer_name_ending_in_it`.
///
/// The fourth is the one worth stating as a class: the needle is built from a
/// name **the packet chose**, so it cannot be renamed out of a collision the way
/// `into_log_and_fold` was.
#[test]
fn every_packet_named_recovery_action_has_a_production_caller() {
    /// (function, how production calls it, the packet clause it performs).
    const CLAUSES: &[(&str, crate::effects::census_domain::Call, &str)] = &[
        (
            "prune_orphan_pin",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-CAND-OBJ].resume_action (b): delete the exact orphan pin \
             expected-old",
        ),
        (
            "refuse_unexpected_refs",
            crate::effects::census_domain::Call::Method,
            "transaction_fault_matrix[T-CAND-OBJ].refusal_condition: an unexpected ref under the \
             run namespace",
        ),
        (
            "expected_refs",
            crate::effects::census_domain::Call::Free,
            "the entitlement `refuse_unexpected_refs` refuses against, derived from the fold",
        ),
        (
            "complete_promotions",
            crate::effects::census_domain::Call::Free,
            "erratum E6: append candidate_prepared for a settled-but-unrecorded candidate",
        ),
        (
            "finish_promotions",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-CAND-REF].resume_action: verify, create the ref, append \
             task_candidate_created, prune the pin",
        ),
        (
            "recreate_open_no_attempt",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-DISPATCH].resume_action: verify the worktree or recreate it",
        ),
        (
            "settle_interrupted",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-ATTEMPT].resume_action: append attempt_interrupted",
        ),
        (
            "close_retained_idle",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-RETAINED].resume_action: a fresh process closes it in \
             recovery",
        ),
        (
            "ensure_recorded_integration_ref",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-RUNSTART].resume_action: P7/P8 create the ref zero-old at \
             the recorded base",
        ),
        (
            "refuse_unimplemented_terminals",
            crate::effects::census_domain::Call::Free,
            "checkpoint_refusals: refuse, before any append, any operation whose terminals this \
             build does not implement",
        ),
        (
            "resume_open_no_attempt",
            crate::effects::census_domain::Call::Free,
            "transaction_fault_matrix[T-DISPATCH].resume_action: continue attempt (no spend \
             repeats)",
        ),
    ];

    let mut test_files_skipped = 0_usize;
    let sources: Vec<(String, String)> = {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut all = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("src is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    all.push(path);
                }
            }
        }
        // **The crate's own declarations, not a file-name rule.** This skipped
        // by the stem `"tests"` and covered fourteen files; the crate declares
        // **seventeen** whole-file test modules and the three it missed —
        // `scaffold`, `premove`, `fake` — are the ones most likely to name what
        // production names. `PR7-R5-ATT-001`.
        let test_modules = crate::effects::census_domain::whole_file_test_modules(&all, 13);
        let mut out = Vec::new();
        {
            for path in all {
                // **An out-of-line test file is test code in full, and
                // `production_code` cannot tell.** The `#[cfg(test)]` is on the
                // *declaration* in the parent, so the file it names carries no
                // attribute of its own and nothing in it is blanked. Without
                // this skip a fixture calling a packet-named function satisfies
                // the clause on production's behalf, which is precisely the
                // class this census exists to close.
                if test_modules.contains(&path) {
                    test_files_skipped += 1;
                    continue;
                }
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let source = std::fs::read_to_string(&path).expect("a source file");
                out.push((relative, crate::effects::production_code(&source)));
            }
        }
        out
    };
    assert!(
        sources.len() > 20,
        "the walk found {} sources, so its zero counts would prove nothing",
        sources.len()
    );
    // The skip is in force and it removed something. A zero here would mean the
    // control was silently inert — the same failure as an empty region, one
    // level up.
    assert!(
        test_files_skipped >= 17
            && sources.iter().all(|(rel, _)| !rel.ends_with("tests.rs")
                && !rel.ends_with("scaffold.rs")
                && !rel.ends_with("premove.rs")
                && !rel.ends_with("fake.rs")),
        "the out-of-line test modules are not being skipped ({test_files_skipped} skipped of the \
         seventeen the crate declares), so a fixture's call can satisfy a clause on production's \
         behalf. The three named here are the ones a file-name rule misses"
    );

    let mut uncalled: Vec<String> = Vec::new();
    let mut undefined: Vec<String> = Vec::new();
    for (name, form, clause) in CLAUSES {
        // **The named item exists.** The census never checked, so renaming a
        // clause's definition out of the tree left it green — measured, S5
        // round 4. Not pinned to exactly one definition, because
        // `settle_interrupted` legitimately names three items and `form` is
        // what separates them.
        let defined: usize = sources
            .iter()
            .map(|(_, code)| code.matches(&format!("fn {name}(")).count())
            .sum();
        if defined == 0 {
            undefined.push((*name).to_owned());
        }
        let calls: usize = sources
            .iter()
            .map(|(_, code)| crate::effects::census_domain::production_calls(code, name, *form))
            .sum();
        if calls == 0 {
            uncalled.push(format!("`{name}` performs `{clause}`"));
        }
    }

    assert!(
        undefined.is_empty(),
        "these are named as performing a packet clause and no production item of that name \
         exists, so the row below cannot fail for the right reason and has been passing on \
         somebody else's call sites: {undefined:?}"
    );

    assert!(
        uncalled.is_empty(),
        "these implement a packet clause and nothing in production calls them, so the clause is \
         not performed by any run — which is how this slice shipped a converged promotion that \
         stalled forever and a resumed run that forgot its spend:\n  {}",
        uncalled.join("\n  ")
    );
}
