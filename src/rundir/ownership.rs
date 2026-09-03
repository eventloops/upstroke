//! The proof and the token it mints, alone in a module.
//!
//! The token's fields are private to this module and this module contains
//! exactly one function that constructs one, so
//! [`prove_private_half_ownership`] is the only constructor of
//! [`PrivateHalfProof`] — not by convention but because no other code, inside
//! `rundir` or outside it, can name the fields. The type derives nothing: a
//! `Clone` would let a spent token authorise a second deletion and a `Default`
//! would mint one out of nothing, and both are exactly what
//! `resource_accounting.completeness_rule` means by "a private-half deletion
//! outside the proof-token funnel fails to compile".
//!
//! **The seal moved out of line unchanged.** Every item keeps the visibility it
//! had inside the inline `mod ownership { … }`: the two names the parent
//! re-exports are the two it re-exported before, `commit_record_proves_absence`
//! is still `pub(super)`, and the three helpers below it are still private. A
//! sibling module of `rundir` can no more name [`PrivateHalfProof`]'s fields
//! than it could yesterday.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree and not by the file, and this module is
// the crate's single deletion authority, so inheriting `src/rundir.rs`'s inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// is exactly the shape `PR6-LANEF-004` is about. The proof is read-only --
// `fs::read_to_string`, `fs::symlink_metadata` and `fs::canonicalize`, none of
// them a governed primitive -- so all three are DENIED here and this module
// takes no `effects/allowlist.toml` row. The deletion the token authorises is
// `rundir::remove_private_husk`, in the parent, behind
// `RunDir.RemovePrivateHusk`, and it stays there.
//
// **Read-only is the whole of that argument, and it is not a claim of
// totality.** The reads are unbounded: `.creating` or `owner.json` swapped for
// a writer-less fifo blocks `read_to_string` in the kernel, and `husk_report`
// calls this proof under the physical worktree lock, so an entry that never
// proves is a lock held for ever. That code is byte-identical to the parent's
// and stays out of this split's scope; the assertion of totality over it does
// not, in either place it was made. `prove_private_half_ownership`'s own doc
// says the same thing where a reader of that function will see it, so the two
// cannot be read as disagreeing.
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

use super::{
    COMMIT_RECORD, CreatingMarker, MARKER, MARKER_STAGED, OWNER_RECORD, OwnerField, OwnerRecord,
    Path, PathBuf, PrivateHalfOwnership, RepoKey, RetainReason, UnboundShape, fs, io,
    read_dir_names, runner_policy_sha256,
};

/// Proof that one private half belongs to one public husk of this
/// repository and never committed.
///
/// Not `Clone`, not `Copy`, not `Default`, and constructed nowhere else.
/// [`super::remove_private_husk`] takes it by value, so it is spent.
#[derive(Debug)]
pub struct PrivateHalfProof {
    target: PathBuf,
    public: PathBuf,
    run_id: String,
}

impl PrivateHalfProof {
    /// The private half this token authorises deleting, and nothing else.
    #[must_use]
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// The public husk it is bound to.
    #[must_use]
    pub fn public_dir(&self) -> &Path {
        &self.public
    }

    /// The run both halves agree they belong to.
    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
}

