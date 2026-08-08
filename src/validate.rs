//! `tactus validate`: parse → config → graph checks → routing preview →
//! rendered report. No execution of anything.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent;
use crate::config::{self, Config};
use crate::error::{TactusError, ValidationErrors};
use crate::gates::{self, ShellGate};
use crate::ir::{Plan, Task, TaskId};
use crate::plan::{self, Parsed};
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
    // A pin naming an agent with no adapter must fail the same way in
    // `validate` and `run`; otherwise the preview promises a binding the run
    // then refuses at pre-flight (§18).
    for pin in &config.pins {
        if agent::by_id(&pin.agent).is_none() {
            return Err(TactusError::Config {
                path: opts
                    .config_path
                    .clone()
                    .unwrap_or_else(|| opts.config_root.join("tactus.toml")),
                message: format!(
                    "pin for tier `{}` names agent `{}`, which has no adapter in this build \
                     (available: {})",
                    pin.tier,
                    pin.agent,
                    agent::ADAPTERS
                        .iter()
                        .map(|a| a.id())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
    }
    let chains = plan
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

pub fn run(opts: &ValidateOptions) -> Result<Report, TactusError> {
    let analysis = analyze(opts)?;
    let mut warnings = analysis.warnings;
    // Zero-spend preview of the §14 gate pre-flight: warn, never refuse.
    gates::preview_resolution(&analysis.gates, &opts.config_root, &mut warnings);
    let rows = analysis
        .plan
        .tasks
        .iter()
        .zip(&analysis.chains)
        .map(|(task, chain)| to_row(task, chain.clone()))
        .collect();
    Ok(Report {
        rows,
        warnings,
        strategy: strategy_echo(&analysis.config),
        capacity: capacity_echo(&analysis.config),
        gates: analysis.gates.iter().map(|g| g.name.clone()).collect(),
        gates_from_config: analysis.gates_from_config,
        plan: analysis.plan,
    })
}

/// §13 is read-only until the capacity engine lands: name the pools that were
/// connected so the preview reflects what `tactus connect` actually wrote.
fn capacity_echo(cfg: &Config) -> String {
    if cfg.pool_names.is_empty() {
        "capacity: not connected".to_owned()
    } else {
        format!(
            "capacity: {} pool(s) connected — {} (estimates arrive with the capacity engine)",
            cfg.pool_names.len(),
            cfg.pool_names.join(", ")
        )
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

fn to_row(task: &Task, resolved: ResolvedChain) -> Row {
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

    fn opts(plan: &str) -> ValidateOptions {
        let hermetic_root =
            env::temp_dir().join(format!("tactus-validate-hermetic-{}", std::process::id()));
        fs::create_dir_all(&hermetic_root).expect("hermetic root");
        ValidateOptions {
            plan_path: PathBuf::from(plan),
            config_path: None,
            config_root: hermetic_root,
            pools_path: Some(
                env::temp_dir()
                    .join("tactus-validate-missing")
                    .join("p.toml"),
            ),
        }
    }

    #[test]
    fn a_pin_without_an_adapter_fails_validate_not_just_run() {
        let root = env::temp_dir().join(format!("tactus-validate-pin-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        let cfg = root.join("tactus.toml");
        // copilot is in the capability catalog but has no adapter until step 9.
        fs::write(
            &cfg,
            "[[pins]]\ntier = \"frontier\"\nagent = \"copilot\"\nmodel = \"gpt-5\"\n",
        )
        .expect("config");
        let mut o = opts("fixtures/sample-plan.md");
        o.config_path = Some(cfg);
        let err = run(&o).expect_err("preview must not promise a binding run would refuse");
        let message = err.to_string();
        assert!(message.contains("no adapter"), "got: {message}");
        assert!(
            message.contains("claude-code"),
            "lists what is available: {message}"
        );
    }

    #[test]
    fn connected_pools_are_named_in_the_capacity_line() {
        let dir = env::temp_dir().join(format!("tactus-validate-pools-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let pools = dir.join("pools.toml");
        fs::write(
            &pools,
            "[pools.claude-max]\nkind = \"subscription-window\"\nagent = \"claude-code\"\n",
        )
        .expect("pools");
        let mut o = opts("fixtures/sample-plan.md");
        o.pools_path = Some(pools);
        let rendered = run(&o).expect("validates").render();
        assert!(rendered.contains("claude-max"), "rendered:\n{rendered}");
        assert!(!rendered.contains("capacity: not connected"));
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
