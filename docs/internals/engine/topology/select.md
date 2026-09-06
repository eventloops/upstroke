# `src/engine/topology/select.rs`

Extended notes for [`src/engine/topology/select.rs`](../../../../src/engine/topology/select.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Selection: the eligibility order, the ceiling, and the checkpoint refusals.

`decisions.sequential_substrate.loop` is one sentence with six branches and
a fixed order, and `eligibility_order` states the part of that order this
module exists to keep: *"eligible integration precedes ready_retry precedes
new ordinary dispatch"*. The order is not a scheduling preference. An
integration that lost to a dispatch would let the queue grow behind a merge
entitlement nothing is using, and a fresh dispatch that beat a `ready_retry`
would abandon a retained session — and with it the cumulative tree the
retry exists to re-gate — for a generation that starts from nothing.

### This module appends nothing

[`select`] is a pure function of the fold, the ceiling and the reported
spend. It returns the branch the loop takes; the loop performs it. That
division is what makes the checkpoint refusal below expressible as a type
rather than as a rule a caller is asked to remember, and it is why the
ceiling is checked *here*: `loop` puts the check inside each admitting
branch, before the provisional reservation and before any effect, so a
breach has to be decided by whatever decides the branch.

### The structural predicates are the fold's

`decisions.admission_and_leases` defines `ready` and `ready_retry` as
"structural over fold state only" and the fold implements them.
[`crate::topology::fold::TopologyFold`] exposes `ready`, `ready_retry`,
`pipeline_reservable`, `structurally_admissible` and
`integration_admissible`, and every one of them is false once the fold is
poisoned. Its `eligible_continuation` reader returns no generation once
poisoned, and excludes lineages with unanswered questions. It exposes
`run_is_ending`, `backoff_pending` and
`questions_open` beside them — statements about the run rather than
authorisations, which is why those three survive a poisoning and why
`derived_outcome` reads the same three. Nothing here re-derives any of
them: a second implementation of "which generation classes hold the
pipeline entitlement", or of "which tasks are waiting on a wait", is two
rules that can disagree, and `wrong_internal_assumption` is this project's
largest measured root cause by a factor of three.

What is left for this module is exactly the packet's own division of
labour: **which** eligible item to take, and **whether the ceiling permits
it**. `CandidateQueue::first_eligible` answers the first of those for an
integration through the fold's question-aware reader; ascending task key
answers it for a dispatch and a retry,
which is §14's "lowest plan index first" over the dense registry keys.

## `pub struct Spend {`

---------------------------------------------------------------------------
Reported spend
---------------------------------------------------------------------------

## `pub struct Spend {`

§13's reported spend, derived by replaying the log.

The ledger's own figure, with the ledger's own honesty: an attempt whose
route reported no dollars contributes nothing, so every number here is a
**floor** rather than a total. `BudgetExceeded4::spent_usd` documents the
same thing about the field this feeds, and `decisions` puts the ceiling
against *reported* dollars deliberately — a table of vendor rates inside a
shipped binary goes stale silently and flatters.

Derived rather than accumulated because INV-02 admits one reader: a live
run and a replay of the bytes it wrote must reach the same ceiling
decision, and a counter that only the live path incremented would not.
[`Self::replay`] is that reader and [`Self::record`] is the single step it
is made of, so the live loop that records each settlement as it appends it
reaches the identical value.

## `impl Spend` › `pub const fn run_total(&self) -> f64 {`

The run's reported spend so far.

A reader because the ceiling is not the only thing that needs it: a
resumed process rebuilds this with [`Self::replay`], and the two totals
have to be comparable or the comparison cannot be asserted.

## `impl Spend` › `pub fn new() -> Self {`

Nothing reported yet.

## `impl Spend` › `pub fn record(&mut self, key: TaskKey, record: &AttemptRecord) {`

Add one attempt's reported dollars to the run and to its task.

Worker and review spend together, exactly as the legacy ledger sums
them: a review pass is spend the attempt caused, and a ceiling that
counted only the implementer would let a two-reviewer route run past it.

## `impl Spend` › `pub fn replay(events: &[TopologyEvent]) -> Self {`

Reported spend over a whole log.

Both event kinds that carry an [`AttemptRecord`] contribute: a failed
attempt records one on `attempt_finished`, and a **successful** one
records it on `candidate_prepared` — `candidate_prepared` is the sole
successful attempt settlement (INV-07), so a reader that only walked
`attempt_finished` would price a run at the cost of its failures.

## `pub fn replay(events: &[TopologyEvent]) -> Self` › `for event in events {`

**One attempt, one contribution — and now by construction rather than
by filtering.** This kept a `BTreeSet` of attempt identities because
a successful attempt's record was appended *twice*: once on
`attempt_finished{Succeeded}` and once on `candidate_prepared`.
Counting each occurrence priced every successful attempt twice, and
only on replay, so a live total and a replay of that run's own log
disagreed — the deduplication existed to hide that.

The `bf927f3` review named the dedup as evidence of the duplicate
rather than a licence for it, and the 2026-08-27 ruling agreed:
`candidate_prepared` is the sole successful settlement, the fold
refuses either half of the old pair, and an attempt's record now
reaches the log exactly once. A failure arrives on `attempt_finished`
and a success on `candidate_prepared`, and no attempt produces both.

Removing it is the point. A filter that survives the shape it was
written for would keep a *second* reading of "one settlement per
attempt" alive beside the fold's, free to disagree with it — and the
one place that rule is enforced should be the one place it is stated.

## `impl Spend` › `pub fn run_usd(&self) -> f64 {`

Reported spend across the whole run.

## `impl Spend` › `pub fn task_usd(&self, key: TaskKey) -> f64 {`

Reported spend attributed to one task.

## `pub struct Ceiling {`

---------------------------------------------------------------------------
The ceiling
---------------------------------------------------------------------------

## `pub struct Ceiling {`

The run's frozen spend ceilings.

A value rather than a trait: there is one rule, it is arithmetic over two
optional limits, and a seam here would only let a test disagree with
production about which limit is stricter.

## `pub struct Ceiling` › `pub run_usd: Option<f64>,`

The whole run's ceiling, when the operator set one.

## `pub struct Ceiling` › `pub task_usd: Option<f64>,`

One task's ceiling, when the operator set one.

## `pub struct Breach {`

A ceiling that refused the next spawn.

## `pub struct Breach` › `pub budget: BudgetKind,`

Which ceiling.

## `pub struct Breach` › `pub limit_usd: f64,`

The limit it names.

## `pub struct Breach` › `pub spent_usd: f64,`

Reported spend against it — a floor. See [`Spend`].

## `impl Ceiling` › `pub const fn unlimited() -> Self {`

No ceilings configured, which never breaches.

## `impl Ceiling` › `pub fn breach(&self, spend: &Spend, key: TaskKey) -> Option<Breach> {`

The breach that refuses the next spawn for `key`, if there is one.

`run_usd` is checked before `task_usd` because it is the stricter
claim: a run at its overall ceiling is done whatever any individual
task has spent, and naming the run budget is what tells the operator
which number to raise.

## `impl Ceiling` › `fn run_breach(&self, spend: &Spend) -> Option<Breach> {`

The run ceiling alone.

Split out because one branch checks this and not [`Self::breach`]: an
integration spawns no worker and is charged to no task. See
[`select`]'s integration branch for why that is not an omission.

## `fn run_breach(&self, spend: &Spend) -> Option<Breach>` › `(spent >= limit).then_some(Breach {`

`>=` rather than `>`: the ceiling refuses the *next* spawn, so
reaching it is already a refusal.

## `impl Ceiling` › `fn task_breach(&self, spend: &Spend, key: TaskKey) -> Option<Breach> {`

One task's ceiling alone, on the same `>=` boundary as the run's.

## `pub enum Step {`

---------------------------------------------------------------------------
The branch
---------------------------------------------------------------------------

## `pub enum Step {`

The branch of `sequential_substrate.loop` this state selects.

One variant per branch of the packet sentence, in the packet's order, so
that "the order is fixed" is a property a reader can check against the
source rather than reconstruct from control flow.

## `pub enum Step` › `Poisoned,`

An append returned an error and this process's fold is poisoned.

Not a branch of the loop — the append-error protocol has already ended
the command. It is here because a predicate that answered `false` and a
selector that then chose *closure* would turn "no further transition"
into "end the run", which is a durable decision derived from a state
this process cannot vouch for.

## `pub enum Step` › `BudgetExceeded(Box<BudgetExceeded4>),`

The ceiling refused the next spawn. Append this **before any effect**,
then proceed to closure.

## `pub enum Step` › `Integrate {`

An eligible integration. Take the `{pipeline, merge}` reservation and
integrate exactly one.

## `pub enum Step` › `candidate: Box<CandidateRef>,`

The candidate `CandidateQueue::first_eligible` chose.

## `pub enum Step` › `Retry {`

A `ready_retry` task. Take the `{pipeline}` reservation and run the
next attempt in the retained generation.

## `pub enum Step` › `key: TaskKey,`

The task.

## `pub enum Step` › `generation: GenerationId,`

Its open, retained generation.

## `pub enum Step` › `attempt: AttemptNumber,`

The attempt number the retry starts: the generation's highest plus
one, which is what `check_attempt_started` requires.

## `pub enum Step` › `Dispatch {`

A `ready` task. Take the dispatch reservation and dispatch.

## `pub enum Step` › `key: TaskKey,`

The task.

## `pub enum Step` › `generation: GenerationId,`

The generation this dispatch opens: dense, so the count of the
task's generations so far.

## `pub enum Step` › `continuing: bool,`

Whether that generation **already exists**, open with no attempt.

`T-DISPATCH`'s resume action is "continue attempt (no spend
repeats)": a run killed between `task_dispatched` and
`attempt_started` leaves the generation `OpenNoAttempt`, recovery
step (g) verifies or recreates its worktree, and the loop starts the
attempt in it. `task_dispatched` is already durable, so the branch
reuses rather than appending a second one.

