# `src/main.rs`

Extended notes for [`src/main.rs`](../../src/main.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `#![allow(`

upstroke — headless orchestration engine for AI coding agents.
Copyright 2026 Cameron Lambert
SPDX-License-Identifier: Apache-2.0
Licensed under the Apache License, Version 2.0; see LICENSE and NOTICE, or
<http://www.apache.org/licenses/LICENSE-2.0>.
LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `const EXIT_PARKED: u8 = 2;`

§12: a run that ends with tasks parked on unanswered questions completed
neither cleanly nor in error. CI has to be able to tell the difference, so
it gets its own status.

## `const EXIT_BUDGET: u8 = 3;`

§13: a run stopped by its own budget completed neither cleanly, in error,
nor waiting on a human. CI has to tell "your ceiling stopped it" from "a task
failed" without parsing prose — and `upstroke resume --budget` is what it does
about it, which is different from what it does about either of the others.

## `enum Command` › `Connect {`

Discover installed agent CLIs and write ~/.upstroke/pools.toml

## `enum Command` › `force: bool,`

Replace an existing pools file that differs from what this would
write. Without it, connect prints the difference and refuses.

## `enum Command` › `pools: Option<PathBuf>,`

Pools file path (default: ~/.upstroke/pools.toml)

## `enum Command` › `Capacity {`

Show every pool: remaining estimate, resets, and what each strategy would do

## `enum Command` › `config: Option<PathBuf>,`

Repo config path (default: ./upstroke.toml, optional)

## `enum Command` › `pools: Option<PathBuf>,`

Pools file path (default: ~/.upstroke/pools.toml)

## `enum Command` › `Validate {`

Parse a plan, resolve routing, and print the task table (no execution)

## `enum Command` › `plan: PathBuf,`

Path to the plan file (annotated or bare markdown)

## `enum Command` › `emit_json: bool,`

Write plan.normalized.json (the IR) to the current directory

## `enum Command` › `config: Option<PathBuf>,`

Repo config path (default: ./upstroke.toml, optional)

## `enum Command` › `Run {`

Execute a plan sequentially: run branch, agent per task, commit per task

## `enum Command` › `plan: PathBuf,`

Path to the plan file (annotated or bare markdown)

## `enum Command` › `dry_run: bool,`

Everything except agents: parse, route, and print the preview at
zero spend

## `enum Command` › `config: Option<PathBuf>,`

Repo config path (default: ./upstroke.toml, optional)

## `enum Command` › `interaction: Option<Interaction>,`

Override [interaction] mode; `never` is the CI setting — questions
park their tasks and the run reports them instead of waiting

## `enum Command` › `budget: Option<f64>,`

Ceiling on api-equivalent dollars, overriding [budgets] run_usd.
The run stops (exit 3) before the attempt that would cross it

## `enum Command` › `Resume {`

Continue a run that was interrupted, parked, or stopped at its budget

## `enum Command` › `run_id: String,`

Run id, or any unambiguous prefix of one

## `enum Command` › `config: Option<PathBuf>,`

Repo config path (default: the one the run recorded)

## `enum Command` › `budget: Option<f64>,`

Raise the ceiling and continue. Budgets are re-derived at resume
rather than inherited from the stopped run

## `enum Command` › `Status {`

Show a run: what happened, what it cost, and what it is waiting for

## `enum Command` › `run_id: Option<String>,`

Run id or prefix; omit for the most recent run

## `enum Command` › `follow: bool,`

Stream events as they are appended, ending when the run finishes

## `enum Command` › `ExportDecisions {`

Export one settled run's local routing decisions to stdout

## `enum Command` › `run_id: String,`

Run id, or any unambiguous prefix of one

## `enum Command` › `format: ExportFormat,`

Output encoding (default: jsonl)

## `enum Command` › `Answer {`

Answer a question a run is parked on (§12)

## `enum Command` › `question_id: String,`

Question id, or any unambiguous prefix of one

## `enum Command` › `option: Option<usize>,`

Pick one of the question's numbered options

## `enum Command` › `text: Option<String>,`

Answer in your own words

## `enum Command` › `decline: bool,`

Give up on the task; its dependents will be blocked

## `enum Interaction {`

CLI spelling of [`InteractionMode`], so CI does not have to edit
`upstroke.toml` to stop a run waiting on a human.

## `enum CommandClass {`

Whether a command may reach a workspace effect, and therefore whether it
has to establish containment before it starts.

**Which commands are write commands is decided by the packet, not by this
file.** `decisions.sequential_substrate.startup_census`: the census is

> performed by every topology write command **(run, resume)** after taking
> the worktree lock and before any run-id use for creation, run-lock
> acquisition for a fresh run, slot or reservation initialization,
> admission, credential-volume use, or probe

and `crash_reconstruction` anchors the ambient job at the same coordinate:
"at process start **every write command** creates one non-inheritable
ambient Job Object … if the ambient job cannot be created or joined the
write command refuses at startup with a diagnostic **before any workspace
effect** (no degraded mode; deferred)". The parenthesis in the census is the
enumeration: a write command is `run` or `resume`.

For today's binary that is `Command::Run` and `Command::Resume`, and the
classification is by **dispatch arm**, so `run --dry-run` is a write command
too. That is deliberately one notch wider than "makes a workspace effect":
the packet's coordinate is *process start*, which precedes flag
interpretation, and a preview that shares an arm with the command that
spends should not be the one place containment is skipped. It is asserted
rather than left implicit (`the_dry_run_preview_is_classified_with_its_arm`).

`answer` writes a file into a run directory and `connect` writes the pools
file, but neither is a topology write command: neither drives a run, and the
census the packet anchors here is a *run's* census. `connect` and `capacity`
do spawn agent CLIs to discover them — two commands, counted and asserted in
`the_commands_that_spawn_outside_a_run_are_named_and_counted` — and those
children are outside INV-18's "the **coordinator's** ambient … Job Object",
because neither command is a coordinator of a run.

## `enum CommandClass` › `Write,`

A topology write command: it drives a run and must contain its children.

## `enum CommandClass` › `ReadOnly,`

Everything else.

## `const fn command_class(command: &Command) -> CommandClass {`

The class of one parsed command.

Exhaustive with no wildcard arm: a `Command` variant added later fails to
compile here, so no command can join the dispatch without being classified.

## `mod containment {`

INV-18's host portion, as a capability rather than a call order.

> ambient job joined at write-command startup (refusal otherwise)

[`Contained`] has a private field, so nothing outside this module can build
one, and [`execute`] cannot be called without one. The contract's
`side_effect_vs_event_ordering` is "no events; ambient job before any
spawn", and that ordering is therefore a compile error to transpose rather
than a convention a later edit can quietly reverse.

## `mod containment` › `pub struct Contained(());`

Proof that this process performed its write-command containment
startup. Unit-like with a private field: only [`establish`] can make
one.

## `mod containment` › `pub fn establish(`

Establish containment for `command`, or refuse it.

On Unix the join is a no-op that returns `Ok`: containment there is the
per-invocation reaper and the isolated process group.

## `fn validate_options(plan: PathBuf, config: Option<PathBuf>) -> anyhow::Result<ValidateOptions> {`

One construction point so `validate` and `run --dry-run` can never drift
into previewing different things.

## `fn validate_options(plan: PathBuf, config: Option<PathBuf>) -> anyhow::Result<ValidateOptions> {` › `engine_limits: upstroke::config::EngineLimits::Fresh,`

Both callers are previewing a run that does not exist yet, so both
want the reading a fresh run gets — including its refusals.

## `fn run() -> anyhow::Result<ExitCode>` › `run_wired(Cli::parse().command, &mut upstroke::agent::proc::NoHooks)`

`NoHooks` is what production passes the process funnel, and the ambient
join is threaded the same way: the observer is there so the step has a
failure path a test can drive on the platform where it is real
(`upstroke::runner::host::contain_write_command`), and production arms
nothing.

## `fn run_wired(`

The CLI's own composition of containment and dispatch — the two statements
`run` would otherwise hold, with the observer as a parameter.

It exists because `run` cannot be driven. `Cli::parse` reads this process's
real argv and exits on a parse error, so a test cannot call `run` with a
command of its choosing, and `run` was therefore the one link in
`expected_failures_refusals[1]`'s chain that nothing exercised.
`dispatch` is driven with an injected failure and
`runner::host::start_write_command` is driven with one on the guest — but
the **wiring between them** was a closure no test ever called, so

```text
|| { let _ = upstroke::runner::host::start_write_command(&mut upstroke::agent::proc::NoHooks); Ok(()) }
```

left `upstroke run … --dry-run` succeeding on a Windows host whose ambient job
could not be established, against the slice `scope`'s "refusal with
diagnostic if it cannot", with the whole suite green.

Threading the **observer** rather than the join closure is what makes the
difference: `start_write_command` is then inside the function under test
instead of inside its caller, and the arm `a_cli_write_command_refuses_when_
the_real_containment_step_refuses` drives it with a hook that refuses at
`Spawn.AmbientJobJoined`. What is left above it — `run`'s single delegating
expression — constructs no `Result` of its own, which is what
`the_cli_wires_the_real_containment_step_into_dispatch` reads the source to
assert.

## `fn dispatch(`

Establish containment, then execute. The ambient join is a parameter so a
test can drive a failure that no machine here can produce, and so the
ordering between the two is testable rather than merely written down.

## `fn execute(command: Command, _contained: Contained) -> anyhow::Result<ExitCode> {` › `if report.refused() {`

A refusal to clobber is not something a retry fixes, and a script
that cannot tell it from success would go on to run against a
pools file that says something else entirely.

## `fn execute(command: Command, _contained: Contained) -> anyhow::Result<ExitCode> {` › `status::follow(`

History first, then live events: dropping a reader into the
middle of a run tells them less than showing how it got here.

## `fn execute(command: Command, _contained: Contained) -> anyhow::Result<ExitCode> {` › `let settled = status::load(&repo_root, Some(&run.run_id))?;`

Re-read: the run has moved since the summary would have been
computed, and the closing summary is the useful one.

## `fn execute(command: Command, _contained: Contained) -> anyhow::Result<ExitCode> {` › `(None, None, false) => Reply::Text(prompt_for_answer(&repo_root, &question_id)?),`

Nothing given: show the question and read one line, so the
common case is `upstroke answer <id>` and then just type.

## `const IDLE_POLLS_BEFORE_GIVING_UP: u32 = 240;`

How long a follower keeps watching a run that nothing is driving any more:
roughly two minutes. A live run holds its lock and `follow` waits on that
for as long as an agent turn takes, so this budget is not a limit on
silence — it starts only once the lock is gone, and exists so a terminal
attached to a dead engine does not hang.

## `fn finish(report: &engine::RunReport) -> anyhow::Result<ExitCode> {` › `RunOutcome::Parked => Ok(ExitCode::from(EXIT_PARKED)),`

§12: parked is neither clean nor broken. Distinguishable so CI can
gate on it without parsing prose.

## `fn finish(report: &engine::RunReport) -> anyhow::Result<ExitCode> {` › `RunOutcome::BudgetExceeded => Ok(ExitCode::from(EXIT_BUDGET)),`

§13: nor is a budget stop. It is not an error — the run did exactly
what the ceiling asked — so it does not `bail`, and the report above
already printed the resume command that continues it.

## `fn prompt_for_answer(repo_root: &std::path::Path, question_id: &str) -> anyhow::Result<String> {`

Show the question, then take the operator's answer.

## `fn prompt_for_answer(repo_root: &std::path::Path, question_id: &str) -> anyhow::Result<String> {` › `eprint!("answer (a number picks an option, empty aborts): ");`

Enter submits — what the legend promises, and the only thing a
person typing at a prompt will try. Reading to end here would wait
for EOF instead (Ctrl+D, or Ctrl+Z then Enter on Windows), so
pressing Enter would leave the command sitting there saying nothing.

## `fn prompt_for_answer(repo_root: &std::path::Path, question_id: &str) -> anyhow::Result<String> {` › `stdin`

Piped: read to end so an answer can span lines. The interpreter
trims and treats the whole thing as the operator's words.

## `mod tests` › `const DISPATCH: &[(&str, &[&str], CommandClass)] = &[`

Every subcommand this binary dispatches, with an invocation that parses
and the class the packet gives it.

Written here by hand from `decisions.sequential_substrate.startup_census`
("every topology write command (run, resume)"), so it is an oracle
independent of [`command_class`]. A command added to the enum without
being added here fails
[`every_dispatch_arm_is_classified_by_the_packets_rule`] — the list that
rots is replaced by a list that is checked.

## `mod tests` › `const ABSENT_PLAN: &str = "/upstroke-pr4-no-such-plan-33f1a9/plan.md";`

A plan path that exists on no machine, so the dispatch arm that reads it
fails in a way nothing else produces.

## `mod tests` › `fn every_dispatch_arm_is_classified_by_the_packets_rule() {`

The whole point of the table: a new subcommand cannot reach the dispatch
without a classification, and cannot be classified in production without
being classified here too.

## `mod tests` › `fn the_dry_run_preview_is_classified_with_its_arm() {`

The classification is by dispatch arm, so the preview shares the class
of the command it previews. Stated in `command_class`, asserted here, so
the widening cannot become invisible.

## `mod tests` › `fn the_commands_that_spawn_outside_a_run_are_named_and_counted() {`

The two commands that spawn a host child outside a run, counted so the
boundary cannot grow in silence. `connect` and `capacity` both probe the
installed agent CLIs through the adapter probes in `connect::run_with` and
`capacity::report`; neither
drives a run, so neither is the "coordinator" whose ambient job INV-18
names.

## `mod tests` › `fn a_write_command_refuses_before_any_effect_when_containment_fails() {`

A refused ambient join stops the write command **before** its arm runs.

The oracle is that the two outcomes are different errors from different
places: the refusal names the ambient job, and the arm — reached only
when the join succeeds — names the plan it could not read. If
containment ran after the arm, or not at all, the first call would carry
the plan's error instead.

## `mod tests` › `fn every_write_command_establishes_containment_and_no_read_only_one_does() {`

**Every** write command joins, and every read-only command does not.

`crash_reconstruction`: "at process start **every write command**
creates one non-inheritable ambient Job Object"; the contract's
`side_effect_vs_event_ordering` is "no events; ambient job before any
spawn". The two tests below drive `dispatch` with one command each —
`run --dry-run` and two read-only arms — so a containment step
conditioned on *which* write command it is (a wet `run`, a `resume`)
would keep every one of their assertions true while the two commands
that actually spend went unprotected: killed between `CreateProcess`
and private-job assignment, they leave a suspended stub with no owner,
and a real ambient failure could not produce the required startup
refusal.

So this crosses `establish` — the classification's one consumer — with
every row of [`DISPATCH`] plus the dry-run preview, and asserts the
**count** of joins on each side. `establish` rather than `dispatch`
because it is the mutation site and because running the wet arms would
execute a run.

## `fn every_write_command_establishes_containment_and_no_read_only_one_does() {` › `argvs.push((`

The preview shares its arm's class, and the arm is what joins.

## `fn every_write_command_establishes_containment_and_no_read_only_one_does() {` › `let command = Cli::try_parse_from(argv).expect("parse").command;`

And the refusal is per command, not per class: a write command
whose join fails refuses, a read-only one cannot fail because it
never calls it.

## `mod tests` › `fn the_cli_write_path_runs_the_real_containment_step() {`

The CLI's own wiring: `run_wired` composes the **real** containment step
with `dispatch`, on every platform.

`a_write_command_refuses_before_any_effect_when_containment_fails` drives
`dispatch` with a join of the test's choosing, so it says nothing about
which join the CLI passes it; `runner::host`'s own tests drive
`start_write_command` directly, so they say nothing about who calls it.
This is the composition, and the oracle is production's own count:
`containment_establishments()` is incremented by `Contained::new`, which
only `contain_write_command` reaches and only after
`proc::join_ambient_job` returned `Ok`. So a `run_wired` that passed
`|| Ok(())` instead of the real step — or that never established
containment at all — cannot move it.

## `fn the_cli_write_path_runs_the_real_containment_step()` › `let reached = format!("{reached:#}");`

And it established it *before* the arm ran, which is what the count
alone cannot say: the error the caller receives is the plan's.

## `fn the_cli_write_path_runs_the_real_containment_step()` › `let mark = containment_establishments();`

The other side of the classification, through the same wiring.

## `mod tests` › `fn a_cli_write_command_refuses_when_the_real_containment_step_refuses() {`

And the refusal reaches the caller through that same wiring, on the
platform where the join can fail.

This is the arm that kills the finding's mutation. `run_wired` threading
the **observer** is what makes it possible: with a hook armed to refuse
at `Spawn.AmbientJobJoined`, a body that discarded
`start_write_command`'s error and answered `Ok(())` would let the
dry-run preview reach its arm and fail on the *plan* instead of refusing
with the ambient job's diagnostic — which is
`expected_failures_refusals[1]` not holding for the CLI.

Windows-only because the step it drives is: `proc::join_ambient_job` is
a no-op on Unix that never consults the observer, deliberately, so a
Linux cell cannot claim this coverage — the same boundary
`PR4-CONF-005` records for `contain_write_command`.

## `fn a_cli_write_command_refuses_when_the_real_containment_step_refuses() {` › `for fragment in ["ambient", "INV-18", "No process was spawned"] {`

The same three fragments `runner::host::tests::the_production_
containment_mint_propagates_a_join_refusal_and_mints_nothing` reads,
and for its reason: what the operator has to be told is that it is
the ambient job, which invariant it enforces, and that nothing ran.
Named fragments rather than the whole sentence, and rather than the
`SubEffectPoint` token — the refusal the CLI hands back is
production's own diagnostic (`proc::AMBIENT_REFUSAL_PREFIX` +
`AMBIENT_REFUSAL_SIMULATED`), not the funnel's internal coordinate.

