//! ULID generation (§15: `run-id = ULID`). Std-only: 48-bit millisecond
//! timestamp plus 80 pseudo-random bits from a splitmix64 stream seeded from
//! time, process id, and a monotonic per-process nonce. Uniqueness against
//! ourselves is the requirement — nothing cryptographic.

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
    ulid_from_parts(now_ms, std::process::id(), nonce)
}

/// The whole construction, over parts the caller supplies rather than the ones
/// `ulid` samples from the process. Splitting the sampling from the arithmetic
/// is what lets a test fix every input and assert an exact string, instead of
/// asserting a probabilistic projection of whatever the clock happened to say.
fn ulid_from_parts(now_ms: u64, pid: u32, nonce: u64) -> String {
    let mut seed = now_ms ^ (u64::from(pid) << 32) ^ nonce.rotate_left(17);
    let mut next = || {
        seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let random_80 = (u128::from(next()) << 16) | u128::from(next() & 0xFFFF);
    let value = (u128::from(now_ms & 0xFFFF_FFFF_FFFF) << 80) | random_80;
    (0..26)
        .rev()
        .map(|i| CROCKFORD[usize::try_from((value >> (i * 5)) & 0x1F).unwrap_or(0)] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(now_ms, pid, nonce, the id those parts construct)`.
    type Vector = (u64, u32, u64, &'static str);

    /// The millisecond of the first call in the recording below, and an
    /// unremarkable clock reading for the boundary rows to hold fixed while
    /// they push some other part to its edge.
    const FIRST_MS: u64 = 1_788_084_161_241;

    /// Recorded from the **pre-extraction** `ulid()`, at
    /// 3db8e5be004dd26eb4503948c849d21db14915c2, where this construction was
    /// still inline in the wrapper and `ulid_from_parts` did not exist. A
    /// harness called that wrapper in three fresh processes, which fixes all
    /// three parts of each call after the fact: the nonce is the call index,
    /// because `NONCE` starts at zero and the wrapper reserves one per call;
    /// the pid is the harness process's own; and `now_ms` is recoverable from
    /// the first ten characters of the id that came back. So these are the old
    /// wrapper's own outputs, and the extraction is what they hold to account —
    /// not this module measured against itself.
    const RECORDED_FROM_THE_AMBIENT_WRAPPER: &[Vector] = &[
        (FIRST_MS, 2_437_999, 0, "01M191Y2PSXY56YHDQP510FKS2"),
        (FIRST_MS + 5, 2_437_999, 5, "01M191Y2PYQRR9VKKQ9WG6DZ1W"),
        (FIRST_MS + 6, 2_438_000, 0, "01M191Y2PZ0ZD7ANPK1VJ7NDK8"),
        (FIRST_MS + 11, 2_438_000, 5, "01M191Y2Q4Z1ZJGV5JZ03NDZHJ"),
        (FIRST_MS + 13, 2_438_001, 0, "01M191Y2Q68KZQ8WHY9ZFW8AWC"),
        (FIRST_MS + 18, 2_438_001, 5, "01M191Y2QB0SET5VG2QDKRVQKG"),
    ];

    /// The edges of each part's range, which no ambient sample reaches: a clock
    /// at zero, at the last millisecond the field can print, and at the first
    /// one past it; a pid at the top of its type; and the nonce rotation at the
    /// two places it carries bits across the top of the word. Computed by an
    /// implementation of splitmix64 and Crockford base32 written independently
    /// of this module and validated first against every row above.
    const PARTS_AT_THEIR_BOUNDARIES: &[Vector] = &[
        (0, 0, 0, "0000000000W8GAGEBV3Q6TYSFM"),
        (0xFFFF_FFFF_FFFF, 0, 0, "7ZZZZZZZZZRQ1NHB27QP0ESAGX"),
        (1 << 48, 0, 0, "0000000000MA2YFC6YPRVN1NJT"),
        (FIRST_MS, u32::MAX, 0, "01M191Y2PSE71HQET9T8S8GXC3"),
        (FIRST_MS, 1, u64::MAX, "01M191Y2PST2RDZM242A4P7D5A"),
        (FIRST_MS, 1, 1 << 47, "01M191Y2PSQJ4F5RHZ3YZPXCEB"),
    ];

    /// The 48-bit timestamp field an id prints, from its first ten characters.
    fn timestamp_field(id: &str) -> u64 {
        id.bytes().take(10).fold(0, |acc, byte| {
            let Some(digit) = CROCKFORD.iter().position(|&c| c == byte) else {
                panic!("not a Crockford digit: {byte:#04x}");
            };
            (acc << 5) | u64::try_from(digit).unwrap_or(0)
        })
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
        // Both parts the wrapper samples are recoverable, so this compares all
        // twenty-six characters rather than a shape.
        //
        // `now_ms`: the leading ten characters hold `now_ms & 0xFFFF_FFFF_FFFF`,
        // and a millisecond clock does not reach that mask until the year 10889,
        // so the decode is the exact value the wrapper mixed into its seed.
        //
        // `nonce`: unknown, but bracketed. `NONCE` only ever increases and a
        // single atomic has one total modification order, so the value this call
        // reserved lies in `low..high` however many other threads raced it.
        let low = NONCE.load(Ordering::Relaxed);
        let id = ulid();
        let high = NONCE.load(Ordering::Relaxed);
        let now_ms = timestamp_field(&id);
        let pid = std::process::id();
        assert!(
            (low..high).any(|nonce| ulid_from_parts(now_ms, pid, nonce) == id),
            "{id} is not ulid_from_parts({now_ms}, {pid}, n) for any n in {low}..{high}"
        );
    }

    #[test]
    fn parts_reconstruct_the_ids_the_ambient_wrapper_recorded() {
        for &(now_ms, pid, nonce, expected) in RECORDED_FROM_THE_AMBIENT_WRAPPER {
            assert_eq!(
                ulid_from_parts(now_ms, pid, nonce),
                expected,
                "({now_ms}, {pid}, {nonce}) no longer constructs what the wrapper returned for it"
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
        // The mask is on the printed field and not on the seed: two clock values
        // exactly one field apart print the same ten leading characters, and the
        // eighty bits under them must still tell the two milliseconds apart.
        let epoch = ulid_from_parts(0, 0, 0);
        let one_field_later = ulid_from_parts(1 << 48, 0, 0);
        assert_eq!(epoch[..10], one_field_later[..10]);
        assert_ne!(epoch[10..], one_field_later[10..]);
    }
}
