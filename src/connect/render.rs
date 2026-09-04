//! Extended notes: `docs/internals/connect/render.md`

#![deny(clippy::disallowed_methods, clippy::disallowed_macros)]

use std::fmt::Write as _;

use crate::capacity::Pool;
use crate::util;

use super::{AgentReport, ConnectReport, Wrote};

pub(super) fn pools_file(agents: &[AgentReport]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Written by `upstroke connect` v{} on {}.\n\
         #\n\
         # Pools are user-level (§17): they describe YOUR subscriptions, not this repo. The file\n\
         # is hand-editable and `upstroke connect` will not overwrite your edits without --force.\n\
         #\n\
         # Model roster provenance: catalog {}, the static capability table shipped with this\n\
         # binary. Neither agent CLI offers non-interactive model enumeration as of this writing,\n\
         # so nothing here was cross-checked against what your installed CLI actually accepts.\n\
         #\n\
         # `profile` selects between several accounts on one vendor (§13). It is parsed, shown by\n\
         # `upstroke capacity`, and acted on by nothing in v0.1 — add it when v0.2 wires it up.",
        env!("CARGO_PKG_VERSION"),
        util::rfc3339_utc_now(),
        env!("CARGO_PKG_VERSION"),
    );

    for report in agents {
        out.push('\n');
        match (&report.outcome, &report.pool) {
            (Ok(discovery), Some(pool)) => {
                let _ = writeln!(out, "# {}: {}", report.agent, discovery.auth);
                for note in &discovery.notes {
                    let _ = writeln!(out, "#   {note}");
                }
                if discovery.shape.is_none() {
                    let _ = writeln!(
                        out,
                        "#   kind below is a default, not something detected — change it if your \
                         plan differs"
                    );
                }
                out.push_str(&pool_section(pool));
            }
            _ => {
                let _ = writeln!(
                    out,
                    "# {}: not usable on this machine, so no pool was written for it.",
                    report.agent
                );
            }
        }
    }
    out
}

fn pool_section(pool: &Pool) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[pools.{}]", pool.name);
    let _ = writeln!(out, "kind = \"{}\"", pool.kind);
    let _ = writeln!(out, "agent = \"{}\"", pool.agent);
    if let Some(window) = pool.window {
        let _ = writeln!(
            out,
            "window = \"{}\"",
            crate::capacity::render_duration(window)
        );
    }
    if pool.weekly {
        let _ = writeln!(out, "weekly = true");
    }
    let sources: Vec<String> = pool.sources.iter().map(|s| format!("\"{s}\"")).collect();
    let _ = writeln!(out, "sources = [{}]", sources.join(", "));
    let _ = writeln!(out, "safety_margin = {:.2}", pool.safety_margin);
    let _ = writeln!(
        out,
        "reserve = {:.2}                     # headroom kept for your own interactive sessions",
        pool.reserve
    );
    if let Some(profile) = &pool.profile {
        let _ = writeln!(out, "profile = \"{profile}\"");
    }
    if let crate::capacity::Allowance::Units(units) = pool.monthly_allowance {
        let _ = writeln!(out, "monthly_allowance = {units}");
    }
    if let Some(endpoint) = &pool.endpoint {
        let _ = writeln!(out, "endpoint = \"{endpoint}\"");
    }
    out
}

pub(super) fn report(report: &ConnectReport) -> String {
    let mut out = String::new();
    for agent in &report.agents {
        match (&agent.outcome, &agent.pool) {
            (Ok(discovery), Some(pool)) => {
                let _ = writeln!(
                    out,
                    "{}: {} — pool `{}` [{}]",
                    agent.agent, discovery.auth, pool.name, pool.kind
                );
                for note in &discovery.notes {
                    let _ = writeln!(out, "  {note}");
                }
            }
            (Err(error), _) => {
                let _ = writeln!(out, "{}: skipped — {error}", agent.agent);
            }
            (Ok(_), None) => {}
        }
    }
    for warning in &report.warnings {
        let _ = writeln!(out, "warning: {warning}");
    }
    match report.outcome {
        Wrote::Written => {
            let _ = writeln!(out, "wrote {}", report.path.display());
        }
        Wrote::Unchanged => {
            let _ = writeln!(out, "unchanged: {}", report.path.display());
        }
        Wrote::Refused => {
            let _ = writeln!(
                out,
                "{} already exists and differs from what connect would write. That file is \
                 hand-editable (§17), so it is not overwritten silently.\n\nWhat connect would \
                 write:\n{}\nRe-run with --force to replace it.",
                report.path.display(),
                indent(&report.content)
            );
        }
    }
    out
}

fn indent(text: &str) -> String {
    text.lines().map(|line| format!("  {line}\n")).collect()
}
