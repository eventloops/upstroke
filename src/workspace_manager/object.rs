//! Object-id vocabulary, and the refusals every ref transition is gated on.
//!
//! **The well-formedness half of the ref rule** (`design/26` step 5: a full
//! hexadecimal object id on both sides, the null id refused on either).
//! `update-ref` is given ids, not references: a malformed one is refused before
//! any funnel runs, and the null id is refused on both sides of an update,
//! because measured on git 2.43 it does not mean "this id" on either. As the expected-old,
//! `update-ref -d <ref> 0{40}` deletes *unconditionally* rather than failing the
//! compare-and-swap the caller believed it had written. As the new value,
//! `update-ref <ref> 0{40} <old>` succeeds and **deletes** the ref when `<old>`
//! matches, and `update-ref <ref> 0{40} ""` succeeds and creates nothing when
//! the ref is absent (a mismatched old value, or an existing ref on the create
//! path, exits 128 and preserves the ref, as for any new value): the null id
//! there means "must not exist afterwards", which turns a compare-and-swap into
//! a guarded delete and a create into a reported success with no ref behind it.
//! The transitions themselves -- create, move, delete, and the merge-queue
//! CAS -- are the parent's funnels.
//!
//! **§6 and §7.** Nothing here is shared, locked or cloned: a refusal copies
//! the ref name and the offered value into the [`Refusal`] it returns, which is
//! the owned snapshot an error value is. The refusals return the [`Refusal`]
//! itself rather than the parent's flattened `UpstrokeError`, so a caller or a
//! test can match the variant; the parent's `?` sites convert through its own
//! `From<Refusal>`. The two `?` in this file are same-type propagation of a
//! refusal that already says everything a reader of the failure needs, and
//! each says so at its site.

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

use super::Refusal;

/// Hexadecimal characters in a SHA-1 object id, the hash of every repository
/// this engine has run against.
const SHA1_HEX_CHARS: usize = 40;

/// Hexadecimal characters in a SHA-256 object id (`extensions.objectFormat =
/// sha256`). Nothing here knows which format the repository uses: the
/// predicates accept either length, and Git decides whether the id names an
/// object of the repository it is given to.
const SHA256_HEX_CHARS: usize = 64;

