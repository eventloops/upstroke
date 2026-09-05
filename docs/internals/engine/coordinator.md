# `src/engine/coordinator.rs`

Extended notes for [`src/engine/coordinator.rs`](../../../src/engine/coordinator.rs).

The code is the authority for what it does. This file preserves the migrated prose;
the concurrency protocol also remains at its source sites under standards §10 and §13.
Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `#![allow(clippy::disallowed_methods)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub(super) fn run_harness_inner(`

Also hands back the state the run ended with — its own fold of its own log.

Only tests use the second half, to hold the live fold and a replay of the
same file side by side. Nothing in the engine reads state back.

## `pub(super) fn run_harness_inner_on(`

The same run, on an explicit boundary. See [`super::run_harness_on`].

`_contained` is INV-18's host portion as a capability: "on Windows every
host child is a member of the coordinator's ambient kill-on-close Job
Object **from creation**", enforced by "ambient job joined at write-command
startup (refusal otherwise)". This function is the write coordinator, and
[`crate::runner::host::Contained`] cannot be built outside
`crate::runner::host`, so no caller — a CLI arm, the frozen public engine
facade, or an entry point added later — can reach a spawn without having
established containment first. That is a compile error rather than a
convention, which is what the previous shape was: the CLI established it
and `engine::run_with` did not.

## `let validated = validate_inputs(opts, config::EngineLimits::Fresh)?;`

Every read-only refusal precedes every lock: the plan, the config, and
`[engine]`'s ceilings are checked here, where nothing has been created
yet, so a config this engine cannot honour cannot leave a git-dir lock
file behind on its way to being refused — and cannot lose a race to a
competing holder of the lease it never needed.

## `let worktree_git_dir = workspace.worktree_git_dir()?;`

Preflight reads the source plan, config, and gate programs from this
physical worktree. Own it before taking that snapshot so another run
cannot leave us with an analysis of its transient edits.

## `let analysis = validated.confirm_under_lease(opts, config::EngineLimits::Fresh)?;`

The lease is what makes a read of this worktree a fact about it, so the
analysis the run executes is captured and validated here rather than
before — and adopted only once it agrees, byte for byte, with what the
refusal above was decided on.

## `let _lock = RunLock::acquire(&paths.public)?;`

Held for the whole run, released by the OS if this process dies — so a
crash leaves nothing for `resume` to clear by hand.

## `let plan_path = paths.plan_json();`

Nothing is on the record until the first event lands, so a failure in
this window would leave a run directory with no `events.jsonl` in it —
and that husk becomes `latest_run`, so a bare `upstroke status` reports
"no event log here" for a run that never began, shadowing the real
latest one until someone deletes it by hand. Best-effort: failing to
tidy up must not mask the error that actually stopped the run.

## `gates: gates.iter().map(|gate| gate.name.clone()).collect(),`

Names for the reader, the full gates for the resume — both from the
one list pre-flight resolved, so the log cannot name a gate its own
record does not describe.

## `run.emit_capacity_snapshot(&BTreeMap::new())?;`

A fresh run has no signals of its own yet, and §13's other sources are
not read in v0.1 — so this snapshot is honestly a record of how little
was known when the run started.

## `pub(super) struct Run<'a>` › `pub(super) log: EventLog,`

The append-only record. Every mutation below goes through
[`Run::emit`], never straight at `state`.

## `pub(super) struct Run<'a>` › `pub(super) log_hooks: Box<dyn crate::events::log::EventHooks>,`

The observer the **legacy** append funnel is driven through.

Production passes [`NoEventHooks`], which is precisely what
`EventLog::append` passes on its own, so nothing about the legacy
engine's behaviour moves — `invariants_preserved[1]`. What moves is that
the failure is now *reachable* (`PR5-CONF-010`, `PR5-CONF-011`).

`production_effect` says "the legacy engine's handling of a returned
append error is unchanged — **it reports and stops**". The shipped code
did; nothing required it to. Replacing this function's `?` with an arm
that pushed a warning and returned `Ok` survived the whole suite, because
every append failure the suite injects targets an `EventLog` a test
built directly, and no fixture could make a **live `Run`**'s append fail:
`emit` called `append`, which hard-codes `NoEventHooks`. A source census
cannot tell propagation from swallowing inside a live run.

