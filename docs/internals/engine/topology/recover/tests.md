# `src/engine/topology/recover/tests.rs`

Repository source for these notes: [`src/engine/topology/recover/tests.rs`](../../../../../src/engine/topology/recover/tests.rs).
[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/engine/topology/recover/tests.rs).
The relative link works in a checkout or on GitHub; the GitHub link also works from the published site.

The code is the authority for what it does. The explanatory prose is preserved below.
Each backticked part of a section heading is an exact source excerpt. Search for the final
excerpt within the preceding item when a heading names both an item and a line inside it.

## Module

The recovery order, exercised against real directories, a real event log,
real locks and the fake container runtime.

### No raw effect primitive appears here

`src/engine/topology/**` is a `TOPOLOGY_MODULE`: it may carry no
module-level `allow` of a governed lint, and `std::fs`'s writing half is on
the clippy denylist **in tests too**. So every byte this file puts on disk
goes through the funnel that owns its site — `rundir::create_public_dir`
for a directory, `rundir::stage_/publish_owner_record` and its commit-record
pair for the two private records, and `EventLog` for the log. That is not a
ceremony: a fixture that planted `owner.json` with `fs::write` would be
asserting against a file the production writer never produced.

`rundir::remove_public_husk` is what takes a fixture down. It removes a
directory's children and then the directory, which is exactly a recursive
delete through a site-taking funnel, and it is the only such funnel this
module can reach.

## `const RUN_ID: &str = "01KZTPR7E00000000000000001";`

---------------------------------------------------------------------------
Fixed identities
---------------------------------------------------------------------------

## `const CREATOR_PID: u32 = 4242;`

The pid the creator wrote into its `.creating` marker. Never consulted by
the ownership proof — the marker's pid is not one of the twelve conjuncts —
but a marker is not a marker without one.

## `struct Frozen;`

A clock that does not move, so a durable byte can be asserted against a
literal.

## `fn fixture_root(tag: &str) -> PathBuf {`

---------------------------------------------------------------------------
The fixture
---------------------------------------------------------------------------

## `fn fixture_root(tag: &str) -> PathBuf {`

A unique directory per fixture, in one per-process tree.

## `struct Fixture {`

One repository, one committed schema-4 run, and both private records.

Every knob is a field rather than a constructor argument, because the
refusal tests differ from the healthy case in exactly one of them and a
nine-argument builder call would hide which.

## `struct Fixture` › `base_sha: CommitSha,`

The seed commit every fixture's repository holds — the base a step (g)
worktree is cut at.

## `struct Fixture` › `first_line: Vec<u8>,`

The committed first line, without its newline.

## `struct Damage {`

What a fixture may be built wrong in.

## `struct Damage` › `no_private_half: bool,`

Write no private half at all.

## `struct Damage` › `no_owner_record: bool,`

Write no `owner.json`.

## `struct Damage` › `owner: Option<fn(&mut OwnerRecord)>,`

Rewrite one field of the owner record.

## `struct Damage` › `commit: Option<fn(&mut CommitRecord)>,`

Rewrite one field of the commit record.

## `struct Damage` › `locator: Option<String>,`

Record a private locator of another shape.

## `struct Damage` › `host_runner: bool,`

Record a host runner rather than the container one.

## `struct Damage` › `extra: Vec<TopologyEventBody>,`

Extra events, appended after `run_started` in order.

## `struct Damage` › `open_generation: bool,`

Leave one generation **open with no attempt** — the state a crash
between `task_dispatched` and `attempt_started` leaves, and the only
state recovery step (g) has anything to do in.

## `struct Damage` › `two_tasks: bool,`

Register a **second task**, `beta`, beside `alpha`.

The default plan has one task, so a recovery step that loops over tasks
or generations cannot be told apart from one that handles the first and
stops. Not hypothetical: catalogue entry `PR7-PIPELINE-010` reduced step
(e) to `.take(1)` and the whole suite stayed green. Opt-in rather than
default, so no existing fixture's registry size moves.

## `struct Damage` › `two_tier: bool,`

Freeze a **two-tier** chain instead of the default one-tier one.

The default chain has a single tier, so a task's rung is always 0 and a
driver that read the rung from the fold is indistinguishable from one
that assumed zero. That is not a hypothetical: it is why the `rung` half
of `PR7-FOLD-LADDER-POSITION`'s reader stayed unwitnessed through the
repair filed against it, and why S5 round 2 found it still open.

## `struct Damage` › `deep_ladder: bool,`

Freeze a two-tier chain with **two attempts per rung**.

Neither existing chain can show an *accumulated* brief. `chain()` has one
tier, so its second failure exhausts the ladder and the task parks with
no third dispatch; `escalating_chain()` allows one attempt per rung, so
nothing ever carries two entries onto a rung. §11.4's second half —
"next rung, fresh session, **accumulated feedback summary included**" —
needs a ladder deep enough to hold two failures below the rung that reads
them, and this is that ladder. Additive so no existing fixture's chain
moves.

## `impl Fixture` › `fn manager(&self) -> crate::workspace_manager::WorkspaceManager {`

The manager recovery step (g) rebuilds worktrees through.

Derived from the fixture's own repository and private root rather than
stubbed: (g)'s whole subject is a real `Worktree.Verify` against a real
checkout, and a manager that could not reach one would make every
assertion about the step vacuous.

## `fn build(tag: &str, damage: Damage) -> Self` › `crate::workspace_manager::fixture::git(&repo_root, &["init", "-q", "-b", "main"]);`

A **real** repository, not a `.git` directory made with `mkdir`.
Recovery step (g) rebuilds worktrees through a `WorkspaceManager`,
and `WorkspaceManager::derive` asks Git where the common dir is — so
a fixture whose `.git` is an empty directory cannot express the step
at all, and every assertion about it would be vacuous.

## `fn build(tag: &str, damage: Damage) -> Self` › `for setting in [`

A seed commit, so the repository has a real base a worktree can be
cut at. Step (g) recreates `OpenNoAttempt` worktrees "at their bases",
and a base that names no object makes the step's own funnel fail for
a reason that has nothing to do with what is being tested.

## `fn build(tag: &str, damage: Damage) -> Self` › `let marker = CreatingMarker {`

P1: the `.creating` marker the creator published and never removed,
because this run was interrupted between P5b's commit record and P8's
`RunDir.RemoveMarker`. That is the shape a resume exists for, and it
is what makes recovery step (a1)'s "this run's own stale marker,
**which the owner removes here**" a removal that removes something.
Without it every "no census effect followed this refusal" assertion
below is vacuously true, and the census's own write has nothing to be
the anchor of.

## `fn build(tag: &str, damage: Damage) -> Self` › `let mut warnings = Vec::new();`

The log, through the Event funnel and nothing else.

## `impl Fixture` › `fn worktree_lock_file(&self) -> PathBuf {`

The repository-scoped R25 lock file, whose *existence* is what a
`*_before_any_lock` test asserts about.

## `impl Fixture` › `fn derive(&self, explicit: Option<&Path>) -> Result<RootDerived, UpstrokeError> {`

(a0), with the reader ceiling raised so a schema-4 log is readable at
all. Production's ceiling is 3 and refuses here; see
`RootDerived::derive_with`.

## `fn drop(&mut self)` › `let _ = rundir::remove_public_husk(&self.root, &mut NoHooks);`

`remove_public_husk` removes a directory's children and then the
directory. It is the one recursive delete this module can reach
through a site-taking funnel, and a fixture per test is what
exhausts inodes on the build box when nothing does.

## `struct PlantedHusk {`

---------------------------------------------------------------------------
A husk beside the run
---------------------------------------------------------------------------

## `struct PlantedHusk {`

A husk this repository's next write command may reclaim, planted through the
same funnels a creator would have used.

The prefix is a creator that died after P3b and before P5b: a published
`.creating`, the private half it names, and the reciprocal `owner.json` — the
twelve conjuncts of [`rundir::prove_private_half_ownership`] all satisfied,
so [`crate::rundir::PrivateHalfOwnership::Proven`] and both halves
reclaimable. `committed` publishes `committed.json` as well, which fails
conjunct 12 and turns the same shape into a retention: the control half, so
"the census reclaimed it" is a claim about the proof rather than about the
census deleting whatever it walks over.

## `fn tree_bytes(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {`

Every file under `root`, by relative path, with its bytes.

What a "retained" assertion compares. Byte-identity, not existence: a census
that emptied `owner.json` would leave the directory present and every weaker
assertion green.

## `fn event_kinds(log: &[u8]) -> Vec<String> {`

Every `"event":"<kind>"` in a log, in order.

## `fn plan_with(two_tasks: bool) -> Plan {`

---------------------------------------------------------------------------
The recorded run
---------------------------------------------------------------------------

## `fn escalating_chain() -> ChainSummary {`

A chain with a rung above the first, so an escalation has somewhere to go.

The binding's **model differs per tier**, which is what makes the rung
observable in the dispatched attempt: a driver reading rung 0 for an
escalated task runs it on the cheap model forever, and the only visible
symptom is a task that never gets better.

## `fn escalating_chain() -> ChainSummary` › `BindingSummary {`

**Rung 0 matches the default chain's binding on purpose.** The
`attempt_started` helper seeds that binding, and the fold refuses
an attempt whose binding is not the one the run froze for its rung
— `check_attempt_started`'s `BindingMismatch`, which caught the
first draft of this fixture. Only the rung *above* differs, which
is the rung this test is about.

## `fn deep_chain() -> ChainSummary {`

Two tiers **and** two attempts per rung.

The only chain in this file on which a rung can read more than one earlier
failure. Rung 0 spends two attempts, both fail, and the escalation lands on
rung 1 with two records below it — which is what §11.4's "accumulated
feedback summary" is a claim about. Its bindings match
[`escalating_chain`]'s for the same reason that one's match [`chain`]'s: the
seeded `attempt_started` carries rung 0's binding, and the fold refuses an
attempt whose binding is not the one the run froze for its rung.

## `fn run_started(`

A `run_started` whose two digests authenticate against the frozen plan.

The registry digest is derived the way the fold derives it — from the plan,
this record and the probed agents — rather than written as a literal,
because a literal would be a second authority on the same number and the
fixture would drift from the fold the first time either changed.
`base` is the repository's real seed commit rather than a literal, because
the driver dispatches at it: a recorded base that names no object makes
`git worktree add` fail for a reason that has nothing to do with what is
being tested, and a fixture whose record disagrees with its own repository
is not the shape any real run has.

## `let mut reviews = review_plan();`

`second_opinion` is per task, so a second task needs a second
entry — the registry refuses a record whose review alignment does
not match its plan, which is the check working.

## `fn runtime_holding_the_record() -> FakeRuntime {`

---------------------------------------------------------------------------
Seams
---------------------------------------------------------------------------

## `fn runtime_holding_the_record() -> FakeRuntime {`

A runtime holding this run's recorded image and its credential volume.

## `struct RecordingRunner {`

A `Runner` that answers every request with `exit 0` and records what it saw.

## `struct RecordingRunner` › `failing: Mutex<Option<String>>,`

A program whose invocation fails, so a probe refusal can be constructed.

## `struct RecordingRunner` › `filters: Mutex<bool>,`

Whether the worker also declares a clean/smudge filter.

A `.gitattributes` naming a filter makes the staged bytes and the bytes
a gate would see potentially different, which is what the ladder's third
cheap rung refuses.

## `struct RecordingRunner` › `edits: Mutex<bool>,`

Whether an `Implement` invocation edits the worktree it was given.

Off by default, because most tests here only care that a process ran.
A driver test that means to reach the **candidate sequence** needs a
non-empty diff: the ladder's cheap rungs reject an empty one, which is
what `pr_sequence[8]`'s "empty-diff attempt failures" names.

## `impl RecordingRunner` › `fn filtering() -> Self {`

A worker that leaves a change behind **and** a filter declaration, so
the staged evidence is not the evidence a gate would see.

## `impl RecordingRunner` › `fn editing() -> Self {`

A runner whose worker leaves a change behind, so an attempt can succeed.

## `fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {` › `if code == 0`

The worker's edit, which is the whole difference between an attempt
the cheap rungs reject and one that reaches a candidate.

## `fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {` › `crate::workspace_manager::fixture::write_file(`

Through the fixture funnel every other test in this file uses:
`std::fs::write` is on the effect denylist here, and a test that
reached around it would be the first.

## `struct StubAdapter;`

An adapter that reports itself through one probe process.

## `struct AlwaysCertifies;`

A `RunnerPreflight` that certifies without spawning, for the tests whose
subject is a step other than (c).

## `enum RefShape {`

---------------------------------------------------------------------------
The integration ref namespace, with no Git behind it
---------------------------------------------------------------------------

## `enum RefShape {`

What `assert_publishable` finds at the recorded ref.

The two refusing shapes are the two `WorkspaceManager::assert_publishable`
has: `refuse_symbolic` first, then a walk of the worktree records. Both are
reproduced with the production [`Refusal`] values rather than with invented
messages, so an assertion on the sentence an operator reads is an assertion
on the sentence the real funnel would have produced.

## `enum RefShape` › `Direct,`

A direct ref nothing has checked out. Publishable.

## `enum RefShape` › `Symbolic,`

A symbolic ref. `INV-17` makes every engine ref direct.

## `enum RefShape` › `CheckedOut,`

A direct ref some worktree has checked out.

## `struct RecordingRefs {`

[`IntegrationRefs`] with no repository behind it, which still enters the
`Ref.CreateIntegration` funnel positions.

**It must enter them.** Every ordering claim in this file reads its evidence
out of the shared [`HookHarness`], and a double that performed the effect
without consulting `hooks.phase` would leave the one durable Git effect of
the whole recovery order invisible to it — a site contributing nothing to
the coverage evidence. `hooks` here is the bundle's own
[`crate::workspace_manager::EffectHooks`], so the recording lands in the same
harness the other four families record into and needs no second wiring.

It also snapshots the run's event log at each entry. The position claim this
file has to make about the P7/P8 step is "the ref was created **before any
recovery event was appended**", and the log's bytes at the instant of the
effect are that claim directly, with no ordering index standing in for it.

## `struct RecordingRefs` › `log: PathBuf,`

Where this run's `events.jsonl` is, so an entry can snapshot it.

## `struct RecordingRefs` › `targets_read: Mutex<usize>,`

How many times `direct_target` was asked. The control half of the
unpublishable-ref test: `assert_publishable` runs **first**, so a build
that dropped it would still refuse a symbolic ref — at
`direct_ref_target`'s own `refuse_symbolic` — and a test that asserted
only "it refused" would stay green through the loss.

