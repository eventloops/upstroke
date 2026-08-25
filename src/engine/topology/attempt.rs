//! The attempt: `attempt_started` → worker → capture → snapshots → judgement.
//!
//! Five ordering clauses, and each of them is a statement about what may exist
//! on disk when a coordinator dies. That is why they are clauses rather than
//! style: `T-ATTEMPT` tables four sub-prefixes — (a) worker running, (b') a
//! capture command interrupted, (b) capture done, (c) an ephemeral snapshot
//! commit with no worktree, (d) a snapshot worktree with gates running — and
//! each one is a **different** durable residue with a **different** tabled
//! recovery.
//!
//! * **O23 — `attempt_started` before spawn.** The event that says an attempt
//!   is in flight is durable before the process that spends money exists.
//!   `T-ATTEMPT`'s `authoritative_state` is "unknown spend, nothing judged", and
//!   unknown spend is recoverable only because the event precedes the spend.
//! * **O24 — retry: reservation, worktree verification, `attempt_started`
//!   (retry), spawn.** A clause with two owners, and this module owns the
//!   second half only. The reservation and the verification are
//!   [`super::settle::retry`]'s: it takes the `{pipeline}` reservation, runs
//!   O22's `Worktree.Verify` against the retained cumulative tree, and on a
//!   failure cancels that reservation and closes the generation with
//!   `generation_closed{WorktreeMissing}` — which is what INV-06 requires,
//!   because a `RetainedIdle` worktree "is never recreated". What it hands
//!   back is the `attempt_started(retry)` it authorized, and
//!   [`AttemptContext::start`] appends exactly that event and spawns.
//!
//!   There is deliberately **no retry entry point on this side**. One would
//!   have to re-observe a worktree `settle::retry` has already passed, and a
//!   refusal from that second look would strand the reservation the first one
//!   took: the branch that cancels it is `settle::retry`'s failure branch, and
//!   that branch was not taken. One observation, one recovery, one owner each.
//! * **O25 — capture before snapshots.** `Object.CandidateStage` then
//!   `Object.CandidateWriteTree`, whose objects are referenced by the task
//!   worktree's index (R9). A snapshot taken before the capture would be a
//!   snapshot of a tree that does not exist yet.
//! * **O26 — ephemeral snapshot commit before the snapshot intent, intent
//!   before the snapshot add.** The commit is unreferenced (R27) when it is
//!   written, so a death between it and the intent leaves an object that is
//!   Git's and nothing else's. [`WorkspaceManager::add_snapshot`] performs the
//!   three in that order.
//! * **O27 — gates and reviews before commit-tree.** The candidate commit is
//!   `candidate.rs`'s (O28–O31); what this module owes is that every judgement
//!   is complete, and complete **on fresh exact snapshots**, before that module
//!   is entered.
//!
//! # Where the attempt path hands off
//!
//! At the judgement, and **not** at a settlement. This module appends no
//! `attempt_finished`: `fold::check_candidate_prepared` requires the generation
//! to be `Promoting`, and only the succeeding settlement produces that class —
//! so `attempt_finished(succeeded)` sits *inside* the candidate sequence,
//! between the pin and `candidate_prepared`, rather than at the end of the
//! attempt. `T-CAND-OBJ`'s window is "attempt unsettled" across both the commit
//! object and the pin, which is the same statement from the fault matrix's
//! side. The append is `settle.rs`'s; what this module produces is the
//! [`Judgement`] that `candidate.rs` gates its sequence on.
//!
//! # Every process here goes through the run's `&dyn Runner`
//!
//! Worker, gates and reviewers alike, each carrying the [`InvocationId`] that
//! [`AttemptIdentities`] assigns. `permits.agent_pool_slots` then splits them:
//! "every agent CLI invocation acquires its atomic `{agent, pool?}` pair:
//! worker, review_pass, review_reask … and agent probe; **gate invocations and
//! the shell probe acquire no slot**". [`is_slotted`] is the single reading of
//! that sentence and this module never re-decides it.

use std::path::PathBuf;

use crate::agent::AdapterSource;
use crate::agent::proc::ProcessOutput;
use crate::engine::attempt::review_failure;
use crate::error::UpstrokeError;
use crate::events::ReviewRecord;
use crate::gates::GateFailure;
use crate::ir::WorkerProfile;
use crate::ladder::AttemptFailure;
use crate::review;
use crate::rundir::RunPaths;
use crate::runner::{
    AgentId, CommandSpec, InvocationId, Runner, RunnerRequest, gate_request, worker_request,
};
use crate::topology::events::{
    AttemptInterrupted4, AttemptNumber, AttemptStarted4, Materialization, RungBinding, SessionId,
    TopologyEventBody,
};
use crate::workspace_manager::{Slot, Snapshot, SnapshotInput, SnapshotName, WorkspaceManager};

