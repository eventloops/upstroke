# `src/events/log.rs`

Extended notes for [`src/events/log.rs`](../../../src/events/log.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The Event funnel: the one place in this crate that writes `events.jsonl`.

`decisions.effect_site_inventory.mechanism` puts this module in the
allowlist's *funnel* section and says what that buys: "`EventLog::open` and
`EventLog::append` are classified as the Event funnel (site-taking) rather
than as wrappers, and the raw writer they wrap is reachable only inside
`src/events/log.rs`". `EffectSiteId::module` for [`FunnelGroup::Event`] names
this file, so the site inventory already pointed here before the code did.

Three things live here that did not live in `src/events.rs`:

1. **Sites.** Every effectful entry takes an [`EventSite`] by value, and the
   funnel calls `hook(Before, site)` → primitive → `hook(After, site)` around
   it, so hooks exist for every site by construction. The two Legacy-scoped
   sites are what schema-1..3 callers pass.
2. **The error contract.** `INV-02`: "an append that was entered and returned
   an error never mutates the live fold, is never retried". This funnel makes
   "never retried" a property of the handle rather than a rule call sites are
   asked to remember — an `Err` after the append was entered poisons the
   handle, and every later append through it fails naming the point that
   poisoned it, until the log is reopened through `Event.OpenLog`.
3. **The stable-prefix helper.** `coordinator_integration.stable_prefix_barrier`
   in one function: open, normalize the torn tail, sync the surviving prefix,
   reread it, prove its bytes *and boundary* unchanged, and hand exactly
   those bytes to the checked replay. It is the only path by which a topology
   write command obtains a fold from an existing log.

### What is *not* here, and why

The Legacy sites carry the pre-move behaviour byte for byte.
`EventSite::LegacyOpenLog.sub_effects()` is `&[]` in the frozen inventory —
no `Create`, no `TruncateTornTail`, no `SyncPrefix` — so the legacy open must
*not* acquire the barrier's extra fsyncs. That is not a shortcut: PR5's
`production_effect` is "the event-log writer keeps its exact write/flush/sync
and torn-tail truncation semantics", and a directory fsync that the pre-move
open never performed is a new way for a legacy open to fail. The frozen enum
and the frozen production-effect sentence agree, and this module follows
both.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_types)]`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
carries this module's review clause -- effects only inside site-taking APIs,
no writable handle returned. `decisions.effect_site_inventory.mechanism` (2).

## `pub trait EventHooks {`

---------------------------------------------------------------------------
The observer
---------------------------------------------------------------------------

## `pub trait EventHooks {`

What the funnel tells whoever is watching, and what it asks them.

The shape is [`crate::agent::proc::SpawnHooks`]'s, for the same reason: a
hook is parent-executed code, production passes an observer that answers
[`Injection::Proceed`] to everything, and the ST-07 subset passes one that
records into PR3's [`HookHarness`] and returns whatever the suite armed.

The site is a parameter rather than a constant because this group has seven
of them and two are Legacy-scoped: an observer that could not tell
`Event.Append` from `Event.LegacyAppend` would let a legacy append report
coverage for a Shared site.

## `pub trait EventHooks` › `fn phase(&mut self, _site: EventSite, _phase: HookPhase) {}`

The funnel is about to run, or has just run, `site`'s primitive.

No injection: `HookPhase::Before` and `HookPhase::After` are
reachability, and [`HookHarness::hook`] answers `Proceed` to both by
construction. They exist so that "hooks exist for every site by
construction" is true of this group too.

## `pub trait EventHooks` › `fn point(`

The funnel reached `point` at the coordinate `mode`'s fault belongs at.

Consulted once per (point, mode) the funnel offers — never once per
point — because the harness is keyed by `(site, point, mode)` and the two
modes of a point do not always fire at the same coordinate.

## `pub trait EventHooks` › `fn written_kill_shape(&mut self, _site: EventSite) -> WrittenShape {`

Which of the two durable shapes T-APPEND tables for a **kill** at
`Written` this observer wants the funnel to leave behind.

`SubEffectPoint::Written`'s frozen doc says its kill entry is "the whole
of what the packet tables for a written append — torn: truncated on the
next open, previous prefix; complete-unsynced: either prefix", and
`WrittenFull`'s says a kill there "leaves the complete-unsynced prefix
Written's kill entry already covers". One key, two durable shapes: the
funnel cannot choose between them and the harness cannot say, so the
observer does. Production never answers anything but the default, and
with the default the line is written by a single `write_all` exactly as
the pre-move writer wrote it.

## `pub trait EventHooks` › `fn durability_ledger(&self) -> DurabilityLedger {`

Where this observer wants the funnel's durability primitives recorded,
**in order and including the ones [`Self::synced`] does not see**.

[`Self::synced`] is keyed by site, point and target — it answers "which
coordinate synced what". It is wired into `sync_log_file` and
`sync_directory`, the two *open-path* helpers, so the append's own
`write_all`, `flush` and `sync_data` emitted no record at all and the
truncation emitted none either. Seven catalogue mutations lived in that
gap: splitting the line's `write_all` in two, deleting the `flush`,
retrying a failed primitive, moving the `Synced` consults to before the
sync, and syncing the *pre*-truncation length at open
(`PR5-EVENTS-011/013/032/035/044/049/051`).

This is the other question — "what did the funnel do, in what order" —
and it is a *handle* rather than a callback so a funnel body can record
into it without a second mutable borrow of the observer. The default
records nothing, which is what production passes.

## `pub trait EventHooks` › `fn synced(&mut self, _record: &SyncRecord) {}`

A sync completed. The record is the funnel's own ledger of what it made
durable and how much of it.

`proof_tests[9]` asks for exactly this: "open syncs the surviving prefix
(**the sync ledger** shows the synced length equal to the file length
after open, incl. a line written unsynced by an earlier handle)". An
fsync is not observable from user space, so the length it reports is
checked against the filesystem's own answer rather than against itself.

## `pub enum WrittenShape {`

Which durable shape a kill at `Written` leaves.

## `pub enum WrittenShape` › `Torn,`

A partial line with no terminating newline: T-APPEND (w), the torn tail
the next open truncates.

## `pub enum WrittenShape` › `Complete,`

The whole newline-terminated line, unsynced: T-APPEND (u), the prefix
the next open's barrier makes durable.

## `pub struct SyncRecord {`

What one sync made durable.

## `pub struct SyncRecord` › `pub site: EventSite,`

The site whose funnel synced.

## `pub struct SyncRecord` › `pub point: SubEffectPoint,`

The point it synced at.

## `pub struct SyncRecord` › `pub target: SyncTarget,`

What was synced.

## `pub struct SyncRecord` › `pub len: u64,`

The log's byte length at the moment of the sync.

## `pub struct SyncRecord` › `pub path: PathBuf,`

The log.

## `pub enum SyncTarget {`

What a sync was applied to.

## `pub enum SyncTarget` › `LogFile,`

The log file itself (`sync_all`: fsync / `FlushFileBuffers`).

## `pub enum SyncTarget` › `LogDirectory,`

The directory holding it, so the name is durable too.

## `pub struct NoEventHooks;`

What production passes: nothing armed, nothing recorded.

## `pub struct HarnessEventHooks {`

The ST-07 observer: records into PR3's harness and returns what was armed.

It answers **all four** of [`EventHooks`]'s questions, and the three beyond
`phase` are not decoration.

[`HookHarness`] is keyed by `(site, phase)`, and two of this funnel's
answers are not expressible in that key at all:

* **Durability.** While this observer overrode `phase` and `point` only, the
  shared bundle could see *that* `Event.OpenLog` ran and nothing whatever
  about what it made durable — for the one funnel whose contract is almost
  entirely durability. Every test of the stable-prefix barrier then needed a
  private observer that touches no `HookHarness`, which is the failure the
  bundle exists to prevent (`HarnessTopologyHooks`: "an observation that
  lands anywhere else is an observation the evidence table will not have").
  [`Self::ledger`] is the ordered trace of primitives and [`Self::syncs`] is
  the richer per-sync record `proof_tests[9]` reads ("the **sync ledger**
  shows the synced length equal to the file length after open").
* **Which durable shape a kill at `Written` leaves.** `SubEffectPoint::
  Written`'s frozen doc tables *two* — torn, and complete-unsynced — under
  one key, so the harness cannot say which and the funnel cannot choose.
  [`WrittenShape::Torn`] is reachable through
  [`EventHooks::written_kill_shape`] and through nothing else, so a bundle
  that did not override it could not produce T-APPEND's (w) row at all.
  [`Self::with_written_kill_shape`] is how a suite asks for it, and the
  default stays [`WrittenShape::Complete`] — the shape production writes,
  in a single `write_all`.

Cloning shares both logs, for the reason [`DurabilityLedger`] gives: a
bundle hands its clone into a funnel body and the test still reads what the
body recorded.

## `impl HarnessEventHooks` › `pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {`

Observe through `harness`.

Durability recording starts **off** and the written shape starts at the
production one, exactly as [`crate::rundir::HarnessHooks::new`] and
[`crate::workspace_manager::HarnessEffects::new`] start their ledgers
off: a ledger costs an allocation per primitive, a torn write costs a
second `write_all`, and only a test that asked for either wants it. Both
are opt-in, so `production_effect: none` is a property of the
constructor rather than of every call site.

## `impl HarnessEventHooks` › `pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {`

The harness this observer records into.

## `impl HarnessEventHooks` › `pub fn recording_durability(mut self) -> Self {`

Also record every durability primitive this funnel performs, in order.

## `impl HarnessEventHooks` › `pub fn with_written_kill_shape(mut self, shape: WrittenShape) -> Self {`

Which of the two durable shapes `Written`'s kill entry tables this
observer asks the funnel to leave behind.

[`WrittenShape::Torn`] splits the line's `write_all` in two with the
`Written` kill consult between the halves, so a kill armed there lands
in T-APPEND's (w) row — a partial line with no terminating newline —
rather than its complete-unsynced one. With nothing armed the two halves
are both written and the bytes are identical, which is why the shape is
observable from the ledger (two `Wrote` entries, not one) and not from
the file.

## `impl HarnessEventHooks` › `pub fn ledger(&self) -> DurabilityLedger {`

The durability ledger this observer records into.

## `impl HarnessEventHooks` › `pub fn syncs(&self) -> Vec<SyncRecord> {`

Every sync the funnel reported, in the order it reported them.

Recorded whether or not [`Self::recording_durability`] was asked for:
a [`SyncRecord`] is one value per successful sync rather than one per
primitive attempt, so it is not the thing whose cost the ledger switch
exists to gate, and a barrier test that had to remember to turn it on
would silently assert nothing when it forgot.

## `fn apply(`

Do what an observer answered at `point` of `site`.

[`Injection::Kill`] aborts, and it is `abort` rather than `panic!` or
`exit` for the reason [`crate::agent::proc`] gives: the claim under test is
what a process that dies **without running any cleanup** leaves durable, and
both of the others run destructors.

## `fn injected(site: EventSite, point: SubEffectPoint, path: &Path) -> UpstrokeError {`

The `Err` an error-return injection produces.

Deliberately *not* [`UpstrokeError::Io`]: a real write/flush/sync failure keeps
the exact error value the pre-move writer returned, which is the whole of
"the legacy engine's handling of a returned append error is unchanged", so a
simulated one must be distinguishable from it. Same reasoning as
[`crate::agent::proc::AMBIENT_REFUSAL_SIMULATED`].

## `pub const INJECTED_PREFIX: &str = "simulated fault: ";`

The opening words of every injected Event-funnel error, so a caller and a
test can recognise a simulated failure without matching a whole sentence.

## `pub struct EventLog {`

---------------------------------------------------------------------------
The writer
---------------------------------------------------------------------------

## `pub struct EventLog {`

The append-only writer. One per run, held by the engine — `upstroke answer`
deliberately does not write here (it drops a file the engine ingests), so
the log has exactly one writer and interleaved lines are impossible.

The `File` is private and no method hands one out: the allowlist's funnel
section requires each entry to "perform effects only inside site-taking APIs
and never to return writable handles", and this type is the reason a
schema-4 append outside this module cannot be written at all.

`expected_failures_refusals`: "a schema-4 append outside the Event funnel
does not compile". Two of the three ways to try it are type errors, and the
fixtures below pin the *reason* rather than the failure — a `compile_fail`
block with an error code fails the test if the code compiles **or if it
fails for a different reason**, which is what a bare "this does not build"
fixture cannot do. (The third way — writing the bytes with `std::fs` from
another module — is not a type error and is denied by the effect denylist;
see `effects/allowlist.toml`.)

Reaching the handle:

```compile_fail,E0616
use std::path::Path;
use upstroke::events::EventLog;
use upstroke::topology::effects::EventSite;

let mut warnings = Vec::new();
let log = EventLog::open(EventSite::OpenLog, Path::new("events.jsonl"), &mut warnings)
    .expect("open");
let mut handle = log.file;
```

Handing a schema-4 event to the schema-1..3 append:

```compile_fail,E0308
use std::path::Path;
use upstroke::events::EventLog;
use upstroke::topology::effects::EventSite;
use upstroke::topology::events::{DeferWaitElapsed4, TopologyEvent, TopologyEventBody};

let event = TopologyEvent {
    ts: "2026-08-20T09:41:02Z".to_owned(),
    body: TopologyEventBody::DeferWaitElapsed {
        data: DeferWaitElapsed4 { waited_ms: 1, round: 1 },
    },
};
let mut warnings = Vec::new();
let mut log = EventLog::open(EventSite::LegacyOpenLog, Path::new("events.jsonl"), &mut warnings)
    .expect("open");
log.append(EventSite::LegacyAppend, event).expect("append");
```

## `pub struct EventLog` › `opened_at: EventSite,`

Which of the two open sites produced this handle. It decides which
append sites the handle accepts, so a schema-3 log cannot be handed a
schema-4 line and a legacy append cannot emit Shared-scoped evidence.

## `pub struct EventLog` › `poisoned: Option<(EventSite, SubEffectPoint)>,`

The point an entered append returned `Err` at, if one did.

INV-02: "an append that was entered and returned an error never mutates
the live fold, is never retried". The mechanism note is explicit that
this belongs to the funnel — "after an Err from a Written or Synced point
the handle is poisoned (every later append through it fails until the log
is reopened), so no caller can silently retry".
The **site and point** an entered append returned `Err` at.

The site as well as the point (`PR5-EVENTS-046`):
`expected_failures_refusals[9]` is "an append on a poisoned handle
returns an error naming the poisoning point", and one handle accepts
`Append`, `AppendFirst` and `AppendInformational`, so "the poisoning
point" is only half an identification. With the site absent, an
implementation that named the *newly attempted* coordinate instead of
the stored one could not be told from a correct one by any fixture that
poisons and re-attempts through the same site — which was every fixture.

## `impl EventLog` › `pub fn open(`

Open for appending, discarding an incomplete trailing record first.

`site` is [`EventSite::LegacyOpenLog`] for a schema-1..3 caller and
[`EventSite::OpenLog`] for the topology funnel; nothing else opens. See
the module docs for why the two are not one code path.

### Errors

A site that is not an open site; any I/O error reading, truncating, or
creating the log; and, for `Event.OpenLog` only, an injected or real
failure at `Create`, `TruncateTornTail`, or `SyncPrefix`.

## `impl EventLog` › `pub fn open_hooked(`

[`Self::open`] with an observer attached.

### Errors

As [`Self::open`].

## `impl EventLog` › `fn open_with_prefix(`

[`Self::open_hooked`], also handing back the normalized surviving prefix.

Only the stable-prefix barrier needs those bytes — step (4) proves the
reread equal to "the normalized prefix observed at open", and a second
read of the file would be proving the file equal to itself.

## `impl EventLog` › `fn open_legacy(`

`Event.LegacyOpenLog`. The pre-move `EventLog::open`, unchanged.

A process killed mid-write can leave a line with no newline. Appending
straight after it would splice the fragment and the next event into one
unparseable line, losing both.

Terminating the fragment with a newline instead is worse than it looks:
it promotes a torn *tail*, which [`read_all`] recovers from, into an
unparseable line in the *middle*, which [`read_all`] must treat as a
rewritten log and refuse. So the fragment is truncated away. That is
not rewriting history — those bytes are by construction an event that
never finished being written, and no reader could ever have parsed
them — and it keeps "damage anywhere but the end means corruption" a
statement the reader can still trust.

## `impl EventLog` › `let mut prefix = Vec::new();`

Truncate before taking the append handle, through a handle of its
own. On Windows an append-only handle is opened with
FILE_APPEND_DATA and *not* FILE_WRITE_DATA, so `set_len` on it fails
outright with access denied.

## `impl EventLog` › `fn open_funnel(`

`Event.OpenLog`: create (and fsync the directory), truncate a torn tail,
then sync the complete surviving prefix — the file, and the directory
after a truncation changed the length.

The error carries the [`BarrierStep`] it belongs to. The barrier gives
`SyncPrefix` its own resume action ("leaves the prefix possibly
non-durable and refuses the write command resumably"), so which step
failed is a typed fact rather than something a caller reads out of a
message.

## `impl EventLog` › `OpenOptions::new()`

Same handle-of-its-own as the legacy path, and for the same
Windows reason.

## `impl EventLog` › `hooks`

The truncation is in the ledger so the two claims about it
are expressible: that the prefix sync **follows** it, and
that the length synced is the **shortened** one
(`PR5-EVENTS-011`, `PR5-EVENTS-013`). Neither is a statement
a trace holding only syncs can make.

## `impl EventLog` › `for mode in InjectionMode::ALL {`

The point's claim is true once the bytes are gone: "an
unterminated final line **was** truncated before the append
handle was taken".

## `impl EventLog` › `sync_directory(path, hooks, site, SubEffectPoint::Create)`

"create the log if absent and **fsync its directory**": the name
has to be durable, not just the (empty) contents.

## `impl EventLog` › `for mode in InjectionMode::ALL {`

`SyncPrefix` is consulted **before** the sync, in both modes, because
that is where both of its tabled claims are true: "a kill before or at
SyncPrefix simply leaves the prefix for the next open to sync", and a
returned `Err` stands *in place of* a successful sync — "an Err from
SyncPrefix, or a kill before it, leaves the prefix possibly
non-durable and refuses the write command resumably".

## `impl EventLog` › `sync_directory(path, hooks, site, SubEffectPoint::SyncPrefix)`

"and its directory after a truncation changed the length".

## `impl EventLog` › `pub fn append(&mut self, site: EventSite, body: EventBody) -> Result<Event, UpstrokeError…`

Append one schema-1..3 event and get it back **as it will be read back**.

Returning the round-tripped event rather than the one just constructed
is what keeps "the log is the source of truth" literally true. Anything
the wire format cannot represent — a sub-millisecond duration, say —
must not survive in the engine's memory either, or live state would
quietly hold more than a replay could ever restore and the two would
disagree in a way no amount of care at the call sites would catch.

Flushed and synced before returning: §19 promises a crash or power loss
is recoverable by replaying this file, which is only true if the event
reached the disk before the work it describes carried on. A run emits
tens of events, so the cost is noise beside a single attempt.

### Errors

A site that is not [`EventSite::LegacyAppend`]; a handle that is not a
legacy handle; a poisoned handle; a value the wire format cannot carry
(before the append is entered, so the handle stays usable); and any
write, flush, or sync failure (after it is entered, so the handle is
poisoned).

## `impl EventLog` › `pub fn append_hooked(`

[`Self::append`] with an observer attached.

### Errors

As [`Self::append`].

## `impl EventLog` › `let event = Event::now(body);`

Serialize and round-trip *before* the append is entered: a value the
wire cannot carry is not an outcome-unknown append, it is an append
that never happened, and `emit`'s contract is "a FoldError aborts
before any write".

## `impl EventLog` › `pub fn append_topology(`

Append the exact bytes of one schema-4 event.

`coordinator_integration.emit` is "build event → serialize → round-trip →
plan_transition → **append the exact bytes** through the Event funnel",
so the funnel takes bytes that were already round-tripped rather than an
event it would serialize a second time. [`TopologyLine`] is the only way
to make some, and making one *is* the round-trip.

### Errors

A site that is not an append site or does not match the line's kind; a
legacy handle; a poisoned handle; and any write, flush, or sync failure.

## `impl EventLog` › `pub fn append_topology_hooked(`

[`Self::append_topology`] with an observer attached.

### Errors

As [`Self::append_topology`].

## `impl EventLog` › `fn write_committed(`

The one write path: `write_all` → `flush` → `sync_data`, with the three
parent-side points around it.

`bytes` already ends in its newline. The newline is the commit marker, so
it is part of the same `write_all` and never a second one — splitting it
would make every append pass through the torn state on purpose.

## `impl EventLog` › `let ledger = hooks.durability_ledger();`

(e-w): "write_all failed after a partial write". The funnel performs
the partial write itself, because an injection mode is defined as
returning Err "after performing or partially performing the
primitive" and a torn tail nobody wrote is not that shape.

## `impl EventLog` › `let cut = torn_cut(bytes);`

Only an observer asks for this, and only to place a kill in
the torn half of `Written`'s kill entry. Production never
reaches it, so production still writes the line once.

## `impl EventLog` › `self.at_point(`

(e-u): "write_all succeeded (full line, newline present) and flush or
sync_data returned an error". `WrittenFull` declares error-return
only — a kill here leaves the shape `Written`'s kill entry covers.

## `impl EventLog` › `let synced = self.file.sync_data();`

Fused with its ledger entry for the reason `sync_log_file` gives, and
for a second one here: the `Synced` consults below are the coordinate
`(e-s)` names — "sync_data returned an error **after the data reached
the disk**" — and an observer can only tell that coordinate from the
one before the sync by reading this entry at the moment it is
consulted (`PR5-EVENTS-032`, `PR5-EVENTS-035`).

## `impl EventLog` › `for mode in InjectionMode::ALL {`

(e-s): "sync_data returned an error **after the data reached the
disk**", which is why this coordinate is after the sync rather than
instead of it. Indistinguishable from (e-u) to the process, and the
durable shape is the same.

## `impl EventLog` › `fn at_point(`

Consult one (point, mode) coordinate. Anything but `Proceed` past the
entry of an append poisons the handle, whichever mode produced it: the
contract is about the *outcome* being unknown, not about how it became
unknown.

## `impl EventLog` › `fn io(&self, source: std::io::Error) -> UpstrokeError {`

A real write, flush, or sync failure keeps [`UpstrokeError::Io`] — the
exact value the pre-move writer returned — so a legacy caller's handling
of it is unchanged. The point it reached is recorded on the handle, and
the *next* append through the handle is the error that names it.

## `impl EventLog` › `ledger.record(DurableStep::Wrote, &self.path, bytes.len() as u64);`

Recorded whatever it returned, so "exactly one primitive attempt and
one error" is a countable claim rather than a description
(`PR5-EVENTS-044`).

## `fn check_poison(&self) -> Result<(), UpstrokeError>` › `Some((site, point)) => Err(UpstrokeError::EventLog {`

The **stored** coordinate, never the one now being attempted:
the message identifies where the outcome became unknown, and the
later attempt is not that place.

## `impl EventLog` › `pub fn poisoned_at(&self) -> Option<SubEffectPoint> {`

The point an entered append returned `Err` at, or `None` while the handle
is usable.

## `impl EventLog` › `pub fn poisoned_site(&self) -> Option<EventSite> {`

The site the poisoning append was made at, or `None` while the handle is
usable.

## `impl EventLog` › `pub fn opened_at(&self) -> EventSite {`

Which site this handle was opened at.

## `pub const POISONED_PREFIX: &str = "the event log handle is poisoned: ";`

The opening words of every poisoned-handle refusal.

## `pub const OPEN_SITES: &[EventSite] = &[EventSite::OpenLog, EventSite::LegacyOpenLog];`

The two sites [`EventLog::open`] accepts.

## `pub const TOPOLOGY_APPEND_SITES: &[EventSite] = &[`

The three sites [`EventLog::append_topology`] accepts.

## `pub fn site_for(body: &TopologyEventBody) -> EventSite {`

The site an event belongs at.

`Event.AppendFirst` is "run_started; the commitment boundary",
`Event.AppendInformational` is "a lenient informational append", and
`Event.Append` is "every later transaction append". The lenient/transactional
split is not re-derived here: `TopologyEventBody::is_transaction` is PR3's
and frozen, and a second list would be a second thing to keep in step.

Filing an event under the wrong site puts its faults at the wrong registry
coordinate, which is why the funnel checks rather than trusts.

## `fn torn_cut(bytes: &[u8]) -> usize {`

Where a partial write stops.

Half the line, rounded down, and never fewer than one byte: a committed line
is at least `{}\n`, so half of it can never include the terminating newline
and the result is always a torn tail rather than an accidental commit.

## `fn sync_log_file(`

fsync the surviving prefix and record what was made durable.

The sync and the ledger entry are one call on purpose, and the reason is a
measured one. An fsync is not observable from user space, so the ledger is
the only proxy a test has for it; with the sync and the record written as two
statements, moving the `SyncPrefix` consult to *between* them puts the
injection after the syscall and before the only thing that can see it, and
the mutation survives the suite. It did, when this was measured. Fused, the
only place the consult can be moved to is after the record — where
`an_injected_sync_failure_at_open_names_syncprefix_and_hands_out_no_handle`
kills it.

The residual that boundary named — "**deleting the `sync_all` call itself is
undetectable by any test on this machine**" — is `PR5-CONF-012`, and it is
narrower now than "undetectable". The syscall is [`crate::util::fsync_file`],
the one place in the funnel modules that may make it, and two things watch
it from either side: `effects::tests::every_file_durability_barrier_in_a_
funnel_module_goes_through_one_call` fails if the call leaves that function,
and `util::barriers_performed` counts entries so a ledger record that no
barrier produced is a disagreement. What is still true, and still stated, is
that nothing here can see *inside* `fsync`.

## `let len = file.metadata().map_err(io)?.len();`

The length is the filesystem's answer, not a number this funnel carried
along: a ledger that reported its own idea of the length could agree with
itself while the file said something else.

## `fn sync_directory(`

fsync the directory holding `path`, so the log's *name* is durable, on every
platform (`PR5-CONF-013`).

This was Unix-only, and the comment that said so had the recipe in it: "needs
`FILE_FLAG_BACKUP_SEMANTICS` on Windows, which std does not expose". True of
std, and the reason [`crate::util::fsync_dir`] does not use std there —
`scope` requires `Event.OpenLog`'s "directory fsync" and "file **and
directory** after a truncation" with no platform exception, and the appeal to
NTFS's own metadata ordering was an argument for a guarantee the packet asks
this crate to make.

## `pub struct TopologyLine {`

---------------------------------------------------------------------------
A checked schema-4 line
---------------------------------------------------------------------------

## `pub struct TopologyLine {`

One serialized, round-tripped schema-4 event: the exact bytes a coordinator
checked, and the only thing [`EventLog::append_topology`] accepts.

The field is private and [`Self::round_trip`] is the only constructor, so a
caller cannot hand the funnel bytes that never survived their own wire
format. That is `emit`'s "serialize → round-trip → … → append the exact
bytes" expressed as a type rather than as a rule a call site is asked to
remember, in the same spirit as `TopologyDelta`.

Bytes that never round-tripped cannot be handed to the funnel, and the
fixture pins which error says so:

```compile_fail,E0451
use upstroke::events::log::TopologyLine;
use upstroke::topology::effects::EventSite;

let line = TopologyLine {
    committed: "{\"ts\":\"now\"}\n".to_owned(),
    kind: "run_started",
    site: EventSite::AppendFirst,
};
```

## `pub struct TopologyLine` › `committed: String,`

The JSON, plus its terminating newline.

## `impl TopologyLine` › `pub fn round_trip(event: &TopologyEvent) -> Result<(Self, TopologyEvent), UpstrokeError> {`

Serialize `event`, prove it survives its own wire format, and keep the
exact bytes. The returned event is what the wire will give back.

### Errors

[`UpstrokeError::EventLog`] if the value cannot be serialized or does not
round-trip.

## `impl TopologyLine` › `pub fn kind(&self) -> &'static str {`

The event's wire tag.

## `impl TopologyLine` › `pub fn site(&self) -> EventSite {`

The site this line belongs at.

## `impl TopologyLine` › `pub fn committed_bytes(&self) -> &[u8] {`

The exact bytes, newline included.

## `pub enum BarrierStep {`

---------------------------------------------------------------------------
The stable-prefix barrier
---------------------------------------------------------------------------

## `pub enum BarrierStep {`

Which step of the stable-prefix barrier refused.

The names are the packet's own — `Event.OpenLog`, its `SyncPrefix` point,
the `Event.ProvePrefixStable` observation, and the checked replay — so a
caller reporting "the failed step" reports something the fault registry can
be keyed by rather than a sentence.

## `pub enum BarrierStep` › `OpenLog,`

`Event.OpenLog` itself could not open or normalize the log.

## `pub enum BarrierStep` › `SyncPrefix,`

`Event.OpenLog.SyncPrefix` returned `Err`; the prefix is possibly not
durable.

## `pub enum BarrierStep` › `ProvePrefixStable,`

`Event.ProvePrefixStable`: the reread differs from the normalized prefix
in a byte, in its length, or at its boundary.

## `pub enum BarrierStep` › `CheckedReplay,`

The checked replay refused the proven bytes.

## `impl BarrierStep` › `pub const ALL: &'static [Self] = &[`

Every step, in the order the barrier performs them.

## `impl BarrierStep` › `pub const fn name(self) -> &'static str {`

The step's name, as the packet writes it.

## `pub struct BarrierError {`

A barrier that did not hold, and which step it failed at.

Typed rather than a formatted string because "returns an error **naming the
barrier step**" is a claim a test has to be able to check without matching
prose, and because PR7 reports the step to the operator.

## `pub struct BarrierError` › `pub step: BarrierStep,`

Which step refused.

## `pub struct BarrierError` › `pub path: PathBuf,`

The log.

## `pub struct BarrierError` › `pub detail: String,`

What the step found.

## `pub struct StablePrefix {`

A log prefix that has been synced, reread, proven stable, and replayed.

Holding one is the evidence `stable_prefix_barrier` requires before "any
other fold-derived mutation". The append handle comes with it because a
write command needs both and the barrier is what entitles it to either.

## `impl StablePrefix` › `pub fn log(&mut self) -> &mut EventLog {`

The append handle the barrier entitles this command to.

## `impl StablePrefix` › `pub fn bytes(&self) -> &[u8] {`

The exact bytes that were synced, reread, proven, and replayed.

## `impl StablePrefix` › `pub fn events(&self) -> &[TopologyEvent] {`

The events the barrier itself parsed from those bytes.

**Exposing the barrier's own parse, not offering a second one.** Step (5)
parses the reread bytes once and replays them into [`Self::fold`]; before
this accessor the events were dropped, so a caller that needed one — a
recovery convergence needing the `AttemptRecord` a settlement carried,
which the fold does not keep — had to parse the log again.

That second parse is what
`the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold`
refuses, and it is right to: "I used the proven bytes" is the argument
every unproven read would make, and a second entry point is reachable
around the barrier by anyone. There is still exactly one production
`TopologyFold::parse_log` call, in this module, on the bytes step (4)
proved — and that census still refuses everyone else.

## `impl StablePrefix` › `pub fn fold(&self) -> &TopologyFold {`

The fold built from exactly those bytes.

## `impl StablePrefix` › `pub fn into_log_and_fold(self) -> (EventLog, Vec<u8>, Vec<TopologyEvent>, TopologyFold) {`

Take the two halves apart.

## `pub fn first_line_digest(bytes: &[u8]) -> Option<String> {`

The digest of a log's committed first line, in the `sha256:<hex>` shape the
records use.

The bytes are the line **without** its terminating newline — the event's own
bytes, the thing `run_started_sha256` names. Exposed so the private commit
record and this barrier compute the same number from one definition rather
than two that agree by inspection.

## `pub fn establish_stable_prefix(`

`coordinator_integration.stable_prefix_barrier`, in order and in one place.

1. `Event.OpenLog` opens the log and normalizes a torn tail.
2. `Event.OpenLog.SyncPrefix` **successfully** syncs the complete surviving
   prefix.
3. The whole file is reread.
4. The reread bytes and boundary are proven unchanged: byte-equal to the
   normalized prefix observed at open, the same length, ending in a newline
   (no torn tail reappeared), and — for a schema-4 run — the committed first
   line unchanged.
5. Exactly those reread bytes are handed to the checked replay. Never a third
   read.

Only then does a caller hold an append handle. A failed sync, an unstable
reread, or a replay refusal returns [`BarrierError`] naming the step and
hands out nothing.

### Errors

[`BarrierError`] naming the step that refused.

## `hooks.phase(EventSite::OpenLog, HookPhase::Before);`

(1) and (2). `open_with_prefix` performs the sync and hands back the
normalized prefix it observed; a failure at `SyncPrefix` is the only one
of its failures the barrier reports separately, because it is the only one
the packet gives its own resume action.

## `hooks.phase(EventSite::ProvePrefixStable, HookPhase::Before);`

(3). Read-only: `Event.ProvePrefixStable` "performs no effect".

## `if !reread.is_empty() && reread.last() != Some(&b'\n') {`

(4). Every clause of "bytes and boundary" separately, so a failure says
which one — and in an order that leaves each clause separately reachable.
Byte-equality implies the other two, so a proof that checked it first
would make the boundary and length clauses unreachable and untestable:
the boundary goes first, then the length, then the bytes.

## `let events = TopologyFold::parse_log(&reread).map_err(|error| BarrierError {`

(5). Exactly those bytes. `reread` is moved into the result afterwards, so
there is no third read to accidentally take.

## `pub fn read_all(path: &Path, warnings: &mut Vec<String>) -> Result<Vec<Event>, UpstrokeEr…`

---------------------------------------------------------------------------
Readers
---------------------------------------------------------------------------

## `pub fn read_all(path: &Path, warnings: &mut Vec<String>) -> Result<Vec<Event>, UpstrokeEr…`

Read a whole log.

An unterminated **final** record is a torn tail — the shape a kill leaves —
and is dropped with a warning. A newline is the commit marker written after
every event, so any invalid newline-terminated record is corruption even
when it is last: something rewrote history, and deriving state from the
survivors would produce a confident wrong answer. That errors.

### Errors

[`UpstrokeError::EventLog`] for a rewritten log; [`UpstrokeError::Io`] otherwise.

## `pub(crate) fn read_bytes(path: &Path) -> Result<Vec<u8>, UpstrokeError> {`

Read the exact bytes a whole-log consumer will parse. Kept separate so a
consumer that needs a stable snapshot can compare two reads before trusting
the first one.

[`crate::util::read_file_bounded`] rather than `std::fs::read`, here and at
every other read of a log in this module, for the reason `PR5-RD-001` gave
about the run-directory classifier: `read_to_end` does not terminate on a
source that never reaches end of file, and a log path is a path in a run
directory rather than a value this crate chose. The bound is the file's own
length, so a real log of any size is still read in full.

## `pub(crate) struct ParsedLines {`

A parsed whole-log snapshot. The only recoverable parse condition is typed
separately from the events so callers never have to infer its meaning from
human-readable warning text.

## `pub(crate) fn parse_bytes(path: &Path, bytes: &[u8]) -> Res…` › `let committed_end = bytes`

EventLog::append writes the newline after the JSON bytes. EventLog::open
likewise discards everything after the last newline before resuming, so
whole-log readers must use the same boundary: even syntactically complete
JSON without its terminating newline was never a committed event.

## `pub struct LogTail {`

Incremental reader for `status --follow`.

Reads only complete lines: a poll that catches the writer mid-line stops at
the last newline and picks the rest up next time, so a follower never sees
half an event.

## `impl LogTail` › `pub fn skip_existing(&mut self) {`

Start from the end, so a follower attached to a live run reports only
what happens from now on.

## `impl LogTail` › `pub fn poll(&mut self, warnings: &mut Vec<String>) -> Result<Vec<Event>, UpstrokeError> {`

Every complete line written since the last poll.

### Errors

[`UpstrokeError::Io`] if the log cannot be read; [`UpstrokeError::EventLog`]
for a rewritten log.

## `pub fn poll(&mut self, warnings: &mut Vec<String>) -> Resul…` › `if length < self.offset {`

Truncated or replaced underneath us: start over rather than
read from an offset that now means something else.
