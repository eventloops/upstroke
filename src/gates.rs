//! Gates (DESIGN.md §11.1): configured shell commands run sequentially in the
//! workspace after every agent attempt, short-circuiting on the first failure.
//! Gates are what make cheap models affordable — objective, free, and they
//! catch most small-model failures before any frontier tokens are spent.
//!
//! Evidence axes owned here: red tests block (a failing test gate fails the
//! attempt), and test provenance for Test tasks — statically in v0.1 (the
//! diff must plausibly add test code; lenient by design, with step-6 review
//! as the backstop); the dynamic fail-on-base/pass-on-HEAD check needs v0.2
//! worktrees to run safely.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::agent::proc;
use crate::error::TactusError;
use crate::util;
use crate::workspace::Workspace;

/// §17 `[engine].shell` — the shell gate commands run under. Default is the
/// platform-native one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Cmd,
    Sh,
    Bash,
    PowerShell,
    Pwsh,
}

impl ShellKind {
    pub fn native() -> Self {
        if cfg!(windows) { Self::Cmd } else { Self::Sh }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cmd" => Some(Self::Cmd),
            "sh" => Some(Self::Sh),
            "bash" => Some(Self::Bash),
            "powershell" => Some(Self::PowerShell),
            "pwsh" => Some(Self::Pwsh),
            _ => None,
        }
    }

    /// PowerShell cmdlets are not PATH programs, so pre-flight resolution of
    /// gate commands cannot be enforced for these shells.
    pub fn resolves_via_path(self) -> bool {
        matches!(self, Self::Cmd | Self::Sh | Self::Bash)
    }

    /// The shell binary itself, verified at pre-flight by [`shell_available`].
    pub fn program(self) -> &'static str {
        match self {
            Self::Cmd => "cmd",
            Self::Sh => "sh",
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
            Self::Pwsh => "pwsh",
        }
    }

    /// Shell builtins that are legal command starters but never PATH files.
    fn builtins(self) -> &'static [&'static str] {
        match self {
            Self::Cmd => &[
                "echo", "cd", "dir", "set", "type", "copy", "del", "ren", "md", "rd", "cls",
                "exit", "call", "start", "ver", "if", "for",
            ],
            Self::Sh | Self::Bash => &[
                "echo", "cd", "test", "[", ".", ":", "exit", "set", "export", "true", "false",
                "if", "for", "while", "command",
            ],
            Self::PowerShell | Self::Pwsh => &[],
        }
    }

    pub fn command(self, cmdline: &str) -> Command {
        match self {
            Self::Cmd => {
                let mut c = Command::new("cmd");
                // std's Windows quoting escapes embedded quotes as \" per
                // CommandLineToArgvW rules, which cmd.exe does not un-escape;
                // the /C tail must go through raw_arg to survive intact.
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    c.arg("/C");
                    c.raw_arg(cmdline);
                }
                #[cfg(not(windows))]
                c.args(["/C", cmdline]);
                c
            }
            Self::Sh => {
                let mut c = Command::new("sh");
                c.args(["-c", cmdline]);
                c
            }
            Self::Bash => {
                let mut c = Command::new("bash");
                c.args(["-c", cmdline]);
                c
            }
            Self::PowerShell | Self::Pwsh => {
                let mut c = Command::new(self.program());
                c.args(["-NoProfile", "-NonInteractive", "-Command", cmdline]);
                c
            }
        }
    }
}

#[derive(Debug)]
pub enum GateResult {
    Pass { log: String },
    Fail { log: String },
}

/// DESIGN.md §8 `Gate` (synchronous until the v0.2 tokio scheduler; the
/// `Result` wrapper distinguishes environment errors — e.g. the shell binary
/// failing to spawn — which abort the run per §19, from gate failures, which
/// fail the attempt).
pub trait Gate {
    fn name(&self) -> &str;
    fn check(&self, ws: &Workspace) -> Result<GateResult, TactusError>;
}

