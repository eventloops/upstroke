//! Extended notes: `docs/internals/ulid.md`

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

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

#[cfg(not(test))]
fn observe_sampled_parts(_now_ms: u64, _pid: u32, _nonce: u64) {}

#[cfg(test)]
mod observation {
    use std::cell::Cell;

    thread_local! {
        pub(super) static SAMPLED_PARTS: Cell<Option<(u64, u32, u64)>> =
            const { Cell::new(None) };
    }

    pub(super) fn observe_sampled_parts(now_ms: u64, pid: u32, nonce: u64) {
        SAMPLED_PARTS.with(|cell| cell.set(Some((now_ms, pid, nonce))));
    }
}

#[cfg(test)]
use observation::observe_sampled_parts;

#[cfg(test)]
mod tests {
    use super::*;

    type Vector = (u64, u32, u64, &'static str);

    const FIRST_MS: u64 = 1_788_084_161_241;

    const RECORDED_FROM_THE_AMBIENT_WRAPPER: &[Vector] = &[
        (FIRST_MS, 2_437_999, 0, "01M191Y2PSXY56YHDQP510FKS2"),
        (FIRST_MS + 5, 2_437_999, 5, "01M191Y2PYQRR9VKKQ9WG6DZ1W"),
        (FIRST_MS + 6, 2_438_000, 0, "01M191Y2PZ0ZD7ANPK1VJ7NDK8"),
        (FIRST_MS + 11, 2_438_000, 5, "01M191Y2Q4Z1ZJGV5JZ03NDZHJ"),
        (FIRST_MS + 13, 2_438_001, 0, "01M191Y2Q68KZQ8WHY9ZFW8AWC"),
        (FIRST_MS + 18, 2_438_001, 5, "01M191Y2QB0SET5VG2QDKRVQKG"),
    ];

    const PARTS_AT_THEIR_BOUNDARIES: &[Vector] = &[
        (0, 0, 0, "0000000000W8GAGEBV3Q6TYSFM"),
        (0xFFFF_FFFF_FFFF, 0, 0, "7ZZZZZZZZZRQ1NHB27QP0ESAGX"),
        (1 << 48, 0, 0, "0000000000MA2YFC6YPRVN1NJT"),
        (FIRST_MS, u32::MAX, 0, "01M191Y2PSE71HQET9T8S8GXC3"),
        (FIRST_MS, 1, u64::MAX, "01M191Y2PST2RDZM242A4P7D5A"),
        (FIRST_MS, 1, 1 << 47, "01M191Y2PSQJ4F5RHZ3YZPXCEB"),
    ];

    fn take_sampled_parts() -> Option<(u64, u32, u64)> {
        observation::SAMPLED_PARTS.with(std::cell::Cell::take)
    }

    #[test]
    fn ulids_are_26_crockford_chars() {
        let id = ulid();
        assert_eq!(id.len(), 26);
        assert!(id.bytes().all(|b| CROCKFORD.contains(&b)), "got: {id}");
    }

    #[test]
    fn ulids_do_not_collide_casually() {
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
        let counter = AtomicU64::new(41);
        assert_eq!(counter.fetch_add(1, Ordering::Relaxed), 41);
        assert_eq!(counter.load(Ordering::Relaxed), 42);

        let at_top = AtomicU64::new(u64::MAX);
        assert_eq!(at_top.fetch_add(1, Ordering::Relaxed), u64::MAX);
        assert_eq!(at_top.load(Ordering::Relaxed), 0);
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
        let epoch = ulid_from_parts(0, 0, 0);
        let one_field_later = ulid_from_parts(1 << 48, 0, 0);
        assert_eq!(epoch[..10], one_field_later[..10]);
        assert_ne!(epoch[10..], one_field_later[10..]);
    }
}