## `struct RecordingRefs` › `entered: Mutex<Vec<Vec<u8>>>,`

The log's bytes at each entry into `Ref.CreateIntegration`.

## `impl RecordingRefs` › `fn with_log(log: &Path, shape: RefShape, at: Option<String>) -> Self {`

The general constructor, by log path — the kill child has no
[`Fixture`], only the repository the parent named it.

## `impl RecordingRefs` › `fn absent(fixture: &Fixture) -> Self {`

Nothing is there — a run killed between P6 and P8.

## `impl RecordingRefs` › `fn at(fixture: &Fixture, sha: &str) -> Self {`

A direct ref already at `sha`.

## `impl RecordingRefs` › `fn shaped(fixture: &Fixture, shape: RefShape) -> Self {`

Nothing is there, and `assert_publishable` answers `shape`.

## `impl RecordingRefs` › `fn created(&self) -> Vec<(String, String)> {`

Every `(refname, sha)` the funnel actually created.

## `impl RecordingRefs` › `fn target(&self) -> Option<String> {`

The ref's current target.

## `impl RecordingRefs` › `fn targets_read(&self) -> usize {`

How many times the ref's target was read.

## `impl RecordingRefs` › `fn log_bytes_at_entries(&self) -> Vec<Vec<u8>> {`

The log's bytes at each entry into the funnel.

## `impl RecordingRefs` › `fn log_kinds_at_entries(&self) -> Vec<Vec<String>> {`

The event kinds the log held at each entry into the funnel.

Beside [`Self::log_bytes_at_entries`] and asserted first, because the
byte comparison's failure output is two `run_started` lines rendered as
`Vec<u8>` and nobody can read which of them grew. The kinds say it in
one line, and the bytes still catch a difference the kinds cannot see.

## `fn direct_target(&self, refname: &str) -> Result<Option<String>, UpstrokeError> {` › `if self.shape == RefShape::Symbolic {`

`WorkspaceManager::direct_ref_target` opens with `refuse_symbolic`
too, and reproducing that is what makes the symbolic case's
`targets_read` assertion load-bearing rather than decorative: without
it, dropping `assert_publishable` would leave the symbolic arm still
refusing here and nothing would notice which check caught it.

## `impl IntegrationRefs for RecordingRefs` › `crate::workspace_manager::refuse_new(refname, new)?;`

The contract's refusal, before the funnel is entered: the real
primitive refuses a malformed or null value before its funnel runs.

## `impl IntegrationRefs for RecordingRefs` › `return Err(UpstrokeError::Git {`

What `git update-ref --no-deref <ref> <new> ""` answers when
the ref appeared between the read and the write.

## `fn the_recording_refs_refuse_a_null_new_value_as_the_real_primitive_does() {`

The contract [`IntegrationRefs::create_zero_old`] states binds this double
as it binds `WorkspaceManager` (`PR126-REVIEW2-DOUBLES-ACCEPT-NULL-NEW`):
a null value is refused before the funnel is entered, and nothing is stored.

## `fn injected(`

`workspace_manager::apply`, which is private to that module — the same three
answers, so an arming at this site does here what it does at every other Git
funnel.

## `struct ArmedHooks {`

---------------------------------------------------------------------------
A funnel that refuses, inside the whole recovery order
---------------------------------------------------------------------------

## `struct ArmedHooks {`

[`HarnessTopologyHooks`] with its run-directory family replaced by one that
returns [`Injection::Error`] at a nominated `(site, phase, nth)`, and records
into the same [`HookHarness`] the other four families do.

Module-local, because `HookHarness::arm` takes a [`SubEffectPoint`] and
`hook()` answers `Proceed` to `Before`/`After` unconditionally, so a
`RunDir` site's two phases are not armable through it.

## `struct Given<'a> {`

---------------------------------------------------------------------------
Driving one resume
---------------------------------------------------------------------------

## `struct Given<'a> {`

What one resume was given, beyond the fixture.

## `struct Given<'a>` › `refs: RecordingRefs,`

The integration ref namespace the P7/P8 step publishes into.

Owned rather than borrowed, so [`Given::healthy`] can build the
default — a resume killed between P6 and P8 finds **no** ref, which is
the shape every other test in this file already implied and none of them
could say. A test that needs another shape assigns the field.

## `impl<'a> Given<'a>` › `fn healthy(`

The healthy case: the runtime holds the record, the pre-flight
certifies, today's config is the recorded one, and the recorded
integration ref is not there yet.

## `fn resume(`

Run (a0) and then the whole order, recording every site into `harness`.

## `(outcome.map(|(recovered, _handle)| recovered), warnings)`

The handle is dropped here, which releases the run lock and then the
worktree lease — the same thing that happened at the end of the recovery
order before the loop existed to hold them. A test that needs them alive
takes [`resume_holding`].

## `fn resume_holding(`

[`resume`], keeping the [`RunHandle`] the order hands back.

## `fn resume_with(`

[`resume`], with the hook bundle supplied — so a test can arm one.

## `fn any_lock_site_ran(harness: &Arc<Mutex<HookHarness>>) -> Vec<&'static str> {`

Whether any lock site ran — the R17 half of "no hold was taken".

## `fn first_observation(harness: &Arc<Mutex<HookHarness>>, site: EffectSiteId) -> Option<usize> {`

The index of a site's first observation, for an ordering assertion.

## `fn resume_with_explicit_private_root_mismatch_refused_before_any_lock() {`

===========================================================================
(a0) — the read-only refusals, before any lock
===========================================================================

## `fn resume_with_explicit_private_root_mismatch_refused_before_any_lock() {`

An explicit `--private-root` that names another root refuses **before any
lock**, and "before any lock" is asserted as the packet states it: no R17
hold was taken and no R25 lock file was created.

"The command refused" is a weaker claim and would be green for an
implementation that took the worktree lease, created
`upstroke-worktree.lock`, and then noticed. The lock file is the one that
bites: `Lock.AcquireWorktree`'s funnel opens it with `create(true)`, so
merely *reaching* the acquisition leaves a repository-scoped artifact behind
on a command that was supposed to end read-only.

## `fn malformed_recorded_locator_refused_before_any_lock() {`

A recorded locator of any shape other than `<root>/runs/<run_id>` refuses
before any lock, and every shape is refused rather than only the obvious
one.

Three shapes, because each fails a different clause: a missing `runs`
component, a trailing component that is not the run id, and a locator whose
path escapes upwards. The third is the one a "does it end with the run id"
check would accept.

## `fn resume_derives_private_root_from_record_when_default_changed() {`

A resume takes the private root **from the record**, not from today's
default — even when the default root has moved somewhere else entirely.

The fixture's root is a temporary directory that is never
`rundir::default_private_root()`, so a `derive` that consulted the default
would produce a different path and the census below it would scan the wrong
tree. Asserted as an equality against the recorded locator's parent rather
than as "the resume succeeded".

## `fn resume_derives_private_root_from_record_when_default_changed() {` › `let canonical =`

Compared canonical-to-canonical, because `authorized_root` is
deliberately **lexical**: it refuses a locator whose shape is not
`<root>/runs/<run_id>` and resolves nothing, so it hands back the root in
whatever form the record wrote. Canonicalising only the right-hand side
compares two spellings of one directory and fails wherever the temporary
directory sits under a symlink — which is macOS, where `TMPDIR` is under
`/var` and `/var` is a link to `/private/var`. Linux's `/tmp` is real, so
this passed there and failed only in CI's macOS leg.

## `fn resume_refuses_missing_private_half() {`

===========================================================================
(a) — the records, before any private write
===========================================================================

## `fn resume_refuses_missing_private_half() {`

A recorded private half that is not on disk refuses, and **is not
recreated**.

`recovery_order` (a): "a missing schema-4 private half is not recreated —
deferred". So the assertion is two-sided: the command refuses, *and* the
directory the record names is still absent afterwards. A build that
helpfully created it would satisfy "refuses" for one more line and then
authorize deletions against a boundary nobody wrote.

## `fn resume_refuses_missing_or_disagreeing_owner_record() {`

A missing `owner.json`, and a present one disagreeing in any of the four
identity fields, both refuse — and each refusal names the field.

One test over five cases rather than five tests, because the claim is that
the check is a *conjunction*: a build that compared only the run id passes
any single-case test that happens to damage the run id.

## `fn resume_refuses_missing_or_disagreeing_owner_record()` › `let private = fixture.private_root.join("runs").join(RUN_ID);`

Before any private write: the private half still holds exactly the
two records the creator left, and nothing new.

## `fn resume_refuses_commit_record_digest_mismatch() {`

`committed.json`'s `run_started_sha256` must equal the digest of the
committed first line, and a mismatch refuses quoting **both** numbers.

## `fn resume_refuses_owner_record_runner_mismatch() {`

`owner.json.runner` must equal `run_started(4).runner` exactly, and the
refusal names **which field** moved.

INV-23 makes this an (a) refusal rather than a (c) one: "every later
incarnation rebuilds the Runner from `run_started(4).runner` — **verified
equal to `owner.json.runner`** — before its RunnerPreflight". A build that
checked only at the rebuild would already have censused, which is a
fold-derived reclaim decided under a runner identity nobody agreed on.

## `fn resume_refuses_digest_mismatch() {`

===========================================================================
(a1) — the stable-prefix barrier
===========================================================================

## `fn resume_refuses_digest_mismatch() {`

A plan whose digest is not the one the log recorded refuses at the barrier's
**checked replay**, and nothing fold-derived happens.

`refusal_condition`'s first clause is "plan or registry digest mismatch",
and `stable_prefix_barrier` step (5) is where a log is replayed through the
checked fold. So the refusal is the replay's, and the assertion is that it
names `CheckedReplay` — not merely that something went wrong.

## `fn resume_establishes_stable_prefix_barrier_before_any_fold_derived_effect() {`

`Event.OpenLog`, its `SyncPrefix` point and `Event.ProvePrefixStable` all
execute **before** the first fold-derived effect of the census.

The ordering is asserted over the harness's first-observation order, which
is what makes this a claim about the *sequence* rather than about
possession. `RunDir.RemoveMarker` is the census's own write and, **in this
fixture**, the earliest fold-derived effect the order performs — the runs
tree holds this run's directory and nothing else, so no husk reclaim can
precede it. That is a property of the fixture rather than of the order: a
husk sorting before this run's id would put `RunDir.RemovePrivateHusk`
first, and the census walks in ascending run-id order. So the fixture's
emptiness is asserted rather than assumed, and the anchor is the census's
first effect *here*: if the barrier's three sites do not all precede it, the
resume decided something from a prefix it had not proven.

## `fn resume_establishes_stable_prefix_barrier_before_any_fold_derived_effect() {` › `assert_eq!(`

Asserted **before** the resume, because the resume reclaims what it walks:
afterwards a husk that had preceded this run in the walk is gone and the
same assertion passes vacuously.

## `fn resume_refuses_before_any_fold_derived_effect_when_prefix_sync_fails() {`

A `SyncPrefix` that returns `Err` ends the command with **nothing done**.

`stable_prefix_barrier`: "a failed sync … performs none of those effects:
the write command ends … with an infrastructure error naming the run id and
the failed step, no append handle is used, the run is NoRunFinished and
resumable". Three assertions, because "it returned an error" is true of a
build that censused first and refused afterwards.

## `const ALPHA: TaskKey = TaskKey(0);`

---------------------------------------------------------------------------
Later events, for the prefixes a resume has to recover from
---------------------------------------------------------------------------

## `fn dispatched_at(base: &CommitSha) -> TopologyEventBody {`

[`dispatched`], at a base that names a real object.

The constant-SHA version is enough for every test whose subject is the
fold, because the fold does not resolve a base. Step (g) does — it cuts a
worktree at it — so its fixture has to name the repository's own commit.

## `fn for_task(key: TaskKey, prefix: &str, body: TopologyEventBody) -> TopologyEventBody {`

Re-key an event built for `alpha` onto another task.

The seeded-event helpers are all `ALPHA`'s, which was enough while every
fixture had one task. A step that loops needs a second, and re-keying is
cheaper than a second set of builders — and keeps the two tasks' events
identical apart from the key, which is what makes "the step handled both" a
claim about the step rather than about the fixture.

The predicted region moves with the key: two tasks holding the same region
is an overlap the fold refuses, and rightly.

## `fn in_generation(generation: GenerationId, body: TopologyEventBody) -> TopologyEventBody {`

Re-key an event built for generation 0 onto a later generation.

`for_task` moves a seeded event sideways; this moves it forward. A
**closed** generation is what a sessionless retry and every escalation leave
behind — `settle::failed` closes it and the next attempt runs in a fresh one
— so a log with two failures below one rung has two generations in it, and
`attempt_started` on a closed generation is a barrier refusal rather than a
fixture. The worktree path moves with the generation because two live
dispatches may not name the same one.

## `fn attempt_finished(attempt: u32, settlement: AttemptSettlement) -> TopologyEventBody {`

`attempt_finished`, whose record **says the attempt failed** — because every
settlement this helper can build is a failure.

`candidate_prepared` is the sole successful settlement, so an
`attempt_finished` is a retry, an escalation, a park, a deferral, a retained
hold or a terminal failure, and each of those is an attempt that did not
succeed. This built `attempt_record(attempt)` — `failure: None`, no reviews —
so every fixture using it produced a settlement that fails a task while
carrying a ledger line saying the work passed. The fold accepted that until
2026-08-27; now `check_attempt_finished` refuses it, and this helper would
have made ~8 fixtures refuse rather than making them coherent.

Deriving the record from the settlement is the fix, not attaching a failure
at each call site: a fixture that has to remember to make its own event
self-consistent is a fixture that will stop doing so.

## `fn attempt_finished(attempt: u32, settlement: AttemptSettlement) -> TopologyEventBody {` › `if let AttemptSettlement::Retained {`

The settlement's retained session and the record's `session_id` are one
value in production — both come from `assessed.outcome.session_id` — and
the fold refuses a retained settlement whose halves name different
conversations.

## `fn attempt_finished_failing(`

`attempt_finished` carrying the failure — and the feedback — a crash left
durable.

[`attempt_finished`] records `failure: None`, which is the shape of an
attempt nothing judged. A crash-resume claim is about the other shape: the
ladder decided something, and §11.4's feedback is on the record it decided
from. `detail` is what the next attempt is told, and it is the field this
helper exists to put in a log.

