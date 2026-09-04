# `src/engine/assembly.rs`

Extended notes for [`src/engine/assembly.rs`](../../../src/engine/assembly.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The single production authority for **what an invocation runs, and as what**.

### Why this module exists

Two engines now need the same answer. The legacy engine (schemas 1–3) has
always assembled a worker's command inline in [`super::attempt::run_attempt`]
and a gate's inline in [`crate::gates::ShellGate::check`]; the schema-4
driver needs the same three command sets **up front**, in an
[`super::topology::AttemptPlan`], because a plan is a value it appends
`attempt_started` from.

Assembling them twice is this slice's dominant defect class. Of the findings
PR7's review rounds produced, the expensive ones were all one rule
implemented twice: three append-error protocols, two barrier witnesses, two
run-directory censuses, two disagreeing retry rules — and, in this slice's
own dispatch branch, two derivations of a task's predicted region that
disagreed on every glob (`84a3978`).

### What is and is not extracted here

**Minting a `CommandSpec` was never duplicated.** This crate has exactly two
production mints — [`crate::gates::ShellKind::spec`] for a shell command and
`agent::bin::Invocation::spec` for an agent one — and both already document
themselves as the single place. What was about to be duplicated is the
**selection of their inputs**: which prompt, which permissions file, which
timeout, which profile. That selection is what moved here, and it is what
`the_worker_command_has_one_production_assembler` pins.

The gate's selection is **not** here, and deliberately: it is one expression
over data [`crate::gates::ShellGate`] already owns, so it lives on that type
as `ShellGate::command`. Putting it here would make `gates.rs` — which sits
below the engine — depend upward on the engine.

## `#![cfg_attr(not(test), allow(dead_code))]`

**`dead_code` is allowed here for a lib build only, and the shape is the
point.** With `engine::topology` narrowed to `pub(crate)`, this subsystem has
no non-`#[cfg(test)]` caller — which is exactly what
`production_effect = "none"` asserts, and `pub` was what kept the compiler
from saying so. Narrowing it made rustc report **328 items** across this
module tree as never used.

`cfg_attr(not(test), …)` rather than a bare allow, deliberately. A blanket
`#![allow(dead_code)]` would hide a genuinely dead item added later, which is
the class this slice's own review rounds kept finding. Under this form the
**test** build carries no allow, so anything not reached even by a test is
still an error at `-D warnings`. What is silenced is precisely the one true
fact — the production binary does not drive schema 4 yet.

**Remove this when PR12 activates the driver.** At that point the items have
production callers and the allow stops being true rather than stops being
convenient.

## `pub(crate) struct WorkerSubject<'a> {`

What the worker's prompt reads about the task.

Five fields, and [`materialize_prompt`] is the only thing in this path to
touch the task at all — the same narrowing `review::ReviewSubject` made, for
the same reason and against the same wall. The schema-4 driver holds a
`FrozenTaskSpec` from the frozen registry and no `ir::Task` anywhere, so
sharing the assembler would otherwise mean synthesising one: inventing an
id, a kind and a dependency list the prompt never reads. A conversion that
fabricates fields is free to drift from the plan it claims to represent.

Separate from `ReviewSubject` rather than one widened type, because the two
prompts genuinely read different things: a reviewer is handed artifacts
already resolved and never sees `artifacts_in`. Merging them would give each
caller fields it does not read, which is the wall this is climbing over.

## `pub(crate) struct WorkerSubject<'a>` › `pub(crate) title: &'a str,`

The task's one-line title.

## `pub(crate) struct WorkerSubject<'a>` › `pub(crate) body: &'a str,`

Its body, which may be empty.

## `pub(crate) struct WorkerSubject<'a>` › `pub(crate) acceptance: &'a [String],`

Its acceptance criteria.

## `pub(crate) struct WorkerSubject<'a>` › `pub(crate) artifacts_in: &'a [crate::ir::ArtifactId],`

Artifacts the prompt wires in as readable files.

## `pub(crate) struct WorkerSubject<'a>` › `pub(crate) artifacts_out: &'a [crate::ir::ArtifactId],`

Artifacts the worker is asked to produce.

## `impl<'a> WorkerSubject<'a>` › `pub(crate) fn of(task: &'a Task) -> Self {`

The subject of a legacy plan's task.

## `impl<'a> WorkerSubject<'a>` › `pub(crate) fn of_frozen(spec: &'a crate::topology::registry::FrozenTaskSpec) -> Self {`

The subject of a frozen registry entry, which is what the schema-4
driver holds. Deliberately a second constructor rather than a
projection: `TaskEntry::to_task` exists, but building a whole `Task` to
read five `&str` out of it allocates the other ten fields to throw them
away.

## `pub(crate) struct WorkerAssembly<'a> {`

Everything one worker invocation's command is derived from.

A struct rather than ten parameters, and every field is an input the legacy
engine already had at the call site this was lifted from — nothing here is
new, and nothing here is defaulted. A field this type invented would be a
field one engine could set and the other could not.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) adapter: &'a dyn AgentAdapter,`

