//! `tactus status` — the run, folded out of its own log (DESIGN.md §15).
//!
//! Status is a pure read: it opens no branch, spawns no agent, and takes no
//! lock. Everything it shows is derived by replaying `events.jsonl` through
//! the same [`RunState::apply`](crate::events::RunState::apply) the engine
//! writes through, so a running engine and a watching operator are looking at
//! one computation rather than two that ought to agree.
//!
//! The plan comes from the run's own `plan.normalized.json` rather than from
//! the plan file on disk: §5 freezes a plan at run start, and status should
//! describe the run that happened even if the source plan has since moved on.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::engine::RunReport;
use crate::error::TactusError;
use crate::events::{self, Event, EventBody, LogTail, RunStarted, RunState};
use crate::interaction::Sleeper;
use crate::ir::Plan;
use crate::rundir::{self, RunPaths};

/// One run, as read back from disk.
pub struct RunStatus {
    pub run_id: String,
    pub paths: RunPaths,
    pub started: RunStarted,
    pub state: RunState,
    pub plan: Plan,
    /// Whether an engine is driving this run right now.
    pub running: bool,
    /// Attempts that were in flight when a previous process stopped.
    pub interrupted: u32,
    pub warnings: Vec<String>,
}

impl RunStatus {
    /// The same projection a run writes to `report.json`.
    pub fn report(&self) -> RunReport {
        RunReport::from_state(
            &self.started,
            &self.plan,
            &self.state,
            self.warnings.clone(),
        )
    }

    /// Whether this run stopped without recording that it had finished — the
    /// signature of a kill, a power loss, or an aborting error.
    pub fn interrupted_run(&self) -> bool {
        !self.running && self.state.finished.is_none()
    }
}

/// Load a run: the newest one, or any unambiguous id prefix.
pub fn load(repo_root: &Path, run_id: Option<&str>) -> Result<RunStatus, TactusError> {
    let run_id = match run_id {
        Some(wanted) => rundir::resolve_run_id(repo_root, wanted)?,
        None => rundir::latest_run(repo_root).ok_or_else(|| TactusError::Refused {
            message: format!(
                "no runs found under {} — nothing has run in this repository yet",
                rundir::runs_root(repo_root).display()
            ),
        })?,
    };
    let public = rundir::public_dir(repo_root, &run_id);
    let events_path = public.join("events.jsonl");

    let mut warnings = Vec::new();
    let events = events::read_all(&events_path, &mut warnings)?;
    let started = events::started_of(&events, &events_path)?.clone();
    let paths = RunPaths::from_parts(public.clone(), PathBuf::from(&started.private_dir));

    let plan_path = paths.plan_json();
    let plan_text = std::fs::read_to_string(&plan_path).map_err(|source| TactusError::Io {
        path: plan_path.clone(),
        source,
    })?;
    let plan: Plan = serde_json::from_str(&plan_text).map_err(|e| TactusError::Parse {
        message: format!("{}: {e}", plan_path.display()),
    })?;

    let task_ids = plan.tasks.iter().map(|task| task.id.to_string()).collect();
    let mut replayed = events::replay(events, task_ids, &events_path)?;
    // Settled in memory only: status is a pure read and must not write to a
    // run it is merely looking at. A resume records the same settlement as
    // events instead.
    let interrupted = replayed.state.settle_interrupted();
    let running = rundir::is_running(&paths);

    Ok(RunStatus {
        run_id,
        paths,
        started: replayed.started,
        state: replayed.state,
        plan,
        running,
        interrupted,
        warnings,
    })
}

/// The whole view: what happened, what it cost, and what it is waiting for.
pub fn render(status: &RunStatus) -> String {
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
             tactus resume {}",
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
    }

    let open = status.state.open_questions();
    if !open.is_empty() {
        let _ = writeln!(out, "waiting on {} answer(s):", open.len());
        for record in open {
            let _ = writeln!(out, "    tactus answer {}", record.question.id);
        }
    }
    let _ = writeln!(out, "transcripts: {}", status.paths.private.display());
    out
}

/// One human line per event, for `--follow`.
pub fn describe(event: &Event) -> String {
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
            ..
        } => match &data.failure {
            Some(failure) => format!("{task}: attempt {attempt} failed — {}", failure.reason),
            None => format!("{task}: attempt {attempt} passed"),
        },
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
            "{task}: asking {} — answer with `tactus answer {}`",
            data.question.kind, data.question.id
        ),
        EventBody::QuestionAnswered { data } => {
            format!("{} answered via {}", data.question, data.via)
        }
        EventBody::DesignDefect { data } => {
            format!("design defect recorded for {}", data.question)
        }
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

