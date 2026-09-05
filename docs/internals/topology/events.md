# `src/topology/events.rs`

Extended notes for [`src/topology/events.rs`](../../../src/topology/events.rs).

The source defines behavior; these notes hold the module's contracts and rationale.
Each code span in a section heading is an exact source fragment. Search it as a fixed string
in the linked module, using the enclosing item to distinguish repeated lines.

## Module

What a schema-4 run records: the complete parallel-topology vocabulary.

This module is the *shape* of the topology log and nothing else. Which
transitions are legal, and what state each one produces, is the checked
fold's — it shares this vocabulary and arrives beside it. Keeping the two
apart is not tidiness: the fold is the thing a live run and a replay must
reach identically (INV-02), and it can only be one function over one set of
types if those types exist without it.

### What changed from schemas 1–3

**Identity is stored once.** Legacy events hoist `task`, `attempt`, `rung`
and `profile` beside the tag so the raw file is greppable, and pay for it
with a class of refusal that exists purely to catch an envelope disagreeing
with its own payload. Schema 4 records identity in the payload only, and
restores the routing question as a total function over the vocabulary
([`TopologyEventBody::key`], [`TopologyEventBody::sequence`]). A hoisted
field that contradicts the record it sits on is not refused here; it is
unrepresentable.

**Tasks are addressed by [`TaskKey`], not by display id.** A run that
spawns repair tasks has ids nobody wrote in the plan, so every relation —
dependencies, leases, queue positions, questions, overrides — is keyed on
the dense index the registry assigned.

**The run has an execution identity beyond its plan.** [`RunnerPolicy`] is
resolved once, before the worktree lock, and recorded in `run_started`;
every later incarnation rebuilds it and records in `run_resumed` what it
established, which must equal the original exactly (INV-23). That is why
[`RunnerPolicy::difference`] names *which* field moved rather than
returning a bool: an operator whose container reference now points at a
rebuilt image needs to be told that, not told "runner mismatch".

**Nothing here is optional-for-legacy.** Schemas 1–3 carry `Option` fields
whose `None` means "a log written before this record existed", because they
grew. Schema 4 has no ancestors — there is no upgrade into it
([`crate::topology::schema::check_upgrade_transition`]) — so every `Option`
in this module is a real choice a writer made, and every absent field is a
refusal rather than a default.

### Unknown fields

Every payload defined here denies unknown fields, **recursively**: a
transaction carrying something this binary does not understand is a
transaction it cannot claim to have applied, and a refusal that stopped at
the top of `data` would only be skin deep. So the rule holds at the
envelope (`ts`, `event`, `data` and nothing else), at each payload, at every
nested struct, and at every data-carrying variant of every nested enum.

Informational events ([`TopologyEventBody::CapacitySnapshot`] and its
neighbours) stay lenient *inside their payload*, because ignoring an extra
column in a record nothing folds on costs nothing.

Records reused from schemas 1–3 ([`AttemptRecord`], the review plan, the
frozen registry entry) keep the leniency they have always had **when a
schema-1..3 log is read**: tightening their own types would change how a
legacy log reads, which this slice must not do. Inside a schema-4
transaction they are decoded through the `strict` door instead — the same
type, reached through a stricter decoder — because `refusals[24]` names the
payload, not the type, and grants no legacy-nested exception.

Schema-4 strictness for records schemas 1–3 also read.

`refusals[24]` refuses an unknown field in a topology transaction
payload, and a payload embeds records the legacy schemas defined. Those
types cannot gain `deny_unknown_fields` of their own — that would change
how a schema-1..3 log reads, and the legacy-unchanged invariant is about
the *decoder a legacy log gets*, not about which fields schema 4 accepts.
So the strictness is attached to the schema-4 field with
`#[serde(deserialize_with = ...)]`, leaving both the embedded type and
every legacy call site untouched.

The check is a *witness comparison*: decode the record, encode it again,
and report any key the input carried that the record did not claim back.
That is exact whenever the embedded type serializes every field it
deserializes — true of every record schema 4 embeds, none of which uses
`skip_serializing_if` (pinned by
`a_known_null_survives_the_strict_door_and_an_unknown_null_does_not`).
It is deliberately not a hand-copied field list: a list would be a second
declaration of the same shape, and the two would drift.

## `pub(crate) mod strict` › `fn unclaimed(input: &Value, echo: &Value, at: &str, found: &mut Vec<String>) {`

Collect every path in `input` that `echo` did not claim back.

## `pub(crate) mod strict` › `fn checked<E, T>(input: Value) -> Result<T, E>`

Decode `T`, refusing any field it does not claim.

## `pub(crate) mod strict` › `pub(crate) fn field<'de, D, T>(deserializer: D) -> Result<T, D::Error>`

A single embedded record.

### Errors

Whatever `T` refuses, plus any field of the input `T` does not claim.

## `pub(crate) mod strict` › `pub(crate) fn boxed<'de, D, T>(deserializer: D) -> Result<Box<T>, D::Error>`

An embedded record behind a `Box`.

### Errors

As [`field`].

## `pub(crate) mod strict` › `pub(crate) fn list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>`

A list of embedded records.

### Errors

As [`field`], for any element.

## `pub(crate) mod strict` › `pub(crate) fn optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>`

An embedded record a writer may have recorded as absent.

### Errors

As [`field`], when one is present.

## `pub(crate) mod strict` › `pub(crate) fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>`

An optional field whose *key* is still required.

Serde reads a missing `Option` field as `None`, which is the right
default for schemas that grew — an absent key there means "written
before this field existed". Schema 4 has no ancestors, so an absent key
means only that the record is incomplete, and `None` is a choice a
writer made and wrote down as `null`. Naming a `deserialize_with` is
what makes serde treat the missing case as `missing_field` instead.

### Errors

Whatever `T` refuses. The absent-key refusal is serde's, not this
function's: it is never called for a key that is not there.

## `pub struct GenerationId(pub u32);`

---------------------------------------------------------------------------
Identities
---------------------------------------------------------------------------

## `pub struct GenerationId(pub u32);`

Which attempt-carrying generation of a task this is: a worktree, a base
commit, and a lease. Dense from 0 per task.

A task gets a new generation when it is dispatched again from a fresh
worktree, and keeps the one it has across a same-session retry — which is
exactly the distinction the retry rule turns on.

## `pub struct AttemptNumber(pub u32);`

Which attempt within a generation. Dense from 1, as the ladder counts them.

## `pub struct SequenceId(pub u32);`

Which integration transaction. Dense from 0 across the whole run, so a
re-verification after an interruption is a new sequence rather than a
second use of an old one.

## `pub struct IncarnationId(pub String);`

Which coordinator process is driving the run: a ULID minted per process.

Not the resume count. Two incarnations can share an epoch only if the run
lock failed, which it cannot, but a container name, an intent path, and a
retained session all have to be attributable to the exact process that
created them, and "the third resume" does not identify a process.

## `pub struct Epoch(pub u32);`

How many times the run has been picked up again. The scope a budget stop
lives in: `budget_exceeded` sets it, `run_resumed` clears it by starting a
new one.

## `pub struct SessionId(pub String);`

An agent CLI conversation an attempt may resume (§11.4).

## `pub struct CommitSha(pub String);`

A full commit sha. Full, never abbreviated: `--short` length varies with
`core.abbrev`, and every relation in the merge queue is an equality.

## `pub struct GitRef(pub String);`

A full ref name (`refs/...`). Distinct from [`CommitSha`] so that a
relation between a ref and a sha cannot be written by accident.

## `pub struct CandidateRef {`

One candidate commit, named by the task and generation that produced it.

## `pub struct CandidateRef` › `pub commit_sha: CommitSha,`

The immutable commit the gates and reviewers judged.

## `pub struct CandidateRef` › `pub candidate_ref: GitRef,`

`refs/upstroke/runs/<id>/candidates/<key>/<gen>` — the authoritative ref
that keeps it reachable and is the protected source a repair is
materialized from.

## `pub enum RunnerKind {`

---------------------------------------------------------------------------
Runner identity (INV-23)
---------------------------------------------------------------------------

## `pub enum RunnerKind {`

Where a run's processes execute.

## `pub enum RunnerContract {`

The mount, environment, Git-view and supervision contract the binary
implements for a [`RunnerKind`].

Versioned separately from the kind because the contract can change while
the kind does not, and a run resumed by a binary implementing a different
contract is a run whose second half executes somewhere else.

## `impl RunnerContract` › `pub fn kind(self) -> RunnerKind {`

The kind this contract is the contract *for*.

## `pub struct ImageIdentity {`

The image a container runner executes from.

Three values rather than one, because they answer three different
questions. The `reference` is what an operator wrote and what a registry
may re-point at any time; the `id` is what the runtime actually holds and
is what every container of the run is created from, so a moved reference
cannot change what executes; the `digest` is what the registry called that
content, when it said so at all.

## `pub struct ImageIdentity` › `pub id: String,`

The runtime's immutable image id.

## `pub struct ImageIdentity` › `pub digest: Option<String>,`

The manifest digest, when the runtime reported one.

## `pub struct RunnerPolicy {`

The execution identity of a schema-4 run.

Resolved once by read-only inspection before the worktree lock, digested
into the marker, recorded in the private owner record before the first
probe, and recorded here in `run_started`. Every later incarnation rebuilds
it from this record and records what it established in `run_resumed`, which
must equal this exactly.

## `pub struct RunnerPolicy` › `pub image: Option<ImageIdentity>,`

`None` for a host runner: there is no image, and recording an empty one
would make "no image" and "an image nobody identified" the same record.

