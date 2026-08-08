//! Claude Code adapter (DESIGN.md §16).
//!
//! `claude -p` with the prompt on stdin, `--output-format json` parsed
//! defensively, `--model`, `--max-turns`, `--resume` for same-rung retries.
//! Permissions are never the skip-all flag: [`permission_settings`] generates
//! a narrow per-run settings JSON the engine materializes to a file and this
//! adapter passes via `--settings`, keeping the workspace's own
//! `.claude/settings.json` untouched.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use super::proc::{self, ProcessOutput};
use super::{AgentAdapter, Caps, TaskRun};
use crate::error::TactusError;
use crate::ir::{Outcome, OutcomeStatus, PermissionMode, Usage, WorkerProfile};

pub const ADAPTER_ID: &str = "claude-code";

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

pub struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }

    fn probe(&self) -> Result<Caps, TactusError> {
        let invocation = locate()?;
        let mut cmd = invocation.command();
        cmd.arg("--version");
        let out = proc::run_with_timeout(cmd, "", PROBE_TIMEOUT)?;
        if out.timed_out {
            return Err(TactusError::Agent {
                message: format!("`{}` --version timed out", invocation.display()),
            });
        }
        if out.code != Some(0) {
            return Err(TactusError::Agent {
                message: format!(
                    "`{}` --version exited with {:?}: {}",
                    invocation.display(),
                    out.code,
                    out.stderr.trim()
                ),
            });
        }
        Ok(Caps {
            version: extract_version(&out.stdout),
            json_output: true,
            session_resume: true,
            cost_reporting: true,
            // No single flag; achieved through the permission settings.
            read_only_mode: true,
            acp: false,
            model_list: false,
        })
    }

    fn build(&self, run: &TaskRun) -> Result<Command, TactusError> {
        let invocation = locate()?;
        let mut cmd = invocation.command();
        cmd.args(build_args(run));
        cmd.current_dir(&run.workspace);
        Ok(cmd)
    }

    fn parse(&self, out: &ProcessOutput) -> Result<Outcome, TactusError> {
        Ok(parse_output(out))
    }
}

/// Argument list, kept separate from binary resolution so it is testable on
/// machines without the CLI installed.
pub fn build_args(run: &TaskRun) -> Vec<String> {
    let mut args = vec![
        "-p".to_owned(),
        "--output-format".to_owned(),
        "json".to_owned(),
        "--model".to_owned(),
        run.profile.model.clone(),
    ];
    if let Some(turns) = run.profile.max_turns {
        args.push("--max-turns".to_owned());
        args.push(turns.to_string());
    }
    if let Some(session) = &run.resume_session {
        args.push("--resume".to_owned());
        args.push(session.clone());
    }
    if let Some(settings) = &run.settings_path {
        args.push("--settings".to_owned());
        args.push(settings.to_string_lossy().into_owned());
    }
    args.extend(run.profile.extra_args.iter().cloned());
    args
}

/// Narrow per-run permission settings (§20): edit profiles get file tools plus
/// exactly the gate commands; reviewers are read-only. Nobody gets network
/// tools. The engine writes this JSON to the run directory and the command
/// carries it via `--settings`.
pub fn permission_settings(profile: &WorkerProfile, gate_cmds: &[String]) -> Value {
    let mut allow: Vec<String> = match profile.permissions {
        PermissionMode::Edit => ["Read", "Glob", "Grep", "Edit", "Write", "NotebookEdit"]
            .map(str::to_owned)
            .to_vec(),
        PermissionMode::ReadOnly => ["Read", "Glob", "Grep"].map(str::to_owned).to_vec(),
    };
    if profile.permissions == PermissionMode::Edit {
        for gate in gate_cmds {
            allow.push(format!("Bash({gate})"));
        }
    }
    json!({
        "permissions": {
            "allow": allow,
            "deny": ["WebFetch", "WebSearch", "Bash(curl:*)", "Bash(wget:*)"],
        }
    })
}

