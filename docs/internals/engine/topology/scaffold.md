# `src/engine/topology/scaffold.rs`

Extended notes for [`src/engine/topology/scaffold.rs`](../../../../src/engine/topology/scaffold.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The schema-4 run a dispatch or attempt test drives.

Shared by [`super::dispatch`] and [`super::attempt`] because the two halves
of one lifecycle are tested against one run: an attempt test needs a
dispatched generation, and a dispatch test needs the attempt that never
started. A second run fixture beside this one would be two hand-maintained
copies of a `run_started` record — the class this crate has recorded three
times.

### Why the effects come from `workspace_manager::fixture`

`src/engine/topology/**` is a topology module. `clippy.toml` denies
`std::fs::write`, `std::fs::create_dir_all` and `std::process::Command`
there, **including in `#[cfg(test)]` code** — measured. Everything this
module needs that no funnel owns (`git init`, bytes in a worktree, a child
to kill) therefore comes from
[`crate::workspace_manager::fixture`], which is inside the reviewed funnel
module. Nothing here carries an `allow`.

## `pub(super) const ALPHA: TaskKey = TaskKey(0);`

The two plan tasks every fixture run carries.

## `pub(super) const BETA: TaskKey = TaskKey(1);`

The second, so an assertion about "this task" can be crossed against
another whose generations move independently.

## `pub(super) const AGENT: &str = "claude-code";`

The agents this fixture's pre-flight probed.

## `pub(super) const REVIEW_AGENT: &str = "copilot";`

A second, so a slot pair taken for the worker is distinguishable from one
taken for a reviewer.

## `const NORMALIZED_DIGEST: &str =`

The digest the fold authenticates the normalized plan against. A literal
because this fixture never writes a `plan.normalized.json`; the fold
compares it to `run_started`'s own field and to nothing on disk.

## `fn run_started(fixture: &Fixture) -> RunStarted4 {`

A `run_started` for a real repository: the execution root, the private root
and the base commit are the fixture's own, so an event this run appends is
checkable against the directory the funnels actually touched.

## `pub(super) struct FoldedEmitter {`

---------------------------------------------------------------------------
The emitter
---------------------------------------------------------------------------

## `pub(super) struct FoldedEmitter {`

`coordinator_integration.emit`, minus the append-error protocol.

"build event → serialize → round-trip → `plan_transition` → append the exact
bytes through the Event funnel", which is what an emit *is* when it
succeeds. The protocol for when it does not is `emit.rs`'s (O17) and is
deliberately absent: a second implementation of it living in a test fixture
would be a second thing to keep in step with the one that ships.

Owns its own [`crate::events::log::HarnessEventHooks`] over the **shared**
harness rather than borrowing the effect bundle's, so an ordering assertion
can read the append and the worktree add off one observation list while the
two values are borrowed independently.

## `impl FoldedEmitter` › `pub(super) fn fold(&self) -> &TopologyFold {`

The fold, for the state an event is supposed to have produced.

## `impl FoldedEmitter` › `pub(super) fn durable_events(&self) -> Vec<TopologyEvent> {`

Every event in the log on disk, replayed from its bytes.

## `impl FoldedEmitter` › `pub(super) fn durable_kinds(&self) -> Vec<&'static str> {`

The kinds in the log on disk, in order.

## `impl FoldedEmitter` › `pub(super) fn task(&self, key: TaskKey) -> &TaskFold {`

One task's fold state.

## `impl FoldedEmitter` › `pub(super) fn generation_class(`

The class of the generation `generation` of `key`.

## `impl EventEmitter for FoldedEmitter` › `fn emit(`

**`_hooks` is ignored, and that is the divergence rather than an
oversight.** This emitter's own `EventHooks` is a `TimelineEvents`,
which records each `(site, phase)` into the ordering timeline as well as
into the harness. The shared bundle's `events` family is a bare
`HarnessEventHooks` and does not. Using the parameter here would
silently drop every append out of the timeline, and the ordering
assertions that read it would go green having stopped observing the
thing they order.

The repair is to give the shared bundle the timeline wrapper, not to
take it away from here — but that is a change to test infrastructure
every topology test depends on, which is the shape PR5's round 7 was
reverted for. Recorded instead.

## `pub(super) struct Timeline(Arc<Mutex<Vec<(EffectSiteId, HookPhase)>>>);`

---------------------------------------------------------------------------
The hook bundle, with the two phases the shared harness cannot arm
---------------------------------------------------------------------------

## `pub(super) struct Timeline(Arc<Mutex<Vec<(EffectSiteId, HookPhase)>>>);`

Every `(site, phase)` any funnel reached, in order, with repeats.

The [`HookHarness`] cannot answer an ordering question on its own, and this
is not a defect in it: `coverage()` is a **set** in first-observation order,
because what it exists to prove is that every site executed at least once.
An ordering clause is about *occurrences* — O24 is "verification, then the
retry's append", and by the time a retry runs, both sites have already been
observed once by the dispatch that opened the generation, so a comparison of
first observations is a comparison of the wrong pair and passes or fails for
the wrong reason. Measured: it failed here, on a `retry` whose order was
right.

So the timeline is a second, ordered record kept beside the harness, fed by
the same calls. The harness still receives everything — nothing here
replaces an observation, it only adds one — so the coverage evidence is
unaffected.

## `impl Timeline` › `pub(super) fn positions(&self, site: EffectSiteId, phase: HookPhase) -> Vec<usize> {`

Every position at which `(site, phase)` was reached.

## `impl Timeline` › `pub(super) fn mark(&self) -> usize {`

How much has happened so far — a fence a later assertion counts from.

## `struct TimelineEvents {`

[`EventHooks`] that record into the shared harness and onto the timeline.

`EventHooks::phase` returns nothing — for that family the two phases are
reachability rather than injection points — so this adds no arming, only the
ordered record an append's position in a clause needs.

## `pub(super) struct ArmedEffects {`

[`EffectHooks`] that record into the shared [`HookHarness`] **and** can be
armed at a `Before` or `After` phase.

[`HookHarness::arm`] takes a [`SubEffectPoint`], and `HookHarness::hook`
answers `Proceed` to both phases unconditionally — deliberately: a phase is
reachability, not an injection coordinate. But `T-DISPATCH` and `T-ATTEMPT`
table prefixes that are exactly "between these two effects", and the only
place to stand between two funnels is a phase of one of them.

So the arming is local and the **recording is not**: every call reaches
`HarnessEffects` first, so the observation lands in the one harness
`check_bijection` reads, and only the answer is this type's.

## `impl ArmedEffects` › `pub(super) fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {`

Answer `injection` the next time `site` reaches `phase`.

## `fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {` › `self.timeline.push(site, phase);`

Recorded first and unconditionally: an armed site is still a site the
suite executed, and a bundle that skipped the harness when it had an
answer of its own would drop exactly the observations the fault tests
produce.

## `impl EffectHooks for ArmedEffects` › `fn refusal_cause(&self) -> Option<String> {`

Forwarded, so a poison the inner observer found is reported as poison
and not as a fault this bundle armed; the method is not defaulted for
exactly this reason.

## `pub(super) struct Hooks {`

The five families, with [`ArmedEffects`] in the git seat.

## `impl Hooks` › `pub(super) fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {`

Arm a phase of a git funnel site.

## `pub(super) struct Ran {`

---------------------------------------------------------------------------
The runner double
---------------------------------------------------------------------------

## `pub(super) struct Ran {`

One request the fake runner was given.

## `pub(super) struct Ran` › `pub(super) invocation: InvocationId,`

Which identity it carried.

## `pub(super) struct Ran` › `pub(super) role: ExecutionRole,`

Which seat it occupied.

## `pub(super) struct Ran` › `pub(super) workspace: PathBuf,`

Where it ran.

## `pub(super) struct Ran` › `pub(super) agent: Option<AgentId>,`

The agent it was bound to, if any.

## `pub(super) struct Ran` › `pub(super) command: CommandSpec,`

Its program and arguments.

## `pub(super) struct Ran` › `pub(super) durable_at_spawn: Vec<String>,`

The event kinds the log **on disk** held at the instant this process was
requested.

The only oracle O23 has. "`attempt_started` before spawn" is a claim
about two things that happen at two moments, and every other record here
is read *after* both — so a `start` that spawned first and appended
afterwards leaves an identical `Ran`, an identical durable log and an
identical fold. Measured: with the append moved after the spawn, the
whole of this test stayed green until this field existed.

## `pub(super) const GATE_DIAGNOSTIC: &str = "scaffold gate rejected the diff";`

A [`Runner`] that runs nothing and records everything.

The engine is the conductor: it never implements an agentic loop and never
calls a model. What a test of *ordering* needs from the runner is therefore
not an execution but a record — which identity, which seat, which workspace
— and the workspace is the load-bearing one here, because
`decisions.workspace_candidates.snapshots` says gates and reviewers execute
only in exact snapshots and "worker worktrees and the staging worktree are
never used for verification processes".
What a refused scaffold process prints, so a test can follow it into the
feedback a retry is given.

## `pub(super) struct RecordingRunner` › `codes: Mutex<Vec<i32>>,`

Exit codes to hand back, in order. Exhausted entries answer 0.

## `pub(super) struct RecordingRunner` › `log: Mutex<Option<PathBuf>>,`

The run's event log, read at the instant of each request.

## `impl RecordingRunner` › `pub(super) fn new() -> Self {`

A runner every process succeeds under.

## `impl RecordingRunner` › `pub(super) fn set_codes(&self, codes: Vec<i32>) {`

Replace the queued exit codes on a runner already in a `Run`.

`failing_with` builds one; a test that needs the fixture's whole run and
only wants different codes cannot rebuild it, because the `Run` owns the
runner and its harness.

## `impl RecordingRunner` › `pub(super) fn failing_with(codes: Vec<i32>) -> Self {`

Hand back these exit codes, in order, then zeroes.

## `impl RecordingRunner` › `pub(super) fn watching(&self, log: &Path) {`

Read `log` at the instant of every request, so an ordering clause about
"before any spawn" has something to be true *at*.

## `impl RecordingRunner` › `fn durable_now(&self) -> Vec<String> {`

The kinds the log on disk holds right now.

## `impl RecordingRunner` › `pub(super) fn ran(&self) -> Vec<Ran> {`

Everything it was asked to run, in order.

## `fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {` › `let durable_at_spawn = self.durable_now();`

Read before the request is recorded, so what it captures is the log
as it stood when the process was asked for.

## `fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {` › `stdout: if code == 0 {`

A refused process says something, the way a real one does: §11.1
makes the tail the feedback a retry is given, and a fixture whose
processes print nothing cannot tell a carried tail from a dropped
one.

## `pub(super) struct AnsweringAdapter {`

---------------------------------------------------------------------------
The agent boundary, doubled
---------------------------------------------------------------------------

## `pub(super) struct AnsweringAdapter {`

An agent CLI that answers, without an agent CLI.

**A double, not a re-implementation.** It implements the real
[`AgentAdapter`] trait, builds a real [`CommandSpec`], and every invocation
it produces is spawned through [`RecordingRunner`] — so what the tests
observe is the engine's own request, at the engine's own boundary. Nothing
here re-implements what an adapter *does*; it stands in for what an adapter
*talks to*.

It exists because the scaffold used to name real adapter ids —
`claude-code` and `copilot` — which `BuiltinAdapters` resolves, sending
`run_review` off to locate an actual CLI that is not there. A fixture that
points at a real boundary and hopes is the same defect as a fixture that
invents a shape production never builds, arriving from the other side.

`review.rs`'s three private fakes were considered and are the wrong shape:
`NeverInvokedAdapter` panics on every method by design, and the other two
model an outage and a deadline. All three are **negative-case** doubles, and
teaching one to answer would change what it means for the tests that own it.

## `pub(super) struct AnsweringAdapter` › `status: crate::ir::OutcomeStatus,`

What this agent reports about its own run.

A field rather than a constant because an **outage** is a distinct path
through the ladder and needs a fixture that reaches it: `RateLimited` is
what `AttemptFailure::is_outage` recognises, and it is the difference
between an attempt that spends one of its rung's allowances and one that
defers spending none.

## `impl AnsweringAdapter` › `pub(super) const fn erroring(id: &'static str) -> Self {`

A reviewer that passes.
An agent whose CLI reports its own error.

`FailureKind::AgentError` is neither an outage nor a question, so
`next_step` retries on the same rung while the allowance lasts — and
`resume: true` when the agent can resume and returned a session, which
is what makes the generation `Retained` rather than closed.

## `impl AnsweringAdapter` › `pub(super) const fn asking(id: &'static str) -> Self {`

An agent that stops and asks rather than working.

`evaluate_outcome` reads `UPSTROKE-QUESTION:` out of the outcome's
detail **before** the evidence rules, because "an agent that stopped to
ask has not failed at anything". That is `FailureKind::NeedsHuman`,
which `next_step` sends straight to a park.

## `impl AnsweringAdapter` › `pub(super) const fn rate_limited(id: &'static str) -> Self {`

An agent whose CLI reports it is rate-limited: `evaluate_outcome` maps
that to `FailureKind::RateLimited`, which `is_outage` recognises and
`next_step` defers rather than blames on the implementer.

## `fn build(&self, run: &crate::agent::TaskRun) -> Result<CommandSpec, UpstrokeError> {` › `let spec = CommandSpec::new(self.id)`

A real spec, carrying the prompt, so a test that reads the recorded
command sees what was actually asked for.

**And the session, when there is one to resume.** Every real adapter
puts it in argv; one that dropped it here would make a retry that
lost its session indistinguishable from one that kept it, which is a
fixture blind spot rather than a simplification — measured, by a
mutation that survived until this line existed.

## `impl crate::agent::AgentAdapter for AnsweringAdapter` › `Ok(crate::ir::Outcome {`

`detail` is where `run_review` reads the verdict from, and `cost_usd`
is what `ReviewRecord` requires — both come from the adapter because
both are things only the agent's own CLI knows.

## `pub(super) struct ScaffoldAdapters {`

The scaffold's adapters: the two the fixture's plans name, and nothing else.

Deliberately not `BuiltinAdapters`. An unknown agent must be a refusal a
test can see, not a silent fall-through to a real CLI.

## `impl ScaffoldAdapters` › `pub(super) const fn erroring() -> Self {`

The same two agents, with the implementer reporting its own error.

## `impl ScaffoldAdapters` › `pub(super) const fn asking() -> Self {`

The same two agents, with the implementer stopping to ask.

## `impl ScaffoldAdapters` › `pub(super) const fn rate_limiting() -> Self {`

The same two agents, with the implementer reporting a rate limit.

## `pub(super) struct Run {`

---------------------------------------------------------------------------
The run
---------------------------------------------------------------------------

## `pub(super) struct Run {`

A real repository, a real event log, a fold over it, and the five hook
families on one harness.

## `pub(super) struct Run` › `pub(super) fixture: Fixture,`

The repository and its manager.

## `pub(super) struct Run` › `pub(super) paths: crate::rundir::RunPaths,`

The run's directories, for the seams that write under them.

## `pub(super) struct Run` › `pub(super) harness: Arc<Mutex<HookHarness>>,`

The one harness every family records into.

## `pub(super) struct Run` › `pub(super) hooks: Hooks,`

The five families.

## `pub(super) struct Run` › `pub(super) timeline: Timeline,`

Every `(site, phase)` in order, with repeats.

## `pub(super) struct Run` › `pub(super) emitter: FoldedEmitter,`

The log, the fold, and the emit sequence over them.

## `pub(super) struct Run` › `pub(super) runner: RecordingRunner,`

What a spawn would have been.

## `pub(super) struct Run` › `pub(super) invocations: crate::engine::topology::identity::InvocationLedger,`

The R4 ledger this fixture discharges obligation (3) against. In
production the driver owns it; here the fixture is the caller.

## `impl Run` › `pub(super) fn started(tag: &str) -> Self {`

A started schema-4 run over a fresh repository.

## `pub(super) fn started(tag: &str) -> Self` › `let paths =`

`RunPaths`'s own doc: "Callers do this once at run start;
every accessor below assumes it has happened." The scaffold
was handing out paths without it, so a review's transcript
write failed into `unavailable_after_error` and the pass was
recorded as an OUTAGE — which spends no attempt. A fixture
that skips a documented precondition does not fail loudly;
it produces a plausible wrong answer.

## `impl Run` › `pub(super) fn manager(&self) -> &WorkspaceManager {`

The manager.

## `impl Run` › `pub(super) fn base(&self) -> CommitSha {`

The base every dispatch of this fixture is made at.

## `impl Run` › `pub(super) fn predicted(&self, key: TaskKey) -> PathSet {`

The predicted region an ordinary dispatch of `key` takes.

**Read off the fold, not restated.** It answered `RepoWide` for every
task while the fixture's entries freeze `src/{id}/` hints, so every
dispatch this scaffold emitted recorded a region the fold did not
derive — the exact disagreement `check_dispatched` now refuses. A
literal here would be a second derivation of the run's own rule, which
is what let the two drift in the first place.

## `impl Run` › `pub(super) fn observed(&self, site: EffectSiteId, phase: HookPhase) -> bool {`

Whether the harness saw `site` at `phase`.

## `impl Run` › `pub(super) fn order_of(&self, site: EffectSiteId, phase: HookPhase) -> Option<usize> {`

Where `(site, phase)` **first** appears on the timeline, or `None` if
nothing drove it.

This is how an ordering clause over a prefix that runs once is asserted:
every family records onto one timeline, so an append and a
`git worktree add` are two positions in one list.

## `impl Run` › `pub(super) fn must_order_of(&self, site: EffectSiteId, phase: HookPhase) -> usize {`

[`Self::order_of`], or a panic naming what never ran.

## `impl Run` › `pub(super) fn mark(&self) -> usize {`

Everything that has happened so far, as a fence.

A clause about a *second* occurrence — O24's retry runs in a generation
whose dispatch already drove both of its sites — is asserted from a mark
taken before the step, so the positions compared are the step's own.

## `impl Run` › `pub(super) fn order_after(&self, mark: usize, site: EffectSiteId, phase: HookPhase) -> usize {`

The first position at or after `mark` at which `(site, phase)` ran.

## `impl Run` › `pub(super) fn count_after(&self, mark: usize, site: EffectSiteId, phase: HookPhase) -> usize {`

How many times `(site, phase)` ran at or after `mark`.

## `impl Run` › `pub(super) fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {`

Arm `injection` at a phase of a git funnel site.

## `impl Run` › `pub(super) fn arm_point(`

Arm a parent-side sub-effect point on the shared harness, which is where
a point genuinely belongs.

## `impl Run` › `pub(super) fn task_state(&self, key: TaskKey) -> TaskState {`

One task's state.

## `impl Run` › `pub(super) fn adopt(root: PathBuf) -> Self {`

Re-open the run a kill child left behind, and replay its log.

This is `recover.rs`'s shape reduced to what `T-DISPATCH` and
`T-ATTEMPT` need: open the log through `Event.OpenLog` (which truncates
a torn tail), parse the surviving bytes, and replay them. It is
deliberately **not** a call into the recovery order — that order is
another lane's and this fixture may not be a second implementation of
it. What it is is the smallest thing that makes the child's durable log
readable, so an assertion can be about the log rather than about a
message the child never got to send.

## `pub(super) fn adopt(root: PathBuf) -> Self` › `let paths =`

`RunPaths`'s own doc: "Callers do this once at run start;
every accessor below assumes it has happened." The scaffold
was handing out paths without it, so a review's transcript
write failed into `unavailable_after_error` and the pass was
recorded as an OUTAGE — which spends no attempt. A fixture
that skips a documented precondition does not fail loudly;
it produces a plausible wrong answer.

## `impl Run` › `pub(super) fn hand_off(&self, dir: &Path) {`

Tell the parent where this child's repository is.

Written **before** anything is armed, so a child that dies at its first
site still hands over a readable pointer. The parent has no other way to
learn it: the scratch directory is keyed by the child's own process id.

## `impl Run` › `pub(super) fn dispatch(&mut self, key: TaskKey, generation: u32) -> Dispatched {`

An ordinary dispatch of `key` at this fixture's head.

## `impl Run` › `pub(super) fn try_dispatch(`

[`Self::dispatch`], keeping the error.

Obligation (3) is discharged against a ledger of this fixture's own:
`dispatch` emits without holding one, so the failure carries the
obligation out, and something has to be the caller. In production that
is `TopologyRun`, which owns the run's ledger.

## `impl Run` › `pub(super) fn spawn_repair(&mut self, root: TaskKey) -> TaskKey {`

Register a repair of `root`, the way a merge rejection will.

PR9 owns the production producer; what this needs to be is an entry the
**fold** accepts, so that a repair dispatch is checked by the same rules
a real one will be. Everything but the identity is cloned from the root's
own entry — ladder, reviews, allowed agents — because every one of those
is a value `check_spawn` compares against the run header, and inventing
them here would be inventing a way to fail.

## `pub(super) const RETAINED_SESSION: &str = "session-01SCAFFOLD";`

The session a retained settlement holds, so a retry has one to resume.

## `impl Run` › `pub(super) fn binding(&self, key: TaskKey, rung: u32) -> RungBinding {`

The binding rung `rung` of `key`'s frozen ladder gives.

Read out of the registry rather than written here, because
`check_attempt_started` compares `attempt_started`'s binding against
exactly this value (INV-19) and a fixture that spelled it out would be
asserting the fold against a literal instead of against the ladder.

## `impl Run` › `pub(super) fn attempt_plan(&self, key: TaskKey, attempt: u32) -> AttemptPlan {`

One attempt of `key`: a worker, one gate, two reviewers.

Two reviewers rather than one, because
`decisions.workspace_candidates.snapshots` requires "one **fresh**
snapshot per reviewer, never reused across roles or attempts", and a
single reviewer cannot distinguish a fresh snapshot from a reused one.

## `pub(super) fn attempt_plan(&self, key: TaskKey, attempt: u32) -> AttemptPlan {` › `gates: vec![{`

Through the production assembler, not invented here. A fixture
that built its own `(command, timeout)` pair would be a second
derivation of the one thing `ShellGate::command` exists to be —
the `frozen_binding` precedent, where a fixture repeating a
production composition kept a fifth copy of it alive.

## `pub(super) fn attempt_plan(&self, key: TaskKey, attempt: u32) -> AttemptPlan {` › `reviewers: vec![`

Identity and policy, no command: the shared review machinery
builds one per invocation, because a re-ask's prompt is not the
first pass's. A fixture carrying a pre-built command would be a
pass shape production never builds.

## `impl Run` › `pub(super) fn review_inputs(&self) -> super::attempt::ReviewInputs {`

What every review pass of one scaffold attempt reads.

Owned fixture data rather than a plan shape invented here: a review that
could not be produced from these inputs would be a pass shape production
never builds.

## `impl Run` › `pub(super) fn retain(&mut self, key: TaskKey, generation: GenerationId, attempt: u32) {`

Settle the in-flight attempt as `Retained`, so the generation becomes
`RetainedIdle` and a same-session retry is admissible.

`settle.rs` owns this transition in production; what is needed here is
only the *state*, so the event is emitted through the same fold-checked
emitter every other event uses and is refused if it is not a transition
the fold allows.

## `pub(super) fn retain(&mut self, key: TaskKey, generation: GenerationId, attempt: u32) {` › `failure: Some(crate::events::FailureRecord {`

**A retained attempt did not succeed.**
`settle::settle_failed` is the only producer of a
`Retained` settlement and it is reached on the
failure path, so production's record always
carries this. This fixture recorded `failure:
None` with no reviews — a record every other door
in the fold calls *successful* — which is the
shape `check_attempt_finished`'s retained arm now
refuses.

## `const HANDOFF: &str = "fixture-root";`

The file a kill child writes its repository root into.

## `pub(super) fn kill_dir(tag: &str) -> PathBuf {`

A directory this process owns, unique to this call, for one kill test.

## `pub(super) fn kill_child_and_adopt(test: &str, dir: &Path, site: &str) -> Run {`

Run `test` as a child that must die, and adopt the run it left behind.

The `unreachable!` inside the child is what fails the test when an
injection stops killing; this end asserts the other half — that the process
really did not exit successfully. Both are needed: a child that returned
early would satisfy neither, and a child that panicked would satisfy only
this one.

## `pub(super) fn kill_child_environment() -> (PathBuf, String) {`

The directory and site a kill child is given.

## `pub(super) const OUTCOME: RunOutcome = RunOutcome::Complete;`

The run outcome a run-end closure records in these tests.