Not a new branch: `eligibility_order` names "new ordinary dispatch",
and a continuation is not a new one. It is the same branch reaching
the same attempt over ground that already exists.

## `pub enum Step` › `Backoff,`

Deferred work and nothing else. Sleep the defer backoff, then append
`defer_wait_elapsed`.

## `pub enum Step` › `HardBlock {`

Open questions and nothing else. Apply the hard-block rules.

## `pub enum Step` › `questions: Vec<QuestionId>,`

The questions blocking, in id order.

## `pub enum Step` › `Closure(DerivedOutcome),`

Run-end closure is due, with the outcome the fold derives.

## `pub enum Admitted {`

The branches an **intermediate build** is entitled to perform.

[`Step`] has **eight** variants and this has five, so **three** do not
cross: `Integrate`, `Closure` and `Poisoned`. The first two are the whole of
`checkpoint_refusals` for PR7 — there is no value of this type that can
carry an integration or a run end, so no caller holding one can append
`merge_verification_started` or `run_finished`. That is the refusal made
unrepresentable rather than remembered.

The third is not a refusal of a *branch*. `Poisoned` is the absence of one:
an append errored, this process's fold is not authoritative, and nothing
further is selected at all. It is excluded from this type for the same
reason the other two are — a caller holding an `Admitted` may act — but not
for the same cause, and the count said "seven" and "two" until 2026-08-27
precisely by folding it into them.