## `pub struct RunnerPolicy` › `pub credential_volumes: Option<BTreeMap<String, String>>,`

Per-agent credential volume names, for a container runner.

A map rather than a list so that the *set* is what equality compares:
two incarnations that enumerated the same volumes in different orders
established the same runner, and refusing that would make a resume
depend on iteration order.

## `pub enum RunnerField {`

Which part of a runner record two incarnations disagree about.

Ordered as the record is read, so the first difference reported is the
most structural one: a run that changed kind has not merely moved its
image.

## `pub enum RunnerRecordDefect {`

What a runner record is missing, or says inconsistently.

## `pub enum RunnerRecordDefect` › `ContractDoesNotMatchKind,`

The contract version does not belong to the recorded kind.

## `pub enum RunnerRecordDefect` › `ContainerWithoutImage,`

A container runner without an image record: nothing names what executes.

## `pub enum RunnerRecordDefect` › `ImageNotIdentified,`

An image record whose reference or id is empty.

## `pub enum RunnerRecordDefect` › `ContainerWithoutCredentialVolumes,`

A container runner without a credential-volume record. An empty map is
a real answer — no agent needs credentials — and is not this.

## `pub enum RunnerRecordDefect` › `HostWithContainerFields,`

A host runner carrying an image or volumes it cannot have used.

## `impl RunnerPolicy` › `pub fn completeness(&self) -> Result<(), RunnerRecordDefect> {`

Whether this record names everything needed to re-establish the runner.

A shape check over the record alone — whether the runtime still *has*
that image is an observation, made by the incarnation that rebuilds it.
The digest is deliberately not required: it is the manifest digest when
the runtime reported one, and runtimes that report none are not thereby
unusable. It is still compared by [`Self::difference`], because a
record that gained or lost one changed.

### Errors

The first [`RunnerRecordDefect`] the record exhibits.

## `impl RunnerPolicy` › `pub fn difference(&self, other: &Self) -> Option<RunnerField> {`

The first field in which `self` and `other` are not the same runner, or
`None` when they are identical.

This is the whole of the resume check: a run's boundary and image are
fixed for its life, so any difference at all refuses. It names the
field because the three ways this happens in practice — a config edit,
a moved tag, a rebuilt image behind an unchanged tag — are indistinguishable
from "runner mismatch" and have completely different fixes.

## `pub struct TopologyLimits {`

---------------------------------------------------------------------------
Run-level records
---------------------------------------------------------------------------

## `pub struct TopologyLimits {`

The ceilings a run froze, and every later fold reads rather than re-derives.

Budgets are not here on purpose: a ceiling on one's own spending is checked
against today's configuration by the loop, and a resume is allowed to raise
it. These three shape what the fold *permits*, which is identity.

## `pub struct TopologyLimits` › `pub max_parallel: u32,`

The global pipeline entitlement.

## `pub struct TopologyLimits` › `pub max_defers: u32,`

How many times one integration may be deferred by an outage before the
next outage parks it for a human instead.

## `pub struct TopologyLimits` › `pub max_merge_repairs: u32,`

How many automatic repairs one lineage root may consume.

## `pub struct RunStarted4 {`

`run_started` for a parallel-topology run.

Everything schemas 1–3 made optional for the sake of logs written before a
field existed is required here, because no schema-4 log predates any of it.

## `pub struct RunStarted4` › `pub schema: u32,`

Always [`TOPOLOGY_SCHEMA`]. First field of the first line, and the only
thing a reader is entitled to look at before choosing a fold.

## `pub struct RunStarted4` › `pub incarnation: IncarnationId,`

The coordinator process that created the run.

## `pub struct RunStarted4` › `pub runner: RunnerPolicy,`

What every process of this run executes through, for its whole life.

## `pub struct RunStarted4` › `pub probed_agents: Vec<String>,`

The agents pre-flight actually probed. The allow-list every task's
bindings — including one a human names for a repair — is drawn from.

## `pub struct RunStarted4` › `pub integration_ref: GitRef,`

The ref this run publishes onto, named rather than re-derived.

Authoritative because a resume has to move the same ref the first half
of the run moved: deriving it again from today's configuration would let
a config edit between two incarnations publish the second half of a run
somewhere else, and the CAS would succeed while doing it.

## `pub struct RunStarted4` › `pub execution_root: String,`

The contained root every worktree, snapshot and staging directory of
this run is created under.

Recorded for the same reason `integration_ref` is, and for one more: it
is the containment boundary every create, reclaim and delete is checked
against, so a recovery that re-derived it from ambient configuration
would be checking containment against a boundary the run never used.

A string rather than a [`std::path::PathBuf`], exactly as `private_dir`
and `worktree_path` are: a recorded root has to mean the same thing on
the Windows machine that resumes the run as on the Linux one that wrote
it, and a platform path type would make that a question about separators.

## `pub struct RunStarted4` › `pub normalized_plan_digest: String,`

Digest of the exact `plan.normalized.json` bytes.

## `pub struct RunStarted4` › `pub registry_digest: String,`

Digest of the original registry entries derived from those bytes and
this record. A reader rebuilds and compares.

## `impl RunStarted4` › `pub fn registry_record(&self) -> RunStarted {`

This record in the shape the registry derivation reads.

A projection, not a second copy: the registry is derived from the
frozen plan and the run record, and that derivation is the same one for
both execution models. Every field here is read straight off `self`, so
the two cannot drift — which matters because the digest this feeds is
what authenticates a rebuilt registry against the log.

## `impl RunStarted4` › `pub fn is_topology_schema(&self) -> bool {`

Whether this record claims to be a topology run at all.

## `pub struct RunResumed4 {`

`run_resumed` for a parallel-topology run.

Carries no re-derived configuration, unlike its legacy counterpart: there
is nothing for a resume to establish, because a schema-4 log never predates
a field. What it does carry is what this incarnation *is* and what it
re-established, so that a forged resume is refused on replay.

## `pub struct RunResumed4` › `pub runner: RunnerPolicy,`

What this incarnation rebuilt and verified. Must equal `run_started`'s
exactly, field for field.

## `pub struct RunResumed4` › `pub probed_agents: Vec<String>,`

What this incarnation's pre-flight probes found.

## `pub struct RunFinished4 {`

`run_finished` for a parallel-topology run.

The outcome is not a decision this event records; it is a value derived
from durable state, and the event is accepted only when it equals it. What
the event adds is the attribution a report needs and a fold would otherwise
have to recompute to print.

## `pub struct RunFinished4` › `pub halted_at: Option<TaskKey>,`

The task whose settlement halted the run, when one did.

## `pub enum DerivedOutcome {`

The total outcome function's result.

[`Self::NotEnding`] is not an error: it is the ordinary answer while work
remains, and the reason the guard is a comparison rather than a validity
check. [`Self::FoldError`] is the arm the design argues is unreachable, kept
as a value so that "unreachable" is something a census can assert rather
than something a `panic!` asserts.

## `pub struct BudgetStop {`

The epoch-scoped budget stop.

Scoped to an epoch because raising the ceiling and resuming is the intended
response to it: the stop belongs to the epoch that hit the old ceiling, and
the next epoch starts without one.

## `pub struct BudgetExceeded4 {`

`budget_exceeded`: the ceiling refused the next spawn.

## `pub struct BudgetExceeded4` › `pub epoch: Epoch,`

The epoch this stop belongs to. Recorded so a replay can tell that a
stop was cleared by a resume rather than inferring it from position.

## `pub struct BudgetExceeded4` › `pub spent_usd: f64,`

Reported spend to date — a floor wherever a route reports no spend.

## `pub struct BudgetExceeded4` › `pub key: Option<TaskKey>,`

The task whose next attempt was refused. Not a failed task: nothing
judged it and nothing was spent on it.

## `impl BudgetExceeded4` › `pub fn stop(&self) -> BudgetStop {`

The stop this event establishes.

## `pub struct FrozenQuestion {`

---------------------------------------------------------------------------
Questions
---------------------------------------------------------------------------

## `pub struct FrozenQuestion {`

A question keyed by the task it blocks.

Keyed rather than addressed by display id, and embedded complete in the
event that raises it: a question raised about a repair names a task that
exists only in the log, and an answer arriving three processes later has to
be validated against the options as they were frozen, not as they would be
re-derived today.

## `pub struct FrozenQuestion` › `pub context: String,`

Human-facing framing. Any agent-authored text inside it is quoted and
labelled as such by whoever built the question.

## `impl FrozenQuestion` › `pub fn is_complete(&self) -> bool {`

Whether this question can actually be answered.

An option-less question parks a task nothing can un-park, and an
unidentified or context-free one cannot be presented to the human it
exists for. All three are the same failure — a question that stops the
run without offering a way to continue it.

## `pub struct QuestionRaised4 {`

`question_raised`.

## `pub struct BindingOverride {`

A one-off binding a human named for a task whose ladder clipped to nothing.

## `pub enum Answer4 {`

What came back.

## `pub enum Answer4` › `binding_override: Option<BindingOverride>,`

Present exactly when the question was asking for a binding.

## `pub enum Answer4` › `Declined {`

The human said no. Whether that halts the run is the run's policy as it
stood when the decline became durable — recorded, not re-derived, so a
config edit between a run and its resume cannot rewrite what the answer
meant.

## `pub struct QuestionAnswered4 {`

`question_answered`.

## `pub struct QuestionAnswered4` › `pub via: String,`

Which channel produced it — a terminal, an out-of-band `upstroke answer`,
or a resume picking up an answer written while the run was dead.

## `pub enum AnswerDefect {`

How an answer disagrees with itself.

