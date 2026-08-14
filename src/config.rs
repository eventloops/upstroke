//! Config loading (DESIGN.md §17 subset for `validate`).
//!
//! Two optional files: repo-level `tactus.toml` (routing overrides, pins,
//! strategy) and user-level `~/.tactus/pools.toml` (capacity pools, normally
//! written by `tactus connect`). Both missing is the normal fresh-repo case
//! and falls back to derived defaults silently.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::capacity::{self, Allowance, Pool, PoolKind, Source};
use crate::catalog;
use crate::error::TactusError;
use crate::gates::ShellKind;
use crate::interaction::InteractionMode;
use crate::ir::{Effort, ResolvedEffortPolicy, TaskKind, Tier};
use crate::util;

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
    budgets: Option<toml::Value>,
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
    /// Seconds a detached interactive run waits at a hard block for an answer
    /// to arrive as an event; `0` disables waiting.
    wait_on_block_secs: Option<u64>,
    /// `ask_before = { frontier_escalation_over_usd = 5.0 }` (§12).
    ask_before: Option<toml::Value>,
}

/// `[interaction] ask_before` (§12) — the thresholds that turn a routing move
/// into a question for a person.
///
/// One key today, and an unknown one is a **hard error** naming the accepted
/// set: a typo here silently deletes a spend approval, which is the same harm
/// that made `second_opinion` error rather than warn.
#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskBefore {
    /// Ask before escalating onto a **frontier** rung once the run's reported
    /// spend has reached this many api-equivalent dollars.
    ///
    /// Deliberately spend-to-*date*, not a forward projection. A literal
    /// reading of "this escalation will cost more than $N" needs per-model
    /// $/token rates the catalog does not ship, and §10's whole position is
    /// that guessing costs is worse than measuring them — inventing a price
    /// table would pile unverifiable static data on top of model names that
    /// have already proved perishable. v0.2 can project forward from *observed*
    /// per-rung costs once decision logs hold them.
    pub frontier_escalation_over_usd: Option<f64>,
}

