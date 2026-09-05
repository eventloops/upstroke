# `src/engine/classify.rs`

Extended notes for [`src/engine/classify.rs`](../../../src/engine/classify.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The single production authority for **what an observation means about an
attempt**.

### Why this module exists

`ladder::next_step` reads an [`AttemptFailure`] and decides the whole of
what happens to a task: retry the rung, escalate, defer, park, or fail. It
is also what the **allowance decision** is derived from — an attempt spends
one of its rung's `attempts_per` iff the worker ran and produced work to
judge, and `AttemptRecord.failure` is the field that says which happened.

So a second engine classifying the same observations its own way would be
two rules deciding one thing, and the disagreement would not surface as a
wrong answer — it would surface as a task escalating to a pricier tier
because the other engine called the same diff something else. That is the
shape of `84a3978`, one field over.

### What is here, and what deliberately is not

Here: the classifications that were **inline** in the legacy engine's
verification ladder — the diff's own problems, and a failed gate.

Not here, because they were already functions and already reachable:
`attempt::review_failure`, which turns a `ReviewResult` into an
`AttemptFailure`, and `attempt::pool_option`. A "pure move" of something
that already sits alone in a callable function is churn, not extraction.

Not here either: the **running**. Gates and reviews execute at each
engine's own point in its own order, and only the *decision* about what
their result means is shared — the same split `ShellGate::command` makes
for a gate's command.

## `pub(crate) fn diff_failure(`

What the diff alone says is wrong with an attempt, before anything runs.

Two observations, in the legacy engine's order, and the order is the rule:
a diff no reviewer can read is refused before a Test task's provenance is
checked, because provenance is a judgement about content and the first
finding is that the content cannot be judged at all.

`has_reviewers` is load-bearing rather than a convenience. A diff that is
merely **too large** is only a failure when something is going to review it;
with reviews disabled there is no reader to defeat, and refusing it would
fail an attempt for a rule the run has switched off. An **opaque** diff
fails either way, because the engine's own capture could not read it.

## `pub(crate) fn gate_failure(failure: &GateFailure) -> AttemptFailure {`

What a failed gate says about an attempt.

The log tail rides as feedback because the next attempt's prompt quotes it:
a gate that failed without saying what it printed asks the worker to guess.

## `pub(crate) struct ReviewPassFacts<'a> {`

What one review pass is recorded as, and what it is recorded from.

A struct rather than ten parameters, the same shape
`assembly::WorkerAssembly` takes and for the same reason: every field is
something the caller already holds, and one this type invented would be a
field one engine could set and the other could not.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) pass: &'a str,`

The lens's name — which review this was.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) agent: &'a str,`

The agent that judged, and the model it ran.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) model: &'a str,`

The model, which `AttemptRecord` requires as a `String` and not an
`Option`.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) adapter: &'a str,`

The adapter that built the invocation.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) preflight_cli_version: Option<String>,`

What pre-flight certified this CLI as, where it certified one.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) effort: Option<crate::ir::Effort>,`

The effort the routing decision bound.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) pool: Option<String>,`

The pool, where the agent takes one.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) cost_usd: Option<f64>,`

What the vendor said this cost, where it says.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) unavailable: bool,`

Whether the judge never ran.

## `pub(crate) struct ReviewPassFacts<'a>` › `pub(crate) failed: bool,`

Whether it ran and said no.

## `impl ReviewPassFacts<'_>` › `pub(crate) fn record(self) -> ReviewRecord {`

The record.

**`unavailable` and `failed` are different questions and this keeps
them apart.** A judge that never ran is not a judge that said no, and
the ledger has to show which happened — a reviewer counted as having
rejected the work when its CLI was rate-limited would spend an attempt
for an outage, and `ladder::next_step` defers an outage precisely so
that it does not.

## `pub(crate) enum FeedbackCarrier {`

Where §11.4's feedback is durable for this settlement.

**An explicit choice with no default, because the record builder is shared and
the two engines answer differently.** §11.4 sends a gate log tail or the
reviewer's `required_changes` back to the next attempt; the question this
answers is which durable record carries that text, and only the caller knows.

It exists because the alternative was tried and was wrong.
`FailureRecord::detail` was added for schema 4 — whose settlement transitions
have no feedback field, so the attempt record is the only carrier there — and
the value was copied in unconditionally. `coordinator.rs` calls the same
builder, so the *legacy* wire and `report.json` began carrying the full text
too, duplicating the `ladder_retry`/`ladder_escalated` copy that already held
it and reversing the reason [`crate::events::LadderRetry`] gives for its own
shape: "a gate log tail runs to kilobytes, and `report.json` should not grow
one per attempt". The 2026-08-26 re-review of `c2c0294` found it, finding A.

**A field rather than a defaulted parameter** so that adding a third engine
does not compile until someone decides which carrier it has. A default here
would answer that question silently, in whichever direction the last author
happened to need.

