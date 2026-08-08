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

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::catalog;
use crate::error::TactusError;
use crate::ir::{TaskKind, Tier};

#[derive(Debug, Default, Deserialize)]
struct RawRepoConfig {
    routing: Option<RawRouting>,
    pins: Option<Vec<RawPin>>,
    // Other sections (engine, interaction, budgets, gates) are legal in
    // tactus.toml but not consumed by validate; serde ignores them.
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

#[derive(Debug)]
pub struct Config {
    pub chains: BTreeMap<TaskKind, KindChain>,
    pub overrides: Vec<CompiledOverride>,
    pub pins: Vec<Pin>,
    pub strategy: Strategy,
    pub pool_names: Vec<String>,
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
/// to look for `./tactus.toml` (missing = silent defaults).
/// `pools_file`: explicit pools path (tests) or `None` to discover
/// `~/.tactus/pools.toml` (missing = silent).
pub fn load(
    repo_config: Option<&Path>,
    pools_file: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Result<Config, TactusError> {
    let (raw, repo_path) = read_repo_config(repo_config)?;

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
    let mut strategy = Strategy {
        mode: "conserve".to_owned(),
        spend_down_after: None,
        from_config: false,
    };

    if let Some(routing) = raw.routing {
        for (key, value) in routing.kinds {
            let Some(kind) = TaskKind::parse(&key) else {
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

    let pool_names = read_pools(pools_file)?;

    Ok(Config {
        chains,
        overrides,
        pins,
        strategy,
        pool_names,
    })
}

fn read_repo_config(repo_config: Option<&Path>) -> Result<(RawRepoConfig, PathBuf), TactusError> {
    let (path, required) = match repo_config {
        Some(p) => (p.to_path_buf(), true),
        None => (PathBuf::from("tactus.toml"), false),
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

    #[test]
    fn missing_files_fall_back_to_derived_defaults() {
        let mut warnings = Vec::new();
        let cfg = load(None, Some(&missing()), &mut warnings).expect("load defaults");
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
        let err = load(Some(&missing()), Some(&missing()), &mut warnings)
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
        let cfg = load(Some(&path), Some(&missing()), &mut warnings).expect("load full config");
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
        // `review` is not a step-1 TaskKind — tolerated with a warning.
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unknown routing kind `review`"))
        );
    }

    #[test]
    fn pin_with_unknown_model_is_a_hard_error() {
        let path = scratch(
            "badpin.toml",
            "[[pins]]\ntier = \"mid\"\nagent = \"claude-code\"\nmodel = \"claude-nonexistent\"\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), Some(&missing()), &mut warnings).expect_err("unknown model");
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
        let cfg = load(Some(&path), Some(&missing()), &mut warnings).expect("load");
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
        let cfg = load(None, Some(&path), &mut warnings).expect("load pools");
        assert_eq!(
            cfg.pool_names,
            vec!["claude-max".to_owned(), "copilot".to_owned()]
        );
    }
}