#[derive(Debug, Clone)]
pub struct ShellGate {
    pub name: String,
    pub cmd: String,
    pub timeout: Duration,
    pub shell: ShellKind,
}

impl Gate for ShellGate {
    fn name(&self) -> &str {
        &self.name
    }

    fn check(&self, ws: &Workspace) -> Result<GateResult, TactusError> {
        let mut command = self.shell.command(&self.cmd);
        command.current_dir(ws.root());
        // A spawn failure here is an environment problem (missing shell),
        // not a task failure — propagate per §19.
        let out = proc::run_with_timeout(command, "", self.timeout)?;
        let mut log = String::new();
        if !out.stdout.trim().is_empty() {
            log.push_str(&out.stdout);
        }
        if !out.stderr.trim().is_empty() {
            if !log.is_empty() {
                log.push_str("\n--- stderr ---\n");
            }
            log.push_str(&out.stderr);
        }
        if out.timed_out {
            log.push_str(&format!(
                "\ngate `{}` timed out after {}s",
                self.name,
                self.timeout.as_secs()
            ));
            return Ok(GateResult::Fail { log });
        }
        if out.code == Some(0) {
            Ok(GateResult::Pass { log })
        } else {
            log.push_str(&format!("\nexit code: {:?}", out.code));
            Ok(GateResult::Fail { log })
        }
    }
}

#[derive(Debug)]
pub struct GateFailure {
    pub gate: String,
    /// Short summary for reports (400 bytes); `log_tail` carries the §11.1
    /// feedback payload and the full log is written to the run dir.
    pub summary: String,
    pub log_tail: String,
}

/// §11.1: retry feedback is the output tail, capped at 8 KB.
pub const FEEDBACK_TAIL_BYTES: usize = 8 * 1024;

/// Run gates sequentially, short-circuiting on the first failure. Every gate
/// with output gets its log written to `log_dir/<task>-<attempt>-<gate>.log`
/// (pass and fail — the pass logs are the evidence trail for committed
/// tasks). Returns `Ok(Some(failure))` for a gate failure (attempt fails),
/// `Err` for environment problems (run aborts, §19).
pub fn run_all(
    gates: &[ShellGate],
    ws: &Workspace,
    log_dir: &Path,
    task: &str,
    attempt: u32,
) -> Result<Option<GateFailure>, TactusError> {
    for gate in gates {
        let result = gate.check(ws)?;
        let (log, passed) = match result {
            GateResult::Pass { log } => (log, true),
            GateResult::Fail { log } => (log, false),
        };
        let mut write_note = String::new();
        if !log.trim().is_empty() {
            let file_name = format!(
                "{}-{attempt}-{}.log",
                util::filename_component(task),
                util::filename_component(&gate.name)
            );
            if let Err(e) = fs::write(log_dir.join(&file_name), &log) {
                write_note = format!("(log write failed: {e}) ");
            }
        }
        if !passed {
            return Ok(Some(GateFailure {
                gate: gate.name.clone(),
                summary: format!("{write_note}{}", util::tail(&log, 400)),
                log_tail: util::tail(&log, FEEDBACK_TAIL_BYTES),
            }));
        }
    }
    Ok(None)
}

/// §14 pre-flight: the configured shell itself must exist before any agent
/// tokens are spent.
pub fn shell_available(shell: ShellKind) -> Result<(), TactusError> {
    if find_program(shell.program()).is_some() {
        Ok(())
    } else {
        Err(TactusError::Gate {
            message: format!(
                "configured shell `{}` not found on PATH; fix [engine].shell or install it",
                shell.program()
            ),
        })
    }
}

enum Resolution {
    Ok,
    /// Quotes, operators, or env-var prefixes: the shell decides — pre-flight
    /// cannot judge these without re-implementing the shell.
    SkippedComplex,
    Missing(String),
}