## `pub enum AnswerDefect` › `OverrideNamesAnotherQuestion,`

The override names a different question from the one being answered.

## `pub enum AnswerDefect` › `OverrideNamesAnotherTask,`

The override names a different task from the one being answered.

## `pub enum AnswerDefect` › `OverrideNamesAnotherOption,`

The override records a different option from the one chosen.

## `pub enum AnswerDefect` › `DeclineWithOverride,`

A decline carrying a binding.

## `impl QuestionAnswered4` › `pub fn self_consistency(&self) -> Result<(), AnswerDefect> {`

Whether the answer agrees with itself.

Only the relations inside one event: whether the option exists, and
whether the question was open, are facts about the question this event
answers and belong to the fold that holds it.

### Errors

The first [`AnswerDefect`] the event exhibits.

## `impl QuestionAnswered4` › `pub fn halts_run(&self) -> bool {`

Whether this answer is the carrier that halts the run.

## `pub enum SpawnAdmission {`

---------------------------------------------------------------------------
Task registration and dispatch
---------------------------------------------------------------------------

## `pub enum SpawnAdmission {`

Whether a freshly registered task may be dispatched, and what it is waiting
for if not.

The registry entry carries its own admission; this carries the question
that admission implies, which the entry has no place for. The two are
checked against each other by the fold.

## `pub enum SpawnAdmission` › `Runnable,`

A resolved ladder with rungs; the scheduler may dispatch it.

## `pub enum SpawnAdmission` › `HumanRequired {`

The lineage has consumed its automatic repairs. Only a human admits
another.

## `pub enum SpawnAdmission` › `HumanBinding {`

The clipped ladder is empty: there is no binding to run, and only a
validated override creates one.

## `impl SpawnAdmission` › `pub fn question(&self) -> Option<&FrozenQuestion> {`

The question this admission raises, where it raises one.

## `pub struct FrozenSpawn {`

A dynamic task, complete, in the event that registers it.

Embedded whole rather than referenced, because a dynamic entry has no
frozen plan behind it: the event *is* its authority, and a reader that had
to reconstruct it would be reconstructing it from nothing.

## `pub struct FrozenSpawn` › `pub key: TaskKey,`

Equal to the registry's length at this event.

## `pub struct TaskSpawned {`

`task_spawned`: a task that was not in the plan joins the registry.

## `pub enum LeaseGrant {`

What a dispatch does to the run's leases.

## `pub enum LeaseGrant` › `Predicted {`

An ordinary dispatch takes a predicted lease over the region the plan's
hints imply.

## `pub enum LeaseGrant` › `InheritedLineage {`

A repair executes inside the lineage lease its root already holds, and
takes nothing of its own.

## `pub struct TaskDispatched {`

`task_dispatched`: a generation is opened, before its worktree exists.

Written first on purpose. A worktree created before the event that records
it is a directory nothing in the log accounts for; an event written before
a worktree that then fails to appear is a generation the next process
closes.

## `pub struct TaskDispatched` › `pub base_sha: CommitSha,`

The commit the worktree is created at.

## `pub struct TaskDispatched` › `pub worktree_path: String,`

Recorded as the string a later process compares and re-derives, exactly
as `private_dir` is. A platform path type here would make a log written
on one operating system a question on another.

## `pub struct TaskDispatched` › `pub source_candidate: Option<CandidateRef>,`

The candidate a repair is materialized from.

## `pub struct RungBinding {`

---------------------------------------------------------------------------
Attempts
---------------------------------------------------------------------------

## `pub struct RungBinding {`

One rung's binding as an attempt actually used it.

Comparable against both authorities: the frozen rung the registry holds,
and an override a human named. The override records no tier — the option
list it chose from is agents, not tiers — which is why the two comparisons
are two methods rather than one equality.

## `pub struct RungBinding` › `pub pinned: bool,`

Whether the frozen rung this binding came from was pinned by the plan
rather than resolved by the run.

Part of the recorded binding because it is part of the frozen rung
([`FrozenRung`]), and INV-19 makes the frozen rung binding execution
identity that `attempt_started` records. Two rungs identical in tier,
agent and model but differing in provenance are two different
authorities, and a recorded binding that dropped this would match both.

## `impl RungBinding` › `pub fn from_frozen(rung: &FrozenRung, effort: Effort) -> Self {`

This binding as the frozen ladder would produce it.

## `impl RungBinding` › `pub fn matches_frozen(&self, rung: &FrozenRung, effort: Effort) -> bool {`

Whether this binding is the one the frozen rung names.

## `impl RungBinding` › `pub fn matches_override(&self, binding: &BindingOverride) -> bool {`

Whether this binding is the one an override names.

Tier and pin are not compared: an override chooses an agent from a
frozen option list, so the tier it lands on is whatever that agent is
bound at, and a human-named binding has no plan pin behind it at all.
[`BindingOverride`] records neither, and comparing a field the authority
does not carry would refuse every valid override.

## `pub enum Materialization {`

What a repair's worktree looked like when its attempt started.

## `pub enum Materialization` › `Clean,`

The rejected candidate applied cleanly onto the new base.

## `pub enum Materialization` › `Conflict,`

It did not, and the conflict is the repair's subject.

## `pub enum Materialization` › `Empty,`

It applied to nothing: the change is already present.

## `pub enum Materialization` › `Retained,`

The worktree was kept from the previous attempt and not re-materialized.

## `pub struct AttemptStarted4 {`

`attempt_started`.

## `pub struct AttemptStarted4` › `pub rung: u32,`

Index into the frozen ladder.

## `pub struct AttemptStarted4` › `pub pool: Option<String>,`

The capacity pool this attempt draws on, where its agent names one.

## `pub struct AttemptStarted4` › `pub resume_session: Option<SessionId>,`

The session this attempt resumed. Only a generation that settled
Retained, in the incarnation that retained it, has one to resume.

## `pub struct AttemptStarted4` › `pub materialization_observed: Option<Materialization>,`

What the repair's worktree looked like when this attempt started.
Present on a repair, absent otherwise.

## `pub enum SettlementTransition {`

The non-parking state transition a settled attempt records.

## `pub enum SettlementTransition` › `Succeeded,`

The attempt produced a tree the gates and reviewers accepted; a
candidate follows.

## `pub enum SettlementTransition` › `Retry,`

Another attempt on the same rung.

## `pub enum SettlementTransition` › `Escalated { rung: u32 },`

The next rung of the ladder.

## `pub enum SettlementTransition` › `Deferred { defers: u32, reason: String },`

Backoff: the task waits for `defer_wait_elapsed` or a resume.

## `pub enum SettlementTransition` › `Parked { question: FrozenQuestion },`

A human is asked.

## `pub enum SettlementTransition` › `Failed { halts_run: bool, reason: String },`

Terminal for the task, and — where the run's policy says so — for the
run.

## `pub enum LeaseDisposition {`

What a settlement does to the generation's lease.

## `pub enum LeaseDisposition` › `PredictedReleased,`

The generation's own predicted lease ends with it.

## `pub enum LeaseDisposition` › `PredictedRetained,`

The generation keeps its predicted lease, because it keeps its worktree.

## `pub enum LeaseDisposition` › `LineageHeld,`

A lineage lease, held across the settlement. The disposition a repair
generation records: a lineage lease belongs to the lineage root and no
attempt-level settlement releases it.

## `pub enum AttemptSettlement {`

How an attempt ended.

## `pub enum AttemptSettlement` › `Retained {`

The generation stays alive holding a session for a same-session retry.

The incarnation is recorded with the session because a session belongs
to a process: after a crash the working tree is rolled back, so the
conversation's belief about what it left behind is false, and only the
process that retained it may resume it.

## `pub enum AttemptSettlement` › `Closed {`

The generation closes.

## `pub struct AttemptFinished4 {`

`attempt_finished`.

## `pub struct AttemptFinished4` › `pub record: Box<AttemptRecord>,`

The ledger line: what it cost, what ran, what went wrong.

## `impl AttemptFinished4` › `pub fn halts_run(&self) -> bool {`

Whether this settlement is the carrier that halts the run.

Only a terminal task failure whose recorded policy says so. A deferral,
a park, a retry, an escalation, and a retained settlement all leave the
run running by construction.

## `impl AttemptFinished4` › `pub fn retained(&self) -> Option<(&SessionId, Epoch)> {`

The session and incarnation this settlement retained, if any.

## `pub struct AttemptInterrupted4 {`

`attempt_interrupted`: a process died holding this attempt.

Never halting. An interruption is a statement about a coordinator, not a
judgement of the work.

## `pub enum GenerationCloseReason {`

Why a generation was closed without a settlement of its own.

## `pub enum GenerationCloseReason` › `ResumeDiscardsRetainedSession,`

A retained session belongs to the incarnation that retained it, and
this is not that incarnation.

## `pub enum GenerationCloseReason` › `WorktreeMissing,`

The recorded worktree is gone, or failed its quiescence check and
cannot be rebuilt into what a retained generation claims to hold.

## `pub enum GenerationCloseReason` › `RunEnding { outcome: RunOutcome },`

Run-end closure, with the outcome it is closing for.

## `pub struct GenerationClosed {`

`generation_closed`.

## `pub struct DeferWaitElapsed4 {`

`defer_wait_elapsed`: the backoff the run was sleeping through is over.

One event for the whole run rather than one per waiter: it wakes every
deferred task and every verification-deferred candidate at once, so the
order they were deferred in cannot become an order they are retried in.

## `pub struct DeferWaitElapsed4` › `pub round: u32,`

**Consecutive waits where deferred work was the only runnable work.**

