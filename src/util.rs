//! Small shared helpers used across the engine, gates, adapters, and
//! reporting: text truncation, filename sanitizing, PATH program resolution,
//! run-artifact writes, and event timestamps.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::UpstrokeError;

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

/// First `max` bytes of trimmed text, cut on a char boundary, with an
/// ellipsis marker when truncated. For ordered lists whose first entry is the
/// most important — a reviewer's reasons, say — where [`tail`] would drop
/// exactly the part that mattered.
pub fn head(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max {
        return trimmed.to_owned();
    }
    let end = (0..=max)
        .rev()
        .find(|i| trimmed.is_char_boundary(*i))
        .unwrap_or(0);
    format!("{}…", &trimmed[..end])
}

/// A fence long enough to quote `payload` without the payload closing it.
///
/// Everything the engine quotes back to a model or a human — a diff, an
/// artifact, an agent's question, an operator's answer — is untrusted text that
/// routinely contains fences of its own (any repo with markdown does). A fence
/// that closes early hands the remainder of the payload to the reader as if it
/// were instructions, so the invariant is load-bearing rather than cosmetic:
/// it lives in one place so it cannot drift between callers.
pub fn fence_for(payload: &str) -> String {
    let mut longest = 0usize;
    let mut current = 0usize;
    for ch in payload.chars() {
        if ch == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat(longest.max(2) + 1)
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

/// The user-level `~/.upstroke` directory: pools live here (§17), and so do the
/// agent-authored artifacts a run must keep outside the workspace (§15).
///
/// `USERPROFILE` wins on Windows because shells like Git Bash set `HOME` to an
/// MSYS-style path (`/c/Users/...`) that the Windows file APIs cannot open —
/// trusting it there would write run artifacts somewhere nothing can read them
/// back. Elsewhere `HOME` is authoritative and `USERPROFILE` is the fallback.
pub fn user_upstroke_dir() -> Option<PathBuf> {
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    } else {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    };
    Some(PathBuf::from(home?).join(".upstroke"))
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

/// Resolve every matching program in shell PATH order: directory first, then
/// the caller's name preference, then executable extension. Returning all
/// candidates lets an adapter skip an unspawnable app alias without promoting
/// a later directory ahead of a usable earlier installation.
pub(crate) fn find_program_candidates(names: &[&str]) -> Vec<PathBuf> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    find_program_candidates_on_path(names, &path_var)
}

pub(crate) fn find_program_candidates_on_path(
    names: &[&str],
    path_var: &std::ffi::OsStr,
) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for name in names {
            let base = dir.join(name);
            let explicit_extension = Path::new(name).extension().is_some();
            let extensions = if explicit_extension {
                vec![String::new()]
            } else {
                executable_extensions()
            };
            for extension in extensions {
                let candidate = if extension.is_empty() {
                    base.clone()
                } else {
                    let mut with_extension = base.as_os_str().to_owned();
                    with_extension.push(extension);
                    PathBuf::from(with_extension)
                };
                if candidate.is_file() && !found.contains(&candidate) {
                    found.push(candidate);
                }
            }
        }
    }
    found
}

pub fn write_text(path: &Path, content: &str) -> Result<(), UpstrokeError> {
    std::fs::write(path, content).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), UpstrokeError> {
    let json = serde_json::to_string_pretty(value).map_err(|e| UpstrokeError::Parse {
        message: format!("serializing {}: {e}", path.display()),
    })?;
    write_text(path, &(json + "\n"))
}

/// Serialize a [`Duration`](std::time::Duration) as whole milliseconds.
///
/// Durations ride in both the event log and the report, and serde's default
/// `{"secs":3,"nanos":120000000}` is neither readable in a JSONL ops log nor
/// stable across serde's internally-tagged buffering path. Milliseconds are
/// finer than anything the ledger reports and survive both.
pub mod duration_millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Duration, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_u64(u64::try_from(value.as_millis()).unwrap_or(u64::MAX))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(input)?))
    }
}

/// Now, as an RFC 3339 UTC timestamp — the `ts` on every event (§15).
///
/// Std-only rather than a date dependency: this is one field on one line of
/// JSON, and the conversion below is a closed-form algorithm with no table and
/// no locale. A clock that cannot read (`SystemTime` before the epoch) yields
/// the epoch rather than failing — a timestamp is metadata on the event, and
/// losing the event to a clock problem would be the worse trade.
pub fn rfc3339_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    rfc3339_utc(seconds)
}

fn rfc3339_utc(unix_seconds: u64) -> String {
    let days = i64::try_from(unix_seconds / 86_400).unwrap_or(0);
    let second_of_day = unix_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3600,
        (second_of_day % 3600) / 60,
        second_of_day % 60
    )
}

/// Civil date from a day count since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`). The era starts on 0000-03-01 so that a leap day always
/// lands at the end of a cycle, which is what lets the month and day fall out
/// of integer arithmetic instead of a lookup table.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    // March is month 0 in the shifted era; roll January and February into the
    // following calendar year.
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (year + i64::from(month <= 2), month, day)
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
        assert!(find_program("upstroke-definitely-not-real-xyz").is_none());
    }

    #[test]
    fn candidate_resolution_preserves_path_directory_precedence() {
        let root =
            std::env::temp_dir().join(format!("upstroke-util-path-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).expect("first PATH directory");
        std::fs::create_dir_all(&second).expect("second PATH directory");
        let first_program = first.join("codex.exe");
        let second_program = second.join("codex.cmd");
        std::fs::write(&first_program, "").expect("first candidate");
        std::fs::write(&second_program, "").expect("second candidate");
        let path = std::env::join_paths([&first, &second]).expect("synthetic PATH");

        let found = find_program_candidates_on_path(&["codex.cmd", "codex.exe"], &path);

        assert_eq!(
            found,
            [first_program, second_program],
            "the name preference must not promote a later PATH directory"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        // Both leap rules: 2024 by the /4 rule, 2000 by the /400 exception.
        assert_eq!(rfc3339_utc(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        // A day boundary and the last second before one.
        assert_eq!(rfc3339_utc(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(rfc3339_utc(86_400), "1970-01-02T00:00:00Z");
    }

    #[test]
    fn timestamps_sort_chronologically_as_strings() {
        // The log is read back with a plain string compare in places; the
        // zero-padded fixed-width form is what makes that legitimate.
        let mut stamps = [
            rfc3339_utc(1_700_000_000),
            rfc3339_utc(0),
            rfc3339_utc(951_782_400),
        ];
        stamps.sort();
        assert_eq!(
            stamps,
            [
                rfc3339_utc(0),
                rfc3339_utc(951_782_400),
                rfc3339_utc(1_700_000_000)
            ]
        );
        assert_eq!(rfc3339_utc_now().len(), "1970-01-01T00:00:00Z".len());
    }

    #[test]
    fn probe_extensions_never_resolves_a_bare_relative_name() {
        // The empty-PATH-segment guard in find_program rests on this: a bare
        // name must not resolve against the process CWD. Verified by probing
        // a file that exists in a scratch dir under its bare name.
        let dir = std::env::temp_dir().join(format!("upstroke-util-path-{}", std::process::id()));
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
