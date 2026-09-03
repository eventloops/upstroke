//! Path hygiene: reparse points, the verbatim prefix, and the canonical
//! comparisons containment is decided by.
//!
//! `DESIGN.md` §15 creates an execution root only when the managed base is a
//! real directory and the chain from the authorized private root down to the
//! root carries no symlink, reparse point or regular file, and every
//! containment answer in the parent is a comparison of two
//! [`canonical_prefix`] results. Those predicates are here; the revalidation
//! that calls them before each effect funnel and again inside it, and every
//! effect it guards, is the parent's.
//!
//! **Read-only, and that is why it can be a child.** `fs::symlink_metadata` and
//! `fs::canonicalize` observe; neither is a governed primitive, and no function
//! here creates, renames, or removes anything.

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

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::UpstrokeError;

use super::Refusal;

/// Whether `error` says the path names nothing, for the peel and the leaf
/// check.
///
/// `NotFound` is the obvious half. `NotADirectory` is the other: a path that
/// runs through a regular file names nothing either, so the peel treats it as
/// a prefix to peel past and the leaf check as "not a real directory". The
/// reparse-point walk does **not** read this: it meets the file itself, one
/// component earlier, and reports it there ([`reparse_point_below`]).
/// Everything else — permission denied, a link loop, a name the filesystem
/// cannot represent, transient I/O — is a failure and stays one: only an
/// actual not-found becomes absence.
fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// Whether `metadata` describes a symlink, junction, or any other reparse
/// point.
///
/// **Windows and Unix answer different questions on purpose.** On Unix the only
/// such object is a symbolic link. On Windows the set is larger — a directory
/// junction (`mklink /J`) and a mount point are reparse points that are *not*
/// symbolic links, and `FileType::is_symlink` answers true only for the
/// name-surrogate tags. `DESIGN.md` §15 names the symlink and the reparse
/// point together, and the retired v0.2 workspace decision spelt the refusal
/// as "symlink/**junction** on the chain", so the Windows half reads the raw attribute
/// (`FILE_ATTRIBUTE_REPARSE_POINT`) instead, which is true for every reparse
/// point whatever its tag. A refusal that fired only on POSIX symlinks would
/// pass every Linux test and refuse nothing a Windows operator can build.
#[cfg(windows)]
pub(super) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// See the Windows half for why the two differ.
#[cfg(not(windows))]
pub(super) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// The components of `path` below `anchor`, when `path` is `anchor` followed
/// by plain components and nothing else.
///
/// `None` when the two share no prefix, and when the remainder carries a
/// prefix, a root or `..`. Such a path has no chain below the anchor to
/// walk, and the walk must not answer for one. `strip_prefix` is lexical: a
/// `..` in the remainder passes it while climbing straight back out of the
/// anchor, which is why the components are checked and not only the prefix.
/// A `.` never reaches here: `components()` folds a non-leading `.` away, so
/// a run id of `.` would alias the repo-key directory unseen, and the run id
/// is refused at [`WorkspaceManager::derive`](super::WorkspaceManager::derive)
/// before any path is built.
fn plain_chain_below<'a>(anchor: &Path, path: &'a Path) -> Option<Vec<&'a OsStr>> {
    let Ok(relative) = path.strip_prefix(anchor) else {
        return None;
    };
    relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect()
}