Not "which sleep this was, counted across the run", which is what this
said and what no writer produces.

The sole production construction is `settle::Deferral::wait`
(`settle.rs:334`), reached from `TopologyRun::step` alone, and it writes
`self.round` — the field `Deferral::progressed()` **resets to zero**.
That reset has three production callers, all in the driver:
`dispatch_ready`, `continue_open` and `retry_ready`.

So the sequence a reader actually sees is **1, 2, 1**, not 1, 2, 3: a run
that defers, sleeps twice, dispatches something, and defers again writes
`round: 1` for that fourth sleep. `wait` increments before it records, so
the value is one-based and restarts at one rather than at zero — which is
why "counted across the run" is not merely imprecise but reads a *later*
sleep as an earlier one.

It is a backoff round because that is what it indexes:
`interaction::defer_backoff(self.base, self.round)`, an exponential
doubling that has to restart when the run stops being stuck.

**Comment-only, on a frozen file, by per-instance approval of
2026-08-26**, carrying `reviews/FINDINGS.md` §20's staged erratum text.
The wire is unchanged: no field, no type and no serde attribute moves,
and `events::SCHEMA_VERSION` — which lives outside this file — is not
touched. The reason it was not left for the G2 pass is
that this is a **reviewer-facing wire doc**, and the frontier review
reads the wire to decide what the events mean. `PR7-R3-EMIT-006`.

## `pub enum CandidateLeaseEffect {`

---------------------------------------------------------------------------
Candidates
---------------------------------------------------------------------------

## `pub enum CandidateLeaseEffect {`

What preparing a candidate does to the run's leases.

## `pub enum CandidateLeaseEffect` › `ReplacesPredicted {`

The predicted region is replaced by the region the diff actually
touched.

## `pub enum CandidateLeaseEffect` › `WidensLineage {`

A lineage member adds its region to the lineage's.

## `pub struct CandidatePrepared {`

`candidate_prepared`: an immutable commit of exactly the tree that was
judged.

## `pub struct CandidatePrepared` › `pub attempt: Box<AttemptRecord>,`

The attempt whose gates and reviewers judged this tree. Embedded whole
because a fast integration publishes this commit with no verification
of its own, and this record is then the entire evidence for it.

## `pub struct CandidatePrepared` › `pub base_sha: CommitSha,`

The commit the worktree was created at, and the commit the candidate is
parented on. Recorded twice because they are two claims — where the work
started, and what the object says — and the merge queue's exact-base
decision depends on them being the same claim.

## `pub struct CandidatePrepared` › `pub prepared_ref: GitRef,`

`refs/upstroke/runs/<id>/candidate-prepared/<key>/<gen>` — the pin that
keeps the commit reachable until the authoritative ref exists.

## `pub struct CandidatePrepared` › `pub candidate_ref: GitRef,`

`refs/upstroke/runs/<id>/candidates/<key>/<gen>` — created next.

## `pub struct CandidatePrepared` › `pub actual_paths: PathSet,`

The region the diff actually touched.

## `impl CandidatePrepared` › `pub fn parent_is_base(&self) -> bool {`

Whether the object's parent is the base the work started from.

An intra-event relation, and the one that makes `base_sha` usable by
the merge queue at all: the exact-base decision compares the
integration head against `base_sha` and then publishes `commit_sha`, so
a commit parented somewhere else would fast-forward the integration ref
onto history nobody judged.

## `impl CandidatePrepared` › `pub fn candidate(&self) -> CandidateRef {`

This candidate as the merge queue names it.

## `pub struct TaskCandidateCreated {`

`task_candidate_created`: the authoritative ref exists and the candidate
takes its queue position.

## `pub enum VerificationBasis {`

---------------------------------------------------------------------------
Integration
---------------------------------------------------------------------------

## `pub enum VerificationBasis {`

Why a verification is running at all.

There is no `fast` variant: an exact-base candidate is published without a
verification of its own, so no `merge_verification_started` exists for it.
That absence is the design — the commit the integration ref fast-forwards
onto is the very commit its gates and reviewers judged.

## `pub enum VerificationBasis` › `StaleClean {`

A stale candidate was cherry-picked onto the current head and the
resulting proposal is under judgement.

## `pub enum VerificationBasis` › `prepared_ref: GitRef,`

`refs/upstroke/runs/<id>/prepared/<seq>` — the proposal pin.

## `pub enum VerificationBasis` › `AlreadyPresent,`

The cherry-pick was empty: the change is already in the head, and the
head itself is what gets verified.

## `pub struct MergeVerificationStarted {`

`merge_verification_started`.

## `pub struct MergeVerificationStarted` › `pub expected_head: CommitSha,`

The integration ref head this transaction read, and the head the CAS
will expect.

## `pub struct MergeVerificationStarted` › `pub proposed_sha: CommitSha,`

What is under judgement: the proposal commit, or the head itself.

## `pub enum VerificationVerdict {`

How a verification ended.

## `pub struct VerificationRecord {`

A completed verification.

## `pub struct VerificationRecord` › `pub reviews: Vec<ReviewRecord>,`

The review passes that actually ran, in order. Empty when the gates
failed first and nothing was reviewed.

## `impl VerificationRecord` › `pub fn passed(&self) -> bool {`

Whether this is a passing terminal record.

## `pub enum UnavailableCause {`

Why an integration could not be judged.

## `pub enum UnavailableCause` › `HumanRequired {`

A reviewer found something only a person may decide. Always parks.

## `pub enum UnavailableCause` › `Infrastructure {`

Something outside the run was unavailable. Defers until it has deferred
enough times, then parks.

## `pub enum InfrastructureKind {`

Which outage. Open-ended: the list is what has been seen, not what can
happen, and an unrecognized outage must still be recordable as one.

## `pub enum UnavailableOutcome {`

What the run does about it.

## `pub enum UnavailableOutcome` › `Deferred { defers: u32 },`

Back off and try again. The candidate keeps its queue position and its
lease, the sequence is consumed, and no attempt is burned — an outage
never fails a task on its own.

## `pub enum UnavailableOutcome` › `Parked { question: FrozenQuestion },`

Ask a person. The task moves to awaiting input and the candidate stays
queued but ineligible.

## `pub struct MergeVerificationUnavailable {`

`merge_verification_unavailable`.

## `pub enum UnavailableDefect {`

How an unavailability record disagrees with itself.

## `pub enum UnavailableDefect` › `HumanRequiredWithoutPark,`

A human finding cannot be waited out.

## `pub enum UnavailableDefect` › `ParkedWithoutCompleteQuestion,`

A park whose question cannot be answered.

## `impl MergeVerificationUnavailable` › `pub fn self_consistency(&self) -> Result<(), UnavailableDefect> {`

Whether the record agrees with itself.

The defer *count* is checked against the run's frozen ceiling and the
candidate's own history, which are the fold's; what is checkable here
is that a human finding parked, and that the park it produced is
answerable.

### Errors

The first [`UnavailableDefect`] the event exhibits.

## `pub struct MergeVerificationInterrupted {`

`merge_verification_interrupted`: a process died holding this transaction.

## `pub enum PreparedDisposition {`

How the integration ref is being moved.

## `pub enum PreparedDisposition` › `Fast,`

The head is exactly the candidate's base, so the candidate commit
itself is published: no staging worktree, no cherry-pick, no proposal
object, no pin. The integration ref fast-forwards onto the very commit
that was judged.

## `pub enum PreparedDisposition` › `StaleClean,`

The candidate was stale, was cherry-picked onto the head, and the
resulting proposal was verified.

## `pub enum PreparedDisposition` › `AlreadyPresent,`

The cherry-pick was empty and the head itself was verified.

## `pub enum VerificationSource {`

What judged the thing being published.

## `pub enum VerificationSource` › `CandidatePrepared {`

The candidate's own attempt record. Only a fast publication may cite
it, because only a fast publication publishes the object that record
judged.

## `pub enum VerificationSource` › `Verification {`

A verification run in this transaction.

## `pub struct MergePrepared {`

`merge_prepared`: the run is authorized to move the integration ref.

## `pub struct MergePrepared` › `pub expected_head: CommitSha,`

The head the CAS expects. Read before any staging effect.

## `pub struct MergePrepared` › `pub proposed_sha: CommitSha,`

What the ref will point at afterwards.

## `pub struct MergePrepared` › `pub key: TaskKey,`

The completion identity INV-20 binds this transaction to: which task and
which generation produced the candidate being published. Recorded beside
the candidate's commit and ref rather than inside them, because
`candidate_sha` and `candidate_ref` are payload fields of `merge_prepared`
itself.

## `pub struct MergePrepared` › `pub candidate_sha: CommitSha,`

The immutable commit the gates and reviewers judged. On a fast
publication this is also `proposed_sha`; on a stale one it is the object
the proposal was cherry-picked from.

## `pub struct MergePrepared` › `pub candidate_ref: GitRef,`

The authoritative candidate ref that keeps `candidate_sha` reachable.

## `pub struct MergePrepared` › `pub prepared_ref: Option<GitRef>,`

The proposal pin, on a stale publication only.

## `pub struct MergePrepared` › `pub verification: Option<VerificationRecord>,`

The verification's terminal record, where one ran.

## `pub struct MergePrepared` › `pub satisfies: Vec<TaskKey>,`

Every task this publication settles, as the fold derived the closure.

## `pub enum PreparedDefect {`

How a publication record disagrees with itself.

Only the relations that live inside one event. The rest of INV-09's
relations — `expected_head` against the candidate's recorded base, the
proposal against the pin, the head against the verification's — compare
this event against records elsewhere in the log and belong to the fold.

