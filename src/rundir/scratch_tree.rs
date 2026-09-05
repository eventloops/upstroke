//! The second deletion-authority token, and it exists only in the test build.
//!
//! # Why there are two tokens rather than one
//!
//! `remove_private_husk` is reachable only with a `PrivateHalfProof`, which
//! is what `resource_accounting.completeness_rule` means by "a private-half
//! deletion outside the proof-token funnel fails to compile". That token is
//! about a **run's** private half: `prove_private_half_ownership` mints it
//! from a marker, a reciprocal owner record, and — conjunct 12, fail-closed —
//! the *proved* absence of `<private>/committed.json`.
//!
//! A test's scratch tree is not that. It is a directory the test created
//! moments earlier, holding fixtures the test itself wrote; no census reads it,
//! no run is bound to it, and reclaiming it is not a run-lifecycle effect.
//! Routing it through `PrivateHalfProof` would mean forging that token or
//! weakening conjunct 12 so that a fixture carrying a published
//! `committed.json` could be cleaned up — and a fixture that publishes one is
//! exactly the fixture the conjunct-12 tests need. Both are prohibited, and
//! both would trade a live production guarantee for test convenience.
//!
//! So run-directory deletion authority is token-carried with **exactly two
//! token classes**:
//!
//! | token | authorises | minted by | refuses |
//! |---|---|---|---|
//! | `PrivateHalfProof` | one run's private half, on the run-lifecycle paths | `prove_private_half_ownership` | every shape a `RetainReason` names, `committed.json` included |
//! | `ScratchTreeOwnership` | one scratch tree, in the test build only | [`acquire`] | an occupied root, and an undecidable one |
//!
//! Neither can be forged, cloned, defaulted or spent twice, and neither can
//! name a path its own minting did not bind. That is the completeness rule in
//! its two-token form, and `decisions/2026-08-30-test-scratch-tree-ownership.md`
//! is the record.
//!
//! # Why reclaiming a scratch tree needs no proof about its contents
//!
//! [`acquire`] **creates** the root it binds, with a non-recursive
//! `fs::create_dir` that fails if anything is already there. So the root did
//! not exist before the token did, and every byte beneath it was written after
//! the token was minted, by whoever holds the token. **Nothing under a token
//! root predates the token**, so a recursive removal of it cannot destroy
//! anything another party is entitled to — exclusivity and safety are
//! structural properties of the acquisition, not claims about the contents.
//!
//! Which is why this funnel does not stat for `committed.json` and must not: a
//! commit record inside a token root is a fixture the holder published there,
//! not a run's deletion boundary. The boundary is unmoved for every path that
//! reaches a run directory; see `remove_private_husk`, the
//! `RunDir.PublishCommitRecord` site, and `engine::topology::create`.
//!
//! # Not a production effect
//!
//! There is no `RunDirSite` variant here, no row in `effect_sites.json`, and no
//! censused site. `effect_site_inventory` is the inventory of the **engine's**
//! effects and nothing here runs in a build of the engine: the module is
//! `#[cfg(test)]`, so it is absent from the rlib altogether. That absence is
//! also why the compile-fixture harness beside `PrivateHalfProof`
//! (`tests::build_refusals`, which compiles against this crate's rlib) cannot
//! reach this type, and why the refusals it carries are written the way
//! `NoSecondToken` describes.
// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/rundir.rs`. The recursive deletion this module owns was
// reviewed into that file's row when the module was inline; it is out of line
// since W1 and carries a row and a level of its own.
//
// `PR6-LANEF-004`: a Rust lint level is scoped by the MODULE TREE and not by
// the file, so without an attribute here the parent's inner allow of all three
// would reach this file silently. `clippy::disallowed_macros` is RE-DENIED
// rather than inherited -- measured at zero sites, and deliberately so: the
// unwinding report path exists because `eprintln!` panics on a write error, so
// a print macro appearing in this file is a build error and not a style note.
// `decisions.effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(clippy::disallowed_macros)]

use std::fs;
use std::io;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use super::ownership::commit_record_proves_absence;

/// Ownership of exactly one scratch tree.
///
/// Not `Clone`, not `Copy`, not `Default`, and its field is private to this
/// module — so the only value of this type that exists anywhere is one
/// [`acquire`] returned, bound to the root that call created.
/// [`remove_scratch_tree`] takes it **by value**, so the call spends it and
/// it cannot authorise a second deletion. `NoSecondToken` is where all
/// three of those are made build failures rather than conventions.
#[derive(Debug)]
pub(crate) struct ScratchTreeOwnership {
    /// The exact root this token authorises removing: not a prefix of it,
    /// not a path handed to the removal call later.
    root: PathBuf,
}

impl ScratchTreeOwnership {
    /// The root this token binds, and the only path a reclaim can name.
    pub(crate) fn path(&self) -> &Path {
        &self.root
    }
}

/// Why an acquisition refused.
///
/// **Neither arm ever pre-cleans.** The helper this module replaces opened
/// with `let _ = fs::remove_dir_all(&dir)` on a path built from a tag and a
/// pid — it deleted whatever it found at a predictable location before
/// asking whether it had any claim on it. A refusal here is a refusal: the
/// occupant keeps its bytes and the caller gets no token.
#[derive(Debug)]
pub(crate) enum ScratchAcquireRefusal {
    /// Something is already at the root. `fs::create_dir` reports it as
    /// `AlreadyExists`, and the report is the kernel's exclusive create
    /// rather than a stat this code raced.
    Occupied { root: PathBuf },
    /// The filesystem declined to say. Every error that is not
    /// `AlreadyExists` lands here — the parent is a file, a component is
    /// missing, a permission is denied, Windows is holding a handle — and
    /// all of them read the same way: the acquisition did not happen, so
    /// there is no token and nothing was touched.
    Undecidable { root: PathBuf, source: io::Error },
}

impl ScratchAcquireRefusal {
    /// The root that was refused.
    pub(crate) fn root(&self) -> &Path {
        match self {
            Self::Occupied { root } | Self::Undecidable { root, .. } => root,
        }
    }

    /// What the filesystem said, when it said something this refusal did
    /// not already encode. `Occupied` *is* `AlreadyExists`, so it carries
    /// no separate error; every other answer keeps its own.
    pub(crate) fn source(&self) -> Option<&io::Error> {
        match self {
            Self::Occupied { .. } => None,
            Self::Undecidable { source, .. } => Some(source),
        }
    }
}

/// A reclaim that did not happen, carrying its token back.
///
/// The token is returned rather than dropped because the tree is still
/// there and somebody has to own it: a caller that gets the token back can
/// re-arm a guard over it ([`ScratchTree::rearm`]) and try again, and a
/// caller that drops the failure has made a decision rather than lost the
/// only handle by accident.
#[derive(Debug)]
pub(crate) struct ScratchReclaimFailure {
    token: ScratchTreeOwnership,
    source: io::Error,
}

