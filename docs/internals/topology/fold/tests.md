# `src/topology/fold/tests.rs`

Extended notes for [`src/topology/fold/tests.rs`](../../../../src/topology/fold/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `type BreakRunner = fn(&mut RunnerPolicy);`

One way to damage an otherwise valid record, for the refusal tables.

## `type ForgeCandidate = fn(&mut MergePrepared);`

One coordinate of an embedded candidate identity, forged.

## `type AddResidue = fn(&mut RunState);`

One residue a Complete run refuses to leave behind.

## `const BETA: TaskKey = TaskKey(3);`

The fourth task of [`wide_plan`] only. `plan()` and `chain_plan()` have
three, and `region` already answers `src/repairs` for this key.

## `fn sha(label: &str) -> CommitSha {`

-----------------------------------------------------------------------
Fixtures

Every independently meaningful field varies independently. Nothing sits
at a default, no two fields that could be read for one another hold the
same value, and every list that has an order is written in one that is
neither sorted nor reversed. Where a value could be confused with
another of its type — a commit sha with a tree sha, a task's floor with
its ceiling, one epoch with another — the two are different literals.
-----------------------------------------------------------------------

## `fn sha(label: &str) -> CommitSha {`

A distinct 40-character hex-shaped sha per label.

Distinct per role rather than per value: a base, a parent, a tree, a
commit and a head are five different claims, and a fixture that let any
two of them share a literal would pass under a relation that compared
the wrong pair.

## `fn probed_agents() -> Vec<String> {`

The agents this run's pre-flight probed: padded, mixed case, multi-byte
and over-length, in an order that is neither sorted nor reversed, and
deliberately a superset of the agents the ladders bind.

## `fn plan() -> Plan {`

Plan order, display-id order and topological order all disagree, and the
three tasks touch three disjoint regions so that a lease check has
something to be wrong about in both directions.

## `fn chain(task: &str) -> ChainSummary {`

A ladder that belongs to one task and to no other: different length,
different attempts allowance, and every rung's agent, model and pin
derived from the task's own id.

## `fn effort_policy() -> ResolvedEffortPolicy {`

Four distinct efforts, so a rung bound at the wrong tier's effort is a
different value rather than the same one.

## `fn chain_plan() -> Plan {`

A three-task chain whose dependencies all refer *forward* in key
order: `aay`(0) depends on `bee`(1), which depends on `cee`(2).

Keys are assigned in plan order (`keys_by_display_id`), and plan order
is not topological order, so this shape is an ordinary plan rather than
a contrived one. It is the shape the derived-`Blocked` predicate has to
be right about: `aay`'s only failure is two hops away, and a derivation
that decided each task from what it had decided so far would reach
`aay` before it had decided `bee`.

## `fn chain_run_started_event() -> TopologyEvent {`

The chain plan's `run_started`, authenticated against its own registry.

## `fn wide_plan() -> Plan {`

Four tasks that wait on nothing and touch four disjoint regions.

`plan()` and `chain_plan()` are both chains, and in a chain at most one
of `ready`, `ready_retry` and `integration_admissible` can hold at a
time: everything waits on one task, and that task is pending, open, or
merged. A predicate that is never independently true is one no guard
over it can be measured against — which is how four of the five poison
guards came to be asserted by a test that would have passed without
them. The three original ids keep their kinds, tiers, hints and
ladders; only `depends_on` differs, and `beta` is the fourth holder a
held pipeline entitlement needs.

## `fn wide_run_started_event(max_parallel: u32) -> TopologyEvent {`

The wide plan's `run_started`, authenticated against its own registry,
at a stated pipeline width.

The width is a parameter because it is the one limit selection reads,
and because `DEFAULT_MAX_PARALLEL` is 1: a fixture fixed at 3 tests a
width `config` refuses to create a run at.

## `fn wide_started(max_parallel: u32) -> TopologyFold {`

A fold over [`wide_plan`] that has recorded its `run_started`.

## `fn run_started_unauthenticated() -> RunStarted4 {`

The run record with a digest field nothing has filled in yet, so that
the digest can be derived from it without deriving it from itself.

## `fn run_started_unauthenticated() -> RunStarted4` › `limits: TopologyLimits {`

Three different numbers: a fold that read one limit where it
meant another lands on a value this fixture does not hold.

## `fn started() -> TopologyFold {`

A fold that has recorded its `run_started` and nothing else.

## `fn attempt_finished_mut(event: &mut TopologyEvent) -> &mut AttemptFinished4 {`

The body of an `attempt_finished` a fixture has just built, for the tables that
damage one coordinate of an otherwise valid event.

These two accessors exist because §7 denies `unreachable!` in tests as well as in
production, with no Clippy allowance to take, and the eleven sites that read
`let ... else { unreachable!("built as an attempt_finished") }` were that
construct. A test fails its own setup with a message naming the failed premise
instead, which is what the panic here does, and `#[track_caller]` reports the
fixture line rather than this one. The premise is live rather than decorative:
handing either accessor an event of another kind panics with the kind it was
given.

## `fn candidate_prepared_mut(event: &mut TopologyEvent) -> &mut CandidatePrepared {`

The same accessor for a `candidate_prepared`.

## `fn review_pass(pass: &str, outcome: ReviewPassOutcome) -> ReviewRecord {`

--- event builders ----------------------------------------------------

## `fn review_pass(pass: &str, outcome: ReviewPassOutcome) -> ReviewRecord {`

One review pass's ledger line, named and concluded.

## `fn attempt_record_for(key: TaskKey, attempt: u32) -> AttemptRecord {`

The complete successful attempt **for this task under the frozen plan**.

`TaskKey` is the plan index, so whether a second opinion is configured is
derived from `review_plan` rather than asserted by the fixture: the
premise carries exactly the passes §11.2 requires of that task, and no
others. `review_plan` configures one for index 2 alone.

## `fn attempt_record_for(key: TaskKey, attempt: u32) -> Attemp…` › `let plan = review_plan(key.0 as usize + 1);`

Long enough to include this task's own slot; `review_plan` decides
each index by the same closure the real fixtures use, so slot `key.0`
holds exactly what the frozen plan gives that task.

## `fn attempt_record(attempt: u32) -> AttemptRecord {`

A **complete** successful attempt for a task the plan gives no second
opinion.

The primary pass is present and `Passed`. This carried a lone
`second-opinion` entry and no primary at all — a record that satisfies
`is_successful` only because `all` over its passes never sees the pass
§11.2 actually requires. A positive premise that passes vacuously
witnesses nothing about the clause it is meant to exercise: delete the
review half of `is_successful` and no positive test here would notice.

## `fn region(key: TaskKey) -> PathSet {`

The region a task's candidate touches. Disjoint per task, so an overlap
in a test is one the test put there.

## `fn dispatch(key: TaskKey, generation: u32, base: &CommitSha) -> TopologyEvent {`

An ordinary dispatch of a task **of the default [`plan`]**, taking the
region that plan's frozen hints derive.

`region` is keyed by [`TaskKey`] and the default plan is the only plan
those keys belong to, so a fixture on another plan — [`chain_plan`] is
the one — takes [`dispatch_in`] instead, which asks the fold. The
agreement between this table and the derivation is not assumed:
[`the_dispatch_fixture_records_the_region_the_fold_derives`] round-trips
all three.

## `fn dispatch_in(`

[`dispatch`], with the predicted region taken from `fold` rather than
from the default plan's table.

What a conforming driver does — `TopologyRun` reads
[`TopologyFold::predicted_region`] — and what any fixture on a plan
other than [`plan`] needs, because the keys are dense per plan and
`region`'s table is the default plan's.

## `fn frozen_binding(fold: &TopologyFold, key: TaskKey, rung: usize) -> RungBinding {`

Delegates to the production reader rather than repeating its
composition.

It used to repeat it, and that made this file hold **two** derivations
of one value — the validator's, in `check_attempt_started`, and this
one. Every test that builds an `attempt_started` goes through here, so
routing it to [`TopologyFold::frozen_rung_binding`] puts that reader
under the whole existing attempt corpus: if it ever disagrees with the
validator beside it, dozens of tests fail rather than none.

## `fn the_frozen_rung_binding_is_what_the_validator_accepts() {`

The reader's answer is exactly what the validator accepts.

**Round-tripped against `check_attempt_started`, not compared to a
literal.** A literal expectation would be a second transcription of the
same rule, and would agree with this reader for the same reason the
reader is right or wrong — the self-oracle shape. Feeding the reader's
output to the validator asks the only question that matters: do the two
halves of this file agree.

The negative half is what gives it teeth. Perturbing one field of the
binding must be refused, or the validator is not checking the thing the
reader produces and the positive half proves nothing.

**The table varies every field of `RungBinding`, and did not always.** It ran
`model`, `agent` and `effort` and left `tier` and `pinned` alone, so a
comparison that dropped either of those two would have passed here.
`matches_frozen` is whole-struct equality against `from_frozen`, so all five are
compared today; that is a property of one expression, and this table is what
holds it. The `assert_ne!` above the refusal is the guard the older arms did
without: a perturbation that perturbs nothing would satisfy the loop by leaving
the accepted binding in place.

## `fn attempt_started_resuming(`

[`attempt_started`], resuming `session` — the same-generation retry a
`Retained` settlement exists to admit.

## `fn settle(`

`attempt_finished`, whose record **says the attempt failed**.

Every settlement this can build is a failure — `candidate_prepared` is the
sole successful one — so the record is derived to match rather than left
as the "worker ran and its work was accepted" shape. Built the other way,
each caller produced a settlement that fails a task while carrying a
ledger line saying the work passed, which `check_attempt_finished` has
refused since 2026-08-27.

## `if let AttemptSettlement::Retained {`

**One session, in both places the event carries it.** Production
takes the settlement's `retained_session` and the record's
`session_id` from one value — `assessed.outcome.session_id` — and the
fold refuses a retained settlement whose two halves disagree about
which conversation was left open. A builder that left the record's
stock id in place would be constructing that disagreement in every
retained fixture in this file.

## `fn settle_failing(`

[`settle`], with a failure on the record.

The allowance is decided from `AttemptRecord.failure`, so a settlement
built without one is the "worker ran and its work was accepted" cell and
cannot exercise any other.

## `fn candidate_prepared_at(`

A `candidate_prepared` naming the attempt that produced it.

ST-06 binds the embedded record to the generation's current successful
attempt, so a fixture whose generation retried has to say so: after one
retry the candidate belongs to attempt 2, and a builder that hard-coded
1 would be asserting the very mismatch the fold refuses.

## `fn selection_accessors_report_an_unstarted_run_as_holding_and_offering_nothing() {`

--- selection accessors -----------------------------------------------

## `fn selection_accessors_report_an_unstarted_run_as_holding_and_offering_nothing() {`

The accessors answer for an unstarted run, and answer it as a statement
rather than as an `Option` the caller must decide what to do with.

## `fn ready_names_only_the_task_whose_dependencies_are_merged() {`

`ready` is the fold's predicate, not a constant: exactly the task whose
dependencies are met is ready, and the two that depend on it are not.

## `fn pipeline_held_tracks_the_generation_classes_that_hold_the_entitlement() {`

`pipeline_held` counts what the packet says holds the entitlement, and
the count moves with the generation class.

This is the accessor a caller would otherwise re-derive by walking
`GenerationClass` itself, so the assertion is that the accessor agrees
with the classes actually present — not merely that it returns a number.

## `fn pipeline_held_tracks_the_generation_classes_that_hold_th…` › `assert!(fold.pipeline_reservable());`

max_parallel is 3 in this fixture, so one held entitlement leaves
room — the reservable predicate is a comparison, not a boolean flag.

## `fn a_retained_generation_holds_no_pipeline_entitlement_and_is_ready_to_retry() {`

A settlement to `RetainedIdle` releases the pipeline entitlement while
keeping the generation open — the one class whose two properties differ.

## `fn retain(key: TaskKey, attempt: u32, session: &str, incarnation: Epoch) -> TopologyEvent {`

--- the Retained arm asks what the Closed arm asks ---------------------

`PR7-G2-W1-RETAINED-ARM-UNGUARDED` (§2, §22e). Round 6's four new
settlement refusals all construct `Closed`, which is why this arm was
undriven: it checked the epoch and stopped.

## `fn retain(key: TaskKey, attempt: u32, session: &str, incarnation: Epoch) -> TopologyEvent {`

A `Retained` settlement of `key`'s first attempt, session and all.

## `fn a_retained_settlement_binds_its_envelope_to_its_record() {`

**The Retained arm asks the same questions the Closed arm asks.**

A settlement carries an envelope and a ledger line, and this arm bound
them to each other in one field — the incarnation — and left the rest
free. So a current-epoch retained settlement could carry a record
belonging to a different attempt of the same generation, and could name
a conversation the ledger line does not report.

The positive premise is asserted first in every row, so each refusal is
about the one field the row moves.

## `fn a_retained_settlement_binds_its_envelope_to_its_record()` › `accepts(&fold, &retain(ZETA, 1, session, Epoch(0)));`

The premise: the coherent settlement applies.

## `fn a_retained_settlement_binds_its_envelope_to_its_record()` › `let mut wrong_attempt = retain(ZETA, 1, session, Epoch(0));`

The record's attempt is not the envelope's.

## `fn a_retained_settlement_binds_its_envelope_to_its_record()` › `let mut wrong_session = retain(ZETA, 1, session, Epoch(0));`

The record names another conversation.

## `fn a_retained_settlement_binds_its_envelope_to_its_record()` › `let mut sessionless = retain(ZETA, 1, session, Epoch(0));`

And names none at all, which is the shape the scaffold emitted.

## `fn a_retained_settlement_binds_its_envelope_to_its_record()` › `let mut wrong_generation = retain(ZETA, 1, session, Epoch(0));`

The envelope's generation is not the open one.

## `fn a_retained_settlement_binds_its_envelope_to_its_record()` › `assert!(`

The incarnation is not this run's.

## `fn a_retained_settlement_binds_its_envelope_to_its_record()` › `let generation = fold`

Nothing moved on any of them: the generation is still in flight.

## `type SettlementArm = (&'static str, fn() -> AttemptSettlement);`

One arm of the settlement door: a label and a builder for the
settlement that reaches it.

## `fn no_attempt_finished_arm_accepts_a_record_that_claims_success() {`

**No settlement of `attempt_finished` accepts a record that claims the
attempt succeeded — on either arm.**

The sibling-arm witness. `candidate_prepared` is the sole successful
settlement (INV-07,
`decisions/2026-08-12-merge-queue-execution-topology.md`), and the
`Closed` arm has enforced that against the record since round 6. The
`Retained` arm did not, so the invariant held on one path through the
door and not the other: a retained settlement could carry a record with
no failure and every configured pass green — a record
`check_candidate_prepared` would itself accept — and the ledger line an
operator reads would say the work passed while the fold held the
generation open for a retry.

**What "retained" means, and why requiring this is not requiring a
terminal failure.** `settle_failed` is the only producer of a `Retained`
settlement: it is reached on the failure path, for a same-rung retry
that has a session to resume. So a retained attempt has *not* succeeded,
by construction. `is_successful()` being false is the record saying that
much and no more — it does not require a `Failed` transition, which
`Retained` has no field for, and it does not make the generation
terminal.

Driven over the identical record on both arms, because the claim is that
the two agree rather than that each refuses something.

## `fn no_attempt_finished_arm_accepts_a_record_that_claims_suc…` › `let claims_success = |event: &mut TopologyEvent| {`

The one shape both arms must refuse: no failure, and the frozen
obligation all green — which is exactly what `candidate_prepared`
requires of the settlement that *is* a success.

## `fn no_attempt_finished_arm_accepts_a_record_that_claims_suc…` › `accepts(&fold, &settle(ZETA, 0, 1, settlement()));`

The premise: with a record that does not claim success, this
exact settlement applies — so the refusal below is about the
claim and nothing else.

## `fn no_attempt_finished_arm_accepts_a_record_that_claims_suc…` › `let generation = fold`

Nothing moved.

## `fn no_attempt_finished_arm_accepts_a_record_that_claims_suc…` › `let mut judged = settle(ZETA, 0, 1, settlement());`

**And the predicate is the shared one, not half of it.** A record
whose failure field is empty and whose configured pass came back
`Failed` makes no success claim — §11.2's "every configured pass
passes" is the other half of `is_successful`, and it is the half
an arm re-deriving the question from `failure.is_none()` would
lose. Both arms take this record.

## `fn no_attempt_finished_arm_accepts_a_record_that_claims_suc…` › `let mut fold = started();`

And the door that *does* take a successful record still takes it, so
this narrows `attempt_finished` rather than closing success off
altogether.

## `fn a_retained_settlement_releases_the_pipeline_and_nothing_else() {`

**What a Retained settlement does to the run: pipeline released, lease
retained, generation open, task not terminal — and once.**

The row's other half. "Releases only the pipeline entitlement" is a
claim about the state after the arm applies, and the negatives are
double release (a second settlement of a generation that is no longer in
flight) and a new-process retry (a resume by an incarnation that did not
retain the session). Both are refusals the fold already makes and
neither was driven from this arm.

## `fn a_retained_settlement_releases_the_pipeline_and_nothing_…` › `assert_eq!(fold.pipeline_held(), 0, "the entitlement was not released");`

Released, exactly once, and nothing else went with it.

## `fn a_retained_settlement_releases_the_pipeline_and_nothing_…` › `assert!(matches!(`

Double release: the generation is no longer in flight, so a second
settlement of it — retained or closed — is refused.

## `fn a_retained_settlement_releases_the_pipeline_and_nothing_…` › `let retry = attempt_started_resuming(&fold, ZETA, 0, 2, 0, session);`

Same-generation retry: accepted in the retaining incarnation, and
only there. The second half is the new-process refusal.

## `fn a_retained_settlement_releases_the_pipeline_and_nothing_…` › `assert!(matches!(`

And a retry naming some other conversation is refused in the
retaining incarnation too.

## `fn a_poisoned_fold_authorises_nothing_while_still_reporting_what_it_holds() {`

A poisoned fold authorises nothing.

INV-20: "no completion is applied after the fold is poisoned by a
returned append error". `plan_transition` already refuses; a predicate
that kept answering `true` would let the coordinator select work from a
state this process can no longer vouch for.

## `fn a_poisoned_fold_authorises_nothing_while_still_reporting…` › `let mut fold = wide_started(3);`

Every one of the five predicates is **independently true** before
the poison, which is the whole of what makes the five assertions
after it load-bearing: `alpha` waits on nothing and holds no
generation, `zeta` holds a generation this incarnation retained,
`mid`'s candidate is queued and eligible, and `beta` holds one of the
three pipeline entitlements. This test used to poison a fold in
which `alpha` had just been dispatched and the other two waited on
it — nothing was admissible even unpoisoned, and four of the five
guards could be deleted without it going red.

## `fn a_poisoned_fold_authorises_nothing_while_still_reporting…` › `assert_eq!(`

Accounting, not authorisation: answering `0` here would be a false
statement about the run rather than a refusal. The rule that keeps a
report from being derived from this is the append-error protocol's,
and it belongs in the emit path.

## `fn an_integration_is_inadmissible_while_the_pipeline_entitlement_is_held() {`

The pipeline entitlement is a clause of `integration_admissible`, and
at the width production actually runs it is the binding one.

`permits.pipeline` counts an unresolved integration transaction among
the held, `permits.provisional_reservations` gives integration
selection `{pipeline, merge}`, and `deadlock_freedom` takes a
reservation "only when the derived count permits". So an integration is
admissible only within `max_parallel`, exactly as a dispatch and a
retry are.

At width 1 — `DEFAULT_MAX_PARALLEL`, and the only width `config`
accepts for a fresh run — this is reachable rather than theoretical: a
crash after `task_dispatched` and before `attempt_started` leaves an
`OpenNoAttempt` generation holding the single slot, and the resumed
loop's first selection is where an admissibility that ignored the count
would spend it twice.

## `fn an_integration_is_inadmissible_while_the_pipeline_entitl…` › `apply(&mut narrow, &dispatch(ZETA, 0, &sha("base")));`

`zeta` takes the only slot, and stops where a crash between the
dispatch and the first attempt stops it.

## `fn an_integration_is_inadmissible_while_the_pipeline_entitl…` › `let mut wider = wide_started(2);`

One slot wider, the identical state admits it: the clause under
test is the count and nothing else about this fixture.

## `fn a_ladder_position_is_derived_by_replay_and_not_assumed() {`

**A task's ladder position survives the process that wrote it.**

The companion to the deferral witness, and the same disease. A
settlement that escalates closes the generation and leaves the task
`Pending` — so the ready-dispatch branch selects it again, and the rung
it runs at is a fact only the log holds.

A driver that assumed rung 0 would dispatch an escalated task on rung 0
forever, never reaching the tier its chain escalated it to. A driver
that assumed attempt 1 would hand `next_step` the first attempt of the
allowance every time, so the task would retry forever and never
escalate at all. Both were true of `TopologyRun` until this field
existed, and neither was visible as a wrong number — only as a run that
behaves differently after a restart.

## `fn a_ladder_position_is_derived_by_replay_and_not_assumed()` › `for attempt in 1..=2u32 {`

Rung 0, two attempts, allowance spent -> escalate onto rung 1.
Zeta rather than alpha since PR #180: alpha's fixture ladder has one
rung, and the fold now refuses an escalation onto a rung the frozen
ladder lacks.

## `fn a_ladder_position_is_derived_by_replay_and_not_assumed()` › `let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");`

Through the wire, because a resume reads bytes.

## `fn a_ladder_position_is_derived_by_replay_and_not_assumed()` › `assert_ne!(0, after.rung, "a process-local rung tally reads zero here");`

The two assumptions this replaces, shown wrong. A fresh process
starts both at zero and agrees with the fold on every reading until
a resume — which is exactly when nothing is watching.

## `fn an_attempt_runs_at_the_replay_derived_rung_and_an_escalation_climbs_exactly_one() {`

**An attempt runs at the rung replay says the task stands on, and an
escalation moves that position by exactly one rung.** PR #180's
second review (`PR180-REVIEW2-001`): `check_attempt_started` indexed
the frozen ladder by the event's own rung and never compared it with
`TaskFold.rung`, so a rung-0 start with rung 0's valid binding was
accepted on a task already escalated to rung 1, and `apply_settlement`
took whatever rung an `Escalated` settlement carried.

The walk: zeta (three rungs) spends rung 0 and escalates onto rung 1;
a start below (rung 0) and above (rung 2) the position is refused as
`WrongRung` with the ladder position untouched, the start at the
position applies; from rung 1 an escalation backward (0), sideways (1)
or over a rung (3) is refused and the one onto rung 2 applies; from the
top rung an escalation onto rung 3 is refused, because the human is
the top rung, and a retry there is charged to rung 2's allowance. Then
through the wire: the replayed fold holds the same position and refuses
and accepts the same starts as the live one.

## `const CHARGE_ALLOWANCE: fn(&mut RunState, TaskKey, &AttemptRecord) = RunState::charge_all…`

`RunState::charge_allowance`, as a value.

`runner::tests::the_rungs_allowance_is_counted_in_one_production_place`
carries a `SPELLINGS` fixture listing the ways this call can be written.
That fixture is a `&str`, so rustc never reads it and a path in it can name
nothing at all — it named `TaskFold::charge_allowance` for a round. This is
the same path where the compiler does read it.

## `fn an_interrupted_attempt_refunds_the_rungs_allowance() {`

**An interrupted attempt does not spend the rung's allowance.**

`transaction_fault_matrix[T-ATTEMPT]`'s `resume_action` in its own words:
append `attempt_interrupted` *"(unknown spend, **allowance refunded**…)"*.
`ladder::spends_allowance` agrees from the other direction —
`FailureKind::Interrupted` is `false`, because "the engine died between
an attempt starting and finishing, so nothing judged the code".

**This fold disagreed with both for the whole of PR7.** It counted every
`attempt_started`, so an interruption, a park and an outage each burned a
rung the packet says they do not — and the divergence was invisible
because the count is only ever read across a resume. Found by S5 round 2
(`emit` and `settle`, independently).

The pair is asserted, not just the repair: a **judged** rejection spends,
an **interruption** does not, and the difference is the only thing that
changed between the two halves. A fold that stopped counting altogether
would satisfy half of this and fail the other.

### The behavioural half of the `runner` census

`runner::tests::the_rungs_allowance_is_counted_in_one_production_place`
counts the *spelling* `charge_allowance(` in each applier's body, and a
count over text cannot enforce a property about calls: an alias and a
closure of the same name leave its per-applier map and its subtree total
both reading exactly what they read today while a whole settlement arm
stops charging. That was measured at `823ad36`, against the whole suite
and not only the census.

This test reads `attempts_on_rung` off the state instead, so a spelling is
invisible to it by construction, and it drives **every settlement the
vocabulary has** rather than the one arm the repair was written on — an
escape that skips a single arm is the shape this class arrives in.
`apply_candidate_prepared`, the successful settlement, is the sibling half
and is driven by
[`a_successful_attempt_charges_its_rung_live_and_on_replay`].

**`Escalated` is excluded, and that is not a gap.** The arm resets
`attempts_on_rung` to zero on the rung it climbs onto, *after* the charge,
so the charge has no observable effect there — not by this test and not by
anything else. There is nothing for an escape to gain by skipping it.

## `fn an_interrupted_attempt_refunds_the_rungs_allowance()` › `let mut spent = started();`

Half one: a judged rejection. The worker ran and produced work to
judge, so it spends — this is the cell that keeps the count honest.

## `fn an_interrupted_attempt_refunds_the_rungs_allowance()` › `let mut refunded = started();`

Half two: the same shape, interrupted. Same dispatch, same start, and
the settlement is the only difference.

## `fn an_interrupted_attempt_refunds_the_rungs_allowance()` › `let label_of = |settlement: &AttemptSettlement| -> &'static str {`

**The same pair on every settlement `apply_settlement` can be handed.**
The two halves above both settle `Closed`/`Retry`, so an applier that
charged on that arm and nowhere else satisfied them — and that is
precisely the escape the lexical census cannot see:

    let real_charge = Self::charge_allowance;
    let charge_allowance = |state: &mut Self| {
        if !matches!(&finished.settlement, AttemptSettlement::Retained { .. }) {
            real_charge(state, finished.key, &finished.record);
        }
    };
    charge_allowance(self);

With `attempts_per = 2` a retained failure then never persists its
spend, the next rejection derives `0 + 1 < 2`, and the run retries the
rung it should have escalated off — indefinitely, while every count in
`runner`'s census still reads what it reads today.

**The label is derived by an exhaustive match, not written beside each
arm.** A hand-written list of arms with hand-written names is how an arm
nobody thought to charge arrives: it is missing, and nothing says so.
`label_of` matches every shape the wire vocabulary has, so a variant
added to `AttemptSettlement` or `SettlementTransition` stops the build
here, and the coverage assertion below is over names this match produced
rather than over names this test asserted about itself.

## `fn an_interrupted_attempt_refunds_the_rungs_allowance()` › `let mut driven: Vec<&str> = arms.iter().map(&label_of).collect();`

The two the vocabulary has and this test does not drive, named rather
than absent. `closed/succeeded` is refused by `check_attempt_finished`
before `apply` is reached — `candidate_prepared` is the sole successful
settlement — and `closed/escalated` resets the count to zero on the rung
it climbs onto, *after* the charge, so the charge has no observable
effect there for an escape to gain.

## `fn an_interrupted_attempt_refunds_the_rungs_allowance()` › `for (kind, spent) in [`

The judged/interrupted pair, so an arm that stopped charging
altogether and an arm that charges everything are told apart by the
same two cells the halves above use.

## `fn an_interrupted_attempt_refunds_the_rungs_allowance()` › `if let TopologyEventBody::AttemptFinished { data } = &mut event.body {`

One session in both places the event carries it, as `settle`
does: the fold refuses a retained settlement whose two halves
disagree about which conversation was left open.

## `fn an_interrupted_attempt_refunds_the_rungs_allowance()` › `let mut direct = started();`

**The compiled half of that census's `SPELLINGS` fixture.** The fixture
is a string the census counts over, so it named
`TaskFold::charge_allowance` — an item that does not exist, the method
being defined on `RunState` — for a whole round with nothing able to
report it. [`CHARGE_ALLOWANCE`] is the same path as a value: a rename or
a move to another type stops the build here rather than silently
emptying a control there. Called, not merely bound, so the item it names
is shown to be the one that moves the count.

## `fn a_deferral_count_is_derived_by_replay_and_not_by_a_process_local_tally() {`

**A deferral count survives the process that wrote it, and a
driver-side tally does not.**

The witness for why this count is the fold's. `ladder::next_step` reads
it on exactly one branch — an outage defers while `defers < max_defers`
and parks at it — so a run that has already spent its allowance must
park rather than defer again.

A driver keeping its own tally is correct for as long as its process
lives. This test is the case where that stops being true: the log holds
three deferrals, the process dies, and the next one replays. The fold
reaches three. A fresh in-memory counter reaches **zero**, and with
`max_defers = 3` the run would defer a fourth time, and a fifth, and
never park — the allowance silently becoming unbounded across a resume.

That is `predicted_region`'s shape with a resume-shaped fuse: two
derivations of one number, agreeing until the moment they do not.

## `fn a_deferral_count_is_derived_by_replay_and_not_by_a_proce…` › `for round in 1..=3u32 {`

Three deferrals of one task, each one a fresh generation the way a
`defer_wait_elapsed` wake produces.

## `fn a_deferral_count_is_derived_by_replay_and_not_by_a_proce…` › `let woken = ev(TopologyEventBody::DeferWaitElapsed {`

`Deferred -> Pending via defer_wait_elapsed`, which is the
transition the contract names and the only way back to a
dispatchable state. The fold refuses a re-dispatch without it.

## `fn a_deferral_count_is_derived_by_replay_and_not_by_a_proce…` › `let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");`

Through the wire, because a resume reads bytes and not values.

## `fn a_deferral_count_is_derived_by_replay_and_not_by_a_proce…` › `let process_local_tally: u32 = 0;`

The tally the driver is forbidden from keeping, shown failing. A new
process starts one at zero: it agrees with the fold on every reading
until a resume, and this is the reading after one.

## `fn the_statement_accessors_report_the_run_rather_than_authorising_anything() {`

The three statements the selector delegates rather than re-derives.

Statements about the run and not authorisations, which is why poisoning
does not flip them: a poisoned fold of a run with a deferred task still
has one, and `false` there would be a false statement rather than a
refusal. `pipeline_held` is exempted for the same reason and by the
same sentence.

## `fn the_statement_accessors_report_the_run_rather_than_autho…` › `let mut stopped = started();`

`run_is_ending` is the epoch-aware half, which is why a caller must
not read `budget_stop` for itself: a stop of **this** epoch ends the
run, and the resume that raised the ceiling clears it.

## `fn the_statement_accessors_report_the_run_rather_than_autho…` › `let mut halted = started();`

A halt ends it in every epoch, poisoned or not.

## `fn queue_candidate(fold: &mut TopologyFold, key: TaskKey, generation: u32) {`

Take one task to a **queued** candidate: dispatch, attempt, success,
prepare, create.

[`merge_task`] minus its last two events. The generation closes at
`task_candidate_created` and releases the entitlement it held, so a
fold built this way holds a queued candidate and nothing else.

## `fn retained_generation(fold: &mut TopologyFold, key: TaskKey, generation: u32) {`

A generation of `key` retained by the incarnation the fold is in.

The incarnation is read from the fold rather than written as `0`:
`ready_retry` is false in every incarnation but the retaining one, so a
fixture that hard-coded the epoch would silently stop being a
`ready_retry` state the moment it was used after a resume.

## `fn merge_task(fold: &mut TopologyFold, key: TaskKey, generation: u32, sequence: u32) {`

Drive one task from pending to merged over the fast path, at the head
the integration ref is currently at.

## `fn a_topology_log_is_folded_from_its_run_started_and_from_nothing_else() {`

-----------------------------------------------------------------------
The header: what a fold may be started with (refusals 4, 5, and the
ladder validation the fold boundary owns)
-----------------------------------------------------------------------

## `fn a_topology_log_is_folded_from_its_run_started_and_from_n…` › `let fold = TopologyFold::new(inputs());`

Every kind, not a sample: the first line of a topology log records
the registry, the runner and the limits that every later event is
checked against, so there is no event that means anything without it
— including the informational ones, which a poisoned or unstarted
process still may not append.

## `fn a_run_begins_once_and_says_it_is_a_topology_run()` › `for schema in [0, 1, 2, 3, 5, 99] {`

A record that does not claim the topology schema is not one this
fold may interpret, whatever else it says.

## `fn a_run_started_carries_a_runner_record_that_could_be_re_e…` › `let mut runner = container_runner();`

refusals[5], first half, over every defect the record can exhibit —
and, at the top, over the one shape that is *not* a defect: a
container whose runtime reported no manifest digest. INV-23 makes the
digest "the manifest digest when reported", so a record without one
is complete, and a fold that refused it would refuse a legitimate
run on a runtime that reports none.

## `fn a_resume_that_established_a_different_runner_is_refused_…` › `let fold = started();`

refusals[5], second half / INV-23: exact equality, and the refusal
names *which* field moved, because a config edit, a moved tag and a
rebuilt image behind an unchanged tag are indistinguishable as
"runner mismatch" and have completely different fixes.

## `fn a_resume_that_established_a_different_runner_is_refused_…` › `let mut reordered = container_runner();`

And the set is a set: the same volumes enumerated in another order
established the same runner.

## `fn a_resume_is_compared_with_run_started_by_value_and_by_ag…` › `let mut fold = started();`

refusals[5]: a `run_resumed` "whose runner kind, policy, image
reference, image id, image digest, or credential-volume set differs
from run_started(4).runner" is refused. Two things that a
field-by-field fixture leaves unpinned: the credential volumes are a
*map*, so its cardinality and its keys are not its value; and the
record it is compared with is `run_started`'s, not the previous
resume's.

## `fn a_resume_is_compared_with_run_started_by_value_and_by_ag…` › `let renamed = || {`

Same size, same agents, one value moved — and then the values
swapped between the two agents, which keeps the multiset of values
as well.

## `fn a_resume_is_compared_with_run_started_by_value_and_by_ag…` › `apply(&mut fold, &resume(container_runner()));`

The baseline is `run_started`, so an accepted resume does not become
the thing the next one is measured against. Drift A -> A -> B -> A:
B is refused where it stands, and A is still the record afterwards.

## `fn both_recorded_digests_are_checked_against_the_frozen_inp…` › `let moved_plan = ev(TopologyEventBody::RunStarted {`

refusals[4]. Two digests, moved one at a time: a fold that compared
one where it meant the other, or that compared neither, is caught by
whichever case it does not implement.

## `fn both_recorded_digests_are_checked_against_the_frozen_inp…` › `let mut moved = plan();`

The refusal is about the *plan* as much as the record: the same
record against a plan that moved by one field is the same refusal,
which is the case the digest exists for.

## `fn both_recorded_digests_are_checked_against_the_frozen_inp…` › `let probed_elsewhere = ev(TopologyEventBody::RunStarted {`

And the allow-list is one of the inputs it authenticates: a run that
probed something else derives a different registry.

## `fn both_recorded_digests_are_checked_against_the_frozen_inp…` › `let nudge = |value: &str| {`

The comparison is of the whole value. The cases above move a digest
to something unrelated, which a truncated or prefix comparison
rejects just as well; these move the *last* character of each,
independently, so a comparison of anything short of the whole
accepts them. The two digests are pairwise unrelated in this
fixture, so neither can supply the other's expected equality.

## `fn a_malformed_ladder_is_refused_before_it_is_stored()` › `let cases: [(&str, BreakFrozenInputs); 3] = [`

Fold-boundary work, not registry work: the registry derives whatever
the record says, and this decides whether that ladder may enter a
fold's state.

The three cases here are the ones a *frozen plan and run record* can
express — every one of them is a registry the derivation builds
without complaint, which is precisely why the check has to live
here. The rest of the malformations cannot be written into a chain
at all (the derivation recomputes the ceiling, refuses an empty
ladder, refuses a misaligned binding) and are exercised below on the
path where an entry *is* the record: a spawn.

## `fn a_malformed_ladder_is_refused_before_it_is_stored()` › `let spawn_cases: [(&str, BreakLadder); 8] = [`

The same check on the way in through a spawn, over every
malformation an embedded entry can carry.

## `fn a_malformed_ladder_is_refused_before_it_is_stored()` › `let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);`

An empty clipped ladder waiting for a human binding is not malformed
— it is the shape a repair takes when its floor and its root's
ceiling do not intersect — but one that offers nothing to choose
from is.

## `fn run_started_with_ladder(break_it: BreakFrozenInputs) -> (FrozenInputs, TopologyEvent) {`

A `run_started` whose frozen inputs give `zeta` a broken ladder, with
the recorded digest recomputed so the fold reaches the ladder check
rather than stopping at the digest.

## `fn run_started_with_ladder(break_it: BreakFrozenInputs) -> …` › `(`

The fold derives from *its* frozen plan, so the frozen inputs move
with the record: the floor lives in the plan, and a fixture that
moved only the record would be refused for the digest instead.

## `fn repair_spawn(key: TaskKey, root: TaskKey, parent: TaskKey) -> FrozenSpawn {`

-----------------------------------------------------------------------
Registration and dispatch (refusals 10, and what a registered entry is)
-----------------------------------------------------------------------

## `fn repair_spawn(key: TaskKey, root: TaskKey, parent: TaskKey) -> FrozenSpawn {`

A repair entry, complete, as its registering event carries it.

## `fn a_registered_entry_is_the_entry_the_event_registers()` › `let cases: [(&str, BreakSpawn); 9] = [`

Each case moves exactly one thing about an otherwise valid spawn,
and each reports something no other case reports.

## `fn a_registered_entry_is_the_entry_the_event_registers()` › `let mut spawn = repair_spawn(TaskKey(3), ALPHA, ALPHA);`

The dependency-count mismatch is its own case: two lists that
describe one relation have to describe the same one.

## `fn a_spawns_admission_and_its_entrys_admission_are_one_stat…` › `let mut human_required = repair_spawn(TaskKey(3), ALPHA, ALPHA);`

The three legal pairings, and the run's frozen repair limit.

## `fn a_spawns_admission_and_its_entrys_admission_are_one_stat…` › `let mut clipped = repair_spawn(TaskKey(3), ALPHA, ALPHA);`

A binding question whose options are not the entry's.

## `fn a_spawns_admission_and_its_entrys_admission_are_one_stat…` › `let mut runnable_over_clipped = clipped.clone();`

A runnable event over an entry that has no binding, and the reverse.

## `fn a_spawns_admission_and_its_entrys_admission_are_one_stat…` › `let mut unanswerable = clipped;`

And a question nobody could answer parks a task nothing un-parks.

## `fn a_dispatch_opens_one_dense_generation_of_a_pending_task()` › `assert!(matches!(`

A second generation while one is open.

## `fn a_dispatch_opens_one_dense_generation_of_a_pending_task()` › `let start = attempt_started(&fold, ZETA, 0, 1, 0);`

A generation that skips a number, once the first has closed.

## `fn a_dispatch_opens_one_dense_generation_of_a_pending_task()` › `let mut merged_fold = started();`

A task that is not pending.

## `fn a_dispatch_opens_one_dense_generation_of_a_pending_task()` › `assert!(matches!(`

And a task nobody registered.

## `struct HintShape {`

--- the recorded region, derivation-checked --------------------------

`TASK-DISPATCHED-REGION-UNVALIDATED` (§2, §22). The sibling for the
recorded *binding* is `the_frozen_rung_binding_is_what_the_validator_
accepts` above; this is the same question one event earlier, and the
asymmetry between the two is what the row was written about.

## `struct HintShape {`

One hint shape, and the region the contract says it derives.

**Transcribed from the rule, not from the code.** The rule is "the
plan's path hints, taken literally: a hint with no glob metacharacter is
its own literal prefix; anything else — an absent hint list, or a hint
whose literal prefix is empty — classifies repo-wide". Reading
`predicted_region`'s body to build this table would make the grid agree
with the derivation for the reason the derivation is right or wrong,
which is the self-oracle shape `CODING_STANDARDS.md` names.

## `struct HintShape` › `id: &'static str,`

The task's display id, which is also its fixture name.

## `struct HintShape` › `hints: &'static [&'static str],`

What the plan froze.

## `struct HintShape` › `derives: Option<&'static [&'static str]>,`

The prefixes the rule derives, or `None` for repo-wide.

## `const HINT_SHAPES: &[HintShape] = &[`

Every hint shape the rule distinguishes, one axis varied at a time.

The four glob metacharacters get a case each rather than one case with
all four, because a truncation that stopped at only three of them would
pass a combined case on the first one it did handle.

## `HintShape {`

A literal is its own prefix, unchanged.

## `HintShape {`

A trailing separator is not part of the name of the directory.

## `HintShape {`

The four metacharacters, one each. Everything from the first one is
dropped, and the separator that precedes it goes with the trim.

## `HintShape {`

A Windows-shaped hint names Git paths once its separators are.

## `HintShape {`

A doubled separator is **kept**: the rule trims the tail and
substitutes nothing. `src/doubled//inner` and `src/doubled/inner`
name one region to `paths_overlap`, which filters empty components —
and they are still two different literals, which is the whole reason
the comparison below is exact rather than semantic.

## `HintShape {`

Case and non-ASCII survive: the region is the hint's own bytes.

## `HintShape {`

Every hint contributes a prefix, in the frozen order.

## `HintShape {`

A leading glob leaves an empty literal prefix, which is repo-wide —
and repo-wide for **one** hint is repo-wide for the task, because an
unbounded region cannot be narrowed by a bounded sibling.

## `HintShape {`

No hints at all: nothing was said about where the work lands.

## `fn hint_shape_started() -> TopologyFold {`

The hint-shape plan's `run_started`, authenticated against its own
registry — the same construction [`chain_run_started_event`] uses.

## `fn every_hint_shape_derives_the_region_the_rule_states_and_the_door_takes_it() {`

**The derivation is one function of the frozen hints, and the door
accepts exactly its answer.**

Two halves over one table. The first is that the fold's own reader
returns what the *rule* says, independently transcribed above — so a
derivation that quietly changed (a metacharacter dropped from the stop
set, a trim that also collapsed separators) fails here rather than
somewhere downstream of it. The second is that a `task_dispatched`
recording that answer is admitted, which is what makes the refusal in
the sibling test a statement about divergence rather than about the
region being checked at all.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive_is_refused() {`

**A dispatch recording any other region is refused, and the refusal
names both regions.**

The negative half of the table above, and the finding itself:
`check_dispatched` used to match the lease's *shape* alone, so the fold
admitted on `predicted_region`'s answer and `apply_dispatched` granted
whatever the event carried — and the lease table's copy is the one every
later overlap check consults.

Each perturbation is one axis of the way two regions can disagree:
a component missing, one added, the same components in another order,
one component rewritten to something that *overlaps identically*, the
case folded under a run whose `PathPolicy` folds case, a narrowed region
widened to repo-wide, and a repo-wide region narrowed. The last two are
the pair that matters most at width: `RepoWide` overlaps everything, so
recording a narrow region for a repo-wide prediction is how a task that
should have serialized against every other runs beside them.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `(`

The literal hint, taken literally *including* the glob — the
shape the driver actually wrote, and the one `84a3978` repaired
in the driver while leaving the door open.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `(`

A component dropped.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `(`

A component added.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `(`

The same components, sorted. Sorting is a normalisation a caller
could think harmless; the frozen order is the plan's.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `(`

Normalised to a region that overlaps identically. `paths_overlap`
filters empty components, so this collides with the derived one
exactly as the derived one collides with itself — and it is still
not the region the frozen hints derive.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `(`

Case-folded, under a run whose policy folds case. The policy
decides what *overlaps*; it does not decide what a region is.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `("widened to repo-wide", key_of("literal"), PathSet::RepoWide),`

A bounded prediction recorded as unbounded.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `(`

And an unbounded prediction recorded as bounded, which is the
one that lets a task run beside work it should have blocked.

## `fn a_dispatch_that_records_a_region_the_hints_do_not_derive…` › `("emptied", key_of("literal"), narrowed(&[])),`

The empty region is a real answer and not the derived one.

## `fn the_dispatch_fixture_records_the_region_the_fold_derives() {`

The default plan's dispatch fixture records the region the fold derives.

`region` is a table and `predicted_region` is a rule, so the corpus held
two answers to one question and every `task_dispatched` in this file
depended on their agreeing. They did — for the default plan — and the
same table was wrong for [`chain_plan`], which is why [`dispatch_in`]
exists. This is the round trip that keeps the surviving table honest:
it is not what proves the door right, it is what stops a fixture edit
from silently making every other test in this file dispatch a region the
run never predicted.

## `fn a_dispatch_takes_the_holding_its_origin_implies()` › `let repair_dispatch = |lease: LeaseGrant, source: Option<CandidateRef>| {`

An ordinary task may not inherit a lineage lease, and a repair may
not take one of its own; a repair names the candidate it was
materialized from, and an ordinary dispatch names none.

## `fn an_attempt_starts_in_the_open_generation_at_the_next_number() {`

-----------------------------------------------------------------------
ST-06: a completion applies only while its identity is the open one
-----------------------------------------------------------------------

## `fn an_attempt_starts_in_the_open_generation_at_the_next_num…` › `let elsewhere = attempt_started(&fold, ZETA, 1, 1, 0);`

The generation: not another task's, not a closed one, not one that
does not exist.

## `fn an_attempt_starts_in_the_open_generation_at_the_next_num…` › `for attempt in [0, 2, 7] {`

The number: dense from 1 within the generation, in both directions.

## `fn an_attempt_starts_in_the_open_generation_at_the_next_num…` › `assert!(matches!(`

A second attempt starts only after the first settles, and then at 2.

## `fn a_retained_session_belongs_to_the_incarnation_that_retai…` › `let base = sha("base");`

refusals[12], over the three ways a resume can be wrong and the one
way it can be right.

## `fn a_retained_session_belongs_to_the_incarnation_that_retai…` › `assert!(matches!(`

A settlement cannot retain a session for another incarnation.

## `fn a_retained_session_belongs_to_the_incarnation_that_retai…` › `assert!(matches!(`

Another session than the one retained.

## `fn a_retained_session_belongs_to_the_incarnation_that_retai…` › `accepts(&fold, &resume_with(&fold, "sess-ÜNI-0042"));`

The right session, in the incarnation that retained it.

## `fn a_retained_session_belongs_to_the_incarnation_that_retai…` › `let mut next_epoch = fold.clone();`

And the same event after a resume: the working tree was rolled back,
so the conversation's belief about what it left behind is false.

## `fn a_retained_session_belongs_to_the_incarnation_that_retai…` › `assert!(matches!(`

A fresh attempt in a retained generation is not a resume, and a
resume in a fresh generation is not a retry.

## `fn an_attempt_runs_the_frozen_binding_or_the_validated_over…` › `let base = sha("base");`

refusals[11] / INV-19, one component at a time. Each case moves one
field of an otherwise exact binding: a check that compared the whole
record, or that compared none of it, fails on the case it skipped.

`pinned` joined `agent`, `model`, `tier` and `effort` in the sweep of row 39.
It was the one coordinate of `RungBinding` no table in this file varied, and
`matches_frozen` ignoring it was a change the whole suite passed.

## `fn an_attempt_runs_the_frozen_binding_or_the_validated_over…` › `let mut off_the_end = attempt_started(&fold, ZETA, 0, 1, 0);`

A rung the ladder does not have. Since PR #180 it is refused as the
wrong rung before the ladder is indexed: the task stands on rung 0,
and an attempt runs at the task's position.

## `fn an_attempt_runs_the_frozen_binding_or_the_validated_over…` › `let mut materializing = attempt_started(&fold, ZETA, 0, 1, 0);`

A repair's attempt records what its worktree was materialized from,
and an ordinary one records nothing.

## `fn an_attempt_runs_the_frozen_binding_or_the_validated_over…` › `for rung in 0..rungs {`

The effort is the ladder's effort *for that rung's tier*, not the
run's default and not another tier's: zeta's rungs are small, mid and
frontier, resolving to three different efforts. Walked rung by rung
since PR #180 — an attempt is accepted only at the task's
replay-derived position, so each rung above the first is reached by
applying the exact start and escalating onto it — which is why this
walk comes last: everything above it runs in generation 0 on rung 0.

## `fn an_override_is_the_binding_the_frozen_admission_authoriz…` › `let mut fold = started();`

`task_registry.binding_override`: the override is "validated against
the frozen options of that task's open HumanBinding question", and
refusals[12] refuses one "for a wrong question ... or mismatched
fields". A1 proves the override names the same task, question and
option as the answer carrying it; the authority it is measured
against is the fold's, and it has to survive from the `task_spawned`
that froze it to the answer that draws on it.

## `fn an_override_is_the_binding_the_frozen_admission_authoriz…` › `for (index, agent) in options.iter().enumerate() {`

Every option, named exactly, is authorized.

## `fn an_override_is_the_binding_the_frozen_admission_authoriz…` › `for (label, index, agent) in [`

An option the admission froze for somebody else. Both directions of
the pairing are wrong: the agent of the *other* option, and an agent
the option list never held at all. Neither is caught by the range
check or by A1's internal agreement, because both are self-consistent
and in range.

## `fn an_override_is_the_binding_the_frozen_admission_authoriz…` › `assert!(matches!(`

An answer to a HumanBinding admission with no override at all leaves
its task with an empty ladder and nothing to run: `Admission::
HumanBinding` says the entry "cannot move until an answer records an
explicit one-off binding", and `Answer4.binding_override` is
"present exactly when the question was asking for a binding".

## `fn an_override_is_the_binding_the_frozen_admission_authoriz…` › `apply(&mut fold, &raised("q-park-Ünicode", ZETA));`

And the converse, which is the half nothing checked: an override on
a question that authorized no binding. The question here is an
ordinary park of another task, and the override is internally exact
— it names that question, that task and that option — so only the
admission authority distinguishes it.

## `fn an_override_is_the_binding_the_frozen_admission_authoriz…` › `accepts(`

The same answer without the override is the ordinary one.

## `fn an_override_is_the_binding_the_frozen_admission_authoriz…` › `let mut required = repair_spawn(TaskKey(4), ALPHA, ALPHA);`

A `HumanRequired` admission asks for a person, not for a binding.

## `fn an_interruption_closes_its_generation_and_returns_its_ta…` › `let base = sha("base");`

transaction_fault_matrix[T-ATTEMPT].resume_action: "append
attempt_interrupted (unknown spend, allowance refunded, generation
Closed, lease by kind); discard residue ... the task worktree
scrubbed with force ... task returns Pending; later dispatch new
generation". Nothing was judged and the spend is unknown, so the
generation is over — not idled and not reusable.

## `fn an_interruption_closes_its_generation_and_returns_its_ta…` › `assert!(matches!(`

"lease by kind", for a generation that closes: an ordinary one gives
up the region it predicted.

## `fn an_interruption_closes_its_generation_and_returns_its_ta…` › `assert!(matches!(`

Generation 0 is over, so it is not closed again and not restarted;
the run continues by dispatching the *next* dense generation.

## `fn an_interruption_closes_its_generation_and_returns_its_ta…` › `apply(&mut fold, &dispatch(ZETA, 1, &base));`

refusals[15], the coordinate that only matters once a *later*
generation is open: `generation_closed(0)` names generation 0, and
generation 1 is the open one. A close that took "whatever is open"
would close the newer generation under the older one's name, which
is a state no reader could recompute from the log.

## `fn an_interruption_closes_its_generation_and_returns_its_ta…` › `let mut lineage = started();`

A repair holds nothing of its own, so its interruption records
`LineageHeld` and its lineage lease is untouched.

## `fn an_override_replaces_the_frozen_binding_for_every_later_…` › `let base = sha("base");`

The other half of refusals[11]: when a human named a binding, that is
the authority, and the frozen rung is no longer one.

## `fn an_override_replaces_the_frozen_binding_for_every_later_…` › `accepts(`

The tier is not compared: an override chooses an agent from a frozen
option list, and the tier it lands on is whatever that agent is
bound at.

## `fn a_settlement_records_the_disposition_its_holding_admits()` › `let base = sha("base");`

refusals[14], as a crossed grid: two kinds of holding, three events
(one that keeps the generation, two that end it), three dispositions.
Exactly one cell per (holding, fate) is accepted.

## `fn a_settlement_records_the_disposition_its_holding_admits()` › `let closing = settle(`

A terminal failure ends the generation.

## `fn a_settlement_records_the_disposition_its_holding_admits()` › `let interrupted = ev(TopologyEventBody::AttemptInterrupted {`

An interruption *closes* the generation
(transaction_fault_matrix[T-ATTEMPT]: "generation Closed,
lease by kind"), so it records the same disposition a
terminal failure does — an ordinary generation releases its
predicted region, a lineage member goes on holding its
root's.

## `fn a_settlement_records_the_disposition_its_holding_admits()` › `for recorded in [`

**No `attempt_finished` leaves a generation open, so there is
no surviving disposition to enumerate here any more.** This
block asserted that a `succeeded` settlement recording
`PredictedRetained` (ordinary) or `LineageHeld` (lineage) is
accepted — the one case where a settlement kept its region to
hand to a candidate. Since the 2026-08-27 CONFORM ruling that
event is refused whatever it records, because
`candidate_prepared` is the sole successful settlement.

Re-derived rather than deleted: the claim becomes *refused
for every disposition*, which is stronger than the row it
replaces and fails if the transition is ever readmitted.

## (end of `fn a_settlement_records_the_disposition_its_holding_admits()`)

And the region a candidate inherits is decided on the event
that now settles the attempt: `check_candidate_prepared`
matches `CandidateLeaseEffect` against the entry's lineage.
`a_lineage_lease_only_ever_grows_and_a_released_one_is_gone`
holds that half.

## `fn a_settlement_applies_only_to_the_attempt_that_is_running…` › `let base = sha("base");`

refusals[16] / ST-06 for settlements, over each coordinate of the
identity in turn.

## `fn a_settlement_applies_only_to_the_attempt_that_is_running…` › `assert!(matches!(`

No attempt is running yet.

## `fn a_settlement_applies_only_to_the_attempt_that_is_running…` › `assert!(matches!(`

Another task, another generation, another attempt.

## `fn a_settlement_applies_only_to_the_attempt_that_is_running…` › `let interrupt = |key: TaskKey, generation: u32, attempt: u32| {`

The same three, for an interruption.

## `fn a_settlement_applies_only_to_the_attempt_that_is_running…` › `lease: LeaseDisposition::PredictedReleased,`

T-ATTEMPT closes the generation, so an ordinary one
releases the region it predicted.

## `fn a_generation_is_closed_only_from_an_open_class_with_no_a…` › `let base = sha("base");`

refusals[15], over every class a generation can be in.

## `fn a_generation_is_closed_only_from_an_open_class_with_no_a…` › `let mut fold = started();`

OpenNoAttempt: closable.

## `fn a_generation_is_closed_only_from_an_open_class_with_no_a…` › `let start = attempt_started(&fold, ZETA, 0, 1, 0);`

InFlight: not closable — the attempt is settled or interrupted first.

## `fn a_generation_is_closed_only_from_an_open_class_with_no_a…` › `let mut retained = fold.clone();`

RetainedIdle: closable — this is how a resume discards a session it
may not resume.

## `fn a_generation_is_closed_only_from_an_open_class_with_no_a…` › `let mut promoting = fold.clone();`

Promoting: not closable — a promoting generation is promoted.

**Reached by preparing a candidate, which is what promotes it.** This
cloned the in-flight fold and applied `succeeded(ZETA, 0, 1)`; since
the 2026-08-27 CONFORM ruling that event is refused, and a clone
alone would have left this case asserting about an *in-flight*
generation while calling itself the promoting one — the same
assertion passing for the wrong reason. `cargo` said so: the binding
stopped needing `mut`.

## `fn a_generation_is_closed_only_from_an_open_class_with_no_a…` › `let mut over = promoting.clone();`

Closed: not closable twice.

## `fn a_candidate_prepared_whose_record_failed_is_refused() {`

-----------------------------------------------------------------------
Candidates, the queue, and the publication relations
-----------------------------------------------------------------------

## `fn a_candidate_prepared_whose_record_failed_is_refused() {`

**A `candidate_prepared` whose record says the attempt failed is refused.**

The round-4 review of `09f9a99` set out the sequence exactly, and this is
it: a valid `run_started`, `task_dispatched` and `attempt_started`, then an
otherwise-consistent `candidate_prepared` whose embedded `AttemptRecord`
carries `failure: Some(GateFailed)`. Before this check the fold accepted
it, recorded the candidate, entered `Promoting`, and the task was carried
to `task_candidate_created` — **durably queued as a successful candidate
whose own authoritative evidence says a gate failed.**

The 2026-08-27 Class B change made this event the sole successful
settlement and enforced everything about it except the one thing that made
it *successful*. The fold is the authority against malformed, reconstructed
and faulty future writers, not just against this build's own driver, which
happens to supply a passing record.

It also earns the property `TopologyRun`'s brief already assumed: a
`candidate_prepared` record never carries feedback, because it never
carries a failure.

## `fn a_candidate_prepared_whose_record_failed_is_refused()` › `accepts(&fold, &candidate_prepared(ZETA, 0, &base));`

The premise: with a passing record this exact event is accepted, so the
refusal below is about the failure and not about anything else in it.

## `fn a_candidate_prepared_whose_record_failed_is_refused()` › `let generation = fold`

And nothing moved: a refused transition changes nothing, so the
generation is still in flight and has no candidate.

## `fn a_candidate_prepared_whose_review_did_not_pass_is_refused() {`

**A review outcome is authoritative, and both are.**

[`a_candidate_prepared_whose_record_failed_is_refused`] covers the
failure field. This covers the other half of the same predicate: a record
carrying no failure at all, whose reviews say `Failed` or `Unavailable`.

§11.2 requires *every* configured pass to pass, and a reviewer that could
not run "says nothing about the code" — which is not approval. Before
`AttemptRecord::is_successful` existed this door read `failure.is_none()`
alone, so a record whose primary reviewer returned `Failed` was promoted,
charged against the rung allowance and queued as a candidate. The
`b1f54a5` review walked that sequence.

## `fn a_candidate_prepared_whose_review_did_not_pass_is_refuse…` › `accepts(&fold, &candidate_prepared(ZETA, 0, &base));`

The premise: the same event with the pass *passed* is accepted, so
the refusal below is about the outcome and nothing else.

## `fn a_candidate_prepared_whose_review_did_not_pass_is_refuse…` › `assert!(data.attempt.failure.is_none());`

The failure field stays empty on purpose: this is the shape the
old `failure.is_none()` door called successful.

## `fn a_candidate_prepared_whose_review_did_not_pass_is_refuse…` › `let generation = fold`

Nothing moved.

## `fn reviews_off_started() -> TopologyFold {`

--- the frozen review plan is the success domain ----------------------

`PR7-G2-W1-SUCCESS-IGNORES-THE-FROZEN-PLAN` (§2, §22e). The witnesses
above are round 6's *outcome* half — a configured pass that ran and did
not pass. These are the *presence* half: a pass the plan configured and
the record does not carry at all.

## `fn reviews_off_started() -> TopologyFold {`

A run whose frozen plan obliges **nothing**: verification off.

`plan_for`'s disabled branch resolves no `primary` either, so this is
the shape production writes and not merely `enabled = false` bolted onto
a resolved reviewer.

## `type ObligationRow = (`

One row of the obligation grid: a label, the fold whose frozen plan is
under test, the task, the passes that plan obliges, and the labelled
lists it refuses.

## `fn prepared_with_passes(`

A `candidate_prepared` for `key` carrying exactly `passes`, all passed.

## `fn in_flight_at(fold: &mut TopologyFold, key: TaskKey, base: &CommitSha) {`

A fold with `key`'s first attempt in flight, ready for its settlement.

## `fn candidate_success_is_judged_against_the_tasks_frozen_review_plan() {`

**Zero, one and many configured passes: the record carries the frozen
obligation and nothing else.**

The arity grid, because the defect is about the *domain* of a predicate
and a one-pass fixture cannot show a domain. `review_plan` configures a
second opinion for index 2 alone, so `MID` obliges two passes and `ZETA`
one; a run that froze verification off obliges none.

Each row asserts both directions: the obliged list is accepted, and the
same event carrying any other list is refused. The negative half is what
makes the positive half a measurement — `is_successful` was true of every
one of these records, which is exactly why it could not tell them apart.

## `fn candidate_success_is_judged_against_the_tasks_frozen_rev…` › `let rows: Vec<ObligationRow> = vec![`

(label, the fold's frozen plan, the task, what it obliges, what it refuses)

## `fn candidate_success_is_judged_against_the_tasks_frozen_rev…` › `("a lone second opinion", vec![pass(SECOND)]),`

The finding's own shape: a lone passed second opinion.
Every entry green, and the pass §11.2 requires absent.

## `fn candidate_success_is_judged_against_the_tasks_frozen_rev…` › `("no passes at all", vec![]),`

An empty list: `all` over nothing is true.

## `fn candidate_success_is_judged_against_the_tasks_frozen_rev…` › `accepts(&fold, &prepared_with_passes(key, &base, &obliged));`

The premise: the obliged list is what the door takes.

## `fn candidate_success_is_judged_against_the_tasks_frozen_rev…` › `let generation = fold`

Nothing moved: the generation is still in flight and holds no
candidate, so a refusal cannot have charged the rung.

## `fn a_run_that_froze_verification_off_obliges_no_pass_whatever_it_resolved() {`

**A run that froze verification off obliges no pass, whatever it
resolved.**

The `enabled` flag and the resolved bindings are independent fields, and
`plan_for`'s disabled branch happens to leave `primary` unset — so a
grid built only from what that function produces cannot tell the flag
from the absence of a reviewer. The fold reads **logs**, and
`enabled: false` beside a resolved primary and a resolved second opinion
is a shape the wire admits: `run_started(4).reviews` carries both, and a
`task_spawned` embeds a whole frozen entry.

Three of this file's fixtures froze exactly that combination while their
records carried a passed `review`, which is what makes this the shape
worth pinning rather than a hypothetical: read one way it obliges a pass
nobody ran, read the other it obliges none.

## `fn a_run_that_froze_verification_off_obliges_no_pass_whatev…` › `let unauthenticated = RunStarted4 {`

Resolved reviewers, and the switch off. The second opinion is
resolved for `MID` too, so the "many" arm is off as well as the
"one" arm.

## `fn a_run_that_froze_verification_off_obliges_no_pass_whatev…` › `let entry = fold`

The premise: the reviewers *are* resolved on this entry, so an
obligation derived from the bindings alone would not be empty.

## `fn the_door_accepts_exactly_the_passes_the_frozen_entry_obliges() {`

**The obligation is the plan's, read through the plan's own reader.**

The round trip: whatever `FrozenReviews::obliged_lenses` says a task
owes is exactly what the door accepts, and the door accepts nothing
else. It is deliberately *not* how
[`candidate_success_is_judged_against_the_tasks_frozen_review_plan`]
is written — that grid transcribes the obligation by hand, so the two
together say both "the fold agrees with the reader" and "the reader says
what §11.2 says".

## `fn the_door_accepts_exactly_the_passes_the_frozen_entry_obl…` › `for dropped in 0..obliged.len() {`

And one fewer is refused, whichever pass is dropped.

## `fn an_attempt_finished_whose_record_says_success_is_refused() {`

**A failed settlement whose record says the attempt succeeded is refused.**

The mirror of the candidate door, through the same predicate. This door
refused `Succeeded` and asked nothing further, so an `attempt_finished`
could fail a task — halting the run — while carrying a ledger line whose
failure field is empty and whose every review passed. That line is what a
person reads when deciding whether to trust a run.

## `fn an_attempt_finished_whose_record_says_success_is_refused…` › `accepts(&fold, &settle(ZETA, 0, 1, closed()));`

The premise: with a record that says failed, this exact settlement
applies.

## `fn an_attempt_finished_whose_record_says_success_is_refused…` › `*data.record = attempt_record(1);`

Exactly the successful shape: no failure, every pass passed.

## `fn an_attempt_finished_whose_record_names_another_attempt_is_refused() {`

**The envelope and the record name one attempt.**

Without this the ledger line a settlement carries can belong to a
different attempt of the same generation — attempt 2's cost, duration and
model recorded against attempt 1's settlement, with every derived total
reading it as authoritative.

## `fn a_successful_attempt_charges_its_rung_live_and_on_replay() {`

**A successful attempt spends one of its rung's allowance, and the count
survives replay.**

`spends_allowance(None)` is `true` — the worker ran and its work was judged
and accepted — so a success charges the rung exactly as a judged failure
does. That was true while `attempt_finished{Succeeded}` was the settlement,
and it stopped being true on 2026-08-27 when the settlement moved to
`candidate_prepared` and the increment stayed behind in `apply_settlement`.

Nothing noticed. The suite was green, the allowance census went on finding
its one write site, and the replacement witness asserted `Promoting` and
candidate presence — none of which is the allowance. A **first-attempt
success left `attempts_on_rung` at zero**, so a later reader could grant an
extra attempt on a rung already paid for. The round-4 review of `09f9a99`
found it.

Both positions are driven, because they fail differently: a first-attempt
success is the count going 0 → 1 with nothing before it, and a
second-attempt success is the successful charge landing *on top of* a
failure's. And the live count is compared against a replay of the same log,
because a fold that counts live and not on replay is the divergence this
project measures everything else against.

## `fn a_successful_attempt_charges_its_rung_live_and_on_replay…` › `let mut generation = 0;`

Optionally a judged failure first, which retries into a new
generation — the shape a second-attempt success actually has.

## `fn a_successful_attempt_charges_its_rung_live_and_on_replay…` › `let replayed = TopologyFold::replay(inputs(), &trace).expect("the trace replays");`

And a replay of exactly those bytes reaches the same number.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_is_in_flight() {`

**A candidate is prepared by the generation whose attempt is in flight,
and preparing it is what settles that attempt.**

Re-derived, not adjusted. This was
`a_candidate_is_prepared_by_the_generation_whose_attempt_succeeded`, and it
asserted the opposite of the first claim below: that `candidate_prepared`
is **refused** while the attempt is still running, and accepted only after
an `attempt_finished{Succeeded}` had promoted the generation. That is the
dual-settlement pattern `decisions/2026-08-12-merge-queue-execution-topology.md`
forbids — "`attempt_finished` is not also emitted for that attempt" — and the
fold was *requiring* it. Ruled CONFORM 2026-08-27.

The other three claims are unchanged and still ST-06's: not another
generation's, not another task's, and parented on the base the generation
was dispatched at.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `accepts(&fold, &candidate_prepared(ZETA, 0, &base));`

The attempt is running, and this event settles it.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `assert!(matches!(`

ST-06: not another generation's, and not another task's.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `let mut reparented = candidate_prepared(ZETA, 0, &base);`

The commit is parented on the base the work started from, and that
base is the one the generation was dispatched at. INV-09's
exact-base decision compares the head against `base_sha` and then
publishes `commit_sha`, so both claims have to hold.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `let mut inconsistent_region = candidate_prepared(ZETA, 0, &base);`

The region it takes is the region its diff touched.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `let mut widening = candidate_prepared(ZETA, 0, &base);`

An ordinary candidate replaces its predicted region; only a lineage
member widens a lineage.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `for wrong in [0, 2, 9] {`

ST-06's "wrong attempt number", for the record the candidate
carries. The generation ran attempt 1, so 0, 2 and 9 all name an
attempt that did not produce this commit. Without this the embedded
record is inert data and a candidate can be published attributed to
an attempt that failed.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `apply(&mut fold, &candidate_prepared(ZETA, 0, &base));`

Preparing takes the actual region and gives up the predicted one.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `assert!(matches!(`

INV-06: "at most one candidate per generation", enforced_by "fold
refuses a second candidate for a generation". The second record is
valid in isolation — it is the *same* event that was just accepted,
and so is a differing one — and it is refused because the generation
has already prepared.

## `fn a_candidate_is_prepared_by_the_generation_whose_attempt_…` › `let mut promotes_second = candidate_created(ZETA, 0);`

And the first candidate is still the one the generation holds, so a
promotion of the second has nothing to promote.

## `fn a_candidate_names_the_attempt_that_produced_it_live_and_…` › `let base = sha("base");`

ST-06 for `candidate_prepared`, through the durable path as well as
the live one: the generation retried, so attempt 2 is the authority
and the number the earlier attempt carried is no longer one.

## `fn a_candidate_names_the_attempt_that_produced_it_live_and_…` › `assert!(matches!(`

Attempt 1 ran and did not produce this candidate; attempt 2 did.

## `fn a_candidate_names_the_attempt_that_produced_it_live_and_…` › `let bytes = |trace: &[TopologyEvent]| -> Vec<u8> {`

The same pair over the wire: a log whose candidate names attempt 1
stops at that line, and the authoritative one replays.

## `fn a_promotion_names_the_candidate_that_was_prepared()` › `let base = sha("base");`

ST-06's "a mismatched task_candidate_created", over every coordinate
of the reference: a promotion that named another commit would give
the queue a position pointing at an object nothing judged.

## `fn a_promotion_names_the_candidate_that_was_prepared()` › `assert!(matches!(`

Before anything was prepared.

## `fn a_promotion_names_the_candidate_that_was_prepared()` › `apply(&mut fold, &candidate_created(ZETA, 0));`

Promotion ends the generation and takes the queue position.

## `fn two_queued() -> TopologyFold {`

Two candidates queued in an order the fixture chose, so "first" is a
position rather than a coincidence.

## `fn an_integration_starts_only_for_the_first_eligible_candid…` › `let head = sha("head");`

refusals[8]. The queue is FIFO by promotion order and the *first
eligible* entry is integrated, which is not the same as the first
one: three of the four ineligibility rules move the answer past the
head of the queue, and the fourth is the head itself being fine.

## `fn an_integration_starts_only_for_the_first_eligible_candid…` › `assert!(matches!(`

A candidate holding no position at all.

## `fn an_integration_starts_only_for_the_first_eligible_candid…` › `let mut parked = fold.clone();`

Its task parked: the entry keeps its place and the next eligible one
is integrated instead.

## `fn sequences_are_dense_and_one_transaction_runs_at_a_time()` › `let head = sha("head");`

refusals[6], [7] and the sequence half of [10].

## `fn sequences_are_dense_and_one_transaction_runs_at_a_time()` › `assert_eq!(`

A second transaction while one is unresolved.

## `fn sequences_are_dense_and_one_transaction_runs_at_a_time()` › `let unavailable = |sequence: u32| {`

An event that names a sequence other than the open one.

## `fn sequences_are_dense_and_one_transaction_runs_at_a_time()` › `apply(&mut fold, &unavailable(0));`

Resolving one consumes its number: the next transaction is 1.

## `fn sequences_are_dense_and_one_transaction_runs_at_a_time()` › `assert_eq!(`

And an event that belongs to no transaction at all.

## `fn a_stale_verification_runs_only_on_a_candidate_that_is_ac…` › `let base = sha("base");`

INV-09: the exact-base decision is made from the head before any
staging effect, so a candidate whose base *is* the head is published
fast and is never cherry-picked or re-verified.

## `fn a_stale_verification_runs_only_on_a_candidate_that_is_ac…` › `let mut stale_at_head = verification_started(MID, 0, 0, &head, &head);`

A stale-clean verification judges the proposal the cherry-pick
produced; an already-present one judges the head itself. Each refuses
the other's shape.

## `fn the_publication_relations_hold_over_the_crossed_disposit…` › `let base = sha("base");`

refusals[9] and the fold half of refusals[22], as relations rather
than examples: for each disposition, the accepted publication and
every single-field departure from it. A lookup table keyed on these
inputs would have to hold every row of this grid, and the rows are
generated from the same fixture the accepted case is.

## `fn the_publication_relations_hold_over_the_crossed_disposit…` › `let fold = two_queued();`

--- fast: the head is exactly the candidate's base -----------------

## `fn the_publication_relations_hold_over_the_crossed_disposit…` › `let mut stale = two_queued();`

--- stale_clean: the pinned proposal, at the head that was read ----

## `fn the_publication_relations_hold_over_the_crossed_disposit…` › `let mut present = two_queued();`

--- already_present: the head is what was verified -----------------

## `fn the_publication_relations_hold_over_the_crossed_disposit…` › `assert!(`

--- the dispositions do not stand in for one another ---------------

## `fn the_publication_relations_hold_over_the_crossed_disposit…` › `assert!(`

And a verified publication cannot open its own transaction, nor a
fast one join somebody else's.

## `fn a_publication_names_the_candidate_durable_history_record…` › `let base = sha("base");`

refusals[8]: a publication's relations are against "the candidate's
recorded base_sha" and "the candidate's recorded commit_sha" — the
record `candidate_prepared` left and the queue entry
`task_candidate_created` took, not a copy the event brought with it.

The disposition grid moves one field of the *event* and leaves the
record alone, so an event that disagrees with itself is what it
catches. What it cannot catch is a forgery: an embedded CandidateRef
that is internally exact and agrees with every intra-event relation
A1 checks, and simply names something durable history never
recorded. Each case below moves exactly one coordinate of that
identity away from history while keeping the event self-consistent,
so a fold that matched on the remaining coordinates accepts it.

## `fn a_publication_names_the_candidate_durable_history_record…` › `let fold = two_queued();`

--- fast ----------------------------------------------------------
A1 pins proposed_sha == candidate_sha for a fast publication, so the
one coordinate a forger is free to move is the ref.

## `fn a_publication_names_the_candidate_durable_history_record…` › `let verified = |basis_stale: bool| {`

--- stale_clean and already_present --------------------------------
Here the proposal is the pinned one rather than the candidate's
commit, so `candidate_sha` is free too: both coordinates of the
cross-record identity can be forged one at a time.

## `fn a_publication_names_the_candidate_durable_history_record…` › `accepts(`

The unforged shape is authorized, so the refusals below are
about the forged coordinate and about nothing else.

## `fn a_publication_names_the_candidate_durable_history_record…` › `if let TopologyEventBody::MergePrepared { data } = &event.body {`

Self-consistent: A1 has nothing to say about it.

## `fn a_publication_names_the_candidate_durable_history_record…` › `let mut trace = vec![run_started_event()];`

--- and the same, through the durable path -------------------------
A forged publication in a log must stop the replay at its own line,
not be applied and then contradicted later.

## `fn nudge_last(value: &CommitSha) -> CommitSha {`

The same SHA with its last character moved: a value that differs from
the original in one position out of forty and agrees on every prefix
shorter than the whole.

## `fn a_publication_compares_whole_shas_and_not_prefixes()` › `let base = sha("base");`

refusals[8] names four SHA relations, and every one of them is
equality of a commit identity. A comparison that truncated, folded
case, or matched a prefix would still reject the grid's cases, which
move a SHA to an unrelated value. These move one character of forty,
at the end, so a comparison of anything less than the whole accepts
them.

## `fn a_verified_publication_belongs_to_its_own_sequence_and_i…` › `let head = sha("head");`

refusals[8] for the two coordinates that identify *which* verification
authorized a publication: the source's sequence, and the candidate
the open transaction is verifying. Any `Verification` source and any
open transaction are the right ones as long as only one exists, so
both need a state where more than one identity is available.

## `fn a_verified_publication_belongs_to_its_own_sequence_and_i…` › `let mut fold = two_queued();`

Sequence 0 ran and was interrupted; sequence 1 is the open one. Both
are `Verification` sources, so the variant alone no longer decides.

## `fn a_verified_publication_belongs_to_its_own_sequence_and_i…` › `let zeta = candidate_of(ZETA, 0);`

The open transaction is verifying mid; a publication of zeta copies
its head, proposal, pin and source and is refused because the
transaction is not about zeta.

## `fn an_already_present_publication_expects_the_head_its_veri…` › `let head = sha("head");`

refusals[8]: "merge_prepared(already_present) whose proposed_sha
differs from expected_head **or from the verified head**". The two
are separate relations, and a self-consistent event satisfies the
first while contradicting the second: H2/H2 agrees with itself and
names a head no verification of this sequence ever read.

## `fn one_integration_transaction_at_a_time_including_an_autho…` › `let base = sha("base");`

refusals[7], and the class it is easiest to lose: a fast
`merge_prepared` opens a transaction that stays unresolved until
`task_merged`. "An authorized publication is always completed
(recovery or run-end closure), never abandoned" (INV-09), so the
next start waits for it.

## `fn one_integration_transaction_at_a_time_including_an_autho…` › `apply(&mut fold, &merged(MID, 0, 0, vec![MID]));`

Once the ref has moved and the merge is recorded, the next one may
start — at the adjacent sequence.

## `fn the_queue_is_ordered_by_creation_and_not_by_preparation()` › `let base = sha("base");`

`coordinator_integration.queue`: "FIFO by **task_candidate_created**
append order". Preparation and creation are separate events and a
fixture that always pairs them cannot tell which clock the order
came from. Here they are deliberately crossed: mid prepares first
and zeta is created first, so the two clocks disagree and only one
of them produces the queue the packet describes.

## `fn the_queue_is_ordered_by_creation_and_not_by_preparation()` › `apply(&mut fold, &candidate_created(ZETA, 0));`

Prepared mid, then zeta. Created zeta, then mid.

## `fn the_queue_is_ordered_by_creation_and_not_by_preparation()` › `let head = sha("head");`

And the first *eligible* entry is the one an integration may start
for, which is the same statement read through the refusal.

## `fn keys_and_generations_are_dense_in_both_directions()` › `let base = sha("base");`

refusals[10]: "non-dense keys, generations". The tested direction has
always been the gap above; the direction nothing reached is the one
below, where a duplicate or earlier key would re-register a task or
re-open a generation that is over.

## `fn keys_and_generations_are_dense_in_both_directions()` › `let mut reopened = started();`

Generations are dense per task, and alpha's generation 0 is over.

## `fn a_wake_clears_every_waiter_in_one_delta()` › `let base = sha("base");`

`defer_wait_elapsed` is a run-level event, not a per-item one: the
closure procedure's step (5b) and `coordinator_integration.queue`
both describe deferral as a flag cleared "until the next
defer_wait_elapsed or run_resumed", with no notion of which waiter it
is about. A wake that cleared the first of each kind is
indistinguishable from one that cleared all of them unless more than
one of each is waiting.

## `fn a_wake_clears_every_waiter_in_one_delta()` › `for key in [ALPHA, MID] {`

Two tasks deferred by their settlements.

## `fn a_wake_clears_every_waiter_in_one_delta()` › `apply(&mut fold, &dispatch(ZETA, 0, &base));`

And a candidate deferred by an outage.

## `fn a_wake_clears_every_waiter_in_one_delta()` › `assert_eq!(fold.queue().expect("started").entries()[0].defers, 1);`

The count survives the wake, so the next deferral is the next
consecutive one rather than a restart.

## `fn a_publication_settles_the_closure_the_fold_derives()` › `let base = sha("base");`

refusals[10]'s "invalid satisfies", over a lineage deep enough that
the closure is neither the candidate alone nor the whole registry.

## `fn a_publication_settles_the_closure_the_fold_derives()` › `let mut lineage = started();`

A repair carries the work of everything it descends from, so
publishing it settles the whole chain back to the root.

## `fn a_merge_copies_the_authorization_exactly()` › `assert!(matches!(`

The ref moves only after a publication was authorized.

## `fn a_merge_copies_the_authorization_exactly()` › `let mut elsewhere = merged(MID, 0, 0, vec![MID]);`

A different commit than the one authorized.

## `fn a_merge_copies_the_authorization_exactly()` › `for wrong in [vec![MID, ZETA], vec![MID, MID], Vec::new(), vec![ZETA]] {`

A closure that is not the authorization's — as a *vector*, so a
duplicated or emptied list is as wrong as a widened one and a
set-shaped comparison is not enough.

## `fn a_merge_copies_the_authorization_exactly()` › `let mut lineage_release = merged(MID, 0, 0, vec![MID]);`

A lease release that is not the one this publication owes.

## `fn a_merge_copies_the_authorization_exactly()` › `apply(&mut fast, &merged(MID, 0, 0, vec![MID]));`

Merging settles the closure, frees the position and the region.

## `fn unavailable_event(`

-----------------------------------------------------------------------
Outages, rejections and lineage
-----------------------------------------------------------------------

## `fn a_deferred_verification_is_consecutive_and_within_the_fr…` › `let head = sha("head");`

refusals[16] and `coordinator_integration.dispositions`, as the
partition they are: an Infrastructure outage defers "while defers <
the frozen max_defers" and parks "at max_defers". The run froze
max_defers = 2, so exactly one deferral is available and the second
outage parks. Both arms are crossed against every count, so a fold
that moved the boundary either way is caught in one direction or the
other.

The allowance is read from the frozen record and the expected
verdicts are computed from the packet's inequality, not from the
function under test.

## `fn a_deferred_verification_is_consecutive_and_within_the_fr…` › `apply(`

Count 0 -> the run may still defer, and may not yet park.

## `fn a_deferred_verification_is_consecutive_and_within_the_fr…` › `apply(`

Count 1 -> the next deferral would be the max_defers'th, so the
allowance is spent: the outage parks and may not defer at all. This
is the cell `defers > max_defers` accepted and `defers >= max_defers`
refuses.

## `fn a_deferred_verification_is_consecutive_and_within_the_fr…` › `apply(`

The count is this candidate's own history, not the run's. The second
queued candidate has deferred nothing, so its own first deferral is
still 1 while MID sits at 1 — a fold that summed the queue would
demand 2 here and refuse the count the packet requires.

## `fn an_outage_that_needs_a_person_parks_with_a_question_that…` › `assert!(matches!(`

A human finding cannot be waited out.

## `fn an_outage_that_needs_a_person_parks_with_a_question_that…` › `assert!(matches!(`

A park that offers nothing to answer with.

## `fn an_outage_that_needs_a_person_parks_with_a_question_that…` › `assert!(matches!(`

A park whose question is about somebody else.

## `fn an_outage_that_needs_a_person_parks_with_a_question_that…` › `apply(`

Parking moves the task to awaiting input, and its answer returns it
to awaiting merge to be re-verified under a new sequence.

## `fn a_rejection_creates_or_widens_exactly_one_lineage_and_re…` › `assert!(matches!(`

The repair's dependency has to be merged, and `alpha` is not yet.

## `fn a_rejection_creates_or_widens_exactly_one_lineage_and_re…` › `apply(&mut ready, &rejection(1, None));`

Applying it: the candidate leaves the queue, the task awaits its
repair, and the lineage holds the region.

## `fn a_conflict_opens_and_closes_its_own_transaction()` › `let base = sha("base");`

A conflict is decided at the cherry-pick, before any verification
starts, so it is the first append of its sequence rather than a
terminal of somebody else's.

## `fn a_conflict_opens_and_closes_its_own_transaction()` › `let leases = fold.leases().expect("started");`

The lineage holds the candidate's region *and* the conflict's.

## `fn answered(key: TaskKey, id: &str, answer: Answer4) -> TopologyEvent {`

-----------------------------------------------------------------------
Questions, budget, and the end of a run
-----------------------------------------------------------------------

## `fn an_answer_names_an_open_question_of_that_task_and_an_opt…` › `let mut fold = started();`

refusals[13]. A1's half — the override must name the same question,
task and option as the answer carrying it — is wired in; this adds
the three the fold owns.

## `fn an_answer_names_an_open_question_of_that_task_and_an_opt…` › `assert!(matches!(`

A question this log never asked.

## `fn an_answer_names_an_open_question_of_that_task_and_an_opt…` › `assert!(matches!(`

The right question, about another task.

## `fn an_answer_names_an_open_question_of_that_task_and_an_opt…` › `for option_index in [3, 4, 99] {`

An option it did not offer: the fixture's question has three.

## `fn an_answer_names_an_open_question_of_that_task_and_an_opt…` › `let mismatched = answered(`

An override that disagrees with the answer carrying it.

## `fn an_answer_names_an_open_question_of_that_task_and_an_opt…` › `apply(`

Answered once: the second answer has no open question to name.

## `fn an_answer_names_an_open_question_of_that_task_and_an_opt…` › `assert!(matches!(`

And its id is never reused for a new question either.

## `fn a_continuation_is_offered_only_for_an_open_generation_no_attempt_has_used() {`

[`TopologyFold::eligible_continuation`], which had no test in this file. The
engine's selector reads it to decide whether a task with an open generation is
continued rather than dispatched afresh, so what it returns is the generation a
worker is about to resume into.

Two things are asserted that no other test held. **The identity**: the fixture
closes generation 0 and dispatches generation 1, so an answer of
`GenerationId(0)` — a reader that took the first generation rather than the open
one — is a different value from the right one. **The class filter**: in flight,
retained-idle and promoting each get their own arm, because dropping the
`OpenNoAttempt` filter is a change the fold suite otherwise passes.
`src/topology/fold/tests/questions.rs` reaches this reader through `select` and
holds the lineage-question guard; it holds neither of these.

## `fn lineage_with_a_dispatched_repair(ask_about: Option<TaskKey>) -> TopologyFold {`

A lineage whose repair holds an open generation, optionally with a question open
on the root.

**The question is installed through `RunState::open_question` rather than by an
event, and the fixture proves it has to be.** Both orders are refused:
dispatching the member after the question fails `task_dispatched` ("dispatch
requires no outstanding lineage question or candidate for this task"), and
raising the question after the dispatch fails `question_raised` ("settle it
before parking its tasks"). So `eligible_continuation`'s `lineage_has_question`
arm guards a state no legal log reaches, and the `refuse` above the hand-built
state records that rather than leaving a reader to wonder. Building an
unreachable cell by hand is what [`grid_state`] does, for the same reason.

## `fn the_eligible_integration_candidate_is_the_first_the_queue_still_offers() {`

[`TopologyFold::eligible_integration_candidate`], which had no test in this file
either. `integration_admissible` is its `is_some()` and is asserted in seven
places, so the *predicate* was covered and the *identity* was not: the selector
integrates the candidate this returns, and a reader that offered the wrong one
would have left all seven of those assertions true.

The arms are its three early returns and the queue's own eligibility. The parked
arm is the one that pins identity — with `mid` awaiting input the offer must be
`zeta`, and the two candidates are asserted distinguishable first, so an
either-answer reader cannot pass. Dropping the open-transaction guard is a change
the fold suite otherwise passes.

## `fn an_answer_is_refused_after_a_halt_or_a_budget_stop_in_th…` › `let base = sha("base");`

refusals[20], and the epoch scope that makes a resume the way back: both a
budget-stopped run and a halted one ingest the answer after their resume.

**The sentence that stood here until the sweep of row 39 said the halted one
never does, "because `halted_at` is never cleared".** `halted_at` is indeed
never cleared, and `derived_outcome` reads it, so a resumed halted run still
derives Halted -- but `check_question_answered` does not read it. It compares
`halted_epoch` with the current epoch, and a resume moves the epoch, so the
answer door reopens. The two halves of this file's own prose disagreed about
that, the paragraph below having it right, and the test stopped one line short
of asking: it resumed and then asserted only that `halted_at` survived.
Replacing the guard with `self.halted_at.is_some()` -- the reading the wrong
sentence describes -- was a change the whole fold suite passed.

## `fn an_answer_is_refused_after_a_halt_or_a_budget_stop_in_th…` › `apply(&mut halted, &resume(container_runner()));`

A halt is epoch-scoped for ingestion and permanent for the outcome:
the answer file stays on disk, and a resumed halted run still
derives Halted.

## `fn a_budget_stop_belongs_to_the_epoch_that_hit_the_ceiling()` › `apply(&mut fold, &resume(container_runner()));`

A resume starts a new epoch without one, and the next breach belongs
to that epoch rather than the old one.

## `fn a_wait_never_elapses_under_a_halt_or_a_budget_stop()` › `let base = sha("base");`

refusals[18]: halt and budget outrank backoff.

## `fn a_wait_never_elapses_under_a_halt_or_a_budget_stop()` › `apply(&mut budget, &resume(container_runner()));`

Cleared by the resume that raises the ceiling.

The halt below is the other half of that, and the asymmetry is the point:
`check_defer_wait_elapsed` guards on `halted_at.is_some()`, which no resume
clears, while `check_question_answered` two files away guards on
`halted_epoch == self.epoch`, which every resume moves. So one resume reopens
answers and leaves waits refused. Each guard is asserted after a resume here,
because each was free to take the other's form: swapping either one for the
other's expression left the fold suite green.

## `fn a_wait_never_elapses_under_a_halt_or_a_budget_stop()` › `let head = sha("head");`

And what it does when it is allowed: wakes every deferred task and
every verification-deferred candidate at once.

## `enum Blocker {`

-----------------------------------------------------------------------
The derived outcome (INV-15, refusals[19])
-----------------------------------------------------------------------

## `enum Blocker {`

What is holding the run open, if anything.

Every open generation class and both transaction classes, because
`common` is the claim that *none* of them is outstanding: a fold that
counted only the ones somebody remembered would end a run holding a
retained session or an authorized publication.

## `const BLOCKERS: [Blocker; 7] = [`

Every value of [`Blocker`], so the grid crosses the whole dimension.

## `enum Budget {`

Whether a budget stop exists, and whether it belongs to this epoch.

## `enum Backoff {`

What is backing off, if anything.

## `enum Shape {`

The shape of the task set. Chosen so that "some task could still be
admitted" and "every task has settled" are both determined by it, since
no state can hold them independently.

## `enum Shape` › `AllTerminal,`

Every task merged.

## `enum Shape` › `BlockedByFailure,`

A failure, and the tasks that can never run because of it.

## `enum Shape` › `AdmissiblePending,`

A task that could be dispatched right now.

## `enum Shape` › `Stuck,`

Neither settled nor admissible: the shape the design argues is
unreachable, kept here because "unreachable" is a claim about
histories and this is a claim about states.

## `fn expected_outcome(`

The packet's total function, written from its text over the dimensions
rather than over a state.

This is the whole point of the grid: production derives each dimension
from state and then applies the precedence, and this applies the
precedence to the dimensions directly. A defect in either half — a
dimension read wrongly from state, or a precedence applied in the wrong
order — separates the two.

## `fn grid_state(`

A state realizing one cell of the grid.

Built by writing the fold's own state rather than by replaying a
history: the obligation is that the function is total over states, and
which of those states a history can reach is the bounded census's
question, not this one's.

## `fn the_derived_outcome_is_total_over_the_crossed_fold_state…` › `let mut cells = 0;`

1008 cells: seven blockers (nothing, each of the four open
generation classes, and each of the two transaction classes),
halting or not, three budget scopes, three backoff shapes, questions
or not, four task-set shapes.

## `fn the_derived_outcome_is_total_over_the_crossed_fold_state…` › `assert_eq!(reached.len(), 6, "arms reached: {reached:?}");`

Every arm of the function, including the one the design argues is
unreachable: a value a census can assert about rather than a panic.

## `fn pending_backoff_blocks_parked_and_complete_and_never_blo…` › `for blocker in BLOCKERS {`

The one precedence consequence the packet states in its own words,
asserted as a relation over the crossed grid rather than as an
example: for every cell, adding backoff moves Parked and Complete to
NotEnding and leaves every other answer exactly where it was.

## `fn a_run_ends_at_the_outcome_its_state_implies_or_not_at_al…` › `let outcomes = [`

refusals[19]: every outcome has an accepted and a refused instance,
and the refusals are the four the packet names by hand.

## `fn a_run_ends_at_the_outcome_its_state_implies_or_not_at_al…` › `let complete = grid_state(`

Complete: every task settled, nothing queued, nothing held.

## `fn a_run_ends_at_the_outcome_its_state_implies_or_not_at_al…` › `let parked = grid_state(`

Parked: an open question and nothing admissible.

## `fn a_run_ends_at_the_outcome_its_state_implies_or_not_at_al…` › `let halted = grid_state(`

Halted: a halting settlement, whatever else is true.

## `fn a_run_ends_at_the_outcome_its_state_implies_or_not_at_al…` › `let budget = grid_state(`

BudgetExceeded: a stop in this epoch and no halting settlement —
accepted with a deferred task present, which Parked and Complete are
not.

## `fn a_run_ends_at_the_outcome_its_state_implies_or_not_at_al…` › `let running = grid_state(`

NotEnding: nothing is accepted at all.

## `fn a_run_ends_at_the_outcome_its_state_implies_or_not_at_al…` › `assert!(matches!(`

And the attribution has to be the fold's: a halt recorded against
another task, or none at all, is a report of a run that did not
happen.

## `fn a_finished_run_is_continued_only_by_the_resume_its_outco…` › `let base = sha("base");`

refusals[21]: Complete and Halted are terminal — finalized and then
refused. Parked and BudgetExceeded resume, and the only event that
continues them is that resume.

## `fn every_kind() -> Vec<TopologyEvent> {`

-----------------------------------------------------------------------
INV-02: one transition, poisoning, and the whole-log parse
-----------------------------------------------------------------------

## `fn every_kind() -> Vec<TopologyEvent> {`

One of every kind, so a table over the vocabulary is a table over all of
it rather than over the ones somebody remembered.

## `fn every_kind() -> Vec<TopologyEvent>` › `settle(`

**`attempt_finished`, on a transition this fold accepts.** The
table held `succeeded(ZETA, 0, 1)` here, and since the 2026-08-27
CONFORM ruling that is not a settlement the fold accepts at all —
`candidate_prepared` further down is the successful one. A
*poisoned* fold must still refuse `attempt_finished`, so the kind
stays in the table on a transition a healthy fold would take.

## `fn every_kind() -> Vec<TopologyEvent>` › `lease: LeaseDisposition::PredictedReleased,`

T-ATTEMPT closes the generation, so an ordinary one
releases the region it predicted.

## `fn a_poisoned_fold_refuses_every_transition()` › `let mut fold = started();`

refusals[24]: the command has already ended. Nothing is appended and
nothing is derived from memory — including the informational records,
which a process that cannot vouch for its own state may not write
either.

## `fn a_poisoned_fold_refuses_every_transition()` › `let mut clean = started();`

And it is not a state a later event clears.

## `fn a_committed_line_that_is_not_an_event_is_a_rewritten_log…` › `let first = serde_json::to_string(&run_started_event()).expect("serialize");`

refusals[23], and the boundary it is distinguished from: the newline
is the commit marker, so an unterminated final line is a torn tail
and is dropped, while a terminated one that will not parse means the
log was rewritten.

## `fn a_committed_line_that_is_not_an_event_is_a_rewritten_log…` › `let torn = format!("{first}\n{second}");`

A torn tail: syntactically complete and never committed.

## `fn a_committed_line_that_is_not_an_event_is_a_rewritten_log…` › `for position in 0..3 {`

A committed line that is not an event, at every position.

## `fn a_committed_line_that_is_not_an_event_is_a_rewritten_log…` › `let mut bytes = format!("{first}\n").into_bytes();`

Invalid UTF-8 inside a committed line is the same situation.

## `fn a_committed_line_that_is_not_an_event_is_a_rewritten_log…` › `for (label, blank) in [`

A committed line that is *blank* is the same situation again, and
the one the refusal is easiest to lose: the newline is the commit
marker, so an empty or whitespace-only terminated line is a
committed record that is not an event. Skipping it would fold a log
whose physical shape no reader can account for — and would let a
rewrite that blanked a line read back as a shorter valid log.

## `fn push(live: &mut TopologyFold, trace: &mut Vec<TopologyEvent>, event: TopologyEvent) {`

Apply an event to a live fold and record it in the trace it came from.

## `fn long_trace() -> Vec<TopologyEvent> {`

A run that retries on a retained session, merges fast, verifies stale,
defers on an outage, wakes, is rejected into a repair, exceeds its
budget and resumes.

## `fn long_trace() -> Vec<TopologyEvent>` › `push(&mut live, &mut trace, dispatch(ALPHA, 0, &base));`

alpha: dispatched, retried on a retained session, then merged fast.

## `fn long_trace() -> Vec<TopologyEvent>` › `push(`

Attempt 2 is the one that succeeded, so it is the one the candidate
is attributed to.

## `fn long_trace() -> Vec<TopologyEvent>` › `push(&mut live, &mut trace, dispatch(ZETA, 0, &base));`

zeta: verified stale, deferred by an outage, woken, then rejected —
which registers a repair — and the repair is dispatched and parked.

## `fn settled_trace() -> Vec<TopologyEvent> {`

A run that is interrupted, closes a generation, merges, registers a
repair by hand, has a verification interrupted, and parks and answers a
question — the guarded kinds the long trace does not reach.

## `fn settled_trace() -> Vec<TopologyEvent>` › `push(&mut live, &mut trace, dispatch(ZETA, 0, &base));`

An interruption closes generation 0 and returns zeta to pending.

## `fn settled_trace() -> Vec<TopologyEvent>` › `push(&mut live, &mut trace, dispatch(ZETA, 1, &base));`

Generation 1 is dispatched and closed without an attempt.

## `fn settled_trace() -> Vec<TopologyEvent>` › `push(&mut live, &mut trace, dispatch(ALPHA, 0, &base));`

alpha merges fast, which gives a repair something to depend on.

## `fn settled_trace() -> Vec<TopologyEvent>` › `push(&mut live, &mut trace, dispatch(ZETA, 2, &base));`

zeta's third generation prepares a candidate whose verification is
interrupted.

## `fn settled_trace() -> Vec<TopologyEvent>` › `push(&mut live, &mut trace, raised("q-park-Ünicode", MID));`

And a question is asked about a third task and answered.

## `fn finished_trace() -> Vec<TopologyEvent> {`

Every task merged, and the run saying so.

## `fn live_and_replay_reach_the_same_state_over_a_long_trace()` › `for trace in [long_trace(), settled_trace(), finished_trace()] {`

INV-02, as the property rather than as the claim: a fold driven
event by event and a fold replayed from the same bytes hold the same
state — and the bytes are what a writer would have appended, so the
comparison is over a serialization round trip too.

## `fn live_and_replay_reach_the_same_state_over_a_long_trace()` › `let parsed = TopologyFold::parse_log(&wire(&trace)).expect("the log parses");`

Through the wire, not through the values: a replay reads bytes.

## `fn wire(trace: &[TopologyEvent]) -> Vec<u8> {`

Serialize a trace the way a writer would append it.

## `fn one_field_invalid(event: &TopologyEvent) -> Vec<(String, TopologyEvent)> {`

Copies of `event` with exactly one coordinate moved to a value the fold
must refuse *in this event's own position*.

One field at a time is the whole point: an event that disagreed with its
state in several places at once could be caught by any one of the
relations, and would not say which. Everything here is a relation the
fold owns — an identity, a count, a SHA, a disposition — never a shape
serialization would already have refused.

## `fn one_field_invalid(event: &TopologyEvent) -> Vec<(String,…` › `let mut moved = data.clone();`

The recorded region, moved one component. Before it was
derivation-checked this line was the whole finding: the fold
admitted on `predicted_region`'s answer and the lease table
kept the event's, so a hostile log carried one region past
the door and every later overlap check consulted the other.

## `fn one_field_invalid(event: &TopologyEvent) -> Vec<(String,…` › `let mut moved = data.clone();`

The Retained arm's own two cells. `long_trace` carries a
retained settlement, and until this arm bound the envelope to
the record a hostile log could move either past it.

## `fn one_field_invalid(event: &TopologyEvent) -> Vec<(String,…` › `let mut moved = data.clone();`

A retained record turned into one that claims the attempt
succeeded. Both arms refuse this shape; only the `Closed` one
did before, so a hostile log could carry it past the door on
the retained path.

## `fn one_field_invalid(event: &TopologyEvent) -> Vec<(String,…` › `let mut moved = data.clone();`

The configured passes, emptied. Every remaining entry is
green — there are none — so `is_successful` is true of this
record and only the frozen plan can tell it apart from a
reviewed one.

## `fn every_guarded_event_is_refused_the_same_way_live_and_on_…` › `let mut covered: BTreeSet<&'static str> = BTreeSet::new();`

INV-02: "Live state and replay use one checked transition over the
exact wire event; an invalid transition is never appended."

Equal *valid* traces cannot prove this: a replay that applied every
event unchecked, or that skipped the ones the checked transition
refused and carried on, reaches exactly the same state over a valid
log. The witness has to be a log a writer would never have produced —
a valid prefix, one event with one field moved, and a valid suffix —
and the claim is that replay stops on that line with the refusal the
live path gives, rather than reaching a state at all.

The expected refusal is taken from the live path over the same
prefix, which is the other half of the invariant and not the
function under test explaining itself: two independent entry points
are required to answer identically.

## `fn every_guarded_event_is_refused_the_same_way_live_and_on_…` › `let live_error = prefix`

Live: refused, and asking left the state exactly as it
was.

## `fn every_guarded_event_is_refused_the_same_way_live_and_on_…` › `let mut hostile = trace[..index].to_vec();`

Replay: the same refusal, over the wire, with a valid
suffix behind it that a lenient reader would have gone on
to apply.

## `fn every_guarded_event_is_refused_the_same_way_live_and_on_…` › `let unguarded: BTreeSet<&'static str> = [`

The sweep is over the vocabulary, not over what was remembered. The
three informational kinds are never refused, and `defer_wait_elapsed`
carries no field a fold relation reads — both are witnessed on their
own below.

## `fn every_guarded_event_is_refused_the_same_way_live_and_on_…` › `let mut live = started();`

`defer_wait_elapsed`'s guard is the state rather than a field
(refusals[18]: no wait elapses under a halt or the epoch's budget
stop), so its hostile witness is one appended where the prefix
forbids it.

## `fn a_delta_carries_the_exact_event_it_was_checked_against()` › `let base = sha("base");`

The emit contract is: build the event, round-trip it, plan the
transition, append *the exact bytes*, apply the delta. A delta whose
event is a rebuilt or normalized copy of the one it was asked about
would let a writer append one record and fold another — which is the
divergence between live state and replay that INV-02 forbids, in the
one place the two are not literally the same call.

## `fn a_refused_transition_changes_nothing()` › `let mut fold = started();`

The other half of INV-02: an invalid transition is never applied,
which is a property of `plan_transition` being a question rather
than an action.

## `fn the_registry_digest_does_not_widen_when_a_repair_is_regi…` › `let mut fold = started();`

The authentication value is over the *originals*: a reader rebuilds
them from the frozen plan and the run record and compares. A dynamic
entry has no frozen input behind it to rebuild from, so a digest that
grew with one would be a value no reader could recompute.

## `fn the_registry_digest_does_not_widen_when_a_repair_is_regi…` › `let bytes = registry.canonical_bytes();`

The other half, and the one that has no producer yet: the canonical
serialization is of the *registry*, so it covers every constructible
entry. The digest is narrow because a reader rebuilds only the
originals; the encoding is not, because a dynamic entry no encoder
ever visits is a value nothing downstream can compare — which is how
a stored entry can differ from the event that registered it and
nobody notices.

## `fn the_registry_digest_does_not_widen_when_a_repair_is_regi…` › `let text = String::from_utf8_lossy(&bytes).into_owned();`

Its own fields are in there, including the allow-list, which is the
field a derivation could quietly substitute for.

## `fn the_registry_digest_does_not_widen_when_a_repair_is_regi…` › `assert_eq!(`

And the stored entry is the entry the event registered, field for
field — not one derived from its ladder rungs or its admission
options. Nothing else in this slice reads a dynamic entry back.

## `fn the_registry_digest_does_not_widen_when_a_repair_is_regi…` › `assert_eq!(`

And the repair is addressable by both of its identities.

## `fn prefixes(paths: &[&str]) -> PathSet {`

-----------------------------------------------------------------------
Regions, holdings and queue eligibility
-----------------------------------------------------------------------

## `fn regions_overlap_component_wise_and_repo_wide_overlaps_ev…` › `for (left, right, overlaps) in [`

Equal, ancestor and descendant overlap; a byte prefix that is not a
component prefix does not. `src/foo` and `src/foobar` are the case
that separates a component comparison from a `starts_with`.

## `fn regions_overlap_component_wise_and_repo_wide_overlaps_ev…` › `for (left, right) in [("src/Zebra", "src/zebra"), ("src/ÜBER", "src/über")] {`

Case folding is the run's, resolved once, and it folds beyond ASCII:
a case-folding filesystem folds `Ü` the same way it folds `U`.

## `fn regions_overlap_component_wise_and_repo_wide_overlaps_ev…` › `for other in [PathSet::RepoWide, prefixes(&[]), prefixes(&["src/foo"])] {`

Repo-wide overlaps everything, including the empty region — the
asymmetry the variant exists for.

## `fn regions_overlap_component_wise_and_repo_wide_overlaps_ev…` › `assert!(!regions_overlap(`

And an empty region overlaps nothing else: a diff that touched
nothing is not a diff that touched everything.

## `fn regions_overlap_component_wise_and_repo_wide_overlaps_ev…` › `assert!(regions_overlap(`

A set overlaps when any member does, not only the first.

## `fn an_ordinary_candidate_waits_for_any_lineage_and_a_member…` › `let policy = path_policy();`

`decisions.coordinator_integration.queue`, as the relation it is: a
lineage holds the region a rejection made contentious, so ordinary
work stays out of it entirely, and two lineages contending for one
region resolve by age rather than taking turns blocking each other.

## `fn an_ordinary_candidate_waits_for_any_lineage_and_a_member…` › `assert_eq!(`

Parking and deferral outrank both, and are distinguished from each
other so a queue that reported one for the other is visible.

## `fn an_ordinary_candidate_waits_for_any_lineage_and_a_member…` › `let elsewhere = QueueEntry {`

A region nobody holds is eligible whatever the lineages are.

## `fn a_lineage_lease_only_ever_grows_and_a_released_one_is_go…` › `leases.widen_lineage(ZETA, &PathSet::RepoWide);`

Repo-wide absorbs: a region nobody could read stays unbounded.

## `fn a_lineage_lease_only_ever_grows_and_a_released_one_is_go…` › `let mut table = LeaseTable::new();`

A holding belongs to its owner: the same region held by somebody
else is a collision, and held by yourself is not.

## `fn a_lineage_lease_only_ever_grows_and_a_released_one_is_go…` › `table.release(LeaseOwner::Lineage { root: MID });`

Releasing what nobody holds is a statement, not an operation.

## `fn a_generations_holding_decides_the_disposition_its_settle…` › `for (lease, survives, expected) in [`

The relation refusals[14] is checked against, stated on its own: two
holdings, two fates, and exactly one disposition per cell.

## `fn a_predicted_region_is_the_literal_prefix_of_every_hint()` › `let registry = started().registry().expect("started").clone();`

`admission_and_leases.path_policy.prediction`: the literal prefix
before the first glob metacharacter, and repo-wide for anything
unsafe or absent — the classification that costs parallelism and
never costs correctness.

## `fn a_predicted_region_is_the_literal_prefix_of_every_hint()` › `let mut hintless = zeta.clone();`

Absent, and unsafe, both classify repo-wide.

## `fn a_predicted_region_is_the_literal_prefix_of_every_hint()` › `let mut windows = zeta.clone();`

A backslash-separated hint is a Windows spelling of the same region,
not a one-component path with a backslash in its name.

## `fn the_pipeline_entitlement_is_what_the_fold_derives_it_to_…` › `let base = sha("base");`

`admission_and_leases.permits.pipeline`: held by generations that are
open with no attempt, in flight, or promoting, plus one for an
unresolved integration transaction — and by nothing else. Retained
and closed generations hold none, and neither does a queued
candidate.

## `fn a_run_reaches_complete_only_when_every_task_has_settled()` › `let mut fold = started();`

The end-to-end shape, driven by events rather than by writing state:
three tasks merged over the fast path, and the outcome moving from
NotEnding to Complete exactly at the last one.

## `fn halt_and_budget_outrank_every_structural_source_that_can…` › `let base = sha("base");`

`run_end_policy.derived_outcome`'s precedence, source by source:
"if not common -> NotEnding; else if halting -> Halted; else if
budget -> BudgetExceeded; else if structurally_admissible or
backoff_pending -> NotEnding". A singleton example cannot reveal an
order, so each structural source is isolated and then crossed with a
halt and with the epoch's budget stop.

## `fn halt_and_budget_outrank_every_structural_source_that_can…` › `let ready_state = || started();`

Source 1: a dispatchable task. A fresh run has exactly one — alpha
depends on nothing; zeta and mid wait on it — and an empty queue, so
`ready` is the only source alight.

## `fn halt_and_budget_outrank_every_structural_source_that_can…` › `let integration_state = || {`

Source 2: an eligible queued candidate and nothing dispatchable.
alpha is failed so no task is ready, and the two prepared candidates
are eligible, so `integration_admissible` is the only source alight.

## `fn halt_and_budget_outrank_every_structural_source_that_can…` › `let mut retained = started();`

Source 3, and why it can never be crossed with either: a retry is
admissible only while a RetainedIdle generation is open, and an open
generation of any class makes `common` false, which outranks
everything. The state is recorded here rather than argued, because
"unreachable" is the kind of claim that stops being true quietly.

## `fn complete_refuses_each_residue_it_leaves_behind_one_at_a_…` › `let terminal = || {`

The Complete arm's conjuncts past the task predicate: "the queue is
empty (no R6 open), and no candidate or lineage lease is active
(R7/R8 none)". Every task is held terminal throughout, so each
residue is the only thing between this state and Complete and a
conjunct that was dropped shows up as Complete rather than as a
different refusal.

## `fn complete_refuses_each_residue_it_leaves_behind_one_at_a_…` › `let mut generation_only = terminal();`

A generation lease is not one of the two: an ordinary generation's
predicted region is released when the generation closes, and the
Complete arm names the candidate and lineage holdings only.

## `fn backoff_is_what_is_waiting_now_and_not_what_once_waited()` › `let head = sha("head");`

`backoff_pending` is "any task is Deferred or any candidate is
verification_deferred (both are woken only by the durable
defer_wait_elapsed or run_resumed)". The historical defer *count* is
kept for the consecutiveness rule and is not a waiting state, so a
candidate that has deferred once and been woken does not block a
closure. The two stay correlated unless a fixture separates them.

## `fn backoff_is_what_is_waiting_now_and_not_what_once_waited()` › `let entry = &fold.queue().expect("started").entries()[0];`

Woken: the flag is clear and the history is not.

## `fn backoff_is_what_is_waiting_now_and_not_what_once_waited()` › `let woken = fold.clone();`

Settle everything around it, so the only thing that could still make
this run NotEnding is that retained count.

## `fn backoff_is_what_is_waiting_now_and_not_what_once_waited()` › `let mut with_entry = fold.clone();`

And with the same entry still queued but *not* waiting, the queue
conjunct is what stops it — not the count.

## `fn backoff_is_what_is_waiting_now_and_not_what_once_waited()` › `let mut parked = woken;`

The state where the two readings disagree about an *outcome* rather
than about a reason: a parked verification. The candidate stays
queued and ineligible with its history intact and its flag clear,
the task is AwaitingInput, and `derived_outcome` is Parked — which
`backoff_pending` outranks. A fold that read the retained count as a
waiting state answers NotEnding here and refuses the closure the
packet requires.

## `fn backoff_is_what_is_waiting_now_and_not_what_once_waited()` › `let mut run = parked.run.take().expect("started");`

Silence the other structural sources so the Parked arm is what is
being read: alpha is terminal, and zeta's candidate leaves the queue
with the holding it took.

## `fn a_failure_blocks_the_whole_dependency_closure_and_not_on…` › `let base = sha("base");`

`run_end_policy.derived_outcome`: Complete requires "every task is
Merged, Failed, or Pending with a Failed task in its **transitive**
dependency closure (derived Blocked)".

The 1008-cell grid cannot prove this, because its BlockedByFailure
fixture makes every pending task depend on the failed one directly:
there, "directly failed dependency" and "failed anywhere in the
closure" are the same predicate. Here they are not. `cee` fails,
`bee` depends on `cee` and is blocked directly, and `aay` depends
only on `bee` and is blocked by two hops and by nothing else. A
derivation that recognized only a directly failed dependency leaves
`aay` Pending-and-unblocked, so no arm of the total function claims
the state and it lands on FoldError.

## `fn a_failure_blocks_the_whole_dependency_closure_and_not_on…` › `let registry = live.registry().expect("started");`

The dependency shape is the fixture's, read back rather than assumed.

## `fn a_failure_blocks_the_whole_dependency_closure_and_not_on…` › `assert_eq!(live.task_state(CEE), Some(TaskState::Failed));`

Nothing else can move: `bee` waits on a task that failed and `aay`
waits on `bee`. Every Complete conjunct holds.

## `fn a_failure_blocks_the_whole_dependency_closure_and_not_on…` › `push(`

Live and replay, through the wire, reach the same verdict — and the
run may say so.

## `fn a_failure_blocks_the_whole_dependency_closure_and_not_on…` › `let prefix = TopologyFold::replay(chain_inputs(), &trace[..1]).expect("the prefix replays…`

And the direction that says the predicate is not vacuous: with `cee`
still Pending rather than Failed, nothing is Blocked, `cee` is
admissible, and the run is not ending.

## `an_exhausted_generation_attempt_counter_is_refused_without_panicking` › `let run = fold.run.as_mut().expect("the checked prefix started a run");`

Construct only the numeric boundary after a checked retained prefix.
This is not a claim that a billions-record history was replayed.

## `an_exhausted_generation_attempt_counter_is_refused_without_panicking` › `let before = fold.state().cloned();`

The independent snapshot witnesses that refusing the retry changes no state.

## Question guard regression traces from PR #153

The four open-generation classes refuse a bare question with identical live and replay errors. The unrelated-task control also proves refusal does not consume the question ID. Each case clones its event prefix to replay it independently; the refusal helper owns a state snapshot and replay log for comparison.

Terminal tasks refuse new questions. Quiet parked tasks and repair lineages permit multiple questions; answering one preserves any remaining question and restores the state implied by the candidate or lineage. A queued candidate returns to AwaitingMerge and cannot dispatch another generation.

The repair-member trace checks both an ordinary answer and a nonhalting decline, live and by replay. Decline fails both unmerged members and avoids FoldError; this is fold state evidence, not proof of operating-system process termination. The attempt settlement trace separately checks a halting decline, released lease, and accepted run_finished. These tests use the broader question contract implemented by PR #152; the earlier blanket lineage and answer-state restrictions are superseded.
