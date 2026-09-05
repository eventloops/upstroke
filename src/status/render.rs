//! The settled view and the one-line event descriptions (DESIGN.md §15).
//!
//! The half of `status` that touches nothing. It takes a `RunStatus` the parent
//! has already folded out of the log and returns a `String`, and it turns one
//! `Event` into one line for `--follow`. What is left in the parent is
//! everything that does reach the world — reading the log, probing the lock,
//! and streaming to a sink — so the two halves have one reason to change each
//! (CODING_STANDARDS.md §3).
//!
//! Splitting the rendering out did not change what it renders: the parent's
//! `render` and `describe` are the public surface and delegate here, and this
//! module is private.
//!
//! # What a `--follow` line promises
//!
//! Each line is a contract with an operator, not a debug aid: it says what
//! happened, to which task, and what the engine decided next. A failure names
//! its reason and the transition recorded with it; a decline names its halt
//! policy; a halted run names the task it halted at. Two things hold for every
//! line, whatever the log carries. It is **one line** — every field printed
//! here is on-disk data (CODING_STANDARDS.md §8), and a failure reason quotes
//! an agent's stderr, so the assembled text passes through [`one_line`] before
//! it is returned. And it is **exhaustive** — the `match` over `EventBody` has
//! no wildcard arm, so a variant this module does not know is a build error
//! rather than an event that renders as nothing.
//!
//! The line is product surface and its contract is `DESIGN.md` §18 (the CLI
//! surface), which this module implements; a change to what a line says is a
//! change to that section in the same pull request (CODING_STANDARDS.md §13).
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

// The remaining `write!` calls append suffixes through String's fmt::Write
// implementation, which cannot return Err. Their discarded results are Ok (§7).
use std::fmt::Write as _;

use super::RunStatus;
use crate::events::{
    AttemptRecord, AttemptTransition, Event, EventBody, LadderEscalated, LadderRetry,
    ReviewPassOutcome, RunOutcome, TaskDeferred, TaskFailed,
};
use crate::ir::Answer;
use crate::util::terminal::{TerminalLines, one_line};

