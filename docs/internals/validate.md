# `src/validate.rs`

Extended notes for [`src/validate.rs`](../../src/validate.rs).

These notes preserve the module comments after the annotation repairs. Item headings quote source lines for navigation.

## Module

`upstroke validate`: parse → config → graph checks → routing preview →
rendered report. No execution of anything.
LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub config_path: Option<PathBuf>,`

Explicit `--config` path; `None` looks for `upstroke.toml` in
`config_root`.

## `pub config_root: PathBuf,`

Root of the repo the plan targets: config discovery and gate
derivation both resolve here, never against the process CWD.

## `pub pools_path: Option<PathBuf>,`

Pools file override for tests; `None` discovers `~/.upstroke/pools.toml`.

## `pub engine_limits: config::EngineLimits,`

Which reading of `[engine]`'s ceilings applies (see
[`config::EngineLimits`]). `Fresh` for `upstroke validate` and for a run
about to be created; a resume passes the reading its own recorded schema
selects.

Carried here rather than decided inside `analyze` because only the
caller knows which it is, and the difference is a refusal.

## `pub review: String,`

Who reviews, and where a second opinion applies (§11.2–§11.3).

## `pub effort: String,`

Effective reasoning policy before any process is spawned.

## `#[derive(Debug)]`

The shared front half of `validate` and the engine's pre-flight (§14:
"plan parses cycle-free"): parse, load config, check the graph, resolve
every routing chain. Executes nothing.

## `pub chains: Vec<ResolvedChain>,`

One resolved chain per task, aligned with `plan.tasks`.

## `pub gates: Vec<ShellGate>,`

Effective gates: `[[gates]]` verbatim, else derived from the repo's
shape (§17) — the single derivation point for validate and the engine.

## `#[derive(Debug, Clone, PartialEq, Eq)]`

Every file an [`Analysis`] is derived from, captured at one instant.

The set has to be *complete* to be worth anything. A capture that covers the
config but not the plan, or the plan but not the files the gate derivation
reads, licenses exactly the confusion it was introduced to rule out: a caller
compares equal captures, concludes nothing moved, and adopts an analysis that
depended on something outside the comparison. So this names all of them, and
[`analyze_captured`] parses out of it rather than beside it.

## `gate_inputs: Vec<config::FileSnapshot>,`

The worktree files the gate derivation looks at when `[[gates]]` does not
spell the gates out: `Cargo.toml`, `go.mod`, and `package.json` beside
the repo root, which are what [`crate::gates::derive`] consults and the
whole of what it consults. Captured here so that a change to one of them
is a change to this analysis's inputs and not an unobserved edit —
keep this list in step with `gates::derive` itself.

## `const GATE_DERIVATION_INPUTS: &[&str] = &["Cargo.toml", "go.mod", "package.json"];`

The gate derivation's inputs, relative to the repo root — see
[`CapturedInputs::gate_inputs`].

## `#[must_use]`

Capture what an [`analyze`] with these options reads.

## `pub fn paths(&self) -> Vec<PathBuf> {`

Every captured file, in a stable order, for a caller that has to name
them in a message.

## `pub fn analyze(opts: &ValidateOptions) -> Result<Analysis, UpstrokeError> {`

Capture validation inputs and resolve the plan, configuration and task chains.

# Errors
Returns the contextual input, configuration, graph or adapter refusal from
[`analyze_captured`], including any warnings gathered before it failed.

## `pub fn analyze_captured(`

[`analyze`], out of bytes that were captured earlier.

The plan, the repo config and the pools file are parsed from `captured` and
from nowhere else, so the analysis this returns is bound to those exact
bytes: a caller holding the same `CapturedInputs` can prove what was
validated by comparing it against the filesystem, and a file that changed and
changed back cannot slip between the check and the answer, because there is
only one read.

The one input still read from the filesystem here is the gate derivation's:
[`crate::gates::derive`] takes a directory, and the three files it looks at
are captured but not consumed. A caller that needs the derivation pinned runs
this where the worktree cannot move — see the engine's pre-flight, which
takes its answer under the worktree lease.

# Errors
Returns the captured input's read or parse error, a configuration refusal,
an invalid task graph, or an unsupported pinned adapter. Warnings gathered
before a refusal accompany its original category in
[`UpstrokeError::WithWarnings`]; failures without warnings keep that category.

## `let raw = captured.plan.text()?.ok_or_else(|| UpstrokeError::Io {`

Named off the capture rather than off `opts`, so an error cannot report a
path other than the one that was actually read.

## `Some(configured) => configured`

Analysis retains the config and a separately owned executable
gate list, so the names and commands are copied into that snapshot.

## `pub fn builtin_adapter(agent: &str) -> bool {`

Whether this build ships an adapter for `agent`.

Injected into the checks below rather than called from them, so the guards
can be tested against agents that do and do not exist without waiting for
the registry to grow one.

## `fn check_pin_adapters(`

A pin naming an agent with no adapter must fail the same way in `validate`
and `run`; otherwise the preview promises a binding the run then refuses at
pre-flight (§18).

