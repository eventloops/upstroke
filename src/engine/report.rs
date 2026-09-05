//! Extended notes: `docs/internals/engine/report.md`

use std::fmt::Write as _;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::capacity;
use crate::events::{self, AttemptRecord, Progress, RunState, TaskState};
use crate::interaction::QuestionRecord;
use crate::ir::{Plan, Task};
use crate::ladder::FailureKind;
use crate::util;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TaskRunStatus {
    Committed {
        sha: String,
    },
    Failed {
        kind: FailureKind,
        reason: String,
    },
    Parked {
        question: String,
        reason: String,
    },
    Blocked {
        by: String,
    },
    Skipped,
    Running {
        attempt: u32,
        tier: String,
        model: String,
    },
    Queued,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReport {
    pub id: String,
    pub title: String,
    pub model: String,
    pub status: TaskRunStatus,
    pub duration: Duration,
    pub cost_usd: Option<f64>,
    pub review_models: Vec<String>,
    pub review_cost_usd: Option<f64>,
    pub review_cost_incomplete: bool,
    pub session_id: Option<String>,
    pub attempts: Vec<AttemptRecord>,
}

impl TaskReport {
    pub fn total_cost_usd(&self) -> Option<f64> {
        match (self.cost_usd, self.review_cost_usd) {
            (None, None) => None,
            (worker, review) => Some(worker.unwrap_or(0.0) + review.unwrap_or(0.0)),
        }
    }

    pub fn cost_incomplete(&self) -> bool {
        self.attempts.iter().any(|record| record.cost_usd.is_none())
    }

    pub fn trail(&self) -> String {
        let mut parts: Vec<(String, u32, bool)> = Vec::new();
        for record in &self.attempts {
            let failed = record.failure.is_some();
            match parts.last_mut() {
                Some((tier, count, last_failed)) if *tier == record.tier => {
                    *count += 1;
                    *last_failed = failed;
                }
                _ => parts.push((record.tier.clone(), 1, failed)),
            }
        }
        parts
            .into_iter()
            .map(|(tier, count, failed)| {
                let count = if count > 1 {
                    format!("×{count}")
                } else {
                    String::new()
                };
                let verdict = if failed { "failed" } else { "ok" };
                format!("{tier}{count} {verdict}")
            })
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Complete,
    Halted,
    BudgetExceeded,
    Parked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub branch: String,
    pub gates: Vec<String>,
    pub gates_from_config: bool,
    pub warnings: Vec<String>,
    pub tasks: Vec<TaskReport>,
    pub halted_at: Option<String>,
    pub questions: Vec<QuestionRecord>,
    #[serde(default)]
    pub budget_stop: Option<events::BudgetExceeded>,
    pub total_cost_usd: f64,
    #[serde(default)]
    pub pool_drain: Vec<PoolDrainRow>,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub interrupted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolDrainRow {
    pub pool: String,
    pub attempts: u32,
    pub cost_usd: Option<f64>,
    pub unpriced: u32,
}

impl RunReport {
    pub fn parked_tasks(&self) -> Vec<&str> {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskRunStatus::Parked { .. }))
            .map(|t| t.id.as_str())
            .collect()
    }

    pub fn total_is_floor(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.cost_incomplete() || task.review_cost_incomplete)
    }

    fn committed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskRunStatus::Committed { .. }))
            .count()
    }

    pub fn outcome(&self) -> RunOutcome {
        if self.halted_at.is_some() {
            RunOutcome::Halted
        } else if self.budget_stop.is_some() {
            RunOutcome::BudgetExceeded
        } else if self.parked_tasks().is_empty() {
            RunOutcome::Complete
        } else {
            RunOutcome::Parked
        }
    }
}

