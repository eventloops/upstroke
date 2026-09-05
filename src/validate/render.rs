//! Extended notes: `docs/internals/validate/render.md`

#![deny(clippy::disallowed_methods)]

use crate::capacity;
use crate::config::Config;
use crate::ir::Task;
use crate::review::{PassBinding, ReviewPlan};
use crate::route::ResolvedChain;

use super::{Report, Row};

pub(super) fn review_echo(plan: &ReviewPlan) -> String {
    let Some(primary) = &plan.primary else {
        return "review: disabled ([routing] review = { enabled = false })".to_owned();
    };
    #[expect(
        clippy::expect_used,
        reason = "resolve() sets a timeout on every plan it returns"
    )]
    let mut line = format!(
        "review: {} ({}s independent timeout per pass)",
        primary.describe(),
        plan.pass_timeout_secs
            .expect("freshly resolved review plans always record their timeout")
    );
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

pub(super) fn capacity_echo(
    cfg: &Config,
    obs: &capacity::Observations,
    run: Option<&str>,
) -> String {
    use std::fmt::Write as _;

    if cfg.pools.is_empty() {
        return "capacity: not connected — run `upstroke connect` to write ~/.upstroke/pools.toml"
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
        "  this preview reads files only and never probes (§18) — `upstroke capacity` asks the \
         installed CLIs as well"
    );
    out
}

pub(super) fn to_row(
    task: &Task,
    resolved: ResolvedChain,
    second_opinion: Option<&PassBinding>,
) -> Row {
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

pub(super) fn strategy_echo(cfg: &Config) -> String {
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

pub(super) fn effort_echo(cfg: &Config) -> String {
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

pub(super) fn report(report: &Report) -> String {
    let id_width = column_width("id", report.rows.iter().map(|r| r.id.as_str()));
    let kind_width = column_width("kind", report.rows.iter().map(|r| r.kind.as_str()));
    let deps_width = column_width("deps", report.rows.iter().map(|r| r.deps.as_str()));

    let mut out = String::new();
    out.push_str(&format!(
        "{:<id_width$}  {:<kind_width$}  {:<deps_width$}  chain\n",
        "id", "kind", "deps"
    ));
    out.push_str(&format!(
        "{:-<id_width$}  {:-<kind_width$}  {:-<deps_width$}  -----\n",
        "", "", ""
    ));
    for row in &report.rows {
        out.push_str(&format!(
            "{:<id_width$}  {:<kind_width$}  {:<deps_width$}  {}\n",
            row.id, row.kind, row.deps, row.chain
        ));
    }
    out.push('\n');
    if !report.warnings.is_empty() {
        out.push_str("warnings:\n");
        for warning in &report.warnings {
            out.push_str(&format!("  - {warning}\n"));
        }
    }
    if report.gates.is_empty() {
        out.push_str("gates: none\n");
    } else {
        out.push_str(&format!(
            "gates: {} [{}]\n",
            report.gates.join(", "),
            if report.gates_from_config {
                "from config"
            } else {
                "derived"
            }
        ));
    }
    out.push_str(&report.review);
    out.push('\n');
    out.push_str(&report.effort);
    out.push('\n');
    out.push_str(&report.strategy);
    out.push('\n');
    out.push_str(&report.capacity);
    out.push('\n');
    out.push_str(&format!(
        "ok: {} tasks, no cycles\n",
        report.plan.tasks.len()
    ));
    out
}

fn column_width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> usize {
    values.map(str::len).fold(header.len(), usize::max)
}
