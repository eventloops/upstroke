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

    let existing_text = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpstrokeError::Io { path, source }),
    };
    let mut carried = existing_text
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
                if let Some(kept) = carried.remove(&pool.name) {
                    if let Err(error) = kept.apply(&mut pool) {
                        warnings.push(format!("pool `{}`: {error}; using auto", pool.name));
                    }
                }
                agents.push(AgentReport {
                    agent: id.to_owned(),
                    outcome: Ok(discovery),
                    pool: Some(pool),
                });
            }
            Err(error) => {
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
        Some(existing) if !settings_match(existing, &content) && !opts.force => Wrote::Refused,
        Some(existing)
            if toml::from_str::<toml::Table>(existing).is_ok()
                && stable_content(existing) == stable_content(&content) =>
        {
            Wrote::Unchanged
        }
        _ => {
            write_pools(&path, &content)?;
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

fn write_pools(path: &std::path::Path, content: &str) -> Result<(), UpstrokeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| UpstrokeError::Filesystem {
            operation: "create directory",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, content).map_err(|source| UpstrokeError::Filesystem {
        operation: "write pools file",
        path: path.to_path_buf(),
        source,
    })
}

fn settings_match(existing: &str, proposed: &str) -> bool {
    match (
        toml::from_str::<toml::Table>(existing),
        toml::from_str::<toml::Table>(proposed),
    ) {
        (Ok(existing), Ok(proposed)) => existing == proposed,
        (Err(_), _) | (_, Err(_)) => false,
    }
}

fn stable_content(text: &str) -> Vec<&str> {
    text.lines()
        .enumerate()
        .filter(|(index, line)| *index != 0 || !line.starts_with(render::WRITTEN_BY))
        .map(|(_, line)| line)
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

#[derive(Debug, Default, PartialEq, serde::Deserialize)]
struct OperatorKeys {
    profile: Option<String>,
    monthly_allowance: Option<toml::Value>,
    endpoint: Option<String>,
}

impl OperatorKeys {
    fn apply(self, pool: &mut Pool) -> Result<(), InvalidAllowance> {
        if let Some(profile) = self.profile {
            pool.profile = Some(profile);
        }
        if let Some(endpoint) = self.endpoint {
            pool.endpoint = Some(endpoint);
        }
        if let Some(value) = self.monthly_allowance {
            pool.monthly_allowance = allowance_of(&value)?;
        }
        Ok(())
    }

    fn any(&self) -> bool {
        self.profile.is_some() || self.monthly_allowance.is_some() || self.endpoint.is_some()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("monthly_allowance must be a positive finite number or the string \"auto\"")]
struct InvalidAllowance;

fn allowance_of(value: &toml::Value) -> Result<crate::capacity::Allowance, InvalidAllowance> {
    match value {
        toml::Value::String(text) if text.trim().eq_ignore_ascii_case("auto") => {
            Ok(crate::capacity::Allowance::Auto)
        }
        toml::Value::Integer(units) if *units > 0 => {
            Ok(crate::capacity::Allowance::Units(*units as f64))
        }
        toml::Value::Float(units) if units.is_finite() && *units > 0.0 => {
            Ok(crate::capacity::Allowance::Units(*units))
        }
        _ => Err(InvalidAllowance),
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
    fn quoted_pool_renames_preserve_every_string_byte_without_force() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-quoted")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "x y",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,
                    models: Vec::new(),
                    shape: Some(PoolKind::Credits),
                    notes: Vec::new(),
                }),
            }],
        };
        let options = ConnectOptions {
            pools_path: Some(path.clone()),
            force: false,
        };
        let first = run_with(&options, &machine, ["x y"]).expect("generate a quoted pool key");
        assert_eq!(first.outcome, Wrote::Written);
        for header in [
            "[pools.\"x  y\"]",
            "[pools.\"x\ty\"]",
            "[pools.\"x\u{a0}y\"]",
            "[pools.'x  y']",
            "[pools.'x\ty']",
            "[pools.\" x y\"]",
            "[pools.\"x y \"]",
        ] {
            let renamed = first.content.replace("[pools.\"x y\"]", header);
            assert_ne!(renamed, first.content, "the header replacement must apply");
            let parsed: toml::Table = toml::from_str(&renamed).expect("a valid TOML rename");
            assert!(parsed.get("pools").is_some(), "the renamed pool exists");
            fs::write(&path, &renamed).expect("write the operator's rename");
            let next = run_with(&options, &machine, ["x y"]).expect("compare the renamed pool");
            assert_eq!(next.outcome, Wrote::Refused, "header {header:?}");
            assert_eq!(
                fs::read_to_string(&path).expect("read after refusal"),
                renamed
            );
        }
    }

    #[test]
    fn review_168_force_replaces_a_malformed_generated_header() {
        let tree = crate::rundir::scratch_tree::acquire(
            &std::env::temp_dir(),
            "review-168-malformed-header",
        )
        .expect("acquire isolated review fixture");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        let (header, rest) = first
            .content
            .split_once('\n')
            .expect("generated file has a first header line");
        let malformed = format!("{header}\u{1}\n{rest}");
        assert!(toml::from_str::<toml::Table>(&malformed).is_err());
        fs::write(&path, &malformed).expect("corrupt only the generated header");
        assert_eq!(connect(&path, false).outcome, Wrote::Refused);
        assert_eq!(
            fs::read_to_string(&path).expect("read refusal bytes"),
            malformed
        );

        let forced = connect(&path, true);
        let persisted = fs::read_to_string(&path).expect("read forced result");
        let mut warnings = Vec::new();
        let loaded = crate::config::load(None, tree.path(), Some(&path), &mut warnings);
        assert_eq!(
            forced.outcome,
            Wrote::Written,
            "--force must repair malformed TOML; original bytes retained: {}; config reader: {loaded:?}",
            persisted == malformed,
        );
        assert!(loaded.is_ok(), "the repaired file must load: {loaded:?}");
        assert_ne!(persisted, malformed);
    }

    #[test]
    fn equivalent_toml_spellings_refresh_without_force() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-spelling")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        for (from, to) in [
            ("reserve = 0.20", "reserve = 0.2"),
            ("[pools.claude-code]", "[pools.'claude-code']"),
            ("[\"signals\", \"self\"]", "[ 'signals' ,\n 'self',\n]"),
            ("agent = \"claude-code\"", "agent = '''claude-code'''"),
        ] {
            let edited = first.content.replace(from, to);
            assert_ne!(edited, first.content, "the spelling change must apply");
            fs::write(&path, edited).expect("write an equivalent TOML spelling");
            assert_eq!(connect(&path, false).outcome, Wrote::Written, "{to}");
            assert_eq!(connect(&path, false).outcome, Wrote::Unchanged, "{to}");
        }
    }

    #[test]
    fn a_changed_multiline_string_is_refused_without_force() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-multiline")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        for spelling in ["'''claude-\ncode'''", "\"\"\"claude-\ncode\"\"\""] {
            let edited = first
                .content
                .replace("agent = \"claude-code\"", &format!("agent = {spelling}"));
            assert_ne!(edited, first.content, "the multiline change must apply");
            toml::from_str::<toml::Table>(&edited).expect("valid multiline TOML string");
            fs::write(&path, &edited).expect("write a changed multiline string");
            assert_eq!(connect(&path, false).outcome, Wrote::Refused);
            assert_eq!(
                fs::read_to_string(&path).expect("read after refusal"),
                edited
            );
        }
    }

    #[test]
    fn an_old_integer_allowance_requires_force_once_when_the_writer_uses_a_float() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-integer")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);

        let old = first.content.replace(
            "agent = \"claude-code\"",
            "agent = \"claude-code\"\nmonthly_allowance = 10000000000000000",
        );
        assert_ne!(old, first.content, "insert the previous allowance spelling");
        fs::write(&path, &old).expect("write the old renderer's integer spelling");
        assert_eq!(connect(&path, false).outcome, Wrote::Refused);
        assert_eq!(fs::read_to_string(&path).expect("read after refusal"), old);
        assert_eq!(connect(&path, true).outcome, Wrote::Written);
        let mut warnings = Vec::new();
        let config = crate::config::load(None, tree.path(), Some(&path), &mut warnings)
            .expect("the real config reader accepts the rewritten allowance");
        assert_eq!(
            config.pools.first().expect("one pool").monthly_allowance,
            crate::capacity::Allowance::Units(1e16)
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(connect(&path, false).outcome, Wrote::Unchanged);
    }

    #[test]
    fn force_rejects_invalid_allowances_and_preserves_other_operator_keys() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-allowance")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        for allowance in [
            "nan", "inf", "-inf", "0", "-1", "0.0", "-0.0", "-1.5", "true", "'bad'",
        ] {
            let existing = format!(
                "[pools.claude-code]\nkind = 'subscription-window'\nagent = 'claude-code'\n\
                 profile = 'work'\nendpoint = 'http://host/#work'\nmonthly_allowance = {allowance}\n"
            );
            fs::write(&path, &existing).expect("write an invalid allowance with other valid keys");
            let mut warnings = Vec::new();
            assert!(
                crate::config::load(None, tree.path(), Some(&path), &mut warnings).is_err(),
                "the real reader rejects {allowance}"
            );
            assert_eq!(connect(&path, false).outcome, Wrote::Refused);
            assert_eq!(
                fs::read_to_string(&path).expect("read after refusal"),
                existing
            );
            let forced = connect(&path, true);
            assert_eq!(forced.outcome, Wrote::Written);
            let mut warnings = Vec::new();
            let config = crate::config::load(None, tree.path(), Some(&path), &mut warnings)
                .expect("the real reader accepts what --force wrote");
            let pool = config.pools.first().expect("one pool");
            assert_eq!(pool.monthly_allowance, crate::capacity::Allowance::Auto);
            assert_eq!(pool.profile.as_deref(), Some("work"));
            assert_eq!(pool.endpoint.as_deref(), Some("http://host/#work"));
            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(
                forced
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("monthly_allowance")),
                "the rejection of {allowance} is visible: {:?}",
                forced.warnings
            );
        }
    }

    #[test]
    fn force_keeps_positive_allowances_the_config_reader_accepts() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-valid-allowance")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        for (spelling, expected) in [
            ("1e-300", 1e-300),
            ("0.5", 0.5),
            ("300", 300.0),
            ("1e16", 1e16),
            ("1e300", 1e300),
        ] {
            fs::write(
                &path,
                format!("[pools.claude-code]\nmonthly_allowance = {spelling}\n"),
            )
            .expect("write a valid allowance");
            let report = connect(&path, true);
            assert_eq!(report.outcome, Wrote::Written);
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
            let mut warnings = Vec::new();
            let config = crate::config::load(None, tree.path(), Some(&path), &mut warnings)
                .expect("the real reader accepts the carried allowance");
            assert_eq!(
                config.pools.first().expect("one pool").monthly_allowance,
                crate::capacity::Allowance::Units(expected),
                "{spelling}"
            );
            assert!(warnings.is_empty(), "{warnings:?}");
        }
    }

    #[test]
    fn invalid_toml_in_a_comment_is_refused_without_force() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-comment")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        let invalid = format!("# operator note with \u{1} control\n{}", first.content);
        assert!(toml::from_str::<toml::Table>(&invalid).is_err());
        fs::write(&path, &invalid).expect("write a malformed comment");
        assert_eq!(connect(&path, false).outcome, Wrote::Refused);
        assert_eq!(
            fs::read_to_string(&path).expect("read after refusal"),
            invalid
        );
        assert_eq!(connect(&path, true).outcome, Wrote::Written);
    }

    #[test]
    fn a_different_header_timestamp_is_unchanged_and_keeps_the_existing_bytes() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-timestamp")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        let old = render::pools_file_at(&first.agents, "1900-01-01T00:00:00Z");
        assert_ne!(old, first.content, "the timestamp must differ");
        fs::write(&path, &old).expect("write a controlled older timestamp");
        assert_eq!(connect(&path, false).outcome, Wrote::Unchanged);
        assert_eq!(fs::read_to_string(&path).expect("read unchanged file"), old);
    }

    #[test]
    fn the_timestamp_prefix_on_a_later_comment_does_not_hide_a_content_change() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-later-prefix")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);

        let annotated = format!("{}{} operator note\n", first.content, render::WRITTEN_BY);
        fs::write(&path, annotated).expect("append a comment using the header prefix");
        assert_eq!(connect(&path, false).outcome, Wrote::Written);
    }

    #[test]
    fn an_unreadable_existing_file_is_an_error_even_with_force() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-read-error")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        fs::write(&path, [0xff, 0xfe]).expect("write a file that cannot be read as UTF-8");
        for force in [false, true] {
            let error = run_with(
                &ConnectOptions {
                    pools_path: Some(path.clone()),
                    force,
                },
                &machine(),
                ["claude-code"],
            )
            .expect_err("an unreadable file must not become absence");
            assert!(
                matches!(error, UpstrokeError::Io { path: ref failed, ref source }
                if failed == &path && source.kind() == std::io::ErrorKind::InvalidData)
            );
            assert_eq!(fs::read(&path).expect("read original bytes"), [0xff, 0xfe]);
        }
    }

    #[test]
    fn pools_write_error_names_a_failed_parent_creation() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-create-error")
                .expect("acquire an isolated pools directory");
        let parent = tree.path().join("not-a-directory");
        fs::write(&parent, "original").expect("block directory creation with a file");
        let error = write_pools(&parent.join("pools.toml"), "proposal")
            .expect_err("the parent path is a file");
        assert!(
            matches!(error, UpstrokeError::Filesystem { operation: "create directory", path, .. }
            if path == parent)
        );
        assert_eq!(
            fs::read_to_string(&parent).expect("read the original file"),
            "original"
        );
    }

    #[test]
    fn pools_write_error_names_a_failed_file_write() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-write-error")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("is-a-directory");
        fs::create_dir(&path).expect("block file writing with a directory");
        let error = write_pools(&path, "proposal").expect_err("the file path is a directory");
        assert!(
            matches!(error, UpstrokeError::Filesystem { operation: "write pools file", path: failed, .. }
            if failed == path)
        );
        assert!(path.is_dir());
    }

    #[test]
    fn discovery_note_lines_cannot_impersonate_warning_records() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-note")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "claude-code",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,
                    models: Vec::new(),
                    shape: Some(PoolKind::Credits),
                    notes: vec![
                        "odd\nwarning: forged\r\n\u{1b}[2Kcontrol\rcarriage\u{85}next".to_owned(),
                    ],
                }),
            }],
        };
        let report = run_with(
            &ConnectOptions {
                pools_path: Some(path),
                force: false,
            },
            &machine,
            ["claude-code"],
        )
        .expect("connect with an untrusted discovery note");
        let output = render_report(&report);
        assert!(output.contains("\n  odd\n  warning: forged\n"), "{output}");
        assert!(
            !output.lines().any(|line| line.starts_with("warning:")),
            "{output}"
        );
        assert!(
            output
                .lines()
                .all(|line| !line.chars().any(char::is_control)),
            "{output:?}"
        );
    }

    #[test]
    fn a_skipped_agent_is_reported_once() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-skipped")
            .expect("acquire an isolated pools directory");
        let report = connect(&tree.path().join("pools.toml"), false);
        let output = render_report(&report);
        assert_eq!(
            output.matches("binary not found on PATH").count(),
            1,
            "{output}"
        );
        assert!(
            output
                .lines()
                .any(|line| line.starts_with("copilot: skipped")),
            "{output}"
        );
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
            report.warnings.is_empty(),
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
    fn operator_keys_with_toml_escapes_survive_a_force_and_read_back_unchanged() {
        let path = scratch("escapes");
        let mine = "[pools.claude-code]\nkind = \"subscription-window\"\nagent = \"claude-code\"\n\
                    profile = 'C:\\Users\\me\\.claude-work'\nendpoint = \"http://host/#frag \\\"q\\\"\"\n";
        fs::write(&path, mine).expect("hand-written file");

        let forced = connect(&path, true);
        assert_eq!(forced.outcome, Wrote::Written);
        let mut warnings = Vec::new();
        let hermetic = path.parent().expect("parent").to_path_buf();
        let cfg = crate::config::load(None, &hermetic, Some(&path), &mut warnings)
            .expect("the file --force wrote parses");
        let pool = cfg.pools.first().expect("one pool");
        assert_eq!(pool.profile.as_deref(), Some(r"C:\Users\me\.claude-work"));
        assert_eq!(pool.endpoint.as_deref(), Some(r#"http://host/#frag "q""#));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");

        let again = connect(&path, false);
        assert_eq!(again.outcome, Wrote::Unchanged, "{}", again.content);
        assert!(
            again
                .content
                .contains("profile = \"C:\\\\Users\\\\me\\\\.claude-work\""),
            "{}",
            again.content
        );
    }

    #[test]
    fn a_renamed_pool_whose_key_carries_an_escaped_quote_is_refused_not_overwritten() {
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "x\"#A",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,
                    models: Vec::new(),
                    shape: Some(PoolKind::Credits),
                    notes: Vec::new(),
                }),
            }],
        };
        let path = scratch("escaped-key");
        let opts = ConnectOptions {
            pools_path: Some(path.clone()),
            force: false,
        };
        let first = run_with(&opts, &machine, ["x\"#A"]).expect("first connect");
        assert_eq!(first.outcome, Wrote::Written);
        let written = fs::read_to_string(&path).expect("file");
        assert!(
            written.contains("[pools.\"x\\\"#A\"]"),
            "the key is quoted and escaped: {written}"
        );

        let renamed = written.replace("[pools.\"x\\\"#A\"]", "[pools.\"x\\\"#B\"]");
        assert_ne!(renamed, written, "the rename changed the file");
        fs::write(&path, &renamed).expect("renamed by hand");
        let second = run_with(&opts, &machine, ["x\"#A"]).expect("second connect");
        assert_eq!(
            second.outcome,
            Wrote::Refused,
            "a renamed pool is a settings difference"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("file"),
            renamed,
            "the operator's rename is untouched"
        );
    }

    #[test]
    fn a_file_connect_cannot_parse_carries_nothing_and_the_refusal_does_not_claim_otherwise() {
        let path = scratch("unparseable");
        let mine = "[pools.claude-code]\nkind = \"subscription-window\"\nagent = \"claude-code\"\n\
                    profile = \"work\"\nthis line is not toml\n";
        fs::write(&path, mine).expect("hand-written file");
        let report = connect(&path, false);
        assert_eq!(report.outcome, Wrote::Refused);
        assert!(
            !report.content.contains("profile = "),
            "nothing was carried out of a file that does not parse:\n{}",
            report.content
        );
        let rendered = render_report(&report);
        assert!(
            !rendered.contains("profile = \"work\""),
            "the proposed text shows the profile is gone: {rendered}"
        );
        assert!(
            rendered.contains("only when it can parse the existing file"),
            "the refusal says when keys are carried, not that they were: {rendered}"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("file"),
            mine,
            "and nothing was written"
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
