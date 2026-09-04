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
//! Every grammar here refuses rather than admits. A field that is not the
//! grammar, a record cut short, a path this platform cannot spell exactly: each
//! is named at the point it is seen, never dropped, skipped over or read as
//! something shorter. What a refusal becomes is decided once per grammar, at
//! the one site that knows the caller's action, and that site says so.
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
/// | valid UTF-8 or Unix path bytes | yes | revalidate containment, then act |
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
/// `commondir` is deliberately not an input to this binding. A valid `gitdir`
/// plus an empty `commondir` is the one safe repairable state: it identifies
/// the checkout while explaining why Git's own enumeration cannot proceed.
///
/// # Errors
///
/// [`UpstrokeError::Git`] naming the registration and the row of the table it
/// fell into. Every row has the same action, refuse before mutation, so the
/// message is the distinction and one variant carries it.
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
/// is silently smaller than the diff. An empty answer is an empty diff.
///
/// The grammar is read exactly. The final NUL is taken off first, so that an
/// empty field means an empty field and not the end of the bytes; a tail
/// without that NUL, an empty field, a doubled terminator, a field that is not
/// a status where a status is due, and a record whose endpoints stop early are
/// each refused with their position, never re-aligned into a plausible
/// shorter list.
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
                Ok(decoded) => paths.push(GitPath::from(decoded)),
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
            | NameStatusError::UndecodablePath { .. },
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

