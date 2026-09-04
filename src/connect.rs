//! Extended notes: `docs/internals/connect.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]

mod render;

use std::fs;
use std::path::PathBuf;

use crate::agent::{AdapterSource, BuiltinAdapters, Discovery};
use crate::capacity::{Pool, PoolKind, Source};
use crate::error::UpstrokeError;
use crate::util;

#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    pub pools_path: Option<PathBuf>,
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    Written,
    Unchanged,
    Refused,
}

#[derive(Debug)]
pub struct ConnectReport {
    pub path: PathBuf,
    pub outcome: Wrote,
    pub content: String,
    pub agents: Vec<AgentReport>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct AgentReport {
    pub agent: String,
    pub outcome: Result<Discovery, String>,
    pub pool: Option<Pool>,
}

pub fn run(opts: &ConnectOptions) -> Result<ConnectReport, UpstrokeError> {
    run_with(
        opts,
        &BuiltinAdapters,
        crate::agent::ADAPTERS.iter().map(|a| a.id()),
    )
}

pub fn run_with<'a>(
    opts: &ConnectOptions,
    adapters: &dyn AdapterSource,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<ConnectReport, UpstrokeError> {
    let path = match &opts.pools_path {
        Some(path) => path.clone(),
        None => util::user_upstroke_dir()
            .map(|dir| dir.join("pools.toml"))
            .ok_or_else(|| UpstrokeError::Refused {
                message:
                    "cannot find a home directory to write ~/.upstroke/pools.toml into — pass \
                          --pools <path> to say where it should go"
                        .to_owned(),
            })?,
    };

    let existing_text = fs::read_to_string(&path).ok();
    let carried = existing_text
        .as_deref()
        .map(operator_keys)
        .unwrap_or_default();

    let mut warnings = Vec::new();
    let mut agents: Vec<AgentReport> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let Some(adapter) = adapters.get(id) else {
            continue;
        };
        let runner = crate::runner::host::HostRunner::new();
        let discovered = adapter
            .probe(&runner)
            .and_then(|caps| adapter.discover(&runner, &caps));
        match discovered {
            Ok(discovery) => {
                let missing = crate::catalog::missing_from(id, &discovery.models);
                if !missing.is_empty() {
                    warnings.push(format!(
                        "{id} does not advertise catalogued model(s): {}. Cross-family review \
                         binds to catalogued names, so one this CLI rejects fails at runtime — \
                         upgrade upstroke or pin a model it lists.",
                        missing.join(", ")
                    ));
                }
                let mut pool = pool_for_agent(id, &discovery);
                if let Some(kept) = carried.get(&pool.name) {
                    kept.apply(&mut pool);
                }
                agents.push(AgentReport {
                    agent: id.to_owned(),
                    outcome: Ok(discovery),
                    pool: Some(pool),
                });
            }
            Err(error) => {
                warnings.push(format!("{id}: no pool written — {error}"));
                agents.push(AgentReport {
                    agent: id.to_owned(),
                    outcome: Err(error.to_string()),
                    pool: None,
                });
            }
        }
    }

    let content = render::pools_file(&agents);
    let existing = existing_text;
    let outcome = match &existing {
        Some(existing) if settings_of(existing) != settings_of(&content) && !opts.force => {
            Wrote::Refused
        }
        Some(existing) if stable_content(existing) == stable_content(&content) => Wrote::Unchanged,
        _ => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| UpstrokeError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            util::write_text(&path, &content)?;
            Wrote::Written
        }
    };

    Ok(ConnectReport {
        path,
        outcome,
        content,
        agents,
        warnings,
    })
}

fn settings_of(text: &str) -> Vec<String> {
    text.lines()
        .map(strip_comment)
        .filter(|line| !line.is_empty())
        .collect()
}

fn strip_comment(line: &str) -> String {
    let mut quoted = false;
    let mut out = String::new();
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                out.push(ch);
            }
            '#' if !quoted => break,
            _ => out.push(ch),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn stable_content(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| !line.starts_with("# Written by `upstroke connect`"))
        .collect()
}