## `fn resume_finalizes_halted_then_refuses() {`

===========================================================================
(b) — Complete or Halted
===========================================================================

## `fn resume_finalizes_halted_then_refuses() {`

A Halted run does not continue.

### About the word "finalizes" in this test's name

Step (b) is "terminal finalization **then** refuse continuation", and this
slice implements the refusal only: `RunDir.WriteReport` carries
`fault_row: t_finalize`, which is not one of PR7's eleven rows, so writing a
report here would be an out-of-row effect with no fault coverage in this
slice. The name is the packet's and is kept unchanged so the row and the
test still correspond; what it asserts is the half in range, and it asserts
the other half's **absence** explicitly rather than leaving it unstated —
no `report.json`, and no `RunDir.WriteReport`.

## `fn resume_finalizes_halted_then_refuses()` › `attempt_finished(`

`halts_run: false`: the task ends terminal and the run does
not halt, so the derived outcome is Complete rather than
Halted — which is what makes both arms of (b) constructible
without any integration terminal this slice does not
implement.

## `fn resume_rebuilds_runner_from_record_and_warns_on_config_drift() {`

===========================================================================
(c) — the rebuild and its warnings
===========================================================================

## `fn resume_rebuilds_runner_from_record_and_warns_on_config_drift() {`

A `[runner]` config that differs from the record **warns naming the
difference** and is ignored: the run resumes on its recorded runner.

Both halves asserted. A build that warned and then used today's config
would satisfy the warning half, and `run_resumed(4).runner` would then
differ from `run_started(4).runner` — which the fold refuses, but only if
the record actually reaches it.

## `fn resume_rebuilds_runner_from_record_and_warns_on_config_drift() {` › `let log = String::from_utf8(fixture.log_bytes()).expect("the log is utf-8");`

The record won: `run_resumed` carries the recorded volume, not today's.

## `fn resume_warns_when_reference_moved_and_uses_recorded_image_id() {`

A recorded reference that now names another image warns, and the run keeps
running **from the recorded id**.

INV-23: "a moved reference cannot change what executes". The fake's mutable
tag table is what makes this constructible at all — the reference is moved
to a second image the runtime also holds, so the refusal path (an absent id)
is not what is being exercised.

## `fn resume_refuses_by_inspection_before_any_spawn_when_runtime_image_id_or_volume_absent() {`

An unavailable runtime, a recorded image id the runtime no longer holds, and
an absent credential volume each refuse **before any spawn**.

The predicate is the type, not the prose: `RunnerRebuilt::rebuild` runs
`rebuild_by_inspection`, and `PreflightCertified::certify` is the only thing
that spawns — so a refusal that produced no `RunnerRebuilt` cannot have
spawned. Asserted here through a pre-flight that would *panic* if it were
reached.

## `fn resume_refuses_by_inspection_before_any_spawn_when_runtime_image_id_or_volume_absent() {` › `struct NeverRuns;`

A pre-flight that must never run. `certify` is unreachable if the
inspection refusals really do precede every spawn, and this is what
turns "unreachable" into a failing test rather than a comment.

## `fn resume_refuses_by_inspection_before_any_spawn_when_runtime_image_id_or_volume_absent() {` › `type Damage = fn(&FakeRuntime);`

One way to leave a recorded runner un-re-establishable.

## `fn resume_refuses_by_inspection_before_any_spawn_when_runtime_image_id_or_volume_absent() {` › `runtime.add_image("sha256:absent", None);`

Remove the image itself: moving the tag alone leaves the id
present, and the id is what the rebuild asks about.

## `fn chain_to_census(`

---------------------------------------------------------------------------
Driving only as far as the census
---------------------------------------------------------------------------

## `fn chain_to_census(`

(a0) → (a) → (a1) → (a), stopping at the census, so a test can read what it
found. The full order consumes the witness at (h) and nothing survives it.

## `fn chain_to_census_with(`

[`chain_to_census`], with the hook bundle supplied — so a test can arm one.

## `fn resume_of_nondefault_root_run_reclaims_earlier_incarnation_intents_in_recorded_root() {`

===========================================================================
(a) — the census, in the recorded root
===========================================================================

## `fn resume_of_nondefault_root_run_reclaims_earlier_incarnation_intents_in_recorded_root() {`

A run whose private root is not today's default still has its **earlier
incarnations'** containers reclaimed, and they are reclaimed **in the
recorded root**.

Three assertions, and the third is the one the test is named for: the
census's own report has to name the recorded root. A build that censused
`default_private_root()` would find nothing, reclaim nothing, and return a
perfectly successful report — so "the container was reclaimed" alone is not
enough; the root the census scanned is part of the claim.

## `fn resume_of_nondefault_root_run_reclaims_earlier_incarnation_intents_in_recorded_root() {` › `let invocation = crate::runner::InvocationId::probe(`

An intent this run's *creator* incarnation left behind, in the recorded
root. It is dead by construction: the run lock is exclusive, so only one
incarnation of a run is ever live, and this process is a different one.

## `fn resume_reclaims_a_provable_husk_beside_the_run_and_retains_a_possibly_committed_one() {`

A resume **reclaims** the husks beside the run it is resuming: the private
half first, through the proof-token funnel, then the public directory with
the marker last.

`recovery_order` (a1)'s census is a "run-directory census incl. this run's
own stale marker, which the owner removes here, **and husk reclamation under
the ownership proof**", and INV-15 reclaims pre-run husks "at write-command
start under the worktree lock". A resume is a write command and holds that
lock. A run-directory pass that classified and reported would leave a
provable husk on disk for ever: every later resume would report it again, and
only a fresh `upstroke run` would ever reclaim it.

Three claims, and the third is what makes the first two mean anything:

* the provable husk is gone, both halves, and the report names the arm;
* `RunDir.RemovePrivateHusk` precedes `RunDir.RemovePublicHusk` — reversed, a
  kill between the two leaves a private half no marker names and no later
  census can ever prove;
* the husk carrying `committed.json` is byte-identical afterwards. A census
  that deleted whatever it walked over would pass the first two.

## `fn resume_reclaims_a_provable_husk_beside_the_run_and_retains_a_possibly_committed_one() {` › `assert_eq!(`

And the run being resumed: its own stale marker repaired by its owner, and
nothing else. The husk arms are gated on the run lock, which this process
holds for its own directory.

## `fn resume_completes_past_a_husk_whose_private_half_cannot_be_removed() {`

**A husk this resume cannot reclaim does not fail this resume.**

Before the census was shared, the resume's run-directory half was
`list_husks` + `husk_report` — both infallible — plus one `remove_marker`.
Sharing the reclaiming census moved a command-fatal error path onto the
resume: one dead run whose private half the filesystem will not release
(`EACCES`, `EPERM`, `EBUSY`, or on Windows any still-open handle) made
`upstroke resume <id>` fail for **every** run in the repository, on every
attempt, for a different run's residue. T-RESUME enumerates its refusals and
this is not among them, and `startup_census` and INV-15 answer "cannot be
reclaimed" with *retain and report* everywhere else.

The husk sorts **before** this run's id, which is what makes the second claim
worth making: `run_dir_names` sorts ascending, so a census that stopped at
the failure never reached this run's own directory at all — and recovery step
(a1) gives this run's stale-marker repair to its owner, which is this
process. So the repair was collateral damage of a different run's residue.

## `fn resume_completes_past_a_husk_whose_private_half_cannot_be_removed() {` › `assert!(stuck.public.exists(), "the public half was removed anyway");`

The husk: retained where it was, with the locator the next census needs.

## `fn resume_completes_past_a_husk_whose_private_half_cannot_be_removed() {` › `assert!(`

And this run's own stale marker, which sorts after the failure, was still
repaired by its owner.

## `fn the_resume_census_reports_the_husk_it_could_not_reclaim() {`

The same husk, from the report's side: an entry naming the step that refused
and carrying its error, beside the own run's completed repair.

The sibling above asserts the **tree** and that the command survived; this
asserts that the census *said* what happened rather than merely surviving
it. INV-15's answer is retained **and reported**, and a census that swallowed
the failure into a `Skipped` or an `Ok` with no entry would pass the sibling.

## `fn the_resume_census_reports_the_husk_it_could_not_reclaim()` › `assert_eq!(`

And the entry after it: this run's own repair, performed.

## `fn resume_refused_while_reaper_hold_observed_then_succeeds() {`

===========================================================================
(a) — the surviving reaper hold
===========================================================================

## `fn resume_refused_while_reaper_hold_observed_then_succeeds() {`

A resume refuses while a surviving reaper's shared cleanup hold (R28) is
observed, and succeeds once it is released.

The observation is [`rundir::observe_cleanup_hold`], which is fail-closed:
a `cleanup.lock` it cannot inspect is a hold, because "an observation that
was made to fail is not an observation that found nothing". A directory in
the lock file's place is exactly that state and is constructible on every
platform through the directory funnel, which is why it is what stands in for
a live reaper here — the alternative is `libc::flock`, which is on the
effect denylist and which this module may not reach.

The refusal half is `#[cfg(unix)]` because the hold is: R28 is "a surviving
**Unix** reaper's shared cleanup hold", and `rundir`'s non-Unix `cleanup`
module answers `false` unconditionally. The success half runs everywhere,
and asserts on both platforms that the observation site executed — a Windows
build that skipped the question entirely would pass a test that only
asserted the outcome.

## `fn resume_refused_while_reaper_hold_observed_then_succeeds()` › `let cleanup = fixture.public().join("cleanup.lock");`

Bound inside the `cfg`, because only the `cfg` uses it. Bound
outside, Windows compiles an unused local and CI's `lint (windows)`
leg refuses it under `-D warnings` — which is exactly the gap
recorded as `windows-gate-lint-level-gap`: a local
`--target x86_64-pc-windows-msvc` check accepts code the guest does
not, because only the guest sets the lint level.

## `fn replayed(fixture: &Fixture) -> TopologyFold {`

===========================================================================
(d), (e), (h)
===========================================================================

## `fn replayed(fixture: &Fixture) -> TopologyFold {`

Replay the fixture's log from disk, which is the only way to read state a
resume left behind: `run_resumed` consumes the witness that carried the
live fold.

Replaying rather than keeping the live fold is also the stronger assertion.
INV-02's "live state and replay use one checked transition over the exact
wire event" means a claim made against the replayed fold is a claim about
the bytes, not about a `TopologyFold` this process happens to hold.

## `fn resume_clears_budget_stop_and_wakes_deferred() {`

A resume clears the previous epoch's budget stop and wakes every Deferred
task.

Both halves, and both read off the **replayed** log rather than off the
return value: the epoch-scoped stop is what makes "raise the ceiling and
resume" the answer to a budget stop, and a build that cleared it only in
memory would leave the next process refusing for a stop the log still
carries.

## `fn steps_d_and_e_reach_every_generation_not_the_first() {`

**Steps (d) and (e) handle every entry, not the first one.**

Two catalogue entries survived the whole suite at `6a21be6` for one reason —
no fixture had a second thing for these loops to reach:

- `PR7-PIPELINE-010` reduced step (e) to
  `retained_idle(..).into_iter().take(1)`, closing only the first
  `RetainedIdle` generation. Green.
- `PR7-PIPELINE-008` added `if lease == LineageHeld { continue; }` to step
  (d)'s loop, skipping a whole lease class. Green.

Both loops were already correct. What was missing was a fixture that could
tell a loop from a `.first()`, which is why this is a witness and not a
repair. `Damage::two_tasks` registers `beta` beside `alpha` so there are two
of everything for the steps to walk.

**Live above `max_parallel = 1`, latent at it** — which is exactly the
condition a carried row would have named. It is cheaper to hold it than to
write it down: PR11 inherits a substrate whose recovery loops are witnessed
rather than a note saying they are not.

## `fn steps_d_and_e_reach_every_generation_not_the_first()` › `dispatched(),`

alpha: retained and idle — step (e)'s subject.

## `fn steps_d_and_e_reach_every_generation_not_the_first()` › `for_task(BETA, "beta", dispatched()),`

beta: the same, and the second entry the loop must reach.

## `fn steps_d_and_e_reach_every_generation_not_the_first()` › `let before = replayed(&fixture);`

The premise: two retained generations before the resume. Without this the
assertion below is satisfied by a fixture that only ever had one.

## `fn retry_refused_after_resume() {`

A retained session belongs to the incarnation that retained it. Step (e)
closes the generation, so after the resume there is no retry to evaluate —
and the fold refuses one.

`recovery_order` (i): "`ready_retry` is never evaluated before (h) and the
fold refuses a stale-incarnation retry". The first clause is structural
here: nothing in this file evaluates `ready_retry`, and the loop that does
is behind `run_resumed`, which consumes the witness. The second is asserted
directly, against the replayed fold.

## `fn retry_refused_after_resume()` › `let refused = after`

And the transition itself is refused: a forged retry into the closed
generation does not plan.

## `fn run_resumed_records_identical_runner_identity() {`

`run_resumed(4).runner` equals `run_started(4).runner` field for field.

Read off the log rather than off the value this process passed in, and
compared with `RunnerPolicy::difference` — which names which field moved —
rather than with `assert_eq!`, so the failure message is the field rather
than two pretty-printed records.

## `fn forged_run_resumed_with_different_runner_identity_refused_on_replay() {`

A `run_resumed` whose runner differs from `run_started`'s is refused **on
replay**, not merely at the point it would be written.

The forged line is appended straight through the Event funnel, which is
exactly what a hand-edited log or a hostile process would produce: the fold
never saw it. So the refusal has to come from the reader, and it does — the
barrier's checked replay refuses the whole prefix, which is what stops a
forged identity from authorizing anything.

## `fn forged_run_resumed_with_different_runner_identity_refused_on_replay() {` › `let harness = harness();`

And a resume over that prefix refuses at the barrier, before anything.

## `fn resume_after_append_error_follows_surviving_prefix() {`

An append that returns an error ends the command, and the **next** resume
establishes the barrier over whichever prefix survived and continues from
it.

The injection is at `Synced`, which is the case where the line is on disk
and the process cannot tell whether it is durable. `append_error_protocol`:
"the event is outcome-unknown; `apply_delta` is not run and the in-memory
fold is marked poisoned … the append is never retried … the run is
NoRunFinished and resumable and the next resume follows the fault row of the
surviving prefix (T-APPEND) only after its own barrier".

So: the first resume fails with the line present, and the second resume sees
a prefix ending in `run_resumed` and opens the epoch after it. Two
`run_resumed` lines is the correct convergence for the after-append order,
not a duplicate.

