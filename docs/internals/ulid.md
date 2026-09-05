# `src/ulid.rs`

Extended notes for [`src/ulid.rs`](../../src/ulid.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

ULID generation (§15: `run-id = ULID`). Std-only: 48-bit millisecond
timestamp plus 80 pseudo-random bits from a splitmix64 stream seeded from
time, process id, and a monotonic per-process nonce. Uniqueness against
ourselves is the requirement — nothing cryptographic.

## `static NONCE: AtomicU64 = AtomicU64::new(0);`

Monotonic per-process nonce: many calls can share one millisecond, so the
timestamp alone must never be the whole seed.

## `fn ulid_from_parts(now_ms: u64, pid: u32, nonce: u64) -> String {`

The whole construction, over parts the caller supplies rather than the ones
`ulid` samples from the process. Splitting the sampling from the arithmetic
is what lets a test fix every input and assert an exact string, instead of
asserting a probabilistic projection of whatever the clock happened to say.

## `fn observe_sampled_parts(_now_ms: u64, _pid: u32, _nonce: u64) {}`

Outside tests the observation seam is nothing at all: an empty call, so
`ulid` keeps the behaviour it had before the seam existed. The half that
records is `observation`, below.

## `mod observation {`

The recording half of the observation seam.

It is a module rather than a pair of loose items because the first
test-configured attribute in a file is where `effects::production_region`
truncates, and `effects::tests::
every_production_region_that_stops_early_stops_at_a_module` requires that
cut to land on a module. Everything above this point is the whole
construction, which is what the region is for.

## `mod observation` › `pub(super) static SAMPLED_PARTS: Cell<Option<(u64, u32, u64)>> =`

The parts of this thread's most recent `ulid` call, or `None` if it
has made none since the cell was last taken.

## `mod observation` › `pub(super) fn observe_sampled_parts(now_ms: u64, pid: u32, nonce: u64) {`

Records the three parts `ulid` has just sampled, so a test can rebuild
the id from exactly those values instead of inferring them from the id
and from `NONCE`. Inference cannot distinguish a wrapper that constructs
from what it sampled from one that constructs from something else;
capture can. The record is per-thread, so tests running in parallel
never see one another's.

## `mod tests` › `type Vector = (u64, u32, u64, &'static str);`

`(now_ms, pid, nonce, the id those parts construct)`.

## `mod tests` › `const FIRST_MS: u64 = 1_788_084_161_241;`

The millisecond of the first call in the recording below, and an
unremarkable clock reading for the boundary rows to hold fixed while
they push some other part to its edge.

## `mod tests` › `const RECORDED_FROM_THE_AMBIENT_WRAPPER: &[Vector] = &[`

Recorded from the **pre-extraction** `ulid()`, at
3db8e5be004dd26eb4503948c849d21db14915c2, where this construction was
still inline in the wrapper and `ulid_from_parts` did not exist. A
harness called that wrapper in three fresh processes, which fixes all
three parts of each call after the fact: the nonce is the call index,
because `NONCE` starts at zero and the wrapper reserves one per call;
the pid is the harness process's own; and `now_ms` is recoverable from
the first ten characters of the id that came back. So these are the old
wrapper's own outputs, and the extraction is what they hold to account —
not this module measured against itself.

Eighteen were recorded and checked out of tree; the six kept here are
the executable ones, two per process, and they are what this test set
proves. The other twelve are not in the repository and no test reads
them.

## `mod tests` › `const PARTS_AT_THEIR_BOUNDARIES: &[Vector] = &[`

The edges of each part's range, which no ambient sample reaches: a clock
at zero, at the last millisecond the field can print, and at the first
one past it; a pid at the top of its type; and the nonce rotation at the
two places it carries bits across the top of the word. Computed by an
implementation of splitmix64 and Crockford base32 written independently
of this module and validated first against every row above.

## `mod tests` › `fn take_sampled_parts() -> Option<(u64, u32, u64)> {`

This thread's most recent record, cleared before the call under test so
that a wrapper which stopped reaching the seam reads as absent rather
than as whatever was left behind.

## `fn ulids_do_not_collide_casually()` › `const PID: u32 = 4_242;`

One clock reading and one pid, and the single part the wrapper does
vary within a millisecond swept across two hundred consecutive values.
That is precisely the collision the nonce exists to prevent, and the
inputs now decide the outcome rather than what the clock happened to
say while the test ran.

## `fn the_public_wrapper_returns_exactly_a_parts_construction()` › `let _ = take_sampled_parts();`

The seam reports what `ulid` sampled, so all three parts arrive
independently of the id rather than being read back out of it. That
is the difference that matters: a wrapper which samples one nonce and
then constructs from another satisfies any test that infers its parts
from its own output, and fails this one.

## `fn the_public_wrapper_returns_exactly_a_parts_construction()` › `let _ = ulid();`

A second call reserves a nonce of its own, and `NONCE` only ever
increases, so this holds however many threads drew from it in between.

## `fn reserving_a_nonce_yields_the_previous_value_and_wraps_at_the_top() {` › `let counter = AtomicU64::new(41);`

The reservation `ulid` makes, on a counter belonging to this test, so
the process-wide `NONCE` other tests draw from is left alone.

## `fn reserving_a_nonce_yields_the_previous_value_and_wraps_at_the_top() {` › `let at_top = AtomicU64::new(u64::MAX);`

At the top of the range it wraps rather than trapping, so a process
that draws more than `u64::MAX` ids keeps issuing them. The nonce is
one of three terms in the seed, not the whole of it.

## `fn parts_at_their_boundaries_construct_their_recorded_ids()` › `let epoch = ulid_from_parts(0, 0, 0);`

The printed field is forty-eight bits wide while the seed takes all of
`now_ms`: two clock values exactly one field apart print the same ten
leading characters, and the eighty bits under them still tell the two
milliseconds apart. This asserts that width, not the mask that spells
it out — `<< 80` into a `u128` discards bit 48 and above by itself, so
dropping the mask entirely would change no output.
