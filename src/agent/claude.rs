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
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};

use super::bin::{self, Invocation};
use super::proc::{self, ProcessOutput};
use super::{AgentAdapter, AuthState, Caps, Discovery, TaskRun, looks_rate_limited};
use crate::capacity::PoolKind;
use crate::error::TactusError;
use crate::ir::{Outcome, OutcomeStatus, PermissionMode, Usage, WorkerProfile};
use crate::util;

pub const ADAPTER_ID: &str = "claude-code";

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

pub struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }

    fn probe(&self) -> Result<Caps, TactusError> {
        let invocation = locate()?;
        let out = proc::run_with_timeout(
            invocation.command(&["--version".to_owned()]),
            "",
            PROBE_TIMEOUT,
        )?;
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
        let version = bin::extract_version(&out.stdout);

        // Capabilities are read from `--help`, not assumed: this CLI has
        // removed and hidden flags between releases, and a missing flag must
        // surface as a pre-flight refusal rather than as per-task failures
        // once a run is already spending (§16, §19).
        let help = proc::run_with_timeout(
            invocation.command(&["--help".to_owned()]),
            "",
            PROBE_TIMEOUT,
        )?;
        let help_text = format!("{}{}", help.stdout, help.stderr);
        // An unreadable --help is not fatal; fall back to assuming the flags
        // this adapter needs are present and let build/parse report reality.
        let has = |flag: &str| !help.stdout.is_empty() && help_text.contains(flag);
        let readable = help.code == Some(0) && !help_text.trim().is_empty();
        if readable {
            let required = [
                "-p",
                "--output-format",
                "--model",
                "--settings",
                "--setting-sources",
                "--permission-mode",
            ];
            let missing: Vec<&str> = required
                .into_iter()
                .filter(|flag| !help_text.contains(flag))
                .collect();
            if !missing.is_empty() {
                return Err(TactusError::Agent {
                    message: format!(
                        "claude {version} does not advertise required flag(s): {}. This adapter \
                         pins known-good behavior per version — upgrade tactus or pin an older \
                         claude.",
                        missing.join(", ")
                    ),
                });
            }
        }
        Ok(Caps {
            version,
            json_output: !readable || has("--output-format"),
            session_resume: !readable || has("--resume"),
            cost_reporting: true,
            // No single flag; achieved through the permission settings.
            read_only_mode: true,
            acp: readable && has("--acp"),
            model_list: readable && has("--list-models"),
        })
    }

    fn build(&self, run: &TaskRun) -> Result<Command, TactusError> {
        let invocation = locate()?;
        let mut cmd = invocation.command(&build_args(run));
        cmd.current_dir(&run.workspace);
        Ok(cmd)
    }

    fn parse(&self, out: &ProcessOutput) -> Result<Outcome, TactusError> {
        Ok(parse_output(out))
    }

    /// `claude auth status --json` — a zero-spend auth probe that handles no
    /// token and reads no credential file: the CLI answers about itself, and
    /// this reads its answer.
    fn discover(&self) -> Result<Discovery, TactusError> {
        let invocation = locate()?;
        let out = proc::run_with_timeout(
            invocation.command(&["auth".to_owned(), "status".to_owned(), "--json".to_owned()]),
            "",
            PROBE_TIMEOUT,
        )?;
        let mut discovery = parse_auth_status(&out);
        // §13's tier classification comes from the catalog either way, but
        // saying so is what stops the pools file reading as though the roster
        // had been confirmed against this machine.
        discovery.notes.push(
            "this CLI offers no non-interactive model listing, so the roster for this agent is \
             the catalog shipped with tactus, not something confirmed here"
                .to_owned(),
        );
        Ok(discovery)
    }

    fn materialize_permissions(
        &self,
        profile: &WorkerProfile,
        gate_cmds: &[String],
        dir: &std::path::Path,
        stem: &str,
    ) -> Result<Option<PathBuf>, TactusError> {
        let path = dir.join(format!("{stem}.json"));
        util::write_json(&path, &permission_settings(profile, gate_cmds))?;
        Ok(Some(path))
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
        // Anything not explicitly allowed is denied rather than prompted:
        // an unattended run must never sit waiting on a permission question.
        "--permission-mode".to_owned(),
        "dontAsk".to_owned(),
        // Load NO user/project/local settings: the per-run settings file is
        // the whole permission surface. Without this, allow rules from
        // ~/.claude/settings.json (or a repo's own .claude/settings.json)
        // union with ours and silently widen the sandbox (§20).
        "--setting-sources".to_owned(),
        String::new(),
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
            // No network tools; and no writing to the files that decide what
            // later attempts may do — an agent that can edit .claude/ or
            // .git/ config escalates its own permissions for the rest of the
            // run (invariant 1 and §20).
            //
            // `.tactus/` joins them now that `events.jsonl` is the source of
            // truth: an agent that can append to it could forge a
            // `task_committed`, and one that can truncate it could erase its
            // own failures. Writes there are also never legitimate — the
            // engine owns that directory the way it owns git.
            //
            // The `Read` denials are defence in depth rather than the
            // mechanism. A gate runs repository code the implementer just
            // wrote, and that code can read any workspace path no permission
            // rule ever sees. The actual guarantee comes from §15's split:
            // transcripts, verdicts, and settings live outside the workspace,
            // where there is no path to them at all.
            "deny": [
                "WebFetch",
                "WebSearch",
                "Bash(curl:*)",
                "Bash(wget:*)",
                "Write(.claude/**)",
                "Edit(.claude/**)",
                "Write(**/.claude/**)",
                "Edit(**/.claude/**)",
                "Write(.git/**)",
                "Edit(.git/**)",
                "Write(.tactus/**)",
                "Edit(.tactus/**)",
                "Write(**/.tactus/**)",
                "Edit(**/.tactus/**)",
                "Read(.tactus/**)",
                "Read(**/.tactus/**)",
            ],
        }
    })
}