## `pub enum PreparedDefect` › `FastWithPreparedRef,`

A fast publication carrying a proposal pin. There is no proposal: the
candidate commit is what is published, and a pin would name an object
this disposition never creates.

## `pub enum PreparedDefect` › `FastProposesAnotherCommit,`

A fast publication proposing something other than the candidate commit.

## `pub enum PreparedDefect` › `FastWithoutCandidateSource,`

A fast publication citing a verification rather than the candidate's
own record.

## `pub enum PreparedDefect` › `StaleWithoutPreparedRef,`

A stale publication without the pin that keeps its proposal reachable.

## `pub enum PreparedDefect` › `AlreadyPresentMovesTheHead,`

An already-present publication proposing something other than the head
it claims is already present.

## `pub enum PreparedDefect` › `VerifiedWithoutVerificationSource,`

A verified disposition citing the candidate's record rather than the
verification that actually judged what is being published.

## `pub enum PreparedDefect` › `VerifiedWithoutRecord,`

A verified disposition without a terminal verification record.

## `pub enum PreparedDefect` › `VerificationDidNotPass,`

A verified disposition whose verification did not pass.

## `impl MergePrepared` › `pub fn candidate(&self) -> CandidateRef {`

The candidate this publication names, in the shape the queue holds it.

A projection of the four payload fields, so the two cannot disagree:
`merge_prepared` records the candidate's identity flat, and every
comparison against a queue entry wants it whole.

## `impl MergePrepared` › `pub fn self_consistency(&self) -> Result<(), PreparedDefect> {`

Whether the record agrees with itself.

### Errors

The first [`PreparedDefect`] the event exhibits.

## `pub enum RejectionDisposition {`

Why a candidate was not published.

## `pub enum RejectionDisposition` › `Conflict {`

The cherry-pick conflicted. The conflicting region is what the repair
inherits and what widens the lineage lease.

## `pub enum RejectionDisposition` › `CodeRejected {`

The proposal was verified and judged unacceptable.

## `pub enum RejectionLeaseEffect {`

What a rejection does to the run's leases.

## `pub enum RejectionLeaseEffect` › `CreatesLineage {`

A non-lineage candidate's lease becomes the new lineage's.

## `pub enum RejectionLeaseEffect` › `WidensLineage {`

A lineage member's rejection widens the lineage it already belongs to.

## `pub struct MergeRejected {`

`merge_rejected`: one append that rejects a candidate and registers the
repair for it.

One append because the two are one decision. A rejection recorded without
its repair is a lineage that a crash could leave holding a lease with
nothing scheduled to release it.

## `pub struct MergeRejected` › `pub rejecting_head: CommitSha,`

The integration head the candidate was judged against.

## `pub struct MergeRejected` › `pub repair: FrozenSpawn,`

The repair this rejection registers, complete.

## `pub enum MergeLeaseRelease {`

Which lease a publication releases.

## `pub enum MergeLeaseRelease` › `Candidate {`

An ordinary candidate's actual lease.

## `pub enum MergeLeaseRelease` › `Lineage {`

The lineage lease, released when the publication settles its root.

## `pub struct TaskMerged {`

`task_merged`: the integration ref moved.

## `pub struct TaskMerged` › `pub merged_sha: CommitSha,`

What the ref now points at — the `proposed_sha` of the authorization.

## `pub struct TaskMerged` › `pub satisfies: Vec<TaskKey>,`

Every task this settles, copied exactly from the authorization.

## `pub enum HaltCarrier {`

---------------------------------------------------------------------------
The vocabulary
---------------------------------------------------------------------------

## `pub enum HaltCarrier {`

What made the run halt, and where.

Two carriers and no others. In particular an outage is not one: a
verification that could not run defers or parks, and only a decline of the
question it parked behind halts anything.

## `pub enum HaltCarrier` › `TaskFailure {`

A terminal task failure whose recorded policy halts the run.

## `pub enum HaltCarrier` › `DeclinedQuestion {`

A declined question whose recorded policy halts the run.

## `impl HaltCarrier` › `pub fn key(&self) -> TaskKey {`

The task the halt is attributed to.

## `pub enum TopologyEventBody {`

Every transition a schema-4 run records.

Internally tagged on `event` with the payload under `data`, exactly as
schemas 1–3 are, so the file stays one JSON object per line and stays
greppable by tag. What it does not carry is the legacy envelope's hoisted
routing fields — see the module documentation.

## `pub enum TopologyEventBody` › `CapacitySnapshot {`

Informational: §14's pre-flight capacity snapshot. Nothing folds on it.

## `pub enum TopologyEventBody` › `PoolExhausted {`

Informational: a pool reported itself empty.

## `pub enum TopologyEventBody` › `DesignDefect {`

Informational: a question routed to the designer rather than execution.

## `pub const TOPOLOGY_EVENT_KINDS: [&str; 24] = [`

Every tag the vocabulary can write, in declaration order.

The first twenty-one are transactions — a fold applies them and refuses
what it cannot apply. The last three are informational.

## `pub const TOPOLOGY_TRANSACTION_KINDS: usize = 21;`

How many of [`TOPOLOGY_EVENT_KINDS`] are transactions rather than
informational records.

## `impl TopologyEventBody` › `pub fn kind(&self) -> &'static str {`

This event's tag, as it appears on the wire.

## `impl TopologyEventBody` › `pub fn is_transaction(&self) -> bool {`

Whether a fold applies this event, as opposed to merely recording it.

The distinction the unknown-field rule turns on: a transaction carrying
a field this binary does not understand is one it cannot claim to have
applied, while an informational record with an extra column costs
nothing to ignore.

## `impl TopologyEventBody` › `pub fn key(&self) -> Option<TaskKey> {`

The task this event concerns, where it concerns exactly one.

Replaces the legacy envelope's hoisted `task` field. Total over the
vocabulary, so a new event kind has to answer the question rather than
silently answering `None`.

## `pub fn key(&self) -> Option<TaskKey>` › `Self::MergeVerificationUnavailable { .. }`

Deliberately keyless: a verification outage and an interruption
are facts about a transaction, and the fold resolves the
candidate from the sequence rather than trusting a second copy.

## `impl TopologyEventBody` › `pub fn sequence(&self) -> Option<SequenceId> {`

The integration transaction this event belongs to, where it belongs to
one.

## `impl TopologyEventBody` › `pub fn halt_carrier(&self) -> Option<HaltCarrier> {`

The halt this event carries, if it carries one.

Total over the vocabulary and deliberately narrow. `halted_at` is first
in wins, and what may set it at all is a closed list: a terminal task
failure the run's policy halts on, and a decline the run's policy halts
on. An interruption, a generation closure, a deferral, and a
verification outage are each a reason the run is *not* progressing, and
none of them is a reason it is over.

## `pub struct TopologyEvent {`

One line of a schema-4 `events.jsonl`.

## `impl TopologyEvent` › `pub fn now(body: TopologyEventBody) -> Self {`

Stamp a body with the current time.

## `mod tests` › `const RUN_ID: &str = "01J8ZQK9WQ4RXN7VYB3TMEF6GD";`

------------------------------------------------------------------
Fixtures

Every independently meaningful field carries a different value from
every other, nothing sits at its type's default, orderings are
deranged against the order a reader would guess, and the strings are
padded, mixed-case, multi-byte and over-length by turns. That is not
decoration: a fixture whose fields correlate lets a test observe a
difference that the field it names did not produce.
------------------------------------------------------------------

## `mod tests` › `const SHA_CANDIDATE: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c";`

Three commit shas that are distinguishable at every position: no shared
prefix, no shared suffix, and different lengths of run so a comparison
that truncated or abbreviated would land somewhere visible.

## `mod tests` › `const SHA_FOURTH: &str = "3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e";`

Two further candidate commits, so a relation stated about "the
candidate's commit" can be crossed over more than one of them.

## `mod tests` › `const SHA_CANDIDATE_ONE_BYTE_OFF: &str = "0f1e2d3c4b5a6978879fa5b4c3d2e1f00f1e2d3c";`

A sha that differs from [`SHA_CANDIDATE`] in exactly one interior byte,
sharing its first 20 and last 19 characters. A comparison that
abbreviated, hashed a prefix, or compared a suffix accepts this as equal.

## `mod tests` › `fn container_runner() -> RunnerPolicy {`

A container runner with every identity field distinct from every other,
including a digest that shares no substring with the id it accompanies.

## `mod tests` › `fn task_of(id: &str, deps: &[&str]) -> Task {`

Plan order, display-id order and topological order all disagree, so a
projection that used one where it meant another shows up.

## `fn run_started(plan: &Plan) -> RunStarted4` › `integration_ref: GitRef::from("refs/heads/Ünïcode/Integration Target"),`

Deliberately not derived from `branch`, `private_dir` or the run
id: a projection that reached for the wrong one of the four
resource identities would still agree with itself if they shared
text.

## `mod tests` › `fn every_kind() -> Vec<TopologyEventBody> {`

One instance of every kind in [`TOPOLOGY_EVENT_KINDS`], in declaration
order. None of them halts: the halt carriers are built separately, so
the totality assertions cannot be satisfied by a fixture that happens
to be a halt.

## `mod tests` › `fn every_kind_is_represented_exactly_once_and_the_list_agrees() {`

------------------------------------------------------------------
The vocabulary itself
------------------------------------------------------------------

## `fn the_wire_tag_of_every_event_is_the_kind_it_reports()` › `for body in every_kind() {`

Asserting the serialized tag, not merely that serialize and
deserialize agree: a renamed variant round-trips perfectly and
silently stops matching every log already written.

