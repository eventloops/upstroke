//! Extended notes: `docs/internals/agent/claude.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use super::bin::{self, Invocation};
use super::proc::ProcessOutput;
use super::{AgentAdapter, AuthState, Caps, Discovery, TaskRun, looks_rate_limited, probe_request};
use crate::capacity::PoolKind;
use crate::error::UpstrokeError;
use crate::ir::{Effort, Outcome, OutcomeStatus, PermissionMode, Usage, WorkerProfile};
use crate::runner::{CommandSpec, Runner};
use crate::util;

pub const ADAPTER_ID: &str = "claude-code";

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

const REQUIRED_FLAGS: [&str; 6] = [
    "--output-format",
    "--model",
    "--effort",
    "--settings",
    "--setting-sources",
    "--permission-mode",
];
const REQUIRED_SHORT_FLAGS: [&str; 1] = ["-p"];

mod probe_ordinal {
    pub const VERSION: u32 = 0;
    pub const HELP: u32 = 1;
    pub const AUTH_STATUS: u32 = 2;
    #[cfg(test)]
    pub const ALL: [u32; 3] = [VERSION, HELP, AUTH_STATUS];
}

pub struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }

    fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {
        let invocation = cli();
        let out = runner
            .run(&probe_request(
                ADAPTER_ID,
                invocation.spec(&["--version".to_owned()])?,
                probe_ordinal::VERSION,
                PROBE_TIMEOUT,
            )?)
            .map_err(|cause| bin::boundary_refused(CLI, INSTALL_HINT, &cause))?;
        if out.output_limited {
            return Err(UpstrokeError::Agent {
                message: format!(
                    "`{}` --version exceeded the output limit",
                    invocation.display()
                ),
            });
        }
        if out.timed_out {
            return Err(UpstrokeError::Agent {
                message: format!("`{}` --version timed out", invocation.display()),
            });
        }
        if out.code != Some(0) {
            return Err(UpstrokeError::Agent {
                message: format!(
                    "`{}` --version exited with {:?}: {}",
                    invocation.display(),
                    out.code,
                    out.stderr.trim()
                ),
            });
        }
        let version = bin::extract_version(&out.stdout);

        let help = runner.run(&probe_request(
            ADAPTER_ID,
            invocation.spec(&["--help".to_owned()])?,
            probe_ordinal::HELP,
            PROBE_TIMEOUT,
        )?)?;
        let help_text = checked_help(&invocation.display(), &help)?;
        validate_help(&version, &help_text)?;
        let has = |flag: &str| super::advertises_flag(&help_text, flag);
        Ok(Caps {
            version,
            json_output: has("--output-format"),
            session_resume: has("--resume"),
            cost_reporting: true,
            read_only_mode: true,
            acp: has("--acp"),
            model_list: has("--list-models"),
        })
    }

    fn build(&self, run: &TaskRun) -> Result<CommandSpec, UpstrokeError> {
        cli().spec(&build_args(run))
    }

    fn parse(&self, out: &ProcessOutput) -> Result<Outcome, UpstrokeError> {
        Ok(parse_output(out))
    }

    fn discover(&self, runner: &dyn Runner, _caps: &Caps) -> Result<Discovery, UpstrokeError> {
        let invocation = cli();
        let out = runner
            .run(&probe_request(
                ADAPTER_ID,
                invocation.spec(&["auth".to_owned(), "status".to_owned(), "--json".to_owned()])?,
                probe_ordinal::AUTH_STATUS,
                PROBE_TIMEOUT,
            )?)
            .map_err(|cause| bin::boundary_refused(CLI, INSTALL_HINT, &cause))?;
        let mut discovery = parse_auth_status(&out);
        discovery.notes.push(
            "this CLI offers no non-interactive model listing, so the roster for this agent is \
             the catalog shipped with upstroke, not something confirmed here"
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
    ) -> Result<Option<PathBuf>, UpstrokeError> {
        let path = dir.join(format!("{stem}.json"));
        util::write_json(&path, &permission_settings(profile, gate_cmds))?;
        Ok(Some(path))
    }
}

