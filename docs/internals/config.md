# `src/config.rs`

Extended notes for [`src/config.rs`](../../src/config.rs).

## Module

Config loading (DESIGN.md §17 subset for `validate`).

Two optional files: repo-level `upstroke.toml` (routing overrides, pins,
strategy) and user-level `~/.upstroke/pools.toml` (capacity pools, normally
written by `upstroke connect`). Both missing is the normal fresh-repo case
and falls back to derived defaults silently.
LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `#[serde(deny_unknown_fields)]`

The top level of `upstroke.toml`. Its seven fields are exactly the sections
`design/17` documents for the repo-level file, so an unknown section name is a
typo and is refused naming it and the accepted seven. Before this attribute a
misspelled header -- `[budgts]`, `[interation]`, `[runer]` -- deserialized into
nothing: the whole section vanished, its defaults took effect (no budget
ceiling; `on_block` interaction in CI; the host runner where the file read as
confined) and `validate` reported a clean file. A top-level typo deleted the
largest unit a typo can delete, and this attribute closes that boundary and
that boundary only (`SWEEP-CONFIG-PARSE-007`, closed by the sweep of
`src/config/read.rs`). It is not the last silent drop in the file:
`[routing.strategy]` is read through `RawStrategy`, which carries neither
`deny_unknown_fields` nor an unknown-key collector, so a misspelled
`spend_down_afer` there is still discarded without a word
(`SWEEP-CONFIG-PARSE-008`, open, guarded to this file's own sweep, row 54).
No forward-compatibility key is lost at the top level: nothing in the tree's
fixtures, docs or examples writes a top-level key outside the seven.

## `fn a_misspelled_top_level_section_is_refused_not_dropped() {`

Each of the three fixtures used to deserialize into nothing: the whole
section vanished and its defaults took effect while `validate` reported a
clean file (`SWEEP-CONFIG-PARSE-007`). The test is witnessed against removing
the attribute above: without it the load succeeds and the section is dropped.

## `gates: Option<toml::Value>,`

Parsed as raw values so shape mistakes get actionable messages instead
of bare serde errors (configs written before these sections were
consumed must not brick on upgrade with cryptic output).

## `#[derive(Debug, Deserialize)]`

`[runner]` (DESIGN.md:612): "a runner is orthogonal to an adapter: `[runner]`
config selects `host` or `container` (image, mounts)".

Read as a raw value like the sections above it, so a shape mistake reports as
a named problem rather than a serde error about a struct nobody wrote.

## `image: Option<String>,`

The image **reference** an operator wrote. Never what a container is
created from — INV-23 pins the runtime's immutable id for that.

## `credential_volumes: Option<BTreeMap<String, String>>,`

Per-agent credential volume names (R20, operator-owned).

## `mounts: Option<Vec<RawRunnerMount>>,`

Extra mounts the boundary receives beyond the ones the runner composes.

## `#[serde(flatten)]`

Everything else. Unlike `[engine]`, an unknown key here is an **error**;
see [`read_runner`].

## `#[derive(Debug, Deserialize)]`

One `[[gates]]` entry as written. An unknown key is collected rather than
dropped, and `parse::parse_gates` decides what it means: an **error** on a
fresh run, as in `[runner]` — the entry has three keys, the one that is
optional decides when a running gate is killed and reported failed, and a
typo there is a timeout the operator asked for and did not get — and a
**warning** on a sequential run's resume, where the recorded gates are what
executes (`design/15`: gates are taken from the record and not refused over).

## `name: Option<String>,`

Required, and optional at the wire so that a missing one is refused by
`parse::parse_gates` beside whatever unknown key replaced it — `nmae`
is named as the likely misspelling rather than lost behind serde's
"missing field".

## `cmd: Option<String>,`

As `name`.

## `#[serde(flatten)]`

Everything else, so a typo is named instead of vanishing.

## `max_parallel: Option<u32>,`

§17's concurrency ceiling.

## `max_merge_repairs: Option<u32>,`

Autonomous repair generations per original task before a human is asked.

## `max_per_agent: Option<u32>,`

Per-agent and per-pool concurrency slots; both default to `max_parallel`.

## `#[serde(flatten)]`

Everything else, so a typo is refused by name instead of vanishing —
see `parse::parse_engine`.

## `#[derive(Debug, Deserialize)]`

`[interaction]` as written. An unknown key is an **error**, as in
`[runner]`: `mode` and `ask_before` each decide whether a person is asked,
and `ask_befor = { … }` is a spend approval that no longer exists — see
`parse::parse_interaction`.

## `wait_on_block_secs: Option<u64>,`

Seconds a detached interactive run waits at a hard block for an answer
to arrive as an event; `0` disables waiting.

## `ask_before: Option<toml::Value>,`

`ask_before = { frontier_escalation_over_usd = 5.0 }` (§12).

## `#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize)]`

`[interaction] ask_before` (§12) — the thresholds that turn a routing move
into a question for a person.

One key today, and an unknown one is a **hard error** naming the accepted
set: a typo here silently deletes a spend approval, which is the same harm
that made `second_opinion` error rather than warn.

## `pub frontier_escalation_over_usd: Option<f64>,`

Ask before escalating onto a **frontier** rung once the run's reported
spend has reached this many api-equivalent dollars.

Deliberately spend-to-*date*, not a forward projection. A literal
reading of "this escalation will cost more than $N" needs per-model
$/token rates the catalog does not ship, and §10's whole position is
that guessing costs is worse than measuring them — inventing a price
table would pile unverifiable static data on top of model names that
have already proved perishable. v0.2 can project forward from *observed*
per-rung costs once decision logs hold them.

## `const ACCEPTED: [&'static str; 1] = ["frontier_escalation_over_usd"];`

Accepted keys, named once so the parser and its error message cannot
disagree about what is legal.

## `effort: Option<toml::Value>,`

`[routing.effort]` is parsed as a raw value so shape and spelling
mistakes can name the two accepted roles rather than failing in serde's
outer config message.

## `#[serde(flatten)]`

Per-kind chain entries (`fix = { chain = [...] }`) plus anything the
config author got wrong — unknown keys warn rather than error.

## `start_at: Option<Tier>,`

Optional since step 9: an override may raise the tier floor, ask for a
cross-family second opinion, or both. Requiring it would force a no-op
`start_at = "small"` on anyone who wants only the second reviewer.

## `effort: Option<String>,`

Optional override of the tier's default reasoning effort (§10), used
when no explicit role policy applies. A pin is the narrower way to buy
a deliberate effort for one tier.

## `timeout_secs: Option<u64>,`

`[routing] review = { timeout_secs = 5400 }`. Kept on this raw shape
because `review` shares the routing table with task kinds; rejected on
task-kind entries below so a misplaced timeout is never ignored.

## `enabled: Option<bool>,`

`[routing] review = { enabled = false }` — the explicit opt-out of
§11.2 review, for plans where a frontier judgement per task costs more
than the work it is judging.

## `#[derive(Debug, Default, Deserialize)]`

`[pools.*]`, with each entry's byte offset kept.

`toml::Spanned` rather than a plain value because a `BTreeMap` iterates in
**sorted key order**, and both `Config.pools` and
[`crate::capacity::pool_for`] promise *file* order — "moving a pool up the
file promotes it" is the whole mechanism an operator has for choosing
between two accounts on one vendor (§13's profiles). Sorting silently
substituted an alphabet for that choice. The span is the offset of the
entry's value in the source, so re-sorting by it restores exactly what was
written, with no new dependency.

## `#[derive(Debug, Default, Deserialize)]`

One `[pools.<name>]` entry, before validation. Every field is optional here
so a shape mistake reports as a named problem rather than a serde error
about a struct the config author never wrote.

## `profile: Option<String>,`

§13's credential-profile seam (D2): which account this pool draws from.

## `#[serde(flatten)]`

Everything else, so a typo warns by name instead of vanishing.

## `#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize)]`

`[budgets]` (§17). API-equivalent dollars; omitting either means unlimited.

`deny_unknown_fields` because §13 lists a third budget kind this build does
not have — per-pool fractions — so `pool_fraction` is the key an operator
reading the design reaches for first. Accepting it silently would let them
believe they had capped a pool while the run spent against no ceiling at
all, which is the one failure mode a budget must not have.

## `pub fn check_budget(name: &str, limit: f64) -> Result<(), String> {`

One ceiling, checked the same way wherever it came from.

Shared with the `--budget` flag rather than living only in the `[budgets]`
parser: a flag that overrides a validated key must not be a way around the
validation. Zero and negative both stop the run before it spends anything,
and NaN silently never fires — three different broken behaviours behind one
mistyped number.

## `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`

`second_opinion` on a `[[routing.overrides]]` (§11.3).

One variant today. It stays an enum rather than a bool because §11.5
generalizes the reviewer into a list of passes with a lens each, and the
security lens arrives here as a second variant with a different ladder
dispatch — not as a second boolean.

## `DifferentVendor,`

A second reviewer from a different model *family* must also pass. The
spelling is §17's; the semantics are §11.3's ("a different model
family"), which is the stricter of the two — see [`crate::catalog::Family`].

## `const ACCEPTED: [&'static str; 1] = ["different-vendor"];`

Accepted spellings, named once so the parser and its error message
cannot disagree about what is legal.

## `pub start_at: Option<Tier>,`

`None` when this override exists only to request a second opinion.

## `#[derive(Debug, Clone)]`

One `[[gates]]` entry (§17). `None` for the whole list means the section
was absent and the engine derives defaults from the repo's shape.

## `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`

`[engine] on_task_failure` (§17).

This governs only a *genuinely failed* task — one a human declined to
unblock, or one whose chain resolved to nothing. A task parked on a
question never halts the run whatever this says: invariant 6 ("questions
never stop the runnable frontier") is not configurable.

## `pub const DEFAULT_MAX_PARALLEL: u32 = 1;`

`[engine] max_parallel` (§17), and the only value this engine accepts.

One attempt at a time is not a tuning choice here, it is the whole shape of
the v0.1 scheduler: one worktree, one candidate, one commit. Parallelism
arrives with the topology engine, and until it does a higher ceiling can only
be a promise the run does not keep.

## `pub const DEFAULT_MAX_MERGE_REPAIRS: u32 = 2;`

`[engine] max_merge_repairs` (§17): autonomous repair generations per
original task before the ladder asks a human instead.

## `pub const LAST_SEQUENTIAL_SCHEMA: u32 = 3;`

The last event schema a sequential engine writes.

Runs recorded at or below it are sequential for the rest of their lives —
they never upgrade into a parallel topology — so their ceilings are read as
a statement about some *other*, future run rather than as an instruction to
this one. See [`EngineLimits`].

## `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`

Which reading of the `[engine]` ceilings a load is performing — and, since
2026-09-05, which reading of today's `[[gates]]` section: the one that
governs the run, or one that is only compared with the gates the run
recorded.

The same keys mean two different things depending on what is about to
happen, and the difference is not cosmetic — it is the difference between a
refusal and a warning.

A run **being created now** is a promise about to be made: `max_parallel = 4`
would have the operator budget wall-clock and spend for four workers and get
one, so it is refused before anything exists.

A run being **resumed** already exists. Its semantics were fixed when it
started, and today's config cannot change them; the only question is whether
it may continue. Refusing there does not prevent a broken promise — the run
is already sequential and will stay sequential — it merely strands a run the
operator can no longer reach, because a key they added for a future run is
sitting in a file the resume happens to re-read. That is a worse outcome
than the one the refusal exists to prevent, so the resume warns, keeps its
recorded sequential ceiling, and continues.

## `Fresh,`

A run about to be created, or a preview of one (`upstroke validate`).
Today's config is the whole truth: every section governs.

## `SequentialResume,`

A resume of a run a sequential engine recorded, whose log predates the
gate record (`run_started.gate_cmds` absent and no earlier resume
established one) — or any caller that passes this variant to a load
directly. The ceilings warn as above, but today's `[[gates]]`
**governs**: this resume settles the run's gates from it and records
them on `run_resumed`, so it is read exactly as a fresh run reads it
and refuses what a fresh run refuses. That is the gate reading this
variant has always had; a 2026-09-05 draft gave it the compare-only
reading below instead, which silently changed what a direct caller
got, and the reading moved to a variant of its own.

## `SequentialResumeWithRecordedGates,`

A resume of a run a sequential engine recorded, whose log records its
gates. The ceilings warn (above). Today's `[[gates]]` is **compared**
with the record and never refused over — `design/15`: gates are taken
from the record, not re-derived, and not refused over — so every shape
a fresh run refuses there is a warning naming the recorded gates as
what runs. Chosen only by [`EngineLimits::for_resume`] from the log's
own record; a caller that has not read the log has no business passing
it.

## `#[must_use]`

The reading that applies to a resume of a run at `effective_schema`
whose log does (`gates_recorded`) or does not record its gates —
`events::recorded_gates(..).is_some()`, which reads `run_started` and
any earlier resume that established them.

Anything past [`LAST_SEQUENTIAL_SCHEMA`] is not a sequential run's
resume, so it gets the ordinary reading rather than the legacy one.
Today that path is unreachable — no schema above it exists — and what it
should do once one does is the activation question, which is not this
slice's to answer.

## `#[derive(Debug, Clone, PartialEq, Eq)]`

One extra mount an operator asked the boundary to receive.

DESIGN.md:612 names `image, mounts` as `[runner]`'s two configurable halves.
The runner composes the role's own worktree mount, the read-only reviewer
mount and the per-agent credential volume itself; this is the operator's
addition to that set — a toolchain cache, a shared model directory.

**`read_only` defaults to `true`.** A mount the operator did not describe is
the one whose blast radius they did not think about, and DESIGN.md:398 is
explicit that "the v0.2 execution root is deliberately non-authoritative":
a writable host path handed to gate-executed repository code is the class
the container runner exists to bound. Writable is a thing you say.

## `pub target: String,`

Where the boundary sees it. A container-side absolute path, so it is a
`String` and not a `PathBuf`: it is never resolved on this machine.

## `#[derive(Debug, Clone, PartialEq, Eq)]`

What `[runner]` selects, before anything has inspected a runtime.

**Not** a [`RunnerPolicy`]: that record carries the runtime's immutable image
id and its manifest digest, which only inspection can establish. This is the
operator's *request* — the input `crate::runner::container::resolve` turns
into a record, and the value INV-23's rebuild path compares against a
recorded one ("today's `[runner]` config that differs warns naming the
difference and is ignored").

[`RunnerPolicy`]: crate::topology::events::RunnerPolicy

## `pub kind: RunnerKind,`

PR3's wire kind, and deliberately not a second enum: the config and the
record have to be comparable, and two spellings of one choice are two
things that drift.

## `pub image: Option<String>,`

The image reference, for a container selection.

## `pub credential_volumes: BTreeMap<String, String>,`

Per-agent credential volume names (R20).

## `pub mounts: Vec<RunnerMount>,`

Extra mounts, parsed and carried. Nothing in this slice acts on them —
`production_effect` is "none" — and, more durably, they are **not part of
the recorded execution identity**: INV-23's `RunnerPolicy` has four
fields and none of them is a mount list.

## `pub from_config: bool,`

Whether a `[runner]` section was present at all.

Load-bearing for the rebuild path: an **absent** section is not "a config
that differs", so a resume with no `[runner]` in the file must not warn
that its runner kind moved.

## `#[must_use]`

What an absent `[runner]` section means: the host runner, nothing else.

## `pub pools: Vec<Pool>,`

`~/.upstroke/pools.toml`, in file order — which is preference order for
[`crate::capacity::pool_for`].

## `pub budgets: Budgets,`

`[budgets]` (§17); both keys optional, both meaning unlimited when absent.

## `pub ask_before: AskBefore,`

`[interaction] ask_before` (§12).

## `pub gates: Option<Vec<GateConfig>>,`

`Some` (possibly empty — explicitly no gates) when `[[gates]]` was
configured; `None` means derive from the repo.

## `pub review_tier: Option<Tier>,`

`[routing] review = { tier = … }` (§11.2). `None` means the frontier
default.

## `pub review_enabled: bool,`

`[routing] review = { enabled = false }` opts out of review entirely.

## `pub review_pass_timeout: Duration,`

Independent wall-clock allowance for each review pass. Unlike a worker
attempt timeout this is frozen into [`crate::review::ReviewPlan`], so a
resume cannot silently adopt a different verification budget.

## `implementation_effort_override: Option<Effort>,`

Explicit role policy. A role setting outranks pin and tier defaults so
`implementation = "xhigh"` really does mean every worker attempt.

## `pub on_task_failure: OnTaskFailure,`

`[engine] on_task_failure` (§17); default `Halt`.

## `pub max_parallel: u32,`

`[engine] max_parallel` (§17): the ceiling this load's run may actually
execute at, [`DEFAULT_MAX_PARALLEL`] by default.

The *effective* ceiling, not a transcription of the file. Above the
default it is refused outright for a fresh run, and for a sequential
run's resume it is warned about and left at the default the run has been
executing at all along — a run's execution shape is a fact about the
run. See [`EngineLimits`], which is what chooses between the two; the
`parse_engine` reader below is where the choice is made.

## `pub max_merge_repairs: u32,`

`[engine] max_merge_repairs` (§17); [`DEFAULT_MAX_MERGE_REPAIRS`].
Validated and kept, acted on by the topology engine.

## `pub max_per_agent: u32,`

`[engine] max_per_agent` (§17); defaults to the configured
`max_parallel`. Validated and kept, acted on by the topology engine.

## `pub max_per_pool: u32,`

`[engine] max_per_pool` (§17); defaults to the configured
`max_parallel`. Validated and kept, acted on by the topology engine.

## `pub interaction_mode: InteractionMode,`

`[interaction] mode` (§12); default `on_block`.

## `pub notify: Vec<String>,`

`[interaction] notify` (§12); default `["cli"]`.

## `pub wait_on_block: Duration,`

`[interaction] wait_on_block_secs` (§12/§19). How long a detached but
interactive run waits at a hard block for an answer to arrive as an
event before ending parked. `ZERO` disables the wait, which is what a
terminal-attached run and CI both want.

## `pub runner: RunnerSelection,`

`[runner]` (DESIGN.md:612). Always the host selection in this build:
`kind = "container"` is refused for every schema-1..3 fresh run and
resume before any effect — see [`refuse_legacy_container_selection`].

## `struct EngineSettings {`

Everything `[engine]` contributes, kept together so adding a knob does not
widen a tuple every caller has to re-destructure — the reason
[`InteractionSettings`] below exists, applied to the section that just grew
four keys.

## `struct InteractionSettings {`

Everything `[interaction]` contributes, kept together so adding a knob does
not widen a tuple every caller has to re-destructure.

## `pub const DEFAULT_WAIT_ON_BLOCK: Duration = Duration::from_secs(30 * 60);`

§12's default hard-block wait for a detached interactive run: long enough
that an operator answering from a phone finds the run still going, short
enough that a forgotten run gives its workspace and branch back the same
day.

## `pub fn effort_for(&self, tier: Tier) -> Effort {`

The tier-bound effort before a role policy is applied: a pin's override,
else the tier's default (§10).

## `pub fn implementation_effort(&self, tier: Tier) -> Effort {`

Effort every implementation attempt uses. An explicit role policy is
global across task kinds and tiers; otherwise the tier/pin rule applies.

## `pub fn review_effort(&self) -> Effort {`

Effort every reviewer judges at. The role policy wins when present;
otherwise use the review tier, with §11.2's frontier default.

## `pub fn resolved_effort_policy(&self) -> ResolvedEffortPolicy {`

Resolve the full role policy once so a run can record and retain it.

## `pub const DEFAULT_REVIEW_PASS_TIMEOUT: Duration = Duration::from_secs(90 * 60);`

Frontier reviews can legitimately spend tens of minutes reading a broad
diff. This is per pass, including its one verdict-format re-ask.

## `pub fn default_chain(kind: TaskKind) -> Vec<Tier> {`

Derived default escalation chain per kind (DESIGN.md §10.1), used when the
repo config is absent or silent for that kind.

## `pub fn load(`

Load effective config.

`repo_config`: explicit `--config` path (missing file = error) or `None`
to look for `upstroke.toml` in `discover_in` (missing = silent defaults).
`discover_in` is the repo root the run targets — never the process CWD,
which can differ and would load another repo's config.
`pools_file`: explicit pools path (tests) or `None` to discover
`~/.upstroke/pools.toml` (missing = silent).

## `pub fn load_limits(`

[`load`] for a caller that is not creating a run.

Only `[engine]`'s ceilings and the `[[gates]]` section read `limits`, and
only to decide whether refusing or warning is the answer: a ceiling this
engine cannot honour, and a `[[gates]]` section that a resume compares
with the gates its run recorded rather than runs — see [`EngineLimits`].
Every other key means the same thing either way.

## `pub fn load_with(`

[`load`] with the adapter registry injected.

Only `[pools]` consults it, to decide whether a pool names an agent this
build can drive. Injected for the same reason
[`crate::validate::builtin_adapter`] is: the engine resolves adapters
through a `Harness`, not through the global registry, so a guard that asks
the registry directly is answering a question about a different set than the
one that will actually run — and the unusable-pool path could only ever be
tested with an agent the binary genuinely lacks.

## `pub fn load_captured(`

[`load_limits`] from bytes that were captured earlier.

The only entry point that can be reasoned about across a lock: everything it
parses comes out of `captured`, so "what was validated" and "what was
captured" are the same bytes rather than two reads that happened to agree.
See [`CapturedConfig`].

## `pub fn load_captured_with(`

[`load_captured`] with the adapter registry injected — see [`load_with`].

## `if key == "review" {`

`review` is a routing ROLE, not a task kind (DESIGN §17's
own example configures it). Parse and echo it rather than
warning users off their own documented config; the reviewer
consumes it in step 6.

## `let second_opinion = match ov.second_opinion.as_deref() {`

A misspelled value here silently deletes a verification layer:
the operator asked for two model families on their blast-radius
paths and would get one, with nothing said. That is the same
reason `[interaction] mode` errors rather than warns.

## `if ov.start_at.is_none() && second_opinion.is_none() {`

Both keys are optional individually, but an override that raises
nothing and asks for nothing does nothing — and reads exactly
like one whose key was misspelled into oblivion.

## `let effort = match pin.effort.as_deref().map(Effort::parse) {`

Validated here rather than discovered at spend time: the provider
rejects an unknown effort with a 400 *after* the turn has started
(measured 2026-08-11), so a typo costs a whole attempt instead of a
config error. Same posture as the pinned-model check above.

## ``const RUNNER_KEYS: &str = "`kind`, `image`, `credential_volumes`, `mounts`";``

Every accepted `[runner]` key, written out.

The error messages name this rather than a serde-derived list, so a key that
stops being read is a key that stops being offered.

## `fn parse_runner(`

Parse `[runner]`, then refuse a container selection this engine may not make.

Two steps and not one, deliberately. [`read_runner`] is the whole of the
parse and accepts `kind = "container"`; [`refuse_legacy_container_selection`]
is the refusal `slice_contract.expected_failures_refusals[0]` names. Keeping
them apart means the refusal is an independently droppable predicate — a
mutation that deletes it does not also delete the ability to describe a
container runner — and it means the resolution path can be given a parsed
container selection without going through a door this engine keeps locked.

## `mod parse;`

---------------------------------------------------------------------------
The section readers
---------------------------------------------------------------------------

## `fn repo_config_location(repo_config: Option<&Path>, discover_in: &Path) -> (PathBuf, bool) {`

Where a load looks for the repo config, and whether an absent file there is
an error.

Split out because [`CapturedConfig::capture`] has to capture *the same* file
the load reads: two copies of "explicit path, else `upstroke.toml` beside the
repo" would be two chances for a pre-lock check to validate a file the run
then does not load.

## `fn pools_location(pools_file: Option<&Path>) -> Option<(PathBuf, bool)> {`

Where a load looks for pools, if anywhere. See [`repo_config_location`].

## `#[derive(Debug, Clone, PartialEq, Eq)]`

One file exactly as it was at one instant: the bytes it had, the fact that
it had none, or the error reading it produced.

This is not a fingerprint taken beside a read — it *is* the read. Everything
downstream of a capture parses these bytes and no others, which is what makes
"the config that was validated" and "the config that was captured" the same
object rather than two reads that happened to agree. A digest, or a
modification time, or a second `fs::read` performed next to the real one,
would each leave the same hole: bytes can change and change back between two
observations, and every such scheme reports "unchanged" while the run
executes something nothing ever checked.

The three cases are kept apart rather than collapsed into "some bytes or
not", because the caller owes a different answer to each: an absent
`--config` someone typed is a typo, an absent discovered one is the ordinary
fresh repo, and one that is there but cannot be read is neither.

## `required: bool,`

Whether an absent file here is an error — see [`repo_config_location`].

## `content: Result<Option<Vec<u8>>, (io::ErrorKind, String)>,`

`Ok(None)`: not there. `Ok(Some(_))`: exactly these bytes. `Err`: the
kind and text of the failure, kept so the error a consumer raises reads
the way the direct read's would have.

## `#[must_use]`

The file this describes.

## `pub fn text(&self) -> Result<Option<String>, UpstrokeError> {`

The captured bytes as text, or `None` if the file was not there.

Fails the way the read it replaces would have: an unreadable file, or one
whose bytes are not UTF-8, is a [`UpstrokeError::Io`] against this path.

## `#[must_use]`

One file as it is right now.

## `#[derive(Debug, Clone, PartialEq, Eq)]`

Every file a load reads, captured at one instant.

A validation performed before a lock is only worth the ordering it buys if
what it validated is what the run then uses, and the only way to know that is
for the validation to have no other source. So a caller captures once and
hands the capture to [`load_captured`]; taking the lease and capturing again
then compares two things that are directly comparable, because one of them is
what was parsed.

## `pools: Option<FileSnapshot>,`

Absent when this load has no pools file to read at all — no `--pools`
and no `~/.upstroke/pools.toml` — which is silence rather than emptiness.

## `#[must_use]`

Capture what a [`load_with`] with these arguments would read.

## `pub fn files(&self) -> impl Iterator<Item = &FileSnapshot> {`

The captured files, for a caller that has to name them.

## `mod read;`

---------------------------------------------------------------------------
Reading the captured bytes
---------------------------------------------------------------------------

## `fn missing() -> PathBuf {`

An explicit pools path with no pools in it.

A real, empty file rather than an absent one: an explicit `--pools` that
does not exist is now a hard error (a path someone typed and that is not
there is a typo), and passing `None` here would reach for the operator's
real `~/.upstroke/pools.toml` — which no test may touch.

## `static PATH: OnceLock<PathBuf> = OnceLock::new();`

Created once: the file is identical for every caller, and rewriting
one shared path from parallel tests means truncating it under a
reader.

## `fn hermetic() -> PathBuf {`

Empty discovery root so tests never pick up a real upstroke.toml.

## `assert_eq!(cfg.review_tier, Some(Tier::Frontier));`

`review` is a routing role, not a task kind: parsed, echoed, and
never warned about (DESIGN §17 configures it in its own example).

## `let path = scratch(`

Requiring `start_at` would force a no-op `start_at = "small"` on
anyone who wants a cross-family reviewer on paths whose difficulty
is already routed correctly.

## `assert_eq!(`

With no floor to apply, routing is untouched.

## `let path = scratch(`

Warning and carrying on would run the task with ONE reviewer while
the config says two — a verification layer deleted in silence.

## `let path = scratch(`

Indistinguishable from one whose only key was misspelled into a
section serde ignores, so it cannot be waved through.

## `let path = scratch(`

What makes a tier mean something to an agent with an effort axis: a
chain that escalates has to move this too, or every rung thinks
exactly as hard as the last one.

## `assert_eq!(cfg.effort_for(Tier::Frontier), Effort::Max);`

The pin wins over the tier's `high` default when no role policy is
present — the original behavior remains intact.

## `assert_eq!(cfg.review_effort(), Effort::Max);`

Reviewers judge at the review tier, which defaults to frontier.

## `let path = scratch(`

The provider rejects an unknown effort with a 400 after the turn has
started (measured), so a typo would otherwise cost an attempt and
report as an agent failure. Same posture as the pinned-model check.

## `assert_eq!(max.profile.as_deref(), Some("personal"));`

D2's seam: two Claude Max pools differing only in `profile` parse and
stay distinct. Nothing acts on the field in v0.1 — this is the shape
being right ahead of the behaviour, deliberately.

## `let err = load_pools(`

`kind` decides which estimator rule runs.

## `let err = load_pools(`

Dropping `signals` by typo would discard §13's ground truth while the
file still claims to have it.

## `for bad in ["safety_margin = 1.5", "reserve = -0.2"] {`

A "150% margin" has no degraded reading, only a wrong one.

## `warnings.clear();`

§17's own example ships `agent = "aider"`, which has no adapter in
v0.1. Erroring would brick anyone who copied the documented file.

## `let path = scratch("gatestable.toml", "[gates]\ncheck = \"cargo check\"\n");`

`[gates]` as a table — the classic array-of-tables mistake.

## `let path = scratch(`

Wrong field type inside an entry.

## `let path = scratch("enginetype.toml", "[engine]\nshell = 5\n");`

[engine] with a wrong type.

## `let path = scratch("nowait.toml", "[interaction]\nwait_on_block_secs = 0\n");`

Zero is a real setting, not "unset" — it is how an operator says a
detached run should end parked rather than hold the workspace.

## `assert_eq!(cfg.ask_before.frontier_escalation_over_usd, Some(5.0));`

Parsed and acted on since step 10 — the "needs the ledger" warning it
used to carry expired when the ledger landed.

## `let path = scratch(`

Warning and carrying on would run past the spend the operator asked
to approve, with nothing said — the `second_opinion` lesson, applied
to money.

## `for bad in ["run_usd = 0.0", "task_usd = -1.0"] {`

Zero and negative both have two readings — "stop before starting" and
"no limit" — and which one happened must never be a surprise.

## `let cfg = load(None, &hermetic(), Some(&missing()), &mut warnings).expect("defaults");`

Absent means unlimited, silently.

## `let path = scratch("badmode.toml", "[interaction]\nmode = \"always\"\n");`

Both decide whether the run stops or waits for a human. A typo that
silently reverts to the default is not a recoverable surprise.

## `let path = scratch(`

`pool_for` takes the first match and its doc promises "table order as
preference", which is the only mechanism an operator has for choosing
between two accounts on one vendor. A `BTreeMap` silently substituted
an alphabet for that choice — and every fixture happened to be
alphabetical already, so nothing noticed.

## `let path = scratch(`

§13 lists per-pool budgets, so `pool_fraction` is the key someone
reading the design reaches for first. Accepting it silently would let
them believe a pool was capped while nothing capped it.

## `let absent = env::temp_dir()`

Same rule `--config` has had: a path someone typed and that is not
there is a mistake, and answering it with "no pools connected — run
`upstroke connect`" sends them to regenerate a file that was fine.

## `let mut warnings = Vec::new();`

The four ceilings have to exist as values before anything can be
said about a config that sets them — and a fresh repo must reach
them without writing an `[engine]` section at all.

## `let path = scratch("engineshellonly.toml", "[engine]\nshell = \"bash\"\n");`

Same values through a section that configures something else, so the
defaults are the parser's and not an artifact of the absent-table path.

## `let mut warnings = Vec::new();`

The refusal this section exists for. Accepting `max_parallel = 4`
and then running one attempt at a time would have the operator
budget a wall-clock and a spend for four workers and get one, with
nothing said — so it errors at load, which is before a lock, a
workspace, or a run directory exists.

## `warnings.clear();`

One is not merely tolerated — it is the engine's actual behaviour, so
writing it down deliberately must not warn.

## `let path = scratch("resumeparallel.toml", "[engine]\nmax_parallel = 3\n");`

The refusal above protects a promise about to be made. A run that
already exists has made its promise, and it was a sequential one —
refusing there would not prevent anything, it would only strand a run
whose one fault is that someone edited a file it re-reads on the way
back in. So the same value that stops a fresh run lets a resume
through, says so by name, and leaves the recorded ceiling in place.

## `let mut fresh_warnings = Vec::new();`

The same file, one line earlier in its life, still refuses.

## `for key in [`

What the resume softens is that one ceiling, not validation. A limit
with no meaning at all is refused for a resume exactly as for a fresh
run — otherwise "legacy" would become a way around every check.

## `#[test]`

The `[[gates]]` twin of the ceiling rule above, witnessed at the level
the property lives at: a *load*. `design/15`: "gates are taken from the
record, not re-derived — and not refused over", so on the resume of a
run whose log records its gates, today's section is compared and never
refused over — every shape a fresh run refuses there is a warning and
the load succeeds — while the same bytes refuse a run being created
now, and a resume of a log with **no** gate record (`SequentialResume`,
the reading a direct caller has always had), which settles its gates
from today's file. The engine's own composition (which reading
`resume.rs` chooses, and that the record is what then runs) is
witnessed in `engine::tests`, not here.

## `for schema in 1..=LAST_SEQUENTIAL_SCHEMA {`

Sequential forever, by the topology design: a run recorded at schema
1, 2 or 3 never becomes a parallel one, so its resume reads the
ceilings as a statement about some future run. Anything past that
ceiling is not a sequential run's resume and gets the ordinary
reading — which is today's refusal, because whether a topology run
may raise its own ceiling is the activation question, not this one.

## `assert_eq!(`

The same run with no gate record: its gates are settled from
today's config on this resume, so that section governs — the
reading `SequentialResume` has always given a direct caller.

## `let mut warnings = Vec::new();`

Zero reads as both "no ceiling" and "nothing may run", and a limit
whose meaning depends on which the reader assumed is not a limit.
Every one of the four is checked: a rule that holds for `max_parallel`
alone is a rule the next key added here quietly escapes.

## `for body in [`

A value of the wrong shape is a mistake about the same setting, and
must not fall through to the default the way an omitted key does.

## `let path = scratch(`

`[engine]` used to drop every key it did not know, then (from
2026-09-04) to warn by name and carry on. Neither survives
`on_task_failur = "continue"`: the key the operator wrote decides
whether a failed task stops the run, and a warning beside a run
that then halts is a deleted control with a footnote. So an unknown
key here is refused, every one of them named, on every reading —
a misspelled key governs no run, recorded or not.

## `let path = scratch(`

These three bound a topology this engine does not have: they are not
wrong, they are early. So they parse, they are kept for the run that
will read them, and each one says out loud that today's run does not
— which is the whole difference between an unacted-on key and an
ignored one.

## `warnings.clear();`

Written at their defaults they change nothing, so there is nothing to
announce — the warning tracks the *value*, not the presence of a key.

## `let path = scratch(`

The section grew four keys; the two it already had must still be
consumed from the same table, and the shell warning must still be the
soft one while `on_task_failure` stays hard.

## `let refusing = "[engine]\nmax_parallel = 3\n";`

The capture/read/restore race, driven by hand at the only speed a
test can drive it. `refusing` is a config this engine must not run;
`accepted` is one it may.

The interleaving is the dangerous one, A to B and back to A: capture
while the file says A, let it say B for exactly as long as the
validation takes, restore A before the confirmation looks. An
implementation that fingerprints the file and then reads it again for
the parse validates B, later compares two equal A captures, concludes
nothing moved, and runs A — a config whose required refusal never
fired. Nothing downstream can detect that, because by then both
observations agree.

What closes it is not a better comparison, it is having one read. The
capture *is* the parser's input, so the answer below is A's.

## `fs::write(&path, refusing).expect("A restored");`

And back to A. The confirmation an engine performs here agrees with
the capture — which is the trap, not the proof: agreement is only
worth something because the thing it agrees with is what was parsed.

## `let path = scratch("abaaccepted.toml", accepted);`

The same claim the other way round, so this cannot pass by refusing
everything: a captured config that is fine stays fine while the file
is briefly one that would be refused. A run must inherit neither a
refusal nor an acceptance from bytes it never held.

## `let repo = scratch("capturedpools-config.toml", "[engine]\nshell = \"bash\"\n");`

Two files feed a load, and a capture that covered one of them would
leave the other free to move unobserved between a check and its use.

## `fs::write(`

A pool named only by the transient file must not reach the config.

## `let path = scratch(`

The name is what an attempt is attributed to; blank is
indistinguishable from "no pool" by the time it reaches the ledger.

## `#[test]`

-----------------------------------------------------------------------
`[runner]` (DESIGN.md:612)
-----------------------------------------------------------------------
An absent `[runner]` section is the host runner, and says it was not
configured.

`from_config` is not decoration: INV-23's rebuild path warns when
"today's `[runner]` config differs", and a repository that never wrote
one has no config to differ. The two halves are asserted separately
because a default that set `from_config: true` would be invisible in the
`kind` alone.

## `#[test]`

The section parses every key DESIGN.md:612 names, and a mount is
read-only unless the operator said otherwise.

`read_runner` rather than `load`, because `parse_runner` refuses a
container selection outright in this build and the parse is what is under
test here. The two are separate functions for exactly this reason.

Second field held constant: the `kind`, which is `container` in every
assertion, so what varies is only which key is being read.

## `read_only: true,`

Writable is a thing you say.

## `assert_eq!(`

The two mounts differ in `read_only` and only one of them said so, so
the default is doing the work rather than the fixture.

## `#[test]`

Every shape `[runner]` refuses, each with the reason, and each named.

An unknown key is an **error** here where `[engine]` warns, and the grid
says so out loud: a mistyped key in `[engine]` leaves a ceiling at its
default, while `knid = "container"` leaves the run executing on the host
while its config reads as though gate code were confined.

Second field held constant: every cell is a `[runner]` section and
nothing else, so no cell can fail for another section's reason.

## `"[runner]:",`

A `[runner]` that is a scalar cannot be written as a section
header, so this cell is driven directly below.

## `let ok: toml::Value =`

The control: the shape these are all variations on is accepted, so
the refusals above are about what each cell changed.

## `#[test]`

The `[runner]` a `upstroke.toml` writes is the value resolution consumes,
end to end.

PR12's activation is one call — `refuse_legacy_container_selection` in
[`parse_runner`] — and everything on either side of it already works.
This drives the whole chain that will be live then, from TOML bytes to a
`RunnerPolicy` that PR3's `completeness()` accepts, so the two halves are
known to fit rather than assumed to.

Second field held constant: the runtime, which holds exactly what the
TOML names, so every assertion is about the value that crossed the seam.

## `let empty = FakeRuntime::new(ContainerTrace::off());`

The control: the same TOML against a runtime that holds nothing is
refused, so the success above is about what the runtime had.

## `#[test]`

A container selection is refused under both readings; a host one is not.

The unit-level twin of
`runner::container::resolve::tests::legacy_container_selection_refused_before_effects`,
which drives the same refusal through both write commands. This one
pins that the refusal is a property of `refuse_legacy_container_selection`
alone, so deleting it from `parse_runner` is a distinct, separately
witnessed kill.

## `assert_eq!(`

The two selections differ in the kind and in nothing else.

## `let expected = match limits {`

The message says which reading it is, so an operator can tell a
refused fresh run from a refused resume.
