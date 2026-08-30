//! `RunReport::render` and `RunReport::render_ledger` — the two strings a run
//! is read as (DESIGN.md §13, and §21's definition-of-done (e)).
//!
//! The half of `report` that derives nothing. It takes a [`RunReport`] the
//! parent has already projected out of the log and returns text. What is left
//! in the parent is everything that decides *what is true* about a run: the
//! wire schemas, the settled view [`super::settle`] derives, the outcome
//! [`RunReport::outcome`] computes, [`super::topo_order`], and serde. So the
//! two halves have one reason to change each (CODING_STANDARDS.md §3).
//!
//! Splitting the rendering out does not change what it renders. These stay
//! **inherent methods on `RunReport`** rather than free functions the parent
//! delegates to: an inherent impl may live in any module of its own crate, and
//! the method path is `RunReport::render` wherever the block is written. There
//! is no shim and no second name, so `crate::engine::RunReport::render` is the
//! same public API it was and every caller — `main`, `status`, `validate` —
//! is untouched. The declaration in `super` is a plain private `mod`, so
//! nothing nests under `engine::report::render` either.
//!
//! A child module also sees its ancestors' private items, which is why
//! `RunReport::committed_count` is still private to `super`. Feeding a free
//! function would have meant widening it to `pub(super)` — a real change to
//! the module's surface, dressed as a move.

use std::fmt::Write as _;

use crate::interaction::QuestionRecord;
use crate::util;

use super::{RunOutcome, RunReport, TaskRunStatus};