impl RunReport {
    pub fn from_state(
        started: &events::RunStarted,
        plan: &Plan,
        state: &RunState,
        warnings: Vec<String>,
        running: bool,
        interrupted: bool,
    ) -> Self {
        build_report(
            ReportHeader {
                run_id: &started.run_id,
                branch: &started.branch,
                gates: started.gates.clone(),
                gates_from_config: started.gates_from_config,
                warnings,
                running,
                interrupted,
            },
            plan,
            state,
        )
    }
}

pub(super) struct ReportHeader<'a> {
    pub(super) run_id: &'a str,
    pub(super) branch: &'a str,
    pub(super) gates: Vec<String>,
    pub(super) gates_from_config: bool,
    pub(super) warnings: Vec<String>,
    pub(super) running: bool,
    pub(super) interrupted: bool,
}

pub(super) fn build_report(header: ReportHeader<'_>, plan: &Plan, state: &RunState) -> RunReport {
    let ReportHeader {
        run_id,
        branch,
        gates,
        gates_from_config,
        warnings,
        running,
        interrupted,
    } = header;
    let settled = settle(plan, &state.states, running);
    let tasks: Vec<TaskReport> = state
        .order
        .iter()
        .copied()
        .chain((0..plan.tasks.len()).filter(|i| !state.order.contains(i)))
        .map(|index| {
            task_report(
                &plan.tasks[index],
                &settled[index],
                &state.progress[index],
                running,
            )
        })
        .collect();
    let total_cost_usd = total_of(&tasks);
    let pool_drain = capacity::drain_of(state.progress.iter().flat_map(|p| p.records.iter()))
        .into_iter()
        .map(|(pool, spend)| PoolDrainRow {
            pool,
            attempts: spend.attempts,
            cost_usd: spend.usd,
            unpriced: spend.unpriced,
        })
        .collect();
    RunReport {
        run_id: run_id.to_owned(),
        branch: branch.to_owned(),
        gates,
        gates_from_config,
        warnings,
        tasks,
        halted_at: state.halted_at.clone(),
        questions: state.questions.clone(),
        budget_stop: state.budget_stop.clone(),
        total_cost_usd,
        pool_drain,
        running,
        interrupted,
    }
}