## `fn the_envelope_is_a_timestamp_a_tag_and_a_payload_and_nothing_else() {` › `for body in every_kind() {`

Schema 4 hoists no routing field beside the tag. Identity lives in
the payload once, so an envelope that contradicts its own record is
not a refusal — it cannot be written.

## `mod tests` › `fn a_transaction_refuses_an_unknown_field_and_an_informational_record_ignores_it() {`

------------------------------------------------------------------
Unknown fields (deny_unknown_fields on transactions only)
------------------------------------------------------------------

## `mod tests` › `fn object_paths(value: &serde_json::Value, at: Vec<String>, found: &mut Vec<Vec<String>>) {`

Every object path in `value`, deepest last, as a list of steps.

Enumerated from the payload itself rather than listed by hand: a list is
a second declaration of the shape, and the shape is what moves.

## `mod tests` › `fn walk<'a>(`

Walk `value` to `path`, where a step of the form `[n]` indexes an array.

## `mod tests` › `fn is_informational_payload(kind: &str, path: &[String]) -> bool {`

Whether `path` is the payload of an event `refusals[24]` calls lenient.

The `data` object of an informational event and everything under it.
The envelope is not in this set for any class: it is `{ts, event, data}`
whatever the event, and an unknown key beside them is one of the hoisted
routing fields schema 4 made unrepresentable.

## `mod tests` › `fn is_open_map(path: &[String]) -> bool {`

The one object in the vocabulary whose keys are *values*.

`credential_volumes` maps an agent name to its volume name, so a key
nobody enumerated is another agent rather than a field this binary does
not understand — and removing one changes which volumes the run used
rather than truncating the record. Every other object in a schema-4
payload is a declared shape. Kept as a named exception rather than a
silently skipped path, because "this object is open" is a design claim.

## `mod tests` › `fn embeds_a_legacy_record(path: &[String]) -> bool {`

Where a schema-4 payload stops declaring a shape and starts embedding
one schemas 1–3 declared.

Which fields those records require is schemas 1–3's rule and this slice
does not restate it: `ChainSummary.bindings` is absent in a log written
before bindings were recorded, and demanding it here would refuse a
legacy record schema 4 legitimately carries. What schema 4 *does* impose
on them — that they carry no field this binary cannot read — is the
strict door, which is exercised by the injection sweep at these very
paths.

## `fn embeds_a_legacy_record(path: &[String]) -> bool` › `let under_repair = path.first().is_some_and(|step| step == "data")`

`merge_rejected` embeds the same registry entry under `repair`.

## `fn an_unknown_field_is_refused_at_every_object_boundary_of_every_transaction() {` › `let mut visited = 0_usize;`

`refusals[24]`: unknown fields in topology transaction payloads are
refused, and only informational events are lenient. Recursively —
a payload that denied at its top and ignored a stray key three levels
down would be a transaction carrying meaning this binary does not
understand, which is the whole thing the rule forbids.

The paths are enumerated from the canonical payloads rather than
sampled, so a nested structure nobody remembered is covered by
construction and a new one is covered the day it is added.

## `fn an_unknown_field_is_refused_at_every_object_boundary_of_every_transaction() {` › `assert_eq!(`

Pinned rather than bounded: the failure this whole test exists to
prevent is a sweep that quietly stops covering something, and a
shrinking corpus is exactly as invisible as a shrinking grid. A
legitimate new nested object raises this number and says so.

## `fn an_unknown_field_is_refused_at_every_object_boundary_of_every_transaction() {` › `assert_eq!(`

Both classes are non-empty, so neither assertion above is vacuously
satisfied by a decoder that refuses or accepts everything.

## `fn a_record_reused_from_the_legacy_schemas_is_read_strictly_inside_a_transaction() {` › `let finished = AttemptFinished4 {`

The reconciliation the design forces. `refusals[24]` refuses an
unknown field in a *topology transaction payload* and grants no
legacy-nested exception; the legacy-unchanged invariant is about the
decoder a schema-1..3 log gets, not about which fields schema 4
accepts. Both hold at once because the strictness is attached to the
schema-4 field, not to `AttemptRecord`.

This replaces the assertion A1 shipped, which required the opposite.

## `fn a_record_reused_from_the_legacy_schemas_is_read_strictly_inside_a_transaction() {` › `let mut legacy = serde_json::to_value(attempt_record()).expect("serialize");`

And the same bytes still read exactly as they always did through the
legacy type itself: the schema-1..3 decoder is untouched.

## `fn a_known_null_survives_the_strict_door_and_an_unknown_null_does_not() {` › `let mut record = serde_json::to_value(attempt_record()).expect("serialize");`

The strict door decides "unknown" by asking the record which keys it
claims back. That is exact only while every embedded record
serializes each field it deserializes — no `skip_serializing_if` —
and this is where that precondition is checked rather than assumed.
`cost_usd` and `session_id` are the optional fields of the attempt
record; supplied as an explicit null they are known, absent-valued
fields and must pass.

## `fn a_known_null_survives_the_strict_door_and_an_unknown_null_does_not() {` › `value["data"]["record"]`

A null under a key the record does not claim is still unknown.

## `fn every_required_payload_field_is_refused_when_it_is_absent() {` › `let mut deletions = 0_usize;`

A field made `#[serde(default)]` on input accepts a truncated durable
record and round-trips unchanged, so no round trip can see it.
Schema 4 has no ancestors — there is no upgrade into it — so every
absent field is a refusal rather than a default, and the way to prove
that is to take each one away.

A key whose value is `null` is excluded: for an `Option` field the
absent key and the null key are the same durable answer, and the
distinction the design draws is between a value and no record at all.

## `fn every_required_payload_field_is_refused_when_it_is_absent() {` › `assert_eq!(`

Pinned rather than bounded, for the same reason the injection sweep
is: a sweep that quietly stops covering a field is exactly as
invisible as a grid that quietly stops at 6.

## `mod tests` › `type MoveRunner = fn(&mut RunnerPolicy);`

------------------------------------------------------------------
Runner identity (INV-23)
------------------------------------------------------------------

## `mod tests` › `type MoveRunner = fn(&mut RunnerPolicy);`

One way to move exactly one identity field of a runner record.

## `mod tests` › `type NamedBindingMove = (&'static str, fn(&mut RungBinding));`

One way to move exactly one field of a rung binding, and its name.

## `fn a_runner_record_differs_in_the_field_that_moved_and_no_other() {` › `let cases: Vec<(RunnerField, MoveRunner)> = vec![`

Crossed over every field the design names, each moved on its own
against a base whose fields are already distinct from one another.
A comparison that read one field and reported the rest would satisfy
any single example.

## `fn a_runner_record_differs_in_the_field_that_moved_and_no_other() {` › `assert_eq!(other.difference(&base), Some(field));`

Symmetric: which side is the record and which the incarnation
does not change what moved.

## `fn the_first_difference_reported_is_the_most_structural_one() {` › `let base = container_runner();`

A record that changed kind has not merely moved its image, and
telling an operator to check their tag when the run changed
confinement boundary sends them to the wrong place.

## `fn a_credential_volume_set_is_a_set_and_not_an_ordered_list() {` › `let forwards = container_runner();`

Two incarnations that enumerated the same volumes in different
orders established the same runner. A list here would refuse a
resume for the order a directory listing came back in.

## `fn a_credential_volume_set_is_a_set_and_not_an_ordered_list() {` › `for changed in [`

But the contents are compared exactly: an added agent, a removed
one, and a renamed volume for the same agent are all differences.

## `fn a_credential_volume_set_is_a_set_and_not_an_ordered_list() {` › `let mut empty = container_runner();`

An empty record and no record at all are different answers.

## `mod tests` › `fn frozen_kind_of(contract: RunnerContract) -> RunnerKind {`

The contract-to-kind mapping the packet fixes, as a literal table.

Not `RunnerContract::kind()`: the completeness grid below is about
whether a record's contract belongs to its kind, and an oracle that
asked the mapping under test what it thought would move with it. A
mapping that sent `host-v1` to `Container` would then refuse every host
run while the grid derived the same wrong expectation and passed.

## `fn each_runner_contract_belongs_to_the_kind_the_packet_gives_it() {` › `assert_eq!(RunnerContract::HostV1.kind(), RunnerKind::Host);`

`decisions.sequential_substrate.runner`: `host-v1` is the host
contract and `container-v1` is the container one. Pinned against
literals so the grid below has an oracle that cannot move with the
implementation.

## `mod tests` › `fn image_grid() -> Vec<Option<ImageIdentity>> {`

Every image record the completeness rule distinguishes.

The digest is crossed *independently* of the reference and the id: a
grid whose only valid image has no digest never asks what a complete
record with one does, so a rule that rejected every reported digest
would pass it.

## `fn runner_completeness_is_decided_over_every_kind_and_field_combination() {` › `let expected = if frozen_kind_of(contract) != kind {`

The rule, restated from the design rather than read
off the implementation.

## `fn runner_completeness_is_decided_over_every_kind_and_field_combination() {` › `assert!(complete > 0 && complete < cells);`

Non-vacuous in both directions, and specifically: a valid container
record *with* a reported digest is among the accepted cells.

## `fn a_missing_digest_is_a_complete_record_but_not_an_equal_one() {` › `let mut without = container_runner();`

The digest is the manifest digest *when reported*, so a runtime that
reports none still produces a re-establishable record. It is
compared all the same: a record that gained or lost one changed.

## `fn two_independently_built_identical_runners_are_the_same_runner() {` › `let pairs: Vec<(&str, RunnerPolicy, RunnerPolicy)> = vec![`