## `pub(crate) enum FeedbackCarrier` › `LadderEvent,`

Schemas 1-3: `ladder_retry` and `ladder_escalated` carry `summary` **and**
`detail` outright, and `Progress::feedback` is rebuilt by replaying them.
The attempt record must not duplicate the text.

## `pub(crate) enum FeedbackCarrier` › `AttemptRecord,`

Schema 4: no `SettlementTransition` variant has a feedback field, so the
attempt record is where §11.4's feedback is durable or it is nowhere.

## `pub(crate) struct AttemptFacts<'a> {`

The routing and product facts one attempt's ledger line is built from.

**Ask for what you read.** [`attempt_record`] reads exactly these; it never
sees a task, a plan or a run. Naming them lets one construction serve the
legacy coordinator, which holds a `Rung` and an `Outcome`, and the schema-4
driver, which holds a `RungBinding` and an `Assessment`.

## `pub(crate) struct AttemptFacts<'a>` › `pub(crate) tier: crate::ir::Tier,`

Which rung's tier the work ran at.

## `pub(crate) struct AttemptFacts<'a>` › `pub(crate) model: &'a str,`

The model that ran it. Required, not optional — `AttemptRecord` has no
shape for "some model".

## `pub(crate) struct AttemptFacts<'a>` › `pub(crate) pool: Option<String>,`

Which subscription pays for it, where one is named.

## `pub(crate) struct AttemptFacts<'a>` › `pub(crate) resumed: bool,`

Whether this attempt resumed the previous one's session.

## `pub(crate) struct AttemptFacts<'a>` › `pub(crate) outcome: &'a crate::ir::Outcome,`

The adapter's parse of what the worker said about itself.

## `pub(crate) struct AttemptFacts<'a>` › `pub(crate) reviews: &'a [crate::events::ReviewRecord],`

What the reviewers said. Empty means **nothing reviewed**, which is a
different claim from "reviewed and passed", and the record keeps the
difference.

## `pub(crate) struct AttemptFacts<'a>` › `pub(crate) failure: Option<&'a AttemptFailure>,`

Why the attempt failed, if it did.

## `pub(crate) struct AttemptFacts<'a>` › `pub(crate) feedback: FeedbackCarrier,`

Which durable record carries this settlement's §11.4 feedback.

## `pub(crate) fn attempt_record(attempt: u32, facts: AttemptFacts<'_>) -> AttemptRecord {`

One attempt's durable ledger line.

The one production construction of an [`AttemptRecord`] **for an attempt
that reached a settlement**. It was inline in `coordinator.rs`'s settlement,
where the schema-4 driver could not reach it, and it belongs here rather
than beside the command assembler because its last field is a
classification: `failure` is what this module exists to decide, and
`ladder::next_step` and `ladder::spends_allowance` both read the answer back
out of the record.

**The qualifier is not decoration.** This said "the one production
construction" outright, and there is a second: `events::InterruptedAttempt::event`
builds the record for an attempt that started and never reported back, which
by construction was never classified. A census over the type's initializers
in production regions returns exactly those two, and the unqualified sentence
is the class §22b of `reviews/FINDINGS.md` names — a property claim about the
rest of the tree, true when written and checkable only by census. It was
caught by adding a field to `FailureRecord` and reading the compiler's list
of what stopped compiling.

**Neither this comment nor that census may spell the tokens it counts.** The
first draft of this paragraph quoted the initializer and the test-only
attribute literally, and a region-cutting census then stopped *here* — inside
a doc comment, above the construction it was looking for — and reported one
production construction where there are two. Same class as §4's
self-referential greps, and the third time this slice has paid for it.

## `pub(crate) fn attempt_record(attempt: u32, facts: AttemptFacts<'_>) -> AttemptRecord {` › `detail: match facts.feedback {`

§11.4's feedback, onto the durable record — **for the caller
that has nowhere else to put it**. It was dropped here entirely:
`AttemptFailure` carried the gate tail and the reviewer's
`required_changes`, and the record kept only the summary, so a
resumed schema-4 run could say *that* an attempt failed and
nothing about what the next one must do differently. Then it was
copied here unconditionally, which handed the same text to the
legacy wire and `report.json` as well. Neither is the rule; the
rule is that the carrier is the caller's answer.
`decisions/2026-08-26-durable-retry-feedback.md`.

## `pub(crate) fn review_input_failure(problem: String) -> AttemptFailure {`

A tree whose staged bytes are not the bytes a gate would see.

`Workspace::review_input_problem_for_tree` decides *whether* — a
clean/smudge filter, or a dirty submodule behind an unchanged gitlink — and
this decides what that means for the attempt. Attributed to the reviewer,
not the implementer: the worker wrote what it was asked to, and the tree is
what cannot be judged.
