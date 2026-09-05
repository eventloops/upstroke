# `src/ir.rs`

Extended notes for [`src/ir.rs`](../../src/ir.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Core data model (DESIGN.md §7): the plan-side types `validate` consumes
and the execution-side types the agent adapters produce.

## `pub struct TaskId(pub String);`

Stable identifier for a task within a plan.

## `pub struct QuestionId(pub String);`

Identifier for one question raised during a run. Short enough to type at a
prompt: `upstroke answer <id>` (step 8) accepts any unambiguous prefix.

## `pub struct ArtifactId(pub String);`

Identifier for an artifact flowing between tasks (contracts, briefs).

## `pub enum Tier {`

Abstract capability tier. Ordering matters: `Small < Mid < Frontier`.

## `pub enum Effort {`

How hard the model should think, where the CLI exposes that axis.

Abstract for the same reason tiers are: the vocabularies differ per vendor
and the engine should not learn one of them. The five built-in adapter CLIs
now share these levels, so each adapter maps them explicitly rather than
inheriting a vendor default.

Codex also exposes `ultra`, documented as "maximum reasoning with automatic
task delegation". That remains deliberately unreachable: it changes what
the agent *does*, not only how hard it thinks, and nothing in this design has
audited an agent spawning its own subagents inside a upstroke attempt.

## `impl Effort` › `pub fn for_tier(tier: Tier) -> Self {`

The tier's default effort — what makes a tier mean something on a CLI
that has this axis.

Without it, a chain that escalates `small → mid → frontier` on one
vendor's models moves nothing at all on another's: codex reads the same
slug at whatever effort the vendor defaults to, which for `gpt-5.6-sol`
is `low`. Frontier maps to `high` rather than `max` because §23.2 prices
review per attempt and a reviewer binds at the review tier; `max` is a
deliberate opt-in through a pin, not the price of routing something to
the top rung.

## `pub struct ResolvedEffortPolicy {`

The concrete effort standard one run resolved before it spent anything.

The three tier fields apply to implementation attempts; `review` applies to
every review pass. Keeping the values explicit rather than positional makes
the event record readable and prevents a future tier-order change from
silently reinterpreting an old run.

## `pub struct PlanSource {`

Where a plan came from: which adapter parsed it and a content hash of the
original text, so a run can detect that its plan file changed underneath it.

## `pub struct Artifact {`

Artifact stub — full artifact handling (files on disk, injection into
prompts) arrives with execution; validate only tracks identity and wiring.

## `pub enum PermissionMode {`

What an agent subprocess may touch (§20). Edit profiles get file tools and
the gate commands; reviewers are read-only. Neither gets network tools.

## `pub struct WorkerProfile {`

§7 `WorkerProfile` — v2.1: an optional PIN. Tiers bind late by default; a
profile forces a fixed binding for one tier.

## `pub struct WorkerProfile` › `pub agent: String,`

Agent adapter id: `claude-code` | `copilot` | `aider`.

## `pub struct WorkerProfile` › `pub pool: String,`

Which capacity pool this profile drains (identity only until the
capacity engine lands).

## `pub struct WorkerProfile` › `pub effort: Option<Effort>,`

Reasoning effort for adapters that have the axis (§16: codex does).

`Some` on every profile the engine builds — the tier's default, or a
pin's override. `None` means "whatever the CLI defaults to", which is
the behaviour this field exists to end: it is reachable only from tests
that construct a profile by hand.

## `pub struct Usage {`

Token accounting as reported by the agent CLI, parsed defensively — any
field may be absent and absence never fails an attempt.

## `pub struct Usage` › `pub reasoning_output_tokens: Option<u64>,`

Output tokens spent thinking rather than answering, where a CLI
separates the two. Vendor-neutral in name because the concept is:
Codex reports `reasoning_output_tokens`, and it is a *subset* of
`output_tokens` rather than an addition to it, so summing the two would
double-count.

## `pub struct Outcome {`

§7 `Outcome` — what one agent attempt produced. The adapter fills status,
session, usage, and cost from process output; the engine owns `diff`
(invariant 3: ground truth is the engine-captured diff) and
`transcript_path`.

There is no per-attempt pool field here. §13's second currency is recorded
where the attribution actually lives — `AttemptRecord.pool` and
`ReviewRecord.pool`, set by the engine from the pools file — because an
adapter has no idea which subscription the engine bound it to. A stub that
every adapter filled with `None` and nothing ever read was a second,
dead mechanism for a job the first one does.

## `pub struct Outcome` › `pub detail: Option<String>,`

The agent's own account of what happened — its final message, or the
error text for a failure. Most CLI failures arrive through the JSON
body with an empty stderr, so without this a report has nothing to
show the user.

## `pub struct Outcome` › `pub cost_usd: Option<f64>,`

API-equivalent dollars as reported by the CLI (subscription spend is
notional — §13).

## `pub struct Verdict {`

§7 `Verdict` — a reviewer's structured judgement of one diff. `pass` is
the only thing the ladder branches on; `required_changes` becomes the
retry feedback (§11.4).

## `pub struct Verdict` › `pub needs_human: bool,`

§12: the reviewer may decline to judge and ask for a human instead.
Defaulted so a verdict written before this field existed still parses,
and so silence means "I judged it" rather than "escalate".

## `pub enum QuestionKind {`

§7 `Question` — why the run is asking, and exactly which tasks park for it.

## `pub enum QuestionKind` › `Unblock,`

Nothing else can move this task forward: the chain is exhausted, or a
pool stayed down. The human is the top rung (§11.4).

## `pub enum QuestionKind` › `ApproveSpend,`

Spend crossed an `ask_before` threshold. Raised once budgets exist
(§12); the variant is here so the shape is settled.

## `pub enum QuestionKind` › `Continue,`

Proceed / stop at a milestone.

## `pub enum QuestionKind` › `Clarify,`

A worker or reviewer hit a decision it should not make alone (§12).

## `pub struct Question {`

§7 `Question`. `affected_tasks` is load-bearing, not descriptive: exactly
those tasks park, and everything else keeps running (invariant 6).

## `pub struct Question` › `pub context: String,`

Human-facing framing. Any agent-authored text inside it is quoted and
labelled as such by whoever built the question.

## `pub enum Answer {`

What came back — or did not. `Unanswered` is not a decline: it means no
channel could reach a human at all (CI, detached terminal), which parks the
task rather than failing it (§12).

## `pub fn content_hash(bytes: &[u8]) -> String {`

FNV-1a 64-bit content hash. Dependency-free and stable across platforms and
releases — identity only, nothing cryptographic. CR bytes are skipped so a
plan checked out with CRLF hashes the same as the LF original (git's
autocrlf would otherwise make the same plan look changed across machines).

## `fn a_verdict_without_needs_human_is_a_judgement_not_an_escalation() {` › `let verdict: Verdict =`

Silence must mean "I judged it". A verdict written before the field
existed, or by a model that ignored it, must not park the task.

## `fn answers_round_trip_and_keep_declined_apart_from_unanswered() {` › `for answer in [`

The distinction decides whether a task Fails or parks (§12), so it
has to survive serialization.

## `fn task_kind_all_lists_every_variant_exactly_once_in_order()` › `fn successor(kind: TaskKind) -> Option<TaskKind> {`

The compile-time canary: no wildcard arm, so an eighth TaskKind
variant refuses to build until it is placed in this chain — and
the walk below then refuses to pass until `ALL` carries it too.

## `fn task_kind_all_lists_every_variant_exactly_once_in_order()` › `break;` (trailing)

a cycle in `successor` must fail the assert, not hang the test

## `fn task_kind_all_lists_every_variant_exactly_once_in_order()` › `for kind in TaskKind::ALL {`

The other hand-kept lists agree with the enum: Display -> parse
round-trips, and serde's derived name is the string Display prints.
