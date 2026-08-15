//! Sequential execution engine (DESIGN.md §14) and the verification ladder it
//! drives (§11.4, §12, §19).
//!
//! Pre-flight, run branch, then a scheduler that drains the task graph one
//! attempt at a time: agent run → engine-captured diff → gates with evidence
//! axes (§11.1) → read-only review with a structured verdict (§11.2) →
//! engine-owned commit. A failed attempt does not end the task — it feeds the
//! failure back to the same rung (resuming the session where the adapter
//! supports it), then escalates a rung on a fresh session with the accumulated
//! feedback, and finally asks a human, who is the top rung.
//!
//! The scheduler's defining property is invariant 6: **a question parks only
//! the tasks it affects.** Everything else keeps draining, and the run
//! hard-blocks only when the runnable frontier is empty and everything left is
//! waiting on an answer. That is the moment — and the only moment — a human is
//! asked.
//!
//! Every transition here is an event (invariant 4). The engine never mutates
//! run state directly: it appends to `events.jsonl` and folds the event back in
//! through [`RunState::apply`], the same function `resume` and `status` use to
//! rebuild state from the file. A live run and a replay of its own log
//! therefore cannot disagree — there is no second path for them to disagree
//! along. `report.json` is written from that state as a projection for humans;
//! nothing ever reads it back.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::{AgentAdapter, Caps, TaskRun, proc};
use crate::capacity;
use crate::config::{self, OnTaskFailure};
use crate::error::TactusError;
use crate::events::{
    self, BindingSummary, ChainSummary, EventBody, EventLog, Feedback, GateSummary, Progress,
    RunState, TaskState,
};
use crate::gates::{self, ShellGate};
use crate::interaction::{
    self, AnswerSource, InteractionMode, Notifier, QuestionRecord, RealSleeper, Sleeper,
};
use crate::ir::{
    Answer, Outcome, OutcomeStatus, PermissionMode, Plan, Question, QuestionId, QuestionKind,
    ResolvedEffortPolicy, Task, TaskKind, WorkerProfile,
};
use crate::ladder::{self, LadderPolicy, LadderState, Next};
use crate::review::{self, PassBinding, ReviewPass, ReviewPlan};
use crate::rundir::{self, RunLock, RunPaths, WorktreeLock};
use crate::ulid;
use crate::util;
use crate::validate::{self, Analysis, ValidateOptions};
use crate::workspace::Workspace;

// Re-exported so `engine::AdapterSource` still resolves for callers that
// reasonably think of it as the engine's seam.
pub use crate::agent::{AdapterSource, BuiltinAdapters};
pub use crate::events::{AttemptRecord, FailureRecord};
pub use crate::ladder::{AttemptFailure, FailureKind, FailureOrigin};

/// §14: per-attempt wall clock, default 30 minutes.
pub const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How many rate limits (or reviewer outages) one task rides out before the
/// pool counts as down and a human is asked instead.
///
/// Step 10 gave the capacity engine reset times — [`crate::capacity`] carries
/// them on an estimate, and `pool_exhausted` records one whenever a signal
/// includes it — so the obvious question is why this bound still exists. Two
/// reasons, both current: neither CLI actually reports a machine-readable reset
/// time today, so the field is almost always `None`; and §13 ships the capacity
/// engine read-only in v0.1, so nothing routes on a reset even when there is
/// one. Waiting for a reset instead of counting deferrals is capacity-*driven*
/// behaviour, and it arrives with the rest of it in v0.2. Until then this is
/// what keeps an exhausted pool from deferring forever.
pub const DEFAULT_MAX_DEFERS: u32 = 3;

/// Most recent feedback entries carried into an escalated prompt. Older
/// failures are summarized; the newest keeps its full log tail.
const MAX_FEEDBACK_ENTRIES: usize = 6;

/// §12: how a worker flags a decision it should not make alone. The prompt
/// teaches this marker; nothing else in the engine parses agent prose.
const QUESTION_MARKER: &str = "TACTUS-QUESTION:";

#[cfg(test)]
type AfterCandidateCapture =
    fn(&Workspace, &crate::workspace::CapturedCandidate) -> Result<(), TactusError>;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub plan_path: PathBuf,
    pub config_path: Option<PathBuf>,
    pub pools_path: Option<PathBuf>,
    /// Repo the run executes in (agents run at its root — §14).
    pub repo_root: PathBuf,
    pub attempt_timeout: Duration,
    /// CLI override for `[interaction] mode`; `None` takes the config's.
    pub interaction: Option<InteractionMode>,
    /// First wait after a rate-limited attempt, doubling per consecutive
    /// round of nothing-but-deferred-work.
    pub defer_backoff: Duration,
    pub max_defers: u32,
    /// Where the agent-authored half of the run directory goes (§15 split).
    /// `None` takes `~/.tactus`; tests point it at a scratch directory so they
    /// never touch the real one.
    pub private_root: Option<PathBuf>,
    /// Override `[interaction] wait_on_block_secs` — how long a detached
    /// interactive run waits at a hard block. `None` takes the config's.
    pub wait_on_block: Option<Duration>,
    /// `--budget <usd>`, overriding `[budgets] run_usd` (§17).
    pub budget_usd: Option<f64>,
    /// Deterministic test seam for changing the mutable index immediately
    /// after the engine has frozen its candidate object identities.
    #[cfg(test)]
    after_candidate_capture: Option<AfterCandidateCapture>,
}

impl RunOptions {
    /// Everything but the paths at its documented default.
    pub fn new(plan_path: PathBuf, repo_root: PathBuf) -> Self {
        Self {
            plan_path,
            config_path: None,
            pools_path: None,
            repo_root,
            attempt_timeout: DEFAULT_ATTEMPT_TIMEOUT,
            interaction: None,
            defer_backoff: interaction::DEFAULT_DEFER_BACKOFF,
            max_defers: DEFAULT_MAX_DEFERS,
            private_root: None,
            wait_on_block: None,
            budget_usd: None,
            #[cfg(test)]
            after_candidate_capture: None,
        }
    }

    fn paths(&self, run_id: &str) -> RunPaths {
        match &self.private_root {
            Some(root) => RunPaths::with_private_root(&self.repo_root, run_id, root),
            None => RunPaths::new(&self.repo_root, run_id),
        }
    }
}

/// Injectable collaborators. `None` means "use the real one", chosen from
/// config where the config has a say.
pub struct Harness<'a> {
    pub adapters: &'a dyn AdapterSource,
    /// `None` derives the channel from `[interaction] mode` (§12).
    pub answers: Option<&'a dyn AnswerSource>,
    /// `None` really sleeps.
    pub sleeper: Option<&'a dyn Sleeper>,
}

impl<'a> Harness<'a> {
    pub fn new(adapters: &'a dyn AdapterSource) -> Self {
        Self {
            adapters,
            answers: None,
            sleeper: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TaskRunStatus {
    Committed {
        sha: String,
    },
    Failed {
        kind: FailureKind,
        reason: String,
    },
    /// Waiting on a human. The rest of the run kept moving (invariant 6), and
    /// nothing about this task is lost — the question carries its context.
    Parked {
        question: String,
        reason: String,
    },
    /// A dependency failed, or parked and was never answered.
    Blocked {
        by: String,
    },
    /// Not attempted because the run halted earlier.
    Skipped,
    /// An attempt is running right now. Only a live `status` produces this: a
    /// run that has ended has nothing left in flight.
    Running {
        attempt: u32,
        tier: String,
        model: String,
    },
    /// Its turn has not come yet, and the run is still going — distinct from
    /// `Skipped`, which means the run ended before this task got a turn.
    Queued,
    /// A status this build does not know, from a `report.json` a newer tactus
    /// wrote. Never produced by this crate.
    ///
    /// `report.json` is a projection for whoever reads the run afterwards, and
    /// this enum is `pub` and `Deserialize` because that reader may be someone
    /// else's program. Without a fallback, every variant added here is a hard
    /// `unknown variant` error in every consumer built against an older
    /// version — which is what `running`, `Queued` and this one did to anything
    /// compiled against 0.0.1, and that break is already published. Adding it
    /// now cannot undo that; it stops the next one.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReport {
    pub id: String,
    pub title: String,
    /// The final attempt's implementer model. `cost_usd` is the implementer's
    /// spend across every attempt; reviewer spend is a separate field because
    /// it is a different model at a different tier, and folding them together
    /// makes cheap rungs look expensive to anyone reading the ledger (§13).
    pub model: String,
    pub status: TaskRunStatus,
    pub duration: Duration,
    pub cost_usd: Option<f64>,
    /// Every model that judged this task, in the order first seen.
    ///
    /// Across *all* attempts, not just the last, because `review_cost_usd`
    /// beside it sums all of them — a list scoped to the final attempt next to
    /// a total scoped to every attempt reads as though one explains the other.
    pub review_models: Vec<String>,
    pub review_cost_usd: Option<f64>,
    /// At least one review pass reported no spend, so `review_cost_usd` is a
    /// floor (§13). Rendered as a `?` rather than left to look exact.
    pub review_cost_incomplete: bool,
    pub session_id: Option<String>,
    /// Every attempt, oldest first — the escalation trail.
    pub attempts: Vec<AttemptRecord>,
}

impl TaskReport {
    /// Implementer plus reviewer, across every attempt.
    pub fn total_cost_usd(&self) -> Option<f64> {
        match (self.cost_usd, self.review_cost_usd) {
            (None, None) => None,
            (worker, review) => Some(worker.unwrap_or(0.0) + review.unwrap_or(0.0)),
        }
    }

    /// Whether an attempt reported no spend, making `cost_usd` a floor.
    ///
    /// The worker-side twin of [`Self::review_cost_incomplete`], and a method
    /// rather than a field because it is derivable from the attempts already
    /// carried here — no schema change, so an older `report.json` reads back
    /// with the same answer this computes.
    ///
    /// Two kinds of attempt land here, and both genuinely spent something
    /// nobody can name: one on a route that reports no dollars at all (Codex
    /// reports tokens — §13), and one the engine was killed inside, whose
    /// `cost_usd` is `null` precisely because the record of its ending was
    /// never written. `unpriced_attempts` counts the same condition for the
    /// capacity estimator, so the ledger and the estimator now agree about
    /// which attempts are unpriced.
    pub fn cost_incomplete(&self) -> bool {
        self.attempts.iter().any(|record| record.cost_usd.is_none())
    }

    /// Compact escalation trail, e.g. `small×2 failed → mid ok`.
    pub fn trail(&self) -> String {
        let mut parts: Vec<(String, u32, bool)> = Vec::new();
        for record in &self.attempts {
            let failed = record.failure.is_some();
            match parts.last_mut() {
                Some((tier, count, last_failed)) if *tier == record.tier => {
                    *count += 1;
                    *last_failed = failed;
                }
                _ => parts.push((record.tier.clone(), 1, failed)),
            }
        }
        parts
            .into_iter()
            .map(|(tier, count, failed)| {
                let count = if count > 1 {
                    format!("×{count}")
                } else {
                    String::new()
                };
                let verdict = if failed { "failed" } else { "ok" };
                format!("{tier}{count} {verdict}")
            })
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

/// How the run ended.
///
/// `Parked` is deliberately not `Halted`: §12 requires CI to tell a clean
/// completion from one that left questions unanswered. `BudgetExceeded` earns
/// its own variant for the same reason one step further out — "your ceiling
/// stopped it" is neither a failure nor a question, and `tactus resume` means
/// something different after each of the three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Complete,
    Halted,
    BudgetExceeded,
    Parked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub branch: String,
    /// Effective gate names, and whether they came from config or derivation.
    pub gates: Vec<String>,
    pub gates_from_config: bool,
    pub warnings: Vec<String>,
    pub tasks: Vec<TaskReport>,
    /// Task id the run halted at, if any.
    pub halted_at: Option<String>,
    /// Every question raised, with its answer where one arrived (§12).
    pub questions: Vec<QuestionRecord>,
    /// The §13 ceiling that stopped the run, if one did.
    #[serde(default)]
    pub budget_stop: Option<events::BudgetExceeded>,
    pub total_cost_usd: f64,
    /// What each pool drained, folded from this run's own attempts (§13).
    #[serde(default)]
    pub pool_drain: Vec<PoolDrainRow>,
    /// Whether an engine is driving this run right now. A live run must not be
    /// rendered as a finished one: its in-flight attempt has not failed, and
    /// the tasks queued behind it have not been skipped.
    #[serde(default)]
    pub running: bool,
    /// Whether this run stopped without ever recording that it finished — the
    /// signature of a kill, a power loss, or an aborting error.
    ///
    /// A run in that state has no outcome, and `outcome()` cannot tell: a
    /// killed run has nothing halted, no budget stop and nothing parked, which
    /// is indistinguishable from a clean finish. So the flag has to be carried
    /// rather than derived, exactly as `running` is.
    ///
    /// Not to be confused with `RunStatus::interrupted`, which is a `u32`
    /// counting the attempts that were cut off mid-flight. This is the yes/no.
    #[serde(default)]
    pub interrupted: bool,
}

/// One pool's line in the ledger: what this run drew from which subscription.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolDrainRow {
    pub pool: String,
    pub attempts: u32,
    /// Reported api-equivalent dollars, `None` when nothing on this pool
    /// reported any.
    pub cost_usd: Option<f64>,
    /// Attempts whose route reports no spend at all (§13), making the figure
    /// above a floor rather than a total.
    pub unpriced: u32,
}

impl RunReport {
    pub fn parked_tasks(&self) -> Vec<&str> {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskRunStatus::Parked { .. }))
            .map(|t| t.id.as_str())
            .collect()
    }

    /// Whether `total_cost_usd` is a floor rather than a figure.
    ///
    /// `total_cost_usd` is an `f64`, so it cannot say this for itself: a run
    /// that reported nothing and a run that genuinely cost nothing both arrive
    /// as `0.0`. The distinction has to be carried alongside, and §13 is
    /// explicit that a ledger which cannot tell free from unreported is worse
    /// than no ledger.
    ///
    /// Both halves count. The review side has been marked since step 9; the
    /// worker side became reachable the moment an implementer could report
    /// tokens without dollars, and is now the *normal* case for a
    /// codex-implemented run rather than an edge one.
    pub fn total_is_floor(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.cost_incomplete() || task.review_cost_incomplete)
    }

    /// How much of the plan actually landed — the one figure every ending
    /// wants, whether the run finished, is still going, or was cut off.
    fn committed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskRunStatus::Committed { .. }))
            .count()
    }

    /// Precedence: a halt outranks a budget stop, which outranks parked work.
    ///
    /// That order falls out of what actually happened rather than being a
    /// policy: a halt stops the drain before any further budget check can run,
    /// so a run with both is one that halted and then found its ceiling
    /// irrelevant. And a budget stop leaves tasks parked-or-skipped behind it,
    /// so reporting `Parked` would name a symptom instead of the cause.
    pub fn outcome(&self) -> RunOutcome {
        if self.halted_at.is_some() {
            RunOutcome::Halted
        } else if self.budget_stop.is_some() {
            RunOutcome::BudgetExceeded
        } else if self.parked_tasks().is_empty() {
            RunOutcome::Complete
        } else {
            RunOutcome::Parked
        }
    }
}

pub fn run(opts: &RunOptions) -> Result<RunReport, TactusError> {
    run_with(opts, &BuiltinAdapters)
}

pub fn run_with(opts: &RunOptions, adapters: &dyn AdapterSource) -> Result<RunReport, TactusError> {
    run_harness(opts, &Harness::new(adapters))
}

/// Everything `run` and `resume` both establish before an agent is spawned.
///
/// Shared so the two cannot drift: §15 requires a resume to re-probe agents
/// and re-check gates, and the surest way to guarantee it performs the same
/// checks as a fresh run is for there to be one function that performs them.
struct Preflight {
    analysis: Analysis,
    caps: BTreeMap<String, Caps>,
    review_plan: ReviewPlan,
    /// Each pass gets this independent frozen allowance. It comes from the
    /// review plan rather than today's config on resume.
    review_pass_timeout: Duration,
    /// The effective gates, in the one shape everything else projects from —
    /// the record, the permission grants, and the report all read this rather
    /// than walking `analysis.gates` again, so they cannot drift apart.
    gates: Vec<GateSummary>,
    gate_cmds: Vec<String>,
    warnings: Vec<String>,
    mode: InteractionMode,
    notifiers: Vec<&'static dyn Notifier>,
    /// §17's ceilings with `--budget` folded in and validated — computed at
    /// pre-flight so a bad flag refuses before the run branch exists.
    budgets: config::Budgets,
}

/// What a resume takes from the run's own record instead of from today's
/// machine (§15). Empty for a fresh run, which has no record to take from.
#[derive(Default)]
struct Recorded {
    /// Who judges this run's code. `None` for a log written before step 9.
    reviews: Option<ReviewPlan>,
    /// What verifies it. `None` for a log written before the gate record.
    gates: Option<Vec<GateSummary>>,
    /// The legacy record identifies the reviewers but predates schema 3's
    /// explicit per-pass timeout. Its first complete-review resume must choose
    /// and serialize that missing part of the verification identity.
    legacy_review_timeout_missing: bool,
    /// Whether those gates came from `[[gates]]` rather than the repo's shape.
    ///
    /// Travels with them, and read only when `gates` is `Some`: it is a label
    /// *on the recorded list*, so leaving it to be re-derived would have the
    /// run's own report and a later `status` disagree about the same gates —
    /// the drift this record exists to stop, one field short of stopped.
    gates_from_config: bool,
    /// The run's routing structure plus the first snapshot that names every
    /// resolved rung binding. Present only on resume.
    routing: Option<RecordedRouting>,
}

struct RecordedRouting {
    run_id: String,
    structure: Vec<ChainSummary>,
    bindings: Option<Vec<ChainSummary>>,
}

fn preflight(opts: &RunOptions, harness: &Harness<'_>) -> Result<Preflight, TactusError> {
    preflight_with_recorded(opts, harness, Recorded::default())
}

