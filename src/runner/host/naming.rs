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
            let candidate = OsString::from(format!("{program}{extension}"));
            if !names.contains(&candidate) {
                names.push(candidate);
            }
        }
        names
    }

    fn extensions(pathext: Option<&OsStr>) -> Vec<String> {
        let listed: Vec<String> = pathext
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default()
            .split(';')
            .map(|entry| entry.trim().to_ascii_lowercase())
            .filter(|entry| entry.len() > 1 && entry.starts_with('.'))
            .collect();
        if listed.is_empty() {
            return Self::DEFAULT_PATHEXT
                .iter()
                .map(|extension| (*extension).to_owned())
                .collect();
        }
        listed
    }

    pub(super) fn is_program(self, path: &Path) -> bool {
        if !path.is_file() {
            return false;
        }
        match self {
            Self::Windows => true,
            Self::Posix => executable_bit(path),
        }
    }
}

#[cfg(unix)]
fn executable_bit(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable_bit(_path: &Path) -> bool {
    true
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
            if naming.is_program(&file) {
                return Ok(file);
            }
        }
    }
    Err(UpstrokeError::Refused {
        message: format!(
            "the host runner cannot execute `{program}`: nothing of that name is on the PATH \
             this runner composes ({searched} director{} searched{}, as {}). The runner resolves \
             a program name against the environment it composes (DESIGN.md:118), so the program \
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