impl ScratchReclaimFailure {
    /// Take the ownership token back out.
    pub(crate) fn into_token(self) -> ScratchTreeOwnership {
        self.token
    }

    /// The root the reclaim was for.
    pub(crate) fn root(&self) -> &Path {
        self.token.path()
    }

    /// What the filesystem said.
    pub(crate) fn source(&self) -> &io::Error {
        &self.source
    }
}

/// How a reclaim removes a tree.
///
/// A function pointer rather than a hook site: a deterministic failure is
/// needed by a few witnesses, and giving the engine a registration point
/// for it would put a production seam in the tree for a test's benefit.
/// Production has one remover, [`remove_tree`], and [`remove_scratch_tree`]
/// is the only way to reach it that does not name a remover.
///
/// **Module-private, and it stays that way.** A remover is an arbitrary
/// callback holding a reclaim's whole authority: it is handed the token's
/// root and decides what happens to it, so a crate-visible one could delete
/// an ancestor of the path it was given, ignore the path entirely and
/// delete something else, or return `Ok(())` having removed nothing — under
/// which last a caller's tree is reported reclaimed and silently leaks.
/// `PR78-SCRATCH-REMOVER-SEAM-AUTHORITY`. Outside witnesses get
/// [`refuse_to_reclaim`] instead, which names no path and cannot succeed.
type Remover = fn(&Path) -> io::Result<()>;

/// The real remover: the recursive deletion, under `src/rundir.rs`'s
/// existing reviewed allowance for raw filesystem primitives.
fn remove_tree(root: &Path) -> io::Result<()> {
    fs::remove_dir_all(root)
}

/// How the guard reports a reclaim it could not perform.
///
/// **Fallible, and that is the whole point.** This arm runs while the
/// thread is already unwinding, so anything that can panic here aborts the
/// process and destroys the diagnosis of whatever actually failed. `eprintln!`
/// can: `std::io::_eprint` panics on a write error — a closed or broken
/// stderr, a full pipe — and that panic is raised *during* the unwind.
/// `PR77-SCRATCH-UNWIND-REPORT-PANICS`. A reporter that returns its error
/// lets the caller decide, and on the unwinding path the decision is to
/// suppress it, explicitly and in writing.
type Reporter = fn(&str) -> io::Result<()>;

/// The real reporter: one line on stderr, with every failure returned
/// rather than raised.
///
/// Written through the handle rather than through `eprintln!` for exactly
/// the reason above — the macro turns a write error into a panic, and this
/// returns it.
fn report_to_stderr(message: &str) -> io::Result<()> {
    use std::io::Write as _;

    let mut stderr = io::stderr().lock();
    stderr.write_all(message.as_bytes())?;
    stderr.write_all(b"\n")?;
    stderr.flush()
}

/// Acquire a scratch tree under `parent`, named for `tag` and a fresh ULID.
///
/// **Fail-closed.** The root is created with a non-recursive, exclusive
/// `fs::create_dir`, so the call succeeds only if the parent was already a
/// directory and nothing was at the name. "Previously nonexistent" is
/// therefore decided by the kernel's own exclusive create rather than by a
/// stat this code could lose a race to, and every other answer refuses.
///
/// **No shared root, and no adopted one.** The name carries `tag` so that a
/// human reading a leftover tree can tell what left it, and a fresh ULID
/// because a tag is not enough: two processes, or two runs of one process,
/// collide on a tag-and-pid name, and colliding on a path somebody else is
/// using is precisely what made the old pre-clean destructive. The ULID is
/// distinct against this process's own draws and, by its millisecond,
/// against every earlier run's; it is **not unpredictable**, and not unique
/// across processes: `crate::ulid` is arithmetic over the clock, the pid
/// and a per-process nonce, so two processes can draw one value in one
/// millisecond for seed-equal pid-and-nonce pairs, and anything that knows
/// those three can compute the next name (PR #149 measured both). What the
/// exclusive create guarantees is that a shared name is refused, never
/// adopted.
///
/// # Errors
///
/// [`ScratchAcquireRefusal::Occupied`] if something is already at the root,
/// [`ScratchAcquireRefusal::Undecidable`] for every other answer. Neither
/// deletes, moves or truncates anything.
pub(crate) fn acquire(parent: &Path, tag: &str) -> Result<ScratchTree, ScratchAcquireRefusal> {
    acquire_named(
        parent,
        &format!("upstroke-scratch-{tag}-{}", crate::ulid::ulid()),
    )
}

/// [`acquire`] over an exact name.
///
/// **Private to this module, deliberately.** A caller that chose the name
/// could choose one somebody else owns, which is the whole hazard the ULID
/// closes; the crate's tests reach only [`acquire`]. It exists because the
/// one shape a fresh ULID makes unreachable from inside this process is the shape the occupied-root
/// witness has to arrange — a root that is already there.
fn acquire_named(parent: &Path, name: &str) -> Result<ScratchTree, ScratchAcquireRefusal> {
    let root = parent.join(name);
    match fs::create_dir(&root) {
        Ok(()) => Ok(ScratchTree {
            token: Some(ScratchTreeOwnership { root }),
            remove: remove_tree,
            report: report_to_stderr,
        }),
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            Err(ScratchAcquireRefusal::Occupied { root })
        }
        Err(source) => Err(ScratchAcquireRefusal::Undecidable { root, source }),
    }
}

/// Remove the tree this token names, and nothing else.
///
/// The path removed is the token's, never a caller's: there is no
/// path-taking variant of this call, so a reclaim cannot be aimed at an
/// ancestor, a sibling, or anything the acquisition did not create.
///
/// # Errors
///
/// [`ScratchReclaimFailure`], carrying the token back, for every answer
/// other than success and `NotFound`.
pub(crate) fn remove_scratch_tree(
    token: ScratchTreeOwnership,
) -> Result<(), ScratchReclaimFailure> {
    remove_scratch_tree_with(token, remove_tree)
}

/// [`remove_scratch_tree`] over an injectable remover.
///
/// The seam exists so that a witness can watch a reclaim fail
/// deterministically, on every platform, without arranging an unwritable
/// directory — which a caller in a `TOPOLOGY_MODULE` could not arrange in
/// any case, `fs` mutation being denied there.
///
/// **Private, for the reason [`acquire_named`] is**: it takes a [`Remover`],
/// and a caller that supplies one supplies the reclaim's whole authority.
/// The two are the same hazard from opposite ends — one lets a caller name
/// the path, the other lets it decide what naming the path means — and
/// neither is anything an outside witness needs.
fn remove_scratch_tree_with(
    token: ScratchTreeOwnership,
    remove: Remover,
) -> Result<(), ScratchReclaimFailure> {
    match remove(token.path()) {
        Ok(()) => Ok(()),
        // The one absence a filesystem proves — the same fail-closed
        // reading `commit_record_proves_absence` makes of conjunct 12's
        // stat. Every other error is an answer the filesystem declined to
        // give, so the token comes back and the tree stays somebody's.
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ScratchReclaimFailure { token, source }),
    }
}

