//! ULID generation (§15: `run-id = ULID`). A 48-bit millisecond timestamp and
//! 80 bits of SHA-256 over a domain tag, time, process id and per-process nonce.
//! The inputs have separate fixed-width encodings, so a pid bit cannot cancel
//! a nonce bit before hashing. These deterministic names are not secrets or
//! proof of ownership. Filesystem callers must reserve new roots exclusively.

use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Monotonic per-process nonce: many calls can share one millisecond, so the
/// timestamp alone must never be the whole seed.
static NONCE: AtomicU64 = AtomicU64::new(0);

pub fn ulid() -> String {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    observe_sampled_parts(now_ms, pid, nonce);
    ulid_from_parts(now_ms, pid, nonce)
}

/// The whole construction, over parts the caller supplies rather than the ones
/// `ulid` samples from the process. Splitting the sampling from the arithmetic
/// is what lets a test fix every input and assert an exact string, instead of
/// asserting a probabilistic projection of whatever the clock happened to say.
fn ulid_from_parts(now_ms: u64, pid: u32, nonce: u64) -> String {
    let mut digest = Sha256::new();
    digest.update(b"upstroke.ulid.v2\0");
    digest.update(now_ms.to_be_bytes());
    digest.update(pid.to_be_bytes());
    digest.update(nonce.to_be_bytes());
    let random_80 = digest
        .finalize()
        .into_iter()
        .take(10)
        .fold(0_u128, |value, byte| (value << 8) | u128::from(byte));
    let value = (u128::from(now_ms & 0xFFFF_FFFF_FFFF) << 80) | random_80;
    (0..26)
        .rev()
        .map(|i| {
            // The mask is at most 31 and CROCKFORD has exactly 32 entries.
            #[expect(
                clippy::indexing_slicing,
                reason = "the five-bit mask bounds this index into the 32-entry alphabet"
            )]
            let byte = CROCKFORD[((value >> (i * 5)) & 0x1F) as usize];
            char::from(byte)
        })
        .collect()
}

/// Outside tests the observation seam is nothing at all: an empty call, so
/// `ulid` keeps the behaviour it had before the seam existed. The half that
/// records is `observation`, below.
#[cfg(not(test))]
fn observe_sampled_parts(_now_ms: u64, _pid: u32, _nonce: u64) {}

/// The recording half of the observation seam.
///
/// It is a module rather than a pair of loose items because the first
/// test-configured attribute in a file is where `effects::production_region`
/// truncates, and `effects::tests::
/// every_production_region_that_stops_early_stops_at_a_module` requires that
/// cut to land on a module. Everything above this point is the whole
/// construction, which is what the region is for.
#[cfg(test)]
mod observation {
    use std::cell::Cell;

    thread_local! {
        /// The parts of this thread's most recent `ulid` call, or `None` if it
        /// has made none since the cell was last taken.
        pub(super) static SAMPLED_PARTS: Cell<Option<(u64, u32, u64)>> =
            const { Cell::new(None) };
    }

    /// Records the three parts `ulid` has just sampled, so a test can rebuild
    /// the id from exactly those values instead of inferring them from the id
    /// and from `NONCE`. Inference cannot distinguish a wrapper that constructs
    /// from what it sampled from one that constructs from something else;
    /// capture can. The record is per-thread, so tests running in parallel
    /// never see one another's.
    pub(super) fn observe_sampled_parts(now_ms: u64, pid: u32, nonce: u64) {
        SAMPLED_PARTS.with(|cell| cell.set(Some((now_ms, pid, nonce))));
    }
}

#[cfg(test)]
use observation::observe_sampled_parts;

#[cfg(test)]
mod tests {
    use super::*;