The bound agent's adapter. Its `id` is the `AgentId` the request
carries, and its `build` is the mint.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) profile: &'a WorkerProfile,`

The routing decision for this attempt: tier, model, effort, pool.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) task: WorkerSubject<'a>,`

What the prompt reads about the task.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) gate_cmds: &'a [String],`

The gate command lines the worker is permitted to run, which the prompt
quotes and the permissions file allows.

## `pub(crate) struct WorkerAssembly<'a>` › `pub paths: &'a RunPaths,`

The run's directories: where the settings file and the artifacts go.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) stem: &'a str,`

The per-task file stem, and the attempt number. Together they name the
settings file, so two attempts of one task never share one.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) attempt: u32,`

Which attempt this is, from 1.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) retry: Option<&'a RetryBrief>,`

On a retry, what the earlier attempts said. `None` on the first.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) workspace: &'a Path,`

The checkout the worker edits.

## `pub(crate) struct WorkerAssembly<'a>` › `pub(crate) resume_session: Option<String>,`

The agent session to resume, when one is being resumed.

## `impl WorkerAssembly<'_>` › `pub(crate) fn command(&self) -> Result<CommandSpec, UpstrokeError> {`

The command this worker invocation runs as.

Permissions first, then the prompt, then the mint — in that order,
because the permissions file's path is a field of the `TaskRun` the
prompt travels in, and an adapter that reads permissions from argv reads
it from there.

The stdin payload is attached here rather than by the caller. It is the
adapter's answer about its own `TaskRun`, so a caller that attached it
would be a second place that had to know which adapters want one.

### Errors

Whatever `materialize_permissions` or `build` returns — a permissions
file that cannot be written, or an agent binary that cannot be resolved.

## `pub(crate) struct ImplementerBinding<'a> {`

The routing facts an implementer's profile is built from.

**Ask for what you read.** [`implementer_profile`] reads exactly these four
and the pool; it never sees a chain, a task or a run. Naming them lets one
construction serve the legacy coordinator, which holds a
[`crate::route::Rung`] and resolves effort from the policy, and the schema-4
driver, which holds a [`crate::topology::events::RungBinding`] that already
carries the effort its run froze.

## `pub(crate) struct ImplementerBinding<'a>` › `pub(crate) tier: Tier,`

Which rung of the chain this is.

## `pub(crate) struct ImplementerBinding<'a>` › `pub(crate) agent: &'a str,`

The agent whose CLI runs the work.

## `pub(crate) struct ImplementerBinding<'a>` › `pub(crate) model: &'a str,`

The model it runs.

## `pub(crate) struct ImplementerBinding<'a>` › `pub(crate) effort: Effort,`

What this tier is worth on an agent with an effort axis.

## `impl<'a> ImplementerBinding<'a>` › `pub(crate) fn of_rung(rung: &'a crate::route::Rung, effort: Effort) -> Self {`

A resolved chain's rung, with the effort the run's policy gives its tier.

## `impl<'a> ImplementerBinding<'a>` › `pub(crate) fn of_frozen(binding: &'a crate::topology::events::RungBinding) -> Self {`

