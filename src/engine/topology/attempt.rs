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

use crate::agent::proc::ProcessOutput;
use crate::error::UpstrokeError;
use crate::runner::{
    AgentId, CommandSpec, InvocationId, Runner, RunnerRequest, gate_request, review_request,
    worker_request,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerPlan {
    /// The agent whose CLI this pass runs.
    pub agent: AgentId,
    /// The capacity pool it draws on, where its agent names one.
    pub pool: Option<String>,
    /// What to run.
    pub command: CommandSpec,
    /// How long it may take.
    pub timeout: std::time::Duration,
}

/// Everything one attempt executes, and the binding it executes under.
///
/// The binding, rung and pool are here rather than derived because
/// `attempt_started` records them and the fold checks them against the frozen
/// ladder: they are the attempt's execution identity (INV-19), and a module
/// that re-derived them would be a second authority for a value the registry
/// already froze.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    /// One per gate, in the order the plan listed them.
    pub gates: Vec<Verdict>,
    /// One per reviewer, in the order the plan listed them.
    pub reviews: Vec<Verdict>,
}

impl Judgement {
    /// Whether every gate and every reviewer passed.
    ///
    /// **O27's precondition.** `candidate.rs` enters the commit-tree sequence
    /// only for an accepted judgement, and a judgement exists only after every
    /// snapshot in it has been created, executed in, and removed.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.gates.iter().all(Verdict::passed) && self.reviews.iter().all(Verdict::passed)
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
        self.emitter.emit(TopologyEventBody::AttemptStarted {
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
        })?;

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
    ) -> Result<Judgement, UpstrokeError> {
        let generation = dispatched.generation.0;
        let attempt = plan.attempt.0;

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
                gates.push(self.verdict(&request, None)?);
            }
            self.manager
                .remove_snapshot(self.hooks.effects(), &snapshot)?;
        }

        let mut reviews = Vec::with_capacity(plan.reviewers.len());
        for (index, reviewer) in plan.reviewers.iter().enumerate() {
            let pass = u32::try_from(index).unwrap_or(u32::MAX);
            let snapshot =
                self.snapshot(SnapshotName::review(generation, attempt, pass), capture)?;
            let request = review_request(
                reviewer.command.clone(),
                snapshot.path.clone(),
                reviewer.agent.clone(),
                reviewer.timeout,
                run.identities.review_pass(pass, 0),
            );
            reviews.push(self.verdict(&request, reviewer.pool.clone())?);
            self.manager
                .remove_snapshot(self.hooks.effects(), &snapshot)?;
        }

        Ok(Judgement { gates, reviews })
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
        self.emitter.emit(TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key: dispatched.key,
                generation: dispatched.generation,
                attempt,
                lease: dispatched.closing_disposition(),
                detail: outcome.detail().to_owned(),
            },
        })?;
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
