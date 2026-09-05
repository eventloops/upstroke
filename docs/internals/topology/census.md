# `src/topology/census.rs`

Extended notes for [`src/topology/census.rs`](../../../src/topology/census.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The bounded reachability census (ST-14), as a skeleton.

`decisions.bounded_census` asks for an executable breadth-first exploration
of abstract fold states: at every state, every event class is offered to the
real [`TopologyFold::plan_transition`], and each offer is either refused or
yields a next state. Nothing is simulated — the transition function under
test is the one a live run and a replay both use.

### What a census is evidence for, and what it is not

Bounded evidence for the stated bounds. It is not closure of the unbounded
system, and it proves nothing at all about effect phases — those are the
typed site registry's business ([`crate::topology::effects`]). What it *can*
settle is the shape of the transition table: that no `(state, event class)`
pair is unmapped, that a replay of every explored trace reaches the state
the exploration reached, and that [`TopologyFold::derived_outcome`] is total
— `NotEnding` or exactly one outcome at every state, with the
[`DerivedOutcome::FoldError`] arm never reached.

That last one is why the arm is a value rather than a `panic!`: "this is
unreachable" is a claim, and a claim wants a census rather than an
assertion.

### Skeleton

This slice ships the explorer, the bounds, the recording, and the totality
assertions over fixtures. PR10 raises the fixtures to the packet's full
bounds and adds the per-arm coverage assertion. What is deliberately *not*
claimed here is stated by [`Census::truncated`] and by this module's tests:
a census that stopped early says so rather than reporting the states it did
reach as if they were all of them.

## `pub struct CensusBounds {`

The exploration bounds (`decisions.bounded_census.bounds`).

Recorded as data rather than as constants so a census can say which bounds
it ran under, and so the two numbers that actually stop the search —
[`Self::max_trace`] and [`Self::max_states`] — are visible beside the
design's own bounds instead of buried in the loop.

## `pub struct CensusBounds` › `pub originals: u32,`

Original tasks in the plan.

## `pub struct CensusBounds` › `pub repairs: u32,`

Repairs, and therefore lineages.

## `pub struct CensusBounds` › `pub generations_per_task: u32,`

Generations per task.

## `pub struct CensusBounds` › `pub attempts_per_generation: u32,`

Attempts per generation.

## `pub struct CensusBounds` › `pub sequences: u32,`

Integration sequences.

## `pub struct CensusBounds` › `pub defers: u32,`

Verification defers per candidate — the fixture's `max_defers`.

## `pub struct CensusBounds` › `pub questions: u32,`

Open questions.

## `pub struct CensusBounds` › `pub resumes: u32,`

Resumes.

## `pub struct CensusBounds` › `pub max_trace: usize,`

The longest trace the search will extend.

## `pub struct CensusBounds` › `pub max_states: usize,`

The most states the search will record before stopping.

## `impl CensusBounds` › `pub const fn dimensions(&self) -> [(&'static str, u32); 8] {`

Every dimension of the *explored space* this census declares, as
`(name, bound)`.

The two search limits are deliberately absent: [`Self::max_trace`] and
[`Self::max_states`] bound the search, not the space, and a census that
reached neither of them has still only generated whatever its fixture
generates. This list exists so "every declared dimension" is something
a test can quantify over rather than a list someone retypes — a bound
that is declared here and never generated is a boundary the skeleton
would otherwise report without evidence.

## `impl Default for CensusBounds` › `fn default() -> Self {`

The packet's bounds, with the two search limits set wide enough that
the fixtures below reach their terminals and narrow enough that the
search finishes in a unit test.

## `pub struct Candidate {`

One event class offered at a state.

## `pub struct Candidate` › `pub label: String,`

The class's name, used as its identity in the coverage record.

## `pub struct Candidate` › `pub event: TopologyEvent,`

The event the class produces at this state.

## `impl Candidate` › `pub fn new(label: impl Into<String>, event: TopologyEvent) -> Self {`

A candidate with a label.

## `pub enum TransitionOutcome {`

What happened when a class was offered.

## `pub enum TransitionOutcome` › `Accepted {`

The fold accepted it; this is the state it reached.

## `pub enum TransitionOutcome` › `to: usize,`

The state's id.

## `pub enum TransitionOutcome` › `Refused {`

The fold refused it, and said why.

## `pub enum TransitionOutcome` › `reason: String,`

The refusal, rendered.

## `pub enum TransitionOutcome` › `Truncated,`

The fold accepted it and the state it reached was new, but the search
had already recorded [`CensusBounds::max_states`] states.

Recorded rather than dropped: an offer that vanished because the search
was full is exactly the kind of silent cap that makes a coverage report
read as complete when it is not.

## `pub struct CensusTransition {`

One `(state, event class)` pair and its answer.

Every offer is recorded, accepted or refused. "No `(state, event class)`
pair is unmapped" is then a property of this list rather than of the
explorer's control flow: an offer that produced neither would be an offer
that is not here.

## `pub struct CensusTransition` › `pub from: usize,`

The state the class was offered at.

## `pub struct CensusTransition` › `pub label: String,`

The class.

## `pub struct CensusTransition` › `pub outcome: TransitionOutcome,`

The answer.

## `pub struct CensusState {`

One explored abstract state.

## `pub struct CensusState` › `pub id: usize,`

The state's id, in discovery order.

## `pub struct CensusState` › `pub trace: Vec<TopologyEvent>,`

A shortest trace that reaches it.

## `pub struct CensusState` › `pub fold: TopologyFold,`

The fold at that state.

## `pub struct CensusState` › `pub outcome: DerivedOutcome,`

What the total outcome function says here.

## `pub struct Census {`

A completed exploration.

## `impl Census` › `pub fn explore<F>(`

Explore breadth-first from `start`, offering every class `classes`
produces at every state.

States are identified by the fold state they hold, not by the trace that
reached it: two different histories that leave the run in the same state
are one state, which is what makes the search finite and what makes
"reachable" mean something.

## `impl Census` › `pub fn bounds(&self) -> CensusBounds {`

The bounds this census ran under.

## `impl Census` › `pub fn states(&self) -> &[CensusState] {`

Every state reached.

## `impl Census` › `pub fn transitions(&self) -> &[CensusTransition] {`

Every `(state, class)` offer and its answer.

## `impl Census` › `pub fn truncated(&self) -> bool {`

Whether the search stopped at [`CensusBounds::max_states`] rather than
because it ran out of new states.

A truncated census has explored a *subset*, and every assertion over it
is an assertion about that subset. Reported rather than inferred,
because a coverage claim over a silently truncated search reads exactly
like a coverage claim over a complete one.

## `impl Census` › `pub fn outgoing(&self, id: usize) -> impl Iterator<Item = &CensusTransition> {`

The offers made at one state.

## `impl Census` › `pub fn has_legal_transition(&self, id: usize) -> bool {`

Whether any class is accepted **at this state**.

Two halves, and both are promises rather than incidental properties of
the expression below, so both are asserted by
`has_legal_transition_is_local_to_the_state_and_excludes_refusals`
against censuses built to make each fail on its own:

* *local* — a state with no accepted offer of its own answers `false`
  even when some other state has one. A global existential would read
  as an answer about a dead end and be an answer about the census.
* *accepted only* — a refusal is not a transition. `plan_transition`
  returning `Err` normally is the fold working, not the run moving, and
  an out-of-range id has no offers and so answers `false`.

## `impl Census` › `pub fn accepted_labels(&self) -> BTreeSet<&str> {`

Every class that was accepted somewhere.

## `impl Census` › `pub fn refused_labels(&self) -> BTreeSet<&str> {`

Every class that was refused somewhere.

## `impl Census` › `pub fn states_with(&self, outcome: &DerivedOutcome) -> Vec<&CensusState> {`

Every state whose outcome is this one.

## `impl Census` › `pub fn totality_audit(&self) -> TotalityAudit {`

Re-evaluate [`TopologyFold::derived_outcome`] once at every explored
state and report what it found, raw.

See [`TotalityAudit`] for why the totality assertion runs over this
rather than over [`CensusState::outcome`].

## `pub struct TotalityAudit {`

One raw [`TopologyFold::derived_outcome`] evaluation per state, and what it
disagreed with.

### Why this exists rather than a loop over `state.outcome`

[`CensusState::outcome`] is written by [`Census::explore`], which is also
the thing the totality assertion is evidence about. A checker whose input
is chosen by what it checks establishes nothing: an explorer that dropped a
[`DerivedOutcome::FoldError`] successor before recording it, recorded it as
a refusal, or wrote `NotEnding` in its place would leave every such loop
green. This audit closes three of those four doors and names the fourth:

* *normalising* — every state's recorded outcome is compared with a fresh
  evaluation of the very same fold, and a disagreement is reported by id
  rather than silently preferred one way or the other.
* *filtering* — [`Self::fold_errors`] counts an id when **either** side is
  `FoldError`, so a checker cannot arrive at zero by discarding one side.
* *skipping* — [`Self::evaluated`] holds one entry per state in the order
  they were given, so a caller can require it to equal the explored id set
  computed from somewhere else (the accepted transitions, say) instead of
  from the same list it is auditing.
* *dropping the successor entirely* — outside this type's reach, because a
  state that was never recorded is not in `states`. That is what
  `the_census_transition_table_is_reproducible_from_the_folds_alone`
  settles, by re-deriving every offer from the folds and requiring each
  accepted one to land on a recorded state with the same fingerprint.

## `pub struct TotalityAudit` › `pub evaluated: Vec<usize>,`

The ids evaluated, one entry per state, in the order given.

## `pub struct TotalityAudit` › `pub fold_errors: Vec<usize>,`

The ids where the recorded outcome or the fresh evaluation — either —
is [`DerivedOutcome::FoldError`].

## `pub struct TotalityAudit` › `pub disagreements: Vec<usize>,`

The ids where the recorded outcome and the fresh evaluation disagree.

## `pub struct TotalityAudit` › `pub not_ending: usize,`

How many fresh evaluations answered [`DerivedOutcome::NotEnding`].

## `pub struct TotalityAudit` › `pub ending: usize,`

How many fresh evaluations answered [`DerivedOutcome::Ending`].

## `impl TotalityAudit` › `pub fn over(states: &[CensusState]) -> Self {`

Audit an arbitrary list of states.

Takes the slice rather than the [`Census`] so that the checker itself
can be given a list containing a `FoldError` and shown to report it. A
negative control that cannot be constructed is not a control.

## `fn fingerprint(fold: &TopologyFold) -> String {`

The abstraction: the fold state, rendered.

`decisions.bounded_census.abstraction` asks for concrete state with commit
SHAs replaced by symbolic labels, paths replaced by regions, and timestamps
dropped. Timestamps are dropped by construction — they live on the event
envelope and never enter the fold — and the fixtures are written in symbolic
SHAs and named regions already, so the abstraction of a fixture state is the
state. Rendering it is then a faithful key: two states with the same
rendering agree on every relation `plan_transition` reads, because every one
of those relations is a field of what is rendered.

### The obligation this key carries, and why a paragraph is not it

The argument above is an argument about *this* body. Any projection of the
state — dropping the lease regions, collapsing `verification_deferred` to a
count, rendering the transaction without its `expected_head` — keeps the
function compiling, keeps it deterministic, and keeps every existing
assertion green, because two states that alias here are *one* state
everywhere downstream: the second is never recorded, so nothing later can
notice it is missing. A weakened key cannot be caught by looking at what
the census explored.

It is caught by looking at what the census *distinguishes*. Every relation
`decisions.bounded_census.abstraction` names as retained has a witness pair
in `the_abstraction_key_separates_states_that_differ_in_one_retained_relation`:
two reachable folds whose traces are identical but for one event, differing
in one field of that event — or, where the fold refuses a log that records
one symbolic label in one place and a different one elsewhere, differing in
that one label throughout — required to have different fingerprints. Path
regions get a second witness in
`an_overlapping_region_is_explored_and_changes_a_transition_answer`, where
A and AB are separate explored states whose answers to the same offer
differ — an overlap the key forgot would make that pair one state and the
differing answer unreachable.

## `mod tests` › `fn sha(label: &str) -> CommitSha {`

-----------------------------------------------------------------------
Fixtures

Written for this module rather than shared with the fold's own tests: a
census that explored the same fixture the transition table was built
against would agree with it about a shape neither had questioned.

Symbolic SHAs and two disjoint regions, so the state rendering that
identifies a census state is already the abstraction the design asks
for. Every independently meaningful field takes a value of its own —
`the_fixture_varies_every_field_a_relation_reads` counts them.
-----------------------------------------------------------------------

## `mod tests` › `fn sha(label: &str) -> CommitSha {`

A 40-character symbolic sha, one per role.

## `mod tests` › `fn plan() -> Plan {`

Two independent tasks over two disjoint regions, so the queue can hold
two candidates at once and a lease has something to be wrong about.

## `fn run_started_unauthenticated() -> RunStarted4` › `limits: TopologyLimits {`

Three different numbers, so a fold that read one limit where it
meant another lands on a value this fixture does not hold.

## `fn run_started_unauthenticated() -> RunStarted4` › `enabled: Some(true),`

**Enabled, because the fixture's attempt records are
reviewed.** This froze verification *off* while every
`candidate_prepared` the census builds carries a passed
`review` — a combination production never produces
(`plan_for`'s disabled branch resolves no `primary` at all)
and one `check_candidate_prepared` now refuses, because a run
that judged nothing obliges no pass and a record that names
one is claiming a review the run never ran.

## `mod tests` › `fn started() -> TopologyFold {`

A fold that has recorded its `run_started` and nothing else.

## `mod tests` › `fn region(key: TaskKey) -> PathSet {`

Region A or B — the region the entry's frozen hint **derives**, not the
hint.

The hints are `src/aleph/` and `src/bet/`, and the derivation trims the
trailing separator, so the literal here carries no slash. It used to
carry one, which made every `task_dispatched` this fixture built record
a region the fold does not derive — refused by `check_dispatched` since
the region became derivation-checked, and invisible before that because
the two spellings name the same components to
[`crate::topology::leases::paths_overlap`]. The round trip is
[`the_fixture_region_is_the_one_the_fold_derives`], so the two cannot
drift apart again silently.

## `mod tests` › `fn overlap_region() -> PathSet {`

Region AB: the union of the two disjoint regions, so it overlaps each
of them and neither of them contains it.

`decisions.bounded_census.abstraction` names "paths replaced by regions
A, B, AB" — three labels, not two — and AB is the only one of the three
under which the overlap relation answers differently from the others.

## `fn attempt_record(attempt: u32) -> AttemptRecord` › `reviews: vec![ReviewRecord {`

The primary pass §11.2 requires, present and passed. Empty
`reviews` satisfies `is_successful` vacuously — the premise then
exercises none of the clause it is the positive witness for.

## `mod tests` › `fn dispatch_over(key: TaskKey, generation: u32, paths: PathSet) -> TopologyEvent {`

The same dispatch with its predicted region named, so a fixture can
vary the one field the overlap relation reads and vary nothing else.

## `mod tests` › `fn dispatch_at(`

The same dispatch cut from a named base.

The base a generation is dispatched at is the base its candidate must
record — `check_candidate_prepared` refuses a record that disagrees
with it — and it is the left operand of the fast publication's head
relation. So a witness that varies the candidate's base varies this
event and the prepared record together, and can vary nothing less.

## `mod tests` › `let mut record = attempt_record(attempt);`

The record says failed, because every settlement this
builds is one: `candidate_prepared` is the sole successful
settlement and `check_attempt_finished` refuses a failure
whose record reports success.

## `mod tests` › `fn candidate_prepared_over(`

The same record with the region its diff touched named. `actual_paths`
and the lease it replaces the prediction with are one region by
`check_candidate_prepared`'s own rule — "the region it takes is not the
region its diff touched" — so one parameter is the honest shape.

## `mod tests` › `fn candidate_prepared_at(`

The same record at a named base and a named commit.

The two labels the fast publication relations compare a candidate
against, as parameters. `parent_sha` moves with `base_sha` because
`parent_is_base` requires it, which is why the base is one parameter
and not two.

## `mod tests` › `fn candidate_at(key: TaskKey, generation: u32, commit: CommitSha) -> CandidateRef {`

The same candidate at a named commit. Its commit is part of its
identity, so every later record that names it carries the same label or
the fold refuses the log.

## `mod tests` › `fn merge_prepared_for(`

The same publication naming its candidate outright, so a witness whose
whole point is a candidate at a label the fixture does not otherwise
derive can still be published.

## `mod tests` › `fn task_merged(`

The merge that resolves an open publication.

`merged_sha` and `satisfies` are read off the transaction the fold is
holding, because a merge is the ref move a publication already
authorized: a class that invented either would only ever be refused,
and the census would then never resolve a transaction.

## `mod tests` › `fn classes(fold: &TopologyFold) -> Vec<Candidate> {`

-----------------------------------------------------------------------
The event classes
-----------------------------------------------------------------------

## `mod tests` › `fn classes(fold: &TopologyFold) -> Vec<Candidate> {`

Every event class the census offers, at every state.

Parameters range over the bounded identities exactly as
`event_payload_classes` asks: both tasks, both generations, both
attempts, the settlement transitions, the three publication
dispositions each in a matching and a mismatching shape, the budget
stop, the backoff wake, and `run_finished` for each of the four
outcomes.

## `fn classes(fold: &TopologyFold) -> Vec<Candidate>` › `out.push(Candidate::new(`

The fast relation, matching and each way of missing it.

## `fn classes(fold: &TopologyFold) -> Vec<Candidate>` › `out.push(Candidate::new(`

A stale verification, then the stale_clean relation both
ways, and already_present both ways.

## `mod tests` › `fn census() -> &'static Census {`

The one exploration every assertion below runs over.

Memoised rather than re-explored per test: the search is deterministic
and the value is shared behind `&`, so a second run would be the same
bytes at the price of another quarter of a million `plan_transition`
calls. Sharing it is what makes the independent re-derivation in
`the_census_transition_table_is_reproducible_from_the_folds_alone`
affordable.

## `mod tests` › `fn common(fold: &TopologyFold) -> bool {`

-----------------------------------------------------------------------
The independent oracle

Every expectation below is computed from the *dimension tuple* by
`run_end_policy.derived_outcome`'s own chain — not by calling
`derived_outcome`, and not from a constant the fold also reads.
-----------------------------------------------------------------------

## `mod tests` › `fn common(fold: &TopologyFold) -> bool {`

`common`: no generation in {OpenNoAttempt, InFlight, Promoting,
RetainedIdle} and no unresolved integration transaction.

## `mod tests` › `fn backoff_pending(fold: &TopologyFold) -> bool {`

`backoff_pending`: any task Deferred or any candidate
verification-deferred.

## `mod tests` › `fn questions_open(fold: &TopologyFold) -> bool {`

`questions_open`: any open question.

## `mod tests` › `fn complete_shape(fold: &TopologyFold) -> bool {`

The Complete arm's own condition, read off the durable state.

## `fn complete_shape(fold: &TopologyFold) -> bool` › `let every_task_terminal = [ALEPH, BET].iter().all(|key| {`

"every task is Merged, Failed, or Pending with a Failed task in its
transitive dependency closure". The fixture plan has no dependencies,
so no Pending task is ever derived-Blocked and the arm reduces to
Merged-or-Failed here. PR10's fixtures carry the dependency shapes.

## `fn the_derived_outcome_is_total_over_every_explored_state()` › `let census = census();`

ST-14's headline: `NotEnding` or exactly one outcome at every
explored state, and the `FoldError` arm never reached.

Run through `TotalityAudit` rather than over `state.outcome`,
because `state.outcome` is written by the explorer this assertion is
evidence about: see that type's own documentation, and
`the_totality_audit_reports_a_fold_error_a_normalisation_and_a_short_domain`
for the three failures it is shown reporting.

## `fn the_derived_outcome_is_total_over_every_explored_state()` › `let reached: BTreeSet<usize> =`

The domain first, and named from somewhere other than the list being
audited: the seed, plus every state some accepted offer landed on.

## `fn the_derived_outcome_is_total_over_every_explored_state()` › `assert!(`

The arm the design argues is unreachable, counted from the recorded
value and from a fresh evaluation of the same fold alike.

## `fn the_derived_outcome_is_total_over_every_explored_state()` › `assert!(not_ending > 0 && ending > 0, "{not_ending}/{ending}");`

Both answers occur, so totality is a statement about a range rather
than about a constant.

## `fn the_derived_outcome_is_total_over_every_explored_state()` › `for state in census.states() {`

And each answer is the one the dimension tuple implies, by the
packet's precedence chain rather than by the function under test.

## `fn the_derived_outcome_is_total_over_every_explored_state()` › `match &state.outcome {`

The necessary conditions of the two arms the oracle above does
not compute forwards, asserted backwards.

## `fn a_state_with_admissible_work_and_no_budget_exceeded_clas…` › `let census = census();`

The pre-`budget_exceeded` counterexample the packet names: a run
with structurally admissible work and no budget record is NotEnding
*whatever the unmodeled spend*, and BudgetExceeded only after the
record exists.

## `fn a_state_with_admissible_work_and_no_budget_exceeded_clas…` › `let admissible_work = [ALEPH, BET].iter().any(|key| {`

The prefix itself: a dispatched, un-attempted generation is
admissible work, and there is no budget record.

## `fn a_state_with_admissible_work_and_no_budget_exceeded_clas…` › `assert!(before > 0, "no pre-budget_exceeded prefix was explored");`

Both halves of the counterexample are populated, so neither branch
above is vacuous.

## `fn every_deferred_state_has_a_legal_next_transition()` › `let census = census();`

`coverage_assertions`: "every state with a Deferred task or
verification-deferred candidate has at least one legal next
transition (defer_wait_elapsed when neither halting nor
budget-stopped; otherwise a closure transition or
run_finished(Halted | BudgetExceeded))".

*Every* state, and a state the search recorded and declined to
extend is a state it recorded — unexpanded is not unreached. So the
condition is evaluated where the packet states it, against the fold,
rather than read off the offers the explorer happened to write down.

The recorded table is evidence for a different claim: that the
answers the explorer wrote are the answers the fold gives. That is
asserted too, below the ceiling where there are any, and asserted as
agreement with the evaluation rather than in place of it.

## `fn every_deferred_state_has_a_legal_next_transition()` › `if !backoff_pending(&state.fold) || state.fold.finished().is_some() {`

A run that has appended its `run_finished` has ended; its
deferred items are void with it (Halted) or resumably open
(BudgetExceeded), and neither is a transition this log can still
make. The assertion is about live states.

## `fn every_deferred_state_has_a_legal_next_transition()` › `let accepted: BTreeSet<String> = classes(&state.fold)`

The semantic answer, evaluated against the fold: every class the
census was explored with, offered to the real `plan_transition`
at this state. No successor is built, nothing is recorded, and
the search's ceiling is untouched — so a state the search
declined to extend is still a state this condition can be asked
of.

## `fn every_deferred_state_has_a_legal_next_transition()` › `assert_eq!(`

Nothing is recorded here, which is what makes the evaluation
above the only evidence rather than a second reading of the
explorer's own record. Pinned rather than assumed: were a
ceiling state extended after all, this branch would be
asserting about a state the other branch already covers.

## `fn every_deferred_state_has_a_legal_next_transition()` › `let recorded: BTreeSet<String> = census`

Below the ceiling the explorer did record answers, and they
are the fold's — label for label. Asserted so the evaluation
above cannot be a weaker oracle quietly standing in for the
recorded one.

## `fn every_deferred_state_has_a_legal_next_transition()` › `assert_eq!(`

And the accessor over that record. Both halves of what
`has_legal_transition` promises — that it is about *this*
state and about acceptance rather than about
`plan_transition` returning at all — are pinned by
`has_legal_transition_is_local_to_the_state_and_excludes_refusals`,
which is what makes this line depend on a tested predicate
rather than on a second reading of the same record.

## `fn every_deferred_state_has_a_legal_next_transition()` › `assert!(`

`precedence_consequences`: after a halting settlement or
budget_exceeded, no defer_wait_elapsed is appended — halt and
budget outrank backoff. What remains is the closure
procedure: drain the in-flight settlements, complete the
owed promotions and publications, close the open
generations, and end.

## `fn every_deferred_state_has_a_legal_next_transition()` › `assert!(`

PR3-ST14-006: the ceiling branch is the one this assertion used to
skip, and a branch that runs over nothing asserts nothing. Both arms
of the packet's condition are required to reach it, so neither is
carried by the states below the ceiling alone.

## `fn every_deferred_state_has_a_legal_next_transition()` › `assert!(`

The packet's condition names two classes — "a Deferred task **or**
verification-deferred candidate" — and this fixture's class set
offers no `merge_verification_unavailable`, so the only one it can
reach is the first. Stated as an assertion rather than left to be
discovered, and answered beside it: the second class is reached by
`a_verification_deferred_candidate_is_a_deferred_state_with_a_way_out`.

## `mod tests` › `fn deferral_classes(fold: &TopologyFold) -> Vec<Candidate> {`

The classes the verification-deferral census offers.

## `fn a_verification_deferred_candidate_is_a_deferred_state_wi…` › `let census = Census::explore(`

The other half of `coverage_assertions`' deferred-state condition.
`every_deferred_state_has_a_legal_next_transition` runs over a
fixture whose only deferred item is a Deferred *task*, so the
verification-deferred candidate — a different field, on a different
record, cleared by a different rule — gets its own census rather
than a claim that the first one covered it.

## `fn a_verification_deferred_candidate_is_a_deferred_state_wi…` › `assert!(state.fold.halted_at().is_none());`

Neither halting nor budget-stopped, so the packet's first arm
applies verbatim: `defer_wait_elapsed` is the way out.

## `fn a_verification_deferred_candidate_is_a_deferred_state_wi…` › `assert!(`

And the deferral is what makes the candidate ineligible: the
verification it just refused cannot be restarted from here.

## `fn a_verification_deferred_candidate_is_a_deferred_state_wi…` › `assert_eq!(`

A deferred candidate holds the run open whatever else is true.

## `fn the_publication_relations_are_exercised_in_both_directio…` › `let census = census();`

`coverage_assertions`: every `merge_prepared(fast)` matching the
relation is accepted and every mismatch refused, and the stale_clean
and already_present relations exercised both ways. Both directions
of each, over the states the census actually reached.

## `fn the_publication_relations_are_exercised_in_both_directio…` › `assert!(census.transitions().iter().any(|transition| {`

A fast publication with a prepared pin is refused everywhere, which
is the one of the three fast clauses that is about a field's
presence rather than about two SHAs agreeing.

## `fn no_offer_is_unmapped_and_every_class_is_offered_everywhe…` › `let census = census();`

"no (state, event class) pair is unmapped": every offer produced an
acceptance or a refusal, and the count is the product rather than
whatever survived.

## `fn no_offer_is_unmapped_and_every_class_is_offered_everywhe…` › `assert!(!census.accepted_labels().is_empty());`

Both directions occur for the census as a whole, and the search ran
to exhaustion rather than stopping at its state ceiling.

## `fn replaying_every_explored_trace_reaches_the_state_it_was_…` › `let census = census();`

INV-02 over the whole explored set: the live path and the replay
path are one transition function, so a trace folded event by event
during exploration and the same trace replayed from nothing must be
the same state.

## `fn replaying_every_explored_trace_reaches_the_state_it_was_…` › `let again = TopologyFold::replay(inputs(), &state.trace).expect("replays again");`

Replaying twice is equal, which is the property a resume needs
and a fold with hidden state would not have.

## `fn the_census_reaches_every_outcome_and_says_what_it_did_no…` › `let mut compared = 0;`

And each is accepted as a `run_finished` exactly where it is derived
and refused everywhere else: the guard is a comparison, not a
validity check.

## `fn the_census_reaches_every_outcome_and_says_what_it_did_no…` › `if state.fold.finished().is_some() {`

A second `run_finished` is refused whatever the derived outcome,
so the comparison the guard makes is only visible before the
first one. Both halves are asserted.

## `fn the_skeleton_states_the_bounds_it_ran_under_and_the_ones…` › `let bounds = CensusBounds::default();`

What this slice does *not* establish, as an assertion rather than as
a paragraph: the fixture is two originals with no repairs, and the
packet's bounds are three originals with two repairs and two
lineages. PR10 raises them; nothing here should read as if it
already had.

## `fn the_skeleton_states_the_bounds_it_ran_under_and_the_ones…` › `assert!(`

No repair is spawned by any class this skeleton offers, so no
lineage lease is ever taken: the lineage half of the census is
PR10's.

## `fn the_fixture_varies_every_field_a_relation_reads()` › `let started = run_started();`

Distinct-value counts, so "hostile" is checkable.

## `fn the_fixture_varies_every_field_a_relation_reads()` › `let limits = BTreeSet::from([`

Three limits, three numbers.

## `fn the_fixture_varies_every_field_a_relation_reads()` › `let efforts = BTreeSet::from([`

Four efforts, four values.

## `fn the_fixture_varies_every_field_a_relation_reads()` › `assert_ne!(started.chains[0].tiers.len(), started.chains[1].tiers.len());`

The two tasks differ in ladder length, attempts allowance, kind and
region, so no relation over them can pass by symmetry.

## `fn the_fixture_varies_every_field_a_relation_reads()` › `let shas = BTreeSet::from([`

Every symbolic sha a relation compares is a different literal: base,
the two candidate commits, the moved head, the proposal, and the
three deliberate non-matches.

## `fn the_fixture_varies_every_field_a_relation_reads()` › `assert_ne!(started.registry_digest, String::new());`

The digest is derived from the record rather than pinned, so a
fixture whose plan moved fails to start rather than folding on a
guess.

## `fn a_census_that_hits_its_ceiling_says_so()` › `let tight = CensusBounds {`

The truncation flag is the one thing standing between "explored
everything" and "explored the first six hundred", so it is asserted
in both directions rather than trusted.

## `fn a_census_that_hits_its_ceiling_says_so()` › `let shallow = CensusBounds {`

And the trace ceiling bounds the search without silently dropping
offers: a state at the ceiling is recorded and simply not extended.

## `fn a_census_that_hits_its_ceiling_says_so()` › `max_trace: 2,`

The seed trace already holds `run_started`, so a ceiling of two
extends the root and nothing beyond it.

## `fn a_transaction_class_is_reachable_and_blocks_the_run_from…` › `let census = census();`

The one relation the totality oracle above leans on hardest — that
an unresolved integration transaction makes `common` false — needs
a state that actually has one, or the oracle is asserting over an
empty set.

## `fn a_transaction_class_is_reachable_and_blocks_the_run_from…` › `let classes_seen: BTreeSet<&'static str> = with_transaction`

Both transaction classes are reached, so `common` is false for the
verification-running case and for the publication-owed case alike.

## `mod tests` › `fn state_at(id: usize, fold: TopologyFold, outcome: DerivedOutcome) -> CensusState {`

-----------------------------------------------------------------------
PR3-ST14-001 — the totality assertion's own independence
-----------------------------------------------------------------------

## `mod tests` › `fn state_at(id: usize, fold: TopologyFold, outcome: DerivedOutcome) -> CensusState {`

A hand-built state, for feeding the checker something the explorer
would never produce.

## `mod tests` › `fn fold_with(outcome: &DerivedOutcome) -> TopologyFold {`

A fold the census actually reached whose outcome is this one.

## `fn the_totality_audit_reports_a_fold_error_a_normalisation_…` › `let ending = fold_with(&DerivedOutcome::Ending(RunOutcome::Complete));`

`the_derived_outcome_is_total_over_every_explored_state` asserts
through `TotalityAudit`, and on the real census every list it
returns is empty — which is also what a checker that does nothing
returns. So the checker is shown three failures it must report,
built by hand because a census cannot be asked to produce them.

## `fn the_totality_audit_reports_a_fold_error_a_normalisation_…` › `let sentinel = vec![`

(1) Filtering. A `FoldError` is reported even when it is only the
*recorded* value and a fresh evaluation of the fold beside it
disagrees: a checker that quietly preferred one side could reach
zero by discarding the other.

## `fn the_totality_audit_reports_a_fold_error_a_normalisation_…` › `let normalised = vec![state_at(0, ending.clone(), DerivedOutcome::NotEnding)];`

(2) Normalising. A state recorded `NotEnding` over a fold that ends
is a disagreement, named by id rather than resolved.

## `fn the_totality_audit_reports_a_fold_error_a_normalisation_…` › `let short = vec![`

(3) Skipping. The domain is what it was handed, in order, so a
caller comparing it with the ids something else computed sees a gap
rather than a shorter list that still looks total.

## `fn the_totality_audit_reports_a_fold_error_a_normalisation_…` › `let clean = vec![`

And the honest list, so the three above are differences rather than
the only thing this checker ever says.

## `fn the_census_transition_table_is_reproducible_from_the_fol…` › `let census = census();`

PR3-ST14-001, the half no loop over `states()` can reach: a
successor that was dropped before it was recorded is not in
`states()`, so nothing that reads `states()` can miss it.

This does not read the record and check it for consistency with
itself. It re-derives the whole table from the folds and the class
function — the real `plan_transition`, the real `apply_delta` — and
requires the record to be what that derivation produced. Every way
the explorer could edit its own evidence lands here: an accepted
offer filed as a refusal, a refusal filed as an acceptance, an edge
pointing at a state that is not the one applying the delta reaches,
an outcome rewritten on the way in, or a row no offer produced.

## `fn the_census_transition_table_is_reproducible_from_the_fol…` › `assert_eq!(`

And the public accessor, re-derived without reading the
transition list at all: an accepted delta exists *at this state*
or it does not.

## `fn the_seed_state_is_evaluated_rather_than_assumed_not_endi…` › `let ended = census()`

A3-ST14-021: an explorer that writes `NotEnding` for its seed — the
one state no transition produced — and evaluates only the
successors. Every assertion over a census seeded from `started()`
survives it, because that seed *is* NotEnding.

So the seed is one the answer cannot be guessed at: a state the
census itself reached with the run already over, re-explored under a
trace ceiling that extends nothing.

## `mod tests` › `fn unresolvable_merge() -> TopologyEvent {`

-----------------------------------------------------------------------
PR3-ST14-003 — what `has_legal_transition` promises
-----------------------------------------------------------------------

## `mod tests` › `fn unresolvable_merge() -> TopologyEvent {`

A merge no state of this fixture accepts: there is no open transaction
for it to resolve.

## `fn has_legal_transition_is_local_to_the_state_and_excludes_…` › `let refusals = Census::explore(`

PR3-ST14-003. `every_deferred_state_has_a_legal_next_transition`
only ever asserts this accessor *true*, so a predicate that answers
true too often passes it: a global existential over the whole
census, or one that counts a refusal as progress. Both are answered
here by censuses in which the honest answer is `false`.

## `fn has_legal_transition_is_local_to_the_state_and_excludes_…` › `let refusals = Census::explore(`

Refusals only. `plan_transition` returning `Err` is the fold
working, not the run moving.

## `fn has_legal_transition_is_local_to_the_state_and_excludes_…` › `let mixed = Census::explore(`

Locality: one state that can move beside one that cannot. A global
existential answers `true` at both.

## `fn has_legal_transition_is_local_to_the_state_and_excludes_…` › `assert_eq!(mixed.outgoing(1).count(), 1);`

The dead state's offers were made and answered; it is not a state
the search declined to extend.

## `fn has_legal_transition_is_local_to_the_state_and_excludes_…` › `assert!(!mixed.has_legal_transition(2));`

An id no state carries has no offers, and so no legal transition —
the answer a whole-census existential cannot give.

## `mod tests` › `fn verification_started(`

-----------------------------------------------------------------------
PR3-ST14-002 — what the abstraction key must keep
-----------------------------------------------------------------------

## `mod tests` › `fn verification_started(`

A stale-clean verification of a candidate, with the three fields the
`merge_prepared` relations are about named.

## `mod tests` › `fn queued_candidate_trace(paths: PathSet) -> Vec<TopologyEvent> {`

The prefix every abstraction witness below shares: one task's
generation carried as far as a queued candidate over `paths`.

## `fn queued_candidate_trace(paths: PathSet) -> Vec<TopologyEv…` › `candidate_prepared_over(ALEPH, 0, 1, paths.clone()),`

No `attempt_finished{Succeeded}` between them: `candidate_prepared`
is the sole successful settlement for a candidate-producing
attempt, and the fold refuses the pair since the 2026-08-27
CONFORM ruling. A prefix that still built it would be refused
here rather than silently exploring a shape no run can write.

## `mod tests` › `fn queued_candidate_at(base: CommitSha, commit: CommitSha) -> Vec<TopologyEvent> {`

The two candidate-side witnesses' shared prefix: `aleph`'s first
generation carried to a queued candidate, with the two labels the fast
publication relations compare a candidate against as parameters.

Each label is carried by more than one event, and the fold is why: it
refuses a `candidate_prepared` whose base disagrees with its
generation's dispatch, one whose parent is not its own base, and a
`task_candidate_created` promoting a commit the prepared record does not
hold. So the reachable unit of variation is one *label* across a trace
rather than one field of one event — checked on the trace by
[`WitnessShape::OneLabel`] and on the state the fold kept by
[`RecordedOperand`].

## `fn queued_candidate_at(base: CommitSha, commit: CommitSha) …` › `candidate_prepared_at(ALEPH, 0, 1, region(ALEPH), base, commit.clone()),`

The settlement is `candidate_prepared` itself; see the sibling
prefix above.

## `mod tests` › `fn fast_publication(base: CommitSha, commit: CommitSha) -> TopologyEvent {`

The fast publication of `aleph`'s first candidate at one head.

The two labels are the offer side of the two relations
`decisions.bounded_census.abstraction` retains, so an offer built from
one witness leg's own labels is exactly the question the other leg has
to answer differently. How each is compared differs, and the refusals
the witnesses draw say which:

* *the base* — `check_merge_prepared`'s own line, refusals[9]: the head
  a fast publication expects is the candidate's recorded base.
* *the commit* — through the identity the publication cites, because
  `self_consistency` refuses a fast publication whose `proposed_sha` is
  not the commit it names and `prepared_candidate` refuses a citation of
  a commit the log never recorded. The two compose into "proposed_sha
  versus the candidate's commit label", and the fold's own later
  comparison of the two is unreachable for a well-formed fast event.

## `mod tests` › `fn prepared_record(fold: &TopologyFold) -> PreparedCandidate {`

The `candidate_prepared` record `aleph`'s first generation kept.

## `mod tests` › `enum WitnessShape {`

How a witness pair's two traces are known to differ by one thing.

## `enum WitnessShape` › `OneField { from: String, to: String },`

One event replaced, and one string-valued field of it changed:
substituting the old value for the new in the replaced event's own
rendering reproduces it exactly, which no two-field change does.

## `enum WitnessShape` › `OneLabel { from: String, to: String },`

One symbolic label replaced throughout, in however many events
carry it: substituting the old label for the new across the whole
trace's rendering reproduces it exactly, so nothing else moved.

Distinct from [`Self::OneField`] rather than a laxer version of it
— a pair that moves one event is required to declare itself
`OneField`, so this variant cannot become the place a two-field
change hides.

## `enum WitnessShape` › `OneRegion,`

One event replaced through a helper that takes the region as its
only parameter.

## `enum WitnessShape` › `OneAppend,`

One event appended whose whole documented effect is the relation.

## `enum WitnessShape` › `Reordered,`

The same events in a different order.

## `mod tests` › `enum RecordedOperand {`

The one field of the recorded [`PreparedCandidate`] a candidate-side
witness moves.

[`WitnessShape`] is a claim about the two traces; this is the same claim
about the state the fold kept, and they are not the same claim. A trace
that varies one label could still leave the fold recording two
differences, or none, and it is the record that the relations read.
Checked the way `OneField` checks an event: copy the named field across
and require the two records to become equal.

## `struct RelationWitness` › `opposed: Option<(TopologyEvent, TopologyEvent)>,`

A pair of offers, the first accepted at `left` and refused at
`right` and the second the mirror. A fingerprint difference says
the key kept the relation; this says the relation was worth
keeping.

## `struct RelationWitness` › `recorded: Option<RecordedOperand>,`

What the two legs' recorded candidates differ in, when the witness
varies the candidate side.

## `fn abstraction_witnesses() -> Vec<RelationWitness>` › `let mut aleph_first = vec![run_started_event()];`

Two candidates, queued in each order. The same events, so nothing
but the queue's order can tell the two states apart.

## `fn abstraction_witnesses() -> Vec<RelationWitness>` › `candidate_prepared(key, 0, 1),`

`candidate_prepared` settles the attempt; see the prefix
helpers above.

## `fn abstraction_witnesses() -> Vec<RelationWitness>` › `let shared_commit = candidate_of(ALEPH, 0).commit_sha;`

The candidate side of the two fast relations. `expected_head` and
`proposed_sha` above vary the *offer*; these vary what an offer is
compared against — the candidate's own base and its own commit —
which no offer-side witness reaches, because the fixture hardcodes
one base and derives one commit per key and generation. So a key
that dropped either operand would confuse two states the fast
relation answers oppositely, and every witness above would stay
green. See `fast_publication` for which comparison each draws.

## `fn the_abstraction_key_separates_states_that_differ_in_one_…` › `let witnesses = abstraction_witnesses();`

PR3-ST14-002. A key that forgets a relation cannot be caught by
looking at what the census explored: the two states it confuses
become one, and the second is never recorded, so nothing downstream
has anything to miss. It is caught by handing it two states that
differ in exactly one retained relation and requiring two answers.

`decisions.bounded_census.abstraction`: "all relational predicates
used by plan_transition retained (... overlap ... verification_deferred
and defers per candidate ... queue order ... and the merge_prepared
relations — expected_head versus the candidate's base label,
proposed_sha versus the candidate's commit label or the pinned
proposal label, prepared_ref presence)".

PR3-ST14-005: two of those relations have an operand on each side.
`expected_head` and `proposed_sha` are the offer's; the candidate's
base and the candidate's commit are the state's, and a key that
dropped either would leave every offer-side witness green. They get
witnesses of their own below, and a second obligation with them:
distinct fingerprints, *and* one fast publication that the two legs
answer in opposite directions.

## `fn the_abstraction_key_separates_states_that_differ_in_one_…` › `match &witness.shape {`

The witness is one difference, checked rather than asserted into
being by the way it was written.

## `fn the_abstraction_key_separates_states_that_differ_in_one_…` › `if let Some(operand) = &witness.recorded {`

The trace's one difference, again as one difference in the
record the relations actually read.

## `fn the_abstraction_key_separates_states_that_differ_in_one_…` › `assert!(`

And the two answers the key owes.

## `fn the_abstraction_key_separates_states_that_differ_in_one_…` › `if let Some((for_left, for_right)) = &witness.opposed {`

For the candidate-side pairs, what the difference is *for*: one
publication, accepted at one leg and refused at the other. Both
directions, so a leg that refuses everything — an unreachable
trace, a candidate that never queued — cannot pass for a witness
by refusing the one offer aimed at it.

## `fn the_abstraction_key_separates_states_that_differ_in_one_…` › `let named: BTreeSet<&str> = witnesses.iter().map(|witness| witness.relation).collect();`

Every relation named once, so a witness cannot be counted twice.

## `fn the_abstraction_key_separates_states_that_differ_in_one_…` › `assert_eq!(`

And the second obligation is carried by both candidate-side operands
rather than by whichever one was written first.

## `mod tests` › `fn overlap_classes(fold: &TopologyFold) -> Vec<Candidate> {`

The overlap census: one route, branching only where a region is chosen.

## `fn an_overlapping_region_is_explored_and_changes_a_transiti…` › `let census = Census::explore(`

PR3-ST14-002's other half: A3-ST14-014 normalises AB to A or drops
path regions from the key. `decisions.bounded_census.abstraction`
names three regions, and AB is the only one under which the overlap
relation answers differently.

The two states here differ in one thing: whether `aleph`'s candidate
lease covers `bet`'s region as well as its own. Under A, `bet` is
still dispatchable, so the run has structurally admissible work and
does not end. Under AB it is lease-blocked, `aleph`'s own queued
candidate is ineligible behind its open question, and the run is
Parked — so `run_finished(Parked)` is refused at one and accepted at
the other. Alias the two and one of those two answers is not in the
census at all.

## `fn an_overlapping_region_is_explored_and_changes_a_transiti…` › `assert_eq!(wide.trace.len(), narrow.trace.len());`

The traces differ in exactly one event, and in that event only the
region.

## `fn an_overlapping_region_is_explored_and_changes_a_transiti…` › `assert_eq!(differing, vec![3], "more than the region moved");`

**Index 3, because the trace is one event shorter than it was.** The
explorer keeps the transitions the fold accepts, and since the
2026-08-27 CONFORM ruling `attempt_finished{Succeeded}` is not one:
`candidate_prepared` is the sole successful settlement, so that edge
is gone from the graph and every trace through a promotion loses a
step. The index is regenerated from the shorter trace rather than the
assertion being loosened — it is still "exactly one event differs,
and in it only the region".

## `fn an_overlapping_region_is_explored_and_changes_a_transiti…` › `assert_ne!(wide.id, narrow.id);`

Two states, and two different answers to the same offer.

## `fn an_overlapping_region_is_explored_and_changes_a_transiti…` › `assert_ne!(region(ALEPH), overlap_region());`

Both regions really are in play, and AB is neither of the other two.

## `mod tests` › `fn generated_by_the_classes() -> (BTreeSet<u32>, BTreeSet<u32>, BTreeSet<String>) {`

-----------------------------------------------------------------------
PR3-ST14-004 — the bounds it generated, and one-field negatives
-----------------------------------------------------------------------

## `mod tests` › `fn generated_by_the_classes() -> (BTreeSet<u32>, BTreeSet<u32>, BTreeSet<String>) {`

Every generation id, attempt number and question id the fixture's
classes construct, read off the events rather than off their labels: a
label is a string a change to the payload it names can leave alone.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `let bounds = CensusBounds::default();`

PR3-ST14-004. A3-ST14-011 turns the attempt generator's `1..=2` into
`1..2`. Attempt 2 stops being offered anywhere, an attempt-2-only
transition defect becomes invisible, and `CensusBounds` goes on
reporting `attempts_per_generation: 2` — a boundary the skeleton did
not generate. So each declared dimension is measured against what
the fixture actually built, and the shortfalls are named rather than
left to read as coverage.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `let open_questions = census`

The most questions any explored state holds open at once, which is
what the bound is about.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `let mut sequences = BTreeSet::new();`

Integration sequences the census ran, and verification deferrals any
candidate took.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `let generated: BTreeMap<&str, u32> = [`

What the fixture generated, per declared dimension.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `assert_eq!(`

The declared list and the measured list are the same list, so a
ninth dimension cannot be declared without being measured.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `let rendered = format!("{bounds:#?}");`

...and `dimensions()` is every dimension `CensusBounds` declares,
read off its own rendering rather than off a list beside it. A field
added to the struct and forgotten here fails.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `let at_maximum = [`

Which dimensions this fixture takes to their declared maximum, and
which it does not. Both lists are asserted, so a shortfall cannot
become coverage and coverage cannot quietly become a shortfall.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `assert_eq!(attempts, BTreeSet::from([1, 2]));`

And maximum-plus-one is excluded rather than merely unobserved: the
identities the fixture generates are exactly `1..=max`, densely,
with nothing above.

## `fn every_declared_dimension_reports_what_the_fixture_genera…` › `assert_eq!(question_ids.len(), 4);`

Four question identities are constructed and at most two are ever
open together, so the bound is about simultaneity and is measured as
such.

## `mod tests` › `fn merge_prepared_diff(left: &MergePrepared, right: &MergePrepared) -> Vec<&'static str> {`

Which fields of two publication records differ, by name.

## `fn every_publication_negative_differs_from_its_positive_in_…` › `let fast = merge_prepared_of("merge_prepared/fast/match/aleph/g0");`

PR3-ST14-004's other half. A3-ST14-044 changes `proposed_sha` in the
moved-head payload as well as `expected_head`. The class named
`moved-head` stays refused — because the proposal is wrong — so a
fold that stopped comparing heads altogether passes a test that
reads as evidence the head relation is checked.

So each negative is required to be its positive with exactly one
field moved, before anything is asserted about the answer.

## `fn every_publication_negative_differs_from_its_positive_in_…` › `let rendered = format!("{fast:#?}");`

The diff names every field of the record, checked against the
record's own rendering: a field added to `MergePrepared` and
forgotten here would make "exactly one" a claim about a subset.

## `fn every_publication_negative_differs_from_its_positive_in_…` › `assert!(refused.contains(label), "{label} was never refused");`

And only then the answers: the positive accepted somewhere, this
negative refused everywhere.