/// Whether `value` is a full hexadecimal object id of either hash length.
///
/// Both letter cases are accepted, as Git's own hex parser accepts them
/// (measured, git 2.43: an uppercase id is taken on both sides of
/// `update-ref`). A short id, a ref name, an option, or anything with a byte
/// outside `[0-9A-Fa-f]` is not an object id.
#[must_use]
pub fn is_object_id(value: &str) -> bool {
    matches!(value.len(), SHA1_HEX_CHARS | SHA256_HEX_CHARS)
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Whether `value` is the null object id of either hash length.
#[must_use]
pub fn is_null_object_id(value: &str) -> bool {
    is_object_id(value) && value.bytes().all(|byte| byte == b'0')
}

/// Refuses `value` unless it is a full hexadecimal object id.
///
/// `role` is which side of the update the value was offered as (`"new"` or
/// `"expected-old"`); it travels into the refusal so the message says which
/// argument was wrong, and nothing branches on it.
///
/// # Errors
///
/// [`Refusal::MalformedObjectId`], naming the ref, the role and the value as it
/// was offered. Nothing has run: this is checked before any funnel.
fn refuse_malformed_object_id(
    refname: &str,
    role: &'static str,
    value: &str,
) -> Result<(), Refusal> {
    if is_object_id(value) {
        return Ok(());
    }
    Err(Refusal::MalformedObjectId {
        refname: refname.to_owned(),
        role,
        value: value.to_owned(),
    })
}

/// The expected-old side of every move and delete: a well-formed, non-null id.
///
/// # Errors
///
/// [`Refusal::MalformedObjectId`] when `old` is not a full hexadecimal id, and
/// [`Refusal::NullExpectedOld`] when it is the null id of either length. Nothing
/// has run.
pub(super) fn refuse_expected_old(refname: &str, old: &str) -> Result<(), Refusal> {
    // Deliberate `?` (§7): the refusal it propagates already names the ref,
    // the side (`expected-old`) and the value as offered, and this function
    // has nothing to add to it.
    refuse_malformed_object_id(refname, "expected-old", old)?;
    if is_null_object_id(old) {
        return Err(Refusal::NullExpectedOld {
            refname: refname.to_owned(),
        });
    }
    Ok(())
}

/// The new side of every create and compare-and-swap: a well-formed, non-null
/// id.
///
/// Crate-visible, not `pub(super)`: the engine's `ensure_integration_ref` and
/// the test doubles that implement its `IntegrationRefs` apply the same
/// refusal, so the contract is one function rather than a copy per
/// implementation.
///
/// # Errors
///
/// [`Refusal::MalformedObjectId`] when `new` is not a full hexadecimal id, and
/// [`Refusal::NullNew`] when it is the null id of either length, which Git
/// would read as "must not exist afterwards" (see the module doc). Nothing has
/// run.
pub(crate) fn refuse_new(refname: &str, new: &str) -> Result<(), Refusal> {
    // Deliberate `?` (§7): as in `refuse_expected_old`, the refusal already
    // names the ref, the side (`new`) and the value as offered.
    refuse_malformed_object_id(refname, "new", new)?;
    if is_null_object_id(new) {
        return Err(Refusal::NullNew {
            refname: refname.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF: &str = "refs/upstroke/runs/run-1/candidates/kalpha/1";

    /// `len` lowercase hexadecimal characters that are not all one digit, so a
    /// well-formed id that is not the null id.
    fn hex_of(len: usize) -> String {
        "0123456789abcdef".chars().cycle().take(len).collect()
    }

    fn zeros(len: usize) -> String {
        "0".repeat(len)
    }

    /// `hex_of(len)` with `replacement` written over its last character.
    fn ending_in(len: usize, replacement: &str) -> String {
        let mut value = hex_of(len);
        value.replace_range(len - 1.., replacement);
        value
    }

    #[test]
    fn a_full_hexadecimal_id_of_either_hash_length_is_an_object_id() {
        for len in [40, 64] {
            let lower = hex_of(len);
            let upper = lower.to_ascii_uppercase();
            let mixed = "aB".repeat(len / 2);
            for id in [&lower, &upper, &mixed, &zeros(len)] {
                assert!(is_object_id(id), "{id} is a full hexadecimal id");
            }
        }
    }

    #[test]
    fn any_other_length_or_any_non_hex_byte_is_not_an_object_id() {
        for len in [0, 1, 39, 41, 63, 65, 128] {
            let id = hex_of(len);
            assert!(!is_object_id(&id), "{len} hexadecimal characters");
        }
        for len in [40, 64] {
            let last = ending_in(len, "g");
            assert!(!is_object_id(&last), "{last}: `g` in the last position");
            let mut first = hex_of(len);
            first.replace_range(..1, "G");
            assert!(!is_object_id(&first), "{first}: `G` in the first position");
        }
        // Forty bytes that are not forty hexadecimal characters.
        let multibyte = format!("{}\u{e9}", hex_of(38));
        assert_eq!(multibyte.len(), 40, "the fixture is forty bytes long");
        assert!(!is_object_id(&multibyte));
        let trailing_newline = format!("{}\n", hex_of(40));
        for hostile in ["--delete", "refs/heads/main", "HEAD", &trailing_newline] {
            assert!(!is_object_id(hostile), "{hostile:?}");
        }
    }

    #[test]
    fn the_null_id_is_all_zeros_at_either_hash_length_and_nothing_else_is() {
        assert!(is_null_object_id(&zeros(40)));
        assert!(is_null_object_id(&zeros(64)));
        for len in [0, 39, 41, 63, 65] {
            assert!(!is_null_object_id(&zeros(len)), "{len} zeros");
        }
        for len in [40, 64] {
            let mut almost = zeros(len);
            almost.replace_range(len - 1.., "1");
            assert!(!is_null_object_id(&almost), "{almost}");
            assert!(!is_null_object_id(&hex_of(len)));
        }
    }

    #[test]
    fn a_malformed_id_is_refused_naming_the_ref_the_role_and_the_value_as_offered() {
        for len in [40, 64] {
            assert_eq!(refuse_malformed_object_id(REF, "new", &hex_of(len)), Ok(()));
            assert_eq!(
                refuse_malformed_object_id(REF, "expected-old", &zeros(len)),
                Ok(()),
                "the null id is well-formed; the sides refuse it separately"
            );
        }
        let short = hex_of(39);
        let long = hex_of(65);
        let non_hex = ending_in(64, "g");
        for hostile in [
            "",
            "--delete",
            "refs/heads/main",
            "zzzz",
            &short,
            &long,
            &non_hex,
        ] {
            assert_eq!(
                refuse_malformed_object_id(REF, "new", hostile),
                Err(Refusal::MalformedObjectId {
                    refname: REF.to_owned(),
                    role: "new",
                    value: hostile.to_owned(),
                }),
                "{hostile:?}"
            );
        }
    }

    #[test]
    fn an_expected_old_is_a_well_formed_non_null_id_at_either_hash_length() {
        for len in [40, 64] {
            assert_eq!(refuse_expected_old(REF, &hex_of(len)), Ok(()));
            assert_eq!(
                refuse_expected_old(REF, &zeros(len)),
                Err(Refusal::NullExpectedOld {
                    refname: REF.to_owned(),
                }),
                "{len} zeros"
            );
            assert_eq!(
                refuse_expected_old(REF, &zeros(len - 1)),
                Err(Refusal::MalformedObjectId {
                    refname: REF.to_owned(),
                    role: "expected-old",
                    value: zeros(len - 1),
                }),
                "{} zeros is malformed, not null",
                len - 1
            );
        }
    }

    #[test]
    fn a_new_value_is_a_well_formed_non_null_id_at_either_hash_length() {
        for len in [40, 64] {
            assert_eq!(refuse_new(REF, &hex_of(len)), Ok(()));
            assert_eq!(
                refuse_new(REF, &zeros(len)),
                Err(Refusal::NullNew {
                    refname: REF.to_owned(),
                }),
                "{len} zeros"
            );
            assert_eq!(
                refuse_new(REF, &ending_in(len, "g")),
                Err(Refusal::MalformedObjectId {
                    refname: REF.to_owned(),
                    role: "new",
                    value: ending_in(len, "g"),
                }),
                "{len} characters with a `g` is malformed, not null"
            );
        }
    }
}
