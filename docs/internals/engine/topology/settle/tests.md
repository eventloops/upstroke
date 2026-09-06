# `src/engine/topology/settle/tests.rs`

Repository source for these notes: [`src/engine/topology/settle/tests.rs`](../../../../../src/engine/topology/settle/tests.rs).
[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/engine/topology/settle/tests.rs).
The relative link works in a checkout or on GitHub; the GitHub link also works from the published site.

The code is the authority for what it does. The explanatory prose is preserved below.
Each backticked part of a section heading is an exact source excerpt. Search for the final
excerpt within the preceding item when a heading names both an item and a line inside it.

## `pub(crate) const ALEPH: TaskKey = TaskKey(0);`

-----------------------------------------------------------------------
Fixtures

Written for this lane rather than shared with the fold's or the census's:
a settlement that explored the fixture the transition table was built
against would agree with it about a shape neither had questioned.

Three tasks with **no dependencies between them**, over three disjoint
regions. That is what lets an eligible integration, a `ready_retry` and a
`ready` dispatch all be true at once, which is the only state in which
`eligibility_order` says anything at all.
-----------------------------------------------------------------------

## `pub(crate) fn sha(role: &str) -> CommitSha {`

A 40-character symbolic sha, one per role, with no shared prefix.

## `fn chain(task: &str) -> ChainSummary` › `let tiers = if task == "aleph" {`

Two rungs for `aleph` so an escalation has somewhere to go, one for
the others so the top rung is reachable in a single failure.

## `fn run_started_unauthenticated() -> RunStarted4` › `limits: TopologyLimits {`

`max_parallel` is 3 so that the pipeline entitlement never
decides an eligibility-order question: a test that ordered
integration ahead of a dispatch because the *ceiling on
parallelism* excluded the dispatch would prove nothing about
`eligibility_order`.

## `fn run_started_unauthenticated() -> RunStarted4` › `enabled: Some(true),`

Enabled: this fixture's successful attempts record a passed
`review` pass, and a run that froze verification off obliges
none. The two together were a shape production cannot write.

## `fn run_started_unauthenticated() -> RunStarted4` › `second_opinion: vec![None, None, None],`

One entry per task: the registry refuses a plan whose
second-opinion list is not aligned with `plan.tasks`, and
this fixture's plan has three.

## `pub(crate) fn apply(fold: &mut TopologyFold, event: &TopologyEvent) {`

Apply `event`, refusing to continue if the fold does not accept it: a
fixture that silently skipped an event would put every later assertion
on a state nobody built.

## `pub(crate) fn started() -> TopologyFold {`

A fold that has recorded its `run_started` and nothing else.

## `pub(crate) fn region(key: TaskKey) -> PathSet {`

The region an ordinary dispatch of `key` predicts.

The frozen hint is `src/{label}/`; the derivation trims the trailing
separator, and this is the derivation rather than the hint. The two
spellings are one region to `paths_overlap`, which is why the fixture
could carry the wrong one until `check_dispatched` began comparing the
recorded region against the derived one.

## `pub(crate) fn record_failing(`

A record of an attempt that failed the way `failure` says.

**A settlement of a failure whose record carries none is a fixture that
cannot happen.** `record`'s `failure: None` means "the work was judged
and accepted", and every `settle_failed` case is by definition not that.
The allowance is decided from this field, so a grid that varied `Next`
and left the failure fixed varied one half of a correlated pair — the
class `reviews/FINDINGS.md` §4 records eleven of.

## `reviews: if failure.is_none() {`

A success premise carries the primary pass §11.2 requires; an
empty list satisfies `is_successful` vacuously and witnesses
nothing about its review clause. A gate failure never reached a
reviewer, so the failing variant's list is empty because it is —
not for want of a fixture.

## `pub(crate) fn finished(key: TaskKey, generation: u32, attempt: u32, next: Next) -> FinishedAttempt {`

A `FinishedAttempt` with every field at a value of its own, so a
settlement that read one field where it meant another lands somewhere
this fixture does not hold.

## `pub(crate) fn finished(key: TaskKey, generation: u32, attempt: u32, next: Next) -> FinishedAttempt {` › `record: record_failing(`