impl RunReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "run: {}", self.run_id);
        let _ = writeln!(out, "branch: {} (return with: git switch -)", self.branch);
        if self.gates.is_empty() {
            let _ = writeln!(out, "gates: none");
        } else {
            let _ = writeln!(
                out,
                "gates: {} [{}]",
                self.gates.join(", "),
                if self.gates_from_config {
                    "from config"
                } else {
                    "derived"
                }
            );
        }
        for warning in &self.warnings {
            let _ = writeln!(out, "warning: {warning}");
        }
        for task in &self.tasks {
            match &task.status {
                TaskRunStatus::Committed { sha } => {
                    // `?` marks a total with unreported components — the
                    // Copilot route bills nothing back, so a two-pass review
                    // shows one reviewer's spend and must not read as both.
                    let partial = if task.review_cost_incomplete { "?" } else { "" };
                    let review = match (task.review_models.as_slice(), task.review_cost_usd) {
                        ([], _) => String::new(),
                        (models, Some(cost)) => {
                            format!(" + review {} ${cost:.4}{partial}", models.join(", "))
                        }
                        // Reviewed only by routes that report no spend (§13) —
                        // say who judged it rather than imply it was free.
                        (models, None) => format!(" + review {} $?", models.join(", ")),
                    };
                    // Same rule as the reviewer half beside it, which has said
                    // `$?` since step 9: a route that reports no spend has not
                    // reported zero. `unwrap_or(0.0)` printed `$0.0000` for a
                    // codex-implemented task while the ledger three lines below
                    // correctly showed `—`, so one run said both.
                    let worker = match task.cost_usd {
                        Some(cost) => format!("${cost:.4}"),
                        None => "$?".to_owned(),
                    };
                    let _ = writeln!(
                        out,
                        "  {}: committed {sha} — {} [{}] ({:.1}s, {} {worker}{review})",
                        task.id,
                        task.title,
                        task.trail(),
                        task.duration.as_secs_f64(),
                        task.model,
                    );
                }
                TaskRunStatus::Failed { reason, .. } => {
                    let _ = writeln!(out, "  {}: FAILED [{}] — {reason}", task.id, task.trail());
                }
                TaskRunStatus::Parked { question, reason } => {
                    let _ = writeln!(
                        out,
                        "  {}: PARKED on {question} [{}] — {reason}",
                        task.id,
                        task.trail()
                    );
                }
                TaskRunStatus::Blocked { by } => {
                    let _ = writeln!(out, "  {}: blocked by `{by}`", task.id);
                }
                TaskRunStatus::Skipped => {
                    // Why it never got its turn, since the two endings are not
                    // the same thing to an operator: a halt is a decision the
                    // run reached, an interruption is one that happened to it
                    // and that `resume` undoes.
                    let ending = if self.interrupted {
                        "run interrupted"
                    } else {
                        "run halted"
                    };
                    let _ = writeln!(out, "  {}: skipped ({ending})", task.id);
                }
                TaskRunStatus::Running {
                    attempt,
                    tier,
                    model,
                } => {
                    let _ = writeln!(
                        out,
                        "  {}: running now — attempt {attempt} on {tier} ({model})",
                        task.id
                    );
                }
                TaskRunStatus::Queued => {
                    let _ = writeln!(out, "  {}: queued", task.id);
                }
                // Only reachable from a `report.json` written by a newer
                // upstroke. Say that, rather than picking a familiar-looking
                // status and being confidently wrong about someone's run.
                TaskRunStatus::Unknown => {
                    let _ = writeln!(
                        out,
                        "  {}: status not recognised by this version of upstroke",
                        task.id
                    );
                }
            }
        }
        let open: Vec<&QuestionRecord> = self.questions.iter().filter(|q| q.is_open()).collect();
        if !open.is_empty() {
            let _ = writeln!(out, "open questions ({}):", open.len());
            for record in open {
                let _ = writeln!(
                    out,
                    "  {} [{}] — {}",
                    record.question.id,
                    record.question.kind,
                    util::head(
                        record
                            .question
                            .context
                            .lines()
                            .next()
                            .unwrap_or("(no context)"),
                        120
                    )
                );
            }
            let _ = writeln!(
                out,
                "  payloads: {}",
                std::path::Path::new(".upstroke")
                    .join("runs")
                    .join(&self.run_id)
                    .join("questions")
                    .display()
            );
        }
        let _ = writeln!(
            out,
            "total: ${:.4}{} (api-equivalent)",
            self.total_cost_usd,
            if self.total_is_floor() { "?" } else { "" }
        );
        // A live run has no outcome yet, and every arm below claims one. Say
        // what is true instead: how far it has got.
        if self.running {
            let _ = writeln!(
                out,
                "run in progress: {} task(s) committed so far on {}",
                self.committed_count(),
                self.branch
            );
            return out;
        }
        // Neither has a run that stopped without recording a finish, and for
        // the same reason: there is no outcome to report yet. `outcome()`
        // cannot see that — a killed run has nothing halted, no budget stop and
        // nothing parked, which reads as `Complete` — so it used to print `run
        // complete: N task(s) committed` about a run that died mid-attempt,
        // one line above `status`'s own `state: interrupted`.
        //
        // "So far" is the live line's word on purpose: more may yet come, once
        // somebody resumes. Which is also why the resume command is not
        // repeated here — the `state:` line in `status` already carries it, and
        // saying it twice invites the two copies to drift.
        if self.interrupted {
            let _ = writeln!(
                out,
                "run interrupted: {} task(s) committed so far on {}",
                self.committed_count(),
                self.branch
            );
            return out;
        }
        match self.outcome() {
            RunOutcome::Halted => {
                let _ = writeln!(
                    out,
                    "run halted at `{}`; completed tasks are committed on {}",
                    self.halted_at.as_deref().unwrap_or("?"),
                    self.branch
                );
            }
            RunOutcome::BudgetExceeded => {
                // `outcome()` only returns this when `budget_stop` is set, so
                // the fallback is unreachable — and it says so rather than
                // naming a plausible ceiling. A specific, checkable, false
                // claim about the operator's own config is the worst thing to
                // print here.
                let stopped = self.budget_stop.as_ref().map_or_else(
                    || "run stopped at a budget it did not record".to_owned(),
                    |stop| {
                        format!(
                            "run stopped at its budget: [budgets] {} = ${:.4}, reported spend \
                             ${:.4}",
                            stop.budget, stop.limit_usd, stop.spent_usd
                        )
                    },
                );
                let _ = writeln!(
                    out,
                    "{stopped}. Committed tasks are on {}; raise the ceiling and continue \
                     with:\n    upstroke resume {} --budget <usd>",
                    self.branch, self.run_id
                );
            }
            RunOutcome::Parked => {
                let _ = writeln!(
                    out,
                    "run ended with {} task(s) parked on unanswered questions: {}",
                    self.parked_tasks().len(),
                    self.parked_tasks().join(", ")
                );
            }
            RunOutcome::Complete => {
                let committed = self.committed_count();
                let _ = writeln!(
                    out,
                    "run complete: {committed} task(s) committed on {}",
                    self.branch
                );
            }
        }
        out
    }

    /// §21's definition-of-done (e): what each task cost, and on what.
    ///
    /// Implementer and reviewer spend stay in separate columns because they
    /// are different models at different tiers — folding them together makes a
    /// cheap rung look expensive to anyone reading the ledger (§13). An
    /// unreported cost prints as `—` rather than `$0.0000`: a ledger that
    /// cannot tell free from unreported is worse than no ledger.
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        let money = |value: Option<f64>| match value {
            Some(amount) => format!("${amount:.4}"),
            None => "—".to_owned(),
        };
        // A figure that omits a reviewer whose route bills nothing back is not
        // the total, and this column is where someone decides what a run cost.
        let partial = |rendered: String, incomplete: bool| {
            if incomplete && rendered != "—" {
                format!("{rendered}?")
            } else {
                rendered
            }
        };
        let rows: Vec<[String; 6]> = self
            .tasks
            .iter()
            .map(|task| {
                [
                    task.id.clone(),
                    task.attempts.len().to_string(),
                    if task.trail().is_empty() {
                        "—".to_owned()
                    } else {
                        task.trail()
                    },
                    partial(money(task.cost_usd), task.cost_incomplete()),
                    partial(money(task.review_cost_usd), task.review_cost_incomplete),
                    partial(
                        money(task.total_cost_usd()),
                        task.cost_incomplete() || task.review_cost_incomplete,
                    ),
                ]
            })
            .collect();
        let headers = ["task", "attempts", "trail", "worker", "review", "total"];
        let widths: Vec<usize> = (0..headers.len())
            .map(|column| {
                rows.iter()
                    .map(|row| row[column].chars().count())
                    .chain(std::iter::once(headers[column].chars().count()))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let line = |cells: &[String]| {
            let mut rendered = String::from("  ");
            for (index, cell) in cells.iter().enumerate() {
                let pad = widths[index].saturating_sub(cell.chars().count());
                let _ = write!(rendered, "{cell}{:pad$}", "", pad = pad);
                if index + 1 < cells.len() {
                    rendered.push_str("  ");
                }
            }
            rendered.trim_end().to_owned()
        };

        let _ = writeln!(out, "ledger:");
        let _ = writeln!(out, "{}", line(&headers.map(str::to_owned)));
        for row in &rows {
            let _ = writeln!(out, "{}", line(row));
        }
        let _ = writeln!(
            out,
            "  total ${:.4}{} (api-equivalent; subscription spend is notional — §13)",
            self.total_cost_usd,
            if self.total_is_floor() { "?" } else { "" }
        );
        if self.total_is_floor() {
            let _ = writeln!(
                out,
                "  `?` marks a figure missing an attempt whose route reports no spend, or one \
                 the engine was killed inside — a floor, not a total (§13)"
            );
        }
        // §13's second currency. An empty section means no attempt in this run
        // named a pool — which is the honest reading of "no pools connected",
        // and is said rather than left as a blank column that looks like
        // "nothing was spent".
        if self.pool_drain.is_empty() {
            let _ = writeln!(
                out,
                "  per-pool drain: no pool is connected for the agents this run used — run \
                 `upstroke connect`"
            );
        } else {
            let _ = writeln!(out, "  per-pool drain:");
            for row in &self.pool_drain {
                let spend = match row.cost_usd {
                    Some(cost) if row.unpriced > 0 => format!("${cost:.4}?"),
                    Some(cost) => format!("${cost:.4}"),
                    // Every attempt on this pool ran on a route that reports no
                    // spend (§13) — saying "$0.0000" would read as free.
                    None => "— (this route reports no spend)".to_owned(),
                };
                let _ = writeln!(
                    out,
                    "    {}: {} attempt(s), {spend}",
                    row.pool, row.attempts
                );
            }
        }
        if let Some(stop) = &self.budget_stop {
            let _ = writeln!(
                out,
                // The ledger annotates; `render` owns the outcome line and the
                // resume advice. Printing both put two near-identical
                // paragraphs, formatted to different precision, with two copies
                // of the same command, back to back in `upstroke status` — which
                // reads as two things having happened.
                "  stopped by [budgets] {} = ${:.4} before `{}` (§13)",
                stop.budget, stop.limit_usd, stop.task
            );
        }
        out
    }
}
