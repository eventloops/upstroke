//! Extended notes: `docs/internals/agent/codex.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::bin::{self, Invocation};
use super::proc::ProcessOutput;
use super::{
    AdapterSource, AgentAdapter, AuthState, Caps, Discovery, TaskRun, looks_rate_limited,
    probe_request,
};
use crate::capacity::PoolKind;
use crate::catalog;
use crate::error::UpstrokeError;
use crate::ir::{Effort, Outcome, OutcomeStatus, PermissionMode, Usage, WorkerProfile};
use crate::runner::{CommandSpec, Runner};
use crate::util;

pub const ADAPTER_ID: &str = "codex";

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

const CONFIG_PROBE_UNKNOWN_KEY: &str = "upstroke_probe_deliberately_unknown";
const CONFIG_PROBE_SCHEMA_FILE: &str = "upstroke-output-schema-must-not-exist.json";
const CONFIG_PROBE_RESUME_ID: &str = "00000000-0000-0000-0000-000000000000";

const REQUIRED_EXEC_FLAGS: [&str; 5] = ["--json", "--sandbox", "--model", "-c", "--config"];
const REQUIRED_RESUME_FLAGS: [&str; 4] = ["--json", "--model", "-c", "--config"];

mod probe_ordinal {
    pub const VERSION: u32 = 0;
    pub const EXEC_HELP: u32 = 1;
    pub const RESUME_HELP: u32 = 2;
    pub const CONFIG_BASE: u32 = 3;
    pub const CONFIG_PER_SURFACE: u32 = 3;
    pub const PROBE_MODELS: u32 = 9;
    pub const LOGIN_STATUS: u32 = 10;
    pub const DISCOVER_MODELS: u32 = 11;
    #[cfg(test)]
    pub const ALL: [u32; 12] = [
        VERSION,
        EXEC_HELP,
        RESUME_HELP,
        CONFIG_BASE,
        CONFIG_BASE + 1,
        CONFIG_BASE + 2,
        CONFIG_BASE + CONFIG_PER_SURFACE,
        CONFIG_BASE + CONFIG_PER_SURFACE + 1,
        CONFIG_BASE + CONFIG_PER_SURFACE + 2,
        PROBE_MODELS,
        LOGIN_STATUS,
        DISCOVER_MODELS,
    ];
}

#[derive(Debug, Deserialize)]
struct DebugModels {
    models: Vec<DebugModel>,
}

#[derive(Debug, Deserialize)]
struct DebugModel {
    slug: String,
    #[serde(default)]
    supported_reasoning_levels: Vec<DebugReasoningLevel>,
}

#[derive(Debug, Deserialize)]
struct DebugReasoningLevel {
    effort: String,
}

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
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

        let fresh_help = runner.run(&probe_request(
            ADAPTER_ID,
            invocation.spec(&["exec".to_owned(), "--help".to_owned()])?,
            probe_ordinal::EXEC_HELP,
            PROBE_TIMEOUT,
        )?)?;
        let fresh_help = checked_help(&invocation.display(), "exec", &fresh_help)?;
        let resume_help = runner.run(&probe_request(
            ADAPTER_ID,
            invocation.spec(&["exec".to_owned(), "resume".to_owned(), "--help".to_owned()])?,
            probe_ordinal::RESUME_HELP,
            PROBE_TIMEOUT,
        )?)?;
        let resume_help = checked_help(&invocation.display(), "exec resume", &resume_help)?;
        validate_probe_contract(&version, &fresh_help, &resume_help)?;
        validate_effort_config_key(runner, &invocation, &version)?;

        let models = runner.run(&probe_request(
            ADAPTER_ID,
            invocation.spec(&["debug".to_owned(), "models".to_owned()])?,
            probe_ordinal::PROBE_MODELS,
            PROBE_TIMEOUT,
        )?)?;
        let models = checked_model_catalog(&invocation.display(), &models)?;
        let parsed = parse_debug_models(&models)?;
        validate_model_efforts(&version, &parsed)?;

        Ok(Caps {
            version,
            json_output: true,
            session_resume: true,
            cost_reporting: false,
            read_only_mode: true,
            acp: false,
            model_list: true,
        })
    }

    fn build(&self, run: &TaskRun) -> Result<CommandSpec, UpstrokeError> {
        if let Some(refusal) = edit_refusal(&run.profile) {
            return Err(refusal);
        }
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
                invocation.spec(&["login".to_owned(), "status".to_owned()])?,
                probe_ordinal::LOGIN_STATUS,
                PROBE_TIMEOUT,
            )?)
            .map_err(|cause| bin::boundary_refused(CLI, INSTALL_HINT, &cause))?;
        let mut discovery = parse_login_status(&out);
        let models = runner.run(&probe_request(
            ADAPTER_ID,
            invocation.spec(&["debug".to_owned(), "models".to_owned()])?,
            probe_ordinal::DISCOVER_MODELS,
            PROBE_TIMEOUT,
        )?)?;
        let models = checked_model_catalog(&invocation.display(), &models)?;
        discovery.models = parse_debug_models(&models)?
            .models
            .into_iter()
            .map(|model| model.slug)
            .collect();
        Ok(discovery.with_note(
            "model slugs and reasoning levels were confirmed against this CLI's local `debug \
             models` catalog",
        ))
    }

    fn materialize_permissions(
        &self,
        profile: &WorkerProfile,
        _gate_cmds: &[String],
        dir: &std::path::Path,
        stem: &str,
    ) -> Result<Option<PathBuf>, UpstrokeError> {
        let path = dir.join(format!("{stem}.json"));
        util::write_json(
            &path,
            &json!({
                "agent": ADAPTER_ID,
                "profile": profile.name,
                "permissions": profile.permissions,
                "note": "recorded for audit only; codex takes its sandbox as an argv flag",
                "sandbox": sandbox_mode(profile),
            }),
        )?;
        Ok(None)
    }
}