**A failed settlement's record says failed.** This used
`record(attempt, Some(0.5))`, whose `failure: None` means "the work
was judged and accepted" — the very shape the comment on
`record_failing` calls "a fixture that cannot happen", two hundred
lines above. `check_attempt_finished` refuses it since 2026-08-27.

## `pub(crate) fn in_flight(fold: &mut TopologyFold, key: TaskKey, generation: u32) {`

Dispatch `key` into generation `generation` and start attempt 1.

## `pub(crate) fn settle_into(fold: &mut TopologyFold, finished: &FinishedAttempt) -> AttemptFinished4 {`

Settle `key`'s in-flight attempt through the module under test.

## `pub(crate) fn resuming(request: &mut FinishedAttempt, session: &SessionId) {`

Give `request` a session to resume — in **both** places the attempt
carries one.

`FinishedAttempt` holds the id twice: on `session`, which the settlement
records, and on `record.session_id`, which the ledger line reports.
Production fills both from `assessed.outcome.session_id`, so they are
one value there; a fixture that set only the first would build a
retained settlement whose two halves name different conversations, which
`check_attempt_finished` refuses.

## `pub(crate) fn retained_generation(`

A retained generation of `key`, held by the current epoch.

## `pub(crate) struct FixedVerify {`

-----------------------------------------------------------------------
A verify double
-----------------------------------------------------------------------

## `pub(crate) struct FixedVerify {`

A [`WorktreeVerify`] whose answer the test fixes.

The seam exists because this module may name neither
`std::process::Command` nor raw `std::fs`, so no test here can build the
repository [`ManagedWorktrees`] is derived from. It records what it was
asked, so a test can assert that the retry verified **the retained
cumulative tree** rather than the base — the one distinction a double
that only answered yes or no would lose.

## `pub(crate) struct Recorded(Mutex<Vec<Duration>>);`

A sleeper that records rather than sleeps.

## `fn each_ladder_decision_maps_to_its_own_settlement() {`

=======================================================================
T-FAILED
=======================================================================

## `fn each_ladder_decision_maps_to_its_own_settlement() {`

Every settlement the ladder can decide, mapped once.

A grid rather than six tests because the property is that the six
answers are **different**: a mapping that collapsed two of them would
pass any single-case test.

## `fn each_ladder_decision_maps_to_its_own_settlement()` › `let cases: Vec<(`

**Each decision beside the failure that produces it.** The allowance
is a function of the failure, not of the decision, so a grid that
varied `Next` against one fixed record would be asserting a mapping
that no `next_step` can reach — and would have kept passing while
`settle_failed` derived the allowance from the wrong field.

## `fn each_ladder_decision_maps_to_its_own_settlement()` › `Next::Defer,`

The one deferral: an outage. `next_step` defers precisely so
that a busy pool does not burn an attempt.

## `fn each_ladder_decision_maps_to_its_own_settlement()` › `defers: 5,`

4 recorded + this one.

## `fn each_ladder_decision_maps_to_its_own_settlement()` › `Next::AskHuman(QuestionKind::Unblock),`

**This cell is the defect this grid now catches.** A park
from `NeedsHuman` spends nothing — "the code was never
judged, so nothing is spent and nothing escalates" — and the
settlement used to answer `true` here, because `AskHuman` is
not `Defer` and that was the whole of its rule.

## `fn each_ladder_decision_maps_to_its_own_settlement()` › `lease: LeaseDisposition::PredictedReleased,`

An ordinary generation that closes releases its
predicted region.

## `fn each_ladder_decision_maps_to_its_own_settlement()` › `let mut request = finished(ALEPH, 0, 1, Next::RetrySameRung { resume: true });`

`RetrySameRung { resume: true }` with a session is the one that does
*not* close, and it is the only one that records an incarnation.

## `fn each_ladder_decision_maps_to_its_own_settlement()` › `let sessionless = finished(ALEPH, 0, 1, Next::RetrySameRung { resume: true });`

The ladder's permission without a session closes: there is nothing
to resume, so the retry starts a fresh generation.

