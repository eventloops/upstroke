//! `tactus validate`: parse → config → graph checks → routing preview →
//! rendered report. No execution of anything.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::error::{TactusError, ValidationErrors};
use crate::ir::{Plan, Task};
use crate::plan::Parsed;
use crate::plan::markdown::MarkdownPlanAdapter;
use crate::route::{self, ResolvedChain};

#[derive(Debug, Clone)]
pub struct ValidateOptions {
    pub plan_path: PathBuf,
    /// Explicit `--config` path; `None` looks for `./tactus.toml`.
    pub config_path: Option<PathBuf>,
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
    pub task_count: usize,
}

pub fn run(opts: &ValidateOptions) -> Result<Report, TactusError> {
    let raw = fs::read_to_string(&opts.plan_path).map_err(|source| TactusError::Io {
        path: opts.plan_path.clone(),
        source,
    })?;
    let Parsed {
        plan,
        warnings: mut all_warnings,
    } = MarkdownPlanAdapter.parse_with_warnings(&raw)?;
    let cfg = config::load(
        opts.config_path.as_deref(),
        opts.pools_path.as_deref(),
        &mut all_warnings,
    )?;
    check_graph(&plan)?;
    let rows = plan
        .tasks
        .iter()
        .map(|t| to_row(t, route::resolve(t, &cfg)))
        .collect();
    Ok(Report {
        task_count: plan.tasks.len(),
        rows,
        warnings: all_warnings,
        strategy: strategy_echo(&cfg),
        plan,
    })
}

/// Duplicate ids, unknown `depends` targets, then cycles — all collected so a
/// broken plan reports everything in one run.
fn check_graph(plan: &Plan) -> Result<(), TactusError> {
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
    if problems.is_empty() {
        Ok(())
    } else {
        Err(TactusError::Validation(ValidationErrors(problems)))
    }
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
        out.push_str(&self.strategy);
        out.push('\n');
        out.push_str("capacity: not connected\n");
        out.push_str(&format!("ok: {} tasks, no cycles\n", self.task_count));
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
        ValidateOptions {
            plan_path: PathBuf::from(plan),
            config_path: None,
            pools_path: Some(
                env::temp_dir()
                    .join("tactus-validate-missing")
                    .join("p.toml"),
            ),
        }
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