/// §14 pre-flight: every gate command resolves. Hard error for path-resolving
/// shells; PowerShell cmdlets downgrade to a warning.
pub fn resolve_programs(
    gates: &[ShellGate],
    workspace_root: &Path,
    warnings: &mut Vec<String>,
) -> Result<(), TactusError> {
    for gate in gates {
        match resolution(gate, workspace_root)? {
            Resolution::Ok | Resolution::SkippedComplex => {}
            Resolution::Missing(program) => {
                if gate.shell.resolves_via_path() {
                    return Err(TactusError::Gate {
                        message: format!(
                            "gate `{}` command `{program}` not found on PATH; fix the gate or \
                             install the tool",
                            gate.name
                        ),
                    });
                }
                warnings.push(format!(
                    "gate `{}` command `{program}` not found on PATH (may be a {} cmdlet)",
                    gate.name,
                    gate.shell.program()
                ));
            }
        }
    }
    Ok(())
}

/// `validate`/dry-run variant: same checks, warnings only, never refuses.
pub fn preview_resolution(gates: &[ShellGate], workspace_root: &Path, warnings: &mut Vec<String>) {
    for gate in gates {
        if let Ok(Resolution::Missing(program)) = resolution(gate, workspace_root) {
            warnings.push(format!(
                "gate `{}` command `{program}` not found on PATH — `tactus run` will refuse it",
                gate.name
            ));
        }
    }
}

fn resolution(gate: &ShellGate, workspace_root: &Path) -> Result<Resolution, TactusError> {
    let cmd = gate.cmd.trim();
    let Some(first) = cmd.split_whitespace().next() else {
        return Err(TactusError::Gate {
            message: format!("gate `{}` has an empty command", gate.name),
        });
    };
    // Shell syntax pre-flight cannot judge: quoting, operators, env prefixes.
    if cmd
        .chars()
        .any(|c| matches!(c, '"' | '\'' | '|' | '&' | '>' | '<' | ';'))
        || first.contains('=')
    {
        return Ok(Resolution::SkippedComplex);
    }
    let is_builtin = gate
        .shell
        .builtins()
        .iter()
        .any(|b| first.eq_ignore_ascii_case(b));
    if is_builtin {
        return Ok(Resolution::Ok);
    }
    let candidate = Path::new(first);
    let found = if candidate.is_absolute() {
        probe_extensions(candidate)
    } else if first.contains('/') || first.contains('\\') {
        // Relative to the workspace, where the gate actually runs.
        probe_extensions(&workspace_root.join(candidate))
    } else {
        find_program(first)
    };
    Ok(match found {
        Some(_) => Resolution::Ok,
        None => Resolution::Missing(first.to_owned()),
    })
}

