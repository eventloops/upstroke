# `src/topology/fold.rs`

Extended notes for [`src/topology/fold.rs`](../../../src/topology/fold.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The checked fold: one transition function for a live run and for a replay.

**INV-02 — an invalid transition is never appended, and never applied.**
[`TopologyFold::plan_transition`] decides whether an event may be applied
and returns a [`TopologyDelta`] when it may; [`TopologyFold::apply_delta`]
is the only thing that changes the state, and a `TopologyDelta` is the only
thing it accepts. The delta has no public constructor, so there is no way to
reach the state except through the check — which is what makes "the live run
and the replay use one transition" a property of the types rather than a
convention two call sites are expected to keep.

A live emission is `plan_transition` → append the exact bytes → `apply_delta`
only after the append returned `Ok`. A replay is
[`TopologyFold::replay`], which is those same two calls per event with the
append taken out. Nothing else exists.

### What the fold refuses

Everything in `decisions.schema_compatibility.refusals`, less the four the
header probe answers before a fold exists ([`crate::topology::schema`]).
The refusals are not a validation pass bolted onto a fold: they *are* the
fold, because a transition this module cannot state the effect of is a
transition it must not pretend to have applied.

Three of them are worth naming here because they are relations rather than
shapes, and a reader looking for them in one event will not find them:

* **The publication relations** (INV-09). A `merge_prepared` is checked
  against the candidate's own record, the pinned proposal, and the head the
  verification read — three records elsewhere in the log.
* **The derived outcome** (INV-15). `run_finished` carries an outcome, and
  the fold accepts it only when it equals [`TopologyFold::derived_outcome`],
  which is computed from durable state alone and never consults spend,
  capacity, or runner availability.
* **Queue order** (`decisions.coordinator_integration.queue`). An
  integration may only start for the first *eligible* candidate, which is
  not the same as the first queued one.

### What it does not do

No production path writes or reads a schema-4 log yet, and nothing here
performs an effect: no ref moves, no worktree is created, no report is
written. The fold decides what a log *means*; the effects that log
authorizes, and the typed sites they run through, arrive in later slices.

## `pub enum FoldError {`

---------------------------------------------------------------------------
Refusals
---------------------------------------------------------------------------

## `pub enum FoldError {`

Why a transition was refused.

Every message names the record it refused and the value it disagreed with,
because a fold error reaches an operator as "your log is invalid" unless it
says which line and which field.

## `pub enum TaskState {`

---------------------------------------------------------------------------
Fold state
---------------------------------------------------------------------------

## `pub enum TaskState {`

What a task is doing, as the log says.

The topology's own states, not [`crate::events::TaskState`]: a task with an
open generation is `Pending` here and is kept out of admission by the
generation rather than by a state of its own, because the thing that has to
be closed before the run may end is the generation.

## `pub enum TaskState` › `Pending,`

Runnable once its dependencies are merged and nothing else holds it.

## `pub enum TaskState` › `AwaitingMerge,`

A candidate exists and is queued for integration.

## `pub enum TaskState` › `AwaitingRepair,`

Its candidate was rejected and a repair carries it.

## `pub enum TaskState` › `AwaitingInput,`

Parked on a question.

## `pub enum TaskState` › `Deferred,`

Backing off after an outage, until `defer_wait_elapsed` or a resume.

## `pub enum TaskState` › `Merged,`

Its work is in the integration ref.

## `pub enum TaskState` › `Failed,`

Terminal.

## `pub enum GenerationClass {`

Where one generation of one task is.

## `pub enum GenerationClass` › `OpenNoAttempt,`

Dispatched; no attempt has started.

## `pub enum GenerationClass` › `InFlight { attempt: AttemptNumber },`

An attempt is running.

## `pub enum GenerationClass` › `RetainedIdle {`

Settled holding a session, for a same-session retry by the incarnation
that retained it.

## `pub enum GenerationClass` › `Promoting,`

An attempt succeeded; the candidate is being promoted to its
authoritative ref.

## `pub enum GenerationClass` › `Closed,`

Over.

## `impl GenerationClass` › `fn holds_pipeline(&self) -> bool {`

Whether this generation holds a pipeline entitlement.

## `impl GenerationClass` › `fn blocks_run_end(&self) -> bool {`

Whether the run may end while this generation is in this class.

## `pub struct GenerationFold {`

One generation of one task.

## `pub struct GenerationFold` › `pub base_sha: CommitSha,`

The commit the worktree was created at.

## `pub struct GenerationFold` › `pub attempts: u32,`

The highest attempt number started in this generation.

## `pub struct GenerationFold` › `pub candidate: Option<PreparedCandidate>,`

The candidate this generation prepared, once it has.

## `pub struct PreparedCandidate {`

What `candidate_prepared` recorded, kept for the relations a publication is
checked against.

## `pub struct PreparedCandidate` › `pub base_sha: CommitSha,`

The base the work started from, and the parent of the commit.

## `pub struct PreparedCandidate` › `pub tree_sha: CommitSha,`

The tree the gates ran against and the reviewers judged.

**Retained because adoption is about identity, not existence.**
`DESIGN.md` §15 requires `candidate_prepared` to record "exactly one
complete attempt/base/commit/tree identity ... so resume adopts only
that exact shape". The tree was on the event and stopped here: recovery
could check that the object exists and that its parent is the recorded
base, and a commit with that parent and a **different tree** passed —
so a resume could publish an object no gate ran against and no reviewer
read. `candidate.rs`'s own comment recorded the gap rather than closing
it, because closing it is this field.

Per-instance **Class B** approval, granted 2026-08-26 against the
frontier re-review of `c2c0294`, finding B; the ledger row is
`reviews/FINDINGS.md` §3 and `PR7-CANDIDATE-TREE-UNVERIFIED` in §2.
Nothing serde-visible moves — `CandidatePrepared::tree_sha` already
exists on the wire and this is the fold keeping what it reads. It
conforms to §15 rather than amending it.

## `pub struct TaskFold {`

One task's fold state.

## `pub struct TaskFold` › `pub defers: u32,`

How many times an attempt on this task has settled `Deferred`.

**The fold owns this count because only the fold survives a resume.**
`ladder::next_step` reads it on exactly one branch — an outage defers
while `defers < max_defers` and parks at it — and a driver keeping its
own tally would restart at zero in the next process while the log still
held the deferrals, so a run that had already exhausted its allowance
would defer forever. The legacy engine keeps it in
`state.progress[index].defers`, which is in-memory schema-3 state; a
schema-4 run derives everything by replay, so this is derived by replay.

Read through the existing [`TopologyFold::task`] reader. It is a field
rather than a twelfth reader for that reason.

`max_defers` is **not** here: the ceiling is policy and stays in
`ladder::LadderPolicy`, read from `run_started(4).limits`. This is the
count, and only the count.

## `pub struct TaskFold` › `pub rung: u32,`

The rung this task's **next** attempt runs at.

**The fold owns it because a task's ladder position survives a resume.**
A settlement that escalates closes the generation and leaves the task
`Pending`, so the ready-dispatch branch selects it again — at a rung the
driver has no other way to know. A driver-side tally reads zero in the
next process while the log holds the escalation, so the task is
dispatched on rung 0 forever and never reaches the tier its chain
escalated it to.

`SettlementTransition::Escalated { rung }` is the durable answer — the
packet defines it as the rung an escalation climbs *onto* — so this is
assigned from it, never computed.

## `pub struct TaskFold` › `pub attempts_on_rung: u32,`

Attempts already spent at [`Self::rung`].

Not `GenerationFold::attempts`: that counts one generation, and attempts
at one rung span generations — a same-rung retry that does not resume
closes its generation and opens a fresh one at the same rung. Feeding
`LadderState::attempts_on_rung` the per-generation count makes
`next_step` see the first attempt of the allowance every time, so a task
retries forever and never escalates.

Reset by an escalation, because the allowance is per rung.

## `impl TaskFold` › `fn open(&self) -> Option<&GenerationFold> {`

The generation that is not closed, if any. At most one exists: a new one
is only opened when the previous closed.

## `pub enum QuestionOrigin {`

Why a question is open, which is what decides where its answer returns the
task to.

## `pub enum QuestionOrigin` › `VerificationPark,`

A verification could not be run. An answer returns the task to awaiting
merge, to be re-verified under a new sequence.

## `pub enum QuestionOrigin` › `Admission,`

An attempt parked, or a repair's admission is gated. An answer returns
the task to pending.

## `pub struct OpenQuestion {`

An open question and what raised it.

## `pub struct OpenQuestion` › `pub binding: Option<Vec<String>>,`

The frozen binding options this question's admission authorized, for a
`HumanBinding` admission and for nothing else.

`decisions.task_registry.binding_override` validates an override
"against the frozen options of that task's open `HumanBinding`
question", so the authority has to survive from the `task_spawned` that
froze it to the `question_answered` that draws on it. Kept here rather
than re-read from the registry entry because it is the *question's*
authority: two questions of one task are answered separately and only
one of them ever authorized a binding.

## `pub enum TransactionClass {`

Where an integration transaction is.

## `pub enum TransactionClass` › `VerificationStarted {`

A verification is running against a recorded head.

## `pub enum TransactionClass` › `Prepared {`

The publication is authorized and the ref move is owed.

## `pub struct Transaction {`

The one unresolved integration transaction, if there is one.

## `pub struct RunState {`

Everything one topology run has recorded.

`PartialEq` and not `Eq`: the run record it holds carries the reported
spend of a budget stop, and a float has no total equality. Comparing two of
these is how a live fold and a replayed one are proved identical (INV-02).

## `pub struct RunState` › `seen_questions: BTreeSet<QuestionId>,`

Every question id this log has used, open or not: an id is never reused.

## `pub struct RunState` › `halted_epoch: Option<Epoch>,`

The epoch the halting settlement was recorded in. `halted_at` is never
cleared, and the answer-ingestion refusal is epoch-scoped.

## `pub struct FrozenInputs {`

The frozen inputs a fold is derived against.

Both are read before the first event: the plan the run normalized, and the
digest of the exact bytes it was normalized to. The fold rebuilds the
registry from the plan and refuses a `run_started` whose recorded digests do
not match, which is the whole of `refusals[4]` — a plan that moved
underneath a log is refused rather than folded on a guess.

## `pub struct FrozenInputs` › `pub normalized_plan_digest: String,`

Digest of the exact `plan.normalized.json` bytes, in the
`sha256:<hex>` shape the registry digest uses.

## `pub struct TopologyDelta {`

One checked transition, ready to apply.

Deliberately opaque and deliberately unconstructible outside this module:
[`TopologyFold::apply_delta`] takes one of these and nothing else, so the
only path into the state runs through [`TopologyFold::plan_transition`].
That is INV-02 expressed as a type rather than as a rule two call sites are
asked to remember.

## `impl TopologyDelta` › `pub fn event(&self) -> &TopologyEvent {`

The event this delta applies. Readable so a caller can append the exact
bytes it checked.

## `enum Derived {`

What the check derived and the application would otherwise have to look up
again.

## `enum Derived` › `Registry(Box<TaskRegistry>),`

The registry rebuilt from the frozen plan and this record, already
authenticated against the recorded digest.

## `enum Derived` › `Answer(QuestionOrigin),`

Where an answered question returns its task to.

## `pub struct TopologyFold {`

The state of one topology run, and the only way to change it.

## `impl TopologyFold` › `pub fn new(inputs: FrozenInputs) -> Self {`

A fold over a run that has recorded nothing yet.

## `impl TopologyFold` › `pub fn replay(inputs: FrozenInputs, events: &[TopologyEvent]) -> Result<Self, FoldError> {`

Fold `events` from nothing, refusing the first transition that does not
apply.

This *is* the live path with the append removed: one `plan_transition`
and one `apply_delta` per event, in order. There is no second reader.

### Errors

The [`FoldError`] of the first event that does not apply.

## `impl TopologyFold` › `pub fn plan_transition(&self, event: &TopologyEvent) -> Result<TopologyDelta, FoldError> {`

-----------------------------------------------------------------------
The transition
-----------------------------------------------------------------------

## `impl TopologyFold` › `pub fn plan_transition(&self, event: &TopologyEvent) -> Result<TopologyDelta, FoldError> {`

Whether `event` may be applied to this state, and what applying it does.

### Errors

The [`FoldError`] naming what the event disagrees with. A refusal is a
statement about the pair — this event against this state — and never a
statement that the event is malformed in isolation, which is
serialization's business.

## `pub fn plan_transition(&self, event: &TopologyEvent) -> Res…` › `if self.poisoned {`

refusals[24]: a process whose fold is poisoned by a returned append
error attempts no further transition. The command has already ended.

## `impl TopologyFold` › `pub fn apply_delta(&mut self, delta: TopologyDelta) {`

Apply a checked transition. Total: every value it needs was decided by
the check that produced the delta.

## `impl RunState {`

---------------------------------------------------------------------------
RunState: the checks
---------------------------------------------------------------------------

## `impl RunState` › `fn pipeline_held(&self) -> usize {`

The pipeline entitlement this state holds.

## `fn predicted_region(entry: &TaskEntry) -> PathSet {`

The region an ordinary dispatch of this entry would predict.

The plan's path hints, taken literally: a hint with no glob metacharacter is
its own literal prefix. Anything else — an absent hint list, or a hint whose
literal prefix is empty — classifies repo-wide, which overlaps everything.