/// First `digits.digits.digits` token wins; otherwise the trimmed first line
/// verbatim (`--version` formats have churned before).
fn extract_version(stdout: &str) -> String {
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

/// Defensive outcome parsing: the JSON result is trusted when present, but a
/// missing or malformed field never panics and never fails the parse — status
/// degrades to `AgentError` instead. Diff, transcript path, and pool drain
/// are engine-owned and left empty here.
fn parse_output(out: &ProcessOutput) -> Outcome {
    let mut outcome = Outcome {
        status: OutcomeStatus::AgentError,
        diff: String::new(),
        session_id: None,
        usage: None,
        cost_usd: None,
        pool_drain: None,
        transcript_path: PathBuf::new(),
        duration: out.duration,
    };

    let payload: Option<Value> = serde_json::from_str(out.stdout.trim()).ok();
    if let Some(payload) = &payload {
        outcome.session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        outcome.cost_usd = payload
            .get("total_cost_usd")
            .or_else(|| payload.get("cost_usd"))
            .and_then(Value::as_f64);
        outcome.usage = parse_usage(payload);
    }

    if out.timed_out {
        outcome.status = OutcomeStatus::Timeout;
        return outcome;
    }
    if looks_rate_limited(&out.stderr)
        || payload.as_ref().is_some_and(|p| {
            p.get("result")
                .and_then(Value::as_str)
                .is_some_and(looks_rate_limited)
        })
    {
        outcome.status = OutcomeStatus::RateLimited;
        return outcome;
    }
    let succeeded = out.code == Some(0)
        && payload
            .as_ref()
            .is_some_and(|p| !p.get("is_error").and_then(Value::as_bool).unwrap_or(false));
    if succeeded {
        outcome.status = OutcomeStatus::Completed;
    }
    outcome
}

fn parse_usage(payload: &Value) -> Option<Usage> {
    let usage = payload.get("usage")?;
    let field = |name: &str| usage.get(name).and_then(Value::as_u64);
    Some(Usage {
        input_tokens: field("input_tokens"),
        output_tokens: field("output_tokens"),
        cache_creation_input_tokens: field("cache_creation_input_tokens"),
        cache_read_input_tokens: field("cache_read_input_tokens"),
        num_turns: payload
            .get("num_turns")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
    })
}

/// Rate-limit signals are ground truth for the capacity engine (§13); detect
/// them from either stream, case-insensitively.
fn looks_rate_limited(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["usage limit", "rate limit", "overloaded", "429"]
        .iter()
        .any(|needle| lower.contains(needle))
}

// ---------------------------------------------------------------------------
// Binary discovery — Windows-first-class: the CLI may be a native claude.exe
// or an npm claude.cmd shim, which CreateProcess cannot exec directly.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Invocation {
    path: PathBuf,
    via_cmd_shell: bool,
}

impl Invocation {
    fn command(&self) -> Command {
        if self.via_cmd_shell {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(&self.path);
            cmd
        } else {
            Command::new(&self.path)
        }
    }

    fn display(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

fn candidate_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["claude.exe", "claude.cmd", "claude.bat"]
    } else {
        &["claude"]
    }
}