A2 refuses a resume on any `Some(field)`, so a comparator that
reported a difference between a record and its own twin would refuse
every resume of the shape it got wrong. Each pair below is built
twice from scratch rather than cloned, and each is a shape the
existing equal-runner coverage never had: a complete host record
(no image, no volumes), a container whose runtime reported no digest,
and a container whose agents need no credentials at all.

## `mod tests` › `fn no_digest_runner() -> RunnerPolicy {`

A container whose runtime reported no manifest digest. Complete by the
packet's when-reported rule, and a shape a resume must accept twice.

## `mod tests` › `fn empty_credentials_runner() -> RunnerPolicy {`

A container whose agents need no credentials. An empty map is a record;
`None` is the absence of one, and the two are different answers.

## `fn an_image_id_is_compared_byte_for_byte_in_both_directions() {` › `let base_id = "sha256:11223344556677889900aabbccddeeff11223344556677889900aabbccddeeff";`

INV-23 requires the re-established image id to equal the recorded one
exactly. A mover that swaps the whole value proves only that the
field is read at all; these change one thing each, and the ASCII-case
pair is the one a normalizing comparison survives.

## `fn a_credential_volume_name_is_compared_byte_for_byte_at_a_fixed_key() {` › `let base = "upstroke-creds-Zeta";`

INV-23 again, on the other side of the map. Every mover below keeps
the keys, the cardinality and the pairing identical and changes one
property of one value, so a comparison that lower-cased, trimmed, or
compared lengths fails on exactly the mover that isolates it.

## `mod tests` › `const FROZEN_VOCABULARY: [(&str, bool); 24] = [`

------------------------------------------------------------------
Test-owned oracles for the vocabulary itself
------------------------------------------------------------------

## `mod tests` › `const FROZEN_VOCABULARY: [(&str, bool); 24] = [`

The twenty-four tags a schema-4 log can carry, and whether a fold
applies each one.

Written down here, in this test module, from the frozen contract.
`TOPOLOGY_EVENT_KINDS`, `kind()` and `is_transaction()` are three
declarations of the same facts in production, and a mutation that moves
all three together is invisible to any test that compares them with each
other. This is the fourth copy, and the only one that is not production.

## `fn the_vocabulary_and_its_transaction_class_match_a_test_owned_frozen_table() {` › `assert_eq!(`

Counting 21 and 3 is satisfied by swapping one member of each class,
which is exactly the mutation that would make `run_finished` lenient
about a payload field the fold reads. So the classes are named, not
counted.

## `mod tests` › `fn the_run_started_this_module_writes_is_the_header_the_probe_reads() {`

------------------------------------------------------------------
The writer and the reader, composed
------------------------------------------------------------------

## `fn the_run_started_this_module_writes_is_the_header_the_probe_reads() {` › `use crate::topology::schema::{`

The two halves of the seam, in one test, over bytes. Separate
fixtures for the producer and the decoder let this module's writer
vocabulary and `schema::probe_header` drift apart while both stay
green — and the first line of the log is exactly where that costs a
reader the ability to choose a fold at all.

## `fn the_run_started_this_module_writes_is_the_header_the_probe_reads() {` › `let torn = &bytes[..bytes.len() - 1];`

Without the commit marker the very same bytes are not a header, so
the composition is not accidentally reading past the line.

## `fn the_run_header_records_each_resource_identity_in_its_own_slot() {` › `let plan = sample_plan();`

`transaction_fault_matrix[0].durable_state` for a committed run:
`run_started` records the integration ref, the base sha, the
execution root, the limits, the registry digest, the incarnation and
the runner. Recovery compares each against the resource it is about
to mutate, so two of them sharing a slot is two resources it can no
longer tell apart.

## `fn the_run_header_records_each_resource_identity_in_its_own_slot() {` › `let independent = [`

And the four the design does not derive from one another share no
text at all: a fixture whose execution root contained its private
directory would hide a projection that reached for the wrong one.
The branch and the run id are excluded deliberately — the branch *is*
`upstroke/run-<id>` by construction, which is why
`canonical_trace_projection` drops ref names containing the run id.

## `mod tests` › `fn a_halt_is_attributed_to_the_key_its_carrier_names() {`

------------------------------------------------------------------
Accessors that must follow their value rather than their variant
------------------------------------------------------------------

## `fn a_halt_is_attributed_to_the_key_its_carrier_names()` › `for key in [0, 1, 5, 19, 4_294_967_295] {`

Checked once per variant with one key each, a variant-keyed constant
satisfies the accessor: `halted_at` would then name a task that
failed nothing.

## `fn a_halt_is_attributed_to_the_key_its_carrier_names()` › `for key in [0, 11, 4_294_967_295] {`

And through the event, so the carrier the vocabulary builds carries
the key the payload names rather than the fixture's usual one.

## `fn a_budget_stop_is_scoped_to_the_exact_epoch_that_hit_the_ceiling() {` › `for epoch in [0, 1, 2, 3, 4, 7, 8, 15, 16, 255, 256, u32::MAX] {`

Checking one epoch exactly and one other for inequality is satisfied
by any projection that is injective on the pair tested — masking the
low bits keeps `Epoch(2)` exact and `Epoch(3)` different. So every
epoch is asserted exactly, across the bits a mask would drop.

## `fn a_budget_stop_is_scoped_to_the_exact_epoch_that_hit_the_ceiling() {` › `let event = BudgetExceeded4 {`

The high-bit case said plainly: a mask to the low two bits sends
epoch 4 to 0, and a stop attributed to epoch 0 is a stop a resume
never clears.

## `fn a_topology_schema_is_exactly_four_and_nothing_near_it()` › `let plan = sample_plan();`

A2's fold gates schema-4 admission on this predicate, and INV-03 says
schema 4 is the topology *only*. Testing the adjacent pair 3/4 leaves
`>= TOPOLOGY_SCHEMA` indistinguishable from `==`, which admits every
future vocabulary as this one.

## `mod tests` › `fn the_commit_corpus_shares_no_run_a_comparison_could_key_on() {`

------------------------------------------------------------------
merge_prepared relations that live inside one event (INV-09)
------------------------------------------------------------------

## `fn the_commit_corpus_shares_no_run_a_comparison_could_key_on() {` › `let corpus = [`

Every relation in the merge queue is an equality over a full sha,
and the fixtures are what decide whether a *partial* comparison
could pass. So the property the grids rely on is checked rather than
asserted in a comment: the six commits are pairwise distinct, all
forty characters, and share no run of eight — long enough that an
abbreviation, a prefix hash, or a suffix comparison lands on a
difference.

SHA_CANDIDATE_ONE_BYTE_OFF is excluded: it exists precisely to share
everything but one interior byte with SHA_CANDIDATE, and is checked
against it directly in `a_fast_publication_publishes_the_commit_that
_was_judged`.

## `fn merge_prepared_self_consistency_over_the_crossed_disposition_grid() {` › `let proposals = [`

Three proposals: the candidate's commit, the expected head, and a
third sha belonging to neither. Sampling two of them would let a
check compare against the wrong one and still pass.

## `fn merge_prepared_self_consistency_over_the_crossed_disposition_grid() {` › `let candidates = [`