/// Defensive outcome parsing: the JSON result is trusted when present, but a
/// missing or malformed field never panics and never fails the parse — status
/// degrades to `AgentError` instead. Diff, transcript path, and pool drain
/// are engine-owned and left empty here.
fn parse_output(out: &ProcessOutput) -> Outcome {
    let mut outcome = Outcome {
        status: OutcomeStatus::AgentError,
        diff: String::new(),
        detail: None,
        session_id: None,
        usage: None,
        cost_usd: None,
        pool_drain: None,
        transcript_path: PathBuf::new(),
        duration: out.duration,
    };

    let payload: Option<Value> = serde_json::from_str(out.stdout.trim()).ok();
    let mut result_text: Option<String> = None;
    let mut subtype: Option<String> = None;
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
        result_text = payload
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_owned);
        subtype = payload
            .get("subtype")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }

    if out.timed_out {
        outcome.status = OutcomeStatus::Timeout;
        outcome.detail = Some("attempt exceeded its wall-clock timeout".to_owned());
        return outcome;
    }

    let is_error = payload
        .as_ref()
        .is_some_and(|p| p.get("is_error").and_then(Value::as_bool).unwrap_or(false));
    let failed = out.code != Some(0) || payload.is_none() || is_error;
    if !failed {
        outcome.status = OutcomeStatus::Completed;
        // The agent's final message, not just error text: the reviewer's
        // verdict travels in exactly this field on the SUCCESS path, so
        // leaving it None here makes every review unparseable.
        outcome.detail = result_text;
        return outcome;
    }

    // Rate-limit detection only applies to failures: a SUCCESSFUL task about
    // rate limiting ("added backoff for 429 responses") must never be read as
    // the pool being exhausted.
    let rate_limited = looks_rate_limited(&out.stderr)
        || result_text.as_deref().is_some_and(looks_rate_limited)
        || subtype.as_deref().is_some_and(looks_rate_limited);
    outcome.status = if rate_limited {
        OutcomeStatus::RateLimited
    } else {
        OutcomeStatus::AgentError
    };
    // Give the engine something to report: the CLI signals most failures
    // through the JSON body with an empty stderr.
    outcome.detail = first_non_empty([
        result_text.as_deref(),
        subtype.as_deref(),
        Some(out.stderr.trim()),
        (payload.is_none() && !out.stdout.trim().is_empty())
            .then_some("agent produced unparseable output"),
    ]);
    outcome
}

