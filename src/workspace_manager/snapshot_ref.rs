//! What an exact snapshot is checked out at, and what one is once it exists.
//!
//! `decisions.workspace_candidates.snapshots` distinguishes the two inputs a
//! snapshot can be taken of -- an existing commit, for which no object is
//! created, and a tree, for which the funnel first writes an ephemeral commit
//! on the recorded parent. [`SnapshotInput`] is that distinction as a value and
//! [`Snapshot`] is what the funnel returns; the funnel, the ephemeral
//! commit-tree it may run, and the ref discipline around it are the parent's.
//!
//! **Every id here is an [`ObjectId`]** (§5): the spelling of a full
//! hexadecimal object id of either hash length that is not the null id,
//! checked once where the value enters and carried as a type after that. The
//! funnel hands `git commit-tree` and `git worktree add` these values as
//! arguments, and Git resolves whatever it is given -- a ref name checks out
//! wherever that ref points at the moment, which is not an *exact* snapshot,
//! and a short or option-shaped string is parsed as something else. The type
//! removes those spellings. It cannot remove a ref *spelt* as hexadecimal of
//! the other object format's length (a branch named with forty hexadecimal
//! characters in a SHA-256 repository, and the inverse), because the type
//! does not know the repository; measured on git 2.43, `git worktree add
//! --detach <that name>` checks out wherever the branch points. So the funnel
//! resolves each input once against the repository (`rev-parse --verify
//! --quiet <id>^{commit}` or `^{tree}`) and accepts only an answer equal to
//! the input: a ref, an abbreviation, an id of the other format's length and
//! a missing object all resolve to something else or to nothing, and are
//! refused by name ([`Refusal::SnapshotInputResolvesElsewhere`]) before any
//! intent is written. What a [`Snapshot`] holds is therefore what Git
//! resolved to itself. The predicate is the parent's [`is_object_id`], the
//! well-formedness half of `design/26` step 5; the null id is refused too,
//! because that step measures it as a condition rather than an id, and no
//! object can be snapshotted at it.
//!
//! **What a [`Snapshot`] holds together** (§5, §6): its fields are private
//! and it has one constructor, [`Snapshot::new`], visible to the parent only.
//! The slot is built from the [`SnapshotName`] the constructor is given, so it
//! is a snapshot slot and never a task or staging one, which is what
//! `remove_snapshot` relies on when it removes the slot the value names. The
//! HEAD is one [`SnapshotHead`], so the ephemeral commit, when there is one,
//! *is* the HEAD rather than a second field that has to agree with it. The
//! checkout path is what `git worktree add` returned for that slot; the type
//! cannot re-derive it without the execution root, so the guarantee there is
//! the private field and the single construction site inside the funnel.
//!
//! **§6.** Nothing here is shared, locked or cloned. [`ObjectId`] and
//! [`SnapshotInput`] are small owned values and derive `Clone`, since an input
//! is built once and may be offered to more than one add. [`Snapshot`] does
//! not: it names one live checkout, and a copy of it would be two values for
//! one resource, of which `remove_snapshot` can only ever remove one. Equality
//! on all three is equality of the canonical spelling, which after the
//! funnel's resolution is equality of the object; the tests use it.

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

use std::fmt;
use std::path::{Path, PathBuf};

use super::{Refusal, Slot, SnapshotName, is_null_object_id, is_object_id};

/// The spelling of a full hexadecimal object id of either hash length that is
/// not the null id.
///
/// Validated once, at [`ObjectId::new`]; the field is private, so every value
/// of this type passed that check. It is spelt lowercase, as Git prints ids,
/// whatever case it was offered in: Git accepts either case for the same
/// object (measured in `object.rs`), so two spellings of one id are equal and
/// hash alike. Whether the spelling names an object of a given repository is
/// not this type's to know: the funnel resolves it (see the module doc).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectId(String);

impl ObjectId {
    /// Accepts `value` as an object id, spelt lowercase.
    ///
    /// # Errors
    ///
    /// [`Refusal::NotAnObjectId`] when `value` is not a full hexadecimal id
    /// of either hash length ([`is_object_id`]), or is the null id of either
    /// length ([`is_null_object_id`]). The refusal carries the value as it was
    /// offered. Nothing has run.
    pub fn new(mut value: String) -> Result<Self, Refusal> {
        if !is_object_id(&value) {
            return Err(Refusal::NotAnObjectId {
                value,
                why: "it is not a full hexadecimal object id",
            });
        }
        if is_null_object_id(&value) {
            return Err(Refusal::NotAnObjectId {
                value,
                why: "it is the null object id, which names no object",
            });
        }
        value.make_ascii_lowercase();
        Ok(Self(value))
    }

