//! ULID generation (§15: `run-id = ULID`). Std-only: 48-bit millisecond
//! timestamp plus 80 pseudo-random bits from a splitmix64 stream seeded from
//! time, pid, and ASLR. Uniqueness against ourselves is the requirement —
//! nothing cryptographic.

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
    let mut seed = now_ms ^ (u64::from(std::process::id()) << 32) ^ nonce.rotate_left(17);
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

    #[test]
    fn ulids_are_26_crockford_chars() {
        let id = ulid();
        assert_eq!(id.len(), 26);
        assert!(id.bytes().all(|b| CROCKFORD.contains(&b)), "got: {id}");
    }

    #[test]
    fn ulids_do_not_collide_casually() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            assert!(seen.insert(ulid()), "collision");
        }
    }
}