use super::dispatch::{self, Dispatched, EventEmitter};
use super::identity::{AttemptIdentities, InvocationLedger, SlotAssertion, SlotPair, is_slotted};
use super::seams::TopologyHooks;

// ---------------------------------------------------------------------------
// What an attempt is asked to run
// ---------------------------------------------------------------------------

/// One reviewer of one attempt.
///
/// A reviewer is an agent CLI, so it is slotted and gets its own **fresh**
/// snapshot: `decisions.workspace_candidates.snapshots` is "one snapshot for
/// the gate set and one fresh snapshot per reviewer, never reused across roles
/// or attempts".
#[derive(Debug, Clone)]
pub struct ReviewerPlan {
    /// The agent whose CLI this pass runs.
    pub agent: AgentId,
    /// The routing decision this pass was bound with: model, effort, pool.
    ///
    /// `AttemptRecord` requires the model as a `String` and not an `Option`,
    /// so a plan that carried only a command could not produce the record the
    /// run must write.
    pub profile: WorkerProfile,
    /// Which review this is — what distinguishes it from the other passes.
    pub lens: review::Lens,
    /// What pre-flight certified this CLI as, where it certified one.
    pub preflight_cli_version: Option<String>,
    /// How long one invocation may take.
    pub timeout: std::time::Duration,
}

/// What every review pass of one attempt reads.
///
/// Owned, and on the plan rather than the context, because it is **data the
/// caller decided** — the same reason `worker: CommandSpec` is on the plan. The
/// context holds seams; the plan holds what this attempt is.
pub struct ReviewInputs {
    /// The task under review, as the prompt quotes it.
    pub title: String,
    /// Its body, which may be empty.
    pub body: String,
    /// Its acceptance criteria.
    pub acceptance: Vec<String>,
    /// The diff being judged.
    pub diff: String,
    /// Named artifacts the prompt wires to real files.
    pub artifacts: Vec<(String, String)>,
    /// Operator decisions the judge must honour, as the worker was given them.
    pub decisions: Vec<String>,
    /// The per-attempt file stem transcripts and settings are named from.
    pub stem: String,
}

/// Where a review pass is executed.
///
/// **A seam, and it is not optional.** `review::run_review` is on the effect
/// denylist — "UPSTROKE-WRAPPER: writes review transcripts through
/// `util::write_text`" — because it writes outside any inventoried `RunDir`
/// site. `decisions.effect_site_inventory.mechanism` (2) then says the legacy
/// allowlist "never contains a topology module", and this file is one, so the
/// escape is forbidden by name. `gates::run_all` is denied for the same reason,
/// which is why [`AttemptContext::judge`] runs gates through `gate_request`
/// itself rather than calling it.
///
/// Making the call legal directly would need a new `RunDirSite` variant for a
/// transcript write — there is none — in `src/topology/effects.rs`, which is
/// the file `ff0490a` froze by name. So the topology module declares what it
/// needs and something outside the tree performs it, exactly as
/// [`EventEmitter`], `Probes` and `IntegrationRefs` already do here.
///
/// **The authority is still `run_review`.** This is a seam over one
/// implementation, not a second one, and PR8's merge verification implements
/// the same trait rather than growing its own review path.
pub trait ReviewPasses {
    /// Run one review pass, re-asks and all.
    ///
    /// # Errors
    ///
    /// Whatever the review machinery returns. A reviewer that could not run is
    /// **not** an error — it is a `ReviewResult::Unavailable`, which the ladder
    /// defers rather than blaming on the implementer.
    fn run(
        &self,
        cx: &review::ReviewCx<'_>,
        runner: &dyn Runner,
        invocations: &review::ReviewInvocations,
    ) -> Result<review::ReviewOutcome, UpstrokeError>;
}

