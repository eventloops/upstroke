//! Locating and invoking an agent CLI — the parts every adapter needs and
//! none of them should own privately.
//!
//! Windows is why this module exists. Both agent CLIs ship as npm packages, so
//! the thing on PATH is frequently a `.cmd` shim rather than a native
//! executable, and `CreateProcess` cannot exec a batch script. That used to be
//! handled here by building a `cmd /C` command line by hand and passing it
//! through `raw_arg`.
//!
//! **It is not any more, and the reason is worth recording.** `raw_arg` opts
//! out of everything the standard library does for batch targets, including the
//! argument escaping added in Rust 1.77.2 for CVE-2024-24576; this crate is
//! edition 2024, so that fix is unconditionally present. Measured against a
//! real npm-shape shim, the hand-rolled version expanded `%VAR%` inside
//! arguments — turning `--allow-tool=shell(echo %PATH%)` into the machine's
//! entire PATH — while `Command::args` carried every case through intact:
//! `&`, `|`, `%`, embedded quotes, `^`, spaces, and the empty argument.
//!
//! Copilot is what made that matter. Its permission surface is argv, so gate
//! commands — strings a user writes in `tactus.toml` — now reach a Windows
//! command line, and a mangled `--allow-tool=shell(<gate>)` is a permission
//! grant that no longer matches the command it is meant to authorize. The
//! module comment used to argue that two copies of the quoting logic would be
//! two chances to get it wrong. The right number was zero.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use crate::error::TactusError;
use crate::util;

/// A located agent binary and how to spawn it.
#[derive(Debug, Clone)]
pub struct Invocation {
    path: PathBuf,
}

impl Invocation {
    /// The command to run, with `args` handed to the standard library verbatim.
    ///
    /// Nothing is quoted, escaped, or wrapped here on purpose: `std` knows
    /// whether the resolved path is a batch shim and applies the right rules,
    /// and every attempt to help it has been a way to get this wrong.
    pub fn command(&self, args: &[String]) -> Command {
        let mut cmd = Command::new(&self.path);
        cmd.args(args);
        cmd
    }

