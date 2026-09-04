# `src/engine/attempt.rs`

Extended notes for [`src/engine/attempt.rs`](../../../src/engine/attempt.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `#![allow(clippy::disallowed_methods)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `const MAX_FEEDBACK_ENTRIES: usize = 6;`

Most recent feedback entries carried into an escalated prompt. Older
failures are summarized; the newest keeps its full log tail.

## `pub(super) const QUESTION_MARKER: &str = "UPSTROKE-QUESTION:";`

§12: how a worker flags a decision it should not make alone. The prompt
teaches this marker; nothing else in the engine parses agent prose.

## `pub(super) fn pool_option(pool: &str) -> Option<String> {`

A `WorkerProfile.pool` as the log records it: `None` rather than `""` when
no pool is configured, so a reader can tell "no pools file" from "a pool
whose name is empty" — and so a fold never attributes spend to `""`.

## `pub(super) struct AttemptCx<'a> {`

Everything one attempt needs, so the ladder can loop over (rung, attempt)
without re-deriving any of it.

## `pub(super) struct AttemptCx<'a>` › `pub(super) runner: &'a dyn Runner,`

Where every process of this attempt executes (DESIGN.md:118). The host
runner today; PR6 swaps in the container one behind the same `dyn`.

## `pub(super) struct AttemptCx<'a>` › `pub(super) task_index: u32,`

This task's position in the plan, which is the legacy engine's own
scope for [`InvocationId`]. See [`AttemptCx::invocation`].

## `pub(super) struct AttemptCx<'a>` › `pub(super) stem: String,`

Collision-free file stem for this task's run artifacts.

## `pub(super) struct AttemptCx<'a>` › `pub(super) reviewers: Vec<Reviewer<'a>>,`

The ordered review passes for this task (§11.3). Empty only when review
is switched off explicitly.

## `pub(super) struct AttemptCx<'a>` › `pub(super) review_pass_timeout: Duration,`

Independent allowance for every reviewer in `reviewers`; one pass may
use it across its initial verdict and one format-only re-ask.

## `pub(super) struct AttemptCx<'a>` › `pub(super) retry: Option<RetryBrief>,`

`None` on the first attempt.

## `pub(super) struct AttemptCx<'a>` › `pub(super) decisions: Vec<String>,`

Answers the operator has given about this task (§12), in the order they
arrived. The worker gets these as instructions; so must the judge.

## `impl AttemptCx<'_>` › `fn invocation(&self, role: AttemptRole) -> InvocationId {`

The identity of one process of this attempt.

The contract's `invariants_introduced[1]`: "legacy engine assigns
**legacy-scoped** values". The scope is
[`crate::runner::invocation::LEGACY_GENERATION`] — generation 0, which
[`InvocationId::legacy_attempt`] supplies — because the legacy engine
has no generations: it never re-dispatches a task from a fresh worktree,
so there is no second generation for a value to sit in. A legacy run is
schema-1..3 and a generation-bearing run is schema-4, and INV-23 forbids
a run changing schema between epochs, so the two sets never share a
ledger and generation 0 is a scope rather than a coincidence.

The key is the task's **position in the plan**, not a topology
`TaskKey`: the legacy engine has no task registry to draw one from, and
what the identity has to be is unique per process, which a dense
position is. `(position, attempt, role, ordinal)` is unique because a
position names one task, `attempt` increments per attempt of it
(INV-20's "changes with every attempt"), `role` distinguishes the
worker from gate `n` and review pass `n`, and nothing inside one
attempt runs a given role twice — so every ordinal here is 0, and a
re-dispatch that did run one twice would need a second ordinal rather
than a reused identity.

## `pub(super) struct RetryBrief {`

What the retry prompt needs to know (§11.4).

## `pub(super) struct RetryBrief` › `pub(super) resumed: bool,`

The session carries the earlier conversation, so the prompt is terse.

## `pub(super) struct RetryBrief` › `pub(super) feedback: Vec<Feedback>,`

Every failure so far, oldest first.

## `pub(super) struct Reviewer<'a> {`

One read-only worker judging an attempt (§11.2). The list is empty only
when the user explicitly set `review = { enabled = false }`; a pass that
cannot be resolved is a hard error, never a silent downgrade.

## `pub(super) struct AttemptResult` › `pub(super) candidate_branch_ref: String,`

Immutable git identities captured with the diff before any gate or
reviewer ran. A successful commit is prepared from these exact objects.

## `pub(super) struct AttemptResult` › `pub(super) reviews: Vec<events::ReviewRecord>,`

The passes that actually ran, in order — empty when the cheap checks
failed first and no review happened. Derived from the reviews having
happened rather than from passes being configured, so the ledger never
credits a model with work it did not do (§13).

## `pub(super) fn run_attempt(`

Run one attempt and verify it, without deciding what happens next: the
caller owns commit, rollback, retry, and escalation (§11/§14).

## `let worker_workspace = workspace.root().to_path_buf();`

The command is assembled by `engine::assembly`, which is the one
production place that decides a worker invocation's inputs. This block
used to do it inline; it moved so the schema-4 driver could be its second
caller rather than its second implementation.

The adapter says what to run; the runner says where. `ExecutionRole::
Implement` with the bound agent is what makes this process slotted
(R3) and what tells `host-v1` to supply that agent's credential
location — both properties of the role, not of this call site.

## `let mut failure = evaluate_outcome(&outcome, &output);`

Verification ladder (§11): outcome sanity → cheap static provenance →
gates → review. Cheapest and most objective first.

## `if failure.is_none() {`

What the diff alone says, in the legacy order. `engine::classify` is the
one production place that decides it; the schema-4 driver reads the same
answer rather than forming its own, because `ladder::next_step` reads the
result and the allowance decision is derived from it.

## `if let Some(problem) =`

Through the seam and the classifier, so the schema-4 driver runs the
same rung rather than a copy of it.

## `let mut reviews = Vec::new();`

§11.2: gates are objective but shallow — a strong reviewer judges the
diff against the acceptance criteria only once the cheap checks pass.
§11.3: on blast-radius paths a second reviewer from another model family
judges the same diff, and both must pass.

Passes short-circuit, like gates do (§11.1): once one has said no, a
second opinion on the same diff changes nothing about what happens next
and costs another frontier invocation to learn it.

## `let review_workspace = workspace.gate_snapshot_for_candidate_in_store(`

Like gates, reviewers may inspect repository context beyond the
supplied diff. Give them the exact staged candidate, never ignored
worker inputs or residue from the authoritative workspace.

## `&review::ReviewInvocations {`

`pass` is which reviewer in this attempt's ordered list, so
the two members the packet gives a review — `review_pass(n)`
and `review_reask(n)` — index the same `n`.

## `let unavailable = matches!(review.result, review::ReviewResult::Unavailable { .. });`

Read before the result is consumed: a judge that never ran is not
a judge that said no, and the ledger has to show which happened.

## `pub(super) struct LegacyReviewPasses;`

Runs a review pass through the legacy machinery, which is the only one.

**The seam's production implementation, and it lives here for a reason the
allowlist already records.** `review::run_review` writes transcripts through
`util::write_text`, outside any inventoried `RunDir` site — this file's
allowlist entry says so, and that `RunDir` "has no transcript site in the
frozen inventory, so there is no funnel to move it to inside this slice".
So the call is denied everywhere except the modules the legacy section
names, and `decisions.effect_site_inventory.mechanism` (2) forbids adding a
topology module to that list.

Both engines reach the review machinery through this one function: the
legacy path below, and the schema-4 driver through
[`super::topology::attempt::ReviewPasses`]. One implementation, two callers
— not a forwarder invented for the second.

## `pub(super) struct LegacyReviewInputPolicy;`

[`crate::workspace::Workspace`]'s review-input policy, for a caller that
holds a worktree path instead of a `Workspace`.

The legacy verification ladder is its first caller — through
`run_attempt`'s own `workspace`, not through this — and the schema-4 driver
is its second. One policy, two callers.

## `pub(super) fn review_failure(result: review::ReviewResult) -> Option<AttemptFailure> {`

Turn a review result into an attempt failure, or `None` if it passed.

## `pub(super) fn review_failure(result: review::ReviewResult) …` › `review::ReviewResult::Unavailable { status, detail } => {`

The judge could not run. That is an environment problem, not a
rejection of the code: it is attributed to the reviewer so the
ladder defers instead of blaming the implementer.

## `pub(super) fn review_failure(result: review::ReviewResult) …` › `if verdict.needs_human {`

§12: the reviewer declined to judge and asked for a person. That is not
a rejection of the code, so it must not spend an attempt or escalate —
it parks the task and asks.

## `pub(super) fn review_failure(result: review::ReviewResult) …` › `let contradictory = verdict.pass && !verdict.required_changes.is_empty();`

A pass carrying required changes contradicts itself, and the engine is
about to commit on the strength of it — fail closed and say why rather
than discard the blockers the reviewer took the trouble to write.

## `pub(super) fn review_failure(result: review::ReviewResult) …` › `let feedback = if verdict.required_changes.is_empty() {`

required_changes is what the retry gets back verbatim (§11.4).

## `pub(super) fn review_failure(result: review::ReviewResult) …` › `format!("review failed: {}", util::head(&summary, 400)),`

Head, not tail: the reviewer's first reason is its primary
finding, and that is what has to reach the user.

## `pub(super) fn load_artifacts(`

Artifacts this task should be judged against: its declared inputs, plus
the conventions brief whenever one exists (§11.2 injects it into every
downstream prompt).

## `let produced: Vec<&str> = task.artifacts_out.iter().map(|id| id.as_str()).collect();`

A task's own outputs are not evidence for judging it: the reviewer
would be validating the change against a standard the same attempt just
wrote. Declared inputs and the brief only.

## `pub(super) fn evaluate_outcome(`

Outcome-level failure reasons, before gates get a say.

## `OutcomeStatus::Completed => {`

§12: the marker is honoured only on a run that actually completed.
`detail` carries the agent's partial output on every failure path,
and the prompt puts the marker string in front of the agent on every
fresh attempt — so scanning before the status match let a timed-out
or rate-limited attempt reclassify itself as a question purely by
quoting its own instructions back. That silently defeated "a rate
limit defers rather than burning an attempt" (§19), which is most of
the point of dispatching on `FailureKind` at all.

## `if let Some(question) = worker_question(outcome.detail.as_deref()) {`

An agent that stopped to ask has not failed at anything —
punishing it for the empty diff its own question explains would
teach it never to ask, so this precedes the evidence rules.

## `Some(`

§11 evidence axis: an empty diff can never pass.

## `.with_feedback(format!(`

§19: the feedback is the transcript tail. Without it the retry
starts blind on a task already known to run long.

## `pub(super) fn worker_question(detail: Option<&str>) -> Option<String> {`

§12: a worker may flag a decision it should not make alone. Everything from
the marker onward is taken, so a multi-line question survives.

The LAST marker wins, matching the prompt's "end your message with it" and
`review.rs`'s rule for verdicts: models restate an instruction before acting
on it, so an earlier occurrence is an echo, not the question. The engine
itself puts the marker in front of the agent — the empty-diff feedback names
it verbatim — so an echo is the expected case, not a rare one.

## `pub(super) fn materialize_prompt(`

§14 prompt materialization: body + acceptance + artifact inputs + the
exact gate commands the agent is permitted to run (the allow rules are
exact-match, so the agent must know the literal strings), plus — on a
retry — why the last attempt did not pass (§11.4).

## `if let Some(retry) = retry {`

A resumed session already holds the task, the artifacts, and the rules;
re-sending them buys nothing and buries the one thing that changed.

## `for id in task.artifacts_in {`

Artifacts are real files in the run directory: a consumer is shown the
content that exists, never told to look for something nothing wrote.

## `if let Some(retry) = retry {`

Whatever earlier rungs learned travels with the task, even though the
conversation does not (§11.4).

## `fn feedback_section(feedback: &[Feedback], all: bool) -> String {`

What earlier attempts learned. `all` carries the accumulated history for a
fresh rung; otherwise only the most recent failure, which is what a
same-rung retry needs.

## `fn feedback_section(feedback: &[Feedback], all: bool) -> St…` › `if position == last {`

Only the newest failure carries its full output; older ones would
bury it, and the newest is the one still standing in the way.

## `pub(super) fn artifact_path(artifacts_dir: &Path, id: &str) -> PathBuf {`

Where an artifact lives for the duration of a run (§15 `artifacts/`).
