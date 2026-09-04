# `src/ladder.rs`

Extended notes for [`src/ladder.rs`](../../src/ladder.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The verification ladder's decision (DESIGN.md §11.4, §19).

One pure function answers the only question the engine has after an attempt
fails: *what now?* Retry the same rung with feedback, escalate to the next
rung on a fresh session, defer without spending an attempt, park the task
behind a question, or fail it.

It is deliberately I/O-free and holds no state of its own. The two rules
that are easy to get wrong live here, in one place, where they can be
tested exhaustively:

1. **Not every failure is the worker's fault.** A rate-limited pool or an
   unavailable reviewer is an outage (§19). Those defer; they must never
   burn an attempt, escalate the task to a more expensive rung, or count
   toward exhausting the chain — that would spend frontier tokens to
   "fix" code nobody found a problem with.
2. **The human is the top rung** (§11.4). Running out of chain is not a
   failure; it is an `Unblock` question.

## `pub enum FailureKind {`

Why an attempt did not pass. Kept apart from the prose reason so the ladder
can dispatch on it and the event log (step 8) can aggregate it.

## `pub enum FailureKind` › `NoChain,`

The resolved chain has no rungs — a config defect, not a task failure.

## `pub enum FailureKind` › `ReviewInputTooLarge,`

The worker produced more evidence than one complete review may accept.
Retrying the same task cannot change that policy boundary, so this
parks for a human to split or otherwise rescope the task after the
attempt's real spend has been settled.

## `pub enum FailureKind` › `ReviewInputOpaque,`

The captured patch names a changed object whose content is not present
in the review evidence (binary, suppressed diff, or gitlink).

## `pub enum FailureKind` › `ReviewUnavailable,`

The reviewer could not run — an environment failure, not a judgement
on the change.

## `pub enum FailureKind` › `NeedsHuman,`

A worker or reviewer hit a decision it should not make alone (§12).

## `pub enum FailureKind` › `Declined,`

A human was asked to unblock the task and said no. Never produced by
[`next_step`] — it is how a question resolves, not how an attempt fails.

## `pub enum FailureKind` › `Interrupted,`

The engine died between an attempt starting and finishing, so nothing
judged the code. Never produced by [`next_step`] either — replay
synthesizes it for a dangling `attempt_started` and hands the task back
to the scheduler still on the same rung. It appears in the ledger with
unknown spend, because an attempt that really ran and really drained a
pool must not vanish from the record just because we cannot price it.

## `pub enum FailureOrigin {`

Who the failure happened to. `Timeout` and `RateLimited` mean opposite
things depending on this: an implementer that timed out failed its attempt
(§19), while a reviewer that timed out told us nothing about the code.

## `pub struct AttemptFailure` › `pub reason: String,`

Human-facing summary, for reports and questions.

## `pub struct AttemptFailure` › `pub feedback: Option<String>,`

What the retry sends back to the agent: a gate log tail (§11.1) or the
reviewer's `required_changes` (§11.2), verbatim.

## `impl AttemptFailure` › `pub fn new(kind: FailureKind, reason: impl Into<String>) -> Self {`

A failure attributed to the worker — the common case.

## `impl AttemptFailure` › `pub fn from_reviewer(mut self) -> Self {`

Re-attribute to the reviewer. The kind stays what actually happened
(a rate limit is still a rate limit, and the capacity engine will want
to know); the origin is what stops the implementer being blamed.

## `impl AttemptFailure` › `pub fn is_outage(&self) -> bool {`

An environment problem rather than a verdict on the code. These defer
instead of consuming an attempt (§19).

## `pub struct LadderPolicy {`

Fixed for a task by its resolved chain and the run's config.

## `pub struct LadderPolicy` › `pub attempts_per: u32,`

`attempts_per` for this task's kind (§10.1).

## `pub struct LadderPolicy` › `pub rungs: usize,`

How many rungs the resolved chain has.

## `pub struct LadderPolicy` › `pub max_defers: u32,`

How many deferrals a task may take before the pool counts as down and
the human is asked instead. Without the capacity engine there is no
reset time to wait for, so this bound is what stops an exhausted pool
spinning forever.

## `pub struct LadderState {`

Where the task stands right now.

## `pub struct LadderState` › `pub rung: usize,`

Index into the chain of the rung the failed attempt ran on.

## `pub struct LadderState` › `pub attempts_on_rung: u32,`

Attempts spent on this rung *including* the one that just failed.

## `pub struct LadderState` › `pub resumable: bool,`

Whether the next attempt could resume this one's session: the adapter
advertises `session_resume` (from `probe()`) and the attempt actually
returned a session id.

## `pub enum Next` › `RetrySameRung { resume: bool },`

Feed the failure back and try again at the same tier. `resume` carries
§14's consequence: a resumed retry keeps the working tree, so the
*cumulative* diff is what gets re-gated.

## `pub enum Next` › `Escalate,`

Next rung, fresh session, accumulated feedback (§11.4).

## `pub enum Next` › `Defer,`

Try again later without spending an attempt.

## `pub enum Next` › `AskHuman(QuestionKind),`

Park this task behind a question; the scheduler keeps draining
everything else (invariant 6).

## `pub enum Next` › `Fail,`

Terminal for this task.

## `pub struct FailureShape {`

The two fields that decide what a failure *costs*.

**Ask for what you read.** [`spends_allowance`] and [`FailureShape::is_outage`]
read exactly `kind` and `origin` — never the reason, the feedback, or
anything else on the failure. Naming that lets one rule serve both shapes a
failure takes in this tree: the live [`AttemptFailure`] the ladder decides
from, and the [`crate::events::FailureRecord`] the durable attempt record
carries.

That second reader is why this exists. `settle_failed` holds an
`AttemptRecord`, not an `AttemptFailure`, and it was deriving the allowance
itself from the ladder's `Next` — which disagreed with this function on a
park, the one cell whose whole point is that nothing is spent.

## `pub struct FailureShape` › `pub kind: FailureKind,`

What went wrong.

## `pub struct FailureShape` › `pub origin: FailureOrigin,`

Who it is attributed to.

## `impl FailureShape` › `pub const fn of(failure: &AttemptFailure) -> Self {`

The shape of a live failure.

## `impl FailureShape` › `pub fn is_outage(self) -> bool {`

Whether this failure is an outage — the environment's fault rather than
the implementer's.

The one implementation. [`AttemptFailure::is_outage`] delegates here.

## `pub fn spends_allowance(failure: Option<FailureShape>) -> bool {`

What to do after one failed attempt.
Whether an attempt that ended this way spent one of its rung's
`attempts_per`.

**Total, and the whole of the rule: an attempt spends iff the worker ran and
produced work to judge.**

This is the single production implementation of the allowance decision, and
it is here because [`next_step`] is its only consumer — the two would
otherwise be a rule and a copy of a rule, free to disagree about whether a
task escalates.

### It is derived, and it is derived from the failure

`attempt_finished` records a `SettlementTransition` and an `AttemptRecord`,
and **nothing that states the allowance decision**. That is deliberate: a
recorded conclusion sitting beside the recorded fact it derives from is an
internal-disagreement channel inside one event. A schema-4 resume derives it
here, from `AttemptRecord.failure`, which the event carries.

Keyed on the failure and not on the transition, because `Parked` is **not
one cell**. The legacy engine reaches `Next::AskHuman` by four paths that
disagree with each other, and `spends_allowance_matches_every_legacy_park_path`
is the grid of them.

### The packet does not state this

Its only attempt-path citations are interruption — T-ATTEMPT's "append
`attempt_interrupted` (unknown spend, allowance refunded…)" — and, by
analogy, one merge-verification "no attempt burned". The rule below is the
legacy engine's, preserved under `invariants_preserved[1]`, and the G2 pass
carries it into the packet.

## `pub fn spends_allowance(failure: Option<FailureShape>) -> b…` › `return true;`

No failure: the worker ran, and its work was judged and accepted.

## `pub fn spends_allowance(failure: Option<FailureShape>) -> b…` › `if failure.is_outage() {`

An outage is not a run that produced work. `next_step` defers rather than
escalating for exactly this reason — "Escalating here would move the task
to a pricier rung because a *pool* was busy, and retrying would burn
attempts on a run that never got a verdict."

## `pub fn spends_allowance(failure: Option<FailureShape>) -> b…` › `match failure.kind {`

Listed rather than defaulted, so a new `FailureKind` does not compile
until someone decides whether it spends. A default arm here would answer
a question nobody asked, in the direction that costs an operator a rung.

## `pub fn spends_allowance(failure: Option<FailureShape>) -> b…` › `FailureKind::NeedsHuman => false,`

"Asked for a human explicitly: the code was never judged, so nothing
is spent and nothing escalates." The agent declined to work.

## `pub fn spends_allowance(failure: Option<FailureShape>) -> b…` › `FailureKind::NoChain => false,`

No chain resolved, so no worker ran at all: "A task whose chain
resolved to nothing cannot be retried into existence."

## `pub fn spends_allowance(failure: Option<FailureShape>) -> b…` › `FailureKind::Interrupted => false,`

"The engine died between an attempt starting and finishing, so
nothing judged the code … hands the task back to the scheduler still
on the same rung." The one cell the packet states outright, and it
agrees: T-ATTEMPT's resume action is "append `attempt_interrupted`
(unknown spend, allowance refunded …)". Two independent sources, one
answer.

## `pub fn spends_allowance(failure: Option<FailureShape>) -> b…` › `FailureKind::Declined => false,`

"A human was asked to unblock the task and said no … it is how a
question resolves, not how an attempt fails." Unreachable as an
attempt outcome — `next_step` never produces it — and answered
anyway, because a match that is total is what makes a new variant
stop the build instead of taking a default. No worker ran for it.

## `pub fn spends_allowance(failure: Option<FailureShape>) -> b…` › `FailureKind::EmptyDiff`

Every remaining kind is a completed run. `ReviewInputTooLarge` and
`ReviewInputOpaque` are the instructive pair — the diff could not be
judged, and it still spends, because "The worker ran, so the attempt
is spent and must stay in the ledger". The line is *the worker ran*,
not *a verdict was reached*.

## `pub fn next_step(failure: &AttemptFailure, state: &LadderSt…` › `if failure.kind == FailureKind::NeedsHuman {`

Asked for a human explicitly: the code was never judged, so nothing is
spent and nothing escalates. Straight to a question (§12).

## `pub fn next_step(failure: &AttemptFailure, state: &LadderSt…` › `if matches!(`

The worker ran, so the attempt is spent and must stay in the ledger, but
no amount of automatic retrying can make the same complete diff fit the
review contract. Ask for a scope decision instead of paying again.

## `pub fn next_step(failure: &AttemptFailure, state: &LadderSt…` › `if failure.is_outage() {`

Outages defer. Escalating here would move the task to a pricier rung
because a *pool* was busy, and retrying would burn attempts on a run
that never got a verdict.

## `pub fn next_step(failure: &AttemptFailure, state: &LadderSt…` › `Next::AskHuman(QuestionKind::Unblock)`

The pool stayed down across every deferral: that is now a
genuine blocker, and blockers go to the top rung.

## `pub fn next_step(failure: &AttemptFailure, state: &LadderSt…` › `if failure.kind == FailureKind::NoChain || policy.rungs == 0 {`

A task whose chain resolved to nothing cannot be retried into existence.

## `pub fn next_step(failure: &AttemptFailure, state: &LadderSt…` › `if state.attempts_on_rung < policy.attempts_per {`

A real rejection of the work: spend an attempt.

## `pub fn next_step(failure: &AttemptFailure, state: &LadderSt…` › `Next::AskHuman(QuestionKind::Unblock)`

§11.4: chain exhausted — the human is the top rung.

## `mod tests` › `fn spends_allowance_matches_every_legacy_park_path() {`

**The four paths by which the legacy engine parks a task, one cell
each, with the comment that defines each cell quoted verbatim.**

`Parked` looks like one settlement and is four decisions. The grid
exists so the principle — *an attempt spends iff the worker ran and
produced work to judge* — cannot drift from the paths that define it:
a future edit that makes the principle prettier and one of these cells
wrong fails here, and the quoted comment beside it says which
engine-behaviour it just changed.

Every quotation is from `next_step` above or from
`engine::attempt::review_failure`, in this repository, and is the
authority under `invariants_preserved[1]` — the packet states none of
it.

## `fn spends_allowance_matches_every_legacy_park_path()` › `let grid: Vec<(FailureKind, FailureOrigin, bool, &str)> = vec![`

(kind, origin, spends, the legacy comment that decides it).

## `mod tests` › `fn chain_exhaustion_parks_only_after_the_allowance_was_already_spent() {`

The fourth park path is chain exhaustion, and it is a cell about
*arithmetic* rather than about a kind.

`next_step` reaches `AskHuman(Unblock)` at the end only once
`attempts_on_rung >= attempts_per` on the top rung — so the retries that
got there already spent them, and the park adds nothing. Asserted
through `next_step` itself rather than restated, because the claim is
about that function's control flow and a restatement would be a second
copy of it.

## `mod tests` › `fn every_failure_kind() -> Vec<FailureKind> {`

Every `FailureKind`, read from the enum's own source.

**Derived, because a hand-written list is not an enumeration.** This was a
14-element array whose comment claimed "a new variant between them fails
this list to compile". It does not: an array literal compiles perfectly
well while an enum grows past it, so the guard the comment described did
not exist. The comment was also **inverted** — it named `Interrupted` as
the first variant and `Declined` as the last, and the enum begins at
`NoChain` and ends at `Interrupted`. The round-3 review of `bf927f3`
found both.

Two mechanisms now, and they fail in different directions:

* this reads the variant names out of `ladder.rs` between the enum's
  header and its closing brace, so a variant that exists is **in the
  list** whether or not anyone remembered it;
* [`kind_of_name`] maps each name to a value through an exhaustive
  `match`, so a variant that exists **has a value here** or the crate
  does not build.

The source read is safe from this file's own prose because the enum is
declared above every test in it, and only the first occurrence of the
header is used.

## `mod tests` › `fn kind_of_name(name: &str) -> FailureKind {`

One variant name to its value, exhaustively.

The `match` is over the enum, so adding a variant stops the crate
building until it is named here; the `_` arm is over *strings* and only
catches a source read that produced something that is not a variant.

## `mod tests` › `fn exactly_thirteen_failure_shapes_spend_no_allowance() {`

**How many `FailureShape`s spend no allowance, counted from the authority.**

[`Settled::spent_attempt`]'s doc has stated this number three times and been
wrong three times: "every other settlement spends one" (off by six), then
"five kinds", then "seven shapes". A `FailureShape` **is** a
`(kind, origin)` pair — `spends_allowance` dispatches on both, and
`FailureShape::is_outage` reads the origin for `Timeout` — so the shape count
and the kind count are different numbers and the doc named one while stating
the other. The round-3 review of `bf927f3` found it.

**13 shapes, spanning 7 kinds.** Both are asserted, and the shape count is the
one the doc quotes, because a shape is what the authority takes.

## `fn exactly_thirteen_failure_shapes_spend_no_allowance()` › `assert_eq!(`

13 = four kinds at both origins, two outage kinds at both origins, and
`Timeout` at the reviewer alone. Spelled out because the arithmetic is
where "seven" came from: it is the kind count, and six of the seven
contribute two shapes each.

## `fn resume_follows_the_adapter_not_the_failure()` › `let mut cold = state(0, 1);`

§11.4 prefers session resume, but only where the adapter supports it
and a session actually came back; otherwise the retry starts fresh.

## `fn exhausting_a_rung_escalates_and_exhausting_the_chain_ask…` › `assert_eq!(`

Last rung, attempts spent: nothing cheaper or stronger is left.

## `fn rate_limits_defer_without_spending_an_attempt()` › `let mut last = state(2, 2);`

The whole point: a busy pool must not push the task up-tier or eat
its retries. Even on the last attempt of the last rung, it defers.

## `fn an_unavailable_reviewer_is_an_outage_not_a_rejection()` › `for kind in [FailureKind::ReviewUnavailable, FailureKind::RateLimited] {`

Step 6's rule, enforced by the ladder: a judge that could not run
says nothing about the code, so the implementer is not retried,
escalated, or blamed.

## `fn an_implementer_timeout_is_a_rejection_even_though_a_revi…` › `let worker = failure(FailureKind::Timeout);`

Same kind, opposite handling — this is why origin exists.

## `fn needs_human_parks_immediately_from_either_side()` › `assert_eq!(`

Not on the last attempt, not on the last rung: the ladder never
gets consulted, because nothing judged the code.
