//! Decoders for the bytes Git's plumbing hands back.
//!
//! Three grammars, none of them UTF-8 by assumption. **Two are NUL-delimited**
//! -- `git worktree list --porcelain -z` and `git diff --name-status -M -z`,
//! both read by splitting on the zero byte -- and the third is not: the `gitdir`
//! file of a linked-worktree registration holds **one textual path**, which
//! [`registration_checkout`] trims of exactly the trailing bytes Git trims and
//! decodes whole. They are functions over bytes so that the hostile cases -- an
//! undecodable path, an embedded newline, a truncated record, a registration
//! that names something outside the root -- can be exercised on every platform
//! rather than only on the one whose filesystem can hold them.
//!
//! Every grammar here refuses rather than admits, to the extent its tests
//! prove: a field that is not the grammar, a record cut short, a path this
//! platform cannot spell exactly, an attribute whose value its own grammar
//! forbids, and a changed path that is not one normalised repository path are
//! each named at the point they are seen, never dropped, skipped over or read
//! as something shorter. What a refusal becomes is decided once per grammar,
//! at the one site that knows the caller's action, and that site says so. The
//! one thing skipped on purpose is an attribute label this module does not
//! know, because Git may add one.
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
use std::path::{Component, Path, PathBuf};
use std::str::Utf8Error;

use crate::error::UpstrokeError;
use crate::topology::paths::{GitPath, PathSet};

use super::WorktreeRecord;

/// Trim what Git trims when it reads a `gitdir` file, and nothing more.
///
/// Git reads the file with `strbuf_rtrim`, whose whitespace class is its own
/// four characters: space, tab, carriage return and line feed. Measured against
/// `git worktree list` (Git 2.43.0): a leading space stays part of the recorded
/// path, which Git then treats as relative, and a trailing form feed or vertical
/// tab stays part of it too. A wider trim here binds a registration to a
/// checkout that Git does not read from it, and the checkout is what recovery
/// acts on.
fn trim_gitdir(mut bytes: &[u8]) -> &[u8] {
    while let [rest @ .., b' ' | b'\t' | b'\r' | b'\n'] = bytes {
        bytes = rest;
    }
    bytes
}

/// Decode the authoritative checkout side of a linked-worktree registration.
///
/// Registration-state table used by recovery:
///
/// | `gitdir` state | Can bind an exact checkout? | Recovery action |
/// |---|---:|---|
/// | absolute UTF-8 or Unix path bytes | yes | revalidate containment, then act |
/// | relative path bytes | yes, joined to `admin` | canonicalise, revalidate containment, then act |
/// | absent or unreadable | no | refuse before mutation |
/// | zero-length | no | refuse before mutation |
/// | partial / not ending in `.git` | no | refuse before mutation |
///
/// The bytes are read the way Git reads them: [`trim_gitdir`] takes off the
/// trailing line terminator and nothing else, so a path Git would read as
/// relative (a leading space) or as ending in a byte other than `t` (a trailing
/// form feed) is refused by the rows below rather than quietly repaired into a
/// checkout Git never named.
///
/// **A relative `gitdir` is Git's own form**, not corruption: since Git 2.48,
/// `worktree.useRelativePaths=true` (and `worktree add --relative-paths`)
/// writes the linking files relative, and Git's reader joins the recorded path
/// to the directory holding the `gitdir` file and resolves it with realpath
/// (`worktree.c`, `get_linked_worktree`). This does the join, against `admin`,
/// which is that directory, and resolves the `..` such a path is made of
/// lexically ([`resolve_relative`]), so the checkout handed out is always one
/// normalised absolute path and no caller has to canonicalise it before a
/// component-wise comparison; the caller's canonicalisation is then Git's
/// realpath over an already normalised path. A relative path that climbs above
/// the filesystem root, or is not normalised in itself, refuses by name; an
/// absolute registration refuses `..` as before.
///
/// `commondir` is deliberately not an input to this binding. A valid `gitdir`
/// plus an empty `commondir` is the one safe repairable state: it identifies
/// the checkout while explaining why Git's own enumeration cannot proceed.
///
/// # Errors
///
/// [`UpstrokeError::Git`] naming the registration and the row of the table it
/// fell into. Every refusing row has the same action, refuse before mutation,
/// so the message is the distinction and one variant carries it.
pub(super) fn registration_checkout(admin: &Path, bytes: &[u8]) -> Result<PathBuf, UpstrokeError> {
    let bytes = trim_gitdir(bytes);
    if bytes.is_empty() {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has an empty gitdir",
                admin.display()
            ),
        });
    }
    let recorded = match decode_path(bytes) {
        Ok(recorded) => recorded,
        Err(error) => {
            return Err(UpstrokeError::Git {
                message: format!(
                    "worktree registration {} has a gitdir that is not UTF-8 from byte {}, which \
                     this platform cannot represent exactly",
                    admin.display(),
                    error.valid_up_to()
                ),
            });
        }
    };
    let recorded = if recorded.is_absolute() {
        recorded
    } else {
        if recorded.has_root() {
            return Err(UpstrokeError::Git {
                message: format!(
                    "worktree registration {} has a gitdir that is rooted but not absolute",
                    admin.display()
                ),
            });
        }
        if !admin.is_absolute() {
            return Err(UpstrokeError::Git {
                message: format!(
                    "worktree registration {} is not an absolute path, so its relative gitdir \
                     cannot be resolved",
                    admin.display()
                ),
            });
        }
        let normalized: PathBuf = recorded.components().collect();
        if normalized.as_os_str() != recorded.as_os_str() {
            return Err(UpstrokeError::Git {
                message: format!(
                    "worktree registration {} has a gitdir that is not a normalized relative path",
                    admin.display()
                ),
            });
        }
        let Some(resolved) = resolve_relative(admin, &recorded) else {
            return Err(UpstrokeError::Git {
                message: format!(
                    "worktree registration {} has a relative gitdir that climbs above the \
                     filesystem root",
                    admin.display()
                ),
            });
        };
        resolved
    };
    let normalized: PathBuf = recorded.components().collect();
    if recorded
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