fn checked_help(
    program: &str,
    surface: &str,
    output: &ProcessOutput,
) -> Result<String, UpstrokeError> {
    if output.output_limited {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program} {surface} --help` exceeded the output limit; reasoning configuration support could not be verified"
            ),
        });
    }
    if output.timed_out {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program} {surface} --help` timed out; reasoning configuration support could \
                 not be verified"
            ),
        });
    }
    if output.code != Some(0) {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program} {surface} --help` exited with {:?}: {}",
                output.code,
                output.stderr.trim()
            ),
        });
    }
    let text = format!("{}\n{}", output.stdout, output.stderr);
    if text.trim().is_empty() {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program} {surface} --help` returned no output; reasoning configuration \
                 support could not be verified"
            ),
        });
    }
    Ok(text)
}

fn validate_probe_contract(
    version: &str,
    fresh_help: &str,
    resume_help: &str,
) -> Result<(), UpstrokeError> {
    for (surface, help, required) in [
        ("exec", fresh_help, REQUIRED_EXEC_FLAGS.as_slice()),
        ("exec resume", resume_help, REQUIRED_RESUME_FLAGS.as_slice()),
    ] {
        let missing: Vec<&str> = required
            .iter()
            .copied()
            .filter(|flag| !super::advertises_flag(help, flag))
            .collect();
        if !missing.is_empty() {
            return Err(UpstrokeError::Agent {
                message: format!(
                    "codex {version} does not advertise required `{surface}` flag(s): {}. The \
                     reasoning override must work on both fresh and resumed attempts — upgrade \
                     upstroke or pin an older codex.",
                    missing.join(", ")
                ),
            });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum ConfigProbeSurface {
    Fresh,
    Resume,
}

impl ConfigProbeSurface {
    fn label(self) -> &'static str {
        match self {
            Self::Fresh => "exec",
            Self::Resume => "exec resume",
        }
    }

    const fn index(self) -> u32 {
        match self {
            Self::Fresh => 0,
            Self::Resume => 1,
        }
    }
}

struct MissingOutputSchema {
    dir: PathBuf,
    path: PathBuf,
}

impl MissingOutputSchema {
    fn create() -> Result<Self, UpstrokeError> {
        let dir = std::env::temp_dir().join(format!(
            "upstroke-codex-config-probe-{}",
            crate::ulid::ulid()
        ));
        std::fs::create_dir(&dir).map_err(|source| UpstrokeError::Agent {
            message: format!(
                "could not create Codex configuration probe directory `{}`: {source}",
                dir.display()
            ),
        })?;
        let path = dir.join(CONFIG_PROBE_SCHEMA_FILE);
        Ok(Self { dir, path })
    }
}

impl Drop for MissingOutputSchema {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.dir);
    }
}

fn validate_effort_config_key(
    runner: &dyn Runner,
    invocation: &Invocation,
    version: &str,
) -> Result<(), UpstrokeError> {
    let schema = MissingOutputSchema::create()?;
    for surface in [ConfigProbeSurface::Fresh, ConfigProbeSurface::Resume] {
        let base = probe_ordinal::CONFIG_BASE + surface.index() * probe_ordinal::CONFIG_PER_SURFACE;
        let control = run_config_parser_probe(
            runner,
            invocation,
            surface,
            &format!("{CONFIG_PROBE_UNKNOWN_KEY}=true"),
            &schema.path,
            base,
        )?;
        validate_unknown_config_control(version, surface, &control)?;

        for (step, effort) in [Effort::XHigh, Effort::Max].into_iter().enumerate() {
            let assignment = format!("model_reasoning_effort={}", effort_flag(effort));
            let output = run_config_parser_probe(
                runner,
                invocation,
                surface,
                &assignment,
                &schema.path,
                base + 1 + u32::try_from(step).unwrap_or(u32::MAX),
            )?;
            validate_effort_config_probe(version, surface, effort, &output)?;
        }
    }
    Ok(())
}

fn run_config_parser_probe(
    runner: &dyn Runner,
    invocation: &Invocation,
    surface: ConfigProbeSurface,
    assignment: &str,
    schema_path: &std::path::Path,
    ordinal: u32,
) -> Result<ProcessOutput, UpstrokeError> {
    runner.run(&probe_request(
        ADAPTER_ID,
        invocation.spec(&config_probe_args(surface, assignment, schema_path))?,
        ordinal,
        PROBE_TIMEOUT,
    )?)
}

fn config_probe_args(
    surface: ConfigProbeSurface,
    assignment: &str,
    schema_path: &std::path::Path,
) -> Vec<String> {
    let mut args = vec!["exec".to_owned()];
    if matches!(surface, ConfigProbeSurface::Resume) {
        args.extend(["resume".to_owned(), CONFIG_PROBE_RESUME_ID.to_owned()]);
    }
    args.extend([
        "--ignore-user-config".to_owned(),
        "--strict-config".to_owned(),
        "-c".to_owned(),
        assignment.to_owned(),
        "--output-schema".to_owned(),
        schema_path.to_string_lossy().into_owned(),
        "upstroke-config-parser-probe".to_owned(),
    ]);
    args
}

fn validate_unknown_config_control(
    version: &str,
    surface: ConfigProbeSurface,
    output: &ProcessOutput,
) -> Result<(), UpstrokeError> {
    if output.output_limited {
        return Err(UpstrokeError::Agent {
            message: format!(
                "codex {version} `{}` strict-config control exceeded the output limit; truncated output cannot prove local parser behavior",
                surface.label()
            ),
        });
    }
    let text = config_probe_text(output);
    let lower = text.to_ascii_lowercase();
    if !output.timed_out
        && output.code.is_some_and(|code| code != 0)
        && text.contains(CONFIG_PROBE_UNKNOWN_KEY)
        && (lower.contains("unknown") || lower.contains("unrecognized"))
        && !text.contains(CONFIG_PROBE_SCHEMA_FILE)
    {
        return Ok(());
    }
    Err(UpstrokeError::Agent {
        message: format!(
            "codex {version} `{}` did not reject the strict-config control before the local \
             missing-schema guard; exact reasoning-key support cannot be proven without spend \
             (exit {:?}, timeout {}, output: {})",
            surface.label(),
            output.code,
            output.timed_out,
            util::head(&text, 400)
        ),
    })
}

