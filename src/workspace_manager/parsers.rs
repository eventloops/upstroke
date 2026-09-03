//! Decoders for the bytes Git's plumbing hands back.
//!
//! Three grammars, all NUL-delimited and none of them UTF-8 by assumption:
//! `git worktree list --porcelain -z`, `git diff --name-status -M -z`, and the
//! `gitdir` file of a linked-worktree registration. They are functions over
//! bytes so that the hostile cases -- an undecodable path, an embedded newline,
//! a truncated record, a registration that names something outside the root --
//! can be exercised on every platform rather than only on the one whose
//! filesystem can hold them.
//!
//! The commands whose output these read are run by the parent, inside its
//! funnels; nothing here starts a process or touches a path.

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
#[cfg(unix)]
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::error::UpstrokeError;
use crate::topology::paths::{GitPath, PathSet};

use super::WorktreeRecord;

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

/// Decode the authoritative checkout side of a linked-worktree registration.
///
/// Registration-state table used by recovery:
///
/// | `gitdir` state | Can bind an exact checkout? | Recovery action |
/// |---|---:|---|
/// | valid UTF-8 or Unix path bytes | yes | revalidate containment, then act |
/// | absent or unreadable | no | refuse before mutation |
/// | zero-length | no | refuse before mutation |
/// | partial / not ending in `.git` | no | refuse before mutation |
///
/// `commondir` is deliberately not an input to this binding. A valid `gitdir`
/// plus an empty `commondir` is the one safe repairable state: it identifies
/// the checkout while explaining why Git's own enumeration cannot proceed.
pub(super) fn registration_checkout(admin: &Path, bytes: &[u8]) -> Result<PathBuf, UpstrokeError> {
    let bytes = trim_ascii(bytes);
    if bytes.is_empty() {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has an empty gitdir",
                admin.display()
            ),
        });
    }
    let Some(recorded) = decode_registration_path(bytes) else {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has a gitdir this platform cannot represent exactly",
                admin.display()
            ),
        });
    };
    let normalized: PathBuf = recorded.components().collect();
    if !recorded.is_absolute()
        || recorded
            .components()
            .any(|component| component == Component::ParentDir)
        || normalized.as_os_str() != recorded.as_os_str()
    {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has a gitdir that is not an absolute normalized path",
                admin.display()
            ),
        });
    }
    if recorded.file_name() != Some(OsStr::new(".git")) {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has a gitdir that does not name a checkout .git",
                admin.display()
            ),
        });
    }
    let Some(checkout) = recorded.parent() else {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has a parentless gitdir",
                admin.display()
            ),
        });
    };
    Ok(checkout.to_path_buf())
}

#[cfg(unix)]
fn decode_registration_path(bytes: &[u8]) -> Option<PathBuf> {
    Some(decode_git_path(bytes))
}

#[cfg(windows)]
fn decode_registration_path(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes)
        .ok()
        .map(|path| PathBuf::from(path.replace('/', "\\")))
}