## `fn the_lease_disposition_is_the_generations_own() {`

A repair's settlement never releases the lineage lease, and the
disposition is read from the fold rather than restated.

## `fn the_lease_disposition_is_the_generations_own()` › `apply(`

The fold is the authority: `check_lease_disposition` refuses any
other answer, so the settlement applying is the assertion that this
one came from `GenerationLease::expected` and not from a constant.

## `fn candidate_prepared_is_the_sole_successful_settlement() {`

**`candidate_prepared` is the successful settlement, and the fold refuses
either half of the pair that used to stand in for it.**

Re-derived from `a_successful_settlement_promotes_the_generation_and_keeps_its_region`,
which asserted that an `attempt_finished{Succeeded}` promotes the generation
— the event `design/26_design_merge_queue_protocol.md` §26 says is
"not also emitted for that attempt". The old test was not wrong about the
build; it was a witness for a shape the record forbids, and re-deriving it
against the invariant is the point of the 2026-08-27 CONFORM ruling. It was
not patched to pass.

Three claims, because the invariant has three parts: the settlement lands on
`candidate_prepared`; an `attempt_finished` that settles `succeeded` is
refused whatever else is true; and a `candidate_prepared` for a generation
that is *already* promoted is refused, so neither order of the old pair can
be written.

## `fn candidate_prepared_is_the_sole_successful_settlement()` › `let mut fold = started();`

(1) The settlement is this event. An in-flight generation reaches
    `Promoting` by applying it, with no `attempt_finished` in between.

## `fn candidate_prepared_is_the_sole_successful_settlement()` › `let mut fold = started();`

(2) An `attempt_finished` that settles `succeeded` is refused outright.

## `fn candidate_prepared_is_the_sole_successful_settlement()` › `let mut fold = started();`

(3) And the other order: a generation already promoted may not then
    prepare a candidate, so a log carrying both is refused whichever
    event it reaches first.

## `fn a_question_is_read_back_from_the_event_and_not_re_decided() {`

`T-FAILED.resume_action`: "rematerialize question from the event …
never re-decide".

## `fn a_question_is_read_back_from_the_event_and_not_re_decided() {` › `let open = fold.open_questions().expect("started");`

The fold opened exactly that question, under exactly that id.

## `fn a_question_is_read_back_from_the_event_and_not_re_decided() {` › `let mut other = started();`

Every other settlement rematerializes nothing: a reader that
answered `Some` for a non-parking settlement would write a question
payload for a task nobody is waiting on.

## `fn a_park_without_a_question_is_refused() {`

A park that carries no question is refused rather than settled with an
invented one.

## `fn a_settlement_naming_the_wrong_generation_is_refused() {`

A settlement naming a generation that is not the open one is refused
before it can be built.

## `fn deferred_task_woken_by_defer_wait_elapsed_or_resume() {`

`deferred_task_woken_by_defer_wait_elapsed_or_resume`.

## `fn the_defer_backoff_doubles_caps_and_resets() {`

The backoff doubles and is capped, and progress resets it.

## `fn the_defer_backoff_doubles_caps_and_resets()` › `assert_eq!(`

**And what the event carries**, which is a different claim from what
the accumulator holds — `reviews/FINDINGS.md` §4's "an accumulator's
witness proves the accumulation and not the read", at four
occurrences. `DeferWaitElapsed4.round` is documented on the wire, a
frontier reviewer reads it there, and until this line nothing asserted
the value a run actually writes.

## `fn deferred_task_does_not_block_halted_or_budget_exceeded_closure() {`

`deferred_task_does_not_block_halted_or_budget_exceeded_closure`.

## `fn deferred_task_does_not_block_halted_or_budget_exceeded_closure() {` › `let mut fold = started();`

Halted.

## `fn deferred_task_does_not_block_halted_or_budget_exceeded_closure() {` › `let refused = fold`

And the wait it is deferred behind can no longer elapse.

## `fn deferred_task_does_not_block_halted_or_budget_exceeded_closure() {` › `let mut fold = started();`

BudgetExceeded.

## `fn halting_settlement_starts_closure() {`

