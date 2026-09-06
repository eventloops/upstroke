# `src/gates.rs`

Extended notes for [`src/gates.rs`](../../src/gates.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Gates (DESIGN.md §11.1): configured shell commands run sequentially in the
workspace after every agent attempt, short-circuiting on the first failure.
Gates are what make cheap models affordable — objective, free, and they
catch most small-model failures before any frontier tokens are spent.

Evidence axes owned here: red tests block (a failing test gate fails the
attempt), and test provenance for Test tasks — statically in v0.1 (the
diff must plausibly add test code; lenient by design, with step-6 review
as the backstop); the dynamic fail-on-base/pass-on-HEAD check needs v0.2
worktrees to run safely.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_types)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub enum ShellKind {`

§17 `[engine].shell` — the shell gate commands run under. Default is the
platform-native one.

Serialized into the run record (§15) because it is half of what a gate
command *means*: `command` below hands the same string to a different
interpreter per variant, and `cmd = "true"` is an always-pass builtin under
`sh` while being neither a `cmd.exe` builtin nor a PATH program. A record
carrying the command without the shell would describe a gate nobody can
reproduce. The wire form matches [`parse`](../../src/gates.rs), so config and
log spell a shell the same way.

## `impl ShellKind` › `pub fn resolves_via_path(self) -> bool {`

PowerShell cmdlets are not PATH programs, so pre-flight resolution of
gate commands cannot be enforced for these shells.

## `impl ShellKind` › `pub fn program(self) -> &'static str {`

The shell binary itself, verified at pre-flight by [`shell_available`].

## `impl ShellKind` › `fn builtins(self) -> &'static [&'static str] {`

Shell builtins that are legal command starters but never PATH files.

## `impl ShellKind` › `pub fn spec(self, cmdline: &str) -> CommandSpec {`

How this shell is asked to run `cmdline`, as data.

The data form is the primitive and [`Self::command`] is derived from
it, because DESIGN.md:117 gives the runner the decision about where a
process runs and DESIGN.md:222 gives it a [`CommandSpec`] to run. A
gate that built its own `Command` would carry a spawn decision the
runner never saw — the same hole the adapter seam closes.

There is exactly one place that knows `cmd.exe`'s `/C` tail must reach
the child un-re-quoted, and it is the host runner's command translation
rather than here. Two copies of that rule would be two chances for a
gate command containing a quote to mean one thing when it is probed and
another when it is run.

## `pub trait Gate {`

DESIGN.md §8 `Gate` (synchronous until the v0.2 tokio scheduler; the
`Result` wrapper distinguishes environment errors — e.g. the shell binary
failing to spawn — which abort the run per §19, from gate failures, which
fail the attempt).

## `pub trait Gate` › `fn check(`

Run this gate's command through `runner`, under `invocation`.

DESIGN.md:229 is `check(&self, runner: &dyn Runner, ws: &Workspace)`;
the identity is the contract's addition — "RunnerRequest carries a
typed InvocationId" — and it is a parameter rather than a field because
a gate is rebuilt from the run record (`ShellGate::from_record`) and
runs once per attempt, so the gate is the *what* and the invocation is
the *which time*.

### Errors

An environment problem: the shell could not be spawned, or the runner
refused the request pre-flight. A gate *failing* is
[`GateResult::Fail`], never an error (§19).

## `impl ShellGate` › `pub fn from_record(record: &crate::events::GateSummary) -> Self {`

Rebuild a gate from the run record (§15), so a resume verifies against
what the run verified against rather than against today's config.

Total, and deliberately so: the record carries every field a gate needs,
which is why it can be rebuilt at all. Whether the shell is *installed*
on this machine is pre-flight's question, not this one's.

## `impl ShellGate` › `pub fn command(&self) -> (CommandSpec, Duration) {`

The command and timeout this gate runs as.

**One expression, and it is here so that it is the only one.** The
legacy engine reached this through [`Gate::check`], which mints and runs
in one step; the schema-4 driver needs the same pair *up front*, because
a `GatePlan` is a value it builds before any process starts. Two engines
deriving "which command is this gate" separately is the shape that made
two derivations of a task's predicted region disagree on every glob.

It lives on this type rather than in `engine::assembly` because the data
is this type's, and `gates.rs` sits below the engine — an assembler in
the engine would make this module depend upward on it.

## `impl Gate for ShellGate` › `let (command, timeout) = self.command();`

A spawn failure here is an environment problem (missing shell),
not a task failure — propagate per §19.

`agent: None` and role `Gate`: a gate is repository-controlled code
and runs no agent CLI, so it takes no `{agent, pool}` pair (R3, via
`ExecutionRole::is_slotted`) and `host-v1` hands it no agent's
credential directory (`host::supplies_credentials`). Both are
properties of the role, so naming the role correctly is what buys
them.

## `pub struct GateFailure` › `pub summary: String,`

Short summary for reports (400 bytes); `log_tail` carries the §11.1
feedback payload and the full log is written to the run dir.

## `pub const FEEDBACK_TAIL_BYTES: usize = 8 * 1024;`

§11.1: retry feedback is the output tail, capped at 8 KB.

## `pub fn run_all(`

Run gates sequentially, short-circuiting on the first failure. Every gate
with output gets its log written to `log_dir/<stem>-<attempt>-<gate>.log`
(pass and fail — the pass logs are the evidence trail for committed
tasks). `stem` is the caller's collision-free per-task file stem, not the
raw task id: two ids that sanitize to the same string would otherwise
overwrite each other's evidence. Returns `Ok(Some(failure))` for a gate
failure (attempt fails), `Err` for environment problems (run aborts, §19).

## `let result = gate.check(`

The identity comes from the caller, keyed by this gate's position in
the list the run recorded: the packet's role set is
`{worker, gate(n), review_pass(n), review_reask(n)}`, and `n` is
which gate, not which run of it.

## `pub fn shell_available(shell: ShellKind) -> Result<(), UpstrokeError> {`

§14 pre-flight: the configured shell itself must exist before any agent
tokens are spent.

## `enum Resolution` › `SkippedComplex,`

Quotes, operators, or env-var prefixes: the shell decides — pre-flight
cannot judge these without re-implementing the shell.

## `pub fn resolve_programs(`

§14 pre-flight: every gate command resolves. Hard error for path-resolving
shells; PowerShell cmdlets downgrade to a warning.

## `pub fn preview_resolution(gates: &[ShellGate], workspace_root: &Path, warnings: &mut Vec<String>) {`

`validate`/dry-run variant: same checks, warnings only, never refuses.

## `fn resolution(gate: &ShellGate, workspace_root: &Path) -> Result<Resolution, UpstrokeError> {` › `if cmd`

Shell syntax pre-flight cannot judge: quoting, operators, env prefixes.

## `fn resolution(gate: &ShellGate, workspace_root: &Path) -> Result<Resolution, UpstrokeError> {` › `probe_extensions(&workspace_root.join(candidate))`

Relative to the workspace, where the gate actually runs.

## `pub fn derive(root: &Path, shell: ShellKind) -> Vec<ShellGate> {`

Derived default gates when `[[gates]]` is absent (§17: a fresh repo runs
with zero config): recognized project markers map to the obvious
compile+test commands. Unknown project shapes derive no gates.

## `pub fn derive(root: &Path, shell: ShellKind) -> Vec<ShellGate> {` › `if !script.contains("no test specified") {`

npm init's placeholder always exits 1 — deriving it would
make every zero-config run fail.

## `pub fn diff_adds_tests(diff: &str) -> bool {`

Static test-provenance check (§11.1, v0.1 form): a Test task's diff must
plausibly add test code. Signals, any of which passes: a test-declaration
marker at an identifier boundary on an added line, an added line in a
test-looking file, or an added assertion. Deliberately lenient — false
passes are caught by review (step 6); false failures would roll back
legitimate work.

## `fn has_test_marker(line: &str) -> bool {`

Marker match anchored at an identifier boundary: `exit(` must not match
`it(`, and `regex.test(` must not match `test(`.

## `impl ShellKind` › `pub(crate) fn command(self, cmdline: &str) -> std::process::Command {`

Test witness for the host runner's translation. Production carries
the data-only spec into the Process funnel.

## `mod tests` › `fn host() -> crate::runner::host::HostRunner {`

The runner every gate test runs through: the real host one, because a
gate test is about a gate actually running.

## `mod tests` › `fn gate_id(n: u32) -> InvocationId {`

One legacy-scoped gate identity. `TaskKey(0)`, attempt 1, gate `n` —
the packet's first form with the legacy engine's generation
(`InvocationId::legacy_attempt`).

## `mod tests` › `fn every_shell_spells_its_invocation_the_way_the_record_says() {`

How each shell is asked to run a command line, written from the
vendors' own flags rather than from [`ShellKind::spec`].

This is the independent pin under the whole gate/probe seam: the shell
probe's request is `ShellKind::spec` and so is a gate's, so a test that
compared the two would move both ends together the moment this changed.
The expected rows are `cmd /C`, `sh -c`, `bash -c` and PowerShell's
`-NoProfile -NonInteractive -Command` — the documented non-interactive
invocation of each — with the command line as the last argument.

## `fn every_shell_spells_its_invocation_the_way_the_record_says() {` › `assert_eq!(expected.len(), 5);`

Every variant, and no more: a sixth shell has to be spelled here
before it can be gated with.

## `fn every_shell_spells_its_invocation_the_way_the_record_says() {` › `let built = shell.command(LINE);`

The `Command` the runner builds from it says the same thing. On
Windows `cmd`'s tail goes through `raw_arg`, which is why this
is asserted through the runner's own translation rather than
re-derived here.

## `fn every_shell_spells_its_invocation_the_way_the_record_says() {` › `let programs: std::collections::BTreeSet<String> =`

Fixture hostility as a count: five shells, four distinct programs
(PowerShell and pwsh are two programs with one flag set), and three
distinct argument shapes. A `spec` that ignored the variant would
collapse all three counts to one.

## `mod tests` › `struct ScriptedRunner {`

-----------------------------------------------------------------------
What a ShellGate does with what the Runner hands back
-----------------------------------------------------------------------

## `mod tests` › `struct ScriptedRunner {`

A Runner that answers with exactly what a test tells it to.

The gate tests above all run a **real** `HostRunner`, which is right for
what they measure and is why two of this mapping's branches had never
been reached: a working host cannot produce `output_limited` on demand
(`PR5-CORRECTNESS-011`) and cannot be made to fail its spawn without
depending on what is installed on the machine — the environment-
assumption class recorded as `PR4-CI-ENVIRONMENT-ASSUMPTIONS`
(`PR5-CORRECTNESS-007`). So the supervision result becomes an input.

It records what it was asked, so a grid cannot pass while sending a
request production never sends.

## `enum Scripted` › `Output(Box<crate::agent::ProcessOutput>),`

What the process did.

## `enum Scripted` › `SpawnFailure,`

What a spawn failure looks like: `agent::proc` maps a failed
`ProcessTree::spawn` to `UpstrokeError::Agent { "failed to spawn …" }`.

## `mod tests` › `fn a_shell_gate_maps_every_supervision_result_the_way_the_contract_says() {`

What the gate must do with each shape the supervisor can hand it.

The expectation is a **literal per row**, not a re-derivation of
`check`'s branch order — a function may not be its own oracle
(`PR3-SELF-ORACLE`), and the two rows that matter here are precisely the
ones where a re-derivation would agree with the wrong branch order:
`output_limited` with `code == Some(0)`, and `timed_out` with
`code == Some(0)`. Both are `Fail`, because §19 makes a gate's verdict a
statement about *evidence*, and truncated or supervisor-terminated
evidence authorizes nothing.

Twelve supervised shapes — three exit codes crossed with both flags —
plus the un-run process, which is not a verdict at all.

## `fn a_shell_gate_maps_every_supervision_result_the_way_the_contract_says() {` › `const GRID: &[(Option<i32>, bool, bool, bool, &str)] = &[`

(code, `timed_out`, `output_limited`, expected pass?, what the log
must name).

## `fn a_shell_gate_maps_every_supervision_result_the_way_the_contract_says() {` › `let seen = runner.seen();`

The request really is the one production sends for this role.

## `mod tests` › `fn a_gate_whose_process_never_ran_returns_the_error_and_synthesizes_nothing() {`

A process that never ran is an environment problem, not a verdict.

`decisions.pr_sequence[5].slice_contract.expected_failures_refusals[2]`:
"spawn failure -> existing semantics (**returned error**; no halting
settlement is synthesized …)". A `GateResult::Fail` here is a synthesized
settlement: it becomes a `GateFailure`, an `attempt_finished` and a
ladder transition, so a machine with a broken shell would burn a task's
whole retry ladder on an outage. §19's own words are in `check`'s doc
comment: "A gate *failing* is `GateResult::Fail`, never an error".

Both layers, because the propagation is the claim and it has two steps:
`ShellGate::check` returns the error, and `run_all` — which owns the
short-circuit and the evidence file — hands it out rather than turning it
into `Ok(Some(GateFailure))`.

## `fn a_gate_whose_process_never_ran_returns_the_error_and_synthesizes_nothing() {` › `let runner = ScriptedRunner::new(Scripted::SpawnFailure);`

And through `run_all`, where the settlement would be synthesized. Two
gates, so a short-circuit that returned `Ok(None)` after skipping
them both would still be caught by the count below.

## `fn quoted_arguments_survive_the_windows_shell()` › `let set = gate("git config --local test.quoted \"two words\"", 30);`

`git config` with a quoted value round-trips only if the shell
preserved the quote grouping.

## `fn resolution_enforces_simple_commands_and_skips_shelly_ones() {` › `for complex in [`

Shell-complex commands are the shell's business, not pre-flight's.

## `fn resolution_enforces_simple_commands_and_skips_shelly_ones() {` › `resolve_programs(&[gate("echo hello", 30)], &root, &mut warnings).expect("builtin ok");`

Builtins are legal starters.

## `fn resolution_enforces_simple_commands_and_skips_shelly_ones() {` › `let script_rel = if cfg!(windows) {`

Workspace-relative scripts resolve against the workspace root.

## `fn resolution_enforces_simple_commands_and_skips_shelly_ones() {` › `let ps = ShellGate {`

PowerShell cmdlets downgrade to a warning.

## `fn derive_recognizes_project_markers()` › `let node_placeholder = temp_dir("derive-node-placeholder");`

npm init's always-failing placeholder must not become a gate.

## `fn provenance_markers_respect_identifier_boundaries()` › `assert!(!diff_adds_tests("+    process.exit(1);\n"));`

Identifier and method-call lookalikes must not count.

## `fn provenance_accepts_test_files_and_assertions()` › `assert!(diff_adds_tests(`

Strengthening an existing test: no declaration marker, but an
assertion counts.

## `fn provenance_accepts_test_files_and_assertions()` › `assert!(diff_adds_tests("+++ b/tests/api.rs\n+        helper(1);\n"));`

Any real addition inside a test-looking file counts.

## `fn provenance_accepts_test_files_and_assertions()` › `assert!(!diff_adds_tests("+++ /dev/null\n+ignored\n"));`

Deleted-file headers must not mark what follows as test content.