fn checked_help(program: &str, output: &ProcessOutput) -> Result<String, UpstrokeError> {
    if output.output_limited {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program}` --help exceeded the output limit; effort support could not be verified"
            ),
        });
    }
    if output.timed_out {
        return Err(UpstrokeError::Agent {
            message: format!("`{program}` --help timed out; effort support could not be verified"),
        });
    }
    if output.code != Some(0) {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program}` --help exited with {:?}: {}",
                output.code,
                output.stderr.trim()
            ),
        });
    }
    let text = format!("{}\n{}", output.stdout, output.stderr);
    if text.trim().is_empty() {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program}` --help returned no output; effort support could not be verified"
            ),
        });
    }
    Ok(text)
}

fn validate_help(version: &str, help: &str) -> Result<(), UpstrokeError> {
    let missing_flags: Vec<&str> = REQUIRED_FLAGS
        .into_iter()
        .filter(|flag| !super::advertises_flag(help, flag))
        .chain(
            REQUIRED_SHORT_FLAGS
                .into_iter()
                .filter(|flag| !super::advertises_flag(help, flag)),
        )
        .collect();
    if !missing_flags.is_empty() {
        return Err(UpstrokeError::Agent {
            message: format!(
                "claude {version} does not advertise required flag(s): {}. This adapter pins \
                 known-good behavior per version — upgrade upstroke or pin an older claude.",
                missing_flags.join(", ")
            ),
        });
    }
    let missing_efforts = super::missing_effort_levels(help);
    if !missing_efforts.is_empty() {
        return Err(UpstrokeError::Agent {
            message: format!(
                "claude {version} advertises `--effort` but not required level(s): {}. Refusing \
                 before spend because this run may request any shared effort level.",
                missing_efforts
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    Ok(())
}

fn effort_flag(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh => "xhigh",
        Effort::Max => "max",
    }
}

pub fn build_args(run: &TaskRun) -> Vec<String> {
    let mut args = vec![
        "-p".to_owned(),
        "--output-format".to_owned(),
        "json".to_owned(),
        "--model".to_owned(),
        run.profile.model.clone(),
        "--permission-mode".to_owned(),
        "dontAsk".to_owned(),
        "--setting-sources".to_owned(),
        String::new(),
    ];
    if let Some(effort) = run.profile.effort {
        args.push("--effort".to_owned());
        args.push(effort_flag(effort).to_owned());
    }
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
                "Write(.upstroke/**)",
                "Edit(.upstroke/**)",
                "Write(**/.upstroke/**)",
                "Edit(**/.upstroke/**)",
                "Read(.upstroke/**)",
                "Read(**/.upstroke/**)",
            ],
        }
    })
}

fn parse_output(out: &ProcessOutput) -> Outcome {
    let mut outcome = Outcome {
        status: OutcomeStatus::AgentError,
        diff: String::new(),
        detail: None,
        session_id: None,
        usage: None,
        cost_usd: None,
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

    if out.output_limited {
        outcome.status = OutcomeStatus::AgentError;
        outcome.detail = Some("agent exceeded the stdout/stderr output limit".to_owned());
        return outcome;
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
        outcome.detail = result_text;
        return outcome;
    }

    let rate_limited = looks_rate_limited(&out.stderr)
        || result_text.as_deref().is_some_and(looks_rate_limited)
        || subtype.as_deref().is_some_and(looks_rate_limited);
    outcome.status = if rate_limited {
        OutcomeStatus::RateLimited
    } else {
        OutcomeStatus::AgentError
    };
    outcome.detail = first_non_empty([
        result_text.as_deref(),
        subtype.as_deref(),
        Some(out.stderr.trim()),
        (payload.is_none() && !out.stdout.trim().is_empty())
            .then_some("agent produced unparseable output"),
    ]);
    outcome
}

fn parse_auth_status(out: &ProcessOutput) -> Discovery {
    let mut discovery = Discovery::unknown();
    if out.timed_out {
        return discovery.with_note("`claude auth status --json` timed out; auth state unknown");
    }
    let Some(payload): Option<Value> = serde_json::from_str(out.stdout.trim()).ok() else {
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
    let subscription = payload
        .get("subscriptionType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    discovery.shape = classify_shape(method, provider, subscription);
    if !method.is_empty() || !provider.is_empty() {
        let plan = if subscription.is_empty() {
            String::new()
        } else {
            format!(", plan `{subscription}`")
        };
        discovery.notes.push(format!(
            "auth method `{method}`, provider `{provider}`{plan}"
        ));
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

fn classify_shape(method: &str, provider: &str, subscription_type: &str) -> Option<PoolKind> {
    const API: [&str; 6] = ["api", "apikey", "api_key", "key", "bedrock", "vertex"];
    const SUBSCRIPTION: [&str; 8] = [
        "subscription",
        "claudeai",
        "claude.ai",
        "oauth",
        "max",
        "pro",
        "team",
        "enterprise",
    ];
    let tokens: Vec<String> = format!("{method} {provider} {subscription_type}")
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect();
    let matches = |set: &[&str]| tokens.iter().any(|t| set.contains(&t.as_str()));
    match (matches(&API), matches(&SUBSCRIPTION)) {
        (true, false) => Some(PoolKind::ApiKey),
        (false, true) => Some(PoolKind::SubscriptionWindow),
        _ => None,
    }
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
        reasoning_output_tokens: None,
    })
}

const CLI: &str = "claude";

const INSTALL_HINT: &str = "Install Claude Code there, or select a different agent.";

fn cli() -> Invocation {
    Invocation::named(CLI)
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
            effort: Some(crate::ir::Effort::Medium),
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
            output_limited: false,
        }
    }

    #[test]
    fn build_args_cover_the_headless_contract() {
        let args = build_args(&task_run());
        let joined = args.join(" ");
        assert!(joined.starts_with("-p --output-format json --model claude-sonnet-5"));
        assert!(joined.contains("--effort medium"));
        assert!(joined.contains("--max-turns 30"));
        assert!(!joined.contains("--resume"));
        assert!(!joined.contains("dangerously"), "never the skip-all flag");
    }

    #[test]
    fn every_effort_has_the_exact_cli_spelling_in_build_args() {
        let expected = [
            (Effort::Low, "low"),
            (Effort::Medium, "medium"),
            (Effort::High, "high"),
            (Effort::XHigh, "xhigh"),
            (Effort::Max, "max"),
        ];
        for (effort, spelling) in expected {
            assert_eq!(effort_flag(effort), spelling);
            let mut run = task_run();
            run.profile.effort = Some(effort);
            let args = build_args(&run);
            let at = args
                .iter()
                .position(|arg| arg == "--effort")
                .expect("effort flag");
            assert_eq!(args.get(at + 1).map(String::as_str), Some(spelling));
        }
    }

    #[test]
    fn help_validation_requires_every_shared_effort_level() {
        let help = "-p --output-format --model --settings --setting-sources \
                    --permission-mode\n  --effort <level> (low, medium, high, xhigh, max)\n";
        validate_help("2.1.226", help).expect("full shared vocabulary");

        let no_print = "--output-format --model --settings --setting-sources \
                        --permission-mode\n  --effort <level> (low, medium, high, xhigh, max)\n";
        let error = validate_help("2.1.226", no_print)
            .expect_err("--permission-mode must not masquerade as -p")
            .to_string();
        assert!(error.contains("-p"), "{error}");

        for (missing, narrowed) in [
            ("xhigh", "low, medium, high, max"),
            ("max", "low, medium, high, xhigh"),
        ] {
            let help = format!(
                "-p --output-format --model --settings --setting-sources --permission-mode\n  \
                 --effort <level> ({narrowed})\n"
            );
            let error = validate_help("2.1.226", &help).expect_err("narrow enum must refuse");
            let message = error.to_string();
            assert!(message.contains(missing), "{message}");
            assert!(message.contains("2.1.226"), "{message}");
        }
    }

    #[test]
    fn unreadable_help_is_a_preflight_refusal() {
        let mut timed_out = output(Some(0), "full help", "");
        timed_out.timed_out = true;
        assert!(
            checked_help("claude", &timed_out)
                .expect_err("timeout")
                .to_string()
                .contains("could not be verified")
        );

        let failed = output(Some(2), "", "bad option");
        assert!(
            checked_help("claude", &failed)
                .expect_err("nonzero")
                .to_string()
                .contains("bad option")
        );

        let empty = output(Some(0), "", "");
        assert!(
            checked_help("claude", &empty)
                .expect_err("empty")
                .to_string()
                .contains("no output")
        );
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
        for permissions in [PermissionMode::Edit, PermissionMode::ReadOnly] {
            let settings = permission_settings(&profile(permissions), &["cargo test".to_owned()]);
            let deny = settings["permissions"]["deny"].to_string();
            for rule in [
                "Write(.upstroke/**)",
                "Edit(.upstroke/**)",
                "Write(**/.upstroke/**)",
                "Edit(**/.upstroke/**)",
            ] {
                assert!(
                    deny.contains(rule),
                    "{permissions:?} is missing {rule}: {deny}"
                );
            }
            assert!(deny.contains("Read(.upstroke/**)"), "{deny}");
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
        let out = output(Some(0), stdout, "");
        let outcome = parse_output(&out);
        assert_eq!(outcome.status, OutcomeStatus::Completed);
        assert_eq!(outcome.duration, out.duration);
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

    #[test]
    fn every_preflight_process_has_its_own_ordinal() {
        use std::collections::BTreeSet;

        let ordinals: BTreeSet<u32> = probe_ordinal::ALL.into_iter().collect();
        assert_eq!(
            ordinals.len(),
            3,
            "`--version`, `--help`, `auth status --json` — 3 processes, 3 identities"
        );
        assert_eq!(probe_ordinal::ALL.len(), 3);

        let ids: BTreeSet<String> = probe_ordinal::ALL
            .into_iter()
            .map(|ordinal| {
                crate::runner::InvocationId::probe(
                    crate::runner::ProbeTarget::Agent(crate::runner::AgentId::new(ADAPTER_ID)),
                    ordinal,
                )
                .expect("the adapter id survives an invocation identity")
                .render()
            })
            .collect();
        assert_eq!(ids.len(), 3);
        assert!(
            ids.iter().all(|id| id.starts_with("p.agent-claude-code.o")),
            "the probe form, naming this agent: {ids:?}"
        );
    }
    #[test]
    fn probe_against_real_binary_when_present() {
        if crate::util::find_program(CLI).is_none() {
            eprintln!("claude not on PATH; skipping live probe");
            return;
        }
        let caps = ClaudeCodeAdapter
            .probe(&crate::runner::host::HostRunner::new())
            .expect("probe should succeed");
        assert!(caps.json_output);
        assert!(!caps.version.is_empty());
    }

    #[test]
    fn auth_status_reads_the_signed_in_shape_including_the_plan() {
        let signed_in = output(
            Some(0),
            r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty",
                "email":"dev@example.com","orgId":"00000000-0000-0000-0000-000000000000",
                "orgName":"dev's Organization","subscriptionType":"max"}"#,
            "",
        );
        let discovery = parse_auth_status(&signed_in);
        assert_eq!(discovery.auth, AuthState::Authenticated);
        assert_eq!(discovery.shape, Some(PoolKind::SubscriptionWindow));
        assert!(
            discovery.notes.iter().any(|n| n.contains("plan `max`")),
            "the plan is worth telling the operator: {:?}",
            discovery.notes
        );

        let sso = output(
            Some(0),
            r#"{"loggedIn":true,"authMethod":"corp-sso","apiProvider":"firstParty",
                "subscriptionType":"enterprise"}"#,
            "",
        );
        assert_eq!(
            parse_auth_status(&sso).shape,
            Some(PoolKind::SubscriptionWindow)
        );

        let signed_out = output(
            Some(1),
            r#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#,
            "",
        );
        let discovery = parse_auth_status(&signed_out);
        assert_eq!(discovery.auth, AuthState::NotAuthenticated);
        assert_eq!(discovery.shape, None);
    }
}