`halting_settlement_starts_closure`.

## `fn halting_settlement_starts_closure()` › `let mut fold = started();`

A non-halting terminal failure of the same shape does not: the
control that separates "a task failed" from "the run is over".

## `fn halting_drain_settlement_after_budget_exceeded_yields_halted() {`

`halting_drain_settlement_after_budget_exceeded_yields_halted` (ST-17).

## `fn halting_drain_settlement_after_budget_exceeded_yields_halted() {` › `apply(&mut fold, &budget_exceeded(Epoch(0), BET));`

The ceiling refused BET's next attempt; ALEPH's attempt is still in
flight and drains.

## `fn retained_generation_closed_before_run_resumed() {`

=======================================================================
T-RETAINED
=======================================================================

## `fn retained_generation_closed_before_run_resumed() {`

`retained_generation_closed_before_run_resumed`.

The ordering is the whole protection and this test says why: **before**
recovery step (e) the fold cannot tell a fresh process from the
retaining one, because `retained_incarnation == state.resumes` and a
fresh process has not resumed yet. `ready_retry` is therefore *true* at
that prefix, and what keeps a fresh process out of the retained session
is that (e) runs before (h) and `ready_retry` is never evaluated before
(h).

## `fn retained_generation_closed_before_run_resumed()` › `let next = dispatch(ALEPH, 1);`

"any attempt to recreate a Retained worktree at base" is refused:
the new generation is a *new* one, at the current head.

## `fn retained_generation_closed_before_run_resumed()` › `assert!(`

Nothing is retained any more.

## `fn retained_generation_closed_when_worktree_missing() {`

`retained_generation_closed_when_worktree_missing` (ST-11).

## `fn retained_generation_closed_at_run_end() {`

`retained_generation_closed_at_run_end` (ST-17).

## `fn only_an_idle_generation_is_closed() {`

A generation with an attempt in flight is not closed: `refusals[15]`.

## `fn only_an_idle_generation_is_closed()` › `let mut fold = started();`

An `OpenNoAttempt` generation *is* closed — the other half of the
rule, so a refusal that had swallowed both would fail here.

## `fn same_generation_retry_regates_cumulative_tree() {`

=======================================================================
T-RETRY
=======================================================================

## `fn same_generation_retry_regates_cumulative_tree() {`

`same_generation_retry_regates_cumulative_tree` (ST-15).

## `fn same_generation_retry_regates_cumulative_tree()` › `assert_eq!(`

The verify asked for the *cumulative tree*, not the base. A retry
verified against the base passes on a worktree that was reset, and
then re-gates an empty tree as if it were the retained one.

## `fn same_generation_retry_regates_cumulative_tree()` › `assert!(!reservations.is_empty(), "the retry took no reservation");`

The reservation bridges the selection to the append and is converted
at it.

## `fn retry_refused_after_resume() {`

`retry_refused_after_resume`.

## `fn retry_refused_with_stale_incarnation() {`

`retry_refused_with_stale_incarnation`.

Two directions, because the field has two ends. Writing it: a
settlement takes `retained_incarnation` from the fold's **epoch**, so a
run that has resumed once retains for `Epoch(1)`. Reading it: an
`attempt_started` naming an epoch the run has moved past is refused by
the fold itself, whatever a caller decided.

## `fn retry_refused_with_stale_incarnation()` › `let stale = ev(TopologyEventBody::AttemptStarted {`

Reading it: a hand-built retry naming the previous epoch's session.

## `fn retry_refused_with_stale_incarnation()` › `let mut other = started();`

And a settlement that claimed an epoch other than the fold's is
refused too — the field is checked on the way in as well as out.

## `fn retained_worktree_with_residue_closed_not_retried() {`

`retained_worktree_with_residue_closed_not_retried`.

## `fn retained_worktree_with_residue_closed_not_retried()` › `apply(`

INV-06: it is closed, and it is **never recreated** at its base.

## `fn only_a_retained_generation_is_retried_in_place() {`

A retry of a generation that is not retained-idle is refused before it
takes a reservation or verifies anything.

## `static SCRATCH: AtomicU32 = AtomicU32::new(0);`

