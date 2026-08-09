//! Locating and invoking an agent CLI — the parts every adapter needs and
//! none of them should own privately.
//!
//! Windows is first-class here, and that is the whole reason this module
//! exists. Both agent CLIs ship as npm packages, so the thing on PATH is
//! frequently a `.cmd` shim rather than a native executable, and `CreateProcess`
//! cannot exec a batch script. Running one means going through `cmd /C`, whose
//! quoting rules are not `CommandLineToArgvW`'s — so the command line has to be
//! built by hand. That logic is subtle enough that two copies of it would be two
//! chances to get it wrong; Copilot in particular passes
//! `--allow-tool=shell(cargo test)`, which carries spaces *and* parentheses
//! through exactly that path.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::error::TactusError;
use crate::util;

/// A located agent binary and how to spawn it.
#[derive(Debug, Clone)]
pub struct Invocation {
    path: PathBuf,
    /// `.cmd`/`.bat` shims (npm installs) are batch scripts: CreateProcess
    /// cannot exec them, so they run through `cmd /C`.
    via_cmd_shell: bool,
}

impl Invocation {
    pub fn command(&self, args: &[String]) -> Command {
        if self.via_cmd_shell {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C");
            // std quotes each arg for CommandLineToArgvW, which cmd.exe does
            // NOT follow: with more than one quoted argument its
            // strip-first-and-last-quote rule mangles the shim path. Build the
            // whole line ourselves and pass it verbatim, wrapped in the outer
            // quote pair cmd strips.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.raw_arg(cmd_c_line(&self.path, args));
            }
            #[cfg(not(windows))]
            {
                cmd.arg(&self.path).args(args);
            }
            cmd
        } else {
            let mut cmd = Command::new(&self.path);
            cmd.args(args);
            cmd
        }
    }

    pub fn display(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

/// `"<program>" <args…>` wrapped in the outer quote pair `cmd /C` strips.
///
/// Only reachable on Windows, but compiled and unit-tested everywhere: the
/// quoting rules are pure string logic, and a bug in them should be caught by
/// whichever platform's CI job runs first, not only by the Windows one.
#[cfg_attr(not(windows), allow(dead_code))]
fn cmd_c_line(program: &Path, args: &[String]) -> String {
    let mut line = String::from("\"");
    line.push_str(&quote_for_cmd(&program.to_string_lossy()));
    for arg in args {
        line.push(' ');
        line.push_str(&quote_for_cmd(arg));
    }
    line.push('"');
    line
}

#[cfg_attr(not(windows), allow(dead_code))]
fn quote_for_cmd(arg: &str) -> String {
    let needs_quotes = arg.is_empty()
        || arg.chars().any(|c| {
            c.is_whitespace() || matches!(c, '"' | '&' | '|' | '<' | '>' | '^' | '(' | ')')
        });
    if !needs_quotes {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for ch in arg.chars() {
        if ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
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
    cache
        .get_or_init(|| {
            names.iter().find_map(|name| {
                // util::find_program skips empty PATH segments, which would
                // otherwise resolve a bare name against the current directory
                // — i.e. run a binary out of the repo being worked on.
                util::find_program(name).map(|path| Invocation {
                    via_cmd_shell: path.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")
                    }),
                    path,
                })
            })
        })
        .clone()
        .ok_or_else(|| TactusError::Agent {
            message: missing(names),
        })
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
        .map(|t| t.trim_start_matches('v').to_owned())
        .unwrap_or_else(|| first_line.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_shim_quoting_survives_spaces_and_multiple_quoted_args() {
        let line = cmd_c_line(
            Path::new(r"C:\Users\John Smith\npm\claude.cmd"),
            &[
                "-p".to_owned(),
                "--settings".to_owned(),
                r"C:\repo with spaces\settings.json".to_owned(),
                String::new(),
            ],
        );
        assert!(
            line.starts_with('"') && line.ends_with('"'),
            "outer pair: {line}"
        );
        assert!(line.contains(r#""C:\Users\John Smith\npm\claude.cmd""#));
        assert!(line.contains(r#""C:\repo with spaces\settings.json""#));
        assert!(
            line.contains(r#" "" "#) || line.ends_with(r#""""#),
            "empty arg kept: {line}"
        );
        assert_eq!(quote_for_cmd("simple"), "simple");
    }

    #[test]
    fn tool_permission_args_survive_the_cmd_shim() {
        // Copilot's permission surface is argv, and `shell(cargo test)` carries
        // both a space and parentheses — the latter are cmd.exe metacharacters,
        // so an unquoted one would end the command mid-line.
        let line = cmd_c_line(
            Path::new(r"C:\Users\me\npm\copilot.cmd"),
            &[
                "--allow-tool=shell(cargo test)".to_owned(),
                "--deny-tool=write".to_owned(),
            ],
        );
        assert!(
            line.contains(r#""--allow-tool=shell(cargo test)""#),
            "quoted whole: {line}"
        );
        assert_eq!(
            quote_for_cmd("--deny-tool=write"),
            "--deny-tool=write",
            "nothing to escape, so nothing added"
        );
    }

    #[test]
    fn version_extraction_handles_known_formats() {
        assert_eq!(extract_version("2.1.35 (Claude Code)\n"), "2.1.35");
        assert_eq!(extract_version("claude v1.0.128\n"), "1.0.128");
        assert_eq!(extract_version("weird output\n"), "weird output");
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
}
