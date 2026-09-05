# `src/events/mod.rs`

Extended notes for [`src/events/mod.rs`](../../../src/events/mod.rs).

The source defines behavior; these notes hold the module's contracts and rationale.
Each code span in a section heading is an exact source fragment. Search it as a fixed string
in the linked module, using the enclosing item to distinguish repeated lines.

## Module

The event log and the state it folds to (DESIGN.md §15, invariant 4).

`events.jsonl` is the run's source of truth: every state transition is an
event, and state is what you get by replaying them. `status`, the ledger,
and `resume` are all folds over this file; `report.json` is a projection of
the same fold, written for humans and never read back as state.

The load-bearing decision here is that **there is one fold, not two**.
[`RunState::apply`] is the only thing that mutates run state, and the live
engine reaches it the same way replay does — by emitting an event and
applying it. A live run and a replay of its own log cannot drift, because
neither has a private path to the state. Any bug is a bug in both, which is
a property a test can actually pin (see `live_state_equals_replayed_state`
in `engine.rs`).

Two things deliberately do *not* survive replay, both for the same reason:
a session id and a `resume_next` flag describe a conversation that believed
it had left edits in the working tree. After a crash that tree is rolled
back, so the belief is false and §14's pairing of session-resume with
tree-retention is broken. `run_resumed` clears both.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_types)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub use log::{EventLog, LogTail, read_all};`

The public paths are unchanged: `crate::events::EventLog`,
`crate::events::read_all` and `crate::events::LogTail` are what every caller
outside this module already names, and `decisions.pr_sequence[6].scope`
requires the first of those to stay put ("EventLog writer moved to
src/events/log.rs (**public path crate::events::EventLog unchanged**)").

## `pub const SCHEMA_VERSION: u32 = 3;`

Bumped when an event's meaning changes in a way an older binary would
**misread**. A newer log is refused rather than folded on a guess — silently
deriving the wrong state from a log we half-understand is the one failure
mode an event-sourced design must not have.

Misread is the operative word. Step 10's additive reporting fields stayed in
schema 1 because ignoring them did not change execution. Schema 2 froze
effort and resolved worker bindings because they are execution identity.
Schema 3 freezes the complete-review and atomic-attempt contracts: a
schema-2 binary would ignore the per-pass timeout and still truncate review
prompts at 60 KiB, and would ignore an embedded ladder transition and
repeat a settled failure after a crash.
Fresh runs therefore say `3` in `run_started`; when this binary resumes an
older run it appends `run_schema_upgraded` before another attempt, so older
binaries refuse the changed verification standard rather than misread it.

## `pub struct Event {`

---------------------------------------------------------------------------
Envelope
---------------------------------------------------------------------------

## `pub struct Event {`

One line of `events.jsonl`, in §15's shape:
`{ts, event, task?, attempt?, rung?, profile?, data}`.

`ts`, and the routing fields hoisted out of each variant, are what make the
raw file greppable — `rung` and `profile` in particular answer "what ran
where" without a JSON parser.

## `impl Event` › `pub fn now(body: EventBody) -> Self {`

Stamp a body with the current time.

## `impl Event` › `pub fn task(&self) -> Option<&str> {`

The task this event concerns, if any.

## `pub enum EventBody {`

Every transition the engine records.

Internally tagged on `event`, with the routing fields alongside the tag and
the payload under `data` — one Rust type per event kind, so a variant and
its payload cannot disagree.

Two things are deliberately *not* events. **Blocked and skipped settlement**
is derived in `finish()` rather than recorded, because it is a view of an
ended run: a task blocked behind an unanswered question must become runnable
again the moment that question is answered, which a recorded state would
fight. And **an unreachable answer channel** is process-local — a question
nobody could answer at 2am is exactly the one the operator answers when they
come back, so `resume` must be free to ask again.

## `pub enum EventBody` › `RunSchemaUpgraded {`

Append-only downgrade barrier for a run whose `run_started` cannot be
rewritten from an older schema. Schema-1 binaries fail on the unknown
event tag; schema-2 binaries understand the tag but reject a transition
to schema 3 before they can apply the old partial-review contract.

## `pub enum EventBody` › `parking: Option<Box<AttemptParking>>,`

A policy refusal that must finish the paid attempt and park the
task atomically. Without this, a crash between `attempt_finished`
and the separate question/parking events can replay the task as
pending and pay for the same known-unreviewable attempt again.

## `pub enum EventBody` › `transition: Option<Box<AttemptTransition>>,`

The ladder decision caused by this failed attempt. It is part of
the same durable append as the attempt record: a crash must not
replay a known failure as pending work on its old rung.

## `pub enum EventBody` › `prepared_commit: Option<Box<PreparedCommit>>,`

The exact commit object prepared from the reviewed index for a
successful attempt. Creating the object does not move a ref; the
event is therefore durable before the branch advances, and resume
can finish either side of that CAS without re-running paid work.

## `pub enum EventBody` › `AttemptInterrupted {`

The `attempt_finished` a dead process never got to write.

Recorded by the resume that finds the attempt dangling, rather than
merely derived in memory: a settlement that lives only in a reader's
head is lost the moment the log is replayed by someone else, taking the
ledger line *and* the rung's refunded allowance with it.

## `pub enum EventBody` › `LadderRetry {`

§11.4: feed the failure back and try the same rung again.

## `pub enum EventBody` › `LadderEscalated {`

§11.4: next rung, fresh session, accumulated feedback.

## `pub enum EventBody` › `TaskDeferred {`

§19: an outage, so the attempt is given back rather than spent.

## `pub enum EventBody` › `DeferWaitElapsed {`

The scheduler waited out a deferral and made that work runnable again.

## `pub enum EventBody` › `DesignDefect {`

§5: every question that reaches a human at runtime is a design-phase
defect, logged as one so the designer prompt can learn from it.

## `pub enum EventBody` › `CapacitySnapshot {`

§14's pre-flight capacity snapshot, taken again after every `run_resumed`
because a resume re-establishes everything a fresh run does (§15).

Folds to **nothing**, like `design_defect`: v0.1's capacity engine is
read-only (§13), so nothing routes on it and recording it as state would
imply otherwise. It is in the log because "what did the pools look like
when this run made its choices" is unanswerable afterwards.

## `pub enum EventBody` › `PoolExhausted {`

§15: a rate-limit signal attributed to a pool — §13's source 1, and the
only thing in v0.1 that can say a pool is empty rather than unmeasured.

Separate from the `task_deferred` that follows it because they are
different facts with different lifetimes: the deferral is about one
task's next move, while this is about a subscription, and a later fold
reads it back as ground truth for every pool estimate ([`crate::capacity::observe`]).

## `pub enum EventBody` › `BudgetExceeded {`

§13's budget ceiling stopped the run before an attempt was spawned.

**Downgrade consequence, stated plainly:** `SCHEMA_VERSION` does not
bump for this (see its docs), so a binary older than step 10 folding a
budget-stopped log fails on an unknown variant — a loud refusal naming
the log, never a silent misread. That is the trade the version contract
is written around.

## `impl EventBody` › `pub fn kind(&self) -> &'static str {`

The `event` tag as it appears in the log — for status rendering.

## `pub struct RunStarted {`

---------------------------------------------------------------------------
Payloads
---------------------------------------------------------------------------

## `pub struct RunStarted {`

Everything `resume` needs to decide whether continuing is still safe, plus
enough context that the log explains itself without the repo beside it.

## `pub struct RunStarted` › `pub base_sha: String,`

Full sha of the commit the run branched from — the expected HEAD until
the first task commits.

## `pub struct RunStarted` › `pub plan_path: String,`

Plan path as given, relative to the repo root where possible so the
record survives the repo moving.

## `pub struct RunStarted` › `pub plan_hash: String,`

Content hash of the plan text (`ir::content_hash`). A run is bound to
the plan it froze; a different hash means the task graph moved under it.

## `pub struct RunStarted` › `pub normalized_plan_digest: Option<String>,`

Digest of the exact bytes written to `plan.normalized.json`.

`plan_hash` above belongs to the source document and is also serialized
inside the normalized plan. It cannot authenticate that file against
itself. Fresh schema-3 runs record this independent byte digest; legacy
runs establish it on their first schema-3 resume after comparing the
old snapshot with a canonical serialization of the validated source.

## `pub struct RunStarted` › `pub private_dir: String,`

Where the agent-authored half of this run lives (§15 split).

## `pub struct RunStarted` › `pub chains: Vec<ChainSummary>,`

The resolved chain per task, in plan order. Recorded so resume can tell
that config moved: `Progress.rung` is an index into this chain, and
re-resolving a different one would silently point it at another tier.

## `pub struct RunStarted` › `pub effort_policy: Option<ResolvedEffortPolicy>,`

The concrete effort standard this run resolved at pre-flight.

Like gates and reviewers, effort is part of the run's verification and
execution identity: changing today's config must not make the back half
of a resumed run think harder or less hard than the front half. `None`
means a legacy log predating this record; its first resume re-derives,
warns, and establishes the value in [`RunResumed::effort_policy`].

## `pub struct RunStarted` › `pub gate_cmds: Option<Vec<GateSummary>>,`

The effective gates in full, as the run resolved them at pre-flight —
**the gates a resume runs**, not merely a fingerprint it compares.
`gates` above names them for the reader; this is the executable record.

A live run is snapshot-safe by construction: config is parsed once into
the analysis and gates execute from memory, so a mid-run edit to
`upstroke.toml` cannot change what a running task is verified against.
Resume honours the same snapshot by rebuilding these gates and running
them, which is what makes every `task_committed` in one log mean the
same thing. Re-deriving from today's config instead would let the
workspace an implementer edits — which contains the very `upstroke.toml`
the gates come from — set the standard for the tasks that follow.

This is the `reviews` contract below, applied to the other half of §14's
verification: recorded because it is a fact about the run, not about
today's machine. Budgets stay deliberately re-derived
([`ResumeOptions::budget_usd`](crate::engine::ResumeOptions)) because a
ceiling on one's own spending is not identity.

`None` means the log predates this record and says nothing about the
gates — not that there were none. Absent means re-derive and warn,
exactly as an absent `reviews` does. Pure addition otherwise:
`#[serde(default)]` folds an old log to the state it always had, so
`SCHEMA_VERSION` does not move.

## `pub struct RunStarted` › `pub reviews: Option<crate::review::ReviewPlan>,`

Who judges this run's code (§11.2–§11.3), resolved at pre-flight.

Recorded because it is a fact about the run, not about today's machine.
The cross-family reviewer is chosen from what has an adapter *and*
probes, so a Copilot CLI installed or removed between a run and its
resume would otherwise change the verification standard halfway through
— the same reasoning that made resume honour the recorded `private_dir`.

`None` means the log predates step 9 and says nothing about reviewers —
which is emphatically **not** the same as saying there were none. A
default-constructed plan has no primary, and every reader treats that as
`review = { enabled = false }`; a resume that made that mistake would
finish the run with verification silently switched off (step-6 finding
#10, from the other direction). Absent means re-derive and say so.

## `pub struct ChainSummary {`

One task's resolved escalation chain, as it stood when the run started.

## `pub struct ChainSummary` › `pub bindings: Option<Vec<BindingSummary>>,`

The exact binding each rung resolved to at pre-flight, aligned with
`tiers`. `None` means a schema-1 log predating this snapshot; its first
schema-2 resume re-derives once, warns, and records the result on
[`RunResumed::chains`]. `Some([])` is a real empty chain list.

## `pub struct BindingSummary {`

One rung's execution identity. `pinned` remains explicit so the event log
preserves why the binding was fixed as well as which adapter/model ran it.

## `pub struct GateSummary {`

One effective gate as it stood when the run started — everything needed to
run it again, because a resume does exactly that.

All four fields, not just name and command. An earlier draft recorded the
pair alone on the theory that `timeout` and `shell` are operational settings
a resume may re-read; that is wrong about `shell`, which is half of what a
command *means* (see [`ShellKind`](crate::gates::ShellKind)) — the same
`cmd = "true"` passes always under `sh` and fails always under `cmd.exe`.
And it is wrong about `timeout` in the same direction, one step weaker: a
gate that was given twenty minutes and is given one verifies less.

The portability this costs is smaller than it looks. Resuming a run on a
machine whose shell it never had is already impossible for an unrelated
reason — `run_started.private_dir` records an absolute host path — so the
case the pair-only record was protecting does not exist.

## `pub struct RunResumed` › `pub head_sha: String,`

HEAD at the moment the run was picked up — the sha the continued work
builds on.

## `pub struct RunResumed` › `pub interrupted_attempts: u32,`

Attempts that were in flight when the previous process died.

## `pub struct RunResumed` › `pub discarded: Vec<String>,`

Uncommitted paths this resume threw away: a dead agent's half-written
edits (§14). Recorded rather than only warned about, so someone reading
the run tomorrow can still see that work was discarded and what it was.

## `pub struct RunResumed` › `pub gates: Option<Vec<GateSummary>>,`

The gates this resume **established**, for a run whose log had none.

`run_started.gate_cmds` is the usual home for this, and where it exists
this is `None` — a fact belongs in one place, and re-stating an unchanged
list on every resume would give the log two authorities that could
disagree. But a log written before that field existed has no home for it,
and the first resume of one has to re-derive from today's config. Left
unrecorded, *every* later resume re-derives too, so a gate weakened
between two of them is adopted silently — the very substitution the
record exists to prevent, surviving in the one population that cannot
carry the record.

So the resume that re-derives writes down what it settled on, and every
resume after it is an ordinary record-bearing resume. `Some(vec![])` is
meaningful and distinct from `None`: it says this run established that it
has no gates, which is what makes a gate appearing later a difference
worth warning about rather than a silent new standard.

Folds to no state, like `capacity_snapshot`: its reader is the *next*
resume, which takes it from the log directly ([`recorded_gates`]).

## `pub struct RunResumed` › `pub effort_policy: Option<ResolvedEffortPolicy>,`

The effort policy established by the first resume of a legacy log.

Current runs record this on `run_started`, so ordinary resumes leave it
`None`. Once an old log establishes a value here, later resumes use the
first recorded value and never re-derive it again.

## `pub struct RunResumed` › `pub reviews: Option<crate::review::ReviewPlan>,`

The review plan established by the first current-binary resume of a
legacy log.

Current runs record this on `run_started`. An older run has to derive
the missing plan once, but leaving that derivation only in memory lets
every later resume silently adopt a different reviewer or timeout.
The first resume therefore appends the plan it established; later
resumes read the first recorded value and leave this `None`.

## `pub struct RunResumed` › `pub chains: Option<Vec<ChainSummary>>,`

The resolved chain bindings established by the first schema-2 resume of
a schema-1 log. Current runs carry them on `run_started`; later resumes
use the first recorded snapshot and leave this `None`.

## `pub struct RunResumed` › `pub normalized_plan_digest: Option<String>,`

Exact normalized-plan byte digest established by the first schema-3
resume of a legacy run. Current runs carry it in `run_started`, and
subsequent resumes leave this absent so the first authority wins.

## `pub struct RunSchemaUpgraded {`

A schema transition appended to an old run without rewriting its beginning.

## `pub struct AttemptStarted` › `pub adapter: Option<String>,`

Adapter id used for this attempt. `agent` remains for wire compatibility.

## `pub struct AttemptStarted` › `pub preflight_cli_version: Option<String>,`

CLI version observed during pre-flight; this is not a per-attempt probe.

## `pub struct AttemptStarted` › `pub effort: Option<Effort>,`

Resolved effort passed to the adapter.

## `pub struct AttemptStarted` › `pub selection_origin: Option<SelectionOrigin>,`

Why this binding was selected. `None` means an old log did not record
this fact; `unknown` deliberately is not a value writers can emit.

## `pub struct AttemptStarted` › `pub pool: Option<String>,`

The capacity pool this attempt draws on (§13), recorded before the
spawn so an attempt the engine died inside can still be attributed: it
really ran and really drained a subscription, and the settlement record
has no other way to know which.

## `pub struct AttemptStarted` › `pub resume_session: Option<String>,`

The session this attempt resumed, if any (§11.4).

## `pub struct AttemptRecord {`

One attempt's ledger line: which rung it ran on, what it cost, and what
went wrong. Shared by the log and `report.json` so the ledger has exactly
one shape.

## `pub struct AttemptRecord` › `pub pool: Option<String>,`

Which capacity pool this attempt drained (§13), where the pools file
names one for its agent. Pure addition: `#[serde(default)]` means a log
written before step 10 folds to exactly the same state it always did,
which is why `SCHEMA_VERSION` did not move for it.

## `pub struct AttemptRecord` › `pub resumed: bool,`

Whether this attempt resumed the previous one's session (§11.4).

## `pub struct AttemptRecord` › `pub reviews: Vec<ReviewRecord>,`

The review passes that actually ran, in order (§11.3). Empty when the
gates failed first and nothing was reviewed.

A list rather than the single `review_model`/`review_cost_usd` pair it
replaces: §11.5 generalizes review into a list of passes, and a
second-opinion verdict has to be attributable to the model that gave it.
Logs written before step 9 read back with this empty — their review
spend does not replay, which is the price of the shape being right.

## `pub struct AttemptRecord` › `pub usage: Option<crate::ir::Usage>,`

Token accounting as the CLI reported it, where it reports any.

Kept beside `cost_usd` rather than folded into it, because dollars and
tokens are different claims and only the vendor gets to make the first
one. Claude Code computes its own api-equivalent cost and upstroke records
it; Codex reports usage and no price. Pricing those tokens here would
mean shipping a rate table inside a published binary, where it goes
stale silently and — on subscription auth, where the marginal dollar is
zero and the real currency is the rate-limit window — produces a number
that is notional twice over. §13's rule holds: an estimate that flatters
is worse than none.

So the ledger keeps saying `?` for a route that reports no dollars, and
the evidence survives anyway. That matters more than it sounds: a run
that did not record its usage can never be re-measured, and §23.2's
conclusions about where spend goes were drawn entirely from
cheap-implementer runs. Adapters have been parsing this into
[`Outcome::usage`](crate::ir::Outcome) since step 3 and the engine threw
it away.

Pure addition, like `pool` above: `#[serde(default)]` means a log
written before this folds to exactly the state it always did, so
`SCHEMA_VERSION` does not move.

## `pub struct AttemptRecord` › `pub failure: Option<FailureRecord>,`

`None` when the attempt passed.

## `pub struct PreparedCommit {`

A hook-free commit object prepared from the exact staged tree that gates
and reviewers accepted. The event log records the owning full branch ref as
well as every object identity because a subject, parent, and mutable HEAD do
not distinguish an amended tree or the ref the run is authorized to move.

## `pub struct PreparedCommit` › `pub pin_ref: String,`

Private ref that keeps the prepared object reachable until HEAD has
advanced. Its target is CAS-created and CAS-deleted.

## `pub enum AttemptTransition {`

The non-parking state transition settled by one failed attempt.

Parking remains beside this on `attempt_finished` because escalation can
both move to the next rung and ask for spend approval atomically. Legacy
standalone ladder events remain readable, but new attempts record their
decision with the attempt they settle.

## `impl AttemptRecord` › `pub fn is_successful(&self) -> bool {`

Whether this record says the attempt **succeeded**.

**One definition, read by both of the fold's settlement doors.** "The
attempt succeeded" is not `failure.is_none()`: a record can carry no
failure and still hold a review whose outcome is `Failed` or
`Unavailable`, both of which are authoritative — §11.2 requires every
configured pass to pass, and a reviewer that could not run "says nothing
about the code", which is not the same as approving it.

`check_candidate_prepared` enforced only the failure field and
`check_attempt_finished` enforced neither, so each door checked a
different half of the same question and a record with a `Failed` review
was promoted, charged and queued as a candidate. The `b1f54a5` review
walked it. This is the same "one derivation, not two" that the rung
allowance needed: the doors now ask one predicate rather than each
deciding for itself.

## `impl AttemptRecord` › `pub fn review_cost_usd(&self) -> Option<f64> {`

**There is deliberately no `is_failed`.** It was added as the complement
of the predicate above and never acquired a caller anywhere in the tree.
Every reader of this question is asking whether a record *claims success*
— `check_candidate_prepared`, and both arms of `check_attempt_finished` —
and each asks [`Self::is_successful`] directly, which is the question it
is actually asking. A named complement would put the same predicate under
two spellings, which is how two readers of one rule begin to disagree.

## `impl AttemptRecord` › `pub fn review_cost_usd(&self) -> Option<f64> {`

Total review spend for this attempt, or `None` when nothing reported any
— which is not the same as nothing costing anything (§13: the Copilot
route reports no spend at all).

## `impl AttemptRecord` › `pub fn review_cost_incomplete(&self) -> bool {`

Whether any pass that ran reported nothing, making the total above a
floor rather than a figure.

This is not pedantry: a cross-vendor review is the normal case for the
paths §11.3 covers, and the Copilot route reports no spend at all — so
"review: $0.05" on a two-pass attempt is one reviewer's bill presented
as the whole. `render_ledger`'s own contract is that a ledger which
cannot tell free from unreported is worse than no ledger.

## `impl AttemptRecord` › `pub fn review_models(&self) -> Vec<String> {`

The models that judged this attempt, in pass order.

## `pub struct ReviewRecord {`

One review pass's ledger line (§11.2–§11.3).

## `pub struct ReviewRecord` › `pub pass: String,`

The lens that ran — `review` or `second-opinion`.

## `pub struct ReviewRecord` › `pub adapter: Option<String>,`

Adapter id used for this pass. `agent` remains for wire compatibility.

## `pub struct ReviewRecord` › `pub preflight_cli_version: Option<String>,`

CLI version observed during pre-flight; this is not a per-pass probe.

## `pub struct ReviewRecord` › `pub effort: Option<Effort>,`

Resolved review effort passed to the adapter.

## `pub struct ReviewRecord` › `pub pool: Option<String>,`

Which capacity pool this pass drained (§13). A cross-vendor second
opinion draws on a *different* subscription than the implementer, so a
per-pool ledger that read only the implementer's line would attribute
the whole attempt to one pool that did not pay for all of it.

## `pub struct ReviewRecord` › `pub cost_usd: Option<f64>,`

`None` where the agent's route reports no spend.

## `pub struct ReviewRecord` › `pub outcome: ReviewPassOutcome,`

What this pass concluded. A later pass only exists because every earlier
one approved, so at most the last entry is ever anything else.

## `pub enum SelectionOrigin {`

Where the worker binding came from. The latter two variants are reserved
for future selectors and deliberately have no producer yet.

## `pub enum ReviewPassOutcome {`

How one review pass ended.

Three states, not two: step-6 finding #8 established that a reviewer which
could not run says nothing about the code, and the ladder already dispatches
on that distinction. Recording it as a plain "did not pass" would put a
rejection in the ledger against a model that never read the diff — and the
ledger is what a person reads when deciding whether to trust a run.

## `pub enum ReviewPassOutcome` › `Unavailable,`

Rate-limited, timed out, or otherwise never reached a verdict.

## `pub struct FailureRecord` › `pub detail: Option<String>,`

What the next attempt is told, verbatim — §11.4's feedback.

**The durable half of [`crate::ladder::AttemptFailure::feedback`]**, which
is "a gate log tail (§11.1) or the reviewer's `required_changes` (§11.2),
verbatim". `reason` is the human-facing summary — `gate \`fmt\` failed:
exit 1` — and a retry given only that is asked to guess what the gate
printed. This field is what it actually printed.

**Additive and optional so `SCHEMA_VERSION` does not move.** A line
written before this field existed reads back as `None` and folds
unchanged. Deliberately *not* `skip_serializing_if`: schema 4's strict
door (`refusals[24]`) proves exactness by re-encoding a decoded record
and comparing keys, and its own documentation rests on no embedded
record using that attribute.

Written at exactly one place — `engine::classify::attempt_record`, which
is the one production construction of an [`AttemptRecord`] **for an
attempt that reached a settlement**; `InterruptedAttempt::event` below builds the
other, for an attempt that started and never reported back, and sets
this to `None` because nothing judged it. Read by `TopologyRun`'s brief,
which derives §11.4's accumulated feedback from the log rather than from
a counter only the live path incremented.

The unqualified sentence was corrected on `classify::attempt_record` in
the commit that added this field, and written again here in the same
commit — one copy fixed, one copy created. The 2026-08-26 re-review of
`c2c0294` found it, finding C.

**And which caller writes it is not a property of this field.**
`classify::FeedbackCarrier` decides, because the builder is shared with
the legacy coordinator and the two engines answer differently. A
sentence here that named one engine would be the §22c class again.
`decisions/2026-08-26-durable-retry-feedback.md` is the Class C
authorization for this field and states its bounds.

## `impl FailureRecord` › `pub const fn shape(&self) -> crate::ladder::FailureShape {`

The two fields that decide what this failure cost.

The durable half of [`crate::ladder::FailureShape`]: a settlement holds
a record rather than the live failure, and the allowance decision is
the same decision either way.

## `pub struct LadderRetry {`

What the next attempt is told. Carried on the ladder events rather than on
the attempt record because this is the full text — a gate log tail runs to
kilobytes, and `report.json` should not grow one per attempt.

## `pub struct LadderRetry` › `pub resume: bool,`

§14: a resumed retry keeps the working tree, so the *cumulative* diff
is what gets re-gated.

## `pub struct LadderEscalated` › `pub to_rung: u32,`

The rung index being moved to. Recorded rather than derived as "+1" so
replay lands where the run actually went.

## `pub struct TaskDeferred` › `pub defers: u32,`

Deferrals this task has taken, after this one.

## `pub struct TaskParked` › `pub refund_attempt: bool,`

Whether the rung's allowance is given back. A worker or reviewer that
stopped to ask never had its code judged (§12), so it costs nothing.

## `pub struct TaskCommitted` › `pub sha: String,`

Full sha. `resume` compares this against HEAD, and `--short` length
varies with `core.abbrev`.

## `pub struct TaskFailed` › `pub halts_run: bool,`

Whether this failure halts the run (`[engine] on_task_failure`).
Recorded rather than re-derived so a config edit between a run and its
resume cannot rewrite which task the report blames.

## `pub struct QuestionAnswered` › `pub decline_halts_run: Option<bool>,`

The halt policy frozen when a decline became durable. `None` is a
legacy answer whose older writer did not record the policy.

## `pub struct QuestionAnswered` › `pub via: String,`

Which channel produced it — a terminal, an out-of-band `upstroke answer`,
or a resume picking up an answer written while the run was dead.

## `pub struct DesignDefect` › `pub context: String,`

The decision execution had to stop for — review material for the
designer prompt (§5).

## `pub struct CapacitySnapshot {`

§14's pre-flight capacity snapshot: what every pool looked like at the
moment the run made its choices.

## `pub struct CapacitySnapshot` › `pub strategy: String,`

`[routing.strategy] mode`, echoed because what a snapshot *means*
depends on which strategy was reading it.

## `pub struct PoolSnapshot {`

One pool's line in a snapshot, already rendered.

Strings rather than the [`crate::capacity`] enums: this is a record of what
a past run believed, and pinning it to today's variants would make a future
rename either break old logs or silently re-interpret them.

## `pub struct PoolExhausted` › `pub reset_at: Option<String>,`

When the signal said the window reopens, where it said so at all.

## `pub struct PoolExhausted` › `pub detail: String,`

The CLI's own words, quoted — the evidence for calling the pool empty.

## `pub enum BudgetKind {`

Which ceiling stopped the run (§17 `[budgets]`).

## `pub struct BudgetExceeded` › `pub spent_usd: f64,`

Reported spend to date. A floor where any attempt's route reports no
spend at all (§13) — which is why the ceiling is checked against
*reported* dollars and the report says so.

## `pub struct BudgetExceeded` › `pub task: String,`

The task whose next attempt was refused. Not a failed task: nothing
judged it, and nothing was spent on it.

## `pub enum RunOutcome` › `BudgetExceeded,`

§13's ceiling stopped the run. Distinct from `Halted` because `resume`
means something different afterwards — raise the ceiling and continue —
and CI needs to tell "your budget stopped it" from "a task failed".

## `pub enum TaskState {`

---------------------------------------------------------------------------
Derived state
---------------------------------------------------------------------------

## `pub enum TaskState {`

Scheduler state for one task. Readiness is derived (deps all `Done`), not
stored, so it can never drift from the graph.

## `pub enum TaskState` › `Pending,`

Runnable once its dependencies are done — the state a task returns to
after an answer un-parks it.

## `pub enum TaskState` › `Deferred,`

A pool was busy. No attempt was spent; try again after a wait (§19).

## `pub enum TaskState` › `AwaitingInput(QuestionId),`

Parked on a question (§12). Exactly this task, never its neighbours.

## `pub enum TaskState` › `Blocked(String),`

Settlement only: derived when a run ends, never applied from an event,
because an answered question has to make these runnable again.

## `pub enum TaskState` › `Skipped,`

Settlement only: the run stopped before this task got its turn.

## `pub struct InFlight {`

An attempt that started and never reported back — the shape a killed
process leaves in the log.

## `pub struct InterruptedAttempt {`

A dangling attempt, with the task it belongs to.

## `impl InterruptedAttempt` › `pub fn event(&self) -> EventBody {`

The event that stands in for the `attempt_finished` never written.

## `pub fn event(&self) -> EventBody` › `pool: self.flight.pool.clone(),`

Its spend is unknown, but which subscription it drew on is
not: the pool was recorded before the spawn precisely so this
line does not have to shrug.

## `pub fn event(&self) -> EventBody` › `reviews: Vec::new(),`

Nothing judged the code, so nothing is attributed to a
reviewer.

## `pub fn event(&self) -> EventBody` › `usage: None,`

Same reason as `cost_usd` above: the process died before the
CLI reported anything, so the tokens it spent are as unknown
as the dollars.

## `pub fn event(&self) -> EventBody` › `detail: None,`

Nothing produced feedback: the process died before any
gate ran or any reviewer read the diff, which is the same
reason `reviews` and `cost_usd` above are empty.

## `pub struct Feedback {`

One thing the next attempt should know. `human` matters: an operator's
answer is an instruction, while a gate log or a reviewer's demand is
tool-authored text quoted back.

## `pub struct Progress {`

Everything one task accumulates across its attempts.

## `pub struct Progress` › `pub rung: usize,`

Index into the resolved chain.

## `pub struct Progress` › `pub attempts_on_rung: u32,`

Attempts spent on the current rung.

## `pub struct Progress` › `pub attempts: u32,`

Total attempts, which also numbers this task's run artifacts.

## `pub struct Progress` › `pub session: Option<String>,`

Session id from the most recent attempt, for §11.4's resume.

## `pub struct Progress` › `pub resume_next: bool,`

Whether the next attempt should resume `session`.

## `pub struct Progress` › `pub in_flight: Option<InFlight>,`

Set while an attempt is running; a value that survives to the end of a
replay is an attempt the engine died inside.

## `pub struct RunState {`

The run state every reader derives and the engine mutates — the only thing
[`apply`](RunState::apply) touches.

## `pub struct RunState` › `pub task_ids: Vec<String>,`

Task ids in plan order; every other vector here is aligned to it.

## `pub struct RunState` › `pub order: Vec<usize>,`

Task indices in the order they first ran, so a report reads as the run
happened.

## `pub struct RunState` › `pub budget_stop: Option<BudgetExceeded>,`

The ceiling that stopped the run (§13), if one did. Folded from the
event rather than recomputed by each reader, so a `status` looking at a
finished run and the engine that finished it reach the same verdict —
the reader has no config and could not recompute it anyway.

First stop wins, like `halted_at`: the scheduler stops scheduling once
this is set, so a second one would describe a spawn that never happened.

## `impl RunState` › `pub fn new(task_ids: Vec<String>) -> Self {`

A fresh state for a plan's tasks, before any event.

## `impl RunState` › `pub fn apply(&mut self, event: &Event) {`

Fold one event in.

The engine calls this immediately after appending the event, and replay
calls it for every event in the file. Unknown tasks are skipped rather
than panicking: a log paired with a plan that no longer contains the
task is a resume refusal, caught before this is ever reached.

## `pub fn apply(&mut self, event: &Event)` › `EventBody::RunStarted { .. }`

Metadata for the reader; contributes no task state.

`capacity_snapshot` and `pool_exhausted` sit here for opposite
reasons. The snapshot folds to nothing because nothing routes on
capacity in v0.1 (§13 read-only) — state it produced would be
state no branch consults. `pool_exhausted` folds to nothing
because its consumer is a *later* run's estimator, which reads it
out of the log directly ([`crate::capacity::observe`]); the task
consequence of the same rate limit rides on `task_deferred`,
which is where the scheduler already looks.

## `pub fn apply(&mut self, event: &Event)` › `EventBody::BudgetExceeded { data } => {`

§13: the run's ceiling refused the next attempt. It stops the
drain but fails nothing — the task it names never ran, and the
tasks behind it settle as skipped exactly as they do after a halt.

## `pub fn apply(&mut self, event: &Event)` › `EventBody::RunResumed { .. } => {`

§14: a resumed run cannot trust a session that believed it left
edits in a tree that has since been rolled back, and deferred
work has by definition already waited.

## `pub fn apply(&mut self, event: &Event)` › `self.finished = None;`

`run_finished` describes the previous driver invocation, not
an immutable terminal once a later resume is durable. Status,
follow, and crash reporting must project the latest epoch.

## `pub fn apply(&mut self, event: &Event)` › `self.budget_stop = None;`

A budget stop is cleared here for the same reason deferred
work wakes: it describes a *ceiling a previous process was
working under*, and the resume has just re-read the ceiling
from today's config and flags (§13/D4). Leaving it folded in
would make `upstroke resume --budget` a command that changes
nothing — the run would replay straight back into the stop it
was resumed to get past. If the new ceiling is still too low,
the very next `step_task` records a fresh stop and says so.

## `pub fn apply(&mut self, event: &Event)` › `progress.session = data.resume_session.clone();`

A fresh attempt has no conversation paired with its fresh
tree. Replace, rather than preserve, the previous identity;
otherwise a sessionless failure can resurrect a discarded
session on the following retry.

## `pub fn apply(&mut self, event: &Event)` › `EventBody::AttemptInterrupted { task, data, .. } => {`

The attempt nobody was alive to finish. Recorded — it really ran
and really drained a pool, and a ledger that hides that is lying
— but it does not spend the rung's allowance, because nothing
judged the code. That is the rule §19 applies to an outage and
step 7 applies to a worker that stopped to ask.

`attempts` is deliberately not rolled back: it numbers this
task's artifacts, and reusing the interrupted attempt's number
would overwrite its transcript with the retry's.

## `pub fn apply(&mut self, event: &Event)` › `progress.session = None;`

§14: whatever session that attempt held described a working
tree that has since been rolled back.

## `pub fn apply(&mut self, event: &Event)` › `self.progress[index].session = None;`

Parking discards the attempt's working tree. Its model
session therefore describes edits that no longer exist
and must not survive as a candidate for a later retry.

## `fn apply_ladder_escalated(&mut self, task: &str, attempt: u32, data: &LadderEscalated) {` › `progress.session = None;`

§11.4: a different model cannot inherit another's conversation; the
accumulated feedback carries the history.

## `fn apply_task_deferred(&mut self, task: &str, data: &TaskDeferred) {` › `progress.attempts_on_rung = progress.attempts_on_rung.saturating_sub(1);`

No attempt was spent on the work itself (§19), and the discarded tree
makes every session that described it invalid.

## `fn apply_task_failed(&mut self, task: &str, data: &TaskFailed) {` › `self.halted_at.get_or_insert_with(|| task.to_owned());`

First failure wins: `halted_at` is what the report and CLI name
as the cause.

## `impl RunState` › `fn answer_question(&mut self, data: &QuestionAnswered) {`

Record an answer and un-park what it releases.

A decline changes no task state here — the caller emits `task_failed`
for that, so the halt policy lives in exactly one place.

## `fn answer_question(&mut self, data: &QuestionAnswered)` › `if !self.questions[position].is_open() {`

An answer that arrives twice — a late file alongside a terminal
reply — must not push the operator's words into the prompt twice.

## `fn answer_question(&mut self, data: &QuestionAnswered)` › `let canned = self.questions[position]`

An `ApproveSpend` answer is a yes/no about money, and its whole
meaning was consumed by the un-park above. Pushing it as feedback
would put "approve: run the escalated attempt" into the next
prompt under `feedback_section`'s human framing — "an instruction
from a person, and it takes precedence over your earlier
assumptions" — handing a coding agent a billing decision as task
guidance.

The same objection applies to any canned option, whatever the
kind, and for a reason the first version of this missed: the
options are the engine's instructions *to the operator*, not the
operator's instructions to anyone. `upstroke answer <id> --option
1` on an unblock question resolves to "retry this task with
guidance you type below" — a sentence about where to type, which
then reached the implementer as binding guidance and, since §12's
decisions were routed to the judge, reached the reviewer as "a
decision from a person… a change that departs from it is a defect
however well argued". A judge grading a diff against meta-UI text
can only reject it, every attempt, until the ladder runs out.

An operator's own words are guidance. A label they picked off a
list is the un-park, and nothing more.

## `fn answer_question(&mut self, data: &QuestionAnswered)` › `if kind == crate::ir::QuestionKind::Unblock {`

The answer buys a fresh allowance on the rung the task is
standing on, and clears the deferrals a pool outage racked up.
It never moves the rung: if the chain exhausted, the task is
already at the top of it.

## `fn answer_question(&mut self, data: &QuestionAnswered)` › `progress.resume_next = false;`

Never resume out of a park, however warm the session looks:
parking always discards the working tree, so the session's
account of what it wrote no longer matches the repository (§14).

## `impl RunState` › `pub fn interrupted_attempts(&self) -> Vec<InterruptedAttempt> {`

Attempts this log ends mid-flight — one per process that died inside
an attempt without a resume having settled it since.

## `impl RunState` › `pub fn settle_interrupted(&mut self) -> u32 {`

Settle dangling attempts *in memory*, for readers.

`status` uses this so an interrupted run reads correctly without
writing anything. A `resume` deliberately does not: it emits the same
events instead, so the settlement lands in the log where the next
reader will find it. Both go through [`RunState::apply`], so what a
reader sees and what a resume records cannot disagree.

## `impl RunState` › `pub fn open_questions(&self) -> Vec<&QuestionRecord> {`

Open questions, oldest first.

## `pub struct Replay {`

The result of folding a log: the state, plus the run metadata a reader
needs but that is not task state.

The state is **not** settled — attempts left mid-flight are still marked as
such. Settling is the caller's decision, because a reader does it in memory
and a resume records it (see [`RunState::settle_interrupted`]).

## `pub struct Replay` › `pub resumes: u32,`

How many times this run has been picked up again.

## `pub(crate) fn normalized_plan_digest(bytes: &[u8]) -> String {`

Stable digest for the exact bytes of `plan.normalized.json`.

This deliberately differs from [`crate::ir::content_hash`]: the source-plan
identity normalizes CRLF, while this value authenticates the snapshot bytes
themselves. The algorithm is named in the value so a future schema can add
a different digest without silently changing what an existing record means.

## `pub(crate) fn recorded_normalized_plan_digest(events: &[Event]) -> Option<&str> {`

The first exact normalized-plan digest this run established.

A fresh schema-3 run carries it in `run_started`; a legacy run gains it on
the first schema-3 `run_resumed`. First-in-log-order wins, matching the gate,
review, effort, and chain identity records above.

## `pub fn recorded_gates(events: &[Event]) -> Option<&Vec<GateSummary>> {`

The gates this run is bound to, from wherever the log records them.

`run_started` for anything written since the gate record existed; otherwise
the first `run_resumed` that had to establish them (see
[`RunResumed::gates`]). First-in-log-order wins, which is the same rule
stated two ways: `run_started` comes first, and among resumes the one that
established the standard is the one the committed work was verified under.

`None` only for a log that predates the record and has never been resumed —
the single case where nothing can be said about what verified this run.

## `pub fn recorded_effort_policy(events: &[Event]) -> Option<ResolvedEffortPolicy> {`

The effort standard this run is bound to, wherever the log first records it.

A current `run_started` wins. For a legacy start, the first resume that had
to establish the missing value wins; a later conflicting entry cannot
rewrite the run's execution standard.

## `pub fn recorded_reviews(events: &[Event]) -> Option<&crate::review::ReviewPlan> {`

The review plan this run is bound to, wherever the log first records it.

A current run carries the plan in `run_started`. A legacy run gains it on
the first current-binary `run_resumed`, so subsequent resumes cannot
re-derive a different reviewer, model, effort, or pass timeout.

## `pub fn recorded_complete_reviews(events: &[Event]) -> Option<&crate::review::ReviewPlan> {`

The first review plan recorded while the complete-review contract was in
force.

Schema 1 and 2 plans preserve an absent `pass_timeout_secs` as `None`. That
makes their reviewer binding usable as a legacy identity snapshot, but not
authoritative for the timeout: a later binary could have a different
default. A current start is complete in place. A legacy start becomes
complete only when a schema-3 resume explicitly serializes the upgraded
plan after the downgrade barrier.

## `pub fn recorded_chains(events: &[Event]) -> Option<&Vec<ChainSummary>> {`

The resolved worker bindings this run is bound to, wherever they first
become available. Schema-2 runs carry them in `run_started`; a schema-1 run
gains them on the first schema-2 `run_resumed` event.

## `pub fn started_of<'a>(events: &'a [Event], path: &Path) -> Result<&'a RunStarted, UpstrokeError> {`

The `run_started` a log opens with — how a run describes itself.

## `pub fn replay(`

Replay a log into state.

The plan's task ids are supplied rather than read from the log: they define
the index space every `Progress` lives in, and the caller has already
checked the plan is the one this run froze.

## `pub(crate) fn ensure_supported_schema(`

Apply the event-schema compatibility boundary shared by every whole-log
interpretation. Additive fields are safe inside the current schema; a
future schema is not something an older binary may silently project.

## `pub(crate) struct LegacyUnsettledFailure {`

A failed attempt written before schema 3 was settled in two appends: first
`attempt_finished`, then its ladder/parking decision. A process can die
between them. Upgrading that prefix would make the known failed task
runnable again, spending another attempt under a decision the log never
durably recorded. New writers avoid the gap by embedding the decision; old
prefixes must be refused when their second append is absent.

## `latest_escalations.retain(|failure| failure.task != *task);`

A next attempt proves an escalation with no approval question
was complete. It does not excuse a question raised without
the TaskParked append that made the approval binding.

## `if failure.kind == FailureKind::NeedsHuman {`

These categories have policy-independent semantics. Accepting a generic
retry/escalation here would turn a request for a person, an outage, or an
unreviewable diff into spend the ladder explicitly forbids.

## `fn the_envelope_matches_the_shape_the_spec_documents()` › `let event = Event::now(attempt_started("t1", 2, 1, "mid"));`

§15: {ts, event, task?, attempt?, rung?, profile?, data}. The
routing fields are hoisted so the raw file is greppable.

## `fn the_envelope_matches_the_shape_the_spec_documents()` › `let plain = Event::now(EventBody::DeferWaitElapsed {`

An event with no task omits the field rather than nulling it.

## `fn durations_are_milliseconds_not_a_struct()` › `let event = Event::now(attempt_finished("t1", 1, 0, "small"));`

Readability in the log, and it survives serde's internally-tagged
buffering, which the default Duration shape does not reliably do.

## `fn a_torn_final_line_is_dropped_but_committed_invalid_events_are_errors() {` › `let torn = format!("{good}\n{also_good}\n{{\"ts\":\"2026-01-0");`

A kill mid-write: the last line stops partway through.

## `fn a_torn_final_line_is_dropped_but_committed_invalid_events_are_errors() {` › `let mut invalid_utf8_tail = format!("{good}\n").into_bytes();`

`serde_json` may write Unicode from a recorded reason or path. A kill
can split that code point, but bytes after the commit newline are no
less recoverable merely because they are not yet valid UTF-8.

## `fn a_torn_final_line_is_dropped_but_committed_invalid_events_are_errors() {` › `let corrupt = format!("{good}\nnot json at all\n{also_good}\n");`

Damage anywhere else means the file was rewritten, not interrupted.

## `fn a_torn_final_line_is_dropped_but_committed_invalid_events_are_errors() {` › `let mut invalid: serde_json::Value =`

Being last is not enough to make an event recoverable. This record is
complete JSON and newline-terminated, but its domain value is invalid.

## `fn appending_after_a_torn_line_discards_it_rather_than_splicing() {` › `let mut warnings = Vec::new();`

Splicing would have lost both the fragment and the new event;
newline-terminating the fragment would have left an unparseable
line in the middle, which the reader must refuse outright.

## `fn a_log_that_is_nothing_but_a_torn_line_opens_empty()` › `let dir = scratch("alltorn");`

The pathological case: killed while writing the very first event.

## `fn a_run_started_without_gate_commands_reads_as_unrecorded()` › `let EventBody::RunStarted { mut data } = started() else {`

The shape every log written before the gate record has. `None`, not
an empty list: "said nothing about the gates" and "said there were
none" must stay distinguishable — the same rule `reviews` follows for
logs that predate step 9, and the difference between re-deriving with
a warning and running a run with verification switched off.

## `fn a_recorded_gate_survives_the_wire_intact_enough_to_run_again() {` › `let EventBody::RunStarted { data } = started() else {`

Resume rebuilds its gates from this record and executes them, so a
field that does not round-trip is a gate that runs differently the
second time. `shell` in particular: the same `cmd` is an always-pass
builtin under one and not a program at all under another.

## `fn a_recorded_gate_survives_the_wire_intact_enough_to_run_again() {` › `assert_eq!(json["data"]["gate_cmds"][0]["timeout_ms"], 600_000);`

Readable in the raw log, like every other duration in it.

## `fn a_recorded_gate_survives_the_wire_intact_enough_to_run_again() {` › `let recorded = read_back.gate_cmds.expect("gates");`

And the shell spells the same way in the log as in `upstroke.toml`, so
an operator comparing the two is comparing like with like.

## `fn an_interrupted_attempt_is_recorded_but_does_not_spend_the_rung() {` › `let events = vec![`

Decision 3, and the property a killed run depends on: the attempt
shows up in the ledger, the allowance does not.

## `fn resuming_drops_the_session_and_wakes_deferred_work()` › `let events = vec![`

§14's pairing: tree retention and session resume travel together, so
a resume that discards the tree must also drop the session.

## `fn an_answer_that_arrives_twice_is_applied_once()` › `let mut state = RunState::new(vec!["t1".to_owned()]);`

A terminal reply racing an out-of-band answer file must not push the
operator's words into the prompt twice.

## `fn a_decline_leaves_the_task_to_the_failure_event()` › `let mut state = RunState::new(vec!["t1".to_owned()]);`

The halt policy lives in exactly one place: task_failed.

## `fn a_tail_never_yields_half_an_event()` › `let mut file = OpenOptions::new().append(true).open(&path).expect("open");`

A partial line is left for the next poll rather than parsed.
