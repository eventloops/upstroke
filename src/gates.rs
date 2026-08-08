//! Gates (DESIGN.md §11.1): configured shell commands run sequentially in the
//! workspace after every agent attempt, short-circuiting on the first failure.
//! Gates are what make cheap models affordable — objective, free, and they
//! catch most small-model failures before any frontier tokens are spent.
//!
//! Evidence axes owned here: red tests block (a failing test gate fails the
//! attempt), and test provenance for Test tasks — statically in v0.1 (the
//! diff must actually add test code); the dynamic fail-on-base/pass-on-HEAD
//! check needs v0.2 worktrees to run safely.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::agent::proc;
use crate::error::TactusError;
use crate::ir::TaskId;
use crate::workspace::Workspace;

/// §17 `[engine].shell` — the shell gate commands run under. Default is the
/// platform-native one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Cmd,
    Sh,
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
            "sh" | "bash" => Some(Self::Sh),
            "powershell" => Some(Self::PowerShell),
            "pwsh" => Some(Self::Pwsh),
            _ => None,
        }
    }

    /// PowerShell cmdlets are not PATH programs, so pre-flight resolution
    /// cannot be enforced for these shells.
    pub fn resolves_via_path(self) -> bool {
        matches!(self, Self::Cmd | Self::Sh)
    }

    pub fn command(self, cmdline: &str) -> Command {
        match self {
            Self::Cmd => {
                let mut c = Command::new("cmd");
                c.args(["/C", cmdline]);
                c
            }
            Self::Sh => {
                let mut c = Command::new("sh");
                c.args(["-c", cmdline]);
                c
            }
            Self::PowerShell | Self::Pwsh => {
                let program = if self == Self::Pwsh {
                    "pwsh"
                } else {
                    "powershell"
                };
                let mut c = Command::new(program);
                c.args(["-NoProfile", "-NonInteractive", "-Command", cmdline]);
                c
            }
        }
    }
}

#[derive(Debug)]
pub enum GateResult {
    Pass,
    Fail { log: String },
}

/// DESIGN.md §8 `Gate` (synchronous until the v0.2 tokio scheduler).
pub trait Gate {
    fn name(&self) -> &str;
    fn check(&self, ws: &Workspace) -> GateResult;
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