    /// The id as Git spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which id of a [`SnapshotInput`] a refusal is about, and what the funnel
/// peels it to when it asks the repository to resolve it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotObject {
    /// [`SnapshotInput::Commit`]: peeled to a commit.
    Commit,
    /// [`SnapshotInput::Tree`]'s `tree`: peeled to a tree.
    Tree,
    /// [`SnapshotInput::Tree`]'s `parent`: peeled to a commit.
    Parent,
}

impl SnapshotObject {
    /// The `rev-parse` peel that names the object type this id must be.
    #[must_use]
    pub fn peel(self) -> &'static str {
        match self {
            Self::Commit | Self::Parent => "^{commit}",
            Self::Tree => "^{tree}",
        }
    }
}

impl fmt::Display for SnapshotObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Commit => "commit",
            Self::Tree => "tree",
            Self::Parent => "parent",
        })
    }
}

/// What a snapshot is checked out at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotInput {
    /// The integration case: an existing commit, and no object is created.
    Commit(ObjectId),
    /// The candidate case: a tree, for which the funnel first writes an
    /// ephemeral commit on `parent`.
    Tree {
        /// The immutable tree under judgment.
        tree: ObjectId,
        /// The recorded parent the ephemeral commit sits on.
        parent: ObjectId,
    },
}

/// The commit a snapshot's detached HEAD names, and where it came from: the
/// two arms of [`SnapshotInput`] after the funnel has run.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SnapshotHead {
    /// The [`SnapshotInput::Commit`] as it was given; no object was created.
    Existing(ObjectId),
    /// The ephemeral commit the funnel wrote for a [`SnapshotInput::Tree`].
    /// It returns to R27 when the snapshot is removed.
    Ephemeral(ObjectId),
}

impl SnapshotHead {
    /// The commit, whichever arm it came from.
    pub(super) fn id(&self) -> &ObjectId {
        match self {
            Self::Existing(id) | Self::Ephemeral(id) => id,
        }
    }
}

/// One live exact snapshot: what the parent's `add_snapshot` returns.
#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Its slot, always [`Slot::Snapshot`].
    slot: Slot,
    /// Its checkout, as `git worktree add` returned it for the slot.
    path: PathBuf,
    /// The commit its detached HEAD names, and whether the funnel created it.
    head: SnapshotHead,
}

impl Snapshot {
    /// The snapshot the funnel has just checked out: the slot named `name`,
    /// at `path`, with its HEAD at `head`.
    ///
    /// Parent-visible only: a snapshot is what the funnel returns, and this
    /// is the one place the three fields meet.
    pub(super) fn new(name: SnapshotName, path: PathBuf, head: SnapshotHead) -> Self {
        Self {
            slot: Slot::Snapshot { name },
            path,
            head,
        }
    }

    /// Its slot: a [`Slot::Snapshot`], by construction.
    #[must_use]
    pub fn slot(&self) -> &Slot {
        &self.slot
    }

    /// Its checkout.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The commit its detached HEAD names.
    #[must_use]
    pub fn head(&self) -> &ObjectId {
        self.head.id()
    }

