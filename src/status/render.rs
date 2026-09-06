//! Extended notes: `docs/internals/status/render.md`
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fmt::Write as _;

use super::RunStatus;
use crate::events::{
    AttemptRecord, AttemptTransition, Event, EventBody, LadderEscalated, LadderRetry,
    ReviewPassOutcome, RunOutcome, TaskDeferred, TaskFailed,
};
use crate::ir::Answer;
use crate::util::terminal::{TerminalLines, one_line};

pub(super) fn render(status: &RunStatus) -> String {
    let report = status.report();
    let mut rendered = report.render();
    rendered.push_str(&report.render_ledger());
    let mut out = TerminalLines::default();

    if status.running {
        out.push(format_args!(
            "state: running now (another process holds this run)"
        ));
    } else if status.interrupted_run() {
        out.push(format_args!(
            "state: interrupted — this run stopped without finishing{}. Continue it with:",
            if status.interrupted > 0 {
                format!(
                    ", with {} attempt(s) cut off mid-flight",
                    status.interrupted
                )
            } else {
                String::new()
            }
        ));
        out.push(format_args!("    upstroke resume {}", status.run_id));
    } else if status.held {
        out.push(format_args!(
            "state: another process holds this run (a resume, most likely)"
        ));
    }

    let open = status.state.open_questions();
    if !open.is_empty() {
        out.push(format_args!("waiting on {} answer(s):", open.len()));
        for record in open {
            out.push(format_args!("    upstroke answer {}", record.question.id));
        }
    }
    out.push(format_args!(
        "transcripts: {}",
        status.paths.private.display()
    ));
    rendered.push_str(&out.into_string());
    rendered
}

pub(super) fn describe(event: &Event) -> String {
    let (at, zone) = clock_of(&event.ts).unwrap_or((event.ts.as_str(), ""));
    let body = match &event.body {
        EventBody::RunStarted { data } => {
            format!("run {} started on {}", data.run_id, data.branch)
        }
        EventBody::RunResumed { data } => {
            let mut line = format!(
                "resumed at {} ({} interrupted attempt(s))",
                short(&data.head_sha),
                data.interrupted_attempts
            );
            if !data.discarded.is_empty() {
                let _ = write!(
                    line,
                    "; {} uncommitted path(s) discarded",
                    data.discarded.len()
                );
            }
            line
        }
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
            let mut line = format!("{task}: attempt {attempt} {}", attempt_outcome(data));
            if let Some(transition) = transition.as_deref() {
                let _ = write!(line, "; {}", describe_transition(transition));
            }
            if let Some(parking) = parking.as_deref() {
                let _ = write!(line, "; parked on question {}", parking.question.id);
            }
            line
        }
        EventBody::AttemptInterrupted { task, attempt, .. } => format!(
            "{task}: attempt {attempt} was cut off mid-flight; its spend is unknown and the \
             rung's allowance is intact"
        ),
        EventBody::LadderRetry { task, data, .. } => format!("{task}: {}", describe_retry(data)),
        EventBody::LadderEscalated { task, data, .. } => {
            format!("{task}: {}", describe_escalation(data))
        }
        EventBody::TaskDeferred { task, data } => {
            format!("{task}: {}", describe_deferral(data))
        }
        EventBody::DeferWaitElapsed { data } => {
            let waited_ms = data.waited.as_millis();
            format!(
                "waited {}.{:03}s for a pool to come back (round {})",
                waited_ms / 1000,
                waited_ms % 1000,
                data.round
            )
        }
        EventBody::TaskParked { task, data } => {
            format!("{task}: parked on {}", data.question)
        }
        EventBody::TaskCommitted { task, data } => {
            format!("{task}: committed {}", short(&data.sha))
        }
        EventBody::TaskFailed { task, data } => format!("{task}: {}", describe_task_failure(data)),
        EventBody::QuestionRaised { task, data } => format!(
            "{task}: asking {} — answer with `upstroke answer {}`",
            data.question.kind, data.question.id
        ),
        EventBody::QuestionAnswered { data } => match &data.answer {
            Answer::Answered { .. } => format!("{} answered via {}", data.question, data.via),
            Answer::Declined => format!(
                "{} declined via {}; the halt policy frozen with it {}",
                data.question,
                data.via,
                match data.decline_halts_run {
                    Some(true) => "says the run halts",
                    Some(false) => "says the run continues",
                    None => "was not recorded",
                }
            ),
            Answer::Unanswered => format!(
                "{} went unanswered via {}: no channel reached a person",
                data.question, data.via
            ),
        },
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
        EventBody::RunFinished { data } => {
            let outcome = match data.outcome {
                RunOutcome::Complete => "complete",
                RunOutcome::Parked => "parked",
                RunOutcome::Halted => "halted",
                RunOutcome::BudgetExceeded => "stopped at its budget",
            };
            let mut line = format!("run finished: {outcome}");
            match (&data.outcome, &data.halted_at) {
                (RunOutcome::Halted, Some(task)) => {
                    let _ = write!(line, " at `{task}`");
                }
                (RunOutcome::Halted, None) => {
                    line.push_str(" at a task the record does not name");
                }
                (_, Some(task)) => {
                    let _ = write!(
                        line,
                        " (`{task}` recorded as halted_at on a run that did not halt)"
                    );
                }
                (_, None) => {}
            }
            let _ = write!(
                line,
                " ({} committed, {} parked)",
                data.committed, data.parked
            );
            line
        }
    };
    one_line(format!("{at}{zone}  {body}"))
}