This is the resolution `PR4-CONF-005` reached for the same shape — no
machine here can make the real primitive fail, so the observer becomes a
parameter and production passes the no-op one.

## `pub(super) struct Run<'a>` › `pub(super) state: RunState,`

Derived state — the same fold `resume` and `status` build from the log.

## `pub(super) struct Run<'a>` › `pub(super) runner: &'a dyn Runner,`

Where every process of this run executes (DESIGN.md:118). Held for the
whole run because pre-flight's probes and the attempts must cross the
same boundary — "Probes run through that same runner, or pre-flight
could certify a host CLI/version different from the one the attempt
executes" (DESIGN.md:612).

## `pub(super) struct Run<'a>` › `pub(super) caps: BTreeMap<String, Caps>,`

Probe results per agent id — `session_resume` gates §11.4's resume.

## `pub(super) struct Run<'a>` › `pub(super) review_plan: ReviewPlan,`

Who judges each task (§11.2–§11.3), resolved once at pre-flight and
recorded in `run_started`.

## `pub(super) struct Run<'a>` › `pub(super) effort_policy: ResolvedEffortPolicy,`

The run's recorded effort standard. Both worker attempts and all review
passes read this snapshot, including after a resume under changed config.

## `pub(super) struct Run<'a>` › `pub(super) review_pass_timeout: Duration,`

Independent wall clock for each configured review pass. Frozen in
`review_plan`, materialized once by pre-flight.

## `pub(super) struct Run<'a>` › `pub(super) budgets: config::Budgets,`

§17's ceilings, with `--budget` already folded in. Checked before every
spawn; never consulted when deciding *what* binds.

## `pub(super) struct Run<'a>` › `pub(super) ask_before: config::AskBefore,`

§12's `ask_before` thresholds.

## `pub(super) struct Run<'a>` › `pub(super) unanswerable: Vec<QuestionId>,`

Questions no channel could reach a human for. Never asked twice — that
is what stops a hard block spinning.

Deliberately *not* replayed: it records that a channel was unreachable
in this process, not something true about the run. A question nobody
could answer at 2am is exactly the one the operator answers when they
come back, so a resume has to be free to ask it again.

## `pub(super) struct Run<'a>` › `pub(super) exhausted_pools: std::collections::BTreeSet<String>,`

Pools this run has already recorded a rate-limit signal for.

Only the *transition* is worth an event. One outage produces a failed
attempt per deferral (up to `max_defers`), and emitting on each wrote N
identical records of a single fact — inflating any later count of
outages by the deferral factor and repeating the same line N times in
`status --follow`. Retired when an attempt proves the pool is serving
again, mirroring [`capacity::observe`]'s rule so the log the engine
writes and the fold a reader performs agree about when a pool came back.

Process-local rather than folded state, like `unanswerable`: seeded on
resume from the log's own signals, so a resumed run neither re-announces
an outage the previous process recorded nor misses a fresh one.

## `fn legacy_append_hooks(opts: &RunOptions) -> Box<dyn crate::events::log::EventHooks> {`

The observer the live run's legacy append funnel is driven through.

Production is [`NoEventHooks`] — the same thing `EventLog::append` passes —
on both arms. The `#[cfg(test)]` arm exists so a fixture can make a live
`Run`'s append fail (`PR5-CONF-010`, `PR5-CONF-011`).

## `fn legacy_append_hooks(_opts: &RunOptions) -> Box<dyn crate::events::log::EventHooks> {`

See the `#[cfg(test)]` twin above.

## `impl Run<'_>` › `pub(super) fn emit(&mut self, body: EventBody) -> Result<(), UpstrokeError> {`

Append an event and fold it in.

The only way run state changes. Everything below emits; nothing reaches
past this into `state`, which is what makes a live run and a replay of
its own log the same computation rather than two that agree by
inspection.

## `impl Run<'_>` › `pub(super) fn drain_and_report(&mut self) -> Result<RunReport, UpstrokeError> {`

Drain, settle, and report.