#[cfg(not(any(unix, windows)))]
fn decode_registration_path(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes).ok().map(PathBuf::from)
}
/// Turn `git diff --name-status -M -z` bytes into a [`PathSet`].
///
/// A separate function from
/// [`WorkspaceManager::changed_paths`](super::WorkspaceManager::changed_paths) so
/// the hostile
/// byte cases — an undecodable path, an embedded newline, a path that is
/// nothing but a delimiter — can be exercised on every platform rather than
/// only on the one whose filesystem can hold them.
///
/// # The record grammar
///
/// `-z --name-status` emits NUL-*terminated* fields, one status field followed
/// by the paths that status has: `A\0path\0`, `D\0path\0`, `M\0path\0`, and for
/// a detected rename or copy **two** — `R100\0old\0new\0`. Both are kept, which
/// is `path_policy.actual`'s "both rename endpoints": the old endpoint is the
/// one another owner may already hold a lease on, and an answer that omits it
/// is silently smaller than the diff.
///
/// # Why unparsable is repo-wide, not shorter
///
/// One undecodable path makes the **whole** answer [`PathSet::RepoWide`], and
/// so does a status field this grammar does not recognise. The alternative,
/// dropping it and returning the rest, would hand the merge queue a region that
/// is silently *smaller* than the diff and let two overlapping tasks run in
/// parallel; `GitPath`'s own contract is that "paths that did not decode are
/// never stored", and `prediction` classifies "unsafe or unparsable forms" as
/// repo-wide. Repo-wide overlaps everything, so it is the direction that
/// refuses rather than the one that admits.
#[must_use]
pub fn decode_changed_paths(bytes: &[u8]) -> PathSet {
    let mut paths = Vec::new();
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(status) = fields.next() {
        let Some(endpoints) = status_endpoints(status) else {
            return PathSet::RepoWide;
        };
        for _ in 0..endpoints {
            // A record that stops mid-way is a truncated answer, and a
            // truncated answer is a shorter region.
            let Some(field) = fields.next() else {
                return PathSet::RepoWide;
            };
            match std::str::from_utf8(field) {
                Ok(decoded) => paths.push(GitPath::from(decoded)),
                Err(_) => return PathSet::RepoWide,
            }
        }
    }
    paths.sort();
    paths.dedup();
    PathSet::Prefixes { paths }
}

/// How many path fields a `--name-status` status field is followed by, or
/// `None` when this is not a status field at all.
///
/// The letters are `git diff`'s own documented set. `R` and `C` carry a
/// similarity score and two endpoints; everything else carries one and no
/// score. Anything else — including a path that arrived where a status was
/// expected, which is what a decoder reading `--name-only` output would see —
/// is unparsable and makes the answer repo-wide.
fn status_endpoints(status: &[u8]) -> Option<usize> {
    let (letter, score) = status.split_first()?;
    match letter {
        b'R' | b'C' => score
            .iter()
            .all(u8::is_ascii_digit)
            .then_some(2)
            .filter(|_| !score.is_empty()),
        b'A' | b'D' | b'M' | b'T' | b'U' | b'X' => score.is_empty().then_some(1),
        _ => None,
    }
}

/// Parse `git worktree list --porcelain -z`.
///
/// Attributes are NUL-terminated and an empty attribute ends a record. Paths
/// are taken as bytes, because a repository path need not be UTF-8 on Unix.
pub(super) fn parse_worktree_records(bytes: &[u8]) -> Vec<WorktreeRecord> {
    let mut records = Vec::new();
    let mut current: Option<WorktreeRecord> = None;
    for field in bytes.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(record) = current.take() {
                records.push(record);
            }
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some(WorktreeRecord {
                path: decode_git_path(path),
                head: None,
                branch: None,
                locked: None,
                prunable: None,
            });
            continue;
        }
        let Some(record) = current.as_mut() else {
            continue;
        };
        let text = String::from_utf8_lossy(field);
        let text = text.trim_end();
        if let Some(head) = text.strip_prefix("HEAD ") {
            record.head = Some(head.to_owned());
        } else if let Some(branch) = text.strip_prefix("branch ") {
            record.branch = Some(branch.to_owned());
        } else if text == "locked" {
            record.locked = Some(String::new());
        } else if let Some(reason) = text.strip_prefix("locked ") {
            record.locked = Some(reason.to_owned());
        } else if text == "prunable" {
            record.prunable = Some(String::new());
        } else if let Some(reason) = text.strip_prefix("prunable ") {
            record.prunable = Some(reason.to_owned());
        }
    }
    if let Some(record) = current.take() {
        records.push(record);
    }
    records
}

#[cfg(unix)]
fn decode_git_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(windows)]
fn decode_git_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).replace('/', "\\"))
}

#[cfg(not(any(unix, windows)))]
fn decode_git_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}