## `fn resume_after_append_error_follows_surviving_prefix()` › `assert!(`

**The append is never retried.** A second attempt through the same handle
would come back as the *poison* error rather than the injected one — the
funnel poisons the handle at the point that failed — so the error the
command ends with is what tells a retry from an end. `INJECTED_PREFIX`
present and `POISONED_PREFIX` absent is that distinction, and it is the
only observable one: a retry cannot succeed through a poisoned handle, so
the line count is the same either way.

## `fn resume_after_append_error_follows_surviving_prefix()` › `assert!(text.contains(RUN_ID), "the report names the run: {text}");`

**The protocol ran, and its report is what the command ends with.**
Everything above this point is true of a build that merely poisoned the
fold and returned the funnel's error, which is why none of it can stand
for `append_error_protocol`. Obligation (5) is the observable one: reopen
through `Event.OpenLog` (torn-tail normalization), establish the
stable-prefix barrier, and end "naming the run id, the event kind, and
whether the proven prefix contains the line".

## `fn resume_after_append_error_follows_surviving_prefix()` › `let second = harness();`

The next resume: a fresh harness, nothing armed, and it follows the
surviving prefix.

## `fn an_append_error_during_recovery_cancels_the_reservation_and_every_running_invocation() {`

An outcome-unknown append during recovery cancels the provisional
reservation and every still-running invocation.

`append_error_protocol` obligations (2) and (3):
[`Reservations::cancel_any`] — `permits`: "cancellation on any pre-append
failure, run end, shutdown, or a poisoned fold" — and
[`InvocationLedger::cancel_all_running`], the ledger half of "in-flight
invocations are cancelled through the Runner".

The recovery order's own ledgers are empty, so on that path both obligations
are satisfied vacuously and no test of `resume` could tell a build that ran
them from one that did not. So this test hands the emitter ledgers that are
**not** empty — one held reservation, one registered running invocation —
which is exactly why they are `EmitContext` fields rather than locals inside
the recovery order. Both ledgers balance afterwards: every entry settled
exactly once, which is the process-end condition R4 states.

## `fn real_preflight<'a>(`

===========================================================================
(c) — the RunnerPreflight probes
===========================================================================

## `fn real_preflight<'a>(`

The real pre-flight, over a runner that answers every process.

## `fn resume_refuses_by_preflight_probe_when_shell_or_cli_fails_before_any_recovery_event() {`

A failing shell, and a failing agent CLI, each refuse **before any recovery
event**.

Two cases and not one, because they are two different processes with two
different accountings: the shell probe is non-slotted and the agent probe
takes a slot pair. A build that refused correctly on one could hold a slot
forever on the other, so both assert the ledgers as well as the refusal.

## `fn resume_refuses_by_preflight_probe_when_shell_or_cli_fails_before_any_recovery_event() {` › `let programs: Vec<String> = runner`

The shell probe fails first, so the agent CLI is never asked. That is
the sequence `runner` states — "probes execute through it
sequentially at pre-flight" — and it is what makes the shell the
cheaper refusal.

## `fn ledgers_empty_after_resume() {`

Every process-local ledger is empty after a resume, and the shell probe took
no slot while the agent probe did.

`crash_reconstruction` requires "provisional reservations, slot table,
invocation ledger, and the coordinator's own lock holds are empty at process
start", and the resume path is what has to leave them that way. The
asymmetry is asserted from the recorded requests rather than from the
ledger's totals, because "one slot was taken" is true of a build that took
it for the wrong process.

## `fn ledgers_empty_after_resume()` › `assert!(crate::engine::topology::identity::Reservations::new().is_empty());`

And the process-local ledgers a fresh coordinator starts with are empty
by construction, which is the other half of the row.

## `struct ProbeContainerRunner<'a> {`

A `Runner` that gives every probe a real container through the container
funnel, and releases it on both paths.

This is the shape `ContainerRunner::run` has — `launch` then `release`,
with the release running whether or not the invocation succeeded — driven
against the fake runtime so a test can read what survived. Built here rather
than reused because `ContainerRunner` owns its runtime by value and hands
back no way to inspect it, and because the four effectful `ContainerRuntime`
methods are on the effect denylist for every module but the funnel — so a
delegating wrapper around the fake is not something this module may write.

## `struct ProbeContainerRunner<'a>` › `failing: String,`

The program whose container exits non-zero.

## `fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {` › `release(`

Released on both paths: R26 is "released on complete …, cancel, or
shutdown" and R19's view is "pruned on complete or cancel".

## `fn resume_preflight_probe_containers_reclaimed_after_refusal() {`

After a pre-flight refusal, the probe containers are reclaimed: no
container, no intent, no Git view survives.

`expected_failures_refusals[2]` ends "…refuses before any recovery event or
work spawn, **the probe containers reclaimed**", and R19/R26 both say
"pruned/released on complete **or cancel**". A refusal is a cancel, so the
namespace has to be empty afterwards — otherwise the next write command's
census finds residue from a command that never started.

## `fn create_ref_entries(harness: &Arc<Mutex<HookHarness>>) -> u32 {`

===========================================================================
T-RUNSTART's P7/P8 repair
===========================================================================

## `fn create_ref_entries(harness: &Arc<Mutex<HookHarness>>) -> u32 {`

The `Ref.CreateIntegration` funnel's `Before` count, which is what "the
funnel was entered" means everywhere below.

Counted rather than tested for presence: "no spend repeats" is a claim about
*how many times* the effect ran, and `touched` would be green for a build
that created the ref, then created it again.

## `fn kill_after_run_started_creates_integration_ref() {`

`transaction_fault_matrix[T-RUNSTART].resume_action`, first clause:
"**P7/P8: create the ref zero-old at the recorded base if absent**".

The fixture *is* the prefix a kill between P6 and P8 leaves —
`run_started(4)` durable, `committed.json` naming its digest, the creator's
`.creating` still on disk because P7 never ran — and the ref namespace is
empty. A resume over it must leave the ref there.

### What this asserts that calling the function could not

This test used to live in `create::tests` and called
[`super::super::create::ensure_integration_ref`] directly with two literals.
That proved the *function* creates a ref, which was never in doubt. Driving
[`run_recovery_order`] proves the three things that actually were:

1. **that the recovery order calls it at all** — its only production caller
   used to be P8, so a run killed between P6 and P8 resumed with no ref and
   nothing to create one;
2. **with the recorded arguments** — asserted against
   `fixture.started.integration_ref` and `fixture.started.base_sha` rather
   than against constants, so a resume that published today's configured ref
   name, or the fold's current head, fails here;
3. **at a point before any recovery event** — the funnel snapshots the log
   on entry, and the bytes it saw are compared against the committed prefix.

## `fn kill_after_run_started_creates_integration_ref()` › `assert_eq!(`

The position claim, read off the effect itself rather than off an index:
when `Ref.CreateIntegration` ran, the log was still exactly the prefix
the creator committed — no `attempt_interrupted`, no `generation_closed`,
no `run_resumed`.

## `fn kill_after_run_started_creates_integration_ref()` › `let after = fixture.log_bytes();`

And the appends did happen — otherwise the assertion above is green for a
resume that never got as far as (d)–(h) at all.

## `fn a_resume_adopts_an_integration_ref_already_at_the_recorded_base() {`

The second clause: "**if present == base continue (no spend repeats)**".

Two ways in, because they fail differently. A resume that *finds* the ref
already at the recorded base is the ordinary case — some other process, or
an earlier resume, got there first. A **second** resume of the same run is
the idempotence case, and it is the one that would catch a step that
remembered nothing and re-pointed the ref every time.

Both assert the funnel's entry count and not the command's exit status: an
implementation that called `create_zero_old` again would get an `Err` back
from Git ("already exists; zero-old refuses"), and a build that swallowed it
would be green on `result.is_ok()` while having repeated the spend.

## `fn a_resume_adopts_an_integration_ref_already_at_the_recorded_base() {` › `{`

(1) Already there when the resume arrives.

## `fn a_resume_adopts_an_integration_ref_already_at_the_recorded_base() {` › `{`

(2) Two resumes of one run: the second adopts what the first created.

## `fn a_resume_refuses_an_integration_ref_at_another_sha_before_touching_anything() {`

A ref at any other SHA refuses — and refuses **before anything the step
would otherwise have done**.

`ensure_integration_ref`'s third disposition. "It refused" is the weak half
of the claim; the load-bearing half is that the refusal costs nothing:
`Ref.CreateIntegration` is never entered, the ref keeps the target it had,
and the log is byte-identical to the prefix the resume started from. A ref
that already names another commit belongs to something else, and a run is
never made room for by moving it.

## `fn a_resume_refuses_a_symbolic_or_checked_out_integration_ref() {`

A symbolic ref, and a checked-out one, refuse at `assert_publishable` —
before the target is ever read.

Two shapes and not one: they are the two arms of
`WorkspaceManager::assert_publishable`, and `refuse_symbolic` is also the
first statement of `direct_ref_target`, so a symbolic ref has two chances to
be caught and a build that lost the first would still pass a test that only
asserted "it refused". The `direct_target` count is what separates them —
neither shape may reach it.

## `fn the_p7_p8_step_runs_after_the_refusals_that_bound_it() {`

The step's three lower bounds, each asserted by the refusal that must
precede it: **(b)**, **(c)** and **(f)** all leave the ref untouched.

The bounds are stated in this module's own comment and this is what makes
them checkable rather than asserted in prose:

* **(b)** a Complete or Halted run does not continue, and publishing a
  finished run's integration ref is continuing it;
* **(c)** the repository is touched only once the recorded Runner has been
  rebuilt and its probes have answered, so a resume that cannot run leaves
  the object store as it found it;
* **(f)** an unresolved promotion — and, by the same clause, an unresolved
  integration transaction — is a prefix whose integration ref may be
  mid-move, and "present == base continue" would adopt one under a
  transaction this build cannot resolve. This case is also the first
  coverage [`refuse_unimplemented_terminals`] has had.

The fourth bound, "**before (d)**", is not here: it is asserted positively by
[`kill_after_run_started_creates_integration_ref`], which reads the log at
the instant the funnel ran.

## `fn the_p7_p8_step_runs_after_the_refusals_that_bound_it()` › `{`

(b): a Halted run.

## `fn the_p7_p8_step_runs_after_the_refusals_that_bound_it()` › `{`

(c): a shell probe that does not answer.

## (end of `fn the_p7_p8_step_runs_after_the_refusals_that_bound_it()`)

**(f)'s pin-absent refusal is gone with the convergence it guarded.**
This case drove a `Promoting` generation whose prepared pin had vanished,
and asserted that step (f) refuses it before P7/P8 publishes any ref.
Since the 2026-08-27 CONFORM ruling there is no convergence to guard:
`candidate_prepared` is the sole successful settlement and the only thing
that promotes a generation, so a promoting generation carries its own
candidate identity and a pin is no longer what recovery rebuilds from.

The refusal that still bounds (f) is the integration transaction's, and
`a_resume_refuses_an_integration_ref_at_another_sha_before_touching_anything`
holds that ordering. Removed rather than rewritten around a predicate
that cannot fire — a case asserting a refusal nothing can reach would
pass for the wrong reason.

## `fn recovery_kill_child() {`

===========================================================================
A kill during recovery
===========================================================================

## `fn recovery_kill_child() {`

The child half of [`kill_during_recovery_repeats_recovery`].

`Injection::Kill` is `std::process::abort()` — a real process death, chosen
so the claim is *what a coordinator that runs no cleanup leaves on disk*.
The `unreachable!` at the end is load-bearing: it is what fails the test if
the injection ever silently stops killing.

## `fn recovery_kill_child()` › `let refs = RecordingRefs::with_log(`

The child's ref namespace is process-local and empty, which is what a run
killed before P8 has. The P7/P8 step runs before the first append, so the
child creates it here and then dies at `Event.Append`'s `Written` point —
nothing of it survives, and the parent's assertions are about the disk.

## `fn recovery_kill_child()` › `let manager = crate::workspace_manager::WorkspaceManager::derive(`

Step (g)'s manager, derived from the root (a0) just computed rather than
from an env var the parent would have to pass: the private root is the
one thing (a0) exists to establish, and taking it from anywhere else
would let the child rebuild worktrees under a root the order refused.

## `fn kill_during_recovery_repeats_recovery() {`

A kill at a recovery event's append leaves the run resumable, and the next
process **repeats the whole order from (a0)**.

`recovery_order` (i): "a kill at any point repeats from (a0)". So the
assertion is not only that a second resume succeeds — it is that the second
process re-derived the root, re-took the locks, re-established the barrier
and re-censused, all of which are (a0), (a) and (a1) running again over a
prefix a dead process left. A build that resumed from a checkpoint would
skip them and still finish.

The child is spawned **through the host Runner**, not through
`std::process::Command`: `std::process::Command` is on the effect denylist
and `src/engine/topology/**` may not reach it even in tests. The Runner is
the funnel that owns `Process.Spawn`, which is exactly the rule.

## `fn kill_during_recovery_repeats_recovery()` › `assert!(`

**Died, rather than failed.** `Injection::Kill` is `std::process::abort()`,
which takes the process before the test harness can print anything about
the test — so an aborted child emits no result line at all. A child whose
injection silently stopped killing reaches the `unreachable!`, panics, and
the harness prints both its message and a result line. Asserting only a
non-zero exit cannot tell those apart, because a failed test is also
non-zero; this is what makes the `unreachable!` load-bearing.

## `fn kill_during_recovery_repeats_recovery()` › `let after_kill = fixture.log_bytes();`

What the dead coordinator left: the line it was writing, unsynced, and no
cleanup of any kind.

## `fn kill_during_recovery_repeats_recovery()` › `const AFTER_THE_KILL: &str = "01KZTKILL00000000000000004";`

And the next process repeats the order from (a0).

The census's evidence has to be something the *repeat* can act on. The
dead child had already censused before it reached the append it died at,
so this run's stale marker is gone and stays gone: `RunDir.RemoveMarker`
would be absent from a build that repeated the census perfectly. A husk
planted now is the evidence instead — another crashed run, arriving
between the two processes — and it is the stronger one, because reclaiming
it is a census *effect* rather than a repair that finds nothing to do.

## `fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {`

===========================================================================
The chain's one entry point, as a source census
===========================================================================

## `fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {`

`StablePrefix::into_log_and_fold` is reached from exactly one production region of
the topology engine: [`BarrierHeld::from`].

