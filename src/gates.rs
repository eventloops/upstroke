//! Extended notes: `docs/internals/gates.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::UpstrokeError;
use crate::runner::invocation::InvocationId;
use crate::runner::{CommandSpec, Runner};
use crate::util;
use crate::workspace::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

    pub fn resolves_via_path(self) -> bool {
        matches!(self, Self::Cmd | Self::Sh | Self::Bash)
    }

    pub fn program(self) -> &'static str {
        match self {
            Self::Cmd => "cmd",
            Self::Sh => "sh",
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
            Self::Pwsh => "pwsh",
        }
    }

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

    #[must_use]
    pub fn spec(self, cmdline: &str) -> CommandSpec {
        let (program, args): (&str, Vec<&str>) = match self {
            Self::Cmd => ("cmd", vec!["/C", cmdline]),
            Self::Sh => ("sh", vec!["-c", cmdline]),
            Self::Bash => ("bash", vec!["-c", cmdline]),
            Self::PowerShell | Self::Pwsh => (
                self.program(),
                vec!["-NoProfile", "-NonInteractive", "-Command", cmdline],
            ),
        };
        CommandSpec {
            program: program.to_owned(),
            args: args.into_iter().map(str::to_owned).collect(),
            env: Vec::new(),
            stdin: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum GateResult {
    Pass { log: String },
    Fail { log: String },
}

pub trait Gate {
    fn name(&self) -> &str;
    fn check(
        &self,
        runner: &dyn Runner,
        invocation: InvocationId,
        ws: &Workspace,
    ) -> Result<GateResult, UpstrokeError>;
}

#[derive(Debug, Clone)]
pub struct ShellGate {
    pub name: String,
    pub cmd: String,
    pub timeout: Duration,
    pub shell: ShellKind,
}

impl ShellGate {
    pub fn from_record(record: &crate::events::GateSummary) -> Self {
        Self {
            name: record.name.clone(),
            cmd: record.cmd.clone(),
            timeout: record.timeout,
            shell: record.shell,
        }
    }
}

impl ShellGate {
    #[must_use]
    pub fn command(&self) -> (CommandSpec, Duration) {
        (self.shell.spec(&self.cmd), self.timeout)
    }
}

impl Gate for ShellGate {
    fn name(&self) -> &str {
        &self.name
    }

    fn check(
        &self,
        runner: &dyn Runner,
        invocation: InvocationId,
        ws: &Workspace,
    ) -> Result<GateResult, UpstrokeError> {
        let (command, timeout) = self.command();
        let out = runner.run(&crate::runner::gate_request(
            command,
            ws.root().to_path_buf(),
            timeout,
            invocation,
        ))?;
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
        if out.output_limited {
            log.push_str(&format!(
                "\ngate `{}` exceeded the stdout/stderr output limit",
                self.name
            ));
            return Ok(GateResult::Fail { log });
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
    pub summary: String,
    pub log_tail: String,
}

pub const FEEDBACK_TAIL_BYTES: usize = 8 * 1024;

pub fn run_all(
    gates: &[ShellGate],
    runner: &dyn Runner,
    invocation: &dyn Fn(u32) -> InvocationId,
    ws: &Workspace,
    log_dir: &Path,
    stem: &str,
    attempt: u32,
) -> Result<Option<GateFailure>, UpstrokeError> {
    for (index, gate) in gates.iter().enumerate() {
        let result = gate.check(
            runner,
            invocation(u32::try_from(index).unwrap_or(u32::MAX)),
            ws,
        )?;
        let (log, passed) = match result {
            GateResult::Pass { log } => (log, true),
            GateResult::Fail { log } => (log, false),
        };
        let mut write_note = String::new();
        if !log.trim().is_empty() {
            let file_name = format!(
                "{}-{attempt}-{}.log",
                util::filename_component(stem),
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

pub fn shell_available(shell: ShellKind) -> Result<(), UpstrokeError> {
    if find_program(shell.program()).is_some() {
        Ok(())
    } else {
        Err(UpstrokeError::Gate {
            message: format!(
                "configured shell `{}` not found on PATH; fix [engine].shell or install it",
                shell.program()
            ),
        })
    }
}

enum Resolution {
    Ok,
    SkippedComplex,
    Missing(String),
}

pub fn resolve_programs(
    gates: &[ShellGate],
    workspace_root: &Path,
    warnings: &mut Vec<String>,
) -> Result<(), UpstrokeError> {
    for gate in gates {
        match resolution(gate, workspace_root)? {
            Resolution::Ok | Resolution::SkippedComplex => {}
            Resolution::Missing(program) => {
                if gate.shell.resolves_via_path() {
                    return Err(UpstrokeError::Gate {
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

pub fn preview_resolution(gates: &[ShellGate], workspace_root: &Path, warnings: &mut Vec<String>) {
    for gate in gates {
        if let Ok(Resolution::Missing(program)) = resolution(gate, workspace_root) {
            warnings.push(format!(
                "gate `{}` command `{program}` not found on PATH — `upstroke run` will refuse it",
                gate.name
            ));
        }
    }
}

fn resolution(gate: &ShellGate, workspace_root: &Path) -> Result<Resolution, UpstrokeError> {
    let cmd = gate.cmd.trim();
    let Some(first) = cmd.split_whitespace().next() else {
        return Err(UpstrokeError::Gate {
            message: format!("gate `{}` has an empty command", gate.name),
        });
    };
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
    if let Ok(text) = fs::read_to_string(root.join("package.json")) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(script) = pkg
                .get("scripts")
                .and_then(|s| s.get("test"))
                .and_then(|t| t.as_str())
            {
                if !script.contains("no test specified") {
                    return vec![gate("test", "npm test", 1200)];
                }
            }
        }
    }
    Vec::new()
}

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
mod test_support {
    use super::ShellKind;

    impl ShellKind {
        pub(crate) fn command(self, cmdline: &str) -> std::process::Command {
            crate::runner::host::test_support::build_command(&self.spec(cmdline))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::process::Command as StdCommand;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("upstroke-gates-{tag}-{}", std::process::id()));
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

    fn host() -> crate::runner::host::HostRunner {
        crate::runner::host::HostRunner::new()
    }

    fn gate_id(n: u32) -> InvocationId {
        use crate::runner::invocation::AttemptRole;
        use crate::topology::events::AttemptNumber;
        use crate::topology::registry::TaskKey;
        InvocationId::legacy_attempt(TaskKey(0), AttemptNumber(1), AttemptRole::Gate(n), 0)
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
    fn every_shell_spells_its_invocation_the_way_the_record_says() {
        const LINE: &str = "cargo test --all";
        let expected: Vec<(ShellKind, &str, Vec<&str>)> = vec![
            (ShellKind::Cmd, "cmd", vec!["/C", LINE]),
            (ShellKind::Sh, "sh", vec!["-c", LINE]),
            (ShellKind::Bash, "bash", vec!["-c", LINE]),
            (
                ShellKind::PowerShell,
                "powershell",
                vec!["-NoProfile", "-NonInteractive", "-Command", LINE],
            ),
            (
                ShellKind::Pwsh,
                "pwsh",
                vec!["-NoProfile", "-NonInteractive", "-Command", LINE],
            ),
        ];
        assert_eq!(expected.len(), 5);

        for (shell, program, args) in &expected {
            let spec = shell.spec(LINE);
            assert_eq!(&spec.program, program, "{shell:?}");
            assert_eq!(spec.args, *args, "{shell:?}");
            assert!(
                spec.env.is_empty() && spec.stdin.is_empty(),
                "a gate carries no overlay and no stdin: {shell:?}"
            );
            let built = shell.command(LINE);
            assert_eq!(built.get_program().to_string_lossy(), *program, "{shell:?}");
            let seen: Vec<String> = built
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            assert_eq!(seen, *args, "{shell:?}");
        }

        let programs: std::collections::BTreeSet<String> =
            expected.iter().map(|(_, p, _)| (*p).to_owned()).collect();
        assert_eq!(programs.len(), 5, "one program name per shell");
        let shapes: std::collections::BTreeSet<Vec<&str>> = expected
            .iter()
            .map(|(_, _, a)| a.iter().take(a.len() - 1).copied().collect())
            .collect();
        assert_eq!(shapes.len(), 3, "/C, -c, and the PowerShell flag set");
    }

    #[test]
    fn passing_and_failing_gates() {
        let repo = temp_repo("passfail");
        let ws = Workspace::open(&repo).expect("open");
        assert!(matches!(
            gate("git --version", 30).check(&host(), gate_id(0), &ws),
            Ok(GateResult::Pass { .. })
        ));

        let Ok(GateResult::Fail { log }) =
            gate("git frobnicate-not-a-command", 30).check(&host(), gate_id(1), &ws)
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
        let Ok(GateResult::Fail { log }) = g.check(&host(), gate_id(0), &ws) else {
            panic!("must time out");
        };
        assert!(log.contains("timed out"), "log: {log}");
    }

    struct ScriptedRunner {
        answer: Scripted,
        seen: std::sync::Mutex<Vec<crate::runner::RunnerRequest>>,
    }

    enum Scripted {
        Output(Box<crate::agent::ProcessOutput>),
        SpawnFailure,
    }

    impl ScriptedRunner {
        fn new(answer: Scripted) -> Self {
            Self {
                answer,
                seen: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<crate::runner::RunnerRequest> {
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl Runner for ScriptedRunner {
        fn run(
            &self,
            request: &crate::runner::RunnerRequest,
        ) -> Result<crate::agent::ProcessOutput, UpstrokeError> {
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            match &self.answer {
                Scripted::Output(out) => Ok((**out).clone()),
                Scripted::SpawnFailure => Err(UpstrokeError::Agent {
                    message: "failed to spawn `sh`: No such file or directory (os error 2)"
                        .to_owned(),
                }),
            }
        }
    }

    fn supervised(
        code: Option<i32>,
        timed_out: bool,
        output_limited: bool,
    ) -> crate::agent::ProcessOutput {
        crate::agent::ProcessOutput {
            code,
            stdout: "some output\n".to_owned(),
            stderr: "some diagnostics\n".to_owned(),
            duration: Duration::from_millis(7),
            timed_out,
            output_limited,
        }
    }

    #[test]
    fn a_shell_gate_maps_every_supervision_result_the_way_the_contract_says() {
        const GRID: &[(Option<i32>, bool, bool, bool, &str)] = &[
            (Some(0), false, false, true, ""),
            (Some(1), false, false, false, "exit code"),
            (None, false, false, false, "exit code"),
            (Some(0), true, false, false, "timed out"),
            (Some(1), true, false, false, "timed out"),
            (None, true, false, false, "timed out"),
            (Some(0), false, true, false, "output limit"),
            (Some(1), false, true, false, "output limit"),
            (None, false, true, false, "output limit"),
            (Some(0), true, true, false, "output limit"),
            (Some(1), true, true, false, "output limit"),
            (None, true, true, false, "output limit"),
        ];

        let repo = temp_repo("supervision-grid");
        let ws = Workspace::open(&repo).expect("open");
        let mut passes = 0_usize;
        let mut fails = 0_usize;
        for (code, timed_out, output_limited, expect_pass, must_name) in GRID {
            let runner = ScriptedRunner::new(Scripted::Output(Box::new(supervised(
                *code,
                *timed_out,
                *output_limited,
            ))));
            let cell = format!("code={code:?} timed_out={timed_out} limited={output_limited}");
            let result = gate("does-not-matter", 30)
                .check(&runner, gate_id(0), &ws)
                .unwrap_or_else(|error| {
                    panic!("{cell}: a supervised result is a verdict: {error}")
                });
            match (result, expect_pass) {
                (GateResult::Pass { log }, true) => {
                    assert!(
                        log.contains("some output"),
                        "{cell}: the log carries stdout"
                    );
                    passes += 1;
                }
                (GateResult::Fail { log }, false) => {
                    assert!(
                        log.contains(must_name),
                        "{cell}: the log must say why it failed (`{must_name}`): {log}"
                    );
                    assert!(
                        log.contains("some output") && log.contains("some diagnostics"),
                        "{cell}: and still carry the evidence: {log}"
                    );
                    fails += 1;
                }
                (actual, _) => panic!("{cell}: expected pass={expect_pass}, got {actual:?}"),
            }
            let seen = runner.seen();
            assert_eq!(seen.len(), 1, "{cell}: one process per gate check");
            assert!(
                matches!(seen[0].role, crate::runner::ExecutionRole::Gate)
                    && seen[0].agent.is_none(),
                "{cell}: a gate runs unbound in the Gate role"
            );
        }
        assert_eq!(GRID.len(), 12, "three exit codes crossed with both flags");
        assert_eq!(passes, 1, "exactly one shape is a pass");
        assert_eq!(fails, 11, "and every other shape is a failure");
    }

    #[test]
    fn a_gate_whose_process_never_ran_returns_the_error_and_synthesizes_nothing() {
        let repo = temp_repo("spawn-failure");
        let ws = Workspace::open(&repo).expect("open");
        let logs = temp_dir("spawn-failure-logs");

        let runner = ScriptedRunner::new(Scripted::SpawnFailure);
        let error = gate("does-not-matter", 30)
            .check(&runner, gate_id(0), &ws)
            .expect_err("an infrastructure failure is not a gate verdict");
        assert!(
            error.to_string().contains("failed to spawn"),
            "the runner's own diagnostic reaches the caller: {error}"
        );

        let runner = ScriptedRunner::new(Scripted::SpawnFailure);
        let gates = [gate("first", 30), gate("second", 30)];
        let error = run_all(&gates, &runner, &gate_id, &ws, &logs, "task-stem", 1)
            .expect_err("run_all propagates it");
        assert!(
            error.to_string().contains("failed to spawn"),
            "and names the same thing: {error}"
        );
        assert_eq!(
            runner.seen().len(),
            1,
            "the first failure stops the sequence"
        );
        let written: Vec<_> = fs::read_dir(&logs)
            .expect("log dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert!(
            written.is_empty(),
            "a process that never ran leaves no evidence file: {written:?}"
        );
    }

    #[test]
    fn quoted_arguments_survive_the_windows_shell() {
        let repo = temp_repo("quoting");
        let ws = Workspace::open(&repo).expect("open");
        let set = gate("git config --local test.quoted \"two words\"", 30);
        assert!(matches!(
            set.check(&host(), gate_id(0), &ws),
            Ok(GateResult::Pass { .. })
        ));
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
        let failure = run_all(&gates, &host(), &gate_id, &ws, &logs, "t1", 1)
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
        let failure = run_all(&gates, &host(), &gate_id, &ws, &logs, "a/b", 1)
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

        for complex in [
            "cd ui && npm test",
            "RUSTFLAGS=-Dwarnings cargo check",
            "\"C:\\Program Files\\tool\\check.exe\" --fast",
            "echo residue> residue.txt",
        ] {
            resolve_programs(&[gate(complex, 30)], &root, &mut warnings)
                .unwrap_or_else(|e| panic!("`{complex}` should be skipped, got {e}"));
        }

        resolve_programs(&[gate("echo hello", 30)], &root, &mut warnings).expect("builtin ok");

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

        assert!(!diff_adds_tests("+    process.exit(1);\n"));
        assert!(!diff_adds_tests("+    let parts = s.split(',');\n"));
        assert!(!diff_adds_tests("+    if (regex.test(input)) {}\n"));
        assert!(!diff_adds_tests("+    tx.commit();\n"));
        assert!(!diff_adds_tests("+just some added prose\n"));
        assert!(!diff_adds_tests("-#[test]\n-fn was_here() {}\n"));
    }

    #[test]
    fn provenance_accepts_test_files_and_assertions() {
        assert!(diff_adds_tests(
            "+++ b/src/lib.rs\n+        assert_eq!(total, 42);\n"
        ));
        assert!(diff_adds_tests("+++ b/tests/api.rs\n+        helper(1);\n"));
        assert!(diff_adds_tests(
            "+++ b/web/src/foo.spec.ts\n+        helper(1);\n"
        ));
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