impl AskBefore {
    /// Accepted keys, named once so the parser and its error message cannot
    /// disagree about what is legal.
    const ACCEPTED: [&'static str; 1] = ["frontier_escalation_over_usd"];
}

#[derive(Debug, Deserialize)]
struct RawRouting {
    strategy: Option<RawStrategy>,
    overrides: Option<Vec<RawOverride>>,
    /// `[routing.effort]` is parsed as a raw value so shape and spelling
    /// mistakes can name the two accepted roles rather than failing in serde's
    /// outer config message.
    effort: Option<toml::Value>,
    /// Per-kind chain entries (`fix = { chain = [...] }`) plus anything the
    /// config author got wrong — unknown keys warn rather than error.
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
struct RawOverride {
    paths: Vec<String>,
    /// Optional since step 9: an override may raise the tier floor, ask for a
    /// cross-family second opinion, or both. Requiring it would force a no-op
    /// `start_at = "small"` on anyone who wants only the second reviewer.
    start_at: Option<Tier>,
    second_opinion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPin {
    tier: Tier,
    agent: String,
    model: String,
    /// Optional override of the tier's default reasoning effort (§10), used
    /// when no explicit role policy applies. A pin is the narrower way to buy
    /// a deliberate effort for one tier.
    effort: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKindRouting {
    chain: Option<Vec<Tier>>,
    tier: Option<Tier>,
    attempts_per: Option<u32>,
    /// `[routing] review = { timeout_secs = 5400 }`. Kept on this raw shape
    /// because `review` shares the routing table with task kinds; rejected on
    /// task-kind entries below so a misplaced timeout is never ignored.
    timeout_secs: Option<u64>,
    /// `[routing] review = { enabled = false }` — the explicit opt-out of
    /// §11.2 review, for plans where a frontier judgement per task costs more
    /// than the work it is judging.
    enabled: Option<bool>,
}

/// `[pools.*]`, with each entry's byte offset kept.
///
/// `toml::Spanned` rather than a plain value because a `BTreeMap` iterates in
/// **sorted key order**, and both `Config.pools` and
/// [`crate::capacity::pool_for`] promise *file* order — "moving a pool up the
/// file promotes it" is the whole mechanism an operator has for choosing
/// between two accounts on one vendor (§13's profiles). Sorting silently
/// substituted an alphabet for that choice. The span is the offset of the
/// entry's value in the source, so re-sorting by it restores exactly what was
/// written, with no new dependency.
#[derive(Debug, Default, Deserialize)]
struct RawPools {
    pools: Option<BTreeMap<String, toml::Spanned<toml::Value>>>,
}

/// One `[pools.<name>]` entry, before validation. Every field is optional here
/// so a shape mistake reports as a named problem rather than a serde error
/// about a struct the config author never wrote.
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
    /// §13's credential-profile seam (D2): which account this pool draws from.
    profile: Option<String>,
    /// Everything else, so a typo warns by name instead of vanishing.
    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

/// `[budgets]` (§17). API-equivalent dollars; omitting either means unlimited.
///
/// `deny_unknown_fields` because §13 lists a third budget kind this build does
/// not have — per-pool fractions — so `pool_fraction` is the key an operator
/// reading the design reaches for first. Accepting it silently would let them
/// believe they had capped a pool while the run spent against no ceiling at
/// all, which is the one failure mode a budget must not have.
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

/// One ceiling, checked the same way wherever it came from.
///
/// Shared with the `--budget` flag rather than living only in the `[budgets]`
/// parser: a flag that overrides a validated key must not be a way around the
/// validation. Zero and negative both stop the run before it spends anything,
/// and NaN silently never fires — three different broken behaviours behind one
/// mistyped number.
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

/// `second_opinion` on a `[[routing.overrides]]` (§11.3).
///
/// One variant today. It stays an enum rather than a bool because §11.5
/// generalizes the reviewer into a list of passes with a lens each, and the
/// security lens arrives here as a second variant with a different ladder
/// dispatch — not as a second boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondOpinion {
    /// A second reviewer from a different model *family* must also pass. The
    /// spelling is §17's; the semantics are §11.3's ("a different model
    /// family"), which is the stricter of the two — see [`crate::catalog::Family`].
    DifferentVendor,
}

impl SecondOpinion {
    /// Accepted spellings, named once so the parser and its error message
    /// cannot disagree about what is legal.
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
    /// `None` when this override exists only to request a second opinion.
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
    /// `~/.tactus/pools.toml`, in file order — which is preference order for
    /// [`crate::capacity::pool_for`].
    pub pools: Vec<Pool>,
    /// `[budgets]` (§17); both keys optional, both meaning unlimited when absent.
    pub budgets: Budgets,
    /// `[interaction] ask_before` (§12).
    pub ask_before: AskBefore,
    /// `Some` (possibly empty — explicitly no gates) when `[[gates]]` was
    /// configured; `None` means derive from the repo.
    pub gates: Option<Vec<GateConfig>>,
    pub shell: ShellKind,
    /// `[routing] review = { tier = … }` (§11.2). `None` means the frontier
    /// default.
    pub review_tier: Option<Tier>,
    /// `[routing] review = { enabled = false }` opts out of review entirely.
    pub review_enabled: bool,
    /// Independent wall-clock allowance for each review pass. Unlike a worker
    /// attempt timeout this is frozen into [`crate::review::ReviewPlan`], so a
    /// resume cannot silently adopt a different verification budget.
    pub review_pass_timeout: Duration,
    /// Explicit role policy. A role setting outranks pin and tier defaults so
    /// `implementation = "xhigh"` really does mean every worker attempt.
    implementation_effort_override: Option<Effort>,
    review_effort_override: Option<Effort>,
    /// `[engine] on_task_failure` (§17); default `Halt`.
    pub on_task_failure: OnTaskFailure,
    /// `[interaction] mode` (§12); default `on_block`.
    pub interaction_mode: InteractionMode,
    /// `[interaction] notify` (§12); default `["cli"]`.
    pub notify: Vec<String>,
    /// `[interaction] wait_on_block_secs` (§12/§19). How long a detached but
    /// interactive run waits at a hard block for an answer to arrive as an
    /// event before ending parked. `ZERO` disables the wait, which is what a
    /// terminal-attached run and CI both want.
    pub wait_on_block: Duration,
}

/// Everything `[interaction]` contributes, kept together so adding a knob does
/// not widen a tuple every caller has to re-destructure.
struct InteractionSettings {
    mode: InteractionMode,
    notify: Vec<String>,
    wait_on_block: Duration,
    ask_before: AskBefore,
}

/// §12's default hard-block wait for a detached interactive run: long enough
/// that an operator answering from a phone finds the run still going, short
/// enough that a forgotten run gives its workspace and branch back the same
/// day.
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