fn settle(plan: &Plan, states: &[TaskState], running: bool) -> Vec<TaskState> {
    let tasks = &plan.tasks;
    let mut settled = states.to_vec();
    loop {
        let mut changed = false;
        for index in 0..tasks.len() {
            if settled[index] != TaskState::Pending {
                continue;
            }
            let blocker = tasks[index].depends_on.iter().find(|dep| {
                tasks
                    .iter()
                    .position(|t| t.id == **dep)
                    .is_some_and(|j| blocks_dependents(&settled[j], running))
            });
            if let Some(blocker) = blocker {
                settled[index] = TaskState::Blocked(blocker.to_string());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    if !running {
        for state in &mut settled {
            if *state == TaskState::Pending {
                *state = TaskState::Skipped;
            }
        }
    }
    settled
}

fn blocks_dependents(state: &TaskState, running: bool) -> bool {
    match state {
        TaskState::Done(_) => false,
        TaskState::Pending | TaskState::Deferred => !running,
        TaskState::AwaitingInput(_)
        | TaskState::Failed { .. }
        | TaskState::Blocked(_)
        | TaskState::Skipped => true,
    }
}

pub(super) fn last_reason(progress: &Progress) -> String {
    progress
        .records
        .last()
        .and_then(|r| r.failure.as_ref())
        .map(|f| f.reason.clone())
        .or_else(|| {
            progress
                .feedback
                .iter()
                .rev()
                .find(|f| !f.human)
                .map(|f| f.summary.clone())
        })
        .unwrap_or_else(|| "no attempt on record".to_owned())
}

pub(super) fn task_report(
    task: &Task,
    state: &TaskState,
    progress: &Progress,
    running: bool,
) -> TaskReport {
    let records = &progress.records;
    let last = records.last();
    TaskReport {
        id: task.id.to_string(),
        title: task.title.clone(),
        model: last.map(|r| r.model.clone()).unwrap_or_default(),
        status: match state {
            TaskState::Done(sha) => TaskRunStatus::Committed { sha: sha.clone() },
            TaskState::Failed { kind, reason } => TaskRunStatus::Failed {
                kind: *kind,
                reason: reason.clone(),
            },
            TaskState::AwaitingInput(question) => TaskRunStatus::Parked {
                question: question.to_string(),
                reason: last_reason(progress),
            },
            TaskState::Blocked(by) => TaskRunStatus::Blocked { by: by.clone() },
            TaskState::Deferred | TaskState::Pending => match &progress.in_flight {
                Some(flight) if running => TaskRunStatus::Running {
                    attempt: flight.attempt,
                    tier: flight.tier.clone(),
                    model: flight.model.clone(),
                },
                None if running => TaskRunStatus::Queued,
                _ => TaskRunStatus::Skipped,
            },
            TaskState::Skipped => TaskRunStatus::Skipped,
        },
        duration: records.iter().map(|r| r.duration).sum(),
        cost_usd: sum_opt(records.iter().map(|r| r.cost_usd)),
        review_models: {
            let mut seen: Vec<String> = Vec::new();
            for model in records.iter().flat_map(AttemptRecord::review_models) {
                if !seen.contains(&model) {
                    seen.push(model);
                }
            }
            seen
        },
        review_cost_usd: sum_opt(records.iter().map(AttemptRecord::review_cost_usd)),
        review_cost_incomplete: records.iter().any(AttemptRecord::review_cost_incomplete),
        session_id: last.and_then(|r| r.session_id.clone()),
        attempts: records.clone(),
    }
}

pub(super) fn total_of(tasks: &[TaskReport]) -> f64 {
    tasks
        .iter()
        .filter_map(TaskReport::total_cost_usd)
        .fold(0.0, |total, cost| total + cost)
}

pub(super) fn sum_opt(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut total: Option<f64> = None;
    for value in values.flatten() {
        total = Some(total.unwrap_or(0.0) + value);
    }
    total
}

pub fn topo_order(plan: &Plan) -> Vec<usize> {
    let mut done = vec![false; plan.tasks.len()];
    let mut order = Vec::with_capacity(plan.tasks.len());
    let index_of = |id: &str| plan.tasks.iter().position(|t| t.id.as_str() == id);
    while order.len() < plan.tasks.len() {
        let mut advanced = false;
        for i in 0..plan.tasks.len() {
            if done[i] {
                continue;
            }
            let ready = plan.tasks[i]
                .depends_on
                .iter()
                .all(|d| index_of(d.as_str()).is_none_or(|j| done[j]));
            if ready {
                done[i] = true;
                order.push(i);
                advanced = true;
                break;
            }
        }
        if !advanced {
            for (i, flag) in done.iter_mut().enumerate() {
                if !*flag {
                    *flag = true;
                    order.push(i);
                }
            }
        }
    }
    order
}

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
                    let partial = if task.review_cost_incomplete { "?" } else { "" };
                    let review = match (task.review_models.as_slice(), task.review_cost_usd) {
                        ([], _) => String::new(),
                        (models, Some(cost)) => {
                            format!(" + review {} ${cost:.4}{partial}", models.join(", "))
                        }
                        (models, None) => format!(" + review {} $?", models.join(", ")),
                    };
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
        if self.running {
            let _ = writeln!(
                out,
                "run in progress: {} task(s) committed so far on {}",
                self.committed_count(),
                self.branch
            );
            return out;
        }
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

    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        let money = |value: Option<f64>| match value {
            Some(amount) => format!("${amount:.4}"),
            None => "—".to_owned(),
        };
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
                "  stopped by [budgets] {} = ${:.4} before `{}` (§13)",
                stop.budget, stop.limit_usd, stop.task
            );
        }
        out
    }
}