Both counts are computed, per §22:

```text
$ awk '/^pub enum Step \{/,/^\}/'     src/engine/topology/select.rs | grep -cE '^    [A-Z]'
8
$ awk '/^pub enum Admitted \{/,/^\}/' src/engine/topology/select.rs | grep -cE '^    [A-Z]'
5
```

## `pub enum Admitted` › `BudgetExceeded(Box<BudgetExceeded4>),`

[`Step::BudgetExceeded`].

## `pub enum Admitted` › `Retry {`

[`Step::Retry`].

## `pub enum Admitted` › `key: TaskKey,`

The task.

## `pub enum Admitted` › `generation: GenerationId,`

Its open, retained generation.

## `pub enum Admitted` › `attempt: AttemptNumber,`

The attempt number the retry starts.

## `pub enum Admitted` › `Dispatch {`

[`Step::Dispatch`].

## `pub enum Admitted` › `key: TaskKey,`

The task.

## `pub enum Admitted` › `generation: GenerationId,`

The generation this dispatch opens, or already opened.

## `pub enum Admitted` › `continuing: bool,`

Whether the generation already exists. See [`Step::Dispatch`].

## `pub enum Admitted` › `Backoff,`

[`Step::Backoff`].

## `pub enum Admitted` › `HardBlock {`

[`Step::HardBlock`].

## `pub enum Admitted` › `questions: Vec<QuestionId>,`

The questions blocking, in id order.

## `pub fn select(fold: &TopologyFold, ceiling: &Ceiling, spend: &Spend) -> Step {`

---------------------------------------------------------------------------
Selection
---------------------------------------------------------------------------

## `pub fn select(fold: &TopologyFold, ceiling: &Ceiling, spend: &Spend) -> Step {`

The branch this state selects, in `eligibility_order`.

Appends nothing and performs nothing. The ceiling is consulted only inside
an admitting branch, which is where `loop` puts it: a run with no
admissible work never asks the ceiling anything, because there is no spawn
for it to refuse and `budget_exceeded` is a record *of a refusal*.

