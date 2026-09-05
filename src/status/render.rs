//! Extended notes: `docs/internals/status/render.md`

#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

use super::RunStatus;
use crate::events::{self, Event, EventBody};

pub(super) fn render(status: &RunStatus) -> String {
    use std::fmt::Write as _;

    let report = status.report();
    let mut out = report.render();
    out.push_str(&report.render_ledger());

    if status.running {
        let _ = writeln!(out, "state: running now (another process holds this run)");
    } else if status.interrupted_run() {
        let _ = writeln!(
            out,
            "state: interrupted — this run stopped without finishing{}. Continue it with:\n    \
             upstroke resume {}",
            if status.interrupted > 0 {
                format!(
                    ", with {} attempt(s) cut off mid-flight",
                    status.interrupted
                )
            } else {
                String::new()
            },
            status.run_id
        );
    } else if status.held {
        let _ = writeln!(
            out,
            "state: another process holds this run (a resume, most likely)"
        );
    }

    let open = status.state.open_questions();
    if !open.is_empty() {
        let _ = writeln!(out, "waiting on {} answer(s):", open.len());
        for record in open {
            let _ = writeln!(out, "    upstroke answer {}", record.question.id);
        }
    }
    let _ = writeln!(out, "transcripts: {}", status.paths.private.display());
    out
}

pub(super) fn describe(event: &Event) -> String {
    let at = event.ts.get(11..19).unwrap_or(&event.ts);
    let body = match &event.body {
        EventBody::RunStarted { data } => {
            format!("run {} started on {}", data.run_id, data.branch)
        }
        EventBody::RunResumed { data } => format!(
            "resumed at {} ({} interrupted attempt(s))",
            short(&data.head_sha),
            data.interrupted_attempts
        ),
        EventBody::RunSchemaUpgraded { data } => {
            format!("event schema upgraded from {} to {}", data.from, data.to)
        }
        EventBody::AttemptStarted {
            task,
            attempt,
            data,
            ..
        } => format!(
            "{task}: attempt {attempt} on {} ({}){}",
            data.tier,
            data.model,
            if data.resume_session.is_some() {
                ", resuming the session"
            } else {
                ""
            }
        ),
        EventBody::AttemptFinished {
            task,
            attempt,
            data,
            parking,
            transition,
            ..
        } => {
            if let Some(parking) = parking {
                let reason = data
                    .failure
                    .as_ref()
                    .map(|failure| failure.reason.as_str())
                    .unwrap_or("policy refusal");
                if let Some(events::AttemptTransition::Escalate(escalation)) = transition.as_deref()
                {
                    format!(
                        "{task}: attempt {attempt} failed — {reason}; escalating past {} to rung \
                         {} and parked on question {}",
                        escalation.tier, escalation.to_rung, parking.question.id
                    )
                } else {
                    format!(
                        "{task}: attempt {attempt} failed and parked on question {} — {reason}",
                        parking.question.id
                    )
                }
            } else {
                match &data.failure {
                    Some(failure) => match transition.as_deref() {
                        Some(events::AttemptTransition::Retry(data)) => format!(
                            "{task}: attempt {attempt} failed — {}; retrying on {}{}",
                            failure.reason,
                            data.tier,
                            if data.resume {
                                " in the same session"
                            } else {
                                ""
                            }
                        ),
                        Some(events::AttemptTransition::Escalate(data)) => format!(
                            "{task}: attempt {attempt} failed — {}; escalating past {} to rung {}",
                            failure.reason, data.tier, data.to_rung
                        ),
                        Some(events::AttemptTransition::Defer(data)) => format!(
                            "{task}: attempt {attempt} failed — {}; deferred ({}) — {}",
                            failure.reason, data.defers, data.reason
                        ),
                        Some(events::AttemptTransition::Fail(data)) => format!(
                            "{task}: attempt {attempt} failed — {}; task failed ({:?})",
                            failure.reason, data.kind
                        ),
                        None => format!("{task}: attempt {attempt} failed — {}", failure.reason),
                    },
                    None => format!("{task}: attempt {attempt} passed"),
                }
            }
        }
        EventBody::AttemptInterrupted { task, attempt, .. } => format!(
            "{task}: attempt {attempt} was cut off mid-flight; its spend is unknown and the \
             rung's allowance is intact"
        ),
        EventBody::LadderRetry { task, data, .. } => format!(
            "{task}: retrying on {}{}",
            data.tier,
            if data.resume {
                " in the same session"
            } else {
                ""
            }
        ),
        EventBody::LadderEscalated { task, data, .. } => {
            format!(
                "{task}: escalating past {} to rung {}",
                data.tier, data.to_rung
            )
        }
        EventBody::TaskDeferred { task, data } => {
            format!("{task}: deferred ({}) — {}", data.defers, data.reason)
        }
        EventBody::DeferWaitElapsed { data } => {
            format!("waited {}s for a pool to come back", data.waited.as_secs())
        }
        EventBody::TaskParked { task, data } => {
            format!("{task}: parked on {}", data.question)
        }
        EventBody::TaskCommitted { task, data } => {
            format!("{task}: committed {}", short(&data.sha))
        }
        EventBody::TaskFailed { task, data } => format!("{task}: failed — {}", data.reason),
        EventBody::QuestionRaised { task, data } => format!(
            "{task}: asking {} — answer with `upstroke answer {}`",
            data.question.kind, data.question.id
        ),
        EventBody::QuestionAnswered { data } => {
            format!("{} answered via {}", data.question, data.via)
        }
        EventBody::DesignDefect { data } => {
            format!("design defect recorded for {}", data.question)
        }
        EventBody::CapacitySnapshot { data } => format!(
            "capacity snapshot under `{}`: {}",
            data.strategy,
            if data.pools.is_empty() {
                "no pools connected".to_owned()
            } else {
                data.pools
                    .iter()
                    .map(|pool| format!("{} {} [{}]", pool.pool, pool.remaining, pool.confidence))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ),
        EventBody::PoolExhausted { task, data } => format!(
            "{task}: pool `{}` reported exhausted{}",
            data.pool,
            match &data.reset_at {
                Some(at) => format!(", resets {at}"),
                None => ", reset time unknown".to_owned(),
            }
        ),
        EventBody::BudgetExceeded { data } => format!(
            "budget {} = ${:.2} reached at ${:.4}; `{}` did not start",
            data.budget, data.limit_usd, data.spent_usd, data.task
        ),
        EventBody::RunFinished { data } => format!(
            "run finished: {:?} ({} committed, {} parked)",
            data.outcome, data.committed, data.parked
        ),
    };
    format!("{at}  {body}")
}

fn short(sha: &str) -> String {
    sha.chars().take(10).collect()
}