/// Read `claude auth status --json`, as defensively as every other payload this
/// adapter parses: a missing or malformed field yields
/// [`AuthState::Unknown`], never an error and never a confident wrong answer.
///
/// The observed shape (Aug 2026) is
/// `{"loggedIn": bool, "authMethod": "…", "apiProvider": "…"}`. `loggedIn`
/// drives the auth state; the other two distinguish §13's two billing shapes —
/// a subscription window from api-key dollars — because that decides which
/// estimator rule the written pool gets.
fn parse_auth_status(out: &ProcessOutput) -> Discovery {
    let mut discovery = Discovery::unknown();
    if out.timed_out {
        return discovery.with_note("`claude auth status --json` timed out; auth state unknown");
    }
    let Some(payload): Option<Value> = serde_json::from_str(out.stdout.trim()).ok() else {
        // A non-zero exit with no JSON is the shape an older CLI without the
        // subcommand leaves. Not being able to ask is not the same as an
        // answer, so it stays Unknown.
        return discovery.with_note(format!(
            "`claude auth status --json` did not return JSON (exit {:?}); auth state unknown — \
             this CLI may predate the subcommand",
            out.code
        ));
    };
    discovery.auth = match payload.get("loggedIn").and_then(Value::as_bool) {
        Some(true) => AuthState::Authenticated,
        Some(false) => AuthState::NotAuthenticated,
        None => AuthState::Unknown,
    };
    let method = payload
        .get("authMethod")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let provider = payload
        .get("apiProvider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let described = format!("{method} {provider}").to_ascii_lowercase();
    // Only two readings are confident enough to act on. Anything else leaves
    // `shape` as None and lets the caller apply a documented default it can
    // then tell the operator about.
    discovery.shape = if described.contains("bedrock")
        || described.contains("vertex")
        || described.contains("api")
        || described.contains("key")
    {
        Some(PoolKind::ApiKey)
    } else if described.contains("subscription")
        || described.contains("claudeai")
        || described.contains("oauth")
        || described.contains("max")
        || described.contains("pro")
    {
        Some(PoolKind::SubscriptionWindow)
    } else {
        None
    };
    if !method.is_empty() || !provider.is_empty() {
        discovery
            .notes
            .push(format!("auth method `{method}`, provider `{provider}`"));
    }
    if discovery.shape.is_none() {
        discovery.notes.push(
            "the CLI did not say whether this account bills as a subscription window or as api \
             dollars, so the pool below takes a default — change `kind` if it is wrong"
                .to_owned(),
        );
    }
    discovery
}

fn first_non_empty<'a>(candidates: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|c| !c.is_empty())
        .map(str::to_owned)
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

// ---------------------------------------------------------------------------
// Binary discovery — Windows-first-class: the CLI may be a native claude.exe
// or an npm claude.cmd shim, which CreateProcess cannot exec directly. The
// mechanics live in `super::bin`, shared with every other adapter.
// ---------------------------------------------------------------------------

fn candidate_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["claude.exe", "claude.cmd", "claude.bat"]
    } else {
        &["claude"]
    }
}

/// This adapter's own resolution cache; `bin::locate` fills it once.
static RESOLVED: OnceLock<Option<Invocation>> = OnceLock::new();