## `pub fn select(fold: &TopologyFold, ceiling: &Ceiling, spend: &Spend) -> Step {` › `return Step::Closure(fold.derived_outcome());`

No `run_started`: nothing has been recorded, so nothing is
selectable and nothing has ended.

## `pub fn select(fold: &TopologyFold, ceiling: &Ceiling, spend: &Spend) -> Step {` › `if fold.run_is_ending() {`

**An ending run offers no work, whatever else is live.**

`loop` says a breach "appends `budget_exceeded` before any effect and
**proceeds to closure**", and a halted run is the same shape one cause
over. `run_is_ending()` is `halted_at.is_some() || budget_stop_is_current()`.

**One guard, at the top, rather than one per arm** — and that placement is
the repair rather than an implementation detail. Three of the eligibility
predicates already embed `!run_is_ending()` inside themselves
(`ready`, `ready_retry`, `integration_admissible`) and the fourth,
`open_no_attempt`, does not: it is a *statement* accessor whose doc
correctly declines to consult run state, and recovery step (g) depends on
that — (g) runs before `run_resumed` increments the epoch, so a
budget-stopped run whose reader refused would silently skip rebuilding its
worktrees. Patching the one arm would leave the next arm to be written in
the same position as `open_no_attempt` was.

Found by round 3's `loop` lens, measured end to end: five consecutive
`step()` calls each returned `Progress::BudgetExceeded` and appended a
duplicate stop record, because the continuation was offered, refused by
the ceiling, and offered again — a run that never terminates. With
`halted_at` set the same path returned `Dispatch { continuing: true }`: a
halted run spawning a worker.

## `pub fn select(fold: &TopologyFold, ceiling: &Ceiling, spend: &Spend) -> Step {` › `return match ceiling.run_breach(spend) {`

The **run** ceiling, and not the task's. `BudgetExceeded4::key` is
"the task whose next attempt was refused. Not a failed task:
nothing judged it and nothing was spent on it", and an integration
is neither half of that sentence: it is not that task's next
attempt, and money *was* spent on it — the candidate exists because
an attempt succeeded and was paid for. Charging the task ceiling
here would refuse the *merge* of work already bought, permanently:
the candidate can never integrate and the task can never unspend.
An integration also spawns no worker, and the identities its
verification passes carry are `(sequence, role, ordinal)` rather
than a task's. The run ceiling still binds, because `loop` puts the
check inside every admitting branch and a run at its overall
ceiling is done whatever the branch would have been.

## `pub fn checkpoint(step: Step) -> Result<Admitted, UpstrokeError> {`

`checkpoint_refusals`: "an intermediate build refuses, **before any
append**, any operation whose terminals it does not implement".

PR7 appends `attempt_started` and implements its terminals; it does not
implement `merge_prepared`, `merge_rejected` or `task_merged`, so it never
appends `merge_verification_started` — INV-07's "every checkpoint build
implements every terminal reachable from any start it appends" is that
sentence read from the other end. Run-end closure is refused for the same
reason: `run_finished` is a terminal whose finalization PR7 does not
perform.

The refusal is taken on the [`Step`], which is a value nothing has acted
on: `select` performed no effect and appended nothing, so "before any
append" holds by construction rather than by the caller checking early
enough.

### Errors

[`UpstrokeError::Refused`] naming the operation and the terminals PR7 does
not implement.

## `fn eligible_integration(fold: &TopologyFold) -> Option<CandidateRef> {`

The candidate an eligible integration would take, if one is eligible.

The fold supplies both eligibility and identity, including questions on
other members of the candidate's lineage. The selected step owns its
candidate snapshot after this borrowed view ends.

## `fn first_ready_retry(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, AttemptNumber)> {`

The lowest-keyed `ready_retry` task, its retained generation, and the
attempt number the retry starts.

## `fn first_ready_retry(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, AttemptNumber)> {` › `let task = fold.task(key)?;`

Only a `RetainedIdle` generation is ever `ready_retry`; asking the
class here is not a second predicate but the way this function gets
the number without inventing it.

## `fn first_ready(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, bool)> {`

The lowest-keyed eligible continuation or fresh dispatch.
No selection if no registered task can continue or open a representable
generation; a missing task has no dispatch to offer.

## `fn first_ready(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, bool)> {` › `if let Some(generation) = fold.open_no_attempt(key) {`

