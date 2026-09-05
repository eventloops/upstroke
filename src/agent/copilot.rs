//! Extended notes: `docs/internals/agent/copilot.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]

use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use super::bin::{self, Invocation};
use super::proc::ProcessOutput;
use super::{AgentAdapter, Caps, Discovery, TaskRun, looks_rate_limited, probe_request};
use crate::error::UpstrokeError;
use crate::ir::{Effort, Outcome, OutcomeStatus, PermissionMode, WorkerProfile};
use crate::runner::{CommandSpec, Runner};
use crate::util;

pub const ADAPTER_ID: &str = "copilot";

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

const REQUIRED_FLAGS: [&str; 5] = [
    "--model",
    "--effort",
    "--allow-tool",
    "--deny-tool",
    "--no-ask-user",
];

const REQUIRED_SHORT_FLAGS: [&str; 1] = ["-s"];

mod probe_ordinal {
    pub const VERSION: u32 = 0;
    pub const HELP: u32 = 1;
    #[cfg(test)]
    pub const ALL: [u32; 2] = [VERSION, HELP];
}

pub struct CopilotAdapter;

impl AgentAdapter for CopilotAdapter {
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
            json_output: false,
            session_resume: has("--resume"),
            cost_reporting: false,
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

    fn discover(&self, _runner: &dyn Runner, caps: &Caps) -> Result<Discovery, UpstrokeError> {
        let invocation = cli();
        let mut discovery = Discovery::unknown().with_note(format!(
            "`{}` reports no non-interactive auth state, so whether this account is signed in \
             could not be checked without spending",
            invocation.display()
        ));
        if !caps.model_list {
            discovery.notes.push(
                "and no model listing either, so the roster for this agent is the catalog \
                 shipped with upstroke, not something confirmed here"
                    .to_owned(),
            );
        }
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
        util::write_json(
            &path,
            &json!({
                "agent": ADAPTER_ID,
                "profile": profile.name,
                "permissions": profile.permissions,
                "note": "recorded for audit only; copilot takes permissions as argv flags",
                "args": permission_args(profile, gate_cmds),
            }),
        )?;
        Ok(None)
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
                "copilot {version} does not advertise required flag(s): {}. This adapter pins \
                 known-good behavior per version — upgrade upstroke or pin an older copilot.",
                missing_flags.join(", ")
            ),
        });
    }
    let missing_efforts = super::missing_effort_levels(help);
    if !missing_efforts.is_empty() {
        return Err(UpstrokeError::Agent {
            message: format!(
                "copilot {version} advertises `--effort` but not required level(s): {}. Refusing \
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
        "-s".to_owned(),
        "--no-ask-user".to_owned(),
        format!("--model={}", run.profile.model),
    ];
    if let Some(effort) = run.profile.effort {
        args.push(format!("--effort={}", effort_flag(effort)));
    }
    args.extend(permission_args(&run.profile, &run.gate_cmds));
    if let Some(session) = &run.resume_session {
        args.push(format!("--resume={session}"));
    }
    args.extend(run.profile.extra_args.iter().cloned());
    args
}

pub fn permission_args(profile: &WorkerProfile, gate_cmds: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    match profile.permissions {
        PermissionMode::Edit => {
            args.push("--allow-tool=write".to_owned());
            for gate in gate_cmds {
                args.push(format!("--allow-tool=shell({gate})"));
            }
        }
        PermissionMode::ReadOnly => {
            args.push("--deny-tool=write".to_owned());
            args.push("--deny-tool=shell".to_owned());
        }
    }
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

    let response = out.stdout.trim();
    if out.code == Some(0) {
        outcome.status = OutcomeStatus::Completed;
        outcome.detail = (!response.is_empty()).then(|| response.to_owned());
        return outcome;
    }

    outcome.status = if looks_rate_limited(&out.stderr) || looks_rate_limited(response) {
        OutcomeStatus::RateLimited
    } else {
        OutcomeStatus::AgentError
    };
    let stderr = out.stderr.trim();
    outcome.detail = if !stderr.is_empty() {
        Some(util::tail(stderr, 2000))
    } else if !response.is_empty() {
        Some(util::tail(response, 2000))
    } else {
        None
    };
    outcome
}

const CLI: &str = "copilot";

const INSTALL_HINT: &str = "Install the GitHub Copilot CLI there (`npm install -g @github/copilot`), or select a \
     different agent.";

fn cli() -> Invocation {
    Invocation::named(CLI)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKIP_ALL_FLAGS: [&str; 6] = [
        "--allow-all",
        "--yolo",
        "--allow-all-tools",
        "--allow-all-paths",
        "--allow-all-urls",
        "--allow-url",
    ];

    fn profile(permissions: PermissionMode) -> WorkerProfile {
        WorkerProfile {
            name: "impl-frontier".to_owned(),
            agent: ADAPTER_ID.to_owned(),
            model: "gpt-5.3-codex".to_owned(),
            pool: "copilot".to_owned(),
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
            gate_cmds: vec!["cargo test".to_owned()],
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
    fn every_preflight_process_has_its_own_ordinal() {
        use std::collections::BTreeSet;

        let ordinals: BTreeSet<u32> = probe_ordinal::ALL.into_iter().collect();
        assert_eq!(
            ordinals.len(),
            2,
            "`--version` and `--help`; this CLI answers no auth query — 2 processes, 2 identities"
        );
        assert_eq!(probe_ordinal::ALL.len(), 2);

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
        assert_eq!(ids.len(), 2);
        assert!(
            ids.iter().all(|id| id.starts_with("p.agent-copilot.o")),
            "the probe form, naming this agent: {ids:?}"
        );
    }

    #[test]
    fn build_args_cover_the_programmatic_contract() {
        let joined = build_args(&task_run()).join(" ");
        assert!(joined.contains("-s"), "response only: {joined}");
        assert!(joined.contains("--no-ask-user"));
        assert!(joined.contains("--model=gpt-5.3-codex"));
        assert!(joined.contains("--effort=medium"));
        assert!(!joined.contains("--resume"), "no session to resume");
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
            assert!(
                build_args(&run)
                    .iter()
                    .any(|arg| arg == &format!("--effort={spelling}")),
                "{effort} must reach argv as {spelling}"
            );
        }
    }

    #[test]
    fn help_validation_requires_every_shared_effort_level() {
        let help = "-s --model --allow-tool --deny-tool --no-ask-user\n  \
                    --effort, --reasoning-effort <level> (choices: \"none\", \"minimal\", \
                    \"low\", \"medium\", \"high\", \"xhigh\", \"max\")\n";
        validate_help("1.0.78", help).expect("full shared vocabulary");

        for (missing, narrowed) in [
            ("xhigh", "none, minimal, low, medium, high, max"),
            ("max", "none, minimal, low, medium, high, xhigh"),
        ] {
            let help = format!(
                "-s --model --allow-tool --deny-tool --no-ask-user\n  --effort <level> \
                 (choices: {narrowed})\n"
            );
            let error = validate_help("1.0.78", &help).expect_err("narrow enum must refuse");
            let message = error.to_string();
            assert!(message.contains(missing), "{message}");
            assert!(message.contains("1.0.78"), "{message}");
        }
    }

    #[test]
    fn unreadable_help_is_a_preflight_refusal() {
        let mut timed_out = output(Some(0), "full help", "");
        timed_out.timed_out = true;
        assert!(
            checked_help("copilot", &timed_out)
                .expect_err("timeout")
                .to_string()
                .contains("could not be verified")
        );

        let failed = output(Some(2), "", "bad option");
        assert!(
            checked_help("copilot", &failed)
                .expect_err("nonzero")
                .to_string()
                .contains("bad option")
        );

        let empty = output(Some(0), "", "");
        assert!(
            checked_help("copilot", &empty)
                .expect_err("empty")
                .to_string()
                .contains("no output")
        );
    }

    #[test]
    fn the_prompt_travels_on_stdin_and_never_as_an_argument() {
        let args = build_args(&task_run());
        assert!(
            !args.iter().any(|a| a == "-p" || a.starts_with("--prompt")),
            "`-p` would discard the piped prompt: {args:?}"
        );
        let run = task_run();
        assert_eq!(
            CopilotAdapter.stdin_payload(&run),
            "Do the thing.",
            "the prompt is delivered on stdin"
        );
    }

    #[test]
    fn edit_profiles_get_write_and_exactly_the_gate_commands() {
        let gates = vec![
            "cargo check --all-targets".to_owned(),
            "cargo test".to_owned(),
        ];
        let args = permission_args(&profile(PermissionMode::Edit), &gates);
        assert!(args.contains(&"--allow-tool=write".to_owned()));
        assert!(args.contains(&"--allow-tool=shell(cargo test)".to_owned()));
        assert!(args.contains(&"--allow-tool=shell(cargo check --all-targets)".to_owned()));
        assert!(
            !args.iter().any(|a| a == "--allow-tool=shell"),
            "no blanket shell: {args:?}"
        );
    }

    #[test]
    fn reviewers_may_neither_write_nor_run_anything() {
        let args = permission_args(
            &profile(PermissionMode::ReadOnly),
            &["cargo test".to_owned()],
        );
        assert!(args.contains(&"--deny-tool=write".to_owned()));
        assert!(args.contains(&"--deny-tool=shell".to_owned()));
        assert!(
            !args.iter().any(|a| a.starts_with("--allow-tool")),
            "reviewers are granted nothing: {args:?}"
        );
    }

    #[test]
    fn no_profile_is_ever_handed_the_whole_machine() {
        for permissions in [PermissionMode::Edit, PermissionMode::ReadOnly] {
            let mut run = task_run();
            run.profile = profile(permissions);
            let joined = build_args(&run).join(" ");
            for flag in SKIP_ALL_FLAGS {
                assert!(
                    !joined.contains(flag),
                    "{permissions:?} must never carry {flag}: {joined}"
                );
            }
        }
    }

    #[test]
    fn the_short_flag_check_is_not_fooled_by_longer_flags() {
        assert!(!crate::agent::advertises_flag(
            "--settings <path>  --share <path>  --stdio",
            "-s"
        ));
        assert!(crate::agent::advertises_flag(
            "  -s, --silent    Suppress stats",
            "-s"
        ));
        assert!(crate::agent::advertises_flag(
            "  -s  Suppress stats and decoration",
            "-s"
        ));
        assert!(
            crate::agent::advertises_flag("-s=VALUE", "-s"),
            "trailing = is a value marker"
        );
        assert!(!crate::agent::advertises_flag("", "-s"));
    }

    #[test]
    fn a_turn_cap_is_not_quietly_pretended_to_apply() {
        let mut run = task_run();
        run.profile.max_turns = Some(7);
        let joined = build_args(&run).join(" ");
        assert!(
            !joined.contains("max-turns") && !joined.contains('7'),
            "no invented flag, and no silent substitution: {joined}"
        );
    }

    #[test]
    fn extra_args_are_appended_last() {
        let mut run = task_run();
        run.profile.extra_args = vec!["--add-dir=/srv/shared".to_owned()];
        assert!(
            build_args(&run)
                .join(" ")
                .ends_with("--add-dir=/srv/shared")
        );
    }

    #[test]
    fn a_successful_run_carries_its_response_as_the_detail() {
        let verdict = "```json\n{\"pass\": true, \"reasons\": [\"ok\"]}\n```";
        let out = output(Some(0), &format!("  {verdict}  \n"), "");
        let outcome = parse_output(&out);
        assert_eq!(outcome.status, OutcomeStatus::Completed);
        assert_eq!(outcome.detail.as_deref(), Some(verdict));
        assert!(outcome.diff.is_empty(), "diff is engine-owned");
        assert_eq!(outcome.duration, out.duration);
    }

    #[test]
    fn unreported_spend_is_none_rather_than_zero() {
        let outcome = parse_output(&output(Some(0), "done", ""));
        assert_eq!(outcome.cost_usd, None);
        assert_eq!(outcome.session_id, None);
        assert!(outcome.usage.is_none());
    }

    #[test]
    fn failures_carry_a_reportable_detail() {
        let outcome = parse_output(&output(
            Some(1),
            "",
            "error: model `gpt-9` is not available",
        ));
        assert_eq!(outcome.status, OutcomeStatus::AgentError);
        assert_eq!(
            outcome.detail.as_deref(),
            Some("error: model `gpt-9` is not available")
        );

        let outcome = parse_output(&output(Some(1), "I could not finish.", ""));
        assert_eq!(outcome.detail.as_deref(), Some("I could not finish."));

        let outcome = parse_output(&output(Some(1), "", ""));
        assert_eq!(outcome.status, OutcomeStatus::AgentError);
        assert!(outcome.detail.is_none());
    }

    #[test]
    fn rate_limit_signals_win_over_exit_codes() {
        let outcome = parse_output(&output(
            Some(1),
            "",
            "You are out of credits for this month",
        ));
        assert_eq!(outcome.status, OutcomeStatus::RateLimited);

        let outcome = parse_output(&output(Some(1), "premium request allowance exhausted", ""));
        assert_eq!(outcome.status, OutcomeStatus::RateLimited);
    }

    #[test]
    fn a_successful_task_about_rate_limits_is_not_rate_limited() {
        let outcome = parse_output(&output(
            Some(0),
            "Added backoff handling for HTTP 429 rate limit responses.",
            "",
        ));
        assert_eq!(outcome.status, OutcomeStatus::Completed);
    }

    #[test]
    fn timeout_maps_to_timeout_status() {
        let mut out = output(None, "", "");
        out.timed_out = true;
        assert_eq!(parse_output(&out).status, OutcomeStatus::Timeout);
    }

    #[test]
    fn probe_against_real_binary_when_present() {
        if crate::util::find_program(CLI).is_none() {
            eprintln!("copilot not on PATH; skipping live probe");
            return;
        }
        let caps = CopilotAdapter
            .probe(&crate::runner::host::HostRunner::new())
            .expect("probe should succeed");
        assert!(!caps.version.is_empty());
        assert!(!caps.cost_reporting, "this route reports no spend");
    }
}
