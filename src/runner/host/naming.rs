//! Extended notes: `docs/internals/runner/host/naming.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::error::UpstrokeError;

use super::{KeyCase, SEARCHES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum ProgramNaming {
    Posix,
    Windows,
}

impl ProgramNaming {
    pub(super) const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }

    const DEFAULT_PATHEXT: &'static [&'static str] = &[".com", ".exe", ".bat", ".cmd"];

    pub(super) fn is_bare_name(self, program: &str) -> bool {
        if program.is_empty() {
            return false;
        }
        !program.chars().any(|c| match self {
            Self::Posix => c == '/',
            Self::Windows => matches!(c, '/' | '\\' | ':'),
        })
    }

    fn candidates(self, program: &str, pathext: Option<&OsStr>) -> Vec<OsString> {
        let mut names = Vec::new();
        if self == Self::Posix {
            names.push(OsString::from(program));
            return names;
        }
        if Path::new(program).extension().is_some() {
            names.push(OsString::from(program));
        }
        for extension in Self::extensions(pathext) {
            let mut candidate = OsString::from(program);
            candidate.push(&extension);
            if !names.contains(&candidate) {
                names.push(candidate);
            }
        }
        names
    }

    fn extensions(pathext: Option<&OsStr>) -> Vec<OsString> {
        let listed = pathext.map(pathext_entries).unwrap_or_default();
        if listed.is_empty() {
            return Self::DEFAULT_PATHEXT
                .iter()
                .map(|extension| OsString::from(*extension))
                .collect();
        }
        listed
    }

    pub(super) fn is_program(self, path: &Path) -> Result<bool, std::io::Error> {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if !metadata.is_file() {
            return Ok(false);
        }
        Ok(match self {
            Self::Windows => true,
            Self::Posix => executable_bit(&metadata),
        })
    }
}

#[cfg(unix)]
fn executable_bit(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable_bit(_metadata: &std::fs::Metadata) -> bool {
    true
}

#[cfg(unix)]
fn pathext_entries(pathext: &OsStr) -> Vec<OsString> {
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    normalised_extensions(pathext.as_bytes())
        .into_iter()
        .map(OsString::from_vec)
        .collect()
}

#[cfg(windows)]
fn pathext_entries(pathext: &OsStr) -> Vec<OsString> {
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    let units: Vec<u16> = pathext.encode_wide().collect();
    normalised_extensions(&units)
        .into_iter()
        .map(|entry| OsString::from_wide(&entry))
        .collect()
}

fn normalised_extensions<U>(value: &[U]) -> Vec<Vec<U>>
where
    U: Copy + PartialEq + From<u8> + TryInto<u8>,
{
    let semicolon = U::from(b';');
    let dot = U::from(b'.');
    value
        .split(|unit| *unit == semicolon)
        .map(|entry| {
            ascii_trimmed(entry)
                .iter()
                .map(|unit| ascii_lowered(*unit))
                .collect::<Vec<U>>()
        })
        .filter(|entry| entry.len() > 1 && entry.first() == Some(&dot))
        .collect()
}

fn ascii_trimmed<U>(entry: &[U]) -> &[U]
where
    U: Copy + TryInto<u8>,
{
    let mut slice = entry;
    while let Some((first, rest)) = slice.split_first() {
        if !ascii_space(*first) {
            break;
        }
        slice = rest;
    }
    while let Some((last, rest)) = slice.split_last() {
        if !ascii_space(*last) {
            break;
        }
        slice = rest;
    }
    slice
}

fn ascii_space<U>(unit: U) -> bool
where
    U: Copy + TryInto<u8>,
{
    matches!(ascii_byte(unit), Some(byte) if byte.is_ascii_whitespace())
}

fn ascii_lowered<U>(unit: U) -> U
where
    U: Copy + From<u8> + TryInto<u8>,
{
    match ascii_byte(unit) {
        Some(byte) => U::from(byte.to_ascii_lowercase()),
        None => unit,
    }
}

fn ascii_byte<U>(unit: U) -> Option<u8>
where
    U: Copy + TryInto<u8>,
{
    let Ok(byte) = unit.try_into() else {
        return None;
    };
    byte.is_ascii().then_some(byte)
}

pub(super) fn composed_value<'a>(
    composed: &'a [(OsString, OsString)],
    case: KeyCase,
    key: &str,
) -> Option<&'a OsStr> {
    composed
        .iter()
        .find(|(name, _)| case.same_key(name, OsStr::new(key)))
        .map(|(_, value)| value.as_os_str())
}