/// The bidirectional ownership proof. Read-only, and **not** total.
///
/// Nothing here bounds a read. `.creating` or `owner.json` swapped for a
/// writer-less fifo blocks `fs::read_to_string` in the kernel, and
/// `husk_report` calls this proof under the physical worktree lock, so an
/// entry that never proves is a lock held for ever. There is deliberately no
/// analogue of the classification probe's byte budget: these two reads take
/// whole small records and have no `take` on them. The race is byte-identical
/// to the parent's and out of this split's scope; asserting totality over it
/// would not be, which is why this sentence no longer does.
///
/// `startup_census` (ii) states the conjunction and this is it, in that
/// order: a parseable marker, whose `run_id` equals the directory basename
/// and whose `repo_key` equals this repository's; then, if the recorded
/// target exists, a locator chain below the runs directory holding no
/// symlink or reparse point and canonicalizing to exactly
/// `<R>/runs/<basename>`; then `<target>/owner.json` parsing and recording
/// `run_id == basename`, `repo_key == this repository's`, `public_dir ==`
/// the canonical path of this husk, `incarnation ==` the marker's, and
/// `sha256(owner.runner) ==` the marker's `runner_policy_sha256`; and
/// finally `<target>/committed.json` absent.
///
/// Every conjunct refuses with its own [`RetainReason`], because each is
/// separately droppable and a suite that tested the happy path and one
/// negative would pass with any single one removed.
pub fn prove_private_half_ownership(
    public: &Path,
    repo_key: &RepoKey,
    authorized_root: &Path,
) -> PrivateHalfOwnership {
    let Some(basename) = public
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        // A run directory always has a basename; a path that does not is
        // not one this census can bind anything to. Retained, never
        // reclaimed: nothing private is deleted on shape alone.
        return PrivateHalfOwnership::Retained(RetainReason::MarkerUnparseable);
    };

    // Conjunct 1: the marker parses. No marker at all is a shape question
    // rather than a proof one, and `expected_failures_refusals` puts both
    // answers here — "a marker-less husk with content" is a RetainReason.
    let marker = match fs::read_to_string(public.join(MARKER)) {
        Ok(text) => match serde_json::from_str::<CreatingMarker>(&text) {
            Ok(marker) => marker,
            Err(_) => return PrivateHalfOwnership::Retained(RetainReason::MarkerUnparseable),
        },
        Err(_) => return unbound_shape(public),
    };

    // Conjunct 2: the marker names the directory it sits in.
    if marker.run_id != basename {
        return PrivateHalfOwnership::Retained(RetainReason::MarkerRunIdMismatch {
            recorded: marker.run_id,
            directory: basename,
        });
    }

    // Conjunct 3: and this repository.
    if marker.repo_key != repo_key.as_str() {
        return PrivateHalfOwnership::Retained(RetainReason::MarkerRepoKeyMismatch {
            recorded: marker.repo_key,
            expected: repo_key.as_str().to_owned(),
        });
    }

    let locator = PathBuf::from(&marker.private_dir);

    // The census's own step between the marker conjuncts and the locator
    // ones: "if the marker's private target does not exist the public husk
    // alone is reclaimed". Existence is asked of the link itself, so a
    // dangling symlink counts as present and is refused below rather than
    // reclaimed past.
    if fs::symlink_metadata(&locator).is_err() {
        return PrivateHalfOwnership::NothingBound(UnboundShape::TargetAbsent);
    }

    // Conjunct 4: no symlink or reparse point below the runs directory.
    let authorized_runs = authorized_root.join("runs");
    if let Some(component) = first_reparse_point(&authorized_runs, &locator) {
        return PrivateHalfOwnership::Retained(RetainReason::LocatorThroughReparsePoint {
            component,
        });
    }

    // Conjunct 5: and it canonicalizes to exactly <R>/runs/<basename>.
    // Both sides are canonicalized, so a private root reached through a
    // link — /tmp on macOS, a home directory on a mounted volume — is the
    // same root, while anything *below* runs had to be real to get here.
    let expected = match fs::canonicalize(&authorized_runs) {
        Ok(runs) => runs.join(&basename),
        Err(_) => authorized_runs.join(&basename),
    };
    match fs::canonicalize(&locator) {
        Ok(resolved) if resolved == expected => {}
        Ok(resolved) => {
            return PrivateHalfOwnership::Retained(RetainReason::LocatorOutsideAuthorizedRoot {
                locator: resolved,
                expected,
            });
        }
        Err(_) => {
            return PrivateHalfOwnership::Retained(RetainReason::LocatorOutsideAuthorizedRoot {
                locator,
                expected,
            });
        }
    }

    // Conjuncts 6-11: the reciprocal record.
    let owner = match fs::read_to_string(locator.join(OWNER_RECORD)) {
        Ok(text) => match serde_json::from_str::<OwnerRecord>(&text) {
            Ok(owner) => owner,
            Err(_) => {
                return PrivateHalfOwnership::Retained(RetainReason::OwnerRecordUnparseable);
            }
        },
        Err(_) => return PrivateHalfOwnership::Retained(RetainReason::OwnerRecordMissing),
    };
    let canonical_public = fs::canonicalize(public).unwrap_or_else(|_| public.to_path_buf());
    let disagreements = [
        (OwnerField::RunId, owner.run_id.clone(), basename.clone()),
        (
            OwnerField::RepoKey,
            owner.repo_key.clone(),
            repo_key.as_str().to_owned(),
        ),
        (
            OwnerField::PublicDir,
            owner.public_dir.clone(),
            canonical_public.to_string_lossy().into_owned(),
        ),
        (
            OwnerField::Incarnation,
            owner.incarnation.clone(),
            marker.incarnation.clone(),
        ),
        (
            OwnerField::RunnerDigest,
            runner_policy_sha256(&owner.runner),
            marker.runner_policy_sha256.clone(),
        ),
    ];
    for (field, recorded, expected) in disagreements {
        if recorded != expected {
            return PrivateHalfOwnership::Retained(RetainReason::OwnerRecordDisagrees {
                field,
                recorded,
                expected,
            });
        }
    }

    // Conjunct 12: and it never crossed P5b, fail-closed.
    if !commit_record_proves_absence(&fs::symlink_metadata(locator.join(COMMIT_RECORD))) {
        return PrivateHalfOwnership::Retained(RetainReason::PossiblyCommitted);
    }

    PrivateHalfOwnership::Proven(PrivateHalfProof {
        target: locator,
        public: public.to_path_buf(),
        run_id: basename,
    })
}