    fn check(&self, ws: &Workspace) -> GateResult {
        let mut command = self.shell.command(&self.cmd);
        command.current_dir(ws.root());
        match proc::run_with_timeout(command, "", self.timeout) {
            Err(e) => GateResult::Fail {
                log: format!("failed to run gate `{}`: {e}", self.name),
            },
            Ok(out) => {
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
                    return GateResult::Fail { log };
                }
                if out.code == Some(0) {
                    GateResult::Pass
                } else {
                    log.push_str(&format!("\nexit code: {:?}", out.code));
                    GateResult::Fail { log }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct GateFailure {
    pub gate: String,
    /// Short summary for reports; the full log lives in the run dir.
    pub summary: String,
}

/// §11.1: feedback is the output tail, capped at 8 KB.
pub const FEEDBACK_TAIL_BYTES: usize = 8 * 1024;

/// Run gates sequentially, short-circuiting on the first failure. Every gate
/// that runs writes its full log to `log_dir/<task>-<attempt>-<gate>.log`.
pub fn run_all(
    gates: &[ShellGate],
    ws: &Workspace,
    log_dir: &Path,
    task: &TaskId,
    attempt: u32,
) -> Result<(), GateFailure> {
    for gate in gates {
        let result = gate.check(ws);
        let (log, failed) = match &result {
            GateResult::Pass => (String::new(), false),
            GateResult::Fail { log } => (log.clone(), true),
        };
        if failed {
            let log_path = log_dir.join(format!("{task}-{attempt}-{}.log", gate.name));
            let _ = fs::write(&log_path, &log);
            return Err(GateFailure {
                gate: gate.name.clone(),
                summary: tail(&log, 400),
            });
        }
    }
    Ok(())
}

fn tail(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max {
        return trimmed.to_owned();
    }
    let start = trimmed.len() - max;
    let start = (start..trimmed.len())
        .find(|i| trimmed.is_char_boundary(*i))
        .unwrap_or(start);
    format!("…{}", &trimmed[start..])
}

/// §14 pre-flight: every gate command resolves. The first token must be a
/// program on PATH for path-resolving shells; PowerShell cmdlets downgrade to
/// a warning.
pub fn resolve_programs(
    gates: &[ShellGate],
    warnings: &mut Vec<String>,
) -> Result<(), TactusError> {
    for gate in gates {
        let Some(program) = gate.cmd.split_whitespace().next() else {
            return Err(TactusError::Gate {
                message: format!("gate `{}` has an empty command", gate.name),
            });
        };
        if find_program(program).is_some() {
            continue;
        }
        if gate.shell.resolves_via_path() {
            return Err(TactusError::Gate {
                message: format!(
                    "gate `{}` command `{program}` not found on PATH; fix the gate or install \
                     the tool",
                    gate.name
                ),
            });
        }
        warnings.push(format!(
            "gate `{}` command `{program}` not found on PATH (may be a {} cmdlet)",
            gate.name,
            if gate.shell == ShellKind::Pwsh {
                "pwsh"
            } else {
                "PowerShell"
            }
        ));
    }
    Ok(())
}

fn find_program(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.is_absolute() {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(&path_var) {
        for ext in extensions {
            let full = dir.join(format!("{name}{ext}"));
            if full.is_file() {
                return Some(full);
            }
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
        && pkg.get("scripts").and_then(|s| s.get("test")).is_some()
    {
        return vec![gate("test", "npm test", 1200)];
    }
    Vec::new()
}

/// Static test-provenance check (§11.1, v0.1 form): a Test task's diff must
/// actually add test code. Scans added lines for test markers across the
/// common ecosystems.
pub fn diff_adds_tests(diff: &str) -> bool {
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
    diff.lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .any(|l| MARKERS.iter().any(|m| l.contains(m)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tactus-gates-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = temp_dir(tag);
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success());
        };
        run(&["init", "-q"]);
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
            GateResult::Pass
        ));

        let GateResult::Fail { log } = gate("git frobnicate-not-a-command", 30).check(&ws) else {
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
        let GateResult::Fail { log } = g.check(&ws) else {
            panic!("must time out");
        };
        assert!(log.contains("timed out"), "log: {log}");
    }

    #[test]
    fn run_all_short_circuits_and_writes_log() {
        let repo = temp_repo("shortcircuit");
        let ws = Workspace::open(&repo).expect("open");
        let logs = temp_dir("shortcircuit-logs");
        let gates = vec![
            ShellGate {
                name: "bad".to_owned(),
                cmd: "git frobnicate".to_owned(),
                timeout: Duration::from_secs(30),
                shell: ShellKind::native(),
            },
            gate("git --version", 30),
        ];
        let err = run_all(&gates, &ws, &logs, &TaskId::from("t1"), 1).expect_err("first fails");
        assert_eq!(err.gate, "bad");
        assert!(!err.summary.is_empty());
        assert!(logs.join("t1-1-bad.log").is_file(), "full log written");
    }

    #[test]
    fn resolution_checks_first_token() {
        let mut warnings = Vec::new();
        resolve_programs(&[gate("git --version", 30)], &mut warnings).expect("git resolves");
        assert!(warnings.is_empty());

        let err = resolve_programs(
            &[gate("definitely-not-a-real-tool-xyz --ok", 30)],
            &mut warnings,
        )
        .expect_err("must refuse");
        assert!(err.to_string().contains("not found on PATH"));

        // PowerShell cmdlets downgrade to a warning.
        let ps = ShellGate {
            name: "psgate".to_owned(),
            cmd: "Get-ChildItem".to_owned(),
            timeout: Duration::from_secs(30),
            shell: ShellKind::PowerShell,
        };
        resolve_programs(&[ps], &mut warnings).expect("cmdlet tolerated");
        assert!(warnings.iter().any(|w| w.contains("psgate")));
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

        let node_no_test = temp_dir("derive-node-quiet");
        fs::write(node_no_test.join("package.json"), r#"{"name":"x"}"#).expect("pkg");
        assert!(derive(&node_no_test, ShellKind::native()).is_empty());

        let empty = temp_dir("derive-none");
        assert!(derive(&empty, ShellKind::native()).is_empty());
    }

    #[test]
    fn provenance_marker_scan() {
        let rust_diff = "+++ b/src/lib.rs\n+#[test]\n+fn works() {}\n";
        assert!(diff_adds_tests(rust_diff));
        let py_diff = "+def test_cursor_roundtrip():\n+    assert True\n";
        assert!(diff_adds_tests(py_diff));
        let prose = "+just some added prose\n+more prose\n";
        assert!(!diff_adds_tests(prose));
        // Removed test code does not count as adding tests.
        let removal = "-#[test]\n-fn was_here() {}\n";
        assert!(!diff_adds_tests(removal));
    }

    #[test]
    fn shell_kind_parsing() {
        assert_eq!(ShellKind::parse("PowerShell"), Some(ShellKind::PowerShell));
        assert_eq!(ShellKind::parse("cmd"), Some(ShellKind::Cmd));
        assert_eq!(ShellKind::parse("fish"), None);
    }
}
