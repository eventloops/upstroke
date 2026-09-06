# `src/topology/fold.rs`

Extended notes for [`src/topology/fold.rs`](../../../src/topology/fold.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The checked fold: one transition function for a live run and for a replay.

[`TopologyFold::plan_transition`] checks an event against the current fold
and returns an opaque [`TopologyDelta`]. Private construction guarantees
that a delta was checked, but does not guarantee freshness or single use.
A cloned delta, or two deltas planned before either is applied, can become
stale. [`TopologyFold::apply_delta`] does not recheck them.

A live emission is `plan_transition` → append the exact bytes → `apply_delta`
only after the append returned `Ok`. Apply the delta exactly once to the
fold that checked it, before planning or applying another transition.
[`TopologyFold::replay`] uses those same calls once per event in order,
with the append removed. This protocol preserves INV-02 and DESIGN §26's
single transition implementation for live execution and replay.

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

This module performs no I/O or Git effects. The schema-4 emitter owns the
append protocol, while this fold checks transitions and derives run state.

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

What raised a question. Answer state is derived from current fold facts,
including outstanding questions, queued work and unelapsed backoff.

## `pub enum QuestionOrigin` › `VerificationPark,`

A verification could not be run. Its queued candidate remains available
for verification under a new sequence after the lineage's last answer.

## `pub enum QuestionOrigin` › `Admission,`

An attempt parked, a repair's admission is gated, or a bare question was
raised. The origin alone does not determine the task's next state.

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
these tests live/replay agreement for the event trace being compared.

## `pub struct RunState` › `seen_questions: BTreeSet<QuestionId>,`

Every question id this log has used, open or not: an id is never reused.

## `pub struct RunState` › `deferred_tasks: BTreeSet<TaskKey>,`

Execution backoff still owed, including tasks parked on questions.
Accepted settlements add keys; elapsed waits and resume clear them.

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

Construction is private, so each delta came from
[`TopologyFold::plan_transition`]. The caller must apply it once to the
same fold, with no intervening transition. Cloning does not renew that
precondition, and applying a stale or duplicate delta is not checked.
On the live path, append its event successfully once before applying it.

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

The checked question's origin, retained before application removes it.

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

Whether `event` may be applied to this state, and what applying it does.

The returned delta is for this fold in its current state. On success,
append its exact event and apply it once before any other transition.
Planning a second delta does not reserve either transition or make the
deltas safe to apply successively without checking again.

### Errors

The [`FoldError`] naming what the event disagrees with. A refusal is a
statement about the pair — this event against this state — and never a
statement that the event is malformed in isolation, which is
serialization's business.

## `pub fn plan_transition(&self, event: &TopologyEvent) -> Res…` › `if self.poisoned {`

refusals[24]: a process whose fold is poisoned by a returned append
error attempts no further transition. The command has already ended.

## `impl TopologyFold` › `pub fn apply_delta(&mut self, delta: TopologyDelta) {`

Apply a delta once to the fold that checked it, with no intervening
transition. On the live path its exact event must first be appended
successfully once; replay applies it once for the corresponding record.

These are caller preconditions. This method does not validate freshness,
reject duplicate application or verify that an append occurred.

## `impl RunState {`

---------------------------------------------------------------------------
RunState: the checks
---------------------------------------------------------------------------

## `impl RunState` › `fn pipeline_held(&self) -> usize {`

The pipeline entitlement this state holds.

## `fn predicted_region(entry: &TaskEntry) -> PathSet {`

The region an ordinary dispatch of this entry would predict.

One [`hint_prefix`] per frozen path hint. An absent hint list, or one hint
that bounds nothing, classifies the whole entry repo-wide, which overlaps
everything.

## `fn hint_prefix(hint: &str) -> Option<GitPath> {`

The bounded prefix one path hint predicts, or `None` for repo-wide.

`DESIGN.md` §26 states the contract: dispatch leases "use normalized non-glob
prefixes from `path_hints`; prefix ancestor/descendant pairs overlap, and an
absent or repo-wide hint takes a global lease", deliberately conservative.
[`crate::topology::leases::paths_overlap`] compares two paths **component by
component**, so the prefix has to be an ancestor in that sense and not merely
a leading substring, and every departure from it has to be wider than the
hint rather than narrower — a region silently smaller than what the hint
covers is what lets two owners of one file run at once.

Three consequences, each of them a case this rule gets right and a character
prefix does not:

* Components are taken whole, up to the first that carries `*`, `?`, `[` or
  `{`. A metacharacter inside a component drops that component: `src/eng*`
  bounds `src`, not `src/eng`, which is not an ancestor of the
  `src/engine/mod.rs` the hint matches.
* A `.` or `..` component bounds nothing. The comparator does not equate
  `src/./alpha` with `src/alpha`, so keeping the dotted spelling would be a
  second spelling of one region that never overlaps its first.
  `src/workspace_manager/parsers.rs` refuses the same shapes on the actual
  paths, for the same reason.
* An empty component is kept, because the comparator filters empties itself:
  `src/doubled//inner/` bounds `src/doubled//inner` byte for byte, and a
  leading or trailing separator changes no comparison.

Backslashes become `/` first, so a hint written on Windows bounds the same
region. A hint that bounds nothing after all that — no components, or every
component globbed — is `None`.

## `mod parent_tests {`

The tests for the items this file itself holds.

`src/topology/fold/tests.rs` is the `fold` family's shared suite and reaches
these items through the checkers; this module is the parent's own, for the
contracts that are stated here and nowhere else: every [`FoldError`]
message's fields and their distinctness, the [`TaskState`] and
[`GenerationClass`] vocabularies, [`TaskFold::open`], and [`hint_prefix`]'s
rule with the [`crate::topology::leases::paths_overlap`] measurement that
says why it is that rule.
