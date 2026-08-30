//! The settled view and the one-line event descriptions (DESIGN.md §15).
//!
//! The half of `status` that touches nothing. It takes a `RunStatus` the parent
//! has already folded out of the log and returns a `String`, and it turns one
//! `Event` into one line for `--follow`. What is left in the parent is
//! everything that does reach the world — reading the log, probing the lock,
//! and streaming to a sink — so the two halves have one reason to change each
//! (CODING_STANDARDS.md §3).
//!
//! Splitting the rendering out does not change what it renders: the parent's
//! `render` and `describe` are the public surface and delegate here, and this
//! module is private.
//!
//! # Why the effect denials are restored here
//!
//! `status` carries a module-level allow of `clippy::disallowed_methods` and
//! `clippy::disallowed_types`, recorded in the **frozen legacy section** of
//! `effects/allowlist.toml` — earned by `follow`, which writes to an
//! `io::Write` sink, and by the husk fixtures, which build run directories with
//! raw `fs` and a `git` subprocess. Lint levels descend through the module
//! tree, so that allowance would reach this file for free.
//!
//! It has no business doing so. Nothing below writes a file, starts a process,
//! or streams to a sink: the view is accumulated into a `String` through
//! `std::fmt::Write`, whose `write_fmt` is a different `DefId` from the denied
//! `io::Write::write_fmt` and is not an effect. Restoring the two denials makes
//! an effect added here a build error rather than something the parent's
//! allowance quietly covers — and it is why this file needs no allowlist row of
//! its own, since an allowance is what that file records and this module takes
//! none.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

use super::RunStatus;
use crate::events::{self, Event, EventBody};

/// The settled view, assembled: the report and its ledger, then the trailing
/// lines that say whether it is still moving and what it is waiting for.
pub(super) fn render(status: &RunStatus) -> String {
    use std::fmt::Write as _;

    let report = status.report();
    let mut out = report.render();
    out.push_str(&report.render_ledger());

    // Liveness first among the trailing lines, because it decides whether any
    // of the above is still moving.
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
        // Finished, and somebody has claimed it anyway — a `resume` between
        // taking the lock and writing `run_resumed`. The outcome above is still
        // this run's outcome; it may just not be the last word for long.
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

/// One line per event: the wall-clock time out of the record's own timestamp,
/// then the body.
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
        } => describe_attempt_finished(
            task,
            *attempt,
            data,
            parking.as_deref(),
            transition.as_deref(),
        ),
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

/// The line for one finished attempt: the outcome, then the decision recorded
/// with it.
///
/// A parked attempt reports its park — and any escalation that landed with it
/// atomically — in one sentence; a parked record carrying no failure is a
/// policy refusal. Otherwise the line is the failure and what the ladder does
/// next, or simply that the attempt passed.
fn describe_attempt_finished(
    task: &str,
    attempt: u32,
    record: &events::AttemptRecord,
    parking: Option<&events::AttemptParking>,
    transition: Option<&events::AttemptTransition>,
) -> String {
    if let Some(parking) = parking {
        let reason = record
            .failure
            .as_ref()
            .map(|failure| failure.reason.as_str())
            .unwrap_or("policy refusal");
        if let Some(events::AttemptTransition::Escalate(escalation)) = transition {
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
        match &record.failure {
            Some(failure) => match transition {
                Some(events::AttemptTransition::Retry(retry)) => format!(
                    "{task}: attempt {attempt} failed — {}; retrying on {}{}",
                    failure.reason,
                    retry.tier,
                    if retry.resume {
                        " in the same session"
                    } else {
                        ""
                    }
                ),
                Some(events::AttemptTransition::Escalate(escalation)) => format!(
                    "{task}: attempt {attempt} failed — {}; escalating past {} to rung {}",
                    failure.reason, escalation.tier, escalation.to_rung
                ),
                Some(events::AttemptTransition::Defer(deferral)) => format!(
                    "{task}: attempt {attempt} failed — {}; deferred ({}) — {}",
                    failure.reason, deferral.defers, deferral.reason
                ),
                Some(events::AttemptTransition::Fail(failed)) => format!(
                    "{task}: attempt {attempt} failed — {}; task failed ({:?})",
                    failure.reason, failed.kind
                ),
                None => format!("{task}: attempt {attempt} failed — {}", failure.reason),
            },
            None => format!("{task}: attempt {attempt} passed"),
        }
    }
}

fn short(sha: &str) -> String {
    sha.chars().take(10).collect()
}
