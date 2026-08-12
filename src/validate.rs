//! `tactus validate`: parse → config → graph checks → routing preview →
//! rendered report. No execution of anything.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent;
use crate::capacity;
use crate::config::{self, Config};
use crate::error::{TactusError, ValidationErrors};
use crate::gates::{self, ShellGate};
use crate::ir::{Plan, Task, TaskId};
use crate::plan::{self, Parsed};
use crate::review::{self, PassBinding, ReviewPlan};
use crate::route::{self, ResolvedChain};

#[derive(Debug, Clone)]
pub struct ValidateOptions {
    pub plan_path: PathBuf,
    /// Explicit `--config` path; `None` looks for `tactus.toml` in
    /// `config_root`.
    pub config_path: Option<PathBuf>,
    /// Root of the repo the plan targets: config discovery and gate
    /// derivation both resolve here, never against the process CWD.
    pub config_root: PathBuf,
    /// Pools file override for tests; `None` discovers `~/.tactus/pools.toml`.
    pub pools_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Row {
    pub id: String,
    pub kind: String,
    pub deps: String,
    pub chain: String,
}

#[derive(Debug)]
pub struct Report {
    pub plan: Plan,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
    pub strategy: String,
    pub capacity: String,
    /// Who reviews, and where a second opinion applies (§11.2–§11.3).
    pub review: String,
    /// Effective reasoning policy before any process is spawned.
    pub effort: String,
    pub gates: Vec<String>,
    pub gates_from_config: bool,
}

/// The shared front half of `validate` and the engine's pre-flight (§14:
/// "plan parses cycle-free"): parse, load config, check the graph, resolve
/// every routing chain. Executes nothing.
#[derive(Debug)]
pub struct Analysis {
    pub plan: Plan,
    pub config: Config,
    /// One resolved chain per task, aligned with `plan.tasks`.
    pub chains: Vec<ResolvedChain>,
    /// Effective gates: `[[gates]]` verbatim, else derived from the repo's
    /// shape (§17) — the single derivation point for validate and the engine.
    pub gates: Vec<ShellGate>,
    pub gates_from_config: bool,
    pub warnings: Vec<String>,
}

pub fn analyze(opts: &ValidateOptions) -> Result<Analysis, TactusError> {
    let raw = fs::read_to_string(&opts.plan_path).map_err(|source| TactusError::Io {
        path: opts.plan_path.clone(),
        source,
    })?;
    let Parsed {
        plan,
        warnings: mut all_warnings,
    } = plan::detect(&raw)?.parse_with_warnings(&raw)?;
    let config = config::load(
        opts.config_path.as_deref(),
        &opts.config_root,
        opts.pools_path.as_deref(),
        &mut all_warnings,
    )?;
    check_graph(&plan, &mut all_warnings)?;
    let config_path = || {
        opts.config_path
            .clone()
            .unwrap_or_else(|| opts.config_root.join("tactus.toml"))
    };
    check_pin_adapters(&config.pins, builtin_adapter, &config_path())?;
    let chains: Vec<ResolvedChain> = plan
        .tasks
        .iter()
        .map(|t| route::resolve(t, &config))
        .collect();
    let gates_from_config = config.gates.is_some();
    let gates = match &config.gates {
        Some(configured) => configured
            .iter()
            .map(|g| ShellGate {
                name: g.name.clone(),
                cmd: g.cmd.clone(),
                timeout: g.timeout,
                shell: config.shell,
            })
            .collect(),
        None => gates::derive(&opts.config_root, config.shell),
    };
    Ok(Analysis {
        plan,
        config,
        chains,
        gates,
        gates_from_config,
        warnings: all_warnings,
    })
}

/// Whether this build ships an adapter for `agent`.
///
/// Injected into the checks below rather than called from them, so the guards
/// can be tested against agents that do and do not exist without waiting for
/// the registry to grow one.
pub fn builtin_adapter(agent: &str) -> bool {
    agent::by_id(agent).is_some()
}

fn adapter_list() -> String {
    agent::ADAPTERS
        .iter()
        .map(|a| a.id())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A pin naming an agent with no adapter must fail the same way in `validate`
/// and `run`; otherwise the preview promises a binding the run then refuses at
/// pre-flight (§18).
///
/// Currently unreachable through `tactus.toml` alone — `config::load` rejects
/// any pin whose (agent, model) is absent from the catalog, and every catalog
/// agent has an adapter as of step 9. It stays because that is a coincidence of
/// today's table, not a property: §13 says the catalog ships ahead of support
/// (Aider models are catalogued in v0.2 before its adapter lands), and the
/// moment it does, this is what stops a preview from promising them.
fn check_pin_adapters(
    pins: &[config::Pin],
    has_adapter: impl Fn(&str) -> bool,
    config_path: &Path,
) -> Result<(), TactusError> {
    for pin in pins {
        if !has_adapter(&pin.agent) {
            return Err(TactusError::Config {
                path: config_path.to_path_buf(),
                message: format!(
                    "pin for tier `{}` names agent `{}`, which has no adapter in this build \
                     (available: {})",
                    pin.tier,
                    pin.agent,
                    adapter_list()
                ),
            });
        }
    }
    Ok(())
}

pub fn run(opts: &ValidateOptions) -> Result<Report, TactusError> {
    let analysis = analyze(opts)?;
    let mut warnings = analysis.warnings;
    // Zero-spend preview of the §14 gate pre-flight: warn, never refuse.
    gates::preview_resolution(&analysis.gates, &opts.config_root, &mut warnings);
    // Who would judge the work (§11.2–§11.3), against the adapters this binary
    // ships. A run asks the same question of the adapters its own harness
    // holds, which in production is the same set — so the preview cannot
    // promise a reviewer the run would then refuse.
    let reviews = review::plan_for(
        &analysis.plan,
        &analysis.chains,
        &analysis.config,
        builtin_adapter,
        &mut warnings,
    )?;
    let rows = analysis
        .plan
        .tasks
        .iter()
        .zip(&analysis.chains)
        .enumerate()
        .map(|(index, (task, chain))| {
            let second = reviews.second_opinion.get(index).and_then(Option::as_ref);
            to_row(task, chain.clone(), second)
        })
        .collect();
    let (observations, run_id) = latest_run_observations(
        &opts.config_root,
        !analysis.config.pools.is_empty(),
        &mut warnings,
    );
    Ok(Report {
        rows,
        warnings,
        strategy: strategy_echo(&analysis.config),
        capacity: capacity_echo(&analysis.config, &observations, run_id.as_deref()),
        review: review_echo(&reviews),
        effort: effort_echo(&analysis.config),
        gates: analysis.gates.iter().map(|g| g.name.clone()).collect(),
        gates_from_config: analysis.gates_from_config,
        plan: analysis.plan,
    })
}

/// Who judges the work (§11.2–§11.3), for the preview.
///
/// Resolved against the adapters this build ships, not against binaries found
/// on PATH: `validate` and `--dry-run` execute nothing (§18), so they cannot
/// probe. Pre-flight is where a named reviewer has to prove it can actually
/// run — and where a missing one either warns or refuses. The line says so,
/// because a preview that reads as a promise is worse than one that reads as a
/// plan.
fn review_echo(plan: &ReviewPlan) -> String {
    let Some(primary) = &plan.primary else {
        return "review: disabled ([routing] review = { enabled = false })".to_owned();
    };
    let mut line = format!("review: {}", primary.describe());
    match &plan.alternative {
        Some(alt) => line.push_str(&format!(
            " (tasks it implements itself would be reviewed by {} instead, if installed)",
            alt.describe()
        )),
        None => line.push_str(" (no cross-family reviewer exists in this build)"),
    }
    let demanded = plan.second_opinion.iter().flatten().count();
    if demanded > 0 {
        line.push_str(&format!(
            "; {demanded} task(s) also require a second opinion, which pre-flight refuses to \
             start without"
        ));
    }
    line
}

/// §13's capacity block, for a command that executes nothing.
///
/// `validate` and `--dry-run` **do not probe** (§18): every figure here comes
/// from files — the pools file, and the latest run's event log in this
/// repository. That is a real distinction rather than a technicality, and the
/// block says which side of it each line is on, because `tactus capacity` shows
/// strictly more by being allowed to spawn the vendors' CLIs.
///
/// The same reason the review line says "if installed": a preview that reads as
/// a promise is worse than one that reads as a plan.
fn capacity_echo(cfg: &Config, obs: &capacity::Observations, run: Option<&str>) -> String {
    use std::fmt::Write as _;

    if cfg.pools.is_empty() {
        return "capacity: not connected — run `tactus connect` to write ~/.tactus/pools.toml"
            .to_owned();
    }
    let estimates = capacity::estimate(&cfg.pools, obs);
    let mut out = format!("capacity: {} pool(s) connected\n", cfg.pools.len());
    for (pool, estimate) in cfg.pools.iter().zip(&estimates) {
        let _ = writeln!(out, "  {}", pool.describe());
        let _ = writeln!(out, "    {}", estimate.describe());
        for note in &estimate.notes {
            let _ = writeln!(out, "    - {note}");
        }
    }
    match run {
        Some(run_id) => {
            let _ = writeln!(
                out,
                "  self-metered draw is folded from run {run_id}, the latest in this repository"
            );
        }
        None => {
            let _ = writeln!(
                out,
                "  no run in this repository yet, so nothing has been self-metered"
            );
        }
    }
    for line in capacity::strategy_preview(&cfg.strategy.mode, &estimates) {
        let _ = writeln!(out, "  {line}");
    }
    let _ = write!(
        out,
        "  this preview reads files only and never probes (§18) — `tactus capacity` asks the \
         installed CLIs as well"
    );
    out
}

/// §13's observations, without executing anything: fold the latest run in this
/// repository, if there is one.
///
/// A missing or unreadable run is not an error here. `validate` describes a
/// plan; a broken run directory beside it is somebody else's problem, and
/// refusing to preview a plan over one would be a strange trade.
/// `has_pools` short-circuits the whole fold. With no pools connected the
/// capacity block is one line and the observations are never consulted, so
/// parsing an entire run's log for it is work with no reader — and `validate`
/// is the fast, zero-spend iteration loop §18 puts on day one.
fn latest_run_observations(
    repo_root: &Path,
    has_pools: bool,
    warnings: &mut Vec<String>,
) -> (capacity::Observations, Option<String>) {
    let none = || (capacity::Observations::default(), None);
    if !has_pools {
        return none();
    }
    let Some(run_id) = crate::rundir::latest_run(repo_root) else {
        return none();
    };
    let events_path = crate::rundir::public_dir(repo_root, &run_id).join("events.jsonl");
    let mut ignored = Vec::new();
    match crate::events::read_all(&events_path, &mut ignored) {
        Ok(events) => (capacity::observe(&events), Some(run_id)),
        // A run that exists but cannot be folded is not "no run" — and
        // `read_all`'s refusal ("the log has been rewritten…") is exactly the
        // loud error the event-log design exists to produce, so swallowing it
        // and reporting an empty repository hid two things at once.
        Err(error) => {
            warnings.push(format!(
                "run {run_id} exists but its event log could not be folded for self-metered \
                 spend ({error}); the capacity block below rests on rate-limit signals alone"
            ));
            none()
        }
    }
}

/// Duplicate ids, unknown `depends` targets, then cycles — all collected so a
/// broken plan reports everything in one run. On a clean graph, artifact
/// wiring that contradicts the dependency order is surfaced as warnings.
fn check_graph(plan: &Plan, warnings: &mut Vec<String>) -> Result<(), TactusError> {
    let mut problems = Vec::new();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for task in &plan.tasks {
        *seen.entry(task.id.as_str()).or_insert(0) += 1;
    }
    for (id, count) in &seen {
        if *count > 1 {
            problems.push(format!("duplicate task id `{id}` ({count} tasks share it)"));
        }
    }
    for task in &plan.tasks {
        for dep in &task.depends_on {
            if !seen.contains_key(dep.as_str()) {
                problems.push(format!("task `{}` depends on unknown id `{dep}`", task.id));
            }
        }
    }
    // Cycle detection only makes sense on a graph whose edges all resolve.
    if problems.is_empty()
        && let Some(cycle) = find_cycle(plan)
    {
        problems.push(format!("dependency cycle: {}", cycle.join(" -> ")));
    }
    if !problems.is_empty() {
        return Err(TactusError::Validation(ValidationErrors(problems)));
    }
    check_artifact_wiring(plan, warnings);
    Ok(())
}

/// A task that `needs` an artifact should depend — directly or transitively —
/// on its producer, or execution order cannot guarantee the artifact exists.
/// The plan is frozen (§5), so this warns rather than inventing edges.
fn check_artifact_wiring(plan: &Plan, warnings: &mut Vec<String>) {
    let index = index_by_id(plan);
    for task in &plan.tasks {
        for needed in &task.artifacts_in {
            let producer = plan
                .artifacts
                .iter()
                .find(|a| a.id == *needed)
                .and_then(|a| a.produced_by.as_ref());
            // Unknown producers already warned during parsing.
            let Some(producer) = producer else { continue };
            if *producer != task.id && !depends_transitively(&index, &task.id, producer) {
                warnings.push(format!(
                    "task `{}` needs artifact `{needed}` produced by `{producer}` but does not \
                     depend on it (directly or transitively)",
                    task.id
                ));
            }
        }
    }
}

/// Id → task, built once per pass and shared by the graph checks.
fn index_by_id(plan: &Plan) -> BTreeMap<&str, &Task> {
    plan.tasks.iter().map(|t| (t.id.as_str(), t)).collect()
}

fn depends_transitively(index: &BTreeMap<&str, &Task>, from: &TaskId, target: &TaskId) -> bool {
    let mut queue: Vec<&TaskId> = index
        .get(from.as_str())
        .map(|t| t.depends_on.iter().collect())
        .unwrap_or_default();
    let mut visited: Vec<&str> = Vec::new();
    while let Some(dep) = queue.pop() {
        if dep == target {
            return true;
        }
        if visited.contains(&dep.as_str()) {
            continue;
        }
        visited.push(dep.as_str());
        if let Some(task) = index.get(dep.as_str()) {
            queue.extend(task.depends_on.iter());
        }
    }
    false
}

fn find_cycle(plan: &Plan) -> Option<Vec<String>> {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let index: BTreeMap<&str, usize> = plan
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();

    fn dfs(
        current: usize,
        plan: &Plan,
        index: &BTreeMap<&str, usize>,
        color: &mut [u8],
        stack: &mut Vec<usize>,
    ) -> Option<Vec<String>> {
        color[current] = GRAY;
        stack.push(current);
        for dep in &plan.tasks[current].depends_on {
            let Some(&next) = index.get(dep.as_str()) else {
                continue;
            };
            if color[next] == GRAY {
                let from = stack.iter().position(|&i| i == next).unwrap_or(0);
                let mut cycle: Vec<String> = stack[from..]
                    .iter()
                    .map(|&i| plan.tasks[i].id.to_string())
                    .collect();
                cycle.push(plan.tasks[next].id.to_string());
                return Some(cycle);
            }
            if color[next] == WHITE
                && let Some(cycle) = dfs(next, plan, index, color, stack)
            {
                return Some(cycle);
            }
        }
        stack.pop();
        color[current] = BLACK;
        None
    }

    let mut color = vec![WHITE; plan.tasks.len()];
    let mut stack = Vec::new();
    for start in 0..plan.tasks.len() {
        if color[start] == WHITE
            && let Some(cycle) = dfs(start, plan, &index, &mut color, &mut stack)
        {
            return Some(cycle);
        }
    }
    None
}

fn to_row(task: &Task, resolved: ResolvedChain, second_opinion: Option<&PassBinding>) -> Row {
    let deps = if task.depends_on.is_empty() {
        "-".to_owned()
    } else {
        task.depends_on
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut chain = resolved
        .rungs
        .iter()
        .map(|rung| {
            let binding_tag = if rung.binding.pinned {
                "pin"
            } else {
                "preview"
            };
            format!(
                "{}({})={}/{}({binding_tag})",
                rung.tier, rung.source, rung.binding.agent, rung.binding.model
            )
        })
        .collect::<Vec<_>>()
        .join(" -> ");
    for note in &resolved.notes {
        chain.push_str(&format!(" [{note}]"));
    }
    // §11.3: a second reviewer is a per-task routing decision like any other,
    // so it belongs in the column that shows what this task's paths bought it.
    if let Some(binding) = second_opinion {
        chain.push_str(&format!(" [second opinion: {}]", binding.describe()));
    }
    Row {
        id: task.id.to_string(),
        kind: task.kind.to_string(),
        deps,
        chain,
    }
}

fn strategy_echo(cfg: &Config) -> String {
    let mut line = format!("strategy: {}", cfg.strategy.mode);
    if let Some(threshold) = cfg.strategy.spend_down_after {
        line.push_str(&format!(" (spend_down_after={threshold})"));
    }
    line.push_str(if cfg.strategy.from_config {
        " [from config; parsed, not acted on]"
    } else {
        " [derived default]"
    });
    line
}

fn effort_echo(cfg: &Config) -> String {
    let policy = cfg.resolved_effort_policy();
    let resolved = [policy.small, policy.mid, policy.frontier];
    let implementation = if resolved.iter().all(|effort| *effort == resolved[0]) {
        resolved[0].to_string()
    } else {
        format!(
            "by tier (small={}, mid={}, frontier={})",
            resolved[0], resolved[1], resolved[2]
        )
    };
    let review = if cfg.review_enabled {
        policy.review.to_string()
    } else {
        "disabled".to_owned()
    };
    format!("effort: implementation={implementation}, review={review}")
}

impl Report {
    pub fn render(&self) -> String {
        let id_width = column_width("id", self.rows.iter().map(|r| r.id.as_str()));
        let kind_width = column_width("kind", self.rows.iter().map(|r| r.kind.as_str()));
        let deps_width = column_width("deps", self.rows.iter().map(|r| r.deps.as_str()));

        let mut out = String::new();
        out.push_str(&format!(
            "{:<id_width$}  {:<kind_width$}  {:<deps_width$}  chain\n",
            "id", "kind", "deps"
        ));
        out.push_str(&format!(
            "{:-<id_width$}  {:-<kind_width$}  {:-<deps_width$}  -----\n",
            "", "", ""
        ));
        for row in &self.rows {
            out.push_str(&format!(
                "{:<id_width$}  {:<kind_width$}  {:<deps_width$}  {}\n",
                row.id, row.kind, row.deps, row.chain
            ));
        }
        out.push('\n');
        if !self.warnings.is_empty() {
            out.push_str("warnings:\n");
            for warning in &self.warnings {
                out.push_str(&format!("  - {warning}\n"));
            }
        }
        if self.gates.is_empty() {
            out.push_str("gates: none\n");
        } else {
            out.push_str(&format!(
                "gates: {} [{}]\n",
                self.gates.join(", "),
                if self.gates_from_config {
                    "from config"
                } else {
                    "derived"
                }
            ));
        }
        out.push_str(&self.review);
        out.push('\n');
        out.push_str(&self.effort);
        out.push('\n');
        out.push_str(&self.strategy);
        out.push('\n');
        out.push_str(&self.capacity);
        out.push('\n');
        out.push_str(&format!("ok: {} tasks, no cycles\n", self.plan.tasks.len()));
        out
    }

    pub fn write_normalized_json(&self, path: &Path) -> Result<(), TactusError> {
        let json = serde_json::to_string_pretty(&self.plan).map_err(|e| TactusError::Parse {
            message: format!("serializing normalized plan: {e}"),
        })?;
        fs::write(path, json + "\n").map_err(|source| TactusError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

fn column_width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> usize {
    values.map(str::len).fold(header.len(), usize::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::OnceLock;

    fn opts(plan: &str) -> ValidateOptions {
        let hermetic_root =
            env::temp_dir().join(format!("tactus-validate-hermetic-{}", std::process::id()));
        fs::create_dir_all(&hermetic_root).expect("hermetic root");
        ValidateOptions {
            plan_path: PathBuf::from(plan),
            config_path: None,
            config_root: hermetic_root,
            pools_path: Some({
                // A real, empty pools file: an explicit `--pools` that does not
                // exist is a hard error, and `None` would reach for the
                // operator's own `~/.tactus/pools.toml`.
                // Created once: identical for every caller, and rewriting one
                // shared path from parallel tests truncates it under a reader.
                static PATH: OnceLock<PathBuf> = OnceLock::new();
                PATH.get_or_init(|| {
                    let dir = env::temp_dir()
                        .join(format!("tactus-validate-nopools-{}", std::process::id()));
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
            }),
        }
    }

    #[test]
    fn a_pin_without_an_adapter_fails_validate_not_just_run() {
        // Every catalogued agent has an adapter as of step 9, so the guard is
        // driven directly rather than through a config file it can no longer be
        // reached from. §13 ships the catalog ahead of adapter support, which is
        // when this fires for real.
        let pins = vec![config::Pin {
            tier: crate::ir::Tier::Frontier,
            agent: "aider".to_owned(),
            model: "qwen-3-coder".to_owned(),
            effort: None,
        }];
        let err = check_pin_adapters(&pins, builtin_adapter, Path::new("tactus.toml"))
            .expect_err("preview must not promise a binding run would refuse");
        let message = err.to_string();
        assert!(message.contains("no adapter"), "got: {message}");
        assert!(
            message.contains("claude-code") && message.contains("copilot"),
            "lists what is available: {message}"
        );

        // And it passes what this build really does ship.
        let pins = vec![config::Pin {
            tier: crate::ir::Tier::Frontier,
            agent: "copilot".to_owned(),
            model: "gpt-5.3-codex".to_owned(),
            effort: None,
        }];
        assert!(
            check_pin_adapters(&pins, builtin_adapter, Path::new("tactus.toml")).is_ok(),
            "copilot gained an adapter in step 9"
        );
    }

    #[test]
    fn the_preview_shows_who_reviews_without_promising_a_binary_it_cannot_probe() {
        // §18: `validate` and `--dry-run` execute nothing, so they cannot check
        // that a named reviewer is installed. Saying "would be, if installed"
        // is the difference between a plan and a promise.
        let root = env::temp_dir().join(format!("tactus-validate-review-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        let plan = root.join("plan.md");
        fs::write(
            &plan,
            "## Rotate the signing key\n\
             <!-- tactus: id=rotate kind=implement depends= paths=src/auth/** -->\n\n\
             ## Note it down\n<!-- tactus: id=note kind=docs depends=rotate -->\n",
        )
        .expect("plan");
        let cfg = root.join("tactus.toml");
        fs::write(
            &cfg,
            "[[routing.overrides]]\npaths = [\"src/auth/**\"]\nsecond_opinion = \
             \"different-vendor\"\n",
        )
        .expect("config");
        let mut o = opts("unused");
        o.plan_path = plan;
        o.config_path = Some(cfg);
        let rendered = run(&o).expect("validate").render();

        assert!(
            rendered.contains("review: claude-code/claude-opus-5"),
            "{rendered}"
        );
        assert!(rendered.contains("if installed"), "{rendered}");
        assert!(
            rendered.contains("1 task(s) also require a second opinion"),
            "{rendered}"
        );
        // The per-task decision belongs in the row that explains what this
        // task's paths bought it — and only on the task whose paths matched.
        let rotate = rendered
            .lines()
            .find(|l| l.starts_with("rotate"))
            .expect("row");
        assert!(
            rotate.contains("[second opinion: copilot/gpt-5.3-codex]"),
            "{rotate}"
        );
        let note = rendered
            .lines()
            .find(|l| l.starts_with("note"))
            .expect("row");
        assert!(!note.contains("second opinion"), "{note}");
    }

    #[test]
    fn the_preview_echoes_resolved_role_tier_pin_and_disabled_review_effort() {
        let root = env::temp_dir().join(format!("tactus-validate-effort-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        let cases = [
            (
                "defaults",
                "",
                "effort: implementation=by tier (small=low, mid=medium, frontier=high), review=high",
            ),
            (
                "pin-fallback",
                "[routing]\nreview = { tier = \"small\" }\n\n\
                 [[pins]]\ntier = \"small\"\nagent = \"claude-code\"\n\
                 model = \"claude-haiku-4-5\"\neffort = \"max\"\n",
                "effort: implementation=by tier (small=max, mid=medium, frontier=high), review=max",
            ),
            (
                "other-role-values",
                "[routing.effort]\nimplementation = \"low\"\nreview = \"xhigh\"\n",
                "effort: implementation=low, review=xhigh",
            ),
            (
                "configured-role-values",
                "[routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n",
                "effort: implementation=xhigh, review=max",
            ),
            (
                "review-disabled",
                "[routing]\nreview = { enabled = false }\n",
                "effort: implementation=by tier (small=low, mid=medium, frontier=high), review=disabled",
            ),
        ];

        for (name, config, expected) in cases {
            let cfg = root.join(format!("{name}.toml"));
            fs::write(&cfg, config).expect("config");
            let mut o = opts("fixtures/sample-plan.md");
            o.config_path = Some(cfg);
            let rendered = run(&o).expect("validate").render();
            let actual = rendered
                .lines()
                .find(|line| line.starts_with("effort:"))
                .expect("effort line");
            assert_eq!(actual, expected, "case {name}:\n{rendered}");
        }
    }

    #[test]
    fn the_capacity_block_estimates_without_probing_and_never_reads_unknown_as_full() {
        let dir = env::temp_dir().join(format!("tactus-validate-pools-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let pools = dir.join("pools.toml");
        fs::write(
            &pools,
            "[pools.claude-max]\nkind = \"subscription-window\"\nagent = \
             \"claude-code\"\nwindow = \"5h\"\nweekly = true\nsources = [\"signals\", \"self\", \
             \"local-logs\"]\nprofile = \"personal\"\n",
        )
        .expect("pools");
        let mut o = opts("fixtures/sample-plan.md");
        o.pools_path = Some(pools);
        let rendered = run(&o).expect("validates").render();

        assert!(rendered.contains("claude-max"), "rendered:\n{rendered}");
        assert!(!rendered.contains("capacity: not connected"));
        assert!(rendered.contains("window=5h"), "rendered:\n{rendered}");
        // D2's seam is echoed even though nothing acts on it.
        assert!(
            rendered.contains("profile=personal"),
            "rendered:\n{rendered}"
        );
        // §13's conservatism, visible: an unmeasured pool reads as unknown, and
        // the block says that is not the same as full.
        assert!(
            rendered.contains("claude-max: unknown [unknown]"),
            "rendered:\n{rendered}"
        );
        assert!(rendered.contains("not full"), "rendered:\n{rendered}");
        // A source the estimate did not read must not pass as accounted for.
        assert!(
            rendered.contains("local-logs") && rendered.contains("not read in v0.1"),
            "rendered:\n{rendered}"
        );
        // §18: this command executes nothing, and says which side of that line
        // it is on rather than letting a preview read as a promise.
        assert!(rendered.contains("never probes"), "rendered:\n{rendered}");
        assert!(rendered.contains("read-only"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("no run in this repository yet"),
            "rendered:\n{rendered}"
        );
    }

    #[test]
    fn derived_gates_appear_in_the_preview() {
        let root = env::temp_dir().join(format!("tactus-validate-gates-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("Cargo.toml"), "[package]\nname='x'\n").expect("marker");
        let mut o = opts("fixtures/sample-plan.md");
        o.config_root = root;
        let report = run(&o).expect("validates");
        let rendered = report.render();
        assert!(
            rendered.contains("gates: check, test [derived]"),
            "rendered:\n{rendered}"
        );

        // Hermetic root with no markers: no gates, still explicit.
        let report = run(&opts("fixtures/sample-plan.md")).expect("validates");
        assert!(report.render().contains("gates: none"));
    }

    #[test]
    fn sample_plan_renders_expected_table() {
        let report = run(&opts("fixtures/sample-plan.md")).expect("sample plan validates");
        let rendered = report.render();

        assert!(rendered.contains("api-design"));
        assert!(
            rendered.contains("frontier(annotation)"),
            "rendered:\n{rendered}"
        );
        assert!(
            rendered.contains("mid(annotation)"),
            "min clip shows as annotation source"
        );
        assert!(rendered.contains("min=mid clipped the chain start"));
        assert!(rendered.contains("paths: src/api/**"));
        assert!(rendered.contains("small(default)=claude-code/claude-haiku-4-5(preview)"));
        assert!(rendered.contains("capacity: not connected"));
        assert!(rendered.contains("ok: 4 tasks, no cycles"));
    }

    #[test]
    fn bare_plan_validates_via_heuristics() {
        let report = run(&opts("fixtures/bare-plan.md")).expect("bare plan validates");
        let rendered = report.render();
        assert!(rendered.contains("ok: 5 tasks, no cycles"));
        assert!(rendered.contains("design-the-search-index-schema"));
    }

    #[test]
    fn cyclic_plan_fails_naming_the_cycle() {
        let err = run(&opts("fixtures/cyclic-plan.md")).expect_err("cycle must fail");
        let message = err.to_string();
        assert!(message.contains("dependency cycle"), "got: {message}");
        assert!(message.contains("a -> c -> b -> a"), "got: {message}");
    }

    #[test]
    fn unknown_depends_fails_clearly() {
        let dir = env::temp_dir().join(format!("tactus-validate-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("unknown-dep.md");
        fs::write(&plan, "## One\n<!-- tactus: id=one depends=ghost -->\n").expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("unknown dep must fail");
        let message = err.to_string();
        assert!(message.contains("unknown id `ghost`"), "got: {message}");
    }

    #[test]
    fn duplicate_ids_fail() {
        let dir = env::temp_dir().join(format!("tactus-validate-dup-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("dup.md");
        fs::write(
            &plan,
            "## One\n<!-- tactus: id=same -->\n\n## Two\n<!-- tactus: id=same depends= -->\n",
        )
        .expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("duplicate ids must fail");
        assert!(err.to_string().contains("duplicate task id `same`"));
    }

    #[test]
    fn steps_plan_validates_via_ordered_list_fallback() {
        let report = run(&opts("fixtures/steps-plan.md")).expect("steps plan validates");
        let rendered = report.render();
        assert!(rendered.contains("ok: 4 tasks, no cycles"));
        assert!(rendered.contains("design-the-limiter-interface-and-storage-schema"));
    }

    #[test]
    fn artifact_needed_from_a_non_dependency_warns() {
        let dir = env::temp_dir().join(format!("tactus-wiring-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("wiring.md");
        fs::write(
            &plan,
            "## Design\n<!-- tactus: id=d out=contract depends= -->\n\n\
             ## Build\n<!-- tactus: id=b needs=contract depends= -->\n",
        )
        .expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let report = run(&o).expect("wiring problems warn, not fail");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("`b` needs artifact `contract` produced by `d`")),
            "warnings: {:?}",
            report.warnings
        );

        // The sample plan wires artifacts along its dependency chain — silent.
        let clean = run(&opts("fixtures/sample-plan.md")).expect("sample validates");
        assert!(clean.warnings.is_empty(), "warnings: {:?}", clean.warnings);
    }

    #[test]
    fn unrecognized_plan_format_names_available_adapters() {
        let dir = env::temp_dir().join(format!("tactus-sniff-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("plan.json");
        fs::write(&plan, "{\"tasks\": []}\n").expect("write file");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("json must not sniff as markdown");
        assert!(err.to_string().contains("no plan adapter recognizes"));
    }

    #[test]
    fn emit_json_round_trips_through_the_ir() {
        let report = run(&opts("fixtures/sample-plan.md")).expect("sample plan validates");
        let dir = env::temp_dir().join(format!("tactus-emit-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let json_path = dir.join("plan.normalized.json");
        report
            .write_normalized_json(&json_path)
            .expect("write json");

        let text = fs::read_to_string(&json_path).expect("read back");
        let plan: Plan = serde_json::from_str(&text).expect("json matches the IR");
        assert_eq!(plan.tasks.len(), 4);
        assert_eq!(plan.source.adapter, "markdown");
        assert_eq!(plan.tasks[2].min_tier, Some(crate::ir::Tier::Mid));
    }
}
