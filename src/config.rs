//! Config loading (DESIGN.md §17 subset for `validate`).
//!
//! Two optional files: repo-level `tactus.toml` (routing overrides, pins,
//! strategy) and user-level `~/.tactus/pools.toml` (capacity pools, normally
//! written by `tactus connect`). Both missing is the normal fresh-repo case
//! and falls back to derived defaults silently.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::catalog;
use crate::error::TactusError;
use crate::gates::ShellKind;
use crate::interaction::InteractionMode;
use crate::ir::{TaskKind, Tier};

#[derive(Debug, Default, Deserialize)]
struct RawRepoConfig {
    routing: Option<RawRouting>,
    pins: Option<Vec<RawPin>>,
    // Parsed as raw values so shape mistakes get actionable messages instead
    // of bare serde errors (configs written before these sections were
    // consumed must not brick on upgrade with cryptic output).
    gates: Option<toml::Value>,
    engine: Option<toml::Value>,
    interaction: Option<toml::Value>,
    // Other sections (budgets) are legal in tactus.toml but not consumed yet;
    // serde ignores them.
}

#[derive(Debug, Deserialize)]
struct RawGate {
    name: String,
    cmd: String,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawEngine {
    shell: Option<String>,
    on_task_failure: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawInteraction {
    mode: Option<String>,
    notify: Option<Vec<String>>,
    /// `ask_before = { frontier_escalation_over_usd = 5.0 }` (§12). Accepted
    /// and echoed so a config written to spec does not error, but nothing
    /// raises `ApproveSpend` until the ledger exists (step 8/10).
    ask_before: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawRouting {
    strategy: Option<RawStrategy>,
    overrides: Option<Vec<RawOverride>>,
    /// Per-kind chain entries (`fix = { chain = [...] }`) plus anything the
    /// config author got wrong — unknown keys warn rather than error.
    #[serde(flatten)]
    kinds: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawStrategy {
    mode: Option<String>,
    spend_down_after: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawOverride {
    paths: Vec<String>,
    start_at: Tier,
    second_opinion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPin {
    tier: Tier,
    agent: String,
    model: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawKindRouting {
    chain: Option<Vec<Tier>>,
    tier: Option<Tier>,
    attempts_per: Option<u32>,
    /// `[routing] review = { enabled = false }` — the explicit opt-out of
    /// §11.2 review, for plans where a frontier judgement per task costs more
    /// than the work it is judging.
    enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPools {
    pools: Option<BTreeMap<String, toml::Value>>,
}

#[derive(Debug, Clone)]
pub struct KindChain {
    pub chain: Vec<Tier>,
    pub attempts_per: u32,
    pub from_config: bool,
}

#[derive(Debug)]
pub struct CompiledOverride {
    pub raw_paths: Vec<String>,
    pub start_at: Tier,
    pub second_opinion: Option<String>,
    pub globs: GlobSet,
}

#[derive(Debug, Clone)]
pub struct Strategy {
    pub mode: String,
    pub spend_down_after: Option<f64>,
    pub from_config: bool,
}

#[derive(Debug, Clone)]
pub struct Pin {
    pub tier: Tier,
    pub agent: String,
    pub model: String,
}

/// One `[[gates]]` entry (§17). `None` for the whole list means the section
/// was absent and the engine derives defaults from the repo's shape.
#[derive(Debug, Clone)]
pub struct GateConfig {
    pub name: String,
    pub cmd: String,
    pub timeout: Duration,
}

pub const DEFAULT_GATE_TIMEOUT: Duration = Duration::from_secs(600);

/// `[engine] on_task_failure` (§17).
///
/// This governs only a *genuinely failed* task — one a human declined to
/// unblock, or one whose chain resolved to nothing. A task parked on a
/// question never halts the run whatever this says: invariant 6 ("questions
/// never stop the runnable frontier") is not configurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnTaskFailure {
    Halt,
    Continue,
}

impl OnTaskFailure {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "halt" => Some(Self::Halt),
            "continue" => Some(Self::Continue),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub chains: BTreeMap<TaskKind, KindChain>,
    pub overrides: Vec<CompiledOverride>,
    pub pins: Vec<Pin>,
    pub strategy: Strategy,
    pub pool_names: Vec<String>,
    /// `Some` (possibly empty — explicitly no gates) when `[[gates]]` was
    /// configured; `None` means derive from the repo.
    pub gates: Option<Vec<GateConfig>>,
    pub shell: ShellKind,
    /// `[routing] review = { tier = … }` (§11.2). `None` means the frontier
    /// default.
    pub review_tier: Option<Tier>,
    /// `[routing] review = { enabled = false }` opts out of review entirely.
    pub review_enabled: bool,
    /// `[engine] on_task_failure` (§17); default `Halt`.
    pub on_task_failure: OnTaskFailure,
    /// `[interaction] mode` (§12); default `on_block`.
    pub interaction_mode: InteractionMode,
    /// `[interaction] notify` (§12); default `["cli"]`.
    pub notify: Vec<String>,
}

impl Config {
    pub fn chain_for(&self, kind: TaskKind) -> KindChain {
        self.chains
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| KindChain {
                chain: default_chain(kind),
                attempts_per: DEFAULT_ATTEMPTS_PER,
                from_config: false,
            })
    }
}

pub const DEFAULT_ATTEMPTS_PER: u32 = 2;

/// Derived default escalation chain per kind (DESIGN.md §10.1), used when the
/// repo config is absent or silent for that kind.
pub fn default_chain(kind: TaskKind) -> Vec<Tier> {
    match kind {
        TaskKind::Design => vec![Tier::Frontier],
        TaskKind::Implement | TaskKind::Refactor => vec![Tier::Mid, Tier::Frontier],
        TaskKind::Fix | TaskKind::Test => vec![Tier::Small, Tier::Mid, Tier::Frontier],
        TaskKind::Docs | TaskKind::Chore => vec![Tier::Small, Tier::Mid],
    }
}

/// Load effective config.
///
/// `repo_config`: explicit `--config` path (missing file = error) or `None`
/// to look for `tactus.toml` in `discover_in` (missing = silent defaults).
/// `discover_in` is the repo root the run targets — never the process CWD,
/// which can differ and would load another repo's config.
/// `pools_file`: explicit pools path (tests) or `None` to discover
/// `~/.tactus/pools.toml` (missing = silent).
pub fn load(
    repo_config: Option<&Path>,
    discover_in: &Path,
    pools_file: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Result<Config, TactusError> {
    let (raw, repo_path) = read_repo_config(repo_config, discover_in)?;

    let mut chains: BTreeMap<TaskKind, KindChain> = TaskKind::ALL
        .iter()
        .map(|k| {
            (
                *k,
                KindChain {
                    chain: default_chain(*k),
                    attempts_per: DEFAULT_ATTEMPTS_PER,
                    from_config: false,
                },
            )
        })
        .collect();
    let mut overrides = Vec::new();
    let mut review_tier: Option<Tier> = None;
    let mut review_enabled = true;
    let mut strategy = Strategy {
        mode: "conserve".to_owned(),
        spend_down_after: None,
        from_config: false,
    };

    if let Some(routing) = raw.routing {
        for (key, value) in routing.kinds {
            let Some(kind) = TaskKind::parse(&key) else {
                // `review` is a routing ROLE, not a task kind (DESIGN §17's
                // own example configures it). Parse and echo it rather than
                // warning users off their own documented config; the reviewer
                // consumes it in step 6.
                if key == "review" {
                    let rr: RawKindRouting = value.try_into().map_err(|e| TactusError::Config {
                        path: repo_path.clone(),
                        message: format!(
                            "routing entry `review`: {e} (expected `tier`, or \
                             `enabled = false` to run without review)"
                        ),
                    })?;
                    review_enabled = rr.enabled.unwrap_or(true);
                    review_tier = rr
                        .tier
                        .or_else(|| rr.chain.and_then(|c| c.first().copied()));
                    continue;
                }
                warnings.push(format!(
                    "unknown routing kind `{key}` in {} (ignored)",
                    repo_path.display()
                ));
                continue;
            };
            let kr: RawKindRouting = value.try_into().map_err(|e| TactusError::Config {
                path: repo_path.clone(),
                message: format!("routing entry `{key}`: {e}"),
            })?;
            if kr.attempts_per == Some(0) {
                return Err(TactusError::Config {
                    path: repo_path.clone(),
                    message: format!(
                        "[routing] `{key}`: attempts_per must be at least 1 — omit it for the \
                         default of {DEFAULT_ATTEMPTS_PER}"
                    ),
                });
            }
            let chain = match (kr.chain, kr.tier) {
                (Some(chain), _) if !chain.is_empty() => chain,
                (_, Some(tier)) => vec![tier],
                _ => default_chain(kind),
            };
            chains.insert(
                kind,
                KindChain {
                    chain,
                    attempts_per: kr.attempts_per.unwrap_or(DEFAULT_ATTEMPTS_PER),
                    from_config: true,
                },
            );
        }
        for ov in routing.overrides.unwrap_or_default() {
            let mut builder = GlobSetBuilder::new();
            for pattern in &ov.paths {
                let glob = Glob::new(pattern).map_err(|e| TactusError::Config {
                    path: repo_path.clone(),
                    message: format!("invalid glob `{pattern}` in [[routing.overrides]]: {e}"),
                })?;
                builder.add(glob);
            }
            let globs = builder.build().map_err(|e| TactusError::Config {
                path: repo_path.clone(),
                message: format!("building glob set for [[routing.overrides]]: {e}"),
            })?;
            overrides.push(CompiledOverride {
                raw_paths: ov.paths,
                start_at: ov.start_at,
                second_opinion: ov.second_opinion,
                globs,
            });
        }
        if let Some(s) = routing.strategy {
            let mode = s.mode.unwrap_or_else(|| "conserve".to_owned());
            if !matches!(mode.as_str(), "conserve" | "value-max" | "deadline") {
                warnings.push(format!(
                    "unknown routing strategy mode `{mode}` in {} (echoed, never acted on in \
                     validate)",
                    repo_path.display()
                ));
            }
            strategy = Strategy {
                mode,
                spend_down_after: s.spend_down_after,
                from_config: true,
            };
        }
    }

    let mut pins: Vec<Pin> = Vec::new();
    for pin in raw.pins.unwrap_or_default() {
        if catalog::lookup(&pin.agent, &pin.model).is_none() {
            let known = catalog::known_models(&pin.agent);
            let known = if known.is_empty() {
                format!("none (unknown agent `{}`)", pin.agent)
            } else {
                known.join(", ")
            };
            return Err(TactusError::UnknownPinnedModel {
                agent: pin.agent,
                model: pin.model,
                known,
            });
        }
        if pins.iter().any(|p: &Pin| p.tier == pin.tier) {
            warnings.push(format!(
                "duplicate pin for tier `{}` in {} (first pin wins)",
                pin.tier,
                repo_path.display()
            ));
            continue;
        }
        pins.push(Pin {
            tier: pin.tier,
            agent: pin.agent,
            model: pin.model,
        });
    }

    let gates = parse_gates(raw.gates, &repo_path)?;
    let (shell, on_task_failure) = parse_engine(raw.engine, &repo_path, warnings)?;
    let (interaction_mode, notify) = parse_interaction(raw.interaction, &repo_path, warnings)?;

    let pool_names = read_pools(pools_file)?;

    Ok(Config {
        chains,
        overrides,
        pins,
        strategy,
        pool_names,
        gates,
        shell,
        review_tier,
        review_enabled,
        on_task_failure,
        interaction_mode,
        notify,
    })
}

/// `[[gates]]` parsing with actionable shape errors: a `[gates]` table, a
/// wrong-typed field, or `timeout_secs = 0` all name what was expected.
fn parse_gates(
    raw: Option<toml::Value>,
    repo_path: &Path,
) -> Result<Option<Vec<GateConfig>>, TactusError> {
    let config_error = |message: String| TactusError::Config {
        path: repo_path.to_path_buf(),
        message,
    };
    let Some(value) = raw else { return Ok(None) };
    let toml::Value::Array(entries) = value else {
        return Err(config_error(format!(
            "`gates` must be an array of tables — write `[[gates]]` entries (double brackets, \
             one per gate), found a {}",
            value.type_str()
        )));
    };
    let mut list = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let n = index + 1;
        let g: RawGate = entry.try_into().map_err(|e| {
            config_error(format!(
                "[[gates]] entry {n}: {e} (each entry takes `name`, `cmd`, and an optional \
                 `timeout_secs` integer)"
            ))
        })?;
        if g.name.trim().is_empty() || g.cmd.trim().is_empty() {
            return Err(config_error(format!(
                "[[gates]] entry {n} needs a non-empty `name` and `cmd`"
            )));
        }
        if g.timeout_secs == Some(0) {
            return Err(config_error(format!(
                "[[gates]] entry {n} (`{}`): timeout_secs must be at least 1 — omit it for the \
                 {}s default",
                g.name,
                DEFAULT_GATE_TIMEOUT.as_secs()
            )));
        }
        list.push(GateConfig {
            name: g.name,
            cmd: g.cmd,
            timeout: g
                .timeout_secs
                .map_or(DEFAULT_GATE_TIMEOUT, Duration::from_secs),
        });
    }
    Ok(Some(list))
}

fn parse_engine(
    raw: Option<toml::Value>,
    repo_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<(ShellKind, OnTaskFailure), TactusError> {
    let Some(value) = raw else {
        return Ok((ShellKind::native(), OnTaskFailure::Halt));
    };
    let engine: RawEngine = value.try_into().map_err(|e| TactusError::Config {
        path: repo_path.to_path_buf(),
        message: format!(
            "[engine]: {e} (expected a table with optional `shell` and `on_task_failure` strings)"
        ),
    })?;
    let shell = match engine.shell {
        None => ShellKind::native(),
        Some(requested) => ShellKind::parse(&requested).unwrap_or_else(|| {
            warnings.push(format!(
                "unknown [engine] shell `{requested}` in {} (using the platform default; known: \
                 cmd, sh, bash, powershell, pwsh)",
                repo_path.display()
            ));
            ShellKind::native()
        }),
    };
    // A misspelling here decides whether a failed task stops the run, so it
    // errors rather than warning: silently halting a run the user asked to
    // continue (or the reverse) is not a recoverable surprise.
    let on_task_failure = match engine.on_task_failure {
        None => OnTaskFailure::Halt,
        Some(requested) => OnTaskFailure::parse(&requested).ok_or_else(|| TactusError::Config {
            path: repo_path.to_path_buf(),
            message: format!(
                "[engine] on_task_failure `{requested}` is not recognized (expected `halt` or \
                     `continue`)"
            ),
        })?,
    };
    Ok((shell, on_task_failure))
}

/// `[interaction]` (§12). Mode decides whether a human can ever be asked, so a
/// typo errors; notifier ids only decide delivery, so they warn.
fn parse_interaction(
    raw: Option<toml::Value>,
    repo_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<(InteractionMode, Vec<String>), TactusError> {
    let default_notify = || vec!["cli".to_owned()];
    let Some(value) = raw else {
        return Ok((InteractionMode::default(), default_notify()));
    };
    let interaction: RawInteraction = value.try_into().map_err(|e| TactusError::Config {
        path: repo_path.to_path_buf(),
        message: format!(
            "[interaction]: {e} (expected optional `mode`, `notify` list, and `ask_before` table)"
        ),
    })?;
    if interaction.ask_before.is_some() {
        warnings.push(
            "[interaction] ask_before is parsed but not acted on yet — spend approvals need the \
             ledger (§13), so no ApproveSpend question is raised"
                .to_owned(),
        );
    }
    let mode = match interaction.mode {
        None => InteractionMode::default(),
        Some(requested) => {
            InteractionMode::parse(&requested).ok_or_else(|| TactusError::Config {
                path: repo_path.to_path_buf(),
                message: format!(
                    "[interaction] mode `{requested}` is not recognized (expected `never`, \
                     `on_block`, or `on_milestone`)"
                ),
            })?
        }
    };
    Ok((mode, interaction.notify.unwrap_or_else(default_notify)))
}

fn read_repo_config(
    repo_config: Option<&Path>,
    discover_in: &Path,
) -> Result<(RawRepoConfig, PathBuf), TactusError> {
    let (path, required) = match repo_config {
        Some(p) => (p.to_path_buf(), true),
        None => (discover_in.join("tactus.toml"), false),
    };
    if !path.exists() {
        if required {
            return Err(TactusError::Config {
                path,
                message: "file not found".to_owned(),
            });
        }
        return Ok((RawRepoConfig::default(), path));
    }
    let text = fs::read_to_string(&path).map_err(|source| TactusError::Io {
        path: path.clone(),
        source,
    })?;
    let raw = toml::from_str(&text).map_err(|e| TactusError::Config {
        path: path.clone(),
        message: e.to_string(),
    })?;
    Ok((raw, path))
}

fn read_pools(pools_file: Option<&Path>) -> Result<Vec<String>, TactusError> {
    let path = match pools_file {
        Some(p) => p.to_path_buf(),
        None => match discovered_pools_path() {
            Some(p) => p,
            None => return Ok(Vec::new()),
        },
    };
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|source| TactusError::Io {
        path: path.clone(),
        source,
    })?;
    let raw: RawPools = toml::from_str(&text).map_err(|e| TactusError::Config {
        path: path.clone(),
        message: e.to_string(),
    })?;
    Ok(raw.pools.unwrap_or_default().into_keys().collect())
}

fn discovered_pools_path() -> Option<PathBuf> {
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".tactus").join("pools.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, content: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tactus-config-tests-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join(name);
        fs::write(&path, content).expect("write scratch file");
        path
    }

    fn missing() -> PathBuf {
        env::temp_dir()
            .join("tactus-definitely-missing")
            .join("pools.toml")
    }

    /// Empty discovery root so tests never pick up a real tactus.toml.
    fn hermetic() -> PathBuf {
        let dir = env::temp_dir().join(format!("tactus-config-hermetic-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("hermetic dir");
        dir
    }

    #[test]
    fn missing_files_fall_back_to_derived_defaults() {
        let mut warnings = Vec::new();
        let cfg = load(None, &hermetic(), Some(&missing()), &mut warnings).expect("load defaults");
        assert!(warnings.is_empty());
        assert_eq!(
            cfg.chain_for(TaskKind::Fix).chain,
            vec![Tier::Small, Tier::Mid, Tier::Frontier]
        );
        assert_eq!(cfg.chain_for(TaskKind::Design).chain, vec![Tier::Frontier]);
        assert_eq!(cfg.strategy.mode, "conserve");
        assert!(!cfg.strategy.from_config);
        assert!(cfg.overrides.is_empty());
        assert!(cfg.pins.is_empty());
        assert!(cfg.pool_names.is_empty());
    }

    #[test]
    fn explicit_config_path_must_exist() {
        let mut warnings = Vec::new();
        let err = load(
            Some(&missing()),
            &hermetic(),
            Some(&missing()),
            &mut warnings,
        )
        .expect_err("missing --config errors");
        assert!(matches!(err, TactusError::Config { .. }));
    }

    #[test]
    fn parses_chains_overrides_pins_and_strategy() {
        let path = scratch(
            "full.toml",
            r#"
[routing.strategy]
mode = "value-max"
spend_down_after = 0.7

[routing]
fix = { chain = ["small", "mid", "frontier"], attempts_per = 3 }
implement = { tier = "frontier" }
review = { tier = "frontier" }

[[routing.overrides]]
paths = ["src/auth/**", "migrations/**"]
start_at = "frontier"
second_opinion = "different-vendor"

[[pins]]
tier = "frontier"
agent = "claude-code"
model = "claude-opus-4-8"
"#,
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect("load full config");
        assert_eq!(cfg.chain_for(TaskKind::Fix).attempts_per, 3);
        assert_eq!(
            cfg.chain_for(TaskKind::Implement).chain,
            vec![Tier::Frontier]
        );
        assert!(
            cfg.chain_for(TaskKind::Docs).chain.len() == 2
                && !cfg.chain_for(TaskKind::Docs).from_config
        );
        assert_eq!(cfg.overrides.len(), 1);
        assert!(cfg.overrides[0].globs.is_match("src/auth/login.rs"));
        assert!(!cfg.overrides[0].globs.is_match("src/api/list.rs"));
        assert_eq!(cfg.pins.len(), 1);
        assert_eq!(cfg.strategy.mode, "value-max");
        assert_eq!(cfg.strategy.spend_down_after, Some(0.7));
        // `review` is a routing role, not a task kind: parsed, echoed, and
        // never warned about (DESIGN §17 configures it in its own example).
        assert_eq!(cfg.review_tier, Some(Tier::Frontier));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn zero_attempts_per_is_rejected() {
        let path = scratch(
            "zeroattempts.toml",
            "[routing]\nfix = { chain = [\"small\"], attempts_per = 0 }\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("zero attempts must error");
        assert!(err.to_string().contains("at least 1"), "got: {err}");
    }

    #[test]
    fn pin_with_unknown_model_is_a_hard_error() {
        let path = scratch(
            "badpin.toml",
            "[[pins]]\ntier = \"mid\"\nagent = \"claude-code\"\nmodel = \"claude-nonexistent\"\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("unknown model");
        let msg = err.to_string();
        assert!(msg.contains("claude-nonexistent"));
        assert!(
            msg.contains("claude-opus-4-8"),
            "should list known models: {msg}"
        );
    }

    #[test]
    fn duplicate_pin_tier_warns_and_first_wins() {
        let path = scratch(
            "duppin.toml",
            r#"
[[pins]]
tier = "frontier"
agent = "claude-code"
model = "claude-opus-5"

[[pins]]
tier = "frontier"
agent = "copilot"
model = "gpt-5"
"#,
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.pins.len(), 1);
        assert_eq!(cfg.pins[0].model, "claude-opus-5");
        assert!(warnings.iter().any(|w| w.contains("duplicate pin")));
    }

    #[test]
    fn pools_file_names_are_collected() {
        let path = scratch(
            "pools.toml",
            r#"
[pools.claude-max]
kind = "subscription-window"
agent = "claude-code"

[pools.copilot]
kind = "credits"
agent = "copilot"
"#,
        );
        let mut warnings = Vec::new();
        let cfg = load(None, &hermetic(), Some(&path), &mut warnings).expect("load pools");
        assert_eq!(
            cfg.pool_names,
            vec!["claude-max".to_owned(), "copilot".to_owned()]
        );
    }

    #[test]
    fn wrong_section_shapes_get_actionable_errors() {
        // `[gates]` as a table — the classic array-of-tables mistake.
        let path = scratch("gatestable.toml", "[gates]\ncheck = \"cargo check\"\n");
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("table shape must error");
        let msg = err.to_string();
        assert!(msg.contains("[[gates]]"), "names the expected shape: {msg}");

        // Wrong field type inside an entry.
        let path = scratch(
            "gatestype.toml",
            "[[gates]]\nname = \"t\"\ncmd = \"cargo test\"\ntimeout_secs = \"600\"\n",
        );
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("string timeout must error");
        assert!(err.to_string().contains("timeout_secs"), "got: {err}");

        // [engine] with a wrong type.
        let path = scratch("enginetype.toml", "[engine]\nshell = 5\n");
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("numeric shell must error");
        assert!(err.to_string().contains("[engine]"), "got: {err}");
    }

    #[test]
    fn zero_gate_timeout_is_rejected_at_load() {
        let path = scratch(
            "zerotimeout.toml",
            "[[gates]]\nname = \"test\"\ncmd = \"cargo test\"\ntimeout_secs = 0\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("zero timeout must error");
        assert!(err.to_string().contains("at least 1"), "got: {err}");
    }

    #[test]
    fn discovery_uses_the_given_root_not_cwd() {
        let root = scratch("discovery-root.toml", "unused = true\n")
            .parent()
            .expect("parent")
            .to_path_buf();
        let repo_root = root.join("discovery-repo");
        fs::create_dir_all(&repo_root).expect("repo root");
        fs::write(
            repo_root.join("tactus.toml"),
            "[[gates]]\nname = \"only-here\"\ncmd = \"git --version\"\n",
        )
        .expect("write config");
        let mut warnings = Vec::new();
        let cfg = load(None, &repo_root, Some(&missing()), &mut warnings).expect("discover");
        let gates = cfg.gates.expect("gates found via repo root");
        assert_eq!(gates[0].name, "only-here");
    }

    #[test]
    fn gates_parse_with_default_timeout() {
        let path = scratch(
            "gates.toml",
            r#"
[engine]
shell = "powershell"

[[gates]]
name = "check"
cmd = "cargo check --all-targets"

[[gates]]
name = "test"
cmd = "cargo test"
timeout_secs = 1200
"#,
        );
        let mut warnings = Vec::new();
        let cfg =
            load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load gates");
        let gates = cfg.gates.expect("gates configured");
        assert_eq!(gates.len(), 2);
        assert_eq!(gates[0].timeout, DEFAULT_GATE_TIMEOUT);
        assert_eq!(gates[1].timeout, Duration::from_secs(1200));
        assert_eq!(cfg.shell, ShellKind::PowerShell);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn absent_gates_mean_derive_and_empty_means_none() {
        let mut warnings = Vec::new();
        let cfg = load(None, &hermetic(), Some(&missing()), &mut warnings).expect("defaults");
        assert!(cfg.gates.is_none(), "absent section derives at run time");
        assert_eq!(cfg.shell, ShellKind::native());

        let path = scratch("nogates.toml", "gates = []\n");
        let cfg =
            load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("empty gates");
        assert_eq!(cfg.gates.expect("explicit").len(), 0);
    }

    #[test]
    fn interaction_and_failure_policy_default_without_config() {
        let mut warnings = Vec::new();
        let cfg = load(None, &hermetic(), Some(&missing()), &mut warnings).expect("defaults");
        assert_eq!(cfg.interaction_mode, InteractionMode::OnBlock);
        assert_eq!(cfg.notify, ["cli"]);
        assert_eq!(cfg.on_task_failure, OnTaskFailure::Halt, "§17's default");
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn interaction_and_failure_policy_parse_from_config() {
        let path = scratch(
            "interaction.toml",
            r#"
[engine]
on_task_failure = "continue"

[interaction]
mode = "never"
notify = ["cli", "desktop"]
ask_before = { frontier_escalation_over_usd = 5.0 }
"#,
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.interaction_mode, InteractionMode::Never);
        assert_eq!(cfg.notify, ["cli", "desktop"]);
        assert_eq!(cfg.on_task_failure, OnTaskFailure::Continue);
        // Accepted per §17's own example, but honestly reported as inert.
        assert!(
            warnings.iter().any(|w| w.contains("ask_before")),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn misspelled_mode_or_failure_policy_is_a_hard_error() {
        // Both decide whether the run stops or waits for a human. A typo that
        // silently reverts to the default is not a recoverable surprise.
        let path = scratch("badmode.toml", "[interaction]\nmode = \"always\"\n");
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("unknown mode must error");
        assert!(err.to_string().contains("on_block"), "got: {err}");

        let path = scratch("badfailure.toml", "[engine]\non_task_failure = \"stop\"\n");
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("unknown policy must error");
        assert!(err.to_string().contains("continue"), "got: {err}");
    }

    #[test]
    fn blank_gate_fields_and_unknown_shell_are_handled() {
        let path = scratch(
            "badgate.toml",
            "[[gates]]\nname = \"\"\ncmd = \"cargo test\"\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("blank name");
        assert!(err.to_string().contains("non-empty"));

        let path = scratch("badshell.toml", "[engine]\nshell = \"fish\"\n");
        let cfg =
            load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("tolerated");
        assert_eq!(cfg.shell, ShellKind::native());
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unknown [engine] shell"))
        );
    }
}