/// Pre-flight, with whatever a previous process already resolved for this run.
///
/// Both halves of §14's verification — who reviews and what gates — are read
/// from the record on resume rather than re-derived, for one reason stated
/// twice: they are facts about the *run*, not about today's machine. A CLI
/// installed or removed since the run started must not change who judges it,
/// and a `tactus.toml` edited since — including by an implementer, in the very
/// workspace it edits — must not change what verifies it. A live run already
/// works this way by construction, holding one analysis in memory for its whole
/// length; this is what makes a resume the same run rather than a new one
/// wearing its branch.
///
/// `None` on either means the log predates that record and said nothing. Both
/// re-derive in that case rather than inherit an empty value, because an empty
/// review plan reads as "review is off" and an empty gate list reads as "there
/// was nothing to pass" — each would finish the run less verified than it began.
/// The caller warns; only it knows which absence it is looking at.
fn preflight_with_recorded(
    opts: &RunOptions,
    harness: &Harness<'_>,
    recorded: Recorded,
) -> Result<Preflight, TactusError> {
    // §14: plan parses cycle-free, config loads, chains resolve.
    let mut analysis = validate::analyze(&ValidateOptions {
        plan_path: opts.plan_path.clone(),
        config_path: opts.config_path.clone(),
        config_root: opts.repo_root.clone(),
        pools_path: opts.pools_path.clone(),
    })?;

    let mut warnings = analysis.warnings.clone();

    // Bindings are execution identity just like reviewers and gates. Restore
    // them before resolving reviewers or probing agents: probing today's pin
    // and only swapping later would let a harmless config edit refuse a resume
    // on an agent this run was never going to use.
    if let Some(routing) = recorded.routing.as_ref() {
        restore_recorded_routing(&mut analysis, routing, &mut warnings)?;
    }

    // The recorded gates replace the re-derived ones *here*, before anything
    // reads them — so the pre-flight resolution below, the `Bash(<cmd>)` grants
    // the workers get, the prompt that names their allowed commands, and the
    // report all describe the gates this run actually verifies against. One
    // substitution point rather than a comparison the rest of the function
    // could forget about.
    if let Some(record) = &recorded.gates {
        if let Some(difference) = gates_differ(record, &gate_summaries(&analysis)) {
            warnings.push(difference);
        }
        analysis.gates = record.iter().map(ShellGate::from_record).collect();
        analysis.gates_from_config = recorded.gates_from_config;
    }

    let mut review_plan = match recorded.reviews {
        Some(mut plan) => {
            let configured = analysis.config.review_pass_timeout.as_secs();
            if recorded.legacy_review_timeout_missing {
                plan.pass_timeout_secs = Some(configured);
                warnings.push(format!(
                    "this run's recorded review plan predates schema 3's per-pass timeout; this \
                     resume establishes today's configured {configured}s timeout in the \
                     append-only log before any more work starts"
                ));
            } else if plan.pass_timeout_secs != Some(configured) {
                let recorded = plan
                    .pass_timeout_secs
                    .expect("a non-legacy recorded review plan has an explicit timeout");
                warnings.push(format!(
                    "today's review pass timeout ({configured}s) differs from the one this run \
                     recorded ({}s). This resume keeps the recorded timeout so one run has one \
                     verification standard. Start a new run to adopt today's timeout.",
                    recorded
                ));
            }
            if plan.enabled.is_none() || plan.alternative_available.is_none() {
                plan.enabled.get_or_insert(plan.primary.is_some());
                plan.alternative_available
                    .get_or_insert(plan.alternative.is_some());
                warnings.push(
                    "this run's recorded review plan predates schema 3's explicit reviewer-identity markers; this resume records them before any more work starts"
                        .to_owned(),
                );
            }
            plan
        }
        // Resolved against the adapters *this harness* holds, not the built-in
        // registry: the harness is what can actually spawn something, and
        // asking the wrong one would let a preview's answer stand in for a
        // capability the run does not have.
        None => review::plan_for(
            &analysis.plan,
            &analysis.chains,
            &analysis.config,
            |id| harness.adapters.get(id).is_some(),
            &mut warnings,
        )?,
    };
    // A legacy record is not trustworthy merely because its missing marker
    // fields can be filled. Validate the complete inherited identity before
    // probing an adapter or dispatching any paid work; otherwise a malformed
    // schema-2 pass list can run once and only be rejected after it has been
    // appended as schema 3.
    events::validate_review_identity(&review_plan, analysis.plan.tasks.len(), &opts.plan_path)?;
    let review_pass_timeout = review_plan.pass_timeout()?;

    // Probe every agent the chains reference; a missing binary is a refusal
    // to start, not a task failure (§19). The capabilities are kept, not
    // discarded: §11.4's same-rung retry resumes a session only where the
    // adapter says it can.
    //
    // Reviewers are probed on the same footing as implementers — step-6
    // finding #10 — but in two classes. Everything the config *asked* for is
    // required. The anti-self-review alternative was tactus's own idea, so a
    // machine that cannot run it loses the upgrade rather than the run.
    //
    // Resume draws the line in the same place. Requiring the alternative there
    // — on the grounds that a run should keep one verification standard — would
    // refuse to continue over a reviewer that may never have judged anything,
    // and the per-attempt record already names who judged each attempt, so the
    // ledger stays honest either way. A loud warning beats a dead run.
    let required = review_plan.required_agents();
    let optional: Vec<String> = review_plan
        .agents()
        .into_iter()
        .filter(|id| !required.contains(id))
        .map(str::to_owned)
        .collect();
    let mut agent_ids: Vec<&str> = analysis
        .chains
        .iter()
        .flat_map(|c| c.rungs.iter().map(|r| r.binding.agent.as_str()))
        .chain(required)
        .collect();
    agent_ids.sort_unstable();
    agent_ids.dedup();
    let mut caps: BTreeMap<String, Caps> = BTreeMap::new();
    for id in agent_ids {
        let adapter = harness.adapters.get(id).ok_or_else(|| TactusError::Agent {
            message: format!("no adapter registered for agent `{id}`"),
        })?;
        caps.insert(id.to_owned(), adapter.probe()?);
    }
    for id in optional {
        if caps.contains_key(&id) {
            continue;
        }
        let probed = harness
            .adapters
            .get(&id)
            .ok_or_else(|| TactusError::Agent {
                message: format!("no adapter registered for agent `{id}`"),
            })
            .and_then(|adapter| adapter.probe());
        match probed {
            Ok(caps_for_id) => {
                caps.insert(id, caps_for_id);
            }
            Err(error) => {
                let binding = review_plan
                    .alternative
                    .as_ref()
                    .map_or_else(|| id.clone(), PassBinding::describe);
                warnings.push(format!(
                    "{binding} would have reviewed tasks their own model implemented, but it \
                     could not be probed: {error}. Those tasks fall back to same-model review \
                     (§11.3)."
                ));
                review_plan.drop_alternative();
                // Now say WHICH tasks. Resolution could not: a shipped binary
                // always has the Copilot adapter, so the only way the rebind
                // actually goes missing is right here, and naming the tasks is
                // the difference between a note and something actionable.
                let tier = analysis
                    .config
                    .review_tier
                    .unwrap_or(crate::ir::Tier::Frontier);
                if let Some(warning) =
                    review_plan.self_review_warning(&analysis.plan, &analysis.chains, tier)
                {
                    warnings.push(warning);
                }
            }
        }
    }

    // Effective gates come from the shared analysis (single derivation point
    // with `validate`), or from the record above. §14 pre-flight: the shell and
    // every gate command must resolve before any agent tokens are spent — and
    // on a resume that check runs against the *recorded* gates, so a machine
    // that cannot run what this run verifies against says so plainly instead of
    // quietly proceeding.
    //
    // Per gate rather than per config: a recorded gate carries the shell it ran
    // under, and nothing requires every gate in a list to share one.
    if !analysis.gates.is_empty() {
        let mut shells: Vec<crate::gates::ShellKind> =
            analysis.gates.iter().map(|gate| gate.shell).collect();
        shells.sort_unstable_by_key(|shell| shell.program());
        shells.dedup();
        for shell in shells {
            gates::shell_available(shell)?;
        }
        gates::resolve_programs(&analysis.gates, &opts.repo_root, &mut warnings)?;
    }
    let gates = gate_summaries(&analysis);
    let gate_cmds: Vec<String> = gates.iter().map(|gate| gate.cmd.clone()).collect();

    let mode = opts.interaction.unwrap_or(analysis.config.interaction_mode);
    let notifiers = interaction::notifiers_for(&analysis.config.notify, &mut warnings);
    // Here, with the other pre-flight refusals, rather than where the ceiling
    // is first read: `--budget 0` must not create a branch and a run directory
    // before discovering it cannot spend anything (§14 — pre-flight refuses
    // before any agent token is spent, and before the workspace is touched).
    let budgets = effective_budgets(analysis.config.budgets, opts.budget_usd)?;

    Ok(Preflight {
        analysis,
        caps,
        review_plan,
        review_pass_timeout,
        gates,
        gate_cmds,
        warnings,
        mode,
        notifiers,
        budgets,
    })
}

pub fn run_harness(opts: &RunOptions, harness: &Harness<'_>) -> Result<RunReport, TactusError> {
    run_harness_inner(opts, harness).map(|(report, _)| report)
}

/// Also hands back the state the run ended with — its own fold of its own log.
///
/// Only tests use the second half, to hold the live fold and a replay of the
/// same file side by side. Nothing in the engine reads state back.
fn run_harness_inner(
    opts: &RunOptions,
    harness: &Harness<'_>,
) -> Result<(RunReport, RunState), TactusError> {
    let workspace = Workspace::open(&opts.repo_root)?;
    // Preflight reads the source plan, config, and gate programs from this
    // physical worktree. Own it before taking that snapshot so another run
    // cannot leave us with an analysis of its transient edits.
    let worktree_git_dir = workspace.worktree_git_dir()?;
    let _worktree_lock = WorktreeLock::acquire_in(workspace.root(), &worktree_git_dir)?;
    let Preflight {
        analysis,
        caps,
        review_plan,
        review_pass_timeout,
        gates,
        gate_cmds,
        mut warnings,
        mode,
        notifiers,
        budgets,
    } = preflight(opts, harness)?;

    workspace.ensure_execution_prerequisites()?;
    workspace.ensure_run_exclusions()?;
    if !workspace.is_clean()? {
        return Err(TactusError::Git {
            message: "working tree is not clean; commit or stash first (the engine refuses \
                      dirty trees)"
                .to_owned(),
        });
    }
    let base_sha = workspace.head_sha_full()?;
    let wait_on_block = opts.wait_on_block;

    let run_id = ulid::ulid();
    let branch = format!("tactus/run-{run_id}");
    let paths = opts.paths(&run_id);
    paths.create()?;
    // Held for the whole run, released by the OS if this process dies — so a
    // crash leaves nothing for `resume` to clear by hand.
    let _lock = RunLock::acquire(&paths.public)?;
    let _cleanup_scope = _lock.enter_cleanup_scope();

    // Nothing is on the record until the first event lands, so a failure in
    // this window would leave a run directory with no `events.jsonl` in it —
    // and that husk becomes `latest_run`, so a bare `tactus status` reports
    // "no event log here" for a run that never began, shadowing the real
    // latest one until someone deletes it by hand. Best-effort: failing to
    // tidy up must not mask the error that actually stopped the run.
    let plan_path = paths.plan_json();
    let normalized_plan = normalized_plan_bytes(&analysis.plan, &plan_path)?;
    let normalized_plan_digest = events::normalized_plan_digest(&normalized_plan);
    let opened = fs::write(&plan_path, &normalized_plan)
        .map_err(|source| TactusError::Io {
            path: plan_path.clone(),
            source,
        })
        .and_then(|()| {
            let read_back = fs::read(&plan_path).map_err(|source| TactusError::Io {
                path: plan_path.clone(),
                source,
            })?;
            if read_back != normalized_plan {
                return Err(TactusError::Refused {
                    message: format!(
                        "{} changed while tactus was freezing it; refusing to record a digest for bytes it did not write",
                        plan_path.display()
                    ),
                });
            }
            workspace.create_branch(&branch)
        });
    if let Err(error) = opened {
        drop(_cleanup_scope);
        drop(_lock);
        let _ = fs::remove_dir_all(&paths.public);
        let _ = fs::remove_dir_all(&paths.private);
        return Err(error);
    }

    let effort_policy = analysis.config.resolved_effort_policy();
    let started = events::RunStarted {
        schema: events::SCHEMA_VERSION,
        tactus_version: env!("CARGO_PKG_VERSION").to_owned(),
        run_id: run_id.clone(),
        branch: branch.clone(),
        base_sha,
        plan_path: repo_relative(&opts.repo_root, &opts.plan_path),
        config_path: opts
            .config_path
            .as_ref()
            .map(|path| repo_relative(&opts.repo_root, path)),
        plan_hash: analysis.plan.source.hash.clone(),
        normalized_plan_digest: Some(normalized_plan_digest),
        private_dir: paths.private.to_string_lossy().into_owned(),
        // Names for the reader, the full gates for the resume — both from the
        // one list pre-flight resolved, so the log cannot name a gate its own
        // record does not describe.
        gates: gates.iter().map(|gate| gate.name.clone()).collect(),
        gates_from_config: analysis.gates_from_config,
        interaction_mode: mode.to_string(),
        chains: chain_summaries(&analysis),
        effort_policy: Some(effort_policy),
        reviews: Some(review_plan.clone()),
        gate_cmds: Some(gates),
    };

    let sleeper = harness.sleeper.unwrap_or(&RealSleeper);
    let default_answers = interaction::answers_for(
        mode,
        paths.answers(),
        wait_on_block.unwrap_or(analysis.config.wait_on_block),
        sleeper,
    );
    let log = EventLog::open(&paths.events(), &mut warnings)?;
    let mut run = Run {
        state: RunState::new(
            analysis
                .plan
                .tasks
                .iter()
                .map(|task| task.id.to_string())
                .collect(),
        ),
        analysis: &analysis,
        workspace: &workspace,
        paths,
        log,
        gate_cmds,
        adapters: harness.adapters,
        answers: harness.answers.unwrap_or(default_answers.as_ref()),
        notifiers,
        sleeper,
        caps,
        review_plan,
        effort_policy,
        attempt_timeout: opts.attempt_timeout,
        review_pass_timeout,
        defer_backoff: opts.defer_backoff,
        max_defers: opts.max_defers,
        on_task_failure: analysis.config.on_task_failure,
        budgets,
        ask_before: analysis.config.ask_before,
        run_id,
        branch,
        warnings,
        unanswerable: Vec::new(),
        exhausted_pools: std::collections::BTreeSet::new(),
        #[cfg(test)]
        after_candidate_capture: opts.after_candidate_capture,
    };
    run.emit(EventBody::RunStarted {
        data: Box::new(started),
    })?;
    // A fresh run has no signals of its own yet, and §13's other sources are
    // not read in v0.1 — so this snapshot is honestly a record of how little
    // was known when the run started.
    run.emit_capacity_snapshot(&BTreeMap::new())?;
    let report = run.drain_and_report()?;
    Ok((report, run.state.clone()))
}

/// `[budgets]` with `--budget` folded in.
///
/// The flag overrides `run_usd` only. `task_usd` has no flag because a
/// per-task ceiling is a property of how the plan is shaped, not of one
/// invocation — and a single `--budget` that quietly moved both would be
/// impossible to reason about at the ledger afterwards.
fn effective_budgets(
    configured: config::Budgets,
    flag: Option<f64>,
) -> Result<config::Budgets, TactusError> {
    // Validated through the same check `[budgets]` uses. A flag that overrides
    // a validated key must not be a way around the validation: `--budget 0` and
    // `--budget -5` both stop the run before it spends anything, and
    // `--budget nan` silently never fires at all — three different broken
    // behaviours behind one mistyped number, where the config key refuses all
    // three at load.
    if let Some(limit) = flag {
        config::check_budget("--budget", limit)
            .map_err(|message| TactusError::Refused { message })?;
    }
    Ok(config::Budgets {
        run_usd: flag.or(configured.run_usd),
        task_usd: configured.task_usd,
    })
}