A frozen rung's binding, which already carries the effort its run
resolved — the schema-4 driver never re-reads the policy.

## `pub(crate) fn implementer_profile(`

The profile one implementation attempt runs under.

The one production construction of an implementer's [`WorkerProfile`]. It
was inline in `coordinator.rs`, where the schema-4 driver could not reach
it; a driver that rebuilt it would be a second answer to `permissions`, and
a worker spawned `ReadyOnly` edits nothing while reporting success.

`pool` is passed rather than resolved here because resolving it needs the
run's config: §13 is read-only, so this is **attribution only** — which
subscription pays for the attempt, so the ledger and the estimator can say
so. Nothing routes on it.

## `effort: Some(binding.effort),`

What the rung's tier is worth on an agent with an effort axis:
without this the whole chain runs at one vendor default and
escalating a rung moves nothing (§10).

## `pub struct FrozenPlans<'a> {`

The frozen registry's answer to [`AttemptPlans`], and the run's config
beside it.

**`pub`, and waiting for its production caller like everything else in the
schema-4 path.** `decisions.pr_sequence[8].production_effect` is "none
(TopologyPreview selector only)": `upstroke run` still drives the legacy
coordinator, so the only thing that builds a `RunSeams` today is a test.
PR12 activates the path and this is what it will construct.

Everything here is run-scoped: the gate set, the worker allowance, the pool
table, the CLI versions pre-flight probed. Nothing is per-attempt — that
arrives in the [`PlanRequest`], which is what makes one of these serve a
whole run.

## `pub struct FrozenPlans<'a>` › `pub adapters: &'a dyn AdapterSource,`

Where an agent name becomes an adapter.

## `pub struct FrozenPlans<'a>` › `pub paths: &'a RunPaths,`

The run's directories — where permissions and artifacts live.

## `pub struct FrozenPlans<'a>` › `pub gates: &'a [crate::gates::ShellGate],`

The gate set, in the order the config wrote it.

## `pub struct FrozenPlans<'a>` › `pub pools: &'a [crate::capacity::Pool],`

The pool table §13 attributes spend against.

## `pub struct FrozenPlans<'a>` › `pub caps: &'a [(String, crate::agent::Caps)],`

What pre-flight certified each agent's CLI as.

## `pub struct FrozenPlans<'a>` › `pub worker_timeout: Duration,`

How long one worker invocation may take.

## `pub struct FrozenPlans<'a>` › `pub decisions: &'a [String],`

The operator decisions a judge must honour, as the worker was given
them.

## `impl FrozenPlans<'_>` › `fn cli_version(&self, agent: &str) -> Option<String> {`

What pre-flight certified this agent's CLI as, where it certified one.

## `impl AttemptPlans for FrozenPlans<'_>` › `fn pool_for(&self, agent: &str) -> Option<String> {`

The one production resolution of an agent's pool over the frozen table:
the plan builder below, the reviewer profile beside it, and the driver
filling the retry's `RetryRequest` all read **this**.

**It did not, until `21f1de0`'s round.** `79cd9c8` introduced this method
and said it gave the rule "one production implementation"; the plan
builder and the reviewer profile went on calling
`crate::capacity::pool_for` directly, so the count was three and this
method's only caller was the driver. The two direct calls were
character-for-character copies of this body, so routing them here is a
substitution and not a behaviour change.
`run::tests::the_frozen_pool_table_is_read_through_one_seam` holds the
count at one. `reviews/FINDINGS.md` §19, claim (4).

## `fn inputs(&self, request: &InputsRequest<'_>) -> Result<Rev…` › `artifacts: super::attempt::load_artifacts(`

Through the one production resolver, which reads the same two
artifact lists the worker's prompt wired to real files.

## `fn inputs(&self, request: &InputsRequest<'_>) -> Result<Rev…` › `stem: crate::util::filename_component(entry.display_id.as_str()),`

**Through the sanitiser, exactly as the legacy engine does it.**
See `Self::plan`'s stem for why this is a guard and not a
convenience.