/// The first component of `anchor` joined with `chain` that is a reparse
/// point, if any.
///
/// # Why the walk is anchored
///
/// `DESIGN.md` §15 requires no symlink or reparse point on the chain, and a
/// chain has to start somewhere. It starts at the operator's own authorized
/// root, canonicalized: §15 records the execution root as
/// `<private root>/workspaces/<repo-key>/<run-id>`, so the private root is
/// the anchor and everything below it is the run's. The root is resolved at
/// `derive` and re-examined at every
/// revalidation by [`refuse_reparse_points`]: it must still be a real
/// directory and still canonicalize to itself, or the chain has moved under
/// the run. What must be reparse-free is everything the run itself builds
/// beneath it.
///
/// The unanchored reading was tried and is wrong on a real platform, not just
/// inconvenient: macOS ships `/var` as a symlink to `private/var` and its
/// `$TMPDIR` lives under it, so an operator whose private root is anywhere
/// under `/var` — including every default temporary directory on that OS —
/// would have every run refused for a link they did not create and cannot
/// remove. No live passage asks for that, and the containment the refusal
/// exists to protect is unaffected. The subsystem's two recursive deletions
/// are the parent's: a worktree's tree, which goes through
/// [`WorkspaceManager::contained`](super::WorkspaceManager::contained) and so
/// compares **canonical** paths, and that worktree's Git admin entry, which
/// `revalidate_removal` bound to the same slot byte-for-byte before anything
/// was deleted. A resolved link cannot carry either outside the root. The
/// parent's other removals never recurse — `remove_dir` on the root and its
/// own empty scaffolding, `remove_file` on an intent, and on the `locked`
/// marker inside that admin entry — but a non-recursive removal still
/// follows a link in its *parent*, so every effect's in-funnel check walks
/// this chain down to the directory the effect acts in, and an exchanged
/// `intents/` or `tasks/` refuses exactly as an exchanged root does.
///
/// Only components that exist are inspected: a root that has not been created
/// yet has an absent leaf, and refusing on absence would refuse every first
/// run. A regular file on the chain is **not** absence. Nothing below it can
/// ever exist, so walking past it hands the failure to whichever effect comes
/// next — or to none: `remove_execution_root` asks `exists()`, which folds
/// the file into "nothing to remove". The walk reports the file where it
/// stands instead, as `NotADirectory` at its own path, and reports it the
/// same on every platform: it reads the component's type rather than waiting
/// for the `ENOTDIR` only Unix raises one component later. `chain` is plain
/// components by construction ([`plain_chain_below`]), so the walk has no
/// root to skip and no `..` to climb.
///
/// # Errors
///
/// A component that exists but cannot be read, or that is a regular file,
/// with its path.
fn reparse_point_below(anchor: &Path, chain: &[&OsStr]) -> Result<Option<PathBuf>, UpstrokeError> {
    let mut walked = anchor.to_path_buf();
    for name in chain {
        walked.push(name);
        match fs::symlink_metadata(&walked) {
            Ok(metadata) if is_reparse_point(&metadata) => return Ok(Some(walked)),
            Ok(metadata) if !metadata.is_dir() => {
                return Err(UpstrokeError::Io {
                    path: walked,
                    source: io::Error::from(io::ErrorKind::NotADirectory),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(UpstrokeError::Io {
                    path: walked,
                    source,
                });
            }
        }
    }
    Ok(None)
}

/// Refuse `path` unless `anchor` is still the real directory it was resolved
/// as and `path`'s chain below it is plain components with no reparse point
/// among them.
///
/// `path` is the execution root and `anchor` the authorized private root at
/// both call sites, and the refusals name them so.
///
/// **The anchor is examined too, not only what hangs from it.** The walk
/// below starts by pushing the first child, so an anchor replaced after
/// `derive` — renamed away and a link planted in its place — was never read,
/// every component under it was read *through* the link, and
/// `canonical_prefix` then resolved the execution root under the link's
/// target with nothing to compare it against. So the anchor must still be a
/// real directory, refused as [`Refusal::BaseIsNotADirectory`] otherwise,
/// the answer `derive` gives, and it must still canonicalize to itself: the
/// anchor is stored canonical, so any difference means a link now sits on
/// its own chain — above it, where the anchored walk never looks — and that
/// is refused as [`Refusal::ReparsePointOnChain`] naming the anchor.
///
/// A path with no plain chain below the anchor — no common prefix, or a
/// prefix, a root or `..` in the remainder — is refused rather than walked:
/// the walk's answer for it would be "no reparse point below the anchor",
/// true of a chain it never inspected. `derive` refuses a run id of that
/// shape before it builds a path, so this arm is the walk's own guarantee
/// behind that one.
///
/// # Errors
///
/// [`Refusal::BaseIsNotADirectory`], [`Refusal::RootOutsidePrivateRoot`],
/// [`Refusal::ReparsePointOnChain`], or an I/O error: the walk's own, which
/// already names the component it could not read and is propagated as it
/// is, or the anchor's resolution failing for a reason other than absence.
pub(super) fn refuse_reparse_points(anchor: &Path, path: &Path) -> Result<(), UpstrokeError> {
    refuse_unreal_directory(anchor)?;
    let resolved = match fs::canonicalize(anchor) {
        Ok(resolved) => resolved,
        // A real directory a moment ago and gone now is the same refusal as
        // never having been one; only a failure to resolve it is an error.
        Err(error) if is_absent(&error) => {
            return Err(Refusal::BaseIsNotADirectory {
                path: anchor.to_path_buf(),
            }
            .into());
        }
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: anchor.to_path_buf(),
                source,
            });
        }
    };
    if strip_verbatim(resolved) != anchor {
        return Err(Refusal::ReparsePointOnChain {
            chain: path.to_path_buf(),
            at: anchor.to_path_buf(),
        }
        .into());
    }
    let Some(chain) = plain_chain_below(anchor, path) else {
        return Err(Refusal::RootOutsidePrivateRoot {
            root: path.to_path_buf(),
            private_root: anchor.to_path_buf(),
        }
        .into());
    };
    if let Some(at) = reparse_point_below(anchor, &chain)? {
        return Err(Refusal::ReparsePointOnChain {
            chain: path.to_path_buf(),
            at,
        }
        .into());
    }
    Ok(())
}