/// A path as the run record should carry it: relative to the repo root where
/// possible, so the record survives the repository being moved or cloned
/// somewhere else before a resume.
fn repo_relative(repo_root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// The resolved chain per task, as it stood at this moment.
fn chain_summaries(analysis: &Analysis) -> Vec<ChainSummary> {
    analysis
        .plan
        .tasks
        .iter()
        .zip(&analysis.chains)
        .map(|(task, chain)| ChainSummary {
            task: task.id.to_string(),
            tiers: chain.rungs.iter().map(|rung| rung.tier).collect(),
            attempts_per: chain.attempts_per,
            bindings: Some(
                chain
                    .rungs
                    .iter()
                    .map(|rung| BindingSummary {
                        tier: rung.tier,
                        agent: rung.binding.agent.clone(),
                        model: rung.binding.model.clone(),
                        pinned: rung.binding.pinned,
                    })
                    .collect(),
            ),
        })
        .collect()
}

/// Validate the rung index space and restore the exact bindings the run began
/// with. Structural changes still refuse: an existing `Progress.rung` cannot be
/// interpreted against a different tier list. Binding-only changes warn and
/// continue with the snapshot, matching gates and effort.
fn restore_recorded_routing(
    analysis: &mut Analysis,
    recorded: &RecordedRouting,
    warnings: &mut Vec<String>,
) -> Result<(), TactusError> {
    let current = chain_summaries(analysis);
    let same_structure = current.len() == recorded.structure.len()
        && current.iter().zip(&recorded.structure).all(|(now, then)| {
            now.task == then.task
                && now.tiers == then.tiers
                && now.attempts_per == then.attempts_per
        });
    if !same_structure {
        let moved: Vec<String> = current
            .iter()
            .zip(&recorded.structure)
            .filter(|(now, then)| {
                now.task != then.task
                    || now.tiers != then.tiers
                    || now.attempts_per != then.attempts_per
            })
            .map(|(now, then)| {
                format!(
                    "`{}` ran on [{}] with {} attempt(s) per rung and would now run on [{}] with {}",
                    then.task,
                    render_tiers(then),
                    then.attempts_per,
                    render_tiers(now),
                    now.attempts_per,
                )
            })
            .collect();
        let detail = if moved.is_empty() {
            format!(
                "the run recorded {} task chain(s), while today's plan resolves {}",
                recorded.structure.len(),
                current.len()
            )
        } else {
            moved.join("; ")
        };
        return Err(TactusError::Resume {
            run_id: recorded.run_id.clone(),
            message: format!(
                "routing has changed since this run started, so a recorded rung would now mean a \
                 different tier or allowance: {detail}. Restore the config it ran with, or start \
                 a new run."
            ),
        });
    }

    let Some(snapshot) = recorded.bindings.as_ref() else {
        warnings.push(
            "this run's log predates the resolved-binding record, so worker agent/model bindings \
             were re-derived from today's config rather than read from the run — earlier attempts \
             may have used different bindings"
                .to_owned(),
        );
        return Ok(());
    };
    if snapshot.len() != analysis.chains.len() {
        return Err(TactusError::Resume {
            run_id: recorded.run_id.clone(),
            message: "the recorded binding snapshot does not align with the run's task chains; \
                      the event log cannot safely identify which model belongs to which task"
                .to_owned(),
        });
    }

    let mut changed = Vec::new();
    for ((chain, now), then) in analysis.chains.iter_mut().zip(&current).zip(snapshot) {
        if then.task != now.task || then.tiers != now.tiers || then.attempts_per != now.attempts_per
        {
            return Err(TactusError::Resume {
                run_id: recorded.run_id.clone(),
                message: format!(
                    "the recorded binding snapshot for `{}` does not match its frozen chain",
                    then.task
                ),
            });
        }
        let Some(bindings) = then.bindings.as_ref() else {
            return Err(TactusError::Resume {
                run_id: recorded.run_id.clone(),
                message: format!(
                    "the recorded binding snapshot for `{}` is missing its bindings",
                    then.task
                ),
            });
        };
        if bindings.len() != chain.rungs.len() {
            return Err(TactusError::Resume {
                run_id: recorded.run_id.clone(),
                message: format!(
                    "the recorded binding snapshot for `{}` has {} binding(s) for {} rung(s)",
                    then.task,
                    bindings.len(),
                    chain.rungs.len()
                ),
            });
        }
        for (rung, binding) in chain.rungs.iter_mut().zip(bindings) {
            if binding.tier != rung.tier {
                return Err(TactusError::Resume {
                    run_id: recorded.run_id.clone(),
                    message: format!(
                        "the recorded binding snapshot for `{}` assigns tier `{}` to a `{}` rung",
                        then.task, binding.tier, rung.tier
                    ),
                });
            }
            if rung.binding.agent != binding.agent
                || rung.binding.model != binding.model
                || rung.binding.pinned != binding.pinned
            {
                changed.push(format!(
                    "`{}` {}: recorded {}/{}, today {}/{}",
                    then.task,
                    rung.tier,
                    binding.agent,
                    binding.model,
                    rung.binding.agent,
                    rung.binding.model
                ));
            }
            rung.binding.agent = binding.agent.clone();
            rung.binding.model = binding.model.clone();
            rung.binding.pinned = binding.pinned;
        }
    }
    if !changed.is_empty() {
        warnings.push(format!(
            "today's worker bindings differ from the ones this run recorded ({}). This resume \
             keeps the recorded bindings. Start a new run to adopt today's routing.",
            changed.join("; ")
        ));
    }
    Ok(())
}

/// The effective gates, in full, as they stood at this moment.
fn gate_summaries(analysis: &Analysis) -> Vec<GateSummary> {
    analysis
        .gates
        .iter()
        .map(|gate| GateSummary {
            name: gate.name.clone(),
            cmd: gate.cmd.clone(),
            timeout: gate.timeout,
            shell: gate.shell,
        })
        .collect()
}

/// What to continue, and what may be overridden while continuing it.
#[derive(Debug, Clone)]
pub struct ResumeOptions {
    /// Run id, or any unambiguous prefix of one.
    pub run_id: String,
    pub repo_root: PathBuf,
    /// `None` takes the config the run recorded.
    pub config_path: Option<PathBuf>,
    pub pools_path: Option<PathBuf>,
    pub interaction: Option<InteractionMode>,
    pub attempt_timeout: Duration,
    pub defer_backoff: Duration,
    pub max_defers: u32,
    pub private_root: Option<PathBuf>,
    pub wait_on_block: Option<Duration>,
    /// `--budget <usd>` (§17), overriding `[budgets] run_usd` for this resume.
    ///
    /// Budgets are **re-derived from today's config and flags**, unlike the
    /// three things a resume takes from the run's own record: the plan (frozen,
    /// and refused on a hash mismatch), the resolved chains (refused, because a
    /// recorded rung is an index into one), and the gates and reviewers (taken
    /// and used, because they are what "this code was verified" means). Those
    /// protect a run's *identity*. A budget is not identity — it is an
    /// operator's ceiling on their own spending, and re-reading it is precisely
    /// what makes a budget stop recoverable in one command instead of a dead
    /// run and a new branch.
    pub budget_usd: Option<f64>,
}

impl ResumeOptions {
    pub fn new(run_id: String, repo_root: PathBuf) -> Self {
        Self {
            run_id,
            repo_root,
            config_path: None,
            pools_path: None,
            interaction: None,
            attempt_timeout: DEFAULT_ATTEMPT_TIMEOUT,
            defer_backoff: interaction::DEFAULT_DEFER_BACKOFF,
            max_defers: DEFAULT_MAX_DEFERS,
            private_root: None,
            wait_on_block: None,
            budget_usd: None,
        }
    }
}

pub fn resume(opts: &ResumeOptions) -> Result<RunReport, TactusError> {
    resume_with(opts, &BuiltinAdapters)
}

pub fn resume_with(
    opts: &ResumeOptions,
    adapters: &dyn AdapterSource,
) -> Result<RunReport, TactusError> {
    resume_harness(opts, &Harness::new(adapters))
}

/// §15: replay, verify the run branch still matches the record, re-probe, and
/// continue — parked questions intact.
///
/// Every refusal below exists because continuing would produce a *wrong*
/// result rather than merely an awkward one, and each says which of the four
/// things moved — the run, the plan, the config, or the branch — because that
/// is what decides what the operator does next.
///
/// Note what is *not* a refusal: gates that resolve differently today. Those
/// are taken from the record and run, so there is nothing to refuse — the
/// difference is a warning about an edit that does not apply here. A refusal is
/// for the cases where continuing would be wrong, and continuing under the
/// gates this run has been using all along is exactly right.
pub fn resume_harness(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
) -> Result<RunReport, TactusError> {
    resume_harness_inner(opts, harness).map(|(report, _)| report)
}

fn resume_harness_inner(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
) -> Result<(RunReport, RunState), TactusError> {
    let run_id = rundir::resolve_run_id(&opts.repo_root, &opts.run_id)?;
    let public = rundir::public_dir(&opts.repo_root, &run_id);
    let refuse = |message: String| TactusError::Resume {
        run_id: run_id.clone(),
        message,
    };
    let workspace = Workspace::open(&opts.repo_root)?;
    let worktree_git_dir = workspace.worktree_git_dir()?;
    let _worktree_lock = WorktreeLock::acquire_in(workspace.root(), &worktree_git_dir)?;

    // Claimed before anything is read, so two resumes cannot race each other
    // into the same branch. The lock sits beside the ops surface, which is the
    // only half of the run directory known this early: where the private half
    // went is recorded in `run_started`, and that has not been read yet.
    let _lock = RunLock::acquire(&public)?;
    let _cleanup_scope = _lock.enter_cleanup_scope();

    let mut warnings = Vec::new();
    let events_path = public.join("events.jsonl");
    let events = events::read_all(&events_path, &mut warnings)?;
    let started = events::started_of(&events, &events_path)?.clone();
    let effective_schema = events::ensure_supported_schema(&started, &events, &events_path)?;
    let recorded_normalized_plan_digest =
        events::recorded_normalized_plan_digest(&events).map(str::to_owned);
    let frozen_plan_path = public.join("plan.normalized.json");
    let frozen_plan_bytes = fs::read(&frozen_plan_path).map_err(|source| TactusError::Io {
        path: frozen_plan_path.clone(),
        source,
    })?;
    let frozen_plan_digest = events::normalized_plan_digest(&frozen_plan_bytes);
    if let Some(recorded) = recorded_normalized_plan_digest.as_deref() {
        if frozen_plan_digest != recorded {
            return Err(refuse(format!(
                "the exact bytes at {} no longer match this run's recorded normalized-plan digest ({recorded}, now {frozen_plan_digest}). Restore the frozen snapshot or start a new run.",
                frozen_plan_path.display()
            )));
        }
    }
    if let Some(failure) = events::legacy_unsettled_failure(started.schema, &events) {
        let detail = match failure.kind {
            events::LegacyUnsettledFailureKind::MissingDecision => {
                "without its durable ladder or parking decision"
            }
            events::LegacyUnsettledFailureKind::MissingSpendParking => {
                "after raising an ApproveSpend question but before durably parking the task"
            }
        };
        return Err(refuse(format!(
            "legacy event schema {} records failed attempt {} for `{}` on rung {} {detail}. The old writer may have stopped between two appends, so resuming could repeat paid work, choose the wrong rung, or bypass required spend approval. Preserve this log for recovery and start a new run rather than guessing.",
            started.schema, failure.attempt, failure.task, failure.rung,
        )));
    }
    // Usually `run_started`'s, but a log too old to carry them there may have
    // had them established by an earlier resume instead — which is what stops
    // the re-derivation repeating, and drifting, on every resume after that.
    let recorded_gates = events::recorded_gates(&events).cloned();
    let recorded_effort_policy = events::recorded_effort_policy(&events);
    let recorded_complete_reviews = events::recorded_complete_reviews(&events).cloned();
    let recorded_reviews = events::recorded_reviews(&events).cloned();
    let recorded_chains = events::recorded_chains(&events).cloned();

    // The run knows its own plan and config; the CLI may override the config
    // but never the plan, which is frozen (§5).
    let mut run_opts = RunOptions::new(
        opts.repo_root.join(&started.plan_path),
        opts.repo_root.clone(),
    );
    run_opts.config_path = opts
        .config_path
        .clone()
        .or_else(|| started.config_path.as_ref().map(|p| opts.repo_root.join(p)));
    run_opts.pools_path = opts.pools_path.clone();
    run_opts.interaction = opts.interaction;
    run_opts.attempt_timeout = opts.attempt_timeout;
    run_opts.defer_backoff = opts.defer_backoff;
    run_opts.max_defers = opts.max_defers;
    run_opts.private_root = opts.private_root.clone();
    run_opts.wait_on_block = opts.wait_on_block;
    let wait_on_block = opts.wait_on_block;

    // Re-probes agents and re-reads config, exactly as a fresh run does —
    // except for the two things that are facts about *this run* rather than
    // about today's machine: who reviews it and what verifies it. Both come
    // from the record (see `preflight_with_recorded`), so a resume continues
    // the run it is resuming rather than starting a differently-judged one on
    // the same branch.
    let Preflight {
        analysis,
        caps,
        review_plan,
        review_pass_timeout,
        gates,
        gate_cmds,
        warnings: preflight_warnings,
        mode,
        notifiers,
        budgets,
    } = preflight_with_recorded(
        &run_opts,
        harness,
        Recorded {
            reviews: recorded_reviews.clone(),
            gates: recorded_gates.clone(),
            legacy_review_timeout_missing: recorded_reviews
                .as_ref()
                .is_some_and(|plan| plan.pass_timeout_secs.is_none()),
            gates_from_config: started.gates_from_config,
            routing: Some(RecordedRouting {
                run_id: run_id.clone(),
                structure: started.chains.clone(),
                bindings: recorded_chains.clone(),
            }),
        },
    )?;
    if recorded_reviews.is_none() {
        warnings.push(
            "this run's log predates the review record (step 9), so who reviews was re-derived \
             from today's config rather than read from the run — earlier tasks may have been \
             judged differently"
                .to_owned(),
        );
    }
    if recorded_gates.is_none() {
        // A log from before the gate record, resumed for the first time — the
        // only case with nothing to rebuild from, since this resume writes down
        // what it settles on and the next one is ordinary.
        //
        // It still recorded gate *names*, which is not enough to rebuild the
        // gates but is enough to say something better than "anything may have
        // changed": if the names have moved, that is proof rather than
        // suspicion, and if they have not, the only undetectable edit left is a
        // command behind an unchanged name.
        let names_now: Vec<String> = gates.iter().map(|gate| gate.name.clone()).collect();
        if names_now != started.gates {
            warnings.push(format!(
                "this run's log predates the gate record, so its gates were re-derived from \
                 today's config — and the gate names have moved, so the tasks it already \
                 committed were verified differently: it recorded [{}], today resolves [{}]",
                render_names(&started.gates),
                render_names(&names_now),
            ));
        } else if !names_now.is_empty() {
            warnings.push(format!(
                "this run's log predates the gate record, so its gates were re-derived from \
                 today's config rather than rebuilt from the run. The names still match what it \
                 recorded ([{}]), but a log this old cannot show whether a command behind one of \
                 them changed",
                render_names(&names_now),
            ));
        }
        // Both empty: the run recorded no gates and none resolve today, so
        // there is nothing a command could have hidden behind. Saying "may have
        // been verified differently" here would be a false alarm on every
        // gateless run, and a warning that cries wolf on the harmless case is
        // one nobody reads on the harmful one.
    }
    let current_effort_policy = analysis.config.resolved_effort_policy();
    let effort_policy = recorded_effort_policy.unwrap_or(current_effort_policy);
    match recorded_effort_policy {
        None => warnings.push(
            "this run's log predates the effort-policy record, so implementation and review \
             effort were re-derived from today's config rather than read from the run — earlier \
             attempts may have used a different effort standard"
                .to_owned(),
        ),
        Some(recorded) if recorded != current_effort_policy => warnings.push(format!(
            "today's effort policy ({}) differs from the one this run recorded ({}). This \
             resume keeps the recorded policy so one run has one execution and review standard. \
             Start a new run to adopt today's policy.",
            render_effort_policy(current_effort_policy),
            render_effort_policy(recorded),
        )),
        Some(_) => {}
    }
    warnings.extend(preflight_warnings);

    // The plan is frozen. A different hash means the file moved under the run,
    // so every task index in the log — which is what `Progress` is keyed by —
    // may now mean a different task.
    if analysis.plan.source.hash != started.plan_hash {
        return Err(refuse(format!(
            "the plan at {} has changed since this run froze it (recorded {}, now {}). Task \
             progress is recorded per task, so replaying it against a different plan would \
             attribute work to the wrong tasks. Restore the plan, or start a new run.",
            run_opts.plan_path.display(),
            started.plan_hash,
            analysis.plan.source.hash
        )));
    }
    let canonical_plan_bytes = normalized_plan_bytes(&analysis.plan, &frozen_plan_path)?;
    let canonical_plan_digest = events::normalized_plan_digest(&canonical_plan_bytes);
    let established_normalized_plan_digest = if let Some(recorded) =
        recorded_normalized_plan_digest.as_deref()
    {
        if canonical_plan_digest != recorded {
            return Err(refuse(format!(
                "the validated source plan now normalizes to digest {canonical_plan_digest}, but this run recorded {recorded}. Restore the source plan semantics or start a new run."
            )));
        }
        None
    } else {
        if canonical_plan_bytes != frozen_plan_bytes {
            return Err(refuse(format!(
                "legacy frozen plan {} does not exactly match the canonical serialization of the validated source plan. Refusing to bless a mutable legacy snapshot during the schema-3 upgrade; restore it or start a new run.",
                frozen_plan_path.display()
            )));
        }
        Some(frozen_plan_digest.clone())
    };

    let task_ids: Vec<String> = analysis
        .plan
        .tasks
        .iter()
        .map(|task| task.id.to_string())
        .collect();
    let replayed = events::replay(events, task_ids, &events_path)?;

    match replayed.state.finished.as_ref().map(|f| &f.outcome) {
        Some(events::RunOutcome::Complete) => {
            return Err(refuse(
                "this run already completed; there is nothing left to continue".to_owned(),
            ));
        }
        Some(events::RunOutcome::Halted) => {
            return Err(refuse(format!(
                "this run halted at `{}` under `on_task_failure = \"halt\"`. Nothing can run \
                 while it is halted — fix what failed and start a new run.",
                replayed.state.halted_at.as_deref().unwrap_or("?")
            )));
        }
        // Ended parked, at a budget, or never ended at all — all three are
        // exactly what resume is for. A budget stop in particular is *designed*
        // to be resumable: `--budget` re-derives the ceiling (see
        // `ResumeOptions::budget_usd`), so raising it and continuing is one
        // command rather than a new run and a lost branch.
        Some(events::RunOutcome::Parked | events::RunOutcome::BudgetExceeded) | None => {}
    }

    // `question_answered`, its design-defect record, and a declined task's
    // failure predate atomic parking and are three durable appends. Preserve
    // every crash prefix so a closed question can never strand its task in
    // AwaitingInput with no legal way to answer it again.
    let defect_questions: BTreeSet<QuestionId> = replayed
        .events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::DesignDefect { data } => Some(data.question.clone()),
            _ => None,
        })
        .collect();
    let missing_answer_defects: Vec<_> = replayed
        .state
        .questions
        .iter()
        .filter_map(|record| {
            let answer = record.answer.as_ref()?;
            (!defect_questions.contains(&record.question.id)).then(|| {
                (
                    record.question.id.clone(),
                    util::head(record.question.context.trim(), 600),
                    match answer {
                        Answer::Answered { text } => text.clone(),
                        _ => "declined".to_owned(),
                    },
                )
            })
        })
        .collect();
    let decline_halt_policies: BTreeMap<_, _> = replayed
        .events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::QuestionAnswered { data } if data.answer == Answer::Declined => {
                Some((data.question.clone(), data.decline_halts_run))
            }
            _ => None,
        })
        .collect();
    let mut declined_questions = Vec::new();
    for record in replayed
        .state
        .questions
        .iter()
        .filter(|record| record.answer.as_ref() == Some(&Answer::Declined))
    {
        let affected: Vec<_> = record
            .question
            .affected_tasks
            .iter()
            .filter(|task_id| {
                replayed
                    .state
                    .index_of(task_id.as_str())
                    .is_some_and(|index| {
                        matches!(
                            &replayed.state.states[index],
                            TaskState::AwaitingInput(open) if open == &record.question.id
                        )
                    })
            })
            .cloned()
            .collect();
        if affected.is_empty() {
            continue;
        }
        let Some(halts_run) = decline_halt_policies
            .get(&record.question.id)
            .copied()
            .flatten()
        else {
            return Err(refuse(format!(
                "legacy declined answer {} stopped before settling its affected task, but the log does not record the contemporaneous on_task_failure policy. Today's config cannot safely decide an old answer; preserve this log for recovery and start a new run.",
                record.question.id
            )));
        };
        declined_questions.push((record.question.id.clone(), affected, halts_run));
    }

    // Resolve the recorded private root before touching the worktree so a
    // killed engine's durable snapshot registrations are reclaimed first.
    let paths = match &opts.private_root {
        Some(root) => RunPaths::with_private_root(&opts.repo_root, &run_id, root),
        None => RunPaths::from_parts(public.clone(), PathBuf::from(&started.private_dir)),
    };
    paths.create()?;

    let reclaimed = workspace.reclaim_gate_workspaces(&paths.gate_worktrees())?;
    if reclaimed > 0 {
        warnings.push(format!(
            "reclaimed {reclaimed} gate/review snapshot worktree(s) left by the interrupted run"
        ));
    }
    workspace.ensure_execution_prerequisites()?;
    workspace.ensure_run_exclusions()?;
    if !workspace.branch_exists(&started.branch)? {
        return Err(refuse(format!(
            "the run branch `{}` no longer exists. Its commits are what this run's record \
             refers to; without it there is nothing to continue onto.",
            started.branch
        )));
    }
    if workspace.current_branch()? != started.branch {
        if !workspace.is_clean()? {
            return Err(refuse(format!(
                "you have uncommitted changes and are not on `{}`. Commit or stash them, then \
                 resume — switching branches over them would lose work that is not this run's \
                 to discard.",
                started.branch
            )));
        }
        workspace.switch_branch(&started.branch)?;
    }

    // §15's check, before anything is discarded: if HEAD moved, refusing has
    // to leave the operator's tree exactly as they left it.
    let recorded_head = last_committed_sha(&replayed.events).unwrap_or(started.base_sha.clone());
    let mut head = workspace.head_sha_full()?;

    // A schema-3 successful settlement durably names the exact commit object
    // that passed review. Recovery may publish that object from its pin, or
    // finish recording it when HEAD already advanced. Subject/parent matching
    // is intentionally insufficient: another commit can share both while
    // containing arbitrary bytes.
    let mut adopted = None;
    if let Some((task, message, prepared)) = unrecorded_commit(&replayed, &analysis.plan) {
        let Some(prepared) = prepared else {
            if head != recorded_head {
                return Err(refuse(format!(
                    "`{}` is at {head}, but the successful legacy settlement for `{task}` did \
                     not record an exact prepared commit. Refusing to adopt a commit by subject \
                     alone; move the branch back to {recorded_head}, or start a new run.",
                    started.branch
                )));
            }
            return Err(refuse(format!(
                "the successful legacy settlement for `{task}` has no exact prepared commit. \
                 It cannot be replayed safely; preserve this log for recovery and start a new run."
            )));
        };
        if prepared.parent_sha != recorded_head
            || prepared.message != message
            || !workspace.prepared_commit_matches(&prepared)?
        {
            return Err(refuse(format!(
                "the recorded prepared commit for `{task}` does not match its task, parent, or \
                 Git object. Refusing to publish or adopt it; preserve the log for recovery."
            )));
        }
        let observed_branch_ref = workspace.current_branch_ref()?;
        if observed_branch_ref != prepared.branch_ref {
            return Err(refuse(format!(
                "HEAD is on `{observed_branch_ref}`, not the prepared commit's recorded branch \
                 `{}`; refusing prepared recovery.",
                prepared.branch_ref
            )));
        }

        if head == prepared.parent_sha {
            if workspace.prepared_pin_target(&prepared.pin_ref)?.as_deref()
                != Some(prepared.commit_sha.as_str())
            {
                return Err(refuse(format!(
                    "the recorded prepared commit for `{task}` is not pinned by `{}`. Refusing \
                     to publish an unprotected or substituted object; preserve the log for recovery.",
                    prepared.pin_ref
                )));
            }
            workspace.advance_prepared_commit(&prepared.branch_ref, &prepared)?;
            head = prepared.commit_sha.clone();
            warnings.push(format!(
                "published prepared commit {head} for `{task}` after the run stopped between \
                 settlement and the branch update"
            ));
            adopted = Some((task, message));
        } else if head == prepared.commit_sha {
            match workspace.prepared_pin_target(&prepared.pin_ref)? {
                Some(target) if target == prepared.commit_sha => {
                    workspace.remove_prepared_pin(&prepared)?;
                }
                Some(target) => {
                    return Err(refuse(format!(
                        "prepared ref `{}` points at {target}, not the recorded commit {}; \
                         refusing to delete or adopt a substituted object.",
                        prepared.pin_ref, prepared.commit_sha
                    )));
                }
                None => {}
            }
            warnings.push(format!(
                "adopted commit {head} as `{task}` from its exact prepared identity after the \
                 run stopped before recording it"
            ));
            adopted = Some((task, message));
        }
    }

    if adopted.is_none() && head != recorded_head {
        return Err(refuse(format!(
            "`{}` is at {head}, but this run's record ends at {recorded_head}. Something \
             committed, reset, or rebased the branch after the run stopped, so replaying the \
             log would describe work that is no longer what is on the branch. Move the branch \
             back to {recorded_head}, or start a new run.",
            started.branch
        )));
    }

    // A pin with no successful settlement is from a crash between preparing
    // the object and appending AttemptFinished. It has no authority to move
    // HEAD and is removed with an expected-old-value CAS before retrying.
    for interrupted in replayed.state.interrupted_attempts() {
        let task_index = replayed
            .state
            .index_of(&interrupted.task)
            .expect("an interrupted task belongs to the replayed plan");
        let pin_ref = prepared_pin_ref(&run_id, task_index, interrupted.flight.attempt);
        if workspace.prepared_pin_target(&pin_ref)?.is_some() {
            workspace.remove_orphan_prepared_pin(&pin_ref)?;
            warnings.push(format!(
                "removed orphan prepared commit pin `{pin_ref}` for interrupted attempt {}",
                interrupted.flight.attempt
            ));
        }
    }

    // Crash residue: a dead agent's half-written edits. §14 rolls a failed
    // attempt back to the last commit, and an attempt that never reported is
    // no different — the session that would have explained these edits is
    // gone, so nothing can verify them.
    let discarded = workspace.uncommitted_summary()?;
    if !discarded.is_empty() {
        warnings.push(format!(
            "discarded {} uncommitted path(s) left by the interrupted run: {}",
            discarded.len(),
            discarded.join(", ")
        ));
        workspace.discard_uncommitted()?;
    }

    // Where the agent-authored half lives is a fact about the run, not about
    // today's defaults. A resume under a different HOME — a service account, a
    // container, the no-home fallback — would otherwise scatter the rest of
    // this run's transcripts into a second private root while `status` went on
    // pointing at the first. An explicit override still wins, for a private
    // root that has genuinely moved.
    let sleeper = harness.sleeper.unwrap_or(&RealSleeper);
    let default_answers = interaction::answers_for(
        mode,
        paths.answers(),
        wait_on_block.unwrap_or(analysis.config.wait_on_block),
        sleeper,
    );
    // §13's ground-truth signals, folded from this run's own log before its
    // state is moved into the scheduler — what the earlier process learned
    // about the pools, which a resumed run's snapshot must not forget.
    let prior_signals = capacity::observe(&replayed.events).exhausted;
    let log = EventLog::open(&paths.events(), &mut warnings)?;
    let established_reviews = recorded_complete_reviews
        .is_none()
        .then(|| review_plan.clone());
    let mut run = Run {
        state: replayed.state,
        analysis: &analysis,
        workspace: &workspace,
        paths,
        log,
        gate_cmds,
        adapters: harness.adapters,
        answers: harness.answers.unwrap_or(default_answers.as_ref()),
        notifiers,
        sleeper,
        caps,
        review_plan,
        effort_policy,
        attempt_timeout: opts.attempt_timeout,
        review_pass_timeout,
        defer_backoff: opts.defer_backoff,
        max_defers: opts.max_defers,
        on_task_failure: analysis.config.on_task_failure,
        // Re-derived from today's config and flags, deliberately (see
        // `ResumeOptions::budget_usd`): raising the ceiling and resuming is the
        // one-command recovery a budget stop is supposed to have.
        budgets,
        ask_before: analysis.config.ask_before,
        run_id,
        branch: started.branch.clone(),
        warnings,
        unanswerable: Vec::new(),
        // Seeded from the log so a resume neither re-announces an outage the
        // previous process recorded nor swallows a fresh one.
        exhausted_pools: prior_signals.keys().cloned().collect(),
        #[cfg(test)]
        after_candidate_capture: None,
    };
    // The `task_committed` the dead process never got to must be the first
    // append after its successful settlement. Schema 3 treats that adjacency
    // as part of the exact prepared-commit binding, so unrelated legacy answer
    // repairs cannot interpose and poison the log.
    if let Some((task, message)) = adopted {
        run.emit(EventBody::TaskCommitted {
            task,
            data: events::TaskCommitted {
                sha: head.clone(),
                message,
            },
        })?;
    }
    // A legacy run cannot have its opening event rewritten without violating
    // append-only history. This no-op event is the current downgrade boundary:
    // schema-1 binaries do not know its tag, while schema-2 binaries reject a
    // transition to schema 3 before applying their old partial-review contract.
    if effective_schema < events::SCHEMA_VERSION {
        run.emit(EventBody::RunSchemaUpgraded {
            data: events::RunSchemaUpgraded {
                from: effective_schema,
                to: events::SCHEMA_VERSION,
            },
        })?;
    }
    for (question, context, answer) in missing_answer_defects {
        run.emit(EventBody::DesignDefect {
            data: events::DesignDefect {
                question,
                context,
                answer,
            },
        })?;
    }
    for (question, affected, halts_run) in declined_questions {
        for task_id in affected {
            let Some(index) = run.state.index_of(task_id.as_str()) else {
                continue;
            };
            if !matches!(&run.state.states[index], TaskState::AwaitingInput(open) if open == &question)
            {
                continue;
            }
            let reason = format!(
                "declined at the human rung: {}",
                last_reason(&run.state.progress[index])
            );
            run.fail_task_with_policy(index, FailureKind::Declined, reason, halts_run)?;
        }
    }
    // A crash between `question_answered` and the payload rewrite leaves a
    // file that still reads as open, which `tactus answer` would accept a
    // second answer against — one no engine can ever ingest, because the
    // question is already closed in the log. The log is what is authoritative;
    // make the payloads agree with it again.
    for record in &run.state.questions {
        interaction::write_question(&run.paths.questions(), record)?;
    }

    // Write the `attempt_finished` the dead process never got to.
    //
    // Recorded rather than settled in memory, because a settlement only a
    // reader performs is lost the moment someone else replays the log: the
    // ledger line vanishes and, worse, the rung's refunded allowance vanishes
    // with it, so a later resume would think the attempt had been spent.
    let interrupted = run.state.interrupted_attempts();
    for attempt in &interrupted {
        run.emit(attempt.event())?;
    }

    // Applying this is what drops every session and wakes deferred work — the
    // §14 pairing, enforced by the same fold a replay uses rather than by this
    // function remembering to do it.
    run.emit(EventBody::RunResumed {
        data: events::RunResumed {
            head_sha: head,
            interrupted_attempts: u32::try_from(interrupted.len()).unwrap_or(u32::MAX),
            discarded,
            // Only when this resume is the one that had to settle the question.
            // Where the log already answers it, re-stating the answer would put
            // the same fact in two places that a later change could pull apart.
            gates: recorded_gates.is_none().then(|| gates.clone()),
            effort_policy: recorded_effort_policy.is_none().then_some(effort_policy),
            reviews: established_reviews,
            chains: recorded_chains
                .is_none()
                .then(|| chain_summaries(&analysis)),
            normalized_plan_digest: established_normalized_plan_digest,
        },
    })?;
    // §14 takes a capacity snapshot at pre-flight, and §15 makes a resume
    // re-establish everything a fresh run establishes. A resume that skipped it
    // would leave the log claiming the pools looked, hours later, exactly as
    // they did when the run began.
    run.emit_capacity_snapshot(&prior_signals)?;
    let report = run.drain_and_report()?;
    Ok((report, run.state.clone()))
}

