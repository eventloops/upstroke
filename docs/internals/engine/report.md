# `src/engine/report.rs`

Extended notes for [`src/engine/report.rs`](../../../src/engine/report.rs).

These notes preserve the module comments after the status repairs. Item headings quote source lines for navigation.

## Module

The remaining write! assembles a ledger cell into String, whose fmt::Write
implementation cannot return Err. The discard does not hide an I/O error.

## `Parked {`

Waiting on a human. The rest of the run kept moving (invariant 6), and
nothing about this task is lost — the question carries its context.

## `Blocked {`

A dependency failed, or parked and was never answered.

## `Skipped,`

Not attempted because the run halted earlier.

## `Running {`

An attempt is running right now. Only a live `status` produces this: a
run that has ended has nothing left in flight.

## `Queued,`

Its turn has not come yet, and the run is still going — distinct from
`Skipped`, which means the run ended before this task got a turn.

## `#[serde(other)]`

A status this build does not know, from a `report.json` a newer upstroke
wrote. Never produced by this crate.

`report.json` is a projection for whoever reads the run afterwards, and
this enum is `pub` and `Deserialize` because that reader may be someone
else's program. Without a fallback, every variant added here is a hard
`unknown variant` error in every consumer built against an older
version — which is what `running`, `Queued` and this one did to anything
compiled against 0.0.1, and that break is already published. Adding it
now cannot undo that; it stops the next one.

## `pub model: String,`

The final attempt's implementer model. `cost_usd` is the implementer's
spend across every attempt; reviewer spend is a separate field because
it is a different model at a different tier, and folding them together
makes cheap rungs look expensive to anyone reading the ledger (§13).

## `pub review_models: Vec<String>,`

Every model that judged this task, in the order first seen.

Across *all* attempts, not just the last, because `review_cost_usd`
beside it sums all of them — a list scoped to the final attempt next to
a total scoped to every attempt reads as though one explains the other.

## `pub review_cost_incomplete: bool,`

At least one review pass reported no spend, so `review_cost_usd` is a
floor (§13). Rendered as a `?` rather than left to look exact.

## `pub attempts: Vec<AttemptRecord>,`

Every attempt, oldest first — the escalation trail.

## `pub fn total_cost_usd(&self) -> Option<f64> {`

Implementer plus reviewer, across every attempt.

## `pub fn cost_incomplete(&self) -> bool {`

Whether an attempt reported no spend, making `cost_usd` a floor.

The worker-side twin of [`Self::review_cost_incomplete`], and a method
rather than a field because it is derivable from the attempts already
carried here — no schema change, so an older `report.json` reads back
with the same answer this computes.

Two kinds of attempt land here, and both genuinely spent something
nobody can name: one on a route that reports no dollars at all (Codex
reports tokens — §13), and one the engine was killed inside, whose
`cost_usd` is `null` precisely because the record of its ending was
never written. `unpriced_attempts` counts the same condition for the
capacity estimator, so the ledger and the estimator now agree about
which attempts are unpriced.

## `pub fn trail(&self) -> String {`

Compact escalation trail, e.g. `small×2 failed → mid ok`.

## `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`

How the run ended.

`Parked` is deliberately not `Halted`: §12 requires CI to tell a clean
completion from one that left questions unanswered. `BudgetExceeded` earns
its own variant for the same reason one step further out — "your ceiling
stopped it" is neither a failure nor a question, and `upstroke resume` means
something different after each of the three.

## `pub gates: Vec<String>,`

Effective gate names, and whether they came from config or derivation.

## `pub halted_at: Option<String>,`

Task id the run halted at, if any.

## `pub questions: Vec<QuestionRecord>,`

Every question raised, with its answer where one arrived (§12).

## `#[serde(default)]`

The §13 ceiling that stopped the run, if one did.

## `#[serde(default)]`

What each pool drained, folded from this run's own attempts (§13).

## `#[serde(default)]`

Whether an engine is driving this run right now. A live run must not be
rendered as a finished one: its in-flight attempt has not failed, and
the tasks queued behind it have not been skipped.

## `#[serde(default)]`

Whether this run stopped without ever recording that it finished — the
signature of a kill, a power loss, or an aborting error.

A run in that state has no outcome, and `outcome()` cannot tell: a
killed run has nothing halted, no budget stop and nothing parked, which
is indistinguishable from a clean finish. So the flag has to be carried
rather than derived, exactly as `running` is.

Not to be confused with `RunStatus::interrupted`, which is a `u32`
counting the attempts that were cut off mid-flight. This is the yes/no.

## `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`

One pool's line in the ledger: what this run drew from which subscription.

## `pub cost_usd: Option<f64>,`

Reported api-equivalent dollars, `None` when nothing on this pool
reported any.

## `pub unpriced: u32,`

Attempts whose route reports no spend at all (§13), making the figure
above a floor rather than a total.

## `pub fn total_is_floor(&self) -> bool {`

