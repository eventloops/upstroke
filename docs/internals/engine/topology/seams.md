# `src/engine/topology/seams.rs`

Extended notes for [`src/engine/topology/seams.rs`](../../../../src/engine/topology/seams.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The seams `TopologyRun` is written against.

`~/tactus-artifacts/quality/`'s classification of 29 findings from PR3–PR6
is the argument for every trait in this file, and against the ones that are
not here. Nearly three in five of those findings needed **no** refactor:
`testability_defect: none` in 58.6% of cases — the code was already cleanly
unit-testable and nobody wrote the test. The genuine structural class is
`io_coupled + env_coupled + no_seam`, 8 of 29, and it concentrates exactly
where effects, platforms and process lifetime meet.

So this is a short file on purpose. It abstracts the three things a
schema-4 run reads from the world that a test cannot otherwise fix — the
five effect-hook families, the wall clock, and identity generation — and
nothing else. Everything else `TopologyRun` needs is already a seam:
`&dyn Runner`, `&dyn ContainerRuntime`, `&dyn OwnerLiveness`,
`&dyn GitView`, and [`crate::interaction::Sleeper`].

**`Sleeper` is deliberately absent from this file.** It already exists, is
already threaded through `RunHarness`, and is already the seam for the
`defer_wait_elapsed` backoff branch this slice implements. A second sleep
abstraction would be the duplication the rest of this design exists to
avoid.

## `pub trait TopologyHooks {`

---------------------------------------------------------------------------
The hook bundle
---------------------------------------------------------------------------

## `pub trait TopologyHooks {`

The five effect-hook families a schema-4 write command drives, as one value.

There are five, not four: [`EffectHooks`] for the git and worktree funnels,
[`RunDirHooks`] for run-directory publication, [`EventHooks`] for the append
funnel and its stable-prefix barrier, [`ContainerHooks`] for the container
funnel, and [`SpawnHooks`] for the platform containment sub-effects.
`ProcessSite::Spawn` and `ProcessSite::Terminate` are both `T-ATTEMPT` rows
and both `SiteScope::Topology`, so a bundle that omitted `SpawnHooks` would
silently exclude two of this slice's own sites.

### Why accessors rather than a supertrait

The obvious shape is `trait TopologyHooks: EffectHooks + RunDirHooks + …`
and a `&mut dyn TopologyHooks` upcast at each call. Trait upcasting
coercion stabilised in Rust **1.86**; this crate's MSRV is **1.85**, pinned
by CI, and `rustc +1.85.0` rejects the upcast with `E0658`. Accessors are
what MSRV permits, and they are no worse: each returns the exact family its
caller needs and the borrow ends with the call.

The five families do not share a signature, which is the other reason a
supertrait would not have helped. `EventHooks::phase` returns nothing
because `Before`/`After` are reachability rather than injection points for
that group; `SpawnHooks` is keyed by `SubEffectPoint` alone because the
process funnel does not take a site by value; the other three return
`Injection` from an `EffectSiteId`. They are one bundle, not one trait.

## `pub trait TopologyHooks` › `fn effects(&mut self) -> &mut dyn EffectHooks;`

The git, worktree, snapshot, ref and object funnels.

## `pub trait TopologyHooks` › `fn rundir(&mut self) -> &mut dyn RunDirHooks;`

Run-directory publication: the marker, the owner and commit records,
the plan, question payloads, and husk removal.

## `pub trait TopologyHooks` › `fn events(&mut self) -> &mut dyn EventHooks;`

The append funnel, `Event.OpenLog` and its `SyncPrefix` point.

## `pub trait TopologyHooks` › `fn container(&mut self) -> &mut dyn ContainerHooks;`

Container intent, creation, start, view, stop, removal.

## `pub trait TopologyHooks` › `fn spawn(&mut self) -> &mut dyn SpawnHooks;`

The platform containment sub-effects of a spawn.

## `pub struct NoTopologyHooks {`

What production passes: nothing armed, nothing recorded.

Five zero-sized no-op observers held by value, so the bundle costs a virtual
call per effect and no allocation.

## `impl NoTopologyHooks` › `pub fn new() -> Self {`

The production bundle.

## `pub struct HarnessTopologyHooks {`

Every family wired onto one PR3 [`HookHarness`].

This is what a coverage pass and every fault-injection test drive
`TopologyRun` through. One harness behind all five families is the whole
point: `check_bijection` reads a single `HookHarness`, so an observation
that lands anywhere else is an observation the evidence table will not have.

A module-local observer that records into its own `Vec` is still the right
tool for **arming** a `Before` or `After` phase — [`HookHarness::arm`] takes
only a [`crate::topology::effects::SubEffectPoint`], and `hook()` answers
`Proceed` to both phases unconditionally. But the *recording* has to come
here, or the 30-plus sites of this slice that expose no sub-effect point
contribute nothing to coverage.

## `pub struct HarnessTopologyHooks` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl HarnessTopologyHooks` › `pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {`

Wire all five families onto `harness`.

## `impl HarnessTopologyHooks` › `pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {`

The harness all five families record into.

## `impl HarnessTopologyHooks` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl HarnessTopologyHooks` › `pub fn recording_durability(mut self) -> Self {`

Also record every durability primitive the record-writing funnels
perform, which is how a P1/P3b/P5b ordering assertion sees the fsyncs.

Three families, not two. The Event funnel is the one whose contract is
*entirely* durability — the append's write/flush/sync and the barrier's
prefix sync — and it was the family this method did not reach, because
[`crate::events::log::HarnessEventHooks`] answered no durability
question at all until PR7 taught it to.

## `impl HarnessTopologyHooks` › `pub fn with_written_kill_shape(mut self, shape: crate::events::log::WrittenShape) -> Self…`

Ask the Event funnel to leave the **torn** durable shape at a kill
armed on `Written`, rather than the complete-unsynced one.

`SubEffectPoint::Written`'s frozen kill entry tables two durable shapes
under one key, so no arming can choose between them and the bundle has
to. Without this, T-APPEND's (w) row — the torn tail the next open
truncates — is unreachable through the bundle at all. Off by default:
the production shape is one `write_all` of the whole line.

## `impl HarnessTopologyHooks` › `pub fn event_observer(&self) -> &crate::events::log::HarnessEventHooks {`

The Event-family observer, so a barrier assertion can read the
durability ledger and the sync records it collected.

The sites themselves are on the shared [`HookHarness`] and are read
there; these are the answers a `(site, phase)` key cannot carry.

## `pub trait TimeSource {`

---------------------------------------------------------------------------
Time
---------------------------------------------------------------------------

## `pub trait TimeSource {`

Where a durable event's timestamp comes from.

This is the seam that matters, and it is not the sleeper. Every
[`crate::topology::events::TopologyEvent`] carries a `ts`, and
`TopologyEvent::now` reads `std::time::SystemTime::now` through
[`crate::util::rfc3339_utc_now`]. That one read makes **every durable byte
of a run non-reproducible**, and three of this slice's obligations are
byte-exact over exactly those bytes:

* `committed.json`'s `run_started_sha256` is the digest of the exact
  `run_started` line, so P5b cannot be asserted against a fixed value while
  the line carries a live clock;
* the stable-prefix barrier proves "the committed first line unchanged" by
  comparing that digest;
* `T-APPEND`'s torn, unsynced and synced cases assert on byte prefixes of
  the log.

A monotonic instant cannot supply this — the field is a wall-clock RFC 3339
string. So the seam is the string.

## `pub trait TimeSource` › `fn now_rfc3339(&self) -> String;`

The timestamp a durable event records, RFC 3339 in UTC.

## `pub struct SystemClock;`

Production: the system wall clock.

Named `SystemClock` rather than `SystemTime`. The obvious name shadows
[`std::time::SystemTime`] inside the one module whose stated rule is that
nothing here calls `SystemTime::now` — a reader grepping for that call would
find this type instead, and a reviewer checking the rule would have to
resolve the shadow by hand. A rule that is checkable by grep stops being one
the moment its subject is ambiguous.

## `pub trait IdSource {`

---------------------------------------------------------------------------
Identity
---------------------------------------------------------------------------

## `pub trait IdSource {`

The identities a fresh run mints before it takes any lock.

`workspace_candidates.run_creation` puts "generation of the coordinator
incarnation id (per-process ULID) and the run id" among the read-only
pre-lock checks, explicitly noting it has "no effect". Both end up in
durable bytes — the run id in the public directory name and the marker, the
incarnation in the marker, the owner record, `run_started`, and **every
container name and intent path** — so a test that cannot fix them cannot
assert on any of those.

`pid` is here for the same reason and no other:
[`crate::rundir::CreatingMarker`] records it, so a byte assertion over a
published marker needs it fixed.

Nothing further is needed for container-name determinism. A container name
is `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`, and the
hash is a pure sha256 of a rendered [`crate::runner::InvocationId`], which
is itself a pure function of its tuple. Fix the run id and the incarnation
and the whole name is fixed.

## `pub trait IdSource` › `fn run_id(&self) -> String;`

A fresh run id.

## `pub trait IdSource` › `fn incarnation(&self) -> IncarnationId;`

This coordinator process's incarnation.

## `pub trait IdSource` › `fn pid(&self) -> u32;`

This process's id, as the `.creating` marker records it.

## `pub trait IdSource` › `fn question_id(&self) -> QuestionId;`

A fresh question id.

Here rather than called directly so a test can drive a park to a known
id: `interaction::new_question_id` is a ULID, and a settlement that
minted one inline would append a different byte string every run,
which no durable-log assertion can pin.

## `pub struct RealIds;`

Production: ULIDs and the real process id.

## `mod tests` › `pub(crate) struct Fixed {`

A fixed clock and fixed identities, so a durable byte can be asserted
against a literal.

## `mod tests` › `fn the_production_bundle_proceeds_from_every_family() {`

The production bundle answers `Proceed` from every family and records
nothing.

Executed per family rather than asserted once, because the five traits
do not share a signature and a bundle that returned the same observer
five times would pass a single-family check.

## `fn the_production_bundle_proceeds_from_every_family()` › `hooks.events().phase(`

`EventHooks::phase` returns nothing — for that group the two phases
are reachability, not injection points. Calling it is the assertion.

## `mod tests` › `fn every_family_of_the_harness_bundle_records_into_the_same_harness() {`

Every family of the harness bundle records into **one** `HookHarness`.

This is the property `check_bijection` depends on. A bundle whose five
adapters each held their own harness would pass every per-family test
and produce an evidence table with four fifths of it missing.

All **five** families are asserted, not the three whose adapters answer
through `EffectSiteId`. `ProcessSite::Spawn` and `ProcessSite::Terminate`
are `SiteScope::Topology` rows, so a bundle that gave `SpawnHooks` a
private harness would lose both of this slice's process sites while
`check_bijection` still reported success — and a three-family assertion
is exactly what would not notice.

The spawn family is checked in both directions, because its two
directions land in different ledgers. An **unarmed** point is
reachability and `HookHarness::hook` records it into `reached` alone,
so `Exec` — whose only mode is `Kill`, and arming that aborts the
process — is asserted through `reached_point`. `AmbientJobJoined` is the
one containment point with an error contract, so arming it on the
*shared* harness and reading `Injection::Error` back out of the bundle
proves the answering direction as well, and puts a `Process.Spawn`
observation in `coverage`, which is the ledger `check_bijection` reads.

## `mod tests` › `fn a_time_source_produces_the_timestamp_a_durable_event_records() {`

The production clock produces a timestamp the wire format accepts, and
the fixed one produces exactly what it was given.

## `mod tests` › `fn an_id_source_mints_fresh_identities_and_a_fixed_one_does_not() {`

Production identities are fresh per call; fixed ones are not.