fn render_tiers(chain: &ChainSummary) -> String {
    chain
        .tiers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" → ")
}

/// A gate name list, for a message.
fn render_names(names: &[String]) -> String {
    names.join(", ")
}

fn render_effort_policy(policy: ResolvedEffortPolicy) -> String {
    format!(
        "implementation small={}, mid={}, frontier={}; review={}",
        policy.small, policy.mid, policy.frontier, policy.review
    )
}

/// What today's config would gate with, against what the run recorded — `None`
/// when they agree.
///
/// This is a **warning**, not a refusal: the run continues under the gates it
/// recorded, and the operator's edit simply does not apply to it. Saying so is
/// still worth a line, because an edit that silently does nothing is how
/// somebody concludes the gate is broken.
///
/// Matching is by whole gate, then paired up by name, which is what makes the
/// message survive the shapes a by-name lookup got wrong: duplicate names are
/// legal in `[[gates]]`, so `find`-by-name silently answers for the wrong entry
/// — reporting an edit nobody made, or finding every name present and claiming
/// a reorder when a gate was added.
fn gates_differ(recorded: &[GateSummary], now: &[GateSummary]) -> Option<String> {
    if recorded == now {
        return None;
    }
    // Whole-gate multiset difference: what the record has that today lacks, and
    // the reverse. Anything appearing in both cancels, however many times.
    let mut unmatched: Vec<&GateSummary> = now.iter().collect();
    let mut dropped: Vec<&GateSummary> = Vec::new();
    for gate in recorded {
        match unmatched.iter().position(|other| *other == gate) {
            Some(index) => {
                unmatched.remove(index);
            }
            None => dropped.push(gate),
        }
    }
    if dropped.is_empty() && unmatched.is_empty() {
        // Same gates, listed in a different order. Worth a line — the record is
        // what runs, and the order it runs in decides which failure a task sees
        // first — but not the same claim as a changed command.
        return Some(
            "the gates in today's config are the ones this run recorded, in a different order; \
             it continues in its recorded order"
                .to_owned(),
        );
    }
    // A name in exactly one dropped and one added gate is one gate edited, not
    // one removed and an unrelated one added. Only when it is unambiguous:
    // with duplicates, "which `check` became which" has no answer worth
    // guessing at, so both sides are reported plainly instead.
    let once = |gates: &[&GateSummary], name: &str| {
        gates.iter().filter(|gate| gate.name == name).count() == 1
    };
    let mut items: Vec<String> = Vec::new();
    let mut paired: Vec<&GateSummary> = Vec::new();
    for gate in &dropped {
        let edited = unmatched
            .iter()
            .find(|other| {
                other.name == gate.name
                    && once(&dropped, &gate.name)
                    && once(&unmatched, &gate.name)
            })
            .copied();
        match edited {
            Some(other) => {
                paired.push(other);
                items.push(format!("`{}` {}", gate.name, changes_between(gate, other)));
            }
            None => items.push(format!(
                "`{}` (`{}`) is in the record and not in today's config",
                gate.name, gate.cmd
            )),
        }
    }
    for gate in unmatched {
        if paired.iter().any(|other| std::ptr::eq(*other, gate)) {
            continue;
        }
        items.push(format!(
            "`{}` (`{}`) is in today's config and not in the record",
            gate.name, gate.cmd
        ));
    }
    Some(format!(
        "the gates in today's config differ from the ones this run recorded, and a run keeps the \
         gates it started with, so these edits do not apply to it: {}. Start a new run to adopt \
         them.",
        items.join("; ")
    ))
}

/// How one gate's recorded form and its form in today's config differ.
fn changes_between(recorded: &GateSummary, now: &GateSummary) -> String {
    let mut parts: Vec<String> = Vec::new();
    if recorded.cmd != now.cmd {
        parts.push(format!(
            "runs `{}` and today's config says `{}`",
            recorded.cmd, now.cmd
        ));
    }
    if recorded.shell != now.shell {
        parts.push(format!(
            "runs under `{}` and today's config says `{}`",
            recorded.shell.program(),
            now.shell.program()
        ));
    }
    if recorded.timeout != now.timeout {
        parts.push(format!(
            "has {}s to finish and today's config allows {}s",
            recorded.timeout.as_secs(),
            now.timeout.as_secs()
        ));
    }
    parts.join(", and ")
}

/// The sha the run's record ends at — what HEAD must still be.
fn last_committed_sha(events: &[events::Event]) -> Option<String> {
    events.iter().rev().find_map(|event| match &event.body {
        EventBody::TaskCommitted { data, .. } => Some(data.sha.clone()),
        _ => None,
    })
}

/// The task an interrupted run committed without living long enough to record.
///
/// The shape is narrow, which is what makes it safe to act on: the log must
/// *end* at an attempt that passed, for a task that never reached `Done`. No
/// other event can follow, because the process that would have written one is
/// the process that died. Returns the task and the message the engine would
/// have used, so the caller can confirm the commit really is the one it is
/// about to adopt rather than trusting the log's shape alone.
fn unrecorded_commit(
    replayed: &events::Replay,
    plan: &Plan,
) -> Option<(String, String, Option<events::PreparedCommit>)> {
    let EventBody::AttemptFinished {
        task,
        data,
        prepared_commit,
        ..
    } = &replayed.events.last()?.body
    else {
        return None;
    };
    if data.failure.is_some() {
        return None;
    }
    let index = replayed.state.index_of(task)?;
    if replayed.state.states[index] != TaskState::Pending {
        return None;
    }
    let task = plan.tasks.get(index)?;
    Some((
        task.id.to_string(),
        format!("[tactus] {}: {}", task.id, task.title),
        prepared_commit.as_deref().cloned(),
    ))
}

fn prepared_pin_ref(run_id: &str, task_index: usize, attempt: u32) -> String {
    format!("refs/tactus/prepared/{run_id}/{task_index}-{attempt}")
}

struct Run<'a> {
    analysis: &'a Analysis,
    workspace: &'a Workspace,
    paths: RunPaths,
    /// The append-only record. Every mutation below goes through
    /// [`Run::emit`], never straight at `state`.
    log: EventLog,
    /// Derived state — the same fold `resume` and `status` build from the log.
    state: RunState,
    gate_cmds: Vec<String>,
    adapters: &'a dyn AdapterSource,
    answers: &'a dyn AnswerSource,
    notifiers: Vec<&'static dyn Notifier>,
    sleeper: &'a dyn Sleeper,
    /// Probe results per agent id — `session_resume` gates §11.4's resume.
    caps: BTreeMap<String, Caps>,
    /// Who judges each task (§11.2–§11.3), resolved once at pre-flight and
    /// recorded in `run_started`.
    review_plan: ReviewPlan,
    /// The run's recorded effort standard. Both worker attempts and all review
    /// passes read this snapshot, including after a resume under changed config.
    effort_policy: ResolvedEffortPolicy,
    attempt_timeout: Duration,
    /// Independent wall clock for each configured review pass. Frozen in
    /// `review_plan`, materialized once by pre-flight.
    review_pass_timeout: Duration,
    defer_backoff: Duration,
    max_defers: u32,
    on_task_failure: OnTaskFailure,
    /// §17's ceilings, with `--budget` already folded in. Checked before every
    /// spawn; never consulted when deciding *what* binds.
    budgets: config::Budgets,
    /// §12's `ask_before` thresholds.
    ask_before: config::AskBefore,
    run_id: String,
    branch: String,
    warnings: Vec<String>,
    /// Questions no channel could reach a human for. Never asked twice — that
    /// is what stops a hard block spinning.
    ///
    /// Deliberately *not* replayed: it records that a channel was unreachable
    /// in this process, not something true about the run. A question nobody
    /// could answer at 2am is exactly the one the operator answers when they
    /// come back, so a resume has to be free to ask it again.
    unanswerable: Vec<QuestionId>,
    /// Pools this run has already recorded a rate-limit signal for.
    ///
    /// Only the *transition* is worth an event. One outage produces a failed
    /// attempt per deferral (up to `max_defers`), and emitting on each wrote N
    /// identical records of a single fact — inflating any later count of
    /// outages by the deferral factor and repeating the same line N times in
    /// `status --follow`. Retired when an attempt proves the pool is serving
    /// again, mirroring [`capacity::observe`]'s rule so the log the engine
    /// writes and the fold a reader performs agree about when a pool came back.
    ///
    /// Process-local rather than folded state, like `unanswerable`: seeded on
    /// resume from the log's own signals, so a resumed run neither re-announces
    /// an outage the previous process recorded nor misses a fresh one.
    exhausted_pools: std::collections::BTreeSet<String>,
    #[cfg(test)]
    after_candidate_capture: Option<AfterCandidateCapture>,
}