Whether `total_cost_usd` is a floor rather than a figure.

`total_cost_usd` is an `f64`, so it cannot say this for itself: a run
that reported nothing and a run that genuinely cost nothing both arrive
as `0.0`. The distinction has to be carried alongside, and §13 is
explicit that a ledger which cannot tell free from unreported is worse
than no ledger.

Both halves count. The review side has been marked since step 9; the
worker side became reachable the moment an implementer could report
tokens without dollars, and is now the *normal* case for a
codex-implemented run rather than an edge one.

## `fn committed_count(&self) -> usize {`

How much of the plan actually landed — the one figure every ending
wants, whether the run finished, is still going, or was cut off.

## `pub fn outcome(&self) -> RunOutcome {`

Precedence: a halt outranks a budget stop, which outranks parked work.

That order falls out of what actually happened rather than being a
policy: a halt stops the drain before any further budget check can run,
so a run with both is one that halted and then found its ceiling
irrelevant. And a budget stop leaves tasks parked-or-skipped behind it,
so reporting `Parked` would name a symptom instead of the cause.

## `pub fn from_state(`

Build a report from a replayed log.

`status` and the `report.json` a run writes go through the same
function, so what an operator sees mid-run and what the file says
afterwards cannot drift into disagreeing.

## `pub(super) struct ReportHeader<'a> {`

Everything a report needs that is not the plan or the state, kept together
so `build_report` stays readable at its call sites.

## `pub(super) running: bool,`

Whether an engine is driving this run right now.

## `pub(super) interrupted: bool,`

Whether this run stopped without ever recording that it finished.

## `.chain((0..plan.tasks.len()).filter(|i| !state.order.contains(i)))`

Tasks that never started append in plan order, so the report reads
as the run happened and still accounts for everything.

## `let pool_drain = capacity::drain_of(state.progress.iter().flat_map(|p| p.records.iter()))`

§13's second currency: what each subscription drained, folded from the
same attempt records the dollar column comes from — so the two halves of
the ledger cannot disagree about the same attempt.

## `fn settle(plan: &Plan, states: &[TaskState], running: bool) -> Vec<TaskState> {`

Derive how an ended run's untouched tasks are reported.

This is a *view*, not state, and deliberately not recorded as events. A
task blocked behind an unanswered question has to become runnable again the
moment that question is answered — so if `Blocked` were folded in from the
log, every resume would have to un-fold it. Deriving it fresh from whatever
the log says is true right now means there is nothing to undo.

## `loop {`

Blocking propagates: a dependent of a blocked task is blocked too.
Repeat until stable rather than assuming plan order carries it — a plan
may list a dependent before the task it waits on.

## `if !running {`

Whatever is still Pending was never reached: the run halted. A run that
is still going has not halted — those tasks are queued, or one of them
is working right now — so leave them Pending for `task_report` to tell
apart.

## `fn blocks_dependents(state: &TaskState, running: bool) -> bool {`

Whether a dependency in this state will keep its dependents from ever
running.

`Blocked` means one thing to an operator — "a dependency failed, or parked
and was never answered" — and that is a claim about the future, not the
present. On an ended run the two coincide: anything short of `Done` is
final, because nothing more is coming. On a live one they do not. A
dependency that is merely pending, deferred, or in flight is a task whose
turn has not come, and its dependent is *queued behind* it rather than
blocked by it. Deciding this from `Done`-ness alone made `Queued`
unreachable for every task with a dependency, so the entire first half of a
live run read as a graph of failures.

## `TaskState::Pending | TaskState::Deferred => !running,`

Still on the way. Only an ended run turns that into "never".

## `TaskState::AwaitingInput(_)`

Terminal even mid-run, which is what keeps the propagation working
while the engine is still going: a parked dependency really does
block its dependents until somebody answers.

## `pub(super) fn last_reason(progress: &Progress) -> String {`

Why a task is parked or failed, for the report.

The most recent *attempt failure* wins, not the most recent feedback entry:
the branches that park a task never record feedback, so once an operator has
answered anything, their answer would otherwise shadow every later failure
and the report would tell them a task is parked because they answered a
question. Human entries are excluded from the fallback for the same reason.

## `TaskState::Deferred | TaskState::Pending => match &progress.in_flight {`

On an ended run, Deferred cannot survive `finish` and Pending is
settled away, so both mean the run stopped before this task got
its turn. On a live one `settle` leaves them alone, and the
attempt record says which of the two it is.

Every arm here is about a run that is still going, which is why
both are guarded. `Running` says of itself that only a live
`status` produces it, and a dangling `in_flight` on an ended run
is not a counter-example — it is an attempt whose engine died
between `attempt_started` and `attempt_finished`, which any error
out of `run_attempt` leaves behind. `finish` then wrote it into
`report.json` as `t1: running now — attempt 2 on mid` beside a
top-level `"running": false`: a stored document contradicting
itself, outliving the process that wrote it.

## `let mut seen: Vec<String> = Vec::new();`