/// Refuse to reclaim a token's tree, deterministically and on every host.
///
/// The one thing a witness outside this module may do to a reclaim, and
/// deliberately the smallest thing that suffices. It **cannot be aimed**:
/// there is no path parameter, and the root removed — or in this case not
/// removed — is the token's own. It **cannot be told what to do**: the
/// remover is a private `fn` item declared inside this one, stateless, with
/// no captured environment for a second call or a second thread to observe.
/// And it **cannot report success**: the return type is the failure itself,
/// so no caller can be handed an `Ok(())` for a tree that is still there.
///
/// What comes back is the genuine [`ScratchReclaimFailure`] the private
/// funnel built, carrying the token it handed back — so a caller reclaims
/// with [`ScratchReclaimFailure::into_token`] and [`ScratchTree::rearm`],
/// exactly as it would after a real refusal. Nothing here is fabricated.
///
/// `PR78-SCRATCH-REMOVER-SEAM-AUTHORITY`: this replaced a crate-visible
/// [`remove_scratch_tree_with`], under which any caller could pass a
/// remover that deleted an ancestor, ignored its argument, or returned
/// `Ok(())` having removed nothing.
pub(crate) fn refuse_to_reclaim(token: ScratchTreeOwnership) -> ScratchReclaimFailure {
    /// Removes nothing, refuses always, remembers nothing between calls.
    fn refuse(_root: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }

    match remove_scratch_tree_with(token, refuse) {
        Err(failure) => failure,
        // Unreachable by construction: `refuse` returns `PermissionDenied`
        // for every input, and the only error the funnel reads as success
        // is `NotFound`.
        Ok(()) => unreachable!("a refusing remover cannot reclaim a tree"),
    }
}

/// Report a reclaim that did not happen, on this subsystem's own reporter.
///
/// **Fallible, and every caller must decide.** The arm that needs this runs
/// while a thread is already unwinding, where a raised panic aborts the
/// process and destroys the diagnosis of whatever actually failed — so the
/// write error comes back rather than up, and suppressing it is a decision
/// the caller writes down. See [`report_to_stderr`], and
/// `PR77-SCRATCH-UNWIND-REPORT-PANICS`.
///
/// It takes the failure rather than a message so that every report of a
/// lost tree has one shape and names the root the token bound. A caller
/// outside this module has no other way to reach the reporter, and cannot
/// substitute one of its own: [`Reporter`] is not exported.
///
/// `PR78-EMIT-UNWIND-REPORT-LOST`: without this, a fixture whose unwinding
/// reclaim failed had nowhere to put its report except a slot that died
/// with it, so the leak was silent unless a witness happened to hold an
/// external handle on that slot.
///
/// `PR78-EMIT-UNWIND-REPORT-ORACLE`: whether a line handed here actually
/// crosses the process's stderr is asserted from **outside** the process —
/// the delivery witnesses below spawn the emit no-observer test as a child
/// of this same binary and read its fd 2 — because every in-process record
/// of the answer is written by the arm under test, and a reporter rewritten
/// to format the message and return `Ok(())` without writing certifies
/// itself to exactly that record.
pub(crate) fn report_reclaim_failure(failure: &ScratchReclaimFailure) -> io::Result<()> {
    report_to_stderr(&format!(
        "scratch tree {} was not reclaimed: {}",
        failure.root().display(),
        failure.source()
    ))
}

/// Absence, proved rather than assumed.
///
/// `Path::exists` answers `false` for "not there" **and** for every error a
/// stat can return — a permission denial, an IO error, a Windows sharing
/// violation — so a witness written on it reports a tree as reclaimed on
/// the evidence that the filesystem refused to answer. This is conjunct
/// 12's predicate, reached through conjunct 12's own function so the two
/// cannot drift: only `NotFound` is proof.
pub(crate) fn proves_absent(path: &Path) -> bool {
    commit_record_proves_absence(&fs::symlink_metadata(path))
}

/// A guard over an acquired tree: it reclaims on the normal return **and**
/// on an unwind, because a `Drop` runs on both.
///
/// That is the whole reason [`acquire`] hands back a guard rather than a
/// bare token. A test that reclaimed at the end of its body leaves the tree
/// behind on exactly the runs that matter — the failing ones — and a suite
/// that leaks a directory per failing fixture is how a build box runs out
/// of inodes while `df` still reports free space.
#[derive(Debug)]
pub(crate) struct ScratchTree {
    /// `Some` for the whole life of a guard. [`ScratchTree::disarm`] takes
    /// it and consumes the guard without running its `Drop`; `drop` takes
    /// it at the end. There is no other state.
    token: Option<ScratchTreeOwnership>,
    /// [`remove_tree`] for every guard [`acquire`] or [`ScratchTree::rearm`]
    /// returns. The field exists so that the `Drop` arm taken when a reclaim
    /// fails *during an unwind* — the one that must not raise a second panic
    /// — is reachable from a witness at all.
    remove: Remover,
    /// [`report_to_stderr`] for every guard [`acquire`] or
    /// [`ScratchTree::rearm`] returns. Injectable for the same reason: the
    /// suppression of a *reporting* failure on the unwinding path is a
    /// second thing that arm has to get right, and a witness cannot reach
    /// it by breaking the real stderr.
    report: Reporter,
}

impl ScratchTree {
    /// The root of the guarded tree.
    pub(crate) fn path(&self) -> &Path {
        match &self.token {
            Some(token) => token.path(),
            // Unreachable by construction: see the field's own note.
            None => unreachable!("a live scratch guard holds its token"),
        }
    }

    /// Hand the token over and stop guarding the tree.
    ///
    /// The caller owns the reclaim from here. **A witness that disarms in
    /// order to watch a reclaim fail must [`rearm`](Self::rearm) before it
    /// asserts anything**: between the disarm and the re-arm a failing
    /// assertion unwinds past a tree that nothing will remove.
    pub(crate) fn disarm(self) -> ScratchTreeOwnership {
        // The guard's own `Drop` must not run: it would reclaim the tree
        // the caller is taking ownership of.
        let mut guard = ManuallyDrop::new(self);
        match guard.token.take() {
            Some(token) => token,
            None => unreachable!("a live scratch guard holds its token"),
        }
    }