fn locate() -> Result<Invocation, TactusError> {
    bin::locate(candidate_names(), &RESOLVED, |tried| {
        format!(
            "claude binary not found on PATH (looked for {}); install Claude Code or adjust PATH",
            tried.join(", ")
        )
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
            gate_cmds: Vec::new(),
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
    fn no_profile_may_write_to_the_run_record() {
        // The event log is the source of truth (invariant 4). An agent that
        // could append to it could forge a `task_committed`; one that could
        // truncate it could erase its own failures. Neither is a permission a
        // worker or a reviewer has any legitimate use for.
        for permissions in [PermissionMode::Edit, PermissionMode::ReadOnly] {
            let settings = permission_settings(&profile(permissions), &["cargo test".to_owned()]);
            let deny = settings["permissions"]["deny"].to_string();
            for rule in [
                "Write(.tactus/**)",
                "Edit(.tactus/**)",
                "Write(**/.tactus/**)",
                "Edit(**/.tactus/**)",
            ] {
                assert!(
                    deny.contains(rule),
                    "{permissions:?} is missing {rule}: {deny}"
                );
            }
            // Defence in depth only — the enforceable half of withholding is
            // §15's split, which puts transcripts outside the workspace where
            // no rule is needed.
            assert!(deny.contains("Read(.tactus/**)"), "{deny}");
        }
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
        assert_eq!(
            outcome.detail.as_deref(),
            Some("done"),
            "the final message must survive on the success path — the reviewer's \
             verdict travels in it"
        );
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
    fn shipped_subscription_limit_phrasings_are_detected() {
        for phrase in [
            "5-hour limit reached ∙ resets 6pm",
            "Weekly limit reached",
            "Session limit reached",
            "API error: rate_limit_error",
            "quota exceeded",
        ] {
            let stdout = format!(
                r#"{{"type":"result","is_error":true,"result":"{}"}}"#,
                phrase.replace('"', "")
            );
            assert_eq!(
                parse_output(&output(Some(1), &stdout, "")).status,
                OutcomeStatus::RateLimited,
                "phrase should signal a rate limit: {phrase}"
            );
        }
    }

    #[test]
    fn a_successful_task_about_rate_limits_is_not_rate_limited() {
        // The agent's own summary mentioning 429s must not be read as the
        // pool being exhausted — that would roll back verified work.
        let stdout = r#"{"type":"result","subtype":"success","is_error":false,
            "result":"Added backoff handling for HTTP 429 rate limit responses.",
            "session_id":"s1","total_cost_usd":0.2}"#;
        let outcome = parse_output(&output(Some(0), stdout, ""));
        assert_eq!(outcome.status, OutcomeStatus::Completed);
    }

    #[test]
    fn json_error_failures_carry_a_reportable_detail() {
        let stdout = r#"{"type":"result","subtype":"error_max_turns","is_error":true,
            "session_id":"s1","result":"Reached the maximum number of turns."}"#;
        let outcome = parse_output(&output(Some(0), stdout, ""));
        assert_eq!(outcome.status, OutcomeStatus::AgentError);
        assert_eq!(
            outcome.detail.as_deref(),
            Some("Reached the maximum number of turns."),
            "the engine has something to show without opening the transcript"
        );

        // Falls back to the subtype, then stderr, then a pointer.
        let stdout = r#"{"is_error":true,"subtype":"error_during_execution"}"#;
        let outcome = parse_output(&output(Some(0), stdout, ""));
        assert_eq!(outcome.detail.as_deref(), Some("error_during_execution"));
        let outcome = parse_output(&output(Some(2), "", "spawn failed"));
        assert_eq!(outcome.detail.as_deref(), Some("spawn failed"));
    }

    #[test]
    fn headless_args_pin_the_sandbox() {
        let joined = build_args(&task_run()).join(" ");
        assert!(
            joined.contains("--permission-mode dontAsk"),
            "unattended runs must deny rather than wait: {joined}"
        );
        assert!(
            joined.contains("--setting-sources "),
            "external settings must not widen the sandbox: {joined}"
        );
        let args = build_args(&task_run());
        let index = args
            .iter()
            .position(|a| a == "--setting-sources")
            .expect("flag");
        assert_eq!(args[index + 1], "", "empty list loads no external sources");
    }

    #[test]
    fn permission_settings_protect_the_permission_files_themselves() {
        let deny = permission_settings(&profile(PermissionMode::Edit), &[])["permissions"]["deny"]
            .to_string();
        assert!(
            deny.contains(".claude/**"),
            "cannot widen its own sandbox: {deny}"
        );
        assert!(
            deny.contains(".git/**"),
            "cannot rewrite git config: {deny}"
        );
    }

    #[test]
    fn timeout_maps_to_timeout_status() {
        let mut out = output(None, "", "");
        out.timed_out = true;
        assert_eq!(parse_output(&out).status, OutcomeStatus::Timeout);
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
