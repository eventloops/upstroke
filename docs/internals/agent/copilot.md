# `src/agent/copilot.rs`

Extended notes for [`src/agent/copilot.rs`](../../../src/agent/copilot.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

GitHub Copilot CLI adapter (DESIGN.md §16) — the multi-vendor pool.

One subscription reaches Anthropic, OpenAI, and Google models through one
harness, which is what makes §11.3's cross-vendor second opinion a `--model`
flag rather than a second product.

**Route A, not ACP (v0.1).** §16 names two routes and prefers ACP "once
stable for us". It is not yet: neither `--acp` nor `--stdio` appears in
GitHub's programmatic CLI reference, so there is no documented surface to
pin known-good behaviour against — and pinning per version is exactly what
§16 says this adapter must do. ACP also needs a persistent bidirectional
JSON-RPC session, where every other part of v0.1 spawns a process, feeds it,
and reads what came back ([`super::proc`]). `probe()` still records
[`Caps::acp`], so switching routes stays a change inside this file.

**The prompt goes on stdin, and there is no `-p`.** GitHub documents
`echo "…" | copilot` as a programmatic form, and documents that "piped input
is ignored if you also provide a prompt with the `-p` option" — so passing
both would silently discard the real prompt. Stdin is also the only delivery
that works: npm installs this CLI as `copilot.cmd` on Windows, and a batch
target is spawned through the command processor whoever does it — so the
~8,191-character command line applies, while a review prompt carries up to
[`crate::review::MAX_DIFF_BYTES`] of diff.

**What this CLI does not give us**, recorded honestly rather than guessed at:
no JSON envelope (so no session id, no usage, no cost — the ledger shows
Copilot attempts as unpriced), and no documented session resume (so §11.4's
same-rung retry starts fresh with accumulated feedback instead of resuming a
conversation). Both are `Caps` axes the engine already dispatches on.

Two further gaps, named rather than papered over: `max_turns` has no
counterpart here, so a per-profile turn cap does not apply to Copilot
attempts (the wall-clock timeout is the only bound); and whether
`--no-ask-user` also suppresses *tool-permission* prompts is undocumented,
so an un-allowed tool could in principle hang an attempt until that timeout.

**Permissions are argv** (§20). There is no settings file and no path-deny
surface as Claude Code has, so the guarantee is the allow-list plus §15's
split: an allow-list that names exactly the gate commands, no URL grant at
all, and never a skip-all flag. That rests on un-allowed tools being denied
by default — which `--allow-url`'s existence implies but nothing here can
verify without the binary, so the reviewer profile denies `write` and
`shell` outright rather than trusting it where the stakes are highest.
Docs:
<https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference>
(flags verified Aug 2026).

## `#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `const PROBE_TIMEOUT: Duration = Duration::from_secs(60);`

Budget for one probe call.

Sixty seconds for `--version` looks absurd until you watch it: this CLI is a
Node program behind an npm shim that runs an update check on startup, and it
takes ~5s warm on an unloaded machine. Under a loaded one — a full test
suite, a CI runner, a laptop doing anything else — 15s was not enough, and
the failure mode is the expensive one: §19 makes a probe failure a refusal
to START, so a slow machine loses the whole run rather than one attempt.

Probing happens once per run at pre-flight, so the cost of being generous
here is bounded and paid only when something is genuinely wrong. Waiting a
minute before refusing beats refusing a working machine in fifteen seconds.

## `const REQUIRED_FLAGS: [&str; 5] = [`

Long flags this adapter passes. §16: this CLI auto-updates and has removed
programmatic flags without deprecation, so a missing one must surface as a
pre-flight refusal rather than as per-task failures once a run is already
spending (§19).

## `const REQUIRED_SHORT_FLAGS: [&str; 1] = ["-s"];`

Short flags this adapter passes, checked separately because a substring
search for them is worthless: `"-s"` occurs inside `--settings`, `--share`
and `--stdio`. Since none of `Caps`' other fields drives behaviour yet, this
refusal is most of what probing actually buys.

## `mod probe_ordinal {`

Which of this adapter's pre-flight processes each identity is. See
[`super::probe_request`] for why these are named rather than counted.
`discover` spawns nothing here — this CLI answers no auth query — so there
are two.

## `mod probe_ordinal` › `pub const ALL: [u32; 2] = [VERSION, HELP];`

Every ordinal above, for the uniqueness assertion.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, Upstro…` › `json_output: false,`

False even if a JSON flag exists: `Caps` describes what this
adapter's route delivers, and Route A neither asks for JSON nor
parses it. Reporting the flag would promise a structured envelope
no caller could read.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, Upstro…` › `session_resume: has("--resume"),`

Optional capabilities stay pessimistic. Required surfaces were
proven above; an unreadable help is a refusal, not permission to
assume support.

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, Upstro…` › `cost_reporting: false,`

No JSON envelope on this route, so nothing reports spend. The
ledger says so rather than recording zero (§13).

## `fn probe(&self, runner: &dyn Runner) -> Result<Caps, Upstro…` › `read_only_mode: true,`

No single flag; achieved by denying the write and shell tools.

## `fn build(&self, run: &TaskRun) -> Result<CommandSpec, Upstr…` › `cli().spec(&build_args(run))`

No `current_dir`: the runner owns cwd (DESIGN.md:118). No
resolution either: `cli()` names the CLI and the runner decides
which file that is, so this and `probe` send one program string by
construction.

## `impl AgentAdapter for CopilotAdapter` › `fn discover(&self, _runner: &dyn Runner, caps: &Caps) -> Result<Discovery, UpstrokeError>…`

Honestly, almost nothing — and the same pessimistic temperament as this
adapter's `Caps`.

GitHub's programmatic CLI reference documents no non-interactive auth
query and no model listing (checked Aug 2026), so there is nothing to
subprocess that would answer either question. Reporting
[`AuthState::Unknown`] is the truthful result: inferring "signed in"
from the binary merely existing would put a confident wrong line in a
file the operator then trusts.

The `probe()` this runs beside is what has actually been load-bearing
here, and it still is: [`Caps::model_list`] gates any future
enumeration, so the day this CLI grows one, `connect` starts
cross-checking the catalog without another decision being made.

## `fn discover(&self, _runner: &dyn Runner, caps: &Caps) -> Re…` › `let invocation = cli();`

Named rather than located: this reports on the CLI a run would
execute, and which file that is belongs to the boundary that
executes it. Nothing here asks this machine what it has.

## `fn discover(&self, _runner: &dyn Runner, caps: &Caps) -> Re…` › `Ok(discovery)`

§13 gives Copilot two billing shapes — credits (post-Jun 2026) and
legacy premium requests — and nothing this CLI prints distinguishes
them. `shape: None` is what makes the writer say so in the file.

## `impl AgentAdapter for CopilotAdapter` › `fn materialize_permissions(`

Nothing to reference: permissions ride on argv, so this returns `None`
and the command carries them itself.

The file is still written. §15 calls `settings/<task>-<attempt>.json`
"the per-attempt permission surface", and an audit trail that exists for
one agent and silently not for another is worse than none — someone
reading a run tomorrow should be able to see what each attempt was
allowed to do without reconstructing it from this source file.

## `fn effort_flag(effort: Effort) -> &'static str {`

Argument list, kept separate from binary resolution so it is testable on
machines without the CLI installed.

## `pub fn build_args(run: &TaskRun) -> Vec<String>` › `let mut args = vec![`

NOTE: no `-p`. The prompt arrives on stdin, and GitHub documents that
piped input is ignored when `-p` is also given — passing both would send
the CLI an empty task and discard the real one.

## `pub fn build_args(run: &TaskRun) -> Vec<String>` › `"-s".to_owned(),`

Only the agent's response on stdout, with no stats or decoration
around it: `parse_output` treats stdout as the final message, and
that message is where a reviewer's verdict travels.

## `pub fn build_args(run: &TaskRun) -> Vec<String>` › `"--no-ask-user".to_owned(),`

An unattended run must never sit waiting on a clarifying question.

## `pub fn build_args(run: &TaskRun) -> Vec<String>` › `args.extend(permission_args(&run.profile, &run.gate_cmds));`

`profile.max_turns` has no counterpart on this CLI and is therefore
NOT applied — see the module header. Nothing sets it today, and it is
named here rather than silently skipped so that whoever first does has
to decide what an unbounded Copilot attempt should cost.

## `pub fn build_args(run: &TaskRun) -> Vec<String>` › `if let Some(session) = &run.resume_session {`

Only reachable on a build whose `--help` advertises `--resume`, because
that is what sets `Caps::session_resume` and the engine will not offer a
session otherwise. Honouring it here means a future release that ships
the flag needs no change beyond the probe noticing it.

## `pub fn permission_args(profile: &WorkerProfile, gate_cmds: &[String]) -> Vec<String> {`

The per-attempt permission surface as argv (§20).

Edit profiles get the write tool and *exactly* the configured gate commands;
reviewers get neither. Nobody is granted a URL, so network access stays
behind a permission this adapter never gives — and with `--no-ask-user` the
agent cannot ask for one either.

Reading is not granted explicitly because this CLI allows the working
directory by default and `--add-dir` is what widens that; the engine never
widens it, so an agent sees the workspace and nothing else.

## `pub fn permission_args(profile: &WorkerProfile, gate_cmds: …` › `args.push("--deny-tool=write".to_owned());`

Denied rather than merely not-allowed: a reviewer that edits the
code it is judging invalidates the verdict, and one that runs
commands is executing the very diff under review.

## `fn parse_output(out: &ProcessOutput) -> Outcome {`

Outcome parsing for a CLI with no JSON envelope.

With `-s` the whole of stdout is the agent's final message, so that is what
lands in `detail` on success — the field a reviewer's verdict is read from
(step-6 finding #1: leaving it empty makes every review unparseable). Diff,
transcript path, and pool drain are engine-owned and left empty here.

## `fn parse_output(out: &ProcessOutput) -> Outcome` › `session_id: None,`

No JSON envelope: nothing to read a session, usage, or cost from.

## `fn parse_output(out: &ProcessOutput) -> Outcome` › `outcome.status = if looks_rate_limited(&out.stderr) || looks_rate_limited(response) {`

Rate-limit detection applies to failures only — see `looks_rate_limited`.

## `fn parse_output(out: &ProcessOutput) -> Outcome` › `let stderr = out.stderr.trim();`

Give the engine something to report without opening the transcript.
stderr first: on a failure it carries the diagnostic, while stdout may
hold half an answer.

## `const CLI: &str = "copilot";`

---------------------------------------------------------------------------
Binary discovery — npm ships this as copilot.cmd on Windows, which
CreateProcess cannot exec directly; `super::bin` owns the mechanics.
---------------------------------------------------------------------------

## `const CLI: &str = "copilot";`

This CLI, as the boundary that will execute it names it.

One name, not a platform-dependent candidate list: choosing between
`copilot.exe`, `copilot.cmd` and `copilot.bat` was a filesystem lookup, and
this adapter no longer performs one. See [`Invocation::named`].

## `const INSTALL_HINT: &str = "Install the GitHub Copilot CLI there (npm install -g @github/…`

What to tell an operator whose boundary has no `copilot`.

## `mod tests` › `const SKIP_ALL_FLAGS: [&str; 6] = [`

Permission flags that hand an agent the whole machine, and the URL grant
that would put it on the network. §20 says none of these is ever used,
so the list lives here: its only job is to be asserted against.

## `mod tests` › `fn every_preflight_process_has_its_own_ordinal() {`

Every pre-flight process of this adapter carries its own identity.

`decisions.admission_and_leases.permits.invocation_identity` says
"unique **per process**", and this adapter runs 2 of them, so the
ordinals it fixes must be 2 distinct values. The expected count is
written here from the steps the adapter performs, not read from the
table under test — a table that lost an entry would otherwise agree
with itself.

## `fn every_preflight_process_has_its_own_ordinal()` › `let ids: BTreeSet<String> = probe_ordinal::ALL`

And they really do render as 2 distinct identities of the packet's
third form, which is the property the ordinals exist for.

## `fn the_prompt_travels_on_stdin_and_never_as_an_argument()` › `let args = build_args(&task_run());`

GitHub documents that piped input is ignored when `-p` is given, so
passing both would send an empty task. Stdin is also the only
delivery a complete review prompt survives through a Windows cmd shim.

## `fn reviewers_may_neither_write_nor_run_anything()` › `let args = permission_args(`

A reviewer that edits the code it is judging invalidates its own
verdict; one that runs commands is executing the diff under review.

## `fn no_profile_is_ever_handed_the_whole_machine()` › `for permissions in [PermissionMode::Edit, PermissionMode::ReadOnly] {`

§20: the skip-all class of flags is never used, and no URL is ever
granted — that is what keeps an edit profile off the network.

## `fn the_short_flag_check_is_not_fooled_by_longer_flags()` › `assert!(!crate::agent::advertises_flag(`

A bare `contains("-s")` matches `--settings`, `--share` and `--stdio`,
so probing would pass on a build that had dropped `-s` — and every
attempt would then fail at runtime, which is the failure §16 says
probing exists to catch.

## `fn a_turn_cap_is_not_quietly_pretended_to_apply()` › `let mut run = task_run();`

There is no `--max-turns` on this CLI. Nothing sets `max_turns`
today, so this pins the gap rather than the behaviour: whoever makes
profiles config-driven has to come here and decide.

## `fn a_turn_cap_is_not_quietly_pretended_to_apply()` › `run.profile.max_turns = Some(7);`

A digit that appears nowhere else in the args — model slugs carry
version numbers, so a cap of 3 would collide with `gpt-5.3-codex` and
the substitution check below would fail on the model rather than on a
turn cap that leaked.

## `fn a_successful_run_carries_its_response_as_the_detail()` › `let verdict = "json\n{\"pass\": true, \"reasons\": [\"ok\"]}\n";`

The reviewer's verdict travels in exactly this field on the SUCCESS
path — leaving it empty makes every review unparseable (step-6 #1).

## `fn a_successful_run_carries_its_response_as_the_detail()` › `assert_eq!(outcome.duration, out.duration);`

What the supervisor measured, carried through unchanged: see the
same assertion in the Claude adapter for why it is asserted at all.

## `fn unreported_spend_is_none_rather_than_zero()` › `let outcome = parse_output(&output(Some(0), "done", ""));`

This route has no JSON envelope. Recording 0.0 would tell the ledger
a frontier attempt was free (§13); None says it is unknown.

## `fn failures_carry_a_reportable_detail()` › `let outcome = parse_output(&output(Some(1), "I could not finish.", ""));`

Falls back to stdout when the CLI reports through it instead.

## `fn failures_carry_a_reportable_detail()` › `let outcome = parse_output(&output(Some(1), "", ""));`

Nothing at all is still a reportable failure, not a pass.

## `fn a_successful_task_about_rate_limits_is_not_rate_limited()` › `let outcome = parse_output(&output(`

The agent's own summary mentioning 429s must not be read as the pool
being exhausted — that would roll back verified work.

## `mod tests` › `fn probe_against_real_binary_when_present() {`

Runs only where the real CLI exists; skips silently elsewhere so CI
without the Copilot CLI stays green.