    /// `(now_ms, pid, nonce, the id those parts construct)`.
    type Vector = (u64, u32, u64, &'static str);

    /// The millisecond of the first call in the recording below, and an
    /// unremarkable clock reading for the boundary rows to hold fixed while
    /// they push some other part to its edge.
    const FIRST_MS: u64 = 1_788_084_161_241;

    /// The inputs were recorded by the old ambient-wrapper extraction. The
    /// outputs here use the new construction, computed independently with
    /// Python's hashlib.sha256 and big-endian integer encoding. They pin the
    /// domain tag, field widths, digest prefix and Crockford encoding. The old
    /// splitmix outputs remain in git history, not a compatibility promise for
    /// newly generated ids.
    const HASH_CONSTRUCTION_VECTORS: &[Vector] = &[
        (FIRST_MS, 2_437_999, 0, "01M191Y2PSP8400DBF5QSFFJT3"),
        (FIRST_MS + 5, 2_437_999, 5, "01M191Y2PYZCAHQWFFE0B0ZNEN"),
        (FIRST_MS + 6, 2_438_000, 0, "01M191Y2PZX38SSRHP2N0WH5HD"),
        (FIRST_MS + 11, 2_438_000, 5, "01M191Y2Q4J64Y3T1XQ81VQ07G"),
        (FIRST_MS + 13, 2_438_001, 0, "01M191Y2Q6YQRV0G4FJPPG7G7P"),
        (FIRST_MS + 18, 2_438_001, 5, "01M191Y2QB8FP00XRJD39669NS"),
    ];

    /// The edges of each part's range, which no ambient sample reaches: a clock
    /// at zero, at the last millisecond the field can print, and at the first
    /// one past it; a pid at the top of its type; and high nonce bits retained
    /// from the former construction's boundary inputs. Computed by an
    /// implementation using hashlib.sha256 and Crockford base32 independently
    /// of this module and validated first against every row above.
    const PARTS_AT_THEIR_BOUNDARIES: &[Vector] = &[
        (0, 0, 0, "0000000000XE2AXP270W5G6HJM"),
        (0xFFFF_FFFF_FFFF, 0, 0, "7ZZZZZZZZZWBB5YA5GXJK04BYR"),
        (1 << 48, 0, 0, "0000000000CKGJF0S4BQ5FRX2J"),
        (FIRST_MS, u32::MAX, 0, "01M191Y2PS9PHQ1G68XZYHJEE2"),
        (FIRST_MS, 1, u64::MAX, "01M191Y2PS5ZV781F18MXR5HQ6"),
        (FIRST_MS, 1, 1 << 47, "01M191Y2PSV05K2R9B9S755N9K"),
    ];

    /// This thread's most recent record, cleared before the call under test so
    /// that a wrapper which stopped reaching the seam reads as absent rather
    /// than as whatever was left behind.
    fn take_sampled_parts() -> Option<(u64, u32, u64)> {
        observation::SAMPLED_PARTS.with(std::cell::Cell::take)
    }

    #[test]
    fn pid_and_nonce_cannot_cancel_in_the_same_millisecond() {
        for ((pid_a, nonce_a), (pid_b, nonce_b)) in [
            ((100, 32_768), (101, 0)),
            ((100, 65_536), (102, 0)),
            ((100, 32_769), (101, 1)),
        ] {
            assert_ne!(
                ulid_from_parts(FIRST_MS, pid_a, nonce_a),
                ulid_from_parts(FIRST_MS, pid_b, nonce_b),
                "pid/nonce pairs ({pid_a}, {nonce_a}) and ({pid_b}, {nonce_b}) cancel"
            );
        }
    }

    #[test]
    fn ulids_are_26_crockford_chars() {
        let id = ulid();
        assert_eq!(id.len(), 26);
        assert!(id.bytes().all(|b| CROCKFORD.contains(&b)), "got: {id}");
    }

    #[test]
    fn ulids_do_not_collide_casually() {
        // One clock reading and one pid, and the single part the wrapper does
        // vary within a millisecond swept across two hundred consecutive values.
        // That is precisely the collision the nonce exists to prevent, and the
        // inputs now decide the outcome rather than what the clock happened to
        // say while the test ran.
        const PID: u32 = 4_242;
        let mut seen = std::collections::BTreeSet::new();
        for nonce in 0..200 {
            assert!(
                seen.insert(ulid_from_parts(FIRST_MS, PID, nonce)),
                "collision at nonce {nonce}"
            );
        }
        assert_eq!(seen.len(), 200);
    }

    #[test]
    fn the_public_wrapper_returns_exactly_a_parts_construction() {
        // The seam reports what `ulid` sampled, so all three parts arrive
        // independently of the id rather than being read back out of it. That
        // is the difference that matters: a wrapper which samples one nonce and
        // then constructs from another satisfies any test that infers its parts
        // from its own output, and fails this one.
        let _ = take_sampled_parts();
        let id = ulid();
        let Some((now_ms, pid, nonce)) = take_sampled_parts() else {
            panic!("`ulid` returned {id} without reaching the observation seam");
        };
        assert_eq!(
            ulid_from_parts(now_ms, pid, nonce),
            id,
            "the id is not what the parts it sampled construct"
        );
        assert_eq!(pid, std::process::id(), "the pid sampled was not this one");

        // A second call reserves a nonce of its own, and `NONCE` only ever
        // increases, so this holds however many threads drew from it in between.
        let _ = ulid();
        let Some((_, _, next_nonce)) = take_sampled_parts() else {
            panic!("the second call did not reach the observation seam");
        };
        assert!(
            next_nonce > nonce,
            "the second call reserved {next_nonce}, which does not follow {nonce}"
        );
    }

    #[test]
    fn reserving_a_nonce_yields_the_previous_value_and_wraps_at_the_top() {
        // The reservation `ulid` makes, on a counter belonging to this test, so
        // the process-wide `NONCE` other tests draw from is left alone.
        let counter = AtomicU64::new(41);
        assert_eq!(counter.fetch_add(1, Ordering::Relaxed), 41);
        assert_eq!(counter.load(Ordering::Relaxed), 42);

        // At the top of the range it wraps rather than trapping, so a process
        // that draws more than `u64::MAX` ids keeps issuing them. The nonce is
        // one of three terms in the seed, not the whole of it.
        let at_top = AtomicU64::new(u64::MAX);
        assert_eq!(at_top.fetch_add(1, Ordering::Relaxed), u64::MAX);
        assert_eq!(at_top.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn parts_construct_the_independently_computed_hash_vectors() {
        for &(now_ms, pid, nonce, expected) in HASH_CONSTRUCTION_VECTORS {
            assert_eq!(
                ulid_from_parts(now_ms, pid, nonce),
                expected,
                "({now_ms}, {pid}, {nonce}) differs from the independent SHA-256 vector"
            );
        }
    }

    #[test]
    fn parts_at_their_boundaries_construct_their_recorded_ids() {
        for &(now_ms, pid, nonce, expected) in PARTS_AT_THEIR_BOUNDARIES {
            assert_eq!(
                ulid_from_parts(now_ms, pid, nonce),
                expected,
                "({now_ms}, {pid}, {nonce}) constructs something other than its recorded id"
            );
        }
        // The printed field is forty-eight bits wide while the seed takes all of
        // `now_ms`: two clock values exactly one field apart print the same ten
        // leading characters, and the eighty bits under them still tell the two
        // milliseconds apart. This asserts that width, not the mask that spells
        // it out — `<< 80` into a `u128` discards bit 48 and above by itself, so
        // dropping the mask entirely would change no output.
        let epoch = ulid_from_parts(0, 0, 0);
        let one_field_later = ulid_from_parts(1 << 48, 0, 0);
        assert_eq!(epoch[..10], one_field_later[..10]);
        assert_ne!(epoch[10..], one_field_later[10..]);
    }
}