Deduped, first-seen order: an escalated task can be judged by one
model on its first rung and another on the next, and both belong
beside a cost that counts both.

## `pub(super) fn total_of(tasks: &[TaskReport]) -> f64 {`

What every task cost, added up.

Deliberately not `Iterator::sum`, which folds floats from `-0.0`. That is
the *correct* additive identity in IEEE 754 — `-0.0 + x` preserves the sign
of `x` where `0.0 + x` does not — but it means the sum of no costs at all is
negative zero, and a run that has not yet spent anything rendered its ledger
as `total: $-0.0000`. Folding from `+0.0` cannot change a non-empty sum,
because the only value `+0.0` fails to preserve is `-0.0`, and a cost is
never that.

## `pub(super) fn sum_opt(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {`

Sum, preserving "nothing was reported" as `None` rather than `0.0` — a
ledger that cannot tell free from unreported is worse than no ledger.

## `pub fn topo_order(plan: &Plan) -> Vec<usize> {`

Stable topological order: among ready tasks, lowest plan index first (§14).
Used for previews and reporting; the live scheduler derives readiness per
step instead, so parked work can be skipped past.

## `for (i, flag) in done.iter_mut().enumerate() {`

Unreachable on a validated plan; degrade to plan order.

## `pub fn render(&self) -> String {`

Human-readable report with one sanitized payload per layout line.
Control characters in recorded fields cannot add terminal commands or
lines. Rendering leaves the report's serializable values unchanged.

## `let partial = if task.review_cost_incomplete { "?" } else { "" };`

`?` marks a total with unreported components — the
Copilot route bills nothing back, so a two-pass review
shows one reviewer's spend and must not read as both.

## `(models, None) => format!(" + review {} $?", models.join(", ")),`

Reviewed only by routes that report no spend (§13) —
say who judged it rather than imply it was free.

## `let worker = match task.cost_usd {`

Same rule as the reviewer half beside it, which has said
`$?` since step 9: a route that reports no spend has not
reported zero. `unwrap_or(0.0)` printed `$0.0000` for a
codex-implemented task while the ledger three lines below
correctly showed `—`, so one run said both.

## `let ending = if self.interrupted {`

Why it never got its turn, since the two endings are not
the same thing to an operator: a halt is a decision the
run reached, an interruption is one that happened to it
and that `resume` undoes.

## `TaskRunStatus::Unknown => {`

Only reachable from a `report.json` written by a newer
upstroke. Say that, rather than picking a familiar-looking
status and being confidently wrong about someone's run.

## `if self.running {`

A live run has no outcome yet, and every arm below claims one. Say
what is true instead: how far it has got.

## `if self.interrupted {`

Neither has a run that stopped without recording a finish, and for
the same reason: there is no outcome to report yet. `outcome()`
cannot see that — a killed run has nothing halted, no budget stop and
nothing parked, which reads as `Complete` — so it used to print `run
complete: N task(s) committed` about a run that died mid-attempt,
one line above `status`'s own `state: interrupted`.

"So far" is the live line's word on purpose: more may yet come, once
somebody resumes. Which is also why the resume command is not
repeated here — the `state:` line in `status` already carries it, and
saying it twice invites the two copies to drift.

## `let stopped = self.budget_stop.as_ref().map_or_else(`

`outcome()` only returns this when `budget_stop` is set, so
the fallback is unreachable — and it says so rather than
naming a plausible ceiling. A specific, checkable, false
claim about the operator's own config is the worst thing to
print here.

## `pub fn render_ledger(&self) -> String {`

§21's definition-of-done (e): what each task cost, and on what.

Implementer and reviewer spend stay in separate columns because they
are different models at different tiers — folding them together makes a
cheap rung look expensive to anyone reading the ledger (§13). An
unreported cost prints as `—` rather than `$0.0000`: a ledger that
cannot tell free from unreported is worse than no ledger.
Cell text is sanitized before measuring widths; row payloads pass
through the same terminal boundary as the report.

## `let partial = |rendered: String, incomplete: bool| {`

A figure that omits a reviewer whose route bills nothing back is not
the total, and this column is where someone decides what a run cost.

## `let rows: Vec<[String; 6]> = self`

Own the formatted cell snapshots so widths are measured from the
same sanitized text that the second pass emits.

## `if self.pool_drain.is_empty() {`

§13's second currency. An empty section means no attempt in this run
named a pool — which is the honest reading of "no pools connected",
and is said rather than left as a blank column that looks like
"nothing was spent".

## `None => "— (this route reports no spend)".to_owned(),`

Every attempt on this pool ran on a route that reports no
spend (§13) — saying "$0.0000" would read as free.

## ``"  stopped by [budgets] {} = ${:.4} before `{}` (§13)",``

The ledger annotates; `render` owns the outcome line and the
resume advice. Printing both put two near-identical
paragraphs, formatted to different precision, with two copies
of the same command, back to back in `upstroke status` — which
reads as two things having happened.
