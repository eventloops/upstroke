# `src/events/log/tests.rs`

Extended notes for [`src/events/log/tests.rs`](../../../../src/events/log/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The Event funnel's tests.

Three rules this project pays for when it forgets them are load-bearing
here:

* **A function may not be its own oracle.** The byte-identity claim is
  measured against [`super::premove::PremoveEventLog`] — the writer as it
  stood at `ff0490a` — never against the moved writer.
* **Enumerations come from the types.** The site grids iterate
  `EventSite::ALL`, `SubEffectPoint::modes()` and `BarrierStep::ALL` rather
  than a list somebody thought of, so a variant added later is uncovered
  loudly instead of silently.
* **Hostility is a count.** The differential grid varies the log's shape,
  the torn tail's length and its bytes independently and asserts how many
  distinct values each axis took.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_types)]`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
carries this module's review clause -- effects only inside site-taking APIs,
no writable handle returned. `decisions.effect_site_inventory.mechanism` (2).

## `static SCRATCH: AtomicU32 = AtomicU32::new(0);`

---------------------------------------------------------------------------
Fixtures
---------------------------------------------------------------------------

## `fn scratch(tag: &str) -> PathBuf {`

A directory of this test's own. Numbered as well as named, because several
of the grids below want a fresh log per cell.

## `fn event_log_message(error: &UpstrokeError) -> &str {`

The **message** a `UpstrokeError::EventLog` carries, without its rendering.

`UpstrokeError::EventLog`'s Display is `event log {path}: {message}`, so
`error.to_string().contains(x)` is satisfied by the *path* as readily as by
anything the funnel decided to say. Two catalogue mutations lived in that
gap, because the fixtures named their scratch directories after the very
point they then looked for (`PR5-EVENTS-045`, `PR5-EVENTS-046`). Assertions
about what an error *says* go through here.

## `fn lossy_duration_attempt() -> EventBody {`

An `attempt_finished` whose duration the wire format **cannot** carry.

`duration_ms` is an integer, so 1,500,123 µs is written as `1500` and reads
back as 1.500 s exactly. That makes this body the one fixture that can tell
the constructed event from the round-tripped one — DESIGN.md:406's "it
applies the event as it will be read back rather than as constructed. A live
run and a replay of its own log are therefore the same computation".

Every other body in these grids is lossless, which is why
`PR5-CORRECTNESS-015` survived: `Ok(written)` -> `Ok(event)` is invisible
to a comparison whose inputs round-trip unchanged.

## `const LOSSY_CONSTRUCTED: Duration = Duration::from_micros(1_500_123);`

The duration `lossy_duration_attempt` carries, and the duration a replay of
it yields. Written out rather than computed from the codec.

## `fn duration_of(body: &EventBody) -> Duration {`

The `duration` of an `attempt_finished` body, for the assertions above.

## `fn unserializable() -> EventBody {`

An event carrying a value `serde_json` refuses to serialize.

Not a contrivance: `limit_usd` is an ordinary `f64` on an ordinary event, and
`serde_json` refuses non-finite floats. It is the only *reachable* failure
this funnel has strictly before the append is entered, which makes it the
only way to prove the entered/not-entered boundary is where the code says.

## `fn run_started_event() -> TopologyEvent {`

A `run_started`: the one kind that belongs at `Event.AppendFirst`.

Hand-built rather than borrowed, because the fold's own fixture lives in a
private `mod tests` of a frozen file. None of these values is read by the
funnel — it takes the round-tripped *bytes* — so what the fixture has to be
is a `RunStarted4` that survives its own wire format, and nothing more.

## `fn informational_event() -> TopologyEvent {`

A `pool_exhausted`: one of the three kinds the frozen lenient class names,
and therefore an `Event.AppendInformational`.

## `fn append_site_lines() -> Vec<(EventSite, TopologyLine)> {`

One line per schema-4 append site, keyed by [`TOPOLOGY_APPEND_SITES`] so a
site added to the frozen inventory later has no line here and says so.

This exists because `PR4-CONF-002` is in the standing ledger: a grid that
drove one role and reasoned about the others left both contract-named probe
paths emitting no evidence, with the whole suite green. Three sites are
named by this slice's contract, and a grid that drives one of them proves
one of them.

## `fn inputs() -> FrozenInputs {`

Frozen inputs for the checked replay. The plan never has to match a
`run_started` here: every barrier test either replays an empty prefix or
asserts a refusal, and the refusals are the fold's, not this plan's.

## `struct Witness {`

---------------------------------------------------------------------------
Observers
---------------------------------------------------------------------------

## `struct Witness {`

Records every coordinate the funnel offered, answers `Proceed` to all of
them, and keeps the sync ledger.

## `struct Witness` › `at_consult: Vec<(SubEffectPoint, InjectionMode, Vec<DurableStep>)>,`

What the durability ledger held **at** each `(point, mode)` consult.

The whole content of `(e-s)` — "sync_data returned an error *after the
data reached the disk*" — is which side of the sync the coordinate is
on, and that is not readable from a trace taken afterwards: by then the
sync has happened either way. So it is read at the moment the funnel
asks.

## `impl Witness` › `fn recording_durability(mut self) -> Self {`

Record the funnel's durability primitives too, in order.

## `impl Witness` › `fn steps(&self) -> Vec<DurableStep> {`

The durability steps this funnel performed, in order.

## `struct FailAt {`

Returns `Err` at exactly one coordinate, and records the ledger so a test can
prove the primitive did *not* run when the coordinate is before it.

## `struct Rewrite {`

Rewrites the log between the barrier's steps.

`synced` fires after `SyncPrefix` and before the reread, so a mutation there
is exactly an unstable reread. `phase(ProvePrefixStable, After)` fires after
the stability proof and before the checked replay, so a mutation there is
what separates "replayed the bytes it proved" from "read the file a third
time".

## `struct TornWriter;`

Asks for the torn half of `Written`'s kill entry without arming a kill.

## `const SITE_ROLES: &[(EventSite, &str)] = &[`

---------------------------------------------------------------------------
The site partition
---------------------------------------------------------------------------

## `const SITE_ROLES: &[(EventSite, &str)] = &[`

Every site of the group, classified into exactly one role.

The table is written from the frozen enum's own doc comments and from
`effect_site_inventory.identity`, not from `EventLog`'s `match` arms — a
classification derived from the code under test cannot disagree with it.

## `fn every_event_site_is_classified_and_the_funnel_accepts_ex…` › `let classified: BTreeSet<EventSite> = SITE_ROLES.iter().map(|(site, _)| *site).collect();`

The list is derived from the type, so a site added later is uncovered
loudly rather than silently.

## `fn every_event_site_is_classified_and_the_funnel_accepts_ex…` › `let lines = append_site_lines();`

Appending: one handle per scope, every site tried against both. The
schema-4 cell hands each site a line of *its own* kind, so an accepting
site is exercised rather than refused for the line's sake — the three
append sites are three separately droppable behaviours, not one.

## `fn every_event_site_is_classified_and_the_funnel_accepts_ex…` › `let line = lines`

A site with no line of its own is one this funnel must refuse, so any
line will do for it; `defer_wait_elapsed` is the one it gets.

## `fn a_handle_does_not_mix_the_legacy_and_shared_scopes()` › `assert_eq!(fs::read(&path).expect("legacy log").len(), 0);`

Nothing was written by either refusal: a scope refusal happens before the
append is entered.

## `const INFORMATIONAL_KINDS: &[&str] = &["capacity_snapshot", "pool_exhausted", "design_def…`

The lenient class, transcribed from `src/topology/events.rs`'s own frozen
statement of it — "the lenient class is exactly these three by name" — and
not computed from the predicate the funnel uses.

## `fn an_events_append_site_is_decided_by_the_frozen_transacti…` › `let expected: &[(&str, EventSite)] = &[`

`run_started` is `AppendFirst` ("the commitment boundary"); the three
lenient kinds are `AppendInformational`; everything else is `Append`.

## `fn an_events_append_site_is_decided_by_the_frozen_transacti…` › `let classified: Vec<(&str, EventSite)> = append_site_lines()`

And the two sites the table names that a `defer_wait_elapsed` is not: a
`run_started` really does classify as the commitment boundary and a
`pool_exhausted` really does classify as lenient, so the three arms of
`site_for` are each reached by an event rather than by argument.

## `fn open_grid() -> Vec<(&'static str, Option<Vec<u8>>)> {`

---------------------------------------------------------------------------
Byte identity with the pre-move writer
---------------------------------------------------------------------------

## `fn open_grid() -> Vec<(&'static str, Option<Vec<u8>>)> {`

Every log shape the open path can meet, each varying one axis.

`None` is "the file does not exist"; the rest are exact byte strings. The
torn cases vary the *length* of the tail (1, 5, 12 bytes) and its *content*
(ASCII, valid JSON, a split multi-byte character) independently of whether
there is a committed prefix in front of it, because a grid that moved those
together could be satisfied by a correlated field.

## `fn open_grid() -> Vec<(&'static str, Option<Vec<u8>>)>` › `split_utf8.extend_from_slice(&[0xE2, 0x82]);`

The first two bytes of a three-byte character: invalid UTF-8 on its own,
and dropped before the committed bytes are validated.

## `fn the_grid_varies_shape_and_tail_length_and_tail_content_i…` › `let grid = open_grid();`

Hostility as counts, not as prose: `PR4-CONF-004`/`-006` are in the
ledger because a grid whose axes moved together was satisfied by a
correlated field.

## `fn the_legacy_open_is_byte_identical_to_the_pre_move_writer…` › `assert_eq!(`

Warnings are compared with the path's directory removed, because the
two writers were pointed at two directories on purpose — a comparison
that shared one file could not tell "wrote the same bytes" from "wrote
nothing twice".

## `fn the_legacy_append_is_byte_identical_to_the_pre_move_writ…` › `let bodies: Vec<EventBody> = vec![`

Four bodies, so a comparison cannot pass by writing one constant twice —
and one of them is **lossy over the wire**, which is what makes the
returned event a claim and not a copy of the input (`PR5-CORRECTNESS-015`).

## `fn the_legacy_append_is_byte_identical_to_the_pre_move_writ…` › `let before = crate::util::rfc3339_utc_now();`

Bracketing clock reads, so the `ts` the writers stamp can be checked
for being a *time* rather than merely for being equal to itself. The
format is fixed-width RFC 3339 UTC, so lexical order is chronological.

## `fn the_legacy_append_is_byte_identical_to_the_pre_move_writ…` › `if matches!(body, EventBody::AttemptFinished { .. }) {`

The returned body is the one the wire carries, not the one that
was handed in. `PR5-CORRECTNESS-015`: returning the constructed
event leaves the coordinator holding a duration a replay of its
own log can never restore.

## `fn the_legacy_append_is_byte_identical_to_the_pre_move_writ…` › `assert_eq!(`

The timestamps are the two writers' own `Event::now`, so the bytes are
compared with the `ts` field of every line normalized and nothing
else touched. A mutation to the separator, the ordering, the newline
or the payload still shows.

## `fn the_legacy_append_is_byte_identical_to_the_pre_move_writ…` › `for (writer, path) in [("moved", &moved), ("oracle", &premove)] {`

Normalising `ts` is what lets the bytes be compared at all, and it is
also a hole: it says nothing about the value. `PR5-CORRECTNESS-006` /
`PR5-SEAMS-003` is a moved writer stamping `1970-01-01T00:00:00Z`,
which this grid folded away. So the field is checked separately, on
both writers, against clock reads taken either side of the appends.

## `fn the_legacy_append_is_byte_identical_to_the_pre_move_writ…` › `let committed_before = committed_lines(seed.as_ref());`

Both files gained exactly one newline-terminated line per body beyond
the committed prefix they started with.

## `fn a_legacy_open_that_fails_fails_the_way_the_pre_move_writer_did() {`

The **error contract** of the legacy open is the pre-move writer's too.

`invariants_preserved[0]` is "EventLog semantics unchanged for legacy
callers", and an error *variant* is semantics: `UpstrokeError::Io` carries the
`std::io::Error` a caller can match `kind()` on, while
`UpstrokeError::EventLog` carries a rendered string and loses it.
`PR5-SEAMS-004` is exactly that swap inside `open_legacy`, and the
differential grid could not see it because every one of its thirteen shapes
**opens successfully** — it varies the file's bytes, and a failing open is a
property of the path.

So this grid varies the path, and the expectation comes from the oracle
rather than from a variant written down here: whatever the pre-move writer
returns, the moved writer returns the same variant, with the same path
named. A control asserts the oracle really did fail, because a grid whose
cells all succeeded would compare two `Ok`s and pass.

## `fn a_legacy_open_that_fails_fails_the_way_the_pre_move_writ…` › `type Case = (&'static str, fn(&Path) -> PathBuf);`

(name, how to build a path that cannot be opened for append).

## `fn a_legacy_open_that_fails_fails_the_way_the_pre_move_writ…` › `unexercisable.push(*name);`

A machine that can open this anyway (a `root` that ignores the
read-only bit) cannot host this cell. Recorded, never silent.

## `fn a_legacy_open_that_fails_fails_the_way_the_pre_move_writ…` › `assert!(`

The variant is `Io`, and it is asserted positively as well as
relatively: a mutation applied to *both* sides would keep the
discriminants equal.

## `fn a_legacy_open_that_fails_fails_the_way_the_pre_move_writ…` › `assert_eq!(`

And the same path is named, with the two directories folded away —
they are different on purpose, so a comparison cannot pass by naming
nothing.

## `fn committed_lines(seed: Option<&Vec<u8>>) -> usize {`

How many newline-terminated lines a seed carried.

## `fn appended_timestamps(path: &Path, skip: usize) -> Vec<String> {`

The `ts` value of every line after the first `skip`, as written.

Read out of the file rather than off the returned event, because the claim
is about the bytes a reader will see: `status` renders this field and
`export` copies it into attempt timestamps.

## `fn the_legacy_append_stamps_the_clocks_answer_at_every_entry_point() {`

The `ts` this writer stamps is the clock's answer, at both append sites.

The differential above compares two writers with `ts` normalised away, so it
is blind to a *shared* wrong answer and to a moved-writer-only one alike;
this asks the value directly. `PR5-CORRECTNESS-006` / `PR5-SEAMS-003` is
`event.ts = "1970-01-01T00:00:00Z"`, which every existing grid folded out.

Both legacy entry points, because they are two functions:
[`EventLog::append`] and [`EventLog::append_hooked`]. The schema-4 sites take
pre-round-tripped bytes and stamp nothing, so they are not in this class —
`the_topology_append_carries_the_callers_own_bytes` is what holds them.

## `fn the_legacy_append_stamps_the_clocks_answer_at_every_entr…` › `assert_ne!(`

The epoch is not merely "an old time": it is the value a clock that
cannot be read yields, and this machine's clock can be read.

## `fn the_legacy_append_stamps_the_clocks_answer_at_every_entr…` › `let written = appended_timestamps(&path, 0);`

And the same value reached the file, which is what `status` renders and
`export` copies.

## `fn the_legacy_append_returns_the_event_a_replay_of_this_log_yields() {`

The event handed back is the event a replay of this log produces.

DESIGN.md:406: "it applies the event **as it will be read back** rather than
as constructed. A live run and a replay of its own log are therefore the same
computation." The oracle is [`crate::events::read_all`] — the reader, not
this writer — so `Ok(written)` -> `Ok(event)` cannot be green.

Both legacy entry points again, and the value is one the wire genuinely
cannot carry, so "returned" and "constructed" are different observations.

## `fn normalize_timestamps(bytes: &[u8]) -> String {`

Replace every `"ts":"…"` value with a constant. Deliberately narrow: only the
one field the two writers cannot agree on.

## `fn the_legacy_open_performs_none_of_the_syncs_the_pre_move_…` › `assert!(`

`EventSite::LegacyOpenLog.sub_effects()` is `&[]` in the frozen inventory.
A legacy open that acquired `SyncPrefix` would be a new way for a
schema-3 run to fail at open, which `production_effect` forbids.

## `fn an_append_writes_the_whole_line_once_then_flushes_then_syncs() {`

---------------------------------------------------------------------------
The append's own durability trace
---------------------------------------------------------------------------

## `fn an_append_writes_the_whole_line_once_then_flushes_then_syncs() {`

One `write_all` of the whole line, then a `flush`, then a `sync_data`
(`PR5-EVENTS-049`, `PR5-EVENTS-051`).

`production_effect`: "the event-log writer keeps its **exact**
write/flush/sync and torn-tail truncation semantics". Until the ledger
covered the append path, none of the three was an observable: splitting the
line's `write_all` into the JSON and then its LF commit marker, and deleting
the `flush` outright, both left the whole suite green. The only guard that
touched either was a *source census* asserting the literal
`self.file.write_all(` appears once in the module — which constrains where
the call is spelled, not how many times it runs, and the split reused the
one call site.

The byte count is asserted, not just the number of calls: one `write_all`
carrying half the line and a second carrying the rest would otherwise read
as "one write" on the count alone.

## `fn the_synced_consults_are_offered_after_the_data_is_durable() {`

Both `Synced` consults happen **after** `sync_data`, which is the whole
content of the coordinate (`PR5-EVENTS-032`, `PR5-EVENTS-035`).

`transaction_fault_matrix[16]` defines `(e-s)` as "sync_data returned an
error **after the data reached the disk** (indistinguishable from (e-u) to
the process)". An injector that short-circuits and returns the injected
`Err` *before* `sync_data` runs produces an `(e-u)` under an `(e-s)` label,
and the tabled-shape test could not tell: `leaves_complete_line: true` holds
either way, because the line was written and flushed before the sync. The
kill coordinate has the same problem in the other direction, and worse — a
kill is `abort`, so no in-process test can observe its aftermath at all.

What separates them is what was already durable *at the moment the funnel
asked*, so that is what is read.

## `fn the_synced_consults_are_offered_after_the_data_is_durabl…` › `witness.at_consult.clear();`

The open's own barrier is not this append's trace: both are cleared so
every step read below was performed by the append.

## `fn the_synced_consults_are_offered_after_the_data_is_durabl…` › `let written = witness`

And the earlier coordinates are on their own side of it, so the assertion
above is about this point rather than about the end of the append.

## `fn a_real_write_failure_is_attempted_once_poisons_the_handle_and_is_not_retried() {`

A **real** primitive failure is attempted once and never retried
(`PR5-EVENTS-044`).

`invariants[1]`: "an append that was entered and returned an error never
mutates the live fold, **is never retried**, and is never resolved from
memory". Every append failure the suite could previously build was an
*injected* one, delivered by the hook harness at a coordinate rather than by
the file — so the retry branch was never entered and "exactly one primitive
attempt" was true by construction of the injector, saying nothing about a
real one.

`/dev/full` is a real one: it opens, it reads as empty, and every write to it
returns `ENOSPC`. Linux only, and named rather than skipped elsewhere — this
is the one place in the lane where the primitive itself fails.

## `fn a_real_write_failure_is_attempted_once_poisons_the_handl…` › `let mut log =`

The **legacy** open, which takes no barrier: `fsync` on a character
device is `EINVAL`, so `Event.OpenLog`'s prefix sync cannot be performed
against one. The claim under test is `write_or_poison`'s, which both
append paths share, and the legacy site reaches it without needing a
device that can be fsynced.

## `fn open_truncates_the_torn_tail_before_it_syncs_and_syncs_the_shortened_length() {`

---------------------------------------------------------------------------
Event.OpenLog
---------------------------------------------------------------------------

## `fn open_truncates_the_torn_tail_before_it_syncs_and_syncs_the_shortened_length() {`

The prefix sync **follows** the truncation and records the **shortened**
length (`PR5-EVENTS-011`, `PR5-EVENTS-013`).

The lane had both axes and never crossed them. The test that compares a
synced length against the filesystem deliberately seeds a *complete*
unsynced line — "and nothing was truncated: the line was complete" is its
own closing assertion — and the test that seeds a torn tail reads only
points, never a length. So syncing the *pre*-normalized length and
truncating afterwards satisfied both: one had no truncation to get wrong,
the other never read the number that would have been wrong.

## `fn open_truncates_the_torn_tail_before_it_syncs_and_syncs_t…` › `let expected: Vec<DurableStep> = vec![`

One expectation for every platform (`PR5-CONF-013`). This used to fork on
`cfg!(unix)` because there was no directory fsync on Windows; `scope`'s
"file **and directory** after a truncation" carries no platform
exception, and `util::fsync_dir` now performs it on both.

## `fn open_syncs_the_surviving_prefix_and_the_ledger_agrees_wi…` › `let mut warnings = Vec::new();`

A line written by an earlier handle and never synced — the case
`proof_tests[9]` names explicitly. `WrittenFull`'s error-return leaves
exactly that shape: the full newline-terminated line, no flush, no sync.

## `fn open_fsyncs_the_directory_when_it_creates_the_log_and_af…` › `let expected_directory_syncs = 1;`

One expectation for every platform (`PR5-CONF-013`). This used to be
`if cfg!(unix) { 1 } else { 0 }`, because `File::open` on a directory needs
`FILE_FLAG_BACKUP_SEMANTICS` and std does not expose it — true of std, and
the reason `util::fsync_dir` calls `CreateFileW` there instead. `scope`'s
"directory fsync" and "file **and directory** after a truncation" carry no
platform exception, so neither does this.

## `fn open_fsyncs_the_directory_when_it_creates_the_log_and_af…` › `let untouched = log_path("dir-fsync-untouched");`

An untouched existing log syncs the file and nothing else: the directory
entry did not move.

## `fn an_injected_sync_failure_at_open_names_syncprefix_and_ha…` › `let mut failing = FailAt::error(SubEffectPoint::SyncPrefix);`

The barrier reports the same failure as its own step.

## `fn every_open_point_is_offered_in_every_mode_the_frozen_inv…` › `let points = EventSite::OpenLog.sub_effects();`

Derived from the type, not from a list: `sub_effects()` x `modes()`.

## `fn every_open_point_is_offered_in_every_mode_the_frozen_inv…` › `let mut offered = Vec::new();`

`Create` needs an absent log; `TruncateTornTail` needs a torn one. One
open cannot be both, so both are run and the offers are unioned.

## `fn every_open_point_is_offered_in_every_mode_the_frozen_inv…` › `8,`

Create x 2 modes + SyncPrefix x 2 (first open), TruncateTornTail x 2 +
SyncPrefix x 2 (second open).

## `const ERROR_RETURN_CASES: &[(SubEffectPoint, bool)] = &[`

---------------------------------------------------------------------------
The error contract
---------------------------------------------------------------------------

## `const ERROR_RETURN_CASES: &[(SubEffectPoint, bool)] = &[`

The three error-return cases `T-APPEND` names, with the durable shape each
leaves. Written from the packet's own words, not from the funnel's code.

* (e-w) `Written` — "write_all failed after a partial write" → a torn tail.
* (e-u) `WrittenFull` — "write_all succeeded (full line, newline present)
  and flush … returned an error" → the complete line.
* (e-s) `Synced` — "sync_data returned an error after the data reached the
  disk" → the complete line.

## `fn every_error_return_case_leaves_its_tabled_shape_names_it…` › `assert_eq!(`

Two distinct durable shapes across the three cases: a grid that produced
one shape three times would be satisfied by the wrong thing.

## `fn every_error_return_case_leaves_its_tabled_shape_names_it…` › `let mut sites: Vec<(EventSite, Option<TopologyLine>)> = append_site_lines()`

Every append site of the group, not the one that was looked at: the
contract names `AppendFirst`, `Append` and `AppendInformational`
separately, and the legacy site is a fourth behaviour again.

## `fn every_error_return_case_leaves_its_tabled_shape_names_it…` › `let path = log_path(&format!("err-{case}-{index}"));`

**The scratch name carries no point and no site**
(`PR5-EVENTS-045`). It used to be `err-<point>-<site>`, and
`UpstrokeError::EventLog`'s Display renders its path — so
`error.to_string().contains(point.name())` was satisfied by the
*directory name* whatever the message said, and a funnel that
reported a `Synced` injection as `Written` passed this grid.
Verified by running it under exactly that mutation.

## `fn every_error_return_case_leaves_its_tabled_shape_names_it…` › `let quoted = |point: &SubEffectPoint| format!("{}", point.name());`

On the **message**, not on `to_string()`: the rendering adds the
path, and a path is not something the funnel decided to say.

And on the *quoted* name, because `Written` is a prefix of
`WrittenFull`: a bare `contains` cannot tell the two points
apart, and they are the two the packet most needs kept apart —
one is a torn tail the next open truncates, the other a complete
unsynced prefix the barrier makes durable.

## `fn every_error_return_case_leaves_its_tabled_shape_names_it…` › `for attempt in 1..=3 {`

**Every** later append through this handle fails, naming the
poisoning coordinate — the first, the second and the third
(`PR5-EVENTS-042`). `scope` is "every later append fails until
the log is reopened through `Event.OpenLog`", and one attempt is
all a poison that *clears itself on read* has to produce: with
`check_poison` reading through `take()`, the handle silently
became usable again from the second attempt on and this grid
stayed green.

## `fn a_handle_poisoned_at_one_site_names_that_site_when_refused_at_another() {`

A handle poisoned at **one** site refuses a later append at **another**, and
still names the site it was poisoned at (`SUPP-EVENTS-046-site`).

The grid above drives 4 sites × 3 points × 3 later attempts, and in every one
of those 36 cells the later attempt is made at *the same binding* the
poisoning append used. So "the stored site" and "the newly attempted site"
are the same string everywhere, and `assert!(message.contains("`Event.<site>`"))`
is satisfied identically by a `check_poison` that names either. Repair round
2 widened `EventLog::poisoned` from `SubEffectPoint` to `(EventSite,
SubEffectPoint)` precisely so the refusal names both — and the grid catches
the **point** half only. Measured: `self.poisoned.map(|(_, point)|
(attempted, point))` survived the whole suite at 1128 / 0 / 21, byte for byte
the baseline.

This is the fourth time this slice has met the same shape —
`PR5-WORKSPACE-036`, `correlation-never-broken`, the poisoned-handle grid,
and this — so it is worth naming plainly: **two axes covered separately,
their intersection never built**. Coverage on each axis reads as coverage of
the pair, and the mutation that varies only the un-crossed field survives.

Here the second field is the **site**, and what varies it is that a schema-4
handle accepts three append sites onto one `EventLog` — `AppendFirst`,
`Append` and `AppendInformational` — which is the whole reason round 2 said
"half an identification on a handle that accepts three append sites". Held
constant: the point, at `Written`, so the point half cannot be what fails.
A legacy handle cannot build this shape at all (`check_scope` admits only
`LegacyAppend`), which is why it took a schema-4 fixture.

## `fn a_handle_poisoned_at_one_site_names_that_site_when_refus…` › `for (poison_at, poison_line) in &lines {`

Every ordered pair of distinct sites, so no single pairing can be the one
that happens to agree.

## `fn a_value_the_wire_cannot_carry_does_not_enter_the_append_…` › `let path = log_path("unserializable");`

`emit`'s contract is "a FoldError aborts before any write", and the
packet's poisoning rule is about an `Err` "after the append **was
entered**". A handle poisoned by a value that never reached the file
would refuse the next, perfectly good, event.

## `fn a_value_the_wire_cannot_carry_does_not_enter_the_append_…` › `assert!(`

`serde_json` writes a non-finite float as `null` rather than refusing, so
the guard that catches it is the round-trip — which is precisely the step
`emit` names ("serialize -> round-trip -> plan_transition -> append").

## `fn every_append_point_is_offered_in_every_mode_the_frozen_i…` › `for (site, line) in append_site_lines() {`

All three sites declare the same points, and all three are driven. A
suppression keyed on one site — `if site == EventSite::Append` around the
consults — is the `PR4-CONF-002` defect, and it passes a grid that drives
only the site somebody happened to look at.

## `fn every_append_point_is_offered_in_every_mode_the_frozen_i…` › `5,`

Written x 2, WrittenFull x 1 (error-return only), Synced x 2.

## `fn the_written_kill_shape_moves_where_a_kill_lands_and_not_…` › `let mut bytes = Vec::new();`

The observer that asks for the torn coordinate must not change the file a
successful append leaves, or every ST-07 kill measurement would be taken
against a writer production does not have.

## `struct KillAt {`

---------------------------------------------------------------------------
Kill injection
---------------------------------------------------------------------------

## `struct KillAt {`

Answers `Kill` at exactly one coordinate, and asks for one of the two
durable shapes `Written`'s kill entry tables.

## `const KILL_CASES: &[(&str, SubEffectPoint, WrittenShape)] = &[`

Every kill coordinate the frozen inventory gives this funnel, by the name
the parent passes down.

Transcribed from `fault_injection_registry.structure`: "Event sites carry
kill entries for `Written` (torn …; complete-unsynced …) and `Synced` …, and
`Event.OpenLog` carries `Create`, `TruncateTornTail`, and `SyncPrefix`
entries (`SyncPrefix` in kill and error-return modes …)". Six cells over five
points, because `Written`'s one kill entry tables two durable shapes.

## `("create", SubEffectPoint::Create, WrittenShape::Complete),`

"create the log if absent and fsync its directory"

## `(`

"an unterminated final line was truncated before the append handle"

## `(`

"a kill before it … leaves the prefix possibly non-durable … and the next
open repeats the barrier"

## `("written-torn", SubEffectPoint::Written, WrittenShape::Torn),`

"torn: truncated on the next open, previous prefix"

## `(`

"complete-unsynced: either prefix"

## `("synced", SubEffectPoint::Synced, WrittenShape::Complete),`

The synced line. Same bytes as the complete-unsynced one, deliberately.

## `fn declared_kill_points() -> BTreeSet<SubEffectPoint> {`

The kill coordinates the frozen inventory declares, derived from the types.

`EventSite::ALL` x `sub_effects()` x `modes()`, keeping the points that
declare `Kill`. A point added to the inventory later is uncovered loudly.

## `fn kill_at(case: &str, point: SubEffectPoint, path: &Path) -> Vec<u8> {`

Run the helper for one case and hand back what the killed process left.

## `fn kill_at(case: &str, point: SubEffectPoint, path: &Path) …` › `let helper = format!(`

The harness names a test by its module path without the crate, so the
filter is derived rather than written out: a module that moves takes
this with it instead of silently matching nothing.

## `fn event_funnel_kill_helper() {`

The child half of the kill tests.

A kill is `std::process::abort` for the reason [`crate::agent::proc`] gives:
the claim under test is what a process that dies **without running any
cleanup** leaves durable, and both `panic!` and `exit` run destructors.

## `fn event_funnel_kill_helper()` › `let _ = EventLog::open_hooked(EventSite::OpenLog, &path, &mut warnings, &mut kill);`

The three open points fire inside `Event.OpenLog` itself; there is no
append in these cases and no handle to append through.

## `fn event_funnel_kill_helper()` › `std::process::exit(0);`

Reached only if the kill did not fire, which the parent detects as a
successful exit.

## `fn every_kill_point_the_inventory_declares_has_a_case_and_n…` › `let declared = declared_kill_points();`

Derived from the types: `EventSite::ALL` x `sub_effects()` x `modes()`.

## `fn a_kill_at_each_open_point_leaves_the_shape_the_packet_tables() {`

`Event.OpenLog`'s three kill entries, each executed by a real abort.

The claims are the inventory's own: `Create` is "create the log if absent and
fsync its directory"; `TruncateTornTail` is "an unterminated final line was
truncated **before the append handle was taken**"; and for `SyncPrefix`,
"a kill before it … leaves the prefix possibly non-durable, no fold-derived
effect is performed, the command refuses resumably, and **the next open
repeats the barrier**".

## `fn a_kill_at_each_open_point_leaves_the_shape_the_packet_ta…` › `let created = log_path("kill-create");`

`Create` — the log was absent, so nothing seeds it.

## `fn a_kill_at_each_open_point_leaves_the_shape_the_packet_ta…` › `let truncated = log_path("kill-truncate-torn-tail");`

`TruncateTornTail` — the truncation is already durable at the point.

## `fn a_kill_at_each_open_point_leaves_the_shape_the_packet_ta…` › `let unsynced = log_path("kill-sync-prefix");`

`SyncPrefix` — consulted before the sync, so the bytes are untouched and
the next open is what makes them durable.

## `fn a_kill_at_each_append_point_leaves_the_shape_the_packet_tables() {`

`Event.Append`'s kill entries: the two durable shapes `Written` tables and
the synced line `Synced` tables, each executed by a real abort, and each
followed by what the next open makes of it.

## `fn a_kill_at_each_append_point_leaves_the_shape_the_packet_…` › `let mut warnings = Vec::new();`

What the next open makes of it — the other half of the tabled entry.

## `fn a_kill_at_each_append_point_leaves_the_shape_the_packet_…` › `assert_eq!(durable.len(), 3);`

Two shapes across three coordinates: the complete-unsynced line a kill at
`Written` leaves and the synced line a kill at `Synced` leaves are the
same bytes, which is why `WrittenFull` declares no kill mode at all.

## `fn a_fresh_log_establishes_the_barrier_trivially_and_hands_out_a_handle() {`

---------------------------------------------------------------------------
The stable-prefix barrier
---------------------------------------------------------------------------

## `fn a_fresh_log_establishes_the_barrier_trivially_and_hands_…` › `let path = log_path("barrier-fresh");`

"a fresh run's Event.OpenLog at P5 creates an empty log, so the barrier is
trivially established (no prefix)".

## `fn the_barrier_syncs_before_it_rereads_and_proves_before_it…` › `assert_eq!(witness.ledger.len(), 2);`

The sync happened inside `OpenLog`, i.e. before `ProvePrefixStable` began.
The file's barrier and the directory's, on every platform (`PR5-CONF-013`).

## `fn an_unstable_reread_refuses_naming_prove_prefix_stable_an…` › `let committed = b"{\"ts\":\"2026-08-20T09:41:02Z\",\"event\":\"defer_wait_elapsed\",\"dat…`

Three independent ways a reread can be unstable: a byte moved, the length
moved, and the boundary moved. `stable_prefix_barrier` step (4) names all
three, so each is a cell rather than one test that happens to trip.

## `fn an_unstable_reread_refuses_naming_prove_prefix_stable_an…` › `let cases: &[(&str, Vec<u8>, &str)] = &[`

Three cells, three *different clauses* of step (4). The order the proof
checks them in is what makes that possible: byte-equality implies the
other two, so it is checked last.

## `fn an_unstable_reread_refuses_naming_prove_prefix_stable_an…` › `let details: BTreeSet<String> = cases`

Each cell produced a *distinct* detail, so a proof that had collapsed the
three clauses into one would fail here rather than pass three times.

## `fn checked_replay_consumes_exactly_the_reread_bytes()` › `let path = log_path("replay-exact-bytes");`

The sharp form: the file is replaced with bytes the replay would refuse,
*after* the stability proof. An implementation that read the file a third
time refuses; one that replays what it proved does not.

## `fn invalid_terminated_line_refused_not_repaired() {`

`T-APPEND`'s `refusal_condition`: "a newline-terminated invalid line
(rewritten log)", and its resume action: "a newline-terminated invalid line
anywhere is corruption and refuses (**never repaired**)".

Named as `transaction_fault_matrix` names it. The second half is a claim
about the bytes, not about the error, so the bytes are what is asserted.

## `fn invalid_terminated_line_refused_not_repaired()` › `assert_eq!(`

"never repaired": the barrier syncs the prefix it found and refuses. It
does not truncate the invalid line, rewrite it, or move it aside — a
repair would turn corruption into a confident wrong answer, which is the
whole reason this refuses rather than recovers.

## `fn the_parsed_events_really_reach_the_checked_fold()` › `let path = log_path("replay-reached");`

A valid schema-4 line that the *fold* refuses: it parses, so `parse_log`
is not the refuser, and the refusal can only come from `replay` having
been handed the events. A barrier that replayed an empty slice would
succeed here.

## `fn a_first_line_digest_that_disagrees_with_the_commit_recor…` › `let expected = {`

Computed here from the line's own bytes rather than by calling the
function again: an oracle that called the function under test would move
with it.

## `fn a_first_line_digest_that_disagrees_with_the_commit_recor…` › `let empty = log_path("first-line-digest-empty");`

An empty log with a commit record that names a first line is the other
half of the same clause.

## `fn every_barrier_step_is_reachable_and_named()` › `assert_eq!(BarrierStep::ALL.len(), 4);`

The enum is the list; a step added later has no test and says so.

## `fn every_barrier_step_is_reachable_and_named()` › `let missing = scratch("barrier-open-fails")`

`OpenLog` is the one step the tests above do not produce, because it is
the ordinary I/O failure: a log whose directory does not exist.

## `fn digest_of(bytes: &[u8]) -> String {`

---------------------------------------------------------------------------
T-APPEND: the kill rows, and the two shapes only a raw mutation can build
---------------------------------------------------------------------------

`T-APPEND`'s `boundary` splits into kill cases — (w) bytes partially written,
(u) the full line written but not yet synced, (s) synced — and error-return
cases. The error-return halves belong to the emit path and live with it in
`src/engine/topology/emit.rs`, which as a `TOPOLOGY_MODULE` may name neither
`std::process::Command` nor a raw `std::fs` mutation. The kill halves need
both: a kill is `std::process::abort` and its claim is what a process that
runs **no** cleanup leaves durable, and "a line lost before the barrier"
needs the loss to be real. So they are here, beside the kill apparatus this
file already has, and they reuse [`kill_at`] rather than growing a second
one.

**What "recovery matches the row" is measured as.** These logs are
`defer_wait_elapsed` lines, which the checked fold refuses before a
`run_started` — so the assertion is over [`TopologyFold::parse_log`], the
barrier's own step-(5) parser, and over the *whole event vector* rather than
any projection of it. That is what recovery consumes, so equality of vectors
is stronger than equality of anything derived from them, not weaker.

## `fn digest_of(bytes: &[u8]) -> String {`

Hash rather than length: a one-character edit does not move a length, and
"the surviving prefix is the before-append one" is a claim about bytes.

## `fn seeded_prefix(tag: &str) -> (PathBuf, Vec<u8>, Vec<TopologyEvent>) {`

A durable before-append prefix, and the events it replays to.

`topology_line(7)` rather than `topology_line(1)`: the kill helper appends
round 1, so a seed of round 1 would make "the line that survived" and "the
line that was already there" the same bytes, and every assertion below
would hold for the wrong reason.

## `fn after_append_events(before: &[TopologyEvent]) -> Vec<TopologyEvent> {`

What the log would hold if the killed append had committed.

## `fn torn_tail_truncated_on_open_and_recovery_matches_before_append_row() {`

T-APPEND (w): "bytes partially written (torn tail: no terminating
newline)". `durable_state` is "the previous prefix (an unterminated final
line is not an event: the newline is the commit marker)", and
`resume_action` is "`Event.OpenLog` truncates an unterminated final line
before taking the append handle … only then does recovery follow the fault
row of the surviving prefix (before-append …)".

## `fn torn_tail_truncated_on_open_and_recovery_matches_before_…` › `let killed = kill_at("written-torn", SubEffectPoint::Written, &path);`

A real process death inside the write, in the torn half of `Written`'s
kill entry.

## `fn torn_tail_truncated_on_open_and_recovery_matches_before_…` › `let mut warnings = Vec::new();`

The open normalizes it, before it takes the append handle.

## `fn torn_tail_truncated_on_open_and_recovery_matches_before_…` › `assert_ne!(before_events, after_append_events(&before_events));`

And the two rows really differ, so the assertion above is a choice.

## `fn unsynced_line_recovery_matches_whichever_prefix_survived() {`

T-APPEND (u): "the full line written but not yet synced". `durable_state`
is "**either** the previous prefix or the prefix incl. the line, decided by
what survives at the next open", and `authoritative_state` is "exactly the
durable prefix; the adjacent effect's fault row applies to **whichever
prefix survived**".

So the assertion is deliberately a disjunction *plus* a statement of which
arm this machine produced. A test that asserted only the arm would be
asserting a durability guarantee the packet does not make; one that asserted
only the disjunction would pass for a log that lost the whole prefix.

## `fn synced_line_recovery_matches_after_append_row() {`

T-APPEND (s): "synced". `durable_state` is "the prefix incl. the line" with
no disjunction, which is the whole difference from (u) — and the reason
this is a separate test rather than a second cell of the one above.

## `fn unsynced_line_made_durable_by_barrier_survives_later_power_loss() {`

`stable_prefix_barrier`: "a line an earlier process wrote but never synced
or whose sync reported failure" is made durable by the barrier's own
`SyncPrefix`, and thereafter "a line the barrier synced **cannot be reverted
by a later loss**".

Two crashes, which is what makes the claim more than a restatement of the
previous test: the first leaves the line unsynced, the barrier syncs it, and
the second crash — a torn write on top of it — does not take it away.

## `fn unsynced_line_made_durable_by_barrier_survives_later_pow…` › `let unsynced = kill_at("written-complete", SubEffectPoint::Written, &path);`

Crash one: the complete-unsynced shape.

## `fn unsynced_line_made_durable_by_barrier_survives_later_pow…` › `let mut witness = Witness::default().recording_durability();`

The barrier's step (2): the whole surviving prefix — including that
line — is successfully synced by the reopening process.

## `fn unsynced_line_made_durable_by_barrier_survives_later_pow…` › `let torn = kill_at("written-torn", SubEffectPoint::Written, &path);`

Crash two, on top of the now-durable prefix: a torn write that the next
open truncates away.

## `fn unsynced_line_lost_before_barrier_converges_to_before_append_order() {`

`stable_prefix_barrier`: "the barrier makes no claim about lines lost before
it — a line no effect ever depended on may still be lost and then converges
to the before-append order of its fault row **precisely because nothing
acted on it**".

The loss has to be real, which is why this test is here rather than beside
the emit path: nothing in any funnel removes a committed line, so the only
way to model a power loss that drops an unsynced write is to truncate the
file directly. It happens **before** any open, so no process ever saw the
line, let alone acted on it.

## `fn unsynced_line_lost_before_barrier_converges_to_before_ap…` › `fs::OpenOptions::new()`

The loss: the unsynced tail is gone, and no process has opened the log
since the crash — so nothing has been authorized by that line, which is
the premise the convergence rests on.

## `fn unsynced_line_lost_before_barrier_converges_to_before_ap…` › `let (kept_path, _, kept_before) = seeded_prefix("unsynced-kept");`

The control. The identical fixture, with the loss removed, converges to
the *other* row — so the assertion above is about the loss and not about
a fixture that could only ever produce one answer.

## `fn unstable_reread_after_open_sync_refuses_resumably() {`

`stable_prefix_barrier`: an unstable reread "performs none of those effects:
the write command ends … **the run is NoRunFinished and resumable, and the
next resume re-establishes the barrier from (a0)**".

`an_unstable_reread_refuses_naming_prove_prefix_stable_and_hands_out_no_handle`
covers the three clauses of the proof and the naming. What is asserted here
is the other half of the same sentence, and it is the half a refusal that
"repaired" the log would fail: nothing was done, and the next barrier over
the same path holds.

## `fn unstable_reread_after_open_sync_refuses_resumably()` › `let mut rewriter = Rewrite::after_sync(&path, b"");`

The reread differs from the prefix synced at open, which is the only
thing the proof is about.

## `fn unstable_reread_after_open_sync_refuses_resumably()` › `assert_eq!(`

Nothing was done *by the refusal*: the log is exactly what the rewrite
left, neither restored nor repaired.

## `fn unstable_reread_after_open_sync_refuses_resumably()` › `let mut prefix =`

Resumable. The next barrier over the same path holds and hands out both
halves — which is what "the next resume re-establishes the barrier"
means, and what a refusal that had left a half-truncated file would not
allow.

## `fn unstable_reread_after_open_sync_refuses_resumably()` › `let untouched = log_path("unstable-resumable-control");`

The control. The identical bytes, with the rewrite removed, get *past*
step (4) and refuse at step (5) instead — these fixture lines need a
`run_started` the frozen inputs do not describe. So the refusal above was
caused by the instability rather than by anything the fixture would have
refused anyway, which is the one thing a single-cell refusal test cannot
otherwise tell.

## `fn the_event_log_is_written_in_exactly_one_module() {`

---------------------------------------------------------------------------
Structure
---------------------------------------------------------------------------

## `fn the_event_log_is_written_in_exactly_one_module() {`

Every module that may write to an event log, and how many places in it do.

`mechanism` (3): "the raw writer they wrap is reachable only inside
`src/events/log.rs`". This is that sentence as a count. It is a source census
and therefore carries `PR4-CENSUS-COMMENT-ORACLE`'s hazard, which is handled
rather than tripped over: comments are stripped first, and the strip is
asserted to have removed something, because this file's own prose names every
primitive it counts.

## `fn the_event_log_is_written_in_exactly_one_module()` › `const PRIMITIVES: &[&str] = &["write_all(", "sync_data(", "set_len(", "OpenOptions::new()…`

`sync_all(` is deliberately absent (`PR5-CONF-012`): the log's *file*
barrier is now `util::fsync_file`, the one call in the funnel modules that
may name the primitive, so requiring it here would require the funnel to
keep a second copy of it. The two halves that replace it are asserted
below — one `util::fsync_file(` and one `util::fsync_dir(` — and
`effects::tests::every_file_durability_barrier_in_a_funnel_module_goes_
through_one_call` is what checks the syscall is still inside them.

## `fn the_event_log_is_written_in_exactly_one_module()` › `assert_eq!(`

The one write path is one `write_all` per shape, and the shapes are the
torn split's two halves plus the whole line: three, and no more.

## `fn the_event_log_is_written_in_exactly_one_module()` › `assert_eq!(`

Both halves of the durability barrier left this module for
`src/util.rs` — the directory's because std cannot make that call on
Windows at all (`PR5-CONF-013`), the file's because a syscall written
beside the ledger entry that certifies it has no oracle
(`PR5-CONF-012`). This census follows them rather than losing sight of
them: still exactly one each, and still in this module's production
region.

## `fn relative_slashed(path: &Path) -> String {`

One scanned path, as `FOLD_MENTIONS` writes it: relative to the manifest
directory and slash-separated, so one literal reads on every platform.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold() {`

The barrier is the **only** path by which a topology write command obtains a
fold from an existing log.

`stable_prefix_barrier` says the checked replay of the proven bytes is what
entitles a write command to act, and a second path — anything that reads the
log and folds it without the sync, the reread and the stability proof — makes
that entitlement a convention rather than a mechanism. This is that sentence
as a count over the whole crate.

Four hazards are handled rather than tripped over, and each has an assertion
of its own in the body rather than a sentence here — a docstring that claims
a control is not a control, which is what this paragraph used to be.

* `PR4-CENSUS-COMMENT-ORACLE`. The regions come from
  [`crate::effects::production_code`], which blanks comments **and string
  literals**, and `the_blanking_this_census_depends_on_is_live` below asserts
  that the blanking removed prose naming the very token counted here. The
  `//`-only strip this census used before saw neither a `/* … */` nor a
  `const CFG_TEST_ATTR: &str = "#[cfg(test)]";`, and either one collapsed a
  whole production file's region to nothing.
* **A region that stops at `#[cfg(test)] mod tests;`.** The `tests.rs`
  entries of `effects::tests::cfg::WHOLE_FILE_TEST_MODULES` declare
  their tests that way, and everything below such a declaration is legal
  production code that a truncating region cannot see.
  `production_code` removes the item and keeps the rest of the file.
* **A skip derived from prose.** The whole-file test modules are read out of
  the **blanked** source, and every declaration is asserted to resolve to a
  file that exists: a `// … #[cfg(test)] mod policy;` in a comment otherwise
  derives a skip for a real production module.
* **A scan that collapses.** The control below asserts the scan really did
  reach the files that mention the fold at all, and a floor on the total
  non-whitespace bytes scanned catches the case where every region is empty
  and the file count alone still passes.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `const FOLD_MENTIONS: &[&str] = &[`

The control: every production region that **names** the fold at all.

A list rather than a count, and the reason is a merge hazard rather than
a readability one. As a count, each module that starts naming the fold
bumped the same number — so two independent changes that each add one
module both write the same new value, the merge takes it once, and the
census silently ends up expecting fewer regions than it scans. That is
wrong in the direction that **weakens** the control: a scan whose regions
collapsed would then be indistinguishable from a correct one, and its
zero counts below would prove nothing. A list merges additively, and when
it does disagree it names the file.

Slash-separated and relative to the manifest directory, so the literals
read the same on Windows, where the walk produces `\`.

Sorted, asserted sorted, and asserted duplicate-free: an entry appended in
the wrong place, or twice by two merges, fails here rather than passing.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `"src/engine/topology/candidate.rs",`

PR7's candidate pipeline. Holds a fold, builds none from bytes.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `"src/engine/topology/create.rs",`

PR7's schema-4 creator. It holds a fold across P5b and P6 because
`emit` puts `plan_transition` before the commit record and
`apply_delta` after the append.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `"src/engine/topology/emit.rs",`

PR7's emit path. It holds a fold and appends to a log and builds
neither from bytes — it obtains one from `establish_stable_prefix` —
so it names the type without adding to `callers` below.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `"src/engine/topology/recover.rs",`

This funnel: `establish_stable_prefix` is the one place a log becomes
a fold.
PR7's selection and settlement halves. Both read a fold; neither
builds one from bytes.
PR7's recovery order. Reads a fold; builds none from bytes.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `"src/engine/topology/run.rs",`

PR7's driver. Holds the fold `RunHandle` handed it and reads it to
select; builds none from bytes, because the one it holds is the one
the barrier proved and a second derivation would be a rule that can
disagree with the first.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `"src/topology/census.rs",`

ST-14's bounded reachability census over fold states.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `"src/topology/fold.rs",`

The fold itself: the root holds the state types, the
`plan_transition`/`apply_delta` pair and the child declarations, and the
children listed below carry an `impl TopologyFold` block of their own.
Every other child names the type in prose only, if at all, and prose is
blanked — so it does not appear here.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `let test_modules: BTreeSet<PathBuf> =`

Whole files the crate declares under `#[cfg(test)]` are test code with no
production half at all, and treating them as production would count a
fixture as a second path. The set is read out of the declarations rather
than guessed from a filename convention — and out of the **blanked**
source, because a declaration written inside a comment is prose. Measured
on this tree before the repair: the raw split derived 50 skip paths of
which 34 named no file at all, and one `//` line was enough to remove a
real production module from this census's domain.
Through the shared resolver. This loop stood here, in `runner::tests`
and — as a *third*, different rule — in `recover::tests`, which keyed on
the file name and so covered only the whole-file test modules named
`tests.rs`, not the rest of them. `PR7-R5-ATT-001`.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `let production = crate::effects::production_code(&source);`

The whole file, comments and string literals blanked and every
`#[cfg(test)]` item removed — not a truncation at the first one.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `assert!(`

Per file, because the aggregate floor below cannot see one region
collapsing: it stands at 750,000 against an actual over 900,000, so a
file may empty itself and the sum still clears the bar. A region that
is empty answers "nobody calls the fold" for that file no matter what
the file contains.

**Necessary, not sufficient.** It sees a region that collapses, not
one that is replaced: `PR7-R2C-CHAR-LITERAL-DESYNC`'s refined form
removes the forged lines and adds a probe of the same size, and
measured a zero-byte delta. `effects::char_literal_end` and
`configured_item_end`'s give-up direction are what close that.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `assert!(`

And the regions were not empty. A file count alone passes with every
region collapsed to nothing, which is precisely what a `#[cfg(test)]` in a
comment used to do to one.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `let funnel_code =`

And that one place is inside the barrier, not merely inside this file.

The slice ends at the barrier's own closing brace. It used to run to end
of file, which also covered `read_all`, `read_bytes`, `parse_bytes` and
`impl LogTail` — so the parse could be lifted into a private helper
*below* the barrier and this check would still count one. Measured: it
did, and the census stayed green.

## `fn the_stable_prefix_barrier_is_the_only_way_a_log_becomes_…` › `for below in ["pub fn read_all(", "impl LogTail {"] {`

The bound is real: the readers below the barrier are outside it.

## `fn the_blanking_this_census_depends_on_is_live() {`

The blanking every source census in this file depends on is doing something.

`PR4-CENSUS-COMMENT-ORACLE` in its live form: this tree's prose names
`#[cfg(test)]` 104 times outside code and `TopologyFold` far more often than
its code does, so a blanking that silently stopped working would not make the
censuses fail — it would make them count prose and pass.

## `fn fn_body(source: &str, header: &str) -> String {`

The body of the item whose declaration begins with `header`, brace-matched.

A suffix slice to end of file is not a function: it is the function plus
everything the file happens to declare after it.

## `fn production_region(source: &str) -> &str {`

Everything before the `#[cfg(test)]` submodules.

## `fn strip_comments(source: &str) -> String {`

Remove `//` line comments.

Enough for its two remaining callers — `the_event_log_is_written_in_exactly_
one_module` and `the_legacy_engine_reports_and_stops_on_a_returned_append_
error` — which count primitives no string literal in those two files spells,
and which assert that this strip shortened the text before counting anything.
It is **not** enough for a census over the whole tree: it sees neither a
`/* … */` nor a string literal, and either can carry a `#[cfg(test)]` that
collapses a file's region. Whole-tree censuses use
[`crate::effects::production_code`].

## `struct BuildRefusal {`

---------------------------------------------------------------------------
The build refusal
---------------------------------------------------------------------------

## `struct BuildRefusal {`

One `compile_fail` fixture lifted out of this module's own doc comments.

## `struct BuildRefusal` › `code: String,`

The `EXXXX` the fence declares.

## `struct BuildRefusal` › `line: usize,`

Where in `src/events/log.rs` the fence opens, for a failure message.

## `struct BuildRefusal` › `body: String,`

The block's Rust, doc-comment prefixes removed.

## `fn declared_build_refusals() -> Vec<BuildRefusal> {`

Every ```` ```compile_fail,EXXXX ```` block in `src/events/log.rs`.

The fixtures are read out of the doc comments rather than copied, so the
executed test and the documented one cannot drift: there is one text.

## `fn crate_under_test() -> (PathBuf, PathBuf) {`

The `--extern` this crate's own rlib is reachable by, and the directory its
dependencies are in.

The test binary lives in `<target>/debug/deps` beside the rlib cargo built
from the same sources, so both are found from `current_exe` rather than from
a guessed target directory — `CARGO_TARGET_DIR` is set by the build wrapper
this project uses and is not `target/`.

## `fn typecheck(dir: &Path, name: &str, body: &str) -> (bool, String) {`

Type-check `body` against this crate and return rustc's diagnostics.

## `fn typecheck(dir: &Path, name: &str, body: &str) -> (bool, …` › `fs::write(&source, format!("fn main() {{\n{body}\n}}\n")).expect("the fixture");`

Doctests without a `fn main` are wrapped in one, so the fixtures are
written that way and this wraps them the same.

## `fn error_codes(stderr: &str) -> BTreeSet<String> {`

The distinct `error[EXXXX]` codes in a rustc diagnostic stream.

## `fn every_declared_build_refusal_fails_for_the_reason_it_declares() {`

`expected_failures_refusals`: "a schema-4 append outside the Event funnel
does not compile", proven by a test the project's own gate runs.

The three fixtures that carry this claim are `compile_fail` doctests, and
**`cargo test --all-targets` does not run doctests** — `--all-targets` is
`--lib --bins --tests --benches --examples`, and the doc target is not in it.
CI runs exactly that command, so as documentation-only fixtures they were
green because they never executed at all: the strongest form of the failure
this slice's contract warns about ("a fixture asserting *this does not build*
is green whether it failed for the intended reason or a typo").

So the blocks are read out of the doc comments and compiled here. Three
things are asserted that a bare "it did not build" cannot:

* the **positive control** compiles, so a mis-wired `--extern` cannot make
  every fixture "refuse" for want of a crate to refuse against;
* each fixture emits **exactly** its declared error code and no other, so a
  typo — which lands on `E0425`, `E0432`, `E0599` — fails this test;
* the **count** is pinned, so a deleted fixture is loud.

### The one boundary, stated rather than hidden

The fixtures are compiled against the rlib cargo built beside this test
binary, so they see the crate as an external consumer does. Under the gate
command that rlib is always current — `--all-targets` builds the `upstroke`
binary, which links it. Under a bare `cargo test --lib` after a visibility
change, cargo has no reason to rebuild the rlib, and a fixture could then
refuse against yesterday's API. That is not guarded here on purpose: every
guard available for it is a timestamp comparison, and a test binary that is
legitimately newer than an unchanged rlib is the *ordinary* case, so the
guard would be a flake rather than a check. The gate is unaffected.

## `fn every_declared_build_refusal_fails_for_the_reason_it_dec…` › `let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"…`

The harness compiles at one edition; a crate that moved would be checked
under rules it is not built with.

## `fn every_declared_build_refusal_fails_for_the_reason_it_dec…` › `let (control_ok, control_stderr) = typecheck(`

The control, and it earns its keep twice.

(1) If it does not compile, nothing below is evidence: every fixture would
    "refuse" for want of a reachable crate.
(2) It is compiled as an **external consumer**, against the rlib and
    nothing else, so it is also this slice's proof of `scope`'s "public
    path `crate::events::EventLog` **unchanged**" — together with
    `read_all` and `LogTail`, the other two names `src/events/mod.rs`
    re-exports. An in-crate `use` could not prove that: the module's own
    callers would compile against `crate::events::log::EventLog` just as
    happily.

## `fn the_legacy_engine_reports_and_stops_on_a_returned_append_error() {`

The legacy engine's handling of a returned append error is unchanged, and
the thing that makes that true is that it does not append again.

This is a census, and it used to be *all there was*. Its own boundary note
said "no test can make one of its appends fail without plumbing hooks
through `engine::Harness` — which is another lane's file this slice does not
touch", and that boundary is what `PR5-CONF-010` and `PR5-CONF-011` were:
with no way to fail a live run's append, `Run::emit`'s `?` could be replaced
by a warning-and-`Ok`, and `drain_and_report`'s partial report could be
deleted, and both survived the whole suite. The hooks are plumbed now —
`RunOptions::log_hooks`, `NoEventHooks` in production — and the behaviour is
held by `engine::tests::a_returned_legacy_append_error_stops_the_run` and
`…_still_leaves_the_partial_report`.

What is left here is what a behavioural test cannot see: that the branch
emits **nothing**, over the whole branch rather than over the paths a
fixture happens to drive.

## `fn the_legacy_engine_reports_and_stops_on_a_returned_append…` › `let squeezed: String = code.chars().filter(|c| !c.is_whitespace()).collect();`

Whitespace out before counting a call, because rustfmt decides where a
method chain breaks and a census that a reformat can silently zero is a
census that reports "clean" for the wrong reason. (Measured: it did —
`chain_width` split this very call across three lines.)