/// The settled view, assembled: the report and its ledger, then the trailing
/// lines that say whether it is still moving and what it is waiting for.
///
/// The `state:` line is one of four readings of two facts — whether anything
/// holds the run's lock (`held`) and whether the log records a finish — which
/// the parent folds into `running` (held and unfinished) and `interrupted_run`
/// (unheld and unfinished). Held and finished is a claim on an ended run, said
/// as such; unheld and finished adds nothing to the outcome the report has
/// already printed, so it says nothing.
pub(super) fn render(status: &RunStatus) -> String {
    let report = status.report();
    let mut rendered = report.render();
    rendered.push_str(&report.render_ledger());
    let mut out = TerminalLines::default();

    // Liveness first among the trailing lines, because it decides whether any
    // of the above is still moving.
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
        // Finished, and somebody has claimed it anyway — a `resume` between
        // taking the lock and writing `run_resumed`. The outcome above is still
        // this run's outcome; it may just not be the last word for long.
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

/// One line per event: the time of day out of the record's own timestamp,
/// with the zone the record wrote (`Z` for everything this engine writes), then
/// the body.
///
/// The time is the record's, not the reader's: a `--follow` at 16:03 local in
/// UTC+2 shows `14:03:07Z`, and the suffix is what says so. A timestamp not in
/// the calendar, clock, and suffix form accepted by [`clock_of`] is printed
/// whole. Leap-second values retain their date too, since abbreviating one
/// would hide information needed to check it against the leap-second schedule.
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
            // Recorded so that someone reading the run tomorrow can see that
            // work was thrown away; a follower reading it today deserves the
            // same.
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
        // The outcome, then each decision the settlement carries, in the
        // order the engine made them. Each half of the settlement renders on
        // its own, so no pairing of transition and parking is a shape this
        // arm has to know about: a parked escalation reads "escalating past
        // …; parked on question …", and a pairing no writer produces today
        // still prints both facts rather than dropping one.
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
        // The legacy standalone forms of the decisions above, spelt by the
        // same helpers so the two wire shapes cannot drift apart.
        EventBody::LadderRetry { task, data, .. } => format!("{task}: {}", describe_retry(data)),
        EventBody::LadderEscalated { task, data, .. } => {
            format!("{task}: {}", describe_escalation(data))
        }
        EventBody::TaskDeferred { task, data } => {
            format!("{task}: {}", describe_deferral(data))
        }
        // Integer milliseconds preserve the wire's precision even beyond
        // f64's exact range. A public caller's submillisecond remainder is
        // truncated, matching the persisted format's precision.
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
        // Three answers, three lines. A decline carries the halt policy
        // frozen with it, and that is what this line reports — the policy, not
        // a transition: the task failure a decline causes is its own later
        // event (`task_failed`, which `resume` appends for a log that stopped
        // before it), the answer names no task, and a question may park more
        // than one. A question no channel could reach a person with was not
        // answered at all.
        EventBody::QuestionAnswered { data } => match &data.answer {
            Answer::Answered { .. } => format!("{} answered via {}", data.question, data.via),
            Answer::Declined => format!(
                "{} declined via {}; the halt policy frozen with it {}",
                data.question,
                data.via,
                match data.decline_halts_run {
                    Some(true) => "says the run halts",
                    Some(false) => "says the run continues",
                    // Only a log older than schema 3, which requires the
                    // policy on every decline.
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
            // Spelt here rather than through `Debug`: a derived `Debug` is a
            // Rust identifier, not a contract, and a halted run's line has to
            // name the task it halted at, which is what the operator acts on.
            let outcome = match data.outcome {
                RunOutcome::Complete => "complete",
                RunOutcome::Parked => "parked",
                RunOutcome::Halted => "halted",
                RunOutcome::BudgetExceeded => "stopped at its budget",
            };
            // The two fields are read together: a halt names its task or
            // says the record did not, and a task named on a run that did not
            // halt is shown as the oddity it is rather than as a halt.
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

/// What one settled attempt's record says happened.
///
/// "Passed" is the record's own claim of success — no failure and every
/// review pass passed, the facts [`AttemptRecord::is_successful`] reads, in
/// the same order — and not `failure.is_none()`. A review that rejected the
/// code or never reached a verdict is named by pass and model whether or not
/// the engine also recorded a failure for it: in production it always does
/// (`engine::attempt::evaluate_review` writes a `ReviewFailed` or
/// `ReviewUnavailable` failure beside the pass's outcome), so the line a run
/// produces is `failed — review failed: …; review \`review\` (model)
/// rejected it`. A record carrying the outcome and no failure is rendered as
/// it reads, "was not approved". Schema-3 validation refuses this inconsistent
/// shape, and schema-4 success checks reject it through `is_successful`.
/// Public describe renders the supplied event without validating a log.
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

/// The ladder decision one failed attempt settled with.
fn describe_transition(transition: &AttemptTransition) -> String {
    match transition {
        AttemptTransition::Retry(data) => describe_retry(data),
        AttemptTransition::Escalate(data) => describe_escalation(data),
        AttemptTransition::Defer(data) => describe_deferral(data),
        AttemptTransition::Fail(data) => describe_task_failure(data),
    }
}

/// The terminal decision, in the one spelling both wire shapes use, carrying
/// the transition's own reason: beside an attempt record it repeats the
/// attempt's reason when the coordinator copied it, and shows the difference
/// when a log carries two, which schema-3 validation admits (it requires the
/// kinds to agree and says nothing about the reasons). The `Debug` of the kind
/// is the log's `snake_case` spelt as a Rust identifier; a `Display` on
/// `FailureKind` belongs to `src/ladder.rs` (SWEEP-RENDER-011).
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

/// What a task's terminal failure means for the rest of the run, which is the
/// fact an operator watching one acts on.
fn halt_suffix(halts_run: bool) -> &'static str {
    if halts_run {
        "; the run halts"
    } else {
        "; the run continues"
    }
}

/// The clock and suffix of a calendar-valid RFC 3339 timestamp with seconds
/// in `00..=59`. Other text, including leap-second values, stays whole.
///
/// Month lengths and Gregorian leap years keep malformed dates visible.
/// Fractions need digits, and a zone is required with no trailing text.
/// This is an abbreviation rule, not event validation: it does not consult
/// the historical leap-second schedule, so those values keep their date.
/// Every Option propagation below selects the whole-timestamp fallback,
/// including the checked slices. Absence never becomes an error (§7).
fn clock_of(ts: &str) -> Option<(&str, &str)> {
    let bytes = ts.as_bytes();
    let field = |range: std::ops::Range<usize>, low: u32, high: u32| -> Option<u32> {
        let digits = bytes.get(range)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        // Every call below requests two or four digits, so the accumulated
        // value is at most 9999 and both arithmetic operations fit u32.
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
    // The suffix: an optional fraction, then `Z` or a signed `HH:MM` offset,
    // and nothing after it.
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
    // Every byte checked above is ASCII, so both boundaries are char
    // boundaries and the two `get`s below cannot fail.
    Some((ts.get(11..19)?, ts.get(19..)?))
}

fn short(sha: &str) -> String {
    sha.chars().take(10).collect()
}