    /// Guard a token again — the fallback a witness arms before asserting.
    pub(crate) fn rearm(token: ScratchTreeOwnership) -> Self {
        Self {
            token: Some(token),
            remove: remove_tree,
            report: report_to_stderr,
        }
    }

    /// [`rearm`](Self::rearm) over an injectable remover **and an
    /// injectable reporter**.
    ///
    /// Module-private, for both reasons at once and one more:
    /// [`remove_scratch_tree_with`]'s, because it takes a [`Remover`]; and
    /// [`Reporter`]'s, because control over *how* a lost tree is reported
    /// belongs here — the suppressed arm of `Drop` is the only caller that
    /// has to get a fallible report right on an unwinding thread, and a
    /// caller that could substitute a reporter could make that arm silent.
    /// An outside witness needs neither: [`refuse_to_reclaim`] gives it a
    /// failing reclaim it cannot aim, [`report_reclaim_failure`] gives it
    /// the one report shape, and [`rearm`](Self::rearm) takes the token
    /// back. Like every item here it is `#[cfg(test)]`, so no production
    /// build contains it.
    fn guarded_with(token: ScratchTreeOwnership, remove: Remover, report: Reporter) -> Self {
        Self {
            token: Some(token),
            remove,
            report,
        }
    }
}

impl Drop for ScratchTree {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            // Unreachable: `disarm` consumes the guard through
            // `ManuallyDrop`, so no path reaches `drop` with the token
            // already gone. Returning is the safe direction regardless —
            // a bookkeeping slip must not be the thing that aborts a run.
            return;
        };
        let root = token.path().to_path_buf();
        match remove_scratch_tree_with(token, self.remove) {
            Ok(()) => {}
            Err(failure) => {
                if std::thread::panicking() {
                    // SUPPRESSED, and only here. A panic raised while the
                    // thread is already unwinding aborts the process, which
                    // would replace the failing assertion's report with
                    // nothing at all — the leak would cost the diagnosis of
                    // whatever actually failed.
                    //
                    // Reported rather than swallowed: this arm is why the
                    // reclaim's result is matched at all. `let _ =` or
                    // `.ok()` would read the same on this path and make a
                    // leaked tree silent on *every* path, including the one
                    // below.
                    //
                    // And the REPORT is fallible too, which is the second
                    // thing this arm has to get right. `eprintln!` panics on
                    // a write error, and a panic here is exactly the abort
                    // the suppression exists to avoid, so the reporter
                    // returns its error and this matches it. There is
                    // genuinely nothing to do with it — the channel that
                    // would carry a complaint is the one that just failed —
                    // but it is matched rather than discarded, so a future
                    // arm that *could* act has a place to be written.
                    // `PR77-SCRATCH-UNWIND-REPORT-PANICS`.
                    let message = format!(
                        "scratch tree {} was not reclaimed while unwinding: {}",
                        root.display(),
                        failure.source()
                    );
                    match (self.report)(&message) {
                        Ok(()) => {}
                        Err(_reporting_failed) => {}
                    }
                } else {
                    panic!(
                        "scratch tree {} was not reclaimed: {}",
                        root.display(),
                        failure.source()
                    );
                }
            }
        }
    }
}

/// The marker a refused duplication returns.
///
/// Its value carries nothing. What it proves is *which* item the call
/// resolved to: a `ScratchTreeOwnership` that gained a `Clone` or a
/// `Default` would not return this type.
#[derive(Debug, PartialEq, Eq)]
struct DuplicationRefused;

/// The refusals this token carries, in the only form a `cfg(test)` type
/// can carry them.
///
/// `PrivateHalfProof`'s equivalents are compile-fail fixtures compiled
/// against this crate's **rlib** — `tests::build_refusals`' `forged-token`,
/// `cloned-token`, `defaulted-token` and `spent-token`. That harness cannot
/// reach this type at all: a `#[cfg(test)]` item is not in the rlib, so a
/// fixture naming `ScratchTreeOwnership` fails to compile for an unresolved
/// path and would go green for a token that *was* `Clone`. A refusal that
/// passes for the wrong reason enforces nothing.
///
/// Measured, on this tree: a fixture returning
/// `upstroke::rundir::scratch_tree::ScratchTreeOwnership`, compiled against
/// this crate's rlib exactly as that harness compiles its own, reports
/// `E0433` — "cannot find `scratch_tree` in `rundir`" — with a note that the
/// item was configured out.
///
/// So the refusals move into the crate, where the compiler decides them on
/// every build of the test target:
///
/// * **Spent, because taken by value.** The coercion in
///   `a_spent_token_cannot_authorise_a_second_deletion` pins
///   [`remove_scratch_tree`] to `fn(ScratchTreeOwnership) -> …`. A
///   signature that took `&ScratchTreeOwnership` — under which one token
///   would authorise any number of deletions — does not satisfy that
///   coercion and does not build.
/// * **Not `Clone`, not `Copy`, not `Default`.** This trait gives the token
///   a second `clone` and a second `default`. Both resolve today because
///   the token implements neither of the std traits; the moment it
///   implements one, the call is `E0034` — multiple applicable items in
///   scope — and the crate stops compiling. `Copy` is caught by the first
///   of the two, because `Copy` requires `Clone`.
///
/// Neither refusal is a source scan. Both are the compiler's answer, on
/// every build, which is what "by API construction" has to mean if it is to
/// mean anything.
trait NoSecondToken {
    /// Shadow-refusal for `Clone::clone`.
    fn clone(&self) -> DuplicationRefused;
    /// Shadow-refusal for `Default::default`.
    fn default() -> DuplicationRefused;
}

impl NoSecondToken for ScratchTreeOwnership {
    fn clone(&self) -> DuplicationRefused {
        DuplicationRefused
    }

    fn default() -> DuplicationRefused {
        DuplicationRefused
    }
}

mod witnesses {
    use super::{
        DuplicationRefused, NoSecondToken, ScratchAcquireRefusal, ScratchReclaimFailure,
        ScratchTree, ScratchTreeOwnership, acquire, acquire_named, fs, io, proves_absent,
        remove_scratch_tree, remove_scratch_tree_with, report_to_stderr,
    };
    use std::path::Path;

    use crate::rundir::read_dir_names;