fn pool_for_agent(agent: &str, discovery: &Discovery) -> Pool {
    let kind = discovery.shape.unwrap_or(match agent {
        "copilot" => PoolKind::Credits,
        _ => PoolKind::SubscriptionWindow,
    });
    Pool::discovered(
        default_pool_name(agent),
        kind,
        agent,
        vec![Source::Signals, Source::SelfMetered],
    )
}

#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize)]
struct OperatorKeys {
    profile: Option<String>,
    monthly_allowance: Option<toml::Value>,
    endpoint: Option<String>,
}

impl OperatorKeys {
    fn apply(&self, pool: &mut Pool) {
        if let Some(profile) = &self.profile {
            pool.profile = Some(profile.clone());
        }
        if let Some(endpoint) = &self.endpoint {
            pool.endpoint = Some(endpoint.clone());
        }
        if let Some(units) = self.monthly_allowance.as_ref().and_then(allowance_of) {
            pool.monthly_allowance = units;
        }
    }

    fn any(&self) -> bool {
        self.profile.is_some() || self.monthly_allowance.is_some() || self.endpoint.is_some()
    }
}

fn allowance_of(value: &toml::Value) -> Option<crate::capacity::Allowance> {
    match value {
        toml::Value::String(text) if text.trim().eq_ignore_ascii_case("auto") => {
            Some(crate::capacity::Allowance::Auto)
        }
        toml::Value::Integer(units) => Some(crate::capacity::Allowance::Units(*units as f64)),
        toml::Value::Float(units) => Some(crate::capacity::Allowance::Units(*units)),
        _ => None,
    }
}

fn operator_keys(text: &str) -> std::collections::BTreeMap<String, OperatorKeys> {
    #[derive(serde::Deserialize)]
    struct Doc {
        pools: Option<std::collections::BTreeMap<String, OperatorKeys>>,
    }
    toml::from_str::<Doc>(text)
        .ok()
        .and_then(|doc| doc.pools)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, keys)| keys.any())
        .collect()
}

fn default_pool_name(agent: &str) -> &str {
    agent
}

pub fn render_report(report: &ConnectReport) -> String {
    render::report(report)
}

impl ConnectReport {
    pub fn refused(&self) -> bool {
        self.outcome == Wrote::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentAdapter, AuthState, Caps, ProcessOutput, TaskRun};
    use crate::ir::Outcome;
    use crate::runner::CommandSpec;
    use std::path::Path;

    struct FakeAdapter {
        id: &'static str,
        discovery: Option<Discovery>,
    }

