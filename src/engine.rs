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

use std::collections::BTreeMap;
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
use crate::rundir::{self, RunLock, RunPaths};
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

/// What each review pass gets of the attempt's wall clock.
///
/// A reviewer reads a diff and answers — it has no shell and no edit tools, so
/// it does not need the implementer's budget. Step-6 finding #13 was that
/// giving it the full one let a single task consume several multiples of the
/// attempt timeout, and the fix was a quarter.
///
/// That quarter is the budget for *review*, not for one reviewer, so it splits
/// across the passes (§11.3). Otherwise configuring a second opinion would
/// silently double a bound that was set deliberately. The 60s floor is per
/// pass, because a budget too small to answer in is not a budget.
fn review_timeout(attempt_timeout: Duration, passes: usize) -> Duration {
    let share = attempt_timeout / 4 / u32::try_from(passes.max(1)).unwrap_or(u32::MAX);
    share.max(Duration::from_secs(60))
}

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
        Some(plan) => plan,
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
    let Preflight {
        analysis,
        caps,
        review_plan,
        gates,
        gate_cmds,
        mut warnings,
        mode,
        notifiers,
        budgets,
    } = preflight(opts, harness)?;

    let workspace = Workspace::open(&opts.repo_root)?;
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

    // Nothing is on the record until the first event lands, so a failure in
    // this window would leave a run directory with no `events.jsonl` in it —
    // and that husk becomes `latest_run`, so a bare `tactus status` reports
    // "no event log here" for a run that never began, shadowing the real
    // latest one until someone deletes it by hand. Best-effort: failing to
    // tidy up must not mask the error that actually stopped the run.
    let opened = util::write_json(&paths.plan_json(), &analysis.plan)
        .and_then(|()| workspace.create_branch(&branch));
    if let Err(error) = opened {
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

    // Claimed before anything is read, so two resumes cannot race each other
    // into the same branch. The lock sits beside the ops surface, which is the
    // only half of the run directory known this early: where the private half
    // went is recorded in `run_started`, and that has not been read yet.
    let _lock = RunLock::acquire(&public)?;

    let mut warnings = Vec::new();
    let events_path = public.join("events.jsonl");
    let events = events::read_all(&events_path, &mut warnings)?;
    let started = events::started_of(&events, &events_path)?.clone();
    let effective_schema = events::ensure_supported_schema(&started, &events, &events_path)?;
    // Usually `run_started`'s, but a log too old to carry them there may have
    // had them established by an earlier resume instead — which is what stops
    // the re-derivation repeating, and drifting, on every resume after that.
    let recorded_gates = events::recorded_gates(&events).cloned();
    let recorded_effort_policy = events::recorded_effort_policy(&events);
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
            reviews: started.reviews.clone(),
            gates: recorded_gates.clone(),
            gates_from_config: started.gates_from_config,
            routing: Some(RecordedRouting {
                run_id: run_id.clone(),
                structure: started.chains.clone(),
                bindings: recorded_chains.clone(),
            }),
        },
    )?;
    if started.reviews.is_none() {
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

    let workspace = Workspace::open(&opts.repo_root)?;
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
    let head = workspace.head_sha_full()?;

    // One commit past the record can be this run's own work rather than
    // foreign history. §14 commits, reads the sha back, scrubs the tree, and
    // only then appends `task_committed` — a process that dies inside those
    // three git calls leaves exactly this shape. Refusing it would tell the
    // operator to throw away a commit that already passed its gates and its
    // review, and to spend the attempt again. So the engine adopts its own
    // commit, but only when every part of the shape agrees: the other reading
    // of an unexpected commit is that somebody else made it.
    let mut adopted = None;
    if head != recorded_head
        && let Some((task, message)) = unrecorded_commit(&replayed, &analysis.plan)
        && workspace.parent_sha(&head)?.as_deref() == Some(recorded_head.as_str())
        && workspace.commit_subject(&head)? == message
    {
        warnings.push(format!(
            "adopted commit {head} as `{task}`: the run committed it and stopped before \
             recording it, which left the branch one commit ahead of its own log"
        ));
        adopted = Some((task, message));
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
    let paths = match &opts.private_root {
        Some(root) => RunPaths::with_private_root(&opts.repo_root, &run_id, root),
        None => RunPaths::from_parts(public.clone(), PathBuf::from(&started.private_dir)),
    };
    paths.create()?;
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
    };
    // A legacy run cannot have its opening event rewritten without violating
    // append-only history. This no-op event is therefore the schema-2 boundary:
    // schema-1 binaries do not know its tag and refuse before ignoring the new
    // effort/binding snapshots that follow.
    if effective_schema < events::SCHEMA_VERSION {
        run.emit(EventBody::RunSchemaUpgraded {
            data: events::RunSchemaUpgraded {
                from: effective_schema,
                to: events::SCHEMA_VERSION,
            },
        })?;
    }
    // The `task_committed` the dead process never got to, now that the commit
    // has been checked against the record. First of everything this resume
    // writes, because it is the thing that happened first.
    if let Some((task, message)) = adopted {
        run.emit(EventBody::TaskCommitted {
            task,
            data: events::TaskCommitted {
                sha: head.clone(),
                message,
            },
        })?;
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
            chains: recorded_chains
                .is_none()
                .then(|| chain_summaries(&analysis)),
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
fn unrecorded_commit(replayed: &events::Replay, plan: &Plan) -> Option<(String, String)> {
    let EventBody::AttemptFinished { task, data, .. } = &replayed.events.last()?.body else {
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
    ))
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
                    retry,
                    // The same entries the worker prompt quotes as operator
                    // instruction, routed to the judge as well (§12).
                    decisions: self.state.progress[index]
                        .feedback
                        .iter()
                        .filter(|entry| entry.human)
                        .filter_map(|entry| entry.detail.clone())
                        .collect(),
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

            self.emit(EventBody::AttemptFinished {
                task: task_id.clone(),
                attempt,
                rung: rung_number,
                profile: profile.name.clone(),
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
            })?;

            let Some(failure) = result.failure else {
                let message = format!("[tactus] {}: {}", task.id, task.title);
                self.workspace.commit(&message)?;
                // The full sha, not `commit`'s abbreviated one: `resume`
                // compares this against HEAD, and abbreviation length varies
                // with `core.abbrev` and the repo's object count.
                let full_sha = self.workspace.head_sha_full()?;
                // Scrub gate side-effects (build artifacts, lockfile churn) so
                // they cannot leak into the next task's captured diff; the
                // commit recorded exactly the verified staged set.
                self.workspace.discard_uncommitted()?;
                self.emit(EventBody::TaskCommitted {
                    task: task_id.clone(),
                    data: events::TaskCommitted {
                        sha: full_sha,
                        message,
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
            if failure.kind == FailureKind::RateLimited {
                self.record_pool_exhausted(&task_id, &profile, &result.reviews, &failure)?;
            } else {
                // This attempt reached a model and got an answer, whatever the
                // verdict on its code, so any pool it drew on is serving again.
                // Same rule as `capacity::observe`'s, applied to the engine's
                // own view so the two cannot disagree about when a pool
                // recovered — without it, the *next* outage on the same pool
                // would go unrecorded because the set still held it.
                self.exhausted_pools.remove(&profile.pool);
                for review in &result.reviews {
                    if review.outcome != events::ReviewPassOutcome::Unavailable
                        && let Some(pool) = &review.pool
                    {
                        self.exhausted_pools.remove(pool);
                    }
                }
            }

            let resumable = self.state.progress[index].session.is_some()
                && self
                    .caps
                    .get(&profile.agent)
                    .is_some_and(|c| c.session_resume);
            let state = LadderState {
                rung: self.state.progress[index].rung,
                attempts_on_rung: self.state.progress[index].attempts_on_rung,
                defers: self.state.progress[index].defers,
                resumable,
            };
            let next = ladder::next_step(&failure, &state, &policy);

            // §14: the tree survives only for a resumed retry, where the
            // *cumulative* diff is what gets re-gated. Every other branch
            // hands the scheduler a clean workspace, because another task may
            // run before this one does again.
            if !matches!(next, Next::RetrySameRung { resume: true }) {
                self.workspace.discard_uncommitted()?;
            }

            match next {
                Next::RetrySameRung { resume } => {
                    self.emit(EventBody::LadderRetry {
                        task: task_id.clone(),
                        attempt,
                        rung: rung_number,
                        data: events::LadderRetry {
                            resume,
                            tier: rung.tier.to_string(),
                            summary: failure.reason.clone(),
                            detail: failure.feedback.clone(),
                        },
                    })?;
                }
                Next::Escalate => {
                    let onto = chain.rungs.get(rung_index + 1).map(|next| next.tier);
                    // Escalate FIRST, then ask. The order is what makes the
                    // approval path need no special case anywhere else: the
                    // escalation's own fold moves the rung and resets
                    // `attempts_on_rung`, so an approved task un-parks already
                    // standing on the frontier rung with a fresh allowance —
                    // rather than re-running the rung it had just exhausted, or
                    // arriving back here and asking the same question again.
                    self.emit(EventBody::LadderEscalated {
                        task: task_id.clone(),
                        attempt,
                        rung: rung_number,
                        data: events::LadderEscalated {
                            to_rung: rung_number.saturating_add(1),
                            tier: rung.tier.to_string(),
                            summary: failure.reason.clone(),
                            detail: failure.feedback.clone(),
                        },
                    })?;
                    if let Some(onto) = onto
                        && self.should_approve_spend(rung.tier, onto)
                    {
                        let question = self.raise_spend_approval(index, onto)?;
                        self.emit(EventBody::TaskParked {
                            task: task_id.clone(),
                            data: events::TaskParked {
                                question: question.to_string(),
                                // The attempt that caused this escalation was
                                // genuinely judged and genuinely failed, so its
                                // allowance stays spent (§12's refund is for
                                // work nobody judged).
                                refund_attempt: false,
                            },
                        })?;
                        return Ok(false);
                    }
                }
                Next::Defer => {
                    // No attempt was spent on the work itself, so the event's
                    // fold gives the rung its allowance back (§19).
                    self.emit(EventBody::TaskDeferred {
                        task: task_id.clone(),
                        data: events::TaskDeferred {
                            reason: failure.reason.clone(),
                            defers: self.state.progress[index].defers.saturating_add(1),
                        },
                    })?;
                    return Ok(true);
                }
                Next::AskHuman(kind) => {
                    let context =
                        question_context(task, kind, &failure, &self.state.progress[index]);
                    let question = self.raise_question(index, kind, context)?;
                    self.emit(EventBody::TaskParked {
                        task: task_id.clone(),
                        data: events::TaskParked {
                            question: question.to_string(),
                            // Nobody judged the code, so the attempt is not
                            // spent (§12).
                            refund_attempt: kind == QuestionKind::Clarify,
                        },
                    })?;
                    return Ok(false);
                }
                Next::Fail => {
                    self.fail_task(index, failure.kind, failure.reason.clone())?;
                    return Ok(false);
                }
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
    fn should_approve_spend(&self, from: crate::ir::Tier, onto: crate::ir::Tier) -> bool {
        let Some(threshold) = self.ask_before.frontier_escalation_over_usd else {
            return false;
        };
        onto == crate::ir::Tier::Frontier
            && from != crate::ir::Tier::Frontier
            && self.reported_spend(None) >= threshold
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
        let task = self.analysis.plan.tasks[index].id.to_string();
        // The halt policy is resolved here and recorded, not re-derived on
        // replay: a `tactus.toml` edited between a run and its resume must not
        // rewrite which task the report blames for stopping.
        let halts_run = self.on_task_failure == OnTaskFailure::Halt;
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
    fn raise_spend_approval(
        &mut self,
        index: usize,
        onto: crate::ir::Tier,
    ) -> Result<QuestionId, TactusError> {
        let context = spend_question_context(
            &self.analysis.plan.tasks[index],
            onto,
            self.reported_spend(None),
            self.ask_before.frontier_escalation_over_usd.unwrap_or(0.0),
            self.unpriced_attempts() > 0,
        );
        self.raise_question(index, QuestionKind::ApproveSpend, context)
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

    fn raise_question(
        &mut self,
        index: usize,
        kind: QuestionKind,
        context: String,
    ) -> Result<QuestionId, TactusError> {
        let task = &self.analysis.plan.tasks[index];
        let question = Question {
            id: interaction::new_question_id(),
            kind,
            // v0.1 parks only the task that raised it. Dependents are held by
            // the graph, not by the question, so they stay eligible the moment
            // an answer arrives.
            affected_tasks: vec![task.id.clone()],
            context,
            options: question_options(kind),
        };
        let id = question.id.clone();
        for notifier in &self.notifiers {
            // A notifier that cannot deliver must not take the run with it:
            // the question is on disk either way (§12).
            if let Err(error) = notifier.ask(&question) {
                self.warnings.push(format!(
                    "notifier `{}` could not deliver question {id}: {error}",
                    notifier.id()
                ));
            }
        }
        // The payload lands on disk before the event, so a reader that sees
        // `question_raised` can always open the file it names.
        interaction::write_question(
            &self.paths.questions(),
            &QuestionRecord::open(question.clone()),
        )?;
        self.emit(EventBody::QuestionRaised {
            task: self.analysis.plan.tasks[index].id.to_string(),
            data: Box::new(events::QuestionRaised { question }),
        })?;
        Ok(id)
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
    /// `None` on the first attempt.
    retry: Option<RetryBrief>,
    /// Answers the operator has given about this task (§12), in the order they
    /// arrived. The worker gets these as instructions; so must the judge.
    decisions: Vec<String>,
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
    outcome.diff = workspace.capture_diff()?;
    outcome.transcript_path = transcript_path;

    // Verification ladder (§11): outcome sanity → cheap static provenance →
    // gates → review. Cheapest and most objective first.
    let mut failure = evaluate_outcome(&outcome, &output);
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
    if failure.is_none()
        && let Some(gate_failure) =
            gates::run_all(cx.gates, workspace, &cx.paths.gates(), &cx.stem, cx.attempt)?
    {
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
        let budget = review_timeout(cx.timeout, cx.reviewers.len());
        for reviewer in &cx.reviewers {
            let review = review::run_review(&review::ReviewCx {
                adapter: reviewer.adapter,
                profile: reviewer.profile.clone(),
                lens: reviewer.lens,
                task: cx.task,
                diff: &outcome.diff,
                artifacts: &artifacts,
                decisions: &cx.decisions,
                workspace: workspace.root(),
                settings_dir: &cx.paths.settings(),
                reviews_dir: &cx.paths.reviews(),
                stem: format!("{}-{}", cx.stem, cx.attempt),
                timeout: budget,
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
        reviews,
    })
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
    if let Some(retry) = retry
        && retry.resumed
    {
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
        if position == last
            && let Some(detail) = &entry.detail
            && !detail.trim().is_empty()
        {
            let fence = util::fence_for(detail);
            let _ = writeln!(out, "{fence}\n{}\n{fence}", detail.trim());
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
mod tests {
    use super::*;
    use crate::agent::{Caps, ProcessOutput};
    use crate::ir::{Effort, TaskId, Usage};
    use std::path::Path;
    use std::process::Command;
    use std::sync::{Mutex, OnceLock};

    #[derive(Clone, Copy, PartialEq)]
    enum Effect {
        /// Simulates an agent that edits the workspace and succeeds.
        EditFile,
        /// Simulates an agent that writes real test code.
        EditTest,
        /// Simulates a lying agent: success report, no changes.
        NoEdit,
        /// Simulates an agent-side failure.
        Error,
        /// Simulates the pool being exhausted.
        RateLimited,
        /// Edits, then stops and asks the operator a question (§12).
        AskQuestion,
        /// Kills the whole process partway through the attempt, leaving the
        /// on-disk shape a `kill -9` or a power loss leaves: a dirty working
        /// tree and an `attempt_started` with no `attempt_finished`.
        Exit,
    }

    /// Distinctive so the parent can tell a deliberate death from a panic,
    /// which would also exit non-zero.
    const CRASH_EXIT_CODE: i32 = 42;

    #[derive(Clone, Copy, PartialEq)]
    enum ReviewBehavior {
        Pass,
        Fail,
        /// Prose with no verdict block: drives the re-ask path.
        Unparseable,
        /// The judge itself could not run.
        RateLimited,
        /// §12: the reviewer declines to judge and asks for a person.
        NeedsHuman,
    }

    /// Scripted stand-in for a real CLI. `build` performs the "agent edit"
    /// directly (test-only shortcut) and returns a trivial command; `parse`
    /// reports the scripted outcome. Read-only profiles are review
    /// invocations and answer with a verdict, exercising the real
    /// command → stdout → parse → verdict path.
    ///
    /// Both scripts are consumed per invocation and the final entry repeats,
    /// so a one-element script behaves exactly like the fixed adapter did.
    struct FakeAdapter {
        /// Which agent this stands in for. Cross-vendor tests (§11.3) need two
        /// ids, because "a different model family" is unreachable otherwise.
        id: &'static str,
        effects: Vec<Effect>,
        reviews: Vec<ReviewBehavior>,
        /// Simulates a CLI that is installed but broken, for the pre-flight
        /// probe classes: required agents refuse the run, the opportunistic
        /// cross-family one only warns.
        probe_error: Option<&'static str>,
        /// Whether this route reports spend. Copilot's does not.
        reports_cost: bool,
        calls: Mutex<Calls>,
    }

    #[derive(Default)]
    struct Calls {
        worker: usize,
        review: usize,
        runs: Vec<RecordedRun>,
    }

    #[derive(Clone)]
    struct RecordedRun {
        model: String,
        resume: Option<String>,
        prompt: String,
    }

    /// Marker the fake's review command prints so `parse` can tell a review
    /// invocation from an implementation one.
    const REVIEW_MARKER: &str = "TACTUS-FAKE-REVIEW";

    impl FakeAdapter {
        fn new(effects: Vec<Effect>, reviews: Vec<ReviewBehavior>) -> Self {
            Self {
                id: "claude-code",
                effects,
                reviews,
                probe_error: None,
                reports_cost: true,
                calls: Mutex::new(Calls::default()),
            }
        }

        /// The second vendor. It only ever reviews in these tests, so it needs
        /// no effects script.
        fn copilot(reviews: Vec<ReviewBehavior>) -> Self {
            Self {
                id: "copilot",
                effects: Vec::new(),
                reviews,
                probe_error: None,
                reports_cost: true,
                calls: Mutex::new(Calls::default()),
            }
        }

        fn broken(mut self, message: &'static str) -> Self {
            self.probe_error = Some(message);
            self
        }

        /// Stands in for the Copilot route, which has no JSON envelope and so
        /// reports no spend at all (§13).
        fn unpriced(mut self) -> Self {
            self.reports_cost = false;
            self
        }

        fn runs(&self) -> Vec<RecordedRun> {
            self.calls
                .lock()
                .map(|c| c.runs.clone())
                .unwrap_or_default()
        }

        /// How many review invocations this adapter was asked for.
        fn reviews_run(&self) -> usize {
            self.calls.lock().map(|c| c.review).unwrap_or_default()
        }
    }

    fn fake(effect: Effect) -> FakeSource {
        source(vec![effect], vec![ReviewBehavior::Pass])
    }

    fn source(effects: Vec<Effect>, reviews: Vec<ReviewBehavior>) -> FakeSource {
        FakeSource {
            adapter: FakeAdapter::new(effects, reviews),
            copilot: None,
        }
    }

    /// A machine with both CLIs installed: claude-code implements and gives the
    /// acceptance verdict, copilot gives the §11.3 second opinion. Each adapter
    /// keeps its own review script and counter, so a test can say what each
    /// vendor answered and check which of them was asked at all.
    fn cross_vendor(
        effects: Vec<Effect>,
        reviews: Vec<ReviewBehavior>,
        second: Vec<ReviewBehavior>,
    ) -> FakeSource {
        FakeSource {
            adapter: FakeAdapter::new(effects, reviews),
            copilot: Some(FakeAdapter::copilot(second)),
        }
    }

    /// The last scripted entry repeats forever.
    fn scripted<T: Copy>(script: &[T], index: usize, fallback: T) -> T {
        script
            .get(index)
            .copied()
            .or_else(|| script.last().copied())
            .unwrap_or(fallback)
    }

    impl AgentAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            self.id
        }

        fn probe(&self) -> Result<Caps, TactusError> {
            if let Some(message) = self.probe_error {
                return Err(TactusError::Agent {
                    message: message.to_owned(),
                });
            }
            Ok(Caps {
                version: "0.0.0-fake".to_owned(),
                json_output: true,
                session_resume: true,
                cost_reporting: true,
                read_only_mode: true,
                acp: false,
                model_list: false,
            })
        }

        fn build(&self, run: &TaskRun) -> Result<std::process::Command, TactusError> {
            if run.profile.permissions == PermissionMode::ReadOnly {
                let mut cmd = shell_command(&format!("echo {REVIEW_MARKER}"));
                cmd.current_dir(&run.workspace);
                return Ok(cmd);
            }
            let index = {
                let mut calls = self.calls.lock().map_err(|_| TactusError::Agent {
                    message: "fake adapter lock poisoned".to_owned(),
                })?;
                let index = calls.worker;
                calls.worker += 1;
                calls.runs.push(RecordedRun {
                    model: run.profile.model.clone(),
                    resume: run.resume_session.clone(),
                    prompt: run.prompt.clone(),
                });
                index
            };
            let edit: Option<(&str, String)> =
                match scripted(&self.effects, index, Effect::EditFile) {
                    Effect::Exit => {
                        // Half-finished edits first, then die without
                        // unwinding — no destructors, no flush of anything the
                        // engine has not already synced. That is what makes
                        // this a faithful stand-in for a kill rather than a
                        // tidy shutdown, and it happens at a deterministic
                        // point instead of racing a signal.
                        let _ = fs::write(
                            run.workspace.join("agent-output.txt"),
                            "half-written by an agent that never came back\n",
                        );
                        std::process::exit(CRASH_EXIT_CODE);
                    }
                    Effect::EditFile | Effect::AskQuestion => {
                        let marker = run.workspace.join("agent-output.txt");
                        let previous = fs::read_to_string(&marker).unwrap_or_default();
                        Some(("agent-output.txt", format!("{previous}edited: {index}\n")))
                    }
                    Effect::EditTest => Some((
                        "widget_test.rs",
                        "#[test]\nfn widget_works() {\n    assert!(true);\n}\n".to_owned(),
                    )),
                    Effect::NoEdit | Effect::Error | Effect::RateLimited => None,
                };
            if let Some((name, content)) = edit {
                fs::write(run.workspace.join(name), content).map_err(|e| TactusError::Agent {
                    message: format!("fake edit failed: {e}"),
                })?;
            }
            let mut cmd = shell_command("exit 0");
            cmd.current_dir(&run.workspace);
            Ok(cmd)
        }

        // Delegate to the real generator so the engine's permission wiring is
        // exercised, not stubbed out.
        fn materialize_permissions(
            &self,
            profile: &WorkerProfile,
            gate_cmds: &[String],
            dir: &Path,
            stem: &str,
        ) -> Result<Option<PathBuf>, TactusError> {
            crate::agent::claude::ClaudeCodeAdapter
                .materialize_permissions(profile, gate_cmds, dir, stem)
        }

        fn parse(&self, out: &ProcessOutput) -> Result<Outcome, TactusError> {
            if out.stdout.contains(REVIEW_MARKER) {
                let index = {
                    let mut calls = self.calls.lock().map_err(|_| TactusError::Agent {
                        message: "fake adapter lock poisoned".to_owned(),
                    })?;
                    let index = calls.review;
                    calls.review += 1;
                    index
                };
                let behavior = scripted(&self.reviews, index, ReviewBehavior::Pass);
                if behavior == ReviewBehavior::RateLimited {
                    return Ok(fake_outcome(
                        OutcomeStatus::RateLimited,
                        Some("5-hour limit reached".to_owned()),
                        "fake-review-session",
                        Some(0.0),
                        out.duration,
                    ));
                }
                let answer = match behavior {
                    ReviewBehavior::Pass => {
                        "Checked every criterion.\n```json\n{\"pass\": true, \"reasons\": \
                         [\"meets the acceptance criteria\"], \"required_changes\": []}\n```"
                    }
                    ReviewBehavior::Fail => {
                        "The diff misses a case.\n```json\n{\"pass\": false, \"reasons\": \
                         [\"no error handling for empty input\"], \"required_changes\": \
                         [\"handle the empty-input case\"]}\n```"
                    }
                    ReviewBehavior::NeedsHuman => {
                        "This turns on a product decision.\n```json\n{\"pass\": false, \
                         \"reasons\": [\"the acceptance criteria contradict the API contract\"], \
                         \"needs_human\": true}\n```"
                    }
                    ReviewBehavior::Unparseable => "Looks fine to me, ship it.",
                    ReviewBehavior::RateLimited => unreachable!("handled above"),
                };
                return Ok(fake_outcome(
                    OutcomeStatus::Completed,
                    Some(answer.to_owned()),
                    "fake-review-session",
                    self.reports_cost.then_some(0.05),
                    out.duration,
                ));
            }
            // `build` already consumed this invocation's slot.
            let index = self
                .calls
                .lock()
                .map(|c| c.worker.saturating_sub(1))
                .unwrap_or(0);
            let effect = scripted(&self.effects, index, Effect::EditFile);
            let status = match effect {
                Effect::Error => OutcomeStatus::AgentError,
                Effect::RateLimited => OutcomeStatus::RateLimited,
                // `Exit` never reaches here — `build` ends the process.
                Effect::EditFile
                | Effect::EditTest
                | Effect::NoEdit
                | Effect::AskQuestion
                | Effect::Exit => OutcomeStatus::Completed,
            };
            let detail = match effect {
                Effect::Error => Some("fake adapter error detail".to_owned()),
                Effect::RateLimited => Some("5-hour limit reached".to_owned()),
                Effect::AskQuestion => Some(
                    "I made a start but stopped.\nTACTUS-QUESTION: should cursors be opaque or \
                     signed?"
                        .to_owned(),
                ),
                _ => None,
            };
            let mut outcome = fake_outcome(
                status,
                detail,
                &format!("s{index}"),
                Some(0.01),
                out.duration,
            );
            outcome.usage = Some(Usage::default());
            Ok(outcome)
        }
    }

    fn fake_outcome(
        status: OutcomeStatus,
        detail: Option<String>,
        session: &str,
        cost_usd: Option<f64>,
        duration: Duration,
    ) -> Outcome {
        Outcome {
            status,
            diff: String::new(),
            detail,
            session_id: Some(session.to_owned()),
            usage: None,
            cost_usd,
            transcript_path: PathBuf::new(),
            duration,
        }
    }

    struct FakeSource {
        adapter: FakeAdapter,
        /// `None` is the single-vendor machine — which is also the shape that
        /// makes a cross-family reviewer unresolvable.
        copilot: Option<FakeAdapter>,
    }

    impl FakeSource {
        fn copilot(&self) -> &FakeAdapter {
            self.copilot.as_ref().expect("this source has a copilot")
        }
    }

    impl AdapterSource for FakeSource {
        fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
            if id == self.adapter.id {
                return Some(&self.adapter as &dyn AgentAdapter);
            }
            self.copilot
                .as_ref()
                .filter(|a| a.id == id)
                .map(|a| a as &dyn AgentAdapter)
        }
    }

    /// Answers handed out in order; anything past the script is unanswered,
    /// which is exactly how a detached terminal behaves.
    struct ScriptedAnswers {
        answers: Mutex<std::collections::VecDeque<Answer>>,
    }

    impl ScriptedAnswers {
        fn new(answers: Vec<Answer>) -> Self {
            Self {
                answers: Mutex::new(answers.into()),
            }
        }
    }

    impl AnswerSource for ScriptedAnswers {
        fn id(&self) -> &'static str {
            "scripted"
        }

        fn resolve(&self, _question: &Question) -> Result<Answer, TactusError> {
            Ok(self
                .answers
                .lock()
                .ok()
                .and_then(|mut a| a.pop_front())
                .unwrap_or(Answer::Unanswered))
        }
    }

    #[derive(Default)]
    struct RecordingSleeper {
        waits: Mutex<Vec<Duration>>,
    }

    impl Sleeper for RecordingSleeper {
        fn sleep(&self, duration: Duration) {
            if let Ok(mut waits) = self.waits.lock() {
                waits.push(duration);
            }
        }
    }

    impl RecordingSleeper {
        fn waits(&self) -> Vec<Duration> {
            self.waits.lock().map(|w| w.clone()).unwrap_or_default()
        }
    }

    /// Shared with the production path so tests exercise the same shell
    /// invocation (including its Windows quoting) rather than a parallel one.
    fn shell_command(script: &str) -> Command {
        crate::gates::ShellKind::native().command(script)
    }

    fn git_in(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn temp_engine_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tactus-engine-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create repo dir");
        git_in(&dir, &["init", "-q", "-b", "main"]);
        git_in(&dir, &["config", "user.email", "test@tactus.local"]);
        git_in(&dir, &["config", "user.name", "tactus tests"]);
        fs::write(dir.join("README.md"), "seed\n").expect("seed");
        fs::write(
            dir.join("plan.md"),
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\nMake it.\n\n\
             ## Document the widget\n<!-- tactus: id=t2 depends=t1 -->\nWrite it up.\n",
        )
        .expect("plan");
        git_in(&dir, &["add", "-A"]);
        git_in(&dir, &["commit", "-q", "-m", "seed"]);
        dir
    }

    /// Replace the plan and config, then commit so the tree is clean.
    fn seed(repo: &Path, plan: &str, config: Option<&str>) {
        fs::write(repo.join("plan.md"), plan).expect("plan");
        if let Some(config) = config {
            fs::write(repo.join("tactus.toml"), config).expect("config");
        }
        git_in(repo, &["add", "-A"]);
        git_in(repo, &["commit", "-q", "-m", "fixture"]);
    }

    fn options(repo: &Path) -> RunOptions {
        let mut opts = RunOptions::new(repo.join("plan.md"), repo.to_path_buf());
        opts.pools_path = Some(no_pools());
        opts.attempt_timeout = Duration::from_secs(60);
        // Tests must never actually wait — not out a rate limit, and not at a
        // hard block either. The test harness has no terminal, so an
        // interactive mode resolves to the waiting answer channel; without a
        // zero budget every parking test would sit out the real one.
        opts.defer_backoff = Duration::ZERO;
        opts.wait_on_block = Some(Duration::ZERO);
        opts.private_root = Some(private_root_for(repo));
        opts
    }

    /// An explicit pools path with no pools in it.
    ///
    /// A real, empty file rather than an absent one: an explicit `--pools` that
    /// does not exist is a hard error now, and `None` would reach for the
    /// operator's real `~/.tactus/pools.toml` — which no test may touch.
    /// An empty pools file, created once for the whole test process.
    ///
    /// Every test routes through here, and this used to *rewrite* the file on
    /// each call — one shared path truncated and rewritten while other threads
    /// were reading it. The content is the same for every caller, so there is
    /// nothing to rewrite: build it once and hand back the path.
    fn no_pools() -> PathBuf {
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let dir =
                std::env::temp_dir().join(format!("tactus-engine-nopools-{}", std::process::id()));
            fs::create_dir_all(&dir).expect("scratch dir");
            let path = dir.join("pools.toml");
            fs::write(
                &path,
                "# no pools
",
            )
            .expect("empty pools file");
            path
        })
        .clone()
    }

    /// A scratch stand-in for `~/.tactus`, so tests never touch the real one.
    ///
    /// A *sibling* of the repo, never a directory inside it. That is not
    /// tidiness: §14's rollback is `git clean -fd`, which deletes untracked
    /// directories — a private root inside the workspace would have its
    /// transcripts and verdicts destroyed by the first failed attempt. The
    /// same reasoning is why production puts it under the user's home.
    fn private_root_for(repo: &Path) -> PathBuf {
        let name = repo
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "run".to_owned());
        repo.with_file_name(format!("{name}-home"))
    }

    /// Resume options matching [`options`], for the same reasons.
    fn resume_options(repo: &Path, run_id: &str) -> ResumeOptions {
        let mut opts = ResumeOptions::new(run_id.to_owned(), repo.to_path_buf());
        opts.pools_path = Some(no_pools());
        opts.attempt_timeout = Duration::from_secs(60);
        opts.defer_backoff = Duration::ZERO;
        opts.wait_on_block = Some(Duration::ZERO);
        opts.private_root = Some(private_root_for(repo));
        opts
    }

    /// The paths a test's run wrote to.
    fn paths_of(repo: &Path, run_id: &str) -> RunPaths {
        RunPaths::with_private_root(repo, run_id, &private_root_for(repo))
    }

    fn committed(report: &RunReport, id: &str) -> bool {
        report
            .tasks
            .iter()
            .any(|t| t.id == id && matches!(t.status, TaskRunStatus::Committed { .. }))
    }

    fn task<'a>(report: &'a RunReport, id: &str) -> &'a TaskReport {
        report
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("no task `{id}` in {report:?}"))
    }

    // ---- step 1-6 behaviour, unchanged by the ladder ----------------------

    #[test]
    fn happy_path_commits_one_commit_per_task() {
        let repo = temp_engine_repo("happy");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run succeeds");

        assert_eq!(report.outcome(), RunOutcome::Complete);
        assert_eq!(report.tasks.len(), 2);
        assert!(
            report
                .tasks
                .iter()
                .all(|t| matches!(t.status, TaskRunStatus::Committed { .. })),
            "report: {report:?}"
        );
        // Per task: implementer 0.01 + reviewer 0.05 (§11.2 reviews every
        // attempt), so both spends are accounted for.
        assert!(
            (report.total_cost_usd - 0.12).abs() < 1e-9,
            "worker and reviewer spend both counted: {}",
            report.total_cost_usd
        );

        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert!(branch.trim().starts_with("tactus/run-"), "on run branch");
        let count = git_in(&repo, &["rev-list", "--count", "main..HEAD"]);
        assert_eq!(count.trim(), "2", "one commit per task");
        let log = git_in(&repo, &["log", "--format=%s", "main..HEAD"]);
        assert!(
            log.contains("[tactus] t1: Implement the widget"),
            "log: {log}"
        );
        assert!(log.contains("[tactus] t2: Document the widget"));
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "clean tree after run"
        );
        assert!(
            repo.join(".tactus").join("runs").exists(),
            "run dir written"
        );
    }

    #[test]
    fn dirty_tree_is_refused() {
        let repo = temp_engine_repo("dirty");
        fs::write(repo.join("stray.txt"), "uncommitted\n").expect("stray");
        let source = fake(Effect::EditFile);
        let err = run_with(&options(&repo), &source).expect_err("must refuse");
        assert!(err.to_string().contains("not clean"), "got: {err}");
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch.trim(), "main", "no run branch created");
    }

    #[test]
    fn passing_configured_gates_commit_and_are_reported() {
        let repo = temp_engine_repo("gatepass");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\n",
            Some("[[gates]]\nname = \"version\"\ncmd = \"git --version\"\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
        assert_eq!(report.gates, ["version"]);
        assert!(report.gates_from_config);
        assert!(report.render().contains("gates: version [from config]"));
    }

    #[test]
    fn unresolvable_gate_refuses_at_preflight() {
        let repo = temp_engine_repo("gateresolve");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\n",
            Some("[[gates]]\nname = \"ghost\"\ncmd = \"definitely-not-a-real-tool-xyz build\"\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let err = run_with(&opts, &source).expect_err("must refuse");
        assert!(err.to_string().contains("not found on PATH"), "got: {err}");
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch.trim(), "main", "refused before branching");
    }

    #[test]
    fn test_task_without_test_code_fails_provenance() {
        let repo = temp_engine_repo("provenance");
        seed(
            &repo,
            "## Test the widget\n<!-- tactus: id=tt depends= -->\nAdd coverage.\n",
            // One rung, one attempt: the provenance failure is what is under
            // test, not the ladder's reaction to it.
            Some("[routing]\ntest = { chain = [\"small\"], attempts_per = 1 }\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("engine ok");
        let reason = &task(&report, "tt").attempts[0]
            .failure
            .as_ref()
            .expect("provenance should fail")
            .reason;
        assert!(reason.contains("provenance"), "reason: {reason}");
    }

    #[test]
    fn test_task_adding_real_tests_passes_provenance() {
        let repo = temp_engine_repo("provenance-ok");
        seed(
            &repo,
            "## Test the widget\n<!-- tactus: id=tt depends= -->\n",
            None,
        );

        let source = fake(Effect::EditTest);
        let report = run_with(&options(&repo), &source).expect("engine ok");
        assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
        assert!(committed(&report, "tt"));
    }

    #[test]
    fn gate_residue_is_scrubbed_not_committed() {
        let repo = temp_engine_repo("residue");
        // A gate that creates a file: residue must never reach a commit nor
        // survive the task.
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\n",
            Some("[[gates]]\nname = \"leaky\"\ncmd = \"echo residue> residue.txt\"\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
        assert!(!repo.join("residue.txt").exists(), "residue scrubbed");
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "clean tree after run"
        );
        let log = git_in(&repo, &["log", "--name-only", "--format=", "main..HEAD"]);
        assert!(!log.contains("residue.txt"), "log: {log}");
    }

    #[test]
    fn the_reviewer_is_read_only_and_bound_to_the_review_tier() {
        let repo = temp_engine_repo("reviewbinding");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let settings = paths_of(&repo, &report.run_id).settings();
        let allow_list = |file: &str| -> Vec<String> {
            let text = fs::read_to_string(settings.join(file)).expect("settings written");
            let value: serde_json::Value = serde_json::from_str(&text).expect("json");
            value["permissions"]["allow"]
                .as_array()
                .expect("allow list")
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_owned())
                .collect()
        };

        let reviewer = allow_list("00-t1-1-review.json");
        assert_eq!(reviewer, ["Read", "Glob", "Grep"], "read-only, no shell");

        let implementer = allow_list("00-t1-1.json");
        assert!(
            implementer.contains(&"Edit".to_owned()),
            "implementer can edit"
        );

        // §15 split: the file describing an agent's own sandbox is not
        // somewhere that agent can read.
        assert!(
            !settings.starts_with(&repo),
            "settings live outside the workspace: {}",
            settings.display()
        );
    }

    #[test]
    fn review_can_be_switched_off_explicitly() {
        let repo = temp_engine_repo("noreview");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 depends= -->\n",
            Some("[routing]\nreview = { enabled = false }\n"),
        );

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // A reviewer that would REJECT everything: if review still ran, the
        // task would never commit.
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "report: {report:?}");
        assert!(report.tasks[0].review_models.is_empty());
        assert!(report.tasks[0].review_cost_usd.is_none());
    }

    #[test]
    fn reviewer_spend_is_attributed_separately() {
        let repo = temp_engine_repo("reviewcost");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let t1 = task(&report, "t1");
        assert_eq!(t1.cost_usd, Some(0.01), "implementer's own spend");
        assert_eq!(t1.review_cost_usd, Some(0.05), "reviewer's, kept apart");
        assert_eq!(t1.review_models, ["claude-opus-5"]);
        assert!((t1.total_cost_usd().expect("both") - 0.06).abs() < 1e-9);
        let rendered = report.render();
        assert!(rendered.contains("+ review claude-opus-5"), "{rendered}");
    }

    // ---- step 9: cross-vendor review (§11.3) -----------------------------

    /// A plan whose one task runs at frontier and touches `src/auth/**`, so
    /// both step-9 mechanisms are in play: its implementer binds to the same
    /// model as the reviewer, and its paths can match a `second_opinion`
    /// override.
    const FRONTIER_AUTH_PLAN: &str = "## Rotate the signing key\n\
         <!-- tactus: id=t1 kind=implement depends= tier=frontier paths=src/auth/** -->\n\
         Rotate it.\n";

    const SECOND_OPINION_CONFIG: &str = "[routing]\n\
         implement = { chain = [\"frontier\"], attempts_per = 1 }\n\n\
         [[routing.overrides]]\n\
         paths = [\"src/auth/**\"]\n\
         second_opinion = \"different-vendor\"\n";

    /// Same task, no override — the implicit anti-self-review path.
    const FRONTIER_ONLY_CONFIG: &str =
        "[routing]\nimplement = { chain = [\"frontier\"], attempts_per = 1 }\n";

    fn cross_vendor_opts(repo: &Path) -> RunOptions {
        let mut opts = options(repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts
    }

    #[test]
    fn a_second_opinion_runs_a_second_family_and_leaves_the_primary_alone() {
        // §11.3: both verdicts must pass. And the primary must NOT rebind here
        // even though it matches the implementer — rebinding would resolve both
        // passes to copilot/gpt-5.3-codex and drop the Anthropic review entirely, which
        // is worse than the self-review the rebind exists to prevent.
        let repo = temp_engine_repo("secondopinion");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
        let source = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Pass],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

        assert!(committed(&report, "t1"), "both passed: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(
            t1.review_models,
            ["claude-opus-5", "gpt-5.3-codex"],
            "one pass per family, primary first"
        );
        assert_eq!(t1.model, "claude-opus-5", "written by the frontier model");
        assert_eq!(source.adapter.reviews_run(), 1);
        assert_eq!(source.copilot().reviews_run(), 1);
        // Both reviewers' spend lands in the review column, not the worker's.
        assert_eq!(t1.review_cost_usd, Some(0.10), "0.05 per pass");
        assert_eq!(t1.cost_usd, Some(0.01), "implementer's own");
        let rendered = report.render();
        assert!(
            rendered.contains("+ review claude-opus-5, gpt-5.3-codex"),
            "{rendered}"
        );
    }

    #[test]
    fn a_second_opinion_that_fails_fails_the_attempt() {
        // The point of two passes: the one that says no decides, even when the
        // first already approved.
        let repo = temp_engine_repo("secondopinionfail");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
        let source = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Pass],
            vec![ReviewBehavior::Fail],
        );
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

        assert!(!committed(&report, "t1"), "a rejected change cannot commit");
        let t1 = task(&report, "t1");
        let last = t1.attempts.last().expect("an attempt ran");
        assert_eq!(
            last.failure.as_ref().map(|f| f.kind),
            Some(FailureKind::ReviewFailed)
        );
        assert_eq!(
            last.reviews.iter().map(|r| r.outcome).collect::<Vec<_>>(),
            [
                events::ReviewPassOutcome::Passed,
                events::ReviewPassOutcome::Failed
            ],
            "the record says which pass objected, and that it really judged"
        );
        assert_eq!(last.reviews[1].agent, "copilot");
    }

    #[test]
    fn a_failing_first_pass_never_spends_the_second_reviewer() {
        // Passes short-circuit like gates do (§11.1): once one has said no, a
        // second opinion on the same diff changes nothing and costs a frontier
        // invocation to learn it.
        let repo = temp_engine_repo("shortcircuit");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
        let source = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Fail],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

        assert!(!committed(&report, "t1"));
        assert_eq!(source.adapter.reviews_run(), 1);
        assert_eq!(
            source.copilot().reviews_run(),
            0,
            "the second vendor was never asked"
        );
        let last = task(&report, "t1").attempts.last().expect("attempt");
        assert_eq!(last.reviews.len(), 1, "only what actually ran is recorded");
    }

    #[test]
    fn a_frontier_task_is_not_reviewed_by_the_model_that_wrote_it() {
        // The item carried since step 6: both binders resolve `frontier`
        // identically, so without the rebind the reviewer IS the implementer.
        let repo = temp_engine_repo("selfreview");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(FRONTIER_ONLY_CONFIG));
        // The claude adapter's review script says FAIL and the copilot one says
        // PASS, so a committed task proves which of them was actually asked.
        let source = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Fail],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

        assert!(committed(&report, "t1"), "{report:?}");
        assert_eq!(task(&report, "t1").review_models, ["gpt-5.3-codex"]);
        assert_eq!(source.adapter.reviews_run(), 0, "never judged its own work");
        assert_eq!(source.copilot().reviews_run(), 1);
    }

    #[test]
    fn a_lower_rung_keeps_the_frontier_reviewer() {
        // A mid-tier implementer judged by the frontier reviewer is already a
        // genuine second look, so nothing rebinds. Triggering on family
        // similarity instead of exact identity would send most of a run
        // cross-vendor for no verification gain.
        let repo = temp_engine_repo("noneedtorebind");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"mid\"], attempts_per = 1 }\n"),
        );
        let source = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Pass],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

        assert_eq!(task(&report, "t1").model, "claude-sonnet-5");
        assert_eq!(task(&report, "t1").review_models, ["claude-opus-5"]);
        assert_eq!(source.copilot().reviews_run(), 0);
    }

    #[test]
    fn a_configured_second_opinion_with_no_second_family_refuses_before_spending() {
        // Step-6 finding #10's posture: the operator asked for two model
        // families on their blast-radius paths. Quietly giving them one is the
        // failure that finding exists to prevent, so this refuses instead.
        let repo = temp_engine_repo("nosecondfamily");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
        let error = run_with(&cross_vendor_opts(&repo), &source)
            .expect_err("a promised reviewer that cannot exist must stop the run");
        let message = error.to_string();
        assert!(message.contains("t1"), "names the task: {message}");
        assert!(
            message.contains("src/auth/**"),
            "names the override: {message}"
        );
        assert!(
            message.contains("second opinion"),
            "says what is missing: {message}"
        );
        assert_eq!(source.adapter.runs().len(), 0, "nothing was spent");
    }

    #[test]
    fn without_a_second_vendor_self_review_warns_rather_than_refusing() {
        // The implicit rebind is tactus's own idea, not the operator's, so a
        // single-vendor machine loses the upgrade rather than the run — but it
        // is told, because a verification property that quietly is not there is
        // exactly what step 6 objected to.
        let repo = temp_engine_repo("selfreviewwarn");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(FRONTIER_ONLY_CONFIG));
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run still works");

        assert!(committed(&report, "t1"));
        assert_eq!(task(&report, "t1").review_models, ["claude-opus-5"]);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("t1") && w.contains("also the reviewer")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn an_unprobeable_cross_family_reviewer_downgrades_instead_of_halting() {
        // Installed but broken is different from absent, and the two probe
        // classes have to agree about which is which: the opportunistic
        // reviewer only warns.
        let repo = temp_engine_repo("brokencopilot");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(FRONTIER_ONLY_CONFIG));
        let source = FakeSource {
            adapter: FakeAdapter::new(vec![Effect::EditFile], vec![ReviewBehavior::Pass]),
            copilot: Some(FakeAdapter::copilot(vec![ReviewBehavior::Pass]).broken("not logged in")),
        };
        let report = run_with(&cross_vendor_opts(&repo), &source)
            .expect("a broken upgrade is not a broken run");

        assert!(committed(&report, "t1"));
        assert_eq!(
            task(&report, "t1").review_models,
            ["claude-opus-5"],
            "fell back to same-model review"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("not logged in") && w.contains("same-model review")),
            "warnings: {:?}",
            report.warnings
        );
        // And it names the tasks. Resolution cannot reach this warning — a
        // shipped binary always has the Copilot adapter, so the only way the
        // rebind really goes missing is a probe failure, and a warning that
        // never fires for a real user is not a warning.
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("t1") && w.contains("also the reviewer")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn the_same_broken_reviewer_is_fatal_when_the_config_asked_for_it() {
        // Same machine, same breakage — but now a `second_opinion` names it, so
        // it is load-bearing rather than opportunistic.
        let repo = temp_engine_repo("brokenrequired");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
        let source = FakeSource {
            adapter: FakeAdapter::new(vec![Effect::EditFile], vec![ReviewBehavior::Pass]),
            copilot: Some(FakeAdapter::copilot(vec![ReviewBehavior::Pass]).broken("not logged in")),
        };
        let error = run_with(&cross_vendor_opts(&repo), &source)
            .expect_err("a required reviewer that cannot run stops the run");
        assert!(error.to_string().contains("not logged in"), "got: {error}");
    }

    #[test]
    fn the_review_budget_is_shared_between_passes_not_doubled_by_them() {
        // Step-6 finding #13 capped review at a quarter of the attempt. That
        // cap is for review, not per reviewer — otherwise configuring a second
        // opinion silently doubles a bound that was chosen deliberately.
        let attempt = Duration::from_secs(40 * 60);
        assert_eq!(review_timeout(attempt, 1), Duration::from_secs(10 * 60));
        assert_eq!(review_timeout(attempt, 2), Duration::from_secs(5 * 60));
        // The floor is per pass: a budget too small to answer in is not one.
        assert_eq!(
            review_timeout(Duration::from_secs(60), 2),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn a_resume_keeps_the_reviewers_the_run_started_with() {
        // Who judged this run is a fact about the run, not about today's
        // machine — step-8 finding #8's lesson on `private_dir`. Re-deriving it
        // would let a CLI installed since the run began become the judge for
        // the back half, leaving one run with two verification standards.
        //
        // The work left over has to be work the rebind would OTHERWISE claim,
        // or this proves nothing: the task resumed onto is at frontier, where
        // the implementer and the reviewer are the same model.
        let repo = temp_engine_repo("resumereviewers");
        seed(
            &repo,
            "## Rotate the signing key\n\
             <!-- tactus: id=t1 kind=implement depends= tier=frontier -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"frontier\"], attempts_per = 1 }\n",
            ),
        );

        // First process: no copilot on the machine, and the agent changes
        // nothing — so t1 exhausts its chain and parks, still unbuilt.
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let first = run_with(&cross_vendor_opts(&repo), &source).expect("run");
        assert!(
            matches!(task(&first, "t1").status, TaskRunStatus::Parked { .. }),
            "{first:?}"
        );

        let paths = paths_of(&repo, &first.run_id);
        let recorded = {
            let mut warnings = Vec::new();
            let events = events::read_all(&paths.events(), &mut warnings).expect("log");
            events::started_of(&events, &paths.events())
                .expect("run_started")
                .reviews
                .clone()
        };
        let recorded = recorded.expect("step 9 records who reviews");
        assert_eq!(
            recorded.alternative, None,
            "there was nothing to rebind to when this run started"
        );

        crate::answer::answer(
            &repo,
            &first.questions[0].question.id.to_string(),
            crate::answer::Reply::Text("put the key in src/auth/keys.rs".to_owned()),
        )
        .expect("answer");

        // Second process: copilot has appeared since. The record still rules,
        // so the retry is judged by the model the run started with.
        let later = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Pass],
            vec![ReviewBehavior::Pass],
        );
        let resumed =
            resume_with(&resume_options(&repo, &first.run_id), &later).expect("resume continues");

        assert!(committed(&resumed, "t1"), "{resumed:?}");
        assert_eq!(
            task(&resumed, "t1").review_models,
            ["claude-opus-5"],
            "the recorded reviewer judged the resumed attempt"
        );
        assert_eq!(
            later.copilot().reviews_run(),
            0,
            "a CLI installed since the run began must not become its judge"
        );
    }

    #[test]
    fn resume_runs_with_the_effort_policy_the_run_recorded_not_todays_config() {
        let original = "[interaction]\nmode = \"never\"\n\n\
                        [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                        [routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n";
        let (repo, run_id) = parked_run_with_config("resumeeffort", original);
        fs::write(
            repo.join("tactus.toml"),
            "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
             [routing.effort]\nimplementation = \"low\"\nreview = \"high\"\n",
        )
        .expect("edit effort only");

        let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        let logged = events_of(&repo, &run_id);
        let resumed_worker = logged
            .iter()
            .filter_map(|event| match &event.body {
                EventBody::AttemptStarted { data, .. } => Some(data),
                _ => None,
            })
            .next_back()
            .expect("resumed worker start");
        assert_eq!(resumed_worker.effort, Some(Effort::XHigh));
        let resumed_reviews = logged
            .iter()
            .filter_map(|event| match &event.body {
                EventBody::AttemptFinished { data, .. } if !data.reviews.is_empty() => {
                    Some(&data.reviews)
                }
                _ => None,
            })
            .next_back()
            .expect("resumed review records");
        assert!(
            resumed_reviews
                .iter()
                .all(|review| review.effort == Some(Effort::Max)),
            "every review pass keeps max: {resumed_reviews:?}"
        );
        let warning = resumed
            .warnings
            .iter()
            .find(|warning| warning.contains("today's effort policy"))
            .unwrap_or_else(|| panic!("no effort difference warning: {:?}", resumed.warnings));
        assert!(warning.contains("implementation small=low"), "{warning}");
        assert!(warning.contains("implementation small=xhigh"), "{warning}");
        assert!(warning.contains("review=max"), "{warning}");
        assert!(warning.contains("Start a new run"), "{warning}");
    }

    #[test]
    fn resume_restores_the_recorded_worker_binding_before_preflight() {
        let original = "[interaction]\nmode = \"never\"\n\n\
                        [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n";
        let (repo, run_id) = parked_run_with_config("resumebinding", original);
        fs::write(
            repo.join("tactus.toml"),
            "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
             [[pins]]\ntier = \"small\"\nagent = \"copilot\"\nmodel = \"gpt-5-mini\"\n",
        )
        .expect("edit only the binding");

        // `resume_answering` exposes only the Claude fake. If pre-flight probes
        // today's Copilot pin before restoring the record, this refuses before
        // the behavioral assertions below can run.
        let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        let logged = events_of(&repo, &run_id);
        let worker = logged
            .iter()
            .filter_map(|event| match &event.body {
                EventBody::AttemptStarted { data, .. } => Some(data),
                _ => None,
            })
            .next_back()
            .expect("resumed worker");
        assert_eq!(worker.agent, "claude-code");
        assert_eq!(worker.model, "claude-haiku-4-5");
        assert_eq!(
            worker.selection_origin,
            Some(events::SelectionOrigin::Auto),
            "the recorded absence of a pin is part of the snapshot too"
        );
        assert!(
            resumed
                .warnings
                .iter()
                .any(|warning| warning.contains("today's worker bindings")
                    && warning.contains("gpt-5-mini")
                    && warning.contains("claude-haiku-4-5")),
            "binding difference warning: {:?}",
            resumed.warnings
        );
    }

    #[test]
    fn the_resume_that_rederives_an_old_logs_effort_records_it_for_the_next_one() {
        let original = "[interaction]\nmode = \"never\"\n\n\
                        [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                        [routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n";
        let (repo, run_id) = parked_run_with_config("oldlogeffort", original);
        let paths = paths_of(&repo, &run_id);
        rewrite_run_started_as_schema_one(&paths, &["effort_policy"]);

        fs::write(
            repo.join("tactus.toml"),
            "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
             [routing.effort]\nimplementation = \"high\"\nreview = \"xhigh\"\n",
        )
        .expect("first derived policy");
        let first = resume_answering(&repo, &run_id, Effect::NoEdit);
        assert_eq!(first.outcome(), RunOutcome::Parked, "{first:?}");
        assert!(
            first
                .warnings
                .iter()
                .any(|warning| warning.contains("predates the effort-policy record")),
            "legacy warning: {:?}",
            first.warnings
        );
        let established = ResolvedEffortPolicy {
            small: Effort::High,
            mid: Effort::High,
            frontier: Effort::High,
            review: Effort::XHigh,
        };
        assert_eq!(
            events::recorded_effort_policy(&events_of(&repo, &run_id)),
            Some(established),
            "the first resume writes down what it derived"
        );
        let after_first = events_of(&repo, &run_id);
        assert!(events::recorded_chains(&after_first).is_some());
        assert_eq!(
            after_first
                .iter()
                .filter(|event| matches!(event.body, EventBody::RunSchemaUpgraded { .. }))
                .count(),
            1,
            "the first schema-2 resume appends one downgrade barrier"
        );

        fs::write(
            repo.join("tactus.toml"),
            "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
             [routing.effort]\nimplementation = \"low\"\nreview = \"medium\"\n",
        )
        .expect("later policy");
        let second = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(second.outcome(), RunOutcome::Complete, "{second:?}");
        let logged = events_of(&repo, &run_id);
        let worker = logged
            .iter()
            .filter_map(|event| match &event.body {
                EventBody::AttemptStarted { data, .. } => Some(data),
                _ => None,
            })
            .next_back()
            .expect("second resumed worker");
        assert_eq!(worker.effort, Some(Effort::High));
        let reviews = logged
            .iter()
            .filter_map(|event| match &event.body {
                EventBody::AttemptFinished { data, .. } if !data.reviews.is_empty() => {
                    Some(&data.reviews)
                }
                _ => None,
            })
            .next_back()
            .expect("second resumed reviews");
        assert!(
            reviews
                .iter()
                .all(|review| review.effort == Some(Effort::XHigh)),
            "reviews retain the established legacy policy: {reviews:?}"
        );
        assert!(
            second
                .warnings
                .iter()
                .any(|warning| warning.contains("today's effort policy")),
            "the later edit is reported: {:?}",
            second.warnings
        );
        assert!(
            !second
                .warnings
                .iter()
                .any(|warning| warning.contains("predates the effort-policy record")),
            "the legacy absence was established once: {:?}",
            second.warnings
        );
        assert_eq!(
            events_of(&repo, &run_id)
                .iter()
                .filter(|event| matches!(event.body, EventBody::RunSchemaUpgraded { .. }))
                .count(),
            1,
            "later resumes must not append duplicate schema transitions"
        );
    }

    #[test]
    fn a_resume_whose_effort_policy_did_not_move_says_nothing_about_it() {
        let config = "[interaction]\nmode = \"never\"\n\n\
                      [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                      [routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n";
        let (repo, run_id) = parked_run_with_config("effortunmoved", config);
        let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(
            !resumed
                .warnings
                .iter()
                .any(|warning| warning.contains("effort policy")
                    || warning.contains("effort-policy")),
            "an unchanged policy must be silent: {:?}",
            resumed.warnings
        );
    }

    #[test]
    fn a_log_written_before_step_9_still_gets_reviewed_on_resume() {
        // `RunStarted.reviews` is #[serde(default)] so a step-8 log still
        // parses — but the default is an EMPTY plan, which every later reader
        // cannot tell apart from `review = { enabled = false }`.
        let repo = temp_engine_repo("oldlogresume");
        seed(
            &repo,
            "## Rotate the signing key\n\
             <!-- tactus: id=t1 kind=implement depends= tier=frontier -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"frontier\"], attempts_per = 1 }\n",
            ),
        );
        let first_source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let first = run_with(&cross_vendor_opts(&repo), &first_source).expect("run");

        // Rewrite run_started as a pre-step-9 process would have written it.
        let paths = paths_of(&repo, &first.run_id);
        strip_run_started_field(&paths, "reviews");

        crate::answer::answer(
            &repo,
            &first.questions[0].question.id.to_string(),
            crate::answer::Reply::Text("put the key in src/auth/keys.rs".to_owned()),
        )
        .expect("answer");

        // A reviewer that rejects everything: if review still runs, nothing can
        // commit. If the absent field read as "review disabled", it commits —
        // verification gone without a word, which is step-6 finding #10.
        let later = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
        let resumed =
            resume_with(&resume_options(&repo, &first.run_id), &later).expect("resume continues");
        assert!(
            !committed(&resumed, "t1"),
            "an older log must not silently switch review off: {resumed:?}"
        );
    }

    #[test]
    fn an_unavailable_reviewer_is_recorded_as_such_not_as_a_rejection() {
        // Step-6 finding #8's distinction, carried into the ledger: a judge
        // that never ran said nothing about the code, and recording it as a
        // plain "did not pass" puts a rejection against a model that never read
        // the diff.
        let repo = temp_engine_repo("outagerecord");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
        let source = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Pass],
            vec![ReviewBehavior::RateLimited, ReviewBehavior::Pass],
        );
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

        let first = &task(&report, "t1").attempts[0];
        assert_eq!(
            first.reviews.iter().map(|r| r.outcome).collect::<Vec<_>>(),
            [
                events::ReviewPassOutcome::Passed,
                events::ReviewPassOutcome::Unavailable
            ],
            "the second vendor was down, not unimpressed"
        );
        // And the ladder treated it as an outage: deferred, then committed.
        assert!(committed(&report, "t1"), "{report:?}");
    }

    #[test]
    fn a_total_missing_an_unreported_reviewer_is_marked_rather_than_implied() {
        // The Copilot route bills nothing back (§13), so a two-pass review
        // shows one reviewer's spend. Presenting that as the total is exactly
        // what `render_ledger` says is worse than no ledger at all.
        let repo = temp_engine_repo("partialcost");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
        let source = FakeSource {
            adapter: FakeAdapter::new(vec![Effect::EditFile], vec![ReviewBehavior::Pass]),
            copilot: Some(FakeAdapter::copilot(vec![ReviewBehavior::Pass]).unpriced()),
        };
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");

        let t1 = task(&report, "t1");
        assert_eq!(t1.review_cost_usd, Some(0.05), "only what was reported");
        assert!(t1.review_cost_incomplete, "and it is not the whole story");
        assert!(
            report.render().contains("$0.0500?"),
            "the summary marks it: {}",
            report.render()
        );
        let ledger = report.render_ledger();
        assert!(ledger.contains("$0.0500?"), "{ledger}");
        assert!(
            ledger.contains("reports no spend"),
            "legend present: {ledger}"
        );
    }

    #[test]
    fn every_model_that_judged_a_task_is_listed_beside_the_cost_of_all_of_them() {
        // An escalated task can be judged on one rung by one model and on the
        // next by another. `review_cost_usd` sums every attempt, so a list
        // scoped to the final attempt would read as though it explained a total
        // it does not cover.
        let repo = temp_engine_repo("reviewtrail");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n",
            ),
        );
        // Mid fails review, escalates to frontier, which passes. The frontier
        // rung is self-review, so its pass rebinds to the other family.
        let source = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Fail, ReviewBehavior::Pass],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2, "escalated: {t1:?}");
        assert_eq!(
            t1.review_models,
            ["claude-opus-5", "gpt-5.3-codex"],
            "both judges, in the order they judged"
        );
    }

    #[test]
    fn each_pass_writes_its_own_verdict_transcript() {
        // Two reviewers, two records. The acceptance pass keeps the bare name
        // it has had since step 6, so a run directory reads the same way
        // whether or not a second opinion was configured.
        let repo = temp_engine_repo("passtranscripts");
        seed(&repo, FRONTIER_AUTH_PLAN, Some(SECOND_OPINION_CONFIG));
        let source = cross_vendor(
            vec![Effect::EditFile],
            vec![ReviewBehavior::Pass],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&cross_vendor_opts(&repo), &source).expect("run");
        let reviews = paths_of(&repo, &report.run_id).reviews();
        assert!(reviews.join("00-t1-1-review.json").is_file());
        assert!(
            reviews.join("00-t1-1-second-opinion-review.json").is_file(),
            "the second verdict cannot overwrite the first"
        );
    }

    #[test]
    fn the_run_record_survives_completion() {
        let repo = temp_engine_repo("record");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let report_path = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("report.json");
        let text = fs::read_to_string(&report_path).expect("report.json written");
        let restored: RunReport = serde_json::from_str(&text).expect("report round-trips");
        assert_eq!(restored.tasks.len(), 2);
        assert_eq!(restored.branch, report.branch);
        assert!(matches!(
            restored.tasks[0].status,
            TaskRunStatus::Committed { .. }
        ));
        assert_eq!(
            restored.tasks[0].attempts.len(),
            1,
            "the per-attempt ledger persists too"
        );
    }

    #[test]
    fn forward_dependencies_run_in_topo_order_not_plan_order() {
        let repo = temp_engine_repo("topo");
        seed(
            &repo,
            "## Second by dependency\n<!-- tactus: id=late depends=early -->\n\n\
             ## First by dependency\n<!-- tactus: id=early depends= -->\n",
            None,
        );

        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let ids: Vec<&str> = report.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["early", "late"], "dependency beats document order");
    }

    #[test]
    fn a_contradictory_pass_fails_closed() {
        let failure = review_failure(review::ReviewResult::Judged(crate::ir::Verdict {
            pass: true,
            reasons: vec!["looks fine".to_owned()],
            required_changes: vec!["parameterize the SQL".to_owned()],
            needs_human: false,
        }))
        .expect("a pass that still demands changes cannot commit");
        assert_eq!(failure.kind, FailureKind::ReviewFailed);
        assert!(
            failure.reason.contains("parameterize the SQL"),
            "{}",
            failure.reason
        );

        // A clean pass still passes.
        assert!(
            review_failure(review::ReviewResult::Judged(crate::ir::Verdict {
                pass: true,
                reasons: vec!["meets the criteria".to_owned()],
                required_changes: Vec::new(),
                needs_human: false,
            }))
            .is_none()
        );
    }

    #[test]
    fn an_unavailable_reviewer_is_not_a_rejection() {
        // A rate-limited or hung judge must not read as "your code is wrong",
        // or the ladder retries the implementer for an outage.
        let failure = review_failure(review::ReviewResult::Unavailable {
            status: OutcomeStatus::RateLimited,
            detail: "5-hour limit reached".to_owned(),
        })
        .expect("still fails the attempt");
        assert_eq!(failure.kind, FailureKind::RateLimited);
        assert_eq!(failure.origin, FailureOrigin::Reviewer);
        assert!(failure.is_outage(), "defers instead of blaming the worker");
        assert!(failure.reason.contains("reviewer unavailable"));

        let failure = review_failure(review::ReviewResult::Unavailable {
            status: OutcomeStatus::Timeout,
            detail: String::new(),
        })
        .expect("still fails");
        assert_eq!(failure.kind, FailureKind::Timeout);
        assert_eq!(failure.origin, FailureOrigin::Reviewer);

        let failure = review_failure(review::ReviewResult::Unavailable {
            status: OutcomeStatus::AgentError,
            detail: "spawn failed".to_owned(),
        })
        .expect("still fails");
        assert_eq!(failure.kind, FailureKind::ReviewUnavailable);
    }

    #[test]
    fn required_changes_reach_the_retry_as_a_clean_list() {
        let failure = review_failure(review::ReviewResult::Judged(crate::ir::Verdict {
            pass: false,
            reasons: vec!["incomplete".to_owned()],
            required_changes: vec![
                "handle the empty-input case".to_owned(),
                "add a round-trip test".to_owned(),
            ],
            needs_human: false,
        }))
        .expect("fails");
        assert_eq!(
            failure.feedback.as_deref(),
            Some("- handle the empty-input case\n- add a round-trip test"),
            "every item bulleted, including the first"
        );
        assert_eq!(
            failure.origin,
            FailureOrigin::Worker,
            "a rejected diff is the worker's to fix"
        );
    }

    #[test]
    fn prompt_names_the_allowed_gate_commands() {
        let task = Task {
            id: TaskId::from("t1"),
            kind: TaskKind::Implement,
            title: "Do the thing".to_owned(),
            body: String::new(),
            depends_on: Vec::new(),
            acceptance: Vec::new(),
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::new(),
        };
        let run_dir = std::env::temp_dir().join(format!("tactus-prompt-{}", std::process::id()));
        fs::create_dir_all(run_dir.join("artifacts")).expect("run dir");
        let prompt = materialize_prompt(
            &task,
            &["cargo check --all-targets".to_owned()],
            &run_dir,
            None,
        );
        assert!(prompt.contains("EXACTLY these commands"));
        assert!(prompt.contains("- cargo check --all-targets"));
        assert!(
            prompt.contains(QUESTION_MARKER),
            "the worker is told how to ask (§12)"
        );
        let bare = materialize_prompt(&task, &[], &run_dir, None);
        assert!(!bare.contains("EXACTLY these commands"));
    }

    #[test]
    fn prompt_wires_artifacts_to_real_files() {
        let run_dir = std::env::temp_dir().join(format!("tactus-artifact-{}", std::process::id()));
        let _ = fs::remove_dir_all(&run_dir);
        fs::create_dir_all(run_dir.join("artifacts")).expect("run dir");
        let mut task = Task {
            id: TaskId::from("t1"),
            kind: TaskKind::Implement,
            title: "Build it".to_owned(),
            body: String::new(),
            depends_on: Vec::new(),
            acceptance: Vec::new(),
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: vec![crate::ir::ArtifactId::from("api-contract")],
            artifacts_out: vec![crate::ir::ArtifactId::from("notes")],
        };

        // Missing input: say so plainly rather than pointing at nothing.
        let prompt = materialize_prompt(&task, &[], &run_dir, None);
        assert!(
            prompt.contains("did \n     not leave one") || prompt.contains("did not leave one")
        );
        assert!(
            prompt.contains("write artifact `notes`"),
            "producer told where to write"
        );

        // Present input: content is inlined.
        fs::write(
            artifact_path(&run_dir, "api-contract"),
            "cursor = base64(offset)",
        )
        .expect("artifact");
        let prompt = materialize_prompt(&task, &[], &run_dir, None);
        assert!(
            prompt.contains("cursor = base64(offset)"),
            "content inlined"
        );

        task.artifacts_in.clear();
        task.artifacts_out.clear();
        let bare = materialize_prompt(&task, &[], &run_dir, None);
        assert!(!bare.contains("artifact"));
    }

    // ---- step 7: the ladder in the engine ---------------------------------

    #[test]
    fn a_gate_failure_recovers_on_the_same_rung_via_session_resume() {
        // §21 definition-of-done (b). The gate demands a file only the second
        // attempt writes, so recovery is real rather than scripted around.
        let repo = temp_engine_repo("resume");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n\n\
                 [[gates]]\nname = \"needs-test\"\ncmd = \"git ls-files --error-unmatch \
                 widget_test.rs\"\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::EditFile, Effect::EditTest],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");

        assert!(committed(&report, "t1"), "report: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2, "one retry, not an escalation");
        assert_eq!(t1.attempts[0].tier, "small");
        assert_eq!(t1.attempts[1].tier, "small", "same rung");
        assert!(!t1.attempts[0].resumed);
        assert!(t1.attempts[1].resumed, "§11.4 retries in-session");

        let runs = source.adapter.runs();
        assert_eq!(runs[0].resume, None);
        assert_eq!(
            runs[1].resume.as_deref(),
            Some("s0"),
            "the retry resumed the failed attempt's session"
        );
        assert!(
            runs[1].prompt.contains("gate `needs-test` failed"),
            "the gate's own words go back: {}",
            runs[1].prompt
        );
        assert!(
            !runs[1].prompt.contains("# Task:"),
            "a resumed session already holds the task; the prompt stays terse"
        );

        // §14: a resumed retry keeps the tree, so the commit carries BOTH
        // attempts' work rather than only the last one's.
        let files = git_in(&repo, &["show", "--name-only", "--format=", "HEAD"]);
        assert!(files.contains("agent-output.txt"), "files: {files}");
        assert!(files.contains("widget_test.rs"), "files: {files}");
    }

    #[test]
    fn exhausting_a_rung_escalates_with_a_fresh_session_and_the_history() {
        // §21 definition-of-done (c).
        let repo = temp_engine_repo("escalate");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");

        assert!(committed(&report, "t1"), "report: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2);
        assert_eq!(t1.attempts[0].tier, "small");
        assert_eq!(t1.attempts[0].model, "claude-haiku-4-5");
        assert_eq!(t1.attempts[1].tier, "mid", "one rung up");
        assert_eq!(t1.attempts[1].model, "claude-sonnet-5");
        assert!(
            !t1.attempts[1].resumed,
            "§11.4: a new rung is a new session — a different model cannot \
             inherit another's conversation"
        );
        assert_eq!(t1.trail(), "small failed → mid ok");

        let runs = source.adapter.runs();
        // The adapter's own record, not just the report echoing what the
        // engine intended: the second attempt really was dispatched to the
        // higher rung's model.
        assert_eq!(runs[0].model, "claude-haiku-4-5");
        assert_eq!(runs[1].model, "claude-sonnet-5");
        assert_eq!(runs[1].resume, None, "fresh session");
        assert!(
            runs[1].prompt.contains("# Task:"),
            "a fresh worker gets the whole task again"
        );
        assert!(
            runs[1].prompt.contains("diff is empty"),
            "and what the previous rung got wrong: {}",
            runs[1].prompt
        );
    }

    #[test]
    fn a_parked_question_does_not_stop_the_runnable_frontier() {
        // §21 definition-of-done (d) and invariant 6: t1 exhausts its chain
        // and parks; the independent t3 must still commit.
        let repo = temp_engine_repo("park");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // t1 fails; every later attempt (t3's) edits and passes.
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");

        let TaskRunStatus::Parked { question, .. } = &task(&report, "t1").status else {
            panic!("t1 should park on a question: {report:?}");
        };
        assert!(committed(&report, "t3"), "independent work kept going");
        assert!(
            matches!(&task(&report, "t2").status, TaskRunStatus::Blocked { by } if by == "t1"),
            "a dependent of a parked task is blocked, not failed"
        );
        assert!(report.halted_at.is_none(), "parking never halts a run");
        assert_eq!(report.outcome(), RunOutcome::Parked);
        assert_eq!(report.parked_tasks(), ["t1"]);

        // The question is on disk where a notifier, `tactus answer`, or a UI
        // can read it — that file is the contract, not the terminal output.
        let path = repo
            .join(".tactus")
            .join("runs")
            .join(&report.run_id)
            .join("questions")
            .join(format!("{question}.json"));
        let record: QuestionRecord =
            serde_json::from_str(&fs::read_to_string(&path).expect("question file"))
                .expect("parses");
        assert_eq!(record.question.kind, QuestionKind::Unblock);
        assert_eq!(record.question.affected_tasks, [TaskId::from("t1")]);
        assert!(record.answer.is_none(), "still open");
        assert!(record.question.context.contains("Doomed"));

        let rendered = report.render();
        assert!(rendered.contains("PARKED"), "{rendered}");
        assert!(rendered.contains("open questions (1)"), "{rendered}");
    }

    #[test]
    fn answering_the_question_retries_the_task_with_the_operators_words() {
        let repo = temp_engine_repo("answered");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let answers = ScriptedAnswers::new(vec![Answer::Answered {
            text: "the widget lives in src/widget.rs — write it there".to_owned(),
        }]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        assert!(committed(&report, "t1"), "report: {report:?}");
        assert_eq!(report.outcome(), RunOutcome::Complete);
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2, "the answer bought a fresh allowance");
        assert_eq!(
            t1.attempts[1].tier, "small",
            "an answer does not move the rung — the chain was already spent"
        );

        let runs = source.adapter.runs();
        assert!(
            runs[1].prompt.contains("src/widget.rs"),
            "the operator's answer reaches the agent: {}",
            runs[1].prompt
        );
        assert!(
            runs[1].prompt.contains("instruction from a person"),
            "and is labelled as an instruction, not quoted data"
        );

        let record = report.questions.first().expect("one question");
        assert!(
            matches!(&record.answer, Some(Answer::Answered { text }) if text.contains("widget.rs")),
            "the answer is recorded against the question: {record:?}"
        );
    }

    #[test]
    fn declining_fails_the_task_and_halt_is_the_default() {
        let repo = temp_engine_repo("declined");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let answers = ScriptedAnswers::new(vec![Answer::Declined]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        let TaskRunStatus::Failed { kind, reason } = &task(&report, "t1").status else {
            panic!("a declined question fails its task: {report:?}");
        };
        assert_eq!(*kind, FailureKind::Declined);
        assert!(!reason.is_empty());
        assert_eq!(
            report.halted_at.as_deref(),
            Some("t1"),
            "§17's default on_task_failure is halt"
        );
        assert_eq!(report.outcome(), RunOutcome::Halted);
    }

    #[test]
    fn on_task_failure_continue_keeps_independent_work_moving() {
        let repo = temp_engine_repo("continue");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some(
                "[engine]\non_task_failure = \"continue\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let answers = ScriptedAnswers::new(vec![Answer::Declined]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        assert!(report.halted_at.is_none(), "configured to continue");
        assert!(matches!(
            task(&report, "t1").status,
            TaskRunStatus::Failed { .. }
        ));
        assert!(committed(&report, "t3"));
        assert!(
            matches!(&task(&report, "t2").status, TaskRunStatus::Blocked { by } if by == "t1"),
            "§19: dependents of a failed task are blocked"
        );
    }

    #[test]
    fn a_rate_limit_defers_without_spending_an_attempt() {
        let repo = temp_engine_repo("ratelimit");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            // A single attempt on a single rung: if the rate limit spent it,
            // the task could never commit.
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::RateLimited, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let sleeper = RecordingSleeper::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: Some(&sleeper),
            },
        )
        .expect("run");

        assert!(
            committed(&report, "t1"),
            "the rate limit cost no attempt: {report:?}"
        );
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2);
        assert_eq!(
            t1.attempts[0].failure.as_ref().map(|f| f.kind),
            Some(FailureKind::RateLimited)
        );
        assert_eq!(t1.attempts[1].tier, "small", "never escalated for a pool");
        assert_eq!(
            sleeper.waits().len(),
            1,
            "waited once, because deferred work was all that was left"
        );
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "a deferred task hands back a clean tree — another task may run next"
        );
    }

    #[test]
    fn a_pool_that_never_returns_ends_at_the_human_rung() {
        let repo = temp_engine_repo("ratelimit-forever");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.max_defers = 2;
        let source = source(vec![Effect::RateLimited], vec![ReviewBehavior::Pass]);
        let sleeper = RecordingSleeper::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: Some(&sleeper),
            },
        )
        .expect("run");

        assert!(
            matches!(task(&report, "t1").status, TaskRunStatus::Parked { .. }),
            "an exhausted pool becomes a question, not an infinite retry: {report:?}"
        );
        assert_eq!(
            task(&report, "t1").attempts.len(),
            3,
            "two deferrals, then the attempt that gave up"
        );
        assert!(
            task(&report, "t1")
                .attempts
                .iter()
                .all(|a| a.tier == "small"),
            "a busy pool never pushes the task up-tier"
        );
        assert_eq!(sleeper.waits().len(), 2, "one wait per deferral");
    }

    #[test]
    fn an_unavailable_reviewer_defers_the_task_instead_of_escalating_it() {
        let repo = temp_engine_repo("reviewdown");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::EditFile],
            vec![ReviewBehavior::RateLimited, ReviewBehavior::Pass],
        );
        let sleeper = RecordingSleeper::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: Some(&sleeper),
            },
        )
        .expect("run");

        assert!(committed(&report, "t1"), "report: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(t1.attempts.len(), 2);
        assert_eq!(
            t1.attempts[0].failure.as_ref().map(|f| f.origin),
            Some(FailureOrigin::Reviewer),
            "the outage is attributed to the judge"
        );
        assert_eq!(
            t1.attempts[1].tier, "small",
            "the implementer was never escalated for the reviewer being down"
        );
    }

    #[test]
    fn a_reviewer_asking_for_a_human_parks_without_spending_the_chain() {
        let repo = temp_engine_repo("needshuman");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::NeedsHuman]);
        let report = run_with(&opts, &source).expect("run");

        let t1 = task(&report, "t1");
        assert!(
            matches!(t1.status, TaskRunStatus::Parked { .. }),
            "report: {report:?}"
        );
        assert_eq!(
            t1.attempts.len(),
            1,
            "the reviewer declined to judge, so nothing was retried or escalated"
        );
        assert_eq!(
            t1.attempts[0].failure.as_ref().map(|f| f.kind),
            Some(FailureKind::NeedsHuman)
        );
        let record = report.questions.first().expect("question raised");
        assert_eq!(record.question.kind, QuestionKind::Clarify);
        assert!(
            record
                .question
                .context
                .contains("contradict the API contract"),
            "the reviewer's reason reaches the person: {}",
            record.question.context
        );
        assert!(
            record.question.context.contains("not instructions to you"),
            "agent-authored text is labelled as data"
        );
    }

    #[test]
    fn a_worker_can_stop_and_ask_rather_than_guess() {
        let repo = temp_engine_repo("workerasks");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::AskQuestion, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let answers = ScriptedAnswers::new(vec![Answer::Answered {
            text: "opaque cursors".to_owned(),
        }]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        let record = report.questions.first().expect("the worker's question");
        assert_eq!(record.question.kind, QuestionKind::Clarify);
        assert!(
            record.question.context.contains("opaque or signed"),
            "context: {}",
            record.question.context
        );
        // Independent work ran while t1 waited, and the answer resumed it.
        assert!(committed(&report, "t3"));
        assert!(committed(&report, "t1"), "report: {report:?}");
        let t1 = task(&report, "t1");
        assert_eq!(
            t1.attempts.len(),
            2,
            "asking cost no attempt — only the retry after the answer"
        );
        // Parking rolled the tree back, so the session's account of what it
        // wrote no longer matches the repository (§14 pairs resume with tree
        // retention). The retry therefore starts fresh and carries the whole
        // task again, with the operator's answer as an instruction.
        assert!(
            !t1.attempts[1].resumed,
            "a parked task never resumes into a tree that was reverted underneath it"
        );
        // Invocation order across the whole run, not just this task: t1 asks
        // (0), the independent t3 proceeds while t1 is parked (1), then t1
        // retries once the answer arrives (2). That interleaving is the point
        // of invariant 6, so the retry is the third invocation.
        let runs = source.adapter.runs();
        let retry = &runs[2];
        assert_eq!(retry.resume, None, "fresh session, not --resume");
        assert!(
            retry.prompt.contains("# Task:"),
            "the whole task is re-sent, since the session no longer carries it: {}",
            retry.prompt
        );
        assert!(
            retry.prompt.contains("opaque cursors"),
            "and the operator's answer travels with it: {}",
            retry.prompt
        );
        assert!(
            retry.prompt.contains("instruction from a person"),
            "labelled as an instruction rather than quoted as data"
        );
    }

    #[test]
    fn ci_mode_parks_rather_than_failing_and_says_so() {
        // §12: `interaction = "never"` degrades questions to parked-task
        // reporting, and the outcome is distinguishable from both a clean run
        // and a halt.
        let repo = temp_engine_repo("ci");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Depends on the doomed one\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");

        assert_eq!(report.outcome(), RunOutcome::Parked);
        assert!(report.halted_at.is_none(), "parked is not halted");
        assert!(matches!(
            task(&report, "t1").status,
            TaskRunStatus::Parked { .. }
        ));
        assert!(committed(&report, "t3"));
        assert!(matches!(
            task(&report, "t2").status,
            TaskRunStatus::Blocked { .. }
        ));
        assert!(
            report.questions.iter().all(QuestionRecord::is_open),
            "nothing answered it, and nothing pretended to"
        );
    }

    #[test]
    fn an_unanswerable_question_is_never_asked_twice() {
        // Without this the hard block spins: ask, get nothing, ask again.
        let repo = temp_engine_repo("noloop");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let answers = CountingAnswers::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run terminates");
        assert_eq!(report.outcome(), RunOutcome::Parked);
        assert_eq!(
            answers.count(),
            1,
            "asked once; an unreachable channel is not retried"
        );
    }

    #[derive(Default)]
    struct CountingAnswers {
        calls: Mutex<usize>,
    }

    impl CountingAnswers {
        fn count(&self) -> usize {
            self.calls.lock().map(|c| *c).unwrap_or(0)
        }
    }

    impl AnswerSource for CountingAnswers {
        fn id(&self) -> &'static str {
            "counting"
        }

        fn resolve(&self, _question: &Question) -> Result<Answer, TactusError> {
            if let Ok(mut calls) = self.calls.lock() {
                *calls += 1;
            }
            Ok(Answer::Unanswered)
        }
    }

    #[test]
    fn agent_errors_and_empty_diffs_carry_feedback_the_retry_can_use() {
        let repo = temp_engine_repo("feedback");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::Error, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");
        assert!(committed(&report, "t1"), "report: {report:?}");
        let runs = source.adapter.runs();
        assert!(
            runs[1].prompt.contains("fake adapter error detail"),
            "the adapter's own diagnosis reaches the retry: {}",
            runs[1].prompt
        );
    }

    #[test]
    fn an_unparseable_reviewer_fails_after_one_reask() {
        let repo = temp_engine_repo("reviewprose");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Unparseable]);
        let report = run_with(&opts, &source).expect("engine ok");

        let failure = task(&report, "t1").attempts[0]
            .failure
            .as_ref()
            .expect("a reviewer that never answers cannot pass a task");
        assert_eq!(failure.kind, FailureKind::ReviewFailed);
        assert!(
            failure.reason.contains("re-ask"),
            "reason: {}",
            failure.reason
        );
        // The re-ask actually happened, and both sides are on record.
        let reviews = paths_of(&repo, &report.run_id).reviews();
        assert!(reviews.join("00-t1-1-review.json").is_file());
        assert!(
            reviews.join("00-t1-1-review-reask.json").is_file(),
            "one re-ask before giving up (§11.2)"
        );
        assert_eq!(
            git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
            "0",
            "nothing commits without a passing verdict"
        );
    }

    #[test]
    fn gate_logs_are_named_by_the_collision_free_stem() {
        let repo = temp_engine_repo("gatelogs");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [[gates]]\nname = \"never\"\ncmd = \"git frobnicate-not-a-command\"\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("engine ok");

        let failure = task(&report, "t1").attempts[0]
            .failure
            .as_ref()
            .expect("gate should fail");
        assert_eq!(failure.kind, FailureKind::GateFailed);
        let gates_dir = paths_of(&repo, &report.run_id).gates();
        assert!(
            gates_dir.join("00-t1-1-never.log").is_file(),
            "the log stem matches the task's other artifacts, so two ids that \
             sanitize alike cannot overwrite each other"
        );
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "rolled back"
        );
    }

    #[test]
    fn the_trail_summarizes_the_ladder() {
        let report = TaskReport {
            id: "t1".to_owned(),
            title: "x".to_owned(),
            model: "m".to_owned(),
            status: TaskRunStatus::Skipped,
            duration: Duration::ZERO,
            cost_usd: None,
            review_models: Vec::new(),
            review_cost_usd: None,
            review_cost_incomplete: false,
            session_id: None,
            attempts: vec![
                attempt_record(1, "small", true),
                attempt_record(2, "small", true),
                attempt_record(3, "mid", false),
            ],
        };
        assert_eq!(report.trail(), "small×2 failed → mid ok");
    }

    fn attempt_record(attempt: u32, tier: &str, failed: bool) -> AttemptRecord {
        AttemptRecord {
            attempt,
            tier: tier.to_owned(),
            model: "m".to_owned(),
            pool: None,
            resumed: false,
            duration: Duration::ZERO,
            cost_usd: None,
            reviews: Vec::new(),
            session_id: None,
            usage: None,
            failure: failed.then(|| FailureRecord {
                kind: FailureKind::GateFailed,
                origin: FailureOrigin::Worker,
                reason: "no".to_owned(),
            }),
        }
    }

    #[test]
    fn a_worker_question_is_read_from_the_marker_onward() {
        assert_eq!(
            worker_question(Some("Did some work.\nTACTUS-QUESTION: opaque or signed?")).as_deref(),
            Some("opaque or signed?")
        );
        // Multi-line questions survive, because the prompt asks for it last.
        assert_eq!(
            worker_question(Some("TACTUS-QUESTION: which store?\nRedis or Postgres?")).as_deref(),
            Some("which store?\nRedis or Postgres?")
        );
        assert_eq!(worker_question(Some("TACTUS-QUESTION:   ")), None);
        assert_eq!(worker_question(Some("no marker here")), None);
        assert_eq!(worker_question(None), None);
    }

    #[test]
    fn an_echoed_marker_does_not_swallow_the_real_question() {
        // The engine hands the agent this marker in every fresh prompt, and
        // the empty-diff feedback names it verbatim — so an agent mentioning
        // it before asking is the expected shape, not a corner case. Taking
        // the first occurrence would hand the operator the agent's reasoning
        // with the question buried at the end.
        let reply = "The retry feedback says I can use the TACTUS-QUESTION: marker if I am \
                     blocked. I considered whether this needs one.\n\n\
                     TACTUS-QUESTION: should cursors be opaque or signed?";
        assert_eq!(
            worker_question(Some(reply)).as_deref(),
            Some("should cursors be opaque or signed?"),
            "last marker wins, matching the prompt and review.rs's verdict rule"
        );
    }

    #[test]
    fn an_outage_is_never_reclassified_as_a_question() {
        // `detail` carries the agent's partial output on every failure path,
        // and that output routinely quotes the prompt. Reading the marker
        // before the status would turn a rate limit into a parked question —
        // silently defeating "RateLimited defers rather than burning an
        // attempt", and losing the timeout's transcript-tail feedback.
        let quoting = "I will end with the TACTUS-QUESTION: marker if I get stuck.";
        let output = crate::agent::ProcessOutput {
            stdout: String::new(),
            stderr: String::new(),
            code: Some(1),
            timed_out: false,
            duration: Duration::ZERO,
        };
        for (status, expected) in [
            (OutcomeStatus::RateLimited, FailureKind::RateLimited),
            (OutcomeStatus::Timeout, FailureKind::Timeout),
            (OutcomeStatus::AgentError, FailureKind::AgentError),
        ] {
            let outcome =
                fake_outcome(status, Some(quoting.to_owned()), "s0", None, Duration::ZERO);
            let failure = evaluate_outcome(&outcome, &output).expect("still a failure");
            assert_eq!(failure.kind, expected, "{status:?} must keep its own kind");
        }

        // A genuine question on a completed run still parks the task.
        let mut asked = fake_outcome(
            OutcomeStatus::Completed,
            Some("TACTUS-QUESTION: opaque or signed?".to_owned()),
            "s0",
            None,
            Duration::ZERO,
        );
        asked.diff = "diff --git a/x b/x\n+x\n".to_owned();
        assert_eq!(
            evaluate_outcome(&asked, &output).expect("parks").kind,
            FailureKind::NeedsHuman
        );
    }

    #[test]
    fn a_halted_run_stops_asking_and_keeps_naming_the_real_cause() {
        // t1 parks on a question, t2 fails terminally under the default halt
        // policy. Asking about t1 afterwards spends the operator's attention
        // on an answer no attempt can consume, and a decline would relabel
        // `halted_at` with t1 — sending triage at the wrong task.
        let repo = temp_engine_repo("haltpark");
        seed(
            &repo,
            "## Asks a question\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Exhausts its chain\n<!-- tactus: id=t2 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // t1 asks and parks; t2 changes nothing and parks on chain exhaustion.
        let source = source(
            vec![Effect::AskQuestion, Effect::NoEdit],
            vec![ReviewBehavior::Pass],
        );
        // Declining t1 fails it, which halts the run under the default policy.
        // The second answer must never be consumed: t2's question cannot be
        // asked once nothing can act on the reply.
        let answers = ScriptedAnswers::new(vec![
            Answer::Declined,
            Answer::Answered {
                text: "this answer must never be used".to_owned(),
            },
        ]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        assert!(
            matches!(
                task(&report, "t1").status,
                TaskRunStatus::Failed {
                    kind: FailureKind::Declined,
                    ..
                }
            ),
            "the decline is what halts the run: {report:?}"
        );
        assert_eq!(
            report.halted_at.as_deref(),
            Some("t1"),
            "halted_at names the task that actually caused the halt"
        );
        // The distinguishing assertion. Unguarded, t2's question would be
        // asked, answered, and flipped back to Pending — where `next_ready`
        // refuses it because the run has halted, so it would surface as
        // `Skipped` with the operator's answer sitting unused on disk.
        assert!(
            matches!(task(&report, "t2").status, TaskRunStatus::Parked { .. }),
            "t2 is never asked after the halt, so it stays parked rather than \
             silently consuming an answer: {report:?}"
        );
        let t2_question = report
            .questions
            .iter()
            .find(|q| q.question.affected_tasks.iter().any(|t| t.as_str() == "t2"))
            .expect("t2 raised a question");
        assert!(
            t2_question.is_open(),
            "left open on disk for a later resume (§15)"
        );
    }

    #[test]
    fn unreported_cost_stays_unreported_rather_than_zero() {
        assert_eq!(sum_opt([None, None].into_iter()), None);
        assert_eq!(
            sum_opt([Some(0.01), None, Some(0.02)].into_iter()),
            Some(0.03)
        );
    }

    // ---- step 8: the event log is the state ------------------------------

    /// Fold a run's log the way `status` and `resume` do.
    fn replay_of(repo: &Path, run_id: &str) -> crate::status::RunStatus {
        crate::status::load(repo, Some(run_id)).expect("the run reads back")
    }

    /// One path through the ladder, for the live-equals-replay property.
    struct Scenario {
        name: &'static str,
        config: &'static str,
        /// Overrides the default two-task plan where a scenario needs path
        /// hints or a particular tier.
        plan: Option<&'static str>,
        effects: Vec<Effect>,
        reviews: Vec<ReviewBehavior>,
        /// `Some` puts a second vendor on the machine (§11.3).
        second_opinion: Option<Vec<ReviewBehavior>>,
        answers: Vec<Answer>,
    }

    impl Scenario {
        fn new(name: &'static str, config: &'static str, effects: Vec<Effect>) -> Self {
            Self {
                name,
                config,
                plan: None,
                effects,
                reviews: vec![ReviewBehavior::Pass],
                second_opinion: None,
                answers: Vec::new(),
            }
        }

        fn reviewed(mut self, reviews: Vec<ReviewBehavior>) -> Self {
            self.reviews = reviews;
            self
        }

        fn cross_vendor(mut self, plan: &'static str, second: Vec<ReviewBehavior>) -> Self {
            self.plan = Some(plan);
            self.second_opinion = Some(second);
            self
        }

        fn answered(mut self, answers: Vec<Answer>) -> Self {
            self.answers = answers;
            self
        }
    }

    /// The property the whole design rests on: a live run and a replay of its
    /// own log are the same computation, not two that happen to agree.
    ///
    /// Asserted on `RunState` rather than on the report, because the report is
    /// a lossy projection — it drops `feedback`, `resume_next`, `session`, and
    /// the rung a task is standing on, which are exactly the fields a resume
    /// depends on being right.
    fn assert_live_equals_replay(repo: &Path, live: &RunState, report: &RunReport) {
        let replayed = replay_of(repo, &report.run_id);
        assert_eq!(
            &replayed.state, live,
            "replaying the log produced different state than the run that wrote it"
        );
        // Warnings are the one field deliberately excluded. They are
        // diagnostics of the *process* — what this invocation noticed about a
        // missing notifier or a discarded working tree — not facts about the
        // run, so a later reader legitimately has different ones. Anything
        // that genuinely belongs to the run is an event instead (a discarded
        // tree, for instance, rides on `run_resumed`).
        let strip = |report: &RunReport| {
            let mut value = serde_json::to_value(report).expect("serialize");
            if let Some(object) = value.as_object_mut() {
                object.remove("warnings");
            }
            value
        };
        assert_eq!(
            strip(&replayed.report()),
            strip(report),
            "the report derived from the log differs from the one the run wrote"
        );
    }

    #[test]
    fn live_state_equals_replayed_state_across_every_ladder_path() {
        // One scenario per branch the engine can take, so the equality is
        // exercised against commits, retries, escalations, deferrals, parks,
        // answers, and a halt — not just the happy path.
        let scenarios = vec![
            Scenario::new(
                "commit",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
                vec![Effect::EditFile],
            ),
            Scenario::new(
                "retry",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n",
                vec![Effect::NoEdit, Effect::EditFile],
            ),
            Scenario::new(
                "escalate",
                "[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n",
                vec![Effect::NoEdit, Effect::EditFile],
            ),
            Scenario::new(
                "defer",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
                vec![Effect::RateLimited, Effect::EditFile],
            ),
            Scenario::new(
                "park-then-answer",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
                vec![Effect::NoEdit, Effect::EditFile],
            )
            .answered(vec![Answer::Answered {
                text: "the widget lives in src/widget.rs".to_owned(),
            }]),
            Scenario::new(
                "decline-and-halt",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
                vec![Effect::NoEdit],
            )
            .answered(vec![Answer::Declined]),
            Scenario::new(
                "reviewer-asks-for-a-human",
                "[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n",
                vec![Effect::EditFile],
            )
            .reviewed(vec![ReviewBehavior::NeedsHuman]),
            // §11.3: two review passes per attempt, so `AttemptRecord.reviews`
            // carries more than one entry through serialize → deserialize. The
            // list replaced a scalar pair in step 9; this is what proves the
            // new shape survives the wire.
            Scenario::new(
                "second-opinion-passes",
                SECOND_OPINION_CONFIG,
                vec![Effect::EditFile],
            )
            .cross_vendor(FRONTIER_AUTH_PLAN, vec![ReviewBehavior::Pass]),
            // And the same with the second reviewer rejecting, so a `false`
            // verdict on a non-final pass replays too.
            Scenario::new(
                "second-opinion-rejects",
                SECOND_OPINION_CONFIG,
                vec![Effect::EditFile],
            )
            .cross_vendor(FRONTIER_AUTH_PLAN, vec![ReviewBehavior::Fail])
            .answered(vec![Answer::Declined]),
            // The anti-self-review rebind: the acceptance pass runs on a model
            // no chain rung names, so the record has to carry the binding
            // rather than let a replay re-derive it.
            Scenario::new(
                "self-review-rebind",
                FRONTIER_ONLY_CONFIG,
                vec![Effect::EditFile],
            )
            .cross_vendor(FRONTIER_AUTH_PLAN, vec![ReviewBehavior::Pass]),
            // Step 10's two new branches. `budget_exceeded` folds into a
            // run-level field and `capacity_snapshot` folds into nothing —
            // opposite shapes, and both have to come back the same on replay.
            Scenario::new(
                "budget-stop",
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [budgets]\nrun_usd = 0.05\n",
                vec![Effect::EditFile],
            ),
            // And the ApproveSpend park, whose fold depends on the escalation
            // having landed *before* the park — the ordering D3 turns on.
            Scenario::new(
                "approve-spend",
                "[routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.005 }\n",
                vec![Effect::NoEdit, Effect::EditFile],
            )
            .answered(vec![Answer::Answered {
                text: "approve: run the escalated attempt".to_owned(),
            }]),
        ];

        for Scenario {
            name,
            config,
            plan,
            effects,
            reviews,
            second_opinion,
            answers,
        } in scenarios
        {
            let repo = temp_engine_repo(&format!("replay-{name}"));
            seed(
                &repo,
                plan.unwrap_or(
                    "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
                     ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
                ),
                Some(config),
            );
            let mut opts = options(&repo);
            opts.config_path = Some(repo.join("tactus.toml"));
            let cross_vendor_scenario = second_opinion.is_some();
            let source = match second_opinion {
                Some(second) => cross_vendor(effects, reviews, second),
                None => source(effects, reviews),
            };
            let scripted = ScriptedAnswers::new(answers);
            let (report, live) = run_harness_inner(
                &opts,
                &Harness {
                    adapters: &source,
                    answers: Some(&scripted),
                    sleeper: None,
                },
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            // A cross-vendor scenario that quietly resolved to one pass would
            // still replay identically and prove nothing about the shape this
            // step introduced. Check the run did what the scenario claims
            // before trusting the equality below.
            if cross_vendor_scenario {
                let judged: Vec<&str> = report
                    .tasks
                    .iter()
                    .flat_map(|t| &t.attempts)
                    .flat_map(|a| &a.reviews)
                    .map(|r| r.agent.as_str())
                    .collect();
                assert!(
                    judged.contains(&"copilot"),
                    "{name}: the second vendor never judged anything, so this scenario \
                     exercises nothing new: {judged:?}"
                );
            }
            assert_live_equals_replay(&repo, &live, &report);
        }
    }

    #[test]
    fn an_aborting_error_still_leaves_a_replayable_log() {
        // The engine dying between the agent's edits and a verdict is §19's
        // "engine crash" row. Nothing gets to write a tidy ending, so the log
        // has to be enough on its own.
        let repo = temp_engine_repo("abortlog");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // Run it through, then rewind the log by hand: reproducing a real
        // abort at exactly this point needs a failure the fake adapter cannot
        // raise, and the on-disk shape is what this test is actually about.
        let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
        opts.attempt_timeout = Duration::from_secs(60);
        let report = run_with(&opts, &source).expect("the run itself succeeds");

        // Now truncate the log to the moment before the attempt reported, the
        // exact on-disk shape a kill leaves, and confirm it still folds.
        let paths = paths_of(&repo, &report.run_id);
        let text = fs::read_to_string(paths.events()).expect("log");
        let lines: Vec<&str> = text.lines().collect();
        let cut = lines
            .iter()
            .position(|line| line.contains("\"attempt_finished\""))
            .expect("the run recorded an attempt");
        fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");

        let replayed = replay_of(&repo, &report.run_id);
        assert_eq!(replayed.interrupted, 1, "the dangling attempt is settled");
        assert!(
            replayed.interrupted_run(),
            "and the run reads as interrupted rather than finished"
        );
        assert_eq!(replayed.state.states[0], TaskState::Pending);
    }

    #[test]
    fn a_run_that_has_spent_nothing_totals_positive_zero() {
        // Observed as `total: $-0.0000` in the ledger of a run whose first
        // attempt was still in flight. This first assertion is the diagnosis,
        // kept because it is the whole reason `total_of` exists: if std ever
        // folds from `+0.0`, the helper can go.
        let nothing: [f64; 0] = [];
        assert!(
            nothing.iter().sum::<f64>().is_sign_negative(),
            "`sum` no longer folds from -0.0, so `total_of` is obsolete"
        );

        assert!(!total_of(&[]).is_sign_negative(), "a spent-nothing total");
        assert_eq!(format!("${:.4}", total_of(&[])), "$0.0000");

        // And the fold change cannot have moved a real total: `+0.0` preserves
        // every value a cost can be.
        let spent = vec![
            task_report_costing(Some(0.25), Some(1.5)),
            task_report_costing(None, None),
            task_report_costing(Some(0.0), None),
        ];
        assert!((total_of(&spent) - 1.75).abs() < f64::EPSILON);
    }

    /// A report carrying nothing but the two cost columns.
    fn task_report_costing(worker: Option<f64>, review: Option<f64>) -> TaskReport {
        TaskReport {
            id: "t".to_owned(),
            title: String::new(),
            model: String::new(),
            status: TaskRunStatus::Skipped,
            duration: Duration::ZERO,
            cost_usd: worker,
            review_models: Vec::new(),
            review_cost_usd: review,
            review_cost_incomplete: false,
            session_id: None,
            attempts: Vec::new(),
        }
    }

    #[test]
    fn a_live_run_reads_as_running_rather_than_halted() {
        // The settlement above, inverted. A run an engine is still driving has
        // a dangling attempt at every instant, exactly like a killed one — so
        // settling unconditionally reports a working attempt as a failure and
        // the whole run as halted. `status` is the only window into a run that
        // holds its own terminal, and a window that lies is worse than none.
        let repo = temp_engine_repo("livestatus");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Waits on the widget\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
        let paths = paths_of(&repo, &report.run_id);

        // Rewind to mid-attempt: the shape a live engine's log has the whole
        // time it is working, not only the shape a kill leaves behind.
        let text = fs::read_to_string(paths.events()).expect("log");
        let lines: Vec<&str> = text.lines().collect();
        let cut = lines
            .iter()
            .position(|line| line.contains("\"attempt_finished\""))
            .expect("an attempt");
        fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");

        // With nothing holding the run, that shape still means interrupted —
        // and `t2` really is blocked, because on an ended run a dependency that
        // never finished never will.
        let stopped = replay_of(&repo, &report.run_id);
        assert!(stopped.interrupted_run());
        let out = crate::status::render(&stopped);
        assert!(out.contains("skipped (run interrupted)"), "{out}");
        assert!(out.contains("t2: blocked by `t1`"), "{out}");

        // Now hold the lock the way a working engine does — through the same
        // `RunLock` a run takes, not a hand-rolled `flock` on the same path.
        // Which primitive holds a run is `rundir`'s to decide, and a test that
        // reaches around it is testing a lock nothing else uses.
        let lock = RunLock::acquire(&paths.public).expect("simulate a live engine");

        let live = replay_of(&repo, &report.run_id);
        assert!(live.running, "a held lock means an engine is driving this");
        assert_eq!(
            live.interrupted, 0,
            "an attempt in flight has not been interrupted"
        );
        let out = crate::status::render(&live);
        assert!(out.contains("t1: running now"), "{out}");
        // The one the dependency-free pair could not catch: `t2` is waiting on
        // a task that is working, which is what `Queued` means. Reading that as
        // `Blocked` tells the operator a dependency failed when it is running.
        assert!(out.contains("t2: queued"), "{out}");
        assert!(out.contains("t3: queued"), "{out}");
        assert!(out.contains("run in progress"), "{out}");
        for lie in [
            "small failed",
            "skipped (run halted)",
            "skipped (run interrupted)",
            "run complete",
            "run interrupted",
            "blocked by",
        ] {
            assert!(!out.contains(lie), "a live run reported `{lie}`:\n{out}");
        }
        drop(lock);
    }

    #[test]
    fn a_truncated_run_resumes_without_spending_the_interrupted_attempt() {
        // Decision 3, end to end: the attempt shows up in the ledger, the
        // rung's allowance does not, and the task completes on the retry.
        let repo = temp_engine_repo("resumetrunc");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            // One attempt on one rung: if the interrupted attempt had been
            // counted, the task could never commit.
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        let run_id = report.run_id.clone();
        let paths = paths_of(&repo, &run_id);

        // Rewind the record to mid-attempt and put the tree back the way a
        // dead agent would have left it.
        let text = fs::read_to_string(paths.events()).expect("log");
        let lines: Vec<&str> = text.lines().collect();
        let cut = lines
            .iter()
            .position(|line| line.contains("\"attempt_finished\""))
            .expect("an attempt");
        fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");
        git_in(&repo, &["reset", "-q", "--hard", "HEAD~1"]);
        fs::write(repo.join("agent-output.txt"), "half-written\n").expect("residue");

        let source = fake(Effect::EditFile);
        let (resumed, state) = resume_harness_inner(
            &resume_options(&repo, &run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume");

        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t1"));

        let t1 = task(&resumed, "t1");
        assert_eq!(
            t1.attempts.len(),
            2,
            "the interrupted attempt is on the record beside the one that worked"
        );
        assert_eq!(
            t1.attempts[0].failure.as_ref().map(|f| f.kind),
            Some(FailureKind::Interrupted)
        );
        assert_eq!(
            t1.attempts[0].cost_usd, None,
            "unknown spend is reported as unknown, not as free"
        );
        assert_eq!(t1.attempts[1].tier, "small", "still on the same rung");
        assert!(
            !t1.attempts[1].resumed,
            "§14: the tree was discarded, so the session cannot be trusted"
        );

        // The residue is gone and the branch is linear.
        assert!(
            git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "crash residue discarded"
        );
        assert_eq!(
            git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
            "1",
            "one commit, not a duplicate of the interrupted attempt's work"
        );
        assert!(
            resumed
                .warnings
                .iter()
                .any(|w| w.contains("discarded") && w.contains("agent-output.txt")),
            "the operator is told what was thrown away: {:?}",
            resumed.warnings
        );
        assert_live_equals_replay(&repo, &state, &resumed);
    }

    #[test]
    fn killing_a_run_mid_attempt_leaves_a_resumable_record() {
        // The real thing: a separate process is driven into an attempt and
        // dies inside it, exactly as `kill -9` or a power cut would.
        let repo = temp_engine_repo("crashkill");
        seed(
            &repo,
            "## First\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Second\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );

        let exe = std::env::current_exe().expect("test binary");
        let status = Command::new(exe)
            .args([
                "--exact",
                "engine::tests::crash_child_dies_inside_an_attempt",
                "--ignored",
                "--test-threads",
                "1",
            ])
            .env("TACTUS_CRASH_REPO", &repo)
            .output()
            .expect("spawn the child run");
        assert_eq!(
            status.status.code(),
            Some(CRASH_EXIT_CODE),
            "the child must die inside the attempt, not finish or panic: {}",
            String::from_utf8_lossy(&status.stderr)
        );

        let run_id = rundir::latest_run(&repo).expect("the child started a run");
        let paths = paths_of(&repo, &run_id);

        // What a kill leaves: a dirty tree and an attempt that never reported.
        assert!(
            !git_in(&repo, &["status", "--porcelain"]).trim().is_empty(),
            "the dead agent's edits are still in the tree"
        );
        let log = fs::read_to_string(paths.events()).expect("log");
        let last = log.lines().last().expect("events");
        assert!(
            last.contains("\"attempt_started\"") && last.contains("\"t2\""),
            "the log ends mid-attempt: {last}"
        );
        assert!(
            !log.contains("\"run_finished\""),
            "a killed run never records an ending"
        );

        let before = replay_of(&repo, &run_id);
        assert!(before.interrupted_run(), "status calls it interrupted");
        assert_eq!(before.interrupted, 1);
        assert!(
            crate::status::render(&before).contains(&format!("tactus resume {run_id}")),
            "and tells the operator how to continue it"
        );

        // The lock died with the process, so nothing has to be cleared by hand.
        assert!(
            !rundir::is_running(&paths.public),
            "the OS released the lock"
        );

        // And the summary line says what happened rather than claiming an
        // outcome. A killed run replays into `Complete` — nothing halted it, no
        // budget stopped it, nothing is parked — so the ledger used to be
        // followed by `run complete: 1 task(s) committed` and then, one line
        // later, `state: interrupted`. Two adjacent lines contradicting each
        // other about a run that died mid-attempt with work left undone.
        let rendered = crate::status::render(&before);
        assert!(
            rendered.contains("run interrupted: 1 task(s) committed so far"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("run complete"),
            "a killed run claimed it completed:\n{rendered}"
        );
        // Its unreached tasks were not skipped because the run *halted* — that
        // is a different ending, and one an operator acts on differently.
        assert!(rendered.contains("skipped (run interrupted)"), "{rendered}");

        let source = fake(Effect::EditFile);
        let (resumed, state) = resume_harness_inner(
            &resume_options(&repo, &run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume the killed run");

        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t1"), "the work it did survived");
        assert!(
            committed(&resumed, "t2"),
            "and the work it died in got done"
        );
        assert_eq!(
            git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
            "2",
            "one commit per task, with nothing duplicated by the resume"
        );
        let t2 = task(&resumed, "t2");
        assert_eq!(
            t2.attempts[0].failure.as_ref().map(|f| f.kind),
            Some(FailureKind::Interrupted),
            "the attempt it died in is on the record: {t2:?}"
        );
        assert_live_equals_replay(&repo, &state, &resumed);
    }

    /// Spawned by `killing_a_run_mid_attempt_leaves_a_resumable_record`.
    /// Ends its own process on purpose, which is why it must never run as part
    /// of the ordinary suite.
    #[test]
    #[ignore = "spawned by killing_a_run_mid_attempt_leaves_a_resumable_record"]
    fn crash_child_dies_inside_an_attempt() {
        let Ok(repo) = std::env::var("TACTUS_CRASH_REPO") else {
            return;
        };
        let repo = PathBuf::from(repo);
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // t1 commits; the process dies inside t2's first attempt.
        let source = source(
            vec![Effect::EditFile, Effect::Exit],
            vec![ReviewBehavior::Pass],
        );
        let _ = run_with(&opts, &source);
        // Only reachable if the adapter never got a second invocation, which
        // would mean this test is not exercising what it claims to.
        std::process::exit(0);
    }

    #[test]
    fn a_parked_run_is_answered_out_of_band_and_resumed() {
        // §21's definition-of-done (d) across processes: the run ends parked,
        // a person answers with `tactus answer` while nothing is running, and
        // the resume picks the answer up.
        let repo = temp_engine_repo("answerresume");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Depends on it\n<!-- tactus: id=t2 kind=implement depends=t1 -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");

        assert_eq!(report.outcome(), RunOutcome::Parked);
        let run_id = report.run_id.clone();
        let question = report
            .questions
            .first()
            .expect("a question was raised")
            .question
            .id
            .to_string();

        // Nothing is running; the answer is written by the CLI path.
        let recorded = crate::answer::answer(
            &repo,
            &question[..8],
            crate::answer::Reply::Text("the widget lives in src/widget.rs".to_owned()),
        )
        .expect("answer by prefix");
        assert_eq!(recorded.run_id, run_id);
        assert!(!recorded.run_is_live);

        let source = fake(Effect::EditFile);
        let (resumed, state) = resume_harness_inner(
            &resume_options(&repo, &run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume");

        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t1"), "the answer un-parked it");
        assert!(committed(&resumed, "t2"), "and its dependent ran");

        // This adapter is fresh for the resume, so its first invocation is
        // t1's retry — the one the answer released. t2 runs after it.
        let runs = source.adapter.runs();
        let retry = runs.first().expect("a retry ran");
        assert!(
            retry.prompt.contains("src/widget.rs"),
            "the operator's answer reached the agent: {}",
            retry.prompt
        );
        assert!(
            retry.prompt.contains("instruction from a person"),
            "labelled as an instruction, not quoted as data"
        );
        assert_live_equals_replay(&repo, &state, &resumed);
    }

    #[test]
    fn an_answer_arriving_mid_run_unparks_without_a_hard_block() {
        // Invariant 6 at its most useful: the operator answers from elsewhere
        // while other work is still going, and the task is released on the
        // next scheduler turn rather than at the end of the run.
        let repo = temp_engine_repo("midrun");
        seed(
            &repo,
            "## Asks a question\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Independent\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::AskQuestion, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        // Nobody is reachable through the answer *channel* at all: if the
        // sweep did not exist, t1 could only ever end parked.
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&AnsweringViaFile { repo: repo.clone() }),
                sleeper: None,
            },
        )
        .expect("run");

        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
        assert!(committed(&report, "t1"), "the file answer released it");
        assert!(committed(&report, "t3"));
        let answered = report.questions.first().expect("one question");
        assert!(
            matches!(&answered.answer, Some(Answer::Answered { text }) if text.contains("opaque")),
            "the answer is recorded against the question: {answered:?}"
        );
    }

    /// Stands in for an operator running `tactus answer` in another terminal
    /// while the run is still going: it writes the file and tells the engine
    /// nobody replied, so only the sweep can find it.
    struct AnsweringViaFile {
        repo: PathBuf,
    }

    impl AnswerSource for AnsweringViaFile {
        fn id(&self) -> &'static str {
            "test-file-writer"
        }

        fn resolve(&self, question: &Question) -> Result<Answer, TactusError> {
            let _ = crate::answer::answer(
                &self.repo,
                question.id.as_str(),
                crate::answer::Reply::Text("opaque cursors".to_owned()),
            );
            Ok(Answer::Unanswered)
        }
    }

    #[test]
    fn blocking_propagates_transitively_and_against_plan_order() {
        // The chain is listed backwards on purpose: a single pass in plan
        // order would settle `late` before `mid` was known to be blocked, and
        // report it as merely skipped.
        let repo = temp_engine_repo("blocked");
        seed(
            &repo,
            "## Last\n<!-- tactus: id=late kind=implement depends=mid -->\n\n\
             ## Middle\n<!-- tactus: id=mid kind=implement depends=first -->\n\n\
             ## First\n<!-- tactus: id=first kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");

        assert!(matches!(
            task(&report, "first").status,
            TaskRunStatus::Parked { .. }
        ));
        assert!(
            matches!(&task(&report, "mid").status, TaskRunStatus::Blocked { by } if by == "first"),
            "the direct dependent is blocked: {report:?}"
        );
        assert!(
            matches!(&task(&report, "late").status, TaskRunStatus::Blocked { by } if by == "mid"),
            "and so is its dependent, naming the nearest blocker: {report:?}"
        );
    }

    #[test]
    fn answering_a_blocker_releases_the_chain_behind_it() {
        // Blocked is a *view*, not recorded state — which is what lets an
        // answer make a whole chain runnable again on resume.
        let repo = temp_engine_repo("unblock");
        seed(
            &repo,
            "## Last\n<!-- tactus: id=late kind=implement depends=mid -->\n\n\
             ## Middle\n<!-- tactus: id=mid kind=implement depends=first -->\n\n\
             ## First\n<!-- tactus: id=first kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");
        let run_id = report.run_id.clone();
        let question = report.questions[0].question.id.to_string();

        crate::answer::answer(
            &repo,
            &question,
            crate::answer::Reply::Text("write src/first.rs".to_owned()),
        )
        .expect("answer");

        let source = fake(Effect::EditFile);
        let resumed = resume_with(&resume_options(&repo, &run_id), &source).expect("resume");
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        for id in ["first", "mid", "late"] {
            assert!(committed(&resumed, id), "{id} should have run: {resumed:?}");
        }
    }

    #[test]
    fn an_exhausted_pool_and_a_silent_operator_still_terminate() {
        // The drain loop's termination argument, executed: an adapter that
        // never succeeds, a pool that never returns, and a channel nobody
        // answers. Every branch of the loop fires and the run still ends.
        let repo = temp_engine_repo("terminate");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- tactus: id=t2 kind=implement depends= -->\n\n\
             ## After one\n<!-- tactus: id=t3 kind=implement depends=t1 -->\n",
            Some("[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.max_defers = 2;
        let source = source(vec![Effect::RateLimited], vec![ReviewBehavior::Pass]);
        let answers = CountingAnswers::default();
        let sleeper = RecordingSleeper::default();
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: Some(&sleeper),
            },
        )
        .expect("the run terminates rather than spinning");

        assert_eq!(report.outcome(), RunOutcome::Parked);
        for id in ["t1", "t2"] {
            assert!(
                matches!(task(&report, id).status, TaskRunStatus::Parked { .. }),
                "{id}: {report:?}"
            );
        }
        assert!(matches!(
            task(&report, "t3").status,
            TaskRunStatus::Blocked { .. }
        ));
        assert_eq!(
            answers.count(),
            2,
            "each question is asked exactly once, however many times the loop turns"
        );
        assert!(
            !sleeper.waits().is_empty(),
            "the deferral branch really fired"
        );
        assert!(
            sleeper.waits().len() <= 8,
            "and it was bounded: {:?}",
            sleeper.waits()
        );
    }

    // ---- step 8: resume refuses rather than guessing -----------------------

    fn resume_err(repo: &Path, run_id: &str) -> String {
        let source = fake(Effect::EditFile);
        resume_with(&resume_options(repo, run_id), &source)
            .expect_err("resume must refuse")
            .to_string()
    }

    /// The base config every parked-run fixture starts from: one rung, one
    /// attempt, no interaction — so a task that cannot pass parks immediately.
    const PARKED_RUN_CONFIG: &str = "[interaction]\nmode = \"never\"\n\n\
         [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n";

    /// A run that ends parked — the resumable shape every refusal test starts
    /// from, so each one isolates exactly the thing it breaks.
    fn parked_run(tag: &str) -> (PathBuf, String) {
        parked_run_with_config(tag, PARKED_RUN_CONFIG)
    }

    /// As [`parked_run`], with the config spelled out — for the tests that need
    /// a `[[gates]]` section in the record.
    ///
    /// One recipe, not two: the chains check runs before anything gate-related,
    /// so a copy whose `[routing]` line drifted from the original would fail
    /// these tests on "routing has changed" and point at the wrong thing.
    fn parked_run_with_config(tag: &str, config: &str) -> (PathBuf, String) {
        let repo = temp_engine_repo(tag);
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(config),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Parked);
        (repo, report.run_id)
    }

    #[test]
    fn resume_refuses_when_the_branch_moved_under_it() {
        // §15's HEAD check. Something committed after the run stopped, so the
        // log no longer describes what is on the branch.
        let (repo, run_id) = parked_run("headmoved");
        fs::write(repo.join("someone-else.txt"), "a hand-made commit\n").expect("file");
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "not the engine's work"]);

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("record ends at"), "got: {err}");
        assert!(
            err.contains("Move the branch back"),
            "and says what to do: {err}"
        );
    }

    #[test]
    fn resume_refuses_when_the_frozen_plan_changed() {
        let (repo, run_id) = parked_run("planmoved");
        fs::write(
            repo.join("plan.md"),
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\nNow with a body.\n",
        )
        .expect("edit the plan");

        let err = resume_err(&repo, &run_id);
        assert!(
            err.contains("has changed since this run froze it"),
            "got: {err}"
        );
        assert!(
            err.contains("attribute work to the wrong tasks"),
            "and why it matters: {err}"
        );
    }

    #[test]
    fn resume_refuses_when_routing_moved_under_a_recorded_rung() {
        // `Progress.rung` is an index into the chain; re-resolving a different
        // chain would point it at another tier without saying so.
        let (repo, run_id) = parked_run("chainmoved");
        fs::write(
            repo.join("tactus.toml"),
            "[interaction]\nmode = \"never\"\n\n\
             [routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n",
        )
        .expect("edit config");

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("routing has changed"), "got: {err}");
        assert!(err.contains("`t1` ran on [small]"), "names the task: {err}");
    }

    /// [`parked_run`], with one `[[gates]]` entry — the resumable shape for the
    /// gate tests, which need a recorded gate to diverge from.
    fn parked_run_with_gate(tag: &str, cmd: &str) -> (PathBuf, String) {
        parked_run_with_config(tag, &gate_config(cmd))
    }

    /// [`PARKED_RUN_CONFIG`] plus a `check` gate running `cmd`.
    fn gate_config(cmd: &str) -> String {
        format!("{PARKED_RUN_CONFIG}\n[[gates]]\nname = \"check\"\ncmd = \"{cmd}\"\n")
    }

    /// Resume and answer the question the parked task is waiting on, so the
    /// task actually runs again and its gates actually execute.
    fn resume_answering(repo: &Path, run_id: &str, effect: Effect) -> RunReport {
        let source = source(vec![effect], vec![ReviewBehavior::Pass]);
        let answers = ScriptedAnswers::new(vec![Answer::Answered {
            text: "carry on".to_owned(),
        }]);
        resume_harness(
            &resume_options(repo, run_id),
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("resume")
    }

    #[test]
    fn resume_runs_the_gates_the_run_recorded_not_todays() {
        // The load-bearing test for the whole gate record, and behavioural
        // rather than textual: the recorded gate passes, today's config would
        // fail, and the task commits — which it can only do if the gate that
        // actually executed came from the log.
        //
        // This is the self-hosting hazard from the gate-config record, closed
        // at the point
        // that matters. The workspace an implementer edits contains the very
        // tactus.toml its gates come from, so an edited gate must not become
        // the standard for what follows. Refusing would also have stopped the
        // weakened gate running, but it would have stopped the *run* too, and
        // a legitimately-committed config edit would have left it unresumable.
        let (repo, run_id) = parked_run_with_gate("gaterecorded", "git --version");
        // `git` still resolves at pre-flight, so nothing refuses before the
        // gate runs — it just exits non-zero when it does.
        fs::write(
            repo.join("tactus.toml"),
            gate_config("git frobnicate-not-a-command"),
        )
        .expect("edit config");

        let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(
            committed(&resumed, "t1"),
            "the recorded gate ran, not the one in today's config: {resumed:?}"
        );
        // And the operator learns their edit did not take effect here, rather
        // than concluding the gate is broken when it never ran.
        let warning = resumed
            .warnings
            .iter()
            .find(|w| w.contains("differ from the ones this run recorded"))
            .unwrap_or_else(|| panic!("no gate-difference warning: {:?}", resumed.warnings));
        assert!(
            warning.contains(
                "`check` runs `git --version` and today's config says `git \
                              frobnicate-not-a-command`"
            ),
            "names the edit: {warning}"
        );
        assert!(
            warning.contains("Start a new run to adopt them"),
            "and what to do about it: {warning}"
        );
        // The report describes the gates that ran, not the ones on disk.
        assert_eq!(resumed.gates, ["check"]);
    }

    #[test]
    fn the_report_labels_gates_from_the_record_not_todays_config() {
        // `gates` came from the record but `gates_from_config` did not, so the
        // run's own report and a later `status` disagreed about the same list:
        // `finish()` read today's analysis while `RunReport::from_state` read
        // the record. The doc above `from_state` promises those two cannot
        // drift, and this is the one field that still let them.
        let (repo, run_id) = parked_run_with_gate("gatelabel", "git --version");
        // `[[gates]]` deleted, so today's flag would be false and today's
        // derivation empty — the temp repo has no project marker.
        fs::write(repo.join("tactus.toml"), PARKED_RUN_CONFIG).expect("edit config");

        let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(resumed.gates, ["check"], "the recorded gate ran");
        assert!(
            resumed.gates_from_config,
            "and is labelled as the record has it, not as today's config would"
        );
        // The other half of the same promise: a reader replaying the log agrees.
        let replayed = replay_of(&repo, &run_id).report();
        assert_eq!(replayed.gates, resumed.gates);
        assert_eq!(replayed.gates_from_config, resumed.gates_from_config);
    }

    #[test]
    fn a_resume_whose_gates_did_not_move_says_nothing_about_them() {
        // The success path, with a non-empty gate list — the direction a false
        // positive would break. Every other gate test edits the config, so
        // without this one an over-eager comparison (order, whitespace, a
        // re-derived timeout) would warn on every ordinary resume unnoticed.
        let (repo, run_id) = parked_run_with_gate("gateunmoved", "git --version");
        let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(
            !resumed
                .warnings
                .iter()
                .any(|w| w.contains("gates") && w.contains("recorded")),
            "an untouched config must warn about nothing: {:?}",
            resumed.warnings
        );
    }

    #[test]
    fn a_gate_difference_is_described_without_inventing_edits() {
        // `[[gates]]` does not require unique names, so the obvious by-name
        // lookup answers for the wrong entry: it reports edits nobody made,
        // and — worse — finds every name present and concludes "reordered"
        // when a gate was added. Each case here produced a false sentence
        // before the comparison paired whole gates instead of names.
        let gate = |name: &str, cmd: &str| GateSummary {
            name: name.to_owned(),
            cmd: cmd.to_owned(),
            timeout: Duration::from_secs(600),
            shell: crate::gates::ShellKind::Sh,
        };
        let check = gate("check", "cargo test");
        let only_check = std::slice::from_ref(&check);

        assert_eq!(
            gates_differ(only_check, only_check),
            None,
            "identical gates are not a difference"
        );

        // A duplicate name added. The record's `check` is present and unchanged,
        // so nothing was edited and nothing reordered — one gate appeared.
        let added = gates_differ(only_check, &[check.clone(), gate("check", "true")])
            .expect("a difference");
        assert!(
            added.contains("`check` (`true`) is in today's config and not in the record"),
            "names the added gate: {added}"
        );
        assert!(
            !added.contains("different order"),
            "and does not invent a reorder: {added}"
        );

        // One of two same-named gates removed. Pairing by name would report
        // `check` as edited from one command to the other; both are real
        // entries and neither changed.
        let removed = gates_differ(&[check.clone(), gate("check", "cargo clippy")], only_check)
            .expect("a difference");
        assert!(
            removed.contains("`check` (`cargo clippy`) is in the record and not in today's config"),
            "names the removed gate: {removed}"
        );

        // An unambiguous single-name edit still reads as one edit.
        let edited = gates_differ(only_check, &[gate("check", "true")]).expect("a difference");
        assert!(
            edited.contains("`check` runs `cargo test` and today's config says `true`"),
            "{edited}"
        );

        // A rename is two facts, and saying so beats guessing which gate the
        // operator meant to rename into which.
        let renamed =
            gates_differ(only_check, &[gate("verify", "cargo test")]).expect("a difference");
        assert!(
            renamed.contains("`check` (`cargo test`) is in the record"),
            "{renamed}"
        );
        assert!(
            renamed.contains("`verify` (`cargo test`) is in today's config"),
            "{renamed}"
        );

        // Shell and timeout are recorded because they decide what a command
        // means and how long it has to mean it — `true` always passes under sh
        // and is not a program at all under cmd.
        let reshelled = gates_differ(
            only_check,
            &[GateSummary {
                shell: crate::gates::ShellKind::Bash,
                ..check.clone()
            }],
        )
        .expect("a difference");
        assert!(
            reshelled.contains("`check` runs under `sh` and today's config says `bash`"),
            "{reshelled}"
        );

        // Same gates, different order: a difference worth a line, but not the
        // same claim as a changed command.
        let other = gate("test", "cargo test");
        let reordered =
            gates_differ(&[check.clone(), other.clone()], &[other, check]).expect("a difference");
        assert!(reordered.contains("in a different order"), "{reordered}");
        assert!(
            !reordered.contains("not in the record"),
            "nothing came or went: {reordered}"
        );
    }

    #[test]
    fn a_log_that_predates_the_gate_record_rederives_and_says_what_it_can() {
        // A v0.1 log recorded gate names and nothing else. Refusing would
        // strand every run written before the record over a field it could
        // never have carried, so resume re-derives — and uses the one thing
        // such a log *does* have. A moved name is proof the standard changed,
        // not a suspicion, and the warning says which.
        let (repo, run_id) = parked_run_with_gate("oldgatelog", "git --version");
        strip_run_started_field(&paths_of(&repo, &run_id), "gate_cmds");
        // Re-derivation must be a real re-derivation, or this test would pass
        // against a resume that ignored today's config entirely.
        fs::write(
            repo.join("tactus.toml"),
            format!(
                "{PARKED_RUN_CONFIG}\n[[gates]]\nname = \"renamed\"\ncmd = \"git --version\"\n"
            ),
        )
        .expect("edit config");

        let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        let warning = resumed
            .warnings
            .iter()
            .find(|w| w.contains("predates the gate record"))
            .unwrap_or_else(|| panic!("no warning: {:?}", resumed.warnings));
        assert!(
            warning.contains("the gate names have moved"),
            "an old log still knows this much: {warning}"
        );
        assert!(
            warning.contains("recorded [check]") && warning.contains("resolves [renamed]"),
            "and says which: {warning}"
        );
    }

    #[test]
    fn the_resume_that_rederives_an_old_logs_gates_records_them_for_the_next_one() {
        // Without this, the pre-record population never gains a record: every
        // resume re-derives, so a gate weakened between two of them is adopted
        // silently — the exact substitution the record exists to prevent,
        // surviving in the one population that could not carry it.
        //
        // Behavioural, and it takes two resumes to show: the first establishes
        // `git --version`, the gate is then weakened to something that fails,
        // and the second must still commit. It can only do that by running the
        // gate the first resume wrote down.
        let (repo, run_id) = parked_run_with_gate("oldgateestablish", "git --version");
        strip_run_started_field(&paths_of(&repo, &run_id), "gate_cmds");

        // First resume: nothing to rebuild from, so it re-derives and says so.
        // `Effect::NoEdit` leaves the task parked, so there is a second resume
        // to make.
        let first = resume_answering(&repo, &run_id, Effect::NoEdit);
        assert_eq!(first.outcome(), RunOutcome::Parked, "{first:?}");
        assert!(
            first
                .warnings
                .iter()
                .any(|w| w.contains("predates the gate record")),
            "the first resume re-derived: {:?}",
            first.warnings
        );

        // It wrote down what it settled on.
        let paths = paths_of(&repo, &run_id);
        let mut log_warnings = Vec::new();
        let logged = events::read_all(&paths.events(), &mut log_warnings).expect("log");
        let established = events::recorded_gates(&logged).expect("the resume recorded its gates");
        assert_eq!(established.len(), 1);
        assert_eq!(established[0].cmd, "git --version");

        // Now weaken the gate, exactly as an implementer editing the workspace
        // would. Under the old behaviour the second resume re-derived and
        // adopted this.
        fs::write(
            repo.join("tactus.toml"),
            gate_config("git frobnicate-not-a-command"),
        )
        .expect("edit config");

        let second = resume_answering(&repo, &run_id, Effect::EditFile);
        assert_eq!(second.outcome(), RunOutcome::Complete, "{second:?}");
        assert!(
            committed(&second, "t1"),
            "the established gate ran, not the weakened one: {second:?}"
        );
        // And it is an ordinary record-bearing resume now: it warns about the
        // difference rather than about the log's age.
        assert!(
            second
                .warnings
                .iter()
                .any(|w| w.contains("differ from the ones this run recorded")),
            "{:?}",
            second.warnings
        );
        assert!(
            !second
                .warnings
                .iter()
                .any(|w| w.contains("predates the gate record")),
            "the log is no longer speechless about its gates: {:?}",
            second.warnings
        );
    }

    #[test]
    fn an_old_gateless_log_is_not_warned_at_about_nothing() {
        // The run recorded no gates and none resolve today, so no command can
        // have hidden behind an unchanged name. A warning here would fire on
        // every gateless pre-record run, and one that cries wolf on the
        // harmless case is not read on the harmful one.
        let (repo, run_id) = parked_run("oldgatelessslog");
        strip_run_started_field(&paths_of(&repo, &run_id), "gate_cmds");

        let resumed = resume_answering(&repo, &run_id, Effect::EditFile);
        assert!(
            !resumed.warnings.iter().any(|w| w.contains("gate")),
            "nothing to say: {:?}",
            resumed.warnings
        );
    }

    #[test]
    fn resume_refuses_when_the_run_branch_is_gone() {
        let (repo, run_id) = parked_run("branchgone");
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim()
            .to_owned();
        git_in(&repo, &["switch", "-q", "main"]);
        git_in(&repo, &["branch", "-q", "-D", &branch]);

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("no longer exists"), "got: {err}");
    }

    #[test]
    fn resume_refuses_to_switch_over_uncommitted_work() {
        let (repo, run_id) = parked_run("dirtyelsewhere");
        git_in(&repo, &["switch", "-q", "main"]);
        fs::write(
            repo.join("my-own-work.txt"),
            "not the engine's to discard\n",
        )
        .expect("file");

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("Commit or stash"), "got: {err}");
        assert!(
            repo.join("my-own-work.txt").exists(),
            "a refusal must not destroy the work it refused over"
        );
    }

    #[test]
    fn resume_refuses_a_run_that_already_finished_or_halted() {
        let repo = temp_engine_repo("finished");
        let complete = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &complete).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete);
        let err = resume_err(&repo, &report.run_id);
        assert!(err.contains("already completed"), "got: {err}");

        let repo = temp_engine_repo("halted");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let answers = ScriptedAnswers::new(vec![Answer::Declined]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");
        assert_eq!(report.outcome(), RunOutcome::Halted);
        let err = resume_err(&repo, &report.run_id);
        assert!(err.contains("halted at `t1`"), "got: {err}");
    }

    #[test]
    fn resume_refuses_while_another_process_holds_the_run() {
        let (repo, run_id) = parked_run("locked");
        let paths = paths_of(&repo, &run_id);
        let _held = RunLock::acquire(&paths.public).expect("hold it");

        let err = resume_err(&repo, &run_id);
        assert!(err.contains("already driving run"), "got: {err}");
    }

    #[test]
    fn an_unknown_run_id_lists_what_is_there() {
        let (repo, _) = parked_run("unknownid");
        let err = resume_err(&repo, "01NOPE");
        assert!(err.contains("known runs"), "got: {err}");
    }

    // ---- step 8: status and the ledger -------------------------------------

    #[test]
    fn status_reports_a_live_run_and_the_ledger_reads_from_the_log() {
        let repo = temp_engine_repo("statusledger");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");

        let loaded = replay_of(&repo, &report.run_id);
        assert!(!loaded.running, "nothing holds a finished run");
        assert!(!loaded.interrupted_run());

        let rendered = crate::status::render(&loaded);
        assert!(rendered.contains("ledger:"), "{rendered}");
        assert!(rendered.contains("t1"), "{rendered}");
        assert!(
            // No pools file in these tests, so no attempt names a pool — and
            // the ledger says exactly that rather than showing a blank column
            // that reads as "nothing was spent".
            rendered.contains("per-pool drain: no pool is connected"),
            "{rendered}"
        );
        assert!(
            rendered.contains(&loaded.paths.private.display().to_string()),
            "and points at where the transcripts actually are"
        );

        // The ledger totals are the run's, derived from the log rather than
        // carried over from the process that wrote it.
        assert!(
            (loaded.report().total_cost_usd - report.total_cost_usd).abs() < 1e-9,
            "{} vs {}",
            loaded.report().total_cost_usd,
            report.total_cost_usd
        );

        // Holding the lock on a run that has already recorded its finish does
        // not make it live again. It says a process has claimed the run —
        // which is what a `resume` looks like before it writes anything — and
        // leaves the outcome above alone. A live run is covered by
        // `a_live_run_reads_as_running_rather_than_halted`, which truncates the
        // log so that the run genuinely has somewhere left to go.
        let paths = paths_of(&repo, &report.run_id);
        let _held = RunLock::acquire(&paths.public).expect("claim the finished run");
        let claimed = replay_of(&repo, &report.run_id);
        assert!(!claimed.running, "a finished run is not running");
        assert!(claimed.held, "but something does hold it");
        let rendered = crate::status::render(&claimed);
        assert!(
            rendered.contains("another process holds this run"),
            "{rendered}"
        );
        assert!(rendered.contains("run complete"), "{rendered}");
        assert!(!rendered.contains("run in progress"), "{rendered}");
    }

    #[test]
    fn following_a_finished_run_replays_it_and_stops() {
        let repo = temp_engine_repo("follow");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");

        let loaded = replay_of(&repo, &report.run_id);
        let sleeper = RecordingSleeper::default();
        let mut out: Vec<u8> = Vec::new();
        crate::status::follow(&loaded, &sleeper, Duration::ZERO, 2, &mut out).expect("follow");
        let text = String::from_utf8(out).expect("utf8");

        assert!(text.contains("run"), "{text}");
        assert!(text.contains("t1: committed"), "{text}");
        assert!(
            text.contains("run finished"),
            "it stops at the ending rather than idling: {text}"
        );
        assert!(
            sleeper.waits().is_empty(),
            "a finished run needs no waiting at all"
        );
        for line in text.lines() {
            assert!(!line.is_empty());
        }
    }

    #[test]
    fn transcripts_live_outside_the_workspace_and_survive_a_rollback() {
        // The §15 split, and the reason the private root cannot be inside the
        // repo: §14's rollback is `git clean -fd`, which would delete it.
        let repo = temp_engine_repo("private");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // The first attempt fails, so a rollback happens before the second.
        let adapters = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &adapters).expect("run");
        let paths = paths_of(&repo, &report.run_id);

        for private in [paths.transcripts(), paths.reviews(), paths.settings()] {
            assert!(
                !private.starts_with(&repo),
                "{} must be outside the workspace",
                private.display()
            );
            assert!(
                fs::read_dir(&private).into_iter().flatten().count() > 0,
                "{} kept its contents across the rollback",
                private.display()
            );
        }
        // The ops surface stays where §15 documents it.
        assert!(paths.events().starts_with(&repo));
        assert!(paths.questions().starts_with(&repo));
        // And nothing agent-authored is reachable from the repo.
        let in_repo = repo.join(".tactus").join("runs").join(&report.run_id);
        for leaked in ["transcripts", "reviews", "settings", "gates"] {
            assert!(
                !in_repo.join(leaked).exists(),
                "{leaked}/ must not exist inside the workspace"
            );
        }
    }

    // ---- step 8.1: the seams either side of the log ------------------------

    /// Drop a field from a log's `run_started` — the shape a log written before
    /// that field existed has.
    ///
    /// Selects the event by its tag rather than by line number: `run_started`
    /// is first today, and a helper that hard-codes that would silently rewrite
    /// an unrelated event the day something precedes it.
    fn strip_run_started_field(paths: &RunPaths, field: &str) {
        let text = fs::read_to_string(paths.events()).expect("log");
        let mut stripped = false;
        let rewritten: Vec<String> = text
            .lines()
            .map(|line| {
                let mut value: serde_json::Value =
                    serde_json::from_str(line).expect("every line is an event");
                if value.get("event").and_then(|e| e.as_str()) == Some("run_started")
                    && let Some(data) = value.get_mut("data").and_then(|d| d.as_object_mut())
                {
                    data.remove(field)
                        .unwrap_or_else(|| panic!("the run recorded no `{field}`"));
                    stripped = true;
                }
                value.to_string()
            })
            .collect();
        assert!(
            stripped,
            "the log has no run_started to strip `{field}` from"
        );
        fs::write(paths.events(), format!("{}\n", rewritten.join("\n"))).expect("rewrite");
    }

    /// Rewrite the opening event into the exact compatibility shape a
    /// schema-1 binary wrote: selected top-level fields absent and no per-chain
    /// binding snapshot. Used only by downgrade/resume regressions.
    fn rewrite_run_started_as_schema_one(paths: &RunPaths, absent: &[&str]) {
        let text = fs::read_to_string(paths.events()).expect("log");
        let mut rewritten_start = false;
        let rewritten: Vec<String> = text
            .lines()
            .map(|line| {
                let mut value: serde_json::Value =
                    serde_json::from_str(line).expect("every line is an event");
                if value.get("event").and_then(|event| event.as_str()) == Some("run_started") {
                    let data = value
                        .get_mut("data")
                        .and_then(serde_json::Value::as_object_mut)
                        .expect("run_started data");
                    data.insert("schema".to_owned(), serde_json::Value::from(1));
                    for field in absent {
                        data.remove(*field)
                            .unwrap_or_else(|| panic!("the run recorded no `{field}`"));
                    }
                    for chain in data
                        .get_mut("chains")
                        .and_then(serde_json::Value::as_array_mut)
                        .expect("run_started chains")
                    {
                        chain
                            .as_object_mut()
                            .expect("chain object")
                            .remove("bindings")
                            .expect("a schema-2 run records chain bindings");
                    }
                    rewritten_start = true;
                }
                value.to_string()
            })
            .collect();
        assert!(rewritten_start, "the log has no run_started event");
        fs::write(paths.events(), format!("{}\n", rewritten.join("\n"))).expect("rewrite");
    }

    /// Rewind a log to just before the named event — the shape a process
    /// killed at that instant leaves behind.
    fn truncate_log_before(paths: &RunPaths, event: &str) {
        let text = fs::read_to_string(paths.events()).expect("log");
        let lines: Vec<&str> = text.lines().collect();
        let cut = lines
            .iter()
            .position(|line| line.contains(&format!("\"{event}\"")))
            .unwrap_or_else(|| panic!("the run recorded no {event}"));
        fs::write(paths.events(), format!("{}\n", lines[..cut].join("\n"))).expect("truncate");
    }

    #[test]
    fn resume_adopts_the_commit_it_made_but_never_recorded() {
        // §14 commits, reads the sha back, scrubs the tree, and only then
        // appends `task_committed`. A process killed inside those three git
        // calls leaves the branch one commit past its own log — which is what
        // foreign history looks like too. Refusing would tell the operator to
        // reset away a commit that already passed its gates and its review,
        // and to spend the attempt a second time.
        let repo = temp_engine_repo("adoptcommit");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
        let run_id = report.run_id.clone();
        let paths = paths_of(&repo, &run_id);
        let sha = git_in(&repo, &["rev-parse", "HEAD"]).trim().to_owned();

        // The commit is on the branch; the log stops just short of it.
        truncate_log_before(&paths, "task_committed");

        let source = fake(Effect::EditFile);
        let (resumed, state) = resume_harness_inner(
            &resume_options(&repo, &run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume");

        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t1"), "adopted rather than redone");
        assert_eq!(
            git_in(&repo, &["rev-parse", "HEAD"]).trim(),
            sha,
            "and the branch was left exactly where it stood"
        );
        assert_eq!(
            git_in(&repo, &["rev-list", "--count", "main..HEAD"]).trim(),
            "1",
            "one commit, not a second one for the same work"
        );
        assert_eq!(
            task(&resumed, "t1").attempts.len(),
            1,
            "the attempt that passed was not spent again: {resumed:?}"
        );
        assert!(
            resumed
                .warnings
                .iter()
                .any(|w| w.contains("adopted commit")),
            "and the operator is told: {:?}",
            resumed.warnings
        );
        assert_live_equals_replay(&repo, &state, &resumed);
    }

    #[test]
    fn resume_refuses_a_commit_it_would_not_have_written() {
        // The adoption above is deliberately narrow: one commit past the
        // record, carrying the message this engine would have used for the
        // task whose attempt just passed. Anything else is someone's history.
        let repo = temp_engine_repo("adoptforeign");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
        truncate_log_before(&paths_of(&repo, &report.run_id), "task_committed");
        // Same place on the branch, different hand.
        git_in(
            &repo,
            &["commit", "-q", "--amend", "-m", "not the engine's"],
        );

        let err = resume_err(&repo, &report.run_id);
        assert!(err.contains("record ends at"), "got: {err}");
    }

    #[test]
    fn resume_writes_where_the_run_recorded_not_where_defaults_point() {
        // Which private root a run used is a fact about that run. Recomputing
        // it from today's environment — another HOME, a service account, the
        // no-home fallback — would scatter the rest of its transcripts
        // somewhere `status` never looks.
        let repo = temp_engine_repo("privatedir");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
        let run_id = report.run_id.clone();
        let recorded = paths_of(&repo, &run_id);
        truncate_log_before(&recorded, "task_committed");
        git_in(&repo, &["reset", "-q", "--hard", "HEAD~1"]);

        // No override, so the resume has to read the location off the record.
        let mut resume = resume_options(&repo, &run_id);
        resume.private_root = None;
        let source = fake(Effect::EditFile);
        let resumed = resume_harness(
            &resume,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume");
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(
            recorded.transcripts().join("00-t1-2.json").is_file(),
            "the resumed attempt wrote under {}",
            recorded.transcripts().display()
        );
    }

    #[test]
    fn resume_makes_a_stale_question_payload_agree_with_the_log() {
        // The engine emits `question_answered` and then rewrites the payload
        // beside it. A crash in between leaves a file that still reads as
        // open — and `tactus answer` will accept a second answer against it,
        // one no engine can ever ingest, because the log has already closed
        // the question.
        let repo = temp_engine_repo("stalepayload");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let doomed = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &doomed).expect("run");
        let run_id = report.run_id.clone();
        let first = report.questions[0].question.id.to_string();

        crate::answer::answer(
            &repo,
            &first,
            crate::answer::Reply::Text("try again".to_owned()),
        )
        .expect("answer");

        // The retry fails the same way, so the run ends parked on a *second*
        // question with the first one answered in the log.
        let retry = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let resumed = resume_with(&resume_options(&repo, &run_id), &retry).expect("resume");
        assert_eq!(resumed.outcome(), RunOutcome::Parked, "{resumed:?}");

        // Rewind the payload to what a crash mid-ingest leaves.
        let questions = rundir::public_dir(&repo, &run_id).join("questions");
        let path = questions.join(format!("{first}.json"));
        let mut record: QuestionRecord =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("parse");
        record.answer = None;
        interaction::write_question(&questions, &record).expect("rewrite");
        crate::answer::answer(&repo, &first, crate::answer::Reply::Decline)
            .expect("a stale payload is exactly what makes a second answer look acceptable");

        let source = fake(Effect::EditFile);
        resume_with(&resume_options(&repo, &run_id), &source).expect("second resume");

        let record: QuestionRecord =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("parse");
        assert!(
            !record.is_open(),
            "the payload agrees with the log again, so nobody answers it twice"
        );
    }

    #[test]
    fn a_run_that_never_started_leaves_no_directory_behind() {
        // Nothing is on the record until the first event lands. A failure in
        // that window would otherwise leave a run directory with no
        // `events.jsonl`, and since run ids sort newest-last it becomes what a
        // bare `tactus status` reports on — "no event log here" for a run that
        // never began, shadowing the real latest one.
        let repo = temp_engine_repo("husk");
        seed(
            &repo,
            "## Implement the widget\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        // Git stores refs as paths, so a branch literally named `tactus` is a
        // file where `tactus/run-<id>` needs a directory: branch creation
        // cannot succeed.
        git_in(&repo, &["branch", "tactus"]);

        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        run_with(&opts, &source).expect_err("the run branch cannot be created");

        assert_eq!(
            rundir::latest_run(&repo),
            None,
            "no husk left behind to shadow the next run"
        );
    }

    /// An operator working a backlog: they answer some other parked question
    /// out of band, reply to this one at the prompt, and then walk away — so a
    /// dropped answer never gets a second chance.
    struct BacklogAnswers {
        repo: PathBuf,
        used: Mutex<bool>,
    }

    impl AnswerSource for BacklogAnswers {
        fn id(&self) -> &'static str {
            "backlog"
        }

        fn resolve(&self, question: &Question) -> Result<Answer, TactusError> {
            let Ok(mut used) = self.used.lock() else {
                return Ok(Answer::Unanswered);
            };
            if *used {
                return Ok(Answer::Unanswered);
            }
            *used = true;
            let run = rundir::latest_run(&self.repo).expect("a run");
            let dir = rundir::public_dir(&self.repo, &run).join("questions");
            let other = fs::read_dir(&dir)
                .expect("questions dir")
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.strip_suffix(".json").map(str::to_owned)
                })
                .find(|id| id.as_str() != question.id.as_str());
            if let Some(other) = other {
                let _ = crate::answer::answer(
                    &self.repo,
                    &other,
                    crate::answer::Reply::Text("write src/other.rs".to_owned()),
                );
            }
            Ok(Answer::Answered {
                text: "write src/widget.rs".to_owned(),
            })
        }
    }

    #[test]
    fn a_typed_answer_survives_another_question_being_answered_at_the_same_time() {
        // Both channels can produce an answer on one scheduler turn. The sweep
        // must not swallow the reply the operator typed: it closed a different
        // question, and discarding this one throws away words a person sat and
        // wrote — words nothing will ask for again.
        let repo = temp_engine_repo("backlog");
        seed(
            &repo,
            "## First\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Second\n<!-- tactus: id=t2 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // Both tasks fail into a question, then both succeed once released.
        let source = source(
            vec![Effect::NoEdit, Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let answers = BacklogAnswers {
            repo: repo.clone(),
            used: Mutex::new(false),
        };
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&answers),
                sleeper: None,
            },
        )
        .expect("run");

        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
        for id in ["t1", "t2"] {
            assert!(committed(&report, id), "{id} was released: {report:?}");
        }
    }

    #[test]
    fn an_answer_file_that_changes_nothing_does_not_spin_the_scheduler() {
        // `sweep_answers` reports whether anything *changed*, and the drain
        // loop trusts that to mean it made progress. A file the sweep reads
        // but declines to apply — `unanswered`, which `tactus answer` refuses
        // to write but a hand-edit produces — must not read as progress: that
        // branch terminates only because it closes the question it fires for.
        // A regression here hangs this test rather than failing it.
        let repo = temp_engine_repo("nullanswer");
        seed(
            &repo,
            "## Doomed\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[interaction]\nmode = \"never\"\n\n\
                 [routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Parked);
        let run_id = report.run_id.clone();
        let question = report.questions[0].question.id.clone();

        let answers = rundir::public_dir(&repo, &run_id).join("answers");
        fs::create_dir_all(&answers).expect("answers dir");
        interaction::write_answer(&answers, &question, &Answer::Unanswered).expect("write");

        let source = fake(Effect::EditFile);
        let resumed = resume_with(&resume_options(&repo, &run_id), &source).expect("resume");
        assert_eq!(
            resumed.outcome(),
            RunOutcome::Parked,
            "still waiting on a real answer, and the run ended saying so: {resumed:?}"
        );
    }

    /// Holds a run's lock and lets go after a set number of sleeps — an engine
    /// that finishes while a follower is waiting on it.
    struct LockReleasingSleeper {
        waits: Mutex<u32>,
        release_after: u32,
        lock: Mutex<Option<RunLock>>,
    }

    impl LockReleasingSleeper {
        fn new(lock: RunLock, release_after: u32) -> Self {
            Self {
                waits: Mutex::new(0),
                release_after,
                lock: Mutex::new(Some(lock)),
            }
        }

        fn waits(&self) -> u32 {
            self.waits.lock().map(|w| *w).unwrap_or(0)
        }
    }

    impl Sleeper for LockReleasingSleeper {
        fn sleep(&self, _: Duration) {
            let Ok(mut waits) = self.waits.lock() else {
                return;
            };
            *waits += 1;
            if *waits == self.release_after
                && let Ok(mut lock) = self.lock.lock()
            {
                drop(lock.take());
            }
        }
    }

    #[test]
    fn following_waits_out_a_silent_live_run_and_stops_once_it_dies() {
        // A whole attempt — the agent's thinking, its tool calls, the gates,
        // the review — folds into one `attempt_finished`, so a healthy run
        // says nothing for minutes at a time. The idle budget exists to
        // release a terminal attached to a dead engine; spending it on a live
        // one drops the operator's view mid-run.
        let repo = temp_engine_repo("followlive");
        let source = fake(Effect::EditFile);
        let report = run_with(&options(&repo), &source).expect("run");
        let paths = paths_of(&repo, &report.run_id);

        // Drop the ending, so `follow` idles rather than stopping at it.
        let text = fs::read_to_string(paths.events()).expect("log");
        let kept: Vec<&str> = text
            .lines()
            .filter(|line| !line.contains("\"run_finished\""))
            .collect();
        fs::write(paths.events(), format!("{}\n", kept.join("\n"))).expect("truncate");

        let loaded = replay_of(&repo, &report.run_id);
        let held = RunLock::acquire(&paths.public).expect("simulate a live engine");
        let sleeper = LockReleasingSleeper::new(held, 5);
        let mut out: Vec<u8> = Vec::new();
        // A budget of one idle poll: without the liveness check this returns
        // after two sleeps, whatever the run is doing.
        crate::status::follow(&loaded, &sleeper, Duration::ZERO, 1, &mut out).expect("follow");

        assert!(
            sleeper.waits() > 5,
            "watched the live run past its idle budget and stopped once the lock went, \
             instead of timing out its silence: {} sleeps",
            sleeper.waits()
        );
    }

    // ---- step 10: pools, budgets, and spend approval (§13) ------------------

    /// A pools file beside the repo — never `~/.tactus`, which is the
    /// operator's, and never inside the workspace, where §14's `git clean -fd`
    /// would delete it.
    fn pools_file(repo: &Path, content: &str) -> PathBuf {
        let dir = private_root_for(repo);
        fs::create_dir_all(&dir).expect("pools dir");
        let path = dir.join("pools.toml");
        fs::write(&path, content).expect("pools file");
        path
    }

    const CLAUDE_POOL: &str = "[pools.claude-max]\nkind = \"subscription-window\"\nagent = \
                               \"claude-code\"\nsources = [\"signals\", \"self\"]\n";

    fn events_of(repo: &Path, run_id: &str) -> Vec<events::Event> {
        let mut ignored = Vec::new();
        events::read_all(&paths_of(repo, run_id).events(), &mut ignored).expect("the log reads")
    }

    fn budget_events(events: &[events::Event]) -> Vec<&events::BudgetExceeded> {
        events
            .iter()
            .filter_map(|event| match &event.body {
                EventBody::BudgetExceeded { data } => Some(data),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_run_budget_stops_the_run_exactly_once_and_survives_replay() {
        // The one-fold property, on the branch step 10 added: the stop is an
        // event, `RunState::apply` is what turns it into state, and a replay of
        // the log lands on the same state the live run held.
        let repo = temp_engine_repo("budgetstop");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- tactus: id=t2 kind=implement depends= -->\n\n\
             ## Three\n<!-- tactus: id=t3 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [budgets]\nrun_usd = 0.05\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let (report, live) = run_harness_inner(
            &opts,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("a budget stop is not an engine error");

        // Each task costs 0.06 (0.01 implementer + 0.05 review), so the ceiling
        // is crossed after the first and the second task is refused before it
        // spawns anything.
        assert_eq!(report.outcome(), RunOutcome::BudgetExceeded, "{report:?}");
        assert!(committed(&report, "t1"));
        let stop = report.budget_stop.as_ref().expect("a recorded stop");
        assert_eq!(stop.budget, events::BudgetKind::Run);
        assert_eq!(stop.task, "t2", "names the task that did not start");
        assert!(stop.spent_usd >= 0.05, "spent: {}", stop.spent_usd);

        // Exactly once: the scheduler stops scheduling on the first stop, so a
        // second would describe a spawn that never happened.
        let events = events_of(&repo, &report.run_id);
        assert_eq!(
            budget_events(&events).len(),
            1,
            "{:?}",
            budget_events(&events)
        );

        // Nothing after t1 ran, and the untouched tasks settle as skipped.
        assert!(matches!(task(&report, "t2").status, TaskRunStatus::Skipped));
        assert!(task(&report, "t2").attempts.is_empty());
        assert_live_equals_replay(&repo, &live, &report);
    }

    #[test]
    fn a_task_budget_also_ends_the_run_and_says_which_ceiling_it_was() {
        let repo = temp_engine_repo("taskbudget");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\", \"mid\"], attempts_per = 1 }\n\n\
                 [budgets]\ntask_usd = 0.005\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // Fails on the first rung, so a second attempt is asked for — and
        // refused, because this task has already spent past its own ceiling.
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::BudgetExceeded, "{report:?}");
        let stop = report.budget_stop.as_ref().expect("a recorded stop");
        assert_eq!(stop.budget, events::BudgetKind::Task);
        assert_eq!(stop.task, "t1");
        assert_eq!(
            task(&report, "t1").attempts.len(),
            1,
            "the escalated attempt never spawned"
        );
        let rendered = report.render();
        assert!(rendered.contains("task_usd"), "{rendered}");
        assert!(rendered.contains("tactus resume"), "{rendered}");
    }

    #[test]
    fn resuming_with_a_higher_ceiling_continues_the_run_the_budget_stopped() {
        // D4's whole point: a budget stop is recoverable in one command,
        // because budgets are re-derived at resume rather than inherited.
        let repo = temp_engine_repo("budgetresume");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- tactus: id=t2 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.budget_usd = Some(0.05);
        let source = fake(Effect::EditFile);
        let stopped = run_with(&opts, &source).expect("run");
        assert_eq!(stopped.outcome(), RunOutcome::BudgetExceeded);
        assert!(!committed(&stopped, "t2"));

        let mut resume_opts = resume_options(&repo, &stopped.run_id);
        resume_opts.budget_usd = Some(10.0);
        let source = fake(Effect::EditFile);
        let resumed = resume_harness(
            &resume_opts,
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("a budget stop is exactly what resume is for");
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t2"));
        assert!(
            resumed.budget_stop.is_none(),
            "the stop the resume got past must not still be reported"
        );
    }

    #[test]
    fn a_resume_that_does_not_raise_the_ceiling_stops_again_rather_than_running_past_it() {
        let repo = temp_engine_repo("budgetresumelow");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- tactus: id=t2 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [budgets]\nrun_usd = 0.05\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let stopped = run_with(&opts, &source).expect("run");
        assert_eq!(stopped.outcome(), RunOutcome::BudgetExceeded);

        let source = fake(Effect::EditFile);
        let again = resume_harness(
            &resume_options(&repo, &stopped.run_id),
            &Harness {
                adapters: &source,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume");
        assert_eq!(again.outcome(), RunOutcome::BudgetExceeded, "{again:?}");
        assert!(!committed(&again, "t2"));
    }

    #[test]
    fn a_frontier_escalation_over_the_threshold_parks_for_approval_then_runs_it() {
        // D3, end to end. The engine escalates FIRST and then asks, so an
        // approved task un-parks already standing on the frontier rung with a
        // fresh allowance — and `answer_question` needs no ApproveSpend arm.
        let repo = temp_engine_repo("approvespend");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.005 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let scripted = ScriptedAnswers::new(vec![Answer::Answered {
            text: "approve: run the escalated attempt".to_owned(),
        }]);
        let (report, live) = run_harness_inner(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&scripted),
                sleeper: None,
            },
        )
        .expect("run");

        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
        assert!(committed(&report, "t1"));
        let asked: Vec<&QuestionRecord> = report
            .questions
            .iter()
            .filter(|q| q.question.kind == QuestionKind::ApproveSpend)
            .collect();
        assert_eq!(asked.len(), 1, "asked once: {:?}", report.questions);
        assert!(
            asked[0].question.context.contains("frontier"),
            "the question names where the money is going: {}",
            asked[0].question.context
        );
        assert!(
            asked[0].question.context.contains("$0.0100"),
            "and quotes reported spend to date: {}",
            asked[0].question.context
        );

        // The approved attempt really ran on the frontier rung with the
        // allowance the escalation reset — not a re-run of the mid rung.
        let tiers: Vec<&str> = task(&report, "t1")
            .attempts
            .iter()
            .map(|a| a.tier.as_str())
            .collect();
        assert_eq!(tiers, ["mid", "frontier"], "{tiers:?}");
        assert_live_equals_replay(&repo, &live, &report);
    }

    #[test]
    fn a_declined_spend_approval_fails_the_task_through_the_halt_policy() {
        let repo = temp_engine_repo("declinespend");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.005 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let scripted = ScriptedAnswers::new(vec![Answer::Declined]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&scripted),
                sleeper: None,
            },
        )
        .expect("run");

        // Through `ingest_answer`'s existing Declined path — the one place that
        // owns the halt policy, with no ApproveSpend special case beside it.
        assert_eq!(report.outcome(), RunOutcome::Halted, "{report:?}");
        assert!(matches!(
            task(&report, "t1").status,
            TaskRunStatus::Failed {
                kind: FailureKind::Declined,
                ..
            }
        ));
        assert_eq!(
            task(&report, "t1").attempts.len(),
            1,
            "declining must not have spent the frontier attempt"
        );
    }

    #[test]
    fn a_chain_that_starts_at_frontier_never_asks_to_approve_spend() {
        // §12's target is silent escalation. A task the operator deliberately
        // routed to frontier in config was not escalated onto it silently, and
        // asking anyway trains people to approve without reading.
        let repo = temp_engine_repo("frontierstart");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"frontier\"], attempts_per = 2 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.0 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
        assert!(
            report
                .questions
                .iter()
                .all(|q| q.question.kind != QuestionKind::ApproveSpend),
            "questions: {:?}",
            report.questions
        );
    }

    #[test]
    fn attempts_are_attributed_to_the_pool_that_paid_them() {
        let repo = temp_engine_repo("poolattrib");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.pools_path = Some(pools_file(&repo, CLAUDE_POOL));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");

        let attempt = &task(&report, "t1").attempts[0];
        assert_eq!(attempt.pool.as_deref(), Some("claude-max"));
        assert!(
            attempt
                .reviews
                .iter()
                .all(|r| r.pool.as_deref() == Some("claude-max")),
            "the reviewer's own pool is attributed too: {:?}",
            attempt.reviews
        );

        // §13's second currency in the ledger, folded from the same records the
        // dollar column comes from.
        let drain = &report.pool_drain;
        assert_eq!(drain.len(), 1, "{drain:?}");
        assert_eq!(drain[0].pool, "claude-max");
        assert_eq!(drain[0].attempts, 2, "implementer plus its reviewer");
        let ledger = report.render_ledger();
        assert!(ledger.contains("claude-max"), "{ledger}");

        // And §14's pre-flight snapshot is on the record — folding to nothing,
        // which `assert_live_equals_replay` elsewhere is what proves.
        let events = events_of(&repo, &report.run_id);
        let started = events
            .iter()
            .find_map(|event| match &event.body {
                EventBody::AttemptStarted { data, .. } => Some(data),
                _ => None,
            })
            .expect("worker start was emitted");
        assert_eq!(started.adapter.as_deref(), Some("claude-code"));
        assert_eq!(started.preflight_cli_version.as_deref(), Some("0.0.0-fake"));
        assert_eq!(started.effort, Some(Effort::XHigh));
        assert_eq!(
            started.selection_origin,
            Some(events::SelectionOrigin::Auto)
        );

        let review = events
            .iter()
            .find_map(|event| match &event.body {
                EventBody::AttemptFinished { data, .. } => data.reviews.first(),
                _ => None,
            })
            .expect("review pass actually ran");
        assert_eq!(review.adapter.as_deref(), Some("claude-code"));
        assert_eq!(review.preflight_cli_version.as_deref(), Some("0.0.0-fake"));
        assert_eq!(review.effort, Some(Effort::Max));
        let snapshots: Vec<&events::CapacitySnapshot> = events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::CapacitySnapshot { data } => Some(data),
                _ => None,
            })
            .collect();
        assert_eq!(snapshots.len(), 1, "one snapshot per run start (§14)");
        assert_eq!(snapshots[0].pools.len(), 1);
        assert_eq!(
            snapshots[0].pools[0].remaining, "unknown",
            "never optimistic: an unmeasured pool is unknown, not full"
        );
    }

    #[test]
    fn a_pinned_live_attempt_records_its_selection_origin() {
        let repo = temp_engine_repo("pinorigin");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\
                 [[pins]]\ntier = \"small\"\nagent = \"claude-code\"\n\
                 model = \"claude-haiku-4-5\"\neffort = \"max\"\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let report = run_with(&opts, &fake(Effect::EditFile)).expect("run");
        let events = events_of(&repo, &report.run_id);
        let started = events
            .iter()
            .find_map(|event| match &event.body {
                EventBody::AttemptStarted { data, .. } => Some(data),
                _ => None,
            })
            .expect("worker start was emitted");
        assert_eq!(started.selection_origin, Some(events::SelectionOrigin::Pin));
        assert_eq!(started.effort, Some(Effort::Max));
    }

    #[test]
    fn a_rate_limit_marks_its_pool_exhausted_and_a_recovery_retires_the_signal() {
        // §13 source 1 made real: the signal is ground truth, and the estimator
        // that reads it back must never let a self-metered figure talk it up.
        let repo = temp_engine_repo("poolexhausted");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let pools = pools_file(&repo, CLAUDE_POOL);
        opts.pools_path = Some(pools.clone());
        let source = source(
            vec![Effect::RateLimited, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");

        let events = events_of(&repo, &report.run_id);
        let signals: Vec<&events::PoolExhausted> = events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::PoolExhausted { data, .. } => Some(data),
                _ => None,
            })
            .collect();
        assert_eq!(signals.len(), 1, "{signals:?}");
        assert_eq!(signals[0].pool, "claude-max");
        assert_eq!(signals[0].agent, "claude-code");

        // A fold that stops at the signal reads the pool as exhausted, at the
        // top confidence rank — the signal is ground truth about that moment.
        let signal_at = events
            .iter()
            .position(|e| matches!(e.body, EventBody::PoolExhausted { .. }))
            .expect("the signal is in the log");
        let through_signal = events[..=signal_at].to_vec();
        let mut warnings = Vec::new();
        let cfg = config::load(None, &repo, Some(&pools), &mut warnings).expect("pools");
        let at_the_signal = capacity::estimate(&cfg.pools, &capacity::observe(&through_signal));
        assert_eq!(at_the_signal[0].remaining, capacity::Remaining::Exhausted);
        assert_eq!(at_the_signal[0].confidence, capacity::Confidence::Signal);

        // But the whole log has the pool serving an attempt afterwards, so the
        // signal is retired rather than standing forever. Reporting `exhausted`
        // here — on the same line that reports the attempts it served — was the
        // shape the review caught.
        let settled = capacity::estimate(&cfg.pools, &capacity::observe(&events));
        assert_ne!(
            settled[0].remaining,
            capacity::Remaining::Exhausted,
            "{}",
            settled[0].describe()
        );
    }

    #[test]
    fn the_budget_flag_is_validated_like_the_config_key() {
        // `[budgets] run_usd = 0.0` is a hard error at load. The flag that
        // overrides it must not be a way around that: zero and negative both
        // stopped the run before it spent anything, and NaN silently never
        // fired at all.
        let repo = temp_engine_repo("budgetflag");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        for bad in [0.0, -5.0, f64::NAN] {
            let mut opts = options(&repo);
            opts.config_path = Some(repo.join("tactus.toml"));
            opts.budget_usd = Some(bad);
            let source = fake(Effect::EditFile);
            let err = run_with(&opts, &source).expect_err("a meaningless ceiling must refuse");
            assert!(
                err.to_string().contains("not a spendable ceiling"),
                "--budget {bad}: {err}"
            );
        }
        // And refused at pre-flight, before a branch or a run directory exists.
        let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch.trim(), "main", "refused before branching");
    }

    #[test]
    fn a_spend_approval_is_not_fed_back_to_the_agent_as_an_instruction() {
        // Every other question's answer is guidance for the next attempt. An
        // ApproveSpend answer is a yes/no about money whose meaning was already
        // consumed by the un-park, and `feedback_section` frames feedback as
        // "an instruction from a person… it takes precedence over your earlier
        // assumptions" — which is not a thing to tell a coding agent about a
        // billing decision.
        let repo = temp_engine_repo("approvalfeedback");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"mid\", \"frontier\"], attempts_per = 1 }\n\n\
                 [interaction]\nask_before = { frontier_escalation_over_usd = 0.005 }\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = source(
            vec![Effect::NoEdit, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let scripted = ScriptedAnswers::new(vec![Answer::Answered {
            text: "approve: run the escalated attempt".to_owned(),
        }]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&scripted),
                sleeper: None,
            },
        )
        .expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");

        let frontier = &source.adapter.runs()[1].prompt;
        assert!(
            !frontier.contains("approve: run the escalated attempt"),
            "the approval reached the implementer as guidance:
{frontier}"
        );
        // An Unblock answer still does, because there it really is guidance.
        assert!(
            !frontier.contains("instruction from a person"),
            "and no human-instruction framing at all:
{frontier}"
        );
    }

    #[test]
    fn picking_an_option_is_an_un_park_and_not_a_decision() {
        // The options a question carries are the engine's instructions to the
        // operator: "retry this task with guidance you type below", "answer in
        // your own words". `tactus answer <id> --option 1` resolved to that
        // sentence and pushed it as human feedback — so it reached the
        // implementer framed as "an instruction from a person", and once §12's
        // decisions were routed to the judge as well, it reached the reviewer
        // as "a decision from a person… a change that departs from it is a
        // defect however well argued". There is no diff that satisfies a
        // sentence about where to type, so an honest judge rejects every
        // attempt until the ladder is spent.
        let repo = temp_engine_repo("cannedoption");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // Ask, then answer by picking the first option verbatim — what
        // `--option 1` writes.
        let source = source(
            vec![Effect::AskQuestion, Effect::EditFile],
            vec![ReviewBehavior::Pass],
        );
        let scripted = ScriptedAnswers::new(vec![Answer::Answered {
            text: question_options(QuestionKind::Clarify)[0].clone(),
        }]);
        let report = run_harness(
            &opts,
            &Harness {
                adapters: &source,
                answers: Some(&scripted),
                sleeper: None,
            },
        )
        .expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");

        // The retry happened — the answer still un-parks the task.
        let runs = source.adapter.runs();
        let retry = &runs
            .iter()
            .filter(|r| !r.prompt.contains("DATA UNDER REVIEW"))
            .nth(1)
            .expect("a second implementer attempt")
            .prompt;
        assert!(
            !retry.contains("answer in your own words"),
            "the option label reached the implementer as guidance:\n{retry}"
        );
        assert!(
            !retry.contains("instruction from a person"),
            "and with no human-instruction framing at all:\n{retry}"
        );
        // And nothing reached the judge as an operator decision either.
        for review in runs
            .iter()
            .filter(|r| r.prompt.contains("DATA UNDER REVIEW"))
        {
            assert!(
                !review.prompt.contains("answer in your own words"),
                "the option label reached the reviewer as a decision:\n{}",
                review.prompt
            );
        }
    }

    #[test]
    fn one_outage_records_one_signal_however_many_deferrals_it_causes() {
        let repo = temp_engine_repo("onesignal");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.pools_path = Some(pools_file(&repo, CLAUDE_POOL));
        // Down for three attempts, then back.
        let source = source(
            vec![
                Effect::RateLimited,
                Effect::RateLimited,
                Effect::RateLimited,
                Effect::EditFile,
            ],
            vec![ReviewBehavior::Pass],
        );
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::Complete, "{report:?}");
        let signals = events_of(&repo, &report.run_id)
            .iter()
            .filter(|e| matches!(e.body, EventBody::PoolExhausted { .. }))
            .count();
        assert_eq!(
            signals, 1,
            "one outage is one fact; the deferrals are already on `task_deferred`"
        );
    }

    #[test]
    fn a_budget_stop_hands_back_a_clean_tree() {
        // §14 keeps the working tree for a resumed same-rung retry, because
        // that retry re-gates the *cumulative* diff. The ceiling is checked at
        // the top of the same loop, so a budget reached between the two
        // returns to the operator with a rejected attempt's edits still staged
        // in their repository — and staged changes follow `git switch` onto
        // whatever branch is visited next. Observed on a real repository:
        // run 01KZNMR59E5ATC9MBYY29WZB6E left two files staged after exit 3.
        //
        // Keeping them buys nothing even in principle: `run_resumed` discards
        // every uncommitted path and clears `session`/`resume_next` on every
        // task, so the retry those edits were preserved for cannot use them.
        let repo = temp_engine_repo("budgetdirty");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        // Enough for the first attempt, not for the retry that attempt asks
        // for — so the stop lands exactly between the two.
        opts.budget_usd = Some(0.05);
        let rejected = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
        let stopped = run_with(&opts, &rejected).expect("run");
        assert_eq!(stopped.outcome(), RunOutcome::BudgetExceeded, "{stopped:?}");

        let workspace = Workspace::open(&repo).expect("open");
        let left = workspace.uncommitted_summary().expect("status");
        assert!(
            left.is_empty(),
            "a clean stop left the rejected attempt in the operator's tree: {left:?}"
        );

        // And the run is still exactly as resumable as it was before.
        let mut resume_opts = resume_options(&repo, &stopped.run_id);
        resume_opts.budget_usd = Some(10.0);
        let accepted = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);
        let resumed = resume_harness(
            &resume_opts,
            &Harness {
                adapters: &accepted,
                answers: None,
                sleeper: None,
            },
        )
        .expect("resume");
        assert_eq!(resumed.outcome(), RunOutcome::Complete, "{resumed:?}");
        assert!(committed(&resumed, "t1"));
        assert!(
            !resumed
                .warnings
                .iter()
                .any(|w| w.contains("discarded") && w.contains("uncommitted")),
            "nothing should have been left for the resume to discard: {:?}",
            resumed.warnings
        );
    }

    /// One attempt that reported its spend and one that did not — the shape a
    /// kill/resume leaves, and a mixed-route ladder too.
    fn priced_and_unpriced_attempts() -> TaskReport {
        let attempt = |cost: Option<f64>| AttemptRecord {
            attempt: 1,
            tier: "frontier".to_owned(),
            model: "m".to_owned(),
            pool: None,
            resumed: false,
            duration: Duration::ZERO,
            cost_usd: cost,
            reviews: Vec::new(),
            session_id: None,
            usage: None,
            failure: None,
        };
        let mut task = task_report_costing(Some(0.2020), None);
        task.status = TaskRunStatus::Committed {
            sha: "abc123".to_owned(),
        };
        task.attempts = vec![attempt(None), attempt(Some(0.2020))];
        task
    }

    /// A `RunReport` with nothing in it, for tests that care about one field.
    fn empty_report() -> RunReport {
        RunReport {
            run_id: "01RUN".to_owned(),
            branch: "b".to_owned(),
            gates: Vec::new(),
            gates_from_config: false,
            warnings: Vec::new(),
            tasks: Vec::new(),
            halted_at: None,
            questions: Vec::new(),
            budget_stop: None,
            total_cost_usd: 0.0,
            pool_drain: Vec::new(),
            running: false,
            interrupted: false,
        }
    }

    #[test]
    fn an_unpriced_worker_reads_as_unreported_rather_than_free() {
        // §13's rule, on the line an operator actually reads. The ledger has
        // always shown `—` for a route that reports no dollars, and the review
        // half of this line has said `$?` since step 9 — but the worker half
        // used `unwrap_or(0.0)`, so a codex-implemented task printed
        // `gpt-5.6-sol $0.0000` above a ledger row reading `—`. One run, two
        // answers, and the wrong one is the one that looks precise.
        let mut task = task_report_costing(None, None);
        task.id = "t1".to_owned();
        task.model = "gpt-5.6-sol".to_owned();
        task.status = TaskRunStatus::Committed {
            sha: "abc123".to_owned(),
        };
        // The attempt that actually ran, as a route reporting no dollars
        // records it. Without this the task has no attempts at all, which is a
        // different thing entirely — nothing ran, so nothing is missing, and
        // the ledger correctly prints `—` rather than a floor.
        task.attempts = vec![AttemptRecord {
            attempt: 1,
            tier: "frontier".to_owned(),
            model: "gpt-5.6-sol".to_owned(),
            pool: None,
            resumed: false,
            duration: Duration::from_secs(46),
            cost_usd: None,
            reviews: Vec::new(),
            session_id: None,
            usage: None,
            failure: None,
        }];
        let report = RunReport {
            tasks: vec![task],
            ..empty_report()
        };

        let rendered = report.render();
        assert!(rendered.contains("gpt-5.6-sol $?"), "{rendered}");
        let task_line = rendered
            .lines()
            .find(|l| l.contains("t1: committed"))
            .expect("the task line");
        assert!(
            !task_line.contains("$0.0000"),
            "unreported spend rendered as free: {task_line}"
        );
        // And the same rule one level up. `total_cost_usd` is an `f64`, so it
        // cannot distinguish a zero sum from an unreported one — the floor has
        // to be carried beside it. Measured on run 01KZRTZ9ZKKF1YS7MVT4350X7M,
        // where a codex-implemented task made `total $0.1561` read as complete
        // while the worker's real spend was unknown.
        assert!(
            report.total_is_floor(),
            "an unpriced worker makes it a floor"
        );
        assert!(rendered.contains("total: $0.0000?"), "{rendered}");
        let ledger = report.render_ledger();
        assert!(ledger.contains("total $0.0000?"), "{ledger}");
        assert!(ledger.contains("a floor, not a total"), "{ledger}");
        // Here every attempt was unpriced, so the worker column is `—`, which
        // already says "unreported" — `partial` leaves it alone rather than
        // decorating it into `—?`.
        let row = ledger
            .lines()
            .find(|l| l.trim_start().starts_with("t1"))
            .expect("the ledger row");
        assert!(row.contains('—'), "{row}");

        // The `?` belongs on a figure that exists but is short: two attempts,
        // one priced and one not. That is what a resumed run looks like after
        // the engine was killed inside the first attempt, and what a mixed
        // ladder looks like when one rung reports and another does not.
        let mut mixed = priced_and_unpriced_attempts();
        mixed.id = "t2".to_owned();
        let row = RunReport {
            tasks: vec![mixed],
            ..empty_report()
        }
        .render_ledger();
        let row = row
            .lines()
            .find(|l| l.trim_start().starts_with("t2"))
            .expect("the ledger row");
        assert!(row.contains("$0.2020?"), "a floor must say so: {row}");

        // And a route that does report keeps its figure.
        let mut priced = report;
        priced.tasks[0].cost_usd = Some(0.2020);
        assert!(priced.render().contains("$0.2020"), "{}", priced.render());
    }

    #[test]
    fn a_status_from_a_newer_tactus_does_not_fail_the_whole_report() {
        // `report.json` is a projection for whoever reads the run afterwards,
        // and `TaskRunStatus` is `pub` and `Deserialize` because that reader
        // may be someone else's program. Every variant added to a serde-tagged
        // enum with no fallback is a hard `unknown variant` error in every
        // consumer built against an older version — one unreadable status makes
        // the entire report unreadable.
        //
        // `running`, `Queued` and `Running` did that to anything compiled
        // against 0.0.1, and that break is published and cannot be taken back.
        // This is so the next variant is not another one.
        let text = r#"{
          "run_id": "01RUN", "branch": "b", "gates": [], "gates_from_config": false,
          "warnings": [], "halted_at": null, "questions": [], "total_cost_usd": 0.0,
          "tasks": [
            {"id": "t1", "title": "One", "model": "m",
             "status": {"status": "teleported", "destination": "elsewhere"},
             "duration": {"secs": 0, "nanos": 0}, "cost_usd": null,
             "review_models": [], "review_cost_usd": null,
             "review_cost_incomplete": false, "session_id": null, "attempts": []},
            {"id": "t2", "title": "Two", "model": "m",
             "status": {"status": "committed", "sha": "abc123"},
             "duration": {"secs": 0, "nanos": 0}, "cost_usd": null,
             "review_models": [], "review_cost_usd": null,
             "review_cost_incomplete": false, "session_id": null, "attempts": []}
          ]
        }"#;

        let report: RunReport =
            serde_json::from_str(text).expect("one unknown status must not sink the report");
        assert!(matches!(task(&report, "t1").status, TaskRunStatus::Unknown));
        // And everything the reader *can* understand still arrives intact.
        assert!(
            matches!(&task(&report, "t2").status, TaskRunStatus::Committed { sha } if sha == "abc123")
        );
        let rendered = report.render();
        assert!(rendered.contains("t1: status not recognised"), "{rendered}");
        assert!(rendered.contains("t2: committed abc123"), "{rendered}");
    }

    #[test]
    fn a_report_for_a_dead_run_never_says_a_task_is_running() {
        // `Running` says of itself that only a live `status` produces it, and
        // the arm that built it consulted `in_flight` alone — while the arm
        // directly below, for `Queued`, guards on `running`. What actually held
        // the promise was a guarantee made one function away: `settle` turns
        // every `Pending` into `Skipped` before `task_report` sees it when the
        // run has ended, so the only way in is `Deferred`, which is recorded
        // after an attempt finishes and therefore never has anything in flight.
        //
        // Unreachable is not the same as impossible, and the distance between
        // the promise and the code keeping it is the whole hazard: a dangling
        // `in_flight` is what any error out of `run_attempt` leaves behind, and
        // `drain_and_report` writes a partial `report.json` on exactly that
        // path. One reordering away, that file reads `t1: running now — attempt
        // 2 on mid` beside a top-level `"running": false`, and outlives the
        // process that wrote it. So the invariant is stated where it is relied
        // upon.
        let task = Task {
            id: TaskId::from("t1"),
            kind: TaskKind::Implement,
            title: "One".to_owned(),
            body: String::new(),
            depends_on: Vec::new(),
            acceptance: Vec::new(),
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::new(),
        };
        let mid_attempt = Progress {
            in_flight: Some(events::InFlight {
                attempt: 2,
                rung: 1,
                tier: "mid".to_owned(),
                model: "claude-sonnet-5".to_owned(),
                profile: "mid-claude-sonnet-5".to_owned(),
                pool: None,
            }),
            ..Progress::default()
        };

        for state in [TaskState::Pending, TaskState::Deferred] {
            let dead = task_report(&task, &state, &mid_attempt, false);
            assert!(
                matches!(dead.status, TaskRunStatus::Skipped),
                "a report for an ended run claimed a live attempt from {state:?}: {:?}",
                dead.status
            );
            let live = task_report(&task, &state, &mid_attempt, true);
            assert!(
                matches!(live.status, TaskRunStatus::Running { .. }),
                "and a live one still reports it: {:?}",
                live.status
            );
        }
    }

    #[test]
    fn a_budget_stop_keeps_its_outcome_while_a_resume_holds_the_lock() {
        // `resume` takes the run's lock and then does a dozen git subprocesses
        // — branch checks, a switch, a discard — before it writes
        // `run_resumed`. Deriving liveness from the lock alone made that whole
        // window read as a live run: `status` printed `run in progress: N
        // task(s) committed so far` and returned early, dropping the stop
        // reason, the parked list, and the `resume --budget` line an operator
        // at a budget stop is running `status` to find.
        //
        // The lock answers who has claimed the run. Whether the run still has
        // anywhere to go is a question only its log answers.
        let repo = temp_engine_repo("resumewindow");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n"),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.budget_usd = Some(0.05);
        let rejected = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
        let stopped = run_with(&opts, &rejected).expect("run");
        assert_eq!(stopped.outcome(), RunOutcome::BudgetExceeded, "{stopped:?}");

        let paths = paths_of(&repo, &stopped.run_id);
        let _held = RunLock::acquire(&paths.public).expect("the resume claims it");

        let seen = replay_of(&repo, &stopped.run_id);
        assert!(
            !seen.running,
            "a run that recorded its finish is not running"
        );
        assert!(seen.held, "though a resume does hold it");
        let out = crate::status::render(&seen);
        assert!(out.contains("run stopped at its budget"), "{out}");
        assert!(out.contains("tactus resume"), "{out}");
        assert!(out.contains("another process holds this run"), "{out}");
        assert!(!out.contains("run in progress"), "{out}");
    }

    #[test]
    fn a_budget_stop_survives_a_git_that_cannot_clean_the_tree() {
        // Handing back a clean tree was added *before* the ceiling was
        // recorded, with a `?` on it. So a `git reset --hard` that failed for
        // any of the ordinary reasons — a locked index, a read-only path, a
        // hook that exits non-zero — took the whole budget stop with it: no
        // `budget_exceeded` event, `budget_stop` left `None`, exit 1 with a git
        // error where CI was gating on exit 3, and a `resume --budget` with no
        // stop to get past. The tidying is a courtesy; the ceiling is the run's
        // account of why it stopped.
        //
        // The gate plants a stale `.git/index.lock`, which is the most faithful
        // portable version of that: every later git command that writes the
        // index refuses, and nothing else changes.
        let repo = temp_engine_repo("budgetjam");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 2 }\n\n\
                 [[gates]]\nname = \"jam\"\ncmd = \"echo jam> .git/index.lock\"\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        opts.budget_usd = Some(0.05);
        let rejected = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);
        let stopped = run_with(&opts, &rejected).expect("the ceiling still ends the run");

        assert_eq!(
            stopped.outcome(),
            RunOutcome::BudgetExceeded,
            "a failed cleanup relabelled the stop: {stopped:?}"
        );
        let stop = stopped
            .budget_stop
            .as_ref()
            .expect("the ceiling is on the record even when the cleanup failed");
        assert_eq!(stop.budget, events::BudgetKind::Run);
        // And it says so rather than leaving the operator to find the mess.
        assert!(
            stopped
                .warnings
                .iter()
                .any(|w| w.contains("could not be cleaned")),
            "the dirty tree went unmentioned: {:?}",
            stopped.warnings
        );
    }

    #[test]
    fn a_budget_stop_survives_a_stale_decline_file() {
        // A decline routes through `fail_task`, which sets `halted_at`, and
        // halted outranks budget in `outcome()`. A decline sitting on disk when
        // the ceiling hits would have relabelled the stop as a task failure —
        // exit 1 where CI was gating on exit 3 to raise the ceiling.
        let repo = temp_engine_repo("budgetdecline");
        seed(
            &repo,
            "## One\n<!-- tactus: id=t1 kind=implement depends= -->\n\n\
             ## Two\n<!-- tactus: id=t2 kind=implement depends= -->\n",
            Some(
                "[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n\n\
                 [budgets]\nrun_usd = 0.05\n",
            ),
        );
        let mut opts = options(&repo);
        opts.config_path = Some(repo.join("tactus.toml"));
        let source = fake(Effect::EditFile);
        let report = run_with(&opts, &source).expect("run");
        assert_eq!(report.outcome(), RunOutcome::BudgetExceeded, "{report:?}");
        assert!(report.halted_at.is_none(), "nothing failed: {report:?}");
    }
}
