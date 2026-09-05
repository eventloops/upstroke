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
    COMMIT_RECORD, CreatingMarker, MARKER, MARKER_STAGED, OWNER_RECORD, OsStr, OwnerField,
    OwnerRecord, Path, PathBuf, PrivateHalfOwnership, RepoKey, RetainReason, UnboundShape, fs, io,
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
    let authorized_runs = authorized_root.join("runs");

    // **Identity before existence, and this ordering is the fix for a P1.**
    // The existence check below used to run first, and `TargetAbsent` is a
    // reclaiming answer, so a recorded locator that names *some other path* was
    // answered "the target is gone" before anything had established that the
    // recorded path was this run's at all. The locator-identity conjuncts below
    // would have refused it — they run too late to stop the reclaim.
    //
    // That is not hypothetical and it needs no hostile input and no failing
    // syscall. A private root whose bytes are not valid UTF-8 — `\xff-private`
    // is an ordinary Unix directory name — is recorded by `create.rs`'s marker
    // write through `to_string_lossy()`, so the marker names `<U+FFFD>-private`
    // and the real half lives at `\xff-private`. Measured on this crate at
    // `cc202c8`: the proof answered `NothingBound(TargetAbsent)` with the real
    // private half still on disk, and the reclaim that licenses deletes
    // `.creating`, which is that half's only locator.
    //
    // So the recorded locator must first be shown to be a child of the
    // authorized runs directory *by name*: its own file name is this run's
    // basename, and its parent canonicalizes to the same directory as
    // `<R>/runs`. Neither question needs the target to exist, which is what
    // keeps the legitimate `TargetAbsent` answer reachable. A mangled locator
    // has a parent that does not canonicalize at all and is refused here.
    //
    // The parent is compared *canonically* rather than textually because
    // `prelock::authorized_private_root` canonicalizes the root and joins
    // `runs/<id>`, while this side canonicalizes `<R>/runs` — the two spellings
    // differ when `runs` is itself a link, which is legitimate, and both
    // canonicalize to one directory.
    //
    // The **lossless** repair — recording a path that round-trips exactly — is
    // deliberately not attempted here. `CreatingMarker.private_dir`,
    // `OwnerRecord.public_dir` and `run_started.private_dir` in the committed
    // event log all carry the same `String`, so it is three schema changes, and
    // the policy question has a parked owner-level record in PR #39. What this
    // conjunct does instead is refuse: a run whose private root is not valid
    // UTF-8 is retained and reported for ever rather than wrongly reclaimed.
    // The stat happens here, and only its **refusing** answer is acted on here.
    // A stat that did not answer is a retention whatever else is true, and
    // taking it first keeps its fixture portable: a locator the platform will
    // not accept as a path fails this stat on every platform, and never reaches
    // the identity questions below because it has no answer to give them.
    // A `match` rather than a let-chain: let-chains are not stable at this
    // crate's MSRV, and `cargo +1.85.0 check` is the gate that says so.
    let target_stat = fs::symlink_metadata(&locator);
    match &target_stat {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return PrivateHalfOwnership::Retained(RetainReason::TargetUndecidable {
                detail: format!("{}: {error}", locator.display()),
            });
        }
        _ => {}
    }

    // Three answers, and only one of them continues.
    match locator_identity(&locator, authorized_root, &authorized_runs, &basename) {
        LocatorIdentity::Established => {}
        LocatorIdentity::Refused { expected } => {
            return PrivateHalfOwnership::Retained(RetainReason::LocatorOutsideAuthorizedRoot {
                locator,
                expected,
            });
        }
        // The same reason as a stat that did not answer, and deliberately so:
        // both are "a question about the recorded target the filesystem
        // declined to answer", the variant's contract names both, and the
        // detail carries the path that did not answer — here the root or its
        // `runs` directory, not the target. The target's own stat may have
        // answered `NotFound` above; that answer is not acted on, because it is
        // about a path nothing has shown to be this run's.
        LocatorIdentity::Undetermined { detail } => {
            return PrivateHalfOwnership::Retained(RetainReason::TargetUndecidable { detail });
        }
    }

    // The census's own step between the marker conjuncts and the locator
    // ones: "if the marker's private target does not exist the public husk
    // alone is reclaimed". Existence is asked of the link itself, so a
    // dangling symlink counts as present and is refused below rather than
    // reclaimed past.
    //
    // **Fail-closed, and only `NotFound` is proof.** This was
    // `symlink_metadata(&locator).is_err()`, which read every error as absence
    // — and `TargetAbsent` is a *reclaiming* answer, whose reclaim deletes the
    // public directory with `.creating` in it. That marker is the private
    // half's only locator, and `create.rs` says what losing it costs: "a
    // private half no marker names is one no census, no `status` and no
    // deferred prune can ever reach again". So an `EACCES` on a parent
    // component, an `ELOOP`, an `ENOTDIR`, a Windows sharing violation or a
    // locator the platform will not accept as a path orphaned a private half
    // that was still there, on evidence that it was gone.
    //
    // It is `SWEEP-CLASSIFY-009`'s class through a different syscall, and not
    // its instance: `lstat(2)` consumes no file descriptor, so the descriptor
    // exhaustion that drives that finding never reaches this conjunct and the
    // listing's repair does not close this one.
    //
    // The rule is the one `commit_record_proves_absence` states for conjunct
    // 12 — only `io::ErrorKind::NotFound` proves a path is not there — spelled
    // again here rather than shared, so that each conjunct keeps refusing on
    // its own and a mutation to one is not a mutation to both.
    // **`TargetAbsent` at last, and only now.** The stat above said `NotFound`
    // and the two questions between said the recorded locator is this run's
    // child of the authorized runs directory. Both are needed: `NotFound` alone
    // was the P1.
    if target_stat.is_err() {
        return PrivateHalfOwnership::NothingBound(UnboundShape::TargetAbsent);
    }

    // Conjunct 4: no symlink or reparse point below the runs directory.
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
    // Conjunct 8's own expected value, fail-closed. This was
    // `fs::canonicalize(public).unwrap_or_else(|_| public.to_path_buf())`, and
    // that fold is a worse shape than the three above it: those turned a
    // failure into a *reclaiming* answer, this one turns a failure into a
    // **proving** one. The fallback supplies the value the comparison then
    // uses, so a canonicalization that failed can carry the conjunct — and
    // `create.rs`'s `canonical_string` uses the identical fallback when
    // *recording* `owner.public_dir`, so both sides degrade the same way and
    // match. A proof that passes because two lookups failed together mints the
    // token `remove_private_husk` accepts.
    //
    // The refusal is `OwnerRecordDisagrees` on the public-directory field
    // rather than a kind of its own, and that mirrors conjunct 12 exactly: the
    // shape is not constructible single-threaded — a `public` this proof has
    // already read a marker out of canonicalizes — so the classification lives
    // in a free function the suite can assert directly, and the reachable
    // shapes go through the whole proof. See
    // [`canonical_public_or_refusal`].
    let canonical_public =
        match canonical_public_or_refusal(public, fs::canonicalize(public), &owner.public_dir) {
            Ok(path) => path,
            Err(reason) => return PrivateHalfOwnership::Retained(reason),
        };
    // **Conjunct 8 compares paths, not renderings of them.** This built the
    // expected side with `canonical_public.to_string_lossy()` and compared it as
    // a `String`, and `create.rs`'s `canonical_string` records the same way, so
    // two canonical roots differing only in bytes that are not valid UTF-8 —
    // `\x80` against `\x81` — collapse to one string on both sides and the
    // conjunct passes for a husk that is not this one. That answer is `Proven`,
    // which mints the token authorising a private-half deletion, so the lossy
    // comparison could hand run B a token for run A's private half.
    //
    // The recorded value is still the `String` the record carries, and it is
    // still rendered lossily in the refusal — a lossy string is a diagnostic.
    // What it is no longer is the thing compared: `Path::new` over the recorded
    // bytes against the canonical path answers identity. With the recorder left
    // lossy (see the note above conjunct 3 on why), a public path that is not
    // valid UTF-8 now never satisfies this conjunct, so such a run is retained
    // and reported rather than proven — fail-closed, and permanent until the
    // recorder question is settled.
    if Path::new(&owner.public_dir) != canonical_public {
        return PrivateHalfOwnership::Retained(RetainReason::OwnerRecordDisagrees {
            field: OwnerField::PublicDir,
            recorded: owner.public_dir,
            expected: canonical_public.to_string_lossy().into_owned(),
        });
    }
    let disagreements = [
        (OwnerField::RunId, owner.run_id.clone(), basename.clone()),
        (
            OwnerField::RepoKey,
            owner.repo_key.clone(),
            repo_key.as_str().to_owned(),
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

/// Conjunct 8's expected value: the husk's **canonical** path, or the refusal
/// that stands in for one that could not be resolved.
///
/// `startup_census` (ii) says the owner record must record `public_dir ==` the
/// canonical path of this husk. A canonicalization that failed produces no
/// canonical path, so there is nothing to compare and the conjunct cannot be
/// carried: the answer is a refusal naming the public-directory field, with the
/// error in the expected side so an operator can see that the comparison did not
/// happen rather than that it failed.
///
/// **Why this refuses rather than falling back to the given path.** The fallback
/// it replaces was the only fold in this file that could turn an I/O failure
/// into `Proven`. `create.rs`'s `canonical_string` uses the same fallback when
/// writing `owner.public_dir`, so a recorder and a prover that both failed to
/// canonicalize agreed on the un-canonical spelling and the conjunct passed —
/// establishing nothing while minting the token that authorises deleting a
/// private half.
///
/// **A free function for the same reason as
/// [`commit_record_proves_absence`].** A `public` this proof has already read a
/// marker out of will canonicalize; making it fail here needs a race, so the
/// classification is not constructible from a single-threaded test through the
/// real filesystem. Extracting it makes the decision itself assertable, and the
/// reachable shape — a canonicalization that succeeds and a record that agrees
/// or disagrees — is exercised through the whole proof by the grid.
pub(super) fn canonical_public_or_refusal(
    public: &Path,
    canonical: io::Result<PathBuf>,
    recorded: &str,
) -> Result<PathBuf, RetainReason> {
    match canonical {
        Ok(path) => Ok(path),
        Err(error) => Err(RetainReason::OwnerRecordDisagrees {
            field: OwnerField::PublicDir,
            recorded: recorded.to_owned(),
            expected: format!(
                "the canonical path of {}, which could not be resolved: {error}",
                public.display()
            ),
        }),
    }
}

/// What the recorded locator was shown to be: the three-way answer conjunct 3a
/// needs, with only one arm that continues.
///
/// **This is a `enum` and not a `bool` because the boolean was the defect.**
/// The first version of this gate read
/// `locator != expected_raw && expected_canonical.as_ref().is_ok_and(|it| *it != locator)`,
/// and `is_ok_and` answers `false` for `Err` — so a canonicalization that
/// *failed* was indistinguishable from one that agreed, every locator unequal
/// to the raw spelling walked past the refusal, and a `NotFound` stat then
/// produced the reclaiming `TargetAbsent`. It is the same class this whole pull
/// request is about, in the fix for the first four instances of it: an error
/// folded into the answer that proceeds.
///
/// A three-way answer removes the shape rather than adding a condition. There
/// is no expression here that can turn an error into [`Self::Established`],
/// because the only place that variant is constructed is a successful equality,
/// and the caller's `match` is exhaustive over the other two.
///
/// The proof checks three locator forms using path equality:
///
/// - Raw `<R>/runs/<id>`, which needs no I/O and permits `TargetAbsent` at P1.
/// - The canonical root joined with `runs/<id>`. This is what
///   `prelock::authorized_private_root` records, including when the configured
///   root reaches its real path through a link and `runs` does not yet exist.
/// - Canonical `<R>/runs` joined with `<id>`, which also resolves a link at
///   `runs` itself.
enum LocatorIdentity {
    /// The recorded locator is this run's child of the authorized runs
    /// directory, in one of the three spellings that name it.
    Established,
    /// It is some other path, and this is the one it should have been.
    Refused { expected: PathBuf },
    /// The question was not answered: the authorized runs directory could not
    /// be resolved, so where this run's private half *should* be is unknown
    /// and no locator can be placed against it.
    Undetermined { detail: String },
}

fn locator_identity(
    locator: &Path,
    authorized_root: &Path,
    authorized_runs: &Path,
    basename: &str,
) -> LocatorIdentity {
    // (1) The raw spelling, and it costs no I/O. This is the one that carries
    // P1 on a machine whose configured root is already canonical.
    if locator == authorized_runs.join(basename) {
        return LocatorIdentity::Established;
    }
    // (2) The **root** canonicalized, which is precisely what
    // `prelock::authorized_private_root` records: it canonicalizes the root,
    // because the run directory does not exist during the pre-lock checks, and
    // joins `runs/<id>`. On any machine where the configured root reaches the
    // real one through a link — `/var` to `/private/var` on macOS, a verbatim
    // prefix on Windows — this is the only spelling that matches at P1, and CI
    // is what taught this function so: the first version checked (1) and (3)
    // alone and turned the legitimate `TargetAbsent` at P1 into a refusal on
    // the macOS and Windows legs while passing on Linux.
    let canonical_root = fs::canonicalize(authorized_root);
    if matches!(&canonical_root, Ok(root) if locator == root.join("runs").join(basename)) {
        return LocatorIdentity::Established;
    }
    // (3) `<R>/runs` canonicalized, which differs from (2) when `runs` is
    // itself a link — legitimate, and refused by neither.
    let canonical_runs = fs::canonicalize(authorized_runs);
    if matches!(&canonical_runs, Ok(runs) if locator == runs.join(basename)) {
        return LocatorIdentity::Established;
    }
    // Nothing matched. That is a refusal only if every spelling was actually
    // computed; a canonicalization that failed is a spelling this function
    // never saw, so it cannot say the locator is not that one.
    match (&canonical_root, &canonical_runs) {
        (Ok(_), Ok(runs)) => LocatorIdentity::Refused {
            expected: runs.join(basename),
        },
        (Err(error), _) => LocatorIdentity::Undetermined {
            detail: format!("{}: {error}", authorized_root.display()),
        },
        (_, Err(error)) => LocatorIdentity::Undetermined {
            detail: format!("{}: {error}", authorized_runs.display()),
        },
    }
}

/// A husk with no marker: `startup_census` (i) reclaims "a bare directory
/// or one holding only a staged `.creating.tmp` (no marker, **no other
/// content**)", and (iii) retains "a marker-less husk carrying run-scoped
/// content". Read literally: anything other than the staging file is other
/// content, the empty run skeleton included, because retention costs a
/// report and reclamation cannot be undone.
///
/// **Both reclaiming answers require a listing that happened**
/// (`SWEEP-CLASSIFY-009`). This is a *second* observation of a directory the
/// caller has already failed to read once — it is reached only from conjunct
/// 1's error arm — and [`super::read_dir_names`] used to answer `[]` for a
/// `read_dir` that failed, which is this function's `Bare` arm and the
/// reclaiming one. Under a failure that refuses the listing while the census's
/// earlier gates still answer — a public directory that can be searched but not
/// listed is the measured one — the plan became `ReclaimPublicOnly`, which
/// carries no commit-record check anywhere on its path, and
/// [`super::remove_public_husk`] then listed the directory again once the
/// failure had cleared and removed a committed run's public half,
/// `events.jsonl` included. That sequence was measured through the real census;
/// `engine::topology::startup::tests` records the measurement and why its
/// fixture cannot be committed as a test in that module.
///
/// A whole-process descriptor exhaustion is **not** that failure: `EMFILE`
/// refuses `run.lock` as well, [`super::is_running`] calls a lock it cannot
/// inspect held, and the census skips the husk before reaching this proof. The
/// refusal here is a second, independent point of the same rule, not a
/// restatement of that gate.
///
/// So a listing that did not answer is [`RetainReason::ListingUnreadable`]: no
/// token, nothing deleted, the husk reported with the error that stopped the
/// listing. It is the same fail-closed choice [`commit_record_proves_absence`]
/// makes one conjunct further down, and the cost of being wrong is the same
/// shape — a husk retained until an operator prunes it, against a committed run
/// that is gone.
fn unbound_shape(public: &Path) -> PrivateHalfOwnership {
    let names = match read_dir_names(public) {
        Ok(names) => names,
        Err(error) => {
            return PrivateHalfOwnership::Retained(RetainReason::ListingUnreadable {
                detail: format!("{}: {error}", public.display()),
            });
        }
    };
    match names.as_slice() {
        [] => PrivateHalfOwnership::NothingBound(UnboundShape::Bare),
        // Compared as an `OsStr`: the listing carries the name the filesystem
        // gave, and only an exact match is the staging file.
        [only] if only == OsStr::new(MARKER_STAGED) => {
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