## `mod tests` › `fn the_cli_wires_the_real_containment_step_into_dispatch() {`

`run` itself cannot fabricate a success, because it constructs no value.

The runtime tests above hold everything from `run_wired` down. `run` is
the one link above them that no test can call — `Cli::parse` reads this
process's real argv — so it is held the way this project already holds
claims of exactly this shape: by reading the source
(`runner::tests::every_production_process_start_is_classified`,
`every_production_runner_request_is_built_by_its_roles_builder`).

The oracle is narrow on purpose. Both functions are pure delegations,
so neither has any reason to write an `Ok`; a body that swallowed the
call below it — `let _ = run_wired(…); Ok(ExitCode::SUCCESS)` — has to
construct one. And `start_write_command` must be named exactly once in
the file, inside `run_wired`, so the step cannot be called somewhere
that discards it and called again where a test can see it.

## `fn the_cli_wires_the_real_containment_step_into_dispatch()` › `let source = source.replace("\r\n", "\n");`

Line endings are the checkout's, not the repository's: the Windows
guest checks this file out with CRLF, and a census that split on
`"\n#[cfg(test)]\n"` found nothing there and read the test module as
production. Normalised first, so the oracle is the source and not the
platform that happens to be reading it — the same class as
`PR4-CI-ENVIRONMENT-ASSUMPTIONS`, and caught by the same guest.