    /// Where a top-level scratch tree goes. The system temp directory is
    /// the parent every test in this crate already uses; what changes is
    /// that the *child* is now acquired rather than assumed.
    fn temp_parent() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    /// Witness 1 — an occupied root refuses, and the occupant keeps its
    /// bytes.
    ///
    /// This is the shape the replaced helper got wrong. It built a
    /// predictable path and opened with `let _ = fs::remove_dir_all(&dir)`,
    /// so a collision was resolved by destroying the other holder's tree
    /// and the destruction was unobservable — the discard is what made it
    /// silent. Here the collision is arranged deliberately, through the
    /// module-private name seam, and the assertion is on the occupant's
    /// content rather than on the refusal alone.
    #[test]
    fn an_occupied_root_refuses_and_leaves_what_it_found() {
        let parent = acquire(&temp_parent(), "occupied").expect("a parent tree");
        let name = "already-here";
        let occupied = parent.path().join(name);
        let nested = occupied.join("nested");
        fs::create_dir(&occupied).expect("the occupant's root");
        fs::create_dir(&nested).expect("and a directory inside it");
        fs::write(occupied.join("evidence.txt"), b"not mine to delete").expect("content");
        fs::write(nested.join("deeper.txt"), b"nor this").expect("nested content");

        let refusal = acquire_named(parent.path(), name).expect_err("an occupied root is refused");
        assert!(
            matches!(refusal, ScratchAcquireRefusal::Occupied { .. }),
            "an occupied root is `Occupied`, not something vaguer: {refusal:?}"
        );
        assert_eq!(
            refusal.root(),
            occupied,
            "the refusal names the root it refused"
        );

        // And nothing was pre-cleaned. A refusal that had "made room" would
        // pass every assertion above this one.
        assert_eq!(
            fs::read(occupied.join("evidence.txt")).expect("the occupant's file survives"),
            b"not mine to delete"
        );
        assert_eq!(
            fs::read(nested.join("deeper.txt")).expect("and so does the nested one"),
            b"nor this"
        );

        // The control: the same parent yields a tree when the name is free,
        // so the refusal above is about the occupancy and not about the
        // parent being unusable.
        let free = acquire(parent.path(), "free").expect("a free name is acquired");
        assert!(free.path().starts_with(parent.path()));
    }

    /// The ULID is load-bearing: the **same tag** twice beneath one live
    /// parent gets two distinct roots.
    ///
    /// `PR77-SCRATCH-ULID-WITNESS-ABSENT`. The name is `<tag>-<ULID>`, and
    /// nothing measured that the second half varies — so replacing
    /// `ulid::ulid()` with the process id, or with any constant, passed
    /// every witness in this module. It must not: two holders on one box
    /// colliding on a path is the hazard the whole token exists for, and a
    /// tag-and-pid name is precisely the shape the replaced helper had.
    ///
    /// Three assertions kill that mutation independently. The second
    /// `acquire` **succeeds** — under a constant suffix it would refuse the
    /// still-live first root as `Occupied`; the two roots **differ**; and
    /// each basename's suffix is **26 Crockford base32 characters**, which
    /// a pid string is not.
    #[test]
    fn the_same_tag_twice_gets_two_distinct_ulid_named_roots() {
        /// Crockford base32, the alphabet `crate::ulid` builds an id from.
        /// Restated here because that constant is private to that module
        /// and this witness may not widen its lease to publish it; the
        /// length and membership below are what a pid or a constant fails.
        const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        /// The tag is fixed, so the whole of the variation under test is
        /// what follows it.
        const PREFIX: &str = "upstroke-scratch-twice-";

        let parent = acquire(&temp_parent(), "same-tag").expect("a parent tree");

        // BOTH LIVE AT ONCE. The first guard is still holding its root when
        // the second acquisition runs, so a name that did not vary would be
        // refused here rather than returning a second tree.
        let first = acquire(parent.path(), "twice").expect("the first tree");
        let second = acquire(parent.path(), "twice")
            .expect("a second acquisition with the same tag must not collide with the first");

        assert_ne!(
            first.path(),
            second.path(),
            "one tag produced one root twice; a colliding name is the hazard the token \
                 exists for"
        );
        assert_eq!(
            read_dir_names(parent.path()).len(),
            2,
            "both roots must be present at once, or the second acquisition took the \
                 first's name"
        );

        let mut suffixes = Vec::new();
        for tree in [&first, &second] {
            let name = tree
                .path()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .expect("an acquired root has a basename");
            let suffix = name
                .strip_prefix(PREFIX)
                .unwrap_or_else(|| panic!("{name} is not `{PREFIX}<ULID>`"))
                .to_owned();
            assert_eq!(
                suffix.len(),
                26,
                "a ULID is 26 characters; `{suffix}` is not one"
            );
            assert!(
                suffix.bytes().all(|byte| CROCKFORD.contains(&byte)),
                "`{suffix}` is not Crockford base32"
            );
            // The named mutation, refused by name as well as by shape.
            assert_ne!(
                suffix,
                std::process::id().to_string(),
                "the varying half of the name is the process id, which every holder on \
                     this box shares"
            );
            suffixes.push(suffix);
        }
        assert_ne!(
            suffixes[0], suffixes[1],
            "two acquisitions drew the same id: {suffixes:?}"
        );
    }

    /// Witness 2 — an undecidable root refuses.
    ///
    /// A file where the parent directory would have to be. The kernel's
    /// answer differs by platform — `ENOTDIR` here, `ERROR_DIRECTORY` or
    /// `ERROR_PATH_NOT_FOUND` on Windows — and the classification does not
    /// depend on which: anything that is not `AlreadyExists` is an answer
    /// the acquisition cannot act on, so it refuses and touches nothing.
    ///
    /// What this promises is exactly that — the **acquisition's** refusal
    /// and its non-modification. It makes no claim about how a stat
    /// *beneath* the refused root classifies; that predicate is
    /// `a_commit_record_stat_that_is_not_not_found_is_not_proof_of_absence`'s
    /// subject, asserted there directly over every shape the stat can
    /// produce.
    #[test]
    fn an_undecidable_root_refuses_without_claiming_it_was_occupied() {
        let parent = acquire(&temp_parent(), "undecidable").expect("a parent tree");
        let not_a_dir = parent.path().join("a-file");
        fs::write(&not_a_dir, b"a file, not a directory").expect("the file");

        let refusal = acquire(&not_a_dir, "under-a-file")
            .expect_err("a root that cannot be created is refused");
        assert!(
            matches!(refusal, ScratchAcquireRefusal::Undecidable { .. }),
            "an answer that is not `AlreadyExists` must not be reported as occupancy: \
                 {refusal:?}"
        );
        assert!(
            refusal.root().starts_with(&not_a_dir),
            "the refusal names the root it tried: {:?}",
            refusal.root()
        );
        assert_ne!(
            refusal.source().map(io::Error::kind),
            Some(io::ErrorKind::AlreadyExists),
            "`AlreadyExists` is the one answer that is occupancy; nothing else may be \
                 filed as undecidable while reporting it"
        );
        assert_eq!(
            fs::read(&not_a_dir).expect("the file is still a file"),
            b"a file, not a directory",
            "a refusal must not have written over what it found"
        );
        // Nothing was created — read from the parent's own directory
        // listing, which every platform answers the same way. A stat under
        // the refused root is deliberately NOT the oracle here: what a
        // filesystem reports for a path beneath a file ancestor is
        // platform-dependent, and asserting on it made this witness
        // non-portable (`PR77-WIN-UNDECIDABLE-STAT-ORACLE` — the Windows
        // guest maps that stat to `NotFound`).
        assert_eq!(
            read_dir_names(parent.path()),
            ["a-file"],
            "a refused acquisition left something behind in the parent"
        );
    }