fn validate_effort_config_probe(
    version: &str,
    surface: ConfigProbeSurface,
    effort: Effort,
    output: &ProcessOutput,
) -> Result<(), UpstrokeError> {
    if output.output_limited {
        return Err(UpstrokeError::Agent {
            message: format!(
                "codex {version} `{}` reasoning-key probe exceeded the output limit; truncated output cannot prove `model_reasoning_effort={}`",
                surface.label(),
                effort_flag(effort)
            ),
        });
    }
    let text = config_probe_text(output);
    if !output.timed_out
        && output.code.is_some_and(|code| code != 0)
        && text.contains(CONFIG_PROBE_SCHEMA_FILE)
        && text.to_ascii_lowercase().contains("schema")
    {
        return Ok(());
    }
    Err(UpstrokeError::Agent {
        message: format!(
            "codex {version} `{}` did not accept exact local override \
             `model_reasoning_effort={}` before the zero-spend missing-schema guard (exit {:?}, \
             timeout {}, output: {})",
            surface.label(),
            effort_flag(effort),
            output.code,
            output.timed_out,
            util::head(&text, 400)
        ),
    })
}

fn config_probe_text(output: &ProcessOutput) -> String {
    format!("{}\n{}", output.stdout, output.stderr)
}

fn checked_model_catalog(program: &str, output: &ProcessOutput) -> Result<String, UpstrokeError> {
    if output.timed_out {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program} debug models` timed out; model effort support could not be verified"
            ),
        });
    }
    if output.code != Some(0) {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program} debug models` exited with {:?}: {}",
                output.code,
                output.stderr.trim()
            ),
        });
    }
    if output.stdout.trim().is_empty() {
        return Err(UpstrokeError::Agent {
            message: format!(
                "`{program} debug models` returned no catalog; model effort support could not be \
                 verified"
            ),
        });
    }
    Ok(output.stdout.clone())
}

fn parse_debug_models(text: &str) -> Result<DebugModels, UpstrokeError> {
    serde_json::from_str(text).map_err(|error| UpstrokeError::Agent {
        message: format!("`codex debug models` returned an unreadable catalog: {error}"),
    })
}

