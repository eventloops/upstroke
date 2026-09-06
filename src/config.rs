//! Extended notes: `docs/internals/config.md`

// LEGACY-EFFECT: this module is in the **frozen legacy section** of
// `effects/allowlist.toml`, which carries its justification and the condition

#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::capacity::{self, Allowance, Pool, PoolKind, Source};
use crate::catalog;
use crate::error::UpstrokeError;
use crate::gates::ShellKind;
use crate::interaction::InteractionMode;
use crate::ir::{Effort, ResolvedEffortPolicy, TaskKind, Tier};
use crate::topology::events::RunnerKind;
use crate::util;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepoConfig {
    routing: Option<RawRouting>,
    pins: Option<Vec<RawPin>>,

    gates: Option<toml::Value>,
    engine: Option<toml::Value>,
    interaction: Option<toml::Value>,
    budgets: Option<toml::Value>,
    runner: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawRunner {
    kind: Option<String>,

    image: Option<String>,

    credential_volumes: Option<BTreeMap<String, String>>,

    mounts: Option<Vec<RawRunnerMount>>,

    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRunnerMount {
    source: PathBuf,
    target: String,
    read_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawGate {
    name: Option<String>,

    cmd: Option<String>,
    timeout_secs: Option<u64>,

    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawEngine {
    shell: Option<String>,
    on_task_failure: Option<String>,

    max_parallel: Option<u32>,

    max_merge_repairs: Option<u32>,

    max_per_agent: Option<u32>,
    max_per_pool: Option<u32>,

    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInteraction {
    mode: Option<String>,
    notify: Option<Vec<String>>,

    wait_on_block_secs: Option<u64>,

    ask_before: Option<toml::Value>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskBefore {
    pub frontier_escalation_over_usd: Option<f64>,
}

impl AskBefore {
    const ACCEPTED: [&'static str; 1] = ["frontier_escalation_over_usd"];
}

#[derive(Debug, Deserialize)]
struct RawRouting {
    strategy: Option<RawStrategy>,
    overrides: Option<Vec<RawOverride>>,

    effort: Option<toml::Value>,

    #[serde(flatten)]
    kinds: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoleEffort {
    implementation: Option<String>,
    review: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawStrategy {
    mode: Option<String>,
    spend_down_after: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOverride {
    paths: Vec<String>,

    start_at: Option<Tier>,
    second_opinion: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPin {
    tier: Tier,
    agent: String,
    model: String,

    effort: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKindRouting {
    chain: Option<Vec<Tier>>,
    tier: Option<Tier>,
    attempts_per: Option<u32>,

    timeout_secs: Option<u64>,

    enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPools {
    pools: Option<BTreeMap<String, toml::Spanned<toml::Value>>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPool {
    kind: Option<String>,
    agent: Option<String>,
    window: Option<String>,
    weekly: Option<bool>,
    sources: Option<Vec<String>>,
    safety_margin: Option<f64>,
    reserve: Option<f64>,
    monthly_allowance: Option<toml::Value>,
    endpoint: Option<String>,

    profile: Option<String>,

    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    pub run_usd: Option<f64>,
    pub task_usd: Option<f64>,
}

impl Budgets {
    pub fn any(self) -> bool {
        self.run_usd.is_some() || self.task_usd.is_some()
    }
}

pub fn check_budget(name: &str, limit: f64) -> Result<(), String> {
    if !limit.is_finite() || limit <= 0.0 {
        return Err(format!(
            "`{name} = {limit}` is not a spendable ceiling — omit it for unlimited, or give it a \
             positive number of dollars"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct KindChain {
    pub chain: Vec<Tier>,
    pub attempts_per: u32,
    pub from_config: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondOpinion {
    DifferentVendor,
}

impl SecondOpinion {
    const ACCEPTED: [&'static str; 1] = ["different-vendor"];

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "different-vendor" => Some(Self::DifferentVendor),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct CompiledOverride {
    pub raw_paths: Vec<String>,

    pub start_at: Option<Tier>,
    pub second_opinion: Option<SecondOpinion>,
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
    pub effort: Option<Effort>,
}

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub name: String,
    pub cmd: String,
    pub timeout: Duration,
}

pub const DEFAULT_GATE_TIMEOUT: Duration = Duration::from_secs(600);

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

pub const DEFAULT_MAX_PARALLEL: u32 = 1;

pub const DEFAULT_MAX_MERGE_REPAIRS: u32 = 2;

pub const LAST_SEQUENTIAL_SCHEMA: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineLimits {
    Fresh,

    SequentialResume,

    SequentialResumeWithRecordedGates,
}

impl EngineLimits {
    #[must_use]
    pub fn for_resume(effective_schema: u32, gates_recorded: bool) -> Self {
        if effective_schema > LAST_SEQUENTIAL_SCHEMA {
            Self::Fresh
        } else if gates_recorded {
            Self::SequentialResumeWithRecordedGates
        } else {
            Self::SequentialResume
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerMount {
    pub source: PathBuf,

    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerSelection {
    pub kind: RunnerKind,

    pub image: Option<String>,

    pub credential_volumes: BTreeMap<String, String>,

    pub mounts: Vec<RunnerMount>,

    pub from_config: bool,
}

impl RunnerSelection {
    #[must_use]
    pub fn host_default() -> Self {
        Self {
            kind: RunnerKind::Host,
            image: None,
            credential_volumes: BTreeMap::new(),
            mounts: Vec::new(),
            from_config: false,
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub chains: BTreeMap<TaskKind, KindChain>,
    pub overrides: Vec<CompiledOverride>,
    pub pins: Vec<Pin>,
    pub strategy: Strategy,

    pub pools: Vec<Pool>,

    pub budgets: Budgets,

    pub ask_before: AskBefore,

    pub gates: Option<Vec<GateConfig>>,
    pub shell: ShellKind,

    pub review_tier: Option<Tier>,

    pub review_enabled: bool,

    pub review_pass_timeout: Duration,

    implementation_effort_override: Option<Effort>,
    review_effort_override: Option<Effort>,

    pub on_task_failure: OnTaskFailure,

    pub max_parallel: u32,

    pub max_merge_repairs: u32,

    pub max_per_agent: u32,

    pub max_per_pool: u32,

    pub interaction_mode: InteractionMode,

    pub notify: Vec<String>,

    pub wait_on_block: Duration,

    pub runner: RunnerSelection,
}

struct EngineSettings {
    shell: ShellKind,
    on_task_failure: OnTaskFailure,
    max_parallel: u32,
    max_merge_repairs: u32,
    max_per_agent: u32,
    max_per_pool: u32,
}

struct InteractionSettings {
    mode: InteractionMode,
    notify: Vec<String>,
    wait_on_block: Duration,
    ask_before: AskBefore,
}

pub const DEFAULT_WAIT_ON_BLOCK: Duration = Duration::from_secs(30 * 60);

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

    pub fn effort_for(&self, tier: Tier) -> Effort {
        self.pins
            .iter()
            .find(|pin| pin.tier == tier)
            .and_then(|pin| pin.effort)
            .unwrap_or_else(|| Effort::for_tier(tier))
    }

    pub fn implementation_effort(&self, tier: Tier) -> Effort {
        self.implementation_effort_override
            .unwrap_or_else(|| self.effort_for(tier))
    }

    pub fn review_effort(&self) -> Effort {
        self.review_effort_override
            .unwrap_or_else(|| self.effort_for(self.review_tier.unwrap_or(Tier::Frontier)))
    }

    pub fn resolved_effort_policy(&self) -> ResolvedEffortPolicy {
        ResolvedEffortPolicy {
            small: self.implementation_effort(Tier::Small),
            mid: self.implementation_effort(Tier::Mid),
            frontier: self.implementation_effort(Tier::Frontier),
            review: self.review_effort(),
        }
    }
}

pub const DEFAULT_ATTEMPTS_PER: u32 = 2;

pub const DEFAULT_REVIEW_PASS_TIMEOUT: Duration = Duration::from_secs(90 * 60);

pub fn default_chain(kind: TaskKind) -> Vec<Tier> {
    match kind {
        TaskKind::Design => vec![Tier::Frontier],
        TaskKind::Implement | TaskKind::Refactor => vec![Tier::Mid, Tier::Frontier],
        TaskKind::Fix | TaskKind::Test => vec![Tier::Small, Tier::Mid, Tier::Frontier],
        TaskKind::Docs | TaskKind::Chore => vec![Tier::Small, Tier::Mid],
    }
}

pub fn load(
    repo_config: Option<&Path>,
    discover_in: &Path,
    pools_file: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Result<Config, UpstrokeError> {
    load_limits(
        repo_config,
        discover_in,
        pools_file,
        EngineLimits::Fresh,
        warnings,
    )
}

pub fn load_limits(
    repo_config: Option<&Path>,
    discover_in: &Path,
    pools_file: Option<&Path>,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<Config, UpstrokeError> {
    load_with(
        repo_config,
        discover_in,
        pools_file,
        &|agent| crate::agent::by_id(agent).is_some(),
        limits,
        warnings,
    )
}

pub fn load_with(
    repo_config: Option<&Path>,
    discover_in: &Path,
    pools_file: Option<&Path>,
    has_adapter: &dyn Fn(&str) -> bool,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<Config, UpstrokeError> {
    load_captured_with(
        &CapturedConfig::capture(repo_config, discover_in, pools_file),
        has_adapter,
        limits,
        warnings,
    )
}

pub fn load_captured(
    captured: &CapturedConfig,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<Config, UpstrokeError> {
    load_captured_with(
        captured,
        &|agent| crate::agent::by_id(agent).is_some(),
        limits,
        warnings,
    )
}

pub fn load_captured_with(
    captured: &CapturedConfig,
    has_adapter: &dyn Fn(&str) -> bool,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<Config, UpstrokeError> {
    let (raw, repo_path) = read_repo_config(&captured.repo)?;

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
    let mut review_pass_timeout = DEFAULT_REVIEW_PASS_TIMEOUT;
    let mut implementation_effort_override = None;
    let mut review_effort_override = None;
    let mut strategy = Strategy {
        mode: "conserve".to_owned(),
        spend_down_after: None,
        from_config: false,
    };

    if let Some(routing) = raw.routing {
        if let Some(value) = routing.effort {
            let policy: RawRoleEffort =
                value.try_into().map_err(|e| UpstrokeError::Config {
                    path: repo_path.clone(),
                    message: format!(
                        "[routing.effort]: {e} (expected optional `implementation` and `review` effort strings)"
                    ),
                })?;
            implementation_effort_override = parse_role_effort(
                policy.implementation.as_deref(),
                "implementation",
                &repo_path,
            )?;
            review_effort_override =
                parse_role_effort(policy.review.as_deref(), "review", &repo_path)?;
        }
        for (key, value) in routing.kinds {
            let Some(kind) = TaskKind::parse(&key) else {
                if key == "review" {
                    let rr: RawKindRouting =
                        value.try_into().map_err(|e| UpstrokeError::Config {
                            path: repo_path.clone(),
                            message: format!(
                                "routing entry `review`: {e} (expected `tier`, `timeout_secs`, or \
                             `enabled = false` to run without review)"
                            ),
                        })?;
                    if rr.attempts_per.is_some() {
                        return Err(UpstrokeError::Config {
                            path: repo_path.clone(),
                            message:
                                "[routing] `review`: attempts_per applies only to task-kind roles"
                                    .to_owned(),
                        });
                    }
                    review_enabled = rr.enabled.unwrap_or(true);
                    review_tier = rr
                        .tier
                        .or_else(|| rr.chain.and_then(|c| c.first().copied()));
                    if rr.timeout_secs == Some(0) {
                        return Err(UpstrokeError::Config {
                            path: repo_path.clone(),
                            message: "[routing] `review`: timeout_secs must be at least 1; omit it for the default of 5400 seconds".to_owned(),
                        });
                    }
                    review_pass_timeout = rr
                        .timeout_secs
                        .map(Duration::from_secs)
                        .unwrap_or(DEFAULT_REVIEW_PASS_TIMEOUT);
                    continue;
                }
                warnings.push(format!(
                    "unknown routing kind `{key}` in {} (ignored)",
                    repo_path.display()
                ));
                continue;
            };
            let kr: RawKindRouting = value.try_into().map_err(|e| UpstrokeError::Config {
                path: repo_path.clone(),
                message: format!("routing entry `{key}`: {e}"),
            })?;
            if kr.attempts_per == Some(0) {
                return Err(UpstrokeError::Config {
                    path: repo_path.clone(),
                    message: format!(
                        "[routing] `{key}`: attempts_per must be at least 1 — omit it for the \
                         default of {DEFAULT_ATTEMPTS_PER}"
                    ),
                });
            }
            if kr.timeout_secs.is_some() {
                return Err(UpstrokeError::Config {
                    path: repo_path.clone(),
                    message: format!(
                        "[routing] `{key}`: timeout_secs applies only to the `review` role"
                    ),
                });
            }
            if kr.enabled.is_some() {
                return Err(UpstrokeError::Config {
                    path: repo_path.clone(),
                    message: format!(
                        "[routing] `{key}`: enabled applies only to the `review` role"
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
        for (index, ov) in routing
            .overrides
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let n = index + 1;

            let second_opinion = match ov.second_opinion.as_deref() {
                None => None,
                Some(raw) => Some(SecondOpinion::parse(raw).ok_or_else(|| {
                    UpstrokeError::Config {
                        path: repo_path.clone(),
                        message: format!(
                            "[[routing.overrides]] entry {n}: `second_opinion = \"{raw}\"` is not \
                         recognized (accepted: {})",
                            SecondOpinion::ACCEPTED.join(", ")
                        ),
                    }
                })?),
            };

            if ov.start_at.is_none() && second_opinion.is_none() {
                return Err(UpstrokeError::Config {
                    path: repo_path.clone(),
                    message: format!(
                        "[[routing.overrides]] entry {n} has neither `start_at` nor \
                         `second_opinion`, so it would have no effect — give it a tier floor, a \
                         second opinion, or remove it"
                    ),
                });
            }
            let mut builder = GlobSetBuilder::new();
            for pattern in &ov.paths {
                let glob = Glob::new(pattern).map_err(|e| UpstrokeError::Config {
                    path: repo_path.clone(),
                    message: format!("invalid glob `{pattern}` in [[routing.overrides]]: {e}"),
                })?;
                builder.add(glob);
            }
            let globs = builder.build().map_err(|e| UpstrokeError::Config {
                path: repo_path.clone(),
                message: format!("building glob set for [[routing.overrides]]: {e}"),
            })?;
            overrides.push(CompiledOverride {
                raw_paths: ov.paths,
                start_at: ov.start_at,
                second_opinion,
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
            return Err(UpstrokeError::UnknownPinnedModel {
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

        let effort = match pin.effort.as_deref().map(Effort::parse) {
            Some(None) => {
                return Err(UpstrokeError::Config {
                    path: repo_path.clone(),
                    message: format!(
                        "pin for tier `{}` sets effort `{}`, which is not one of: {}",
                        pin.tier,
                        pin.effort.unwrap_or_default(),
                        Effort::KNOWN
                    ),
                });
            }
            Some(effort) => effort,
            None => None,
        };
        pins.push(Pin {
            tier: pin.tier,
            agent: pin.agent,
            model: pin.model,
            effort,
        });
    }

    let gates = parse_gates(raw.gates, &repo_path, limits, warnings)?;
    let runner = parse_runner(raw.runner, &repo_path, limits)?;
    let engine = parse_engine(raw.engine, &repo_path, limits, warnings)?;
    let interaction = parse_interaction(raw.interaction, &repo_path)?;
    let budgets = parse_budgets(raw.budgets, &repo_path)?;

    let pools = read_pools(captured.pools.as_ref(), has_adapter, warnings)?;

    Ok(Config {
        chains,
        overrides,
        pins,
        strategy,
        pools,
        budgets,
        ask_before: interaction.ask_before,
        gates,
        shell: engine.shell,
        review_tier,
        review_enabled,
        review_pass_timeout,
        implementation_effort_override,
        review_effort_override,
        on_task_failure: engine.on_task_failure,
        max_parallel: engine.max_parallel,
        max_merge_repairs: engine.max_merge_repairs,
        max_per_agent: engine.max_per_agent,
        max_per_pool: engine.max_per_pool,
        interaction_mode: interaction.mode,
        notify: interaction.notify,
        wait_on_block: interaction.wait_on_block,
        runner,
    })
}

const RUNNER_KEYS: &str = "`kind`, `image`, `credential_volumes`, `mounts`";

fn parse_runner(
    raw: Option<toml::Value>,
    repo_path: &Path,
    limits: EngineLimits,
) -> Result<RunnerSelection, UpstrokeError> {
    let selection = read_runner(raw, repo_path)?;
    refuse_legacy_container_selection(&selection, repo_path, limits)?;
    Ok(selection)
}

mod parse;
use self::parse::{
    parse_budgets, parse_engine, parse_gates, parse_interaction, parse_role_effort, read_runner,
    refuse_legacy_container_selection,
};

fn repo_config_location(repo_config: Option<&Path>, discover_in: &Path) -> (PathBuf, bool) {
    match repo_config {
        Some(p) => (p.to_path_buf(), true),
        None => (discover_in.join("upstroke.toml"), false),
    }
}

fn pools_location(pools_file: Option<&Path>) -> Option<(PathBuf, bool)> {
    match pools_file {
        Some(p) => Some((p.to_path_buf(), true)),
        None => discovered_pools_path().map(|p| (p, false)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    path: PathBuf,

    required: bool,

    content: Result<Option<Vec<u8>>, (io::ErrorKind, String)>,
}

impl FileSnapshot {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> Result<Option<String>, UpstrokeError> {
        let io_error = |source| UpstrokeError::Io {
            path: self.path.clone(),
            source,
        };
        match &self.content {
            Ok(None) => Ok(None),
            Ok(Some(bytes)) => String::from_utf8(bytes.clone()).map(Some).map_err(|_| {
                io_error(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "stream did not contain valid UTF-8",
                ))
            }),
            Err((kind, message)) => Err(io_error(io::Error::new(*kind, message.clone()))),
        }
    }
}

#[must_use]
pub fn snapshot_file(path: &Path, required: bool) -> FileSnapshot {
    let content = match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err((error.kind(), error.to_string())),
    };
    FileSnapshot {
        path: path.to_path_buf(),
        required,
        content,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedConfig {
    repo: FileSnapshot,

    pools: Option<FileSnapshot>,
}

impl CapturedConfig {
    #[must_use]
    pub fn capture(
        repo_config: Option<&Path>,
        discover_in: &Path,
        pools_file: Option<&Path>,
    ) -> Self {
        let (repo_path, repo_required) = repo_config_location(repo_config, discover_in);
        Self {
            repo: snapshot_file(&repo_path, repo_required),
            pools: pools_location(pools_file)
                .map(|(path, required)| snapshot_file(&path, required)),
        }
    }

    pub fn files(&self) -> impl Iterator<Item = &FileSnapshot> {
        std::iter::once(&self.repo).chain(self.pools.as_ref())
    }
}

mod read;
use self::read::{read_pools, read_repo_config};

fn discovered_pools_path() -> Option<PathBuf> {
    Some(util::user_upstroke_dir()?.join("pools.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::OnceLock;

    fn scratch(name: &str, content: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("upstroke-config-tests-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join(name);
        fs::write(&path, content).expect("write scratch file");
        path
    }

    fn missing() -> PathBuf {
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let dir =
                env::temp_dir().join(format!("upstroke-config-nopools-{}", std::process::id()));
            fs::create_dir_all(&dir).expect("scratch dir");
            let path = dir.join("pools.toml");
            fs::write(
                &path,
                "# no pools
",
            )
            .expect("empty pools file");
            path
        })
        .clone()
    }

    fn hermetic() -> PathBuf {
        let dir = env::temp_dir().join(format!("upstroke-config-hermetic-{}", std::process::id()));
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
        assert!(cfg.pools.is_empty());
        assert_eq!(cfg.review_pass_timeout, DEFAULT_REVIEW_PASS_TIMEOUT);
    }

    #[test]
    fn explicit_config_path_must_exist() {
        let mut warnings = Vec::new();
        let absent = env::temp_dir()
            .join("upstroke-definitely-missing")
            .join("upstroke.toml");
        let err = load(Some(&absent), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("missing --config errors");
        assert!(matches!(err, UpstrokeError::Config { .. }));
    }

    #[test]
    fn a_misspelled_top_level_section_is_refused_not_dropped() {
        // Each of these used to deserialize into nothing: the whole section vanished and its
        // defaults took effect while `validate` reported a clean file (SWEEP-CONFIG-PARSE-007).
        for (name, body, typo) in [
            (
                "misspelled-budgets.toml",
                "[budgts]\nrun_usd = 15.0\n",
                "budgts",
            ),
            (
                "misspelled-interaction.toml",
                "[interation]\nmode = \"never\"\n",
                "interation",
            ),
            (
                "misspelled-runner.toml",
                "[runer]\nkind = \"container\"\n",
                "runer",
            ),
        ] {
            let path = scratch(name, body);
            let mut warnings = Vec::new();
            let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
                .expect_err("an unknown top-level section is a typo, not silence");
            assert!(matches!(err, UpstrokeError::Config { .. }), "{typo}: {err}");
            let message = err.to_string();
            assert!(
                message.contains(typo),
                "the refusal names the misspelled section: {message}"
            );
            assert!(
                message.contains("`runner`") && message.contains("`budgets`"),
                "the refusal lists the accepted sections: {message}"
            );
            assert!(
                warnings.is_empty(),
                "{typo}: this is a refusal, not a degraded warning"
            );
        }
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
review = { tier = "frontier", timeout_secs = 7200 }

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
        assert_eq!(cfg.overrides[0].start_at, Some(Tier::Frontier));
        assert_eq!(
            cfg.overrides[0].second_opinion,
            Some(SecondOpinion::DifferentVendor)
        );
        assert_eq!(cfg.pins.len(), 1);
        assert_eq!(cfg.strategy.mode, "value-max");
        assert_eq!(cfg.strategy.spend_down_after, Some(0.7));

        assert_eq!(cfg.review_tier, Some(Tier::Frontier));
        assert_eq!(cfg.review_pass_timeout, Duration::from_secs(7200));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn an_override_may_ask_for_a_second_opinion_without_raising_the_floor() {
        let path = scratch(
            "soonly.toml",
            "[[routing.overrides]]\npaths = [\"docs/**\"]\nsecond_opinion = \
             \"different-vendor\"\n",
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.overrides[0].start_at, None);
        assert_eq!(
            cfg.overrides[0].second_opinion,
            Some(SecondOpinion::DifferentVendor)
        );

        assert_eq!(
            cfg.chain_for(TaskKind::Fix).chain,
            vec![Tier::Small, Tier::Mid, Tier::Frontier]
        );
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn a_misspelled_second_opinion_is_a_hard_error() {
        let path = scratch(
            "badso.toml",
            "[[routing.overrides]]\npaths = [\"src/auth/**\"]\nstart_at = \"frontier\"\n\
             second_opinion = \"different-model\"\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("unknown second_opinion must error");
        let msg = err.to_string();
        assert!(msg.contains("different-model"), "names the typo: {msg}");
        assert!(
            msg.contains("different-vendor"),
            "lists what is accepted: {msg}"
        );
    }

    #[test]
    fn misspelled_second_opinion_key_is_a_hard_error_even_with_start_at() {
        let path = scratch(
            "bad-so-key.toml",
            "[[routing.overrides]]\npaths = [\"src/auth/**\"]\nstart_at = \"frontier\"\n\
             second_opinon = \"different-vendor\"\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("an unknown override key must not silently remove a reviewer");
        let msg = err.to_string();
        assert!(
            msg.contains("second_opinon"),
            "names the misspelled key: {msg}"
        );
        assert!(
            msg.contains("second_opinion"),
            "lists the accepted spelling: {msg}"
        );
    }

    #[test]
    fn an_override_that_does_nothing_is_a_hard_error() {
        let path = scratch(
            "emptyov.toml",
            "[[routing.overrides]]\npaths = [\"src/**\"]\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("an inert override must error");
        assert!(err.to_string().contains("no effect"), "got: {err}");
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
    fn review_timeout_must_be_positive_and_is_review_only() {
        let zero = scratch(
            "zeroreviewtimeout.toml",
            "[routing]\nreview = { tier = \"frontier\", timeout_secs = 0 }\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&zero), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("zero review timeout must error");
        assert!(err.to_string().contains("timeout_secs must be at least 1"));

        let misplaced = scratch(
            "workerreviewtimeout.toml",
            "[routing]\nfix = { chain = [\"small\"], timeout_secs = 5400 }\n",
        );
        let err = load(
            Some(&misplaced),
            &hermetic(),
            Some(&missing()),
            &mut warnings,
        )
        .expect_err("review timeout on a task kind must error");
        assert!(
            err.to_string()
                .contains("applies only to the `review` role")
        );

        let misspelled = scratch(
            "misspelledreviewtimeout.toml",
            "[routing]\nreview = { tier = \"frontier\", timeout_sec = 60 }\n",
        );
        let err = load(
            Some(&misspelled),
            &hermetic(),
            Some(&missing()),
            &mut warnings,
        )
        .expect_err("an unknown review-routing key must not fall back to 5400 seconds");
        let message = err.to_string();
        assert!(message.contains("timeout_sec"), "names the typo: {message}");
        assert!(
            message.contains("timeout_secs"),
            "names the accepted key: {message}"
        );
    }

    #[test]
    fn routing_role_fields_are_rejected_in_the_wrong_entry() {
        let review = scratch(
            "reviewattempts.toml",
            "[routing]\nreview = { tier = \"frontier\", attempts_per = 2 }\n",
        );
        let mut warnings = Vec::new();
        let error = load(Some(&review), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("review must not silently ignore task retry policy");
        assert!(
            error
                .to_string()
                .contains("applies only to task-kind roles"),
            "{error}"
        );

        let task = scratch(
            "taskenabled.toml",
            "[routing]\nfix = { chain = [\"small\"], enabled = false }\n",
        );
        let error = load(Some(&task), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("task routing must not silently ignore review-only enablement");
        assert!(
            error
                .to_string()
                .contains("applies only to the `review` role"),
            "{error}"
        );
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
    fn misspelled_pin_effort_key_is_a_hard_error() {
        let path = scratch(
            "misspelledpineffort.toml",
            "[[pins]]\ntier = \"frontier\"\nagent = \"claude-code\"\nmodel = \
             \"claude-opus-5\"\neffrot = \"max\"\n",
        );
        let mut warnings = Vec::new();
        let error = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("a misspelled pin effort must not fall back to the tier default");
        let message = error.to_string();
        assert!(message.contains("effrot"), "names the typo: {message}");
        assert!(
            message.contains("effort"),
            "names the accepted key: {message}"
        );
    }

    #[test]
    fn effort_defaults_by_tier_and_a_pin_overrides_it() {
        let path = scratch(
            "effortpin.toml",
            "[[pins]]\ntier = \"frontier\"\nagent = \"claude-code\"\nmodel = \"claude-opus-5\"\n\
             effort = \"max\"\n",
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.effort_for(Tier::Small), Effort::Low);
        assert_eq!(cfg.effort_for(Tier::Mid), Effort::Medium);

        assert_eq!(cfg.effort_for(Tier::Frontier), Effort::Max);
        assert_eq!(cfg.implementation_effort(Tier::Frontier), Effort::Max);

        assert_eq!(cfg.review_effort(), Effort::Max);
    }

    #[test]
    fn role_effort_policy_overrides_pin_and_tier_defaults_independently() {
        let path = scratch(
            "roleeffort.toml",
            r#"
[routing]
review = { tier = "small" }

[routing.effort]
implementation = "xhigh"
review = "max"

[[pins]]
tier = "small"
agent = "claude-code"
model = "claude-haiku-4-5"
effort = "low"
"#,
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");

        assert_eq!(
            cfg.effort_for(Tier::Small),
            Effort::Low,
            "pin default remains intact"
        );
        for tier in [Tier::Small, Tier::Mid, Tier::Frontier] {
            assert_eq!(
                cfg.implementation_effort(tier),
                Effort::XHigh,
                "the implementation role policy is global across tiers"
            );
        }
        assert_eq!(
            cfg.review_effort(),
            Effort::Max,
            "review policy outranks its small tier and low pin"
        );
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn the_repository_self_host_policy_is_frontier_only_with_fixed_role_effort() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("upstroke.toml");
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &root, Some(&missing()), &mut warnings)
            .expect("the checked-in self-host config loads");

        for kind in TaskKind::ALL {
            let chain = cfg.chain_for(kind);
            assert_eq!(
                chain.chain,
                [Tier::Frontier],
                "{kind} must not fall back to a cheaper implementation model"
            );
            assert!(
                chain.from_config,
                "{kind} must be explicit repository policy"
            );
        }
        assert_eq!(cfg.review_tier, Some(Tier::Frontier));
        assert!(cfg.review_enabled);
        assert_eq!(
            cfg.review_pass_timeout,
            Duration::from_secs(5400),
            "self-hosted max reviews get a full independent 90-minute pass"
        );
        let effort = cfg.resolved_effort_policy();
        assert_eq!(
            [effort.small, effort.mid, effort.frontier],
            [Effort::XHigh; 3]
        );
        assert_eq!(effort.review, Effort::Max);

        let pin = cfg
            .pins
            .iter()
            .find(|pin| pin.tier == Tier::Frontier)
            .expect("frontier identity is pinned for reproducible self-hosting");
        assert_eq!(
            (pin.agent.as_str(), pin.model.as_str()),
            ("codex", "gpt-5.6-sol")
        );
        assert_eq!(
            catalog::lookup(&pin.agent, &pin.model).map(|entry| entry.tier),
            Some(Tier::Frontier)
        );
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn role_effort_typos_are_config_errors_before_an_attempt_starts() {
        let path = scratch(
            "badroleeffort.toml",
            "[routing.effort]\nimplementation = \"ultra\"\nreview = \"max\"\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("an unsupported role effort must error");
        let msg = err.to_string();
        assert!(msg.contains("implementation"), "names the role: {msg}");
        assert!(msg.contains("ultra"), "names what was written: {msg}");
        assert!(msg.contains(Effort::KNOWN), "lists valid values: {msg}");

        let path = scratch(
            "badrolekey.toml",
            "[routing.effort]\nimplementer = \"xhigh\"\n",
        );
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("an unknown role key must error");
        let msg = err.to_string();
        assert!(msg.contains("implementer"), "names the typo: {msg}");
        assert!(
            msg.contains("implementation"),
            "names the accepted role: {msg}"
        );
    }

    #[test]
    fn a_misspelled_effort_is_a_config_error_not_a_burned_attempt() {
        let path = scratch(
            "badeffort.toml",
            "[[pins]]\ntier = \"mid\"\nagent = \"claude-code\"\nmodel = \"claude-sonnet-5\"\n\
             effort = \"maximum\"\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("an unknown effort must error");
        let msg = err.to_string();
        assert!(msg.contains("maximum"), "names what was written: {msg}");
        assert!(msg.contains(Effort::KNOWN), "lists valid: {msg}");
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
model = "gpt-5.3-codex"
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
            cfg.pools
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["claude-max", "copilot"]
        );
    }

    #[test]
    fn every_pool_key_parses_into_the_shape_the_estimator_reads() {
        let path = scratch(
            "fullpools.toml",
            r#"
[pools.claude-max]
kind = "subscription-window"
agent = "claude-code"
window = "5h"
weekly = true
sources = ["signals", "self", "local-logs"]
safety_margin = 0.25
reserve = 0.10
profile = "personal"

[pools.claude-max-work]
kind = "subscription-window"
agent = "claude-code"
profile = "work"

[pools.copilot]
kind = "credits"
agent = "copilot"
sources = ["signals", "self"]
monthly_allowance = 300
"#,
        );
        let mut warnings = Vec::new();
        let cfg = load(None, &hermetic(), Some(&path), &mut warnings).expect("load pools");
        assert!(warnings.is_empty(), "warnings: {warnings:?}");

        let max = &cfg.pools[0];
        assert_eq!(max.kind, PoolKind::SubscriptionWindow);
        assert_eq!(max.window, Some(Duration::from_secs(5 * 3600)));
        assert!(max.weekly);
        assert_eq!(
            max.sources,
            [Source::Signals, Source::SelfMetered, Source::LocalLogs]
        );
        assert_eq!(max.safety_margin, 0.25);
        assert_eq!(max.reserve, 0.10);
        assert_eq!(max.monthly_allowance, Allowance::Auto);
        assert!(max.usable);

        assert_eq!(max.profile.as_deref(), Some("personal"));
        assert_eq!(cfg.pools[1].profile.as_deref(), Some("work"));
        assert_eq!(
            capacity::pool_for("claude-code", &cfg.pools).map(|p| p.name.as_str()),
            Some("claude-max"),
            "first match in file order wins"
        );

        assert_eq!(cfg.pools[2].kind, PoolKind::Credits);
        assert_eq!(cfg.pools[2].monthly_allowance, Allowance::Units(300.0));
    }

    #[test]
    fn pool_mistakes_error_where_they_would_change_the_estimate_and_warn_where_they_degrade_it() {
        let mut warnings = Vec::new();
        let load_pools = |name: &str, body: &str, warnings: &mut Vec<String>| {
            let path = scratch(name, body);
            load(None, &hermetic(), Some(&path), warnings)
        };

        let err = load_pools(
            "badkind.toml",
            "[pools.p]\nkind = \"subscription\"\nagent = \"claude-code\"\n",
            &mut warnings,
        )
        .expect_err("unknown kind must error");
        assert!(
            err.to_string().contains("subscription-window"),
            "lists what is accepted: {err}"
        );

        let err = load_pools(
            "badsource.toml",
            "[pools.p]\nkind = \"credits\"\nagent = \"copilot\"\nsources = [\"signal\"]\n",
            &mut warnings,
        )
        .expect_err("unknown source must error");
        assert!(err.to_string().contains("signals"), "got: {err}");

        for bad in ["safety_margin = 1.5", "reserve = -0.2"] {
            let err = load_pools(
                "badfraction.toml",
                &format!("[pools.p]\nkind = \"credits\"\nagent = \"copilot\"\n{bad}\n"),
                &mut warnings,
            )
            .expect_err("an out-of-range fraction must error");
            assert!(err.to_string().contains("fraction"), "got: {err}");
        }

        let err = load_pools(
            "badwindow.toml",
            "[pools.p]\nkind = \"subscription-window\"\nagent = \"claude-code\"\nwindow = \
             \"five hours\"\n",
            &mut warnings,
        )
        .expect_err("an unparseable window must error");
        assert!(err.to_string().contains("duration"), "got: {err}");

        warnings.clear();
        let cfg = load_pools(
            "aider.toml",
            "[pools.local]\nkind = \"unmetered\"\nagent = \"aider\"\nendpoint = \
             \"http://homeserver:11434/v1\"\nbogus = 1\n",
            &mut warnings,
        )
        .expect("a pool for an agent this build cannot drive is still a pool");
        assert_eq!(cfg.pools.len(), 1);
        assert!(!cfg.pools[0].usable);
        assert_eq!(
            cfg.pools[0].endpoint.as_deref(),
            Some("http://homeserver:11434/v1")
        );
        assert!(
            warnings.iter().any(|w| w.contains("no adapter")),
            "warnings: {warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("bogus")),
            "an unknown key warns by name: {warnings:?}"
        );
    }

    #[test]
    fn wrong_section_shapes_get_actionable_errors() {
        let path = scratch("gatestable.toml", "[gates]\ncheck = \"cargo check\"\n");
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("table shape must error");
        let msg = err.to_string();
        assert!(msg.contains("[[gates]]"), "names the expected shape: {msg}");

        let path = scratch(
            "gatestype.toml",
            "[[gates]]\nname = \"t\"\ncmd = \"cargo test\"\ntimeout_secs = \"600\"\n",
        );
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("string timeout must error");
        assert!(err.to_string().contains("timeout_secs"), "got: {err}");

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
            repo_root.join("upstroke.toml"),
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
        assert_eq!(cfg.wait_on_block, DEFAULT_WAIT_ON_BLOCK);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn wait_on_block_is_configurable_and_zero_means_do_not_wait() {
        let path = scratch("wait.toml", "[interaction]\nwait_on_block_secs = 90\n");
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.wait_on_block, Duration::from_secs(90));

        let path = scratch("nowait.toml", "[interaction]\nwait_on_block_secs = 0\n");
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.wait_on_block, Duration::ZERO);
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
wait_on_block_secs = 120
ask_before = { frontier_escalation_over_usd = 5.0 }
"#,
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.interaction_mode, InteractionMode::Never);
        assert_eq!(cfg.notify, ["cli", "desktop"]);
        assert_eq!(cfg.wait_on_block, Duration::from_secs(120));
        assert_eq!(cfg.on_task_failure, OnTaskFailure::Continue);

        assert_eq!(cfg.ask_before.frontier_escalation_over_usd, Some(5.0));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn a_misspelled_ask_before_key_is_a_hard_error() {
        let path = scratch(
            "badask.toml",
            "[interaction]\nask_before = { frontier_escalation_over_usdd = 5.0 }\n",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("an unknown ask_before key must error");
        let msg = err.to_string();
        assert!(msg.contains("ask_before"), "names the section: {msg}");
        assert!(
            msg.contains("frontier_escalation_over_usd"),
            "lists what is accepted: {msg}"
        );
    }

    #[test]
    fn budgets_parse_and_a_meaningless_ceiling_is_refused() {
        let path = scratch(
            "budgets.toml",
            "[budgets]\nrun_usd = 15.0\ntask_usd = 4.0\n",
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.budgets.run_usd, Some(15.0));
        assert_eq!(cfg.budgets.task_usd, Some(4.0));
        assert!(cfg.budgets.any());

        for bad in ["run_usd = 0.0", "task_usd = -1.0"] {
            let path = scratch("badbudget.toml", &format!("[budgets]\n{bad}\n"));
            let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
                .expect_err("a non-positive ceiling must error");
            assert!(err.to_string().contains("ceiling"), "got: {err}");
        }

        let cfg = load(None, &hermetic(), Some(&missing()), &mut warnings).expect("defaults");
        assert!(!cfg.budgets.any());
    }

    #[test]
    fn misspelled_mode_or_failure_policy_is_a_hard_error() {
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

    #[test]
    fn pools_keep_the_order_they_were_written_in() {
        let path = scratch(
            "orderpools.toml",
            "[pools.work]
kind = \"subscription-window\"
agent = \"claude-code\"

             [pools.personal]
kind = \"subscription-window\"
agent = \"claude-code\"
",
        );
        let mut warnings = Vec::new();
        let cfg = load(None, &hermetic(), Some(&path), &mut warnings).expect("load");
        assert_eq!(
            cfg.pools
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["work", "personal"],
            "file order, not alphabetical"
        );
        assert_eq!(
            capacity::pool_for("claude-code", &cfg.pools).map(|p| p.name.as_str()),
            Some("work"),
            "the first pool in the FILE is the preferred one"
        );
    }

    #[test]
    fn an_unbuilt_budget_key_is_refused_rather_than_ignored() {
        let path = scratch(
            "poolbudget.toml",
            "[budgets]
run_usd = 10.0
pool_fraction = 0.5
",
        );
        let mut warnings = Vec::new();
        let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
            .expect_err("an unknown budget key must not pass");
        assert!(err.to_string().contains("pool_fraction"), "got: {err}");
    }

    #[test]
    fn an_explicit_pools_path_that_does_not_exist_is_a_typo_not_an_empty_machine() {
        let absent = env::temp_dir()
            .join("upstroke-definitely-missing")
            .join("pools.toml");
        let mut warnings = Vec::new();
        let err = load(None, &hermetic(), Some(&absent), &mut warnings)
            .expect_err("an explicit pools path must exist");
        assert!(
            err.to_string().contains("pools file not found"),
            "got: {err}"
        );
    }

    #[test]
    fn engine_limits_default_when_nothing_configures_them() {
        let mut warnings = Vec::new();
        let cfg = load(None, &hermetic(), Some(&missing()), &mut warnings).expect("defaults");
        assert_eq!(cfg.max_parallel, DEFAULT_MAX_PARALLEL);
        assert_eq!(cfg.max_merge_repairs, DEFAULT_MAX_MERGE_REPAIRS);
        assert_eq!(cfg.max_per_agent, DEFAULT_MAX_PARALLEL);
        assert_eq!(cfg.max_per_pool, DEFAULT_MAX_PARALLEL);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");

        let path = scratch("engineshellonly.toml", "[engine]\nshell = \"bash\"\n");
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.max_parallel, DEFAULT_MAX_PARALLEL);
        assert_eq!(cfg.max_merge_repairs, DEFAULT_MAX_MERGE_REPAIRS);
        assert_eq!(cfg.max_per_agent, DEFAULT_MAX_PARALLEL);
        assert_eq!(cfg.max_per_pool, DEFAULT_MAX_PARALLEL);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn max_parallel_above_one_is_refused_rather_than_read_past() {
        let mut warnings = Vec::new();
        for parallel in [2u32, 4, 64] {
            let path = scratch(
                "manyparallel.toml",
                &format!("[engine]\nmax_parallel = {parallel}\n"),
            );
            let error = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
                .expect_err("a ceiling this engine cannot honour must not load");
            let message = error.to_string();
            assert!(
                message.contains(&format!("max_parallel = {parallel}")),
                "names what was written: {message}"
            );
            assert!(
                message.contains("max_parallel = 1"),
                "names the one accepted value: {message}"
            );
        }

        warnings.clear();
        let path = scratch("oneparallel.toml", "[engine]\nmax_parallel = 1\n");
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.max_parallel, 1);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn a_sequential_resume_warns_about_an_impossible_ceiling_rather_than_refusing_it() {
        let path = scratch("resumeparallel.toml", "[engine]\nmax_parallel = 3\n");
        let mut warnings = Vec::new();
        let cfg = load_limits(
            Some(&path),
            &hermetic(),
            Some(&missing()),
            EngineLimits::SequentialResume,
            &mut warnings,
        )
        .expect("a legacy run must stay reachable");
        assert_eq!(
            cfg.max_parallel, DEFAULT_MAX_PARALLEL,
            "and the ceiling it continues on is its own, not the file's"
        );
        assert!(
            warnings.iter().any(|warning| {
                warning.contains("max_parallel = 3") && warning.contains("not acted on")
            }),
            "the value is named and disowned: {warnings:?}"
        );

        let mut fresh_warnings = Vec::new();
        load_limits(
            Some(&path),
            &hermetic(),
            Some(&missing()),
            EngineLimits::Fresh,
            &mut fresh_warnings,
        )
        .expect_err("a run being created now must still refuse it");

        for key in [
            "max_parallel",
            "max_merge_repairs",
            "max_per_agent",
            "max_per_pool",
        ] {
            let path = scratch("resumezero.toml", &format!("[engine]\n{key} = 0\n"));
            let error = load_limits(
                Some(&path),
                &hermetic(),
                Some(&missing()),
                EngineLimits::SequentialResume,
                &mut warnings,
            )
            .expect_err("a zero limit must error on a resume too");
            assert!(error.to_string().contains(key), "names the key: {error}");
        }
    }

    #[test]
    fn a_sequential_resume_announces_gate_shapes_a_fresh_run_refuses() {
        let path = scratch(
            "resumegates.toml",
            "[[gates]]\nname = \"check\"\ncmd = \"cargo check\"\ntimeout_sec = 900\n\
             [[gates]]\nname = \"Check\"\ncmd = \"cargo clippy\"\n",
        );
        for limits in [EngineLimits::Fresh, EngineLimits::SequentialResume] {
            let mut strict_warnings = Vec::new();
            let error = load_limits(
                Some(&path),
                &hermetic(),
                Some(&missing()),
                limits,
                &mut strict_warnings,
            )
            .expect_err("a run whose gates come from this file refuses it");
            assert!(
                error.to_string().contains("[[gates]] entry 1"),
                "{limits:?}: the first refusal is the unknown key, in entry order: {error}"
            );
            assert!(
                strict_warnings.is_empty(),
                "{limits:?}: {strict_warnings:?}"
            );
        }

        let mut warnings = Vec::new();
        let cfg = load_limits(
            Some(&path),
            &hermetic(),
            Some(&missing()),
            EngineLimits::SequentialResumeWithRecordedGates,
            &mut warnings,
        )
        .expect("a recorded run stays reachable through the same file");
        let gates = cfg.gates.expect("the section was present");
        assert_eq!(
            gates
                .iter()
                .map(|gate| gate.name.as_str())
                .collect::<Vec<_>>(),
            vec!["check", "Check"],
            "both entries are kept, so the record can be compared with them"
        );
        assert_eq!(
            gates.first().map(|gate| gate.timeout),
            Some(DEFAULT_GATE_TIMEOUT),
            "the misspelled timeout bought nothing"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("`timeout_sec`") && w.contains("recorded")),
            "the unknown key is named and the recorded gates are named as what runs: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("entry 2 repeats the name `Check`") && w.contains("recorded")),
            "the repeated name is named the same way: {warnings:?}"
        );
    }

    #[test]
    fn the_engine_limit_reading_follows_the_schema_the_run_recorded() {
        for schema in 1..=LAST_SEQUENTIAL_SCHEMA {
            assert_eq!(
                EngineLimits::for_resume(schema, true),
                EngineLimits::SequentialResumeWithRecordedGates,
                "schema {schema} with a gate record compares today's section with it"
            );

            assert_eq!(
                EngineLimits::for_resume(schema, false),
                EngineLimits::SequentialResume,
                "schema {schema} without a gate record settles its gates from today's file"
            );
        }
        for gates_recorded in [true, false] {
            assert_eq!(
                EngineLimits::for_resume(LAST_SEQUENTIAL_SCHEMA + 1, gates_recorded),
                EngineLimits::Fresh
            );
        }
    }

    #[test]
    fn zero_and_non_integer_engine_limits_are_config_errors() {
        let mut warnings = Vec::new();
        for key in [
            "max_parallel",
            "max_merge_repairs",
            "max_per_agent",
            "max_per_pool",
        ] {
            let path = scratch("zerolimit.toml", &format!("[engine]\n{key} = 0\n"));
            let error = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
                .expect_err("a zero limit must error");
            let message = error.to_string();
            assert!(message.contains(key), "names the key: {message}");
            assert!(
                message.contains("at least 1"),
                "says what is acceptable: {message}"
            );
        }

        for body in [
            "max_parallel = \"1\"",
            "max_merge_repairs = 1.5",
            "max_per_agent = -1",
            "max_per_pool = true",
        ] {
            let path = scratch("shapelimit.toml", &format!("[engine]\n{body}\n"));
            let error = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
                .expect_err("a non-integer limit must error");
            assert!(
                error.to_string().contains("[engine]"),
                "names the section: {error}"
            );
        }
    }

    #[test]
    fn an_unknown_engine_key_is_refused_by_name_instead_of_vanishing() {
        let path = scratch(
            "unknownengine.toml",
            "[engine]\nmax_paralel = 4\non_task_failur = \"continue\"\n",
        );
        for limits in [
            EngineLimits::Fresh,
            EngineLimits::SequentialResume,
            EngineLimits::SequentialResumeWithRecordedGates,
        ] {
            let mut warnings = Vec::new();
            let error = load_limits(
                Some(&path),
                &hermetic(),
                Some(&missing()),
                limits,
                &mut warnings,
            )
            .expect_err("an unknown [engine] key must refuse the load");
            let message = error.to_string();
            assert!(message.contains("`max_paralel`"), "{limits:?}: {message}");
            assert!(
                message.contains("`on_task_failur`"),
                "{limits:?}: every unknown key is named: {message}"
            );
            assert!(
                message.contains("[engine]"),
                "{limits:?}: located: {message}"
            );
            assert!(
                message.contains("`on_task_failure`"),
                "{limits:?}: the accepted keys are listed: {message}"
            );
            assert!(warnings.is_empty(), "{limits:?}: {warnings:?}");
        }
    }

    #[test]
    fn topology_only_limits_are_kept_and_announced_as_inert() {
        let path = scratch(
            "topologylimits.toml",
            "[engine]\nmax_merge_repairs = 5\nmax_per_agent = 3\nmax_per_pool = 2\n",
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.max_merge_repairs, 5);
        assert_eq!(cfg.max_per_agent, 3);
        assert_eq!(cfg.max_per_pool, 2);
        for key in ["max_merge_repairs", "max_per_agent", "max_per_pool"] {
            assert!(
                warnings
                    .iter()
                    .any(|w| w.contains(key) && w.contains("not acted on")),
                "`{key}` must say it is not acted on: {warnings:?}"
            );
        }

        warnings.clear();
        let path = scratch(
            "defaultlimits.toml",
            "[engine]\nmax_parallel = 1\nmax_merge_repairs = 2\nmax_per_agent = 1\n\
             max_per_pool = 1\n",
        );
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.max_merge_repairs, DEFAULT_MAX_MERGE_REPAIRS);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn the_new_engine_limits_sit_beside_the_keys_that_already_worked() {
        let path = scratch(
            "engineall.toml",
            "[engine]\nshell = \"powershell\"\non_task_failure = \"continue\"\n\
             max_parallel = 1\nmax_merge_repairs = 3\n",
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.shell, ShellKind::PowerShell);
        assert_eq!(cfg.on_task_failure, OnTaskFailure::Continue);
        assert_eq!(cfg.max_merge_repairs, 3);
        assert_eq!(
            warnings.len(),
            1,
            "only the inert repair ceiling is announced: {warnings:?}"
        );
        assert!(warnings[0].contains("max_merge_repairs"), "{warnings:?}");
    }

    #[test]
    fn a_load_validates_the_captured_bytes_and_not_a_second_read_of_the_file() {
        let refusing = "[engine]\nmax_parallel = 3\n";
        let accepted = "[engine]\nmax_merge_repairs = 4\n";
        let path = scratch("abarefusing.toml", refusing);
        let captured = CapturedConfig::capture(Some(&path), &hermetic(), Some(&missing()));

        fs::write(&path, accepted).expect("B, for the length of the validation");
        let mut warnings = Vec::new();
        let error = load_captured(&captured, EngineLimits::Fresh, &mut warnings)
            .expect_err("the captured bytes are the ones that had to be validated");
        assert!(
            error.to_string().contains("max_parallel = 3"),
            "the transient file was validated in place of the captured one: {error}"
        );

        fs::write(&path, refusing).expect("A restored");
        assert_eq!(
            CapturedConfig::capture(Some(&path), &hermetic(), Some(&missing())),
            captured,
            "the excursion is invisible to the confirmation, which is why the \
             validation is what had to see it"
        );

        let path = scratch("abaaccepted.toml", accepted);
        let captured = CapturedConfig::capture(Some(&path), &hermetic(), Some(&missing()));
        fs::write(&path, refusing).expect("B, for the length of the validation");
        let cfg = load_captured(&captured, EngineLimits::Fresh, &mut warnings)
            .expect("the captured config is loadable, whatever the file says now");
        assert_eq!(cfg.max_merge_repairs, 4, "and it is the captured one");
        fs::write(&path, accepted).expect("A restored");
        assert_eq!(
            CapturedConfig::capture(Some(&path), &hermetic(), Some(&missing())),
            captured
        );
    }

    #[test]
    fn a_capture_covers_the_pools_file_as_well_as_the_repo_config() {
        let repo = scratch("capturedpools-config.toml", "[engine]\nshell = \"bash\"\n");
        let pools = scratch(
            "capturedpools-pools.toml",
            "[pools.one]\nkind = \"subscription-window\"\nagent = \"claude-code\"\n",
        );
        let captured = CapturedConfig::capture(Some(&repo), &hermetic(), Some(&pools));
        assert_eq!(
            captured.files().map(FileSnapshot::path).collect::<Vec<_>>(),
            vec![repo.as_path(), pools.as_path()]
        );

        fs::write(
            &pools,
            "[pools.two]\nkind = \"subscription-window\"\nagent = \"claude-code\"\n",
        )
        .expect("the transient pools file");
        let mut warnings = Vec::new();
        let cfg = load_captured(&captured, EngineLimits::Fresh, &mut warnings).expect("load");
        assert_eq!(
            cfg.pools
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["one"],
            "the pools that were captured are the pools that were parsed"
        );
    }

    #[test]
    fn a_blank_pool_name_is_refused() {
        let path = scratch(
            "blankname.toml",
            "[pools.\"\"]
kind = \"credits\"
agent = \"copilot\"
",
        );
        let mut warnings = Vec::new();
        let err = load(None, &hermetic(), Some(&path), &mut warnings)
            .expect_err("a blank pool name must error");
        assert!(err.to_string().contains("non-empty name"), "got: {err}");
    }

    #[test]
    fn an_absent_runner_section_is_the_unconfigured_host_runner() {
        let mut warnings = Vec::new();
        let cfg = load(None, &hermetic(), Some(&missing()), &mut warnings).expect("defaults");
        assert_eq!(cfg.runner.kind, RunnerKind::Host);
        assert_eq!(cfg.runner.image, None);
        assert!(cfg.runner.credential_volumes.is_empty());
        assert!(cfg.runner.mounts.is_empty());
        assert!(
            !cfg.runner.from_config,
            "an absent section reported itself as configured"
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn the_runner_section_parses_kind_image_volumes_and_mounts() {
        let raw: toml::Value = toml::from_str(
            r#"
kind = "container"
image = "upstroke/ci:3.2"
credential_volumes = { claude-code = "creds-cc", codex = "creds-cx" }
mounts = [
  { source = "/opt/toolchain", target = "/opt/toolchain" },
  { source = "/var/cache/models", target = "/models", read_only = false },
]
"#,
        )
        .expect("fixture parses as toml");
        let selection = read_runner(Some(raw), Path::new("upstroke.toml")).expect("parses");

        assert_eq!(selection.kind, RunnerKind::Container);
        assert_eq!(selection.image.as_deref(), Some("upstroke/ci:3.2"));
        assert_eq!(
            selection.credential_volumes,
            [
                ("claude-code".to_owned(), "creds-cc".to_owned()),
                ("codex".to_owned(), "creds-cx".to_owned()),
            ]
            .into_iter()
            .collect::<BTreeMap<_, _>>()
        );
        assert_eq!(
            selection.mounts,
            vec![
                RunnerMount {
                    source: PathBuf::from("/opt/toolchain"),
                    target: "/opt/toolchain".to_owned(),

                    read_only: true,
                },
                RunnerMount {
                    source: PathBuf::from("/var/cache/models"),
                    target: "/models".to_owned(),
                    read_only: false,
                },
            ]
        );
        assert!(selection.from_config);

        assert_eq!(
            selection
                .mounts
                .iter()
                .map(|mount| mount.read_only)
                .collect::<Vec<_>>(),
            vec![true, false]
        );
    }

    #[test]
    fn the_runner_section_refuses_every_shape_it_cannot_act_on() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "unknown key",
                "knid = \"container\"\n",
                "unknown key `knid` in [runner]",
            ),
            (
                "unknown kind",
                "kind = \"vm\"\n",
                "[runner] `kind = \"vm\"` is not recognized",
            ),
            (
                "host with an image",
                "kind = \"host\"\nimage = \"upstroke/ci:3.2\"\n",
                "[runner] `kind = \"host\"` with `image`",
            ),
            (
                "host with volumes and mounts",
                "credential_volumes = { claude-code = \"c\" }\nmounts = []\n",
                "with `credential_volumes`, `mounts`",
            ),
            (
                "container without an image",
                "kind = \"container\"\n",
                "[runner] `kind = \"container\"` without `image`",
            ),
            (
                "empty image",
                "kind = \"container\"\nimage = \"  \"\n",
                "[runner] `image` is empty",
            ),
            (
                "empty volume name",
                "kind = \"container\"\nimage = \"i\"\ncredential_volumes = { claude-code = \"\" }\n",
                "both the agent id and the volume name must be non-empty",
            ),
            (
                "empty mount target",
                "kind = \"container\"\nimage = \"i\"\nmounts = [{ source = \"/a\", target = \"\" }]\n",
                "a mount has an empty `target`",
            ),
            (
                "empty mount source",
                "kind = \"container\"\nimage = \"i\"\nmounts = [{ source = \"\", target = \"/b\" }]\n",
                "the mount at `/b` has an empty `source`",
            ),
            (
                "unknown mount key",
                "kind = \"container\"\nimage = \"i\"\nmounts = [{ source = \"/a\", target = \"/b\", ro = true }]\n",
                "[runner]:",
            ),
            ("not a table", "", "[runner]:"),
        ];

        let mut refused = 0;
        for (label, body, needle) in cases {
            let raw: toml::Value = if *label == "not a table" {
                toml::Value::String("container".to_owned())
            } else {
                toml::from_str(body).expect("fixture parses as toml")
            };
            let error = read_runner(Some(raw), Path::new("upstroke.toml"))
                .expect_err("this shape is refused");
            let UpstrokeError::Config { message, .. } = &error else {
                panic!("`{label}`: refused as {error:?}, not as a config error");
            };
            assert!(
                message.contains(needle),
                "`{label}`: the message does not name the problem: {message}"
            );
            refused += 1;
        }
        assert_eq!(refused, cases.len(), "every shape was driven");

        let ok: toml::Value =
            toml::from_str("kind = \"container\"\nimage = \"upstroke/ci:3.2\"\n").expect("toml");
        read_runner(Some(ok), Path::new("upstroke.toml")).expect("the base shape is accepted");
    }

    #[test]
    fn a_container_section_parses_into_the_selection_resolution_consumes() {
        use crate::runner::container::FakeRuntime;
        use crate::runner::container::resolve::resolve_container;
        use crate::runner::container::runtime::ContainerTrace;

        let raw: toml::Value = toml::from_str(
            r#"
kind = "container"
image = "upstroke/ci:3.2"
credential_volumes = { claude-code = "creds-cc" }
"#,
        )
        .expect("toml");
        let selection = read_runner(Some(raw), Path::new("upstroke.toml")).expect("parses");

        let runtime = FakeRuntime::new(ContainerTrace::off());
        runtime.add_image("sha256:abc", Some("sha256:def"));
        runtime.tag("upstroke/ci:3.2", "sha256:abc");
        runtime.add_volume("creds-cc");

        let policy = resolve_container(&runtime, &selection).expect("resolves");
        let image = policy.image.as_ref().expect("image");
        assert_eq!(image.reference, "upstroke/ci:3.2", "from the TOML");
        assert_eq!(image.id, "sha256:abc", "from the runtime");
        assert_eq!(image.digest.as_deref(), Some("sha256:def"));
        assert_eq!(
            policy.credential_volumes.as_ref().expect("volumes")["claude-code"],
            "creds-cc",
            "from the TOML"
        );
        policy.completeness().expect("a complete record");

        let empty = FakeRuntime::new(ContainerTrace::off());
        resolve_container(&empty, &selection)
            .expect_err("a runtime holding nothing cannot resolve it");
    }

    #[test]
    fn the_legacy_refusal_is_about_the_kind_and_about_nothing_else() {
        for limits in [
            EngineLimits::Fresh,
            EngineLimits::SequentialResume,
            EngineLimits::SequentialResumeWithRecordedGates,
        ] {
            let container = RunnerSelection {
                kind: RunnerKind::Container,
                image: Some("upstroke/ci:3.2".to_owned()),
                credential_volumes: BTreeMap::new(),
                mounts: Vec::new(),
                from_config: true,
            };
            let host = RunnerSelection {
                kind: RunnerKind::Host,
                ..container.clone()
            };

            assert_eq!(
                RunnerSelection {
                    kind: container.kind,
                    ..host.clone()
                },
                container
            );

            let error =
                refuse_legacy_container_selection(&container, Path::new("upstroke.toml"), limits)
                    .expect_err("a container selection is refused");
            let UpstrokeError::Config { message, .. } = &error else {
                panic!("{limits:?}: refused as {error:?}");
            };
            assert!(
                message.contains("[runner] `kind = \"container\"` is refused"),
                "{limits:?}: {message}"
            );

            let expected = match limits {
                EngineLimits::Fresh => "is being created by the schema-1..3 engine",
                EngineLimits::SequentialResume
                | EngineLimits::SequentialResumeWithRecordedGates => {
                    "keeps the boundary it started with"
                }
            };
            assert!(message.contains(expected), "{limits:?}: {message}");
            refuse_legacy_container_selection(&host, Path::new("upstroke.toml"), limits)
                .expect("a host selection is not refused");
        }
    }
}