/// Parse `git worktree list --porcelain -z`.
///
/// Attributes are NUL-terminated and an empty attribute ends a record, so a
/// complete answer ends in two NULs. Paths are taken as bytes through
/// [`decode_path`], because a repository path need not be UTF-8 on Unix and a
/// lossy spelling is not the path. The other attributes are read lossily into
/// the [`String`]s [`WorktreeRecord`] gives them: `HEAD` is hexadecimal,
/// `locked` and `prunable` are Git's own reasons and are read only for the
/// word `initializing`, and `branch` is compared with a UTF-8 refname, which a
/// non-UTF-8 branch name cannot equal in any spelling. An attribute this
/// parser does not know is skipped, since Git may add one.
///
/// The framing is read exactly: bytes that do not end in NUL, a final record
/// that no empty attribute closed, and an attribute before any `worktree` line
/// are refused rather than read as a complete list. A list cut short at a
/// record boundary would otherwise drop the `locked initializing` line that
/// tells a registered-but-unpopulated worktree from a populated one.
///
/// # Errors
///
/// [`UpstrokeError::Git`] naming the record and what was wrong with it. The
/// callers have one action, refuse, so one variant carries the distinction.
pub(super) fn parse_worktree_records(bytes: &[u8]) -> Result<Vec<WorktreeRecord>, UpstrokeError> {
    let mut records = Vec::new();
    if bytes.is_empty() {
        return Ok(records);
    }
    let Some(body) = bytes.strip_suffix(b"\0") else {
        return Err(UpstrokeError::Git {
            message: "worktree list ends without a terminator".to_owned(),
        });
    };
    let mut current: Option<WorktreeRecord> = None;
    for field in body.split(|byte| *byte == 0) {
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
            let path = match decode_path(path) {
                Ok(path) => path,
                Err(error) => {
                    return Err(UpstrokeError::Git {
                        message: format!(
                            "worktree list record {} names a path that is not UTF-8 from byte {}, \
                             which this platform cannot represent exactly",
                            records.len(),
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
            return Err(UpstrokeError::Git {
                message: "worktree list has an attribute before its first record".to_owned(),
            });
        };
        let text = String::from_utf8_lossy(field);
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
    if current.is_some() {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree list record {} is not closed; the list was cut short",
                records.len()
            ),
        });
    }
    Ok(records)
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

    fn admin() -> &'static Path {
        Path::new("/repository/.git/worktrees/example")
    }

    fn message(error: UpstrokeError) -> String {
        error.to_string()
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
            let decoded = registration_checkout(admin(), bytes.as_bytes())
                .expect("a trailing line terminator is not part of the path");
            assert_eq!(decoded, checkout, "with tail {tail:?}");
        }
        let leading = format!(" {root}/wt/.git\n");
        let refused = registration_checkout(admin(), leading.as_bytes())
            .expect_err("a leading space is part of the path, which is then relative");
        assert!(
            message(refused).contains("not an absolute normalized path"),
            "the refusal names the row: a relative path"
        );
        let form_feed = format!("{root}/wt/.git\x0c\n");
        let refused = registration_checkout(admin(), form_feed.as_bytes())
            .expect_err("a trailing form feed is part of the path, whose name is then not .git");
        assert!(
            message(refused).contains("does not name a checkout .git"),
            "the refusal names the row: the file name"
        );
    }

    #[test]
    fn registration_checkout_names_the_row_a_gitdir_falls_into() {
        // `C:` makes a Windows path absolute; on Unix the leading `/` does.
        let root = if cfg!(windows) { "C:" } else { "" };
        let cases: &[(String, &str)] = &[
            (String::new(), "has an empty gitdir"),
            ("  \n".to_owned(), "has an empty gitdir"),
            (
                "relative/.git".to_owned(),
                "not an absolute normalized path",
            ),
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
            let refused = registration_checkout(admin(), bytes.as_bytes()).expect_err("refused");
            let text = message(refused);
            assert!(
                text.contains(expected),
                "{bytes:?}: expected {expected:?} in {text:?}"
            );
            assert!(
                text.contains("worktree registration /repository/.git/worktrees/example"),
                "the refusal names the registration"
            );
        }
    }

    #[cfg(not(unix))]
    #[test]
    fn a_registration_that_is_not_utf8_is_refused_with_its_offset() {
        let refused = registration_checkout(admin(), b"C:\\x\\\xff\\.git\n")
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

        let decoded = registration_checkout(admin(), b"/tmp/non-utf8-\xff/.git\n")
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
    /// lock reason is verbatim, newline and trailing space included.
    #[test]
    fn worktree_records_are_read_from_the_porcelain_grammar() {
        let mut bytes = Vec::new();
        for field in [
            &b"worktree /repo"[..],
            b"HEAD 88663d58b63b0acaf3c31e98aa723336b24f1510",
            b"branch refs/heads/master",
            b"",
            b"worktree /repo/wt",
            b"HEAD 88663d58b63b0acaf3c31e98aa723336b24f1510",
            b"detached",
            b"locked why\nnot ",
            b"prunable gitdir file points to non-existent location",
            b"",
            b"worktree /repo/bare",
            b"bare",
            b"locked",
            b"prunable",
            b"",
        ] {
            bytes.extend_from_slice(field);
            bytes.push(0);
        }
        let records = parse_worktree_records(&bytes).expect("Git's own grammar");
        let expected = vec![
            WorktreeRecord {
                path: PathBuf::from(if cfg!(windows) { "\\repo" } else { "/repo" }),
                head: Some("88663d58b63b0acaf3c31e98aa723336b24f1510".to_owned()),
                branch: Some("refs/heads/master".to_owned()),
                locked: None,
                prunable: None,
            },
            WorktreeRecord {
                path: PathBuf::from(if cfg!(windows) {
                    "\\repo\\wt"
                } else {
                    "/repo/wt"
                }),
                head: Some("88663d58b63b0acaf3c31e98aa723336b24f1510".to_owned()),
                branch: None,
                locked: Some("why\nnot ".to_owned()),
                prunable: Some("gitdir file points to non-existent location".to_owned()),
            },
            WorktreeRecord {
                path: PathBuf::from(if cfg!(windows) {
                    "\\repo\\bare"
                } else {
                    "/repo/bare"
                }),
                head: None,
                branch: None,
                locked: Some(String::new()),
                prunable: Some(String::new()),
            },
        ];
        assert_eq!(records, expected);
        assert!(parse_worktree_records(b"").expect("no bytes").is_empty());
    }

    #[test]
    fn a_worktree_list_cut_short_is_refused_not_read_as_complete() {
        let cases: &[(&[u8], &str)] = &[
            (
                b"worktree /repo\0HEAD abc\0\0worktree /repo/wt\0HEAD abc",
                "ends without a terminator",
            ),
            (
                b"worktree /repo\0HEAD abc\0\0worktree /repo/wt\0HEAD abc\0",
                "record 1 is not closed",
            ),
            (b"worktree /repo\0", "record 0 is not closed"),
            (
                b"HEAD abc\0worktree /repo\0\0",
                "an attribute before its first record",
            ),
        ];
        for (bytes, expected) in cases {
            let refused = parse_worktree_records(bytes).expect_err("cut short");
            let text = message(refused);
            assert!(
                text.contains(expected),
                "{bytes:?}: expected {expected:?} in {text:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_worktree_path_keeps_every_byte_on_unix() {
        use std::os::unix::ffi::OsStrExt as _;

        let records = parse_worktree_records(b"worktree /repo/caf\xe9\0HEAD abc\0\0")
            .expect("every byte string is a Unix path");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path.as_os_str().as_bytes(), b"/repo/caf\xe9");
    }

    #[cfg(not(unix))]
    #[test]
    fn a_worktree_path_that_is_not_utf8_is_refused_with_its_offset() {
        let refused = parse_worktree_records(
            b"worktree C:/repo\0HEAD abc\0\0worktree C:/repo/caf\xe9\0HEAD abc\0\0",
        )
        .expect_err("a lossy spelling is not a worktree's identity");
        assert!(
            message(refused).contains("record 1 names a path that is not UTF-8 from byte 11"),
            "the refusal says which record and where"
        );
    }
}
