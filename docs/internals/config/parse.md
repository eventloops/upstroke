# `src/config/parse.rs`

Extended notes for [`src/config/parse.rs`](../../../src/config/parse.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The `upstroke.toml` section readers (DESIGN.md §17).

One function per section — `[runner]`, `[routing.effort]`, `[budgets]`,
`[[gates]]`, `[engine]`, `[interaction]` — each taking the section's raw
`toml::Value` and returning the typed shape the parent's `Config` is built
from. The parent owns the ladder that calls them and the types they return;
this module owns only the readings.

**The error-versus-warning split is a rule, not a per-section taste.** A key
whose typo would silently delete a control the operator asked for is a hard
error; a key whose typo only degrades what the run can say about itself is a
warning that names the key. Each function's own documentation records which
side of that line its keys sit on and why, because the reason is what a
later reader needs in order to place a new key correctly.

Nothing here reads a path, starts a process, or writes anything: every input
arrives as an already-parsed `toml::Value`, and the reading of the bytes it
came from belongs to `super::read`.

## `#![deny(`

**This child states its own lint level and inherits nothing.** A Rust lint
level is scoped by the module tree rather than by the file, so an out-of-line
child of `src/config.rs` inherits that file's inner
`#![allow(clippy::disallowed_methods)]` unless it says otherwise --
`PR6-LANEF-004`, and the mistake two W1 pull requests then made
independently (#100 and #102). Nothing here reaches a governed primitive, so
all three governed lints are DENIED and this module takes no
`effects/allowlist.toml` row: a row records an allowance, and this module
takes none.

The three are not equally load-bearing, and which is which is worth stating.
`src/config.rs` allows `clippy::disallowed_methods` and that lint alone, so
the first line below is the one that restores a level the parent removed
outright: without it, a denied method here raises no diagnostic at all. The
other two raise this module from clippy's default `warn` to `deny`, so a
denied type or macro fails here on its own rather than only under CI's
`-D warnings`. All three are written out because what decides the first one
is a property of the parent's attribute rather than of this file, and a
parent's attribute can widen without this file changing.

## `pub(super) fn read_runner(`

`[runner]` as written, with no policy applied.

## `if let Some(key) = runner.unknown.keys().next() {`

An error, where `[engine]` warns. `[engine]`'s unknown keys are ceilings
and timeouts: a typo leaves a default in place and the run does slightly
less than asked. A typo here — `knid = "container"`, `iamge = "..."` —
leaves the run executing on the **host** while the operator believes gate
code is confined, which is the one thing this section exists to decide.
Same rule as `[interaction] ask_before` and `[budgets]`, for the same
reason: silently ignoring a key is silently deleting a control.

## `let stray: Vec<&str> = [`

The config-side twin of `RunnerRecordDefect::HostWithContainerFields`,
which PR3 already refuses on the recorded side. An operator who set
`kind = "host"` under an image line has described two boundaries and
gets one; accepting it silently is how a run executes unconfined
while its config reads as if it did not.

## `read_only: mount.read_only.unwrap_or(true),`

Writable is a thing you say. See `RunnerMount`.

## `pub(super) fn refuse_legacy_container_selection(`

`expected_failures_refusals[0]`: "`[runner] kind = container` under a
schema-1..3 fresh run **or** resume -> config error before any effect".

#### Why this is structural and not stylistic

`production_effect`: "the legacy engine's preflight probes precede any run
identity or lock and can own no container intent". R26's own rule is that
"no container ever lacks a race-free owner or a durable boundary identity",
and the schema-1..3 engine has nothing to give one: it probes before it has a
run id, a `run.lock` or a recorded runner. So refusing **late** — after a
probe, after a lock, after any effect — is not a weaker version of this
refusal, it is a different and broken one: the container it would refuse
already exists and belongs to nobody.

#### Where "before any effect" is bought

Here, by position. Both write commands run
`preflight::validate_inputs` — which is `config::load_captured` — as their
first statement, before `Workspace::open`, before `WorktreeLock::acquire_in`
and before `RunPaths::create`: `coordinator.rs`'s comment on that line is
"every read-only refusal precedes every lock", and `resume.rs` marks the
line after it "the first effect of the command".
`runner::container::resolve::tests::legacy_container_selection_refused_before_effects`
drives both commands and asserts the tree afterwards.

#### Both readings refuse, and that is the whole of today's answer

[`EngineLimits`] distinguishes a run being created from a sequential run's
resume, and `expected_failures_refusals[0]` names **both**. There is no
third reading in this build: `EngineLimits::Fresh` means "a run being
created now", and every run this binary creates is schema-3.
`PR12 config acceptance for fresh schema-4 runs only` (INV-23's
`enforced_by`) is where a fresh schema-4 run learns to accept it, and that is
a new reading rather than a relaxation of this one.

### Errors

[`UpstrokeError::Config`] when `selection` is a container selection.

## `pub(super) fn parse_role_effort(`

Parse one role's explicit effort at config load. All three providers reject
an unknown value after process launch, so accepting a typo here would burn an
attempt for a routing policy the operator never actually selected.

## `pub(super) fn parse_budgets(`

`[budgets]` (§17). A ceiling that is zero, negative, or not a number is a
hard error: every one of those readings would either stop the run before it
began or be ignored, and which of the two happened must not be a surprise.

## `pub(super) fn parse_gates(`

`[[gates]]` parsing with actionable shape errors: a `[gates]` table, a
wrong-typed field, or `timeout_secs = 0` all name what was expected.

## `pub(super) fn parse_engine(`

`[engine]` (§17).

Every key here is now consumed, refused, or named in a warning. Nothing is
read past: accepting `max_parallel = 4` and then running one attempt at a
time is the failure a config file exists to prevent — the operator believes
they bought four workers, the run costs and takes what one worker costs and
takes, and nothing anywhere says otherwise. That is the same silent-ignore
harm `second_opinion` and `[budgets] pool_fraction` each earned a refusal
for, and it is this section's own long-standing defect.

The three ceilings split from `max_parallel` on which reading is wrong.
`max_parallel` above 1 describes a run **this engine cannot perform**, so on
a fresh run it is a hard error — raised here, which is before a lock, a
workspace, or a run directory exists. `max_merge_repairs`, `max_per_agent`,
and `max_per_pool` bound a topology that arrives with the parallel engine; a
nondefault value is a true statement about a later run and a silent no-op in
this one, so it parses, is kept, and warns.

`limits` is what keeps that refusal from reaching a run it cannot help. See
[`EngineLimits`]: on a sequential run's resume every one of these keys is
about a future run, `max_parallel` included, so all four warn and the resume
continues on the ceiling it recorded.

## `let on_task_failure = match engine.on_task_failure {`

A misspelling here decides whether a failed task stops the run, so it
errors rather than warning: silently halting a run the user asked to
continue (or the reverse) is not a recoverable surprise.

## `let limit = |key: &str, configured: Option<u32>, default: u32| -> Result<u32, UpstrokeError> {`

Zero has two readings — "no ceiling" and "nothing may run" — and which one
happened must never be a surprise. The rule `attempts_per` and every
`timeout_secs` already follow.

## `let max_parallel = match (limits, configured_parallel > DEFAULT_MAX_PARALLEL) {`

What this load's run will actually be allowed to do. It parts company
with what the file says in exactly one case — a sequential run's resume,
whose ceiling is a fact about the run and not about today's config — and
that case says so out loud below rather than carrying the file's number
into a Config field nothing may act on.

## `let max_per_agent = limit("max_per_agent", engine.max_per_agent, configured_parallel)?;`

Defaulted off what the file asked for rather than off the effective
ceiling: `max_parallel = 3` with neither companion written is one
statement about a future run, and splitting it into a refused 3 and two
inherited 1s would announce two edits the operator never made.

## `for (key, configured, default) in [`

Kept, and announced as inert. A warning rather than an error because the
value is not wrong — it is simply about a run this build cannot perform
yet, and erroring would refuse a config an operator wrote for the engine
they are waiting for.

## `pub(super) fn parse_interaction(`

`[interaction]` (§12).

Everything here is a hard error or nothing: `mode` and `ask_before` both
decide whether a human is ever asked, so a typo in either must not degrade
quietly. Notifier ids are the one soft setting, and they are validated by
`notifiers_for` at run time rather than here — which is why this function
takes no warning sink.

## `let ask_before = match interaction.ask_before {`

An unknown key inside `ask_before` errors rather than warning: the whole
point of the table is to stop the run and ask, so a misspelling that
silently drops the threshold spends the money the operator asked to be
consulted about. Same reasoning as `second_opinion`.