Currently unreachable through `upstroke.toml` alone — `config::load` rejects
any pin whose (agent, model) is absent from the catalog, and every catalog
agent has an adapter as of step 9. It stays because that is a coincidence of
today's table, not a property: §13 says the catalog ships ahead of support
(Aider models are catalogued in v0.2 before its adapter lands), and the
moment it does, this is what stops a preview from promising them.

## `pub fn run(opts: &ValidateOptions) -> Result<Report, UpstrokeError> {`

Build a zero-spend validation preview, including routing, gates and reviews.

# Errors
Returns an input, configuration, graph or adapter refusal from [`analyze`],
or a refusal to construct the requested review plan. Warnings gathered
before failure accompany the original typed error; successful previews
return them in [`Report::warnings`].

## `gates::preview_resolution(&analysis.gates, &opts.config_root, &mut warnings);`

Zero-spend preview of the §14 gate pre-flight: warn, never refuse.

## `let reviews = match review::plan_for(`

Who would judge the work (§11.2–§11.3), against the adapters this binary
ships. A run asks the same question of the adapters its own harness
holds, which in production is the same set — so the preview cannot
promise a reviewer the run would then refuse.

## `fn latest_run_observations(`

§13's observations, without executing anything: fold the latest run in this
repository, if there is one.

A missing or unreadable run is not an error here. `validate` describes a
plan; a broken run directory beside it is somebody else's problem, and
refusing to preview a plan over one would be a strange trade.
`has_pools` short-circuits the whole fold. With no pools connected the
capacity block is one line and the observations are never consulted, so
parsing an entire run's log for it is work with no reader — and `validate`
is the fast, zero-spend iteration loop §18 puts on day one.

## `Err(error) => {`

A run that exists but cannot be folded is not "no run" — and
`read_all`'s refusal ("the log has been rewritten…") is exactly the
loud error the event-log design exists to produce, so swallowing it
and reporting an empty repository hid two things at once.

## `pub fn render(&self) -> String {`

The rendered preview.

The surface stays here — it is the one every caller names, and the one
`effects/wrappers.toml` classifies under this module — while the table
it produces is `render::report`.

## `static PATH: OnceLock<PathBuf> = OnceLock::new();`

A real, empty pools file: an explicit `--pools` that does not
exist is a hard error, and `None` would reach for the
operator's own `~/.upstroke/pools.toml`.
Created once: identical for every caller, and rewriting one
shared path from parallel tests truncates it under a reader.

## `fn scratch_root(tag: &str) -> PathBuf {`

A scratch repo root of its own, so a test that rewrites its inputs
cannot be read half-written by another running beside it.

## `fn opts_in(root: &Path, plan: &str) -> ValidateOptions {`

[`opts`], rooted in `root` rather than in the shared hermetic directory.

## `options.config_path = Some(PathBuf::from("fixtures/annotation-invalid-plan.md"));`

This tracked Markdown fixture is deliberately invalid TOML.

## `let root = scratch_root("capturedset");`

Completeness is the property, and it is the one an incomplete capture
silently loses: a caller comparing two equal captures concludes
nothing moved, so anything outside the comparison is free to move.
The plan, the repo config, the pools file, and the three worktree
files the gate derivation consults are the whole set.

## `let root = scratch_root("capturedplan");`

The plan is an input like any other, and it was the one an earlier
capture left out. Same interleaving as the config's: capture, let the
file become something else for exactly as long as the parse takes,
restore it. What comes back has to describe the captured plan.

## `let root = scratch_root("capturedgates");`

`gates::derive` takes a directory, so these three are captured rather
than consumed — which makes it worth proving they are genuinely
inputs, and that a change to one of them is a change the capture sees.

## `let pins = vec![config::Pin {`

Every catalogued agent has an adapter as of step 9, so the guard is
driven directly rather than through a config file it can no longer be
reached from. §13 ships the catalog ahead of adapter support, which is
when this fires for real.

## `let pins = vec![config::Pin {`

And it passes what this build really does ship.

## `let root = env::temp_dir().join(format!("upstroke-validate-review-{}", std::process::id()));`

§18: `validate` and `--dry-run` execute nothing, so they cannot check
that a named reviewer is installed. Saying "would be, if installed"
is the difference between a plan and a promise.

## `let rotate = rendered`

The per-task decision belongs in the row that explains what this
task's paths bought it — and only on the task whose paths matched.

## `assert!(`

D2's seam is echoed even though nothing acts on it.

## `assert!(`

§13's conservatism, visible: an unmeasured pool reads as unknown, and
the block says that is not the same as full.

## `assert!(`

A source the estimate did not read must not pass as accounted for.

## `assert!(rendered.contains("never probes"), "rendered:\n{rendered}");`

§18: this command executes nothing, and says which side of that line
it is on rather than letting a preview read as a promise.

## `let report = run(&opts("fixtures/sample-plan.md")).expect("validates");`

Hermetic root with no markers: no gates, still explicit.

## `let clean = run(&opts("fixtures/sample-plan.md")).expect("sample validates");`

The sample plan wires artifacts along its dependency chain — silent.
