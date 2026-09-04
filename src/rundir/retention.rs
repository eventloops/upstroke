//! The retention taxonomy: why a husk is kept rather than reclaimed.
//!
//! `startup_census` (iii) enumerates the shapes the ownership proof refuses on
//! and `expected_failures_refusals` enumerates them again as refusals, so the
//! set is closed and every variant here is one of them. Nothing in this module
//! decides anything — it is the vocabulary [`super::ownership`]'s proof answers
//! in, and the report surface renders. The proof itself is next door, and the
//! deletion it authorises is in the parent, behind a site.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree and not by the file, so an out-of-line
// child of `src/rundir.rs` would otherwise inherit that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// -- `PR6-LANEF-004`, measured twice in the Container subtree and made again,
// independently, by two W1 pull requests. Nothing here reaches a governed
// primitive, so all three are DENIED rather than allowed, and this module takes
// no `effects/allowlist.toml` row: an allowance is what that file records, and
// this module takes none.
//
// **Measured, not believed.** A probe of three lines -- a `std::fs::write`, a
// `std::process::Command` and a `println!` -- is refused three times here, once
// per lint, with this attribute cited as the level; the identical three lines in
// `src/rundir.rs` emit no `disallowed_*` at all, under that file's own allow. So
// the deny is load-bearing rather than a restatement of an ambient rule.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::path::PathBuf;

use super::PrivateHalfProof;

/// Why a husk is retained rather than reclaimed.
///
/// Every variant is a condition `prove_private_half_ownership` refuses on, and
/// the set is closed: `startup_census` (iii) enumerates the shapes, and
/// `expected_failures_refusals` enumerates them again as refusals. Nothing
/// private is ever deleted for any of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetainReason {
    /// A marker that is not JSON, or not this marker's shape.
    MarkerUnparseable,
    /// The marker names a run other than the directory it sits in — a forged
    /// marker pointing at another run's private half.
    MarkerRunIdMismatch { recorded: String, directory: String },
    /// The marker's repository key is not this repository's: a directory
    /// copied from another repository.
    MarkerRepoKeyMismatch { recorded: String, expected: String },
    /// The recorded locator does not canonicalize to
    /// `<authorized private root>/runs/<basename>`.
    LocatorOutsideAuthorizedRoot { locator: PathBuf, expected: PathBuf },
    /// A component of the locator below the runs directory is a symlink or,
    /// on Windows, any reparse point — a junction included.
    LocatorThroughReparsePoint { component: PathBuf },
    /// P3a: the private directory exists, its owner record does not.
    OwnerRecordMissing,
    /// The owner record is not readable as one.
    OwnerRecordUnparseable,
    /// The owner record disagrees with the marker or with the directory.
    OwnerRecordDisagrees {
        field: OwnerField,
        recorded: String,
        expected: String,
    },
    /// A husk with no marker at all, carrying run-scoped content.
    MarkerlessWithContent,
    /// The public half's own listing did not answer, so its shape was never
    /// established and neither reclaiming shape can be ruled in.
    ///
    /// `startup_census` (i) reclaims a husk that *is* "a bare directory or one
    /// holding only a staged `.creating.tmp`", and a directory that could not be
    /// listed has not been shown to be either. The detail carries the path and
    /// the error, because "could not be listed" leaves an operator no way to
    /// tell a descriptor exhaustion that has since cleared from a permission
    /// that has not.
    ListingUnreadable { detail: String },
    /// `committed.json` is present: the private half may have crossed P5b, so
    /// no census and no creating process ever deletes it.
    PossiblyCommitted,
}

