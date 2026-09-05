//! Extended notes: `docs/internals/engine/preflight.md`

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use crate::agent::Caps;
use crate::config;
use crate::error::UpstrokeError;
use crate::events::{self, BindingSummary, ChainSummary, GateSummary};
use crate::gates::{self, ShellGate};
use crate::interaction::{self, InteractionMode, Notifier};
use crate::ir::Plan;
use crate::review::{self, PassBinding, ReviewPlan};
use crate::runner::Runner;
use crate::validate::{self, Analysis, ValidateOptions};

use super::options::{Harness, RunOptions};

pub(super) struct Preflight {
    pub(super) analysis: Analysis,
    pub(super) caps: BTreeMap<String, Caps>,
    pub(super) review_plan: ReviewPlan,
    pub(super) review_pass_timeout: Duration,
    pub(super) gates: Vec<GateSummary>,
    pub(super) gate_cmds: Vec<String>,
    pub(super) warnings: Vec<String>,
    pub(super) mode: InteractionMode,
    pub(super) notifiers: Vec<&'static dyn Notifier>,
    pub(super) budgets: config::Budgets,
}

#[derive(Default)]
pub(super) struct Recorded {
    pub(super) reviews: Option<ReviewPlan>,
    pub(super) gates: Option<Vec<GateSummary>>,
    pub(super) legacy_review_timeout_missing: bool,
    pub(super) gates_from_config: bool,
    pub(super) routing: Option<RecordedRouting>,
}

pub(super) struct RecordedRouting {
    pub(super) run_id: String,
    pub(super) structure: Vec<ChainSummary>,
    pub(super) bindings: Option<Vec<ChainSummary>>,
}

pub(super) struct Validated {
    analysis: Analysis,
    inputs: validate::CapturedInputs,
    limits: config::EngineLimits,
}

pub(super) fn validate_inputs(
    opts: &RunOptions,
    limits: config::EngineLimits,
) -> Result<Validated, UpstrokeError> {
    let validate_opts = ValidateOptions {
        plan_path: opts.plan_path.clone(),
        config_path: opts.config_path.clone(),
        config_root: opts.repo_root.clone(),
        pools_path: opts.pools_path.clone(),
        engine_limits: limits,
    };
    let inputs = validate::CapturedInputs::capture(&validate_opts);
    let analysis = validate::analyze_captured(&inputs, &validate_opts)?;
    Ok(Validated {
        analysis,
        inputs,
        limits,
    })
}