/// Conjunct 12, fail-closed: whether a stat of `<target>/committed.json`
/// **proves** the private half never crossed P5b.
///
/// Only [`io::ErrorKind::NotFound`] is proof of absence. Every other error
/// — `EACCES`, `EIO`, a Windows sharing violation — is an answer the
/// filesystem declined to give, and the packet's boundary is "the private
/// half is never deleted by cleanup once `committed.json` exists (creator
/// or census alike)": a record whose presence cannot be ruled out is on the
/// wrong side of it.
///
/// The conjunct was `fs::symlink_metadata(…).is_ok()`, which took every one
/// of those errors as absence and fell through to `Proven` — minting the
/// one token [`super::remove_private_husk`] accepts for a private half that
/// may carry a commit record. [`super::commit_record_after_error`] already
/// answers the same question the other way (`Unknown`, and
/// `permits_deletion()` is false for it), so the two paths into the
/// deletion boundary disagreed, and this is the half that failed **open**.
///
/// A free function rather than an inline `match` because the shape that
/// matters — which `io::ErrorKind`s are proof — is not deterministically
/// constructible from a single-threaded test through the real filesystem:
/// a directory made unreadable refuses conjunct 6's owner-record read
/// first, so reaching conjunct 12 with an unreadable stat needs a race.
/// Extracting the predicate makes the classification itself assertable,
/// and the two reachable shapes (present, absent) are asserted through the
/// whole proof.
pub(super) fn commit_record_proves_absence(stat: &io::Result<fs::Metadata>) -> bool {
    matches!(stat, Err(error) if error.kind() == io::ErrorKind::NotFound)
}

/// A husk with no marker: `startup_census` (i) reclaims "a bare directory
/// or one holding only a staged `.creating.tmp` (no marker, **no other
/// content**)", and (iii) retains "a marker-less husk carrying run-scoped
/// content". Read literally: anything other than the staging file is other
/// content, the empty run skeleton included, because retention costs a
/// report and reclamation cannot be undone.
fn unbound_shape(public: &Path) -> PrivateHalfOwnership {
    match read_dir_names(public).as_slice() {
        [] => PrivateHalfOwnership::NothingBound(UnboundShape::Bare),
        [only] if only == MARKER_STAGED => {
            PrivateHalfOwnership::NothingBound(UnboundShape::StagedMarkerOnly)
        }
        _ => PrivateHalfOwnership::Retained(RetainReason::MarkerlessWithContent),
    }
}

/// The first component of `locator` strictly below `runs` that is a link.
///
/// On Windows this is the reparse-point attribute rather than
/// `FileType::is_symlink`, because a **junction** is a reparse point that
/// is not a symbolic link, needs no privilege to create, and is exactly
/// what `expected_failures_refusals[0]` means by "symlink/junction on the
/// chain". A check that only fired on POSIX symlinks would pass every
/// Linux test and refuse nothing on the platform the word "junction" is
/// about.
///
/// Only *below* `runs`: `startup_census` says "the locator chain below the
/// runs directory holds no symlink or reparse point", and the private root
/// itself is legitimately reached through one on plenty of machines.
fn first_reparse_point(runs: &Path, locator: &Path) -> Option<PathBuf> {
    let below = locator.strip_prefix(runs).ok()?;
    let mut walked = runs.to_path_buf();
    for component in below.components() {
        walked.push(component);
        if is_reparse_point(&walked) {
            return Some(walked);
        }
    }
    None
}

fn is_reparse_point(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        // FILE_ATTRIBUTE_REPARSE_POINT. Every reparse point, so a junction
        // (IO_REPARSE_TAG_MOUNT_POINT) is refused alongside a symbolic
        // link (IO_REPARSE_TAG_SYMLINK).
        const REPARSE_POINT: u32 = 0x0000_0400;
        metadata.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