    impl AgentAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            self.id
        }

        fn probe(&self, _runner: &dyn crate::runner::Runner) -> Result<Caps, UpstrokeError> {
            if self.discovery.is_none() {
                return Err(UpstrokeError::Agent {
                    message: "binary not found on PATH".to_owned(),
                });
            }
            Ok(Caps {
                version: "0.0.0-fake".to_owned(),
                json_output: true,
                session_resume: true,
                cost_reporting: true,
                read_only_mode: true,
                acp: false,
                model_list: false,
            })
        }

        fn build(&self, _run: &TaskRun) -> Result<CommandSpec, UpstrokeError> {
            unreachable!("connect never spawns an attempt")
        }

        fn parse(&self, _out: &ProcessOutput) -> Result<Outcome, UpstrokeError> {
            unreachable!("connect never parses an attempt")
        }

        fn discover(
            &self,
            _runner: &dyn crate::runner::Runner,
            _caps: &Caps,
        ) -> Result<Discovery, UpstrokeError> {
            self.discovery.clone().ok_or_else(|| UpstrokeError::Agent {
                message: "binary not found on PATH".to_owned(),
            })
        }
    }

    struct Machine {
        adapters: Vec<FakeAdapter>,
    }

    impl AdapterSource for Machine {
        fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
            self.adapters
                .iter()
                .find(|a| a.id == id)
                .map(|a| a as &dyn AgentAdapter)
        }
    }

    fn machine() -> Machine {
        Machine {
            adapters: vec![
                FakeAdapter {
                    id: "claude-code",
                    discovery: Some(Discovery {
                        auth: AuthState::Authenticated,
                        models: Vec::new(),
                        shape: Some(PoolKind::SubscriptionWindow),
                        notes: vec!["auth method `subscription`".to_owned()],
                    }),
                },
                FakeAdapter {
                    id: "copilot",
                    discovery: None,
                },
            ],
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("upstroke-connect-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir.join("pools.toml")
    }

    fn connect(path: &Path, force: bool) -> ConnectReport {
        run_with(
            &ConnectOptions {
                pools_path: Some(path.to_path_buf()),
                force,
            },
            &machine(),
            ["claude-code", "copilot"],
        )
        .expect("connect runs")
    }

    #[test]
    fn a_missing_agent_skips_its_pool_without_taking_the_others_with_it() {
        let path = scratch("partial");
        let report = connect(&path, false);
        assert_eq!(report.outcome, Wrote::Written);
        let written = fs::read_to_string(&path).expect("file");
        assert!(written.contains("[pools.claude-code]"), "{written}");
        assert!(
            !written.contains("[pools.copilot]"),
            "no pool for a CLI that is not installed: {written}"
        );
        assert!(
            written.contains("# copilot: not usable"),
            "and it says why: {written}"
        );
        assert!(
            report.warnings.iter().any(|w| w.contains("copilot")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn what_connect_writes_parses_back_into_the_pools_it_describes() {
        let path = scratch("roundtrip");
        connect(&path, false);
        let mut warnings = Vec::new();
        let hermetic = path.parent().expect("parent").to_path_buf();
        let cfg = crate::config::load(None, &hermetic, Some(&path), &mut warnings)
            .expect("the written file parses");
        assert_eq!(cfg.pools.len(), 1);
        let pool = &cfg.pools[0];
        assert_eq!(pool.name, "claude-code");
        assert_eq!(pool.kind, PoolKind::SubscriptionWindow);
        assert_eq!(pool.agent, "claude-code");
        assert_eq!(pool.sources, [Source::Signals, Source::SelfMetered]);
        assert_eq!(pool.safety_margin, crate::capacity::DEFAULT_SAFETY_MARGIN);
        assert_eq!(pool.reserve, crate::capacity::DEFAULT_RESERVE);
        assert_eq!(pool.profile, None, "connect never invents a profile");
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn an_existing_file_that_differs_is_never_clobbered() {
        let path = scratch("clobber");
        let mine = "[pools.claude-code]\nkind = \"subscription-window\"\nagent = \
                    \"claude-code\"\nprofile = \"work\"\nmonthly_allowance = 300\n";
        fs::write(&path, mine).expect("hand-written file");

        let report = connect(&path, false);
        assert_eq!(report.outcome, Wrote::Refused);
        assert!(report.refused());
        assert_eq!(
            fs::read_to_string(&path).expect("still there"),
            mine,
            "the hand-written file is untouched"
        );
        let rendered = render_report(&report);
        assert!(rendered.contains("--force"), "{rendered}");
        assert!(
            rendered.contains("[pools.claude-code]"),
            "it shows what it would have written: {rendered}"
        );

        let forced = connect(&path, true);
        assert_eq!(forced.outcome, Wrote::Written);
        let after = fs::read_to_string(&path).expect("file");
        assert!(
            after.contains("profile = \"work\"") && after.contains("monthly_allowance = 300"),
            "--force keeps operator keys:
{after}"
        );
        assert!(
            after.contains("weekly = true"),
            "and still refreshes the rest:
{after}"
        );
    }

    #[test]
    fn re_connecting_an_unchanged_machine_reports_unchanged_rather_than_a_conflict() {
        let path = scratch("idempotent");
        connect(&path, false);
        let first = fs::read_to_string(&path).expect("file");

        let again = connect(&path, false);
        assert_eq!(again.outcome, Wrote::Unchanged, "{:?}", again.outcome);
        assert_eq!(
            fs::read_to_string(&path).expect("file"),
            first,
            "nothing changed, so nothing was rewritten — including the date it says it was written"
        );

        fs::write(&path, format!("# my own note\n{first}")).expect("annotate");
        assert_eq!(connect(&path, false).outcome, Wrote::Written);
    }

    #[test]
    fn a_login_between_connects_updates_the_file() {
        let path = scratch("relogin");
        let with = |auth: AuthState| Machine {
            adapters: vec![FakeAdapter {
                id: "claude-code",
                discovery: Some(Discovery {
                    auth,
                    models: Vec::new(),
                    shape: Some(PoolKind::SubscriptionWindow),
                    notes: Vec::new(),
                }),
            }],
        };
        let opts = |force| ConnectOptions {
            pools_path: Some(path.clone()),
            force,
        };
        run_with(
            &opts(false),
            &with(AuthState::NotAuthenticated),
            ["claude-code"],
        )
        .expect("first connect");
        assert!(
            fs::read_to_string(&path)
                .expect("file")
                .contains("NOT signed in"),
            "precondition"
        );

        let second = run_with(
            &opts(false),
            &with(AuthState::Authenticated),
            ["claude-code"],
        )
        .expect("second connect");
        assert_eq!(second.outcome, Wrote::Written, "the auth state changed");
        assert!(
            !fs::read_to_string(&path)
                .expect("file")
                .contains("NOT signed in"),
            "the file must not still say the operator is signed out:\n{}",
            fs::read_to_string(&path).expect("file")
        );
    }

    #[test]
    fn a_cli_that_lists_models_is_cross_checked_against_the_catalog() {
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "copilot",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,
                    models: [
                        "gpt-5-mini",
                        "gemini-3.1-pro",
                        "claude-sonnet-5",
                        "claude-opus-5",
                    ]
                    .map(str::to_owned)
                    .to_vec(),
                    shape: Some(PoolKind::Credits),
                    notes: Vec::new(),
                }),
            }],
        };
        let report = run_with(
            &ConnectOptions {
                pools_path: Some(scratch("crosscheck")),
                force: false,
            },
            &machine,
            ["copilot"],
        )
        .expect("connect runs");
        let warning = report
            .warnings
            .iter()
            .find(|w| w.contains("does not advertise"))
            .unwrap_or_else(|| panic!("expected a cross-check warning: {:?}", report.warnings));
        assert!(
            warning.contains("gpt-5.3-codex"),
            "names the frontier slug the second opinion depends on: {warning}"
        );
    }

    #[test]
    fn an_undetectable_plan_shape_takes_a_default_and_says_so() {
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "copilot",
                discovery: Some(Discovery::unknown().with_note("no auth query exists")),
            }],
        };
        let path = scratch("shape");
        let report = run_with(
            &ConnectOptions {
                pools_path: Some(path),
                force: false,
            },
            &machine,
            ["copilot"],
        )
        .expect("connect runs");
        assert!(
            report.content.contains("kind = \"credits\""),
            "{}",
            report.content
        );
        assert!(
            report.content.contains("kind below is a default"),
            "the default is visible in the file: {}",
            report.content
        );
        assert!(
            report
                .content
                .contains("auth state could not be determined"),
            "unknown auth never renders as 'not connected': {}",
            report.content
        );
    }

    #[test]
    fn discovery_against_the_real_claude_binary_when_present() {
        let runner = crate::runner::host::HostRunner::new();
        let Ok(caps) = crate::agent::claude::ClaudeCodeAdapter.probe(&runner) else {
            eprintln!("skipped: no claude on PATH");
            return;
        };
        let discovery = crate::agent::claude::ClaudeCodeAdapter
            .discover(&runner, &caps)
            .expect("discovery never fails on a CLI that probes");
        assert!(
            !discovery.notes.is_empty(),
            "discovery always says how it worked it out"
        );
        assert!(
            discovery.models.is_empty() || caps.model_list,
            "models may only be reported by a CLI whose --help advertises listing"
        );
    }
}
