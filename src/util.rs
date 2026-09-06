//! Extended notes: `docs/internals/util.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::UpstrokeError;

pub(crate) mod terminal;

pub fn tail(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max {
        return trimmed.to_owned();
    }
    let start = trimmed.len() - max;
    let start = (start..trimmed.len())
        .find(|i| trimmed.is_char_boundary(*i))
        .unwrap_or(trimmed.len());
    format!("…{}", &trimmed[start..])
}

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

pub fn user_upstroke_dir() -> Option<PathBuf> {
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    } else {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    };
    Some(PathBuf::from(home?).join(".upstroke"))
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DurableStep {
    Wrote,
    Flushed,
    SyncedData,
    Truncated,
    SyncedFile,
    Renamed,
    SyncedDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableRecord {
    pub step: DurableStep,
    pub path: PathBuf,
    pub len: u64,
}

#[derive(Debug, Clone, Default)]
pub struct DurabilityLedger(Option<std::sync::Arc<std::sync::Mutex<Vec<DurableRecord>>>>);

impl DurabilityLedger {
    #[must_use]
    pub fn off() -> Self {
        Self(None)
    }

    #[must_use]
    pub fn recording() -> Self {
        Self(Some(std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))))
    }

    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.0.is_some()
    }

    pub fn record(&self, step: DurableStep, path: &Path, len: u64) {
        if let Some(log) = &self.0 {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(DurableRecord {
                    step,
                    path: path.to_path_buf(),
                    len,
                });
        }
    }

    #[must_use]
    pub fn records(&self) -> Vec<DurableRecord> {
        self.0.as_ref().map_or_else(Vec::new, |log| {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }

    #[must_use]
    pub fn records_for(&self, path: &Path) -> Vec<DurableRecord> {
        self.records()
            .into_iter()
            .filter(|record| record.path == path)
            .collect()
    }

    #[must_use]
    pub fn steps(&self) -> Vec<DurableStep> {
        self.records()
            .into_iter()
            .map(|record| record.step)
            .collect()
    }

    pub fn clear(&self) {
        if let Some(log) = &self.0 {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        }
    }
}

pub fn read_file_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let bound = file.metadata()?.len();
    let mut bytes = Vec::new();
    file.by_ref().take(bound).read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn write_text(path: &Path, content: &str) -> Result<(), UpstrokeError> {
    std::fs::write(path, content).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })
}

static BARRIERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn barriers_performed() -> u64 {
    BARRIERS.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BarrierCounts {
    pub file: u64,
    pub directory: u64,
}

thread_local! {
    static THREAD_BARRIERS: std::cell::Cell<BarrierCounts> =
        const { std::cell::Cell::new(BarrierCounts { file: 0, directory: 0 }) };
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn barriers_on_this_thread() -> BarrierCounts {
    THREAD_BARRIERS.with(std::cell::Cell::get)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarrierHalf {
    File,
    Directory,
}

fn count_barrier(half: BarrierHalf) {
    BARRIERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    THREAD_BARRIERS.with(|counts| {
        let mut current = counts.get();
        match half {
            BarrierHalf::File => current.file += 1,
            BarrierHalf::Directory => current.directory += 1,
        }
        counts.set(current);
    });
}

pub(crate) fn fsync_file(file: &std::fs::File) -> std::io::Result<()> {
    count_barrier(BarrierHalf::File);
    file.sync_all()
}

pub(crate) fn fsync_dir(dir: &Path) -> std::io::Result<()> {
    count_barrier(BarrierHalf::Directory);
    #[cfg(unix)]
    {
        std::fs::File::open(dir)?.sync_all()
    }
    #[cfg(windows)]
    {
        windows_fsync_dir(dir, WINDOWS_DIRECTORY_ACCESS)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = dir;
        Ok(())
    }
}

#[cfg(windows)]
const WINDOWS_DIRECTORY_ACCESS: u32 =
    windows_sys::Win32::Foundation::GENERIC_READ | windows_sys::Win32::Foundation::GENERIC_WRITE;

#[cfg(windows)]
fn windows_fsync_dir(dir: &Path, access: u32) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FlushFileBuffers, OPEN_EXISTING,
    };

    let mut wide: Vec<u16> = dir.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a directory path with an interior NUL cannot be opened",
        ));
    }
    wide.push(0);

    // Shared for read, write and delete: this handle exists for one flush and
    // must not be able to stop a concurrent command from using the directory.
    // SAFETY: `wide` is a live NUL-terminated UTF-16 path that outlives the
    // call, and the two pointer arguments are the documented "no security
    // attributes" and "no template" nulls.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `handle` is a live directory handle this function owns.
    let flushed = unsafe { FlushFileBuffers(handle) };
    let outcome = if flushed == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    };
    // SAFETY: same handle, closed exactly once, and not used afterwards.
    let _ = unsafe { CloseHandle(handle) };
    outcome
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), UpstrokeError> {
    let json = serde_json::to_string_pretty(value).map_err(|e| UpstrokeError::Parse {
        message: format!("serializing {}: {e}", path.display()),
    })?;
    write_text(path, &(json + "\n"))
}

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
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        (Ok(_), Err(_)) | (Err(_), Ok(_)) => false,
        (Err(left_error), Err(right_error)) => panic!(
            "neither `{}` ({left_error}) nor `{}` ({right_error}) resolves, so no comparison \
             of the two says anything",
            left.display(),
            right.display()
        ),
    }
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
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(rfc3339_utc(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(rfc3339_utc(86_400), "1970-01-02T00:00:00Z");
    }

    #[test]
    fn timestamps_sort_chronologically_as_strings() {
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
        let dir = std::env::temp_dir().join(format!("upstroke-util-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        std::fs::write(dir.join("bait.txt"), "").expect("bait");
        assert!(
            probe_extensions(&dir.join("bait.txt")).is_some(),
            "probe finds real paths"
        );
        assert!(find_program("bait.txt").is_none());
    }

    #[test]
    fn same_path_compares_directories_rather_than_spellings() {
        let root = std::env::temp_dir().join(format!("upstroke-util-same-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let inner = root.join("inner");
        std::fs::create_dir_all(&inner).expect("scratch directories");

        let detour = inner.join("..").join("inner").join(".");
        assert_ne!(detour, inner, "the fixture must differ as a string");
        assert!(same_path(&detour, &inner), "…and agree as a directory");

        assert!(!same_path(&root, &inner), "a parent is not its child");
        assert!(
            !same_path(&root.join("absent"), &root),
            "a path that does not resolve is not one that does"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[should_panic(expected = "so no comparison of the two says anything")]
    fn same_path_refuses_to_answer_when_neither_side_resolves() {
        let root =
            std::env::temp_dir().join(format!("upstroke-util-absent-{}", std::process::id()));
        let _ = same_path(&root.join("a"), &root.join("b"));
    }

    #[test]
    fn the_directory_barrier_runs_on_this_platform() {
        let root = std::env::temp_dir().join(format!("upstroke-util-fsync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch directory");

        let staged = root.join("record.tmp");
        std::fs::write(&staged, b"{}\n").expect("stage");
        fsync_dir(&root).expect("the barrier must run on this platform after a create");
        std::fs::rename(&staged, root.join("record")).expect("publish");
        fsync_dir(&root).expect("the barrier must run on this platform after a rename");

        let absent = fsync_dir(&root.join("absent"));
        assert!(
            absent.is_err(),
            "the barrier reported success for a directory that does not exist"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn the_directory_barrier_needs_exactly_the_access_it_asks_for() {
        let root = std::env::temp_dir().join(format!("upstroke-util-mask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch directory");
        std::fs::write(root.join("record"), b"{}\n").expect("a changed directory");

        windows_fsync_dir(&root, WINDOWS_DIRECTORY_ACCESS)
            .expect("the production mask must flush a directory");

        let read_only = windows_fsync_dir(&root, windows_sys::Win32::Foundation::GENERIC_READ);
        let refusal = read_only
            .expect_err("a read-only handle must not be able to flush; the mask is over-asking");
        assert_eq!(
            refusal.raw_os_error(),
            Some(5),
            "the refusal must be ERROR_ACCESS_DENIED, which is what makes the write \
             right load-bearing rather than incidental: {refusal:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