impl Validated {
    pub(super) fn confirm_under_lease(
        self,
        opts: &RunOptions,
        limits: config::EngineLimits,
    ) -> Result<Analysis, UpstrokeError> {
        const ATTEMPTS: usize = 3;
        let Self {
            mut inputs,
            limits: mut validated_for,
            ..
        } = self;
        for _ in 0..ATTEMPTS {
            let confirmed = validate_inputs(opts, limits)?;
            if confirmed.inputs == inputs && limits == validated_for {
                return Ok(confirmed.analysis);
            }
            inputs = confirmed.inputs;
            validated_for = limits;
        }
        Err(UpstrokeError::Refused {
            message: format!(
                "{} kept changing while upstroke was reading them; refusing to run inputs it \
                 could not check and then hold still",
                inputs
                    .paths()
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
    }
}

pub(super) fn preflight(
    opts: &RunOptions,
    harness: &Harness<'_>,
    runner: &dyn Runner,
    analysis: Analysis,
) -> Result<Preflight, UpstrokeError> {
    preflight_with_recorded(opts, harness, runner, analysis, Recorded::default())
}

pub(super) fn preflight_with_recorded(
    opts: &RunOptions,
    harness: &Harness<'_>,
    runner: &dyn Runner,
    mut analysis: Analysis,
    recorded: Recorded,
) -> Result<Preflight, UpstrokeError> {
    let mut warnings = analysis.warnings.clone();

    if let Some(routing) = recorded.routing.as_ref() {
        restore_recorded_routing(&mut analysis, routing, &mut warnings)?;
    }

    if let Some(record) = &recorded.gates {
        if let Some(difference) = gates_differ(record, &gate_summaries(&analysis)) {
            warnings.push(difference);
        }
        analysis.gates = record.iter().map(ShellGate::from_record).collect();
        analysis.gates_from_config = recorded.gates_from_config;
    }

    let mut review_plan = match recorded.reviews {
        Some(mut plan) => {
            let configured = analysis.config.review_pass_timeout.as_secs();
            if recorded.legacy_review_timeout_missing {
                plan.pass_timeout_secs = Some(configured);
                warnings.push(format!(
                    "this run's recorded review plan predates schema 3's per-pass timeout; this \
                     resume establishes today's configured {configured}s timeout in the \
                     append-only log before any more work starts"
                ));
            } else if plan.pass_timeout_secs != Some(configured) {
                #[expect(
                    clippy::expect_used,
                    reason = "a non-legacy plan recorded its timeout; replay enforces the pairing"
                )]
                let recorded = plan
                    .pass_timeout_secs
                    .expect("a non-legacy recorded review plan has an explicit timeout");
                warnings.push(format!(
                    "today's review pass timeout ({configured}s) differs from the one this run \
                     recorded ({}s). This resume keeps the recorded timeout so one run has one \
                     verification standard. Start a new run to adopt today's timeout.",
                    recorded
                ));
            }
            if plan.enabled.is_none() || plan.alternative_available.is_none() {
                plan.enabled.get_or_insert(plan.primary.is_some());
                plan.alternative_available
                    .get_or_insert(plan.alternative.is_some());
                warnings.push(
                    "this run's recorded review plan predates schema 3's explicit reviewer-identity markers; this resume records them before any more work starts"
                        .to_owned(),
                );
            }
            plan
        }
        None => review::plan_for(
            &analysis.plan,
            &analysis.chains,
            &analysis.config,
            |id| harness.adapters.get(id).is_some(),
            &mut warnings,
        )?,
    };
    events::validate_review_identity(&review_plan, analysis.plan.tasks.len(), &opts.plan_path)?;
    let review_pass_timeout = review_plan.pass_timeout()?;

    let required = review_plan.required_agents();
    let optional: Vec<String> = review_plan
        .agents()
        .into_iter()
        .filter(|id| !required.contains(id))
        .map(str::to_owned)
        .collect();
    let mut agent_ids: Vec<&str> = analysis
        .chains
        .iter()
        .flat_map(|c| c.rungs.iter().map(|r| r.binding.agent.as_str()))
        .chain(required)
        .collect();
    agent_ids.sort_unstable();
    agent_ids.dedup();
    let mut caps: BTreeMap<String, Caps> = BTreeMap::new();
    for id in agent_ids {
        let adapter = harness
            .adapters
            .get(id)
            .ok_or_else(|| UpstrokeError::Agent {
                message: format!("no adapter registered for agent `{id}`"),
            })?;
        caps.insert(id.to_owned(), adapter.probe(runner)?);
    }
    for id in optional {
        if caps.contains_key(&id) {
            continue;
        }
        let probed = harness
            .adapters
            .get(&id)
            .ok_or_else(|| UpstrokeError::Agent {
                message: format!("no adapter registered for agent `{id}`"),
            })
            .and_then(|adapter| adapter.probe(runner));
        match probed {
            Ok(caps_for_id) => {
                caps.insert(id, caps_for_id);
            }
            Err(error) => {
                let binding = review_plan
                    .alternative
                    .as_ref()
                    .map_or_else(|| id.clone(), PassBinding::describe);
                warnings.push(format!(
                    "{binding} would have reviewed tasks their own model implemented, but it \
                     could not be probed: {error}. Those tasks fall back to same-model review \
                     (§11.3)."
                ));
                review_plan.drop_alternative();
                let tier = analysis
                    .config
                    .review_tier
                    .unwrap_or(crate::ir::Tier::Frontier);
                if let Some(warning) =
                    review_plan.self_review_warning(&analysis.plan, &analysis.chains, tier)
                {
                    warnings.push(warning);
                }
            }
        }
    }

    if !analysis.gates.is_empty() {
        let mut shells: Vec<crate::gates::ShellKind> =
            analysis.gates.iter().map(|gate| gate.shell).collect();
        shells.sort_unstable_by_key(|shell| shell.program());
        shells.dedup();
        for shell in shells {
            gates::shell_available(shell)?;
        }
        gates::resolve_programs(&analysis.gates, &opts.repo_root, &mut warnings)?;
    }
    let gates = gate_summaries(&analysis);
    let gate_cmds: Vec<String> = gates.iter().map(|gate| gate.cmd.clone()).collect();

    let mode = opts.interaction.unwrap_or(analysis.config.interaction_mode);
    let notifiers = interaction::notifiers_for(&analysis.config.notify, &mut warnings);
    let budgets = effective_budgets(analysis.config.budgets, opts.budget_usd)?;

    Ok(Preflight {
        analysis,
        caps,
        review_plan,
        review_pass_timeout,
        gates,
        gate_cmds,
        warnings,
        mode,
        notifiers,
        budgets,
    })
}

fn effective_budgets(
    configured: config::Budgets,
    flag: Option<f64>,
) -> Result<config::Budgets, UpstrokeError> {
    if let Some(limit) = flag {
        config::check_budget("--budget", limit)
            .map_err(|message| UpstrokeError::Refused { message })?;
    }
    Ok(config::Budgets {
        run_usd: flag.or(configured.run_usd),
        task_usd: configured.task_usd,
    })
}

pub(super) fn repo_relative(repo_root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

pub(super) fn chain_summaries(analysis: &Analysis) -> Vec<ChainSummary> {
    analysis
        .plan
        .tasks
        .iter()
        .zip(&analysis.chains)
        .map(|(task, chain)| ChainSummary {
            task: task.id.to_string(),
            tiers: chain.rungs.iter().map(|rung| rung.tier).collect(),
            attempts_per: chain.attempts_per,
            bindings: Some(
                chain
                    .rungs
                    .iter()
                    .map(|rung| BindingSummary {
                        tier: rung.tier,
                        agent: rung.binding.agent.clone(),
                        model: rung.binding.model.clone(),
                        pinned: rung.binding.pinned,
                    })
                    .collect(),
            ),
        })
        .collect()
}

fn restore_recorded_routing(
    analysis: &mut Analysis,
    recorded: &RecordedRouting,
    warnings: &mut Vec<String>,
) -> Result<(), UpstrokeError> {
    let current = chain_summaries(analysis);
    let same_structure = current.len() == recorded.structure.len()
        && current.iter().zip(&recorded.structure).all(|(now, then)| {
            now.task == then.task
                && now.tiers == then.tiers
                && now.attempts_per == then.attempts_per
        });
    if !same_structure {
        let moved: Vec<String> = current
            .iter()
            .zip(&recorded.structure)
            .filter(|(now, then)| {
                now.task != then.task
                    || now.tiers != then.tiers
                    || now.attempts_per != then.attempts_per
            })
            .map(|(now, then)| {
                format!(
                    "`{}` ran on [{}] with {} attempt(s) per rung and would now run on [{}] with {}",
                    then.task,
                    render_tiers(then),
                    then.attempts_per,
                    render_tiers(now),
                    now.attempts_per,
                )
            })
            .collect();
        let detail = if moved.is_empty() {
            format!(
                "the run recorded {} task chain(s), while today's plan resolves {}",
                recorded.structure.len(),
                current.len()
            )
        } else {
            moved.join("; ")
        };
        return Err(UpstrokeError::Resume {
            run_id: recorded.run_id.clone(),
            message: format!(
                "routing has changed since this run started, so a recorded rung would now mean a \
                 different tier or allowance: {detail}. Restore the config it ran with, or start \
                 a new run."
            ),
        });
    }

    let Some(snapshot) = recorded.bindings.as_ref() else {
        warnings.push(
            "this run's log predates the resolved-binding record, so worker agent/model bindings \
             were re-derived from today's config rather than read from the run — earlier attempts \
             may have used different bindings"
                .to_owned(),
        );
        return Ok(());
    };
    if snapshot.len() != analysis.chains.len() {
        return Err(UpstrokeError::Resume {
            run_id: recorded.run_id.clone(),
            message: "the recorded binding snapshot does not align with the run's task chains; \
                      the event log cannot safely identify which model belongs to which task"
                .to_owned(),
        });
    }

    let mut changed = Vec::new();
    for ((chain, now), then) in analysis.chains.iter_mut().zip(&current).zip(snapshot) {
        if then.task != now.task || then.tiers != now.tiers || then.attempts_per != now.attempts_per
        {
            return Err(UpstrokeError::Resume {
                run_id: recorded.run_id.clone(),
                message: format!(
                    "the recorded binding snapshot for `{}` does not match its frozen chain",
                    then.task
                ),
            });
        }
        let Some(bindings) = then.bindings.as_ref() else {
            return Err(UpstrokeError::Resume {
                run_id: recorded.run_id.clone(),
                message: format!(
                    "the recorded binding snapshot for `{}` is missing its bindings",
                    then.task
                ),
            });
        };
        if bindings.len() != chain.rungs.len() {
            return Err(UpstrokeError::Resume {
                run_id: recorded.run_id.clone(),
                message: format!(
                    "the recorded binding snapshot for `{}` has {} binding(s) for {} rung(s)",
                    then.task,
                    bindings.len(),
                    chain.rungs.len()
                ),
            });
        }
        for (rung, binding) in chain.rungs.iter_mut().zip(bindings) {
            if binding.tier != rung.tier {
                return Err(UpstrokeError::Resume {
                    run_id: recorded.run_id.clone(),
                    message: format!(
                        "the recorded binding snapshot for `{}` assigns tier `{}` to a `{}` rung",
                        then.task, binding.tier, rung.tier
                    ),
                });
            }
            if rung.binding.agent != binding.agent
                || rung.binding.model != binding.model
                || rung.binding.pinned != binding.pinned
            {
                changed.push(format!(
                    "`{}` {}: recorded {}/{}, today {}/{}",
                    then.task,
                    rung.tier,
                    binding.agent,
                    binding.model,
                    rung.binding.agent,
                    rung.binding.model
                ));
            }
            rung.binding.agent = binding.agent.clone();
            rung.binding.model = binding.model.clone();
            rung.binding.pinned = binding.pinned;
        }
    }
    if !changed.is_empty() {
        warnings.push(format!(
            "today's worker bindings differ from the ones this run recorded ({}). This resume \
             keeps the recorded bindings. Start a new run to adopt today's routing.",
            changed.join("; ")
        ));
    }
    Ok(())
}

fn gate_summaries(analysis: &Analysis) -> Vec<GateSummary> {
    analysis
        .gates
        .iter()
        .map(|gate| GateSummary {
            name: gate.name.clone(),
            cmd: gate.cmd.clone(),
            timeout: gate.timeout,
            shell: gate.shell,
        })
        .collect()
}

fn render_tiers(chain: &ChainSummary) -> String {
    chain
        .tiers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" → ")
}

pub(super) fn gates_differ(recorded: &[GateSummary], now: &[GateSummary]) -> Option<String> {
    if recorded == now {
        return None;
    }
    let mut unmatched: Vec<&GateSummary> = now.iter().collect();
    let mut dropped: Vec<&GateSummary> = Vec::new();
    for gate in recorded {
        match unmatched.iter().position(|other| *other == gate) {
            Some(index) => {
                unmatched.remove(index);
            }
            None => dropped.push(gate),
        }
    }
    if dropped.is_empty() && unmatched.is_empty() {
        return Some(
            "the gates in today's config are the ones this run recorded, in a different order; \
             it continues in its recorded order"
                .to_owned(),
        );
    }
    let once = |gates: &[&GateSummary], name: &str| {
        gates.iter().filter(|gate| gate.name == name).count() == 1
    };
    let mut items: Vec<String> = Vec::new();
    let mut paired: Vec<&GateSummary> = Vec::new();
    for gate in &dropped {
        let edited = unmatched
            .iter()
            .find(|other| {
                other.name == gate.name
                    && once(&dropped, &gate.name)
                    && once(&unmatched, &gate.name)
            })
            .copied();
        match edited {
            Some(other) => {
                paired.push(other);
                items.push(format!("`{}` {}", gate.name, changes_between(gate, other)));
            }
            None => items.push(format!(
                "`{}` (`{}`) is in the record and not in today's config",
                gate.name, gate.cmd
            )),
        }
    }
    for gate in unmatched {
        if paired.iter().any(|other| std::ptr::eq(*other, gate)) {
            continue;
        }
        items.push(format!(
            "`{}` (`{}`) is in today's config and not in the record",
            gate.name, gate.cmd
        ));
    }
    Some(format!(
        "the gates in today's config differ from the ones this run recorded, and a run keeps the \
         gates it started with, so these edits do not apply to it: {}. Start a new run to adopt \
         them.",
        items.join("; ")
    ))
}

fn changes_between(recorded: &GateSummary, now: &GateSummary) -> String {
    let mut parts: Vec<String> = Vec::new();
    if recorded.cmd != now.cmd {
        parts.push(format!(
            "runs `{}` and today's config says `{}`",
            recorded.cmd, now.cmd
        ));
    }
    if recorded.shell != now.shell {
        parts.push(format!(
            "runs under `{}` and today's config says `{}`",
            recorded.shell.program(),
            now.shell.program()
        ));
    }
    if recorded.timeout != now.timeout {
        parts.push(format!(
            "has {}s to finish and today's config allows {}s",
            recorded.timeout.as_secs(),
            now.timeout.as_secs()
        ));
    }
    parts.join(", and ")
}

pub(super) fn normalized_plan_bytes(plan: &Plan, path: &Path) -> Result<Vec<u8>, UpstrokeError> {
    let mut bytes = serde_json::to_vec_pretty(plan).map_err(|error| UpstrokeError::Parse {
        message: format!("serializing {}: {error}", path.display()),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}
