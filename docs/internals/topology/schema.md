# `src/topology/schema.rs`

Extended notes for [`src/topology/schema.rs`](../../../src/topology/schema.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Which reader a log gets, decided before a single event is folded (INV-03).

Schemas 1–3 are legacy sequential runs; schema 4 is the parallel execution
topology. They are different execution models sharing a file name, and
**there is no upgrade between them** — a run is one or the other for its
whole life ([`check_upgrade_transition`]).

That makes the choice of reader a header decision rather than a fold
decision. [`probe_header`] reads exactly the first newline-terminated line
of `events.jsonl` and nothing else, because that line is the only one whose
meaning is fixed before the schema is known: `run_started` is always first,
and it always carries the schema the rest of the file is written in. A
reader that instead learned the schema while folding would already have
interpreted events under the wrong model by the time it found out.

Two boundaries are enforced here and stated plainly to whoever hits them:

* **The newline is the commit marker.** An unterminated first line is a
  torn write, not a header. Nothing is committed, so nothing is read —
  which is the same rule `EventLog::open` and every whole-log reader in
  [`crate::events`] already apply to the tail, applied to the head.
* **A schema above the ceiling refuses explicitly.** Already-released
  schema-3 binaries refuse a schema-4 log too, but generically — they fail
  on a record they cannot deserialize, and the operator is told the line is
  invalid. From this binary onwards the refusal names the schema, the
  ceiling, and the fact that no upgrade path exists, because "your log is
  corrupt" is the wrong thing to tell someone whose log is merely newer.

### Activation

[`MAX_READABLE_SCHEMA`] is *derived* from [`TOPOLOGY_ACTIVATION`], not
written down. Production is [`TopologyActivation::Inactive`], so the ceiling
is 3 and a schema-4 log is refused by a released binary rather than folded
by a reader that does not exist yet. Activation is a one-token change in a
later slice; every decision that depends on the ceiling already reads it
through [`select_reader_with`], so nothing has to be rewritten when it moves.

## `pub const LATEST_LEGACY_SCHEMA: u32 = 3;`

The last schema a legacy sequential run can be written in.

## `pub const TOPOLOGY_SCHEMA: u32 = LATEST_LEGACY_SCHEMA + 1;`

The schema a parallel-topology run is written in. Adjacent to
[`LATEST_LEGACY_SCHEMA`] and reachable only by starting a fresh run.

## `pub enum TopologyActivation {`

Whether this binary's topology reader is switched on.

A separate type rather than a `bool` so that the two states are named at
every use site: `max_readable_schema(false)` says nothing about which side
of the switch is production.

## `pub enum TopologyActivation` › `Inactive,`

No topology reader is wired up; schema-4 logs are refused explicitly.

## `pub enum TopologyActivation` › `Active,`

The topology reader is wired up and schema-4 logs are read.

## `pub const TOPOLOGY_ACTIVATION: TopologyActivation = TopologyActivation::Inactive;`

This binary's activation state. **Production is [`TopologyActivation::Inactive`].**

## `pub const fn max_readable_schema(activation: TopologyActivation) -> u32 {`

The highest schema a binary at `activation` may interpret.

The ceiling is the *activation* expressed as a number: an inactive binary
stops at the legacy schema, an active one reads the topology too. Nothing
in between is meaningful, which is why this is a match and not arithmetic.

## `pub const MAX_READABLE_SCHEMA: u32 = max_readable_schema(TOPOLOGY_ACTIVATION);`

This binary's ceiling, derived from [`TOPOLOGY_ACTIVATION`].

## `const _: () = assert!(matches!(TOPOLOGY_ACTIVATION, TopologyActivation::Inactive));`

The slice's invariant — production reads to schema 3 and no further — held
where a test cannot hold it. Every assertion in the `tests` module below
compiles only under `cfg(test)`, so an activation that is `Inactive` for the
test build and `Active` for the released one satisfies the entire suite while
shipping a binary that folds schema-4 logs through a reader this slice does
not have. These are evaluated in the ordinary build too — the one `src/main.rs`
links — so that shape fails to compile rather than shipping.

## `pub enum WriterSelector {`

Which schema a *fresh* run is written in.

Separate from the read ceiling on purpose. Reading is about what a binary
can be handed; writing is about what it chooses to create, and a binary that
could read schema 4 still writes schema 3 for every ordinary run until the
topology is what `upstroke run` means.

## `pub enum WriterSelector` › `Production,`

What `upstroke run` creates.

## `pub enum WriterSelector` › `TopologyPreview,`

What a deliberate topology preview creates.

## `pub const fn fresh_writer_schema(selector: WriterSelector) -> u32 {`

The schema `selector` writes into a fresh `run_started`.

## `pub struct LogHeader {`

---------------------------------------------------------------------------
Header probe
---------------------------------------------------------------------------

## `pub struct LogHeader {`

What the first committed line of a log says about the rest of it.

## `pub struct LogHeader` › `pub event: String,`

The event tag on line 1. Always `run_started` in an accepted header —
carried anyway so a caller can report what it found instead.

## `pub struct LogHeader` › `pub schema: u32,`

The schema the rest of the file is written in.

## `pub enum ReaderSelection {`

Which fold a log's bytes belong to.

## `pub enum ReaderSelection` › `Legacy {`

A sequential run: [`crate::events::RunState::apply`], unchanged.

## `pub enum ReaderSelection` › `schema: u32,`

The exact legacy schema, which still decides legacy-side behaviour.

## `pub enum ReaderSelection` › `Topology,`

A parallel-topology run: the checked topology fold.

## `struct ProbeLine {`

The minimum shape a header must have. Deliberately not the real
`run_started` payload of either schema: the probe's whole job is to run
*before* either payload type is chosen, so it must not be able to fail for
a reason that belongs to one of them.

## `const RUN_STARTED: &str = "run_started";`

The tag every log's first committed line must carry.

## `pub fn probe_header(bytes: &[u8]) -> Result<LogHeader, SchemaRefusal> {`

Read the header off the first newline-terminated line of `bytes`.

Everything after that line is ignored — including whether it parses at all.
A reader that refused here on a damaged line 7 would be refusing for a
reason the *fold* is entitled to state precisely, having read the file.

### Errors

[`SchemaRefusal::NoCommittedHeader`] when no line is newline-terminated,
[`SchemaRefusal::FirstLineUnreadable`] when line 1 is not a JSON event
envelope, [`SchemaRefusal::RunStartedNotFirst`] when it is some other
event, and [`SchemaRefusal::HeaderWithoutSchema`] when it records no schema.

## `pub fn probe_header(bytes: &[u8]) -> Result<LogHeader, Sche…` › `let end = bytes`

The newline is the commit marker (see `crate::events::parse_bytes`): a
syntactically complete first line without one was never committed, so
there is no header to read even though the bytes look like one.

## `pub fn select_for_schema(schema: u32, ceiling: u32) -> Result<ReaderSelection, SchemaRefu…`

Which reader a log written in `schema` gets from a binary whose ceiling is
`ceiling`.

Pure, and separate from [`probe_header`] so the ceiling is an argument
rather than a constant every caller silently inherits. That is what lets
the post-activation ceiling be exercised without moving production's.

### Errors

[`SchemaRefusal::TopologyLogUnreadable`] for a topology log this binary is
not activated for, and [`SchemaRefusal::NewerThanReadable`] for anything
above the topology schema.

## `pub fn select_for_schema(schema: u32, ceiling: u32) -> Resu…` › `if schema == TOPOLOGY_SCHEMA {`

Two refusals, because they are two different situations for the
person reading them: one is fixed by upgrading upstroke, the other by
upgrading upstroke *and* knowing that this log will never be a legacy
run no matter what reads it.

## `pub fn select_reader_with(bytes: &[u8], ceiling: u32) -> Result<ReaderSelection, SchemaRe…`

Probe `bytes` and choose a reader against an explicit `ceiling`.

### Errors

Every [`SchemaRefusal`] [`probe_header`] and [`select_for_schema`] produce.

## `pub fn select_reader(bytes: &[u8]) -> Result<ReaderSelection, SchemaRefusal> {`

Probe `bytes` and choose a reader against this binary's ceiling.

### Errors

Every [`SchemaRefusal`] [`select_reader_with`] produces.

## `pub fn check_upgrade_transition(from: u32, to: u32) -> Result<(), SchemaRefusal> {`

Whether a legacy `run_schema_upgraded` transition may be applied.

**No run upgrades into the topology** (INV-03). The schemas are different
execution models, not successive versions of one: a schema-3 log records a
sequential run whose tasks committed to a branch, and reinterpreting it as
a topology run would invent a merge queue, a candidate for every commit,
and a runner identity nobody resolved. The way to get a topology run is to
start one.

### Errors

[`SchemaRefusal::NoUpgradePath`] for any transition into schema 4 or above,
and [`SchemaRefusal::NotAnUpgrade`] for one that does not move forwards.

## `pub enum SchemaRefusal {`

---------------------------------------------------------------------------
Refusals
---------------------------------------------------------------------------

## `pub enum SchemaRefusal {`

Why a log was not read.

Every message names the numbers involved and what to do, because a refusal
an operator cannot act on is indistinguishable from a crash.

## `mod tests` › `fn header_line(event: &str, schema: Option<u32>) -> String {`

A first line that is genuinely hostile in every field a probe reads:
a run id in mixed case with padding around it, a multi-byte branch
name, and the schema buried after other keys rather than first.

## `mod tests` › `const HOSTILE_SCHEMAS: [u32; 22] = [`

-- the domain the relations are stated over -------------------------

The wire carries a `u32`, so the schema domain is 2^32 values and no
grid enumerates it. What a grid can do is cover every *partition* the
relation distinguishes and every *representation boundary* a narrowing
mutation could hide behind, which is what these two lists are.

Partitions of the schema domain against a ceiling: below it, equal to
it, above it but below the topology schema, exactly the topology schema,
and above the topology schema. Every one of those is populated below.

Representation boundaries: 127/128 (i8), 255/256/257/259 (u8),
511/512, 65535/65536 (u16), and the top of the u32 range. A guard
narrowed to any smaller integer width, or capped at any recognized
version, changes its answer at one of these.

## `mod tests` › `const HOSTILE_SCHEMAS: [u32; 22] = [`

Schema values every relation over the wire domain is crossed against.

## `mod tests` › `const HOSTILE_CEILINGS: [u32; 5] = [0, 1, 2, LATEST_LEGACY_SCHEMA, TOPOLOGY_SCHEMA];`

Ceilings the relation is crossed against.

`max_readable_schema` has image exactly `{3, 4}` — the two production
can hold — and the rest are hostile values that cannot arise and must
not change the relation. Ceilings above the topology schema are
deliberately absent: the design fixes the answer for `<= 3`, for `4`,
and for "above the ceiling", and says nothing about a binary claiming
to read schema 9, so a test asserting one would freeze an answer the
frozen design does not give.

## `mod tests` › `fn expected_selection(schema: u32, ceiling: u32) -> Result<ReaderSelection, SchemaRefusal…`

The reader-selection rule as the design states it, restated here and
never read off the implementation.

## `mod tests` › `fn hostile_later_header(schema: u32) -> Vec<u8> {`

A later line that is a *perfect* header in its own right, and whose
schema is chosen independently of whatever line 1 says. Anything that
looked past line 1 would find this and be believed.

## `mod tests` › `fn schema_constants_are_the_frozen_values_and_adjacent() {`

-- constants ---------------------------------------------------------

## `fn max_readable_is_the_activation_switch_and_production_is_…` › `assert_eq!(`

The ceiling is not a number this crate writes down twice: it is the
activation, evaluated. A mutation that hard-codes either side of the
switch is what these four assertions exist to catch.

## `fn max_readable_is_the_activation_switch_and_production_is_…` › `assert_eq!(MAX_READABLE_SCHEMA, 3);`

The slice's stated invariant, said in the plainest possible way.

## `fn fresh_writer_schema_maps_each_selector_to_a_different_mo…` › `assert!(fresh_writer_schema(WriterSelector::Production) <= MAX_READABLE_SCHEMA);`

Production never writes something production cannot read back.

## `mod tests` › `fn reader_selection_is_a_relation_over_every_ceiling_and_schema() {`

-- reader selection --------------------------------------------------

## `fn reader_selection_is_a_relation_over_every_ceiling_and_sc…` › `for ceiling in [LATEST_LEGACY_SCHEMA, TOPOLOGY_SCHEMA] {`

Crossed grid, not samples: a lookup table keyed on a handful of
pairs satisfies any finite set of examples, so the expectation here
is restated from the design rather than read off the implementation
— `<= 3` legacy, `4` topology, above the ceiling refuses, and the
topology refusal is a different refusal from the generic one.

## `mod tests` › `fn a_first_line_is_a_header_only_once_its_newline_commits_it() {`

-- header probe ------------------------------------------------------

## `fn a_first_line_is_a_header_only_once_its_newline_commits_i…` › `let line = header_line(RUN_STARTED, Some(2));`

The two inputs differ in exactly one byte. Anything that reads the
header from an uncommitted line passes the first assertion, so the
pair is the test: same bytes, opposite answers.

## `fn the_probe_reads_line_one_and_refuses_to_look_further()` › `let mut log = committed("task_merged", None);`

A run_started on line 2 is still a log that does not begin with one.
Scanning for the first run_started anywhere would accept this, and
would accept a log whose real header had been prefixed away.

## `fn the_probe_reads_line_one_and_refuses_to_look_further()` › `let mut good = committed(RUN_STARTED, Some(1));`

And the converse: damage after line 1 is not the probe's business.

## `fn a_committed_first_line_that_is_not_an_event_is_a_rewritt…` › `assert!(matches!(`

Invalid UTF-8 inside the committed first line, not after it.

## `mod tests` › `fn the_topology_refusal_is_a_different_message_from_the_generic_newer_one() {`

-- refusal messages --------------------------------------------------

## `fn the_topology_refusal_is_a_different_message_from_the_gen…` › `let topology = SchemaRefusal::TopologyLogUnreadable {`

The ceiling here is deliberately not production's, so a message that
renders a hard-coded 3 rather than the ceiling it was given fails.

## `mod tests` › `fn no_upgrade_reaches_the_topology_from_any_legacy_schema() {`

-- migration ---------------------------------------------------------

## `fn no_upgrade_reaches_the_topology_from_any_legacy_schema()` › `for from in 0..=5 {`

Crossed grid again: the rule is about the destination, and a test
that only ever asks about 3 -> 4 cannot tell a `>=` from a `>`, or a
destination check from a source check.

## `fn the_legacy_upgrade_ladder_still_runs_to_its_own_ceiling()` › `assert_eq!(check_upgrade_transition(1, 2), Ok(()));`

1 -> 2 -> 3 remains exactly what it was; only the step into the
topology is refused, and it is refused from every rung.

## `mod tests` › `fn reader_selection_holds_across_every_partition_and_integer_boundary() {`

==================================================================
The relations over the whole wire domain, not a sample of it
==================================================================

## `fn reader_selection_holds_across_every_partition_and_intege…` › `let mut cells = 0_u32;`

The grid above stops at 6, which is a range a `schema <= 6` cap
satisfies exactly and a `(schema as u8) > (ceiling as u8)`
narrowing never contradicts. This one crosses the partitions the
relation distinguishes against the boundaries of every integer
width the value could be narrowed to, including the top of u32.

## `fn reader_selection_holds_across_every_partition_and_intege…` › `assert_eq!(`

Named singly as well, so the intent survives a change to the lists.

## `fn no_upgrade_reaches_any_destination_at_or_above_the_topol…` › `let froms: [u32; 9] = [0, 1, 2, 3, 4, 5, 255, 256, u32::MAX];`

The destination rule is unbounded above. A grid that stops at 6 is
satisfied by `(TOPOLOGY_SCHEMA..=6).contains(&to)` and by an
`as u8` narrowing of the same comparison; both are wrong for a log
recording an upgrade into a schema nobody has written yet.

## `fn the_production_wrapper_refuses_every_future_schema_its_i…` › `for schema in HOSTILE_SCHEMAS {`

`select_reader` is what production calls, and a wrapper that
short-circuits before delegating passes every test of the function
it is supposed to be a composition of. Asserted as an identity over
committed bytes rather than as a sample of outcomes.

## `fn the_production_wrapper_refuses_every_future_schema_its_i…` › `assert_eq!(`

The two cases the wrapper is most tempting to special-case.

## `fn a_future_schema_survives_the_probe_at_its_recorded_width…` › `for schema in [5_u32, 6, 7, 9, 255, 256, 257, 259, 65_536, u32::MAX] {`

The probe's own integer type is a place the value can be lost: a
`u8` field cannot represent 256, and a header that cannot be
represented is reported as unreadable rather than as too new, or is
silently clamped to the topology schema and reported as a topology
log. Driven through committed JSON so the width under test is the
decoder's, not the caller's.

## `mod tests` › `fn the_line_feed_is_the_only_byte_that_commits_a_first_line() {`

==================================================================
The commit marker
==================================================================

## `fn the_line_feed_is_the_only_byte_that_commits_a_first_line…` › `let line = header_line(RUN_STARTED, Some(2));`

The existing pair proves LF-present against no-suffix, which any
"stop at the first line-ending byte" rule also satisfies. Crossing
the same header over all 256 one-byte suffixes is what separates
"the newline commits" from "some terminator commits": a CR-only
suffix is a torn write on Windows and must record nothing.

## `fn the_line_feed_is_the_only_byte_that_commits_a_first_line…` › `let mut crlf = line.clone().into_bytes();`

CRLF is committed, because it contains the newline: the CR is
trailing whitespace inside the committed line, not a terminator.

## `fn commitment_depends_on_the_newline_and_on_nothing_the_hea…` › `for schema in [`

The torn-write rule is exercised at one schema only, which a
schema-dependent exception at the 3/4 boundary survives. The same
bytes, differing in the commit marker alone, at every schema class.

## `fn commitment_depends_on_the_newline_and_on_nothing_the_hea…` › `assert_eq!(`

And through the composite entry point, where a torn line must
outrank whatever its bytes claim about the schema.

## `fn a_committed_header_outranks_every_kind_of_damage_after_i…` › `let head = committed(RUN_STARTED, Some(5));`

The converse of the torn-first-line rule, at the composite entry
point: line 1 is committed and above the ceiling, so the refusal is
fixed before anything later is looked at. A selector that inspected
the tail would report "nothing committed" for a log whose first line
records exactly what is wrong with it.

## `fn a_committed_header_outranks_every_kind_of_damage_after_i…` › `for schema in [1_u32, 5, 9, u32::MAX] {`

And the mirror: an uncommitted first line stays uncommitted however
newsworthy its bytes are.

## `mod tests` › `fn no_later_line_repairs_any_first_line_refusal() {`

==================================================================
Line 1 is the header, and nothing repairs it
==================================================================

## `fn no_later_line_repairs_any_first_line_refusal()` › `let first_lines: Vec<(&str, Vec<u8>, SchemaRefusal)> = vec![`

Every first-line refusal state, each paired with a perfect later
header whose schema is chosen independently of line 1. A probe that
fell through on a parse error, on invalid UTF-8, on a blank line, on
a wrong tag, or on a missing schema would find that later header and
read a rewritten log as a sound one — which refusals[22] says is
never repaired.

## `fn no_later_line_repairs_any_first_line_refusal()` › `assert!(`

And through the composite selector, where a repaired header
would silently choose a reader for the wrong model.

## `fn no_later_line_repairs_any_first_line_refusal()` › `let mut good = committed(RUN_STARTED, Some(1));`

The one case a suffix is allowed not to change: an accepted header
ignores everything after it. Stated so the assertions above cannot
be satisfied by refusing every multi-line log.

## `fn a_schema_read_out_of_invalid_committed_bytes_is_not_a_he…` › `let damaged: [&[u8]; 4] = [`

A committed first line that will not parse is a rewritten log,
whatever recognizable text it contains. Scanning it for a `schema`
token would let corruption choose the reader — and would report a
newer-schema refusal, sending the operator to upgrade upstroke for a
file that is damaged.

## `fn one_physical_line_holds_exactly_one_event()` › `let one = header_line(RUN_STARTED, Some(3));`

A decoder that took the first JSON value off the line and stopped
would accept a line carrying two records, or a record followed by
anything at all. Both are newline-terminated lines that are not a
valid event, which refusals[22] classifies as a rewritten log.

## `fn one_physical_line_holds_exactly_one_event()` › `assert_eq!(`

The single value it is built from is accepted, so the cases above
fail for the reason claimed.

## `mod tests` › `fn the_first_tag_is_compared_exactly_and_reported_verbatim() {`

==================================================================
The first event's tag
==================================================================

## `fn the_first_tag_is_compared_exactly_and_reported_verbatim()` › `for found in [`

Near misses, not an unrelated tag: a case-normalizing or trimming
comparison accepts a header no writer of this project ever wrote,
and `found` is what tells the operator which one they have.

## `fn a_non_run_started_first_line_refuses_whatever_schema_it_…` › `for schema in [`

The existing case correlates the wrong tag with an absent schema, so
a guard that refused only schema-less non-headers passes it. The tag
is decided before the schema is read, at every schema class.

## `fn a_first_line_that_is_not_an_event_envelope_is_unreadable…` › `let not_envelopes: [&[u8]; 9] = [`

`refusals` distinguishes a log that begins with the wrong event from
a committed line that is not a valid event at all, and they carry
different consequences: the second is a rewritten log, never
repaired. A defaulted `event` field collapses the two and reports a
rewritten log as a header with an empty tag.

## `mod tests` › `fn the_newer_schema_diagnostics_bind_each_number_to_its_role() {`

==================================================================
Refusal messages: the numbers keep their roles
==================================================================

## `fn the_newer_schema_diagnostics_bind_each_number_to_its_rol…` › `for (schema, ceiling) in [(9_u32, 7_u32), (5, 3), (4, 2), (256, 255), (u32::MAX, 0)] {`

Asserting that both numerals appear proves nothing about which is
which, and a diagnostic that swaps them tells the operator their
binary reads a schema newer than the log — the opposite of why it
refused, and an instruction to do nothing.

## `fn the_no_upgrade_refusal_never_advises_the_upgrade_it_refu…` › `for (from, to) in [(3_u32, TOPOLOGY_SCHEMA), (1, 4), (2, 9), (0, u32::MAX)] {`

The packet does not freeze this sentence and this test does not
pretend it does. What it does fix is the one thing the remediation
may not say: a refusal that tells the operator to append the
transition and carry on counsels violating INV-03, and the run it
produces is a schema-3 log reinterpreted as a topology one.

## `mod tests` › `fn the_activation_constant_is_asserted_outside_the_test_configuration() {`

==================================================================
Activation
==================================================================

## `fn the_activation_constant_is_asserted_outside_the_test_con…` › `const { assert!(matches!(TOPOLOGY_ACTIVATION, TopologyActivation::Inactive)) };`

The four `const _` assertions beside `MAX_READABLE_SCHEMA` are the
load-bearing ones: they are evaluated in the ordinary build, so an
activation that is `Inactive` under `cfg(test)` and `Active`
otherwise fails to compile rather than shipping a binary that folds
schema-4 logs. This test records that they exist and agrees with
them, and is deliberately not the proof.