/// `relative` joined to `admin` with every `..` resolved lexically, or `None`
/// when a `..` would climb above the filesystem root.
///
/// Lexical on purpose: this module touches no path, and the caller's
/// canonicalisation resolves links afterwards over a path that no longer has
/// a `..` for a component-wise comparison to misread. `admin` is absolute, so
/// [`PathBuf::pop`] answers `false` only at the root; `relative` has no root
/// and is normalised in itself, so its components are `..`, names, and at
/// most a leading `.`.
fn resolve_relative(admin: &Path, relative: &Path) -> Option<PathBuf> {
    let mut resolved = admin.to_path_buf();
    for component in relative.components() {
        match component {
            Component::ParentDir => {
                if !resolved.pop() {
                    return None;
                }
            }
            Component::Normal(name) => resolved.push(name),
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(resolved)
}

/// A path as Git's plumbing spells it, as this platform's [`PathBuf`].
///
/// On Unix every byte string is a path and the decode cannot fail. Elsewhere a
/// path is UTF-8 or it is nothing: the alternative, a lossy decode, spells a
/// *different* path (`U+FFFD` where the bytes were), and every path this module
/// produces is identity -- compared with a target, walked for containment,
/// handed to removal -- never a diagnostic (§8). The error says where the bytes
/// stop being UTF-8. Git for Windows writes UTF-8, so the failing arm is for
/// hostile or corrupt bytes, and it refuses.
#[cfg(unix)]
fn decode_path(bytes: &[u8]) -> Result<PathBuf, Utf8Error> {
    use std::os::unix::ffi::OsStringExt as _;
    // The one copy in this module: the borrowed bytes becoming the owned path.
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
}

/// A path as Git's plumbing spells it, as this platform's [`PathBuf`].
///
/// See the Unix arm for the contract. Git spells a Windows path with `/`, and
/// [`Path::components`] reads either separator, but [`registration_checkout`]
/// compares a spelling with its own normalisation, and the normalisation is
/// spelled with the platform's separator; so is the recorded path, then.
#[cfg(not(unix))]
fn decode_path(bytes: &[u8]) -> Result<PathBuf, Utf8Error> {
    std::str::from_utf8(bytes)
        .map(|text| PathBuf::from(text.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

/// Why `git diff --name-status -M -z` bytes are not a list of paths.
///
/// One variant per shape the grammar refuses, each naming the field it stopped
/// at (fields are counted from zero across the whole answer), so a test can say
/// which refusal it saw and a caller that chooses to record the reason has one
/// to record. Today the one caller, [`decode_changed_paths`], has one action
/// for all of them and says so at its match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum NameStatusError {
    /// The bytes do not end at a field boundary. `-z` terminates every field
    /// with NUL, so a tail without one is a record cut short, and a record cut
    /// short is a shorter region.
    #[error("the last field has no terminator")]
    Unterminated,
    /// A field is empty where the grammar has a status or a path.
    #[error("field {field} is empty")]
    EmptyField { field: usize },
    /// A field where a status was expected is not one of `git diff`'s. This is
    /// what a decoder reading `--name-only` output sees in its first field.
    #[error("field {field} is not a status field")]
    UnknownStatus { field: usize },
    /// A status's endpoints stop before the status said they would.
    #[error("the record at field {record} is truncated")]
    Truncated { record: usize },
    /// A path field is not UTF-8; `valid_up_to` is the offset of the first byte
    /// that is not.
    #[error("field {field} is not UTF-8 from byte {valid_up_to}")]
    UndecodablePath { field: usize, valid_up_to: usize },
    /// A path field is not one normalised repository path: it is absolute, ends
    /// in a separator, has an empty, `.` or `..` component, or carries a
    /// backslash. Git writes none of these; each is a second spelling of a path
    /// the lease comparator would not match to its first.
    #[error("field {field} is not a normalised repository path")]
    UnsafePath { field: usize },
}

/// Whether a decoded `--name-status` path is one normalised repository path.
///
/// The lease comparator (`topology::leases`) compares paths component by
/// component, so `src/./shared.rs` and `src/shared.rs` would be two regions
/// that do not overlap, and two owners of one file would run at once. Git
/// itself emits only normalised, relative, forward-slash paths; anything else
/// is not Git's answer, and the region for it is repo-wide.
fn is_normalised_repository_path(path: &str) -> bool {
    // An empty path, a leading or trailing `/` and a doubled `/` all split
    // into an empty component, so the component rule is the whole rule but
    // for the backslash, which `/`-splitting never sees.
    !path.contains('\\')
        && path
            .split('/')
            .all(|component| !matches!(component, "" | "." | ".."))
}

/// Read `git diff --name-status -M -z` bytes as the paths they name, sorted
/// and deduplicated, or the first shape the grammar refuses.
///
/// # The record grammar
///
/// `-z --name-status` emits NUL-*terminated* fields, one status field followed
/// by the paths that status has: `A\0path\0`, `D\0path\0`, `M\0path\0`, and for
/// a detected rename or copy **two** — `R100\0old\0new\0`. Both are kept, which
/// is `path_policy.actual`'s "both rename endpoints": the old endpoint is the
/// one another owner may already hold a lease on, and an answer that omits it
/// is silently smaller than the diff. An empty answer is an empty diff, which
/// is the one answer `git diff` gives with no bytes at all.
///
/// The final NUL is taken off first, so that an empty field means an empty
/// field and not the end of the bytes; a tail without that NUL, an empty
/// field, a doubled terminator, a field that is not a status where a status is
/// due, a record whose endpoints stop early, and a path that is not one
/// normalised repository path are each refused with their position, never
/// re-aligned into a plausible shorter list.
///
/// # Errors
///
/// [`NameStatusError`], the first refusal in field order.
pub(super) fn changed_path_records(bytes: &[u8]) -> Result<Vec<GitPath>, NameStatusError> {
    let mut paths = Vec::new();
    if bytes.is_empty() {
        return Ok(paths);
    }
    let Some(body) = bytes.strip_suffix(b"\0") else {
        return Err(NameStatusError::Unterminated);
    };
    let mut fields = body.split(|byte| *byte == 0).enumerate();
    while let Some((record, status)) = fields.next() {
        if status.is_empty() {
            return Err(NameStatusError::EmptyField { field: record });
        }
        let Some(endpoints) = status_endpoints(status) else {
            return Err(NameStatusError::UnknownStatus { field: record });
        };
        for _ in 0..endpoints {
            let Some((field, path)) = fields.next() else {
                return Err(NameStatusError::Truncated { record });
            };
            if path.is_empty() {
                return Err(NameStatusError::EmptyField { field });
            }
            match std::str::from_utf8(path) {
                Ok(decoded) if is_normalised_repository_path(decoded) => {
                    paths.push(GitPath::from(decoded));
                }
                Ok(_) => return Err(NameStatusError::UnsafePath { field }),
                Err(error) => {
                    return Err(NameStatusError::UndecodablePath {
                        field,
                        valid_up_to: error.valid_up_to(),
                    });
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Turn `git diff --name-status -M -z` bytes into a [`PathSet`].
///
/// A separate function from
/// [`WorkspaceManager::changed_paths`](super::WorkspaceManager::changed_paths) so
/// the hostile byte cases can be exercised on every platform rather than only
/// on the one whose filesystem can hold them; [`changed_path_records`] is the
/// grammar and this is the one decision over its refusals.
///
/// # Why unparsable is repo-wide, not shorter
///
/// Every refusal makes the **whole** answer [`PathSet::RepoWide`]. The
/// alternative, dropping the offending record and returning the rest, would
/// hand the merge queue a region that is silently *smaller* than the diff and
/// let two overlapping tasks run in parallel; `GitPath`'s own contract is that
/// "paths that did not decode are never stored", and `prediction` classifies
/// "unsafe or unparsable forms" as repo-wide. Repo-wide overlaps everything, so
/// it is the direction that refuses rather than the one that admits.
///
/// The reason ends here, at the one site that decides, and not at its source.
/// The parent's `changed_paths` is the layer that knows the slot and the base,
/// and whether the reason is recorded is that layer's decision; until it makes
/// one, the arm below is the only place a [`NameStatusError`] is dropped, and
/// it spells every variant so that a new one is a decision here too.
#[must_use]
pub fn decode_changed_paths(bytes: &[u8]) -> PathSet {
    match changed_path_records(bytes) {
        Ok(paths) => PathSet::Prefixes { paths },
        Err(
            NameStatusError::Unterminated
            | NameStatusError::EmptyField { .. }
            | NameStatusError::UnknownStatus { .. }
            | NameStatusError::Truncated { .. }
            | NameStatusError::UndecodablePath { .. }
            | NameStatusError::UnsafePath { .. },
        ) => PathSet::RepoWide,
    }
}

/// How many path fields a `--name-status` status field is followed by, or
/// `None` when this is not a status field at all.
///
/// The letters are `git diff`'s own documented set. `R` and `C` carry a
/// similarity score and two endpoints; everything else carries one and no
/// score. Anything else — including a path that arrived where a status was
/// expected, which is what a decoder reading `--name-only` output would see —
/// is not a status field.
fn status_endpoints(status: &[u8]) -> Option<usize> {
    // A membership test's own verdict: an empty field is not a status field,
    // and `None` is this function's answer for that, not a failure it hides.
    // The caller has already refused an empty field by name; this keeps the
    // function total over every slice.
    let (letter, score) = status.split_first()?;
    match letter {
        b'R' | b'C' => (!score.is_empty() && score.iter().all(u8::is_ascii_digit)).then_some(2),
        b'A' | b'D' | b'M' | b'T' | b'U' | b'X' => score.is_empty().then_some(1),
        _ => None,
    }
}

/// Whether `value` is an object id as `git worktree list` prints one: every
/// byte a hexadecimal digit, forty of them (SHA-1) or sixty-four (SHA-256).
fn is_object_id(value: &[u8]) -> bool {
    matches!(value.len(), 40 | 64) && value.iter().all(u8::is_ascii_hexdigit)
}

/// Whether `value` can be a refname at all: `git check-ref-format` forbids
/// ASCII control characters, space, DEL and the seven bytes `~ ^ : ? * [ \`
/// anywhere in one. Not the whole rule (the `..`, `@{` and `.lock` clauses are
/// not applied), but everything a stray or hostile byte could add to a name.
fn can_be_refname(value: &[u8]) -> bool {
    !value.is_empty()
        && !value
            .iter()
            .any(|byte| *byte <= b' ' || *byte == 0x7f || b"~^:?*[\\".contains(byte))
}

/// Parse `git worktree list --porcelain -z`.
///
/// # The record grammar
///
/// Git's own words: "the porcelain format has a line per attribute. If `-z` is
/// given then the lines are terminated with NUL rather than a newline.
/// Attributes are listed with a label and value separated by a single space.
/// Boolean attributes (like `bare` and `detached`) are listed as a label only
/// … The first attribute of a worktree is always `worktree`, an empty line
/// indicates the end of the record." So a complete answer ends in two NULs,
/// and it is never empty: Git lists the repository's own worktree first.
///
/// Read exactly, to the extent the tests prove: bytes that do not end in NUL,
/// an empty answer, a `worktree` header while a record is still open, a final
/// record no empty attribute closed, an empty attribute with no record open,
/// and an attribute before any `worktree` line are refused rather than read as
/// a complete list. A list cut short at a record boundary would otherwise drop
/// the `locked initializing` line that tells a registered-but-unpopulated
/// worktree from a populated one.
///
/// # The attributes
///
/// The path is taken as bytes through [`decode_path`], because a repository
/// path need not be UTF-8 on Unix and a lossy spelling is not the path; under
/// `-z` it is verbatim, and a space is a legal byte of one. The structural
/// attributes are held to their own grammars: `HEAD` is an object id
/// ([`is_object_id`]), `branch` is a refname's byte set ([`can_be_refname`],
/// which forbids whitespace), `detached` and `bare` are labels with no value,
/// and none of the four appears twice in a record. `locked` and `prunable` are
/// Git's own reasons, verbatim (a lock reason may carry a newline, which is why
/// `-z` exists), read lossily into the [`String`]s [`WorktreeRecord`] gives
/// them and consulted only for the word `initializing`. A label this parser
/// does not know is skipped, since Git may add one.
///
/// # Errors
///
/// [`UpstrokeError::Git`] naming the record and what was wrong with it. The
/// callers have one action, refuse, so one variant carries the distinction.
pub(super) fn parse_worktree_records(bytes: &[u8]) -> Result<Vec<WorktreeRecord>, UpstrokeError> {
    let refuse = |record: usize, what: &str| UpstrokeError::Git {
        message: format!("worktree list record {record} {what}"),
    };
    if bytes.is_empty() {
        return Err(UpstrokeError::Git {
            message: "worktree list is empty; Git lists at least the repository's own worktree"
                .to_owned(),
        });
    }
    let Some(body) = bytes.strip_suffix(b"\0") else {
        return Err(UpstrokeError::Git {
            message: "worktree list ends without a terminator".to_owned(),
        });
    };
    let mut records: Vec<WorktreeRecord> = Vec::new();
    let mut current: Option<WorktreeRecord> = None;
    for field in body.split(|byte| *byte == 0) {
        let index = records.len();
        if field.is_empty() {
            let Some(record) = current.take() else {
                return Err(refuse(index, "is closed before it is opened"));
            };
            records.push(record);
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            if current.is_some() {
                return Err(refuse(index, "is not closed before the next record begins"));
            }
            if path.is_empty() {
                return Err(refuse(index, "has an empty path"));
            }
            let path = match decode_path(path) {
                Ok(path) => path,
                Err(error) => {
                    return Err(UpstrokeError::Git {
                        message: format!(
                            "worktree list record {index} names a path that is not UTF-8 from \
                             byte {}, which this platform cannot represent exactly",
                            error.valid_up_to()
                        ),
                    });
                }
            };
            current = Some(WorktreeRecord {
                path,
                head: None,
                branch: None,
                locked: None,
                prunable: None,
            });
            continue;
        }
        let Some(record) = current.as_mut() else {
            return Err(refuse(index, "has an attribute before its worktree line"));
        };
        let (label, value) = match field.iter().position(|byte| *byte == b' ') {
            Some(space) => (&field[..space], Some(&field[space + 1..])),
            None => (field, None),
        };
        match (label, value) {
            (b"HEAD", Some(value)) if is_object_id(value) && record.head.is_none() => {
                record.head = Some(String::from_utf8_lossy(value).into_owned());
            }
            (b"HEAD", _) => return Err(refuse(index, "has a HEAD that is not one object id")),
            (b"branch", Some(value)) if can_be_refname(value) && record.branch.is_none() => {
                record.branch = Some(String::from_utf8_lossy(value).into_owned());
            }
            (b"branch", _) => return Err(refuse(index, "has a branch that is not one refname")),
            (b"detached" | b"bare", None) => {}
            (b"detached" | b"bare", Some(_)) => {
                return Err(refuse(index, "has a boolean attribute carrying a value"));
            }
            (b"locked", value) if record.locked.is_none() => {
                record.locked = Some(reason(value));
            }
            (b"prunable", value) if record.prunable.is_none() => {
                record.prunable = Some(reason(value));
            }
            (b"locked" | b"prunable", _) => {
                return Err(refuse(index, "has a reason attribute twice"));
            }
            _ => {}
        }
    }
    if current.is_some() {
        return Err(refuse(
            records.len(),
            "is not closed; the list was cut short",
        ));
    }
    Ok(records)
}

/// A `locked` or `prunable` reason as Git printed it: empty for the bare
/// label, otherwise the bytes after the label's one space, verbatim.
fn reason(value: Option<&[u8]>) -> String {
    value.map_or_else(String::new, |value| {
        String::from_utf8_lossy(value).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One `-z --name-status` record: a status field and its path fields.
    fn status_record(status: &[u8], paths: &[&[u8]]) -> Vec<u8> {
        let mut bytes = status.to_vec();
        bytes.push(0);
        for path in paths {
            bytes.extend_from_slice(path);
            bytes.push(0);
        }
        bytes
    }

    /// NUL-terminated porcelain fields, in order.
    fn porcelain(fields: &[&[u8]]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for field in fields {
            bytes.extend_from_slice(field);
            bytes.push(0);
        }
        bytes
    }

    const HEAD: &[u8] = b"HEAD 88663d58b63b0acaf3c31e98aa723336b24f1510";
    const OID: &str = "88663d58b63b0acaf3c31e98aa723336b24f1510";

    /// The registration directory, absolute on this platform: the relative
    /// branch of `registration_checkout` refuses an admin path that is not,
    /// and `/repository` alone is relative on Windows.
    fn admin() -> PathBuf {
        absolute("/repository/.git/worktrees/example")
    }

    fn message(error: UpstrokeError) -> String {
        error.to_string()
    }

    /// `path`, spelled with this platform's separator.
    fn platform(path: &str) -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            path.replace('/', "\\")
        } else {
            path.to_owned()
        })
    }

    /// An absolute path: `C:` makes a Windows path absolute; on Unix the
    /// leading `/` does.
    fn absolute(path: &str) -> PathBuf {
        platform(&format!("{}{path}", if cfg!(windows) { "C:" } else { "" }))
    }

    /// Git's own reading of a `gitdir` file, measured: trailing space, tab,
    /// carriage return and line feed come off; a leading space is part of the
    /// path, and so is a trailing form feed.
    #[test]
    fn a_gitdir_is_trimmed_exactly_as_git_trims_it() {
        let root = if cfg!(windows) { "C:\\repo" } else { "/repo" };
        let checkout = PathBuf::from(root).join("wt");
        for tail in ["\n", "\r\n", " \t\n", "\n\n", ""] {
            let bytes = format!("{root}/wt/.git{tail}");
            let decoded = registration_checkout(&admin(), bytes.as_bytes())
                .expect("a trailing line terminator is not part of the path");
            assert_eq!(decoded, checkout, "with tail {tail:?}");
        }
        // A leading space is part of the path, which is then relative, and a
        // relative registration is joined to `admin` as Git joins it: the
        // decoded checkout is under the registration directory, never the
        // absolute checkout the bytes resemble.
        let leading = format!(" {root}/wt/.git\n");
        let decoded = registration_checkout(&admin(), leading.as_bytes())
            .expect("a leading space makes the path relative, which Git resolves");
        assert_ne!(
            decoded, checkout,
            "the space is not trimmed into the absolute checkout"
        );
        assert!(
            decoded.starts_with(admin()),
            "a relative path is joined to the registration directory: {decoded:?}"
        );
        let form_feed = format!("{root}/wt/.git\x0c\n");
        let refused = registration_checkout(&admin(), form_feed.as_bytes())
            .expect_err("a trailing form feed is part of the path, whose name is then not .git");
        assert!(
            message(refused).contains("does not name a checkout .git"),
            "the refusal names the row: the file name"
        );
    }

    #[test]
    fn registration_checkout_names_the_row_a_gitdir_falls_into() {
        let root = if cfg!(windows) { "C:" } else { "" };
        let cases: &[(String, &str)] = &[
            (String::new(), "has an empty gitdir"),
            ("  \n".to_owned(), "has an empty gitdir"),
            (
                format!("{root}/absolute/../traversal/.git"),
                "not an absolute normalized path",
            ),
            (
                format!("{root}/absolute/./alias/.git"),
                "not an absolute normalized path",
            ),
            (
                format!("{root}/absolute/checkout"),
                "does not name a checkout .git",
            ),
            (
                format!("{root}/absolute/checkout/.git/"),
                "not an absolute normalized path",
            ),
        ];
        for (bytes, expected) in cases {
            let refused = registration_checkout(&admin(), bytes.as_bytes()).expect_err("refused");
            let text = message(refused);
            assert!(
                text.contains(expected),
                "{bytes:?}: expected {expected:?} in {text:?}"
            );
            assert!(
                text.contains(&format!("worktree registration {}", admin().display())),
                "the refusal names the registration"
            );
        }
    }

    /// Git 2.48's `worktree.useRelativePaths`: the path is relative to the
    /// directory holding the `gitdir` file, and the `..` it is made of are
    /// resolved here, so the checkout handed out is one normalised absolute
    /// path; a climb above the filesystem root refuses by name.
    #[test]
    fn a_relative_registration_is_resolved_against_its_registration_directory() {
        let admin = absolute("/repo/.git/worktrees/example");
        let decoded = registration_checkout(&admin, b"../../../wt/.git\n")
            .expect("a relative gitdir is Git's own form");
        assert_eq!(decoded, absolute("/repo/wt"), "resolved, with no `..` left");
        assert!(
            decoded.components().all(|c| c != Component::ParentDir),
            "no caller has to normalise it"
        );

        let refused = registration_checkout(&admin, b"../../../../../x/.git\n")
            .expect_err("five `..` from a four-deep registration climb above the root");
        assert!(message(refused).contains("climbs above the filesystem root"));
        let decoded = registration_checkout(&admin, b"../../../../x/.git\n")
            .expect("four `..` reach the root exactly");
        assert_eq!(decoded, absolute("/x"));

        let refused = registration_checkout(&admin, b"../../../wt\n")
            .expect_err("a relative gitdir still names a checkout .git");
        assert!(message(refused).contains("does not name a checkout .git"));

        let refused = registration_checkout(&admin, b"../../.././wt/.git\n")
            .expect_err("a relative gitdir is still normalised");
        assert!(message(refused).contains("not a normalized relative path"));

        let refused = registration_checkout(&platform("relative/admin"), b"../wt/.git\n")
            .expect_err("nothing to resolve a relative gitdir against");
        assert!(message(refused).contains("cannot be resolved"));

        let elsewhere = absolute("/elsewhere/wt/.git");
        let mut bytes = elsewhere.as_os_str().as_encoded_bytes().to_vec();
        bytes.push(b'\n');
        let decoded =
            registration_checkout(&admin, &bytes).expect("an absolute registration is unchanged");
        assert_eq!(decoded, absolute("/elsewhere/wt"));
        assert!(!decoded.starts_with(&admin), "and is not joined to admin");
    }

    #[cfg(not(unix))]
    #[test]
    fn a_registration_that_is_not_utf8_is_refused_with_its_offset() {
        let refused = registration_checkout(&admin(), b"C:\\x\\\xff\\.git\n")
            .expect_err("a lossy spelling is not registration identity");
        assert!(
            message(refused).contains("not UTF-8 from byte 5"),
            "the refusal says where the bytes stop being UTF-8"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_registration_keeps_every_path_byte_on_unix() {
        use std::os::unix::ffi::OsStrExt as _;

        let decoded = registration_checkout(&admin(), b"/tmp/non-utf8-\xff/.git\n")
            .expect("every byte string is a Unix path");
        assert_eq!(decoded.as_os_str().as_bytes(), b"/tmp/non-utf8-\xff");
    }

    #[test]
    fn changed_path_records_reads_every_status_and_both_rename_endpoints() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&status_record(b"M", &[b"src/lib.rs"]));
        bytes.extend_from_slice(&status_record(
            b"R100",
            &[b"src/auth.rs", b"archive/auth.rs"],
        ));
        bytes.extend_from_slice(&status_record(b"C075", &[b"src/lib.rs", b"src/copy.rs"]));
        bytes.extend_from_slice(&status_record(b"A", &[b"src/added.rs"]));
        bytes.extend_from_slice(&status_record(b"D", &[b"src/gone.rs"]));
        bytes.extend_from_slice(&status_record(b"T", &[b"link"]));
        bytes.extend_from_slice(&status_record(b"U", &[b"conflict"]));
        bytes.extend_from_slice(&status_record(b"X", &[b"unknown"]));
        let records = changed_path_records(&bytes).expect("well formed");
        let paths: Vec<&str> = records.iter().map(GitPath::as_str).collect();
        assert_eq!(
            paths,
            vec![
                "archive/auth.rs",
                "conflict",
                "link",
                "src/added.rs",
                "src/auth.rs",
                "src/copy.rs",
                "src/gone.rs",
                "src/lib.rs",
                "unknown",
            ],
            "sorted, and `src/lib.rs` once though two records name it"
        );
        assert!(
            changed_path_records(b"").expect("an empty diff").is_empty(),
            "no bytes is an empty diff, not a refusal"
        );
    }

    /// Each hostile shape is refused, and refused for its own reason: a test
    /// that asks only "was it refused" cannot tell a rename score that is not
    /// a number from a record that was cut short.
    #[test]
    fn changed_path_records_names_the_shape_it_refuses() {
        let cases: Vec<(&str, Vec<u8>, NameStatusError)> = vec![
            (
                "a path that is nothing but a delimiter",
                b"\0".to_vec(),
                NameStatusError::EmptyField { field: 0 },
            ),
            (
                "a tail without its terminator",
                b"A\0src/x".to_vec(),
                NameStatusError::Unterminated,
            ),
            (
                "a second record without its terminator",
                b"A\0src/a\0M\0src/b".to_vec(),
                NameStatusError::Unterminated,
            ),
            (
                "an empty path field",
                b"A\0\0src/x\0".to_vec(),
                NameStatusError::EmptyField { field: 1 },
            ),
            (
                "a doubled terminator",
                b"A\0src/a\0\0".to_vec(),
                NameStatusError::EmptyField { field: 2 },
            ),
            (
                "--name-only output, where a path arrives as a status",
                b"archive/auth.rs\0src/added.rs\0".to_vec(),
                NameStatusError::UnknownStatus { field: 0 },
            ),
            (
                "a status letter that is not one of Git's",
                status_record(b"Z", &[b"src/auth.rs"]),
                NameStatusError::UnknownStatus { field: 0 },
            ),
            (
                "a single-endpoint letter carrying a score",
                status_record(b"M50", &[b"src/auth.rs"]),
                NameStatusError::UnknownStatus { field: 0 },
            ),
            (
                "a rename letter carrying no score",
                status_record(b"R", &[b"src/auth.rs", b"archive/auth.rs"]),
                NameStatusError::UnknownStatus { field: 0 },
            ),
            (
                "a rename score that is not a number",
                status_record(b"Rxx", &[b"src/auth.rs", b"archive/auth.rs"]),
                NameStatusError::UnknownStatus { field: 0 },
            ),
            (
                "a status field that does not decode",
                status_record(b"\xff", &[b"src/auth.rs"]),
                NameStatusError::UnknownStatus { field: 0 },
            ),
            (
                "a rename record with only one endpoint",
                status_record(b"R100", &[b"src/auth.rs"]),
                NameStatusError::Truncated { record: 0 },
            ),
            (
                "a later record with only one endpoint",
                [
                    status_record(b"A", &[b"src/a.rs"]),
                    status_record(b"C075", &[b"src/b.rs"]),
                ]
                .concat(),
                NameStatusError::Truncated { record: 2 },
            ),
            (
                "an undecodable path",
                status_record(b"M", &[b"src/\xff\xfe.rs"]),
                NameStatusError::UndecodablePath {
                    field: 1,
                    valid_up_to: 4,
                },
            ),
            (
                "an undecodable rename source",
                status_record(b"R100", &[b"src/\xff.rs", b"archive/auth.rs"]),
                NameStatusError::UndecodablePath {
                    field: 1,
                    valid_up_to: 4,
                },
            ),
            (
                "an undecodable rename destination",
                status_record(b"R100", &[b"src/auth.rs", b"archive/\xff.rs"]),
                NameStatusError::UndecodablePath {
                    field: 2,
                    valid_up_to: 8,
                },
            ),
        ];
        for (name, bytes, expected) in &cases {
            let refused = changed_path_records(bytes).expect_err(name);
            assert_eq!(refused, *expected, "{name}");
            assert!(
                decode_changed_paths(bytes).is_repo_wide(),
                "{name}: every refusal is the repo-wide region, never a shorter list"
            );
        }
        assert_eq!(cases.len(), 16, "sixteen independent refused shapes");
    }

    /// A second spelling of a path is not a narrow region: the lease
    /// comparator matches components literally, so `src/./shared.rs` would
    /// not overlap `src/shared.rs` and two owners of one file would run at
    /// once. Each alias is refused by name and the region is repo-wide.
    #[test]
    fn a_changed_path_that_is_not_one_normalised_path_is_repo_wide() {
        let aliases: &[(&str, &[u8])] = &[
            ("a `.` component", b"src/./shared.rs"),
            ("a `..` component", b"src/../x"),
            ("an absolute path", b"/abs"),
            ("an empty component", b"a//b"),
            ("a backslash", b"a\\b"),
            ("a trailing separator", b"src/"),
            ("a lone `.`", b"."),
        ];
        for (name, path) in aliases {
            let bytes = status_record(b"M", &[path]);
            assert_eq!(
                changed_path_records(&bytes).expect_err(name),
                NameStatusError::UnsafePath { field: 1 },
                "{name}"
            );
            assert!(decode_changed_paths(&bytes).is_repo_wide(), "{name}");
            let as_destination = status_record(b"R100", &[b"src/shared.rs", path]);
            assert_eq!(
                changed_path_records(&as_destination).expect_err(name),
                NameStatusError::UnsafePath { field: 2 },
                "{name}, as a rename destination"
            );
        }
        let plain = status_record(b"M", &[b"src/shared.rs"]);
        let decoded = decode_changed_paths(&plain);
        assert!(!decoded.is_repo_wide(), "a plain path stays narrow");
        assert_eq!(
            decoded
                .prefixes()
                .expect("narrow")
                .iter()
                .map(GitPath::as_str)
                .collect::<Vec<_>>(),
            vec!["src/shared.rs"]
        );
        assert_eq!(aliases.len(), 7, "seven independent aliases");
    }

    #[test]
    fn decode_changed_paths_is_the_records_or_repo_wide() {
        let bytes = status_record(b"R100", &[b"src/auth.rs", b"archive/auth.rs"]);
        let decoded = decode_changed_paths(&bytes);
        assert!(!decoded.is_repo_wide());
        let paths: Vec<&str> = decoded
            .prefixes()
            .expect("decoded")
            .iter()
            .map(GitPath::as_str)
            .collect();
        assert_eq!(paths, vec!["archive/auth.rs", "src/auth.rs"]);
        assert!(
            decode_changed_paths(b"")
                .prefixes()
                .expect("an empty diff")
                .is_empty()
        );
    }

    /// The porcelain `-z` grammar, as Git 2.43.0 emits it: a `worktree` line, its
    /// attributes, and an empty attribute closing each record. Under `-z` a
    /// lock reason is verbatim, newline and trailing space included, and a
    /// label this parser does not know is skipped.
    #[test]
    fn worktree_records_are_read_from_the_porcelain_grammar() {
        let bytes = porcelain(&[
            b"worktree /repo",
            HEAD,
            b"branch refs/heads/master",
            b"",
            b"worktree /repo/wt",
            HEAD,
            b"detached",
            b"locked why\nnot ",
            b"prunable gitdir file points to non-existent location",
            b"extension a label this parser does not know",
            b"",
            b"worktree /repo/bare",
            b"bare",
            b"locked",
            b"prunable",
            b"",
        ]);
        let records = parse_worktree_records(&bytes).expect("Git's own grammar");
        let expected = vec![
            WorktreeRecord {
                path: platform("/repo"),
                head: Some(OID.to_owned()),
                branch: Some("refs/heads/master".to_owned()),
                locked: None,
                prunable: None,
            },
            WorktreeRecord {
                path: platform("/repo/wt"),
                head: Some(OID.to_owned()),
                branch: None,
                locked: Some("why\nnot ".to_owned()),
                prunable: Some("gitdir file points to non-existent location".to_owned()),
            },
            WorktreeRecord {
                path: platform("/repo/bare"),
                head: None,
                branch: None,
                locked: Some(String::new()),
                prunable: Some(String::new()),
            },
        ];
        assert_eq!(records, expected);
        let refused = parse_worktree_records(b"").expect_err("Git never lists nothing");
        assert!(message(refused).contains("worktree list is empty"));
    }

    /// Framing: a record ends only at the empty attribute. A header while a
    /// record is open, a list without its final terminator, a final record no
    /// empty attribute closed, a separator with nothing open, and an attribute
    /// before any header are each refused, not read as a complete list.
    #[test]
    fn a_worktree_list_cut_short_is_refused_not_read_as_complete() {
        let cases: &[(&str, Vec<u8>, &str)] = &[
            (
                "two records without the separator between them",
                [
                    porcelain(&[b"worktree /slot", HEAD]),
                    porcelain(&[b"worktree /next", HEAD, b""]),
                ]
                .concat(),
                "record 0 is not closed before the next record begins",
            ),
            (
                "the lock line cut off before the next header",
                [
                    porcelain(&[b"worktree /slot", HEAD, b"detached"]),
                    porcelain(&[b"worktree /next", HEAD, b""]),
                ]
                .concat(),
                "record 0 is not closed before the next record begins",
            ),
            (
                "a list without its final terminator",
                {
                    let mut bytes = porcelain(&[b"worktree /repo", HEAD, b"", b"worktree /wt"]);
                    bytes.extend_from_slice(HEAD);
                    bytes
                },
                "ends without a terminator",
            ),
            (
                "a final record no empty attribute closed",
                porcelain(&[b"worktree /repo", HEAD, b"", b"worktree /wt", HEAD]),
                "record 1 is not closed; the list was cut short",
            ),
            (
                "a header alone",
                porcelain(&[b"worktree /repo"]),
                "record 0 is not closed; the list was cut short",
            ),
            (
                "a separator with no record open",
                porcelain(&[b"worktree /repo", HEAD, b"", b""]),
                "record 1 is closed before it is opened",
            ),
            (
                "an attribute before any header",
                porcelain(&[HEAD, b"worktree /repo", b""]),
                "record 0 has an attribute before its worktree line",
            ),
            (
                "a header with no path",
                porcelain(&[b"worktree ", HEAD, b""]),
                "record 0 has an empty path",
            ),
        ];
        for (name, bytes, expected) in cases {
            let refused = parse_worktree_records(bytes).expect_err(name);
            let text = message(refused);
            assert!(
                text.contains(expected),
                "{name}: expected {expected:?} in {text:?}"
            );
        }
        assert_eq!(cases.len(), 8, "eight independent framing refusals");
    }

    /// The structural attributes are held to their own grammars, so a stray
    /// byte cannot make `refs/heads/main ` a branch nobody has checked out;
    /// the reasons stay verbatim.
    #[test]
    fn structural_attributes_refuse_what_their_grammars_forbid() {
        let cases: &[(&str, &[u8], &str)] = &[
            (
                "a branch with a trailing space",
                b"branch refs/heads/main ",
                "record 0 has a branch that is not one refname",
            ),
            (
                "a branch with an embedded tab",
                b"branch refs/heads/ma\tin",
                "record 0 has a branch that is not one refname",
            ),
            (
                "a branch with a control byte",
                b"branch refs/heads/main\x01",
                "record 0 has a branch that is not one refname",
            ),
            (
                "a branch with a forbidden byte",
                b"branch refs/heads/ma[in",
                "record 0 has a branch that is not one refname",
            ),
            (
                "an empty branch",
                b"branch ",
                "record 0 has a branch that is not one refname",
            ),
            (
                "a HEAD with a trailing space",
                b"HEAD 88663d58b63b0acaf3c31e98aa723336b24f1510 ",
                "record 0 has a HEAD that is not one object id",
            ),
            (
                "a HEAD that is not hexadecimal",
                b"HEAD abc",
                "record 0 has a HEAD that is not one object id",
            ),
            (
                "a HEAD with no value",
                b"HEAD",
                "record 0 has a HEAD that is not one object id",
            ),
            (
                "a boolean attribute carrying a value",
                b"detached yes",
                "record 0 has a boolean attribute carrying a value",
            ),
        ];
        for (name, attribute, expected) in cases {
            let bytes = porcelain(&[b"worktree /repo", attribute, b""]);
            let refused = parse_worktree_records(&bytes).expect_err(name);
            let text = message(refused);
            assert!(
                text.contains(expected),
                "{name}: expected {expected:?} in {text:?}"
            );
        }
        assert_eq!(cases.len(), 9, "nine independent attribute refusals");

        let twice = porcelain(&[b"worktree /repo", HEAD, HEAD, b""]);
        assert!(
            message(parse_worktree_records(&twice).expect_err("HEAD twice"))
                .contains("record 0 has a HEAD that is not one object id")
        );
        let locked_twice = porcelain(&[b"worktree /repo", HEAD, b"locked", b"locked again", b""]);
        assert!(
            message(parse_worktree_records(&locked_twice).expect_err("locked twice"))
                .contains("record 0 has a reason attribute twice")
        );

        let verbatim = porcelain(&[b"worktree /repo", HEAD, b"locked initializing ", b""]);
        let records = parse_worktree_records(&verbatim).expect("a reason is verbatim");
        assert_eq!(records[0].locked.as_deref(), Some("initializing "));
        let sha256 = porcelain(&[
            b"worktree /repo",
            b"HEAD 88663d58b63b0acaf3c31e98aa723336b24f151088663d58b63b0acaf3c31e98",
            b"",
        ]);
        assert!(
            parse_worktree_records(&sha256).is_ok(),
            "a SHA-256 object id is sixty-four hexadecimal digits"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_worktree_path_keeps_every_byte_on_unix() {
        use std::os::unix::ffi::OsStrExt as _;

        let records = parse_worktree_records(&porcelain(&[b"worktree /repo/caf\xe9", HEAD, b""]))
            .expect("every byte string is a Unix path");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path.as_os_str().as_bytes(), b"/repo/caf\xe9");
    }

    #[cfg(not(unix))]
    #[test]
    fn a_worktree_path_that_is_not_utf8_is_refused_with_its_offset() {
        let bytes = porcelain(&[
            b"worktree C:/repo",
            HEAD,
            b"",
            b"worktree C:/repo/caf\xe9",
            HEAD,
            b"",
        ]);
        let refused = parse_worktree_records(&bytes)
            .expect_err("a lossy spelling is not a worktree's identity");
        assert!(
            message(refused).contains("record 1 names a path that is not UTF-8 from byte 11"),
            "the refusal says which record and where"
        );
    }
}