fn validate_model_efforts(version: &str, models: &DebugModels) -> Result<(), UpstrokeError> {
    for slug in catalog::known_models(ADAPTER_ID) {
        let model = models
            .models
            .iter()
            .find(|model| model.slug == slug)
            .ok_or_else(|| UpstrokeError::Agent {
                message: format!(
                    "codex {version}'s local model catalog does not contain known model `{slug}`; \
                     refusing before a configured `--model` fails at runtime"
                ),
            })?;
        let supported: Vec<Effort> = model
            .supported_reasoning_levels
            .iter()
            .filter_map(|level| Effort::parse(&level.effort))
            .collect();
        let missing: Vec<Effort> = Effort::ALL
            .into_iter()
            .filter(|effort| !supported.contains(effort))
            .collect();
        if !missing.is_empty() {
            return Err(UpstrokeError::Agent {
                message: format!(
                    "codex {version} model `{slug}` does not advertise required reasoning \
                     level(s): {}. Refusing before `model_reasoning_effort` can fail an attempt.",
                    missing
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
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

fn sandbox_mode(profile: &WorkerProfile) -> &'static str {
    match profile.permissions {
        PermissionMode::Edit => "workspace-write",
        PermissionMode::ReadOnly => "read-only",
    }
}

fn edit_refusal(profile: &WorkerProfile) -> Option<UpstrokeError> {
    (cfg!(windows) && profile.permissions == PermissionMode::Edit)
        .then(|| refuse_edit_profile(profile))
}

fn refuse_edit_profile(profile: &WorkerProfile) -> UpstrokeError {
    UpstrokeError::Refused {
        message: format!(
            "codex cannot run `{}` as an implementer on Windows: this CLI's sandbox is an \
             external helper that does not exist here (`codex doctor` reports `linux helper: \
             none`), so `codex exec` degrades to read-only — it accepts `--sandbox \
             workspace-write` and then writes nothing, with no error. Its only writing mode \
             (`--approve-for-me`) auto-approves writes anywhere on the filesystem, including \
             outside the repository, which §14's rollback cannot undo. Run codex under Linux \
             where its sandbox is enforced, or route implementation to another agent and keep \
             codex as a reviewer — its read-only sandbox works everywhere, and its different \
             model family is the point (§11.3).",
            profile.name
        ),
    }
}

pub fn build_args(run: &TaskRun) -> Vec<String> {
    let mut args = vec!["exec".to_owned()];
    if let Some(session) = &run.resume_session {
        args.push("resume".to_owned());
        args.push(session.clone());
    }
    args.push("--json".to_owned());
    args.push("--model".to_owned());
    args.push(run.profile.model.clone());
    if let Some(effort) = run.profile.effort {
        args.push("-c".to_owned());
        args.push(format!("model_reasoning_effort={}", effort_flag(effort)));
    }
    if run.resume_session.is_none() {
        args.push("--sandbox".to_owned());
        args.push(sandbox_mode(&run.profile).to_owned());
    }
    args.extend(run.profile.extra_args.iter().cloned());
    args.push("-".to_owned());
    args
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

    let mut message: Option<String> = None;
    let mut errors: Vec<String> = Vec::new();
    let mut usage: Option<Usage> = None;

    for line in out.stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(event): Result<Value, _> = serde_json::from_str(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("thread.started") => {
                outcome.session_id = event
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            Some("item.completed") => {
                let item = event.get("item");
                let is_message = item
                    .and_then(|i| i.get("type"))
                    .and_then(Value::as_str)
                    .is_some_and(|t| t == "agent_message");
                if is_message {
                    message = item
                        .and_then(|i| i.get("text"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
            }
            Some("turn.completed") => {
                usage = Some(add_usage(usage, event.get("usage")));
            }
            Some("error") => {
                if let Some(text) = event.get("message").and_then(Value::as_str) {
                    errors.push(text.to_owned());
                }
            }
            _ => {}
        }
    }
    outcome.usage = usage;

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

    if out.code == Some(0) {
        outcome.status = OutcomeStatus::Completed;
        outcome.detail = message;
        return outcome;
    }

    let joined = errors.join("\n");
    outcome.status = if looks_rate_limited(&joined) || looks_rate_limited(&out.stderr) {
        OutcomeStatus::RateLimited
    } else {
        OutcomeStatus::AgentError
    };
    outcome.detail = [
        (!joined.is_empty()).then(|| util::tail(&joined, 2000)),
        message,
        (!out.stderr.trim().is_empty()).then(|| util::tail(out.stderr.trim(), 2000)),
    ]
    .into_iter()
    .flatten()
    .next();
    outcome
}

fn add_usage(total: Option<Usage>, reported: Option<&Value>) -> Usage {
    let mut total = total.unwrap_or_default();
    let Some(reported) = reported else {
        return total;
    };
    let field = |name: &str| reported.get(name).and_then(Value::as_u64);
    let add = |slot: &mut Option<u64>, value: Option<u64>| {
        if let Some(value) = value {
            *slot = Some(slot.unwrap_or(0) + value);
        }
    };
    add(&mut total.input_tokens, field("input_tokens"));
    add(&mut total.output_tokens, field("output_tokens"));
    add(
        &mut total.cache_read_input_tokens,
        field("cached_input_tokens"),
    );
    add(
        &mut total.cache_creation_input_tokens,
        field("cache_write_input_tokens"),
    );
    add(
        &mut total.reasoning_output_tokens,
        field("reasoning_output_tokens"),
    );
    total.num_turns = Some(total.num_turns.unwrap_or(0) + 1);
    total
}

fn parse_login_status(out: &ProcessOutput) -> Discovery {
    let mut discovery = Discovery::unknown();
    if out.timed_out {
        return discovery.with_note("`codex login status` timed out; auth state unknown");
    }
    let text = format!("{}{}", out.stdout, out.stderr).to_ascii_lowercase();
    if text.contains("not logged in") || text.contains("not authenticated") {
        discovery.auth = AuthState::NotAuthenticated;
        return discovery.with_note("`codex login status` reports no stored credentials");
    }
    if !text.contains("logged in") {
        return discovery.with_note(format!(
            "`codex login status` said something this adapter does not recognise: {}",
            util::head(text.trim(), 120)
        ));
    }
    discovery.auth = AuthState::Authenticated;
    if text.contains("chatgpt") {
        discovery.shape = Some(PoolKind::SubscriptionWindow);
        discovery = discovery.with_note(
            "signed in through a ChatGPT plan, so this pool is a rate-limit window rather than \
             metered dollars",
        );
    } else if text.contains("api key") {
        discovery = discovery.with_note(
            "signed in with an API key; the pool kind below is a default rather than something \
             detected",
        );
    }
    discovery
}

const CLI: &str = "codex";

const INSTALL_HINT: &str = "Install the OpenAI Codex CLI there (`npm install -g @openai/codex`), or select a different \
     agent.";

fn cli() -> Invocation {
    Invocation::named(CLI)
}

impl AdapterSource for CodexAdapter {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        (id == ADAPTER_ID).then_some(self)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::ir::WorkerProfile;

    const FORBIDDEN: [&str; 4] = [
        "--dangerously-bypass-approvals-and-sandbox",
        "--dangerously-bypass-hook-trust",
        "danger-full-access",
        "--ephemeral",
    ];

    fn profile(permissions: PermissionMode) -> WorkerProfile {
        WorkerProfile {
            name: "small-gpt-5.6-sol".to_owned(),
            agent: ADAPTER_ID.to_owned(),
            model: "gpt-5.6-sol".to_owned(),
            pool: String::new(),
            permissions,
            effort: Some(Effort::Medium),
            max_turns: None,
            extra_args: Vec::new(),
        }
    }

    fn run(permissions: PermissionMode, resume: Option<&str>) -> TaskRun {
        TaskRun {
            prompt: "do the thing".to_owned(),
            profile: profile(permissions),
            workspace: PathBuf::from("/repo"),
            gate_cmds: vec!["cargo test".to_owned()],
            resume_session: resume.map(str::to_owned),
            settings_path: None,
        }
    }

    fn output(code: i32, stdout: &str, stderr: &str) -> ProcessOutput {
        ProcessOutput {
            code: Some(code),
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            timed_out: false,
            output_limited: false,
            duration: Duration::from_secs(1),
        }
    }

    fn debug_models_json(levels: &[&str]) -> String {
        json!({
            "models": [{
                "slug": "gpt-5.6-sol",
                "supported_reasoning_levels": levels
                    .iter()
                    .map(|effort| json!({ "effort": effort }))
                    .collect::<Vec<_>>(),
            }]
        })
        .to_string()
    }

    #[test]
    fn probe_contract_requires_reasoning_config_on_fresh_and_resume() {
        let fresh = "--json --sandbox --model -c, --config";
        let resumed = "--json --model -c, --config";
        validate_probe_contract("0.147.0", fresh, resumed).expect("complete surfaces");

        let error = validate_probe_contract(
            "0.147.0",
            "--json --sandbox --model --configuration",
            resumed,
        )
        .expect_err("fresh must carry reasoning config")
        .to_string();
        assert!(error.contains("`exec`"), "{error}");
        assert!(error.contains("--config"), "{error}");
        assert!(error.contains("-c"), "{error}");

        let error = validate_probe_contract("0.147.0", fresh, "--json --model --configuration")
            .expect_err("resume must carry reasoning config")
            .to_string();
        assert!(error.contains("`exec resume`"), "{error}");
        assert!(error.contains("--config"), "{error}");
        assert!(error.contains("-c"), "{error}");
    }

    #[test]
    fn exact_key_probe_uses_strict_local_guards_on_both_cli_surfaces() {
        let schema = std::path::Path::new(r"C:\missing\upstroke-output-schema-must-not-exist.json");
        for surface in [ConfigProbeSurface::Fresh, ConfigProbeSurface::Resume] {
            let args = config_probe_args(surface, "model_reasoning_effort=xhigh", schema);
            assert!(
                args.contains(&"--ignore-user-config".to_owned()),
                "{args:?}"
            );
            assert!(args.contains(&"--strict-config".to_owned()), "{args:?}");
            assert!(args.contains(&"--output-schema".to_owned()), "{args:?}");
            assert!(
                args.windows(2).any(|pair| {
                    pair == ["-c".to_owned(), "model_reasoning_effort=xhigh".to_owned()]
                }),
                "the exact provider key must reach argv: {args:?}"
            );
            assert!(
                !args.contains(&"--help".to_owned()),
                "help skips config parsing"
            );
            match surface {
                ConfigProbeSurface::Fresh => {
                    assert_eq!(args.first().map(String::as_str), Some("exec"));
                    assert!(!args.contains(&"resume".to_owned()), "{args:?}");
                }
                ConfigProbeSurface::Resume => assert_eq!(
                    &args[..3],
                    ["exec", "resume", CONFIG_PROBE_RESUME_ID],
                    "the resumed surface must be exercised independently"
                ),
            }
        }
    }

    #[test]
    fn strict_config_control_must_fail_before_the_missing_schema_guard() {
        let rejected = output(
            1,
            "",
            "error: unknown configuration key `upstroke_probe_deliberately_unknown`",
        );
        validate_unknown_config_control("0.147.0", ConfigProbeSurface::Fresh, &rejected)
            .expect("strict parsing is active");

        let skipped = output(
            1,
            "",
            "failed to read output schema upstroke-output-schema-must-not-exist.json",
        );
        let error =
            validate_unknown_config_control("0.147.0", ConfigProbeSurface::Resume, &skipped)
                .expect_err("an ignored unknown key proves nothing")
                .to_string();
        assert!(error.contains("strict-config control"), "{error}");
    }

    #[test]
    fn strict_config_evidence_rejects_output_limited_transcript() {
        let mut truncated = output(
            1,
            "",
            "error: unknown configuration key `upstroke_probe_deliberately_unknown`",
        );
        truncated.output_limited = true;
        let error =
            validate_unknown_config_control("0.147.0", ConfigProbeSurface::Fresh, &truncated)
                .expect_err("truncated parser evidence must fail closed")
                .to_string();
        assert!(error.contains("output limit"), "{error}");
        assert!(error.contains("truncated output"), "{error}");
    }

    #[test]
    fn exact_effort_key_must_reach_the_zero_spend_schema_guard() {
        let accepted = output(
            1,
            "",
            "error reading output schema C:\\missing\\upstroke-output-schema-must-not-exist.json",
        );
        for surface in [ConfigProbeSurface::Fresh, ConfigProbeSurface::Resume] {
            for effort in [Effort::XHigh, Effort::Max] {
                validate_effort_config_probe("0.147.0", surface, effort, &accepted)
                    .expect("the exact key and value passed strict local parsing");
            }
        }

        let unknown = output(
            1,
            "",
            "error: unknown configuration key `model_reasoning_effort`",
        );
        let error = validate_effort_config_probe(
            "0.147.0",
            ConfigProbeSurface::Fresh,
            Effort::XHigh,
            &unknown,
        )
        .expect_err("a renamed key must refuse before spend")
        .to_string();
        assert!(error.contains("model_reasoning_effort=xhigh"), "{error}");

        let mut timed_out = accepted;
        timed_out.timed_out = true;
        assert!(
            validate_effort_config_probe(
                "0.147.0",
                ConfigProbeSurface::Resume,
                Effort::Max,
                &timed_out,
            )
            .is_err(),
            "a timeout cannot be mistaken for parser evidence"
        );
    }

    #[test]
    fn effort_config_evidence_rejects_output_limited_transcript() {
        let mut truncated = output(
            1,
            "",
            "error reading output schema upstroke-output-schema-must-not-exist.json",
        );
        truncated.output_limited = true;
        let error = validate_effort_config_probe(
            "0.147.0",
            ConfigProbeSurface::Resume,
            Effort::Max,
            &truncated,
        )
        .expect_err("truncated reasoning-key evidence must fail closed")
        .to_string();
        assert!(error.contains("output limit"), "{error}");
        assert!(error.contains("model_reasoning_effort=max"), "{error}");
    }

    #[test]
    fn unreadable_fresh_or_resume_help_is_a_preflight_refusal() {
        let mut timed_out = output(0, "full help", "");
        timed_out.timed_out = true;
        let error = checked_help("codex", "exec", &timed_out)
            .expect_err("fresh timeout")
            .to_string();
        assert!(error.contains("exec --help"), "{error}");
        assert!(error.contains("could not be verified"), "{error}");

        let failed = output(2, "", "resume help failed");
        let error = checked_help("codex", "exec resume", &failed)
            .expect_err("resume nonzero")
            .to_string();
        assert!(error.contains("exec resume --help"), "{error}");
        assert!(error.contains("resume help failed"), "{error}");

        let empty = output(0, "", "");
        assert!(
            checked_help("codex", "exec resume", &empty)
                .expect_err("empty")
                .to_string()
                .contains("no output")
        );
    }

    #[test]
    fn model_catalog_requires_every_effort_for_each_known_codex_model() {
        let complete = debug_models_json(&["low", "medium", "high", "xhigh", "max", "ultra"]);
        let parsed = parse_debug_models(&complete).expect("realistic catalog");
        validate_model_efforts("0.147.0", &parsed).expect("all Upstroke levels are present");

        for (missing, levels) in [
            ("xhigh", ["low", "medium", "high", "max"]),
            ("max", ["low", "medium", "high", "xhigh"]),
        ] {
            let parsed = parse_debug_models(&debug_models_json(&levels)).expect("catalog");
            let error = validate_model_efforts("0.147.0", &parsed)
                .expect_err("a missing shared level must refuse")
                .to_string();
            assert!(error.contains("gpt-5.6-sol"), "{error}");
            assert!(error.contains(missing), "{error}");
        }

        let unrelated = serde_json::to_string(&json!({
            "models": [{
                "slug": "not-the-configured-model",
                "supported_reasoning_levels": [
                    { "effort": "low" },
                    { "effort": "medium" },
                    { "effort": "high" },
                    { "effort": "xhigh" },
                    { "effort": "max" },
                ],
            }]
        }))
        .expect("json");
        let parsed = parse_debug_models(&unrelated).expect("catalog");
        let error = validate_model_efforts("0.147.0", &parsed)
            .expect_err("another slug cannot satisfy the configured model")
            .to_string();
        assert!(error.contains("gpt-5.6-sol"), "{error}");
    }

    #[test]
    fn unreadable_model_catalog_is_a_preflight_refusal() {
        let mut timed_out = output(0, "{}", "");
        timed_out.timed_out = true;
        assert!(
            checked_model_catalog("codex", &timed_out)
                .expect_err("timeout")
                .to_string()
                .contains("could not be verified")
        );

        let failed = output(2, "", "not available");
        assert!(
            checked_model_catalog("codex", &failed)
                .expect_err("nonzero")
                .to_string()
                .contains("not available")
        );

        let empty = output(0, "", "");
        assert!(
            checked_model_catalog("codex", &empty)
                .expect_err("empty")
                .to_string()
                .contains("no catalog")
        );

        let malformed = checked_model_catalog("codex", &output(0, "not-json", ""))
            .and_then(|text| parse_debug_models(&text))
            .expect_err("malformed catalog")
            .to_string();
        assert!(malformed.contains("unreadable catalog"), "{malformed}");
    }

    #[test]
    fn a_fresh_attempt_sets_its_sandbox_and_a_resumed_one_must_not() {
        let fresh = build_args(&run(PermissionMode::Edit, None));
        assert!(fresh.starts_with(&["exec".to_owned()]), "{fresh:?}");
        assert!(!fresh.contains(&"resume".to_owned()), "{fresh:?}");
        assert!(fresh.contains(&"--sandbox".to_owned()), "{fresh:?}");
        assert!(fresh.contains(&"workspace-write".to_owned()), "{fresh:?}");

        let resumed = build_args(&run(PermissionMode::Edit, Some("019ff122-4d61")));
        assert_eq!(
            resumed[..3],
            [
                "exec".to_owned(),
                "resume".to_owned(),
                "019ff122-4d61".to_owned()
            ],
            "{resumed:?}"
        );
        assert!(
            !resumed.contains(&"--sandbox".to_owned()),
            "a resumed attempt must not re-specify the sandbox: {resumed:?}"
        );
    }

    #[test]
    fn every_effort_has_the_exact_config_spelling_on_fresh_and_resumed_attempts() {
        let expected = [
            (Effort::Low, "low"),
            (Effort::Medium, "medium"),
            (Effort::High, "high"),
            (Effort::XHigh, "xhigh"),
            (Effort::Max, "max"),
        ];
        for (effort, spelling) in expected {
            assert_eq!(effort_flag(effort), spelling);
            for resume in [None, Some("019ff122-4d61")] {
                let mut task = run(PermissionMode::Edit, resume);
                task.profile.effort = Some(effort);
                let args = build_args(&task);
                let expected = format!("model_reasoning_effort={spelling}");
                assert!(
                    args.windows(2)
                        .any(|window| window[0] == "-c" && window[1] == expected),
                    "{effort} must reach {:?} argv exactly: {args:?}",
                    resume
                );
            }
        }
    }

    #[test]
    fn a_profile_without_an_effort_passes_none_rather_than_guessing() {
        let mut run = run(PermissionMode::Edit, None);
        run.profile.effort = None;
        let args = build_args(&run);
        assert!(!args.contains(&"-c".to_owned()), "{args:?}");
    }

    #[test]
    fn the_prompt_is_the_last_argument_and_it_is_stdin() {
        for resume in [None, Some("sess")] {
            let args = build_args(&run(PermissionMode::ReadOnly, resume));
            assert_eq!(args.last().map(String::as_str), Some("-"), "{args:?}");
        }
        let run = run(PermissionMode::Edit, None);
        assert_eq!(CodexAdapter.stdin_payload(&run), "do the thing");
    }

    #[cfg(windows)]
    #[test]
    fn an_implementer_is_refused_where_no_sandbox_can_enforce_it() {
        let err = edit_refusal(&profile(PermissionMode::Edit))
            .expect("an implementer profile must be refused on Windows");
        let text = err.to_string();
        assert!(text.contains("cannot run"), "{text}");
        assert!(
            text.contains("--approve-for-me"),
            "the refusal has to say which door was tried and why it is shut: {text}"
        );
        assert!(text.contains("Linux"), "{text}");
        assert!(text.contains("reviewer"), "{text}");
    }

    #[cfg(not(windows))]
    #[test]
    fn an_implementer_is_allowed_where_the_sandbox_is_real() {
        assert!(
            edit_refusal(&profile(PermissionMode::Edit)).is_none(),
            "an implementer is fine where the sandbox is enforced"
        );
    }

    #[test]
    fn a_reviewer_is_read_only_and_nothing_is_ever_given_the_machine() {
        assert!(edit_refusal(&profile(PermissionMode::ReadOnly)).is_none());
        let args = build_args(&run(PermissionMode::ReadOnly, None));
        assert!(args.contains(&"read-only".to_owned()), "{args:?}");
        for permissions in [PermissionMode::Edit, PermissionMode::ReadOnly] {
            for resume in [None, Some("sess")] {
                let args = build_args(&run(permissions, resume)).join(" ");
                for flag in FORBIDDEN {
                    assert!(
                        !args.contains(flag),
                        "`{flag}` must never be passed: {args}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_successful_run_yields_its_session_message_and_tokens() {
        let stdout = r#"{"type":"thread.started","thread_id":"019ff122-4d61-7323-a217-843ddfe5932c"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_0","type":"command_execution"}}
{"type":"item.completed","item":{"id":"item_0","type":"command_execution"}}
{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"hi"}}
{"type":"turn.completed","usage":{"input_tokens":27707,"cached_input_tokens":22016,"cache_write_input_tokens":0,"output_tokens":102,"reasoning_output_tokens":0}}"#;
        let out = output(0, stdout, "some tracing noise");
        let outcome = parse_output(&out);

        assert_eq!(outcome.status, OutcomeStatus::Completed);
        assert_eq!(outcome.duration, out.duration);
        assert_eq!(
            outcome.session_id.as_deref(),
            Some("019ff122-4d61-7323-a217-843ddfe5932c"),
            "the thread id is what `exec resume` takes"
        );
        assert_eq!(outcome.detail.as_deref(), Some("hi"));

        let usage = outcome.usage.expect("usage");
        assert_eq!(usage.input_tokens, Some(27707));
        assert_eq!(usage.output_tokens, Some(102));
        assert_eq!(usage.cache_read_input_tokens, Some(22016));
        assert_eq!(usage.num_turns, Some(1));
        assert_eq!(outcome.cost_usd, None);
    }

    #[test]
    fn several_turns_are_summed_rather_than_last_wins() {
        let stdout = r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":2,"reasoning_output_tokens":1}}
{"type":"turn.completed","usage":{"input_tokens":30,"output_tokens":5,"reasoning_output_tokens":4}}"#;
        let usage = parse_output(&output(0, stdout, "")).usage.expect("usage");
        assert_eq!(usage.input_tokens, Some(40));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.reasoning_output_tokens, Some(5));
        assert_eq!(usage.num_turns, Some(2));
    }

    #[test]
    fn an_unauthenticated_run_is_an_agent_error_not_an_exhausted_pool() {
        let stdout = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"error","message":"Reconnecting... 2/5 (unexpected status 401 Unauthorized: Missing bearer or basic authentication in header)"}"#;
        let outcome = parse_output(&output(101, stdout, "ERROR codex_api::endpoint: 401"));
        assert_eq!(outcome.status, OutcomeStatus::AgentError);
        assert!(
            outcome.detail.as_deref().is_some_and(|d| d.contains("401")),
            "{:?}",
            outcome.detail
        );
    }

    #[test]
    fn a_rate_limited_failure_is_told_apart_from_an_ordinary_one() {
        let stdout =
            r#"{"type":"error","message":"You have hit your usage limit for this window"}"#;
        assert_eq!(
            parse_output(&output(1, stdout, "")).status,
            OutcomeStatus::RateLimited
        );
        let stdout = r#"{"type":"error","message":"the file could not be written"}"#;
        assert_eq!(
            parse_output(&output(1, stdout, "")).status,
            OutcomeStatus::AgentError
        );
    }

    #[test]
    fn junk_on_stdout_never_fails_an_attempt() {
        let stdout = "Reading additional input from stdin...\n\
                      {\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"ok\"}}\n\
                      {not json at all";
        let outcome = parse_output(&output(0, stdout, ""));
        assert_eq!(outcome.status, OutcomeStatus::Completed);
        assert_eq!(outcome.detail.as_deref(), Some("ok"));
    }

    #[test]
    fn signed_out_is_never_read_as_signed_in() {
        let signed_out = parse_login_status(&output(0, "Not logged in\n", ""));
        assert_eq!(signed_out.auth, AuthState::NotAuthenticated);
        assert_eq!(signed_out.shape, None);

        let signed_in = parse_login_status(&output(0, "Logged in using ChatGPT\n", ""));
        assert_eq!(signed_in.auth, AuthState::Authenticated);
        assert_eq!(
            signed_in.shape,
            Some(PoolKind::SubscriptionWindow),
            "a ChatGPT plan is a window, not metered dollars"
        );

        let odd = parse_login_status(&output(0, "something new entirely\n", ""));
        assert_eq!(odd.auth, AuthState::Unknown);
        assert!(!odd.notes.is_empty());
    }

    struct RecordingRunner {
        seen: std::sync::Mutex<Vec<crate::runner::RunnerRequest>>,
    }

    impl RecordingRunner {
        fn new() -> Self {
            Self {
                seen: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn programs(&self) -> Vec<String> {
            self.seen()
                .iter()
                .map(|request| request.command.program.clone())
                .collect()
        }

        fn seen(&self) -> Vec<crate::runner::RunnerRequest> {
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn identities(&self) -> Vec<String> {
            self.seen()
                .iter()
                .map(|request| request.invocation.render())
                .collect()
        }
    }

    impl Runner for RecordingRunner {
        fn run(
            &self,
            request: &crate::runner::RunnerRequest,
        ) -> Result<ProcessOutput, UpstrokeError> {
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            let args = request.command.args.join(" ");
            if args.contains(CONFIG_PROBE_UNKNOWN_KEY) {
                return Ok(output(
                    2,
                    "",
                    &format!("error: unknown key `{CONFIG_PROBE_UNKNOWN_KEY}` in -c override"),
                ));
            }
            if args.contains("model_reasoning_effort=") {
                return Ok(output(
                    2,
                    "",
                    &format!("error: output schema `{CONFIG_PROBE_SCHEMA_FILE}` does not exist"),
                ));
            }
            if args.contains("--version") {
                return Ok(output(0, "codex-cli 0.9.9\n", ""));
            }
            if args == "exec --help" {
                return Ok(output(0, "--json --sandbox --model -c, --config", ""));
            }
            if args == "exec resume --help" {
                return Ok(output(0, "--json --model -c, --config", ""));
            }
            if args == "debug models" {
                let models: Vec<_> = catalog::known_models(ADAPTER_ID)
                    .into_iter()
                    .map(|slug| {
                        json!({
                            "slug": slug,
                            "supported_reasoning_levels": Effort::ALL
                                .into_iter()
                                .map(|effort| json!({ "effort": effort.to_string() }))
                                .collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                return Ok(output(0, &json!({ "models": models }).to_string(), ""));
            }
            if args == "login status" {
                return Ok(output(0, "Logged in using ChatGPT\n", ""));
            }
            Ok(output(0, "", ""))
        }
    }

    #[test]
    fn the_six_config_parser_probes_are_six_distinct_identities() {
        let runner = RecordingRunner::new();
        let invocation = Invocation::at(if cfg!(windows) {
            r"C:\nowhere\codex.cmd"
        } else {
            "/nowhere/codex"
        });
        validate_effort_config_key(&runner, &invocation, "0.9.9")
            .expect("the scripted CLI satisfies every strict-config validator");

        let identities = runner.identities();
        assert_eq!(
            identities.len(),
            6,
            "two surfaces x {{control, xhigh, max}}: {identities:?}"
        );
        let distinct: BTreeSet<&String> = identities.iter().collect();
        assert_eq!(
            distinct.len(),
            6,
            "six processes carrying {} identities: {identities:?}",
            distinct.len()
        );
        assert!(
            identities
                .iter()
                .all(|id| id.starts_with("p.agent-codex.o")),
            "{identities:?}"
        );

        let resumed = runner
            .seen()
            .iter()
            .filter(|request| request.command.args.iter().any(|arg| arg == "resume"))
            .count();
        assert_eq!(resumed, 3, "three of the six probe the resumed surface");

        let declared: BTreeSet<u32> = probe_ordinal::ALL.into_iter().collect();
        let computed: BTreeSet<u32> = identities
            .iter()
            .map(|id| {
                id.rsplit_once(".o")
                    .and_then(|(_, ordinal)| ordinal.parse::<u32>().ok())
                    .expect("a probe identity ends in its ordinal")
            })
            .collect();
        assert_eq!(computed.len(), 6);
        assert!(
            computed.iter().all(|ordinal| declared.contains(ordinal)),
            "the six computed ordinals must be the six the table reserves: \
             computed {computed:?}, declared {declared:?}"
        );
    }

    #[test]
    fn preflight_starts_exactly_the_processes_the_ordinal_table_declares() {
        let runner = RecordingRunner::new();
        let caps = CodexAdapter
            .probe(&runner)
            .expect("the scripted boundary satisfies every pre-flight validator");
        assert_eq!(runner.identities().len(), 10, "probe's ten processes");
        CodexAdapter
            .discover(&runner, &caps)
            .expect("the scripted boundary answers discovery too");

        let identities = runner.identities();
        assert_eq!(
            identities.len(),
            12,
            "probe's ten and discovery's two: {identities:?}"
        );
        let distinct: BTreeSet<&String> = identities.iter().collect();
        assert_eq!(
            distinct.len(),
            identities.len(),
            "two pre-flight processes shared one identity: {identities:?}"
        );

        let declared: BTreeSet<u32> = probe_ordinal::ALL.into_iter().collect();
        let used: BTreeSet<u32> = identities
            .iter()
            .map(|id| {
                id.rsplit_once(".o")
                    .and_then(|(_, ordinal)| ordinal.parse().ok())
                    .expect("a probe identity ends in its ordinal")
            })
            .collect();
        assert_eq!(
            used, declared,
            "the ordinals pre-flight used and the ordinals the table declares differ"
        );
    }

    #[test]
    fn every_preflight_process_names_the_cli_at_whichever_boundary_is_asked() {
        let first = RecordingRunner::new();
        let second = RecordingRunner::new();
        let caps = CodexAdapter.probe(&first).expect("first boundary");
        CodexAdapter
            .discover(&first, &caps)
            .expect("first boundary discovery");
        let caps = CodexAdapter.probe(&second).expect("second boundary");
        CodexAdapter
            .discover(&second, &caps)
            .expect("second boundary discovery");

        let programs = first.programs();
        assert_eq!(programs.len(), 12);
        assert!(
            programs.iter().all(|program| program == "codex"),
            "a pre-flight process carried something other than the bare CLI name: {programs:?}"
        );
        assert_eq!(
            programs,
            second.programs(),
            "the second boundary in this process was asked something different from the first"
        );
    }

    #[test]
    fn every_preflight_process_has_its_own_ordinal() {
        use std::collections::BTreeSet;

        let ordinals: BTreeSet<u32> = probe_ordinal::ALL.into_iter().collect();
        assert_eq!(
            ordinals.len(),
            12,
            "`--version`, two `--help` surfaces, six strict-config parser probes, `debug models`, `login status`, and discovery's `debug models` — 12 processes, 12 identities"
        );
        assert_eq!(probe_ordinal::ALL.len(), 12);

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
        assert_eq!(ids.len(), 12);
        assert!(
            ids.iter().all(|id| id.starts_with("p.agent-codex.o")),
            "the probe form, naming this agent: {ids:?}"
        );
    }
    #[test]
    fn probe_against_real_binary_when_present() {
        if crate::util::find_program(CLI).is_none() {
            eprintln!("codex not on PATH; skipping live probe");
            return;
        }
        let caps = CodexAdapter
            .probe(&crate::runner::host::HostRunner::new())
            .expect("probe should succeed");
        assert!(caps.json_output);
        assert!(caps.session_resume);
        assert!(caps.model_list);
        assert!(!caps.version.is_empty());
    }
}