fn attempt_outcome(record: &AttemptRecord) -> String {
    let verdict = record.reviews.iter().find_map(|pass| match pass.outcome {
        ReviewPassOutcome::Passed => None,
        ReviewPassOutcome::Failed => Some(format!(
            "review `{}` ({}) rejected it",
            pass.pass, pass.model
        )),
        ReviewPassOutcome::Unavailable => Some(format!(
            "review `{}` ({}) reached no verdict",
            pass.pass, pass.model
        )),
    });
    match (&record.failure, verdict) {
        (Some(failure), Some(verdict)) => format!("failed — {}; {verdict}", failure.reason),
        (Some(failure), None) => format!("failed — {}", failure.reason),
        (None, Some(verdict)) => format!("was not approved — {verdict}"),
        (None, None) => "passed".to_owned(),
    }
}

fn describe_transition(transition: &AttemptTransition) -> String {
    match transition {
        AttemptTransition::Retry(data) => describe_retry(data),
        AttemptTransition::Escalate(data) => describe_escalation(data),
        AttemptTransition::Defer(data) => describe_deferral(data),
        AttemptTransition::Fail(data) => describe_task_failure(data),
    }
}

fn describe_task_failure(data: &TaskFailed) -> String {
    format!(
        "task failed ({:?}) — {}{}",
        data.kind,
        data.reason,
        halt_suffix(data.halts_run)
    )
}

fn describe_retry(data: &LadderRetry) -> String {
    format!(
        "retrying on {}{}",
        data.tier,
        if data.resume {
            " in the same session"
        } else {
            ""
        }
    )
}

fn describe_escalation(data: &LadderEscalated) -> String {
    format!("escalating past {} to rung {}", data.tier, data.to_rung)
}

fn describe_deferral(data: &TaskDeferred) -> String {
    format!("deferred ({}) — {}", data.defers, data.reason)
}

fn halt_suffix(halts_run: bool) -> &'static str {
    if halts_run {
        "; the run halts"
    } else {
        "; the run continues"
    }
}

fn clock_of(ts: &str) -> Option<(&str, &str)> {
    let bytes = ts.as_bytes();
    let field = |range: std::ops::Range<usize>, low: u32, high: u32| -> Option<u32> {
        let digits = bytes.get(range)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let value = digits
            .iter()
            .fold(0u32, |acc, digit| acc * 10 + u32::from(digit - b'0'));
        (low..=high).contains(&value).then_some(value)
    };
    let byte_at = |index: usize, expected: u8| -> Option<()> {
        (*bytes.get(index)? == expected).then_some(())
    };
    let year = field(0..4, 0, 9999)?;
    byte_at(4, b'-')?;
    let month = field(5..7, 1, 12)?;
    byte_at(7, b'-')?;
    let days = match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    field(8..10, 1, days)?;
    if !matches!(bytes.get(10), Some(b'T' | b't')) {
        return None;
    }
    field(11..13, 0, 23)?;
    byte_at(13, b':')?;
    field(14..16, 0, 59)?;
    byte_at(16, b':')?;
    field(17..19, 0, 59)?;
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        let digits = bytes
            .get(index + 1..)?
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        index += 1 + digits;
    }
    match bytes.get(index)? {
        b'Z' | b'z' => {
            if bytes.len() != index + 1 {
                return None;
            }
        }
        b'+' | b'-' => {
            field(index + 1..index + 3, 0, 23)?;
            byte_at(index + 3, b':')?;
            field(index + 4..index + 6, 0, 59)?;
            if bytes.len() != index + 6 {
                return None;
            }
        }
        _ => return None,
    }
    Some((ts.get(11..19)?, ts.get(19..)?))
}

fn short(sha: &str) -> String {
    sha.chars().take(10).collect()
}