/// Stream a run's events, from the beginning and then as they arrive.
///
/// Starting from the beginning is deliberate: `--follow` on a run already in
/// progress should show how it got here, not drop the reader into the middle
/// of a story. Reads only whole lines, so a follower attached to a live engine
/// never sees half an event. Returns once the run records that it is done — or
/// after `max_idle_polls` with nothing new, so a follower attached to a run
/// whose engine has died gives up instead of waiting forever.
pub fn follow(
    status: &RunStatus,
    sleeper: &dyn Sleeper,
    poll: Duration,
    max_idle_polls: u32,
    out: &mut dyn std::io::Write,
) -> Result<(), TactusError> {
    let mut tail = LogTail::new(status.paths.events());
    let mut warnings = Vec::new();
    let mut idle = 0;
    loop {
        let events = tail.poll(&mut warnings)?;
        if events.is_empty() {
            idle += 1;
            if idle > max_idle_polls {
                return Ok(());
            }
            sleeper.sleep(poll);
            continue;
        }
        idle = 0;
        for event in &events {
            let _ = writeln!(out, "{}", describe(event));
            if matches!(event.body, EventBody::RunFinished { .. }) {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AttemptRecord, AttemptStarted, RunFinished, RunOutcome, TaskCommitted};
    use crate::ir::{Answer, Question, QuestionId, QuestionKind, TaskId};

    fn event(body: EventBody) -> Event {
        Event {
            ts: "2026-08-09T14:03:07Z".to_owned(),
            body,
        }
    }

    #[test]
    fn every_event_describes_itself_in_one_line() {
        let lines = [
            event(EventBody::AttemptStarted {
                task: "t1".to_owned(),
                attempt: 1,
                rung: 0,
                profile: "small-haiku".to_owned(),
                data: AttemptStarted {
                    tier: "small".to_owned(),
                    agent: "claude-code".to_owned(),
                    model: "claude-haiku-4-5".to_owned(),
                    resume_session: None,
                },
            }),
            event(EventBody::TaskCommitted {
                task: "t1".to_owned(),
                data: TaskCommitted {
                    sha: "0123456789abcdef".to_owned(),
                    message: "[tactus] t1: do it".to_owned(),
                },
            }),
            event(EventBody::RunFinished {
                data: RunFinished {
                    outcome: RunOutcome::Complete,
                    halted_at: None,
                    committed: 1,
                    parked: 0,
                },
            }),
        ];
        let rendered: Vec<String> = lines.iter().map(describe).collect();
        assert!(rendered[0].starts_with("14:03:07  "), "{:?}", rendered[0]);
        assert!(rendered[0].contains("t1: attempt 1 on small"));
        assert!(rendered[1].contains("committed 0123456789"));
        assert!(rendered[2].contains("run finished"));
        for line in &rendered {
            assert_eq!(line.lines().count(), 1, "one line per event: {line:?}");
        }
    }

    #[test]
    fn a_raised_question_tells_the_operator_the_command_to_run() {
        let line = describe(&event(EventBody::QuestionRaised {
            task: "t1".to_owned(),
            data: Box::new(events::QuestionRaised {
                question: Question {
                    id: QuestionId::from("q-01ABC"),
                    kind: QuestionKind::Unblock,
                    affected_tasks: vec![TaskId::from("t1")],
                    context: "every rung failed".to_owned(),
                    options: Vec::new(),
                },
            }),
        }));
        assert!(line.contains("tactus answer q-01ABC"), "{line}");
    }

    #[test]
    fn the_ledger_keeps_worker_and_review_spend_apart() {
        let report = RunReport {
            run_id: "01RUN".to_owned(),
            branch: "tactus/run-01RUN".to_owned(),
            gates: vec!["check".to_owned()],
            gates_from_config: true,
            warnings: Vec::new(),
            tasks: vec![crate::engine::TaskReport {
                id: "t1".to_owned(),
                title: "Do it".to_owned(),
                model: "claude-haiku-4-5".to_owned(),
                status: crate::engine::TaskRunStatus::Committed {
                    sha: "abc".to_owned(),
                },
                duration: Duration::from_secs(3),
                cost_usd: Some(0.01),
                review_model: Some("claude-opus-5".to_owned()),
                review_cost_usd: Some(0.05),
                session_id: None,
                attempts: vec![AttemptRecord {
                    attempt: 1,
                    tier: "small".to_owned(),
                    model: "claude-haiku-4-5".to_owned(),
                    resumed: false,
                    duration: Duration::from_secs(3),
                    cost_usd: Some(0.01),
                    review_model: None,
                    review_cost_usd: None,
                    session_id: None,
                    failure: None,
                }],
            }],
            halted_at: None,
            questions: Vec::new(),
            total_cost_usd: 0.06,
        };
        let ledger = report.render_ledger();
        assert!(ledger.contains("worker"), "{ledger}");
        assert!(ledger.contains("$0.0100"), "implementer's own spend");
        assert!(ledger.contains("$0.0500"), "reviewer's, kept apart");
        assert!(ledger.contains("$0.0600"), "and the total");
        assert!(
            ledger.contains("not connected"),
            "pool drain is honest about arriving with the capacity engine"
        );
    }

    #[test]
    fn an_unreported_cost_is_not_rendered_as_free() {
        let report = RunReport {
            run_id: "01RUN".to_owned(),
            branch: "b".to_owned(),
            gates: Vec::new(),
            gates_from_config: false,
            warnings: Vec::new(),
            tasks: vec![crate::engine::TaskReport {
                id: "t1".to_owned(),
                title: "Never ran".to_owned(),
                model: String::new(),
                status: crate::engine::TaskRunStatus::Skipped,
                duration: Duration::ZERO,
                cost_usd: None,
                review_model: None,
                review_cost_usd: None,
                session_id: None,
                attempts: Vec::new(),
            }],
            halted_at: None,
            questions: Vec::new(),
            total_cost_usd: 0.0,
        };
        let ledger = report.render_ledger();
        assert!(
            ledger.contains('—'),
            "unreported must not read as $0.0000: {ledger}"
        );
    }

    #[test]
    fn answers_and_defects_render_without_quoting_the_operator() {
        // The operator's words are an instruction to the agent, not something
        // status needs to echo into a terminal it does not control.
        let line = describe(&event(EventBody::QuestionAnswered {
            data: events::QuestionAnswered {
                question: QuestionId::from("q-1"),
                answer: Answer::Answered {
                    text: "\u{1b}[31mnot a control sequence\u{1b}[0m".to_owned(),
                },
                via: "answer-file".to_owned(),
            },
        }));
        assert!(
            !line.contains('\u{1b}'),
            "no escape codes reach the terminal"
        );
        assert!(line.contains("q-1 answered via answer-file"), "{line}");
    }
}