fn locate() -> Result<Invocation, TactusError> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        for name in candidate_names() {
            let candidate = dir.join(name);
            if candidate.is_file() {
                let via_cmd_shell = candidate.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")
                });
                return Ok(Invocation {
                    path: candidate,
                    via_cmd_shell,
                });
            }
        }
    }
    Err(TactusError::Agent {
        message: format!(
            "claude binary not found on PATH (looked for {}); install Claude Code or adjust PATH",
            candidate_names().join(", ")
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::PermissionMode;

    fn profile(permissions: PermissionMode) -> WorkerProfile {
        WorkerProfile {
            name: "impl-mid".to_owned(),
            agent: ADAPTER_ID.to_owned(),
            model: "claude-sonnet-5".to_owned(),
            pool: "claude-max".to_owned(),
            permissions,
            max_turns: Some(30),
            extra_args: Vec::new(),
        }
    }

    fn task_run() -> TaskRun {
        TaskRun {
            prompt: "Do the thing.".to_owned(),
            profile: profile(PermissionMode::Edit),
            workspace: PathBuf::from("."),
            resume_session: None,
            settings_path: None,
        }
    }

    fn output(code: Option<i32>, stdout: &str, stderr: &str) -> ProcessOutput {
        ProcessOutput {
            code,
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            duration: Duration::from_secs(1),
            timed_out: false,
        }
    }

    #[test]
    fn build_args_cover_the_headless_contract() {
        let args = build_args(&task_run());
        let joined = args.join(" ");
        assert!(joined.starts_with("-p --output-format json --model claude-sonnet-5"));
        assert!(joined.contains("--max-turns 30"));
        assert!(!joined.contains("--resume"));
        assert!(!joined.contains("dangerously"), "never the skip-all flag");
    }

    #[test]
    fn resume_settings_and_extra_args_are_appended() {
        let mut run = task_run();
        run.resume_session = Some("sess-123".to_owned());
        run.settings_path = Some(PathBuf::from("run-settings.json"));
        run.profile.extra_args = vec!["--verbose".to_owned()];
        let joined = build_args(&run).join(" ");
        assert!(joined.contains("--resume sess-123"));
        assert!(joined.contains("--settings run-settings.json"));
        assert!(joined.ends_with("--verbose"));
    }

    #[test]
    fn edit_settings_allow_file_tools_and_exact_gates_only() {
        let gates = vec![
            "cargo check --all-targets".to_owned(),
            "cargo test".to_owned(),
        ];
        let settings = permission_settings(&profile(PermissionMode::Edit), &gates);
        let allow = settings["permissions"]["allow"]
            .as_array()
            .expect("allow list");
        let allow: Vec<&str> = allow.iter().filter_map(Value::as_str).collect();
        assert!(allow.contains(&"Edit"));
        assert!(allow.contains(&"Bash(cargo test)"));
        assert!(!allow.iter().any(|a| a == &"Bash"), "no blanket shell");
        let deny = settings["permissions"]["deny"].to_string();
        assert!(deny.contains("WebFetch"), "no network tools: {deny}");
    }

    #[test]
    fn readonly_settings_have_no_edit_or_bash() {
        let gates = vec!["cargo test".to_owned()];
        let settings = permission_settings(&profile(PermissionMode::ReadOnly), &gates);
        let rendered = settings["permissions"]["allow"].to_string();
        assert!(rendered.contains("Read"));
        assert!(!rendered.contains("Edit"));
        assert!(
            !rendered.contains("Bash"),
            "reviewers run nothing: {rendered}"
        );
    }

    #[test]
    fn successful_json_parses_to_completed() {
        let stdout = r#"{"type":"result","subtype":"success","is_error":false,
            "result":"done","session_id":"abc-123","total_cost_usd":0.42,
            "num_turns":6,"usage":{"input_tokens":1200,"output_tokens":300,
            "cache_read_input_tokens":9000}}"#;
        let outcome = parse_output(&output(Some(0), stdout, ""));
        assert_eq!(outcome.status, OutcomeStatus::Completed);
        assert_eq!(outcome.session_id.as_deref(), Some("abc-123"));
        assert_eq!(outcome.cost_usd, Some(0.42));
        let usage = outcome.usage.expect("usage");
        assert_eq!(usage.input_tokens, Some(1200));
        assert_eq!(usage.cache_read_input_tokens, Some(9000));
        assert_eq!(usage.num_turns, Some(6));
        assert!(outcome.diff.is_empty(), "diff is engine-owned");
    }

    #[test]
    fn error_json_and_garbage_degrade_to_agent_error() {
        let stdout =
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"s1"}"#;
        let outcome = parse_output(&output(Some(0), stdout, ""));
        assert_eq!(outcome.status, OutcomeStatus::AgentError);
        assert_eq!(
            outcome.session_id.as_deref(),
            Some("s1"),
            "session survives for resume"
        );

        let outcome = parse_output(&output(Some(0), "not json at all", ""));
        assert_eq!(outcome.status, OutcomeStatus::AgentError);

        let outcome = parse_output(&output(Some(2), "", "boom"));
        assert_eq!(outcome.status, OutcomeStatus::AgentError);
    }

    #[test]
    fn rate_limit_signals_win_over_exit_codes() {
        let outcome = parse_output(&output(Some(1), "", "Claude AI usage limit reached|1723"));
        assert_eq!(outcome.status, OutcomeStatus::RateLimited);

        let stdout = r#"{"type":"result","is_error":true,"result":"5-hour rate limit hit"}"#;
        let outcome = parse_output(&output(Some(0), stdout, ""));
        assert_eq!(outcome.status, OutcomeStatus::RateLimited);
    }

    #[test]
    fn timeout_maps_to_timeout_status() {
        let mut out = output(None, "", "");
        out.timed_out = true;
        assert_eq!(parse_output(&out).status, OutcomeStatus::Timeout);
    }

    #[test]
    fn version_extraction_handles_known_formats() {
        assert_eq!(extract_version("2.1.35 (Claude Code)\n"), "2.1.35");
        assert_eq!(extract_version("claude v1.0.128\n"), "1.0.128");
        assert_eq!(extract_version("weird output\n"), "weird output");
    }

    // Runs only where the real CLI exists; skips silently elsewhere so CI
    // without Claude Code stays green.
    #[test]
    fn probe_against_real_binary_when_present() {
        if locate().is_err() {
            eprintln!("claude not on PATH; skipping live probe");
            return;
        }
        let caps = ClaudeCodeAdapter.probe().expect("probe should succeed");
        assert!(caps.json_output);
        assert!(!caps.version.is_empty());
    }
}