    /// The ephemeral commit this snapshot created, when its input was a tree.
    /// It is the HEAD, and it returns to R27 when the snapshot is removed.
    #[must_use]
    pub fn ephemeral(&self) -> Option<&ObjectId> {
        match &self.head {
            SnapshotHead::Existing(_) => None,
            SnapshotHead::Ephemeral(id) => Some(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `len` lowercase hexadecimal characters that are not all one digit, so
    /// a well-formed id that is not the null id.
    fn hex_of(len: usize) -> String {
        "0123456789abcdef".chars().cycle().take(len).collect()
    }

    fn zeros(len: usize) -> String {
        "0".repeat(len)
    }

    fn id(len: usize) -> ObjectId {
        ObjectId::new(hex_of(len)).expect("a full non-null hexadecimal id")
    }

    #[test]
    fn a_full_non_null_hexadecimal_id_of_either_hash_length_is_an_object_id() {
        for len in [40, 64] {
            let lower = hex_of(len);
            let mut almost_null = zeros(len);
            almost_null.replace_range(len - 1.., "1");
            for value in [lower, almost_null] {
                let accepted = ObjectId::new(value.clone());
                assert_eq!(
                    accepted.as_ref().map(ObjectId::as_str),
                    Ok(value.as_str()),
                    "{value} is accepted and kept as offered"
                );
                assert_eq!(accepted.as_ref().map(ToString::to_string), Ok(value));
            }
        }
    }

    #[test]
    fn an_id_offered_in_upper_or_mixed_case_is_the_same_object_id_spelt_lowercase() {
        for len in [40, 64] {
            let lower = hex_of(len);
            let upper = lower.to_ascii_uppercase();
            let mixed: String = lower
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    if i % 2 == 0 {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    }
                })
                .collect();
            let canonical = ObjectId::new(lower.clone()).expect("lowercase");
            for offered in [upper, mixed] {
                let accepted = ObjectId::new(offered.clone()).expect("either case is accepted");
                assert_eq!(
                    accepted.as_str(),
                    lower,
                    "{offered} is spelt as Git prints it"
                );
                assert_eq!(accepted, canonical, "and names the same object");
            }
        }
    }

    #[test]
    fn a_malformed_value_is_refused_as_not_an_object_id_with_the_value_as_offered() {
        let short = hex_of(39);
        let long = hex_of(65);
        let mut non_hex = hex_of(64);
        non_hex.replace_range(63.., "g");
        let trailing_newline = format!("{}\n", hex_of(40));
        for hostile in [
            "",
            "HEAD",
            "refs/heads/main",
            "--delete",
            "-p",
            &short,
            &long,
            &non_hex,
            &trailing_newline,
        ] {
            assert_eq!(
                ObjectId::new(hostile.to_owned()),
                Err(Refusal::NotAnObjectId {
                    value: hostile.to_owned(),
                    why: "it is not a full hexadecimal object id",
                }),
                "{hostile:?}"
            );
        }
    }

    #[test]
    fn the_null_id_of_either_hash_length_is_refused_as_naming_no_object() {
        for len in [40, 64] {
            assert_eq!(
                ObjectId::new(zeros(len)),
                Err(Refusal::NotAnObjectId {
                    value: zeros(len),
                    why: "it is the null object id, which names no object",
                }),
                "{len} zeros"
            );
        }
    }

    #[test]
    fn an_existing_head_is_the_commit_given_and_records_no_ephemeral_commit() {
        let commit = id(40);
        let snapshot = Snapshot::new(
            SnapshotName::integration(3),
            PathBuf::from("checkout"),
            SnapshotHead::Existing(commit.clone()),
        );
        assert_eq!(
            snapshot.slot(),
            &Slot::Snapshot {
                name: SnapshotName::integration(3)
            }
        );
        assert_eq!(snapshot.path(), Path::new("checkout"));
        assert_eq!(snapshot.head(), &commit);
        assert_eq!(snapshot.ephemeral(), None);
    }

    #[test]
    fn an_ephemeral_head_is_both_the_head_and_the_ephemeral_commit() {
        let created = id(64);
        let snapshot = Snapshot::new(
            SnapshotName::review(1, 2, 0),
            PathBuf::from("checkout"),
            SnapshotHead::Ephemeral(created.clone()),
        );
        assert_eq!(
            snapshot.slot(),
            &Slot::Snapshot {
                name: SnapshotName::review(1, 2, 0)
            }
        );
        assert_eq!(snapshot.head(), &created);
        assert_eq!(
            snapshot.ephemeral(),
            Some(&created),
            "the ephemeral commit is the HEAD, not a value beside it"
        );
    }

    #[test]
    fn two_snapshots_are_equal_exactly_when_they_name_the_same_checkout() {
        let build = |name: SnapshotName, path: &str, head: SnapshotHead| {
            Snapshot::new(name, PathBuf::from(path), head)
        };
        let gates = build(
            SnapshotName::gates(1, 1),
            "a",
            SnapshotHead::Ephemeral(id(40)),
        );
        assert_eq!(
            gates,
            build(
                SnapshotName::gates(1, 1),
                "a",
                SnapshotHead::Ephemeral(id(40))
            )
        );
        for other in [
            build(
                SnapshotName::gates(1, 2),
                "a",
                SnapshotHead::Ephemeral(id(40)),
            ),
            build(
                SnapshotName::gates(1, 1),
                "b",
                SnapshotHead::Ephemeral(id(40)),
            ),
            build(
                SnapshotName::gates(1, 1),
                "a",
                SnapshotHead::Existing(id(40)),
            ),
            build(
                SnapshotName::gates(1, 1),
                "a",
                SnapshotHead::Ephemeral(id(64)),
            ),
        ] {
            assert_ne!(gates, other, "{other:?}");
        }
    }
}