impl RetainReason {
    /// Every kind of retention, as a closed set.
    ///
    /// The list a suite is measured against, so that a variant added later and
    /// tested by nobody fails a count rather than passing quietly. Rust has no
    /// reflection over variants, so [`Self::kind`]'s exhaustive match is what
    /// makes adding one to the enum and not to this list impossible.
    pub const KINDS: &'static [&'static str] = &[
        "marker-unparseable",
        "marker-run-id-mismatch",
        "marker-repo-key-mismatch",
        "locator-outside-authorized-root",
        "locator-through-reparse-point",
        "owner-record-missing",
        "owner-record-unparseable",
        "owner-record-disagrees",
        "markerless-with-content",
        "listing-unreadable",
        "possibly-committed",
    ];

    /// This reason's kind. Exhaustive by construction.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::MarkerUnparseable => "marker-unparseable",
            Self::MarkerRunIdMismatch { .. } => "marker-run-id-mismatch",
            Self::MarkerRepoKeyMismatch { .. } => "marker-repo-key-mismatch",
            Self::LocatorOutsideAuthorizedRoot { .. } => "locator-outside-authorized-root",
            Self::LocatorThroughReparsePoint { .. } => "locator-through-reparse-point",
            Self::OwnerRecordMissing => "owner-record-missing",
            Self::OwnerRecordUnparseable => "owner-record-unparseable",
            Self::OwnerRecordDisagrees { .. } => "owner-record-disagrees",
            Self::MarkerlessWithContent => "markerless-with-content",
            Self::ListingUnreadable { .. } => "listing-unreadable",
            Self::PossiblyCommitted => "possibly-committed",
        }
    }

    /// Which owner-record field disagreed, when that is what happened.
    #[must_use]
    pub const fn owner_field(&self) -> Option<OwnerField> {
        match self {
            Self::OwnerRecordDisagrees { field, .. } => Some(*field),
            _ => None,
        }
    }
}

/// Which field of the owner record disagreed.
///
/// `startup_census` (iii) names them: "a private target without an owner record
/// or with a disagreeing one" — disagreeing on "run id, repo key, public path,
/// incarnation, or runner digest" (ST-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnerField {
    RunId,
    RepoKey,
    PublicDir,
    Incarnation,
    RunnerDigest,
}

impl OwnerField {
    /// Every field the record is checked on.
    pub const ALL: &'static [Self] = &[
        Self::RunId,
        Self::RepoKey,
        Self::PublicDir,
        Self::Incarnation,
        Self::RunnerDigest,
    ];

    /// The field's name in the record.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RunId => "run_id",
            Self::RepoKey => "repo_key",
            Self::PublicDir => "public_dir",
            Self::Incarnation => "incarnation",
            Self::RunnerDigest => "runner digest",
        }
    }
}

impl std::fmt::Display for RetainReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MarkerUnparseable => f.write_str("its .creating marker cannot be read"),
            Self::MarkerRunIdMismatch {
                recorded,
                directory,
            } => write!(f, "its marker names run `{recorded}`, not `{directory}`"),
            Self::MarkerRepoKeyMismatch { recorded, expected } => write!(
                f,
                "its marker carries repository key `{recorded}`, not this repository's `{expected}`"
            ),
            Self::LocatorOutsideAuthorizedRoot { locator, expected } => write!(
                f,
                "its recorded private locator {} is not {}",
                locator.display(),
                expected.display()
            ),
            Self::LocatorThroughReparsePoint { component } => write!(
                f,
                "its private locator passes through the link {}",
                component.display()
            ),
            Self::OwnerRecordMissing => f.write_str("its private half carries no owner record"),
            Self::OwnerRecordUnparseable => {
                f.write_str("its private half's owner record cannot be read")
            }
            Self::OwnerRecordDisagrees {
                field,
                recorded,
                expected,
            } => write!(
                f,
                "its owner record's {} is `{recorded}`, not `{expected}`",
                field.name()
            ),
            Self::MarkerlessWithContent => {
                f.write_str("it carries run-scoped content but no marker to bind it")
            }
            Self::ListingUnreadable { detail } => write!(
                f,
                "its contents could not be listed, so nothing about its shape is \
                 established: {detail}"
            ),
            Self::PossiblyCommitted => {
                f.write_str("its private half carries a commit record, so the run may have started")
            }
        }
    }
}

/// The shape of a husk that binds nothing private.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnboundShape {
    /// P0: an empty public directory.
    Bare,
    /// P1a: only `.creating.tmp`, so the marker was never published and no
    /// private half exists by ordering.
    StagedMarkerOnly,
    /// P1b/P2: the marker published, its recorded target absent.
    TargetAbsent,
}

impl UnboundShape {
    /// Every shape, so a suite is measured against the closed set.
    pub const ALL: &'static [Self] = &[Self::Bare, Self::StagedMarkerOnly, Self::TargetAbsent];
}

/// What `prove_private_half_ownership` decided.
#[derive(Debug)]
pub enum PrivateHalfOwnership {
    /// The bidirectional proof holds and the private half carries no commit
    /// record. The token is the only key to [`super::remove_private_husk`].
    Proven(PrivateHalfProof),
    /// Nothing private is bound to this husk. The public half alone is
    /// reclaimed; there is no private half to prove anything about.
    NothingBound(UnboundShape),
    /// No token, ever. The census retains the husk and reports it.
    Retained(RetainReason),
}