## `fn plan(&self, request: &PlanRequest<'_>) -> Result<Attempt…` › `self.pool_for(&request.binding.agent),`

Through the seam, which is what makes it *the* production
resolution rather than one of three. This read
`crate::capacity::pool_for(...).map(|pool| pool.name.clone())` --
`Self::pool_for`'s body, character for character -- while that
method's own doc said the plan builder read it.

## `fn plan(&self, request: &PlanRequest<'_>) -> Result<Attempt…` › `let gate_cmds: Vec<String> = self.gates.iter().map(|gate| gate.cmd.clone()).collect();`

The cmdlines, not the specs: this is what the worker's prompt quotes
as the bar it has to clear, and it is the same list the gate plans
below turn into commands.

## `fn plan(&self, request: &PlanRequest<'_>) -> Result<Attempt…` › `stem: &crate::util::filename_component(entry.display_id.as_str()),`

**A task id is plan-authored input, and this becomes a filename.**

`WorkerAssembly::command` hands this to
`materialize_permissions`, which does
`dir.join(format!("{stem}.json"))` and writes. A `display_id` is
whatever an `id=` annotation said: `plan/markdown.rs`'s `assemble`
takes `Some(explicit) => explicit` verbatim, and
`keys_by_display_id` checks only the reserved `repair-N-` prefix
and duplicates. So `id=../../x` wrote outside the run directory
until this call existed.

`util::filename_component` is the guard the legacy authority this
module was extracted from already used —
`coordinator.rs`: `format!("{index:02}-{}", filename_component(..))`
— and the extraction dropped it. Its own test names the shapes:
`filename_component_neutralizes_hostile_names` asserts
`"unit/fast"` becomes `"unit-fast"`, and an all-dots result
becomes `"x"`, so `..` is neutralised too.

`PR7-R3-ATTEMPT-001`, round 3. Three rules broken by one omission:
`invariants_preserved[1]`, the one-rule-two-production-places
class in its most consequential form — the copy dropped a
**guard** — and §4's "all paths through `std::path`" honoured
mechanically while the value entering the path was untrusted.

## `fn plan(&self, request: &PlanRequest<'_>) -> Result<Attempt…` › `retry: brief.as_ref(),`

§11.4's brief, when there is one. A first dispatch has no
feedback and passes `None`; a retry passes what the attempts
before it failed on.

## `fn plan(&self, request: &PlanRequest<'_>) -> Result<Attempt…` › `let gates = self`

Through `ShellGate::command`, the one production place a gate's
cmdline becomes a spec.

## `fn plan(&self, request: &PlanRequest<'_>) -> Result<Attempt…` › `let reviewers = if let Some(bindings) = entry.reviews.bindings() {`

Through `FrozenReviews::bindings`, which is where `enabled` gates the
plan — the same reader `FrozenReviews::obliged_lenses` projects, so
the passes this dispatches and the passes the fold requires of the
record are one answer rather than two.

## `fn plan(&self, request: &PlanRequest<'_>) -> Result<Attempt…` › `profile: {`

**The reviewer's effort, from §10's own review axis.** This
said exactly that and then passed `request.binding.effort` —
the *implementer's*, the rung the work ran at. A comment
asserting the opposite of its line is worse than no comment:
it answers the question a reader would otherwise ask.

`ResolvedEffortPolicy::review` is the axis, frozen on the
entry beside the implementation efforts.

## `fn plan(&self, request: &PlanRequest<'_>) -> Result<Attempt…` › `let mut profile = pass.profile(entry.ladder.effort.review);`

**A reviewer's pool is looked up from its own agent, not
inherited from the implementer.** `coordinator.rs` states
the reason on its own line and §11.3/§13 are the citation:
a cross-vendor second opinion draws on a different
subscription than the work it is reviewing.

The legacy caller did this and the extraction did not, so
a schema-4 reviewer drained an empty pool name — the same
class as `PR7-R3-ATTEMPT-001`, one payload over: that
dropped a **guard**, this drops a **value**. Found by
Sol's independent `seams` read, round 3.