**A continuation first, and it cannot compete with a fresh dispatch.**
`T-DISPATCH`'s `authoritative_state` is "generation open
(OpenNoAttempt) ... entitlement derived from the open generation", so
an open generation holds the run's only entitlement at
`max_parallel = 1` and `ready` is false for every other task —
`pipeline_reservable` sees none free. The order between the two
therefore cannot arise in this build.

**`eligibility_order` is silent on it**, naming only "eligible
integration precedes ready_retry precedes new ordinary dispatch",
and a continuation is not a *new* dispatch. Reported as a candidate
erratum rather than chosen here: at a wider pipeline the two can
coexist and the packet will have to say which wins.

## `fn first_ready(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, bool)> {` › `let task = fold.task(key)?;`

refusals[10]: generations are dense per task, so the next one is the
count of the ones recorded.

## `fn open_generation(fold: &TopologyFold, key: TaskKey) -> Option<GenerationId> {`

The open generation of `key`, if it has one.

## `fn keys(fold: &TopologyFold) -> impl Iterator<Item = TaskKey> + '_ {`

Every registered key, ascending.

## `fn backoff_pending(fold: &TopologyFold) -> bool {`

Whether the backoff branch is entered: something is waiting on a wait, and
neither `halted_at` nor this epoch's `budget_stop` is set.

`refusals[18]` refuses `defer_wait_elapsed` under either, and `loop` states
the same guard on the branch — so a selector that offered the branch anyway
would hand the loop an append the fold is about to refuse.

Both halves are the fold's. This function is the **conjunction** and
nothing else: an earlier version walked `0..registry.len()` for a
`Deferred` task while `TopologyFold::backoff_pending` walked its own
`tasks`, and `derived_outcome` reads the fold's. Two rules that can
disagree is precisely what this module's header argues against.

## `fn open_questions(fold: &TopologyFold) -> Vec<QuestionId> {`

The open question ids, in id order.

Whether there are any is [`TopologyFold::questions_open`]; this builds the
payload the branch carries and decides nothing.

## `fn ceiling_or(`

The ceiling check every admitting branch performs, and what it produces.

A breach is [`Step::BudgetExceeded`] carrying the exact event to append;
`loop` says it is appended "before any effect and proceeds to closure", so
the value the loop is handed is the event and not a flag it has to build
one from.

## `Some(breach) => budget_exceeded(epoch, breach, Some(key)),`

"The task whose next attempt was refused. Not a failed task:
nothing judged it and nothing was spent on it."

## `fn budget_exceeded(epoch: Epoch, breach: Breach, key: Option<TaskKey>) -> Step {`

The stop a breach records.

`key` is `None` where no task's next attempt was refused, which is the
integration branch and only it.

## `select` › `if fold.run_is_ending() {`

**An ending run offers no work, whatever else is live.**

`loop` says a breach "appends `budget_exceeded` before any effect and
**proceeds to closure**", and a halted run is the same shape one cause
over. `run_is_ending()` is `halted_at.is_some() || budget_stop_is_current()`.

The fold's eligibility readers also refuse ending runs, including the
continuation reader. This top guard keeps closure ahead of every branch,
including the question branch, whose accounting reader survives a stop.
Continuation originally used `open_no_attempt` directly. Recovery still
needs that accounting accessor before `run_resumed`, when a budget stop
must not hide the worktrees it needs to rebuild.

Found by round 3's `loop` lens, measured end to end: five consecutive
`step()` calls each returned `Progress::BudgetExceeded` and appended a
duplicate stop record, because the continuation was offered, refused by
the ceiling, and offered again — a run that never terminates. With
`halted_at` set the same path returned `Dispatch { continuing: true }`: a
halted run spawning a worker.

## `first_ready` › `if let Some(generation) = fold.eligible_continuation(key) {`

**A continuation first, and it cannot compete with a fresh dispatch.**
`T-DISPATCH`'s `authoritative_state` is "generation open
(OpenNoAttempt) ... entitlement derived from the open generation", so
an open generation holds the run's only entitlement at
`max_parallel = 1` and `ready` is false for every other task —
`pipeline_reservable` sees none free. The order between the two
therefore cannot arise in this build.

**`eligibility_order` is silent on it**, naming only "eligible
integration precedes ready_retry precedes new ordinary dispatch",
and a continuation is not a *new* dispatch. Reported as a candidate
erratum rather than chosen here: at a wider pipeline the two can
coexist and the packet will have to say which wins.

## `first_ready` › `return None;`

This task cannot name another generation in the event format.