    /// Witness 3 — reclaiming a root that is not there succeeds.
    ///
    /// Both tokens below are minted by [`acquire`], so neither names a path
    /// this module invented: the inner root is removed by the outer
    /// reclaim, and the inner token is then spent against a root the
    /// filesystem says is gone. `NotFound` is the one answer that proves
    /// absence, and it is the one answer treated as success.
    #[test]
    fn reclaiming_a_root_that_is_not_there_succeeds() {
        let outer = acquire(&temp_parent(), "outer").expect("the outer tree");
        let inner = acquire(outer.path(), "inner").expect("the inner tree");
        let inner_token = inner.disarm();
        let inner_root = inner_token.path().to_path_buf();
        assert!(inner_root.starts_with(outer.path()));

        remove_scratch_tree(outer.disarm()).expect("the outer tree is reclaimed");
        assert!(
            proves_absent(&inner_root),
            "and it took the inner root with it"
        );

        remove_scratch_tree(inner_token)
            .expect("a root that is already gone is not a reclaim failure");
    }

    /// A remover that fails without touching the filesystem: what the
    /// witnesses using it are about is the funnel's answer and the guard's,
    /// not the kernel's.
    fn refuses(_root: &Path) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected"))
    }

    /// Witness 5 — an injected reclaim failure returns the error *and* the
    /// token.
    ///
    /// The token is what makes the failure recoverable: a funnel that
    /// dropped it would leave the tree with no owner and no handle, which
    /// is the same leak as a silent failure with an extra step.
    #[test]
    fn an_injected_reclaim_failure_returns_the_token_with_the_error() {
        let tree = acquire(&temp_parent(), "injected-failure").expect("a tree");
        let root = tree.path().to_path_buf();
        fs::write(root.join("fixture.txt"), b"still here").expect("content");

        // ARM THE FALLBACK BEFORE ASSERTING. From the `disarm` inside this
        // expression to the `rearm` below, nothing guards the tree — so no
        // assertion runs in between, and the token goes straight back under
        // a guard that will reclaim it however this test ends.
        let (tree, kind, reported) = match remove_scratch_tree_with(tree.disarm(), refuses) {
            Err(failure) => {
                let kind = failure.source().kind();
                let reported = failure.root().to_path_buf();
                (ScratchTree::rearm(failure.into_token()), kind, reported)
            }
            // The token is consumed by a success, so there is nothing left
            // to re-arm; this arm is reachable only from a funnel that
            // reported a reclaim it did not perform.
            Ok(()) => panic!("an injected failure was reported as a successful reclaim"),
        };

        assert_eq!(
            kind,
            io::ErrorKind::PermissionDenied,
            "the error is carried out whole"
        );
        assert_eq!(reported, root, "the failure names the root it was for");
        assert_eq!(
            fs::read(root.join("fixture.txt")).expect("the tree is untouched"),
            b"still here"
        );
        assert!(!proves_absent(&root), "a failed reclaim removes nothing");

        // And the re-armed guard is a real guard: this is the normal-return
        // half of "reclaims on normal return and on unwind".
        drop(tree);
        assert!(
            proves_absent(&root),
            "the re-armed guard reclaimed the tree"
        );
    }

    /// The guard reclaims on an **unwind**, which is the exit that matters.
    ///
    /// A test that reclaimed at the end of its body leaks on exactly the
    /// runs worth diagnosing — the failing ones — and a suite that leaks one
    /// directory per failing fixture is how a build box runs out of inodes
    /// with free space still on the disk.
    #[test]
    fn a_guard_reclaims_on_an_unwind_as_well_as_on_a_normal_return() {
        let acquired: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
        let outcome = std::panic::catch_unwind(|| {
            let tree = acquire(&temp_parent(), "unwind").expect("a tree");
            *acquired.lock().expect("the record") = Some(tree.path().to_path_buf());
            fs::write(
                tree.path().join("fixture.txt"),
                b"written before the failure",
            )
            .expect("content");
            panic!("the failure the guard has to survive");
        });
        assert!(outcome.is_err(), "the fixture must actually unwind");

        let root = acquired
            .lock()
            .expect("the record")
            .take()
            .expect("the guard was built before the panic");
        assert!(
            proves_absent(&root),
            "an unwind past a guard left {} behind",
            root.display()
        );
    }

    /// A reclaim that fails on the **normal** path is raised.
    ///
    /// The measured surviving mutation this exists for: with the `panic!`
    /// replaced by the same `eprintln!` the suppressed arm uses, every other
    /// witness in this module still passed. A guard that reported a leak
    /// only on stderr would leave a suite green while it filled a build
    /// box — so the message is asserted too, because a panic that did not
    /// name the tree is not a diagnosis.
    #[test]
    fn a_reclaim_failure_on_the_normal_path_is_raised() {
        let outer = acquire(&temp_parent(), "raised").expect("the outer tree");
        let inner = acquire(outer.path(), "inner").expect("the inner tree");
        let root = inner.path().to_path_buf();

        let payload = std::panic::catch_unwind(|| {
            drop(ScratchTree::guarded_with(
                inner.disarm(),
                refuses,
                report_to_stderr,
            ));
        })
        .expect_err("a reclaim that did not happen must not return quietly");
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(
            message.contains("was not reclaimed"),
            "the panic must say what happened: {message:?}"
        );
        assert!(
            message.contains(&root.display().to_string()),
            "the panic must name the tree that leaked: {message:?}"
        );
        assert!(
            !proves_absent(&root),
            "the injected remover performed nothing"
        );

        let outer_root = outer.path().to_path_buf();
        drop(outer);
        assert!(proves_absent(&outer_root));
        assert!(proves_absent(&root));
    }

    /// A **reporting** failure during an unwind is suppressed too.
    ///
    /// `PR77-SCRATCH-UNWIND-REPORT-PANICS`. The suppressed arm used
    /// `eprintln!`, which panics when the write fails — a closed stderr, a
    /// broken pipe — and that panic is raised while the thread is already
    /// unwinding, so it aborts. The arm that exists to protect the
    /// diagnosis destroyed it on exactly the hosts where stderr is not a
    /// terminal.
    ///
    /// Both failures are injected here at once: the reclaim fails, and so
    /// does the report of that failure. The witness is the process still
    /// being alive to run the assertions — under a raising reporter this
    /// test takes the whole binary with it and nothing after it reports.
    /// The counter proves the reporter was actually reached, so a guard
    /// that stopped reporting altogether is red rather than green.
    #[test]
    fn a_reporting_failure_while_already_unwinding_is_suppressed_too() {
        thread_local! {
            /// How many times this thread's injected reporter was called.
            /// Per-thread, so a parallel harness never crosses two
            /// witnesses.
            static REPORTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }

        /// Records the call, then fails the way a closed stderr does.
        fn refuses_to_report(_message: &str) -> io::Result<()> {
            REPORTS.with(|calls| calls.set(calls.get() + 1));
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected"))
        }

        let outer = acquire(&temp_parent(), "report-failure").expect("the outer tree");
        let inner = acquire(outer.path(), "inner").expect("the inner tree");
        let root = inner.path().to_path_buf();
        REPORTS.with(std::cell::Cell::take);

        let outcome = std::panic::catch_unwind(|| {
            let _guard = ScratchTree::guarded_with(inner.disarm(), refuses, refuses_to_report);
            panic!("the failure being diagnosed, which both suppressions preserve");
        });
        assert!(outcome.is_err(), "the fixture must actually unwind");
        assert_eq!(
            REPORTS.with(std::cell::Cell::get),
            1,
            "the guard did not reach the reporter, so this witness proves nothing about \
                 what happens when the reporter fails"
        );
        assert!(
            !proves_absent(&root),
            "the injected remover performed nothing, so the tree is still there"
        );

        // Reached at all, which is the assertion: a raised reporting failure
        // aborts, and no line below here executes.
        let outer_root = outer.path().to_path_buf();
        drop(outer);
        assert!(proves_absent(&outer_root));
        assert!(proves_absent(&root));
    }

    /// A reclaim that fails **while the thread is already unwinding** is
    /// suppressed, not raised.
    ///
    /// A panic raised during an unwind aborts the process. The witness is
    /// therefore the process still being here to run the assertions below:
    /// under the other arm this test would take the whole binary with it and
    /// no later test would report at all. The inner tree is nested inside an
    /// outer acquired one, so the tree the injected remover refuses to touch
    /// is still reclaimed — by the outer guard, on this test's own exit.
    #[test]
    fn a_reclaim_failure_while_already_unwinding_is_suppressed() {
        let outer = acquire(&temp_parent(), "suppressed").expect("the outer tree");
        let inner = acquire(outer.path(), "inner").expect("the inner tree");
        let root = inner.path().to_path_buf();

        let outcome = std::panic::catch_unwind(|| {
            let _guard = ScratchTree::guarded_with(inner.disarm(), refuses, report_to_stderr);
            panic!("the failure being diagnosed, which the suppressed arm preserves");
        });
        assert!(outcome.is_err(), "the fixture must actually unwind");
        assert!(
            !proves_absent(&root),
            "the injected remover performed nothing, so the tree is still there"
        );

        // And the outer guard takes it on this test's own exit, which is why
        // a suppressed failure is not a leak here.
        let outer_root = outer.path().to_path_buf();
        drop(outer);
        assert!(proves_absent(&outer_root));
        assert!(proves_absent(&root));
    }

    // -------------------------------------------------------------------
    // Delivery of the unwinding report, observed from outside the process
    //
    // `PR78-EMIT-UNWIND-REPORT-ORACLE`. The emit fixtures' unwinding arm
    // hands its line to `report_reclaim_failure`, and every in-process
    // witness over that arm can read only the record the same branch
    // writes — so a reporter rewritten to format the message and return
    // `Ok(())` without touching stderr certified itself and stayed green
    // while external reporting regressed to nothing. The observation
    // cannot live beside those witnesses: `src/engine/topology/**` is a
    // `TOPOLOGY_MODULE`, and watching fd 2 takes a subprocess, which
    // takes `Command`, a denied effect there. This module is the
    // reporter's home and carries the reviewed allowance, so the
    // witnesses live here: each spawns one emit test out of this same
    // binary as a child process and reads what actually crossed the
    // child's own stderr, where no arm of the code under test can reach.
    // -------------------------------------------------------------------

    /// The emit no-observer witness, named the way the harness names it:
    /// the child whose report crossing fd 2 is the whole subject.
    const EMIT_NO_OBSERVER_TEST: &str = "engine::topology::emit::tests::\
             an_unwinding_reclaim_failure_reports_with_no_observer_on_the_slot";

    /// The emit unwind witness whose reclaims all succeed: the silence
    /// control for the delivery witness.
    const EMIT_SILENT_UNWIND_TEST: &str = "engine::topology::emit::tests::\
             a_guard_reclaims_on_an_unwind_as_well_as_on_a_normal_return";

    /// One test out of this same binary, run to completion as a child
    /// process, with its stderr wired however the witness needs it.
    ///
    /// `RUST_TEST_NOCAPTURE` is scrubbed so the child's harness keeps its
    /// default capture: the induced panic's hook output stays in the
    /// harness's buffer, and the only bytes on the child's fd 2 are the
    /// ones written through a real stderr handle — the channel under
    /// observation.
    fn one_test_as_a_child(name: &str, stderr: std::process::Stdio) -> std::process::Output {
        std::process::Command::new(std::env::current_exe().expect("the test executable"))
            .args(["--exact", name])
            .env_remove("RUST_TEST_NOCAPTURE")
            .stdout(std::process::Stdio::piped())
            .stderr(stderr)
            .output()
            .expect("run one test as a child process")
    }

    /// The child's two streams, with "it passed" and "it really ran
    /// `name`" already asserted.
    ///
    /// The second assertion is the vacuity guard: `--exact` with a name
    /// that has drifted matches nothing, and a child that ran no test
    /// exits 0 with exactly the silent stderr a delivery witness would
    /// misread as an answer.
    fn passed_child_streams(name: &str, output: &std::process::Output) -> (String, String) {
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            output.status.success(),
            "the child test failed ({}):\n--- child stdout ---\n{stdout}\n\
                 --- child stderr ---\n{stderr}",
            output.status
        );
        assert!(
            stdout.contains(name),
            "`--exact {name}` matched nothing — the name has drifted, so the child ran \
                 no test at all:\n{stdout}"
        );
        (stdout, stderr)
    }

    /// The unobserved fixture's unwinding report reaches the process's
    /// **real stderr** — asserted from outside the process.
    ///
    /// `PR78-EMIT-UNWIND-REPORT-ORACLE`, and this is the witness that
    /// closes it: the child is the emit no-observer test itself, so the
    /// whole production-equivalent chain runs — the fixture's unwinding
    /// arm, [`super::report_reclaim_failure`], [`super::report_to_stderr`],
    /// the write on the real handle — and the line is read back off the
    /// child's actual fd 2. A reporter rewritten to format the message
    /// and return `Ok(())` without writing, an arm that stops calling the
    /// reporter, or a write moved off stderr each leave this channel
    /// empty and read red here, whatever the child's own records say.
    ///
    /// The child passes on its own, and with the harness's capture
    /// holding the induced panic's hook output, its fd 2 carries the
    /// reporter's line and nothing besides. Exactly one reclaim is
    /// refused in that child, so exactly one report may cross, and it
    /// must name the tree acquired under the child's `unobserved` tag: a
    /// line that appears without the failure, or twice, or for another
    /// tree is a different defect and reads red rather than green.
    #[test]
    fn an_unobserved_unwind_report_reaches_the_process_stderr() {
        let output = one_test_as_a_child(EMIT_NO_OBSERVER_TEST, std::process::Stdio::piped());
        let (_stdout, stderr) = passed_child_streams(EMIT_NO_OBSERVER_TEST, &output);

        assert_eq!(
            stderr.matches("was not reclaimed").count(),
            1,
            "the child's one refused reclaim must put exactly one report on the real \
                 stderr:\n--- child stderr ---\n{stderr}"
        );
        assert!(
            stderr.contains("scratch tree "),
            "the line is the reporter's own shape: {stderr:?}"
        );
        assert!(
            stderr.contains("upstroke-scratch-unobserved-"),
            "the line names the refused tree itself — the one acquired under the \
                 child's `unobserved` tag: {stderr:?}"
        );
    }

    /// A reclaim that succeeded reports nothing: the silence control.
    ///
    /// Without this, the delivery witness above could be green on ambient
    /// noise — an arm that reported unconditionally, on success and
    /// failure alike, would put its line on every child's stderr and
    /// presence would stop meaning "the failure was reported". This child
    /// runs the same unwinding drop path with reclaims that all succeed,
    /// so its stderr must carry no report at all.
    #[test]
    fn a_successful_unwind_reclaim_reports_nothing_to_stderr() {
        let output = one_test_as_a_child(EMIT_SILENT_UNWIND_TEST, std::process::Stdio::piped());
        let (_stdout, stderr) = passed_child_streams(EMIT_SILENT_UNWIND_TEST, &output);

        assert!(
            !stderr.contains("was not reclaimed"),
            "a reclaim that succeeded has nothing to report; a line here is ambient \
                 noise:\n--- child stderr ---\n{stderr}"
        );
    }

    /// A stderr with no space fails the **real** report, and the
    /// suppression holds: the child neither aborts nor loses its panic.
    ///
    /// The reporter's failure arm was reachable before only through an
    /// injected reporter. Here [`super::report_to_stderr`] itself fails:
    /// the child's fd 2 is `/dev/full`, whose every write returns
    /// `ENOSPC` — deterministically, with no timing, no signal handling,
    /// and no seam. The child is the same no-observer test: its own
    /// assertions prove the induced panic came back out unreplaced and
    /// the outer guard still reclaimed, so a suppression rewritten to
    /// raise — an abort, on an unwinding thread, measured as exactly that
    /// on this tree — or to swap payloads is a failing child here.
    ///
    /// **Why a full device rather than a closed or read-only one, and why
    /// only Linux.** A descriptor that cannot be written at all is not a
    /// failing stderr to std: `handle_ebadf` in `std::io::stdio` defines
    /// `EBADF` on the standard streams as ignorable success, so a child
    /// given a read-only fd 2 makes both write syscalls, takes `EBADF` on
    /// each — measured on this tree under strace — and its reporter still
    /// returns `Ok`. The one portable-API way to make the real write
    /// *return its error* is a device that accepts the descriptor and
    /// refuses the bytes, and `/dev/full` is that device; the other hosts
    /// have no equivalent reachable without new dependencies, so this
    /// control runs on the Linux leg of the matrix and the suppression
    /// logic it exercises is host-independent Rust either way.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_full_stderr_fails_the_real_report_and_the_suppression_holds() {
        let full = fs::File::options()
            .write(true)
            .open("/dev/full")
            .expect("open the always-ENOSPC device for writing");

        let output = one_test_as_a_child(EMIT_NO_OBSERVER_TEST, std::process::Stdio::from(full));
        passed_child_streams(EMIT_NO_OBSERVER_TEST, &output);
    }

    /// Witness 6 — a spent token cannot authorise a second deletion, by API
    /// construction.
    ///
    /// Three compiler-decided facts, none of them a source scan:
    ///
    /// 1. the coercion below pins `remove_scratch_tree` to a by-value
    ///    parameter, so the call spends the token;
    /// 2. `token.clone()` resolves, which it can only do while the token is
    ///    not `Clone` — and `Copy` requires `Clone`;
    /// 3. `ScratchTreeOwnership::default()` resolves, which it can only do
    ///    while the token is not `Default`.
    ///
    /// Adding any of the three impls makes this function fail to compile.
    /// The `assert_eq!`s below are what proves the calls went to the
    /// refusals rather than to a std trait: only the refusals return
    /// [`DuplicationRefused`].
    ///
    /// Measured on this tree: `#[derive(Clone)]` and `#[derive(Default)]`
    /// on the token each produce `E0034`, and a `&ScratchTreeOwnership`
    /// parameter on `remove_scratch_tree` produces `E0308`. All three stop
    /// the build rather than failing a test, which is the point.
    #[test]
    fn a_spent_token_cannot_authorise_a_second_deletion() {
        let spend: fn(ScratchTreeOwnership) -> Result<(), ScratchReclaimFailure> =
            remove_scratch_tree;

        let tree = acquire(&temp_parent(), "spent-token").expect("a tree");
        let root = tree.path().to_path_buf();
        assert!(!proves_absent(&root), "the acquisition created the root");

        let token = tree.disarm();
        // Nothing between the disarm and the spend can unwind: every step
        // is total, so the tree is never stranded by a failing assertion.
        let refused_clone = token.clone();
        let refused_default = ScratchTreeOwnership::default();
        let leftover = match spend(token) {
            Ok(()) => None,
            Err(failure) => Some(ScratchTree::rearm(failure.into_token())),
        };

        assert_eq!(
            refused_clone, DuplicationRefused,
            "`clone` on a token resolved to something other than the refusal"
        );
        assert_eq!(
            refused_default, DuplicationRefused,
            "`default` on the token type resolved to something other than the refusal"
        );
        assert!(leftover.is_none(), "the spend did not reclaim the tree");
        assert!(
            proves_absent(&root),
            "one token, one deletion, and it happened"
        );
    }
}
