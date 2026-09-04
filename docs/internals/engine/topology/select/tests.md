# `src/engine/topology/select/tests.rs`

Extended notes for [`src/engine/topology/select/tests.rs`](../../../../../src/engine/topology/select/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `fn candidate_of(key: TaskKey, generation: u32) -> CandidateRef {`

-----------------------------------------------------------------------
Fixtures the settlement lane does not need
-----------------------------------------------------------------------

## `fn queue_candidate(fold: &mut TopologyFold, key: TaskKey, generation: u32) -> CandidateRe…`

Take `key` all the way to a queued candidate: dispatch, attempt,
success, prepare, create.

## `fn queue_candidate(fold: &mut TopologyFold, key: TaskKey, g…` › `let candidate = candidate_of(key, generation);`

**No `attempt_finished` between the pin and `candidate_prepared`.**
`candidate_prepared` is the sole successful settlement for a
candidate-producing attempt, and the fold refuses either half of the
pair this fixture used to build — so a fixture that still appended one
would be refused by `apply` rather than quietly agreeing with itself.

## `fn all_failed() -> TopologyFold {`

Every task terminal and nothing queued: the state a run ends from.

## `fn started_at_width(max_parallel: u32) -> TopologyFold {`

`started()` at a stated pipeline width.

Every other selection fixture runs at `max_parallel = 3`, and the
comment on that number is right about why: a test that ordered an
integration ahead of a dispatch because the *entitlement* excluded the
dispatch would prove nothing about `eligibility_order`. But 3 is a
width `config` refuses to create a run at — `DEFAULT_MAX_PARALLEL` is 1
and `[engine] max_parallel` above it is rejected outright — so a suite
with no fixture below 3 never binds the entitlement clause of any
predicate, and never asks what selection does at the only width
production runs.

## `fn an_eligible_integration_precedes_a_retry_precedes_a_dispatch() {`

-----------------------------------------------------------------------
eligibility_order
-----------------------------------------------------------------------

## `fn an_eligible_integration_precedes_a_retry_precedes_a_dispatch() {`

"eligible integration precedes ready_retry precedes new ordinary
dispatch".

Three states that differ by exactly one removed alternative, so each
assertion is about the branch that *lost*: in the first, a retry and a
dispatch were both live and the integration still won; in the second, a
dispatch was live and the retry still won.

## `fn an_eligible_integration_precedes_a_retry_precedes_a_disp…` › `let mut fold = started();`

Without the candidate, the retry wins over the dispatch.

## `fn an_eligible_integration_precedes_a_retry_precedes_a_disp…` › `attempt: AttemptNumber(2),`

The retry runs the *next* attempt of the generation that
retained the session, not a first attempt of a new one.

## `fn an_eligible_integration_precedes_a_retry_precedes_a_disp…` › `let fold = started();`

Without either, the dispatch. Lowest key first.

## `fn nothing_is_selected_at_width_one_while_the_single_entitlement_is_held() {`

At the width production runs, the entitlement decides every branch.

`DEFAULT_MAX_PARALLEL` is 1 and `[engine] max_parallel` above it is
refused for a fresh run, so one held entitlement is a full pipeline. An
`OpenNoAttempt` generation is what a crash between `task_dispatched`
and `attempt_started` leaves holding it, and recovery does not close it
— so this is the state the resumed loop's first `select` sees.

## `fn nothing_is_selected_at_width_one_while_the_single_entitl…` › `assert_eq!(`

**The entitlement's holder is the one thing still selectable**, and
that is `T-DISPATCH`'s "continue attempt (no spend repeats)": this
dispatch opened a generation and started no attempt, so the loop's
job is to start one in it. What the held entitlement forbids is a
*second* claim on it — the queued integration above is no longer
selected, which is what this test is measuring.

## `fn nothing_is_selected_at_width_one_while_the_single_entitl…` › `let mut wider = started_at_width(2);`

One slot wider, the identical state selects the integration: what
this asserts is the count, not something else about the fixture.

## `fn a_dispatch_opens_the_next_dense_generation() {`

A dispatch opens the next dense generation, not generation zero again.

## `fn selection_takes_the_first_eligible_candidate_and_not_the_head() {`

The candidate the queue chooses, not the head of the queue.

`first_eligible` skips an entry whose task is awaiting input rather
than blocking behind it, and this is the selector inheriting that
rather than re-deriving it.

## `fn selection_takes_the_first_eligible_candidate_and_not_the…` › `apply(`

Park ALEPH's task: its candidate keeps its place and loses its turn.

## `fn reported_spend_replays_both_record_carrying_events() {`

-----------------------------------------------------------------------
The ceiling
-----------------------------------------------------------------------

## `fn reported_spend_replays_both_record_carrying_events() {`

Reported spend is derived by replaying the log, and both event kinds
that carry a record contribute.

## `fn reported_spend_replays_both_record_carrying_events()` › `in_flight(&mut fold, ALEPH, 0);`

A failure records on `attempt_finished`.

## `fn reported_spend_replays_both_record_carrying_events()` › `failing.record = record_failing(`

The record says failed, because the settlement does — `record`'s
`failure: None` is "judged and accepted", which is not what an
`attempt_finished` can carry.

## `fn reported_spend_replays_both_record_carrying_events()` › `let mut fold = started();`

A success records on `candidate_prepared`, and a replay that only
walked settlements would price the run at the cost of its failures.

## `fn reported_spend_replays_both_record_carrying_events()` › `let mut floored = Spend::new();`

An unpriced route contributes nothing, which is why the number is a
floor and the field says so.

## `fn reported_spend_replays_both_record_carrying_events()` › `let mut reviewed = record(1, Some(0.125));`

Review spend counts too: a ceiling that priced only the implementer
would let a two-reviewer route run past it. The worker's dollars and
the two passes' are three different numbers so a sum that dropped
one lands somewhere this fixture does not hold.

## `fn reported_spend_replays_both_record_carrying_events()` › `review_costing(None),`

An unpriced pass, which contributes nothing and makes the total
a floor rather than a figure.

## `fn the_ceiling_arm_refuses_on_either_budget_alone() {`

**The selector's ceiling arm checks both budgets, not one.**

`the_run_ceiling_is_checked_before_the_task_ceiling` exercises
`Ceiling::breach` directly and is green for either half alone. The arm
is what the loop actually runs, and catalogue entries `PR7-SELECT-020`
and `PR7-SELECT-023` reduced `ceiling_or`'s call to
`ceiling.task_breach(..)` and `ceiling.run_breach(..)` respectively —
each dropping one comparison — and the whole suite stayed green **twice**.

The two halves need opposite fixtures, and that is the whole reason
neither was caught: a ceiling where both budgets are breached, or
neither, cannot tell the halves apart. Each case below has **headroom in
one budget and a breach in the other**.

## `fn the_ceiling_arm_refuses_on_either_budget_alone()` › `let fold = started();`

Case one: the task is over, the run has room. A dropped
`task_breach` admits an attempt the task's own budget refuses.

## `fn the_ceiling_arm_refuses_on_either_budget_alone()` › `let fold = started();`

Case two, the mirror: the run is over, this task has spent nothing.

## `fn the_run_ceiling_is_checked_before_the_task_ceiling() {`

The run ceiling is named before the task ceiling, and reaching a
ceiling is already a refusal.

## `fn the_run_ceiling_is_checked_before_the_task_ceiling()` › `assert_eq!(`

Over the task ceiling, under the run ceiling.

## `fn the_run_ceiling_is_checked_before_the_task_ceiling()` › `spend.record(BET, &record(1, Some(0.5)));`

Over both: the run ceiling is the stricter claim and is what the
operator is told to raise.

## `fn the_run_ceiling_is_checked_before_the_task_ceiling()` › `let mut exact = Spend::new();`

Exactly at the ceiling refuses the next spawn.

## `fn the_run_ceiling_is_checked_before_the_task_ceiling()` › `let task_only = Ceiling {`

And on the task arm, which is the same boundary and a separate
comparison. `0.5` and `0.5` are exact in binary, so `>` here admits
the spawn the operator's limit has already refused and `>=` does
not — there is no epsilon in which the two agree.

## `fn a_run_with_no_admissible_work_never_asks_the_ceiling() {`

The ceiling is consulted only inside an admitting branch: a run with
nothing to spawn never records a refusal of a spawn.

## `fn a_breach_appends_budget_exceeded_and_integration_and_run_end_are_refused() {`

-----------------------------------------------------------------------
checkpoint_refusals
-----------------------------------------------------------------------

## `fn a_breach_appends_budget_exceeded_and_integration_and_run_end_are_refused() {`

The checkpoint refusal, in the three shapes `checkpoint_refusals` and
`loop` give it.

A budget breach with structurally admissible work appends
`budget_exceeded` **before any spawn**; integration and run end are
refused **before any start append**.

## `fn a_breach_appends_budget_exceeded_and_integration_and_run…` › `let fold = started();`

(1) A breach with work to do. `select` is a pure function — it
performs no effect and appends nothing — so "before any spawn" is
structural, and what the loop is handed is the event itself.

## `fn a_breach_appends_budget_exceeded_and_integration_and_run…` › `assert_eq!(`

It is not a start, so the checkpoint admits it, and the fold takes
it — after which the run is ending.

## `fn a_breach_appends_budget_exceeded_and_integration_and_run…` › `let mut fold = started();`

(2) An eligible integration is refused before the
`merge_verification_started` that would start one.

## `fn a_breach_appends_budget_exceeded_and_integration_and_run…` › `let mut spend = Spend::new();`

The ceiling is checked *inside* the integration branch and before
it, so a breach with an eligible integration records the stop rather
than the refusal.

## `fn a_breach_appends_budget_exceeded_and_integration_and_run…` › `let fold = all_failed();`

(3) Run-end closure is refused before `run_finished`.

## `fn the_checkpoint_admits_every_branch_this_build_implements() {`

Every branch an intermediate build *is* entitled to perform survives
the checkpoint unchanged.

## `fn every_step_variant_is_admitted_or_refused_and_the_split_is_five_three() {`

**Which of `Step`'s variants cross the checkpoint, counted rather than
asserted in prose.**

`Admitted`'s doc said "[`Step`] has seven variants and this has five.
The two that are missing…" for as long as `Step` had **eight** and three
were missing. The undercount folded `Poisoned` into the two
`checkpoint_refusals` branches, which is a different thing: `Integrate`
and `Closure` are branches this build declines to perform, and
`Poisoned` is the absence of a branch — the fold is not authoritative
and nothing is selected at all.

The `match` below has **no wildcard arm**, so adding a variant to `Step`
stops this file compiling until someone says which side it falls on.
That is the part a count in a doc comment cannot do.

## `fn every_step_variant_is_admitted_or_refused_and_the_split_…` › `let mut names = Vec::new();`

Exhaustive by construction: no `_` arm, so a ninth variant is a
compile error here rather than a silently untested branch.

## `fn every_step_variant_is_admitted_or_refused_and_the_split_…` › `let mut distinct = names.clone();`

On a COPY: `names` must stay in the list's order, because it is
zipped with it below. Sorting it in place paired every step with
another step's label and the assertion read
`["Backoff", "Closure", "Retry"]`.

## `fn the_retry_branch_checks_the_ceiling_and_names_the_retained_task() {`

-----------------------------------------------------------------------
The remaining branches
-----------------------------------------------------------------------

## `fn the_retry_branch_checks_the_ceiling_and_names_the_retained_task() {`

The retry branch checks the ceiling before it admits the retry.

Its own branch and not the dispatch branch's: `loop` puts the check
inside **each** admitting branch, and `ALEPH` is `ready` here, so a
selector that admitted the retry unconditionally would still have a
later branch to fall through to and a `BudgetExceeded` to produce from
it. The assertion is therefore on `key`: only the retry's own check
names the retained task.

## `fn the_retry_branch_checks_the_ceiling_and_names_the_retain…` › `let ceiling = Ceiling {`

`BET` is over its own ceiling; `ALEPH` has spent nothing.

## `fn the_retry_branch_checks_the_ceiling_and_names_the_retain…` › `assert_eq!(`

Under the ceiling, the same state runs the retry.

## `fn a_ready_dispatch_precedes_the_backoff_when_both_are_live() {`

**A ready dispatch precedes the defer backoff, and both are live at once.**

The order `loop` fixes, and the one adjacent pair no fixture held.
`the_backoff_branch_precedes_the_hard_block_when_both_are_live` pins the
pair below this one; `an_eligible_integration_precedes_a_retry_precedes_a_dispatch`
pins the three above it. Between them sat `first_ready` / `backoff_pending`,
and S5 round 4 measured the swap — the defer backoff selected **before** a
ready dispatch, which is the starvation this module's header warns about
("The order is not a scheduling preference") — leaving the **entire suite
green**.

A run with runnable work must not sleep on a wait that belongs to a task
which is not the one it could be running.

## `fn a_ready_dispatch_precedes_the_backoff_when_both_are_live…` › `assert!(`

Both premises, because "not Backoff" is satisfied by a fold where the
backoff was never pending in the first place.

## `fn the_backoff_branch_precedes_the_hard_block_when_both_are_live() {`

Backoff precedes the hard block, and the two are live at once.

`loop`'s order is fixed, and no other fixture holds a `Deferred` task
**and** an open question at the same time — so with the two branches
swapped every one of them still passes. A deferred task is waiting on a
wait that will elapse on its own; a question waits on a person. Serving
the person first would park a run that was about to make progress.

## `fn the_backoff_branch_precedes_the_hard_block_when_both_are…` › `apply(&mut fold, &resume_event());`

With the wait elapsed and the woken task run out, the question is
what is left — the other half of the same order.

## `fn an_integration_is_charged_to_the_run_and_never_to_the_candidates_task() {`

An integration is charged to the run and never to the candidate's task.

`BudgetExceeded4::key` is "the task whose next attempt was refused. Not
a failed task: nothing judged it and nothing was spent on it", and an
integration is neither half: it is not that task's next attempt, and
money *was* spent — the candidate exists because an attempt succeeded
and was paid for. Charging the task ceiling would refuse the merge of
work already bought, and refuse it permanently: the candidate can never
integrate and the task can never unspend.

## `fn an_integration_is_charged_to_the_run_and_never_to_the_ca…` › `assert_eq!(`

The same ceiling still refuses that task's next *attempt*, which is
what it is a ceiling on.

## `fn the_backoff_branch_is_entered_only_while_the_run_is_not_ending() {`

The backoff branch, and the guard that keeps it out from under a halt
or a budget stop.

## `fn the_backoff_branch_is_entered_only_while_the_run_is_not_…` › `let mut woken = fold.clone();`

Waking it returns the task to an ordinary dispatch.

## `fn the_backoff_branch_is_entered_only_while_the_run_is_not_…` › `let mut halted = started();`

A halt: the branch is not offered, and the closure is.

## `fn the_backoff_branch_is_entered_only_while_the_run_is_not_…` › `let mut stopped = started();`

A budget stop in this epoch: likewise.

## `fn arm_label(step: &Step) -> &'static str {`

The label of a step, total over [`Step`].

The reason it is a `match` and not a list: adding a variant to `Step` is
a **compile error here**, which is what lets
[`an_ending_run_offers_no_work_from_any_arm`] claim "every arm" and have
the claim mean something. A list of names someone remembers to extend is
how that test came to cover three of six while its own doc said every.

## `const OFFERS_WORK: &[&str] = &[`

Every label [`arm_label`] can return for a step that **offers work**.

**Below `arm_label`, not above it.** These two `const`s were inserted
between that function's doc block and the function, so the block
attached to `OFFERS_WORK` and `arm_label` rendered undocumented —
occurrence 10 of `reviews/FINDINGS.md` §4's doc-re-targeting class,
committed by the commit whose ledger entry corrected that class's own
count. `clippy::doc_lazy_continuation` does not fire here because the
stranded block's last line is prose rather than a list item, which is
the half §4 records that detector cannot see. `PR7-R6-ATT-005`.

## `const OFFERS_NO_WORK: &[&str] = &["Poisoned", "BudgetExceeded", "Closure"];`

And every label for a step that does not. None of the three is work, and
each is a state an ending run is allowed to reach.

**Pinned by name, and that is what ties membership to behaviour.** The
census below checks only that `arm_label`'s literals equal the union of
these two lists, so moving a *work* label into this one would satisfy it
and quietly drop that arm from the ending witness's coverage
requirement — `PR7-R6-LOOP-008`. These three are structural and cannot
grow: a poisoned fold, a budget stop, and closure. So a seventh label has
exactly one place to go, and the coverage assertion then demands a case
for it.

## `fn every_label_the_arm_classifier_returns_is_classified() {`

**Every label [`arm_label`] can return is classified.**

The half of "every arm" that the type does not give. `arm_label` is total
over [`Step`], so a new variant is a compile error there — measured by S5
round 5, which added a `Step::Provision` and saw `E0004` exactly as
claimed. But the claim went one step further and said the new arm "cannot
then be left out of this test without the coverage assertion failing",
and **that half was false**: once the author satisfies the compiler with
`Step::Provision => "Provision"`, [`OFFERS_WORK`] is a hand-written
`const` nothing forces them to extend, and
`an_ending_run_offers_no_work_from_any_arm` passes with the new arm
undriven. `PR7-R5-LOOP-002`, `R5-SEAMS-004`, `R5-SETTLE-003`.

So this closes the loop from the other end: it reads `arm_label`'s own
match body out of this file and asserts every literal it returns appears
in exactly one of the two lists. A new variant now costs three edits and
**none of them can be skipped** — the match arm (rustc), the
classification (this test), and, if it offers work, a case in the
witness (that test's own coverage assertion).

The body is bounded by brace matching rather than by a line count,
because this file's own history has three occurrences of an anchor going
stale under `cargo fmt` alone.

## `fn an_ending_run_offers_no_work_from_any_arm() {`

**An ending run offers no work — from every arm, not from the empty fold.**

The property is "an ending run proceeds to closure". What was asserted
before `PR7-R3-LOOP-001` was "an *idle* ending run does":
`a_run_with_no_admissible_work_never_asks_the_ceiling` drives an
`all_failed()` fold, where **nothing else is live**, and
`a_breach_appends_budget_exceeded_and_integration_and_run_end_are_refused`
asserts the closure on the same shape. That is the scoping gap round 3
harvested.

**A correction, because the version of this comment that shipped cited a
test that does not exist.** It named `an_ending_run_reaches_closure` as
the predecessor whose scope this widens;
```text
$ grep -rn 'an_ending_run_reaches_closure' --include='*.rs' src/ | grep -v '///'
(no output)
```

**Zero code occurrences.** The doc-comment filter is not tidiness: this
sentence quotes the name, so the unfiltered command matches *itself* and
reports a hit for a test that does not exist. `reviews/FINDINGS.md` §4
carries that as a class — a command quoted as evidence becomes part of
its own input — and it is the documentation half of
`PR4-CENSUS-COMMENT-ORACLE`. The two tests named above are the real
predecessors. §19, claim (1).

`PR7-R3-LOOP-001` is what got through the gap.
`TopologyFold::open_no_attempt` is a statement accessor and — correctly,
and unlike `ready`, `ready_retry` and `integration_admissible` — consults
no run state, so the continuation arm offered work on a budget-stopped
run. Measured end to end by that lens: five `step()` calls, five
duplicate `budget_exceeded` records, no closure; and with `halted_at`
set, `Dispatch { continuing: true }` — a halted run spawning a worker.

**"Every arm" is now the whole of `select`'s work-offering surface**, and
it is checkable rather than asserted: [`arm_label`] is total over `Step`,
[`OFFERS_WORK`] is the subset of its labels that offer work, and the six
cases below are asserted to *cover that subset exactly*. A seventh arm
cannot be added without `arm_label` failing to compile, and it cannot
then be left out of this test without the coverage assertion failing.
The version this replaces covered `Dispatch`, `Dispatch (continuing)` and
`Retry`, and its doc claimed all of them — §19, claim (5).

**What the top guard is load-bearing for, measured rather than assumed.**
Delete `select`'s `if fold.run_is_ending()` and this test reports **two**
arms:

```text
Dispatch (continuing) -> Dispatch { …, continuing: true }
HardBlock             -> HardBlock { questions: [QuestionId("q-aleph-park")] }
```

Four of the six are protected twice over: `ready`, `ready_retry` and
`integration_admissible` embed `!run_is_ending()` in the fold, and this
module's own `backoff_pending` wrapper embeds it here. The two that rest
on the guard **alone** are the continuation — `PR7-R3-LOOP-001`, which is
why the guard exists — and `HardBlock`, which is the same shape one
accessor over: `TopologyFold::questions_open` is a statement accessor and
consults no run state either.

`HardBlock` was not witnessed before this widening. The guard already
covered it, so this is not an open defect — it is the difference between
a guard that happens to be correct and one that is held to it, and the
three-arm version could not tell them apart.

## `fn an_ending_run_offers_no_work_from_any_arm()` › `type Arm = (&'static str, fn() -> TopologyFold);`

Each case: a fold where THIS arm is live, then the same fold ended.
The live assertion is the premise, and it names the arm — asserting
merely "not closure" let a case pass on a *different* arm being live,
which is how three cases were mistaken for six.

## `fn an_ending_run_offers_no_work_from_any_arm()` › `type Arm = (&'static str, fn() -> TopologyFold);`

A fixture builder for one arm, and the label its fold must select.

## `fn an_ending_run_offers_no_work_from_any_arm()` › `let mut fold = started();`

Deferred work and nothing else runnable: `Backoff` sits below
the three dispatching arms, so every other task must be out.

## `fn an_ending_run_offers_no_work_from_any_arm()` › `let mut fold = started();`

An open question and nothing else — including no deferred
task, because `Backoff` precedes this branch.

## `fn an_ending_run_offers_no_work_from_any_arm()` › `offered.push(format!("{arm} -> {after:?}"));`

Accumulated rather than asserted in the loop: a guard that
stops covering three arms should report three, not the first
one the case order happens to reach.

## `fn a_halted_run_offers_no_work_from_the_arms_that_rest_on_the_guard() {`

**A halted run offers no work either — the guard's other disjunct.**

`run_is_ending()` is `halted_at.is_some() || budget_stop_is_current()`
and every case in [`an_ending_run_offers_no_work_from_any_arm`] ends its
run the second way. So the halted half was unpinned: S5 round 4 measured
`if fold.run_is_ending() && fold.halted_at().is_none()` — the guard with
the halted disjunct dropped — surviving the **whole suite**, twice.

A halted run that keeps offering work is the worse of the two: a budget
stop at least appends a record each iteration, and `halts_run` is set by
a task that asked the run to stop.

Two arms rather than six, and the reason is measured rather than
economised: those two are the ones that rest on the guard at all — the
other four embed `!run_is_ending()` in their own predicate, so they are
closed against a halt by the same code that closes them against a
breach. The proof is in the sibling test's doc: delete the guard and it
reports exactly `Dispatch (continuing)` and `HardBlock`.

## `fn a_halted_run_offers_no_work_from_the_arms_that_rest_on_t…` › `fn settle_bet(fold: &mut TopologyFold, halts: bool) {`

`BET` fails, and `halts` decides whether it asks the run to stop.

**One field varied and everything else held constant**, because a
halted run cannot be built by *adding* a settlement to a fold where
the hard block is already live: the hard block needs every task
settled, and a halt needs a task left to settle. The control fold is
the same fold with `halts_run = false`, so the comparison isolates
the flag rather than the shape.

## `fn a_halted_run_offers_no_work_from_the_arms_that_rest_on_t…` › `fn continuation(halts: bool) -> TopologyFold {`

The continuation arm: `TopologyFold::open_no_attempt` is a statement
accessor, so nothing below the guard refuses on its behalf.

## `fn a_halted_run_offers_no_work_from_the_arms_that_rest_on_t…` › `fn hard_block(halts: bool) -> TopologyFold {`

The hard block: `TopologyFold::questions_open` is the same shape one
accessor over.

## `fn a_halted_run_offers_no_work_from_the_arms_that_rest_on_t…` › `assert!(`

The premise: this ends the run the **other** way. A fixture that
also carried a current budget stop would pass with the halted
disjunct deleted, which is the mutation this test exists for.

## `fn open_questions_reach_the_hard_block_branch_before_closure() {`

The hard-block branch: open questions and nothing else runnable.

## `fn open_questions_reach_the_hard_block_branch_before_closur…` › `assert_eq!(`

Left to itself the fold would already end this run Parked, which is
exactly why the branch order matters.

## `fn a_poisoned_fold_selects_nothing() {`

A poisoned fold selects nothing and is refused.

## `fn an_unstarted_run_selects_nothing() {`

A fold with no `run_started` has recorded nothing, so nothing is
selectable and nothing has ended.

## `fn the_selected_retry_is_the_one_the_settlement_module_runs() {`

The retry the selector names is the one the settlement module runs.

Two modules deciding "which generation, which attempt" independently is
two rules that can disagree; this is the assertion that they do not.