## `fn the_cli_wires_the_real_containment_step_into_dispatch()` › `let code: String = production`

Comments are not code, and this census would otherwise be the exact
hazard `reviews/FINDINGS.md` records as `PR4-CENSUS-COMMENT-ORACLE`:
`run_wired`'s doc comment quotes the mutation it exists to kill, and
a source count that read it would make the doc and the code
indistinguishable. The rule is deliberately simple — everything from
a `//` that is not part of a `://` — and it is checked, below,
against a line this file is known to carry.

## `mod tests` › `fn a_read_only_command_does_not_join_the_ambient_job() {`

A read-only command never joins. The oracle is a join that cannot be
called without failing the test.

## `mod tests` › `const AMBIENT_LATCH_RECORD: &str = "UPSTROKE_PR4_CLI_LATCH_RECORD";`

Where the ambient-latch helper writes what it observed.

## `mod tests` › `fn cli_ambient_latch_helper() {`

The child half of
[`a_write_command_establishes_the_ambient_job_and_a_read_only_command_does_not`]:
it drives the CLI's real wiring and records the process-wide latch at
three points, leaving the judgement to its parent.

It records rather than asserts because the parent's output is where a
developer reads a failure — this child's streams are closed — and
because the record is also the evidence that the child ran at all. Each
observation is flushed as it is taken, so a panic still leaves what was
seen up to it.

