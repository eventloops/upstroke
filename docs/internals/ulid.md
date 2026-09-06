# `src/ulid.rs`

Extended notes for [`src/ulid.rs`](../../src/ulid.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

ULID generation (§15: `run-id = ULID`). A 48-bit millisecond timestamp and
80 bits of SHA-256 over a domain tag, time, process id and per-process nonce.
The inputs have separate fixed-width encodings, so a pid bit cannot cancel
a nonce bit before hashing. These deterministic names are not secrets or
proof of ownership. Filesystem callers must reserve new roots exclusively.

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

## `const HASH_CONSTRUCTION_VECTORS: &[Vector] = &[`

The inputs were recorded by the old ambient-wrapper extraction. The
outputs here use the new construction, computed independently with
Python's hashlib.sha256 and big-endian integer encoding. They pin the
domain tag, field widths, digest prefix and Crockford encoding. The old
splitmix outputs remain in git history, not a compatibility promise for
newly generated ids.

## `const PARTS_AT_THEIR_BOUNDARIES: &[Vector] = &[`

The edges of each part's range, which no ambient sample reaches: a clock
at zero, at the last millisecond the field can print, and at the first
one past it; a pid at the top of its type; and high nonce bits retained
from the former construction's boundary inputs. Computed by an
implementation using hashlib.sha256 and Crockford base32 independently
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

## `ulid_from_parts` › `CROCKFORD`

The mask is at most 31 and CROCKFORD has exactly 32 entries.
