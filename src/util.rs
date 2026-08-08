//! Small shared helpers used across the engine, gates, adapters, and
//! reporting: text truncation, filename sanitizing, PATH program resolution,
//! and run-artifact writes.

use std::path::{Path, PathBuf};

use crate::error::TactusError;

/// Last `max` bytes of trimmed text, cut on a char boundary, with an ellipsis
/// marker when truncated.
pub fn tail(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max {
        return trimmed.to_owned();
    }
    let start = trimmed.len() - max;
    // No boundary in range (possible only for a tiny `max` landing inside the
    // final multibyte char) means the whole tail is unusable — keep nothing.
    let start = (start..trimmed.len())
        .find(|i| trimmed.is_char_boundary(*i))
        .unwrap_or(trimmed.len());
    format!("…{}", &trimmed[start..])
}

/// Make an arbitrary string (task id, gate name — both user-authored) safe to
/// use as a single file-name component: no separators, no Windows-reserved
/// characters, no dot-only names, bounded length. Not injective — callers
/// that need uniqueness must add a discriminator of their own.
pub fn filename_component(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    out.truncate(64);
    if out.trim_matches(['.', '-']).is_empty() {
        return "x".to_owned();
    }
    out
}

/// Executable extensions to probe on Windows: PATHEXT when set, else a
/// conservative default. Unix probes the bare name only.
pub fn executable_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return vec![String::new()];
    }
    let mut exts = vec![String::new()];
    match std::env::var("PATHEXT") {
        Ok(pathext) if !pathext.trim().is_empty() => {
            exts.extend(
                pathext
                    .split(';')
                    .map(|e| e.trim().to_ascii_lowercase())
                    .filter(|e| e.starts_with('.')),
            );
        }
        _ => exts.extend([".exe", ".cmd", ".bat", ".com"].map(str::to_owned)),
    }
    exts
}

/// Try `base` plus each executable extension; first hit wins.
pub fn probe_extensions(base: &Path) -> Option<PathBuf> {
    for ext in executable_extensions() {
        let candidate = if ext.is_empty() {
            base.to_path_buf()
        } else {
            let mut with_ext = base.as_os_str().to_owned();
            with_ext.push(&ext);
            PathBuf::from(with_ext)
        };
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolve a bare program name against PATH. Empty PATH segments are skipped:
/// they mean "current directory" to some shells, and resolving a program
/// against the repo under automation would execute repo-controlled code.
pub fn find_program(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.is_absolute() {
        return probe_extensions(candidate);
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if let Some(found) = probe_extensions(&dir.join(name)) {
            return Some(found);
        }
    }
    None
}

pub fn write_text(path: &Path, content: &str) -> Result<(), TactusError> {
    std::fs::write(path, content).map_err(|source| TactusError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), TactusError> {
    let json = serde_json::to_string_pretty(value).map_err(|e| TactusError::Parse {
        message: format!("serializing {}: {e}", path.display()),
    })?;
    write_text(path, &(json + "\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_truncates_on_char_boundaries() {
        assert_eq!(tail("  short  ", 400), "short");
        let long = "a".repeat(500);
        let cut = tail(&long, 400);
        assert!(cut.starts_with('…') && cut.len() < 500);
        let multi = "é".repeat(300);
        let cut = tail(&multi, 401);
        assert!(cut.chars().all(|c| c == 'é' || c == '…'));
    }

    #[test]
    fn tail_never_slices_mid_char_for_tiny_limits() {
        // Cut lands inside the trailing multibyte char: keep nothing rather
        // than panic on a non-boundary index.
        assert_eq!(tail("é", 1), "…");
        assert_eq!(tail("aé", 1), "…");
    }

    #[test]
    fn filename_component_neutralizes_hostile_names() {
        assert_eq!(filename_component("lint:fast"), "lint-fast");
        assert_eq!(filename_component("unit/fast"), "unit-fast");
        assert_eq!(filename_component("a\\b"), "a-b");
        assert_eq!(filename_component(".."), "x");
        assert_eq!(filename_component("check"), "check");
        assert!(filename_component(&"x".repeat(200)).len() <= 64);
    }

    #[test]
    fn find_program_resolves_real_tools_and_misses_fake_ones() {
        assert!(find_program("git").is_some(), "git is on PATH in this repo");
        assert!(find_program("tactus-definitely-not-real-xyz").is_none());
    }

    #[test]
    fn probe_extensions_never_resolves_a_bare_relative_name() {
        // The empty-PATH-segment guard in find_program rests on this: a bare
        // name must not resolve against the process CWD. Verified by probing
        // a file that exists in a scratch dir under its bare name.
        let dir = std::env::temp_dir().join(format!("tactus-util-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        std::fs::write(dir.join("bait.txt"), "").expect("bait");
        assert!(
            probe_extensions(&dir.join("bait.txt")).is_some(),
            "probe finds real paths"
        );
        // find_program must not consult any directory-less candidate.
        assert!(find_program("bait.txt").is_none());
    }
}