### Why this is a census and not a visibility

Design v4 §4 makes `BarrierHeld` unforgeable by taking a `StablePrefix` **by
value**, and `StablePrefix`'s only constructor is
`events::log::establish_stable_prefix` — so barrier *evidence* cannot be
manufactured. What it does not close is the other direction:
`StablePrefix::into_log_and_fold` is `pub`, so a topology module could take a
proven prefix apart and hold the append handle and the fold **without**
wrapping them in a `BarrierHeld`, and then everything the chain hangs off —
`ResumeCensused`, and through it every recovery emitter — would be reachable
beside the chain rather than through it.

Narrowing the visibility cannot fix that here. `pub(crate)` does not stop
one topology module reaching another's dependency, and anything tighter than
`pub(in crate::events)` would break `BarrierHeld::from` itself, which *is*
built on `into_parts`. So the claim is the honest one — `BarrierHeld` is the
only route **the topology engine takes** — and this is what makes it a
checkable claim rather than a convention. Same idiom, and same reason, as
`events::log::tests::the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold`.

## `fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {` › `if !relative.starts_with("engine/topology") {`

Only the topology engine is in scope: the funnel that defines
`into_parts` and its own tests are not a second route into the
chain, they are where it lives.

## `fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {` › `if test_modules.contains(&path) {`

A file the crate declares as a whole-file test module is test
code in full and has no production half; counting one would
count a fixture as a second route. **Through the crate's own
declarations**, not through the file name: six of the crate's
whole-file test modules are not called `tests.rs`, and one of
those is `engine/topology/scaffold.rs` — inside this very
census's `engine/topology` domain. `PR7-R5-ATT-001`.

## `fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {` › `let production = crate::effects::production_code(&source);`

The production half only. A test that takes a prefix apart is a
fixture, not a path a run can take.

**`effects::production_code`, not a cut at the first
`#[cfg(test)]`.** The cut was the bug: it fired on the first *raw*
occurrence of the text, comments included, and in
`engine/topology/run.rs` that is **line 83 of 1777** — inside a
doc comment — so this census was scanning 4.7% of the driver, the
single most likely file for a second route to appear in. In
`engine/topology.rs` it was line 39, inside the module doc.

An earlier repair built the needle with `format!` so that a
mention *in this file* could not cut it. That fixed one instance
of `PR4-CENSUS-COMMENT-ORACLE` and left the class open in every
file this walk reads. `production_code` blanks comments and
string literals and removes each `#[cfg(test)]` **item** in place
rather than truncating, which is the repair the four whole-tree
censuses already have.

Found by S5 round 2's `seams` lens, and it lands on this slice's
own evidence: the guard was cited as proving that
`StablePrefix::events` did not become a second entry point, and
that check ran against the truncated domain.

## `fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {` › `regions.push((relative.clone(), production.len(), source.len()));`

Calls, not definitions — a definition is not a route.

The needle used to be the bare `into_parts(`, and at integration
it reported five false routes: three definitions in `startup.rs`
and two calls in `create.rs`, every one of them a typestate
witness of that lane handing back its own fields. The comment
here said the fix was "to rename, not to widen the needle", and
that is what was done: `StablePrefix`'s accessor is
`into_log_and_fold`, a name nothing else in the crate carries,
so the needle now means what it says.
**The control this census did not have.** A zero count from an
empty region is indistinguishable from a zero count from a clean
file, and that is exactly how the truncation hid: the driver's
region was 83 lines of 1777 and its zero looked like a pass. The
four whole-tree censuses each carry this control; this one did
not, which is why the class survived here.

## `fn the_barrier_is_the_only_topology_route_from_a_proven_prefix_to_an_append_handle() {` › `for (file, region, whole) in &regions {`

Every region is a real fraction of its file. A tenth is a generous floor
and still an order of magnitude above what the truncation left behind.

## `fn the_recovery_order_performs_every_step_the_packet_names() {`

---------------------------------------------------------------------------
The order's completeness against the packet's own list
---------------------------------------------------------------------------

## `fn the_recovery_order_performs_every_step_the_packet_names() {`

**The recovery order performs every step `recovery_order` names.**

This is the test that did not exist while step (g) did not exist. For the
whole of PR7's implementation and two review rounds, `run_recovery_order`
performed nine of the ten steps it owns, with all 117 named tests passing,
every gate green on three platforms, and its own doc comment claiming
"steps (a) through (h)". Nothing could see it: a mutation catalogue measures
whether existing code is pinned, and **omission has nothing to mutate**.

So the assertion is against [`RecoveryStep::ALL`], which is the packet's
sentence transcribed into a type, and not against a second list written from
the implementation. Two steps are excluded **by name and with a reason**
rather than by being quietly absent — see [`RecoveryStep::performer`].

## `fn the_recovery_order_performs_every_step_the_packet_names()` › `let mut performed = recovered.steps.clone();`

Completeness first, and it is the half this test exists for: every step
the packet gives this order was performed, exactly once. Sorted, so a
step that moved for a stated reason cannot fail the completeness claim —
that is the next assertion's subject and it is a different question.

## `fn the_recovery_order_performs_every_step_the_packet_names()` › `let packet_order: Vec<RecoveryStep> = owed`

Then the order, for every step whose position the packet alone decides.
`(f)` is excluded **by a named live clause**, not by being skipped: see
`RecoveryStep::position_override`.

## `fn the_recovery_order_performs_every_step_the_packet_names()` › `let at = |step: RecoveryStep| {`

And the one that does move, moved for its reason and not by accident:
**(f) had two halves and now has one.** Its refusing half — the
unresolved integration transaction, one of the two things
`checkpoint_refusals` authorises — still runs before any append. Its
converging half, which appended a rebuilt `candidate_prepared` for a
settled-but-unrecorded candidate, is deleted with erratum E6's window:
since the 2026-08-27 CONFORM ruling `Promoting` is set only in the block
that records the candidate, so a `Promoting` generation without one
cannot occur and the walk could only ever have returned nothing.

What survives at (f) is `finish_promotions`, which still appends —
`T-CAND-REF`'s four-step sequence for candidates that *are* recorded —
so the step keeps its position among the appending steps and is still
what `steps` records here. The ordering this asserts is unchanged; the
reason given for it was not.

## `fn the_transcribed_recovery_steps_are_the_packets_eleven() {`

The transcribed list is the packet's list — eleven steps, these labels, in
this order.

The companion to the test above, and it guards the *other* direction. That
one proves the implementation covers [`RecoveryStep::ALL`]; this one proves
`ALL` is still the packet's sentence, because a variant deleted from `ALL`
would make the first test pass by asking for less.

## `fn resume_recreates_an_open_no_attempt_worktree_at_its_base() {`

---------------------------------------------------------------------------
(g) — recreate `OpenNoAttempt` worktrees at their bases
---------------------------------------------------------------------------

## `fn resume_recreates_an_open_no_attempt_worktree_at_its_base() {`

**The step does work, and the work is a worktree at the recorded base.**

The companion to `the_recovery_order_performs_every_step_the_packet_names`,
and it is the half that test cannot give: over a healthy fixture (g) runs
and finds nothing, so "the step ran" and "the step is a no-op" are the same
observation. This fixture leaves the one state (g) exists for — a generation
dispatched and never attempted, which is what a crash between
`task_dispatched` and `attempt_started` leaves.

## `fn a_repair_generation_cannot_reach_step_g_in_this_slice() {`

A repair generation cannot reach step (g) in this slice, and the reason is
measured rather than asserted.

(g) refuses a generation whose lease is an inherited lineage: `T-DISPATCH`'s
resume action for a repair is to re-run the recorded materialization, whose
source candidate the fold does not retain, and `checkpoint_refusals` gives
repair execution to PR8. That arm is **unreachable here**, and this test
pins both walls that make it so, because "unreachable" written in a comment
is the same sentence as "I did not check".

The wall this test will lose first is the second one: the day a slice admits
repairs, `TaskRegistry::from_plan` starts producing entries with a lineage,
this test fails, and (g)'s arm becomes reachable — which is precisely when
someone should be made to look at it.

## `fn a_repair_generation_cannot_reach_step_g_in_this_slice()` › `let repair = {`

Wall one: the fold refuses an inherited lease on an ordinary task, at the
barrier's checked replay — so the event never becomes fold state at all.

## `fn a_repair_generation_cannot_reach_step_g_in_this_slice()` › `let registry = TaskRegistry::originals_with_agents(`

Wall two: and there is no task it *would* be legal on, because this
slice's registry gives every entry `lineage: None`.

## `fn the_recovery_order_hands_the_run_on_rather_than_dropping_it() {`

**The recovery order hands its state on rather than dropping it.**

This is the assertion that did not exist while `TopologyRun` did not exist.
`run_resumed` consumed the last witness and returned a two-field summary, so
the append handle `(a1)` had just proved, the fold built from exactly those
bytes, and both locks were destroyed at the end of the order. A loop cannot
be written against a function that ends by throwing the run away — so the
missing driver was not only a missing function, it was a missing *value*.

What is asserted is that the three survive and are the *same* three, not
replacements: the log still appends to the proven prefix, the fold is the
one the barrier replayed, and the locks are still held.

## `fn the_recovery_order_hands_the_run_on_rather_than_dropping_it() {` › `let contested = rundir::RunLock::acquire(&rundir::public_dir(&fixture.repo_root, RUN_ID));`

The run lock is still held, which is the property that lets a loop run
at all. Measured by asking for it: a second acquisition must be refused
while the handle is alive.

## `fn the_recovery_order_hands_the_run_on_rather_than_dropping_it() {` › `drop(handle);`

