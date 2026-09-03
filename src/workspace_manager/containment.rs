//! Path hygiene: reparse points, the verbatim prefix, and the canonical
//! comparisons containment is decided by.
//!
//! `decisions.workspace_candidates.execution_root` requires an execution root
//! reached "with no symlink/reparse point on the chain", under a base that is a
//! "real directory", and every containment answer in the parent is a comparison
//! of two [`canonical_prefix`] results. Those four predicates are here; the
//! revalidation that calls them before each effect, and every effect it guards,
//! is the parent's.
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
use std::path::{Component, Path, PathBuf};

use crate::error::UpstrokeError;

use super::Refusal;

/// Whether `metadata` describes a symlink, junction, or any other reparse
/// point.
///
/// **Windows and Unix answer different questions on purpose.** On Unix the only
/// such object is a symbolic link. On Windows the set is larger — a directory
/// junction (`mklink /J`) and a mount point are reparse points that are *not*
/// symbolic links, and `FileType::is_symlink` answers true only for the
/// name-surrogate tags. `expected_failures_refusals[0]` is "symlink/**junction**
/// on the chain", so the Windows half reads the raw attribute
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

/// The first component of `path`'s chain **at or below `anchor`** that is a
/// reparse point, if any.
///
/// # Why the walk is anchored
///
/// `decisions.workspace_candidates.execution_root` says "with no
/// symlink/reparse point on the chain", and a chain has to start somewhere.
/// It starts at the operator's own authorized root, canonicalized — which is
/// how the packet anchors the same check on the other half of the same
/// structure: `expected_failures_refusals[9]` requires "a locator chain without
/// reparse points **canonicalizing to** `<authorized private root>/runs/
/// <basename>`". The root is resolved and trusted; what must be reparse-free is
/// everything the run itself builds beneath it.
///
/// The unanchored reading was tried and is wrong on a real platform, not just
/// inconvenient: macOS ships `/var` as a symlink to `private/var` and its
/// `$TMPDIR` lives under it, so an operator whose private root is anywhere
/// under `/var` — including every default temporary directory on that OS —
/// would have every run refused for a link they did not create and cannot
/// remove. No live passage asks for that, and the containment the refusal
/// exists to protect is unaffected: every deletion in this module goes through
/// [`WorkspaceManager::contained`](super::WorkspaceManager::contained), which
/// compares **canonical** paths, so a
/// resolved link cannot carry a removal outside the root.
///
/// Only components that exist are inspected: a root that has not been created
/// yet has an absent leaf, and refusing on absence would refuse every first
/// run.
fn reparse_point_below(anchor: &Path, path: &Path) -> Result<Option<PathBuf>, UpstrokeError> {
    let Ok(relative) = path.strip_prefix(anchor) else {
        return Ok(None);
    };
    let mut walked = anchor.to_path_buf();
    for component in relative.components() {
        walked.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        match fs::symlink_metadata(&walked) {
            Ok(metadata) => {
                if is_reparse_point(&metadata) {
                    return Ok(Some(walked));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
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

/// Refuse `path` when a component of its chain below `anchor` is a reparse
/// point.
pub(super) fn refuse_reparse_points(anchor: &Path, path: &Path) -> Result<(), UpstrokeError> {
    if let Some(at) = reparse_point_below(anchor, path)? {
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
pub(super) fn refuse_unreal_directory(path: &Path) -> Result<(), UpstrokeError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() || is_reparse_point(&metadata) {
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
/// canonicalized private root failed with it. Whatever this module hands to Git
/// has to be a path Git can open, so the verbatim prefix comes back off.
///
/// A path that genuinely *requires* the verbatim form — one longer than
/// `MAX_PATH`, or carrying a component Win32 would reject — is left as it is:
/// stripping it would produce a path that names something else, and Git could
/// not have used either spelling.
#[cfg(windows)]
pub(super) fn strip_verbatim(path: PathBuf) -> PathBuf {
    use std::path::Prefix;

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    let mut rebuilt = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:\\", letter as char)),
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
/// compared for containment before it does.
pub(super) fn canonical_prefix(path: &Path) -> Result<PathBuf, UpstrokeError> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(strip_verbatim(canonical));
    }
    let mut tail = Vec::new();
    let mut head = path.to_path_buf();
    loop {
        let Some(parent) = head.parent().map(Path::to_path_buf) else {
            return Ok(path.to_path_buf());
        };
        let Some(name) = head.file_name().map(OsStr::to_os_string) else {
            return Ok(path.to_path_buf());
        };
        tail.push(name);
        head = parent;
        if let Ok(canonical) = fs::canonicalize(&head) {
            let mut canonical = strip_verbatim(canonical);
            for name in tail.iter().rev() {
                canonical.push(name);
            }
            return Ok(canonical);
        }
        if head.parent().is_none() {
            return Ok(path.to_path_buf());
        }
    }
}

/// Whether `inner` is `outer` or lies beneath it. Both must already be
/// canonical-prefixed.
pub(super) fn is_at_or_inside(outer: &Path, inner: &Path) -> bool {
    inner == outer || inner.starts_with(outer)
}