impl Run<'_> {
    /// Append an event and fold it in.
    ///
    /// The only way run state changes. Everything below emits; nothing reaches
    /// past this into `state`, which is what makes a live run and a replay of
    /// its own log the same computation rather than two that agree by
    /// inspection.
    fn emit(&mut self, body: EventBody) -> Result<(), TactusError> {
        let event = self.log.append(body)?;
        self.state.apply(&event);
        Ok(())
    }

    /// Drain, settle, and report.
    fn drain_and_report(&mut self) -> Result<RunReport, TactusError> {
        if let Err(error) = self.drain() {
            // The log already holds everything that happened, including the
            // attempt this died inside — that is what `resume` reads. The
            // report beside it is a courtesy for whoever opens the directory
            // next, and failing to write it must not mask the error that
            // actually stopped the run.
            let partial = self.finish();
            let _ = util::write_json(&self.paths.report_json(), &partial);
            return Err(error);
        }
        let report = self.finish();
        let committed = report
            .tasks
            .iter()
            .filter(|task| matches!(task.status, TaskRunStatus::Committed { .. }))
            .count();
        self.emit(EventBody::RunFinished {
            data: events::RunFinished {
                outcome: match report.outcome() {
                    RunOutcome::Complete => events::RunOutcome::Complete,
                    RunOutcome::Parked => events::RunOutcome::Parked,
                    RunOutcome::Halted => events::RunOutcome::Halted,
                    RunOutcome::BudgetExceeded => events::RunOutcome::BudgetExceeded,
                },
                halted_at: report.halted_at.clone(),
                committed: u32::try_from(committed).unwrap_or(u32::MAX),
                parked: u32::try_from(report.parked_tasks().len()).unwrap_or(u32::MAX),
            },
        })?;
        util::write_json(&self.paths.report_json(), &report)?;
        Ok(report)
    }

    /// Drain the graph (§14, §12).
    ///
    /// The four branches are the whole interaction model: pick up answers that
    /// arrived from somewhere else; run what is ready; if only deferred work is
    /// left, wait for the pool rather than burning attempts against it; and
    /// only when none of those is possible — the precise definition of a hard
    /// block — ask a human.
    ///
    /// **Why this terminates.** Every branch consumes something finite and
    /// nothing replenishes any of them:
    ///
    /// - the answer sweep fires only for an *open* question and closes it, and
    ///   questions are created only by `step_task`;
    /// - `step_task` moves its task out of `Pending`, and the only routes back
    ///   are a deferral — bounded by `max_defers`, after which the ladder parks
    ///   the task instead — or an answer, which closed a question to get there;
    /// - the wait branch requires a `Deferred` task, which only a deferral
    ///   creates;
    /// - the ask branch either closes a question or adds it to `unanswerable`,
    ///   which is only ever appended to and is checked before asking.
    ///
    /// So no cycle exists that does not spend an attempt, a deferral, or a
    /// question. `an_exhausted_pool_and_a_silent_operator_still_terminate`
    /// holds it to that against an adapter that never succeeds and an operator
    /// who never replies.
    fn drain(&mut self) -> Result<(), TactusError> {
        let mut defer_round = 0u32;
        loop {
            // Invariant 6 in its most useful form: an answer that arrives while
            // other work is still running un-parks its task there and then,
            // rather than waiting for the run to have nothing else to do.
            //
            // Guarded on the budget stop like the two branches below, and for a
            // sharper reason than theirs: an answer this run cannot act on is
            // merely wasted, but a *declined* one routes through `fail_task`,
            // which sets `halted_at` — and halted outranks budget in
            // `outcome()`. A decline file sitting on disk would relabel a
            // budget stop as a task failure, so CI gating on exit 3 to raise a
            // ceiling would instead see exit 1 and a task blamed for something
            // the ceiling did. The answers keep for the resume (§15).
            if self.state.budget_stop.is_none() && self.sweep_answers()? {
                continue;
            }
            if let Some(index) = self.next_ready() {
                let deferred = self.step_task(index)?;
                if !deferred {
                    defer_round = 0;
                }
                continue;
            }
            if self.state.states.contains(&TaskState::Deferred)
                && self.state.halted_at.is_none()
                && self.state.budget_stop.is_none()
            {
                let waited = interaction::defer_backoff(self.defer_backoff, defer_round);
                self.sleeper.sleep(waited);
                defer_round = defer_round.saturating_add(1);
                self.emit(EventBody::DeferWaitElapsed {
                    data: events::DeferWaitElapsed {
                        waited,
                        round: defer_round,
                    },
                })?;
                continue;
            }
            // Guarded like the other branches: once the run has halted, no
            // answer can reach an attempt this session, so asking would spend
            // a human's attention on a decision the scheduler cannot act on —
            // and a decline would relabel `halted_at` with a task that was not
            // the cause. The questions stay open on disk for a resume (§15).
            if self.state.halted_at.is_none()
                && self.state.budget_stop.is_none()
                && self.resolve_one_question()?
            {
                continue;
            }
            break;
        }
        Ok(())
    }

    /// Stable order: among tasks whose dependencies are all done, lowest plan
    /// index first (§14). Parked, deferred, and blocked tasks are simply not
    /// ready — which is exactly the skip-ahead §14 asks for.
    fn next_ready(&self) -> Option<usize> {
        // A halt and a budget stop both end scheduling, for the same reason:
        // whatever runs next would be work the run has already decided not to
        // do. The remaining tasks settle as skipped exactly as they do after a
        // halt, and the questions already open stay open for a resume (§15).
        if self.state.halted_at.is_some() || self.state.budget_stop.is_some() {
            return None;
        }
        let tasks = &self.analysis.plan.tasks;
        (0..tasks.len()).find(|&i| {
            matches!(self.state.states[i], TaskState::Pending)
                && tasks[i].depends_on.iter().all(|dep| {
                    tasks
                        .iter()
                        .position(|t| t.id == *dep)
                        // An unknown dependency cannot exist on a validated
                        // plan; treating it as satisfied keeps the scheduler
                        // total rather than deadlocking.
                        .is_none_or(|j| matches!(self.state.states[j], TaskState::Done(_)))
                })
        })
    }

    /// Drive one task until it yields the scheduler: done, failed, deferred,
    /// or parked. Retries and escalations happen *inside* — a resumed retry
    /// keeps the working tree (§14), so no other task may run in between, and
    /// this loop is what guarantees that.
    ///
    /// Returns whether the task ended deferred.
    fn step_task(&mut self, index: usize) -> Result<bool, TactusError> {
        // Copied out of `self` so they carry the run's lifetime rather than
        // this method's `&mut self` borrow.
        let analysis = self.analysis;
        let adapters = self.adapters;
        let workspace = self.workspace;
        let task = &analysis.plan.tasks[index];
        let task_id = task.id.to_string();
        let chain = &analysis.chains[index];
        let policy = LadderPolicy {
            attempts_per: chain.attempts_per,
            rungs: chain.rungs.len(),
            max_defers: self.max_defers,
        };
        let stem = format!("{index:02}-{}", util::filename_component(task.id.as_str()));

        loop {
            let rung_index = self.state.progress[index].rung;
            let Some(rung) = chain.rungs.get(rung_index) else {
                self.fail_task(
                    index,
                    FailureKind::NoChain,
                    "resolved chain has no rung to run on".to_owned(),
                )?;
                return Ok(false);
            };
            // §13's ceiling, checked before EVERY spawn rather than once per
            // task. The placement is the whole point: an escalation onto a
            // frontier rung happens inside this loop, so a check that ran only
            // on the way in would let the most expensive attempt of the run be
            // the one that dodged the budget. It never influences *what* binds
            // — capacity-driven routing is v0.2 (§13) — only whether the next
            // attempt happens at all.
            if let Some(exceeded) = self.budget_breach(index) {
                // The ceiling is recorded first, and nothing below may take it
                // back. It is what `outcome()` reads to return `BudgetExceeded`
                // rather than a task failure, what turns into exit 3 for the CI
                // job gating on it, and what `resume --budget` needs to find in
                // order to have a stop to get past. Tidying up afterwards is a
                // courtesy; the record is the run's account of itself.
                self.emit(EventBody::BudgetExceeded { data: exceeded })?;
                // The tree may still hold a rejected attempt's edits, kept by
                // the ladder below for a resumed retry that is now never going
                // to run. Handing those back is the one thing §14 rules out —
                // they are unverified, and staged changes follow `git switch`
                // onto whatever branch the operator visits next. Nor can they
                // be saved for the resume: `run_resumed` discards every
                // uncommitted path and clears the session they belong to, so
                // keeping them past this point buys nothing at all.
                //
                // A git that cannot do it says so and the run still stops at
                // its ceiling, the way it did before the tidying existed. The
                // sibling discard on the error path below is `let _ =` for the
                // same reason; this one warns, because here there is a report
                // left to carry the warning.
                if let Err(error) = workspace.discard_uncommitted() {
                    self.warnings.push(format!(
                        "the budget stopped the run, but the working tree could not be cleaned: \
                         {error}"
                    ));
                }
                return Ok(false);
            }

            let profile = WorkerProfile {
                name: format!("{}-{}", rung.tier, rung.binding.model),
                agent: rung.binding.agent.clone(),
                model: rung.binding.model.clone(),
                // Attribution only (§13 read-only): which subscription pays for
                // this attempt, so the ledger and the estimator can say so.
                // Nothing routes on it.
                pool: self.pool_name_for(&rung.binding.agent).unwrap_or_default(),
                permissions: PermissionMode::Edit,
                // What the rung's tier is worth on an agent with an effort
                // axis: without this the whole chain runs at one vendor
                // default and escalating a rung moves nothing (§10).
                effort: Some(self.effort_policy.implementation_for(rung.tier)),
                max_turns: None,
                extra_args: Vec::new(),
            };
            let adapter = adapters
                .get(&profile.agent)
                .ok_or_else(|| TactusError::Agent {
                    message: format!("no adapter registered for agent `{}`", profile.agent),
                })?;

            let attempt = self.state.progress[index].attempts + 1;
            let resume = self.state.progress[index]
                .resume_next
                .then(|| self.state.progress[index].session.clone())
                .flatten();

            // Recorded *before* the agent is spawned, so a process that dies
            // mid-attempt leaves an `attempt_started` with no
            // `attempt_finished`. That dangling pair is precisely what tells a
            // later replay an attempt was interrupted (§19's crash row) — the
            // engine cannot write a record of its own death afterwards.
            let rung_number = u32::try_from(rung_index).unwrap_or(u32::MAX);
            self.emit(EventBody::AttemptStarted {
                task: task_id.clone(),
                attempt,
                rung: rung_number,
                profile: profile.name.clone(),
                data: events::AttemptStarted {
                    tier: rung.tier.to_string(),
                    agent: profile.agent.clone(),
                    model: profile.model.clone(),
                    adapter: Some(adapter.id().to_owned()),
                    preflight_cli_version: self
                        .caps
                        .get(&profile.agent)
                        .map(|caps| caps.version.clone()),
                    effort: profile.effort,
                    selection_origin: Some(if rung.binding.pinned {
                        events::SelectionOrigin::Pin
                    } else {
                        events::SelectionOrigin::Auto
                    }),
                    pool: pool_option(&profile.pool),
                    resume_session: resume.clone(),
                },
            })?;

            // Scoped so every borrow the attempt takes on `self` is released
            // before the ladder updates this task's progress below.
            let result = {
                let retry = (attempt > 1).then(|| RetryBrief {
                    resumed: resume.is_some(),
                    // Owned: the ladder appends to this task's feedback the
                    // moment the attempt returns, and one clone per attempt
                    // costs less than threading that borrow through.
                    feedback: self.state.progress[index].feedback.clone(),
                });
                let attempt_cx = AttemptCx {
                    task,
                    profile: profile.clone(),
                    adapter,
                    attempt,
                    stem: stem.clone(),
                    paths: &self.paths,
                    gates: &analysis.gates,
                    gate_cmds: &self.gate_cmds,
                    reviewers: self.reviewers(index, &profile)?,
                    timeout: self.attempt_timeout,
                    review_pass_timeout: self.review_pass_timeout,
                    retry,
                    // The same entries the worker prompt quotes as operator
                    // instruction, routed to the judge as well (§12).
                    decisions: self.state.progress[index]
                        .feedback
                        .iter()
                        .filter(|entry| entry.human)
                        .filter_map(|entry| entry.detail.clone())
                        .collect(),
                    #[cfg(test)]
                    after_candidate_capture: self.after_candidate_capture,
                };

                // Any error between the agent editing files and the verdict
                // leaves the tree dirty; the run cannot continue but must not
                // hand the user a half-staged workspace either (§14).
                match run_attempt(&attempt_cx, workspace, resume.clone()) {
                    Ok(result) => result,
                    Err(error) => {
                        let _ = workspace.discard_uncommitted();
                        return Err(error);
                    }
                }
            };

            // Decide the ladder transition before writing the settlement, then
            // carry both in one event. A failure record without its decision is
            // not a safe crash prefix: replay would otherwise buy another
            // attempt on the old rung or lose an outage refund.
            let next = result.failure.as_ref().map(|failure| {
                let settlement_session = result.outcome.session_id.as_ref().or(resume.as_ref());
                let resumable = settlement_session.is_some()
                    && self
                        .caps
                        .get(&profile.agent)
                        .is_some_and(|c| c.session_resume);
                ladder::next_step(
                    failure,
                    &LadderState {
                        rung: self.state.progress[index].rung,
                        attempts_on_rung: self.state.progress[index].attempts_on_rung,
                        defers: self.state.progress[index].defers,
                        resumable,
                    },
                    &policy,
                )
            });
            let mut transition = None;
            let mut parking = None;
            let mut parking_question = None;
            let pending_spend = result.outcome.cost_usd.unwrap_or(0.0)
                + result
                    .reviews
                    .iter()
                    .map(|review| review.cost_usd.unwrap_or(0.0))
                    .sum::<f64>();
            let pending_unpriced = result.outcome.cost_usd.is_none()
                || result
                    .reviews
                    .iter()
                    .any(|review| review.cost_usd.is_none());
            if let (Some(failure), Some(next)) = (result.failure.as_ref(), next) {
                match next {
                    Next::RetrySameRung { resume } => {
                        transition = Some(Box::new(events::AttemptTransition::Retry(
                            events::LadderRetry {
                                resume,
                                tier: rung.tier.to_string(),
                                summary: failure.reason.clone(),
                                detail: failure.feedback.clone(),
                            },
                        )));
                    }
                    Next::Escalate => {
                        transition = Some(Box::new(events::AttemptTransition::Escalate(
                            events::LadderEscalated {
                                to_rung: rung_number.saturating_add(1),
                                tier: rung.tier.to_string(),
                                summary: failure.reason.clone(),
                                detail: failure.feedback.clone(),
                            },
                        )));
                        if let Some(onto) = chain.rungs.get(rung_index + 1).map(|next| next.tier) {
                            if self.should_approve_spend(rung.tier, onto, pending_spend) {
                                let question = self.build_spend_approval(
                                    index,
                                    onto,
                                    pending_spend,
                                    pending_unpriced,
                                );
                                parking = Some(Box::new(events::AttemptParking {
                                    question: question.clone(),
                                    refund_attempt: false,
                                }));
                                parking_question = Some(question);
                            }
                        }
                    }
                    Next::Defer => {
                        transition = Some(Box::new(events::AttemptTransition::Defer(
                            events::TaskDeferred {
                                reason: failure.reason.clone(),
                                defers: self.state.progress[index].defers.saturating_add(1),
                            },
                        )));
                    }
                    Next::AskHuman(kind) => {
                        let context =
                            question_context(task, kind, failure, &self.state.progress[index]);
                        let question = self.build_question(index, kind, context);
                        parking = Some(Box::new(events::AttemptParking {
                            question: question.clone(),
                            // An outage or clarification never received a code
                            // verdict, so its allowance is returned even when
                            // the outage ceiling sends it to a human.
                            refund_attempt: kind == QuestionKind::Clarify || failure.is_outage(),
                        }));
                        parking_question = Some(question);
                    }
                    Next::Fail => {
                        transition = Some(Box::new(events::AttemptTransition::Fail(
                            events::TaskFailed {
                                kind: failure.kind,
                                reason: failure.reason.clone(),
                                halts_run: self.on_task_failure == OnTaskFailure::Halt,
                            },
                        )));
                    }
                }
            }

            // A passing attempt is turned into an immutable commit object and
            // pinned before its settlement becomes durable. The event, HEAD
            // CAS, and pin deletion can therefore be recovered at every crash
            // prefix without re-running paid work or trusting the mutable
            // index.
            let prepared_commit = if result.failure.is_none() {
                let message = format!("[tactus] {}: {}", task.id, task.title);
                let pin_ref = prepared_pin_ref(&self.run_id, index, attempt);
                let recorded_branch_ref = format!("refs/heads/{}", self.branch);
                if result.candidate_branch_ref != recorded_branch_ref {
                    let _ = self.workspace.discard_uncommitted();
                    return Err(TactusError::Git {
                        message: format!(
                            "candidate was captured from `{}`, not recorded run branch `{recorded_branch_ref}`; refusing publication",
                            result.candidate_branch_ref
                        ),
                    });
                }
                match self.workspace.prepare_commit_from_candidate(
                    &result.candidate_branch_ref,
                    &result.candidate_parent,
                    &result.candidate_tree,
                    &message,
                    &pin_ref,
                ) {
                    Ok(prepared) => Some(prepared),
                    Err(error) => {
                        let _ = self.workspace.discard_uncommitted();
                        return Err(error);
                    }
                }
            } else {
                None
            };

            let settlement = self.emit(EventBody::AttemptFinished {
                task: task_id.clone(),
                attempt,
                rung: rung_number,
                profile: profile.name.clone(),
                parking,
                transition,
                prepared_commit: prepared_commit.clone().map(Box::new),
                data: Box::new(AttemptRecord {
                    attempt,
                    tier: rung.tier.to_string(),
                    model: profile.model.clone(),
                    pool: pool_option(&profile.pool),
                    resumed: resume.is_some(),
                    duration: result.outcome.duration,
                    cost_usd: result.outcome.cost_usd,
                    reviews: result.reviews.clone(),
                    session_id: result.outcome.session_id.clone(),
                    usage: result.outcome.usage.clone(),
                    failure: result.failure.as_ref().map(|f| FailureRecord {
                        kind: f.kind,
                        origin: f.origin,
                        reason: f.reason.clone(),
                    }),
                }),
            });
            if let Err(error) = settlement {
                // A write/flush/sync error cannot prove whether the newline-
                // committed event reached disk. Deliberately retain a prepared
                // pin: resume removes it as an orphan if no settlement landed,
                // or publishes it if the complete settlement is readable.
                // Deleting it here would turn an ambiguous sync error into a
                // schema-3 settlement whose exact object is no longer durable.
                if let Err(cleanup) = self.workspace.discard_uncommitted() {
                    return Err(TactusError::Git {
                        message: format!(
                            "{error}; additionally failed to clean the unreviewed workspace: {cleanup}"
                        ),
                    });
                }
                return Err(error);
            }
            if let Some(question) = parking_question.as_ref() {
                if let Err(error) = self.materialize_question(question) {
                    // The durable settlement is authoritative and already carries
                    // the complete question. A crash or write failure here cannot
                    // expose an orphan projection; resume rematerializes the
                    // question from the event before accepting an answer.
                    if let Err(cleanup) = self.workspace.discard_uncommitted() {
                        return Err(TactusError::Git {
                            message: format!(
                                "{error}; additionally failed to clean the unreviewed workspace: {cleanup}"
                            ),
                        });
                    }
                    return Err(error);
                }
            }

            let Some(failure) = result.failure else {
                let prepared = prepared_commit
                    .expect("a successful schema-3 settlement has a prepared commit");
                self.workspace
                    .advance_prepared_commit(&result.candidate_branch_ref, &prepared)?;
                // Scrub gate side-effects (build artifacts, lockfile churn) so
                // they cannot leak into the next task's captured diff; the
                // commit recorded exactly the verified staged set.
                self.workspace.discard_uncommitted()?;
                self.emit(EventBody::TaskCommitted {
                    task: task_id.clone(),
                    data: events::TaskCommitted {
                        sha: prepared.commit_sha,
                        message: prepared.message,
                    },
                })?;
                return Ok(false);
            };

            // §13 source 1: a rate-limit signal is ground truth about a pool,
            // and the only thing in v0.1 that can call one empty rather than
            // unmeasured. Recorded separately from the deferral that follows
            // because they are facts with different lifetimes — the deferral is
            // about this task's next move, this is about a subscription, and a
            // later run's estimator reads it back out of the log.
            if failure.kind != FailureKind::Interrupted
                && !(failure.kind == FailureKind::RateLimited
                    && failure.origin == FailureOrigin::Worker)
            {
                // This attempt reached a model and got an answer, whatever the
                // verdict on its code, so any pool it drew on is serving again.
                // Same rule as `capacity::observe`'s, applied to the engine's
                // own view so the two cannot disagree about when a pool
                // recovered — without it, the *next* outage on the same pool
                // would go unrecorded because the set still held it.
                self.exhausted_pools.remove(&profile.pool);
            }
            for review in &result.reviews {
                if review.outcome != events::ReviewPassOutcome::Unavailable {
                    if let Some(pool) = &review.pool {
                        self.exhausted_pools.remove(pool);
                    }
                }
            }
            if failure.kind == FailureKind::RateLimited {
                self.record_pool_exhausted(&task_id, &profile, &result.reviews, &failure)?;
            }

            let next = next.expect("a failed attempt has a ladder decision");

            // §14: the tree survives only for a resumed retry, where the
            // *cumulative* diff is what gets re-gated. Every other branch
            // hands the scheduler a clean workspace, because another task may
            // run before this one does again.
            if !matches!(next, Next::RetrySameRung { resume: true }) {
                self.workspace.discard_uncommitted()?;
            }

            match next {
                Next::RetrySameRung { .. } => {}
                Next::Escalate => {
                    if parking_question.is_some() {
                        return Ok(false);
                    }
                }
                Next::Defer => return Ok(true),
                Next::AskHuman(_) | Next::Fail => return Ok(false),
            }
        }
    }

    /// §11.2/§11.3: the read-only passes that judge one task's attempt.
    ///
    /// Reviewers bind at the configured review tier (frontier by default)
    /// rather than the implementer's rung — a small model reviewing its own
    /// work is not verification — and [`ReviewPlan::passes_for`] decides
    /// whether that means one pass or two, and whether the primary rebinds
    /// away from the model that wrote the code.
    ///
    /// An empty list means review is switched off explicitly. A pass whose
    /// adapter cannot be built is a hard error: verification vanishing without
    /// a word is worse than a refusal, and pre-flight has already probed every
    /// agent named here.
    fn reviewers(
        &self,
        index: usize,
        implementer: &WorkerProfile,
    ) -> Result<Vec<Reviewer<'_>>, TactusError> {
        let running_on = PassBinding::new(implementer.agent.clone(), implementer.model.clone());
        self.review_plan
            .passes_for(index, &running_on)
            .into_iter()
            .map(|pass: ReviewPass| {
                // Every pass judges at the review tier's effort, including a
                // second opinion bound to another vendor: the standard belongs
                // to the review, not to whichever family happens to apply it.
                let mut profile = pass.profile(self.effort_policy.review);
                // A cross-vendor second opinion draws on a different
                // subscription than the implementer (§11.3, §13), so its pool
                // is looked up from its own agent rather than inherited.
                profile.pool = self.pool_name_for(&profile.agent).unwrap_or_default();
                Ok(Reviewer {
                    adapter: self.adapters.get(&pass.binding.agent).ok_or_else(|| {
                        TactusError::Agent {
                            message: format!(
                                "the {} pass binds to agent `{}`, which has no adapter in this \
                                 build",
                                pass.lens.name(),
                                pass.binding.agent
                            ),
                        }
                    })?,
                    profile,
                    lens: pass.lens,
                    preflight_cli_version: self
                        .caps
                        .get(&pass.binding.agent)
                        .map(|caps| caps.version.clone()),
                })
            })
            .collect()
    }

    /// §14's pre-flight capacity snapshot, from the state this run has folded
    /// so far.
    ///
    /// Deliberately does **not** probe. Everything a probe would add — auth
    /// state, versions — is already established by pre-flight, and spawning the
    /// vendors' CLIs a second time to fill in a metadata event would be work
    /// nothing reads. The estimator's inputs come from the run's own log, which
    /// on a fresh run is empty and on a resume carries every signal the earlier
    /// process recorded.
    fn emit_capacity_snapshot(
        &mut self,
        signals: &BTreeMap<String, Option<String>>,
    ) -> Result<(), TactusError> {
        // No early return on an empty pools file: "nothing was connected" is
        // exactly as worth recording as a list, and its absence is otherwise
        // indistinguishable from a pre-step-10 log, or from a binary that never
        // took a snapshot at all (§14).
        let pools = &self.analysis.config.pools;
        // Signals come from the caller's fold of this run's log (empty on a
        // fresh run) rather than from a field kept here, so there is exactly one
        // place that turns `pool_exhausted` events into observations — the same
        // reasoning that keeps `RunState::apply` the only writer of run state.
        let estimates = capacity::estimate(
            pools,
            &capacity::Observations {
                exhausted: signals.clone(),
                self_spend: capacity::drain_of(
                    self.state
                        .progress
                        .iter()
                        .flat_map(|progress| progress.records.iter()),
                ),
            },
        );
        let snapshot = events::CapacitySnapshot {
            strategy: self.analysis.config.strategy.mode.clone(),
            pools: estimates
                .iter()
                .map(|estimate| events::PoolSnapshot {
                    pool: estimate.pool.clone(),
                    agent: estimate.agent.clone(),
                    kind: estimate.kind.to_string(),
                    remaining: estimate.remaining.to_string(),
                    confidence: estimate.confidence.to_string(),
                    reset_at: estimate.reset_at.clone(),
                })
                .collect(),
        };
        self.emit(EventBody::CapacitySnapshot { data: snapshot })
    }

    /// Which pool an agent's attempts drain (§13), or `None` when the pools
    /// file names none for it. Attribution only — nothing routes on it.
    fn pool_name_for(&self, agent: &str) -> Option<String> {
        capacity::pool_for(agent, &self.analysis.config.pools).map(|pool| pool.name.clone())
    }

    /// §13's reported spend so far — the ledger's own figure, with the ledger's
    /// own honesty: unpriced attempts contribute nothing, so this is a floor
    /// wherever a route reports no spend at all.
    fn reported_spend(&self, task: Option<usize>) -> f64 {
        let indices: Vec<usize> = match task {
            Some(index) => vec![index],
            None => (0..self.state.progress.len()).collect(),
        };
        indices
            .into_iter()
            .filter_map(|index| self.state.progress.get(index))
            .flat_map(|progress| progress.records.iter())
            .map(|record| record.cost_usd.unwrap_or(0.0) + record.review_cost_usd().unwrap_or(0.0))
            .sum()
    }

    /// Whether a ceiling has been reached, and which one.
    ///
    /// `run_usd` is checked before `task_usd` because it is the stricter claim:
    /// a run at its overall ceiling is done whatever any individual task has
    /// spent, and naming the run budget is what tells the operator which number
    /// to raise.
    fn budget_breach(&self, index: usize) -> Option<events::BudgetExceeded> {
        let task = self.analysis.plan.tasks[index].id.to_string();
        if let Some(limit) = self.budgets.run_usd {
            let spent = self.reported_spend(None);
            if spent >= limit {
                return Some(events::BudgetExceeded {
                    budget: events::BudgetKind::Run,
                    limit_usd: limit,
                    spent_usd: spent,
                    task,
                });
            }
        }
        if let Some(limit) = self.budgets.task_usd {
            let spent = self.reported_spend(Some(index));
            if spent >= limit {
                return Some(events::BudgetExceeded {
                    budget: events::BudgetKind::Task,
                    limit_usd: limit,
                    spent_usd: spent,
                    task,
                });
            }
        }
        None
    }

    /// §12's `ask_before`: does this escalation need a person's approval first?
    ///
    /// Only a move *onto* a frontier rung from somewhere cheaper counts. A
    /// chain that starts at frontier is where the operator deliberately routed
    /// the task in config or in an annotation, and §12's concern is silent
    /// escalation — asking permission for a decision the operator already made
    /// in writing would train them to answer without reading.
    fn should_approve_spend(
        &self,
        from: crate::ir::Tier,
        onto: crate::ir::Tier,
        pending_spend: f64,
    ) -> bool {
        let Some(threshold) = self.ask_before.frontier_escalation_over_usd else {
            return false;
        };
        onto == crate::ir::Tier::Frontier
            && from != crate::ir::Tier::Frontier
            && self.reported_spend(None) + pending_spend >= threshold
    }

    /// §13 source 1, recorded: attribute a rate limit to the pool that hit it.
    ///
    /// A reviewer's rate limit belongs to the *reviewer's* pool, which on a
    /// cross-vendor second opinion is a different subscription from the one the
    /// implementer drained — attributing it to the implementer's would mark a
    /// healthy pool exhausted and leave the empty one looking fine.
    fn record_pool_exhausted(
        &mut self,
        task: &str,
        implementer: &WorkerProfile,
        reviews: &[events::ReviewRecord],
        failure: &AttemptFailure,
    ) -> Result<(), TactusError> {
        let (pool, agent) = match failure.origin {
            FailureOrigin::Reviewer => match reviews.last() {
                Some(review) => (review.pool.clone(), review.agent.clone()),
                None => return Ok(()),
            },
            FailureOrigin::Worker => (pool_option(&implementer.pool), implementer.agent.clone()),
        };
        // No pool named for that agent means no subscription to mark. The
        // signal is still in the log on the attempt record; inventing a pool id
        // to hang it on would put a fact about nothing into the estimator.
        let Some(pool) = pool else { return Ok(()) };
        // Only the transition (see `exhausted_pools`).
        if !self.exhausted_pools.insert(pool.clone()) {
            return Ok(());
        }
        self.emit(EventBody::PoolExhausted {
            task: task.to_owned(),
            data: events::PoolExhausted {
                pool,
                agent,
                // §13 wants a retry-at-reset timer here. Neither CLI reports a
                // machine-readable reset time today, and parsing one out of
                // prose would be a guess dressed as a timestamp — so it stays
                // `None`, `DEFAULT_MAX_DEFERS` stays the bound, and the estimate
                // says the reset is unknown.
                reset_at: None,
                detail: util::head(&failure.reason, 400),
            },
        })
    }

    fn fail_task(
        &mut self,
        index: usize,
        kind: FailureKind,
        reason: String,
    ) -> Result<(), TactusError> {
        // The halt policy is resolved here and recorded, not re-derived on
        // replay: a `tactus.toml` edited between a run and its resume must not
        // rewrite which task the report blames for stopping.
        let halts_run = self.on_task_failure == OnTaskFailure::Halt;
        self.fail_task_with_policy(index, kind, reason, halts_run)
    }

    fn fail_task_with_policy(
        &mut self,
        index: usize,
        kind: FailureKind,
        reason: String,
        halts_run: bool,
    ) -> Result<(), TactusError> {
        let task = self.analysis.plan.tasks[index].id.to_string();
        self.emit(EventBody::TaskFailed {
            task,
            data: events::TaskFailed {
                kind,
                reason,
                halts_run,
            },
        })
    }

    /// §12: raise eagerly, park exactly the affected task, tell the notifiers,
    /// and write the payload where a UI or `tactus answer` can read it.
    /// §12's `ask_before` question: this task is about to escalate onto a
    /// frontier rung, and the run has already reported enough spend that the
    /// operator asked to be consulted first.
    fn build_spend_approval(
        &self,
        index: usize,
        onto: crate::ir::Tier,
        pending_spend: f64,
        pending_unpriced: bool,
    ) -> Question {
        let context = spend_question_context(
            &self.analysis.plan.tasks[index],
            onto,
            self.reported_spend(None) + pending_spend,
            self.ask_before.frontier_escalation_over_usd.unwrap_or(0.0),
            self.unpriced_attempts() > 0 || pending_unpriced,
        );
        self.build_question(index, QuestionKind::ApproveSpend, context)
    }

    /// Attempts whose route reported no spend at all (§13), so the figures this
    /// run quotes are floors rather than totals.
    fn unpriced_attempts(&self) -> u32 {
        let unpriced = self
            .state
            .progress
            .iter()
            .flat_map(|progress| progress.records.iter())
            .filter(|record| record.cost_usd.is_none() || record.review_cost_incomplete())
            .count();
        u32::try_from(unpriced).unwrap_or(u32::MAX)
    }

    fn build_question(&self, index: usize, kind: QuestionKind, context: String) -> Question {
        let task = &self.analysis.plan.tasks[index];
        Question {
            id: interaction::new_question_id(),
            kind,
            // v0.1 parks only the task that raised it. Dependents are held by
            // the graph, not by the question, so they stay eligible the moment
            // an answer arrives.
            affected_tasks: vec![task.id.clone()],
            context,
            options: question_options(kind),
        }
    }

    fn materialize_question(&mut self, question: &Question) -> Result<(), TactusError> {
        // Materialize before notifying: a recipient must always be able to open
        // the payload it was told about. The caller decides whether the
        // authoritative event belongs before (atomic settlement parking) or
        // after (ordinary question flow) this projection.
        interaction::write_question(
            &self.paths.questions(),
            &QuestionRecord::open(question.clone()),
        )?;
        let id = question.id.clone();
        for notifier in &self.notifiers {
            // A notifier that cannot deliver must not take the run with it: the
            // question is already on disk either way (§12).
            if let Err(error) = notifier.ask(question) {
                self.warnings.push(format!(
                    "notifier `{}` could not deliver question {id}: {error}",
                    notifier.id()
                ));
            }
        }
        Ok(())
    }

    /// Ingest answers left by `tactus answer` in another process.
    ///
    /// Returns whether anything changed. This is what makes the answer command
    /// useful while a run is alive rather than only between runs: an operator
    /// answering from a phone at 2am un-parks the task on the next scheduler
    /// turn, with no resume needed.
    fn sweep_answers(&mut self) -> Result<bool, TactusError> {
        let open: Vec<QuestionId> = self
            .state
            .open_questions()
            .iter()
            .map(|record| record.question.id.clone())
            .collect();
        if open.is_empty() {
            return Ok(false);
        }
        let dir = self.paths.answers();
        let mut changed = false;
        for id in open {
            let Some(answer) = interaction::read_answer(&dir, &id)? else {
                continue;
            };
            // Only what actually applied counts as change. A file the engine
            // reads but declines to act on — an `Unanswered` one, say, which
            // nothing in `tactus answer` will write but a hand-edit can —
            // would otherwise report progress on every turn, and the drain
            // loop would spin on it forever: this branch is only bounded
            // because it closes the question it fires for.
            if self.ingest_answer(&id, answer, "answer-file")? {
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Record an answer and let it take effect. Returns whether it applied.
    ///
    /// One path for every channel — a terminal reply, a file written by
    /// `tactus answer`, or an answer picked up on resume — so what an answer
    /// *does* cannot depend on where it came from. The guards below are also
    /// what makes it safe to offer the same answer twice: a question that is
    /// already closed absorbs the second one instead of applying it.
    fn ingest_answer(
        &mut self,
        id: &QuestionId,
        answer: Answer,
        via: &str,
    ) -> Result<bool, TactusError> {
        let Some(record) = self
            .state
            .questions
            .iter()
            .find(|record| record.question.id == *id)
        else {
            return Ok(false);
        };
        if !record.is_open() || answer == Answer::Unanswered {
            return Ok(false);
        }
        let context = record.question.context.clone();
        let affected = record.question.affected_tasks.clone();

        self.emit(EventBody::QuestionAnswered {
            data: events::QuestionAnswered {
                question: id.clone(),
                answer: answer.clone(),
                decline_halts_run: (answer == Answer::Declined)
                    .then_some(self.on_task_failure == OnTaskFailure::Halt),
                via: via.to_owned(),
            },
        })?;

        // §5: a question that reached a human at runtime is, by definition, a
        // design-phase defect — logged as one so the accumulated defects can
        // become review material for the designer prompt.
        self.emit(EventBody::DesignDefect {
            data: events::DesignDefect {
                question: id.clone(),
                context: util::head(context.trim(), 600),
                answer: match &answer {
                    Answer::Answered { text } => text.clone(),
                    _ => "declined".to_owned(),
                },
            },
        })?;

        // A decline is the task's failure, not the question's, so it goes
        // through the one place that owns the halt policy. `apply` leaves a
        // declined task parked precisely so this can still see who was waiting.
        if answer == Answer::Declined {
            for task_id in affected {
                let Some(index) = self.state.index_of(task_id.as_str()) else {
                    continue;
                };
                if !matches!(&self.state.states[index], TaskState::AwaitingInput(q) if q == id) {
                    continue;
                }
                let reason = format!(
                    "declined at the human rung: {}",
                    last_reason(&self.state.progress[index])
                );
                self.fail_task(index, FailureKind::Declined, reason)?;
            }
        }

        // Rewrite the payload so a late reader — a UI, or someone opening the
        // directory tomorrow — sees the whole exchange, not just the question.
        if let Some(record) = self
            .state
            .questions
            .iter()
            .find(|record| record.question.id == *id)
        {
            interaction::write_question(&self.paths.questions(), record)?;
        }
        Ok(true)
    }

    /// Ask about the oldest open question. Returns whether anything changed.
    ///
    /// This runs only at a hard block, and each question is asked at most
    /// once: an `Unanswered` result marks it unreachable rather than looping
    /// back to a channel that already said nobody is there.
    fn resolve_one_question(&mut self) -> Result<bool, TactusError> {
        let Some(position) = self.state.questions.iter().position(|record| {
            record.is_open() && !self.unanswerable.contains(&record.question.id)
        }) else {
            return Ok(false);
        };
        let question = self.state.questions[position].question.clone();
        let answer = self.answers.resolve(&question)?;

        // The channel may have been waiting on the very file the sweep reads,
        // so sweep before applying what it returned — and then still apply it.
        // `ingest_answer` is guarded on the question being open, which is what
        // makes doing both safe: if the sweep answered *this* question the
        // typed reply is absorbed, and if it answered a different one — an
        // operator working through a backlog of parked tasks — this reply
        // still lands instead of being discarded along with it.
        self.sweep_answers()?;
        if answer == Answer::Unanswered {
            // §12 CI mode: the task stays parked and the run's exit status
            // reports it. Not a failure — nobody rejected anything.
            self.unanswerable.push(question.id);
            return Ok(true);
        }
        self.ingest_answer(&question.id, answer, self.answers.id())?;
        Ok(true)
    }

    /// Settle every task that never ran, then report.
    fn finish(&self) -> RunReport {
        build_report(
            ReportHeader {
                run_id: &self.run_id,
                branch: &self.branch,
                gates: self.analysis.gates.iter().map(|g| g.name.clone()).collect(),
                gates_from_config: self.analysis.gates_from_config,
                warnings: self.warnings.clone(),
                // The engine only reports on itself once it has stopped.
                running: false,
                // A `finish` that runs is by definition not an interruption:
                // the shape this flag describes is the one left behind when
                // this function never got the chance.
                interrupted: false,
            },
            &self.analysis.plan,
            &self.state,
        )
    }
}

impl RunReport {
    /// Build a report from a replayed log.
    ///
    /// `status` and the `report.json` a run writes go through the same
    /// function, so what an operator sees mid-run and what the file says
    /// afterwards cannot drift into disagreeing.
    pub fn from_state(
        started: &events::RunStarted,
        plan: &Plan,
        state: &RunState,
        warnings: Vec<String>,
        running: bool,
        interrupted: bool,
    ) -> Self {
        build_report(
            ReportHeader {
                run_id: &started.run_id,
                branch: &started.branch,
                gates: started.gates.clone(),
                gates_from_config: started.gates_from_config,
                warnings,
                running,
                interrupted,
            },
            plan,
            state,
        )
    }
}

/// Everything a report needs that is not the plan or the state, kept together
/// so `build_report` stays readable at its call sites.
struct ReportHeader<'a> {
    run_id: &'a str,
    branch: &'a str,
    gates: Vec<String>,
    gates_from_config: bool,
    warnings: Vec<String>,
    /// Whether an engine is driving this run right now.
    running: bool,
    /// Whether this run stopped without ever recording that it finished.
    interrupted: bool,
}

fn build_report(header: ReportHeader<'_>, plan: &Plan, state: &RunState) -> RunReport {
    let ReportHeader {
        run_id,
        branch,
        gates,
        gates_from_config,
        warnings,
        running,
        interrupted,
    } = header;
    let settled = settle(plan, &state.states, running);
    let tasks: Vec<TaskReport> = state
        .order
        .iter()
        .copied()
        // Tasks that never started append in plan order, so the report reads
        // as the run happened and still accounts for everything.
        .chain((0..plan.tasks.len()).filter(|i| !state.order.contains(i)))
        .map(|index| {
            task_report(
                &plan.tasks[index],
                &settled[index],
                &state.progress[index],
                running,
            )
        })
        .collect();
    let total_cost_usd = total_of(&tasks);
    // §13's second currency: what each subscription drained, folded from the
    // same attempt records the dollar column comes from — so the two halves of
    // the ledger cannot disagree about the same attempt.
    let pool_drain = capacity::drain_of(state.progress.iter().flat_map(|p| p.records.iter()))
        .into_iter()
        .map(|(pool, spend)| PoolDrainRow {
            pool,
            attempts: spend.attempts,
            cost_usd: spend.usd,
            unpriced: spend.unpriced,
        })
        .collect();
    RunReport {
        run_id: run_id.to_owned(),
        branch: branch.to_owned(),
        gates,
        gates_from_config,
        warnings,
        tasks,
        halted_at: state.halted_at.clone(),
        questions: state.questions.clone(),
        budget_stop: state.budget_stop.clone(),
        total_cost_usd,
        pool_drain,
        running,
        interrupted,
    }
}

/// Derive how an ended run's untouched tasks are reported.
///
/// This is a *view*, not state, and deliberately not recorded as events. A
/// task blocked behind an unanswered question has to become runnable again the
/// moment that question is answered — so if `Blocked` were folded in from the
/// log, every resume would have to un-fold it. Deriving it fresh from whatever
/// the log says is true right now means there is nothing to undo.
fn settle(plan: &Plan, states: &[TaskState], running: bool) -> Vec<TaskState> {
    let tasks = &plan.tasks;
    let mut settled = states.to_vec();
    // Blocking propagates: a dependent of a blocked task is blocked too.
    // Repeat until stable rather than assuming plan order carries it — a plan
    // may list a dependent before the task it waits on.
    loop {
        let mut changed = false;
        for index in 0..tasks.len() {
            if settled[index] != TaskState::Pending {
                continue;
            }
            let blocker = tasks[index].depends_on.iter().find(|dep| {
                tasks
                    .iter()
                    .position(|t| t.id == **dep)
                    .is_some_and(|j| blocks_dependents(&settled[j], running))
            });
            if let Some(blocker) = blocker {
                settled[index] = TaskState::Blocked(blocker.to_string());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Whatever is still Pending was never reached: the run halted. A run that
    // is still going has not halted — those tasks are queued, or one of them
    // is working right now — so leave them Pending for `task_report` to tell
    // apart.
    if !running {
        for state in &mut settled {
            if *state == TaskState::Pending {
                *state = TaskState::Skipped;
            }
        }
    }
    settled
}

/// Whether a dependency in this state will keep its dependents from ever
/// running.
///
/// `Blocked` means one thing to an operator — "a dependency failed, or parked
/// and was never answered" — and that is a claim about the future, not the
/// present. On an ended run the two coincide: anything short of `Done` is
/// final, because nothing more is coming. On a live one they do not. A
/// dependency that is merely pending, deferred, or in flight is a task whose
/// turn has not come, and its dependent is *queued behind* it rather than
/// blocked by it. Deciding this from `Done`-ness alone made `Queued`
/// unreachable for every task with a dependency, so the entire first half of a
/// live run read as a graph of failures.
fn blocks_dependents(state: &TaskState, running: bool) -> bool {
    match state {
        TaskState::Done(_) => false,
        // Still on the way. Only an ended run turns that into "never".
        TaskState::Pending | TaskState::Deferred => !running,
        // Terminal even mid-run, which is what keeps the propagation working
        // while the engine is still going: a parked dependency really does
        // block its dependents until somebody answers.
        TaskState::AwaitingInput(_)
        | TaskState::Failed { .. }
        | TaskState::Blocked(_)
        | TaskState::Skipped => true,
    }
}

/// Why a task is parked or failed, for the report.
///
/// The most recent *attempt failure* wins, not the most recent feedback entry:
/// the branches that park a task never record feedback, so once an operator has
/// answered anything, their answer would otherwise shadow every later failure
/// and the report would tell them a task is parked because they answered a
/// question. Human entries are excluded from the fallback for the same reason.
fn last_reason(progress: &Progress) -> String {
    progress
        .records
        .last()
        .and_then(|r| r.failure.as_ref())
        .map(|f| f.reason.clone())
        .or_else(|| {
            progress
                .feedback
                .iter()
                .rev()
                .find(|f| !f.human)
                .map(|f| f.summary.clone())
        })
        .unwrap_or_else(|| "no attempt on record".to_owned())
}

fn task_report(task: &Task, state: &TaskState, progress: &Progress, running: bool) -> TaskReport {
    let records = &progress.records;
    let last = records.last();
    TaskReport {
        id: task.id.to_string(),
        title: task.title.clone(),
        model: last.map(|r| r.model.clone()).unwrap_or_default(),
        status: match state {
            TaskState::Done(sha) => TaskRunStatus::Committed { sha: sha.clone() },
            TaskState::Failed { kind, reason } => TaskRunStatus::Failed {
                kind: *kind,
                reason: reason.clone(),
            },
            TaskState::AwaitingInput(question) => TaskRunStatus::Parked {
                question: question.to_string(),
                reason: last_reason(progress),
            },
            TaskState::Blocked(by) => TaskRunStatus::Blocked { by: by.clone() },
            // On an ended run, Deferred cannot survive `finish` and Pending is
            // settled away, so both mean the run stopped before this task got
            // its turn. On a live one `settle` leaves them alone, and the
            // attempt record says which of the two it is.
            //
            // Every arm here is about a run that is still going, which is why
            // both are guarded. `Running` says of itself that only a live
            // `status` produces it, and a dangling `in_flight` on an ended run
            // is not a counter-example — it is an attempt whose engine died
            // between `attempt_started` and `attempt_finished`, which any error
            // out of `run_attempt` leaves behind. `finish` then wrote it into
            // `report.json` as `t1: running now — attempt 2 on mid` beside a
            // top-level `"running": false`: a stored document contradicting
            // itself, outliving the process that wrote it.
            TaskState::Deferred | TaskState::Pending => match &progress.in_flight {
                Some(flight) if running => TaskRunStatus::Running {
                    attempt: flight.attempt,
                    tier: flight.tier.clone(),
                    model: flight.model.clone(),
                },
                None if running => TaskRunStatus::Queued,
                _ => TaskRunStatus::Skipped,
            },
            TaskState::Skipped => TaskRunStatus::Skipped,
        },
        duration: records.iter().map(|r| r.duration).sum(),
        cost_usd: sum_opt(records.iter().map(|r| r.cost_usd)),
        review_models: {
            // Deduped, first-seen order: an escalated task can be judged by one
            // model on its first rung and another on the next, and both belong
            // beside a cost that counts both.
            let mut seen: Vec<String> = Vec::new();
            for model in records.iter().flat_map(AttemptRecord::review_models) {
                if !seen.contains(&model) {
                    seen.push(model);
                }
            }
            seen
        },
        review_cost_usd: sum_opt(records.iter().map(AttemptRecord::review_cost_usd)),
        review_cost_incomplete: records.iter().any(AttemptRecord::review_cost_incomplete),
        session_id: last.and_then(|r| r.session_id.clone()),
        attempts: records.clone(),
    }
}

/// What every task cost, added up.
///
/// Deliberately not `Iterator::sum`, which folds floats from `-0.0`. That is
/// the *correct* additive identity in IEEE 754 — `-0.0 + x` preserves the sign
/// of `x` where `0.0 + x` does not — but it means the sum of no costs at all is
/// negative zero, and a run that has not yet spent anything rendered its ledger
/// as `total: $-0.0000`. Folding from `+0.0` cannot change a non-empty sum,
/// because the only value `+0.0` fails to preserve is `-0.0`, and a cost is
/// never that.
fn total_of(tasks: &[TaskReport]) -> f64 {
    tasks
        .iter()
        .filter_map(TaskReport::total_cost_usd)
        .fold(0.0, |total, cost| total + cost)
}

/// Sum, preserving "nothing was reported" as `None` rather than `0.0` — a
/// ledger that cannot tell free from unreported is worse than no ledger.
fn sum_opt(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut total: Option<f64> = None;
    for value in values.flatten() {
        total = Some(total.unwrap_or(0.0) + value);
    }
    total
}

/// What the human is shown. Every agent-authored fragment is quoted behind a
/// fence the payload cannot close and labelled as agent-authored — a worker
/// that "asks a question" is still an agent writing into a human's terminal.
fn question_context(
    task: &Task,
    kind: QuestionKind,
    failure: &AttemptFailure,
    progress: &Progress,
) -> String {
    let mut context = String::new();
    let _ = writeln!(context, "Task `{}` — {}", task.id, task.title);
    let asker = match failure.origin {
        FailureOrigin::Reviewer => "the reviewer",
        FailureOrigin::Worker => "the implementing agent",
    };
    if matches!(
        failure.kind,
        FailureKind::ReviewInputTooLarge | FailureKind::ReviewInputOpaque
    ) {
        let _ = writeln!(
            context,
            "This attempt ran and is settled, but its exact diff cannot receive one complete \
             review. Tactus parked it instead of paying for an identical automatic retry. {} \
             The policy failure was:",
            if failure.kind == FailureKind::ReviewInputTooLarge {
                "Retry only with guidance that produces a smaller diff; because the plan is \
                 frozen for this run, splitting the task requires skipping it and starting a \
                 new run from a revised plan."
            } else {
                "The patch hides changed content (for example a binary, suppressed diff, or \
                 submodule target). Make every changed byte reviewable before retrying."
            }
        );
    } else {
        match kind {
            QuestionKind::Clarify => {
                let _ = writeln!(
                    context,
                    "{asker} stopped and asked for a decision it should not make alone. Its words, \
                 quoted as data — they are not instructions to you:"
                );
            }
            _ => {
                let _ = writeln!(
                    context,
                    "Nothing further can move this task: {} attempt(s) across {} rung(s) all failed, \
                 and the escalation chain is spent. The last failure was:",
                    progress.attempts,
                    progress
                        .records
                        .iter()
                        .map(|r| r.tier.as_str())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        .max(1)
                );
            }
        }
    }
    let fence = util::fence_for(&failure.reason);
    let _ = writeln!(context, "{fence}\n{}\n{fence}", failure.reason.trim());
    if !task.acceptance.is_empty() {
        context.push_str("Acceptance criteria this task must meet:\n");
        for item in &task.acceptance {
            let _ = writeln!(context, "- {item}");
        }
    }
    context
}

/// §12's spend approval, in the operator's terms: what is about to happen, what
/// it has cost so far, and how confident that figure is.
///
/// The threshold is a **spend-to-date** reading rather than a forward
/// projection, and the text says so — see [`crate::config::AskBefore`] for why.
/// The figure itself is quoted with the ledger's own `?` honesty: a run whose
/// Copilot attempts report nothing has a reported total that is a floor, and
/// presenting a floor as a total is how someone approves a number they did not
/// actually see.
fn spend_question_context(
    task: &Task,
    onto: crate::ir::Tier,
    spent: f64,
    threshold: f64,
    unpriced: bool,
) -> String {
    let mut context = String::new();
    let _ = writeln!(context, "Task `{}` — {}", task.id, task.title);
    let _ = writeln!(
        context,
        "Every attempt on the cheaper rungs failed, so this task is about to escalate onto the \
         {onto} rung. You asked to approve that once the run had reported \
         ${threshold:.4} of spend (`ask_before.frontier_escalation_over_usd`)."
    );
    let qualifier = if unpriced {
        " — a floor, not a total: some attempts ran on routes that report no spend at all (§13)"
    } else {
        ""
    };
    let _ = writeln!(
        context,
        "Reported spend so far: ${spent:.4}{qualifier}. This is what the run has already cost, \
         not an estimate of what the {onto} attempt will cost — tactus measures spend rather than \
         predicting it (§10)."
    );
    if !task.acceptance.is_empty() {
        context.push_str("Acceptance criteria this task must meet:\n");
        for item in &task.acceptance {
            let _ = writeln!(context, "- {item}");
        }
    }
    context
}

fn question_options(kind: QuestionKind) -> Vec<String> {
    match kind {
        QuestionKind::Clarify => {
            vec!["answer in your own words (typed free text is sent back to the agent)".to_owned()]
        }
        QuestionKind::ApproveSpend => vec![
            "approve: run the escalated attempt".to_owned(),
            "decline (`skip`) — this task fails and its dependents are blocked".to_owned(),
        ],
        _ => vec![
            "retry this task with guidance you type below".to_owned(),
            "give up on this task (`skip`) — its dependents will be blocked".to_owned(),
        ],
    }
}

/// A `WorkerProfile.pool` as the log records it: `None` rather than `""` when
/// no pool is configured, so a reader can tell "no pools file" from "a pool
/// whose name is empty" — and so a fold never attributes spend to `""`.
fn pool_option(pool: &str) -> Option<String> {
    (!pool.is_empty()).then(|| pool.to_owned())
}

/// Everything one attempt needs, so the ladder can loop over (rung, attempt)
/// without re-deriving any of it.
struct AttemptCx<'a> {
    task: &'a Task,
    profile: WorkerProfile,
    adapter: &'a dyn AgentAdapter,
    attempt: u32,
    /// Collision-free file stem for this task's run artifacts.
    stem: String,
    paths: &'a RunPaths,
    gates: &'a [ShellGate],
    gate_cmds: &'a [String],
    /// The ordered review passes for this task (§11.3). Empty only when review
    /// is switched off explicitly.
    reviewers: Vec<Reviewer<'a>>,
    timeout: Duration,
    /// Independent allowance for every reviewer in `reviewers`; one pass may
    /// use it across its initial verdict and one format-only re-ask.
    review_pass_timeout: Duration,
    /// `None` on the first attempt.
    retry: Option<RetryBrief>,
    /// Answers the operator has given about this task (§12), in the order they
    /// arrived. The worker gets these as instructions; so must the judge.
    decisions: Vec<String>,
    #[cfg(test)]
    after_candidate_capture: Option<AfterCandidateCapture>,
}

/// What the retry prompt needs to know (§11.4).
struct RetryBrief {
    /// The session carries the earlier conversation, so the prompt is terse.
    resumed: bool,
    /// Every failure so far, oldest first.
    feedback: Vec<Feedback>,
}

/// One read-only worker judging an attempt (§11.2). The list is empty only
/// when the user explicitly set `review = { enabled = false }`; a pass that
/// cannot be resolved is a hard error, never a silent downgrade.
#[derive(Clone)]
struct Reviewer<'a> {
    adapter: &'a dyn AgentAdapter,
    profile: WorkerProfile,
    lens: review::Lens,
    preflight_cli_version: Option<String>,
}

struct AttemptResult {
    outcome: Outcome,
    failure: Option<AttemptFailure>,
    /// Immutable git identities captured with the diff before any gate or
    /// reviewer ran. A successful commit is prepared from these exact objects.
    candidate_branch_ref: String,
    candidate_parent: String,
    candidate_tree: String,
    /// The passes that actually ran, in order — empty when the cheap checks
    /// failed first and no review happened. Derived from the reviews having
    /// happened rather than from passes being configured, so the ledger never
    /// credits a model with work it did not do (§13).
    reviews: Vec<events::ReviewRecord>,
}

/// Run one attempt and verify it, without deciding what happens next: the
/// caller owns commit, rollback, retry, and escalation (§11/§14).
fn run_attempt(
    cx: &AttemptCx<'_>,
    workspace: &Workspace,
    resume_session: Option<String>,
) -> Result<AttemptResult, TactusError> {
    let settings_path = cx.adapter.materialize_permissions(
        &cx.profile,
        cx.gate_cmds,
        &cx.paths.settings(),
        &format!("{}-{}", cx.stem, cx.attempt),
    )?;

    let task_run = TaskRun {
        prompt: materialize_prompt(
            cx.task,
            cx.gate_cmds,
            &cx.paths.artifacts(),
            cx.retry.as_ref(),
        ),
        profile: cx.profile.clone(),
        workspace: workspace.root().to_path_buf(),
        gate_cmds: cx.gate_cmds.to_vec(),
        resume_session,
        settings_path,
    };
    let command = cx.adapter.build(&task_run)?;
    let output = proc::run_with_timeout(command, cx.adapter.stdin_payload(&task_run), cx.timeout)?;

    let transcripts = cx.paths.transcripts();
    let transcript_path = transcripts.join(format!("{}-{}.json", cx.stem, cx.attempt));
    util::write_text(&transcript_path, &output.stdout)?;
    if !output.stderr.trim().is_empty() {
        util::write_text(
            &transcripts.join(format!("{}-{}.stderr.log", cx.stem, cx.attempt)),
            &output.stderr,
        )?;
    }

    let mut outcome: Outcome = cx.adapter.parse(&output)?;
    let candidate = workspace.capture_candidate()?;
    #[cfg(test)]
    if let Some(after_capture) = cx.after_candidate_capture {
        after_capture(workspace, &candidate)?;
    }
    outcome.diff = candidate.diff;
    outcome.transcript_path = transcript_path;

    // Verification ladder (§11): outcome sanity → cheap static provenance →
    // gates → review. Cheapest and most objective first.
    let mut failure = evaluate_outcome(&outcome, &output);
    if failure.is_none() {
        if let Some(error) = review::complete_diff_error(&outcome.diff) {
            if matches!(error, review::CompleteDiffError::Opaque) || !cx.reviewers.is_empty() {
                let kind = match error {
                    review::CompleteDiffError::Opaque => FailureKind::ReviewInputOpaque,
                    review::CompleteDiffError::TooLarge { .. } => FailureKind::ReviewInputTooLarge,
                };
                failure = Some(AttemptFailure::new(kind, error.to_string()).from_reviewer());
            }
        }
    }
    if failure.is_none() && cx.task.kind == TaskKind::Test && !gates::diff_adds_tests(&outcome.diff)
    {
        failure = Some(
            AttemptFailure::new(
                FailureKind::TestProvenance,
                "test provenance: this Test task adds no test code — a Test task that changes no \
                 tests proves nothing",
            )
            .with_feedback(
                "The diff contains no test code. Add tests that would fail without your change."
                    .to_owned(),
            ),
        );
    }
    if failure.is_none() {
        if let Some(problem) = workspace.review_input_problem_for_tree(&candidate.tree_oid)? {
            failure =
                Some(AttemptFailure::new(FailureKind::ReviewInputOpaque, problem).from_reviewer());
        }
    }
    if failure.is_none() && !cx.gates.is_empty() {
        let gate_workspace = workspace.gate_snapshot_for_candidate_in_store(
            &candidate.parent_oid,
            &candidate.tree_oid,
            &cx.paths.gate_worktrees(),
        )?;
        if let Some(gate_failure) = gates::run_all(
            cx.gates,
            gate_workspace.workspace(),
            &cx.paths.gates(),
            &cx.stem,
            cx.attempt,
        )? {
            failure = Some(
                AttemptFailure::new(
                    FailureKind::GateFailed,
                    format!(
                        "gate `{}` failed: {}",
                        gate_failure.gate, gate_failure.summary
                    ),
                )
                .with_feedback(gate_failure.log_tail),
            );
        }
    }

    // §11.2: gates are objective but shallow — a strong reviewer judges the
    // diff against the acceptance criteria only once the cheap checks pass.
    // §11.3: on blast-radius paths a second reviewer from another model family
    // judges the same diff, and both must pass.
    //
    // Passes short-circuit, like gates do (§11.1): once one has said no, a
    // second opinion on the same diff changes nothing about what happens next
    // and costs another frontier invocation to learn it.
    let mut reviews = Vec::new();
    if failure.is_none() && !cx.reviewers.is_empty() {
        let artifacts = load_artifacts(&cx.paths.artifacts(), cx.task);
        // Like gates, reviewers may inspect repository context beyond the
        // supplied diff. Give them the exact staged candidate, never ignored
        // worker inputs or residue from the authoritative workspace.
        let review_workspace = workspace.gate_snapshot_for_candidate_in_store(
            &candidate.parent_oid,
            &candidate.tree_oid,
            &cx.paths.gate_worktrees(),
        )?;
        for reviewer in &cx.reviewers {
            let review = review::run_review(&review::ReviewCx {
                adapter: reviewer.adapter,
                profile: reviewer.profile.clone(),
                lens: reviewer.lens,
                task: cx.task,
                diff: &outcome.diff,
                artifacts: &artifacts,
                decisions: &cx.decisions,
                workspace: review_workspace.workspace().root(),
                settings_dir: &cx.paths.settings(),
                reviews_dir: &cx.paths.reviews(),
                stem: format!("{}-{}", cx.stem, cx.attempt),
                timeout: cx.review_pass_timeout,
            })?;
            let cost_usd = review.cost_usd;
            // Read before the result is consumed: a judge that never ran is not
            // a judge that said no, and the ledger has to show which happened.
            let unavailable = matches!(review.result, review::ReviewResult::Unavailable { .. });
            failure = review_failure(review.result);
            reviews.push(events::ReviewRecord {
                pass: reviewer.lens.name().to_owned(),
                agent: reviewer.profile.agent.clone(),
                model: reviewer.profile.model.clone(),
                adapter: Some(reviewer.adapter.id().to_owned()),
                preflight_cli_version: reviewer.preflight_cli_version.clone(),
                effort: reviewer.profile.effort,
                pool: pool_option(&reviewer.profile.pool),
                cost_usd,
                outcome: match (unavailable, failure.is_none()) {
                    (true, _) => events::ReviewPassOutcome::Unavailable,
                    (false, true) => events::ReviewPassOutcome::Passed,
                    (false, false) => events::ReviewPassOutcome::Failed,
                },
            });
            if failure.is_some() {
                break;
            }
        }
    }

    Ok(AttemptResult {
        outcome,
        failure,
        candidate_branch_ref: candidate.branch_ref,
        candidate_parent: candidate.parent_oid,
        candidate_tree: candidate.tree_oid,
        reviews,
    })
}

fn normalized_plan_bytes(plan: &Plan, path: &Path) -> Result<Vec<u8>, TactusError> {
    let mut bytes = serde_json::to_vec_pretty(plan).map_err(|error| TactusError::Parse {
        message: format!("serializing {}: {error}", path.display()),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Turn a review result into an attempt failure, or `None` if it passed.
fn review_failure(result: review::ReviewResult) -> Option<AttemptFailure> {
    let verdict = match result {
        // The judge could not run. That is an environment problem, not a
        // rejection of the code: it is attributed to the reviewer so the
        // ladder defers instead of blaming the implementer.
        review::ReviewResult::Unavailable { status, detail } => {
            let kind = match status {
                OutcomeStatus::RateLimited => FailureKind::RateLimited,
                OutcomeStatus::Timeout => FailureKind::Timeout,
                _ => FailureKind::ReviewUnavailable,
            };
            return Some(
                AttemptFailure::new(
                    kind,
                    format!("reviewer unavailable: {}", util::head(&detail, 400)),
                )
                .from_reviewer(),
            );
        }
        review::ReviewResult::Judged(verdict) => verdict,
    };

    // §12: the reviewer declined to judge and asked for a person. That is not
    // a rejection of the code, so it must not spend an attempt or escalate —
    // it parks the task and asks.
    if verdict.needs_human {
        let reasons = if verdict.reasons.is_empty() {
            "the reviewer asked for a human decision but gave no reason".to_owned()
        } else {
            verdict.reasons.join("; ")
        };
        return Some(
            AttemptFailure::new(
                FailureKind::NeedsHuman,
                format!("reviewer asked for a human decision: {reasons}"),
            )
            .from_reviewer(),
        );
    }

    // A pass carrying required changes contradicts itself, and the engine is
    // about to commit on the strength of it — fail closed and say why rather
    // than discard the blockers the reviewer took the trouble to write.
    let contradictory = verdict.pass && !verdict.required_changes.is_empty();
    if verdict.pass && !contradictory {
        return None;
    }
    let summary = if contradictory {
        format!(
            "reviewer passed the change but still required: {}",
            verdict.required_changes.join("; ")
        )
    } else if verdict.reasons.is_empty() {
        "no reasons given".to_owned()
    } else {
        verdict.reasons.join("; ")
    };
    // required_changes is what the retry gets back verbatim (§11.4).
    let feedback = if verdict.required_changes.is_empty() {
        summary.clone()
    } else {
        verdict
            .required_changes
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some(
        AttemptFailure::new(
            FailureKind::ReviewFailed,
            // Head, not tail: the reviewer's first reason is its primary
            // finding, and that is what has to reach the user.
            format!("review failed: {}", util::head(&summary, 400)),
        )
        .with_feedback(feedback),
    )
}

/// Artifacts this task should be judged against: its declared inputs, plus
/// the conventions brief whenever one exists (§11.2 injects it into every
/// downstream prompt).
fn load_artifacts(artifacts_dir: &Path, task: &Task) -> Vec<(String, String)> {
    let mut wanted: Vec<String> = vec![CONVENTIONS_BRIEF.to_owned()];
    wanted.extend(task.artifacts_in.iter().map(|id| id.as_str().to_owned()));
    // A task's own outputs are not evidence for judging it: the reviewer
    // would be validating the change against a standard the same attempt just
    // wrote. Declared inputs and the brief only.
    let produced: Vec<&str> = task.artifacts_out.iter().map(|id| id.as_str()).collect();
    let mut seen: Vec<String> = Vec::new();
    wanted
        .into_iter()
        .filter(|id| !produced.contains(&id.as_str()))
        .filter(|id| {
            let fresh = !seen.contains(id);
            if fresh {
                seen.push(id.clone());
            }
            fresh
        })
        .filter_map(|id| {
            let content = fs::read_to_string(artifact_path(artifacts_dir, &id)).ok()?;
            (!content.trim().is_empty()).then_some((id, content))
        })
        .collect()
}

const CONVENTIONS_BRIEF: &str = "conventions-brief";

/// Outcome-level failure reasons, before gates get a say.
fn evaluate_outcome(outcome: &Outcome, output: &proc::ProcessOutput) -> Option<AttemptFailure> {
    let detail = || {
        outcome
            .detail
            .clone()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| {
                let stderr = util::tail(&output.stderr, 400);
                if stderr.is_empty() {
                    "no diagnostic output; see the transcript".to_owned()
                } else {
                    stderr
                }
            })
    };
    match outcome.status {
        // §12: the marker is honoured only on a run that actually completed.
        // `detail` carries the agent's partial output on every failure path,
        // and the prompt puts the marker string in front of the agent on every
        // fresh attempt — so scanning before the status match let a timed-out
        // or rate-limited attempt reclassify itself as a question purely by
        // quoting its own instructions back. That silently defeated "a rate
        // limit defers rather than burning an attempt" (§19), which is most of
        // the point of dispatching on `FailureKind` at all.
        OutcomeStatus::Completed => {
            // An agent that stopped to ask has not failed at anything —
            // punishing it for the empty diff its own question explains would
            // teach it never to ask, so this precedes the evidence rules.
            if let Some(question) = worker_question(outcome.detail.as_deref()) {
                return Some(AttemptFailure::new(FailureKind::NeedsHuman, question));
            }
            if !outcome.diff.trim().is_empty() {
                return None;
            }
            // §11 evidence axis: an empty diff can never pass.
            Some(
                AttemptFailure::new(
                    FailureKind::EmptyDiff,
                    "agent reported success but the diff is empty — \"done\" claims require \
                     changed code",
                )
                .with_feedback(
                    "You reported the task complete, but the repository is unchanged. Either make \
                     the change the task asks for, or explain what blocks it using the \
                     TACTUS-QUESTION marker."
                        .to_owned(),
                ),
            )
        }
        OutcomeStatus::AgentError => Some(
            AttemptFailure::new(
                FailureKind::AgentError,
                format!("agent error (exit {:?}): {}", output.code, detail()),
            )
            .with_feedback(detail()),
        ),
        OutcomeStatus::Timeout => Some(
            AttemptFailure::new(
                FailureKind::Timeout,
                "attempt hit the wall-clock timeout",
            )
            // §19: the feedback is the transcript tail. Without it the retry
            // starts blind on a task already known to run long.
            .with_feedback(format!(
                "Your previous attempt was cut off at its time limit. Work in smaller steps and \
                 finish the highest-value change first. Its last output was:\n{}",
                util::tail(&outcome.detail.clone().unwrap_or_default(), 2000)
            )),
        ),
        OutcomeStatus::RateLimited => Some(AttemptFailure::new(
            FailureKind::RateLimited,
            format!("pool rate-limited: {}", detail()),
        )),
    }
}

/// §12: a worker may flag a decision it should not make alone. Everything from
/// the marker onward is taken, so a multi-line question survives.
///
/// The LAST marker wins, matching the prompt's "end your message with it" and
/// `review.rs`'s rule for verdicts: models restate an instruction before acting
/// on it, so an earlier occurrence is an echo, not the question. The engine
/// itself puts the marker in front of the agent — the empty-diff feedback names
/// it verbatim — so an echo is the expected case, not a rare one.
fn worker_question(detail: Option<&str>) -> Option<String> {
    let detail = detail?;
    let start = detail.rfind(QUESTION_MARKER)?;
    let text = detail[start + QUESTION_MARKER.len()..].trim();
    (!text.is_empty()).then(|| util::head(text, 2000))
}

/// §14 prompt materialization: body + acceptance + artifact inputs + the
/// exact gate commands the agent is permitted to run (the allow rules are
/// exact-match, so the agent must know the literal strings), plus — on a
/// retry — why the last attempt did not pass (§11.4).
fn materialize_prompt(
    task: &Task,
    gate_cmds: &[String],
    artifacts_dir: &Path,
    retry: Option<&RetryBrief>,
) -> String {
    // A resumed session already holds the task, the artifacts, and the rules;
    // re-sending them buys nothing and buries the one thing that changed.
    if let Some(retry) = retry {
        if retry.resumed {
            let mut prompt = String::new();
            prompt.push_str(
                "Your previous attempt did not pass verification. Fix it in this same session — the \
                 task and its rules have not changed.\n\n",
            );
            prompt.push_str(&feedback_section(&retry.feedback, false));
            prompt.push_str(
                "\nMake the smallest change that resolves the above, then stop and summarize.\n",
            );
            return prompt;
        }
    }

    let mut prompt = String::new();
    prompt.push_str(
        "You are executing one task from a frozen plan, conducted by the tactus engine.\n\n",
    );
    let _ = writeln!(prompt, "# Task: {}\n", task.title);
    if !task.body.is_empty() {
        prompt.push_str(&task.body);
        prompt.push_str("\n\n");
    }
    if !task.acceptance.is_empty() {
        prompt.push_str("Acceptance criteria (all must hold when you finish):\n");
        for item in &task.acceptance {
            let _ = writeln!(prompt, "- {item}");
        }
        prompt.push('\n');
    }
    // Artifacts are real files in the run directory: a consumer is shown the
    // content that exists, never told to look for something nothing wrote.
    for id in &task.artifacts_in {
        let path = artifact_path(artifacts_dir, id.as_str());
        match fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                let _ = writeln!(
                    prompt,
                    "Input artifact `{id}` (produced by an earlier task):\n---\n{}\n---\n",
                    content.trim()
                );
            }
            _ => {
                let _ = writeln!(
                    prompt,
                    "Note: this task expected input artifact `{id}`, but the earlier task did \
                     not leave one. Work from the repository as it stands.\n"
                );
            }
        }
    }
    for id in &task.artifacts_out {
        let _ = writeln!(
            prompt,
            "Before you finish, write artifact `{id}` — the notes later tasks depend on — to:\n\
             {}\n",
            artifact_path(artifacts_dir, id.as_str()).display()
        );
    }
    if !gate_cmds.is_empty() {
        prompt.push_str(
            "Verification gates run after you finish. You may run EXACTLY these commands \
             yourself to check your work (any other shell command is denied):\n",
        );
        for cmd in gate_cmds {
            let _ = writeln!(prompt, "- {cmd}");
        }
        prompt.push('\n');
    }
    // Whatever earlier rungs learned travels with the task, even though the
    // conversation does not (§11.4).
    if let Some(retry) = retry {
        prompt.push_str(&feedback_section(&retry.feedback, true));
    }
    prompt.push_str(
        "Rules:\n\
         - Complete ONLY this task; leave work that belongs to other tasks alone.\n\
         - Edit files inside this repository only.\n\
         - NEVER run git commit, branch, merge, push, or reset — the engine owns git.\n\
         - When the acceptance criteria hold, stop and summarize what changed.\n\
         - If a decision genuinely is not yours to make — the task is ambiguous in a way that \
           changes what \"correct\" means, or it turns on a product or policy call you cannot \
           settle from this repository — stop and end your message with a line beginning \
           `TACTUS-QUESTION:` followed by the decision a person has to make. That pauses this \
           task and asks them. Do not use it for uncertainty you could resolve by reading the \
           code.\n",
    );
    prompt
}

/// What earlier attempts learned. `all` carries the accumulated history for a
/// fresh rung; otherwise only the most recent failure, which is what a
/// same-rung retry needs.
fn feedback_section(feedback: &[Feedback], all: bool) -> String {
    let entries: Vec<&Feedback> = if all {
        feedback
            .iter()
            .skip(feedback.len().saturating_sub(MAX_FEEDBACK_ENTRIES))
            .collect()
    } else {
        feedback.last().into_iter().collect()
    };
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if all && entries.len() > 1 {
        out.push_str(
            "Earlier attempts at this task failed. You are a fresh, stronger worker on the same \
             task — do not repeat these:\n\n",
        );
    }
    let last = entries.len() - 1;
    for (position, entry) in entries.iter().enumerate() {
        if entry.human {
            let fence = util::fence_for(entry.detail.as_deref().unwrap_or_default());
            let _ = writeln!(
                out,
                "The operator answered the question that paused this task. This is an \
                 instruction from a person, and it takes precedence over your earlier \
                 assumptions:\n{fence}\n{}\n{fence}\n",
                entry.detail.as_deref().unwrap_or_default().trim()
            );
            continue;
        }
        let where_ = if entry.tier.is_empty() {
            String::new()
        } else {
            format!(" on the {} rung", entry.tier)
        };
        let _ = writeln!(
            out,
            "Attempt {}{where_} failed: {}",
            entry.attempt, entry.summary
        );
        // Only the newest failure carries its full output; older ones would
        // bury it, and the newest is the one still standing in the way.
        if position == last {
            if let Some(detail) = &entry.detail {
                if !detail.trim().is_empty() {
                    let fence = util::fence_for(detail);
                    let _ = writeln!(out, "{fence}\n{}\n{fence}", detail.trim());
                }
            }
        }
        out.push('\n');
    }
    out
}

/// Where an artifact lives for the duration of a run (§15 `artifacts/`).
fn artifact_path(artifacts_dir: &Path, id: &str) -> PathBuf {
    artifacts_dir.join(format!("{}.md", util::filename_component(id)))
}

/// Stable topological order: among ready tasks, lowest plan index first (§14).
/// Used for previews and reporting; the live scheduler derives readiness per
/// step instead, so parked work can be skipped past.
pub fn topo_order(plan: &Plan) -> Vec<usize> {
    let mut done = vec![false; plan.tasks.len()];
    let mut order = Vec::with_capacity(plan.tasks.len());
    let index_of = |id: &str| plan.tasks.iter().position(|t| t.id.as_str() == id);
    while order.len() < plan.tasks.len() {
        let mut advanced = false;
        for i in 0..plan.tasks.len() {
            if done[i] {
                continue;
            }
            let ready = plan.tasks[i]
                .depends_on
                .iter()
                .all(|d| index_of(d.as_str()).is_none_or(|j| done[j]));
            if ready {
                done[i] = true;
                order.push(i);
                advanced = true;
                break;
            }
        }
        if !advanced {
            // Unreachable on a validated plan; degrade to plan order.
            for (i, flag) in done.iter_mut().enumerate() {
                if !*flag {
                    *flag = true;
                    order.push(i);
                }
            }
        }
    }
    order
}

impl RunReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "run: {}", self.run_id);
        let _ = writeln!(out, "branch: {} (return with: git switch -)", self.branch);
        if self.gates.is_empty() {
            let _ = writeln!(out, "gates: none");
        } else {
            let _ = writeln!(
                out,
                "gates: {} [{}]",
                self.gates.join(", "),
                if self.gates_from_config {
                    "from config"
                } else {
                    "derived"
                }
            );
        }
        for warning in &self.warnings {
            let _ = writeln!(out, "warning: {warning}");
        }
        for task in &self.tasks {
            match &task.status {
                TaskRunStatus::Committed { sha } => {
                    // `?` marks a total with unreported components — the
                    // Copilot route bills nothing back, so a two-pass review
                    // shows one reviewer's spend and must not read as both.
                    let partial = if task.review_cost_incomplete { "?" } else { "" };
                    let review = match (task.review_models.as_slice(), task.review_cost_usd) {
                        ([], _) => String::new(),
                        (models, Some(cost)) => {
                            format!(" + review {} ${cost:.4}{partial}", models.join(", "))
                        }
                        // Reviewed only by routes that report no spend (§13) —
                        // say who judged it rather than imply it was free.
                        (models, None) => format!(" + review {} $?", models.join(", ")),
                    };
                    // Same rule as the reviewer half beside it, which has said
                    // `$?` since step 9: a route that reports no spend has not
                    // reported zero. `unwrap_or(0.0)` printed `$0.0000` for a
                    // codex-implemented task while the ledger three lines below
                    // correctly showed `—`, so one run said both.
                    let worker = match task.cost_usd {
                        Some(cost) => format!("${cost:.4}"),
                        None => "$?".to_owned(),
                    };
                    let _ = writeln!(
                        out,
                        "  {}: committed {sha} — {} [{}] ({:.1}s, {} {worker}{review})",
                        task.id,
                        task.title,
                        task.trail(),
                        task.duration.as_secs_f64(),
                        task.model,
                    );
                }
                TaskRunStatus::Failed { reason, .. } => {
                    let _ = writeln!(out, "  {}: FAILED [{}] — {reason}", task.id, task.trail());
                }
                TaskRunStatus::Parked { question, reason } => {
                    let _ = writeln!(
                        out,
                        "  {}: PARKED on {question} [{}] — {reason}",
                        task.id,
                        task.trail()
                    );
                }
                TaskRunStatus::Blocked { by } => {
                    let _ = writeln!(out, "  {}: blocked by `{by}`", task.id);
                }
                TaskRunStatus::Skipped => {
                    // Why it never got its turn, since the two endings are not
                    // the same thing to an operator: a halt is a decision the
                    // run reached, an interruption is one that happened to it
                    // and that `resume` undoes.
                    let ending = if self.interrupted {
                        "run interrupted"
                    } else {
                        "run halted"
                    };
                    let _ = writeln!(out, "  {}: skipped ({ending})", task.id);
                }
                TaskRunStatus::Running {
                    attempt,
                    tier,
                    model,
                } => {
                    let _ = writeln!(
                        out,
                        "  {}: running now — attempt {attempt} on {tier} ({model})",
                        task.id
                    );
                }
                TaskRunStatus::Queued => {
                    let _ = writeln!(out, "  {}: queued", task.id);
                }
                // Only reachable from a `report.json` written by a newer
                // tactus. Say that, rather than picking a familiar-looking
                // status and being confidently wrong about someone's run.
                TaskRunStatus::Unknown => {
                    let _ = writeln!(
                        out,
                        "  {}: status not recognised by this version of tactus",
                        task.id
                    );
                }
            }
        }
        let open: Vec<&QuestionRecord> = self.questions.iter().filter(|q| q.is_open()).collect();
        if !open.is_empty() {
            let _ = writeln!(out, "open questions ({}):", open.len());
            for record in open {
                let _ = writeln!(
                    out,
                    "  {} [{}] — {}",
                    record.question.id,
                    record.question.kind,
                    util::head(
                        record
                            .question
                            .context
                            .lines()
                            .next()
                            .unwrap_or("(no context)"),
                        120
                    )
                );
            }
            let _ = writeln!(
                out,
                "  payloads: {}",
                std::path::Path::new(".tactus")
                    .join("runs")
                    .join(&self.run_id)
                    .join("questions")
                    .display()
            );
        }
        let _ = writeln!(
            out,
            "total: ${:.4}{} (api-equivalent)",
            self.total_cost_usd,
            if self.total_is_floor() { "?" } else { "" }
        );
        // A live run has no outcome yet, and every arm below claims one. Say
        // what is true instead: how far it has got.
        if self.running {
            let _ = writeln!(
                out,
                "run in progress: {} task(s) committed so far on {}",
                self.committed_count(),
                self.branch
            );
            return out;
        }
        // Neither has a run that stopped without recording a finish, and for
        // the same reason: there is no outcome to report yet. `outcome()`
        // cannot see that — a killed run has nothing halted, no budget stop and
        // nothing parked, which reads as `Complete` — so it used to print `run
        // complete: N task(s) committed` about a run that died mid-attempt,
        // one line above `status`'s own `state: interrupted`.
        //
        // "So far" is the live line's word on purpose: more may yet come, once
        // somebody resumes. Which is also why the resume command is not
        // repeated here — the `state:` line in `status` already carries it, and
        // saying it twice invites the two copies to drift.
        if self.interrupted {
            let _ = writeln!(
                out,
                "run interrupted: {} task(s) committed so far on {}",
                self.committed_count(),
                self.branch
            );
            return out;
        }
        match self.outcome() {
            RunOutcome::Halted => {
                let _ = writeln!(
                    out,
                    "run halted at `{}`; completed tasks are committed on {}",
                    self.halted_at.as_deref().unwrap_or("?"),
                    self.branch
                );
            }
            RunOutcome::BudgetExceeded => {
                // `outcome()` only returns this when `budget_stop` is set, so
                // the fallback is unreachable — and it says so rather than
                // naming a plausible ceiling. A specific, checkable, false
                // claim about the operator's own config is the worst thing to
                // print here.
                let stopped = self.budget_stop.as_ref().map_or_else(
                    || "run stopped at a budget it did not record".to_owned(),
                    |stop| {
                        format!(
                            "run stopped at its budget: [budgets] {} = ${:.4}, reported spend \
                             ${:.4}",
                            stop.budget, stop.limit_usd, stop.spent_usd
                        )
                    },
                );
                let _ = writeln!(
                    out,
                    "{stopped}. Committed tasks are on {}; raise the ceiling and continue \
                     with:\n    tactus resume {} --budget <usd>",
                    self.branch, self.run_id
                );
            }
            RunOutcome::Parked => {
                let _ = writeln!(
                    out,
                    "run ended with {} task(s) parked on unanswered questions: {}",
                    self.parked_tasks().len(),
                    self.parked_tasks().join(", ")
                );
            }
            RunOutcome::Complete => {
                let committed = self.committed_count();
                let _ = writeln!(
                    out,
                    "run complete: {committed} task(s) committed on {}",
                    self.branch
                );
            }
        }
        out
    }

    /// §21's definition-of-done (e): what each task cost, and on what.
    ///
    /// Implementer and reviewer spend stay in separate columns because they
    /// are different models at different tiers — folding them together makes a
    /// cheap rung look expensive to anyone reading the ledger (§13). An
    /// unreported cost prints as `—` rather than `$0.0000`: a ledger that
    /// cannot tell free from unreported is worse than no ledger.
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        let money = |value: Option<f64>| match value {
            Some(amount) => format!("${amount:.4}"),
            None => "—".to_owned(),
        };
        // A figure that omits a reviewer whose route bills nothing back is not
        // the total, and this column is where someone decides what a run cost.
        let partial = |rendered: String, incomplete: bool| {
            if incomplete && rendered != "—" {
                format!("{rendered}?")
            } else {
                rendered
            }
        };
        let rows: Vec<[String; 6]> = self
            .tasks
            .iter()
            .map(|task| {
                [
                    task.id.clone(),
                    task.attempts.len().to_string(),
                    if task.trail().is_empty() {
                        "—".to_owned()
                    } else {
                        task.trail()
                    },
                    partial(money(task.cost_usd), task.cost_incomplete()),
                    partial(money(task.review_cost_usd), task.review_cost_incomplete),
                    partial(
                        money(task.total_cost_usd()),
                        task.cost_incomplete() || task.review_cost_incomplete,
                    ),
                ]
            })
            .collect();
        let headers = ["task", "attempts", "trail", "worker", "review", "total"];
        let widths: Vec<usize> = (0..headers.len())
            .map(|column| {
                rows.iter()
                    .map(|row| row[column].chars().count())
                    .chain(std::iter::once(headers[column].chars().count()))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let line = |cells: &[String]| {
            let mut rendered = String::from("  ");
            for (index, cell) in cells.iter().enumerate() {
                let pad = widths[index].saturating_sub(cell.chars().count());
                let _ = write!(rendered, "{cell}{:pad$}", "", pad = pad);
                if index + 1 < cells.len() {
                    rendered.push_str("  ");
                }
            }
            rendered.trim_end().to_owned()
        };

        let _ = writeln!(out, "ledger:");
        let _ = writeln!(out, "{}", line(&headers.map(str::to_owned)));
        for row in &rows {
            let _ = writeln!(out, "{}", line(row));
        }
        let _ = writeln!(
            out,
            "  total ${:.4}{} (api-equivalent; subscription spend is notional — §13)",
            self.total_cost_usd,
            if self.total_is_floor() { "?" } else { "" }
        );
        if self.total_is_floor() {
            let _ = writeln!(
                out,
                "  `?` marks a figure missing an attempt whose route reports no spend, or one \
                 the engine was killed inside — a floor, not a total (§13)"
            );
        }
        // §13's second currency. An empty section means no attempt in this run
        // named a pool — which is the honest reading of "no pools connected",
        // and is said rather than left as a blank column that looks like
        // "nothing was spent".
        if self.pool_drain.is_empty() {
            let _ = writeln!(
                out,
                "  per-pool drain: no pool is connected for the agents this run used — run \
                 `tactus connect`"
            );
        } else {
            let _ = writeln!(out, "  per-pool drain:");
            for row in &self.pool_drain {
                let spend = match row.cost_usd {
                    Some(cost) if row.unpriced > 0 => format!("${cost:.4}?"),
                    Some(cost) => format!("${cost:.4}"),
                    // Every attempt on this pool ran on a route that reports no
                    // spend (§13) — saying "$0.0000" would read as free.
                    None => "— (this route reports no spend)".to_owned(),
                };
                let _ = writeln!(
                    out,
                    "    {}: {} attempt(s), {spend}",
                    row.pool, row.attempts
                );
            }
        }
        if let Some(stop) = &self.budget_stop {
            let _ = writeln!(
                out,
                // The ledger annotates; `render` owns the outcome line and the
                // resume advice. Printing both put two near-identical
                // paragraphs, formatted to different precision, with two copies
                // of the same command, back to back in `tactus status` — which
                // reads as two things having happened.
                "  stopped by [budgets] {} = ${:.4} before `{}` (§13)",
                stop.budget, stop.limit_usd, stop.task
            );
        }
        out
    }
}

#[cfg(test)]
mod tests;