## `fn cli_ambient_latch_helper()` › `let read_only = Cli::try_parse_from(["upstroke", "validate", ABSENT_PLAN])`

`run_wired` rather than `dispatch` with a join of our own: it is the
composition `run` uses, so the child exercises the CLI's real path to
the join instead of a reassembly of it.

## `mod tests` › `fn a_write_command_establishes_the_ambient_job_and_a_read_only_command_does_not() {`

The real join, at the real coordinate, on the platform that has one.

In a subprocess because the ambient job is a process-wide singleton and
this binary's tests run in **threads**: "not yet established" is a fact
about a process, so a test that reads it in a shared one is reading its
siblings too. Held in-process, none of the three readings below was an
observation of this test's own commands:
`the_cli_write_path_runs_the_real_containment_step` drives a write
command through the same real step on another thread, and whichever
reading it lands between is the one that fails — before the first,
"nothing has run a write command in this process yet"; between the first
and the second, "a read-only command established the coordinator's
ambient job". Neither is what happened. Measured on the guest at one
failure in three full-suite runs when it was first seen and one in six
when it was diagnosed. The other tests here are immune for a reason that
does not extend to this one: they read `containment_establishments`, a
**thread-local** count, as a delta around their own call.

So the oracle is unchanged — the real process-wide latch, not a
thread-local proxy, because the property is that the *process* joins —
and what changes is who observes it: a child with its own latch, running
exactly one test (`--ignored` plus a filter naming one helper), which no
sibling thread can reach.

The premise stays checked rather than assumed. `start 0` is asserted here
as loudly as the two observations that depend on it, because a child that
somehow began with the latch set would make them vacuous. And the record
file is the evidence the child ran: a libtest filter that matches nothing
exits 0, so a parent that read only the exit status would pass with no
child at all.