    /// The tier-bound effort before a role policy is applied: a pin's override,
    /// else the tier's default (§10).
    pub fn effort_for(&self, tier: Tier) -> Effort {
        self.pins
            .iter()
            .find(|pin| pin.tier == tier)
            .and_then(|pin| pin.effort)
            .unwrap_or_else(|| Effort::for_tier(tier))
    }

    /// Effort every implementation attempt uses. An explicit role policy is
    /// global across task kinds and tiers; otherwise the tier/pin rule applies.
    pub fn implementation_effort(&self, tier: Tier) -> Effort {
        self.implementation_effort_override
            .unwrap_or_else(|| self.effort_for(tier))
    }

    /// Effort every reviewer judges at. The role policy wins when present;
    /// otherwise use the review tier, with §11.2's frontier default.
    pub fn review_effort(&self) -> Effort {
        self.review_effort_override
            .unwrap_or_else(|| self.effort_for(self.review_tier.unwrap_or(Tier::Frontier)))
    }

    /// Resolve the full role policy once so a run can record and retain it.
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

/// Frontier reviews can legitimately spend tens of minutes reading a broad
/// diff. This is per pass, including its one verdict-format re-ask.
pub const DEFAULT_REVIEW_PASS_TIMEOUT: Duration = Duration::from_secs(90 * 60);

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
    load_with(
        repo_config,
        discover_in,
        pools_file,
        &|agent| crate::agent::by_id(agent).is_some(),
        warnings,
    )
}

/// [`load`] with the adapter registry injected.
///
/// Only `[pools]` consults it, to decide whether a pool names an agent this
/// build can drive. Injected for the same reason
/// [`crate::validate::builtin_adapter`] is: the engine resolves adapters
/// through a `Harness`, not through the global registry, so a guard that asks
/// the registry directly is answering a question about a different set than the
/// one that will actually run — and the unusable-pool path could only ever be
/// tested with an agent the binary genuinely lacks.
pub fn load_with(
    repo_config: Option<&Path>,
    discover_in: &Path,
    pools_file: Option<&Path>,
    has_adapter: &dyn Fn(&str) -> bool,
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
                value.try_into().map_err(|e| TactusError::Config {
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
                // `review` is a routing ROLE, not a task kind (DESIGN §17's
                // own example configures it). Parse and echo it rather than
                // warning users off their own documented config; the reviewer
                // consumes it in step 6.
                if key == "review" {
                    let rr: RawKindRouting = value.try_into().map_err(|e| TactusError::Config {
                        path: repo_path.clone(),
                        message: format!(
                            "routing entry `review`: {e} (expected `tier`, `timeout_secs`, or \
                             `enabled = false` to run without review)"
                        ),
                    })?;
                    if rr.attempts_per.is_some() {
                        return Err(TactusError::Config {
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
                        return Err(TactusError::Config {
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
            if kr.timeout_secs.is_some() {
                return Err(TactusError::Config {
                    path: repo_path.clone(),
                    message: format!(
                        "[routing] `{key}`: timeout_secs applies only to the `review` role"
                    ),
                });
            }
            if kr.enabled.is_some() {
                return Err(TactusError::Config {
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
            // A misspelled value here silently deletes a verification layer:
            // the operator asked for two model families on their blast-radius
            // paths and would get one, with nothing said. That is the same
            // reason `[interaction] mode` errors rather than warns.
            let second_opinion = match ov.second_opinion.as_deref() {
                None => None,
                Some(raw) => Some(SecondOpinion::parse(raw).ok_or_else(|| {
                    TactusError::Config {
                        path: repo_path.clone(),
                        message: format!(
                            "[[routing.overrides]] entry {n}: `second_opinion = \"{raw}\"` is not \
                         recognized (accepted: {})",
                            SecondOpinion::ACCEPTED.join(", ")
                        ),
                    }
                })?),
            };
            // Both keys are optional individually, but an override that raises
            // nothing and asks for nothing does nothing — and reads exactly
            // like one whose key was misspelled into oblivion.
            if ov.start_at.is_none() && second_opinion.is_none() {
                return Err(TactusError::Config {
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
        // Validated here rather than discovered at spend time: the provider
        // rejects an unknown effort with a 400 *after* the turn has started
        // (measured 2026-08-11), so a typo costs a whole attempt instead of a
        // config error. Same posture as the pinned-model check above.
        let effort = match pin.effort.as_deref().map(Effort::parse) {
            Some(None) => {
                return Err(TactusError::Config {
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

    let gates = parse_gates(raw.gates, &repo_path)?;
    let (shell, on_task_failure) = parse_engine(raw.engine, &repo_path, warnings)?;
    let interaction = parse_interaction(raw.interaction, &repo_path)?;
    let budgets = parse_budgets(raw.budgets, &repo_path)?;

    let pools = read_pools(pools_file, has_adapter, warnings)?;

    Ok(Config {
        chains,
        overrides,
        pins,
        strategy,
        pools,
        budgets,
        ask_before: interaction.ask_before,
        gates,
        shell,
        review_tier,
        review_enabled,
        review_pass_timeout,
        implementation_effort_override,
        review_effort_override,
        on_task_failure,
        interaction_mode: interaction.mode,
        notify: interaction.notify,
        wait_on_block: interaction.wait_on_block,
    })
}

/// Parse one role's explicit effort at config load. All three providers reject
/// an unknown value after process launch, so accepting a typo here would burn an
/// attempt for a routing policy the operator never actually selected.
fn parse_role_effort(
    raw: Option<&str>,
    role: &str,
    repo_path: &Path,
) -> Result<Option<Effort>, TactusError> {
    let Some(raw) = raw else { return Ok(None) };
    Effort::parse(raw)
        .map(Some)
        .ok_or_else(|| TactusError::Config {
            path: repo_path.to_path_buf(),
            message: format!(
                "[routing.effort] `{role} = \"{raw}\"` is not recognized (accepted: {})",
                Effort::KNOWN
            ),
        })
}

/// `[budgets]` (§17). A ceiling that is zero, negative, or not a number is a
/// hard error: every one of those readings would either stop the run before it
/// began or be ignored, and which of the two happened must not be a surprise.
fn parse_budgets(raw: Option<toml::Value>, repo_path: &Path) -> Result<Budgets, TactusError> {
    let Some(value) = raw else {
        return Ok(Budgets::default());
    };
    let budgets: Budgets = value.try_into().map_err(|e| TactusError::Config {
        path: repo_path.to_path_buf(),
        message: format!(
            "[budgets]: {e} (expected optional `run_usd` and `task_usd` numbers, in \
             api-equivalent dollars)"
        ),
    })?;
    for (name, limit) in [("run_usd", budgets.run_usd), ("task_usd", budgets.task_usd)] {
        let Some(limit) = limit else { continue };
        check_budget(name, limit).map_err(|message| TactusError::Config {
            path: repo_path.to_path_buf(),
            message: format!("[budgets] {message}"),
        })?;
    }
    Ok(budgets)
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

/// `[interaction]` (§12).
///
/// Everything here is a hard error or nothing: `mode` and `ask_before` both
/// decide whether a human is ever asked, so a typo in either must not degrade
/// quietly. Notifier ids are the one soft setting, and they are validated by
/// `notifiers_for` at run time rather than here — which is why this function
/// takes no warning sink.
fn parse_interaction(
    raw: Option<toml::Value>,
    repo_path: &Path,
) -> Result<InteractionSettings, TactusError> {
    let default_notify = || vec!["cli".to_owned()];
    let Some(value) = raw else {
        return Ok(InteractionSettings {
            mode: InteractionMode::default(),
            notify: default_notify(),
            wait_on_block: DEFAULT_WAIT_ON_BLOCK,
            ask_before: AskBefore::default(),
        });
    };
    let interaction: RawInteraction = value.try_into().map_err(|e| TactusError::Config {
        path: repo_path.to_path_buf(),
        message: format!(
            "[interaction]: {e} (expected optional `mode`, `notify` list, \
             `wait_on_block_secs`, and `ask_before` table)"
        ),
    })?;
    // An unknown key inside `ask_before` errors rather than warning: the whole
    // point of the table is to stop the run and ask, so a misspelling that
    // silently drops the threshold spends the money the operator asked to be
    // consulted about. Same reasoning as `second_opinion`.
    let ask_before = match interaction.ask_before {
        None => AskBefore::default(),
        Some(value) => value.try_into().map_err(|e| TactusError::Config {
            path: repo_path.to_path_buf(),
            message: format!(
                "[interaction] ask_before: {e} (accepted: {})",
                AskBefore::ACCEPTED.join(", ")
            ),
        })?,
    };
    if let Some(threshold) = ask_before.frontier_escalation_over_usd
        && (!threshold.is_finite() || threshold < 0.0)
    {
        return Err(TactusError::Config {
            path: repo_path.to_path_buf(),
            message: format!(
                "[interaction] ask_before `frontier_escalation_over_usd = {threshold}` is not a \
                 spend threshold — omit the key to never ask, or give it a number of dollars"
            ),
        });
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
    Ok(InteractionSettings {
        mode,
        notify: interaction.notify.unwrap_or_else(default_notify),
        wait_on_block: interaction
            .wait_on_block_secs
            .map_or(DEFAULT_WAIT_ON_BLOCK, Duration::from_secs),
        ask_before,
    })
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

/// Read `~/.tactus/pools.toml` into typed pools (§17).
///
/// Temperament matches the rest of this file: anything that would silently
/// change what the estimator does is an error, and anything that only degrades
/// what it can say is a warning.
///
/// - unknown `kind` → **error**; it decides which estimator rule runs.
/// - unknown `sources` entry → **error**; dropping `signals` by typo would
///   discard §13's ground truth while the file still claims to have it.
/// - `safety_margin` / `reserve` outside `0.0..=1.0` → **error**; both are
///   fractions, and a "150% margin" has no reading that is merely degraded.
/// - `agent` with no adapter in this build → **warn**, pool kept and marked
///   unusable. §17's own example ships `[pools.local] agent = "aider"`, so
///   erroring would brick anyone who copied the documented file.
/// - unknown keys → **warn**, by name.
///
/// An **explicit** `--pools` path that does not exist is an error, the way an
/// explicit `--config` is in [`read_repo_config`]: a path someone typed and
/// that is not there is a typo, and answering it with "no pools connected —
/// run `tactus connect`" sends them to regenerate a file that was never the
/// problem. A *discovered* one that is absent is the normal fresh case and
/// stays silent.
fn read_pools(
    pools_file: Option<&Path>,
    has_adapter: &dyn Fn(&str) -> bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<Pool>, TactusError> {
    let (path, required) = match pools_file {
        Some(p) => (p.to_path_buf(), true),
        None => match discovered_pools_path() {
            Some(p) => (p, false),
            None => return Ok(Vec::new()),
        },
    };
    if !path.exists() {
        if required {
            return Err(TactusError::Config {
                path,
                message: "pools file not found".to_owned(),
            });
        }
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
    // Back into the order they were written in — see [`RawPools`].
    let mut entries: Vec<(String, toml::Spanned<toml::Value>)> =
        raw.pools.unwrap_or_default().into_iter().collect();
    entries.sort_by_key(|(_, spanned)| spanned.span().start);
    let mut pools = Vec::new();
    for (name, spanned) in entries {
        pools.push(parse_pool(
            &name,
            spanned.into_inner(),
            &path,
            has_adapter,
            warnings,
        )?);
    }
    Ok(pools)
}

fn parse_pool(
    name: &str,
    value: toml::Value,
    path: &Path,
    has_adapter: &dyn Fn(&str) -> bool,
    warnings: &mut Vec<String>,
) -> Result<Pool, TactusError> {
    let config_error = |message: String| TactusError::Config {
        path: path.to_path_buf(),
        message,
    };
    // A pool's name is its identity everywhere downstream — it is what an
    // attempt is attributed to and what the ledger prints. A blank one is
    // indistinguishable from "no pool" by the time it reaches the engine
    // (`pool_option` maps `""` to `None`), so the attribution would vanish
    // while the pool still matched for routing. Same reasoning as the
    // non-empty `[[gates]]` `name`.
    if name.trim().is_empty() {
        return Err(config_error(
            "a pool needs a non-empty name — `[pools.<name>]` is what attempts are attributed to"
                .to_owned(),
        ));
    }
    let raw: RawPool = value.try_into().map_err(|e| {
        config_error(format!(
            "[pools.{name}]: {e} (expected `kind` and `agent` strings, with optional `window`, \
             `weekly`, `sources`, `safety_margin`, `reserve`, `monthly_allowance`, `endpoint`, \
             and `profile`)"
        ))
    })?;

    let kind_text = raw.kind.ok_or_else(|| {
        config_error(format!(
            "[pools.{name}] has no `kind` — one of: {}",
            PoolKind::ACCEPTED.join(", ")
        ))
    })?;
    let kind = PoolKind::parse(&kind_text).ok_or_else(|| {
        config_error(format!(
            "[pools.{name}] `kind = \"{kind_text}\"` is not recognized (accepted: {})",
            PoolKind::ACCEPTED.join(", ")
        ))
    })?;
    let agent = raw
        .agent
        .ok_or_else(|| config_error(format!("[pools.{name}] has no `agent`")))?;

    let window = match raw.window {
        None => None,
        Some(text) => Some(capacity::parse_duration(&text).ok_or_else(|| {
            config_error(format!(
                "[pools.{name}] `window = \"{text}\"` is not a duration — write a number and one \
                 of s, m, h, d (for example \"5h\")"
            ))
        })?),
    };

    let mut sources = Vec::new();
    for entry in raw.sources.unwrap_or_default() {
        let source = Source::parse(&entry).ok_or_else(|| {
            config_error(format!(
                "[pools.{name}] `sources` entry `{entry}` is not recognized (accepted: {})",
                Source::ACCEPTED.join(", ")
            ))
        })?;
        if !sources.contains(&source) {
            sources.push(source);
        }
    }

    let fraction = |field: &str, value: Option<f64>, default: f64| -> Result<f64, TactusError> {
        let Some(value) = value else {
            return Ok(default);
        };
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(config_error(format!(
                "[pools.{name}] `{field} = {value}` is out of range — it is a fraction of the \
                 pool between 0.0 and 1.0"
            )));
        }
        Ok(value)
    };
    let safety_margin = fraction(
        "safety_margin",
        raw.safety_margin,
        capacity::DEFAULT_SAFETY_MARGIN,
    )?;
    let reserve = fraction("reserve", raw.reserve, capacity::DEFAULT_RESERVE)?;

    let monthly_allowance = match raw.monthly_allowance {
        None => Allowance::Auto,
        Some(toml::Value::String(text)) if text.trim().eq_ignore_ascii_case("auto") => {
            Allowance::Auto
        }
        Some(toml::Value::Integer(units)) => Allowance::Units(units as f64),
        Some(toml::Value::Float(units)) => Allowance::Units(units),
        Some(other) => {
            return Err(config_error(format!(
                "[pools.{name}] `monthly_allowance` must be a number of units or the string \
                 \"auto\", found a {}",
                other.type_str()
            )));
        }
    };
    if let Allowance::Units(units) = monthly_allowance
        && (!units.is_finite() || units <= 0.0)
    {
        return Err(config_error(format!(
            "[pools.{name}] `monthly_allowance = {units}` is not an allowance — write \"auto\" if \
             you do not know its size"
        )));
    }

    for key in raw.unknown.keys() {
        warnings.push(format!(
            "unknown key `{key}` in [pools.{name}] in {} (ignored)",
            path.display()
        ));
    }

    let usable = has_adapter(&agent);
    if !usable {
        warnings.push(format!(
            "[pools.{name}] names agent `{agent}`, which has no adapter in this build — the pool \
             is listed but this engine can never draw from it"
        ));
    }

    Ok(Pool {
        name: name.to_owned(),
        kind,
        agent,
        window,
        weekly: raw.weekly.unwrap_or(false),
        sources,
        safety_margin,
        reserve,
        monthly_allowance,
        endpoint: raw.endpoint,
        profile: raw.profile,
        usable,
    })
}

fn discovered_pools_path() -> Option<PathBuf> {
    Some(util::user_tactus_dir()?.join("pools.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::OnceLock;

    fn scratch(name: &str, content: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tactus-config-tests-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join(name);
        fs::write(&path, content).expect("write scratch file");
        path
    }

    /// An explicit pools path with no pools in it.
    ///
    /// A real, empty file rather than an absent one: an explicit `--pools` that
    /// does not exist is now a hard error (a path someone typed and that is not
    /// there is a typo), and passing `None` here would reach for the operator's
    /// real `~/.tactus/pools.toml` — which no test may touch.
    fn missing() -> PathBuf {
        // Created once: the file is identical for every caller, and rewriting
        // one shared path from parallel tests means truncating it under a
        // reader.
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let dir = env::temp_dir().join(format!("tactus-config-nopools-{}", std::process::id()));
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
        assert!(cfg.pools.is_empty());
        assert_eq!(cfg.review_pass_timeout, DEFAULT_REVIEW_PASS_TIMEOUT);
    }

    #[test]
    fn explicit_config_path_must_exist() {
        let mut warnings = Vec::new();
        let absent = env::temp_dir()
            .join("tactus-definitely-missing")
            .join("tactus.toml");
        let err = load(Some(&absent), &hermetic(), Some(&missing()), &mut warnings)
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
        // `review` is a routing role, not a task kind: parsed, echoed, and
        // never warned about (DESIGN §17 configures it in its own example).
        assert_eq!(cfg.review_tier, Some(Tier::Frontier));
        assert_eq!(cfg.review_pass_timeout, Duration::from_secs(7200));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn an_override_may_ask_for_a_second_opinion_without_raising_the_floor() {
        // Requiring `start_at` would force a no-op `start_at = "small"` on
        // anyone who wants a cross-family reviewer on paths whose difficulty
        // is already routed correctly.
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
        // With no floor to apply, routing is untouched.
        assert_eq!(
            cfg.chain_for(TaskKind::Fix).chain,
            vec![Tier::Small, Tier::Mid, Tier::Frontier]
        );
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn a_misspelled_second_opinion_is_a_hard_error() {
        // Warning and carrying on would run the task with ONE reviewer while
        // the config says two — a verification layer deleted in silence.
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
    fn an_override_that_does_nothing_is_a_hard_error() {
        // Indistinguishable from one whose only key was misspelled into a
        // section serde ignores, so it cannot be waved through.
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
    fn effort_defaults_by_tier_and_a_pin_overrides_it() {
        // What makes a tier mean something to an agent with an effort axis: a
        // chain that escalates has to move this too, or every rung thinks
        // exactly as hard as the last one.
        let path = scratch(
            "effortpin.toml",
            "[[pins]]\ntier = \"frontier\"\nagent = \"claude-code\"\nmodel = \"claude-opus-5\"\n\
             effort = \"max\"\n",
        );
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.effort_for(Tier::Small), Effort::Low);
        assert_eq!(cfg.effort_for(Tier::Mid), Effort::Medium);
        // The pin wins over the tier's `high` default when no role policy is
        // present — the original behavior remains intact.
        assert_eq!(cfg.effort_for(Tier::Frontier), Effort::Max);
        assert_eq!(cfg.implementation_effort(Tier::Frontier), Effort::Max);
        // Reviewers judge at the review tier, which defaults to frontier.
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
        let path = root.join("tactus.toml");
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
        // The provider rejects an unknown effort with a 400 after the turn has
        // started (measured), so a typo would otherwise cost an attempt and
        // report as an agent failure. Same posture as the pinned-model check.
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

        // D2's seam: two Claude Max pools differing only in `profile` parse and
        // stay distinct. Nothing acts on the field in v0.1 — this is the shape
        // being right ahead of the behaviour, deliberately.
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

        // `kind` decides which estimator rule runs.
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

        // Dropping `signals` by typo would discard §13's ground truth while the
        // file still claims to have it.
        let err = load_pools(
            "badsource.toml",
            "[pools.p]\nkind = \"credits\"\nagent = \"copilot\"\nsources = [\"signal\"]\n",
            &mut warnings,
        )
        .expect_err("unknown source must error");
        assert!(err.to_string().contains("signals"), "got: {err}");

        // A "150% margin" has no degraded reading, only a wrong one.
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

        // §17's own example ships `agent = "aider"`, which has no adapter in
        // v0.1. Erroring would brick anyone who copied the documented file.
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
        assert_eq!(cfg.wait_on_block, DEFAULT_WAIT_ON_BLOCK);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn wait_on_block_is_configurable_and_zero_means_do_not_wait() {
        let path = scratch("wait.toml", "[interaction]\nwait_on_block_secs = 90\n");
        let mut warnings = Vec::new();
        let cfg = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings).expect("load");
        assert_eq!(cfg.wait_on_block, Duration::from_secs(90));

        // Zero is a real setting, not "unset" — it is how an operator says a
        // detached run should end parked rather than hold the workspace.
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
        // Parsed and acted on since step 10 — the "needs the ledger" warning it
        // used to carry expired when the ledger landed.
        assert_eq!(cfg.ask_before.frontier_escalation_over_usd, Some(5.0));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn a_misspelled_ask_before_key_is_a_hard_error() {
        // Warning and carrying on would run past the spend the operator asked
        // to approve, with nothing said — the `second_opinion` lesson, applied
        // to money.
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

        // Zero and negative both have two readings — "stop before starting" and
        // "no limit" — and which one happened must never be a surprise.
        for bad in ["run_usd = 0.0", "task_usd = -1.0"] {
            let path = scratch("badbudget.toml", &format!("[budgets]\n{bad}\n"));
            let err = load(Some(&path), &hermetic(), Some(&missing()), &mut warnings)
                .expect_err("a non-positive ceiling must error");
            assert!(err.to_string().contains("ceiling"), "got: {err}");
        }

        // Absent means unlimited, silently.
        let cfg = load(None, &hermetic(), Some(&missing()), &mut warnings).expect("defaults");
        assert!(!cfg.budgets.any());
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

    #[test]
    fn pools_keep_the_order_they_were_written_in() {
        // `pool_for` takes the first match and its doc promises "table order as
        // preference", which is the only mechanism an operator has for choosing
        // between two accounts on one vendor. A `BTreeMap` silently substituted
        // an alphabet for that choice — and every fixture happened to be
        // alphabetical already, so nothing noticed.
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
        // §13 lists per-pool budgets, so `pool_fraction` is the key someone
        // reading the design reaches for first. Accepting it silently would let
        // them believe a pool was capped while nothing capped it.
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
        // Same rule `--config` has had: a path someone typed and that is not
        // there is a mistake, and answering it with "no pools connected — run
        // `tactus connect`" sends them to regenerate a file that was fine.
        let absent = env::temp_dir()
            .join("tactus-definitely-missing")
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
    fn a_blank_pool_name_is_refused() {
        // The name is what an attempt is attributed to; blank is
        // indistinguishable from "no pool" by the time it reaches the ledger.
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
}
