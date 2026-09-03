//! Object-id vocabulary, and the two refusals every ref transition is gated on.
//!
//! **INV-17's well-formedness half.** `update-ref` is given ids, not
//! references: a malformed one is refused before any funnel runs, and a null
//! expected-old is refused outright, because measured on git 2.43
//! `update-ref -d <ref> 0{40}` deletes *unconditionally* rather than failing the
//! compare-and-swap the caller believed it had written. The transitions
//! themselves -- create, move, delete, and the merge-queue CAS -- are the
//! parent's funnels.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree rather than by the file, so an out-of-line
// child of `src/workspace_manager.rs` inherits that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// unless it says otherwise -- `PR6-LANEF-004`, and the mistake two W1 pull
// requests then made independently (#100 and #102). Nothing here reaches a
// governed primitive, so all three are DENIED and this module takes no
// `effects/allowlist.toml` row: a row records an allowance, and this module
// takes none.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use crate::error::UpstrokeError;

use super::Refusal;

/// Whether `value` is a full hexadecimal object id of either hash length.
#[must_use]
pub fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Whether `value` is the null object id of either hash length.
#[must_use]
pub fn is_null_object_id(value: &str) -> bool {
    is_object_id(value) && value.bytes().all(|byte| byte == b'0')
}

pub(super) fn refuse_malformed_object_id(
    refname: &str,
    role: &'static str,
    value: &str,
) -> Result<(), UpstrokeError> {
    if is_object_id(value) {
        return Ok(());
    }
    Err(Refusal::MalformedObjectId {
        refname: refname.to_owned(),
        role,
        value: value.to_owned(),
    }
    .into())
}

/// The expected-old side of every move and delete: a well-formed, non-null id.
pub(super) fn refuse_expected_old(refname: &str, old: &str) -> Result<(), UpstrokeError> {
    refuse_malformed_object_id(refname, "expected-old", old)?;
    if is_null_object_id(old) {
        return Err(Refusal::NullExpectedOld {
            refname: refname.to_owned(),
        }
        .into());
    }
    Ok(())
}
