# `src/engine/topology/attempt.rs`

Extended notes for [`src/engine/topology/attempt.rs`](../../../../src/engine/topology/attempt.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The attempt: `attempt_started` → worker → capture → snapshots → judgement.

Five ordering clauses, and each of them is a statement about what may exist
on disk when a coordinator dies. That is why they are clauses rather than
style: `T-ATTEMPT` tables four sub-prefixes — (a) worker running, (b') a
capture command interrupted, (b) capture done, (c) an ephemeral snapshot
commit with no worktree, (d) a snapshot worktree with gates running — and
each one is a **different** durable residue with a **different** tabled
recovery.

* **O23 — `attempt_started` before spawn.** The event that says an attempt
  is in flight is durable before the process that spends money exists.
  `T-ATTEMPT`'s `authoritative_state` is "unknown spend, nothing judged", and
  unknown spend is recoverable only because the event precedes the spend.
* **O24 — retry: reservation, worktree verification, `attempt_started`
  (retry), spawn.** A clause with two owners, and this module owns the
  second half only. The reservation and the verification are
  [`super::settle::retry`]'s: it takes the `{pipeline}` reservation, runs
  O22's `Worktree.Verify` against the retained cumulative tree, and on a
  failure cancels that reservation and closes the generation with
  `generation_closed{WorktreeMissing}` — which is what INV-06 requires,
  because a `RetainedIdle` worktree "is never recreated". What it hands
  back is the `attempt_started(retry)` it authorized, and
  [`AttemptContext::start`] appends exactly that event and spawns.

  There is deliberately **no retry entry point on this side**. One would
  have to re-observe a worktree `settle::retry` has already passed, and a
  refusal from that second look would strand the reservation the first one
  took: the branch that cancels it is `settle::retry`'s failure branch, and
  that branch was not taken. One observation, one recovery, one owner each.
* **O25 — capture before snapshots.** `Object.CandidateStage` then
  `Object.CandidateWriteTree`, whose objects are referenced by the task
  worktree's index (R9). A snapshot taken before the capture would be a
  snapshot of a tree that does not exist yet.
* **O26 — ephemeral snapshot commit before the snapshot intent, intent
  before the snapshot add.** The commit is unreferenced (R27) when it is
  written, so a death between it and the intent leaves an object that is
  Git's and nothing else's. [`WorkspaceManager::add_snapshot`] performs the
  three in that order.
* **O27 — gates and reviews before commit-tree.** The candidate commit is
  `candidate.rs`'s (O28–O31); what this module owes is that every judgement
  is complete, and complete **on fresh exact snapshots**, before that module
  is entered.

### Where the attempt path hands off

At the judgement, and **not** at a settlement. This module appends nothing:
a failed attempt settles on `attempt_finished` and a successful one on
`candidate_prepared`, which is the sole successful settlement for a
candidate-producing attempt and is what promotes the generation.

**This described the other shape** — `check_candidate_prepared` requiring
`Promoting`, and an `attempt_finished(succeeded)` between the pin and
`candidate_prepared` to produce it. That requirement *forced* the dual
settlement `design/26_design_merge_queue_protocol.md` §26 forbids,
and the 2026-08-27 CONFORM ruling reversed it: the fold now requires the
generation to be **in flight**, because this event settles it.

`T-CAND-OBJ`'s window is "attempt unsettled" across both the commit object
and the pin, which is the same statement from the fault matrix's side, and it
now runs all the way to `candidate_prepared`. What this module produces is
the [`Judgement`] that `candidate.rs` gates its sequence on.

### Every process here goes through the run's `&dyn Runner`

Worker, gates and reviewers alike, each carrying the [`InvocationId`] that
[`AttemptIdentities`] assigns. `permits.agent_pool_slots` then splits them:
"every agent CLI invocation acquires its atomic `{agent, pool?}` pair:
worker, review_pass, review_reask … and agent probe; **gate invocations and
the shell probe acquire no slot**". [`is_slotted`] is the single reading of
that sentence and this module never re-decides it.

## `pub struct ReviewerPlan {`

---------------------------------------------------------------------------
What an attempt is asked to run
---------------------------------------------------------------------------

## `pub struct ReviewerPlan {`

One reviewer of one attempt.

A reviewer is an agent CLI, so it is slotted and gets its own **fresh**
snapshot: `decisions.workspace_candidates.snapshots` is "one snapshot for
the gate set and one fresh snapshot per reviewer, never reused across roles
or attempts".

## `pub struct ReviewerPlan` › `pub agent: AgentId,`

The agent whose CLI this pass runs.

## `pub struct ReviewerPlan` › `pub profile: WorkerProfile,`

The routing decision this pass was bound with: model, effort, pool.

`AttemptRecord` requires the model as a `String` and not an `Option`,
so a plan that carried only a command could not produce the record the
run must write.

## `pub struct ReviewerPlan` › `pub lens: review::Lens,`

Which review this is — what distinguishes it from the other passes.

## `pub struct ReviewerPlan` › `pub preflight_cli_version: Option<String>,`

What pre-flight certified this CLI as, where it certified one.

## `pub struct ReviewerPlan` › `pub timeout: std::time::Duration,`

How long one invocation may take.

## `pub struct ReviewInputs {`

What every review pass of one attempt reads.

Owned, and on the plan rather than the context, because it is **data the
caller decided** — the same reason `worker: CommandSpec` is on the plan. The
context holds seams; the plan holds what this attempt is.

## `pub struct ReviewInputs` › `pub title: String,`

The task under review, as the prompt quotes it.

## `pub struct ReviewInputs` › `pub body: String,`

Its body, which may be empty.

## `pub struct ReviewInputs` › `pub acceptance: Vec<String>,`

Its acceptance criteria.

## `pub struct ReviewInputs` › `pub diff: String,`

The diff being judged.

## `pub struct ReviewInputs` › `pub artifacts: Vec<(String, String)>,`

Named artifacts the prompt wires to real files.

## `pub struct ReviewInputs` › `pub decisions: Vec<String>,`

Operator decisions the judge must honour, as the worker was given them.

## `pub struct ReviewInputs` › `pub stem: String,`

The per-attempt file stem transcripts and settings are named from.

## `pub struct PlanRequest<'a> {`

What only the driver knows about the attempt a plan is being built for.

The frozen entry travels whole rather than narrowed. Narrowing earns its
keep where a consumer reads three fields of fifteen — it is why
[`crate::review::ReviewBindings`] exists — and the plan assembler reads the
spec's five prompt fields, the ladder's effort, and all three review
bindings. A subject that wide is the entry with a second name.

The reference is short-lived by construction: the driver reads it out of the
fold it owns, and the plan that comes back is an owned value, so the borrow
ends before the append the plan authorizes.

## `pub struct PlanRequest<'a>` › `pub key: crate::topology::registry::TaskKey,`

The task.

## `pub struct PlanRequest<'a>` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `pub struct PlanRequest<'a>` › `pub entry: &'a crate::topology::registry::TaskEntry,`

Its frozen registration — spec, ladder, review bindings.

## `pub struct PlanRequest<'a>` › `pub attempt: AttemptNumber,`

Which attempt of this generation.

## `pub struct PlanRequest<'a>` › `pub rung: u32,`

Index into the frozen ladder.

## `pub struct PlanRequest<'a>` › `pub binding: RungBinding,`

The binding that rung resolved to, effort included.

## `pub struct PlanRequest<'a>` › `pub workspace: &'a std::path::Path,`

The task worktree the worker runs in.

## `pub struct PlanRequest<'a>` › `pub resume_session: Option<SessionId>,`

The session a same-session retry resumes.

## `pub struct PlanRequest<'a>` › `pub feedback: Vec<crate::events::Feedback>,`

What the previous attempts failed on, oldest first.

§11.4: failure feedback goes back to the same rung, and an escalation
carries the accumulated feedback with it. This was always empty — the
plan hard-coded `retry: None` — so a retried worker was given the same
prompt as the first attempt and no reason to do anything differently.
The whole point of spending a second attempt is that it is informed.

## `pub struct PlanRequest<'a>` › `pub materialization_observed: Option<Materialization>,`

What a repair's worktree looked like when this attempt started.

## `pub struct InputsRequest<'a> {`

Where an [`AttemptPlan`] is assembled.

**A seam for the same reason [`ReviewPasses`] is one.** Building the plan
means building the worker's command, and
[`crate::engine::assembly::WorkerAssembly::command`] materializes the
permissions file that defines the attempt's sandbox — a write outside any
inventoried `RunDir` site, in a legacy funnel this module may not be
allowlisted into. So the topology module declares the plan it needs and
something outside the tree builds it.

It is also the honest shape. A plan needs the run's config — the gate set,
the worker allowance, the pool table, the probed CLI versions — and none of
that is in the event log the fold replays. The driver knows the attempt; the
implementation knows the run it is part of.

## `pub struct InputsRequest<'a>` › `pub entry: &'a crate::topology::registry::TaskEntry,`

The task under review, as the prompt quotes it.

## `pub struct InputsRequest<'a>` › `pub diff: String,`

The diff being judged, as the engine captured it.

## `pub trait AttemptPlans {`

Where an [`AttemptPlan`] and the review inputs beside it are assembled.

## `pub trait AttemptPlans` › `fn inputs(&self, request: &InputsRequest<'_>) -> Result<ReviewInputs, UpstrokeError>;`

What a reviewer is shown, beside the diff the driver captured.

Separate from [`Self::plan`] because it needs the *outcome* of the
attempt — a plan is built before the worker runs and this is built
after. Same implementation, because both read the run's config: the
artifacts directory a named artifact resolves against, and the operator
decisions the judge must honour.

### Errors

Whatever the assembler returns.

## `pub trait AttemptPlans` › `fn pool_for(&self, agent: &str) -> Option<String>;`

The capacity pool `agent` drains, where the run's table names one.

**Here rather than in the driver because the pool rule has one
production implementation** and this seam owns the frozen table it reads.
The dispatch arm gets the pool from the plan (`plan.pool`); the retry arm
appends `attempt_started` **before** its plan exists — `settle::retry`
produces the event and the plan is built after — so it needs the same
answer one step earlier. Resolving it in the driver would be a second
place that decides which pool an attempt drains, which is what
`a_reviewers_profile_is_accounted_for_at_both_callers` exists to forbid.

Sol's `R3-SEAMS-001`: the retry passed `pool: None`, so a resumed run's
`attempt_started` recorded no pool while the plan it then built resolved
one — the ledger and the plan disagreeing about the same attempt.

## `pub trait AttemptPlans` › `fn plan(&self, request: &PlanRequest<'_>) -> Result<AttemptPlan, UpstrokeError>;`

The plan for one attempt.

### Errors

Whatever the assembler returns: an agent with no registered adapter, a
permissions file that cannot be written, a binary that cannot be
resolved.

## `pub trait ReviewInputPolicy {`

Whether the staged evidence is the evidence a gate would see.

**The ladder's third cheap rung, and a seam for the reason
[`ReviewPasses`] is one.** `Workspace::review_input_problem_for_tree` is the
policy: it refuses a tree whose bytes a clean/smudge filter has transformed,
and a worktree still holding unstaged or dirty nested state behind an
unchanged gitlink. Either makes the diff describe something other than what
the gates run against, so neither can be reviewed completely.

It reads a [`crate::workspace::Workspace`], which is the legacy type; this
module holds a `WorkspaceManager` and a `Slot`. Re-deriving the policy on
the manager would be a second implementation of a refusal whose whole job is
to be exact, so the topology module declares what it needs and something
outside the tree performs it.

## `pub trait ReviewInputPolicy` › `fn problem(`

The problem with this tree as review input, or `None`.

### Errors

A Git or I/O error from the inspection.

## `pub trait ReviewPasses {`

Where a review pass is executed.

**A seam, and it is not optional.** `review::run_review` is on the effect
denylist — "UPSTROKE-WRAPPER: writes review transcripts through
`util::write_text`" — because it writes outside any inventoried `RunDir`
site. `decisions.effect_site_inventory.mechanism` (2) then says the legacy
allowlist "never contains a topology module", and this file is one, so the
escape is forbidden by name. `gates::run_all` is denied for the same reason,
which is why [`AttemptContext::judge`] runs gates through `gate_request`
itself rather than calling it.

Making the call legal directly would need a new `RunDirSite` variant for a
transcript write — there is none — in `topology::effects`, whose root
`src/topology/effects.rs` is the file `ff0490a` froze by name and whose site
enums are in its `sites.rs` child since that module was split. So the topology module declares what it
needs and something outside the tree performs it, exactly as
[`EventEmitter`], `Probes` and `IntegrationRefs` already do here.

**The authority is still `run_review`.** This is a seam over one
implementation, not a second one, and PR8's merge verification implements
the same trait rather than growing its own review path.

## `pub trait ReviewPasses` › `fn run(`

Run one review pass, re-asks and all.

### Errors

Whatever the review machinery returns. A reviewer that could not run is
**not** an error — it is a `ReviewResult::Unavailable`, which the ladder
defers rather than blaming on the implementer.

## `pub struct AttemptPlan {`

Everything one attempt executes, and the binding it executes under.

The binding, rung and pool are here rather than derived because
`attempt_started` records them and the fold checks them against the frozen
ladder: they are the attempt's execution identity (INV-19), and a module
that re-derived them would be a second authority for a value the registry
already froze.

## `pub struct AttemptPlan` › `pub attempt: AttemptNumber,`

Which attempt of this generation. A retry is a **new** number, which is
what makes its identities new.

## `pub struct AttemptPlan` › `pub rung: u32,`

Index into the frozen ladder.

## `pub struct AttemptPlan` › `pub binding: RungBinding,`

The binding this attempt used.

## `pub struct AttemptPlan` › `pub pool: Option<String>,`

The capacity pool, where the agent names one.

## `pub struct AttemptPlan` › `pub resume_session: Option<SessionId>,`

The session this attempt resumes. `Some` only for a same-session retry
in the incarnation that retained it.

## `pub struct AttemptPlan` › `pub materialization_observed: Option<Materialization>,`

What the repair's worktree looked like when this attempt started.
`Some` for a repair, `None` otherwise.

## `pub struct AttemptPlan` › `pub agent: AgentId,`

The agent the worker runs as.

## `pub struct AttemptPlan` › `pub session_resume: bool,`

Whether that agent's CLI can resume a session, as pre-flight probed it.

Half of `LadderState::resumable`; the other half is whether the attempt
actually returned a session id. Both are required, and the settlement
reads them together: a `Retained` generation with no session to resume
is a generation nothing can continue.

## `pub struct AttemptPlan` › `pub worker: CommandSpec,`

The worker's command.

## `pub struct AttemptPlan` › `pub worker_timeout: std::time::Duration,`

How long the worker may take.

## `pub struct AttemptPlan` › `pub gates: Vec<GatePlan>,`

The gate set, in order. Non-slotted, and run on one shared snapshot.

## `pub struct AttemptPlan` › `pub reviewers: Vec<ReviewerPlan>,`

The reviewers, in order. Slotted, and one fresh snapshot each.

## `pub struct GatePlan {`

One gate of the gate set.

No agent field, and that is the invariant rather than an omission: "a gate is
repository-controlled code and runs no agent CLI, so it takes no
`{agent, pool}` pair (R3)". [`gate_request`] is the one construction point
that says so.

## `pub struct GatePlan` › `pub name: String,`

The gate's configured name, which is what a failure reports and a human
reads. `gate {index}` named the position instead, so an operator had to
count the config to find out which gate rejected their task.

## `pub struct GatePlan` › `pub command: CommandSpec,`

What to run.

## `pub struct GatePlan` › `pub timeout: std::time::Duration,`

How long it may take.

## `pub struct AttemptRun {`

---------------------------------------------------------------------------
What an attempt produced
---------------------------------------------------------------------------

## `pub struct AttemptRun {`

The worker ran and the attempt is in flight.

## `pub struct AttemptRun` › `pub identities: AttemptIdentities,`

The identities every process of this attempt draws from.

## `pub struct AttemptRun` › `pub worker: ProcessOutput,`

What the worker process did.

## `pub struct Capture {`

The exact tree the capture wrote, and where it came from.

`decisions.workspace_candidates.candidate`: "capture the exact tree
(`Object.CandidateStage` then `Object.CandidateWriteTree`: the blob and tree
objects are referenced by the task worktree index, R9 …)". The tree id is the
whole product; the objects behind it are reachable only through that index
until something references the tree, which is what makes `T-ATTEMPT`'s
sub-prefix (b) a *distinct* durable state from (c).

## `pub struct Capture` › `pub tree: String,`

The tree `git write-tree` printed.

## `pub struct Capture` › `pub parent: String,`

The commit the tree is judged against, and the parent of every snapshot's
ephemeral commit.

## `fn captured_object_id(source: &str, value: String) -> Result<ObjectId, UpstrokeError> {`

One of the capture's recorded ids as an [`ObjectId`], or a Git error.

**§7: whose mistake is it.** These two values are not a caller's: the tree
is `git write-tree`'s own output and the parent is the base commit this run
recorded, both already object ids by the time they are held as strings. A
value here that is not one is the tool or the engine misbehaving, so it is
[`UpstrokeError::Git`] naming where the value came from, as
`add_snapshot` does for a `git commit-tree` line -- not
`UpstrokeError::Refused`, which says the caller offered something it should
not have and which `ObjectId::new`'s refusal would otherwise become here.

## `pub struct Assessment {`

What the ladder's **cheap rungs** said, before a gate or a reviewer ran.

`run_attempt`'s order is "outcome sanity → cheap static provenance → diff
classification → gates → review. Cheapest and most objective first", and
each rung is guarded by `failure.is_none()` — a worker that died is not
asked to pass gates. [`AttemptContext::judge`] implements the last two
rungs; this is the first ones, and without it the driver would hand an
empty diff to a frontier reviewer and then settle it as a success.

## `pub struct Assessment` › `pub outcome: crate::ir::Outcome,`

The adapter's parse of what the worker said about itself: session, cost,
usage, status. The attempt record is built from it.

## `pub struct Assessment` › `pub failure: Option<AttemptFailure>,`

What the cheap rungs concluded, if anything.

## `pub struct AttemptSite<'a> {`

Where one attempt runs, whichever branch put it there.

**A dispatch and a retry are the same attempt over different ground.** The
ready-dispatch branch opens a generation and gets a [`Dispatched`]; the
ready-retry branch continues one whose worktree already holds the previous
attempt's cumulative work, and no dispatch happened. Constructing a
`Dispatched` for the second would be a type asserting something false, so
the phases ask for the five facts they actually read instead.

## `pub struct AttemptSite<'a>` › `pub key: crate::topology::registry::TaskKey,`

The task.

## `pub struct AttemptSite<'a>` › `pub generation: crate::topology::events::GenerationId,`

The generation the attempt runs in.

## `pub struct AttemptSite<'a>` › `pub base: &'a crate::topology::events::CommitSha,`

The commit the worktree was created at.

## `pub struct AttemptSite<'a>` › `pub slot: &'a Slot,`

The worktree's slot.

## `pub struct AttemptSite<'a>` › `pub worktree: &'a std::path::Path,`

Its path on disk.

## `impl Dispatched` › `pub fn site(&self) -> AttemptSite<'_> {`

This dispatch, as the ground an attempt runs on.

## `pub struct Judging<'a> {`

What one attempt produced, for the judging that reads all three.

A bundle because they arrive together and are read together: `judge` needs
the identities to name its gate and review invocations, the tree to snapshot
from, and the cheap rungs' verdict to start its own ladder at. Passing them
singly put `judge` at eight arguments, which is a signal about the shape
rather than about the lint.

## `pub struct Judging<'a>` › `pub run: &'a AttemptRun,`

The identities and the worker process.

## `pub struct Judging<'a>` › `pub capture: &'a Capture,`

The exact tree and the commit it is judged against.

## `pub struct Judging<'a>` › `pub assessed: &'a Assessment,`

What outcome sanity and the diff already concluded.

## `pub struct Verdict {`

One process's verdict, as the runner reported it.

## `pub struct Verdict` › `pub output_limited: bool,`

Whether the Runner truncated the process's output.

§11.1 makes the 8-KiB tail the feedback a retry is given, so a truncated
log is a different claim from a short one and the ladder is entitled to
know which it has.

## `pub struct Verdict` › `pub timed_out: bool,`

Whether the Runner killed it on its timeout.

## `pub struct Verdict` › `pub log: String,`

What it printed, for the tail `GateFailure::log_tail` carries.

Kept because the driver's gate failure had `log_tail: String::new()`: a
gate could reject an attempt and the retry that followed was told the
exit code and nothing else. §11.4 sends the diagnostic back to the same
rung, and there was no diagnostic to send.

## `pub struct Verdict` › `pub invocation: InvocationId,`

Which process.

## `pub struct Verdict` › `pub workspace: PathBuf,`

Where it ran — always an exact snapshot, never the task worktree.

## `pub struct Verdict` › `pub code: Option<i32>,`

Its exit code, `None` when it was killed for a timeout or an output
limit.

## `impl Verdict` › `pub fn passed(&self) -> bool {`

Whether the process succeeded.

## `pub struct Judgement {`

What the gate set and the reviewers said.

## `pub struct Judgement` › `pub gates: Vec<Verdict>,`

One per gate, in the order the plan listed them.

A [`Verdict`] and not something richer, because a shell gate's verdict
**is** an exit code — it runs repository-controlled code and reports
whether it passed. Widening this to carry a model and a cost would give
gates fields nothing can fill.

## `pub struct Judgement` › `pub reviews: Vec<ReviewRecord>,`

One per review pass that ran, in the order the plan listed them.

**The record itself, not a verdict.** `AttemptRecord.reviews` is
`Vec<ReviewRecord>` and its emptiness *means* "nothing was reviewed", so
an attempt that was reviewed must produce records or write a false
statement into a log replay can never backfill. A `ReviewRecord`
requires `model` as a `String`, a `cost_usd`, a `preflight_cli_version`
and a typed `ReviewPassOutcome` — none of which an exit code can give,
which is why this is what the review machinery returns rather than what
this module derives from a process result.

## `pub struct Judgement` › `pub failure: Option<AttemptFailure>,`

What the gates and the reviews together say is wrong, if anything.

Decided by the single production authorities — `engine::classify` for a
failed gate, `engine::attempt::review_failure` for a review — because
`ladder::next_step` reads this and `ladder::spends_allowance` derives
the allowance decision from it. A second opinion formed here would
change what a task costs.

## `impl Judgement` › `pub fn accepted(&self) -> bool {`

Whether every gate and every reviewer passed.

**O27's precondition.** `candidate.rs` enters the commit-tree sequence
only for an accepted judgement, and a judgement exists only after every
snapshot in it has been created, executed in, and removed.

One field, not a second walk of the verdicts. This used to re-derive
"did everything pass" by folding the gate and review results, which is a
second opinion about the same question `failure` already answers — and
the two could disagree, because `failure` is decided by
`engine::classify` and `review_failure` while a fold here would be
decided by this line. `ladder::next_step` and
`ladder::spends_allowance` both read `failure`; nothing should read a
different answer to the same question.

## `pub enum AttemptOutcome {`

How an attempt left the run when it did not settle.

## `pub enum AttemptOutcome` › `Interrupted,`

A coordinator died holding it, and the recovering process appended the
terminal.

## `pub enum AttemptOutcome` › `Cancelled,`

The run halted, and the cancellation appended the same terminal.

## `impl AttemptOutcome` › `const fn detail(self) -> &'static str {`

The `detail` the terminal records.

`AttemptInterrupted4` is "never halting … a statement about a
coordinator, not a judgement of the work", so the two outcomes differ in
what they say about *why* and in nothing else.

## `pub struct AttemptContext<'a> {`

---------------------------------------------------------------------------
The context
---------------------------------------------------------------------------

## `pub struct AttemptContext<'a> {`

Everything one attempt needs from the run.

A borrowed bundle rather than eight parameters, because six of the eight are
process-lifetime ledgers that must be *the run's* and not a fresh one: a
caller that passed a new [`SlotAssertion`] would assert a single slotted
invocation against an empty table and never see the overlap the assertion
exists to catch.

## `pub struct AttemptContext<'a>` › `pub manager: &'a WorkspaceManager,`

The execution root and its funnels.

## `pub struct AttemptContext<'a>` › `pub hooks: &'a mut dyn TopologyHooks,`

The five effect-hook families.

## `pub struct AttemptContext<'a>` › `pub emitter: &'a mut dyn EventEmitter,`

Where a durable event goes. `emit.rs`'s, behind its seam.

## `pub struct AttemptContext<'a>` › `pub runner: &'a dyn Runner,`

The boundary every process of this run crosses.

## `pub struct AttemptContext<'a>` › `pub slots: &'a mut SlotAssertion,`

R3, "assertion only" at `max_parallel = 1`.

## `pub struct AttemptContext<'a>` › `pub ledger: &'a mut InvocationLedger,`

R4: every Runner process registered exactly once, settled exactly once.

## `pub struct AttemptContext<'a>` › `pub adapters: &'a dyn AdapterSource,`

Where a reviewer's adapter is resolved from.

A seam and not a plan field: the plan says *which agent* a pass is bound
to, and this says how a name becomes an adapter. A plan carrying the
adapter would need a lifetime, and a plan is a value the driver builds
and holds across the appends it authorizes.

## `pub struct AttemptContext<'a>` › `pub paths: &'a RunPaths,`

The run's directories — where a review's settings and transcripts go.

## `pub struct AttemptContext<'a>` › `pub reviews: &'a dyn ReviewPasses,`

Where a review pass is executed. See [`ReviewPasses`].

## `pub struct AttemptContext<'a>` › `pub input_policy: &'a dyn ReviewInputPolicy,`

Whether the staged evidence is reviewable. See [`ReviewInputPolicy`].

## `impl AttemptContext<'_>` › `fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError> {`

Emit, discharging obligation (3) from this context's own ledger.

The seam hands back an [`EmitFailure`] because an ordering module may
not hold the ledger. This one does — it is the same ledger every Runner
process of the attempt registers in — so the obligation is discharged
here and the attempt half keeps returning [`UpstrokeError`].

## `fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError> {` › `self.emitter`

Three disjoint field borrows, which is why the hooks are taken from
`self` here rather than passed in: a caller writing
`self.emit(body, self.hooks)` borrows all of `self` twice.

## `impl AttemptContext<'_>` › `pub fn start(`

**O23, and O24's second half.** Append `attempt_started`, then spawn the
worker.

The append is first and the spawn is second, and nothing sits between
them: `T-ATTEMPT`'s boundary is "`attempt_started` (first or retry)
without terminal", and its sub-prefix (a) is "worker running, no
capture". A spawn that preceded the append would put a paid-for process
outside the log entirely, and the recovering process would find a
generation with no attempt and dispatch over the top of it.

### A retry appends through here too

The boundary says "first **or retry**", and one function serves both
because there is one event: a retry's `attempt_started` is one whose
`resume_session` is `Some` and whose [`AttemptPlan::attempt`] is a new
number, and `plan` carries both. What makes it a retry is the decision
taken *before* it — [`super::settle::retry`]'s reservation and
`Worktree.Verify` — and this function does not re-take it. O24's
"converted at `attempt_started(retry)`" is the caller's conversion of
that same reservation at this append.

A `retry` method here would be this function plus a second verification
of a worktree that has already been verified, and its refusal branch
would return while the reservation `settle::retry` took was still held —
never converted, and never cancelled, because cancellation lives in the
failure branch that the first, passing, verify did not take.

**This block used to sit above `fn emit`**, which `bcc5c2f` inserted
between it and this function with no blank line — so all of it attached
to `emit`, `start` rendered undocumented, and `emit`'s own two
paragraphs read as a continuation of the `# Errors` section below.
`PR7-R3-EMIT-004`, the doc-re-targeting class `reviews/FINDINGS.md` §4
records; split 2026-08-26 by moving `emit` above this block rather than
by moving the prose. **Above the `# Errors` heading, not after it**: the
first attempt appended it to the end of the block, and rustdoc renders
everything from a heading to the next heading as that section — so this
paragraph became part of `start`'s error contract, which is the same
rendering failure one level in. `PR7-R5-ATT-006`.

### Errors

Whatever the emitter returns; [`UpstrokeError::Refused`] from the slot
assertion or the invocation ledger; or a runner failure. A non-zero exit
is not an error — it is a [`ProcessOutput`] and the caller's to judge.

## `impl AttemptContext<'_>` › `pub fn run_worker(`

The worker half of [`Self::start`], without the append.

**A retry's `attempt_started` is appended by `settle::retry`**, after
`Worktree.Verify` — the verify is what makes the claim true, and the
fold refuses a second append for the same attempt. So the ready-retry
branch reaches the worker through here and the ready-dispatch branch
through `start`, which is that append plus this.

### Errors

Whatever the Runner returns.

## `impl AttemptContext<'_>` › `pub fn capture(&mut self, site: AttemptSite<'_>) -> Result<Capture, UpstrokeError> {`

**O25.** `Object.CandidateStage` then `Object.CandidateWriteTree`, in the
task worktree.

Two sites and not one, because a kill inside either leaves the same
residue class and a different amount of it: `T-ATTEMPT` sub-prefix (b')
is "git add or write-tree killed after writing objects and before
publishing the index or cache-tree". The staged objects are behind the
**task index** afterwards (R9), which is what makes them recoverable by
scrubbing the worktree rather than by anything cleverer.

### Errors

The containment refusals or a Git error.

## `impl AttemptContext<'_>` › `pub fn assess(`

The ladder's cheap rungs: **outcome sanity, then the diff.**

Both answers come from the production authorities rather than being
formed here — `engine::attempt::evaluate_outcome` for what the worker's
own report says, `engine::classify::diff_failure` for what the diff
alone says. `classify`'s doc already named this caller: "the schema-4
driver reads the same answer rather than forming its own, because
`ladder::next_step` reads the result and the allowance decision is
derived from it".

The third rung is here too, behind [`ReviewInputPolicy`]: staged
evidence whose bytes are not the bytes a gate would see — a clean/smudge
filter, or a dirty submodule behind an unchanged gitlink — cannot be
reviewed completely, and a driver that skipped the check would judge a
transformed blob and call it reviewed.

### Errors

Whatever the adapter's parse returns, or a refusal when the plan's agent
has no adapter.

## `impl AttemptContext<'_>` › `let mut failure = crate::engine::attempt::evaluate_outcome(&outcome, &run.worker);`

In the legacy order, and each rung guarded: a worker that died is not
asked what its diff looked like.

## `impl AttemptContext<'_>` › `if failure.is_none() {`

The third rung: is the staged evidence the evidence a gate would
see. Attributed to the reviewer, not the implementer — a filter or a
dirty submodule is a policy failure of the tree, and the classifier
that decides so is `run_attempt`'s, reused rather than restated.

## `impl AttemptContext<'_>` › `pub fn judge(`

**O26 and O27.** The gate set on one fresh exact snapshot, then each
reviewer on its own.

`decisions.workspace_candidates.snapshots`: "exact snapshot worktrees
(R24) are the only places gates and reviewers execute … one snapshot for
the gate set and one fresh snapshot per reviewer, **never reused across
roles or attempts**, cleaned on completion". The name carries the
generation, the attempt and the role, so "never reused" is a property of
[`SnapshotName`] rather than of this loop's discipline — and each
snapshot is removed before the next is created, so a reviewer cannot
inherit the previous one's checkout even by mistake.

### What the name does not carry, and where that is owed

The **task key**. [`SnapshotName::gates`] is `g<gen>-a<attempt>-gates`
and [`SnapshotName::review`] is `g<gen>-a<attempt>-review<pass>`, so two
*different tasks* at the same generation and attempt name the same slot.
"Never reused" is therefore a property of the name **within** a task and
a property of the substrate **across** tasks: at `max_parallel = 1` one
attempt runs to completion before the next begins, so no two live
snapshots can collide and the claim above holds exactly.

This is the same PR11 debt [`Self::settle_interrupted`] records for the
reclaim scope, reached from the other side — that one would remove a
sibling's snapshot, this one would hand a sibling the same slot to
create. A wider substrate owes the name a key; it is recorded here
rather than approximated, because a snapshot name is what
`WorkspaceManager` derives a path from and two tasks deriving one path
is a collision no assertion in this module could see.

O26 lives inside [`WorkspaceManager::add_snapshot`], which for a
tree-only input performs commit-tree → intent → add in that order. This
function's contribution to O26 is passing [`SnapshotInput::Tree`] — an
integration snapshot checks out an existing commit and creates no object,
and passing the wrong one here would silently skip the whole clause.

### Errors

The containment refusals, a Git error, a slot or ledger refusal, or a
runner failure.

## `impl AttemptContext<'_>` › `let mut failure = assessed.failure.clone();`

**The cheap rungs carry forward, and they short-circuit the expensive
ones.** `run_attempt` guards every rung with `failure.is_none()`, so
a worker that died or produced no diff is not asked to pass gates and
is certainly not shown to a frontier reviewer. Starting this at
`None` would run the whole verification ladder over an empty diff.

## `impl AttemptContext<'_>` › `log_tail: crate::util::tail(`

§11.1's tail, which is what §11.4 sends back to the
same rung. This was empty.

## `impl AttemptContext<'_>` › `if refused {`

**First failure ends the set**, which is `gates::run_all`'s own
shape: it `return`s the first `GateFailure` rather than running
the rest. Two consequences, and both matter. A refused gate
must not buy the gates after it — the diff is already
rejected. And the first cause must survive: a later gate that
fails to *spawn* would otherwise overwrite the real failure
with an infrastructure one, and `ladder::next_step` would
price the attempt from the wrong kind.

## `impl AttemptContext<'_>` › `if failure.is_some() {`

A failed gate is a rejection of the work, and a reviewer judging
a diff the gates already refused is a frontier invocation bought
to learn nothing. `run_attempt` short-circuits for the same
reason and §11.1 is the same sentence for gates.

## `impl AttemptContext<'_>` › `&invocations(pass),`

**Caller-supplied, ordinal and all.** Nothing pass-shaped is
minted here: PR8's merge verification is this machinery's
third caller and its identities come from
`SequenceIdentities`, not `AttemptIdentities`. A `judge` that
reached for either scheme would be a seam its next caller had
to redesign.

## `impl AttemptContext<'_>` › `let ids = invocations(pass);`

**R4, for the processes the seam ran.** `permits.protocol` is
"every Runner process is registered and settled exactly once",
and a review pass reaches the Runner through `run_review` with
the raw handle — so the worker and the gates were in the ledger
and the reviewers and their re-asks were not.

Reconciled after the call rather than before it, because the seam
owns the spawning and only its outcome knows how many processes
ran: `ReviewOutcome::invocations` counts the pass plus each
re-ask. Registering a re-ask up front would put an id in the
ledger for a process that may never exist, which is the opposite
failure.

The ids are the caller's own — the same `ReviewInvocations` the
pass was given — so this records what ran under the names it ran
under rather than minting new ones.

## `impl AttemptContext<'_>` › `let unavailable = matches!(outcome.result, review::ReviewResult::Unavailable { .. });`

Read before the result is consumed: a judge that never ran is not
a judge that said no, and the ledger has to show which happened.

## `impl AttemptContext<'_>` › `fn snapshot(`

One exact snapshot of the captured tree, on the recorded parent.

## `impl AttemptContext<'_>` › `pub fn settle_interrupted(`

`T-ATTEMPT`'s resume action, for a coordinator that died or a run that
halted.

> "append `attempt_interrupted` (unknown spend, allowance refunded,
> generation Closed, lease by kind); discard residue: snapshot intents
> and worktrees reclaimed (releasing an ephemeral commit to R27), the
> task worktree scrubbed with force (releasing staged objects to R27 and
> removing any index.lock or other administrative residue); an ephemeral
> commit without a snapshot and objects written by an interrupted capture
> are left to Git (nothing to reclaim)"

The event is first and the reclaim second, for the same reason a run-end
closure is: until the terminal is durable the log still says this attempt
is in flight, and a scrub before it would remove the worktree the next
resume would look for.

"Nothing to reclaim" is load-bearing and is why this function does not
go looking for unreferenced objects. An ephemeral commit written before
its intent, and blobs written by a killed `git add`, are R27 — **Git's**
— and an engine that pruned them would be establishing authority over
the object store, which `cleanup` forbids ("cleanup is expected-path,
contained, idempotent, and never establishes authority").

### Scope of the snapshot reclaim

Every snapshot intent of this execution root, not only this attempt's.
That is what the sentence above says, and at `max_parallel = 1` it is
exact: the sequential substrate runs one attempt to completion, so the
snapshot namespace holds this attempt's snapshots and nothing else. PR11
widens the substrate and owes this a per-attempt scope; it is recorded
here rather than approximated, because a reclaim that quietly removed a
sibling's snapshot would take its gates down with it.

### Errors

Whatever the emitter returns, or a Git or I/O error from the reclaim.

## `impl AttemptContext<'_>` › `pub fn cancel_in_flight(`

The source retains the cancellation ordering protocol required by §10.

The cancellation half of `T-ATTEMPT`: "at Halted the same terminal is
appended by cancellation".

`permits.protocol`: "cancel(invocation_id): a pending slotted request is
removed …; a granted or non-slotted running invocation is cancelled
**after the Runner terminated its process or container**". In the
sequential substrate `Runner::run` is synchronous, so a halt observed by
this coordinator is observed between invocations and there is no live
child to terminate — but the *ledger* may still carry a registration
whose completion never ran, and leaving it registered would make the
process-end balance check pass over a leak. So the ledger is cancelled
first, the slot released with it, and the terminal appended after.

### Errors

As [`Self::settle_interrupted`].

## `impl AttemptContext<'_>` › `if let Some(held) = self.slots.held().cloned() {`

Released without naming an invocation, because what is being
cancelled is whatever this process still holds. Naming one would be
asserting *which*, and a halt is the moment that assertion is least
safe: the pair may belong to the worker, to a reviewer, or to a
re-ask, and a guess that missed would leave the ledger unbalanced at
process end with nothing to say so.

## `impl AttemptContext<'_>` › `fn discard_residue(&mut self, dispatched: &Dispatched) -> Result<(), UpstrokeError> {`

Snapshots reclaimed, then the task worktree scrubbed with force.

## `impl AttemptContext<'_>` › `fn execute(`

The source retains the registration, slot and settlement protocol required by §10.

Register, take the slot pair if the identity is slotted, run, release,
complete.

`permits.protocol` in order: "register(invocation_id, slots) -> if
slotted, wait for the atomic pair grant … -> Runner spawn ->
complete(invocation_id) releasing any slots". Nothing waits here: at
`max_parallel = 1` a second concurrent slotted acquisition is a leaked
hold rather than contention, and [`SlotAssertion`] refuses it.

### The registration is settled on every path out

The register is here and the pair, the run and the release are in
[`Self::run_registered`], which is the whole reason the two are separate
functions. `permits.protocol` settles an invocation "exactly once", and
[`InvocationLedger::balances`] states that as "no entry is `Running`" —
so a `?` between the register and the settlement would abandon a
`Running` entry that at process end is **indistinguishable** from a
process this coordinator genuinely lost. A slot pair the assertion
refuses is not a lost process; it is a process that never started, and
reporting it as a leak would spend a real signal on a bookkeeping
mistake.

## `impl AttemptContext<'_>` › `Ok(output) => {`

`permits.protocol` settles an invocation exactly once, and the
two settlements are not interchangeable: a process the Runner
could not start or supervise never completed, and recording it as
completed would put a failure in the ledger under the name of a
success.

## `impl AttemptContext<'_>` › `drop(self.ledger.cancel(&request.invocation));`

Cancelled, not completed: the protocol failed before the
Runner answered, so nothing ran. It cannot itself fail —
`cancel` refuses only an identity that was never registered,
and the line above registered this one — and the failure that
brought us here is the one worth reporting either way.

## `impl AttemptContext<'_>` › `fn run_registered(`

Everything between the registration and its settlement: the pair, the
run, and the release.

The nesting keeps the two failures apart, the way
[`WorkspaceManager::verify_worktree`] keeps its two apart. The **outer**
error is a protocol failure — a slotted request naming no agent, a pair
the assertion refuses, a release that did not match — and means no
process ran, so [`Self::execute`] cancels the registration. The
**inner** one is the Runner's own answer about a process it could not
start or supervise, and is what decides `complete` against `cancel`.

Every early return here is therefore safe: this function may fail
anywhere and the ledger entry is still settled exactly once, by its
caller.

## `impl AttemptContext<'_>` › `fn verdict(`

[`Self::execute`], reduced to what a judgement records.

## `impl AttemptContext<'_>` › `log: format!("{}{}", output.stdout, output.stderr),`

Both streams, in the order `gates::run_all` joins them: a gate's
diagnostic is as often on stderr as on stdout.