And three candidate commits, crossed independently of the proposal.
INV-09's relation is `proposed_sha == the candidate's recorded
commit` *whatever that commit is*; a grid built around one candidate
sha is satisfied by an implementation keyed on that literal value.
How distinct the three are is asserted in
`the_commit_corpus_shares_no_run_a_comparison_could_key_on`, not
claimed here.

## `fn merge_prepared_self_consistency_over_the_crossed_disposition_grid() {` › `let cited_candidate =`

The rule as the design states it, restated here.

## `fn a_fast_publication_publishes_the_commit_that_was_judged()` › `assert_eq!(merge_prepared_fast().self_consistency(), Ok(()));`

The shape that must be accepted, stated once so the grid above
cannot be satisfied by refusing everything.

## `fn a_fast_publication_publishes_the_commit_that_was_judged()` › `let mut near = merge_prepared_fast();`

And the relation is byte-exact. A proposal one interior byte away
from the candidate's recorded commit shares its first twenty and
last nineteen characters and its length, so a comparison that
abbreviated — which is what `core.abbrev` does to every sha an
operator ever sees — would publish an object nobody judged.

## `fn a_fast_publication_publishes_the_commit_that_was_judged()` › `let mut other = merge_prepared_fast();`

Symmetrically, with the candidate moved instead of the proposal.

## `fn a_candidate_commit_is_parented_on_the_base_its_worktree_used() {` › `let mut rebased = candidate_prepared();`

And the other direction: moving the base, not the parent.

## `fn a_candidate_commit_is_parented_on_the_base_its_worktree_used() {` › `let base = SHA_BASE;`

Full equality, not a prefix, a suffix or a length. Every pair below
is unequal while agreeing everywhere a partial comparison would
look — and a commit parented somewhere other than its worktree base
fast-forwards the integration ref onto history nobody judged, so the
cheap comparison is the expensive bug.

## `fn a_candidate_commit_is_parented_on_the_base_its_worktree_used() {` › `let mut other = candidate_prepared();`

And symmetrically, with the base moved instead of the parent.

## `mod tests` › `fn finished_with(settlement: AttemptSettlement) -> TopologyEventBody {`

------------------------------------------------------------------
Halting
------------------------------------------------------------------

## `fn exactly_two_carriers_can_halt_a_run_and_only_when_their_policy_says_so() {` › `let halting = [`

Every settlement and every answer, crossed against the carriers the
design names. The near-misses are the point: an outage that parked,
a deferral, an interruption, a run-ending closure, and a terminal
failure the run's policy does not halt on all look like the end of
something and none of them ends the run.

## `fn exactly_two_carriers_can_halt_a_run_and_only_when_their_policy_says_so() {` › `for body in every_kind() {`

And none of the ordinary vocabulary carries a halt either.

## `mod tests` › `fn an_answer_and_its_override_must_name_the_same_question_task_and_option() {`

------------------------------------------------------------------
Answers and binding overrides
------------------------------------------------------------------

## `fn an_answer_and_its_override_must_name_the_same_question_task_and_option() {` › `let outer_keys = [3_u32, 4, 12];`

2^3 over the three identity fields, crossed against values chosen so
that no cheaper relation than equality satisfies the grid: the
unequal task keys include a same-parity pair (3/5) and a pair
differing only above the low bits (4/12), the unequal questions share
a prefix and a length, and the unequal options are 2/10 rather than
2/1. A check that compared parity, a low bit, or a first character
would otherwise pass every cell.

## `fn an_answer_and_its_override_must_name_the_same_question_task_and_option() {` › `for answer in [`

An answer without an override, and a decline, have nothing to
disagree with.

## `fn a_question_is_complete_only_when_it_can_actually_be_answered() {` › `let mut one_option = complete;`

A single option is enough; the bar is answerable, not plural.

## `mod tests` › `fn a_human_finding_always_parks_and_a_park_always_carries_an_answerable_question() {`

------------------------------------------------------------------
Verification outages
------------------------------------------------------------------

## `fn every_infrastructure_kind_is_distinguishable_on_the_wire() {` › `let kinds = [`

Including the open-ended one: an outage nobody enumerated must still
be recordable as itself rather than collapsing into a neighbour.

## `mod tests` › `fn every_close_reason_is_distinguishable_including_each_run_ending_outcome() {`

------------------------------------------------------------------
Generation closure
------------------------------------------------------------------

## `fn every_close_reason_is_distinguishable_including_each_run_ending_outcome() {` › `let tags = [`

The exact tags, not merely six distinct strings: a renamed reason
round-trips and stays distinct while no longer matching a log
already written, and the run-ending reason must name its outcome.

## `mod tests` › `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {`

------------------------------------------------------------------
Routing, restored as a function
------------------------------------------------------------------

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `let expected_keys: Vec<Option<u32>> = vec![`

The legacy envelope hoisted these; schema 4 derives them. Stated as
a table over the whole vocabulary so a kind that quietly answers
`None` is visible rather than convenient.

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

run_started

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

run_resumed

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(9),` (trailing)

task_spawned

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(5),` (trailing)

task_dispatched

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(5),` (trailing)

attempt_started

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(5),` (trailing)

attempt_finished

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(7),` (trailing)

attempt_interrupted

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(6),` (trailing)

generation_closed

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

defer_wait_elapsed

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(2),` (trailing)

candidate_prepared

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(2),` (trailing)

task_candidate_created

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(2),` (trailing)

merge_verification_started

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

merge_verification_unavailable

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

merge_verification_interrupted

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(2),` (trailing)

merge_prepared

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(2),` (trailing)

merge_rejected

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

task_merged

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(3),` (trailing)

question_raised

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(3),` (trailing)

question_answered

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(4),` (trailing)

budget_exceeded

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

run_finished

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

capacity_snapshot

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

pool_exhausted

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `None,` (trailing)

design_defect

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(6),` (trailing)

merge_verification_started

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(6),` (trailing)

merge_verification_unavailable

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(6),` (trailing)

merge_verification_interrupted

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(6),` (trailing)

merge_prepared

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(8),` (trailing)

merge_rejected

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `Some(6),` (trailing)

task_merged

## `fn the_task_and_sequence_are_recoverable_from_every_event_that_has_one() {` › `assert_ne!(expected_keys[2], expected_keys[3]);`

A key that differs from the one the surrounding fixture uses, so an
accessor reading the wrong field of the right event is caught.

## `mod tests` › `fn a_binding_is_compared_against_both_authorities_field_by_field() {`

------------------------------------------------------------------
Bindings
------------------------------------------------------------------

## `fn a_binding_is_compared_against_both_authorities_field_by_field() {` › `let movers: Vec<NamedBindingMove> = vec![`

Each field moved on its own.

## `fn a_binding_is_compared_against_both_authorities_field_by_field() {` › `assert!(!frozen.matches_frozen(&rung, Effort::Low));`

And the effort argument itself is part of the comparison.

## `fn a_binding_is_compared_against_both_authorities_field_by_field() {` › `for pinned in [true, false] {`

The pin is crossed rather than sampled. A frozen rung the plan
pinned and one the run resolved are two different authorities even
when tier, agent and model agree, so a binding recorded against one
must not match the other — in *both* directions, which one fixture at
`pinned: true` cannot show.

## `fn a_binding_is_compared_against_both_authorities_field_by_field() {` › `let binding = BindingOverride {`

The override comparison ignores tier and nothing else: the option
list an override chooses from is agents, not tiers.

## `fn a_binding_is_compared_against_both_authorities_field_by_field() {` › `for pinned in [true, false] {`

The pin is ignored for the same reason the tier is, and for both of
its values: `BindingOverride` records neither, so comparing either
would refuse a validated one-off binding rather than check it.

## `mod tests` › `fn a_topology_run_record_projects_to_the_registry_derivation_intact() {`

------------------------------------------------------------------
The run record
------------------------------------------------------------------

## `fn a_topology_run_record_projects_to_the_registry_derivation_intact() {` › `let plan = sample_plan();`

The registry is the oracle: it refuses a run record that does not
describe the same run as the plan, and it refuses one that is
incomplete. A projection that dropped or defaulted a field it needs
is therefore not a field comparison away from being caught — it
fails to build a registry at all, or builds a different one.

## `fn a_topology_run_record_projects_to_the_registry_derivation_intact() {` › `let mut elsewhere = run_started(&plan);`

And the derivation actually read the projected values: the digest
moves when the record does.

## `fn a_topology_run_record_leaves_nothing_the_registry_would_call_incomplete() {` › `let plan = sample_plan();`

Schema 4 has no ancestors, so the fields schemas 1–3 made optional
for the sake of older logs are required here — and the projection
must therefore never hand the derivation a `None` it would refuse.

## `mod tests` › `fn pinned<T>(value: &T, canonical: serde_json::Value)`

------------------------------------------------------------------
The frozen wire, pinned against independently written payloads

A round trip compares an encoder against its own decoder, so it agrees
with any symmetric rename: `#[serde(rename = "repeat")]` on
`SettlementTransition::Retry` round-trips perfectly and stops matching
every log already written. Everything below is the independent side of
that comparison — payloads written out here from the declared shape, so
no schema-4 wire name can move without a test failing.
------------------------------------------------------------------

## `mod tests` › `fn pinned<T>(value: &T, canonical: serde_json::Value)`

Pin one value to its exact canonical JSON, in both directions.

## `mod tests` › `fn canonical_runner() -> serde_json::Value {`

The frozen wire for the run's execution identity (INV-23).

## `mod tests` › `fn canonical_entry() -> serde_json::Value {`

The frozen wire for the registry entry a `task_spawned` embeds whole.

## `mod tests` › `fn canonical_events() -> Vec<serde_json::Value> {`

Every event of [`every_kind`], in the same order, as the exact line a
conforming writer commits.

Written here from the frozen design — the declared field list, the
declared tag — never read back from the serializer.

Records schemas 1–3 also define (the attempt record, the gate and chain
summaries, the effort policy, the review plan, and the three
informational payloads) are spliced from their own values rather than
respelled. A1 embeds those types; it does not declare or freeze their
shape, and their keys are already pinned by the schema-1..3 suite that
reads them. What is written out by hand here is exactly what schema 4
froze, which is exactly what this slice owns.

## `fn every_event_decodes_from_its_independently_written_payload() {` › `for (body, canonical) in every_kind().iter().zip(canonical_events()) {`

The other direction, and the one a replay actually performs: bytes a
conforming writer produced, read by this decoder. A rename that
moved encoder and decoder together passes the round trip and fails
here.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `let paths = PathSet::Prefixes {`

`bounded_census.event_payload_classes`: every nested payload class,
including the variants no fixture in `every_kind` instantiates.
Sampling one arm of an enum leaves the other free to be renamed.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(&RunnerKind::Host, serde_json::json!("host"));`

Runner identity (INV-23): the kebab-case contract spellings are
durable identity, and `host-v1` is the arm no event fixture uses.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Spawn admission: all three arms.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Lease grants: both, including the repair arm nothing else builds.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Settlement transitions: every arm, including `retry`, whose tag no
other test reads off the wire.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Attempt settlement and lease disposition: the frozen vocabulary.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(&Materialization::Clean, serde_json::json!("clean"));`

Every repair materialization, including the three no fixture uses.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Generation closure.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Candidate and rejection lease effects: both arms of each.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Verification bases, sources, verdicts, and both rejection forms.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Outages: every enumerated kind and the open-ended one.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Merge lease release: both arms.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

Answers: both arms, and the override whose authoritative slot is a
key rather than a task label.

## `fn every_nested_variant_is_pinned_to_its_frozen_tag_and_fields() {` › `pinned(`

The run's frozen ceilings, and the region vocabulary.

## `fn budget_exceeded_carries_the_epoch_its_stop_belongs_to()` › `let event = BudgetExceeded4 {`

Epoch-scoped, because raising the ceiling and resuming is the
intended response: the stop belongs to the epoch that hit the old
ceiling and must not outlive it.