/// Everything one attempt executes, and the binding it executes under.
///
/// The binding, rung and pool are here rather than derived because
/// `attempt_started` records them and the fold checks them against the frozen
/// ladder: they are the attempt's execution identity (INV-19), and a module
/// that re-derived them would be a second authority for a value the registry
/// already froze.
#[derive(Debug, Clone)]
pub struct AttemptPlan {
    /// Which attempt of this generation. A retry is a **new** number, which is
    /// what makes its identities new.
    pub attempt: AttemptNumber,
    /// Index into the frozen ladder.
    pub rung: u32,
    /// The binding this attempt used.
    pub binding: RungBinding,
    /// The capacity pool, where the agent names one.
    pub pool: Option<String>,
    /// The session this attempt resumes. `Some` only for a same-session retry
    /// in the incarnation that retained it.
    pub resume_session: Option<SessionId>,
    /// What the repair's worktree looked like when this attempt started.
    /// `Some` for a repair, `None` otherwise.
    pub materialization_observed: Option<Materialization>,
    /// The agent the worker runs as.
    pub agent: AgentId,
    /// The worker's command.
    pub worker: CommandSpec,
    /// How long the worker may take.
    pub worker_timeout: std::time::Duration,
    /// The gate set, in order. Non-slotted, and run on one shared snapshot.
    pub gates: Vec<GatePlan>,
    /// The reviewers, in order. Slotted, and one fresh snapshot each.
    pub reviewers: Vec<ReviewerPlan>,
}

/// One gate of the gate set.
///
/// No agent field, and that is the invariant rather than an omission: "a gate is
/// repository-controlled code and runs no agent CLI, so it takes no
/// `{agent, pool}` pair (R3)". [`gate_request`] is the one construction point
/// that says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatePlan {
    /// What to run.
    pub command: CommandSpec,
    /// How long it may take.
    pub timeout: std::time::Duration,
}

// ---------------------------------------------------------------------------
// What an attempt produced
// ---------------------------------------------------------------------------

/// The worker ran and the attempt is in flight.
#[derive(Debug, Clone)]
pub struct AttemptRun {
    /// The identities every process of this attempt draws from.
    pub identities: AttemptIdentities,
    /// What the worker process did.
    pub worker: ProcessOutput,
}

/// The exact tree the capture wrote, and where it came from.
///
/// `decisions.workspace_candidates.candidate`: "capture the exact tree
/// (`Object.CandidateStage` then `Object.CandidateWriteTree`: the blob and tree
/// objects are referenced by the task worktree index, R9 …)". The tree id is the
/// whole product; the objects behind it are reachable only through that index
/// until something references the tree, which is what makes `T-ATTEMPT`'s
/// sub-prefix (b) a *distinct* durable state from (c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    /// The tree `git write-tree` printed.
    pub tree: String,
    /// The commit the tree is judged against, and the parent of every snapshot's
    /// ephemeral commit.
    pub parent: String,
}

/// One process's verdict, as the runner reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Which process.
    pub invocation: InvocationId,
    /// Where it ran — always an exact snapshot, never the task worktree.
    pub workspace: PathBuf,
    /// Its exit code, `None` when it was killed for a timeout or an output
    /// limit.
    pub code: Option<i32>,
}

impl Verdict {
    /// Whether the process succeeded.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.code == Some(0)
    }
}

/// What the gate set and the reviewers said.
#[derive(Debug, Clone)]
pub struct Judgement {
    /// One per gate, in the order the plan listed them.
    ///
    /// A [`Verdict`] and not something richer, because a shell gate's verdict
    /// **is** an exit code — it runs repository-controlled code and reports
    /// whether it passed. Widening this to carry a model and a cost would give
    /// gates fields nothing can fill.
    pub gates: Vec<Verdict>,
    /// One per review pass that ran, in the order the plan listed them.
    ///
    /// **The record itself, not a verdict.** `AttemptRecord.reviews` is
    /// `Vec<ReviewRecord>` and its emptiness *means* "nothing was reviewed", so
    /// an attempt that was reviewed must produce records or write a false
    /// statement into a log replay can never backfill. A `ReviewRecord`
    /// requires `model` as a `String`, a `cost_usd`, a `preflight_cli_version`
    /// and a typed `ReviewPassOutcome` — none of which an exit code can give,
    /// which is why this is what the review machinery returns rather than what
    /// this module derives from a process result.
    pub reviews: Vec<ReviewRecord>,
    /// What the gates and the reviews together say is wrong, if anything.
    ///
    /// Decided by the single production authorities — `engine::classify` for a
    /// failed gate, `engine::attempt::review_failure` for a review — because
    /// `ladder::next_step` reads this and `ladder::spends_allowance` derives
    /// the allowance decision from it. A second opinion formed here would
    /// change what a task costs.
    pub failure: Option<AttemptFailure>,
}