## `pub(super) fn drain_and_report(&mut self) -> Result<RunReport, UpstrokeError> {` › `let partial = self.finish();`

The log already holds everything that happened, including the
attempt this died inside — that is what `resume` reads. The
report beside it is a courtesy for whoever opens the directory
next, and failing to write it must not mask the error that
actually stopped the run.

## `impl Run<'_>` › `fn drain(&mut self) -> Result<(), UpstrokeError> {`

Drain the graph (§14, §12).

The four branches are the whole interaction model: pick up answers that
arrived from somewhere else; run what is ready; if only deferred work is
left, wait for the pool rather than burning attempts against it; and
only when none of those is possible — the precise definition of a hard
block — ask a human.

**Why this terminates.** Every branch consumes something finite and
nothing replenishes any of them:

- the answer sweep fires only for an *open* question and closes it, and
  questions are created only by `step_task`;
- `step_task` moves its task out of `Pending`, and the only routes back
  are a deferral — bounded by `max_defers`, after which the ladder parks
  the task instead — or an answer, which closed a question to get there;
- the wait branch requires a `Deferred` task, which only a deferral
  creates;
- the ask branch either closes a question or adds it to `unanswerable`,
  which is only ever appended to and is checked before asking.

So no cycle exists that does not spend an attempt, a deferral, or a
question. `an_exhausted_pool_and_a_silent_operator_still_terminate`
holds it to that against an adapter that never succeeds and an operator
who never replies.

## `fn drain(&mut self) -> Result<(), UpstrokeError>` › `if self.state.budget_stop.is_none() && self.sweep_answers()? {`

Invariant 6 in its most useful form: an answer that arrives while
other work is still running un-parks its task there and then,
rather than waiting for the run to have nothing else to do.

Guarded on the budget stop like the two branches below, and for a
sharper reason than theirs: an answer this run cannot act on is
merely wasted, but a *declined* one routes through `fail_task`,
which sets `halted_at` — and halted outranks budget in
`outcome()`. A decline file sitting on disk would relabel a
budget stop as a task failure, so CI gating on exit 3 to raise a
ceiling would instead see exit 1 and a task blamed for something
the ceiling did. The answers keep for the resume (§15).

## `fn drain(&mut self) -> Result<(), UpstrokeError>` › `if self.state.halted_at.is_none()`

Guarded like the other branches: once the run has halted, no
answer can reach an attempt this session, so asking would spend
a human's attention on a decision the scheduler cannot act on —
and a decline would relabel `halted_at` with a task that was not
the cause. The questions stay open on disk for a resume (§15).

## `impl Run<'_>` › `fn next_ready(&self) -> Option<usize> {`

Stable order: among tasks whose dependencies are all done, lowest plan
index first (§14). Parked, deferred, and blocked tasks are simply not
ready — which is exactly the skip-ahead §14 asks for.

## `fn next_ready(&self) -> Option<usize>` › `if self.state.halted_at.is_some() || self.state.budget_stop.is_some() {`

A halt and a budget stop both end scheduling, for the same reason:
whatever runs next would be work the run has already decided not to
do. The remaining tasks settle as skipped exactly as they do after a
halt, and the questions already open stay open for a resume (§15).

## `fn next_ready(&self) -> Option<usize>` › `.is_none_or(|j| matches!(self.state.states[j], TaskState::Done(_)))`

An unknown dependency cannot exist on a validated
plan; treating it as satisfied keeps the scheduler
total rather than deadlocking.

## `impl Run<'_>` › `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {`

Drive one task until it yields the scheduler: done, failed, deferred,
or parked. Retries and escalations happen *inside* — a resumed retry
keeps the working tree (§14), so no other task may run in between, and
this loop is what guarantees that.

Returns whether the task ended deferred.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `let analysis = self.analysis;`

Copied out of `self` so they carry the run's lifetime rather than
this method's `&mut self` borrow.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `if let Some(exceeded) = self.budget_breach(index) {`