pub(super) fn resolve_program(
    program: &str,
    composed: &[(OsString, OsString)],
    case: KeyCase,
    naming: ProgramNaming,
) -> Result<PathBuf, UpstrokeError> {
    SEARCHES.with(|count| count.set(count.get() + 1));
    if !naming.is_bare_name(program) {
        return Ok(PathBuf::from(program));
    }
    let path = composed_value(composed, case, "PATH");
    let candidates = naming.candidates(program, composed_value(composed, case, "PATHEXT"));
    let mut searched = 0_usize;
    let mut skipped = 0_usize;
    for dir in std::env::split_paths(path.unwrap_or_else(|| OsStr::new(""))) {
        if !dir.is_absolute() {
            skipped += 1;
            continue;
        }
        searched += 1;
        for candidate in &candidates {
            let file = dir.join(candidate);
            match naming.is_program(&file) {
                Ok(true) => return Ok(file),
                Ok(false) => {}
                Err(source) => {
                    return Err(UpstrokeError::Filesystem {
                        operation: "stat",
                        path: file,
                        source,
                    });
                }
            }
        }
    }
    Err(UpstrokeError::Refused {
        message: format!(
            "the host runner cannot execute `{program}`: nothing of that name is on the PATH \
             this runner composes ({searched} director{} searched{}, as {}). The runner resolves \
             a program name against the environment it composes (DESIGN.md §6), so the program \
             must be installed inside the boundary that executes it — on PATH for the host \
             runner, in the image for a container runner. PATH: {}",
            if searched == 1 { "y" } else { "ies" },
            match skipped {
                0 => String::new(),
                1 => ", 1 PATH entry skipped as not absolute".to_owned(),
                n => format!(", {n} PATH entries skipped as not absolute"),
            },
            candidates
                .iter()
                .map(|candidate| format!("`{}`", candidate.to_string_lossy()))
                .collect::<Vec<_>>()
                .join(", "),
            path.unwrap_or_else(|| OsStr::new("<unset>"))
                .to_string_lossy()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::{ProgramNaming, normalised_extensions, resolve_program};
    use crate::error::UpstrokeError;
    use crate::runner::host::KeyCase;
    use std::ffi::{OsStr, OsString};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn composed(pairs: &[(&str, &OsStr)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(key, value)| (OsString::from(*key), (*value).to_os_string()))
            .collect()
    }

    /// A `PATH` entry every platform refuses to `stat` before it reaches the
    /// filesystem: an interior NUL is `InvalidInput`, never `NotFound`, so it
    /// is the one undetermined candidate no filesystem state can produce or
    /// remove. The assertion is what keeps the fixture honest.
    fn undeterminable_directory() -> PathBuf {
        let directory = std::env::temp_dir().join("upstroke-naming\u{0}sweep");
        let probe = std::fs::metadata(directory.join("x"))
            .expect_err("a path with an interior NUL cannot be stat'ed");
        assert_ne!(
            probe.kind(),
            std::io::ErrorKind::NotFound,
            "the fixture must fail with something other than not-found to witness anything"
        );
        directory
    }

    /// A directory this process owns the name of and never creates, so every
    /// candidate under it is a genuine `NotFound` whatever else is in the
    /// ambient temporary directory (`SWEEP-HOST-NAMING-005`). Unique per
    /// call: process id, a per-process counter and the clock.
    fn never_created_directory(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "upstroke-naming-{tag}-{}-{nonce}-{nanos}",
            std::process::id()
        ));
        let probe = std::fs::symlink_metadata(&directory)
            .expect_err("a directory nothing has created must not exist");
        assert_eq!(
            probe.kind(),
            std::io::ErrorKind::NotFound,
            "the fixture must be absent, not merely unreadable, to be a control"
        );
        directory
    }

    /// This test binary, as the one program every platform has installed
    /// and executable by construction: the directory it lives in and its
    /// bare file name.
    fn this_test_binary() -> (PathBuf, String) {
        let exe = std::env::current_exe().expect("the test binary has a path");
        let name = exe
            .file_name()
            .and_then(OsStr::to_str)
            .expect("the test binary has a Unicode file name")
            .to_owned();
        let dir = exe
            .parent()
            .expect("the test binary lives in a directory")
            .to_path_buf();
        assert!(
            ProgramNaming::current().is_bare_name(&name),
            "the fixture must be a bare name for the search to run at all: {name}"
        );
        (dir, name)
    }

    #[test]
    fn a_candidate_this_platform_cannot_stat_is_never_reported_as_absence() {
        let path = undeterminable_directory().into_os_string();
        let message = resolve_program(
            "x",
            &composed(&[("PATH", path.as_os_str())]),
            KeyCase::Sensitive,
            ProgramNaming::current(),
        )
        .expect_err("a candidate the platform could not answer for is not an answer")
        .to_string();
        assert!(
            message.contains("failed to stat") && message.contains("upstroke-naming"),
            "the refusal must name the failed operation and its candidate: {message}"
        );
        assert!(
            !message.contains("nothing of that name"),
            "an undetermined candidate was reported as absence: {message}"
        );
    }

    #[test]
    fn a_candidate_that_is_merely_absent_is_still_absence() {
        let path = never_created_directory("absent").into_os_string();
        let message = resolve_program(
            "upstroke-no-such-program",
            &composed(&[("PATH", path.as_os_str())]),
            KeyCase::Sensitive,
            ProgramNaming::current(),
        )
        .expect_err("nothing of that name is installed there")
        .to_string();
        assert!(
            message.contains("nothing of that name"),
            "a not-found candidate must stay absence: {message}"
        );
    }

    #[test]
    fn an_undetermined_candidate_stops_the_search_before_a_later_match() {
        let (dir, name) = this_test_binary();
        let undeterminable = undeterminable_directory();
        let path = std::env::join_paths([undeterminable.as_path(), dir.as_path()])
            .expect("neither entry carries the PATH separator");

        let error = resolve_program(
            &name,
            &composed(&[("PATH", path.as_os_str())]),
            KeyCase::Sensitive,
            ProgramNaming::current(),
        )
        .expect_err("a candidate the platform could not answer for is not walked past");
        match error {
            UpstrokeError::Filesystem {
                operation,
                path,
                source,
            } => {
                assert_eq!(operation, "stat");
                assert_eq!(
                    path,
                    undeterminable.join(&name),
                    "the candidate reported must be the undetermined one, not the later match"
                );
                assert_ne!(
                    source.kind(),
                    std::io::ErrorKind::NotFound,
                    "the source carried must be the platform's own answer"
                );
            }
            other => panic!("expected the stat failure to propagate, got: {other}"),
        }
    }

    #[test]
    fn a_directory_that_is_merely_absent_is_walked_past_to_a_later_match() {
        let (dir, name) = this_test_binary();
        let absent = never_created_directory("walked-past");
        let path = std::env::join_paths([absent.as_path(), dir.as_path()])
            .expect("neither entry carries the PATH separator");

        let resolved = resolve_program(
            &name,
            &composed(&[("PATH", path.as_os_str())]),
            KeyCase::Sensitive,
            ProgramNaming::current(),
        )
        .expect("a not-found miss continues the search to the installed program");
        assert_eq!(
            resolved,
            dir.join(&name),
            "the search must reach the program past an absent directory"
        );
    }

    #[test]
    fn a_pathext_entry_no_string_can_carry_reaches_the_candidate_intact() {
        #[cfg(unix)]
        let (pathext, folded) = {
            use std::os::unix::ffi::OsStringExt as _;
            (
                OsString::from_vec(vec![b'.', 0x80, b'X']),
                OsString::from_vec(vec![b'.', 0x80, b'x']),
            )
        };
        #[cfg(windows)]
        let (pathext, folded) = {
            use std::os::windows::ffi::OsStringExt as _;
            (
                OsString::from_wide(&[u16::from(b'.'), 0xd800, u16::from(b'X')]),
                OsString::from_wide(&[u16::from(b'.'), 0xd800, u16::from(b'x')]),
            )
        };

        assert!(
            pathext.to_str().is_none(),
            "the fixture is valid Unicode, so it witnesses nothing"
        );

        let candidates = ProgramNaming::Windows.candidates("claude", Some(pathext.as_os_str()));
        let mut expected = OsString::from("claude");
        expected.push(&folded);
        assert_eq!(
            candidates,
            vec![expected],
            "the candidate must carry PATHEXT's own code units, ASCII-folded and nothing else"
        );

        let mut lossy = OsString::from("claude");
        lossy.push(pathext.to_string_lossy().to_lowercase());
        assert!(
            !candidates.contains(&lossy),
            "the candidate was built through a lossy conversion: {candidates:?}"
        );
    }

    #[test]
    fn the_pathext_grammar_reads_the_same_over_both_code_unit_widths() {
        let bytes = b" .CoM ;; x ; .b ".to_vec();
        let wide: Vec<u16> = bytes.iter().map(|byte| u16::from(*byte)).collect();

        let from_bytes = normalised_extensions(&bytes);
        assert_eq!(
            from_bytes,
            vec![b".com".to_vec(), b".b".to_vec()],
            "an entry is trimmed and ASCII-folded, and only an extension survives"
        );

        let widened: Vec<Vec<u16>> = from_bytes
            .iter()
            .map(|entry| entry.iter().map(|byte| u16::from(*byte)).collect())
            .collect();
        assert_eq!(
            normalised_extensions(&wide),
            widened,
            "the width Windows reads PATHEXT in must read it exactly as the byte width does"
        );
    }
}