impl Judgement {
    /// Whether every gate and every reviewer passed.
    ///
    /// **O27's precondition.** `candidate.rs` enters the commit-tree sequence
    /// only for an accepted judgement, and a judgement exists only after every
    /// snapshot in it has been created, executed in, and removed.
    ///
    /// One field, not a second walk of the verdicts. This used to re-derive
    /// "did everything pass" by folding the gate and review results, which is a
    /// second opinion about the same question `failure` already answers — and
    /// the two could disagree, because `failure` is decided by
    /// `engine::classify` and `review_failure` while a fold here would be
    /// decided by this line. `ladder::next_step` and
    /// `ladder::spends_allowance` both read `failure`; nothing should read a
    /// different answer to the same question.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.failure.is_none()
    }
}

/// How an attempt left the run when it did not settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// A coordinator died holding it, and the recovering process appended the
    /// terminal.
    Interrupted,
    /// The run halted, and the cancellation appended the same terminal.
    Cancelled,
}

impl AttemptOutcome {
    /// The `detail` the terminal records.
    ///
    /// `AttemptInterrupted4` is "never halting … a statement about a
    /// coordinator, not a judgement of the work", so the two outcomes differ in
    /// what they say about *why* and in nothing else.
    const fn detail(self) -> &'static str {
        match self {
            Self::Interrupted => {
                "a coordinator died holding this attempt; the spend is unknown and nothing was \
                 judged"
            }
            Self::Cancelled => {
                "the run halted while this attempt was in flight; its invocations were cancelled \
                 and the spend is unknown"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The context
// ---------------------------------------------------------------------------

/// Everything one attempt needs from the run.
///
/// A borrowed bundle rather than eight parameters, because six of the eight are
/// process-lifetime ledgers that must be *the run's* and not a fresh one: a
/// caller that passed a new [`SlotAssertion`] would assert a single slotted
/// invocation against an empty table and never see the overlap the assertion
/// exists to catch.
pub struct AttemptContext<'a> {
    /// The execution root and its funnels.
    pub manager: &'a WorkspaceManager,
    /// The five effect-hook families.
    pub hooks: &'a mut dyn TopologyHooks,
    /// Where a durable event goes. `emit.rs`'s, behind its seam.
    pub emitter: &'a mut dyn EventEmitter,
    /// The boundary every process of this run crosses.
    pub runner: &'a dyn Runner,
    /// R3, "assertion only" at `max_parallel = 1`.
    pub slots: &'a mut SlotAssertion,
    /// R4: every Runner process registered exactly once, settled exactly once.
    pub ledger: &'a mut InvocationLedger,
    /// Where a reviewer's adapter is resolved from.
    ///
    /// A seam and not a plan field: the plan says *which agent* a pass is bound
    /// to, and this says how a name becomes an adapter. A plan carrying the
    /// adapter would need a lifetime, and a plan is a value the driver builds
    /// and holds across the appends it authorizes.
    pub adapters: &'a dyn AdapterSource,
    /// The run's directories — where a review's settings and transcripts go.
    pub paths: &'a RunPaths,
    /// Where a review pass is executed. See [`ReviewPasses`].
    pub reviews: &'a dyn ReviewPasses,
}

impl AttemptContext<'_> {
    /// **O23, and O24's second half.** Append `attempt_started`, then spawn the
    /// worker.
    ///
    /// The append is first and the spawn is second, and nothing sits between
    /// them: `T-ATTEMPT`'s boundary is "`attempt_started` (first or retry)
    /// without terminal", and its sub-prefix (a) is "worker running, no
    /// capture". A spawn that preceded the append would put a paid-for process
    /// outside the log entirely, and the recovering process would find a
    /// generation with no attempt and dispatch over the top of it.
    ///
    /// # A retry appends through here too
    ///
    /// The boundary says "first **or retry**", and one function serves both
    /// because there is one event: a retry's `attempt_started` is one whose
    /// `resume_session` is `Some` and whose [`AttemptPlan::attempt`] is a new
    /// number, and `plan` carries both. What makes it a retry is the decision
    /// taken *before* it — [`super::settle::retry`]'s reservation and
    /// `Worktree.Verify` — and this function does not re-take it. O24's
    /// "converted at `attempt_started(retry)`" is the caller's conversion of
    /// that same reservation at this append.
    ///
    /// A `retry` method here would be this function plus a second verification
    /// of a worktree that has already been verified, and its refusal branch
    /// would return while the reservation `settle::retry` took was still held —
    /// never converted, and never cancelled, because cancellation lives in the
    /// failure branch that the first, passing, verify did not take.
    ///
    /// # Errors
    ///
    /// Whatever the emitter returns; [`UpstrokeError::Refused`] from the slot
    /// assertion or the invocation ledger; or a runner failure. A non-zero exit
    /// is not an error — it is a [`ProcessOutput`] and the caller's to judge.
    pub fn start(
        &mut self,
        dispatched: &Dispatched,
        plan: &AttemptPlan,
    ) -> Result<AttemptRun, UpstrokeError> {
        self.emitter.emit(
            TopologyEventBody::AttemptStarted {
                data: AttemptStarted4 {
                    key: dispatched.key,
                    generation: dispatched.generation,
                    attempt: plan.attempt,
                    rung: plan.rung,
                    binding: plan.binding.clone(),
                    pool: plan.pool.clone(),
                    resume_session: plan.resume_session.clone(),
                    materialization_observed: plan.materialization_observed,
                },
            },
            self.hooks,
        )?;

        let identities =
            AttemptIdentities::new(dispatched.key, dispatched.generation, plan.attempt);
        let invocation = identities.worker();
        let request = worker_request(
            plan.worker.clone(),
            dispatched.worktree.clone(),
            plan.agent.clone(),
            plan.worker_timeout,
            invocation.clone(),
        );
        let worker = self.execute(&request, plan.pool.clone())?;
        Ok(AttemptRun { identities, worker })
    }

    /// **O25.** `Object.CandidateStage` then `Object.CandidateWriteTree`, in the
    /// task worktree.
    ///
    /// Two sites and not one, because a kill inside either leaves the same
    /// residue class and a different amount of it: `T-ATTEMPT` sub-prefix (b')
    /// is "git add or write-tree killed after writing objects and before
    /// publishing the index or cache-tree". The staged objects are behind the
    /// **task index** afterwards (R9), which is what makes them recoverable by
    /// scrubbing the worktree rather than by anything cleverer.
    ///
    /// # Errors
    ///
    /// The containment refusals or a Git error.
    pub fn capture(&mut self, dispatched: &Dispatched) -> Result<Capture, UpstrokeError> {
        self.manager
            .candidate_stage(self.hooks.effects(), &dispatched.slot)?;
        let tree = self
            .manager
            .candidate_write_tree(self.hooks.effects(), &dispatched.slot)?;
        Ok(Capture {
            tree,
            parent: dispatched.base.0.clone(),
        })
    }

    /// **O26 and O27.** The gate set on one fresh exact snapshot, then each
    /// reviewer on its own.
    ///
    /// `decisions.workspace_candidates.snapshots`: "exact snapshot worktrees
    /// (R24) are the only places gates and reviewers execute … one snapshot for
    /// the gate set and one fresh snapshot per reviewer, **never reused across
    /// roles or attempts**, cleaned on completion". The name carries the
    /// generation, the attempt and the role, so "never reused" is a property of
    /// [`SnapshotName`] rather than of this loop's discipline — and each
    /// snapshot is removed before the next is created, so a reviewer cannot
    /// inherit the previous one's checkout even by mistake.
    ///
    /// # What the name does not carry, and where that is owed
    ///
    /// The **task key**. [`SnapshotName::gates`] is `g<gen>-a<attempt>-gates`
    /// and [`SnapshotName::review`] is `g<gen>-a<attempt>-review<pass>`, so two
    /// *different tasks* at the same generation and attempt name the same slot.
    /// "Never reused" is therefore a property of the name **within** a task and
    /// a property of the substrate **across** tasks: at `max_parallel = 1` one
    /// attempt runs to completion before the next begins, so no two live
    /// snapshots can collide and the claim above holds exactly.
    ///
    /// This is the same PR11 debt [`Self::settle_interrupted`] records for the
    /// reclaim scope, reached from the other side — that one would remove a
    /// sibling's snapshot, this one would hand a sibling the same slot to
    /// create. A wider substrate owes the name a key; it is recorded here
    /// rather than approximated, because a snapshot name is what
    /// `WorkspaceManager` derives a path from and two tasks deriving one path
    /// is a collision no assertion in this module could see.
    ///
    /// O26 lives inside [`WorkspaceManager::add_snapshot`], which for a
    /// tree-only input performs commit-tree → intent → add in that order. This
    /// function's contribution to O26 is passing [`SnapshotInput::Tree`] — an
    /// integration snapshot checks out an existing commit and creates no object,
    /// and passing the wrong one here would silently skip the whole clause.
    ///
    /// # Errors
    ///
    /// The containment refusals, a Git error, a slot or ledger refusal, or a
    /// runner failure.
    pub fn judge(
        &mut self,
        dispatched: &Dispatched,
        run: &AttemptRun,
        capture: &Capture,
        plan: &AttemptPlan,
        inputs: &ReviewInputs,
        invocations: &dyn Fn(u32) -> review::ReviewInvocations,
    ) -> Result<Judgement, UpstrokeError> {
        let generation = dispatched.generation.0;
        let attempt = plan.attempt.0;

        let mut failure: Option<AttemptFailure> = None;
        let mut gates = Vec::with_capacity(plan.gates.len());
        if !plan.gates.is_empty() {
            let snapshot = self.snapshot(SnapshotName::gates(generation, attempt), capture)?;
            for (index, gate) in plan.gates.iter().enumerate() {
                let invocation = run
                    .identities
                    .gate(u32::try_from(index).unwrap_or(u32::MAX), 0);
                let request = gate_request(
                    gate.command.clone(),
                    snapshot.path.clone(),
                    gate.timeout,
                    invocation,
                );
                let verdict = self.verdict(&request, None)?;
                if !verdict.passed() && failure.is_none() {
                    failure = Some(crate::engine::classify::gate_failure(&GateFailure {
                        gate: format!("gate {index}"),
                        summary: format!(
                            "exit {}",
                            verdict
                                .code
                                .map_or_else(|| "signal".to_owned(), |code| code.to_string())
                        ),
                        log_tail: String::new(),
                    }));
                }
                gates.push(verdict);
            }
            self.manager
                .remove_snapshot(self.hooks.effects(), &snapshot)?;
        }

        let mut reviews = Vec::with_capacity(plan.reviewers.len());
        for (index, reviewer) in plan.reviewers.iter().enumerate() {
            // A failed gate is a rejection of the work, and a reviewer judging
            // a diff the gates already refused is a frontier invocation bought
            // to learn nothing. `run_attempt` short-circuits for the same
            // reason and §11.1 is the same sentence for gates.
            if failure.is_some() {
                break;
            }
            let pass = u32::try_from(index).unwrap_or(u32::MAX);
            let snapshot =
                self.snapshot(SnapshotName::review(generation, attempt, pass), capture)?;
            let adapter = self.adapters.get(reviewer.agent.as_str()).ok_or_else(|| {
                UpstrokeError::Refused {
                    message: format!(
                        "review pass {pass} is bound to agent `{}` and no adapter answers to that \
                         name; pre-flight probed the agents this run recorded and this is not one \
                         of them",
                        reviewer.agent.as_str()
                    ),
                }
            })?;
            let outcome = self.reviews.run(
                &review::ReviewCx {
                    adapter,
                    profile: reviewer.profile.clone(),
                    lens: reviewer.lens,
                    task: review::ReviewSubject {
                        title: &inputs.title,
                        body: &inputs.body,
                        acceptance: &inputs.acceptance,
                    },
                    diff: &inputs.diff,
                    artifacts: &inputs.artifacts,
                    decisions: &inputs.decisions,
                    workspace: &snapshot.path,
                    settings_dir: &self.paths.settings(),
                    reviews_dir: &self.paths.reviews(),
                    stem: format!("{}-{}", inputs.stem, attempt),
                    timeout: reviewer.timeout,
                },
                self.runner,
                // **Caller-supplied, ordinal and all.** Nothing pass-shaped is
                // minted here: PR8's merge verification is this machinery's
                // third caller and its identities come from
                // `SequenceIdentities`, not `AttemptIdentities`. A `judge` that
                // reached for either scheme would be a seam its next caller had
                // to redesign.
                &invocations(pass),
            )?;

            // Read before the result is consumed: a judge that never ran is not
            // a judge that said no, and the ledger has to show which happened.
            let unavailable = matches!(outcome.result, review::ReviewResult::Unavailable { .. });
            let cost_usd = outcome.cost_usd;
            failure = review_failure(outcome.result);
            reviews.push(
                super::super::classify::ReviewPassFacts {
                    pass: reviewer.lens.name(),
                    agent: &reviewer.profile.agent,
                    model: &reviewer.profile.model,
                    adapter: adapter.id(),
                    preflight_cli_version: reviewer.preflight_cli_version.clone(),
                    effort: reviewer.profile.effort,
                    pool: crate::engine::attempt::pool_option(&reviewer.profile.pool),
                    cost_usd,
                    unavailable,
                    failed: failure.is_some(),
                }
                .record(),
            );
            self.manager
                .remove_snapshot(self.hooks.effects(), &snapshot)?;
        }

        Ok(Judgement {
            gates,
            reviews,
            failure,
        })
    }

    /// One exact snapshot of the captured tree, on the recorded parent.
    fn snapshot(
        &mut self,
        name: SnapshotName,
        capture: &Capture,
    ) -> Result<Snapshot, UpstrokeError> {
        self.manager.add_snapshot(
            self.hooks.effects(),
            &name,
            &SnapshotInput::Tree {
                tree: capture.tree.clone(),
                parent: capture.parent.clone(),
            },
        )
    }

    /// `T-ATTEMPT`'s resume action, for a coordinator that died or a run that
    /// halted.
    ///
    /// > "append `attempt_interrupted` (unknown spend, allowance refunded,
    /// > generation Closed, lease by kind); discard residue: snapshot intents
    /// > and worktrees reclaimed (releasing an ephemeral commit to R27), the
    /// > task worktree scrubbed with force (releasing staged objects to R27 and
    /// > removing any index.lock or other administrative residue); an ephemeral
    /// > commit without a snapshot and objects written by an interrupted capture
    /// > are left to Git (nothing to reclaim)"
    ///
    /// The event is first and the reclaim second, for the same reason a run-end
    /// closure is: until the terminal is durable the log still says this attempt
    /// is in flight, and a scrub before it would remove the worktree the next
    /// resume would look for.
    ///
    /// "Nothing to reclaim" is load-bearing and is why this function does not
    /// go looking for unreferenced objects. An ephemeral commit written before
    /// its intent, and blobs written by a killed `git add`, are R27 — **Git's**
    /// — and an engine that pruned them would be establishing authority over
    /// the object store, which `cleanup` forbids ("cleanup is expected-path,
    /// contained, idempotent, and never establishes authority").
    ///
    /// # Scope of the snapshot reclaim
    ///
    /// Every snapshot intent of this execution root, not only this attempt's.
    /// That is what the sentence above says, and at `max_parallel = 1` it is
    /// exact: the sequential substrate runs one attempt to completion, so the
    /// snapshot namespace holds this attempt's snapshots and nothing else. PR11
    /// widens the substrate and owes this a per-attempt scope; it is recorded
    /// here rather than approximated, because a reclaim that quietly removed a
    /// sibling's snapshot would take its gates down with it.
    ///
    /// # Errors
    ///
    /// Whatever the emitter returns, or a Git or I/O error from the reclaim.
    pub fn settle_interrupted(
        &mut self,
        dispatched: &Dispatched,
        attempt: AttemptNumber,
        outcome: AttemptOutcome,
    ) -> Result<(), UpstrokeError> {
        self.emitter.emit(
            TopologyEventBody::AttemptInterrupted {
                data: AttemptInterrupted4 {
                    key: dispatched.key,
                    generation: dispatched.generation,
                    attempt,
                    lease: dispatched.closing_disposition(),
                    detail: outcome.detail().to_owned(),
                },
            },
            self.hooks,
        )?;
        self.discard_residue(dispatched)
    }

    /// The cancellation half of `T-ATTEMPT`: "at Halted the same terminal is
    /// appended by cancellation".
    ///
    /// `permits.protocol`: "cancel(invocation_id): a pending slotted request is
    /// removed …; a granted or non-slotted running invocation is cancelled
    /// **after the Runner terminated its process or container**". In the
    /// sequential substrate `Runner::run` is synchronous, so a halt observed by
    /// this coordinator is observed between invocations and there is no live
    /// child to terminate — but the *ledger* may still carry a registration
    /// whose completion never ran, and leaving it registered would make the
    /// process-end balance check pass over a leak. So the ledger is cancelled
    /// first, the slot released with it, and the terminal appended after.
    ///
    /// # Errors
    ///
    /// As [`Self::settle_interrupted`].
    pub fn cancel_in_flight(
        &mut self,
        dispatched: &Dispatched,
        attempt: AttemptNumber,
    ) -> Result<usize, UpstrokeError> {
        let cancelled = self.ledger.cancel_all_running();
        // Released without naming an invocation, because what is being
        // cancelled is whatever this process still holds. Naming one would be
        // asserting *which*, and a halt is the moment that assertion is least
        // safe: the pair may belong to the worker, to a reviewer, or to a
        // re-ask, and a guess that missed would leave the ledger unbalanced at
        // process end with nothing to say so.
        if let Some(held) = self.slots.held().cloned() {
            self.slots.release(&held)?;
        }
        self.settle_interrupted(dispatched, attempt, AttemptOutcome::Cancelled)?;
        Ok(cancelled)
    }

    /// Snapshots reclaimed, then the task worktree scrubbed with force.
    fn discard_residue(&mut self, dispatched: &Dispatched) -> Result<(), UpstrokeError> {
        for slot in self.manager.intents()? {
            if matches!(slot, Slot::Snapshot { .. }) {
                self.manager.remove_worktree(self.hooks.effects(), &slot)?;
                self.manager.remove_intent(self.hooks.effects(), &slot)?;
            }
        }
        dispatch::scrub(self.manager, self.hooks, &dispatched.slot)
    }

    /// Register, take the slot pair if the identity is slotted, run, release,
    /// complete.
    ///
    /// `permits.protocol` in order: "register(invocation_id, slots) -> if
    /// slotted, wait for the atomic pair grant … -> Runner spawn ->
    /// complete(invocation_id) releasing any slots". Nothing waits here: at
    /// `max_parallel = 1` a second concurrent slotted acquisition is a leaked
    /// hold rather than contention, and [`SlotAssertion`] refuses it.
    ///
    /// # The registration is settled on every path out
    ///
    /// The register is here and the pair, the run and the release are in
    /// [`Self::run_registered`], which is the whole reason the two are separate
    /// functions. `permits.protocol` settles an invocation "exactly once", and
    /// [`InvocationLedger::balances`] states that as "no entry is `Running`" —
    /// so a `?` between the register and the settlement would abandon a
    /// `Running` entry that at process end is **indistinguishable** from a
    /// process this coordinator genuinely lost. A slot pair the assertion
    /// refuses is not a lost process; it is a process that never started, and
    /// reporting it as a leak would spend a real signal on a bookkeeping
    /// mistake.
    fn execute(
        &mut self,
        request: &RunnerRequest,
        pool: Option<String>,
    ) -> Result<ProcessOutput, UpstrokeError> {
        self.ledger.register(&request.invocation)?;
        match self.run_registered(request, pool) {
            // `permits.protocol` settles an invocation exactly once, and the
            // two settlements are not interchangeable: a process the Runner
            // could not start or supervise never completed, and recording it as
            // completed would put a failure in the ledger under the name of a
            // success.
            Ok(output) => {
                if output.is_ok() {
                    self.ledger.complete(&request.invocation)?;
                } else {
                    self.ledger.cancel(&request.invocation)?;
                }
                output
            }
            Err(error) => {
                // Cancelled, not completed: the protocol failed before the
                // Runner answered, so nothing ran. It cannot itself fail —
                // `cancel` refuses only an identity that was never registered,
                // and the line above registered this one — and the failure that
                // brought us here is the one worth reporting either way.
                drop(self.ledger.cancel(&request.invocation));
                Err(error)
            }
        }
    }

    /// Everything between the registration and its settlement: the pair, the
    /// run, and the release.
    ///
    /// The nesting keeps the two failures apart, the way
    /// [`WorkspaceManager::verify_worktree`] keeps its two apart. The **outer**
    /// error is a protocol failure — a slotted request naming no agent, a pair
    /// the assertion refuses, a release that did not match — and means no
    /// process ran, so [`Self::execute`] cancels the registration. The
    /// **inner** one is the Runner's own answer about a process it could not
    /// start or supervise, and is what decides `complete` against `cancel`.
    ///
    /// Every early return here is therefore safe: this function may fail
    /// anywhere and the ledger entry is still settled exactly once, by its
    /// caller.
    fn run_registered(
        &mut self,
        request: &RunnerRequest,
        pool: Option<String>,
    ) -> Result<Result<ProcessOutput, UpstrokeError>, UpstrokeError> {
        let slotted = is_slotted(&request.invocation);
        if slotted {
            let agent = request
                .agent
                .as_ref()
                .ok_or_else(|| UpstrokeError::Refused {
                    message: format!(
                        "`{}` is a slotted invocation and its request names no agent; the pair it \
                         would take is `{{agent, pool?}}` and there is no agent to key it by",
                        request.invocation
                    ),
                })?;
            self.slots.acquire(
                &request.invocation,
                SlotPair {
                    agent: agent.as_str().to_owned(),
                    pool,
                },
            )?;
        }
        let output = self.runner.run(request);
        if slotted {
            self.slots.release(&request.invocation)?;
        }
        Ok(output)
    }

    /// [`Self::execute`], reduced to what a judgement records.
    fn verdict(
        &mut self,
        request: &RunnerRequest,
        pool: Option<String>,
    ) -> Result<Verdict, UpstrokeError> {
        let output = self.execute(request, pool)?;
        Ok(Verdict {
            invocation: request.invocation.clone(),
            workspace: request.workspace.clone(),
            code: output.code,
        })
    }
}

#[cfg(test)]
mod tests;