fn executable_extensions() -> Vec<String> {
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

fn probe_extensions(base: &Path) -> Option<PathBuf> {
    for ext in executable_extensions() {
        let candidate = if ext.is_empty() {
            base.to_path_buf()
        } else {
            PathBuf::from(format!("{}{ext}", base.display()))
        };
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_program(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.is_absolute() {
        return probe_extensions(candidate);
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        if let Some(found) = probe_extensions(&dir.join(name)) {
            return Some(found);
        }
    }
    None
}

/// Derived default gates when `[[gates]]` is absent (§17: a fresh repo runs
/// with zero config): recognized project markers map to the obvious
/// compile+test commands. Unknown project shapes derive no gates.
pub fn derive(root: &Path, shell: ShellKind) -> Vec<ShellGate> {
    let gate = |name: &str, cmd: &str, secs: u64| ShellGate {
        name: name.to_owned(),
        cmd: cmd.to_owned(),
        timeout: Duration::from_secs(secs),
        shell,
    };
    if root.join("Cargo.toml").is_file() {
        return vec![
            gate("check", "cargo check --all-targets", 600),
            gate("test", "cargo test", 1200),
        ];
    }
    if root.join("go.mod").is_file() {
        return vec![
            gate("build", "go build ./...", 600),
            gate("test", "go test ./...", 1200),
        ];
    }
    if let Ok(text) = fs::read_to_string(root.join("package.json"))
        && let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(script) = pkg
            .get("scripts")
            .and_then(|s| s.get("test"))
            .and_then(|t| t.as_str())
        // npm init's placeholder always exits 1 — deriving it would make
        // every zero-config run fail.
        && !script.contains("no test specified")
    {
        return vec![gate("test", "npm test", 1200)];
    }
    Vec::new()
}

/// Static test-provenance check (§11.1, v0.1 form): a Test task's diff must
/// plausibly add test code. Signals, any of which passes: a test-declaration
/// marker at an identifier boundary on an added line, an added line in a
/// test-looking file, or an added assertion. Deliberately lenient — false
/// passes are caught by review (step 6); false failures would roll back
/// legitimate work.
pub fn diff_adds_tests(diff: &str) -> bool {
    let mut in_test_file = false;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.trim().trim_start_matches("b/");
            in_test_file = path != "/dev/null" && is_testish_path(path);
            continue;
        }
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        let added = line[1..].trim();
        if added.is_empty() {
            continue;
        }
        if in_test_file || added.contains("assert") || has_test_marker(added) {
            return true;
        }
    }
    false
}

const MARKERS: &[&str] = &[
    "#[test]",
    "#[tokio::test]",
    "fn test_",
    "def test_",
    "@Test",
    "[Test]",
    "[TestMethod]",
    "func Test",
    "it(",
    "test(",
    "describe(",
];

/// Marker match anchored at an identifier boundary: `exit(` must not match
/// `it(`, and `regex.test(` must not match `test(`.
fn has_test_marker(line: &str) -> bool {
    for marker in MARKERS {
        for (index, _) in line.match_indices(marker) {
            let boundary = match line[..index].chars().next_back() {
                None => true,
                Some(prev) => !(prev.is_alphanumeric() || matches!(prev, '_' | '.' | '$')),
            };
            if boundary {
                return true;
            }
        }
    }
    false
}

fn is_testish_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    let file = lower.rsplit('/').next().unwrap_or_default();
    lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.starts_with("tests/")
        || lower.starts_with("test/")
        || lower.contains("__tests__")
        || file.starts_with("test_")
        || file.contains("_test.")
        || file.contains(".test.")
        || file.contains(".spec.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::process::Command as StdCommand;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tactus-gates-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = temp_dir(tag);
        let out = StdCommand::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-q"])
            .output()
            .expect("git");
        assert!(out.status.success());
        dir
    }

    fn gate(cmd: &str, secs: u64) -> ShellGate {
        ShellGate {
            name: "g".to_owned(),
            cmd: cmd.to_owned(),
            timeout: Duration::from_secs(secs),
            shell: ShellKind::native(),
        }
    }

    #[test]
    fn passing_and_failing_gates() {
        let repo = temp_repo("passfail");
        let ws = Workspace::open(&repo).expect("open");
        assert!(matches!(
            gate("git --version", 30).check(&ws),
            Ok(GateResult::Pass { .. })
        ));

        let Ok(GateResult::Fail { log }) = gate("git frobnicate-not-a-command", 30).check(&ws)
        else {
            panic!("bogus git subcommand must fail");
        };
        assert!(log.contains("frobnicate"), "log carries output: {log}");
    }

    #[test]
    fn gate_timeout_fails_with_note() {
        let repo = temp_repo("timeout");
        let ws = Workspace::open(&repo).expect("open");
        let cmd = if cfg!(windows) {
            "ping -n 30 127.0.0.1 > NUL"
        } else {
            "sleep 30"
        };
        let mut g = gate(cmd, 30);
        g.timeout = Duration::from_millis(300);
        let Ok(GateResult::Fail { log }) = g.check(&ws) else {
            panic!("must time out");
        };
        assert!(log.contains("timed out"), "log: {log}");
    }

    #[test]
    fn quoted_arguments_survive_the_windows_shell() {
        let repo = temp_repo("quoting");
        let ws = Workspace::open(&repo).expect("open");
        // `git config` with a quoted value round-trips only if the shell
        // preserved the quote grouping.
        let set = gate("git config --local test.quoted \"two words\"", 30);
        assert!(matches!(set.check(&ws), Ok(GateResult::Pass { .. })));
        let get = StdCommand::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "--local", "test.quoted"])
            .output()
            .expect("read back");
        assert_eq!(
            String::from_utf8_lossy(&get.stdout).trim(),
            "two words",
            "quote grouping preserved through the shell"
        );
    }

    #[test]
    fn run_all_short_circuits_and_writes_logs_for_all_run_gates() {
        let repo = temp_repo("shortcircuit");
        let ws = Workspace::open(&repo).expect("open");
        let logs = temp_dir("shortcircuit-logs");
        let gates = vec![
            ShellGate {
                name: "ok".to_owned(),
                cmd: "git --version".to_owned(),
                timeout: Duration::from_secs(30),
                shell: ShellKind::native(),
            },
            ShellGate {
                name: "bad".to_owned(),
                cmd: "git frobnicate".to_owned(),
                timeout: Duration::from_secs(30),
                shell: ShellKind::native(),
            },
            gate("git --version", 30),
        ];
        let failure = run_all(&gates, &ws, &logs, "t1", 1)
            .expect("no environment error")
            .expect("second gate fails");
        assert_eq!(failure.gate, "bad");
        assert!(!failure.summary.is_empty());
        assert!(!failure.log_tail.is_empty());
        assert!(logs.join("t1-1-ok.log").is_file(), "passing gate log kept");
        assert!(logs.join("t1-1-bad.log").is_file(), "failing gate log kept");
    }

    #[test]
    fn hostile_gate_and_task_names_still_get_logs() {
        let repo = temp_repo("hostile");
        let ws = Workspace::open(&repo).expect("open");
        let logs = temp_dir("hostile-logs");
        let gates = vec![ShellGate {
            name: "lint:fast/unit".to_owned(),
            cmd: "git frobnicate".to_owned(),
            timeout: Duration::from_secs(30),
            shell: ShellKind::native(),
        }];
        let failure = run_all(&gates, &ws, &logs, "a/b", 1)
            .expect("no environment error")
            .expect("gate fails");
        assert_eq!(failure.gate, "lint:fast/unit", "report keeps the real name");
        assert!(
            logs.join("a-b-1-lint-fast-unit.log").is_file(),
            "sanitized log written"
        );
    }

    #[test]
    fn resolution_enforces_simple_commands_and_skips_shelly_ones() {
        let mut warnings = Vec::new();
        let root = temp_dir("resolve-root");
        resolve_programs(&[gate("git --version", 30)], &root, &mut warnings).expect("git resolves");

        let err = resolve_programs(
            &[gate("definitely-not-a-real-tool-xyz --ok", 30)],
            &root,
            &mut warnings,
        )
        .expect_err("must refuse");
        assert!(err.to_string().contains("not found on PATH"));

        // Shell-complex commands are the shell's business, not pre-flight's.
        for complex in [
            "cd ui && npm test",
            "RUSTFLAGS=-Dwarnings cargo check",
            "\"C:\\Program Files\\tool\\check.exe\" --fast",
            "echo residue> residue.txt",
        ] {
            resolve_programs(&[gate(complex, 30)], &root, &mut warnings)
                .unwrap_or_else(|e| panic!("`{complex}` should be skipped, got {e}"));
        }

        // Builtins are legal starters.
        resolve_programs(&[gate("echo hello", 30)], &root, &mut warnings).expect("builtin ok");

        // Workspace-relative scripts resolve against the workspace root.
        let script_rel = if cfg!(windows) {
            "scripts\\check.bat"
        } else {
            "scripts/check.sh"
        };
        fs::create_dir_all(root.join("scripts")).expect("scripts dir");
        fs::write(
            root.join("scripts").join(if cfg!(windows) {
                "check.bat"
            } else {
                "check.sh"
            }),
            "",
        )
        .expect("script");
        resolve_programs(&[gate(script_rel, 30)], &root, &mut warnings)
            .expect("relative script resolves against workspace");

        // PowerShell cmdlets downgrade to a warning.
        let ps = ShellGate {
            name: "psgate".to_owned(),
            cmd: "Get-ChildItem".to_owned(),
            timeout: Duration::from_secs(30),
            shell: ShellKind::PowerShell,
        };
        resolve_programs(&[ps], &root, &mut warnings).expect("cmdlet tolerated");
        assert!(warnings.iter().any(|w| w.contains("psgate")));
    }

    #[test]
    fn preview_resolution_warns_instead_of_refusing() {
        let mut warnings = Vec::new();
        let root = temp_dir("preview-root");
        preview_resolution(
            &[gate("definitely-not-a-real-tool-xyz build", 30)],
            &root,
            &mut warnings,
        );
        assert!(
            warnings.iter().any(|w| w.contains("will refuse")),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn native_shell_is_available() {
        shell_available(ShellKind::native()).expect("native shell exists");
    }

    #[test]
    fn derive_recognizes_project_markers() {
        let rust = temp_dir("derive-rust");
        fs::write(rust.join("Cargo.toml"), "[package]\nname='x'\n").expect("cargo");
        let gates = derive(&rust, ShellKind::native());
        assert_eq!(gates.len(), 2);
        assert!(gates[0].cmd.contains("cargo check"));
        assert!(gates[1].cmd.contains("cargo test"));

        let node = temp_dir("derive-node");
        fs::write(
            node.join("package.json"),
            r#"{"scripts":{"test":"vitest"}}"#,
        )
        .expect("pkg");
        let gates = derive(&node, ShellKind::native());
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].cmd, "npm test");

        // npm init's always-failing placeholder must not become a gate.
        let node_placeholder = temp_dir("derive-node-placeholder");
        fs::write(
            node_placeholder.join("package.json"),
            r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
        )
        .expect("pkg");
        assert!(derive(&node_placeholder, ShellKind::native()).is_empty());

        let empty = temp_dir("derive-none");
        assert!(derive(&empty, ShellKind::native()).is_empty());
    }

    #[test]
    fn provenance_markers_respect_identifier_boundaries() {
        assert!(diff_adds_tests(
            "+++ b/src/lib.rs\n+#[test]\n+fn works() {}\n"
        ));
        assert!(diff_adds_tests("+def test_cursor_roundtrip():\n"));
        assert!(diff_adds_tests("+it(\"renders\", () => {})\n"));

        // Identifier and method-call lookalikes must not count.
        assert!(!diff_adds_tests("+    process.exit(1);\n"));
        assert!(!diff_adds_tests("+    let parts = s.split(',');\n"));
        assert!(!diff_adds_tests("+    if (regex.test(input)) {}\n"));
        assert!(!diff_adds_tests("+    tx.commit();\n"));
        assert!(!diff_adds_tests("+just some added prose\n"));
        assert!(!diff_adds_tests("-#[test]\n-fn was_here() {}\n"));
    }

    #[test]
    fn provenance_accepts_test_files_and_assertions() {
        // Strengthening an existing test: no declaration marker, but an
        // assertion counts.
        assert!(diff_adds_tests(
            "+++ b/src/lib.rs\n+        assert_eq!(total, 42);\n"
        ));
        // Any real addition inside a test-looking file counts.
        assert!(diff_adds_tests("+++ b/tests/api.rs\n+        helper(1);\n"));
        assert!(diff_adds_tests(
            "+++ b/web/src/foo.spec.ts\n+        helper(1);\n"
        ));
        // Deleted-file headers must not mark what follows as test content.
        assert!(!diff_adds_tests("+++ /dev/null\n+ignored\n"));
    }

    #[test]
    fn shell_kind_parsing() {
        assert_eq!(ShellKind::parse("PowerShell"), Some(ShellKind::PowerShell));
        assert_eq!(ShellKind::parse("cmd"), Some(ShellKind::Cmd));
        assert_eq!(ShellKind::parse("bash"), Some(ShellKind::Bash));
        assert_eq!(ShellKind::parse("fish"), None);
        assert_eq!(ShellKind::Bash.program(), "bash");
    }
}
