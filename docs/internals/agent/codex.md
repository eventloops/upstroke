# `src/agent/codex.rs`

Extended notes for [`src/agent/codex.rs`](../../../src/agent/codex.rs).

[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/agent/codex.rs).

The code defines current behavior. These notes preserve contracts and implementation
history. Search each backticked heading fragment separately in the source.

References below to `decisions.*` and `INV-20` use retired v0.2 planning identifiers.
They record implementation history and do not add current requirements.
[DESIGN.md](https://github.com/eventloops/upstroke/blob/master/DESIGN.md#retired-records)
is the living design authority.

## Module

OpenAI Codex CLI adapter (DESIGN.md §16) — a second pool, and a reviewer
from a different model family that costs nothing on the first one.

§13's capacity engine is built around several subscriptions with independent
windows, and until this adapter there was one that upstroke could actually
drive on its own: Copilot reaches OpenAI models, but through GitHub's
harness and GitHub's billing.

**Implementing works where the sandbox is real, and only there.** This
CLI's sandbox is an external helper, present on Linux and absent on
Windows — so on Windows `exec` silently degrades to read-only and
[`refuse_edit_profile`] turns an implementer away at build time rather than
letting it spend attempts on empty diffs. On Linux the same flags write
inside the workspace and are blocked outside it, which is what §20 asks
for, so the implementer path is open. The evidence for both lives on that
function.

The judge's seat works everywhere: `read-only` is enforced on every
platform, the family is genuinely different from Anthropic's (§11.3), and a
review that spends nothing on the Claude window is worth having on its own —
measured end to end on run `01KZRN48A4ZK3AEDST3RJ8HMA4`, where Sonnet
implemented and this adapter judged.

**Two command shapes, not one with a flag swapped.** `codex exec` and
`codex exec resume` accept *different* flag sets: resume takes no `-s`, no
`-C`, no `--profile`. That is not a gap to work around. The sandbox is a
property of the session, fixed when it is created and inherited by every
resumed turn — which is exactly upstroke's model, where a same-rung retry has
the same profile by definition (§11.4). Observed 2026-08-11 against
codex-cli 0.147.0: a resume with no sandbox flag ran under the policy its
session recorded.

**The prompt goes on stdin, as `-`.** Windows caps a command line at ~8,191
characters and a review prompt carries up to
[`crate::review::MAX_DIFF_BYTES`] of diff, so argv was never an option. The
CLI also *waits* on stdin when it expects input ("Reading additional input
from stdin…"), so the payload must always be written and the pipe always
closed — [`super::proc`] does both, and an adapter that returned an empty
payload here would hang every attempt until the wall-clock timeout.

**stdout is JSONL, stderr is tracing.** `--json` emits one event per line —
`thread.started`, `turn.started`, `item.started`, `item.completed`,
`turn.completed` — while stderr carries `ERROR codex_api::…` log lines.
Only stdout is parsed; stderr survives in the transcript for whoever is
debugging.

**What this route reports, and what it does not.** A session id worth
resuming (`thread_id`), the final message, and token usage — but no
dollars. Tokens are recorded on the attempt and `cost_reporting` stays
false, so the ledger keeps saying `?` for these routes rather than
inventing a price. Pricing them here would mean a rate table inside a
published binary, going stale silently, to produce a figure that is
notional twice over on subscription auth where the marginal dollar is zero.
§13 already has the words: an estimate that flatters is worse than none.

**Two of this CLI's own features are deliberately unused for model turns.**

`codex review` runs a code review non-interactively, and adopting it would
swap the standard. §11.3's second opinion is *the same standard, a
different judge*: upstroke's review prompt carries the task's acceptance
criteria, the anti-sycophancy framing, the `DATA UNDER REVIEW` fencing and
the operator's decisions (§12). A verdict from OpenAI's own rubric applied
to a bare diff is not comparable with one from the Claude reviewer, and a
cross-family disagreement between them would be uninterpretable — the model
disagreeing, or the rubric? Reviews therefore run through plain `exec` with
`-s read-only`, like every other reviewer. This adapter cannot even tell it
is reviewing; it sees [`PermissionMode::ReadOnly`] and nothing else, and
that is the right amount to know.

`--output-schema` would force the model's final message into a JSON shape,
which is tempting for §7 verdicts — but it would make a third copy of the
verdict shape (prompt, parser, schema) that can drift, hold two reviewers to
two different contracts, and push the reviewer's prose into escaped strings
where humans read it. The existing re-ask-on-unparseable path already covers
the failure it would prevent, and nothing has yet measured that failure
happening. Revisit if real runs show it firing more than rarely. Pre-flight
does pass a deliberately missing schema path to the CLI's local parser; that
is a zero-spend guard proving the exact reasoning key before any model turn,
not an output contract for a turn.

**Never passed:** `--dangerously-bypass-approvals-and-sandbox`,
`--dangerously-bypass-hook-trust`, `-s danger-full-access`. §20 grants the
narrowest surface that lets the work happen, and there is no task for which
the answer is "turn the sandbox off". `--ephemeral` is also never passed —
it would discard the session that §11.4's same-rung retry resumes.

Surface captured from `codex --help`, `codex exec --help` and
`codex exec resume --help` at 0.147.0, and verified by running it.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `const PROBE_TIMEOUT: Duration = Duration::from_secs(60);`

Budget for one probe call. Generous for the same reason Copilot's is: §19
makes a probe failure a refusal to START, so a slow machine that times out
here loses a whole run rather than one attempt. Paid once per run.

## `const CONFIG_PROBE_UNKNOWN_KEY: &str = "upstroke_probe_deliberately_unknown";`

The strict-config control must be rejected before the missing-schema guard.
If it is not, a CLI has stopped enforcing the parser contract this probe
relies on and an apparently successful effort-key check would mean nothing.

## `const REQUIRED_EXEC_FLAGS: [&str; 5] = ["--json", "--sandbox", "--model", "-c", "--config"];`

Flags `exec` must still advertise, checked at pre-flight.

Every one is load-bearing rather than decorative: without `--json` there is
no session id and no usage, and without `--sandbox` a reviewer could edit
the code it is judging. A CLI that has dropped one of these must refuse the
run up front, not fail attempts once it is already spending (§19).

## `mod probe_ordinal {`

Which of this adapter's pre-flight processes each identity is.

Named rather than counted, for the reason [`super::probe_request`] gives —
and this is the adapter that made the reason concrete. Binary resolution
here used to *spawn*, once per PATH candidate, and to cache the answer, so
the second `probe()` in one process performed none of those spawns; a
counter would have renumbered every capability step on the second call, and
two pre-flights of one machine would have minted different identities for
the same work.

**That variable-length step is gone** — the adapter names its CLI and the
boundary resolves it (`PR4-ADAPTER-RESOLVES-ON-THE-HOST`), so every process
this adapter starts is now a fixed, named step. The table below is
therefore the whole domain, which is what
`every_preflight_process_has_its_own_ordinal` asserts.

## `mod probe_ordinal` › `pub const CONFIG_BASE: u32 = 3;`

The six strict-config parser probes: two surfaces x
{unknown-key control, xhigh, max}. `CONFIG_BASE + surface * 3 + step`.

## `mod probe_ordinal` › `pub const ALL: [u32; 12] = [`

Every fixed ordinal above, for the uniqueness assertion.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {` › `let fresh_help = runner.run(&probe_request(`

Fresh and resumed attempts are different CLI surfaces. Both carry
the reasoning override, so both must prove `--config` before spend;
only fresh attempts carry the sandbox.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {` › `let models = runner.run(&probe_request(`

The strict local parser above proves the exact key and the two role
policy values. The CLI's local catalog is separate zero-spend
evidence for each model × effort pair, so require every known Codex
model to expose every shared effort level before a run can start.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {` › `json_output: true,`

Asked for and parsed, unlike Copilot's route where the flag's
existence would promise an envelope no caller reads.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {` › `session_resume: true,`

`codex exec resume <id>` — proven to round-trip: the resumed turn
returned the same `thread_id` and recalled the prior exchange.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {` › `cost_reporting: false,`

Tokens, not dollars. See the module header — this is a decision
about what upstroke is willing to claim, not a missing feature.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {` › `acp: false,`

The CLI has `mcp-server` and `app-server`, neither of which is
ACP, and this adapter spawns a process per attempt either way.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {` › `model_list: true,`

`debug models` is a local catalog rather than a network query;
probe validated it above and discovery exposes its slugs.

## `fn build(&self, run: &TaskRun) -> Result<CommandSpec, UpstrokeError> {` › `cli().spec(&build_args(run))`

The working root comes from the process, not from `-C`: `exec resume`
has no `-C`, and one mechanism that works for both shapes beats two
that have to agree. It is now the *runner's* cwd
(`RunnerRequest.workspace`) rather than one this adapter set, which
is DESIGN.md:118's split and changes nothing about the mechanism.

`cli()` names the CLI and the runner decides which file that is, so
`build` performs no lookup of any kind and sends exactly the program
string `probe` certified. `build` being data-only used to force a
second, non-spawning resolution path beside the probing one; there is
now one path, and it is a function of its argument.

## `impl AgentAdapter for CodexAdapter` › `fn discover`

The one thing this CLI does better than either incumbent: it answers
"am I signed in?" without spending anything.

`codex login status` is non-interactive, exits 0 either way, and prints
`Logged in using ChatGPT` or `Not logged in` (observed 2026-08-11).
Copilot's adapter has to report [`AuthState::Unknown`] because GitHub
documents no such query; here the honest answer is a real one, so
`upstroke connect` writes a pool an operator can trust rather than a
shrug.

## `impl AgentAdapter for CodexAdapter` › `fn materialize_permissions(`

Nothing to reference — permissions are argv here, as they are for
Copilot — but the audit file is still written, because §15 calls
`settings/<task>-<attempt>.json` the per-attempt permission surface and
a trail that exists for one agent and silently not another is worse than
none.

## `impl ConfigProbeSurface` › `const fn index(self) -> u32 {`

Which surface this is, so its three parser probes get their own block
of invocation ordinals.

## `struct MissingOutputSchema {`

A unique empty directory whose child path is guaranteed not to exist.
Codex validates `--output-schema` locally before starting a turn, making
that absent child a deterministic, zero-spend stopping point.

## `fn drop(&mut self)` › `let _ = std::fs::remove_dir(&self.dir);`

The child is intentionally never created. Avoid recursive cleanup:
if a surprising CLI did write anything, preserving it is safer than
deleting an unexpected artifact.

## `for (step, effort) in [Effort::XHigh, Effort::Max].into_iter().enumerate() {`

These are the two policy values Upstroke promises for the roles this
feature introduced. Model catalogs validate the remaining shared
values separately; accepting either assignment here proves the exact
key, while checking both catches a provider-side enum regression.

## `fn effort_flag(effort: Effort) -> &'static str {`

This CLI's name for a tier-neutral effort level.

One-to-one today, and a function rather than a `Display` impl because that
is the adapter's job: the mapping belongs on this side of the seam where a
vendor can differ without the engine learning about it. Every value below
is in the provider's validated enum (`low, medium, high, xhigh, max` plus
`none` and `minimal`) — checked against the 400 it returns for anything else.

## `fn sandbox_mode(profile: &WorkerProfile) -> &'static str {`

The sandbox this profile runs under (§20).

Two modes and no third: `danger-full-access` exists on this CLI and is
never used. A reviewer may read and nothing else, because a reviewer that
edits the code it is judging has invalidated its own verdict.

## `fn edit_refusal(profile: &WorkerProfile) -> Option<UpstrokeError> {`

Why an implementer is refused **on Windows only**, and what was measured.

This CLI's sandbox is an external helper. `codex doctor` reports it as
`linux helper: <path>` where one exists and `none` on Windows — where there
is therefore nothing to enforce a boundary with. The consequence is a rule
the binary states itself:

> `approval_policy = "never"` cannot be used because requirements do not
> allow `sandbox_mode = "danger-full-access"`; Codex would fall back to
> read-only permissions with approvals disabled.

`exec` is non-interactive, so it forces `never`. With no enforceable
sandbox that degrades to read-only, and `--sandbox workspace-write` is
*accepted and then ignored*: exit 0, no warning, no diff. The silence is the
dangerous part — run `01KZRMHA28M5CM88VAXP613X9P` spent both attempts on
empty diffs and parked asking for write access it had been granted.
`-c approval_policy="on-request"` and `-c permission_profile="…"` were both
tried; `exec` wins.

The only mode that writes there is `--approve-for-me`, which routes
approvals through an automatic reviewer rather than a human — and it is not
a sandbox. Asked to write outside the repository it did so, and
`sandbox_workspace_write.writable_roots` did not constrain it. §20 grants
permission by mechanism, not by asking an LLM nicely, and §14's rollback is
`git clean -fd` *inside* the workspace: anything written outside it survives
a failed attempt, which is the one thing the design rules out.

**On Linux the sandbox is real and none of this applies.** Same CLI, same
flags, helper present: `--sandbox workspace-write` writes inside the
workspace and is *blocked* outside it — both measured. So the refusal is
scoped to the platform that cannot enforce it, and the implementer path is
open everywhere else.

One trap worth recording for whoever containerises this: Docker's default
seccomp profile blocks the syscalls the sandbox needs to initialise, and the
failure is a *different* message ("the workspace sandbox failed to
initialize") with the same empty-diff result. Granting
`--security-opt seccomp=unconfined --cap-add SYS_ADMIN` let it initialise;
which of the two is strictly required was not isolated.
The platform gate, kept out of [`AgentAdapter::build`] so it is testable on
a machine with no codex installed — the same reason [`build_args`] is its
own function.

## `pub fn build_args(run: &TaskRun) -> Vec<String> {`

Argument list, kept separate from binary resolution so it is testable on a
machine with no CLI installed.

Two shapes, because the CLI has two. A fresh attempt sets the sandbox that
the session will carry; a resumed one inherits it and would be rejected for
passing `-s` at all (observed: exit 2, "unexpected argument '-s' found").

## `pub fn build_args(run: &TaskRun) -> Vec<String>` › `args.push("--model".to_owned());`

Passed on both shapes even though a resumed session already knows its
model: the recorded command should say what it ran on without a reader
having to open the session file, and a future change to the CLI's
default must not silently move a resumed retry to another model.

## `pub fn build_args(run: &TaskRun) -> Vec<String>` › `if let Some(effort) = run.profile.effort {`

Effort, for exactly the reason the model is passed above — and this axis
had the bug that argument was written to prevent. This CLI's default
comes from the *provider's* roster, not from the flag set: `gpt-5.6-sol`
carries `default_reasoning_level: low`, so every review this project ran
before this line existed was judged at the lowest setting, silently, and
a roster refresh could move it again without a release. Passed on the
resumed shape too: `-c` is accepted there (measured — unlike `-s`, which
is rejected), and a retry must not think harder or less hard than the
attempt it is continuing.

## `pub fn build_args(run: &TaskRun) -> Vec<String>` › `args.push("-".to_owned());`

`-` is "read the prompt from stdin" and must be last: everything after it
would be taken as the prompt's own arguments.

## `fn parse_output(out: &ProcessOutput) -> Outcome {`

Outcome parsing over the JSONL event stream.

Defensive throughout, like every other adapter here: a line that is not JSON
is skipped rather than failing the attempt, and a missing field degrades the
status instead of panicking. The engine owns `diff` and `transcript_path`.

## `fn parse_output(out: &ProcessOutput) -> Outcome` › `message = item`

Last one wins: the final message is the agent's answer,
and it is the field a reviewer's verdict travels in.

## `fn parse_output(out: &ProcessOutput) -> Outcome` › `usage = Some(add_usage(usage, event.get("usage")));`

Summed rather than replaced. One invocation emitted exactly
one of these, tool call and all (measured), so this is
defence against a future version that reports per step —
where taking the last would quietly under-count.

## `fn parse_output(out: &ProcessOutput) -> Outcome` › `let joined = errors.join("\n");`

Failures only — a successful task *about* rate limiting must never read
as the pool being exhausted (see `looks_rate_limited`).

## `fn parse_output(out: &ProcessOutput) -> Outcome` › `outcome.detail = [`

The `error` events first: on this route stderr is a tracing log, so the
event stream carries the diagnostic a human actually wants. An
unauthenticated run exits 101 with 401s here, which is an agent error
and not a rate limit — a distinction the ladder acts on.

## `fn add_usage(total: Option<Usage>, reported: Option<&Value>) -> Usage {`

Fold one `turn.completed`'s usage into the running total.

`reasoning_output_tokens` is a *subset* of `output_tokens` on this CLI, not
an addition to it, so it is carried across rather than added in — summing
both would double-count the thinking.

## `fn add_usage` › `add(`

Vendor names differ; the concepts line up. `cached_input_tokens` is a
read from the cache, `cache_write_input_tokens` is a write into it.

## `fn add_usage` › `total.num_turns = Some(total.num_turns.unwrap_or(0) + 1);`

One `turn.completed` is one turn, so this counts them for free.

## `fn parse_login_status(out: &ProcessOutput) -> Discovery {`

Read `codex login status`, as defensively as everything else here.

Observed forms (0.147.0): `Not logged in`, and `Logged in using ChatGPT`.
The negative is checked first because it contains the positive as a
substring — matching "logged in" first would call a signed-out account
signed in, which is the one error `AuthState` exists to prevent.

## `fn parse_login_status(out: &ProcessOutput) -> Discovery` › `if text.contains("chatgpt") {`

§13's two billing shapes. A ChatGPT plan is a rate-limit window; an API
key is metered dollars. Anything else is left for the caller's documented
default rather than guessed at.

## `const CLI: &str = "codex";`

---------------------------------------------------------------------------
The CLI this adapter names, and what to tell an operator whose boundary
does not have it. `super::bin` owns the mechanics.
---------------------------------------------------------------------------

## `const CLI: &str = "codex";`

This CLI, as the boundary that will execute it names it.

One name, not a platform-dependent candidate list. This adapter used to
resolve the name against the coordinator host's `PATH`, spawning once per
candidate to skip a Windows Store package payload that is visible to a
filesystem lookup but returns access denied when spawned. Both halves were
answers to a question an adapter may not ask: the boundary that executes
the CLI is the thing that knows which file the name is, and with a
container runner it is not this machine's filesystem at all
(`PR4-ADAPTER-RESOLVES-ON-THE-HOST`). Skipping an unspawnable candidate is
now the job of whatever resolves the name; so is `PATHEXT`.

## `const INSTALL_HINT`

What to tell an operator whose boundary has no `codex`.

## `impl AdapterSource for CodexAdapter {`

Registry entry, so `by_id("codex")` resolves without this module being
reached through the concrete type.

## `mod tests` › `const FORBIDDEN: [&str; 4] = [`

Flags that would hand the agent the machine. §20 says none is ever
passed, so the list exists to be asserted against.

## `fn a_fresh_attempt_sets_its_sandbox_and_a_resumed_one_must_not` › `let fresh = build_args(&run(PermissionMode::Edit, None));`

The CLI's two shapes, which are not one shape with a flag swapped.
`exec resume` rejects `-s` outright — observed as exit 2, "unexpected
argument '-s' found" — because the sandbox belongs to the session and
is inherited. Passing it anyway would fail every same-rung retry for
a reason that has nothing to do with the code.

## `fn a_profile_without_an_effort_passes_none_rather_than_guessing` › `let mut run = run(PermissionMode::Edit, None);`

Only reachable from a hand-built profile: the engine sets an effort
on every profile it makes. Passing a guess here would be worse than
the CLI's own default, because it would look deliberate.

## `fn the_prompt_is_the_last_argument_and_it_is_stdin()` › `for resume in [None, Some("sess")] {`

Windows caps argv at ~8,191 bytes and a review prompt carries the
diff, so the prompt has never been passable as an argument. `-` says
"read it from stdin", and anything after it would be swallowed as the
prompt's own arguments.

## `fn the_prompt_is_the_last_argument_and_it_is_stdin()` › `let run = run(PermissionMode::Edit, None);`

And the payload is actually written, or the CLI sits waiting on a
pipe nobody closed.

## `fn an_implementer_is_refused_where_no_sandbox_can_enforce_it` › `let err = edit_refusal(&profile(PermissionMode::Edit))`

Windows has no sandbox helper (`codex doctor`: `linux helper: none`),
so `exec` degrades to read-only and writes nothing while returning 0.
Measured on run 01KZRMHA28M5CM88VAXP613X9P, which spent both attempts
on empty diffs and then parked asking for write access it had been
granted. A capability this platform cannot deliver is a refusal to
start (§19), not a task that fails after spending.

## `fn an_implementer_is_refused_where_no_sandbox_can_enforce_it` › `assert!(text.contains("Linux"), "{text}");`

And where to go instead: Linux, or another agent.

## `fn an_implementer_is_allowed_where_the_sandbox_is_real()` › `assert!(`

Same CLI, same flags, helper present: `--sandbox workspace-write`
wrote inside the workspace and was blocked outside it, both measured
in a container. The refusal above is scoped to the platform that
cannot enforce a boundary, not to the CLI.

## `fn a_reviewer_is_read_only_and_nothing_is_ever_given_the_machine` › `assert!(edit_refusal(&profile(PermissionMode::ReadOnly)).is_none());`

Never refused anywhere: read-only is enforced on every platform, and
it is the seat this adapter is most useful in.

## `fn a_successful_run_yields_its_session_message_and_tokens()` › `let stdout = r#"{"type":"thread.started","thread_id":"019ff122-4d61-7323-a217-843ddfe5932c"}`

The real event stream, from a tool-using run against codex-cli
0.147.0 on 2026-08-11.

## `fn a_successful_run_yields_its_session_message_and_tokens()` › `assert_eq!(outcome.duration, out.duration);`

What the supervisor measured, carried through unchanged: see the
same assertion in the Claude adapter for why it is asserted at all.

## `fn a_successful_run_yields_its_session_message_and_tokens()` › `assert_eq!(outcome.detail.as_deref(), Some("hi"));`

The agent's final message, not the command_execution item before it.
A reviewer's verdict travels in exactly this field.

## `fn a_successful_run_yields_its_session_message_and_tokens()` › `assert_eq!(outcome.cost_usd, None);`

Tokens, never a price: this route reports no dollars and upstroke does
not own a rate table.

## `fn several_turns_are_summed_rather_than_last_wins()` › `let stdout = r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":2,"reasoning_output_tokens":1}}`

One invocation emits one `turn.completed` today, tool call and all.
This is the guard for a version that reports per step, where taking
the last would silently under-count the run.

## `fn several_turns_are_summed_rather_than_last_wins()` › `assert_eq!(usage.reasoning_output_tokens, Some(5));`

Carried, not double-counted: reasoning tokens are a subset of output.

## `fn an_unauthenticated_run_is_an_agent_error_not_an_exhausted_pool` › `let stdout = r#"{"type":"thread.started","thread_id":"t1"}`

Observed: five 401 retries then exit 101. The ladder acts on this
distinction — a rate limit defers and waits for a window, an agent
error spends an attempt — so calling a signed-out account "rate
limited" would park a run forever on a problem that never resolves.

## `fn junk_on_stdout_never_fails_an_attempt()` › `let stdout = "Reading additional input from stdin...\n\`

Warnings, progress chatter, a half-written line at a kill — none of
it is JSON and none of it should turn a finished attempt into a
failure.

## `fn signed_out_is_never_read_as_signed_in()` › `let signed_out = parse_login_status(&output(0, "Not logged in\n", ""));`

"Not logged in" contains "logged in", so order of checks is the whole
test: a confident wrong "you are signed in" writes a pool the
operator trusts and a run then fails against.

## `fn signed_out_is_never_read_as_signed_in()` › `let odd = parse_login_status(&output(0, "something new entirely\n", ""));`

Anything unrecognised stays Unknown and says so, rather than being
forced into one of the two answers.

## `mod tests` › `struct RecordingRunner {`

A Runner that records every request and answers each config-probe
surface the way a working `codex` does.

The answers are what let the sequence *complete*: a validator that
refuses stops the walk, and a walk that stops after one process cannot
say anything about the identities of the other five.
A boundary that answers every one of this adapter's pre-flight
processes, and records each request.

It answers by **argument**, never by program: what the CLI is called at
the boundary is the boundary's business, and a fixture that keyed on the
program string would be asserting the adapter's answer against itself.

## `impl Runner for RecordingRunner` › `return Ok(output(`

The control: the strict parser rejects the unknown key
*before* the local missing-schema guard.

## `impl Runner for RecordingRunner` › `return Ok(output(`

The key is accepted, and the run then stops on the schema
file that deliberately does not exist.

## `impl Runner for RecordingRunner` › `let models: Vec<_> = catalog::known_models(ADAPTER_ID)`

Every model the catalog knows, each advertising every
effort. Derived from the catalog rather than written out:
nothing here asserts *which* models exist, so the catalog is
an input to this fixture and the oracle for nothing.

## `mod tests` › `fn the_six_config_parser_probes_are_six_distinct_identities() {`

The six strict-config parser probes really are six identities.

`decisions.admission_and_leases.permits.invocation_identity`:
`InvocationId` is "unique **per process**", and `invariants[19]`
(INV-20) requires every Runner process to carry one. Two processes
sharing an identity collide in the invocation ledger and in every
invocation-derived containment scope.

`every_preflight_process_has_its_own_ordinal` asserts the *table*
`probe_ordinal::ALL`, which is hand-written and contains only the
**declared** ordinals. These six are **computed** — `CONFIG_BASE +
surface.index() * CONFIG_PER_SURFACE + step` — so a `ConfigProbeSurface::
Resume` whose `index()` returned `Fresh`'s left the six processes
carrying three identities with the whole suite green
(`PR5-CORRECTNESS-008`). The repair is to stop asking the table and
start asking the requests.

The invocation is built with [`Invocation::at`] rather than named, so
this drives the six config probes over an absolute program without
depending on what this machine has installed.

## `fn the_six_config_parser_probes_are_six_distinct_identities` › `assert!(`

And they are the probe form naming this agent, so a "distinct" set
cannot be six values of some other shape.

## `fn the_six_config_parser_probes_are_six_distinct_identities` › `let resumed = runner`

The two surfaces are really two: the resumed one carries `resume`
and the fresh one does not, so the six requests are six *different*
processes and not one repeated six times.

## `fn the_six_config_parser_probes_are_six_distinct_identities` › `let declared: BTreeSet<u32> = probe_ordinal::ALL.into_iter().collect();`

No computed ordinal may land on a declared one, which is the other
way this block can collide.

## `mod tests` › `fn preflight_starts_exactly_the_processes_the_ordinal_table_declares() {`

The twelve declared ordinals are **exactly** the processes this
adapter's pre-flight starts — no thirteenth, and none outside the table.

This replaces `every_binary_resolution_candidate_carries_its_own_identity`,
and the property it carries is the one that mattered: *no process this
adapter starts takes an identity the table does not enumerate.* That
test could only assert it of the variable-length block separately,
because binary resolution spawned once per unbounded PATH candidate and
no table could speak for it. The adapter now names its CLI and the
boundary resolves it (`PR4-ADAPTER-RESOLVES-ON-THE-HOST`), so the
variable-length block is gone and the claim can be made over the whole
domain at once, against the requests rather than against the table.

Both entry points, because `probe` and `discover` are separately
droppable and each has its own ordinals: ten and two.

## `fn preflight_starts_exactly_the_processes_the_ordinal_table_declares` › `let declared: BTreeSet<u32> = probe_ordinal::ALL.into_iter().collect();`

Every ordinal actually used is one the table declares. The table is
the expected value here and the requests are the result, which is
the direction that catches a step taking an ordinal nobody reserved.

## `mod tests` › `fn every_preflight_process_names_the_cli_at_whichever_boundary_is_asked() {`

Every pre-flight process this adapter starts names the CLI and nothing
this machine contributed — over the whole pre-flight, not one call.

The second field this holds constant is **the boundary**: the same
adapter is driven against two boundaries in one process, in both
orders, and each must be asked the identical program string. A
resolution memoised in a process-wide cell — which is what this adapter
had — is invisible to any test that constructs one runner, and would
hand the second boundary the first one's answer.

## `fn every_preflight_process_names_the_cli_at_whichever_boundary_is_asked` › `let programs = first.programs();`

`codex`, written here rather than read from `CLI`: a constant
compared against itself proves nothing.

## `mod tests` › `fn every_preflight_process_has_its_own_ordinal() {`

Every pre-flight process of this adapter carries its own identity.

`decisions.admission_and_leases.permits.invocation_identity` says
"unique **per process**", and this adapter runs 12 of them, so the
ordinals it fixes must be 12 distinct values. The expected count is
written here from the steps the adapter performs, not read from the
table under test — a table that lost an entry would otherwise agree
with itself.

## `fn every_preflight_process_has_its_own_ordinal()` › `let ids: BTreeSet<String> = probe_ordinal::ALL`

And they really do render as 12 distinct identities of the packet's
third form, which is the property the ordinals exist for.

## `mod tests` › `fn probe_against_real_binary_when_present() {`

Runs only where the real CLI exists; deterministic contract fixtures do
the compatibility proof, while this catches local help/catalog drift.

## `fn probe_against_real_binary_when_present()` › `if crate::util::find_program(CLI).is_none() {`

The host runner's boundary *is* this machine, so what gates this is
whether this machine has the CLI — asked of `util::find_program`
rather than of the adapter, which no longer knows.