/// The leaf clause of `execution_root`: "the managed base is a **real
/// directory**".
///
/// A path that names nothing is not a real directory and gets the same
/// refusal as a file or a link; every other failure to read it stays an I/O
/// error with the path attached.
///
/// # Errors
///
/// [`Refusal::BaseIsNotADirectory`], or the I/O error.
pub(super) fn refuse_unreal_directory(path: &Path) -> Result<(), UpstrokeError> {
    let real = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_dir() && !is_reparse_point(&metadata),
        Err(error) if is_absent(&error) => false,
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if !real {
        return Err(Refusal::BaseIsNotADirectory {
            path: path.to_path_buf(),
        }
        .into());
    }
    Ok(())
}

/// Undo Windows' extended-length (`\\?\`) canonical form.
///
/// **Measured on the Windows Server 2025 guest**, and a production defect
/// rather than a test artefact: `fs::canonicalize` on Windows returns
/// `\\?\C:\...`, and Git — an MSYS program — rewrites that to `//?/C:/...`
/// and fails with `could not create leading directories … Invalid argument`.
/// Every `git worktree add` under an execution root derived from a
/// canonicalized private root failed with it. Whatever **the parent** hands to
/// Git has to be a path Git can open, so the verbatim prefix comes back off
/// here, before the path leaves this module. (Nothing in this module invokes
/// Git; it returns paths, and the parent's funnels are what run the children.)
///
/// The prefix comes off **unconditionally**; there is no length or component
/// check here. For a path within `MAX_PATH` the stripped spelling is the one
/// Git can open. For a longer one Git can open neither spelling without
/// `core.longpaths`, and std puts the prefix back at its own syscall boundary,
/// so this subsystem's filesystem calls are no worse off. The one spelling
/// stripping changes is a component Win32 itself rewrites — a trailing dot or
/// space — which only a verbatim-path creator can produce and `naming` keeps
/// out of every slot component; and both operands of every containment
/// comparison pass through here, so a comparison is between like spellings
/// either way.
#[cfg(windows)]
pub(super) fn strip_verbatim(path: PathBuf) -> PathBuf {
    use std::path::Prefix;

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    let mut rebuilt = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:\\", char::from(letter))),
        Prefix::VerbatimUNC(server, share) => {
            let mut unc = PathBuf::from("\\\\");
            unc.push(server);
            unc.push(share);
            unc
        }
        _ => return path,
    };
    for component in components {
        if matches!(component, Component::RootDir) {
            continue;
        }
        rebuilt.push(component.as_os_str());
    }
    rebuilt
}