And released when the handle dies, in declaration order.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps() {`

---------------------------------------------------------------------------
The driver, taking over from the order
---------------------------------------------------------------------------

## `fn the_driver_takes_over_from_the_recovery_order_and_steps() {`

**`TopologyRun` drives a resumed run, and `Step` finally has a consumer.**

This test lives here rather than beside `run.rs` because the only thing that
produces a real [`RunHandle`] is a real recovery, and the fixture for that
is this file's. Duplicating it there to keep the test adjacent to its
subject would be a second fixture for one state — the duplication shape this
slice has paid for four times.

What it asserts is the seam that did not exist: the order hands the run on,
the driver takes it, and one iteration of `loop` selects a branch and acts.
Before `RunHandle`, there was no value to hand over; before `run.rs`,
nothing outside `select.rs` so much as matched on a `Step`.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps()` › `let plans = crate::engine::assembly::FrozenPlans {`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps()` › `let Progress::Settled {`

**The driver ran an attempt.** Named exactly, not with a `matches!` that
would pass whichever branch the fixture happened to reach — a fixture
that silently started reaching a different one would take the assertion
with it.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps()` › `assert!(`

**Not accepted, and the reason is the contract's.** This fixture's runner
answers every request with `exit 0` and never touches the worktree, so
the capture's tree is the base's and the diff is empty.
`pr_sequence[8].slice_contract.expected_failures_refusals` names
"empty-diff and unresolved-index attempt failures" as this slice's, and
this is the driver reaching one.

It asserted `accepted` before the ladder's cheap rungs were wired, and
passed: `judge` starts at gates, the plan configures none, and nothing
had asked what the diff contained. A driver that accepted this would have
pinned a candidate whose commit is its own parent.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps()` › `assert_eq!(`

The dispatch AND the attempt are real and durable, in that order. Both
went through the production emitter, which is what makes them subject to
the append-error protocol; the scaffold's emitter re-implements the
append and runs none of it.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps()` › `assert!(`

**The allowance, from `ladder::spends_allowance` and nowhere else.** An
empty diff spends: the line is "the worker ran", not "a verdict was
reached". The settlement carries the answer out of the branch because it
is the input the *next* ladder decision reads.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps()` › `assert!(`

**The worker ran through the Runner**, which is what makes this the
fourth clause rather than a plan that was built and dropped. The whole
point of the driver is that something calls the machinery.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps()` › `let recorded = TopologyFold::parse_log(&fixture.log_bytes())`

**The recorded region is the fold's, not a second derivation.**
`dispatch_lease_check` admits this task by computing the region and
asking the lease table what it overlaps; the log then holds whatever the
dispatch recorded, and the lease table keeps the log's. Two derivations
means the fold admits on one answer and the run is protected by another.

The fixture's hint is a glob (`src/alpha/*.rs`), which is what makes this
assertion able to fail: the fold strips it to the literal prefix
`src/alpha`, and a driver taking hints literally would record a prefix
that overlaps nothing. Measured — that shipped, for one commit.

## `fn the_driver_takes_over_from_the_recovery_order_and_steps()` › `assert_eq!(`

And the provisional reservation did not leak. O24 converts it AT the
append; a refusal after that must not leave an entitlement held, or the
next selection at width 1 sees a full pipeline forever.

## `fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {`

**The driver carries an accepted attempt through the whole candidate
sequence.**

The companion to `the_driver_takes_over_from_the_recovery_order_and_steps`,
which is the rejection case: there the fixture's worker edits nothing and
the ladder's cheap rungs stop it at the empty diff. Here it leaves a change
behind, so nothing rejects and the branch runs the sequence
`side_effect_vs_event_ordering` specifies — commit object, pin, settlement,
`candidate_prepared`, candidates ref, `task_candidate_created`, then the pin
prune and the forced scrub.

Two tests rather than one parameterised over a flag: the two paths append
different events in different orders, and a grid would assert the union of
them.

## `fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {` › `let plans = crate::engine::assembly::FrozenPlans {`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.

## `fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {` › `assert_eq!(`

**The settlement *is* `candidate_prepared`, so there are six events and
not seven.** This comment required an `attempt_finished` between the pin
and `candidate_prepared`, on the deleted settle_succeeded's own note that
`INV-07`
was "about which event records the candidate, not about which event
settles the attempt". That reading is wrong and
`design/26_design_merge_queue_protocol.md` §26 had already
answered it: `attempt_finished` "is not also emitted for that attempt".
Ruled CONFORM 2026-08-27, and the count below is the assertion — a build
that re-introduced the pair puts a seventh kind back here.

## `fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {` › `assert_eq!(`

-----------------------------------------------------------------------
**The same clause over the EFFECTS, not only the events.**

`side_effect_vs_event_ordering`: "commit object (R27) before pin
(IdUnread between); **pin before `candidate_prepared`**; **candidates ref
after `candidate_prepared`** and before `task_candidate_created`". The
event list above cannot see any of that — it holds no refs and no objects.

`candidate::tests::pin_pruned_after_promotion` asserts exactly this, over
`candidate::promote`. The driver assembles the same steps from the three
split halves, and **no ordering assertion reached that composition**:
four `PR7-PIPELINE-*` catalogue mutations that reorder it — the pin moved
after `candidate_prepared`, the candidates ref moved before it, the commit
object moved to just after capture, the pin created before `commit-tree` —
were all green. One rule, two production compositions, one witness.

## `fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {` › `"Event.Append".to_owned(),`

task_dispatched and attempt_started: the branch's own prologue,
which this fixture drives in the same step.

## `fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {` › `"Event.Append".to_owned(),`

**candidate_prepared — one append here, not two.** This list
carried an `attempt_finished(succeeded)` above it; that event is
not emitted for a candidate-producing attempt
(`design/26_design_merge_queue_protocol.md` §26, ruled
CONFORM 2026-08-27), and the fold now refuses it. The count is
part of the ordering claim.

## `fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {` › `"Event.Append".to_owned(),`

task_candidate_created.

## `fn the_driver_carries_an_accepted_attempt_through_the_candidate_sequence() {` › `"Worktree.Remove".to_owned(),`

O31's scrub, which `PR7-PIPELINE-029` moved to immediately after
`candidate_prepared` — three appends too early — and was green.

## `fn a_runs_spend_is_the_same_live_as_on_replay() {`

**A run's spend is the same live as it is on replay.**

The ground-truth invariant, pinned as a property rather than as a count. The
ceiling reads `Spend`; a live process keeps it current as it settles, and
every fresh process rebuilds it with `Spend::replay` from the log. If those
two disagree, a resumed run either refuses work it could afford or buys work
it could not, and neither shows up as a wrong number anywhere — it shows up
as a run that behaves differently after a restart.

**Why this class, not this instance.** Both `attempt_finished` and
`candidate_prepared` carry an `AttemptRecord`, and for a successful attempt
the driver appends both. `Spend::replay` counted each occurrence, so replay
priced every success twice while live priced it once. Asserting a corrected
number would have fixed the instance; asserting **live == replay over the
run's own log** kills the class, including the next event kind that carries
a record.

## `fn a_runs_spend_is_the_same_live_as_on_replay()` › `let plans = crate::engine::assembly::FrozenPlans {`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.

## `fn a_runs_spend_is_the_same_live_as_on_replay()` › `let live = run.spend().run_total();`

What the process believes it has spent, after settling one success.

## `fn a_runs_spend_is_the_same_live_as_on_replay()` › `let events = TopologyFold::parse_log(&fixture.log_bytes()).expect("the log parses");`

What any fresh process would believe, from the same bytes.

## `fn the_driver_settles_an_outage_from_the_folds_deferral_count() {`

**The driver settles an outage from the fold's deferral count.**

The witness that closes the mutation named in `deferrals_recorded`'s own
doc. The fold-level witness
(`fold::tests::a_deferral_count_is_derived_by_replay_and_not_by_a_process_local_tally`)
covers the *accumulation*; this covers the **read**, which is load-bearing
on exactly one branch and so needed a fixture that reaches it.

The chain is the one the ladder specifies: an agent whose CLI reports a rate
limit -> `evaluate_outcome` maps it to `FailureKind::RateLimited` ->
`is_outage` recognises it -> `next_step` defers rather than blaming the
implementer -> `settle_failed` records `Deferred`.

**The prior deferral is what makes the read load-bearing.** The fixture's
log already holds one, so the settlement must record `defers: 2`. Without
it, a driver reading a constant zero would record `1` and be
indistinguishable from a correct one — which is precisely why the mutation
survived before this test existed.

## `fn the_driver_settles_an_outage_from_the_folds_deferral_count() {` › `let fixture = Fixture::build(`

One deferral already in the log, and the resume wakes the task back to
`Pending` so the driver can dispatch it again.

## `fn the_driver_settles_an_outage_from_the_folds_deferral_count() {` › `let plans = crate::engine::assembly::FrozenPlans {`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.

## `fn the_driver_settles_an_outage_from_the_folds_deferral_count() {` › `assert!(`

**An outage spends no allowance.** `next_step` defers precisely so that
"retrying would burn attempts on a run that never got a verdict" does not
happen, and `spends_allowance` prices it the same way.

## `fn the_driver_settles_an_outage_from_the_folds_deferral_count() {` › `let settlements: Vec<u32> = TopologyFold::parse_log(&fixture.log_bytes())`

**The count came from the fold**, not from a tally this process kept.
One deferral was already durable, so the settlement records two.

## `fn the_driver_parks_an_attempt_with_the_question_it_raised() {`

**The driver parks an attempt, and the question it raises is durable.**

The last case of the ready-dispatch branch. An agent that stops and asks has
not failed at anything — `evaluate_outcome` reads `UPSTROKE-QUESTION:` out
of the outcome before the evidence rules, precisely so that an agent is not
punished for the empty diff its own question explains — so the chain is
`NeedsHuman` -> `Next::AskHuman(Clarify)` -> a parking settlement.

**`settle_failed` refuses a park that carries no question**, so reaching a
durable settlement at all is half the assertion. The other half is that the
question is the one the legacy engine would have asked: its context comes
from `coordinator::question_context` and its options from
`coordinator::question_options`, and this test reads both back out of the
log rather than out of the builder.

## `fn the_driver_parks_an_attempt_with_the_question_it_raised()` › `let plans = crate::engine::assembly::FrozenPlans {`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.

## `fn the_driver_parks_an_attempt_with_the_question_it_raised()` › `assert!(`

**A park spends no allowance.** "The code was never judged, so nothing is
spent and nothing escalates" — `next_step`'s own words, and the cell that
was wrong when the settlement derived the allowance from `Next` instead
of from the failure.

## `fn the_driver_parks_an_attempt_with_the_question_it_raised()` › `let parked = TopologyFold::parse_log(&fixture.log_bytes())`

The settlement is durable and carries its question.

## `fn the_driver_parks_an_attempt_with_the_question_it_raised()` › `assert!(`

**The words are the legacy authorities', not the driver's.** The context
quotes the agent as data and names the task; the options are what
`question_options` gives a `Clarify`. A driver that worded its own would
pass every assertion above and fail these.

## `fn the_driver_refuses_a_tree_a_filter_has_transformed() {`

**The driver refuses a tree whose bytes a gate would not see.**

The ladder's third cheap rung, and the one that was owed longest.
`Workspace::review_input_problem_for_tree` refuses staged evidence a
clean/smudge filter has transformed, or a worktree still holding unstaged or
dirty nested state — either makes the reviewed diff describe something other
than what the gates run against.

The worker here leaves a real edit **and** a `.gitattributes` naming a
filter, so the diff is non-empty (the first two rungs pass) and the tree is
still unreviewable. Without this rung the attempt would be accepted and a
candidate pinned from a transformed blob.

## `fn the_driver_refuses_a_tree_a_filter_has_transformed()` › `let plans = crate::engine::assembly::FrozenPlans {`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.

## `fn the_driver_refuses_a_tree_a_filter_has_transformed()` › `let failure = TopologyFold::parse_log(&fixture.log_bytes())`

**The refusal is the policy's own words, attributed to the reviewer.**
`classify::review_input_failure` is the one place that decides what an
unreviewable tree means for the attempt, and the message is
`Workspace`'s, not a driver paraphrase.

## `fn the_retaining_incarnation_retries_in_place() {`

**The retaining incarnation takes its next attempt in place.**

The ready-retry branch, end to end and in two iterations of the loop.

The first settles `Retained`: the agent reports its own error, which is
neither an outage nor a question, so `next_step` retries on the same rung —
and `resume: true`, because pre-flight probed the agent as
`session_resume` and the attempt returned a session. **Both halves are
required**, which is why the caps are given here and were empty everywhere
else: with either missing the generation closes and the task retries from a
fresh one instead.

The second is the retry itself: `{pipeline}` reservation, `Worktree.Verify`
against the retained tree, `attempt_started(retry)` carrying the session,
then the attempt and its settlement.

`Quiescence::HoldsTree` is the reason this needs a real worktree: a retry
verified against the base would pass on a tree that had been reset and would
re-gate an empty one as if it were the retained work.

## `fn the_retaining_incarnation_retries_in_place()` › `const RETRY_POOL: &str = "the-retrying-agents-pool";`

The pool this fixture's agent resolves to. Named rather than empty so
that "took the pool from the authority" and "took nothing" are different
observations.

## `fn the_retaining_incarnation_retries_in_place()` › `let pools = vec![crate::capacity::Pool::discovered(`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.
**A pool the implementer's agent resolves to**, because the two
`attempt_started` appends below are asserted to carry it. With `pools:
&[]` — what this fixture had — `AttemptPlans::pool_for` returns `None`
for every agent, so the retry arm's `pool: None` and its repair are
indistinguishable, and `run.rs` passing a literal `None` left the whole
suite green. Measured, twice: once as `R3-SEAMS-001` and once when round
4 restored the literal.

## `fn the_retaining_incarnation_retries_in_place()` › `let retained = TopologyFold::parse_log(&fixture.log_bytes())`

The generation is retained, not closed: only a retained one is retried in
place, and `settle::retry` refuses any other class by name.

## `fn the_retaining_incarnation_retries_in_place()` › `let starts: Vec<(u32, bool, Option<String>)> = TopologyFold::parse_log(&fixture.log_bytes())`

**Two attempts in one generation, the second resuming the first.** A
driver that opened a fresh generation would append `task_dispatched`
again; a driver that lost the session would append `attempt_started`
with none.

**And the pool each attempt drained**, which is the field `R3-SEAMS-001`
was about and the one no test held. The dispatch arm reads `plan.pool`;
the retry arm appends before its plan exists and takes the same answer
from `AttemptPlans::pool_for` one step earlier. Both are asserted here, in
one run, against the pool the assembler actually resolves — which is the
behavioural witness `79cd9c8` said was unavailable because "no driver
fixture can reach the arm". This fixture reaches it, and reached it then.
`reviews/FINDINGS.md` §19, claims (2) and (3).

## `fn the_retaining_incarnation_retries_in_place()` › `let briefed = runner`

**And told what went wrong.** §11.4 sends the failure back to the same
rung; the plan hard-coded `retry: None`, so the second attempt got the
first attempt's prompt verbatim and no reason to behave differently. A
retry that is not informed is a rung's allowance spent to learn nothing.

## `fn the_retaining_incarnation_retries_in_place()` › `assert!(`

Balance, which says every registration was settled. It does **not** say
the reviewers were registered — an empty ledger balances too — so R4's
review coverage is asserted where reviewers actually run, in
`attempt::tests`.

## `fn the_retaining_incarnation_retries_in_place()` › `let resumed = runner`

**The worker was actually told to resume.** The event records that a
session was retained; this records that the command carried it. They are
different claims, and a retry that appended the first without the second
would re-implement the task from scratch on a worktree that already holds
its previous work.

## `fn a_refused_step_leaves_no_entitlement_held() {`

**A step that refused holds no entitlement afterwards.**

`permits.protocol` is "every Runner process registered exactly once, settled
exactly once", and `append_error_protocol`'s obligation (2) is
`Reservations::cancel_any` on any outcome-unknown path. Three catalogue
entries take an entitlement **before** the step that can refuse and leak it
on the refusing path — `PR7-PIPELINE-014` (the `Dispatch` take moved into the
`Ok` arm), `PR7-SELECT-024` (a `Retry` reservation taken before `select`),
`PR7-SELECT-033` (an `Integration` pair taken before `checkpoint` refuses) —
and all three were green, because nothing asked the ledger anything after a
refusal.

The budget breach is the refusal this drives, because it is the one a fixture
can reach without arming an injection: seed a settled attempt that cost
something, resume, and set a ceiling below it. That the spend is visible at
all is itself new — `Spend::replay` had no production caller until `6d3fc6f`.

## `fn a_refused_step_leaves_no_entitlement_held()` › `let mut run = TopologyRun::resumed(`

A ceiling the log's own spend has already passed.

## `fn a_retried_worker_is_told_what_the_last_attempt_failed_on() {`

**A retried worker is told what the last one failed on.**

§11.4, quoted on `PlanRequest::feedback` itself: "failure feedback goes back
to the same rung, and an escalation carries the accumulated feedback with
it."

The driver accumulated the brief inside [`Retained`], which `settle::retry`
produces **only** for a resumable same-rung retry that returned a session.
Every escalation and every sessionless retry — which is every Copilot
attempt, `DESIGN.md:452` — therefore dispatched with `feedback: Vec::new()`
and handed the next worker attempt 1's prompt verbatim: a rung's allowance
spent to be told nothing. Found by round 2's `contract`, `seams` and
`attempt` lenses independently.

This drives **two** attempts through the real assembler and asserts on the
second worker's own stdin, because the prompt is the only place the claim is
observable — a brief the driver holds and does not send is the defect.

## `fn a_retried_worker_is_told_what_the_last_attempt_failed_on() {` › `run.step(&seams, &mut hooks)`

Attempt one fails and, with one attempt per rung, escalates onto rung 1.

## `fn a_retried_worker_is_told_what_the_last_attempt_failed_on() {` › `run.step(&seams, &mut hooks)`

Attempt two, on the rung above, from a fresh generation.

## `fn a_retried_worker_is_told_what_the_last_attempt_failed_on() {` › `let settled: Vec<serde_json::Value> = TopologyFold::parse_log(&fixture.log_bytes())`

**And the feedback is on the durable record, because this engine has no
other carrier.** The crash-resume witnesses seed a log directly, so they
assert what a resume does with a `detail` that is already there and
cannot tell whether a *live* schema-4 settlement writes one. Nothing did,
until this: `classify::FeedbackCarrier` is a per-caller choice, and
pointing the driver at `LadderEvent` — the legacy answer — left every
test in this file green while making the brief empty again for every run
that actually crashes. Measured 2026-08-26.

## `struct Timeline(Arc<Mutex<Vec<String>>>);`

The order the funnels ran in, for the **driver's** composition of the
candidate sequence.

`candidate.rs` has one of these and it covers `candidate::promote`. The
driver assembles the same four steps from the three split halves
(`create_candidates_ref`, `append_candidate_created`,
`reclaim_after_creation`), and until this existed **no ordering assertion
reached that composition** — the only two `trace.order(` calls in the
topology engine were both in `candidate.rs`. Found by S5 round 2's catalogue:
four `PR7-PIPELINE-*` mutations that reorder the driver's sequence were green,
while `pin_pruned_after_promotion` would have caught every one of them on the
other path.

## `fn push(&self, site: EffectSiteId, phase: HookPhase)` › `if phase != HookPhase::Before {`

`Before` only: one entry per funnel, at the point it begins, which is
what an ordering clause is about.

## `impl Timeline` › `fn order(&self, of_interest: &[EffectSiteId]) -> Vec<String> {`

The recorded sequence with everything not `of_interest` dropped.

Filtered rather than compared whole: a driver step also creates an
execution root, writes an intent and adds a worktree, and an assertion
over the unfiltered list would be an assertion about the fixture.

## `impl crate::workspace_manager::EffectHooks for TracedEffects` › `fn refusal_cause(&self) -> Option<String> {`

Forwarded, so a poison the inner observer found is reported as poison
and not as a fault this double armed.

## `struct TracedHooks {`

[`HarnessTopologyHooks`] with the two families an ordering clause spans
recorded onto one timeline.

## `fn the_driver_escalates_onto_the_rung_above() {`

**The driver writes the rung an escalation climbs ONTO, not the one it leaves.**

The **write** side of `PR7-FOLD-LADDER-POSITION`'s class, and the exact
mirror of the read-side gap closed at `6d3fc6f`. That repair witnessed the
driver *reading* `ladder_position`; nothing witnessed the driver *writing*
the escalation target, and `PR7-R3-ESCALATED-RUNG-WRITER-UNPINNED` is what
got through: replacing `rung: position.0.saturating_add(1)` with
`rung: position.0` left the whole suite green.

The consequence of that mutation is not a wrong number in a record. The fold
assigns `task.rung = *rung` and resets the allowance, so the task escalates
onto the rung it is **leaving**, `ready` selects it again, the binding
resolves, and it loops without bound — never reaching the tier its chain
escalated it to and never exhausting the chain.

**Written as the round trip, which is the class boundary.** Asserting the
recorded number alone would pin the write and leave the same gap one step
over. This drives the escalation, reads the durable settlement, and then
drives the *next* attempt and asserts the model it actually ran at — so the
value the driver wrote and the value it later reads are held by one test.

## `fn the_driver_escalates_onto_the_rung_above()` › `let fixture = Fixture::build(`

Two tiers, one attempt per rung: the first failure exhausts rung 0.

## `fn the_driver_escalates_onto_the_rung_above()` › `run.step(&seams, &mut hooks)`

Attempt one, on rung 0, fails and exhausts the rung's allowance.

## `fn the_driver_escalates_onto_the_rung_above()` › `run.step(&seams, &mut hooks)`

And the read side, in the same test: the next attempt runs at rung 1's
binding. Pinning the written number alone would leave the same gap one
step over, which is how this class keeps recurring.

## `fn the_driver_escalates_onto_the_rung_above()` › `run.step(&seams, &mut hooks)`

**And the third step exhausts the chain, which is where the human is
told a number.** This is the class boundary the frontier review of
`75da796` (finding 3) found unguarded: `park_question` hard-coded
`rungs_spent: 1` and passed *this rung's* attempts as the total, so a
two-rung exhaustion said "1 attempt(s) across 1 rung(s) all failed" when
two attempts across two rungs had. Nothing asserted it — this file's
two-rung test stopped at the escalated model, its count test drove a
single rung, and **no topology test asserted `rung(s)` at all**.

Asserted here rather than in a fixture of its own because the numbers are
only true of a task that has *actually* climbed: a single-rung fixture
reports 1 and 1 whether the code derives them or hard-codes them, which
is exactly how the constant survived.

## `fn the_driver_dispatches_at_the_rung_the_log_records() {`

**The driver dispatches at the rung the log records, not at rung 0.**

The **other** driver-side half of `PR7-FOLD-LADDER-POSITION`, and the one
that stayed open through the repair filed against it.
`the_driver_spends_the_allowance_the_log_records` witnesses the
`attempts_on_rung` half of the same reader; nothing witnessed `rung`, because
that fixture's chain has one tier and a one-tier chain makes rung 0 the only
rung there is. Measured at `cf22a8c`: replacing the driver's
`self.ladder_position(key)?.0` with a literal `0` failed **no** topology
test.

That is occurrence 4 of `reviews/FINDINGS.md` §4's accumulator class, and the
sharpest argument for its re-scoping: the class was filed *from* this
instance, and half of this instance was still open.

The fold half — `fold::tests::a_ladder_position_is_derived_by_replay_and_not_assumed`
— already states the consequence in words: "A driver that assumed rung 0
would dispatch an escalated task on rung 0 forever, never reaching the tier
its chain escalated it to." This asserts it.

## `fn the_driver_dispatches_at_the_rung_the_log_records()` › `let fixture = Fixture::build(`

A two-tier chain, one attempt per rung, and an attempt already escalated
off rung 0 in the durable log. The task is `Pending` on **rung 1**.

## `fn the_driver_dispatches_at_the_rung_the_log_records()` › `let ran_at = TopologyFold::parse_log(&fixture.log_bytes())`

**The model the attempt actually ran at.** The rung selects the binding,
and the two tiers of this chain differ by model — so this is the rung,
observed rather than asserted about itself.

## `fn a_crash_does_not_erase_what_the_last_attempt_was_told_to_fix() {`

**A crash does not erase what the last attempt was told to fix.**

§11.4's first half: "failure feedback (gate log or `required_changes`) goes
back to the *same rung*". The brief that carries it was a process-local
`BTreeMap` the live loop pushed to, and `TopologyRun::resumed` created it
**empty** — so the sequence the 2026-08-26 frontier review of `75da796` set
out in finding 2 held exactly: attempt 1 fails a gate with an 8-KiB
diagnostic tail, `attempt_finished` is durably appended, the conductor
crashes before the next dispatch, and the retry is handed attempt 1's prompt
verbatim. A rung's allowance spent to be told nothing, and the same defect
free to repeat.

The fixture is that crash: one attempt already settled in the durable log,
carrying the tail, and a chain with a second attempt left on the rung. The
process that wrote it is gone — this run is built by `resumed` from the log
alone, which is the only path a real resume has.

**Asserted on the worker's own stdin**, because the prompt is the only place
the claim is observable: a brief the driver rebuilds and does not send is the
same defect one rung further along. And asserted on the tail's exact text
rather than on the prompt's length — a longer prompt is evidence that
*something* was carried, which is what the live-mode witness
`a_retried_worker_is_told_what_the_last_attempt_failed_on` could say and is
not what §11.4 requires.

## `fn a_crash_does_not_erase_what_the_last_attempt_was_told_to_fix() {` › `const TAIL: &str = "error[E0308]: mismatched types\n  --> src/alpha.rs:12:9\n   \`

§11.1's payload: what the gate printed, not the summary of it.

## `fn a_crash_does_not_erase_what_the_last_attempt_was_told_to_fix() {` › `let fixture = Fixture::build(`

One tier, `attempts_per = 2`: the retry the log entitles this run to is on
the **same rung**, which is the half of §11.4 this test is about.

## `fn an_escalation_after_a_crash_carries_the_accumulated_feedback() {`

**An escalation after a crash carries the accumulated feedback.**

§11.4's second half: "`attempts_per` exhausted → next rung, fresh session,
**accumulated feedback summary included**", and its other named source — the
reviewer's `required_changes`, which §11.2 says the retry gets back verbatim.

Two failures are already durable on rung 0 and the ladder has a rung above,
so the only dispatch this run can make is the escalation. The empty brief a
resume used to rebuild sent it none of them: a fresh, stronger worker on the
same task, given attempt 1's prompt and no reason to do anything different.

**What "accumulated" means here is what `feedback_section` actually sends**,
and this asserts that rather than a stronger reading of the sentence. Every
earlier attempt contributes its summary line; only the newest carries its
full detail, because "older ones would bury it, and the newest is the one
still standing in the way" — the production comment on that decision. So the
claim under test is: both summaries reach the rung above, the newest
reviewer's required changes reach it verbatim, and the accumulated section
exists at all — `feedback_section` writes its header **only** when it is
rendering more than one entry for a fresh rung, so that sentence is the
accumulation itself and not a statement about it.

## `fn an_escalation_after_a_crash_carries_the_accumulated_feedback() {` › `in_generation(GenerationId(1), dispatched()),`

A closed generation cannot take another attempt, so the
second failure is in the generation the retry opened —
which is exactly what a sessionless retry leaves in a log.

## `fn a_log_predating_the_detail_field_folds_and_resumes() {`

**A log written before the field existed still folds, and still resumes.**

`FailureRecord::detail` is additive and `SCHEMA_VERSION` does not move, which
is a claim about *older logs* rather than about new ones:
`decisions/2026-08-26-durable-retry-feedback.md` argues that a line without
the key reads back as `None`, folds unchanged, and passes schema 4's strict
door — the door being a witness comparison that reports "any key the input
carried that the record did not claim back", so an added output key is not an
unknown input key.

That argument is worth exactly as much as a log that tests it. This deletes
the key from every `attempt_finished` in a real fixture's bytes — the shape a
binary one commit older wrote — and resumes from the result through the
production parse.

## `fn a_log_predating_the_detail_field_folds_and_resumes()` › `let current = String::from_utf8(fixture.log_bytes()).expect("the log is utf-8");`

The older binary's bytes: the same log with the key it never wrote.

## `fn a_log_predating_the_detail_field_folds_and_resumes()` › `if position == 0 {`

**The first line is passed through byte for byte.** The commit
record pins its sha256, and a re-serialization that only reorders
keys is enough to make recovery refuse for a reason that has
nothing to do with this field.

## `fn a_log_predating_the_detail_field_folds_and_resumes()` › `if let Some(failure) = value.pointer_mut("/data/record/failure") {`

Nested rather than a let-chain: MSRV is 1.85 and let-chains
are 1.88.

## `fn a_log_predating_the_detail_field_folds_and_resumes()` › `let harness = harness();`

And the run it describes still resumes. **What the brief holds for those
attempts is a line per failure carrying its `summary` and no `detail`** —
not an empty brief, which is what this comment used to claim and what the
2026-08-26 re-review of `c2c0294` corrected as finding C. `Brief::record`
adds an entry whenever the record carries a failure at all, and that is
right: a summary is what an older log preserved, and sending the next
worker "attempt 1 failed: gate `x` failed" beats sending it nothing.
Asserted below rather than described, because a comment about content is
the thing this slice keeps getting wrong.

## `fn a_log_predating_the_detail_field_folds_and_resumes()` › `crate::workspace_manager::fixture::write_file(&fixture.log(), aged.as_bytes());`

`std::fs::write` is on the effect denylist here; the fixture writer is
the sanctioned way a test plants bytes.

## `fn a_log_predating_the_detail_field_folds_and_resumes()` › `let brief = crate::engine::topology::run::Brief::replay(&handle.events);`

The brief that resume rebuilds from those bytes, asserted rather than
described: one line for the failure the older log recorded, carrying its
summary and no detail.

## `fn drive_one_attempt(fixture: &Fixture, runner: &RecordingRunner) -> Vec<String> {`

Resume the fixture from its log alone, take one step, and return the
implementer prompts the run actually sent.

The whole apparatus of the driver tests above, factored out because the two
crash-resume witnesses differ only in what their logs already hold and in
what they expect to come back. **Recovery is not stubbed**: this is
`resume_holding` into `TopologyRun::resumed`, the same pair every other
driver test in this file uses, so the brief under test is rebuilt from the
barrier's own parse of the durable bytes.

## `fn the_driver_spends_the_allowance_the_log_records() {`

**The driver spends the allowance the log records, not the one it assumed.**

The driver-level half of `PR7-FOLD-LADDER-POSITION`. The fold half is
`fold::tests::a_ladder_position_is_derived_by_replay_and_not_assumed`; this
is the read, and it needed a fixture that makes the read observable.

This fixture's chain has **one** tier and `attempts_per = 2`, so an
escalation has nowhere to climb and the allowance is the whole ladder. One
attempt is already durable in the log. The driver's next attempt is
therefore the **second** on that rung, which exhausts the allowance, and
`next_step` has no rung to escalate onto — so the task fails terminally.

A driver that assumed `attempts_on_rung: 1` would hand `next_step` the first
attempt of two and get `RetrySameRung` instead: the task would retry
forever, spending a rung's allowance on every restart and never failing.
That is what the constant did before this test existed.

## `fn the_driver_spends_the_allowance_the_log_records()` › `let fixture = Fixture::build(`

One attempt already spent on rung 0, settled as a same-rung retry so the
task returns to `Pending` and this branch selects it again.

## `fn the_driver_spends_the_allowance_the_log_records()` › `let plans = crate::engine::assembly::FrozenPlans {`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.

## `fn the_driver_spends_the_allowance_the_log_records()` › `let SettlementTransition::Parked { question } = transition else {`

**Parked, not failed** — and that is `next_step`'s answer, not a
weakening of the assertion. A spent chain asks a human rather than
failing the task: "Nothing further can move this task ... and the
escalation chain is spent." What matters here is that the allowance was
seen as spent at all.

## `fn the_driver_spends_the_allowance_the_log_records()` › `assert!(`

**And the human is told how many attempts actually ran.** The count in
the question is the task's spend on this rung, not the new generation's
attempt number — a park that said "1 attempt" after two would send an
operator looking for a run that had barely started.

## `fn the_loop_continues_an_attempt_recovery_recreated() {`

**The loop continues an attempt in a generation recovery recreated.**

`T-DISPATCH`'s `resume_action` in its own words: "verify the worktree at the
recorded base ... or remove it with force and recreate it ... **continue
attempt (no spend repeats)**".

Step (g) recreated those worktrees and nothing then started an attempt in
them. `fold::ready` excludes the task — correctly, since a task with an open
generation is not *ready to be dispatched* — and `ready_retry` wants
`RetainedIdle`, so no branch could select it. The run stalled with its only
pipeline entitlement held by a generation nothing could drive, and the loop
fell through to a closure it refuses.

`fold::open_no_attempt` identifies the generation for recovery. Selection
now uses `fold::eligible_continuation` to check whether it may start, and
`resume_open_no_attempt` — which had no production caller — is what reuses
the ground.

## `fn the_loop_continues_an_attempt_recovery_recreated()` › `let fixture = Fixture::build(`

Killed after `task_dispatched`, before `attempt_started`.

## `fn the_loop_continues_an_attempt_recovery_recreated()` › `let plans = crate::engine::assembly::FrozenPlans {`

**Through the production assembler, not a fixture plan shape.** The
condition on this extraction was that the scaffold be re-pointed at the
real one or round-tripped against it; a fixture that hand-built an
`AttemptPlan` here would be exactly the fifth copy the `frozen_binding`
precedent warns about.

## `fn the_loop_continues_an_attempt_recovery_recreated()` › `let after = durable_kinds(&fixture);`

**No spend repeats**, in `T-DISPATCH`'s own words: the generation was
already open, so continuing it appends an attempt and never a second
`task_dispatched`.

## `fn a_reviewer_runs_at_the_review_effort_not_the_implementers() {`

**A reviewer runs at §10's review effort, not the implementer's.**

`ResolvedEffortPolicy` has four axes and `review` is one of them: the tier a
rung binds decides what the *work* costs, and review has its own budget.
`FrozenPlans` passed `request.binding.effort` — the implementer's — while its
own comment said "the reviewer's effort, not the implementer's". A comment
asserting the opposite of its line is worse than none: it answers the
question a reader would otherwise ask.

This fixture's Mid rung is `High` and its review axis is `Medium`, so the two
are distinguishable. A fixture where they matched would assert nothing.

## `fn a_reviewer_runs_at_the_review_effort_not_the_implementers() {` › `use crate::engine::topology::scaffold::REVIEW_AGENT;`

**The reviewer is bound to a different agent than the implementer, and it
has to be.** The comment here used to say the single pool was "named so
that a plan inheriting the implementer's pool and one looking up the
reviewer's own could not both pass" — and the fixture's primary reviewer
is `(claude-code, opus)` while its rung-0 implementer is
`(claude-code, alpha-Mid-model)`. `review::passes_for` rebinds only on
**exact `(agent, model)` equality**, so it does not fire and the pass
keeps agent `claude-code`: the two lookups were the same lookup, both
behaviours passed, and the mutation recorded as killed died because the
pool became *empty* rather than wrong. `reviews/FINDINGS.md` §19,
claim (8).

With `REVIEW_AGENT` on the primary and a pool for each agent, inheriting
the implementer's pool yields `the-implementers-pool` and fails.

Through the scaffold's own constant rather than a literal: it is the
agent that fixture's `alternative` binding already names, so this is the
second agent the run actually probed and not one invented here.

## `fn a_reviewer_runs_at_the_review_effort_not_the_implementers() {` › `assert_eq!(`

The implementer's own pool, so the two values in play are distinguishable
and the assertion below is about which one the reviewer got.

## `fn a_reviewer_runs_at_the_review_effort_not_the_implementers() {` › `assert_eq!(`

**And its own agent's pool**, which is the other cell of
`a_reviewers_profile_is_accounted_for_at_both_callers` whose value the
extraction dropped. That census checks the roll is complete and cannot
check a value — a cell is prose. This is the value.

§11.3/§13: a cross-vendor second opinion draws on a different
subscription than the implementer, so the pool is looked up from the
reviewer's own agent. `coordinator.rs` did it and `assembly.rs` did
not, leaving `profile_for`'s empty string — so the capacity engine
attributed a reviewer's spend to a pool with no name. Sol's
independent `seams` read, round 3.

## `fn the_loop_inherits_the_committed_digest_recovery_verified() {`

**The loop inherits the digest recovery verified.**

`committed.json.run_started_sha256` is what step (a) checks the committed
first line against, and the append-error protocol reads it back: the creator
disposition is a projection of the outcome onto the run's *commitment*
boundary, and a run that cannot say whether it is committed cannot report
one.

Recovery's own emitter passes `Some(...)`. `TopologyRun::resumed` passed
`None` — so over one run, the two emitters disagreed about whether it was
committed, and only the loop's appends lost the answer. Nothing observed it
because nothing compared them.

## `fn the_loop_inherits_the_committed_digest_recovery_verified() {` › `let expected = crate::rundir::run_started_sha256(&fixture.first_line);`

Computed independently from the run's own committed first line, rather
than read back from the record the handle came through: a comparison of
the record with itself would pass however the digest was carried.

## `fn the_loop_inherits_the_committed_digest_recovery_verified() {` › `let run = crate::engine::topology::run::TopologyRun::resumed(`

**And it survives into the loop's own identity**, which is the hop that
matters: `establish_stable_prefix` skips its check entirely when the
digest is `None`, so a loop that carried the handle's value and then
dropped it would reopen after an append error and accept a first line the
commit record does not name. `PR31-CONTRACT-006`.

## `fn a_prepared_pin_without_a_candidate_record_is_orphan_residue() {`

**A prepared pin with no `candidate_prepared` is orphan residue, not a
candidate to reconstruct.**

This replaces three tests —
`a_resume_converges_a_settled_candidate_that_was_never_recorded`,
`a_second_resume_finishes_nothing_and_appends_nothing` and
`a_converged_log_prices_its_attempt_once` — which drove erratum **E6**'s
convergence: a `Promoting` generation with no recorded candidate, whose
`candidate_prepared` a resume rebuilt from whatever the prepared pin pointed
at, deriving tree, message and paths from that commit.

**They were witnesses for a window that no longer exists, and for a path that
was a defect.** `Promoting` was reachable by `attempt_finished{Succeeded}`
alone; since the 2026-08-27 CONFORM ruling `candidate_prepared` is the sole
successful settlement and is the only thing that sets `Promoting`, in the
same block that records the candidate. The `bf927f3` review's first P1 was
exactly this reconstruction: substitute the pin between the settlement and
the append and recovery builds a successful candidate around an object no
gate judged — and the tree check cannot catch it, because recovery itself
recorded that tree.

So the fixture is the same crash and the expectation is the other one: the
attempt was never settled, so it settles **interrupted**, and the pin is
residue that recovery prunes. Not patched to pass — the log this drives is
one the fold accepts, which the old fixture no longer is.

## `fn a_prepared_pin_without_a_candidate_record_is_orphan_residue() {` › `extra: vec![attempt_started(1)],`

An attempt that started and never settled: the whole of what a
crash between the pin and `candidate_prepared` leaves durable.

## `fn a_prepared_pin_without_a_candidate_record_is_orphan_residue() {` › `let prepared: Vec<_> = TopologyFold::parse_log(&fixture.log_bytes())`

**And no `candidate_prepared` was invented.** This is the assertion the
removed trio inverted: they required exactly one to appear.

## `fn seed_candidate_commit(fixture: &Fixture, generation: u32) -> String {`

Put a real candidate commit and its pin in the fixture's repository.

**Both halves are real because the residue is real.** This said the halves
had to be real so that erratum E6's convergence could reconstruct the
candidate's identity from the object the pin points at, and called the state
"what a run killed after its settlement leaves behind". Neither survives the
2026-08-27 CONFORM ruling: the convergence is deleted with the window it
converged, and a run killed *after* its settlement has appended
`candidate_prepared`, so it leaves a recorded candidate rather than a bare
pin.

What this seeds is the crash **before** the settlement — the commit object
and its pin written, R27's order, and the append that would have settled the
attempt never reached. A fixture that seeded only events would leave nothing
on disk for the pruning to be about, so the object and the pin are written
for real.

Returns the commit sha, so a test can assert what became of the object the
pin named.

## `fn seed_candidate_commit(fixture: &Fixture, generation: u32) -> String {` › `write_file(&repo.join("candidate.txt"), b"the worker's edit\n");`

A change on top of the base, committed without moving the branch: the
candidate commit is unreferenced except by its pin, which is R23's shape.

**One path, never `add -A`.** The run's own directory lives under
`.upstroke/` inside this repository, so `add -A` stages it and any
subsequent worktree restore deletes it — measured, as a resume that could
not find its own run.

## `fn seed_candidate_commit(fixture: &Fixture, generation: u32) -> String {` › `git(repo, &["rm", "-q", "-f", "--", "candidate.txt"]);`

Unstage and remove just that path, so the index matches the base again
and the pin is the only thing referencing the commit. Targeted for the
same reason the add was.

`git rm` rather than `std::fs::remove_file`: deletion is a funnel in this
crate and the effect denylist refuses the raw call even in a fixture,
which is the rule working rather than getting in the way.

## `struct FixedIds;`

An [`IdSource`] whose question id is a constant.

A park appends the id it minted, and `rematerialize_question` reads it back
on resume rather than re-deciding it — so a test that asserts on the durable
question needs the id to be the same bytes every run. `RealIds` gives a
ULID, which is right in production and unpinnable here.

## `fn durable_kinds(fixture: &Fixture) -> Vec<String> {`

The kinds in a fixture's durable log, in order.

## `struct RecordingSleeper {`

A sleeper that records rather than sleeps.

## `fn a_call_census_needle_is_not_satisfied_by_a_longer_name_ending_in_it() {`

**A call census's needle is not satisfied by a longer name ending in it.**

The class boundary, not the instance. S5 round 4 found that
`every_packet_named_recovery_action_has_a_production_caller` counted
`refuse_unexpected_refs(` as a call to `expected_refs` — but the interesting
half is that the same needle is built for **every** entry from a name the
packet chose, so any future clause whose function name is a suffix of another
identifier is satisfied by that other identifier's call sites, silently and
in the passing direction.

So this asserts the needle's rule over the four shapes that decide it, and
then over the real file the collision was found in — a unit assertion alone
would pass against a helper that was never wired into the census.

## `fn a_call_census_needle_is_not_satisfied_by_a_longer_name_ending_in_it() {` › `let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));`

And on the file the collision was measured in, through the same region
the census reads — a unit assertion over literals would pass against a
helper nothing was wired into.

**The two counts differ, and the difference is worth keeping.** The module
carries four occurrences of `expected_refs(`; the region the census reads
carries one, because three of the four sit inside `#[cfg(test)]` items
that `production_code` blanks. The one that survives is the **definition
line** of `refuse_unexpected_refs`, which the "calls, not definitions"
filter does not catch: the text before the match is `pub fn refuse_un`,
and that does not end in `fn`.

The three that `production_code` blanks now sit in the sibling file
`mod tests;` declares, so the module is two files and `whole` is read
from both. The comparison is the one it always was: every occurrence the
module carries, against the one the production region keeps.

## `fn every_packet_named_recovery_action_has_a_production_caller() {`

**Every packet-named recovery action and refusal has a production caller.**

The class this slice produced more than any other, and the one census that
closes it. Across three review rounds, ten separate things were found
**built, correct, and never called**: `TopologyRun` itself, `settle_*`, the
candidate sequence, `resume_open_no_attempt`, `Started`, `CandidateJournal`,
`Spend::replay`, `complete_promotion`'s continuation, `prune_orphan_pin` and
`refuse_unexpected_refs`.

Two of those were P0/P1 liveness defects — a converged promotion that stalled
the run forever, and a resumed run that forgot its whole spend. The rest were
coverage gaps that would have become defects the moment a caller appeared.
Each was found separately, by a different reviewer noticing a different
symptom, over four rounds.

**This asserts the property the packet states, rather than waiting for a
reviewer to notice its absence.** A function that implements a
`resume_action` or a `refusal_condition` and has no production caller is not
an implementation of that clause — it is a plan to implement it.

**What this census covers, exactly.** The eleven entries below and nothing
else. Of the ten never-called things listed above, `Spend::replay`,
`TopologyRun`, `Started`, `CandidateJournal`, `settle_*` and
`complete_promotion`'s continuation are **not** among them — this would not
have caught them, and the commit that added it said otherwise. Corrected in
`reviews/FINDINGS.md` §19, claim (7); recorded here because the reader who
needs it is the one adding the twelfth entry.

Four ways this could pass while a clause stayed unperformed, and each is
closed by a named thing rather than by the needle being "obviously right":

* **A mention in a doc comment or a string.** The region is
  `effects::production_code`, which blanks comments and string literals.
* **A `#[cfg(test)]` caller in the same file.** The same region removes each
  configured item in place.
* **A caller in an out-of-line `tests.rs`**, where the attribute is on the
  parent's declaration and there is nothing in the file to blank. Skipped by
  file stem in the walk below. This was live until S5 round 4.
* **A longer identifier ending in the entry's name.** `expected_refs(` was
  satisfied by `refuse_unexpected_refs(`. Closed by [`crate::effects::census_domain::production_calls`],
  whose own witness is
  `a_call_census_needle_is_not_satisfied_by_a_longer_name_ending_in_it`.

The fourth is the one worth stating as a class: the needle is built from a
name **the packet chose**, so it cannot be renamed out of a collision the way
`into_log_and_fold` was.

## `fn every_packet_named_recovery_action_has_a_production_caller() {` › `const CLAUSES: &[(&str, crate::effects::census_domain::Call, &str)] = &[`

(function, how production calls it, the packet clause it performs).

## `fn every_packet_named_recovery_action_has_a_production_caller() {` › `let test_modules = crate::effects::census_domain::whole_file_test_modules(&root, &all, 13);`

**The crate's own declarations, not a file-name rule.** This skipped
by the stem `"tests"`, so it covered only the modules named
`tests.rs`; the crate declares **more** whole-file test modules than
that — `effects::tests::cfg::WHOLE_FILE_TEST_MODULES` lists them all,
against the `tests.rs` entries of it the stem finds —
and the six it missed — `scaffold`, `premove`, `fake`, `fixture`,
`scratch_tree`, `readiness` — are the ones most likely to name what
production names. `PR7-R5-ATT-001`.

## `fn every_packet_named_recovery_action_has_a_production_caller() {` › `if test_modules.contains(&path) {`

**An out-of-line test file is test code in full, and
`production_code` cannot tell.** The `#[cfg(test)]` is on the
*declaration* in the parent, so the file it names carries no
attribute of its own and nothing in it is blanked. Without
this skip a fixture calling a packet-named function satisfies
the clause on production's behalf, which is precisely the
class this census exists to close.

## `fn every_packet_named_recovery_action_has_a_production_caller() {` › `assert!(`

The skip is in force and it removed something. A zero here would mean the
control was silently inert — the same failure as an empty region, one
level up. The floor is the pinned list's length rather than a literal,
which is why it is that list and not `test_modules.len()`: the derivation
is what this floor exists to catch, so a floor read off its own output
would pass on an empty answer.

## `fn every_packet_named_recovery_action_has_a_production_caller() {` › `let defined: usize = sources`

**The named item exists.** The census never checked, so renaming a
clause's definition out of the tree left it green — measured, S5
round 4. Not pinned to exactly one definition, because
`settle_interrupted` legitimately names three items and `form` is
what separates them.