§13's ceiling, checked before EVERY spawn rather than once per
task. The placement is the whole point: an escalation onto a
frontier rung happens inside this loop, so a check that ran only
on the way in would let the most expensive attempt of the run be
the one that dodged the budget. It never influences *what* binds
— capacity-driven routing is v0.2 (§13) — only whether the next
attempt happens at all.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `self.emit(EventBody::BudgetExceeded { data: exceeded })?;`

The ceiling is recorded first, and nothing below may take it
back. It is what `outcome()` reads to return `BudgetExceeded`
rather than a task failure, what turns into exit 3 for the CI
job gating on it, and what `resume --budget` needs to find in
order to have a stop to get past. Tidying up afterwards is a
courtesy; the record is the run's account of itself.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `if let Err(error) = workspace.discard_uncommitted() {`

The tree may still hold a rejected attempt's edits, kept by
the ladder below for a resumed retry that is now never going
to run. Handing those back is the one thing §14 rules out —
they are unverified, and staged changes follow `git switch`
onto whatever branch the operator visits next. Nor can they
be saved for the resume: `run_resumed` discards every
uncommitted path and clears the session they belong to, so
keeping them past this point buys nothing at all.

A git that cannot do it says so and the run still stops at
its ceiling, the way it did before the tidying existed. The
sibling discard on the error path below is `let _ =` for the
same reason; this one warns, because here there is a report
left to carry the warning.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `let profile = super::assembly::implementer_profile(`

Attribution only (§13 read-only): which subscription pays for
this attempt. Resolved here because it needs the run's config,
and passed in.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `let rung_number = u32::try_from(rung_index).unwrap_or(u32::MAX);`