/// See the Windows half: nothing to undo anywhere else.
#[cfg(not(windows))]
pub(super) fn strip_verbatim(path: PathBuf) -> PathBuf {
    path
}

/// Canonicalize the longest existing prefix of `path` and rejoin the rest.
///
/// `fs::canonicalize` needs the whole path to exist; an execution root is
/// compared for containment before it does. The peel stops at the first
/// prefix that canonicalizes, and only absence ([`is_absent`]) is peeled
/// past: a prefix the filesystem refuses to resolve for any other reason —
/// permission, a link loop, a name it cannot represent, transient I/O — is
/// an error, because a comparison over a path the filesystem never verified
/// proves nothing about containment. And a prefix that resolves to something
/// other than a directory while components remain below it is
/// `NotADirectory` at that prefix, on every platform: the filesystem has
/// said the rest of the path cannot exist, and rejoining it lexically would
/// hand back exactly the unverified path this function exists not to.
///
/// The rejoined tail is plain components. A `..` below a component that
/// does not exist has no directory to refer to, and the platforms
/// disagree about what it would name — Win32 resolves it lexically before the
/// filesystem sees it, POSIX cannot traverse the absent directory — so there
/// is no canonical form to hand back: where the filesystem fails on it the
/// peel meets the `..`, finds no plain component left to peel, and returns
/// that failure rather than the raw path.
///
/// A relative path is anchored at the current directory, so `missing` and
/// `./missing` resolve alike: when the peel reaches the empty parent it
/// canonicalizes `.`, the current directory, and rejoins the rest onto that.
///
/// # Errors
///
/// [`UpstrokeError::Io`] naming the prefix whose resolution failed: the whole
/// path, a `..`-terminated head, or `.` when the current directory itself
/// cannot be resolved.
pub(super) fn canonical_prefix(path: &Path) -> Result<PathBuf, UpstrokeError> {
    let mut tail = Vec::new();
    let mut head = path.to_path_buf();
    loop {
        let absent = match fs::canonicalize(&head) {
            Ok(canonical) => {
                let mut canonical = strip_verbatim(canonical);
                if !tail.is_empty() {
                    // It resolved a moment ago; a read failure now is a
                    // failure, and anything but a directory cannot carry the
                    // tail.
                    let directory = fs::metadata(&canonical)
                        .map(|metadata| metadata.is_dir())
                        .map_err(|source| UpstrokeError::Io {
                            path: canonical.clone(),
                            source,
                        })?;
                    if !directory {
                        return Err(UpstrokeError::Io {
                            path: canonical,
                            source: io::Error::from(io::ErrorKind::NotADirectory),
                        });
                    }
                }
                for name in tail.iter().rev() {
                    canonical.push(name);
                }
                return Ok(canonical);
            }
            Err(error) if is_absent(&error) => error,
            Err(source) => return Err(UpstrokeError::Io { path: head, source }),
        };
        // `file_name` is `None` when what is left ends in `..` or is a root:
        // there is no plain component to peel.
        let Some(name) = head.file_name().map(OsStr::to_os_string) else {
            return Err(UpstrokeError::Io {
                path: head,
                source: absent,
            });
        };
        // The empty parent is the current-directory anchor of a relative
        // path: `.` is what canonicalizes to it. Otherwise `pop` truncates in
        // place rather than copying the parent each step; it answers false
        // only when there is no parent to pop to, which is the same terminal
        // case as a `..`-terminated head and gets the same answer.
        if head
            .parent()
            .is_some_and(|parent| parent.as_os_str().is_empty())
        {
            head = PathBuf::from(".");
        } else if !head.pop() {
            return Err(UpstrokeError::Io {
                path: head,
                source: absent,
            });
        }
        tail.push(name);
    }
}

/// Whether `inner` is `outer` or lies beneath it. Both must already be
/// canonical-prefixed.
pub(super) fn is_at_or_inside(outer: &Path, inner: &Path) -> bool {
    inner == outer || inner.starts_with(outer)
}