=======================================================================
The kill tests

`Injection::Kill` is `std::process::abort()` — a real process death,
chosen so the claim is *what a coordinator that runs no cleanup leaves
on disk*. An early `return` would unwind and prove something weaker.
=======================================================================

## `fn scratch(label: &str) -> ScratchTree {`

A scratch tree for one kill test: [`scratch_tree::acquire`]'s exclusive,
ULID-named root, reclaimed when the guard drops.

Through the run directory's own test allocator because this module may
not name `std::fs`, and through *that* allocator because of what the
fixture it replaces did. It named the root by `std::process::id()` and a
counter, created it with `create_dir_all`, and never removed it. A
process id is unique only among live processes; Windows reissues one
within hours, and the `test (winguest)` image carried one leftover pair
per harness that had ever run on it. A harness that drew a leftover's id
was handed the dead harness's directory, its kill child appended a
second run to the log the dead child had left, and `replay` refused the
second `RunStarted`: both kill tests red together, at their `the log
replays` expectations, with every earlier assertion passing.

The root is created with one exclusive `create_dir`: a name occupied at
acquisition is refused. The guard retains the original directory handle;
the parent checks its identity before launching the child and reading the
residue, and reclaim checks independently before removal. A replacement
after one of those observations remains a check-to-use interval.
The name is a tag and a deterministic ULID. A dead harness's name can
recur if the clock, pid and nonce repeat, and knowledge of those inputs
permits computing names ahead of allocation. [`scratch_with`] draws again
on `Occupied`, up to [`SCRATCH_DRAWS`] times, and never on
`Undecidable`, which is not a collision.

The guard reclaims the tree on return and on unwind: the **child** still
dies without cleanup, which is the claim the kill tests make, and the
**parent** removes what it left once the assertions have read it. What
this family leaves behind is an aborted parent's tree, which ran no
`Drop`, or a tree whose removal the filesystem refused — a panic on the
normal path, a report while unwinding, and the tree stays either way.

## `struct KillAtPhase {`

A [`RunDirHooks`] that records into the shared harness **and** answers
`Kill` at one `(site, phase)`.

`HookHarness::arm` takes a `SubEffectPoint`, and
`RunDir.WriteQuestionPayload` exposes none — so arming its `Before`
phase needs a local double. The *recording* still goes to the shared
harness, or this site would contribute nothing to the coverage evidence.

## `fn append(log: &mut EventLog, hooks: &mut HarnessTopologyHooks, event: &TopologyEvent) {`

Append `event` through the real funnel.

## `fn settlement_kill_child() {`

The child of both kill tests: build a run whose settlement is durable,
then die at the boundary the site names.

## `fn settlement_kill_child()` › `let mut request = finished(ALEPH, 0, 1, Next::RetrySameRung { resume: true });`

T-APPEND (s): the line is synced and the process dies. The
settlement is durable and nothing after it was ever attempted.

## `fn settlement_kill_child()` › `let mut request = finished(ALEPH, 0, 1, Next::AskHuman(QuestionKind::Unblock));`

T-FAILED's boundary: the settlement is appended and the question
file is not applied.

## `fn spawn_kill_child(dir: &Path, site: &str) -> ProcessOutput {`

Spawn [`settlement_kill_child`] through the host Runner and wait for it
to die.

Through the Runner rather than `std::process::Command`, which this
module may not name: `Process.Spawn` is the funnel that owns process
start, and a test that reached around it would be the exact bypass the
denylist exists to prevent.

## `fn spawn_kill_child(dir: &Path, site: &str) -> ProcessOutput` › `assert!(`

The `unreachable!` is what fails this test if the injection silently
stopped killing — and it only fails it if the parent looks. A panic
and an abort both exit non-zero, so the exit code alone cannot tell
"the process died at the injection" from "the process ran past it
and panicked one line later".

## `fn committed(dir: &Path) -> Vec<TopologyEvent> {`

Every committed event of the log the child left behind.

## `fn kill_after_failed_settlement_rematerializes_question() {`

`kill_after_failed_settlement_rematerializes_question`.