    pub fn display(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

/// Resolve the first of `names` that exists on PATH, caching the answer in the
/// adapter's own `cache`.
///
/// PATH resolution is process-stable and the engine builds one command per
/// task after probing, so each adapter resolves once. The cache is passed in
/// rather than kept here because two adapters must not share one slot.
///
/// `missing` renders the error when nothing resolves; it takes the names that
/// were tried so the message can name them.
pub fn locate(
    names: &[&str],
    cache: &OnceLock<Option<Invocation>>,
    missing: impl FnOnce(&[&str]) -> String,
) -> Result<Invocation, TactusError> {
    locate_with(names, cache, |_| true, missing)
}

/// Resolve the first usable candidate in shell PATH order and cache it.
///
/// Some platforms expose aliases that look like files but cannot be spawned.
/// The predicate lets an adapter reject one of those and continue through the
/// remaining PATH entries. Rejection happens before the cache is populated, so
/// a bad alias cannot poison every later probe and attempt in this process.
pub fn locate_with(
    names: &[&str],
    cache: &OnceLock<Option<Invocation>>,
    usable: impl FnMut(&Invocation) -> bool,
    missing: impl FnOnce(&[&str]) -> String,
) -> Result<Invocation, TactusError> {
    let mut usable = usable;
    cache
        .get_or_init(|| {
            // util::find_program_candidates skips empty PATH segments, which
            // would otherwise resolve a bare name against the current
            // directory — i.e. run a binary out of the repo being worked on.
            first_usable(
                util::find_program_candidates(names)
                    .into_iter()
                    .map(|path| Invocation { path }),
                &mut usable,
            )
        })
        .clone()
        .ok_or_else(|| TactusError::Agent {
            message: missing(names),
        })
}

fn first_usable(
    candidates: impl IntoIterator<Item = Invocation>,
    usable: &mut impl FnMut(&Invocation) -> bool,
) -> Option<Invocation> {
    candidates.into_iter().find(|candidate| usable(candidate))
}

/// First `digits.digits.digits` token wins; otherwise the trimmed first line
/// verbatim (`--version` formats have churned before, in both CLIs).
pub fn extract_version(stdout: &str) -> String {
    let first_line = stdout.lines().next().unwrap_or_default().trim();
    first_line
        .split_whitespace()
        .find(|token| {
            let mut parts = token.trim_start_matches('v').split('.');
            let numeric = |s: Option<&str>| {
                s.is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
            };
            numeric(parts.next()) && numeric(parts.next()) && parts.next().is_some()
        })
        // Trailing punctuation is not part of a version. The Copilot CLI ends
        // its line with a full stop — `GitHub Copilot CLI 1.0.78.` — which
        // otherwise rides along into `Caps.version` and out through every
        // message that quotes it (`tactus capacity`, and the probe refusal that
        // names the version an adapter would not support).
        .map(|t| {
            t.trim_start_matches('v')
                .trim_end_matches(['.', ',', ';'])
                .to_owned()
        })
        .unwrap_or_else(|| first_line.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::Path;

    fn invocation(path: &str) -> Invocation {
        Invocation {
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn arguments_reach_the_command_untouched() {
        // The property the deleted quoting code kept breaking. These are the
        // exact shapes Copilot's permission surface produces: a gate command
        // with spaces and parentheses, a cmd metacharacter, a percent sign, an
        // embedded quote, and an empty argument.
        let args: Vec<String> = [
            "-s",
            "--allow-tool=shell(cargo test)",
            "--allow-tool=shell(echo hi & whoami)",
            "--allow-tool=shell(echo %PATH%)",
            r#"--allow-tool=shell(cargo test -- --exact "my test")"#,
            "--setting-sources",
            "",
        ]
        .map(str::to_owned)
        .to_vec();

        let cmd = invocation(r"C:\Users\John Smith\npm\copilot.cmd").command(&args);
        assert_eq!(
            cmd.get_program(),
            OsStr::new(r"C:\Users\John Smith\npm\copilot.cmd"),
            "the shim is the program; nothing wraps it in a shell"
        );
        let seen: Vec<&OsStr> = cmd.get_args().collect();
        let expected: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        assert_eq!(seen, expected, "every argument survives verbatim");
    }

    #[test]
    fn version_extraction_handles_known_formats() {
        assert_eq!(extract_version("2.1.35 (Claude Code)\n"), "2.1.35");
        assert_eq!(extract_version("claude v1.0.128\n"), "1.0.128");
        assert_eq!(extract_version("weird output\n"), "weird output");
        // Verbatim from the Copilot CLI: the sentence's full stop is not part
        // of the version, and rode into `Caps.version` when it was not trimmed.
        assert_eq!(
            extract_version(
                "GitHub Copilot CLI 1.0.78.\nRun 'copilot update' to check for updates.\n"
            ),
            "1.0.78"
        );
    }

    #[test]
    fn a_missing_binary_reports_every_name_it_tried() {
        static CACHE: OnceLock<Option<Invocation>> = OnceLock::new();
        let names = ["tactus-definitely-not-a-real-binary"];
        let error = locate(&names, &CACHE, |tried| {
            format!("not found (looked for {})", tried.join(", "))
        })
        .expect_err("nothing should resolve");
        assert!(
            error
                .to_string()
                .contains("tactus-definitely-not-a-real-binary"),
            "got: {error}"
        );
    }

    #[test]
    fn an_unusable_candidate_is_skipped_before_the_answer_is_cached() {
        let first = invocation(r"C:\WindowsApps\codex.exe");
        let second = invocation(r"C:\Users\me\npm\codex.cmd");
        let mut inspected = Vec::new();

        let selected = first_usable([first, second.clone()], &mut |candidate| {
            inspected.push(candidate.display());
            candidate.display() == second.display()
        })
        .expect("the later usable installation wins");

        assert_eq!(selected.display(), second.display());
        assert_eq!(inspected.len(), 2, "the bad alias was actually tested");
    }

    /// A `.cmd` shim really does execute, and an argument really does arrive.
    ///
    /// Asserting on the constructed `Command` proves we hand `std` the right
    /// thing; only spawning proves `std` then does the right thing with a batch
    /// target, which is the half the old hand-rolled code got wrong.
    #[cfg(windows)]
    #[test]
    fn a_batch_shim_runs_and_receives_its_argument() {
        let dir = std::env::temp_dir().join(format!("tactus-bin-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let shim = dir.join("tactus-test-shim.cmd");
        // `%~1` strips the quotes the child got; a benign argument keeps this
        // about plumbing rather than about batch re-parsing.
        std::fs::write(&shim, "@echo off\r\necho GOT:%~1\r\n").expect("write shim");

        let out = invocation(&shim.to_string_lossy())
            .command(&["hello world".to_owned()])
            .output()
            .expect("the shim spawns");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("GOT:hello world"),
            "the shim ran and saw its argument: {stdout:?}"
        );
    }

    #[test]
    fn display_is_the_resolved_path() {
        assert_eq!(
            invocation("/usr/local/bin/claude").display(),
            "/usr/local/bin/claude"
        );
        assert!(Path::new("/usr/local/bin/claude").is_absolute() || cfg!(windows));
    }
}