Recorded *before* the agent is spawned, so a process that dies
mid-attempt leaves an `attempt_started` with no
`attempt_finished`. That dangling pair is precisely what tells a
later replay an attempt was interrupted (§19's crash row) — the
engine cannot write a record of its own death afterwards.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `let result = {`

Scoped so every borrow the attempt takes on `self` is released
before the ladder updates this task's progress below.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `feedback: self.state.progress[index].feedback.clone(),`

Owned: the ladder appends to this task's feedback the
moment the attempt returns, and one clone per attempt
costs less than threading that borrow through.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `task_index: u32::try_from(index).unwrap_or(u32::MAX),`

The legacy engine's own scope for an invocation
identity: this task's position in the plan. See
`AttemptCx::invocation`.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `decisions: self.state.progress[index]`

The same entries the worker prompt quotes as operator
instruction, routed to the judge as well (§12).

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `match run_attempt(&attempt_cx, workspace, resume.clone()) {`

Any error between the agent editing files and the verdict
leaves the tree dirty; the run cannot continue but must not
hand the user a half-staged workspace either (§14).

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `let next = result.failure.as_ref().map(|failure| {`

Decide the ladder transition before writing the settlement, then
carry both in one event. A failure record without its decision is
not a safe crash prefix: replay would otherwise buy another
attempt on the old rung or lose an outage refund.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `refund_attempt: kind == QuestionKind::Clarify || failure.is_outage(),`

An outage or clarification never received a code
verdict, so its allowance is returned even when
the outage ceiling sends it to a human.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `let prepared_commit = if result.failure.is_none() {`

A passing attempt is turned into an immutable commit object and
pinned before its settlement becomes durable. The event, HEAD
CAS, and pin deletion can therefore be recovered at every crash
prefix without re-running paid work or trusting the mutable
index.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `feedback: super::classify::FeedbackCarrier::LadderEvent,`

The legacy wire's own carrier. `ladder_retry` and
`ladder_escalated` are appended with `summary` and
`detail` a few lines below, and `Progress::feedback`
is rebuilt by replaying them, so a copy on the record
would be the same kilobytes twice — once in the log
and once in every `report.json`.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `if let Err(cleanup) = self.workspace.discard_uncommitted() {`

A write/flush/sync error cannot prove whether the newline-
committed event reached disk. Deliberately retain a prepared
pin: resume removes it as an orphan if no settlement landed,
or publishes it if the complete settlement is readable.
Deleting it here would turn an ambiguous sync error into a
schema-3 settlement whose exact object is no longer durable.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `if let Err(cleanup) = self.workspace.discard_uncommitted() {`

The durable settlement is authoritative and already carries
the complete question. A crash or write failure here cannot
expose an orphan projection; resume rematerializes the
question from the event before accepting an answer.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `self.workspace.discard_uncommitted()?;`

Scrub gate side-effects (build artifacts, lockfile churn) so
they cannot leak into the next task's captured diff; the
commit recorded exactly the verified staged set.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `if failure.kind != FailureKind::Interrupted`

§13 source 1: a rate-limit signal is ground truth about a pool,
and the only thing in v0.1 that can call one empty rather than
unmeasured. Recorded separately from the deferral that follows
because they are facts with different lifetimes — the deferral is
about this task's next move, this is about a subscription, and a
later run's estimator reads it back out of the log.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `self.exhausted_pools.remove(&profile.pool);`

This attempt reached a model and got an answer, whatever the
verdict on its code, so any pool it drew on is serving again.
Same rule as `capacity::observe`'s, applied to the engine's
own view so the two cannot disagree about when a pool
recovered — without it, the *next* outage on the same pool
would go unrecorded because the set still held it.

## `fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {` › `if !matches!(next, Next::RetrySameRung { resume: true }) {`

§14: the tree survives only for a resumed retry, where the
*cumulative* diff is what gets re-gated. Every other branch
hands the scheduler a clean workspace, because another task may
run before this one does again.

## `impl Run<'_>` › `fn reviewers(`

§11.2/§11.3: the read-only passes that judge one task's attempt.

Reviewers bind at the configured review tier (frontier by default)
rather than the implementer's rung — a small model reviewing its own
work is not verification — and [`ReviewPlan::passes_for`] decides
whether that means one pass or two, and whether the primary rebinds
away from the model that wrote the code.

An empty list means review is switched off explicitly. A pass whose
adapter cannot be built is a hard error: verification vanishing without
a word is worse than a refusal, and pre-flight has already probed every
agent named here.

## `impl Run<'_>` › `let mut profile = pass.profile(self.effort_policy.review);`

Every pass judges at the review tier's effort, including a
second opinion bound to another vendor: the standard belongs
to the review, not to whichever family happens to apply it.

## `impl Run<'_>` › `profile.pool = self.pool_name_for(&profile.agent).unwrap_or_default();`

A cross-vendor second opinion draws on a different
subscription than the implementer (§11.3, §13), so its pool
is looked up from its own agent rather than inherited.

## `impl Run<'_>` › `pub(super) fn emit_capacity_snapshot(`

§14's pre-flight capacity snapshot, from the state this run has folded
so far.

Deliberately does **not** probe. Everything a probe would add — auth
state, versions — is already established by pre-flight, and spawning the
vendors' CLIs a second time to fill in a metadata event would be work
nothing reads. The estimator's inputs come from the run's own log, which
on a fresh run is empty and on a resume carries every signal the earlier
process recorded.

## `impl Run<'_>` › `let pools = &self.analysis.config.pools;`

No early return on an empty pools file: "nothing was connected" is
exactly as worth recording as a list, and its absence is otherwise
indistinguishable from a pre-step-10 log, or from a binary that never
took a snapshot at all (§14).

## `impl Run<'_>` › `let estimates = capacity::estimate(`

Signals come from the caller's fold of this run's log (empty on a
fresh run) rather than from a field kept here, so there is exactly one
place that turns `pool_exhausted` events into observations — the same
reasoning that keeps `RunState::apply` the only writer of run state.

## `impl Run<'_>` › `fn pool_name_for(&self, agent: &str) -> Option<String> {`

Which pool an agent's attempts drain (§13), or `None` when the pools
file names none for it. Attribution only — nothing routes on it.

## `impl Run<'_>` › `fn reported_spend(&self, task: Option<usize>) -> f64 {`

§13's reported spend so far — the ledger's own figure, with the ledger's
own honesty: unpriced attempts contribute nothing, so this is a floor
wherever a route reports no spend at all.

## `impl Run<'_>` › `fn budget_breach(&self, index: usize) -> Option<events::BudgetExceeded> {`

Whether a ceiling has been reached, and which one.

`run_usd` is checked before `task_usd` because it is the stricter claim:
a run at its overall ceiling is done whatever any individual task has
spent, and naming the run budget is what tells the operator which number
to raise.

## `impl Run<'_>` › `fn should_approve_spend(`

§12's `ask_before`: does this escalation need a person's approval first?

Only a move *onto* a frontier rung from somewhere cheaper counts. A
chain that starts at frontier is where the operator deliberately routed
the task in config or in an annotation, and §12's concern is silent
escalation — asking permission for a decision the operator already made
in writing would train them to answer without reading.

## `impl Run<'_>` › `fn record_pool_exhausted(`

§13 source 1, recorded: attribute a rate limit to the pool that hit it.

A reviewer's rate limit belongs to the *reviewer's* pool, which on a
cross-vendor second opinion is a different subscription from the one the
implementer drained — attributing it to the implementer's would mark a
healthy pool exhausted and leave the empty one looking fine.

## `impl Run<'_>` › `let Some(pool) = pool else { return Ok(()) };`

No pool named for that agent means no subscription to mark. The
signal is still in the log on the attempt record; inventing a pool id
to hang it on would put a fact about nothing into the estimator.

## `impl Run<'_>` › `if !self.exhausted_pools.insert(pool.clone()) {`

Only the transition (see `exhausted_pools`).

## `impl Run<'_>` › `reset_at: None,`

§13 wants a retry-at-reset timer here. Neither CLI reports a
machine-readable reset time today, and parsing one out of
prose would be a guess dressed as a timestamp — so it stays
`None`, `DEFAULT_MAX_DEFERS` stays the bound, and the estimate
says the reset is unknown.

## `impl Run<'_>` › `let halts_run = self.on_task_failure == OnTaskFailure::Halt;`

The halt policy is resolved here and recorded, not re-derived on
replay: a `upstroke.toml` edited between a run and its resume must not
rewrite which task the report blames for stopping.

## `impl Run<'_>` › `fn build_spend_approval(`

§12: raise eagerly, park exactly the affected task, tell the notifiers,
and write the payload where a UI or `upstroke answer` can read it.
§12's `ask_before` question: this task is about to escalate onto a
frontier rung, and the run has already reported enough spend that the
operator asked to be consulted first.

## `impl Run<'_>` › `fn unpriced_attempts(&self) -> u32 {`

Attempts whose route reported no spend at all (§13), so the figures this
run quotes are floors rather than totals.

## `fn build_question(&self, index: usize, kind: QuestionKind, context: String) -> Question {` › `affected_tasks: vec![task.id.clone()],`

v0.1 parks only the task that raised it. Dependents are held by
the graph, not by the question, so they stay eligible the moment
an answer arrives.

## `fn materialize_question(&mut self, question: &Question) -> Result<(), UpstrokeError> {` › `interaction::write_question(`

Materialize before notifying: a recipient must always be able to open
the payload it was told about. The caller decides whether the
authoritative event belongs before (atomic settlement parking) or
after (ordinary question flow) this projection.

## `fn materialize_question(&mut self, question: &Question) -> Result<(), UpstrokeError> {` › `if let Err(error) = notifier.ask(question) {`

A notifier that cannot deliver must not take the run with it: the
question is already on disk either way (§12).

## `impl Run<'_>` › `fn sweep_answers(&mut self) -> Result<bool, UpstrokeError> {`

Ingest answers left by `upstroke answer` in another process.

Returns whether anything changed. This is what makes the answer command
useful while a run is alive rather than only between runs: an operator
answering from a phone at 2am un-parks the task on the next scheduler
turn, with no resume needed.

## `fn sweep_answers(&mut self) -> Result<bool, UpstrokeError>` › `if self.ingest_answer(&id, answer, "answer-file")? {`

Only what actually applied counts as change. A file the engine
reads but declines to act on — an `Unanswered` one, say, which
nothing in `upstroke answer` will write but a hand-edit can —
would otherwise report progress on every turn, and the drain
loop would spin on it forever: this branch is only bounded
because it closes the question it fires for.

## `impl Run<'_>` › `fn ingest_answer(`

Record an answer and let it take effect. Returns whether it applied.

One path for every channel — a terminal reply, a file written by
`upstroke answer`, or an answer picked up on resume — so what an answer
*does* cannot depend on where it came from. The guards below are also
what makes it safe to offer the same answer twice: a question that is
already closed absorbs the second one instead of applying it.

## `impl Run<'_>` › `self.emit(EventBody::DesignDefect {`

§5: a question that reached a human at runtime is, by definition, a
design-phase defect — logged as one so the accumulated defects can
become review material for the designer prompt.

## `impl Run<'_>` › `if answer == Answer::Declined {`

A decline is the task's failure, not the question's, so it goes
through the one place that owns the halt policy. `apply` leaves a
declined task parked precisely so this can still see who was waiting.

## `impl Run<'_>` › `if let Some(record) = self`

Rewrite the payload so a late reader — a UI, or someone opening the
directory tomorrow — sees the whole exchange, not just the question.

## `impl Run<'_>` › `fn resolve_one_question(&mut self) -> Result<bool, UpstrokeError> {`

Ask about the oldest open question. Returns whether anything changed.

This runs only at a hard block, and each question is asked at most
once: an `Unanswered` result marks it unreachable rather than looping
back to a channel that already said nobody is there.

## `fn resolve_one_question(&mut self) -> Result<bool, UpstrokeError> {` › `self.sweep_answers()?;`

The channel may have been waiting on the very file the sweep reads,
so sweep before applying what it returned — and then still apply it.
`ingest_answer` is guarded on the question being open, which is what
makes doing both safe: if the sweep answered *this* question the
typed reply is absorbed, and if it answered a different one — an
operator working through a backlog of parked tasks — this reply
still lands instead of being discarded along with it.

## `fn resolve_one_question(&mut self) -> Result<bool, UpstrokeError> {` › `self.unanswerable.push(question.id);`

§12 CI mode: the task stays parked and the run's exit status
reports it. Not a failure — nobody rejected anything.

## `impl Run<'_>` › `fn finish(&self) -> RunReport {`

Settle every task that never ran, then report.

## `fn finish(&self) -> RunReport` › `running: false,`

The engine only reports on itself once it has stopped.

## `fn finish(&self) -> RunReport` › `interrupted: false,`

A `finish` that runs is by definition not an interruption:
the shape this flag describes is the one left behind when
this function never got the chance.

## `pub(super) struct ParkSubject<'a> {`

What the human is shown. Every agent-authored fragment is quoted behind a
fence the payload cannot close and labelled as agent-authored — a worker
that "asks a question" is still an agent writing into a human's terminal.

## `pub(super) struct ParkSubject<'a>` › `pub(super) display_id: &'a str,`

The id a human reads.

## `pub(super) struct ParkSubject<'a>` › `pub(super) title: &'a str,`

What the task was asked to do.

## `pub(super) struct ParkSubject<'a>` › `pub(super) acceptance: &'a [String],`

The bar it was asked to clear.

## `pub(super) struct ParkSubject<'a>` › `pub(super) attempts: u32,`

How many attempts have run, which the context quotes back.

## `pub(super) struct ParkSubject<'a>` › `pub(super) rungs_spent: usize,`

How many distinct rungs those attempts spent, at least one.

## `impl<'a> ParkSubject<'a>` › `pub(super) fn of(task: &'a Task, progress: &Progress) -> Self {`

The subject of a schema-3 task and its progress.

## `pub(super) fn question_context(`

The context a parked task's question quotes back to the human.

**Ask for what you read.** It reads the display id, the title, the
acceptance list and the attempt count — never the body, the artifacts or the
plan. Naming them lets the schema-4 driver, which holds a `FrozenTaskSpec`
and no `Task`, raise the same question the legacy engine does rather than
wording its own.

## `fn spend_question_context(`

§12's spend approval, in the operator's terms: what is about to happen, what
it has cost so far, and how confident that figure is.

The threshold is a **spend-to-date** reading rather than a forward
projection, and the text says so — see [`crate::config::AskBefore`] for why.
The figure itself is quoted with the ledger's own `?` honesty: a run whose
Copilot attempts report nothing has a reported total that is a floor, and
presenting a floor as a total is how someone approves a number they did not
actually see.