## `fn kill_after_failed_settlement_rematerializes_question()` › `assert_eq!(rematerialize_question(data), Some(&question_for(ALEPH)));`

Rematerialized from the event, byte for byte, and never re-decided.

## `fn kill_after_failed_settlement_rematerializes_question()` › `let fold = TopologyFold::replay(inputs(), &events).expect("the log replays");`

And a replay reaches the same open question, which is what makes the
answer the operator already wrote answer *this* question.

## `fn retained_generation_not_continued_after_kill() {`

`retained_generation_not_continued_after_kill`.

## `fn retained_generation_not_continued_after_kill()` › `let mut fold = TopologyFold::replay(inputs(), &events).expect("the log replays");`

The fresh process: recovery step (e) closes it, and only then does
the run resume. After that, nothing can retry it.

## `fn a_resume_that_moved_the_runner_is_refused() {`

The container runner policy is a value this fixture never uses, and
this is what says so: `run_started`'s runner is `host-v1`, so a resume
carrying the container record is refused rather than folded.

## `type Acquire = fn(&Path, &str) -> Result<ScratchTree, ScratchAcquireRefusal>;`

How a scratch tree is acquired: [`scratch_tree::acquire`] in every test,
and a double in the witnesses of [`scratch_with`]'s own policy.

## `const SCRATCH_DRAWS: u32 = 3;`

How many names [`scratch_with`] draws before it gives up.

Each occupied name is preserved and the fixture asks for another. The
bound prevents an exhausted or deliberately preoccupied namespace from
keeping a test alive indefinitely; it does not promise eventual success.

## `fn scratch_with(acquire: Acquire, label: &str) -> ScratchTree {`

[`scratch`] over an injectable acquisition, which is what lets the
policy below be witnessed without arranging a real collision: the double
supplies the refusal, and the assertion is about what this function does
with it. The real refusal is the allocator's, witnessed where it lives
and by the launcher reproduction PR #149 records.

## `fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {`

The payload of a caught panic, as a string.

## `static REFUSED_ONCE: AtomicBool = AtomicBool::new(false);`

Whether [`refuse_once_then_acquire`] has refused yet.

## `static OCCUPIED_CALLS: AtomicU32 = AtomicU32::new(0);`

How many times [`always_occupied`] has been asked.

## `static UNDECIDABLE_CALLS: AtomicU32 = AtomicU32::new(0);`

How many times [`undecidable`] has been asked.

## `fn refuse_once_then_acquire(`

An acquisition that answers `Occupied` to its first call and is the real
allocator after that.

## `fn always_occupied(parent: &Path, tag: &str) -> Result<ScratchTree, ScratchAcquireRefusal> {`

An acquisition that answers `Occupied` to every call, each time with a
root of its own, so a report that names every refused root can be told
from one that names the last root three times.

## `fn undecidable(parent: &Path, tag: &str) -> Result<ScratchTree, ScratchAcquireRefusal> {`

An acquisition whose answer is not a collision, counting its calls.

## `fn an_occupied_draw_is_drawn_again() {`

An occupied name is drawn again, and the second draw is a live tree.

## `fn the_draws_are_bounded_and_every_refused_root_is_named() {`

The draws are bounded — exactly [`SCRATCH_DRAWS`] acquisitions, counted
at the double — and the refusal past the bound names each distinct root
it met, so a collision that is not a coincidence reads as what it is.

## `fn an_undecidable_refusal_is_not_drawn_again() {`

`Undecidable` is not a collision and is not drawn again: the double is
asked exactly once.

## `fn a_kill_tests_scratch_tree_is_reclaimed_when_its_guard_drops() {`

The fixture hands back the guard, and the guard reclaims: nothing is at
the tree's path once it drops. The kill tests hold it through their
assertions, so their residue is read and then removed. Absence is
[`scratch_tree::proves_absent`]'s `NotFound`, not `Path::exists`'s
`false`, which a stat the filesystem refused to answer would also give.

## `kill_after_failed_settlement_rematerializes_question` › `assert!(`

Absence proved, not assumed: `Path::exists` answers `false` to a stat
the filesystem refused as well as to `NotFound`.
